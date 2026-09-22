mod audio;
mod config;
mod input;

use anyhow::{Context, Result};
use audio::AudioEngine;
use clap::{Parser, Subcommand};
use config::{Category, Config};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "keystroke-noise", version, about = "Mechanical key sounds without keylogging")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Run the sound daemon (default)
    Daemon,
    /// Toggle enabled in config and print ON/OFF
    Toggle,
    /// Play one category: normal|space|enter|modifier
    Test { category: String },
    /// Write default config if missing
    Init,
    /// Show status
    Status,
}

fn notify_toggle(on: bool) {
    let msg = if on {
        "KeystrokeNoise: ON"
    } else {
        "KeystrokeNoise: OFF"
    };
    let _ = std::process::Command::new("notify-send")
        .args(["-a", "KeystrokeNoise", "-t", "2000", msg])
        .status();
    println!("{msg}");
}

fn ensure_default_sounds() -> Result<()> {
    let assets = Config::assets_dir();
    std::fs::create_dir_all(&assets)?;
    // Copy packaged defaults if user sounds missing.
    let exe = std::env::current_exe().unwrap_or_default();
    let candidates = [
        exe.parent().map(|p| p.join("../share/keystroke-noise/sounds")).unwrap_or_default(),
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets"),
    ];
    for name in ["normal.wav", "space.wav", "enter.wav", "modifier.wav"] {
        let dest = assets.join(name);
        if dest.exists() {
            continue;
        }
        for root in &candidates {
            let src = root.join(name);
            if src.exists() {
                std::fs::copy(&src, &dest)?;
                break;
            }
        }
    }
    Ok(())
}

fn run_daemon() -> Result<()> {
    ensure_default_sounds()?;
    let mut cfg = Config::load()?;
    let mut engine = AudioEngine::new(&cfg)?;
    let device = input::open_keyboard(&cfg.device)?;

    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })?;

    let cfg_path = Config::config_path();
    let (tx, rx) = std::sync::mpsc::channel();
    let mut watcher = RecommendedWatcher::new(tx, notify::Config::default())?;
    if let Some(parent) = cfg_path.parent() {
        let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    }

    eprintln!("keystroke-noise: listening (no key content is logged)");

    let mut device = device;
    while running.load(Ordering::SeqCst) {
        // Hot-reload config on change.
        while let Ok(Ok(event)) = rx.try_recv() {
            use notify::EventKind;
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if let Ok(new_cfg) = Config::load() {
                    cfg = new_cfg;
                    let _ = engine.reload(&cfg);
                }
            }
        }

        match device.fetch_events() {
            Ok(events) => {
                for ev in events {
                    if let Some(cat) = input::event_category(&ev) {
                        engine.play(cat, &cfg);
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(err) => return Err(err).context("read input events"),
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd.unwrap_or(Cmd::Daemon) {
        Cmd::Init => {
            ensure_default_sounds()?;
            let cfg = Config::load()?;
            println!("config: {}", Config::config_path().display());
            println!("enabled={}", cfg.enabled);
        }
        Cmd::Status => {
            let cfg = Config::load()?;
            println!("enabled={}", cfg.enabled);
            println!("volume={}", cfg.volume);
            println!("config={}", Config::config_path().display());
        }
        Cmd::Toggle => {
            let mut cfg = Config::load()?;
            cfg.enabled = !cfg.enabled;
            cfg.save()?;
            notify_toggle(cfg.enabled);
        }
        Cmd::Test { category } => {
            ensure_default_sounds()?;
            let cfg = Config::load()?;
            let engine = AudioEngine::new(&cfg)?;
            let cat = match category.to_ascii_lowercase().as_str() {
                "normal" => Category::Normal,
                "space" => Category::Space,
                "enter" => Category::Enter,
                "modifier" | "shift" | "backspace" => Category::Modifier,
                other => anyhow::bail!("unknown category: {other}"),
            };
            engine.play(cat, &cfg);
            std::thread::sleep(Duration::from_millis(200));
        }
        Cmd::Daemon => run_daemon()?,
    }
    Ok(())
}
