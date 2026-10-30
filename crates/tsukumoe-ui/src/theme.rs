// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::collections::HashMap;

use serde::Deserialize;
use wgpui::{App, Global, Pixels, Rgba, WindowAppearance, px, rgb};
use wgpui_base::ThemeAppearance;

/// Semantic colors. Components read these and never hard-code a color.
#[derive(Clone, Copy)]
pub struct Colors {
    pub background: Rgba,
    pub surface: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub accent: Rgba,
    pub accent_text: Rgba,
    pub control: Rgba,
    pub control_hover: Rgba,
    pub disabled: Rgba,
}

/// Sizes of the dense layout: small type, short controls, tight spacing.
#[derive(Clone, Copy)]
pub struct Metrics {
    pub text: Pixels,
    pub line: Pixels,
    pub control: Pixels,
    pub gap: Pixels,
    pub pad: Pixels,
    pub radius: Pixels,
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

impl Global for Theme {}

impl Metrics {
    pub fn dense() -> Self {
        Self {
            text: px(12.0),
            line: px(16.0),
            control: px(24.0),
            gap: px(4.0),
            pad: px(8.0),
            radius: px(4.0),
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

    /// The active theme. Panics before [`init`].
    pub fn get(cx: &App) -> Self {
        *cx.global::<Theme>()
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
}

fn hex(s: &str) -> Option<Rgba> {
    let digits = s.strip_prefix('#')?;
    if digits.len() != 6 {
        return None;
    }
    u32::from_str_radix(digits, 16).ok().map(rgb)
}

/// Installs `theme` and tells `wgpui-base` its appearance (scrollbars follow it).
pub fn set_theme(theme: Theme, cx: &mut App) {
    wgpui_base::Theme::global_mut(cx).appearance = if theme.dark {
        ThemeAppearance::Dark
    } else {
        ThemeAppearance::Light
    };
    cx.set_global(theme);
}

/// Switches to the built-in theme for `mode`.
pub fn set_mode(mode: Mode, cx: &mut App) {
    let dark = match mode {
        Mode::Light => false,
        Mode::Dark => true,
        Mode::System => matches!(
            cx.window_appearance(),
            WindowAppearance::Dark | WindowAppearance::VibrantDark
        ),
    };
    set_theme(if dark { Theme::dark() } else { Theme::light() }, cx);
}

/// Initializes `wgpui-base` and installs the dark theme. Call once, first.
pub fn init(cx: &mut App) {
    wgpui_base::init(cx);
    set_mode(Mode::Dark, cx);
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
}
