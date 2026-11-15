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
