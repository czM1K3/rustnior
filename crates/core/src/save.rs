//! Saving rotated/flipped images.
//!
//! JPEGs are "rotated" losslessly by rewriting the EXIF orientation tag, so
//! not a single DCT coefficient changes. Other formats are re-encoded with
//! the pixels physically transformed.

use std::io::Cursor;
use std::path::Path;

use exif::experimental::Writer as ExifWriter;
use exif::{Field, In, Reader, Tag, Value};
use image::codecs::jpeg::JpegEncoder;
use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::{DynamicImage, ImageBuffer, ImageFormat};

use crate::pixels::{DecodedImage, Frame, PixelFormat};
use crate::transform::Orientation;

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("cannot write {0} files")]
    Unsupported(String),
    #[error("rotating animations is not supported")]
    Animated,
    #[error("encoding failed: {0}")]
    Encode(String),
    #[error("malformed JPEG: {0}")]
    Jpeg(&'static str),
}

#[derive(Clone, Copy, Debug)]
pub struct SaveOptions {
    /// 1–100.
    pub jpeg_quality: u8,
    /// 0 (fast) – 9 (smallest), like Viewnior's "png-compression".
    pub png_compression: u8,
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            jpeg_quality: 90,
            png_compression: 9,
        }
    }
}

/// What a save did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveKind {
    /// Only the EXIF orientation tag changed (JPEG).
    Lossless,
    /// Pixels were transformed and the file re-encoded.
    Reencoded,
}

fn is_jpeg(ext: &str) -> bool {
    matches!(ext, "jpg" | "jpeg" | "jpe" | "jfif")
}

/// Writes `img`, displayed with the extra user transform `user`, to `dest`.
/// `src` is the file the image was loaded from (used for lossless JPEG).
pub fn save_transformed(
    src: &Path,
    img: &DecodedImage,
    user: Orientation,
    dest: &Path,
    opts: &SaveOptions,
) -> Result<SaveKind, SaveError> {
    let total = img.orientation.then(user);
    let dest_ext = crate::decode::extension_lowercase(dest).unwrap_or_default();
    let src_is_jpeg = img.format_name == "JPEG";

    if src_is_jpeg && is_jpeg(&dest_ext) {
        let data = std::fs::read(src)?;
        let out = set_jpeg_orientation(&data, total)?;
        write_atomic(dest, &out)?;
        return Ok(SaveKind::Lossless);
    }
    if img.is_animated() && dest == src {
        return Err(SaveError::Animated);
    }
    let first = img
        .first_frame()
        .ok_or_else(|| SaveError::Encode("no pixels".into()))?;
    let frame = total.apply_to_frame(first);
    let bytes = encode(&frame, &dest_ext, opts)?;
    write_atomic(dest, &bytes)?;
    Ok(SaveKind::Reencoded)
}

/// Encodes a frame in the format implied by `ext`.
pub fn encode(frame: &Frame, ext: &str, opts: &SaveOptions) -> Result<Vec<u8>, SaveError> {
    let format =
        ImageFormat::from_extension(ext).ok_or_else(|| SaveError::Unsupported(ext.to_owned()))?;
    if !format.writing_enabled() {
        return Err(SaveError::Unsupported(ext.to_owned()));
    }
    let img = to_dynamic(frame).ok_or_else(|| SaveError::Encode("bad pixel buffer".into()))?;
    let enc = |e: image::ImageError| SaveError::Encode(e.to_string());
    let mut out = Vec::new();
    match format {
        ImageFormat::Jpeg => {
            let rgb = flatten_alpha(img);
            JpegEncoder::new_with_quality(&mut out, opts.jpeg_quality.clamp(1, 100))
                .encode_image(&rgb)
                .map_err(enc)?;
        }
        ImageFormat::Png => {
            let compression = match opts.png_compression {
                0..=2 => CompressionType::Fast,
                3..=6 => CompressionType::Default,
                _ => CompressionType::Best,
            };
            img.write_with_encoder(PngEncoder::new_with_quality(
                &mut out,
                compression,
                FilterType::Adaptive,
            ))
            .map_err(enc)?;
        }
        _ => img
            .write_to(&mut Cursor::new(&mut out), format)
            .map_err(enc)?,
    }
    Ok(out)
}

fn to_dynamic(f: &Frame) -> Option<DynamicImage> {
    let row = f.width as usize * f.format.bytes_per_pixel();
    let packed: Vec<u8> = if f.stride == row {
        f.data[..row * f.height as usize].to_vec()
    } else {
        (0..f.height as usize)
            .flat_map(|y| f.data[y * f.stride..y * f.stride + row].iter().copied())
            .collect()
    };
    Some(match f.format {
        PixelFormat::Gray8 => {
            DynamicImage::ImageLuma8(ImageBuffer::from_raw(f.width, f.height, packed)?)
        }
        PixelFormat::Rgb8 => {
            DynamicImage::ImageRgb8(ImageBuffer::from_raw(f.width, f.height, packed)?)
        }
        PixelFormat::Rgba8 => {
            DynamicImage::ImageRgba8(ImageBuffer::from_raw(f.width, f.height, packed)?)
        }
        PixelFormat::Rgba8Premultiplied => {
            let straight = packed
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let a = u16::from(p[3]);
                    if a == 0 {
                        [0, 0, 0, 0]
                    } else {
                        let c = |v: u8| ((u16::from(v) * 255 + a / 2) / a).min(255) as u8;
                        [c(p[0]), c(p[1]), c(p[2]), p[3]]
                    }
                })
                .collect();
            DynamicImage::ImageRgba8(ImageBuffer::from_raw(f.width, f.height, straight)?)
        }
    })
}

/// JPEG has no alpha: composite transparent pixels over white.
fn flatten_alpha(img: DynamicImage) -> DynamicImage {
    if !img.color().has_alpha() {
        return img;
    }
    let mut rgba = img.into_rgba8();
    for p in rgba.pixels_mut() {
        let a = u16::from(p[3]);
        for c in 0..3 {
            p[c] = ((u16::from(p[c]) * a + 255 * (255 - a) + 127) / 255) as u8;
        }
        p[3] = 255;
    }
    DynamicImage::ImageRgba8(rgba).into_rgb8().into()
}

/// Writes via a temporary file in the same folder and renames it over the
/// destination, keeping the original's permissions.
pub fn write_atomic(dest: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let tmp = dir.join(format!(".{name}.rustnior-{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes)?;
    if let Ok(meta) = std::fs::metadata(dest) {
        let _ = std::fs::set_permissions(&tmp, meta.permissions());
    }
    std::fs::rename(&tmp, dest).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

// ---------------------------------------------------------------- JPEG EXIF

const EXIF_HEADER: &[u8] = b"Exif\0\0";

struct Segment {
    marker: u8,
    /// Start of the 0xFF marker byte.
    start: usize,
    /// One past the end of the segment.
    end: usize,
}

/// Header segments up to (not including) the start of scan.
fn segments(data: &[u8]) -> Result<Vec<Segment>, SaveError> {
    if !data.starts_with(&[0xFF, 0xD8]) {
        return Err(SaveError::Jpeg("missing SOI"));
    }
    let mut out = Vec::new();
    let mut pos = 2;
    loop {
        if pos + 4 > data.len() {
            return Err(SaveError::Jpeg("truncated header"));
        }
        if data[pos] != 0xFF {
            return Err(SaveError::Jpeg("expected marker"));
        }
        let marker = data[pos + 1];
        if marker == 0xFF {
            pos += 1; // fill byte
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            return Ok(out);
        }
        let len = usize::from(u16::from_be_bytes([data[pos + 2], data[pos + 3]]));
        let end = pos + 2 + len;
        if len < 2 || end > data.len() {
            return Err(SaveError::Jpeg("bad segment length"));
        }
        out.push(Segment {
            marker,
            start: pos,
            end,
        });
        pos = end;
    }
}

/// Returns a copy of the JPEG `data` whose EXIF orientation is `o`.
pub fn set_jpeg_orientation(data: &[u8], o: Orientation) -> Result<Vec<u8>, SaveError> {
    let segs = segments(data)?;
    let exif_seg = segs
        .iter()
        .find(|s| s.marker == 0xE1 && data[s.start + 4..s.end].starts_with(EXIF_HEADER));
    let value = o.to_exif();

    if let Some(seg) = exif_seg {
        let tiff_start = seg.start + 4 + EXIF_HEADER.len();
        let mut out = data.to_vec();
        if patch_orientation(&mut out[tiff_start..seg.end], value) {
            return Ok(out);
        }
        // No orientation entry to patch: rebuild the EXIF block with one.
        let tiff = rebuild_exif(&data[tiff_start..seg.end], value)?;
        return Ok(splice_app1(data, seg.start, seg.end, &tiff));
    }

    // No EXIF at all: insert a minimal block after SOI (and JFIF APP0, if any).
    let at = segs
        .first()
        .filter(|s| s.marker == 0xE0)
        .map_or(2, |s| s.end);
    Ok(splice_app1(data, at, at, &minimal_tiff(value)))
}

fn splice_app1(data: &[u8], from: usize, to: usize, tiff: &[u8]) -> Vec<u8> {
    let len = (2 + EXIF_HEADER.len() + tiff.len()) as u16;
    let mut out = Vec::with_capacity(data.len() + tiff.len() + 10);
    out.extend_from_slice(&data[..from]);
    out.extend_from_slice(&[0xFF, 0xE1]);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(EXIF_HEADER);
    out.extend_from_slice(tiff);
    out.extend_from_slice(&data[to..]);
    out
}

/// Overwrites the orientation value in IFD0 in place. False if absent.
fn patch_orientation(tiff: &mut [u8], value: u16) -> bool {
    let le = match tiff.get(..2) {
        Some(b"II") => true,
        Some(b"MM") => false,
        _ => return false,
    };
    let u16_at = |t: &[u8], i: usize| -> Option<u16> {
        let b: [u8; 2] = t.get(i..i + 2)?.try_into().ok()?;
        Some(if le {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    };
    let u32_at = |t: &[u8], i: usize| -> Option<u32> {
        let b: [u8; 4] = t.get(i..i + 4)?.try_into().ok()?;
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    let Some(ifd) = u32_at(tiff, 4).map(|v| v as usize) else {
        return false;
    };
    let Some(count) = u16_at(tiff, ifd) else {
        return false;
    };
    for i in 0..usize::from(count) {
        let e = ifd + 2 + i * 12;
        if u16_at(tiff, e) == Some(0x0112) && u16_at(tiff, e + 2) == Some(3) {
            let bytes = if le {
                value.to_le_bytes()
            } else {
                value.to_be_bytes()
            };
            if let Some(slot) = tiff.get_mut(e + 8..e + 10) {
                slot.copy_from_slice(&bytes);
                return true;
            }
        }
    }
    false
}

fn minimal_tiff(value: u16) -> Vec<u8> {
    let mut t = Vec::with_capacity(26);
    t.extend_from_slice(b"MM\0\x2A\0\0\0\x08"); // big-endian, IFD0 at 8
    t.extend_from_slice(&1u16.to_be_bytes()); // one entry
    t.extend_from_slice(&0x0112u16.to_be_bytes()); // Orientation
    t.extend_from_slice(&3u16.to_be_bytes()); // SHORT
    t.extend_from_slice(&1u32.to_be_bytes()); // count
    t.extend_from_slice(&value.to_be_bytes());
    t.extend_from_slice(&[0, 0]); // padding of the 4-byte value field
    t.extend_from_slice(&0u32.to_be_bytes()); // no next IFD
    t
}

fn rebuild_exif(tiff: &[u8], value: u16) -> Result<Vec<u8>, SaveError> {
    let exif = Reader::new()
        .read_raw(tiff.to_vec())
        .map_err(|_| SaveError::Jpeg("unreadable EXIF"))?;
    let orientation = Field {
        tag: Tag::Orientation,
        ifd_num: In::PRIMARY,
        value: Value::Short(vec![value]),
    };
    let mut w = ExifWriter::new();
    w.push_field(&orientation);
    for f in exif.fields() {
        // Pointers are regenerated by the writer; the thumbnail IFD would
        // need its image data re-attached, so it is dropped.
        let skip = f.ifd_num != In::PRIMARY
            || matches!(
                f.tag,
                Tag::Orientation
                    | Tag::ExifIFDPointer
                    | Tag::GPSInfoIFDPointer
                    | Tag::InteropIFDPointer
                    | Tag::JPEGInterchangeFormat
                    | Tag::JPEGInterchangeFormatLength
            );
        if !skip {
            w.push_field(f);
        }
    }
    let mut out = Cursor::new(Vec::new());
    w.write(&mut out, exif.little_endian())
        .map_err(|_| SaveError::Jpeg("cannot write EXIF"))?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_jpeg(with_exif: Option<u16>) -> Vec<u8> {
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 2, image::Rgb([10, 20, 30])));
        let mut out = Vec::new();
        img.write_to(&mut Cursor::new(&mut out), ImageFormat::Jpeg)
            .expect("encode");
        match with_exif {
            Some(v) => set_jpeg_orientation(&out, Orientation::from_exif(v)).expect("exif"),
            None => out,
        }
    }

    fn read_orientation(jpeg: &[u8]) -> Option<u16> {
        let e = Reader::new()
            .read_from_container(&mut Cursor::new(jpeg))
            .ok()?;
        e.get_field(Tag::Orientation, In::PRIMARY)?
            .value
            .get_uint(0)
            .and_then(|v| u16::try_from(v).ok())
    }

    #[test]
    fn inserts_exif_when_missing() {
        let j = tiny_jpeg(None);
        assert_eq!(read_orientation(&j), None);
        let out = set_jpeg_orientation(&j, Orientation::from_exif(6)).expect("set");
        assert_eq!(read_orientation(&out), Some(6));
        // Image data is untouched.
        assert!(out.ends_with(&j[j.len() - 64..]));
    }

    #[test]
    fn patches_existing_tag_in_place() {
        let j = tiny_jpeg(Some(6));
        let out = set_jpeg_orientation(&j, Orientation::from_exif(3)).expect("set");
        assert_eq!(out.len(), j.len());
        assert_eq!(read_orientation(&out), Some(3));
    }

    #[test]
    fn rebuilds_exif_without_orientation_tag() {
        // An EXIF block holding only a Software tag.
        let sw = Field {
            tag: Tag::Software,
            ifd_num: In::PRIMARY,
            value: Value::Ascii(vec![b"test".to_vec()]),
        };
        let mut w = ExifWriter::new();
        w.push_field(&sw);
        let mut tiff = Cursor::new(Vec::new());
        w.write(&mut tiff, false).expect("tiff");
        let base = tiny_jpeg(None);
        let j = splice_app1(&base, 2, 2, tiff.get_ref());
        let out = set_jpeg_orientation(&j, Orientation::from_exif(8)).expect("set");
        assert_eq!(read_orientation(&out), Some(8));
        let e = Reader::new()
            .read_from_container(&mut Cursor::new(&out))
            .expect("exif");
        assert!(e.get_field(Tag::Software, In::PRIMARY).is_some());
    }

    #[test]
    fn rejects_non_jpeg() {
        assert!(set_jpeg_orientation(b"\x89PNG", Orientation::IDENTITY).is_err());
    }

    #[test]
    fn encodes_png_and_jpeg() {
        let f = Frame::new(2, 2, PixelFormat::Rgba8, vec![255; 16]).expect("frame");
        let png = encode(&f, "png", &SaveOptions::default()).expect("png");
        assert!(png.starts_with(b"\x89PNG"));
        let jpg = encode(&f, "jpg", &SaveOptions::default()).expect("jpg");
        assert!(jpg.starts_with(&[0xFF, 0xD8]));
        assert!(matches!(
            encode(&f, "xyz", &SaveOptions::default()),
            Err(SaveError::Unsupported(_))
        ));
    }

    #[test]
    fn flatten_composites_over_white() {
        let img =
            DynamicImage::ImageRgba8(ImageBuffer::from_pixel(1, 1, image::Rgba([0, 0, 0, 0])));
        let flat = flatten_alpha(img).into_rgb8();
        assert_eq!(flat.get_pixel(0, 0).0, [255, 255, 255]);
    }
}
