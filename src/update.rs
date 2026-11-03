//! In-app updater.
//!
//! Queries the GitHub Releases API for `yiqiui/PinSight`, compares the newest
//! published version with the running build, and picks the asset whose name
//! matches this machine's OS/architecture (the release workflow names them
//! `PinSight-<version>-<platform>.zip|tar.gz`). The user can then download the
//! archive with live progress; on Windows we can hand off to a tiny script that
//! swaps the executable once the app has exited.
//!
//! Everything network-facing runs on a worker thread — the UI only polls a
//! channel and an atomic byte counter, so a slow or blocked GitHub never freezes
//! the window.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde::Deserialize;

use crate::config::{Config, ProxyMode};

/// GitHub repository that publishes releases.
pub const REPO: &str = "yiqiui/PinSight";
/// Human-facing releases page (also the fallback when the API is unreachable).
pub const RELEASES_URL: &str = "https://github.com/yiqiui/PinSight/releases";

#[derive(Debug, Clone)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Debug, Clone)]
pub struct Release {
    /// Version without the leading `v`, e.g. `1.2.3`.
    pub version: String,
    pub notes: String,
    pub html_url: String,
    /// The asset matching this OS/architecture, if the release shipped one.
    pub asset: Option<Asset>,
}

#[derive(Debug, Clone)]
pub enum Outcome {
    /// No newer version. `latest` is the newest published release anyway, so
    /// the UI can still offer to (re)download the current build.
    UpToDate {
        current: String,
        latest: Option<Box<Release>>,
    },
    Available(Box<Release>),
}

#[derive(Deserialize, Clone)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    body: String,
    html_url: String,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize, Clone)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// The `platform` slug the release workflow uses for this machine, e.g.
/// `windows-x64` or `aarch64-apple-darwin`.
pub fn platform_key() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => "windows-x64",
        ("windows", "aarch64") => "windows-arm64",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("linux", "x86_64") => "linux-x86_64",
        ("linux", "aarch64") => "linux-arm64",
        _ => "unknown",
    }
}

/// Proxy URL derived from the settings, or `None` for a direct connection.
fn proxy_url(cfg: &Config) -> Option<String> {
    let auth = if cfg.proxy_user.trim().is_empty() {
        String::new()
    } else {
        format!("{}:{}@", cfg.proxy_user.trim(), cfg.proxy_password.trim())
    };
    match cfg.proxy_mode {
        ProxyMode::None => None,
        ProxyMode::System => ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
            .iter()
            .find_map(|k| std::env::var(k).ok())
            .filter(|v| !v.trim().is_empty()),
        ProxyMode::Http | ProxyMode::Manual => {
            let server = cfg.proxy_server.trim();
            if server.is_empty() {
                None
            } else {
                Some(format!("http://{auth}{server}:{}", cfg.proxy_port))
            }
        }
        ProxyMode::Socks5 => {
            let server = cfg.proxy_server.trim();
            if server.is_empty() {
                None
            } else {
                Some(format!("socks5://{auth}{server}:{}", cfg.proxy_port))
            }
        }
    }
}

fn client(cfg: &Config, total_timeout: Option<Duration>) -> reqwest::blocking::Client {
    let mut b = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(20))
        .user_agent(concat!("PinSight/", env!("CARGO_PKG_VERSION")));
    if let Some(t) = total_timeout {
        b = b.timeout(t);
    }
    match cfg.proxy_mode {
        ProxyMode::None => {
            b = b.no_proxy();
        }
        _ => {
            if let Some(url) = proxy_url(cfg) {
                if let Ok(p) = reqwest::Proxy::all(&url) {
                    b = b.proxy(p);
                }
            }
        }
    }
    b.build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

fn parse_version(s: &str) -> Vec<u64> {
    s.trim()
        .trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|p| !p.is_empty())
        .filter_map(|p| p.parse::<u64>().ok())
        .collect()
}

/// True when `remote` is a strictly higher version than `current`.
fn is_newer(remote: &str, current: &str) -> bool {
    let a = parse_version(remote);
    let b = parse_version(current);
    for i in 0..a.len().max(b.len()) {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// Score how well an asset name matches this machine. Higher is better; names
/// for a different OS/architecture score below zero so they are never chosen
/// when anything else exists.
///
/// Two naming schemes exist in the wild: the CI workflow uses
/// `PinSight-<v>-windows-x64.zip`, while `scripts/package.ps1` publishes the
/// Windows x64 build as `PinSight-<v>-portable.zip`. Both are handled.
fn asset_score(name: &str) -> Option<i32> {
    let lower = name.to_ascii_lowercase();
    let ext = if cfg!(windows) { ".zip" } else { ".tar.gz" };
    if !lower.ends_with(ext) {
        return None;
    }
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let key = platform_key();

    let mut score = 0;
    if lower.contains(key) {
        score += 100;
    }

    let os_hit = match os {
        "windows" => lower.contains("windows"),
        "macos" => lower.contains("apple-darwin") || lower.contains("macos"),
        "linux" => lower.contains("linux"),
        _ => false,
    };
    // `portable.zip` is the Windows x64 build from package.ps1.
    let portable_windows = os == "windows" && lower.contains("portable");
    if os_hit || portable_windows {
        score += 30;
    }

    let arch_hit = match arch {
        "x86_64" => lower.contains("x64") || lower.contains("x86_64") || lower.contains("amd64"),
        "aarch64" => lower.contains("arm64") || lower.contains("aarch64"),
        _ => false,
    };
    if arch_hit {
        score += 20;
    }

    // A name for the *other* architecture must never win on a tie.
    let other_arch = match arch {
        "x86_64" => lower.contains("arm64") || lower.contains("aarch64"),
        "aarch64" => lower.contains("x64") || lower.contains("x86_64"),
        _ => false,
    };
    if other_arch {
        score -= 60;
    }

    // Prefer the plain build over the UPX-compressed one (antivirus noise).
    if lower.contains("upx") {
        score -= 10;
    }

    Some(score)
}

fn pick_asset(assets: &[ApiAsset]) -> Option<Asset> {
    assets
        .iter()
        .filter_map(|a| asset_score(&a.name).map(|s| (s, a)))
        .filter(|(s, _)| *s > 0)
        .max_by_key(|(s, _)| *s)
        .map(|(_, a)| Asset {
            name: a.name.clone(),
            url: a.browser_download_url.clone(),
            size: a.size,
        })
}

/// Query GitHub and compare against the running build.
pub fn check(cfg: &Config) -> Result<Outcome, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=20");
    let resp = client(cfg, Some(Duration::from_secs(30)))
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| format!("network: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("GitHub API {}", resp.status()));
    }
    let list: Vec<ApiRelease> = resp.json().map_err(|e| format!("bad response: {e}"))?;

    let current = env!("CARGO_PKG_VERSION").to_string();
    let mut best_new: Option<ApiRelease> = None;
    let mut latest: Option<ApiRelease> = None;
    for r in list {
        if r.draft {
            continue;
        }
        if r.prerelease && !cfg.update_beta {
            continue;
        }
        // Highest published version overall (for the "download anyway" path).
        if latest
            .as_ref()
            .map(|l| is_newer(&r.tag_name, &l.tag_name))
            .unwrap_or(true)
        {
            latest = Some(r.clone());
        }
        if !is_newer(&r.tag_name, &current) {
            continue;
        }
        let better = best_new
            .as_ref()
            .map(|b| is_newer(&r.tag_name, &b.tag_name))
            .unwrap_or(true);
        if better {
            best_new = Some(r);
        }
    }

    match best_new {
        Some(r) => Ok(Outcome::Available(Box::new(to_release(&r)))),
        None => Ok(Outcome::UpToDate {
            current,
            latest: latest.as_ref().map(|r| Box::new(to_release(r))),
        }),
    }
}

fn to_release(r: &ApiRelease) -> Release {
    Release {
        version: r.tag_name.trim_start_matches('v').to_string(),
        notes: if r.body.trim().is_empty() {
            r.name.clone()
        } else {
            r.body.clone()
        },
        html_url: r.html_url.clone(),
        asset: pick_asset(&r.assets),
    }
}

/// A download running on a worker thread.
pub struct DownloadJob {
    pub rx: mpsc::Receiver<Result<PathBuf, String>>,
    pub progress: Arc<AtomicU64>,
    pub total: u64,
    pub cancel: Arc<AtomicBool>,
}

impl DownloadJob {
    /// Bytes downloaded so far.
    pub fn done(&self) -> u64 {
        self.progress.load(Ordering::Relaxed)
    }
}

/// Start downloading `asset` into `dir` on a background thread.
pub fn start_download(cfg: Config, asset: Asset, dir: PathBuf) -> DownloadJob {
    let (tx, rx) = mpsc::channel();
    let progress = Arc::new(AtomicU64::new(0));
    let cancel = Arc::new(AtomicBool::new(false));
    let p = Arc::clone(&progress);
    let c = Arc::clone(&cancel);
    let total = asset.size;

    std::thread::Builder::new()
        .name("pinsight-update-dl".into())
        .spawn(move || {
            let result = (|| -> Result<PathBuf, String> {
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                let path = dir.join(&asset.name);
                let mut resp = client(&cfg, None)
                    .get(&asset.url)
                    .send()
                    .map_err(|e| format!("network: {e}"))?;
                if !resp.status().is_success() {
                    return Err(format!("download {}", resp.status()));
                }
                let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
                let mut buf = vec![0u8; 64 * 1024];
                loop {
                    if c.load(Ordering::Relaxed) {
                        let _ = std::fs::remove_file(&path);
                        return Err("cancelled".into());
                    }
                    let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
                    p.fetch_add(n as u64, Ordering::Relaxed);
                }
                Ok(path)
            })();
            let _ = tx.send(result);
        })
        .ok();

    DownloadJob {
        rx,
        progress,
        total,
        cancel,
    }
}

/// Human-readable byte size, e.g. `12.4 MB`.
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Show `path` in the OS file manager (selecting it where supported).
pub fn reveal(path: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("explorer")
            .arg("/select,")
            .arg(path)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let target = path.parent().unwrap_or(path);
        let _ = std::process::Command::new("xdg-open").arg(target).spawn();
    }
}

/// Where downloads land: `Downloads/PinSight` when known, else the temp dir.
pub fn download_dir() -> PathBuf {
    dirs::download_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(std::env::temp_dir)
        .join("PinSight")
}

/// Extract the archive with the OS `tar` (Windows 10+ ships bsdtar, which reads
/// zip; macOS/Linux read tar.gz natively). Returns the folder holding the app
/// binary, so the caller can offer to install from it.
pub fn extract(archive: &Path, dest: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    let status = std::process::Command::new("tar")
        .arg("-xf")
        .arg(archive)
        .arg("-C")
        .arg(dest)
        .status()
        .map_err(|e| format!("tar not available: {e}"))?;
    if !status.success() {
        return Err(format!("tar exited with {status}"));
    }
    // The archive wraps everything in one `pinsight-<platform>/` folder; find
    // the binary so we know which folder to copy from.
    let bin_name = if cfg!(windows) {
        "pinsight.exe"
    } else {
        "pinsight"
    };
    find_file(dest, bin_name)
        .and_then(|bin| bin.parent().map(Path::to_path_buf))
        .ok_or_else(|| format!("no {bin_name} inside the archive"))
}

fn find_file(root: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_file(&path, name) {
                return Some(found);
            }
        } else if path.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(path);
        }
    }
    None
}

/// Replace the running executable with the freshly extracted build and restart.
///
/// Windows refuses to overwrite a running image, so a detached batch file waits
/// for this process to exit, copies the new files over the install folder,
/// relaunches, and deletes itself. The caller must quit right after this returns.
#[cfg(windows)]
pub fn install_and_restart(source_dir: &Path) -> Result<(), String> {
    use std::os::windows::process::CommandExt;

    let current = std::env::current_exe().map_err(|e| e.to_string())?;
    let install_dir = current
        .parent()
        .ok_or_else(|| "cannot find install directory".to_string())?
        .to_path_buf();

    let bat = std::env::temp_dir().join("pinsight-update.bat");
    let script = format!(
        "@echo off\r\n\
         setlocal\r\n\
         set \"SRC={src}\"\r\n\
         set \"DST={dst}\"\r\n\
         :wait\r\n\
         tasklist /FI \"IMAGENAME eq pinsight.exe\" 2>nul | find /I \"pinsight.exe\" >nul\r\n\
         if not errorlevel 1 (\r\n\
         \x20 timeout /t 1 /nobreak >nul\r\n\
         \x20 goto wait\r\n\
         )\r\n\
         xcopy /Y /E /I \"%SRC%\\*\" \"%DST%\\\" >nul\r\n\
         start \"\" \"%DST%\\pinsight.exe\"\r\n\
         rmdir /S /Q \"%SRC%\"\r\n\
         (goto) 2>nul & del \"%~f0\"\r\n",
        src = source_dir.display(),
        dst = install_dir.display()
    );
    std::fs::write(&bat, script).map_err(|e| e.to_string())?;

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    std::process::Command::new("cmd")
        .arg("/C")
        .arg(&bat)
        .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
        .spawn()
        .map_err(|e| format!("cannot start updater: {e}"))?;
    Ok(())
}

/// Non-Windows builds cannot reliably hot-swap a running binary (and macOS apps
/// live in a bundle), so we only reveal the extracted build.
#[cfg(not(windows))]
pub fn install_and_restart(_source_dir: &Path) -> Result<(), String> {
    Err("automatic install is not supported on this platform".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v1.2.3"), vec![1, 2, 3]);
        assert_eq!(parse_version("1.0"), vec![1, 0]);
        assert_eq!(parse_version("v0.1.0-beta.2"), vec![0, 1, 0, 2]);
    }

    #[test]
    fn newer_only_when_strictly_higher() {
        assert!(is_newer("v1.2.3", "1.2.2"));
        assert!(is_newer("1.0.1", "1.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("v0.9.9", "1.0.0"));
        assert!(is_newer("2.0.0", "1.99.99"));
    }

    #[test]
    fn platform_key_is_known() {
        // The test host is one of the supported targets.
        assert_ne!(platform_key(), "unknown");
    }

    #[test]
    fn picks_the_platform_asset() {
        let key = platform_key();
        let ext = if cfg!(windows) { "zip" } else { "tar.gz" };
        let assets = vec![
            ApiAsset {
                name: format!("PinSight-1.2.3-other-platform.{ext}"),
                browser_download_url: "https://example.com/other".into(),
                size: 1,
            },
            ApiAsset {
                name: format!("PinSight-1.2.3-{key}.{ext}"),
                browser_download_url: "https://example.com/mine".into(),
                size: 42,
            },
        ];
        let picked = pick_asset(&assets).expect("an asset");
        assert_eq!(picked.url, "https://example.com/mine");
        assert_eq!(picked.size, 42);
    }

    #[test]
    fn windows_x64_prefers_plain_portable_over_upx_and_arm64() {
        if !(cfg!(windows) && std::env::consts::ARCH == "x86_64") {
            return;
        }
        let assets = vec![
            ApiAsset {
                name: "PinSight-0.1.0-portable-upx.zip".into(),
                browser_download_url: "https://example.com/upx".into(),
                size: 1,
            },
            ApiAsset {
                name: "PinSight-0.1.0-portable.zip".into(),
                browser_download_url: "https://example.com/portable".into(),
                size: 2,
            },
            ApiAsset {
                name: "PinSight-0.1.0-windows-arm64.zip".into(),
                browser_download_url: "https://example.com/arm".into(),
                size: 3,
            },
        ];
        let picked = pick_asset(&assets).expect("an asset");
        assert_eq!(picked.url, "https://example.com/portable");
    }
}
