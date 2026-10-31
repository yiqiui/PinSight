//! Visual design system: colour palette, widget styling and typography scale.
//!
//! egui ships a serviceable default theme, but a screenshot tool is a *tool*:
//! it should look like a precise instrument. Everything visual therefore comes
//! from one palette, so light/dark and the five accent colours stay consistent
//! across the main window, the overlay toolbar and the settings page.

use eframe::egui;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    /// Follow the operating system.
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn preference(self) -> egui::ThemePreference {
        match self {
            Self::System => egui::ThemePreference::System,
            Self::Light => egui::ThemePreference::Light,
            Self::Dark => egui::ThemePreference::Dark,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Accent {
    #[default]
    Blue,
    Violet,
    Emerald,
    Amber,
    Rose,
    /// Owl-eye cyan: the right iris of the logo.
    Cyan,
    /// Whatever the user picked in the colour dialog.
    Custom,
}

impl Accent {
    /// The accent for the active theme, resolving [`Accent::Custom`].
    pub fn resolve(self, custom: [u8; 3]) -> egui::Color32 {
        match self {
            Self::Custom => egui::Color32::from_rgb(custom[0], custom[1], custom[2]),
            other => other.color(),
        }
    }

    /// Same, for the light theme: a custom colour is darkened slightly so it
    /// keeps enough contrast against a white card.
    pub fn resolve_light(self, custom: [u8; 3]) -> egui::Color32 {
        match self {
            Self::Custom => {
                let mix = |c: u8| ((c as f32 * 0.88) as u8).max(8);
                egui::Color32::from_rgb(mix(custom[0]), mix(custom[1]), mix(custom[2]))
            }
            other => other.light_color(),
        }
    }

    /// Black or white, whichever reads better on top of the accent fill.
    fn on_accent(self, custom: [u8; 3]) -> egui::Color32 {
        let rgb = match self {
            Self::Custom => custom,
            _ => {
                let c = self.color();
                [c.r(), c.g(), c.b()]
            }
        };
        let lum = 0.299 * rgb[0] as f32 + 0.587 * rgb[1] as f32 + 0.114 * rgb[2] as f32;
        if lum > 150.0 {
            egui::Color32::from_rgb(0x08, 0x0C, 0x12)
        } else {
            egui::Color32::WHITE
        }
    }

    /// The raw brand colour, independent of the active theme.
    pub fn color(self) -> egui::Color32 {
        match self {
            Self::Blue => egui::Color32::from_rgb(0x5B, 0x9D, 0xFF),
            Self::Violet => egui::Color32::from_rgb(0xA7, 0x8B, 0xFA),
            Self::Emerald => egui::Color32::from_rgb(0x34, 0xD3, 0x99),
            Self::Amber => egui::Color32::from_rgb(0xFB, 0xBF, 0x24),
            Self::Rose => egui::Color32::from_rgb(0xFB, 0x71, 0x85),
            Self::Cyan => egui::Color32::from_rgb(0x22, 0xD3, 0xEE),
            Self::Custom => egui::Color32::from_rgb(0x5B, 0x9D, 0xFF),
        }
    }

    fn light_color(self) -> egui::Color32 {
        match self {
            Self::Blue => egui::Color32::from_rgb(0x25, 0x63, 0xEB),
            Self::Violet => egui::Color32::from_rgb(0x7C, 0x3A, 0xED),
            Self::Emerald => egui::Color32::from_rgb(0x05, 0x96, 0x69),
            Self::Amber => egui::Color32::from_rgb(0xB4, 0x53, 0x09),
            Self::Rose => egui::Color32::from_rgb(0xE1, 0x1D, 0x48),
            Self::Cyan => egui::Color32::from_rgb(0x08, 0x91, 0xA8),
            Self::Custom => egui::Color32::from_rgb(0x25, 0x63, 0xEB),
        }
    }
}

/// Resolved colours for one theme + accent combination.
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Window background.
    pub bg: egui::Color32,
    /// Navigation rail / sidebar.
    pub rail: egui::Color32,
    /// Raised surface (cards, tiles).
    pub card: egui::Color32,
    pub card_hover: egui::Color32,
    pub stroke: egui::Color32,
    pub stroke_strong: egui::Color32,
    pub text: egui::Color32,
    pub text_weak: egui::Color32,
    pub accent: egui::Color32,
    /// Accent with low alpha, for icon plates and selected rows.
    pub accent_soft: egui::Color32,
    /// Text drawn on top of a solid accent fill.
    pub on_accent: egui::Color32,
    pub success: egui::Color32,
    pub warn: egui::Color32,
    pub danger: egui::Color32,
}

impl Palette {
    pub fn for_theme(theme: egui::Theme, accent: Accent, custom: [u8; 3]) -> Self {
        match theme {
            egui::Theme::Dark => Self {
                dark: true,
                bg: egui::Color32::from_rgb(0x0F, 0x12, 0x17),
                rail: egui::Color32::from_rgb(0x13, 0x17, 0x1E),
                card: egui::Color32::from_rgb(0x18, 0x1D, 0x26),
                card_hover: egui::Color32::from_rgb(0x1F, 0x26, 0x31),
                stroke: egui::Color32::from_rgb(0x24, 0x2B, 0x37),
                stroke_strong: egui::Color32::from_rgb(0x33, 0x3D, 0x4C),
                text: egui::Color32::from_rgb(0xE8, 0xEC, 0xF3),
                text_weak: egui::Color32::from_rgb(0x8B, 0x95, 0xA7),
                accent: accent.resolve(custom),
                accent_soft: with_alpha(accent.resolve(custom), 34),
                on_accent: accent.on_accent(custom),
                success: egui::Color32::from_rgb(0x3D, 0xD6, 0x8C),
                warn: egui::Color32::from_rgb(0xF5, 0xB5, 0x4B),
                danger: egui::Color32::from_rgb(0xF8, 0x71, 0x71),
            },
            egui::Theme::Light => Self {
                dark: false,
                bg: egui::Color32::from_rgb(0xF5, 0xF7, 0xFA),
                rail: egui::Color32::from_rgb(0xFF, 0xFF, 0xFF),
                card: egui::Color32::from_rgb(0xFF, 0xFF, 0xFF),
                card_hover: egui::Color32::from_rgb(0xEF, 0xF3, 0xF9),
                stroke: egui::Color32::from_rgb(0xE2, 0xE7, 0xEF),
                stroke_strong: egui::Color32::from_rgb(0xC7, 0xCF, 0xDD),
                text: egui::Color32::from_rgb(0x14, 0x18, 0x21),
                text_weak: egui::Color32::from_rgb(0x64, 0x6E, 0x7E),
                accent: accent.resolve_light(custom),
                accent_soft: with_alpha(accent.resolve_light(custom), 26),
                on_accent: accent.on_accent(custom),
                success: egui::Color32::from_rgb(0x0F, 0x9D, 0x62),
                warn: egui::Color32::from_rgb(0xB4, 0x6B, 0x05),
                danger: egui::Color32::from_rgb(0xD9, 0x3A, 0x3A),
            },
        }
    }

    /// Palette for whichever theme egui currently resolves (this honours
    /// `ThemeMode::System` automatically).
    pub fn current(ctx: &egui::Context, accent: Accent, custom: [u8; 3]) -> Self {
        Self::for_theme(ctx.theme(), accent, custom)
    }
}

pub fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Install the theme for the whole context. Safe to call every frame.
pub fn apply(ctx: &egui::Context, mode: ThemeMode, accent: Accent, custom: [u8; 3]) {
    ctx.set_theme(mode.preference());
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        let palette = Palette::for_theme(theme, accent, custom);
        ctx.style_mut_of(theme, |style| {
            style.visuals = visuals_for(palette);
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            style.spacing.button_padding = egui::vec2(12.0, 7.0);
            style.spacing.interact_size = egui::vec2(30.0, 28.0);
            style.spacing.slider_width = 170.0;
            style.spacing.combo_width = 168.0;
            style.spacing.indent = 18.0;
            // Floating bars, but *not* the high-contrast "foreground" palette:
            // that one paints the handle in a near-black grey which reads as a
            // hard black stripe down a light window. Neutral greys with a low
            // idle opacity look like part of the surface instead.
            style.spacing.scroll.floating = true;
            style.spacing.scroll.foreground_color = false;
            style.spacing.scroll.bar_width = 8.0;
            style.spacing.scroll.floating_width = 5.0;
            style.spacing.scroll.floating_allocated_width = 8.0;
            style.spacing.scroll.bar_inner_margin = 4.0;
            style.spacing.scroll.bar_outer_margin = 2.0;
            style.spacing.scroll.handle_min_length = 28.0;
            style.spacing.scroll.dormant_background_opacity = 0.0;
            style.spacing.scroll.active_background_opacity = 0.0;
            style.spacing.scroll.interact_background_opacity = 0.30;
            style.spacing.scroll.dormant_handle_opacity = 0.0;
            style.spacing.scroll.active_handle_opacity = 0.45;
            style.spacing.scroll.interact_handle_opacity = 0.85;
            style.animation_time = 0.12;
            style.text_styles = text_styles(1.0);
            style.interaction.selectable_labels = false;
        });
    }
}

pub fn text_styles(scale: f32) -> std::collections::BTreeMap<egui::TextStyle, egui::FontId> {
    use egui::TextStyle;
    let s = scale.clamp(0.7, 1.8);
    let prop = egui::FontFamily::Proportional;
    let mono = egui::FontFamily::Monospace;
    [
        (
            TextStyle::Heading,
            egui::FontId::new(21.0 * s, prop.clone()),
        ),
        (
            TextStyle::Name("h2".into()),
            egui::FontId::new(15.5 * s, prop.clone()),
        ),
        (TextStyle::Body, egui::FontId::new(13.5 * s, prop.clone())),
        (TextStyle::Button, egui::FontId::new(13.5 * s, prop.clone())),
        (TextStyle::Small, egui::FontId::new(11.5 * s, prop.clone())),
        (TextStyle::Monospace, egui::FontId::new(12.5 * s, mono)),
    ]
    .into()
}

/// A "section heading" font, used by the page headers.
pub fn h2() -> egui::TextStyle {
    egui::TextStyle::Name("h2".into())
}

fn visuals_for(p: Palette) -> egui::Visuals {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };

    let radius = 9.into();

    v.dark_mode = p.dark;
    v.panel_fill = p.bg;
    v.window_fill = p.card;
    v.window_stroke = egui::Stroke::new(1.0, p.stroke);
    v.window_corner_radius = 14.into();
    v.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: egui::Color32::from_black_alpha(if p.dark { 120 } else { 40 }),
    };
    v.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: egui::Color32::from_black_alpha(if p.dark { 140 } else { 48 }),
    };
    v.menu_corner_radius = 10.into();
    v.extreme_bg_color = if p.dark {
        egui::Color32::from_rgb(0x0B, 0x0E, 0x13)
    } else {
        egui::Color32::from_rgb(0xF0, 0xF3, 0xF8)
    };
    v.faint_bg_color = if p.dark {
        egui::Color32::from_rgb(0x15, 0x1A, 0x22)
    } else {
        egui::Color32::from_rgb(0xF7, 0xF9, 0xFC)
    };
    v.code_bg_color = p.accent_soft;
    v.hyperlink_color = p.accent;
    v.warn_fg_color = p.warn;
    v.error_fg_color = p.danger;
    v.weak_text_color = Some(p.text_weak);
    v.selection.bg_fill = with_alpha(p.accent, if p.dark { 70 } else { 50 });
    v.selection.stroke = egui::Stroke::new(1.0, p.accent);
    v.resize_corner_size = 0.0;

    let noninteractive = egui::style::WidgetVisuals {
        bg_fill: p.card,
        weak_bg_fill: p.card,
        bg_stroke: egui::Stroke::new(1.0, p.stroke),
        corner_radius: radius,
        fg_stroke: egui::Stroke::new(1.0, p.text),
        expansion: 0.0,
    };

    v.widgets = egui::style::Widgets {
        noninteractive,
        inactive: egui::style::WidgetVisuals {
            bg_fill: if p.dark {
                egui::Color32::from_rgb(0x22, 0x2A, 0x36)
            } else {
                egui::Color32::from_rgb(0xEC, 0xF0, 0xF6)
            },
            weak_bg_fill: if p.dark {
                egui::Color32::from_rgb(0x1C, 0x22, 0x2C)
            } else {
                egui::Color32::from_rgb(0xF2, 0xF5, 0xFA)
            },
            bg_stroke: egui::Stroke::new(1.0, p.stroke),
            corner_radius: radius,
            fg_stroke: egui::Stroke::new(1.0, p.text),
            expansion: 0.0,
        },
        hovered: egui::style::WidgetVisuals {
            bg_fill: if p.dark {
                egui::Color32::from_rgb(0x28, 0x31, 0x40)
            } else {
                egui::Color32::from_rgb(0xE4, 0xEA, 0xF3)
            },
            weak_bg_fill: if p.dark {
                egui::Color32::from_rgb(0x23, 0x2B, 0x37)
            } else {
                egui::Color32::from_rgb(0xEB, 0xEF, 0xF6)
            },
            bg_stroke: egui::Stroke::new(1.0, p.stroke_strong),
            corner_radius: radius,
            fg_stroke: egui::Stroke::new(1.0, p.text),
            expansion: 0.0,
        },
        active: egui::style::WidgetVisuals {
            bg_fill: with_alpha(p.accent, if p.dark { 90 } else { 60 }),
            weak_bg_fill: p.accent_soft,
            bg_stroke: egui::Stroke::new(1.0, p.accent),
            corner_radius: radius,
            fg_stroke: egui::Stroke::new(1.0, p.text),
            expansion: 0.0,
        },
        open: egui::style::WidgetVisuals {
            bg_fill: p.card_hover,
            weak_bg_fill: p.card_hover,
            bg_stroke: egui::Stroke::new(1.0, p.stroke_strong),
            corner_radius: radius,
            fg_stroke: egui::Stroke::new(1.0, p.text),
            expansion: 0.0,
        },
    };

    v
}
