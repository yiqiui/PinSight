//! Windows notification-area (tray) icon and its context menu.
//!
//! The icon lives on a message-only window created on the **UI thread**, so the
//! messages winit pumps for the main window also reach our window procedure —
//! no extra thread and no cross-thread window handling. Menu selections travel
//! back to the app over an mpsc channel, which the UI loop drains each frame.
//!
//! Everything here is best effort: if the shell refuses the icon we simply do
//! not show one, and the app keeps working with its window and hotkeys.

// The whole module is Win32 glue written as `unsafe fn`s; the edition-2024
// "unsafe block inside unsafe fn" lint would only add noise here.
#![allow(unsafe_op_in_unsafe_fn)]

use std::sync::Mutex;
use std::sync::mpsc::Sender;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos, HICON, HWND_MESSAGE, IMAGE_FLAGS,
    MENU_ITEM_FLAGS, MF_POPUP, MF_SEPARATOR, MF_STRING, RegisterClassW, RegisterWindowMessageW,
    SetForegroundWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TrackPopupMenu, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_COMMAND, WM_CONTEXTMENU, WM_LBUTTONDBLCLK, WM_RBUTTONUP, WNDCLASSW,
    WS_OVERLAPPED,
};
use windows::core::{HSTRING, PCWSTR};

use crate::app::TrayCommand;

const TRAY_CLASS: &str = "PinSightTray";
/// Callback message the shell sends to our window.
const WM_TRAY_CALLBACK: u32 = WM_APP + 1;

/// One row of the tray menu.
///
/// A row is either a command, a separator, or a submenu with its own rows —
/// the "Pins" submenu lists the pins that are currently open.
#[derive(Debug, Clone)]
pub enum TrayItem {
    /// A clickable row that sends a command back to the app.
    Command {
        id: u32,
        label: String,
        command: TrayCommand,
    },
    /// A horizontal rule.
    Separator,
    /// A row that opens a nested menu.
    Submenu {
        id: u32,
        label: String,
        items: Vec<TrayItem>,
    },
}

/// Multi-resolution icon compiled into the binary so the tray works from an
/// installed, portable or development layout alike.
static TRAY_ICON: &[u8] = include_bytes!("../assets/tray.ico");

/// Pull one icon image out of an embedded `.ico`, picking the entry whose size
/// is closest to `want`.
///
/// `CreateIconFromResourceEx` expects a bare `RT_ICON` resource — the DIB image
/// on its own — and rejects the ICO container header, so the entry has to be
/// sliced out by hand.
fn icon_entry(bytes: &[u8], want: u32) -> Option<&[u8]> {
    let count = u16::from_le_bytes([*bytes.get(4)?, *bytes.get(5)?]) as usize;
    let mut best: Option<(u32, usize, usize)> = None;
    let mut offset = 6;
    for _ in 0..count {
        let w = *bytes.get(offset)? as u32;
        let h = *bytes.get(offset + 1)? as u32;
        let size =
            u32::from_le_bytes(bytes.get(offset + 8..offset + 12)?.try_into().ok()?) as usize;
        let at = u32::from_le_bytes(bytes.get(offset + 12..offset + 16)?.try_into().ok()?) as usize;
        offset += 16;
        // A stored 0 means 256.
        let (w, h) = (if w == 0 { 256 } else { w }, if h == 0 { 256 } else { h });
        let score = w.abs_diff(want) + h.abs_diff(want);
        if best.is_none_or(|(best_score, _, _)| score < best_score) {
            best = Some((score, at, size));
        }
    }
    let (_, at, size) = best?;
    bytes.get(at..at + size)
}

/// Build an `HICON` from the embedded file at (roughly) `size` pixels.
fn load_icon(size: u32) -> windows::core::Result<HICON> {
    let dib = icon_entry(TRAY_ICON, size).ok_or_else(windows::core::Error::from_thread)?;
    unsafe {
        CreateIconFromResourceEx(
            dib,
            true,
            0x0003_0000,
            size as i32,
            size as i32,
            IMAGE_FLAGS(0),
        )
    }
}

/// Build an `HICON` from straight RGBA pixels (32×32 is what the shell asks
/// for). Used for the white and custom tray styles, which have no `.ico`.
pub fn icon_from_rgba(width: u32, height: u32, rgba: &[u8]) -> Option<HICON> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
        DeleteObject, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};

    if rgba.len() < (width * height * 4) as usize {
        return None;
    }
    unsafe {
        let mut bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let color = CreateDIBSection(None, &mut bmi, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(color.0));
            return None;
        }
        // GDI wants BGRA; the source is RGBA. Premultiply as well, because
        // `CreateIconIndirect` treats the bitmap as premultiplied alpha.
        let dst = std::slice::from_raw_parts_mut(bits as *mut u8, (width * height * 4) as usize);
        for i in 0..(width * height) as usize {
            let a = rgba[i * 4 + 3] as u32;
            let scale = |c: u8| ((c as u32 * a) / 255) as u8;
            dst[i * 4] = scale(rgba[i * 4 + 2]);
            dst[i * 4 + 1] = scale(rgba[i * 4 + 1]);
            dst[i * 4 + 2] = scale(rgba[i * 4]);
            dst[i * 4 + 3] = a as u8;
        }
        // The AND mask is unused for 32bpp icons, but must exist.
        let mask = CreateBitmap(width as i32, height as i32, 1, 1, None);
        let info = ICONINFO {
            fIcon: true.into(),
            xHotspot: 0,
            yHotspot: 0,
            hbmMask: mask,
            hbmColor: color,
        };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(HGDIOBJ(mask.0));
        let _ = DeleteObject(HGDIOBJ(color.0));
        icon
    }
}

/// A process-wide singleton: the window procedure is a plain `extern "system"`
/// function, so it can only reach the app state through a static.
///
/// `Option` (rather than `OnceLock`) because installing and tearing the icon
/// down has to be able to clear the slot again.
static TRAY_STATE: Mutex<Option<TrayState>> = Mutex::new(None);

/// `HICON`/`HWND` are plain pointers. They are only ever touched on the UI
/// thread, which is what makes the static sound.
#[derive(Clone, Copy)]
struct Handle<T>(T);
unsafe impl<T> Send for Handle<T> {}
unsafe impl<T> Sync for Handle<T> {}

struct TrayState {
    tx: Sender<TrayCommand>,
    /// The menu rows, in display order.
    items: Vec<TrayItem>,
    tip: String,
    icon: Handle<HICON>,
    hwnd: Handle<HWND>,
    /// `RegisterWindowMessageW("TaskbarCreated")` — the explorer restart ping.
    taskbar_created: u32,
}

/// Guard for a live tray icon; dropping it removes the icon again.
pub struct Tray;

impl Tray {
    /// Install the icon. Returns `None` when the shell refuses it.
    /// `custom` overrides the embedded artwork with raw RGBA pixels.
    pub fn install_with(
        tx: Sender<TrayCommand>,
        items: Vec<TrayItem>,
        tip: String,
        custom: Option<(u32, u32, Vec<u8>)>,
    ) -> Option<Self> {
        let hwnd = create_message_window()?;
        // The notification area asks for the small icon; 32 px covers the
        // default 100 % and 125 % scaling without an upscale blur.
        let built = custom
            .as_ref()
            .and_then(|(w, h, rgba)| icon_from_rgba(*w, *h, rgba))
            .or_else(|| load_icon(32).ok());
        let Some(icon) = built else {
            tracing::warn!("could not build the tray icon from the embedded resource");
            let _ = unsafe { DestroyWindow(hwnd) };
            return None;
        };

        let taskbar_created = unsafe { RegisterWindowMessageW(&HSTRING::from("TaskbarCreated")) };

        let state = TrayState {
            tx,
            items,
            tip,
            icon: Handle(icon),
            hwnd: Handle(hwnd),
            taskbar_created,
        };
        {
            let mut slot = match TRAY_STATE.lock() {
                Ok(slot) => slot,
                Err(poisoned) => poisoned.into_inner(),
            };
            if slot.is_some() {
                tracing::warn!("tray state already installed");
                drop(slot);
                unsafe {
                    let _ = DestroyIcon(icon);
                    let _ = DestroyWindow(hwnd);
                }
                return None;
            }
            *slot = Some(state);
        }

        if !add_icon() {
            let _ = TRAY_STATE.lock().map(|mut slot| slot.take());
            unsafe {
                let _ = DestroyIcon(icon);
                let _ = DestroyWindow(hwnd);
            }
            return None;
        }
        Some(Self)
    }

    /// Replace the menu rows.
    ///
    /// The popup is built fresh every time the menu opens, so this only has to
    /// update the stored copy — the shell does not need to be told.
    pub fn set_items(&self, items: Vec<TrayItem>) {
        let mut slot = match TRAY_STATE.lock() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(state) = slot.as_mut() {
            state.items = items;
        }
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        // Copy the handles out and drop the guard before calling into Win32,
        // so nothing holds the lock across a foreign call.
        let Some(state) = (match TRAY_STATE.lock() {
            Ok(mut slot) => slot.take(),
            Err(poisoned) => poisoned.into_inner().take(),
        }) else {
            return;
        };
        unsafe {
            let data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: state.hwnd.0,
                uID: 1,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            let _ = DestroyIcon(state.icon.0);
            let _ = DestroyWindow(state.hwnd.0);
        }
    }
}

/// Snapshot of what the icon needs, taken while the lock is held only briefly.
struct IconSpec {
    hwnd: HWND,
    icon: HICON,
    tip: String,
    taskbar_created: u32,
}

fn icon_spec() -> Option<IconSpec> {
    let slot = TRAY_STATE.lock().ok()?;
    let state = slot.as_ref()?;
    Some(IconSpec {
        hwnd: state.hwnd.0,
        icon: state.icon.0,
        tip: state.tip.clone(),
        taskbar_created: state.taskbar_created,
    })
}

/// Build the `NOTIFYICONDATAW` for the current state and hand it to the shell.
fn add_icon() -> bool {
    let Some(spec) = icon_spec() else {
        return false;
    };
    let mut tip: [u16; 128] = [0; 128];
    for (slot, unit) in tip.iter_mut().zip(spec.tip.encode_utf16()) {
        *slot = unit;
    }
    let data = NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: spec.hwnd,
        uID: 1,
        uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
        uCallbackMessage: WM_TRAY_CALLBACK,
        hIcon: spec.icon,
        szTip: tip,
        ..Default::default()
    };
    let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
    if !ok.as_bool() {
        tracing::warn!("the shell refused the tray icon");
    }
    ok.as_bool()
}

/// A message-only window: no taskbar entry, no z-order, just a message sink.
fn create_message_window() -> Option<HWND> {
    unsafe {
        let hinstance = GetModuleHandleW(None).ok()?;
        let class = HSTRING::from(TRAY_CLASS);
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinstance.into(),
            lpszClassName: PCWSTR(class.as_ptr()),
            ..Default::default()
        };
        // A previous run in this process may have registered it already.
        let _ = RegisterClassW(&wc);

        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            PCWSTR(class.as_ptr()),
            PCWSTR::null(),
            WINDOW_STYLE(WS_OVERLAPPED.0),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(hinstance.into()),
            None,
        )
        .ok()
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // Explorer restarts and forgets every tray icon; re-arm ours. The id is
    // only meaningful once something is installed, hence the `!= 0` guard.
    if msg != 0 {
        if let Some(spec) = icon_spec() {
            if msg == spec.taskbar_created {
                add_icon();
                return LRESULT(0);
            }
        }
    }

    match msg {
        WM_TRAY_CALLBACK => {
            // The low word of lParam is the actual mouse or keyboard event.
            let event = (lparam.0 & 0xffff) as u32;
            match event {
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                WM_LBUTTONDBLCLK => send_command(TrayCommand::ShowWindow),
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as u32;
            tracing::debug!(msg, id, "tray window got WM_COMMAND");
            run_command(id);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Wakes the UI thread after a command is queued.
///
/// The app runs with `ControlFlow::Wait`, so an idle app does not repaint —
/// and `poll_tray` only runs inside the update pass. Without this nudge a
/// queued command would sit unread until the user happened to move the mouse
/// over the window, which is exactly the "tray menu does nothing" bug.
static WAKE: std::sync::Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> =
    std::sync::Mutex::new(None);

pub fn set_wake(wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut slot) = WAKE.lock() {
        *slot = Some(wake);
    }
}

fn wake_ui() {
    let wake = WAKE.lock().ok().and_then(|slot| slot.clone());
    if let Some(wake) = wake {
        wake();
    }
}

fn send_command(command: TrayCommand) {
    let tx = lock_state().as_ref().map(|state| state.tx.clone());
    if let Some(tx) = tx {
        let _ = tx.send(command);
    }
    // "Open PinSight" must work even when the window is hidden and the UI
    // thread is asleep, so restore it here rather than waiting for a frame.
    if command == TrayCommand::ShowWindow {
        show_main_window();
    }
    wake_ui();
}

/// Best-effort restore of the main window from the tray thread.
fn show_main_window() {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow,
    };
    let title = windows::core::HSTRING::from("PinSight");
    if let Ok(hwnd) = unsafe { FindWindowW(None, &title) } {
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

fn run_command(id: u32) {
    let found = lock_state().as_ref().and_then(|state| {
        find_command(&state.items, id)
    });
    tracing::debug!(id, ?found, "tray run_command");
    if let Some(command) = found {
        send_command(command);
    }
}

/// Depth-first search for the command behind a menu id.
fn find_command(items: &[TrayItem], id: u32) -> Option<TrayCommand> {
    for item in items {
        match item {
            TrayItem::Command {
                id: item_id,
                command,
                ..
            } if *item_id == id => return Some(*command),
            TrayItem::Submenu { id: item_id, items, .. } if *item_id == id => {
                return None;
            }
            TrayItem::Submenu { items, .. } => {
                if let Some(found) = find_command(items, id) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

/// Lock the tray state, tolerating a poisoned mutex: a panic while the lock is
/// held must not take the tray down with it.
fn lock_state() -> std::sync::MutexGuard<'static, Option<TrayState>> {
    match TRAY_STATE.lock() {
        Ok(slot) => slot,
        Err(poisoned) => poisoned.into_inner(),
    }
}

unsafe fn show_menu(hwnd: HWND) {
    // Copy the rows out first: the menu is modal and pumps messages, so holding
    // the lock across it would deadlock against the WM_COMMAND it posts.
    let items = lock_state().as_ref().map(|state| state.items.clone());
    let Some(items) = items else {
        return;
    };

    let Ok(menu) = CreatePopupMenu() else {
        return;
    };
    // Submenus are separate popup menus; the parent keeps a reference so they
    // are not garbage collected while the menu is up.
    let mut submenus = Vec::new();
    append_items(menu, &items, &mut submenus);

    let mut point = windows::Win32::Foundation::POINT::default();
    let _ = GetCursorPos(&mut point);
    // Without this the menu stays open when it loses focus.
    let _ = SetForegroundWindow(hwnd);
    let picked = TrackPopupMenu(
        menu,
        TPM_LEFTALIGN | TPM_RETURNCMD,
        point.x,
        point.y,
        None,
        hwnd,
        None,
    );
    let _ = DestroyMenu(menu);
    for submenu in submenus {
        let _ = DestroyMenu(submenu);
    }
    if picked.0 != 0 {
        run_command(picked.0 as u32);
    }
}

/// Append `items` to `parent`, creating popup menus for the submenus.
unsafe fn append_items(
    parent: windows::Win32::UI::WindowsAndMessaging::HMENU,
    items: &[TrayItem],
    submenus: &mut Vec<windows::Win32::UI::WindowsAndMessaging::HMENU>,
) {
    for item in items {
        match item {
            TrayItem::Separator => {
                let _ = AppendMenuW(
                    parent,
                    MENU_ITEM_FLAGS(MF_SEPARATOR.0),
                    0,
                    &HSTRING::from(""),
                );
            }
            TrayItem::Command { id, label, .. } => {
                let _ = AppendMenuW(
                    parent,
                    MENU_ITEM_FLAGS(MF_STRING.0),
                    *id as usize,
                    &HSTRING::from(label),
                );
            }
            TrayItem::Submenu { id, label, items } => {
                let Ok(sub) = CreatePopupMenu() else {
                    continue;
                };
                append_items(sub, items, submenus);
                // `MF_POPUP` makes the row open `sub`; the "id" is the menu
                // handle, not a command id, so the row itself is not clickable.
                let _ = AppendMenuW(
                    parent,
                    MENU_ITEM_FLAGS(MF_POPUP.0 | MF_STRING.0),
                    sub.0 as usize,
                    &HSTRING::from(label),
                );
                let _ = id;
                submenus.push(sub);
            }
        }
    }
}
