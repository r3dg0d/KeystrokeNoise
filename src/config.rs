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

/// Whether a config hot-reload must rebuild the input device set.
///
/// Only `device` and `mouse_enabled` choose which evdev nodes are opened.
/// Volume and `enabled` are read on each play. Sound files are re-read only
/// when their paths change (`sound_banks_reload_required`). None of those
/// reopen keyboards or mice. This does not touch `/dev/input`.
pub fn input_rescan_required(previous: &Config, next: &Config) -> bool {
    previous.device != next.device || previous.mouse_enabled != next.mouse_enabled
}

/// Whether a config hot-reload must re-read WAV banks.
///
/// Only the `*_sound` fields choose which files are loaded. Volume and
/// `enabled` are applied in `AudioEngine::play` from the live config and must
/// not reopen PipeWire or re-read banks. This does not open an audio device.
pub fn sound_banks_reload_required(previous: &Config, next: &Config) -> bool {
    previous.normal_sound != next.normal_sound
        || previous.space_sound != next.space_sound
        || previous.enter_sound != next.enter_sound
        || previous.modifier_sound != next.modifier_sound
        || previous.mouse_left_sound != next.mouse_left_sound
        || previous.mouse_middle_sound != next.mouse_middle_sound
        || previous.mouse_right_sound != next.mouse_right_sound
}

/// Config to keep when `AudioEngine::reload` fails.
///
/// The incoming `*_sound` paths were not loaded, so they must not replace the
/// paths that still name the banks in memory. Volume, `enabled`, `device`, and
/// `mouse_enabled` from `incoming` still apply. Does not open a device or read
/// a WAV. Because the stored paths stay on the previous files, the next watch
/// of the same bad config still reports `sound_banks_reload_required`.
pub fn config_after_failed_bank_reload(previous: &Config, mut incoming: Config) -> Config {
    incoming.normal_sound = previous.normal_sound.clone();
    incoming.space_sound = previous.space_sound.clone();
    incoming.enter_sound = previous.enter_sound.clone();
    incoming.modifier_sound = previous.modifier_sound.clone();
    incoming.mouse_left_sound = previous.mouse_left_sound.clone();
    incoming.mouse_middle_sound = previous.mouse_middle_sound.clone();
    incoming.mouse_right_sound = previous.mouse_right_sound.clone();
    incoming
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

    #[test]
    fn input_rescan_only_when_device_or_mouse_enabled_changes() {
        let prev = Config::default();
        assert!(
            !input_rescan_required(&prev, &prev),
            "identical config must not reopen inputs"
        );

        let mut audio_only = prev.clone();
        audio_only.volume = 0.1;
        audio_only.normal_volume = 0.2;
        audio_only.space_volume = 0.3;
        audio_only.enter_volume = 0.4;
        audio_only.modifier_volume = 0.5;
        audio_only.mouse_volume = 0.6;
        audio_only.enabled = false;
        audio_only.normal_sound = "other.wav".into();
        audio_only.space_sound = "s.wav".into();
        audio_only.enter_sound = "e.wav".into();
        audio_only.modifier_sound = "m.wav".into();
        audio_only.mouse_left_sound = "ml.wav".into();
        audio_only.mouse_middle_sound = "mm.wav".into();
        audio_only.mouse_right_sound = "mr.wav".into();
        assert!(
            !input_rescan_required(&prev, &audio_only),
            "audio-only reload must not reopen inputs"
        );

        let mut mouse_off = prev.clone();
        mouse_off.mouse_enabled = false;
        assert!(input_rescan_required(&prev, &mouse_off));
        assert!(input_rescan_required(&mouse_off, &prev));

        let mut explicit = prev.clone();
        explicit.device = "/dev/input/event0".into();
        assert!(
            input_rescan_required(&prev, &explicit),
            "auto → explicit device"
        );
        let mut other = explicit.clone();
        other.device = "/dev/input/event1".into();
        assert!(
            input_rescan_required(&explicit, &other),
            "device path change"
        );
        assert!(input_rescan_required(&explicit, &prev), "explicit → auto");

        let mut same_selection = explicit.clone();
        same_selection.volume = 0.01;
        same_selection.enabled = false;
        same_selection.mouse_enabled = true;
        assert!(
            !input_rescan_required(&explicit, &same_selection),
            "same device and mouse_enabled"
        );

        let mut both = other.clone();
        both.mouse_enabled = false;
        assert!(input_rescan_required(&explicit, &both));
    }

    #[test]
    fn sound_banks_reload_only_when_a_sound_path_changes() {
        let prev = Config::default();
        assert!(
            !sound_banks_reload_required(&prev, &prev),
            "identical config must not reload banks"
        );

        let mut knobs = prev.clone();
        knobs.volume = 0.1;
        knobs.normal_volume = 0.2;
        knobs.space_volume = 0.3;
        knobs.enter_volume = 0.4;
        knobs.modifier_volume = 0.5;
        knobs.mouse_volume = 0.6;
        knobs.enabled = false;
        knobs.mouse_enabled = false;
        knobs.device = "/dev/input/event3".into();
        assert!(
            !sound_banks_reload_required(&prev, &knobs),
            "volume, enabled, device, and mouse_enabled must not reload banks"
        );

        let mut next = prev.clone();
        next.normal_sound = "other-normal.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.space_sound = "other-space.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.enter_sound = "other-enter.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.modifier_sound = "other-modifier.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.mouse_left_sound = "other-left.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.mouse_middle_sound = "other-middle.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
        next = prev.clone();
        next.mouse_right_sound = "other-right.wav".into();
        assert!(sound_banks_reload_required(&prev, &next));
    }

    #[test]
    fn failed_bank_reload_keeps_previous_sound_paths() {
        let prev = Config::default();
        let mut incoming = prev.clone();
        incoming.volume = 0.2;
        incoming.enabled = false;
        incoming.mouse_enabled = false;
        incoming.device = "/dev/input/event9".into();
        incoming.normal_sound = "missing-normal.wav".into();
        incoming.space_sound = "missing-space.wav".into();
        incoming.enter_sound = "missing-enter.wav".into();
        incoming.modifier_sound = "missing-modifier.wav".into();
        incoming.mouse_left_sound = "missing-left.wav".into();
        incoming.mouse_middle_sound = "missing-middle.wav".into();
        incoming.mouse_right_sound = "missing-right.wav".into();

        let kept = config_after_failed_bank_reload(&prev, incoming.clone());
        assert_eq!(kept.normal_sound, prev.normal_sound);
        assert_eq!(kept.space_sound, prev.space_sound);
        assert_eq!(kept.enter_sound, prev.enter_sound);
        assert_eq!(kept.modifier_sound, prev.modifier_sound);
        assert_eq!(kept.mouse_left_sound, prev.mouse_left_sound);
        assert_eq!(kept.mouse_middle_sound, prev.mouse_middle_sound);
        assert_eq!(kept.mouse_right_sound, prev.mouse_right_sound);
        assert_eq!(kept.volume, 0.2);
        assert!(!kept.enabled);
        assert!(!kept.mouse_enabled);
        assert_eq!(kept.device, "/dev/input/event9");
        assert!(
            !sound_banks_reload_required(&prev, &kept),
            "kept paths must match the banks still loaded"
        );
        assert!(
            sound_banks_reload_required(&kept, &incoming),
            "the bad file must still look like a pending reload"
        );
    }
}
