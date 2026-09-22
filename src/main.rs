mod audio;
mod config;
mod input;

use anyhow::{Context, Result};
use audio::AudioEngine;
use clap::{Parser, Subcommand};
use config::{Category, Config};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::os::fd::AsRawFd;
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
    let exe = std::env::current_exe().unwrap_or_default();
    let candidates = [
        exe.parent()
            .map(|p| p.join("../share/keystroke-noise/sounds"))
            .unwrap_or_default(),
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
    let mut devices = input::open_keyboards(&cfg.device)?;
    for (_path, dev) in &devices {
        input::set_nonblocking(dev)?;
    }

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

    eprintln!(
        "keystroke-noise: listening on {} keyboard device(s) (no key content is logged)",
        devices.len()
    );

    while running.load(Ordering::SeqCst) {
        while let Ok(Ok(event)) = rx.try_recv() {
            use notify::EventKind;
            if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                if let Ok(new_cfg) = Config::load() {
                    cfg = new_cfg;
                    let _ = engine.reload(&cfg);
                }
            }
        }

        // poll(2) across all keyboard fds
        let mut fds: Vec<libc::pollfd> = devices
            .iter()
            .map(|(_, d)| libc::pollfd {
                fd: d.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            })
            .collect();

        let rc = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 50) };
        if rc < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err).context("poll input devices");
        }

        for (i, (_path, device)) in devices.iter_mut().enumerate() {
            if fds[i].revents == 0 {
                continue;
            }
            match device.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let Some(cat) = input::event_category(&ev) {
                            engine.play(cat, &cfg);
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    // Device disappeared (unplug) — drop it and keep going.
                    eprintln!("keystroke-noise: device read error, dropping: {err}");
                    fds[i].revents = 0;
                    // mark for removal via path index
                    // handled below by rebuilding if needed
                }
            }
        }

        // Drop broken devices (EIO / ENODEV typically)
        let before = devices.len();
        devices.retain(|(_, d)| {
            // If fd still looks open; cheap check via fcntl
            let fd = d.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            flags >= 0
        });
        if devices.is_empty() {
            // Rescan after a short pause (hotplug)
            std::thread::sleep(Duration::from_millis(500));
            if let Ok(new_devs) = input::open_keyboards(&cfg.device) {
                for (_p, d) in &new_devs {
                    let _ = input::set_nonblocking(d);
                }
                devices = new_devs;
            }
        } else if devices.len() != before {
            eprintln!(
                "keystroke-noise: now listening on {} keyboard device(s)",
                devices.len()
            );
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
            match input::open_keyboards(&cfg.device) {
                Ok(devs) => {
                    println!("keyboards={}", devs.len());
                    for (p, d) in &devs {
                        let name = d.name().unwrap_or("unknown");
                        println!("  {} ({name})", p.display());
                    }
                }
                Err(e) => println!("keyboards=0 ({e})"),
            }
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
            for _ in 0..3 {
                engine.play(cat, &cfg);
                std::thread::sleep(Duration::from_millis(120));
            }
            std::thread::sleep(Duration::from_millis(350));
        }
        Cmd::Daemon => run_daemon()?,
    }
    Ok(())
}
