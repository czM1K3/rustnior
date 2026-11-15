//! Decode → resize → transform hot paths.
//!
//! `cargo bench -p rustnior-core` (add `--features turbojpeg` to include
//! libjpeg-turbo). Inputs are generated, so no large fixtures are needed.

use std::hint::black_box;
use std::io::Cursor;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgb, Rgba};
use rustnior_core::natsort::natural_cmp_total;
use rustnior_core::resize::{fit_size, resize_frame};
use rustnior_core::{DecodeRequest, Decoders, Frame, Orientation, PixelFormat};

/// Photo-like content: smooth gradients plus a little deterministic noise.
fn photo(w: u32, h: u32) -> ImageBuffer<Rgb<u8>, Vec<u8>> {
    let mut seed = 0x1234_5678_u32;
    ImageBuffer::from_fn(w, h, |x, y| {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let n = (seed >> 28) as u8;
        let r = (x * 255 / w) as u8;
        let g = (y * 255 / h) as u8;
        let b = (((x + y) / 7) % 256) as u8;
        Rgb([
            r.saturating_add(n),
            g.saturating_add(n),
            b.saturating_sub(n),
        ])
    })
}

fn encode(img: DynamicImage, format: ImageFormat) -> Vec<u8> {
    let mut out = Vec::new();
    img.write_to(&mut Cursor::new(&mut out), format)
        .expect("encode");
    out
}

fn decode(c: &mut Criterion) {
    let decoders = Decoders::default();
    let mut g = c.benchmark_group("decode");
    g.sample_size(20);
    for (w, h) in [(1920, 1080), (4000, 3000)] {
        let jpeg = encode(DynamicImage::ImageRgb8(photo(w, h)), ImageFormat::Jpeg);
        g.throughput(Throughput::Elements(u64::from(w * h)));
        let id = format!("{w}x{h}");
        g.bench_with_input(BenchmarkId::new("jpeg-default", &id), &jpeg, |b, data| {
            b.iter(|| {
                decoders.decode_bytes(black_box(data), Some("jpg"), &DecodeRequest::default())
            });
        });
        g.bench_with_input(BenchmarkId::new("jpeg-zune", &id), &jpeg, |b, data| {
            b.iter(|| {
                let mut d = zune_jpeg::JpegDecoder::new(zune_core::bytestream::ZCursor::new(
                    black_box(data),
                ));
                d.decode()
            });
        });
        #[cfg(feature = "turbojpeg")]
        for denom in [1usize, 2, 4, 8] {
            g.bench_with_input(
                BenchmarkId::new(format!("jpeg-turbo-1/{denom}"), &id),
                &jpeg,
                |b, data| {
                    b.iter(|| {
                        let mut d = turbojpeg::Decompressor::new().expect("tj");
                        let factor = turbojpeg::ScalingFactor::new(1, denom);
                        d.set_scaling_factor(factor).expect("scale");
                        let hd = d.read_header(data).expect("header").scaled(factor);
                        let mut buf = vec![0u8; hd.width * hd.height * 3];
                        let img = turbojpeg::Image {
                            pixels: buf.as_mut_slice(),
                            width: hd.width,
                            pitch: hd.width * 3,
                            height: hd.height,
                            format: turbojpeg::PixelFormat::RGB,
                        };
                        d.decompress(black_box(data), img).expect("decode");
                        buf
                    });
                },
            );
        }
    }
    let rgba = ImageBuffer::from_fn(2000, 1500, |x, y| {
        Rgba([(x % 256) as u8, (y % 256) as u8, 90, ((x + y) % 256) as u8])
    });
    let png = encode(DynamicImage::ImageRgba8(rgba), ImageFormat::Png);
    g.throughput(Throughput::Elements(2000 * 1500));
    g.bench_function("png-rgba/2000x1500", |b| {
        b.iter(|| decoders.decode_bytes(black_box(&png), Some("png"), &DecodeRequest::default()));
    });
    g.finish();
}

fn resize(c: &mut Criterion) {
    let src =
        Frame::new(4000, 3000, PixelFormat::Rgb8, photo(4000, 3000).into_raw()).expect("frame");
    let mut g = c.benchmark_group("resize");
    g.sample_size(20);
    for (tw, th) in [(1920, 1080), (256, 256)] {
        let (w, h) = fit_size(4000, 3000, tw, th);
        g.bench_function(format!("rgb-12mp-to-{w}x{h}"), |b| {
            b.iter(|| resize_frame(black_box(&src), w, h));
        });
    }
    g.finish();
}

fn transform(c: &mut Criterion) {
    let src =
        Frame::new(4000, 3000, PixelFormat::Rgb8, photo(4000, 3000).into_raw()).expect("frame");
    let mut g = c.benchmark_group("transform");
    g.sample_size(10);
    g.bench_function("rotate-cw-12mp", |b| {
        b.iter(|| Orientation::new(1, false).apply_to_frame(black_box(&src)));
    });
    g.finish();
}

fn sort(c: &mut Criterion) {
    let names: Vec<String> = (0..10_000)
        .map(|i| format!("IMG_{:04}_{}.JPG", (i * 7919) % 10_000, i % 13))
        .collect();
    c.bench_function("natural-sort-10k", |b| {
        b.iter(|| {
            let mut v: Vec<&str> = names.iter().map(String::as_str).collect();
            v.sort_by(|a, b| natural_cmp_total(a, b));
            v
        });
    });
}

criterion_group!(benches, decode, resize, transform, sort);
criterion_main!(benches);
