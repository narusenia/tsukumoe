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

/// A heading above a group of controls, with a rule under it. The space above is
/// left out at the top of a panel.
pub fn section_header(ui: &mut Ui, text: &str) -> Response {
    let t = Theme::get(ui.ctx());
    if ui.cursor().top() > ui.max_rect().top() + 1.0 {
        ui.add_space(t.metrics.pad);
    }
    let response = ui.label(RichText::new(text).color(t.colors.text).strong());
    let y = ui.cursor().top();
    let x = ui.max_rect().x_range();
    ui.painter()
        .hline(x, y, egui::Stroke::new(1.0, t.colors.border));
    ui.add_space(t.metrics.gap + 1.0);
    response
}

/// A thin vertical rule between groups of buttons in a row.
pub fn group_rule(ui: &mut Ui) {
    let t = Theme::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, t.metrics.control - 8.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.colors.border_strong);
}

/// A row of a form: `name` in a column whose width follows the panel, then the
/// control, filling what is left. Every field of one form starts at one place.
pub fn form_row<R>(ui: &mut Ui, name: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    let label_width = (ui.available_width() * 0.38).clamp(64.0, 200.0);
    let height = Theme::get(ui.ctx()).metrics.control;
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            vec2(label_width, height),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(label_width);
                ui.add(egui::Label::new(name).truncate())
                    .on_hover_text(name);
            },
        );
        ui.spacing_mut().combo_width = ui.available_width().max(48.0);
        add(ui)
    })
    .inner
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
