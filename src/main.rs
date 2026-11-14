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
//! - `filters`  — whole-image adjustments (rotate/flip/invert/blur/emboss)
//! - `automation` — scheduling, region monitoring, after-capture workflow
//! - `pin`      — native top-most pin windows
//! - `window_shadow` — drop-shadow modes for whole-window captures
//! - `longshot` — scroll capture and stitching
//! - `record`   — frame capture and GIF/MP4 encoding
//! - `hotkeys`  — global hotkey registration
//! - `mcp`      — MCP stdio server

mod annotate;
mod app;
mod app_keys;
mod automation;
mod autorun;
mod background;
mod capture;
mod cli;
mod clipboard;
mod config;
mod decorate;
mod effects;
mod filters;
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
mod pin_layout;
#[cfg(not(windows))]
mod pin_viewport;
mod qr;
mod record;
mod single_instance;
mod snap;
mod startup;
mod text_font;
mod theme;
mod translate;
#[cfg(windows)]
mod tray;
#[cfg(not(windows))]
#[path = "tray_stub.rs"]
mod tray;
mod trim;
mod ui;
mod update;
mod upload;
mod upload_presets;
mod util;
mod voice;
mod window_shadow;

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
    let instance_guard = if mcp_mode {
        None
    } else {
        match single_instance::acquire() {
            Some(guard) => Some(guard),
            None => {
                // Another instance holds the slot. Raising its window is the
                // normal outcome — but if no window answers, the holder is
                // probably a PinSight that is *shutting down* right now (the
                // restart hand-off). Its mutex is released only when the
                // process fully exits, so wait briefly instead of quitting
                // and turning every in-app restart into a plain quit.
                if single_instance::raise_existing() {
                    tracing::info!("another PinSight is already running; raised it");
                    return Ok(());
                }
                tracing::info!("instance slot held but no window found; waiting for hand-off");
                let mut handed_off = None;
                for _ in 0..12 {
                    std::thread::sleep(std::time::Duration::from_millis(250));
                    if let Some(guard) = single_instance::acquire() {
                        handed_off = Some(guard);
                        break;
                    }
                }
                match handed_off {
                    Some(guard) => Some(guard),
                    None => {
                        tracing::info!("another PinSight is already running; raising it");
                        single_instance::raise_existing();
                        return Ok(());
                    }
                }
            }
        }
    };

    let (config, config_reset) = config::Config::load_detailed();

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
        Box::new(move |cc| {
            Ok(Box::new(app::PinSightApp::new(
                cc,
                config,
                mcp_mode,
                instance_guard,
                config_reset,
            )))
        }),
    )
}
