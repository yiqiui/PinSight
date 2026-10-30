//! GIF editing: decode a GIF, transform the frames, re-encode.
//!
//! Trimming, cropping and text overlays are pure Rust on top of the `image`
//! decoder and the `gif` encoder PinSight already ships for screen recording.
//! Converting to MP4 shells out to `ffmpeg` — the same optional runtime
//! dependency the recorder uses — and fails with a clear error when it is not
//! installed.
//!
//! Re-encoding always quantises to 256 colours (that is what GIF is), so the
//! output is lossy relative to the source; the `speed` knob trades encode
//! time for quality the same way `gif::Frame::from_rgba_speed` does.

use anyhow::{Context, Result};
use image::AnimationDecoder;
use std::path::Path;

/// One decoded frame: an RGBA buffer plus its delay in centiseconds (the unit
/// GIF delays are expressed in; 10 = 100 ms).
pub struct GifFrame {
    pub image: image::RgbaImage,
    pub delay_cs: u16,
}

/// Decode every frame of a GIF into RGBA buffers.
pub fn load_gif(path: &Path) -> Result<Vec<GifFrame>> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    let decoder = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(file))
        .context("creating GIF decoder")?;
    let frames = decoder
        .into_frames()
        .collect_frames()
        .context("decoding GIF frames")?;
    let mut out = Vec::with_capacity(frames.len());
    for frame in frames {
        let delay_ms = frame.delay().numer_denom_ms().0;
        // Clamp to the GIF-legal range so a broken source cannot produce 0.
        let delay_cs = ((delay_ms + 5) / 10).clamp(1, 600) as u16;
        out.push(GifFrame {
            image: frame.into_buffer(),
            delay_cs,
        });
    }
    Ok(out)
}

/// Encode RGBA frames back into an animated GIF. `speed` (1–30) is the
/// `gif` crate's quality/time trade-off; 10 is the recorder's default.
pub fn save_gif(frames: &[GifFrame], path: &Path, speed: i32) -> Result<()> {
    if frames.is_empty() {
        anyhow::bail!("no frames to write");
    }
    let w = frames[0].image.width();
    let h = frames[0].image.height();
    if w > u16::MAX as u32 || h > u16::MAX as u32 {
        anyhow::bail!("frame size {w}x{h} exceeds the GIF limit of 65535");
    }
    let file =
        std::fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let mut encoder = gif::Encoder::new(file, w as u16, h as u16, &[])
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    encoder
        .set_repeat(gif::Repeat::Infinite)
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    for frame in frames {
        let mut rgba = frame.image.as_raw().clone();
        let mut encoded = gif::Frame::from_rgba_speed(
            w as u16,
            h as u16,
            rgba.as_mut_slice(),
            speed.clamp(1, 30),
        );
        encoded.delay = frame.delay_cs.max(1);
        encoder
            .write_frame(&encoded)
            .map_err(|e| anyhow::anyhow!(e.to_string()))?;
    }
    Ok(())
}

/// Keep only frames in `[start, end)` (end `None` = to the last frame).
pub fn trim(
    input: &Path,
    output: &Path,
    start: usize,
    end: Option<usize>,
    speed: i32,
) -> Result<()> {
    let frames = load_gif(input)?;
    if frames.is_empty() {
        anyhow::bail!("the GIF has no frames");
    }
    let end = end.unwrap_or(frames.len()).min(frames.len());
    if start >= end {
        anyhow::bail!("start frame {start} is past the end ({end} frames kept)");
    }
    save_gif(&frames[start..end], output, speed)
}

/// Crop every frame to the given rectangle (clamped to the frame bounds).
pub fn crop(
    input: &Path,
    output: &Path,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    speed: i32,
) -> Result<()> {
    let frames = load_gif(input)?;
    if frames.is_empty() {
        anyhow::bail!("the GIF has no frames");
    }
    let (fw, fh) = (frames[0].image.width(), frames[0].image.height());
    let w = w.min(fw.saturating_sub(x));
    let h = h.min(fh.saturating_sub(y));
    if w == 0 || h == 0 {
        anyhow::bail!("crop region is outside the {fw}x{fh} frame");
    }
    let mut out = Vec::with_capacity(frames.len());
    for frame in &frames {
        out.push(GifFrame {
            image: image::imageops::crop_imm(&frame.image, x, y, w, h).to_image(),
            delay_cs: frame.delay_cs,
        });
    }
    save_gif(&out, output, speed)
}

/// Where a text overlay lands on the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextPos {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    Center,
}

/// Options for burning text into every frame.
pub struct TextOptions {
    pub text: String,
    pub font_size: u32,
    pub color: [u8; 3],
    pub position: TextPos,
}

/// Burn text onto every frame (CJK-friendly, via the shared rasteriser).
pub fn add_text(
    input: &Path,
    output: &Path,
    opts: &TextOptions,
    speed: i32,
) -> Result<()> {
    let frames = load_gif(input)?;
    if frames.is_empty() {
        anyhow::bail!("the GIF has no frames");
    }
    let (fw, fh) = (frames[0].image.width(), frames[0].image.height());
    let Some(overlay) =
        crate::text_font::render_text_overlay(&opts.text, fw, opts.font_size, opts.color)
    else {
        anyhow::bail!("the text is empty");
    };
    let (ow, oh) = (overlay.width(), overlay.height());
    let margin = 8u32;
    let (x, y) = match opts.position {
        TextPos::TopLeft => (margin, margin),
        TextPos::TopRight => (fw.saturating_sub(ow + margin), margin),
        TextPos::BottomLeft => (margin, fh.saturating_sub(oh + margin)),
        TextPos::BottomRight => (fw.saturating_sub(ow + margin), fh.saturating_sub(oh + margin)),
        TextPos::Center => ((fw.saturating_sub(ow)) / 2, (fh.saturating_sub(oh)) / 2),
    };
    let mut out = Vec::with_capacity(frames.len());
    for frame in &frames {
        let mut image = frame.image.clone();
        image::imageops::overlay(&mut image, &overlay, x as i64, y as i64);
        out.push(GifFrame {
            image,
            delay_cs: frame.delay_cs,
        });
    }
    save_gif(&out, output, speed)
}

/// Convert a GIF to MP4 (H.264) via ffmpeg. Requires ffmpeg on PATH.
pub fn to_mp4(input: &Path, output: &Path) -> Result<()> {
    let status = std::process::Command::new("ffmpeg")
        .args(["-y", "-hide_banner", "-loglevel", "error", "-i"])
        .arg(input)
        .args([
            "-movflags",
            "+faststart",
            "-pix_fmt",
            "yuv420p",
            "-c:v",
            "libx264",
            "-preset",
            "veryfast",
        ])
        .arg(output)
        .status()
        .context("running ffmpeg (is it installed?)")?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("ffmpeg exited with {status}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn make_gif(path: &Path, frames: usize) {
        let mut out = Vec::new();
        for i in 0..frames {
            out.push(GifFrame {
                image: image::RgbaImage::from_pixel(
                    24,
                    24,
                    Rgba([(i * 40) as u8, 0, 0, 255]),
                ),
                delay_cs: 10,
            });
        }
        save_gif(&out, path, 10).unwrap();
    }

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("pinsight-gifedit-{}-{name}", std::process::id()))
    }

    #[test]
    fn round_trip_preserves_frames_and_size() {
        let src = tmp("rt.gif");
        let _ = std::fs::remove_file(&src);
        make_gif(&src, 3);
        let frames = load_gif(&src).unwrap();
        assert_eq!(frames.len(), 3);
        assert_eq!((frames[0].image.width(), frames[0].image.height()), (24, 24));
        assert_eq!(frames[0].delay_cs, 10);
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn trim_keeps_the_requested_range() {
        let src = tmp("trim-in.gif");
        let out = tmp("trim-out.gif");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        make_gif(&src, 5);
        trim(&src, &out, 1, Some(3), 10).unwrap();
        assert_eq!(load_gif(&out).unwrap().len(), 2);
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn crop_shrinks_every_frame() {
        let src = tmp("crop-in.gif");
        let out = tmp("crop-out.gif");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        make_gif(&src, 2);
        crop(&src, &out, 4, 4, 12, 12, 10).unwrap();
        let frames = load_gif(&out).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].image.width(), frames[0].image.height()), (12, 12));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn add_text_keeps_frame_count() {
        let src = tmp("text-in.gif");
        let out = tmp("text-out.gif");
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
        make_gif(&src, 2);
        add_text(
            &src,
            &out,
            &TextOptions {
                text: "你好 Hi".into(),
                font_size: 16,
                color: [255, 255, 255],
                position: TextPos::BottomLeft,
            },
            10,
        )
        .unwrap();
        let frames = load_gif(&out).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].image.width(), frames[0].image.height()), (24, 24));
        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&out);
    }
}
