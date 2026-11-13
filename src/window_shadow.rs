//! Drop-shadow options for whole-window captures.
//!
//! `capture::grab_window` asks the OS for the window's own pixels. On Windows
//! that is a WGC capture: it works even when the window is partly covered, but
//! it excludes the drop shadow the desktop compositor draws *outside* the
//! window. Two complementary ways to get a shadow are offered here:
//!
//! * `System` — read the DWM visible-frame bounds, enlarge the rectangle by the
//!   gap between those bounds and `GetWindowRect`, and grab that region from
//!   the screen. This reproduces Windows' real shadow, including the desktop
//!   wallpaper behind it. Because it is a screen grab, an overlapping window
//!   will show up in the margin.
//! * `Synthetic` — keep the occlusion-free WGC window grab, round its corners
//!   and cast a soft shadow onto a transparent canvas. Portable and clean for
//!   pasting into documents, but the shadow is a stylised approximation.
//!
//! `Off` is the default and preserves the exact previous behaviour.
//!
//! The geometry is kept in pure functions so it can be unit-tested on every
//! platform; only `window_frame` touches Win32.

use crate::capture;
use crate::decorate;
use crate::util::PhysRect;
use anyhow::Result;
use image::RgbaImage;
use serde::{Deserialize, Serialize};

/// How a whole-window capture handles the drop shadow.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Plain WGC window grab (no shadow, no extra margin).
    #[default]
    Off,
    /// Grab the real DWM shadow from the screen (Windows only).
    System,
    /// Draw a transparent, rounded shadow around the window grab.
    Synthetic,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Off, Mode::System, Mode::Synthetic];

    /// Stable CLI token.
    pub fn code(self) -> &'static str {
        match self {
            Mode::Off => "off",
            Mode::System => "system",
            Mode::Synthetic => "synthetic",
        }
    }

    pub fn from_code(s: &str) -> Option<Mode> {
        Mode::ALL
            .iter()
            .copied()
            .find(|m| m.code() == s.trim().to_ascii_lowercase())
    }
}

/// Never let a corrupted config produce a multi-thousand-pixel margin.
const MAX_INSET: i32 = 64;
const MAX_PADDING: u32 = 96;

/// Grow `rect` by the DWM shadow gaps plus a user padding.
///
/// `insets` is `[left, top, right, bottom]`; negative or absurd values are
/// clamped so a buggy DWM result cannot produce a negative size or a huge
/// allocation.
pub fn expand(rect: PhysRect, insets: [i32; 4], padding: u32) -> PhysRect {
    let p = padding.min(MAX_PADDING) as i32;
    let clamp = |v: i32| v.clamp(0, MAX_INSET);
    let l = clamp(insets[0]) + p;
    let t = clamp(insets[1]) + p;
    let r = clamp(insets[2]) + p;
    let b = clamp(insets[3]) + p;
    PhysRect {
        x: rect.x.saturating_sub(l),
        y: rect.y.saturating_sub(t),
        w: rect.w.saturating_add((l + r) as u32),
        h: rect.h.saturating_add((t + b) as u32),
    }
}

/// Capture a window according to `mode`. Falls back to a plain window grab if
/// the platform cannot supply the requested shadow, so a capture never fails
/// just because a decoration was asked for.
pub fn grab(id: u32, mode: Mode, padding: u32) -> Result<RgbaImage> {
    match mode {
        Mode::Off => capture::grab_window(id),
        Mode::System => grab_system(id, padding),
        Mode::Synthetic => {
            let image = capture::grab_window(id)?;
            let blur = padding.clamp(6, 24);
            Ok(decorate::window_shadow_frame(
                &image,
                10,
                blur,
                (blur / 4) as i32,
                110,
            ))
        }
    }
}

#[cfg(not(windows))]
fn grab_system(id: u32, _padding: u32) -> Result<RgbaImage> {
    // No DWM on other platforms. The synthetic mode is the portable option.
    capture::grab_window(id)
}

#[cfg(windows)]
fn grab_system(id: u32, padding: u32) -> Result<RgbaImage> {
    match window_frame(id) {
        Some((outer, insets)) => capture::grab_physical_rect(expand(outer, insets, padding)),
        None => capture::grab_window(id),
    }
}

/// The `GetWindowRect` bounds plus the DWM shadow gaps. Returns `None` only
/// when the id no longer names a live window; a failed DWM query still yields
/// the outer rectangle with zero insets (then just the user padding is used).
#[cfg(windows)]
pub fn window_frame(id: u32) -> Option<(PhysRect, [i32; 4])> {
    use std::ffi::c_void;
    use std::mem::size_of;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
    use windows::Win32::UI::WindowsAndMessaging::GetWindowRect;

    unsafe {
        // xcap's `Window::id()` is `HWND.0 as u32`; user-mode window handles
        // fit in 32 bits, so the cast back is lossless in practice. This is the
        // same narrowing xcap itself relies on.
        let hwnd = HWND(id as usize as *mut c_void);
        let mut outer = RECT::default();
        if GetWindowRect(hwnd, &mut outer).is_err() {
            return None;
        }
        let logical_w = (outer.right - outer.left).max(0) as u32;
        let logical_h = (outer.bottom - outer.top).max(0) as u32;
        if logical_w < 16 || logical_h < 16 {
            return None;
        }

        // Win32 returns *logical* coordinates when the process is DPI-unaware
        // (the headless CLI never starts winit, which is what normally sets
        // DPI awareness), while `grab_physical_rect` works in physical pixels.
        // Use xcap's physical rectangle as the basis and scale the DWM gaps by
        // the ratio so both live in one coordinate space. When the process is
        // already DPI-aware the ratio is 1 and this is a no-op.
        let physical = capture::window_rect(id).ok();
        let base = physical.unwrap_or(PhysRect {
            x: outer.left,
            y: outer.top,
            w: logical_w,
            h: logical_h,
        });
        let scale = physical
            .map(|p| p.w as f64 / logical_w as f64)
            .unwrap_or(1.0)
            .clamp(0.25, 4.0);
        let scaled = |v: i32| ((v as f64 * scale).round() as i32).clamp(0, MAX_INSET);

        let mut visible = RECT::default();
        let got_dwm = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut visible as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .is_ok();
        let insets = if got_dwm {
            [
                scaled((visible.left - outer.left).clamp(0, MAX_INSET)),
                scaled((visible.top - outer.top).clamp(0, MAX_INSET)),
                scaled((outer.right - visible.right).clamp(0, MAX_INSET)),
                scaled((outer.bottom - visible.bottom).clamp(0, MAX_INSET)),
            ]
        } else {
            [0; 4]
        };
        Some((base, insets))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_adds_insets_and_padding_on_every_side() {
        let r = PhysRect {
            x: 100,
            y: 200,
            w: 800,
            h: 600,
        };
        let out = expand(r, [7, 0, 7, 9], 12);
        assert_eq!(out.x, 81);
        assert_eq!(out.y, 188);
        assert_eq!(out.w, 838);
        assert_eq!(out.h, 633);
    }

    #[test]
    fn expand_clamps_hostile_values() {
        let r = PhysRect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        };
        let out = expand(r, [-999, 9999, 7, -5], 100_000);
        // padding capped at 96, top inset capped at 64, left/bottom clamped to 0
        assert_eq!(out.x, -96);
        assert_eq!(out.y, -(64 + 96));
        assert_eq!(out.w, 10 + 96 + (7 + 96));
        assert_eq!(out.h, 10 + (64 + 96) + 96);
    }

    #[test]
    fn expand_with_no_shadow_is_a_no_op_when_padding_is_zero() {
        let r = PhysRect {
            x: -50,
            y: 30,
            w: 40,
            h: 20,
        };
        let out = expand(r, [0; 4], 0);
        assert_eq!((out.x, out.y, out.w, out.h), (-50, 30, 40, 20));
    }

    #[test]
    fn mode_codes_roundtrip() {
        for m in Mode::ALL {
            assert_eq!(Mode::from_code(m.code()), Some(m));
        }
        assert_eq!(Mode::from_code("SYSTEM"), Some(Mode::System));
        assert_eq!(Mode::from_code("nonsense"), None);
        assert_eq!(Mode::default(), Mode::Off);
    }

    #[test]
    fn synthetic_frame_is_larger_and_has_transparent_corners() {
        let src = RgbaImage::from_pixel(40, 30, image::Rgba([255, 255, 255, 255]));
        let out = decorate::window_shadow_frame(&src, 8, 8, 2, 110);
        assert!(out.width() > 40 && out.height() > 30);
        assert_eq!(out.get_pixel(0, 0).0[3], 0);
        // The centre of the source is still opaque.
        assert_eq!(out.get_pixel(out.width() / 2, out.height() / 2).0[3], 255);
    }
}
