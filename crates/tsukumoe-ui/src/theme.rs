// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::HashMap;

use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Id, Stroke, TextStyle, ThemePreference,
    Vec2, Visuals,
};
use serde::Deserialize;

/// Semantic colors. Components read these and never hard-code a color.
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Color32,
    pub surface: Color32,
    pub border: Color32,
    pub text: Color32,
    pub text_muted: Color32,
    pub accent: Color32,
    pub accent_text: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub disabled: Color32,
}

/// Sizes of the dense layout, in points: small type, short controls, tight spacing.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub text: f32,
    pub control: f32,
    pub gap: f32,
    pub pad: f32,
    pub radius: f32,
}

#[derive(Clone, Copy)]
pub struct Theme {
    pub dark: bool,
    pub colors: Colors,
    pub metrics: Metrics,
}

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum Mode {
    Light,
    #[default]
    Dark,
    /// Follows the OS appearance, re-read on every [`set_mode`] call.
    System,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Metrics {
    pub fn dense() -> Self {
        Self {
            text: 12.0,
            control: 24.0,
            gap: 4.0,
            pad: 8.0,
            radius: 4.0,
        }
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            dark: true,
            metrics: Metrics::dense(),
            colors: Colors {
                background: rgb(0x14161a),
                surface: rgb(0x1b1e23),
                border: rgb(0x2c3037),
                text: rgb(0xd9dce1),
                text_muted: rgb(0x8a909b),
                accent: rgb(0x5b7cfa),
                accent_text: rgb(0xffffff),
                control: rgb(0x252930),
                control_hover: rgb(0x2f343c),
                disabled: rgb(0x4a4f59),
            },
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            metrics: Metrics::dense(),
            colors: Colors {
                background: rgb(0xf3f4f6),
                surface: rgb(0xffffff),
                border: rgb(0xd9dce1),
                text: rgb(0x1d2025),
                text_muted: rgb(0x666d79),
                accent: rgb(0x3b5bdb),
                accent_text: rgb(0xffffff),
                control: rgb(0xeceef1),
                control_hover: rgb(0xdfe2e7),
                disabled: rgb(0xb3b8c2),
            },
        }
    }

    /// The active theme; the dark one before any has been set.
    pub fn get(ctx: &Context) -> Self {
        ctx.data(|d| d.get_temp(Id::NULL))
            .unwrap_or_else(Self::dark)
    }

    /// Reads a theme file: `{"appearance": "dark"|"light", "colors": {"accent": "#5b7cfa"}}`.
    /// The built-in theme of that appearance fills any color the file leaves out.
    pub fn from_json(json: &str) -> Result<Self, String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            appearance: String,
            #[serde(default)]
            colors: HashMap<String, String>,
        }
        let file: File = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut theme = match file.appearance.as_str() {
            "dark" => Self::dark(),
            "light" => Self::light(),
            other => return Err(format!("appearance must be dark or light, got {other:?}")),
        };
        for (key, value) in &file.colors {
            let c = &mut theme.colors;
            let slot = match key.as_str() {
                "background" => &mut c.background,
                "surface" => &mut c.surface,
                "border" => &mut c.border,
                "text" => &mut c.text,
                "text_muted" => &mut c.text_muted,
                "accent" => &mut c.accent,
                "accent_text" => &mut c.accent_text,
                "control" => &mut c.control,
                "control_hover" => &mut c.control_hover,
                "disabled" => &mut c.disabled,
                other => return Err(format!("unknown color {other:?}")),
            };
            *slot = hex(value).ok_or_else(|| format!("{key}: {value:?} is not #rrggbb"))?;
        }
        Ok(theme)
    }

    fn style(&self) -> egui::Style {
        let Self {
            dark,
            colors: c,
            metrics: m,
        } = *self;
        let mut visuals = if dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.panel_fill = c.background;
        visuals.window_fill = c.surface;
        visuals.faint_bg_color = c.surface;
        visuals.extreme_bg_color = c.control;
        visuals.window_stroke = Stroke::new(1.0, c.border);
        visuals.selection.bg_fill = c.accent;
        visuals.selection.stroke = Stroke::new(1.0, c.accent_text);
        visuals.hyperlink_color = c.accent;
        visuals.handle_shape = egui::style::HandleShape::Circle;
        let radius = CornerRadius::same(m.radius as u8);
        let w = &mut visuals.widgets;
        w.noninteractive.bg_fill = c.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
        for (state, fill) in [
            (&mut w.inactive, c.control),
            (&mut w.hovered, c.control_hover),
            (&mut w.active, c.control_hover),
            (&mut w.open, c.control),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.fg_stroke = Stroke::new(1.0, c.text);
        }
        // The slider rail is drawn with this fill; buttons use the weak fill above.
        w.inactive.bg_fill = c.control_hover;
        for state in [
            &mut w.noninteractive,
            &mut w.inactive,
            &mut w.hovered,
            &mut w.active,
            &mut w.open,
        ] {
            state.corner_radius = radius;
        }
        let mut style = egui::Style {
            visuals,
            ..Default::default()
        };
        style.spacing.item_spacing = Vec2::splat(m.gap);
        style.spacing.button_padding = Vec2::new(m.pad, 2.0);
        style.spacing.interact_size.y = m.control;
        style.spacing.slider_rail_height = 3.0;
        style.spacing.icon_width = 14.0;
        style.text_styles = [
            (
                TextStyle::Small,
                FontId::new(m.text - 2.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Body,
                FontId::new(m.text, FontFamily::Proportional),
            ),
            (
                TextStyle::Button,
                FontId::new(m.text, FontFamily::Proportional),
            ),
            (
                TextStyle::Heading,
                FontId::new(m.text + 3.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(m.text, FontFamily::Monospace),
            ),
        ]
        .into();
        style
    }
}

fn hex(s: &str) -> Option<Color32> {
    let digits = s.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

/// Installs `theme`: egui's style and the tokens components read.
pub fn set_theme(ctx: &Context, theme: Theme) {
    let which = if theme.dark {
        egui::Theme::Dark
    } else {
        egui::Theme::Light
    };
    ctx.set_style_of(which, theme.style());
    ctx.set_theme(match which {
        egui::Theme::Dark => ThemePreference::Dark,
        egui::Theme::Light => ThemePreference::Light,
    });
    ctx.data_mut(|d| d.insert_temp(Id::NULL, theme));
}

/// Switches to the built-in theme for `mode`.
pub fn set_mode(ctx: &Context, mode: Mode) {
    let dark = match mode {
        Mode::Light => false,
        Mode::Dark => true,
        Mode::System => ctx.system_theme() != Some(egui::Theme::Light),
    };
    set_theme(ctx, if dark { Theme::dark() } else { Theme::light() });
}

/// Installs a CJK font and the dark theme. Call once, when the app is created.
pub fn init(ctx: &Context) {
    crate::font::install_cjk_font(ctx);
    set_mode(ctx, Mode::Dark);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_overrides_one_color_and_keeps_the_rest() {
        let t =
            Theme::from_json(r##"{"appearance":"light","colors":{"accent":"#ff0000"}}"##).unwrap();
        assert!(!t.dark);
        assert_eq!(t.colors.accent, rgb(0xff0000));
        assert_eq!(t.colors.surface, Theme::light().colors.surface);
    }

    #[test]
    fn json_rejects_bad_input() {
        for bad in [
            r#"{"appearance":"sepia"}"#,
            r##"{"appearance":"dark","colors":{"nope":"#000000"}}"##,
            r##"{"appearance":"dark","colors":{"accent":"red"}}"##,
            r#"{"appearance":"dark","extra":1}"#,
        ] {
            assert!(Theme::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn set_mode_installs_the_theme_components_read() {
        let ctx = Context::default();
        assert!(Theme::get(&ctx).dark);
        set_mode(&ctx, Mode::Light);
        assert!(!Theme::get(&ctx).dark);
        set_mode(&ctx, Mode::Dark);
        assert_eq!(Theme::get(&ctx).colors.accent, Theme::dark().colors.accent);
    }
}
