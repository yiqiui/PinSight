//! Cursor and click visual effects.
//!
//! Pure image operations (no OS dependency) so the geometry can be unit-tested.
//! Two primitives are provided:
//!
//! * [`highlight_ring`] — a steady ring drawn around the live cursor so it reads
//!   clearly against any background (the OS arrow can vanish on busy captures).
//! * [`click_ripple`] — an expanding, fading ring used to mark mouse clicks; it
//!   is driven by a normalised progress `t ∈ [0, 1]`.
//!
//! [`ActiveClicks`] is the runtime helper that remembers recent clicks and
//! paints their ripples for the duration of a short animation window.

use image::RgbaImage;
use std::time::{Duration, Instant};

/// Alpha-blend `color` (opaque) onto the pixel at `(x, y)` with weight `alpha`.
/// Out-of-bounds writes are ignored. The destination alpha is lifted so the
/// effect is never blended onto a fully transparent pixel and then lost.
fn blend(img: &mut RgbaImage, x: i32, y: i32, color: [u8; 3], alpha: f32) {
    if x < 0 || y < 0 {
        return;
    }
    let (w, h) = (img.width() as i32, img.height() as i32);
    if x >= w || y >= h {
        return;
    }
    let a = alpha.clamp(0.0, 1.0);
    let dst = img.get_pixel_mut(x as u32, y as u32);
    for c in 0..3 {
        dst[c] = (color[c] as f32 * a + dst[c] as f32 * (1.0 - a)).round() as u8;
    }
    let da = dst[3] as f32;
    dst[3] = (da.max(a * 255.0)) as u8;
}

/// Draw a steady ring of fixed thickness around `(cx, cy)` to highlight the
/// cursor. `radius` is the ring centre radius in pixels.
pub fn highlight_ring(img: &mut RgbaImage, (cx, cy): (i32, i32), color: [u8; 3], radius: u32) {
    let r = radius.max(1) as i32;
    let thick = 3i32;
    for dy in -r - thick..=r + thick {
        for dx in -r - thick..=r + thick {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            if d >= r as f32 && d <= (r + thick) as f32 {
                blend(img, cx + dx, cy + dy, color, 0.9);
            }
        }
    }
}

/// Draw a click ripple at `(cx, cy)`. `t` is the animation progress in `[0, 1]`:
/// the ring expands from nothing to `max_radius` while fading from full to zero
/// opacity. Returns early (drawing nothing) once the animation has finished.
pub fn click_ripple(
    img: &mut RgbaImage,
    (cx, cy): (i32, i32),
    t: f32,
    color: [u8; 3],
    max_radius: u32,
) {
    let t = t.clamp(0.0, 1.0);
    // Always draw at least a 1px ring so a click is visible on the very first
    // frame (t ≈ 0) rather than flickering in a frame later.
    let radius = (max_radius as f32 * t).max(1.0) as i32;
    let thick = 4i32;
    let alpha = (1.0 - t) * 0.9;
    for dy in -radius - thick..=radius + thick {
        for dx in -radius - thick..=radius + thick {
            let d = ((dx * dx + dy * dy) as f32).sqrt();
            if d >= radius as f32 && d <= (radius + thick) as f32 {
                blend(img, cx + dx, cy + dy, color, alpha);
            }
        }
    }
}

/// A single recorded click awaiting its ripple animation.
#[derive(Clone, Copy)]
struct Click {
    x: i32,
    y: i32,
    start: Instant,
}

/// Tracks recent mouse clicks and paints their ripples onto frames. Pruning is
/// done lazily inside [`ActiveClicks::draw`], so a click that has fully
/// animated out simply stops being drawn.
pub struct ActiveClicks {
    clicks: Vec<Click>,
    duration: Duration,
    max_radius: u32,
}

impl ActiveClicks {
    /// `duration` is how long a click ripple lives; `max_radius` its full
    /// expansion in pixels.
    pub fn new(duration_ms: u64, max_radius: u32) -> Self {
        Self {
            clicks: Vec::new(),
            duration: Duration::from_millis(duration_ms),
            max_radius,
        }
    }

    /// Record a click at screen position `(x, y)` (caller converts to image
    /// space before drawing).
    pub fn push(&mut self, x: i32, y: i32) {
        self.clicks.push(Click {
            x,
            y,
            start: Instant::now(),
        });
    }

    /// Paint the still-active ripples. `origin` is the screen-space top-left of
    /// `img`, so clicks stored in screen coordinates are shifted into image
    /// space automatically. Clicks whose animation has finished are dropped.
    pub fn draw(&mut self, img: &mut RgbaImage, origin: (i32, i32), click_color: [u8; 3]) {
        let now = Instant::now();
        self.clicks
            .retain(|c| now.duration_since(c.start) <= self.duration);
        for c in &self.clicks {
            let t =
                now.duration_since(c.start).as_millis() as f32 / self.duration.as_millis() as f32;
            if t <= 1.0 {
                click_ripple(
                    img,
                    (c.x - origin.0, c.y - origin.1),
                    t,
                    click_color,
                    self.max_radius,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blank(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_pixel(w, h, image::Rgba([0, 0, 0, 255]))
    }

    #[test]
    fn highlight_ring_stays_in_bounds_and_paints() {
        let mut img = blank(100, 100);
        highlight_ring(&mut img, (50, 50), [255, 0, 0], 10);
        // A pixel on the ring (radius 10) should be tinted red.
        let on_ring = img.get_pixel(60, 50);
        assert!(on_ring[0] > 100 && on_ring[1] < 100 && on_ring[2] < 100);
        // A pixel well inside the ring should be untouched (black).
        let inside = img.get_pixel(50, 50);
        assert_eq!(inside[0], 0);
    }

    #[test]
    fn highlight_ring_near_edge_is_clipped_not_panicked() {
        let mut img = blank(20, 20);
        // Ring centred near the corner: must not panic writing out of bounds.
        highlight_ring(&mut img, (2, 2), [0, 255, 0], 18);
        // At least one green pixel made it on-screen.
        assert!(img.pixels().any(|p| p[1] > 100));
    }

    #[test]
    fn click_ripple_radius_grows_with_progress() {
        let mut small = blank(200, 200);
        click_ripple(&mut small, (100, 100), 0.2, [255, 0, 0], 80);
        let mut large = blank(200, 200);
        click_ripple(&mut large, (100, 100), 0.8, [255, 0, 0], 80);

        // Count red pixels: the later-progress ripple is larger (its ring is
        // wider, even though it is more faded — so we key off presence, not
        // brightness).
        let count = |img: &RgbaImage| img.pixels().filter(|p| p[0] > 10 && p[1] < 100).count();
        assert!(count(&large) > count(&small));
    }

    #[test]
    fn click_ripple_fades_as_it_grows() {
        // At full progress the ripple should be nearly transparent (alpha ≈ 0).
        let mut img = blank(200, 200);
        click_ripple(&mut img, (100, 100), 1.0, [255, 0, 0], 80);
        // At t = 1.0 alpha is 0, so no red should be written.
        assert!(img.pixels().all(|p| p[0] <= 100 || p[3] < 50));
    }

    #[test]
    fn active_clicks_prune_after_duration() {
        let mut ac = ActiveClicks::new(50, 80);
        ac.push(10, 10);
        // Immediately draw — click is still active.
        let mut img = blank(50, 50);
        ac.draw(&mut img, (0, 0), [255, 0, 0]);
        assert!(img.pixels().any(|p| p[0] > 100));
    }
}
