//! Global hotkey registration and parsing.
//!
//! `global-hotkey` gives us one process-wide manager. We keep it alive for the
//! lifetime of the app and map hotkey ids back to [`Action`]s.

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};

use crate::config::Hotkeys as HotkeyConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    CaptureArea,
    CaptureFull,
    PinClipboard,
    LongCapture,
    Record,
    SaveLast,
}

/// Why a binding could not be installed. The UI turns these into localised
/// hints instead of leaking the backend's `Debug` output to the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingError {
    /// Another process (or our own previous instance) owns the combination.
    InUse,
    /// The shortcut string could not be parsed.
    Invalid,
    /// The platform refused the registration for another reason.
    Backend,
}

pub struct Hotkeys {
    _manager: Option<GlobalHotKeyManager>,
    bindings: Vec<(HotKey, Action)>,
    /// Actions that failed to register, with the reason.
    pub failures: Vec<(Action, BindingError)>,
}

impl Hotkeys {
    /// A placeholder with no manager and no bindings.
    fn unbound() -> Self {
        Self {
            _manager: None,
            bindings: Vec::new(),
            failures: Vec::new(),
        }
    }

    pub fn register(config: &HotkeyConfig) -> Self {
        let manager = match GlobalHotKeyManager::new() {
            Ok(manager) => manager,
            Err(err) => {
                tracing::error!(%err, "could not create the global hotkey manager");
                return Self::unbound();
            }
        };

        let mut hotkeys = Self {
            _manager: Some(manager),
            bindings: Vec::new(),
            failures: Vec::new(),
        };
        hotkeys.apply(config);
        hotkeys
    }

    /// Re-register every binding after the user changed one.
    ///
    /// A brand new [`GlobalHotKeyManager`] cannot simply replace the old one:
    /// on Windows each manager registers its hotkeys against its own hidden
    /// window, so the OS reports "already registered" for combinations that
    /// *we* still hold. Releasing the old set on the live manager sidesteps
    /// that, and keeps us from tearing down and rebuilding a window on every
    /// rebind.
    pub fn rebind(&mut self, config: &HotkeyConfig) {
        if self._manager.is_none() {
            *self = Self::register(config);
            return;
        }
        self.apply(config);
    }

    /// Release everything currently bound, then install `config`'s set.
    fn apply(&mut self, config: &HotkeyConfig) {
        let Some(manager) = self._manager.as_ref() else {
            return;
        };
        // Free the old bindings first so a combination can move to a different
        // action without colliding with the one we are about to install.
        for (hotkey, _) in self.bindings.drain(..) {
            if let Err(err) = manager.unregister(hotkey) {
                tracing::debug!(?hotkey, %err, "could not release hotkey");
            }
        }
        self.failures.clear();

        let specs = [
            (&config.capture_area, Action::CaptureArea),
            (&config.capture_full, Action::CaptureFull),
            (&config.pin_clipboard, Action::PinClipboard),
            (&config.long_capture, Action::LongCapture),
            (&config.record, Action::Record),
            (&config.save_last, Action::SaveLast),
        ];

        for (text, action) in specs {
            match parse_hotkey(text) {
                Ok(hotkey) => match manager.register(hotkey) {
                    Ok(()) => self.bindings.push((hotkey, action)),
                    Err(err) => {
                        let kind = match &err {
                            global_hotkey::Error::AlreadyRegistered(_) => BindingError::InUse,
                            global_hotkey::Error::HotKeyParseError(_)
                            | global_hotkey::Error::UnrecognizedHotKeyCode(_)
                            | global_hotkey::Error::EmptyHotKeyToken(_)
                            | global_hotkey::Error::UnexpectedHotKeyFormat(_) => {
                                BindingError::Invalid
                            }
                            _ => BindingError::Backend,
                        };
                        self.failures.push((action, kind));
                        tracing::warn!(%text, %err, "hotkey registration failed");
                    }
                },
                Err(err) => {
                    self.failures.push((action, BindingError::Invalid));
                    tracing::warn!(%text, %err, "could not parse hotkey");
                }
            }
        }
    }

    /// Release every binding without installing new ones.
    ///
    /// This is what the tray's "disable hotkeys" uses: the user's
    /// combinations stay in the config, they just stop being registered.
    pub fn unbind_all(&mut self) {
        let Some(manager) = self._manager.as_ref() else {
            self.bindings.clear();
            self.failures.clear();
            return;
        };
        for (hotkey, _) in self.bindings.drain(..) {
            if let Err(err) = manager.unregister(hotkey) {
                tracing::debug!(?hotkey, %err, "could not release hotkey");
            }
        }
        self.failures.clear();
    }

    /// Reason a given action is not bound, if any.
    pub fn failure_for(&self, action: Action) -> Option<BindingError> {
        self.failures
            .iter()
            .find(|(a, _)| *a == action)
            .map(|(_, kind)| *kind)
    }

    /// Drain pending hotkey events and return the actions that were pressed.
    pub fn poll(&self) -> Vec<Action> {
        let mut actions = Vec::new();
        while let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
            if event.state() != HotKeyState::Pressed {
                continue;
            }
            if let Some((_, action)) = self.bindings.iter().find(|(hk, _)| hk.id() == event.id()) {
                actions.push(*action);
            }
        }
        actions
    }

    pub fn registered(&self) -> usize {
        self.bindings.len()
    }
}

/// Modifier keys on their own: pressing one of these while recording a hotkey
/// must not finish the recording, the user is still building the combination.
pub fn is_modifier(key: egui::Key) -> bool {
    matches!(
        key,
        egui::Key::ShiftLeft
            | egui::Key::ShiftRight
            | egui::Key::ControlLeft
            | egui::Key::ControlRight
            | egui::Key::AltLeft
            | egui::Key::AltRight
            | egui::Key::SuperLeft
            | egui::Key::SuperRight
    )
}

/// Printable name of a key in the same dialect [`parse_hotkey`] understands.
/// `None` for keys we deliberately do not bind.
pub fn key_name(key: egui::Key) -> Option<&'static str> {
    use egui::Key::*;
    Some(match key {
        A => "A",
        B => "B",
        C => "C",
        D => "D",
        E => "E",
        F => "F",
        G => "G",
        H => "H",
        I => "I",
        J => "J",
        K => "K",
        L => "L",
        M => "M",
        N => "N",
        O => "O",
        P => "P",
        Q => "Q",
        R => "R",
        S => "S",
        T => "T",
        U => "U",
        V => "V",
        W => "W",
        X => "X",
        Y => "Y",
        Z => "Z",
        Num0 => "0",
        Num1 => "1",
        Num2 => "2",
        Num3 => "3",
        Num4 => "4",
        Num5 => "5",
        Num6 => "6",
        Num7 => "7",
        Num8 => "8",
        Num9 => "9",
        F1 => "F1",
        F2 => "F2",
        F3 => "F3",
        F4 => "F4",
        F5 => "F5",
        F6 => "F6",
        F7 => "F7",
        F8 => "F8",
        F9 => "F9",
        F10 => "F10",
        F11 => "F11",
        F12 => "F12",
        Enter => "Enter",
        Escape => "Esc",
        Space => "Space",
        Tab => "Tab",
        Backspace => "Backspace",
        Insert => "Insert",
        Delete => "Delete",
        Home => "Home",
        End => "End",
        PageUp => "PageUp",
        PageDown => "PageDown",
        ArrowUp => "Up",
        ArrowDown => "Down",
        ArrowLeft => "Left",
        ArrowRight => "Right",
        Colon => "Colon",
        Comma => "Comma",
        Backslash => "Backslash",
        Slash => "Slash",
        Pipe => "Pipe",
        Questionmark => "Questionmark",
        OpenBracket => "OpenBracket",
        CloseBracket => "CloseBracket",
        OpenCurlyBracket => "OpenCurlyBracket",
        CloseCurlyBracket => "CloseCurlyBracket",
        Backtick => "Backtick",
        Minus => "Minus",
        Period => "Period",
        Plus => "Plus",
        Equals => "Equals",
        Semicolon => "Semicolon",
        Quote => "Quote",
        _ => return None,
    })
}

/// Build the `Ctrl+Shift+A` style text for a combination the user just typed.
/// Returns `None` when no bindable key was pressed.
pub fn format_hotkey(modifiers: &egui::Modifiers, key: egui::Key) -> Option<String> {
    let name = key_name(key)?;
    let mut parts: Vec<&str> = Vec::new();
    if modifiers.ctrl {
        parts.push("Ctrl");
    }
    if modifiers.alt {
        parts.push("Alt");
    }
    if modifiers.shift {
        parts.push("Shift");
    }
    // `mac_cmd` is only set by the real Cmd key on macOS; `command` also fires
    // for Ctrl elsewhere, so it must not be double counted.
    if modifiers.mac_cmd {
        parts.push("Win");
    }
    parts.push(name);
    Some(parts.join("+"))
}

fn parse_hotkey(text: &str) -> Result<HotKey, String> {
    let mut mods = Modifiers::empty();
    let mut key = None;

    for part in text.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        let lower = part.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => mods |= Modifiers::CONTROL,
            "alt" => mods |= Modifiers::ALT,
            "shift" => mods |= Modifiers::SHIFT,
            "win" | "meta" | "super" | "cmd" => mods |= Modifiers::META,
            _ => {
                if key.is_some() {
                    return Err("expected exactly one non-modifier key".into());
                }
                key = Some(parse_code(part)?);
            }
        }
    }

    let key = key.ok_or_else(|| "missing key".to_string())?;
    Ok(HotKey::new(Some(mods), key))
}

fn parse_code(part: &str) -> Result<Code, String> {
    let upper = part.to_ascii_uppercase();
    if upper.len() == 1 {
        let ch = upper.chars().next().unwrap();
        if ch.is_ascii_alphabetic() {
            return Ok(match ch {
                'A' => Code::KeyA,
                'B' => Code::KeyB,
                'C' => Code::KeyC,
                'D' => Code::KeyD,
                'E' => Code::KeyE,
                'F' => Code::KeyF,
                'G' => Code::KeyG,
                'H' => Code::KeyH,
                'I' => Code::KeyI,
                'J' => Code::KeyJ,
                'K' => Code::KeyK,
                'L' => Code::KeyL,
                'M' => Code::KeyM,
                'N' => Code::KeyN,
                'O' => Code::KeyO,
                'P' => Code::KeyP,
                'Q' => Code::KeyQ,
                'R' => Code::KeyR,
                'S' => Code::KeyS,
                'T' => Code::KeyT,
                'U' => Code::KeyU,
                'V' => Code::KeyV,
                'W' => Code::KeyW,
                'X' => Code::KeyX,
                'Y' => Code::KeyY,
                'Z' => Code::KeyZ,
                _ => unreachable!(),
            });
        }
        if ch.is_ascii_digit() {
            return Ok(match ch {
                '0' => Code::Digit0,
                '1' => Code::Digit1,
                '2' => Code::Digit2,
                '3' => Code::Digit3,
                '4' => Code::Digit4,
                '5' => Code::Digit5,
                '6' => Code::Digit6,
                '7' => Code::Digit7,
                '8' => Code::Digit8,
                '9' => Code::Digit9,
                _ => unreachable!(),
            });
        }
    }

    match upper.as_str() {
        "PRINTSCREEN" | "PRTSC" => Ok(Code::PrintScreen),
        "ESC" | "ESCAPE" => Ok(Code::Escape),
        "ENTER" | "RETURN" => Ok(Code::Enter),
        "SPACE" => Ok(Code::Space),
        "TAB" => Ok(Code::Tab),
        "INSERT" => Ok(Code::Insert),
        "DELETE" | "DEL" => Ok(Code::Delete),
        "HOME" => Ok(Code::Home),
        "END" => Ok(Code::End),
        "PAGEUP" => Ok(Code::PageUp),
        "PAGEDOWN" => Ok(Code::PageDown),
        "UP" => Ok(Code::ArrowUp),
        "DOWN" => Ok(Code::ArrowDown),
        "LEFT" => Ok(Code::ArrowLeft),
        "RIGHT" => Ok(Code::ArrowRight),
        _ if upper.starts_with('F') && upper[1..].parse::<u8>().is_ok() => {
            let n: u8 = upper[1..].parse().map_err(|_| "invalid function key")?;
            Ok(match n {
                1 => Code::F1,
                2 => Code::F2,
                3 => Code::F3,
                4 => Code::F4,
                5 => Code::F5,
                6 => Code::F6,
                7 => Code::F7,
                8 => Code::F8,
                9 => Code::F9,
                10 => Code::F10,
                11 => Code::F11,
                12 => Code::F12,
                _ => return Err("only F1-F12 are supported".into()),
            })
        }
        _ => Err(format!("unsupported key `{part}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_shortcuts() {
        let hk = parse_hotkey("Ctrl+Alt+A").unwrap();
        assert!(hk.mods.contains(Modifiers::CONTROL));
        assert!(hk.mods.contains(Modifiers::ALT));
        assert_eq!(hk.key, Code::KeyA);

        let hk = parse_hotkey("shift+F11").unwrap();
        assert!(hk.mods.contains(Modifiers::SHIFT));
        assert_eq!(hk.key, Code::F11);
    }

    #[test]
    fn rejects_bad_shortcuts() {
        assert!(parse_hotkey("Ctrl+Alt").is_err());
        assert!(parse_hotkey("Ctrl+A+B").is_err());
    }

    #[test]
    fn formats_recorded_combinations() {
        let mods = egui::Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert_eq!(
            format_hotkey(&mods, egui::Key::A).as_deref(),
            Some("Ctrl+Shift+A")
        );
        // Modifiers alone are not a binding.
        assert!(format_hotkey(&mods, egui::Key::ControlLeft).is_none());
        assert_eq!(
            format_hotkey(&egui::Modifiers::default(), egui::Key::F5).as_deref(),
            Some("F5")
        );
    }

    #[test]
    fn recorded_combinations_round_trip() {
        let mods = egui::Modifiers {
            alt: true,
            ..Default::default()
        };
        let text = format_hotkey(&mods, egui::Key::Num7).unwrap();
        let hk = parse_hotkey(&text).unwrap();
        assert!(hk.mods.contains(Modifiers::ALT));
    }
}
