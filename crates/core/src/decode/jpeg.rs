use zune_core::bytestream::ZCursor;
use zune_core::colorspace::ColorSpace;
use zune_core::options::DecoderOptions;

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

/// JPEG via `zune-jpeg` (SIMD, pure Rust). Grayscale stays single-channel.
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
        let mut img = DecodedImage::still(frame, NAME);
        img.exif = exif;
        Ok(img)
    }
}

/// APP1 payloads start with `Exif\0\0`; kamadak-exif wants the TIFF data after it.
fn strip_exif_header(data: &[u8]) -> &[u8] {
    data.strip_prefix(b"Exif\0\0").unwrap_or(data)
}
