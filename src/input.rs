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

/// True for the kernel's `BTN_*` blocks: misc/mouse/joystick/gamepad/digitizer/wheel
/// (0x100-0x15f), the gamepad D-pad (0x220-0x223) and the trigger-happy block
/// (0x2c0-0x2e7). Numeric on purpose: this runs on every key event and must not
/// allocate or depend on `Debug` names.
pub fn is_button_code(key: Key) -> bool {
    matches!(key.code(), 0x100..=0x15f | 0x220..=0x223 | 0x2c0..=0x2e7)
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
            // Ignore BTN_* codes we don't map (gamepad, stylus, ...): they are not typing.
            if is_button_code(key) {
                return None;
            }
            Some(categorize_key(key))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evdev::InputEvent;

    fn key_event(key: Key, value: i32) -> InputEvent {
        InputEvent::new(EventType::KEY, key.code(), value)
    }

    #[test]
    fn keys_map_to_the_right_sound_category() {
        assert_eq!(categorize_key(Key::KEY_SPACE), Category::Space);
        assert_eq!(categorize_key(Key::KEY_ENTER), Category::Enter);
        assert_eq!(categorize_key(Key::KEY_KPENTER), Category::Enter);
        for k in [
            Key::KEY_LEFTSHIFT,
            Key::KEY_BACKSPACE,
            Key::KEY_TAB,
            Key::KEY_ESC,
            Key::KEY_RIGHTMETA,
        ] {
            assert_eq!(categorize_key(k), Category::Modifier, "{k:?}");
        }
        assert_eq!(categorize_key(Key::KEY_A), Category::Normal);
        assert_eq!(categorize_key(Key::KEY_F5), Category::Normal);
    }

    #[test]
    fn mouse_buttons_map_and_unknown_buttons_do_not() {
        assert_eq!(categorize_button(Key::BTN_LEFT), Some(Category::MouseLeft));
        assert_eq!(
            categorize_button(Key::BTN_MIDDLE),
            Some(Category::MouseMiddle)
        );
        assert_eq!(
            categorize_button(Key::BTN_RIGHT),
            Some(Category::MouseRight)
        );
        assert_eq!(categorize_button(Key::BTN_SIDE), Some(Category::MouseLeft));
        assert_eq!(categorize_button(Key::BTN_SOUTH), None);
        assert_eq!(categorize_button(Key::KEY_A), None);
    }

    /// The numeric BTN_ test must agree with evdev's own names for every named code.
    /// (evdev prints "unknown key" for codes the kernel has not defined; inside a BTN_
    /// block those are still buttons, so only named codes are compared.)
    #[test]
    fn button_ranges_match_evdev_names() {
        for code in 0u16..0x300 {
            let key = Key::new(code);
            let name = format!("{key:?}");
            if name.starts_with("unknown key") {
                continue;
            }
            assert_eq!(
                is_button_code(key),
                name.starts_with("BTN_"),
                "code {code:#x} ({name})"
            );
        }
    }

    #[test]
    fn gamepad_dpad_and_undefined_button_codes_never_make_typing_sounds() {
        assert_eq!(event_category(&key_event(Key::BTN_DPAD_UP, 1), true), None);
        assert_eq!(event_category(&key_event(Key::new(0x10a), 1), true), None);
        assert_eq!(
            event_category(&key_event(Key::BTN_TRIGGER_HAPPY1, 1), true),
            None
        );
    }

    #[test]
    fn only_key_down_makes_a_sound() {
        assert!(is_key_down(1));
        assert!(!is_key_down(0), "release");
        assert!(!is_key_down(2), "autorepeat");
        assert_eq!(
            event_category(&key_event(Key::KEY_A, 1), true),
            Some(Category::Normal)
        );
        assert_eq!(event_category(&key_event(Key::KEY_A, 0), true), None);
        assert_eq!(event_category(&key_event(Key::KEY_A, 2), true), None);
    }

    #[test]
    fn mouse_clicks_respect_the_mouse_setting_and_stray_buttons_are_ignored() {
        assert_eq!(
            event_category(&key_event(Key::BTN_LEFT, 1), true),
            Some(Category::MouseLeft)
        );
        assert_eq!(event_category(&key_event(Key::BTN_LEFT, 1), false), None);
        // A gamepad button is neither a key nor a mapped mouse button.
        assert_eq!(event_category(&key_event(Key::BTN_SOUTH, 1), true), None);
        // Non-key events (e.g. relative motion) never sound.
        let motion = InputEvent::new(EventType::RELATIVE, 0, 5);
        assert_eq!(event_category(&motion, true), None);
    }
}
