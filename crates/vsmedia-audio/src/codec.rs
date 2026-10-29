//! Bounded WAV PCM decoder/encoder, sharing the stereo PCM frame representation.
//! This is the first audio file format; compressed codecs are separate adapters.
use std::io::Cursor;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use vsmedia_api::{MediaError, MediaResult};

pub const MAX_FRAMES: usize = 5_000_000;
pub const MAX_INPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct StereoAudio {
    pub sample_rate: u32,
    pub frames: Vec<[f32;2]>,
}
impl StereoAudio {
    pub fn validate(&self) -> MediaResult<()> {
        if !(8_000..=192_000).contains(&self.sample_rate) ||
            self.frames.len() > MAX_FRAMES ||
            self.frames.iter().any(|p| p.iter().any(|v| !v.is_finite())) {
            return Err(MediaError::InvalidInput("invalid stereo PCM".into()));
        }
        Ok(())
    }
}

fn wav_error(e: hound::Error) -> MediaError {
    MediaError::InvalidInput(format!("WAV error: {e}"))
}
pub fn decode_wav(bytes: &[u8]) -> MediaResult<StereoAudio> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err(MediaError::InvalidInput("WAV exceeds size limits".into()));
    }
    let mut reader = WavReader::new(Cursor::new(bytes)).map_err(wav_error)?;
    let spec = reader.spec();
    if !(8_000..=192_000).contains(&spec.sample_rate) ||
        !(1..=2).contains(&spec.channels) ||
        reader.duration() as usize > MAX_FRAMES {
        return Err(MediaError::InvalidInput("WAV format or duration unsupported".into()));
    }
    let channels = spec.channels as usize;
    let mut output = Vec::with_capacity(reader.duration() as usize);
    match spec.sample_format {
        SampleFormat::Float if spec.bits_per_sample == 32 => {
            let mut samples = reader.samples::<f32>();
            for _ in 0..reader.duration() {
                let l = samples.next().ok_or_else(|| MediaError::InvalidInput("short WAV".into()))?
                    .map_err(wav_error)?;
                let r = if channels == 2 {
                    samples.next().ok_or_else(|| MediaError::InvalidInput("short WAV".into()))?
                        .map_err(wav_error)?
                } else { l };
                if !l.is_finite() || !r.is_finite() {
                    return Err(MediaError::InvalidInput("non-finite WAV samples".into()));
                }
                output.push([l,r]);
            }
        },
        SampleFormat::Int if matches!(spec.bits_per_sample, 8|16|24|32) => {
            let scale = (1_i64 << (spec.bits_per_sample - 1)) as f32;
            let mut samples = reader.samples::<i32>();
            for _ in 0..reader.duration() {
                let l = samples.next().ok_or_else(|| MediaError::InvalidInput("short WAV".into()))?
                    .map_err(wav_error)? as f32 / scale;
                let r = if channels == 2 {
                    samples.next().ok_or_else(|| MediaError::InvalidInput("short WAV".into()))?
                        .map_err(wav_error)? as f32 / scale
                } else { l };
                output.push([l,r]);
            }
        },
        _ => return Err(MediaError::InvalidInput("unsupported WAV sample format".into())),
    }
    let audio = StereoAudio { sample_rate: spec.sample_rate, frames: output };
    audio.validate()?;
    Ok(audio)
}

/// WAV PCM16 export for exchange with other editors; OGG resource-pack export is
/// intentionally not declared implemented until an encoder is integrated.
pub fn encode_wav_pcm16(audio: &StereoAudio) -> MediaResult<Vec<u8>> {
    audio.validate()?;
    let output_bytes = audio.frames.len().checked_mul(4)
        .and_then(|n| n.checked_add(128))
        .ok_or_else(|| MediaError::InvalidInput("output size overflow".into()))?;
    if output_bytes > MAX_INPUT_BYTES {
        return Err(MediaError::InvalidInput("WAV output too large".into()));
    }
    let mut cursor = Cursor::new(Vec::with_capacity(output_bytes));
    let spec = WavSpec {
        channels: 2, sample_rate: audio.sample_rate,
        bits_per_sample: 16, sample_format: SampleFormat::Int,
    };
    {
        let mut writer = WavWriter::new(&mut cursor, spec).map_err(wav_error)?;
        for pair in &audio.frames {
            for &value in pair {
                let normalized = value.clamp(-1.0, 1.0);
                let quantized = if normalized < 0.0 {
                    (normalized * 32768.0).round() as i16
                } else { (normalized * 32767.0).round() as i16 };
                writer.write_sample(quantized).map_err(wav_error)?;
            }
        }
        writer.finalize().map_err(wav_error)?;
    }
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wav_pcm16_round_trip_stereo() {
        let input = StereoAudio {sample_rate: 48000, frames: vec![
            [0.0, 1.0], [0.25, -0.5], [-1.0, 0.5]
        ]};
        let bytes = encode_wav_pcm16(&input).unwrap();
        let result = decode_wav(&bytes).unwrap();
        assert_eq!(result.sample_rate, 48000);
        assert_eq!(result.frames.len(), 3);
        for (got, expected) in result.frames.iter().zip(input.frames.iter()) {
            for channel in 0..2 { assert!((got[channel]-expected[channel]).abs() < 0.00004); }
        }
    }
    #[test]
    fn supports_mono_pcm_and_float32() {
        let mut raw = Cursor::new(Vec::new());
        {
            let mut writer = WavWriter::new(&mut raw, WavSpec {
                channels:1, sample_rate:44100, bits_per_sample:32,
                sample_format:SampleFormat::Float
            }).unwrap();
            writer.write_sample(0.125_f32).unwrap();
            writer.write_sample(-0.25_f32).unwrap();
            writer.finalize().unwrap();
        }
        let result = decode_wav(&raw.into_inner()).unwrap();
        assert_eq!(result.frames, vec![[0.125,0.125],[-0.25,-0.25]]);
    }
    #[test]
    fn rejects_invalid_wav_and_nans() {
        assert!(decode_wav(b"not a WAV").is_err());
        assert!(encode_wav_pcm16(&StereoAudio {
            sample_rate:48000,frames:vec![[f32::NAN,0.0]]
        }).is_err());
    }
}
