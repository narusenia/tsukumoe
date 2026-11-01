// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Tsukumoe's UI layer on egui (ADR-0027): design tokens, a theme that can be
//! switched, loaded from JSON and remembered, components that read it, icons,
//! and a keymap. The `gallery` example shows every component in every state;
//! `docs/specifications/ui-library.md` has the rules for adding one.

mod components;
pub mod dock;
mod font;
mod icons;
mod keymap;
mod lang;
mod pie;
pub mod showcase;
mod theme;

pub use components::{
    Toasts, Tone, Variant, badge, button, checkbox, color_picker, dialog, hint, icon_button,
    icon_text_button, label, list, menu_item, number, panel, progress, radio, section_header,
    select, selectable_row, slider, switch, tabs, text_input, tip,
};
pub use font::{has_cjk_font, install_cjk_font};
pub use icons::{Icon, icon};
pub use keymap::{Keymap, KeymapFile, parse_shortcut};
pub use lang::{language, set_language, tr, tr_args};
pub use pie::{DEAD_RADIUS, OUTER_RADIUS, PieEvent, PieHit, PieItem, PieMenu, pie_hit};
pub use theme::{
    ColorOverrides, Colors, Density, Metrics, Mode, NamedTheme, Preferences, Theme, ThemeFile,
    builtin_themes, set_density, set_mode, set_theme,
};
pub use tsukumoe_i18n::{Arg, Language};

/// Prepares egui for the library: image loaders for icons, a CJK font, and the
/// dark theme. Call once, when the app is created.
pub fn init(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
    font::install_cjk_font(ctx);
    set_mode(ctx, Mode::Dark);
}
