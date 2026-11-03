//! Floating status windows for background jobs.
//!
//! Both live in their own always-on-top, transparent viewport rather than in
//! the main window: while a capture or a recording runs the main window is
//! parked off-screen so it cannot appear in the captured pixels, which would
//! also hide anything drawn inside it. A separate window keeps the status
//! visible where the user is looking — the centre top of the display being
//! recorded.
//!
//! The pill is deliberately interactive: pausing, stopping and annotating are
//! the answers to "how do I end this?", which used to be invisible until the
//! job was already over.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use egui::{Color32, Frame, RichText, Vec2};

use crate::i18n::Strings;
use crate::longshot::LongShotJob;
use crate::record::RecordJob;
use crate::theme::Palette;
use crate::ui;

pub const RECORD_HUD_ID: &str = "pinsight-record-hud";
pub const LONG_HUD_ID: &str = "pinsight-long-hud";
/// Native window titles of the floating HUD pills. `capture::set_capture_exclusion`
/// finds these windows by title and marks them `WDA_EXCLUDEFROMCAPTURE` so the
/// Windows.Graphics.Capture pipeline (xcap) never bakes them into a screenshot
/// or recording. They MUST match the `with_title(...)` values passed to
/// `pill_window` below, or the exclusion cannot find the window.
pub const RECORD_WINDOW_TITLE: &str = "PinSight — recording";
pub const LONG_WINDOW_TITLE: &str = "PinSight — scrolling capture";
/// Wide enough for the status row of every locale: state label, timer,
/// pause/resume, annotate and stop all share one line, and the Chinese
/// labels ("录制中 / 暂停 / 标注 / 停止") need the most room of all.
const HUD_WIDTH: f32 = 540.0;
/// Three text lines (status row, hint, toast) plus padding.
const HUD_HEIGHT: f32 = 112.0;

/// What the user asked the recording pill to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordHudAction {
    Pause,
    Resume,
    Stop,
    Annotate,
}

/// Top-centre placement of a pill on `monitor`, in logical points.
fn pill_position(monitor: &crate::util::PhysRect, scale: f32, width: f32) -> egui::Pos2 {
    let scale = scale.max(0.5);
    let left = monitor.x as f32 / scale;
    let top = monitor.y as f32 / scale;
    let w = monitor.w as f32 / scale;
    // Whole pixels: a fractional position is re-rounded by the OS on every
    // frame, which makes the window jitter and the hovered cursor flicker.
    egui::pos2(
        (left + (w - width).max(0.0) / 2.0).round(),
        (top + 14.0).round(),
    )
}

/// A small always-on-top window holding `add_contents` on a rounded card, so
/// the status reads over any background.
#[allow(clippy::too_many_arguments)]
fn pill_window(
    ctx: &egui::Context,
    id: &str,
    title: &str,
    monitor: &crate::util::PhysRect,
    scale: f32,
    height: f32,
    palette: &Palette,
    add_contents: impl FnOnce(&mut egui::Ui),
) {
    let builder = egui::ViewportBuilder::default()
        .with_title(title)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false)
        .with_inner_size(Vec2::new(HUD_WIDTH, height))
        .with_position(pill_position(monitor, scale, HUD_WIDTH));
    // `show_viewport_immediate` takes an `FnMut`, so the one-shot body is
    // parked in an Option and taken on the first (only) call.
    let mut body = Some(add_contents);
    let frame = Frame::NONE
        .fill(palette.card)
        .stroke(egui::Stroke::new(1.0, palette.stroke_strong))
        .corner_radius(16)
        .inner_margin(egui::Margin::symmetric(14, 8));
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of(id), builder, |ui, _| {
        let add = body.take().expect("viewport body called once");
        egui::CentralPanel::default().frame(frame).show(ui, add);
    });
}

/// Latest status line, if the toast is still fresh. Drawn under the pill so a
/// "recording started / saving" message is not lost behind the parked main
/// window.
fn toast_line(ui: &mut egui::Ui, palette: &Palette, toast: Option<&str>) {
    let Some(text) = toast else { return };
    if text.is_empty() {
        return;
    }
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(11.5).color(palette.text_weak));
}

/// Compact pill width/height (recording, after the countdown).
const PILL_W: f32 = 340.0;
/// Countdown overlay is a square, centred on the recorded region.
const COUNTDOWN_SIZE: f32 = 220.0;

/// Live snapshot the deferred pill draws from. Refreshed every main frame so
/// the timer stays honest while the viewport repaints on its own schedule.
#[derive(Clone)]
struct RecordSnap {
    time: String,
    paused: bool,
    countdown: u32,
    limit: u32,
    toast: String,
    region: crate::util::PhysRect,
    monitor: crate::util::PhysRect,
    scale: f32,
}

impl RecordSnap {
    fn blank() -> Self {
        let unit = crate::util::PhysRect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        Self {
            time: String::new(),
            paused: false,
            countdown: 0,
            limit: 0,
            toast: String::new(),
            region: unit,
            monitor: unit,
            scale: 1.0,
        }
    }
}

static RECORD_SNAP: OnceLock<Arc<Mutex<RecordSnap>>> = OnceLock::new();
static RECORD_ACTION: OnceLock<Arc<Mutex<Option<RecordHudAction>>>> = OnceLock::new();
/// Whether the deferred recording viewport is currently open, so it can be
/// closed the moment the job ends (a deferred viewport outlives the frame).
static RECORD_OPEN: AtomicBool = AtomicBool::new(false);

fn record_snap() -> &'static Arc<Mutex<RecordSnap>> {
    RECORD_SNAP.get_or_init(|| Arc::new(Mutex::new(RecordSnap::blank())))
}

fn record_action() -> &'static Arc<Mutex<Option<RecordHudAction>>> {
    RECORD_ACTION.get_or_init(|| Arc::new(Mutex::new(None)))
}

/// Top-centre placement of the compact pill on `monitor`, in logical points.
fn pill_pos(monitor: &crate::util::PhysRect, scale: f32, w: f32) -> egui::Pos2 {
    let scale = scale.max(0.5);
    let left = monitor.x as f32 / scale;
    let top = monitor.y as f32 / scale;
    let mw = monitor.w as f32 / scale;
    // Whole pixels — see the note in `pill_position`.
    egui::pos2(
        (left + (mw - w).max(0.0) / 2.0).round(),
        (top + 12.0).round(),
    )
}

/// Centre of `region` for a square countdown window, in logical points.
fn center_pos(region: &crate::util::PhysRect, scale: f32, size: f32) -> egui::Pos2 {
    let scale = scale.max(0.5);
    let left = region.x as f32 / scale;
    let top = region.y as f32 / scale;
    let rw = region.w as f32 / scale;
    let rh = region.h as f32 / scale;
    // Whole pixels — see the note in `pill_position`.
    egui::pos2(
        (left + (rw - size) / 2.0).round(),
        (top + (rh - size) / 2.0).round(),
    )
}

/// A filled red "Stop" pill, the one action that must never be missed.
fn stop_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let font = egui::FontId::proportional(13.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE);
    let size = Vec2::new(galley.size().x + 22.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        Color32::from_rgb(0xF2, 0x5B, 0x5B)
    } else {
        Color32::from_rgb(0xD8, 0x3F, 0x3F)
    };
    ui.painter().rect_filled(rect, 14.0, fill);
    ui.painter().galley(
        egui::pos2(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        Color32::WHITE,
    );
    response
}

/// Big centred countdown number with a soft translucent disc, so it reads like
/// every other recorder instead of being squeezed into the status bar.
fn draw_countdown(ui: &mut egui::Ui, snap: &RecordSnap, tr: &'static Strings) {
    let rect = ui.max_rect();
    let center = rect.center();
    let r = rect.width().min(rect.height()) * 0.42;
    let painter = ui.painter();
    painter.circle_filled(center, r, Color32::from_black_alpha(165));
    painter.circle_stroke(
        center,
        r,
        egui::Stroke::new(2.0, Color32::from_white_alpha(70)),
    );
    painter.text(
        center - egui::vec2(0.0, r * 0.08),
        egui::Align2::CENTER_CENTER,
        snap.countdown.to_string(),
        egui::FontId::proportional(r * 1.02),
        Color32::WHITE,
    );
    painter.text(
        center + egui::vec2(0.0, r * 0.58),
        egui::Align2::CENTER_CENTER,
        tr.rec_countdown_label,
        egui::FontId::proportional(12.0),
        Color32::from_white_alpha(210),
    );
}

/// The compact recording pill: pulsing dot, timer, pause / annotate / stop.
fn draw_record_pill(
    ui: &mut egui::Ui,
    snap: &RecordSnap,
    tr: &'static Strings,
    palette: &Palette,
) -> Option<RecordHudAction> {
    let mut action = None;
    let paused = snap.paused;
    let pulse = 0.55 + 0.45 * (ui.input(|i| i.time) as f32 * 4.0).sin();
    let frame = Frame::NONE
        .fill(palette.card.gamma_multiply(0.97))
        .stroke(egui::Stroke::new(1.0, palette.stroke_strong))
        .corner_radius(26)
        .inner_margin(egui::Margin::symmetric(14, 7));
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 9.0;
            // Pulsing dot: red while recording, amber while paused.
            let (dot_rect, dot_resp) =
                ui.allocate_exact_size(Vec2::splat(10.0), egui::Sense::hover());
            let dot = if paused {
                palette.warn
            } else {
                Color32::from_rgb(0xE8, 0x4A, 0x4A)
            };
            ui.painter().circle_filled(
                dot_rect.center(),
                4.5,
                dot.gamma_multiply(if paused { 1.0 } else { pulse }),
            );
            dot_resp.on_hover_text(if paused {
                tr.rec_hud_paused
            } else {
                tr.rec_hud_recording
            });
            ui.label(
                RichText::new(snap.time.as_str())
                    .monospace()
                    .size(15.0)
                    .color(palette.text),
            );
            if snap.limit > 0 {
                ui.label(
                    RichText::new(format!("/ {}", crate::util::fmt_seconds(snap.limit)))
                        .monospace()
                        .size(10.5)
                        .color(palette.text_weak),
                );
            }
            // Hairline separator between the readout and the controls.
            let sep = ui
                .allocate_exact_size(Vec2::new(1.0, 18.0), egui::Sense::hover())
                .0;
            ui.painter().rect_filled(sep, 0.0, palette.stroke);
            if ui::icon_button(
                ui,
                palette,
                if paused {
                    ui::Icon::Reset
                } else {
                    ui::Icon::Record
                },
                false,
                if paused {
                    tr.rec_hud_resume
                } else {
                    tr.rec_hud_pause
                },
            )
            .clicked()
            {
                action = Some(if paused {
                    RecordHudAction::Resume
                } else {
                    RecordHudAction::Pause
                });
            }
            if ui::icon_button(ui, palette, ui::Icon::Pencil, false, tr.rec_annotate).clicked() {
                action = Some(RecordHudAction::Annotate);
            }
            if stop_button(ui, tr.rec_hud_stop)
                .on_hover_text(tr.rec_hud_hint)
                .clicked()
            {
                action = Some(RecordHudAction::Stop);
            }
        });
        if !snap.toast.is_empty() {
            ui.label(
                RichText::new(snap.toast.as_str())
                    .size(10.5)
                    .color(palette.text_weak),
            );
        }
    });
    action
}

/// Recording status pill (deferred viewport).
///
/// The viewport is *deferred* rather than immediate: an immediate viewport is
/// re-shown every frame, which made the OS reset the mouse cursor at frame rate
/// whenever it hovered a button — the "cursor flickers crazily" bug. The pin
/// windows use the same deferred pattern and never flicker. The countdown is
/// drawn as a large centred number (click-through) instead of a line in the bar.
pub fn show_record(
    ctx: &egui::Context,
    job: &mut RecordJob,
    tr: &'static Strings,
    palette: &Palette,
    toast: Option<&str>,
) -> Option<RecordHudAction> {
    ctx.request_repaint_after(Duration::from_millis(250));

    // Clock that stops while paused — the same semantics as every desktop
    // recorder. `active_elapsed` updates the job's active-time accumulator.
    let elapsed = job.active_elapsed();
    let secs = elapsed.as_secs();
    let time = if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    };
    let toast_s = toast.unwrap_or_default().to_string();
    let snap = RecordSnap {
        time,
        paused: job.paused(),
        countdown: job.countdown,
        limit: job.max_seconds,
        toast: toast_s.clone(),
        region: job.region,
        monitor: job.monitor,
        scale: job.scale,
    };
    if let Ok(mut s) = record_snap().lock() {
        *s = snap.clone();
    }

    let counting = snap.countdown > 0;
    let (w, h, pos) = if counting {
        (
            COUNTDOWN_SIZE,
            COUNTDOWN_SIZE,
            center_pos(&snap.region, snap.scale, COUNTDOWN_SIZE),
        )
    } else {
        let h = if toast_s.is_empty() { 46.0 } else { 66.0 };
        (PILL_W, h, pill_pos(&snap.monitor, snap.scale, PILL_W))
    };

    let builder = egui::ViewportBuilder::default()
        .with_title(RECORD_WINDOW_TITLE)
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top()
        .with_resizable(false)
        .with_taskbar(false)
        // The countdown is click-through; the pill needs its buttons.
        .with_mouse_passthrough(counting)
        .with_inner_size(Vec2::new(w, h))
        .with_position(pos);

    let state = Arc::clone(record_snap());
    let action_slot = Arc::clone(record_action());
    let palette = *palette;
    ctx.show_viewport_deferred(
        egui::ViewportId::from_hash_of(RECORD_HUD_ID),
        builder,
        move |ui, _class| {
            ui.ctx().request_repaint_after(Duration::from_millis(250));
            let snap = match state.lock() {
                Ok(g) => g.clone(),
                Err(_) => return,
            };
            if snap.countdown > 0 {
                egui::CentralPanel::default()
                    .frame(Frame::NONE.fill(Color32::TRANSPARENT))
                    .show(ui, |ui| draw_countdown(ui, &snap, tr));
            } else if let Some(a) = draw_record_pill(ui, &snap, tr, &palette) {
                if let Ok(mut slot) = action_slot.lock() {
                    *slot = Some(a);
                }
            }
        },
    );
    RECORD_OPEN.store(true, Ordering::Relaxed);

    // Esc stops the recording too — the same key that cancels a capture.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(RecordHudAction::Stop);
    }
    // Pick up a button press the deferred viewport recorded this frame.
    record_action().lock().ok().and_then(|mut slot| slot.take())
}

/// Close the deferred recording viewport once its job is over. A deferred
/// viewport outlives the frame, so without this a stale pill would linger.
pub fn close_record(ctx: &egui::Context) {
    if RECORD_OPEN.swap(false, Ordering::Relaxed) {
        ctx.send_viewport_cmd_to(
            egui::ViewportId::from_hash_of(RECORD_HUD_ID),
            egui::ViewportCommand::Close,
        );
    }
}

/// Scrolling-capture progress pill: frames stitched, current height and a stop
/// button, so ending a long screenshot never depends on remembering the Esc
/// key.
pub fn show_long(
    ctx: &egui::Context,
    job: &LongShotJob,
    tr: &'static Strings,
    palette: &Palette,
    toast: Option<&str>,
) -> bool {
    ctx.request_repaint_after(Duration::from_millis(300));
    let mut stop = false;

    pill_window(
        ctx,
        LONG_HUD_ID,
        LONG_WINDOW_TITLE,
        &job.monitor,
        job.scale,
        HUD_HEIGHT,
        palette,
        |ui| {
            ui.set_width(HUD_WIDTH);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                    ui.painter().circle_filled(
                        rect.center(),
                        5.5,
                        palette.accent.gamma_multiply(0.9),
                    );
                    ui.label(
                        RichText::new(tr.long_settings)
                            .size(12.5)
                            .color(palette.text),
                    );
                    ui.label(
                        RichText::new(format!("{} {} · {} px", job.frames, tr.frames, job.height))
                            .monospace()
                            .size(12.0)
                            .color(palette.text),
                    );
                    ui.add_space(4.0);
                    if ui::primary_button(ui, palette, tr.rec_hud_stop).clicked() {
                        stop = true;
                    }
                });
                ui.label(
                    RichText::new(tr.long_hud_hint)
                        .size(10.5)
                        .color(palette.text_weak),
                );
                toast_line(ui, palette, toast);
            });
        },
    );

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return true;
    }
    stop
}
