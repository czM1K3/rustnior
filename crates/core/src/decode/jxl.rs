use std::io::Cursor;
use std::time::Duration;

use jxl_oxide::{JxlImage, PixelFormat as JxlFormat};

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

const NAME: &str = "JPEG XL";

/// JPEG XL via the pure-Rust `jxl-oxide`. Orientation is applied by the
/// decoder; animations are supported.
pub struct JxlDecoder;

fn is_jxl(head: &[u8]) -> bool {
    head.starts_with(&[0xFF, 0x0A])
        || head.starts_with(&[
            0, 0, 0, 0x0C, b'J', b'X', b'L', b' ', 0x0D, 0x0A, 0x87, 0x0A,
        ])
}

impl Decoder for JxlDecoder {
    fn name(&self) -> &'static str {
        "jxl"
    }

    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool {
        is_jxl(head) || (head.len() < 12 && ext == Some("jxl"))
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        let image = JxlImage::builder()
            .read(Cursor::new(data))
            .map_err(|e| DecodeError::format(NAME, e))?;
        let (w, h) = (image.width(), image.height());
        req.check_dimensions(w, h)?;
        let (format, channels) = match image.pixel_format() {
            JxlFormat::Gray => (PixelFormat::Gray8, 1),
            JxlFormat::Rgb => (PixelFormat::Rgb8, 3),
            JxlFormat::Rgba | JxlFormat::Graya => (PixelFormat::Rgba8, 4),
            JxlFormat::Cmyk | JxlFormat::Cmyka => {
                return Err(DecodeError::format(NAME, "CMYK is not supported"));
            }
        };
        let tick = image
            .image_header()
            .metadata
            .animation
            .as_ref()
            .filter(|a| a.tps_numerator > 0)
            .map(|a| {
                Duration::from_secs_f64(f64::from(a.tps_denominator) / f64::from(a.tps_numerator))
            });

        let mut frames = Vec::new();
        for i in 0..image.num_loaded_keyframes() {
            req.cancel.check()?;
            let render = image
                .render_frame(i)
                .map_err(|e| DecodeError::format(NAME, e))?;
            let mut stream = render.stream();
            let (fw, fh) = (stream.width(), stream.height());
            let src_channels = stream.channels() as usize;
            let mut buf = vec![0u8; fw as usize * fh as usize * src_channels];
            stream.write_to_buffer(&mut buf);
            // Gray+alpha is expanded to RGBA, which GDK can upload directly.
            let data = if src_channels == 2 {
                buf.as_chunks::<2>()
                    .0
                    .iter()
                    .flat_map(|ga| [ga[0], ga[0], ga[0], ga[1]])
                    .collect()
            } else if src_channels == channels {
                buf
            } else {
                return Err(DecodeError::format(NAME, "unexpected channel count"));
            };
            let mut frame = Frame::new(fw, fh, format, data)
                .ok_or_else(|| DecodeError::format(NAME, "bad buffer"))?;
            frame.delay = tick.map_or(Duration::ZERO, |t| t * render.duration());
            frames.push(frame);
        }
        let first = frames
            .first()
            .cloned()
            .ok_or_else(|| DecodeError::format(NAME, "no frames"))?;
        let mut img = DecodedImage::still(first, NAME);
        img.frames = frames;
        Ok(img)
    }
}
