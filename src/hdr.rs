//! HDR (advanced colour) detection.
//!
//! Windows.Graphics.Capture — the pipeline `xcap` uses — returns washed-out
//! frames while a display runs in HDR mode: the desktop composites scRGB
//! float values, and the 8-bit capture path clamps them instead of tone
//! mapping. A GDI screen blit reads DWM's SDR compositing instead, which is
//! tone-mapped and looks correct. That is why [`crate::capture`] prefers the
//! GDI path on displays this module reports as HDR.
//!
//! Detection goes through DXGI: every output reports the colour space it is
//! currently driven in, and `RGB_FULL_G2084_NONE_P2020` (PQ / BT.2020) is the
//! signature of advanced colour being on.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::util::PhysRect;

/// Enumeration is cheap but not free, and the answer changes only when the
/// user flips the HDR switch — cache it for a couple of seconds.
const CACHE_TTL: Duration = Duration::from_secs(2);

struct Cache {
    at: Instant,
    rects: Vec<PhysRect>,
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

/// Physical rectangles of every display currently in HDR mode.
pub fn hdr_rects() -> Vec<PhysRect> {
    #[cfg(windows)]
    {
        windows_rects()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Whether the display covering `rect` is in HDR mode.
///
/// Matching is by geometric overlap: DXGI reports the output rectangle, and a
/// capture region usually sits inside one output. The output with the largest
/// shared area wins, so a region straddling two displays follows the one it
/// mostly lives on.
pub fn is_hdr(rect: &PhysRect) -> bool {
    best_match(rect, &hdr_rects()).is_some()
}

/// Whether any part of `rect` lands on an HDR display.
pub fn overlaps_hdr(rect: &PhysRect) -> bool {
    hdr_rects().iter().any(|hdr| intersects(rect, hdr))
}

/// Whether at least one display is in HDR mode.
pub fn any_hdr() -> bool {
    !hdr_rects().is_empty()
}

fn intersects(a: &PhysRect, b: &PhysRect) -> bool {
    let ix0 = a.x.max(b.x);
    let iy0 = a.y.max(b.y);
    let ix1 = (a.x + a.w as i32).min(b.x + b.w as i32);
    let iy1 = (a.y + a.h as i32).min(b.y + b.h as i32);
    ix1 > ix0 && iy1 > iy0
}

/// Largest-overlap HDR rectangle for `rect`, if any.
fn best_match(rect: &PhysRect, hdrs: &[PhysRect]) -> Option<PhysRect> {
    let ix0 = rect.x;
    let iy0 = rect.y;
    let ix1 = rect.x + rect.w as i32;
    let iy1 = rect.y + rect.h as i32;
    hdrs.iter()
        .filter_map(|hdr| {
            let x0 = ix0.max(hdr.x);
            let y0 = iy0.max(hdr.y);
            let x1 = ix1.min(hdr.x + hdr.w as i32);
            let y1 = iy1.min(hdr.y + hdr.h as i32);
            let overlap = (x1 - x0).max(0) as i64 * (y1 - y0).max(0) as i64;
            (overlap > 0).then_some((overlap, *hdr))
        })
        .max_by_key(|(overlap, _)| *overlap)
        .map(|(_, hdr)| hdr)
}

#[cfg(windows)]
fn windows_rects() -> Vec<PhysRect> {
    {
        let guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(cache) = guard.as_ref() {
            if cache.at.elapsed() < CACHE_TTL {
                return cache.rects.clone();
            }
        }
    }
    let rects = enumerate();
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = Some(Cache {
        at: Instant::now(),
        rects: rects.clone(),
    });
    rects
}

#[cfg(windows)]
fn enumerate() -> Vec<PhysRect> {
    use windows::Win32::Graphics::Dxgi::Common::DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020;
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, IDXGIOutput6};
    use windows::core::Interface;

    let mut out = Vec::new();
    unsafe {
        let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory1>() else {
            return out;
        };
        let mut adapter_index = 0u32;
        // A disconnected adapter makes EnumAdapters1 fail with the end-of-list
        // HRESULT, which is the loop's exit condition.
        while let Ok(adapter) = factory.EnumAdapters1(adapter_index) {
            adapter_index += 1;
            let mut output_index = 0u32;
            while let Ok(output) = adapter.EnumOutputs(output_index) {
                output_index += 1;
                let Ok(output6) = output.cast::<IDXGIOutput6>() else {
                    continue;
                };
                let Ok(desc) = output6.GetDesc1() else {
                    continue;
                };
                if desc.ColorSpace != DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 {
                    continue;
                }
                let rect = desc.DesktopCoordinates;
                out.push(PhysRect {
                    x: rect.left,
                    y: rect.top,
                    w: (rect.right - rect.left).max(0) as u32,
                    h: (rect.bottom - rect.top).max(0) as u32,
                });
            }
        }
    }
    out
}

#[cfg(not(windows))]
fn enumerate() -> Vec<PhysRect> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_prefers_the_dominant_display() {
        let left = PhysRect {
            x: 0,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let right = PhysRect {
            x: 1920,
            y: 0,
            w: 1920,
            h: 1080,
        };
        let region = PhysRect {
            x: 1900,
            y: 0,
            w: 200,
            h: 100,
        };
        assert_eq!(best_match(&region, &[left, right]), Some(right));
        assert!(intersects(&region, &left));
    }

    #[test]
    fn no_overlap_means_no_match() {
        let hdr = PhysRect {
            x: 0,
            y: 0,
            w: 100,
            h: 100,
        };
        let region = PhysRect {
            x: 500,
            y: 500,
            w: 50,
            h: 50,
        };
        assert_eq!(best_match(&region, &[hdr]), None);
        assert!(!intersects(&region, &hdr));
    }
}
