//! Annotation layer drawn on top of a running recording.
//!
//! The layer is two always-on-top, transparent windows: a canvas that covers
//! exactly the recording region, and a small toolbar floating above it. Marks
//! are kept as plain vector data and repainted every frame, so they stay crisp
//! and can be undone as a whole.
//!
//! Because the canvas is a real window inside the recorded region, the desktop
//! compositor blends it into every frame the recorder grabs — the marks are
//! burned into the output without the recorder knowing anything about them.
//! The canvas is click-through by default so the recording stays usable; picking
//! a tool turns input back on until the mark is placed.

use egui::{Color32, FontId, Pos2, Rect, Shape, Stroke, Vec2};

use crate::i18n::Strings;
use crate::theme::Palette;
use crate::ui;
use crate::util::PhysRect;

const CANVAS_ID: &str = "pinsight-mark-canvas";
const TOOLBAR_ID: &str = "pinsight-mark-toolbar";
const TOOLBAR_WIDTH: f32 = 520.0;
const TOOLBAR_HEIGHT: f32 = 150.0;

/// Emoji offered by the palette. Rendered with the fonts egui already carries
/// (Noto Emoji ships with egui's default font set), so no extra asset.
const EMOJIS: [&str; 14] = [
    "👍", "❤️", "😂", "😮", "😢", "😡", "🔥", "✅", "❌", "⭐", "🎉", "❓", "💯", "😀",
];

/// Swatches offered in the toolbar, in toolbar order.
const COLORS: [[u8; 3]; 9] = [
    [0xE8, 0x4A, 0x4A], // red
    [0xF5, 0xA5, 0x4B], // orange
    [0xF5, 0xE1, 0x4B], // yellow
    [0x4B, 0xD6, 0x8C], // green
    [0x4B, 0xC3, 0xE8], // cyan
    [0x5B, 0x9D, 0xFF], // blue
    [0xB4, 0x4B, 0xE8], // purple
    [0xFF, 0xFF, 0xFF], // white
    [0x1A, 0x1A, 0x1A], // black
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkTool {
    None,
    Arrow,
    Line,
    Rect,
    Ellipse,
    Text,
    Emoji,
}

/// What the layer wants the app to do this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MarkLayerOutcome {
    /// The user closed the layer.
    pub close: bool,
    /// All marks were just cleared.
    pub cleared: bool,
}

/// One committed mark. Coordinates are canvas-local logical points, so a mark
/// stays anchored to the content under it no matter where the window sits.
#[derive(Debug, Clone)]
pub enum Mark {
    Arrow {
        from: Pos2,
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Line {
        from: Pos2,
        to: Pos2,
        color: Color32,
        width: f32,
    },
    Rect {
        min: Pos2,
        max: Pos2,
        color: Color32,
        width: f32,
    },
    Ellipse {
        center: Pos2,
        radius: Vec2,
        color: Color32,
        width: f32,
    },
    Text {
        pos: Pos2,
        text: String,
        px: f32,
        color: Color32,
    },
    Emoji {
        pos: Pos2,
        ch: String,
        px: f32,
    },
}

struct TextDraft {
    pos: Pos2,
    buffer: String,
    focused: bool,
}

/// The live annotation layer over a recording.
pub struct MarkLayer {
    /// Scale factor of the monitor owning the region.
    scale: f32,
    /// Logical top-left of the canvas on the desktop.
    origin: Pos2,
    /// Logical size of the canvas.
    size: Vec2,
    /// Physical rect of the monitor (toolbar placement).
    monitor: PhysRect,

    tool: MarkTool,
    color: [u8; 3],
    width: f32,
    text_px: f32,
    marks: Vec<Mark>,
    drag: Option<(Pos2, Pos2)>,
    text_draft: Option<TextDraft>,
    emoji_at: Option<Pos2>,
    /// When true the canvas ignores the mouse and clicks reach the window
    /// below, so the recording stays interactive.
    passthrough: bool,

    tr: &'static Strings,
    palette: Palette,
}

impl MarkLayer {
    pub fn new(
        region: PhysRect,
        monitor: PhysRect,
        scale: f32,
        tr: &'static Strings,
        palette: Palette,
    ) -> Self {
        let scale = scale.max(0.5);
        Self {
            scale,
            origin: Pos2::new(region.x as f32 / scale, region.y as f32 / scale),
            size: Vec2::new(region.w as f32 / scale, region.h as f32 / scale),
            monitor,
            tool: MarkTool::None,
            color: [0xE8, 0x4A, 0x4A],
            width: 4.0,
            text_px: 20.0,
            marks: Vec::new(),
            drag: None,
            text_draft: None,
            emoji_at: None,
            passthrough: true,
            tr,
            palette,
        }
    }

    /// Draw both windows and report what the user asked for.
    pub fn show(&mut self, ctx: &egui::Context) -> MarkLayerOutcome {
        let mut outcome = self.show_canvas(ctx);
        let toolbar = self.show_toolbar(ctx);
        outcome.close |= toolbar.close;
        outcome.cleared |= toolbar.cleared;
        outcome
    }

    // ---- canvas ----------------------------------------------------------

    fn show_canvas(&mut self, ctx: &egui::Context) -> MarkLayerOutcome {
        let builder = egui::ViewportBuilder::default()
            .with_title("PinSight — annotations")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size(self.size)
            .with_position(self.origin)
            .with_mouse_passthrough(self.passthrough);
        let mut outcome = MarkLayerOutcome::default();
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(CANVAS_ID),
            builder,
            |ui, _| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE.fill(Color32::TRANSPARENT))
                    .show(ui, |ui| {
                        let rect = ui.max_rect();
                        // A faint outline of the recorded region so the user can
                        // see where the layer ends.
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0, self.palette.accent.gamma_multiply(0.55)),
                            egui::StrokeKind::Inside,
                        );

                        self.paint_marks(ui);
                        self.paint_preview(ui);
                        self.handle_pointer(ui);
                        self.show_text_draft(ui);
                        self.show_emoji_palette(ui);
                        outcome.close |= self.handle_keys(ui);
                    });
            },
        );
        outcome
    }

    fn paint_marks(&self, ui: &mut egui::Ui) {
        let painter = ui.painter();
        for mark in &self.marks {
            match mark {
                Mark::Arrow { from, to, color, width } => {
                    let stroke = Stroke::new(*width, *color);
                    painter.line_segment([*from, *to], stroke);
                    let delta = *to - *from;
                    let len = delta.length();
                    if len < 1.0 {
                        continue;
                    }
                    let back = -delta / len;
                    let head = (width * 3.0).clamp(8.0, 22.0);
                    for sign in [-1.0_f32, 1.0] {
                        let (sin, cos) = (sign * 0.42).sin_cos();
                        let dir = Vec2::new(
                            back.x * cos - back.y * sin,
                            back.x * sin + back.y * cos,
                        );
                        painter.line_segment([*to, *to + dir * head], stroke);
                    }
                }
                Mark::Line { from, to, color, width } => {
                    painter.line_segment([*from, *to], Stroke::new(*width, *color));
                }
                Mark::Rect { min, max, color, width } => {
                    painter.rect_stroke(
                        Rect::from_two_pos(*min, *max),
                        4.0,
                        Stroke::new(*width, *color),
                        egui::StrokeKind::Inside,
                    );
                }
                Mark::Ellipse { center, radius, color, width } => {
                    painter.add(Shape::ellipse_stroke(
                        *center,
                        *radius,
                        Stroke::new(*width, *color),
                    ));
                }
                Mark::Text { pos, text, px, color } => {
                    let galley = painter.layout_no_wrap(
                        text.clone(),
                        FontId::proportional(*px),
                        *color,
                    );
                    let pad = 6.0;
                    let box_rect = Rect::from_min_size(
                        *pos,
                        galley.size() + Vec2::splat(pad * 2.0),
                    );
                    painter.rect_filled(
                        box_rect,
                        6.0,
                        Color32::from_rgba_unmultiplied(0, 0, 0, 150),
                    );
                    painter.galley(*pos + Vec2::splat(pad), galley, *color);
                }
                Mark::Emoji { pos, ch, px } => {
                    let galley = painter.layout_no_wrap(
                        ch.clone(),
                        FontId::proportional(*px),
                        Color32::WHITE,
                    );
                    painter.galley(*pos, galley, Color32::WHITE);
                }
            }
        }
    }

    fn paint_preview(&self, ui: &mut egui::Ui) {
        let Some((from, to)) = self.drag else {
            return;
        };
        let color = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
        let stroke = Stroke::new(self.width, color);
        let painter = ui.painter();
        match self.tool {
            MarkTool::Arrow => {
                painter.line_segment([from, to], stroke);
                let delta = to - from;
                let len = delta.length();
                if len >= 1.0 {
                    let back = -delta / len;
                    let head = (self.width * 3.0).clamp(8.0, 22.0);
                    for sign in [-1.0_f32, 1.0] {
                        let (sin, cos) = (sign * 0.42).sin_cos();
                        let dir = Vec2::new(
                            back.x * cos - back.y * sin,
                            back.x * sin + back.y * cos,
                        );
                        painter.line_segment([to, to + dir * head], stroke);
                    }
                }
            }
            MarkTool::Line => {
                painter.line_segment([from, to], stroke);
            }
            MarkTool::Rect => {
                painter.rect_stroke(Rect::from_two_pos(from, to), 4.0, stroke, egui::StrokeKind::Inside);
            }
            MarkTool::Ellipse => {
                let center = from + (to - from) / 2.0;
                painter.add(Shape::ellipse_stroke(
                    center,
                    (to - from) / 2.0,
                    stroke,
                ));
            }
            _ => {}
        }
    }

    // ---- input -----------------------------------------------------------

    fn handle_pointer(&mut self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        // Stable id: the canvas is recreated every frame and must keep the
        // same widget id for a press and its release to pair up.
        #[allow(deprecated)]
        let response = ui.interact(
            rect,
            egui::Id::new("pinsight-mark-canvas"),
            egui::Sense::click_and_drag(),
        );
        if self.tool == MarkTool::Text || self.tool == MarkTool::Emoji {
            if response.clicked() {
                if let Some(pos) = response.interact_pointer_pos() {
                    match self.tool {
                        MarkTool::Text => {
                            self.text_draft = Some(TextDraft {
                                pos,
                                buffer: String::new(),
                                focused: false,
                            });
                        }
                        MarkTool::Emoji => self.emoji_at = Some(pos),
                        _ => {}
                    }
                }
            }
            return;
        }
        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.drag = Some((pos, pos));
            }
        } else if self.drag.is_some() {
            // egui 0.37 has no `drag_released`; the button coming up is read
            // straight from the pointer state.
            let released = ui.input(|i| i.pointer.primary_released());
            if released {
                if let Some((from, to)) = self.drag.take() {
                    self.commit_shape(from, to);
                }
            } else if let (Some(drag), Some(pos)) =
                (self.drag.as_mut(), response.interact_pointer_pos())
            {
                drag.1 = pos;
            }
        }
    }

    fn commit_shape(&mut self, from: Pos2, to: Pos2) {
        let color = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
        let mark = match self.tool {
            MarkTool::Arrow => Some(Mark::Arrow {
                from,
                to,
                color,
                width: self.width,
            }),
            MarkTool::Line => Some(Mark::Line {
                from,
                to,
                color,
                width: self.width,
            }),
            MarkTool::Rect => Some(Mark::Rect {
                min: Pos2::new(from.x.min(to.x), from.y.min(to.y)),
                max: Pos2::new(from.x.max(to.x), from.y.max(to.y)),
                color,
                width: self.width,
            }),
            MarkTool::Ellipse => Some(Mark::Ellipse {
                center: from + (to - from) / 2.0,
                radius: (to - from) / 2.0,
                color,
                width: self.width,
            }),
            _ => None,
        };
        if let Some(mark) = mark {
            // A stray click that dragged nowhere would add a zero-size mark.
            if !matches!(&mark, Mark::Arrow { from, to, .. } if (*to - *from).length() < 3.0)
                && !matches!(&mark, Mark::Line { from, to, .. } if (*to - *from).length() < 3.0)
                && !matches!(&mark, Mark::Rect { min, max, .. } if (max.x - min.x) < 3.0 || (max.y - min.y) < 3.0)
                && !matches!(&mark, Mark::Ellipse { radius, .. } if radius.x < 3.0 || radius.y < 3.0)
            {
                self.marks.push(mark);
            }
            // Back to click-through: the mark is placed, the recording is
            // interactive again until the next tool is picked.
            self.passthrough = true;
        }
    }

    fn show_text_draft(&mut self, ui: &mut egui::Ui) {
        let Some(draft) = self.text_draft.as_mut() else {
            return;
        };
        let mut commit = false;
        let mut cancel = false;
        let anchor = draft.pos;
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new(("pinsight-mark-text", anchor.x as i32, anchor.y as i32)))
            .fixed_pos(anchor)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .fill(self.palette.card)
                    .stroke(egui::Stroke::new(1.0, self.palette.stroke_strong))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(10, 8))
                    .show(ui, |ui| {
                        ui.set_width(260.0);
                        // A multiline TextEdit swallows plain Enter as a
                        // newline, so it could never be committed with Enter.
                        // Consume Enter first: plain Enter commits, Shift+Enter
                        // inserts a newline.
                        let enter = ui.input_mut(|i| {
                            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)
                        });
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut draft.buffer)
                                .hint_text(self.tr.mk_text_hint)
                                .desired_width(240.0)
                                .font(egui::FontId::proportional(self.text_px * 0.8)),
                        );
                        if !draft.focused {
                            response.request_focus();
                            draft.focused = true;
                        }
                        if enter {
                            commit = true;
                        }
                        ui.horizontal(|ui| {
                            if ui::primary_button(ui, &self.palette, self.tr.mk_text).clicked() {
                                commit = true;
                            }
                            if ui::link_button(ui, &self.palette, ui::Icon::Close, self.tr.mk_close)
                                .clicked()
                            {
                                cancel = true;
                            }
                        });
                    });
            });
        if cancel {
            self.text_draft = None;
            self.passthrough = true;
        } else if commit {
            if let Some(draft) = self.text_draft.take() {
                let text = draft.buffer.trim().to_string();
                if !text.is_empty() {
                    let color =
                        Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
                    self.marks.push(Mark::Text {
                        pos: draft.pos,
                        text,
                        px: self.text_px,
                        color,
                    });
                }
            }
            self.passthrough = true;
        }
    }

    fn show_emoji_palette(&mut self, ui: &mut egui::Ui) {
        let Some(at) = self.emoji_at else {
            return;
        };
        let mut picked: Option<&'static str> = None;
        #[allow(deprecated)]
        egui::Area::new(egui::Id::new((
            "pinsight-mark-emoji",
            at.x as i32,
            at.y as i32,
        )))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(self.palette.card)
                .stroke(egui::Stroke::new(1.0, self.palette.stroke_strong))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(8, 8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(self.tr.mk_emoji_title)
                            .size(11.0)
                            .color(self.palette.text_weak),
                    );
                    egui::Grid::new("pinsight-mark-emoji-grid")
                        .spacing(Vec2::new(4.0, 4.0))
                        .show(ui, |ui| {
                            for (i, emoji) in EMOJIS.iter().enumerate() {
                                if i % 7 == 0 && i > 0 {
                                    ui.end_row();
                                }
                                if ui
                                    .add(
                                        egui::Button::new(
                                            egui::RichText::new(*emoji).size(18.0),
                                        )
                                        .frame(false),
                                    )
                                    .clicked()
                                {
                                    picked = Some(emoji);
                                }
                            }
                        });
                });
        });
        if let Some(emoji) = picked {
            self.marks.push(Mark::Emoji {
                pos: at,
                ch: emoji.to_string(),
                px: self.text_px * 1.4,
            });
            self.emoji_at = None;
            self.passthrough = true;
        }
    }

    fn handle_keys(&mut self, ui: &mut egui::Ui) -> bool {
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if !escape {
            return false;
        }
        // Escape unwinds one layer at a time: the open draft, then the palette,
        // then the tool, and only then the whole layer.
        if self.text_draft.take().is_some() {
            self.passthrough = true;
            return false;
        }
        if self.emoji_at.take().is_some() {
            self.passthrough = true;
            return false;
        }
        if self.tool != MarkTool::None {
            self.tool = MarkTool::None;
            self.passthrough = true;
            return false;
        }
        true
    }

    // ---- toolbar ---------------------------------------------------------

    fn show_toolbar(&mut self, ctx: &egui::Context) -> MarkLayerOutcome {
        let scale = self.scale;
        let left = self.monitor.x as f32 / scale;
        let top = self.monitor.y as f32 / scale;
        let width = self.monitor.w as f32 / scale;
        let pos = Pos2::new(
            left + (width - TOOLBAR_WIDTH).max(0.0) / 2.0,
            top + 14.0 + 96.0 + 8.0,
        );
        let builder = egui::ViewportBuilder::default()
            .with_title("PinSight — annotation tools")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_resizable(false)
            .with_taskbar(false)
            .with_inner_size(Vec2::new(TOOLBAR_WIDTH, TOOLBAR_HEIGHT))
            .with_position(pos);
        let mut outcome = MarkLayerOutcome::default();
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of(TOOLBAR_ID),
            builder,
            |ui, _| {
                egui::Frame::NONE
                    .fill(self.palette.card)
                    .stroke(egui::Stroke::new(1.0, self.palette.stroke_strong))
                    .corner_radius(14)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(TOOLBAR_WIDTH - 24.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let tools: [(MarkTool, ui::Icon, &str); 6] = [
                                (MarkTool::Arrow, ui::Icon::ArrowUpRight, self.tr.mk_arrow),
                                (MarkTool::Line, ui::Icon::Line, self.tr.mk_line),
                                (MarkTool::Rect, ui::Icon::Square, self.tr.mk_rect),
                                (MarkTool::Ellipse, ui::Icon::Circle, self.tr.mk_ellipse),
                                (MarkTool::Text, ui::Icon::Type, self.tr.mk_text),
                                (MarkTool::Emoji, ui::Icon::Palette, self.tr.mk_emoji),
                            ];
                            for (tool, icon, label) in tools {
                                if ui::icon_button(
                                    ui,
                                    &self.palette,
                                    icon,
                                    self.tool == tool,
                                    label,
                                )
                                .on_hover_text(label)
                                .clicked()
                                {
                                    if self.tool == tool {
                                        self.tool = MarkTool::None;
                                        self.passthrough = true;
                                    } else {
                                        self.tool = tool;
                                        self.passthrough = false;
                                    }
                                }
                            }
                            ui.add_space(8.0);
                            // Colour swatches.
                            for rgb in COLORS {
                                let active = self.color == rgb;
                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::splat(20.0),
                                    egui::Sense::click(),
                                );
                                let color = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                                ui.painter()
                                    .rect_filled(rect, 5.0, color);
                                if active {
                                    ui.painter().rect_stroke(
                                        rect,
                                        5.0,
                                        Stroke::new(2.0, self.palette.accent),
                                        egui::StrokeKind::Inside,
                                    );
                                }
                                if response.clicked() {
                                    self.color = rgb;
                                }
                            }
                        });

                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            ui.label(
                                egui::RichText::new(self.tr.mk_arrow)
                                    .size(11.0)
                                    .color(self.palette.text_weak),
                            );
                            let widths: [(f32, &str); 3] =
                                [(2.0, "细"), (4.0, "中"), (8.0, "粗")];
                            if let Some(picked) = ui::segmented(
                                ui,
                                &self.palette,
                                self.width,
                                &widths,
                            ) {
                                self.width = picked;
                            }
                            ui.add_space(8.0);
                            let sizes: [(f32, &str); 3] =
                                [(14.0, "小"), (20.0, "中"), (32.0, "大")];
                            if let Some(picked) =
                                ui::segmented(ui, &self.palette, self.text_px, &sizes)
                            {
                                self.text_px = picked;
                            }
                            ui.add_space(8.0);
                            if ui::toggle(ui, &self.palette, &mut self.passthrough).changed() {
                                // Click-through on: the recording is interactive.
                            }
                            ui.label(
                                egui::RichText::new(self.tr.mk_passthrough)
                                    .size(11.0)
                                    .color(self.palette.text_weak),
                            );
                            ui.add_space(4.0);
                            if ui::icon_button(
                                ui,
                                &self.palette,
                                ui::Icon::Undo,
                                false,
                                self.tr.mk_clear,
                            )
                            .on_hover_text(self.tr.ov_undo)
                            .clicked()
                            {
                                self.marks.pop();
                            }
                            if ui::icon_button(
                                ui,
                                &self.palette,
                                ui::Icon::Trash,
                                false,
                                self.tr.mk_clear,
                            )
                            .on_hover_text(self.tr.mk_clear)
                            .clicked()
                            {
                                self.marks.clear();
                                outcome.cleared = true;
                            }
                            if ui::primary_button(ui, &self.palette, self.tr.mk_close).clicked() {
                                outcome.close = true;
                            }
                        });

                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new(self.tr.mk_passthrough_hint)
                                .size(10.5)
                                .color(self.palette.text_weak),
                        );
                    });
            },
        );
        outcome
    }
}
