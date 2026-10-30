//! Annotation model, live preview and rasterisation.
//!
//! Coordinates are always in *source image pixels*. The overlay converts from
//! egui points to image pixels exactly once, so annotations stay correct when
//! the preview is scaled to fit a monitor.

use eframe::egui;
use image::{Rgba, RgbaImage};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Rect,
    Ellipse,
    Arrow,
    Line,
    Pen,
    Highlight,
    Mosaic,
    Numbered,
    Text,
}

impl Tool {
    pub const ALL: [Tool; 10] = [
        Tool::Select,
        Tool::Rect,
        Tool::Ellipse,
        Tool::Arrow,
        Tool::Line,
        Tool::Pen,
        Tool::Highlight,
        Tool::Mosaic,
        Tool::Numbered,
        Tool::Text,
    ];

    pub fn shortcut(self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::Rect => "R",
            Tool::Ellipse => "E",
            Tool::Arrow => "A",
            Tool::Line => "L",
            Tool::Pen => "D",
            Tool::Highlight => "H",
            Tool::Mosaic => "M",
            Tool::Numbered => "N",
            Tool::Text => "T",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImgRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl ImgRect {
    pub fn from_points(a: egui::Pos2, b: egui::Pos2) -> Self {
        let x = a.x.min(b.x);
        let y = a.y.min(b.y);
        Self {
            x,
            y,
            w: (b.x - a.x).abs(),
            h: (b.y - a.y).abs(),
        }
    }

    pub fn from_min_size(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn center(&self) -> egui::Pos2 {
        egui::pos2(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(&self, p: egui::Pos2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.right() && p.y <= self.bottom()
    }

    pub fn normalized(&self) -> Self {
        Self {
            x: self.x.min(self.right()),
            y: self.y.min(self.bottom()),
            w: self.w.abs(),
            h: self.h.abs(),
        }
    }

    pub fn clamped(&self, width: u32, height: u32) -> Self {
        let n = self.normalized();
        let x = n.x.clamp(0.0, width as f32);
        let y = n.y.clamp(0.0, height as f32);
        let right = n.right().clamp(0.0, width as f32);
        let bottom = n.bottom().clamp(0.0, height as f32);
        Self {
            x,
            y,
            w: (right - x).max(0.0),
            h: (bottom - y).max(0.0),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Annotation {
    Rect {
        rect: ImgRect,
        color: [u8; 4],
        width: f32,
    },
    Ellipse {
        rect: ImgRect,
        color: [u8; 4],
        width: f32,
    },
    Arrow {
        from: egui::Pos2,
        to: egui::Pos2,
        color: [u8; 4],
        width: f32,
    },
    /// A plain straight line (no arrowhead).
    Line {
        from: egui::Pos2,
        to: egui::Pos2,
        color: [u8; 4],
        width: f32,
    },
    Pen {
        points: Vec<egui::Pos2>,
        color: [u8; 4],
        width: f32,
    },
    Highlight {
        rect: ImgRect,
    },
    Mosaic {
        rect: ImgRect,
        cell: u32,
    },
    /// A numbered step marker (Snipaste's `N` tool): a filled disc with the
    /// 1-based `index` drawn in seven-segment strokes, so it reads at any
    /// size without shipping a font to the rasteriser.
    Numbered {
        point: egui::Pos2,
        index: u32,
        color: [u8; 4],
    },
    /// Free text placed at `at` (top-left corner, image pixels), `size` being
    /// the font size in image pixels.
    Text {
        at: egui::Pos2,
        text: String,
        color: [u8; 4],
        size: f32,
    },
}

impl Annotation {
    pub fn translate(&mut self, delta: egui::Vec2) {
        match self {
            Annotation::Rect { rect, .. }
            | Annotation::Ellipse { rect, .. }
            | Annotation::Highlight { rect }
            | Annotation::Mosaic { rect, .. } => {
                rect.x += delta.x;
                rect.y += delta.y;
            }
            Annotation::Arrow { from, to, .. } | Annotation::Line { from, to, .. } => {
                *from += delta;
                *to += delta;
            }
            Annotation::Pen { points, .. } => {
                for p in points {
                    *p += delta;
                }
            }
            Annotation::Numbered { point, .. } => {
                *point += delta;
            }
            Annotation::Text { at, .. } => {
                *at += delta;
            }
        }
    }
}

/// Base radius of a numbered marker, in source-image pixels. Grows a little
/// with the digit count so multi-digit numbers stay legible.
pub fn marker_radius(index: u32) -> f32 {
    let digits = index.max(1).to_string().len() as f32;
    13.0 + 2.0 * (digits - 1.0)
}

/// Seven-segment masks for 0–9, bits `a`..`g` = 1, 2, 4, 8, 16, 32, 64.
const SEGMENT_MASKS: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];

/// Segment endpoints inside a 0..1 × 0..1 digit box.
const SEGMENT_ENDS: [(f32, f32, f32, f32); 7] = [
    (0.16, 0.06, 0.84, 0.06), // a — top
    (0.84, 0.17, 0.84, 0.46), // b — upper right
    (0.84, 0.54, 0.84, 0.83), // c — lower right
    (0.16, 0.94, 0.84, 0.94), // d — bottom
    (0.16, 0.54, 0.16, 0.83), // e — lower left
    (0.16, 0.17, 0.16, 0.46), // f — upper left
    (0.16, 0.50, 0.84, 0.50), // g — middle
];

/// Line segments that draw `index` centred in a `size` × `size` box at
/// `centre`. Shared by the egui preview and the tiny-skia rasteriser so the
/// two paths cannot drift apart.
pub fn number_segments(index: u32, centre: egui::Pos2, size: f32) -> Vec<(egui::Pos2, egui::Pos2)> {
    let text = index.max(1).to_string();
    let digits = text.len() as f32;
    let gap_ratio = 0.14;
    let digit_w = (0.74 * size / (digits + gap_ratio * (digits - 1.0))).min(0.54 * size);
    let digit_h = digit_w * 1.28;
    let total_w = digits * digit_w + (digits - 1.0) * gap_ratio * digit_w;
    let x0 = centre.x - total_w * 0.5;
    let y0 = centre.y - digit_h * 0.5;

    let mut out = Vec::new();
    for (i, ch) in text.chars().enumerate() {
        let digit = ch.to_digit(10).unwrap_or(0) as usize;
        let mask = SEGMENT_MASKS[digit.min(9)];
        let bx = x0 + i as f32 * digit_w * (1.0 + gap_ratio);
        for (bit, (x0s, y0s, x1s, y1s)) in SEGMENT_ENDS.iter().enumerate() {
            if mask & (1 << bit) != 0 {
                out.push((
                    egui::pos2(bx + x0s * digit_w, y0 + y0s * digit_h),
                    egui::pos2(bx + x1s * digit_w, y0 + y1s * digit_h),
                ));
            }
        }
    }
    out
}

pub const ACCENT: [u8; 4] = [255, 84, 96, 255];

/// Snapshot-based undo/redo over an annotation list.
///
/// Snapshots (rather than inverse operations) keep the semantics correct for
/// every mutation the overlay can perform — appending a shape, deleting the
/// last one, and translating all of them when the selection is dragged. The
/// list is tiny (a screenshot session rarely exceeds a few dozen shapes), so
/// cloning it per action is cheaper than the bookkeeping an inverse model
/// would need.
#[derive(Debug, Default)]
pub struct History {
    past: Vec<Vec<Annotation>>,
    future: Vec<Vec<Annotation>>,
}

impl History {
    /// Deepest history kept; a long session cannot grow this without bound.
    const LIMIT: usize = 64;

    /// Call *before* mutating the list, with its current contents.
    pub fn record(&mut self, current: &[Annotation]) {
        if self.past.len() >= Self::LIMIT {
            self.past.remove(0);
        }
        self.past.push(current.to_vec());
        self.future.clear();
    }

    /// Restore the previous state, if any. `current` becomes the redo target.
    pub fn undo(&mut self, current: &[Annotation]) -> Option<Vec<Annotation>> {
        let previous = self.past.pop()?;
        self.future.push(current.to_vec());
        Some(previous)
    }

    /// Re-apply the state undone by [`Self::undo`], if any.
    pub fn redo(&mut self, current: &[Annotation]) -> Option<Vec<Annotation>> {
        let next = self.future.pop()?;
        self.past.push(current.to_vec());
        Some(next)
    }

    /// Forget everything — used when a new selection invalidates the past.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }
}

pub fn color32(color: [u8; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3])
}

fn to_skia_color(color: [u8; 4]) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(color[0], color[1], color[2], color[3])
}

/// Draw an annotation onto the egui overlay. `img_rect` is the on-screen rect
/// of the complete source image; `scale` converts image pixels to points.
pub fn preview(
    painter: &egui::Painter,
    annotation: &Annotation,
    img_rect: egui::Rect,
    scale: f32,
    source: &RgbaImage,
) {
    let map = |p: egui::Pos2| img_rect.min + p.to_vec2() * scale;
    match annotation {
        Annotation::Rect { rect, color, width } => {
            painter.rect_stroke(
                egui::Rect::from_min_size(map(egui::pos2(rect.x, rect.y)), rect_size(*rect, scale)),
                0.0,
                egui::Stroke::new((*width * scale).max(1.0), color32(*color)),
                egui::StrokeKind::Middle,
            );
        }
        Annotation::Ellipse { rect, color, width } => {
            let center = map(rect.center());
            let radius = egui::vec2(rect.w * 0.5 * scale, rect.h * 0.5 * scale);
            painter.add(egui::Shape::ellipse_stroke(
                center,
                radius,
                egui::Stroke::new((*width * scale).max(1.0), color32(*color)),
            ));
        }
        Annotation::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let a = map(*from);
            let b = map(*to);
            let stroke = egui::Stroke::new((*width * scale).max(1.0), color32(*color));
            painter.line_segment([a, b], stroke);
            let dir = (b - a).normalized();
            if dir.length() > 0.0 {
                let head = 14.0 * scale;
                let left = b - dir * head + egui::vec2(-dir.y, dir.x) * head * 0.55;
                let right = b - dir * head + egui::vec2(dir.y, -dir.x) * head * 0.55;
                painter.add(egui::Shape::convex_polygon(
                    vec![b, left, right],
                    color32(*color),
                    egui::Stroke::NONE,
                ));
            }
        }
        Annotation::Line {
            from,
            to,
            color,
            width,
        } => {
            painter.line_segment(
                [map(*from), map(*to)],
                egui::Stroke::new((*width * scale).max(1.0), color32(*color)),
            );
        }
        Annotation::Pen {
            points,
            color,
            width,
        } => {
            let pts: Vec<egui::Pos2> = points.iter().map(|p| map(*p)).collect();
            if pts.len() > 1 {
                painter.add(egui::Shape::line(
                    pts,
                    egui::Stroke::new((*width * scale).max(1.0), color32(*color)),
                ));
            }
        }
        Annotation::Highlight { rect } => {
            painter.rect_filled(
                egui::Rect::from_min_size(map(egui::pos2(rect.x, rect.y)), rect_size(*rect, scale)),
                0.0,
                egui::Color32::from_rgba_unmultiplied(255, 220, 80, 90),
            );
        }
        Annotation::Mosaic { rect, cell } => {
            let cell = (*cell).max(4) as f32;
            let mut y = rect.y;
            while y < rect.bottom() {
                let mut x = rect.x;
                while x < rect.right() {
                    let sx = (x + cell * 0.5).clamp(0.0, source.width() as f32 - 1.0) as u32;
                    let sy = (y + cell * 0.5).clamp(0.0, source.height() as f32 - 1.0) as u32;
                    let px = source.get_pixel(sx, sy);
                    let screen = egui::Rect::from_min_size(
                        map(egui::pos2(x, y)),
                        egui::vec2(cell * scale + 0.5, cell * scale + 0.5),
                    );
                    painter.rect_filled(
                        screen,
                        0.0,
                        egui::Color32::from_rgba_unmultiplied(px[0], px[1], px[2], px[3]),
                    );
                    x += cell;
                }
                y += cell;
            }
        }
        Annotation::Numbered {
            point,
            index,
            color,
        } => {
            let centre = map(*point);
            let r = marker_radius(*index) * scale;
            painter.circle_filled(centre, r, color32(*color));
            let stroke = egui::Stroke::new((r * 0.30).max(1.0), egui::Color32::WHITE);
            for (a, b) in number_segments(*index, centre, r * 2.0) {
                painter.line_segment([a, b], stroke);
            }
        }
        Annotation::Text {
            at,
            text,
            color,
            size,
        } => {
            // egui renders the preview with the UI's own font stack; the
            // rasteriser outlines the same file, so the two stay close.
            painter.text(
                map(*at),
                egui::Align2::LEFT_TOP,
                text.clone(),
                egui::FontId::proportional(size * scale),
                color32(*color),
            );
        }
    }
}

fn rect_size(rect: ImgRect, scale: f32) -> egui::Vec2 {
    egui::vec2(rect.w * scale, rect.h * scale)
}

/// Rasterise all annotations into a cropped copy of `source`. `crop` must be
/// inside the source image. Mosaics are applied last, which matches the
/// intuitive "redact the final result" behaviour.
pub fn rasterize(source: &RgbaImage, crop: ImgRect, annotations: &[Annotation]) -> RgbaImage {
    let crop = crop.clamped(source.width(), source.height());
    let x = crop.x.round() as u32;
    let y = crop.y.round() as u32;
    let w = crop.w.round().max(1.0) as u32;
    let h = crop.h.round().max(1.0) as u32;

    let mut out = image::imageops::crop_imm(
        source,
        x,
        y,
        w.min(source.width() - x),
        h.min(source.height() - y),
    )
    .to_image();
    let (ow, oh) = out.dimensions();
    if ow == 0 || oh == 0 {
        return out;
    }

    let mut pixmap = Pixmap::new(ow, oh).expect("valid pixmap size");
    pixmap.data_mut().copy_from_slice(out.as_raw());

    for annotation in annotations {
        draw_annotation(&mut pixmap, annotation, crop.x, crop.y);
    }

    for annotation in annotations {
        if let Annotation::Mosaic { rect, cell } = annotation {
            apply_mosaic(&mut out, rect, *cell, crop.x, crop.y);
        }
    }

    // Vector annotations were drawn on the pixmap; mosaics were applied to
    // `out`. Merge mosaic results back, keeping vector art elsewhere.
    if annotations
        .iter()
        .any(|a| matches!(a, Annotation::Mosaic { .. }))
    {
        // Rebuild the final image: start from the mosaic-processed base, then
        // replay only the vector annotations.
        let mut final_pixmap = Pixmap::new(ow, oh).expect("valid pixmap size");
        final_pixmap.data_mut().copy_from_slice(out.as_raw());
        for annotation in annotations {
            if !matches!(annotation, Annotation::Mosaic { .. }) {
                draw_annotation(&mut final_pixmap, annotation, crop.x, crop.y);
            }
        }
        if let Some(image) = RgbaImage::from_raw(ow, oh, final_pixmap.data().to_vec()) {
            out = image;
        }
    } else if let Some(image) = RgbaImage::from_raw(ow, oh, pixmap.data().to_vec()) {
        out = image;
    }

    out
}

fn draw_annotation(pixmap: &mut Pixmap, annotation: &Annotation, ox: f32, oy: f32) {
    match annotation {
        Annotation::Rect { rect, color, width } => {
            let r = tiny_skia::Rect::from_xywh(
                rect.x - ox,
                rect.y - oy,
                rect.w.max(1.0),
                rect.h.max(1.0),
            );
            if let Some(r) = r {
                let path = PathBuilder::from_rect(r);
                let mut paint = Paint::default();
                paint.set_color(to_skia_color(*color));
                paint.anti_alias = true;
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: *width,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
        Annotation::Ellipse { rect, color, width } => {
            if let Some(r) = tiny_skia::Rect::from_xywh(
                rect.x - ox,
                rect.y - oy,
                rect.w.max(1.0),
                rect.h.max(1.0),
            ) {
                if let Some(path) = PathBuilder::from_oval(r) {
                    let mut paint = Paint::default();
                    paint.set_color(to_skia_color(*color));
                    paint.anti_alias = true;
                    pixmap.stroke_path(
                        &path,
                        &paint,
                        &Stroke {
                            width: *width,
                            ..Default::default()
                        },
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        Annotation::Line {
            from,
            to,
            color,
            width,
        } => {
            let a = tiny_skia::Point::from_xy(from.x - ox, from.y - oy);
            let b = tiny_skia::Point::from_xy(to.x - ox, to.y - oy);
            let mut paint = Paint::default();
            paint.set_color(to_skia_color(*color));
            paint.anti_alias = true;
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: *width,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
        Annotation::Arrow {
            from,
            to,
            color,
            width,
        } => {
            let a = tiny_skia::Point::from_xy(from.x - ox, from.y - oy);
            let b = tiny_skia::Point::from_xy(to.x - ox, to.y - oy);
            let mut paint = Paint::default();
            paint.set_color(to_skia_color(*color));
            paint.anti_alias = true;
            let mut pb = PathBuilder::new();
            pb.move_to(a.x, a.y);
            pb.line_to(b.x, b.y);
            if let Some(path) = pb.finish() {
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: *width,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
            let dx = b.x - a.x;
            let dy = b.y - a.y;
            let len = (dx * dx + dy * dy).sqrt();
            if len > 0.1 {
                let ux = dx / len;
                let uy = dy / len;
                let head = (width * 4.5).max(12.0);
                let p1 = tiny_skia::Point::from_xy(
                    b.x - ux * head - uy * head * 0.55,
                    b.y - uy * head + ux * head * 0.55,
                );
                let p2 = tiny_skia::Point::from_xy(
                    b.x - ux * head + uy * head * 0.55,
                    b.y - uy * head - ux * head * 0.55,
                );
                let mut pb = PathBuilder::new();
                pb.move_to(b.x, b.y);
                pb.line_to(p1.x, p1.y);
                pb.line_to(p2.x, p2.y);
                pb.close();
                if let Some(path) = pb.finish() {
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
        }
        Annotation::Pen {
            points,
            color,
            width,
        } => {
            if points.len() < 2 {
                return;
            }
            let mut pb = PathBuilder::new();
            pb.move_to(points[0].x - ox, points[0].y - oy);
            for p in &points[1..] {
                pb.line_to(p.x - ox, p.y - oy);
            }
            if let Some(path) = pb.finish() {
                let mut paint = Paint::default();
                paint.set_color(to_skia_color(*color));
                paint.anti_alias = true;
                pixmap.stroke_path(
                    &path,
                    &paint,
                    &Stroke {
                        width: *width,
                        ..Default::default()
                    },
                    Transform::identity(),
                    None,
                );
            }
        }
        Annotation::Highlight { rect } => {
            if let Some(r) = tiny_skia::Rect::from_xywh(
                rect.x - ox,
                rect.y - oy,
                rect.w.max(1.0),
                rect.h.max(1.0),
            ) {
                let path = PathBuilder::from_rect(r);
                let mut paint = Paint::default();
                paint.set_color(to_skia_color([255, 220, 80, 90]));
                paint.anti_alias = true;
                pixmap.fill_path(
                    &path,
                    &paint,
                    FillRule::Winding,
                    Transform::identity(),
                    None,
                );
            }
        }
        Annotation::Mosaic { .. } => {}
        Annotation::Numbered {
            point,
            index,
            color,
        } => {
            let cx = point.x - ox;
            let cy = point.y - oy;
            let r = marker_radius(*index);
            if let Some(disc) = tiny_skia::Rect::from_xywh(cx - r, cy - r, r * 2.0, r * 2.0) {
                if let Some(path) = PathBuilder::from_oval(disc) {
                    let mut paint = Paint::default();
                    paint.set_color(to_skia_color(*color));
                    paint.anti_alias = true;
                    pixmap.fill_path(
                        &path,
                        &paint,
                        FillRule::Winding,
                        Transform::identity(),
                        None,
                    );
                }
            }
            // Seven-segment digits, white, stroked like the egui preview.
            let mut digit_paint = Paint::default();
            digit_paint.set_color(tiny_skia::Color::WHITE);
            digit_paint.anti_alias = true;
            let stroke = Stroke {
                width: (r * 0.30).max(1.0),
                line_cap: tiny_skia::LineCap::Round,
                ..Default::default()
            };
            for (a, b) in number_segments(*index, egui::pos2(cx, cy), r * 2.0) {
                let mut builder = PathBuilder::new();
                builder.move_to(a.x, a.y);
                builder.line_to(b.x, b.y);
                if let Some(path) = builder.finish() {
                    pixmap.stroke_path(&path, &digit_paint, &stroke, Transform::identity(), None);
                }
            }
        }
        Annotation::Text {
            at,
            text,
            color,
            size,
        } => {
            // Glyph outlines from the same font file the UI renders with.
            crate::text_font::font().draw(pixmap, text, at.x - ox, at.y - oy, *size, *color);
        }
    }
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;

    #[test]
    fn rasterizes_into_cropped_output() {
        let source = RgbaImage::from_pixel(40, 40, Rgba([250, 250, 250, 255]));
        let annotation = Annotation::Rect {
            rect: ImgRect::from_min_size(4.0, 4.0, 12.0, 12.0),
            color: [255, 0, 0, 255],
            width: 2.0,
        };
        let out = rasterize(
            &source,
            ImgRect::from_min_size(0.0, 0.0, 24.0, 24.0),
            &[annotation],
        );
        assert_eq!(out.dimensions(), (24, 24));
        assert!(out.pixels().any(|p| p[0] > 200 && p[1] < 100));
    }

    #[test]
    fn mosaic_changes_region() {
        let source =
            RgbaImage::from_fn(30, 30, |x, y| Rgba([(x * 8) as u8, (y * 8) as u8, 0, 255]));
        let annotation = Annotation::Mosaic {
            rect: ImgRect::from_min_size(5.0, 5.0, 10.0, 10.0),
            cell: 4,
        };
        let out = rasterize(
            &source,
            ImgRect::from_min_size(0.0, 0.0, 30.0, 30.0),
            &[annotation],
        );
        assert_ne!(out.get_pixel(5, 5), source.get_pixel(5, 5));
    }

    fn rect_at(x: f32) -> Annotation {
        Annotation::Rect {
            rect: ImgRect::from_min_size(x, 4.0, 8.0, 8.0),
            color: [255, 0, 0, 255],
            width: 2.0,
        }
    }

    #[test]
    fn undo_redo_round_trip() {
        let mut history = History::default();
        let first = vec![rect_at(0.0)];
        let second = vec![rect_at(0.0), rect_at(20.0)];

        // Nothing recorded yet: both directions are no-ops.
        assert!(!history.can_undo());
        assert!(history.undo(&second).is_none());
        assert!(history.redo(&first).is_none());

        history.record(&first);
        assert!(history.can_undo());
        let back = history.undo(&second).expect("undo returns the snapshot");
        assert_eq!(back.len(), 1);

        assert!(history.can_redo());
        let forward = history.redo(&back).expect("redo re-applies");
        assert_eq!(forward.len(), 2);

        // Undoing consumed the redo branch.
        assert!(!history.can_redo());
    }

    #[test]
    fn recording_clears_the_redo_branch() {
        let mut history = History::default();
        let a = vec![rect_at(0.0)];
        let b = vec![rect_at(0.0), rect_at(20.0)];
        history.record(&a);
        let back = history.undo(&b).unwrap();
        assert!(history.can_redo());

        // A new action after an undo starts a fresh future.
        history.record(&back);
        assert!(!history.can_redo());
    }

    #[test]
    fn history_is_capped_and_clearable() {
        let mut history = History::default();
        let list = vec![rect_at(0.0)];
        for _ in 0..(History::LIMIT + 8) {
            history.record(&list);
        }
        assert!(history.past.len() <= History::LIMIT);

        history.clear();
        assert!(!history.can_undo());
        assert!(!history.can_redo());
    }

    #[test]
    fn number_segments_fit_their_box() {
        for index in [1u32, 5, 9, 10, 42, 137] {
            let size = 26.0;
            let centre = egui::pos2(100.0, 100.0);
            let segs = number_segments(index, centre, size);
            assert!(!segs.is_empty(), "index {index} drew no segments");
            let half = size * 0.5;
            for (a, b) in &segs {
                for p in [a, b] {
                    assert!(
                        (p.x - centre.x).abs() <= half + 0.01
                            && (p.y - centre.y).abs() <= half + 0.01,
                        "index {index} segment escaped the marker box"
                    );
                }
            }
        }
        // Different digits must produce different strokes.
        assert_ne!(
            number_segments(1, egui::pos2(0.0, 0.0), 26.0).len(),
            number_segments(8, egui::pos2(0.0, 0.0), 26.0).len()
        );
    }

    #[test]
    fn marker_radius_grows_with_digits() {
        assert!(marker_radius(9) < marker_radius(10));
        assert!(marker_radius(10) < marker_radius(100));
        assert!(marker_radius(0) > 0.0);
    }

    #[test]
    fn rasterizes_numbered_marker() {
        let source = RgbaImage::from_pixel(60, 60, Rgba([250, 250, 250, 255]));
        let annotation = Annotation::Numbered {
            point: egui::pos2(30.0, 30.0),
            index: 3,
            color: [255, 84, 96, 255],
        };
        let out = rasterize(
            &source,
            ImgRect::from_min_size(0.0, 0.0, 60.0, 60.0),
            &[annotation],
        );
        // The disc covers the centre in the annotation colour …
        assert!(out.pixels().any(|p| p[0] > 200 && p[1] < 130));
        // … and the white digits punch a hole in it.
        assert!(out.pixels().any(|p| p[0] > 240 && p[1] > 240 && p[2] > 240));
    }
}

fn apply_mosaic(out: &mut RgbaImage, rect: &ImgRect, cell: u32, ox: f32, oy: f32) {
    let r = rect.normalized();
    let cell = cell.max(2);
    let x0 = (r.x - ox).max(0.0) as u32;
    let y0 = (r.y - oy).max(0.0) as u32;
    let x1 = ((r.right() - ox).min(out.width() as f32)) as u32;
    let y1 = ((r.bottom() - oy).min(out.height() as f32)) as u32;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut y = y0;
    while y < y1 {
        let mut x = x0;
        while x < x1 {
            let bx1 = (x + cell).min(x1);
            let by1 = (y + cell).min(y1);
            let mut sum = [0u64; 4];
            let mut count = 0u64;
            for py in y..by1 {
                for px in x..bx1 {
                    let p = out.get_pixel(px, py);
                    for c in 0..4 {
                        sum[c] += p[c] as u64;
                    }
                    count += 1;
                }
            }
            let count = count.max(1);
            let avg = Rgba([
                (sum[0] / count) as u8,
                (sum[1] / count) as u8,
                (sum[2] / count) as u8,
                (sum[3] / count) as u8,
            ]);
            for py in y..by1 {
                for px in x..bx1 {
                    out.put_pixel(px, py, avg);
                }
            }
            x += cell;
        }
        y += cell;
    }
}
