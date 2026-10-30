//! Parsing for the in-app (overlay) shortcuts.
//!
//! The config stores them as text — "V", "Enter", "Ctrl+Z" — because that is
//! what a user types and what the settings page shows. This module turns that
//! text into the `(key, modifiers)` pair the overlay matches against, and
//! formats a recorded combination back into text.

use egui::{Key, Modifiers};

/// A parsed in-app binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Binding {
    pub key: Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl Binding {
    /// Does this binding match the key event?
    pub fn matches(&self, key: Key, mods: &Modifiers) -> bool {
        self.key == key
            && self.ctrl == mods.ctrl
            && self.shift == mods.shift
            && self.alt == mods.alt
    }
}

/// Parse "Ctrl+Shift+Z", "F1", "V", "Enter" …
pub fn parse(text: &str) -> Option<Binding> {
    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut key = None;
    for part in text.split('+') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => ctrl = true,
            "shift" => shift = true,
            "alt" => alt = true,
            "win" | "super" | "meta" => {}
            _ => {
                // An unknown token is a typo, not a key: reject the whole
                // binding rather than silently binding whatever came after it.
                key = Some(key_from_name(part)?);
            }
        }
    }
    Some(Binding {
        key: key?,
        ctrl,
        shift,
        alt,
    })
}

fn key_from_name(name: &str) -> Option<Key> {
    let lower = name.to_ascii_lowercase();
    let key = match lower.as_str() {
        "enter" | "return" => Key::Enter,
        "esc" | "escape" => Key::Escape,
        "space" | " " => Key::Space,
        "backspace" => Key::Backspace,
        "delete" | "del" => Key::Delete,
        "tab" => Key::Tab,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "insert" | "ins" => Key::Insert,
        _ => {
            if let Some(rest) = lower.strip_prefix('f') {
                // F1 – F12
                if let Ok(n) = rest.parse::<u8>() {
                    return match n {
                        1 => Some(Key::F1),
                        2 => Some(Key::F2),
                        3 => Some(Key::F3),
                        4 => Some(Key::F4),
                        5 => Some(Key::F5),
                        6 => Some(Key::F6),
                        7 => Some(Key::F7),
                        8 => Some(Key::F8),
                        9 => Some(Key::F9),
                        10 => Some(Key::F10),
                        11 => Some(Key::F11),
                        12 => Some(Key::F12),
                        _ => None,
                    };
                }
            }
            if lower.len() == 1 {
                let ch = lower.chars().next()?;
                if ch.is_ascii_alphabetic() {
                    return Some(letter(ch));
                }
                if ch.is_ascii_digit() {
                    return Some(digit(ch));
                }
            }
            return None;
        }
    };
    Some(key)
}

fn letter(ch: char) -> Key {
    match ch.to_ascii_uppercase() {
        'A' => Key::A,
        'B' => Key::B,
        'C' => Key::C,
        'D' => Key::D,
        'E' => Key::E,
        'F' => Key::F,
        'G' => Key::G,
        'H' => Key::H,
        'I' => Key::I,
        'J' => Key::J,
        'K' => Key::K,
        'L' => Key::L,
        'M' => Key::M,
        'N' => Key::N,
        'O' => Key::O,
        'P' => Key::P,
        'Q' => Key::Q,
        'R' => Key::R,
        'S' => Key::S,
        'T' => Key::T,
        'U' => Key::U,
        'V' => Key::V,
        'W' => Key::W,
        'X' => Key::X,
        'Y' => Key::Y,
        _ => Key::Z,
    }
}

fn digit(ch: char) -> Key {
    match ch {
        '0' => Key::Num0,
        '1' => Key::Num1,
        '2' => Key::Num2,
        '3' => Key::Num3,
        '4' => Key::Num4,
        '5' => Key::Num5,
        '6' => Key::Num6,
        '7' => Key::Num7,
        '8' => Key::Num8,
        _ => Key::Num9,
    }
}

/// Format a recorded key event back into config text.
pub fn format(key: Key, mods: &Modifiers) -> String {
    let mut out = String::new();
    if mods.ctrl {
        out.push_str("Ctrl+");
    }
    if mods.alt {
        out.push_str("Alt+");
    }
    if mods.shift {
        out.push_str("Shift+");
    }
    out.push_str(&key_name(key));
    out
}

fn key_name(key: Key) -> String {
    match key {
        Key::Enter => "Enter".into(),
        Key::Escape => "Esc".into(),
        Key::Space => "Space".into(),
        Key::Backspace => "Backspace".into(),
        Key::Delete => "Delete".into(),
        Key::Tab => "Tab".into(),
        Key::Home => "Home".into(),
        Key::End => "End".into(),
        Key::PageUp => "PageUp".into(),
        Key::PageDown => "PageDown".into(),
        Key::Insert => "Insert".into(),
        Key::F1 => "F1".into(),
        Key::F2 => "F2".into(),
        Key::F3 => "F3".into(),
        Key::F4 => "F4".into(),
        Key::F5 => "F5".into(),
        Key::F6 => "F6".into(),
        Key::F7 => "F7".into(),
        Key::F8 => "F8".into(),
        Key::F9 => "F9".into(),
        Key::F10 => "F10".into(),
        Key::F11 => "F11".into(),
        Key::F12 => "F12".into(),
        Key::Num0 => "0".into(),
        Key::Num1 => "1".into(),
        Key::Num2 => "2".into(),
        Key::Num3 => "3".into(),
        Key::Num4 => "4".into(),
        Key::Num5 => "5".into(),
        Key::Num6 => "6".into(),
        Key::Num7 => "7".into(),
        Key::Num8 => "8".into(),
        Key::Num9 => "9".into(),
        other => other.name().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_and_modified_keys() {
        let v = parse("V").unwrap();
        assert_eq!(v.key, Key::V);
        assert!(!v.ctrl && !v.shift);

        let undo = parse("Ctrl+Z").unwrap();
        assert_eq!(undo.key, Key::Z);
        assert!(undo.ctrl && !undo.shift);

        let redo = parse("Ctrl+Shift+Z").unwrap();
        assert!(redo.ctrl && redo.shift);

        assert_eq!(parse("Enter").unwrap().key, Key::Enter);
        assert_eq!(parse("Esc").unwrap().key, Key::Escape);
        assert_eq!(parse("F5").unwrap().key, Key::F5);
        assert_eq!(parse("3").unwrap().key, Key::Num3);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse("Ctrl+").is_none());
        assert!(parse("Hyper+Q").is_none());
        assert!(parse("F13").is_none());
    }

    #[test]
    fn round_trips_through_format() {
        for text in ["Ctrl+Z", "Enter", "V", "Shift+A", "Ctrl+Shift+S"] {
            let parsed = parse(text).unwrap();
            let again = parse(&format(
                parsed.key,
                &Modifiers {
                    ctrl: parsed.ctrl,
                    shift: parsed.shift,
                    alt: parsed.alt,
                    ..Default::default()
                },
            ))
            .unwrap();
            assert_eq!(again, parsed, "{text} did not round-trip");
        }
    }

    #[test]
    fn matches_only_exact_modifiers() {
        let binding = parse("Ctrl+C").unwrap();
        let ctrl = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let plain = Modifiers::default();
        assert!(binding.matches(Key::C, &ctrl));
        assert!(!binding.matches(Key::C, &plain));
        assert!(!binding.matches(Key::V, &ctrl));
    }
}
