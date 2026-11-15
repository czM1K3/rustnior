//! Format decoders behind a common [`Decoder`] trait.
//!
//! The [`Decoders`] registry probes the file header (falling back to the file
//! extension) and dispatches to the first decoder that claims the file.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use crate::cancel::CancelToken;
use crate::error::{DecodeError, Result};
use crate::pixels::DecodedImage;

mod animated;
mod generic;
mod jpeg;
#[cfg(feature = "svg")]
mod svg;

pub use animated::AnimatedDecoder;
pub use generic::GenericDecoder;
pub use jpeg::JpegDecoder;
#[cfg(feature = "svg")]
pub use svg::SvgDecoder;

/// Images with more pixels than this are refused (≈ 4 GB as RGBA).
pub const DEFAULT_MAX_PIXELS: u64 = 1 << 30;

/// Parameters for a single decode.
#[derive(Clone, Debug)]
pub struct DecodeRequest {
    pub cancel: CancelToken,
    /// Size the image will be shown at, in device pixels. Vector formats render
    /// at this size; raster decoders may use it to produce a reduced preview.
    pub target_size: Option<(u32, u32)>,
    pub max_pixels: u64,
}

impl Default for DecodeRequest {
    fn default() -> Self {
        Self {
            cancel: CancelToken::new(),
            target_size: None,
            max_pixels: DEFAULT_MAX_PIXELS,
        }
    }
}

impl DecodeRequest {
    pub fn check_dimensions(&self, width: u32, height: u32) -> Result<()> {
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > self.max_pixels {
            Err(DecodeError::TooLarge { width, height })
        } else {
            Ok(())
        }
    }
}

/// A decoder for one family of image formats.
pub trait Decoder: Send + Sync {
    fn name(&self) -> &'static str;

    /// Returns true if this decoder handles a file with these leading bytes
    /// (at least 32 when the file is that long) and lowercase extension.
    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool;

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage>;
}

/// Ordered set of decoders. Earlier entries win.
pub struct Decoders {
    decoders: Vec<Box<dyn Decoder>>,
}

impl Default for Decoders {
    fn default() -> Self {
        let mut decoders: Vec<Box<dyn Decoder>> = vec![Box::new(JpegDecoder)];
        #[cfg(feature = "svg")]
        decoders.push(Box::new(SvgDecoder::default()));
        decoders.push(Box::new(AnimatedDecoder));
        decoders.push(Box::new(GenericDecoder));
        Self { decoders }
    }
}

impl Decoders {
    pub fn empty() -> Self {
        Self {
            decoders: Vec::new(),
        }
    }

    pub fn push(&mut self, decoder: Box<dyn Decoder>) {
        self.decoders.push(decoder);
    }

    pub fn find(&self, head: &[u8], ext: Option<&str>) -> Option<&dyn Decoder> {
        self.decoders
            .iter()
            .find(|d| d.matches(head, ext))
            .map(AsRef::as_ref)
    }

    /// True if the path's extension looks like something we can open. Used for
    /// cheap folder filtering without reading files.
    pub fn is_supported_extension(ext: &str) -> bool {
        SUPPORTED_EXTENSIONS
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext))
    }

    /// Decodes an in-memory file. Decoder panics are caught and reported as errors.
    pub fn decode_bytes(
        &self,
        data: &[u8],
        ext: Option<&str>,
        req: &DecodeRequest,
    ) -> Result<DecodedImage> {
        req.cancel.check()?;
        let head = &data[..data.len().min(64)];
        let decoder = self.find(head, ext).ok_or(DecodeError::Unsupported)?;
        tracing::debug!(decoder = decoder.name(), bytes = data.len(), "decoding");
        catch_unwind(AssertUnwindSafe(|| decoder.decode(data, req)))
            .unwrap_or(Err(DecodeError::Panicked))
    }

    /// Reads and decodes a file.
    pub fn decode_path(&self, path: &Path, req: &DecodeRequest) -> Result<DecodedImage> {
        req.cancel.check()?;
        let data = std::fs::read(path).map_err(|source| DecodeError::Io {
            path: path.to_owned(),
            source,
        })?;
        let ext = extension_lowercase(path);
        self.decode_bytes(&data, ext.as_deref(), req)
    }
}

pub fn extension_lowercase(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// Extensions considered images when scanning folders.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "apng", "gif", "webp", "bmp", "dib", "tif", "tiff", "ico",
    "tga", "pbm", "pgm", "ppm", "pnm", "pam", "qoi", "hdr", "exr", "dds", "ff", "svg", "svgz",
    "avif", "heic", "heif", "jxl",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probes_by_magic_before_extension() {
        let d = Decoders::default();
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0];
        assert_eq!(d.find(&jpeg, Some("png")).map(|d| d.name()), Some("jpeg"));
        let png = b"\x89PNG\r\n\x1a\n";
        assert_eq!(d.find(png, None).map(|d| d.name()), Some("animated"));
        assert_eq!(
            d.find(b"BM", Some("bmp")).map(|d| d.name()),
            Some("generic")
        );
    }

    #[test]
    fn rejects_garbage() {
        let d = Decoders::default();
        let r = d.decode_bytes(
            b"not an image at all",
            Some("txt"),
            &DecodeRequest::default(),
        );
        assert!(matches!(r, Err(DecodeError::Unsupported)));
    }

    #[test]
    fn honours_cancellation() {
        let d = Decoders::default();
        let req = DecodeRequest::default();
        req.cancel.cancel();
        let r = d.decode_bytes(&[0xFF, 0xD8, 0xFF], None, &req);
        assert!(matches!(r, Err(DecodeError::Cancelled)));
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        assert!(Decoders::is_supported_extension("JPG"));
        assert!(!Decoders::is_supported_extension("txt"));
    }
}
