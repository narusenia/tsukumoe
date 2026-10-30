// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::epaint::Shadow;
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Id, Stroke, TextStyle, ThemePreference,
    Vec2, Visuals,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A `#rrggbb` color in a theme file.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(transparent)]
pub struct Hex(#[schemars(regex(pattern = r"^#[0-9a-fA-F]{6}$"))] String);

impl Hex {
    fn color(&self) -> Result<Color32, String> {
        let digits = self.0.strip_prefix('#').filter(|d| d.len() == 6);
        let value = digits.and_then(|d| u32::from_str_radix(d, 16).ok());
        value
            .map(rgb)
            .ok_or_else(|| format!("{:?} is not #rrggbb", self.0))
    }
}

/// Declares the color tokens once: the runtime colors, the optional overrides a
/// theme file may set, and the code that applies them.
macro_rules! color_tokens {
    ($($(#[$doc:meta])* $name:ident),* $(,)?) => {
        /// Semantic colors. Components read these and never hard-code a color.
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct Colors {
            $($(#[$doc])* pub $name: Color32,)*
        }

        /// The colors a theme file may override; the rest come from the
        /// built-in theme of the file's appearance.
        #[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct ColorOverrides {
            $(#[serde(default, skip_serializing_if = "Option::is_none")] pub $name: Option<Hex>,)*
        }

        impl Colors {
            fn apply(&mut self, overrides: &ColorOverrides) -> Result<(), String> {
                $(if let Some(hex) = &overrides.$name {
                    self.$name = hex.color().map_err(|e| format!("{}: {e}", stringify!($name)))?;
                })*
                Ok(())
            }
        }
    };
}

color_tokens! {
    /// The window behind everything.
    background,
    /// Panels and cards.
    surface,
    /// Menus, popups, dialogs and tooltips.
    surface_raised,
    border,
    /// Hovered borders and separators being dragged.
    border_strong,
    text,
    text_muted,
    accent,
    /// Text and icons on an accent or status fill.
    accent_text,
    /// Resting fill of buttons and fields.
    control,
    control_hover,
    control_active,
    /// Text and icons that cannot be used.
    disabled,
    error,
    warning,
    success,
}

/// How tightly the interface is packed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// The default: 12pt text, 22pt controls.
    #[default]
    Compact,
    /// 13pt text, 26pt controls.
    Regular,
}

/// Sizes in points, set by the density.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    pub text: f32,
    pub control: f32,
    pub gap: f32,
    pub pad: f32,
    pub radius: f32,
    pub icon: f32,
}

impl Metrics {
    pub fn new(density: Density) -> Self {
        match density {
            Density::Compact => Self {
                text: 12.0,
                control: 22.0,
                gap: 4.0,
                pad: 8.0,
                radius: 3.0,
                icon: 14.0,
            },
            Density::Regular => Self {
                text: 13.0,
                control: 26.0,
                gap: 6.0,
                pad: 10.0,
                radius: 4.0,
                icon: 16.0,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Theme {
    pub dark: bool,
    pub density: Density,
    pub colors: Colors,
    pub metrics: Metrics,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Light,
    #[default]
    Dark,
    /// Follows the OS appearance, re-read on every [`set_mode`] call.
    System,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum Appearance {
    Light,
    Dark,
}

/// A theme file: `{"appearance": "dark", "density": "compact", "colors": {"accent": "#5b7cfa"}}`.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ThemeFile {
    appearance: Appearance,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    density: Option<Density>,
    #[serde(default)]
    colors: ColorOverrides,
}

fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            dark: true,
            density: Density::Compact,
            metrics: Metrics::new(Density::Compact),
            colors: Colors {
                background: rgb(0x0e0e0e),
                surface: rgb(0x151515),
                surface_raised: rgb(0x1c1c1c),
                border: rgb(0x242424),
                border_strong: rgb(0x343434),
                text: rgb(0xe8e8e8),
                text_muted: rgb(0x888888),
                accent: rgb(0x6c7ae0),
                accent_text: rgb(0xffffff),
                control: rgb(0x1d1d1d),
                control_hover: rgb(0x262626),
                control_active: rgb(0x303030),
                disabled: rgb(0x4a4a4a),
                error: rgb(0xf0616d),
                warning: rgb(0xe5a946),
                success: rgb(0x4cc38a),
            },
        }
    }

    pub fn light() -> Self {
        Self {
            dark: false,
            density: Density::Compact,
            metrics: Metrics::new(Density::Compact),
            colors: Colors {
                background: rgb(0xfafafa),
                surface: rgb(0xffffff),
                surface_raised: rgb(0xffffff),
                border: rgb(0xebebeb),
                border_strong: rgb(0xd6d6d6),
                text: rgb(0x1a1a1a),
                text_muted: rgb(0x6b6b6b),
                accent: rgb(0x4f5bd5),
                accent_text: rgb(0xffffff),
                control: rgb(0xf1f1f1),
                control_hover: rgb(0xe8e8e8),
                control_active: rgb(0xdddddd),
                disabled: rgb(0xb0b0b0),
                error: rgb(0xd6303f),
                warning: rgb(0xb7791f),
                success: rgb(0x1f8f5f),
            },
        }
    }

    pub fn with_density(mut self, density: Density) -> Self {
        self.density = density;
        self.metrics = Metrics::new(density);
        self
    }

    /// The active theme; the dark one before any has been set.
    pub fn get(ctx: &Context) -> Self {
        ctx.data(|d| d.get_temp(Id::NULL))
            .unwrap_or_else(Self::dark)
    }

    /// Reads a theme file (see [`ThemeFile`]). The built-in theme of the file's
    /// appearance fills any color or the density the file leaves out.
    pub fn from_json(json: &str) -> Result<Self, String> {
        let file: ThemeFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut theme = match file.appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light(),
        };
        theme.colors.apply(&file.colors)?;
        Ok(theme.with_density(file.density.unwrap_or_default()))
    }

    /// A mix of two colors, `t` of the way from `a` to `b`: for hover and
    /// pressed states derived from a base color.
    pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
        let lerp = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
        Color32::from_rgb(lerp(a.r(), b.r()), lerp(a.g(), b.g()), lerp(a.b(), b.b()))
    }

    fn style(&self) -> egui::Style {
        let Self {
            dark,
            colors: c,
            metrics: m,
            ..
        } = *self;
        let mut visuals = if dark {
            Visuals::dark()
        } else {
            Visuals::light()
        };
        visuals.panel_fill = c.background;
        visuals.window_fill = c.surface_raised;
        visuals.faint_bg_color = c.surface;
        visuals.extreme_bg_color = c.control;
        visuals.code_bg_color = c.control;
        visuals.window_stroke = Stroke::new(1.0, c.border);
        visuals.window_corner_radius = CornerRadius::same((m.radius + 2.0) as u8);
        visuals.menu_corner_radius = CornerRadius::same(m.radius as u8);
        visuals.window_shadow = Shadow::NONE;
        visuals.popup_shadow = Shadow::NONE;
        visuals.selection.bg_fill = c.accent;
        // The stroke colors the text of a selected row, which sits on the accent fill.
        visuals.selection.stroke = Stroke::new(1.0, c.accent_text);
        visuals.hyperlink_color = c.accent;
        visuals.warn_fg_color = c.warning;
        visuals.error_fg_color = c.error;
        visuals.weak_text_color = Some(c.text_muted);
        visuals.handle_shape = egui::style::HandleShape::Circle;
        let radius = CornerRadius::same(m.radius as u8);
        let w = &mut visuals.widgets;
        w.noninteractive.bg_fill = c.surface;
        w.noninteractive.bg_stroke = Stroke::new(1.0, c.border);
        w.noninteractive.fg_stroke = Stroke::new(1.0, c.text);
        for (state, fill) in [
            (&mut w.inactive, c.control),
            (&mut w.hovered, c.control_hover),
            (&mut w.active, c.control_active),
            (&mut w.open, c.control_hover),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.bg_stroke = Stroke::NONE;
            state.fg_stroke = Stroke::new(1.0, c.text);
        }
        // The slider rail and checkbox frames use this fill; buttons use the weak one.
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
        style.spacing.slider_rail_height = 2.0;
        style.spacing.icon_width = m.icon;
        style.spacing.menu_margin = egui::Margin::same(4);
        style.spacing.tooltip_width = 280.0;
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

/// Switches to the built-in theme for `mode`, keeping the current density.
pub fn set_mode(ctx: &Context, mode: Mode) {
    let dark = match mode {
        Mode::Light => false,
        Mode::Dark => true,
        Mode::System => ctx.system_theme() != Some(egui::Theme::Light),
    };
    let base = if dark { Theme::dark() } else { Theme::light() };
    set_theme(ctx, base.with_density(Theme::get(ctx).density));
}

/// Changes the density of the current theme.
pub fn set_density(ctx: &Context, density: Density) {
    set_theme(ctx, Theme::get(ctx).with_density(density));
}

/// A theme with the name it is listed under.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedTheme {
    pub name: String,
    pub theme: Theme,
}

impl NamedTheme {
    /// Reads a theme file (see [`ThemeFile`]) and lists it under `name`.
    pub fn from_json(name: &str, json: &str) -> Result<Self, String> {
        Ok(Self {
            name: name.to_owned(),
            theme: Theme::from_json(json).map_err(|e| format!("{name}: {e}"))?,
        })
    }
}

/// The themes that ship with the library, beside the plain dark and light ones.
pub fn builtin_themes() -> Vec<NamedTheme> {
    [
        ("Figma Dark", include_str!("../themes/figma-dark.json")),
        ("Figma Light", include_str!("../themes/figma-light.json")),
    ]
    .into_iter()
    .map(|(name, json)| NamedTheme::from_json(name, json).expect("built-in theme is valid"))
    .collect()
}

/// What to remember between runs: the mode, the density and, when one is
/// chosen, a named theme (which then takes the place of the mode's theme).
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Preferences {
    #[serde(default)]
    pub mode: Mode,
    #[serde(default)]
    pub density: Density,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
}

impl Preferences {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("preferences serialize")
    }

    /// Reads saved preferences; anything unreadable gives the defaults, since
    /// a damaged settings file must not stop the app from starting.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap_or_default()
    }

    /// Installs the saved theme: the named one from `themes` if it is still
    /// there, else the mode's. The density here wins over a theme file's own.
    pub fn apply(&self, ctx: &Context, themes: &[NamedTheme]) {
        let named = self
            .theme
            .as_ref()
            .and_then(|name| themes.iter().find(|t| &t.name == name));
        match named {
            Some(t) => set_theme(ctx, t.theme.with_density(self.density)),
            None => {
                set_density(ctx, self.density);
                set_mode(ctx, self.mode);
            }
        }
    }
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
        assert_eq!(t.density, Density::Compact);
    }

    #[test]
    fn json_sets_the_density() {
        let t = Theme::from_json(r#"{"appearance":"dark","density":"regular"}"#).unwrap();
        assert_eq!(t.metrics, Metrics::new(Density::Regular));
    }

    #[test]
    fn json_rejects_bad_input() {
        for bad in [
            r#"{"appearance":"sepia"}"#,
            r##"{"appearance":"dark","colors":{"nope":"#000000"}}"##,
            r##"{"appearance":"dark","colors":{"accent":"red"}}"##,
            r##"{"appearance":"dark","colors":{"accent":"#12345"}}"##,
            r#"{"appearance":"dark","density":"huge"}"#,
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
        assert_eq!(Theme::get(&ctx).colors, Theme::dark().colors);
    }

    #[test]
    fn density_survives_a_mode_change() {
        let ctx = Context::default();
        set_density(&ctx, Density::Regular);
        set_mode(&ctx, Mode::Light);
        let t = Theme::get(&ctx);
        assert!(!t.dark);
        assert_eq!(t.density, Density::Regular);
    }

    #[test]
    fn preferences_round_trip_and_survive_damage() {
        let p = Preferences {
            mode: Mode::System,
            density: Density::Regular,
            theme: Some("Figma Dark".into()),
        };
        assert_eq!(Preferences::from_json(&p.to_json()), p);
        assert_eq!(Preferences::from_json("not json"), Preferences::default());
        assert_eq!(Preferences::from_json("{}"), Preferences::default());
    }

    #[test]
    fn mix_blends_two_colors() {
        let m = Theme::mix(Color32::BLACK, Color32::WHITE, 0.5);
        assert_eq!(m, Color32::from_rgb(128, 128, 128));
    }

    #[test]
    fn builtin_themes_load_and_a_named_theme_replaces_the_mode() {
        let themes = builtin_themes();
        assert_eq!(themes.len(), 2);
        let ctx = Context::default();
        let prefs = Preferences {
            theme: Some("Figma Light".into()),
            density: Density::Regular,
            ..Default::default()
        };
        prefs.apply(&ctx, &themes);
        let t = Theme::get(&ctx);
        assert!(!t.dark);
        assert_eq!(t.colors.accent, rgb(0x0d99ff));
        assert_eq!(t.density, Density::Regular);
        // A name that no longer exists falls back to the mode.
        Preferences {
            theme: Some("gone".into()),
            ..Default::default()
        }
        .apply(&ctx, &themes);
        assert_eq!(Theme::get(&ctx).colors, Theme::dark().colors);
    }
}
