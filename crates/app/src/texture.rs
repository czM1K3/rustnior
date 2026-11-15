//! Zero-copy conversion of decoded frames into GDK textures.

use std::sync::Arc;

use gtk::{gdk, glib, prelude::*};
use rustnior_core::{Frame, PixelFormat};

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

/// Wraps the frame's pixels in a texture without copying them.
pub fn frame_to_texture(frame: &Frame) -> gdk::Texture {
    let bytes = glib::Bytes::from_owned(SharedBuf(Arc::clone(&frame.data)));
    gdk::MemoryTexture::new(
        frame.width as i32,
        frame.height as i32,
        memory_format(frame.format),
        &bytes,
        frame.stride,
    )
    .upcast()
}
