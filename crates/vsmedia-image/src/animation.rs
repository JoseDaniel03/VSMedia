//! Changed-tile optimization for precalculated map-image animation frames.
use super::{to_map_tiles, MapTile, RgbaImage, MAP_SIZE};
use vsmedia_api::{MediaError, MediaResult};

const MAX_FRAMES: usize = 120;
const MAX_DURATION_MS: u64 = 600_000;
const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
#[derive(Clone, Debug)]
pub struct AnimationFrame {
    pub image: RgbaImage,
    pub duration_ms: u32,
}
#[derive(Clone, Debug)]
pub struct TileDelta {
    pub frame_index: usize,
    pub duration_ms: u32,
    /// Only changed tiles are sent on subsequent frames.
    pub tiles: Vec<MapTile>,
}
pub fn changed_map_tiles(
    frames: &[AnimationFrame], columns: usize, rows: usize,
) -> MediaResult<Vec<TileDelta>> {
    if frames.is_empty() || frames.len() > MAX_FRAMES || columns == 0 || rows == 0 {
        return Err(MediaError::InvalidInput("invalid animation dimensions".into()));
    }
    let width = frames[0].image.width;
    let height = frames[0].image.height;
    let mut total_duration = 0u64;
    for frame in frames {
        if frame.image.width != width || frame.image.height != height
            || !(40..=60_000).contains(&frame.duration_ms)
        {
            return Err(MediaError::InvalidInput("mismatched frame or duration".into()));
        }
        total_duration += frame.duration_ms as u64;
        if total_duration > MAX_DURATION_MS {
            return Err(MediaError::InvalidInput("animation is too long".into()));
        }
    }
    let projected = columns.checked_mul(rows)
        .and_then(|v| v.checked_mul(MAP_SIZE * MAP_SIZE * 4))
        .and_then(|v| v.checked_mul(frames.len()));
    if projected.is_none_or(|v| v > MAX_OUTPUT_BYTES) {
        return Err(MediaError::InvalidInput("animation output exceeds memory quota".into()));
    }
    let mut previous: Option<Vec<MapTile>> = None;
    let mut result = Vec::with_capacity(frames.len());
    for (frame_index, frame) in frames.iter().enumerate() {
        let tiles = to_map_tiles(&frame.image, columns, rows)?;
        let changed = tiles.iter().enumerate().filter(|(index, tile)| {
            previous.as_ref().is_none_or(|prev| prev[*index].rgba != tile.rgba)
        }).map(|(_, tile)| tile.clone()).collect();
        result.push(TileDelta { frame_index, duration_ms: frame.duration_ms, tiles: changed });
        previous = Some(tiles);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn skips_unchanged_tiles() {
        let red = RgbaImage::new(1,1,vec![255,0,0,255]).unwrap();
        let blue = RgbaImage::new(1,1,vec![0,0,255,255]).unwrap();
        let result = changed_map_tiles(&[
            AnimationFrame { image: red.clone(), duration_ms: 100 },
            AnimationFrame { image: red, duration_ms: 100 },
            AnimationFrame { image: blue, duration_ms: 100 },
        ], 1, 1).unwrap();
        assert_eq!(result[0].tiles.len(), 1);
        assert!(result[1].tiles.is_empty());
        assert_eq!(result[2].tiles.len(), 1);
    }
    #[test]
    fn rejects_short_frames_and_expensive_output() {
        let image = RgbaImage::new(1,1,vec![0;4]).unwrap();
        let frames = vec![AnimationFrame { image, duration_ms: 10 }];
        assert!(changed_map_tiles(&frames, 1, 1).is_err());
    }
}
