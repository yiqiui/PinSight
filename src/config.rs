//! User configuration, stored as JSON in the platform config directory.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};

use crate::fonts::FontKind;
use crate::i18n::Lang;
use crate::theme::{Accent, ThemeMode};

pub const APP_DIR: &str = "PinSight";

/// How a custom background image is fitted into the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BgFit {
    /// Fill the window, cropping the overflow (like `background-size: cover`).
    Cover,
    /// Fit the whole image inside the window, letterboxing the rest.
    Contain,
    /// Ignore the aspect ratio and stretch to the window.
    Stretch,
}

/// How a copied image is placed on the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClipboardMode {
    /// Put the image bytes on the clipboard (PNG). Most apps accept this.
    #[default]
    Image,
    /// Also write the image to a file and put that file on the clipboard, so
    /// pasting into a chat app attaches the picture instead of inlining it.
    File,
    /// Do not touch the clipboard on copy.
    Disabled,
}

/// Which slice of the settings page is showing. Persisted so reopening the
/// window lands where the user left off instead of back on Appearance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsTab {
    General,
    #[default]
    Appearance,
    Capture,
    Pins,
    Output,
    /// Global hotkeys, in-app shortcuts and mouse bindings.
    #[serde(alias = "Hotkeys")]
    Controls,
    Updates,
    About,
}

/// Which slice of the Pins settings page is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PinsSub {
    /// Shadow, opacity, size, thumbnails.
    #[default]
    Display,
    /// Interaction rules: wheel zoom, close behaviour, groups.
    Behavior,
    /// Text-to-image conversion defaults.
    Convert,
}

/// How the update checker reaches the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProxyMode {
    /// Use the operating system's proxy settings.
    #[default]
    System,
    /// Connect directly, ignoring any system proxy.
    None,
    /// Route through an HTTP proxy.
    Http,
    /// Route through a SOCKS5 proxy.
    Socks5,
    /// Use the manually entered server, port and credentials.
    Manual,
}

/// What a double-click or middle-click inside the capture overlay does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClickAction {
    /// Copy the selection and close.
    #[default]
    Copy,
    /// Pin the selection and close.
    Pin,
    /// Save the selection to disk and close.
    Save,
    /// Do nothing (the click is ignored).
    Nothing,
}

impl ClickAction {
    pub const ALL: [Self; 4] = [Self::Copy, Self::Pin, Self::Save, Self::Nothing];
}

/// How the overlay prints the colour under the cursor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorFormat {
    /// `#RRGGBB`
    #[default]
    Hex,
    /// `rgb(12, 34, 56)`
    Rgb,
    /// `hsl(210, 40%, 60%)`
    Hsl,
}

impl ColorFormat {
    /// Format one pixel for display.
    pub fn render(self, rgb: [u8; 3]) -> String {
        match self {
            Self::Hex => format!("#{:02X}{:02X}{:02X}", rgb[0], rgb[1], rgb[2]),
            Self::Rgb => format!("rgb({}, {}, {})", rgb[0], rgb[1], rgb[2]),
            Self::Hsl => {
                let (h, s, l) = rgb_to_hsl(rgb);
                format!("hsl({h:.0}, {s:.0}%, {l:.0}%)")
            }
        }
    }
}

/// Hue/saturation/lightness in degrees and percent.
fn rgb_to_hsl(rgb: [u8; 3]) -> (f32, f32, f32) {
    let r = rgb[0] as f32 / 255.0;
    let g = rgb[1] as f32 / 255.0;
    let b = rgb[2] as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l * 100.0);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        ((g - b) / d + if g < b { 6.0 } else { 0.0 }) * 60.0
    } else if max == g {
        ((b - r) / d + 2.0) * 60.0
    } else {
        ((r - g) / d + 4.0) * 60.0
    };
    (h, s * 100.0, l * 100.0)
}

/// Which artwork the tray icon uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayIconStyle {
    /// Follow the taskbar theme: white on dark, colour on light.
    #[default]
    Auto,
    /// Monochrome white, for dark taskbars.
    White,
    /// The full-colour app icon.
    Color,
    /// A PNG picked by the user.
    Custom,
}

/// Verbosity of the log file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogLevel {
    Error,
    Warn,
    #[default]
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub const ALL: [Self; 5] = [
        Self::Error,
        Self::Warn,
        Self::Info,
        Self::Debug,
        Self::Trace,
    ];

    /// The value `tracing_subscriber`'s `EnvFilter` understands.
    pub fn as_filter(self) -> &'static str {
        match self {
            Self::Error => "pinsight=error",
            Self::Warn => "pinsight=warn",
            Self::Info => "pinsight=info",
            Self::Debug => "pinsight=debug",
            Self::Trace => "pinsight=trace",
        }
    }
}

/// What the recorder captures. Cross-platform: every mode works wherever
/// PinSight runs (Windows/macOS/Linux), because capture is `xcap`-backed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordMode {
    /// A region the user draws on screen (default).
    #[default]
    Region,
    /// The entire monitor under the cursor.
    Screen,
    /// The foreground window's rectangle.
    Window,
}

/// Output resolution preset for screen recordings (MP4 path only; GIF always
/// uses the source size).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecordResolution {
    /// Keep the captured size.
    #[default]
    Source,
    /// Scale down to 1920×1080.
    R1080,
    /// Scale down to 1280×720.
    R720,
}
impl RecordResolution {
    /// The target (width, height) for this preset, or `None` for source.
    pub fn size(self) -> Option<(u32, u32)> {
        match self {
            Self::Source => None,
            Self::R1080 => Some((1920, 1080)),
            Self::R720 => Some((1280, 720)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkeys: Hotkeys,
    pub save_dir: PathBuf,
    /// File-name template for saved captures, e.g.
    /// `PinSight_$yyyy-MM-dd_HH-mm-ss$`. See `crate::naming`.
    pub name_template: String,
    /// Image quality for lossy formats. 1–100; 100 is visually lossless.
    /// PNG ignores this value.
    pub image_quality: u8,
    /// Default file format for saved captures.
    pub image_format: crate::util::ImageFormat,
    /// How a copied image reaches the clipboard.
    pub clipboard_mode: ClipboardMode,
    pub gif_fps: u32,
    /// Hard wall-clock cap for a recording, in seconds. 0 = no limit (the
    /// recorder still stops after an hour so a forgotten session cannot fill
    /// the disk).
    pub record_max_seconds: u32,
    /// Countdown before a recording starts, in seconds (0/3/5). Lets the user
    /// position the mouse or focus a window before frame 0 lands.
    #[serde(default)]
    pub record_countdown: u32,
    /// Record desktop/microphone audio together with the screen capture.
    /// Requires `ffmpeg` (the recorder already shells out to it for MP4).
    /// Legacy single-toggle; superseded by `record_mic` / `record_system_audio`.
    pub record_audio: bool,
    /// Audio input device for screen recording. Empty = platform default
    /// (Windows: first DirectShow capture device, discovered at runtime).
    /// Device names are machine-specific, so users may need to set this.
    /// Legacy; superseded by `mic_device` / `system_audio_device`.
    pub audio_device: String,
    /// Recording frame rate, in frames per second (4–60).
    pub record_fps: u32,
    /// What the recorder captures: a drawn region, the whole screen, or the
    /// foreground window.
    pub record_mode: RecordMode,
    /// Output resolution preset. `Source` keeps the captured size; the others
    /// scale the frame down via ffmpeg (MP4 path only; GIF uses source).
    pub record_resolution: RecordResolution,
    /// Overlay the webcam as a picture-in-picture bubble (MP4 path only).
    pub record_webcam: bool,
    /// Webcam device name for the platform capture API
    /// (Windows/Linux: dshow `video="Name"`; macOS: avfoundation device index
    /// or name). Empty = first available.
    pub webcam_device: String,
    /// Capture desktop/system audio (Windows: WASAPI loopback; macOS/Linux: a
    /// monitor/loopback device the user has set up). Requires `ffmpeg`.
    pub record_system_audio: bool,
    /// System audio device for the platform capture API. Empty = default
    /// playback/monitor device.
    pub system_audio_device: String,
    /// Capture the microphone. Requires `ffmpeg`.
    pub record_mic: bool,
    /// Microphone device name. Empty = platform default input device.
    pub mic_device: String,
    /// Duration (seconds) of a voice-note recording triggered from the tray.
    pub voice_seconds: u32,
    pub long_capture_max_height: u32,
    /// Wheel notches posted per sampling step in auto mode. The overlay's
    /// manual mode always passes 0 — PinSight never scrolls there.
    pub long_scroll_ticks: u32,
    /// How long the stitcher waits after a scroll before sampling the frame,
    /// in milliseconds. Smooth-scrolling pages need the longer end.
    pub long_scroll_delay_ms: u32,
    /// Mean per-pixel difference (0–255 scale) below which a sample counts as
    /// "the page did not move". Higher ends the capture sooner; lower keeps
    /// sampling through blinking cursors and video.
    pub long_diff_threshold: f32,
    /// Consecutive still samples tolerated in auto mode before giving up.
    pub long_idle_limit: u32,
    /// The same tolerance for manual mode, where the user sets the pace and
    /// may stop to read — a short pause must not cut the capture.
    pub long_manual_idle_limit: u32,
    /// Hard cap on sampling rounds, so a stuck scroll cannot loop forever.
    pub long_max_frames: u32,
    /// Prefer the SDR (GDI) capture path on displays running in HDR mode,
    /// where the WGC pipeline returns washed-out frames.
    pub hdr_fix: bool,
    pub mcp_enabled: bool,
    // ---- upload ------------------------------------------------------------
    /// Generic image upload backend (HTTP to a configured URL, or a local
    /// folder copy). Disabled by default; see [`UploadConfig`].
    pub upload: UploadConfig,
    // ---- appearance -------------------------------------------------------
    /// `system` follows the operating system, `light` / `dark` force a theme.
    pub theme: ThemeMode,
    pub accent: Accent,
    /// RGB used when [`Config::accent`] is [`Accent::Custom`].
    pub accent_custom: [u8; 3],
    /// Recently used colours, shared by every colour field (max 16).
    pub custom_colors: Vec<[u8; 3]>,
    /// UI language. Defaults to the system UI language on first run.
    pub lang: Lang,
    pub font: FontKind,
    /// Chosen font family, or `None` to follow [`Config::font`].
    pub font_family: Option<String>,
    /// Interface font size in points; 9 is the palette baseline.
    pub font_size_pt: f32,
    /// Used when [`Config::font`] is [`FontKind::Custom`].
    pub font_path: Option<PathBuf>,
    /// Custom background image for the main window. `None` uses the flat theme
    /// colour.
    pub bg_image: Option<PathBuf>,
    /// Gaussian blur radius applied to the background image, in pixels.
    pub bg_blur: f32,
    /// Darkening veil over the background image, 0 (none) – 0.85 (heavy).
    pub bg_dim: f32,
    pub bg_fit: BgFit,
    // ---- window ------------------------------------------------------------
    /// Closing the window keeps the app in the tray instead of quitting.
    pub close_to_tray: bool,
    /// Launch PinSight when the user signs in.
    pub run_at_startup: bool,
    /// Keep a `config.json.bak` copy of the previous configuration.
    pub auto_backup_config: bool,
    /// Show the extended rows (pins submenu, restart, …) in the tray menu.
    pub enhanced_tray: bool,
    /// Verbosity of the log file.
    pub log_level: LogLevel,
    /// Settings page slice restored on the next launch.
    pub settings_tab: SettingsTab,
    /// Tray icon artwork.
    pub tray_icon_style: TrayIconStyle,
    /// PNG used when [`Config::tray_icon_style`] is [`TrayIconStyle::Custom`].
    pub tray_icon_path: Option<PathBuf>,
    // ---- capture (截屏) ----------------------------------------------------
    /// Selection border thickness in logical points.
    pub capture_border_width: f32,
    /// Dimming alpha outside the selection, 0 (clear) – 220 (near black).
    pub capture_mask_alpha: u8,
    /// RGB of the dimming veil outside the selection.
    pub capture_mask_color: [u8; 3],
    /// Draw the eight resize handles on the selection.
    pub capture_anchors: bool,
    /// RGB of the resize-handle outline.
    pub capture_anchor_color: [u8; 3],
    /// Full-screen crosshair that follows the cursor.
    pub capture_crosshair: bool,
    /// Hint line with the confirm/cancel shortcuts.
    pub capture_hints: bool,
    /// How many recent captures the history keeps.
    pub capture_history_limit: u32,
    /// Ask for confirmation before Escape discards the capture.
    pub capture_esc_confirm: bool,
    /// Action on a double-click inside the overlay.
    pub capture_double_click: ClickAction,
    /// Action on a middle-click inside the overlay.
    pub capture_middle_click: ClickAction,
    /// Play a short system sound when a capture is copied.
    pub capture_sound_copy: bool,
    /// Play a short system sound when a capture is saved.
    pub capture_sound_save: bool,
    /// Optional custom notification sound (WAV/MP3/OGG…). When set and the
    /// file exists it replaces the default system beep; `None` = system beep.
    pub sound_file: Option<PathBuf>,
    /// Zoomed pixel preview next to the cursor while selecting.
    pub capture_magnifier: bool,
    /// How many source pixels the magnifier covers, per axis.
    pub capture_magnifier_size: u32,
    /// Notation used for the colour under the cursor.
    pub color_format: ColorFormat,
    /// Composite the mouse cursor into full-screen captures.
    pub capture_pointer: bool,
    /// Seconds to wait after a capture is triggered before grabbing (0 = now).
    /// Lets the user open a menu, hover a tooltip or arrange a window first.
    pub capture_delay_secs: u32,
    // ---- save decorations (保存后处理) -----------------------------------
    /// Border / shadow / watermark applied to every saved capture.
    pub decorations: crate::decorate::Decorations,
    // ---- pins (贴图) ------------------------------------------------------
    /// Soft drop shadow around a pinned image.
    pub pin_shadow: bool,
    /// RGB of the drop shadow.
    pub pin_shadow_color: [u8; 3],
    /// Default opacity of a new pin, 20 – 100 %.
    pub pin_opacity: u8,
    /// Largest pin window edge in pixels.
    pub pin_max_size: u32,
    /// New pins start in click-through mode (clicks pass to the window below).
    pub pin_through: bool,
    /// Side of the thumbnails in the "recent pins" list.
    pub pin_thumb: u32,
    /// Mouse wheel over a pin zooms it.
    pub pin_wheel_zoom: bool,
    /// Zoom keeps the point under the cursor fixed.
    pub pin_zoom_at_cursor: bool,
    /// Zoom in finer steps for a smoother feel.
    pub pin_smooth_zoom: bool,
    /// Middle-click on a pin closes it.
    pub pin_middle_close: bool,
    /// Hide every pin at once (the tray's "show/hide all pins").
    pub pin_hidden: bool,
    /// A new pin reuses the position of the capture it came from.
    pub pin_keep_position: bool,
    /// Text pasted into a pin is rendered as an image instead of staying text.
    pub pin_text_to_image: bool,
    /// Padding around text rendered by the text-to-image converter, in pixels.
    pub pin_text_margin: u32,
    /// Widest a text-to-image conversion may get, in pixels.
    pub pin_text_max_width: u32,
    /// Font size used when rasterising text, in pixels.
    pub pin_text_font_size: u32,
    /// Page colour behind rasterised text.
    pub pin_text_bg: [u8; 3],
    /// Glyph colour of rasterised text.
    pub pin_text_fg: [u8; 3],
    /// Do not turn a bare file path into a pin.
    pub pin_skip_file_paths: bool,
    /// How many closed pins can be brought back, 0 disables the feature.
    pub pin_restore_limit: u32,
    // ---- controls (控制) --------------------------------------------------
    /// In-app (overlay) shortcuts.
    pub app_keys: AppKeys,
    /// Mouse bindings for pin windows.
    pub pin_gestures: PinGestures,
    /// Named groups pins can belong to. Always has at least one entry.
    pub pin_groups: Vec<PinGroup>,
    /// Global hotkeys are live. The tray's "disable hotkeys" flips this
    /// without losing the bindings themselves.
    pub hotkeys_enabled: bool,
    // ---- updates (更新) ---------------------------------------------------
    pub update_on_start: bool,
    pub update_hours: u32,
    pub update_beta: bool,
    /// How the update check reaches the network.
    pub proxy_mode: ProxyMode,
    /// Proxy host name or address, used when `proxy_mode` is `Manual`.
    pub proxy_server: String,
    /// Proxy port, used when `proxy_mode` is `Manual`.
    pub proxy_port: u16,
    /// Optional user name for an authenticated proxy.
    pub proxy_user: String,
    /// Optional password for an authenticated proxy.
    pub proxy_password: String,
    // ---- translation (翻译) -----------------------------------------------
    /// Provider + target language for the OCR panel's translate button.
    pub translate: TranslatePrefs,
}

/// Translation settings for the OCR panel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslatePrefs {
    /// Provider id: `google` or `deepl` (see `translate::Provider::parse`).
    pub provider: String,
    /// Target language, a BCP-47 code such as `zh-CN`, `en`, `ja`.
    pub target: String,
    /// DeepL auth key; also used as the optional key for custom endpoints.
    pub key: String,
    /// Full URL of a LibreTranslate-compatible endpoint for the custom
    /// provider (e.g. `https://translate.example.com/translate`).
    pub endpoint: String,
}

impl Default for TranslatePrefs {
    fn default() -> Self {
        Self {
            provider: "google".into(),
            target: "zh-CN".into(),
            key: String::new(),
            endpoint: String::new(),
        }
    }
}

/// Modifier half of a mouse binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureMod {
    #[default]
    None,
    Ctrl,
    Shift,
    Alt,
}

impl GestureMod {
    pub const ALL: [Self; 4] = [Self::None, Self::Ctrl, Self::Shift, Self::Alt];
}

/// Button half of a mouse binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GestureButton {
    /// The wheel; a click gesture never matches it.
    Wheel,
    Left,
    #[default]
    Middle,
    Right,
}

impl GestureButton {
    pub const ALL: [Self; 4] = [Self::Wheel, Self::Left, Self::Middle, Self::Right];
}

/// One mouse binding: modifier + button, optionally on a double-click.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Gesture {
    pub modifier: GestureMod,
    pub button: GestureButton,
    /// Fire on the double-click of the button instead of a single click.
    #[serde(default)]
    pub double_click: bool,
}

impl Default for Gesture {
    fn default() -> Self {
        Self {
            modifier: GestureMod::None,
            button: GestureButton::Middle,
            double_click: false,
        }
    }
}

impl Gesture {
    /// Pack into a `u32` so the native pin window can hold the whole table in
    /// one fixed-size array.
    pub fn encode(self) -> u32 {
        (self.modifier as u32) << 16 | (self.button as u32) << 8 | u32::from(self.double_click)
    }

    pub fn decode(value: u32) -> Self {
        let modifier = match (value >> 16) & 0xFF {
            1 => GestureMod::Ctrl,
            2 => GestureMod::Shift,
            3 => GestureMod::Alt,
            _ => GestureMod::None,
        };
        let button = match (value >> 8) & 0xFF {
            0 => GestureButton::Wheel,
            1 => GestureButton::Left,
            3 => GestureButton::Right,
            _ => GestureButton::Middle,
        };
        Self {
            modifier,
            button,
            double_click: value & 0xFF != 0,
        }
    }
}

/// A named group of pins.
///
/// Groups live in the config because their names and hidden state should
/// survive a restart; the pins themselves are windows and therefore ephemeral.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinGroup {
    pub id: u32,
    /// Empty means "use the translated default name".
    #[serde(default)]
    pub name: String,
    /// Hides every pin in the group at once.
    #[serde(default)]
    pub hidden: bool,
    /// Global hotkey that toggles this group's visibility. Empty = unbound.
    #[serde(default)]
    pub hotkey: String,
}

/// Mouse bindings for pin windows.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PinGestures {
    /// Zoom the pin (Snipaste: wheel, no modifier).
    pub zoom: Gesture,
    /// Change opacity (Snipaste: Ctrl + wheel).
    pub opacity: Gesture,
    /// Close the pin (Snipaste: double-click).
    pub close: Gesture,
    /// Reset zoom and opacity (Snipaste: middle click).
    pub reset: Gesture,
    /// Shrink to a thumbnail (Snipaste: Shift + double-click).
    pub thumbnail: Gesture,
    /// Copy the pin's text to the clipboard (Snipaste: Shift + right click).
    pub copy_text: Gesture,
}

impl Default for PinGestures {
    fn default() -> Self {
        Self {
            zoom: Gesture {
                modifier: GestureMod::None,
                button: GestureButton::Wheel,
                double_click: false,
            },
            opacity: Gesture {
                modifier: GestureMod::Ctrl,
                button: GestureButton::Wheel,
                double_click: false,
            },
            close: Gesture {
                modifier: GestureMod::None,
                button: GestureButton::Left,
                double_click: true,
            },
            reset: Gesture {
                modifier: GestureMod::None,
                button: GestureButton::Middle,
                double_click: false,
            },
            thumbnail: Gesture {
                modifier: GestureMod::Shift,
                button: GestureButton::Left,
                double_click: true,
            },
            copy_text: Gesture {
                modifier: GestureMod::Shift,
                button: GestureButton::Right,
                double_click: false,
            },
        }
    }
}

impl PinGestures {
    /// Encoded table, in [`PIN_GESTURE_ACTIONS`] order.
    pub fn encoded(&self) -> [u32; 6] {
        [
            self.zoom.encode(),
            self.opacity.encode(),
            self.close.encode(),
            self.reset.encode(),
            self.thumbnail.encode(),
            self.copy_text.encode(),
        ]
    }
}

/// Shortcuts used inside the capture overlay.
///
/// Stored as text ("V", "Ctrl+Z") so the config stays readable and a user can
/// type a combination instead of recording one. [`crate::app_keys::parse`]
/// turns them into egui keys; duplicate detection happens in the settings page.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppKeys {
    pub tool_select: String,
    pub tool_rect: String,
    pub tool_ellipse: String,
    pub tool_arrow: String,
    pub tool_line: String,
    pub tool_pen: String,
    pub tool_highlight: String,
    pub tool_mosaic: String,
    pub tool_numbered: String,
    pub tool_text: String,
    pub tool_measure: String,
    pub tool_lasso: String,
    pub confirm: String,
    pub pin: String,
    pub copy: String,
    pub save: String,
    pub cancel: String,
    pub undo: String,
    pub redo: String,
}

impl Default for AppKeys {
    fn default() -> Self {
        use crate::annotate::Tool;
        // Keep the built-in shortcuts in one place: `Tool::shortcut()` is the
        // source of truth for the defaults, this struct only stores overrides.
        let key = |tool: Tool| tool.shortcut().to_string();
        Self {
            tool_select: key(Tool::Select),
            tool_rect: key(Tool::Rect),
            tool_ellipse: key(Tool::Ellipse),
            tool_arrow: key(Tool::Arrow),
            tool_line: key(Tool::Line),
            tool_pen: key(Tool::Pen),
            tool_highlight: key(Tool::Highlight),
            tool_mosaic: key(Tool::Mosaic),
            tool_numbered: key(Tool::Numbered),
            tool_text: key(Tool::Text),
            tool_measure: key(Tool::Measure),
            tool_lasso: key(Tool::Lasso),
            confirm: "Enter".into(),
            pin: "P".into(),
            copy: "Ctrl+C".into(),
            save: "Ctrl+S".into(),
            cancel: "Esc".into(),
            undo: "Ctrl+Z".into(),
            redo: "Ctrl+Y".into(),
        }
    }
}

impl AppKeys {
    /// Every binding in settings-row order, for duplicate checks.
    pub fn all(&self) -> [(&'static str, &str); 19] {
        [
            ("tool_select", &self.tool_select),
            ("tool_rect", &self.tool_rect),
            ("tool_ellipse", &self.tool_ellipse),
            ("tool_arrow", &self.tool_arrow),
            ("tool_line", &self.tool_line),
            ("tool_pen", &self.tool_pen),
            ("tool_highlight", &self.tool_highlight),
            ("tool_mosaic", &self.tool_mosaic),
            ("tool_numbered", &self.tool_numbered),
            ("tool_text", &self.tool_text),
            ("tool_measure", &self.tool_measure),
            ("tool_lasso", &self.tool_lasso),
            ("confirm", &self.confirm),
            ("pin", &self.pin),
            ("copy", &self.copy),
            ("save", &self.save),
            ("cancel", &self.cancel),
            ("undo", &self.undo),
            ("redo", &self.redo),
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub capture_area: String,
    pub capture_full: String,
    pub pin_clipboard: String,
    pub long_capture: String,
    pub record: String,
    pub save_last: String,
    /// Show or hide every open pin (Snipaste-style F3 toggle).
    #[serde(default = "default_toggle_pins_hotkey")]
    pub toggle_pins: String,
    /// Re-grab the last selection straight to the clipboard.
    #[serde(default = "default_repeat_capture_hotkey")]
    pub repeat_capture: String,
    /// Open the screen colour picker (Snipaste-style eyedropper).
    #[serde(default = "default_pick_color_hotkey")]
    pub pick_color: String,
}

fn default_pick_color_hotkey() -> String {
    "Ctrl+Shift+K".into()
}

fn default_repeat_capture_hotkey() -> String {
    "Ctrl+Shift+P".into()
}

fn default_toggle_pins_hotkey() -> String {
    "Ctrl+Shift+H".into()
}

impl Hotkeys {
    /// Shortcut text bound to an action, used in status messages.
    pub fn for_action(&self, action: crate::hotkeys::Action) -> &str {
        use crate::hotkeys::Action;
        match action {
            Action::CaptureArea => &self.capture_area,
            Action::CaptureFull => &self.capture_full,
            Action::PinClipboard => &self.pin_clipboard,
            Action::LongCapture => &self.long_capture,
            Action::Record => &self.record,
            Action::SaveLast => &self.save_last,
            Action::TogglePins => &self.toggle_pins,
            Action::RepeatCapture => &self.repeat_capture,
            Action::PickColor => &self.pick_color,
            // Group hotkeys live on the groups, not here; the status readout
            // never needs their text.
            Action::ToggleGroup(_) => "",
        }
    }

    /// Every binding in settings-row order, for conflict checks.
    pub fn all(&self) -> [&str; 9] {
        [
            &self.capture_area,
            &self.capture_full,
            &self.pin_clipboard,
            &self.long_capture,
            &self.record,
            &self.save_last,
            &self.toggle_pins,
            &self.repeat_capture,
            &self.pick_color,
        ]
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            save_dir: default_save_dir(),
            name_template: crate::naming::DEFAULT_TEMPLATE.to_string(),
            image_quality: 90,
            image_format: crate::util::ImageFormat::Png,
            clipboard_mode: ClipboardMode::Image,
            gif_fps: 12,
            record_max_seconds: 0,
            record_countdown: 3,
            record_audio: false,
            audio_device: String::new(),
            record_fps: 30,
            record_mode: RecordMode::Region,
            record_resolution: RecordResolution::Source,
            record_webcam: false,
            webcam_device: String::new(),
            record_system_audio: false,
            system_audio_device: String::new(),
            record_mic: false,
            mic_device: String::new(),
            voice_seconds: 10,
            long_capture_max_height: 20_000,
            long_scroll_ticks: 4,
            long_scroll_delay_ms: 420,
            long_diff_threshold: 1.5,
            long_idle_limit: 8,
            long_manual_idle_limit: 30,
            long_max_frames: 400,
            hdr_fix: true,
            mcp_enabled: false,
            upload: UploadConfig::default(),
            theme: ThemeMode::System,
            accent: Accent::Blue,
            accent_custom: [0x5B, 0x9D, 0xFF],
            custom_colors: Vec::new(),
            lang: Lang::detect(),
            font: FontKind::Auto,
            font_family: None,
            font_size_pt: 9.0,
            font_path: None,
            bg_image: None,
            bg_blur: 12.0,
            bg_dim: 0.45,
            bg_fit: BgFit::Cover,
            close_to_tray: cfg!(windows),
            run_at_startup: false,
            auto_backup_config: true,
            enhanced_tray: true,
            log_level: LogLevel::Info,
            settings_tab: SettingsTab::default(),
            tray_icon_style: TrayIconStyle::Auto,
            tray_icon_path: None,
            capture_border_width: 1.5,
            capture_mask_alpha: 120,
            capture_mask_color: [0, 0, 0],
            capture_anchors: true,
            capture_anchor_color: [255, 255, 255],
            capture_crosshair: true,
            capture_hints: true,
            capture_history_limit: 20,
            capture_esc_confirm: false,
            capture_double_click: ClickAction::Copy,
            capture_middle_click: ClickAction::Pin,
            capture_sound_copy: false,
            capture_sound_save: false,
            sound_file: None,
            capture_magnifier: true,
            capture_magnifier_size: 15,
            color_format: ColorFormat::Hex,
            capture_pointer: false,
            capture_delay_secs: 0,
            decorations: crate::decorate::Decorations::default(),
            pin_shadow: true,
            pin_shadow_color: [0, 0, 0],
            pin_opacity: 100,
            pin_max_size: 4000,
            pin_through: false,
            pin_thumb: 64,
            pin_wheel_zoom: true,
            pin_zoom_at_cursor: true,
            pin_smooth_zoom: true,
            pin_middle_close: true,
            pin_hidden: false,
            pin_keep_position: true,
            pin_text_to_image: true,
            pin_text_margin: 12,
            pin_text_max_width: 750,
            pin_text_font_size: 18,
            pin_text_bg: [255, 255, 255],
            pin_text_fg: [17, 17, 17],
            pin_skip_file_paths: false,
            pin_restore_limit: 5,
            app_keys: AppKeys::default(),
            pin_gestures: PinGestures::default(),
            pin_groups: vec![PinGroup {
                id: 1,
                name: String::new(),
                hidden: false,
                hotkey: String::new(),
            }],
            hotkeys_enabled: true,
            update_on_start: true,
            update_hours: 24,
            update_beta: false,
            proxy_mode: ProxyMode::System,
            proxy_server: String::new(),
            proxy_port: 0,
            proxy_user: String::new(),
            proxy_password: String::new(),
            translate: TranslatePrefs::default(),
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        // Ctrl+Shift+* avoids the very common Ctrl+Alt+A screenshot shortcut
        // used by Snipaste, QQ, WeChat and others. Users can rebind everything.
        Self {
            capture_area: "Ctrl+Shift+A".into(),
            capture_full: "Ctrl+Shift+F".into(),
            pin_clipboard: "Ctrl+Shift+V".into(),
            long_capture: "Ctrl+Shift+L".into(),
            record: "Ctrl+Shift+R".into(),
            save_last: "Ctrl+Shift+S".into(),
            toggle_pins: default_toggle_pins_hotkey(),
            repeat_capture: default_repeat_capture_hotkey(),
            pick_color: default_pick_color_hotkey(),
        }
    }
}

/// Where PinSight keeps its configuration and data files.
#[derive(Debug, Clone)]
pub struct Paths {
    /// `config.json` location.
    pub config: PathBuf,
    /// Directory for user supplied assets (background images).
    pub data_dir: PathBuf,
    /// True when everything lives next to the executable.
    pub portable: bool,
}

impl Paths {
    /// Resolve the paths for this process.
    ///
    /// Portable mode is requested with `--portable`, `PINSIGHT_PORTABLE=1`, or a
    /// `pinsight.portable` marker file next to the executable. In that mode no
    /// registry entries and no `%APPDATA%` folders are touched, which is what a
    /// USB-stick or single-folder install wants.
    pub fn resolve() -> Self {
        let flag = std::env::args().any(|a| a == "--portable" || a == "-p")
            || std::env::var_os("PINSIGHT_PORTABLE").is_some_and(|v| v != "0" && !v.is_empty());

        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from));

        let marker = exe_dir
            .as_ref()
            .map(|d| d.join("pinsight.portable"))
            .is_some_and(|p| p.exists());

        if let (true, Some(dir)) = (flag || marker, exe_dir) {
            return Self {
                config: dir.join("data").join("config.json"),
                data_dir: dir.join("data"),
                portable: true,
            };
        }

        // A user-chosen folder (GUI) or `PINSIGHT_CONFIG_DIR` wins over the
        // default `%APPDATA%\PinSight`.
        let root = custom_root().unwrap_or_else(default_root);
        Self {
            config: root.join("config.json"),
            data_dir: root,
            portable: false,
        }
    }
}

/// File in the default folder that points at a user-chosen config folder.
const DIR_POINTER: &str = "config_dir.txt";

/// The default config folder: `%APPDATA%\PinSight` (or the platform equivalent).
fn default_root() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

/// A user-chosen config folder, from the env var or the pointer file.
fn custom_root() -> Option<PathBuf> {
    if let Some(v) = std::env::var_os("PINSIGHT_CONFIG_DIR") {
        let p = PathBuf::from(v);
        if !p.as_os_str().is_empty() {
            return Some(p);
        }
    }
    let pointer = default_root().join(DIR_POINTER);
    let text = std::fs::read_to_string(pointer).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Recursively copy `src` into `dst`, returning how many files were copied.
fn copy_dir_contents(src: &Path, dst: &Path) -> std::io::Result<usize> {
    let mut count = 0;
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == DIR_POINTER {
            continue;
        }
        let path = entry.path();
        let target = dst.join(&name);
        if path.is_dir() {
            count += copy_dir_contents(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
            count += 1;
        }
    }
    Ok(count)
}

/// Switch to `new_dir`, optionally copying the current files over first, and
/// record the choice so the next launch resolves it. Returns the file count
/// migrated. Takes effect after a restart (paths are resolved once at startup).
pub fn set_custom_dir(new_dir: &Path, migrate: bool) -> std::io::Result<usize> {
    let current = paths().data_dir.clone();
    std::fs::create_dir_all(new_dir)?;
    let mut copied = 0;
    if migrate && current != new_dir && current.exists() {
        copied = copy_dir_contents(&current, new_dir)?;
    }
    let pointer = default_root().join(DIR_POINTER);
    if let Some(parent) = pointer.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pointer, new_dir.display().to_string())?;
    Ok(copied)
}

/// Paths for this process, resolved once.
pub fn paths() -> &'static Paths {
    static PATHS: OnceLock<Paths> = OnceLock::new();
    PATHS.get_or_init(Paths::resolve)
}

/// True when running in portable mode (settings page shows it).
pub fn is_portable() -> bool {
    paths().portable
}

/// Generic image upload backend. PinSight hard-codes no host: the HTTP backend
/// POSTs/PUTs to a URL you configure (covering image hosts, OSS/S3 presigned
/// URLs and WebDAV), and the local backend just copies the file into a folder.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UploadConfig {
    /// Master switch. When off, the editor hides the Upload action and the CLI
    /// `--upload` flag reports a clear error instead of silently doing nothing.
    pub enabled: bool,
    /// Which backend to use.
    pub backend: UploadBackend,
    /// HTTP backend settings (used when `backend == http`).
    pub http: HttpUpload,
    /// Local backend settings (used when `backend == local`).
    pub local: LocalUpload,
}

impl Default for UploadConfig {
    fn default() -> Self {
        // Upload works out of the box with the local backend: it copies the
        // capture into `<save_dir>/uploaded` and hands back the path, no
        // network or host configuration required. Switch to HTTP in
        // Settings → Output → Upload to post to an image host / OSS / WebDAV.
        Self {
            enabled: true,
            backend: UploadBackend::Local,
            http: HttpUpload::default(),
            local: LocalUpload::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadBackend {
    /// POST/PUT multipart (or raw) to a configured URL.
    #[default]
    Http,
    /// Copy the file into a local folder (no network).
    Local,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HttpUpload {
    /// Target URL.
    pub url: String,
    /// HTTP method.
    pub method: HttpMethod,
    /// Form field name when sending multipart/form-data (ignored for raw PUT).
    pub field: String,
    /// Extra request headers, one per entry as `Name: Value`.
    pub headers: Vec<String>,
    /// How to read the resulting link from the response body.
    pub response: UploadResponse,
    /// When `response == json`, the (possibly nested, "."-separated) key that
    /// holds the shareable URL. Ignored otherwise.
    pub json_field: String,
    /// Connect/read timeout in seconds.
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HttpMethod {
    #[default]
    Post,
    Put,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UploadResponse {
    /// Response body is the URL (or plain text containing it).
    #[default]
    Raw,
    /// Response body is JSON; `json_field` names the URL key.
    Json,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalUpload {
    /// Destination directory, created on first use.
    pub folder: PathBuf,
}

impl Config {
    pub fn path() -> PathBuf {
        paths().config.clone()
    }

    /// Load the configuration from disk. Returns the parsed config plus — when
    /// the on-disk file existed but failed to parse — the path the malformed
    /// file was moved to so it is not lost. `None` on the second element means
    /// the file was missing (fresh install; defaults are materialised) or parsed
    /// cleanly. The caller uses that path to tell the user their settings were
    /// reset and where the bad file went.
    pub fn load_detailed() -> (Self, Option<PathBuf>) {
        let path = Self::path();
        let (mut config, corrupt) = read_config(&path);
        // A fresh install (or a config that was reset) has no file yet; write
        // the defaults so the settings page and future saves have something to
        // read back.
        if !path.exists() {
            if let Err(err) = config.save() {
                tracing::warn!(%err, "failed to write default config");
            }
        }
        // Migrate the legacy single audio toggle into the new microphone field
        // so existing users keep recording audio without reconfiguring. The
        // legacy default capture device (Windows dshow) is a microphone, so the
        // natural home is `record_mic`.
        if !config.record_mic && config.record_audio {
            config.record_mic = true;
            if config.mic_device.is_empty() {
                config.mic_device = config.audio_device.clone();
            }
        }
        (config, corrupt)
    }

    /// Load the configuration, discarding where the malformed file was kept.
    pub fn load() -> Self {
        Self::load_detailed().0
    }

    /// Load the configuration for a headless (CLI) subcommand. Behaves like
    /// [`Self::load`] but prints a warning to stderr when the on-disk file was
    /// corrupt and had to be reset, so a script or pipeline operator is not
    /// left wondering why every setting came back as a default.
    pub fn load_or_warn() -> Self {
        let (cfg, corrupt) = Self::load_detailed();
        if let Some(path) = corrupt {
            eprintln!(
                "pinsight: config file was corrupt and has been reset to defaults; \
                 the bad file was moved to {}",
                path.display()
            );
        }
        cfg
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Self::path();
        // Keep the previous file around so a bad edit is recoverable.
        if self.auto_backup_config && path.exists() {
            let backup = path.with_extension("json.bak");
            let _ = std::fs::copy(&path, backup);
        }
        let text = serde_json::to_string_pretty(self)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        // Write to a sibling `*.tmp` first, then rename over the target. Rename
        // is atomic on a single volume, so a crash or power loss mid-write can
        // no longer leave a half-written config that the next launch would fail
        // to parse and silently discard — which would wipe every setting.
        write_json_atomic(&path, &text)
    }

    pub fn ensure_save_dir(&self) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(&self.save_dir)?;
        Ok(self.save_dir.clone())
    }
}

/// Read and parse the config at `path`. On a parse error the existing file is
/// moved aside to `<path>.corrupt-<timestamp>` — never overwritten in place — so
/// the user can recover their settings; defaults are returned instead, together
/// with the path the bad file was moved to. A missing file yields defaults with
/// no disk side effect. Kept as a free function so it can be unit-tested with an
/// arbitrary path, independent of the global resolved config location.
fn read_config(path: &Path) -> (Config, Option<PathBuf>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (Config::default(), None);
    };
    match serde_json::from_str::<Config>(&text) {
        Ok(cfg) => (cfg, None),
        Err(err) => {
            tracing::warn!(
                ?path, %err,
                "config failed to parse; resetting to defaults and preserving the bad file"
            );
            let corrupt = preserve_corrupt(path);
            (Config::default(), corrupt)
        }
    }
}

/// Move a malformed config file aside so a subsequent save does not destroy it,
/// returning the new path (or `None` if it could not be moved).
fn preserve_corrupt(path: &Path) -> Option<PathBuf> {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    let corrupt = path.with_extension(format!("json.corrupt-{ts}"));
    std::fs::rename(path, &corrupt).ok()?;
    Some(corrupt)
}

/// Write `text` to `path` atomically: the bytes go to a `*.tmp` sibling first,
/// then it is renamed over the target. Rename is atomic on a single volume, so
/// the file is never observed half-written. Any temp file left behind on failure
/// is removed.
fn write_json_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("json.tmp");
    let result = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

pub fn default_save_dir() -> PathBuf {
    // In portable mode captures land next to the executable so the folder can be
    // moved or deleted as a unit.
    let paths = paths();
    if paths.portable {
        if let Some(dir) = paths.data_dir.parent() {
            return dir.join("captures");
        }
    }
    dirs::picture_dir()
        .or_else(dirs::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(APP_DIR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_config_missing_new_fields_still_loads() {
        // A config written before `endpoint`, `tool_line` and `tool_lasso`
        // existed must not fail to deserialize on upgrade.
        let legacy_translate = r#"{"provider":"deepl","target":"zh-CN","key":"secret"}"#;
        let prefs: TranslatePrefs = serde_json::from_str(legacy_translate).unwrap();
        assert_eq!(prefs.provider, "deepl");
        assert_eq!(prefs.key, "secret");
        assert_eq!(prefs.endpoint, "");

        let legacy_keys = r#"{"tool_select":"V","tool_rect":"R","confirm":"Enter"}"#;
        let keys: AppKeys = serde_json::from_str(legacy_keys).unwrap();
        assert_eq!(keys.tool_select, "V");
        assert_eq!(keys.tool_line, "L");
        assert_eq!(keys.tool_lasso, "F");
        assert_eq!(keys.confirm, "Enter");
    }

    #[test]
    fn app_keys_cover_every_tool_once() {
        use crate::annotate::Tool;
        // Each tool has exactly one configurable binding, and the defaults in
        // AppKeys must match Tool::shortcut() (the toolbar shows the bindings).
        let keys = AppKeys::default();
        let bindings = [
            keys.tool_select,
            keys.tool_rect,
            keys.tool_ellipse,
            keys.tool_arrow,
            keys.tool_line,
            keys.tool_pen,
            keys.tool_highlight,
            keys.tool_mosaic,
            keys.tool_numbered,
            keys.tool_text,
            keys.tool_measure,
            keys.tool_lasso,
        ];
        for (i, tool) in Tool::ALL.iter().enumerate() {
            assert_eq!(bindings[i], tool.shortcut(), "binding for {tool:?}");
        }
    }

    #[test]
    fn shipped_example_config_deserializes() {
        // The example file is documentation users copy directly: it must stay
        // loadable as a real Config, including all the newly added sections.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config.example.json");
        let text = std::fs::read_to_string(path).unwrap();
        let cfg: Config = serde_json::from_str(&text).unwrap();
        assert_eq!(cfg.hotkeys.pick_color, "Ctrl+Shift+K");
        assert_eq!(cfg.app_keys.tool_line, "L");
        assert_eq!(cfg.app_keys.tool_lasso, "F");
        assert_eq!(cfg.decorations.rounded, Some(12));
        assert_eq!(cfg.translate.provider, "google");
        assert_eq!(cfg.translate.endpoint, "");
    }

    #[test]
    fn read_config_returns_defaults_when_missing() {
        let dir = std::env::temp_dir().join(format!("ps-cfg-missing-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        let (cfg, corrupt) = read_config(&path);
        assert!(corrupt.is_none());
        assert_eq!(cfg.image_quality, 90, "default survived");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_config_preserves_a_corrupt_file_and_resets() {
        let dir = std::env::temp_dir().join(format!("ps-cfg-corrupt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(&path, "{ this is not valid json ").unwrap();
        let (cfg, corrupt) = read_config(&path);
        // Defaults, and the bad file was moved aside rather than overwritten.
        assert_eq!(cfg.image_quality, 90);
        let Some(bad) = corrupt else {
            panic!("expected the corrupt file to be preserved");
        };
        assert!(bad.exists(), "preserved file should exist");
        assert!(!path.exists(), "original should have been moved aside");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_config_parses_a_valid_override() {
        let dir = std::env::temp_dir().join(format!("ps-cfg-valid-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        std::fs::write(&path, r#"{"image_quality":55}"#).unwrap();
        let (cfg, corrupt) = read_config(&path);
        assert!(corrupt.is_none());
        assert_eq!(cfg.image_quality, 55);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_json_atomic_roundtrips_and_cleans_temp() {
        let dir = std::env::temp_dir().join(format!("ps-cfg-atomic-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        write_json_atomic(&path, r#"{"a":1}"#).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), r#"{"a":1}"#);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "temp file must be cleaned up"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
