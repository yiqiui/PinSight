#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! PinSight — a native screenshot, pinboard, long-capture and recording tool.
//!
//! The binary is intentionally small: all behaviour lives in the library-style
//! modules (see `app.rs`). Run with `--mcp` to expose the MCP stdio server.
//!
//! Module map:
//! - `app`      — window shell, pages, background job plumbing
//! - `ui`       — painted widgets and the vector icon set
//! - `theme`    — palette, widget styling, typography scale
//! - `fonts`    — system font discovery (CJK coverage without a WebView)
//! - `i18n`     — all user-visible strings, 7 languages
//! - `capture`  — monitor enumeration and frame grabbing
//! - `overlay`  — frozen-screen selection surface
//! - `annotate` — annotation model, preview and rasterisation
//! - `pin`      — native top-most pin windows
//! - `longshot` — scroll capture and stitching
//! - `record`   — frame capture and GIF/MP4 encoding
//! - `hotkeys`  — global hotkey registration
//! - `mcp`      — MCP stdio server

mod annotate;
mod app;
mod app_keys;
mod background;
mod capture;
mod cli;
mod clipboard;
mod config;
mod decorate;
mod fonts;
mod gifedit;
mod hdr;
mod hotkeys;
mod hud;
mod i18n;
mod longshot;
mod marklayer;
mod mcp;
mod naming;
mod ocr;
mod overlay;
mod pdf;
mod picker;
mod pin;
#[cfg(not(windows))]
mod pin_viewport;
mod record;
mod single_instance;
mod startup;
mod text_font;
mod theme;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
#[path = "tray_stub.rs"]
mod tray;
mod ui;
mod upload;
mod util;
mod voice;

use eframe::egui;

fn main() -> eframe::Result {
    // The settings page owns the verbosity; `RUST_LOG` still wins so a
    // developer can debug without touching the config file.
    let log_level = config::Config::load().log_level;
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(log_level.as_filter())),
        )
        .with_writer(std::io::stderr)
        .init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("PinSight {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mcp_mode = args.iter().any(|a| a == "--mcp");

    // `pinsight shot …` runs headless (scrot/maim style) and never touches the
    // GUI, the tray or the single-instance slot.
    if let Some(result) = cli::maybe_run(&args) {
        if let Err(err) = result {
            eprintln!("pinsight: {err:#}");
            std::process::exit(1);
        }
        return Ok(());
    }

    // One instance per session: a second process would register a second set
    // of global hotkeys and report phantom conflicts. `--mcp` runs alongside
    // the GUI by design, so it never takes the slot.
    let _instance_guard = if mcp_mode {
        None
    } else {
        match single_instance::acquire() {
            Some(guard) => Some(guard),
            None => {
                tracing::info!("another PinSight is already running; raising it");
                single_instance::raise_existing();
                return Ok(());
            }
        }
    };

    let config = config::Config::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("PinSight")
        .with_inner_size([900.0, 660.0])
        .with_min_inner_size([720.0, 520.0])
        .with_app_id("pinsight");
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let native_options = eframe::NativeOptions {
        viewport,
        centered: true,
        ..Default::default()
    };

    eframe::run_native(
        "PinSight",
        native_options,
        Box::new(move |cc| Ok(Box::new(app::PinSightApp::new(cc, config, mcp_mode)))),
    )
}
