//! Stable timeline element identities and reorder-aware editing commands.
use crate::{NativeDocument,NativePayload,VSAudio};
use serde::{Serialize,Deserialize};
use std::collections::BTreeSet;
use vsmedia_api::{MediaResult,MediaError};
use vsmedia_audio::{AudioProject,AudioClip,AudioTrack,editing::AudioEditCommand};

fn invalid(why:&str)->MediaError {MediaError::InvalidInput(why.into())}

#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackIdentity {pub id:String,pub clip_ids:Vec<String>}
#[derive(Clone,Debug,PartialEq,Eq,Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AudioIdentities {pub next_id:u64,pub tracks:Vec<TrackIdentity>}
impl AudioIdentities {
    pub fn from_legacy(project:&AudioProject)->MediaResult<Self> {
        let mut ids=Self{next_id:0,tracks:Vec::with_capacity(project.tracks.len())};
        for track in &project.tracks {
            let id=ids.new_id("t")?;
            let mut clips=Vec::with_capacity(track.clips.len());
            for _ in &track.clips {clips.push(ids.new_id("c")?);}
            ids.tracks.push(TrackIdentity{id,clip_ids:clips});
        }
        Ok(ids)
    }
    fn new_id(&mut self,kind:&str)->MediaResult<String> {
        let id=format!("{kind}-{}",self.next_id);
        self.next_id=self.next_id.checked_add(1)
            .ok_or_else(||invalid("identity counter overflow"))?;
        Ok(id)
    }
    pub fn validate(&self,project:&AudioProject)->MediaResult<()> {
        if self.tracks.len()!=project.tracks.len() {
            return Err(invalid("track identity count mismatch"));
        }
        let mut used=BTreeSet::new();
        for (track,identity) in project.tracks.iter().zip(&self.tracks) {
            if track.clips.len()!=identity.clip_ids.len() {
                return Err(invalid("clip identity count mismatch"));
            }
            for (value,kind) in std::iter::once((&identity.id,"t"))
                .chain(identity.clip_ids.iter().map(|value|(value,"c"))) {
                let number=value.strip_prefix(&format!("{kind}-"))
                    .and_then(|digits|digits.parse::<u64>().ok())
                    .ok_or_else(||invalid("invalid audio identity"))?;
                if *value!=format!("{kind}-{number}") ||
                    number>=self.next_id || !used.insert(value) {
                    return Err(invalid("duplicate or stale audio identity"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone,Debug,Serialize,Deserialize)]
#[serde(tag="operation",rename_all="snake_case")]
pub enum StudioCommand {
    Edit {command:AudioEditCommand},
    MoveTrack {from:usize,to:usize},
    MoveClip {track_index:usize,from:usize,to:usize},
    InsertTrack {index:usize,name:String},
    DeleteTrack {index:usize},
    InsertClip {track_index:usize,index:usize,clip:AudioClip},
    DeleteClip {track_index:usize,index:usize},
    RenameTrack {index:usize,name:String},
    ReplaceClip {track_index:usize,index:usize,clip:AudioClip},
}
fn relocated(index:usize,from:usize,to:usize)->usize {
    if index==from {to}
    else if from<to && index>from && index<=to {index-1}
    else if to<from && index>=to && index<from {index+1}
    else {index}
}
/// Edits only a candidate snapshot; validation before commit is mandatory.
pub fn apply_to_document(doc:&mut VSAudio,command:StudioCommand)->MediaResult<()> {
    if doc.identities.is_none() {
        doc.identities=Some(AudioIdentities::from_legacy(&doc.timeline)?);
    }
    let ids=doc.identities.as_mut().ok_or_else(||invalid("missing identities"))?;
    match command {
        StudioCommand::Edit{command}=>doc.apply_edit(command)?,
        StudioCommand::MoveTrack{from,to}=>{
            if from>=doc.timeline.tracks.len() || to>=doc.timeline.tracks.len() {
                return Err(invalid("track index out of range"));
            }
            let track=doc.timeline.tracks.remove(from);
            doc.timeline.tracks.insert(to,track);
            let identity=ids.tracks.remove(from);
            ids.tracks.insert(to,identity);
            for edit in &mut doc.track_edits {edit.track_index=relocated(edit.track_index,from,to);}
            for edit in &mut doc.clip_edits {edit.track_index=relocated(edit.track_index,from,to);}
        }
        StudioCommand::MoveClip{track_index,from,to}=>{
            let track=doc.timeline.tracks.get_mut(track_index).ok_or_else(||invalid("unknown track"))?;
            if from>=track.clips.len() || to>=track.clips.len() {
                return Err(invalid("clip index out of range"));
            }
            let clip=track.clips.remove(from);
            track.clips.insert(to,clip);
            let identity=ids.tracks[track_index].clip_ids.remove(from);
            ids.tracks[track_index].clip_ids.insert(to,identity);
            for edit in &mut doc.clip_edits {
                if edit.track_index==track_index {
                    edit.clip_index=relocated(edit.clip_index,from,to);
                }
            }
        }
        StudioCommand::InsertTrack{index,name}=>{
            if index>doc.timeline.tracks.len() {return Err(invalid("track insertion index out of range"));}
            let id=ids.new_id("t")?;
            doc.timeline.tracks.insert(index,AudioTrack{name,clips:vec![],muted:false});
            ids.tracks.insert(index,TrackIdentity{id,clip_ids:vec![]});
            for edit in &mut doc.track_edits {
                if edit.track_index>=index {edit.track_index+=1;}
            }
            for edit in &mut doc.clip_edits {
                if edit.track_index>=index {edit.track_index+=1;}
            }
        }
        StudioCommand::DeleteTrack{index}=>{
            if index>=doc.timeline.tracks.len() {return Err(invalid("unknown track"));}
            doc.timeline.tracks.remove(index);
            ids.tracks.remove(index);
            doc.track_edits.retain(|edit|edit.track_index!=index);
            doc.clip_edits.retain(|edit|edit.track_index!=index);
            for edit in &mut doc.track_edits {
                if edit.track_index>index {edit.track_index-=1;}
            }
            for edit in &mut doc.clip_edits {
                if edit.track_index>index {edit.track_index-=1;}
            }
        }
        StudioCommand::InsertClip{track_index,index,clip}=>{
            let track=doc.timeline.tracks.get_mut(track_index).ok_or_else(||invalid("unknown track"))?;
            if index>track.clips.len() {return Err(invalid("clip insertion index out of range"));}
            let id=ids.new_id("c")?;
            track.clips.insert(index,clip);
            ids.tracks[track_index].clip_ids.insert(index,id);
            for edit in &mut doc.clip_edits {
                if edit.track_index==track_index && edit.clip_index>=index {edit.clip_index+=1;}
            }
        }
        StudioCommand::DeleteClip{track_index,index}=>{
            let track=doc.timeline.tracks.get_mut(track_index).ok_or_else(||invalid("unknown track"))?;
            if index>=track.clips.len() {return Err(invalid("unknown clip"));}
            track.clips.remove(index);
            ids.tracks[track_index].clip_ids.remove(index);
            doc.clip_edits.retain(|edit|edit.track_index!=track_index || edit.clip_index!=index);
            for edit in &mut doc.clip_edits {
                if edit.track_index==track_index && edit.clip_index>index {edit.clip_index-=1;}
            }
        }
        StudioCommand::RenameTrack{index,name}=>{
            doc.timeline.tracks.get_mut(index).ok_or_else(||invalid("unknown track"))?.name=name;
        }
        StudioCommand::ReplaceClip{track_index,index,clip}=>{
            *doc.timeline.tracks.get_mut(track_index)
                .and_then(|track|track.clips.get_mut(index))
                .ok_or_else(||invalid("unknown clip"))?=clip;
        }
    }
    NativeDocument::new(NativePayload::Audio(doc.clone())).validate()
}
