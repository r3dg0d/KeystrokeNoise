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
    pub mouse_volume: f32,
    pub normal_sound: String,
    pub space_sound: String,
    pub enter_sound: String,
    pub modifier_sound: String,
    pub mouse_left_sound: String,
    pub mouse_middle_sound: String,
    pub mouse_right_sound: String,
    /// Play sounds for mouse button presses.
    pub mouse_enabled: bool,
    /// Optional explicit input device path; empty = auto-discover keyboards (+ mice when enabled).
    pub device: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            volume: 0.55,
            normal_volume: 1.0,
            space_volume: 1.0,
            enter_volume: 1.0,
            modifier_volume: 1.0,
            mouse_volume: 0.85,
            normal_sound: "normal.wav".into(),
            space_sound: "space.wav".into(),
            enter_sound: "enter.wav".into(),
            modifier_sound: "modifier.wav".into(),
            mouse_left_sound: "mouse-left.wav".into(),
            mouse_middle_sound: "mouse-middle.wav".into(),
            mouse_right_sound: "mouse-right.wav".into(),
            mouse_enabled: true,
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
        Ok(cfg.sanitized())
    }

    /// Replace non-finite volumes (TOML allows `nan` / `inf`) with the defaults. Range is
    /// enforced where the sound is played; a NaN would slip through `clamp` unchanged.
    pub fn sanitized(mut self) -> Self {
        let d = Self::default();
        let fix = |v: f32, default: f32| if v.is_finite() { v } else { default };
        self.volume = fix(self.volume, d.volume);
        self.normal_volume = fix(self.normal_volume, d.normal_volume);
        self.space_volume = fix(self.space_volume, d.space_volume);
        self.enter_volume = fix(self.enter_volume, d.enter_volume);
        self.modifier_volume = fix(self.modifier_volume, d.modifier_volume);
        self.mouse_volume = fix(self.mouse_volume, d.mouse_volume);
        self
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

    /// Primary WAVs that `init` / the daemon seed into the user sounds dir.
    /// Must cover every default `*_sound` filename — AudioEngine::new fails if any
    /// primary (including mouse) is missing from the sounds directory.
    pub fn packaged_primary_sounds() -> &'static [&'static str] {
        &[
            "normal.wav",
            "space.wav",
            "enter.wav",
            "modifier.wav",
            "mouse-left.wav",
            "mouse-middle.wav",
            "mouse-right.wav",
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    Normal,
    Space,
    Enter,
    Modifier,
    MouseLeft,
    MouseMiddle,
    MouseRight,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_round_trip_through_toml() {
        let cfg = Config::default();
        let text = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.volume, cfg.volume);
        assert_eq!(back.normal_sound, cfg.normal_sound);
        assert!(back.enabled && back.mouse_enabled);
    }

    #[test]
    fn a_partial_config_fills_in_defaults() {
        let cfg: Config = toml::from_str("volume = 0.2\nenabled = false\n").unwrap();
        assert_eq!(cfg.volume, 0.2);
        assert!(!cfg.enabled);
        assert_eq!(cfg.space_sound, "space.wav");
        assert!(cfg.mouse_enabled);
    }

    #[test]
    fn unknown_or_mistyped_fields_are_reported_not_silently_used() {
        assert!(toml::from_str::<Config>("volume = \"loud\"").is_err());
    }

    #[test]
    fn non_finite_volumes_fall_back_to_defaults() {
        let cfg: Config =
            toml::from_str("volume = nan\nmouse_volume = inf\nnormal_volume = 0.5\n").unwrap();
        let cfg = cfg.sanitized();
        let d = Config::default();
        assert_eq!(cfg.volume, d.volume);
        assert_eq!(cfg.mouse_volume, d.mouse_volume);
        assert_eq!(cfg.normal_volume, 0.5, "finite values are left alone");
    }

    #[test]
    fn relative_sound_names_resolve_under_the_sounds_dir_and_absolute_ones_are_kept() {
        let cfg = Config::default();
        assert!(cfg
            .sound_path("click.wav")
            .starts_with(Config::assets_dir()));
        assert_eq!(
            cfg.sound_path("/opt/x/click.wav"),
            PathBuf::from("/opt/x/click.wav")
        );
    }

    #[test]
    fn packaged_primaries_cover_every_default_sound_field() {
        let cfg = Config::default();
        let seeds: std::collections::HashSet<_> =
            Config::packaged_primary_sounds().iter().copied().collect();
        for name in [
            cfg.normal_sound.as_str(),
            cfg.space_sound.as_str(),
            cfg.enter_sound.as_str(),
            cfg.modifier_sound.as_str(),
            cfg.mouse_left_sound.as_str(),
            cfg.mouse_middle_sound.as_str(),
            cfg.mouse_right_sound.as_str(),
        ] {
            assert!(
                seeds.contains(name),
                "packaged_primary_sounds is missing {name}; init would leave AudioEngine unable to load it"
            );
        }
    }

    #[test]
    fn packaged_primaries_exist_in_repo_assets() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets");
        for name in Config::packaged_primary_sounds() {
            assert!(
                root.join(name).is_file(),
                "repo assets/{name} missing — seed copy has nothing to install"
            );
        }
    }
}
