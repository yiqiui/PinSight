//! After-capture workflow executor: turns an [`automation::plan`] into real
//! side effects (save, copy, upload, notify). Each step is isolated so one
//! failure never aborts the rest of the chain — the same "best effort" model
//! ShareX uses. Runs on a worker thread; never touches the GUI.

use image::RgbaImage;

use crate::automation::{ExtraFormat, WorkflowAction};
use crate::config::UploadConfig;
use crate::util::{ImageFormat, SaveOptions};

/// Everything the executor needs, snapshotted from `Config` before the worker
/// starts so it never borrows app state across threads.
pub struct RunParams {
    pub save_dir: std::path::PathBuf,
    pub save_options: SaveOptions,
    pub upload: UploadConfig,
    pub notify_sound: Option<std::path::PathBuf>,
}

impl RunParams {
    /// Snapshot the pieces of `Config` a workflow needs. The caller adjusts
    /// `upload.local.folder` first when the local backend has no folder set.
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        Self {
            save_dir: cfg.save_dir.clone(),
            save_options: SaveOptions {
                template: cfg.name_template.clone(),
                quality: cfg.image_quality,
                format: cfg.image_format,
                title: String::new(),
                decorations: cfg.decorations.clone(),
            },
            upload: cfg.upload.clone(),
            notify_sound: cfg.sound_file.clone(),
        }
    }
}

/// The outcome of one automation tick, relayed back from the worker thread.
/// `frame` carries the freshly sampled frame so the app can keep it as the
/// previous frame for the next change-detection pass without re-grabbing.
pub struct AutoTick {
    pub fired: bool,
    pub report: Option<WorkflowReport>,
    pub frame: Option<RgbaImage>,
}

/// What the workflow actually did, for the toast / CLI output.
#[derive(Debug, Default)]
pub struct WorkflowReport {
    pub saved: Vec<std::path::PathBuf>,
    pub link: Option<String>,
    pub copied_image: bool,
    pub copied_link: bool,
    pub notified: bool,
    /// Per-step failures; the chain continues after each one.
    pub errors: Vec<String>,
}

impl WorkflowReport {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    /// A compact human summary used by the tray toast and the CLI.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if !self.saved.is_empty() {
            parts.push(format!("saved {}", self.saved.len()));
        }
        if self.link.is_some() {
            parts.push("uploaded".into());
        }
        if self.copied_image || self.copied_link {
            parts.push("copied".into());
        }
        if parts.is_empty() {
            parts.push("done".into());
        }
        parts.join(" · ")
    }
}

/// Execute `plan` against `image`. Safe to call off the GUI thread.
pub fn execute(image: &RgbaImage, plan: &[WorkflowAction], params: &RunParams) -> WorkflowReport {
    let mut report = WorkflowReport::default();
    for &action in plan {
        match action {
            WorkflowAction::Save => {
                match crate::util::save_image(image, &params.save_dir, &params.save_options) {
                    Ok(path) => report.saved.push(path),
                    Err(e) => report.errors.push(format!("save: {e}")),
                }
            }
            WorkflowAction::SaveExtra(fmt) => {
                let mut opts = params.save_options.clone();
                opts.format = extra_to_format(fmt);
                match crate::util::save_image(image, &params.save_dir, &opts) {
                    Ok(path) => report.saved.push(path),
                    Err(e) => report.errors.push(format!("save:{:?}: {e}", fmt)),
                }
            }
            WorkflowAction::CopyImage => match crate::clipboard::copy_image(image) {
                Ok(()) => report.copied_image = true,
                Err(e) => report.errors.push(format!("copy image: {e}")),
            },
            WorkflowAction::Upload => {
                if report.link.is_none() {
                    match crate::upload::upload_image(image, &params.upload) {
                        Ok(link) => report.link = Some(link),
                        Err(e) => report.errors.push(format!("upload: {e}")),
                    }
                }
            }
            WorkflowAction::CopyLink => {
                // The plan guarantees an Upload precedes this; if it failed
                // there is no link to copy, so report a clear error.
                match &report.link {
                    Some(link) => match crate::clipboard::copy_text(link) {
                        Ok(()) => report.copied_link = true,
                        Err(e) => report.errors.push(format!("copy link: {e}")),
                    },
                    None => report.errors.push("copy link: no upload result".into()),
                }
            }
            WorkflowAction::Notify => {
                crate::util::play_notification(params.notify_sound.as_deref());
                report.notified = true;
            }
        }
    }
    report
}

fn extra_to_format(f: ExtraFormat) -> ImageFormat {
    match f {
        ExtraFormat::Jpeg => ImageFormat::Jpeg,
        ExtraFormat::Webp => ImageFormat::Webp,
        ExtraFormat::Bmp => ImageFormat::Bmp,
        ExtraFormat::Tiff => ImageFormat::Tiff,
        ExtraFormat::Gif => ImageFormat::Gif,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::automation::plan;
    use crate::config::{ClipboardMode, UploadBackend};
    use image::{Rgba, RgbaImage};

    fn local_params(dir: &std::path::Path) -> RunParams {
        let upload = UploadConfig {
            enabled: true,
            backend: UploadBackend::Local,
            local: crate::config::LocalUpload {
                folder: dir.join("uploaded"),
            },
            ..Default::default()
        };
        RunParams {
            save_dir: dir.join("saves"),
            save_options: SaveOptions {
                template: crate::naming::DEFAULT_TEMPLATE.to_string(),
                quality: 90,
                format: ImageFormat::Png,
                title: String::new(),
                decorations: Default::default(),
            },
            upload,
            notify_sound: None,
        }
    }

    #[test]
    fn save_and_extra_format_produce_real_files() {
        let dir = std::env::temp_dir().join(format!("ps-autorun-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let params = local_params(&dir);
        let img = RgbaImage::from_pixel(24, 24, Rgba([200, 100, 50, 255]));
        let actions = plan(&[
            WorkflowAction::Save,
            WorkflowAction::SaveExtra(ExtraFormat::Jpeg),
        ]);
        let report = execute(&img, &actions, &params);
        assert!(report.errors.is_empty(), "errors: {:?}", report.errors);
        assert_eq!(report.saved.len(), 2);
        for p in &report.saved {
            assert!(p.exists(), "{} should exist", p.display());
        }
        assert_eq!(report.summary(), "saved 2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn copy_link_without_upload_reports_an_error_but_continues() {
        let dir = std::env::temp_dir().join(format!("ps-autorun-nolink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let params = local_params(&dir);
        let img = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 255]));
        // A deliberately broken upload: disabled backend makes it fail.
        let params = RunParams {
            upload: UploadConfig {
                enabled: false,
                ..params.upload
            },
            ..params
        };
        let actions = plan(&[WorkflowAction::CopyLink, WorkflowAction::Save]);
        let report = execute(&img, &actions, &params);
        // Save still ran even though upload failed.
        assert_eq!(report.saved.len(), 1);
        assert!(!report.ok());
        assert!(report.errors.iter().any(|e| e.contains("upload")));
        assert!(report.errors.iter().any(|e| e.contains("copy link")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn upload_then_copy_link_uses_one_upload() {
        let dir = std::env::temp_dir().join(format!("ps-autorun-upload-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let params = local_params(&dir);
        let img = RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255]));
        // Duplicate Upload + CopyLink: plan dedups to a single upload.
        let actions = plan(&[
            WorkflowAction::Upload,
            WorkflowAction::Upload,
            WorkflowAction::CopyLink,
            WorkflowAction::CopyLink,
        ]);
        assert_eq!(
            actions,
            vec![WorkflowAction::Upload, WorkflowAction::CopyLink]
        );
        let report = execute(&img, &actions, &params);
        assert!(report.link.is_some(), "errors: {:?}", report.errors);
        assert!(report.copied_link);
        assert!(report.summary().contains("uploaded"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn empty_plan_is_a_clean_noop() {
        let dir = std::env::temp_dir().join(format!("ps-autorun-empty-{}", std::process::id()));
        let params = local_params(&dir);
        let img = RgbaImage::from_pixel(4, 4, Rgba([0, 0, 0, 255]));
        let report = execute(&img, &[], &params);
        assert!(report.ok());
        assert_eq!(report.summary(), "done");
        // Keep the compiler honest about ClipboardMode being available.
        let _ = ClipboardMode::Image;
    }
}
