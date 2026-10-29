//! In-memory, non-destructive stereo mixing primitives. Source samples stay untouched.
use vsmedia_api::{MediaError, MediaResult};

const MAX_FRAMES: usize = 5_000_000;
const MAX_LAYERS: usize = 256;

#[derive(Clone, Debug)]
pub struct StereoLayer {
    pub start_frame: usize,
    /// Linear amplitude (not dB), 0.0..=16.0.
    pub gain: f32,
    /// Stereo balance from -1.0 (left) to +1.0 (right).
    pub pan: f32,
    /// Frames are interleaved left/right pairs at a common sample rate.
    pub frames: Vec<[f32; 2]>,
}

/// Mixed floating-point output retains headroom; do mastering and clipping only on export.
pub fn mix_stereo(layers: &[StereoLayer], output_frames: usize) -> MediaResult<Vec<[f32; 2]>> {
    if output_frames > MAX_FRAMES || layers.len() > MAX_LAYERS {
        return Err(MediaError::InvalidInput("stereo mix exceeds limits".into()));
    }
    let mut result = vec![[0.0f32; 2]; output_frames];
    for layer in layers {
        if !layer.gain.is_finite() || !(0.0..=16.0).contains(&layer.gain)
            || !layer.pan.is_finite() || !(-1.0..=1.0).contains(&layer.pan)
            || layer.start_frame.checked_add(layer.frames.len())
                .is_none_or(|end| end > output_frames)
            || layer.frames.iter().any(|frame| {
                frame.iter().any(|sample| !sample.is_finite() || sample.abs() > 32.0)
            })
        {
            return Err(MediaError::InvalidInput("invalid stereo layer".into()));
        }
        let left_gain = (1.0 - layer.pan).min(1.0) * layer.gain;
        let right_gain = (1.0 + layer.pan).min(1.0) * layer.gain;
        for (i, frame) in layer.frames.iter().enumerate() {
            result[layer.start_frame + i][0] += frame[0] * left_gain;
            result[layer.start_frame + i][1] += frame[1] * right_gain;
        }
    }
    Ok(result)
}

/// Fade at sample boundaries. Works on a single PCM channel; caller decides channel routing.
pub fn fade_linear(samples: &mut [f32], fade_in: usize, fade_out: usize) -> MediaResult<()> {
    if samples.iter().any(|s| !s.is_finite()) {
        return Err(MediaError::InvalidInput("non-finite PCM sample".into()));
    }
    let len = samples.len();
    let fade_in = fade_in.min(len);
    let fade_out = fade_out.min(len);
    for (index, sample) in samples.iter_mut().enumerate() {
        let mut gain = 1.0;
        if fade_in > 0 && index < fade_in {
            gain *= index as f32 / fade_in as f32;
        }
        if fade_out > 0 && len - index <= fade_out {
            gain *= (len - index - 1) as f32 / fade_out as f32;
        }
        *sample *= gain;
    }
    Ok(())
}

pub fn db_to_linear(db: f32) -> MediaResult<f32> {
    if !db.is_finite() || !(-120.0..=24.0).contains(&db) {
        return Err(MediaError::InvalidInput("gain outside -120..24 dB".into()));
    }
    Ok(10.0_f32.powf(db / 20.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn balances_channels_without_mutating_sources() {
        let layer = StereoLayer {
            start_frame: 1, gain: 1.0, pan: 1.0, frames: vec![[0.5, 0.5]]
        };
        let mixed = mix_stereo(&[layer.clone()], 3).unwrap();
        assert_eq!(mixed[1], [0.0, 0.5]);
        assert_eq!(layer.frames[0], [0.5, 0.5]);
    }
    #[test]
    fn fades_are_bounded_and_deterministic() {
        let mut samples = vec![1.0; 5];
        fade_linear(&mut samples, 2, 2).unwrap();
        assert_eq!(samples, vec![0.0, 0.5, 1.0, 0.5, 0.0]);
        assert!(db_to_linear(f32::NAN).is_err());
    }
    #[test]
    fn rejects_nan_audio() {
        assert!(mix_stereo(&[StereoLayer {
            start_frame: 0, gain: 1.0, pan: f32::NAN, frames: vec![[0.0, 0.0]]
        }], 1).is_err());
    }
}
