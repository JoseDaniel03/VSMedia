//! Reusable non-destructive effect pipeline for stereo PCM clips.
//! All audio tracks use the same trait-based chain; no per-effect scripts.
use vsmedia_api::{MediaError, MediaResult};
use super::{dsp::{db_to_linear, equal_power_gains}, filter::StereoBiquad};

pub trait StereoEffect: Send {
    fn process(&mut self, frames:&mut [[f32;2]]) -> MediaResult<()>;
}
impl StereoEffect for StereoBiquad {
    fn process(&mut self, frames:&mut [[f32;2]]) -> MediaResult<()> {
        StereoBiquad::process(self,frames)
    }
}
#[derive(Debug, Clone, Copy)]
pub struct Gain { linear:f32 }
impl Gain {
    pub fn from_db(db:f32) -> MediaResult<Self> {
        Ok(Self {linear:db_to_linear(db)?})
    }
}
impl StereoEffect for Gain {
    fn process(&mut self, frames:&mut [[f32;2]]) -> MediaResult<()> {
        if frames.iter().any(|frame| frame.iter().any(|s|!s.is_finite())) {
            return Err(MediaError::InvalidInput("non-finite PCM input".into()));
        }
        for frame in frames {
            for value in frame { *value *= self.linear; }
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct EffectChain { effects: Vec<Box<dyn StereoEffect>> }
impl EffectChain {
    pub const MAX_EFFECTS:usize=32;
    pub fn new() -> Self { Self::default() }
    pub fn add<E:StereoEffect+'static>(&mut self,effect:E) -> MediaResult<()> {
        if self.effects.len()>=Self::MAX_EFFECTS {
            return Err(MediaError::InvalidInput("too many effects per clip".into()));
        }
        self.effects.push(Box::new(effect));
        Ok(())
    }
    pub fn process(&mut self,frames:&mut[[f32;2]]) -> MediaResult<()> {
        if frames.iter().any(|frame|frame.iter().any(|x|!x.is_finite())) {
            return Err(MediaError::InvalidInput("non-finite PCM input".into()));
        }
        for effect in &mut self.effects { effect.process(frames)?; }
        Ok(())
    }
    pub fn len(&self)->usize {self.effects.len()}
    pub fn is_empty(&self)->bool {self.effects.is_empty()}
}

/// Mix a pair of equal-length stereo segments with a constant-power transition.
/// Headroom is deliberately retained; apply mastering at export time.
pub fn crossfade_equal_power(from:&[[f32;2]], to:&[[f32;2]]) -> MediaResult<Vec<[f32;2]>> {
    if from.len()!=to.len() || from.len()>5_000_000 ||
        from.iter().chain(to).any(|frame|frame.iter().any(|x|!x.is_finite())) {
        return Err(MediaError::InvalidInput("invalid crossfade segments".into()));
    }
    if from.is_empty() { return Ok(vec![]); }
    let last=from.len().saturating_sub(1);
    from.iter().zip(to).enumerate().map(|(i,(left,right))| {
        let progress=if last==0 {0.5} else {i as f32/last as f32};
        let [a,b]=equal_power_gains(progress)?;
        Ok([left[0]*a+right[0]*b, left[1]*a+right[1]*b])
    }).collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    use super::super::filter::{StereoBiquad,FilterKind};
    #[test]
    fn chain_reuses_common_processors() {
        let mut chain=EffectChain::new();
        chain.add(Gain::from_db(0.0).unwrap()).unwrap();
        chain.add(StereoBiquad::new(48_000, FilterKind::Peaking {
            frequency_hz:2000.0,q:1.0,gain_db:0.0
        }).unwrap()).unwrap();
        let mut frames=[[0.3,-0.3];2];
        chain.process(&mut frames).unwrap();
        assert!((frames[0][0]-0.3).abs()<1e-6);
        assert_eq!(chain.len(),2);
    }
    #[test]
    fn crossfade_retains_endpoints() {
        let a=[[1.0,0.0];3];
        let b=[[0.0,1.0];3];
        let output=crossfade_equal_power(&a,&b).unwrap();
        assert!((output[0][0]-1.0).abs()<1e-6);
        assert!((output[2][1]-1.0).abs()<1e-6);
        assert!((output[1][0]-std::f32::consts::FRAC_1_SQRT_2).abs()<1e-6);
    }
}
