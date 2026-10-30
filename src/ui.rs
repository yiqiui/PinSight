//! Hand-painted widgets and vector icons.
//!
//! egui's stock widgets are deliberately plain. PinSight draws its own tiles,
//! pills and icon set so the interface reads as a precise instrument instead of
//! a debug panel. Everything here is painted with the `egui::Painter`, so it
//! scales with the display DPI and needs no image assets.

use eframe::egui::{
    self, Align2, Color32, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{Palette, with_alpha};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)] // a small icon library: not every glyph is used by every page
pub enum Icon {
    Crop,
    Monitor,
    Pin,
    Scroll,
    Record,
    Save,
    Settings,
    Layers,
    Palette,
    Keyboard,
    Info,
    Image,
    Close,
    Copy,
    Focus,
    Through,
    Globe,
    Type,
    Folder,
    Sun,
    Moon,
    Desktop,
    Trash,
    Check,
    Reset,
    ChevronDown,
    // annotation tools
    Cursor,
    Square,
    Circle,
    ArrowUpRight,
    Line,
    Pencil,
    Marker,
    Mosaic,
    Undo,
    Redo,
    Numbered,
    /// Text recognition (Windows.Media.Ocr).
    Ocr,
    Eye,
    EyeOff,
    /// Filmstrip, for the GIF editor.
    Film,
}

/// Paint an icon inside `rect` (a square-ish box) using a 24×24 design grid.
pub fn paint_icon(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32, weight: f32) {
    let k = rect.width().min(rect.height()) / 24.0;
    let o = rect.min;
    let pt = |x: f32, y: f32| o + Vec2::new(x * k, y * k);
    let w = (weight * k).max(1.0);
    let stroke = Stroke::new(w, color);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([pt(a.0, a.1), pt(b.0, b.1)], stroke);
    };
    let path = |pts: &[(f32, f32)], closed: bool| {
        let points: Vec<Pos2> = pts.iter().map(|(x, y)| pt(*x, *y)).collect();
        painter.add(egui::Shape::line(points, Stroke::new(w, color)));
        if closed {
            let _ = closed;
        }
    };
    let boxed = |x0: f32, y0: f32, x1: f32, y1: f32, r: f32| {
        painter.rect_stroke(
            Rect::from_min_max(pt(x0, y0), pt(x1, y1)),
            r * k,
            stroke,
            StrokeKind::Middle,
        );
    };

    match icon {
        Icon::Crop => {
            // Four corner brackets: the universal "select an area" glyph.
            path(&[(3.5, 9.0), (3.5, 3.5), (9.0, 3.5)], false);
            path(&[(15.0, 3.5), (20.5, 3.5), (20.5, 9.0)], false);
            path(&[(3.5, 15.0), (3.5, 20.5), (9.0, 20.5)], false);
            path(&[(15.0, 20.5), (20.5, 20.5), (20.5, 15.0)], false);
        }
        Icon::Monitor => {
            boxed(2.5, 4.0, 21.5, 17.0, 2.5);
            line((12.0, 17.0), (12.0, 20.5));
            line((8.0, 20.5), (16.0, 20.5));
        }
        Icon::Pin => {
            // A pushpin seen from the side — head bar, body, needle. The old
            // circle-with-a-tail read as a map marker, not "pin this image".
            painter.rect_filled(
                Rect::from_min_max(pt(8.2, 2.8), pt(15.8, 5.0)),
                0.8 * k,
                color,
            );
            line((9.6, 5.0), (9.6, 11.6));
            line((14.4, 5.0), (14.4, 11.6));
            line((7.4, 11.6), (16.6, 11.6));
            line((12.0, 11.6), (12.0, 20.6));
            line((12.0, 20.6), (11.1, 21.2));
        }
        Icon::Scroll => {
            // Tall page with a double arrow: scroll, capture, stitch. The old
            // single down-arrow read as "download".
            boxed(5.0, 2.5, 19.0, 21.5, 2.5);
            line((12.0, 7.2), (12.0, 16.8));
            path(&[(9.5, 9.6), (12.0, 7.0), (14.5, 9.6)], false);
            path(&[(9.5, 14.4), (12.0, 17.0), (14.5, 14.4)], false);
        }
        Icon::Record => {
            painter.circle_stroke(pt(12.0, 12.0), 8.2 * k, stroke);
            painter.circle_filled(pt(12.0, 12.0), 3.6 * k, color);
        }
        Icon::Save => {
            line((12.0, 3.0), (12.0, 14.5));
            path(&[(7.6, 10.2), (12.0, 14.6), (16.4, 10.2)], false);
            line((4.5, 20.0), (19.5, 20.0));
        }
        Icon::Settings => {
            painter.circle_stroke(pt(12.0, 12.0), 3.4 * k, stroke);
            painter.circle_stroke(pt(12.0, 12.0), 7.2 * k, Stroke::new(w * 0.8, color));
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (s, c) = a.sin_cos();
                line(
                    (12.0 + c * 7.2, 12.0 + s * 7.2),
                    (12.0 + c * 9.6, 12.0 + s * 9.6),
                );
            }
        }
        Icon::Layers => {
            // A stack of pinned frames (photo mark inside), not the generic
            // "layers" diamond: the nav entry is about pinned screenshots.
            boxed(2.8, 6.2, 15.6, 19.8, 2.6);
            boxed(8.4, 2.6, 21.2, 16.2, 2.6);
            painter.circle_filled(pt(12.4, 7.4), 1.2 * k, color);
            path(
                &[
                    (9.6, 14.4),
                    (12.2, 11.6),
                    (14.2, 13.8),
                    (15.6, 12.4),
                    (18.6, 15.0),
                ],
                false,
            );
        }
        Icon::Palette => {
            painter.circle_stroke(pt(12.0, 12.0), 8.6 * k, stroke);
            painter.circle_filled(pt(9.0, 9.5), 1.5 * k, color);
            painter.circle_filled(pt(15.0, 9.5), 1.5 * k, color);
            painter.circle_filled(pt(9.0, 15.0), 1.5 * k, color);
        }
        Icon::Keyboard => {
            boxed(2.0, 6.0, 22.0, 18.0, 2.5);
            for x in [6.0_f32, 9.5, 13.0, 16.5] {
                line((x, 10.0), (x + 0.01, 10.0));
            }
            line((7.0, 14.0), (17.0, 14.0));
        }
        Icon::Info => {
            painter.circle_stroke(pt(12.0, 12.0), 8.6 * k, stroke);
            line((12.0, 10.6), (12.0, 16.4));
            painter.circle_filled(pt(12.0, 7.8), 1.1 * k, color);
        }
        Icon::Image => {
            // Picture frame with a sun and a hill — "background image".
            boxed(3.0, 4.5, 21.0, 19.5, 2.5);
            painter.circle_stroke(pt(8.6, 9.4), 1.9 * k, Stroke::new(w * 0.8, color));
            path(&[(4.6, 17.2), (10.0, 11.2), (14.4, 15.4)], false);
            path(&[(12.4, 15.4), (16.4, 11.0), (19.6, 14.4)], false);
        }
        Icon::Close => {
            line((6.5, 6.5), (17.5, 17.5));
            line((17.5, 6.5), (6.5, 17.5));
        }
        Icon::Copy => {
            boxed(8.5, 3.0, 21.0, 15.5, 2.5);
            path(
                &[
                    (15.5, 18.5),
                    (15.5, 21.0),
                    (3.0, 21.0),
                    (3.0, 8.5),
                    (5.5, 8.5),
                ],
                false,
            );
        }
        Icon::Focus => {
            painter.circle_stroke(pt(12.0, 12.0), 7.6 * k, stroke);
            painter.circle_filled(pt(12.0, 12.0), 2.2 * k, color);
            line((12.0, 1.5), (12.0, 4.5));
            line((12.0, 19.5), (12.0, 22.5));
            line((1.5, 12.0), (4.5, 12.0));
            line((19.5, 12.0), (22.5, 12.0));
        }
        Icon::Through => {
            line((2.5, 12.0), (18.0, 12.0));
            path(&[(14.4, 8.4), (18.0, 12.0), (14.4, 15.6)], false);
            line((20.0, 5.0), (20.0, 19.0));
        }
        Icon::Globe => {
            painter.circle_stroke(pt(12.0, 12.0), 8.6 * k, stroke);
            painter.add(egui::Shape::line(
                (0..=24)
                    .map(|i| {
                        let t = i as f32 / 24.0;
                        let y = 3.4 + t * 17.2;
                        let x = 12.0 - (1.0 - (t * 2.0 - 1.0).powi(2)).max(0.0).sqrt() * 8.6;
                        pt(x, y)
                    })
                    .collect(),
                stroke,
            ));
            painter.add(egui::Shape::line(
                (0..=24)
                    .map(|i| {
                        let t = i as f32 / 24.0;
                        let y = 3.4 + t * 17.2;
                        let x = 12.0 + (1.0 - (t * 2.0 - 1.0).powi(2)).max(0.0).sqrt() * 8.6;
                        pt(x, y)
                    })
                    .collect(),
                stroke,
            ));
            line((3.4, 12.0), (20.6, 12.0));
        }
        Icon::Type => {
            line((4.5, 5.5), (19.5, 5.5));
            line((12.0, 5.5), (12.0, 19.0));
            line((8.0, 19.0), (16.0, 19.0));
        }
        Icon::Folder => {
            path(
                &[
                    (2.8, 6.5),
                    (9.5, 6.5),
                    (11.5, 9.0),
                    (21.2, 9.0),
                    (21.2, 18.5),
                    (2.8, 18.5),
                    (2.8, 6.5),
                ],
                true,
            );
        }
        Icon::Sun => {
            painter.circle_stroke(pt(12.0, 12.0), 4.4 * k, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                let (s, c) = a.sin_cos();
                line(
                    (12.0 + c * 6.6, 12.0 + s * 6.6),
                    (12.0 + c * 9.4, 12.0 + s * 9.4),
                );
            }
        }
        Icon::Moon => {
            let mut pts = Vec::new();
            for i in 0..=28 {
                let t = i as f32 / 28.0;
                let a = std::f32::consts::PI * 0.55 + t * std::f32::consts::PI * 1.55;
                pts.push(pt(11.0 + a.cos() * 8.4, 12.0 + a.sin() * 8.4));
            }
            for i in 0..=28 {
                let t = i as f32 / 28.0;
                let a = std::f32::consts::PI * 2.1 - t * std::f32::consts::PI * 1.55;
                pts.push(pt(15.4 + a.cos() * 6.4, 12.0 + a.sin() * 6.4));
            }
            painter.add(egui::Shape::line(pts, stroke));
        }
        Icon::Desktop => {
            boxed(2.5, 4.0, 21.5, 16.0, 2.5);
            line((8.5, 20.0), (15.5, 20.0));
            line((12.0, 16.0), (12.0, 20.0));
            line((7.0, 10.0), (17.0, 10.0));
        }
        Icon::Trash => {
            line((4.0, 6.5), (20.0, 6.5));
            path(&[(9.0, 6.5), (9.0, 4.0), (15.0, 4.0), (15.0, 6.5)], false);
            path(&[(5.5, 6.5), (6.8, 20.5), (17.2, 20.5), (18.5, 6.5)], false);
            line((10.0, 10.0), (10.6, 17.5));
            line((14.0, 10.0), (13.4, 17.5));
        }
        Icon::Check => {
            path(&[(5.0, 12.5), (10.0, 17.5), (19.0, 6.5)], false);
        }
        Icon::Reset => {
            // Circular arrow: "back to defaults".
            let mut pts = Vec::new();
            for i in 0..=22 {
                let a =
                    -std::f32::consts::FRAC_PI_2 + i as f32 / 22.0 * std::f32::consts::TAU * 0.82;
                pts.push(pt(12.0 + a.cos() * 7.4, 12.0 + a.sin() * 7.4));
            }
            painter.add(egui::Shape::line(pts, stroke));
            path(&[(12.6, 1.6), (12.2, 6.0), (16.4, 6.6)], false);
        }
        Icon::ChevronDown => {
            path(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)], false);
        }
        Icon::Cursor => {
            path(
                &[
                    (6.0, 3.5),
                    (6.0, 18.5),
                    (10.0, 14.8),
                    (12.6, 20.5),
                    (15.2, 19.4),
                    (12.6, 13.8),
                    (18.0, 13.8),
                    (6.0, 3.5),
                ],
                true,
            );
        }
        Icon::Square => {
            boxed(4.0, 4.0, 20.0, 20.0, 2.0);
        }
        Icon::Circle => {
            painter.circle_stroke(pt(12.0, 12.0), 8.0 * k, stroke);
        }
        Icon::ArrowUpRight => {
            line((5.0, 19.0), (18.5, 5.5));
            path(&[(11.5, 5.5), (18.5, 5.5), (18.5, 12.5)], false);
        }
        Icon::Line => {
            line((4.5, 19.5), (19.5, 4.5));
        }
        Icon::Pencil => {
            path(
                &[
                    (3.5, 20.5),
                    (5.0, 15.8),
                    (15.6, 5.2),
                    (18.8, 8.4),
                    (8.2, 19.0),
                    (3.5, 20.5),
                ],
                true,
            );
            line((13.4, 7.4), (16.6, 10.6));
        }
        Icon::Marker => {
            painter.add(egui::Shape::line(
                vec![pt(7.5, 16.5), pt(17.0, 7.0)],
                Stroke::new(w * 2.4, color),
            ));
            line((4.0, 20.0), (9.0, 20.0));
        }
        Icon::Mosaic => {
            for (i, j) in [
                (0, 0),
                (1, 0),
                (2, 0),
                (0, 1),
                (2, 1),
                (0, 2),
                (1, 2),
                (2, 2),
            ] {
                let x = 4.0 + i as f32 * 5.6;
                let y = 4.0 + j as f32 * 5.6;
                painter.rect_filled(
                    Rect::from_min_size(pt(x, y), Vec2::splat(4.0 * k)),
                    1.0 * k,
                    color,
                );
            }
        }
        Icon::Undo => {
            // Sweeping arrow pointing left, tail down the right side.
            path(
                &[
                    (19.0, 6.5),
                    (16.2, 3.6),
                    (11.8, 2.5),
                    (7.4, 3.6),
                    (4.8, 6.8),
                ],
                false,
            );
            path(&[(4.8, 6.8), (8.8, 4.6)], false);
            path(&[(4.8, 6.8), (7.2, 9.8)], false);
            line((19.0, 6.5), (19.0, 17.5));
        }
        Icon::Redo => {
            // Mirror of `Undo`: arrow pointing right, tail down the left side.
            path(
                &[
                    (5.0, 6.5),
                    (7.8, 3.6),
                    (12.2, 2.5),
                    (16.6, 3.6),
                    (19.2, 6.8),
                ],
                false,
            );
            path(&[(19.2, 6.8), (15.2, 4.6)], false);
            path(&[(19.2, 6.8), (16.8, 9.8)], false);
            line((5.0, 6.5), (5.0, 17.5));
        }
        Icon::Eye | Icon::EyeOff => {
            // Almond outline plus a pupil; the crossed variant adds a slash.
            let left = pt(2.5, 12.0);
            let right = pt(21.5, 12.0);
            path(&[(4.2, 6.4), (8.0, 4.4), (16.0, 4.4), (19.8, 6.4)], false);
            path(&[(4.2, 17.6), (8.0, 19.6), (16.0, 19.6), (19.8, 17.6)], false);
            painter.circle_stroke(pt(12.0, 12.0), 3.2 * k, stroke);
            let _ = (left, right);
            if matches!(icon, Icon::EyeOff) {
                line((4.5, 4.5), (19.5, 19.5));
            }
        }
        Icon::Numbered => {
            // Step marker: a disc with a hand-drawn "1".
            painter.circle_stroke(pt(12.0, 12.0), 8.4 * k, stroke);
            line((10.6, 8.6), (13.4, 7.4));
            line((13.4, 7.4), (13.4, 16.4));
        }
        Icon::Film => {
            // A filmstrip: outer frame with two perforation columns.
            boxed(2.5, 4.0, 21.5, 20.0, 2.0);
            line((7.0, 4.0), (7.0, 20.0));
            line((17.0, 4.0), (17.0, 20.0));
            for y in [6.5_f32, 9.5, 12.5, 15.5, 18.5] {
                line((4.0, y), (5.6, y));
                line((18.4, y), (20.0, y));
            }
        }
        Icon::Ocr => {
            // Scan frame with three text lines: the classic "recognise the
            // text in this box" glyph.
            painter.rect_stroke(
                Rect::from_min_size(pt(4.5, 4.5), Vec2::new(15.0 * k, 15.0 * k)),
                2.0 * k,
                stroke,
                StrokeKind::Inside,
            );
            line((7.5, 9.0), (16.5, 9.0));
            line((7.5, 12.2), (14.5, 12.2));
            line((7.5, 15.4), (12.5, 15.4));
        }
    }
}

fn body_size(ui: &Ui) -> f32 {
    ui.style()
        .text_styles
        .get(&egui::TextStyle::Body)
        .map(|f| f.size)
        .unwrap_or(13.5)
}

fn small_size(ui: &Ui) -> f32 {
    ui.style()
        .text_styles
        .get(&egui::TextStyle::Small)
        .map(|f| f.size)
        .unwrap_or(11.5)
}

fn mono_size(ui: &Ui) -> f32 {
    ui.style()
        .text_styles
        .get(&egui::TextStyle::Monospace)
        .map(|f| f.size)
        .unwrap_or(12.5)
}

fn h2_size(ui: &Ui) -> f32 {
    ui.style()
        .text_styles
        .get(&crate::theme::h2())
        .map(|f| f.size)
        .unwrap_or(15.5)
}

/// A card surface with padding, used for grouped content.
pub fn card<R>(ui: &mut Ui, palette: &Palette, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::NONE
        .fill(palette.card)
        .stroke(Stroke::new(1.0, palette.stroke))
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(16, 14))
        .show(ui, |ui| {
            // Fill the available width so grouped content lines up.
            let width = ui.available_width();
            ui.set_min_width(width);
            add(ui)
        })
        .inner
}

/// A large clickable tile: icon plate, title, description and shortcut badge.
pub fn action_tile(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    title: &str,
    desc: &str,
    shortcut: &str,
    primary: bool,
) -> Response {
    let width = ui.available_width();
    let height = 92.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

    if !ui.is_rect_visible(rect) {
        return response;
    }

    let hovered = response.hovered();
    let bg = if hovered {
        palette.card_hover
    } else {
        palette.card
    };
    let border = if hovered {
        with_alpha(palette.accent, 190)
    } else {
        palette.stroke
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 14.0, bg);
    painter.rect_stroke(rect, 14.0, Stroke::new(1.0, border), StrokeKind::Inside);

    let plate = Rect::from_min_size(rect.min + Vec2::new(14.0, 15.0), Vec2::splat(36.0));
    painter.rect_filled(
        plate,
        11.0,
        if primary {
            palette.accent
        } else {
            palette.accent_soft
        },
    );
    let icon_color = if primary {
        palette.on_accent
    } else {
        palette.accent
    };
    paint_icon(painter, plate.shrink(9.0), icon, icon_color, 1.7);

    let text_x = plate.right() + 12.0;
    let title_font = egui::FontId::proportional(body_size(ui) * 1.05);
    let desc_font = egui::FontId::proportional(small_size(ui));
    painter.text(
        Pos2::new(text_x, rect.top() + 17.0),
        Align2::LEFT_TOP,
        title,
        title_font,
        palette.text,
    );

    let badge_w = if shortcut.is_empty() {
        0.0
    } else {
        let font = egui::FontId::monospace(mono_size(ui) * 0.92);
        let galley = painter.layout_no_wrap(shortcut.to_owned(), font, palette.text_weak);
        let size = galley.size() + Vec2::new(12.0, 4.0);
        let badge = Rect::from_min_size(
            Pos2::new(rect.right() - size.x - 12.0, rect.top() + 12.0),
            size,
        );
        painter.rect_filled(
            badge,
            6.0,
            if palette.dark {
                Color32::from_rgb(0x0E, 0x12, 0x18)
            } else {
                Color32::from_rgb(0xF1, 0xF4, 0xF9)
            },
        );
        painter.rect_stroke(
            badge,
            6.0,
            Stroke::new(1.0, palette.stroke),
            StrokeKind::Inside,
        );
        painter.galley(badge.min + Vec2::new(6.0, 2.0), galley, palette.text_weak);
        size.x + 12.0
    };

    let wrap = (rect.right() - text_x - 12.0 - badge_w).max(60.0);
    // Clamp the description to two rows with an ellipsis: translations vary a
    // lot in length and must never spill out of the tile.
    let mut job =
        egui::text::LayoutJob::simple(desc.to_owned(), desc_font, palette.text_weak, wrap);
    job.wrap.max_rows = 2;
    job.wrap.overflow_character = Some('…');
    let galley = painter.layout_job(job);
    painter.galley(
        Pos2::new(text_x, rect.top() + 18.0 + body_size(ui) * 1.32),
        galley,
        palette.text_weak,
    );

    response
}

/// Compact square icon button used in pin rows and toolbars.
pub fn icon_button(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    active: bool,
    tooltip: &str,
) -> Response {
    icon_button_enabled(ui, palette, icon, active, tooltip, true)
}

/// Like [`icon_button`], but `enabled = false` dims the icon, shows the plain
/// cursor and swallows clicks — used for undo/redo when the stack is empty.
pub fn icon_button_enabled(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    active: bool,
    tooltip: &str,
    enabled: bool,
) -> Response {
    let size = Vec2::splat(28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response.on_hover_cursor(egui::CursorIcon::Default)
    };
    let response = if tooltip.is_empty() {
        response
    } else {
        response.on_hover_text(tooltip)
    };
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = response.hovered();
    if (active || hovered) && enabled {
        ui.painter().rect_filled(
            rect,
            8.0,
            if active {
                palette.accent_soft
            } else {
                palette.card_hover
            },
        );
    }
    let color = if active {
        palette.accent
    } else if hovered && enabled {
        palette.text
    } else if enabled {
        palette.text_weak
    } else {
        // Disabled: faded out, clearly inert.
        with_alpha(palette.text_weak, 80)
    };
    paint_icon(ui.painter(), rect.shrink(7.0), icon, color, 1.9);
    response
}

/// Left navigation rail entry: icon on top of a small caption.
pub fn nav_button(
    ui: &mut Ui,
    palette: &Palette,
    icon: Icon,
    label: &str,
    selected: bool,
) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 60.0), Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if !ui.is_rect_visible(rect) {
        return response;
    }
    let hovered = response.hovered();
    // Keep the pill off the rail edges so it reads as a card, not a stripe.
    let pill = Rect::from_min_max(
        Pos2::new(rect.left() + 7.0, rect.top()),
        Pos2::new(rect.right() - 7.0, rect.bottom()),
    );
    if selected {
        ui.painter().rect_filled(pill, 12.0, palette.accent_soft);
        let bar = Rect::from_min_size(
            Pos2::new(pill.left() + 5.0, rect.center().y - 9.0),
            Vec2::new(3.0, 18.0),
        );
        ui.painter().rect_filled(bar, 1.5, palette.accent);
    } else if hovered {
        ui.painter().rect_filled(pill, 12.0, palette.card_hover);
    }
    let color = if selected {
        palette.accent
    } else if hovered {
        palette.text
    } else {
        palette.text_weak
    };
    let icon_rect = Rect::from_center_size(
        Pos2::new(rect.center().x, rect.center().y - 9.5),
        Vec2::splat(19.0),
    );
    paint_icon(ui.painter(), icon_rect, icon, color, 1.85);

    // Captions differ wildly in length ("Pins" vs "Einstellungen" vs
    // "キャプチャ"): shrink until it fits, then ellipsise.
    let max_w = pill.width() - 10.0;
    let mut size = small_size(ui) * 0.88;
    let mut galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), egui::FontId::proportional(size), color);
    while galley.size().x > max_w && size > 7.5 {
        size -= 0.5;
        galley =
            ui.painter()
                .layout_no_wrap(label.to_owned(), egui::FontId::proportional(size), color);
    }
    if galley.size().x > max_w {
        let mut job = egui::text::LayoutJob::simple(
            label.to_owned(),
            egui::FontId::proportional(size),
            color,
            max_w,
        );
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        galley = ui.painter().layout_job(job);
    }
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y + 13.5 - galley.size().y * 0.5,
        ),
        galley,
        color,
    );
    response
}

/// Section title with a small icon, used inside the settings page.
pub fn section_header(ui: &mut Ui, palette: &Palette, icon: Icon, title: &str) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        paint_icon(ui.painter(), rect, icon, palette.accent, 1.9);
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new(title)
                .font(egui::FontId::proportional(h2_size(ui)))
                .color(palette.text)
                .strong(),
        );
    });
    ui.add_space(2.0);
}

/// Small rounded status pill with a coloured dot.
pub fn pill(ui: &mut Ui, palette: &Palette, text: &str, color: Color32) {
    let font = egui::FontId::proportional(small_size(ui) * 0.95);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, palette.text_weak);
    let size = galley.size() + Vec2::new(26.0, 6.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, 999.0, with_alpha(color, 28));
    ui.painter().rect_stroke(
        rect,
        999.0,
        Stroke::new(1.0, with_alpha(color, 90)),
        StrokeKind::Inside,
    );
    ui.painter()
        .circle_filled(Pos2::new(rect.left() + 11.0, rect.center().y), 3.0, color);
    ui.painter().galley(
        Pos2::new(rect.left() + 19.0, rect.center().y - galley.size().y * 0.5),
        galley,
        color,
    );
}

/// Horizontal segmented control. Returns the newly selected value.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    palette: &Palette,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let mut picked = None;
    let height = 30.0;
    let font = egui::FontId::proportional(body_size(ui) * 0.95);
    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| {
            ui.painter()
                .layout_no_wrap((*label).to_owned(), font.clone(), palette.text)
        })
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 26.0).collect();
    let total: f32 = widths.iter().sum::<f32>() + 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, height), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, palette.card);
    ui.painter().rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, palette.stroke),
        StrokeKind::Inside,
    );

    let mut x = rect.left() + 3.0;
    for (i, (value, _)) in options.iter().enumerate() {
        let w = widths[i];
        let cell = Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), Vec2::new(w, height - 6.0));
        let response = ui.interact(
            cell,
            // Deprecated in egui 0.37 in favour of `Id::unique`, but a *stable*
            // id is exactly what a click target needs: `Id::unique` would mint
            // a new one every frame and no press would ever complete.
            #[allow(deprecated)]
            egui::Id::new(("pinsight-seg", i, galleys[i].size().x as i32)),
            Sense::click(),
        );
        response
            .clone()
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let selected = *value == current;
        if selected {
            ui.painter().rect_filled(cell, 7.0, palette.accent_soft);
            ui.painter().rect_stroke(
                cell,
                7.0,
                Stroke::new(1.0, with_alpha(palette.accent, 120)),
                StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(cell, 7.0, palette.card_hover);
        }
        let color = if selected {
            palette.accent
        } else {
            palette.text_weak
        };
        let galley = ui
            .painter()
            .layout_no_wrap(options[i].1.to_owned(), font.clone(), color);
        ui.painter().galley(
            Pos2::new(
                cell.center().x - galley.size().x * 0.5,
                cell.center().y - galley.size().y * 0.5,
            ),
            galley,
            color,
        );
        if response.clicked() && !selected {
            picked = Some(*value);
        }
        x += w;
    }
    picked
}

/// A full-width tab strip with an accent underline.
///
/// Eight pill cells in a 700 px row turn into visual soup: the container plate
/// competes with the cards below it and every cell screams for attention. An
/// underline strip reads as navigation, stays legible at 8+ items, and gives the
/// page header a natural baseline.
pub fn tabs<T: PartialEq + Copy>(
    ui: &mut Ui,
    palette: &Palette,
    current: T,
    options: &[(T, &str)],
) -> Option<T> {
    let mut picked = None;
    let height = 40.0;
    let font = egui::FontId::proportional(body_size(ui) * 0.98);
    let full = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(full, height), Sense::hover());

    // Hairline across the whole strip, so the tabs sit on a rail.
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, palette.stroke),
    );

    let galleys: Vec<_> = options
        .iter()
        .map(|(_, label)| ui.painter().layout_no_wrap((*label).to_owned(), font.clone(), palette.text))
        .collect();
    let widths: Vec<f32> = galleys.iter().map(|g| g.size().x + 26.0).collect();

    let mut x = rect.left();
    for (i, (value, _)) in options.iter().enumerate() {
        let w = widths[i];
        let cell = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, height));
        // A stable id: the tab strip lives inside a freshly allocated child
        // Ui every frame, so `scope_id()` would change and the click would
        // never complete a press+release pair.
        #[allow(deprecated)]
        let tab_id = egui::Id::new(("pinsight-tab", i));
        let response = ui.interact(cell, tab_id, Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let selected = *value == current;
        let hovered = response.hovered();

        let color = if selected {
            palette.accent
        } else if hovered {
            palette.text
        } else {
            palette.text_weak
        };
        let galley = ui
            .painter()
            .layout_no_wrap(options[i].1.to_owned(), font.clone(), color);
        let text_w = galley.size().x;
        ui.painter().galley(
            Pos2::new(
                cell.center().x - galley.size().x * 0.5,
                cell.center().y - galley.size().y * 0.5 - 2.0,
            ),
            galley,
            color,
        );

        // Underline: solid and 2 px when selected, a faint 2 px hover hint
        // otherwise. Same geometry for both, so nothing jumps on click.
        if selected || hovered {
            let bar_w = (text_w + 8.0).min(w);
            let bar = Rect::from_center_size(
                Pos2::new(cell.center().x, rect.bottom() - 1.5),
                Vec2::new(bar_w, if selected { 2.5 } else { 1.5 }),
            );
            ui.painter().rect_filled(
                bar,
                2.0,
                if selected {
                    palette.accent
                } else {
                    with_alpha(palette.text_weak, 90)
                },
            );
        }

        if response.clicked() && !selected {
            picked = Some(*value);
        }
        x += w;
    }
    picked
}

/// A row of accent swatches laid out left-to-right inside one allocation.
///
/// Doing it as a single widget matters: inside a right-aligned row, adding six
/// separate swatches makes egui pack them right-to-left and the last one lands
/// outside the card. One allocation with an explicit width cannot overflow.
/// Returns the index that was clicked.
pub fn swatch_row(
    ui: &mut Ui,
    _palette: &Palette,
    items: &[(Color32, bool, &str)],
) -> Option<usize> {
    const SIZE: f32 = 30.0;
    const GAP: f32 = 8.0;
    if items.is_empty() {
        return None;
    }
    let total = items.len() as f32 * SIZE + (items.len() - 1) as f32 * GAP;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(total, SIZE), Sense::hover());
    let mut picked = None;
    for (index, (color, selected, label)) in items.iter().enumerate() {
        let center = Pos2::new(
            rect.left() + index as f32 * (SIZE + GAP) + SIZE * 0.5,
            rect.center().y,
        );
        let hit = Rect::from_center_size(center, Vec2::splat(SIZE));
        let response = ui.interact(hit, ui.scope_id().with(("swatch-row", index)), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
        let response = if label.is_empty() {
            response
        } else {
            response.on_hover_text(*label)
        };
        ui.painter().circle_filled(center, 13.0, *color);
        if *selected {
            ui.painter()
                .circle_stroke(center, 14.0, Stroke::new(1.5, *color));
            ui.painter()
                .circle_filled(center, 4.0, Color32::WHITE);
        } else if response.hovered() {
            ui.painter().circle_stroke(
                center,
                15.0,
                Stroke::new(1.0, with_alpha(*color, 120)),
            );
        }
        if response.clicked() {
            picked = Some(index);
        }
    }
    picked
}

/// A filled accent button, for the confirming action of a dialog.
pub fn primary_button(ui: &mut Ui, palette: &Palette, text: &str) -> Response {
    let font = egui::FontId::proportional(body_size(ui) * 0.95);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, palette.on_accent);
    let size = Vec2::new(galley.size().x + 28.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let fill = if response.hovered() {
        palette.accent.gamma_multiply(1.08)
    } else {
        palette.accent
    };
    ui.painter().rect_filled(rect, 8.0, fill);
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        palette.on_accent,
    );
    response
}

/// A secondary (outline) button for less prominent actions.
pub fn button(ui: &mut Ui, palette: &Palette, text: &str) -> Response {
    let font = egui::FontId::proportional(body_size(ui) * 0.95);
    let galley =
        ui.painter()
            .layout_no_wrap(text.to_owned(), font, palette.text);
    let size = Vec2::new(galley.size().x + 24.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let stroke = if response.hovered() {
        palette.accent
    } else {
        palette.stroke
    };
    ui.painter()
        .rect_stroke(rect, 8.0, egui::Stroke::new(1.0, stroke), egui::StrokeKind::Inside);
    ui.painter().galley(
        Pos2::new(
            rect.center().x - galley.size().x * 0.5,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        palette.text,
    );
    response
}

/// A colour field: a bordered swatch with its hex value.
///
/// It does not open anything by itself — the app owns the colour dialog (see
/// `crate::picker`), so this only reports a click. Drawing the swatch here keeps
/// the row layout consistent with every other control.
pub fn color_field(ui: &mut Ui, palette: &Palette, rgb: [u8; 3]) -> Response {
    let text = format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]);
    let font = egui::FontId::monospace(small_size(ui));
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font.clone(), palette.text);
    let size = Vec2::new(28.0 + 8.0 + galley.size().x + 4.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(text);
    let hovered = response.hovered();

    let chip = Rect::from_center_size(
        Pos2::new(rect.left() + 14.0, rect.center().y),
        Vec2::new(28.0, 22.0),
    );
    let painter = ui.painter();
    if hovered {
        painter.rect_stroke(
            chip.expand(2.0),
            9.0,
            Stroke::new(1.5, with_alpha(palette.accent, 120)),
            StrokeKind::Outside,
        );
    }
    painter.rect_filled(chip, 7.0, Color32::from_rgb(rgb[0], rgb[1], rgb[2]));
    painter.rect_stroke(
        chip,
        7.0,
        Stroke::new(1.0, palette.stroke_strong),
        StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(
        format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]),
        font,
        palette.text_weak,
    );
    painter.galley(
        Pos2::new(
            chip.right() + 8.0,
            rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        palette.text_weak,
    );
    response
}

/// Compact placeholder row: a small icon plate next to one line of text. Used
/// inside cards that are waiting for their first piece of content.
pub fn hint_row(ui: &mut Ui, palette: &Palette, icon: Icon, text: &str) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 36.0), Sense::hover());
    let plate = Rect::from_min_size(
        Pos2::new(rect.left(), rect.center().y - 17.0),
        Vec2::splat(34.0),
    );
    ui.painter().rect_filled(plate, 10.0, palette.accent_soft);
    paint_icon(ui.painter(), plate.shrink(9.0), icon, palette.accent, 1.8);
    ui.painter().text(
        Pos2::new(plate.right() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(small_size(ui) + 0.5),
        palette.text_weak,
    );
}

/// A labelled row: label on the left, control flush against the right edge.
///
/// The control column is right-aligned on purpose. Left-aligning it after a
/// fixed-width label leaves a dead 300 px gap at the end of every row, which is
/// what makes a settings page read as "unfinished" — the eye follows the empty
/// space instead of the values.
pub fn field_row(ui: &mut Ui, palette: &Palette, label: &str, add: impl FnOnce(&mut Ui)) {
    field_row_h(ui, palette, ROW_HEIGHT, label, add);
}

/// Row height shared by every settings row, so cards line up vertically even
/// when they mix switches, sliders and combo boxes.
pub const ROW_HEIGHT: f32 = 34.0;

pub fn field_row_h(
    ui: &mut Ui,
    palette: &Palette,
    height: f32,
    label: &str,
    add: impl FnOnce(&mut Ui),
) {
    let full = ui.available_width();
    let label_w = (full * 0.42).clamp(110.0, 300.0);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(Vec2::new(label_w, height), Sense::hover());
        let mut job = egui::text::LayoutJob::simple(
            label.to_owned(),
            egui::FontId::proportional(body_size(ui) * 0.97),
            palette.text_weak,
            label_w - 8.0,
        );
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('…');
        let galley = ui.painter().layout_job(job);
        ui.painter().galley(
            Pos2::new(rect.left(), rect.center().y - galley.size().y * 0.5),
            galley,
            palette.text_weak,
        );

        // Everything the caller adds lands in the remaining width, packed to
        // the right edge. Single-widget rows (the vast majority: a switch, a
        // combo, a slider) are unaffected by the right-to-left order.
        let rest = ui.available_width().max(40.0);
        ui.allocate_ui_with_layout(
            Vec2::new(rest, height),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                add(ui);
            },
        );
    });
}

/// iOS-style switch, used in place of the stock checkbox.
///
/// A 12 px square with a hairline tick reads as a form from 1995 and gives no
/// sense that the row is a toggle. The returned response reports `changed()`
/// exactly like `Ui::checkbox`, so call sites keep their shape.
pub fn toggle(ui: &mut Ui, palette: &Palette, on: &mut bool) -> Response {
    let size = Vec2::new(42.0, 24.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    let mut changed = false;
    if response.clicked() {
        *on = !*on;
        changed = true;
    }
    let hovered = response.hovered();
    paint_switch(ui, palette, rect, response.id, *on, hovered, response.has_focus());
    let mut response = response;
    if changed {
        response.mark_changed();
        ui.ctx().request_repaint();
    }
    response
}

/// Paint a switch into an explicit rect (also used by the row-level helper).
fn paint_switch(
    ui: &Ui,
    palette: &Palette,
    rect: Rect,
    id: egui::Id,
    on: bool,
    hovered: bool,
    focused: bool,
) {
    let t = ui
        .ctx()
        .animate_bool_with_time(id, on, 0.13)
        .clamp(0.0, 1.0);
    let track = if on {
        palette.accent
    } else if hovered {
        if palette.dark {
            palette.card_hover
        } else {
            egui::Color32::from_rgb(0xC3, 0xCC, 0xDA)
        }
    } else {
        palette.stroke_strong
    };
    let painter = ui.painter();
    if on {
        painter.rect_filled(rect.expand(1.0), 13.0, with_alpha(palette.accent, 38));
    }
    painter.rect_filled(rect, 12.0, track);
    if focused {
        painter.rect_stroke(
            rect.expand(2.0),
            14.0,
            Stroke::new(1.5, with_alpha(palette.accent, 160)),
            StrokeKind::Outside,
        );
    }
    let x = egui::lerp((rect.left() + 12.0)..=(rect.right() - 12.0), t);
    painter.circle_filled(Pos2::new(x, rect.center().y), 9.0, Color32::WHITE);
}

/// A slider that always draws left-to-right, with its value in a badge.
///
/// `egui::Slider` mirrors itself inside a right-to-left layout — the knob then
/// grows *leftwards* as the value rises, which is the opposite of what the
/// track suggests. Since settings rows are right-aligned, every stock slider
/// would have come out backwards. Drawing our own also lets the track stay thin
/// and the value live in a quiet badge instead of a boxed number field.
pub fn slider(
    ui: &mut Ui,
    palette: &Palette,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
    suffix: &str,
) -> Response {
    let (min, max) = (*range.start(), *range.end());
    let track_w = 200.0;
    let text = format!("{value:.decimals$}{suffix}");
    let font = egui::FontId::proportional(small_size(ui) + 0.5);
    let galley = ui
        .painter()
        .layout_no_wrap(text.clone(), font, palette.text);
    let badge_w = (galley.size().x + 18.0).max(46.0);
    let total = Vec2::new(track_w + 10.0 + badge_w, 26.0);
    let (rect, response) = ui.allocate_exact_size(total, Sense::click_and_drag());
    let response = response.on_hover_cursor(egui::CursorIcon::ResizeHorizontal);

    let track = Rect::from_min_max(
        Pos2::new(rect.left(), rect.center().y - 2.0),
        Pos2::new(rect.left() + track_w, rect.center().y + 2.0),
    );
    let badge = Rect::from_min_size(
        Pos2::new(track.right() + 10.0, rect.center().y - 12.0),
        Vec2::new(badge_w, 24.0),
    );

    let mut changed = false;
    if response.dragged() || response.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let raw = min + t * (max - min);
            let step = 10f32.powi(-(decimals as i32));
            let snapped = (raw / step).round() * step;
            let next = snapped.clamp(min, max);
            if (next - *value).abs() > step * 0.01 {
                *value = next;
                changed = true;
            }
        }
    }
    let mut response = response;
    if changed {
        response.mark_changed();
    }

    let t = ((*value - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0);
    let painter = ui.painter();
    painter.rect_filled(track, 2.0, with_alpha(palette.text_weak, 40));
    let filled = Rect::from_min_max(
        track.min,
        Pos2::new(track.left() + track.width() * t, track.bottom()),
    );
    painter.rect_filled(filled, 2.0, palette.accent);

    let knob_x = egui::lerp(track.left()..=track.right(), t);
    let hovered = response.hovered() || response.dragged();
    painter.circle_filled(
        Pos2::new(knob_x, track.center().y),
        if hovered { 8.5 } else { 7.5 },
        palette.card,
    );
    painter.circle_stroke(
        Pos2::new(knob_x, track.center().y),
        if hovered { 8.5 } else { 7.5 },
        Stroke::new(2.0, palette.accent),
    );

    painter.rect_filled(badge, 7.0, if palette.dark {
        palette.card_hover
    } else {
        egui::Color32::from_rgb(0xF1, 0xF4, 0xF9)
    });
    painter.rect_stroke(
        badge,
        7.0,
        Stroke::new(1.0, palette.stroke),
        StrokeKind::Inside,
    );
    let galley = painter.layout_no_wrap(
        format!("{value:.decimals$}{suffix}"),
        egui::FontId::proportional(small_size(ui) + 0.5),
        palette.text,
    );
    painter.galley(
        Pos2::new(
            badge.center().x - galley.size().x * 0.5,
            badge.center().y - galley.size().y * 0.5,
        ),
        galley,
        palette.text,
    );
    response
}


/// Frameless text button with an accent tint, used for inline actions.
pub fn link_button(ui: &mut Ui, palette: &Palette, icon: Icon, text: &str) -> Response {
    let font = egui::FontId::proportional(body_size(ui) * 0.95);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, palette.accent);
    let size = Vec2::new(galley.size().x + 26.0, 26.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response
        .clone()
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    if response.hovered() {
        ui.painter().rect_filled(rect, 8.0, palette.accent_soft);
    }
    paint_icon(
        ui.painter(),
        Rect::from_min_size(
            Pos2::new(rect.left() + 5.0, rect.center().y - 7.0),
            Vec2::splat(14.0),
        ),
        icon,
        palette.accent,
        2.0,
    );
    ui.painter().galley(
        Pos2::new(rect.left() + 23.0, rect.center().y - galley.size().y * 0.5),
        galley,
        palette.accent,
    );
    response
}

/// Draw a soft rounded rectangle behind a region (used for previews).
pub fn preview_frame(painter: &egui::Painter, rect: Rect, palette: &Palette) {
    painter.rect_filled(rect, 10.0, with_alpha(palette.stroke, 70));
    painter.rect_stroke(
        rect,
        10.0,
        Stroke::new(1.0, palette.stroke),
        StrokeKind::Inside,
    );
}

/// Paint an image inside `rect`, preserving aspect ratio (letterboxed).
pub fn paint_thumbnail(
    painter: &egui::Painter,
    rect: Rect,
    texture: &egui::TextureHandle,
    palette: &Palette,
) {
    preview_frame(painter, rect, palette);
    let size = texture.size_vec2();
    if size.x <= 0.0 || size.y <= 0.0 {
        return;
    }
    let scale = (rect.width() / size.x).min(rect.height() / size.y).max(0.0);
    let target = Rect::from_center_size(rect.center(), size * scale);
    painter.image(
        texture.id(),
        target,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        Color32::WHITE,
    );
}
