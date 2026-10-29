//! Convert predecoded RGBA images to 128x128 map-sized RGBA tiles.
//! This is a sampling stage; vanilla map palette conversion remains TODO.
pub mod animation;
use vsmedia_api::{MediaError, MediaResult};

pub const MAP_SIZE: usize = 128;
const MAX_DIMENSION: usize = 4096;
const MAX_GRID: usize = 16;

#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
}
impl RgbaImage {
    pub fn new(width: usize, height: usize, pixels: Vec<u8>) -> MediaResult<Self> {
        let expected = width.checked_mul(height).and_then(|s| s.checked_mul(4));
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION
            || expected != Some(pixels.len())
        {
            return Err(MediaError::InvalidInput("invalid RGBA image dimensions or buffer".into()));
        }
        Ok(Self { width, height, pixels })
    }
}
#[derive(Clone, Debug)]
pub struct MapTile {
    pub column: usize,
    pub row: usize,
    pub rgba: Vec<u8>,
}

/// Nearest-neighbor scaling across the entire image, then split into tiles.
pub fn to_map_tiles(image: &RgbaImage, columns: usize, rows: usize) -> MediaResult<Vec<MapTile>> {
    if columns == 0 || rows == 0 || columns > MAX_GRID || rows > MAX_GRID {
        return Err(MediaError::InvalidInput("invalid map grid".into()));
    }
    let output_width = columns * MAP_SIZE;
    let output_height = rows * MAP_SIZE;
    let mut result = Vec::with_capacity(columns * rows);
    for row in 0..rows {
        for column in 0..columns {
            let mut rgba = vec![0u8; MAP_SIZE * MAP_SIZE * 4];
            for y in 0..MAP_SIZE {
                for x in 0..MAP_SIZE {
                    let source_x = (column * MAP_SIZE + x) * image.width / output_width;
                    let source_y = (row * MAP_SIZE + y) * image.height / output_height;
                    let src = (source_y * image.width + source_x) * 4;
                    let dst = (y * MAP_SIZE + x) * 4;
                    rgba[dst..dst + 4].copy_from_slice(&image.pixels[src..src + 4]);
                }
            }
            result.push(MapTile { column, row, rgba });
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn divides_scaled_image_into_map_tiles() {
        let image = RgbaImage::new(2, 1, vec![255,0,0,255,0,0,255,255]).unwrap();
        let tiles = to_map_tiles(&image, 2, 1).unwrap();
        assert_eq!(tiles.len(), 2);
        assert_eq!(&tiles[0].rgba[..4], &[255,0,0,255]);
        assert_eq!(&tiles[1].rgba[..4], &[0,0,255,255]);
        assert_eq!(tiles[1].rgba.len(), 128 * 128 * 4);
    }
    #[test]
    fn rejects_invalid_dimensions() {
        assert!(RgbaImage::new(0, 1, vec![]).is_err());
        let image = RgbaImage::new(1, 1, vec![0; 4]).unwrap();
        assert!(to_map_tiles(&image, 100, 100).is_err());
    }
}
