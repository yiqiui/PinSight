//! Voice-note recorder.
//!
//! Captures the default microphone to a WAV file using `ffmpeg` — the same
//! optional runtime dependency the screen recorder already relies on for audio.
//! In-app playback is deliberately out of scope: the produced `.wav` opens with
//! the operating system's default player, which avoids pulling in an audio
//! decoding/playback crate.

use anyhow::{Context, Result};
use image::{Rgba, RgbaImage};
use std::path::Path;
use std::process::Command;

/// Record the default audio input device to `out`.
///
/// `seconds` caps the recording; pass `None` to record until the process is
/// killed (the CLI always passes a duration). Returns an error when ffmpeg is
/// missing or no input device can be resolved.
pub fn record(out: &Path, seconds: Option<u32>) -> Result<()> {
    if !crate::record::ffmpeg_available() {
        anyhow::bail!("ffmpeg is required to record audio and was not found on PATH");
    }
    let format = crate::record::audio_format_flag();
    let device = crate::record::resolve_audio_device("");
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-y", "-hide_banner", "-loglevel", "error", "-f", format]);
    match std::env::consts::OS {
        "macos" => {
            let dev = device.unwrap_or_else(|| "default".to_string());
            cmd.arg("-i").arg(&dev);
        }
        _ => {
            let dev = device.context("no audio input device was found")?;
            cmd.arg("-i").arg(crate::record::audio_device_spec(&dev));
        }
    }
    // Cap at one day: the CLI takes this from `--seconds`, and a typo
    // ("--seconds 99999999") would otherwise keep ffmpeg recording for years.
    if let Some(sec) = seconds {
        cmd.args(["-t", &(sec.clamp(1, 86_400)).to_string()]);
    }
    // Mono 44.1 kHz WAV: small, universal, and plenty for voice memos.
    cmd.args(["-ac", "1", "-ar", "44100"]).arg(out);
    let status = cmd.status().context("running ffmpeg to record audio")?;
    if status.success() {
        Ok(())
    } else {
        anyhow::bail!("ffmpeg exited with {status} while recording audio")
    }
}

/// Render a pinned "voice note" thumbnail: a dark rounded card with a play
/// button and a waveform drawn from the audio. Returns `None` if the file is
/// not a readable WAV, in which case the caller falls back to a static icon.
pub fn thumbnail(path: &Path) -> Option<RgbaImage> {
    let data = std::fs::read(path).ok()?;
    let samples = parse_pcm(&data)?;
    Some(draw_waveform(&samples))
}

/// A static accent card used when [`thumbnail`] cannot parse the audio — the
/// WAV is missing or corrupt. Keeps the pin usable (double-click still plays).
pub fn fallback_thumbnail() -> RgbaImage {
    draw_card([78, 205, 196])
}

/// Draw the bare rounded card (no waveform) used by [`fallback_thumbnail`].
fn draw_card(accent: [u8; 3]) -> RgbaImage {
    const W: u32 = 340;
    const H: u32 = 120;
    let mut img = RgbaImage::from_pixel(W, H, Rgba([0, 0, 0, 0]));
    let set = |img: &mut RgbaImage, x: i32, y: i32, c: [u8; 4]| {
        if x >= 0 && y >= 0 && (x as u32) < W && (y as u32) < H {
            img.put_pixel(x as u32, y as u32, Rgba(c));
        }
    };
    let (mx, my, mw, mh, r) = (8i32, 8i32, 324i32, 104i32, 14i32);
    for y in my..my + mh {
        for x in mx..mx + mw {
            // Snap to the relevant rounded-corner centre when the pixel sits in
            // a corner band; otherwise it's a plain interior point.
            let (ccx, ccy) = if x < mx + r && y < my + r {
                (mx + r, my + r)
            } else if x > mx + mw - r && y < my + r {
                (mx + mw - r, my + r)
            } else if x < mx + r && y > my + mh - r {
                (mx + r, my + mh - r)
            } else if x > mx + mw - r && y > my + mh - r {
                (mx + mw - r, my + mh - r)
            } else {
                (x, y)
            };
            let in_corner = ccx != x || ccy != y;
            let outside = in_corner && (x - ccx) * (x - ccx) + (y - ccy) * (y - ccy) > r * r;
            if !outside {
                set(&mut img, x, y, [30, 32, 40, 235]);
            }
        }
    }
    let (bx, by, br) = (30i32, 60i32, 16i32);
    for y in by - br..=by + br {
        for x in bx - br..=bx + br {
            if (x - bx) * (x - bx) + (y - by) * (y - by) <= br * br {
                set(&mut img, x, y, [accent[0], accent[1], accent[2], 255]);
            }
        }
    }
    for y in by - 8..=by + 8 {
        let t = (y - (by - 8)) as f32 / 16.0;
        let x0 = bx - 6 + (t * 5.0) as i32;
        let x1 = bx + 7;
        for x in x0..=x1 {
            set(&mut img, x, y, [255, 255, 255, 255]);
        }
    }
    img
}

/// Parse a PCM WAV into normalised mono samples in `[-1, 1]`. Returns `None`
/// for anything that is not a recognizable RIFF/WAVE file.
fn parse_pcm(data: &[u8]) -> Option<Vec<f32>> {
    if data.len() < 12 || &data[0..4] != b"RIFF" || &data[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut channels = 1u16;
    let mut bits = 16u16;
    let mut pcm = false;
    let mut sample_bytes: Option<&[u8]> = None;
    while pos + 8 <= data.len() {
        let id = &data[pos..pos + 4];
        let size = u32::from_le_bytes([data[pos + 4], data[pos + 5], data[pos + 6], data[pos + 7]])
            as usize;
        let start = pos + 8;
        let end = (start + size).min(data.len());
        if id == b"fmt " && end - start >= 16 {
            channels = u16::from_le_bytes([data[start + 2], data[start + 3]]);
            let fmt = u16::from_le_bytes([data[start], data[start + 1]]);
            bits = u16::from_le_bytes([data[start + 14], data[start + 15]]);
            pcm = fmt == 1 || fmt == 0xFFFE;
        } else if id == b"data" {
            sample_bytes = Some(&data[start..end]);
        }
        pos = end + (size & 1); // chunks are word-aligned
    }
    let bytes = sample_bytes?;
    if !pcm || channels == 0 {
        return None;
    }
    let ch = channels as usize;
    let mut out = Vec::with_capacity(bytes.len() / 2);
    match bits {
        8 => {
            for i in (0..bytes.len()).step_by(ch) {
                out.push((bytes[i] as f32 - 128.0) / 128.0);
            }
        }
        16 => {
            for i in (0..bytes.len().saturating_sub(1)).step_by(2 * ch) {
                let s = i16::from_le_bytes([bytes[i], bytes[i + 1]]) as f32 / 32768.0;
                out.push(s);
            }
        }
        24 => {
            for i in (0..bytes.len().saturating_sub(2)).step_by(3 * ch) {
                let s = (bytes[i] as i32 | (bytes[i + 1] as i32) << 8 | (bytes[i + 2] as i32) << 16)
                    as f32
                    / 8_388_608.0;
                out.push(s);
            }
        }
        32 => {
            for i in (0..bytes.len().saturating_sub(3)).step_by(4 * ch) {
                let s = i32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]])
                    as f32
                    / 2_147_483_648.0;
                out.push(s);
            }
        }
        _ => return None,
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Draw the 340×120 voice-note thumbnail: the standard accent card plus a
/// waveform rendered from the audio samples.
fn draw_waveform(samples: &[f32]) -> RgbaImage {
    const W: u32 = 340;
    const H: u32 = 120;
    const ACCENT: [u8; 3] = [78, 205, 196];
    let mut img = draw_card(ACCENT);

    let set = |img: &mut RgbaImage, x: i32, y: i32, c: [u8; 4]| {
        if x >= 0 && y >= 0 && (x as u32) < W && (y as u32) < H {
            img.put_pixel(x as u32, y as u32, Rgba(c));
        }
    };

    // Waveform bars across the remaining width (right of the play button).
    let bars = 48u32;
    let x0 = 56i32;
    let x1 = 326i32;
    let span = (x1 - x0) as f32;
    let mid = 60i32;
    let max_h = 40i32;
    for b in 0..bars {
        let s = (b as f32 * samples.len() as f32 / bars as f32) as usize;
        let e = ((b + 1) as f32 * samples.len() as f32 / bars as f32).ceil() as usize;
        let e = e.min(samples.len());
        let rms = if e > s {
            (s..e).map(|i| samples[i] * samples[i]).sum::<f32>() / (e - s) as f32
        } else {
            0.0
        };
        let h = ((rms.sqrt() * max_h as f32) as i32).clamp(2, max_h);
        let bx_c = x0 + (b as f32 * span / bars as f32) as i32;
        for y in mid - h..=mid + h {
            set(&mut img, bx_c, y, [ACCENT[0], ACCENT[1], ACCENT[2], 255]);
        }
    }

    img
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal mono 16-bit PCM WAV in memory for the parser tests.
    fn make_wav(samples: &[i16]) -> Vec<u8> {
        let data_len = samples.len() * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
        out.extend_from_slice(b"WAVE");
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&44100u32.to_le_bytes());
        out.extend_from_slice(&(44100 * 2u32).to_le_bytes()); // byte rate
        out.extend_from_slice(&2u16.to_le_bytes()); // block align
        out.extend_from_slice(&16u16.to_le_bytes()); // bits
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data_len as u32).to_le_bytes());
        for &s in samples {
            out.extend_from_slice(&s.to_le_bytes());
        }
        out
    }

    #[test]
    fn parses_mono_16bit_wav() {
        let wav = make_wav(&[0, 16384, -16384, 32767, -32768]);
        let samples = parse_pcm(&wav).expect("valid WAV should parse");
        assert_eq!(samples.len(), 5);
        // First sample is silence → 0.0.
        assert!((samples[0]).abs() < 1e-6);
        // A full-scale negative sample maps to ≈ -1.0.
        assert!((samples[4] + 1.0).abs() < 1e-3);
    }

    #[test]
    fn rejects_non_wav() {
        assert!(parse_pcm(b"not a wav at all").is_none());
        assert!(parse_pcm(&[0u8; 4]).is_none());
    }

    #[test]
    fn thumbnail_dimensions_and_fallback() {
        let wav = make_wav(&[0, 8000, -8000, 4000]);
        // `thumbnail` takes a path; feed it the bytes through a temp file.
        let dir = std::env::temp_dir();
        let path = dir.join("pinsight_voice_test.wav");
        std::fs::write(&path, &wav).unwrap();
        let thumb = thumbnail(&path).expect("waveform thumbnail");
        assert_eq!((thumb.width(), thumb.height()), (340, 120));
        let _ = std::fs::remove_file(&path);
        // The fallback must also produce a correctly sized card.
        let fb = fallback_thumbnail();
        assert_eq!((fb.width(), fb.height()), (340, 120));
    }
}
