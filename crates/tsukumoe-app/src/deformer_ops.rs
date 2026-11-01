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
}

impl OpError {
    /// The translation key of the message.
    pub fn key(&self) -> &'static str {
        match self {
            Self::NothingSelected => "deformer-need-selection",
            Self::Missing(_) => "deformer-missing",
            Self::Flat => "deformer-flat-selection",
            Self::Space(_) => "deformer-flat-space",
        }
    }
}

impl fmt::Display for OpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing(name) => write!(f, "'{name}' is not in the model"),
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
    let (x0, y0, x1, y1) = (
        b[0] - w * margin,
        b[1] - h * margin,
        b[2] + w * margin,
        b[3] + h * margin,
    );
    let mut points = Vec::with_capacity(((rows + 1) * (columns + 1)) as usize);
    for r in 0..=rows {
        for c in 0..=columns {
            points.push([
                x0 + (x1 - x0) * c as f32 / columns as f32,
                y0 + (y1 - y0) * r as f32 / rows as f32,
            ]);
        }
    }
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
}
