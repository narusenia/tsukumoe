// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the view aids draw over the viewport (M5-02e): the reference image
//! behind the model, the grid, the guides and the rulers, and the box that
//! switches them on. The settings are in `view_aids`; this file draws them and
//! reads the pointer for the guides.

use egui::{Align2, FontId, Id, Painter, Pos2, Rect, Sense, Stroke, TextureHandle, Ui, pos2, vec2};
use tsukumoe_ui::{Icon, Theme, Variant, button, checkbox, icon_button, number_edit, tr};

use crate::mesh_tool::Mapper;
use crate::view_aids::{self, Axis, GUIDE_GRAB, Reference, ViewAids};

/// The width of a ruler, in points.
pub const RULER: f32 = 16.0;
/// Most grid lines and ruler ticks drawn along one side.
const MAX_LINES: usize = 200;

fn at(m: &Mapper, p: [f32; 2]) -> Pos2 {
    let s = m.to_screen(p);
    pos2(s[0], s[1])
}

/// The visible model rectangle `[x0, y0, x1, y1]` of `m`, sorted.
fn visible(m: &Mapper) -> [f32; 4] {
    [
        m.view[0].min(m.view[2]),
        m.view[1].min(m.view[3]),
        m.view[0].max(m.view[2]),
        m.view[1].max(m.view[3]),
    ]
}

/// A tick's number with as many decimals as the step needs.
fn tick_text(value: f32, step: f32) -> String {
    let decimals = (-step.log10().floor()).max(0.0) as usize;
    format!("{value:.decimals$}")
}

/// Draws the reference image fitted to `bounds` (the model's), moved and scaled.
pub fn paint_reference(
    painter: &Painter,
    m: &Mapper,
    bounds: [f32; 4],
    texture: &TextureHandle,
    reference: &Reference,
) {
    let [w, h] = texture.size_vec2().into();
    if w <= 0.0 || h <= 0.0 || !reference.visible {
        return;
    }
    let width = (bounds[2] - bounds[0]) * reference.scale;
    let height = width * h / w;
    let centre = [
        (bounds[0] + bounds[2]) / 2.0 + reference.offset[0],
        (bounds[1] + bounds[3]) / 2.0 + reference.offset[1],
    ];
    let min = at(m, [centre[0] - width / 2.0, centre[1] + height / 2.0]);
    let max = at(m, [centre[0] + width / 2.0, centre[1] - height / 2.0]);
    let tint = egui::Color32::WHITE.gamma_multiply(reference.opacity.clamp(0.0, 1.0));
    painter.image(
        texture.id(),
        Rect::from_min_max(min, max),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        tint,
    );
}

/// The grid lines, fainter for the ones between the round steps' tenths.
pub fn paint_grid(painter: &Painter, theme: &Theme, rect: Rect, m: &Mapper, aids: &ViewAids) {
    let v = visible(m);
    let step = aids.step(v[2] - v[0], m.size[0]);
    let stroke = Stroke::new(1.0, theme.colors.border.gamma_multiply(0.6));
    for x in view_aids::ticks(v[0], v[2], step, MAX_LINES) {
        let sx = at(m, [x, 0.0]).x;
        painter.vline(sx, rect.y_range(), stroke);
    }
    for y in view_aids::ticks(v[1], v[3], step, MAX_LINES) {
        let sy = at(m, [0.0, y]).y;
        painter.hline(rect.x_range(), sy, stroke);
    }
}

pub fn paint_guides(painter: &Painter, theme: &Theme, rect: Rect, m: &Mapper, aids: &ViewAids) {
    let stroke = Stroke::new(1.0, theme.colors.accent);
    for &x in &aids.guides_x {
        painter.vline(at(m, [x, 0.0]).x, rect.y_range(), stroke);
    }
    for &y in &aids.guides_y {
        painter.hline(rect.x_range(), at(m, [0.0, y]).y, stroke);
    }
}

/// The rulers along the top and the left edge, with the pointer marked.
pub fn paint_rulers(
    painter: &Painter,
    theme: &Theme,
    rect: Rect,
    m: &Mapper,
    pointer: Option<Pos2>,
) {
    let v = visible(m);
    // The ruler's own step is the round one even when the grid's is set.
    let step = view_aids::nice_step(v[2] - v[0], m.size[0] / 60.0);
    let c = &theme.colors;
    let font = FontId::proportional(9.0);
    let top = Rect::from_min_size(rect.min, vec2(rect.width(), RULER));
    let left = Rect::from_min_size(rect.min, vec2(RULER, rect.height()));
    painter.rect_filled(top, 0.0, c.surface_raised);
    painter.rect_filled(left, 0.0, c.surface_raised);
    painter.hline(top.x_range(), top.bottom(), Stroke::new(1.0, c.border));
    painter.vline(left.right(), left.y_range(), Stroke::new(1.0, c.border));
    for x in view_aids::ticks(v[0], v[2], step, MAX_LINES) {
        let sx = at(m, [x, 0.0]).x;
        painter.vline(
            sx,
            top.bottom() - 5.0..=top.bottom(),
            Stroke::new(1.0, c.text_muted),
        );
        painter.text(
            pos2(sx + 2.0, top.top() + 1.0),
            Align2::LEFT_TOP,
            tick_text(x, step),
            font.clone(),
            c.text_muted,
        );
    }
    for y in view_aids::ticks(v[1], v[3], step, MAX_LINES) {
        let sy = at(m, [0.0, y]).y;
        painter.hline(
            left.right() - 5.0..=left.right(),
            sy,
            Stroke::new(1.0, c.text_muted),
        );
        painter.text(
            pos2(left.left() + 1.0, sy - 1.0),
            Align2::LEFT_BOTTOM,
            tick_text(y, step),
            font.clone(),
            c.text_muted,
        );
    }
    if let Some(p) = pointer.filter(|p| rect.contains(*p)) {
        let mark = Stroke::new(1.0, c.accent);
        painter.vline(p.x, top.y_range(), mark);
        painter.hline(left.x_range(), p.y, mark);
    }
}

/// Makes, moves and removes guides with the pointer: a drag from a ruler makes
/// one, a drag on a guide moves it, and one let go over a ruler is removed.
pub fn interact_guides(
    ui: &mut Ui,
    rect: Rect,
    m: &Mapper,
    aids: &mut ViewAids,
    making: &mut Option<Axis>,
) {
    let top = Rect::from_min_size(rect.min, vec2(rect.width(), RULER));
    let left = Rect::from_min_size(rect.min, vec2(RULER, rect.height()));
    let snap = |aids: &ViewAids, axis: Axis, v: f32| match (aids.snap_step(m), axis) {
        (Some(step), _) => (v / step).round() * step,
        _ => v,
    };
    let model_at = |p: Pos2| m.to_model([p.x, p.y]);
    for (axis, ruler) in [(Axis::Y, top), (Axis::X, left)] {
        let id = Id::unique(("view-aids-ruler", axis == Axis::X));
        let r = ui.interact(ruler, id, Sense::drag());
        let Some(p) = r.interact_pointer_pos() else {
            continue;
        };
        let pick = |p: Pos2| match axis {
            Axis::X => model_at(p)[0],
            Axis::Y => model_at(p)[1],
        };
        if r.drag_started() {
            let v = snap(aids, axis, pick(p));
            aids.guides_mut(axis).push(v);
            *making = Some(axis);
        }
    }
    // A guide being made follows the pointer wherever it goes.
    if let (Some(axis), Some(p)) = (*making, ui.ctx().pointer_latest_pos()) {
        let v = snap(
            aids,
            axis,
            match axis {
                Axis::X => model_at(p)[0],
                Axis::Y => model_at(p)[1],
            },
        );
        if let Some(last) = aids.guides_mut(axis).last_mut() {
            *last = v;
        }
        if !ui.input(|i| i.pointer.primary_down()) {
            let on_ruler = match axis {
                Axis::Y => top.contains(p),
                Axis::X => left.contains(p),
            };
            if on_ruler {
                aids.guides_mut(axis).pop();
            }
            *making = None;
        }
    }
    for axis in [Axis::X, Axis::Y] {
        let mut remove = None;
        for i in 0..aids.guides(axis).len() {
            if *making == Some(axis) && i + 1 == aids.guides(axis).len() {
                continue;
            }
            let g = aids.guides(axis)[i];
            let line = at(m, if axis == Axis::X { [g, 0.0] } else { [0.0, g] });
            let strip = match axis {
                Axis::X => Rect::from_min_max(
                    pos2(line.x - GUIDE_GRAB, rect.top() + RULER),
                    pos2(line.x + GUIDE_GRAB, rect.bottom()),
                ),
                Axis::Y => Rect::from_min_max(
                    pos2(rect.left() + RULER, line.y - GUIDE_GRAB),
                    pos2(rect.right(), line.y + GUIDE_GRAB),
                ),
            };
            let r = ui.interact(
                strip,
                Id::unique(("view-aids-guide", axis == Axis::X, i)),
                Sense::drag(),
            );
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(match axis {
                    Axis::X => egui::CursorIcon::ResizeHorizontal,
                    Axis::Y => egui::CursorIcon::ResizeVertical,
                });
            }
            if r.dragged()
                && let Some(p) = r.interact_pointer_pos()
            {
                let v = snap(
                    aids,
                    axis,
                    match axis {
                        Axis::X => model_at(p)[0],
                        Axis::Y => model_at(p)[1],
                    },
                );
                aids.guides_mut(axis)[i] = v;
            }
            if r.drag_stopped()
                && let Some(p) = r
                    .interact_pointer_pos()
                    .or_else(|| ui.ctx().pointer_latest_pos())
            {
                let on_ruler = match axis {
                    Axis::Y => top.contains(p),
                    Axis::X => left.contains(p),
                };
                if on_ruler {
                    remove = Some(i);
                }
            }
        }
        if let Some(i) = remove {
            aids.guides_mut(axis).remove(i);
        }
    }
}

/// The actions the view aids box asks the editor for.
pub const LOAD: &str = "view.reference-load";
pub const REMOVE: &str = "view.reference-remove";

/// The box that switches the aids on. Returns the actions asked for.
pub fn options(
    ui: &mut Ui,
    aids: &mut ViewAids,
    open: &mut bool,
    has_reference: bool,
) -> Vec<&'static str> {
    let ctx = ui.ctx().clone();
    let t = |k: &str| tr(&ctx, k);
    let mut actions = Vec::new();
    let variant = if *open {
        Variant::Primary
    } else {
        Variant::Ghost
    };
    if icon_button(ui, Icon::Eye, &t("view-aids"), variant).clicked() {
        *open = !*open;
    }
    if !*open {
        return actions;
    }
    super::overlay::boxed(ui, |ui| {
        checkbox(ui, &mut aids.grid, &t("view-aids-grid"));
        checkbox(ui, &mut aids.rulers, &t("view-aids-rulers"));
        checkbox(ui, &mut aids.snap, &t("view-aids-snap"));
        checkbox(ui, &mut aids.stats, &t("view-aids-stats"));
        ui.horizontal(|ui| {
            ui.label(t("view-aids-step"));
            if let Some(v) = number_edit(
                ui,
                aids.grid_step,
                0.0..=100_000.0,
                0.1,
                &t("view-aids-step"),
            ) {
                aids.grid_step = v.max(0.0);
            }
        });
        ui.add_enabled_ui(
            !aids.guides_x.is_empty() || !aids.guides_y.is_empty(),
            |ui| {
                if button(ui, &t("view-aids-guides-clear"), Variant::Secondary).clicked() {
                    aids.clear_guides();
                }
            },
        );
        ui.separator();
        if button(ui, &t("view-aids-reference-load"), Variant::Secondary).clicked() {
            actions.push(LOAD);
        }
        if has_reference {
            if let Some(reference) = aids.reference.as_mut() {
                checkbox(
                    ui,
                    &mut reference.visible,
                    &t("view-aids-reference-visible"),
                );
                ui.horizontal(|ui| {
                    ui.label(t("view-aids-reference-opacity"));
                    ui.add(egui::Slider::new(&mut reference.opacity, 0.0..=1.0).show_value(false))
                        .accessible_name(t("view-aids-reference-opacity"));
                });
                ui.horizontal(|ui| {
                    ui.label(t("view-aids-reference-scale"));
                    if let Some(v) = number_edit(
                        ui,
                        reference.scale,
                        0.01..=100.0,
                        0.01,
                        &t("view-aids-reference-scale"),
                    ) {
                        reference.scale = v;
                    }
                });
                for (axis, key) in [(0, "view-aids-reference-x"), (1, "view-aids-reference-y")] {
                    ui.horizontal(|ui| {
                        ui.label(t(key));
                        if let Some(v) =
                            number_edit(ui, reference.offset[axis], -1.0e6..=1.0e6, 0.5, &t(key))
                        {
                            reference.offset[axis] = v;
                        }
                    });
                }
            }
            if button(ui, &t("view-aids-reference-remove"), Variant::Secondary).clicked() {
                actions.push(REMOVE);
            }
        }
    });
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    struct State {
        aids: ViewAids,
        open: bool,
        actions: Vec<&'static str>,
    }

    fn harness(reference: bool) -> Harness<'static, State> {
        let mut aids = ViewAids {
            guides_x: vec![2.0],
            ..ViewAids::default()
        };
        if reference {
            aids.reference = Some(Reference::default());
        }
        Harness::builder()
            .with_size(egui::vec2(300.0, 500.0))
            .build_ui_state(
                move |ui, s: &mut State| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    let actions = options(ui, &mut s.aids, &mut s.open, reference);
                    s.actions.extend(actions);
                },
                State {
                    aids,
                    open: true,
                    actions: Vec::new(),
                },
            )
    }

    #[test]
    fn the_box_switches_the_aids_and_clears_the_guides() {
        let mut h = harness(false);
        h.run();
        for label in ["Grid", "Rulers", "Snap", "Info"] {
            h.get_by_label(label).click();
            h.run();
        }
        let a = &h.state().aids;
        assert!(a.grid && a.rulers && a.snap && a.stats);
        h.get_by_label("Clear guides").click();
        h.run();
        assert!(h.state().aids.guides_x.is_empty());
    }

    #[test]
    fn the_reference_buttons_ask_the_editor() {
        let mut h = harness(false);
        h.run();
        h.get_by_label("Load reference image").click();
        h.run();
        assert_eq!(h.state().actions, [LOAD]);
        assert!(h.query_by_label("Remove reference image").is_none());

        let mut h = harness(true);
        h.run();
        h.get_by_label("Remove reference image").click();
        h.run();
        assert_eq!(h.state().actions, [REMOVE]);
        h.get_by_label("Show reference image").click();
        h.run();
        assert!(!h.state().aids.reference.unwrap().visible);
    }

    #[test]
    fn the_box_is_closed_until_its_button_is_pressed() {
        let mut h = harness(false);
        h.state_mut().open = false;
        h.run();
        assert!(h.query_by_label("Grid").is_none());
        h.get_by_label("View aids").click();
        h.run();
        assert!(h.state().open && h.query_by_label("Grid").is_some());
    }

    #[test]
    fn tick_numbers_have_the_decimals_their_step_needs() {
        assert_eq!(tick_text(10.0, 10.0), "10");
        assert_eq!(tick_text(0.5, 0.5), "0.5");
        assert_eq!(tick_text(0.25, 0.05), "0.25");
    }
}
