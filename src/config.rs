//! Settings persisted to `config.toml`, editable from the TUI (`/settings`).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Library folder. When absent, the OS Music folder is used.
    pub library_dir: Option<PathBuf>,
    /// Browser for YouTube sign-in: auto | chrome | chromium | edge | brave | vivaldi | none.
    pub browser: Option<String>,
    /// Auto-update policy: auto | notify | off.
    pub autoupdate: Option<String>,
    /// Playback volume, 0.0 ..= 1.0.
    pub volume: Option<f32>,
}

impl Config {
    pub fn path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ytcut")
            .join("config.toml")
    }

    pub fn load() -> Config {
        fs::read_to_string(Self::path())
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)
                .with_context(|| format!("could not create {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self).context("could not serialize config")?;
        fs::write(&path, text).with_context(|| format!("could not write {}", path.display()))
    }

    /// Playback volume clamped to 0.0 ..= 1.0, defaulting to full.
    pub fn volume(&self) -> f32 {
        self.volume.unwrap_or(1.0).clamp(0.0, 1.0)
    }
}

/// The OS Music folder, or `~/Music`, or None.
pub fn default_music_dir() -> Option<PathBuf> {
    dirs::audio_dir().or_else(|| dirs::home_dir().map(|home| home.join("Music")))
}
