//! Audio project data model and a bounded PCM mixing primitive.
use serde::{Deserialize, Serialize};
use vsmedia_api::{AssetKey, MediaError, MediaResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioClip {
    pub asset: AssetKey,
    pub start_ms: u64,
    pub duration_ms: u64,
    pub gain_db: f32,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioTrack {
    pub name: String,
    pub clips: Vec<AudioClip>,
    pub muted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AudioProject {
    pub name: String,
    pub tracks: Vec<AudioTrack>,
}
impl AudioProject {
    pub fn validate(&self) -> MediaResult<()> {
        if self.name.trim().is_empty() || self.tracks.len() > 256 {
            return Err(MediaError::InvalidInput("invalid audio project".into()));
        }
        for track in &self.tracks {
            if track.name.trim().is_empty() || track.clips.len() > 4096 {
                return Err(MediaError::InvalidInput("invalid track".into()));
            }
            for clip in &track.clips {
                clip.asset.validated()?;
                if clip.duration_ms == 0 || clip.start_ms.checked_add(clip.duration_ms).is_none()
                    || !clip.gain_db.is_finite() || !(-120.0..=24.0).contains(&clip.gain_db)
                {
                    return Err(MediaError::InvalidInput("invalid clip parameters".into()));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct PcmLayer {
    /// Starting sample in the output, mono float PCM.
    pub offset_samples: usize,
    pub gain: f32,
    pub samples: Vec<f32>,
}

/// Mix already-decoded mono PCM. No decoding, sample-rate conversion or effects here.
/// Gain is linear. Bound output memory and reject NaN/Inf.
pub fn mix_mono(layers: &[PcmLayer], output_samples: usize) -> MediaResult<Vec<f32>> {
    const MAX_SAMPLES: usize = 10_000_000;
    if output_samples > MAX_SAMPLES || layers.len() > 256 {
        return Err(MediaError::InvalidInput("mix exceeds configured limits".into()));
    }
    let mut output = vec![0.0f32; output_samples];
    for layer in layers {
        if !layer.gain.is_finite() || layer.gain < 0.0 || layer.gain > 16.0
            || layer.offset_samples > output_samples
            || layer.samples.iter().any(|s| !s.is_finite())
            || layer.offset_samples.checked_add(layer.samples.len())
                .is_none_or(|end| end > output_samples)
        {
            return Err(MediaError::InvalidInput("invalid PCM layer".into()));
        }
        for (index, sample) in layer.samples.iter().enumerate() {
            output[layer.offset_samples + index] += sample * layer.gain;
        }
    }
    for sample in &mut output {
        *sample = sample.clamp(-1.0, 1.0);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mixes_and_limits_samples() {
        let out = mix_mono(&[
            PcmLayer { offset_samples: 0, gain: 1.0, samples: vec![0.75, 0.5] },
            PcmLayer { offset_samples: 1, gain: 2.0, samples: vec![0.5] },
        ], 3).unwrap();
        assert_eq!(out, vec![0.75, 1.0, 0.0]);
    }
    #[test]
    fn rejects_nans_and_oversized_mix() {
        assert!(mix_mono(&[PcmLayer { offset_samples: 0, gain: f32::NAN, samples: vec![0.1] }], 1).is_err());
        assert!(mix_mono(&[], 10_000_001).is_err());
    }
}
