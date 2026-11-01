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
    egui::pos2(left + (w - width).max(0.0) / 2.0, top + 14.0)
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

/// Recording status pill: state, elapsed time, pause/stop and the annotation
/// layer toggle. Returns the action the user picked, if any.
pub fn show_record(
    ctx: &egui::Context,
    job: &mut RecordJob,
    tr: &'static Strings,
    palette: &Palette,
    toast: Option<&str>,
) -> Option<RecordHudAction> {
    // A fresh repaint twice a second keeps the timer honest without spinning
    // the GPU when nothing else changes.
    ctx.request_repaint_after(Duration::from_millis(500));

    // Clock that stops while paused — the same semantics as every desktop
    // recorder. `active_elapsed` updates the job's active-time accumulator.
    let elapsed = job.active_elapsed();
    let paused = job.paused();
    let secs = elapsed.as_secs();
    let time = if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
    } else {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    };
    // Pre-recording countdown replaces the state label ("starting in 3 s").
    let countdown = job.countdown;
    let limit = job.max_seconds;
    let mut action = None;

    pill_window(
        ctx,
        RECORD_HUD_ID,
        RECORD_WINDOW_TITLE,
        &job.monitor,
        job.scale,
        HUD_HEIGHT,
        palette,
        |ui| {
            ui.set_width(HUD_WIDTH);
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 10.0;
                    // Pulsing dot: solid red while recording, amber while
                    // paused.
                    let pulse = 0.55 + 0.45 * (elapsed.as_millis() as f32 / 1000.0).sin();
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), egui::Sense::hover());
                    let dot = if paused {
                        palette.warn
                    } else {
                        Color32::from_rgb(0xE8, 0x4A, 0x4A)
                    };
                    ui.painter().circle_filled(
                        rect.center(),
                        5.5,
                        dot.gamma_multiply(if paused { 1.0 } else { pulse }),
                    );
                    let status: std::borrow::Cow<'_, str> = if countdown > 0 {
                        std::borrow::Cow::Owned(
                            tr.rec_countdown.replace("{n}", &countdown.to_string()),
                        )
                    } else if paused {
                        std::borrow::Cow::Borrowed(tr.rec_hud_paused)
                    } else {
                        std::borrow::Cow::Borrowed(tr.rec_hud_recording)
                    };
                    ui.label(RichText::new(status).size(12.5).color(palette.text));
                    ui.label(
                        RichText::new(time.clone())
                            .monospace()
                            .size(13.0)
                            .color(palette.text),
                    );
                    if limit > 0 {
                        ui.label(
                            RichText::new(format!("/ {}", crate::util::fmt_seconds(limit)))
                                .monospace()
                                .size(11.0)
                                .color(palette.text_weak),
                        );
                    }
                    ui.add_space(4.0);
                    if ui::link_button(
                        ui,
                        palette,
                        if paused {
                            ui::Icon::Reset
                        } else {
                            ui::Icon::Record
                        },
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
                    if ui::link_button(ui, palette, ui::Icon::Pencil, tr.rec_annotate).clicked() {
                        action = Some(RecordHudAction::Annotate);
                    }
                    if ui::primary_button(ui, palette, tr.rec_hud_stop).clicked() {
                        action = Some(RecordHudAction::Stop);
                    }
                });
                ui.label(
                    RichText::new(tr.rec_hud_hint)
                        .size(10.5)
                        .color(palette.text_weak),
                );
                toast_line(ui, palette, toast);
            });
        },
    );

    // Esc stops the recording too — the same key that cancels a capture.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(RecordHudAction::Stop);
    }
    action
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
