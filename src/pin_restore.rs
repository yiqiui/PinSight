//! Persist active pins across app restarts.
//!
//! Images are written as PNGs into `<save_dir>/pins_restore/`; a small JSON
//! manifest records each pin's position, group and optional voice note. The
//! image bytes deliberately stay out of `config.json` (which is a tiny,
//! frequently-written JSON file). Snapshots are taken in `App::on_exit` and
//! restored once in `PinSightApp::new`.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// One saved pin. `file` is a filename relative to the restore directory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SavedPin {
    pub file: String,
    pub x: i32,
    pub y: i32,
    pub group: u32,
    /// Voice-note audio, if the pin had one attached.
    #[serde(default)]
    pub audio: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(default)]
    pins: Vec<SavedPin>,
}

fn default_version() -> u32 {
    1
}

/// Directory holding the snapshot. Created on demand.
pub fn restore_dir(save_dir: &Path) -> PathBuf {
    save_dir.join("pins_restore")
}

/// Write a snapshot of `pins`. `pins` is `(id, image, x, y, group, audio)`.
/// Old PNGs are removed first so deleted pins do not accumulate on disk.
pub fn save(
    save_dir: &Path,
    pins: &[(u64, &image::RgbaImage, i32, i32, u32, Option<PathBuf>)],
) -> anyhow::Result<()> {
    let dir = restore_dir(save_dir);
    std::fs::create_dir_all(&dir)?;

    // Clear previous PNGs and manifest from this directory only.
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("png")
                || path.file_name().and_then(|n| n.to_str()) == Some("manifest.json")
            {
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    let mut manifest = Manifest {
        version: default_version(),
        pins: Vec::with_capacity(pins.len()),
    };
    for (id, image, x, y, group, audio) in pins {
        let file = format!("{id}.png");
        image.save(dir.join(&file))?;
        manifest.pins.push(SavedPin {
            file,
            x: *x,
            y: *y,
            group: *group,
            audio: audio.as_ref().map(|p| p.to_string_lossy().into_owned()),
        });
    }
    write_manifest(&dir, &manifest)
}

fn write_manifest(dir: &Path, manifest: &Manifest) -> anyhow::Result<()> {
    let text = serde_json::to_string_pretty(manifest)?;
    let final_path = dir.join("manifest.json");
    let tmp = dir.join("manifest.json.tmp");
    std::fs::write(&tmp, text)?;
    match std::fs::rename(&tmp, &final_path) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e.into())
        }
    }
}

/// Read the snapshot. Missing/corrupt files simply yield no pins — restoring
/// is best effort and must never prevent the app from starting.
pub fn load(save_dir: &Path) -> Vec<(PathBuf, SavedPin)> {
    let dir = restore_dir(save_dir);
    let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) else {
        return Vec::new();
    };
    let Ok(manifest) = serde_json::from_str::<Manifest>(&text) else {
        return Vec::new();
    };
    if manifest.version != default_version() {
        return Vec::new();
    }
    manifest
        .pins
        .into_iter()
        .filter_map(|pin| {
            let path = dir.join(&pin.file);
            // Reject path traversal from a tampered manifest.
            if !path.starts_with(&dir) || !path.is_file() {
                return None;
            }
            Some((path, pin))
        })
        .collect()
}

/// Delete the snapshot directory contents (called when the user turns the
/// feature off, so stale pins do not silently come back later).
pub fn clear(save_dir: &Path) {
    let dir = restore_dir(save_dir);
    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let _ = std::fs::remove_dir(&dir);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("ps-restore-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let img = image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255]));
        let audio = PathBuf::from("C:/voice/note.wav");
        let pins = vec![(42u64, &img, 100, -20, 3, Some(audio.clone()))];
        save(&dir, &pins).unwrap();
        let loaded = load(&dir);
        assert_eq!(loaded.len(), 1);
        let (path, pin) = &loaded[0];
        assert!(path.is_file());
        assert_eq!(pin.file, "42.png");
        assert_eq!((pin.x, pin.y, pin.group), (100, -20, 3));
        assert_eq!(pin.audio.as_deref(), Some(audio.to_str().unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn clear_removes_everything() {
        let dir = std::env::temp_dir().join(format!("ps-restore-clear-{}", std::process::id()));
        let img = image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 0, 0, 255]));
        save(&dir, &[(1, &img, 0, 0, 0, None)]).unwrap();
        assert_eq!(load(&dir).len(), 1);
        clear(&dir);
        assert!(load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn tampered_manifest_cannot_escape_the_directory() {
        let dir = std::env::temp_dir().join(format!("ps-restore-bad-{}", std::process::id()));
        let rd = restore_dir(&dir);
        std::fs::create_dir_all(&rd).unwrap();
        let bad = r#"{"version":1,"pins":[{"file":"../evil.png","x":0,"y":0,"group":0}]}"#;
        std::fs::write(rd.join("manifest.json"), bad).unwrap();
        assert!(load(&dir).is_empty(), "traversal path must be rejected");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
