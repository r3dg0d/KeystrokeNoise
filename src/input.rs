use crate::config::Category;
use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

/// Map a key event to a sound category and immediately discard key identity.
/// Never log, store, or transmit key codes or names.
pub fn categorize(key: Key) -> Category {
    match key {
        Key::KEY_SPACE => Category::Space,
        Key::KEY_ENTER | Key::KEY_KPENTER => Category::Enter,
        Key::KEY_LEFTSHIFT
        | Key::KEY_RIGHTSHIFT
        | Key::KEY_BACKSPACE
        | Key::KEY_TAB
        | Key::KEY_CAPSLOCK
        | Key::KEY_LEFTCTRL
        | Key::KEY_RIGHTCTRL
        | Key::KEY_LEFTALT
        | Key::KEY_RIGHTALT
        | Key::KEY_LEFTMETA
        | Key::KEY_RIGHTMETA
        | Key::KEY_DELETE
        | Key::KEY_ESC => Category::Modifier,
        // Ignore pure media / power / LED-only noise if somehow categorized
        _ => Category::Normal,
    }
}

fn is_keyboard_like(dev: &Device) -> bool {
    if !dev.supported_events().contains(EventType::KEY) {
        return false;
    }
    let Some(keys) = dev.supported_keys() else {
        return false;
    };
    // Real keyboards usually expose alphabetic keys.
    keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) && keys.contains(Key::KEY_ENTER)
}

pub fn open_keyboards(preferred: &str) -> Result<Vec<(PathBuf, Device)>> {
    if !preferred.is_empty() {
        let path = PathBuf::from(preferred);
        let dev = Device::open(&path).with_context(|| format!("open {preferred}"))?;
        return Ok(vec![(path, dev)]);
    }

    let mut found = Vec::new();
    for entry in fs::read_dir("/dev/input").context("read /dev/input")? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.starts_with("event") {
            continue;
        }
        let Ok(dev) = Device::open(&path) else {
            continue;
        };
        if !is_keyboard_like(&dev) {
            continue;
        }
        found.push((path, dev));
    }

    if found.is_empty() {
        anyhow::bail!(
            "no readable keyboard event device under /dev/input (need `input` group / seat ACL)"
        );
    }
    Ok(found)
}

pub fn set_nonblocking(dev: &Device) -> Result<()> {
    let fd = dev.as_raw_fd();
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        anyhow::bail!("fcntl F_GETFL failed");
    }
    let rc = unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    if rc < 0 {
        anyhow::bail!("fcntl F_SETFL O_NONBLOCK failed");
    }
    Ok(())
}

pub fn is_key_down(value: i32) -> bool {
    value == 1
}

pub fn event_category(ev: &evdev::InputEvent) -> Option<Category> {
    match ev.kind() {
        InputEventKind::Key(key) if is_key_down(ev.value()) => Some(categorize(key)),
        _ => None,
    }
}
