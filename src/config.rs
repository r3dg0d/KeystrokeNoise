use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub enabled: bool,
    pub volume: f32,
    pub normal_volume: f32,
    pub space_volume: f32,
    pub enter_volume: f32,
    pub modifier_volume: f32,
    pub normal_sound: String,
    pub space_sound: String,
    pub enter_sound: String,
    pub modifier_sound: String,
    /// Optional explicit input device path; empty = first keyboard-like device.
    pub device: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.35,
            normal_volume: 1.0,
            space_volume: 1.0,
            enter_volume: 1.0,
            modifier_volume: 1.0,
            normal_sound: "normal.wav".into(),
            space_sound: "space.wav".into(),
            enter_sound: "enter.wav".into(),
            modifier_sound: "modifier.wav".into(),
            device: String::new(),
        }
    }
}

impl Config {
    pub fn config_dir() -> PathBuf {
        directories::BaseDirs::new()
            .map(|b| b.config_dir().join("keystroke-noise"))
            .unwrap_or_else(|| PathBuf::from(".config/keystroke-noise"))
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn assets_dir() -> PathBuf {
        Self::config_dir().join("sounds")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            let cfg = Self::default();
            cfg.save()?;
            return Ok(cfg);
        }
        let text = fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let cfg: Self = toml::from_str(&text).context("parse config.toml")?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(Self::assets_dir())?;
        let path = Self::config_path();
        let text = toml::to_string_pretty(self)?;
        fs::write(&path, text)?;
        Ok(())
    }

    pub fn sound_path(&self, name: &str) -> PathBuf {
        let p = Path::new(name);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        Self::assets_dir().join(name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Normal,
    Space,
    Enter,
    Modifier,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => "NORMAL",
            Self::Space => "SPACE",
            Self::Enter => "ENTER",
            Self::Modifier => "MODIFIER",
        }
    }
}
