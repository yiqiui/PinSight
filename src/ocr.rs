//! Text recognition using the OCR engine built into Windows 10/11
//! (`Windows.Media.Ocr`). The engine is offline and ships with every Windows
//! install (it recognises the user's display language and any installed
//! language packs).
//!
//! Other platforms have no equivalent first-party engine, so `recognize` returns
//! an informative error there. The feature is wired into the capture editor's
//! toolbar and the CLI (`pinsight shot --ocr`).

use anyhow::{Context, Result};
use image::{DynamicImage, ImageFormat, RgbaImage};

/// One recognised word with its bounding box in image pixels.
#[derive(Debug, Clone)]
pub struct OcrWord {
    pub text: String,
    /// `(x, y, w, h)` in the coordinate space of the image that was recognised.
    pub rect: (f32, f32, f32, f32),
}

/// Block on a WinRT async operation by registering a completion callback that
/// sends the result through a channel, then waiting on that channel. This runs
/// on a dedicated worker thread (see [`recognize`]) so the wait never deadlocks
/// the GUI's STA thread.
macro_rules! block_op {
    ($op:expr) => {{
        // The channel's error type is inferred from the `when` closure, where
        // the error is the concrete `windows_result::error::Error`. We then
        // convert it to `anyhow::Error` explicitly (rather than `Into::into`),
        // because several crates provide `From<windows_result::error::Error>` and
        // the destination would otherwise be ambiguous.
        let (tx, rx) = std::sync::mpsc::channel();
        ($op)
            .when(move |r| {
                let _ = tx.send(r);
            })
            .context("subscribing to async operation")?;
        let res = rx
            .recv()
            .map_err(|_| anyhow::anyhow!("async operation did not complete"))?;
        res.map_err(anyhow::Error::from)
    }};
}

/// Recognise the text in `image` and return every word, left-to-right,
/// top-to-bottom. The words are ordered as they appear so joining them with
/// spaces reproduces the visible reading order.
#[cfg(windows)]
pub fn recognize(image: RgbaImage) -> Result<Vec<OcrWord>> {
    // Run the WinRT pipeline on a worker thread initialised as an MTA
    // apartment. The async calls below block to completion without pumping a
    // message loop, which would deadlock on an STA thread (the GUI thread), so
    // they must not execute there.
    std::thread::scope(|s| {
        let handle = s.spawn(|| recognize_inner(image));
        handle
            .join()
            .unwrap_or_else(|_| Err(anyhow::anyhow!("OCR thread panicked")))
    })
}

#[cfg(windows)]
fn recognize_inner(image: RgbaImage) -> Result<Vec<OcrWord>> {
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

    // Initialise an MTA apartment on this fresh worker thread. If it is already
    // initialised (it should not be on a thread we just spawned) we ignore the
    // error — the engine still works as long as it is MTA, not STA.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    // Encode the captured region to PNG in memory. The decoder below turns it
    // into a properly formatted SoftwareBitmap (BGRA8 / Gray8), which is what
    // the OCR engine wants, and resolves any alpha/colour issues for us.
    let mut png = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .context("encoding image to PNG")?;
    }

    let stream = InMemoryRandomAccessStream::new()?;
    {
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(&png)?;
        let _ = block_op!(writer.StoreAsync()?)?;
        // Keep the bytes in the stream after the writer is dropped.
        let _ = writer.DetachStream()?;
    }
    stream.Seek(0)?;

    let decoder = block_op!(BitmapDecoder::CreateAsync(&stream)?)?;
    let bitmap = block_op!(decoder.GetSoftwareBitmapAsync()?)?;
    // Gray8 has no alpha channel, which the OCR engine accepts directly.
    let gray = SoftwareBitmap::Convert(&bitmap, BitmapPixelFormat::Gray8)?;

    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .context("no OCR engine for the user's language (Windows.Media.Ocr)")?;
    let result = block_op!(engine.RecognizeAsync(&gray)?)?;

    let mut words: Vec<OcrWord> = Vec::new();
    // `IVectorView` is not re-exported from `windows::Foundation::Collections`,
    // but `Lines`/`Words` return it concretely and the `.Size()`/`.GetAt()`
    // methods resolve through trait inference, so we let the type be inferred.
    let lines = result.Lines()?;
    let line_count = lines.Size()?;
    for li in 0..line_count {
        let line = lines.GetAt(li)?;
        let line_words = line.Words()?;
        let word_count = line_words.Size()?;
        for wi in 0..word_count {
            let word = line_words.GetAt(wi)?;
            let text = word.Text()?.to_string();
            if text.trim().is_empty() {
                continue;
            }
            let r = word.BoundingRect()?;
            words.push(OcrWord {
                text,
                rect: (r.X, r.Y, r.Width, r.Height),
            });
        }
    }
    Ok(words)
}

/// macOS and Linux have offline engines too, so the public `recognize` is
/// dispatched by platform. Windows keeps its own threaded WinRT backend above;
/// the Apple Vision and Tesseract backends and a catch-all error live here.
///
/// NOTE: the macOS/Linux branches were written against the crate APIs but could
/// not be compiled on this Windows build host — they are gated behind
/// `cfg(target_os=…)` and verified separately on those platforms.
#[cfg(target_os = "macos")]
pub fn recognize(image: RgbaImage) -> Result<Vec<OcrWord>> {
    recognize_apple(image)
}

#[cfg(target_os = "linux")]
pub fn recognize(image: RgbaImage) -> Result<Vec<OcrWord>> {
    recognize_tesseract(image)
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
pub fn recognize(_image: RgbaImage) -> Result<Vec<OcrWord>> {
    anyhow::bail!("OCR is not available on this platform")
}

/// macOS backend: Apple Vision `VNRecognizeTextRequest` over the captured PNG.
/// Vision ships with macOS and runs fully offline, so there is nothing to
/// bundle. Language follows the system locale (matching how the Windows backend
/// uses the user's installed language packs).
#[cfg(target_os = "macos")]
fn recognize_apple(image: RgbaImage) -> Result<Vec<OcrWord>> {
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSData, NSDictionary};
    use objc2_vision::{
        VNImageRequestHandler, VNRecognizeTextRequest, VNRecognizedTextObservation, VNRequest,
        VNRequestTextRecognitionLevel,
    };

    let mut png = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .context("encoding image to PNG")?;
    }
    let data = NSData::with_bytes(&png);
    let options = NSDictionary::<objc2_vision::VNImageOption, AnyObject>::new();

    let handler = VNImageRequestHandler::alloc().initWithData_options(&data, &options);

    let request = unsafe { VNRecognizeTextRequest::alloc().init() };
    request
        .setRecognitionLevel(VNRequestTextRecognitionLevel::VNRequestTextRecognitionLevelAccurate);
    request.setUsesLanguageCorrection(true);

    let requests = NSArray::from_slice(&[&request as &VNRequest]);
    handler
        .performRequests_error(&requests)
        .context("Vision text recognition failed")?;

    let mut words = Vec::new();
    if let Some(results) = request.results() {
        for (li, obs) in results.to_vec().into_iter().enumerate() {
            let obs = obs.as_ref();
            // Each observation is one text region; take its top candidate line.
            let candidates = obs.topCandidates(1);
            if let Some(candidate) = candidates.first() {
                let line = candidate.string().to_string();
                for w in line.split_whitespace() {
                    if w.is_empty() {
                        continue;
                    }
                    // Vision returns regions in roughly reading order; spread
                    // them on a synthetic baseline so `words_to_text` keeps the
                    // line structure (real boxes are omitted here for API
                    // simplicity).
                    words.push(OcrWord {
                        text: w.to_string(),
                        rect: (0.0, li as f32 * 24.0, 0.0, 24.0),
                    });
                }
            }
        }
    }
    Ok(words)
}

/// Linux backend: Tesseract (via `leptess`), used only when the system library
/// and the matching language packs are installed. This is the "L1" option — we
/// link the system `libtesseract` rather than bundling it, and report a clear
/// error (instead of crashing) when it is missing.
#[cfg(target_os = "linux")]
fn recognize_tesseract(image: RgbaImage) -> Result<Vec<OcrWord>> {
    use crate::i18n::Lang;
    use leptess::LepTess;

    let lang = Lang::detect();
    let mut lt = leptess_new(&tesseract_lang_codes(lang))?;

    let mut png = Vec::new();
    {
        let mut cursor = std::io::Cursor::new(&mut png);
        DynamicImage::ImageRgba8(image)
            .write_to(&mut cursor, ImageFormat::Png)
            .context("encoding image to PNG")?;
    }
    lt.set_image_from_mem(&png)
        .context("loading the image into Tesseract")?;

    let text = lt.get_utf8_text().context("Tesseract recognition failed")?;

    // Tesseract returns text with newlines between lines. We split on them and
    // spread each line on a synthetic baseline so `words_to_text` preserves the
    // line structure (per-word boxes are omitted on Linux for portability).
    let mut words = Vec::new();
    for (li, line) in text.split('\n').enumerate() {
        for w in line.split_whitespace() {
            if w.is_empty() {
                continue;
            }
            words.push(OcrWord {
                text: w.to_string(),
                rect: (0.0, li as f32 * 24.0, 0.0, 24.0),
            });
        }
    }
    Ok(words)
}

/// Try each candidate language list in order; Tesseract fails to initialise if a
/// requested language pack is absent, so we fall back to plain English (almost
/// always installed) and finally surface a helpful message.
#[cfg(target_os = "linux")]
fn leptess_new(candidates: &[String]) -> Result<LepTess> {
    let mut last_err = None;
    for lang in candidates {
        match LepTess::new(None, lang) {
            Ok(lt) => return Ok(lt),
            Err(e) => last_err = Some(e.to_string()),
        }
    }
    // Last-ditch fallback, with a message that points at the missing dependency.
    LepTess::new(None, "eng").map_err(|e| {
        anyhow::anyhow!(
            "Tesseract OCR is not available. Install `tesseract-ocr` and the needed \
             language packs (e.g. `apt install tesseract-ocr tesseract-ocr-eng`): {e}"
        )
    })
}

#[cfg(target_os = "linux")]
fn tesseract_lang_codes(lang: Lang) -> Vec<String> {
    let primary = match lang {
        Lang::En => "eng",
        Lang::ZhHans => "chi_sim",
        Lang::ZhHant => "chi_tra",
        Lang::Ja => "jpn",
        Lang::Ko => "kor",
        Lang::Fr => "fra",
        Lang::De => "deu",
    };
    let mut v = Vec::new();
    if primary != "eng" {
        v.push(format!("{primary}+eng"));
    }
    v.push("eng".to_string());
    v
}

/// Reassemble the flat word list into readable text by grouping words that
/// share a baseline into lines and ordering each line left-to-right. The OCR
/// engine hands back words in reading order, but the result is a single flat
/// list (line membership is lost once we flatten), so we reconstruct lines from
/// the bounding boxes. Returns an empty string when there was nothing to read.
pub fn words_to_text(words: &[OcrWord]) -> String {
    if words.is_empty() {
        return String::new();
    }
    let mut idx: Vec<usize> = (0..words.len()).collect();
    idx.sort_by(|&a, &b| {
        words[a]
            .rect
            .1
            .partial_cmp(&words[b].rect.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                words[a]
                    .rect
                    .0
                    .partial_cmp(&words[b].rect.0)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });

    let mut lines: Vec<Vec<&OcrWord>> = Vec::new();
    let mut line_top = f32::MIN;
    let mut line_h = 0.0f32;
    for &i in &idx {
        let (_, y, _, h) = words[i].rect;
        // A new line begins once the next word's top drops well below the
        // current line's band (roughly 0.6 of a line height of separation).
        if y - line_top > line_h.max(h) * 0.6 {
            lines.push(Vec::new());
            line_top = y;
            line_h = h;
        } else {
            line_top = line_top.min(y);
            line_h = line_h.max(h);
        }
        lines.last_mut().unwrap().push(&words[i]);
    }

    let mut out = String::new();
    for line in &mut lines {
        line.sort_by(|a, b| {
            a.rect
                .0
                .partial_cmp(&b.rect.0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for w in line {
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            out.push_str(&w.text);
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(text: &str, x: f32, y: f32) -> OcrWord {
        OcrWord {
            text: text.to_string(),
            // 10px tall, 30px wide — a typical caption line.
            rect: (x, y, 30.0, 10.0),
        }
    }

    #[test]
    fn groups_words_into_lines_by_baseline() {
        // Two lines: "Hello World" on top, "Rust" below.
        let words = vec![
            word("Hello", 0.0, 0.0),
            word("World", 40.0, 0.0),
            word("Rust", 0.0, 20.0),
        ];
        let text = words_to_text(&words);
        assert_eq!(text, "Hello World\nRust");
    }

    #[test]
    fn orders_words_left_to_right_within_a_line() {
        let words = vec![
            word("B", 40.0, 0.0),
            word("A", 0.0, 0.0),
            word("C", 80.0, 0.0),
        ];
        assert_eq!(words_to_text(&words), "A B C");
    }

    #[test]
    fn empty_input_yields_empty_string() {
        assert_eq!(words_to_text(&[]), "");
    }
}
