//! Glyph outlines for rasterising text annotations.
//!
//! egui renders the overlay preview with its own text stack, but the final
//! image has to be produced by `tiny-skia`, which has no text support. This
//! module bridges the two: it outlines glyphs with `ab_glyph` from the same
//! font file the UI uses, converts the curves into `tiny-skia` paths and fills
//! them — so a committed text annotation looks the same in the preview and in
//! the saved PNG.

use std::path::PathBuf;

use ab_glyph::{Font, FontVec, GlyphId, OutlineCurve, ScaleFont};
use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

/// Last-resort Latin faces, in preference order.
const FALLBACK_FONTS: &[&str] = &["arial.ttf", "segoeui.ttf", "DejaVuSans.ttf", "tahoma.ttf"];

pub struct TextFont {
    /// Parsed on first use: a 16 MB CJK face is a real cost and most sessions
    /// never place a text annotation at all.
    font: std::sync::OnceLock<Option<FontVec>>,
}

impl TextFont {
    fn resolve_path() -> Option<PathBuf> {
        // Prefer whatever the UI is drawn with; fall back to any Latin face so
        // text annotations still rasterise when font discovery failed.
        crate::fonts::primary_font_path()
            .cloned()
            .or_else(|| crate::fonts::find_system_font(FALLBACK_FONTS))
    }

    fn load() -> Self {
        Self {
            font: std::sync::OnceLock::new(),
        }
    }

    /// Parse the face on first use; `None` means text annotations preview but
    /// never rasterise.
    fn parsed(&self) -> Option<&FontVec> {
        self.font
            .get_or_init(|| {
                let path = Self::resolve_path()?;
                let bytes = match std::fs::read(&path) {
                    Ok(bytes) => bytes,
                    Err(err) => {
                        tracing::debug!(?path, %err, "text rasteriser could not read the font");
                        return None;
                    }
                };
                match FontVec::try_from_vec(bytes) {
                    Ok(font) => Some(font),
                    Err(err) => {
                        tracing::debug!(?path, %err, "text rasteriser could not parse the font");
                        None
                    }
                }
            })
            .as_ref()
    }

    /// Advance width of one character at `px`, or a rough estimate when no
    /// face could be loaded (so wrapping still terminates).
    pub fn char_width(&self, ch: char, px: f32) -> f32 {
        match self.parsed() {
            Some(font) => font.as_scaled(px).h_advance(font.glyph_id(ch)),
            None => px * 0.5,
        }
    }

    /// Width of a single line at `px`.
    pub fn text_width(&self, text: &str, px: f32) -> f32 {
        text.chars().map(|ch| self.char_width(ch, px)).sum()
    }

    /// Height of one line at `px`, including the natural leading.
    pub fn line_height(&self, px: f32) -> f32 {
        match self.parsed() {
            Some(font) => font.as_scaled(px).height(),
            None => px * 1.2,
        }
    }

    /// Draw `text` with its top-left corner at (`x`, `top`) in pixmap pixels.
    /// `px` is the font size in pixels. Missing glyphs fall back to advances,
    /// so unknown characters shift the pen instead of breaking the line.
    pub fn draw(&self, pixmap: &mut Pixmap, text: &str, x: f32, top: f32, px: f32, color: [u8; 4]) {
        let Some(font) = self.parsed() else {
            return;
        };
        let upm = font.units_per_em().unwrap_or(1000.0).max(1.0);
        let scale = px / upm;
        let scaled = font.as_scaled(px);
        let mut paint = Paint::default();
        paint.set_color(tiny_skia::Color::from_rgba8(
            color[0], color[1], color[2], color[3],
        ));
        paint.anti_alias = true;

        let mut baseline = top + scaled.ascent();
        for line in text.split('\n') {
            let mut pen_x = x;
            for ch in line.chars() {
                let gid: GlyphId = font.glyph_id(ch);
                if let Some(outline) = font.outline(gid) {
                    let mut builder = PathBuilder::new();
                    // Font units are y-up; pixmaps are y-down.
                    let map = |p: ab_glyph::Point| (pen_x + p.x * scale, baseline - p.y * scale);
                    // A glyph is a list of closed contours concatenated without
                    // separators: each contour's last curve returns to its own
                    // start. `move_to` must therefore fire only where the pen
                    // jumps — starting a sub-path per curve would leave every
                    // segment open and fill nothing.
                    let mut pen: Option<(f32, f32)> = None;
                    for curve in &outline.curves {
                        let (start, end) = match *curve {
                            OutlineCurve::Line(p0, p1) => (map(p0), map(p1)),
                            OutlineCurve::Quad(p0, _, p2) => (map(p0), map(p2)),
                            OutlineCurve::Cubic(p0, _, _, p3) => (map(p0), map(p3)),
                        };
                        if pen != Some(start) {
                            builder.move_to(start.0, start.1);
                        }
                        match *curve {
                            OutlineCurve::Line(_, p1) => {
                                let b = map(p1);
                                builder.line_to(b.0, b.1);
                            }
                            OutlineCurve::Quad(_, p1, _) => {
                                let c = map(p1);
                                builder.quad_to(c.0, c.1, end.0, end.1);
                            }
                            OutlineCurve::Cubic(_, p1, p2, _) => {
                                let c1 = map(p1);
                                let c2 = map(p2);
                                builder.cubic_to(c1.0, c1.1, c2.0, c2.1, end.0, end.1);
                            }
                        }
                        pen = Some(end);
                    }
                    builder.close();
                    if let Some(path) = builder.finish() {
                        pixmap.fill_path(
                            &path,
                            &paint,
                            FillRule::Winding,
                            Transform::identity(),
                            None,
                        );
                    }
                }
                pen_x += scaled.h_advance(gid);
            }
            baseline += scaled.height();
        }
    }
}

/// The process-wide rasteriser font, resolved from the same file the UI uses.
/// A lock (rather than a bare `OnceLock`) lets [`reload`] swap in a new face
/// after the user changes the UI font, while readers still get a reference.
pub fn font() -> std::sync::RwLockReadGuard<'static, TextFont> {
    static FONT: std::sync::OnceLock<std::sync::RwLock<TextFont>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| std::sync::RwLock::new(TextFont::load()))
        .read()
        .unwrap_or_else(|e| e.into_inner())
}

/// Re-resolve after the user changes the UI font.
pub fn reload() {
    let fresh = TextFont::load();
    if let Ok(mut guard) = font_cell().write() {
        *guard = fresh;
    }
}

fn font_cell() -> &'static std::sync::RwLock<TextFont> {
    static FONT: std::sync::OnceLock<std::sync::RwLock<TextFont>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| std::sync::RwLock::new(TextFont::load()))
}

/// Rasterise plain text onto a white image, wrapping at `max_width`.
///
/// Used when the clipboard holds text and the user pins it: the result is an
/// ordinary image that the rest of the pin pipeline can treat like a
/// screenshot. Returns `None` when no font could be loaded or the text is
/// blank.
pub fn render_text_image(
    text: &str,
    margin: u32,
    max_width: u32,
    font_size: u32,
    bg: [u8; 3],
    fg: [u8; 3],
) -> Option<image::RgbaImage> {
    if text.trim().is_empty() {
        return None;
    }
    let font = font();
    let px = font_size.clamp(8, 96) as f32;
    let margin = margin as f32;
    let max_w = max_width.max(240) as f32;
    let inner = (max_w - margin * 2.0).max(80.0);

    // Wrap on newlines, then greedily per character so CJK text (which has no
    // spaces) breaks as cleanly as Latin text.
    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut width = 0.0_f32;
        for ch in raw.chars() {
            let ch_w = font.char_width(ch, px);
            if !current.is_empty() && width + ch_w > inner {
                lines.push(std::mem::take(&mut current));
                width = 0.0;
            }
            current.push(ch);
            width += ch_w;
        }
        lines.push(current);
    }
    if lines.is_empty() {
        return None;
    }

    let line_h = font.line_height(px).max(px);
    let content_w = lines
        .iter()
        .map(|line| font.text_width(line, px))
        .fold(0.0_f32, f32::max);
    let width = (content_w + margin * 2.0).clamp(80.0, max_w).ceil();
    let height = (margin * 2.0 + line_h * lines.len() as f32).ceil();

    let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32)?;
    pixmap.fill(tiny_skia::Color::from_rgba8(bg[0], bg[1], bg[2], 255));
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        font.draw(
            &mut pixmap,
            line,
            margin,
            margin + i as f32 * line_h,
            px,
            [fg[0], fg[1], fg[2], 255],
        );
    }
    let (w, h) = (pixmap.width(), pixmap.height());
    image::RgbaImage::from_raw(w, h, pixmap.take())
}

/// Like [`render_text_image`] but with a fully transparent background, so the
/// result can be composited onto an existing image (e.g. a GIF frame).
pub fn render_text_overlay(
    text: &str,
    max_width: u32,
    font_size: u32,
    color: [u8; 3],
) -> Option<image::RgbaImage> {
    if text.trim().is_empty() {
        return None;
    }
    let font = font();
    let px = font_size.clamp(8, 96) as f32;
    let margin = 12.0_f32;
    let max_w = max_width.max(240) as f32;
    let inner = (max_w - margin * 2.0).max(80.0);

    let mut lines: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        if raw.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        let mut width = 0.0_f32;
        for ch in raw.chars() {
            let ch_w = font.char_width(ch, px);
            if !current.is_empty() && width + ch_w > inner {
                lines.push(std::mem::take(&mut current));
                width = 0.0;
            }
            current.push(ch);
            width += ch_w;
        }
        lines.push(current);
    }
    if lines.is_empty() {
        return None;
    }

    let line_h = font.line_height(px).max(px);
    let content_w = lines
        .iter()
        .map(|line| font.text_width(line, px))
        .fold(0.0_f32, f32::max);
    let width = (content_w + margin * 2.0).clamp(80.0, max_w).ceil();
    let height = (margin * 2.0 + line_h * lines.len() as f32).ceil();

    let mut pixmap = tiny_skia::Pixmap::new(width as u32, height as u32)?;
    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }
        font.draw(
            &mut pixmap,
            line,
            margin,
            margin + i as f32 * line_h,
            px,
            [color[0], color[1], color[2], 255],
        );
    }
    let (w, h) = (pixmap.width(), pixmap.height());
    image::RgbaImage::from_raw(w, h, pixmap.take())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    #[test]
    fn resolves_a_font_on_a_normal_machine() {
        // CI containers may have no fonts at all; only assert when one exists.
        if TextFont::resolve_path().is_some() {
            assert!(font().parsed().is_some());
        }
    }

    #[test]
    fn draws_text_into_a_pixmap() {
        let f = font();
        if f.parsed().is_none() {
            return;
        }
        let mut pixmap = Pixmap::new(200, 60).expect("valid pixmap");
        let before = pixmap.data().iter().filter(|b| **b > 0).count();
        f.draw(&mut pixmap, "Hi 42", 8.0, 10.0, 24.0, [255, 0, 0, 255]);
        let after = pixmap.data().iter().filter(|b| **b > 0).count();
        assert!(after > before, "text drawing changed no pixels");

        // And it survives the round trip back into an RgbaImage.
        let img = RgbaImage::from_raw(200, 60, pixmap.data().to_vec()).expect("rgba");
        assert!(img.pixels().any(|p| p[0] > 200 && p[3] > 200));
    }

    #[test]
    fn renders_text_to_an_image() {
        if font().parsed().is_none() {
            return;
        }
        let img = render_text_image(
            "Hello 世界\nsecond line",
            12,
            320,
            18,
            [255, 255, 255],
            [17, 17, 17],
        )
        .expect("rendered image");
        assert!(img.width() <= 320, "image respects the max width");
        assert!(img.height() > 24, "two lines are taller than one");
        // Dark glyphs were painted over the white background.
        assert!(img.pixels().any(|p| p[0] < 100), "no glyphs were drawn");
    }
}
