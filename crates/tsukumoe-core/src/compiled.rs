// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The read-only evaluation model (ADR-0020).
//!
//! Every element is flattened into contiguous buffers addressed by offsets.
//! Coordinates are interleaved `x0, y0, x1, y1, …`. Indices below are
//! positions in the iteration order of the editing model's tables.

use std::collections::HashMap;

use crate::affine::Affine;
use crate::elements::{ParameterId, Point};
use crate::expr::{self, ExprError, Op};
use crate::keyform::BindingId;
use crate::model::Model;
use crate::physics::{Channel, Range};
use crate::rig::{
    BlendMode, BlendTarget, BoneId, BoneTransform, DeformerRef, GlueNode, RotationKeyform,
};
use crate::validate::ModelError;

#[derive(Debug, Clone, PartialEq)]
pub enum CompileError {
    /// Only a consistent model compiles.
    Invalid(Vec<ModelError>),
    Expression {
        driver: String,
        error: ExprError,
    },
    /// Two drivers write the same parameter.
    DuplicateDriverOutput {
        driver: String,
    },
    /// The drivers read each other's outputs in a loop.
    DriverCycle {
        driver: String,
    },
    /// The bone's rest transform cannot be inverted (a scale of 0).
    DegenerateBone {
        bone: String,
    },
    /// Meshes with different parent deformers share the bone's tree (ADR-0021).
    BoneSpaceMismatch {
        bone: String,
    },
    /// A driver also writes a parameter that physics writes (ADR-0022).
    PhysicsOutputConflict {
        group: String,
    },
}

/// A driver's instructions in `CompiledModel::ops`, and the parameter it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DriverDef {
    pub output: usize,
    pub start: usize,
    pub len: usize,
}

/// The deformer an element hangs from, as an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentRef {
    None,
    Warp(usize),
    Rotation(usize),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamDef {
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub repeat: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AxisDef {
    pub parameter: usize,
    pub keys: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BindingDef {
    pub axes: Vec<AxisDef>,
    pub keyform_count: usize,
    /// Where this binding's corner scratch starts in a frame, and its size.
    pub corner_start: usize,
    pub max_corners: usize,
}

/// A keyform grid: which binding weights it and the index of its first
/// keyform in the element kind's keyform array.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyformRef {
    pub binding: usize,
    pub first: usize,
}

/// Opacity, draw order, multiply color, screen color.
pub type MeshAttrs = [f32; 8];

#[derive(Debug, Clone, PartialEq)]
pub struct BoneDef {
    pub parent: Option<usize>,
    pub base: BoneTransform,
    pub keyforms: Option<KeyformRef>,
    /// Inverse of the rest transform through the bone hierarchy.
    pub rest_inverse: Affine,
}

pub const NO_BONE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsInputDef {
    pub parameter: usize,
    pub channel: Channel,
    /// Percent.
    pub weight: f32,
    pub reflect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsOutputDef {
    pub parameter: usize,
    pub channel: Channel,
    /// Index within the group's particles.
    pub particle: usize,
    pub scale: f32,
    /// Percent.
    pub weight: f32,
    pub reflect: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhysicsKind {
    Pendulum { position: Range, angle: Range },
    Chain { gravity: Point, wind: Point },
}

/// One pendulum particle or chain joint. Fields a kind does not use are 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhysicsParticle {
    pub rest: Point,
    /// Distance to the parent: a pendulum's `radius`, a chain's joint length.
    pub length: f32,
    pub mobility: f32,
    pub delay: f32,
    pub acceleration: f32,
    pub stiffness: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsDef {
    pub name: String,
    pub kind: PhysicsKind,
    pub input_start: usize,
    pub input_count: usize,
    pub output_start: usize,
    pub output_count: usize,
    pub particle_start: usize,
    pub particle_count: usize,
}

/// Up to four bones moving one vertex; unused slots are `NO_BONE` with weight 0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Influence {
    pub bones: [u32; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct MeshDef {
    /// First vertex in the shared vertex buffer (a float offset is twice this).
    pub vertex_start: usize,
    pub vertex_count: usize,
    pub parent: ParentRef,
    pub part: usize,
    pub base_attrs: MeshAttrs,
    pub keyforms: Option<KeyformRef>,
    /// Float offset of keyform 0's positions in `mesh_kf_positions`.
    pub kf_positions: usize,
    pub blends: Vec<usize>,
    /// First index in `mesh_indices`. Indices are local to the mesh.
    pub index_start: usize,
    pub index_count: usize,
    pub texture: u32,
    pub blend_mode: BlendMode,
    /// Meshes clipping this one: `mesh_masks[mask_start..mask_start + mask_count]`.
    pub mask_start: usize,
    pub mask_count: usize,
    pub mask_inverted: bool,
    pub double_sided: bool,
    /// First entry in `influences`; one per vertex. `None` when no bone moves it.
    pub influence_start: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarpDef {
    pub rows: usize,
    pub columns: usize,
    pub parent: ParentRef,
    /// Float offset of this warp's control points; the same offset addresses
    /// its slice of the frame's world grid.
    pub point_start: usize,
    pub point_count: usize,
    pub base_opacity: f32,
    pub keyforms: Option<KeyformRef>,
    /// Float offset of keyform 0's points in `warp_kf_points`.
    pub kf_points: usize,
    pub blends: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RotationDef {
    pub parent: ParentRef,
    pub base: RotationKeyform,
    pub keyforms: Option<KeyformRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GlueDef {
    pub mesh_a: usize,
    pub mesh_b: usize,
    pub node_start: usize,
    pub node_count: usize,
    pub intensity: f32,
    pub keyforms: Option<KeyformRef>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendTargetRef {
    Mesh(usize),
    Warp(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlendDef {
    pub target: BlendTargetRef,
    pub parameter: usize,
    pub keys: Vec<f32>,
    pub weights: Vec<f32>,
    /// Float offset in `blend_deltas`.
    pub delta_start: usize,
}

#[derive(Debug, Clone, Default)]
pub struct CompiledModel {
    pub parameter_names: Vec<String>,
    pub parameters: Vec<ParamDef>,
    pub bindings: Vec<BindingDef>,
    /// Total corner scratch a frame needs.
    pub corner_capacity: usize,

    /// Run in this order: a driver comes after the drivers whose outputs it reads.
    pub drivers: Vec<DriverDef>,
    pub ops: Vec<Op>,

    /// Parents come before children.
    pub part_parents: Vec<Option<usize>>,
    pub part_opacity: Vec<f32>,

    pub mesh_names: Vec<String>,
    pub meshes: Vec<MeshDef>,
    pub mesh_base_positions: Vec<f32>,
    /// `u, v` interleaved, one pair per vertex of `mesh_base_positions`.
    pub mesh_uvs: Vec<f32>,
    pub mesh_indices: Vec<u32>,
    /// Mesh indices.
    pub mesh_masks: Vec<u32>,
    pub mesh_kf_positions: Vec<f32>,
    pub mesh_kf_attrs: Vec<MeshAttrs>,

    pub warps: Vec<WarpDef>,
    pub warp_base_points: Vec<f32>,
    pub warp_kf_points: Vec<f32>,
    pub warp_kf_opacity: Vec<f32>,

    pub rotations: Vec<RotationDef>,
    pub rotation_kf: Vec<RotationKeyform>,

    /// Warps and rotations, parents first.
    pub deformer_order: Vec<ParentRef>,

    /// Parents come before children.
    pub bone_names: Vec<String>,
    pub bones: Vec<BoneDef>,
    pub bone_kf: Vec<BoneTransform>,
    pub influences: Vec<Influence>,

    pub physics: Vec<PhysicsDef>,
    pub physics_inputs: Vec<PhysicsInputDef>,
    pub physics_outputs: Vec<PhysicsOutputDef>,
    pub physics_particles: Vec<PhysicsParticle>,

    pub glues: Vec<GlueDef>,
    pub glue_nodes: Vec<GlueNode>,
    pub glue_kf_intensity: Vec<f32>,

    pub blends: Vec<BlendDef>,
    pub blend_deltas: Vec<f32>,
}

impl CompiledModel {
    pub fn parameter_index(&self, name: &str) -> Option<usize> {
        self.parameter_names.iter().position(|n| n == name)
    }

    pub fn mesh_index(&self, name: &str) -> Option<usize> {
        self.mesh_names.iter().position(|n| n == name)
    }

    pub fn vertex_count(&self) -> usize {
        self.mesh_base_positions.len() / 2
    }
}

fn flatten(points: &[[f32; 2]], out: &mut Vec<f32>) {
    out.extend(points.iter().flatten());
}

impl Model {
    /// Flattens the model for evaluation. Physics is compiled too, but
    /// its state lives in the frame.
    pub fn compile(&self) -> Result<CompiledModel, CompileError> {
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(CompileError::Invalid(errors));
        }
        let mut c = CompiledModel::default();

        let mut param_index = HashMap::new();
        for (i, (id, name, p)) in self.parameters.iter().enumerate() {
            param_index.insert(id, i);
            c.parameter_names.push(name.to_owned());
            c.parameters.push(ParamDef {
                min: p.min,
                max: p.max,
                default: p.default,
                repeat: p.repeat,
            });
        }

        compile_drivers(self, &param_index, &mut c)?;
        compile_physics(self, &param_index, &mut c)?;

        let mut binding_index: HashMap<BindingId, usize> = HashMap::new();
        for (i, (id, binding)) in self.bindings.iter().enumerate() {
            binding_index.insert(id, i);
            let axes: Vec<AxisDef> = binding
                .axes()
                .iter()
                .map(|a| AxisDef {
                    parameter: param_index[&a.parameter],
                    keys: a.keys.clone(),
                })
                .collect();
            let max_corners = axes.iter().map(|a| a.keys.len().min(2)).product();
            c.bindings.push(BindingDef {
                axes,
                keyform_count: binding.keyform_count(),
                corner_start: c.corner_capacity,
                max_corners,
            });
            c.corner_capacity += max_corners;
        }

        // Parts, parents first.
        let mut part_ids: Vec<_> = self.parts.iter().map(|(id, _, _)| id).collect();
        let part_depth = |mut id| {
            let mut depth = 0;
            while let Some(parent) = self.parts.get(id).and_then(|p| p.parent) {
                depth += 1;
                id = parent;
            }
            depth
        };
        part_ids.sort_by_key(|&id| part_depth(id));
        let part_index: HashMap<_, _> = part_ids
            .iter()
            .enumerate()
            .map(|(i, &id)| (id, i))
            .collect();
        for &id in &part_ids {
            let part = self.parts.get(id).expect("listed part exists");
            c.part_parents
                .push(part.parent.map(|parent| part_index[&parent]));
            c.part_opacity.push(part.opacity);
        }

        let mesh_index: HashMap<_, _> = self
            .meshes
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| (id, i))
            .collect();
        let warp_index: HashMap<_, _> = self
            .warps
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| (id, i))
            .collect();
        let rotation_index: HashMap<_, _> = self
            .rotations
            .iter()
            .enumerate()
            .map(|(i, (id, _, _))| (id, i))
            .collect();
        let parent_ref = |parent: Option<DeformerRef>| match parent {
            None => ParentRef::None,
            Some(DeformerRef::Warp(id)) => ParentRef::Warp(warp_index[&id]),
            Some(DeformerRef::Rotation(id)) => ParentRef::Rotation(rotation_index[&id]),
        };
        let keyform_ref = |grid_binding: BindingId, first: usize| KeyformRef {
            binding: binding_index[&grid_binding],
            first,
        };

        let bone_index = compile_bones(self, &binding_index, &mut c)?;

        for (_, name, mesh) in self.meshes.iter() {
            let vertex_start = c.mesh_base_positions.len() / 2;
            flatten(&mesh.positions, &mut c.mesh_base_positions);
            flatten(&mesh.uvs, &mut c.mesh_uvs);
            let index_start = c.mesh_indices.len();
            c.mesh_indices.extend(&mesh.indices);
            let mask_start = c.mesh_masks.len();
            c.mesh_masks
                .extend(mesh.masks.iter().map(|m| mesh_index[m] as u32));
            let base_attrs = attrs(
                mesh.opacity,
                mesh.draw_order,
                mesh.multiply_color,
                mesh.screen_color,
            );
            let mut keyforms = None;
            let kf_positions = c.mesh_kf_positions.len();
            if let Some(grid) = &mesh.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.mesh_kf_attrs.len()));
                for kf in &grid.keyforms {
                    flatten(&kf.positions, &mut c.mesh_kf_positions);
                    c.mesh_kf_attrs.push(attrs(
                        kf.opacity,
                        kf.draw_order,
                        kf.multiply_color,
                        kf.screen_color,
                    ));
                }
            }
            let influence_start = (!mesh.influences.is_empty()).then(|| {
                let start = c.influences.len();
                c.influences.extend(mesh.influences.iter().map(|inf| {
                    let mut out = Influence {
                        bones: [NO_BONE; 4],
                        weights: [0.0; 4],
                    };
                    for (slot, bone) in inf.bones.iter().enumerate() {
                        if let Some(b) = bone {
                            out.bones[slot] = bone_index[b] as u32;
                            out.weights[slot] = inf.weights[slot];
                        }
                    }
                    out
                }));
                start
            });
            c.mesh_names.push(name.to_owned());
            c.meshes.push(MeshDef {
                vertex_start,
                vertex_count: mesh.positions.len(),
                parent: parent_ref(mesh.parent),
                part: part_index[&mesh.part],
                base_attrs,
                keyforms,
                kf_positions,
                blends: Vec::new(),
                index_start,
                index_count: mesh.indices.len(),
                texture: mesh.texture,
                blend_mode: mesh.blend_mode,
                mask_start,
                mask_count: mesh.masks.len(),
                mask_inverted: mesh.mask_inverted,
                double_sided: mesh.double_sided,
                influence_start,
            });
        }

        for (_, _, warp) in self.warps.iter() {
            let point_start = c.warp_base_points.len();
            flatten(&warp.base_points, &mut c.warp_base_points);
            let kf_points = c.warp_kf_points.len();
            let mut keyforms = None;
            if let Some(grid) = &warp.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.warp_kf_opacity.len()));
                for kf in &grid.keyforms {
                    flatten(&kf.points, &mut c.warp_kf_points);
                    c.warp_kf_opacity.push(kf.opacity);
                }
            }
            c.warps.push(WarpDef {
                rows: warp.rows as usize,
                columns: warp.columns as usize,
                parent: parent_ref(warp.parent),
                point_start,
                point_count: warp.base_points.len(),
                base_opacity: 1.0,
                keyforms,
                kf_points,
                blends: Vec::new(),
            });
        }

        for (_, _, rotation) in self.rotations.iter() {
            let mut keyforms = None;
            if let Some(grid) = &rotation.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.rotation_kf.len()));
                c.rotation_kf.extend(grid.keyforms.iter().copied());
            }
            c.rotations.push(RotationDef {
                parent: parent_ref(rotation.parent),
                base: rotation.base,
                keyforms,
            });
        }

        for (_, _, glue) in self.glues.iter() {
            let node_start = c.glue_nodes.len();
            c.glue_nodes.extend(glue.nodes.iter().copied());
            let mut keyforms = None;
            if let Some(grid) = &glue.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.glue_kf_intensity.len()));
                c.glue_kf_intensity.extend(grid.keyforms.iter().copied());
            }
            c.glues.push(GlueDef {
                mesh_a: mesh_index[&glue.mesh_a],
                mesh_b: mesh_index[&glue.mesh_b],
                node_start,
                node_count: glue.nodes.len(),
                intensity: glue.intensity,
                keyforms,
            });
        }

        for (_, _, shape) in self.blend_shapes.iter() {
            let index = c.blends.len();
            let target = match shape.target {
                BlendTarget::Mesh(id) => {
                    let m = mesh_index[&id];
                    c.meshes[m].blends.push(index);
                    BlendTargetRef::Mesh(m)
                }
                BlendTarget::Warp(id) => {
                    let w = warp_index[&id];
                    c.warps[w].blends.push(index);
                    BlendTargetRef::Warp(w)
                }
            };
            let delta_start = c.blend_deltas.len();
            flatten(&shape.deltas, &mut c.blend_deltas);
            c.blends.push(BlendDef {
                target,
                parameter: param_index[&shape.parameter],
                keys: shape.keys.clone(),
                weights: shape.weights.clone(),
                delta_start,
            });
        }

        // Deformers, parents first: sort by depth in the parent chain.
        let depth = |mut parent: ParentRef| {
            let mut depth = 0;
            loop {
                parent = match parent {
                    ParentRef::None => return depth,
                    ParentRef::Warp(i) => c.warps[i].parent,
                    ParentRef::Rotation(i) => c.rotations[i].parent,
                };
                depth += 1;
            }
        };
        let mut order: Vec<ParentRef> = (0..c.warps.len())
            .map(ParentRef::Warp)
            .chain((0..c.rotations.len()).map(ParentRef::Rotation))
            .collect();
        order.sort_by_key(|&d| depth(d));
        c.deformer_order = order;

        check_bone_spaces(&c)?;

        Ok(c)
    }
}

fn compile_physics(
    model: &Model,
    param_index: &HashMap<ParameterId, usize>,
    c: &mut CompiledModel,
) -> Result<(), CompileError> {
    let rig = &model.physics;
    let ios = |c: &mut CompiledModel,
               inputs: &[crate::physics::PhysicsInput],
               outputs: &[crate::physics::PhysicsOutput]| {
        let start = (c.physics_inputs.len(), c.physics_outputs.len());
        c.physics_inputs
            .extend(inputs.iter().map(|i| PhysicsInputDef {
                parameter: param_index[&i.parameter],
                channel: i.channel,
                weight: i.weight,
                reflect: i.reflect,
            }));
        c.physics_outputs
            .extend(outputs.iter().map(|o| PhysicsOutputDef {
                parameter: param_index[&o.parameter],
                channel: o.channel,
                particle: o.particle,
                scale: o.scale,
                weight: o.weight,
                reflect: o.reflect,
            }));
        start
    };

    for group in &rig.pendulums {
        let (input_start, output_start) = ios(c, &group.inputs, &group.outputs);
        let particle_start = c.physics_particles.len();
        c.physics_particles
            .extend(group.particles.iter().map(|p| PhysicsParticle {
                rest: p.position,
                length: p.radius,
                mobility: p.mobility,
                delay: p.delay,
                acceleration: p.acceleration,
                stiffness: 0.0,
                damping: 0.0,
            }));
        c.physics.push(PhysicsDef {
            name: group.name.clone(),
            kind: PhysicsKind::Pendulum {
                position: group.position_range,
                angle: group.angle_range,
            },
            input_start,
            input_count: group.inputs.len(),
            output_start,
            output_count: group.outputs.len(),
            particle_start,
            particle_count: group.particles.len(),
        });
    }
    for chain in &rig.chains {
        let (input_start, output_start) = ios(c, &chain.inputs, &chain.outputs);
        let particle_start = c.physics_particles.len();
        c.physics_particles
            .extend(chain.joints.iter().map(|j| PhysicsParticle {
                rest: [0.0, 0.0],
                length: j.length,
                mobility: 0.0,
                delay: 0.0,
                acceleration: 0.0,
                stiffness: j.stiffness,
                damping: j.damping,
            }));
        c.physics.push(PhysicsDef {
            name: chain.name.clone(),
            kind: PhysicsKind::Chain {
                gravity: chain.gravity,
                wind: chain.wind,
            },
            input_start,
            input_count: chain.inputs.len(),
            output_start,
            output_count: chain.outputs.len(),
            particle_start,
            particle_count: chain.joints.len(),
        });
    }

    for def in &c.physics {
        let outputs = &c.physics_outputs[def.output_start..def.output_start + def.output_count];
        if outputs
            .iter()
            .any(|o| c.drivers.iter().any(|d| d.output == o.parameter))
        {
            return Err(CompileError::PhysicsOutputConflict {
                group: def.name.clone(),
            });
        }
    }
    Ok(())
}

/// Bones, parents first, with the inverse of each rest transform.
fn compile_bones(
    model: &Model,
    binding_index: &HashMap<BindingId, usize>,
    c: &mut CompiledModel,
) -> Result<HashMap<BoneId, usize>, CompileError> {
    let depth = |mut id: BoneId| {
        let mut depth = 0;
        while let Some(parent) = model.bones.get(id).and_then(|b| b.parent) {
            depth += 1;
            id = parent;
        }
        depth
    };
    let mut ids: Vec<_> = model.bones.iter().map(|(id, _, _)| id).collect();
    ids.sort_by_key(|&id| depth(id));
    let index: HashMap<_, _> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();

    let mut rest_world: Vec<Affine> = Vec::new();
    for &id in &ids {
        let bone = model.bones.get(id).expect("listed bone exists");
        let name = model.bones.name(id).expect("listed bone exists");
        let local = Affine::from_bone(&bone.base);
        let world = match bone.parent {
            Some(p) => rest_world[index[&p]].then_after(&local),
            None => local,
        };
        let rest_inverse = world
            .inverse()
            .ok_or_else(|| CompileError::DegenerateBone {
                bone: name.to_owned(),
            })?;
        rest_world.push(world);
        let keyforms = bone.keyforms.as_ref().map(|grid| {
            let k = KeyformRef {
                binding: binding_index[&grid.binding],
                first: c.bone_kf.len(),
            };
            c.bone_kf.extend(grid.keyforms.iter().copied());
            k
        });
        c.bone_names.push(name.to_owned());
        c.bones.push(BoneDef {
            parent: bone.parent.map(|p| index[&p]),
            base: bone.base,
            keyforms,
            rest_inverse,
        });
    }
    Ok(index)
}

/// Every mesh using a bone tree must hang from the same parent (ADR-0021).
fn check_bone_spaces(c: &CompiledModel) -> Result<(), CompileError> {
    let root_of = |mut b: usize| {
        while let Some(p) = c.bones[b].parent {
            b = p;
        }
        b
    };
    let mut space: HashMap<usize, ParentRef> = HashMap::new();
    for def in &c.meshes {
        let Some(start) = def.influence_start else {
            continue;
        };
        let used = c.influences[start..start + def.vertex_count]
            .iter()
            .flat_map(|inf| inf.bones)
            .filter(|&b| b != NO_BONE);
        for b in used {
            let b = b as usize;
            if *space.entry(root_of(b)).or_insert(def.parent) != def.parent {
                return Err(CompileError::BoneSpaceMismatch {
                    bone: c.bone_names[b].clone(),
                });
            }
        }
    }
    Ok(())
}

/// Compiles every driver's expression and orders the drivers so each runs
/// after the drivers that write the parameters it reads.
fn compile_drivers(
    model: &Model,
    param_index: &HashMap<ParameterId, usize>,
    c: &mut CompiledModel,
) -> Result<(), CompileError> {
    let by_name: HashMap<&str, usize> = c
        .parameter_names
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();

    let mut defs = Vec::new();
    let mut names = Vec::new();
    let mut writer: HashMap<usize, usize> = HashMap::new();
    for (_, name, driver) in model.drivers.iter() {
        let output = param_index[&driver.output];
        if writer.insert(output, defs.len()).is_some() {
            return Err(CompileError::DuplicateDriverOutput {
                driver: name.to_owned(),
            });
        }
        let start = c.ops.len();
        expr::compile(&driver.expression, |n| by_name.get(n).copied(), &mut c.ops).map_err(
            |error| CompileError::Expression {
                driver: name.to_owned(),
                error,
            },
        )?;
        defs.push(DriverDef {
            output,
            start,
            len: c.ops.len() - start,
        });
        names.push(name);
    }

    // shortcut: quadratic ordering, fine for the tens of drivers a rig has;
    // upgrade to Kahn's algorithm if models grow to thousands.
    let mut done = vec![false; defs.len()];
    while c.drivers.len() < defs.len() {
        let before = c.drivers.len();
        for (i, def) in defs.iter().enumerate() {
            let ready = !done[i]
                && c.ops[def.start..def.start + def.len]
                    .iter()
                    .all(|op| match op {
                        Op::Param(p) => writer.get(p).is_none_or(|&w| done[w]),
                        _ => true,
                    });
            if ready {
                done[i] = true;
                c.drivers.push(*def);
            }
        }
        if c.drivers.len() == before {
            let stuck = done.iter().position(|d| !d).expect("a driver is left");
            return Err(CompileError::DriverCycle {
                driver: names[stuck].to_owned(),
            });
        }
    }
    Ok(())
}

fn attrs(opacity: f32, draw_order: f32, multiply: [f32; 3], screen: [f32; 3]) -> MeshAttrs {
    [
        opacity,
        draw_order,
        multiply[0],
        multiply[1],
        multiply[2],
        screen[0],
        screen[1],
        screen[2],
    ]
}
