// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::{Color32, Rect, Response, RichText, Sense, TextStyle, Ui, vec2};

use crate::icons::Icon;
use crate::theme::Theme;

pub fn label(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(text.into())
}

/// A secondary line of text.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    let t = Theme::get(ui.ctx());
    ui.label(RichText::new(text.into()).color(t.colors.text_muted))
}

/// A small heading above a group of controls.
pub fn section_header(ui: &mut Ui, text: &str) -> Response {
    let t = Theme::get(ui.ctx());
    ui.label(
        RichText::new(text.to_uppercase())
            .size(t.metrics.text - 2.0)
            .color(t.colors.text_muted)
            .strong(),
    )
}

/// Shows `text` while the pointer rests on `response`.
pub fn tip(response: Response, text: &str) -> Response {
    response.on_hover_text(text)
}

/// What a badge or a toast is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Neutral,
    Accent,
    Success,
    Warning,
    Error,
}

impl Tone {
    pub(crate) fn color(self, t: &Theme) -> Color32 {
        match self {
            Tone::Neutral => t.colors.text_muted,
            Tone::Accent => t.colors.accent,
            Tone::Success => t.colors.success,
            Tone::Warning => t.colors.warning,
            Tone::Error => t.colors.error,
        }
    }

    pub(crate) fn icon(self) -> Icon {
        match self {
            Tone::Neutral | Tone::Accent => Icon::Info,
            Tone::Success => Icon::Success,
            Tone::Warning => Icon::Warning,
            Tone::Error => Icon::Error,
        }
    }
}

/// A small pill with a status word.
pub fn badge(ui: &mut Ui, text: &str, tone: Tone) -> Response {
    let t = Theme::get(ui.ctx());
    let color = tone.color(&t);
    let font = TextStyle::Small.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let pad = vec2(t.metrics.gap + 2.0, 2.0);
    let size = galley.size() + 2.0 * pad;
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let [r, g, b, _] = color.to_array();
    let fill = Color32::from_rgba_unmultiplied(r, g, b, if t.dark { 40 } else { 32 });
    ui.painter().rect_filled(rect, size.y / 2.0, fill);
    ui.painter().galley(rect.min + pad, galley, color);
    response
}

/// A thin progress bar for `fraction` in `0.0..=1.0`.
pub fn progress(ui: &mut Ui, fraction: f32) -> Response {
    let t = Theme::get(ui.ctx());
    let size = vec2(ui.available_width().max(40.0), 4.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, t.colors.control_hover);
    let fill = Rect::from_min_size(rect.min, vec2(rect.width() * fraction.clamp(0.0, 1.0), 4.0));
    painter.rect_filled(fill, 2.0, t.colors.accent);
    response
}
