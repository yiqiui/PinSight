//! Voice-note recorder.
//!
//! Captures the default microphone to a WAV file using `ffmpeg` — the same
//! optional runtime dependency the screen recorder already relies on for audio.
//! In-app playback is deliberately out of scope: the produced `.wav` opens with
//! the operating system's default player, which avoids pulling in an audio
//! decoding/playback crate.

use anyhow::{Context, Result};
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
    if let Some(sec) = seconds {
        cmd.args(["-t", &sec.to_string()]);
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
