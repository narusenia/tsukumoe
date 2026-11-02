// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The mesh edit tool as plain state and functions: what is selected, what a
//! drag is doing, and the command each operation sends. It never touches the
//! model (ADR-0019): every operation returns a `Command` for the dispatcher,
//! and a drag only changes the preview until it is released. The viewport
//! feeds it pointer positions; `panels::mesh_overlay` draws it.

use std::collections::BTreeSet;

use tsukumoe_core::command::Command;
use tsukumoe_core::contour::{self, Alpha, ContourError, ContourParams};
use tsukumoe_core::deformer_space::Space;
use tsukumoe_core::elements::Point;
use tsukumoe_core::mesh_edit::{self, EditError, Geometry};
use tsukumoe_core::model::Model;
use tsukumoe_core::proportional;
use tsukumoe_core::rig::ArtMesh;

use crate::proportional::{Proportional, Snapshot};

/// A press that moves less than this many points is a click.
const DRAG_THRESHOLD: f32 = 3.0;
/// A lasso takes a new point once the pointer is this far (screen points) from the last.
const LASSO_STEP: f32 = 2.0;

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
    /// A weld needs two vertices or more.
    NeedSeveral,
    NeedEdge,
    /// Re-triangulating would change nothing.
    NoChange,
    /// The texture of the mesh could not be read.
    NoTexture,
    Contour(ContourError),
    /// A deformer edit was refused; the translation key says why.
    Deformer(&'static str),
    Edit(EditError),
}

impl ToolError {
    /// The translation key of the message.
    pub fn key(self) -> &'static str {
        match self {
            Self::NeedVertices => "mesh-edit-need-vertices",
            Self::NeedTwoVertices => "mesh-edit-need-two",
            Self::NeedSeveral => "mesh-edit-need-several",
            Self::NeedEdge => "mesh-edit-need-edge",
            Self::NoChange => "mesh-edit-no-change",
            Self::NoTexture => "mesh-edit-no-texture",
            Self::Deformer(key) => key,
            Self::Contour(ContourError::BadImage) => "mesh-edit-no-texture",
            Self::Contour(ContourError::BadParams) => "mesh-edit-contour-params",
            Self::Contour(ContourError::Empty) => "mesh-edit-contour-empty",
            Self::Contour(ContourError::NoUvMapping) => "mesh-edit-contour-no-uv",
            Self::Contour(ContourError::TooManyPoints) => "mesh-edit-contour-too-many",
            Self::Contour(ContourError::CannotTriangulate) => "mesh-edit-contour-cannot",
            Self::Edit(EditError::OutsideMesh) => "mesh-edit-outside",
            Self::Edit(EditError::TooClose) => "mesh-edit-too-close",
            Self::Edit(EditError::BoundaryEdge) => "mesh-edit-outline-edge",
            Self::Edit(EditError::NotConvex) => "mesh-edit-not-convex",
            Self::Edit(EditError::CannotRemove) => "mesh-edit-cannot-remove",
            Self::Edit(EditError::CannotConnect) => "mesh-edit-cannot-connect",
            Self::Edit(EditError::BadGrid) => "mesh-edit-grid-size",
            Self::Edit(EditError::NothingToAdd) => "mesh-edit-grid-empty",
            Self::Edit(EditError::InsideMesh) => "mesh-edit-inside",
            Self::Edit(EditError::CannotExtend) => "mesh-edit-cannot-extend",
            Self::Edit(EditError::OutsideTexture) => "mesh-edit-outside-texture",
            Self::Edit(EditError::NothingLeft) => "mesh-edit-nothing-left",
            Self::Edit(EditError::BadVertex | EditError::NoSuchEdge) => "mesh-edit-no-edge",
        }
    }
}

impl From<ContourError> for ToolError {
    fn from(e: ContourError) -> Self {
        Self::Contour(e)
    }
}

/// The alpha of a mesh's texture, owned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AlphaImage {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Columns and rows for a mesh from the opaque area of its texture: one cell
/// per ~64 px along the longer side (2 to 16), the shorter side by aspect.
/// An empty texture gives 2 x 2.
pub fn auto_grid(alpha: &AlphaImage, threshold: u8) -> (u32, u32) {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, &a) in alpha.data.iter().enumerate() {
        if a >= threshold {
            let (x, y) = (i as u32 % alpha.width, i as u32 / alpha.width);
            (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
        }
    }
    if x0 > x1 {
        return (2, 2);
    }
    let (w, h) = ((x1 - x0 + 1) as f32, (y1 - y0 + 1) as f32);
    let long = ((w.max(h) / 64.0).round() as u32).clamp(2, 16);
    let short = ((long as f32 * w.min(h) / w.max(h)).round() as u32).clamp(2, 16);
    if w >= h { (long, short) } else { (short, long) }
}

impl From<EditError> for ToolError {
    fn from(e: EditError) -> Self {
        Self::Edit(e)
    }
}

/// The actions of the right-click menu, in order. Each one is run through
/// `Editor::run_action`, so the menu has no logic of its own.
pub const MENU_ACTIONS: [&str; 16] = [
    "mesh.retriangulate",
    "mesh.fill-grid",
    "mesh.contour",
    "mesh.even-outline",
    "mesh.connect",
    "mesh.flip-edge",
    "mesh.edge-pen",
    "mesh.delete-edge",
    "mesh.weld",
    "mesh.weld-near",
    "mesh.glue-source",
    "mesh.glue",
    "mesh.delete-vertices",
    "deformer.create-warp",
    "deformer.create-rotation",
    "mesh.reset",
];

/// A command and the name it gets in the history list.
pub type Op = (Command, String);

#[derive(Clone, Debug, PartialEq)]
enum Gesture {
    /// Pressed, not yet moved far enough to be a drag.
    Pending {
        at: [f32; 2],
        hit: Hit,
        shift: bool,
        lasso: bool,
    },
    /// Dragging the selected vertices; `from` and `to` are screen points.
    Move { from: [f32; 2], to: [f32; 2] },
    /// Dragging a box.
    Rect {
        from: [f32; 2],
        to: [f32; 2],
        add: bool,
    },
    /// Dragging a free curve; `points` are screen points.
    Lasso { points: Vec<[f32; 2]>, add: bool },
}

#[derive(Clone, Debug, PartialEq)]
pub struct MeshTool {
    mesh: Option<String>,
    vertices: BTreeSet<u32>,
    edge: Option<(u32, u32)>,
    /// The edge pen is on: clicks draw edges instead of selecting.
    pub pen: bool,
    /// The vertex the next pen edge starts at.
    pen_from: Option<u32>,
    gesture: Option<Gesture>,
    /// Columns and rows of the grid `mesh.fill-grid` lays; kept across meshes.
    pub grid: (u32, u32),
    /// Take the grid from the texture's opaque area when filling (`auto_grid`).
    pub grid_auto: bool,
    /// Settings of `mesh.contour`; kept across meshes.
    pub contour: ContourParams,
    /// Proportional edit; the viewport copies the editor's options here.
    pub proportional: Proportional,
    /// The drawn points at the press, kept while proportional edit is on.
    snapshot: Option<Snapshot>,
    /// The next press on empty space drags a lasso instead of a box; the
    /// viewport sets it from the Mod key.
    pub lasso: bool,
    /// Moving a vertex moves its mirror image about `center` too; kept across meshes.
    pub symmetry: bool,
    /// The x of the mirror line in the drawing space; the viewport sets it.
    pub center: f32,
}

/// Whether `p` is inside the polygon `poly` (even-odd rule).
pub fn in_polygon(poly: &[[f32; 2]], p: [f32; 2]) -> bool {
    let mut inside = false;
    let mut j = poly.len().wrapping_sub(1);
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[j]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
        j = i;
    }
    inside
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
        .filter(|&(_, d)| d <= crate::options::tuning().vertex_radius)
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
        .filter(|&(_, d)| d <= crate::options::tuning().edge_radius)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    edge.map_or(Hit::Nothing, |((a, b), _)| Hit::Edge(a, b))
}

/// The visible mesh drawn in front at `at` (a model point), by name. A mesh under
/// a hidden part is not visible. Meshes are taken at their base shape, as the
/// tool shows them.
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
        .filter(|(_, _, m)| shown(model, m) && inside(&m.positions, &m.indices))
        .max_by(|a, b| a.2.draw_order.total_cmp(&b.2.draw_order))
        .map(|(_, name, _)| name.to_owned())
}

/// Whether the mesh and every part above it are visible (as when drawing).
fn shown(model: &Model, mesh: &ArtMesh) -> bool {
    let mut part = Some(mesh.part);
    let mut shown = mesh.visible;
    while let Some(id) = part {
        let p = model.parts().get(id).expect("a mesh's part exists");
        shown &= p.visible;
        part = p.parent;
    }
    shown
}

/// The visible mesh drawn in front at `at` (a point of the drawing space), by
/// name, from the evaluated draw list: where the pointer sees it, deformers
/// and parameters included.
pub fn pick_drawn(list: &tsukumoe_core::draw::DrawList, at: Point) -> Option<String> {
    let side =
        |a: [f32; 2], b: [f32; 2]| (b[0] - a[0]) * (at[1] - a[1]) - (b[1] - a[1]) * (at[0] - a[0]);
    let order = list.order();
    order.iter().rev().find_map(|&mesh| {
        let d = list.drawable(mesh as usize);
        if d.opacity <= 0.0 {
            return None;
        }
        let p = d.positions.as_chunks::<2>().0;
        let hit = d.indices.as_chunks::<3>().0.iter().any(|t| {
            let [a, b, c] = t.map(|i| p.get(i as usize).copied());
            let (Some(a), Some(b), Some(c)) = (a, b, c) else {
                return false;
            };
            let s = [side(a, b), side(b, c), side(c, a)];
            s.iter().all(|&x| x >= 0.0) || s.iter().all(|&x| x <= 0.0)
        });
        hit.then(|| d.name.to_owned())
    })
}

/// The meshes drawn with their centre inside the lasso `path` (screen points),
/// back to front.
pub fn pick_in_lasso(
    list: &tsukumoe_core::draw::DrawList,
    m: &Mapper,
    path: &[[f32; 2]],
) -> Vec<String> {
    list.order()
        .iter()
        .filter_map(|&mesh| {
            let d = list.drawable(mesh as usize);
            let p = d.positions.as_chunks::<2>().0;
            if d.opacity <= 0.0 || p.is_empty() {
                return None;
            }
            let n = p.len() as f32;
            let sum = p.iter().fold([0.0; 2], |s, q| [s[0] + q[0], s[1] + q[1]]);
            in_polygon(path, m.to_screen([sum[0] / n, sum[1] / n])).then(|| d.name.to_owned())
        })
        .collect()
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

/// `g` with a vertex added where the pointer is: on an edge it splits the
/// edge, in a triangle it splits the triangle, outside the outline it grows
/// the outline. The new vertex is the last one.
fn added_in(
    g: &Geometry,
    shown: &[Point],
    m: &Mapper,
    at: [f32; 2],
    space: &Space,
) -> Result<(Geometry, &'static str), ToolError> {
    Ok(match hit_test(shown, &g.indices, m, at) {
        Hit::Vertex(_) => return Err(EditError::TooClose.into()),
        Hit::Edge(a, b) => {
            let (pa, pb) = (
                m.to_screen(shown[a as usize]),
                m.to_screen(shown[b as usize]),
            );
            let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
            let t = ((at[0] - pa[0]) * dx + (at[1] - pa[1]) * dy) / (dx * dx + dy * dy).max(1e-9);
            (
                mesh_edit::split_edge(g, a, b, t.clamp(0.0, 1.0))?,
                "mesh_split_edge",
            )
        }
        Hit::Nothing => {
            let local = space
                .point_from_world(m.to_model(at))
                .map_err(|_| EditError::OutsideMesh)?;
            match mesh_edit::add_vertex(g, local) {
                Err(EditError::OutsideMesh) => {
                    (mesh_edit::extend_outline(g, local)?, "mesh_extend_outline")
                }
                added => (added?, "mesh_add_vertex"),
            }
        }
    })
}

impl Default for MeshTool {
    fn default() -> Self {
        Self {
            mesh: None,
            vertices: BTreeSet::new(),
            edge: None,
            pen: false,
            pen_from: None,
            gesture: None,
            grid: (4, 4),
            grid_auto: false,
            contour: ContourParams::default(),
            proportional: Proportional::default(),
            snapshot: None,
            lasso: false,
            symmetry: false,
            center: 0.0,
        }
    }
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
                pen: self.pen,
                grid: self.grid,
                grid_auto: self.grid_auto,
                contour: self.contour,
                proportional: self.proportional,
                symmetry: self.symmetry,
                center: self.center,
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

    /// True while selected vertices are being dragged (not a box).
    pub fn is_moving(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Move { .. }))
    }

    pub fn is_dragging(&self) -> bool {
        matches!(
            self.gesture,
            Some(Gesture::Move { .. } | Gesture::Rect { .. } | Gesture::Lasso { .. })
        )
    }

    /// The box being dragged, as screen corners.
    pub fn rect(&self) -> Option<([f32; 2], [f32; 2])> {
        match self.gesture {
            Some(Gesture::Rect { from, to, .. }) => Some((from, to)),
            _ => None,
        }
    }

    /// The lasso being dragged, as screen points.
    pub fn lasso_path(&self) -> Option<&[[f32; 2]]> {
        match &self.gesture {
            Some(Gesture::Lasso { points, .. }) => Some(points),
            _ => None,
        }
    }

    /// Whether the menu item `action` can be used with the current selection:
    /// drawing an edge needs two vertices, flipping an edge, deleting some
    /// vertices; the whole-mesh actions always work.
    pub fn menu_enabled(&self, action: &str) -> bool {
        match action {
            "mesh.connect" => self.vertices.len() == 2,
            "mesh.weld" => self.vertices.len() >= 2,
            "mesh.glue-source" | "mesh.glue" => !self.vertices.is_empty(),
            "mesh.flip-edge" => self.edge.is_some(),
            "mesh.delete-vertices" => !self.vertices.is_empty() || self.edge.is_some(),
            "mesh.delete-edge" => self.edge.is_some(),
            _ => true,
        }
    }

    /// A right click at `at`: a vertex or an edge under it that is not in the
    /// selection yet becomes the selection, so the menu acts on it. Empty
    /// space leaves the selection as it is.
    pub fn context_select(
        &mut self,
        positions: &[Point],
        indices: &[u32],
        m: &Mapper,
        at: [f32; 2],
    ) {
        match hit_test(positions, indices, m, at) {
            Hit::Vertex(v) if !self.vertices.contains(&v) => self.select(Hit::Vertex(v), false),
            Hit::Edge(a, b) if self.edge != Some((a, b)) => self.select(Hit::Edge(a, b), false),
            _ => {}
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
        if self.pen_from.is_some_and(|v| v as usize >= vertex_count) {
            self.pen_from = None;
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

    /// Selects the vertices inside the screen curve (closed by a straight line).
    fn select_in_lasso(&mut self, positions: &[Point], m: &Mapper, path: &[[f32; 2]], add: bool) {
        if !add {
            self.clear_selection();
        }
        for (i, &p) in positions.iter().enumerate() {
            if in_polygon(path, m.to_screen(p)) {
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
        self.snapshot = self.proportional.on.then(|| Snapshot {
            positions: positions.to_vec(),
            edges: edges(indices),
        });
        self.gesture = Some(Gesture::Pending {
            at,
            hit,
            shift,
            lasso: self.lasso,
        });
    }

    /// The pointer moved with the button down.
    pub fn drag(&mut self, to: [f32; 2]) {
        self.gesture = match self.gesture.take() {
            Some(Gesture::Pending {
                at,
                hit,
                shift,
                lasso,
            }) => {
                if (to[0] - at[0]).hypot(to[1] - at[1]) < DRAG_THRESHOLD {
                    Some(Gesture::Pending {
                        at,
                        hit,
                        shift,
                        lasso,
                    })
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
                } else if lasso {
                    Some(Gesture::Lasso {
                        points: vec![at, to],
                        add: shift,
                    })
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
            Some(Gesture::Lasso { mut points, add }) => {
                if points
                    .last()
                    .is_none_or(|l| (to[0] - l[0]).hypot(to[1] - l[1]) >= LASSO_STEP)
                {
                    points.push(to);
                }
                Some(Gesture::Lasso { points, add })
            }
            None => None,
        };
    }

    /// Abandons a drag (Escape) without changing anything.
    pub fn cancel(&mut self) {
        self.gesture = None;
        self.snapshot = None;
        self.pen_from = None;
    }

    /// How far each vertex follows the drag: 1 for the selected ones and
    /// less around them with proportional edit on; `None` with it off (only
    /// the selected ones move). Measured on the points drawn at the press.
    fn weights(&self, m: &Mapper) -> Option<Vec<f32>> {
        let snap = self.snapshot.as_ref().filter(|_| self.proportional.on)?;
        let base: Vec<u32> = self.vertices.iter().copied().collect();
        Some(proportional::weights(
            &snap.positions,
            &snap.edges,
            &base,
            &self.proportional.influence(m),
        ))
    }

    /// The weights of a move in progress, for the overlay's shading.
    pub fn drag_weights(&self, m: &Mapper) -> Option<Vec<f32>> {
        matches!(self.gesture, Some(Gesture::Move { .. }))
            .then(|| self.weights(m))
            .flatten()
    }

    /// The vertices that move and by how much of the drag: the selected ones
    /// whole, or all that have a weight.
    fn follow(&self, weights: Option<&[f32]>, len: usize) -> Vec<(usize, f32)> {
        match weights.filter(|w| w.len() == len) {
            Some(w) => (0..len)
                .filter(|&i| w[i] > 0.0)
                .map(|i| (i, w[i]))
                .collect(),
            None => self.vertices.iter().map(|&v| (v as usize, 1.0)).collect(),
        }
    }

    /// `positions` with the selected vertices moved by the drag from `from` to `to`.
    fn shifted(&self, positions: &[Point], m: &Mapper, from: [f32; 2], to: [f32; 2]) -> Vec<Point> {
        let (a, b) = (m.to_model(from), m.to_model(to));
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut out = positions.to_vec();
        let weights = self.weights(m);
        let moved = self.follow(weights.as_deref(), out.len());
        for &(i, k) in &moved {
            if let Some(p) = out.get_mut(i) {
                *p = [p[0] + d[0] * k, p[1] + d[1] * k];
            }
        }
        if self.symmetry {
            self.mirror_moved(&mut out, positions, &moved, &|p| p, &|p| Some(p));
        }
        out
    }

    /// Gives the mirror image of each moved vertex the mirrored place of the
    /// moved one. `base` are the vertices before the move, `to_world` and
    /// `from_world` turn them into the drawing space where the mirror line is,
    /// and back. A vertex with no mirror image, or on the line, is left alone.
    fn mirror_moved(
        &self,
        out: &mut [Point],
        base: &[Point],
        moved: &[(usize, f32)],
        to_world: &dyn Fn(Point) -> Point,
        from_world: &dyn Fn(Point) -> Option<Point>,
    ) -> Option<()> {
        let world: Vec<Point> = base.iter().map(|&p| to_world(p)).collect();
        let (lo, hi) = world
            .iter()
            .fold((f32::MAX, f32::MIN), |(l, h), p| (l.min(p[0]), h.max(p[0])));
        let tolerance = ((hi - lo) * crate::options::tuning().mirror_tolerance).max(1e-4);
        let c = self.center;
        let mut done = BTreeSet::new();
        for &(i, _) in moved {
            if done.contains(&i) {
                continue;
            }
            let at = [2.0 * c - world[i][0], world[i][1]];
            let partner = (0..world.len()).filter(|&j| j != i).min_by(|&a, &b| {
                let d = |j: usize| (world[j][0] - at[0]).hypot(world[j][1] - at[1]);
                d(a).total_cmp(&d(b))
            });
            let Some(j) = partner.filter(|&j| {
                (world[j][0] - at[0]).hypot(world[j][1] - at[1]) <= tolerance && !done.contains(&j)
            }) else {
                continue;
            };
            let w = to_world(out[i]);
            out[j] = from_world([2.0 * c - w[0], w[1]])?;
            done.extend([i, j]);
        }
        Some(())
    }

    /// `stored` with the selected vertices moved so that they are drawn the
    /// drag's distance from where `space` draws them; `None` if the space is flat there.
    fn shifted_through(
        &self,
        stored: &[Point],
        space: &Space,
        m: &Mapper,
        from: [f32; 2],
        to: [f32; 2],
    ) -> Option<Vec<Point>> {
        let (a, b) = (m.to_model(from), m.to_model(to));
        let d = [b[0] - a[0], b[1] - a[1]];
        let mut out = stored.to_vec();
        let weights = self.weights(m);
        let moved = self.follow(weights.as_deref(), out.len());
        for &(i, k) in &moved {
            if let Some(p) = out.get_mut(i) {
                let w = space.point_to_world(*p);
                *p = space
                    .point_from_world([w[0] + d[0] * k, w[1] + d[1] * k])
                    .ok()?;
            }
        }
        if self.symmetry {
            self.mirror_moved(
                &mut out,
                stored,
                &moved,
                &|p| space.point_to_world(p),
                &|p| space.point_from_world(p).ok(),
            )?;
        }
        Some(out)
    }

    /// The stored positions after the drag from `from` to `to`.
    fn moved_stored(
        &self,
        stored: &[Point],
        m: &Mapper,
        from: [f32; 2],
        to: [f32; 2],
        space: &Space,
    ) -> Option<Vec<Point>> {
        if matches!(space, Space::Drawing) {
            Some(self.shifted(stored, m, from, to))
        } else {
            self.shifted_through(stored, space, m, from, to)
        }
    }

    /// The stored positions a release now would send, while a move is on:
    /// what the viewport shows live.
    pub fn dragging_stored(
        &self,
        stored: &[Point],
        m: &Mapper,
        space: &Space,
    ) -> Option<Vec<Point>> {
        let Some(Gesture::Move { from, to }) = self.gesture else {
            return None;
        };
        if from == to {
            return None;
        }
        self.moved_stored(stored, m, from, to, space)
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
        self.release_in(name, positions, positions, m, &Space::Drawing)
    }

    /// [`Self::release`] for a mesh drawn through its parent deformer: `shown`
    /// are the vertices as drawn (what the pointer touches), `stored` the
    /// ones in the model, and `space` the parent's child space. A move turns
    /// into the stored positions whose drawn place is the dragged one.
    pub fn release_in(
        &mut self,
        name: &str,
        stored: &[Point],
        shown: &[Point],
        m: &Mapper,
        space: &Space,
    ) -> Option<Op> {
        match self.gesture.take()? {
            Gesture::Pending { hit, shift, .. } => {
                self.snapshot = None;
                self.select(hit, shift);
                None
            }
            Gesture::Rect { from, to, add } => {
                self.snapshot = None;
                self.select_in(shown, m, from, to, add);
                None
            }
            Gesture::Lasso { points, add } => {
                self.snapshot = None;
                self.select_in_lasso(shown, m, &points, add);
                None
            }
            Gesture::Move { from, to } => {
                let moved = (from != to)
                    .then(|| self.moved_stored(stored, m, from, to, space))
                    .flatten();
                self.snapshot = None;
                let moved = moved?;
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
        self.add_at_in(name, g, &g.positions, m, at, &Space::Drawing)
    }

    /// [`Self::add_at`] for a mesh drawn through its parent deformer: the
    /// pointer meets `shown`, and the new vertex is stored where `space` draws it
    /// under the pointer.
    pub fn add_at_in(
        &mut self,
        name: &str,
        g: &Geometry,
        shown: &[Point],
        m: &Mapper,
        at: [f32; 2],
        space: &Space,
    ) -> Result<Op, ToolError> {
        let (out, label) = added_in(g, shown, m, at, space)?;
        self.vertices = BTreeSet::from([g.positions.len() as u32]);
        self.edge = None;
        Ok(geometry_op(name, label, out))
    }

    /// Removes the selected vertices as one command.
    /// With only an edge selected, it removes that edge instead.
    pub fn delete_selected(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        if self.vertices.is_empty() && self.edge.is_some() {
            return self.delete_edge(name, g);
        }
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

    /// Lays the tool's grid of vertices inside the mesh and re-triangulates it.
    pub fn fill_grid(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let out = mesh_edit::fill_grid(g, self.grid.0, self.grid.1)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_fill_grid", out))
    }

    /// Puts the mesh back to one quad over its UV box (all editing undone in shape).
    pub fn reset(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let out = contour::reset_to_quad(g)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_reset", out))
    }

    /// Spaces the points of the mesh's outline evenly (`contour::even_outline`).
    pub fn even_outline(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let out = contour::even_outline(g)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_even_outline", out))
    }

    /// Makes the mesh again from the alpha of its texture: an outline per
    /// island and lattice points inside (`contour::contour_mesh`). `alpha` is
    /// `None` when the texture could not be read.
    pub fn contour_mesh(
        &mut self,
        name: &str,
        g: &Geometry,
        alpha: Option<&AlphaImage>,
    ) -> Result<Op, ToolError> {
        let alpha = alpha.ok_or(ToolError::NoTexture)?;
        let image = Alpha {
            width: alpha.width,
            height: alpha.height,
            data: &alpha.data,
        };
        let out = contour::contour_mesh(g, &image, &self.contour)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_contour", out))
    }

    /// Removes the selected edge and the triangles on its sides (a hole, or a
    /// smaller outline).
    pub fn delete_edge(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let (a, b) = self.edge.ok_or(ToolError::NeedEdge)?;
        let out = mesh_edit::remove_edge(g, a, b)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_remove_edge", out))
    }

    /// The vertex the next pen edge starts at.
    pub fn pen_from(&self) -> Option<u32> {
        self.pen_from
    }

    /// Ends the pen chain; the next click starts a new one.
    pub fn pen_end(&mut self) {
        self.pen_from = None;
    }

    /// A drag with the pen starts at the vertex under the press.
    pub fn pen_press(&mut self, shown: &[Point], indices: &[u32], m: &Mapper, at: [f32; 2]) {
        if let Hit::Vertex(v) = hit_test(shown, indices, m, at) {
            self.pen_from = Some(v);
        }
    }

    /// A pen click (or the end of a pen drag) at `at`. On a vertex it draws
    /// an edge from the chain's start and goes on from there; elsewhere it
    /// adds a vertex (see [`added_in`]) and joins it to the start. Each edge
    /// is one command; a refused one changes nothing.
    pub fn pen_click(
        &mut self,
        name: &str,
        g: &Geometry,
        shown: &[Point],
        m: &Mapper,
        at: [f32; 2],
        space: &Space,
    ) -> Result<Option<Op>, ToolError> {
        let hit = hit_test(shown, &g.indices, m, at);
        let (out, label, to) = if let Hit::Vertex(b) = hit {
            let Some(a) = self.pen_from else {
                self.pen_from = Some(b);
                return Ok(None);
            };
            if a == b {
                return Ok(None);
            }
            (mesh_edit::connect(g, a, b)?, "mesh_connect", b)
        } else {
            let (added, label) = added_in(g, shown, m, at, space)?;
            let to = g.positions.len() as u32;
            match self.pen_from {
                Some(a) => (mesh_edit::connect(&added, a, to)?, "mesh_connect", to),
                None => (added, label, to),
            }
        };
        self.pen_from = Some(to);
        self.vertices.clear();
        self.edge = None;
        Ok(Some(geometry_op(name, label, out)))
    }

    /// Merges the selected vertices (two or more) into one at their centre.
    pub fn weld_selected(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        if self.vertices.len() < 2 {
            return Err(ToolError::NeedSeveral);
        }
        let ids: Vec<u32> = self.vertices.iter().copied().collect();
        let out = mesh_edit::weld(g, &[ids])?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_weld", out))
    }

    /// Merges every set of vertices that lie almost on top of each other.
    pub fn weld_near(&mut self, name: &str, g: &Geometry) -> Result<Op, ToolError> {
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &g.positions {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let tolerance = (hi[0] - lo[0]).hypot(hi[1] - lo[1]) * crate::options::tuning().weld_ratio;
        let out = mesh_edit::weld_near(g, tolerance)?.ok_or(ToolError::NoChange)?;
        self.clear_selection();
        Ok(geometry_op(name, "mesh_weld_near", out))
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

    /// A unit square plus a vertex above it at (0.5, 1.5) that is not in a
    /// triangle yet, and UVs in the middle of the texture.
    fn pen_square() -> (Geometry, Mapper) {
        let (mut g, _) = square();
        for uv in &mut g.uvs {
            *uv = [0.25 + 0.5 * uv[0], 0.25 + 0.5 * uv[1]];
        }
        // Show 2 x 2 model units over 200 points, with the square at the bottom left.
        let m = Mapper {
            origin: [0.0, 0.0],
            size: [200.0, 200.0],
            view: [-0.5, -0.5, 1.5, 1.5],
        };
        (g, m)
    }

    #[test]
    fn the_pen_draws_edges_from_vertex_to_vertex_and_adds_points_where_it_is_empty() {
        let (g, m) = pen_square();
        let mut t = tool();
        t.pen = true;
        let at = |p: Point| m.to_screen(p);
        let space = Space::Drawing;
        let pen = |t: &mut MeshTool, g: &Geometry, p: Point| {
            t.pen_click("Skin", g, &g.positions, &m, at(p), &space)
        };
        // The first click on a vertex only sets the start.
        assert!(matches!(pen(&mut t, &g, [0.0, 0.0]), Ok(None)));
        assert_eq!(t.pen_from(), Some(0));
        // An empty click above the square grows the outline and joins the start.
        let Ok(Some((
            Command::SetMeshGeometry {
                positions, indices, ..
            },
            label,
        ))) = pen(&mut t, &g, [0.5, 1.3])
        else {
            panic!("a geometry");
        };
        assert_eq!(label, "mesh_connect: Skin");
        assert_eq!(positions.len(), 5);
        assert_eq!(indices.len(), 9);
        assert_eq!(t.pen_from(), Some(4));
        // Escape ends the chain.
        t.cancel();
        assert_eq!(t.pen_from(), None);
        // A refused edge says why and leaves the start where it was.
        assert!(matches!(pen(&mut t, &g, [0.0, 0.0]), Ok(None)));
        let far = pen(&mut t, &g, [-9.0, -9.0]);
        assert!(far.is_err());
        assert_eq!(t.pen_from(), Some(0));
    }

    #[test]
    fn the_pen_connects_two_vertices_and_continues_from_the_second() {
        let (g, m) = pen_square();
        let mut t = tool();
        t.pen = true;
        let space = Space::Drawing;
        let click = |t: &mut MeshTool, p: Point| {
            t.pen_click("Skin", &g, &g.positions, &m, m.to_screen(p), &space)
        };
        assert!(matches!(click(&mut t, [1.0, 0.0]), Ok(None)));
        // 1 -> 3 crosses the diagonal 0-2: flipped into place, one command.
        let Ok(Some((Command::SetMeshGeometry { indices, .. }, _))) = click(&mut t, [0.0, 1.0])
        else {
            panic!("a geometry");
        };
        assert_eq!(indices.len(), 6);
        assert_eq!(t.pen_from(), Some(3));
        assert!(t.vertices().is_empty());
    }

    #[test]
    fn deleting_an_edge_needs_one_and_a_lone_edge_selection_deletes_it_on_delete() {
        let (g, m) = square();
        let mut t = tool();
        assert_eq!(t.delete_edge("Skin", &g), Err(ToolError::NeedEdge));
        // The diagonal 0-2: both triangles go, so nothing is left.
        click(&mut t, &g, &m, [50.0, 50.0], false);
        assert_eq!(t.edge(), Some((0, 2)));
        assert_eq!(
            t.delete_selected("Skin", &g),
            Err(ToolError::Edit(EditError::NothingLeft))
        );
        // An outline edge of a bigger mesh: one triangle goes.
        let mut g = g;
        g.positions.push([2.0, 0.0]);
        g.uvs.push([2.0, 0.0]);
        g.indices.extend([1, 4, 2]);
        t.clear_selection();
        click(&mut t, &g, &m, [150.0, 100.0], false);
        let (command, label) = t.delete_selected("Skin", &g).unwrap();
        assert_eq!(label, "mesh_remove_edge: Skin");
        let Command::SetMeshGeometry { indices, .. } = command else {
            panic!("a geometry");
        };
        assert_eq!(indices.len(), 6);
        assert!(t.edge().is_none());
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
    fn symmetry_moves_the_mirror_image_of_a_dragged_vertex_the_other_way() {
        let (g, m) = square();
        let mut t = tool();
        t.symmetry = true;
        t.center = 0.5;
        click(&mut t, &g, &m, [100.0, 100.0], false);
        t.press(&g.positions, &g.indices, &m, [100.0, 100.0], false);
        t.drag([80.0, 80.0]);
        let preview = t.preview(&g.positions, &m).unwrap();
        assert!((preview[0][0] - 0.2).abs() < 1e-6 && (preview[0][1] - 0.2).abs() < 1e-6);
        let Some((Command::SetMeshPositions { positions, .. }, _)) =
            t.release("Skin", &g.positions, &m)
        else {
            panic!("a move");
        };
        assert!((positions[1][0] - 0.8).abs() < 1e-6);
        assert!((positions[0][0] - 0.2).abs() < 1e-6 && (positions[0][1] - 0.2).abs() < 1e-6);
        // The top corners are not touched.
        assert_eq!(positions[2], g.positions[2]);
        assert_eq!(positions[3], g.positions[3]);
        // Off, only the dragged vertex moves.
        t.symmetry = false;
        t.press(&g.positions, &g.indices, &m, [100.0, 100.0], false);
        t.drag([80.0, 80.0]);
        assert_eq!(t.preview(&g.positions, &m).unwrap()[0], g.positions[0]);
    }

    #[test]
    fn a_lasso_selects_the_vertices_inside_the_curve() {
        let (g, m) = square();
        let mut t = tool();
        t.lasso = true;
        // A triangle around the bottom left, left and bottom edges of the square
        // (screen y is down): it holds (0,0) at (0,100) and not (1,0) at (100,100).
        t.press(&g.positions, &g.indices, &m, [-20.0, 60.0], false);
        for to in [[-20.0, 130.0], [60.0, 130.0], [60.0, 60.0]] {
            t.drag(to);
        }
        assert!(t.is_dragging() && !t.is_moving() && t.rect().is_none());
        assert_eq!(t.lasso_path().map(<[_]>::len), Some(4));
        assert!(t.release("Skin", &g.positions, &m).is_none());
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [0]);
        // Shift adds: the same curve around the bottom right joins.
        t.press(&g.positions, &g.indices, &m, [60.0, 60.0], true);
        for to in [[60.0, 130.0], [130.0, 130.0], [130.0, 60.0]] {
            t.drag(to);
        }
        t.release("Skin", &g.positions, &m);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [0, 1]);
        assert!(in_polygon(
            &[[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
            [2.0, 2.0]
        ));
        assert!(!in_polygon(
            &[[0.0, 0.0], [10.0, 0.0], [0.0, 10.0]],
            [8.0, 8.0]
        ));
    }

    #[test]
    fn weld_joins_the_selected_vertices_and_near_welds_the_close_ones() {
        let (g, m) = square();
        let mut t = tool();
        assert_eq!(
            t.weld_selected("Skin", &g).unwrap_err(),
            ToolError::NeedSeveral
        );
        click(&mut t, &g, &m, [0.0, 100.0], false);
        click(&mut t, &g, &m, [100.0, 100.0], true);
        // Welding both bottom corners collapses the triangle 0 1 2 only.
        let (
            Command::SetMeshGeometry {
                positions, indices, ..
            },
            label,
        ) = t.weld_selected("Skin", &g).unwrap()
        else {
            panic!("a geometry");
        };
        assert_eq!(label, "mesh_weld: Skin");
        assert_eq!(positions.len(), 3);
        assert_eq!(indices.len(), 3);
        assert!(t.vertices().is_empty());
        assert_eq!(t.weld_near("Skin", &g).unwrap_err(), ToolError::NoChange);
    }

    /// A square of 4 corners with proportional edit on, a radius of 150 points
    /// (1.5 units), the corner at (1, 0) selected and dragged 0.5 up.
    fn proportional_drag(configure: impl FnOnce(&mut MeshTool)) -> (MeshTool, Geometry, Mapper) {
        let (g, m) = square();
        let mut t = tool();
        t.proportional.on = true;
        t.proportional.radius = 150.0;
        t.proportional.falloff = proportional::Falloff::Linear;
        configure(&mut t);
        click(&mut t, &g, &m, [100.0, 100.0], false);
        t.press(&g.positions, &g.indices, &m, [100.0, 100.0], false);
        t.drag([100.0, 50.0]);
        (t, g, m)
    }

    #[test]
    fn proportional_edit_moves_neighbours_by_their_weight_and_ends_in_one_command() {
        let (mut t, g, m) = proportional_drag(|_| {});
        let preview = t.preview(&g.positions, &m).unwrap();
        // The selected corner follows fully; a neighbour one unit away by 1 - 1/1.5.
        assert!((preview[1][1] - 0.5).abs() < 1e-6);
        let k = 1.0 - 1.0 / 1.5;
        assert!((preview[0][1] - 0.5 * k).abs() < 1e-5, "{:?}", preview[0]);
        assert!((preview[2][1] - (1.0 + 0.5 * k)).abs() < 1e-5);
        // The far corner is 2 along the edges: beyond the radius.
        assert_eq!(preview[3], g.positions[3]);
        let live = t
            .dragging_stored(&g.positions, &m, &Space::Drawing)
            .unwrap();
        assert_eq!(live, preview);
        let (command, label) = t.release("Skin", &g.positions, &m).unwrap();
        assert_eq!(label, "mesh_move_vertices: Skin");
        assert_eq!(
            command,
            Command::SetMeshPositions {
                name: "Skin".into(),
                positions: live
            }
        );
        // The drag is over: no weights, no snapshot.
        assert!(t.drag_weights(&m).is_none() && t.preview(&g.positions, &m).is_none());
    }

    #[test]
    fn proportional_edit_measures_on_the_press_not_on_the_moving_shape() {
        let (t, g, m) = proportional_drag(|_| {});
        let first = t.drag_weights(&m).unwrap();
        let mut t = t;
        t.drag([100.0, 20.0]);
        assert_eq!(t.drag_weights(&m).unwrap(), first);
        // Widening the radius mid drag widens the weights.
        t.proportional.radius = 300.0;
        let wide = t.drag_weights(&m).unwrap();
        assert!(wide[0] > first[0]);
        let _ = g;
    }

    #[test]
    fn a_zero_weight_outside_the_radius_and_off_means_selected_only() {
        let (t, g, m) = proportional_drag(|t| t.proportional.radius = 50.0);
        let preview = t.preview(&g.positions, &m).unwrap();
        assert_eq!(preview[0], g.positions[0]);
        assert_eq!(preview[3], g.positions[3]);
        let (t, g, m) = proportional_drag(|t| t.proportional.on = false);
        let preview = t.preview(&g.positions, &m).unwrap();
        assert_eq!(preview[0], g.positions[0]);
        assert!((preview[1][1] - 0.5).abs() < 1e-6);
        assert!(t.drag_weights(&m).is_none());
    }

    #[test]
    fn proportional_edit_keeps_several_selected_vertices_whole() {
        let (g, m) = square();
        let mut t = tool();
        t.proportional.on = true;
        t.proportional.radius = 150.0;
        click(&mut t, &g, &m, [0.0, 100.0], false);
        click(&mut t, &g, &m, [100.0, 100.0], true);
        t.press(&g.positions, &g.indices, &m, [100.0, 100.0], false);
        t.drag([100.0, 50.0]);
        let preview = t.preview(&g.positions, &m).unwrap();
        assert!((preview[0][1] - 0.5).abs() < 1e-6 && (preview[1][1] - 0.5).abs() < 1e-6);
        assert!(preview[2][1] > 1.0 && preview[2][1] < 1.5);
    }

    #[test]
    fn proportional_edit_through_a_deformer_moves_neighbours_in_drawing_space() {
        let (model, g, m) = warped();
        let (shown, space) = drawn(&model);
        let mut t = tool();
        t.proportional.on = true;
        t.proportional.radius = 1000.0;
        t.press(&shown, &g.indices, &m, m.to_screen(shown[2]), false);
        t.drag(m.to_screen([14.5, 25.0]));
        let live = t.dragging_stored(&g.positions, &m, &space).unwrap();
        // The pressed vertex is where the pointer is; the others moved too.
        let moved = live
            .iter()
            .zip(&g.positions)
            .filter(|(a, b)| a != b)
            .count();
        assert!(moved > 1, "{live:?}");
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
        // Outside would extend the outline, but this mesh covers the whole
        // texture; on a vertex is refused.
        assert_eq!(
            t.add_at("Skin", &g, &m, [150.0, 30.0]),
            Err(ToolError::Edit(EditError::OutsideTexture))
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
    fn auto_grid_follows_the_opaque_area_and_its_aspect() {
        let mut data = vec![0u8; 512 * 256];
        // Opaque 256 x 128 block: 4 columns, 2 rows.
        for y in 10..138 {
            for x in 20..276 {
                data[y * 512 + x] = 255;
            }
        }
        let alpha = AlphaImage {
            width: 512,
            height: 256,
            data,
        };
        assert_eq!(auto_grid(&alpha, 16), (4, 2));
        let clear = AlphaImage {
            data: vec![0; 512 * 256],
            ..alpha
        };
        assert_eq!(auto_grid(&clear, 16), (2, 2));
    }

    #[test]
    fn fill_grid_uses_the_tools_grid_and_keeps_it_across_meshes() {
        let (g, _) = square();
        let mut t = tool();
        assert_eq!(t.grid, (4, 4));
        t.grid = (2, 3);
        let (command, label) = t.fill_grid("Skin", &g).unwrap();
        assert_eq!(label, "mesh_fill_grid: Skin");
        let Command::SetMeshGeometry { positions, .. } = command else {
            panic!("a fill sends SetMeshGeometry");
        };
        // 1 x 2 inner points on a 2 x 3 grid.
        assert_eq!(positions.len(), 4 + 2);
        t.set_mesh(Some("Other"));
        assert_eq!(t.grid, (2, 3));
        t.grid = (0, 3);
        assert_eq!(
            t.fill_grid("Skin", &g),
            Err(ToolError::Edit(EditError::BadGrid))
        );
        t.grid = (1, 1);
        assert_eq!(
            t.fill_grid("Skin", &g),
            Err(ToolError::Edit(EditError::NothingToAdd))
        );
    }

    #[test]
    fn contour_uses_the_tools_settings_and_reports_a_missing_texture() {
        let (g, _) = square();
        let mut t = tool();
        assert_eq!(t.contour, ContourParams::default());
        assert_eq!(t.contour_mesh("Skin", &g, None), Err(ToolError::NoTexture));
        // A disc on a 64 x 64 image.
        let data: Vec<u8> = (0..64 * 64)
            .map(|i| {
                let (x, y) = ((i % 64) as f32 - 32.0, (i / 64) as f32 - 32.0);
                if x.hypot(y) < 24.0 { 255 } else { 0 }
            })
            .collect();
        let alpha = AlphaImage {
            width: 64,
            height: 64,
            data,
        };
        t.contour.spacing = 8.0;
        let (command, label) = t.contour_mesh("Skin", &g, Some(&alpha)).unwrap();
        assert_eq!(label, "mesh_contour: Skin");
        let Command::SetMeshGeometry { positions, .. } = command else {
            panic!("a contour sends SetMeshGeometry");
        };
        let with_lattice = positions.len();
        t.contour.spacing = 0.0;
        let Ok((Command::SetMeshGeometry { positions, .. }, _)) =
            t.contour_mesh("Skin", &g, Some(&alpha))
        else {
            panic!();
        };
        assert!(
            positions.len() < with_lattice,
            "the setting changes the result"
        );
        // The settings survive a change of mesh; a transparent image is refused.
        t.set_mesh(Some("Other"));
        assert_eq!(t.contour.spacing, 0.0);
        let clear = AlphaImage {
            data: vec![0; 64 * 64],
            ..alpha
        };
        assert_eq!(
            t.contour_mesh("Skin", &g, Some(&clear)),
            Err(ToolError::Contour(ContourError::Empty))
        );
    }

    #[test]
    fn menu_items_follow_the_selection() {
        let (g, m) = square();
        let mut t = tool();
        let enabled = |t: &MeshTool| MENU_ACTIONS.map(|a| t.menu_enabled(a));
        // Nothing selected: only the whole-mesh actions.
        assert_eq!(
            enabled(&t),
            [
                true, true, true, true, false, false, true, false, false, true, false, false,
                false, true, true, true
            ]
        );
        click(&mut t, &g, &m, [100.0, 100.0], false);
        assert_eq!(
            enabled(&t),
            [
                true, true, true, true, false, false, true, false, false, true, true, true, true,
                true, true, true
            ]
        );
        click(&mut t, &g, &m, [0.0, 0.0], true);
        assert_eq!(
            enabled(&t),
            [
                true, true, true, true, true, false, true, false, true, true, true, true, true,
                true, true, true
            ]
        );
        click(&mut t, &g, &m, [100.0, 0.0], true);
        assert_eq!(
            enabled(&t),
            [
                true, true, true, true, false, false, true, false, true, true, true, true, true,
                true, true, true
            ]
        );
        click(&mut t, &g, &m, [50.0, 50.0], false);
        assert_eq!(
            enabled(&t),
            [
                true, true, true, true, false, true, true, true, false, true, false, false, true,
                true, true, true
            ]
        );
    }

    #[test]
    fn a_right_click_selects_what_is_under_it_unless_already_selected() {
        let (g, m) = square();
        let mut t = tool();
        let right = |t: &mut MeshTool, at| t.context_select(&g.positions, &g.indices, &m, at);
        right(&mut t, [100.0, 100.0]);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [1]);
        // Two selected: a right click on one of them keeps both.
        click(&mut t, &g, &m, [0.0, 0.0], true);
        right(&mut t, [100.0, 100.0]);
        assert_eq!(t.vertices().len(), 2);
        // On another vertex: it alone.
        right(&mut t, [100.0, 0.0]);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [2]);
        // An edge selects it (and drops the vertices); the same edge again keeps it.
        right(&mut t, [50.0, 50.0]);
        assert_eq!((t.edge(), t.vertices().len()), (Some((0, 2)), 0));
        right(&mut t, [50.0, 50.0]);
        assert_eq!(t.edge(), Some((0, 2)));
        // Empty space changes nothing.
        right(&mut t, [80.0, 30.0]);
        assert_eq!(t.edge(), Some((0, 2)));
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
    fn picking_skips_a_visible_mesh_under_a_hidden_part() {
        let mut model = Model::new();
        for (name, parent) in [("Root", None), ("Child", Some("Root"))] {
            model
                .apply(Command::AddPart {
                    name: name.into(),
                    parent: parent.map(Into::into),
                })
                .unwrap();
        }
        let tri = vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
        model
            .apply(Command::AddMesh {
                name: "M".into(),
                part: "Child".into(),
                positions: tri.clone(),
                uvs: tri,
                indices: vec![0, 1, 2],
            })
            .unwrap();
        assert_eq!(pick_mesh(&model, [0.5, 0.5]).as_deref(), Some("M"));
        model
            .apply(Command::SetPartVisible {
                name: "Root".into(),
                visible: false,
            })
            .unwrap();
        assert_eq!(pick_mesh(&model, [0.5, 0.5]), None);
    }

    #[test]
    fn every_error_has_a_message_in_every_language() {
        use tsukumoe_i18n::{I18n, Language};
        let errors = [
            ToolError::NeedVertices,
            ToolError::NeedTwoVertices,
            ToolError::NeedSeveral,
            ToolError::NeedEdge,
            ToolError::NoChange,
            ToolError::NoTexture,
            ContourError::BadImage.into(),
            ContourError::BadParams.into(),
            ContourError::Empty.into(),
            ContourError::NoUvMapping.into(),
            ContourError::TooManyPoints.into(),
            ContourError::CannotTriangulate.into(),
            EditError::OutsideMesh.into(),
            EditError::TooClose.into(),
            EditError::BoundaryEdge.into(),
            EditError::NotConvex.into(),
            EditError::CannotRemove.into(),
            EditError::CannotConnect.into(),
            EditError::NoSuchEdge.into(),
            EditError::BadGrid.into(),
            EditError::NothingToAdd.into(),
        ];
        for lang in Language::ALL {
            let t = I18n::new(lang);
            for e in errors {
                assert_ne!(t.tr(e.key()), e.key(), "{lang:?} {e:?}");
            }
        }
    }

    /// A mesh under a warp that doubles and lifts it: stored (u, v) in 0..1,
    /// drawn over [10, 14] x [20, 26].
    fn warped() -> (Model, Geometry, Mapper) {
        let mut model = Model::new();
        for command in [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "P".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[10.0, 20.0], [14.0, 20.0], [10.0, 26.0], [14.0, 26.0]],
            },
            Command::AddMesh {
                name: "Skin".into(),
                part: "P".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
                indices: vec![0, 1, 2, 0, 2, 3],
            },
            Command::SetMeshParent {
                name: "Skin".into(),
                parent: Some("W".into()),
            },
        ] {
            model.apply(command).unwrap();
        }
        let g = geometry_of(&model, "Skin").unwrap();
        // 50 points per unit over [8, 16] x [18, 28].
        let m = Mapper {
            origin: [0.0, 0.0],
            size: [400.0, 500.0],
            view: [8.0, 18.0, 16.0, 28.0],
        };
        (model, g, m)
    }

    fn drawn(model: &Model) -> (Vec<Point>, Space) {
        let mesh = model
            .meshes()
            .get(model.meshes().find("Skin").unwrap())
            .unwrap();
        let space = Space::of(model, mesh.parent).unwrap();
        (
            mesh.positions
                .iter()
                .map(|&p| space.point_to_world(p))
                .collect(),
            space,
        )
    }

    #[test]
    fn dragging_a_vertex_of_a_deformed_mesh_moves_it_to_where_the_pointer_goes() {
        let (mut model, g, m) = warped();
        let (shown, space) = drawn(&model);
        assert_eq!(shown[2], [14.0, 26.0]);
        let mut t = tool();
        t.press(&shown, &g.indices, &m, m.to_screen(shown[2]), false);
        let to = m.to_screen([14.5, 25.0]);
        t.drag(to);
        let preview = t.preview(&shown, &m).unwrap();
        assert!((preview[2][0] - 14.5).abs() < 1e-3 && (preview[2][1] - 25.0).abs() < 1e-3);
        // What is shown live while dragging is what the release stores.
        let live = t.dragging_stored(&g.positions, &m, &space).unwrap();
        let (command, _) = t
            .release_in("Skin", &g.positions, &shown, &m, &space)
            .unwrap();
        assert_eq!(
            command,
            Command::SetMeshPositions {
                name: "Skin".into(),
                positions: live
            }
        );
        assert!(t.dragging_stored(&g.positions, &m, &space).is_none());
        model.apply(command).unwrap();
        let (after, _) = drawn(&model);
        assert!(
            (after[2][0] - 14.5).abs() < 1e-3 && (after[2][1] - 25.0).abs() < 1e-3,
            "{:?}",
            after[2]
        );
        for i in [0, 1, 3] {
            assert!(
                (after[i][0] - shown[i][0]).abs() < 1e-4
                    && (after[i][1] - shown[i][1]).abs() < 1e-4
            );
        }
        // The stored value is in (u, v), not in drawing units.
        let stored = geometry_of(&model, "Skin").unwrap().positions[2];
        assert!(
            stored[0] > 0.9 && stored[0] < 1.2 && stored[1] > 0.7 && stored[1] < 1.0,
            "{stored:?}"
        );
    }

    #[test]
    fn the_pointer_meets_the_drawn_vertices_not_the_stored_ones() {
        let (model, g, m) = warped();
        let (shown, _) = drawn(&model);
        let mut t = tool();
        // Clicking where the stored (u, v) would be (near the origin) selects nothing.
        t.click(&shown, &g.indices, &m, m.to_screen(g.positions[1]), false);
        assert!(t.vertices().is_empty());
        t.click(&shown, &g.indices, &m, m.to_screen(shown[1]), false);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [1]);
        // A box over the drawn upper vertices takes them.
        t.press(&shown, &g.indices, &m, m.to_screen([9.0, 27.0]), false);
        t.drag(m.to_screen([15.0, 25.0]));
        t.release("Skin", &shown, &m);
        assert_eq!(t.vertices().iter().copied().collect::<Vec<_>>(), [2, 3]);
    }

    #[test]
    fn a_vertex_added_in_a_deformed_mesh_lands_under_the_pointer() {
        let (mut model, g, m) = warped();
        let (shown, space) = drawn(&model);
        let mut t = tool();
        let (command, label) = t
            .add_at_in("Skin", &g, &shown, &m, m.to_screen([12.0, 22.0]), &space)
            .unwrap();
        assert_eq!(label, "mesh_add_vertex: Skin");
        model.apply(command).unwrap();
        let (after, _) = drawn(&model);
        let last = after.last().unwrap();
        assert!(
            (last[0] - 12.0).abs() < 1e-3 && (last[1] - 22.0).abs() < 1e-3,
            "{last:?}"
        );
    }

    #[test]
    fn the_front_mesh_is_picked_where_it_is_drawn() {
        use tsukumoe_core::eval::Frame;
        let (model, _, _) = warped();
        let compiled = model.compile().unwrap();
        let mut frame = Frame::new(&compiled);
        frame.evaluate(&compiled);
        let list = frame.draw_list(&compiled);
        // Drawn over [10, 14] x [20, 26]; the stored shape sits near the origin.
        assert_eq!(pick_drawn(&list, [12.0, 23.0]).as_deref(), Some("Skin"));
        assert_eq!(pick_drawn(&list, [0.5, 0.25]), None);
        assert_eq!(pick_mesh(&model, [0.5, 0.25]).as_deref(), Some("Skin"));
    }
}
