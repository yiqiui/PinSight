//! Embeds the icon and a version block into the Windows executable.
//!
//! Without this the release exe shows the generic blank application icon in
//! Explorer and in Task Manager. Rather than pull in a resource crate (which
//! locates `rc.exe` through the registry), the script finds the Windows SDK
//! directly and compiles a tiny `.rc` file itself; when no SDK is present the
//! build still succeeds and the exe simply keeps the default icon.

use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");

    #[cfg(windows)]
    {
        if let Err(err) = embed_windows_resources() {
            println!("cargo:warning=could not embed the Windows resources: {err}");
        }
    }
}

/// Locate `rc.exe`: an explicit `RC` env var first, then the newest Windows
/// SDK under `Program Files (x86)`.
#[cfg(windows)]
fn find_rc() -> Option<PathBuf> {
    if let Ok(explicit) = std::env::var("RC") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }
    let root = PathBuf::from(std::env::var("ProgramFiles(x86)").ok()?)
        .join("Windows Kits")
        .join("10")
        .join("bin");
    let mut candidates: Vec<PathBuf> = std::fs::read_dir(&root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path().join("x64").join("rc.exe"))
        .filter(|path| path.is_file())
        .collect();
    // Newest SDK wins: the directory names are version numbers.
    candidates.sort();
    candidates.pop()
}

#[cfg(windows)]
fn embed_windows_resources() -> Result<(), Box<dyn std::error::Error>> {
    let Some(rc) = find_rc() else {
        return Err("rc.exe not found (install the Windows SDK or set RC)".into());
    };
    let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")?);
    let icon = manifest.join("assets").join("icon.ico");
    if !icon.is_file() {
        return Err("assets/icon.ico is missing".into());
    }

    let rc_file = out_dir.join("pinsight.rc");
    let res_file = out_dir.join("pinsight.res");
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
    let (major, minor, patch) = {
        let mut parts = version.split('.').map(|p| p.parse::<u16>().unwrap_or(0));
        (
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
            parts.next().unwrap_or(0),
        )
    };
    // Numeric constants instead of the winver.h names: `rc.exe` has no SDK
    // include path here, and the values are stable ABI.
    let script = format!(
        r#"IDI_ICON1 ICON "{icon}"
VS_VERSION_INFO VERSIONINFO
 FILEVERSION {major},{minor},{patch},0
 PRODUCTVERSION {major},{minor},{patch},0
 FILEFLAGSMASK 0x3fL
 FILEFLAGS 0x0L
 FILEOS 0x40004L
 FILETYPE 0x1L
 FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904b0"
        BEGIN
            VALUE "CompanyName", "PinSight"
            VALUE "FileDescription", "PinSight - screenshot, pin and record"
            VALUE "FileVersion", "{version}"
            VALUE "InternalName", "pinsight"
            VALUE "OriginalFilename", "pinsight.exe"
            VALUE "ProductName", "PinSight"
            VALUE "ProductVersion", "{version}"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x409, 1200
    END
END
"#,
        icon = icon.display().to_string().replace('\\', "\\\\"),
        version = version
    );
    std::fs::write(&rc_file, script)?;

    let status = Command::new(&rc)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_file)
        .arg(&rc_file)
        .status()?;
    if !status.success() {
        return Err(format!("rc.exe exited with {status}").into());
    }

    println!("cargo:rustc-link-arg-bins={}", res_file.display());
    Ok(())
}
