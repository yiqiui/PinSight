//! A Windows-style colour dialog.
//!
//! The stock `egui` picker is a small popup with three sliders; what people
//! actually expect from a desktop tool is the familiar "选择颜色" dialog: a grid
//! of basic colours, a row of custom colours that persist, an HSV field with a
//! hue bar, numeric RGB/HSL entry, an HTML field and a screen picker.
//!
//! Everything here is painted by hand so it matches the rest of the app and
//! needs no OS dialog (which would look alien next to the egui window and
//! cannot be themed).
//!
//! `egui::Id::new` is deprecated in 0.37 in favour of `Id::unique`, but a
//! *stable* id is exactly what an interactive dialog needs: `Id::unique` would
//! mint a new one every frame and no press would ever complete.
#![allow(deprecated)]

use eframe::egui;
use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

use crate::theme::{Palette, with_alpha};
use crate::ui::{self, Icon};

/// The classic 48-colour palette every Windows colour dialog shows.
const BASIC: [[u8; 3]; 48] = [
    [0x00, 0x00, 0x00],
    [0x99, 0x33, 0x00],
    [0x33, 0x33, 0x00],
    [0x00, 0x33, 0x00],
    [0x00, 0x33, 0x66],
    [0x00, 0x00, 0x80],
    [0x33, 0x33, 0x99],
    [0x33, 0x33, 0x33],
    [0x80, 0x00, 0x00],
    [0xFF, 0x66, 0x00],
    [0x80, 0x80, 0x00],
    [0x00, 0x80, 0x00],
    [0x00, 0x80, 0x80],
    [0x00, 0x00, 0xFF],
    [0x66, 0x66, 0x99],
    [0x80, 0x80, 0x80],
    [0xFF, 0x00, 0x00],
    [0xFF, 0x99, 0x00],
    [0x99, 0xCC, 0x00],
    [0x33, 0x99, 0x66],
    [0x33, 0xCC, 0xCC],
    [0x33, 0x66, 0xFF],
    [0x80, 0x00, 0x80],
    [0x96, 0x96, 0x96],
    [0xFF, 0x00, 0xFF],
    [0xFF, 0xCC, 0x00],
    [0xFF, 0xFF, 0x00],
    [0x00, 0xFF, 0x00],
    [0x00, 0xFF, 0xFF],
    [0x00, 0xCC, 0xFF],
    [0x99, 0x33, 0x66],
    [0xC0, 0xC0, 0xC0],
    [0xFF, 0x99, 0xCC],
    [0xFF, 0xCC, 0x99],
    [0xFF, 0xFF, 0x99],
    [0xCC, 0xFF, 0xCC],
    [0xCC, 0xFF, 0xFF],
    [0x99, 0xCC, 0xFF],
    [0xCC, 0x99, 0xFF],
    [0xFF, 0xFF, 0xFF],
    [0xE6, 0xE6, 0xE6],
    [0xD9, 0xD9, 0xD9],
    [0xBF, 0xBF, 0xBF],
    [0xA6, 0xA6, 0xA6],
    [0x8C, 0x8C, 0x8C],
    [0x73, 0x73, 0x73],
    [0x59, 0x59, 0x59],
    [0x40, 0x40, 0x40],
];

/// Which setting the dialog is editing. Kept as data so the dialog itself does
/// not need to know anything about the config file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorTarget {
    Accent,
    CaptureMask,
    CaptureAnchor,
    PinShadow,
    PinTextBg,
    PinTextFg,
    DecoBorder,
    DecoShadow,
}

impl ColorTarget {
    /// Title suffix so it is obvious which field is being edited.
    pub fn label(self, t: &crate::i18n::Strings) -> &'static str {
        match self {
            Self::Accent => t.accent,
            Self::CaptureMask => t.cap_mask_color,
            Self::CaptureAnchor => t.cap_anchor_color,
            Self::PinShadow => t.pin_shadow_color,
            Self::PinTextBg => t.pin_text_bg,
            Self::PinTextFg => t.pin_text_fg,
            Self::DecoBorder => t.deco_border_color,
            Self::DecoShadow => t.deco_shadow_color,
        }
    }
}

/// What the dialog decided this frame.
pub enum PickerAction {
    /// Commit the colour.
    Apply(Color32),
    /// Close without touching the setting.
    Cancel,
    /// Ask the app to run the screen picker; the dialog stays open.
    PickScreen,
}

pub struct ColorPicker {
    pub target: ColorTarget,
    /// Hue in degrees, saturation and value in 0..1.
    h: f32,
    s: f32,
    v: f32,
    /// 0..255, only used by targets that can be translucent.
    pub alpha: u8,
    /// Recently used colours, most recent last.
    pub custom: Vec<[u8; 3]>,
    /// Text of the HTML field while the user is typing in it.
    hex_draft: String,
    /// Set while the pointer drags inside the HSV field.
    dragging: Option<Drag>,
    /// First-frame positioning.
    first: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Drag {
    Field,
    Hue,
}

impl ColorPicker {
    pub fn new(target: ColorTarget, rgb: [u8; 3], custom: Vec<[u8; 3]>) -> Self {
        let (h, s, v) = rgb_to_hsv(rgb);
        Self {
            target,
            h,
            s,
            v,
            alpha: 255,
            custom,
            hex_draft: format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]),
            dragging: None,
            first: true,
        }
    }

    pub fn rgb(&self) -> [u8; 3] {
        hsv_to_rgb(self.h, self.s, self.v)
    }

    /// Adopt a colour picked from the screen (or any external source).
    pub fn set_rgb(&mut self, rgb: [u8; 3]) {
        let (h, s, v) = rgb_to_hsv(rgb);
        self.h = h;
        self.s = s;
        self.v = v;
        self.hex_draft = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        p: &Palette,
        t: &crate::i18n::Strings,
    ) -> Option<PickerAction> {
        let mut action = None;
        let mut open = true;
        let mut pos = if self.first {
            let screen = ctx.content_rect();
            Pos2::new(
                screen.center().x - 260.0,
                (screen.center().y - 230.0).max(screen.top() + 40.0),
            )
        } else {
            Pos2::ZERO
        };
        if !self.first {
            pos = ctx
                .memory(|m| m.area_rect(egui::Id::new("pinsight-color-picker")))
                .map(|r| r.min)
                .unwrap_or(Pos2::new(240.0, 160.0));
        }
        self.first = false;

        let area = egui::Area::new(egui::Id::new("pinsight-color-picker"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(p.card)
                    .stroke(Stroke::new(1.0, p.stroke_strong))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::same(14))
                    .show(ui, |ui| {
                        ui.set_width(470.0);
                        self.body(ui, p, t, &mut action, &mut open);
                    });
            });
        let _ = area;

        if !open && action.is_none() {
            action = Some(PickerAction::Cancel);
        }
        action
    }

    fn body(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        t: &crate::i18n::Strings,
        action: &mut Option<PickerAction>,
        open: &mut bool,
    ) {
        // ---- title bar ----------------------------------------------------
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("{} — {}", t.color_pick_title, self.target.label(t)))
                    .size(14.0)
                    .strong()
                    .color(p.text),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::icon_button_enabled(ui, p, Icon::Close, false, t.key_cancel, true).clicked()
                {
                    *open = false;
                }
            });
        });
        ui.add_space(10.0);

        let preview = Color32::from_rgb(self.rgb()[0], self.rgb()[1], self.rgb()[2]);

        ui.horizontal_top(|ui| {
            // ---- left: basic + custom swatches ---------------------------
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(t.color_basic)
                        .size(11.5)
                        .color(p.text_weak),
                );
                ui.add_space(4.0);
                if let Some(rgb) = self.swatch_grid(ui, p, &BASIC, 8) {
                    self.set_rgb(rgb);
                }
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new(t.color_custom)
                        .size(11.5)
                        .color(p.text_weak),
                );
                ui.add_space(4.0);
                // Two rows of eight, empty slots drawn as white wells — that
                // is what makes the dialog read as "custom colours" instead of
                // "another palette".
                let mut slots = self.custom.clone();
                slots.resize(16, [0xFF, 0xFF, 0xFF]);
                let before = self.custom.len();
                if let Some(picked) = self.swatch_grid_indexed(ui, p, &slots, 8, before) {
                    self.set_rgb(picked);
                }
                ui.add_space(10.0);
                if ui::link_button(ui, p, Icon::Check, t.color_add_custom).clicked() {
                    let rgb = self.rgb();
                    self.custom.retain(|c| *c != rgb);
                    self.custom.push(rgb);
                    while self.custom.len() > 16 {
                        self.custom.remove(0);
                    }
                }
            });

            ui.add_space(16.0);

            // ---- right: HSV field + numbers ------------------------------
            ui.vertical(|ui| {
                self.sv_field(ui, p);
                ui.add_space(6.0);
                self.hue_strip(ui, p);
                ui.add_space(10.0);

                // Numbers, Windows style: two columns of label + spin box.
                egui::Grid::new("pinsight-color-numbers")
                    .num_columns(2)
                    .spacing([10.0, 6.0])
                    .show(ui, |ui| {
                        let mut hue = self.h;
                        if self
                            .num_row(ui, p, t.color_hue, &mut hue, 0.0, 360.0)
                            .changed()
                        {
                            self.h = hue.clamp(0.0, 359.9);
                        }
                        ui.end_row();
                        let mut sat = self.s * 100.0;
                        if self
                            .num_row(ui, p, t.color_sat, &mut sat, 0.0, 100.0)
                            .changed()
                        {
                            self.s = (sat / 100.0).clamp(0.0, 1.0);
                        }
                        ui.end_row();
                        let mut val = self.v * 100.0;
                        if self
                            .num_row(ui, p, t.color_val, &mut val, 0.0, 100.0)
                            .changed()
                        {
                            self.v = (val / 100.0).clamp(0.0, 1.0);
                        }
                        ui.end_row();

                        let mut rgb = self.rgb();
                        let mut touched = false;
                        for (index, name) in
                            [(0, t.color_red), (1, t.color_green), (2, t.color_blue)]
                        {
                            let mut channel = rgb[index] as f32;
                            if self
                                .num_row(ui, p, name, &mut channel, 0.0, 255.0)
                                .changed()
                            {
                                rgb[index] = channel.round().clamp(0.0, 255.0) as u8;
                                touched = true;
                            }
                            ui.end_row();
                        }
                        if touched {
                            self.set_rgb(rgb);
                        }

                        let mut alpha = self.alpha as f32;
                        if self
                            .num_row(ui, p, t.color_alpha, &mut alpha, 0.0, 255.0)
                            .changed()
                        {
                            self.alpha = alpha.round().clamp(0.0, 255.0) as u8;
                        }
                        ui.end_row();

                        // HTML field
                        ui.label(
                            egui::RichText::new(t.color_html)
                                .size(11.5)
                                .color(p.text_weak),
                        );
                        let response = ui.add(
                            egui::TextEdit::singleline(&mut self.hex_draft)
                                .desired_width(96.0)
                                .font(egui::TextStyle::Monospace),
                        );
                        if response.changed() {
                            if let Some(rgb) = parse_hex(&self.hex_draft) {
                                let (h, s, v) = rgb_to_hsv(rgb);
                                self.h = h;
                                self.s = s;
                                self.v = v;
                            }
                        }
                        ui.end_row();
                    });
            });
        });

        ui.add_space(12.0);

        // ---- footer -------------------------------------------------------
        ui.horizontal(|ui| {
            // Live preview strip: the colour as it will be applied.
            let (rect, _) = ui.allocate_exact_size(Vec2::new(64.0, 26.0), Sense::hover());
            ui.painter().rect_filled(rect, 6.0, preview);
            ui.painter().rect_stroke(
                rect,
                6.0,
                Stroke::new(1.0, p.stroke_strong),
                StrokeKind::Inside,
            );
            ui.add_space(6.0);
            if ui::link_button(ui, p, Icon::Focus, t.color_pick_screen).clicked() {
                *action = Some(PickerAction::PickScreen);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui::primary_button(ui, p, t.ok).clicked() {
                    *action = Some(PickerAction::Apply(preview));
                }
                ui.add_space(6.0);
                if ui::link_button(ui, p, Icon::Close, t.key_cancel).clicked() {
                    *open = false;
                }
            });
        });
    }

    /// 8-wide grid of colour wells. Returns the picked colour, if any.
    fn swatch_grid(
        &self,
        ui: &mut egui::Ui,
        p: &Palette,
        colors: &[[u8; 3]],
        columns: usize,
    ) -> Option<[u8; 3]> {
        self.swatch_grid_indexed(ui, p, colors, columns, colors.len())
    }

    fn swatch_grid_indexed(
        &self,
        ui: &mut egui::Ui,
        p: &Palette,
        colors: &[[u8; 3]],
        columns: usize,
        active: usize,
    ) -> Option<[u8; 3]> {
        let cell = 18.0;
        let gap = 3.0;
        let rows = colors.len().div_ceil(columns);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(
                columns as f32 * (cell + gap) - gap,
                rows as f32 * (cell + gap) - gap,
            ),
            Sense::hover(),
        );
        let mut picked = None;
        for (index, color) in colors.iter().enumerate() {
            let (row, column) = (index / columns, index % columns);
            let at = rect.min + Vec2::new(column as f32 * (cell + gap), row as f32 * (cell + gap));
            let well = Rect::from_min_size(at, Vec2::splat(cell));
            let response = ui.interact(
                well,
                egui::Id::new(("color-well", index, active)),
                Sense::click(),
            );
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            let fill = Color32::from_rgb(color[0], color[1], color[2]);
            ui.painter().rect_filled(well, 3.0, fill);
            let edge = if index >= active && active < colors.len() {
                // Unused custom slots get a dashed-looking hairline.
                with_alpha(p.text_weak, 90)
            } else {
                p.stroke_strong
            };
            ui.painter()
                .rect_stroke(well, 3.0, Stroke::new(1.0, edge), StrokeKind::Inside);
            if response.hovered() {
                ui.painter().rect_stroke(
                    well.expand(1.5),
                    4.0,
                    Stroke::new(2.0, p.accent),
                    StrokeKind::Outside,
                );
            }
            if response.clicked() && index < active {
                picked = Some(*color);
            }
        }
        picked
    }

    /// Saturation/value square with the hue applied.
    fn sv_field(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let size = Vec2::new(190.0, 150.0);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let response = response.on_hover_cursor(egui::CursorIcon::Crosshair);
        if response.dragged() || response.clicked() {
            self.dragging = Some(Drag::Field);
        }
        if response.drag_stopped()
            || (self.dragging == Some(Drag::Field) && !response.is_pointer_button_down_on())
        {
            self.dragging = None;
        }
        if self.dragging == Some(Drag::Field) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                self.s = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                self.v = 1.0 - ((pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
            }
        }

        // A mesh with one quad per cell: a plain 4-corner gradient would bend
        // the hue into the black corner and look wrong.
        const STEPS: usize = 16;
        let mut mesh = egui::Mesh::default();
        for row in 0..STEPS {
            for column in 0..STEPS {
                let s0 = column as f32 / STEPS as f32;
                let s1 = (column + 1) as f32 / STEPS as f32;
                let v1 = 1.0 - row as f32 / STEPS as f32;
                let v0 = 1.0 - (row + 1) as f32 / STEPS as f32;
                let pos = |s: f32, v: f32| {
                    Pos2::new(
                        rect.left() + s * rect.width(),
                        rect.top() + (1.0 - v) * rect.height(),
                    )
                };
                let col = |s: f32, v: f32| {
                    let rgb = hsv_to_rgb(self.h, s, v);
                    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
                };
                let base = mesh.vertices.len() as u32;
                mesh.colored_vertex(pos(s0, v0), col(s0, v0));
                mesh.colored_vertex(pos(s1, v0), col(s1, v0));
                mesh.colored_vertex(pos(s1, v1), col(s1, v1));
                mesh.colored_vertex(pos(s0, v1), col(s0, v1));
                mesh.add_triangle(base, base + 1, base + 2);
                mesh.add_triangle(base, base + 2, base + 3);
            }
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, p.stroke_strong),
            StrokeKind::Inside,
        );

        let knob = Pos2::new(
            rect.left() + self.s * rect.width(),
            rect.top() + (1.0 - self.v) * rect.height(),
        );
        ui.painter()
            .circle_stroke(knob, 5.5, Stroke::new(1.5, Color32::WHITE));
        ui.painter()
            .circle_stroke(knob, 6.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
    }

    fn hue_strip(&mut self, ui: &mut egui::Ui, p: &Palette) {
        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(190.0, 16.0), Sense::click_and_drag());
        let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if response.dragged() || response.clicked() {
            self.dragging = Some(Drag::Hue);
        }
        if response.drag_stopped()
            || (self.dragging == Some(Drag::Hue) && !response.is_pointer_button_down_on())
        {
            self.dragging = None;
        }
        if self.dragging == Some(Drag::Hue) {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                self.h =
                    (((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * 360.0).min(359.9);
            }
        }

        const STEPS: usize = 12;
        let mut mesh = egui::Mesh::default();
        for i in 0..STEPS {
            let h0 = i as f32 / STEPS as f32 * 360.0;
            let h1 = (i + 1) as f32 / STEPS as f32 * 360.0;
            let x0 = rect.left() + rect.width() * i as f32 / STEPS as f32;
            let x1 = rect.left() + rect.width() * (i + 1) as f32 / STEPS as f32;
            let c0 = hsv_to_rgb(h0, 1.0, 1.0);
            let c1 = hsv_to_rgb(h1.min(359.9), 1.0, 1.0);
            let base = mesh.vertices.len() as u32;
            mesh.colored_vertex(
                Pos2::new(x0, rect.top()),
                Color32::from_rgb(c0[0], c0[1], c0[2]),
            );
            mesh.colored_vertex(
                Pos2::new(x1, rect.top()),
                Color32::from_rgb(c1[0], c1[1], c1[2]),
            );
            mesh.colored_vertex(
                Pos2::new(x1, rect.bottom()),
                Color32::from_rgb(c1[0], c1[1], c1[2]),
            );
            mesh.colored_vertex(
                Pos2::new(x0, rect.bottom()),
                Color32::from_rgb(c0[0], c0[1], c0[2]),
            );
            mesh.add_triangle(base, base + 1, base + 2);
            mesh.add_triangle(base, base + 2, base + 3);
        }
        ui.painter().add(egui::Shape::mesh(mesh));
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0, p.stroke_strong),
            StrokeKind::Inside,
        );

        let x = rect.left() + (self.h / 360.0) * rect.width();
        let marker = Rect::from_center_size(
            Pos2::new(x, rect.center().y),
            Vec2::new(4.0, rect.height() + 4.0),
        );
        ui.painter().rect_stroke(
            marker,
            1.0,
            Stroke::new(1.5, Color32::WHITE),
            StrokeKind::Outside,
        );
        ui.painter().rect_stroke(
            marker,
            1.0,
            Stroke::new(1.0, Color32::from_black_alpha(140)),
            StrokeKind::Inside,
        );
    }

    fn num_row(
        &mut self,
        ui: &mut egui::Ui,
        p: &Palette,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
    ) -> egui::Response {
        ui.label(egui::RichText::new(label).size(11.5).color(p.text_weak));
        ui.add(
            egui::DragValue::new(value)
                .range(min..=max)
                .speed(1.0)
                .fixed_decimals(0),
        )
    }
}

/// `#RRGGBB` or `RRGGBB`, case-insensitive.
fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let value = u32::from_str_radix(hex, 16).ok()?;
    Some([
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
    ])
}

pub fn rgb_to_hsv(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d <= f32::EPSILON {
        0.0
    } else if max == r {
        (60.0 * ((g - b) / d)).rem_euclid(360.0)
    } else if max == g {
        60.0 * ((b - r) / d) + 120.0
    } else {
        60.0 * ((r - g) / d) + 240.0
    };
    let s = if max <= f32::EPSILON { 0.0 } else { d / max };
    (h, s, max)
}

pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match h as u32 / 60 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    [
        ((r + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((g + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        ((b + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips_primary_colours() {
        for rgb in [
            [255, 0, 0],
            [0, 255, 0],
            [0, 0, 255],
            [255, 255, 255],
            [0, 0, 0],
            [128, 64, 32],
        ] {
            let (h, s, v) = rgb_to_hsv(rgb);
            assert_eq!(hsv_to_rgb(h, s, v), rgb, "round trip for {rgb:?}");
        }
    }

    #[test]
    fn hex_parses_with_and_without_hash() {
        assert_eq!(parse_hex("#2080F0"), Some([0x20, 0x80, 0xF0]));
        assert_eq!(parse_hex("2080f0"), Some([0x20, 0x80, 0xF0]));
        assert_eq!(parse_hex("#20"), None);
    }
}
