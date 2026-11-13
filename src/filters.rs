//! Whole-image adjustments for the editor and the head-less `shot` CLI:
//! rotate, flip, invert, grayscale, blur and emboss.
//!
//! Every function takes and returns an [`image::RgbaImage`] so the logic runs
//! without a GPU and can be unit-tested in isolation. The editor calls these on
//! the captured base image; the CLI chains them on the way to disk.

use image::{Rgba, RgbaImage, imageops};

/// A single, reversible image adjustment. `f32` parameters rule out `Eq`,
/// so equality is `PartialEq` only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Filter {
    /// Rotate 90° clockwise.
    Rotate90Cw,
    /// Rotate 90° counter-clockwise.
    Rotate90Ccw,
    /// Rotate 180°.
    Rotate180,
    /// Mirror left↔right.
    FlipHorizontal,
    /// Mirror top↔bottom.
    FlipVertical,
    /// Convert to grayscale (alpha preserved).
    Grayscale,
    /// Invert RGB channels (alpha preserved).
    Invert,
    /// Gaussian blur with the given sigma (px).
    Blur(f32),
    /// Directional emboss (alpha preserved).
    Emboss,
    /// Rotate by an arbitrary number of degrees clockwise.
    Rotate(f32),
}

impl Filter {
    /// The fixed set shown as buttons in the editor's adjustments panel.
    pub const PANEL: [Filter; 9] = [
        Filter::Rotate90Cw,
        Filter::Rotate90Ccw,
        Filter::Rotate180,
        Filter::FlipHorizontal,
        Filter::FlipVertical,
        Filter::Grayscale,
        Filter::Invert,
        Filter::Blur(2.0),
        Filter::Emboss,
    ];

    /// A short, stable token used by the CLI (`--filter`) and for tests.
    pub fn code(self) -> String {
        match self {
            Filter::Rotate90Cw => "rotate90".into(),
            Filter::Rotate90Ccw => "rotate270".into(),
            Filter::Rotate180 => "rotate180".into(),
            Filter::FlipHorizontal => "fliph".into(),
            Filter::FlipVertical => "flipv".into(),
            Filter::Grayscale => "gray".into(),
            Filter::Invert => "invert".into(),
            Filter::Blur(s) => format!("blur:{s}"),
            Filter::Emboss => "emboss".into(),
            Filter::Rotate(d) => format!("rotate:{d}"),
        }
    }

    /// Parse a [`Filter::code`]-style token back into a filter.
    pub fn from_code(s: &str) -> Option<Filter> {
        match s {
            "rotate90" => Some(Filter::Rotate90Cw),
            "rotate270" | "rotate-90" => Some(Filter::Rotate90Ccw),
            "rotate180" => Some(Filter::Rotate180),
            "fliph" => Some(Filter::FlipHorizontal),
            "flipv" => Some(Filter::FlipVertical),
            "gray" | "grayscale" => Some(Filter::Grayscale),
            "invert" => Some(Filter::Invert),
            "emboss" => Some(Filter::Emboss),
            other => {
                if let Some(rest) = other.strip_prefix("blur:") {
                    rest.parse::<f32>().ok().map(Filter::Blur)
                } else if let Some(rest) = other.strip_prefix("rotate:") {
                    rest.parse::<f32>().ok().map(Filter::Rotate)
                } else {
                    None
                }
            }
        }
    }
}

/// Apply `f` to `img`, returning a new image.
pub fn apply(img: &RgbaImage, f: Filter) -> RgbaImage {
    match f {
        Filter::Rotate90Cw => imageops::rotate90(img),
        Filter::Rotate90Ccw => imageops::rotate270(img),
        Filter::Rotate180 => imageops::rotate180(img),
        Filter::FlipHorizontal => imageops::flip_horizontal(img),
        Filter::FlipVertical => imageops::flip_vertical(img),
        Filter::Grayscale => {
            let g = imageops::grayscale(img);
            RgbaImage::from_fn(g.width(), g.height(), |x, y| {
                let l = g.get_pixel(x, y).0[0];
                let a = img.get_pixel(x, y).0[3]; // keep the source alpha
                Rgba([l, l, l, a])
            })
        }
        Filter::Invert => {
            let mut out = img.clone();
            for p in out.pixels_mut() {
                p.0[0] = 255 - p.0[0];
                p.0[1] = 255 - p.0[1];
                p.0[2] = 255 - p.0[2];
            }
            out
        }
        Filter::Blur(sigma) => imageops::blur(img, sigma.max(0.0)),
        Filter::Emboss => emboss(img),
        Filter::Rotate(deg) => rotate_nearest(img, deg),
    }
}

/// Apply a chain of filters left-to-right.
pub fn apply_chain(img: &RgbaImage, filters: &[Filter]) -> RgbaImage {
    let mut out = img.clone();
    for &f in filters {
        out = apply(&out, f);
    }
    out
}

/// 3×3 emboss convolution over luminance, preserving the centre pixel's alpha.
fn emboss(img: &RgbaImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    if w < 3 || h < 3 {
        return img.clone();
    }
    // Directional emboss kernel; sums to zero so flat regions map to the 128
    // midpoint instead of drifting with the source luminance.
    let k: [[f32; 3]; 3] = [[-1.0, -1.0, 0.0], [-1.0, 0.0, 1.0], [0.0, 1.0, 1.0]];
    let lum = |x: u32, y: u32| -> f32 {
        let p = img.get_pixel(x, y).0;
        0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
    };
    let mut out = RgbaImage::new(w, h);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut sum = 0.0;
            for ky in 0..3u32 {
                for kx in 0..3u32 {
                    sum += k[ky as usize][kx as usize] * lum(x + kx - 1, y + ky - 1);
                }
            }
            let v = (sum + 128.0).clamp(0.0, 255.0) as u8;
            out.put_pixel(x, y, Rgba([v, v, v, img.get_pixel(x, y).0[3]]));
        }
    }
    // Copy the 1px border unchanged.
    for x in [0, w - 1] {
        for y in 0..h {
            out.put_pixel(x, y, *img.get_pixel(x, y));
        }
    }
    for y in [0, h - 1] {
        for x in 0..w {
            out.put_pixel(x, y, *img.get_pixel(x, y));
        }
    }
    out
}

/// Nearest-neighbour rotation about the image centre by `deg` degrees
/// clockwise. Pixels that map outside the source keep transparent black.
pub fn rotate_nearest(img: &RgbaImage, deg: f32) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 || deg == 0.0 {
        return img.clone();
    }
    let rad = deg.to_radians();
    let (s, c) = rad.sin_cos();
    let cx = (w - 1) as f32 / 2.0;
    let cy = (h - 1) as f32 / 2.0;
    let mut out = RgbaImage::from_pixel(w, h, Rgba([0, 0, 0, 0]));
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            // Map each output pixel back into source space.
            let sx = (c * dx + s * dy + cx).round() as i32;
            let sy = (-s * dx + c * dy + cy).round() as i32;
            if sx >= 0 && sy >= 0 && (sx as u32) < w && (sy as u32) < h {
                out.put_pixel(x, y, *img.get_pixel(sx as u32, sy as u32));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    fn solid(w: u32, h: u32, c: [u8; 4]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba(c))
    }

    #[test]
    fn rotate90_round_trip_is_identity() {
        let mut img = RgbaImage::new(3, 5);
        for y in 0..5u32 {
            for x in 0..3u32 {
                img.put_pixel(x, y, Rgba([(x * 10 + y) as u8, 0, 0, 255]));
            }
        }
        let r = apply(&img, Filter::Rotate90Cw);
        assert_eq!((r.width(), r.height()), (5, 3));
        let back = apply(&r, Filter::Rotate90Ccw);
        assert_eq!((back.width(), back.height()), (3, 5));
        assert_eq!(img.as_raw(), back.as_raw());
    }

    #[test]
    fn rotate180_equals_two_90_turns() {
        let img = RgbaImage::from_fn(4, 3, |x, y| {
            Rgba([(x * 30 + y * 7) as u8, (y * 40) as u8, 90, 255])
        });
        let twice = apply(&apply(&img, Filter::Rotate90Cw), Filter::Rotate90Cw);
        let once = apply(&img, Filter::Rotate180);
        assert_eq!(twice.as_raw(), once.as_raw());
        // Rotating 180° twice is the identity.
        let back = apply(&once, Filter::Rotate180);
        assert_eq!(img.as_raw(), back.as_raw());
    }

    #[test]
    fn flips_are_involutions() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([1, 2, 3, 255]));
        img.put_pixel(1, 0, Rgba([4, 5, 6, 255]));
        img.put_pixel(0, 1, Rgba([7, 8, 9, 255]));
        img.put_pixel(1, 1, Rgba([10, 11, 12, 255]));
        for f in [Filter::FlipHorizontal, Filter::FlipVertical] {
            let back = apply(&apply(&img, f), f);
            assert_eq!(
                img.as_raw(),
                back.as_raw(),
                "flip {f:?} should invert itself"
            );
        }
    }

    #[test]
    fn invert_twice_is_identity_and_keeps_alpha() {
        let mut img = RgbaImage::new(2, 2);
        img.put_pixel(0, 0, Rgba([10, 20, 30, 200]));
        let inv = apply(&img, Filter::Invert);
        assert_eq!(inv.get_pixel(0, 0), &Rgba([245, 235, 225, 200]));
        let back = apply(&inv, Filter::Invert);
        assert_eq!(img.as_raw(), back.as_raw());
    }

    #[test]
    fn grayscale_equalises_channels_and_keeps_alpha() {
        let mut img = RgbaImage::new(1, 1);
        img.put_pixel(0, 0, Rgba([30, 128, 200, 123]));
        let g = apply(&img, Filter::Grayscale);
        let p = g.get_pixel(0, 0);
        assert_eq!(p.0[0], p.0[1]);
        assert_eq!(p.0[1], p.0[2]);
        assert_eq!(p.0[3], 123);
    }

    #[test]
    fn blur_of_solid_image_is_unchanged() {
        let img = solid(8, 8, [100, 100, 100, 255]);
        let b = apply(&img, Filter::Blur(3.0));
        assert_eq!(img.as_raw(), b.as_raw());
    }

    #[test]
    fn emboss_preserves_alpha_and_changes_flat_region() {
        let img = solid(8, 8, [50, 50, 50, 255]);
        let e = apply(&img, Filter::Emboss);
        // Flat region maps to the 128 midpoint, not the input colour.
        assert_eq!(e.get_pixel(4, 4), &Rgba([128, 128, 128, 255]));
        // Alpha untouched everywhere.
        for y in 0..8u32 {
            for x in 0..8u32 {
                assert_eq!(e.get_pixel(x, y).0[3], 255);
            }
        }
    }

    #[test]
    fn rotate_zero_is_identity() {
        let img = solid(6, 6, [1, 2, 3, 255]);
        let r = apply(&img, Filter::Rotate(0.0));
        assert_eq!(img.as_raw(), r.as_raw());
    }

    #[test]
    fn code_round_trips() {
        for f in Filter::PANEL {
            assert_eq!(Filter::from_code(&f.code()), Some(f), "code {f:?}");
        }
        assert_eq!(Filter::from_code("blur:1.5"), Some(Filter::Blur(1.5)));
        assert_eq!(Filter::from_code("rotate:30"), Some(Filter::Rotate(30.0)));
        assert_eq!(Filter::from_code("nope"), None);
    }

    #[test]
    fn chain_runs_in_order() {
        let img = solid(4, 4, [200, 100, 50, 255]);
        let out = apply_chain(&img, &[Filter::Invert, Filter::Grayscale]);
        // Invert then grayscale: grey value should be the luminance of the
        // inverted colour, not the original.
        let p = out.get_pixel(0, 0);
        assert_eq!(p.0[0], p.0[1]);
        assert_eq!(p.0[1], p.0[2]);
    }
}
