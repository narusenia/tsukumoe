// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The bone tool as plain state and functions: where the bones are, picking a
//! joint, placing and moving one, and painting bone weights on a mesh. Like
//! the other tools it never touches the model (ADR-0019): a drag only moves
//! the preview, and its release gives one command (`SetBoneBase`,
//! `SetBoneKeyform` or `SetMeshInfluences`). The viewport feeds pointer
//! positions, `panels::bone_overlay` draws.
//!
//! Bones are stored in the space of the parent deformer of the meshes they
//! skin (ADR-0021); [`space_of`] gives that space so they can be drawn and
//! picked where the picture shows them.

use std::collections::BTreeSet;

use tsukumoe_core::affine::Affine;
use tsukumoe_core::command::{BoneBase, BoneWeight, Command};
use tsukumoe_core::deformer_space::Space;
use tsukumoe_core::elements::{ParameterId, Point};
use tsukumoe_core::keyform_edit::sample_at;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::{BoneId, BoneTransform};

use crate::keyed::{self, Place};
use crate::mesh_tool::{Mapper, Op, ToolError};

/// A joint is picked within this many points of the pointer.
pub const PICK_RADIUS: f32 = 10.0;
/// A press that moves less than this many points is a click.
const DRAG_THRESHOLD: f32 = 3.0;
/// Most bones that drive one vertex.
const MAX_BONES: usize = 4;
/// A weight under this is dropped.
const MIN_WEIGHT: f32 = 1e-3;
/// The weight a full-strength stamp at the brush centre moves.
const STAMP: f32 = 0.35;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    /// Place and move joints.
    #[default]
    Bones,
    /// Paint the weights of the picked bone on the picked mesh.
    Weights,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BrushKind {
    #[default]
    Add,
    Subtract,
    /// Pull the weight toward the average of the neighbours.
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// In screen points.
    pub radius: f32,
    /// 0 to 1.
    pub strength: f32,
    pub kind: BrushKind,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            radius: 40.0,
            strength: 0.5,
            kind: BrushKind::Add,
        }
    }
}

/// One bone as the overlay draws it, in the drawing space.
#[derive(Clone, Debug, PartialEq)]
pub struct Joint {
    pub name: String,
    pub parent: Option<String>,
    pub at: Point,
}

/// The weights of every vertex of a mesh: bone names and weights.
pub type Weights = Vec<Vec<(String, f32)>>;

#[derive(Clone, Debug, PartialEq)]
enum Drag {
    /// A joint is being moved, from where the press began.
    Joint { name: String, start: [f32; 2] },
    /// A stroke is painting `weights` on `mesh`.
    Stroke { mesh: String, weights: Weights },
}

/// What the tool remembers between frames. Not saved, not in the history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoneTool {
    pub mode: Mode,
    pub brush: Brush,
    /// The mesh whose weights are painted: the mesh chosen last.
    pub mesh: Option<String>,
    /// The bone whose weights are painted and under which a new bone goes:
    /// the bone chosen last.
    pub bone: Option<String>,
    drag: Option<Drag>,
    /// Where the pointer is now while a joint is dragged (screen).
    pointer: Option<[f32; 2]>,
}

fn p64(p: Point) -> [f64; 2] {
    [f64::from(p[0]), f64::from(p[1])]
}

fn pt(p: [f64; 2]) -> Point {
    [p[0] as f32, p[1] as f32]
}

/// The local transform of bone `id` with every parameter at `at`.
pub fn local_at(
    model: &Model,
    id: BoneId,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<BoneTransform> {
    let bone = model.bones().get(id)?;
    Some(match &bone.keyforms {
        Some(grid) => sample_at(model.bindings().get(grid.binding)?, &grid.keyforms, at),
        None => bone.base,
    })
}

/// Where bone `id` puts a point of its own space, in the space the roots are
/// in: the chain of parents applied.
pub fn world_of(model: &Model, id: BoneId, at: &dyn Fn(ParameterId) -> f32) -> Option<Affine> {
    let mut chain = vec![id];
    while let Some(parent) = model.bones().get(*chain.last()?)?.parent {
        if chain.contains(&parent) {
            return None;
        }
        chain.push(parent);
    }
    let mut world = Affine::default();
    for &bone in chain.iter().rev() {
        world = world.then_after(&Affine::from_bone(&local_at(model, bone, at)?));
    }
    Some(world)
}

/// The space the bones of `mesh` are in: its parent deformer's space as the
/// picture shows it. The drawing space when there is no mesh or no parent.
pub fn space_of(model: &Model, mesh: Option<&str>, at: &dyn Fn(ParameterId) -> f32) -> Space {
    let parent = mesh
        .and_then(|n| model.meshes().get(model.meshes().find(n)?))
        .and_then(|m| m.parent);
    Space::of_at(model, parent, at).unwrap_or(Space::Drawing)
}

/// Every bone with the point its joint is at, in the drawing space.
pub fn joints(model: &Model, space: &Space, at: &dyn Fn(ParameterId) -> f32) -> Vec<Joint> {
    model
        .bones()
        .iter()
        .filter_map(|(id, name, bone)| {
            let origin = world_of(model, id, at)?.apply([0.0, 0.0]);
            Some(Joint {
                name: name.to_owned(),
                parent: bone
                    .parent
                    .and_then(|p| model.bones().name(p))
                    .map(str::to_owned),
                at: pt(space.to_world(p64(origin))),
            })
        })
        .collect()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// The joint nearest to `screen` within [`PICK_RADIUS`].
pub fn pick_joint(joints: &[Joint], mapper: &Mapper, screen: [f32; 2]) -> Option<String> {
    joints
        .iter()
        .map(|j| (dist(mapper.to_screen(j.at), screen), j))
        .filter(|(d, _)| *d <= PICK_RADIUS)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, j)| j.name.clone())
}

/// A name for a new bone that no bone has.
pub fn unique_name(model: &Model) -> String {
    (1..)
        .map(|n| format!("Bone{n}"))
        .find(|name| model.bones().find(name).is_none())
        .expect("endless range")
}

/// The base that puts the joint of a bone (a child of `parent`) at `world`
/// (drawing space), keeping `keep`'s rotation and scale. `None` when the
/// point cannot be mapped back.
fn base_at(
    model: &Model,
    space: &Space,
    parent: Option<BoneId>,
    world: Point,
    keep: BoneTransform,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<BoneBase> {
    let local = space.from_world(p64(world)).ok()?;
    let local = pt(local);
    let translation = match parent {
        Some(p) => world_of(model, p, at)?.inverse()?.apply(local),
        None => local,
    };
    Some(BoneBase {
        translation,
        rotation_degrees: keep.rotation_degrees,
        scale: keep.scale,
    })
}

/// The command that sets the transform of bone `name` as the sliders stand: a
/// bone with keyforms is edited at the key they are on.
pub fn set_command(
    model: &Model,
    name: &str,
    base: BoneBase,
    at: &dyn Fn(ParameterId) -> f32,
) -> Result<Op, ToolError> {
    let id = model
        .bones()
        .find(name)
        .ok_or(ToolError::Bone("bone-err-missing"))?;
    let has_keys = model.bones().get(id).is_some_and(|b| b.keyforms.is_some());
    let label = format!("bone_move: {name}");
    if !has_keys {
        return Ok((
            Command::SetBoneBase {
                name: name.to_owned(),
                base,
            },
            label,
        ));
    }
    match keyed::bone_place(model, name, at) {
        Some(Place::Key { key, .. }) => Ok((
            Command::SetBoneKeyform {
                name: name.to_owned(),
                key,
                base,
            },
            label,
        )),
        _ => Err(ToolError::Bone("param-err-not-on-key")),
    }
}

impl BoneTool {
    /// The joint being dragged and where the pointer is, for the preview.
    pub fn dragged_joint(&self) -> Option<(&str, [f32; 2])> {
        match (&self.drag, self.pointer) {
            (Some(Drag::Joint { name, .. }), Some(p)) => Some((name.as_str(), p)),
            _ => None,
        }
    }

    /// The weights being painted, for the overlay to show before they are
    /// committed.
    pub fn stroke(&self) -> Option<(&str, &Weights)> {
        match &self.drag {
            Some(Drag::Stroke { mesh, weights }) => Some((mesh.as_str(), weights)),
            _ => None,
        }
    }

    /// Follows the selection: a mesh or bone chosen last becomes the target.
    /// One that is gone from the model is forgotten.
    pub fn follow(&mut self, model: &Model, mesh: Option<String>, bone: Option<String>) {
        if mesh.is_some() {
            self.mesh = mesh;
        }
        if bone.is_some() {
            self.bone = bone;
        }
        if self
            .mesh
            .as_deref()
            .is_some_and(|n| model.meshes().find(n).is_none())
        {
            self.mesh = None;
        }
        if self
            .bone
            .as_deref()
            .is_some_and(|n| model.bones().find(n).is_none())
        {
            self.bone = None;
        }
    }

    pub fn cancel(&mut self) {
        self.drag = None;
        self.pointer = None;
    }

    /// A press that starts dragging the joint under `screen`, if any.
    pub fn press_joint(&mut self, joints: &[Joint], mapper: &Mapper, screen: [f32; 2]) -> bool {
        match pick_joint(joints, mapper, screen) {
            Some(name) => {
                self.drag = Some(Drag::Joint {
                    name,
                    start: screen,
                });
                self.pointer = Some(screen);
                true
            }
            None => false,
        }
    }

    pub fn drag_to(&mut self, screen: [f32; 2]) {
        if self.drag.is_some() {
            self.pointer = Some(screen);
        }
    }

    /// The end of a joint drag: the command that moves it, or nothing when the
    /// pointer did not move enough to be a drag.
    pub fn release_joint(
        &mut self,
        model: &Model,
        space: &Space,
        mapper: &Mapper,
        at: &dyn Fn(ParameterId) -> f32,
    ) -> Option<Result<Op, ToolError>> {
        let Some(Drag::Joint { name, start }) = self.drag.take() else {
            return None;
        };
        let end = self.pointer.take()?;
        if dist(start, end) < DRAG_THRESHOLD {
            return None;
        }
        let id = model.bones().find(&name)?;
        let bone = model.bones().get(id)?;
        let keep = local_at(model, id, at)?;
        let world = mapper.to_model(end);
        Some(
            base_at(model, space, bone.parent, world, keep, at)
                .ok_or(ToolError::Bone("bone-err-space"))
                .and_then(|base| set_command(model, &name, base, at)),
        )
    }

    /// A click on empty ground: a new bone there, under `parent`.
    pub fn add_command(
        model: &Model,
        space: &Space,
        mapper: &Mapper,
        screen: [f32; 2],
        parent: Option<&str>,
        at: &dyn Fn(ParameterId) -> f32,
    ) -> Result<(String, Op), ToolError> {
        let parent_id = parent.and_then(|p| model.bones().find(p));
        let base = base_at(
            model,
            space,
            parent_id,
            mapper.to_model(screen),
            BoneTransform::default(),
            at,
        )
        .ok_or(ToolError::Bone("bone-err-space"))?;
        let name = unique_name(model);
        let command = Command::AddBone {
            name: name.clone(),
            parent: parent_id.and_then(|_| parent.map(str::to_owned)),
            base,
        };
        Ok((name.clone(), (command, format!("bone_add: {name}"))))
    }

    /// Starts or continues a stroke: a stamp of the brush at `screen` on the
    /// weights of `mesh`, which are taken from the model on the first stamp.
    /// `vertices` are the mesh's vertices on screen.
    pub fn stamp(
        &mut self,
        model: &Model,
        mesh: &str,
        bone: &str,
        vertices: &[[f32; 2]],
        screen: [f32; 2],
    ) {
        let fresh = !matches!(&self.drag, Some(Drag::Stroke { mesh: m, .. }) if m == mesh);
        if fresh {
            let Some(weights) = weights_of(model, mesh) else {
                return;
            };
            self.drag = Some(Drag::Stroke {
                mesh: mesh.to_owned(),
                weights,
            });
        }
        let brush = self.brush;
        let neighbours = neighbours_of(model, mesh);
        if let Some(Drag::Stroke { weights, .. }) = &mut self.drag {
            paint(weights, bone, vertices, &neighbours, screen, &brush);
        }
    }

    /// The end of a stroke: the command that sets the weights it painted.
    pub fn release_stroke(&mut self) -> Option<Op> {
        let Some(Drag::Stroke { mesh, weights }) = self.drag.take() else {
            return None;
        };
        let influences = weights
            .iter()
            .map(|v| {
                v.iter()
                    .map(|(bone, weight)| BoneWeight {
                        bone: bone.clone(),
                        weight: *weight,
                    })
                    .collect()
            })
            .collect();
        Some((
            Command::SetMeshInfluences {
                name: mesh.clone(),
                influences,
            },
            format!("bone_weights: {mesh}"),
        ))
    }
}

/// The bone weights of every vertex of `mesh` as the model has them: one empty
/// list per vertex when the mesh is not skinned yet.
pub fn weights_of(model: &Model, mesh: &str) -> Option<Weights> {
    let m = model.meshes().get(model.meshes().find(mesh)?)?;
    if m.influences.is_empty() {
        return Some(vec![Vec::new(); m.positions.len()]);
    }
    Some(
        m.influences
            .iter()
            .map(|inf| {
                inf.bones
                    .iter()
                    .zip(&inf.weights)
                    .filter_map(|(b, w)| Some((model.bones().name((*b)?)?.to_owned(), *w)))
                    .filter(|(_, w)| *w > 0.0)
                    .collect()
            })
            .collect(),
    )
}

/// For each vertex of `mesh`, the vertices it shares a triangle edge with.
pub fn neighbours_of(model: &Model, mesh: &str) -> Vec<Vec<u32>> {
    let Some(m) = model
        .meshes()
        .find(mesh)
        .and_then(|id| model.meshes().get(id))
    else {
        return Vec::new();
    };
    let mut sets: Vec<BTreeSet<u32>> = vec![BTreeSet::new(); m.positions.len()];
    for tri in m.indices.as_chunks::<3>().0 {
        for (a, b) in [(0, 1), (1, 2), (2, 0)] {
            sets[tri[a] as usize].insert(tri[b]);
            sets[tri[b] as usize].insert(tri[a]);
        }
    }
    sets.into_iter().map(|s| s.into_iter().collect()).collect()
}

fn weight_of(list: &[(String, f32)], bone: &str) -> f32 {
    list.iter()
        .find(|(b, _)| b == bone)
        .map_or(0.0, |(_, w)| *w)
}

/// Sets the weight of `bone` in `list`. Others give way to keep the total at
/// most 1, and the smallest goes when there would be more than four bones.
fn set_weight(list: &mut Vec<(String, f32)>, bone: &str, weight: f32) {
    let weight = weight.clamp(0.0, 1.0);
    list.retain(|(b, _)| b != bone);
    let others: f32 = list.iter().map(|(_, w)| w).sum();
    if others > 1.0 - weight && others > 0.0 {
        let scale = (1.0 - weight).max(0.0) / others;
        for (_, w) in list.iter_mut() {
            *w *= scale;
        }
    }
    if weight >= MIN_WEIGHT {
        list.push((bone.to_owned(), weight));
    }
    list.retain(|(_, w)| *w >= MIN_WEIGHT);
    while list.len() > MAX_BONES {
        let weakest = list
            .iter()
            .enumerate()
            .filter(|(_, (b, _))| b != bone)
            .min_by(|a, b| a.1.1.total_cmp(&b.1.1))
            .map(|(i, _)| i);
        match weakest {
            Some(i) => {
                list.remove(i);
            }
            None => break,
        }
    }
}

/// One stamp of `brush` at `screen` on `weights`, for `bone`.
pub fn paint(
    weights: &mut Weights,
    bone: &str,
    vertices: &[[f32; 2]],
    neighbours: &[Vec<u32>],
    screen: [f32; 2],
    brush: &Brush,
) {
    let before: Vec<f32> = weights.iter().map(|w| weight_of(w, bone)).collect();
    for (i, v) in vertices.iter().enumerate().take(weights.len()) {
        let d = dist(*v, screen);
        if d > brush.radius {
            continue;
        }
        let t = 1.0 - d / brush.radius.max(1e-3);
        let amount = STAMP * brush.strength * t * t.sqrt();
        let now = before[i];
        let next = match brush.kind {
            BrushKind::Add => now + amount,
            BrushKind::Subtract => now - amount,
            BrushKind::Smooth => {
                let near = neighbours.get(i).map_or(&[][..], |n| &n[..]);
                if near.is_empty() {
                    continue;
                }
                let mean =
                    near.iter().map(|&n| before[n as usize]).sum::<f32>() / near.len() as f32;
                now + (mean - now) * (brush.strength * t).min(1.0)
            }
        };
        set_weight(&mut weights[i], bone, next);
    }
}

/// The weight of `bone` at every vertex, for the overlay's heat colours.
pub fn heat(weights: &Weights, bone: &str) -> Vec<f32> {
    weights.iter().map(|w| weight_of(w, bone)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1 model unit is 1 point; y goes up on screen.
    const MAPPER: Mapper = Mapper {
        origin: [0.0, 0.0],
        size: [100.0, 100.0],
        view: [0.0, 0.0, 100.0, 100.0],
    };

    fn screen(p: Point) -> [f32; 2] {
        MAPPER.to_screen(p)
    }

    fn base(x: f32, y: f32) -> BoneBase {
        BoneBase {
            translation: [x, y],
            rotation_degrees: 0.0,
            scale: [1.0, 1.0],
        }
    }

    fn model() -> Model {
        let mut m = Model::new();
        let steps = [
            Command::AddParameter {
                name: "P".into(),
                min: -1.0,
                max: 1.0,
                default: 0.0,
                repeat: false,
            },
            Command::AddPart {
                name: "Part".into(),
                parent: None,
            },
            Command::AddMesh {
                name: "Skin".into(),
                part: "Part".into(),
                positions: vec![[10.0, 10.0], [20.0, 10.0], [10.0, 20.0], [90.0, 90.0]],
                uvs: vec![[0.0, 0.0]; 4],
                indices: vec![0, 1, 2, 1, 2, 3],
            },
            Command::AddBone {
                name: "Root".into(),
                parent: None,
                base: base(10.0, 10.0),
            },
            Command::AddBone {
                name: "Arm".into(),
                parent: Some("Root".into()),
                base: base(5.0, 0.0),
            },
        ];
        for step in steps {
            m.apply(step).unwrap();
        }
        m
    }

    fn zero(_: ParameterId) -> f32 {
        0.0
    }

    fn near(a: Point, b: Point) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    #[test]
    fn a_child_joint_is_where_its_parents_chain_puts_it() {
        let m = model();
        let js = joints(&m, &Space::Drawing, &zero);
        let at = |n: &str| js.iter().find(|j| j.name == n).unwrap().at;
        assert!(near(at("Root"), [10.0, 10.0]));
        assert!(near(at("Arm"), [15.0, 10.0]));
        assert_eq!(
            js.iter()
                .find(|j| j.name == "Arm")
                .unwrap()
                .parent
                .as_deref(),
            Some("Root")
        );
    }

    #[test]
    fn the_nearest_joint_within_reach_is_picked() {
        let m = model();
        let js = joints(&m, &Space::Drawing, &zero);
        let near_root = screen([10.0, 10.0]);
        assert_eq!(
            pick_joint(&js, &MAPPER, [near_root[0] + 2.0, near_root[1]]).as_deref(),
            Some("Root")
        );
        assert_eq!(pick_joint(&js, &MAPPER, screen([50.0, 50.0])), None);
    }

    #[test]
    fn a_new_bone_takes_the_point_in_its_parents_space() {
        let m = model();
        let (name, (command, _)) = BoneTool::add_command(
            &m,
            &Space::Drawing,
            &MAPPER,
            screen([40.0, 30.0]),
            Some("Root"),
            &zero,
        )
        .unwrap();
        assert_eq!(name, "Bone1");
        let Command::AddBone { parent, base, .. } = &command else {
            panic!("an AddBone");
        };
        assert_eq!(parent.as_deref(), Some("Root"));
        assert!(
            near(base.translation, [30.0, 20.0]),
            "{:?}",
            base.translation
        );
        // A root takes the point itself.
        let (_, (command, _)) = BoneTool::add_command(
            &m,
            &Space::Drawing,
            &MAPPER,
            screen([40.0, 30.0]),
            None,
            &zero,
        )
        .unwrap();
        let Command::AddBone { parent, base, .. } = &command else {
            panic!("an AddBone");
        };
        assert!(parent.is_none() && near(base.translation, [40.0, 30.0]));
    }

    #[test]
    fn dragging_a_joint_moves_its_bone_and_a_short_press_does_nothing() {
        let m = model();
        let js = joints(&m, &Space::Drawing, &zero);
        let mut tool = BoneTool::default();
        let from = screen([15.0, 10.0]);
        assert!(tool.press_joint(&js, &MAPPER, from));
        tool.drag_to([from[0] + 1.0, from[1]]);
        assert!(
            tool.release_joint(&m, &Space::Drawing, &MAPPER, &zero)
                .is_none()
        );
        assert!(tool.press_joint(&js, &MAPPER, from));
        tool.drag_to(screen([30.0, 25.0]));
        let (command, _) = tool
            .release_joint(&m, &Space::Drawing, &MAPPER, &zero)
            .unwrap()
            .unwrap();
        let Command::SetBoneBase { name, base } = command else {
            panic!("a SetBoneBase");
        };
        assert_eq!(name, "Arm");
        assert!(near(base.translation, [20.0, 15.0]));
    }

    #[test]
    fn a_bone_with_keyforms_is_moved_at_its_key_and_refused_between_keys() {
        let mut m = model();
        m.apply(Command::BindParameter {
            parameter: "P".into(),
            elements: vec![tsukumoe_core::command::ElementRef::Bone { name: "Arm".into() }],
        })
        .unwrap();
        let (command, _) = set_command(&m, "Arm", base(9.0, 9.0), &zero).unwrap();
        assert!(matches!(command, Command::SetBoneKeyform { ref key, .. } if key == &[1]));
        m.apply(command).unwrap();
        let between = |_: ParameterId| 0.3;
        assert_eq!(
            set_command(&m, "Arm", base(1.0, 1.0), &between).unwrap_err(),
            ToolError::Bone("param-err-not-on-key")
        );
    }

    fn weights_with(entries: &[(&str, f32)]) -> Vec<(String, f32)> {
        entries.iter().map(|(b, w)| ((*b).to_owned(), *w)).collect()
    }

    fn brush(kind: BrushKind) -> Brush {
        Brush {
            radius: 20.0,
            strength: 1.0,
            kind,
        }
    }

    #[test]
    fn adding_raises_the_weight_in_reach_and_the_others_give_way() {
        let mut w: Weights = vec![weights_with(&[("Root", 1.0)]), Vec::new()];
        let vertices = [[0.0, 0.0], [90.0, 90.0]];
        for _ in 0..6 {
            paint(
                &mut w,
                "Arm",
                &vertices,
                &[],
                [0.0, 0.0],
                &brush(BrushKind::Add),
            );
        }
        assert!(weight_of(&w[0], "Arm") > 0.9);
        let total: f32 = w[0].iter().map(|(_, v)| v).sum();
        assert!(total <= 1.0 + 1e-4, "{total}");
        assert!(w[1].is_empty(), "out of reach");
    }

    #[test]
    fn subtracting_takes_the_weight_off_and_drops_the_bone_at_zero() {
        let mut w: Weights = vec![weights_with(&[("Arm", 0.5), ("Root", 0.3)])];
        for _ in 0..8 {
            paint(
                &mut w,
                "Arm",
                &[[0.0, 0.0]],
                &[],
                [0.0, 0.0],
                &brush(BrushKind::Subtract),
            );
        }
        assert_eq!(w[0], weights_with(&[("Root", 0.3)]));
    }

    #[test]
    fn smoothing_pulls_toward_the_neighbours() {
        let mut w: Weights = vec![
            Vec::new(),
            weights_with(&[("Arm", 1.0)]),
            weights_with(&[("Arm", 1.0)]),
        ];
        let neighbours = vec![vec![1, 2], vec![0], vec![0]];
        paint(
            &mut w,
            "Arm",
            &[[0.0, 0.0], [50.0, 50.0], [50.0, 50.0]],
            &neighbours,
            [0.0, 0.0],
            &brush(BrushKind::Smooth),
        );
        assert!(weight_of(&w[0], "Arm") > 0.0);
        assert_eq!(weight_of(&w[1], "Arm"), 1.0, "out of reach");
    }

    #[test]
    fn at_most_four_bones_stay_and_the_painted_one_is_not_the_one_dropped() {
        let mut list = weights_with(&[("A", 0.1), ("B", 0.2), ("C", 0.2), ("D", 0.2)]);
        set_weight(&mut list, "E", 0.2);
        assert_eq!(list.len(), 4);
        assert!(list.iter().any(|(b, _)| b == "E"));
        assert!(!list.iter().any(|(b, _)| b == "A"), "the weakest goes");
    }

    #[test]
    fn a_stroke_becomes_one_command_that_skins_the_mesh() {
        let mut m = model();
        let mut tool = BoneTool::default();
        let vertices: Vec<[f32; 2]> = [[10.0, 10.0], [20.0, 10.0], [10.0, 20.0], [90.0, 90.0]]
            .iter()
            .map(|&p| screen(p))
            .collect();
        for _ in 0..8 {
            tool.stamp(&m, "Skin", "Arm", &vertices, vertices[0]);
        }
        assert!(tool.stroke().is_some());
        let (command, _) = tool.release_stroke().unwrap();
        assert!(tool.stroke().is_none());
        m.apply(command).unwrap();
        let w = weights_of(&m, "Skin").unwrap();
        assert!(weight_of(&w[0], "Arm") > 0.9);
        assert!(w[3].is_empty());
        // The preview heat is the weight of the picked bone.
        assert!(heat(&w, "Arm")[0] > 0.9);
    }

    #[test]
    fn the_neighbours_follow_the_triangles() {
        let m = model();
        let n = neighbours_of(&m, "Skin");
        assert_eq!(n[0], vec![1, 2]);
        assert_eq!(n[3], vec![1, 2]);
    }

    #[test]
    fn the_tool_forgets_a_mesh_or_bone_that_is_gone() {
        let mut m = model();
        let mut tool = BoneTool::default();
        tool.follow(&m, Some("Skin".into()), Some("Arm".into()));
        assert_eq!(tool.bone.as_deref(), Some("Arm"));
        m.apply(Command::RemoveBone { name: "Arm".into() }).unwrap();
        tool.follow(&m, None, None);
        assert_eq!(tool.bone, None);
        assert_eq!(tool.mesh.as_deref(), Some("Skin"));
    }
}
