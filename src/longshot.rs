//! Scrolling long-screenshot capture and frame stitching.
//!
//! The worker scrolls the content under the selected region with `enigo`,
//! captures frames with `xcap`, finds the vertical overlap between consecutive
//! frames and appends the new part. Everything runs off the UI thread.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::time::Duration;

use image::RgbaImage;

use crate::annotate::ImgRect;

/// Set by the low-level hooks when the user asks to stop (Esc, or a left/right
/// mouse click). A watcher thread bridges it onto the active job's `stop` flag.
/// Kept as a plain flag (never a pointer to the job's flag) so the hook never
/// touches job memory.
#[cfg(windows)]
static STOP_HIT: AtomicBool = AtomicBool::new(false);

/// Watch for stop requests while a long screenshot runs — Esc **or a left/right
/// mouse click** — so the user can end it at any time regardless of which window
/// has focus. Low-level hooks (`WH_KEYBOARD_LL` / `WH_MOUSE_LL`) are the only
/// mechanism that see the input while the target window (a browser, an editor)
/// is in the foreground; polling the UI would see nothing because the root
/// window is parked off-screen during the capture.
#[cfg(windows)]
fn spawn_esc_watcher(stop: Arc<AtomicBool>, done: Arc<AtomicBool>) {
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, PeekMessageW, SetWindowsHookExW,
        UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_LBUTTONDOWN,
        WM_RBUTTONDOWN, WM_SYSKEYDOWN,
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

    unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        unsafe {
            if code == 0 {
                let msg = wparam.0 as u32;
                if msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN {
                    // A left/right click ends the capture (phone-long-screenshot
                    // style: tap to finish). The wheel is untouched, so the user
                    // can still scroll in manual mode.
                    STOP_HIT.store(true, Ordering::Relaxed);
                }
            }
            CallNextHookEx(None, code, wparam, lparam)
        }
    }

    std::thread::Builder::new()
        .name("pinsight-longshot-esc".into())
        .spawn(move || {
            unsafe {
                let key: HHOOK =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), None, 0).unwrap_or_default();
                let mouse: HHOOK =
                    SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), None, 0).unwrap_or_default();
                if key.0.is_null() && mouse.0.is_null() {
                    tracing::warn!(
                        "stop hooks failed to install; press the long-capture hotkey to stop"
                    );
                }
                let mut msg = MSG::default();
                // PeekMessageW keeps the thread pumping so the low-level hooks
                // fire; the sleep bounds how quickly we notice `done`.
                loop {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {}
                    if STOP_HIT.swap(false, Ordering::Relaxed) {
                        tracing::debug!("stop requested (Esc / mouse) — stopping long screenshot");
                        stop.store(true, Ordering::Relaxed);
                        break;
                    }
                    if done.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
                if !key.0.is_null() {
                    let _ = UnhookWindowsHookEx(key);
                }
                if !mouse.0.is_null() {
                    let _ = UnhookWindowsHookEx(mouse);
                }
            }
        })
        .ok();
}
use crate::capture;
use crate::util;

pub enum LongShotUpdate {
    Progress { frames: u32, height: u32 },
    Finished(RgbaImage),
    Failed(String),
}

/// Tunables for one scrolling capture. They live in the settings page because
/// the right values depend on the target app: a browser with smooth scrolling
/// needs a longer settle time than a native list box, and a page full of video
/// needs a higher difference threshold so it does not end early.
#[derive(Debug, Clone, Copy)]
pub struct LongShotOptions {
    /// Stop once the stitched image reaches this height.
    pub max_height: u32,
    /// Wheel notches posted per step in auto mode; 0 = manual (user scrolls).
    pub scroll_ticks: u32,
    /// Settle time after a scroll, before the frame is sampled.
    pub scroll_delay: Duration,
    /// Mean per-pixel difference below which a sample counts as "not moving".
    pub diff_threshold: f32,
    /// Still samples tolerated in auto mode before the capture ends.
    pub idle_limit: u32,
    /// Still samples tolerated in manual mode before the capture ends.
    pub manual_idle_limit: u32,
    /// Hard cap on sampling rounds.
    pub max_frames: u32,
}

impl Default for LongShotOptions {
    fn default() -> Self {
        Self {
            max_height: 20_000,
            scroll_ticks: 4,
            scroll_delay: Duration::from_millis(420),
            diff_threshold: 1.5,
            idle_limit: 8,
            // Manual mode: the user drives the scroll and may pause to read.
            // Allow ~30 s of stillness (each idle round is ~150 ms) before
            // giving up, so a pause never ends the capture or — worse — pastes a
            // duplicate.  The hard `max_frames` cap still bounds a forgotten job.
            manual_idle_limit: 200,
            max_frames: 400,
        }
    }
}

pub struct LongShotJob {
    stop: Arc<AtomicBool>,
    pub rx: Receiver<LongShotUpdate>,
    pub frames: u32,
    pub height: u32,
    /// Physical rectangle of the monitor the capture runs on — the floating
    /// progress pill is centred on it.
    pub monitor: crate::util::PhysRect,
    /// Scale factor of that monitor (points → physical pixels).
    pub scale: f32,
}

impl LongShotJob {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn poll(&mut self) -> Option<LongShotUpdate> {
        while let Ok(update) = self.rx.try_recv() {
            match &update {
                LongShotUpdate::Progress { frames, height } => {
                    self.frames = *frames;
                    self.height = *height;
                }
                _ => return Some(update),
            }
        }
        None
    }
}

pub fn start(monitor_id: u32, region: ImgRect, options: LongShotOptions) -> LongShotJob {
    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicBool::new(false));
    let (tx, rx) = std::sync::mpsc::channel();
    let stop_thread = stop.clone();
    let done_thread = done.clone();
    // Placement metadata for the floating progress pill. Best effort: a failed
    // lookup falls back to the region itself at scale 1.
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
    // Esc-to-stop works while the target window has focus; reset the latch so a
    // stale press from a previous capture cannot end this one.
    #[cfg(windows)]
    STOP_HIT.store(false, Ordering::Relaxed);
    #[cfg(windows)]
    spawn_esc_watcher(stop.clone(), done.clone());
    std::thread::Builder::new()
        .name("pinsight-longshot".into())
        .spawn(move || {
            let result = run(monitor_id, region, options, &tx, &stop_thread);
            // Tell the Esc watcher the capture is over so it unhooks and exits.
            done_thread.store(true, Ordering::Relaxed);
            if let Err(err) = result {
                let _ = tx.send(LongShotUpdate::Failed(err));
            }
        })
        .expect("spawn long screenshot thread");

    LongShotJob {
        stop,
        rx,
        frames: 0,
        height: 0,
        monitor: monitor_rect,
        scale,
    }
}

fn run(
    monitor_id: u32,
    region: ImgRect,
    options: LongShotOptions,
    tx: &Sender<LongShotUpdate>,
    stop: &AtomicBool,
) -> Result<(), String> {
    let monitor = capture::find_monitor(monitor_id).map_err(|e| e.to_string())?;
    let rx = region.x.round().max(0.0) as u32;
    let ry = region.y.round().max(0.0) as u32;
    let rw = region.w.round().max(2.0) as u32;
    let rh = region.h.round().max(2.0) as u32;
    let max_height = options.max_height.max(rh + 64);
    let manual = options.scroll_ticks == 0;

    std::thread::sleep(Duration::from_millis(220));

    // Go through the shared capture path so the GDI black-frame fallback
    // applies: a region grab straight from xcap returned a stale frame here,
    // which made every sample identical and the job end with "no content".
    let abs_rect = crate::util::PhysRect {
        x: monitor.rect.x + rx as i32,
        y: monitor.rect.y + ry as i32,
        w: rw,
        h: rh,
    };
    // Exclude the floating progress pill from the capture. With the `wgc`
    // feature enabled, `capture_region` runs on Windows.Graphics.Capture, which
    // honours `WDA_EXCLUDEFROMCAPTURE`: the UI thread re-asserts it every frame,
    // and we set it once here too so the very first grab — which can race ahead
    // of the first UI frame — is already clean. WGC then drops the pill from the
    // frame entirely (no black rectangle, no hide/show flicker).
    let _ = crate::capture::set_capture_exclusion(crate::hud::LONG_WINDOW_TITLE);
    let first = crate::capture::grab_physical_rect(abs_rect).map_err(|e| e.to_string())?;
    let mut output = Stitch::from_image(&first);
    let mut previous_frame = first.clone();
    let mut frames = 0u32;
    dump_frame(frames, &first);
    let _ = tx.send(LongShotUpdate::Progress {
        frames,
        height: output.height(),
    });

    tracing::info!(monitor_id, rw, rh, "long screenshot started");
    // Frames that showed no movement. The job keeps sampling instead of giving
    // up immediately, so scrolling by hand also extends the capture; it only
    // stops after the content has been still for a few seconds.
    let mut idle_rounds = 0u32;
    // Frames that could not be aligned with the output at all.
    let mut unaligned_rounds = 0u32;
    // Fixed footer lifted out of the stitched body, re-attached at the very end.
    let mut footer: Option<RgbaImage> = None;
    for _ in 0..options.max_frames.max(10) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if output.height() >= max_height {
            break;
        }

        {
            let abs_x = monitor.rect.x + rx as i32 + (rw / 2) as i32;
            let abs_y = monitor.rect.y + ry as i32 + (rh / 2) as i32;
            // Manual mode (scroll_ticks == 0): PinSight does not scroll — the
            // user rolls the wheel themselves and we just sample whatever moves
            // into view. This is the most reliable mode because it uses the
            // real input the user is already producing, and it lets them decide
            // exactly how long the capture is. Auto mode posts the wheel.
            if !manual {
                scroll_at(abs_x, abs_y, options.scroll_ticks as i32);
            }
        }

        // Give smooth-scrolling animations time to settle before sampling.
        // In manual mode the wheel is not posted — the user scrolls by hand —
        // so a long settle here is dead time during which content scrolls past
        // uncaptured, which is exactly what produced the "blank seam" reports.
        // Sample quickly instead so consecutive frames always overlap the
        // previous one and no slice is skipped.
        let settle = if manual {
            Duration::from_millis(30)
        } else {
            options.scroll_delay
        };
        std::thread::sleep(settle);
        // The pill stays excluded via `WDA_EXCLUDEFROMCAPTURE` (re-asserted by
        // the UI thread every frame), so each region grab is already clean — no
        // per-frame hide/show flicker.
        let next = match crate::capture::grab_physical_rect(abs_rect) {
            Ok(next) => next,
            Err(_) => {
                break;
            }
        };
        frames += 1;
        dump_frame(frames, &next);

        // A frame that is essentially identical to the previous one means the
        // page did not move.  Pausing to read must never paste a duplicate, so
        // this is checked *before* aligning: a static frame would otherwise be
        // mistaken for a tiny scroll and append a repeated band (the "stayed on
        // a page and got two copies" report).
        if frame_difference(&previous_frame, &next) < options.diff_threshold {
            idle_rounds += 1;
            let idle_limit = if manual {
                options.manual_idle_limit
            } else {
                options.idle_limit
            };
            // At the bottom of the scroll range the page is still but the
            // scrollbar thumb has reached the end; stop after a short
            // confirmation instead of waiting out the long reading-pause timeout.
            if idle_rounds > idle_limit || (idle_rounds >= 3 && scrollbar_at_bottom(&next)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(120));
            continue;
        }

        // Classify the movement.  Aligning against the *previous full frame*
        // (not the tall stitched output) makes a static frame a full overlap and
        // stops a small scroll from being under-estimated against rows that
        // already belong to older frames.  The forward and reverse matches are
        // compared by quality so a reverse scroll — which can also produce a
        // weak, coincidental forward match — is never stitched as new content.
        let overlap = match classify_scroll(&previous_frame, &next) {
            Scroll::Down(overlap) => overlap,
            Scroll::Up => {
                // The user scrolled back into content that is already stitched.
                tracing::debug!(frames, "long screenshot: scrolled back up — skipped");
                idle_rounds += 1;
                let idle_limit = if manual {
                    options.manual_idle_limit
                } else {
                    options.idle_limit
                };
                if idle_rounds > idle_limit {
                    break;
                }
                std::thread::sleep(Duration::from_millis(120));
                continue;
            }
            Scroll::Unknown => {
                // Pasting the whole frame would duplicate the screen, so the
                // frame is dropped instead — the next sample may line up again.
                tracing::warn!(
                    frames,
                    "long screenshot frame does not overlap the output — skipped"
                );
                unaligned_rounds += 1;
                if unaligned_rounds > 6 {
                    tracing::warn!("long screenshot: too many unalignable frames — stopping");
                    break;
                }
                continue;
            }
        };

        // Strip fixed (sticky) chrome from the appended band.  On a large
        // scroll the overlap can fall inside the header, which would then be
        // re-appended and repeat down the stitched image (a browser nav bar or
        // the editor's tab strip).  If the overlap itself is inside the header
        // the alignment cannot be trusted — a header row matches content
        // anywhere — so the frame is dropped instead.
        let (fixed_top, fixed_bottom) = fixed_bands(&previous_frame, &next);
        if fixed_top > 0 && overlap < fixed_top {
            tracing::warn!(
                frames,
                overlap,
                fixed_top,
                "long screenshot: overlap inside the fixed header — skipped"
            );
            unaligned_rounds += 1;
            if unaligned_rounds > 6 {
                tracing::warn!("long screenshot: too many unalignable frames — stopping");
                break;
            }
            continue;
        }
        unaligned_rounds = 0;

        // A fixed footer is identical in every frame; the first capture already
        // holds it at its bottom, so lift it out of the stitched body here and
        // re-attach it once at the very end instead of repeating it per slice.
        if footer.is_none() && fixed_bottom > 0 && output.height() > fixed_bottom {
            footer = Some(output.take_tail_rows(fixed_bottom));
        }

        let append_from = overlap.max(fixed_top);
        let append_to = next.height().saturating_sub(fixed_bottom);
        let new_rows = append_to.saturating_sub(append_from);
        tracing::debug!(
            frames,
            overlap,
            fixed_top,
            fixed_bottom,
            new_rows,
            mean = pixel_mean(&next),
            out_h = output.height(),
            "long screenshot stitch"
        );
        if new_rows == 0 {
            // The page did not move: nothing to add.  Cursor blinking and
            // video overlays make exact equality too strict, so treat a frame
            // that contributes nothing as "not moving" and keep sampling —
            // this is expected when the user drives the scroll by hand.
            idle_rounds += 1;
            let idle_limit = if manual {
                options.manual_idle_limit
            } else {
                options.idle_limit
            };
            if idle_rounds > idle_limit {
                break;
            }
            std::thread::sleep(Duration::from_millis(120));
            continue;
        }
        idle_rounds = 0;
        previous_frame = next.clone();

        // Append the rows the frame adds below what the output already holds,
        // starting past the fixed header.
        let append_height = new_rows.min(max_height.saturating_sub(output.height()));
        if append_height == 0 {
            break;
        }
        output.push_rows(&next, append_from, append_height);
        let _ = tx.send(LongShotUpdate::Progress {
            frames,
            height: output.height(),
        });
        if output.height() >= max_height {
            break;
        }
    }

    if stop.load(Ordering::Relaxed) {
        // Stopped on demand (Esc / the finish button). Keep whatever was
        // captured rather than throwing it away — stopping early is a normal
        // way to end a capture, not a failure.
        if output.height() > rh {
            tracing::info!(
                frames,
                output_height = output.height(),
                "long screenshot stopped by user"
            );
            let mut image = output.into_image();
            if let Some(band) = footer {
                image = append_band(&image, &band, true);
            }
            let _ = tx.send(LongShotUpdate::Finished(image));
        } else {
            let _ = tx.send(LongShotUpdate::Failed("cancelled".into()));
        }
        return Ok(());
    }
    if output.height() <= rh {
        tracing::warn!("long screenshot produced no new content");
        return Err(
            "No scrollable content detected. Select a region over the scrolling area, \
             click that window once, then scroll — or let PinSight scroll for you."
                .into(),
        );
    }
    tracing::info!(
        frames,
        output_height = output.height(),
        "long screenshot finished"
    );
    let mut image = output.into_image();
    if let Some(band) = footer {
        image = append_band(&image, &band, true);
    }
    let _ = tx.send(LongShotUpdate::Finished(image));
    Ok(())
}

/// Mean absolute RGB difference over a dense sample of the frame. Values are
/// in 0..=255; static content is normally below 1.0 even with a blinking caret.
fn frame_difference(a: &RgbaImage, b: &RgbaImage) -> f32 {
    let w = a.width().min(b.width());
    let h = a.height().min(b.height());
    if w == 0 || h == 0 {
        return f32::MAX;
    }
    let x_step = (w / 160).max(1);
    let y_step = (h / 120).max(1);
    let mut total = 0u64;
    let mut samples = 0u64;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let pa = a.get_pixel(x, y);
            let pb = b.get_pixel(x, y);
            total += pa[0].abs_diff(pb[0]) as u64
                + pa[1].abs_diff(pb[1]) as u64
                + pa[2].abs_diff(pb[2]) as u64;
            samples += 3;
            x += x_step;
        }
        y += y_step;
    }
    total as f32 / samples.max(1) as f32
}

/// Find the height of the overlapping band at the bottom of `prev` / top of
/// `next`. Returns the overlap in pixels, or `None` when no plausible match is
/// found (e.g. content changed too much or the page has reached its end).
/// Move the cursor to (`x`, `y`) and turn the wheel `clicks` notches towards
/// the end of the document.
///
/// The wheel is delivered by posting `WM_MOUSEWHEEL` straight to the window
/// under the point. The wheel is delivered only by posting `WM_MOUSEWHEEL`;
/// injecting it through the input queue as well made some apps scroll twice
/// per step (see the note in the function body).
#[cfg(windows)]
fn scroll_at(x: i32, y: i32, clicks: i32) {
    use windows::Win32::Foundation::{LPARAM, POINT, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOT, GetAncestor, PostMessageW, SetCursorPos, WM_MOUSEWHEEL, WindowFromPoint,
    };

    let clicks = clicks.max(1);
    unsafe {
        let _ = SetCursorPos(x, y);
        std::thread::sleep(Duration::from_millis(30));

        // Deliver the wheel to the top-level window under the point. Notepad
        // (and most classic Win32 apps) scroll from the frame window, while the
        // inner edit control ignores a posted WM_MOUSEWHEEL — posting to the
        // inner window left the page completely still. Posting to *both* made
        // Notepad scroll twice per step, so frames no longer overlapped and
        // whole slices went missing; the top-level window is the reliable
        // single target.
        //
        // Note: some apps only scroll while they are the foreground window. We
        // deliberately do NOT call SetForegroundWindow here — from this
        // background thread it is blocked by the foreground lock and made the
        // scroll stop entirely. The capture overlay is closed and the root
        // window parked before scrolling, so the target keeps the foreground it
        // already had.
        let inner = WindowFromPoint(POINT { x, y });
        if !inner.is_invalid() {
            let root = GetAncestor(inner, GA_ROOT);
            let target = if !root.is_invalid() { root } else { inner };
            // High word of wParam is the signed wheel delta (negative = down).
            let wparam = WPARAM(((-120 * clicks) as u16 as usize) << 16);
            let lparam = LPARAM((((y as u16 as usize) << 16) | (x as u16 as usize)) as isize);
            for _ in 0..clicks {
                let _ = PostMessageW(Some(target), WM_MOUSEWHEEL, wparam, lparam);
            }
        }
        // No SendInput fallback here: with the overlay gone the injected wheel
        // event also reaches the window, so posting *and* injecting scrolled
        // the page twice per step. Frames then no longer overlapped, the
        // stitcher skipped them, and the capture stopped after a sliver.
        // stitcher skipped them, and the capture stopped after a sliver.
    }
}

/// Non-Windows fallback: enigo drives the wheel through the platform's own
/// input stack (CGEvent on macOS, XTest/libei on Linux), so the same call
/// works everywhere. Windows uses the raw SendInput path above because enigo's
/// wheel did not move the target window there.
#[cfg(not(windows))]
fn scroll_at(x: i32, y: i32, clicks: i32) {
    use enigo::{Axis, Coordinate, Mouse};
    let settings = enigo::Settings::default();
    let Ok(mut enigo) = enigo::Enigo::new(&settings) else {
        return;
    };
    let _ = enigo.move_mouse(x, y, Coordinate::Abs);
    std::thread::sleep(Duration::from_millis(40));
    // Positive vertical length scrolls towards the end of the document on
    // macOS and Linux.
    let _ = enigo.scroll(clicks.max(1), Axis::Vertical);
}

/// Mean channel value of an image: 0 for a black frame, ~255 for white. Used
/// only for diagnostics, to tell "the capture is black" apart from "the page
/// really did not move".
fn pixel_mean(image: &image::RgbaImage) -> u32 {
    let mut total = 0u64;
    let mut samples = 0u64;
    for pixel in image.pixels().step_by(97) {
        total += (pixel[0] as u64 + pixel[1] as u64 + pixel[2] as u64) / 3;
        samples += 1;
    }
    (total / samples.max(1)) as u32
}

/// Write every captured frame to disk when `PINSIGHT_DUMP_FRAMES` names a
/// directory.
///
/// This is a development aid: with the raw frames on disk the aligner can be
/// replayed against exactly what the screen looked like, instead of guessing
/// from the stitched result.  It is off unless the variable is set.
fn dump_frame(index: u32, image: &RgbaImage) {
    let Ok(dir) = std::env::var("PINSIGHT_DUMP_FRAMES") else {
        return;
    };
    if dir.trim().is_empty() {
        return;
    }
    let path = std::path::Path::new(&dir).join(format!("frame_{index:04}.png"));
    if let Err(err) = image.save(&path) {
        tracing::warn!(%err, ?path, "could not dump frame");
    }
}

// ---------------------------------------------------------------------------
// Frame alignment
// ---------------------------------------------------------------------------
//
// Two frames of the same region differ by a vertical translation: the content
// moved up by `d` pixels, so `next[0 .. H-d]` is what `prev` showed at its
// bottom.  Finding `d` is the whole job of stitching, and getting it wrong is
// what makes a long screenshot look like a pile of separate grabs.
//
// Three rules keep the answer honest:
//
//  * *Search every overlap.*  The old code stopped at 90% of the frame, which
//    excludes the true overlap whenever the page moved only slightly (a
//    smooth-scrolling animation, or a terminal that has reached the end of its
//    buffer).  The aligner then locked onto a wrong, smaller overlap or found
//    none at all and pasted a whole duplicate frame.
//  * *Let the rows that carry ink vote.*  Blank rows look identical at every
//    offset, so a mean over the whole band is dominated by whatever is
//    empty.  Rows without ink are skipped, and the remaining rows vote on
//    whether they match.
//  * *Vote per row, not per pixel.*  A row counts as a match when most of its
//    columns agree.  One foreign window overlapping the region — or a
//    blinking caret — then costs a handful of votes instead of poisoning the
//    whole comparison.

/// Minimum number of rows two frames must share before a match is trusted.
const MIN_OVERLAP: u32 = 24;
/// Per-channel tolerance (0..255) when comparing two pixels.
const PIXEL_TOL: i32 = 12;
/// A row counts as the same when at least this share of its sampled columns
/// agree within `PIXEL_TOL`.
const ROW_MATCH_RATIO: f32 = 0.85;
/// Longest run of consecutive matching rows, as a share of all informative
/// rows (`best_run / total`).  A true overlap lines the page up so that every
/// informative row matches in one unbroken run (`coverage` ≈ 1.0); a wrong
/// offset only ever matches scattered, single rows, which gives a short run.
/// This is the *primary* ranking and the acceptance gate.  It is what lets a
/// large scroll — the page moved by 600–900 px, leaving only a small remaining
/// overlap — still win decisively over a tempting wrong offset whose votes
/// happen to add up.  `MIN_COVERAGE` is deliberately low: the bottom of a real
/// page can be genuinely different from the new frame, but as long as the
/// *aligned* portion forms one solid block the offset is right.
const MIN_COVERAGE: f32 = 0.30;
/// A comparison also needs at least this many informative rows to mean
/// anything, and at least this many matching rows to be accepted.
const MIN_VOTES: u32 = 12;

/// Number of sampled columns of `row` that carry ink (a pixel that is not
/// near-white).  Blank rows match at any vertical offset, so they must not be
/// allowed to vote.
fn row_ink(img: &RgbaImage, y: u32, x_step: u32) -> u32 {
    let w = img.width();
    let mut ink = 0u32;
    let mut x = 0u32;
    while x < w {
        let p = img.get_pixel(x, y);
        if p[0] < 245 || p[1] < 245 || p[2] < 245 {
            ink += 1;
        }
        x += x_step;
    }
    ink
}

/// Compare the bottom `k` rows of `prev` with the top `k` rows of `next`.
///
/// Returns `(matching rows, informative rows, summed column difference,
/// coverage)`. `coverage` is the longest run of consecutive matching rows
/// divided by the total number of informative rows — see `MIN_COVERAGE` for
/// why that is the decisive number.
#[allow(clippy::too_many_arguments)]
fn compare_band(
    prev: &RgbaImage,
    next: &RgbaImage,
    prev_h: u32,
    k: u32,
    x_step: u32,
    row_step: u32,
    prev_ink: &[u32],
    next_ink: &[u32],
) -> (u32, u32, u64, f32) {
    let w = prev.width().min(next.width());
    let mut votes = 0u32;
    let mut total = 0u32;
    let mut diff_sum = 0u64;
    // Longest run of consecutive informative-and-matching rows, and the run
    // currently being measured.  Blank rows (no ink) are not informative, so
    // they are skipped entirely and neither extend nor break a run.
    let mut best_run = 0u32;
    let mut cur_run = 0u32;
    let mut k_row = 0u32;
    while k_row < k {
        let py = prev_h - k + k_row;
        let ny = k_row;
        if py < prev_h && ny < next.height() {
            let pi = prev_ink[py as usize];
            let ni = next_ink[ny as usize];
            // Only rows that carry ink on both sides can say anything about
            // the alignment.
            if pi >= 2 && ni >= 2 {
                total += 1;
                let mut same = 0u32;
                let mut cols = 0u32;
                let mut x = 0u32;
                while x < w {
                    let a = prev.get_pixel(x, py);
                    let b = next.get_pixel(x, ny);
                    let d = (a[0] as i32 - b[0] as i32).abs()
                        + (a[1] as i32 - b[1] as i32).abs()
                        + (a[2] as i32 - b[2] as i32).abs();
                    diff_sum += d as u64;
                    if d <= PIXEL_TOL * 3 {
                        same += 1;
                    }
                    cols += 1;
                    x += x_step;
                }
                if cols > 0 && same as f32 >= ROW_MATCH_RATIO * cols as f32 {
                    votes += 1;
                    cur_run += 1;
                    if cur_run > best_run {
                        best_run = cur_run;
                    }
                } else {
                    cur_run = 0;
                }
            }
        }
        k_row += row_step;
    }
    let coverage = if total > 0 {
        best_run as f32 / total as f32
    } else {
        0.0
    };
    (votes, total, diff_sum, coverage)
}

/// Find the best overlap at the bottom of `prev` / top of `next`, together with
/// its match quality `(overlap, coverage, votes)`.
///
/// The overlap is measured against the *bottom* of `prev` — the accumulated
/// output grows without bound, so reading a band from its middle (which is
/// what the shared `min` height used to do) pointed at the wrong rows and
/// duplicated content.
fn find_overlap_scored(prev: &RgbaImage, next: &RgbaImage) -> Option<(u32, f32, u32)> {
    let w = prev.width().min(next.width());
    if w < 8 {
        return None;
    }
    let prev_h = prev.height();
    let next_h = next.height();
    // `next` only has `next_h` rows to offer, and `prev` only has `prev_h`.
    let max_overlap = prev_h.min(next_h);
    if max_overlap < MIN_OVERLAP {
        return None;
    }

    let x_step = (w / 64).max(1);
    let row_step = (max_overlap / 256).max(1);
    let cols = w.div_ceil(x_step).max(1) as f32;

    let prev_ink: Vec<u32> = (0..prev_h).map(|y| row_ink(prev, y, x_step)).collect();
    let next_ink: Vec<u32> = (0..next_h).map(|y| row_ink(next, y, x_step)).collect();

    // Search from the largest overlap down.  A large overlap means the page
    // barely moved, which is the common case for smooth-scrolling content.
    //
    // Ranking is by *coverage* — the longest unbroken run of matching rows as a
    // share of the informative rows — and only secondarily by the count of
    // matching rows.  The right offset lines the page up so that every
    // informative row matches in one solid block (coverage ≈ 1.0); a wrong
    // offset only ever matches scattered single rows, so its longest run is
    // short no matter how many votes it accumulates.  That is what makes a
    // large scroll (the page moved by 600–900 px, leaving only a small
    // remaining overlap) still win decisively instead of being out-voted by a
    // coincidental wrong offset.
    // Coarse-to-fine: scanning *every* overlap height is O(max_overlap) passes
    // of row comparison.  A coarse scan (every COARSE_STEP rows) locates the few
    // peaks, then only those peaks' neighbourhoods are compared at full
    // resolution.  The final selection still uses exact comparisons, so the
    // resulting offset is unchanged — this only skips provably-uninteresting
    // heights.
    const COARSE_STEP: u32 = 4;
    const COARSE_PEAKS: usize = 6;
    let mut coarse: Vec<(f32, u32, f32, f32, u32)> = Vec::new();
    let mut k = max_overlap;
    while k >= MIN_OVERLAP {
        let (votes, total, diff_sum, coverage) = compare_band(
            prev, next, prev_h, k, x_step, row_step, &prev_ink, &next_ink,
        );
        if total >= MIN_VOTES {
            let frac = votes as f32 / total as f32;
            let mean_diff = diff_sum as f32 / (total as f32 * cols);
            coarse.push((coverage, votes, frac, mean_diff, k));
        }
        k = k.saturating_sub(COARSE_STEP);
    }
    coarse.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.cmp(&a.1))
            .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
    });
    coarse.truncate(COARSE_PEAKS);

    let mut candidates: Vec<(f32, u32, f32, f32, u32)> = Vec::new();
    let mut seen: Vec<u32> = Vec::new();
    for &(_, _, _, _, peak) in &coarse {
        let lo = peak.saturating_sub(COARSE_STEP).max(MIN_OVERLAP);
        let hi = (peak + COARSE_STEP).min(max_overlap);
        let mut k = hi;
        loop {
            if !seen.contains(&k) {
                seen.push(k);
                let (votes, total, diff_sum, coverage) = compare_band(
                    prev, next, prev_h, k, x_step, row_step, &prev_ink, &next_ink,
                );
                if total >= MIN_VOTES {
                    let frac = votes as f32 / total as f32;
                    let mean_diff = diff_sum as f32 / (total as f32 * cols);
                    candidates.push((coverage, votes, frac, mean_diff, k));
                }
            }
            if k == lo {
                break;
            }
            k -= 1;
        }
    }
    // Safety net: if the coarse scan found nothing, fall back to the exhaustive
    // scan so a match is never lost to the sampling.
    if coarse.is_empty() {
        let mut k = max_overlap;
        while k >= MIN_OVERLAP {
            let (votes, total, diff_sum, coverage) = compare_band(
                prev, next, prev_h, k, x_step, row_step, &prev_ink, &next_ink,
            );
            if total >= MIN_VOTES {
                let frac = votes as f32 / total as f32;
                let mean_diff = diff_sum as f32 / (total as f32 * cols);
                candidates.push((coverage, votes, frac, mean_diff, k));
            }
            k -= 1;
        }
    }

    // Best first: highest coverage, then most votes, then highest agreement
    // fraction, then closest per-pixel match.
    candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.1.cmp(&a.1))
            .then(b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal))
            .then(a.3.partial_cmp(&b.3).unwrap_or(std::cmp::Ordering::Equal))
    });

    // Prefer the top-ranked offset whose appended band is not a near-exact copy
    // of the output immediately above the seam.  A duplicate means the overlap
    // was under-estimated; the next candidate is a larger overlap (smaller
    // new_rows) and is the correct one for repetitive pages.  If every candidate
    // duplicates (a uniform page), fall back to the top-ranked offset so a gap is
    // never introduced by this guard.
    let mut fallback: Option<(u32, f32, u32)> = None;
    for (coverage, votes, _, _, overlap) in &candidates {
        if *votes < MIN_VOTES || *coverage < MIN_COVERAGE {
            continue;
        }
        if fallback.is_none() {
            fallback = Some((*overlap, *coverage, *votes));
        }
        if !appended_band_duplicates_seam(prev, next, *overlap) {
            return Some((*overlap, *coverage, *votes));
        }
    }
    fallback
}

/// Overlap height only — see [`find_overlap_scored`] for the quality score.
pub fn find_overlap(prev: &RgbaImage, next: &RgbaImage) -> Option<u32> {
    find_overlap_scored(prev, next).map(|(overlap, _, _)| overlap)
}

/// Direction of movement between two consecutive frames.
enum Scroll {
    /// The page scrolled down; carries the overlap height to append.
    Down(u32),
    /// The page scrolled back up into content that is already stitched.
    Up,
    /// The frames do not line up (jumped further than one frame).
    Unknown,
}

/// Decide whether `next` continues below `prev` (down), returns into content
/// already captured (up), or cannot be aligned (unknown).
///
/// A reverse scroll can *also* produce a weak, coincidental forward match — that
/// is why a plain "no forward overlap ⇒ reverse scroll" test was not enough and
/// a scroll-back still pasted a duplicate.  Both directions are matched and
/// compared by quality (`coverage × votes`); the reverse only wins when it is
/// stronger and covers at least half the frame.  A genuine downward scroll has a
/// solid forward match, so it is never mistaken for a reverse one.
fn classify_scroll(prev: &RgbaImage, next: &RgbaImage) -> Scroll {
    let down = find_overlap_scored(prev, next);
    let up = find_overlap_scored(next, prev);
    let down_q = down.map(|(_, c, v)| c * v as f32).unwrap_or(0.0);
    let up_q = up.map(|(_, c, v)| c * v as f32).unwrap_or(0.0);
    if up_q > down_q {
        if let Some((up_overlap, _, _)) = up {
            if up_overlap * 2 >= next.height() {
                return Scroll::Up;
            }
        }
    }
    match down {
        Some((overlap, _, _)) => Scroll::Down(overlap),
        None => Scroll::Unknown,
    }
}

/// Minimum height (rows) of a fixed band before it is treated as chrome.
const MIN_FIXED_BAND: u32 = 8;
/// Never treat more than this share of the frame as fixed chrome.
const MAX_FIXED_SHARE: f32 = 0.4;

/// Heights of the fixed (non-scrolling) bands at the top and bottom of the
/// capture region, as `(top, bottom)` in rows.
///
/// A sticky header (browser nav bar, editor tab strip, frozen spreadsheet rows)
/// or footer (tmux status line, terminal status bar) sits at the same rows in
/// every frame while the content between them has scrolled.  Comparing two
/// consecutive frames at the *same* row detects both: those rows are
/// near-identical, whereas content rows have moved.  Either value is 0 when
/// there is no clear band on that side.
fn fixed_bands(prev: &RgbaImage, next: &RgbaImage) -> (u32, u32) {
    let h = prev.height().min(next.height());
    let w = prev.width().min(next.width());
    if w == 0 || h < MIN_OVERLAP * 2 {
        return (0, 0);
    }
    let x_step = (w / 64).max(1);
    let row_is_fixed = |y: u32| -> bool {
        let mut diff = 0u64;
        let mut n = 0u64;
        // Blank (near-white) rows are identical in every frame too, but they are
        // empty space, not chrome — treating them as a fixed band would strip
        // real content from mostly-white pages.  Require actual ink.
        let mut ink = 0u32;
        let mut x = 0u32;
        while x < w {
            let a = prev.get_pixel(x, y);
            let b = next.get_pixel(x, y);
            diff += a[0].abs_diff(b[0]) as u64
                + a[1].abs_diff(b[1]) as u64
                + a[2].abs_diff(b[2]) as u64;
            n += 3;
            if a[0] < 245 || a[1] < 245 || a[2] < 245 {
                ink += 1;
            }
            x += x_step;
        }
        n > 0 && ink >= 2 && (diff as f32 / n as f32) < 1.0
    };
    let max_band = (h as f32 * MAX_FIXED_SHARE) as u32;
    let mut top = 0u32;
    while top < max_band && row_is_fixed(top) {
        top += 1;
    }
    let mut bottom = 0u32;
    while bottom < max_band && top + bottom < h && row_is_fixed(h - 1 - bottom) {
        bottom += 1;
    }
    if top < MIN_FIXED_BAND {
        top = 0;
    }
    if bottom < MIN_FIXED_BAND {
        bottom = 0;
    }
    (top, bottom)
}

/// Prepend/append a captured fixed band without touching the stitched body.
fn append_band(body: &RgbaImage, band: &RgbaImage, at_bottom: bool) -> RgbaImage {
    let w = body.width();
    let h = body.height() + band.height();
    let mut out = RgbaImage::new(w, h);
    if at_bottom {
        image::imageops::replace(&mut out, body, 0, 0);
        image::imageops::replace(&mut out, band, 0, body.height() as i64);
    } else {
        image::imageops::replace(&mut out, band, 0, 0);
        image::imageops::replace(&mut out, body, 0, band.height() as i64);
    }
    out
}

/// True when a vertical scrollbar at the right edge of `frame` clearly shows
/// its thumb at the bottom — the content cannot scroll any further.
///
/// Best effort and deliberately conservative: it only ever *shortens* the wait
/// once the page is already still (the caller requires several idle rounds
/// first), so a mis-detection can never cut a mid-page reading pause short.  Apps
/// with overlay / auto-hidden scrollbars simply return `false` and fall back to
/// the normal idle timeout.
fn scrollbar_at_bottom(frame: &RgbaImage) -> bool {
    let w = frame.width();
    let h = frame.height();
    if w < 40 || h < 80 {
        return false;
    }
    // Brightness of the two rightmost columns (the scrollbar gutter).
    let mut edge = Vec::with_capacity(h as usize);
    for y in 0..h {
        let a = frame.get_pixel(w - 1, y);
        let b = frame.get_pixel(w - 2, y);
        let lum =
            (a[0] as u32 + a[1] as u32 + a[2] as u32 + b[0] as u32 + b[1] as u32 + b[2] as u32) / 6;
        edge.push(lum as u8);
    }
    // The gutter must be dominated by one colour, otherwise it is content, not
    // a scrollbar track.
    let mut hist = [0u32; 256];
    for &v in &edge {
        hist[v as usize] += 1;
    }
    let Some((track, track_count)) = hist
        .iter()
        .enumerate()
        .max_by_key(|(_, count)| **count)
        .map(|(value, count)| (value as u8, *count))
    else {
        return false;
    };
    if track_count * 100 < h * 70 {
        return false;
    }
    // The thumb is the longest run of rows that differ clearly from the track.
    let is_thumb = |v: u8| (v as i32 - track as i32).abs() > 12;
    let mut best_start = 0u32;
    let mut best_len = 0u32;
    let mut run_start = 0u32;
    let mut run_len = 0u32;
    for (y, &v) in edge.iter().enumerate() {
        let y = y as u32;
        if is_thumb(v) {
            if run_len == 0 {
                run_start = y;
            }
            run_len += 1;
            if run_len > best_len {
                best_len = run_len;
                best_start = run_start;
            }
        } else {
            run_len = 0;
        }
    }
    // A real thumb is at least a twentieth of the track.
    if best_len * 20 < h {
        return false;
    }
    // Within the bottom arrow button's height of the end ⇒ fully scrolled.
    best_start + best_len + 24 >= h
}

/// True when the rows `next` would contribute below `overlap`
/// (`next[overlap ..]`) are nearly pixel-identical to the output rows directly
/// above where they would be placed (`prev[prev_h - new_rows .. prev_h]`).
///
/// That is the signature of an *under-estimated* overlap: the appended strip
/// re-pastes content already in the stitched output, producing the "repeated
/// content" long screenshot.  The threshold is tight (near-exact) so merely
/// similar — but genuinely new — rows are never rejected.
fn appended_band_duplicates_seam(prev: &RgbaImage, next: &RgbaImage, overlap: u32) -> bool {
    let prev_h = prev.height();
    let next_h = next.height();
    let new_rows = next_h.saturating_sub(overlap);
    if new_rows < MIN_OVERLAP {
        return false;
    }
    let w = prev.width().min(next.width());
    if w == 0 {
        return false;
    }
    let x_step = (w / 64).max(1);
    let mut diff = 0u64;
    let mut n = 0u64;
    // Mean brightness of the appended band itself — used to reject blank bands,
    // which would otherwise read as a "duplicate" against an equally blank seam.
    let mut band_sum = 0u64;
    let mut band_n = 0u64;
    for row in 0..new_rows {
        let py = prev_h - new_rows + row;
        let ny = overlap + row;
        if py < prev_h && ny < next_h {
            let mut x = 0u32;
            while x < w {
                let a = prev.get_pixel(x, py);
                let b = next.get_pixel(x, ny);
                diff += a[0].abs_diff(b[0]) as u64
                    + a[1].abs_diff(b[1]) as u64
                    + a[2].abs_diff(b[2]) as u64;
                n += 3;
                band_sum += b[0] as u64 + b[1] as u64 + b[2] as u64;
                band_n += 3;
                x += x_step;
            }
        }
    }
    if n == 0 {
        return false;
    }
    // A band that is itself blank (near-white) cannot be re-pasting content, so
    // it is never a duplicate — this avoids false positives on mostly-empty
    // pages where the seam and the new band are both white.
    if band_n > 0 && band_sum / band_n > 245 {
        return false;
    }
    (diff as f32 / n as f32) < 1.5
}

/// Growable row buffer for the stitched image.
///
/// The old code re-allocated a whole `RgbaImage` and copied the entire output on
/// every append, which is O(n²) over a long capture (hundreds of frames × a
/// growing image, so a 20 000 px capture copies gigabytes in total).  Pushing
/// rows into a `Vec<u8>` is amortised O(n); the single `into_image` conversion at
/// the end is O(n).
struct Stitch {
    w: u32,
    h: u32,
    data: Vec<u8>,
}

impl Stitch {
    fn from_image(img: &RgbaImage) -> Self {
        Self {
            w: img.width(),
            h: img.height(),
            data: img.as_raw().clone(),
        }
    }

    fn height(&self) -> u32 {
        self.h
    }

    /// Append `rows` rows of `frame`, starting at `from_row`.
    fn push_rows(&mut self, frame: &RgbaImage, from_row: u32, rows: u32) {
        if rows == 0 {
            return;
        }
        let row_bytes = self.w as usize * 4;
        let start = from_row as usize * row_bytes;
        let end = start + rows as usize * row_bytes;
        if end <= frame.as_raw().len() {
            self.data.extend_from_slice(&frame.as_raw()[start..end]);
            self.h += rows;
        }
    }

    /// Detach and return the last `rows` rows (used to lift a fixed footer out of
    /// the body), shrinking the buffer.
    fn take_tail_rows(&mut self, rows: u32) -> RgbaImage {
        let row_bytes = self.w as usize * 4;
        let start = self.h.saturating_sub(rows) as usize * row_bytes;
        let data = self.data.split_off(start);
        self.h -= rows;
        RgbaImage::from_raw(self.w, rows, data).expect("tail rows are a whole image")
    }

    fn into_image(self) -> RgbaImage {
        RgbaImage::from_raw(self.w, self.h, self.data).expect("stitch buffer is a whole image")
    }
}

/// Quick unit-level sanity helper used by tests.
#[allow(dead_code)]
pub fn stitch_pair(prev: &RgbaImage, next: &RgbaImage) -> Option<RgbaImage> {
    let overlap = find_overlap(prev, next)?;
    let mut out = Stitch::from_image(prev);
    out.push_rows(next, overlap, next.height() - overlap);
    Some(out.into_image())
}

/// Write a long screenshot next to normal captures.
///
/// The name comes from the same template as a normal capture, with `_long`
/// appended so the two are easy to tell apart.
pub fn save(
    image: &RgbaImage,
    dir: &std::path::Path,
    options: &util::SaveOptions,
) -> anyhow::Result<std::path::PathBuf> {
    let mut options = options.clone();
    options.template = format!("{}_long", options.template);
    util::save_image(image, dir, &options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn gradient(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| {
            Rgba([
                (x * 7 % 256) as u8,
                (y * 3 % 256) as u8,
                ((x + y) % 256) as u8,
                255,
            ])
        })
    }

    /// A page that looks like text: lines of dark blocks on white, every line
    /// with its own pattern, so that a shift by one line is detectable and
    /// not merely a vertical alignment.
    fn text_page(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
        let mut y = 4u32;
        let mut line = 0u64;
        while y + 10 < h {
            let mut x = 4u32;
            let mut word = 0u64;
            while x + 8 < w {
                let mut r = line
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(word.wrapping_mul(1442695040888963407))
                    .wrapping_add(0x9E37_79B9_7F4A_7C15);
                r ^= r >> 33;
                r = r.wrapping_mul(0xff51_afd7_ed55_8ccd);
                r ^= r >> 33;
                let len = 8 + (r % 36) as u32;
                let gap = 4 + ((r >> 12) % 10) as u32;
                for yy in y..(y + 10).min(h) {
                    for xx in x..(x + len).min(w) {
                        img.put_pixel(xx, yy, Rgba([25, 25, 25, 255]));
                    }
                }
                x += len + gap;
                word += 1;
            }
            y += 16;
            line += 1;
        }
        img
    }

    /// Sparse content: a few separated lines of differing length on an
    /// otherwise empty page — the shape of a terminal near the end of its
    /// scrollback.
    fn sparse_page(w: u32, h: u32) -> RgbaImage {
        let mut img = RgbaImage::from_pixel(w, h, Rgba([255, 255, 255, 255]));
        let mut y = 30u32;
        let mut i = 0u32;
        while y + 10 < h {
            let len = (40 + (i * 37) % (w - 70)).min(w - 10);
            for yy in y..y + 10 {
                for xx in 10..(10 + len).min(w) {
                    img.put_pixel(xx, yy, Rgba([20, 20, 20, 255]));
                }
            }
            y += 100;
            i += 1;
        }
        img
    }

    #[test]
    fn finds_shifted_overlap() {
        let source = gradient(120, 300);
        let prev = image::imageops::crop_imm(&source, 0, 0, 120, 240).to_image();
        let next = image::imageops::crop_imm(&source, 0, 60, 120, 240).to_image();
        assert_eq!(find_overlap(&prev, &next), Some(180));
    }

    #[test]
    fn stitches_pair() {
        let source = gradient(80, 260);
        let prev = image::imageops::crop_imm(&source, 0, 0, 80, 200).to_image();
        let next = image::imageops::crop_imm(&source, 0, 40, 80, 200).to_image();
        let stitched = stitch_pair(&prev, &next).expect("overlap");
        assert_eq!(stitched.dimensions(), (80, 240));
        assert_eq!(stitched.get_pixel(0, 200), source.get_pixel(0, 200));
    }

    /// Regression test for the long-screenshot "repeated content" bug. The
    /// accumulated `prev` is already much taller than a single captured `next`
    /// frame (exactly the state the worker reaches after a few stitches). The
    /// old `find_overlap` indexed `prev` by the shared `min` height, which for a
    /// tall `prev` pointed at a *middle* row instead of its bottom edge, so the
    /// band never matched and the whole frame was appended — duplicating
    /// content every frame. The fix reads the band from the bottom of `prev`.
    #[test]
    fn finds_overlap_on_tall_accumulated_prev() {
        // A tall column of unique content we "scroll" through.
        let source = gradient(120, 2000);
        // prev = the accumulated output so far: source rows [0, 600).
        let prev = image::imageops::crop_imm(&source, 0, 0, 120, 600).to_image();
        // next = the next captured frame: source rows [420, 660) — its top 180
        // rows must overlap the bottom 180 rows of prev.
        let next = image::imageops::crop_imm(&source, 0, 420, 120, 240).to_image();
        assert_eq!(find_overlap(&prev, &next), Some(180));
    }

    /// End-to-end check that stitching a tall prev with a shifted next appends
    /// only the new rows (no duplication), mirroring finds_overlap_on_tall...
    #[test]
    fn stitches_tall_prev_without_duplication() {
        let source = gradient(120, 2000);
        let prev = image::imageops::crop_imm(&source, 0, 0, 120, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 420, 120, 240).to_image();
        let stitched = stitch_pair(&prev, &next).expect("overlap");
        // 600 accumulated + (240 - 180) new rows = 660.
        assert_eq!(stitched.dimensions(), (120, 660));
        // The seam row equals the corresponding source row, proving the overlap
        // was taken from the bottom of prev rather than a middle row.
        assert_eq!(stitched.get_pixel(0, 600), source.get_pixel(0, 600));
        assert_eq!(stitched.get_pixel(0, 659), source.get_pixel(0, 659));
    }

    /// Regression for the "looks like several screenshots glued together"
    /// report.  The page had moved by only a handful of pixels, so the true
    /// overlap was 592 of 600 rows — 98.7% of the frame.  The old search range
    /// stopped at 90% of the frame, so the right answer was out of reach: the
    /// aligner locked onto a wrong, smaller offset or found nothing at all,
    /// and the worker then pasted a whole duplicate of the screen into the
    /// output.
    #[test]
    fn finds_overlap_when_the_page_moved_only_a_few_pixels() {
        let source = text_page(220, 1400);
        let prev = image::imageops::crop_imm(&source, 0, 0, 220, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 8, 220, 600).to_image();
        assert_eq!(find_overlap(&prev, &next), Some(592));
    }

    /// A window that overlaps the captured region — a console popping up on
    /// top of the app being captured — must not derail the alignment.  The rows
    /// it covers stop agreeing, the rest still vote, and the right offset wins
    /// on the number of matches.
    #[test]
    fn aligns_even_with_a_foreign_window_over_the_region() {
        let source = text_page(220, 1400);
        let prev = image::imageops::crop_imm(&source, 0, 0, 220, 600).to_image();
        let mut next = image::imageops::crop_imm(&source, 0, 8, 220, 600).to_image();
        // A black console window over the top third of the next frame.
        for yy in 0..200 {
            for xx in 60..160 {
                next.put_pixel(xx, yy, Rgba([0, 0, 0, 255]));
            }
        }
        assert_eq!(find_overlap(&prev, &next), Some(592));
    }

    /// A mostly empty page — a terminal that has reached the end of its
    /// scrollback.  Blank rows match at every offset, so they must not decide
    /// the alignment.
    #[test]
    fn aligns_a_page_that_is_mostly_blank() {
        let source = sparse_page(220, 1400);
        let prev = image::imageops::crop_imm(&source, 0, 0, 220, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 24, 220, 600).to_image();
        assert_eq!(find_overlap(&prev, &next), Some(576));
    }

    /// A page that scrolled by more than half a frame still has to line up.
    #[test]
    fn aligns_after_a_large_scroll() {
        let source = text_page(220, 2000);
        let prev = image::imageops::crop_imm(&source, 0, 0, 220, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 430, 220, 600).to_image();
        assert_eq!(find_overlap(&prev, &next), Some(170));
    }

    /// A static page (the user paused to read) must report a full-height
    /// overlap so the worker adds nothing.  Aligning against the tall stitched
    /// output used to fail here — its bottom is not a full frame after a partial
    /// append — and pasted a duplicate band.
    #[test]
    fn static_frame_has_full_overlap_and_adds_nothing() {
        let prev = gradient(120, 600);
        let next = prev.clone();
        assert_eq!(find_overlap(&prev, &next), Some(600));
        let stitched = stitch_pair(&prev, &next).expect("overlap");
        assert_eq!(stitched.dimensions(), (120, 600));
    }

    /// Scrolling back up shows content that is already stitched.  The forward
    /// aligner may still return a weak, coincidental match, so the direction is
    /// decided by comparing the forward and reverse matches by quality.
    #[test]
    fn detects_scrolling_back_up() {
        let source = gradient(120, 1200);
        let prev = image::imageops::crop_imm(&source, 0, 300, 120, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 200, 120, 600).to_image();
        assert!(matches!(classify_scroll(&prev, &next), Scroll::Up));
    }

    /// A genuine downward scroll must still classify as `Down` with the right
    /// overlap — the reverse check must not steal it.
    #[test]
    fn classifies_a_downward_scroll() {
        let source = gradient(120, 1200);
        let prev = image::imageops::crop_imm(&source, 0, 0, 120, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 100, 120, 600).to_image();
        assert!(matches!(classify_scroll(&prev, &next), Scroll::Down(500)));
    }

    /// A sticky header (tab strip / nav bar) is identical in every frame; the
    /// detector must find it so the stitcher can skip it instead of repeating it.
    #[test]
    fn detects_a_fixed_header() {
        let w = 120u32;
        let header_h = 40u32;
        let content = gradient(w, 1200);
        let make = |offset: u32| -> RgbaImage {
            let mut img = RgbaImage::new(w, 600);
            for y in 0..header_h {
                for x in 0..w {
                    img.put_pixel(x, y, Rgba([20, 20, 20, 255]));
                }
            }
            for y in header_h..600 {
                for x in 0..w {
                    img.put_pixel(x, y, *content.get_pixel(x, offset + y - header_h));
                }
            }
            img
        };
        let prev = make(0);
        let next = make(100);
        assert_eq!(fixed_bands(&prev, &next).0, header_h);
    }

    /// A sticky footer (terminal status bar / tmux line) must be detected too.
    #[test]
    fn detects_a_fixed_footer() {
        let w = 120u32;
        let footer_h = 30u32;
        let content = gradient(w, 1200);
        let make = |offset: u32| -> RgbaImage {
            let mut img = RgbaImage::new(w, 600);
            let body_h = 600 - footer_h;
            for y in 0..body_h {
                for x in 0..w {
                    img.put_pixel(x, y, *content.get_pixel(x, offset + y));
                }
            }
            for y in body_h..600 {
                for x in 0..w {
                    img.put_pixel(x, y, Rgba([30, 40, 60, 255]));
                }
            }
            img
        };
        let prev = make(0);
        let next = make(100);
        let (top, bottom) = fixed_bands(&prev, &next);
        assert_eq!(top, 0);
        assert_eq!(bottom, footer_h);
    }

    /// Plain content (no fixed band) must report no header, so nothing is
    /// stripped from ordinary pages.
    #[test]
    fn plain_content_has_no_fixed_header() {
        let source = gradient(120, 1200);
        let prev = image::imageops::crop_imm(&source, 0, 0, 120, 600).to_image();
        let next = image::imageops::crop_imm(&source, 0, 100, 120, 600).to_image();
        assert_eq!(fixed_bands(&prev, &next).0, 0);
    }

    /// Blank rows are identical in every frame but are empty space, not chrome —
    /// a mostly-white page must not be misread as one giant fixed header.
    #[test]
    fn blank_rows_are_not_a_fixed_header() {
        let mut prev = RgbaImage::from_pixel(120, 600, Rgba([255, 255, 255, 255]));
        let mut next = prev.clone();
        for y in 200..260 {
            for x in 10..60 {
                prev.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        for y in 260..320 {
            for x in 10..60 {
                next.put_pixel(x, y, Rgba([0, 0, 0, 255]));
            }
        }
        assert_eq!(fixed_bands(&prev, &next), (0, 0));
    }

    /// Draw a frame whose rightmost two columns are a scrollbar: light track and
    /// a darker thumb at `thumb_start..thumb_start + thumb_len`.
    fn frame_with_scrollbar(w: u32, h: u32, thumb_start: u32, thumb_len: u32) -> RgbaImage {
        let mut img = gradient(w, h);
        for y in 0..h {
            let c = if y >= thumb_start && y < thumb_start + thumb_len {
                Rgba([160, 160, 160, 255])
            } else {
                Rgba([240, 240, 240, 255])
            };
            img.put_pixel(w - 1, y, c);
            img.put_pixel(w - 2, y, c);
        }
        img
    }

    #[test]
    fn scrollbar_at_bottom_detects_bottom_thumb() {
        let img = frame_with_scrollbar(200, 600, 500, 100);
        assert!(scrollbar_at_bottom(&img));
    }

    #[test]
    fn scrollbar_at_bottom_rejects_top_thumb() {
        let img = frame_with_scrollbar(200, 600, 0, 100);
        assert!(!scrollbar_at_bottom(&img));
    }

    #[test]
    fn scrollbar_at_bottom_false_on_plain_content() {
        let img = gradient(200, 600);
        assert!(!scrollbar_at_bottom(&img));
    }

    /// The growable stitch buffer must append exactly the requested rows and be
    /// able to hand back a fixed footer from its tail.
    #[test]
    fn stitch_buffer_appends_and_drops_rows() {
        let a = gradient(120, 100);
        let b = gradient(120, 200);
        let mut s = Stitch::from_image(&a);
        assert_eq!(s.height(), 100);
        s.push_rows(&b, 50, 120);
        assert_eq!(s.height(), 220);
        let img = s.into_image();
        assert_eq!(img.dimensions(), (120, 220));
        assert_eq!(img.get_pixel(0, 100), b.get_pixel(0, 50));
        assert_eq!(img.get_pixel(0, 219), b.get_pixel(0, 169));

        let mut s = Stitch::from_image(&b);
        let tail = s.take_tail_rows(40);
        assert_eq!(tail.dimensions(), (120, 40));
        assert_eq!(s.height(), 160);
        assert_eq!(tail.get_pixel(0, 0), b.get_pixel(0, 160));
    }
}
