# Performance notes

## Startup

The first decode starts on a thread right after argument parsing, *before* GTK
initialises, so decoding overlaps toolkit start-up. The window is presented
before its folder is scanned (the scan runs on a worker), and the config file
is read in well under a millisecond.

Measured with `RUSTNIOR_TIMING=1` (release build, headless sway, software
renderer, stock Adwaita icon theme):

| milestone | time |
|---|---|
| GTK application started | ~27 ms |
| window presented | ~37 ms |
| **first frame painted** | **~60–70 ms** |
| first image frame, small JPEG | same frame |
| first image frame, 6 MP JPEG | ~125 ms |
| first image frame, 24 MP JPEG (1/4-scale preview first) | ~175 ms |
| first image frame, 108 MP PNG | ~650 ms (PNG decoding) |

Environment matters more than the app here. GTK loads the icon theme while the
application starts; with a very large theme (Papirus: ~300k files) that alone
adds ~475 ms to *every* GTK4 application, rustnior included. A GPU renderer's
first-time initialisation (GL/Vulkan) also adds to the first frame on some systems.

## Decode → display pipeline

* **Zero copies after decoding**: decoders write into a `Vec<u8>` that becomes
  an `Arc`; the cache and the GDK texture (`glib::Bytes::from_owned`) share it.
  Opaque images stay 3 bytes/pixel and grayscale JPEGs 1 byte/pixel.
* **GPU scaling**: zoom, rotation and flipping are GSK transforms on a scaled
  texture node (trilinear/mipmapped when zoomed out). Pixels are never resampled
  on the CPU to display them.
* **Preview first** (`turbojpeg` feature): for the image being opened, JPEGs much
  larger than the screen are first decoded at 1/2, 1/4 or 1/8 scale with
  libjpeg-turbo's DCT scaling, shown, then replaced in place by the full decode.
* **Reduced copy**: stills over 8 MP that are much larger than the monitor get a
  monitor-sized copy (SIMD area-averaging resize, ~7 ms for 12 MP), so
  fitted display never uploads hundreds of megabytes to the GPU.
* **Tiles**: images over 40 MP (or wider/taller than 8192 px) are cut into
  2048 px tiles that share the decoded buffer (sub-range + stride, no copy). Only
  tiles on screen are created and uploaded; others are released. Tiles overlap by
  one pixel so linear filtering is seamless; rendering at 100% was verified
  pixel-exact across tile borders, including rotated.
* **Preloading** of ±2 neighbours (configurable) into a byte-bounded LRU cache
  (768 MiB default) on 2–4 worker threads; navigating cancels queued work and
  tells in-flight decoders to stop at their next checkpoint.
* **Thumbnails** reuse the freedesktop thumbnail cache, and only visible strip
  items are requested (off-screen requests are cancelled while scrolling).

## Benchmarks

`cargo bench -p rustnior-core --features turbojpeg` (criterion). Typical results
on a 12-thread desktop:

| benchmark | time |
|---|---|
| JPEG 12 MP, zune-jpeg | 47 ms |
| JPEG 12 MP, libjpeg-turbo | 39 ms |
| JPEG 12 MP, libjpeg-turbo at 1/2 · 1/4 · 1/8 | 28 · 24 · 19 ms |
| JPEG 2 MP, libjpeg-turbo | 6.4 ms |
| PNG RGBA 3 MP | 24 ms |
| resize 12 MP → 1440×1080 | 7.3 ms |
| rotate 12 MP (save path only) | 53 ms |
| natural sort, 10 000 names | 8.6 ms |

Entropy decoding dominates JPEG cost, which is why a 1/8-scale decode is "only"
2× faster than a full one.

`cargo run --release -p rustnior-core --example decode -- FILE…` prints
per-file decode timings (`TARGET=1920x1080` adds the preview and reduced copy).
