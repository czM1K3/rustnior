use libheif_rs::{ColorSpace, HeifContext, LibHeif, RgbChroma};

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

/// HEIC/HEIF and AVIF via the system libheif. libheif applies the
/// container's rotation and mirroring itself.
pub struct HeifDecoder;

/// `ftyp` brands of HEIF-family files.
fn brand(head: &[u8]) -> Option<&'static str> {
    if head.len() < 12 || &head[4..8] != b"ftyp" {
        return None;
    }
    match &head[8..12] {
        b"avif" | b"avis" => Some("AVIF"),
        b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" => {
            Some("HEIF")
        }
        _ => None,
    }
}

impl Decoder for HeifDecoder {
    fn name(&self) -> &'static str {
        "heif"
    }

    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool {
        brand(head).is_some() || (head.len() < 12 && matches!(ext, Some("heic" | "heif" | "avif")))
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        let name = brand(data).unwrap_or("HEIF");
        let err = |e: libheif_rs::HeifError| DecodeError::format(name, e);
        let ctx = HeifContext::read_from_bytes(data).map_err(err)?;
        let handle = ctx.primary_image_handle().map_err(err)?;
        req.check_dimensions(handle.width(), handle.height())?;
        req.cancel.check()?;
        let alpha = handle.has_alpha_channel();
        let chroma = if alpha {
            RgbChroma::Rgba
        } else {
            RgbChroma::Rgb
        };
        let image = LibHeif::new()
            .decode(&handle, ColorSpace::Rgb(chroma), None)
            .map_err(err)?;
        let plane = image
            .planes()
            .interleaved
            .ok_or_else(|| DecodeError::format(name, "no interleaved plane"))?;
        let (format, bpp) = if alpha {
            (PixelFormat::Rgba8, 4)
        } else {
            (PixelFormat::Rgb8, 3)
        };
        let row = plane.width as usize * bpp;
        let mut buf = Vec::with_capacity(row * plane.height as usize);
        for y in 0..plane.height as usize {
            buf.extend_from_slice(&plane.data[y * plane.stride..y * plane.stride + row]);
        }
        let frame = Frame::new(plane.width, plane.height, format, buf)
            .ok_or_else(|| DecodeError::format(name, "bad buffer"))?;
        Ok(DecodedImage::still(frame, name))
    }
}
