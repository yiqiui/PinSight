//! Application shell: navigation, capture page, pin board, settings and the
//! background job plumbing.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use base64::Engine as _;
use eframe::CreationContext;
use eframe::egui;
use image::RgbaImage;

use crate::annotate::ImgRect;
use crate::background::{self, Background};
use crate::capture;
use crate::config::BgFit;
use crate::config::PinsSub;
use crate::config::SettingsTab;
use crate::config::{self, Config, Hotkeys as HotkeyConfig};
use crate::fonts::{self, FontKind, FontReport};
use crate::hotkeys::{self, Action, BindingError, Hotkeys};
use crate::i18n::{Lang, Translator};
use crate::longshot::{self, LongShotJob, LongShotUpdate};
use crate::mcp::{self, CapturedImage, McpHandle, PinSummary, StatusInfo, UiCommand};
use crate::overlay::{CaptureSession, Intent, OverlayAction, OverlayOutcome};
use crate::pin::{Pin, PinEvent};
use crate::record::{self, RecordJob, RecordUpdate};
use crate::startup;
use crate::theme::{self, Palette, ThemeMode};
use crate::ui::{self, Icon};
use crate::util;

const TOAST_LIFETIME: Duration = Duration::from_secs(6);

/// Sub-pages of the Controls settings tab (mirrors Snipaste: global hotkeys,
/// in-app hotkeys, mouse).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ControlsSub {
    Global,
    AppKeys,
    Mouse,
}

struct Toast {
    text: String,
    error: bool,
    at: Instant,
}

enum AreaJob {
    Overlay(Intent),
    FullCopy,
}

struct PendingArea {
    job: AreaJob,
    at: Instant,
}

struct PendingMcpCapture {
    monitor_id: Option<u32>,
    region: Option<util::PhysRect>,
    save: bool,
    return_base64: bool,
    reply: std::sync::mpsc::Sender<Result<CapturedImage, String>>,
    at: Instant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Capture,
    History,
    Pins,
    Settings,
    /// GIF editor: trim, crop, add text, convert to MP4.
    Gif,
}

/// One image file found in the save directory, for the gallery page.
#[derive(Clone)]
struct HistoryFile {
    path: PathBuf,
    modified: std::time::SystemTime,
}

/// Commands the native tray menu can send back into the UI loop. Only Windows
/// builds one, so the variants look unused everywhere else.
#[cfg_attr(not(windows), allow(dead_code))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrayCommand {
    CaptureArea,
    CaptureFull,
    PinClipboard,
    LongCapture,
    Record,
    SaveLast,
    /// Show or hide every open pin at once.
    TogglePins,
    /// Bring a specific pin to the front (the "Pins" submenu).
    FocusPin(u64),
    /// Show or hide every pin in a group.
    ToggleGroup(u32),
    /// Reopen the most recently closed pin.
    RestorePin,
    /// Drop the capture history.
    ClearHistory,
    /// Temporarily release every global hotkey.
    ToggleHotkeys,
    /// Open the releases page in the browser.
    CheckUpdate,
    /// Relaunch the process.
    Restart,
    /// Open the settings window on the Updates tab.
    ShowSettings,
    /// Open the documentation in the browser.
    Help,
    ShowWindow,
    Quit,
    /// Open the in-app GIF editor page.
    EditGif,
    /// Record a voice note from the tray.
    RecordVoice,
}

/// A pin that was closed: enough to recreate it exactly where it was.
struct ClosedPin {
    image: Arc<RgbaImage>,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    group: u32,
}

/// State for the in-app GIF editor page.
struct GifEditorState {
    input: String,
    /// 0 = trim, 1 = crop, 2 = text, 3 = mp4.
    op: u8,
    start: u32,
    end: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    text: String,
    font_size: u32,
    /// 0..=4 → TextPos variants.
    pos: u8,
    speed: i32,
    output: String,
    status: Option<(String, bool)>,
}

impl Default for GifEditorState {
    fn default() -> Self {
        Self {
            input: String::new(),
            op: 0,
            start: 0,
            end: 0,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            text: String::new(),
            font_size: 32,
            pos: 3,
            speed: 10,
            output: String::new(),
            status: None,
        }
    }
}

pub struct PinSightApp {
    pub(crate) config: Config,
    pub(crate) hotkeys: Hotkeys,
    /// Index of the in-app shortcut row currently being recorded.
    recording_app_key: Option<usize>,
    /// Which sub-page of the Controls settings tab is showing.
    controls_sub: ControlsSub,
    /// Which sub-page of the Pins settings tab is showing.
    pins_sub: PinsSub,
    /// When the process started, for the one-shot idle working-set trim.
    started: std::time::Instant,
    last_trim: std::time::Instant,
    tr: Translator,
    mcp_handle: Option<McpHandle>,
    ui_rx: Option<Receiver<UiCommand>>,
    capture: Option<CaptureSession>,
    pins: Vec<Pin>,
    pin_tx: Sender<PinEvent>,
    pin_rx: Receiver<PinEvent>,
    /// Active upload job, if any. The HTTP call runs on a worker thread so the
    /// GUI never blocks; this channel carries the resulting link (or error
    /// string) back.
    upload_rx: Option<std::sync::mpsc::Receiver<Result<String, String>>>,
    /// GIF editor state and its background job channel.
    gif_editor: GifEditorState,
    gif_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    /// Background voice-note recording result channel.
    voice_rx: Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    next_pin_id: u64,
    root_restore: Option<egui::Pos2>,
    pending_area: Option<PendingArea>,
    /// Whether the main window has already been parked for the pending capture.
    /// A delayed capture parks only when its countdown ends, so the user can
    /// still see the countdown toast until then.
    pending_parked: bool,
    pending_park_at: Option<Instant>,
    /// Open window-capture picker, if any. Holds the enumerated windows so the
    /// list is stable while the modal is open.
    window_picker: Option<Vec<crate::capture::WindowInfo>>,
    /// Cached gallery listing, refreshed at most once a second so a big capture
    /// folder is not re-read on every frame.
    history_scan: Option<(Instant, Vec<HistoryFile>)>,
    /// Gallery items the user ticked for batch export to PDF.
    history_selection: std::collections::HashSet<PathBuf>,
    pending_mcp: Option<PendingMcpCapture>,
    long_job: Option<LongShotJob>,
    record_job: Option<RecordJob>,
    /// Annotation layer over the running recording, open while the user draws.
    mark_layer: Option<crate::marklayer::MarkLayer>,
    toast: Option<Toast>,
    last_image: Option<Arc<RgbaImage>>,
    /// Recent captures, newest first. Bounded so a long session cannot grow
    /// the resident set without limit.
    history: Vec<Arc<RgbaImage>>,
    /// Pins the user closed, newest last, so they can be brought back.
    closed_pins: std::collections::VecDeque<ClosedPin>,
    /// Open colour dialog, if any.
    picker: Option<crate::picker::ColorPicker>,
    /// Installed font families, enumerated on first use and cached.
    font_families: Option<Vec<(String, std::path::PathBuf)>>,
    /// Group that new pins join, and the group the pins page shows.
    active_group: u32,
    /// Group name being typed in the rename box, if any.
    group_rename: Option<(u32, String)>,
    last_saved: Option<PathBuf>,
    ffmpeg: bool,
    // ---- view state -------------------------------------------------------
    page: Page,
    textures: HashMap<String, egui::TextureHandle>,
    font_report: FontReport,
    hotkey_draft: HotkeyConfig,
    save_dir_draft: String,
    name_template_draft: String,
    font_path_draft: String,
    last_capture_at: Option<Instant>,
    /// Decoded + blurred custom wallpaper, when the user picked one.
    background: Option<Background>,
    settings_tab: SettingsTab,
    /// Index of the hotkey row currently recording a combination, if any.
    recording_hotkey: Option<usize>,
    /// Set while the tray menu is open so the click that dismisses it does not
    /// also fall through to the UI.
    tray_rx: Option<Receiver<TrayCommand>>,
    /// Set by the tray's "Quit" item so the close it triggers is not turned
    /// into "hide to tray" by [`Self::handle_close_request`].
    quitting: bool,
    /// Live tray icon. `None` when the shell refused it or on other platforms.
    #[cfg(windows)]
    tray: Option<crate::tray::Tray>,
    /// Cache of the state the tray labels depend on, so the menu is only
    /// rebuilt when something actually changed.
    #[cfg(windows)]
    /// Everything the tray rows are derived from, so a stale copy is never
    /// shown: pin count, global hide, hotkeys, enhanced menu, and the group
    /// table (ids, names, hidden flags).
    #[allow(clippy::type_complexity)]
    tray_sig: (usize, bool, bool, bool, Vec<(u32, String, bool)>),
}

impl PinSightApp {
    pub fn new(cc: &CreationContext<'_>, config: Config, mcp_mode: bool) -> Self {
        let tr = Translator::new(config.lang);
        // HDR-aware capture routing is a capture-pipeline decision, so it is
        // pushed into `capture` once at startup and again on every toggle.
        crate::capture::set_prefer_sdr_on_hdr(config.hdr_fix);
        // eframe asks the GL driver for its limit (often 16k) and the glyph
        // atlas is allocated as `max_texture_side × growing height`. Capping it
        // keeps that allocation in the low megabytes; the largest texture we
        // ever upload is a 420 px thumbnail.
        cc.egui_ctx.all_styles_mut(|style| {
            let limit = style.visuals.text_options.max_texture_side.min(4096);
            style.visuals.text_options.max_texture_side = limit;
        });
        theme::apply(
            &cc.egui_ctx,
            config.theme,
            config.accent,
            config.accent_custom,
        );
        let font_report = fonts::apply(
            &cc.egui_ctx,
            config.lang,
            config.font,
            config.font_path.as_deref(),
            config.font_family.as_deref(),
            config.font_size_pt,
        );

        if let Err(err) = config.ensure_save_dir() {
            tracing::warn!(%err, "could not create output folder");
        }
        let mut hotkeys = Hotkeys::register(&config.hotkeys);
        // Honour "hotkeys disabled" from the last session: registering above
        // succeeds regardless, so release them again when the user turned the
        // feature off.
        if !config.hotkeys_enabled {
            hotkeys.unbind_all();
        }

        let (ui_tx, ui_rx) = std::sync::mpsc::channel();
        let mcp_handle = if mcp_mode {
            Some(mcp::start(ui_tx, config.save_dir.clone()))
        } else {
            None
        };
        let (pin_tx, pin_rx) = std::sync::mpsc::channel();

        let settings_tab = config.settings_tab;
        let mut app = Self {
            hotkey_draft: config.hotkeys.clone(),
            recording_app_key: None,
            controls_sub: ControlsSub::Global,
            pins_sub: PinsSub::Display,
            started: std::time::Instant::now(),
            last_trim: std::time::Instant::now(),
            save_dir_draft: config.save_dir.display().to_string(),
            name_template_draft: config.name_template.clone(),
            font_path_draft: config
                .font_path
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            active_group: config.pin_groups.first().map(|g| g.id).unwrap_or(1),
            config,
            hotkeys,
            tr,
            mcp_handle,
            ui_rx: if mcp_mode { Some(ui_rx) } else { None },
            capture: None,
            pins: Vec::new(),
            pin_tx,
            pin_rx,
            upload_rx: None,
            gif_editor: GifEditorState::default(),
            gif_rx: None,
            voice_rx: None,
            next_pin_id: 1,
            root_restore: None,
            pending_area: None,
            pending_parked: false,
            pending_park_at: None,
            window_picker: None,
            history_scan: None,
            history_selection: Default::default(),
            pending_mcp: None,
            long_job: None,
            record_job: None,
            mark_layer: None,
            toast: None,
            last_image: None,
            history: Vec::new(),
            closed_pins: std::collections::VecDeque::new(),
            picker: None,
            font_families: None,
            group_rename: None,
            last_saved: None,
            ffmpeg: record_ffmpeg(),
            page: Page::Capture,
            textures: HashMap::new(),
            font_report,
            last_capture_at: None,
            background: None,
            settings_tab,
            recording_hotkey: None,
            tray_rx: None,
            quitting: false,
            #[cfg(windows)]
            tray: None,
            #[cfg(windows)]
            tray_sig: (0, false, true, true, Vec::new()),
        };

        app.install_tray(&cc.egui_ctx);
        app.sync_background(&cc.egui_ctx);

        if app.hotkeys.failures.is_empty() {
            app.toast(app.tr.tr.ready, false);
        } else {
            let msg = app.hotkey_failure_message();
            app.toast(msg, true);
        }
        app
    }

    /// Human readable summary of the shortcuts that could not be registered,
    /// e.g. `Shortcut unavailable: Ctrl+Shift+A — already used by another app`.
    fn hotkey_failure_message(&self) -> String {
        let t = self.tr.tr;
        let Some((action, kind)) = self.hotkeys.failures.first().copied() else {
            return t.ready.to_string();
        };
        let combo = self.config.hotkeys.for_action(action);
        let reason = match kind {
            BindingError::InUse => t.hotkey_in_use,
            BindingError::Invalid => t.hotkey_invalid,
            BindingError::Backend => t.hotkey_warning,
        };
        let extra = self.hotkeys.failures.len().saturating_sub(1);
        if extra > 0 {
            format!("{}: {combo} — {reason} (+{extra})", t.hotkey_unavailable)
        } else {
            format!("{}: {combo} — {reason}", t.hotkey_unavailable)
        }
    }

    // ---- plumbing ---------------------------------------------------------

    fn busy_with_capture(&self) -> bool {
        self.capture.is_some() || self.pending_area.is_some() || self.pending_mcp.is_some()
    }

    fn toast(&mut self, text: impl Into<String>, error: bool) {
        self.toast = Some(Toast {
            text: text.into(),
            error,
            at: Instant::now(),
        });
    }

    fn park_root(&mut self, ctx: &egui::Context) {
        if self.root_restore.is_some() {
            return;
        }
        self.root_restore = ctx.input(|i| i.viewport().outer_rect.map(|r| r.min));
        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
            -10_000.0, -10_000.0,
        )));
        ctx.request_repaint();
    }

    fn restore_root(&mut self, ctx: &egui::Context) {
        if let Some(pos) = self.root_restore.take() {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(pos));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            ctx.request_repaint();
            // The capture overlay is gone; drop the cold font/glyph pages it
            // faulted in so an idle tray process stays lean.
            crate::util::trim_working_set();
        }
    }

    // ---- colour dialog ----------------------------------------------------

    fn open_color_picker(&mut self, target: crate::picker::ColorTarget) {
        use crate::picker::ColorTarget;
        let rgb = match target {
            ColorTarget::Accent => self.config.accent_custom,
            ColorTarget::CaptureMask => self.config.capture_mask_color,
            ColorTarget::CaptureAnchor => self.config.capture_anchor_color,
            ColorTarget::PinShadow => self.config.pin_shadow_color,
            ColorTarget::PinTextBg => self.config.pin_text_bg,
            ColorTarget::PinTextFg => self.config.pin_text_fg,
            ColorTarget::DecoBorder => self
                .config
                .decorations
                .border
                .as_ref()
                .map(|b| [b.color[0], b.color[1], b.color[2]])
                .unwrap_or([255, 255, 255]),
            ColorTarget::DecoShadow => self
                .config
                .decorations
                .shadow
                .as_ref()
                .map(|s| s.color)
                .unwrap_or([0, 0, 0]),
        };
        self.picker = Some(crate::picker::ColorPicker::new(
            target,
            rgb,
            self.config.custom_colors.clone(),
        ));
    }

    fn apply_picked_color(
        &mut self,
        target: crate::picker::ColorTarget,
        color: egui::Color32,
        ctx: &egui::Context,
    ) {
        use crate::picker::ColorTarget;
        let rgb = [color.r(), color.g(), color.b()];
        match target {
            ColorTarget::Accent => {
                self.config.accent = theme::Accent::Custom;
                self.config.accent_custom = rgb;
                self.apply_theme(ctx);
            }
            ColorTarget::CaptureMask => self.config.capture_mask_color = rgb,
            ColorTarget::CaptureAnchor => self.config.capture_anchor_color = rgb,
            ColorTarget::PinShadow => self.config.pin_shadow_color = rgb,
            ColorTarget::PinTextBg => self.config.pin_text_bg = rgb,
            ColorTarget::PinTextFg => self.config.pin_text_fg = rgb,
            ColorTarget::DecoBorder => {
                if let Some(b) = &mut self.config.decorations.border {
                    b.color = [rgb[0], rgb[1], rgb[2], b.color[3]];
                }
            }
            ColorTarget::DecoShadow => {
                if let Some(s) = &mut self.config.decorations.shadow {
                    s.color = rgb;
                }
            }
        }
        let _ = self.config.save();
    }

    /// Modal list of capturable top-level windows. Picking one captures it and
    /// copies it to the clipboard (the same result as a full-screen copy).
    fn show_window_picker(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(windows) = self.window_picker.clone() else {
            return;
        };
        let t = self.tr.tr;
        let mut close = false;
        let mut picked: Option<u32> = None;
        egui::Window::new(t.window_picker_title)
            .collapsible(false)
            .resizable(true)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                if windows.is_empty() {
                    ui.label(t.window_picker_empty);
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for w in &windows {
                            let title = if w.title.trim().is_empty() {
                                w.app.clone()
                            } else {
                                w.title.clone()
                            };
                            let label = format!("{}  —  {}×{}", title, w.rect.w, w.rect.h);
                            if ui.selectable_label(false, label).clicked() {
                                picked = Some(w.id);
                                close = true;
                            }
                        }
                    });
                ui.add_space(8.0);
                if ui::primary_button(ui, p, t.key_cancel).clicked() {
                    close = true;
                }
            });
        if let Some(id) = picked {
            self.capture_window_now(id);
        }
        if close {
            self.window_picker = None;
        }
    }

    fn capture_window_now(&mut self, id: u32) {
        match capture::grab_window(id) {
            Ok(image) => {
                let image = Arc::new(image);
                match copy_to_clipboard(&self.config, &image) {
                    Ok(()) => {
                        if self.config.clipboard_mode != config::ClipboardMode::Disabled {
                            self.toast(self.tr.tr.copied, false);
                        }
                    }
                    Err(err) => self.toast(format!("{}: {err}", self.tr.tr.clipboard_error), true),
                }
                self.last_capture_at = Some(Instant::now());
                self.last_image = Some(image);
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.capture_failed), true),
        }
    }

    /// Draw the colour dialog and act on what it decided.
    fn show_color_picker(&mut self, ctx: &egui::Context, p: &Palette) {
        let Some(mut picker) = self.picker.take() else {
            return;
        };
        let action = picker.show(ctx, p, self.tr.tr);
        match action {
            None => self.picker = Some(picker),
            Some(crate::picker::PickerAction::Cancel) => {
                self.config.custom_colors = picker.custom.clone();
                let _ = self.config.save();
            }
            Some(crate::picker::PickerAction::PickScreen) => {
                self.config.custom_colors = picker.custom.clone();
                self.picker = Some(picker);
                self.begin_area(Intent::PickColor, ctx);
            }
            Some(crate::picker::PickerAction::Apply(color)) => {
                self.config.custom_colors = picker.custom.clone();
                self.apply_picked_color(picker.target, color, ctx);
            }
        }
    }

    /// Floating status pills for the background jobs, plus the recording
    /// annotation layer.
    ///
    /// These live in their own always-on-top viewports rather than inside the
    /// main window: while a job runs the main window is parked off-screen so it
    /// cannot bleed into the captured pixels — which would also hide anything
    /// drawn in it. A separate window keeps the status visible where the user
    /// is looking, and its buttons are the answer to "how do I stop this?".
    fn show_job_huds(&mut self, ctx: &egui::Context, p: &Palette) {
        let toast = self
            .toast
            .as_ref()
            .map(|t| t.text.clone())
            .unwrap_or_default();

        // Keep both floating HUD pills out of the Windows.Graphics.Capture
        // pipeline. `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` makes the
        // windows render as transparent to WGC while staying fully visible and
        // interactive on screen — so neither pill is ever baked into a
        // screenshot, GIF or recorded frame, and there is no hide/show flicker.
        // The call is idempotent, so re-applying it every frame is harmless and
        // also re-covers a pill whose window was recreated.
        crate::capture::set_capture_exclusion(crate::hud::RECORD_WINDOW_TITLE);
        crate::capture::set_capture_exclusion(crate::hud::LONG_WINDOW_TITLE);

        if let Some(job) = self.record_job.as_ref() {
            match crate::hud::show_record(ctx, job, self.tr.tr, p, Some(toast.as_str())) {
                Some(crate::hud::RecordHudAction::Pause) => job.pause(),
                Some(crate::hud::RecordHudAction::Resume) => job.resume(),
                Some(crate::hud::RecordHudAction::Stop) => {
                    job.stop();
                    self.toast(self.tr.tr.stopping_recording, false);
                }
                Some(crate::hud::RecordHudAction::Annotate) => self.open_mark_layer(ctx),
                None => {}
            }
        } else if self.mark_layer.is_some() {
            // The recording ended under the layer (limit reached, or stopped
            // from the tray / MCP): the layer goes with the job.
            self.mark_layer = None;
        }

        if let Some(job) = self.long_job.as_ref() {
            // The pill is excluded from WGC via display affinity (see above), so
            // it is simply shown every frame; no hide/show juggling is needed.
            if crate::hud::show_long(ctx, job, self.tr.tr, p, Some(toast.as_str())) {
                job.stop();
                self.toast(self.tr.tr.long_cancelled, false);
            }
        }

        if let Some(layer) = self.mark_layer.as_mut() {
            let outcome = layer.show(ctx);
            if outcome.cleared {
                self.toast(self.tr.tr.mk_cleared, false);
            }
            if outcome.close {
                self.mark_layer = None;
            }
        }
    }

    /// Open the annotation layer over the region currently being recorded.
    fn open_mark_layer(&mut self, ctx: &egui::Context) {
        let Some(job) = self.record_job.as_ref() else {
            return;
        };
        if self.mark_layer.is_some() {
            return;
        }
        let theme = match self.config.theme {
            ThemeMode::Light => egui::Theme::Light,
            ThemeMode::Dark => egui::Theme::Dark,
            ThemeMode::System => ctx.theme(),
        };
        self.mark_layer = Some(crate::marklayer::MarkLayer::new(
            job.region,
            job.monitor,
            job.scale,
            self.tr.tr,
            Palette::for_theme(theme, self.config.accent, self.config.accent_custom),
        ));
    }

    /// Arm a capture: honour the configured delay, then let `process_pending`
    /// park the window and grab. Parking happens there (not here) so a delayed
    /// capture keeps the window — and its countdown toast — visible until the
    /// countdown ends.
    fn start_pending(&mut self, job: AreaJob, ctx: &egui::Context) {
        let delay = self.config.capture_delay_secs.min(30) as u64;
        self.pending_parked = false;
        self.pending_park_at = None;
        self.pending_area = Some(PendingArea {
            job,
            at: Instant::now() + Duration::from_secs(delay),
        });
        if delay > 0 {
            self.toast(format!("{} {}", delay, self.tr.tr.cap_delay_unit), false);
        }
        ctx.request_repaint_after(Duration::from_millis(40));
    }

    fn begin_area(&mut self, intent: Intent, ctx: &egui::Context) {
        if self.busy_with_capture() {
            self.toast(self.tr.tr.capture_in_progress, true);
            return;
        }
        self.start_pending(AreaJob::Overlay(intent), ctx);
    }

    fn begin_full_copy(&mut self, ctx: &egui::Context) {
        if self.busy_with_capture() {
            self.toast(self.tr.tr.capture_in_progress, true);
            return;
        }
        self.start_pending(AreaJob::FullCopy, ctx);
    }

    fn process_pending(&mut self, ctx: &egui::Context) {
        const PARK_DELAY: Duration = Duration::from_millis(140);

        // Phase 1: honour the configured delay (0 s = immediately).  The window
        // stays visible during the countdown so the toast can be read.
        if let Some(pending) = self.pending_area.as_ref() {
            if Instant::now() < pending.at {
                ctx.request_repaint_after(Duration::from_millis(100));
                return;
            }
        }
        // Phase 2: park the main window, then let DWM drop it from the desktop
        // frame before grabbing, so it cannot appear in the capture.
        if self.pending_area.is_some() && !self.pending_parked {
            self.park_root(ctx);
            self.pending_parked = true;
            self.pending_park_at = Some(Instant::now());
            ctx.request_repaint_after(Duration::from_millis(40));
            return;
        }
        if self.pending_area.is_some()
            && self
                .pending_park_at
                .map(|t| t.elapsed() < PARK_DELAY)
                .unwrap_or(false)
        {
            ctx.request_repaint_after(Duration::from_millis(40));
            return;
        }
        if let Some(pending) = self.pending_area.take() {
            match pending.job {
                AreaJob::Overlay(intent) => {
                    let pointer = self.config.capture_pointer;
                    if let Ok(monitor) = capture::monitor_at_cursor() {
                        if crate::hdr::is_hdr(&monitor.rect) {
                            self.toast(self.tr.tr.hdr_toast, false);
                        }
                    }
                    let result = capture::monitor_at_cursor().and_then(|monitor| {
                        let mut image = capture::grab_monitor(&monitor)?;
                        if pointer {
                            capture::composite_cursor(&mut image, (monitor.rect.x, monitor.rect.y));
                        }
                        Ok((monitor, image))
                    });
                    self.restore_root(ctx);
                    match result {
                        Ok((monitor, image)) => {
                            let mut session = CaptureSession::new(
                                monitor,
                                image,
                                intent,
                                self.tr.tr,
                                // Follow the *configured* theme, not the
                                // overlay viewport's default: the overlay is
                                // its own viewport and would otherwise stay
                                // dark while the app runs in light mode.
                                match self.config.theme {
                                    ThemeMode::Light => Palette::for_theme(
                                        egui::Theme::Light,
                                        self.config.accent,
                                        self.config.accent_custom,
                                    ),
                                    ThemeMode::Dark => Palette::for_theme(
                                        egui::Theme::Dark,
                                        self.config.accent,
                                        self.config.accent_custom,
                                    ),
                                    ThemeMode::System => Palette::for_theme(
                                        ctx.theme(),
                                        self.config.accent,
                                        self.config.accent_custom,
                                    ),
                                },
                            );
                            // Apply the settings-page preferences to this run.
                            session.prefs = crate::overlay::OverlayPrefs {
                                border_width: self.config.capture_border_width,
                                mask_alpha: self.config.capture_mask_alpha,
                                mask_color: self.config.capture_mask_color,
                                anchors: self.config.capture_anchors,
                                anchor_color: self.config.capture_anchor_color,
                                crosshair: self.config.capture_crosshair,
                                hints: self.config.capture_hints,
                                esc_confirm: self.config.capture_esc_confirm,
                                double_click: self.config.capture_double_click,
                                middle_click: self.config.capture_middle_click,
                                keys: self.config.app_keys.clone(),
                                magnifier: self.config.capture_magnifier,
                                magnifier_size: self.config.capture_magnifier_size,
                                color_format: self.config.color_format,
                            };
                            self.capture = Some(session);
                        }
                        Err(err) => {
                            tracing::warn!(%err, "overlay capture failed");
                            self.toast(format!("{}: {err}", self.tr.tr.capture_failed), true)
                        }
                    }
                }
                AreaJob::FullCopy => {
                    let pointer = self.config.capture_pointer;
                    let result = capture::grab_full_screen().map(|(monitor, mut image)| {
                        if pointer {
                            capture::composite_cursor(&mut image, (monitor.rect.x, monitor.rect.y));
                        }
                        (monitor, image)
                    });
                    self.restore_root(ctx);
                    match result {
                        Ok((_monitor, image)) => {
                            let image = Arc::new(image);
                            match copy_to_clipboard(&self.config, &image) {
                                Ok(()) => {
                                    if self.config.clipboard_mode != config::ClipboardMode::Disabled
                                    {
                                        self.toast(self.tr.tr.copied, false);
                                    }
                                }
                                Err(err) => self
                                    .toast(format!("{}: {err}", self.tr.tr.clipboard_error), true),
                            }
                            self.last_capture_at = Some(Instant::now());
                            self.last_image = Some(image);
                        }
                        Err(err) => {
                            self.toast(format!("{}: {err}", self.tr.tr.capture_failed), true)
                        }
                    }
                }
            }
        }

        if let Some(pending) = &self.pending_mcp {
            if pending.at.elapsed() < PARK_DELAY {
                ctx.request_repaint_after(Duration::from_millis(40));
                return;
            }
            let Some(pending) = self.pending_mcp.take() else {
                return;
            };

            let captured = (|| -> Result<(RgbaImage, Option<PathBuf>), String> {
                let image = if let Some(rect) = pending.region {
                    capture::grab_physical_rect(rect).map_err(|e| e.to_string())?
                } else {
                    let monitor = match pending.monitor_id {
                        Some(id) => capture::find_monitor(id).map_err(|e| e.to_string())?,
                        None => capture::monitor_at_cursor().map_err(|e| e.to_string())?,
                    };
                    capture::grab_monitor(&monitor).map_err(|e| e.to_string())?
                };
                let path = if pending.save {
                    Some(
                        util::save_image(&image, &self.config.save_dir, &self.save_options())
                            .map_err(|e| e.to_string())?,
                    )
                } else {
                    None
                };
                Ok((image, path))
            })();

            self.restore_root(ctx);
            match captured {
                Ok((image, path)) => {
                    let base64 = if pending.return_base64 {
                        encode_png_base64(&image).ok()
                    } else {
                        None
                    };
                    let width = image.width();
                    let height = image.height();
                    self.last_image = Some(Arc::new(image));
                    self.last_capture_at = Some(Instant::now());
                    if let Some(path) = &path {
                        self.last_saved = Some(path.clone());
                    }
                    let _ = pending.reply.send(Ok(CapturedImage {
                        path,
                        width,
                        height,
                        base64,
                    }));
                }
                Err(err) => {
                    let _ = pending.reply.send(Err(err.clone()));
                    self.toast(format!("{}: {err}", self.tr.tr.capture_failed), true);
                }
            }
        }
    }

    fn process_ui_commands(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.ui_rx.take() else { return };
        let commands: Vec<UiCommand> = rx.try_iter().collect();
        self.ui_rx = Some(rx);
        for command in commands {
            self.handle_ui_command(command, ctx);
        }
    }

    fn handle_ui_command(&mut self, command: UiCommand, ctx: &egui::Context) {
        match command {
            UiCommand::Capture {
                monitor_id,
                region,
                save,
                return_base64,
                reply,
            } => {
                if self.busy_with_capture() {
                    let _ = reply.send(Err("a capture is already in progress".into()));
                    return;
                }
                self.park_root(ctx);
                self.pending_mcp = Some(PendingMcpCapture {
                    monitor_id,
                    region,
                    save,
                    return_base64,
                    reply,
                    at: Instant::now(),
                });
                ctx.request_repaint_after(Duration::from_millis(40));
            }
            UiCommand::Pin {
                path,
                base64,
                reply,
            } => {
                let result = load_image(path.as_deref(), base64.as_deref())
                    .map(|image| self.add_pin(image))
                    .map_err(|e| e.to_string());
                let _ = reply.send(result);
            }
            UiCommand::ListPins { reply } => {
                let pins = self
                    .pins
                    .iter()
                    .map(|pin| PinSummary {
                        id: pin.id,
                        width: pin.image.width(),
                        height: pin.image.height(),
                        x: pin.x as f32,
                        y: pin.y as f32,
                    })
                    .collect();
                let _ = reply.send(Ok(pins));
            }
            UiCommand::ClosePin { id, reply } => {
                let existed = self.pins.iter().any(|pin| pin.id == id);
                if let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == id) {
                    pin.close();
                }
                self.pins.retain(|pin| pin.id != id);
                self.textures.remove(&format!("pin-{id}"));
                let _ = reply.send(if existed {
                    Ok(())
                } else {
                    Err(format!("no pin with id {id}"))
                });
            }
            UiCommand::StartRecording {
                monitor_id,
                region,
                fps,
                reply,
            } => {
                if self.record_job.is_some() {
                    let _ = reply.send(Err("already recording".into()));
                    return;
                }
                let monitor = match monitor_id {
                    Some(id) => capture::find_monitor(id).map_err(|e| e.to_string()),
                    None => capture::monitor_at_cursor().map_err(|e| e.to_string()),
                };
                let monitor = match monitor {
                    Ok(monitor) => monitor,
                    Err(err) => {
                        let _ = reply.send(Err(err));
                        return;
                    }
                };
                let relative = match region {
                    Some(rect) => ImgRect::from_min_size(
                        (rect.x - monitor.rect.x) as f32,
                        (rect.y - monitor.rect.y) as f32,
                        rect.w as f32,
                        rect.h as f32,
                    )
                    .clamped(monitor.rect.w, monitor.rect.h),
                    None => ImgRect::from_min_size(
                        0.0,
                        0.0,
                        monitor.rect.w as f32,
                        monitor.rect.h as f32,
                    ),
                };
                if self.busy_with_capture() {
                    let _ = reply.send(Err("a capture is in progress".into()));
                    return;
                }
                self.park_root(ctx);
                let job = record::start(
                    monitor.id,
                    relative,
                    fps,
                    &self.config.save_dir,
                    &self.save_options(),
                    self.config.record_max_seconds,
                    self.config.record_audio,
                    self.config.audio_device.clone(),
                );
                let path = job.path.display().to_string();
                self.record_job = Some(job);
                let _ = reply.send(Ok(path));
            }
            UiCommand::StopRecording { reply } => match &self.record_job {
                Some(job) => {
                    job.stop();
                    let _ = reply.send(Ok(job.path.display().to_string()));
                    self.toast(self.tr.tr.stopping_recording, false);
                }
                None => {
                    let _ = reply.send(Err("not recording".into()));
                }
            },
            UiCommand::Status { reply } => {
                let status = StatusInfo {
                    version: env!("CARGO_PKG_VERSION").to_string(),
                    pins: self.pins.len(),
                    recording: self.record_job.is_some(),
                    save_dir: self.config.save_dir.display().to_string(),
                    mcp_requests: self
                        .mcp_handle
                        .as_ref()
                        .map(|h| h.requests.load(std::sync::atomic::Ordering::Relaxed))
                        .unwrap_or(0),
                };
                let _ = reply.send(Ok(status));
            }
        }
    }

    fn poll_jobs(&mut self, ctx: &egui::Context) {
        if let Some(mut job) = self.long_job.take() {
            match job.poll() {
                Some(LongShotUpdate::Finished(image)) => {
                    self.restore_root(ctx);
                    match longshot::save(&image, &self.config.save_dir, &self.save_options()) {
                        Ok(path) => {
                            self.last_saved = Some(path.clone());
                            self.last_capture_at = Some(Instant::now());
                            // A long screenshot is usually far taller than the
                            // screen, so a pin would render it as an unreadable
                            // thin strip. Open the saved file in the default
                            // viewer instead, where the full image can be
                            // scrolled and zoomed.
                            util::open_path(&path);
                            self.toast(
                                format!("{}: {}", self.tr.tr.long_saved, path.display()),
                                false,
                            );
                        }
                        Err(err) => self.toast(format!("{}: {err}", self.tr.tr.save_failed), true),
                    }
                }
                Some(LongShotUpdate::Failed(err)) => {
                    self.restore_root(ctx);
                    self.toast(format!("{}: {err}", self.tr.tr.long_failed), true)
                }
                Some(LongShotUpdate::Progress { frames, height }) => {
                    // Live feedback during the multi-second auto-scroll: each
                    // tick refreshes the toast (resetting its lifetime) so the
                    // progress stays on screen until the capture finishes,
                    // instead of the plain "started" toast expiring at 6s.
                    self.toast(
                        format!(
                            "{} · {} {} · {}px",
                            self.tr.tr.long_progress, frames, self.tr.tr.frames, height
                        ),
                        false,
                    );
                    self.long_job = Some(job);
                }
                None => {
                    self.long_job = Some(job);
                }
            }
            ctx.request_repaint_after(Duration::from_millis(200));
        }

        if let Some(mut job) = self.record_job.take() {
            match job.poll() {
                Some(RecordUpdate::Finished { path, frames }) => {
                    self.restore_root(ctx);
                    self.last_saved = Some(path.clone());
                    self.toast(
                        format!(
                            "{}: {} ({} {})",
                            self.tr.tr.saved,
                            path.display(),
                            frames,
                            self.tr.tr.frames
                        ),
                        false,
                    );
                }
                Some(RecordUpdate::Failed(err)) => {
                    self.restore_root(ctx);
                    self.toast(format!("{}: {err}", self.tr.tr.save_failed), true)
                }
                Some(RecordUpdate::Progress { .. }) => {
                    self.record_job = Some(job);
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
                None => {
                    self.record_job = Some(job);
                    ctx.request_repaint_after(Duration::from_millis(250));
                }
            }
        }
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context) {
        let actions = self.hotkeys.poll();
        if !actions.is_empty() {
            tracing::debug!(?actions, "global hotkey fired");
        }
        for action in actions {
            match action {
                Action::CaptureArea => self.begin_area(Intent::Copy, ctx),
                Action::CaptureFull => self.begin_full_copy(ctx),
                Action::PinClipboard => self.pin_clipboard(),
                Action::LongCapture => {
                    if let Some(job) = &self.long_job {
                        job.stop();
                        self.toast(self.tr.tr.long_cancelled, false);
                    } else {
                        self.begin_area(Intent::LongShot, ctx);
                    }
                }
                Action::Record => {
                    if self.record_job.is_some() {
                        if let Some(job) = &self.record_job {
                            job.stop();
                            self.toast(self.tr.tr.stopping_recording, false);
                        }
                    } else {
                        self.begin_area(Intent::Record, ctx);
                    }
                }
                Action::SaveLast => self.save_last(),
            }
            ctx.request_repaint();
        }
    }

    fn pin_clipboard(&mut self) {
        // An image is the common case; text is rendered to an image when the
        // "convert text to an image" option is on.
        if let Ok(image) = clipboard_image() {
            self.add_pin(image);
            self.toast(self.tr.tr.clipboard_pinned, false);
            return;
        }
        if self.config.pin_text_to_image {
            if let Some(text) = clipboard_text() {
                // A single existing file path is usually an Explorer copy, not
                // something the user wants pinned as text.
                if self.config.pin_skip_file_paths && looks_like_file_path(&text) {
                    self.toast(self.tr.tr.clipboard_no_image, true);
                    return;
                }
                if let Some(image) = crate::text_font::render_text_image(
                    &text,
                    self.config.pin_text_margin,
                    self.config.pin_text_max_width,
                    self.config.pin_text_font_size,
                    self.config.pin_text_bg,
                    self.config.pin_text_fg,
                ) {
                    self.add_pin(image);
                    self.toast(self.tr.tr.clipboard_pinned, false);
                    return;
                }
            }
        }
        self.toast(self.tr.tr.clipboard_no_image, true);
    }

    /// How a capture should be named and encoded, taken from the settings.
    fn save_options(&self) -> util::SaveOptions {
        util::SaveOptions {
            template: self.config.name_template.clone(),
            quality: self.config.image_quality,
            format: self.config.image_format,
            title: String::new(),
            decorations: self.config.decorations.clone(),
        }
    }

    fn save_last(&mut self) {
        let Some(image) = self.last_image.clone() else {
            self.toast(self.tr.tr.nothing_to_save, true);
            return;
        };
        match util::save_image(&image, &self.config.save_dir, &self.save_options()) {
            Ok(path) => {
                self.last_saved = Some(path.clone());
                if self.config.capture_sound_save {
                    util::play_notification(self.config.sound_file.as_deref());
                }
                self.toast(format!("{}: {}", self.tr.tr.saved, path.display()), false);
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.save_failed), true),
        }
    }

    /// Remember a capture in the rolling history.
    ///
    /// The history is capped so a long session cannot grow the resident set
    /// without bound; the oldest entry falls off the end.
    fn push_history(&mut self, image: &Arc<RgbaImage>) {
        // The cap comes from the settings page (1–200, so a typo cannot make
        // the process hold every capture of the session).
        let limit = self.config.capture_history_limit.clamp(1, 200) as usize;
        self.history.insert(0, image.clone());
        self.history.truncate(limit);
    }

    /// Keep a closed pin around so it can be reopened.
    ///
    /// Only the bitmap and geometry are kept: a pin is a window, and the
    /// window is exactly what the user asked to get rid of. `pin_restore_limit`
    /// of 0 turns the whole feature off.
    fn push_closed_pin(&mut self, closed: ClosedPin) {
        let limit = self.config.pin_restore_limit.min(50) as usize;
        if limit == 0 {
            self.closed_pins.clear();
            return;
        }
        self.closed_pins.push_back(closed);
        while self.closed_pins.len() > limit {
            self.closed_pins.pop_front();
        }
    }

    /// Bring back the most recently closed pin, at its old position.
    fn restore_closed_pin(&mut self) {
        let Some(closed) = self.closed_pins.pop_back() else {
            self.toast(self.tr.tr.pin_nothing_to_restore, true);
            return;
        };
        let id = self.next_pin_id;
        self.next_pin_id += 1;
        let group = self.resolve_group(closed.group);
        let pin_w = closed.w.max(24);
        let pin_h = closed.h.max(24);
        // The stored image is the original full resolution but `w`/`h` are the
        // on-screen size, so resize to match before recreating (same corruption
        // guard as `add_pin`).
        let image = image::imageops::resize(
            &*closed.image,
            pin_w,
            pin_h,
            image::imageops::FilterType::Triangle,
        );
        match Pin::create(
            id,
            Arc::new(image),
            closed.x,
            closed.y,
            pin_w,
            pin_h,
            self.tr.tr,
            self.config.pin_opacity,
            self.config.pin_shadow,
            self.config.pin_shadow_color,
            self.config.pin_wheel_zoom,
            self.config.pin_zoom_at_cursor,
            self.config.pin_smooth_zoom,
            self.config.pin_middle_close,
            self.config.pin_gestures.encoded(),
            group,
            self.pin_tx.clone(),
        ) {
            Ok(mut pin) => {
                if self.config.pin_through {
                    pin.toggle_passthrough();
                }
                self.pins.push(pin);
                self.toast(self.tr.tr.pin_restored, false);
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.pin_create_failed), true),
        }
    }

    /// Collect a finished upload job and report it. On success the shareable
    /// link is copied to the clipboard; on failure an error toast is shown.
    fn poll_upload(&mut self) {
        let Some(rx) = self.upload_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(url)) => {
                if let Err(err) = crate::clipboard::copy_text(&url) {
                    self.toast(format!("{}: {err}", self.tr.tr.upload_failed), true);
                } else {
                    self.toast(format!("{}: {url}", self.tr.tr.uploaded), false);
                }
                self.upload_rx = None;
            }
            Ok(Err(err)) => {
                self.toast(format!("{}: {err}", self.tr.tr.upload_failed), true);
                self.upload_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.upload_rx = None;
            }
        }
    }

    /// Receive the result of a background GIF-edit job and surface it as a
    /// status line on the GIF editor page.
    fn poll_gif(&mut self) {
        let Some(rx) = self.gif_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.gif_editor.status = Some((
                    format!("{}: {}", self.tr.tr.gif_done, path.display()),
                    false,
                ));
                self.gif_rx = None;
            }
            Ok(Err(err)) => {
                self.gif_editor.status = Some((format!("{}: {err}", self.tr.tr.gif_failed), true));
                self.gif_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.gif_rx = None;
            }
        }
    }

    /// Record a voice note from the tray: capture the microphone for the
    /// configured duration into the save folder, off the UI thread.
    fn record_voice_note(&mut self) {
        if !crate::record::ffmpeg_available() {
            self.toast(self.tr.tr.voice_needs_ffmpeg, true);
            return;
        }
        if self.voice_rx.is_some() {
            return;
        }
        let dir = self.config.save_dir.clone();
        let _ = std::fs::create_dir_all(&dir);
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let out = dir.join(format!("voice-{stamp}.wav"));
        let seconds = self.config.voice_seconds.max(1);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let res = crate::voice::record(&out, Some(seconds))
                .map(|_| out.clone())
                .map_err(|e| e.to_string());
            let _ = tx.send(res);
        });
        self.voice_rx = Some(rx);
        self.toast(self.tr.tr.voice_recording, false);
    }

    /// Receive a finished voice-note recording and report it.
    fn poll_voice(&mut self) {
        let Some(rx) = self.voice_rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.toast(
                    format!("{}: {}", self.tr.tr.voice_done, path.display()),
                    false,
                );
                self.voice_rx = None;
            }
            Ok(Err(err)) => {
                self.toast(format!("{}: {err}", self.tr.tr.voice_failed), true);
                self.voice_rx = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.voice_rx = None;
            }
        }
    }

    /// The GIF editor page: pick a GIF, pick an operation, run it on a worker
    /// thread and report the result on the same page.
    fn gif_page(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ed = &mut self.gif_editor;
        ui.heading(t.gif_title);
        ui.label(t.gif_subtitle);
        ui.add_space(10.0);

        // Input GIF.
        ui.horizontal(|ui| {
            ui.label(t.gif_input);
            ui.text_edit_singleline(&mut ed.input);
            if ui.button(t.gif_choose).clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("GIF", &["gif"])
                    .pick_file()
                {
                    ed.input = path.to_string_lossy().to_string();
                    if ed.output.is_empty() {
                        let mut o = path.clone();
                        let stem = o
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        o.set_file_name(format!("{stem}-out"));
                        o.set_extension("gif");
                        ed.output = o.to_string_lossy().to_string();
                    }
                }
            }
        });

        ui.add_space(6.0);
        ui.label(t.gif_operation);
        ui.horizontal_wrapped(|ui| {
            let ops = [t.gif_op_trim, t.gif_op_crop, t.gif_op_text, t.gif_op_mp4];
            for (i, label) in ops.iter().enumerate() {
                if ui.selectable_label(ed.op as usize == i, *label).clicked() {
                    ed.op = i as u8;
                }
            }
        });

        ui.add_space(6.0);
        match ed.op {
            0 => {
                ui.horizontal(|ui| {
                    ui.label(t.gif_start_frame);
                    ui.add(egui::DragValue::new(&mut ed.start).range(0..=1_000_000));
                    ui.label(t.gif_end_frame);
                    ui.add(egui::DragValue::new(&mut ed.end).range(0..=1_000_000));
                });
            }
            1 => {
                ui.horizontal(|ui| {
                    ui.label(t.gif_x);
                    ui.add(egui::DragValue::new(&mut ed.x).range(0..=100_000));
                    ui.label(t.gif_y);
                    ui.add(egui::DragValue::new(&mut ed.y).range(0..=100_000));
                    ui.label(t.gif_w);
                    ui.add(egui::DragValue::new(&mut ed.w).range(0..=100_000));
                    ui.label(t.gif_h);
                    ui.add(egui::DragValue::new(&mut ed.h).range(0..=100_000));
                });
            }
            2 => {
                ui.horizontal(|ui| {
                    ui.label(t.gif_text);
                    ui.text_edit_singleline(&mut ed.text);
                });
                ui.horizontal(|ui| {
                    ui.label(t.gif_font_size);
                    ui.add(egui::DragValue::new(&mut ed.font_size).range(4..=200));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label(t.gif_text_pos);
                    let poses = [
                        t.gif_pos_tl,
                        t.gif_pos_tr,
                        t.gif_pos_bl,
                        t.gif_pos_br,
                        t.gif_pos_c,
                    ];
                    for (i, label) in poses.iter().enumerate() {
                        if ui.selectable_label(ed.pos as usize == i, *label).clicked() {
                            ed.pos = i as u8;
                        }
                    }
                });
            }
            _ => {
                ui.label(t.gif_needs_ffmpeg);
            }
        }

        if ed.op != 3 {
            ui.horizontal(|ui| {
                ui.label(t.gif_speed);
                ui.add(egui::Slider::new(&mut ed.speed, 1..=30));
            });
        }

        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(t.gif_output);
            ui.text_edit_singleline(&mut ed.output);
        });

        ui.add_space(10.0);
        let busy = self.gif_rx.is_some();
        let run_label = if busy { t.gif_running } else { t.gif_run };
        if ui
            .add_enabled(!busy, egui::Button::new(run_label))
            .clicked()
        {
            if ed.input.trim().is_empty() {
                ed.status = Some((t.gif_need_input.to_string(), true));
            } else {
                let input = PathBuf::from(ed.input.trim());
                let output = if ed.output.trim().is_empty() {
                    let mut o = input.clone();
                    let stem = o
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    o.set_file_name(format!("{stem}-out"));
                    o.set_extension(if ed.op == 3 { "mp4" } else { "gif" });
                    o
                } else {
                    let mut o = PathBuf::from(ed.output.trim());
                    if ed.op == 3 {
                        o.set_extension("mp4");
                    }
                    o
                };
                if let Some(parent) = output.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                let op = ed.op;
                let start = ed.start as usize;
                let end = if ed.end == 0 {
                    None
                } else {
                    Some(ed.end as usize)
                };
                let (x, y, w, h) = (ed.x, ed.y, ed.w, ed.h);
                let text = ed.text.clone();
                let font_size = ed.font_size;
                let pos = ed.pos;
                let speed = ed.speed;
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let res = (|| -> anyhow::Result<()> {
                        match op {
                            0 => crate::gifedit::trim(&input, &output, start, end, speed)?,
                            1 => crate::gifedit::crop(&input, &output, x, y, w, h, speed)?,
                            2 => {
                                let position = match pos {
                                    0 => crate::gifedit::TextPos::TopLeft,
                                    1 => crate::gifedit::TextPos::TopRight,
                                    2 => crate::gifedit::TextPos::BottomLeft,
                                    3 => crate::gifedit::TextPos::BottomRight,
                                    _ => crate::gifedit::TextPos::Center,
                                };
                                crate::gifedit::add_text(
                                    &input,
                                    &output,
                                    &crate::gifedit::TextOptions {
                                        text,
                                        font_size,
                                        color: [255, 255, 255],
                                        position,
                                    },
                                    speed,
                                )?;
                            }
                            _ => crate::gifedit::to_mp4(&input, &output)?,
                        }
                        Ok(())
                    })();
                    let _ = tx.send(res.map(|_| output).map_err(|e| e.to_string()));
                });
                self.gif_rx = Some(rx);
                ed.status = None;
            }
        }

        if let Some((msg, is_err)) = &ed.status {
            let color = if *is_err { p.danger } else { p.accent };
            ui.add_space(6.0);
            ui.label(egui::RichText::new(msg.clone()).color(color));
            if !*is_err && ui.button(t.gif_open).clicked() {
                let out = PathBuf::from(ed.output.trim());
                if !out.as_os_str().is_empty() {
                    crate::util::open_path(&out);
                }
            }
        }
    }

    // ---- pin groups ------------------------------------------------------

    /// A group id that still exists, falling back to the first one.
    ///
    /// Pins outlive the group they were created in (a group can be deleted
    /// while its pins are open), so every lookup has to degrade gracefully.
    fn resolve_group(&self, id: u32) -> u32 {
        if self.config.pin_groups.iter().any(|g| g.id == id) {
            id
        } else {
            self.config.pin_groups.first().map(|g| g.id).unwrap_or(1)
        }
    }

    /// Display name of a group, falling back to the translated default.
    fn group_name(&self, id: u32) -> String {
        let t = self.tr.tr;
        self.config
            .pin_groups
            .iter()
            .find(|g| g.id == id)
            .map(|g| {
                if g.name.trim().is_empty() {
                    t.pin_group_default.to_string()
                } else {
                    g.name.clone()
                }
            })
            .unwrap_or_else(|| t.pin_group_default.to_string())
    }

    /// How many pins a group currently holds.
    fn group_count(&self, id: u32) -> usize {
        self.pins.iter().filter(|pin| pin.group == id).count()
    }

    /// Add an empty group and make it the active one.
    fn create_group(&mut self) {
        let next = self
            .config
            .pin_groups
            .iter()
            .map(|g| g.id)
            .max()
            .unwrap_or(0)
            + 1;
        self.config.pin_groups.push(config::PinGroup {
            id: next,
            name: String::new(),
            hidden: false,
        });
        self.active_group = next;
        let _ = self.config.save();
    }

    /// Rename a group; an empty name falls back to the default label.
    fn rename_group(&mut self, id: u32, name: String) {
        if let Some(group) = self.config.pin_groups.iter_mut().find(|g| g.id == id) {
            group.name = name.trim().to_string();
            let _ = self.config.save();
        }
    }

    /// Delete a group. Its pins move to the first remaining group rather than
    /// closing: destroying windows the user did not point at is the kind of
    /// surprise a settings action should never spring.
    fn delete_group(&mut self, id: u32) {
        if self.config.pin_groups.len() <= 1 {
            self.toast(self.tr.tr.pin_group_last, true);
            return;
        }
        let fallback = self
            .config
            .pin_groups
            .iter()
            .find(|g| g.id != id)
            .map(|g| g.id)
            .unwrap_or(1);
        self.config.pin_groups.retain(|g| g.id != id);
        for pin in self.pins.iter_mut().filter(|pin| pin.group == id) {
            pin.group = fallback;
        }
        if self.active_group == id {
            self.active_group = fallback;
        }
        let _ = self.config.save();
    }

    /// Show or hide a whole group.
    fn set_group_hidden(&mut self, id: u32, hidden: bool) {
        if let Some(group) = self.config.pin_groups.iter_mut().find(|g| g.id == id) {
            group.hidden = hidden;
        }
        for pin in self.pins.iter_mut().filter(|pin| pin.group == id) {
            pin.set_visible(!hidden);
        }
        let _ = self.config.save();
    }

    /// Move one pin into another group.
    fn set_pin_group(&mut self, pin_id: u64, group: u32) {
        let group = self.resolve_group(group);
        if let Some(pin) = self.pins.iter_mut().find(|pin| pin.id == pin_id) {
            pin.group = group;
            // A pin moved into a hidden group must disappear with it.
            let hidden = self
                .config
                .pin_groups
                .iter()
                .find(|g| g.id == group)
                .map(|g| g.hidden)
                .unwrap_or(false);
            pin.set_visible(!hidden);
        }
    }

    fn add_pin(&mut self, image: RgbaImage) -> u64 {
        let id = self.next_pin_id;
        self.next_pin_id += 1;

        let monitor = capture::monitor_at_cursor()
            .or_else(|_| capture::monitors().map(|m| m[0].clone()))
            .ok();
        let (pos, size) = if let Some(monitor) = monitor {
            let rect = monitor.rect;
            let max_w = rect.w as f32 * 0.8;
            let max_h = rect.h as f32 * 0.85;
            let scale = (max_w / image.width() as f32)
                .min(max_h / image.height() as f32)
                .min(1.0);
            let size = egui::vec2(image.width() as f32, image.height() as f32) * scale;
            let offset = (self.pins.len() as f32 * 16.0).min(120.0);
            let x = rect.x as f32 + (rect.w as f32 - size.x) * 0.5 + offset;
            let y = rect.y as f32 + (rect.h as f32 - size.y) * 0.5 + offset;
            (egui::pos2(x, y), size)
        } else {
            (
                egui::pos2(200.0, 200.0),
                egui::vec2(image.width() as f32, image.height() as f32)
                    .min(egui::vec2(640.0, 480.0)),
            )
        };

        let pin_w = size.x.round().max(24.0) as u32;
        let pin_h = size.y.round().max(24.0) as u32;
        // `Pin::create` builds its BGRA buffer from the exact pixel dimensions
        // passed in, and that buffer's row stride must match the `width`/`height`
        // the window blits it with. The fit-to-monitor `size` here is the
        // on-screen size, so resize the bitmap to it — otherwise a full-res image
        // paired with a scaled width/height desyncs the stride and yields the
        // white/horizontal-noise corruption (triggered whenever scale < 1).
        let image =
            image::imageops::resize(&image, pin_w, pin_h, image::imageops::FilterType::Triangle);
        match Pin::create(
            id,
            Arc::new(image),
            pos.x.round() as i32,
            pos.y.round() as i32,
            pin_w,
            pin_h,
            self.tr.tr,
            self.config.pin_opacity,
            self.config.pin_shadow,
            self.config.pin_shadow_color,
            self.config.pin_wheel_zoom,
            self.config.pin_zoom_at_cursor,
            self.config.pin_smooth_zoom,
            self.config.pin_middle_close,
            self.config.pin_gestures.encoded(),
            self.active_group,
            self.pin_tx.clone(),
        ) {
            Ok(mut pin) => {
                if self.config.pin_through {
                    pin.toggle_passthrough();
                }
                self.pins.push(pin)
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.pin_create_failed), true),
        }
        id
    }

    fn add_pin_at(&mut self, image: RgbaImage, pos: egui::Pos2, size: egui::Vec2) -> u64 {
        // "Reuse the capture position" off means the pin is centred on the
        // monitor instead of sitting exactly where the region was.
        if !self.config.pin_keep_position {
            return self.add_pin(image);
        }
        let id = self.next_pin_id;
        self.next_pin_id += 1;
        // The settings page owns the cap; keep it inside the monitor as well.
        let cap = self.config.pin_max_size.max(320) as f32;
        let max = egui::vec2(cap.min(1200.0), cap.min(900.0));
        let shrink = (max.x / size.x).min(max.y / size.y).min(1.0);
        let size = size * shrink;
        let pin_w = size.x.round().max(24.0) as u32;
        let pin_h = size.y.round().max(24.0) as u32;
        // Resize the bitmap to the on-screen size so the BGRA stride matches the
        // width/height the pin window blits with (see `add_pin` for the full
        // explanation of why a mismatched full-res image corrupts the pin).
        let image =
            image::imageops::resize(&image, pin_w, pin_h, image::imageops::FilterType::Triangle);
        match Pin::create(
            id,
            Arc::new(image),
            pos.x.round() as i32,
            pos.y.round() as i32,
            pin_w,
            pin_h,
            self.tr.tr,
            self.config.pin_opacity,
            self.config.pin_shadow,
            self.config.pin_shadow_color,
            self.config.pin_wheel_zoom,
            self.config.pin_zoom_at_cursor,
            self.config.pin_smooth_zoom,
            self.config.pin_middle_close,
            self.config.pin_gestures.encoded(),
            self.active_group,
            self.pin_tx.clone(),
        ) {
            Ok(mut pin) => {
                if self.config.pin_through {
                    pin.toggle_passthrough();
                }
                self.pins.push(pin)
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.pin_create_failed), true),
        }
        id
    }

    fn poll_pin_events(&mut self) {
        while let Ok(event) = self.pin_rx.try_recv() {
            match event {
                PinEvent::Close(id) => {
                    // Take the entry out first: `remember_closed_pin` needs
                    // `&mut self`, and the pin is gone either way.
                    if let Some(index) = self.pins.iter().position(|pin| pin.id == id) {
                        let closed = ClosedPin {
                            image: self.pins[index].image.clone(),
                            x: self.pins[index].x,
                            y: self.pins[index].y,
                            w: self.pins[index].width,
                            h: self.pins[index].height,
                            group: self.pins[index].group,
                        };
                        self.push_closed_pin(closed);
                        self.pins.remove(index);
                    }
                    self.pins.retain(|pin| pin.id != id);
                    self.textures.remove(&format!("pin-{id}"));
                }
                PinEvent::CopyText(_id) => {
                    // A pin holds an image, so there is no text to copy; the
                    // gesture is offered for symmetry with the other bindings
                    // and simply reports that.
                    self.toast(self.tr.tr.pin_no_text, true);
                }
                PinEvent::Copy(id) => {
                    if let Some(pin) = self.pins.iter().find(|p| p.id == id) {
                        match crate::clipboard::copy_image(&pin.image) {
                            Ok(()) => self.toast(self.tr.tr.copied, false),
                            Err(err) => {
                                tracing::warn!(%err, "pin image copy to clipboard failed");
                                self.toast(format!("{}: {err}", self.tr.tr.copy_failed), true);
                            }
                        }
                    }
                }
                PinEvent::SaveAs(id) => {
                    if let Some(pin) = self.pins.iter().find(|p| p.id == id) {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png"])
                            .set_file_name(format!("pinsight-pin-{id}.png"))
                            .save_file()
                        {
                            match util::save_png_to(&pin.image, &path) {
                                Ok(()) => self.toast(self.tr.tr.pin_saved, false),
                                Err(err) => {
                                    self.toast(format!("{}: {err}", self.tr.tr.pin_saved), true)
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// Drive every live pin window. On Windows the pins are real native
    /// windows with their own message loop and nothing to do here; on every
    /// other platform the pin is an egui deferred viewport fed from this pass.
    #[cfg(not(windows))]
    fn show_pins(&mut self, ctx: &egui::Context) {
        for pin in self.pins.iter_mut() {
            pin.show_viewport(ctx);
        }
    }

    #[cfg(windows)]
    fn show_pins(&mut self, _ctx: &egui::Context) {}

    /// Make the wallpaper texture match the config: load it the first time,
    /// reload it when the path or the blur radius changed, drop it when the
    /// user clears the setting.
    fn sync_background(&mut self, ctx: &egui::Context) {
        let path = self.config.bg_image.clone();
        let blur = self.config.bg_blur;

        if path.is_none() {
            self.background = None;
            self.textures.remove("pinsight-bg");
            return;
        }

        match &mut self.background {
            Some(bg) => {
                if bg.refresh(path.as_deref(), blur) {
                    self.upload_background(ctx);
                }
            }
            None => {
                let Some(path) = path else { return };
                match Background::load(&path, blur) {
                    Some(bg) => {
                        self.background = Some(bg);
                        self.upload_background(ctx);
                    }
                    None => {
                        // A dead path must not break startup, so the setting is
                        // simply forgotten.
                        self.config.bg_image = None;
                        let _ = self.config.save();
                        self.toast(self.tr.tr.bg_load_failed, true);
                    }
                }
            }
        }
    }

    fn upload_background(&mut self, ctx: &egui::Context) {
        let Some(bg) = &self.background else { return };
        let texture = ctx.load_texture(
            "pinsight-bg",
            bg.color_image(),
            egui::TextureOptions::LINEAR,
        );
        self.textures.insert("pinsight-bg".to_string(), texture);
    }

    /// Paint the wallpaper and its darkening veil behind everything else.
    fn paint_background(&self, ui: &egui::Ui, p: &Palette) {
        let Some(bg) = &self.background else { return };
        let Some(texture) = self.textures.get("pinsight-bg") else {
            return;
        };

        let area = ui.max_rect();
        let dest = background::dest_rect(area, bg.size(), self.config.bg_fit);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        ui.painter()
            .image(texture.id(), dest, uv, egui::Color32::WHITE);

        let dim = self.config.bg_dim.clamp(0.0, 0.95);
        if dim > 0.002 {
            let base = if p.dark { egui::Color32::BLACK } else { p.bg };
            ui.painter()
                .rect_filled(area, 0.0, theme::with_alpha(base, (dim * 255.0) as u8));
        }
    }

    /// True when a wallpaper is loaded and should show through the layout.
    fn has_background(&self) -> bool {
        self.background.is_some() && self.textures.contains_key("pinsight-bg")
    }

    // ---- tray -------------------------------------------------------------

    /// Drop the live tray so the next `install_tray` builds a fresh one. A
    /// no-op on platforms without a tray (the field only exists on Windows).
    #[cfg(windows)]
    fn tray_reset(&mut self) {
        self.tray = None;
    }

    #[cfg(not(windows))]
    fn tray_reset(&mut self) {}

    /// Build the tray menu from the current language and install the icon.
    /// On anything other than Windows this is a no-op: the hotkeys and the
    /// main window still work, which is what matters most.
    fn install_tray(&mut self, ctx: &egui::Context) {
        let (tx, rx) = std::sync::mpsc::channel();
        self.tray_rx = Some(rx);

        #[cfg(windows)]
        {
            let items = self.tray_items();
            let icon = self.tray_icon_pixels();
            self.tray =
                crate::tray::Tray::install_with(tx, items, self.tr.tr.tray_tip.to_string(), icon);
            // Let the tray wake the UI: with `ControlFlow::Wait` an idle app
            // never repaints, so queued commands would otherwise be ignored.
            let wake_ctx = ctx.clone();
            crate::tray::set_wake(std::sync::Arc::new(move || wake_ctx.request_repaint()));
            if self.tray.is_none() {
                self.tray_rx = None;
            }
        }
        #[cfg(not(windows))]
        {
            let _ = tx;
        }
    }

    /// Rebuild the tray rows when the state behind their labels changed.
    ///
    /// The popup is assembled from the stored copy each time it opens, so a
    /// stale copy would show outdated "show/hide" wording or a Pins submenu
    /// that is missing a freshly created pin.
    #[cfg(windows)]
    fn sync_tray_menu(&mut self) {
        let sig = (
            self.pins.len(),
            self.config.pin_hidden,
            self.config.hotkeys_enabled,
            self.config.enhanced_tray,
            self.config
                .pin_groups
                .iter()
                .map(|g| (g.id, g.name.clone(), g.hidden))
                .collect(),
        );
        if sig == self.tray_sig {
            return;
        }
        self.tray_sig = sig;
        let items = self.tray_items();
        if let Some(tray) = self.tray.as_ref() {
            tray.set_items(items);
        }
    }

    /// The rows of the tray menu, mirroring Snipaste's layout:
    /// capture actions, pin actions, history, then the app-level entries.
    #[cfg(windows)]
    /// Artwork for the tray icon, or `None` to keep the embedded colour icon.
    ///
    /// The "white" style keeps only the glyph of the app icon: the blue plate
    /// has a luminance of about 122 while the crop marks are pure white, so a
    /// threshold cleanly separates them and the result reads as a normal
    /// monochrome tray icon instead of a solid white square.
    #[cfg(windows)]
    fn tray_icon_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        use config::TrayIconStyle;
        const SIZE: u32 = 32;

        fn glyph(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
            let img = image::load_from_memory(bytes).ok()?.to_rgba8();
            let scaled =
                image::imageops::resize(&img, SIZE, SIZE, image::imageops::FilterType::Lanczos3);
            let mut out = Vec::with_capacity((SIZE * SIZE * 4) as usize);
            for px in scaled.pixels() {
                let lum = 0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32;
                // Soft edge between the plate and the glyph so the marks keep
                // their anti-aliasing.
                let t = ((lum - 150.0) / 70.0).clamp(0.0, 1.0);
                let alpha = (px[3] as f32 * t).round() as u8;
                out.extend_from_slice(&[255, 255, 255, alpha]);
            }
            Some((SIZE, SIZE, out))
        }

        fn custom(path: &std::path::Path) -> Option<(u32, u32, Vec<u8>)> {
            let bytes = std::fs::read(path).ok()?;
            let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
            let scaled =
                image::imageops::resize(&img, SIZE, SIZE, image::imageops::FilterType::Lanczos3);
            Some((SIZE, SIZE, scaled.into_raw()))
        }

        match self.config.tray_icon_style {
            TrayIconStyle::Color => None,
            TrayIconStyle::White => glyph(include_bytes!("../assets/icon.png")),
            TrayIconStyle::Custom => self
                .config
                .tray_icon_path
                .as_deref()
                .and_then(custom)
                .or_else(|| glyph(include_bytes!("../assets/icon.png"))),
            TrayIconStyle::Auto => {
                if taskbar_is_dark() {
                    glyph(include_bytes!("../assets/icon.png"))
                } else {
                    None
                }
            }
        }
    }

    #[cfg(not(windows))]
    fn tray_icon_pixels(&self) -> Option<(u32, u32, Vec<u8>)> {
        None
    }

    fn tray_items(&self) -> Vec<crate::tray::TrayItem> {
        use crate::tray::TrayItem;
        let t = self.tr.tr;
        // Showing the shortcut next to each action is what makes the tray
        // menu self-teaching, the way Snipaste and PixPin do it.
        let hk = &self.config.hotkeys;
        let row = |label: &str, shortcut: &str| format!("{label}\t{shortcut}");

        // The "Pins" submenu lists the open pins grouped, so a whole group can
        // be hidden and a specific pin brought to the front without hunting
        // for it on screen.
        let mut pin_rows: Vec<TrayItem> = Vec::new();
        if self.pins.is_empty() {
            pin_rows.push(TrayItem::Command {
                id: 900,
                label: t.tray_no_pins.to_string(),
                command: TrayCommand::ShowWindow,
            });
        } else {
            let mut next_id = 900u32;
            let groups = self.config.pin_groups.clone();
            for group in &groups {
                let pins: Vec<&Pin> = self
                    .pins
                    .iter()
                    .filter(|pin| pin.group == group.id)
                    .collect();
                let mut rows: Vec<TrayItem> = Vec::new();
                rows.push(TrayItem::Command {
                    id: next_id,
                    label: if group.hidden {
                        t.pin_group_show.to_string()
                    } else {
                        t.pin_group_hide.to_string()
                    },
                    command: TrayCommand::ToggleGroup(group.id),
                });
                next_id += 1;
                if pins.is_empty() {
                    rows.push(TrayItem::Command {
                        id: next_id,
                        label: t.tray_no_pins.to_string(),
                        command: TrayCommand::ShowWindow,
                    });
                    next_id += 1;
                } else {
                    for pin in pins {
                        rows.push(TrayItem::Command {
                            id: next_id,
                            label: format!("{} {}", t.pins_title, pin.id),
                            command: TrayCommand::FocusPin(pin.id),
                        });
                        next_id += 1;
                    }
                }
                let label = if group.hidden {
                    format!("{} ({})", self.group_name(group.id), t.pin_group_hide)
                } else {
                    self.group_name(group.id)
                };
                pin_rows.push(TrayItem::Submenu {
                    id: next_id,
                    label,
                    items: rows,
                });
                next_id += 1;
            }
        }

        let items = vec![
            TrayItem::Command {
                id: 1,
                label: row(t.tray_capture_area, &hk.capture_area),
                command: TrayCommand::CaptureArea,
            },
            TrayItem::Command {
                id: 2,
                label: row(t.tray_capture_full, &hk.capture_full),
                command: TrayCommand::CaptureFull,
            },
            TrayItem::Command {
                id: 3,
                label: row(t.tray_pin_clipboard, &hk.pin_clipboard),
                command: TrayCommand::PinClipboard,
            },
            TrayItem::Command {
                id: 4,
                label: row(t.tray_long_capture, &hk.long_capture),
                command: TrayCommand::LongCapture,
            },
            TrayItem::Command {
                id: 5,
                label: row(t.tray_record, &hk.record),
                command: TrayCommand::Record,
            },
            TrayItem::Command {
                id: 18,
                label: t.tray_record_voice.to_string(),
                command: TrayCommand::RecordVoice,
            },
            TrayItem::Command {
                id: 19,
                label: t.tray_edit_gif.to_string(),
                command: TrayCommand::EditGif,
            },
            TrayItem::Command {
                id: 6,
                label: row(t.tray_save_last, &hk.save_last),
                command: TrayCommand::SaveLast,
            },
            TrayItem::Separator,
            TrayItem::Command {
                id: 7,
                label: if self.config.pin_hidden {
                    t.tray_toggle_pins_show.to_string()
                } else {
                    t.tray_toggle_pins_hide.to_string()
                },
                command: TrayCommand::TogglePins,
            },
            TrayItem::Command {
                id: 17,
                label: t.pin_restore_action.to_string(),
                command: TrayCommand::RestorePin,
            },
            TrayItem::Command {
                id: 8,
                label: t.tray_clear_history.to_string(),
                command: TrayCommand::ClearHistory,
            },
            TrayItem::Separator,
            TrayItem::Command {
                id: 9,
                label: if self.config.hotkeys_enabled {
                    t.tray_disable_hk.to_string()
                } else {
                    t.tray_enable_hk.to_string()
                },
                command: TrayCommand::ToggleHotkeys,
            },
            TrayItem::Separator,
            TrayItem::Submenu {
                id: 10,
                label: t.tray_pins_submenu.to_string(),
                items: pin_rows,
            },
            TrayItem::Command {
                id: 11,
                label: t.tray_prefs.to_string(),
                command: TrayCommand::ShowSettings,
            },
            TrayItem::Command {
                id: 12,
                label: t.tray_help.to_string(),
                command: TrayCommand::Help,
            },
            TrayItem::Command {
                id: 13,
                label: t.tray_check_update.to_string(),
                command: TrayCommand::CheckUpdate,
            },
            TrayItem::Separator,
            TrayItem::Command {
                id: 14,
                label: t.tray_show.to_string(),
                command: TrayCommand::ShowWindow,
            },
            TrayItem::Command {
                id: 15,
                label: t.tray_restart.to_string(),
                command: TrayCommand::Restart,
            },
            TrayItem::Command {
                id: 16,
                label: t.tray_quit.to_string(),
                command: TrayCommand::Quit,
            },
        ];

        // "Enhanced tray menu" off keeps only the essentials, the way
        // Snipaste's compact menu looks.
        if !self.config.enhanced_tray {
            let keep: [u32; 7] = [1, 2, 3, 11, 12, 14, 16];
            let mut basic: Vec<TrayItem> = Vec::new();
            for item in items {
                let wanted = match &item {
                    TrayItem::Command { id, .. } => keep.contains(id),
                    TrayItem::Separator => true,
                    TrayItem::Submenu { .. } => false,
                };
                if !wanted {
                    continue;
                }
                // Collapse separators left adjacent by the filtering.
                if matches!(item, TrayItem::Separator)
                    && matches!(basic.last(), Some(TrayItem::Separator) | None)
                {
                    continue;
                }
                basic.push(item);
            }
            return basic;
        }
        items
    }

    /// Drain tray menu selections and run them like the matching hotkey.
    fn poll_tray(&mut self, ctx: &egui::Context) {
        let Some(rx) = self.tray_rx.as_ref() else {
            return;
        };
        let mut commands = Vec::new();
        while let Ok(command) = rx.try_recv() {
            tracing::debug!(?command, "tray command received");
            commands.push(command);
        }
        if commands.is_empty() {
            return;
        }

        for command in commands {
            match command {
                TrayCommand::CaptureArea => self.begin_area(Intent::Copy, ctx),
                TrayCommand::CaptureFull => self.begin_full_copy(ctx),
                TrayCommand::PinClipboard => self.pin_clipboard(),
                TrayCommand::LongCapture => {
                    if let Some(job) = &self.long_job {
                        job.stop();
                        self.toast(self.tr.tr.long_cancelled, false);
                    } else {
                        self.begin_area(Intent::LongShot, ctx);
                    }
                }
                TrayCommand::Record => {
                    if self.record_job.is_some() {
                        if let Some(job) = &self.record_job {
                            job.stop();
                            self.toast(self.tr.tr.stopping_recording, false);
                        }
                    } else {
                        self.begin_area(Intent::Record, ctx);
                    }
                }
                TrayCommand::SaveLast => self.save_last(),
                TrayCommand::TogglePins => self.toggle_all_pins(),
                TrayCommand::FocusPin(id) => {
                    if let Some(pin) = self.pins.iter().find(|p| p.id == id) {
                        pin.focus();
                    }
                }
                TrayCommand::RestorePin => self.restore_closed_pin(),
                TrayCommand::ToggleGroup(id) => {
                    let hidden = self
                        .config
                        .pin_groups
                        .iter()
                        .find(|g| g.id == id)
                        .map(|g| g.hidden)
                        .unwrap_or(false);
                    self.set_group_hidden(id, !hidden);
                }
                TrayCommand::ClearHistory => {
                    if self.history.is_empty() {
                        self.toast(self.tr.tr.tray_no_history_toast, true);
                    } else {
                        self.history.clear();
                        self.textures.retain(|key, _| !key.starts_with("hist-"));
                        self.toast(self.tr.tr.tray_history_cleared_toast, false);
                    }
                }
                TrayCommand::ToggleHotkeys => {
                    self.config.hotkeys_enabled = !self.config.hotkeys_enabled;
                    let _ = self.config.save();
                    self.apply_hotkey_state();
                    if self.config.hotkeys_enabled {
                        self.toast(self.tr.tr.tray_hotkeys_on_toast, false);
                    } else {
                        self.toast(self.tr.tr.tray_hotkeys_off_toast, false);
                    }
                }
                TrayCommand::CheckUpdate => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.page = Page::Settings;
                    self.settings_tab = SettingsTab::Updates;
                    ctx.open_url(egui::OpenUrl::new_tab(
                        "https://github.com/xiu-k/pinsight/releases",
                    ));
                }
                TrayCommand::Restart => {
                    self.toast(self.tr.tr.tray_restarting_toast, false);
                    self.restart_app(ctx);
                }
                TrayCommand::ShowSettings => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.page = Page::Settings;
                }
                TrayCommand::Help => {
                    ctx.open_url(egui::OpenUrl::new_tab(
                        "https://github.com/xiu-k/pinsight#readme",
                    ));
                }
                TrayCommand::ShowWindow => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                TrayCommand::EditGif => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                    self.page = Page::Gif;
                }
                TrayCommand::RecordVoice => {
                    self.record_voice_note();
                }
                TrayCommand::Quit => {
                    // Mark the intent first: the close we are about to ask for
                    // comes back as a `CloseRequested` on the next frame, and
                    // `handle_close_request` would otherwise hide the window
                    // instead of letting the process exit.
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            ctx.request_repaint();
        }
    }

    /// Closing the window hides it instead of quitting when the user asked for
    /// tray behaviour, so the global hotkeys stay live.
    fn handle_close_request(&mut self, ctx: &egui::Context) {
        let wants_close = ctx.input(|i| i.viewport().close_requested());
        if !wants_close {
            return;
        }
        // "Quit" from the tray means quit, even though closing the window any
        // other way would only hide it.
        if self.quitting || !self.config.close_to_tray {
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        // Hidden to the tray: nothing of the UI is on screen, so give the
        // working set back instead of holding ~90 MB hostage.
        util::trim_working_set();
    }

    fn show_overlay(&mut self, ctx: &egui::Context) {
        let Some(mut session) = self.capture.take() else {
            return;
        };

        let placement = session.selection.map(|sel| {
            (
                egui::pos2(
                    session.monitor.rect.x as f32 + sel.x,
                    session.monitor.rect.y as f32 + sel.y,
                ),
                egui::vec2(sel.w, sel.h),
            )
        });

        let viewport_id = egui::ViewportId::from_hash_of(("pinsight-overlay", session.monitor.id));
        let builder = session.viewport_builder();
        let mut outcome = None;
        let mut embedded = false;
        ctx.show_viewport_immediate(viewport_id, builder, |ui, class| {
            if matches!(class, egui::ViewportClass::EmbeddedWindow) {
                embedded = true;
                ui.label("PinSight is running in embedded-viewport mode.");
            }
            outcome = session.show(ui);
        });

        match outcome {
            Some(outcome) => self.finish_overlay(outcome, placement, ctx),
            None => {
                self.capture = Some(session);
                if embedded {
                    self.toast(self.tr.tr.embed_unsupported, true);
                    self.capture = None;
                }
            }
        }
    }

    fn finish_overlay(
        &mut self,
        outcome: OverlayOutcome,
        placement: Option<(egui::Pos2, egui::Vec2)>,
        ctx: &egui::Context,
    ) {
        match outcome {
            OverlayOutcome::Cancel => {}
            OverlayOutcome::Color(rgb) => {
                if let Some(picker) = self.picker.as_mut() {
                    picker.set_rgb(rgb);
                }
            }
            OverlayOutcome::Image { action, image } => {
                // Whatever the user did with it, this is now the latest capture.
                self.last_capture_at = Some(Instant::now());
                let arc = Arc::new(image.clone());
                self.last_image = Some(arc.clone());
                self.push_history(&arc);
                match action {
                    OverlayAction::Copy => match copy_to_clipboard(&self.config, &image) {
                        Ok(()) => {
                            if self.config.clipboard_mode != config::ClipboardMode::Disabled {
                                self.toast(self.tr.tr.copied, false)
                            }
                        }
                        Err(err) => {
                            self.toast(format!("{}: {err}", self.tr.tr.clipboard_error), true)
                        }
                    },
                    OverlayAction::Pin => {
                        let (pos, size) = placement.unwrap_or_else(|| {
                            (
                                egui::pos2(240.0, 240.0),
                                egui::vec2(image.width() as f32, image.height() as f32),
                            )
                        });
                        self.add_pin_at(image, pos, size);
                        self.toast(self.tr.tr.pinned, false);
                    }
                    OverlayAction::Save => {
                        match util::save_image(&image, &self.config.save_dir, &self.save_options())
                        {
                            Ok(path) => {
                                self.last_saved = Some(path.clone());
                                if self.config.capture_sound_save {
                                    util::play_notification(self.config.sound_file.as_deref());
                                }
                                self.toast(
                                    format!("{}: {}", self.tr.tr.saved, path.display()),
                                    false,
                                );
                            }
                            Err(err) => {
                                self.toast(format!("{}: {err}", self.tr.tr.save_failed), true)
                            }
                        }
                    }
                    OverlayAction::Upload => {
                        let mut upload_cfg = self.config.upload.clone();
                        // Local backend with no folder configured: fall back to
                        // `<save_dir>/uploaded` so Upload works with zero setup.
                        if upload_cfg.backend == crate::config::UploadBackend::Local
                            && upload_cfg.local.folder.as_os_str().is_empty()
                        {
                            upload_cfg.local.folder = self.config.save_dir.join("uploaded");
                        }
                        let (tx, rx) = std::sync::mpsc::channel();
                        std::thread::spawn(move || {
                            let result = crate::upload::upload_image(&image, &upload_cfg)
                                .map_err(|e| e.to_string());
                            let _ = tx.send(result);
                        });
                        self.upload_rx = Some(rx);
                        self.toast(self.tr.tr.uploading, false);
                    }
                    OverlayAction::Start | OverlayAction::StartManual | OverlayAction::Cancel => {}
                }
            }
            OverlayOutcome::StartLongShot {
                monitor_id,
                region,
                manual,
            } => {
                if self.long_job.is_some() {
                    self.toast(self.tr.tr.capture_in_progress, true);
                    return;
                }
                self.warn_if_hdr_monitor(monitor_id);
                self.park_root(ctx);
                self.long_job = Some(longshot::start(
                    monitor_id,
                    region,
                    longshot::LongShotOptions {
                        max_height: self.config.long_capture_max_height,
                        // Auto mode: the configured number of notches per step
                        // (a single notch barely moves the page on some
                        // machines; four reliably scrolls, and when it
                        // overshoots a frame the stitcher appends the whole
                        // frame so nothing is lost). Manual mode passes 0:
                        // PinSight does not scroll — the user rolls the wheel
                        // and decides how long the capture is, which is the
                        // reliable fallback for windows that ignore the posted
                        // wheel.
                        scroll_ticks: if manual {
                            0
                        } else {
                            self.config.long_scroll_ticks.clamp(1, 24)
                        },
                        scroll_delay: Duration::from_millis(
                            self.config.long_scroll_delay_ms.clamp(80, 3000) as u64,
                        ),
                        diff_threshold: self.config.long_diff_threshold.clamp(0.1, 32.0),
                        idle_limit: self.config.long_idle_limit.clamp(1, 120),
                        manual_idle_limit: self.config.long_manual_idle_limit.clamp(1, 600),
                        max_frames: self.config.long_max_frames.clamp(10, 5000),
                    },
                ));
                self.toast(
                    if manual {
                        self.tr.tr.long_manual_started
                    } else {
                        self.tr.tr.long_started
                    },
                    false,
                );
                ctx.request_repaint_after(Duration::from_millis(200));
            }
            OverlayOutcome::StartRecording { monitor_id, region } => {
                if self.record_job.is_some() {
                    self.toast(self.tr.tr.already_recording, true);
                    return;
                }
                self.warn_if_hdr_monitor(monitor_id);
                self.park_root(ctx);
                let job = record::start(
                    monitor_id,
                    region,
                    self.config.gif_fps,
                    &self.config.save_dir,
                    &self.save_options(),
                    self.config.record_max_seconds,
                    self.config.record_audio,
                    self.config.audio_device.clone(),
                );
                let path = job.path.clone();
                self.record_job = Some(job);
                self.toast(
                    format!("{}: {}", self.tr.tr.act_record_title, path.display()),
                    false,
                );
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }
    }

    // ---- settings helpers -------------------------------------------------

    /// Warn once when a capture lands on an HDR display — the washed-out
    /// frames are a pipeline behaviour, not a bug the user caused.
    fn warn_if_hdr_monitor(&mut self, monitor_id: u32) {
        if let Ok(info) = capture::find_monitor(monitor_id) {
            if crate::hdr::is_hdr(&info.rect) {
                self.toast(self.tr.tr.hdr_toast, false);
            }
        }
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        theme::apply(
            ctx,
            self.config.theme,
            self.config.accent,
            self.config.accent_custom,
        );
        let _ = self.config.save();
        ctx.request_repaint();
    }

    fn apply_fonts(&mut self, ctx: &egui::Context) {
        self.font_report = fonts::apply(
            ctx,
            self.config.lang,
            self.config.font,
            self.config.font_path.as_deref(),
            self.config.font_family.as_deref(),
            self.config.font_size_pt,
        );
        if self.font_report.warnings.is_empty() {
            self.toast(self.tr.tr.font_loaded, false);
        } else {
            self.toast(self.tr.tr.font_missing, true);
        }
        ctx.request_repaint();
    }

    /// Register or release the global hotkeys, according to
    /// [`Config::hotkeys_enabled`].
    fn apply_hotkey_state(&mut self) {
        if self.config.hotkeys_enabled {
            self.hotkeys.rebind(&self.config.hotkeys);
        } else {
            self.hotkeys.unbind_all();
        }
    }

    /// Show or hide every open pin at once.
    fn toggle_all_pins(&mut self) {
        if self.pins.is_empty() {
            return;
        }
        let hidden = !self.config.pin_hidden;
        self.config.pin_hidden = hidden;
        let _ = self.config.save();
        for pin in self.pins.iter_mut() {
            pin.set_visible(!hidden);
        }
        if hidden {
            self.toast(self.tr.tr.tray_pins_hidden_toast, false);
        } else {
            self.toast(self.tr.tr.tray_pins_shown_toast, false);
        }
    }

    /// Relaunch the process.
    ///
    /// The current instance exits and a fresh one starts, so an updated
    /// binary takes effect without the user hunting for the exe.
    fn restart_app(&mut self, ctx: &egui::Context) {
        if let Ok(exe) = std::env::current_exe() {
            let _ = std::process::Command::new(exe)
                .args(std::env::args().skip(1))
                .spawn();
        }
        // Same path as the tray's "Quit": mark the intent, then ask the
        // window to close so the process exits instead of hiding.
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Relaunch through the UAC prompt so the new process runs elevated.
    ///
    /// `ShellExecuteW` with the `runas` verb is the only way to ask for
    /// elevation from a running process: `CreateProcess` cannot raise the
    /// integrity level, and a scheduled-task trick would leave a stray entry.
    #[cfg(windows)]
    fn restart_elevated(&mut self, ctx: &egui::Context) {
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        use windows::core::HSTRING;

        let Ok(exe) = std::env::current_exe() else {
            self.toast(self.tr.tr.gen_admin_failed, true);
            return;
        };
        let verb = HSTRING::from("runas");
        let file = HSTRING::from(exe.as_os_str());
        let result = unsafe { ShellExecuteW(None, &verb, &file, None, None, SW_SHOWNORMAL) };
        // ShellExecuteW returns a value > 32 on success (an HINSTANCE cast).
        if result.0 as isize <= 32 {
            self.toast(self.tr.tr.gen_admin_failed, true);
            return;
        }
        self.quitting = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    #[cfg(not(windows))]
    fn restart_elevated(&mut self, _ctx: &egui::Context) {
        self.toast(self.tr.tr.gen_admin_failed, true);
    }

    fn set_language(&mut self, lang: Lang, ctx: &egui::Context) {
        self.config.lang = lang;
        self.tr.set(lang);
        // "Auto" fonts follow the language, so refresh them.
        if self.config.font == FontKind::Auto {
            self.font_report = fonts::apply(
                ctx,
                lang,
                self.config.font,
                self.config.font_path.as_deref(),
                self.config.font_family.as_deref(),
                self.config.font_size_pt,
            );
        }
        let _ = self.config.save();
        ctx.request_repaint();
        // The tray labels are translated, so rebuild them on language change.
        #[cfg(windows)]
        if let Some(tray) = self.tray.as_ref() {
            tray.set_items(self.tray_items());
        }
    }

    // ---- view -------------------------------------------------------------

    fn nav_rail(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(38.0, 38.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 12.0, p.accent);
            ui::paint_icon(ui.painter(), rect.shrink(9.0), Icon::Crop, p.on_accent, 2.0);
            ui.add_space(16.0);
            if ui::nav_button(ui, p, Icon::Crop, t.nav_capture, self.page == Page::Capture)
                .clicked()
            {
                self.page = Page::Capture;
            }
            ui.add_space(4.0);
            if ui::nav_button(
                ui,
                p,
                Icon::Image,
                t.nav_history,
                self.page == Page::History,
            )
            .clicked()
            {
                self.page = Page::History;
            }
            ui.add_space(4.0);
            if ui::nav_button(ui, p, Icon::Layers, t.nav_pins, self.page == Page::Pins).clicked() {
                self.page = Page::Pins;
            }
            ui.add_space(4.0);
            if ui::nav_button(ui, p, Icon::Film, t.nav_gif, self.page == Page::Gif).clicked() {
                self.page = Page::Gif;
            }
            ui.add_space(4.0);
            if ui::nav_button(
                ui,
                p,
                Icon::Settings,
                t.nav_settings,
                self.page == Page::Settings,
            )
            .clicked()
            {
                self.page = Page::Settings;
            }
        });

        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(10.0)
                    .color(p.text_weak),
            );
            if self.mcp_handle.is_some() {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(20.0, 12.0), egui::Sense::hover());
                ui.painter().circle_filled(rect.center(), 3.5, p.success);
            }
        });
    }

    fn page_header(&mut self, ui: &mut egui::Ui, p: &Palette, title: &str, subtitle: &str) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(title).size(22.0).strong().color(p.text));
                ui.add_space(1.0);
                ui.label(egui::RichText::new(subtitle).size(12.0).color(p.text_weak));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (text, color) = if self.mcp_handle.is_some() {
                    (self.tr.tr.mcp_on, p.success)
                } else {
                    (self.tr.tr.mcp_off, p.text_weak)
                };
                ui::pill(ui, p, text, color);
            });
        });
        ui.add_space(14.0);
    }

    /// Gallery of captures in the save directory.
    fn history_page(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        self.page_header(ui, p, t.history_title, t.history_subtitle);

        let files = self.scan_history();
        if files.is_empty() {
            ui.label(egui::RichText::new(t.history_empty).color(p.text_weak));
            return;
        }

        let sel_count = self.history_selection.len();
        let mut to_pin: Option<PathBuf> = None;
        let mut to_delete: Option<PathBuf> = None;
        let mut to_toggle: Option<PathBuf> = None;
        let mut select_all = false;
        let mut clear_sel = false;
        let mut export = false;

        // Toolbar: batch actions over the ticked items.
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(8.0, 0.0);
            if ui::button(ui, p, t.history_select_all).clicked() {
                select_all = true;
            }
            if sel_count > 0 {
                if ui::button(ui, p, t.history_clear_sel).clicked() {
                    clear_sel = true;
                }
                if ui::primary_button(ui, p, &format!("{} ({})", t.history_export_pdf, sel_count))
                    .clicked()
                {
                    export = true;
                }
                ui.label(
                    egui::RichText::new(format!("{} {}", sel_count, t.history_selected))
                        .color(p.text_weak),
                );
            }
        });
        ui.add_space(8.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 10.0);
                    for f in &files {
                        let Some(tex) = self.history_texture(ui.ctx(), &f.path) else {
                            continue;
                        };
                        let size = egui::vec2(168.0, 112.0);
                        let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
                        let selected = self.history_selection.contains(&f.path);
                        ui.painter().image(
                            tex.id(),
                            rect,
                            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                            egui::Color32::WHITE,
                        );
                        // Selection tick in the top-left corner; its hit area is
                        // carved out of the tile click below so pinning and
                        // ticking never fire on the same press.
                        let cb = egui::Rect::from_min_size(rect.min, egui::vec2(22.0, 22.0));
                        Self::draw_checkbox(ui, p, cb, selected);
                        if resp.clicked() {
                            let pos = ui.input(|i| i.pointer.interact_pos()).unwrap_or(rect.min);
                            if cb.contains(pos) {
                                to_toggle = Some(f.path.clone());
                            } else {
                                to_pin = Some(f.path.clone());
                            }
                        }
                        resp.context_menu(|ui| {
                            if ui.button(t.history_pin).clicked() {
                                to_pin = Some(f.path.clone());
                                ui.close();
                            }
                            if ui.button(t.history_delete).clicked() {
                                to_delete = Some(f.path.clone());
                                ui.close();
                            }
                        });
                    }
                });
            });

        if select_all {
            self.history_selection = files.iter().map(|f| f.path.clone()).collect();
        }
        if clear_sel {
            self.history_selection.clear();
        }
        if let Some(path) = to_toggle {
            if !self.history_selection.remove(&path) {
                self.history_selection.insert(path);
            }
        }
        if let Some(path) = to_pin {
            self.pin_history_file(&path);
        }
        if let Some(path) = to_delete {
            if let Err(err) = std::fs::remove_file(&path) {
                self.toast(format!("{}: {err}", t.history_delete), true);
            } else {
                self.textures.remove(&format!("hist-{}", path.display()));
                self.history_selection.remove(&path);
            }
        }
        if export {
            self.export_history_pdf(&files);
        }
    }

    /// Collect the ticked gallery items (newest-first, matching the grid) and
    /// write them to a PDF the user picks with a save dialog.
    fn export_history_pdf(&mut self, files: &[HistoryFile]) {
        let t = self.tr.tr;
        let chosen: Vec<PathBuf> = files
            .iter()
            .filter(|f| self.history_selection.contains(&f.path))
            .map(|f| f.path.clone())
            .collect();
        if chosen.is_empty() {
            return;
        }
        let Some(out) = rfd::FileDialog::new()
            .add_filter("PDF", &["pdf"])
            .set_file_name("pinsight-export.pdf")
            .save_file()
        else {
            return;
        };
        match crate::pdf::export_paths(&chosen, &out) {
            Ok(()) => self.toast(format!("{} {}", chosen.len(), t.history_exported), false),
            Err(err) => self.toast(format!("{}: {err}", t.history_export_pdf), true),
        }
    }

    /// Small selection tick drawn over a gallery tile's top-left corner.
    fn draw_checkbox(ui: &mut egui::Ui, p: &Palette, rect: egui::Rect, selected: bool) {
        let painter = ui.painter();
        let fill = if selected {
            p.accent
        } else {
            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200)
        };
        painter.rect_filled(rect, 5.0, fill);
        painter.rect_stroke(
            rect,
            5.0,
            egui::Stroke::new(1.0, p.stroke),
            egui::StrokeKind::Inside,
        );
        if selected {
            // White check mark.
            let c = rect.center();
            let s = rect.width().min(rect.height());
            let a = egui::pos2(c.x - s * 0.28, c.y + s * 0.02);
            let b = egui::pos2(c.x - s * 0.08, c.y + s * 0.22);
            let d = egui::pos2(c.x + s * 0.30, c.y - s * 0.24);
            painter.line_segment([a, b], egui::Stroke::new(s * 0.16, p.on_accent));
            painter.line_segment([b, d], egui::Stroke::new(s * 0.16, p.on_accent));
        }
    }

    fn scan_history(&mut self) -> Vec<HistoryFile> {
        if let Some((at, files)) = &self.history_scan {
            if at.elapsed() < Duration::from_secs(1) {
                return files.clone();
            }
        }
        let Ok(dir) = std::fs::read_dir(&self.config.save_dir) else {
            return Vec::new();
        };
        let mut out: Vec<HistoryFile> = dir
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                let ext = path.extension()?.to_str()?.to_ascii_lowercase();
                if !matches!(
                    ext.as_str(),
                    "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tif" | "tiff" | "gif"
                ) {
                    return None;
                }
                let modified = entry.metadata().ok()?.modified().ok()?;
                Some(HistoryFile { path, modified })
            })
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.modified));
        out.truncate(240);
        self.history_scan = Some((Instant::now(), out.clone()));
        out
    }

    fn history_texture(
        &mut self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureHandle> {
        let key = format!("hist-{}", path.display());
        if let Some(tex) = self.textures.get(&key) {
            return Some(tex.clone());
        }
        let img = image::open(path).ok()?.thumbnail(168, 112).to_rgba8();
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [img.width() as usize, img.height() as usize],
            img.as_raw(),
        );
        let tex = ctx.load_texture(key.clone(), color, egui::TextureOptions::LINEAR);
        self.textures.insert(key, tex.clone());
        Some(tex)
    }

    fn pin_history_file(&mut self, path: &std::path::Path) {
        match image::open(path) {
            Ok(img) => {
                let image = Arc::new(img.to_rgba8());
                self.last_image = Some(image.clone());
                self.push_history(&image);
                self.add_pin((*image).clone());
            }
            Err(err) => self.toast(format!("{}: {err}", self.tr.tr.capture_failed), true),
        }
    }

    fn capture_page(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        self.page_header(ui, p, t.capture_title, t.capture_subtitle);

        ui.horizontal(|ui| {
            if ui::primary_button(ui, p, t.act_window_title).clicked() {
                self.window_picker = Some(capture::list_windows());
            }
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(t.act_window_desc)
                    .size(12.0)
                    .color(p.text_weak),
            );
        });
        ui.add_space(10.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let hk = self.config.hotkeys.clone();
                let recording = self.record_job.is_some();
                let specs: [(Icon, &'static str, &'static str, String, bool, TileAction); 6] = [
                    (
                        Icon::Crop,
                        t.act_area_title,
                        t.act_area_desc,
                        hk.capture_area.clone(),
                        true,
                        TileAction::CaptureArea,
                    ),
                    (
                        Icon::Monitor,
                        t.act_full_title,
                        t.act_full_desc,
                        hk.capture_full.clone(),
                        false,
                        TileAction::CaptureFull,
                    ),
                    (
                        Icon::Pin,
                        t.act_pin_title,
                        t.act_pin_desc,
                        hk.pin_clipboard.clone(),
                        false,
                        TileAction::PinClipboard,
                    ),
                    (
                        Icon::Scroll,
                        t.act_long_title,
                        t.act_long_desc,
                        hk.long_capture.clone(),
                        false,
                        TileAction::LongShot,
                    ),
                    (
                        Icon::Record,
                        if recording {
                            t.stop_recording
                        } else {
                            t.act_record_title
                        },
                        t.act_record_desc,
                        hk.record.clone(),
                        false,
                        TileAction::Record,
                    ),
                    (
                        Icon::Save,
                        t.act_save_title,
                        t.act_save_desc,
                        hk.save_last.clone(),
                        false,
                        TileAction::SaveLast,
                    ),
                ];

                let gap = 12.0;
                let tile_w = ((ui.available_width() - gap) / 2.0).max(180.0);
                let mut clicked = None;
                for chunk in specs.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for (icon, title, desc, shortcut, primary, action) in chunk {
                            ui.allocate_ui(egui::vec2(tile_w, 84.0), |ui| {
                                ui.set_width(tile_w);
                                if ui::action_tile(ui, p, *icon, title, desc, shortcut, *primary)
                                    .clicked()
                                {
                                    clicked = Some(*action);
                                }
                            });
                        }
                    });
                    ui.add_space(gap);
                }

                if let Some(action) = clicked {
                    match action {
                        TileAction::CaptureArea => self.begin_area(Intent::Copy, &ctx),
                        TileAction::CaptureFull => self.begin_full_copy(&ctx),
                        TileAction::PinClipboard => self.pin_clipboard(),
                        TileAction::LongShot => self.begin_area(Intent::LongShot, &ctx),
                        TileAction::Record => {
                            if let Some(job) = &self.record_job {
                                job.stop();
                                self.toast(self.tr.tr.stopping_recording, false);
                            } else {
                                self.begin_area(Intent::Record, &ctx);
                            }
                        }
                        TileAction::SaveLast => self.save_last(),
                    }
                }

                ui.add_space(8.0);

                // ---- running jobs --------------------------------------
                if self.record_job.is_some() || self.long_job.is_some() {
                    ui::section_header(ui, p, Icon::Info, t.activity_title);
                    ui::card(ui, p, |ui| {
                        if let Some(job) = &self.record_job {
                            ui.horizontal(|ui| {
                                let (dot, _) = ui.allocate_exact_size(
                                    egui::vec2(10.0, 10.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().circle_filled(dot.center(), 4.0, p.danger);
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{:.1}s · {} {}",
                                        job.started.elapsed().as_secs_f32(),
                                        job.frames,
                                        t.frames
                                    ))
                                    .color(p.text),
                                );
                                ui.label(
                                    egui::RichText::new(job.path.display().to_string())
                                        .size(11.0)
                                        .color(p.text_weak),
                                );
                            });
                        }
                        if let Some(job) = &self.long_job {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(
                                    egui::RichText::new(format!(
                                        "{} · {} {} · {} {} px",
                                        t.long_running, job.frames, t.frames, job.height, t.tall
                                    ))
                                    .color(p.text),
                                );
                            });
                        }
                    });
                    ui.add_space(18.0);
                }

                // ---- last capture --------------------------------------
                ui::section_header(ui, p, Icon::Layers, t.recent_title);
                ui::card(ui, p, |ui| {
                    let Some(image) = self.last_image.clone() else {
                        ui::hint_row(ui, p, Icon::Crop, t.recent_empty);
                        return;
                    };
                    let width = ui.available_width();
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(width, 150.0), egui::Sense::hover());
                    let texture = self.texture_for(ui.ctx(), "last", &image);
                    ui::paint_thumbnail(ui.painter(), rect, &texture, p);

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("{}×{}", image.width(), image.height()))
                                .monospace()
                                .size(11.5)
                                .color(p.text_weak),
                        );
                        if let Some(at) = self.last_capture_at {
                            ui.label(
                                egui::RichText::new(format!(
                                    "· {:.0}s",
                                    at.elapsed().as_secs_f32()
                                ))
                                .size(11.0)
                                .color(p.text_weak),
                            );
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui::link_button(ui, p, Icon::Save, t.act_save_title).clicked() {
                                self.save_last();
                            }
                            if ui::link_button(ui, p, Icon::Pin, t.ov_pin).clicked() {
                                let image = (*image).clone();
                                self.add_pin(image);
                                self.toast(self.tr.tr.pinned, false);
                            }
                        });
                    });
                });

                if let Some(path) = self.last_saved.clone() {
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(path.display().to_string())
                                .size(11.0)
                                .color(p.text_weak),
                        );
                        if ui::link_button(ui, p, Icon::Folder, t.open_folder).clicked() {
                            util::open_path(&path);
                        }
                    });
                }
                ui.add_space(10.0);
            });
    }

    fn pins_page(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        self.page_header(ui, p, t.pins_title, t.pins_subtitle);

        if self.group_count(self.active_group) == 0 {
            ui::card(ui, p, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 16.0, p.accent_soft);
                    ui::paint_icon(ui.painter(), rect.shrink(13.0), Icon::Pin, p.accent, 1.8);
                    ui.add_space(12.0);
                    ui.label(
                        egui::RichText::new(t.pins_empty_title)
                            .size(15.0)
                            .strong()
                            .color(p.text),
                    );
                    ui.add_space(2.0);
                    ui.label(
                        egui::RichText::new(t.pins_empty_desc)
                            .size(12.0)
                            .color(p.text_weak),
                    );
                    ui.add_space(28.0);
                });
            });
            return;
        }

        let shown = self.group_count(self.active_group);
        ui.horizontal(|ui| {
            ui::pill(ui, p, &format!("{} {}", shown, t.pins_title), p.accent);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::link_button(ui, p, Icon::Trash, t.close_all).clicked() {
                    // Scoped to the group on screen: "close all" should never
                    // reach into a group the user is not looking at.
                    let closed: Vec<ClosedPin> = self
                        .pins
                        .iter()
                        .filter(|pin| pin.group == self.active_group)
                        .map(|pin| ClosedPin {
                            image: pin.image.clone(),
                            x: pin.x,
                            y: pin.y,
                            w: pin.width,
                            h: pin.height,
                            group: pin.group,
                        })
                        .collect();
                    for pin in self.pins.iter_mut() {
                        if pin.group == self.active_group {
                            pin.close();
                        }
                    }
                    self.pins.retain(|pin| pin.group != self.active_group);
                    for pin in closed {
                        self.push_closed_pin(pin);
                    }
                }
                let restore_label =
                    format!("{} ({})", t.pin_restore_action, self.closed_pins.len());
                if ui::link_button(ui, p, Icon::Reset, &restore_label).clicked() {
                    self.restore_closed_pin();
                }
            });
        });

        // ---- groups -------------------------------------------------------
        // Renaming takes over the row so the user can type in place.
        if let Some((editing, draft)) = self.group_rename.clone() {
            ui::field_row(ui, p, t.pin_group_rename, |ui| {
                if ui::link_button(ui, p, Icon::Check, t.ok).clicked() {
                    self.rename_group(editing, draft.clone());
                    self.group_rename = None;
                }
                if ui::link_button(ui, p, Icon::Close, t.key_cancel).clicked() {
                    self.group_rename = None;
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.group_rename.as_mut().unwrap().1)
                        .hint_text(t.pin_group_name_hint)
                        .desired_width(200.0),
                );
                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    self.rename_group(editing, draft.clone());
                    self.group_rename = None;
                }
            });
        } else {
            ui::field_row(ui, p, t.pin_group_move, |ui| {
                // Right-to-left row: the action buttons come first so they end
                // up rightmost, the selector fills the space to their left.
                if ui::link_button(ui, p, Icon::Trash, t.pin_group_delete).clicked() {
                    self.delete_group(self.active_group);
                    self.toast(self.tr.tr.pin_group_deleted, false);
                }
                let hidden = self
                    .config
                    .pin_groups
                    .iter()
                    .find(|g| g.id == self.active_group)
                    .map(|g| g.hidden)
                    .unwrap_or(false);
                if ui::link_button(
                    ui,
                    p,
                    if hidden { Icon::Eye } else { Icon::EyeOff },
                    if hidden {
                        t.pin_group_show
                    } else {
                        t.pin_group_hide
                    },
                )
                .clicked()
                {
                    self.set_group_hidden(self.active_group, !hidden);
                }
                if ui::link_button(ui, p, Icon::Type, t.pin_group_rename).clicked() {
                    let name = self
                        .config
                        .pin_groups
                        .iter()
                        .find(|g| g.id == self.active_group)
                        .map(|g| g.name.clone())
                        .unwrap_or_default();
                    self.group_rename = Some((self.active_group, name));
                }
                if ui::link_button(ui, p, Icon::Layers, t.pin_group_new).clicked() {
                    self.create_group();
                    self.toast(self.tr.tr.pin_group_created, false);
                }

                let mut picked = None;
                egui::ComboBox::from_id_salt("pinsight-pin-group")
                    .selected_text(self.group_name(self.active_group))
                    .width(200.0)
                    .show_ui(ui, |ui| {
                        for group in self.config.pin_groups.clone() {
                            let label = format!(
                                "{} ({})",
                                self.group_name(group.id),
                                self.group_count(group.id)
                            );
                            if ui
                                .selectable_label(group.id == self.active_group, label)
                                .clicked()
                            {
                                picked = Some(group.id);
                            }
                        }
                    });
                if let Some(id) = picked {
                    self.active_group = id;
                }
            });
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(t.pin_group_hint)
                    .size(11.0)
                    .color(p.text_weak),
            );
            ui.label(
                egui::RichText::new(t.pin_group_active)
                    .size(11.0)
                    .color(p.text_weak),
            );
        }
        ui.add_space(10.0);

        let gap = 12.0;
        let cell_w = ((ui.available_width() - gap) / 2.0).max(220.0);
        const CARD_H: f32 = 156.0;
        const PAD: f32 = 10.0;
        // The settings page owns the preview height.
        let thumb_h = self.config.pin_thumb.clamp(40, 160) as f32;
        const BTN: f32 = 28.0;

        // Snapshot what the UI needs so we can mutate `self` inside the loop.
        struct Row {
            id: u64,
            image: Arc<RgbaImage>,
            zoom: f32,
            passthrough: bool,
            group: u32,
        }
        let active = self.active_group;
        let rows: Vec<Row> = self
            .pins
            .iter()
            .filter(|pin| pin.group == active)
            .map(|pin| Row {
                id: pin.id,
                image: pin.image.clone(),
                zoom: pin.zoom,
                passthrough: pin.passthrough,
                group: pin.group,
            })
            .collect();

        let mut action: Option<(u64, PinAction)> = None;

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for chunk in rows.chunks(2) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for row in chunk {
                            // The card is painted by hand: mixing egui layouts for
                            // the preview, the metadata and the action row kept
                            // fighting over the available width.
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(cell_w, CARD_H),
                                egui::Sense::hover(),
                            );
                            if !ui.is_rect_visible(rect) {
                                continue;
                            }
                            let painter = ui.painter().clone();
                            let hovered = ui.rect_contains_pointer(rect);
                            painter.rect_filled(
                                rect,
                                14.0,
                                if hovered { p.card_hover } else { p.card },
                            );
                            painter.rect_stroke(
                                rect,
                                14.0,
                                egui::Stroke::new(
                                    1.0,
                                    if hovered {
                                        theme::with_alpha(p.accent, 160)
                                    } else {
                                        p.stroke
                                    },
                                ),
                                egui::StrokeKind::Inside,
                            );

                            let thumb = egui::Rect::from_min_size(
                                rect.min + egui::vec2(PAD, PAD),
                                egui::vec2(cell_w - PAD * 2.0, thumb_h),
                            );
                            let texture =
                                self.texture_for(&ctx, &format!("pin-{}", row.id), &row.image);
                            ui::paint_thumbnail(&painter, thumb, &texture, p);

                            // Clicking the preview raises that pin.
                            let focus = ui.interact(
                                thumb,
                                ui.scope_id().with(("pin-thumb", row.id)),
                                egui::Sense::click(),
                            );
                            if focus.clicked() {
                                action = Some((row.id, PinAction::Focus));
                            }
                            if focus.hovered() {
                                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                            }

                            let foot_y = thumb.bottom() + 20.0;
                            painter.text(
                                egui::pos2(thumb.left(), foot_y),
                                egui::Align2::LEFT_CENTER,
                                format!("#{}", row.id),
                                egui::FontId::monospace(11.5),
                                p.text,
                            );
                            let dims = format!("{}×{}", row.image.width(), row.image.height());
                            let dims_galley = painter.layout_no_wrap(
                                dims,
                                egui::FontId::proportional(11.0),
                                p.text_weak,
                            );
                            painter.galley(
                                egui::pos2(
                                    thumb.left() + 34.0,
                                    foot_y - dims_galley.size().y * 0.5,
                                ),
                                dims_galley,
                                p.text_weak,
                            );

                            let buttons: [(Icon, bool, &str, PinAction); 5] = [
                                (Icon::Focus, false, t.pin_focus, PinAction::Focus),
                                (
                                    Icon::Through,
                                    row.passthrough,
                                    t.pin_through,
                                    PinAction::Through,
                                ),
                                (Icon::Copy, false, t.pin_copy, PinAction::Copy),
                                (Icon::Close, false, t.pin_close, PinAction::Close),
                                (Icon::Layers, false, t.pin_group_move, PinAction::Group),
                            ];
                            let strip =
                                buttons.len() as f32 * BTN + (buttons.len() - 1) as f32 * 4.0;
                            let strip_x = rect.right() - PAD - strip;

                            let zoom = format!("{:.0}%", row.zoom * 100.0);
                            let zoom_galley = painter.layout_no_wrap(
                                zoom,
                                egui::FontId::monospace(11.0),
                                p.text_weak,
                            );
                            let zoom_x = strip_x - 10.0 - zoom_galley.size().x;
                            if zoom_x > thumb.left() + 90.0 {
                                painter.galley(
                                    egui::pos2(zoom_x, foot_y - zoom_galley.size().y * 0.5),
                                    zoom_galley,
                                    p.text_weak,
                                );
                            }

                            let mut bx = strip_x;
                            for (i, (icon, active, tip, act)) in buttons.into_iter().enumerate() {
                                let br = egui::Rect::from_min_size(
                                    egui::pos2(bx, rect.bottom() - PAD - BTN),
                                    egui::Vec2::splat(BTN),
                                );
                                let r = ui
                                    .interact(
                                        br,
                                        ui.scope_id().with(("pin-act", row.id, i)),
                                        egui::Sense::click(),
                                    )
                                    .on_hover_text(tip);
                                if matches!(act, PinAction::Group) {
                                    // A menu anchored on the button itself: no
                                    // extra state, and egui handles the
                                    // click-outside dismissal.
                                    let mut chosen = None;
                                    egui::Popup::menu(&r)
                                        .close_behavior(
                                            egui::PopupCloseBehavior::CloseOnClickOutside,
                                        )
                                        .show(|ui| {
                                            ui.set_min_width(140.0);
                                            for group in self.config.pin_groups.clone() {
                                                let label = format!(
                                                    "{} ({})",
                                                    self.group_name(group.id),
                                                    self.group_count(group.id)
                                                );
                                                if ui
                                                    .selectable_label(group.id == row.group, label)
                                                    .clicked()
                                                {
                                                    chosen = Some(group.id);
                                                }
                                            }
                                        });
                                    if let Some(target) = chosen {
                                        self.set_pin_group(row.id, target);
                                    }
                                    continue;
                                }
                                if active || r.hovered() {
                                    painter.rect_filled(
                                        br,
                                        8.0,
                                        if active { p.accent_soft } else { p.card_hover },
                                    );
                                }
                                let color = if active {
                                    p.accent
                                } else if r.hovered() {
                                    p.text
                                } else {
                                    p.text_weak
                                };
                                ui::paint_icon(&painter, br.shrink(7.0), icon, color, 1.9);
                                if r.clicked() {
                                    action = Some((row.id, act));
                                }
                                bx += BTN + 4.0;
                            }
                        }
                    });
                    ui.add_space(gap);
                }
            });

        if let Some((id, act)) = action {
            match act {
                PinAction::Focus => {
                    if let Some(pin) = self.pins.iter().find(|p| p.id == id) {
                        pin.focus();
                    }
                }
                PinAction::Through => {
                    if let Some(pin) = self.pins.iter_mut().find(|p| p.id == id) {
                        pin.toggle_passthrough();
                    }
                }
                PinAction::Copy => {
                    if let Some(pin) = self.pins.iter().find(|p| p.id == id) {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            let _ = clipboard.set_image(util::to_arboard(&pin.image));
                        }
                        self.toast(self.tr.tr.copied, false);
                    }
                }
                PinAction::Close => {
                    if let Some(pin) = self.pins.iter_mut().find(|p| p.id == id) {
                        pin.close();
                    }
                    self.pins.retain(|p| p.id != id);
                    self.textures.remove(&format!("pin-{id}"));
                }
                // Handled inline: the popup performs the move itself.
                PinAction::Group => {}
            }
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        self.page_header(ui, p, t.settings_title, t.settings_subtitle);

        let tabs: [(SettingsTab, &str); 8] = [
            (SettingsTab::General, t.tab_general),
            (SettingsTab::Appearance, t.tab_appearance),
            (SettingsTab::Capture, t.tab_capture),
            (SettingsTab::Pins, t.tab_pins),
            (SettingsTab::Output, t.tab_output),
            (SettingsTab::Controls, t.tab_controls),
            (SettingsTab::Updates, t.tab_updates),
            (SettingsTab::About, t.tab_about),
        ];
        if let Some(tab) = ui::tabs(ui, p, self.settings_tab, &tabs) {
            if tab != self.settings_tab {
                self.settings_tab = tab;
                // Remember the slice so the next launch opens here too.
                self.config.settings_tab = tab;
                let _ = self.config.save();
            }
            // Leaving the hotkeys tab must not leave a recorder dangling.
            self.recording_hotkey = None;
        }
        ui.add_space(14.0);

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| match self.settings_tab {
                SettingsTab::General => self.general_tab(ui, p),
                SettingsTab::Appearance => {
                    self.appearance_tab(ui, p);
                    ui.add_space(14.0);
                    self.background_tab(ui, p);
                }
                SettingsTab::Capture => self.capture_tab(ui, p),
                SettingsTab::Pins => self.pins_tab(ui, p),
                SettingsTab::Output => self.output_tab(ui, p),
                SettingsTab::Controls => self.controls_tab(ui, p),
                SettingsTab::Updates => self.updates_tab(ui, p),
                SettingsTab::About => self.about_tab(ui, p),
            });
        ui.add_space(12.0);
    }

    /// Theme, accent, language and typography.
    fn appearance_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Palette, t.sec_appearance);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.theme, |ui| {
                let options = [
                    (ThemeMode::System, t.theme_system),
                    (ThemeMode::Light, t.theme_light),
                    (ThemeMode::Dark, t.theme_dark),
                ];
                if let Some(mode) = ui::segmented(ui, p, self.config.theme, &options) {
                    self.config.theme = mode;
                    self.apply_theme(&ctx);
                }
            });

            ui::field_row(ui, p, t.accent, |ui| {
                let presets = [
                    (theme::Accent::Blue, t.accent_blue),
                    (theme::Accent::Violet, t.accent_violet),
                    (theme::Accent::Emerald, t.accent_emerald),
                    (theme::Accent::Amber, t.accent_amber),
                    (theme::Accent::Rose, t.accent_rose),
                    (theme::Accent::Cyan, t.accent_cyan),
                ];
                let mut items: Vec<(egui::Color32, bool, &str)> = presets
                    .iter()
                    .map(|(accent, name)| {
                        (
                            accent.resolve(self.config.accent_custom),
                            self.config.accent == *accent,
                            *name,
                        )
                    })
                    .collect();
                // The custom slot sits after the presets, like Snipaste.
                items.push((
                    theme::Accent::Custom.resolve(self.config.accent_custom),
                    self.config.accent == theme::Accent::Custom,
                    t.accent_custom,
                ));
                if let Some(index) = ui::swatch_row(ui, p, &items) {
                    if index < presets.len() {
                        self.config.accent = presets[index].0;
                        self.apply_theme(&ctx);
                    } else {
                        self.open_color_picker(crate::picker::ColorTarget::Accent);
                    }
                }
            });

            ui::field_row(ui, p, t.font_family, |ui| {
                // A real family list, like a font dialog: every installed
                // family, plus an "automatic" entry that follows the language.
                // The list is rebuilt only when the setting actually changes,
                // because enumerating the registry is not free.
                if self.font_families.is_none() {
                    self.font_families = Some(crate::fonts::families());
                }
                let families = self.font_families.as_deref().unwrap_or(&[]);
                let automatic = t.font_auto;
                let current = self
                    .config
                    .font_family
                    .clone()
                    .unwrap_or_else(|| automatic.to_string());
                let mut picked: Option<Option<String>> = None;
                egui::ComboBox::from_id_salt("pinsight-font")
                    .selected_text(current)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(self.config.font_family.is_none(), automatic)
                            .clicked()
                        {
                            picked = Some(None);
                        }
                        for (family, _) in families {
                            let chosen = self.config.font_family.as_deref() == Some(family);
                            if ui.selectable_label(chosen, family.as_str()).clicked() {
                                picked = Some(Some(family.clone()));
                            }
                        }
                    });
                if let Some(family) = picked {
                    self.config.font_family = family;
                    let _ = self.config.save();
                    self.apply_fonts(&ctx);
                }
            });

            if self.config.font == FontKind::Custom {
                ui.add_space(6.0);
                ui::field_row(ui, p, "", |ui| {
                    // Right-aligned rows lay their children out right-to-left,
                    // so the *last* widget added ends up leftmost. The action
                    // buttons therefore come first here.
                    if ui::link_button(ui, p, Icon::Check, t.apply).clicked() {
                        self.config.font_path = non_empty_path(&self.font_path_draft);
                        let _ = self.config.save();
                        self.apply_fonts(&ctx);
                    }
                    ui.add(
                        egui::TextEdit::singleline(&mut self.font_path_draft)
                            .hint_text(t.font_path_hint)
                            .desired_width(220.0),
                    );
                });
            }

            ui::field_row(ui, p, t.font_size, |ui| {
                // Point sizes, the way a font dialog offers them. 9 pt is the
                // baseline the palette was drawn at.
                let sizes = [8.0, 9.0, 10.0, 11.0, 12.0, 14.0, 16.0];
                let mut picked = None;
                egui::ComboBox::from_id_salt("pinsight-font-size")
                    .selected_text(format!("{} pt", self.config.font_size_pt))
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for size in sizes {
                            if ui
                                .selectable_label(
                                    (size - self.config.font_size_pt).abs() < 0.01,
                                    format!("{size} pt"),
                                )
                                .clicked()
                            {
                                picked = Some(size);
                            }
                        }
                    });
                if let Some(size) = picked {
                    self.config.font_size_pt = size;
                    let _ = self.config.save();
                    self.apply_fonts(&ctx);
                }
                if ui::link_button(ui, p, Icon::Reset, t.reset).clicked() {
                    self.config.font_size_pt = 9.0;
                    let _ = self.config.save();
                    self.apply_fonts(&ctx);
                }
            });

            if let Some(name) = &self.font_report.primary {
                ui.add_space(6.0);
                let mut line = format!("{}: {}", t.font_family, name);
                if let Some(fallback) = &self.font_report.fallback {
                    line.push_str(&format!(" + {fallback}"));
                }
                ui.label(egui::RichText::new(line).size(11.0).color(p.text_weak));
            }
        });
    }

    /// Custom wallpaper: pick a file, then tune blur, dimming and framing.
    fn background_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Image, t.sec_background);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(match &self.config.bg_image {
                    Some(path) => path.display().to_string(),
                    None => t.bg_none.to_string(),
                })
                .size(11.0)
                .color(p.text_weak),
            );
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui::link_button(ui, p, Icon::Folder, t.bg_choose).clicked() {
                    // Blocking on purpose: it is a modal dialog and the window
                    // has nothing worth painting meanwhile.
                    if let Some(path) = background::pick_image() {
                        self.config.bg_image = Some(path);
                        self.config.bg_blur = self.config.bg_blur.clamp(0.0, 40.0);
                        let _ = self.config.save();
                        self.sync_background(&ctx);
                    }
                }
                if self.config.bg_image.is_some()
                    && ui::link_button(ui, p, Icon::Close, t.bg_clear).clicked()
                {
                    self.config.bg_image = None;
                    let _ = self.config.save();
                    self.sync_background(&ctx);
                }
            });

            if self.config.bg_image.is_some() {
                ui.add_space(10.0);

                ui::field_row(ui, p, t.bg_blur, |ui| {
                    let mut blur = self.config.bg_blur;
                    let response = ui::slider(ui, p, &mut blur, 0.0..=40.0, 0, " px");
                    if response.changed() {
                        self.config.bg_blur = blur;
                        let _ = self.config.save();
                        self.sync_background(&ctx);
                    }
                });

                ui::field_row(ui, p, t.bg_dim, |ui| {
                    let mut dim = self.config.bg_dim;
                    let response = ui::slider(ui, p, &mut dim, 0.0..=0.85, 2, "");
                    if response.changed() {
                        self.config.bg_dim = dim;
                        let _ = self.config.save();
                    }
                });

                ui::field_row(ui, p, t.bg_fit, |ui| {
                    let options = [
                        (BgFit::Cover, t.bg_cover),
                        (BgFit::Contain, t.bg_contain),
                        (BgFit::Stretch, t.bg_stretch),
                    ];
                    if let Some(fit) = ui::segmented(ui, p, self.config.bg_fit, &options) {
                        self.config.bg_fit = fit;
                        let _ = self.config.save();
                    }
                });
            }
        });
    }

    /// Where captures land and where the config lives.
    fn output_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Folder, t.sec_output);
            ui::field_row(ui, p, t.output_dir, |ui| {
                // Measure before adding anything: this row lays out
                // right-to-left, so by the time the buttons are in the
                // available width is the leftover, not the row width.
                let width = (ui.available_width() - 210.0).clamp(150.0, 420.0);
                // Right-to-left row: the first widget added sits rightmost.
                if ui::link_button(ui, p, Icon::Folder, t.open_folder).clicked() {
                    util::open_path(&self.config.save_dir);
                }
                if ui::link_button(ui, p, Icon::Check, t.apply).clicked() {
                    let path = PathBuf::from(self.save_dir_draft.trim());
                    if !path.as_os_str().is_empty() {
                        self.config.save_dir = path;
                        let _ = self.config.ensure_save_dir();
                        let _ = self.config.save();
                    }
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.save_dir_draft)
                        .desired_width(width)
                        .font(egui::TextStyle::Monospace),
                );
            });
            ui.add_space(6.0);
            ui::field_row(ui, p, t.config_file, |ui| {
                ui.label(
                    egui::RichText::new(Config::path().display().to_string())
                        .monospace()
                        .size(11.0)
                        .color(p.text_weak),
                );
                if ui::link_button(ui, p, Icon::Folder, t.open_config).clicked() {
                    if let Some(parent) = Config::path().parent() {
                        util::open_path(parent);
                    }
                }
            });
            if !config::is_portable() {
                ui.label(
                    egui::RichText::new(t.config_installed_hint)
                        .size(11.0)
                        .color(p.text_weak),
                );
            }
        });

        // ---- naming rule --------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Type, t.out_naming);
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.out_naming_hint)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
            ui.add_space(6.0);
            ui::field_row(ui, p, t.out_naming, |ui| {
                let width = (ui.available_width() - 110.0).clamp(150.0, 420.0);
                if ui::link_button(ui, p, Icon::Reset, t.out_reset_name).clicked() {
                    self.name_template_draft = crate::naming::DEFAULT_TEMPLATE.to_string();
                    self.config.name_template = self.name_template_draft.clone();
                    let _ = self.config.save();
                }
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.name_template_draft)
                        .desired_width(width)
                        .font(egui::TextStyle::Monospace),
                );
                if response.changed() {
                    self.config.name_template = self.name_template_draft.trim().to_string();
                    let _ = self.config.save();
                }
            });
            ui.add_space(6.0);
            ui::field_row(ui, p, t.out_preview, |ui| {
                let preview = crate::naming::sanitize(&crate::naming::expand(
                    &self.name_template_draft,
                    "Example — 示例窗口",
                ));
                ui.label(
                    egui::RichText::new(format!(
                        "{preview}.{}",
                        self.config.image_format.extension()
                    ))
                    .monospace()
                    .size(12.0)
                    .color(p.accent),
                );
            });
            ui.add_space(6.0);
            ui.collapsing(t.out_format_codes, |ui| {
                // Three groups, mirroring the Snipaste naming dialog: date
                // codes, special variables and plain environment variables.
                // Every description comes from the translation table, so the
                // panel is never half English.
                ui.label(
                    egui::RichText::new(t.nm_date_codes)
                        .size(11.0)
                        .color(p.text_weak),
                );
                for code in crate::naming::DATE_CODES {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("${code}$"))
                                .monospace()
                                .size(11.5)
                                .color(p.accent),
                        );
                        ui.label(
                            egui::RichText::new(naming_code_desc(code, t))
                                .size(11.0)
                                .color(p.text_weak),
                        );
                    });
                }

                ui.add_space(6.0);
                ui.label(egui::RichText::new(t.nm_vars).size(11.0).color(p.text_weak));
                for name in crate::naming::VAR_NAMES {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("%{name}%"))
                                .monospace()
                                .size(11.5)
                                .color(p.accent),
                        );
                        ui.label(
                            egui::RichText::new(naming_var_desc(name, t))
                                .size(11.0)
                                .color(p.text_weak),
                        );
                    });
                }
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t.nm_env_hint)
                        .monospace()
                        .size(11.0)
                        .color(p.text_weak),
                );
            });
        });

        // ---- image format + quality --------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Image, t.out_img_format);
            ui::field_row(ui, p, t.out_img_format, |ui| {
                let all = [
                    util::ImageFormat::Png,
                    util::ImageFormat::Jpeg,
                    util::ImageFormat::Webp,
                    util::ImageFormat::Bmp,
                    util::ImageFormat::Tiff,
                    util::ImageFormat::Gif,
                ];
                let label = |f: util::ImageFormat| -> &'static str {
                    match f {
                        util::ImageFormat::Png => "PNG",
                        util::ImageFormat::Jpeg => "JPEG",
                        util::ImageFormat::Webp => "WebP",
                        util::ImageFormat::Bmp => "BMP",
                        util::ImageFormat::Tiff => "TIFF",
                        util::ImageFormat::Gif => "GIF",
                    }
                };
                let mut picked = None;
                egui::ComboBox::from_id_salt("pinsight-image-format")
                    .selected_text(label(self.config.image_format))
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for format in all {
                            if ui
                                .selectable_label(format == self.config.image_format, label(format))
                                .clicked()
                            {
                                picked = Some(format);
                            }
                        }
                    });
                if let Some(format) = picked {
                    self.config.image_format = format;
                    let _ = self.config.save();
                }
            });
            if self.config.image_format != util::ImageFormat::Png {
                ui::field_row(ui, p, t.out_img_quality, |ui| {
                    let mut value = self.config.image_quality as f32;
                    if ui::slider(ui, p, &mut value, 10.0..=100.0, 0, " %").changed() {
                        self.config.image_quality = value.round() as u8;
                        let _ = self.config.save();
                    }
                });
            }
        });

        // ---- save decorations: border / shadow / watermark ----------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Layers, t.out_decorations);
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.out_decorations_hint)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );

            // Border frame.
            let mut border_on = self.config.decorations.border.is_some();
            ui::field_row(ui, p, t.deco_border, |ui| {
                if ui::toggle(ui, p, &mut border_on).changed() {
                    self.config.decorations.border = if border_on {
                        Some(crate::decorate::BorderDeco {
                            width: 6,
                            color: [255, 255, 255, 255],
                        })
                    } else {
                        None
                    };
                    let _ = self.config.save();
                }
            });
            if let Some(border) = self.config.decorations.border.clone() {
                ui::field_row(ui, p, t.deco_border_width, |ui| {
                    let mut value = border.width as f32;
                    if ui::slider(ui, p, &mut value, 1.0..=40.0, 0, " px").changed() {
                        if let Some(b) = &mut self.config.decorations.border {
                            b.width = value.round() as u32;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_border_color, |ui| {
                    let rgb = [border.color[0], border.color[1], border.color[2]];
                    if ui::color_field(ui, p, rgb).clicked() {
                        self.open_color_picker(crate::picker::ColorTarget::DecoBorder);
                    }
                });
            }

            // Drop shadow.
            let mut shadow_on = self.config.decorations.shadow.is_some();
            ui::field_row(ui, p, t.deco_shadow, |ui| {
                if ui::toggle(ui, p, &mut shadow_on).changed() {
                    self.config.decorations.shadow = if shadow_on {
                        Some(crate::decorate::ShadowDeco {
                            blur: 12,
                            offset_x: 0,
                            offset_y: 6,
                            color: [0, 0, 0],
                            opacity: 120,
                        })
                    } else {
                        None
                    };
                    let _ = self.config.save();
                }
            });
            if let Some(shadow) = self.config.decorations.shadow.clone() {
                ui::field_row(ui, p, t.deco_shadow_blur, |ui| {
                    let mut value = shadow.blur as f32;
                    if ui::slider(ui, p, &mut value, 2.0..=48.0, 0, " px").changed() {
                        if let Some(s) = &mut self.config.decorations.shadow {
                            s.blur = value.round() as u32;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_shadow_offset, |ui| {
                    let mut value = shadow.offset_y as f32;
                    if ui::slider(ui, p, &mut value, -24.0..=24.0, 0, " px").changed() {
                        if let Some(s) = &mut self.config.decorations.shadow {
                            s.offset_y = value.round() as i32;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_shadow_opacity, |ui| {
                    let mut value = shadow.opacity as f32 / 2.55;
                    if ui::slider(ui, p, &mut value, 0.0..=100.0, 0, " %").changed() {
                        if let Some(s) = &mut self.config.decorations.shadow {
                            s.opacity = (value * 2.55).round() as u8;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_shadow_color, |ui| {
                    if ui::color_field(ui, p, shadow.color).clicked() {
                        self.open_color_picker(crate::picker::ColorTarget::DecoShadow);
                    }
                });
            }

            // Watermark.
            let mut wm_on = self.config.decorations.watermark.is_some();
            ui::field_row(ui, p, t.deco_watermark, |ui| {
                if ui::toggle(ui, p, &mut wm_on).changed() {
                    self.config.decorations.watermark = if wm_on {
                        Some(crate::decorate::WatermarkDeco {
                            text: "PinSight".into(),
                            image_path: None,
                            position: crate::decorate::WatermarkPos::BottomRight,
                            opacity: 140,
                            text_scale: 0.08,
                            image_scale: 0.2,
                            margin: 12,
                        })
                    } else {
                        None
                    };
                    let _ = self.config.save();
                }
            });
            if let Some(wm) = self.config.decorations.watermark.clone() {
                ui::field_row(ui, p, t.deco_wm_text, |ui| {
                    let width = (ui.available_width() - 20.0).clamp(120.0, 360.0);
                    let mut text = wm.text.clone();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut text)
                                .desired_width(width)
                                .hint_text("PinSight"),
                        )
                        .changed()
                    {
                        if let Some(w) = &mut self.config.decorations.watermark {
                            w.text = text;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_wm_position, |ui| {
                    let label = |pos: crate::decorate::WatermarkPos| -> &'static str {
                        match pos {
                            crate::decorate::WatermarkPos::Tile => t.deco_wm_tile,
                            crate::decorate::WatermarkPos::Center => t.deco_wm_center,
                            crate::decorate::WatermarkPos::TopLeft => t.deco_wm_tl,
                            crate::decorate::WatermarkPos::TopRight => t.deco_wm_tr,
                            crate::decorate::WatermarkPos::BottomLeft => t.deco_wm_bl,
                            crate::decorate::WatermarkPos::BottomRight => t.deco_wm_br,
                        }
                    };
                    egui::ComboBox::from_id_salt("pinsight-wm-pos")
                        .selected_text(label(wm.position))
                        .width(150.0)
                        .show_ui(ui, |ui| {
                            let mut picked = None;
                            for pos in crate::decorate::WatermarkPos::ALL {
                                if ui
                                    .selectable_label(pos == wm.position, label(pos))
                                    .clicked()
                                {
                                    picked = Some(pos);
                                }
                            }
                            if let Some(pos) = picked {
                                if let Some(w) = &mut self.config.decorations.watermark {
                                    w.position = pos;
                                }
                                let _ = self.config.save();
                            }
                        });
                });
                ui::field_row(ui, p, t.deco_wm_opacity, |ui| {
                    let mut value = wm.opacity as f32 / 2.55;
                    if ui::slider(ui, p, &mut value, 0.0..=100.0, 0, " %").changed() {
                        if let Some(w) = &mut self.config.decorations.watermark {
                            w.opacity = (value * 2.55).round() as u8;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_wm_size, |ui| {
                    let mut value = wm.text_scale * 100.0;
                    if ui::slider(ui, p, &mut value, 2.0..=30.0, 0, " %").changed() {
                        if let Some(w) = &mut self.config.decorations.watermark {
                            w.text_scale = value / 100.0;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_wm_margin, |ui| {
                    let mut value = wm.margin as f32;
                    if ui::slider(ui, p, &mut value, 0.0..=96.0, 0, " px").changed() {
                        if let Some(w) = &mut self.config.decorations.watermark {
                            w.margin = value.round() as u32;
                        }
                        let _ = self.config.save();
                    }
                });
                ui::field_row(ui, p, t.deco_wm_image, |ui| {
                    let width = (ui.available_width() - 90.0).clamp(120.0, 300.0);
                    let mut path = wm
                        .image_path
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_default();
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut path)
                                .desired_width(width)
                                .hint_text(t.deco_wm_image_hint),
                        )
                        .changed()
                    {
                        if let Some(w) = &mut self.config.decorations.watermark {
                            w.image_path = if path.trim().is_empty() {
                                None
                            } else {
                                Some(PathBuf::from(path.trim()))
                            };
                        }
                        let _ = self.config.save();
                    }
                    if ui::link_button(ui, p, Icon::Folder, t.deco_wm_browse).clicked() {
                        if let Some(picked) = rfd::FileDialog::new()
                            .add_filter("Image", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
                            .pick_file()
                        {
                            if let Some(w) = &mut self.config.decorations.watermark {
                                w.image_path = Some(picked);
                            }
                            let _ = self.config.save();
                        }
                    }
                });
            }
        });

        // ---- recording ----------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Record, t.rec_settings);
            ui::field_row(ui, p, t.rec_fps, |ui| {
                let mut value = self.config.gif_fps as f32;
                if ui::slider(ui, p, &mut value, 4.0..=30.0, 0, " fps").changed() {
                    self.config.gif_fps = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.rec_max_seconds, |ui| {
                let mut value = self.config.record_max_seconds as f32;
                // 0 renders as "unlimited"; the slider goes to 600 s in 5 s
                // steps, which covers every sane use.
                if ui::slider(ui, p, &mut value, 0.0..=600.0, 0, " s").changed() {
                    self.config.record_max_seconds = value.round() as u32;
                    let _ = self.config.save();
                }
                let text = if self.config.record_max_seconds == 0 {
                    t.rec_unlimited.to_string()
                } else {
                    util::fmt_seconds(self.config.record_max_seconds)
                };
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(text)
                        .monospace()
                        .size(11.0)
                        .color(p.text_weak),
                );
            });
            ui::field_row(ui, p, t.rec_audio, |ui| {
                if ui::toggle(ui, p, &mut self.config.record_audio).changed() {
                    let _ = self.config.save();
                }
            });
            ui::field_row(ui, p, t.rec_audio_device, |ui| {
                if ui
                    .text_edit_singleline(&mut self.config.audio_device)
                    .changed()
                {
                    self.config.audio_device = self.config.audio_device.trim().to_string();
                    let _ = self.config.save();
                }
            });
            ui.add_space(2.0);
            ui.label(
                egui::RichText::new(t.rec_audio_hint)
                    .size(11.0)
                    .color(p.text_weak),
            );
        });

        // ---- clipboard ----------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Copy, t.out_clipboard);
            ui::field_row(ui, p, t.out_clipboard, |ui| {
                let options = [
                    (config::ClipboardMode::Image, t.out_clip_image),
                    (config::ClipboardMode::File, t.out_clip_file),
                    (config::ClipboardMode::Disabled, t.out_clip_disabled),
                ];
                if let Some(mode) = ui::segmented(ui, p, self.config.clipboard_mode, &options) {
                    self.config.clipboard_mode = mode;
                    let _ = self.config.save();
                }
            });
            if self.config.clipboard_mode == config::ClipboardMode::Image {
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        egui::RichText::new(t.out_clip_image_hint)
                            .size(11.0)
                            .color(p.text_weak),
                    )
                    .wrap(),
                );
            }
        });

        // ---- upload -------------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Globe, t.up_section);
            ui::field_row(ui, p, t.up_enabled, |ui| {
                if ui::toggle(ui, p, &mut self.config.upload.enabled).changed() {
                    let _ = self.config.save();
                }
            });
            ui.add_space(4.0);
            ui::field_row(ui, p, t.up_backend, |ui| {
                let options = [
                    (config::UploadBackend::Http, t.up_http),
                    (config::UploadBackend::Local, t.up_local),
                ];
                if let Some(b) = ui::segmented(ui, p, self.config.upload.backend, &options) {
                    self.config.upload.backend = b;
                    let _ = self.config.save();
                }
            });
            match self.config.upload.backend {
                config::UploadBackend::Http => {
                    ui::field_row(ui, p, t.up_url, |ui| {
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.config.upload.http.url)
                                    .desired_width(280.0),
                            )
                            .changed()
                        {
                            let _ = self.config.save();
                        }
                    });
                    ui::field_row(ui, p, t.up_method, |ui| {
                        let options = [
                            (config::HttpMethod::Post, "POST"),
                            (config::HttpMethod::Put, "PUT"),
                        ];
                        if let Some(m) =
                            ui::segmented(ui, p, self.config.upload.http.method, &options)
                        {
                            self.config.upload.http.method = m;
                            let _ = self.config.save();
                        }
                    });
                    ui::field_row(ui, p, t.up_field, |ui| {
                        if ui
                            .add(
                                egui::TextEdit::singleline(&mut self.config.upload.http.field)
                                    .desired_width(160.0),
                            )
                            .changed()
                        {
                            let _ = self.config.save();
                        }
                    });
                    ui::field_row(ui, p, t.up_response, |ui| {
                        let options = [
                            (config::UploadResponse::Raw, t.up_resp_raw),
                            (config::UploadResponse::Json, t.up_resp_json),
                        ];
                        if let Some(r) =
                            ui::segmented(ui, p, self.config.upload.http.response, &options)
                        {
                            self.config.upload.http.response = r;
                            let _ = self.config.save();
                        }
                    });
                    if self.config.upload.http.response == config::UploadResponse::Json {
                        ui::field_row(ui, p, t.up_json_field, |ui| {
                            if ui
                                .add(
                                    egui::TextEdit::singleline(
                                        &mut self.config.upload.http.json_field,
                                    )
                                    .desired_width(160.0),
                                )
                                .changed()
                            {
                                let _ = self.config.save();
                            }
                        });
                    }
                }
                config::UploadBackend::Local => {
                    ui::field_row(ui, p, t.up_folder, |ui| {
                        if ui::link_button(ui, p, Icon::Folder, t.gif_choose).clicked() {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                self.config.upload.local.folder = dir;
                                let _ = self.config.save();
                            }
                        }
                        ui.label(
                            egui::RichText::new(
                                self.config.upload.local.folder.display().to_string(),
                            )
                            .monospace()
                            .size(11.0)
                            .color(p.text_weak),
                        );
                    });
                }
            }
            ui.add_space(4.0);
            ui.add(
                egui::Label::new(egui::RichText::new(t.up_hint).size(11.0).color(p.text_weak))
                    .wrap(),
            );
        });
    }

    /// Language, startup behaviour and where the config lives.
    fn general_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Settings, t.tab_general);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.language, |ui| {
                let mut picked = None;
                egui::ComboBox::from_id_salt("pinsight-lang")
                    .selected_text(self.config.lang.display_name(self.config.lang))
                    .width(168.0)
                    .show_ui(ui, |ui| {
                        for lang in Lang::ALL {
                            if ui
                                .selectable_label(
                                    self.config.lang == lang,
                                    lang.display_name(self.config.lang),
                                )
                                .clicked()
                            {
                                picked = Some(lang);
                            }
                        }
                    });
                if let Some(lang) = picked {
                    self.set_language(lang, &ctx);
                }
            });

            ui::field_row(ui, p, t.run_at_startup, |ui| {
                let mut on = self.config.run_at_startup;
                if ui::toggle(ui, p, &mut on).changed() {
                    self.config.run_at_startup = on;
                    if startup::set_enabled(on) {
                        let _ = self.config.save();
                        self.toast(t.gen_autostart_saved, false);
                    } else {
                        self.config.run_at_startup = !on;
                        self.toast(t.startup_failed, true);
                    }
                }
            });

            ui::field_row(ui, p, t.close_to_tray, |ui| {
                if ui::toggle(ui, p, &mut self.config.close_to_tray).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.gen_auto_backup, |ui| {
                if ui::toggle(ui, p, &mut self.config.auto_backup_config).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.gen_enhanced_tray, |ui| {
                if ui::toggle(ui, p, &mut self.config.enhanced_tray).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.tray_icon_style, |ui| {
                let options = [
                    (config::TrayIconStyle::Auto, t.tray_icon_auto),
                    (config::TrayIconStyle::White, t.tray_icon_white),
                    (config::TrayIconStyle::Color, t.tray_icon_color),
                    (config::TrayIconStyle::Custom, t.tray_icon_custom),
                ];
                if let Some(style) = ui::segmented(ui, p, self.config.tray_icon_style, &options) {
                    self.config.tray_icon_style = style;
                    let _ = self.config.save();
                    // Rebuild the icon in place: dropping the old tray removes
                    // the shell icon, installing again picks up the new art.
                    self.tray_reset();
                    self.install_tray(&ctx);
                }
            });

            if self.config.tray_icon_style == config::TrayIconStyle::Custom {
                ui::field_row(ui, p, "", |ui| {
                    if ui::link_button(ui, p, Icon::Image, t.tray_icon_custom).clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("PNG", &["png", "ico", "jpg", "jpeg"])
                            .pick_file()
                        {
                            self.config.tray_icon_path = Some(path);
                            let _ = self.config.save();
                            self.tray_reset();
                            self.install_tray(&ctx);
                        }
                    }
                });
            }

            ui::field_row(ui, p, t.gen_admin_restart, |ui| {
                if ui::link_button(ui, p, Icon::Settings, t.tray_restart).clicked() {
                    self.restart_elevated(&ctx);
                }
            });

            ui::field_row(ui, p, t.gen_log_level, |ui| {
                let label = |l: config::LogLevel| match l {
                    config::LogLevel::Error => t.log_error,
                    config::LogLevel::Warn => t.log_warn,
                    config::LogLevel::Info => t.log_info,
                    config::LogLevel::Debug => t.log_debug,
                    config::LogLevel::Trace => t.log_trace,
                };
                egui::ComboBox::from_id_salt("log-level")
                    .selected_text(label(self.config.log_level))
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for level in config::LogLevel::ALL {
                            if ui
                                .selectable_label(self.config.log_level == level, label(level))
                                .clicked()
                            {
                                self.config.log_level = level;
                                let _ = self.config.save();
                            }
                        }
                    });
            });

            ui::field_row(ui, p, t.gen_config, |ui| {
                let path = config::paths().config.clone();
                let shown = path
                    .parent()
                    .map(|d| d.display().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                // Right-to-left row: the action comes first so it lands on the
                // right, then the path fills the space to its left.
                if ui::link_button(ui, p, Icon::Folder, t.gen_open_folder).clicked() {
                    if let Some(dir) = path.parent() {
                        util::open_path(dir);
                    }
                }
                let mut job = egui::text::LayoutJob::simple(
                    shown,
                    egui::FontId::monospace(11.0),
                    p.text_weak,
                    330.0,
                );
                job.wrap.max_rows = 1;
                job.wrap.overflow_character = Some('…');
                ui.add(egui::Label::new(job).selectable(false));
            });
        });
    }

    /// Selection border, mask and overlay helpers.
    fn capture_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let click_label = |a: config::ClickAction| match a {
            config::ClickAction::Copy => t.click_copy,
            config::ClickAction::Pin => t.click_pin,
            config::ClickAction::Save => t.click_save,
            config::ClickAction::Nothing => t.click_nothing,
        };

        // ---- display ------------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Crop, t.sec_capture);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.cap_border, |ui| {
                let mut width = self.config.capture_border_width;
                if ui::slider(ui, p, &mut width, 1.0..=6.0, 1, " px").changed() {
                    self.config.capture_border_width = width;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_mask, |ui| {
                let mut alpha = self.config.capture_mask_alpha as f32;
                if ui::slider(ui, p, &mut alpha, 0.0..=220.0, 0, "").changed() {
                    self.config.capture_mask_alpha = alpha.round() as u8;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_mask_color, |ui| {
                let rgb = self.config.capture_mask_color;
                if ui::color_field(ui, p, rgb).clicked() {
                    self.open_color_picker(crate::picker::ColorTarget::CaptureMask);
                }
            });

            ui::field_row(ui, p, t.cap_anchors, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_anchors).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_anchor_color, |ui| {
                let rgb = self.config.capture_anchor_color;
                if ui::color_field(ui, p, rgb).clicked() {
                    self.open_color_picker(crate::picker::ColorTarget::CaptureAnchor);
                }
            });

            ui::field_row(ui, p, t.cap_crosshair, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_crosshair).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_hints, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_hints).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_magnifier, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_magnifier).changed() {
                    let _ = self.config.save();
                }
            });

            if self.config.capture_magnifier {
                ui::field_row(ui, p, t.cap_magnifier_size, |ui| {
                    let mut value = self.config.capture_magnifier_size as f32;
                    if ui::slider(ui, p, &mut value, 5.0..=31.0, 0, " px").changed() {
                        // The overlay renders an odd number of cells so the
                        // cursor always lands on a whole one.
                        self.config.capture_magnifier_size =
                            (value.round() as u32).clamp(5, 31) | 1;
                        let _ = self.config.save();
                    }
                });

                ui::field_row(ui, p, t.cap_color_format, |ui| {
                    let options = [
                        (config::ColorFormat::Hex, t.fmt_hex),
                        (config::ColorFormat::Rgb, t.fmt_rgb),
                        (config::ColorFormat::Hsl, t.fmt_hsl),
                    ];
                    if let Some(fmt) = ui::segmented(ui, p, self.config.color_format, &options) {
                        self.config.color_format = fmt;
                        let _ = self.config.save();
                    }
                });
            }

            ui::field_row(ui, p, t.cap_pointer, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_pointer).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(2.0);
            ui::field_row(ui, p, t.hdr_state, |ui| {
                let active = crate::hdr::any_hdr();
                ui.label(
                    egui::RichText::new(if active { t.hdr_detected } else { t.hdr_none })
                        .size(12.0)
                        .color(if active { p.warn } else { p.text_weak }),
                );
            });

            ui::field_row(ui, p, t.hdr_fix, |ui| {
                if ui::toggle(ui, p, &mut self.config.hdr_fix).changed() {
                    crate::capture::set_prefer_sdr_on_hdr(self.config.hdr_fix);
                    let _ = self.config.save();
                }
            });
            ui.add_space(2.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.hdr_fix_hint)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
        });

        // ---- behaviour ----------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Settings, t.cap_behavior);
            ui::field_row(ui, p, t.cap_history_limit, |ui| {
                let mut value = self.config.capture_history_limit as f32;
                if ui::slider(ui, p, &mut value, 1.0..=200.0, 0, "").changed() {
                    self.config.capture_history_limit = value.round() as u32;
                    let _ = self.config.save();
                }
            });
        });

        // ---- scrolling capture --------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Crop, t.long_settings);
            ui::field_row(ui, p, t.long_max_height, |ui| {
                let mut value = self.config.long_capture_max_height as f32;
                if ui::slider(ui, p, &mut value, 2000.0..=60000.0, 0, " px").changed() {
                    self.config.long_capture_max_height = (value.round() as u32).max(2000);
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_scroll_ticks, |ui| {
                let mut value = self.config.long_scroll_ticks as f32;
                if ui::slider(ui, p, &mut value, 1.0..=24.0, 0, "").changed() {
                    self.config.long_scroll_ticks = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_scroll_delay, |ui| {
                let mut value = self.config.long_scroll_delay_ms as f32;
                if ui::slider(ui, p, &mut value, 80.0..=2000.0, 0, " ms").changed() {
                    self.config.long_scroll_delay_ms = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_diff_threshold, |ui| {
                let mut value = self.config.long_diff_threshold;
                if ui::slider(ui, p, &mut value, 0.2..=12.0, 1, "").changed() {
                    self.config.long_diff_threshold = value;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_idle_limit, |ui| {
                let mut value = self.config.long_idle_limit as f32;
                if ui::slider(ui, p, &mut value, 1.0..=60.0, 0, "").changed() {
                    self.config.long_idle_limit = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_manual_idle, |ui| {
                let mut value = self.config.long_manual_idle_limit as f32;
                if ui::slider(ui, p, &mut value, 5.0..=300.0, 0, "").changed() {
                    self.config.long_manual_idle_limit = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.long_max_frames, |ui| {
                let mut value = self.config.long_max_frames as f32;
                if ui::slider(ui, p, &mut value, 10.0..=2000.0, 0, "").changed() {
                    self.config.long_max_frames = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui.add_space(4.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.long_settings_hint)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
        });

        // ---- keys ---------------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Keyboard, t.cap_keys);
            ui::field_row(ui, p, t.cap_delay, |ui| {
                let mut value = self.config.capture_delay_secs as f32;
                if ui::slider(ui, p, &mut value, 0.0..=30.0, 0, "s").changed() {
                    self.config.capture_delay_secs = value.round() as u32;
                    let _ = self.config.save();
                }
            });
            ui::field_row(ui, p, t.cap_esc_confirm, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_esc_confirm).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.cap_double_click, |ui| {
                egui::ComboBox::from_id_salt("cap-double")
                    .selected_text(click_label(self.config.capture_double_click))
                    .width(168.0)
                    .show_ui(ui, |ui| {
                        for action in config::ClickAction::ALL {
                            if ui
                                .selectable_label(
                                    self.config.capture_double_click == action,
                                    click_label(action),
                                )
                                .clicked()
                            {
                                self.config.capture_double_click = action;
                                let _ = self.config.save();
                            }
                        }
                    });
            });

            ui::field_row(ui, p, t.cap_middle_click, |ui| {
                egui::ComboBox::from_id_salt("cap-middle")
                    .selected_text(click_label(self.config.capture_middle_click))
                    .width(168.0)
                    .show_ui(ui, |ui| {
                        for action in config::ClickAction::ALL {
                            if ui
                                .selectable_label(
                                    self.config.capture_middle_click == action,
                                    click_label(action),
                                )
                                .clicked()
                            {
                                self.config.capture_middle_click = action;
                                let _ = self.config.save();
                            }
                        }
                    });
            });
        });

        // ---- sound --------------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Info, t.cap_sound);
            ui::field_row(ui, p, t.cap_sound_copy, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_sound_copy).changed() {
                    let _ = self.config.save();
                }
            });
            ui::field_row(ui, p, t.cap_sound_save, |ui| {
                if ui::toggle(ui, p, &mut self.config.capture_sound_save).changed() {
                    let _ = self.config.save();
                }
            });
            ui::field_row(ui, p, t.cap_sound_file, |ui| {
                if let Some(path) = &self.config.sound_file {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "file".into());
                    ui.label(name);
                    if ui::link_button(ui, p, Icon::Reset, t.reset).clicked() {
                        self.config.sound_file = None;
                        let _ = self.config.save();
                    }
                }
                if ui::link_button(ui, p, Icon::Folder, t.cap_sound_browse).clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Audio", &["wav", "mp3", "ogg", "m4a", "flac"])
                        .pick_file()
                    {
                        self.config.sound_file = Some(path);
                        let _ = self.config.save();
                    }
                }
            });
        });
    }

    /// Pin window defaults.
    fn pins_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::section_header(ui, p, Icon::Pin, t.sec_pins);
        ui.add_space(10.0);
        // Sub-page switch, mirroring Snipaste: display / behavior / convert.
        let subs: [(PinsSub, &str); 3] = [
            (PinsSub::Display, t.pin_sub_display),
            (PinsSub::Behavior, t.pin_sub_behavior),
            (PinsSub::Convert, t.pin_sub_convert),
        ];
        if let Some(sub) = ui::segmented(ui, p, self.pins_sub, &subs) {
            self.pins_sub = sub;
        }
        ui.add_space(12.0);
        match self.pins_sub {
            PinsSub::Display => self.pins_display_tab(ui, p),
            PinsSub::Behavior => self.pins_behavior_tab(ui, p),
            PinsSub::Convert => self.pins_convert_tab(ui, p),
        }
    }

    /// How a pin looks: shadow, opacity, size and thumbnail size.
    fn pins_display_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Layers, t.pin_sub_display);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.pin_shadow, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_shadow).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_shadow_color, |ui| {
                let rgb = self.config.pin_shadow_color;
                if ui::color_field(ui, p, rgb).clicked() {
                    self.open_color_picker(crate::picker::ColorTarget::PinShadow);
                }
            });

            ui::field_row(ui, p, t.pin_opacity, |ui| {
                let mut value = self.config.pin_opacity as f32;
                if ui::slider(ui, p, &mut value, 20.0..=100.0, 0, " %").changed() {
                    self.config.pin_opacity = value.round() as u8;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_max_size, |ui| {
                let mut value = self.config.pin_max_size as f32;
                if ui::slider(ui, p, &mut value, 320.0..=8000.0, 0, " px").changed() {
                    self.config.pin_max_size = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_thumb, |ui| {
                let mut value = self.config.pin_thumb as f32;
                if ui::slider(ui, p, &mut value, 40.0..=160.0, 0, " px").changed() {
                    self.config.pin_thumb = value.round() as u32;
                    let _ = self.config.save();
                }
            });
        });
    }

    /// How a pin reacts to the mouse.
    fn pins_behavior_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Settings, t.pin_sub_behavior);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.pin_wheel_zoom, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_wheel_zoom).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_zoom_at_cursor, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_zoom_at_cursor).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_smooth_zoom, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_smooth_zoom).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_middle_close, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_middle_close).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_restore_limit, |ui| {
                let mut value = self.config.pin_restore_limit as f32;
                if ui::slider(ui, p, &mut value, 0.0..=50.0, 0, "").changed() {
                    self.config.pin_restore_limit = value.round() as u32;
                    // Shrinking the ring drops the oldest entries immediately.
                    while self.closed_pins.len() > self.config.pin_restore_limit as usize {
                        self.closed_pins.pop_front();
                    }
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_keep_position, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_keep_position).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_through, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_through).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.pin_behavior_note)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
        });
    }

    /// How text is turned into an image when it is pinned.
    fn pins_convert_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Type, t.pin_sub_convert);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.pin_text_to_image, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_text_to_image).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_text_margin, |ui| {
                let mut value = self.config.pin_text_margin as f32;
                if ui::slider(ui, p, &mut value, 0.0..=64.0, 0, " px").changed() {
                    self.config.pin_text_margin = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_text_max_width, |ui| {
                let mut value = self.config.pin_text_max_width as f32;
                if ui::slider(ui, p, &mut value, 240.0..=2000.0, 0, " px").changed() {
                    self.config.pin_text_max_width = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_text_font_size, |ui| {
                let mut value = self.config.pin_text_font_size as f32;
                if ui::slider(ui, p, &mut value, 8.0..=96.0, 0, " px").changed() {
                    self.config.pin_text_font_size = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.pin_text_bg, |ui| {
                let rgb = self.config.pin_text_bg;
                if ui::color_field(ui, p, rgb).clicked() {
                    self.open_color_picker(crate::picker::ColorTarget::PinTextBg);
                }
            });

            ui::field_row(ui, p, t.pin_text_fg, |ui| {
                let rgb = self.config.pin_text_fg;
                if ui::color_field(ui, p, rgb).clicked() {
                    self.open_color_picker(crate::picker::ColorTarget::PinTextFg);
                }
            });

            ui::field_row(ui, p, t.pin_skip_file_paths, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_skip_file_paths).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.pin_text_to_image_hint)
                        .size(11.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
        });
    }

    /// Global hotkeys plus the in-app (overlay) shortcuts.
    fn controls_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::section_header(ui, p, Icon::Keyboard, t.sec_controls);
        ui.add_space(10.0);
        // Sub-page switch, mirroring Snipaste: global hotkeys / in-app hotkeys /
        // mouse behaviour.
        let subs: [(ControlsSub, &str); 3] = [
            (ControlsSub::Global, t.ctl_sub_global),
            (ControlsSub::AppKeys, t.ctl_sub_app),
            (ControlsSub::Mouse, t.ctl_sub_mouse),
        ];
        if let Some(sub) = ui::segmented(ui, p, self.controls_sub, &subs) {
            self.controls_sub = sub;
        }
        ui.add_space(12.0);
        match self.controls_sub {
            ControlsSub::Global => self.hotkeys_tab(ui, p),
            ControlsSub::AppKeys => self.app_keys_card(ui, p),
            ControlsSub::Mouse => self.mouse_card(ui, p),
        }
    }

    /// Mouse-behaviour settings for pins.
    fn mouse_card(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Cursor, t.ctl_mouse);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t.ctl_mouse_hint)
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.add_space(10.0);

            ui::field_row(ui, p, t.ctl_mouse_through, |ui| {
                if ui::toggle(ui, p, &mut self.config.pin_through).changed() {
                    let _ = self.config.save();
                }
            });
        });

        // ---- bindings -----------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Keyboard, t.ctl_gestures);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t.ctl_gestures_hint)
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.add_space(10.0);

            // One row per action: label, modifier, button, double-click flag.
            #[allow(clippy::type_complexity)]
            let rows: [(&str, fn(&mut config::PinGestures) -> &mut config::Gesture);
                6] = [
                (t.gst_zoom, |g| &mut g.zoom),
                (t.gst_opacity, |g| &mut g.opacity),
                (t.gst_close, |g| &mut g.close),
                (t.gst_reset, |g| &mut g.reset),
                (t.gst_thumbnail, |g| &mut g.thumbnail),
                (t.gst_copy_text, |g| &mut g.copy_text),
            ];
            for (label, pick) in rows {
                ui::field_row(ui, p, label, |ui| {
                    let gesture = *pick(&mut self.config.pin_gestures);
                    let mut next = gesture;

                    let mod_label = |m: config::GestureMod| -> &'static str {
                        match m {
                            config::GestureMod::None => t.gst_none,
                            config::GestureMod::Ctrl => t.gst_ctrl,
                            config::GestureMod::Shift => t.gst_shift,
                            config::GestureMod::Alt => t.gst_alt,
                        }
                    };
                    let mut mod_picked = None;
                    egui::ComboBox::from_id_salt(("pinsight-gst-mod", label))
                        .selected_text(mod_label(gesture.modifier))
                        .width(78.0)
                        .show_ui(ui, |ui| {
                            for m in config::GestureMod::ALL {
                                if ui
                                    .selectable_label(m == gesture.modifier, mod_label(m))
                                    .clicked()
                                {
                                    mod_picked = Some(m);
                                }
                            }
                        });
                    if let Some(m) = mod_picked {
                        next.modifier = m;
                    }

                    ui.add_space(4.0);
                    ui.label(egui::RichText::new("+").color(p.text_weak));
                    ui.add_space(4.0);

                    let btn_label = |b: config::GestureButton| -> &'static str {
                        match b {
                            config::GestureButton::Wheel => t.gst_wheel,
                            config::GestureButton::Left => t.gst_left,
                            config::GestureButton::Middle => t.gst_middle,
                            config::GestureButton::Right => t.gst_right,
                        }
                    };
                    let mut btn_picked = None;
                    egui::ComboBox::from_id_salt(("pinsight-gst-btn", label))
                        .selected_text(btn_label(gesture.button))
                        .width(82.0)
                        .show_ui(ui, |ui| {
                            for b in config::GestureButton::ALL {
                                if ui
                                    .selectable_label(b == gesture.button, btn_label(b))
                                    .clicked()
                                {
                                    btn_picked = Some(b);
                                }
                            }
                        });
                    if let Some(b) = btn_picked {
                        next.button = b;
                    }

                    // A wheel gesture has no "double click", so the flag only
                    // makes sense for the three buttons.
                    if next.button != config::GestureButton::Wheel {
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(t.gst_double)
                                .size(11.5)
                                .color(p.text_weak),
                        );
                        if ui::toggle(ui, p, &mut next.double_click).changed() {}
                    }

                    if next != gesture {
                        *pick(&mut self.config.pin_gestures) = next;
                        let _ = self.config.save();
                    }
                });
            }

            ui.add_space(6.0);
            if ui::link_button(ui, p, Icon::Reset, t.reset).clicked() {
                self.config.pin_gestures = config::PinGestures::default();
                let _ = self.config.save();
                // Bindings are read when a pin is created, so live pins keep
                // the old table; say so rather than silently doing nothing.
                self.toast(t.gestures_new_pins_only, false);
            }
            let _ = ctx;
        });
    }

    /// The 16 overlay shortcuts, each with a record button.
    fn app_keys_card(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Keyboard, t.ctl_app_keys);
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(t.ctl_app_keys_hint)
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.add_space(10.0);

            let labels: [&str; 16] = [
                t.tool_select,
                t.tool_rect,
                t.tool_ellipse,
                t.tool_arrow,
                t.tool_pen,
                t.tool_highlight,
                t.tool_mosaic,
                t.tool_numbered,
                t.tool_text,
                t.key_confirm,
                t.key_pin,
                t.key_copy,
                t.key_save,
                t.key_cancel,
                t.key_undo,
                t.key_redo,
            ];
            let values: Vec<String> = self
                .config
                .app_keys
                .all()
                .iter()
                .map(|(_, v)| (*v).to_string())
                .collect();
            // Which bindings appear more than once: those rows are flagged red.
            let mut dup = [false; 16];
            for i in 0..16 {
                for j in (i + 1)..16 {
                    if values[i].eq_ignore_ascii_case(&values[j]) {
                        dup[i] = true;
                        dup[j] = true;
                    }
                }
            }

            let mut commit: Option<(usize, String)> = None;
            for (i, label) in labels.iter().enumerate() {
                ui::field_row(ui, p, label, |ui| {
                    let recording = self.recording_app_key == Some(i);
                    let text = if recording {
                        t.hk_press_prompt.to_string()
                    } else {
                        values[i].clone()
                    };
                    let mut colour = p.text;
                    if dup[i] {
                        colour = p.danger;
                    }
                    ui.label(egui::RichText::new(text).size(13.0).color(colour));
                    if dup[i] {
                        ui.label(egui::RichText::new(t.ctl_dup).size(11.0).color(p.danger));
                    }
                    let button = if recording {
                        ui::link_button(ui, p, Icon::Close, t.key_cancel)
                    } else {
                        ui::link_button(ui, p, Icon::Keyboard, t.hk_record)
                    };
                    if button.clicked() {
                        self.recording_app_key = if recording { None } else { Some(i) };
                    }
                });
                ui.add_space(4.0);
            }

            // One key event per frame is enough: the recorder only lives while
            // the user is looking at it.
            if let Some(index) = self.recording_app_key {
                let mut captured = None;
                ui.input(|input| {
                    for event in &input.events {
                        if let egui::Event::Key {
                            key,
                            pressed: true,
                            modifiers,
                            ..
                        } = event
                        {
                            if *key == egui::Key::Escape {
                                captured = Some(None);
                            } else if !matches!(
                                key,
                                egui::Key::ControlLeft
                                    | egui::Key::ControlRight
                                    | egui::Key::ShiftLeft
                                    | egui::Key::ShiftRight
                                    | egui::Key::AltLeft
                                    | egui::Key::AltRight
                            ) {
                                captured = Some(Some(crate::app_keys::format(*key, modifiers)));
                            }
                        }
                    }
                });
                match captured {
                    Some(None) => {
                        self.recording_app_key = None;
                        self.toast(t.hk_cancelled, false);
                    }
                    Some(Some(text)) => {
                        self.recording_app_key = None;
                        commit = Some((index, text));
                    }
                    None => {}
                }
            }

            if let Some((index, text)) = commit {
                let duplicate = self
                    .config
                    .app_keys
                    .all()
                    .iter()
                    .enumerate()
                    .any(|(i, (_, v))| i != index && v.eq_ignore_ascii_case(&text));
                if duplicate {
                    self.toast(t.hk_duplicate, true);
                } else {
                    let slot = match index {
                        0 => &mut self.config.app_keys.tool_select,
                        1 => &mut self.config.app_keys.tool_rect,
                        2 => &mut self.config.app_keys.tool_ellipse,
                        3 => &mut self.config.app_keys.tool_arrow,
                        4 => &mut self.config.app_keys.tool_pen,
                        5 => &mut self.config.app_keys.tool_highlight,
                        6 => &mut self.config.app_keys.tool_mosaic,
                        7 => &mut self.config.app_keys.tool_numbered,
                        8 => &mut self.config.app_keys.tool_text,
                        9 => &mut self.config.app_keys.confirm,
                        10 => &mut self.config.app_keys.pin,
                        11 => &mut self.config.app_keys.copy,
                        12 => &mut self.config.app_keys.save,
                        13 => &mut self.config.app_keys.cancel,
                        14 => &mut self.config.app_keys.undo,
                        _ => &mut self.config.app_keys.redo,
                    };
                    *slot = text;
                    let _ = self.config.save();
                }
            }
        });
    }

    /// Update checks. There is no update server of our own, so "check now"
    /// opens the releases page instead of pretending to poll an endpoint.
    fn updates_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Reset, t.sec_updates);
            ui.add_space(8.0);

            ui::field_row(ui, p, t.upd_on_start, |ui| {
                if ui::toggle(ui, p, &mut self.config.update_on_start).changed() {
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.upd_hours, |ui| {
                let mut value = self.config.update_hours as f32;
                if ui::slider(ui, p, &mut value, 1.0..=168.0, 0, " h").changed() {
                    self.config.update_hours = value.round() as u32;
                    let _ = self.config.save();
                }
            });

            ui::field_row(ui, p, t.upd_beta, |ui| {
                if ui::toggle(ui, p, &mut self.config.update_beta).changed() {
                    let _ = self.config.save();
                }
            });

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new(t.upd_note)
                    .size(12.0)
                    .color(p.text_weak),
            );
            ui.add_space(8.0);
            if ui::link_button(ui, p, Icon::Reset, t.upd_check_now).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(
                    "https://github.com/xiu-k/pinsight/releases",
                ));
            }
        });

        // ---- update proxy -------------------------------------------------
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Globe, t.upd_proxy);
            ui::field_row(ui, p, t.upd_proxy_mode, |ui| {
                let modes = [
                    config::ProxyMode::System,
                    config::ProxyMode::None,
                    config::ProxyMode::Http,
                    config::ProxyMode::Socks5,
                    config::ProxyMode::Manual,
                ];
                let label = |m: config::ProxyMode| -> &'static str {
                    match m {
                        config::ProxyMode::System => t.upd_proxy_system,
                        config::ProxyMode::None => t.upd_proxy_none,
                        config::ProxyMode::Http => "HTTP",
                        config::ProxyMode::Socks5 => "SOCKS5",
                        config::ProxyMode::Manual => t.upd_proxy_manual,
                    }
                };
                let mut picked = None;
                egui::ComboBox::from_id_salt("pinsight-proxy-mode")
                    .selected_text(label(self.config.proxy_mode))
                    .width(160.0)
                    .show_ui(ui, |ui| {
                        for mode in modes {
                            if ui
                                .selectable_label(mode == self.config.proxy_mode, label(mode))
                                .clicked()
                            {
                                picked = Some(mode);
                            }
                        }
                    });
                if let Some(mode) = picked {
                    self.config.proxy_mode = mode;
                    let _ = self.config.save();
                }
            });

            if matches!(
                self.config.proxy_mode,
                config::ProxyMode::Manual | config::ProxyMode::Http | config::ProxyMode::Socks5
            ) {
                ui::field_row(ui, p, t.upd_proxy_addr, |ui| {
                    let width = (ui.available_width() - 40.0).clamp(160.0, 460.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.proxy_server)
                            .desired_width(width)
                            .font(egui::TextStyle::Monospace),
                    );
                });

                ui::field_row(ui, p, t.upd_proxy_port, |ui| {
                    let mut value = self.config.proxy_port as f32;
                    if ui::slider(ui, p, &mut value, 0.0..=65535.0, 0, "").changed() {
                        self.config.proxy_port = value.round() as u16;
                        let _ = self.config.save();
                    }
                });

                ui::field_row(ui, p, t.upd_proxy_user, |ui| {
                    let width = (ui.available_width() - 40.0).clamp(160.0, 460.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.proxy_user)
                            .desired_width(width),
                    );
                });

                ui::field_row(ui, p, t.upd_proxy_pass, |ui| {
                    let width = (ui.available_width() - 40.0).clamp(160.0, 460.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.config.proxy_password)
                            .password(true)
                            .desired_width(width),
                    );
                });
            }

            ui.add_space(12.0);
            ui.add(
                egui::Label::new(
                    egui::RichText::new(t.upd_proxy_note)
                        .size(12.0)
                        .color(p.text_weak),
                )
                .wrap(),
            );
        });
    }

    /// Hotkey rows. Each row can be typed into or recorded by pressing the
    /// combination, which is what most users reach for first.
    fn hotkeys_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        let ctx = ui.ctx().clone();
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Keyboard, t.sec_hotkeys);
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(t.hotkeys_hint)
                    .size(11.0)
                    .color(p.text_weak),
            );
            if !self.hotkeys.failures.is_empty() {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t.hotkeys_conflict_hint)
                        .size(11.0)
                        .color(p.danger),
                );
            }
            ui.add_space(8.0);

            let status = [
                self.hotkeys.failure_for(Action::CaptureArea),
                self.hotkeys.failure_for(Action::CaptureFull),
                self.hotkeys.failure_for(Action::PinClipboard),
                self.hotkeys.failure_for(Action::LongCapture),
                self.hotkeys.failure_for(Action::Record),
                self.hotkeys.failure_for(Action::SaveLast),
            ];
            let fields: [(&mut String, &str); 6] = [
                (&mut self.hotkey_draft.capture_area, t.hk_capture_area),
                (&mut self.hotkey_draft.capture_full, t.hk_capture_full),
                (&mut self.hotkey_draft.pin_clipboard, t.hk_pin_clipboard),
                (&mut self.hotkey_draft.long_capture, t.hk_long_capture),
                (&mut self.hotkey_draft.record, t.hk_record),
                (&mut self.hotkey_draft.save_last, t.hk_save_last),
            ];

            for (i, (value, label)) in fields.into_iter().enumerate() {
                let failure = status[i];
                let recording = self.recording_hotkey == Some(i);
                ui::field_row(ui, p, label, |ui| {
                    if recording {
                        // Show a live highlight instead of the field so the
                        // pressed combination never lands in the text box.
                        let (rect, response) =
                            ui.allocate_exact_size(egui::vec2(150.0, 22.0), egui::Sense::click());
                        let _ = response;
                        ui.painter().rect_filled(rect, 6.0, p.accent_soft);
                        ui.painter().rect_stroke(
                            rect,
                            6.0,
                            egui::Stroke::new(1.5, p.accent),
                            egui::StrokeKind::Inside,
                        );
                        let galley = ui.painter().layout_no_wrap(
                            t.hk_press_prompt.to_owned(),
                            egui::FontId::proportional(11.0),
                            p.accent,
                        );
                        ui.painter().galley(
                            egui::pos2(
                                rect.center().x - galley.size().x * 0.5,
                                rect.center().y - galley.size().y * 0.5,
                            ),
                            galley,
                            p.accent,
                        );
                    } else {
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .desired_width(140.0)
                                .font(egui::TextStyle::Monospace),
                        );
                    }

                    ui.add_space(4.0);
                    // A dot tells at a glance whether the binding is live.
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    let color = if failure.is_some() {
                        p.danger
                    } else {
                        p.success
                    };
                    ui.painter().circle_filled(dot.center(), 3.5, color);

                    let record = ui::link_button(ui, p, Icon::Keyboard, t.hk_press);
                    if record.clicked() {
                        self.recording_hotkey = Some(i);
                    }

                    if let Some(kind) = failure {
                        let reason = match kind {
                            BindingError::InUse => t.hotkey_in_use,
                            BindingError::Invalid => t.hotkey_invalid,
                            BindingError::Backend => t.hotkey_warning,
                        };
                        ui.label(egui::RichText::new(reason).size(11.0).color(p.danger));
                    }
                });
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui::link_button(ui, p, Icon::Check, t.apply).clicked() {
                    self.config.hotkeys = self.hotkey_draft.clone();
                    let _ = self.config.save();
                    self.hotkeys.rebind(&self.config.hotkeys);
                    self.recording_hotkey = None;
                    if self.hotkeys.failures.is_empty() {
                        self.toast(self.tr.tr.hotkeys_applied, false);
                    } else {
                        let msg = self.hotkey_failure_message();
                        self.toast(msg, true);
                    }
                }
                if ui::link_button(ui, p, Icon::Reset, t.reset).clicked() {
                    self.hotkey_draft = HotkeyConfig::default();
                    self.config.hotkeys = self.hotkey_draft.clone();
                    let _ = self.config.save();
                    self.hotkeys.rebind(&self.config.hotkeys);
                    self.recording_hotkey = None;
                }
            });

            // Recording is handled here, after the widgets, so the press that
            // armed it is not read back as the new combination.
            if let Some(index) = self.recording_hotkey {
                self.capture_hotkey(ui, index);
            }
            let _ = ctx;
        });
    }

    /// Read one key press and store it as the draft binding for `index`.
    fn capture_hotkey(&mut self, ui: &egui::Ui, index: usize) {
        /// What the recorder saw: nothing, a cancelled session, a key with no
        /// modifier (a mistake worth explaining), or a usable binding.
        enum Outcome {
            Nothing,
            Cancelled,
            Rejected,
            Bound(String),
        }
        let mut outcome = Outcome::Nothing;
        ui.input(|input| {
            for event in &input.events {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = event
                else {
                    continue;
                };
                if *key == egui::Key::Escape {
                    outcome = Outcome::Cancelled;
                } else if !hotkeys::is_modifier(*key) {
                    outcome = match hotkeys::format_hotkey(modifiers, *key) {
                        Some(text) => Outcome::Bound(text),
                        None => Outcome::Rejected,
                    };
                }
            }
        });

        let text = match outcome {
            Outcome::Nothing => return,
            // Esc backs out quietly — an error toast for "I changed my mind"
            // reads like something went wrong.
            Outcome::Cancelled => {
                self.recording_hotkey = None;
                self.toast(self.tr.tr.hk_cancelled, false);
                return;
            }
            Outcome::Rejected => {
                self.recording_hotkey = None;
                self.toast(self.tr.tr.hk_rejected, true);
                return;
            }
            Outcome::Bound(text) => text,
        };
        self.recording_hotkey = None;
        // Refuse a combination another action already owns: both would be
        // registered, the second would fail, and the settings page would show
        // a phantom "already in use" conflict.
        let duplicate = self
            .hotkey_draft
            .all()
            .iter()
            .enumerate()
            .any(|(i, binding)| i != index && binding.eq_ignore_ascii_case(&text));
        if duplicate {
            self.toast(self.tr.tr.hk_duplicate, true);
            return;
        }
        let field = match index {
            0 => &mut self.hotkey_draft.capture_area,
            1 => &mut self.hotkey_draft.capture_full,
            2 => &mut self.hotkey_draft.pin_clipboard,
            3 => &mut self.hotkey_draft.long_capture,
            4 => &mut self.hotkey_draft.record,
            _ => &mut self.hotkey_draft.save_last,
        };
        *field = text;
        // Apply straight away: recording a key and then hunting for an Apply
        // button is two steps where one is expected.
        self.config.hotkeys = self.hotkey_draft.clone();
        let _ = self.config.save();
        self.hotkeys.rebind(&self.config.hotkeys);
        if self.hotkeys.failures.is_empty() {
            self.toast(self.tr.tr.hotkeys_applied, false);
        } else {
            let msg = self.hotkey_failure_message();
            self.toast(msg, true);
        }
    }

    /// Build, version and runtime status.
    fn about_tab(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let t = self.tr.tr;
        ui::card(ui, p, |ui| {
            ui::section_header(ui, p, Icon::Info, t.sec_about);
            ui.add_space(8.0);
            let displays = capture::monitors()
                .map(|monitors| {
                    monitors
                        .iter()
                        .map(|m| {
                            format!(
                                "{} {}×{}{}",
                                m.name,
                                m.rect.w,
                                m.rect.h,
                                if m.primary { " ★" } else { "" }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("   ")
                })
                .unwrap_or_else(|e| format!("— ({e})"));
            let rows: [(&str, String); 7] = [
                (t.version, env!("CARGO_PKG_VERSION").to_string()),
                (
                    t.mcp_status,
                    if self.mcp_handle.is_some() {
                        t.mcp_desc_on.to_string()
                    } else {
                        t.mcp_desc_off.to_string()
                    },
                ),
                (t.displays, displays),
                (
                    t.record_format,
                    if self.ffmpeg {
                        t.record_mp4.to_string()
                    } else {
                        t.record_gif.to_string()
                    },
                ),
                (
                    t.sec_hotkeys,
                    format!(
                        "{} / {} {}",
                        self.hotkeys.registered(),
                        6,
                        t.hotkeys_registered
                    ),
                ),
                (
                    t.portable_mode,
                    if config::is_portable() {
                        t.portable_yes.to_string()
                    } else {
                        t.portable_installed.to_string()
                    },
                ),
                (t.overlay_help, String::new()),
            ];
            for (label, value) in rows {
                if value.is_empty() {
                    ui.label(egui::RichText::new(label).size(11.0).color(p.text_weak));
                    continue;
                }
                ui::field_row(ui, p, label, |ui| {
                    ui.label(egui::RichText::new(value).size(11.5).color(p.text_weak));
                });
            }
        });
    }
    fn status_bar(&mut self, ui: &mut egui::Ui, p: &Palette) {
        ui.horizontal(|ui| {
            if let Some(toast) = &self.toast {
                if toast.at.elapsed() < TOAST_LIFETIME {
                    let color = if toast.error { p.danger } else { p.success };
                    let (dot, _) =
                        ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                    ui.painter().circle_filled(dot.center(), 3.0, color);
                    ui.label(egui::RichText::new(&toast.text).size(11.5).color(color));
                    ui.ctx().request_repaint_after(Duration::from_millis(400));
                }
            } else if !self.hotkeys.failures.is_empty() {
                let msg = self.hotkey_failure_message();
                ui.label(egui::RichText::new(msg).size(11.0).color(p.warn));
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(self.tr.tr.tagline)
                        .size(10.5)
                        .color(p.text_weak),
                );
            });
        });
    }

    /// Upload (and downscale) an image once, then reuse the texture.
    fn texture_for(
        &mut self,
        ctx: &egui::Context,
        key: &str,
        image: &RgbaImage,
    ) -> egui::TextureHandle {
        if let Some(texture) = self.textures.get(key) {
            return texture.clone();
        }
        const MAX: u32 = 420;
        let (w, h) = (image.width().max(1), image.height().max(1));
        let scale = (MAX as f32 / w as f32).min(MAX as f32 / h as f32).min(1.0);
        let thumb = if scale < 1.0 {
            image::imageops::thumbnail(
                image,
                (w as f32 * scale).max(1.0) as u32,
                (h as f32 * scale).max(1.0) as u32,
            )
        } else {
            image.clone()
        };
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [thumb.width() as usize, thumb.height() as usize],
            thumb.as_raw(),
        );
        let texture = ctx.load_texture(key.to_owned(), color, egui::TextureOptions::LINEAR);
        self.textures.insert(key.to_owned(), texture.clone());
        texture
    }
}

#[derive(Clone, Copy)]
enum TileAction {
    CaptureArea,
    CaptureFull,
    PinClipboard,
    LongShot,
    Record,
    SaveLast,
}

#[derive(Clone, Copy)]
enum PinAction {
    Focus,
    Through,
    Copy,
    Close,
    Group,
}

impl eframe::App for PinSightApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.poll_jobs(&ctx);
        self.process_ui_commands(&ctx);
        self.process_pending(&ctx);
        self.handle_hotkeys(&ctx);
        self.poll_pin_events();
        self.poll_upload();
        self.poll_gif();
        self.poll_voice();
        self.show_overlay(&ctx);
        self.show_pins(&ctx);
        self.sync_background(&ctx);
        self.poll_tray(&ctx);
        #[cfg(windows)]
        self.sync_tray_menu();
        self.handle_close_request(&ctx);

        // Startup is allocation-heavy (fonts, icon atlas, GL warm-up); the
        // first trim runs once that has settled. After that the working set is
        // re-trimmed on a slow cadence: the UI fonts are memory-mapped, so
        // glyph pages fault back in during use and would otherwise accumulate
        // again, leaving an idle tray process sitting on tens of MB. A 45 s
        // cadence keeps it lean at the cost of one cheap wake-up.
        {
            let interval = std::time::Duration::from_secs(45);
            let startup_settle = std::time::Duration::from_secs(8);
            let elapsed = self.started.elapsed();
            if elapsed < startup_settle {
                // Keep the loop alive until the first trim is due.
                ctx.request_repaint_after(startup_settle - elapsed);
            }
            let since_trim = self.last_trim.elapsed();
            if elapsed >= startup_settle && since_trim >= interval {
                self.last_trim = std::time::Instant::now();
                util::trim_working_set();
            } else if elapsed >= startup_settle {
                let wait = interval - since_trim;
                ctx.request_repaint_after(wait.max(std::time::Duration::from_millis(250)));
            }
        }

        let p = Palette::current(&ctx, self.config.accent, self.config.accent_custom);

        // A wallpaper goes down first so every panel below paints over it.
        let glass = self.has_background();
        if glass {
            self.paint_background(ui, &p);
        }
        let rail_fill = if glass {
            theme::with_alpha(p.rail, 232)
        } else {
            p.rail
        };

        egui::Panel::left("pinsight-nav")
            .exact_size(92.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(rail_fill)
                    .inner_margin(egui::Margin::symmetric(8, 12)),
            )
            .show(ui, |ui| self.nav_rail(ui, &p));

        egui::Panel::bottom("pinsight-status")
            .exact_size(26.0)
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(rail_fill)
                    .inner_margin(egui::Margin::symmetric(14, 3)),
            )
            .show(ui, |ui| self.status_bar(ui, &p));

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(if glass {
                        egui::Color32::TRANSPARENT
                    } else {
                        p.bg
                    })
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show(ui, |ui| {
                // Cap the reading column and centre it. Without the cap a
                // maximised window stretches every row into a thin ribbon:
                // label pinned to the far left, one switch pinned to the far
                // right, 900 px of nothing in between.
                let width = ui.available_width().min(720.0);
                let height = ui.available_height();
                ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(width, height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| match self.page {
                            Page::Capture => self.capture_page(ui, &p),
                            Page::History => self.history_page(ui, &p),
                            Page::Pins => self.pins_page(ui, &p),
                            Page::Settings => self.settings_page(ui, &p),
                            Page::Gif => self.gif_page(ui, &p),
                        },
                    );
                });
            });

        self.show_color_picker(&ctx, &p);
        self.show_job_huds(&ctx, &p);
        self.show_window_picker(&ctx, &p);

        if let Some(toast) = &self.toast {
            if toast.at.elapsed() > TOAST_LIFETIME {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(400));
            }
        }
    }
}

/// Whether the taskbar is drawn dark, i.e. whether a white tray icon reads
/// better than the colour one. Mirrors what Explorer itself asks for.
#[cfg(windows)]
fn taskbar_is_dark() -> bool {
    use windows::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_READ, REG_DWORD, RegCloseKey, RegOpenKeyExW, RegQueryValueExW,
    };
    use windows::core::HSTRING;

    let path = HSTRING::from(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let mut key = HKEY::default();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, &path, Some(0), KEY_READ, &mut key).is_err() {
            return true;
        }
        let name = HSTRING::from("SystemUsesLightTheme");
        let mut value = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        let mut kind = REG_DWORD;
        let status = RegQueryValueExW(
            key,
            &name,
            None,
            Some(&mut kind),
            Some(&mut value as *mut u32 as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        // Missing value: assume dark, the Windows default.
        status.is_err() || value == 0
    }
}

/// Localised description for one `$date$` code in the naming panel.
fn naming_code_desc(code: &str, t: &crate::i18n::Strings) -> &'static str {
    match code {
        "yyyy" => t.nm_yyyy,
        "yy" => t.nm_yy,
        "MMMM" => t.nm_MMMM,
        "MMM" => t.nm_MMM,
        "MM" => t.nm_MM,
        "M" => t.nm_M,
        "dddd" => t.nm_dddd,
        "ddd" => t.nm_ddd,
        "dd" => t.nm_dd,
        "d" => t.nm_d,
        "HH" => t.nm_HH,
        "H" => t.nm_H,
        "hh" => t.nm_hh,
        "h" => t.nm_h,
        "mm" => t.nm_mm,
        "m" => t.nm_m,
        "ss" => t.nm_ss,
        "s" => t.nm_s,
        "SSS" => t.nm_SSS,
        "A" => t.nm_A,
        _ => "",
    }
}

/// Localised description for one `%variable%`.
fn naming_var_desc(name: &str, t: &crate::i18n::Strings) -> &'static str {
    match name {
        "os" => t.nm_os,
        "computername" => t.nm_computername,
        "username" => t.nm_username,
        "userprofile" => t.nm_userprofile,
        "title" => t.nm_title,
        "title:50" => t.nm_title_cap,
        _ => "",
    }
}

fn non_empty_path(text: &str) -> Option<PathBuf> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn load_image(path: Option<&std::path::Path>, base64: Option<&str>) -> anyhow::Result<RgbaImage> {
    if let Some(path) = path {
        return Ok(image::open(path)?.to_rgba8());
    }
    let Some(data) = base64 else {
        anyhow::bail!("no image data supplied");
    };
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|e| anyhow::anyhow!("invalid base64: {e}"))?;
    Ok(image::load_from_memory(&bytes)?.to_rgba8())
}

fn copy_to_clipboard(config: &Config, image: &RgbaImage) -> anyhow::Result<()> {
    let result = crate::clipboard::copy_capture(config, image);
    if result.is_ok() && config.capture_sound_copy {
        util::play_notification(config.sound_file.as_deref());
    }
    result
}

fn clipboard_image() -> anyhow::Result<RgbaImage> {
    let mut clipboard = arboard::Clipboard::new()?;
    let data = clipboard.get_image()?;
    RgbaImage::from_raw(
        data.width as u32,
        data.height as u32,
        data.bytes.into_owned(),
    )
    .ok_or_else(|| anyhow::anyhow!("clipboard image has an invalid buffer size"))
}

/// Plain text on the clipboard, if any.
fn clipboard_text() -> Option<String> {
    let mut clipboard = arboard::Clipboard::new().ok()?;
    clipboard.get_text().ok()
}

/// Heuristic: the clipboard holds one line that names an existing file.
fn looks_like_file_path(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty()
        && !trimmed.contains('\n')
        && trimmed.len() <= 260
        && std::path::Path::new(trimmed).is_file()
}

fn encode_png_base64(image: &RgbaImage) -> anyhow::Result<String> {
    let mut cursor = std::io::Cursor::new(Vec::new());
    image.write_to(&mut cursor, image::ImageFormat::Png)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(cursor.into_inner()))
}

fn record_ffmpeg() -> bool {
    std::process::Command::new("ffmpeg")
        .arg("-version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}
