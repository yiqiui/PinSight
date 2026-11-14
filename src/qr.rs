//! QR code generation and recognition.
//!
//! Generation uses the pure-Rust `qrcode` encoder; recognition uses `rqrr` (a
//! Rust port of quirc). Both are CPU-only and platform independent, so this
//! works on every build target — unlike OCR, which relies on OS frameworks.
//! The heavy decoding runs off the GUI thread (see `overlay.rs`).

use image::{Rgba, RgbaImage};

/// Build a QR code image from `text`.
///
/// `module_px` is the size of one black/white square; the returned picture is
/// opaque with a 4-module quiet zone, so it can be copied, saved or pinned as
/// a normal capture. Returns an error if the payload is too large to encode.
pub fn generate(text: &str, module_px: u32) -> Result<RgbaImage, String> {
    if text.is_empty() {
        return Err("empty text".into());
    }
    let module_px = module_px.clamp(1, 32);
    let code = qrcode::QrCode::new(text.as_bytes()).map_err(|e| e.to_string())?;
    let n = code.width(); // modules per side
    let colors = code.to_colors(); // row-major, n * n
    let quiet = 4usize;
    let side = (n + quiet * 2) as u32 * module_px;
    let mut img = RgbaImage::from_pixel(side, side, Rgba([255, 255, 255, 255]));
    for (i, c) in colors.iter().enumerate() {
        if *c != qrcode::Color::Dark {
            continue;
        }
        let mx = i % n;
        let my = i / n;
        let x0 = ((mx + quiet) as u32) * module_px;
        let y0 = ((my + quiet) as u32) * module_px;
        for y in y0..y0 + module_px {
            for x in x0..x0 + module_px {
                img.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
    }
    Ok(img)
}

/// Avoid pathological allocations (a 4K frame at 2x is already > 32M pixels).
const MAX_SCAN_PIXELS: usize = 32_000_000;
/// Tile size / overlap for the large-frame fallback path.
const TILE: u32 = 1024;
const TILE_OVERLAP: u32 = 256;

/// Conditional speckle removal over the luma buffer. Salt-and-pepper specks
/// (single pixels flipped to the opposite extreme, plus the spurious edges they
/// draw in the quiet zone) are the one degradation rqrr's grid finder is
/// fragile to. A *plain* 3×3 median would also erase genuinely small modules
/// (a 2px-module code has features narrower than the kernel), so instead we
/// only rewrite a pixel when it is an isolated outlier: its 8 neighbours agree
/// on the opposite tone with at least 6 of 8 votes. That kills lone specks
/// while leaving module interiors and edges intact (they always keep ≥3
/// same-tone neighbours), and on a perfectly clean capture nothing changes.
fn despeckle(luma: &mut [u8], w: usize, h: usize) {
    if w < 3 || h < 3 {
        return;
    }
    let src = luma.to_vec(); // snapshot so we read neighbours from the old frame
    for y in 0..h {
        for x in 0..w {
            let c = src[y * w + x];
            let c_dark = c < 128;
            let mut dark = 0u32;
            let mut light = 0u32;
            let mut dark_sum = 0u32;
            let mut light_sum = 0u32;
            for dy in -1i32..=1i32 {
                for dx in -1i32..=1i32 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let nx = (x as i32 + dx).clamp(0, w as i32 - 1) as usize;
                    let ny = (y as i32 + dy).clamp(0, h as i32 - 1) as usize;
                    let v = src[ny * w + nx];
                    if v < 128 {
                        dark += 1;
                        dark_sum += v as u32;
                    } else {
                        light += 1;
                        light_sum += v as u32;
                    }
                }
            }
            if c_dark && light >= 6 {
                luma[y * w + x] = (light_sum / light) as u8;
            } else if !c_dark && dark >= 6 {
                luma[y * w + x] = (dark_sum / dark) as u8;
            }
        }
    }
}

/// Run rqrr over one scaled/inverted view of a luma buffer. `pixel` maps
/// output coordinates back to a source sample, so the whole frame and individual
/// tiles can share the same decoder without copying pixels.
fn scan<F>(sw: usize, sh: usize, pixel: F, found: &mut Vec<String>)
where
    F: FnMut(usize, usize) -> u8,
{
    if sw == 0 || sh == 0 || sw.saturating_mul(sh) > MAX_SCAN_PIXELS {
        return;
    }
    let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(sw, sh, pixel);
    for grid in prepared.detect_grids() {
        if let Ok((_, text)) = grid.decode() {
            if !text.is_empty() && !found.contains(&text) {
                found.push(text);
            }
        }
    }
}

/// Overlapping square tiles covering a large frame. The overlap matters: a
/// code straddling a tile boundary must land wholly inside at least one tile.
fn tile_rects(w: u32, h: u32) -> Vec<(u32, u32, u32, u32)> {
    if w == 0 || h == 0 {
        return Vec::new();
    }
    let step = TILE - TILE_OVERLAP;
    let mut out = Vec::new();
    let mut y = 0u32;
    loop {
        let th = TILE.min(h - y);
        let mut x = 0u32;
        loop {
            let tw = TILE.min(w - x);
            out.push((x, y, tw, th));
            if x + TILE >= w {
                break;
            }
            x += step;
        }
        if y + TILE >= h {
            break;
        }
        y += step;
    }
    out
}

/// Decode every QR code visible in `image`, returning their payloads in the
/// order the detector found them. An empty vector means "no QR code found"
/// (not an error); a QR that is present but damaged may also decode empty.
///
/// Screenshots often contain codes drawn small or inverted (light-on-dark).
/// We first try the whole frame at 1x (plus 2x/3x for small images), then —
/// for large frames where a full 2x upscale would blow the memory cap — scan
/// overlapping tiles at 2x. Decoding uses a luma callback, avoiding rqrr's
/// `image` feature entirely.
pub fn recognize(image: &RgbaImage) -> Vec<String> {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return Vec::new();
    }
    // Composite over white so translucent pixels do not read as black.
    let mut luma = vec![0u8; (w as usize) * (h as usize)];
    for y in 0..h {
        for x in 0..w {
            let p = image.get_pixel(x, y).0;
            let a = p[3] as f32 / 255.0;
            let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)) as u8;
            let r = blend(p[0]) as u32;
            let g = blend(p[1]) as u32;
            let b = blend(p[2]) as u32;
            // Rec. 601 luma, integer-only so it stays cheap on big frames.
            luma[(y as usize) * (w as usize) + x as usize] =
                ((r * 299 + g * 587 + b * 114) / 1000) as u8;
        }
    }
    // Drop isolated salt-and-pepper specks before detection (see `despeckle`).
    despeckle(&mut luma, w as usize, h as usize);

    let mut found: Vec<String> = Vec::new();
    let max_side = w.max(h);
    let scales: &[u32] = if max_side < 600 { &[1, 2, 3] } else { &[1] };
    for invert in [false, true] {
        for &scale in scales {
            let sw = (w * scale) as usize;
            let sh = (h * scale) as usize;
            scan(
                sw,
                sh,
                |x, y| {
                    let src = luma[(y / scale as usize) * (w as usize) + (x / scale as usize)];
                    if invert { 255 - src } else { src }
                },
                &mut found,
            );
            if !found.is_empty() {
                return found;
            }
        }
    }

    // Large frame: a whole-frame 2x upscale is skipped by the memory guard, so
    // small codes hidden in it would be missed. Scan overlapping tiles instead,
    // with a wall-clock budget so a frame with no codes cannot stall the worker.
    if max_side >= 600 {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
        for (tx, ty, tw, th) in tile_rects(w, h) {
            if std::time::Instant::now() >= deadline {
                break;
            }
            for invert in [false, true] {
                // One upscale level is enough on a tile (2x already gives
                // small codes ~2px modules); more would just burn the budget.
                let scale = 2u32;
                let sw = (tw * scale) as usize;
                let sh = (th * scale) as usize;
                scan(
                    sw,
                    sh,
                    |x, y| {
                        let sx = tx + x as u32 / scale;
                        let sy = ty + y as u32 / scale;
                        let src = luma[sy as usize * w as usize + sx as usize];
                        if invert { 255 - src } else { src }
                    },
                    &mut found,
                );
                if !found.is_empty() {
                    return found;
                }
            }
        }
    }
    found
}

/// What the generator window should do with the rendered code after the user
/// clicks a button. Carries the image because the window closure borrows `self`.
pub(crate) enum GenAction {
    Copy(std::sync::Arc<RgbaImage>),
    Save(std::sync::Arc<RgbaImage>),
    Pin(std::sync::Arc<RgbaImage>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_a_square_image_with_quiet_zone() {
        let img = generate("https://example.com", 6).unwrap();
        let side = img.width();
        assert_eq!(side, img.height());
        assert!(side > 100);
        // The 4-module quiet zone along the top-left corner is white.
        assert_eq!(img.get_pixel(0, 0).0, [255, 255, 255, 255]);
        // And some modules are black.
        assert!(img.pixels().any(|p| p.0[0] == 0));
    }

    #[test]
    fn rejects_empty_and_overlong_payloads() {
        assert!(generate("", 4).is_err());
        let huge = "x".repeat(10_000);
        assert!(generate(&huge, 4).is_err());
    }

    #[test]
    fn round_trip_generate_then_recognize() {
        let payload = "PinSight QR round-trip 12345";
        let img = generate(payload, 8).unwrap();
        let decoded = recognize(&img);
        assert_eq!(decoded, vec![payload.to_string()]);
    }

    #[test]
    fn recognize_handles_blank_and_tiny_images() {
        let blank = RgbaImage::from_pixel(64, 64, Rgba([255, 255, 255, 255]));
        assert!(recognize(&blank).is_empty());
        let empty = RgbaImage::new(0, 0);
        assert!(recognize(&empty).is_empty());
    }

    #[test]
    fn recognize_handles_inverted_codes() {
        let payload = "inverted";
        let qr = generate(payload, 8).unwrap();
        let mut inverted = qr.clone();
        for p in inverted.pixels_mut() {
            p.0[0] = 255 - p.0[0];
            p.0[1] = 255 - p.0[1];
            p.0[2] = 255 - p.0[2];
        }
        let decoded = recognize(&inverted);
        assert_eq!(decoded, vec![payload.to_string()]);
    }

    #[test]
    fn tile_rects_cover_every_pixel_with_overlap() {
        let (w, h) = (3000u32, 2000u32);
        let rects = tile_rects(w, h);
        assert!(!rects.is_empty());
        let mut covered = vec![false; (w * h) as usize];
        for &(x, y, tw, th) in &rects {
            assert!(x + tw <= w && y + th <= h);
            for py in y..y + th {
                for px in x..x + tw {
                    covered[(py * w + px) as usize] = true;
                }
            }
        }
        assert!(covered.iter().all(|c| *c), "a pixel is left uncovered");
        // Consecutive columns must overlap, not just abut.
        assert!(rects.windows(2).any(|pair| {
            pair[1].1 == pair[0].1
                && pair[1].0 > pair[0].0
                && pair[1].0 - pair[0].0 == TILE - TILE_OVERLAP
        }));
    }

    #[test]
    fn recognize_finds_a_small_code_in_a_large_frame() {
        let payload = "small-code-in-big-frame";
        // 2px modules: the whole-frame 1x pass may miss it, the tiled 2x pass
        // must recover it (this is the path large screenshots rely on).
        let qr = generate(payload, 2).unwrap();
        let mut big = RgbaImage::from_pixel(1200, 900, Rgba([255, 255, 255, 255]));
        for sy in 0..qr.height() {
            for sx in 0..qr.width() {
                big.put_pixel(700 + sx, 500 + sy, *qr.get_pixel(sx, sy));
            }
        }
        let decoded = recognize(&big);
        assert_eq!(decoded, vec![payload.to_string()]);
    }

    // ---- robustness under realistic capture degradations ----
    //
    // Screenshots are rarely pristine: they pick up defocus/motion blur, washed
    // out low-contrast UIs, tilted phone photos, JPEG compression noise and the
    // occasional finger/reflection. These tests lock the decoder's behaviour on
    // *moderate* versions of each so a future refactor cannot silently regress
    // recognition, and document (via comments) how far rqrr carries us.

    /// Deterministic xorshift RNG so noise tests are reproducible.
    fn rng(seed: u64) -> impl FnMut() -> u64 {
        let mut s = seed;
        move || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        }
    }

    /// Shrink the dynamic range by pulling every channel toward mid-gray (128).
    /// `factor = 1.0` is the original; `0.5` halves the departure from grey.
    fn degrade_low_contrast(img: &RgbaImage, factor: f32) -> RgbaImage {
        let mut out = img.clone();
        for p in out.pixels_mut() {
            // Iterate the RGB channels as a slice: rustc 1.100 denies a loop
            // variable that only indexes a single tuple field (`p.0[c]`).
            for channel in &mut p.0[..3] {
                let v = *channel as f32;
                *channel = (128.0 + (v - 128.0) * factor).clamp(0.0, 255.0) as u8;
            }
        }
        out
    }

    /// Flip `density` fraction of pixels to the opposite extreme — a stand-in
    /// for salt-and-pepper / heavy JPEG-artifact damage. Seeded for stability.
    fn degrade_salt_pepper(img: &RgbaImage, density: f32, seed: u64) -> RgbaImage {
        let mut out = img.clone();
        let mut rnd = rng(seed);
        let span = u64::MAX as f64;
        for p in out.pixels_mut() {
            if (rnd() as f64) / span < density as f64 {
                let light = p.0[0] > 127;
                let v = if light { 0u8 } else { 255u8 };
                p.0[0] = v;
                p.0[1] = v;
                p.0[2] = v;
            }
        }
        out
    }

    /// Paint a solid white block over a data region of the symbol (avoiding the
    /// three finder patterns) to emulate occlusion by a finger or reflection.
    fn occlude_data_block(img: &RgbaImage, modules: u32, module_px: u32) -> RgbaImage {
        let mut out = img.clone();
        let n = img.width() / module_px; // modules per side (incl. quiet zone)
        let center = n / 2;
        let x0 = (center - modules / 2) * module_px;
        let y0 = (center - modules / 2) * module_px;
        for py in y0..(y0 + modules * module_px).min(out.height()) {
            for px in x0..(x0 + modules * module_px).min(out.width()) {
                out.put_pixel(px, py, Rgba([255, 255, 255, 255]));
            }
        }
        out
    }

    /// Rotate by `deg` degrees about the centre into a white-background canvas
    /// sized to the image diagonal (no clipping). Nearest-neighbour sampling;
    /// this avoids depending on the affine feature and keeps the test offline.
    fn rotate_nearest(img: &RgbaImage, deg: f32) -> RgbaImage {
        let (w, h) = img.dimensions();
        let diag = (w as f32).hypot(h as f32).ceil() as u32;
        let mut out = RgbaImage::from_pixel(diag, diag, Rgba([255, 255, 255, 255]));
        let cx = w as f32 / 2.0;
        let cy = h as f32 / 2.0;
        let ocx = diag as f32 / 2.0;
        let ocy = diag as f32 / 2.0;
        let rad = deg.to_radians();
        let cos = rad.cos();
        let sin = rad.sin();
        for oy in 0..diag {
            for ox in 0..diag {
                let dx = ox as f32 - ocx;
                let dy = oy as f32 - ocy;
                let sx = cx + dx * cos + dy * sin;
                let sy = cy - dx * sin + dy * cos;
                if sx >= 0.0 && sy >= 0.0 && sx < w as f32 && sy < h as f32 {
                    *out.get_pixel_mut(ox, oy) = *img.get_pixel(sx as u32, sy as u32);
                }
            }
        }
        out
    }

    #[test]
    fn recognize_survives_mild_gaussian_blur() {
        // Defocus / motion blur on a 10px-module code (sigma 2.0 px ≈ 0.2 module).
        let payload = "blur-resilience-check-abc123";
        let img = generate(payload, 10).unwrap();
        let blurred = image::imageops::blur(&img, 2.0);
        assert_eq!(recognize(&blurred), vec![payload.to_string()]);
    }

    #[test]
    fn recognize_survives_low_contrast() {
        // A washed-out / dark-mode UI where black-on-white has collapsed toward grey.
        let payload = "low-contrast-abc123";
        let img = generate(payload, 10).unwrap();
        let faded = degrade_low_contrast(&img, 0.5);
        assert_eq!(recognize(&faded), vec![payload.to_string()]);
    }

    #[test]
    fn recognize_survives_rotation_90() {
        let payload = "rotation-90-abc123";
        let img = generate(payload, 10).unwrap();
        assert_eq!(
            recognize(&image::imageops::rotate90(&img)),
            vec![payload.to_string()]
        );
    }

    #[test]
    fn recognize_survives_small_tilt() {
        // A tilted phone photo of a screen (~10°); rqrr samples the grid via a
        // homography so mild rotation must not break it.
        let payload = "tilt-10deg-abc123";
        let img = generate(payload, 10).unwrap();
        assert_eq!(
            recognize(&rotate_nearest(&img, 10.0)),
            vec![payload.to_string()]
        );
    }

    #[test]
    fn recognize_survives_salt_and_pepper_noise() {
        // Heavy JPEG-style speckle: 5% of pixels flipped to the opposite extreme.
        let payload = "noise-5pct-abc123";
        let img = generate(payload, 10).unwrap();
        let noisy = degrade_salt_pepper(&img, 0.05, 0x9E3779B9);
        assert_eq!(recognize(&noisy), vec![payload.to_string()]);
    }

    #[test]
    fn recognize_survives_data_occlusion() {
        // A 5×5-module white block over the centre data area (finder patterns
        // untouched) — emulates a finger/reflection; Reed-Solomon covers it.
        let payload = "occlusion-5x5-abc123";
        let img = generate(payload, 10).unwrap();
        let blocked = occlude_data_block(&img, 5, 10);
        assert_eq!(recognize(&blocked), vec![payload.to_string()]);
    }
}
