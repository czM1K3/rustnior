//! EXIF helpers on top of `kamadak-exif`.

use exif::{In, Reader, Tag};

use crate::transform::Orientation;

/// Reads the orientation tag from a raw EXIF (TIFF) block.
pub fn orientation(raw: &[u8]) -> Orientation {
    Reader::new()
        .read_raw(raw.to_vec())
        .ok()
        .and_then(|e| {
            e.get_field(Tag::Orientation, In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        })
        .and_then(|v| u16::try_from(v).ok())
        .map_or(Orientation::IDENTITY, Orientation::from_exif)
}

/// Human-readable EXIF fields (tag name, value), primary image first.
pub fn fields(raw: &[u8]) -> Vec<(String, String)> {
    let Ok(exif) = Reader::new().read_raw(raw.to_vec()) else {
        return Vec::new();
    };
    exif.fields()
        .filter(|f| f.ifd_num == In::PRIMARY && !is_noise(f.tag))
        .map(|f| {
            (
                f.tag.to_string(),
                f.display_value().with_unit(&exif).to_string(),
            )
        })
        .collect()
}

/// Structural tags that mean nothing to a person looking at a photo.
fn is_noise(tag: Tag) -> bool {
    matches!(
        tag,
        Tag::ExifIFDPointer
            | Tag::GPSInfoIFDPointer
            | Tag::InteropIFDPointer
            | Tag::JPEGInterchangeFormat
            | Tag::JPEGInterchangeFormatLength
            | Tag::MakerNote
            | Tag::ComponentsConfiguration
    )
}
