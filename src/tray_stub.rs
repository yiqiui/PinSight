//! No-op stand-in for the Windows tray, compiled on every non-Windows target.
//!
//! [`crate::app::PinSightApp`] owns a `tray` field and tray plumbing on all
//! platforms; mirroring the Windows module's public surface here keeps that
//! code compiling unchanged. Everything is inert: `install_with` always
//! reports "not installed", exactly like a Windows machine whose shell
//! refused the icon — the app keeps working with its window and hotkeys.
//!
//! `dead_code` is allowed because most call sites are `#[cfg(windows)]`-gated:
//! on this platform the types exist (so the field type checks) but nothing
//! constructs or calls them.
#![allow(dead_code)]

use std::sync::mpsc::Sender;

use crate::app::TrayCommand;

/// Mirror of the Windows [`crate::tray::TrayItem`]: same variants, so
/// `PinSightApp::tray_items` compiles unchanged on every platform.
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

/// Inert tray handle; never installed on this platform.
pub struct Tray;

impl Tray {
    /// Always `None`: there is no notification area here.
    pub fn install_with(
        _tx: Sender<TrayCommand>,
        _items: Vec<TrayItem>,
        _tip: String,
        _custom: Option<(u32, u32, Vec<u8>)>,
    ) -> Option<Self> {
        None
    }

    /// No-op: no popup menu exists to update.
    pub fn set_items(&self, _items: Vec<TrayItem>) {}
}

/// No-op wake hook: no tray exists to wake the UI loop.
pub fn set_wake(_wake: std::sync::Arc<dyn Fn() + Send + Sync>) {}
