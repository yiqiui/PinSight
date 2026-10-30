//! Clipboard output for captures, honoring the user's [`ClipboardMode`].
//!
//! * `Image` puts a PNG (and, on Windows, a DIB) on the clipboard via `arboard`,
//!   which every modern app and Office accept.
//! * `File` writes the image to the save folder and places that file on the
//!   clipboard as `CF_HDROP`, so pasting into a chat app attaches the picture
//!   instead of inlining it.
//! * `Disabled` does nothing — the capture is still saved or pinned.

use std::path::Path;

use image::RgbaImage;

use crate::config::{ClipboardMode, Config};

/// Copy `image` to the clipboard according to the configured mode.
///
/// Returns `Ok(())` whether or not anything was written (so callers can decide
/// their own "copied" toast). Errors are only returned for unexpected failures
/// (clipboard busy, disk full for the file path, …).
pub fn copy_capture(config: &Config, image: &RgbaImage) -> anyhow::Result<()> {
    match config.clipboard_mode {
        ClipboardMode::Disabled => Ok(()),
        ClipboardMode::Image => copy_image(image),
        ClipboardMode::File => copy_as_file(config, image),
    }
}

/// Plain image copy: PNG (+ DIB on Windows) through `arboard`.
pub fn copy_image(image: &RgbaImage) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_image(crate::util::to_arboard(image))?;
    Ok(())
}

/// Copy plain text to the clipboard (used by the OCR results panel).
pub fn copy_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_text(text.to_owned())?;
    Ok(())
}

/// Write the image into the save folder and put that file on the clipboard as
/// `CF_HDROP`. The configured naming template, format and quality are used so
/// the file matches what "Save" would have produced.
#[cfg(windows)]
fn copy_as_file(config: &Config, image: &RgbaImage) -> anyhow::Result<()> {
    let options = crate::util::SaveOptions {
        template: config.name_template.clone(),
        quality: config.image_quality,
        format: config.image_format,
        title: String::new(),
        decorations: config.decorations.clone(),
    };
    let path = crate::util::save_image(image, &config.save_dir, &options)?;
    match set_clipboard_file(&path) {
        Ok(()) => Ok(()),
        // If the shell route fails for any reason, fall back to a plain image
        // copy so the user still gets something pasteable.
        Err(err) => {
            tracing::warn!(%err, "CF_HDROP copy failed, falling back to image");
            copy_image(image)
        }
    }
}

/// On non-Windows platforms a file-on-clipboard needs shell support we do not
/// ship, so we copy the image bytes instead.
#[cfg(not(windows))]
fn copy_as_file(_config: &Config, image: &RgbaImage) -> anyhow::Result<()> {
    copy_image(image)
}

/// Place a single file path on the clipboard as `CF_HDROP` (Unicode).
#[cfg(windows)]
fn set_clipboard_file(path: &Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::Foundation::{HANDLE, POINT};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::System::Ole::CF_HDROP;
    use windows::Win32::UI::Shell::DROPFILES;
    use windows::core::BOOL;

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0u16))
        .collect();
    // Struct + wide string + terminating double-null.
    let struct_size = std::mem::size_of::<DROPFILES>() as usize;
    let bytes = struct_size + (wide.len() + 1) * std::mem::size_of::<u16>();

    unsafe {
        let hglobal = GlobalAlloc(GMEM_MOVEABLE, bytes)?;
        let base = GlobalLock(hglobal) as *mut u8;
        if base.is_null() {
            return Err(anyhow::anyhow!("GlobalLock returned null"));
        }
        // DROPFILES.pFiles is the byte offset where the file list begins.
        let header = DROPFILES {
            pFiles: struct_size as u32,
            pt: POINT { x: 0, y: 0 },
            fNC: BOOL(0),
            fWide: BOOL(1),
        };
        std::ptr::write(base as *mut DROPFILES, header);
        let strings = base.add(struct_size) as *mut u16;
        std::ptr::copy_nonoverlapping(wide.as_ptr(), strings, wide.len());
        // Extra null terminator already accounted for by `wide.len() + 1`.
        *strings.add(wide.len()) = 0;
        let _ = GlobalUnlock(hglobal);

        OpenClipboard(None)?;
        EmptyClipboard()?;
        // SetClipboardData takes ownership of the handle; do not free it.
        let result = SetClipboardData(
            CF_HDROP.0 as u32,
            Some(HANDLE(hglobal.0 as *mut std::ffi::c_void)),
        );
        let _ = CloseClipboard();
        result?;
        Ok(())
    }
}
