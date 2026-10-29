//! Bounded decoding of still images and GIF/APNG frame sequences.
//! Never decode remote URLs directly; the future fetcher must enforce SSRF policy.
use std::io::Cursor;
use image::{
    codecs::{gif::GifDecoder, png::PngDecoder},
    AnimationDecoder, ImageDecoder, ImageFormat, ImageReader, Limits,
};
use vsmedia_api::{MediaError, MediaResult};
use crate::{
    animation::AnimationFrame,
    RgbaImage,
};
const MAX_INPUT_BYTES: usize = 16 * 1024 * 1024;
const MAX_ANIMATION_DECODED_BYTES: usize = 64 * 1024 * 1024;
const MAX_FRAMES: usize = 120;
const MAX_DIMENSION: u32 = 4096;

fn decode_error(e: image::ImageError) -> MediaError {
    MediaError::InvalidInput(format!("image decoder: {e}"))
}
fn validate_bytes(bytes: &[u8]) -> MediaResult<()> {
    if bytes.is_empty() || bytes.len() > MAX_INPUT_BYTES {
        return Err(MediaError::InvalidInput("image input exceeds size limit".into()));
    }
    Ok(())
}
fn limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(MAX_ANIMATION_DECODED_BYTES as u64);
    limits
}

/// Decode PNG/JPEG/WebP/GIF as a single frame. Animated files produce a still
/// preview; use decode_animation for animated GIF or PNG.
pub fn decode_still(bytes: &[u8]) -> MediaResult<RgbaImage> {
    validate_bytes(bytes)?;
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()
        .map_err(|e| MediaError::InvalidInput(format!("unrecognized image format: {e}")))?;
    match reader.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif) => {}
        _ => return Err(MediaError::InvalidInput("image format not supported".into())),
    }
    reader.limits(limits());
    let decoded = reader.decode().map_err(decode_error)?.to_rgba8();
    RgbaImage::new(decoded.width() as usize, decoded.height() as usize, decoded.into_raw())
}

/// Supported frame-by-frame animation decoders. WebP animation is a future adapter.
#[derive(Clone, Copy, Debug)]
pub enum AnimatedFormat { Gif, Apng }

pub fn decode_animation(bytes:&[u8], format:AnimatedFormat) -> MediaResult<Vec<AnimationFrame>> {
    validate_bytes(bytes)?;
    let reader=Cursor::new(bytes);
    match format {
        AnimatedFormat::Gif => {
            let mut decoder=GifDecoder::new(reader).map_err(decode_error)?;
            let (width,height)=decoder.dimensions();
            check_dimensions(width,height)?;
            decoder.set_limits(limits()).map_err(decode_error)?;
            collect_frames(decoder.into_frames())
        },
        AnimatedFormat::Apng => {
            let mut decoder=PngDecoder::new(reader).map_err(decode_error)?;
            let (width,height)=decoder.dimensions();
            check_dimensions(width,height)?;
            decoder.set_limits(limits()).map_err(decode_error)?;
            collect_frames(decoder.apng().map_err(decode_error)?.into_frames())
        },
    }
}
fn check_dimensions(width:u32,height:u32) -> MediaResult<()> {
    if width==0 || height==0 || width>MAX_DIMENSION || height>MAX_DIMENSION {
        return Err(MediaError::InvalidInput("animation dimensions exceed limits".into()));
    }
    Ok(())
}
fn collect_frames(
    source: impl Iterator<Item = image::ImageResult<image::Frame>>
) -> MediaResult<Vec<AnimationFrame>> {
    let mut result=Vec::new();
    let mut decoded_bytes=0usize;
    for frame in source {
        if result.len()>=MAX_FRAMES {
            return Err(MediaError::InvalidInput("too many animation frames".into()));
        }
        let frame=frame.map_err(decode_error)?;
        let (numerator,denominator)=frame.delay().numer_denom_ms();
        let duration=if denominator==0 {40} else {
            u64::from(numerator).div_ceil(u64::from(denominator)).clamp(40,60_000) as u32
        };
        let buffer=frame.into_buffer();
        check_dimensions(buffer.width(),buffer.height())?;
        decoded_bytes=decoded_bytes.checked_add(buffer.len())
            .ok_or_else(|| MediaError::InvalidInput("animation decoded size overflow".into()))?;
        if decoded_bytes>MAX_ANIMATION_DECODED_BYTES {
            return Err(MediaError::InvalidInput("animation exceeds decoded size quota".into()));
        }
        result.push(AnimationFrame {
            image:RgbaImage::new(buffer.width() as usize,buffer.height() as usize,buffer.into_raw())?,
            duration_ms:duration,
        });
    }
    if result.is_empty() {
        return Err(MediaError::InvalidInput("animation has no frames".into()));
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ExtendedColorType, ImageEncoder};
    #[test]
    fn decodes_png_and_rejects_invalid_input() {
        let pixels=[14u8,24,34,255, 44,54,64,255];
        let mut encoded=Vec::new();
        PngEncoder::new(&mut encoded).write_image(&pixels,2,1,ExtendedColorType::Rgba8).unwrap();
        let result=decode_still(&encoded).unwrap();
        assert_eq!(result.width,2);
        assert_eq!(result.height,1);
        assert_eq!(result.pixels,pixels);
        assert!(decode_still(b"not a supported image").is_err());
        assert!(decode_animation(b"not a GIF",AnimatedFormat::Gif).is_err());
    }
    #[test]
    fn bounded_input() {
        assert!(decode_still(&[]).is_err());
    }
}
