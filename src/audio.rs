use crate::config::{Category, Config};
use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::io::BufReader;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AudioEngine {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    gate: Arc<Mutex<()>>,
    normal: Vec<Vec<u8>>,
    space: Vec<Vec<u8>>,
    enter: Vec<Vec<u8>>,
    modifier: Vec<Vec<u8>>,
    mouse_left: Vec<Vec<u8>>,
    mouse_middle: Vec<Vec<u8>>,
    mouse_right: Vec<Vec<u8>>,
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

fn pick(bank: &[Vec<u8>], seed: u64) -> &[u8] {
    if bank.is_empty() {
        return &[];
    }
    let i = (seed.wrapping_mul(0x9E3779B97F4A7C15) >> 32) as usize % bank.len();
    &bank[i]
}

fn jitter_speed(seed: u64) -> f32 {
    let x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let r = ((x >> 33) as f32) / (u32::MAX as f32);
    0.98 + r * 0.04
}

fn load_file(path: &PathBuf) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("load sound {}", path.display()))
}

fn load_bank(cfg: &Config, primary: &str, subdir: &str, glob_prefix: &str) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let primary_path = cfg.sound_path(primary);
    if primary_path.is_file() {
        out.push(load_file(&primary_path)?);
    }
    let bank_dir = Config::assets_dir().join(subdir);
    if bank_dir.is_dir() {
        let mut paths: Vec<_> = std::fs::read_dir(&bank_dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()) == Some("wav")
                    && p.file_name()
                        .and_then(|n| n.to_str())
                        .map(|n| n.starts_with(glob_prefix))
                        .unwrap_or(false)
            })
            .collect();
        paths.sort();
        for p in paths {
            if let Ok(bytes) = load_file(&p) {
                out.push(bytes);
            }
        }
    }
    if out.is_empty() {
        anyhow::bail!("no sounds loaded for {primary} / {subdir}/{glob_prefix}*");
    }
    Ok(out)
}

impl AudioEngine {
    pub fn new(cfg: &Config) -> Result<Self> {
        let (stream, handle) = OutputStream::try_default().context("open audio output")?;
        Ok(Self {
            _stream: stream,
            handle,
            gate: Arc::new(Mutex::new(())),
            normal: load_bank(cfg, &cfg.normal_sound, "buckle", "normal-")?,
            space: load_bank(cfg, &cfg.space_sound, "buckle", "space")?,
            enter: load_bank(cfg, &cfg.enter_sound, "buckle", "enter")?,
            modifier: load_bank(cfg, &cfg.modifier_sound, "buckle", "modifier")?,
            mouse_left: load_bank(cfg, &cfg.mouse_left_sound, "mouse", "mouse-left")?,
            mouse_middle: load_bank(cfg, &cfg.mouse_middle_sound, "mouse", "mouse-middle")
                .or_else(|_| load_bank(cfg, &cfg.mouse_middle_sound, "mouse", "mouse-left"))?,
            mouse_right: load_bank(cfg, &cfg.mouse_right_sound, "mouse", "mouse-right")
                .or_else(|_| load_bank(cfg, &cfg.mouse_right_sound, "mouse", "mouse-left"))?,
        })
    }

    pub fn reload(&mut self, cfg: &Config) -> Result<()> {
        *self = Self::new(cfg)?;
        Ok(())
    }

    pub fn play(&self, cat: Category, cfg: &Config) {
        if !cfg.enabled {
            return;
        }
        let bank = match cat {
            Category::Normal => &self.normal,
            Category::Space => &self.space,
            Category::Enter => &self.enter,
            Category::Modifier => &self.modifier,
            Category::MouseLeft => &self.mouse_left,
            Category::MouseMiddle => &self.mouse_middle,
            Category::MouseRight => &self.mouse_right,
        };
        let seed = now_seed() ^ ((cat as u64) << 17);
        let bytes = pick(bank, seed);
        if bytes.is_empty() {
            return;
        }
        let cat_vol = match cat {
            Category::Normal => cfg.normal_volume,
            Category::Space => cfg.space_volume,
            Category::Enter => cfg.enter_volume,
            Category::Modifier => cfg.modifier_volume,
            Category::MouseLeft | Category::MouseMiddle | Category::MouseRight => cfg.mouse_volume,
        };
        let vol = (cfg.volume * cat_vol).clamp(0.0, 1.0);
        let is_mouse = matches!(
            cat,
            Category::MouseLeft | Category::MouseMiddle | Category::MouseRight
        );
        let cursor = std::io::Cursor::new(bytes.to_vec());
        let Ok(decoder) = Decoder::new(BufReader::new(cursor)) else {
            return;
        };
        // Mouse needs minimal latency: skip speed jitter (can add startup delay).
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(sink) = Sink::try_new(&self.handle) {
            sink.set_volume(vol);
            if is_mouse {
                sink.append(decoder);
            } else {
                sink.append(decoder.speed(jitter_speed(seed)));
            }
            sink.detach();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_handles_an_empty_bank_and_stays_in_range() {
        assert!(pick(&[], 42).is_empty());
        let bank = vec![vec![1u8], vec![2u8], vec![3u8]];
        for seed in [0u64, 1, 7, u64::MAX, 0x9E3779B97F4A7C15] {
            let got = pick(&bank, seed);
            assert!(bank.iter().any(|b| b.as_slice() == got));
        }
        assert_eq!(pick(&bank, 99), pick(&bank, 99), "deterministic for a seed");
    }

    #[test]
    fn pick_uses_every_sound_in_a_bank_over_many_seeds() {
        let bank = vec![vec![1u8], vec![2u8], vec![3u8], vec![4u8]];
        let mut seen = [false; 4];
        for seed in 0..2000u64 {
            seen[(pick(&bank, seed)[0] - 1) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s), "{seen:?}");
    }

    #[test]
    fn speed_jitter_stays_within_two_percent() {
        for seed in (0..5000u64).map(|i| i.wrapping_mul(0x1234_5678_9ABC_DEF1)) {
            let j = jitter_speed(seed);
            assert!((0.98..=1.02).contains(&j), "{j}");
        }
    }
}
