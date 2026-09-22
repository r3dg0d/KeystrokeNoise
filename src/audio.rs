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
}

fn now_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

fn pick<'a>(bank: &'a [Vec<u8>], seed: u64) -> &'a [u8] {
    if bank.is_empty() {
        return &[];
    }
    let i = (seed.wrapping_mul(0x9E3779B97F4A7C15) >> 32) as usize % bank.len();
    &bank[i]
}

/// Mild ±2% speed variation — real samples sound wrong with heavy pitch shifts.
fn jitter_speed(seed: u64) -> f32 {
    let x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let r = ((x >> 33) as f32) / (u32::MAX as f32);
    0.98 + r * 0.04
}

fn load_file(path: &PathBuf) -> Result<Vec<u8>> {
    std::fs::read(path).with_context(|| format!("load sound {}", path.display()))
}

fn load_bank(cfg: &Config, primary: &str, glob_prefix: &str) -> Result<Vec<Vec<u8>>> {
    let mut out = Vec::new();
    let primary_path = cfg.sound_path(primary);
    if primary_path.is_file() {
        out.push(load_file(&primary_path)?);
    }
    // Optional bank: ~/.config/keystroke-noise/sounds/buckle/<prefix>-*.wav
    let bank_dir = Config::assets_dir().join("buckle");
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
        anyhow::bail!("no sounds loaded for {primary} / {glob_prefix}*");
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
            normal: load_bank(cfg, &cfg.normal_sound, "normal-")?,
            space: load_bank(cfg, &cfg.space_sound, "space")?,
            enter: load_bank(cfg, &cfg.enter_sound, "enter")?,
            modifier: load_bank(cfg, &cfg.modifier_sound, "modifier")?,
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
        };
        let vol = (cfg.volume * cat_vol).clamp(0.0, 1.0);
        let speed = jitter_speed(seed);
        let cursor = std::io::Cursor::new(bytes.to_vec());
        let Ok(decoder) = Decoder::new(BufReader::new(cursor)) else {
            return;
        };
        let source = decoder.speed(speed);
        let _guard = self.gate.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(sink) = Sink::try_new(&self.handle) {
            sink.set_volume(vol);
            sink.append(source);
            sink.detach();
        }
    }
}
