//! System-tray implementation for macOS and Linux via `tray-icon`.
//!
//! On Windows the tray is native Win32 glue in `tray.rs`; here we use the
//! cross-platform `tray-icon` crate (NSStatusItem on macOS, StatusNotifierItem
//! on Linux). The public surface mirrors `tray.rs` exactly so the app's tray
//! plumbing is identical on every platform:
//!
//! - [`TrayItem`] has the same Command / Separator / Submenu shape.
//! - [`Tray::install_with`] builds the icon + menu, returning `None` on failure
//!   just like the Windows backend.
//! - [`Tray::set_items`] rebuilds the popup when labels change.
//! - Menu clicks are relayed to the app through the same `TrayCommand` channel;
//!   [`poll_events`] must be called from the UI loop each frame.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::mpsc::Sender;

use tray_icon::TrayIcon;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

use crate::app::TrayCommand;

/// Mirror of the Windows `tray::TrayItem` so `tray_items()` compiles unchanged.
#[derive(Debug, Clone)]
pub enum TrayItem {
    Command {
        id: u32,
        label: String,
        command: TrayCommand,
    },
    Separator,
    Submenu {
        id: u32,
        label: String,
        items: Vec<TrayItem>,
    },
}

/// The live tray icon. Dropping it removes the icon from the system tray.
pub struct Tray {
    icon: TrayIcon,
}

// Globals shared between `install_with` / `set_items` (UI thread) and
// `poll_events` (also UI thread, but reached through tray-icon's global
// event channel). Kept as statics to mirror the Windows module's approach.
static TRAY_TX: Mutex<Option<Sender<TrayCommand>>> = Mutex::new(None);
static MENU_MAP: Mutex<HashMap<String, TrayCommand>> = Mutex::new(HashMap::new());
static WAKE: Mutex<Option<std::sync::Arc<dyn Fn() + Send + Sync>>> = Mutex::new(None);
static NEXT_ID: Mutex<u64> = Mutex::new(0);

fn fresh_id() -> String {
    let mut n = NEXT_ID.lock().unwrap_or_else(|e| e.into_inner());
    *n = n.wrapping_add(1);
    format!("ps-{}", *n)
}

/// Convert our portable item tree into a native menu, recording the
/// id → command mapping used to relay clicks later.
fn build_menu(items: &[TrayItem]) -> (Menu, HashMap<String, TrayCommand>) {
    let menu = Menu::new();
    let mut map = HashMap::new();
    populate(&menu, items, &mut map);
    (menu, map)
}

fn populate(menu: &Menu, items: &[TrayItem], map: &mut HashMap<String, TrayCommand>) {
    for item in items {
        match item {
            TrayItem::Separator => menu.append(&PredefinedMenuItem::separator()),
            TrayItem::Command { label, command, .. } => {
                let id = fresh_id();
                map.insert(id.clone(), *command);
                let entry = MenuItem::with_id(MenuId::new(id), label, true, None);
                let _ = menu.append(&entry);
            }
            TrayItem::Submenu { label, items, .. } => {
                let sub = Submenu::with_id(MenuId::new(fresh_id()), label, true);
                populate(&sub, items, map);
                let _ = menu.append(&sub);
            }
        }
    }
}

impl Tray {
    /// Install the icon. `custom` is an optional RGBA icon override; otherwise
    /// a simple embedded 32×32 PinSight mark is generated.
    pub fn install_with(
        tx: Sender<TrayCommand>,
        items: Vec<TrayItem>,
        tip: String,
        custom: Option<(u32, u32, Vec<u8>)>,
    ) -> Option<Self> {
        if let Ok(mut slot) = TRAY_TX.lock() {
            *slot = Some(tx);
        }
        let (menu, map) = build_menu(&items);
        if let Ok(mut slot) = MENU_MAP.lock() {
            *slot = map;
        }
        let (w, h, rgba) = custom.unwrap_or_else(default_icon);
        let icon = tray_icon::Icon::from_rgba(rgba, w, h).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(tip)
            .with_icon(icon)
            // Let the app handle left clicks (show/focus the window itself);
            // the context menu stays on right click, as on Windows.
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        Some(Self { icon })
    }

    /// Replace the popup menu when translated labels / pins change.
    pub fn set_items(&self, items: Vec<TrayItem>) {
        let (menu, map) = build_menu(&items);
        if let Ok(mut slot) = MENU_MAP.lock() {
            *slot = map;
        }
        self.icon.set_menu(Some(Box::new(menu)));
    }

    /// Update the hover tooltip (e.g. after switching language).
    pub fn set_tip(&self, tip: String) {
        let _ = self.icon.set_tooltip(Some(tip));
    }
}

/// A 32×32 indigo square with a light "pin" dot — a minimal, dependency-free
/// fallback when the caller has no custom icon pixels.
fn default_icon() -> (u32, u32, Vec<u8>) {
    let (w, h) = (32u32, 32u32);
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let inside = x >= 4 && x < 28 && y >= 4 && y < 28;
            let dot = (x as i32 - 16).pow(2) + (y as i32 - 14).pow(2) < 25;
            let (r, g, b, a) = if !inside {
                (0, 0, 0, 0)
            } else if dot {
                (240, 244, 255, 255)
            } else {
                (79, 70, 229, 255)
            };
            rgba[i..i + 4].copy_from_slice(&[r, g, b, a]);
        }
    }
    (w, h, rgba)
}

/// Register a wake callback so an idle (tray-hidden) app repaints when a menu
/// event arrives — otherwise `ControlFlow::Wait` would never deliver it.
pub fn set_wake(wake: std::sync::Arc<dyn Fn() + Send + Sync>) {
    if let Ok(mut slot) = WAKE.lock() {
        *slot = Some(wake);
    }
}

/// Drain queued tray menu events. Call once per UI frame on every platform.
/// Unknown ids are ignored (stale menu after a rebuild).
pub fn poll_events() {
    let mut events = Vec::new();
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        events.push(event.id.0.clone());
    }
    if events.is_empty() {
        return;
    }
    let commands: Vec<TrayCommand> = {
        let Ok(map) = MENU_MAP.lock() else {
            return;
        };
        events
            .iter()
            .filter_map(|id| map.get(id).copied())
            .collect()
    };
    if commands.is_empty() {
        return;
    }
    let sent = {
        let Ok(slot) = TRAY_TX.lock() else {
            return;
        };
        match slot.as_ref() {
            Some(tx) => commands.iter().all(|cmd| tx.send(*cmd).is_ok()),
            None => false,
        }
    };
    if sent {
        if let Ok(slot) = WAKE.lock() {
            if let Some(wake) = slot.as_ref() {
                wake();
            }
        }
    }
}
