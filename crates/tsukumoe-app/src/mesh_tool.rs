// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The mesh edit tool as plain state and functions: what is selected, what a
//! drag is doing, and the command each operation sends. It never touches the
//! model (ADR-0019): every operation returns a `Command` for the dispatcher,
//! and a drag only changes the preview until it is released. The viewport
//! feeds it pointer positions; `panels::mesh_overlay` draws it.

use std::collections::BTreeSet;

use tsukumoe_core::command::Command;
use tsukumoe_core::elements::Point;
use tsukumoe_core::mesh_edit::{self, EditError, Geometry};
use tsukumoe_core::model::Model;

/// A vertex is picked within this many points of the pointer.
pub const VERTEX_RADIUS: f32 = 8.0;
/// An edge is picked within this many points of the pointer.
pub const EDGE_RADIUS: f32 = 5.0;
/// A press that moves less than this many points is a click.
const DRAG_THRESHOLD: f32 = 3.0;

/// Model space (y up) to the viewport's screen rectangle (points, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mapper {
    /// Top left of the viewport on screen.
    pub origin: [f32; 2],
    pub size: [f32; 2],
    /// The visible model rectangle `[x0, y0, x1, y1]`.
    pub view: [f32; 4],
}

impl Mapper {
    pub fn to_screen(&self, p: Point) -> [f32; 2] {
        let (w, h) = (self.view[2] - self.view[0], self.view[3] - self.view[1]);
        [
            self.origin[0] + (p[0] - self.view[0]) / w * self.size[0],
            self.origin[1] + (self.view[3] - p[1]) / h * self.size[1],
        ]
    }

    pub fn to_model(&self, s: [f32; 2]) -> Point {
        let (w, h) = (self.view[2] - self.view[0], self.view[3] - self.view[1]);
        [
            self.view[0] + (s[0] - self.origin[0]) / self.size[0].max(1e-6) * w,
            self.view[3] - (s[1] - self.origin[1]) / self.size[1].max(1e-6) * h,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Vertex(u32),
    /// An edge by its two vertices, smaller index first.
    Edge(u32, u32),
    Nothing,
}

/// What was asked of the tool that it could not do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolError {
    NeedVertices,
    NeedTwoVertices,
    NeedEdge,
    /// Re-triangulating would change nothing.
    NoChange,
    Edit(EditError),
}

impl ToolError {
    /// The translation key of the message.
    pub fn key(self) -> &'static str {
        match self {
            Self::NeedVertices => "mesh-edit-need-vertices",
            Self::NeedTwoVertices => "mesh-edit-need-two",
            Self::NeedEdge => "mesh-edit-need-edge",
            Self::NoChange => "mesh-edit-no-change",
            Self::Edit(EditError::OutsideMesh) => "mesh-edit-outside",
            Self::Edit(EditError::TooClose) => "mesh-edit-too-close",
            Self::Edit(EditError::BoundaryEdge) => "mesh-edit-outline-edge",
            Self::Edit(EditError::NotConvex) => "mesh-edit-not-convex",
            Self::Edit(EditError::CannotRemove) => "mesh-edit-cannot-remove",
            Self::Edit(EditError::CannotConnect) => "mesh-edit-cannot-connect",
            Self::Edit(EditError::BadVertex | EditError::NoSuchEdge) => "mesh-edit-no-edge",
        }
    }
}

impl From<EditError> for ToolError {
    fn from(e: EditError) -> Self {
        Self::Edit(e)
    }
}

/// A command and the name it gets in the history list.
pub type Op = (Command, String);

#[derive(Clone, Debug, PartialEq)]
enum Gesture {
    /// Pressed, not yet moved far enough to be a drag.
    Pending { at: [f32; 2], hit: Hit, shift: bool },
    /// Dragging the selected vertices; `from` and `to` are screen points.
    Move { from: [f32; 2], to: [f32; 2] },
    /// Dragging a box.
    Rect {
        from: [f32; 2],
        to: [f32; 2],
        add: bool,
    },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshTool {
    mesh: Option<String>,
    vertices: BTreeSet<u32>,
    edge: Option<(u32, u32)>,
    gesture: Option<Gesture>,
}

fn dist_to_segment(p: [f32; 2], a: [f32; 2], b: [f32; 2]) -> f32 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len2 = dx * dx + dy * dy;
    let t = if len2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p[0] - (a[0] + t * dx)).hypot(p[1] - (a[1] + t * dy))
}

/// The unique edges of a triangle list, smaller index first.
pub fn edges(indices: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<_> = indices
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .map(|(a, b)| (a.min(b), a.max(b)))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The vertex, else the edge, under `at` (screen points).
pub fn hit_test(positions: &[Point], indices: &[u32], m: &Mapper, at: [f32; 2]) -> Hit {
    let screen: Vec<[f32; 2]> = positions.iter().map(|&p| m.to_screen(p)).collect();
    let dist = |i: usize| (screen[i][0] - at[0]).hypot(screen[i][1] - at[1]);
    let vertex = (0..screen.len())
        .map(|i| (i, dist(i)))
        .filter(|&(_, d)| d <= VERTEX_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((i, _)) = vertex {
        return Hit::Vertex(i as u32);
    }
    let edge = edges(indices)
        .into_iter()
        .filter(|&(a, b)| (a.max(b) as usize) < screen.len())
        .map(|e| {
            let d = dist_to_segment(at, screen[e.0 as usize], screen[e.1 as usize]);
            (e, d)
        })
        .filter(|&(_, d)| d <= EDGE_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    edge.map_or(Hit::Nothing, |((a, b), _)| Hit::Edge(a, b))
}

/// The visible mesh drawn in front at `at` (a model point), by name. Meshes are
/// taken at their base shape, as the tool shows them.
pub fn pick_mesh(model: &Model, at: Point) -> Option<String> {
    let inside = |p: &[Point], t: &[u32]| {
        let side =
            |a: Point, b: Point| (b[0] - a[0]) * (at[1] - a[1]) - (b[1] - a[1]) * (at[0] - a[0]);
        let ok = |i: u32| (i as usize) < p.len();
        t.as_chunks::<3>().0.iter().any(|v| {
            if !v.iter().all(|&i| ok(i)) {
                return false;
            }
            let s = [
                side(p[v[0] as usize], p[v[1] as usize]),
                side(p[v[1] as usize], p[v[2] as usize]),
                side(p[v[2] as usize], p[v[0] as usize]),
            ];
            s.iter().all(|&x| x >= 0.0) || s.iter().all(|&x| x <= 0.0)
        })
    };
    model
        .meshes()
        .iter()
        .filter(|(_, _, m)| m.visible && inside(&m.positions, &m.indices))
        .max_by(|a, b| a.2.draw_order.total_cmp(&b.2.draw_order))
        .map(|(_, name, _)| name.to_owned())
}

/// The mesh `name` as a [`Geometry`].
pub fn geometry_of(model: &Model, name: &str) -> Option<Geometry> {
    let m = model.meshes().get(model.meshes().find(name)?)?;
    Some(Geometry {
        positions: m.positions.clone(),
        uvs: m.uvs.clone(),
        indices: m.indices.clone(),
    })
}

fn geometry_op(name: &str, label: &str, g: Geometry) -> Op {
    (
        Command::SetMeshGeometry {
            name: name.to_owned(),
            positions: g.positions,
            uvs: g.uvs,
            indices: g.indices,
        },
        format!("{label}: {name}"),
    )
}

impl MeshTool {
    pub fn mesh(&self) -> Option<&str> {
        self.mesh.as_deref()
    }

    /// Sets the mesh being edited; a different one drops the selection.
    pub fn set_mesh(&mut self, name: Option<&str>) {
        if self.mesh.as_deref() != name {
            *self = Self {
                mesh: name.map(str::to_owned),
                ..Self::default()
            };
        }
    }

    pub fn vertices(&self) -> &BTreeSet<u32> {
        &self.vertices
    }

    pub fn edge(&self) -> Option<(u32, u32)> {
        self.edge
    }

    pub fn is_dragging(&self) -> bool {
        matches!(
            self.gesture,
            Some(Gesture::Move { .. } | Gesture::Rect { .. })
        )
    }

    /// The box being dragged, as screen corners.
    pub fn rect(&self) -> Option<([f32; 2], [f32; 2])> {
        match self.gesture {
            Some(Gesture::Rect { from, to, .. }) => Some((from, to)),
            _ => None,
        }
    }

    /// Drops what no longer exists after the mesh changed (an undo, say).
    pub fn sanitize(&mut self, vertex_count: usize) {
        self.vertices.retain(|&v| (v as usize) < vertex_count);
        if self
            .edge
            .is_some_and(|(a, b)| a.max(b) as usize >= vertex_count)
        {
            self.edge = None;
        }
        if self.gesture.is_some() && vertex_count == 0 {
            self.gesture = None;
        }
    }

    pub fn clear_selection(&mut self) {
        self.vertices.clear();
        self.edge = None;
    }

    fn select(&mut self, hit: Hit, shift: bool) {
        match hit {
            Hit::Vertex(v) => {
                self.edge = None;
                if shift {
                    if !self.vertices.remove(&v) {
                        self.vertices.insert(v);
                    }
                } else {
                    self.vertices = BTreeSet::from([v]);
                }
            }
            Hit::Edge(a, b) => {
                if !shift {
                    self.vertices.clear();
                }
                self.edge = Some((a, b));
            }
            Hit::Nothing => {
                if !shift {
                    self.clear_selection();
                }
            }
        }
    }

    /// Selects the vertices inside the screen box.
    fn select_in(&mut self, positions: &[Point], m: &Mapper, a: [f32; 2], b: [f32; 2], add: bool) {
        if !add {
            self.clear_selection();
        }
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        for (i, &p) in positions.iter().enumerate() {
            let s = m.to_screen(p);
            if (x0..=x1).contains(&s[0]) && (y0..=y1).contains(&s[1]) {
                self.vertices.insert(i as u32);
            }
        }
        self.edge = None;
    }

    /// A press at `at`. What it does waits for the pointer to move or release.
    pub fn press(
        &mut self,
        positions: &[Point],
        indices: &[u32],
        m: &Mapper,
        at: [f32; 2],
        shift: bool,
    ) {
        let hit = hit_test(positions, indices, m, at);
        self.gesture = Some(Gesture::Pending { at, hit, shift });
    }

    /// The pointer moved with the button down.
    pub fn drag(&mut self, to: [f32; 2]) {
        self.gesture = match self.gesture.take() {
            Some(Gesture::Pending { at, hit, shift }) => {
                if (to[0] - at[0]).hypot(to[1] - at[1]) < DRAG_THRESHOLD {
                    Some(Gesture::Pending { at, hit, shift })
                } else if let Hit::Vertex(v) = hit {
                    // Dragging a vertex that is not selected selects it alone
                    // (with Shift, adds it); dragging a selected one moves them all.
                    if !self.vertices.contains(&v) {
                        if !shift {
                            self.vertices.clear();
                        }
                        self.vertices.insert(v);
                    }
                    self.edge = None;
                    Some(Gesture::Move { from: at, to })
                } else {
                    Some(Gesture::Rect {
                        from: at,
                        to,
                        add: shift,
                    })
                }
            }
            Some(Gesture::Move { from, .. }) => Some(Gesture::Move { from, to }),
            Some(Gesture::Rect { from, add, .. }) => Some(Gesture::Rect { from, to, add }),
            None => None,
        };
    }

    /// Abandons a drag (Escape) without changing anything.
    pub fn cancel(&mut self) {
        self.gesture = None;
    }

    /// `positions` with the selected vertices moved by the drag from `from` to `to`.
    fn shifted(&self, positions: &[Point], m: &Mapper, from: [f32; 2], to: [f32; 2]) -> Vec<Point> {
        let (a, b) = (m.to_model(from), m.to_model(to));
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut out = positions.to_vec();
        for &v in &self.vertices {
            if let Some(p) = out.get_mut(v as usize) {
                *p = [p[0] + d[0], p[1] + d[1]];
            }
        }
        out
    }

    /// The vertex positions with the dragged ones moved, while a move is on.
    /// The model is not changed.
    pub fn preview(&self, positions: &[Point], m: &Mapper) -> Option<Vec<Point>> {
        let Some(Gesture::Move { from, to }) = self.gesture else {
            return None;
        };
        Some(self.shifted(positions, m, from, to))
    }

    /// The button is released: a click selects, a box selects what is inside,
    /// a move gives its one command.
    pub fn release(&mut self, name: &str, positions: &[Point], m: &Mapper) -> Option<Op> {
        match self.gesture.take()? {
            Gesture::Pending { hit, shift, .. } => {
                self.select(hit, shift);
                None
            }
            Gesture::Rect { from, to, add } => {
                self.select_in(positions, m, from, to, add);
                None
            }
            Gesture::Move { from, to } => {
                if from == to {
                    return None;
                }
                let moved = self.shifted(positions, m, from, to);
                Some((
                    Command::SetMeshPositions {
                        name: name.to_owned(),
                        positions: moved,
                    },
                    format!("mesh_move_vertices: {name}"),
                ))
            }
        }
    }

    /// A click that selects, without a press and release in between.
    pub fn click(
        &mut self,
        positions: &[Point],
        indices: &[u32],
        m: &Mapper,
        at: [f32; 2],
        shift: bool,
    ) {
        let hit = hit_test(positions, indices, m, at);
        self.select(hit, shift);
    }

    /// Adds a vertex at `at`, or splits the edge under it, and selects the new vertex.
    pub fn add_at(
        &mut self,
        name: &str,
        g: &Geometry,
        m: &Mapper,
        at: [f32; 2],
    ) -> Result<Op, ToolError> {
        let (out, label) = match hit_test(&g.positions, &g.indices, m, at) {
            Hit::Vertex(_) => return Err(EditError::TooClose.into()),
            Hit::Edge(a, b) => {
                let (pa, pb) = (
                    m.to_screen(g.positions[a as usize]),
                    m.to_screen(g.positions[b as usize]),
                );
                let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
                let t =
                    ((at[0] - pa[0]) * dx + (at[1] - pa[1]) * dy) / (dx * dx + dy * dy).max(1e-9);
                (
                    mesh_edit::split_edge(g, a, b, t.clamp(0.0, 1.0))?,
                    "mesh_split_edge",
                )
            }
            Hit::Nothing => (mesh_edit::add_vertex(g, m.to_model(at))?, "mesh_add_vertex"),
        };
        self.vertices = BTreeSet::from([g.positions.len() as u32]);
        self.edge = None;
        Ok(geometry_op(name, label, out))
    }

    /// Removes the selected vertices as one command.
    pub fn delete_selected(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        if self.vertices.is_empty() {
            return Err(ToolError::NeedVertices);
        }
        let ids: Vec<u32> = self.vertices.iter().copied().collect();
        let out = mesh_edit::remove_vertices(g, &ids)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_remove_vertices", out))
    }

    /// Draws an edge between the two selected vertices and selects it.
    pub fn connect_selected(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let [a, b] = self.vertices.iter().copied().collect::<Vec<_>>()[..] else {
            return Err(ToolError::NeedTwoVertices);
        };
        let out = mesh_edit::connect(g, a, b)?;
        self.edge = Some((a, b));
        self.vertices.clear();
        Ok(geometry_op(name, "mesh_connect", out))
    }

    /// Re-triangulates the whole mesh as a constrained Delaunay triangulation;
    /// the selected edge stays as it is.
    pub fn retriangulate(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let keep: Vec<(u32, u32)> = self.edge.into_iter().collect();
        let out = mesh_edit::retriangulate(g, &keep)?;
        if out == *g {
            return Err(ToolError::NoChange);
        }
        Ok(geometry_op(name, "mesh_retriangulate", out))
    }

    /// Flips the selected edge; the selection is dropped, as the edge is gone.
    pub fn flip_selected(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let (a, b) = self.edge.ok_or(ToolError::NeedEdge)?;
        let out = mesh_edit::flip_edge(g, a, b)?;
        self.edge = None;
        Ok(geometry_op(name, "mesh_flip_edge", out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A square of two triangles, 100 points wide at the origin of the screen.
    fn square() -> (Geometry, Mapper) {
        let g = Geometry {
            positions: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        // Model y up, screen y down: model (0,0) is screen (0,100).
        let m = Mapper {
            origin: [0.0, 0.0],
            size: [100.0, 100.0],
            view: [0.0, 0.0, 1.0, 1.0],
        };
        (g, m)
    }

    fn tool() -> MeshTool {
        let mut t = MeshTool::default();
        t.set_mesh(Some("Skin"));
        t
    }

    fn click(t: &mut MeshTool, g: &Geometry, m: &Mapper, at: [f32; 2], shift: bool) {
        t.click(&g.positions, &g.indices, m, at, shift);
    }

    #[test]
    fn the_mapper_round_trips_and_flips_y() {
        let (_, m) = square();
        assert_eq!(m.to_screen([0.0, 0.0]), [0.0, 100.0]);
        assert_eq!(m.to_screen([1.0, 1.0]), [100.0, 0.0]);
        let p = m.to_model([25.0, 40.0]);
        assert!((p[0] - 0.25).abs() < 1e-6 && (p[1] - 0.6).abs() < 1e-6);
    }

    #[test]
    fn clicks_pick_a_vertex_before_an_edge_and_shift_toggles() {
        let (g, m) = square();
        let mut t = tool();
        click(&mut t, &g, &m, [2.0, 98.0], false);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [0]);
        click(&mut t, &g, &m, [98.0, 98.0], true);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [0, 1]);
        click(&mut t, &g, &m, [2.0, 98.0], true);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [1]);
        // The diagonal at its middle: an edge, which replaces the vertices.
        click(&mut t, &g, &m, [50.0, 50.0], false);
        assert_eq!((t.edge(), t.vertices().len()), (Some((0, 2)), 0));
        // Empty space clears; with Shift it keeps.
        click(&mut t, &g, &m, [80.0, 30.0], true);
        assert_eq!(t.edge(), Some((0, 2)));
        click(&mut t, &g, &m, [80.0, 30.0], false);
        assert_eq!(t.edge(), None);
    }

    #[test]
    fn a_box_selects_the_vertices_inside_and_shift_adds() {
        let (g, m) = square();
        let mut t = tool();
        t.press(&g.positions, &g.indices, &m, [60.0, 60.0], false);
        t.drag([110.0, -10.0]);
        assert!(t.is_dragging() && t.rect().is_some());
        assert!(t.release("Skin", &g.positions, &m).is_none());
        // Only the vertex at screen (100, 0) is inside.
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [2]);
        t.press(&g.positions, &g.indices, &m, [-10.0, 60.0], true);
        t.drag([40.0, 110.0]);
        t.release("Skin", &g.positions, &m);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [0, 2]);
    }

    #[test]
    fn a_move_previews_without_a_command_and_ends_in_one() {
        let (g, m) = square();
        let mut t = tool();
        click(&mut t, &g, &m, [100.0, 100.0], false);
        t.press(&g.positions, &g.indices, &m, [100.0, 100.0], false);
        t.drag([101.0, 100.0]);
        // Below the threshold it is still a click.
        assert!(!t.is_dragging());
        t.drag([90.0, 80.0]);
        let preview = t.preview(&g.positions, &m).unwrap();
        assert!((preview[1][0] - 0.9).abs() < 1e-6 && (preview[1][1] - 0.2).abs() < 1e-6);
        assert_eq!(preview[0], g.positions[0]);
        t.drag([80.0, 80.0]);
        let (command, label) = t.release("Skin", &g.positions, &m).unwrap();
        assert_eq!(label, "mesh_move_vertices: Skin");
        let Command::SetMeshPositions { name, positions } = command else {
            panic!("a move sends SetMeshPositions");
        };
        assert_eq!(name, "Skin");
        assert!((positions[1][0] - 0.8).abs() < 1e-6 && (positions[1][1] - 0.2).abs() < 1e-6);
        assert!(t.preview(&g.positions, &m).is_none());
        // Cancelling leaves nothing to send.
        t.press(&g.positions, &g.indices, &m, [80.0, 80.0], false);
        t.drag([10.0, 10.0]);
        t.cancel();
        assert!(t.release("Skin", &g.positions, &m).is_none());
    }

    #[test]
    fn dragging_an_unselected_vertex_selects_only_it() {
        let (g, m) = square();
        let mut t = tool();
        click(&mut t, &g, &m, [0.0, 100.0], false);
        t.press(&g.positions, &g.indices, &m, [100.0, 0.0], false);
        t.drag([60.0, 60.0]);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [2]);
    }

    #[test]
    fn the_model_is_untouched_until_the_command_is_applied() {
        use tsukumoe_core::model::Model;
        let (g, m) = square();
        let mut model = Model::new();
        model
            .apply(Command::AddPart {
                name: "Face".into(),
                parent: None,
            })
            .unwrap();
        model
            .apply(Command::AddMesh {
                name: "Skin".into(),
                part: "Face".into(),
                positions: g.positions.clone(),
                uvs: g.uvs.clone(),
                indices: g.indices.clone(),
            })
            .unwrap();
        let before = geometry_of(&model, "Skin").unwrap();
        let mut t = tool();
        click(&mut t, &before, &m, [100.0, 100.0], false);
        t.press(
            &before.positions,
            &before.indices,
            &m,
            [100.0, 100.0],
            false,
        );
        t.drag([50.0, 50.0]);
        assert_eq!(geometry_of(&model, "Skin").unwrap(), before);
        let (command, _) = t.release("Skin", &before.positions, &m).unwrap();
        model.apply(command).unwrap();
        assert_ne!(geometry_of(&model, "Skin").unwrap(), before);
        // One undo returns it.
        assert!(model.undo());
        assert_eq!(geometry_of(&model, "Skin").unwrap(), before);
    }

    #[test]
    fn mod_click_adds_a_vertex_or_splits_an_edge_and_selects_it() {
        let (g, m) = square();
        let mut t = tool();
        let (command, label) = t.add_at("Skin", &g, &m, [80.0, 30.0]).unwrap();
        assert_eq!(label, "mesh_add_vertex: Skin");
        let Command::SetMeshGeometry {
            positions, indices, ..
        } = command
        else {
            panic!("an add sends SetMeshGeometry");
        };
        assert_eq!((positions.len(), indices.len()), (5, 12));
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [4]);
        // On the diagonal.
        let (command, label) = t.add_at("Skin", &g, &m, [50.0, 50.0]).unwrap();
        assert_eq!(label, "mesh_split_edge: Skin");
        let Command::SetMeshGeometry { indices, .. } = command else {
            panic!();
        };
        assert_eq!(indices.len(), 12);
        // Outside and on a vertex are refused.
        assert_eq!(
            t.add_at("Skin", &g, &m, [150.0, 30.0]),
            Err(ToolError::Edit(EditError::OutsideMesh))
        );
        assert_eq!(
            t.add_at("Skin", &g, &m, [1.0, 99.0]),
            Err(ToolError::Edit(EditError::TooClose))
        );
    }

    #[test]
    fn delete_removes_the_selection_in_one_command_and_refuses_an_empty_one() {
        let g = {
            // A 3 x 3 grid so an inner vertex can go.
            let mut g = Geometry::default();
            for y in 0..3 {
                for x in 0..3 {
                    g.positions.push([x as f32, y as f32]);
                    g.uvs.push([x as f32 / 2.0, y as f32 / 2.0]);
                }
            }
            for y in 0..2u32 {
                for x in 0..2u32 {
                    let a = y * 3 + x;
                    g.indices.extend([a, a + 1, a + 4, a, a + 4, a + 3]);
                }
            }
            g
        };
        let m = Mapper {
            origin: [0.0, 0.0],
            size: [200.0, 200.0],
            view: [0.0, 0.0, 2.0, 2.0],
        };
        let mut t = tool();
        assert_eq!(t.delete_selected("Skin", &g), Err(ToolError::NeedVertices));
        click(&mut t, &g, &m, [100.0, 100.0], false);
        let (command, label) = t.delete_selected("Skin", &g).unwrap();
        assert_eq!(label, "mesh_remove_vertices: Skin");
        let Command::SetMeshGeometry {
            positions,
            uvs,
            indices,
            ..
        } = command
        else {
            panic!();
        };
        assert_eq!((positions.len(), uvs.len(), indices.len()), (8, 8, 18));
        assert!(t.vertices().is_empty());
    }

    #[test]
    fn connect_needs_two_vertices_and_flip_needs_an_edge() {
        let (g, m) = square();
        let mut t = tool();
        assert_eq!(
            t.connect_selected("Skin", &g),
            Err(ToolError::NeedTwoVertices)
        );
        assert_eq!(t.flip_selected("Skin", &g), Err(ToolError::NeedEdge));
        click(&mut t, &g, &m, [100.0, 100.0], false);
        click(&mut t, &g, &m, [0.0, 0.0], true);
        // 1 and 3 are the other diagonal; drawing it flips the shared edge.
        let (command, label) = t.connect_selected("Skin", &g).unwrap();
        assert_eq!(label, "mesh_connect: Skin");
        assert!(matches!(command, Command::SetMeshGeometry { .. }));
        assert_eq!(t.edge(), Some((1, 3)));
        // Flip the existing diagonal.
        click(&mut t, &g, &m, [50.0, 50.0], false);
        let (_, label) = t.flip_selected("Skin", &g).unwrap();
        assert_eq!(label, "mesh_flip_edge: Skin");
        assert_eq!(t.edge(), None);
        // An outline edge cannot flip.
        click(&mut t, &g, &m, [50.0, 100.0], false);
        assert_eq!(
            t.flip_selected("Skin", &g),
            Err(ToolError::Edit(EditError::BoundaryEdge))
        );
    }

    #[test]
    fn retriangulate_keeps_the_selected_edge_and_refuses_a_no_op() {
        let g = Geometry {
            positions: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2, 0, 2, 3],
        };
        let mut t = tool();
        // Square corner pairs are cocircular, so nothing flips.
        let square = Geometry {
            positions: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            ..g.clone()
        };
        assert_eq!(t.retriangulate("Skin", &square), Err(ToolError::NoChange));
        // A skewed quad where the drawn diagonal is the wrong one.
        let skew = Geometry {
            positions: vec![[0.0, 0.0], [2.0, 0.0], [3.0, 2.0], [-0.5, 2.0]],
            ..g.clone()
        };
        let (command, label) = t.retriangulate("Skin", &skew).unwrap();
        assert_eq!(label, "mesh_retriangulate: Skin");
        assert!(matches!(command, Command::SetMeshGeometry { .. }));
        // With that diagonal selected it is kept, so nothing changes.
        t.edge = Some((0, 2));
        assert_eq!(t.retriangulate("Skin", &skew), Err(ToolError::NoChange));
    }

    #[test]
    fn changing_the_mesh_drops_the_selection_and_sanitize_drops_stale_ids() {
        let (g, m) = square();
        let mut t = tool();
        click(&mut t, &g, &m, [100.0, 100.0], false);
        t.set_mesh(Some("Skin"));
        assert_eq!(t.vertices().len(), 1);
        t.sanitize(1);
        assert!(t.vertices().is_empty());
        click(&mut t, &g, &m, [100.0, 100.0], false);
        t.set_mesh(Some("Other"));
        assert!(t.vertices().is_empty());
    }

    #[test]
    fn picking_a_mesh_takes_the_front_one_and_skips_hidden() {
        let mut model = Model::new();
        model
            .apply(Command::AddPart {
                name: "P".into(),
                parent: None,
            })
            .unwrap();
        let tri = vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        for name in ["Back", "Front", "Hidden"] {
            model
                .apply(Command::AddMesh {
                    name: name.into(),
                    part: "P".into(),
                    positions: tri.clone(),
                    uvs: tri.clone(),
                    indices: vec![0, 1, 2],
                })
                .unwrap();
        }
        for (name, order) in [("Back", 400.0), ("Front", 600.0), ("Hidden", 900.0)] {
            model
                .apply(Command::SetMeshDrawOrder {
                    name: name.into(),
                    draw_order: order,
                })
                .unwrap();
        }
        model
            .apply(Command::SetMeshVisible {
                name: "Hidden".into(),
                visible: false,
            })
            .unwrap();
        assert_eq!(pick_mesh(&model, [0.5, 0.5]).as_deref(), Some("Front"));
        assert_eq!(pick_mesh(&model, [5.0, 5.0]), None);
    }

    #[test]
    fn every_error_has_a_message_in_every_language() {
        use tsukumoe_i18n::{I18n, Language};
        let errors = [
            ToolError::NeedVertices,
            ToolError::NeedTwoVertices,
            ToolError::NeedEdge,
            ToolError::NoChange,
            EditError::OutsideMesh.into(),
            EditError::TooClose.into(),
            EditError::BoundaryEdge.into(),
            EditError::NotConvex.into(),
            EditError::CannotRemove.into(),
            EditError::CannotConnect.into(),
            EditError::NoSuchEdge.into(),
        ];
        for lang in Language::ALL {
            let t = I18n::new(lang);
            for e in errors {
                assert_ne!(t.tr(e.key()), e.key(), "{lang:?} {e:?}");
            }
        }
    }
}
