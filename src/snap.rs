//! Control-level ("smart") screenshot: snap the UI control under the cursor.
//!
//! Hybrid detection engine, chosen during the #128 technical-selection phase.
//! Each platform contributes what its APIs can actually resolve, and every
//! platform degrades gracefully to the same cross-platform fallback:
//!
//! | Platform | Control / Container | Window | Fallback |
//! |---|---|---|---|
//! | Windows | UI Automation `ElementFromPoint` (resolves modern UWP/WinUI/Electron widgets) | Win32 `GA_ROOT` hit-test | `list_windows()` |
//! | macOS   | Accessibility API `AXUIElementCopyElementAtPosition` (needs the user to grant Accessibility permission) | `list_windows()` | `list_windows()` |
//! | Linux (X11 / XWayland) | X11 window-tree walk — the deepest mapped window under the point (widgets that are not X11 windows — modern GTK/Qt — resolve to their nearest mapped ancestor) | `list_windows()` | `list_windows()` |
//! | Linux (native Wayland) | — the compositor does not expose window geometry to clients | `list_windows()` | `list_windows()` |
//!
//! The cross-platform fallback picks the **smallest** window from
//! [`capture::list_windows`] that contains the cursor, which is the right pick
//! under overlapping windows. All rectangles stay in the platform's native
//! global coordinate space — the same space `xcap` reports monitors/windows in,
//! which is what `capture::grab_physical_rect` consumes — so no conversion is
//! needed anywhere in the pipeline.

use crate::util::PhysRect;

/// How much of the surrounding UI to include when snapping a control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ControlLevel {
    /// The leaf control the cursor is over (a button, an input, a list row…).
    #[default]
    Control,
    /// The control's immediate container (a group box, a panel, a tab page…).
    Container,
    /// The whole top-level window the control lives in.
    Window,
}

/// Resolve the cursor position in the platform's global coordinate space.
///
/// Windows reads it straight from `GetCursorInfo`; elsewhere `enigo` already
/// translates across CoreGraphics (macOS) and X11 (Linux) for us.
pub fn cursor_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    {
        crate::capture::cursor_screen_pos()
    }
    #[cfg(not(windows))]
    {
        crate::capture::cursor_position()
    }
}

/// Resolve the rectangle of the control under `cursor` (global coordinates in
/// the platform's native space). Returns `None` when nothing could be resolved
/// there — the caller should tell the user rather than grab a wild rectangle.
pub fn control_rect_at(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    if matches!(level, ControlLevel::Window) && !cfg!(windows) {
        // Outside Windows, the window level is identical to the cross-platform
        // fallback (macOS/Linux have no cheaper exact hit-test).
        return window_rect_at(cursor);
    }
    platform_control_rect(cursor, level).or_else(|| window_rect_at(cursor))
}

/// The cross-platform fallback: the smallest listed window containing the
/// cursor. Works everywhere `xcap` enumerates windows, which covers all three
/// platforms in practice (native Wayland has no window list — see above).
fn window_rect_at(cursor: (i32, i32)) -> Option<PhysRect> {
    crate::capture::list_windows()
        .into_iter()
        .filter(|w| w.rect.contains(cursor.0, cursor.1))
        .min_by_key(|w| w.rect.w * w.rect.h)
        .map(|w| w.rect)
}

/// Platform engine. Windows has the full hybrid (UIA + Win32); macOS and Linux
/// resolve the control/container levels with their accessibility / window-tree
/// APIs and return `None` when unavailable so the fallback takes over.
#[cfg(windows)]
fn platform_control_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    // UIA is a classic COM API and must not run on the GUI thread (which is STA
    // and would deadlock waiting on the synchronous recognition). Run it on a
    // throwaway MTA worker — the same pattern the OCR path uses.
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel::<Option<PhysRect>>();
    std::thread::spawn(move || {
        let _ = tx.send(detect_on_worker(cursor, level));
    });
    // Bound the wait so a hung provider can never freeze the capture action.
    rx.recv_timeout(std::time::Duration::from_millis(800))
        .ok()
        .flatten()
}

#[cfg(windows)]
fn detect_on_worker(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
    // A fresh thread needs its own apartment. MTA avoids the STA re-entrancy
    // traps the synchronous UIA calls would hit.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    let initialized = hr.is_ok();
    let result = uia_rect(cursor, level).or_else(|| win32_rect(cursor, level));
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

/// UI Automation path: `ElementFromPoint` then optionally walk the tree up to a
/// container. Returns `None` for the `Window` level (the Win32 path is more
/// reliable for top-level windows) or when UIA cannot resolve anything.
#[cfg(windows)]
fn uia_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
    use windows::Win32::UI::Accessibility::{
        CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTreeWalker,
    };

    // The top-level window is better handled by the Win32 hit-test, so let that
    // take it and keep UIA focused on the control/container case.
    if matches!(level, ControlLevel::Window) {
        return None;
    }

    let automation: IUIAutomation =
        unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_ALL) }.ok()?;
    let pt = POINT {
        x: cursor.0,
        y: cursor.1,
    };
    let element: IUIAutomationElement = unsafe { automation.ElementFromPoint(pt) }.ok()?;

    // A container is one step up the raw view of the tree.
    let element = if matches!(level, ControlLevel::Container) {
        let walker: IUIAutomationTreeWalker = unsafe { automation.RawViewWalker() }.ok()?;
        unsafe { walker.GetParentElement(&element) }
            .ok()
            .unwrap_or(element)
    } else {
        element
    };

    let rect = unsafe { element.CurrentBoundingRectangle() }.ok()?;
    rect_to_phys(rect)
}

/// Win32 fallback: `WindowFromPoint` (deepest window at the point) plus an
/// optional step to the top-level window for the `Window` level. Covers legacy
/// Win32 controls UIA may miss.
#[cfg(windows)]
fn win32_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    use windows::Win32::Foundation::POINT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, GetParent, GetWindowRect, WindowFromPoint,
    };
    unsafe {
        let pt = POINT {
            x: cursor.0,
            y: cursor.1,
        };
        let mut hwnd = WindowFromPoint(pt);
        if hwnd.is_invalid() {
            return None;
        }
        match level {
            // The deepest window is usually the control when it is a real HWND.
            ControlLevel::Control => {}
            // The immediate parent of that deepest window.
            ControlLevel::Container => {
                if let Ok(parent) = GetParent(hwnd) {
                    if !parent.is_invalid() {
                        hwnd = parent;
                    }
                }
            }
            // The root ancestor = the top-level window.
            ControlLevel::Window => {
                let root = GetAncestor(hwnd, GA_ROOT);
                if !root.is_invalid() {
                    hwnd = root;
                }
            }
        }
        let mut rect = windows::Win32::Foundation::RECT::default();
        if GetWindowRect(hwnd, &mut rect).is_ok() {
            rect_to_phys(rect)
        } else {
            None
        }
    }
}

#[cfg(windows)]
fn rect_to_phys(rect: windows::Win32::Foundation::RECT) -> Option<PhysRect> {
    let w = (rect.right - rect.left).max(0) as u32;
    let h = (rect.bottom - rect.top).max(0) as u32;
    if w >= 1 && h >= 1 {
        Some(PhysRect {
            x: rect.left,
            y: rect.top,
            w,
            h,
        })
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// macOS — Accessibility API
// ---------------------------------------------------------------------------

#[cfg(target_os = "macos")]
fn platform_control_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    macos_ax_rect(cursor, level)
}

#[cfg(target_os = "macos")]
fn macos_ax_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    let (x, y, w, h) = ax::element_rect(cursor.0 as f64, cursor.1 as f64, level)?;
    let rect = PhysRect {
        x: x.round() as i32,
        y: y.round() as i32,
        w: w.round().max(1.0) as u32,
        h: h.round().max(1.0) as u32,
    };
    if rect.w >= 1 && rect.h >= 1 {
        Some(rect)
    } else {
        None
    }
}

/// Thin hand-written FFI over the macOS Accessibility + CoreFoundation APIs.
/// Kept dependency-free on purpose: the `AXUIElement*` functions live in the
/// ApplicationServices umbrella (no new crate), and the surface used here is
/// small and stable. `AXUIElementCopyElementAtPosition` requires the user to
/// grant the app Accessibility permission; when it is denied the calls return
/// an error and the cross-platform fallback takes over.
#[cfg(target_os = "macos")]
mod ax {
    use core::ffi::{c_char, c_void};

    /// Any CF/AX object reference, as used by these C APIs.
    pub type CF = *mut c_void;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Pt {
        pub x: f64,
        pub y: f64,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    pub struct Sz {
        pub width: f64,
        pub height: f64,
    }

    const K_CF_STRING_UTF8: u32 = 0x0800_0100;
    /// `kCFNumberFloat64Type`.
    const K_CF_NUMBER_FLOAT64: isize = 6;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFStringCreateWithCString(alloc: CF, cstr: *const u8, encoding: u32) -> CF;
        fn CFRelease(cf: CF);
    }

    #[link(name = "ApplicationServices", kind = "framework")]
    unsafe extern "C" {
        fn AXUIElementCreateSystemWide() -> CF;
        fn AXUIElementCopyElementAtPosition(element: CF, x: f64, y: f64, out: *mut CF) -> i32;
        fn AXUIElementCopyAttributeValue(element: CF, attr: CF, out: *mut CF) -> i32;
        fn AXValueGetValue(value: CF, ty: CF, out: *mut c_void) -> u8;
    }

    // The `kAXValue*Type` constants are CFStringRef globals exported by the
    // HIServices sub-framework. The ApplicationServices umbrella still links
    // functions, but its tbd file no longer lists these constants, so a static
    // `extern static` fails at link time ("Undefined symbols"). Resolve them at
    // runtime through dlsym instead — libSystem provides dlopen/dlsym, so no
    // extra link flag is needed.
    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flag: i32) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }
    /// `RTLD_NOW` on macOS.
    const RTLD_NOW: i32 = 2;

    /// Look up an exported CFStringRef constant by name. Returns `None` when
    /// the framework cannot be opened or the symbol is missing.
    fn ax_constant(name: &str) -> Option<CF> {
        const FRAMEWORK: &[u8] =
            b"/System/Library/Frameworks/ApplicationServices.framework/Versions/Current/ApplicationServices\0";
        static HANDLE: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        let handle = *HANDLE.get_or_init(|| unsafe {
            dlopen(FRAMEWORK.as_ptr() as *const c_char, RTLD_NOW) as usize
        });
        if handle == 0 {
            return None;
        }
        let mut symbol = name.as_bytes().to_vec();
        symbol.push(0);
        let address = unsafe { dlsym(handle as *mut c_void, symbol.as_ptr() as *const c_char) };
        if address.is_null() {
            None
        } else {
            // The symbol points at the CFStringRef; read its value once.
            Some(unsafe { *(address as *const CF) })
        }
    }

    /// Create a CFString from a Rust string. Leaks intentionally: these are a
    /// handful of fixed attribute-name strings per capture action, and the
    /// release churn is not worth the unsafe bookkeeping on a one-shot path.
    fn cf_str(s: &str) -> CF {
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        unsafe { CFStringCreateWithCString(std::ptr::null_mut(), bytes.as_ptr(), K_CF_STRING_UTF8) }
    }

    /// Rect (x, y, w, h in screen points) of the accessibility element at the
    /// position, or of its container when `container` is set. `None` when the
    /// position has no element (desktop) or the API is unavailable — most
    /// commonly because the user has not granted Accessibility permission.
    pub fn element_rect(
        x: f64,
        y: f64,
        level: super::ControlLevel,
    ) -> Option<(f64, f64, f64, f64)> {
        use super::ControlLevel;

        let attr_pos = cf_str("AXPosition");
        let attr_size = cf_str("AXSize");
        let result = unsafe {
            let system_wide = AXUIElementCreateSystemWide();
            if system_wide.is_null() {
                return None;
            }
            let mut element: CF = std::ptr::null_mut();
            // kAXErrorSuccess == 0.
            let err = AXUIElementCopyElementAtPosition(system_wide, x, y, &mut element);
            if err != 0 || element.is_null() {
                return None;
            }

            // Walk one step up for the container level.
            let target = if matches!(level, ControlLevel::Container) {
                let attr_parent = cf_str("AXParent");
                let mut parent: CF = std::ptr::null_mut();
                let perr = AXUIElementCopyAttributeValue(element, attr_parent, &mut parent);
                if perr == 0 && !parent.is_null() {
                    parent
                } else {
                    element
                }
            } else {
                element
            };

            let mut pos_value: CF = std::ptr::null_mut();
            let mut size_value: CF = std::ptr::null_mut();
            let err_pos = AXUIElementCopyAttributeValue(target, attr_pos, &mut pos_value);
            let err_size = AXUIElementCopyAttributeValue(target, attr_size, &mut size_value);
            if err_pos != 0 || err_size != 0 {
                return None;
            }

            let mut pt = Pt::default();
            let mut sz = Sz::default();
            let Some(point_type) = ax_constant("kAXValueCGPointType") else {
                return None;
            };
            let Some(size_type) = ax_constant("kAXValueCGSizeType") else {
                return None;
            };
            let ok_pos = AXValueGetValue(pos_value, point_type, &mut pt as *mut Pt as *mut c_void);
            let ok_size = AXValueGetValue(size_value, size_type, &mut sz as *mut Sz as *mut c_void);
            if ok_pos == 0 || ok_size == 0 {
                return None;
            }
            Some((pt.x, pt.y, sz.width, sz.height))
        };
        // The attribute-name strings are process-lifetime constants by design.
        unsafe {
            CFRelease(attr_pos);
            CFRelease(attr_size);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Linux — X11 window-tree walk (X11 and XWayland)
// ---------------------------------------------------------------------------

#[cfg(target_os = "linux")]
fn platform_control_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    x11_rect(cursor, level)
}

/// Walk the X11 window tree from the root and take the deepest mapped window
/// containing the point; its parent is the container and the topmost ancestor
/// below the root is the window. Modern GTK/Qt widgets are not X11 windows, so
/// they resolve to the nearest mapped ancestor — the honest ceiling of what X11
/// exposes without an AT-SPI (D-Bus) bridge.
#[cfg(target_os = "linux")]
fn x11_rect(cursor: (i32, i32), level: ControlLevel) -> Option<PhysRect> {
    use x11rb::connection::Connection;
    use x11rb::protocol::xproto::ConnectionExt as _;

    let (conn, screen_num) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen_num)?.root;

    // Stack of windows from the root down to the deepest hit, following the
    // topmost containing child at every level.
    let mut chain = vec![root];
    loop {
        let current = *chain.last()?;
        let children = match conn.query_tree(current).ok()?.reply() {
            Ok(reply) => reply.children,
            Err(_) => break,
        };
        // QueryTree lists children bottom-to-top, so the reverse iteration
        // tests the topmost sibling first.
        let hit = children.iter().rev().find(|&&child| {
            root_rect(&conn, root, child)
                .is_some_and(|r| r.w > 1 && r.h > 1 && r.contains(cursor.0, cursor.1))
        });
        match hit {
            Some(&child) => chain.push(child),
            None => break,
        }
    }

    // The cursor on the root window (empty desktop) has no control to offer —
    // the caller's cross-platform fallback handles that case better.
    if chain.len() < 2 {
        return None;
    }

    let target = match level {
        ControlLevel::Control => *chain.last()?,
        ControlLevel::Container => *chain.get(chain.len() - 2)?,
        // Unreachable here (the Window level is dispatched before the platform
        // engines on non-Windows), but keep the top-level ancestor for safety.
        ControlLevel::Window => chain.get(1).copied()?,
    };
    root_rect(&conn, root, target)
}

/// A window's rectangle translated into root coordinates (physical pixels on
/// X11 — the same space xcap reports monitors and windows in).
#[cfg(target_os = "linux")]
fn root_rect<C: x11rb::connection::Connection>(
    conn: &C,
    root: x11rb::protocol::xproto::Window,
    window: x11rb::protocol::xproto::Window,
) -> Option<crate::util::PhysRect> {
    use x11rb::protocol::xproto::ConnectionExt as _;
    let geometry = conn.get_geometry(window).ok()?.reply().ok()?;
    let position = conn
        .translate_coordinates(window, root, 0, 0)
        .ok()?
        .reply()
        .ok()?;
    Some(crate::util::PhysRect {
        x: position.dst_x as i32,
        y: position.dst_y as i32,
        w: geometry.width as u32,
        h: geometry.height as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_level_is_control() {
        assert_eq!(ControlLevel::default(), ControlLevel::Control);
    }

    #[test]
    fn all_levels_are_distinct() {
        let levels = [
            ControlLevel::Control,
            ControlLevel::Container,
            ControlLevel::Window,
        ];
        for (i, a) in levels.iter().enumerate() {
            for b in &levels[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn serde_round_trips() {
        for level in [
            ControlLevel::Control,
            ControlLevel::Container,
            ControlLevel::Window,
        ] {
            let json = serde_json::to_string(&level).unwrap();
            let back: ControlLevel = serde_json::from_str(&json).unwrap();
            assert_eq!(back, level);
        }
    }
}
