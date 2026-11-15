use std::sync::{Arc, OnceLock};

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg::{self, fontdb};

use super::{DecodeRequest, Decoder};
use crate::error::{DecodeError, Result};
use crate::pixels::{DecodedImage, Frame, PixelFormat};

const NAME: &str = "SVG";
/// Upper bound for the rasterised size of one side.
const MAX_SIDE: f32 = 8192.0;

/// SVG/SVGZ rasterised with `resvg`. System fonts are loaded lazily, once,
/// the first time an SVG is decoded (on a worker thread).
#[derive(Default)]
pub struct SvgDecoder {
    fonts: OnceLock<Arc<fontdb::Database>>,
}

impl SvgDecoder {
    fn fonts(&self) -> Arc<fontdb::Database> {
        self.fonts
            .get_or_init(|| {
                let mut db = fontdb::Database::new();
                db.load_system_fonts();
                Arc::new(db)
            })
            .clone()
    }
}

fn looks_like_svg(head: &[u8]) -> bool {
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with("<svg") || (t.starts_with("<?xml") && text.contains("<svg"))
}

impl Decoder for SvgDecoder {
    fn name(&self) -> &'static str {
        "svg"
    }

    fn matches(&self, head: &[u8], ext: Option<&str>) -> bool {
        matches!(ext, Some("svg" | "svgz")) || looks_like_svg(head)
    }

    fn decode(&self, data: &[u8], req: &DecodeRequest) -> Result<DecodedImage> {
        let opt = usvg::Options {
            fontdb: self.fonts(),
            ..usvg::Options::default()
        };
        let tree = usvg::Tree::from_data(data, &opt).map_err(|e| DecodeError::format(NAME, e))?;
        req.cancel.check()?;

        let size = tree.size();
        let (sw, sh) = (size.width(), size.height());
        // Render at intrinsic size, or larger if the view is larger, so vectors stay crisp.
        let mut scale = match req.target_size {
            Some((tw, th)) => (tw as f32 / sw).min(th as f32 / sh).max(1.0),
            None => 1.0,
        };
        scale = scale.min(MAX_SIDE / sw.max(sh));
        let (w, h) = (
            (sw * scale).ceil().max(1.0) as u32,
            (sh * scale).ceil().max(1.0) as u32,
        );
        req.check_dimensions(w, h)?;
        let mut pixmap = Pixmap::new(w, h).ok_or(DecodeError::TooLarge {
            width: w,
            height: h,
        })?;
        resvg::render(
            &tree,
            Transform::from_scale(scale, scale),
            &mut pixmap.as_mut(),
        );

        let frame = Frame::new(w, h, PixelFormat::Rgba8Premultiplied, pixmap.take())
            .ok_or_else(|| DecodeError::format(NAME, "bad buffer"))?;
        let mut img = DecodedImage::still(frame, NAME);
        img.source_width = sw.ceil() as u32;
        img.source_height = sh.ceil() as u32;
        Ok(img)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_svg_header() {
        assert!(looks_like_svg(b"<?xml version=\"1.0\"?>\n<svg xmlns="));
        assert!(looks_like_svg(b"  <svg width=\"1\""));
        assert!(!looks_like_svg(b"\x89PNG"));
    }

    #[test]
    fn renders_simple_svg() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="20"><rect width="10" height="20" fill="red"/></svg>"#;
        let img = SvgDecoder::default().decode(svg, &DecodeRequest::default());
        let Ok(img) = img else {
            panic!("decode failed")
        };
        let f = &img.frames[0];
        assert_eq!((f.width, f.height), (10, 20));
        assert_eq!(&f.data[..4], &[255, 0, 0, 255]);
    }
}
