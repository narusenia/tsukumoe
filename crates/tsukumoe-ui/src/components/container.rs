// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use egui::{
    Color32, CursorIcon, Frame, Margin, Response, Role, ScrollArea, Sense, Stroke, TextStyle, Ui,
    WidgetInfo, vec2,
};

use crate::icons::Icon;
use crate::theme::Theme;

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

/// A row of tabs; `selected` is the index of the open one. `changed()` is true
/// on the frame it changes.
pub fn tabs(ui: &mut Ui, selected: &mut usize, names: &[&str]) -> Response {
    let t = Theme::get(ui.ctx());
    let m = t.metrics;
    let mut outer = None::<Response>;
    let mut changed = false;
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (i, name) in names.iter().enumerate() {
            let font = TextStyle::Button.resolve(ui.style());
            let galley =
                ui.painter()
                    .layout_no_wrap((*name).to_owned(), font, Color32::PLACEHOLDER);
            let size = vec2(galley.size().x + 2.0 * m.pad, m.control);
            let (rect, response) = ui.allocate_exact_size(size, Sense::click());
            response
                .widget_info(|| WidgetInfo::selected(Role::Button, true, *selected == i, *name));
            response.clone().on_hover_cursor(CursorIcon::PointingHand);
            if response.clicked() && *selected != i {
                *selected = i;
                changed = true;
            }
            let active = *selected == i;
            let c = &t.colors;
            let color = if active || response.hovered() {
                c.text
            } else {
                c.text_muted
            };
            let painter = ui.painter();
            if response.hovered() && !active {
                painter.rect_filled(rect.shrink2(vec2(2.0, 3.0)), m.radius, c.control_hover);
            }
            let pos = egui::pos2(
                rect.center().x - galley.size().x / 2.0,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(pos, galley, color);
            if active {
                let bar = egui::Rect::from_min_max(
                    egui::pos2(rect.left() + m.gap, rect.bottom() - 2.0),
                    egui::pos2(rect.right() - m.gap, rect.bottom()),
                );
                painter.rect_filled(bar, 1.0, c.accent);
            }
            outer = Some(match outer.take() {
                Some(o) => o.union(response),
                None => response,
            });
        }
    });
    let mut response = outer.unwrap_or(row.response);
    if changed {
        response.mark_changed();
    }
    response
}

/// A full-width row for lists and trees. `clicked()` is true when picked.
pub fn selectable_row(ui: &mut Ui, selected: bool, icon: Option<Icon>, text: &str) -> Response {
    let t = Theme::get(ui.ctx());
    let m = t.metrics;
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), m.control), Sense::click());
    response.widget_info(|| WidgetInfo::selected(Role::Button, true, selected, text));
    let c = &t.colors;
    let painter = ui.painter();
    if selected {
        let [r, g, b, _] = c.accent.to_array();
        painter.rect_filled(
            rect,
            m.radius,
            Color32::from_rgba_unmultiplied(r, g, b, if t.dark { 60 } else { 40 }),
        );
    } else if response.hovered() {
        painter.rect_filled(rect, m.radius, c.control_hover);
    }
    let mut x = rect.left() + m.pad;
    let color = if selected { c.text } else { c.text_muted };
    if let Some(icon) = icon {
        let at = egui::Rect::from_min_size(
            egui::pos2(x, rect.center().y - m.icon / 2.0),
            vec2(m.icon, m.icon),
        );
        icon.image(color, m.icon).paint_at(ui, at);
        x += m.icon + m.gap + 2.0;
    }
    let font = TextStyle::Body.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);
    let pos = egui::pos2(x, rect.center().y - galley.size().y / 2.0);
    let color = if response.hovered() || selected {
        c.text
    } else {
        c.text_muted
    };
    ui.painter().galley(pos, galley, color);
    response
}

/// A scrolling list of `rows` equal-height rows; only the visible ones are
/// built. `add_row` gets the row index.
pub fn list(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    rows: usize,
    mut add_row: impl FnMut(&mut Ui, usize),
) {
    let height = Theme::get(ui.ctx()).metrics.control;
    ScrollArea::vertical()
        .id_salt(egui::Id::unique(id))
        .auto_shrink(false)
        .show_rows(ui, height, rows, |ui, range| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in range {
                add_row(ui, i);
            }
        });
}
