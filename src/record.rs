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
    /// Emitted once per second while the pre-recording countdown runs. The
    /// HUD swaps its status row for "starting in N s" during this phase.
    Countdown {
        remaining: u32,
    },
    Progress {
        frames: u32,
    },
    Finished {
        path: PathBuf,
        frames: u32,
    },
    Failed(String),
    /// The user stopped during the countdown: no file was written, and the
    /// UI should just put everything back without a toast.
    Cancelled,
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
                let hook: HHOOK =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0).unwrap_or_default();
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
    /// Recorded time EXCLUDING paused spans. The HUD timer reads this, so the
    /// clock stops while paused (as it does in every desktop recorder).
    pub active: Duration,
    /// Wall-clock instant of the last `active` update; not exposed.
    pub last_tick: Instant,
    /// Whether the job was paused at `last_tick`.
    pub was_paused: bool,
    /// Seconds left in the pre-recording countdown; 0 once recording. The HUD
    /// polls this through `poll()` updates rather than reading it directly.
    pub countdown: u32,
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

    /// Recorded time excluding paused spans. Call this from the UI instead of
    /// reading `started.elapsed()` so the timer freezes while paused.
    pub fn active_elapsed(&mut self) -> Duration {
        let now = Instant::now();
        if !self.was_paused {
            self.active += now - self.last_tick;
        }
        self.last_tick = now;
        self.was_paused = self.pause.load(Ordering::Relaxed);
        self.active
    }

    pub fn poll(&mut self) -> Option<RecordUpdate> {
        while let Ok(update) = self.rx.try_recv() {
            match &update {
                RecordUpdate::Progress { frames, .. } => self.frames = *frames,
                RecordUpdate::Countdown { remaining } => self.countdown = *remaining,
                _ => {
                    self.done.store(true, Ordering::Relaxed);
                    return Some(update);
                }
            }
        }
        None
    }
}

/// Everything the recorder needs beyond the capture geometry. Aggregated so the
/// call signature does not grow without bound as we add camera / audio tracks.
///
/// Every field is cross-platform: capture is `xcap`-backed and the audio/video
/// device resolution picks the right ffmpeg input format per operating system.
#[derive(Clone, Debug, Default)]
pub struct RecordOptions {
    /// Frames per second (4–60).
    pub fps: u32,
    /// Output resolution; `None` keeps the captured size.
    pub resolution: Option<(u32, u32)>,
    /// Picture-in-picture webcam. `None` = off; `Some("")` = default device,
    /// `Some("Name")` = a specific device.
    pub webcam: Option<String>,
    /// System/desktop audio (Windows: WASAPI loopback; macOS/Linux: a
    /// monitor/loopback device the user has set up). `None` = off.
    pub system_audio: Option<String>,
    /// Microphone. `None` = off.
    pub mic: Option<String>,
    /// Pre-recording countdown seconds.
    pub countdown: u32,
    /// Optional watermark burned into every frame. Shares the screenshot
    /// decoration settings, so a user who set one gets it on video too.
    pub watermark: Option<crate::decorate::WatermarkDeco>,
}

#[allow(clippy::too_many_arguments)]
pub fn start(
    monitor_id: u32,
    region: ImgRect,
    dir: &std::path::Path,
    options: &util::SaveOptions,
    max_seconds: u32,
    record_options: RecordOptions,
) -> RecordJob {
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let stop_thread = stop.clone();
    let fps = record_options.fps.clamp(4, 60);
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

    let countdown = record_options.countdown.clamp(0, 10);
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
                &record_options,
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
        active: Duration::ZERO,
        last_tick: Instant::now(),
        was_paused: false,
        countdown,
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
    opts: &RecordOptions,
) -> Result<(), String> {
    // Pre-recording countdown: one update per second, abortable via stop
    // (Esc or the pill) at any point before frame 0.
    for remaining in (1..=opts.countdown.clamp(0, 10)).rev() {
        if stop.load(Ordering::Relaxed) {
            let _ = tx.send(RecordUpdate::Cancelled);
            return Ok(());
        }
        let _ = tx.send(RecordUpdate::Countdown { remaining });
        std::thread::sleep(Duration::from_secs(1));
    }
    if stop.load(Ordering::Relaxed) {
        let _ = tx.send(RecordUpdate::Cancelled);
        return Ok(());
    }
    // Countdown finished: clear it so the HUD swaps the big number for the
    // status pill. Without this the last sent value (1) would stick forever.
    if opts.countdown > 0 {
        let _ = tx.send(RecordUpdate::Countdown { remaining: 0 });
    }

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

    let mut sink = Sink::create(path, rw, rh, fps, opts)?;
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
    // Last captured frame, held so a pause can be filled with it: the MP4
    // pipe advances at a constant frame rate, so skipping writes while paused
    // would make the video fast-forward through the pause. Repeating the last
    // frame keeps the timeline honest (and GIFs get a free freeze-frame).
    let mut held: Option<image::RgbaImage> = None;

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
        let mut paused_this_pause = false;
        while pause.load(Ordering::Relaxed) && !stop.load(Ordering::Relaxed) {
            paused_this_pause = true;
            std::thread::sleep(Duration::from_millis(60));
        }
        // Back from a pause: top the stream up with the held frame so the
        // encoded timeline matches wall-clock time.
        if paused_this_pause {
            if let Some(held_frame) = &held {
                let owed = frames_owed(start.elapsed(), frames, fps);
                for _ in 0..owed {
                    sink.write(held_frame, fps)?;
                    frames += 1;
                }
            }
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
        held = Some(image);
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

/// Frames owed to fill a paused span so the encoded timeline stays aligned
/// with wall-clock time. Pure helper so it can be unit-tested.
fn frames_owed(elapsed: Duration, frames_done: u32, fps: u32) -> u32 {
    ((elapsed.as_secs_f64() * fps as f64) as u32)
        .saturating_sub(frames_done)
        .min(3600 * fps.max(1))
}

enum Sink {
    Gif {
        encoder: gif::Encoder<std::fs::File>,
        rgba: Vec<u8>,
        watermark: Option<crate::decorate::WatermarkStamp>,
    },
    Mp4 {
        child: Child,
        stdin: Option<ChildStdin>,
        rgba: Vec<u8>,
        watermark: Option<crate::decorate::WatermarkStamp>,
    },
}

impl Sink {
    fn create(
        path: &std::path::Path,
        w: u32,
        h: u32,
        fps: u32,
        opts: &RecordOptions,
    ) -> Result<Self, String> {
        // Pre-render the watermark once for this frame size. `None` when it is
        // off or has neither text nor a readable image, so the hot loop pays
        // nothing in the common case.
        let watermark = opts
            .watermark
            .as_ref()
            .and_then(|wm| crate::decorate::WatermarkStamp::prepare(wm, w, h));
        if path.extension().and_then(|e| e.to_str()) == Some("mp4") {
            // Input 0 is the raw RGBA screen pipe fed by the recorder thread.
            // A webcam (picture-in-picture) and audio tracks are additional
            // ffmpeg inputs; everything is composited/muxed in filter_complex.
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

            // Extra inputs: webcam (video), then system + mic audio.
            let mut next_idx = 1u32;
            let mut cam_idx: Option<u32> = None;
            if let Some(cam) = &opts.webcam {
                match camera_input(cam) {
                    Some((fmt, spec)) => {
                        // Live device inputs need a larger demuxer queue or
                        // ffmpeg blocks/drops frames while the screen pipe is
                        // being written at full speed.
                        args.push("-thread_queue_size".into());
                        args.push("1024".into());
                        args.push("-f".into());
                        args.push(fmt);
                        args.push("-i".into());
                        args.push(spec);
                        cam_idx = Some(next_idx);
                        next_idx += 1;
                    }
                    None => tracing::warn!(
                        "webcam is on but no camera device could be resolved; recording screen only"
                    ),
                }
            }
            let mut audio_idxs: Vec<u32> = Vec::new();
            if let Some(sys) = &opts.system_audio {
                if let Some((fmt, spec)) = system_audio_input(sys) {
                    args.push("-thread_queue_size".into());
                    args.push("1024".into());
                    args.push("-f".into());
                    args.push(fmt);
                    args.push("-i".into());
                    args.push(spec);
                    audio_idxs.push(next_idx);
                    next_idx += 1;
                } else {
                    tracing::warn!(
                        "system audio is on but no loopback device could be resolved \
                         (macOS/Linux need a BlackHole/monitor device); recording without it"
                    );
                }
            }
            if let Some(mic) = &opts.mic {
                if let Some(device) = resolve_audio_device(mic) {
                    args.push("-thread_queue_size".into());
                    args.push("1024".into());
                    args.push("-f".into());
                    args.push(audio_format_flag().into());
                    args.push("-i".into());
                    args.push(audio_device_spec(&device));
                    audio_idxs.push(next_idx);
                } else {
                    tracing::warn!(
                        "microphone is on but no audio device could be resolved; recording without it"
                    );
                }
            }

            // ---- filter_complex: optional scale, camera overlay, audio mix ----
            let uses_filter =
                opts.resolution.is_some() || cam_idx.is_some() || audio_idxs.len() >= 2;
            let mut fc = String::new();
            if uses_filter {
                if let Some((rw, rh)) = opts.resolution {
                    fc.push_str(&format!("[0:v]scale={rw}:{rh}[base];"));
                } else {
                    fc.push_str("[0:v]copy[base];");
                }
            }
            let video_label: String = if let Some(ci) = cam_idx {
                fc.push_str(&format!("[{ci}:v]scale=320:-1[cam];"));
                // Drop the bubble in the bottom-right corner; W/H are the main
                // (screen) input's dimensions inside the overlay filter.
                fc.push_str("[base][cam]overlay=W-330:H-190[camv];");
                "[camv]".to_string()
            } else if uses_filter {
                "[base]".to_string()
            } else {
                "0:v:0".to_string()
            };
            let audio_label: Option<String> = if audio_idxs.len() >= 2 {
                fc.push_str(&format!(
                    "[{}:a][{}:a]amix=inputs=2:normalize=0[a];",
                    audio_idxs[0], audio_idxs[1]
                ));
                Some("[a]".to_string())
            } else {
                None
            };
            if !fc.is_empty() {
                args.push("-filter_complex".into());
                args.push(fc);
            }

            // Maps + codecs.
            args.push("-map".into());
            args.push(video_label);
            if let Some(al) = &audio_label {
                args.push("-map".into());
                args.push(al.clone());
                args.push("-c:a".into());
                args.push("aac".into());
            } else if audio_idxs.len() == 1 {
                args.push("-map".into());
                args.push(format!("{}:a:0", audio_idxs[0]));
                args.push("-c:a".into());
                args.push("aac".into());
            } else {
                args.push("-an".into());
            }
            args.push("-c:v".into());
            args.push("libx264".into());
            args.push("-preset".into());
            args.push("veryfast".into());
            args.push("-pix_fmt".into());
            args.push("yuv420p".into());
            args.push("-movflags".into());
            args.push("+faststart".into());
            // End as soon as the screen pipe (the shortest input) reaches EOF so
            // persistent device inputs do not keep ffmpeg alive after we stop.
            args.push("-shortest".into());
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
                watermark,
            });
        }

        // GIF fallback: silent, source-sized, no camera (the gif encoder writes
        // raw frames from the screen pipe only).
        let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
        let mut encoder =
            gif::Encoder::new(file, w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
        encoder
            .set_repeat(gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        Ok(Sink::Gif {
            encoder,
            rgba: Vec::new(),
            watermark,
        })
    }

    fn write(&mut self, image: &image::RgbaImage, fps: u32) -> Result<(), String> {
        // Burn the watermark in first, on a copy: `image` may be the held frame
        // that a pause re-writes, and stamping it twice would darken the mark.
        let wm = match self {
            Sink::Gif { watermark, .. } => watermark.as_ref(),
            Sink::Mp4 { watermark, .. } => watermark.as_ref(),
        };
        let mut stamped: Option<image::RgbaImage> = None;
        if let Some(wm) = wm {
            let mut copy = image.clone();
            wm.stamp(&mut copy);
            stamped = Some(copy);
        }
        let frame = stamped.as_ref().unwrap_or(image);

        match self {
            Sink::Gif { encoder, rgba, .. } => {
                rgba.clear();
                rgba.extend_from_slice(frame.as_raw());
                let mut gif_frame = gif::Frame::from_rgba_speed(
                    frame.width() as u16,
                    frame.height() as u16,
                    rgba,
                    10,
                );
                // GIF delays are expressed in hundredths of a second.
                gif_frame.delay = (100 / fps.max(1)).max(1) as u16;
                encoder.write_frame(&gif_frame).map_err(|e| e.to_string())
            }
            Sink::Mp4 { stdin, rgba, .. } => {
                let Some(stdin) = stdin.as_mut() else {
                    return Err("ffmpeg stdin closed".into());
                };
                rgba.clear();
                rgba.extend_from_slice(frame.as_raw());
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

/// Build the ffmpeg `-f`/`-i` pair for a webcam device, picking the right input
/// format per OS and resolving a default device when the name is empty.
/// Returns `None` only if a default cannot be discovered (rare on Windows).
fn camera_input(device: &str) -> Option<(String, String)> {
    match std::env::consts::OS {
        "windows" => {
            let name = if device.is_empty() {
                windows_default_dshow_video_device()?
            } else {
                device.to_string()
            };
            Some(("dshow".into(), format!("video=\"{name}\"")))
        }
        "macos" => {
            let name = if device.is_empty() {
                "0".to_string()
            } else {
                device.to_string()
            };
            Some(("avfoundation".into(), name))
        }
        "linux" => {
            let name = if device.is_empty() {
                "/dev/video0".to_string()
            } else {
                device.to_string()
            };
            Some(("v4l2".into(), name))
        }
        _ => None,
    }
}

/// Build the ffmpeg `-f`/`-i` pair for a system/desktop audio loopback device.
///
/// On Windows this is WASAPI, which captures the default output device when the
/// name is empty (no extra driver needed). On macOS/Linux a loopback device
/// (BlackHole / a Pulse monitor) must be named explicitly, so an empty name
/// returns `None` and the caller skips system audio with a warning.
fn system_audio_input(device: &str) -> Option<(String, String)> {
    match std::env::consts::OS {
        "windows" => {
            let spec = if device.is_empty() {
                String::new() // WASAPI default = default output (loopback)
            } else {
                format!("\"{device}\"")
            };
            Some(("wasapi".into(), spec))
        }
        "macos" => {
            if device.is_empty() {
                None
            } else {
                Some(("avfoundation".into(), format!(":{device}")))
            }
        }
        "linux" => {
            if device.is_empty() {
                None
            } else {
                Some(("pulse".into(), device.to_string()))
            }
        }
        _ => None,
    }
}

/// Ask ffmpeg to enumerate DirectShow devices and return the first video one.
fn windows_default_dshow_video_device() -> Option<String> {
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
    let mut in_video = false;
    for line in text.lines() {
        if line.contains("DirectShow video devices") {
            in_video = true;
            continue;
        }
        if in_video {
            if let Some(start) = line.find('"') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.find('"') {
                    let name = rest[..end].to_string();
                    if !name.is_empty() {
                        return Some(name);
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::frames_owed;
    use std::time::Duration;

    #[test]
    fn owes_frames_for_the_paused_span() {
        // 2 s at 10 fps with 5 frames already written → 15 owed.
        assert_eq!(frames_owed(Duration::from_secs(2), 5, 10), 15);
        // Nothing owed when the timeline is already ahead.
        assert_eq!(frames_owed(Duration::from_secs(1), 30, 10), 0);
        // Sanity cap: an hour of frames at most, regardless of elapsed.
        assert_eq!(frames_owed(Duration::from_secs(100_000), 0, 10), 3600 * 10);
        // fps 0 encodes no frames whatever the elapsed time.
        assert_eq!(frames_owed(Duration::from_secs(1), 0, 0), 0);
    }
}
