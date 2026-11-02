// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The pieces every viewport overlay is built from, so they share one look and
//! one place each: tools top left with the active tool's flyout beside them, a
//! status badge top centre, help bottom left. Nothing here is positioned by
//! the size of another overlay, so they cannot overlap.

use egui::{
    Align, Align2, Area, Color32, Frame, Id, Layout, Margin, Order, Rect, Stroke, Ui, vec2,
};
use tsukumoe_ui::{Icon, Theme, tr};

use crate::keyed::Place;

/// Gap between an overlay and the viewport edge.
pub const MARGIN: f32 = 8.0;

/// A raised box with the theme's border; the frame of every floating overlay.
pub fn boxed<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let theme = Theme::get(ui.ctx());
    Frame::new()
        .fill(theme.colors.surface_raised)
        .stroke(Stroke::new(1.0, theme.colors.border))
        .corner_radius(theme.metrics.radius)
        .inner_margin(Margin::symmetric(theme.metrics.pad as i8, 4))
        .show(ui, add)
        .inner
}

fn area(id: &'static str, pos: egui::Pos2, pivot: Align2) -> Area {
    Area::new(Id::unique(id))
        .fixed_pos(pos)
        .pivot(pivot)
        .order(Order::Foreground)
}

/// Memory key for the width of the tool bar, so the flyout can sit beside it.
fn tools_width_id() -> Id {
    Id::unique("tsukumoe-viewport-tools-width")
}

/// A boxed list of tools at the top left; the caller draws the buttons.
pub fn tools(ctx: &egui::Context, rect: Rect, add: impl FnOnce(&mut Ui)) {
    let response = area(
        "tsukumoe-viewport-tools",
        rect.left_top() + vec2(MARGIN, MARGIN),
        Align2::LEFT_TOP,
    )
    .show(ctx, |ui| boxed(ui, add));
    let width = response.response.rect.width();
    ctx.data_mut(|d| d.insert_temp(tools_width_id(), width));
}

/// The flyout of the active tool, right of the tool bar: boxes stacked down
/// from the top, left-aligned. The caller draws the boxes (see `boxed`).
pub fn flyout(ctx: &egui::Context, rect: Rect, add: impl FnOnce(&mut Ui)) {
    let bar = ctx
        .data(|d| d.get_temp::<f32>(tools_width_id()))
        .unwrap_or(32.0);
    area(
        "tsukumoe-viewport-flyout",
        rect.left_top() + vec2(2.0 * MARGIN + bar, MARGIN),
        Align2::LEFT_TOP,
    )
    .show(ctx, |ui| {
        ui.with_layout(Layout::top_down(Align::LEFT), add);
    });
}

/// A pill at the top centre saying what the viewport shows.
pub fn status(ctx: &egui::Context, rect: Rect, text: &str) {
    let theme = Theme::get(ctx);
    area(
        "tsukumoe-viewport-status",
        rect.center_top() + vec2(0.0, MARGIN),
        Align2::CENTER_TOP,
    )
    .interactable(false)
    .show(ctx, |ui| {
        Frame::new()
            .fill(theme.colors.accent)
            .corner_radius(theme.metrics.radius * 2.0)
            .inner_margin(Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.label(egui::RichText::new(text).color(theme.colors.accent_text));
            });
    });
}

/// [`status`] with a close button at its right. True on the frame it is clicked.
pub fn status_closable(ctx: &egui::Context, rect: Rect, text: &str, close_name: &str) -> bool {
    let theme = Theme::get(ctx);
    let mut closed = false;
    area(
        "tsukumoe-viewport-status",
        rect.center_top() + vec2(0.0, MARGIN),
        Align2::CENTER_TOP,
    )
    .show(ctx, |ui| {
        Frame::new()
            .fill(theme.colors.accent)
            .corner_radius(theme.metrics.radius * 2.0)
            .inner_margin(Margin::symmetric(8, 3))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(text).color(theme.colors.accent_text));
                    let size = theme.metrics.icon;
                    let (at, response) =
                        ui.allocate_exact_size(vec2(size + 4.0, size + 4.0), egui::Sense::click());
                    let shown = if response.hovered() {
                        theme.colors.accent_text
                    } else {
                        theme.colors.accent_text.gamma_multiply(0.75)
                    };
                    Icon::Close
                        .image(shown, size)
                        .paint_at(ui, Rect::from_center_size(at.center(), vec2(size, size)));
                    let response = response.on_hover_text(close_name);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, close_name)
                    });
                    closed = response.clicked();
                });
            });
    });
    closed
}

/// The line that says which key the sliders are on (or that they are on none).
fn key_line(ctx: &egui::Context, theme: &Theme, place: &Place) -> (String, Color32) {
    match place {
        Place::Key { label, .. } => (
            format!("{}: {label}", tr(ctx, "keyform-editing")),
            theme.colors.text,
        ),
        Place::Off { label } => (
            format!("{}: {label}", tr(ctx, "keyform-off")),
            theme.colors.text_muted,
        ),
    }
}

/// Help at the bottom left: what the pointer does now, and the key being
/// edited. It wraps to the viewport's width and lets clicks through.
pub fn help(ctx: &egui::Context, rect: Rect, text: &str, place: Option<&Place>) {
    let theme = Theme::get(ctx);
    area(
        "tsukumoe-viewport-help",
        rect.left_bottom() + vec2(MARGIN, -MARGIN),
        Align2::LEFT_BOTTOM,
    )
    .interactable(false)
    .show(ctx, |ui| {
        ui.set_max_width((rect.width() - 2.0 * MARGIN).max(80.0));
        Frame::new()
            .fill(theme.colors.surface_raised.gamma_multiply(0.9))
            .corner_radius(theme.metrics.radius)
            .inner_margin(Margin::symmetric(theme.metrics.pad as i8, 4))
            .show(ui, |ui| {
                if let Some(place) = place {
                    let (line, color) = key_line(ctx, &theme, place);
                    ui.label(egui::RichText::new(line).color(color));
                }
                ui.label(egui::RichText::new(text).color(theme.colors.text_muted));
            });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn help_shows_the_key_line_and_the_text_in_a_narrow_viewport() {
        let mut harness = Harness::builder()
            .with_size(vec2(120.0, 200.0))
            .build_ui(|ui| {
                tsukumoe_ui::init(ui.ctx());
                tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                let place = Place::Off {
                    label: "ParamAngleX = 12.00".into(),
                };
                help(
                    ui.ctx(),
                    ui.max_rect(),
                    "Click to select, Shift to add.",
                    Some(&place),
                );
            });
        harness.run();
        harness.get_by_label("Click to select, Shift to add.");
        harness.get_by_label("Not on a key: ParamAngleX = 12.00");
    }

    #[test]
    fn the_close_button_of_a_status_pill_reports_a_click() {
        let mut harness = Harness::builder()
            .with_size(vec2(300.0, 100.0))
            .build_ui_state(
                |ui, closed: &mut bool| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    *closed |=
                        status_closable(ui.ctx(), ui.max_rect(), "Solo: Head", "End the solo");
                },
                false,
            );
        harness.run();
        harness.get_by_label("Solo: Head");
        harness.get_by_label("End the solo").click();
        harness.run();
        assert!(*harness.state());
    }
}
