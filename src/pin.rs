//! Floating pinned screenshots.
//!
//! On Windows pins are real native popup windows painted with GDI. This keeps
//! them genuinely topmost, borderless, draggable and click-through without
//! relying on egui's child viewports (which are optimised for transient
//! windows). Each pin owns a small message-loop thread.

use std::sync::Arc;

use image::RgbaImage;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinEvent {
    /// The user asked to copy the pin's text (a configured mouse gesture).
    CopyText(u64),
    Close(u64),
    /// Right-click → Copy image: handled on the UI thread (clipboard).
    Copy(u64),
    /// Right-click → Save as…: handled on the UI thread (file dialog).
    SaveAs(u64),
}

#[cfg(windows)]
#[allow(unsafe_op_in_unsafe_fn)]
mod imp {
    use std::ffi::c_void;
    use std::sync::Arc;
    use std::sync::mpsc::Sender;
    use std::time::Duration;

    use image::RgbaImage;
    use windows::Win32::Foundation::{
        COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
    };
    use windows::Win32::Graphics::Gdi::{
        AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, BeginPaint,
        CreateCompatibleDC, CreateDIBSection, CreateSolidBrush, DIB_RGB_COLORS, DeleteDC,
        DeleteObject, EndPaint, FrameRect, GetDC, HBRUSH, HGDIOBJ, InvalidateRect, PAINTSTRUCT,
        RGBQUAD, ReleaseDC, SRCCOPY, SelectObject, StretchDIBits,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetKeyState, ReleaseCapture, VK_CONTROL, VK_ESCAPE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CREATESTRUCTW, CS_HREDRAW, CS_VREDRAW, CreatePopupMenu, CreateWindowExW,
        DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW, GWL_EXSTYLE, GWLP_USERDATA,
        GetClientRect, GetCursorPos, GetMessageW, GetWindowLongPtrW, GetWindowRect, HTCAPTION,
        HWND_NOTOPMOST, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, LoadCursorW, MENU_ITEM_FLAGS,
        MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, PostMessageW,
        PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetLayeredWindowAttributes,
        SetWindowLongPtrW, SetWindowPos, ShowWindow, TPM_LEFTALIGN, TPM_RETURNCMD, TPM_RIGHTBUTTON,
        TrackPopupMenu, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WM_APP, WM_CLOSE,
        WM_COMMAND, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
        WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MOUSEWHEEL, WM_NCCREATE, WM_NCLBUTTONDOWN, WM_PAINT,
        WM_RBUTTONDBLCLK, WM_RBUTTONUP, WNDCLASSW, WS_EX_LAYERED, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
        WS_EX_TRANSPARENT, WS_POPUP, WS_VISIBLE,
    };
    use windows::core::{HSTRING, PCWSTR};

    use super::PinEvent;

    const CLASS_NAME: &str = "PinSightNativePin";
    const WM_TOGGLE_PASS: u32 = WM_APP + 1;
    /// Show or hide the pin window. The parameter carries 1 for show, 0 for hide.
    const WM_SET_VISIBLE: u32 = WM_APP + 2;

    // Right-click menu command ids. WM_APP-based so they never collide with
    // the system's reserved WM_COMMAND range.
    const IDM_COPY: usize = WM_APP as usize + 100;
    const IDM_SAVE_AS: usize = WM_APP as usize + 101;
    const IDM_ZOOM_25: usize = WM_APP as usize + 102;
    const IDM_ZOOM_50: usize = WM_APP as usize + 103;
    const IDM_ZOOM_100: usize = WM_APP as usize + 104;
    const IDM_ZOOM_200: usize = WM_APP as usize + 105;
    const IDM_ON_TOP: usize = WM_APP as usize + 106;
    const IDM_THROUGH: usize = WM_APP as usize + 107;
    const IDM_CLOSE: usize = WM_APP as usize + 108;
    const IDM_DESTROY: usize = WM_APP as usize + 109;

    struct PinCtx {
        id: u64,
        width: u32,
        height: u32,
        bgra: Vec<u8>,
        zoom: f32,
        opacity: f32,
        /// Menu labels in the current UI language.
        tr: &'static crate::i18n::Strings,
        /// Soft drop shadow around the image (settings → pins).
        shadow: bool,
        /// RGB of the drop shadow.
        shadow_color: [u8; 3],
        /// Whether the wheel zooms the pin (settings → pins → behaviour).
        wheel_zoom: bool,
        /// Keep the point under the cursor fixed while zooming.
        zoom_at_cursor: bool,
        /// Use finer zoom steps.
        smooth_zoom: bool,
        /// Whether a middle click closes the pin.
        middle_close: bool,
        /// Encoded mouse bindings (settings → controls → mouse).
        gestures: [u32; 6],
        /// True once `UpdateLayeredWindow` has taken over the painting.
        layered: bool,
        on_top: bool,
        through: bool,
        tx: Sender<PinEvent>,
    }

    pub struct NativePin {
        pub hwnd: isize,
    }

    /// `tr` supplies the right-click menu labels in the current UI language.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: u64,
        image: Arc<RgbaImage>,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        tr: &'static crate::i18n::Strings,
        opacity: u8,
        shadow: bool,
        shadow_color: [u8; 3],
        wheel_zoom: bool,
        zoom_at_cursor: bool,
        smooth_zoom: bool,
        middle_close: bool,
        gestures: [u32; 6],
        tx: Sender<PinEvent>,
    ) -> Result<NativePin, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<isize>();
        std::thread::Builder::new()
            .name(format!("pinsight-pin-{id}"))
            .spawn(move || {
                run(
                    id,
                    image,
                    x,
                    y,
                    width,
                    height,
                    tr,
                    opacity,
                    shadow,
                    shadow_color,
                    wheel_zoom,
                    zoom_at_cursor,
                    smooth_zoom,
                    middle_close,
                    gestures,
                    tx,
                    ready_tx,
                )
            })
            .map_err(|e| e.to_string())?;
        match ready_rx.recv_timeout(Duration::from_secs(3)) {
            Ok(0) | Err(_) => Err("could not create the pin window".into()),
            Ok(hwnd) => Ok(NativePin { hwnd }),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn run(
        id: u64,
        image: Arc<RgbaImage>,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        tr: &'static crate::i18n::Strings,
        opacity: u8,
        shadow: bool,
        shadow_color: [u8; 3],
        wheel_zoom: bool,
        zoom_at_cursor: bool,
        smooth_zoom: bool,
        middle_close: bool,
        gestures: [u32; 6],
        tx: Sender<PinEvent>,
        ready_tx: Sender<isize>,
    ) {
        let mut bgra = Vec::with_capacity((width * height * 4) as usize);
        for px in image.pixels() {
            bgra.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
        let ctx = Box::new(PinCtx {
            id,
            width,
            height,
            bgra,
            zoom: 1.0,
            opacity: (opacity.clamp(20, 100) as f32) / 100.0,
            tr,
            shadow,
            shadow_color,
            wheel_zoom,
            zoom_at_cursor,
            smooth_zoom,
            middle_close,
            gestures,
            layered: false,
            on_top: true,
            through: false,
            tx,
        });
        let ctx_ptr = Box::into_raw(ctx);

        unsafe {
            let hinstance = GetModuleHandleW(None).unwrap_or_default();
            let class = HSTRING::from(CLASS_NAME);
            let title = HSTRING::from(format!("PinSight pin #{id}"));
            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(wndproc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: hinstance.into(),
                hIcon: Default::default(),
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH::default(),
                lpszMenuName: PCWSTR::null(),
                lpszClassName: PCWSTR(class.as_ptr()),
            };
            // Another pin thread may have registered it already; that's fine.
            let _ = RegisterClassW(&wc);

            // Leave room for the shadow margin so the image keeps its size.
            let margin = if shadow { SHADOW_MARGIN * 2 } else { 0 };
            let hwnd = match CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP | WS_VISIBLE,
                x - margin / 2,
                y - margin / 2,
                width as i32 + margin,
                height as i32 + margin,
                None,
                None,
                Some(HINSTANCE(hinstance.0)),
                Some(ctx_ptr as *const c_void),
            ) {
                Ok(hwnd) => hwnd,
                Err(_) => {
                    drop(Box::from_raw(ctx_ptr));
                    let _ = ready_tx.send(0);
                    return;
                }
            };

            // Per-pixel alpha via UpdateLayeredWindow: it is the only way to
            // draw a soft shadow (a uniform window alpha would paint the
            // shadow ring as solid black). Falls back to the flat alpha when
            // the call fails, so the pin still shows up.
            let layered = apply_layered(hwnd, &*ctx_ptr);
            if !layered {
                let alpha = ((*ctx_ptr).opacity * 255.0).round().clamp(0.0, 255.0) as u8;
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), alpha, LWA_ALPHA);
            }
            (*ctx_ptr).layered = layered;
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = ready_tx.send(hwnd.0 as isize);

            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// Margin reserved around the image for the drop shadow, in pixels.
    const SHADOW_MARGIN: i32 = 10;

    /// Rebuild the window content with per-pixel alpha: the (zoomed) image plus
    /// an optional soft shadow, submitted through `UpdateLayeredWindow`.
    ///
    /// Returns false when the DIB or the update failed — the caller then falls
    /// back to a uniform window alpha so the pin is still visible.
    unsafe fn apply_layered(hwnd: HWND, ctx: &PinCtx) -> bool {
        let margin = if ctx.shadow { SHADOW_MARGIN } else { 0 };
        let zoom = ctx.zoom.clamp(0.1, 4.0);
        let iw = (ctx.width as f32 * zoom).round().max(1.0) as i32;
        let ih = (ctx.height as f32 * zoom).round().max(1.0) as i32;
        let w = iw + margin * 2;
        let h = ih + margin * 2;
        if w <= 0 || h <= 0 {
            return false;
        }

        let screen = GetDC(None);
        if screen.is_invalid() {
            return false;
        }
        let mem = CreateCompatibleDC(Some(screen));
        if mem.is_invalid() {
            let _ = ReleaseDC(None, screen);
            return false;
        }

        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(Some(screen), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        else {
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return false;
        };
        if bits.is_null() {
            let _ = DeleteObject(HGDIOBJ(bitmap.0));
            let _ = DeleteDC(mem);
            let _ = ReleaseDC(None, screen);
            return false;
        }
        let previous = SelectObject(mem, HGDIOBJ(bitmap.0));

        // The DIB is premultiplied BGRA, cleared to fully transparent.
        let px = bits as *mut u8;
        let len = (w * h * 4) as usize;
        std::ptr::write_bytes(px, 0, len);
        let opacity = ctx.opacity.clamp(0.0, 1.0);

        // 1) shadow: a black ring outside the image with a quadratic falloff.
        if margin > 0 {
            let m = margin as f32;
            for y in 0..h {
                for x in 0..w {
                    let dx = if x < margin {
                        margin - x
                    } else if x >= margin + iw {
                        x - (margin + iw) + 1
                    } else {
                        0
                    };
                    let dy = if y < margin {
                        margin - y
                    } else if y >= margin + ih {
                        y - (margin + ih) + 1
                    } else {
                        0
                    };
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let dist = ((dx * dx + dy * dy) as f32).sqrt();
                    if dist > m {
                        continue;
                    }
                    let alpha = (1.0 - dist / m).powf(1.7) * 0.55 * opacity;
                    let index = ((y * w + x) * 4) as usize;
                    // Premultiplied: the RGB channels carry the shadow colour
                    // scaled by the ring's alpha, so a tinted shadow works.
                    let [sr, sg, sb] = ctx.shadow_color;
                    *px.add(index) = (sr as f32 * alpha).round() as u8;
                    *px.add(index + 1) = (sg as f32 * alpha).round() as u8;
                    *px.add(index + 2) = (sb as f32 * alpha).round() as u8;
                    *px.add(index + 3) = (alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }

        // 2) the image itself, nearest-neighbour scaled into the margin inset.
        let sw = ctx.width.max(1) as f32;
        let sh = ctx.height.max(1) as f32;
        for y in 0..ih {
            let sy = ((y as f32 + 0.5) / zoom).min(sh - 1.0).max(0.0) as u32;
            for x in 0..iw {
                let sx = ((x as f32 + 0.5) / zoom).min(sw - 1.0).max(0.0) as u32;
                let si = ((sy * ctx.width + sx) * 4) as usize;
                let b = *ctx.bgra.get(si).unwrap_or(&0) as f32;
                let g = *ctx.bgra.get(si + 1).unwrap_or(&0) as f32;
                let r = *ctx.bgra.get(si + 2).unwrap_or(&0) as f32;
                let a = *ctx.bgra.get(si + 3).unwrap_or(&255) as f32;
                let af = (a / 255.0) * opacity;
                let di = (((y + margin) * w + (x + margin)) * 4) as usize;
                *px.add(di) = (b * af).round() as u8;
                *px.add(di + 1) = (g * af).round() as u8;
                *px.add(di + 2) = (r * af).round() as u8;
                *px.add(di + 3) = (af * 255.0).round() as u8;
            }
        }

        // 3) submit the whole surface in one call.
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        let dst = windows::Win32::Foundation::POINT {
            x: rect.left,
            y: rect.top,
        };
        let size = windows::Win32::Foundation::SIZE { cx: w, cy: h };
        let src_point = windows::Win32::Foundation::POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let ok = UpdateLayeredWindow(
            hwnd,
            Some(screen),
            Some(&dst),
            Some(&size),
            Some(mem),
            Some(&src_point),
            COLORREF(0),
            Some(&blend),
            ULW_ALPHA,
        )
        .is_ok();

        SelectObject(mem, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(mem);
        let _ = ReleaseDC(None, screen);
        ok
    }

    /// Resize the window for the current zoom (the shadow margin is included).
    unsafe fn resize_for_zoom(hwnd: HWND, ctx: &PinCtx) {
        let margin = if ctx.shadow { SHADOW_MARGIN } else { 0 };
        let w = (ctx.width as f32 * ctx.zoom).round().max(24.0) as i32 + margin * 2;
        let h = (ctx.height as f32 * ctx.zoom).round().max(24.0) as i32 + margin * 2;
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            w,
            h,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }

    /// Zoom while keeping the image point under the cursor in place.
    ///
    /// The window grows/shrinks around the cursor instead of its top-left
    /// corner, which is what makes wheel zoom feel like a magnifier.
    unsafe fn zoom_at_cursor(hwnd: HWND, ctx: &PinCtx, old: f32, new: f32) {
        let margin = if ctx.shadow { SHADOW_MARGIN } else { 0 };
        let mut pt = POINT::default();
        let mut rect = RECT::default();
        if GetCursorPos(&mut pt).is_err() || GetWindowRect(hwnd, &mut rect).is_err() {
            resize_for_zoom(hwnd, ctx);
            return;
        }
        // Pixel under the cursor, in source-image coordinates.
        let local_x = (pt.x - rect.left - margin) as f32;
        let local_y = (pt.y - rect.top - margin) as f32;
        let img_x = local_x / old.max(0.01);
        let img_y = local_y / old.max(0.01);

        let w = (ctx.width as f32 * new).round().max(24.0) as i32 + margin * 2;
        let h = (ctx.height as f32 * new).round().max(24.0) as i32 + margin * 2;
        let x = pt.x - margin - (img_x * new).round() as i32;
        let y = pt.y - margin - (img_y * new).round() as i32;
        let _ = SetWindowPos(hwnd, None, x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }

    /// Rotate the image 90° in place, swapping the stored dimensions.
    fn rotate_bgra(ctx: &mut PinCtx, clockwise: bool) {
        let w = ctx.width as usize;
        let h = ctx.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let src = (y * w + x) * 4;
                let (dx, dy) = if clockwise {
                    (h - 1 - y, x)
                } else {
                    (y, w - 1 - x)
                };
                // After a 90° turn the row length is the old height.
                let dst = (dy * h + dx) * 4;
                out[dst..dst + 4].copy_from_slice(&ctx.bgra[src..src + 4]);
            }
        }
        ctx.bgra = out;
        ctx.width = h as u32;
        ctx.height = w as u32;
    }

    /// Mirror the image horizontally or vertically, in place.
    fn flip_bgra(ctx: &mut PinCtx, horizontal: bool) {
        let w = ctx.width as usize;
        let h = ctx.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        let mut out = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let src = (y * w + x) * 4;
                let (dx, dy) = if horizontal {
                    (w - 1 - x, y)
                } else {
                    (x, h - 1 - y)
                };
                let dst = (dy * w + dx) * 4;
                out[dst..dst + 4].copy_from_slice(&ctx.bgra[src..src + 4]);
            }
        }
        ctx.bgra = out;
    }

    unsafe fn ctx_from(hwnd: HWND) -> Option<&'static mut PinCtx> {
        unsafe {
            let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PinCtx;
            ptr.as_mut()
        }
    }

    /// Right-click menu, modelled on Snipaste's pin menu: copy, save as, zoom
    /// presets, the two window toggles, close/destroy, and the live pixel size.
    unsafe fn show_menu(hwnd: HWND, ctx: &mut PinCtx) {
        let Ok(menu) = CreatePopupMenu() else {
            return;
        };
        let t = ctx.tr;
        let flags = |extra: MENU_ITEM_FLAGS| MENU_ITEM_FLAGS(MF_STRING.0 | extra.0);

        let _ = AppendMenuW(
            menu,
            flags(MENU_ITEM_FLAGS(0)),
            IDM_COPY,
            &HSTRING::from(t.pin_menu_copy),
        );
        let _ = AppendMenuW(
            menu,
            flags(MENU_ITEM_FLAGS(0)),
            IDM_SAVE_AS,
            &HSTRING::from(t.pin_menu_save_as),
        );
        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_SEPARATOR.0), 0, &HSTRING::new());

        // Zoom presets as a submenu.
        let Ok(zoom_menu) = CreatePopupMenu() else {
            let _ = DestroyMenu(menu);
            return;
        };
        for (id, label, zoom) in [
            (IDM_ZOOM_25, "25%", 0.25_f32),
            (IDM_ZOOM_50, "50%", 0.5),
            (IDM_ZOOM_100, "100%", 1.0),
            (IDM_ZOOM_200, "200%", 2.0),
        ] {
            let mut f = MF_STRING.0;
            if (ctx.zoom - zoom).abs() < 0.01 {
                f |= MF_CHECKED.0;
            }
            let _ = AppendMenuW(zoom_menu, MENU_ITEM_FLAGS(f), id, &HSTRING::from(label));
        }
        let _ = AppendMenuW(
            menu,
            MENU_ITEM_FLAGS(MF_POPUP.0),
            zoom_menu.0 as usize,
            &HSTRING::from(t.pin_menu_zoom),
        );

        let on_top_flags = if ctx.on_top { MF_CHECKED.0 } else { 0 };
        let _ = AppendMenuW(
            menu,
            MENU_ITEM_FLAGS(MF_STRING.0 | on_top_flags),
            IDM_ON_TOP,
            &HSTRING::from(t.pin_menu_ontop),
        );
        let through_flags = if ctx.through { MF_CHECKED.0 } else { 0 };
        let _ = AppendMenuW(
            menu,
            MENU_ITEM_FLAGS(MF_STRING.0 | through_flags),
            IDM_THROUGH,
            &HSTRING::from(t.pin_menu_through),
        );

        let _ = AppendMenuW(menu, MENU_ITEM_FLAGS(MF_SEPARATOR.0), 0, &HSTRING::new());
        let _ = AppendMenuW(
            menu,
            flags(MENU_ITEM_FLAGS(0)),
            IDM_CLOSE,
            &HSTRING::from(t.pin_menu_close),
        );
        let _ = AppendMenuW(
            menu,
            flags(MENU_ITEM_FLAGS(0)),
            IDM_DESTROY,
            &HSTRING::from(t.pin_menu_destroy),
        );

        // Live pixel size, greyed out like Snipaste's footer row.
        let pw = (ctx.width as f32 * ctx.zoom).round() as u32;
        let ph = (ctx.height as f32 * ctx.zoom).round() as u32;
        let _ = AppendMenuW(
            menu,
            MENU_ITEM_FLAGS(MF_STRING.0 | MF_GRAYED.0),
            0,
            &HSTRING::from(format!("{pw} × {ph}")),
        );

        let mut point = windows::Win32::Foundation::POINT::default();
        let _ = GetCursorPos(&mut point);
        // Without this the menu can stay open when it loses focus.
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(
            menu,
            TPM_LEFTALIGN | TPM_RETURNCMD | TPM_RIGHTBUTTON,
            point.x,
            point.y,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(zoom_menu);
        let _ = DestroyMenu(menu);
        if picked.0 != 0 {
            run_menu_command(hwnd, ctx, picked.0 as usize);
        }
    }

    unsafe fn run_menu_command(hwnd: HWND, ctx: &mut PinCtx, cmd: usize) {
        match cmd {
            IDM_COPY => {
                let _ = ctx.tx.send(PinEvent::Copy(ctx.id));
            }
            IDM_SAVE_AS => {
                let _ = ctx.tx.send(PinEvent::SaveAs(ctx.id));
            }
            IDM_ZOOM_25 | IDM_ZOOM_50 | IDM_ZOOM_100 | IDM_ZOOM_200 => {
                let zoom = match cmd {
                    IDM_ZOOM_25 => 0.25,
                    IDM_ZOOM_50 => 0.5,
                    IDM_ZOOM_200 => 2.0,
                    _ => 1.0,
                };
                ctx.zoom = zoom;
                resize_for_zoom(hwnd, ctx);
                if ctx.layered {
                    let _ = apply_layered(hwnd, ctx);
                } else {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            IDM_ON_TOP => {
                ctx.on_top = !ctx.on_top;
                let insert = if ctx.on_top {
                    HWND_TOPMOST
                } else {
                    HWND_NOTOPMOST
                };
                let _ = SetWindowPos(
                    hwnd,
                    Some(insert),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            IDM_THROUGH => {
                ctx.through = !ctx.through;
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                let new_ex = if ctx.through {
                    ex | WS_EX_TRANSPARENT.0
                } else {
                    ex & !WS_EX_TRANSPARENT.0
                };
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
            }
            IDM_CLOSE | IDM_DESTROY => {
                let _ = DestroyWindow(hwnd);
            }
            _ => {}
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        /// What a configured mouse gesture does to a pin.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        enum PinAction {
            Zoom,
            Opacity,
            Close,
            Reset,
            Thumbnail,
            CopyText,
        }

        /// Match a mouse event against the configured bindings.
        ///
        /// `button` is 0 = wheel, 1 = left, 2 = middle, 3 = right. `double_click`
        /// selects the double-click variant. Returns `None` when nothing matches, so
        /// the caller can fall back to its legacy behaviour.
        fn pin_gesture_action(
            ctx: &PinCtx,
            modifier: u32,
            button: u32,
            double_click: bool,
        ) -> Option<PinAction> {
            const ACTIONS: [PinAction; 6] = [
                PinAction::Zoom,
                PinAction::Opacity,
                PinAction::Close,
                PinAction::Reset,
                PinAction::Thumbnail,
                PinAction::CopyText,
            ];
            let keys = [(VK_CONTROL.0 as i32, 1u32), (0x10, 2), (0x12, 3)];
            let held = |mask: u32| -> bool {
                if mask == 0 {
                    return true;
                }
                keys.iter()
                    .filter(|(_, m)| *m == mask)
                    .all(|(vk, _)| unsafe { GetKeyState(*vk) } < 0)
            };
            for (index, encoded) in ctx.gestures.iter().enumerate() {
                let gesture = crate::config::Gesture::decode(*encoded);
                let want_mod = match gesture.modifier {
                    crate::config::GestureMod::None => 0,
                    crate::config::GestureMod::Ctrl => 1,
                    crate::config::GestureMod::Shift => 2,
                    crate::config::GestureMod::Alt => 3,
                };
                let want_button = match gesture.button {
                    crate::config::GestureButton::Wheel => 0,
                    crate::config::GestureButton::Left => 1,
                    crate::config::GestureButton::Middle => 2,
                    crate::config::GestureButton::Right => 3,
                };
                if want_button != button || gesture.double_click != double_click {
                    continue;
                }
                if want_mod != 0 && !held(want_mod) {
                    continue;
                }
                // "No modifier" must really mean *no* modifier: Ctrl+middle would
                // otherwise also trigger the plain middle binding.
                if want_mod == 0 && modifier != 0 {
                    continue;
                }
                return Some(ACTIONS[index]);
            }
            None
        }

        /// Run one pin action. Kept out of the window procedure so the match arms stay
        /// readable.
        unsafe fn run_pin_action(hwnd: HWND, ctx: &mut PinCtx, action: PinAction) {
            match action {
                PinAction::Close => {
                    let _ = DestroyWindow(hwnd);
                }
                PinAction::Reset => {
                    ctx.zoom = 1.0;
                    ctx.opacity = 1.0;
                    resize_for_zoom(hwnd, ctx);
                }
                PinAction::Thumbnail => {
                    // Shrink to a quarter of the current size, and back.
                    let small = ctx.zoom <= 0.35;
                    ctx.zoom = if small { 1.0 } else { 0.25 };
                    resize_for_zoom(hwnd, ctx);
                }
                PinAction::CopyText => {
                    let _ = ctx.tx.send(PinEvent::CopyText(ctx.id));
                }
                PinAction::Zoom | PinAction::Opacity => {}
            }
        }

        match msg {
            WM_NCCREATE => {
                let cs = lparam.0 as *const CREATESTRUCTW;
                if let Some(cs) = cs.as_ref() {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, cs.lpCreateParams as isize);
                }
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
            WM_PAINT => {
                match ctx_from(hwnd) {
                    // With UpdateLayeredWindow the pixels are already on screen;
                    // painting would only draw over them.
                    Some(ctx) if ctx.layered => {}
                    Some(ctx) => paint(hwnd, ctx),
                    None => {}
                }
                LRESULT(0)
            }
            WM_ERASEBKGND => LRESULT(1),
            WM_LBUTTONDOWN => {
                let _ = ReleaseCapture();
                let _ = SendMessageW(
                    hwnd,
                    WM_NCLBUTTONDOWN,
                    Some(WPARAM(HTCAPTION as usize)),
                    Some(LPARAM(0)),
                );
                LRESULT(0)
            }
            // Double-click variants: the single-click handlers below must not
            // also fire, so a double-click swallows the press.
            m if m == WM_LBUTTONDBLCLK || m == WM_MBUTTONDBLCLK || m == WM_RBUTTONDBLCLK => {
                let button = if m == WM_LBUTTONDBLCLK {
                    1
                } else if m == WM_MBUTTONDBLCLK {
                    2
                } else {
                    3
                };
                if let Some(ctx) = ctx_from(hwnd) {
                    if let Some(action) = pin_gesture_action(ctx, 0, button, true) {
                        run_pin_action(hwnd, ctx, action);
                    }
                }
                LRESULT(0)
            }
            WM_MBUTTONDOWN => {
                let close = ctx_from(hwnd)
                    .map(|c| {
                        pin_gesture_action(c, 0, 2, false)
                            .map(|a| a == PinAction::Close)
                            .unwrap_or(c.middle_close)
                    })
                    .unwrap_or(true);
                if close {
                    let _ = DestroyWindow(hwnd);
                }
                LRESULT(0)
            }
            WM_RBUTTONUP => {
                if let Some(ctx) = ctx_from(hwnd) {
                    show_menu(hwnd, ctx);
                }
                LRESULT(0)
            }
            WM_COMMAND => {
                if let Some(ctx) = ctx_from(hwnd) {
                    run_menu_command(hwnd, ctx, wparam.0);
                }
                LRESULT(0)
            }
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            // Image shortcuts, mirroring Snipaste's internal keys: 1/2 rotate,
            // 3/4 flip, 5 grayscale, 6 invert.
            WM_KEYDOWN => {
                if let Some(ctx) = ctx_from(hwnd) {
                    let changed = match wparam.0 as u32 {
                        0x31 => {
                            rotate_bgra(ctx, true);
                            true
                        }
                        0x32 => {
                            rotate_bgra(ctx, false);
                            true
                        }
                        0x33 => {
                            flip_bgra(ctx, true);
                            true
                        }
                        0x34 => {
                            flip_bgra(ctx, false);
                            true
                        }
                        0x35 => {
                            for px in ctx.bgra.chunks_exact_mut(4) {
                                let l =
                                    (px[0] as u32 * 29 + px[1] as u32 * 150 + px[2] as u32 * 77)
                                        / 256;
                                px[0] = l as u8;
                                px[1] = l as u8;
                                px[2] = l as u8;
                            }
                            true
                        }
                        0x36 => {
                            for px in ctx.bgra.chunks_exact_mut(4) {
                                px[0] = 255 - px[0];
                                px[1] = 255 - px[1];
                                px[2] = 255 - px[2];
                            }
                            true
                        }
                        _ => false,
                    };
                    if changed {
                        resize_for_zoom(hwnd, ctx);
                        if ctx.layered {
                            let _ = apply_layered(hwnd, ctx);
                        } else {
                            let _ = InvalidateRect(Some(hwnd), None, false);
                        }
                    }
                }
                LRESULT(0)
            }
            WM_MOUSEWHEEL => {
                if let Some(ctx) = ctx_from(hwnd) {
                    let delta = ((wparam.0 >> 16) & 0xffff) as i16 as f32 / 120.0;
                    // A configured binding wins over the legacy flags.
                    if let Some(action) = pin_gesture_action(ctx, 0, 0, false) {
                        match action {
                            PinAction::Zoom => {
                                let old = ctx.zoom;
                                let step = if ctx.smooth_zoom { 0.05 } else { 0.1 };
                                let new = (old + delta * step).clamp(0.2, 4.0);
                                if (new - old).abs() > f32::EPSILON {
                                    ctx.zoom = new;
                                    if ctx.zoom_at_cursor {
                                        zoom_at_cursor(hwnd, ctx, old, new);
                                    } else {
                                        resize_for_zoom(hwnd, ctx);
                                    }
                                }
                            }
                            PinAction::Opacity => {
                                ctx.opacity = (ctx.opacity + delta * 0.08).clamp(0.15, 1.0);
                                if ctx.layered {
                                    let _ = apply_layered(hwnd, ctx);
                                } else {
                                    let _ = SetLayeredWindowAttributes(
                                        hwnd,
                                        COLORREF(0),
                                        (ctx.opacity * 255.0) as u8,
                                        LWA_ALPHA,
                                    );
                                }
                            }
                            PinAction::Reset => {
                                ctx.zoom = 1.0;
                                ctx.opacity = 1.0;
                                resize_for_zoom(hwnd, ctx);
                            }
                            _ => {}
                        }
                        return LRESULT(0);
                    }
                    let ctrl = GetKeyState(VK_CONTROL.0 as i32) < 0;
                    if ctrl {
                        ctx.opacity = (ctx.opacity + delta * 0.08).clamp(0.15, 1.0);
                        if ctx.layered {
                            let _ = apply_layered(hwnd, ctx);
                        } else {
                            let _ = SetLayeredWindowAttributes(
                                hwnd,
                                COLORREF(0),
                                (ctx.opacity * 255.0) as u8,
                                LWA_ALPHA,
                            );
                        }
                    } else if ctx.wheel_zoom {
                        let old = ctx.zoom;
                        let step = if ctx.smooth_zoom { 0.05 } else { 0.1 };
                        let new = (old + delta * step).clamp(0.2, 4.0);
                        if (new - old).abs() > f32::EPSILON {
                            ctx.zoom = new;
                            if ctx.zoom_at_cursor {
                                zoom_at_cursor(hwnd, ctx, old, new);
                            } else {
                                resize_for_zoom(hwnd, ctx);
                            }
                            if ctx.layered {
                                let _ = apply_layered(hwnd, ctx);
                            } else {
                                let _ = InvalidateRect(Some(hwnd), None, false);
                            }
                        }
                    }
                }
                LRESULT(0)
            }
            WM_TOGGLE_PASS => {
                let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
                let new_ex = if ex & WS_EX_TRANSPARENT.0 != 0 {
                    ex & !WS_EX_TRANSPARENT.0
                } else {
                    ex | WS_EX_TRANSPARENT.0
                };
                SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex as isize);
                LRESULT(0)
            }
            WM_SET_VISIBLE => {
                // The pin window lives on its own thread, so the visibility
                // change has to happen there rather than from the UI thread.
                let visible = wparam.0 != 0;
                let _ = ShowWindow(hwnd, if visible { SW_SHOW } else { SW_HIDE });
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut PinCtx;
                if !ptr.is_null() {
                    let ctx = Box::from_raw(ptr);
                    let _ = ctx.tx.send(PinEvent::Close(ctx.id));
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                }
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn paint(hwnd: HWND, ctx: &PinCtx) {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut rc);
        let cw = rc.right - rc.left;
        let ch = rc.bottom - rc.top;
        if cw > 0 && ch > 0 {
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: ctx.width as i32,
                    biHeight: -(ctx.height as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    biSizeImage: 0,
                    biXPelsPerMeter: 0,
                    biYPelsPerMeter: 0,
                    biClrUsed: 0,
                    biClrImportant: 0,
                },
                bmiColors: [RGBQUAD::default()],
            };
            let _ = StretchDIBits(
                hdc,
                0,
                0,
                cw,
                ch,
                0,
                0,
                ctx.width as i32,
                ctx.height as i32,
                Some(ctx.bgra.as_ptr() as *const c_void),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );
            let brush = CreateSolidBrush(COLORREF(0x00303030));
            let _ = FrameRect(hdc, &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
        let _ = EndPaint(hwnd, &ps);
    }

    pub fn close(hwnd: isize) {
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut c_void)),
                    WM_CLOSE,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn toggle_passthrough(hwnd: isize) {
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut c_void)),
                    WM_TOGGLE_PASS,
                    WPARAM(0),
                    LPARAM(0),
                );
            }
        }
    }

    pub fn focus(hwnd: isize) {
        if hwnd != 0 {
            unsafe {
                let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(HWND(
                    hwnd as *mut c_void,
                ));
            }
        }
    }

    /// Show or hide the pin window.
    pub fn set_visible(hwnd: isize, visible: bool) {
        if hwnd != 0 {
            unsafe {
                let _ = PostMessageW(
                    Some(HWND(hwnd as *mut c_void)),
                    WM_SET_VISIBLE,
                    WPARAM(visible as usize),
                    LPARAM(0),
                );
            }
        }
    }
}

/// A pinned image window.
pub struct Pin {
    #[cfg(not(windows))]
    vp: crate::pin_viewport::ViewportPin,
    pub id: u64,
    pub image: Arc<RgbaImage>,
    pub x: i32,
    pub y: i32,
    #[allow(dead_code)]
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
    pub zoom: f32,
    pub passthrough: bool,
    /// Which group this pin belongs to (see `config::PinGroup`).
    pub group: u32,
    #[cfg(windows)]
    native: imp::NativePin,
}

impl Pin {
    /// `tr` supplies the right-click menu labels in the current UI language.
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        id: u64,
        image: Arc<RgbaImage>,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        tr: &'static crate::i18n::Strings,
        opacity: u8,
        shadow: bool,
        shadow_color: [u8; 3],
        wheel_zoom: bool,
        zoom_at_cursor: bool,
        smooth_zoom: bool,
        middle_close: bool,
        gestures: [u32; 6],
        group: u32,
        tx: std::sync::mpsc::Sender<PinEvent>,
    ) -> Result<Self, String> {
        #[cfg(windows)]
        {
            let native = imp::create(
                id,
                image.clone(),
                x,
                y,
                width,
                height,
                tr,
                opacity,
                shadow,
                shadow_color,
                wheel_zoom,
                zoom_at_cursor,
                smooth_zoom,
                middle_close,
                gestures,
                tx,
            )?;
            Ok(Self {
                id,
                image,
                x,
                y,
                width,
                height,
                zoom: 1.0,
                passthrough: false,
                group,
                native,
            })
        }
        #[cfg(not(windows))]
        {
            // The viewport pins draw their own context menu from egui; the
            // native menu labels are Windows-only.
            let _ = tr;
            let _ = opacity;
            let _ = shadow;
            let _ = shadow_color;
            let _ = wheel_zoom;
            let _ = zoom_at_cursor;
            let _ = smooth_zoom;
            let _ = middle_close;
            let _ = gestures;
            let _ = group;
            let vp =
                crate::pin_viewport::ViewportPin::new(id, image.clone(), x, y, width, height, tx);
            Ok(Self {
                id,
                image,
                x,
                y,
                width,
                height,
                zoom: 1.0,
                passthrough: false,
                group,
                vp,
            })
        }
    }

    pub fn close(&mut self) {
        #[cfg(windows)]
        imp::close(self.native.hwnd);
        #[cfg(not(windows))]
        self.vp.close();
    }

    pub fn toggle_passthrough(&mut self) {
        self.passthrough = !self.passthrough;
        #[cfg(windows)]
        imp::toggle_passthrough(self.native.hwnd);
        #[cfg(not(windows))]
        {
            self.vp.set_passthrough(self.passthrough);
        }
    }

    pub fn focus(&self) {
        #[cfg(windows)]
        imp::focus(self.native.hwnd);
    }

    /// Show or hide the pin window.
    pub fn set_visible(&mut self, visible: bool) {
        #[cfg(windows)]
        imp::set_visible(self.native.hwnd, visible);
        #[cfg(not(windows))]
        {
            self.vp.set_visible(visible);
        }
    }

    /// Render the pin window (non-Windows only: egui viewports are driven from
    /// the app's UI pass, unlike the native Windows message loop).
    #[cfg(not(windows))]
    pub fn show_viewport(&mut self, ctx: &eframe::egui::Context) {
        self.vp.show(ctx);
    }
}
