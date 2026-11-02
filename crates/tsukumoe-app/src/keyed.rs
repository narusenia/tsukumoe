// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Editing a shape that has keyforms (M5-06b). An element with keyforms is
//! edited at a key position: when every parameter of its binding sits on a
//! key (within a hair of its range), the edit goes into that keyform; any
//! other place is refused. An element without keyforms is edited as before.
//!
//! The tools keep making the base-shape commands (`SetMeshPositions`,
//! `SetWarpBasePoints`, `SetRotationBase`, `SetMeshOpacity`, ...);
//! [`retarget`] turns them, at the one door every edit passes, into the
//! keyform command for the key the sliders are on, or says why not.

use tsukumoe_core::command::{Command, ElementRef, RotationBase};
use tsukumoe_core::elements::{ParameterId, Point};
use tsukumoe_core::keyform::{Binding, KeyformGrid};
use tsukumoe_core::keyform_edit::sample_at;
use tsukumoe_core::model::Model;
use tsukumoe_core::rig::{MeshKeyform, RotationKeyform, WarpKeyform};

use crate::mesh_tool::Op;
use crate::scene::Preview;
use crate::view::ViewState;

/// A parameter value counts as on a key when it is this close, in fractions of its range.
const TOLERANCE: f32 = 1e-4;

/// The value each parameter has now: the slider, else the default. A copy,
/// so it can be held while the view is edited.
pub fn values(model: &Model, view: &ViewState) -> impl Fn(ParameterId) -> f32 + 'static {
    let map: std::collections::HashMap<ParameterId, f32> = model
        .parameters()
        .iter()
        .map(|(id, name, p)| (id, view.param(name, p)))
        .collect();
    move |id| map.get(&id).copied().unwrap_or(0.0)
}

/// Where the sliders are, for an element with keyforms.
#[derive(Clone, Debug, PartialEq)]
pub enum Place {
    /// On a key: one index per axis, the position in the dense grid, and a label
    /// such as `ParamAngleX = 30`.
    Key {
        key: Vec<usize>,
        flat: usize,
        label: String,
    },
    /// Not on a key on every axis; the label names the axes and their values.
    Off { label: String },
}

/// The key the sliders are on for `binding`, or where they are instead.
pub fn place(model: &Model, binding: &Binding, at: &dyn Fn(ParameterId) -> f32) -> Place {
    let mut key = Vec::new();
    let mut label = Vec::new();
    let mut on = true;
    for axis in binding.axes() {
        let value = at(axis.parameter);
        let (min, max) = model
            .parameters()
            .get(axis.parameter)
            .map_or((0.0, 1.0), |p| (p.min, p.max));
        let tolerance = (max - min).abs().max(1e-6) * TOLERANCE;
        let name = model.parameters().name(axis.parameter).unwrap_or("?");
        label.push(format!("{name} = {value:.2}"));
        match axis
            .keys
            .iter()
            .position(|k| (k - value).abs() <= tolerance)
        {
            Some(i) => key.push(i),
            None => on = false,
        }
    }
    let label = label.join(", ");
    if on {
        let flat = binding.index(&key).expect("a key on every axis");
        Place::Key { key, flat, label }
    } else {
        Place::Off { label }
    }
}

/// The key indices of the dense position `flat` on `binding`.
pub fn key_of(binding: &Binding, flat: usize) -> Vec<usize> {
    let mut rest = flat;
    let mut key = vec![0; binding.axes().len()];
    for (k, axis) in key.iter_mut().zip(binding.axes()).rev() {
        *k = rest % axis.keys.len();
        rest /= axis.keys.len();
    }
    key
}

fn grid_place<T>(
    model: &Model,
    grid: Option<&KeyformGrid<T>>,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Place> {
    let binding = model.bindings().get(grid?.binding)?;
    Some(place(model, binding, at))
}

/// Where the sliders are for a mesh, `None` if it has no keyforms.
pub fn mesh_place(model: &Model, name: &str, at: &dyn Fn(ParameterId) -> f32) -> Option<Place> {
    let m = model.meshes().get(model.meshes().find(name)?)?;
    grid_place(model, m.keyforms.as_ref(), at)
}

/// Where the sliders are for a deformer, `None` if it has no keyforms.
pub fn deformer_place(model: &Model, name: &str, at: &dyn Fn(ParameterId) -> f32) -> Option<Place> {
    if let Some(id) = model.warps().find(name) {
        return grid_place(model, model.warps().get(id)?.keyforms.as_ref(), at);
    }
    let id = model.rotations().find(name)?;
    grid_place(model, model.rotations().get(id)?.keyforms.as_ref(), at)
}

/// Where the sliders are for a mesh or a deformer, `None` if it has no keyforms.
pub fn element_place(
    model: &Model,
    element: &ElementRef,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Place> {
    match element {
        ElementRef::Mesh { name } => mesh_place(model, name, at),
        ElementRef::Deformer { name } => deformer_place(model, name, at),
    }
}

/// The vertices of a mesh to edit: the keyform at the key the sliders are on,
/// else the base shape (also off a key, where the edit will be refused).
pub fn mesh_stored(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Vec<Point>> {
    let m = model.meshes().get(model.meshes().find(name)?)?;
    Some(match (mesh_place(model, name, at), &m.keyforms) {
        (Some(Place::Key { flat, .. }), Some(g)) => g.keyforms[flat].positions.clone(),
        _ => m.positions.clone(),
    })
}

/// The control points of a warp as drawn at the sliders: what the keyforms
/// interpolate there (on a key, that keyform), else the base shape.
pub fn warp_stored(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<Vec<Point>> {
    let w = model.warps().get(model.warps().find(name)?)?;
    Some(match &w.keyforms {
        Some(g) => sample_at(model.bindings().get(g.binding)?, &g.keyforms, at).points,
        None => w.base_points.clone(),
    })
}

/// The shape of a rotation as drawn at the sliders (see [`warp_stored`]).
pub fn rotation_stored(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<RotationKeyform> {
    let r = model.rotations().get(model.rotations().find(name)?)?;
    Some(match &r.keyforms {
        Some(g) => sample_at(model.bindings().get(g.binding)?, &g.keyforms, at),
        None => r.base,
    })
}

/// Why an edit cannot go to an element that has keyforms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The sliders are not on a key; the label tells where they are.
    OffKey(String),
    /// The edit changes the shape of a mesh that has keyforms.
    ShapeChange(String),
}

impl Refusal {
    pub fn key(&self) -> &'static str {
        match self {
            Self::OffKey(_) => "keyform-off-key",
            Self::ShapeChange(_) => "keyform-shape-change",
        }
    }
}

fn mesh_keyform(model: &Model, name: &str, flat: usize) -> Option<MeshKeyform> {
    let m = model.meshes().get(model.meshes().find(name)?)?;
    Some(m.keyforms.as_ref()?.keyforms.get(flat)?.clone())
}

/// The command for the keyform the sliders are on, for a mesh edit that
/// replaces some of the keyform's fields.
fn mesh_edit(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
    change: impl FnOnce(&mut MeshKeyform),
) -> Result<Option<Command>, Refusal> {
    match mesh_place(model, name, at) {
        None => Ok(None),
        Some(Place::Off { label }) => Err(Refusal::OffKey(label)),
        Some(Place::Key { key, flat, .. }) => {
            let mut kf = mesh_keyform(model, name, flat).expect("a key of the grid");
            change(&mut kf);
            Ok(Some(Command::SetMeshKeyform {
                name: name.to_owned(),
                key,
                positions: kf.positions,
                opacity: kf.opacity,
                draw_order: kf.draw_order,
            }))
        }
    }
}

/// `command` aimed at the keyform the sliders are on, if it edits the shape of
/// an element that has keyforms; the same command otherwise. A `Batch` is
/// turned command by command.
pub fn retarget(
    model: &Model,
    at: &dyn Fn(ParameterId) -> f32,
    command: Command,
) -> Result<Command, Refusal> {
    match command {
        Command::Batch { commands } => Ok(Command::Batch {
            commands: commands
                .into_iter()
                .map(|c| retarget(model, at, c))
                .collect::<Result<_, _>>()?,
        }),
        Command::SetMeshPositions { name, positions } => {
            Ok(
                mesh_edit(model, &name, at, |k| k.positions = positions.clone())?
                    .unwrap_or(Command::SetMeshPositions { name, positions }),
            )
        }
        Command::SetMeshOpacity { name, opacity } => {
            Ok(mesh_edit(model, &name, at, |k| k.opacity = opacity)?
                .unwrap_or(Command::SetMeshOpacity { name, opacity }))
        }
        Command::SetMeshDrawOrder { name, draw_order } => {
            Ok(mesh_edit(model, &name, at, |k| k.draw_order = draw_order)?
                .unwrap_or(Command::SetMeshDrawOrder { name, draw_order }))
        }
        Command::SetMeshGeometry {
            name,
            positions,
            uvs,
            indices,
        } => {
            if mesh_place(model, &name, at).is_some() {
                return Err(Refusal::ShapeChange(name));
            }
            Ok(Command::SetMeshGeometry {
                name,
                positions,
                uvs,
                indices,
            })
        }
        Command::SetWarpBasePoints { name, points } => match deformer_place(model, &name, at) {
            None => Ok(Command::SetWarpBasePoints { name, points }),
            Some(Place::Off { label }) => Err(Refusal::OffKey(label)),
            Some(Place::Key { key, flat, .. }) => {
                let kf = warp_keyform(model, &name, flat).expect("a key of the grid");
                Ok(Command::SetWarpKeyform {
                    name,
                    key,
                    points,
                    opacity: kf.opacity,
                })
            }
        },
        Command::SetRotationBase { name, base } => match deformer_place(model, &name, at) {
            None => Ok(Command::SetRotationBase { name, base }),
            Some(Place::Off { label }) => Err(Refusal::OffKey(label)),
            Some(Place::Key { key, flat, .. }) => {
                let kf = rotation_keyform(model, &name, flat).expect("a key of the grid");
                Ok(Command::SetRotationKeyform {
                    name,
                    key,
                    base,
                    opacity: kf.opacity,
                })
            }
        },
        other => Ok(other),
    }
}

fn warp_keyform(model: &Model, name: &str, flat: usize) -> Option<WarpKeyform> {
    let w = model.warps().get(model.warps().find(name)?)?;
    Some(w.keyforms.as_ref()?.keyforms.get(flat)?.clone())
}

fn rotation_keyform(model: &Model, name: &str, flat: usize) -> Option<RotationKeyform> {
    let r = model.rotations().get(model.rotations().find(name)?)?;
    Some(*r.keyforms.as_ref()?.keyforms.get(flat)?)
}

/// A preview of a base-shape edit as the preview of the keyform the sliders
/// are on, if the element has keyforms and they are on a key; else unchanged.
pub fn lift_preview(model: &Model, at: &dyn Fn(ParameterId) -> f32, preview: Preview) -> Preview {
    match preview {
        Preview::Mesh { name, positions } => match mesh_place(model, &name, at) {
            Some(Place::Key { flat, .. }) => {
                let kf = mesh_keyform(model, &name, flat).expect("a key of the grid");
                Preview::MeshKeyform {
                    name,
                    key: flat,
                    positions,
                    opacity: kf.opacity,
                    draw_order: kf.draw_order,
                }
            }
            _ => Preview::Mesh { name, positions },
        },
        Preview::Warp { name, points } => match deformer_place(model, &name, at) {
            Some(Place::Key { flat, .. }) => {
                let kf = warp_keyform(model, &name, flat);
                Preview::WarpKeyform {
                    name,
                    key: flat,
                    points,
                    opacity: kf.map_or(1.0, |k| k.opacity),
                }
            }
            _ => Preview::Warp { name, points },
        },
        Preview::Rotation { name, base } => match deformer_place(model, &name, at) {
            Some(Place::Key { flat, .. }) => {
                let kf = rotation_keyform(model, &name, flat);
                Preview::RotationKeyform {
                    name,
                    key: flat,
                    base,
                    opacity: kf.map_or(1.0, |k| k.opacity),
                }
            }
            _ => Preview::Rotation { name, base },
        },
        other => other,
    }
}

/// The vertex opacity and draw order of a mesh as drawn now (interpolated
/// at the slider values when it has keyforms).
pub fn mesh_attrs(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
) -> Option<(f32, f32)> {
    let m = model.meshes().get(model.meshes().find(name)?)?;
    Some(match &m.keyforms {
        Some(g) => {
            let binding = model.bindings().get(g.binding)?;
            let kf = sample_at(binding, &g.keyforms, at);
            (kf.opacity, kf.draw_order)
        }
        None => (m.opacity, m.draw_order),
    })
}

/// The opacity of a deformer's keyform at the sliders; `None` without keyforms.
pub fn deformer_opacity(model: &Model, name: &str, at: &dyn Fn(ParameterId) -> f32) -> Option<f32> {
    if let Some(id) = model.warps().find(name) {
        let g = model.warps().get(id)?.keyforms.as_ref()?;
        let binding = model.bindings().get(g.binding)?;
        return Some(sample_at(binding, &g.keyforms, at).opacity);
    }
    let g = model
        .rotations()
        .get(model.rotations().find(name)?)?
        .keyforms
        .as_ref()?;
    let binding = model.bindings().get(g.binding)?;
    Some(sample_at(binding, &g.keyforms, at).opacity)
}

/// The command that sets a deformer keyform's opacity at the key the sliders
/// are on (refused off a key); `None` for a deformer without keyforms.
pub fn deformer_opacity_command(
    model: &Model,
    name: &str,
    at: &dyn Fn(ParameterId) -> f32,
    opacity: f32,
) -> Result<Option<Command>, Refusal> {
    match deformer_place(model, name, at) {
        None => Ok(None),
        Some(Place::Off { label }) => Err(Refusal::OffKey(label)),
        Some(Place::Key { key, flat, .. }) => {
            if let Some(kf) = warp_keyform(model, name, flat) {
                return Ok(Some(Command::SetWarpKeyform {
                    name: name.to_owned(),
                    key,
                    points: kf.points,
                    opacity,
                }));
            }
            let kf = rotation_keyform(model, name, flat).expect("a keyform of the grid");
            Ok(Some(Command::SetRotationKeyform {
                name: name.to_owned(),
                key,
                base: RotationBase::of(&kf),
                opacity,
            }))
        }
    }
}

/// An op with its command retargeted (see [`retarget`]).
pub fn retarget_op(
    model: &Model,
    at: &dyn Fn(ParameterId) -> f32,
    (command, label): Op,
) -> Result<Op, Refusal> {
    Ok((retarget(model, at, command)?, label))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::eval::Frame;

    /// A mesh and a warp, both bound to `P` (-1, 0, 1).
    fn model() -> Model {
        let mut m = Model::new();
        let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
        for c in [
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
                positions: pts.clone(),
                uvs: pts,
                indices: vec![0, 1, 2],
            },
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Part".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
            },
            Command::BindParameter {
                parameter: "P".into(),
                elements: vec![
                    ElementRef::Mesh {
                        name: "Skin".into(),
                    },
                    ElementRef::Deformer { name: "W".into() },
                ],
            },
        ] {
            m.apply(c).unwrap();
        }
        m
    }

    fn at(p: &Model, v: f32) -> impl Fn(ParameterId) -> f32 + 'static {
        let id = p.parameters().find("P").unwrap();
        move |i| if i == id { v } else { 0.0 }
    }

    fn drawn(m: &Model, v: f32) -> Vec<f32> {
        let compiled = m.compile().unwrap();
        let mut frame = Frame::new(&compiled);
        frame.set_parameter(0, v);
        frame.evaluate(&compiled);
        frame.positions().to_vec()
    }

    #[test]
    fn the_place_is_a_key_only_when_every_axis_is_on_one() {
        let m = model();
        let g = m.meshes().get(m.meshes().find("Skin").unwrap()).unwrap();
        let binding = m
            .bindings()
            .get(g.keyforms.as_ref().unwrap().binding)
            .unwrap();
        assert!(matches!(
            place(&m, binding, &at(&m, 1.0)),
            Place::Key { flat: 2, ref key, .. } if *key == [2]
        ));
        assert!(matches!(
            place(&m, binding, &at(&m, 0.99)),
            Place::Off { .. }
        ));
        let Place::Key { label, .. } = place(&m, binding, &at(&m, 0.0)) else {
            panic!("on a key");
        };
        assert_eq!(label, "P = 0.00");
        assert_eq!(key_of(binding, 1), [1]);
    }

    #[test]
    fn a_mesh_edit_goes_to_the_keyform_the_sliders_are_on_and_is_refused_elsewhere() {
        let mut m = model();
        let moved = vec![[5.0, 5.0], [6.0, 5.0], [5.0, 6.0]];
        let edit = Command::SetMeshPositions {
            name: "Skin".into(),
            positions: moved.clone(),
        };
        // Off a key: refused with where the sliders are.
        let Err(Refusal::OffKey(label)) = retarget(&m, &at(&m, 0.3), edit.clone()) else {
            panic!("off key");
        };
        assert_eq!(label, "P = 0.30");
        // On the key P = 1: the keyform command for key 2.
        let command = retarget(&m, &at(&m, 1.0), edit).unwrap();
        assert!(matches!(
            &command,
            Command::SetMeshKeyform { key, positions, .. } if *key == [2] && *positions == moved
        ));
        m.apply(command).unwrap();
        // Drawn at the key, and interpolated between the keys.
        assert_eq!(&drawn(&m, 1.0)[..2], &[5.0, 5.0]);
        assert!((drawn(&m, 0.5)[0] - 2.5).abs() < 1e-4);
        assert_eq!(&drawn(&m, 0.0)[..2], &[0.0, 0.0]);
        // The base shape did not change.
        let g = m.meshes().get(m.meshes().find("Skin").unwrap()).unwrap();
        assert_eq!(g.positions[0], [0.0, 0.0]);
    }

    #[test]
    fn opacity_and_draw_order_go_to_the_keyform_and_a_shape_change_is_refused() {
        let m = model();
        let c = retarget(
            &m,
            &at(&m, -1.0),
            Command::SetMeshOpacity {
                name: "Skin".into(),
                opacity: 0.4,
            },
        )
        .unwrap();
        assert!(matches!(
            c,
            Command::SetMeshKeyform { key, opacity, draw_order, .. }
                if key == [0] && opacity == 0.4 && draw_order == 500.0
        ));
        assert!(matches!(
            retarget(
                &m,
                &at(&m, 0.2),
                Command::SetMeshDrawOrder {
                    name: "Skin".into(),
                    draw_order: 1.0,
                }
            ),
            Err(Refusal::OffKey(_))
        ));
        let geometry = Command::SetMeshGeometry {
            name: "Skin".into(),
            positions: vec![[0.0, 0.0]; 4],
            uvs: vec![[0.0, 0.0]; 4],
            indices: vec![0, 1, 2],
        };
        assert_eq!(
            retarget(&m, &at(&m, 1.0), geometry),
            Err(Refusal::ShapeChange("Skin".into()))
        );
    }

    #[test]
    fn a_warp_edit_and_a_batch_follow_the_same_rule_and_unkeyed_shapes_pass_through() {
        let mut m = model();
        let points = vec![[0.0, 0.0], [3.0, 0.0], [0.0, 1.0], [3.0, 1.0]];
        let edit = Command::SetWarpBasePoints {
            name: "W".into(),
            points: points.clone(),
        };
        let batch = Command::Batch {
            commands: vec![edit.clone()],
        };
        assert!(retarget(&m, &at(&m, 0.5), batch.clone()).is_err());
        let Command::Batch { commands } = retarget(&m, &at(&m, 1.0), batch).unwrap() else {
            panic!("a batch");
        };
        assert!(matches!(
            &commands[0],
            Command::SetWarpKeyform { key, .. } if *key == [2]
        ));
        m.apply(commands[0].clone()).unwrap();
        // A mesh with no keyforms is passed through, wherever the sliders are.
        let mut plain = Model::new();
        for c in [
            Command::AddPart {
                name: "Part".into(),
                parent: None,
            },
            Command::AddMesh {
                name: "M".into(),
                part: "Part".into(),
                positions: vec![[0.0, 0.0]; 3],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            },
        ] {
            plain.apply(c).unwrap();
        }
        let positions = Command::SetMeshPositions {
            name: "M".into(),
            positions: vec![[1.0, 1.0]; 3],
        };
        assert_eq!(retarget(&plain, &|_| 0.3, positions.clone()), Ok(positions));
    }

    #[test]
    fn a_preview_of_a_keyed_shape_becomes_the_preview_of_its_keyform() {
        let m = model();
        let p = lift_preview(
            &m,
            &at(&m, 1.0),
            Preview::Warp {
                name: "W".into(),
                points: vec![[0.0, 0.0]; 4],
            },
        );
        assert!(matches!(p, Preview::WarpKeyform { key: 2, .. }));
        // Off a key (or unkeyed) it stays a base preview.
        let off = lift_preview(
            &m,
            &at(&m, 0.4),
            Preview::Mesh {
                name: "Skin".into(),
                positions: vec![[0.0, 0.0]; 3],
            },
        );
        assert!(matches!(off, Preview::Mesh { .. }));
    }

    #[test]
    fn the_scene_shows_a_keyform_preview_and_puts_it_back() {
        use crate::scene::Scene;
        let m = model();
        let mut scene = Scene::new(&m).unwrap();
        let id = m.parameters().find("P").unwrap();
        let _ = id;
        let mut view = ViewState::default();
        let p = m
            .parameters()
            .get(m.parameters().find("P").unwrap())
            .unwrap()
            .clone();
        view.set_param("P", &p, 1.0);
        scene.apply(&view);
        scene.advance(0.0);
        let before = scene.drawn_positions("Skin").unwrap();
        let preview = Preview::MeshKeyform {
            name: "Skin".into(),
            key: 2,
            positions: vec![[9.0, 9.0], [10.0, 9.0], [9.0, 10.0]],
            opacity: 1.0,
            draw_order: 500.0,
        };
        assert!(scene.set_previews(&[preview]));
        scene.advance(0.0);
        assert_eq!(scene.drawn_positions("Skin").unwrap()[0], [9.0, 9.0]);
        scene.set_previews(&[]);
        scene.advance(0.0);
        assert_eq!(scene.drawn_positions("Skin").unwrap(), before);
    }

    /// A keyed warp, dragged by the deformer tool with proportional edit on, lands in its keyform.
    #[test]
    fn the_deformer_tool_edits_a_keyform_with_proportional_edit_and_undo_restores_it() {
        use crate::deformer_tool::{DeformerTool, Shape};
        use crate::mesh_tool::Mapper;
        let mut m = Model::new();
        let mut grid = Vec::new();
        for r in 0..3 {
            for c in 0..3 {
                grid.push([c as f32, r as f32]);
            }
        }
        for c in [
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
            Command::AddWarpDeformer {
                name: "W".into(),
                part: "Part".into(),
                parent: None,
                rows: 2,
                columns: 2,
                base_points: grid,
            },
            Command::BindParameter {
                parameter: "P".into(),
                elements: vec![ElementRef::Deformer { name: "W".into() }],
            },
        ] {
            m.apply(c).unwrap();
        }
        let before = m
            .warps()
            .get(m.warps().find("W").unwrap())
            .unwrap()
            .keyforms
            .clone();
        let mapper = Mapper {
            origin: [0.0, 0.0],
            size: [400.0, 400.0],
            view: [-0.5, -0.5, 2.5, 2.5],
        };
        let at_key = at(&m, 1.0);
        let shape = Shape::of_at(&m, "W", 0.1, &at_key).unwrap();
        let mut tool = DeformerTool::default();
        tool.set_deformer(Some("W"));
        tool.proportional.on = true;
        tool.proportional.radius = 200.0;
        // Drag the centre point (1, 1) up.
        let start = mapper.to_screen([1.0, 1.0]);
        tool.press(&shape, &mapper, start, false);
        tool.drag([start[0], start[1] - 40.0]);
        let (command, label) = tool.release("W", &shape, &mapper).unwrap();
        assert_eq!(label, "deformer_move_points: W");
        // Off the key it is refused; on it, it is the keyform command for key 2.
        assert!(retarget(&m, &at(&m, 0.4), command.clone()).is_err());
        let keyed = retarget(&m, &at_key, command).unwrap();
        m.apply(keyed).unwrap();
        let w = m.warps().get(m.warps().find("W").unwrap()).unwrap();
        let kfs = &w.keyforms.as_ref().unwrap().keyforms;
        // The centre and its neighbours moved in key 2 only.
        assert!(kfs[2].points[4][1] > 1.0);
        assert!(
            kfs[2].points[1][1] > 0.0,
            "a neighbour follows by its weight"
        );
        assert_eq!(kfs[0].points, kfs[1].points);
        assert_eq!(kfs[0].points[4], [1.0, 1.0]);
        assert_eq!(w.base_points[4], [1.0, 1.0]);
        assert!(m.undo());
        let w = m.warps().get(m.warps().find("W").unwrap()).unwrap();
        assert_eq!(w.keyforms, before);
    }

    /// A keyed mesh dragged by the mesh tool lands in its keyform; at another key it is separate.
    #[test]
    fn the_mesh_tool_edits_a_keyform_and_only_that_key_changes() {
        use crate::mesh_tool::{Mapper, MeshTool};
        use tsukumoe_core::deformer_space::Space;
        let mut m = model();
        let mapper = Mapper {
            origin: [0.0, 0.0],
            size: [100.0, 100.0],
            view: [0.0, 0.0, 1.0, 1.0],
        };
        let at_key = at(&m, 1.0);
        let stored = mesh_stored(&m, "Skin", &at_key).unwrap();
        let g = m.meshes().get(m.meshes().find("Skin").unwrap()).unwrap();
        let (indices, shown) = (g.indices.clone(), stored.clone());
        let mut tool = MeshTool::default();
        tool.set_mesh(Some("Skin"));
        tool.click(&shown, &indices, &mapper, mapper.to_screen(shown[1]), false);
        tool.press(&shown, &indices, &mapper, mapper.to_screen(shown[1]), false);
        let to = mapper.to_screen([1.0, 0.5]);
        tool.drag(to);
        let (command, _) = tool
            .release_in("Skin", &stored, &shown, &mapper, &Space::Drawing)
            .unwrap();
        m.apply(retarget(&m, &at_key, command).unwrap()).unwrap();
        let moved = drawn(&m, 1.0);
        assert!((moved[3] - 0.5).abs() < 1e-4, "{moved:?}");
        // The same vertex at the key P = -1 is where it was.
        assert!(drawn(&m, -1.0)[3].abs() < 1e-4);
    }
}
