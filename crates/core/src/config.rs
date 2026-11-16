//! User preferences, stored as TOML in the XDG config directory.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::collection::{SortKey, SortOrder};

/// Initial zoom for each newly shown image (Viewnior's "zoom-mode").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ZoomPref {
    /// Fit large images, show small ones at 100%.
    #[default]
    Smart,
    /// Always 100%.
    Normal,
    /// Always fit, enlarging small images.
    Fit,
    /// Keep the zoom of the previous image.
    LastUsed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WheelPref {
    #[default]
    Zoom,
    Navigate,
    Scroll,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClickPref {
    /// Toggle best fit ↔ 100%.
    #[default]
    Zoom,
    Fullscreen,
    Next,
}

/// What to do with unsaved rotations when leaving an image.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModifyPref {
    #[default]
    Ask,
    Save,
    Ignore,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemePref {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SortPref {
    #[default]
    Name,
    Modified,
    Size,
}

impl From<SortPref> for SortKey {
    fn from(p: SortPref) -> Self {
        match p {
            SortPref::Name => Self::Name,
            SortPref::Modified => Self::Modified,
            SortPref::Size => Self::Size,
        }
    }
}

impl From<SortKey> for SortPref {
    fn from(k: SortKey) -> Self {
        match k {
            SortKey::Name => Self::Name,
            SortKey::Modified => Self::Modified,
            SortKey::Size => Self::Size,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct WindowState {
    pub width: i32,
    pub height: i32,
    pub maximized: bool,
    /// Size of the monitor last used, in device pixels. Lets the very first
    /// decode (started before the window exists) size its preview.
    pub monitor_width: u32,
    pub monitor_height: u32,
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1000,
            height: 700,
            maximized: false,
            monitor_width: 1920,
            monitor_height: 1080,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Config {
    pub zoom_mode: ZoomPref,
    pub fit_on_fullscreen: bool,
    pub smooth_images: bool,
    pub show_hidden: bool,
    pub sort_by: SortPref,
    pub sort_descending: bool,

    pub confirm_delete: bool,
    pub on_modified: ModifyPref,
    pub reload_on_save: bool,
    /// 1–100.
    pub jpeg_quality: u8,
    /// 0–9.
    pub png_compression: u8,

    pub wheel: WheelPref,
    pub double_click: ClickPref,
    pub slideshow_seconds: u32,

    pub theme: ThemePref,
    /// `#rrggbb` or `#rrggbbaa`; empty means the theme's window background.
    /// An alpha below `ff` makes the window see-through behind the image
    /// (needs a compositor).
    pub background: String,
    pub checkerboard: bool,
    pub show_toolbar: bool,
    pub show_statusbar: bool,
    pub show_thumbnails: bool,
    pub start_maximized: bool,

    /// Decoded-image cache budget in MiB.
    pub cache_mib: u32,
    /// Images preloaded on each side of the current one.
    pub preload: u8,

    pub window: WindowState,

    /// Overrides for keyboard shortcuts: action name (e.g. `"next"`) to
    /// accelerators in GTK syntax (e.g. `["space", "<Alt>Right"]`).
    pub shortcuts: BTreeMap<String, Vec<String>>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zoom_mode: ZoomPref::Smart,
            fit_on_fullscreen: true,
            smooth_images: true,
            show_hidden: false,
            sort_by: SortPref::Name,
            sort_descending: false,
            confirm_delete: true,
            on_modified: ModifyPref::Ask,
            reload_on_save: true,
            jpeg_quality: 90,
            png_compression: 9,
            wheel: WheelPref::Zoom,
            double_click: ClickPref::Zoom,
            slideshow_seconds: 5,
            theme: ThemePref::System,
            background: String::new(),
            checkerboard: true,
            show_toolbar: true,
            show_statusbar: true,
            show_thumbnails: false,
            start_maximized: false,
            cache_mib: 768,
            preload: 2,
            window: WindowState::default(),
            shortcuts: BTreeMap::new(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("cannot write config: {0}")]
    Write(#[from] std::io::Error),
    #[error("cannot serialise config: {0}")]
    Serialize(#[from] toml::ser::Error),
}

impl Config {
    pub const FILE_NAME: &'static str = "config.toml";

    /// Parses TOML; unknown keys are ignored and missing keys take defaults,
    /// so old and new versions can share a file.
    pub fn from_toml(s: &str) -> Result<Self, toml::de::Error> {
        toml::from_str::<Self>(s).map(Self::sanitized)
    }

    pub fn to_toml(&self) -> Result<String, toml::ser::Error> {
        toml::to_string_pretty(self)
    }

    /// Loads `path`; a missing file yields the defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(s) => Self::from_toml(&s).map_err(|source| ConfigError::Parse {
                path: path.to_owned(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(source) => Err(ConfigError::Read {
                path: path.to_owned(),
                source,
            }),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::save::write_atomic(path, self.to_toml()?.as_bytes())?;
        Ok(())
    }

    /// Clamps out-of-range values instead of rejecting the whole file.
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.jpeg_quality = self.jpeg_quality.clamp(1, 100);
        self.png_compression = self.png_compression.min(9);
        self.slideshow_seconds = self.slideshow_seconds.clamp(1, 3600);
        self.cache_mib = self.cache_mib.clamp(64, 65536);
        self.preload = self.preload.min(8);
        self.window.width = self.window.width.clamp(200, 16384);
        self.window.height = self.window.height.clamp(150, 16384);
        self.window.monitor_width = self.window.monitor_width.clamp(320, 32768);
        self.window.monitor_height = self.window.monitor_height.clamp(240, 32768);
        if !is_hex_color(&self.background) {
            self.background.clear();
        }
        self
    }

    pub fn sort_order(&self) -> SortOrder {
        SortOrder {
            key: self.sort_by.into(),
            descending: self.sort_descending,
        }
    }

    /// Background as RGBA in 0..=1, if a custom colour is set. A colour
    /// without an alpha component is opaque.
    pub fn background_rgba(&self) -> Option<(f32, f32, f32, f32)> {
        if !is_hex_color(&self.background) {
            return None;
        }
        let hex = &self.background[1..];
        let v = u32::from_str_radix(hex, 16).ok()?;
        let v = if hex.len() == 6 { v << 8 | 0xFF } else { v };
        let c = |shift: u32| ((v >> shift) & 0xFF) as f32 / 255.0;
        Some((c(24), c(16), c(8), c(0)))
    }

    /// True when the custom background lets the desktop show through.
    pub fn background_is_translucent(&self) -> bool {
        self.background_rgba().is_some_and(|(.., a)| a < 1.0)
    }
}

/// Formats RGBA in 0..=1 as `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn hex_color(r: f32, g: f32, b: f32, a: f32) -> String {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (byte(r), byte(g), byte(b), byte(a));
    if a == 0xFF {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

fn is_hex_color(s: &str) -> bool {
    matches!(s.len(), 7 | 9) && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let c = Config {
            wheel: WheelPref::Navigate,
            background: "#102030".into(),
            window: WindowState {
                maximized: true,
                ..WindowState::default()
            },
            ..Config::default()
        };
        let s = c.to_toml().expect("ser");
        assert_eq!(Config::from_toml(&s).expect("de"), c);
    }

    #[test]
    fn missing_keys_use_defaults_and_unknown_are_ignored() {
        let c = Config::from_toml("jpeg-quality = 70\nsomething-new = true\n").expect("de");
        assert_eq!(c.jpeg_quality, 70);
        assert_eq!(c.slideshow_seconds, 5);
        assert_eq!(c.on_modified, ModifyPref::Ask);
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let c = Config::from_toml(
            "jpeg-quality = 0\npng-compression = 42\nslideshow-seconds = 0\nbackground = \"red\"",
        )
        .expect("de");
        assert_eq!(
            (c.jpeg_quality, c.png_compression, c.slideshow_seconds),
            (1, 9, 1)
        );
        assert!(c.background.is_empty());
    }

    #[test]
    fn shortcut_overrides_parse() {
        let c = Config::from_toml("[shortcuts]\nnext = [\"n\", \"<Alt>Right\"]\n").expect("de");
        assert_eq!(c.shortcuts.get("next").map(Vec::len), Some(2));
    }

    #[test]
    fn enums_use_kebab_case() {
        let c = Config::from_toml("zoom-mode = \"last-used\"\ndouble-click = \"fullscreen\"")
            .expect("de");
        assert_eq!(c.zoom_mode, ZoomPref::LastUsed);
        assert_eq!(c.double_click, ClickPref::Fullscreen);
        assert!(Config::from_toml("wheel = \"sideways\"").is_err());
    }

    #[test]
    fn parses_background_colour() {
        let c = Config {
            background: "#ff8000".into(),
            ..Config::default()
        };
        let (r, g, b, a) = c.background_rgba().expect("rgba");
        assert!((r - 1.0).abs() < 1e-6 && (g - 128.0 / 255.0).abs() < 1e-6 && b == 0.0);
        assert_eq!(a, 1.0);
        assert!(!c.background_is_translucent());
        assert!(Config::default().background_rgba().is_none());
        assert!(!Config::default().background_is_translucent());
    }

    #[test]
    fn parses_translucent_background() {
        let c = Config::from_toml("background = \"#10203080\"").expect("de");
        assert_eq!(c.background, "#10203080");
        let (.., a) = c.background_rgba().expect("rgba");
        assert!((a - 128.0 / 255.0).abs() < 1e-6);
        assert!(c.background_is_translucent());
        let c = Config::from_toml("background = \"#1020300\"").expect("de");
        assert!(c.background.is_empty());
    }

    #[test]
    fn formats_hex_colours() {
        assert_eq!(hex_color(1.0, 0.5, 0.0, 1.0), "#ff8000");
        assert_eq!(hex_color(0.0, 0.0, 0.0, 0.0), "#00000000");
        assert_eq!(hex_color(2.0, -1.0, 0.0, 0.5), "#ff000080");
    }

    #[test]
    fn load_missing_file_gives_defaults_and_save_round_trips() {
        let dir = std::env::temp_dir().join(format!("rustnior-cfg-{}", std::process::id()));
        let path = dir.join("sub/config.toml");
        assert_eq!(Config::load(&path).expect("load"), Config::default());
        let c = Config {
            slideshow_seconds: 9,
            ..Config::default()
        };
        c.save(&path).expect("save");
        assert_eq!(Config::load(&path).expect("reload"), c);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
