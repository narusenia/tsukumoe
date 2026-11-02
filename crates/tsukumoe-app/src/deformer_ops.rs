// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Making deformers and moving elements between them without changing how
//! they look. The commands do not convert coordinates (M5-05), so these
//! functions compute the converted points and return one `Batch`, which the
//! dispatcher applies as a single undo step. They read the model and never
//! change it (ADR-0019).

use std::fmt;

use tsukumoe_core::command::{Command, RotationBase};
use tsukumoe_core::deformer_space::{Space, SpaceError};
use tsukumoe_core::elements::Point;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::{DeformerRef, RotationKeyform};

use crate::mesh_tool::Op;
use crate::view::Selection;

/// An element that can be put under a deformer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Mesh(String),
    Deformer(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpError {
    /// Nothing to put under the new deformer.
    NothingSelected,
    /// An element named in the request is not in the model.
    Missing(String),
    /// The selection has no extent to wrap (a single point).
    Flat,
    /// A deformer is flat where the conversion needs its inverse.
    Space(SpaceError),
    /// Every deformer is in its initial shape already.
    NoChange,
    /// This deformer, or something under it, has keyforms, whose coordinates
    /// the edit would invalidate.
    HasKeyforms(String),
}

impl OpError {
    /// The translation key of the message.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NothingSelected => "deformer-need-selection",
            Self::Missing(_) => "deformer-missing",
            Self::Flat => "deformer-flat-selection",
            Self::Space(_) => "deformer-flat-space",
            Self::NoChange => "deformer-reset-nochange",
            Self::HasKeyforms(_) => "deformer-has-keyforms",
        }
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "'{name}' is not in the model"),
            Self::HasKeyforms(name) => write!(f, "'{name}' has keyforms"),
            other => f.write_str(other.key()),
        }
    }
}

impl From<SpaceError> for OpError {
    fn from(e: SpaceError) -> Self {
        Self::Space(e)
    }
}

fn deformer_ref(model: &Model, name: &str) -> Option<DeformerRef> {
    model
        .warps()
        .find(name)
        .map(DeformerRef::Warp)
        .or_else(|| model.rotations().find(name).map(DeformerRef::Rotation))
}

/// A name for a new deformer: `base`, else `base2`, `base3`, ...
pub fn unique_name(model: &Model, base: &str) -> String {
    let taken = |n: &str| model.warps().find(n).is_some() || model.rotations().find(n).is_some();
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base}{i}"))
        .find(|n| !taken(n))
        .expect("an unused name exists")
}

/// The points of `target` in the drawing space, at the base shape: the
/// vertices of a mesh, the control points of a warp, the origin of a rotation.
fn world_points(model: &Model, target: &Target) -> Result<Vec<Point>, OpError> {
    match target {
        Target::Mesh(name) => {
            let mesh = model
                .meshes()
                .find(name)
                .and_then(|id| model.meshes().get(id))
                .ok_or_else(|| OpError::Missing(name.clone()))?;
            let space = Space::of(model, mesh.parent)?;
            Ok(mesh
                .positions
                .iter()
                .map(|&p| space.point_to_world(p))
                .collect())
        }
        Target::Deformer(name) => {
            let r = deformer_ref(model, name).ok_or_else(|| OpError::Missing(name.clone()))?;
            Ok(match Space::of(model, Some(r))? {
                Space::Warp { grid, .. } => {
                    grid.iter().map(|&p| [p[0] as f32, p[1] as f32]).collect()
                }
                Space::Rotation { origin, .. } => vec![[origin[0] as f32, origin[1] as f32]],
                Space::Drawing => Vec::new(),
            })
        }
    }
}

/// `[x0, y0, x1, y1]` around all points of all targets.
fn bounds(model: &Model, targets: &[Target]) -> Result<[f32; 4], OpError> {
    let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
    for target in targets {
        for p in world_points(model, target)? {
            b = [
                b[0].min(p[0]),
                b[1].min(p[1]),
                b[2].max(p[0]),
                b[3].max(p[1]),
            ];
        }
    }
    if b[0] > b[2] {
        return Err(OpError::NothingSelected);
    }
    Ok(b)
}

/// The part of the first target, for the new deformer to belong to.
fn part_of(model: &Model, targets: &[Target]) -> Result<String, OpError> {
    let first = targets.first().ok_or(OpError::NothingSelected)?;
    let part = match first {
        Target::Mesh(n) => model
            .meshes()
            .find(n)
            .and_then(|id| model.meshes().get(id))
            .map(|m| m.part),
        Target::Deformer(n) => match deformer_ref(model, n) {
            Some(DeformerRef::Warp(id)) => model.warps().get(id).map(|d| d.part),
            Some(DeformerRef::Rotation(id)) => model.rotations().get(id).map(|d| d.part),
            None => None,
        },
    }
    .ok_or_else(|| OpError::Missing(format!("{first:?}")))?;
    model
        .parts()
        .name(part)
        .map(str::to_owned)
        .ok_or_else(|| OpError::Missing("part".into()))
}

/// A rotation deformer's base seen in another parent space, with its world
/// axes kept as far as one angle and one scale can: the origin follows the
/// point it marked, the angle and scale follow the image of the x axis.
fn convert_rotation(
    old: &Space,
    new: &Space,
    base: &RotationKeyform,
) -> Result<RotationBase, OpError> {
    const STEP: f64 = 0.01;
    let (sin, cos) = f64::from(base.angle_degrees).to_radians().sin_cos();
    let sx = if base.reflect_x { -1.0 } else { 1.0 };
    let s = f64::from(base.scale);
    let ex = [s * cos * sx, s * sin * sx];
    let o = [f64::from(base.origin[0]), f64::from(base.origin[1])];
    let ow = old.to_world(o);
    let wex = {
        let p = old.to_world([o[0] + STEP * ex[0], o[1] + STEP * ex[1]]);
        [(p[0] - ow[0]) / STEP, (p[1] - ow[1]) / STEP]
    };
    let on = new.from_world(ow)?;
    let nex = {
        let p = new.from_world([ow[0] + STEP * wex[0], ow[1] + STEP * wex[1]])?;
        [(p[0] - on[0]) / STEP, (p[1] - on[1]) / STEP]
    };
    let scale = nex[0].hypot(nex[1]);
    let angle = (sx * nex[1]).atan2(sx * nex[0]).to_degrees();
    Ok(RotationBase {
        origin: [on[0] as f32, on[1] as f32],
        angle_degrees: angle as f32,
        scale: scale as f32,
        reflect_x: base.reflect_x,
        reflect_y: base.reflect_y,
    })
}

/// The commands that put `target` under `parent` (a deformer name, or none)
/// whose child space is `new`, keeping the drawn position of everything.
fn reparent(
    model: &Model,
    target: &Target,
    parent: Option<&str>,
    new: &Space,
) -> Result<Vec<Command>, OpError> {
    let parent = parent.map(str::to_owned);
    match target {
        Target::Mesh(name) => {
            let mesh = model
                .meshes()
                .find(name)
                .and_then(|id| model.meshes().get(id))
                .ok_or_else(|| OpError::Missing(name.clone()))?;
            let old = Space::of(model, mesh.parent)?;
            let positions = mesh
                .positions
                .iter()
                .map(|&p| Ok(new.point_from_world(old.point_to_world(p))?))
                .collect::<Result<Vec<_>, OpError>>()?;
            Ok(vec![
                Command::SetMeshParent {
                    name: name.clone(),
                    parent,
                },
                Command::SetMeshPositions {
                    name: name.clone(),
                    positions,
                },
            ])
        }
        Target::Deformer(name) => {
            let set_parent = Command::SetDeformerParent {
                name: name.clone(),
                parent,
            };
            match deformer_ref(model, name).ok_or_else(|| OpError::Missing(name.clone()))? {
                DeformerRef::Warp(id) => {
                    let warp = model.warps().get(id).expect("found above");
                    let old = Space::of(model, warp.parent)?;
                    let points = warp
                        .base_points
                        .iter()
                        .map(|&p| Ok(new.point_from_world(old.point_to_world(p))?))
                        .collect::<Result<Vec<_>, OpError>>()?;
                    Ok(vec![
                        set_parent,
                        Command::SetWarpBasePoints {
                            name: name.clone(),
                            points,
                        },
                    ])
                }
                DeformerRef::Rotation(id) => {
                    let rot = model.rotations().get(id).expect("found above");
                    let old = Space::of(model, rot.parent)?;
                    let base = convert_rotation(&old, new, &rot.base)?;
                    Ok(vec![
                        set_parent,
                        Command::SetRotationBase {
                            name: name.clone(),
                            base,
                        },
                    ])
                }
            }
        }
    }
}

fn batch(commands: Vec<Command>, label: String) -> Op {
    (Command::Batch { commands }, label)
}

/// The control points of an evenly spaced grid of `rows` x `columns` cells
/// over `rect` (`[x0, y0, x1, y1]`), row by row.
pub fn regular_grid(rect: [f32; 4], rows: u32, columns: u32) -> Vec<Point> {
    let [x0, y0, x1, y1] = rect;
    let mut points = Vec::with_capacity(((rows + 1) * (columns + 1)) as usize);
    for r in 0..=rows {
        for c in 0..=columns {
            points.push([
                x0 + (x1 - x0) * c as f32 / columns as f32,
                y0 + (y1 - y0) * r as f32 / rows as f32,
            ]);
        }
    }
    points
}

/// Moves the origin of the rotation deformer `name` to `origin` and keeps
/// everything that hangs from it where it is drawn: the meshes and the child
/// deformers get the coordinates that, in the moved axes, come out at the
/// same place (a child rotation keeps its world axes as far as one angle and
/// one scale can). One `Batch`, so one undo step. Fails if the rotation or
/// anything directly under it has keyforms.
pub fn move_axis(model: &Model, name: &str, origin: Point) -> Result<Op, OpError> {
    let id = model
        .rotations()
        .find(name)
        .ok_or_else(|| OpError::Missing(name.to_owned()))?;
    let rot = model.rotations().get(id).expect("found above");
    if rot.keyforms.is_some() {
        return Err(OpError::HasKeyforms(name.to_owned()));
    }
    let me = DeformerRef::Rotation(id);
    let old = Space::of(model, Some(me))?;
    let up = Space::of(model, rot.parent)?;
    let mut base = RotationBase::of(&rot.base);
    base.origin = origin;
    let new = Space::rotation(&up, &base);
    let keep = |points: &[Point]| -> Result<Vec<Point>, OpError> {
        points
            .iter()
            .map(|&p| Ok(new.point_from_world(old.point_to_world(p))?))
            .collect()
    };
    let mut commands = vec![Command::SetRotationBase {
        name: name.to_owned(),
        base,
    }];
    for (_, mesh, m) in model
        .meshes()
        .iter()
        .filter(|(_, _, m)| m.parent == Some(me))
    {
        if m.keyforms.is_some() {
            return Err(OpError::HasKeyforms(mesh.to_owned()));
        }
        commands.push(Command::SetMeshPositions {
            name: mesh.to_owned(),
            positions: keep(&m.positions)?,
        });
    }
    for (_, child, d) in model
        .warps()
        .iter()
        .filter(|(_, _, d)| d.parent == Some(me))
    {
        if d.keyforms.is_some() {
            return Err(OpError::HasKeyforms(child.to_owned()));
        }
        commands.push(Command::SetWarpBasePoints {
            name: child.to_owned(),
            points: keep(&d.base_points)?,
        });
    }
    for (_, child, d) in model
        .rotations()
        .iter()
        .filter(|(_, _, d)| d.parent == Some(me))
    {
        if d.keyforms.is_some() {
            return Err(OpError::HasKeyforms(child.to_owned()));
        }
        commands.push(Command::SetRotationBase {
            name: child.to_owned(),
            base: convert_rotation(&old, &new, &d.base)?,
        });
    }
    Ok(batch(commands, format!("deformer_move_axis: {name}")))
}

/// What the scene shows while the commands of a drag are not yet committed:
/// each shape-setting command as a preview, so the picture is the committed one.
pub fn previews_of(command: &Command) -> Vec<crate::scene::Preview> {
    use crate::scene::Preview;
    let all = match command {
        Command::Batch { commands } => commands.iter().collect::<Vec<_>>(),
        one => vec![one],
    };
    all.into_iter()
        .filter_map(|c| match c {
            Command::SetMeshPositions { name, positions } => Some(Preview::Mesh {
                name: name.clone(),
                positions: positions.clone(),
            }),
            Command::SetWarpBasePoints { name, points } => Some(Preview::Warp {
                name: name.clone(),
                points: points.clone(),
            }),
            Command::SetRotationBase { name, base } => Some(Preview::Rotation {
                name: name.clone(),
                base: *base,
            }),
            _ => None,
        })
        .collect()
}

/// Puts the base shape of the deformers in `items` back to its initial state:
/// a warp's grid becomes the evenly spaced grid of its own bounding box (the
/// cell counts stay), a rotation goes back to angle 0, scale 1 and no
/// reflection (its origin stays). Deformers already so are left out; if all
/// are, `NoChange`. One `Batch`, so one undo step; a deformer with keyforms
/// makes the whole batch fail when applied, and nothing changes.
pub fn reset(model: &Model, items: &[Selection]) -> Result<Op, OpError> {
    let mut commands = Vec::new();
    let mut names = Vec::new();
    for item in items {
        match item {
            Selection::Warp(name) => {
                let id = model
                    .warps()
                    .find(name)
                    .ok_or_else(|| OpError::Missing(name.clone()))?;
                let w = model.warps().get(id).expect("found above");
                let mut b = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for p in &w.base_points {
                    b = [
                        b[0].min(p[0]),
                        b[1].min(p[1]),
                        b[2].max(p[0]),
                        b[3].max(p[1]),
                    ];
                }
                let grid = regular_grid(b, w.rows, w.columns);
                let same = grid.len() == w.base_points.len()
                    && grid
                        .iter()
                        .zip(&w.base_points)
                        .all(|(a, p)| (a[0] - p[0]).abs() <= 1e-6 && (a[1] - p[1]).abs() <= 1e-6);
                if !same {
                    commands.push(Command::SetWarpBasePoints {
                        name: name.clone(),
                        points: grid,
                    });
                    names.push(name.clone());
                }
            }
            Selection::Rotation(name) => {
                let id = model
                    .rotations()
                    .find(name)
                    .ok_or_else(|| OpError::Missing(name.clone()))?;
                let base = RotationBase::of(&model.rotations().get(id).expect("found above").base);
                let initial = RotationBase {
                    angle_degrees: 0.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                    ..base
                };
                if initial != base {
                    commands.push(Command::SetRotationBase {
                        name: name.clone(),
                        base: initial,
                    });
                    names.push(name.clone());
                }
            }
            _ => {}
        }
    }
    let asked = items
        .iter()
        .any(|s| matches!(s, Selection::Warp(_) | Selection::Rotation(_)));
    if !asked {
        return Err(OpError::NothingSelected);
    }
    if commands.is_empty() {
        return Err(OpError::NoChange);
    }
    let label = match names.as_slice() {
        [one] => format!("deformer_reset: {one}"),
        many => format!("deformer_reset: {} items", many.len()),
    };
    Ok(batch(commands, label))
}

/// A warp deformer of `rows` x `columns` cells around the targets (with a
/// small margin), which become its children. One command, one undo step.
pub fn create_warp(
    model: &Model,
    targets: &[Target],
    rows: u32,
    columns: u32,
) -> Result<Op, OpError> {
    let b = bounds(model, targets)?;
    let (w, h) = (b[2] - b[0], b[3] - b[1]);
    if w <= 0.0 || h <= 0.0 {
        return Err(OpError::Flat);
    }
    let margin = 0.05;
    let points = regular_grid(
        [
            b[0] - w * margin,
            b[1] - h * margin,
            b[2] + w * margin,
            b[3] + h * margin,
        ],
        rows,
        columns,
    );
    let name = unique_name(model, "Warp");
    let space = Space::Warp {
        grid: points
            .iter()
            .map(|p| [f64::from(p[0]), f64::from(p[1])])
            .collect(),
        rows: rows as usize,
        columns: columns as usize,
    };
    let mut commands = vec![Command::AddWarpDeformer {
        name: name.clone(),
        part: part_of(model, targets)?,
        parent: None,
        rows,
        columns,
        base_points: points,
    }];
    for target in targets {
        commands.extend(reparent(model, target, Some(&name), &space)?);
    }
    Ok(batch(commands, format!("deformer_create_warp: {name}")))
}

/// A rotation deformer at the centre of the targets, which become its children.
pub fn create_rotation(model: &Model, targets: &[Target]) -> Result<Op, OpError> {
    let b = bounds(model, targets)?;
    let origin = [(b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0];
    let name = unique_name(model, "Rotation");
    let space = Space::Rotation {
        origin: [f64::from(origin[0]), f64::from(origin[1])],
        ex: [1.0, 0.0],
        ey: [0.0, 1.0],
    };
    let mut commands = vec![Command::AddRotationDeformer {
        name: name.clone(),
        part: part_of(model, targets)?,
        parent: None,
        base: RotationBase {
            origin,
            angle_degrees: 0.0,
            scale: 1.0,
            reflect_x: false,
            reflect_y: false,
        },
    }];
    for target in targets {
        commands.extend(reparent(model, target, Some(&name), &space)?);
    }
    Ok(batch(commands, format!("deformer_create_rotation: {name}")))
}

/// Puts `target` under the deformer `parent` (none to take it out) keeping
/// where it is drawn. A parent that would make a loop is refused by the
/// command, and the model stays as it was.
pub fn set_parent(model: &Model, target: &Target, parent: Option<&str>) -> Result<Op, OpError> {
    let space = match parent {
        None => Space::Drawing,
        Some(n) => {
            let r = deformer_ref(model, n).ok_or_else(|| OpError::Missing(n.to_owned()))?;
            Space::of(model, Some(r))?
        }
    };
    let name = match target {
        Target::Mesh(n) | Target::Deformer(n) => n,
    };
    Ok(batch(
        reparent(model, target, parent, &space)?,
        format!("deformer_set_parent: {name}"),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::compiled::CompiledModel;
    use tsukumoe_core::eval::Frame;

    fn part() -> Command {
        Command::AddPart {
            name: "P".into(),
            parent: None,
        }
    }

    fn mesh(name: &str, x: f32) -> Command {
        let pts = vec![[x, 0.0], [x + 2.0, 0.0], [x + 2.0, 3.0], [x, 3.0]];
        Command::AddMesh {
            name: name.into(),
            part: "P".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2, 0, 2, 3],
        }
    }

    fn built(commands: Vec<Command>) -> Model {
        let mut m = Model::new();
        for c in commands {
            m.apply(c).unwrap();
        }
        m
    }

    /// Every mesh's evaluated vertices, in the order of the model.
    fn drawn(m: &Model) -> Vec<f32> {
        let compiled: CompiledModel = m.compile().unwrap();
        let mut frame = Frame::new(&compiled);
        frame.evaluate(&compiled);
        frame.positions().to_vec()
    }

    fn same(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-3, "{x} vs {y}");
        }
    }

    fn mesh_target(n: &str) -> Target {
        Target::Mesh(n.into())
    }

    #[test]
    fn a_warp_around_a_quad_keeps_it_looking_the_same_and_undoes_in_one_step() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let before = drawn(&m);
        let (command, label) = create_warp(&m, &[mesh_target("Skin")], 4, 4).unwrap();
        assert!(label.starts_with("deformer_create_warp: Warp"));
        m.apply(command).unwrap();
        let warp = m.warps().find("Warp").unwrap();
        let skin = m.meshes().get(m.meshes().find("Skin").unwrap()).unwrap();
        assert_eq!(skin.parent, Some(DeformerRef::Warp(warp)));
        assert_eq!(m.warps().get(warp).unwrap().base_points.len(), 25);
        same(&drawn(&m), &before);
        // The grid wraps the quad with a margin.
        let grid = &m.warps().get(warp).unwrap().base_points;
        assert!(grid[0][0] < 1.0 && grid[24][0] > 3.0 && grid[0][1] < 0.0 && grid[24][1] > 3.0);
        assert!(m.undo());
        assert!(m.warps().find("Warp").is_none());
        same(&drawn(&m), &before);
        assert!(!m.undo() || m.history().commands().count() == 1);
    }

    #[test]
    fn a_warp_wraps_several_meshes_and_takes_a_fresh_name() {
        let mut m = built(vec![part(), mesh("A", 0.0), mesh("B", 10.0)]);
        let before = drawn(&m);
        let (command, _) = create_warp(&m, &[mesh_target("A"), mesh_target("B")], 2, 3).unwrap();
        m.apply(command).unwrap();
        let w = m.warps().get(m.warps().find("Warp").unwrap()).unwrap();
        assert_eq!((w.rows, w.columns), (2, 3));
        assert!(w.base_points[0][0] < 0.0 && w.base_points.last().unwrap()[0] > 12.0);
        same(&drawn(&m), &before);
        assert_eq!(unique_name(&m, "Warp"), "Warp2");
    }

    #[test]
    fn a_rotation_deformer_sits_at_the_centre_and_keeps_the_look() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let before = drawn(&m);
        let (command, _) = create_rotation(&m, &[mesh_target("Skin")]).unwrap();
        m.apply(command).unwrap();
        let rot = m
            .rotations()
            .get(m.rotations().find("Rotation").unwrap())
            .unwrap();
        assert_eq!(rot.base.origin, [2.0, 1.5]);
        same(&drawn(&m), &before);
        assert!(m.undo());
        same(&drawn(&m), &before);
    }

    #[test]
    fn moving_out_of_and_back_under_a_bent_warp_keeps_the_bent_look() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let (create, _) = create_warp(&m, &[mesh_target("Skin")], 3, 3).unwrap();
        m.apply(create).unwrap();
        // Bend the grid, then take the mesh out: it keeps the bent look.
        let id = m.warps().find("Warp").unwrap();
        let mut points = m.warps().get(id).unwrap().base_points.clone();
        points[5][0] += 0.3;
        m.apply(Command::SetWarpBasePoints {
            name: "Warp".into(),
            points,
        })
        .unwrap();
        let bent = drawn(&m);
        let (out, _) = set_parent(&m, &mesh_target("Skin"), None).unwrap();
        m.apply(out).unwrap();
        same(&drawn(&m), &bent);
        // The unbent look after the grid is straight again and the mesh is back inside.
        let (back, _) = set_parent(&m, &mesh_target("Skin"), Some("Warp")).unwrap();
        m.apply(back).unwrap();
        same(&drawn(&m), &bent);
    }

    #[test]
    fn a_rotation_deformer_moved_under_another_keeps_its_children_in_place() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let (c, _) = create_rotation(&m, &[mesh_target("Skin")]).unwrap();
        m.apply(c).unwrap();
        // A second rotation, turned, at another place.
        m.apply(Command::AddRotationDeformer {
            name: "Turn".into(),
            part: "P".into(),
            parent: None,
            base: RotationBase {
                origin: [5.0, 5.0],
                angle_degrees: 40.0,
                scale: 1.5,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        let before = drawn(&m);
        let (c, _) = set_parent(&m, &Target::Deformer("Rotation".into()), Some("Turn")).unwrap();
        m.apply(c).unwrap();
        same(&drawn(&m), &before);
        let (c, _) = set_parent(&m, &Target::Deformer("Rotation".into()), None).unwrap();
        m.apply(c).unwrap();
        same(&drawn(&m), &before);
    }

    #[test]
    fn a_warp_moved_under_a_rotation_keeps_its_grid_in_place() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let (c, _) = create_warp(&m, &[mesh_target("Skin")], 2, 2).unwrap();
        m.apply(c).unwrap();
        m.apply(Command::AddRotationDeformer {
            name: "Turn".into(),
            part: "P".into(),
            parent: None,
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 25.0,
                scale: 2.0,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        let before = drawn(&m);
        let (c, _) = set_parent(&m, &Target::Deformer("Warp".into()), Some("Turn")).unwrap();
        m.apply(c).unwrap();
        same(&drawn(&m), &before);
    }

    #[test]
    fn a_loop_is_refused_and_the_model_is_untouched() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let (c, _) = create_warp(&m, &[mesh_target("Skin")], 2, 2).unwrap();
        m.apply(c).unwrap();
        let (c, _) = create_rotation(&m, &[Target::Deformer("Warp".into())]).unwrap();
        m.apply(c).unwrap();
        let before = drawn(&m);
        let steps = m.history().commands().count();
        // The rotation holds the warp; putting the rotation under the warp loops.
        let (c, _) = set_parent(&m, &Target::Deformer("Rotation".into()), Some("Warp")).unwrap();
        assert!(m.apply(c).is_err());
        assert_eq!(m.history().commands().count(), steps);
        same(&drawn(&m), &before);
    }

    #[test]
    fn nothing_selected_a_single_point_and_a_missing_name_are_errors() {
        let m = built(vec![part(), mesh("Skin", 1.0)]);
        assert_eq!(create_warp(&m, &[], 4, 4), Err(OpError::NothingSelected));
        assert_eq!(
            create_rotation(&m, &[mesh_target("Nope")]),
            Err(OpError::Missing("Nope".into()))
        );
        assert_eq!(
            set_parent(&m, &mesh_target("Skin"), Some("Nope")),
            Err(OpError::Missing("Nope".into()))
        );
        let dot = {
            let mut m = Model::new();
            m.apply(part()).unwrap();
            m.apply(Command::AddMesh {
                name: "Dot".into(),
                part: "P".into(),
                positions: vec![[1.0, 1.0]; 3],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            })
            .unwrap();
            m
        };
        assert_eq!(
            create_warp(&dot, &[mesh_target("Dot")], 2, 2),
            Err(OpError::Flat)
        );
    }

    fn bent_warp_model() -> Model {
        let mut m = built(vec![part(), mesh("Skin", 0.0)]);
        let (c, _) = create_warp(&m, &[mesh_target("Skin")], 2, 3).unwrap();
        m.apply(c).unwrap();
        let id = m.warps().find("Warp").unwrap();
        let mut points = m.warps().get(id).unwrap().base_points.clone();
        points[4][0] += 0.4;
        points[4][1] -= 0.3;
        points[9][1] += 0.5;
        m.apply(Command::SetWarpBasePoints {
            name: "Warp".into(),
            points,
        })
        .unwrap();
        m
    }

    #[test]
    fn resetting_a_warp_makes_its_grid_even_over_its_own_box_and_one_undo_brings_the_shape_back() {
        let mut m = bent_warp_model();
        let id = m.warps().find("Warp").unwrap();
        let bent = m.warps().get(id).unwrap().base_points.clone();
        let box_of = |p: &[Point]| {
            p.iter()
                .fold([f32::MAX, f32::MAX, f32::MIN, f32::MIN], |b, q| {
                    [
                        b[0].min(q[0]),
                        b[1].min(q[1]),
                        b[2].max(q[0]),
                        b[3].max(q[1]),
                    ]
                })
        };
        let before_drawn = drawn(&m);
        let (command, label) = reset(&m, &[Selection::Warp("Warp".into())]).unwrap();
        assert_eq!(label, "deformer_reset: Warp");
        let steps = m.history().commands().count();
        m.apply(command).unwrap();
        assert_eq!(m.history().commands().count(), steps + 1);
        let w = m.warps().get(id).unwrap();
        // The same cells, the same bounding box, evenly spaced.
        assert_eq!((w.rows, w.columns, w.base_points.len()), (2, 3, 12));
        assert_eq!(box_of(&w.base_points), box_of(&bent));
        assert_eq!(w.base_points, regular_grid(box_of(&bent), 2, 3));
        // The child is drawn where the new grid puts it (not where the bent one did).
        assert_ne!(drawn(&m), before_drawn);
        assert!(m.undo());
        assert_eq!(m.warps().get(id).unwrap().base_points, bent);
        same(&drawn(&m), &before_drawn);
        // Reset twice: the second finds nothing to do.
        m.redo();
        assert_eq!(
            reset(&m, &[Selection::Warp("Warp".into())]),
            Err(OpError::NoChange)
        );
    }

    #[test]
    fn resetting_a_rotation_zeroes_the_angle_and_keeps_the_origin() {
        let mut m = built(vec![part(), mesh("Skin", 1.0)]);
        let (c, _) = create_rotation(&m, &[mesh_target("Skin")]).unwrap();
        m.apply(c).unwrap();
        m.apply(Command::SetRotationBase {
            name: "Rotation".into(),
            base: RotationBase {
                origin: [2.0, 1.5],
                angle_degrees: 40.0,
                scale: 2.0,
                reflect_x: true,
                reflect_y: true,
            },
        })
        .unwrap();
        let (command, label) = reset(&m, &[Selection::Rotation("Rotation".into())]).unwrap();
        assert_eq!(label, "deformer_reset: Rotation");
        m.apply(command).unwrap();
        let base = m
            .rotations()
            .get(m.rotations().find("Rotation").unwrap())
            .unwrap()
            .base;
        assert_eq!(
            (
                base.origin,
                base.angle_degrees,
                base.scale,
                base.reflect_x,
                base.reflect_y
            ),
            ([2.0, 1.5], 0.0, 1.0, false, false)
        );
        assert_eq!(
            reset(&m, &[Selection::Rotation("Rotation".into())]),
            Err(OpError::NoChange)
        );
    }

    #[test]
    fn several_deformers_reset_in_one_batch_and_the_untouched_ones_are_left_out() {
        let mut m = bent_warp_model();
        let (c, _) = create_rotation(&m, &[Target::Deformer("Warp".into())]).unwrap();
        m.apply(c).unwrap();
        // Rotation is still initial: only the warp changes, and the label names it alone.
        let both = [
            Selection::Warp("Warp".into()),
            Selection::Rotation("Rotation".into()),
        ];
        let (command, label) = reset(&m, &both).unwrap();
        assert_eq!(label, "deformer_reset: Warp");
        assert!(matches!(&command, Command::Batch { commands } if commands.len() == 1));
        m.apply(command).unwrap();
        // Turn the rotation too: now two go in one batch, one undo step.
        m.apply(Command::SetRotationBase {
            name: "Rotation".into(),
            base: RotationBase {
                origin: [0.0, 0.0],
                angle_degrees: 30.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        })
        .unwrap();
        let id = m.warps().find("Warp").unwrap();
        let mut points = m.warps().get(id).unwrap().base_points.clone();
        points[2][0] += 0.2;
        m.apply(Command::SetWarpBasePoints {
            name: "Warp".into(),
            points,
        })
        .unwrap();
        let (command, label) = reset(&m, &both).unwrap();
        assert_eq!(label, "deformer_reset: 2 items");
        let steps = m.history().commands().count();
        m.apply(command).unwrap();
        assert_eq!(m.history().commands().count(), steps + 1);
        assert!(m.undo());
        assert_eq!(
            m.rotations()
                .get(m.rotations().find("Rotation").unwrap())
                .unwrap()
                .base
                .angle_degrees,
            30.0
        );
    }

    #[test]
    fn a_deformer_with_keyforms_refuses_and_the_whole_batch_changes_nothing() {
        use tsukumoe_core::elements::{Parameter, Part};
        use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
        use tsukumoe_core::model::ModelParts;
        use tsukumoe_core::rig::{WarpDeformer, WarpKeyform};
        let mut parts = ModelParts::default();
        let p = parts
            .parameters
            .insert("P", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let part_id = parts.parts.insert("Part", Part::default()).unwrap();
        let binding = parts.bindings.insert(
            Binding::new(vec![Axis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let bent = vec![[0.0, 0.0], [1.5, 0.2], [0.0, 1.0], [1.0, 1.2]];
        let kf = WarpKeyform {
            points: bent.clone(),
            opacity: 1.0,
        };
        parts
            .warps
            .insert(
                "Keyed",
                WarpDeformer {
                    part: part_id,
                    parent: None,
                    rows: 1,
                    columns: 1,
                    bezier: false,
                    base_points: bent.clone(),
                    rest_points: None,
                    keyforms: Some(KeyformGrid::new(binding, vec![kf.clone(), kf.clone()])),
                },
            )
            .unwrap();
        let mut m = Model::from_parts(parts).unwrap();
        // A plain warp beside it that would change.
        m.apply(Command::AddWarpDeformer {
            name: "Plain".into(),
            part: "Part".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: bent.clone(),
        })
        .unwrap();
        let (command, _) = reset(
            &m,
            &[
                Selection::Warp("Plain".into()),
                Selection::Warp("Keyed".into()),
            ],
        )
        .unwrap();
        let steps = m.history().commands().count();
        let err = m.apply(command).unwrap_err();
        assert!(err.to_string().contains("keyforms"), "{err}");
        // The batch is atomic: the plain warp kept its shape too.
        assert_eq!(m.history().commands().count(), steps);
        let plain = m.warps().get(m.warps().find("Plain").unwrap()).unwrap();
        assert_eq!(plain.base_points, bent);
    }

    #[test]
    fn nothing_selected_or_something_that_is_not_a_deformer_is_an_error() {
        let m = bent_warp_model();
        assert_eq!(reset(&m, &[]), Err(OpError::NothingSelected));
        assert_eq!(
            reset(&m, &[Selection::Mesh("Skin".into())]),
            Err(OpError::NothingSelected)
        );
        assert_eq!(
            reset(&m, &[Selection::Warp("Nope".into())]),
            Err(OpError::Missing("Nope".into()))
        );
    }

    /// A rotation at (5, 4), turned, scaled and reflected, holding a mesh, a
    /// warp and a rotation (with a mesh of its own).
    fn axis_model(parent_warp: bool) -> Model {
        let tri = |x: f32| vec![[x, 0.0], [x + 1.0, 0.2], [x + 0.3, 1.5]];
        let mut commands = vec![
            part(),
            Command::AddRotationDeformer {
                name: "Axis".into(),
                part: "P".into(),
                parent: None,
                base: RotationBase {
                    origin: [5.0, 4.0],
                    angle_degrees: 33.0,
                    scale: 1.6,
                    reflect_x: false,
                    reflect_y: true,
                },
            },
            Command::AddMesh {
                name: "Direct".into(),
                part: "P".into(),
                positions: tri(0.2),
                uvs: tri(0.0),
                indices: vec![0, 1, 2],
            },
            Command::AddWarpDeformer {
                name: "ChildWarp".into(),
                part: "P".into(),
                parent: Some("Axis".into()),
                rows: 1,
                columns: 1,
                base_points: vec![[-1.0, -1.0], [1.0, -1.0], [-1.0, 1.0], [1.2, 1.1]],
            },
            Command::AddRotationDeformer {
                name: "ChildTurn".into(),
                part: "P".into(),
                parent: Some("Axis".into()),
                base: RotationBase {
                    origin: [0.5, -0.4],
                    angle_degrees: 20.0,
                    scale: 0.8,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
            Command::AddMesh {
                name: "InWarp".into(),
                part: "P".into(),
                positions: tri(0.0),
                uvs: tri(0.0),
                indices: vec![0, 1, 2],
            },
            Command::AddMesh {
                name: "InTurn".into(),
                part: "P".into(),
                positions: tri(0.1),
                uvs: tri(0.0),
                indices: vec![0, 1, 2],
            },
            Command::SetMeshParent {
                name: "Direct".into(),
                parent: Some("Axis".into()),
            },
            Command::SetMeshParent {
                name: "InWarp".into(),
                parent: Some("ChildWarp".into()),
            },
            Command::SetMeshParent {
                name: "InTurn".into(),
                parent: Some("ChildTurn".into()),
            },
        ];
        if parent_warp {
            commands.insert(
                1,
                Command::AddWarpDeformer {
                    name: "Outer".into(),
                    part: "P".into(),
                    parent: None,
                    rows: 2,
                    columns: 2,
                    base_points: vec![
                        [0.0, 0.0],
                        [5.0, -0.5],
                        [10.0, 0.3],
                        [-0.4, 4.0],
                        [5.5, 4.5],
                        [10.4, 4.0],
                        [0.2, 8.0],
                        [5.0, 8.6],
                        [10.0, 8.0],
                    ],
                },
            );
            commands.insert(
                3,
                Command::SetDeformerParent {
                    name: "Axis".into(),
                    parent: Some("Outer".into()),
                },
            );
            // Its origin and size are in the warp's (u, v) space now.
            commands.insert(
                4,
                Command::SetRotationBase {
                    name: "Axis".into(),
                    base: RotationBase {
                        origin: [0.5, 0.45],
                        angle_degrees: 33.0,
                        scale: 0.1,
                        reflect_x: false,
                        reflect_y: true,
                    },
                },
            );
        }
        built(commands)
    }

    #[test]
    fn moving_the_axis_keeps_every_child_where_it_is_drawn() {
        let mut m = axis_model(false);
        let before = drawn(&m);
        let (command, label) = move_axis(&m, "Axis", [7.0, 2.5]).unwrap();
        assert_eq!(label, "deformer_move_axis: Axis");
        let steps = m.history().commands().count();
        m.apply(command).unwrap();
        // One step; the origin moved; nothing drawn moved (a mesh, a warp's mesh, a rotation's mesh).
        assert_eq!(m.history().commands().count(), steps + 1);
        let id = m.rotations().find("Axis").unwrap();
        assert_eq!(m.rotations().get(id).unwrap().base.origin, [7.0, 2.5]);
        same(&drawn(&m), &before);
        // The children's own coordinates did change, to make that so.
        let direct = m.meshes().get(m.meshes().find("Direct").unwrap()).unwrap();
        assert_ne!(direct.positions[0], [0.2, 0.0]);
        // The axis is where the handle put it: the rotation's child space is drawn from there.
        assert!(m.undo());
        same(&drawn(&m), &before);
        assert_eq!(m.rotations().get(id).unwrap().base.origin, [5.0, 4.0]);
    }

    #[test]
    fn under_a_warp_the_drawn_positions_stay_within_the_tolerance() {
        let mut m = axis_model(true);
        let before = drawn(&m);
        let (command, _) = move_axis(&m, "Axis", [0.56, 0.4]).unwrap();
        m.apply(command).unwrap();
        let after = drawn(&m);
        assert_eq!(before.len(), after.len());
        for (a, b) in before.iter().zip(&after) {
            // The parent warp bends the axes, so a child rotation's angle is the
            // closest one angle and scale can give: close, not exact.
            assert!((a - b).abs() < 0.05, "{a} vs {b}");
        }
    }

    #[test]
    fn a_plain_origin_edit_still_moves_everything_under_the_rotation() {
        let mut m = axis_model(false);
        let before = drawn(&m);
        let id = m.rotations().find("Axis").unwrap();
        let mut base = RotationBase::of(&m.rotations().get(id).unwrap().base);
        base.origin = [7.0, 2.5];
        m.apply(Command::SetRotationBase {
            name: "Axis".into(),
            base,
        })
        .unwrap();
        assert_ne!(drawn(&m), before);
    }

    #[test]
    fn a_child_with_keyforms_refuses_and_the_model_is_untouched() {
        use tsukumoe_core::elements::{Parameter, Part};
        use tsukumoe_core::keyform::{Axis as KeyAxis, Binding, KeyformGrid};
        use tsukumoe_core::model::ModelParts;
        use tsukumoe_core::rig::{ArtMesh, DeformerRef, MeshKeyform, RotationDeformer};
        let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        let mut parts = ModelParts::default();
        let p = parts
            .parameters
            .insert("P", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let part_id = parts.parts.insert("Part", Part::default()).unwrap();
        let binding = parts.bindings.insert(
            Binding::new(vec![KeyAxis {
                parameter: p,
                keys: vec![0.0, 1.0],
            }])
            .unwrap(),
        );
        let rot = parts
            .rotations
            .insert(
                "Axis",
                RotationDeformer {
                    part: part_id,
                    parent: None,
                    base: Default::default(),
                    keyforms: None,
                },
            )
            .unwrap();
        let key = MeshKeyform {
            positions: tri.clone(),
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        };
        let mut keyed = ArtMesh::new(part_id, tri.clone(), tri.clone(), vec![0, 1, 2]);
        keyed.parent = Some(DeformerRef::Rotation(rot));
        keyed.keyforms = Some(KeyformGrid::new(binding, vec![key.clone(), key]));
        parts.meshes.insert("Keyed", keyed).unwrap();
        let m = Model::from_parts(parts).unwrap();
        assert_eq!(
            move_axis(&m, "Axis", [1.0, 1.0]),
            Err(OpError::HasKeyforms("Keyed".into()))
        );
        assert_eq!(
            move_axis(&m, "Nope", [0.0, 0.0]),
            Err(OpError::Missing("Nope".into()))
        );
    }

    #[test]
    fn the_live_preview_of_an_axis_move_is_the_committed_picture() {
        use crate::scene::Scene;
        let mut m = axis_model(false);
        let mut scene = Scene::new(&m).unwrap();
        scene.advance(0.0);
        let names = ["Direct", "InWarp", "InTurn"];
        let shown = |scene: &Scene| -> Vec<Vec<[f32; 2]>> {
            names
                .iter()
                .map(|n| scene.drawn_positions(n).unwrap())
                .collect()
        };
        let before = shown(&scene);
        let (command, _) = move_axis(&m, "Axis", [7.0, 2.5]).unwrap();
        let previews = previews_of(&command);
        // The axis and the three things hanging directly from it.
        assert_eq!(previews.len(), 4);
        scene.set_previews(&previews);
        scene.advance(0.0);
        // The children stay where they are drawn, though the axis moved.
        let live = shown(&scene);
        for (a, b) in before.iter().flatten().zip(live.iter().flatten()) {
            assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3);
        }
        // The committed picture is the same as the preview.
        m.apply(command).unwrap();
        let mut done = Scene::new(&m).unwrap();
        done.advance(0.0);
        for (a, b) in shown(&done).iter().flatten().zip(live.iter().flatten()) {
            assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
        }
        // Esc: the previews go and the original model's picture is back.
        scene.set_previews(&[]);
        scene.advance(0.0);
        for (a, b) in before.iter().flatten().zip(shown(&scene).iter().flatten()) {
            assert!((a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4);
        }
    }
}
