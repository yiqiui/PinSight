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
    let side = ((n + quiet * 2) as u32 * module_px) as u32;
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

/// Decode every QR code visible in `image`, returning their payloads in the
/// order the detector found them. An empty vector means "no QR code found"
/// (not an error); a QR that is present but damaged may also decode empty.
///
/// Screenshots often contain codes drawn small or inverted (light-on-dark),
/// so we try the native greyscale, an inverted copy, and a 2×/3× nearest
/// upscale of each. Decoding is done on a `PreparedImage` built from a luma
/// callback, avoiding any dependency on the `image` feature of `rqrr`.
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

    let mut found: Vec<String> = Vec::new();
    let max_side = w.max(h);
    let scales: &[u32] = if max_side < 600 { &[1, 2, 3] } else { &[1, 2] };
    for invert in [false, true] {
        for &scale in scales {
            let sw = (w * scale) as usize;
            let sh = (h * scale) as usize;
            // Guard against pathological allocations on very large frames.
            if sw.saturating_mul(sh) > 32_000_000 {
                continue;
            }
            let mut prepared = rqrr::PreparedImage::prepare_from_greyscale(sw, sh, |x, y| {
                let src = luma[(y / scale as usize) * (w as usize) + (x / scale as usize)];
                if invert {
                    255 - src
                } else {
                    src
                }
            });
            for grid in prepared.detect_grids() {
                if let Ok((_, text)) = grid.decode() {
                    if !text.is_empty() && !found.contains(&text) {
                        found.push(text);
                    }
                }
            }
            if !found.is_empty() {
                return found;
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
}
