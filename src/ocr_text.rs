//! OCR text utilities shared by the capture editor and the gallery search.
//!
//! Three pieces, all kept free of GUI code so they are unit-testable:
//!
//! 1. [`text_matches`] — the substring match used by the gallery search
//!    (case-insensitive, so CJK queries behave the same as word-based ones).
//! 2. [`export_text`] — write the editor's OCR text to a `.txt` next to the
//!    captures, with a timestamped, collision-safe file name.
//! 3. [`OcrSearch`] — a tiny background indexer: a serial worker OCRs the
//!    gallery files one at a time (large images are downscaled first, search
//!    does not need print resolution) while the UI keeps rendering; results
//!    land in an mtime-keyed cache, so a failed attempt is remembered and never
//!    retried until the file actually changes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::SystemTime;

/// Case-insensitive substring match. An empty (or blank) query matches
/// everything, which is what an unfilled search box should do.
pub fn text_matches(query: &str, text: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    text.to_lowercase().contains(&q)
}

/// Write `text` to a `.txt` file in `dir` and return its path.
///
/// The name is `ocr-YYYYMMDD-HHMMSS.txt` (local time); when the second is not
/// enough to be unique, `-2`, `-3`… are appended instead of overwriting.
pub fn export_text(dir: &Path, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S").to_string();
    let mut path = dir.join(format!("ocr-{stamp}.txt"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("ocr-{stamp}-{n}.txt"));
        n += 1;
    }
    std::fs::write(&path, text)?;
    Ok(path)
}

/// Gallery OCR search state: the cache plus at most one background worker.
#[derive(Default)]
pub struct OcrSearch {
    /// `path -> (mtime indexed, recognised text)`. An empty string means the
    /// file was indexed but OCR found nothing (or failed) — it counts as
    /// "done" so a broken engine cannot cause endless retries.
    cache: HashMap<PathBuf, (SystemTime, String)>,
    /// Receiver of the running (or most recent) worker.
    rx: Option<Receiver<(PathBuf, SystemTime, String)>>,
    /// Files handed to the worker that have not been collected yet.
    pending: usize,
}

impl OcrSearch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recognised text for `path` when its cached index is current.
    pub fn text_for(&self, path: &Path, mtime: SystemTime) -> Option<&str> {
        self.cache
            .get(path)
            .filter(|(at, _)| *at == mtime)
            .map(|(_, text)| text.as_str())
    }

    /// True when the gallery entry matches `query`: the OCR text (if indexed)
    /// or, failing that, the file name. A blank query matches everything.
    pub fn matches(&self, path: &Path, mtime: SystemTime, query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return true;
        }
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        match self.text_for(path, mtime) {
            Some(text) => text_matches(q, text) || text_matches(q, &name),
            // Not indexed yet: fall back to the file name so the search box is
            // never silently useless while the indexer catches up.
            None => text_matches(q, &name),
        }
    }

    /// Make sure a worker is running that will index every file in `files`
    /// that is not cached yet. One serial worker at a time; when it drains,
    /// the next frame's call restarts it for whatever is left (new captures,
    /// changed files).
    pub fn ensure_started(&mut self, files: &[(PathBuf, SystemTime)]) {
        if self.rx.is_some() {
            return;
        }
        let todo: Vec<(PathBuf, SystemTime)> = files
            .iter()
            .filter(|(path, mtime)| !self.is_indexed(path, *mtime))
            .cloned()
            .collect();
        if todo.is_empty() {
            return;
        }
        self.pending = todo.len();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (path, mtime) in todo {
                let text = index_file(&path);
                // A send only fails when the receiver is gone (app closing) —
                // the worker can simply stop then.
                if tx.send((path, mtime, text)).is_err() {
                    return;
                }
            }
        });
        self.rx = Some(rx);
    }

    fn is_indexed(&self, path: &Path, mtime: SystemTime) -> bool {
        self.cache.get(path).is_some_and(|(at, _)| *at == mtime)
    }

    /// Collect whatever the worker has produced so far (draining the channel
    /// keeps the frame cheap; the caller just polls every frame).
    pub fn poll(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        loop {
            match rx.try_recv() {
                Ok((path, mtime, text)) => {
                    self.cache.insert(path, (mtime, text));
                    self.pending = self.pending.saturating_sub(1);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.pending = 0;
                    break;
                }
            }
        }
    }

    /// How many files were handed to the worker and are not collected yet.
    pub fn pending(&self) -> usize {
        self.pending
    }
}

/// OCR one gallery file for the search index. Large images are downscaled to
/// at most 1600 px wide first — the engine reads them better at native text
/// size and the index does not need print resolution. Any failure (unreadable
/// image, engine unavailable) yields an empty string, which the cache treats
/// as "indexed, no text".
fn index_file(path: &Path) -> String {
    let Ok(image) = image::open(path) else {
        return String::new();
    };
    let rgba = image.to_rgba8();
    const MAX_W: u32 = 1600;
    let small = if rgba.width() > MAX_W {
        image::imageops::resize(
            &rgba,
            MAX_W,
            (rgba.height() * MAX_W / rgba.width()).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        rgba
    };
    crate::ocr::recognize(small)
        .map(|words| crate::ocr::words_to_text(&words))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_query_matches_everything() {
        assert!(text_matches("", "anything"));
        assert!(text_matches("   ", "anything"));
    }

    #[test]
    fn matching_is_case_insensitive_and_substring() {
        assert!(text_matches("NOTE", "meeting notes 2026"));
        assert!(text_matches("notes 20", "Meeting Notes 2026"));
        assert!(!text_matches("invoice", "meeting notes"));
        // CJK has no case, but substring semantics must hold.
        assert!(text_matches("截图", "我的截图文件"));
        assert!(!text_matches("贴图", "我的截图文件"));
    }

    #[test]
    fn export_text_writes_a_timestamped_file() {
        let dir = std::env::temp_dir().join(format!("pinsight-ocr-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = export_text(&dir, "hello\nworld").expect("export");
        assert!(path.starts_with(&dir));
        assert!(path.extension().is_some_and(|e| e == "txt"));
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        assert!(name.starts_with("ocr-") && name.ends_with(".txt"));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\nworld");
        // A second export in the same second must not overwrite the first.
        let second = export_text(&dir, "second").expect("export again");
        assert_ne!(second, path, "collision should get a numeric suffix");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello\nworld");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn search_falls_back_to_the_file_name() {
        let mut search = OcrSearch::new();
        let path = Path::new("/tmp/20261114-report.png");
        let now = SystemTime::UNIX_EPOCH;
        // Unindexed: the file name carries the match.
        assert!(search.matches(path, now, "report"));
        assert!(!search.matches(path, now, "nomatch"));
        // Blank query always matches.
        assert!(search.matches(path, now, ""));
        // Once indexed, both the text and the name are searched.
        search
            .cache
            .insert(path.to_path_buf(), (now, "quarterly revenue".to_string()));
        assert!(search.matches(path, now, "revenue"));
        assert!(search.matches(path, now, "report"));
        assert!(!search.matches(path, now, "expenses"));
        // A changed mtime invalidates the cache entry.
        let later = now + std::time::Duration::from_secs(1);
        assert!(search.matches(path, later, "report"));
        assert_eq!(search.text_for(path, later), None);
    }
}
