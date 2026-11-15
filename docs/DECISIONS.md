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
