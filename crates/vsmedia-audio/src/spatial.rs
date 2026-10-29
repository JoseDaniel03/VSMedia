//! Foundational 3D voice/audio positioning math; does not implement network voice.
use vsmedia_api::{MediaError, MediaResult};
use super::dsp::equal_power_gains;

#[derive(Debug,Clone,Copy)]
pub struct Vec3 { pub x:f64,pub y:f64,pub z:f64 }
impl Vec3 {
    fn finite(self)->bool { self.x.is_finite() && self.y.is_finite() && self.z.is_finite() }
}
#[derive(Debug,Clone,Copy)]
pub struct SpatialProfile {
    pub reference_distance:f64,
    pub max_distance:f64,
    pub rolloff:f64,
}
impl SpatialProfile {
    fn validate(self)->MediaResult<()> {
        if !self.reference_distance.is_finite() || !self.max_distance.is_finite()
            || self.reference_distance<=0.0 || self.max_distance<=self.reference_distance
            || self.max_distance>1024.0 || !self.rolloff.is_finite()
            || !(0.0..=16.0).contains(&self.rolloff) {
            return Err(MediaError::InvalidInput("invalid spatial profile".into()));
        }
        Ok(())
    }
}
#[derive(Debug,Clone,Copy)]
pub struct SpatialGains {
    pub left:f32,
    pub right:f32,
    pub distance:f64,
    pub attenuation:f32,
}

/// Inverse-distance attenuation, smooth cutoff, constant-power stereo positioning.
/// yaw_rad = 0 faces +Z; positive pan points to listener's right (+X).
pub fn spatialize(
    source:Vec3, listener:Vec3, yaw_rad:f64, profile:SpatialProfile
) -> MediaResult<SpatialGains> {
    profile.validate()?;
    if !source.finite() || !listener.finite() || !yaw_rad.is_finite() {
        return Err(MediaError::InvalidInput("invalid spatial coordinates".into()));
    }
    let dx=source.x-listener.x;
    let dy=source.y-listener.y;
    let dz=source.z-listener.z;
    let distance=dx.hypot(dy).hypot(dz);
    if !distance.is_finite() {
        return Err(MediaError::InvalidInput("spatial distance overflow".into()));
    }
    if distance>=profile.max_distance {
        return Ok(SpatialGains{left:0.0,right:0.0,distance,attenuation:0.0});
    }
    let attenuation=if distance<=profile.reference_distance {1.0} else {
        let base=profile.reference_distance/(profile.reference_distance
            +profile.rolloff*(distance-profile.reference_distance));
        // Zero first derivative on both ends of the cutoff window.
        let start=profile.reference_distance
            +0.9*(profile.max_distance-profile.reference_distance);
        let fade=if distance<=start {1.0} else {
            let t=((profile.max_distance-distance)/(profile.max_distance-start)).clamp(0.0,1.0);
            t*t*(3.0-2.0*t)
        };
        (base*fade).clamp(0.0,1.0)
    };
    let horizontal=dx.hypot(dz);
    let pan=if horizontal<1e-12 {0.0} else {
        ((dx*yaw_rad.cos()-dz*yaw_rad.sin())/horizontal).clamp(-1.0,1.0) as f32
    };
    let [left,right]=equal_power_gains((pan+1.0)/2.0)?;
    Ok(SpatialGains{left:left*attenuation as f32,right:right*attenuation as f32,
        distance,attenuation:attenuation as f32})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn p()->SpatialProfile { SpatialProfile{reference_distance:2.0,max_distance:48.0,rolloff:1.0} }
    fn origin()->Vec3 {Vec3{x:0.0,y:0.0,z:0.0}}
    #[test]
    fn right_side_is_louder_in_right_channel() {
        let gains=spatialize(Vec3{x:3.0,y:0.0,z:0.0}, origin(),0.0,p()).unwrap();
        assert!(gains.right>gains.left);
        let distant=spatialize(Vec3{x:49.0,y:0.0,z:0.0},origin(),0.0,p()).unwrap();
        assert_eq!(distant.attenuation,0.0);
    }
    #[test]
    fn gains_follow_distance_and_preserve_power() {
        let near=spatialize(Vec3{x:0.0,y:0.0,z:1.0},origin(),0.0,p()).unwrap();
        let far=spatialize(Vec3{x:0.0,y:0.0,z:12.0},origin(),0.0,p()).unwrap();
        assert!(near.attenuation>far.attenuation);
        assert!((near.left*near.left+near.right*near.right-1.0).abs()<1e-5);
    }
    #[test]
    fn rejects_nonfinite_coordinates() {
        assert!(spatialize(Vec3{x:f64::NAN,y:0.0,z:0.0},origin(),0.0,p()).is_err());
    }
}
