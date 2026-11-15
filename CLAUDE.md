# Project: rustnios (modern Viewnior alternative)

## Goal
Build a fast, modern, lightweight image viewer in Rust that matches the
feature set of Viewnior (https://github.com/hellosiyan/Viewnior). The
priorities are startup time, image load speed, low memory use, and a clean
UI. The project name is : `rustnior`. Suggest a better name if you have one.

## Tech stack
- Language: Rust (stable, edition 2021 or newer)
- GUI: GTK4 via `gtk4-rs`, with libadwaita optional. If you think `iced`,
  `slint`, or `egui` would be faster or cleaner, explain the tradeoff
  briefly before starting, then pick one.
- Image decoding: `image` crate plus specialized decoders where they are
  faster or add features:
  - `zune-jpeg` or `turbojpeg` for JPEG
  - `libwebp` / `webp` for WebP
  - `resvg` for SVG
  - `libavif` / `ravif` / `libheif-rs` for AVIF/HEIC (feature-gated)
  - `jxl-oxide` for JPEG XL (feature-gated)
  - `gif` crate for animated GIF; animated WebP and APNG too
- EXIF: `kamadak-exif` or `rexiv2` (orientation, metadata display)
- Concurrency: `rayon` / `std::thread` plus channels. Never decode on the
  UI thread.
- Config: `serde` + `toml`, stored in the XDG config dir (`directories` crate)
- CLI: `clap`
- Target platform: Linux first (X11 and Wayland). Keep the code portable so
  Windows and macOS builds are possible later.

## Feature parity with Viewnior
Core viewing:
- Open a single image, multiple images, or a directory from the CLI
  or a file chooser, plus drag-and-drop
- Browse the other images in the same folder (next/previous, first/last)
- Natural-sort file ordering, with an option to sort by name, date, or size
- Zoom: fit to window, 100%, zoom in/out, scroll-wheel zoom, smooth zoom
- Pan by dragging, with keyboard panning
- Rotate left/right in 90° steps, flip horizontal/vertical
- Fullscreen mode with auto-hiding UI
- Slideshow with configurable interval
- Animated image playback (GIF, APNG, animated WebP)
- Respect EXIF orientation automatically
- Image properties/metadata dialog (dimensions, size, format, EXIF)

File operations:
- Save as / save a rotated or flipped copy
- Lossless JPEG rotation if feasible (otherwise re-encode with a warning)
- Delete to trash (`trash` crate), with a confirmation option
- Copy image to clipboard, copy file path
- Set as desktop wallpaper (GNOME, KDE, and generic fallbacks)
- Open containing folder

UI:
- Minimal chrome: a toggleable toolbar, status bar, and a thumbnail strip
- Dark and light themes that follow the system setting
- Configurable background color, with an optional checkerboard for transparency
- Keyboard shortcuts compatible with Viewnior's defaults, plus the
  standard ones (arrow keys, Space, Backspace, +/-, F11, Esc, Del)
- Right-click context menu
- Remember window size, position, and last settings
- A preferences dialog

## Performance requirements
- Cold start under 100 ms to first painted frame on a typical machine
- Show the first image as fast as possible. Show a downscaled preview first
  when possible (JPEG DCT scaling, embedded thumbnails), then refine to full
  resolution.
- Preload the next and previous images in the background, using a bounded
  LRU cache with a configurable memory limit
- Cancel stale decode jobs when the user navigates quickly
- GPU-accelerated rendering or scaling where the toolkit allows it. Otherwise
  use SIMD resizing with `fast_image_resize`.
- Handle very large images (100+ MP) without freezing, using tiled or
  downscaled rendering
- Add benchmarks (`criterion`) for decode and resize paths
- Avoid unnecessary allocations and copies in the decode-to-texture path

## Architecture
- Cargo workspace with separate crates, for example:
  - `core`: image loading, caching, folder scanning, and transforms, with
    no UI dependencies
  - `ui`: the GUI application
  - `cli`: argument parsing and the entry point (or fold this into `ui`)
- Define a clear async-style pipeline: request → decode → scale → upload →
  display, with cancellation
- Use traits for decoders so formats can be added or feature-gated
- Use proper error handling (`thiserror` for libraries, `anyhow` in the
  binary). No `unwrap()` in non-test code.
- Use `tracing` for logging

## Quality requirements
- `cargo clippy -- -D warnings` and `cargo fmt` must pass
- Unit tests for sorting, caching, transforms, and config parsing
- Integration tests with sample images in `tests/fixtures`
- Set up GitHub Actions CI for build, test, clippy, and fmt
- Write a README with features, build instructions, and keybindings
- Provide packaging: a `.desktop` file, an app icon, an AppStream metainfo
  file, and a Flatpak manifest

## Working process
1. First, inspect the Viewnior repo's feature list and keybindings, and
   write a short `docs/FEATURES.md` checklist of what to match.
2. Propose the architecture and toolkit choice in a short plan, then proceed
   without waiting for approval.
3. Work in milestones, committing after each one:
   - M1: Open one image, display it, fit to window, zoom and pan
   - M2: Folder navigation, async loading, preloading, LRU cache
   - M3: Rotate/flip, EXIF orientation, fullscreen, slideshow
   - M4: Animated formats, metadata dialog, thumbnail strip
   - M5: File operations, clipboard, wallpaper, trash
   - M6: Preferences, config persistence, theming, shortcuts
   - M7: Performance tuning, benchmarks, packaging, docs
4. After each milestone, make sure it builds and the tests pass. Briefly
   summarize what works and what's left.
5. Ask me only when a decision is truly blocking. Otherwise choose
   sensible defaults and note them in `docs/DECISIONS.md`.

## Non-goals
- Image editing beyond rotate/flip (no filters, painting, or cropping for now)
- Photo library management or cataloging
- Cloud integration

Start with the feature checklist and the plan, then begin M1.
