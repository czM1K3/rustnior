//! Format decoders behind a common [`Decoder`] trait.
//!
//! The [`Decoders`] registry probes the file header (falling back to the file
//! extension) and dispatches to the first decoder that claims the file.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;
use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame};

mod animated;
mod generic;
#[cfg(feature = "heif")]
mod heif;
mod jpeg;
#[cfg(feature = "jxl")]
mod jxl;
#[cfg(feature = "svg")]
mod svg;

pub use animated::AnimatedDecoder;
pub use generic::GenericDecoder;
#[cfg(feature = "heif")]
pub use heif::HeifDecoder;
pub use jpeg::JpegDecoder;
#[cfg(feature = "jxl")]
pub use jxl::JxlDecoder;
#[cfg(feature = "svg")]
pub use svg::SvgDecoder;

/// Images with more pixels than this are refused (≈ 4 GB as RGBA).
pub const DEFAULT_MAX_PIXELS: u64 = 1 << 30;
/// Stills larger than this many pixels get a screen-sized reduced copy.
const REDUCE_MIN_PIXELS: u64 = 8_000_000;

/// Receives a quick reduced-size decode before the full one finishes.
#[derive(Clone)]
pub struct PreviewSink(pub Arc<dyn Fn(DecodedImage) + Send + Sync>);

impl std::fmt::Debug for PreviewSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PreviewSink")
    }
}

/// Parameters for a single decode.
#[derive(Clone, Debug)]
pub struct DecodeRequest {
    pub cancel: CancelToken,
    /// Size the image will be shown at, in device pixels (normally the
    /// monitor size). Vector formats render at this size, large images get a
    /// reduced copy of it, and previews are at least this big.
    pub target_size: Option<(u32, u32)>,
    pub max_pixels: u64,
    /// If set, decoders that can do so cheaply deliver a preview here first.
    pub preview: Option<PreviewSink>,
}

impl Default for DecodeRequest {
    fn default() -> Self {
        Self {
            cancel: CancelToken::new(),
            target_size: None,
            max_pixels: DEFAULT_MAX_PIXELS,
            preview: None,
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

    /// A fast, reduced decode at least `min` in size (device pixels), when
    /// the format allows one much cheaper than the full decode (e.g. JPEG DCT
    /// scaling). `None` if not supported or not worthwhile.
    fn decode_preview(
        &self,
        _data: &[u8],
        _min: (u32, u32),
        _req: &DecodeRequest,
    ) -> Option<Result<DecodedImage>> {
        None
    }
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
        #[cfg(feature = "jxl")]
        decoders.push(Box::new(JxlDecoder));
        #[cfg(feature = "heif")]
        decoders.push(Box::new(HeifDecoder));
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
        if let (Some(sink), Some(target)) = (&req.preview, req.target_size) {
            let preview = catch_unwind(AssertUnwindSafe(|| {
                decoder.decode_preview(data, target, req)
            }));
            if let Ok(Some(Ok(mut p))) = preview {
                p.is_preview = true;
                (sink.0)(p);
            }
            req.cancel.check()?;
        }
        let mut img = catch_unwind(AssertUnwindSafe(|| decoder.decode(data, req)))
            .unwrap_or(Err(DecodeError::Panicked))?;
        req.cancel.check()?;
        if let Some(target) = req.target_size {
            img.reduced = reduce_for_display(&img, target);
        }
        Ok(img)
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

/// A screen-sized copy of a large still, or `None` if not worth having.
fn reduce_for_display(img: &DecodedImage, (tw, th): (u32, u32)) -> Option<Frame> {
    let f = img.first_frame()?;
    let big = u64::from(f.width) * u64::from(f.height) >= REDUCE_MIN_PIXELS;
    // Compare against the target in the image's own (unrotated) orientation.
    let (tw, th) = img.orientation.inverse().apply_size(tw, th);
    let much_larger = f.width > tw.saturating_mul(3) / 2 || f.height > th.saturating_mul(3) / 2;
    if img.is_animated() || !big || !much_larger {
        return None;
    }
    let (w, h) = crate::resize::fit_size(f.width, f.height, tw, th);
    crate::resize::resize_frame(f, w, h).ok()
}

pub fn extension_lowercase(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
}

/// Extensions considered images when scanning folders.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    "jpg",
    "jpeg",
    "jpe",
    "jfif",
    "png",
    "apng",
    "gif",
    "webp",
    "bmp",
    "dib",
    "tif",
    "tiff",
    "ico",
    "tga",
    "pbm",
    "pgm",
    "ppm",
    "pnm",
    "pam",
    "qoi",
    "hdr",
    "exr",
    "dds",
    "ff",
    "svg",
    "svgz",
    #[cfg(feature = "heif")]
    "avif",
    #[cfg(feature = "heif")]
    "heic",
    #[cfg(feature = "heif")]
    "heif",
    #[cfg(feature = "jxl")]
    "jxl",
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
    fn large_images_get_a_reduced_copy() {
        let f = Frame::new(4000, 3000, crate::PixelFormat::Gray8, vec![7; 12_000_000]);
        let img = DecodedImage::still(f.expect("frame"), "test");
        let r = reduce_for_display(&img, (1000, 1000)).expect("reduced");
        assert_eq!((r.width, r.height), (1000, 750));
        assert!(
            reduce_for_display(&img, (3000, 3000)).is_none(),
            "not much larger"
        );
    }

    #[test]
    fn extension_check_is_case_insensitive() {
        assert!(Decoders::is_supported_extension("JPG"));
        assert!(!Decoders::is_supported_extension("txt"));
    }
}
