// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the bone tool draws over the viewport: the bones as lines between
//! joints, the joint being dragged, the weights of the picked bone on the
//! picked mesh as colours, the brush, and the small options box. It only
//! reads the tool and the shapes; the pointer handling is in the viewport.

use egui::{Color32, Painter, Pos2, Stroke, pos2};
use tsukumoe_ui::{Theme, Variant, button, tr};

use crate::bone_tool::{BoneTool, BrushKind, Joint, Mode};
use crate::mesh_tool::Mapper;

const JOINT: f32 = 4.5;
const JOINT_ACTIVE: f32 = 6.5;
const VERTEX: f32 = 3.0;

fn screen(m: &Mapper, p: [f32; 2]) -> Pos2 {
    let s = m.to_screen(p);
    pos2(s[0], s[1])
}

/// Blue at weight 0, through green and yellow, to red at 1.
pub fn heat_color(weight: f32) -> Color32 {
    let w = weight.clamp(0.0, 1.0);
    let (r, g, b) = if w < 0.5 {
        let t = w * 2.0;
        (0.0, t, 1.0 - t)
    } else {
        let t = (w - 0.5) * 2.0;
        (t, 1.0 - t * 0.5, 0.0)
    };
    Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// The bones: a line from each joint to its parent, a dot on each joint, the
/// picked bone larger and in the accent colour. A joint being dragged is drawn
/// where the pointer is.
pub fn paint_bones(
    painter: &Painter,
    theme: &Theme,
    m: &Mapper,
    joints: &[Joint],
    active: Option<&str>,
    dragged: Option<(&str, [f32; 2])>,
) {
    let c = &theme.colors;
    let at = |j: &Joint| match dragged {
        Some((name, pointer)) if name == j.name => pos2(pointer[0], pointer[1]),
        _ => screen(m, j.at),
    };
    for joint in joints {
        let Some(parent) = joint
            .parent
            .as_deref()
            .and_then(|p| joints.iter().find(|j| j.name == p))
        else {
            continue;
        };
        painter.line_segment([at(parent), at(joint)], Stroke::new(2.0, c.text));
    }
    for joint in joints {
        let on = active == Some(joint.name.as_str());
        let (radius, fill) = if on {
            (JOINT_ACTIVE, c.accent)
        } else {
            (JOINT, c.text)
        };
        painter.circle(at(joint), radius, fill, Stroke::new(1.5, c.background));
    }
}

/// The vertices of the picked mesh coloured by the weight of the picked bone,
/// over the triangle edges.
pub fn paint_weights(
    painter: &Painter,
    theme: &Theme,
    m: &Mapper,
    vertices: &[[f32; 2]],
    edges: &[[u32; 2]],
    heat: &[f32],
) {
    let faint = Stroke::new(1.0, theme.colors.text.gamma_multiply(0.35));
    for [a, b] in edges {
        if let (Some(a), Some(b)) = (vertices.get(*a as usize), vertices.get(*b as usize)) {
            painter.line_segment([screen(m, *a), screen(m, *b)], faint);
        }
    }
    for (v, w) in vertices.iter().zip(heat) {
        painter.circle_filled(screen(m, *v), VERTEX, heat_color(*w));
    }
}

/// The brush ring at the pointer.
pub fn paint_brush(painter: &Painter, theme: &Theme, at: Pos2, radius: f32) {
    painter.circle_stroke(at, radius, Stroke::new(1.5, theme.colors.accent));
}

/// The options box: the mode, and for weights the brush.
pub fn options(ui: &mut egui::Ui, tool: &mut BoneTool) {
    let ctx = ui.ctx().clone();
    let t = |key: &str| tr(&ctx, key);
    super::overlay::boxed(ui, |ui| {
        ui.horizontal(|ui| {
            for (mode, key) in [
                (Mode::Bones, "bone-mode-bones"),
                (Mode::Weights, "bone-mode-weights"),
            ] {
                let variant = if tool.mode == mode {
                    Variant::Primary
                } else {
                    Variant::Ghost
                };
                if button(ui, &t(key), variant).clicked() {
                    tool.mode = mode;
                    tool.cancel();
                }
            }
        });
        if tool.mode != Mode::Weights {
            return;
        }
        ui.horizontal(|ui| {
            for (kind, key) in [
                (BrushKind::Add, "bone-brush-add"),
                (BrushKind::Subtract, "bone-brush-subtract"),
                (BrushKind::Smooth, "bone-brush-smooth"),
            ] {
                let variant = if tool.brush.kind == kind {
                    Variant::Primary
                } else {
                    Variant::Ghost
                };
                if button(ui, &t(key), variant).clicked() {
                    tool.brush.kind = kind;
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label(t("bone-brush-radius"));
            ui.add(egui::Slider::new(&mut tool.brush.radius, 8.0..=200.0).show_value(false))
                .accessible_name(t("bone-brush-radius"));
        });
        ui.horizontal(|ui| {
            ui.label(t("bone-brush-strength"));
            ui.add(egui::Slider::new(&mut tool.brush.strength, 0.05..=1.0).show_value(false))
                .accessible_name(t("bone-brush-strength"));
        });
    });
}
