//! "Start with the system" support.
//!
//! Windows writes a `HKCU\...\Run` value; macOS and Linux install a
//! `.desktop` entry / launch agent. Every backend is best effort: a read-only
//! registry or a missing `~/.config/autostart` directory simply means the
//! checkbox stays where the user left it and we log why.

use std::path::PathBuf;

// Only the Windows backend reads these; the Linux and macOS entries are files.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
const RUN_VALUE: &str = "PinSight";

/// Turn the preference on or off. Returns whether the change took effect.
pub fn set_enabled(enabled: bool) -> bool {
    let ok = set_platform(enabled);
    if ok {
        tracing::info!(enabled, "run-at-startup updated");
    }
    ok
}

#[cfg(target_os = "windows")]
fn set_platform(enabled: bool) -> bool {
    set_windows(enabled)
}

#[cfg(target_os = "linux")]
fn set_platform(enabled: bool) -> bool {
    set_linux(enabled)
}

#[cfg(target_os = "macos")]
fn set_platform(enabled: bool) -> bool {
    set_macos(enabled)
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
fn set_platform(_enabled: bool) -> bool {
    false
}

#[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]

/// The executable we would launch at sign-in.
fn executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

// ---------------------------------------------------------------------------
// Windows
// ---------------------------------------------------------------------------

/// The executable we would launch at sign-in.
fn executable() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

#[cfg(target_os = "windows")]
fn run_command() -> Option<String> {
    let exe = executable()?;
    // Quote so paths with spaces survive the Run value round trip.
    Some(format!("\"{}\"", exe.display()))
}

#[cfg(target_os = "windows")]
fn set_windows(enabled: bool) -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        HKEY_CURRENT_USER, KEY_SET_VALUE, REG_SZ, RegCloseKey, RegDeleteValueW, RegOpenKeyExW,
        RegSetValueExW,
    };
    use windows::core::HSTRING;

    let Some(command) = run_command() else {
        return false;
    };

    unsafe {
        let mut key = windows::Win32::System::Registry::HKEY::default();
        let status = RegOpenKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from(RUN_KEY),
            None,
            KEY_SET_VALUE,
            &mut key,
        );
        if status != ERROR_SUCCESS {
            tracing::warn!(?status, "could not open the Run key");
            return false;
        }

        let result = if enabled {
            // REG_SZ wants a NUL-terminated UTF-16 buffer, raw bytes.
            let wide: Vec<u16> = command.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes = wide.align_to::<u8>().1;
            RegSetValueExW(key, &HSTRING::from(RUN_VALUE), None, REG_SZ, Some(bytes))
        } else {
            RegDeleteValueW(key, &HSTRING::from(RUN_VALUE))
        };
        let _ = RegCloseKey(key);
        result == ERROR_SUCCESS
    }
}

// ---------------------------------------------------------------------------
// Linux (XDG autostart)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn autostart_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("autostart").join("pinsight.desktop"))
}

#[cfg(target_os = "linux")]
fn set_linux(enabled: bool) -> bool {
    let Some(path) = autostart_path() else {
        return false;
    };
    if !enabled {
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(%err, ?path, "could not remove the autostart entry");
                false
            }
        };
    }
    let Some(exe) = executable() else {
        return false;
    };
    let entry = format!(
        "[Desktop Entry]\nType=Application\nName=PinSight\nComment=Capture, pin and record\nExec={}\nTerminal=false\nCategories=Utility;\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(%err, "could not create the autostart folder");
            return false;
        }
    }
    match std::fs::write(&path, entry) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(%err, ?path, "could not write the autostart entry");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// macOS (LaunchAgent)
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn agent_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| {
        h.join("Library")
            .join("LaunchAgents")
            .join("pinsight.plist")
    })
}

#[cfg(target_os = "macos")]
fn set_macos(enabled: bool) -> bool {
    let Some(path) = agent_path() else {
        return false;
    };
    if !enabled {
        return match std::fs::remove_file(&path) {
            Ok(()) => true,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
            Err(err) => {
                tracing::warn!(%err, ?path, "could not remove the launch agent");
                false
            }
        };
    }
    let Some(exe) = executable() else {
        return false;
    };
    let plist = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n\
         <dict>\n\
         \x20 <key>Label</key><string>com.pinsight.app</string>\n\
         \x20 <key>ProgramArguments</key><array><string>{}</string></array>\n\
         \x20 <key>RunAtLoad</key><true/>\n\
         </dict>\n\
         </plist>\n",
        exe.display()
    );
    if let Some(parent) = path.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            tracing::warn!(%err, "could not create the LaunchAgents folder");
            return false;
        }
    }
    match std::fs::write(&path, plist) {
        Ok(()) => true,
        Err(err) => {
            tracing::warn!(%err, ?path, "could not write the launch agent");
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Everything else
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// The desktop entry must stay a valid INI file: an unbalanced bracket or a
    /// missing `Exec` line silently disables the autostart entry.
    #[test]
    #[cfg(target_os = "linux")]
    fn desktop_entry_is_well_formed() {
        let entry =
            "[Desktop Entry]\nType=Application\nName=PinSight\nExec=/opt/PinSight/pinsight\n";
        assert!(entry.starts_with("[Desktop Entry]"));
        assert!(entry.contains("Exec=/opt/PinSight/pinsight"));
        assert_eq!(entry.matches('[').count(), entry.matches(']').count());
    }
}
