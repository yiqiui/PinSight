//! Custom background image for the main window.
//!
//! The image is loaded from disk, downscaled and (optionally) blurred once,
//! then kept as a single RGBA buffer that the app uploads as a texture. That
//! keeps the steady-state memory cost of a wallpaper to one buffer instead of
//! re-decoding on every frame.

use std::path::{Path, PathBuf};

use eframe::egui;
use image::{RgbaImage, imageops};

/// Longest edge we keep. 1920 px covers any single monitor at 100 % scale and
/// a 1920×1080 RGBA buffer is 8 MB — small next to the winit/glow baseline.
const MAX_EDGE: u32 = 1920;

/// A processed background image ready to be uploaded as a texture.
pub struct Background {
    /// Absolute path it came from, so we can tell whether the config changed.
    pub source: PathBuf,
    /// Blur radius baked in when it was loaded.
    pub blur: f32,
    pub width: u32,
    pub height: u32,
    pixels: RgbaImage,
}

impl Background {
    /// Load, downscale and blur an image. Returns `None` when the file cannot
    /// be decoded, so a bad path never takes the window down with it.
    pub fn load(path: &Path, blur: f32) -> Option<Self> {
        let image = image::open(path).ok()?.to_rgba8();
        let image = downscale(image, MAX_EDGE);
        let image = if blur > 0.5 {
            imageops::blur(&image, blur)
        } else {
            image
        };
        Some(Self {
            source: path.to_path_buf(),
            blur,
            width: image.width(),
            height: image.height(),
            pixels: image,
        })
    }

    /// Reload when the path or the blur radius actually changed. Returns
    /// `true` when a new image is now loaded.
    pub fn refresh(&mut self, path: Option<&Path>, blur: f32) -> bool {
        let target = path.map(Path::to_path_buf);
        let same_source = target.as_deref() == Some(self.source.as_path());
        if same_source && (blur - self.blur).abs() <= 0.01 {
            return false;
        }
        let Some(path) = target else { return false };
        match Background::load(&path, blur) {
            Some(next) => {
                *self = next;
                true
            }
            None => false,
        }
    }

    pub fn color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgba_unmultiplied(
            [self.width as usize, self.height as usize],
            self.pixels.as_raw(),
        )
    }

    pub fn size(&self) -> egui::Vec2 {
        egui::vec2(self.width as f32, self.height as f32)
    }
}

/// Shrink so the longest edge is at most `max_edge`, preserving the aspect
/// ratio. Images already small enough are returned untouched.
fn downscale(image: RgbaImage, max_edge: u32) -> RgbaImage {
    let (w, h) = (image.width(), image.height());
    let longest = w.max(h);
    if longest <= max_edge {
        return image;
    }
    let scale = max_edge as f32 / longest as f32;
    let nw = ((w as f32 * scale).round() as u32).max(1);
    let nh = ((h as f32 * scale).round() as u32).max(1);
    imageops::resize(&image, nw, nh, imageops::FilterType::Triangle)
}

/// Where the image lands inside `area` for a given fit mode.
pub fn dest_rect(area: egui::Rect, image: egui::Vec2, fit: crate::config::BgFit) -> egui::Rect {
    use crate::config::BgFit;
    if image.x <= 0.0 || image.y <= 0.0 {
        return area;
    }
    let (aw, ah) = (area.width(), area.height());
    let scale = match fit {
        BgFit::Cover => (aw / image.x).max(ah / image.y),
        BgFit::Contain => (aw / image.x).min(ah / image.y),
        BgFit::Stretch => return area,
    };
    let size = egui::vec2(image.x * scale, image.y * scale);
    let min = egui::pos2(
        area.center().x - size.x * 0.5,
        area.center().y - size.y * 0.5,
    );
    egui::Rect::from_min_size(min, size)
}

/// Open the platform file picker for an image. Blocking on purpose: it is a
/// modal dialog and the window has nothing else to show meanwhile.
pub fn pick_image() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Choose a background image")
        .add_filter("Image", &["png", "jpg", "jpeg", "bmp", "webp", "gif"])
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cover_crops_to_fill() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let rect = dest_rect(area, egui::vec2(50.0, 100.0), crate::config::BgFit::Cover);
        assert_eq!(rect.width(), 100.0);
        assert_eq!(rect.height(), 200.0);
        assert_eq!(rect.center(), area.center());
    }

    #[test]
    fn contain_letterboxes() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 100.0));
        let rect = dest_rect(area, egui::vec2(50.0, 100.0), crate::config::BgFit::Contain);
        assert_eq!(rect.width(), 50.0);
        assert_eq!(rect.height(), 100.0);
    }

    #[test]
    fn stretch_fills_exactly() {
        let area = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 60.0));
        let rect = dest_rect(area, egui::vec2(10.0, 90.0), crate::config::BgFit::Stretch);
        assert_eq!(rect, area);
    }
}
