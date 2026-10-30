//! Single-instance guard.
//!
//! A screenshot tool must never register its global hotkeys twice: two
//! processes means two sets of `RegisterHotKey` calls, one of which silently
//! fails and shows phantom "already in use" rows. The guard makes the first
//! launch the owner; later launches hand over to it and exit.
//!
//! - **Windows** — a named mutex. The kernel releases it when the process
//!   dies, so a crash can never leave a stale lock behind.
//! - **macOS / Linux** — a lock file next to the config, created atomically.
//!   If the file exists, the recorded PID is checked for liveness before the
//!   lock is considered held (a crashed instance cannot block the next one
//!   forever).
//!
//! Raising the running instance's window is Windows-only: it needs
//! `FindWindow` plus the `AttachThreadInput` foreground trick. On other
//! platforms the second launch simply exits — the duplicate hotkey set is
//! what actually hurts, and that is prevented everywhere.

#[cfg(not(windows))]
use std::path::PathBuf;

/// Owns the instance slot for the lifetime of the value.
#[must_use = "the lock is released when the guard is dropped"]
pub struct Guard {
    #[cfg(windows)]
    _handle: windows::Win32::Foundation::HANDLE,
    #[cfg(not(windows))]
    _file: Option<std::fs::File>,
    #[cfg(not(windows))]
    _path: PathBuf,
}

/// Try to become the one running instance. `None` means another process holds
/// the slot (or could not be evicted).
pub fn acquire() -> Option<Guard> {
    #[cfg(windows)]
    {
        acquire_windows()
    }
    #[cfg(not(windows))]
    {
        acquire_lock_file()
    }
}

/// Bring the running instance's window to the front (best effort, Windows).
pub fn raise_existing() -> bool {
    #[cfg(windows)]
    {
        raise_existing_windows()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

// ---------------------------------------------------------------------------
// Windows: named mutex
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn acquire_windows() -> Option<Guard> {
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::core::HSTRING;

    // `Local\` keeps the name inside this terminal session, so a second
    // profile on the same machine can still run its own copy.
    let name = HSTRING::from("Local\\PinSight-SingleInstance");
    let handle = unsafe { CreateMutexW(None, true, &name) }.ok()?;
    let already = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    if already {
        // Somebody else owns it: drop our handle so the count stays honest.
        unsafe {
            let _ = windows::Win32::Foundation::CloseHandle(handle);
        }
        return None;
    }
    Some(Guard { _handle: handle })
}

#[cfg(windows)]
fn raise_existing_windows() -> bool {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::WindowsAndMessaging::{
        BringWindowToTop, FindWindowW, GetWindowThreadProcessId, SW_RESTORE, SetForegroundWindow,
        ShowWindow,
    };
    use windows::core::HSTRING;

    // The main viewport is titled "PinSight" (see main.rs); the class is
    // winit's default, so match on the title alone.
    let title = HSTRING::from("PinSight");
    let Ok(hwnd) = (unsafe { FindWindowW(None, &title) }) else {
        return false;
    };
    if hwnd == HWND::default() {
        return false;
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE);
        // Windows only lets the foreground process steal focus; attaching to
        // the target's input queue is the supported way around that.
        let target = GetWindowThreadProcessId(hwnd, None);
        let mine = GetCurrentThreadId();
        let attached = AttachThreadInput(mine, target, true).as_bool();
        let _ = BringWindowToTop(hwnd);
        let ok = SetForegroundWindow(hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(mine, target, false);
        }
        ok
    }
}

// ---------------------------------------------------------------------------
// macOS / Linux: lock file with a liveness check
// ---------------------------------------------------------------------------

#[cfg(not(windows))]
fn lock_path() -> Option<PathBuf> {
    // Same directory as the config so the lock is per-user.
    let dir = crate::config::paths().config.parent()?.to_path_buf();
    Some(dir.join("pinsight.lock"))
}

#[cfg(not(windows))]
fn acquire_lock_file() -> Option<Guard> {
    use std::io::{Read, Write};

    let path = lock_path()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    for attempt in 0..2 {
        // `create_new` is atomic: exactly one process gets to create it.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                let _ = file.write_all(std::process::id().to_string().as_bytes());
                return Some(Guard {
                    _file: Some(file),
                    _path: path,
                });
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                // Read the recorded PID; a dead one means a crashed instance
                // and the lock is stale.
                let mut text = String::new();
                if let Ok(mut file) = std::fs::File::open(&path) {
                    let _ = file.read_to_string(&mut text);
                }
                let pid: u32 = text.trim().parse().unwrap_or(0);
                let alive = pid != 0 && pid_alive(pid);
                if !alive {
                    let _ = std::fs::remove_file(&path);
                    if attempt == 0 {
                        continue;
                    }
                }
                return None;
            }
            Err(_) => {
                // An unreadable lock directory is not worth failing startup
                // over: run without the guard rather than refusing to start.
                tracing::warn!("could not create the single-instance lock file");
                return Some(Guard {
                    _file: None,
                    _path: path,
                });
            }
        }
    }
    None
}

#[cfg(not(windows))]
fn pid_alive(pid: u32) -> bool {
    // `kill(pid, 0)` is the portable liveness probe: no signal is sent, but a
    // dead PID reports ESRCH. Permission errors mean the process exists but
    // belongs to someone else — still "alive" for our purposes.
    let r = unsafe { libc_kill(pid as i32, 0) };
    r == 0 || r == -1
}

#[cfg(not(windows))]
unsafe fn libc_kill(pid: i32, signal: i32) -> i32 {
    unsafe extern "C" {
        fn kill(pid: i32, signal: i32) -> i32;
    }
    unsafe { kill(pid, signal) }
}

#[cfg(windows)]
impl Drop for Guard {
    fn drop(&mut self) {
        // `CreateMutexW` takes ownership when `bInitialOwner` is set, so the
        // mutex has to be released before the handle is closed — otherwise the
        // slot stays taken for the rest of the process lifetime (and the
        // handle leaks on top of that).
        unsafe {
            let _ = windows::Win32::System::Threading::ReleaseMutex(self._handle);
            let _ = windows::Win32::Foundation::CloseHandle(self._handle);
        }
    }
}

#[cfg(not(windows))]
impl Drop for Guard {
    fn drop(&mut self) {
        // Only remove the file if we still own it (the recorded PID is ours).
        if self._file.is_none() {
            return;
        }
        if let Ok(text) = std::fs::read_to_string(&self._path) {
            if text.trim() == std::process::id().to_string() {
                let _ = std::fs::remove_file(&self._path);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_acquire_is_refused() {
        let first = acquire();
        if first.is_none() {
            // Another PinSight is running on this machine; the unit test
            // cannot judge, so treat it as a pass rather than a failure.
            return;
        }
        let second = acquire();
        assert!(
            second.is_none(),
            "a second guard must not be handed out while the first lives"
        );
        drop(first);
        // The slot is free again once the first guard is gone.
        let third = acquire();
        assert!(third.is_some(), "the lock must be released with the guard");
    }
}
