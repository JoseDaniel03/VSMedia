//! High-quality offline resampling for photos and custom Minecraft map murals.
//! Use the image crate's well-tested Lanczos3 implementation instead of inventing a kernel.
//! Pixel-perfect nearest neighbor is still provided by to_map_tiles() for pixel art.
use super::{to_map_tiles, MapTile, RgbaImage, MAP_SIZE};
use image::{imageops::{resize, FilterType}, ImageBuffer, Rgba};
use vsmedia_api::{MediaError, MediaResult};

const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

/// Resize once with separable Lanczos3 and then tile the resulting raster.
pub fn to_photo_map_tiles(
    image: &RgbaImage, columns: usize, rows: usize,
) -> MediaResult<Vec<MapTile>> {
    // Same grid bounds as nearest-neighbor API; fail early before allocations.
    if columns == 0 || rows == 0 || columns > 16 || rows > 16 {
        return Err(MediaError::InvalidInput("invalid map grid".into()));
    }
    let width = columns.checked_mul(MAP_SIZE)
        .ok_or_else(|| MediaError::InvalidInput("map width overflow".into()))?;
    let height = rows.checked_mul(MAP_SIZE)
        .ok_or_else(|| MediaError::InvalidInput("map height overflow".into()))?;
    if width.checked_mul(height).and_then(|n| n.checked_mul(4))
        .is_none_or(|n| n > MAX_OUTPUT_BYTES) {
        return Err(MediaError::InvalidInput("map output exceeds memory limit".into()));
    }
    let source = ImageBuffer::<Rgba<u8>, _>::from_raw(
        image.width as u32, image.height as u32, image.pixels.as_slice()
    ).ok_or_else(|| MediaError::InvalidInput("invalid source RGBA buffer".into()))?;
    let resized = resize(&source, width as u32, height as u32, FilterType::Lanczos3);
    let output = RgbaImage::new(width, height, resized.into_raw())?;
    to_map_tiles(&output, columns, rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photographs_can_be_resized_to_map_tiles() {
        let source = RgbaImage::new(2,1,vec![230,0,0,255,0,0,220,255]).unwrap();
        let tiles = to_photo_map_tiles(&source,2,1).unwrap();
        assert_eq!(tiles.len(),2);
        assert_eq!(tiles[0].rgba.len(),MAP_SIZE*MAP_SIZE*4);
        let left=&tiles[0].rgba[0..4];
        let right=&tiles[1].rgba[(MAP_SIZE-1)*4..MAP_SIZE*4];
        assert!(left[0] > left[2]);
        assert!(right[2] > right[0]);
    }
    #[test]
    fn constant_image_remains_constant() {
        let source=RgbaImage::new(1,1,vec![12,67,155,255]).unwrap();
        let tiles=to_photo_map_tiles(&source,1,1).unwrap();
        assert!(tiles[0].rgba.chunks_exact(4).all(|p|p==[12,67,155,255]));
    }
    #[test]
    fn bound_outputs_before_scaling() {
        let source=RgbaImage::new(1,1,vec![255;4]).unwrap();
        assert!(to_photo_map_tiles(&source,0,1).is_err());
        assert!(to_photo_map_tiles(&source,17,1).is_err());
    }
}
