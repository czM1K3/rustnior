//! Locating, loading and saving the config file.

use std::path::PathBuf;

use rustnior_core::Config;

/// `$XDG_CONFIG_HOME/rustnior/config.toml` (or the platform equivalent).
pub fn config_path() -> Option<PathBuf> {
    directories::ProjectDirs::from("io.github", "rustnior", "rustnior")
        .map(|d| d.config_dir().join(Config::FILE_NAME))
}

/// Loads the config, falling back to defaults (with a warning) if the file
/// is unreadable or invalid, so a typo never prevents the viewer from starting.
pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    Config::load(&path).unwrap_or_else(|err| {
        tracing::warn!(%err, "using default settings");
        Config::default()
    })
}

pub fn save(config: &Config) {
    let Some(path) = config_path() else { return };
    if let Err(err) = config.save(&path) {
        tracing::warn!(%err, "could not save settings");
    }
}
