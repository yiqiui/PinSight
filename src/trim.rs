//! Trim the head/tail off a finished recording.
//!
//! MP4 files are re-encoded with ffmpeg (`-ss` + `-t`, so the result starts on
//! an exact second rather than the nearest keyframe). GIF recordings — the
//! no-ffmpeg fallback — are trimmed with the pure-Rust GIF editor. Both write a
//! new `*-trim.*` file next to the source; the original is never touched.
//!
//! The work runs on a worker thread (see `app.rs`), never on the GUI thread.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Quality passed to the GIF encoder; matches the CLI's default.
const GIF_SPEED: i32 = 10;

/// A `*-trim.<ext>` path that does not clobber an existing file.
pub fn output_path(input: &Path) -> PathBuf {
    let stem = input
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let ext = input
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mp4".into());
    let parent = input.parent().unwrap_or_else(|| Path::new("."));
    let mut candidate = parent.join(format!("{stem}-trim.{ext}"));
    let mut n = 2u32;
    while candidate.exists() && n < 1000 {
        candidate = parent.join(format!("{stem}-trim-{n}.{ext}"));
        n += 1;
    }
    candidate
}

/// Reject ranges that would produce an empty or out-of-bounds clip.
pub fn validate(start: f64, end: f64, duration: f64) -> Result<(), String> {
    if !start.is_finite() || !end.is_finite() || !duration.is_finite() || duration <= 0.0 {
        return Err("invalid duration".into());
    }
    if start < 0.0 || end > duration + 0.001 {
        return Err(format!("range is outside the 0–{duration:.1}s recording"));
    }
    if end - start < 0.2 {
        return Err("keep at least 0.2 seconds".into());
    }
    Ok(())
}

/// ffmpeg argv for a precise trim. Pure so it can be unit-tested without
/// ffmpeg being installed on the test machine.
fn ffmpeg_args(input: &Path, output: &Path, start: f64, end: f64) -> Vec<String> {
    let arg = |s: &str| s.to_string();
    vec![
        arg("-y"),
        arg("-ss"),
        format!("{start:.3}"),
        arg("-i"),
        input.to_string_lossy().into_owned(),
        arg("-t"),
        format!("{:.3}", end - start),
        arg("-c:v"),
        arg("libx264"),
        arg("-preset"),
        arg("veryfast"),
        arg("-crf"),
        arg("20"),
        arg("-c:a"),
        arg("aac"),
        arg("-movflags"),
        arg("+faststart"),
        output.to_string_lossy().into_owned(),
    ]
}

/// Trim `input` to `[start, end)` seconds and return the new file path.
/// `fps` is only used to translate seconds into GIF frame indices.
pub fn trim(input: &Path, start: f64, end: f64, fps: u32) -> Result<PathBuf, String> {
    if !(start.is_finite() && end.is_finite() && start >= 0.0 && end - start >= 0.2) {
        return Err("invalid trim range".into());
    }
    let output = output_path(input);
    let is_gif = input
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gif"));

    if is_gif {
        let frames = crate::gifedit::load_gif(input).map_err(|e| e.to_string())?;
        let total = frames.len();
        if total == 0 {
            return Err("the recording has no frames".into());
        }
        let rate = fps.clamp(1, 60) as f64;
        let first = ((start * rate).round() as usize).min(total);
        let last = ((end * rate).round() as usize).min(total);
        if first >= last {
            return Err("trim range keeps no frames".into());
        }
        crate::gifedit::trim(input, &output, first, Some(last), GIF_SPEED)
            .map_err(|e| e.to_string())?;
    } else {
        let out = Command::new("ffmpeg")
            .args(ffmpeg_args(input, &output, start, end))
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .output()
            .map_err(|e| format!("could not run ffmpeg: {e}"))?;
        if !out.status.success() {
            // ffmpeg may leave a zero-length file behind on failure.
            let _ = std::fs::remove_file(&output);
            let tail = String::from_utf8_lossy(&out.stderr);
            let detail: Vec<&str> = tail.lines().rev().take(2).collect();
            return Err(format!("ffmpeg: {}", detail.join(" ").trim()));
        }
    }
    if !output.exists() {
        return Err("no output file was produced".into());
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_bad_ranges() {
        assert!(validate(0.0, 5.0, 10.0).is_ok());
        assert!(validate(-1.0, 5.0, 10.0).is_err());
        assert!(validate(0.0, 11.0, 10.0).is_err());
        assert!(validate(5.0, 5.1, 10.0).is_err());
        assert!(validate(f64::NAN, 5.0, 10.0).is_err());
        assert!(validate(0.0, 5.0, 0.0).is_err());
    }

    #[test]
    fn output_path_never_clobbers() {
        let dir = std::env::temp_dir().join(format!(
            "pinsight-trim-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("clip.mp4");
        std::fs::write(&src, b"x").unwrap();
        let first = output_path(&src);
        assert_eq!(first.file_name().unwrap(), "clip-trim.mp4");
        assert_eq!(first.parent().unwrap(), dir);
        std::fs::write(&first, b"x").unwrap();
        let second = output_path(&src);
        assert_eq!(second.file_name().unwrap(), "clip-trim-2.mp4");
        // The original is never returned.
        assert_ne!(second, src);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ffmpeg_args_seek_and_limit() {
        let args = ffmpeg_args(Path::new("in.mp4"), Path::new("out.mp4"), 2.5, 7.75);
        assert_eq!(args[0], "-y");
        assert_eq!(args[1], "-ss");
        assert_eq!(args[2], "2.500");
        assert_eq!(args[3], "-i");
        assert_eq!(args[4], "in.mp4");
        assert_eq!(args[5], "-t");
        assert_eq!(args[6], "5.250");
        assert!(args.contains(&"libx264".to_string()));
        assert_eq!(args.last().unwrap(), "out.mp4");
    }

    #[test]
    fn trim_rejects_invalid_range_without_touching_ffmpeg() {
        let err = trim(Path::new("nonexistent.mp4"), 5.0, 1.0, 12).unwrap_err();
        assert!(err.contains("invalid"));
    }

    #[test]
    fn trims_a_real_gif_by_seconds() {
        // End-to-end exercise of the no-ffmpeg path: ten 100 ms frames at
        // 10 fps, keeping [0.3s, 0.8s) must leave frames 3..8.
        let dir = std::env::temp_dir().join(format!(
            "pinsight-trim-gif-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("clip.gif");
        let frames: Vec<crate::gifedit::GifFrame> = (0..10u8)
            .map(|i| crate::gifedit::GifFrame {
                image: image::RgbaImage::from_pixel(
                    8,
                    8,
                    image::Rgba([i.saturating_mul(20), 0, 0, 255]),
                ),
                delay_cs: 10,
            })
            .collect();
        crate::gifedit::save_gif(&frames, &src, 10).unwrap();
        let out = trim(&src, 0.3, 0.8, 10).unwrap();
        assert_eq!(out.file_name().unwrap(), "clip-trim.gif");
        let kept = crate::gifedit::load_gif(&out).unwrap();
        assert_eq!(kept.len(), 5, "0.3–0.8s at 10 fps keeps 5 frames");
        assert!(kept.iter().all(|f| f.delay_cs == 10));
        // The source is untouched.
        assert_eq!(crate::gifedit::load_gif(&src).unwrap().len(), 10);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
