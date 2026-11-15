use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

/// JPEG via `zune-jpeg` (SIMD, pure Rust), or libjpeg-turbo with the
/// `turbojpeg` feature, which also enables DCT-scaled previews. Grayscale
/// stays single-channel.
pub struct JpegDecoder;

const NAME: &str = "JPEG";

impl Decoder for JpegDecoder {
    fn name(&self) -> &'static str {
        "jpeg"
    }

    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool {
        if head.len() >= 3 {
            head.starts_with(&[0xFF, 0xD8, 0xFF])
        } else {
            matches!(ext, Some("jpg" | "jpeg" | "jpe" | "jfif"))
        }
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        #[cfg(feature = "turbojpeg")]
        if let Some(result) = turbo::decode(data, 1, req) {
            return result;
        }
        decode_zune(data, req)
    }

    #[cfg(feature = "turbojpeg")]
    fn decode_preview(
        &self,
        data: &[u8],
        min: (u32, u32),
        req: &DecodeRequest,
    ) -> Option<Result<DecodedImage>> {
        let h = turbojpeg::read_header(data).ok()?;
        let denom = turbo::preview_denominator(h.width as u32, h.height as u32, min)?;
        turbo::decode(data, denom, req)
    }
}

/// Pure-Rust decode (also the fallback for JPEGs libjpeg-turbo rejects).
fn decode_zune(data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
    let options = DecoderOptions::new_fast()
        .set_max_width(u16::MAX as usize)
        .set_max_height(u16::MAX as usize);
    let mut dec = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(data), options);
    dec.decode_headers()
        .map_err(|e| DecodeError::format(NAME, e))?;
    let info = dec
        .info()
        .ok_or_else(|| DecodeError::format(NAME, "missing header"))?;
    let (width, height) = (u32::from(info.width), u32::from(info.height));
    req.check_dimensions(width, height)?;

    let gray = matches!(dec.input_colorspace(), Some(ColorSpace::Luma));
    let (out_cs, format) = if gray {
        (ColorSpace::Luma, PixelFormat::Gray8)
    } else {
        (ColorSpace::RGB, PixelFormat::Rgb8)
    };
    dec.set_options(options.jpeg_set_out_colorspace(out_cs));
    req.cancel.check()?;

    let size = dec
        .output_buffer_size()
        .ok_or_else(|| DecodeError::format(NAME, "unknown output size"))?;
    let mut buf = vec![0u8; size];
    dec.decode_into(&mut buf)
        .map_err(|e| DecodeError::format(NAME, e))?;

    let exif = dec.exif().map(|e| strip_exif_header(e).to_vec());
    let frame = Frame::new(width, height, format, buf)
        .ok_or_else(|| DecodeError::format(NAME, "short pixel buffer"))?;
    Ok(DecodedImage::still(frame, NAME).with_exif(exif))
}

#[cfg(feature = "turbojpeg")]
mod turbo {
    use turbojpeg::{Colorspace, Decompressor, Image, PixelFormat as TjFormat, ScalingFactor};

    use super::{DecodeRequest, NAME, exif_block};
    use crate::error::{DecodeError, Result};
    use crate::pixels::{DecodedImage, Frame, PixelFormat};

    /// Largest power-of-two reduction (2, 4 or 8) that still (nearly) covers
    /// `min` when fitted — a preview is only briefly on screen, so up to 25%
    /// upscaling is fine. `None` if a preview would not be at least 2× smaller.
    pub fn preview_denominator(w: u32, h: u32, (mw, mh): (u32, u32)) -> Option<usize> {
        let fit = (f64::from(mw) / f64::from(w)).min(f64::from(mh) / f64::from(h));
        [8usize, 4, 2]
            .into_iter()
            .find(|&d| 1.0 / d as f64 >= fit * 0.75)
    }

    /// Decodes at 1/`denom` scale. `None` means "use the other decoder"
    /// (e.g. CMYK, which libjpeg-turbo cannot convert to RGB).
    pub fn decode(data: &[u8], denom: usize, req: &DecodeRequest) -> Option<Result<DecodedImage>> {
        let mut d = Decompressor::new().ok()?;
        let header = d.read_header(data).ok()?;
        if matches!(header.colorspace, Colorspace::CMYK | Colorspace::YCCK) {
            return None;
        }
        let (fw, fh) = (header.width as u32, header.height as u32);
        if let Err(e) = req.check_dimensions(fw, fh) {
            return Some(Err(e));
        }
        let factor = ScalingFactor::new(1, denom);
        d.set_scaling_factor(factor).ok()?;
        let scaled = header.scaled(factor);
        let (format, tj, bpp) = if header.colorspace == Colorspace::Gray {
            (PixelFormat::Gray8, TjFormat::GRAY, 1)
        } else {
            (PixelFormat::Rgb8, TjFormat::RGB, 3)
        };
        let pitch = scaled.width * bpp;
        let mut buf = vec![0u8; pitch * scaled.height];
        let image = Image {
            pixels: buf.as_mut_slice(),
            width: scaled.width,
            pitch,
            height: scaled.height,
            format: tj,
        };
        if let Err(e) = d.decompress(data, image) {
            return Some(Err(DecodeError::format(NAME, e)));
        }
        let frame = Frame::new(scaled.width as u32, scaled.height as u32, format, buf)?;
        let mut img = DecodedImage::still(frame, NAME).with_exif(exif_block(data));
        img.source_width = fw;
        img.source_height = fh;
        Some(Ok(img))
    }

    #[cfg(test)]
    mod tests {
        use super::preview_denominator as p;

        #[test]
        fn picks_smallest_sufficient_scale() {
            // Fit is 0.27; 1/4 is within the 25% allowance.
            assert_eq!(p(6000, 4000, (1920, 1080)), Some(4));
            assert_eq!(p(24000, 16000, (1920, 1080)), Some(8));
            assert_eq!(p(3000, 2000, (1920, 1080)), Some(2));
            assert_eq!(p(1500, 1000, (1920, 1080)), None);
        }
    }
}

/// Raw EXIF (TIFF) data from the APP1 segment, without zune's full decode.
#[cfg(feature = "turbojpeg")]
fn exif_block(data: &[u8]) -> Option<Vec<u8>> {
    let mut dec = zune_jpeg::JpegDecoder::new(ZCursor::new(data));
    dec.decode_headers().ok()?;
    dec.exif().map(|e| strip_exif_header(e).to_vec())
}

/// APP1 payloads start with `Exif\0\0`; kamadak-exif wants the TIFF data after it.
fn strip_exif_header(data: &[u8]) -> &[u8] {
    data.strip_prefix(b"Exif\0\0").unwrap_or(data)
}
