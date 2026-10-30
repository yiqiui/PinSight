//! Post-capture image decorations: a border, a soft drop shadow, and a
//! watermark (text or an image). Applied just before a capture is encoded so
//! every save path — full-screen, region, window, clipboard-as-file — gets the
//! same treatment, and the result is plain `image` data with no platform code.

use std::path::PathBuf;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

/// Where a single (non-tiled) watermark sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum WatermarkPos {
    /// Repeat the mark across the whole image.
    Tile,
    /// One copy, centred.
    Center,
    /// Corners, with a margin.
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

impl WatermarkPos {
    pub const ALL: [WatermarkPos; 6] = [
        WatermarkPos::Tile,
        WatermarkPos::Center,
        WatermarkPos::TopLeft,
        WatermarkPos::TopRight,
        WatermarkPos::BottomLeft,
        WatermarkPos::BottomRight,
    ];
}

/// A uniform frame around the content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BorderDeco {
    /// Frame thickness in pixels.
    pub width: u32,
    /// Frame colour, straight RGBA.
    pub color: [u8; 4],
}

/// A soft drop shadow behind the content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShadowDeco {
    /// Blur radius in pixels (box-blurred three times for a Gaussian look).
    pub blur: u32,
    /// Horizontal offset of the shadow in pixels.
    pub offset_x: i32,
    /// Vertical offset of the shadow in pixels.
    pub offset_y: i32,
    /// Shadow colour, straight RGB.
    pub color: [u8; 3],
    /// Shadow strength, 0–255.
    pub opacity: u8,
}

/// A watermark drawn on top of the capture.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WatermarkDeco {
    /// Text mark; empty means "no text mark".
    pub text: String,
    /// Optional image mark. When both `text` and this are set, the image is
    /// drawn first and the text on top of it.
    pub image_path: Option<PathBuf>,
    /// Placement of a single mark, or tiling for `Tile`.
    pub position: WatermarkPos,
    /// Mark opacity, 0–255.
    pub opacity: u8,
    /// Text size as a fraction of the image's smaller side (0.05 ≈ 5 %).
    pub text_scale: f32,
    /// Image mark size as a fraction of the image's smaller side.
    pub image_scale: f32,
    /// Gap from the edge for corner / centre placement, in pixels.
    pub margin: u32,
}

/// Everything the "decorate" step can do to an image. `None` on a sub-field
/// means "skip that effect", so a user can mix and match freely.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct Decorations {
    pub border: Option<BorderDeco>,
    pub shadow: Option<ShadowDeco>,
    pub watermark: Option<WatermarkDeco>,
}

impl Decorations {
    /// True when no effect is requested; lets the save path skip the clone.
    pub fn is_empty(&self) -> bool {
        self.border.is_none() && self.shadow.is_none() && self.watermark.is_none()
    }
}

/// Apply all requested decorations to a copy of `image`.
///
/// Order matters: the border frame is wrapped around the content first, then the
/// drop shadow is cast by the *framed* image (so the white frame itself has a
/// shadow, not a transparent gap), and the watermark sits on top of the
/// finished picture. Returns `image` unchanged when nothing is requested.
pub fn decorate_image(image: &RgbaImage, deco: &Decorations) -> RgbaImage {
    if deco.is_empty() {
        return image.clone();
    }
    let mut img = image.clone();
    if let Some(border) = &deco.border {
        img = add_border(&img, border);
    }
    if let Some(shadow) = &deco.shadow {
        img = add_shadow(&img, shadow);
    }
    if let Some(wm) = &deco.watermark {
        apply_watermark(&mut img, wm);
    }
    img
}

/// Wrap the image in a solid frame.
fn add_border(image: &RgbaImage, border: &BorderDeco) -> RgbaImage {
    let w = border.width.max(1);
    let (iw, ih) = (image.width(), image.height());
    let ow = iw + w * 2;
    let oh = ih + w * 2;
    let mut out = RgbaImage::from_pixel(ow, oh, Rgba(border.color));
    for y in 0..ih {
        for x in 0..iw {
            out.put_pixel(x + w, y + w, *image.get_pixel(x, y));
        }
    }
    out
}

/// Drop a soft shadow behind the image on a transparent, expanded canvas.
fn add_shadow(image: &RgbaImage, shadow: &ShadowDeco) -> RgbaImage {
    let r = shadow.blur.max(1);
    // Three box passes reach `r * 3` pixels, so the silhouette mask needs that
    // much transparent padding for the falloff to actually happen. Blurring a
    // mask that touches the array edge would clamp and stay fully opaque.
    let spread = (r * 3) as i32;
    let (iw, ih) = (image.width() as i32, image.height() as i32);
    let pad = spread + shadow.offset_x.abs().max(shadow.offset_y.abs()) + 2;
    let ow = (iw + pad * 2) as u32;
    let oh = (ih + pad * 2) as u32;

    // 1. Silhouette of the content, inset in a padded mask.
    let mw = iw + spread * 2;
    let mh = ih + spread * 2;
    let mut alpha = vec![0.0f32; (mw * mh) as usize];
    for y in 0..ih {
        for x in 0..iw {
            let a = image.get_pixel(x as u32, y as u32).0[3] as f32 / 255.0;
            alpha[((y + spread) * mw + (x + spread)) as usize] = a * shadow.opacity as f32 / 255.0;
        }
    }
    // 2. Box-blur the alpha three times for a smooth, Gaussian-like falloff.
    let blurred = blur_alpha(alpha, mw as u32, mh as u32, r);

    // 3. Composite shadow then content into the output canvas.
    let mut out = RgbaImage::new(ow, oh);
    let mx = pad + shadow.offset_x - spread;
    let my = pad + shadow.offset_y - spread;
    for y in 0..mh {
        for x in 0..mw {
            let a = blurred[(y * mw + x) as usize];
            if a > 0.0 {
                out.put_pixel(
                    (x + mx) as u32,
                    (y + my) as u32,
                    Rgba([
                        shadow.color[0],
                        shadow.color[1],
                        shadow.color[2],
                        (a * 255.0).clamp(0.0, 255.0) as u8,
                    ]),
                );
            }
        }
    }
    for y in 0..ih {
        for x in 0..iw {
            blend_over(
                out.get_pixel_mut((x + pad) as u32, (y + pad) as u32),
                *image.get_pixel(x as u32, y as u32),
            );
        }
    }
    out
}

/// Separable box blur of an alpha mask; three H/V pairs approximate a Gaussian.
fn blur_alpha(mut a: Vec<f32>, w: u32, h: u32, radius: u32) -> Vec<f32> {
    let (w, h, r) = (w as usize, h as usize, radius as usize);
    if r == 0 || w == 0 || h == 0 {
        return a;
    }
    let mut tmp = vec![0.0f32; a.len()];
    for _ in 0..3 {
        // Horizontal pass.
        for y in 0..h {
            let row = y * w;
            box_blur_line(&a[row..row + w], &mut tmp[row..row + w], w, r);
        }
        // Vertical pass.
        for x in 0..w {
            let mut col = vec![0.0f32; h];
            for y in 0..h {
                col[y] = tmp[y * w + x];
            }
            let mut out = vec![0.0f32; h];
            box_blur_line(&col, &mut out, h, r);
            for y in 0..h {
                a[y * w + x] = out[y];
            }
        }
    }
    a
}

/// Average each element with its `radius` neighbours to either side.
fn box_blur_line(src: &[f32], dst: &mut [f32], n: usize, radius: usize) {
    if n == 0 {
        return;
    }
    // Seed the window with indices [0, min(radius, n-1)].
    let right0 = radius.min(n - 1);
    let mut sum: f32 = src[..=right0].iter().sum();
    for i in 0..n {
        let left = i.saturating_sub(radius);
        let right = (i + radius).min(n - 1);
        if i > 0 {
            // Slide: drop the element that left the left edge, add the one
            // that entered the right edge. Both edges clamp at the borders.
            if left > 0 {
                sum -= src[left - 1];
            }
            if i + radius < n {
                sum += src[i + radius];
            }
        }
        dst[i] = sum / (right - left + 1) as f32;
    }
}

/// Standard "over" blend of `src` (straight alpha) onto `dst` in place.
fn blend_over(dst: &mut Rgba<u8>, src: Rgba<u8>) {
    let sa = src.0[3] as f32 / 255.0;
    if sa <= 0.0 {
        return;
    }
    if sa >= 1.0 {
        *dst = src;
        return;
    }
    let da = dst.0[3] as f32 / 255.0;
    let out_a = sa + da * (1.0 - sa);
    if out_a <= 0.0 {
        *dst = Rgba([0, 0, 0, 0]);
        return;
    }
    for c in 0..3 {
        let s = src.0[c] as f32;
        let d = dst.0[c] as f32;
        let v = (s * sa + d * da * (1.0 - sa)) / out_a;
        dst.0[c] = v as u8;
    }
    dst.0[3] = (out_a * 255.0) as u8;
}

/// Composite a straight-alpha RGBA layer (already carrying final opacity in
/// its alpha channel) onto `dst` at (`x`, `y`).
fn stamp_layer(dst: &mut RgbaImage, layer: &RgbaImage, x: i32, y: i32) {
    let (lw, lh) = (layer.width() as i32, layer.height() as i32);
    let (dw, dh) = (dst.width() as i32, dst.height() as i32);
    for ly in 0..lh {
        for lx in 0..lw {
            let dx = x + lx;
            let dy = y + ly;
            if dx < 0 || dy < 0 || dx >= dw || dy >= dh {
                continue;
            }
            let s = *layer.get_pixel(lx as u32, ly as u32);
            if s.0[3] == 0 {
                continue;
            }
            blend_over(dst.get_pixel_mut(dx as u32, dy as u32), s);
        }
    }
}

/// Draw the watermark (image and/or text) onto `image` in place.
fn apply_watermark(image: &mut RgbaImage, wm: &WatermarkDeco) {
    let (iw, ih) = (image.width() as i32, image.height() as i32);
    let small = iw.min(ih) as f32;
    let opacity = wm.opacity.clamp(0, 255) as f32 / 255.0;
    if opacity <= 0.0 {
        return;
    }

    // Build the mark layers (image layer then text layer, both at full
    // opacity) and stamp them according to the placement.
    let mut layers: Vec<RgbaImage> = Vec::new();
    if let Some(path) = &wm.image_path {
        if let Ok(loaded) = image::open(path) {
            let mut img = loaded.to_rgba8();
            let target = (small * wm.image_scale.clamp(0.02, 1.0)).max(8.0) as u32;
            let scale = target as f32 / img.width().max(1) as f32;
            let nw = (img.width() as f32 * scale).max(1.0) as u32;
            let nh = (img.height() as f32 * scale).max(1.0) as u32;
            img = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
            layers.push(img);
        }
    }
    if !wm.text.trim().is_empty() {
        if let Some(text_layer) =
            render_text_layer(&wm.text, small * wm.text_scale.clamp(0.01, 0.5))
        {
            layers.push(text_layer);
        }
    }
    if layers.is_empty() {
        return;
    }

    for layer in &layers {
        let (lw, lh) = (layer.width() as i32, layer.height() as i32);
        // Apply global opacity by scaling each layer's alpha.
        let mut layer = layer.clone();
        for p in layer.pixels_mut() {
            p.0[3] = (p.0[3] as f32 * opacity) as u8;
        }
        match wm.position {
            WatermarkPos::Tile => {
                let gap = (lw as f32 * 0.6).max(8.0) as i32;
                let step_x = lw + gap;
                let step_y = lh + gap;
                let mut y = 0;
                while y < ih {
                    let mut x = 0;
                    while x < iw {
                        stamp_layer(image, &layer, x, y);
                        x += step_x;
                    }
                    y += step_y;
                }
            }
            WatermarkPos::Center => {
                stamp_layer(image, &layer, (iw - lw) / 2, (ih - lh) / 2);
            }
            WatermarkPos::TopLeft => {
                stamp_layer(image, &layer, wm.margin as i32, wm.margin as i32);
            }
            WatermarkPos::TopRight => {
                stamp_layer(image, &layer, iw - lw - wm.margin as i32, wm.margin as i32);
            }
            WatermarkPos::BottomLeft => {
                stamp_layer(image, &layer, wm.margin as i32, ih - lh - wm.margin as i32);
            }
            WatermarkPos::BottomRight => {
                stamp_layer(
                    image,
                    &layer,
                    iw - lw - wm.margin as i32,
                    ih - lh - wm.margin as i32,
                );
            }
        }
    }
}

/// Rasterise `text` to a straight-alpha RGBA layer using the UI font.
fn render_text_layer(text: &str, px: f32) -> Option<RgbaImage> {
    use tiny_skia::Pixmap;

    let tf = crate::text_font::font();
    let px = px.clamp(8.0, 400.0);
    let lines: Vec<&str> = text.split('\n').collect();
    let line_h = tf.line_height(px);
    // 2 px of padding all round leaves room for the 1 px dark halo.
    let pad = 2.0_f32;
    let width = lines
        .iter()
        .map(|l| tf.text_width(l, px))
        .fold(0.0f32, f32::max)
        .ceil()
        .max(1.0) as u32
        + 4;
    let height = (line_h * lines.len() as f32).ceil().max(1.0) as u32 + 4;
    let mut pm = Pixmap::new(width, height)?;
    for (i, line) in lines.iter().enumerate() {
        let top = pad + i as f32 * line_h;
        // A dark halo keeps white text legible on light captures (documents,
        // white pages) as well as on photos.
        for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
            tf.draw(&mut pm, line, pad + dx, top + dy, px, [0, 0, 0, 170]);
        }
        tf.draw(&mut pm, line, pad, top, px, [255, 255, 255, 255]);
    }
    // tiny-skia stores premultiplied alpha; un-premultiply so we can re-blend
    // at an arbitrary opacity.
    let mut img = RgbaImage::new(width, height);
    for (dst, src) in img.pixels_mut().zip(pm.data().chunks_exact(4)) {
        let a = src[3] as f32 / 255.0;
        if a > 0.0 {
            dst.0 = [
                (src[0] as f32 / a) as u8,
                (src[1] as f32 / a) as u8,
                (src[2] as f32 / a) as u8,
                src[3],
            ];
        } else {
            dst.0 = [0, 0, 0, 0];
        }
    }
    Some(img)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_decorations_passthrough() {
        let img = RgbaImage::from_pixel(10, 10, Rgba([1, 2, 3, 255]));
        let out = decorate_image(&img, &Decorations::default());
        assert_eq!(out.dimensions(), (10, 10));
        assert_eq!(out.get_pixel(0, 0), &Rgba([1, 2, 3, 255]));
    }

    #[test]
    fn border_expands_canvas() {
        let img = RgbaImage::from_pixel(20, 10, Rgba([9, 9, 9, 255]));
        let deco = Decorations {
            border: Some(BorderDeco {
                width: 5,
                color: [255, 0, 0, 255],
            }),
            ..Default::default()
        };
        let out = decorate_image(&img, &deco);
        assert_eq!(out.dimensions(), (30, 20));
        assert_eq!(out.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(out.get_pixel(5, 5), &Rgba([9, 9, 9, 255]));
    }

    #[test]
    fn shadow_expands_canvas_and_keeps_content() {
        let img = RgbaImage::from_pixel(20, 20, Rgba([7, 7, 7, 255]));
        let deco = Decorations {
            shadow: Some(ShadowDeco {
                blur: 4,
                offset_x: 0,
                offset_y: 2,
                color: [0, 0, 0],
                opacity: 120,
            }),
            ..Default::default()
        };
        let out = decorate_image(&img, &deco);
        // Canvas grew by padding on every side.
        assert!(out.width() > 20 && out.height() > 20);
        // Content (at pad, pad) is intact and opaque.
        let pad = (out.width() - 20) / 2;
        assert_eq!(out.get_pixel(pad, pad), &Rgba([7, 7, 7, 255]));
    }

    #[test]
    fn shadow_edge_is_soft() {
        // Regression: a mask blurred at the image edge used to clamp and stay
        // fully opaque, producing a hard shadow line instead of a gradient.
        let img = RgbaImage::from_pixel(40, 40, Rgba([10, 10, 10, 255]));
        let deco = Decorations {
            shadow: Some(ShadowDeco {
                blur: 6,
                offset_x: 0,
                offset_y: 0,
                color: [0, 0, 0],
                opacity: 200,
            }),
            ..Default::default()
        };
        let out = decorate_image(&img, &deco);
        let x = out.width() / 2;
        let mut distinct = std::collections::BTreeSet::new();
        for y in 0..out.height() {
            distinct.insert(out.get_pixel(x, y).0[3]);
        }
        // A soft shadow shows many alpha levels; a hard one shows two.
        assert!(
            distinct.len() > 5,
            "shadow should have a gradient, got {distinct:?}"
        );
    }

    #[test]
    fn watermark_text_renders_pixels() {
        let img = RgbaImage::from_pixel(120, 60, Rgba([200, 200, 200, 255]));
        let deco = Decorations {
            watermark: Some(WatermarkDeco {
                text: "PinSight".into(),
                image_path: None,
                position: WatermarkPos::BottomRight,
                opacity: 180,
                text_scale: 0.12,
                image_scale: 0.2,
                margin: 6,
            }),
            ..Default::default()
        };
        let out = decorate_image(&img, &deco);
        // The mark must alter at least one pixel away from the flat background.
        let mut changed = false;
        for p in out.pixels() {
            if p.0[0] != 200 {
                changed = true;
                break;
            }
        }
        assert!(changed, "watermark should alter some pixels");
    }
}
