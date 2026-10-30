// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::{
    Button, Frame, Margin, Response, RichText, Sense, Stroke, Ui, Vec2, epaint::StrokeKind,
};

use crate::theme::Theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Primary,
    Secondary,
}

/// A text button. Disable it with `ui.add_enabled_ui(false, ..)`.
pub fn button(ui: &mut Ui, text: &str, variant: Variant) -> Response {
    let t = Theme::get(ui.ctx());
    let size = Vec2::new(0.0, t.metrics.control);
    match variant {
        Variant::Primary => ui.add(
            Button::new(RichText::new(text).color(t.colors.accent_text))
                .fill(t.colors.accent)
                .min_size(size),
        ),
        Variant::Secondary => ui.add(Button::new(text).min_size(size)),
    }
}

pub fn label(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(text.into())
}

/// A secondary line of text.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    let t = Theme::get(ui.ctx());
    ui.label(RichText::new(text.into()).color(t.colors.text_muted))
}

/// A bordered surface.
pub fn panel<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let t = Theme::get(ui.ctx());
    Frame::new()
        .fill(t.colors.surface)
        .stroke(Stroke::new(1.0, t.colors.border))
        .corner_radius(t.metrics.radius)
        .inner_margin(Margin::same(t.metrics.pad as i8))
        .show(ui, add_contents)
        .inner
}

/// An on/off switch. Returns the response; `changed()` is true on the frame it flips.
pub fn switch(ui: &mut Ui, on: &mut bool) -> Response {
    let t = Theme::get(ui.ctx());
    let size = Vec2::new(28.0, 16.0);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    response
        .widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), *on, ""));
    let enabled = ui.is_enabled();
    let track = match (enabled, *on) {
        (false, _) => t.colors.control,
        (_, true) => t.colors.accent,
        _ => t.colors.control_hover,
    };
    let thumb = if enabled {
        t.colors.text
    } else {
        t.colors.disabled
    };
    let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
    let painter = ui.painter();
    painter.rect(rect, size.y / 2.0, track, Stroke::NONE, StrokeKind::Inside);
    let radius = size.y / 2.0 - 2.0;
    let x = egui::lerp(
        (rect.left() + size.y / 2.0)..=(rect.right() - size.y / 2.0),
        how_on,
    );
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, thumb);
    response
}

/// A horizontal slider over `range`, as wide as the space left. No value text:
/// show it beside the slider if needed.
pub fn slider(ui: &mut Ui, value: &mut f32, range: std::ops::RangeInclusive<f32>) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = ui.available_width().max(40.0);
        ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .trailing_fill(true),
        )
    })
    .inner
}
