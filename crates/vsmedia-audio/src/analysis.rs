//! Frequency analysis for waveform preview and diagnostics.
//! Uses a proven FFT implementation rather than duplicating a transform kernel.
use std::f32::consts::TAU;
use rustfft::{num_complex::Complex32, FftPlanner};
use vsmedia_api::{MediaError, MediaResult};

#[derive(Debug, Clone, Copy)]
pub struct FrequencyBin { pub frequency_hz:f32, pub magnitude:f32 }

/// Hann-windowed single-channel magnitude spectrum for a single frame.
/// Caller provides a time-domain buffer with N samples; FFT size must be a power of two.
pub fn spectrum(samples:&[f32], sample_rate:u32) -> MediaResult<Vec<FrequencyBin>> {
    let n=samples.len();
    if !n.is_power_of_two() || !(256..=16384).contains(&n)
        || !(8_000..=192_000).contains(&sample_rate)
        || samples.iter().any(|s|!s.is_finite()) {
        return Err(MediaError::InvalidInput("invalid FFT frame or sample rate".into()));
    }
    let mut input:Vec<Complex32> = samples.iter().enumerate().map(|(index,sample)| {
        let window=0.5-0.5*(TAU*index as f32/(n-1) as f32).cos();
        Complex32::new(sample*window,0.0)
    }).collect();
    let mut planner=FftPlanner::<f32>::new();
    let fft=planner.plan_fft_forward(n);
    fft.process(&mut input);
    // Coherent amplitude factor of Hann window is about 0.5.
    let gain=4.0/(n as f32-1.0);
    Ok(input.into_iter().take(n/2+1).enumerate().map(|(index,bin)| FrequencyBin {
        frequency_hz:index as f32 * sample_rate as f32/n as f32,
        magnitude:bin.norm()*if index==0 || index==n/2 {gain/2.0} else {gain},
    }).collect())
}

/// RMS level is useful for automatic gain staging and editor metering.
pub fn rms(samples:&[f32]) -> MediaResult<f32> {
    if samples.is_empty() || samples.iter().any(|s|!s.is_finite()) {
        return Err(MediaError::InvalidInput("invalid PCM samples".into()));
    }
    let energy=samples.iter().map(|v|f64::from(*v)*f64::from(*v)).sum::<f64>();
    Ok((energy/samples.len() as f64).sqrt() as f32)
}
/// Compact stereo peak/RMS values for a zoomable waveform timeline.
/// Each bucket can represent hundreds of original frames without sending PCM
/// through a web panel or allocating one point per sample.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaveformPeak {
    pub left_min:f32,
    pub left_max:f32,
    pub right_min:f32,
    pub right_max:f32,
    pub rms:f32,
}
pub fn waveform_peaks(
    frames:&[[f32;2]], frames_per_bucket:usize,
) -> MediaResult<Vec<WaveformPeak>> {
    const MAX_SOURCE_FRAMES:usize=5_000_000;
    const MAX_BUCKETS:usize=8192;
    if frames_per_bucket==0 || frames.len()>MAX_SOURCE_FRAMES ||
        frames.len().div_ceil(frames_per_bucket)>MAX_BUCKETS ||
        frames.iter().any(|f|f.iter().any(|v|!v.is_finite()))
    {
        return Err(MediaError::InvalidInput("invalid waveform preview size or PCM".into()));
    }
    let mut output=Vec::with_capacity(frames.len().div_ceil(frames_per_bucket));
    for bucket in frames.chunks(frames_per_bucket) {
        let mut left_min=f32::INFINITY;
        let mut left_max=f32::NEG_INFINITY;
        let mut right_min=f32::INFINITY;
        let mut right_max=f32::NEG_INFINITY;
        let mut energy=0.0f64;
        for [left,right] in bucket.iter().copied() {
            left_min=left_min.min(left);
            left_max=left_max.max(left);
            right_min=right_min.min(right);
            right_max=right_max.max(right);
            energy+=f64::from(left)*f64::from(left)+f64::from(right)*f64::from(right);
        }
        output.push(WaveformPeak {
            left_min,left_max,right_min,right_max,
            rms:(energy/(bucket.len()*2) as f64).sqrt() as f32,
        });
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sine_peak_is_at_1000_hz() {
        let samples:Vec<f32>=(0..1024).map(|i| (TAU*125.0*i as f32/1024.0).sin()).collect();
        let bins=spectrum(&samples,8192).unwrap();
        let peak=bins.iter().enumerate().max_by(|a,b|a.1.magnitude.total_cmp(&b.1.magnitude)).unwrap().0;
        assert_eq!(bins[peak].frequency_hz,1000.0);
        assert!((bins[peak].magnitude-1.0).abs()<0.02);
    }
    #[test]
    fn waveform_preview_keeps_stereo_extrema_and_level() {
        let frames=[[0.25,-0.5],[-0.75,0.25],[0.1,0.4],[0.2,0.2],[0.8,-0.1]];
        let buckets=waveform_peaks(&frames,2).unwrap();
        assert_eq!(buckets.len(),3);
        assert_eq!(buckets[0].left_min,-0.75);
        assert_eq!(buckets[0].left_max,0.25);
        assert_eq!(buckets[0].right_min,-0.5);
        assert_eq!(buckets[0].right_max,0.25);
        assert!((buckets[2].rms-((0.64+0.01)/2.0f32).sqrt()).abs()<1e-6);
        assert!(waveform_peaks(&frames,0).is_err());
        assert!(waveform_peaks(&[[f32::NAN,0.0]],1).is_err());
        assert!(waveform_peaks(&vec![[0.0,0.0];9000],1).is_err());
        assert!(waveform_peaks(&[],16).unwrap().is_empty());
    }
    #[test]
    fn rms_of_constant_signal_and_nan_validation() {
        assert!((rms(&[0.5,0.5,0.5]).unwrap()-0.5).abs()<1e-6);
        assert!(spectrum(&[0.0;200],48000).is_err());
        assert!(rms(&[f32::NAN]).is_err());
    }
}
