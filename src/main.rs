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
    let mut devices = input::open_input_devices(&cfg.device, cfg.mouse_enabled)?;
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
        "keystroke-noise: listening on {} input device(s) (no key/button content is logged)",
        devices.len()
    );

    let mut last_rescan = std::time::Instant::now();
    const RESCAN_EVERY: Duration = Duration::from_secs(2);

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

        // Indices that hit ENODEV/EIO — remove after the loop (don't mutate while iterating).
        let mut dead: Vec<usize> = Vec::new();
        for (i, (_path, device)) in devices.iter_mut().enumerate() {
            if fds.get(i).map(|f| f.revents).unwrap_or(0) == 0 {
                continue;
            }
            // POLLERR / POLLHUP / POLLNVAL → device is gone
            let re = fds[i].revents;
            if re & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 {
                dead.push(i);
                continue;
            }
            match device.fetch_events() {
                Ok(events) => {
                    for ev in events {
                        if let Some(cat) = input::event_category(&ev, cfg.mouse_enabled) {
                            engine.play(cat, &cfg);
                        }
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    // Device disappeared (unplug / node recycled) — drop immediately.
                    // Note: the FD can still pass F_GETFD after ENODEV, so do not rely on fcntl.
                    eprintln!(
                        "keystroke-noise: device read error, dropping {}: {err}",
                        _path.display()
                    );
                    dead.push(i);
                }
            }
        }
        if !dead.is_empty() {
            let mut remove = vec![false; devices.len()];
            for i in dead {
                remove[i] = true;
            }
            let mut kept = Vec::with_capacity(devices.len());
            for (i, item) in devices.into_iter().enumerate() {
                if !remove[i] {
                    kept.push(item);
                }
            }
            devices = kept;
            eprintln!(
                "keystroke-noise: now listening on {} input device(s)",
                devices.len()
            );
            // Force a rescan soon so hotplugged replacements are picked up.
            last_rescan = std::time::Instant::now()
                .checked_sub(RESCAN_EVERY)
                .unwrap_or_else(std::time::Instant::now);
        }

        // Periodic hotplug rescan: add newly appeared devices even when some FDs remain.
        // (Old bug: only rescanned when devices became empty, so a sticky dead FD blocked recovery.)
        if last_rescan.elapsed() >= RESCAN_EVERY || devices.is_empty() {
            last_rescan = std::time::Instant::now();
            if let Ok(found) = input::open_input_devices(&cfg.device, cfg.mouse_enabled) {
                let open_paths: std::collections::HashSet<_> =
                    devices.iter().map(|(p, _)| p.clone()).collect();
                let mut added = 0usize;
                for (p, d) in found {
                    if open_paths.contains(&p) {
                        continue;
                    }
                    if input::set_nonblocking(&d).is_ok() {
                        devices.push((p, d));
                        added += 1;
                    }
                }
                if added > 0 || devices.is_empty() {
                    eprintln!(
                        "keystroke-noise: hotplug rescan → {} device(s) (+{added})",
                        devices.len()
                    );
                }
            }
            if devices.is_empty() {
                std::thread::sleep(Duration::from_millis(400));
            }
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
            match input::open_input_devices(&cfg.device, cfg.mouse_enabled) {
                Ok(devs) => {
                    println!("devices={}", devs.len());
                    for (p, d) in &devs {
                        let name = d.name().unwrap_or("unknown");
                        println!("  {} ({name})", p.display());
                    }
                }
                Err(e) => println!("devices=0 ({e})"),
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
                "mouse" | "mouse-left" | "left" => Category::MouseLeft,
                "mouse-middle" | "middle" => Category::MouseMiddle,
                "mouse-right" | "right" => Category::MouseRight,
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
