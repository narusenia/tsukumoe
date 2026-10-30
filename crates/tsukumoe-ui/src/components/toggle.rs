// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::epaint::StrokeKind;
use egui::{Color32, CursorIcon, Response, Role, Sense, Stroke, TextStyle, Ui, WidgetInfo, vec2};

use crate::icons::Icon;
use crate::theme::Theme;

/// Paints the keyboard-focus ring around `rect`.
fn focus_ring(ui: &Ui, rect: egui::Rect, radius: f32, accent: Color32) {
    ui.painter().rect_stroke(
        rect.expand(1.0),
        radius + 1.0,
        Stroke::new(1.5, accent),
        StrokeKind::Outside,
    );
}

/// An on/off switch. `changed()` is true on the frame it flips.
/// `name` is what a screen reader announces.
pub fn switch(ui: &mut Ui, on: &mut bool, name: &str) -> Response {
    let t = Theme::get(ui.ctx());
    let height = t.metrics.icon + 2.0;
    let size = vec2(height * 1.75, height);
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *on, name));
    if enabled {
        response.clone().on_hover_cursor(CursorIcon::PointingHand);
    }
    let c = &t.colors;
    let track = match (enabled, *on, response.hovered()) {
        (false, _, _) => c.control,
        (_, true, true) => Theme::mix(c.accent, Color32::WHITE, 0.12),
        (_, true, false) => c.accent,
        (_, false, true) => c.control_active,
        _ => c.control_hover,
    };
    let thumb = if enabled { c.accent_text } else { c.disabled };
    let how_on = ui.ctx().animate_bool_responsive(response.id, *on);
    let painter = ui.painter();
    painter.rect_filled(rect, height / 2.0, track);
    let radius = height / 2.0 - 2.0;
    let x = egui::lerp(
        (rect.left() + height / 2.0)..=(rect.right() - height / 2.0),
        how_on,
    );
    painter.circle_filled(egui::pos2(x, rect.center().y), radius, thumb);
    if response.has_focus() {
        focus_ring(ui, rect, height / 2.0, c.accent);
    }
    response
}

/// A checkbox with a label. `changed()` is true on the frame it flips.
pub fn checkbox(ui: &mut Ui, checked: &mut bool, text: &str) -> Response {
    let t = Theme::get(ui.ctx());
    let m = t.metrics;
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(m.icon + m.gap + galley.size().x, m.control.max(m.icon));
    let (rect, mut response) = ui.allocate_exact_size(size, Sense::click());
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, enabled, *checked, text));
    if enabled {
        response.clone().on_hover_cursor(CursorIcon::PointingHand);
    }
    let c = &t.colors;
    let boxed = egui::Rect::from_min_size(
        egui::pos2(rect.left(), rect.center().y - m.icon / 2.0),
        vec2(m.icon, m.icon),
    );
    let (fill, stroke) = match (enabled, *checked, response.hovered()) {
        (false, _, _) => (c.control, c.border),
        (_, true, _) => (c.accent, c.accent),
        (_, false, true) => (c.control_hover, c.border_strong),
        _ => (c.control, c.border_strong),
    };
    let painter = ui.painter();
    painter.rect(
        boxed,
        m.radius - 1.0,
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    if *checked {
        let tint = if enabled { c.accent_text } else { c.disabled };
        Icon::Check
            .image(tint, m.icon - 2.0)
            .paint_at(ui, boxed.shrink(1.0));
    }
    let color = if enabled { c.text } else { c.disabled };
    let pos = egui::pos2(
        boxed.right() + m.gap,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
    if response.has_focus() {
        focus_ring(ui, boxed, m.radius - 1.0, c.accent);
    }
    response
}

/// A radio button with a label; the caller keeps the selection. `clicked()` is
/// true when the user picks it.
pub fn radio(ui: &mut Ui, selected: bool, text: &str) -> Response {
    let t = Theme::get(ui.ctx());
    let m = t.metrics;
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let size = vec2(m.icon + m.gap + galley.size().x, m.control.max(m.icon));
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let enabled = ui.is_enabled();
    response.widget_info(|| WidgetInfo::selected(Role::RadioButton, enabled, selected, text));
    if enabled {
        response.clone().on_hover_cursor(CursorIcon::PointingHand);
    }
    let c = &t.colors;
    let center = egui::pos2(rect.left() + m.icon / 2.0, rect.center().y);
    let (fill, stroke) = match (enabled, response.hovered()) {
        (false, _) => (c.control, c.border),
        (_, true) => (c.control_hover, c.border_strong),
        _ => (c.control, c.border_strong),
    };
    let painter = ui.painter();
    painter.circle(center, m.icon / 2.0, fill, Stroke::new(1.0, stroke));
    if selected {
        let dot = if enabled { c.accent } else { c.disabled };
        painter.circle_filled(center, m.icon / 2.0 - 3.5, dot);
    }
    let color = if enabled { c.text } else { c.disabled };
    let pos = egui::pos2(
        rect.left() + m.icon + m.gap,
        rect.center().y - galley.size().y / 2.0,
    );
    ui.painter().galley(pos, galley, color);
    if response.has_focus() {
        let ring = egui::Rect::from_center_size(center, vec2(m.icon, m.icon));
        focus_ring(ui, ring, m.icon / 2.0, c.accent);
    }
    response
}
