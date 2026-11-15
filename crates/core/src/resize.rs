//! SIMD downscaling with `fast_image_resize`.

use std::sync::Arc;

use fast_image_resize::images::{Image, ImageRef};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

use crate::error::{DecodeError, Result};
use crate::pixels::{Frame, PixelFormat};

/// Largest size with the same aspect ratio that fits in `max_w`×`max_h`
/// (never larger than the source).
pub fn fit_size(w: u32, h: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    if w <= max_w && h <= max_h {
        return (w.max(1), h.max(1));
    }
    let s = (f64::from(max_w) / f64::from(w)).min(f64::from(max_h) / f64::from(h));
    (
        ((f64::from(w) * s).round() as u32).max(1),
        ((f64::from(h) * s).round() as u32).max(1),
    )
}

fn pixel_type(f: PixelFormat) -> PixelType {
    match f {
        PixelFormat::Gray8 => PixelType::U8,
        PixelFormat::Rgb8 => PixelType::U8x3,
        PixelFormat::Rgba8 | PixelFormat::Rgba8Premultiplied => PixelType::U8x4,
    }
}

/// Resizes a frame to exactly `w`×`h` (area-averaging when shrinking).
pub fn resize_frame(src: &Frame, w: u32, h: u32) -> Result<Frame> {
    let err = |e: &dyn std::fmt::Display| DecodeError::format("resize", e.to_string());
    let pt = pixel_type(src.format);
    let row = src.width as usize * src.format.bytes_per_pixel();
    // fast_image_resize needs tightly packed rows.
    let packed;
    let data: &[u8] = if src.stride == row {
        &src.data
    } else {
        packed = (0..src.height as usize)
            .flat_map(|y| &src.data[y * src.stride..y * src.stride + row])
            .copied()
            .collect::<Vec<u8>>();
        &packed
    };
    let src_img = ImageRef::new(src.width, src.height, data, pt).map_err(|e| err(&e))?;
    let mut dst = Image::new(w.max(1), h.max(1), pt);
    let opts = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
        // Straight alpha must be premultiplied while filtering; premultiplied is already.
        .use_alpha(src.format == PixelFormat::Rgba8);
    Resizer::new()
        .resize(&src_img, &mut dst, &opts)
        .map_err(|e| err(&e))?;
    let (dw, dh) = (dst.width(), dst.height());
    let mut out = Frame::new(dw, dh, src.format, dst.into_vec())
        .ok_or_else(|| DecodeError::format("resize", "bad buffer"))?;
    out.delay = src.delay;
    Ok(out)
}

/// Shrinks to fit `max`×`max`; returns a cheap clone if already small enough.
pub fn shrink_to_fit(src: &Frame, max: u32) -> Result<Frame> {
    let (w, h) = fit_size(src.width, src.height, max, max);
    if (w, h) == (src.width, src.height) {
        return Ok(Frame {
            data: Arc::clone(&src.data),
            ..src.clone()
        });
    }
    resize_frame(src, w, h)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_size_keeps_aspect() {
        assert_eq!(fit_size(4000, 3000, 256, 256), (256, 192));
        assert_eq!(fit_size(100, 50, 256, 256), (100, 50));
        assert_eq!(fit_size(10000, 1, 100, 100), (100, 1));
    }

    #[test]
    fn resizes_rgb() {
        let src = Frame::new(4, 4, PixelFormat::Rgb8, vec![200; 48]).expect("frame");
        let out = resize_frame(&src, 2, 2).expect("resize");
        assert_eq!((out.width, out.height, out.data.len()), (2, 2, 12));
        assert!(out.data.iter().all(|&v| v == 200));
    }

    #[test]
    fn handles_strided_input() {
        let mut src = Frame::new(2, 2, PixelFormat::Gray8, vec![9, 9, 0, 9, 9, 0]).expect("frame");
        src.stride = 3;
        let out = resize_frame(&src, 1, 1).expect("resize");
        assert_eq!(&out.data[..], &[9]);
    }
}
