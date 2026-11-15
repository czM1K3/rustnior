# Decisions log

Defaults chosen without asking, per the working process in CLAUDE.md.

- **Name**: kept `rustnior` (binary, crates, app id `io.github.rustnior.Rustnior`).
  A suggestion if you want something more distinctive: *Oxview*. Renaming is a
  search-and-replace plus the app id.
- **Toolkit**: GTK4 via gtk4-rs, without libadwaita (startup time, fewer deps). See PLAN.md.
- **Minimum GTK**: 4.14 (`gnome_46` feature): needed for `append_scaled_texture`
  filters and current GSK renderers; covers Ubuntu 24.04+, Fedora 40+, Flatpak GNOME 46+.
- **Edition**: 2024 (already in the scaffold; toolchain is 1.99).
- **Single instance**: no. Like Viewnior each invocation opens its own window;
  `NON_UNIQUE` also skips D-Bus name registration at startup.
- **Pixel formats**: opaque images stay RGB8 (25% less memory than RGBA); textures are
  created from the decoded buffer without copying (`glib::Bytes::from_owned`).
- **Transforms**: rotation/flip/EXIF orientation are a dihedral-group value (8 states)
  applied at draw time, never by touching pixels, until the user saves.
- **Decoders**: pure Rust by default (`image` w/ zune-jpeg, png, gif, webp; resvg),
  native libraries behind features (`heif`, `avif`, `jxl`, `turbojpeg`).
- **Status bar**: shown by default (Viewnior hides it); toggle in the menu.
- **Wheel / double-click defaults**: as Viewnior — wheel zooms, Shift/horizontal wheel
  navigates, Ctrl+wheel always zooms; double-click toggles best fit ↔ 100%.
  Touchpads pan with two fingers and zoom with Ctrl or pinch.
- **Preloading**: ±2 images around the current one, forward first; 768 MiB decoded-image
  cache (configurable in M6). 2–4 decode threads.
- **Folder scanning** runs on a worker thread; a single opened file is shown before
  its folder has been listed.
- **No folder recursion**, matching Viewnior.
- **Extra keys**: `R` rotates clockwise and `L` anticlockwise in addition to Viewnior's
  `Ctrl+R` / `Ctrl+Shift+R`.
- **Fullscreen** hides the header and status bar and shows a floating OSD toolbar
  (prev/next, slideshow toggle + interval, fit, rotate, leave) that hides with the
  cursor after 2 s without pointer movement. Leaving fullscreen stops the slideshow.
- **Slideshow** wraps around at the end; manual navigation restarts the countdown.
- **Animations** are decoded fully (composited RGBA frames) up to 1 GiB of pixels;
  bigger ones show their first frame as a still. Delays under 20 ms play at 100 ms,
  as browsers do. Loop count is ignored (always loops).
- **Thumbnails** use the freedesktop.org thumbnail cache (`~/.cache/thumbnails`):
  existing thumbnails from file managers are reused when `Thumb::MTime` matches, and
  new ones are written (0600, atomic rename) when the image was actually reduced.
  The strip is hidden by default; toggle with `T` or the menu.
- **Lossless JPEG rotation** is done by rewriting the EXIF orientation tag (patched in
  place, or an EXIF block is inserted/rebuilt), so JPEG scan data never changes. Every
  mainstream viewer, browser and photo tool honours the tag. Other formats are
  re-encoded with the pixels transformed; animations cannot be rotated-and-saved.
  `Save As` to a JPEG from a JPEG is also lossless; other targets are encoded by
  extension (JPEG quality and PNG compression from preferences, alpha flattened on
  white for JPEG).
- **Unsaved rotations** follow Viewnior's "behavior-modify" (ask by default) when
  navigating, opening, or closing the window. During a slideshow they are discarded.
- **Copy Path** puts both a file reference (pastes as a file in file managers) and the
  plain path (pastes as text) on the clipboard.
- **Wallpaper**: desktop-specific commands for GNOME/Unity/Budgie, Cinnamon, MATE,
  XFCE, KDE Plasma, LXDE, LXQt, sway and Hyprland, then the XDG wallpaper portal (used
  first inside Flatpak), then `feh`/`nitrogen`.
- **Crop** from Viewnior is intentionally left out (non-goal: editing).
- **Config** lives in `$XDG_CONFIG_HOME/rustnior/config.toml` (via `directories`). It
  is loaded before GTK starts, unknown keys are ignored, missing keys default and
  out-of-range values are clamped, so a bad edit never stops the viewer from opening.
  Preferences are saved immediately on change; window size/maximised state and view
  toggles are saved on close.
- **Window position is not restored**: GTK4 has no API to place toplevel windows (and
  Wayland forbids it). Size and maximised state are restored.
- **Shortcuts** are a single table (`actions::SHORTCUTS`) that drives the accelerators
  and the Shortcuts window; `[shortcuts]` in the config overrides any action's keys.
  Extra keys: `Ctrl+,` preferences, `F1`/`Ctrl+?` shortcuts, `Ctrl+H` toolbar,
  `T` thumbnails.
- **Theme**: on GTK ≥ 4.20 the app sets `gtk-interface-color-scheme` (GTK then follows
  the desktop itself for "System"); on older GTK it reads `color-scheme` from the XDG
  settings portal and follows changes live. Themes that only ship a dark (or light)
  variant, or a `GTK_THEME` environment override, take precedence — as for any GTK app.
- **Preferences** use a `Stack` + `StackSwitcher` instead of a `Notebook`. Known
  upstream noise: GTK 4.24 prints "GtkImage reported baselines…" warnings for every
  `GtkSwitch` (reproduced with a bare switch and stock Adwaita); harmless.
- **Default features** of the app: `turbojpeg` (system libjpeg-turbo: ~20% faster
  full JPEG decodes in benchmarks, plus DCT-scaled previews), `jxl` (pure Rust) and
  `heif` (system libheif ≥ 1.17, for HEIC and AVIF). `--no-default-features` gives
  a binary without optional C libraries. The core crate keeps all of them opt-in.
- **Preview first** only for the image being opened (loader priority 0). A preview
  may be up to 25% smaller than the fitted size, since it is on screen briefly.
  The very first image uses the monitor size remembered in the config, because it
  is decoded before any window exists.
- **Large images**: stills ≥ 8 MP and ≥ 1.5× the monitor get a monitor-sized copy
  for zoomed-out display; images > 40 MP or > 8192 px on a side are drawn as
  2048 px zero-copy tiles with 1 px overlap. Thresholds are constants in
  `core::decode` and `app::texture`.
- **Dev profile** optimises `rustnior-core` too: generic decode/resize code from
  dependencies is instantiated there, and unoptimised it is 10–50× slower.
- **License**: GPL-3.0-or-later (Viewnior's licence family; no Viewnior code is used).
- **Startup target**: < 100 ms to first frame is met with a normal icon theme
  (~60–70 ms measured). Very large icon themes (e.g. Papirus) cost ~0.5 s inside
  GTK's own application start-up for every GTK4 app; see PERFORMANCE.md.
- **Folder watching**: the browsed folder is monitored (GIO); creations, deletions
  and renames trigger a debounced (400 ms) rescan that keeps the current image
  selected; modified files are re-decoded. Only single-folder collections are watched.
- **Adjust window size** (menu): sizes the window for 100% within 90% of the
  monitor; the header bar's minimum width can make it wider than small images.
