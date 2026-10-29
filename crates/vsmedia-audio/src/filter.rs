//! In-place stereo parametric EQ based on the RBJ Audio EQ Cookbook.
//! Coefficients and two independent channel states are f64 for numerical stability.
//! Processing requires no allocation and is suitable for offline or streaming blocks.
use std::f64::consts::TAU;
use vsmedia_api::{MediaError, MediaResult};

#[derive(Debug, Clone, Copy)]
pub enum FilterKind {
    LowPass { frequency_hz: f64, q: f64 },
    HighPass { frequency_hz: f64, q: f64 },
    Peaking { frequency_hz: f64, q: f64, gain_db: f64 },
}

#[derive(Debug, Clone, Copy)]
struct Coefficients { b0: f64, b1: f64, b2: f64, a1: f64, a2: f64 }

impl Coefficients {
    fn design(sample_rate: u32, kind: FilterKind) -> MediaResult<Self> {
        if !(8_000..=192_000).contains(&sample_rate) {
            return Err(MediaError::InvalidInput("invalid sample rate".into()));
        }
        let (frequency_hz, q, gain_db) = match kind {
            FilterKind::LowPass { frequency_hz, q } |
            FilterKind::HighPass { frequency_hz, q } => (frequency_hz, q, 0.0),
            FilterKind::Peaking { frequency_hz, q, gain_db } => (frequency_hz, q, gain_db),
        };
        let nyquist = f64::from(sample_rate) / 2.0;
        if !frequency_hz.is_finite() || !(1.0..nyquist * 0.98).contains(&frequency_hz)
            || !q.is_finite() || !(0.1..=20.0).contains(&q)
            || !gain_db.is_finite() || !(-24.0..=24.0).contains(&gain_db)
        {
            return Err(MediaError::InvalidInput("invalid filter parameters".into()));
        }
        let omega = TAU * frequency_hz / f64::from(sample_rate);
        let cos = omega.cos();
        let alpha = omega.sin() / (2.0 * q);
        let (b0,b1,b2,a0,a1,a2) = match kind {
            FilterKind::LowPass { .. } => (
                (1.0-cos)/2.0, 1.0-cos, (1.0-cos)/2.0,
                1.0+alpha, -2.0*cos, 1.0-alpha
            ),
            FilterKind::HighPass { .. } => (
                (1.0+cos)/2.0, -(1.0+cos), (1.0+cos)/2.0,
                1.0+alpha, -2.0*cos, 1.0-alpha
            ),
            FilterKind::Peaking { .. } => {
                let a = 10.0_f64.powf(gain_db / 40.0);
                (
                    1.0+alpha*a, -2.0*cos, 1.0-alpha*a,
                    1.0+alpha/a, -2.0*cos, 1.0-alpha/a
                )
            }
        };
        if !a0.is_finite() || a0 <= 0.0 {
            return Err(MediaError::InvalidInput("unstable filter coefficients".into()));
        }
        Ok(Self { b0:b0/a0, b1:b1/a0, b2:b2/a0, a1:a1/a0, a2:a2/a0 })
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct ChannelState { s1:f64, s2:f64 }
impl ChannelState {
    fn tick(&mut self, x:f64, c:Coefficients) -> f64 {
        let y = c.b0 * x + self.s1;
        self.s1 = c.b1 * x - c.a1 * y + self.s2;
        self.s2 = c.b2 * x - c.a2 * y;
        if self.s1.abs() < 1e-24 { self.s1 = 0.0; }
        if self.s2.abs() < 1e-24 { self.s2 = 0.0; }
        y
    }
}
/// A streaming biquad instance with independent left/right memories.
/// Recreate or reset the filter at clip boundaries where appropriate.
pub struct StereoBiquad {
    coeffs: Coefficients,
    state: [ChannelState;2],
}
impl StereoBiquad {
    pub fn new(sample_rate:u32, kind:FilterKind) -> MediaResult<Self> {
        Ok(Self { coeffs: Coefficients::design(sample_rate,kind)?, state: [ChannelState::default();2] })
    }
    pub fn reset(&mut self) { self.state = [ChannelState::default();2]; }
    pub fn process(&mut self, frames:&mut [[f32;2]]) -> MediaResult<()> {
        if frames.iter().any(|frame| frame.iter().any(|sample| !sample.is_finite())) {
            return Err(MediaError::InvalidInput("non-finite PCM input".into()));
        }
        for frame in frames {
            for (channel, value) in frame.iter_mut().enumerate() {
                *value = self.state[channel].tick(f64::from(*value),self.coeffs) as f32;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lowpass_passes_dc_and_highpass_rejects_dc() {
        let mut low = StereoBiquad::new(48_000, FilterKind::LowPass {frequency_hz:1000.0,q:0.707}).unwrap();
        let mut high = StereoBiquad::new(48_000, FilterKind::HighPass {frequency_hz:1000.0,q:0.707}).unwrap();
        let mut a=vec![[0.5,0.0];5000];
        let mut b=a.clone();
        low.process(&mut a).unwrap();
        high.process(&mut b).unwrap();
        assert!((a.last().unwrap()[0]-0.5).abs()<0.002);
        assert!(a.iter().all(|x|x[1].abs()<1e-7));
        assert!(b.last().unwrap()[0].abs()<0.002);
    }
    #[test]
    fn zero_db_peaking_is_identity() {
        let mut f=StereoBiquad::new(48_000, FilterKind::Peaking {frequency_hz:2000.0,q:1.0,gain_db:0.0}).unwrap();
        let mut samples=[[0.25,-0.5],[-0.1,0.8],[0.3,0.0]];
        let reference=samples;
        f.process(&mut samples).unwrap();
        for (a,b) in samples.iter().zip(reference.iter()) {
            for i in 0..2 { assert!((a[i]-b[i]).abs()<1e-6); }
        }
    }
    #[test]
    fn rejects_invalid_q_and_nyquist() {
        assert!(StereoBiquad::new(48_000, FilterKind::LowPass {frequency_hz:24_000.0,q:1.0}).is_err());
        assert!(StereoBiquad::new(48_000, FilterKind::LowPass {frequency_hz:1000.0,q:f64::NAN}).is_err());
        assert!(StereoBiquad::new(48_000, FilterKind::HighPass {frequency_hz:1000.0,q:0.0}).is_err());
    }
}
