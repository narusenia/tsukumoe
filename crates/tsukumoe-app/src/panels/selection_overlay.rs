// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The bounding box of the selection, drawn over the viewport when no
//! vertex-level tool is open: a thin outline with a small dot on each corner.
//! It is shown only; there is nothing to drag.

use egui::{Painter, Rect, Stroke, StrokeKind, pos2, vec2};
use tsukumoe_core::model::Model;
use tsukumoe_ui::Theme;

use crate::actions::Tool;
use crate::mesh_tool::Mapper;
use crate::scene::Scene;
use crate::view::{Selection, ViewState};

/// Whether the box is drawn with this tool: the mesh tool has its own vertex
/// and edge overlay.
pub fn shown_with(tool: Tool) -> bool {
    tool != Tool::Mesh
}

/// `[x0, y0, x1, y1]` around what is selected, as drawn now: the vertices of
/// the selected meshes (all of them, with several), of the meshes under a
/// part, or of those hanging from a deformer; for a warp deformer, its grid
/// as well. Meshes that are not drawn are left out.
pub fn selection_bounds(scene: &Scene, model: &Model, view: &ViewState) -> Option<[f32; 4]> {
    let names = view.meshes_of(model);
    let mut b = scene.drawn_bounds(names.iter().map(String::as_str));
    if let Some(Selection::Warp(name)) = &view.selection
        && let Some(shape) = crate::deformer_tool::Shape::of(model, name, 0.0)
    {
        for p in shape.world_points() {
            b = Some(b.map_or([p[0], p[1], p[0], p[1]], |b| {
                [
                    b[0].min(p[0]),
                    b[1].min(p[1]),
                    b[2].max(p[0]),
                    b[3].max(p[1]),
                ]
            }));
        }
    }
    b
}

/// The edges that belong to one triangle only, as vertex pairs: the outline
/// of a mesh (an inner edge is shared by two triangles).
pub fn boundary_edges(indices: &[u32]) -> Vec<(u32, u32)> {
    let mut all: Vec<(u32, u32)> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect();
    all.sort_unstable();
    let mut out = Vec::new();
    let mut i = 0;
    while i < all.len() {
        let mut j = i;
        while j < all.len() && all[j] == all[i] {
            j += 1;
        }
        if j - i == 1 {
            out.push(all[i]);
        }
        i = j;
    }
    out
}

/// The meshes to outline when several things are selected, and the one
/// chosen last among them (drawn bolder). Nothing for a single selection:
/// the box says enough.
pub fn outlined_meshes(view: &ViewState) -> (Vec<String>, Option<String>) {
    let meshes = view.selected_meshes();
    if meshes.len() < 2 {
        return (Vec::new(), None);
    }
    let main = meshes.last().cloned();
    (meshes, main)
}

/// The outlines of the selected meshes where they are drawn: each in a faint
/// accent, the main one bold. Meshes that are not drawn are skipped.
pub fn paint_outlines(
    painter: &Painter,
    theme: &Theme,
    m: &Mapper,
    scene: &Scene,
    view: &ViewState,
) {
    let (names, main) = outlined_meshes(view);
    if names.is_empty() {
        return;
    }
    let c = &theme.colors;
    let list = scene.frame.draw_list(&scene.model);
    let shown = list.order();
    let clip = painter.clip_rect().expand(2.0);
    for name in &names {
        let Some(i) = scene.model.mesh_index(name) else {
            continue;
        };
        if !shown.contains(&(i as u32)) {
            continue;
        }
        let d = list.drawable(i);
        let points = d.positions.as_chunks::<2>().0;
        let is_main = main.as_deref() == Some(name.as_str());
        let stroke = if is_main {
            Stroke::new(2.5, c.accent)
        } else {
            Stroke::new(1.0, c.accent.gamma_multiply(0.55))
        };
        let screen = |v: u32| {
            points.get(v as usize).map(|p| {
                let s = m.to_screen(*p);
                pos2(s[0], s[1])
            })
        };
        let lines: Vec<egui::Shape> = boundary_edges(d.indices)
            .into_iter()
            .filter_map(|(a, b)| Some([screen(a)?, screen(b)?]))
            .filter(|l| Rect::from_two_pos(l[0], l[1]).intersects(clip))
            .map(|l| egui::Shape::line_segment(l, stroke))
            .collect();
        painter.extend(lines);
    }
}

/// A small badge with the number of things selected, inside the top right of the box.
pub fn paint_badge(painter: &Painter, theme: &Theme, m: &Mapper, bounds: [f32; 4], count: usize) {
    if count < 2 {
        return;
    }
    let c = &theme.colors;
    let corner = m.to_screen([bounds[2], bounds[3]]);
    let center = pos2(corner[0], corner[1]);
    let text = count.to_string();
    let galley = painter.layout_no_wrap(
        text,
        egui::FontId::proportional(theme.metrics.text - 1.0),
        c.accent_text,
    );
    let size = vec2(galley.size().x.max(10.0) + 10.0, galley.size().y + 2.0);
    // Inside the top right corner, so it is never cut by the edge of the view.
    let rect = Rect::from_center_size(center + vec2(-size.x / 2.0 - 4.0, size.y / 2.0 + 4.0), size);
    painter.rect_filled(rect, size.y / 2.0, c.accent);
    painter.galley(rect.center() - galley.size() / 2.0, galley, c.accent_text);
}

/// Draws the box `bounds` (drawing space) in screen points.
pub fn paint(painter: &Painter, theme: &Theme, m: &Mapper, bounds: [f32; 4]) {
    let c = &theme.colors;
    let (a, b) = (
        m.to_screen([bounds[0], bounds[1]]),
        m.to_screen([bounds[2], bounds[3]]),
    );
    let rect = Rect::from_two_pos(pos2(a[0], a[1]), pos2(b[0], b[1]));
    painter.rect_stroke(
        rect,
        0.0,
        Stroke::new(1.0, c.accent.gamma_multiply(0.85)),
        StrokeKind::Outside,
    );
    for corner in [
        rect.left_top(),
        rect.right_top(),
        rect.left_bottom(),
        rect.right_bottom(),
    ] {
        let dot = Rect::from_center_size(corner, vec2(6.0, 6.0));
        painter.rect_filled(dot, 1.0, c.surface);
        painter.rect_stroke(dot, 1.0, Stroke::new(1.0, c.accent), StrokeKind::Inside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;

    fn model() -> Model {
        let tri = |x: f32, y: f32| vec![[x, y], [x + 1.0, y], [x, y + 2.0]];
        let mut m = Model::new();
        let mut commands = vec![
            Command::AddPart {
                name: "Head".into(),
                parent: None,
            },
            Command::AddPart {
                name: "Hair".into(),
                parent: Some("Head".into()),
            },
            Command::AddPart {
                name: "Other".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Head".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[10.0, 10.0], [14.0, 10.0], [10.0, 16.0], [14.0, 16.0]],
            },
        ];
        for (name, part, x, y) in [
            ("A", "Head", 0.0, 0.0),
            ("B", "Hair", 5.0, 3.0),
            ("C", "Other", 50.0, 50.0),
            ("Under", "Head", 0.0, 0.0),
        ] {
            commands.push(Command::AddMesh {
                name: name.into(),
                part: part.into(),
                positions: tri(x, y),
                uvs: tri(0.0, 0.0),
                indices: vec![0, 1, 2],
            });
        }
        commands.push(Command::SetMeshParent {
            name: "Under".into(),
            parent: Some("W".into()),
        });
        for c in commands {
            m.apply(c).unwrap();
        }
        m
    }

    fn bounds_of(model: &Model, selection: Option<Selection>, more: &[&str]) -> Option<[f32; 4]> {
        let mut scene = Scene::new(model).unwrap();
        scene.advance(0.0);
        let mut view = ViewState::default();
        match selection {
            Some(Selection::Mesh(first)) => {
                let mut names = vec![first];
                names.extend(more.iter().map(|s| (*s).to_owned()));
                view.select_meshes(names);
            }
            other => view.selection = other,
        }
        selection_bounds(&scene, model, &view)
    }

    #[test]
    fn one_mesh_several_meshes_and_a_part_give_their_box() {
        let m = model();
        assert_eq!(
            bounds_of(&m, Some(Selection::Mesh("A".into())), &[]),
            Some([0.0, 0.0, 1.0, 2.0])
        );
        // Two meshes: one box around both.
        assert_eq!(
            bounds_of(&m, Some(Selection::Mesh("A".into())), &["C"]),
            Some([0.0, 0.0, 51.0, 52.0])
        );
        // A part: every mesh under it, including the part below it.
        let head = bounds_of(&m, Some(Selection::Part("Head".into())), &[]).unwrap();
        assert_eq!(head[0..2], [0.0, 0.0]);
        assert!(head[2] >= 6.0 && head[3] >= 5.0 && head[2] < 20.0);
        assert_eq!(bounds_of(&m, None, &[]), None);
        assert_eq!(
            bounds_of(&m, Some(Selection::Mesh("Nope".into())), &[]),
            None
        );
    }

    #[test]
    fn a_mesh_under_a_deformer_is_boxed_where_it_is_drawn() {
        let m = model();
        // Stored (0..1, 0..2) in the warp's (u, v); drawn over the grid [10, 14] x [10, 16].
        let b = bounds_of(&m, Some(Selection::Mesh("Under".into())), &[]).unwrap();
        assert!(
            (b[0] - 10.0).abs() < 1e-4 && (b[1] - 10.0).abs() < 1e-4,
            "{b:?}"
        );
        assert!((b[2] - 14.0).abs() < 1e-4 && b[3] > 16.0, "{b:?}");
        // The warp itself: its grid and what hangs from it.
        let w = bounds_of(&m, Some(Selection::Warp("W".into())), &[]).unwrap();
        assert!((w[0] - 10.0).abs() < 1e-4 && (w[2] - 14.0).abs() < 1e-4 && w[3] >= 16.0);
    }

    #[test]
    fn meshes_that_are_not_drawn_are_left_out() {
        let mut m = model();
        m.apply(Command::SetMeshVisible {
            name: "C".into(),
            visible: false,
        })
        .unwrap();
        assert_eq!(
            bounds_of(&m, Some(Selection::Mesh("A".into())), &["C"]),
            Some([0.0, 0.0, 1.0, 2.0])
        );
        assert_eq!(bounds_of(&m, Some(Selection::Mesh("C".into())), &[]), None);
        // A hidden part hides what is under it.
        m.apply(Command::SetPartVisible {
            name: "Hair".into(),
            visible: false,
        })
        .unwrap();
        assert_eq!(bounds_of(&m, Some(Selection::Mesh("B".into())), &[]), None);
    }

    #[test]
    fn the_outline_of_a_mesh_is_the_edges_of_one_triangle() {
        assert_eq!(boundary_edges(&[0, 1, 2]), [(0, 1), (0, 2), (1, 2)]);
        // A quad of two triangles: the diagonal is inside.
        assert_eq!(
            boundary_edges(&[0, 1, 2, 0, 2, 3]),
            [(0, 1), (0, 3), (1, 2), (2, 3)]
        );
        // Two separate triangles keep all six.
        assert_eq!(boundary_edges(&[0, 1, 2, 3, 4, 5]).len(), 6);
        assert!(boundary_edges(&[]).is_empty());
    }

    #[test]
    fn several_selected_meshes_are_outlined_with_the_last_one_main() {
        let mut view = ViewState::default();
        assert_eq!(outlined_meshes(&view), (Vec::new(), None));
        view.select_only(Selection::Mesh("A".into()));
        assert_eq!(outlined_meshes(&view), (Vec::new(), None));
        view.toggle_mesh("B");
        view.toggle_mesh("C");
        let (names, main) = outlined_meshes(&view);
        assert_eq!(names, ["A", "B", "C"]);
        assert_eq!(main.as_deref(), Some("C"));
        // Parts and deformers in the selection are not outlined.
        view.toggle(Selection::Part("P".into()));
        assert_eq!(outlined_meshes(&view).0, ["A", "B", "C"]);
        assert_eq!(view.selected_count(), 4);
    }

    #[test]
    fn the_box_is_not_drawn_with_the_mesh_tool() {
        assert!(!shown_with(Tool::Mesh));
        assert!(shown_with(Tool::Select) && shown_with(Tool::Deformer));
    }
}
