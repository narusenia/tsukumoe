// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, Id};

const INTER: &[u8] = include_bytes!("../assets/fonts/Inter[opsz,wght].ttf");
/// Subset to kana, punctuation and CJK ideographs (see `assets/fonts`).
const NOTO_JP: &[u8] = include_bytes!("../assets/fonts/NotoSansCJKjp-Regular.otf");

/// Installs the bundled fonts: Inter for Latin text, Noto Sans CJK JP as the
/// fallback for Japanese. Returns `true` (kept for callers that check).
pub fn install_cjk_font(ctx: &Context) -> bool {
    let mut fonts = FontDefinitions::default();
    for (name, bytes) in [("inter", INTER), ("noto-jp", NOTO_JP)] {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        let list = fonts.families.entry(family).or_default();
        list.insert(0, "inter".into());
        list.insert(1, "noto-jp".into());
    }
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(Id::NULL.with("tsukumoe-ui-cjk"), true));
    true
}

/// Whether [`install_cjk_font`] found a font on this context.
pub fn has_cjk_font(ctx: &Context) -> bool {
    ctx.data(|d| d.get_temp(Id::NULL.with("tsukumoe-ui-cjk")))
        .unwrap_or(false)
}
