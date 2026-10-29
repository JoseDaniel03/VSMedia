//! Non-destructive first-pass timeline render. Resolves audio assets by key and
//! delegates mixing to the shared StereoLayer mixer.
use std::collections::{hash_map::Entry, HashMap};
use vsmedia_api::{AssetKey, MediaError, MediaResult};
use crate::{
    AudioProject,
    codec::{StereoAudio, MAX_FRAMES},
    dsp::{db_to_linear, mix_stereo, StereoLayer},
    resample::resample_offline,
    editing::{ClipEdit,TrackEdit,validate_edits,validate_track_edits,process_clip_frames},
};
pub const MAX_RENDER_CLIPS: usize = 256;
const MAX_LAYER_FRAMES: usize = 10_000_000;

/// Render active clips at the requested sample rate. The existing model defines
/// source in-point as 0: source offsets, automation and fades come next.
pub fn render_project(
    project:&AudioProject, sources:&HashMap<AssetKey, StereoAudio>, sample_rate:u32
) -> MediaResult<StereoAudio> {
    render_project_edited(project, sources, sample_rate, &[])
}

/// Render with non-destructive clip edits. Legacy projects pass an empty list;
/// source audio is never modified, and resampling is still cached per asset.
pub fn render_project_edited(
    project:&AudioProject, sources:&HashMap<AssetKey, StereoAudio>,
    sample_rate:u32, edits:&[ClipEdit]
) -> MediaResult<StereoAudio> {
    render_project_advanced(project,sources,sample_rate,edits,&[])
}

/// Mixer with editor-defined controls per clip and per track.
/// The original render API remains unchanged for older projects.
pub fn render_project_advanced(
    project:&AudioProject, sources:&HashMap<AssetKey, StereoAudio>,
    sample_rate:u32, edits:&[ClipEdit], track_edits:&[TrackEdit]
) -> MediaResult<StereoAudio> {
    project.validate()?;
    let indexed_edits=validate_edits(project,edits)?;
    let indexed_tracks=validate_track_edits(project,track_edits)?;
    let has_solo=track_edits.iter().any(|e|e.solo);
    if !(8_000..=192_000).contains(&sample_rate) {
        return Err(MediaError::InvalidInput("invalid project sample rate".into()));
    }
    let mut layers=Vec::new();
    let mut final_frame=0usize;
    let mut layer_frames=0usize;
    // Cache resampled source PCM once for every distinct asset used by the timeline.
    let mut resampled_sources:HashMap<AssetKey,StereoAudio>=HashMap::new();
    for (track_id,track) in project.tracks.iter().enumerate() {
        let track_edit=indexed_tracks.get(&track_id).copied();
        if track.muted || track_edit.is_some_and(|e|e.muted) ||
            (has_solo && !track_edit.is_some_and(|e|e.solo)) {
            continue;
        }
        for (clip_id,clip) in track.clips.iter().enumerate() {
            let edit=indexed_edits.get(&(track_id,clip_id)).copied();
            if edit.is_some_and(|entry|entry.muted) {continue;}
            if layers.len()>=MAX_RENDER_CLIPS {
                return Err(MediaError::InvalidInput("render exceeds clip quota".into()));
            }
            let offset_ms=clip.start_ms as u128*sample_rate as u128/1000;
            let duration_ms=(clip.duration_ms as u128*sample_rate as u128).div_ceil(1000);
            let offset=usize::try_from(offset_ms)
                .map_err(|_|MediaError::InvalidInput("clip offset exceeds limit".into()))?;
            if offset>MAX_FRAMES {
                return Err(MediaError::InvalidInput("clip starts past render limit".into()));
            }
            let source=sources.get(&clip.asset).ok_or_else(||
                MediaError::NotFound(clip.asset.qualified()))?;
            let resampled=match resampled_sources.entry(clip.asset.clone()) {
                Entry::Occupied(entry)=>entry.into_mut(),
                Entry::Vacant(entry)=>entry.insert(resample_offline(source,sample_rate)?),
            };
            let source_start=edit.map_or(0,|e|e.source_in_ms);
            let source_offset=usize::try_from(
                (source_start as u128*sample_rate as u128)/1000
            ).map_err(|_|MediaError::InvalidInput("source inpoint exceeds limit".into()))?;
            if source_offset >= resampled.frames.len() {
                return Err(MediaError::InvalidInput("source inpoint is outside clip audio".into()));
            }
            let length=usize::try_from(duration_ms)
                .map_err(|_|MediaError::InvalidInput("clip duration exceeds limit".into()))?
                .min(resampled.frames.len()-source_offset);
            let stop=offset.checked_add(length)
                .ok_or_else(||MediaError::InvalidInput("clip endpoint overflow".into()))?;
            if stop>MAX_FRAMES {
                return Err(MediaError::InvalidInput("render exceeds sample quota".into()));
            }
            if length==0 { continue; }
            layer_frames=layer_frames.checked_add(length)
                .ok_or_else(||MediaError::InvalidInput("clip memory budget overflow".into()))?;
            if layer_frames>MAX_LAYER_FRAMES {
                return Err(MediaError::InvalidInput("combined clip memory budget exceeded".into()));
            }
            final_frame=final_frame.max(stop);
            let combined_gain=clip.gain_db+
                track_edit.map_or(0.0,|edit|edit.gain_db);
            let gain=db_to_linear(combined_gain)?;
            let mut frames=resampled.frames[source_offset..source_offset+length].to_vec();
            if let Some(edit)=edit {
                process_clip_frames(&mut frames,sample_rate,edit)?;
            }
            layers.push(StereoLayer{
                start_frame:offset, gain,
                pan:(edit.map_or(0.0,|e|e.pan)
                    +track_edit.map_or(0.0,|e|e.pan)).clamp(-1.0,1.0),
                frames,
            });
        }
    }
    let output=StereoAudio{
        sample_rate, frames:mix_stereo(&layers,final_frame)?
    };
    output.validate()?;
    Ok(output)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AudioTrack,AudioClip};
    fn project()->AudioProject {
        AudioProject{name:"dialogue".into(),tracks:vec![
            AudioTrack{name:"narrator".into(),muted:false,clips:vec![
                AudioClip {asset:AssetKey::new("demo","dialogue/hello").unwrap(),
                    start_ms:0,duration_ms:200,gain_db:0.0}
            ]},
            AudioTrack{name:"ambient".into(),muted:true,clips:vec![
                AudioClip {asset:AssetKey::new("demo","dialogue/muted").unwrap(),
                    start_ms:0,duration_ms:200,gain_db:0.0}
            ]},
        ]}
    }
    #[test]
    fn renders_timeline_and_skips_muted_tracks() {
        let key=AssetKey::new("demo","dialogue/hello").unwrap();
        let sources=HashMap::from([(key,StereoAudio{
            sample_rate:8000,frames:vec![[0.25,-0.25];1600]
        })]);
        let output=render_project(&project(),&sources,8000).unwrap();
        assert_eq!(output.frames.len(),1600);
        assert_eq!(output.frames[0],[0.25,-0.25]);
    }
    #[test]
    fn reuses_same_source_across_multiple_clip_positions() {
        let key=AssetKey::new("demo","dialogue/hello").unwrap();
        let mut project=project();
        project.tracks[0].clips.push(AudioClip {
            asset:key.clone(),start_ms:100,duration_ms:200,gain_db:0.0
        });
        let sources=HashMap::from([(key,StereoAudio {
            sample_rate:8000,frames:vec![[0.25,-0.25];1600]
        })]);
        let output=render_project(&project,&sources,8000).unwrap();
        assert_eq!(output.frames.len(),2400);
        assert!((output.frames[800][0]-0.5).abs()<0.0001);
    }
    #[test]
    fn edited_render_trims_source_and_applies_pan_and_fades_without_modifying_source() {
        let p=project();
        let key=AssetKey::new("demo","dialogue/hello").unwrap();
        let original=StereoAudio {sample_rate:8_000,
            frames:(0..1_600).map(|i|[i as f32 / 1600.0,0.5]).collect()};
        let sources=HashMap::from([(key,original.clone())]);
        let edit=ClipEdit {
            source_in_ms:50,pan:-1.0,fade_in_ms:50,fade_out_ms:50,
            ..ClipEdit::default()
        };
        let output=render_project_edited(&p,&sources,8_000,&[edit]).unwrap();
        assert_eq!(output.frames.len(),1_200);
        assert_eq!(output.frames[0],[0.0,0.0]);
        assert!((output.frames[400][0]-0.5).abs()<0.0001);
        assert_eq!(output.frames[400][1],0.0);
        assert_eq!(output.frames[1199],[0.0,0.0]);
        assert_eq!(sources.values().next().unwrap().frames, original.frames);
    }

    #[test]
    fn edited_render_supports_muting_and_reverse_without_changing_legacy_path() {
        let p=project();
        let key=AssetKey::new("demo","dialogue/hello").unwrap();
        let source=StereoAudio {sample_rate:8_000,frames:vec![[0.25,0.25];1600]};
        let sources=HashMap::from([(key,source)]);
        let mut edit=ClipEdit {muted:true,..ClipEdit::default()};
        assert!(render_project_edited(&p,&sources,8_000,&[edit.clone()])
            .unwrap().frames.is_empty());
        edit.muted=false;
        edit.reversed=true;
        let rendered=render_project_edited(&p,&sources,8_000,&[edit]).unwrap();
        assert_eq!(rendered.frames.len(),1600);
        assert_eq!(rendered.frames[0],[0.25,0.25]);
        assert_eq!(rendered.frames[1599],[0.25,0.25]);
        assert_eq!(render_project(&p,&sources,8_000).unwrap().frames,rendered.frames);
    }

    #[test]
    fn edited_track_solo_gain_and_pan_override_work_without_changing_clips() {
        let mut p=project();
        p.tracks.push(AudioTrack {
            name:"effects".into(),muted:false,
            clips:vec![AudioClip {
                asset:AssetKey::new("demo","effects/sting").unwrap(),
                start_ms:0,duration_ms:200,gain_db:0.0,
            }]
        });
        let key1=AssetKey::new("demo","dialogue/hello").unwrap();
        let key2=AssetKey::new("demo","effects/sting").unwrap();
        let sources=HashMap::from([
            (key1,StereoAudio{sample_rate:8_000,frames:vec![[0.5,0.5];1600]}),
            (key2,StereoAudio{sample_rate:8_000,frames:vec![[0.25,0.25];1600]}),
        ]);
        let track=TrackEdit {track_index:2,gain_db:-6.0,pan:1.0,solo:true,..TrackEdit::default()};
        let output=render_project_advanced(&p,&sources,8_000,&[],&[track]).unwrap();
        assert!(output.frames[200][0].abs()<1e-6);
        assert!((output.frames[200][1]-0.1253).abs()<0.002);
        assert_eq!(p.tracks[2].clips[0].gain_db,0.0);
        let invalid=TrackEdit {track_index:77,..TrackEdit::default()};
        assert!(render_project_advanced(&p,&sources,8_000,&[],&[invalid]).is_err());
    }
    #[test]
    fn reports_missing_audio_and_bounds_timeline() {
        assert!(render_project(&project(),&HashMap::new(),48000).is_err());
        let mut project=project();
        project.tracks[0].clips[0].start_ms=u64::MAX-1000;
        assert!(render_project(&project,&HashMap::new(),48000).is_err());
    }
}
