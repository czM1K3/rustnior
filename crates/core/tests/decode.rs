//! Decodes the sample images in `tests/fixtures`.

use std::path::PathBuf;

use rustnior_core::{DecodeError, DecodeRequest, Decoders, PixelFormat};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

fn decode(name: &str) -> rustnior_core::Result<rustnior_core::DecodedImage> {
    Decoders::default().decode_path(&fixture(name), &DecodeRequest::default())
}

#[test]
fn decodes_still_formats() {
    let cases = [
        ("rgb.jpg", 64, 48, PixelFormat::Rgb8, "JPEG"),
        ("gray.jpg", 64, 48, PixelFormat::Gray8, "JPEG"),
        ("rgb.png", 40, 30, PixelFormat::Rgb8, "PNG"),
        ("alpha.png", 32, 32, PixelFormat::Rgba8, "PNG"),
        ("still.webp", 40, 30, PixelFormat::Rgb8, "WebP"),
        ("bitmap.bmp", 20, 10, PixelFormat::Rgb8, "BMP"),
        ("vector.svg", 40, 20, PixelFormat::Rgba8Premultiplied, "SVG"),
    ];
    for (name, w, h, format, format_name) in cases {
        let img = decode(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        let f = &img.frames[0];
        assert_eq!((f.width, f.height), (w, h), "{name} size");
        assert_eq!(f.format, format, "{name} format");
        assert_eq!(img.format_name, format_name, "{name} format name");
        assert_eq!(f.data.len(), f.stride * h as usize, "{name} buffer");
        assert!(img.orientation.is_identity(), "{name} orientation");
    }
}

#[test]
fn jpeg_exposes_exif() {
    let img = decode("orient6.jpg").expect("decode");
    assert_eq!(img.orientation, rustnior_core::Orientation::from_exif(6));
    assert_eq!(img.oriented_size(), (48, 64));
    let exif = img.exif.expect("exif block");
    assert!(exif.starts_with(b"II") || exif.starts_with(b"MM"));
    let fields = rustnior_core::exif::fields(&exif);
    assert!(fields.iter().any(|(k, _)| k == "Orientation"), "{fields:?}");
}

#[test]
fn broken_file_is_an_error_not_a_panic() {
    assert!(matches!(
        decode("broken.jpg"),
        Err(DecodeError::Format { .. } | DecodeError::Unsupported)
    ));
}

#[test]
fn missing_file_is_io_error() {
    assert!(matches!(decode("nope.png"), Err(DecodeError::Io { .. })));
}

#[test]
fn decodes_animations() {
    for name in ["anim.gif", "anim.apng", "anim.webp"] {
        let img = decode(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(img.frames.len(), 3, "{name} frames");
        assert!(img.is_animated());
        for f in &img.frames {
            assert_eq!((f.width, f.height), (16, 16), "{name}");
            assert_eq!(
                f.delay,
                std::time::Duration::from_millis(100),
                "{name} delay"
            );
        }
        // Frames are red, lime, blue (ImageMagick's APNG is off by one or two).
        let expected = [[255u8, 0, 0], [0, 255, 0], [0, 0, 255]];
        for (f, want) in img.frames.iter().zip(expected) {
            let close = f.data[..3]
                .iter()
                .zip(want)
                .all(|(a, b)| a.abs_diff(b) <= 2);
            assert!(close, "{name}: {:?} vs {want:?}", &f.data[..3]);
        }
    }
}

#[cfg(feature = "jxl")]
#[test]
fn decodes_jpeg_xl() {
    let img = decode("still.jxl").expect("jxl");
    assert_eq!((img.source_width, img.source_height), (40, 30));
    assert_eq!(img.format_name, "JPEG XL");
}

#[cfg(feature = "heif")]
#[test]
fn decodes_heif_family() {
    for (name, format) in [("still.avif", "AVIF"), ("still.heic", "HEIF")] {
        let img = decode(name).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!((img.source_width, img.source_height), (40, 30), "{name}");
        assert_eq!(img.format_name, format);
    }
}

#[cfg(feature = "turbojpeg")]
#[test]
fn turbojpeg_preview_is_reduced_and_full_matches() {
    use std::sync::{Arc, Mutex};
    // 64×48 is too small for a worthwhile preview against a 16×12 target…
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&got);
    let req = DecodeRequest {
        target_size: Some((8, 6)),
        preview: Some(rustnior_core::decode::PreviewSink(Arc::new(move |img| {
            if let Ok(mut v) = sink.lock() {
                v.push((img.frames[0].width, img.is_preview, img.source_width));
            }
        }))),
        ..DecodeRequest::default()
    };
    let full = Decoders::default()
        .decode_path(&fixture("rgb.jpg"), &req)
        .expect("decode");
    assert_eq!((full.frames[0].width, full.is_preview), (64, false));
    // …but against an 8×6 target a 1/8-scale preview (8 px wide) arrives first.
    let previews = got.lock().map(|v| v.clone()).unwrap_or_default();
    assert_eq!(previews, vec![(8, true, 64)]);
}
