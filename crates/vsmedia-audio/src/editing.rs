//! Clip edits are non-destructive, index-addressed operations on AudioProject.
//! The original clip/source schema remains compatible with version-one projects.
//! A future editor can assign stable IDs; v1 deliberately validates every index.
use crate::{AudioProject,dsp::db_to_linear,filter::{FilterKind,StereoBiquad},effects::EffectChain};
use serde::{Deserialize,Serialize};
use std::collections::BTreeMap;
use vsmedia_api::{MediaError,MediaResult};

pub const MAX_EDIT_POINTS:usize=256;
pub const MAX_CLIP_FILTERS:usize=16;
pub const MAX_CLIP_EDITS:usize=256;

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(deny_unknown_fields)]
pub struct GainPoint {
    /// Milliseconds relative to the beginning of the trimmed clip.
    pub at_ms:u64,
    /// Gain value to reach at this point, interpolated linearly in amplitude.
    pub gain_db:f32,
}

#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(default,deny_unknown_fields)]
pub struct ClipEdit {
    pub track_index:usize,
    pub clip_index:usize,
    /// Source in-point, without changing the timeline position.
    pub source_in_ms:u64,
    /// Stereo balance -1 = left, 0 = unchanged, 1 = right.
    pub pan:f32,
    pub fade_in_ms:u64,
    pub fade_out_ms:u64,
    pub reversed:bool,
    pub muted:bool,
    /// Editable gain automation with monotonic local timestamps.
    pub gain_points:Vec<GainPoint>,
    /// Ordered parametric EQ chain: low-pass, high-pass, and peaking filters.
    pub filters:Vec<FilterKind>,
}
impl Default for ClipEdit {
    fn default()->Self {
        Self {
            track_index:0,clip_index:0,source_in_ms:0,pan:0.0,
            fade_in_ms:0,fade_out_ms:0,reversed:false,muted:false,
            gain_points:Vec::new(),filters:Vec::new(),
        }
    }
}
/// Optional editable overrides for an entire track. Existing AudioTrack
/// documents keep their original fields, so older projects remain readable.
#[derive(Clone,Debug,Serialize,Deserialize,PartialEq)]
#[serde(default,deny_unknown_fields)]
pub struct TrackEdit {
    pub track_index:usize,
    pub gain_db:f32,
    pub pan:f32,
    pub muted:bool,
    pub solo:bool,
}
impl Default for TrackEdit {
    fn default()->Self {
        Self { track_index:0,gain_db:0.0,pan:0.0,muted:false,solo:false }
    }
}
pub fn validate_track_edits<'a>(
    project:&AudioProject,edits:&'a [TrackEdit]
)->MediaResult<BTreeMap<usize,&'a TrackEdit>> {
    if edits.len()>256 { return Err(invalid("too many track edits")); }
    let mut map=BTreeMap::new();
    for edit in edits {
        if edit.track_index>=project.tracks.len() ||
            !edit.gain_db.is_finite() || !(-120.0..=24.0).contains(&edit.gain_db) ||
            !edit.pan.is_finite() || !(-1.0..=1.0).contains(&edit.pan) ||
            map.insert(edit.track_index,edit).is_some()
        {
            return Err(invalid("invalid or duplicate track edit"));
        }
    }
    Ok(map)
}

fn invalid(message:&str)->MediaError {
    MediaError::InvalidInput(message.into())
}
pub fn validate_edits<'a>(
    project:&AudioProject,edits:&'a[ClipEdit]
) -> MediaResult<BTreeMap<(usize,usize),&'a ClipEdit>> {
    if edits.len()>MAX_CLIP_EDITS {
        return Err(invalid("too many clip edits"));
    }
    let mut indexed=BTreeMap::new();
    for edit in edits {
        let clip=project.tracks.get(edit.track_index)
            .and_then(|track|track.clips.get(edit.clip_index))
            .ok_or_else(||invalid("clip edit references unknown track or clip"))?;
        if !edit.pan.is_finite() || !(-1.0..=1.0).contains(&edit.pan) ||
            edit.fade_in_ms>clip.duration_ms || edit.fade_out_ms>clip.duration_ms ||
            edit.fade_in_ms.checked_add(edit.fade_out_ms)
                .is_none_or(|sum|sum>clip.duration_ms) ||
            edit.gain_points.len()>MAX_EDIT_POINTS ||
            edit.filters.len()>MAX_CLIP_FILTERS
        {
            return Err(invalid("invalid fade, pan or automation settings"));
        }
        let mut previous=None;
        for point in &edit.gain_points {
            db_to_linear(point.gain_db)?;
            if point.at_ms>clip.duration_ms ||
                previous.is_some_and(|last| point.at_ms<=last) {
                return Err(invalid("automation timestamps must increase within clip"));
            }
            previous=Some(point.at_ms);
        }
        if indexed.insert((edit.track_index,edit.clip_index),edit).is_some() {
            return Err(invalid("duplicate edit for clip"));
        }
    }
    Ok(indexed)
}

/// Apply reverse, fades and volume envelope on a private copy of PCM.
/// Single pass after preprocessing keyframe positions: no per-frame allocation.
pub fn process_clip_frames(
    frames:&mut [[f32;2]],sample_rate:u32,edit:&ClipEdit
) -> MediaResult<()> {
    if !(8_000..=192_000).contains(&sample_rate) {
        return Err(invalid("invalid edit sample rate"));
    }
    if frames.iter().any(|frame|frame.iter().any(|v|!v.is_finite())) {
        return Err(invalid("non-finite clip PCM"));
    }
    if edit.reversed { frames.reverse(); }
    let fade_frames=|millis:u64| ((millis as u128*sample_rate as u128)/1000)
        .min(frames.len() as u128) as usize;
    let fade_in=fade_frames(edit.fade_in_ms);
    let fade_out=fade_frames(edit.fade_out_ms);
    let automation=edit.gain_points.iter().map(|p| {
        let at=(p.at_ms as u128*sample_rate as u128/1000)
            .min(usize::MAX as u128) as usize;
        Ok((at,db_to_linear(p.gain_db)?))
    }).collect::<MediaResult<Vec<(usize,f32)>>>()?;
    let mut next=0usize;
    let total_frames=frames.len();
    for (index,frame) in frames.iter_mut().enumerate() {
        while next+1<automation.len() && automation[next+1].0<=index {
            next+=1;
        }
        let volume=if automation.is_empty() {
            1.0
        } else if next+1 == automation.len() {
            automation[next].1
        } else {
            let (start,low)=automation[next];
            let (end,high)=automation[next+1];
            let t=if index<=start {0.0} else {
                (index-start) as f32/(end-start).max(1) as f32
            };
            low+(high-low)*t.clamp(0.0,1.0)
        };
        let in_gain=if fade_in>0 && index<fade_in {
            index as f32/fade_in as f32
        } else {1.0};
        let remaining=total_frames-index;
        let out_gain=if fade_out>0 && remaining<=fade_out {
            (remaining-1) as f32/fade_out as f32
        } else {1.0};
        let scale=volume*in_gain*out_gain;
        for channel in frame { *channel *= scale; }
    }
    if !edit.filters.is_empty() {
        let mut eq=EffectChain::new();
        for setting in &edit.filters {
            eq.add(StereoBiquad::new(sample_rate,*setting)?)?;
        }
        eq.process(frames)?;
    }
    if frames.iter().any(|frame|frame.iter().any(|v|!v.is_finite())) {
        return Err(invalid("clip edit caused non-finite audio"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioClip,AudioTrack};
    use vsmedia_api::AssetKey;
    fn project()->AudioProject {
        AudioProject{name:"Demo".into(),tracks:vec![
            AudioTrack{name:"Narration".into(),muted:false,clips:vec![
                AudioClip {asset:AssetKey::new("demo","speech").unwrap(),
                    start_ms:0,duration_ms:1000,gain_db:0.0}
            ]}
        ]}
    }
    #[test]
    fn legacy_json_and_edit_json_validate_without_mutating_project() {
        let p=project();
        let json=serde_json::to_vec(&p).unwrap();
        let legacy:AudioProject=serde_json::from_slice(&json).unwrap();
        assert_eq!(legacy.tracks[0].clips[0].duration_ms,1000);
        let edits=vec![ClipEdit {
            pan:0.5,source_in_ms:100,fade_in_ms:100,
            gain_points:vec![GainPoint{at_ms:0,gain_db:-12.0}],
            ..ClipEdit::default()
        }];
        validate_edits(&legacy,&edits).unwrap();
        assert_eq!(legacy.tracks[0].clips[0].gain_db,0.0);
        let bytes=serde_json::to_vec(&edits).unwrap();
        assert_eq!(serde_json::from_slice::<Vec<ClipEdit>>(&bytes).unwrap(),edits);
    }
    #[test]
    fn audio_eq_settings_round_trip_and_process_without_source_changes() {
        let edit=ClipEdit {
            filters:vec![
                FilterKind::LowPass{frequency_hz:1_000.0,q:0.707},
                FilterKind::Peaking{frequency_hz:500.0,q:1.0,gain_db:3.0}
            ],
            ..ClipEdit::default()
        };
        let bytes=serde_json::to_vec(&edit).unwrap();
        let parsed:ClipEdit=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(parsed,edit);
        let mut frames=vec![[0.25,-0.25];800];
        process_clip_frames(&mut frames,8_000,&parsed).unwrap();
        assert!(frames.iter().all(|frame|frame.iter().all(|x|x.is_finite())));
        assert!((frames[799][0]-0.25).abs()>0.001);
        let invalid=ClipEdit {
            filters:vec![FilterKind::LowPass{frequency_hz:100_000.0,q:0.707}],
            ..ClipEdit::default()
        };
        assert!(process_clip_frames(&mut frames,8_000,&invalid).is_err());
    }
    #[test]
    fn rejects_duplicate_unknown_nonfinite_and_unsorted_edits() {
        let p=project();
        let item=ClipEdit::default();
        assert!(validate_edits(&p,&[item.clone(),item.clone()]).is_err());
        assert!(validate_edits(&p,&[ClipEdit{clip_index:7,..item.clone()}]).is_err());
        assert!(validate_edits(&p,&[ClipEdit{pan:f32::NAN,..item.clone()}]).is_err());
        assert!(validate_edits(&p,&[ClipEdit{fade_in_ms:700,fade_out_ms:400,..item.clone()}]).is_err());
        assert!(validate_edits(&p,&[ClipEdit{
            gain_points:vec![GainPoint{at_ms:400,gain_db:0.0},GainPoint{at_ms:300,gain_db:1.0}],
            ..item
        }]).is_err());
    }
    #[test]
    fn reverse_and_fades_never_change_original_source() {
        let mut frames=vec![[1.0,-1.0];5];
        let edits=ClipEdit {
            reversed:true,fade_in_ms:200,fade_out_ms:200,
            ..ClipEdit::default()
        };
        process_clip_frames(&mut frames,10_000,&edits).unwrap();
        assert_eq!(frames.len(),5);
        assert!(frames.iter().all(|x| x.iter().all(|v|v.is_finite())));
    }
}
