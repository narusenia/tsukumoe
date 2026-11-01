// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the deformer tool draws over the viewport: the grid and handles of
//! the deformer being edited, its parents and child deformers in other
//! colours, the selected control points and the drag box, and the small
//! options box for new warps. It only reads the tool and the shapes; the
//! pointer handling is in the viewport.

use egui::{Align2, Painter, Pos2, Rect, Shape as Draw, Stroke, StrokeKind, pos2, vec2};
use tsukumoe_ui::Theme;

use crate::deformer_tool::{DeformerTool, Handle, Role, Shape};
use crate::mesh_tool::Mapper;

const DOT: f32 = 3.5;
const DOT_SELECTED: f32 = 5.0;
const HANDLE: f32 = 5.0;

fn screen(m: &Mapper, p: [f32; 2]) -> Pos2 {
    let s = m.to_screen(p);
    pos2(s[0], s[1])
}

/// The lines of a warp grid: every row and every column.
fn grid_lines(points: &[Pos2], rows: u32, columns: u32) -> Vec<[Pos2; 2]> {
    let at = |r: u32, c: u32| points.get((r * (columns + 1) + c) as usize).copied();
    let mut lines = Vec::new();
    for r in 0..=rows {
        for c in 0..columns {
            if let (Some(a), Some(b)) = (at(r, c), at(r, c + 1)) {
                lines.push([a, b]);
            }
        }
    }
    for c in 0..=columns {
        for r in 0..rows {
            if let (Some(a), Some(b)) = (at(r, c), at(r + 1, c)) {
                lines.push([a, b]);
            }
        }
    }
    lines
}

/// A deformer other than the one being edited: its grid, or a cross at the
/// origin of a rotation, in the colour of its role.
fn paint_related(painter: &Painter, theme: &Theme, m: &Mapper, role: Role, shape: &Shape) {
    let c = &theme.colors;
    let color = match role {
        Role::Parent => c.warning,
        Role::Child => c.success,
    }
    .gamma_multiply(0.75);
    match shape {
        Shape::Warp { rows, columns, .. } => {
            let points: Vec<Pos2> = shape.world_points().iter().map(|&p| screen(m, p)).collect();
            painter.extend(
                grid_lines(&points, *rows, *columns)
                    .into_iter()
                    .map(|l| Draw::line_segment(l, Stroke::new(1.0, color))),
            );
        }
        Shape::Rotation { .. } => {
            if let Some((_, o)) = shape.handles().first() {
                let p = screen(m, *o);
                let r = 6.0;
                painter.line_segment(
                    [p - vec2(r, 0.0), p + vec2(r, 0.0)],
                    Stroke::new(1.5, color),
                );
                painter.line_segment(
                    [p - vec2(0.0, r), p + vec2(0.0, r)],
                    Stroke::new(1.5, color),
                );
            }
        }
    }
}

/// Draws `shape` (the one being edited, or its preview) with the other
/// deformers around it and the tool's selection.
pub fn paint(
    painter: &Painter,
    theme: &Theme,
    m: &Mapper,
    shape: &Shape,
    related: &[(Role, Shape)],
    tool: &DeformerTool,
) {
    let c = &theme.colors;
    for (role, other) in related {
        paint_related(painter, theme, m, *role, other);
    }
    match shape {
        Shape::Warp { rows, columns, .. } => {
            let points: Vec<Pos2> = shape.world_points().iter().map(|&p| screen(m, p)).collect();
            painter.extend(
                grid_lines(&points, *rows, *columns)
                    .into_iter()
                    .map(|l| Draw::line_segment(l, Stroke::new(1.5, c.accent))),
            );
            let clip = painter.clip_rect().expand(DOT_SELECTED);
            for (i, &p) in points.iter().enumerate() {
                if !clip.contains(p) {
                    continue;
                }
                if tool.points().contains(&(i as u32)) {
                    painter.circle_filled(p, DOT_SELECTED, c.accent);
                    painter.circle_stroke(p, DOT_SELECTED, Stroke::new(1.0, c.accent_text));
                } else {
                    painter.circle_filled(p, DOT, c.surface);
                    painter.circle_stroke(p, DOT, Stroke::new(1.0, c.accent));
                }
            }
        }
        Shape::Rotation { .. } => {
            let handles = shape.handles();
            let at = |h: Handle| {
                handles
                    .iter()
                    .find(|(k, _)| *k == h)
                    .map(|&(_, p)| screen(m, p))
            };
            if let (Some(o), Some(a), Some(s)) =
                (at(Handle::Origin), at(Handle::Angle), at(Handle::Scale))
            {
                painter.line_segment([o, a], Stroke::new(1.5, c.accent));
                painter.line_segment([o, s], Stroke::new(1.5, c.accent.gamma_multiply(0.6)));
                painter.circle_filled(o, DOT_SELECTED, c.accent);
                painter.circle_stroke(o, DOT_SELECTED, Stroke::new(1.0, c.accent_text));
                painter.circle_filled(a, HANDLE, c.surface);
                painter.circle_stroke(a, HANDLE, Stroke::new(1.5, c.accent));
                let corner = Rect::from_center_size(s, vec2(HANDLE * 1.8, HANDLE * 1.8));
                painter.rect_filled(corner, 1.0, c.surface);
                painter.rect_stroke(corner, 1.0, Stroke::new(1.5, c.accent), StrokeKind::Inside);
            }
        }
    }
    if let Some((a, b)) = tool.rect() {
        let rect = Rect::from_two_pos(pos2(a[0], a[1]), pos2(b[0], b[1]));
        painter.rect_filled(rect, 0.0, c.accent.gamma_multiply(0.15));
        painter.rect_stroke(rect, 0.0, Stroke::new(1.0, c.accent), StrokeKind::Inside);
    }
}

/// A line of help in the corner of the viewport.
pub fn caption(painter: &Painter, theme: &Theme, rect: Rect, text: &str) {
    super::mesh_overlay::caption(painter, theme, rect, text);
}

/// The options box in the top right corner: rows and columns of the warp that
/// `deformer.create-warp` makes. The values live in the tool, not in the model.
pub fn options(ctx: &egui::Context, rect: Rect, tool: &mut DeformerTool) {
    let theme = Theme::get(ctx);
    let m = theme.metrics;
    let t = |key: &str| tsukumoe_ui::tr(ctx, key);
    egui::Area::new(egui::Id::unique("tsukumoe-deformer-options"))
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
                        ui.label(t("deformer-warp-grid"));
                        let range = 1..=32u32;
                        ui.add(egui::DragValue::new(&mut tool.grid.0).range(range.clone()))
                            .accessible_name(t("deformer-warp-rows"));
                        ui.label("\u{d7}");
                        ui.add(egui::DragValue::new(&mut tool.grid.1).range(range))
                            .accessible_name(t("deformer-warp-columns"));
                    });
                });
        });
}
