//! Non-destructive first-pass timeline render. Resolves audio assets by key and
//! delegates mixing to the shared StereoLayer mixer.
use std::collections::HashMap;
use vsmedia_api::{AssetKey, MediaError, MediaResult};
use crate::{
    AudioProject,
    codec::{StereoAudio, MAX_FRAMES},
    dsp::{db_to_linear, mix_stereo, StereoLayer},
    resample::resample_offline,
};
pub const MAX_RENDER_CLIPS: usize = 256;

/// Render active clips at the requested sample rate. The existing model defines
/// source in-point as 0: source offsets, automation and fades come next.
pub fn render_project(
    project:&AudioProject, sources:&HashMap<AssetKey, StereoAudio>, sample_rate:u32
) -> MediaResult<StereoAudio> {
    project.validate()?;
    if !(8_000..=192_000).contains(&sample_rate) {
        return Err(MediaError::InvalidInput("invalid project sample rate".into()));
    }
    let mut layers=Vec::new();
    let mut final_frame=0usize;
    for track in &project.tracks {
        if track.muted { continue; }
        for clip in &track.clips {
            if layers.len()>=MAX_RENDER_CLIPS {
                return Err(MediaError::InvalidInput("render exceeds clip quota".into()));
            }
            let source=sources.get(&clip.asset).ok_or_else(||
                MediaError::NotFound(clip.asset.qualified()))?;
            let resampled=resample_offline(source,sample_rate)?;
            let offset_ms=clip.start_ms as u128*sample_rate as u128/1000;
            let duration_ms=(clip.duration_ms as u128*sample_rate as u128).div_ceil(1000);
            let offset=usize::try_from(offset_ms)
                .map_err(|_|MediaError::InvalidInput("clip offset exceeds limit".into()))?;
            let length=usize::try_from(duration_ms)
                .map_err(|_|MediaError::InvalidInput("clip duration exceeds limit".into()))?
                .min(resampled.frames.len());
            let stop=offset.checked_add(length)
                .ok_or_else(||MediaError::InvalidInput("clip endpoint overflow".into()))?;
            if stop>MAX_FRAMES {
                return Err(MediaError::InvalidInput("render exceeds sample quota".into()));
            }
            if length==0 { continue; }
            final_frame=final_frame.max(stop);
            let gain=db_to_linear(clip.gain_db)?;
            layers.push(StereoLayer{
                start_frame:offset, gain, pan:0.0,
                frames:resampled.frames.into_iter().take(length).collect(),
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
    fn reports_missing_audio_and_bounds_timeline() {
        assert!(render_project(&project(),&HashMap::new(),48000).is_err());
        let mut project=project();
        project.tracks[0].clips[0].start_ms=u64::MAX-1000;
        assert!(render_project(&project,&HashMap::new(),48000).is_err());
    }
}
