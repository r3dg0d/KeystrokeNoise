use crate::config::{Category, Config};
use anyhow::{Context, Result};
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::io::BufReader;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub struct AudioEngine {
    _stream: OutputStream,
    handle: OutputStreamHandle,
    /// Serialize sink creation lightly; overlaps are allowed via detach.
    gate: Arc<Mutex<()>>,
    normal: Vec<u8>,
    space: Vec<u8>,
    enter: Vec<u8>,
    modifier: Vec<u8>,
}

fn jitter_seed() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(1)
}

/// Tiny deterministic-ish LCG so we avoid pulling in rand.
fn jitter_factor(seed: u64) -> f32 {
    let x = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
    let r = ((x >> 33) as f32) / (u32::MAX as f32);
    // ±4% pitch / speed variation for less machine-gun sameness
    0.96 + r * 0.08
}

impl AudioEngine {
    pub fn new(cfg: &Config) -> Result<Self> {
        let (stream, handle) = OutputStream::try_default().context("open audio output")?;
        let load = |path: std::path::PathBuf| -> Result<Vec<u8>> {
            std::fs::read(&path).with_context(|| format!("load sound {}", path.display()))
        };
        Ok(Self {
            _stream: stream,
            handle,
            gate: Arc::new(Mutex::new(())),
            normal: load(cfg.sound_path(&cfg.normal_sound))?,
            space: load(cfg.sound_path(&cfg.space_sound))?,
            enter: load(cfg.sound_path(&cfg.enter_sound))?,
            modifier: load(cfg.sound_path(&cfg.modifier_sound))?,
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
        let bytes = match cat {
            Category::Normal => &self.normal,
            Category::Space => &self.space,
            Category::Enter => &self.enter,
            Category::Modifier => &self.modifier,
        };
        let cat_vol = match cat {
            Category::Normal => cfg.normal_volume,
            Category::Space => cfg.space_volume,
            Category::Enter => cfg.enter_volume,
            Category::Modifier => cfg.modifier_volume,
        };
        let vol = (cfg.volume * cat_vol).clamp(0.0, 1.0);
        let speed = jitter_factor(jitter_seed() ^ (cat as u64).wrapping_mul(0x9E3779B97F4A7C15));
        let cursor = std::io::Cursor::new(bytes.clone());
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
        // Intentionally never log category or key material.
    }
}
