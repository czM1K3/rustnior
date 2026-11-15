//! Zero-copy conversion of decoded frames into GDK textures, and the
//! [`Picture`] the image view draws (single texture or lazy tiles).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use gtk::{gdk, glib, prelude::*};
use rustnior_core::{Frame, PixelFormat};

/// Images with a side longer than this, or more pixels than
/// [`TILE_MIN_PIXELS`], are drawn as tiles so only visible parts reach the GPU.
const TILE_MAX_SIDE: u32 = 8192;
const TILE_MIN_PIXELS: u64 = 40_000_000;
pub const TILE: u32 = 2048;

/// Lets GLib own a reference to the decoder's buffer instead of a copy.
struct SharedBuf(Arc<Vec<u8>>);

impl AsRef<[u8]> for SharedBuf {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

pub fn memory_format(format: PixelFormat) -> gdk::MemoryFormat {
    match format {
        PixelFormat::Gray8 => gdk::MemoryFormat::G8,
        PixelFormat::Rgb8 => gdk::MemoryFormat::R8g8b8,
        PixelFormat::Rgba8 => gdk::MemoryFormat::R8g8b8a8,
        PixelFormat::Rgba8Premultiplied => gdk::MemoryFormat::R8g8b8a8Premultiplied,
    }
}

fn frame_bytes(frame: &Frame) -> glib::Bytes {
    glib::Bytes::from_owned(SharedBuf(Arc::clone(&frame.data)))
}

/// Wraps the frame's pixels in a texture without copying them.
pub fn frame_to_texture(frame: &Frame) -> gdk::Texture {
    gdk::MemoryTexture::new(
        frame.width as i32,
        frame.height as i32,
        memory_format(frame.format),
        &frame_bytes(frame),
        frame.stride,
    )
    .upcast()
}

/// Full-resolution pixels cut into `TILE`-sized textures on demand. Each
/// tile shares the decoded buffer (sub-range + full stride), so creating one
/// costs no copy; only tiles that are drawn are uploaded.
pub struct Tiles {
    frame: Frame,
    bytes: glib::Bytes,
    cache: RefCell<HashMap<(u32, u32), gdk::Texture>>,
}

impl Tiles {
    pub fn columns(&self) -> u32 {
        self.frame.width.div_ceil(TILE)
    }

    pub fn rows(&self) -> u32 {
        self.frame.height.div_ceil(TILE)
    }

    /// Texture for tile (`col`, `row`), extended by one pixel on each side
    /// (where the image allows) so linear filtering has no seams. Returns the
    /// texture and the source rectangle it covers.
    pub fn tile(&self, col: u32, row: u32) -> (gdk::Texture, [u32; 4]) {
        let f = &self.frame;
        let x0 = (col * TILE).saturating_sub(1);
        let y0 = (row * TILE).saturating_sub(1);
        let x1 = ((col + 1) * TILE + 1).min(f.width);
        let y1 = ((row + 1) * TILE + 1).min(f.height);
        let rect = [x0, y0, x1 - x0, y1 - y0];
        let tex = self
            .cache
            .borrow_mut()
            .entry((col, row))
            .or_insert_with(|| {
                let bpp = f.format.bytes_per_pixel();
                let start = y0 as usize * f.stride + x0 as usize * bpp;
                let len = (rect[3] as usize - 1) * f.stride + rect[2] as usize * bpp;
                let sub = glib::Bytes::from_bytes(&self.bytes, start..start + len);
                gdk::MemoryTexture::new(
                    rect[2] as i32,
                    rect[3] as i32,
                    memory_format(f.format),
                    &sub,
                    f.stride,
                )
                .upcast()
            })
            .clone();
        (tex, rect)
    }

    /// Forgets tiles not in `keep`, releasing their GPU memory.
    pub fn retain(&self, keep: &[(u32, u32)]) {
        self.cache.borrow_mut().retain(|k, _| keep.contains(k));
    }
}

pub enum Full {
    Texture(gdk::Texture),
    Tiled(Tiles),
}

/// Everything the view needs to draw one image.
pub struct Picture {
    pub full: Full,
    /// Screen-sized copy used while zoomed out far enough.
    pub reduced: Option<gdk::Texture>,
    /// Image size in pixels before orientation. Textures may be smaller
    /// (previews) and are scaled to it.
    pub width: u32,
    pub height: u32,
    pub has_alpha: bool,
}

impl Picture {
    /// `size` is the source image size (a preview frame is smaller).
    pub fn new(frame: &Frame, reduced: Option<&Frame>, size: (u32, u32)) -> Self {
        let huge = frame.width > TILE_MAX_SIDE
            || frame.height > TILE_MAX_SIDE
            || u64::from(frame.width) * u64::from(frame.height) > TILE_MIN_PIXELS;
        let full = if huge {
            Full::Tiled(Tiles {
                bytes: frame_bytes(frame),
                frame: frame.clone(),
                cache: RefCell::new(HashMap::new()),
            })
        } else {
            Full::Texture(frame_to_texture(frame))
        };
        Self {
            full,
            reduced: reduced.map(frame_to_texture),
            width: size.0.max(1),
            height: size.1.max(1),
            has_alpha: frame.format.has_alpha(),
        }
    }

    /// Pixel width of the full-resolution source drawn.
    pub fn full_width(&self) -> u32 {
        match &self.full {
            Full::Texture(t) => t.width() as u32,
            Full::Tiled(t) => t.frame.width,
        }
    }
}
