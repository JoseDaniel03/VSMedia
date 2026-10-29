//! Stable timeline element identities and reorder-aware editing commands.
use crate::{NativeDocument,NativePayload,VSAudio,MAX_DOCUMENT_BYTES};
use serde::{Serialize,Deserialize};
use std::collections::{BTreeSet,VecDeque};
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
    EditClipById {clip_id:String, edit:vsmedia_audio::editing::ClipEdit},
    EditTrackById {track_id:String, edit:vsmedia_audio::editing::TrackEdit},
    MoveTrack {from:usize,to:usize},
    MoveClip {track_index:usize,from:usize,to:usize},
    InsertTrack {index:usize,name:String},
    DeleteTrack {index:usize},
    InsertClip {track_index:usize,index:usize,clip:AudioClip},
    DeleteClip {track_index:usize,index:usize},
    RenameTrack {index:usize,name:String},
    ReplaceClip {track_index:usize,index:usize,clip:AudioClip},
}
fn find_track(ids:&AudioIdentities,track_id:&str)->MediaResult<usize> {
    ids.tracks.iter().position(|track|track.id==track_id)
        .ok_or_else(||invalid("track identifier no longer exists"))
}
fn find_clip(ids:&AudioIdentities,clip_id:&str)->MediaResult<(usize,usize)> {
    for (track_index,track) in ids.tracks.iter().enumerate() {
        if let Some(clip_index)=track.clip_ids.iter().position(|value|value==clip_id) {
            return Ok((track_index,clip_index));
        }
    }
    Err(invalid("clip identifier no longer exists"))
}
fn relocated(index:usize,from:usize,to:usize)->usize {
    if index==from {to}
    else if from<to && index>from && index<=to {index-1}
    else if to<from && index>=to && index<from {index+1}
    else {index}
}
/// Edits only a candidate snapshot; validation before commit is mandatory.
fn apply_to_document(doc:&mut VSAudio,command:StudioCommand)->MediaResult<()> {
    if doc.identities.is_none() {
        doc.identities=Some(AudioIdentities::from_legacy(&doc.timeline)?);
    }
    let ids=doc.identities.as_mut().ok_or_else(||invalid("missing identities"))?;
    match command {
        StudioCommand::Edit{command}=>doc.apply_edit(command)?,
        StudioCommand::EditClipById{clip_id,mut edit}=>{
            let (track_index,clip_index)=find_clip(ids,&clip_id)?;
            edit.track_index=track_index;
            edit.clip_index=clip_index;
            doc.apply_edit(AudioEditCommand::UpsertClip{edit})?;
        }
        StudioCommand::EditTrackById{track_id,mut edit}=>{
            edit.track_index=find_track(ids,&track_id)?;
            doc.apply_edit(AudioEditCommand::UpsertTrack{edit})?;
        }
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

pub const MAX_UNDO_STEPS:usize=64;
pub const MAX_HISTORY_BYTES:usize=16 * 1024 * 1024;

/// In-memory editing session. Only manifests are copied (never decoded PCM).
/// Undo and redo restore the timeline and edits, including persistent IDs.
pub struct AudioStudio {
    document:VSAudio,
    undo:VecDeque<VSAudio>,
    redo:VecDeque<VSAudio>,
}
fn size(document:&VSAudio)->MediaResult<usize> {
    serde_json::to_vec(document).map(|data|data.len())
        .map_err(|e|invalid(&format!("invalid audio document: {e}")))
}
fn trim_history(undo:&mut VecDeque<VSAudio>,redo:&mut VecDeque<VSAudio>) {
    // Limit total history memory across both stacks, not 16 MiB per stack.
    let bytes=|stack:&VecDeque<VSAudio>| stack.iter()
        .map(|item|size(item).unwrap_or(MAX_DOCUMENT_BYTES+1))
        .fold(0usize,usize::saturating_add);
    while undo.len()+redo.len()>MAX_UNDO_STEPS ||
        bytes(undo).saturating_add(bytes(redo))>MAX_HISTORY_BYTES
    {
        if undo.pop_front().is_none() {redo.pop_front();}
    }
}
impl AudioStudio {
    pub fn open(document:VSAudio)->MediaResult<Self> {
        NativeDocument::new(NativePayload::Audio(document.clone())).validate()?;
        if size(&document)?>MAX_DOCUMENT_BYTES {
            return Err(invalid("audio manifest exceeds 2 MiB"));
        }
        Ok(Self{document,undo:VecDeque::new(),redo:VecDeque::new()})
    }
    pub fn document(&self)->&VSAudio {&self.document}
    /// Exposes IDs for a UI without altering old manifests until first edit.
    pub fn identities(&self)->MediaResult<AudioIdentities> {
        match &self.document.identities {
            Some(ids)=>Ok(ids.clone()),
            None=>AudioIdentities::from_legacy(&self.document.timeline),
        }
    }
    pub fn can_undo(&self)->bool {!self.undo.is_empty()}
    pub fn can_redo(&self)->bool {!self.redo.is_empty()}
    pub fn undo_depth(&self)->usize {self.undo.len()}
    pub fn apply(&mut self,command:StudioCommand)->MediaResult<()> {
        let mut candidate=self.document.clone();
        apply_to_document(&mut candidate,command)?;
        if size(&candidate)?>MAX_DOCUMENT_BYTES {
            return Err(invalid("audio manifest exceeds 2 MiB"));
        }
        self.undo.push_back(std::mem::replace(&mut self.document,candidate));
        trim_history(&mut self.undo,&mut self.redo);
        self.redo.clear();
        Ok(())
    }
    pub fn undo(&mut self)->bool {
        if let Some(previous)=self.undo.pop_back() {
            self.redo.push_back(std::mem::replace(&mut self.document,previous));
            trim_history(&mut self.undo,&mut self.redo);
            true
        } else {false}
    }
    pub fn redo(&mut self)->bool {
        if let Some(next)=self.redo.pop_back() {
            self.undo.push_back(std::mem::replace(&mut self.document,next));
            trim(&mut self.undo);
            true
        } else {false}
    }
    pub fn into_document(self)->VSAudio {self.document}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AssetRef,MediaCodec};
    use vsmedia_api::AssetKey;
    use vsmedia_audio::editing::{ClipEdit,TrackEdit};
    fn sample()->VSAudio {
        let asset=AssetKey::new("demo","audio/source").unwrap();
        let clip=|start|AudioClip{
            asset:asset.clone(),start_ms:start,duration_ms:1000,gain_db:0.0
        };
        VSAudio {
            timeline:AudioProject{name:"Studio".into(),tracks:vec![
                AudioTrack{name:"Voice".into(),clips:vec![clip(0),clip(1000)],muted:false},
                AudioTrack{name:"Music".into(),clips:vec![clip(250)],muted:false}
            ]},
            sources:vec![AssetRef{key:asset,sha256:"a".repeat(64),codec:MediaCodec::Wav}],
            clip_edits:vec![ClipEdit{track_index:0,clip_index:1,pan:0.7,..ClipEdit::default()}],
            track_edits:vec![TrackEdit{track_index:1,gain_db:-6.0,..TrackEdit::default()}],
            identities:None,
        }
    }
    #[test]
    fn old_documents_gain_persistent_identifiers_only_when_edited() {
        let mut studio=AudioStudio::open(sample()).unwrap();
        assert!(studio.document().identities.is_none());
        studio.apply(StudioCommand::MoveTrack{from:0,to:1}).unwrap();
        let ids=studio.document().identities.as_ref().unwrap();
        assert_eq!(ids.tracks[0].id,"t-3");
        assert_eq!(ids.tracks[1].id,"t-0");
        assert_eq!(studio.document().clip_edits[0].track_index,1);
        assert_eq!(studio.document().track_edits[0].track_index,0);
        let old_clip_id=ids.tracks[1].clip_ids[1].clone();
        studio.apply(StudioCommand::MoveClip{track_index:1,from:1,to:0}).unwrap();
        assert_eq!(studio.document().identities.as_ref().unwrap().tracks[1].clip_ids[0],old_clip_id);
        assert_eq!(studio.document().clip_edits[0].clip_index,0);
    }
    #[test]
    fn insert_delete_and_undo_redo_restore_timeline_and_edits() {
        let mut studio=AudioStudio::open(sample()).unwrap();
        studio.apply(StudioCommand::InsertTrack{index:1,name:"FX".into()}).unwrap();
        assert_eq!(studio.document().track_edits[0].track_index,2);
        studio.apply(StudioCommand::DeleteClip{track_index:0,index:0}).unwrap();
        assert_eq!(studio.document().clip_edits[0].clip_index,0);
        assert!(studio.undo());
        assert_eq!(studio.document().timeline.tracks[0].clips.len(),2);
        assert!(studio.undo());
        assert_eq!(studio.document().timeline.tracks.len(),2);
        assert!(studio.document().identities.is_none());
        assert!(!studio.undo());
        assert!(studio.redo());
        assert_eq!(studio.document().timeline.tracks.len(),3);
        assert!(studio.redo());
        assert_eq!(studio.document().timeline.tracks[0].clips.len(),1);
    }
    #[test]
    fn invalid_command_keeps_document_and_redo_unchanged() {
        let mut studio=AudioStudio::open(sample()).unwrap();
        studio.apply(StudioCommand::RenameTrack{index:0,name:"First".into()}).unwrap();
        studio.undo();
        let before=serde_json::to_vec(studio.document()).unwrap();
        assert!(studio.apply(StudioCommand::RenameTrack{index:0,name:"".into()}).is_err());
        assert_eq!(serde_json::to_vec(studio.document()).unwrap(),before);
        assert!(studio.can_redo());
        assert!(studio.redo());
        assert_eq!(studio.document().timeline.tracks[0].name,"First");
        studio.apply(StudioCommand::RenameTrack{index:0,name:"Second".into()}).unwrap();
        assert!(!studio.can_redo());
    }
    #[test]
    fn id_addressed_edits_follow_the_same_clip_after_reorder() {
        let mut studio=AudioStudio::open(sample()).unwrap();
        let voice_clip=studio.identities().unwrap().tracks[0].clip_ids[1].clone();
        let music_track=studio.identities().unwrap().tracks[1].id.clone();
        studio.apply(StudioCommand::MoveTrack{from:0,to:1}).unwrap();
        studio.apply(StudioCommand::MoveClip{track_index:1,from:1,to:0}).unwrap();
        studio.apply(StudioCommand::EditClipById{
            clip_id:voice_clip.clone(),
            edit:ClipEdit {pan:-0.7, ..ClipEdit::default()}
        }).unwrap();
        assert_eq!(studio.document().clip_edits.len(),1);
        assert_eq!(studio.document().clip_edits[0].track_index,1);
        assert_eq!(studio.document().clip_edits[0].clip_index,0);
        assert_eq!(studio.document().clip_edits[0].pan,-0.7);
        studio.apply(StudioCommand::EditTrackById {
            track_id:music_track,edit:TrackEdit {gain_db:-18.0,..TrackEdit::default()}
        }).unwrap();
        assert_eq!(studio.document().track_edits[0].track_index,0);
        assert_eq!(studio.document().track_edits[0].gain_db,-18.0);
        let revision=serde_json::to_vec(studio.document()).unwrap();
        assert!(studio.apply(StudioCommand::EditClipById {
            clip_id:"c-999999".into(),edit:ClipEdit::default()
        }).is_err());
        assert_eq!(revision,serde_json::to_vec(studio.document()).unwrap());
    }
    #[test]
    fn insertion_shifts_edits_and_json_round_trips_stable_ids() {
        let original=sample();
        assert!(serde_json::to_value(&original).unwrap().get("identities").is_none());
        let mut editor=AudioStudio::open(original).unwrap();
        editor.apply(StudioCommand::InsertClip{track_index:0,index:0,clip:AudioClip{
            asset:AssetKey::new("demo","audio/source").unwrap(),
            start_ms:700,duration_ms:300,gain_db:0.0,
        }}).unwrap();
        assert_eq!(editor.document().clip_edits[0].clip_index,2);
        let saved=serde_json::to_vec(editor.document()).unwrap();
        let loaded:VSAudio=serde_json::from_slice(&saved).unwrap();
        assert_eq!(loaded.identities,editor.document().identities);
        let mut ids=loaded.identities.unwrap();
        ids.tracks[0].clip_ids[0]=ids.tracks[0].id.clone();
        assert!(ids.validate(&editor.document().timeline).is_err());
    }
    #[test]
    fn undo_is_bounded_and_does_not_duplicate_pcm() {
        let mut studio=AudioStudio::open(sample()).unwrap();
        for i in 0..(MAX_UNDO_STEPS+6) {
            studio.apply(StudioCommand::RenameTrack{
                index:0,name:format!("Name {i}")
            }).unwrap();
        }
        assert_eq!(studio.undo_depth(),MAX_UNDO_STEPS);
        for _ in 0..MAX_UNDO_STEPS {assert!(studio.undo());}
        assert!(!studio.undo());
        assert!(studio.can_redo());
    }
}
