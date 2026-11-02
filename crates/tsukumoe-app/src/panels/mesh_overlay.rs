// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the mesh edit tool draws over the viewport: the edges and vertices of
//! the mesh being edited, the selection, the drag box, and a veil that dims
//! the picture behind. It only reads the tool and the mesh; the tool's
//! pointer handling is in the viewport.

use egui::{Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2};
use tsukumoe_core::elements::Point;
use tsukumoe_ui::{Icon, Theme, Variant};

use crate::mesh_tool::{self, MeshTool};

/// Radius of a vertex dot, and of a selected one, in points.
const DOT: f32 = 3.0;
const DOT_SELECTED: f32 = 4.5;

/// Dims the picture so that the mesh being edited stands out: a veil over
/// `rect`, then `focus` (the edited mesh drawn alone on a transparent
/// texture) above it, so that mesh is not dimmed.
pub fn veil(painter: &Painter, theme: &Theme, rect: Rect, focus: Option<egui::TextureId>) {
    painter.rect_filled(rect, 0.0, theme.colors.background.gamma_multiply(0.45));
    if let Some(id) = focus {
        painter.image(
            id,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    }
}

/// Draws `positions` (or `preview` while a vertex drag is on) as edges and dots.
pub fn paint(
    painter: &Painter,
    theme: &Theme,
    m: &mesh_tool::Mapper,
    positions: &[Point],
    indices: &[u32],
    tool: &MeshTool,
    preview: Option<&[Point]>,
) {
    let c = &theme.colors;
    let clip = painter.clip_rect();
    let screen: Vec<Pos2> = preview
        .unwrap_or(positions)
        .iter()
        .map(|&p| {
            let s = m.to_screen(p);
            pos2(s[0], s[1])
        })
        .collect();
    let mut shapes = Vec::with_capacity(screen.len() * 2);
    let edge = Stroke::new(1.0, c.accent.gamma_multiply(0.7));
    let chosen = tool.edge();
    for (a, b) in mesh_tool::edges(indices) {
        let (Some(&pa), Some(&pb)) = (screen.get(a as usize), screen.get(b as usize)) else {
            continue;
        };
        if !Rect::from_two_pos(pa, pb).intersects(clip) || chosen == Some((a, b)) {
            continue;
        }
        shapes.push(Shape::line_segment([pa, pb], edge));
    }
    if let Some((a, b)) = chosen
        && let (Some(&pa), Some(&pb)) = (screen.get(a as usize), screen.get(b as usize))
    {
        shapes.push(Shape::line_segment([pa, pb], Stroke::new(2.5, c.warning)));
    }
    let margin = clip.expand(DOT_SELECTED);
    for (i, &p) in screen.iter().enumerate() {
        if !margin.contains(p) || tool.vertices().contains(&(i as u32)) {
            continue;
        }
        shapes.push(Shape::circle_filled(p, DOT, c.surface));
        shapes.push(Shape::circle_stroke(p, DOT, Stroke::new(1.0, c.accent)));
    }
    for &v in tool.vertices() {
        if let Some(&p) = screen.get(v as usize) {
            shapes.push(Shape::circle_filled(p, DOT_SELECTED, c.accent));
            shapes.push(Shape::circle_stroke(
                p,
                DOT_SELECTED,
                Stroke::new(1.0, c.accent_text),
            ));
        }
    }
    painter.extend(shapes);
    if let Some(path) = tool.lasso_path() {
        let points: Vec<Pos2> = path.iter().map(|p| pos2(p[0], p[1])).collect();
        painter.add(Shape::convex_polygon(
            points.clone(),
            c.accent.gamma_multiply(0.15),
            Stroke::NONE,
        ));
        painter.add(Shape::closed_line(points, Stroke::new(1.0, c.accent)));
    }
    if tool.symmetry {
        let top = m.to_screen([tool.center, m.view[3]]);
        let bottom = m.to_screen([tool.center, m.view[1]]);
        painter.line_segment(
            [pos2(top[0], top[1]), pos2(bottom[0], bottom[1])],
            Stroke::new(1.0, c.warning.gamma_multiply(0.7)),
        );
    }
    if let Some((a, b)) = tool.rect() {
        let rect = Rect::from_two_pos(pos2(a[0], a[1]), pos2(b[0], b[1]));
        painter.rect_filled(rect, 0.0, c.accent.gamma_multiply(0.15));
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, c.accent), StrokeKind::Inside);
    }
}

fn float(v: &mut f32, max: f32) -> egui::DragValue<'_> {
    egui::DragValue::new(v)
        .range(0.0..=max)
        .speed(0.1)
        .max_decimals(1)
}

/// The flyout of the mesh tool: its sub-tools and symmetry, the grid
/// (rows × columns, like the warp and the inspector) and the contour settings,
/// each with the button that runs it. Returns the action clicked. The values
/// live in the tool, not in the model.
pub fn grid_options(ui: &mut egui::Ui, tool: &mut MeshTool) -> Option<&'static str> {
    let ctx = ui.ctx().clone();
    let t = |key: &str| tsukumoe_ui::tr(&ctx, key);
    let run = |ui: &mut egui::Ui, key: &str, action: &'static str| {
        let text = t(key);
        tsukumoe_ui::button(ui, &text, Variant::Secondary)
            .on_hover_text(t(&crate::actions::label_key(action)))
            .clicked()
            .then_some(action)
    };
    let mut chosen = None;
    super::overlay::boxed(ui, |ui| {
        ui.horizontal(|ui| {
            for (pen, icon, key) in [
                (false, Icon::BoxSelect, "mesh-edit-select"),
                (true, Icon::PenLine, "mesh-edit-pen"),
            ] {
                let variant = if tool.pen == pen {
                    Variant::Primary
                } else {
                    Variant::Secondary
                };
                if tsukumoe_ui::icon_button(ui, icon, &t(key), variant).clicked() && tool.pen != pen
                {
                    tool.pen = pen;
                    tool.cancel();
                }
            }
            let variant = if tool.symmetry {
                Variant::Primary
            } else {
                Variant::Secondary
            };
            if tsukumoe_ui::icon_button(ui, Icon::Mirror, &t("mesh-edit-symmetry"), variant)
                .clicked()
            {
                tool.symmetry = !tool.symmetry;
            }
            chosen = chosen.or(run(ui, "mesh-edit-weld", "mesh.weld"));
        });
        ui.horizontal(|ui| {
            ui.label(t("mesh-edit-grid"));
            let range = 1..=tsukumoe_core::mesh_edit::MAX_GRID;
            // The tool keeps (columns, rows); the screen reads rows first.
            let mut edited = ui
                .add(egui::DragValue::new(&mut tool.grid.1).range(range.clone()))
                .accessible_name(t("mesh-edit-grid-rows"))
                .changed();
            ui.label("\u{d7}");
            edited |= ui
                .add(egui::DragValue::new(&mut tool.grid.0).range(range))
                .accessible_name(t("mesh-edit-grid-cols"))
                .changed();
            if edited {
                tool.grid_auto = false;
            }
            chosen = chosen.or(run(ui, "mesh-edit-make-grid", "mesh.fill-grid"));
        });
        ui.horizontal(|ui| {
            for n in [2, 4, 8] {
                let on = !tool.grid_auto && tool.grid == (n, n);
                let variant = if on { Variant::Primary } else { Variant::Ghost };
                if tsukumoe_ui::button(ui, &format!("{n}\u{d7}{n}"), variant).clicked() {
                    tool.grid = (n, n);
                    tool.grid_auto = false;
                }
            }
            let variant = if tool.grid_auto {
                Variant::Primary
            } else {
                Variant::Ghost
            };
            if tsukumoe_ui::button(ui, &t("mesh-edit-grid-auto"), variant).clicked() {
                tool.grid_auto = true;
            }
        });
        ui.separator();
        ui.label(t("mesh-edit-contour"));
        let c = &mut tool.contour;
        egui::Grid::new("tsukumoe-mesh-contour")
            .num_columns(2)
            .show(ui, |ui| {
                let key = "mesh-edit-contour-threshold";
                ui.label(t(key));
                ui.add(egui::DragValue::new(&mut c.threshold))
                    .accessible_name(t(key));
                ui.end_row();
                let rows: [(&str, &mut f32, f32); 3] = [
                    ("mesh-edit-contour-simplify", &mut c.simplify, 50.0),
                    ("mesh-edit-contour-offset", &mut c.offset, 50.0),
                    ("mesh-edit-contour-spacing", &mut c.spacing, 512.0),
                ];
                for (key, value, max) in rows {
                    ui.label(t(key));
                    ui.add(float(value, max)).accessible_name(t(key));
                    ui.end_row();
                }
            });
        // The spacing is 0 (none) or at least 4.
        if c.spacing > 0.0 && c.spacing < 4.0 {
            c.spacing = 4.0;
        }
        chosen = chosen.or(run(ui, "mesh-edit-make-contour", "mesh.contour"));
    });
    chosen
}

/// The edge pen's preview: the start vertex ringed, and a line from it to
/// the pointer.
pub fn pen_preview(
    painter: &Painter,
    theme: &Theme,
    m: &mesh_tool::Mapper,
    positions: &[Point],
    tool: &MeshTool,
    cursor: Option<Pos2>,
) {
    let Some(&from) = tool.pen_from().and_then(|v| positions.get(v as usize)) else {
        return;
    };
    let s = m.to_screen(from);
    let start = pos2(s[0], s[1]);
    let color = theme.colors.warning;
    painter.circle_stroke(start, DOT_SELECTED + 2.0, Stroke::new(1.5, color));
    if let Some(cursor) = cursor {
        painter.line_segment([start, cursor], Stroke::new(1.5, color.gamma_multiply(0.8)));
    }
}

/// The items of a right-click menu: one button per action, disabled when
/// `enabled` says the selection does not fit, with its shortcut on the right.
/// Returns the action that was clicked.
pub fn menu_ui(
    ui: &mut egui::Ui,
    actions: &[&'static str],
    enabled: &dyn Fn(&str) -> bool,
    shortcut: &dyn Fn(&str) -> Option<String>,
) -> Option<&'static str> {
    let ctx = ui.ctx().clone();
    let mut chosen = None;
    let item = |ui: &mut egui::Ui, action: &'static str| -> Option<&'static str> {
        let label = tsukumoe_ui::tr(&ctx, &crate::actions::label_key(action));
        let mut button = egui::Button::new(label);
        if let Some(keys) = shortcut(action) {
            button = button.shortcut_text(keys);
        }
        ui.add_enabled(enabled(action), button)
            .clicked()
            .then_some(action)
    };
    // Whole-mesh actions, then vertices and edges in sub-menus, then the
    // deformer makers; an action in none of them (the deformer tool's) stays on top.
    let has = |group: &[&'static str]| {
        group
            .iter()
            .copied()
            .filter(|a| actions.contains(a))
            .collect::<Vec<_>>()
    };
    let known: Vec<&str> = MENU_MESH
        .iter()
        .chain(&MENU_VERTICES)
        .chain(&MENU_EDGES)
        .chain(&MENU_DEFORMERS)
        .copied()
        .collect();
    for &action in actions.iter().filter(|a| !known.contains(a)) {
        chosen = chosen.or(item(ui, action));
    }
    for action in has(&MENU_MESH) {
        chosen = chosen.or(item(ui, action));
    }
    for (key, group) in [
        ("menu-group-vertices", &MENU_VERTICES[..]),
        ("menu-group-edges", &MENU_EDGES[..]),
    ] {
        let group = has(group);
        if group.is_empty() {
            continue;
        }
        let label = tsukumoe_ui::tr(&ctx, key);
        let any = group.iter().any(|a| enabled(a));
        let mut picked = None;
        ui.add_enabled_ui(any, |ui| {
            ui.menu_button(label, |ui| {
                for action in group {
                    picked = picked.or(item(ui, action));
                }
                if picked.is_some() {
                    ui.close();
                }
            });
        });
        chosen = chosen.or(picked);
    }
    let deformers = has(&MENU_DEFORMERS);
    if !deformers.is_empty() {
        ui.separator();
        for action in deformers {
            chosen = chosen.or(item(ui, action));
        }
    }
    chosen
}

const MENU_MESH: [&str; 3] = ["mesh.retriangulate", "mesh.fill-grid", "mesh.contour"];
const MENU_VERTICES: [&str; 6] = [
    "mesh.connect",
    "mesh.weld",
    "mesh.weld-near",
    "mesh.glue-source",
    "mesh.glue",
    "mesh.delete-vertices",
];
const MENU_EDGES: [&str; 3] = ["mesh.flip-edge", "mesh.edge-pen", "mesh.delete-edge"];
const MENU_DEFORMERS: [&str; 2] = ["deformer.create-warp", "deformer.create-rotation"];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_tool::MENU_ACTIONS;
    use egui::vec2;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use tsukumoe_i18n::{I18n, Language};

    fn menu_harness(tool: MeshTool) -> Harness<'static, (MeshTool, Option<&'static str>)> {
        Harness::builder()
            .with_size(vec2(420.0, 440.0))
            .build_ui_state(
                |ui, (tool, chosen): &mut (MeshTool, Option<&'static str>)| {
                    tsukumoe_ui::init(ui.ctx());
                    tsukumoe_ui::set_language(ui.ctx(), tsukumoe_ui::Language::En);
                    if let Some(action) =
                        menu_ui(ui, &MENU_ACTIONS, &|a| tool.menu_enabled(a), &|_| None)
                    {
                        *chosen = Some(action);
                    }
                },
                (tool, None),
            )
    }

    #[test]
    fn each_menu_item_sends_its_action_and_disabled_ones_send_nothing() {
        let t = I18n::new(Language::En);
        let label = |a: &str| t.tr(&crate::actions::label_key(a));
        for action in MENU_ACTIONS {
            assert!(crate::actions::ACTIONS.contains(&action), "{action}");
            // The whole-mesh items are enabled with nothing selected.
            let mut harness = menu_harness(MeshTool::default());
            harness.run();
            // Vertex and edge items sit in a sub-menu; a group with nothing enabled stays shut.
            let group = if MENU_VERTICES.contains(&action) {
                Some("Vertices")
            } else if MENU_EDGES.contains(&action) {
                Some("Edges")
            } else {
                None
            };
            if let Some(group) = group {
                harness.get_by_label(group).click();
                harness.run();
            }
            let text = label(action);
            let Some(node) = harness.query_by_label(&text) else {
                assert!(!MeshTool::default().menu_enabled(action), "{action}");
                continue;
            };
            node.click();
            harness.run();
            let expected = MeshTool::default().menu_enabled(action).then_some(action);
            assert_eq!(harness.state().1, expected, "{action}");
        }
    }
}
