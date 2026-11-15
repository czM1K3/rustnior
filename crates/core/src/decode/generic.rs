use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

/// Fallback decoder backed by the `image` crate (PNG, GIF, WebP, BMP, TIFF,
/// ICO, TGA, PNM, QOI, HDR, EXR, DDS, farbfeld).
pub struct GenericDecoder;

impl Decoder for GenericDecoder {
    fn name(&self) -> &'static str {
        "generic"
    }

    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool {
        image::guess_format(head).is_ok() || ext.and_then(ImageFormat::from_extension).is_some()
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        let reader = ImageReader::new(Cursor::new(data))
            .with_guessed_format()
            .map_err(|e| DecodeError::format("image", e))?;
        let format = reader.format().ok_or(DecodeError::Unsupported)?;
        let name = format_name(format);
        let (width, height) = reader
            .into_dimensions()
            .map_err(|e| DecodeError::format(name, e))?;
        req.check_dimensions(width, height)?;
        req.cancel.check()?;

        let mut reader = ImageReader::with_format(Cursor::new(data), format);
        reader.no_limits();
        let mut decoder = reader
            .into_decoder()
            .map_err(|e| DecodeError::format(name, e))?;
        let exif = decoder.exif_metadata().ok().flatten();
        let img = DynamicImage::from_decoder(decoder).map_err(|e| DecodeError::format(name, e))?;
        req.cancel.check()?;
        let frame = dynamic_to_frame(img).ok_or_else(|| DecodeError::format(name, "bad buffer"))?;
        Ok(DecodedImage::still(frame, name).with_exif(exif))
    }
}

/// Converts to one of our 8-bit layouts, avoiding a copy when it already is one.
pub(crate) fn dynamic_to_frame(img: DynamicImage) -> Option<Frame> {
    let (w, h) = (img.width(), img.height());
    let (format, data) = match img {
        DynamicImage::ImageLuma8(b) => (PixelFormat::Gray8, b.into_raw()),
        DynamicImage::ImageRgb8(b) => (PixelFormat::Rgb8, b.into_raw()),
        DynamicImage::ImageRgba8(b) => (PixelFormat::Rgba8, b.into_raw()),
        other if other.color().has_alpha() => (PixelFormat::Rgba8, other.into_rgba8().into_raw()),
        other if other.color().has_color() => (PixelFormat::Rgb8, other.into_rgb8().into_raw()),
        other => (PixelFormat::Gray8, other.into_luma8().into_raw()),
    };
    Frame::new(w, h, format, data)
}

fn format_name(f: ImageFormat) -> &'static str {
    match f {
        ImageFormat::Png => "PNG",
        ImageFormat::Gif => "GIF",
        ImageFormat::WebP => "WebP",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Tiff => "TIFF",
        ImageFormat::Ico => "ICO",
        ImageFormat::Tga => "TGA",
        ImageFormat::Pnm => "PNM",
        ImageFormat::Qoi => "QOI",
        ImageFormat::Hdr => "HDR",
        ImageFormat::OpenExr => "OpenEXR",
        ImageFormat::Dds => "DDS",
        ImageFormat::Farbfeld => "Farbfeld",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::Avif => "AVIF",
        _ => "image",
    }
}
