// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The deformer tool as plain state and functions: which deformer is being
//! edited, which grid points are selected, and what a drag is doing. Like
//! the mesh tool it never touches the model (ADR-0019): a drag only moves
//! the preview, and its release gives one command (`SetWarpBasePoints` or
//! `SetRotationBase`). Shapes are read at the base pose of the model, in the
//! drawing space; the viewport feeds pointer positions, `panels::deformer_overlay`
//! draws.

use std::collections::BTreeSet;

use tsukumoe_core::command::{Command, RotationBase};
use tsukumoe_core::deformer_space::Space;
use tsukumoe_core::elements::Point;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::DeformerRef;

use crate::mesh_tool::{Mapper, Op};

/// The actions of the deformer tool's right-click menu.
pub const MENU_ACTIONS: [&str; 2] = ["deformer.create-warp", "deformer.create-rotation"];

/// A control point or a handle is picked within this many points of the pointer.
pub const PICK_RADIUS: f32 = 8.0;
/// A press that moves less than this many points is a click.
const DRAG_THRESHOLD: f32 = 3.0;

/// The handles of a rotation deformer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Handle {
    Origin,
    /// Sets the angle: sits on the local x axis.
    Angle,
    /// Sets the scale: sits on the local y axis, farther out for a bigger scale.
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hit {
    Point(u32),
    Handle(Handle),
    Nothing,
}

/// A deformer as the tool edits it.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    Warp {
        /// Control points in the parent's child space (the stored base).
        points: Vec<Point>,
        rows: u32,
        columns: u32,
        /// The parent's child space.
        parent: Space,
    },
    Rotation {
        base: RotationBase,
        parent: Space,
        /// Handle length in the parent's units at scale 1.
        radius: f32,
    },
}

fn p64(p: Point) -> [f64; 2] {
    [f64::from(p[0]), f64::from(p[1])]
}

fn pt(p: [f64; 2]) -> Point {
    [p[0] as f32, p[1] as f32]
}

impl Shape {
    /// The shape of the deformer `name`. `radius_world` is how long the
    /// rotation handles are, in drawing units.
    pub fn of(model: &Model, name: &str, radius_world: f32) -> Option<Self> {
        if let Some(id) = model.warps().find(name) {
            let w = model.warps().get(id)?;
            return Some(Self::Warp {
                points: w.base_points.clone(),
                rows: w.rows,
                columns: w.columns,
                parent: Space::of(model, w.parent).ok()?,
            });
        }
        let id = model.rotations().find(name)?;
        let r = model.rotations().get(id)?;
        let parent = Space::of(model, r.parent).ok()?;
        // One parent unit is this long in the drawing space.
        let o = p64(r.base.origin);
        let there = parent.to_world([o[0] + 1.0, o[1]]);
        let here = parent.to_world(o);
        let unit = (there[0] - here[0]).hypot(there[1] - here[1]);
        let radius = if unit > 1e-9 {
            radius_world / unit as f32
        } else {
            radius_world
        };
        Some(Self::Rotation {
            base: RotationBase::of(&r.base),
            parent,
            radius,
        })
    }

    /// The control points (warp) in the drawing space, row by row.
    pub fn world_points(&self) -> Vec<Point> {
        match self {
            Self::Warp { points, parent, .. } => {
                points.iter().map(|&p| parent.point_to_world(p)).collect()
            }
            Self::Rotation { .. } => Vec::new(),
        }
    }

    /// The handles (rotation) in the drawing space.
    pub fn handles(&self) -> Vec<(Handle, Point)> {
        let Self::Rotation {
            base,
            parent,
            radius,
        } = self
        else {
            return Vec::new();
        };
        let (sin, cos) = f64::from(base.angle_degrees).to_radians().sin_cos();
        let sx = if base.reflect_x { -1.0 } else { 1.0 };
        let sy = if base.reflect_y { -1.0 } else { 1.0 };
        let (o, r, s) = (p64(base.origin), f64::from(*radius), f64::from(base.scale));
        let angle = [o[0] + r * cos * sx, o[1] + r * sin * sx];
        let scale = [o[0] - r * s * sin * sy, o[1] + r * s * cos * sy];
        vec![
            (Handle::Origin, parent.point_to_world(base.origin)),
            (Handle::Angle, parent.point_to_world(pt(angle))),
            (Handle::Scale, parent.point_to_world(pt(scale))),
        ]
    }

    /// The command that stores this shape in the deformer `name`.
    fn command(&self, name: &str) -> Command {
        match self {
            Self::Warp { points, .. } => Command::SetWarpBasePoints {
                name: name.to_owned(),
                points: points.clone(),
            },
            Self::Rotation { base, .. } => Command::SetRotationBase {
                name: name.to_owned(),
                base: *base,
            },
        }
    }

    /// The shape after moving the pointer's grip from `from` to `to` (drawing
    /// space): the selected control points, or the handle that was grabbed.
    fn dragged(
        &self,
        grab: Grab,
        selected: &BTreeSet<u32>,
        from: Point,
        to: Point,
    ) -> Option<Self> {
        let mut out = self.clone();
        match (&mut out, grab) {
            (Self::Warp { points, parent, .. }, Grab::Points) => {
                let (a, b) = (from, to);
                let d = [b[0] - a[0], b[1] - a[1]];
                for &i in selected {
                    let world = parent.point_to_world(*points.get(i as usize)?);
                    points[i as usize] = parent
                        .point_from_world([world[0] + d[0], world[1] + d[1]])
                        .ok()?;
                }
            }
            (
                Self::Rotation {
                    base,
                    parent,
                    radius,
                },
                Grab::Handle(handle),
            ) => {
                let o_world = parent.point_to_world(base.origin);
                match handle {
                    Handle::Origin => {
                        let d = [to[0] - from[0], to[1] - from[1]];
                        base.origin = parent
                            .point_from_world([o_world[0] + d[0], o_world[1] + d[1]])
                            .ok()?;
                    }
                    Handle::Angle | Handle::Scale => {
                        let p = parent.point_from_world(to).ok()?;
                        let v = [
                            f64::from(p[0] - base.origin[0]),
                            f64::from(p[1] - base.origin[1]),
                        ];
                        let len = v[0].hypot(v[1]);
                        if len < 1e-9 {
                            return None;
                        }
                        if handle == Handle::Angle {
                            let sx = if base.reflect_x { -1.0 } else { 1.0 };
                            base.angle_degrees = (sx * v[1]).atan2(sx * v[0]).to_degrees() as f32;
                        } else {
                            base.scale = (len / f64::from(*radius)).clamp(0.01, 100.0) as f32;
                        }
                    }
                }
            }
            _ => return None,
        }
        Some(out)
    }
}

/// What a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Grab {
    Points,
    Handle(Handle),
}

#[derive(Clone, Debug, PartialEq)]
enum Gesture {
    Pending {
        at: [f32; 2],
        hit: Hit,
        shift: bool,
    },
    Move {
        grab: Grab,
        from: [f32; 2],
        to: [f32; 2],
    },
    Rect {
        from: [f32; 2],
        to: [f32; 2],
        add: bool,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeformerTool {
    deformer: Option<String>,
    points: BTreeSet<u32>,
    gesture: Option<Gesture>,
    /// Rows and columns of the warp `deformer.create-warp` makes; kept across deformers.
    pub grid: (u32, u32),
}

impl Default for DeformerTool {
    fn default() -> Self {
        Self {
            deformer: None,
            points: BTreeSet::new(),
            gesture: None,
            grid: (4, 4),
        }
    }
}

/// The control point or handle under `at` (screen points).
pub fn hit_test(shape: &Shape, m: &Mapper, at: [f32; 2]) -> Hit {
    let near = |p: Point| {
        let s = m.to_screen(p);
        (s[0] - at[0]).hypot(s[1] - at[1])
    };
    // Handles first: they sit apart from the grid, but the origin may sit on a point.
    let handle = shape
        .handles()
        .into_iter()
        .map(|(h, p)| (h, near(p)))
        .filter(|&(_, d)| d <= PICK_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((h, _)) = handle {
        return Hit::Handle(h);
    }
    shape
        .world_points()
        .iter()
        .enumerate()
        .map(|(i, &p)| (i, near(p)))
        .filter(|&(_, d)| d <= PICK_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or(Hit::Nothing, |(i, _)| Hit::Point(i as u32))
}

fn polygon_area(ring: &[Point]) -> f32 {
    let n = ring.len();
    (0..n)
        .map(|i| ring[i][0] * ring[(i + 1) % n][1] - ring[(i + 1) % n][0] * ring[i][1])
        .sum::<f32>()
        .abs()
        / 2.0
}

fn inside(ring: &[Point], p: Point) -> bool {
    let n = ring.len();
    let mut odd = false;
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
        {
            odd = !odd;
        }
    }
    odd
}

/// The deformer a click at `at` names: the origin of a rotation deformer
/// within reach, else the smallest warp whose grid covers the point.
pub fn pick_deformer(model: &Model, m: &Mapper, at: [f32; 2], radius_world: f32) -> Option<String> {
    let world = m.to_model(at);
    let origin = model
        .rotations()
        .iter()
        .filter_map(|(_, name, _)| {
            let shape = Shape::of(model, name, radius_world)?;
            let (_, o) = shape.handles().into_iter().next()?;
            let s = m.to_screen(o);
            let d = (s[0] - at[0]).hypot(s[1] - at[1]);
            (d <= PICK_RADIUS).then(|| (name.to_owned(), d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    if let Some((name, _)) = origin {
        return Some(name);
    }
    model
        .warps()
        .iter()
        .filter_map(|(_, name, w)| {
            let shape = Shape::of(model, name, radius_world)?;
            let g = shape.world_points();
            let (rows, cols) = (w.rows as usize, w.columns as usize);
            // The outline of the grid, walked around.
            let mut ring = Vec::new();
            ring.extend((0..=cols).map(|c| g[c]));
            ring.extend((1..=rows).map(|r| g[r * (cols + 1) + cols]));
            ring.extend((0..cols).rev().map(|c| g[rows * (cols + 1) + c]));
            ring.extend((1..rows).rev().map(|r| g[r * (cols + 1)]));
            inside(&ring, world).then(|| (name.to_owned(), polygon_area(&ring)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(n, _)| n)
}

impl DeformerTool {
    pub fn deformer(&self) -> Option<&str> {
        self.deformer.as_deref()
    }

    /// Sets the deformer being edited; a different one drops the selection.
    pub fn set_deformer(&mut self, name: Option<&str>) {
        if self.deformer.as_deref() != name {
            *self = Self {
                deformer: name.map(str::to_owned),
                grid: self.grid,
                ..Self::default()
            };
        }
    }

    pub fn points(&self) -> &BTreeSet<u32> {
        &self.points
    }

    pub fn is_dragging(&self) -> bool {
        matches!(
            self.gesture,
            Some(Gesture::Move { .. } | Gesture::Rect { .. })
        )
    }

    pub fn rect(&self) -> Option<([f32; 2], [f32; 2])> {
        match self.gesture {
            Some(Gesture::Rect { from, to, .. }) => Some((from, to)),
            _ => None,
        }
    }

    /// Drops selected points that no longer exist.
    pub fn sanitize(&mut self, point_count: usize) {
        self.points.retain(|&p| (p as usize) < point_count);
    }

    fn select(&mut self, hit: Hit, shift: bool) {
        match hit {
            Hit::Point(i) => {
                if shift {
                    if !self.points.remove(&i) {
                        self.points.insert(i);
                    }
                } else {
                    self.points = BTreeSet::from([i]);
                }
            }
            Hit::Handle(_) | Hit::Nothing => {
                if !shift {
                    self.points.clear();
                }
            }
        }
    }

    pub fn press(&mut self, shape: &Shape, m: &Mapper, at: [f32; 2], shift: bool) {
        let hit = hit_test(shape, m, at);
        self.gesture = Some(Gesture::Pending { at, hit, shift });
    }

    pub fn drag(&mut self, to: [f32; 2]) {
        self.gesture = match self.gesture.take() {
            Some(Gesture::Pending { at, hit, shift }) => {
                if (to[0] - at[0]).hypot(to[1] - at[1]) < DRAG_THRESHOLD {
                    Some(Gesture::Pending { at, hit, shift })
                } else {
                    match hit {
                        Hit::Point(i) => {
                            // A point that is not selected is selected alone
                            // (with Shift, added) before the move.
                            if !self.points.contains(&i) {
                                if !shift {
                                    self.points.clear();
                                }
                                self.points.insert(i);
                            }
                            Some(Gesture::Move {
                                grab: Grab::Points,
                                from: at,
                                to,
                            })
                        }
                        Hit::Handle(h) => Some(Gesture::Move {
                            grab: Grab::Handle(h),
                            from: at,
                            to,
                        }),
                        Hit::Nothing => Some(Gesture::Rect {
                            from: at,
                            to,
                            add: shift,
                        }),
                    }
                }
            }
            Some(Gesture::Move { grab, from, .. }) => Some(Gesture::Move { grab, from, to }),
            Some(Gesture::Rect { from, add, .. }) => Some(Gesture::Rect { from, to, add }),
            None => None,
        };
    }

    pub fn cancel(&mut self) {
        self.gesture = None;
    }

    /// The shape with the drag applied, while a move is on. The model is not changed.
    pub fn preview(&self, shape: &Shape, m: &Mapper) -> Option<Shape> {
        let Some(Gesture::Move { grab, from, to }) = self.gesture else {
            return None;
        };
        shape.dragged(grab, &self.points, m.to_model(from), m.to_model(to))
    }

    /// The button is released: a click selects, a box selects the points
    /// inside, a move gives its one command.
    pub fn release(&mut self, name: &str, shape: &Shape, m: &Mapper) -> Option<Op> {
        match self.gesture.take()? {
            Gesture::Pending { hit, shift, .. } => {
                self.select(hit, shift);
                None
            }
            Gesture::Rect { from, to, add } => {
                if !add {
                    self.points.clear();
                }
                let (x0, x1) = (from[0].min(to[0]), from[0].max(to[0]));
                let (y0, y1) = (from[1].min(to[1]), from[1].max(to[1]));
                for (i, p) in shape.world_points().into_iter().enumerate() {
                    let s = m.to_screen(p);
                    if (x0..=x1).contains(&s[0]) && (y0..=y1).contains(&s[1]) {
                        self.points.insert(i as u32);
                    }
                }
                None
            }
            Gesture::Move { grab, from, to } => {
                if from == to {
                    return None;
                }
                let moved = shape.dragged(grab, &self.points, m.to_model(from), m.to_model(to))?;
                let label = match grab {
                    Grab::Points => "deformer_move_points",
                    Grab::Handle(Handle::Origin) => "deformer_move_origin",
                    Grab::Handle(Handle::Angle) => "deformer_rotate",
                    Grab::Handle(Handle::Scale) => "deformer_scale",
                };
                Some((moved.command(name), format!("{label}: {name}")))
            }
        }
    }

    /// The shape a release now would store, while a move is on.
    pub fn dragging(&self, shape: &Shape, m: &Mapper) -> Option<Shape> {
        let Some(Gesture::Move { from, to, .. }) = self.gesture else {
            return None;
        };
        if from == to {
            return None;
        }
        self.preview(shape, m)
    }

    /// A click that selects, without a press and release in between.
    pub fn click(&mut self, shape: &Shape, m: &Mapper, at: [f32; 2], shift: bool) {
        let hit = hit_test(shape, m, at);
        self.select(hit, shift);
    }
}

impl Shape {
    /// The live preview of this shape for deformer `name`.
    pub fn as_preview(&self, name: &str) -> crate::scene::Preview {
        match self {
            Self::Warp { points, .. } => crate::scene::Preview::Warp {
                name: name.to_owned(),
                points: points.clone(),
            },
            Self::Rotation { base, .. } => crate::scene::Preview::Rotation {
                name: name.to_owned(),
                base: *base,
            },
        }
    }
}

/// How a deformer relates to the one being edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Parent,
    Child,
}

/// The parents (up the chain) and the direct child deformers of `name`, as
/// shapes, for the overlay to show in other colours.
pub fn related(model: &Model, name: &str, radius_world: f32) -> Vec<(Role, Shape)> {
    let Some(me) = model
        .warps()
        .find(name)
        .map(DeformerRef::Warp)
        .or_else(|| model.rotations().find(name).map(DeformerRef::Rotation))
    else {
        return Vec::new();
    };
    let parent_of = |r: DeformerRef| match r {
        DeformerRef::Warp(id) => model.warps().get(id).and_then(|d| d.parent),
        DeformerRef::Rotation(id) => model.rotations().get(id).and_then(|d| d.parent),
    };
    let mut out = Vec::new();
    let mut up = parent_of(me);
    // Bounded: the model has no loops, but never trust that in a draw loop.
    for _ in 0..16 {
        let Some(r) = up else { break };
        if let Some(shape) = name_of(model, r).and_then(|n| Shape::of(model, &n, radius_world)) {
            out.push((Role::Parent, shape));
        }
        up = parent_of(r);
    }
    let children = model
        .warps()
        .iter()
        .filter(|(_, _, d)| d.parent == Some(me))
        .map(|(_, n, _)| n.to_owned())
        .chain(
            model
                .rotations()
                .iter()
                .filter(|(_, _, d)| d.parent == Some(me))
                .map(|(_, n, _)| n.to_owned()),
        );
    for n in children {
        if let Some(shape) = Shape::of(model, &n, radius_world) {
            out.push((Role::Child, shape));
        }
    }
    out
}

/// The deformer a `DeformerRef` names, for callers that hold ids.
pub fn name_of(model: &Model, r: DeformerRef) -> Option<String> {
    match r {
        DeformerRef::Warp(id) => model.warps().name(id),
        DeformerRef::Rotation(id) => model.rotations().name(id),
    }
    .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 x 2 warp over [0,4] x [0,4], a rotation at (6, 2), and a mesh under the warp.
    fn model() -> Model {
        let mut m = Model::new();
        let mut grid = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                grid.push([c as f32 * 2.0, r as f32 * 2.0]);
            }
        }
        for command in [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "Warp".into(),
                part: "P".into(),
                parent: None,
                rows: 2,
                columns: 2,
                base_points: grid,
            },
            Command::AddRotationDeformer {
                name: "Rot".into(),
                part: "P".into(),
                parent: None,
                base: RotationBase {
                    origin: [6.0, 2.0],
                    angle_degrees: 0.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
        ] {
            m.apply(command).unwrap();
        }
        m
    }

    /// Screen = model * 100 with y flipped, over [0, 8] x [0, 6].
    fn mapper() -> Mapper {
        Mapper {
            origin: [0.0, 0.0],
            size: [800.0, 600.0],
            view: [0.0, 0.0, 8.0, 6.0],
        }
    }

    fn at(x: f32, y: f32) -> [f32; 2] {
        mapper().to_screen([x, y])
    }

    fn tool(name: &str) -> DeformerTool {
        let mut t = DeformerTool::default();
        t.set_deformer(Some(name));
        t
    }

    #[test]
    fn clicks_select_points_and_a_box_adds_more() {
        let m = model();
        let shape = Shape::of(&m, "Warp", 0.5).unwrap();
        let mut t = tool("Warp");
        t.click(&shape, &mapper(), at(2.0, 2.0), false);
        assert_eq!(t.points().iter().copied().collect::<Vec<_>>(), [4]);
        t.click(&shape, &mapper(), at(0.0, 0.0), true);
        assert_eq!(t.points().len(), 2);
        t.click(&shape, &mapper(), at(2.0, 2.0), true);
        assert_eq!(t.points().iter().copied().collect::<Vec<_>>(), [0]);
        t.click(&shape, &mapper(), at(1.0, 3.0), false);
        assert!(t.points().is_empty());
        // A box over the right column.
        let a = at(3.5, -0.5);
        t.press(&shape, &mapper(), a, false);
        t.drag(at(4.5, 4.5));
        assert!(t.is_dragging() && t.rect().is_some());
        assert!(t.release("Warp", &shape, &mapper()).is_none());
        assert_eq!(t.points().iter().copied().collect::<Vec<_>>(), [2, 5, 8]);
    }

    #[test]
    fn dragging_a_point_previews_and_ends_in_one_command() {
        let mut m = model();
        let before = Shape::of(&m, "Warp", 0.5).unwrap();
        let mut t = tool("Warp");
        t.press(&before, &mapper(), at(2.0, 2.0), false);
        t.drag(at(2.0, 2.0));
        assert!(!t.is_dragging());
        t.drag(at(2.5, 2.25));
        let preview = t.preview(&before, &mapper()).unwrap();
        assert_ne!(preview, before);
        // The model is untouched until the command is applied.
        assert_eq!(Shape::of(&m, "Warp", 0.5).unwrap(), before);
        let live = t.dragging(&before, &mapper()).unwrap();
        let (command, label) = t.release("Warp", &before, &mapper()).unwrap();
        assert_eq!(command, live.command("Warp"));
        assert!(t.dragging(&before, &mapper()).is_none());
        assert_eq!(label, "deformer_move_points: Warp");
        m.apply(command).unwrap();
        let after = Shape::of(&m, "Warp", 0.5).unwrap().world_points();
        assert!((after[4][0] - 2.5).abs() < 1e-4 && (after[4][1] - 2.25).abs() < 1e-4);
        assert_eq!(after[0], before.world_points()[0]);
        assert!(m.undo());
        assert_eq!(Shape::of(&m, "Warp", 0.5).unwrap(), before);
    }

    #[test]
    fn several_selected_points_move_together_and_cancel_leaves_nothing() {
        let m = model();
        let shape = Shape::of(&m, "Warp", 0.5).unwrap();
        let mut t = tool("Warp");
        t.click(&shape, &mapper(), at(2.0, 2.0), false);
        t.click(&shape, &mapper(), at(4.0, 2.0), true);
        t.press(&shape, &mapper(), at(2.0, 2.0), false);
        t.drag(at(2.0, 3.0));
        let (command, _) = t.release("Warp", &shape, &mapper()).unwrap();
        let Command::SetWarpBasePoints { points, .. } = command else {
            panic!("a warp move sends SetWarpBasePoints");
        };
        assert!((points[4][1] - 3.0).abs() < 1e-4 && (points[5][1] - 3.0).abs() < 1e-4);
        assert_eq!(points[3], [0.0, 2.0]);
        t.press(&shape, &mapper(), at(2.0, 2.0), false);
        t.drag(at(1.0, 1.0));
        t.cancel();
        assert!(t.release("Warp", &shape, &mapper()).is_none());
    }

    #[test]
    fn the_rotation_handles_set_origin_angle_and_scale() {
        let mut m = model();
        let shape = Shape::of(&m, "Rot", 1.0).unwrap();
        let handles: std::collections::BTreeMap<_, _> = shape.handles().into_iter().collect();
        assert_eq!(handles[&Handle::Origin], [6.0, 2.0]);
        assert_eq!(handles[&Handle::Angle], [7.0, 2.0]);
        assert_eq!(handles[&Handle::Scale], [6.0, 3.0]);
        let base = |m: &Model| {
            RotationBase::of(
                &m.rotations()
                    .get(m.rotations().find("Rot").unwrap())
                    .unwrap()
                    .base,
            )
        };
        // Origin.
        let mut t = tool("Rot");
        t.press(&shape, &mapper(), at(6.0, 2.0), false);
        t.drag(at(6.5, 2.5));
        let (c, label) = t.release("Rot", &shape, &mapper()).unwrap();
        assert_eq!(label, "deformer_move_origin: Rot");
        m.apply(c).unwrap();
        assert!((base(&m).origin[0] - 6.5).abs() < 1e-4 && (base(&m).origin[1] - 2.5).abs() < 1e-4);
        // Angle: drag the x handle straight up from the origin: 90 degrees.
        let shape = Shape::of(&m, "Rot", 1.0).unwrap();
        t.press(&shape, &mapper(), at(7.5, 2.5), false);
        t.drag(at(6.5, 3.5));
        let (c, label) = t.release("Rot", &shape, &mapper()).unwrap();
        assert_eq!(label, "deformer_rotate: Rot");
        m.apply(c).unwrap();
        assert!((base(&m).angle_degrees - 90.0).abs() < 1e-3);
        // Scale: the y handle now points left; pull it twice as far.
        let shape = Shape::of(&m, "Rot", 1.0).unwrap();
        let (_, h) = shape.handles()[2];
        t.press(&shape, &mapper(), mapper().to_screen(h), false);
        t.drag(at(4.5, 2.5));
        let (c, label) = t.release("Rot", &shape, &mapper()).unwrap();
        assert_eq!(label, "deformer_scale: Rot");
        m.apply(c).unwrap();
        assert!((base(&m).scale - 2.0).abs() < 1e-3, "{}", base(&m).scale);
    }

    #[test]
    fn a_warp_under_a_turned_rotation_follows_the_pointer_in_the_drawing_space() {
        let mut m = model();
        m.apply(Command::SetDeformerParent {
            name: "Warp".into(),
            parent: Some("Rot".into()),
        })
        .unwrap();
        m.apply(Command::SetRotationBase {
            name: "Rot".into(),
            base: RotationBase {
                origin: [1.0, 1.0],
                angle_degrees: 35.0,
                scale: 1.5,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        let shape = Shape::of(&m, "Warp", 0.5).unwrap();
        let world = shape.world_points();
        let mut t = tool("Warp");
        let start = mapper().to_screen(world[4]);
        t.press(&shape, &mapper(), start, false);
        t.drag([start[0] + 40.0, start[1] - 30.0]);
        let (c, _) = t.release("Warp", &shape, &mapper()).unwrap();
        m.apply(c).unwrap();
        let now = Shape::of(&m, "Warp", 0.5).unwrap().world_points();
        assert!((now[4][0] - (world[4][0] + 0.4)).abs() < 1e-3);
        assert!((now[4][1] - (world[4][1] + 0.3)).abs() < 1e-3);
    }

    #[test]
    fn a_click_names_the_rotation_origin_or_the_smallest_warp_under_it() {
        let mut m = model();
        let r = 0.5;
        assert_eq!(
            pick_deformer(&m, &mapper(), at(6.0, 2.0), r).as_deref(),
            Some("Rot")
        );
        assert_eq!(
            pick_deformer(&m, &mapper(), at(1.0, 1.0), r).as_deref(),
            Some("Warp")
        );
        assert_eq!(pick_deformer(&m, &mapper(), at(5.0, 5.0), r), None);
        // A smaller warp inside the first one wins over it.
        m.apply(Command::AddWarpDeformer {
            name: "Small".into(),
            part: "P".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.5, 0.5], [1.5, 0.5], [0.5, 1.5], [1.5, 1.5]],
        })
        .unwrap();
        assert_eq!(
            pick_deformer(&m, &mapper(), at(1.0, 1.0), r).as_deref(),
            Some("Small")
        );
        assert_eq!(
            pick_deformer(&m, &mapper(), at(3.0, 3.0), r).as_deref(),
            Some("Warp")
        );
    }

    #[test]
    fn changing_the_deformer_drops_the_selection_but_keeps_the_grid_setting() {
        let m = model();
        let shape = Shape::of(&m, "Warp", 0.5).unwrap();
        let mut t = tool("Warp");
        t.grid = (3, 5);
        t.click(&shape, &mapper(), at(2.0, 2.0), false);
        t.set_deformer(Some("Warp"));
        assert_eq!(t.points().len(), 1);
        t.set_deformer(Some("Rot"));
        assert!(t.points().is_empty());
        assert_eq!(t.grid, (3, 5));
        t.sanitize(0);
        assert_eq!(
            name_of(&m, DeformerRef::Warp(m.warps().find("Warp").unwrap())).as_deref(),
            Some("Warp")
        );
    }
}
