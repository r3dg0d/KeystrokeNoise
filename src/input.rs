use crate::config::Category;
use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use std::fs;

/// Map a key event to a sound category and immediately discard key identity.
/// Never log, store, or transmit key codes or names.
pub fn categorize(key: Key) -> Category {
    match key {
        Key::KEY_SPACE => Category::Space,
        Key::KEY_ENTER | Key::KEY_KPENTER => Category::Enter,
        Key::KEY_LEFTSHIFT
        | Key::KEY_RIGHTSHIFT
        | Key::KEY_BACKSPACE
        | Key::KEY_LEFTCTRL
        | Key::KEY_RIGHTCTRL
        | Key::KEY_LEFTALT
        | Key::KEY_RIGHTALT
        | Key::KEY_LEFTMETA
        | Key::KEY_RIGHTMETA => Category::Modifier,
        _ => Category::Normal,
    }
}

pub fn open_keyboard(preferred: &str) -> Result<Device> {
    if !preferred.is_empty() {
        return Device::open(preferred).with_context(|| format!("open {preferred}"));
    }
    let mut best: Option<Device> = None;
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
        if !dev.supported_events().contains(EventType::KEY) {
            continue;
        }
        let looks_kb = dev
            .name()
            .map(|n| {
                let l = n.to_ascii_lowercase();
                l.contains("keyboard") || l.contains("keychron") || l.contains("atkbd")
            })
            .unwrap_or(false);
        if looks_kb {
            return Ok(dev);
        }
        if best.is_none() {
            best = Some(dev);
        }
    }
    best.ok_or_else(|| {
        anyhow::anyhow!("no readable keyboard event device under /dev/input (need input group / seat ACL)")
    })
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
