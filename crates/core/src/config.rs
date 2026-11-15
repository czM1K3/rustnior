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
}

impl Default for WindowState {
    fn default() -> Self {
        Self {
            width: 1000,
            height: 700,
            maximized: false,
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
    /// `#rrggbb`; empty means the theme's window background.
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

    /// Background as RGB in 0..=1, if a custom colour is set.
    pub fn background_rgb(&self) -> Option<(f32, f32, f32)> {
        if !is_hex_color(&self.background) {
            return None;
        }
        let v = u32::from_str_radix(&self.background[1..], 16).ok()?;
        let c = |shift: u32| ((v >> shift) & 0xFF) as f32 / 255.0;
        Some((c(16), c(8), c(0)))
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
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
        let (r, g, b) = c.background_rgb().expect("rgb");
        assert!((r - 1.0).abs() < 1e-6 && (g - 128.0 / 255.0).abs() < 1e-6 && b == 0.0);
        assert!(Config::default().background_rgb().is_none());
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
