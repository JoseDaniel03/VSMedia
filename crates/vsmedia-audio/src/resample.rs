//! Fixed-rate offline resampling using Rubato's FFT anti-aliasing resampler.
//! Intentionally separate from time-stretching/pitch-preserving transformations.
use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use vsmedia_api::{MediaError, MediaResult};
use crate::codec::{StereoAudio, MAX_FRAMES};

/// Resample entire audio with Rubato 5; input and output formats remain f32 stereo.
/// Use the same resampler instance per rate-pair for repeated offline jobs in future.
pub fn resample_offline(audio: &StereoAudio, target_rate: u32) -> MediaResult<StereoAudio> {
    audio.validate()?;
    if !(8_000..=192_000).contains(&target_rate) {
        return Err(MediaError::InvalidInput("unsupported output sample rate".into()));
    }
    if target_rate == audio.sample_rate { return Ok(audio.clone()); }
    if audio.frames.is_empty() { return Ok(StereoAudio{sample_rate:target_rate,frames:Vec::new()}); }
    let estimated = (audio.frames.len() as u128 * target_rate as u128)
        .div_ceil(audio.sample_rate as u128) as usize;
    if estimated > MAX_FRAMES {
        return Err(MediaError::InvalidInput("resampled output would exceed frame quota".into()));
    }
    let mut samples=Vec::with_capacity(audio.frames.len()*2);
    for frame in &audio.frames {
        samples.extend_from_slice(frame);
    }
    let input=InterleavedSlice::new(samples.as_slice(),2,audio.frames.len())
        .map_err(|e| MediaError::InvalidInput(format!("invalid adapter: {e}")))?;
    let mut resampler=Fft::<f32>::new(
        audio.sample_rate as usize,target_rate as usize,1024,2,FixedSync::Both
    ).map_err(|e| MediaError::InvalidInput(format!("resampler construction: {e}")))?;
    let result=resampler.process_all(&input,audio.frames.len(),None)
        .map_err(|e| MediaError::Io(format!("resampling failed: {e}")))?;
    let raw=result.take_data();
    if raw.len()%2!=0 || raw.len()/2>MAX_FRAMES {
        return Err(MediaError::Io("resampler returned invalid output length".into()));
    }
    let output=StereoAudio {
        sample_rate:target_rate,
        frames:raw.chunks_exact(2).map(|p|[p[0],p[1]]).collect(),
    };
    output.validate()?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resamples_44100_to_48000_and_preserves_channels() {
        let rate=44100;
        let frames=(0..44100).map(|i| {
            let t=i as f32 / rate as f32;
            [(t*std::f32::consts::TAU*440.0).sin()*0.5,0.0]
        }).collect();
        let original=StereoAudio{sample_rate:rate,frames};
        let result=resample_offline(&original,48000).unwrap();
        assert!((result.frames.len() as i64-48000).abs()<=2);
        assert!(result.frames.iter().all(|p|p[1].abs()<1e-5));
        assert!(result.frames.iter().any(|p|p[0].abs()>0.4));
        assert_eq!(original.frames.len(),44100);
    }
    #[test]
    fn no_op_preserves_same_rate() {
        let a=StereoAudio{sample_rate:48000,frames:vec![[0.5,-0.2];100]};
        assert_eq!(resample_offline(&a,48000).unwrap().frames,a.frames);
        assert!(resample_offline(&a,0).is_err());
    }
}
