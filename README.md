# rustnior

A fast, minimal image viewer for Linux, written in Rust with GTK4. It follows
[Viewnior](https://github.com/hellosiyan/Viewnior)'s feature set and keyboard
shortcuts, with a focus on startup time, decode speed and low memory use.

## Features

- Opens instantly; the first image is decoded while GTK is still starting
- Browse the folder of the opened image (natural sort, or by date/size),
  or only the files you opened
- GPU zoom and pan: best fit, 100%, smooth animated zoom, zoom around the pointer,
  touchpad pinch and two-finger panning
- Rotate and flip; JPEGs are saved **losslessly** (EXIF orientation), other formats
  re-encoded; EXIF orientation is always respected
- Fullscreen with auto-hiding controls, slideshow with adjustable interval
- Animated GIF, APNG, WebP and JPEG XL
- Image properties with EXIF metadata
- Thumbnail strip using the shared freedesktop thumbnail cache
- Move to trash, copy image or path, set as wallpaper (GNOME, KDE, Cinnamon, MATE,
  XFCE, LXDE/LXQt, sway, Hyprland, portal, feh/nitrogen), open with, show in folder
- Right-click menu, preferences, light/dark theme following the system,
  configurable background and transparency checkerboard
- Very large images (100+ MP) stay responsive: screen-sized copy when zoomed out,
  on-demand tiles when zoomed in
- Preloads neighbouring images with a memory-bounded cache and cancels stale work

**Formats:** JPEG, PNG/APNG, GIF, WebP, TIFF, BMP, ICO, TGA, PNM, QOI, HDR, OpenEXR,
DDS, farbfeld, SVG/SVGZ, JPEG XL, and AVIF/HEIC/HEIF via libheif.

## Building

Requirements: Rust ≥ 1.92, GTK ≥ 4.14, and for the default features
libjpeg-turbo and libheif (≥ 1.17).

```sh
# Debian/Ubuntu
sudo apt install libgtk-4-dev libturbojpeg0-dev libheif-dev
# Fedora
sudo dnf install gtk4-devel turbojpeg-devel libheif-devel
# Arch
sudo pacman -S gtk4 libjpeg-turbo libheif

cargo build --release
./target/release/rustnior ~/Pictures
```

| Cargo feature | default | what it adds |
|---|---|---|
| `turbojpeg` | yes | faster JPEG decoding and quick scaled previews (system libjpeg-turbo) |
| `heif` | yes | HEIC/HEIF and AVIF (system libheif) |
| `jxl` | yes | JPEG XL (pure Rust) |

`cargo build --release --no-default-features` builds a binary without the
optional C libraries.

### Installing

```sh
make            # cargo build --release --locked
sudo make install PREFIX=/usr/local
```

This installs the binary, the `.desktop` file, the AppStream metainfo and the
icons. A Flatpak manifest is in `build-aux/` (generate `cargo-sources.json` with
`make flatpak-sources` first).

## Usage

```
rustnior [--fullscreen] [--slideshow] [FILES|FOLDERS]...
```

One file opens its whole folder at that file; one folder opens its images; several
paths limit browsing to those files (and the contents of any folders).

## Keyboard shortcuts

| Action | Keys |
|---|---|
| Next / previous image | `Space` / `Backspace`, `Page Down` / `Page Up`, `Alt+→` / `Alt+←` |
| First / last image | `Home` / `End` |
| Arrows | switch images when the whole image is visible, otherwise pan (`Shift` pans a page) |
| Zoom in / out | `+` `=` / `-` (also with `Ctrl` and keypad) |
| Best fit · 100% · 200% · 300% | `F` or `0` · `1` or `Ctrl+0` · `2` · `3` |
| Rotate clockwise / anticlockwise | `Ctrl+R` or `R` / `Ctrl+Shift+R` or `L` |
| Flip horizontally / vertically | `H` / `V` |
| Fullscreen | `F11` |
| Slideshow | `F5` |
| Leave fullscreen or quit | `Esc`, `Q` |
| Pause animation / next frame | `P` / `J` |
| Open image / folder | `Ctrl+O` / `Ctrl+F` |
| Save / save as | `Ctrl+S` / `Ctrl+Shift+S` |
| Move to trash | `Delete` |
| Copy image / path | `Ctrl+C` / `Ctrl+Shift+C` |
| Set as wallpaper | `Ctrl+F8` |
| Properties | `Alt+Enter` |
| Toolbar / thumbnails | `Ctrl+H` / `T` |
| Preferences · shortcuts | `Ctrl+,` · `F1` |
| Close window | `Ctrl+W` |

Mouse: the wheel zooms (Shift+wheel or tilting navigates; configurable),
`Ctrl`+wheel always zooms, drag pans, double-click toggles fit/100%, right-click
opens the menu.

## Configuration

Settings are changed in Preferences and stored in
`~/.config/rustnior/config.toml`. Any shortcut can be overridden there:

```toml
background = "#1e1e1e"
wheel = "navigate"

[shortcuts]
next = ["n", "space"]
prev = ["b", "BackSpace"]
```

## Development

```sh
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
cargo bench -p rustnior-core --all-features
RUSTNIOR_TIMING=1 rustnior photo.jpg                     # startup timings
cargo run -p rustnior-core --example decode -- FILE      # decode timings
```

The workspace has two crates: `crates/core` (decoding, caching, folder scanning,
transforms, config; no UI dependencies) and `crates/app` (the GTK UI and CLI).
Design notes live in `docs/` (`PLAN.md`, `DECISIONS.md`, `FEATURES.md`,
`PERFORMANCE.md`).

## License

GPL-3.0-or-later. rustnior is a new implementation; it shares no code with
Viewnior, whose behaviour and shortcuts it follows.
