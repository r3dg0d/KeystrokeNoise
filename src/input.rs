use crate::config::Category;
use anyhow::{Context, Result};
use evdev::{Device, EventType, InputEventKind, Key};
use std::fs;
use std::os::fd::AsRawFd;
use std::path::PathBuf;

/// Map a key event to a sound category and immediately discard key identity.
/// Never log, store, or transmit key codes or names.
pub fn categorize_key(key: Key) -> Category {
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
        _ => Category::Normal,
    }
}

pub fn categorize_button(key: Key) -> Option<Category> {
    match key {
        Key::BTN_LEFT => Some(Category::MouseLeft),
        Key::BTN_MIDDLE => Some(Category::MouseMiddle),
        Key::BTN_RIGHT => Some(Category::MouseRight),
        // Side buttons → treat as left-click-ish soft click
        Key::BTN_SIDE | Key::BTN_EXTRA => Some(Category::MouseLeft),
        _ => None,
    }
}

fn is_keyboard_like(dev: &Device) -> bool {
    if !dev.supported_events().contains(EventType::KEY) {
        return false;
    }
    let Some(keys) = dev.supported_keys() else {
        return false;
    };
    keys.contains(Key::KEY_A) && keys.contains(Key::KEY_Z) && keys.contains(Key::KEY_ENTER)
}

fn is_mouse_like(dev: &Device) -> bool {
    if !dev.supported_events().contains(EventType::KEY) {
        return false;
    }
    let Some(keys) = dev.supported_keys() else {
        return false;
    };
    keys.contains(Key::BTN_LEFT)
}

pub fn open_input_devices(preferred: &str, mouse_enabled: bool) -> Result<Vec<(PathBuf, Device)>> {
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
        let kb = is_keyboard_like(&dev);
        let mouse = mouse_enabled && is_mouse_like(&dev);
        if kb || mouse {
            found.push((path, dev));
        }
    }

    if found.is_empty() {
        anyhow::bail!(
            "no readable keyboard/mouse event device under /dev/input (need `input` group / seat ACL)"
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

pub fn event_category(ev: &evdev::InputEvent, mouse_enabled: bool) -> Option<Category> {
    match ev.kind() {
        InputEventKind::Key(key) if is_key_down(ev.value()) => {
            if let Some(cat) = categorize_button(key) {
                if mouse_enabled {
                    return Some(cat);
                }
                return None;
            }
            // Ignore mouse buttons when already handled; skip pure BTN_* that we don't map
            let name = format!("{key:?}");
            if name.starts_with("BTN_") {
                return None;
            }
            Some(categorize_key(key))
        }
        _ => None,
    }
}
