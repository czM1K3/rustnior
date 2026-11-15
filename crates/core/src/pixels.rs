use std::sync::Arc;
use std::time::Duration;

/// In-memory pixel layouts produced by decoders. All are 8 bits per channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    Gray8,
    Rgb8,
    /// RGBA with straight (non-premultiplied) alpha.
    Rgba8,
    /// RGBA with premultiplied alpha.
    Rgba8Premultiplied,
}

impl PixelFormat {
    pub const fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Gray8 => 1,
            Self::Rgb8 => 3,
            Self::Rgba8 | Self::Rgba8Premultiplied => 4,
        }
    }

    pub const fn has_alpha(self) -> bool {
        matches!(self, Self::Rgba8 | Self::Rgba8Premultiplied)
    }
}

/// One frame of pixels. The buffer is reference counted so it can be shared
/// between the cache and the GPU texture without copying.
#[derive(Clone, Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub format: PixelFormat,
    pub data: Arc<Vec<u8>>,
    /// Display duration for animation frames; zero for stills.
    pub delay: Duration,
}

impl Frame {
    /// Creates a tightly packed frame. Returns `None` if `data` is too short.
    pub fn new(width: u32, height: u32, format: PixelFormat, data: Vec<u8>) -> Option<Self> {
        let stride = width as usize * format.bytes_per_pixel();
        if data.len() < stride * height as usize {
            return None;
        }
        Some(Self {
            width,
            height,
            stride,
            format,
            data: Arc::new(data),
            delay: Duration::ZERO,
        })
    }

    pub fn byte_size(&self) -> usize {
        self.data.len()
    }
}

/// A decoded image: one frame for stills, several for animations.
#[derive(Clone, Debug)]
pub struct DecodedImage {
    pub frames: Vec<Frame>,
    /// Number of times an animation repeats; `None` = forever.
    pub loop_count: Option<u32>,
    /// Short format name, e.g. "JPEG".
    pub format_name: &'static str,
    /// Dimensions of the source image. May differ from the frame size when
    /// the frame is a reduced preview.
    pub source_width: u32,
    pub source_height: u32,
    /// True if this is a reduced-size preview that will be refined later.
    pub is_preview: bool,
    /// Raw EXIF block (TIFF structure) if the container had one.
    pub exif: Option<Vec<u8>>,
}

impl DecodedImage {
    pub fn still(frame: Frame, format_name: &'static str) -> Self {
        Self {
            source_width: frame.width,
            source_height: frame.height,
            frames: vec![frame],
            loop_count: None,
            format_name,
            is_preview: false,
            exif: None,
        }
    }

    pub fn first_frame(&self) -> Option<&Frame> {
        self.frames.first()
    }

    pub fn is_animated(&self) -> bool {
        self.frames.len() > 1
    }

    /// Bytes of pixel memory held by this image.
    pub fn byte_size(&self) -> usize {
        self.frames.iter().map(Frame::byte_size).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_rejects_short_buffer() {
        assert!(Frame::new(2, 2, PixelFormat::Rgb8, vec![0; 11]).is_none());
        let f = Frame::new(2, 2, PixelFormat::Rgb8, vec![0; 12]);
        assert_eq!(f.map(|f| f.stride), Some(6));
    }

    #[test]
    fn byte_size_sums_frames() {
        let f = Frame::new(4, 4, PixelFormat::Rgba8, vec![0; 64]);
        let Some(f) = f else { panic!("frame") };
        let mut img = DecodedImage::still(f.clone(), "PNG");
        img.frames.push(f);
        assert_eq!(img.byte_size(), 128);
        assert!(img.is_animated());
    }
}
