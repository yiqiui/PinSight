//! Screen capture primitives built on top of `xcap`.
//!
//! All geometry here is in physical screen pixels. The overlay module is the
//! only place that converts to egui points.

use anyhow::{Context, Result, anyhow};
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::util::PhysRect;

/// Route captures through the GDI (SDR) path while a display is in HDR mode.
/// Flipped by the settings page; on by default because the WGC pipeline is the
/// one that washes out.
static PREFER_SDR_ON_HDR: AtomicBool = AtomicBool::new(true);

/// Apply the `hdr_fix` setting. Called when the config loads and whenever the
/// user toggles the row.
pub fn set_prefer_sdr_on_hdr(prefer: bool) {
    PREFER_SDR_ON_HDR.store(prefer, Ordering::Relaxed);
}

fn prefer_sdr_on_hdr() -> bool {
    PREFER_SDR_ON_HDR.load(Ordering::Relaxed)
}

#[derive(Debug, Clone)]
pub struct MonitorInfo {
    pub id: u32,
    pub name: String,
    pub rect: PhysRect,
    pub scale: f32,
    pub primary: bool,
}

impl MonitorInfo {
    /// Logical position/size as expected by egui's viewport API.
    pub fn logical_rect(&self) -> egui::Rect {
        egui::Rect::from_min_size(
            egui::pos2(
                self.rect.x as f32 / self.scale,
                self.rect.y as f32 / self.scale,
            ),
            egui::vec2(
                self.rect.w as f32 / self.scale,
                self.rect.h as f32 / self.scale,
            ),
        )
    }
}

pub fn monitors() -> Result<Vec<MonitorInfo>> {
    let mut out = Vec::new();
    for monitor in xcap::Monitor::all().context("enumerating monitors")? {
        out.push(MonitorInfo {
            id: monitor.id().unwrap_or_default(),
            name: monitor.name().unwrap_or_else(|_| "Display".into()),
            rect: PhysRect {
                x: monitor.x().unwrap_or(0),
                y: monitor.y().unwrap_or(0),
                w: monitor.width().unwrap_or(0),
                h: monitor.height().unwrap_or(0),
            },
            scale: monitor.scale_factor().unwrap_or(1.0).max(0.5),
            primary: monitor.is_primary().unwrap_or(false),
        });
    }
    if out.is_empty() {
        return Err(anyhow!("no monitors reported by xcap"));
    }
    Ok(out)
}

/// Physical cursor position. `enigo` handles DPI translation for us.
pub fn cursor_position() -> Option<(i32, i32)> {
    use enigo::Mouse;
    let settings = enigo::Settings::default();
    let enigo = enigo::Enigo::new(&settings).ok()?;
    enigo.location().ok()
}

pub fn monitor_at(x: i32, y: i32) -> Result<MonitorInfo> {
    let all = monitors()?;
    if let Some(found) = all.iter().find(|m| m.rect.contains(x, y)) {
        return Ok(found.clone());
    }
    if let Some(primary) = all.iter().find(|m| m.primary) {
        return Ok(primary.clone());
    }
    all.into_iter()
        .next()
        .ok_or_else(|| anyhow!("no monitors available"))
}

pub fn monitor_at_cursor() -> Result<MonitorInfo> {
    let (x, y) = cursor_position().unwrap_or((0, 0));
    monitor_at(x, y)
}

pub fn find_monitor(id: u32) -> Result<MonitorInfo> {
    monitors()?
        .into_iter()
        .find(|m| m.id == id)
        .ok_or_else(|| anyhow!("monitor {id} not found"))
}

/// A capturable top-level window (cross-platform, via `xcap`).
#[derive(Debug, Clone)]
pub struct WindowInfo {
    pub id: u32,
    pub title: String,
    pub app: String,
    pub rect: PhysRect,
}

/// Enumerate capturable top-level windows, biggest first.
///
/// Windows with neither a title nor an app name (tool windows, the desktop) are
/// filtered out, as are tiny ones — a picker full of invisible 1×1 windows is
/// useless. Sorting by area puts the window the user probably wants on top.
pub fn list_windows() -> Vec<WindowInfo> {
    let Ok(windows) = xcap::Window::all() else {
        return Vec::new();
    };
    let mut out: Vec<WindowInfo> = windows
        .into_iter()
        .filter_map(|w| {
            let title = w.title().unwrap_or_default();
            let app = w.app_name().unwrap_or_default();
            if title.trim().is_empty() && app.trim().is_empty() {
                return None;
            }
            let rect = PhysRect {
                x: w.x().ok()?,
                y: w.y().ok()?,
                w: w.width().ok()?,
                h: w.height().ok()?,
            };
            if rect.w < 16 || rect.h < 16 {
                return None;
            }
            Some(WindowInfo {
                id: w.id().ok()?,
                title,
                app,
                rect,
            })
        })
        .collect();
    out.sort_by(|a, b| (b.rect.w * b.rect.h).cmp(&(a.rect.w * a.rect.h)));
    out
}

/// Capture a single window by its `xcap` id.
pub fn grab_window(id: u32) -> Result<RgbaImage> {
    let windows = xcap::Window::all().context("listing windows")?;
    let window = windows
        .into_iter()
        .find(|w| w.id().ok() == Some(id))
        .ok_or_else(|| anyhow!("window {id} not found"))?;
    window.capture_image().context("capturing window")
}

/// Mark a top-level window so the Windows.Graphics.Capture pipeline (what xcap
/// uses on Windows) renders it as transparent — i.e. the window stays visible
/// and interactive on screen but is never captured, exactly like a hardware
/// overlay. This is how the floating HUD pills (recording / scrolling capture)
/// keep themselves out of every screenshot, GIF and recorded frame without the
/// hide-and-show flicker an explicit hide produces.
///
/// `title` is matched against the native window title. Returns `true` when the
/// window was found and the affinity was applied (or was already set). The call
/// is idempotent and cheap, so it is safe to repeat every frame.
#[cfg(windows)]
pub fn set_capture_exclusion(title: &str) -> bool {
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, FindWindowW, GetWindowTextLengthW, GetWindowTextW,
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
    };

    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    // Fast path: exact title match.
    if let Ok(hwnd) = unsafe { FindWindowW(None, windows::core::PCWSTR(wide.as_ptr())) } {
        if !hwnd.is_invalid() {
            // Safe to call repeatedly: the property persists until the window
            // is destroyed or the affinity is changed.
            return unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE).is_ok() };
        }
    }

    // Fallback: walk every top-level window and match by substring. egui's
    // immediate viewports set the title we passed, but this covers any case
    // where the native title differs (a prefix/suffix) or the window class
    // hides it from FindWindowW's exact-name lookup. EnumWindows runs on the
    // calling thread, so a thread-local carries the target and result across
    // the callback.
    thread_local! {
        static TARGET: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
        static FOUND: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    }
    TARGET.with(|t| *t.borrow_mut() = Some(title.to_string()));
    FOUND.with(|f| f.set(false));
    unsafe extern "system" fn cb(hwnd: HWND, _lparam: LPARAM) -> windows::core::BOOL {
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if n > 0 {
                let text = String::from_utf16_lossy(&buf[..n as usize]);
                let target = TARGET.with(|t| t.borrow().clone().unwrap_or_default());
                // The full distinctive title ("PinSight — scrolling capture")
                // can never accidentally match the main window ("PinSight").
                if text.contains(&target) {
                    unsafe {
                        let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
                    }
                    FOUND.with(|f| f.set(true));
                }
            }
        }
        true.into()
    }
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(0));
    }
    FOUND.with(|f| f.get())
}

#[cfg(not(windows))]
pub fn set_capture_exclusion(_title: &str) -> bool {
    // Non-Windows capture backends (X11/XTest, CGDisplay) do not bake in
    // always-on-top windows the way WGC does, so there is nothing to exclude.
    true
}

pub fn grab_monitor(info: &MonitorInfo) -> Result<RgbaImage> {
    // HDR displays: the WGC path returns washed-out frames (scRGB composited,
    // clamped into 8 bits). DWM's SDR compositing that GDI reads is tone
    // mapped, so prefer it while the display reports advanced colour. The GDI
    // blit ignores `WDA_EXCLUDEFROMCAPTURE`, so the floating HUD pills are
    // hidden around the grab here — the same exclusion WDA gives us for free on
    // the WGC path, otherwise a layered window bakes in as a black rectangle.
    if prefer_sdr_on_hdr() && crate::hdr::is_hdr(&info.rect) {
        if let Ok(fixed) = with_hidden_hud_pills(|| grab_monitor_gdi(info)) {
            if !is_black(&fixed) {
                tracing::debug!("HDR display: used the GDI SDR path");
                return Ok(fixed);
            }
        }
    }
    let monitor = xcap::Monitor::all()
        .context("enumerating monitors")?
        .into_iter()
        .find(|m| m.id().map(|id| id == info.id).unwrap_or(false))
        .ok_or_else(|| anyhow!("monitor {} disappeared", info.id))?;
    let image = monitor.capture_image().context("capturing monitor")?;
    Ok(with_black_frame_fallback(
        image,
        || with_hidden_hud_pills(|| grab_monitor_gdi(info)),
    ))
}

/// Paint the current mouse cursor onto a capture.
///
/// `origin` is the physical screen coordinate of the image's top-left pixel.
/// Windows has no "give me the cursor as RGBA" call, and `DrawIconEx` into a
/// 32-bit DIB writes an all-zero alpha channel (icons are drawn with the legacy
/// AND/XOR masks), so the shape is recovered by drawing twice — once on black,
/// once on white — and solving for alpha. That handles monochrome, colour and
/// anti-aliased cursors alike.
#[cfg(windows)]
pub fn composite_cursor(img: &mut RgbaImage, origin: (i32, i32)) {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, HGDIOBJ, SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CURSOR_SHOWING, CURSORINFO, DI_NORMAL, DrawIconEx, GetCursorInfo, GetSystemMetrics,
        SM_CXCURSOR, SM_CYCURSOR,
    };

    unsafe {
        let mut info = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut info).is_err() || info.flags.0 & CURSOR_SHOWING.0 == 0 {
            return;
        }
        let (cw, ch) = (
            GetSystemMetrics(SM_CXCURSOR).max(1),
            GetSystemMetrics(SM_CYCURSOR).max(1),
        );

        // Draw once with a black backdrop and once with white.
        let render = |fill: u32| -> Option<Vec<u8>> {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: cw,
                    biHeight: -ch,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let dc = CreateCompatibleDC(None);
            let bmp = CreateDIBSection(Some(dc), &mut bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .ok()?;
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
                let _ = DeleteDC(dc);
                return None;
            }
            let previous = SelectObject(dc, HGDIOBJ(bmp.0));
            let pixels = std::slice::from_raw_parts_mut(bits as *mut u32, (cw * ch) as usize);
            pixels.fill(fill);
            let _ = DrawIconEx(
                dc,
                0,
                0,
                windows::Win32::UI::WindowsAndMessaging::HICON(info.hCursor.0),
                cw,
                ch,
                0,
                None,
                DI_NORMAL,
            );
            let mut out = vec![0u8; (cw * ch * 4) as usize];
            for (i, px) in pixels.iter().enumerate() {
                out[i * 4] = (*px & 0xFF) as u8;
                out[i * 4 + 1] = ((*px >> 8) & 0xFF) as u8;
                out[i * 4 + 2] = ((*px >> 16) & 0xFF) as u8;
                out[i * 4 + 3] = 255;
            }
            SelectObject(dc, previous);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(dc);
            Some(out)
        };

        let (on_black, on_white) = match (render(0xFF00_0000), render(0xFFFF_FFFF)) {
            (Some(black), Some(white)) => (black, white),
            _ => return,
        };

        let (px, py) = (info.ptScreenPos.x - origin.0, info.ptScreenPos.y - origin.1);
        let (iw, ih) = (img.width() as i32, img.height() as i32);
        for y in 0..ch {
            for x in 0..cw {
                let i = ((y * cw + x) * 4) as usize;
                // alpha = 1 - (white - black); the three channels must agree
                // for a correct composite, so take the strongest difference.
                let d = (0..3)
                    .map(|c| on_white[i + c] as i32 - on_black[i + c] as i32)
                    .max()
                    .unwrap_or(0);
                if d <= 0 {
                    continue;
                }
                let alpha = (255 - d) as u32;
                let (tx, ty) = (px + x, py + y);
                if tx < 0 || ty < 0 || tx >= iw || ty >= ih {
                    continue;
                }
                let src = [
                    (on_black[i] as u32 * 255 / alpha.max(1)).min(255) as u8,
                    (on_black[i + 1] as u32 * 255 / alpha.max(1)).min(255) as u8,
                    (on_black[i + 2] as u32 * 255 / alpha.max(1)).min(255) as u8,
                ];
                let a = alpha as f32 / 255.0;
                let dst = img.get_pixel_mut(tx as u32, ty as u32);
                for c in 0..3 {
                    dst[c] = (src[c] as f32 * a + dst[c] as f32 * (1.0 - a)).round() as u8;
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub fn composite_cursor(_img: &mut RgbaImage, _origin: (i32, i32)) {}

/// True when every sampled pixel of `img` is black — the signature of a failed
/// capture rather than a dark desktop. A genuinely black screen is harmless:
/// the fallback returns the same black image and the caller keeps the first.
pub(crate) fn is_black(img: &RgbaImage) -> bool {
    let mut sampled = false;
    for p in img.pixels().step_by(4093) {
        sampled = true;
        if p[0] != 0 || p[1] != 0 || p[2] != 0 {
            return false;
        }
    }
    sampled
}

/// Find the top-level HUD pill windows that are currently visible, by title.
/// Only windows the user can actually see get toggled, so a pill that is not
/// currently shown (no active capture) is left untouched — and never forced
/// visible by the restore step.
#[cfg(windows)]
fn enum_hud_pill_hwnds() -> Vec<windows::Win32::Foundation::HWND> {
    use windows::Win32::Foundation::LPARAM;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextLengthW, GetWindowTextW, IsWindowVisible,
    };
    let mut found: Vec<windows::Win32::Foundation::HWND> = Vec::new();
    unsafe extern "system" fn cb(
        hwnd: windows::Win32::Foundation::HWND,
        lparam: LPARAM,
    ) -> windows::core::BOOL {
        let found =
            unsafe { &mut *(lparam.0 as *mut Vec<windows::Win32::Foundation::HWND>) };
        if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
            return true.into();
        }
        let len = unsafe { GetWindowTextLengthW(hwnd) };
        if len > 0 {
            let mut buf = vec![0u16; (len + 1) as usize];
            let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
            if n > 0 {
                let text = String::from_utf16_lossy(&buf[..n as usize]);
                if text.contains(crate::hud::RECORD_WINDOW_TITLE)
                    || text.contains(crate::hud::LONG_WINDOW_TITLE)
                {
                    found.push(hwnd);
                }
            }
        }
        true.into()
    }
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut found as *mut _ as isize));
    }
    found
}

/// Run `f` with the floating HUD pill windows hidden, then restore them. This
/// is the GDI equivalent of `set_capture_exclusion`: `WDA_EXCLUDEFROMCAPTURE`
/// is a Windows.Graphics.Capture / PrintScreen concept and is ignored by a raw
/// `BitBlt`, which is what the GDI fallback uses — and a `BitBlt` cannot
/// composite a layered (transparent) window, so it lands as a black rectangle.
/// Hiding the window for the duration of the blit keeps it out of the capture
/// without the hide/show flicker that would otherwise affect the WGC path (where
/// WDA already handles it).
#[cfg(windows)]
fn with_hidden_hud_pills<R>(f: impl FnOnce() -> R) -> R {
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNA};
    let hwnds = enum_hud_pill_hwnds();
    if hwnds.is_empty() {
        return f();
    }
    for hwnd in &hwnds {
        unsafe {
            let _ = ShowWindow(*hwnd, SW_HIDE);
        }
    }
    // `SW_HIDE` flips the window's own visibility flag immediately, but DWM
    // needs at least one composition (~16 ms) to drop the window from the
    // desktop frame that `BitBlt` reads. Grabbing too early keeps the layered
    // (transparent) pill baked in as a *black* rectangle — GDI cannot
    // composite a transparent window, so it fills the alpha with 0,0,0,0 and the
    // DIB surfaces as pure black. We therefore wait out a couple of frames
    // instead of trusting `IsWindowVisible` (which is already false at this
    // point).
    //
    // The hide is re-asserted every few milliseconds because egui's viewport
    // repaint loop (`show_viewport_immediate`) re-shows the pill on its own
    // schedule; without the re-assert the pill can pop back into a later grab.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(70);
    std::thread::sleep(std::time::Duration::from_millis(20));
    while std::time::Instant::now() < deadline {
        for hwnd in &hwnds {
            unsafe {
                let _ = ShowWindow(*hwnd, SW_HIDE);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(8));
    }
    let r = f();
    for hwnd in &hwnds {
        unsafe {
            let _ = ShowWindow(*hwnd, SW_SHOWNA);
        }
    }
    r
}

#[cfg(not(windows))]
fn with_hidden_hud_pills<R>(f: impl FnOnce() -> R) -> R {
    // Off Windows the capture backends (X11 / CGDisplay) grab the framebuffer
    // directly and do not composite an always-on-top window the way a Win32
    // layered window is baked in, so there is nothing to hide around the grab.
    f()
}

/// Capture a monitor region through the WGC backend.
///
/// With the `wgc` feature enabled, `xcap`'s `capture_region` runs on
/// Windows.Graphics.Capture, which honours `WDA_EXCLUDEFROMCAPTURE`: the
/// floating HUD pills are marked excluded (`set_capture_exclusion`, re-asserted
/// every UI frame and once by the job worker) and WGC drops them from the frame
/// entirely — no black rectangle and no hide/show flicker. The GDI fallback
/// (HDR displays, WGC black-frame retry) still uses `with_hidden_hud_pills`,
/// because GDI ignores display affinity and cannot composite a layered window.
fn grab_region_excluding_pills(
    monitor: &xcap::Monitor,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
) -> Result<RgbaImage> {
    monitor
        .capture_region(rx, ry, rw, rh)
        .context("capturing monitor region")
}

/// Retry a black frame with `gdi` and keep whichever result has content.
fn with_black_frame_fallback(
    image: RgbaImage,
    gdi: impl FnOnce() -> Result<RgbaImage>,
) -> RgbaImage {
    if !is_black(&image) {
        return image;
    }
    match gdi() {
        Ok(fixed) if !is_black(&fixed) => {
            tracing::debug!("capture came back black; used the GDI fallback");
            fixed
        }
        Ok(_) => image,
        Err(err) => {
            tracing::debug!(%err, "GDI capture fallback failed");
            image
        }
    }
}

/// Blit the whole virtual screen with GDI and return it with its origin.
///
/// Shared by the monitor and rectangle fallbacks so the Win32 plumbing exists
/// once. Returns `(origin_x, origin_y, image)`.
#[cfg(windows)]
fn grab_virtual_screen_gdi() -> Result<(i32, i32, RgbaImage)> {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC,
        DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HDC, HGDIOBJ, ReleaseDC, SRCCOPY,
        SelectObject,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
        SM_YVIRTUALSCREEN,
    };

    unsafe {
        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN).max(1);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN).max(1);

        let screen: HDC = GetDC(None);
        if screen.is_invalid() {
            return Err(anyhow!("GetDC(NULL) failed"));
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, vw, vh);
        if mem.is_invalid() || bmp.is_invalid() {
            if !bmp.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
            }
            if !mem.is_invalid() {
                let _ = DeleteDC(mem);
            }
            let _ = ReleaseDC(None, screen);
            return Err(anyhow!("could not allocate a GDI capture surface"));
        }
        let previous = SelectObject(mem, HGDIOBJ(bmp.0));

        // The screen DC's origin is the primary monitor, so the virtual-screen
        // offset has to be subtracted to reach the requested region.
        if BitBlt(mem, 0, 0, vw, vh, Some(screen), -vx, -vy, SRCCOPY).is_err() {
            SelectObject(mem, previous);
            let _ = DeleteObject(HGDIOBJ(bmp.0));
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return Err(anyhow!("BitBlt from the screen DC failed"));
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: vw,
                biHeight: -vh, // negative: top-down rows, no flipping needed
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            bmiColors: [Default::default(); 1],
        };
        let mut info = info;
        let mut buf = vec![0u8; (vw * vh * 4) as usize];
        let lines = GetDIBits(
            mem,
            bmp,
            0,
            vh as u32,
            Some(buf.as_mut_ptr() as *mut _),
            &mut info,
            DIB_RGB_COLORS,
        );

        SelectObject(mem, previous);
        let _ = DeleteObject(HGDIOBJ(bmp.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);
        if lines == 0 {
            return Err(anyhow!("GetDIBits returned no rows"));
        }

        // 32bpp BI_RGB comes back as BGRA; the image crate wants RGBA.
        let mut out = RgbaImage::new(vw as u32, vh as u32);
        for (i, px) in out.pixels_mut().enumerate() {
            let c = &buf[i * 4..i * 4 + 4];
            *px = image::Rgba([c[2], c[1], c[0], 255]);
        }
        Ok((vx, vy, out))
    }
}

/// Crop `(origin, screen)` to `rect`, clamped to what the screen covers.
#[cfg(windows)]
fn crop_virtual_screen(origin: (i32, i32), screen: &RgbaImage, rect: PhysRect) -> RgbaImage {
    let (vx, vy) = origin;
    let sw = screen.width() as i32;
    let sh = screen.height() as i32;
    let x0 = (rect.x - vx).clamp(0, sw) as usize;
    let y0 = (rect.y - vy).clamp(0, sh) as usize;
    let x1 = (rect.x + rect.w as i32 - vx).clamp(0, sw) as usize;
    let y1 = (rect.y + rect.h as i32 - vy).clamp(0, sh) as usize;
    if x1 <= x0 || y1 <= y0 {
        return RgbaImage::new(rect.w.max(1), rect.h.max(1));
    }
    let mut out = RgbaImage::new((x1 - x0) as u32, (y1 - y0) as u32);
    for row in y0..y1 {
        let src = (row * sw as usize + x0) * 4;
        let dst = ((row - y0) * (x1 - x0)) * 4;
        out.as_mut()[dst..dst + (x1 - x0) * 4]
            .copy_from_slice(&screen.as_raw()[src..src + (x1 - x0) * 4]);
    }
    out
}

/// Capture one monitor with a plain GDI screen blit.
///
/// This exists because xcap's Windows backend is Windows.Graphics.Capture,
/// which returns an all-black frame on machines where that pipeline is
/// unavailable (some remote sessions and driver states). A `BitBlt` from the
/// screen DC always works on an interactive desktop — it just cannot see
/// secure desktops (UAC prompts, the lock screen), which is why it is the
/// fallback and not the primary path.
#[cfg(windows)]
fn grab_monitor_gdi(info: &MonitorInfo) -> Result<RgbaImage> {
    let (vx, vy, screen) = grab_virtual_screen_gdi()?;
    Ok(crop_virtual_screen((vx, vy), &screen, info.rect))
}

/// Capture a physical desktop rectangle with GDI, for the region fallback.
#[cfg(windows)]
fn grab_rect_gdi(rect: PhysRect) -> Result<RgbaImage> {
    let (vx, vy, screen) = grab_virtual_screen_gdi()?;
    Ok(crop_virtual_screen((vx, vy), &screen, rect))
}

#[cfg(not(windows))]
fn grab_monitor_gdi(_info: &MonitorInfo) -> Result<RgbaImage> {
    Err(anyhow!("the GDI fallback is Windows-only"))
}

#[cfg(not(windows))]
fn grab_rect_gdi(_rect: PhysRect) -> Result<RgbaImage> {
    Err(anyhow!("the GDI fallback is Windows-only"))
}

pub fn grab_full_screen() -> Result<(MonitorInfo, RgbaImage)> {
    let info = monitor_at_cursor()?;
    let image = grab_monitor(&info)?;
    Ok((info, image))
}

/// Capture a physical rectangle of the virtual desktop. The rectangle is
/// intersected with each monitor; in practice screenshots are contained in one
/// monitor, but handling the union keeps the API predictable.
pub fn grab_physical_rect(rect: PhysRect) -> Result<RgbaImage> {
    let mut out = RgbaImage::new(rect.w.max(1), rect.h.max(1));
    if rect.w == 0 || rect.h == 0 {
        return Ok(out);
    }

    // Same HDR reasoning as `grab_monitor`: read the tone-mapped SDR
    // compositing instead of the clamped WGC frame. The GDI blit used here
    // ignores `WDA_EXCLUDEFROMCAPTURE`, so the progress pill would otherwise
    // bake in as a black rectangle — hide it around the grab (see
    // `with_hidden_hud_pills`).
    if prefer_sdr_on_hdr() && crate::hdr::overlaps_hdr(&rect) {
        if let Ok(fixed) = with_hidden_hud_pills(|| grab_rect_gdi(rect)) {
            if !is_black(&fixed) {
                return Ok(fixed);
            }
        }
    }

    // Background: transparent black. For a single-monitor region this is fully
    // covered by the monitor capture.
    let infos = monitors()?;
    let native = xcap::Monitor::all().context("enumerating monitors")?;
    for info in infos {
        let ix0 = rect.x.max(info.rect.x);
        let iy0 = rect.y.max(info.rect.y);
        let ix1 = (rect.x + rect.w as i32).min(info.rect.x + info.rect.w as i32);
        let iy1 = (rect.y + rect.h as i32).min(info.rect.y + info.rect.h as i32);
        if ix1 <= ix0 || iy1 <= iy0 {
            continue;
        }
        let Some(monitor) = native
            .iter()
            .find(|m| m.id().map(|id| id == info.id).unwrap_or(false))
        else {
            continue;
        };
        let rx = (ix0 - info.rect.x) as u32;
        let ry = (iy0 - info.rect.y) as u32;
        let rw = (ix1 - ix0) as u32;
        let rh = (iy1 - iy0) as u32;
        let part = grab_region_excluding_pills(monitor, rx, ry, rw, rh)?;
        image::imageops::overlay(
            &mut out,
            &part,
            (ix0 - rect.x) as i64,
            (iy0 - rect.y) as i64,
        );
    }
    Ok(with_black_frame_fallback(
        out,
        || with_hidden_hud_pills(|| grab_rect_gdi(rect)),
    ))
}
