// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the mesh edit tool draws over the viewport: the edges and vertices of
//! the mesh being edited, the selection, the drag box, and a veil that dims
//! the picture behind. It only reads the tool and the mesh; the tool's
//! pointer handling is in the viewport.

use egui::{Align2, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};
use tsukumoe_core::elements::Point;
use tsukumoe_ui::Theme;

use crate::mesh_tool::{self, MeshTool};

/// Radius of a vertex dot, and of a selected one, in points.
const DOT: f32 = 3.0;
const DOT_SELECTED: f32 = 4.5;

/// Dims the picture so that the mesh being edited stands out.
pub fn veil(painter: &Painter, theme: &Theme, rect: Rect) {
    painter.rect_filled(rect, 0.0, theme.colors.background.gamma_multiply(0.45));
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
    if let Some((a, b)) = tool.rect() {
        let rect = Rect::from_two_pos(pos2(a[0], a[1]), pos2(b[0], b[1]));
        painter.rect_filled(rect, 0.0, c.accent.gamma_multiply(0.15));
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, c.accent), StrokeKind::Inside);
    }
}

/// A line of help in the corner of the viewport.
pub fn caption(painter: &Painter, theme: &Theme, rect: Rect, text: &str) {
    painter.text(
        rect.left_top() + vec2(8.0, 8.0),
        Align2::LEFT_TOP,
        text,
        FontId::proportional(theme.metrics.text),
        theme.colors.text_muted,
    );
}

/// The small options box in the top right corner: columns and rows of the
/// grid `mesh.fill-grid` lays. The values live in the tool, not in the model.
pub fn grid_options(ctx: &egui::Context, rect: Rect, tool: &mut MeshTool) {
    let theme = Theme::get(ctx);
    let m = theme.metrics;
    egui::Area::new(egui::Id::unique("tsukumoe-mesh-grid-options"))
        .fixed_pos(rect.right_top() + vec2(-8.0, 8.0))
        .pivot(Align2::RIGHT_TOP)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(theme.colors.surface_raised)
                .stroke(Stroke::new(1.0, theme.colors.border))
                .corner_radius(m.radius)
                .inner_margin(egui::Margin::symmetric(m.pad as i8, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(tsukumoe_ui::tr(ctx, "mesh-edit-grid"));
                        let range = 1..=tsukumoe_core::mesh_edit::MAX_GRID;
                        ui.add(egui::DragValue::new(&mut tool.grid.0).range(range.clone()))
                            .accessible_name(tsukumoe_ui::tr(ctx, "mesh-edit-grid-cols"));
                        ui.label("\u{d7}");
                        ui.add(egui::DragValue::new(&mut tool.grid.1).range(range))
                            .accessible_name(tsukumoe_ui::tr(ctx, "mesh-edit-grid-rows"));
                    });
                });
        });
}
