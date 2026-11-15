//! Decodes files and prints timings: `cargo run --release --example decode -- FILE...`
//! Set `TARGET=1920x1080` to also produce a preview and a reduced copy.

use std::sync::Arc;
use std::time::Instant;

use rustnior_core::decode::PreviewSink;
use rustnior_core::{DecodeRequest, Decoders};

fn main() {
    let decoders = Decoders::default();
    let target = std::env::var("TARGET").ok().and_then(|t| {
        let (w, h) = t.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    for path in std::env::args().skip(1) {
        let start = Instant::now();
        let req = DecodeRequest {
            target_size: target,
            preview: target.map(|_| {
                PreviewSink(Arc::new(move |img: rustnior_core::DecodedImage| {
                    let f = &img.frames[0];
                    println!(
                        "  preview {}x{} after {:.1} ms",
                        f.width,
                        f.height,
                        start.elapsed().as_secs_f64() * 1000.0
                    );
                }))
            }),
            ..DecodeRequest::default()
        };
        match decoders.decode_path(path.as_ref(), &req) {
            Ok(img) => println!(
                "{path}: {}x{} {} {} frame(s){} in {:.1} ms",
                img.source_width,
                img.source_height,
                img.format_name,
                img.frames.len(),
                img.reduced
                    .as_ref()
                    .map(|r| format!(", reduced {}x{}", r.width, r.height))
                    .unwrap_or_default(),
                start.elapsed().as_secs_f64() * 1000.0
            ),
            Err(e) => println!("{path}: error: {e}"),
        }
    }
}
