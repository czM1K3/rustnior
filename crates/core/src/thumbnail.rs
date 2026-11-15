//! Thumbnails following the freedesktop.org Thumbnail Managing Standard:
//! existing thumbnails made by file managers are reused, and new ones are
//! written back so other applications benefit too.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use md5::{Digest, Md5};

use crate::decode::{DecodeRequest, Decoders};
use crate::error::{DecodeError, Result};
use crate::pixels::{Frame, PixelFormat};
use crate::resize::shrink_to_fit;

/// Standard thumbnail sizes ("flavors") and their directory names.
const FLAVORS: &[(u32, &str)] = &[
    (128, "normal"),
    (256, "large"),
    (512, "x-large"),
    (1024, "xx-large"),
];

pub struct Thumbnailer {
    decoders: std::sync::Arc<Decoders>,
    /// `$XDG_CACHE_HOME/thumbnails`, or `None` to disable the shared cache.
    cache_dir: Option<PathBuf>,
}

impl Thumbnailer {
    pub fn new(decoders: std::sync::Arc<Decoders>, cache_dir: Option<PathBuf>) -> Self {
        Self {
            decoders,
            cache_dir,
        }
    }

    /// Returns a thumbnail no larger than the smallest standard flavor that
    /// covers `size`, already rotated per EXIF.
    pub fn thumbnail(&self, path: &Path, size: u32, req: &DecodeRequest) -> Result<Frame> {
        let (flavor_size, flavor) = FLAVORS
            .iter()
            .copied()
            .find(|(s, _)| *s >= size)
            .unwrap_or(FLAVORS[FLAVORS.len() - 1]);
        let mtime = std::fs::metadata(path)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let uri = file_uri(path);
        let cached = self
            .cache_dir
            .as_ref()
            .map(|d| d.join(flavor).join(format!("{}.png", md5_hex(&uri))));

        if let (Some(cp), Some(mtime)) = (&cached, mtime)
            && let Some(frame) = read_cached(cp, mtime)
        {
            return Ok(frame);
        }
        req.cancel.check()?;

        let req = DecodeRequest {
            target_size: Some((flavor_size, flavor_size)),
            ..req.clone()
        };
        let img = self.decoders.decode_path(path, &req)?;
        req.cancel.check()?;
        let first = img
            .first_frame()
            .ok_or_else(|| DecodeError::format("thumbnail", "no frames"))?;
        let small = shrink_to_fit(first, flavor_size)?;
        let thumb = img.orientation.apply_to_frame(&small);

        // Only cache when we actually reduced the image; tiny files are cheap.
        let reduced = first.width > flavor_size || first.height > flavor_size;
        if let (Some(cp), Some(mtime), true) = (&cached, mtime, reduced)
            && let Err(err) = write_cached(cp, &thumb, &uri, mtime)
        {
            tracing::debug!(path = %cp.display(), %err, "could not save thumbnail");
        }
        Ok(thumb)
    }
}

/// `file://` URI with the same escaping GLib uses for paths.
pub fn file_uri(path: &Path) -> String {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_owned());
    let mut out = String::from("file://");
    for &b in abs.as_os_str().as_encoded_bytes() {
        let keep = b.is_ascii_alphanumeric() || b"!$&'()*+,-./:=@_~".contains(&b);
        if keep {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn md5_hex(s: &str) -> String {
    Md5::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn read_cached(path: &Path, mtime: u64) -> Option<Frame> {
    let file = File::open(path).ok()?;
    let mut dec = png::Decoder::new(BufReader::new(file));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let info = reader.info();
    let stored = info
        .uncompressed_latin1_text
        .iter()
        .find(|c| c.keyword == "Thumb::MTime")
        .map(|c| c.text.clone())
        .or_else(|| {
            info.utf8_text
                .iter()
                .find(|c| c.keyword == "Thumb::MTime")
                .and_then(|c| c.get_text().ok())
        })?;
    if stored.trim().parse::<u64>().ok()? != mtime {
        return None; // Stale: the image changed after the thumbnail was made.
    }
    let mut buf = vec![0; reader.output_buffer_size()?];
    let out = reader.next_frame(&mut buf).ok()?;
    buf.truncate(out.buffer_size());
    let (format, data) = match out.color_type {
        png::ColorType::Rgba => (PixelFormat::Rgba8, buf),
        png::ColorType::Rgb => (PixelFormat::Rgb8, buf),
        png::ColorType::Grayscale => (PixelFormat::Gray8, buf),
        png::ColorType::GrayscaleAlpha => (
            PixelFormat::Rgba8,
            buf.as_chunks::<2>()
                .0
                .iter()
                .flat_map(|ga| [ga[0], ga[0], ga[0], ga[1]])
                .collect(),
        ),
        png::ColorType::Indexed => return None,
    };
    Frame::new(out.width, out.height, format, data)
}

fn write_cached(path: &Path, frame: &Frame, uri: &str, mtime: u64) -> std::io::Result<()> {
    let io = |e: png::EncodingError| std::io::Error::other(e.to_string());
    let dir = path
        .parent()
        .ok_or_else(|| std::io::Error::other("no parent"))?;
    create_private_dir(dir)?;

    let (color, data): (png::ColorType, std::borrow::Cow<'_, [u8]>) = match frame.format {
        PixelFormat::Gray8 => (png::ColorType::Grayscale, (&frame.data[..]).into()),
        PixelFormat::Rgb8 => (png::ColorType::Rgb, (&frame.data[..]).into()),
        PixelFormat::Rgba8 => (png::ColorType::Rgba, (&frame.data[..]).into()),
        PixelFormat::Rgba8Premultiplied => {
            (png::ColorType::Rgba, unpremultiply(&frame.data).into())
        }
    };

    // Write to a temporary file and rename, so readers never see partial PNGs.
    let tmp = dir.join(format!(
        ".{}.rustnior-{}.tmp",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default(),
        std::process::id()
    ));
    {
        let file = create_private_file(&tmp)?;
        let mut enc = png::Encoder::new(BufWriter::new(file), frame.width, frame.height);
        enc.set_color(color);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        enc.add_text_chunk("Thumb::URI".into(), uri.into())
            .map_err(io)?;
        enc.add_text_chunk("Thumb::MTime".into(), mtime.to_string())
            .map_err(io)?;
        enc.add_text_chunk("Software".into(), "rustnior".into())
            .map_err(io)?;
        let mut w = enc.write_header().map_err(io)?;
        w.write_image_data(&data).map_err(io)?;
        w.finish().map_err(io)?;
    }
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

fn unpremultiply(data: &[u8]) -> Vec<u8> {
    data.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = u16::from(p[3]);
            if a == 0 {
                [0, 0, 0, 0]
            } else {
                let f = |c: u8| ((u16::from(c) * 255 + a / 2) / a).min(255) as u8;
                [f(p[0]), f(p[1]), f(p[2]), p[3]]
            }
        })
        .collect()
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn create_private_file(path: &Path) -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn create_private_file(path: &Path) -> std::io::Result<File> {
    File::create(path)
}

/// Lets thumbnails live in a [`crate::Loader`] cache.
impl crate::cache::Weighted for Frame {
    fn weight(&self) -> usize {
        self.byte_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uri_escaping_matches_glib() {
        assert_eq!(
            file_uri(Path::new("/a b/c#d.jpg")),
            "file:///a%20b/c%23d.jpg"
        );
        assert_eq!(file_uri(Path::new("/x/é.png")), "file:///x/%C3%A9.png");
        assert_eq!(
            file_uri(Path::new("/x/(1)+[2].png")),
            "file:///x/(1)+%5B2%5D.png"
        );
    }

    #[test]
    fn md5_of_uri() {
        // Value from the thumbnail spec's example.
        assert_eq!(
            md5_hex("file:///home/jens/photos/me.png"),
            "c6ee772d9e49320e97ec29a7eb5b1697"
        );
    }

    #[test]
    fn writes_and_reads_back_with_mtime_check() {
        let dir = std::env::temp_dir().join(format!("rustnior-thumb-{}", std::process::id()));
        let p = dir.join("normal/x.png");
        let f = Frame::new(2, 1, PixelFormat::Rgb8, vec![1, 2, 3, 4, 5, 6]).expect("frame");
        write_cached(&p, &f, "file:///x", 42).expect("write");
        let back = read_cached(&p, 42).expect("read");
        assert_eq!(&back.data[..], &f.data[..]);
        assert!(
            read_cached(&p, 43).is_none(),
            "stale thumbnail must be ignored"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unpremultiply_restores_colour() {
        assert_eq!(unpremultiply(&[64, 0, 0, 128]), vec![128, 0, 0, 128]);
        assert_eq!(unpremultiply(&[0, 0, 0, 0]), vec![0, 0, 0, 0]);
    }
}
