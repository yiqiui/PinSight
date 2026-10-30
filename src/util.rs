//! Small shared helpers.

use std::path::{Path, PathBuf};

use image::{RgbImage, RgbaImage};
use serde::{Deserialize, Serialize};

/// Timestamp used in generated file names.
#[allow(dead_code)] // kept for ad-hoc tooling; the naming engine has its own
pub fn timestamp() -> String {
    chrono::Local::now().format("%Y-%m-%d_%H-%M-%S").to_string()
}

/// Default file stem for a capture, e.g. `PinSight_2026-10-22_16-07-04`.
#[allow(dead_code)]
pub fn capture_stem() -> String {
    format!("PinSight_{}", timestamp())
}

/// What a saved capture should be called and how it should be encoded.
#[derive(Debug, Clone)]
pub struct SaveOptions {
    /// The file-name template, see `crate::naming`.
    pub template: String,
    /// Encoder quality, 1–100. Ignored by PNG.
    pub quality: u8,
    /// Image format of the written file.
    pub format: ImageFormat,
    /// Title of the active window, available to the template as `%title%`.
    pub title: String,
    /// Border / shadow / watermark applied before encoding.
    pub decorations: crate::decorate::Decorations,
}

/// The formats a capture can be written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    #[default]
    Png,
    Jpeg,
    Webp,
    /// Uncompressed 24/32-bit; huge files, universal compatibility.
    Bmp,
    /// Tagged image file, lossless; common in print and document pipelines.
    Tiff,
    /// Animated GIF still stores a single frame fine; useful for dropping a
    /// capture into a chat that only accepts GIF.
    Gif,
}

/// File extensions offered for a save, in the order the naming dialog lists
/// them.
#[allow(dead_code)]
pub const FORMATS: &[(ImageFormat, &str)] = &[
    (ImageFormat::Png, "png"),
    (ImageFormat::Jpeg, "jpg"),
    (ImageFormat::Jpeg, "jpeg"),
    (ImageFormat::Webp, "webp"),
    (ImageFormat::Bmp, "bmp"),
    (ImageFormat::Tiff, "tiff"),
    (ImageFormat::Gif, "gif"),
];

impl ImageFormat {
    /// The extension used for the written file, without a dot.
    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::Webp => "webp",
            ImageFormat::Bmp => "bmp",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Gif => "gif",
        }
    }
}

impl Default for SaveOptions {
    fn default() -> Self {
        Self {
            template: crate::naming::DEFAULT_TEMPLATE.to_string(),
            quality: 90,
            format: ImageFormat::Png,
            title: String::new(),
            decorations: crate::decorate::Decorations::default(),
        }
    }
}

/// Write a capture to `dir`, naming the file from the template.
///
/// The stem is expanded and sanitised, so a window title with a colon in it
/// still yields a writable path. When the target exists, a counter is added
/// rather than overwriting the earlier shot.
pub fn save_image(image: &RgbaImage, dir: &Path, options: &SaveOptions) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    // Apply border / shadow / watermark before any format-specific encoding.
    // Skip the clone entirely when nothing is requested, so the common path is
    // byte-for-byte what it was before decorations existed.
    let decorated = (!options.decorations.is_empty())
        .then(|| crate::decorate::decorate_image(image, &options.decorations));
    let image = decorated.as_ref().unwrap_or(image);
    let stem = crate::naming::sanitize(&crate::naming::expand(&options.template, &options.title));
    let path = unique_path(dir, &stem, options.format.extension());
    match options.format {
        ImageFormat::Png => image.save(&path)?,
        ImageFormat::Jpeg => {
            // JPEG has no alpha channel. Transparent pixels are composited
            // over white so they do not turn black.
            let mut rgb = RgbImage::new(image.width(), image.height());
            for (out, src) in rgb.pixels_mut().zip(image.pixels()) {
                let alpha = src.0[3] as u32;
                let blend =
                    |c: u8| -> u8 { ((c as u32 * alpha + 255 * (255 - alpha)) / 255) as u8 };
                out.0 = [blend(src.0[0]), blend(src.0[1]), blend(src.0[2])];
            }
            let mut jpeg = image::codecs::jpeg::JpegEncoder::new_with_quality(
                std::fs::File::create(&path)?,
                options.quality.clamp(1, 100),
            );
            jpeg.encode(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )?;
        }
        ImageFormat::Webp => {
            image::DynamicImage::ImageRgba8(image.clone())
                .save_with_format(&path, image::ImageFormat::WebP)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
        // The remaining encoders all go through the image crate's format
        // registry, which handles the pixel conversion itself.
        ImageFormat::Bmp | ImageFormat::Tiff | ImageFormat::Gif => {
            let codec = match options.format {
                ImageFormat::Bmp => image::ImageFormat::Bmp,
                ImageFormat::Tiff => image::ImageFormat::Tiff,
                _ => image::ImageFormat::Gif,
            };
            image::DynamicImage::ImageRgba8(image.clone())
                .save_with_format(&path, codec)
                .map_err(|e| anyhow::anyhow!("{e}"))?;
        }
    }
    Ok(path)
}

/// Find a free path, appending `_1`, `_2`, … when the name is taken.
fn unique_path(dir: &Path, stem: &str, extension: &str) -> PathBuf {
    let mut candidate = dir.join(format!("{stem}.{extension}"));
    let mut counter = 1;
    while candidate.exists() {
        candidate = dir.join(format!("{stem}_{counter}.{extension}"));
        counter += 1;
    }
    candidate
}

/// Write a PNG to the given directory and return the full path.
#[allow(dead_code)]
pub fn save_png(image: &RgbaImage, dir: &Path, stem: &str) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{stem}.png"));
    image.save(&path)?;
    Ok(path)
}

/// Hand the idle working set back to the OS.
///
/// A tray tool sits idle for hours; keeping ~90 MB of GL driver pages and
/// mapped font data resident the whole time is pure waste. Trimming is cheap
/// and the pages fault back in on the next interaction (a few ms), which is a
/// good trade for an app that is idle 99 % of the time.
#[cfg(windows)]
pub fn trim_working_set() {
    use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
    unsafe {
        // usize::MAX for both bounds tells the OS to trim as much as it can.
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
}

#[cfg(not(windows))]
pub fn trim_working_set() {}

/// Short notification sound when a capture is copied or saved.
///
/// Plays a user-selected sound file when one is configured and exists;
/// otherwise falls back to the default system beep. A no-op off Windows.
pub fn play_notification(sound_file: Option<&Path>) {
    #[cfg(windows)]
    {
        if let Some(path) = sound_file {
            if path.exists() {
                use std::os::windows::ffi::OsStrExt;
                use windows::Win32::Media::Audio::{PlaySoundW, SND_ASYNC, SND_FILENAME};
                let wide: Vec<u16> = path
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
                unsafe {
                    let _ = PlaySoundW(
                        windows::core::PCWSTR(wide.as_ptr()),
                        None,
                        SND_FILENAME | SND_ASYNC,
                    );
                }
                return;
            }
        }
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MB_ICONASTERISK;
        unsafe {
            let _ = MessageBeep(MB_ICONASTERISK);
        }
    }
}

/// Save to an explicit path (used by "Save as…" in the pin context menu).
pub fn save_png_to(image: &RgbaImage, path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    image.save(path)?;
    Ok(())
}

/// Convert an RGBA image into the byte layout `arboard` expects.
pub fn to_arboard(image: &RgbaImage) -> arboard::ImageData<'static> {
    arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: std::borrow::Cow::Owned(image.as_raw().clone()),
    }
}

pub fn open_path(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("explorer").arg(path).spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(path).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(path).spawn();
    }
}

/// A rough "is this a virtual/remote monitor" check is unnecessary; we simply
/// pick the monitor whose rectangle contains the point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysRect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl PhysRect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w as i32 && y < self.y + self.h as i32
    }
}

/// `mm:ss` for a duration given in seconds. Shared by the recording pill and
/// the settings sliders so both render a limit the same way.
pub fn fmt_seconds(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_every_supported_format() {
        let dir = std::env::temp_dir().join(format!("pinsight-save-{}", std::process::id()));
        let image = RgbaImage::from_pixel(8, 8, image::Rgba([200, 30, 40, 255]));
        for format in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::Webp] {
            let options = SaveOptions {
                template: "T_$yyyy-MM-dd$".into(),
                quality: 90,
                format,
                title: String::new(),
                decorations: crate::decorate::Decorations::default(),
            };
            let path = save_image(&image, &dir, &options).expect("save");
            assert!(path.exists(), "missing file for {format:?}");
            assert_eq!(
                path.extension().and_then(|e| e.to_str()),
                Some(format.extension()),
                "wrong extension for {format:?}"
            );
            assert!(
                !path.to_string_lossy().contains('$'),
                "template left unexpanded"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
