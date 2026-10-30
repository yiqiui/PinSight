//! Font resolution.
//!
//! egui only ships a Latin font (Ubuntu Light), so any UI in Chinese, Japanese
//! or Korean would render as tofu. PinSight therefore discovers system fonts at
//! startup and installs:
//!
//! 1. a *primary* face matching the UI language (or the user's explicit pick),
//! 2. one CJK fallback so mixed-script text never breaks,
//! 3. the bundled egui icon font, kept last for its private-use glyphs.
//!
//! Nothing is downloaded and no WebView is involved — the faces are memory
//! mapped from files that already exist on the machine, so a 20 MB system font
//! costs almost no private memory.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use eframe::egui;
use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum FontKind {
    /// Pick the face that matches the current UI language.
    #[default]
    Auto,
    Sans,
    Serif,
    Mono,
    /// Use [`crate::config::Config::font_path`].
    Custom,
}

struct Candidate {
    label: &'static str,
    files: &'static [&'static str],
    /// Scripts this face covers, used to pick a sensible fallback.
    cjk: bool,
}

const YAHEI: Candidate = Candidate {
    label: "Microsoft YaHei",
    files: &["msyh.ttc", "msyh.ttf", "MicrosoftYaHei.ttf"],
    cjk: true,
};
const JHENGHEI: Candidate = Candidate {
    label: "Microsoft JhengHei",
    files: &["msjh.ttc", "msjh.ttf"],
    cjk: true,
};
const DENGXIAN: Candidate = Candidate {
    label: "DengXian",
    files: &["Deng.ttf"],
    cjk: true,
};
const YU_GOTHIC: Candidate = Candidate {
    label: "Yu Gothic",
    files: &[
        "YuGothM.ttc",
        "YuGothR.ttc",
        "yugothm.ttc",
        "YuGothic-Medium.otf",
    ],
    cjk: true,
};
const MEIRYO: Candidate = Candidate {
    label: "Meiryo",
    files: &["meiryo.ttc"],
    cjk: true,
};
const MALGUN: Candidate = Candidate {
    label: "Malgun Gothic",
    files: &["malgun.ttf", "malgunbd.ttf"],
    cjk: true,
};
const NOTO_CJK: Candidate = Candidate {
    label: "Noto Sans CJK",
    files: &[
        "NotoSansCJK-Regular.ttc",
        "NotoSansCJKsc-Regular.otf",
        "NotoSansCJKjp-Regular.otf",
        "NotoSansCJKkr-Regular.otf",
        "NotoSansSC-Regular.otf",
        "NotoSansTC-Regular.otf",
    ],
    cjk: true,
};
const SOURCE_HAN: Candidate = Candidate {
    label: "Source Han Sans",
    files: &[
        "SourceHanSansSC-Regular.otf",
        "SourceHanSans-Regular.ttc",
        "SourceHanSansSC-Regular.ttc",
    ],
    cjk: true,
};
const PINGFANG: Candidate = Candidate {
    label: "PingFang SC",
    files: &["PingFang.ttc", "PingFangSC-Regular.otf"],
    cjk: true,
};
const HIRAGINO: Candidate = Candidate {
    label: "Hiragino Sans",
    files: &["Hiragino Sans GB.ttc", "ヒラギノ角ゴシック W3.ttc"],
    cjk: true,
};
const WENQUANYI: Candidate = Candidate {
    label: "WenQuanYi Micro Hei",
    files: &["wqy-microhei.ttc", "wqy-zenhei.ttc"],
    cjk: true,
};

const SEGOE_UI: Candidate = Candidate {
    label: "Segoe UI",
    files: &["segoeui.ttf", "SegUIVar.ttf", "segoeui.ttf"],
    cjk: false,
};
const SF_PRO: Candidate = Candidate {
    label: "SF Pro",
    files: &["SFNS.ttf", "SFNSDisplay.ttf", "Helvetica.ttc"],
    cjk: false,
};
const INTER: Candidate = Candidate {
    label: "Inter",
    files: &["Inter-Regular.otf", "Inter.ttf", "Inter-VariableFont.ttf"],
    cjk: false,
};
const DEJAVU: Candidate = Candidate {
    label: "DejaVu Sans",
    files: &["DejaVuSans.ttf"],
    cjk: false,
};
const TAHOMA: Candidate = Candidate {
    label: "Tahoma",
    files: &["tahoma.ttf"],
    cjk: false,
};
const ARIAL: Candidate = Candidate {
    label: "Arial",
    files: &["arial.ttf"],
    cjk: false,
};

const SERIFS: &[&Candidate] = &[
    &Candidate {
        label: "Songti SC",
        files: &["Songti.ttc", "STSong.ttf"],
        cjk: true,
    },
    &Candidate {
        label: "SimSun",
        files: &["simsun.ttc", "simsun.ttf"],
        cjk: true,
    },
    &Candidate {
        label: "Yu Mincho",
        files: &["yumin.ttf", "YuMincho.ttc"],
        cjk: true,
    },
    &Candidate {
        label: "Georgia",
        files: &["georgia.ttf"],
        cjk: false,
    },
    &Candidate {
        label: "Times New Roman",
        files: &["times.ttf", "Times New Roman.ttf"],
        cjk: false,
    },
    &Candidate {
        label: "DejaVu Serif",
        files: &["DejaVuSerif.ttf"],
        cjk: false,
    },
];

const MONOS: &[&Candidate] = &[
    &Candidate {
        label: "Cascadia Mono",
        files: &["CascadiaMono.ttf", "CascadiaCode.ttf"],
        cjk: false,
    },
    &Candidate {
        label: "Consolas",
        files: &["consola.ttf", "consolab.ttf"],
        cjk: false,
    },
    &Candidate {
        label: "JetBrains Mono",
        files: &["JetBrainsMono-Regular.ttf", "JetBrainsMono-Regular.otf"],
        cjk: false,
    },
    &Candidate {
        label: "Menlo",
        files: &["Menlo.ttc"],
        cjk: false,
    },
    &Candidate {
        label: "DejaVu Sans Mono",
        files: &["DejaVuSansMono.ttf"],
        cjk: false,
    },
    &Candidate {
        label: "Courier New",
        files: &["cour.ttf"],
        cjk: false,
    },
];

/// The result of a font installation, surfaced in the settings page.
#[derive(Debug, Clone, Default)]
pub struct FontReport {
    /// Human readable name of the face that ended up primary.
    pub primary: Option<String>,
    /// Extra face installed purely for CJK coverage.
    pub fallback: Option<String>,
    pub warnings: Vec<String>,
}

fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = dirs::font_dir() {
        dirs.push(dir);
    }
    #[cfg(windows)]
    {
        dirs.push(PathBuf::from(r"C:\Windows\Fonts"));
        if let Some(local) = dirs::data_local_dir() {
            dirs.push(local.join("Microsoft/Windows/Fonts"));
        }
    }
    #[cfg(target_os = "macos")]
    {
        dirs.push(PathBuf::from("/System/Library/Fonts"));
        dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        dirs.push(PathBuf::from("/Library/Fonts"));
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join("Library/Fonts"));
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        dirs.push(PathBuf::from("/usr/share/fonts"));
        dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Some(data) = dirs::data_dir() {
            dirs.push(data.join("fonts"));
        }
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".fonts"));
            dirs.push(home.join(".local/share/fonts"));
        }
    }
    dirs.retain(|d| d.is_dir());
    dirs.dedup();
    dirs
}

/// Lower-cased file name -> path, built once per process.
fn index() -> &'static Vec<(String, PathBuf)> {
    static INDEX: OnceLock<Vec<(String, PathBuf)>> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut out = Vec::new();
        for dir in font_dirs() {
            walk(&dir, 0, &mut out);
        }
        out
    })
}

/// Face chosen by the last [`apply`] call, for the rasteriser's benefit.
static PRIMARY_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();

fn walk(dir: &Path, depth: usize, out: &mut Vec<(String, PathBuf)>) {
    if depth > 3 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, depth + 1, out);
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".ttf")
            || lower.ends_with(".otf")
            || lower.ends_with(".ttc")
            || lower.ends_with(".otc")
        {
            out.push((lower, path));
        }
    }
}

fn find(candidate: &Candidate) -> Option<PathBuf> {
    let index = index();
    for file in candidate.files {
        let needle = file.to_ascii_lowercase();
        if let Some((_, path)) = index.iter().find(|(name, _)| *name == needle) {
            return Some(path.clone());
        }
    }
    None
}

/// First font file (by exact, lower-cased name) that exists on this machine.
/// Used as a last-resort Latin face for the text-annotation rasteriser.
pub fn find_system_font(names: &[&str]) -> Option<PathBuf> {
    let index = index();
    for name in names {
        let needle = name.to_ascii_lowercase();
        if let Some((_, path)) = index.iter().find(|(n, _)| *n == needle) {
            return Some(path.clone());
        }
    }
    None
}

/// Path of the face that ended up primary, recorded by [`apply`] so the
/// rasteriser can outline glyphs from the same file the UI renders with.
pub fn primary_font_path() -> Option<&'static PathBuf> {
    PRIMARY_PATH.get().and_then(|p| p.as_ref())
}

/// Every font family installed on this machine, name first.
///
/// Windows keeps the mapping in the registry (`…\Windows NT\CurrentVersion\Fonts`)
/// as `"Family Name (Type)" → file`, which is the only source that also covers
/// per-user installs and `.ttc` collections. Other platforms fall back to a
/// short list of well-known families, which is honest: there is no portable
/// registry to read.
pub fn families() -> Vec<(String, PathBuf)> {
    #[cfg(windows)]
    {
        use std::collections::BTreeMap;
        use windows::Win32::System::Registry::{
            HKEY, HKEY_LOCAL_MACHINE, KEY_READ, RegCloseKey, RegEnumValueW, RegOpenKeyExW,
            REG_SZ,
        };
        use windows::core::HSTRING;

        let mut out: BTreeMap<String, PathBuf> = BTreeMap::new();
        let key = HSTRING::from(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Fonts",
        );
        let mut handle = HKEY::default();
        unsafe {
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, &key, Some(0), KEY_READ, &mut handle).is_err() {
                return Vec::new();
            }
            let fonts_dir = windows_dir();
            for index in 0.. {
                let mut name = [0u16; 128];
                let mut name_len = name.len() as u32;
                let mut data = [0u16; 260];
                let mut data_len = data.len() as u32;
                let mut kind = REG_SZ.0;
                let status = RegEnumValueW(
                    handle,
                    index,
                    Some(windows::core::PWSTR(name.as_mut_ptr())),
                    &mut name_len,
                    None,
                    Some(&mut kind),
                    Some(data.as_mut_ptr() as *mut u8),
                    Some(&mut data_len),
                );
                if status.is_err() {
                    break;
                }
                let value = String::from_utf16_lossy(
                    &data[..(data_len as usize / 2).saturating_sub(1)],
                );
                // "Microsoft YaHei & Microsoft YaHei UI (TrueType)" → family
                let display = String::from_utf16_lossy(&name[..name_len as usize]);
                let family = display
                    .split(" (")
                    .next()
                    .unwrap_or(&display)
                    .split('&')
                    .map(str::trim)
                    .next()
                    .unwrap_or(&display)
                    .to_string();
                if family.is_empty() || value.is_empty() {
                    continue;
                }
                let path = if value.contains('\\') || value.contains('/') {
                    PathBuf::from(value)
                } else {
                    fonts_dir.join(value)
                };
                out.entry(family).or_insert(path);
            }
            let _ = RegCloseKey(handle);
        }
        out.into_iter().collect()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// `C://Windows//Fonts`, or empty when it cannot be resolved.
#[cfg(windows)]
fn windows_dir() -> PathBuf {
    use windows::Win32::System::SystemInformation::GetWindowsDirectoryW;
    let mut buf = [0u16; 260];
    let len = unsafe { GetWindowsDirectoryW(Some(&mut buf)) };
    if len == 0 {
        return PathBuf::from("C:////Windows");
    }
    PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])).join("Fonts")
}

/// Resolve a family name to a font file, or `None` when it is not installed.
pub fn family_path(name: &str) -> Option<PathBuf> {
    let wanted = name.trim().to_ascii_lowercase();
    families()
        .into_iter()
        .find(|(family, _)| family.to_ascii_lowercase() == wanted)
        .map(|(_, path)| path)
        .filter(|p| p.is_file())
}

fn first_of(candidates: &[&Candidate]) -> Option<(&'static str, PathBuf, bool)> {
    candidates
        .iter()
        .find_map(|c| find(c).map(|path| (c.label, path, c.cjk)))
}

fn language_font(lang: Lang) -> Option<(&'static str, PathBuf, bool)> {
    let order: &[&Candidate] = match lang {
        Lang::ZhHans => &[
            &YAHEI,
            &DENGXIAN,
            &NOTO_CJK,
            &SOURCE_HAN,
            &PINGFANG,
            &WENQUANYI,
        ],
        Lang::ZhHant => &[
            &JHENGHEI,
            &PINGFANG,
            &NOTO_CJK,
            &SOURCE_HAN,
            &YAHEI,
            &WENQUANYI,
        ],
        Lang::Ja => &[&YU_GOTHIC, &MEIRYO, &NOTO_CJK, &SOURCE_HAN, &HIRAGINO],
        Lang::Ko => &[&MALGUN, &NOTO_CJK, &SOURCE_HAN],
        _ => &[&SEGOE_UI, &SF_PRO, &INTER, &DEJAVU, &TAHOMA, &ARIAL],
    };
    first_of(order)
}

/// Pick a second face for CJK coverage. The order depends on the UI language:
/// a Chinese UI should not fall back to a Korean face, and a Latin UI should
/// prefer a Chinese one (it also covers kana and most hangul).
/// Faces that carry Hangul. `malgun.ttf` is the Windows default; the others
/// cover older Windows, macOS and Linux.
fn korean_fallback() -> Option<PathBuf> {
    find_system_font(&[
        "malgun.ttf",
        "malgunsl.ttf",
        "gulim.ttc",
        "batang.ttc",
        "AppleSDGothicNeo.ttc",
        "NotoSansKR-Regular.otf",
        "NotoSansCJK-Regular.ttc",
    ])
}

fn cjk_fallback(lang: Lang, primary_label: Option<&str>) -> Option<(&'static str, PathBuf)> {
    let cjk_primary: &[&Candidate] = match lang {
        Lang::ZhHans => &[&NOTO_CJK, &SOURCE_HAN, &JHENGHEI, &PINGFANG, &WENQUANYI],
        Lang::ZhHant => &[&NOTO_CJK, &SOURCE_HAN, &YAHEI, &PINGFANG, &WENQUANYI],
        Lang::Ja => &[&NOTO_CJK, &SOURCE_HAN, &MEIRYO, &HIRAGINO, &YAHEI],
        Lang::Ko => &[&NOTO_CJK, &SOURCE_HAN, &MALGUN, &YAHEI],
        _ => &[
            &YAHEI,
            &JHENGHEI,
            &NOTO_CJK,
            &SOURCE_HAN,
            &YU_GOTHIC,
            &MALGUN,
            &WENQUANYI,
        ],
    };
    cjk_primary.iter().find_map(|c| {
        if !c.cjk || Some(c.label) == primary_label {
            return None;
        }
        find(c).map(|path| (c.label, path))
    })
}

/// A system face that can render text the user typed themselves.
///
/// The embedded subsets cover every string the UI draws, but text *inputs*
/// (the save directory, a custom font path, an annotation) accept anything.
/// The face is chosen for coverage rather than for the UI language: a Chinese
/// or Korean path has to render even while the UI is in English. Preference
/// order favours faces that carry both CJK and Hangul, so one mapped file
/// answers both scripts.
fn user_text_fallback(lang: Lang) -> Option<(&'static str, PathBuf)> {
    // Noto Sans CJK and Source Han Sans carry Chinese, Japanese and Korean in
    // one file, so they are the best single choice when present.
    let wide: &[&Candidate] = &[&NOTO_CJK, &SOURCE_HAN];
    if let Some((label, path)) = wide.iter().find_map(|c| find(c).map(|p| (c.label, p))) {
        return Some((label, path));
    }
    // Otherwise fall back to the language's own CJK face, then a Korean one:
    // YaHei has no Hangul, Malgun Gothic has no Chinese, so both are worth
    // mapping when the script in play is the one they lack.
    let mut order: Vec<&Candidate> = Vec::new();
    match lang {
        Lang::Ko => order.push(&MALGUN),
        Lang::Ja => {
            order.push(&YU_GOTHIC);
            order.push(&MEIRYO);
        }
        Lang::ZhHant => order.push(&JHENGHEI),
        _ => order.push(&YAHEI),
    }
    order.push(&YAHEI);
    order.push(&MALGUN);
    order.iter().find_map(|c| find(c).map(|p| (c.label, p)))
}

/// A memory-mapped file. Derefs to the file bytes without copying them into
/// the private heap: system CJK fonts are 16–21 MB each, and a mapped file is
/// backed by the page cache (shareable, evictable) instead of a private
/// commit — which is what the Task Manager "Memory" column counts.
struct Mapped(memmap2::Mmap);

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn read_font(path: &Path) -> Result<egui::FontData, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // Safety: the mapping is read-only and the file is a system font that
    // nothing truncates while the app runs; the `Mmap` is kept alive by the
    // `Arc` inside the returned `FontData`.
    let map =
        unsafe { memmap2::Mmap::map(&file) }.map_err(|e| format!("{}: {e}", path.display()))?;
    let blob: Arc<dyn AsRef<[u8]> + Send + Sync> = Arc::new(Mapped(map));
    Ok(egui::FontData::from_blob(blob, 0))
}

/// Resolve and install fonts. Cheap enough to call on every settings change.
pub fn apply(
    ctx: &egui::Context,
    lang: Lang,
    kind: FontKind,
    custom_path: Option<&Path>,
    family: Option<&str>,
    size_pt: f32,
) -> FontReport {
    // 9 pt is the baseline the palette was designed around; every other size
    // is a multiplier on top of it.
    let scale = (size_pt / 9.0).clamp(0.7, 1.8);
    let mut report = FontReport::default();
    let mut defs = egui::FontDefinitions::default();

    // Default (Auto) face: small embedded subsets carrying exactly the glyphs
    // the UI renders, instead of memory-mapping the full ~19 MB system CJK
    // font. This is the single biggest memory win for a tool that sits idle in
    // the tray.
    //
    // The subsets are split across three faces because no single system font
    // covers everything the UI shows: YaHei has Latin + CJK + kana but no
    // Hangul (the Korean UI would be blank) and no dingbats (✓ ✗ ↔). They
    // are installed as one fallback chain, so a glyph missing from the first
    // face is picked up from the next.
    //
    // A mapped system font closes the chain: the user can type arbitrary text
    // (a Chinese save path, for instance) that no subset can predict. It is
    // memory-mapped, so it costs no private commit — only the glyphs actually
    // rasterised reach the atlas.
    //
    // Rebuild the subsets with tools/subset_ui_font.py after adding UI strings.
    if kind == FontKind::Auto {
        const SUBSET_MAIN: &[u8] = include_bytes!("../assets/pinsight_ui.ttf");
        const SUBSET_KO: &[u8] = include_bytes!("../assets/pinsight_ui_ko.ttf");
        const SUBSET_SYM: &[u8] = include_bytes!("../assets/pinsight_ui_sym.ttf");

        let embedded: [(&str, &[u8]); 3] = [
            ("pinsight-ui-main", SUBSET_MAIN),
            ("pinsight-ui-ko", SUBSET_KO),
            ("pinsight-ui-sym", SUBSET_SYM),
        ];
        // egui's default family already carries Ubuntu-Light and the emoji
        // fonts; those must stay *behind* the subsets, otherwise the default
        // Latin face would answer every Latin character and the subsets would
        // never be consulted. `insert(0, ..)` in reverse order leaves the
        // chain as main -> ko -> sym -> defaults.
        for (name, bytes) in embedded.iter().rev() {
            defs.font_data.insert(
                name.to_string(),
                Arc::new(egui::FontData::from_owned(bytes.to_vec())),
            );
            if let Some(family) = defs.families.get_mut(&egui::FontFamily::Proportional) {
                family.insert(0, name.to_string());
            }
            if let Some(family) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                family.insert(0, name.to_string());
            }
        }
        report.primary = Some("UI subset".to_string());

        // Last-resort face for user-typed text. The user can type anything —
        // a Chinese save path, a Korean folder name — and no subset can
        // predict that, so a system font closes the chain. It must cover CJK
        // and Hangul regardless of the UI language, otherwise an English UI
        // would show tofu for a Chinese path. Memory-mapped, so it costs no
        // private commit; only the glyphs actually rasterised reach the atlas.
        if let Some((label, path)) = user_text_fallback(lang) {
            if let Ok(data) = read_font(&path) {
                let name = "pinsight-ui-system-fallback".to_string();
                defs.font_data.insert(name.clone(), Arc::new(data));
                if let Some(family) = defs.families.get_mut(&egui::FontFamily::Proportional) {
                    family.push(name.clone());
                }
                if let Some(family) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                    family.push(name);
                }
                report.fallback = Some(format!("{label} (fallback)"));
            }
        }

        ctx.set_fonts(defs);
        // Text styles must be refreshed because the family chain changed.
        let styles = crate::theme::text_styles(scale);
        ctx.all_styles_mut(|style| style.text_styles = styles.clone());
        return report;
    }

    // `true` when the chosen face already renders CJK, in which case a second
    // CJK font would only cost 20 MB.
    let primary: Option<(&'static str, PathBuf, bool)> = match kind {
        // An explicitly chosen family always wins, so the settings page can
        // show exactly what the user picked.
        _ if family.is_some() => family
            .and_then(family_path)
            .map(|path| ("Custom", path, false)),
        FontKind::Auto => language_font(lang),
        FontKind::Sans => first_of(&[&SEGOE_UI, &SF_PRO, &INTER, &DEJAVU, &TAHOMA, &ARIAL])
            .or_else(|| language_font(lang)),
        FontKind::Serif => first_of(SERIFS),
        FontKind::Mono => first_of(MONOS),
        FontKind::Custom => match custom_path {
            // Coverage of a user file is unknown: assume it needs a fallback.
            Some(path) if path.is_file() => Some(("Custom", path.to_path_buf(), false)),
            Some(path) => {
                report.warnings.push(format!("{}", path.display()));
                language_font(lang)
            }
            None => language_font(lang),
        },
    };

    let mut primary_label: Option<String> = None;
    let mut primary_is_cjk = false;
    let primary_path = primary.as_ref().map(|(_, path, _)| path.clone());
    // Hangul coverage is a separate question from CJK: YaHei and PingFang have
    // Chinese and Japanese but no Korean, so a Korean face has to follow them.
    let primary_is_korean = primary_path
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .map(|n| {
            [
                "malgun",
                "gulim",
                "batang",
                "dotum",
                "gothicneo",
                "notosanskr",
                "nanum",
            ]
            .iter()
            .any(|hint| n.contains(hint))
        })
        .unwrap_or(false);
    if let Some((label, path, cjk)) = primary {
        match read_font(&path) {
            Ok(data) => {
                let name = format!("pinsight-primary-{}", defs.font_data.len());
                defs.font_data.insert(name.clone(), Arc::new(data));
                if let Some(family) = defs.families.get_mut(&egui::FontFamily::Proportional) {
                    family.insert(0, name.clone());
                }
                if kind == FontKind::Mono {
                    if let Some(family) = defs.families.get_mut(&egui::FontFamily::Monospace) {
                        family.insert(0, name);
                    }
                }
                primary_label = Some(label.to_string());
                primary_is_cjk = cjk;
            }
            Err(err) => report.warnings.push(err),
        }
    }

    // A CJK fallback is worth its footprint whenever the primary face cannot
    // supply those glyphs — not only when the UI language itself is CJK. The
    // language picker lists every language in its own script, so an English UI
    // still renders 简体中文 / 日本語 / 한국어 natively; without the fallback
    // those rows are tofu. The face is memory-mapped, so the extra font costs
    // no private commit — only the glyphs actually rasterised take atlas space.
    // Fallback chain: Chinese/Japanese faces carry no Hangul, so the language
    // picker (which lists 한국어 in its own script) would show tofu without a
    // Korean face behind them. Both are memory-mapped, so the extra coverage
    // costs no private commit — only the glyphs actually used reach the atlas.
    // egui copies font bytes into the heap, so every extra face is a real
    // memory cost (a CJK face is 15-20 MB). Only load what the UI language
    // actually needs: the language picker no longer lists names in their own
    // script, so a Latin UI needs no CJK face at all.
    let mut chain: Vec<(&'static str, PathBuf)> = Vec::new();
    if lang.needs_cjk() {
        if lang == Lang::Ko {
            if !primary_is_korean {
                if let Some(path) = korean_fallback() {
                    chain.push(("Korean fallback", path));
                }
            }
        } else if !primary_is_cjk {
            if let Some(found) = cjk_fallback(lang, primary_label.as_deref()) {
                chain.push(found);
            }
        }
    }
    for (label, path) in chain {
        if let Ok(data) = read_font(&path) {
            let name = format!("pinsight-fallback-{}", defs.font_data.len());
            defs.font_data.insert(name.clone(), Arc::new(data));
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                if let Some(list) = defs.families.get_mut(&family) {
                    // Before the icon fonts so CJK punctuation is not swallowed
                    // by the symbol font, after the primary face and any earlier
                    // fallback in the chain.
                    let at = list.len().saturating_sub(1);
                    list.insert(at, name.clone());
                }
            }
            if report.fallback.is_none() {
                report.fallback = Some(label.to_string());
            }
        }
    }

    ctx.set_fonts(defs);
    // Text styles must be refreshed because the family chain changed.
    let styles = crate::theme::text_styles(scale);
    ctx.all_styles_mut(|style| style.text_styles = styles.clone());

    // Remember which face won so the text rasteriser can outline glyphs from
    // the same file the UI is drawn with.
    let resolved = primary_path;
    let changed = PRIMARY_PATH.set(resolved.clone()).is_err();
    if changed || resolved.is_some() {
        crate::text_font::reload();
    }

    report.primary = primary_label;
    report
}
