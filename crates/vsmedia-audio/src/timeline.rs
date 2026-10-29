//! Non-destructive first-pass timeline render. Resolves audio assets by key and
//! delegates mixing to the shared StereoLayer mixer.
use std::collections::{hash_map::Entry, HashMap};
use vsmedia_api::{AssetKey, MediaError, MediaResult};
use crate::{
    AudioProject,
    codec::{StereoAudio, MAX_FRAMES},
    dsp::{db_to_linear, mix_stereo, StereoLayer},
    resample::resample_offline,
    editing::{ClipEdit,validate_edits,process_clip_frames},
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
    project.validate()?;
    let indexed_edits=validate_edits(project,edits)?;
    if !(8_000..=192_000).contains(&sample_rate) {
        return Err(MediaError::InvalidInput("invalid project sample rate".into()));
    }
    let mut layers=Vec::new();
    let mut final_frame=0usize;
    let mut layer_frames=0usize;
    // Cache resampled source PCM once for every distinct asset used by the timeline.
    let mut resampled_sources:HashMap<AssetKey,StereoAudio>=HashMap::new();
    for (track_id,track) in project.tracks.iter().enumerate() {
        if track.muted { continue; }
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
            let gain=db_to_linear(clip.gain_db)?;
            let mut frames=resampled.frames[source_offset..source_offset+length].to_vec();
            if let Some(edit)=edit {
                process_clip_frames(&mut frames,sample_rate,edit)?;
            }
            layers.push(StereoLayer{
                start_frame:offset, gain, pan:edit.map_or(0.0,|e|e.pan),
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
    fn reports_missing_audio_and_bounds_timeline() {
        assert!(render_project(&project(),&HashMap::new(),48000).is_err());
        let mut project=project();
        project.tracks[0].clips[0].start_ms=u64::MAX-1000;
        assert!(render_project(&project,&HashMap::new(),48000).is_err());
    }
}
