//! Region recording to GIF, with an optional H.264/MP4 path when `ffmpeg` is
//! available on `PATH`.
//!
//! Recording runs on a worker thread and writes frames incrementally so memory
//! usage stays bounded even for long recordings.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::{Duration, Instant};

use crate::annotate::ImgRect;
use crate::capture;
use crate::util;

pub enum RecordUpdate {
    Progress { frames: u32 },
    Finished { path: PathBuf, frames: u32 },
    Failed(String),
}

/// Set by the low-level keyboard hook while a recording runs. A watcher thread
/// bridges it onto the active job's `stop` flag, so Esc ends the recording even
/// when the status pill has lost focus to the window being recorded — the
/// status pill is a separate window and only sees keys while it is focused.
#[cfg(windows)]
static STOP_HIT: AtomicBool = AtomicBool::new(false);

/// Watch for Esc while a recording runs. Same mechanism as the long-screenshot
/// watcher: a `WH_KEYBOARD_LL` hook is the only thing that sees the key while
/// another window is in the foreground.
#[cfg(windows)]
fn spawn_stop_watcher(stop: Arc<AtomicBool>, done: Arc<AtomicBool>) {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, PeekMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WM_KEYDOWN, WM_SYSKEYDOWN,
    };

    unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if code == 0 {
                let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                let msg = wparam.0 as u32;
                if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && kb.vkCode == VK_ESCAPE.0 as u32 {
                    STOP_HIT.store(true, Ordering::Relaxed);
                }
            }
            CallNextHookEx(None, code, wparam, lparam)
        }
    }

    std::thread::Builder::new()
        .name("pinsight-record-esc".into())
        .spawn(move || {
            unsafe {
                let hook: HHOOK = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0)
                    .unwrap_or_default();
                let mut msg = MSG::default();
                loop {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {}
                    // Esc pressed, or the job already ended another way (the
                    // pill's stop button, the time limit, MCP): either way the
                    // hook has no reason to stay installed.
                    if STOP_HIT.swap(false, Ordering::Relaxed) || stop.load(Ordering::Relaxed) {
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                if !hook.0.is_null() {
                    let _ = UnhookWindowsHookEx(hook);
                }
            }
        })
        .ok();
}

pub struct RecordJob {
    stop: Arc<AtomicBool>,
    pause: Arc<AtomicBool>,
    /// Set once the job is over, so the Esc watcher can unhook and exit.
    done: Arc<AtomicBool>,
    pub rx: Receiver<RecordUpdate>,
    pub path: PathBuf,
    pub frames: u32,
    pub started: Instant,
    /// Wall-clock seconds the recording may run; 0 means no limit.
    pub max_seconds: u32,
    /// Absolute physical rectangle being recorded — where the status pill and
    /// the annotation layer are placed.
    pub region: crate::util::PhysRect,
    /// Physical rectangle of the monitor that owns the region.
    pub monitor: crate::util::PhysRect,
    /// Scale factor of that monitor (points → physical pixels).
    pub scale: f32,
}

impl RecordJob {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Freeze the capture without ending the recording. The encoder keeps the
    /// frames already written, so the result simply has no new content while
    /// paused.
    pub fn pause(&self) {
        self.pause.store(true, Ordering::Relaxed);
    }

    pub fn resume(&self) {
        self.pause.store(false, Ordering::Relaxed);
    }

    pub fn paused(&self) -> bool {
        self.pause.load(Ordering::Relaxed)
    }

    pub fn poll(&mut self) -> Option<RecordUpdate> {
        while let Ok(update) = self.rx.try_recv() {
            match &update {
                RecordUpdate::Progress { frames, .. } => self.frames = *frames,
                _ => {
                    self.done.store(true, Ordering::Relaxed);
                    return Some(update);
                }
            }
        }
        None
    }
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    monitor_id: u32,
    region: ImgRect,
    fps: u32,
    dir: &std::path::Path,
    options: &util::SaveOptions,
    max_seconds: u32,
    record_audio: bool,
    audio_device: String,
) -> RecordJob {
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let stop_thread = stop.clone();
    let fps = fps.clamp(4, 30);
    let pause_thread = pause.clone();
    // Esc ends the recording from anywhere, even while the recorded window has
    // focus and the status pill is not the active window.
    #[cfg(windows)]
    STOP_HIT.store(false, Ordering::Relaxed);
    #[cfg(windows)]
    spawn_stop_watcher(stop.clone(), done.clone());
    let stem = crate::naming::sanitize(&crate::naming::expand(&options.template, &options.title));
    let path = dir.join(format!(
        "{}_recording.{}",
        stem,
        if ffmpeg_available() { "mp4" } else { "gif" }
    ));
    let path_for_thread = path.clone();

    // Placement metadata for the floating status pill and the annotation
    // layer. Best effort: if the monitor cannot be resolved the region still
    // records, the HUD just falls back to the primary display.
    let (monitor_rect, scale) = capture::find_monitor(monitor_id)
        .map(|m| (m.rect, m.scale))
        .unwrap_or((
            crate::util::PhysRect {
                x: region.x.round() as i32,
                y: region.y.round() as i32,
                w: region.w.round() as u32,
                h: region.h.round() as u32,
            },
            1.0,
        ));
    let abs_region = crate::util::PhysRect {
        x: monitor_rect.x + region.x.round() as i32,
        y: monitor_rect.y + region.y.round() as i32,
        w: region.w.round().max(2.0) as u32,
        h: region.h.round().max(2.0) as u32,
    };

    std::thread::Builder::new()
        .name("pinsight-recorder".into())
        .spawn(move || {
            if let Err(err) = run(
                monitor_id,
                region,
                fps,
                &path_for_thread,
                &tx,
                &stop_thread,
                &pause_thread,
                max_seconds,
                record_audio,
                &audio_device,
            ) {
                let _ = tx.send(RecordUpdate::Failed(err));
            }
        })
        .expect("spawn recorder thread");

    RecordJob {
        stop,
        pause,
        done,
        rx,
        path,
        max_seconds,
        frames: 0,
        started: Instant::now(),
        region: abs_region,
        monitor: monitor_rect,
        scale,
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    monitor_id: u32,
    region: ImgRect,
    fps: u32,
    path: &std::path::Path,
    tx: &Sender<RecordUpdate>,
    stop: &AtomicBool,
    pause: &AtomicBool,
    max_seconds: u32,
    record_audio: bool,
    audio_device: &str,
) -> Result<(), String> {
    let _monitor = capture::find_monitor(monitor_id).map_err(|e| e.to_string())?;
    let rx = region.x.round().max(0.0) as u32;
    let ry = region.y.round().max(0.0) as u32;
    let rw = (region.w.round().max(2.0) as u32) & !1;
    let rh = (region.h.round().max(2.0) as u32) & !1;
    if rw < 2 || rh < 2 {
        return Err("recording region is too small".into());
    }

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    // With the `wgc` feature enabled, `capture_region` honours
    // `WDA_EXCLUDEFROMCAPTURE`. The UI thread re-asserts it every frame; set it
    // once here too so the first frame — which can race the first UI frame — is
    // already free of the status pill (no black rectangle, no flicker).
    #[cfg(windows)]
    let _ = crate::capture::set_capture_exclusion(crate::hud::RECORD_WINDOW_TITLE);

    let mut sink = Sink::create(path, rw, rh, fps, record_audio, audio_device)?;
    let frame_time = Duration::from_secs_f64(1.0 / fps as f64);
    let start = Instant::now();
    // The cap comes from the settings page (0 = no limit) and is clamped so a
    // typo cannot produce a 20-day file.
    let max_duration = if max_seconds == 0 {
        Duration::from_secs(3600)
    } else {
        Duration::from_secs(max_seconds.clamp(5, 3600) as u64)
    };
    let mut frames = 0u32;
    let mut last_progress = Instant::now();

    // Enumerate monitors once. `Monitor::all()` is expensive; calling it every
    // frame (15–30×/s) starved the compositor and made the live cursor stutter
    // and jump during recording. A monitor cannot change mid-recording, so
    // resolve it before the loop.
    let mon = xcap::Monitor::all()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|m| m.id().map(|id| id == monitor_id).unwrap_or(false))
        .ok_or_else(|| "monitor disappeared".to_string())?;

    loop {
        if stop.load(Ordering::Relaxed) || start.elapsed() > max_duration {
            break;
        }
        // Paused: idle in small steps so Stop still answers immediately.
        while pause.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(60));
        }
        let tick = Instant::now();
        let image = match mon.capture_region(rx, ry, rw, rh) {
            Ok(image) => image,
            Err(err) => {
                if frames == 0 {
                    return Err(format!("capture failed: {err}"));
                }
                break;
            }
        };
        sink.write(&image, fps)?;
        frames += 1;

        if last_progress.elapsed() > Duration::from_millis(400) {
            last_progress = Instant::now();
            let _ = tx.send(RecordUpdate::Progress { frames });
        }

        if let Some(remaining) = frame_time.checked_sub(tick.elapsed()) {
            std::thread::sleep(remaining);
        }
    }

    sink.finish()?;
    if frames == 0 {
        return Err("no frames were captured".into());
    }
    let _ = tx.send(RecordUpdate::Finished {
        path: path.to_path_buf(),
        frames,
    });
    Ok(())
}

enum Sink {
    Gif {
        encoder: gif::Encoder<std::fs::File>,
        rgba: Vec<u8>,
    },
    Mp4 {
        child: Child,
        stdin: Option<ChildStdin>,
        rgba: Vec<u8>,
    },
}

impl Sink {
    fn create(
        path: &std::path::Path,
        w: u32,
        h: u32,
        fps: u32,
        record_audio: bool,
        audio_device: &str,
    ) -> Result<Self, String> {
        if path.extension().and_then(|e| e.to_str()) == Some("mp4") {
            // Video input comes from the raw RGBA pipe. Audio is an optional
            // second input discovered from a configured device (or the platform
            // default on Windows).
            let mut args: Vec<String> = vec![
                "-hide_banner".into(),
                "-loglevel".into(),
                "error".into(),
                "-y".into(),
                "-f".into(),
                "rawvideo".into(),
                "-pixel_format".into(),
                "rgba".into(),
                "-video_size".into(),
                format!("{w}x{h}"),
                "-framerate".into(),
                fps.to_string(),
                "-i".into(),
                "-".into(),
            ];
            let mut has_audio = false;
            if record_audio {
                if let Some(device) = resolve_audio_device(audio_device) {
                    args.push("-f".into());
                    args.push(audio_format_flag().into());
                    args.push("-i".into());
                    args.push(audio_device_spec(&device));
                    has_audio = true;
                } else {
                    tracing::warn!(
                        "record_audio is on but no audio device could be resolved; \
                         recording video only (set config.audio_device or install ffmpeg)"
                    );
                }
            }
            if has_audio {
                args.push("-map".into());
                args.push("0:v:0".into());
                args.push("-map".into());
                args.push("1:a:0".into());
                args.push("-c:a".into());
                args.push("aac".into());
                args.push("-c:v".into());
                args.push("libx264".into());
                args.push("-preset".into());
                args.push("veryfast".into());
                args.push("-pix_fmt".into());
                args.push("yuv420p".into());
                args.push("-movflags".into());
                args.push("+faststart".into());
            } else {
                args.push("-an".into());
                args.push("-c:v".into());
                args.push("libx264".into());
                args.push("-preset".into());
                args.push("veryfast".into());
                args.push("-pix_fmt".into());
                args.push("yuv420p".into());
                args.push("-movflags".into());
                args.push("+faststart".into());
            }
            args.push(path.to_string_lossy().into_owned());

            let mut child = Command::new("ffmpeg")
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("failed to start ffmpeg: {e}"))?;
            let stdin = child
                .stdin
                .take()
                .ok_or_else(|| "ffmpeg stdin unavailable".to_string())?;
            return Ok(Sink::Mp4 {
                child,
                stdin: Some(stdin),
                rgba: Vec::new(),
            });
        }

        // GIF fallback.
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut encoder =
            gif::Encoder::new(file, w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        Ok(Sink::Gif {
            encoder,
            rgba: Vec::new(),
        })
    }

    fn write(&mut self, image: &image::RgbaImage, fps: u32) -> Result<(), String> {
        match self {
            Sink::Gif { encoder, rgba } => {
                rgba.clear();
                rgba.extend_from_slice(image.as_raw());
                let mut frame = gif::Frame::from_rgba_speed(
                    image.width() as u16,
                    image.height() as u16,
                    rgba,
                    10,
                );
                // GIF delays are expressed in hundredths of a second.
                frame.delay = (100 / fps.max(1)).max(1) as u16;
                encoder.write_frame(&frame).map_err(|e| e.to_string())
            }
            Sink::Mp4 { stdin, rgba, .. } => {
                let Some(stdin) = stdin.as_mut() else {
                    return Err("ffmpeg stdin closed".into());
                };
                rgba.clear();
                rgba.extend_from_slice(image.as_raw());
                stdin.write_all(rgba).map_err(|e| e.to_string())
            }
        }
    }

    fn finish(mut self) -> Result<(), String> {
        match &mut self {
            Sink::Gif { encoder, .. } => {
                let _ = encoder;
                Ok(())
            }
            Sink::Mp4 { child, stdin, .. } => {
                drop(stdin.take());
                let status = child.wait().map_err(|e| e.to_string())?;
                if status.success() {
                    Ok(())
                } else {
                    Err(format!("ffmpeg exited with {status}"))
                }
            }
        }
    }
}

pub(crate) fn ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Resolve the audio input device to pass to ffmpeg.
///
/// A configured (non-empty) name wins; otherwise the platform default is used.
/// On Windows we discover the first DirectShow capture device by asking ffmpeg
/// to list devices. Other platforms have no reliable auto-detection, so an
/// empty configuration there yields `None` and the caller records video only.
pub(crate) fn resolve_audio_device(configured: &str) -> Option<String> {
    if !configured.is_empty() {
        return Some(configured.to_string());
    }
    match std::env::consts::OS {
        "windows" => windows_default_dshow_audio_device(),
        _ => None,
    }
}

/// The ffmpeg `-f` input format for audio on the current platform.
pub(crate) fn audio_format_flag() -> &'static str {
    match std::env::consts::OS {
        "windows" => "dshow",
        "macos" => "avfoundation",
        "linux" => "pulse",
        _ => "dshow",
    }
}

/// The device spec string for the `-i` flag (after `-f`), matching the format.
pub(crate) fn audio_device_spec(device: &str) -> String {
    match std::env::consts::OS {
        "windows" => format!("audio=\"{device}\""),
        "macos" => format!(":{device}"),
        "linux" => device.to_string(),
        _ => format!("audio=\"{device}\""),
    }
}

/// Ask ffmpeg to enumerate DirectShow devices and return the first audio one.
fn windows_default_dshow_audio_device() -> Option<String> {
    let out = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "info",
            "-list_devices",
            "true",
            "-f",
            "dshow",
            "-i",
            "dummy",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    let mut in_audio = false;
    for line in text.lines() {
        if line.contains("DirectShow audio devices") {
            in_audio = true;
            continue;
        }
        if in_audio {
            if let Some(start) = line.find('"') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.find('"') {
                    return Some(rest[..end].to_string());
                }
            }
        }
    }
    None
}
