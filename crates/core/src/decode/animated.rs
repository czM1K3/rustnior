use std::io::Cursor;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngDecoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, Frames};

use super::{DecodeRequest, Decoder, GenericDecoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

/// Total pixel memory allowed for all frames of one animation. Larger
/// animations are shown as a still of their first frame.
pub const MAX_ANIMATION_BYTES: usize = 1 << 30;
/// Browsers treat very short delays as "as fast as possible" bugs and slow
/// them down; do the same so such GIFs play at their intended speed.
const MIN_DELAY: Duration = Duration::from_millis(20);
const DEFAULT_DELAY: Duration = Duration::from_millis(100);

/// Animated GIF, APNG and animated WebP. Single-frame files are delegated to
/// [`GenericDecoder`].
pub struct AnimatedDecoder;

#[derive(Clone, Copy)]
enum Kind {
    Gif,
    Png,
    WebP,
}

fn kind(head: &[u8]) -> Option<Kind> {
    if head.starts_with(b"GIF8") {
        Some(Kind::Gif)
    } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some(Kind::Png)
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        Some(Kind::WebP)
    } else {
        None
    }
}

impl Decoder for AnimatedDecoder {
    fn name(&self) -> &'static str {
        "animated"
    }

    fn matches(&self, head: &[u8], _ext: Option<&str>) -> bool {
        kind(head).is_some()
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        let still = || GenericDecoder.decode(data, req);
        let (frames, name) = match kind(data) {
            Some(Kind::Gif) => {
                let d = GifDecoder::new(Cursor::new(data))
                    .map_err(|e| DecodeError::format("GIF", e))?;
                (d.into_frames(), "GIF")
            }
            Some(Kind::Png) => {
                let d = PngDecoder::new(Cursor::new(data))
                    .map_err(|e| DecodeError::format("PNG", e))?;
                if !d.is_apng().unwrap_or(false) {
                    return still();
                }
                let apng = d.apng().map_err(|e| DecodeError::format("APNG", e))?;
                (apng.into_frames(), "APNG")
            }
            Some(Kind::WebP) => {
                let d = WebPDecoder::new(Cursor::new(data))
                    .map_err(|e| DecodeError::format("WebP", e))?;
                if !d.has_animation() {
                    return still();
                }
                (d.into_frames(), "WebP")
            }
            None => return Err(DecodeError::Unsupported),
        };
        match collect(frames, name, req)? {
            Some(img) if img.frames.len() > 1 => Ok(img),
            // One frame, or too big to animate: decode as a still image.
            _ => still(),
        }
    }
}

/// Collects composited frames. Returns `None` when over the memory cap.
fn collect(
    frames: Frames<'_>,
    name: &'static str,
    req: &DecodeRequest,
) -> Result<Option<DecodedImage>> {
    let mut out: Vec<Frame> = Vec::new();
    let mut bytes = 0usize;
    for f in frames {
        req.cancel.check()?;
        let f = match f {
            Ok(f) => f,
            // Show what decoded so far if a later frame is corrupt.
            Err(e) if !out.is_empty() => {
                tracing::warn!(%e, "truncated animation");
                break;
            }
            Err(e) => return Err(DecodeError::format(name, e)),
        };
        let (n, d) = f.delay().numer_denom_ms();
        let delay = if d == 0 {
            DEFAULT_DELAY
        } else {
            Duration::from_micros(u64::from(n) * 1000 / u64::from(d))
        };
        let delay = if delay < MIN_DELAY {
            DEFAULT_DELAY
        } else {
            delay
        };
        let buf = f.into_buffer();
        let (w, h) = buf.dimensions();
        req.check_dimensions(w, h)?;
        bytes += buf.len();
        if bytes > MAX_ANIMATION_BYTES {
            return Ok(None);
        }
        let mut frame = Frame::new(w, h, PixelFormat::Rgba8, buf.into_raw())
            .ok_or_else(|| DecodeError::format(name, "bad frame buffer"))?;
        frame.delay = delay;
        out.push(frame);
    }
    let Some(first) = out.first() else {
        return Err(DecodeError::format(name, "no frames"));
    };
    let mut img = DecodedImage::still(first.clone(), name);
    img.frames = out;
    Ok(Some(img))
}
