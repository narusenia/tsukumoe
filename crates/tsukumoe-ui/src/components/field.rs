// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::ops::RangeInclusive;

use egui::{Color32, DragValue, Frame, Id, Margin, Response, Stroke, TextEdit, Ui, Vec2};

use crate::theme::Theme;

/// A horizontal slider over `range`, as wide as the space left. No value text:
/// show it beside the slider, or use [`number`]. `name` is what a screen reader
/// announces.
pub fn slider(ui: &mut Ui, value: &mut f32, range: RangeInclusive<f32>, name: &str) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().slider_width = ui.available_width().max(24.0);
        // The thumb radius is a fraction of this height, so a short row gives a slim thumb.
        ui.spacing_mut().interact_size.y = 14.0;
        ui.add(
            egui::Slider::new(value, range)
                .show_value(false)
                .trailing_fill(true),
        )
        .accessible_name(name)
    })
    .inner
}

/// A number you can type into or drag. `speed` is the change per point of drag;
/// Shift makes it a tenth. Escape while typing puts back the value the field had
/// when it was entered.
pub fn number(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    speed: f64,
    name: &str,
) -> Response {
    number_or(ui, value, range, speed, name, None)
}

/// [`number`] that a double-click puts back to `default`, when there is one.
pub fn number_or(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    speed: f64,
    name: &str,
    default: Option<f32>,
) -> Response {
    let m = Theme::get(ui.ctx()).metrics;
    let fine = ui.input(|i| i.modifiers.shift);
    let start_id = Id::unique(("number-start", name, ui.next_auto_id()));
    let mut response = ui.add_sized(
        Vec2::new(64.0, m.control),
        DragValue::new(value)
            .range(range)
            .speed(if fine { speed * 0.1 } else { speed })
            .fixed_decimals(2),
    );
    if response.gained_focus() {
        ui.data_mut(|d| d.insert_temp(start_id, *value));
    }
    // The first click already turned the field into a text edit, whose own response
    // may not report the second one, so read the pointer instead.
    if ui.input(|i| {
        i.pointer
            .button_double_clicked(egui::PointerButton::Primary)
    }) && ui.rect_contains_pointer(response.rect)
        && let Some(d) = default
    {
        *value = d;
        response.mark_changed();
        response.surrender_focus();
    }
    if (response.has_focus() || response.lost_focus())
        && ui.input(|i| i.key_pressed(egui::Key::Escape))
        && let Some(start) = ui.data(|d| d.get_temp::<f32>(start_id))
    {
        *value = start;
        response.mark_changed();
        response.surrender_focus();
    }
    response.accessible_name(name)
}

/// A [`number`] field for a value the caller keeps elsewhere (a model): while
/// it is dragged or typed in, it shows the draft, and gives the finished value
/// once, when the edit ends and the value is not `value`. Escape drops the edit.
pub fn number_edit(
    ui: &mut Ui,
    value: f32,
    range: RangeInclusive<f32>,
    speed: f64,
    name: &str,
) -> Option<f32> {
    let draft_id = Id::unique(("number-draft", name, ui.next_auto_id()));
    let draft = ui.data(|d| d.get_temp::<f32>(draft_id));
    let mut shown = draft.unwrap_or(value);
    let response = number(ui, &mut shown, range, speed, name);
    let ended = response.drag_stopped() || response.lost_focus();
    if ended {
        ui.data_mut(|d| d.remove_temp::<f32>(draft_id));
        return (shown != value).then_some(shown);
    }
    if response.changed() || response.dragged() || response.has_focus() {
        ui.data_mut(|d| d.insert_temp(draft_id, shown));
    } else if draft.is_some() {
        ui.data_mut(|d| d.remove_temp::<f32>(draft_id));
    }
    None
}

/// A single-line text field that fills the width left. `hint` shows while empty;
/// `name` is what a screen reader announces.
pub fn text_input(ui: &mut Ui, text: &mut String, name: &str, hint: &str) -> Response {
    let t = Theme::get(ui.ctx());
    let mut frame = Frame::new()
        .fill(t.colors.control)
        .corner_radius(t.metrics.radius)
        .inner_margin(Margin::symmetric(t.metrics.gap as i8 + 2, 0))
        .begin(ui);
    let response = frame.content_ui.add_sized(
        Vec2::new(frame.content_ui.available_width(), t.metrics.control - 2.0),
        TextEdit::singleline(text)
            .hint_text(hint)
            .frame(egui::Frame::NONE)
            .align(egui::Align2::LEFT_CENTER),
    );
    frame.frame.stroke = Stroke::new(
        1.0,
        if response.has_focus() {
            t.colors.accent
        } else {
            Color32::TRANSPARENT
        },
    );
    if response.hovered() && !response.has_focus() {
        frame.frame.fill = t.colors.control_hover;
    }
    frame.end(ui);
    response.accessible_name(name)
}

/// A drop-down list. `changed()` is true on the frame the choice changes.
pub fn select<T: PartialEq + Clone>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    name: &str,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let current = options
        .iter()
        .find(|(v, _)| v == value)
        .map_or("", |(_, name)| *name);
    let mut changed = false;
    let mut response = egui::ComboBox::from_id_salt(Id::unique(id))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (v, name) in options {
                changed |= ui.selectable_value(value, v.clone(), *name).clicked();
            }
        })
        .response;
    if changed {
        response.mark_changed();
    }
    response.accessible_name(name)
}

/// A color swatch that opens a picker.
pub fn color_picker(ui: &mut Ui, color: &mut Color32, name: &str) -> Response {
    ui.color_edit_button_srgba(color).accessible_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn a_typed_value_comes_back_once_when_the_edit_ends() {
        let mut h = Harness::new_ui_state(
            |ui, got: &mut Vec<f32>| {
                crate::init(ui.ctx());
                if let Some(v) = number_edit(ui, 1.0, 0.0..=100.0, 0.1, "Width") {
                    got.push(v);
                }
            },
            Vec::new(),
        );
        h.run();
        h.get_by_label("Width").click();
        h.run();
        h.get_by_label("Width").type_text("5");
        h.run();
        assert!(h.state().is_empty(), "nothing while typing");
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(h.state().len(), 1);
        assert!(h.state()[0] != 1.0);
        h.run();
        assert_eq!(h.state().len(), 1, "once");
    }

    #[test]
    fn escape_while_typing_puts_back_the_entered_value() {
        let mut h = Harness::new_ui_state(
            |ui, value: &mut f32| {
                crate::init(ui.ctx());
                number(ui, value, 0.0..=1.0, 0.01, "Opacity");
            },
            0.5_f32,
        );
        h.run();
        h.get_by_label("Opacity").click();
        h.run();
        h.get_by_label("Opacity").type_text("0.9");
        h.run();
        h.key_press(egui::Key::Escape);
        h.run();
        assert_eq!(*h.state(), 0.5);
    }
}
