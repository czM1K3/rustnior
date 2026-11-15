# Architecture plan

## Toolkit: GTK4 (gtk4-rs), no libadwaita

| Option | Pros | Cons |
|---|---|---|
| **GTK4** | Native on Linux, GPU scene graph (GSK: GL/Vulkan) with texture scaling, mipmaps and automatic tiling of large textures; portals, DnD, clipboard, file chooser, accessibility for free | C library dependency; Windows/macOS builds possible but heavier |
| iced / egui | Pure Rust, easy cross-compile | Immediate/retained redraw of whole window, weaker native integration (file chooser, DnD, clipboard images, IME), egui repaints continuously |
| slint | Small, fast | Image-viewer niceties (GPU texture zoom, DnD of files, portals) need more custom code |

GTK4 wins: an image viewer is mostly "put a big texture on the GPU and transform it", which
GSK does well, and the desktop integration items in the feature list come built in.
libadwaita is skipped to save startup time and a dependency; the system dark/light
preference is read from the XDG settings portal instead (see DECISIONS.md).

## Workspace

```
crates/
  core/   rustnior-core  — no UI deps. decode, cache, folder scan, sort, transforms,
                           exif, config, loader pipeline. thiserror + tracing.
  app/    rustnior       — GTK4 UI + clap CLI entry point (binary `rustnior`). anyhow.
```

## Pipeline

```
 UI thread                         worker pool (core::loader)
 ─────────                         ─────────────────────────
 request(path, priority) ──job──▶  [cancel check] → probe format → decode (Decoder trait)
                                   → [cancel check] → optional preview (embedded thumb / DCT-scaled)
 ◀──LoadEvent::Preview──────────   (sent early, refined later)
 ◀──LoadEvent::Full─────────────   → insert into LRU (byte budget)
 wrap Arc<pixels> in glib::Bytes   (zero copy) → GdkMemoryTexture → GSK draws with
 scaled-texture node; rotation/flip/EXIF orientation are snapshot transforms (GPU, free).
```

* Each request carries a generation number; navigating bumps the generation, which
  cancels queued jobs and makes in-flight results be dropped (checked between stages).
* Current image = highest priority; neighbours (±1, ±2) are preloaded at low priority.
* `Decoder` trait + registry: formats are added and feature-gated independently.
* Results reach GTK through an `async-channel` awaited by a `glib::spawn_future_local` task;
  core only sees a `Send` callback.
* Startup: CLI parsing and the first decode start *before* GTK initialises, so the decode
  overlaps toolkit start-up.

## Milestones
M1 view/fit/zoom/pan · M2 folders/async/preload/LRU · M3 transforms/EXIF/fullscreen/slideshow ·
M4 animation/metadata/thumbnails · M5 file ops · M6 prefs/config/theming/shortcuts ·
M7 perf/benchmarks/packaging/docs.
