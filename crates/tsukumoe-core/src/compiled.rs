// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The read-only evaluation model (ADR-0020).
//!
//! Every element is flattened into contiguous buffers addressed by offsets.
//! Coordinates are interleaved `x0, y0, x1, y1, …`. Indices below are
//! positions in the iteration order of the editing model's tables.

use std::borrow::Cow;
use std::collections::HashMap;

use bytemuck::{Pod, Zeroable};

use crate::affine::Affine;
use crate::elements::{ParameterId, Point};
use crate::expr::{self, ExprError, Op, OpRaw};
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
    /// The compiled data breaks a limit of the runtime layout
    /// (see [`CompiledModel::check`]).
    Layout(String),
}

/// Marks "no element" in an index field (a bone, a part, an influence run).
pub const NONE: u32 = u32::MAX;
pub const NO_BONE: u32 = NONE;

/// A range of bytes in [`CompiledModel::strings`] holding one UTF-8 name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct StrRef {
    pub offset: u32,
    pub len: u32,
}

/// A driver's instructions in `CompiledModel::ops`, and the parameter it writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct DriverDef {
    pub output: u32,
    pub start: u32,
    pub len: u32,
}

/// The deformer an element hangs from, as an index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentRef {
    None,
    Warp(usize),
    Rotation(usize),
}

/// [`ParentRef`] in the runtime format: kind 0 none, 1 warp, 2 rotation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct ParentRaw {
    pub kind: u32,
    pub index: u32,
}

impl ParentRaw {
    pub const NONE: Self = Self { kind: 0, index: 0 };

    /// An unknown kind reads as no parent; [`CompiledModel::check`] rejects it.
    pub fn get(self) -> ParentRef {
        match self.kind {
            1 => ParentRef::Warp(self.index as usize),
            2 => ParentRef::Rotation(self.index as usize),
            _ => ParentRef::None,
        }
    }
}

impl From<ParentRef> for ParentRaw {
    fn from(p: ParentRef) -> Self {
        match p {
            ParentRef::None => Self::NONE,
            ParentRef::Warp(i) => Self {
                kind: 1,
                index: idx(i),
            },
            ParentRef::Rotation(i) => Self {
                kind: 2,
                index: idx(i),
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct ParamDef {
    pub min: f32,
    pub max: f32,
    pub default: f32,
    /// 0 or 1.
    pub repeat: u32,
}

/// One axis of a binding; its keys are `axis_keys[key_start..key_start + key_count]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct AxisDef {
    pub parameter: u32,
    pub key_start: u32,
    pub key_count: u32,
}

/// A binding; its axes are `axes[axis_start..axis_start + axis_count]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct BindingDef {
    pub axis_start: u32,
    pub axis_count: u32,
    pub keyform_count: u32,
    /// Where this binding's corner scratch starts in a frame, and its size.
    pub corner_start: u32,
    pub max_corners: u32,
}

/// A keyform grid: which binding weighs it and the index of its first
/// keyform in the element kind's keyform array. `binding == NONE` means the
/// element has no keyforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct KeyformRef {
    pub binding: u32,
    pub first: u32,
}

impl KeyformRef {
    pub const NONE: Self = Self {
        binding: NONE,
        first: 0,
    };

    pub fn get(self) -> Option<Self> {
        (self.binding != NONE).then_some(self)
    }
}

impl From<Option<KeyformRef>> for KeyformRef {
    fn from(k: Option<KeyformRef>) -> Self {
        k.unwrap_or(Self::NONE)
    }
}

/// Opacity, draw order, multiply color, screen color.
pub type MeshAttrs = [f32; 8];

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct BoneDef {
    /// Index of the parent bone, or `NONE`.
    pub parent: u32,
    pub base: BoneTransform,
    pub keyforms: KeyformRef,
    /// Inverse of the rest transform through the bone hierarchy.
    pub rest_inverse: Affine,
}

impl BoneDef {
    pub fn parent(&self) -> Option<usize> {
        (self.parent != NONE).then_some(self.parent as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct PhysicsInputDef {
    pub parameter: u32,
    /// A [`Channel`] as `u32`.
    pub channel: u32,
    /// Percent.
    pub weight: f32,
    /// 0 or 1.
    pub reflect: u32,
}

impl PhysicsInputDef {
    pub fn channel(&self) -> Channel {
        Channel::from_u32(self.channel).unwrap_or(Channel::X)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct PhysicsOutputDef {
    pub parameter: u32,
    /// A [`Channel`] as `u32`.
    pub channel: u32,
    /// Index within the group's particles.
    pub particle: u32,
    pub scale: f32,
    /// Percent.
    pub weight: f32,
    /// 0 or 1.
    pub reflect: u32,
}

impl PhysicsOutputDef {
    pub fn channel(&self) -> Channel {
        Channel::from_u32(self.channel).unwrap_or(Channel::X)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PhysicsKind {
    Pendulum { position: Range, angle: Range },
    Chain { gravity: Point, wind: Point },
}

/// One pendulum particle or chain joint. Fields a kind does not use are 0.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
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

pub const PHYSICS_PENDULUM: u32 = 0;
pub const PHYSICS_CHAIN: u32 = 1;

/// A pendulum group or a chain. `values` are, for kind 0, the position range
/// then the angle range (`min, max, default` each); for kind 1, gravity then
/// wind (`x, y` each, the rest 0).
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct PhysicsDef {
    pub name: StrRef,
    pub kind: u32,
    pub values: [f32; 6],
    pub input_start: u32,
    pub input_count: u32,
    pub output_start: u32,
    pub output_count: u32,
    pub particle_start: u32,
    pub particle_count: u32,
}

impl PhysicsDef {
    pub fn kind(&self) -> PhysicsKind {
        let v = self.values;
        if self.kind == PHYSICS_CHAIN {
            PhysicsKind::Chain {
                gravity: [v[0], v[1]],
                wind: [v[2], v[3]],
            }
        } else {
            PhysicsKind::Pendulum {
                position: Range {
                    min: v[0],
                    max: v[1],
                    default: v[2],
                },
                angle: Range {
                    min: v[3],
                    max: v[4],
                    default: v[5],
                },
            }
        }
    }
}

/// Up to four bones moving one vertex; unused slots are `NO_BONE` with weight 0.
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct Influence {
    pub bones: [u32; 4],
    pub weights: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct MeshDef {
    /// First vertex in the shared vertex buffer (a float offset is twice this).
    pub vertex_start: u32,
    pub vertex_count: u32,
    pub parent: ParentRaw,
    pub part: u32,
    pub base_attrs: MeshAttrs,
    pub keyforms: KeyformRef,
    /// Float offset of keyform 0's positions in `mesh_kf_positions`.
    pub kf_positions: u32,
    /// Blend shapes on this mesh: `blend_refs[blend_start..blend_start + blend_count]`.
    pub blend_start: u32,
    pub blend_count: u32,
    /// First index in `mesh_indices`. Indices are local to the mesh.
    pub index_start: u32,
    pub index_count: u32,
    pub texture: u32,
    /// A [`BlendMode`] as `u32`.
    pub blend_mode: u32,
    /// Meshes clipping this one: `mesh_masks[mask_start..mask_start + mask_count]`.
    pub mask_start: u32,
    pub mask_count: u32,
    /// 0 or 1.
    pub mask_inverted: u32,
    /// 0 or 1.
    pub double_sided: u32,
    /// First entry in `influences`; one per vertex. `NONE` when no bone moves it.
    pub influence_start: u32,
}

impl MeshDef {
    pub fn parent(&self) -> ParentRef {
        self.parent.get()
    }

    pub fn keyforms(&self) -> Option<KeyformRef> {
        self.keyforms.get()
    }

    pub fn blend_mode(&self) -> BlendMode {
        BlendMode::from_u32(self.blend_mode).unwrap_or_default()
    }

    pub fn influence_start(&self) -> Option<usize> {
        (self.influence_start != NONE).then_some(self.influence_start as usize)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct WarpDef {
    pub rows: u32,
    pub columns: u32,
    pub parent: ParentRaw,
    /// Float offset of this warp's control points; the same offset addresses
    /// its slice of the frame's world grid.
    pub point_start: u32,
    pub point_count: u32,
    pub base_opacity: f32,
    pub keyforms: KeyformRef,
    /// Float offset of keyform 0's points in `warp_kf_points`.
    pub kf_points: u32,
    /// Blend shapes on this warp: `blend_refs[blend_start..blend_start + blend_count]`.
    pub blend_start: u32,
    pub blend_count: u32,
}

impl WarpDef {
    pub fn parent(&self) -> ParentRef {
        self.parent.get()
    }

    pub fn keyforms(&self) -> Option<KeyformRef> {
        self.keyforms.get()
    }
}

/// [`RotationKeyform`] with its flags as `u32` (0 or 1).
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct RotationKf {
    pub origin: Point,
    pub angle_degrees: f32,
    pub scale: f32,
    pub reflect_x: u32,
    pub reflect_y: u32,
    pub opacity: f32,
}

impl From<RotationKeyform> for RotationKf {
    fn from(k: RotationKeyform) -> Self {
        Self {
            origin: k.origin,
            angle_degrees: k.angle_degrees,
            scale: k.scale,
            reflect_x: k.reflect_x.into(),
            reflect_y: k.reflect_y.into(),
            opacity: k.opacity,
        }
    }
}

impl From<RotationKf> for RotationKeyform {
    fn from(k: RotationKf) -> Self {
        Self {
            origin: k.origin,
            angle_degrees: k.angle_degrees,
            scale: k.scale,
            reflect_x: k.reflect_x != 0,
            reflect_y: k.reflect_y != 0,
            opacity: k.opacity,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct RotationDef {
    pub parent: ParentRaw,
    pub base: RotationKf,
    pub keyforms: KeyformRef,
}

impl RotationDef {
    pub fn parent(&self) -> ParentRef {
        self.parent.get()
    }

    pub fn keyforms(&self) -> Option<KeyformRef> {
        self.keyforms.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
#[repr(C)]
pub struct GlueDef {
    pub mesh_a: u32,
    pub mesh_b: u32,
    pub node_start: u32,
    pub node_count: u32,
    pub intensity: f32,
    pub keyforms: KeyformRef,
}

impl GlueDef {
    pub fn keyforms(&self) -> Option<KeyformRef> {
        self.keyforms.get()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlendTargetRef {
    Mesh(usize),
    Warp(usize),
}

/// [`BlendTargetRef`] in the runtime format: kind 0 mesh, 1 warp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct BlendTargetRaw {
    pub kind: u32,
    pub index: u32,
}

impl BlendTargetRaw {
    pub fn get(self) -> Option<BlendTargetRef> {
        match self.kind {
            0 => Some(BlendTargetRef::Mesh(self.index as usize)),
            1 => Some(BlendTargetRef::Warp(self.index as usize)),
            _ => None,
        }
    }
}

impl From<BlendTargetRef> for BlendTargetRaw {
    fn from(t: BlendTargetRef) -> Self {
        match t {
            BlendTargetRef::Mesh(i) => Self {
                kind: 0,
                index: idx(i),
            },
            BlendTargetRef::Warp(i) => Self {
                kind: 1,
                index: idx(i),
            },
        }
    }
}

/// A blend shape. Its keys are `blend_keys[key_start..key_start + key_count]`
/// and its weights the same range of `blend_weights`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
#[repr(C)]
pub struct BlendDef {
    pub target: BlendTargetRaw,
    pub parameter: u32,
    pub key_start: u32,
    pub key_count: u32,
    /// Float offset in `blend_deltas`.
    pub delta_start: u32,
}

/// An index that does not fit in 32 bits becomes `NONE`, which
/// [`CompiledModel::check`] rejects.
pub(crate) fn idx(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(NONE)
}

/// The evaluation data. Every array is a [`Cow`]: owned when compiled from a
/// [`Model`], borrowed from the buffer when loaded from a `.tmod`
/// (`crate::tmod`).
#[derive(Debug, Clone, Default)]
pub struct CompiledModel<'a> {
    /// UTF-8 bytes the name references point into.
    pub strings: Cow<'a, [u8]>,
    pub parameter_names: Cow<'a, [StrRef]>,
    pub parameters: Cow<'a, [ParamDef]>,
    pub bindings: Cow<'a, [BindingDef]>,
    pub axes: Cow<'a, [AxisDef]>,
    pub axis_keys: Cow<'a, [f32]>,
    /// Total corner scratch a frame needs.
    pub corner_capacity: u32,

    /// Run in this order: a driver comes after the drivers whose outputs it reads.
    pub drivers: Cow<'a, [DriverDef]>,
    pub ops: Cow<'a, [OpRaw]>,

    /// Parents come before children; `NONE` for a root.
    pub part_parents: Cow<'a, [u32]>,
    pub part_opacity: Cow<'a, [f32]>,

    pub mesh_names: Cow<'a, [StrRef]>,
    pub meshes: Cow<'a, [MeshDef]>,
    pub mesh_base_positions: Cow<'a, [f32]>,
    /// `u, v` interleaved, one pair per vertex of `mesh_base_positions`.
    pub mesh_uvs: Cow<'a, [f32]>,
    pub mesh_indices: Cow<'a, [u32]>,
    /// Mesh indices.
    pub mesh_masks: Cow<'a, [u32]>,
    pub mesh_kf_positions: Cow<'a, [f32]>,
    pub mesh_kf_attrs: Cow<'a, [MeshAttrs]>,

    pub warps: Cow<'a, [WarpDef]>,
    pub warp_base_points: Cow<'a, [f32]>,
    pub warp_kf_points: Cow<'a, [f32]>,
    pub warp_kf_opacity: Cow<'a, [f32]>,

    pub rotations: Cow<'a, [RotationDef]>,
    pub rotation_kf: Cow<'a, [RotationKf]>,

    /// Warps and rotations, parents first.
    pub deformer_order: Cow<'a, [ParentRaw]>,

    /// Parents come before children.
    pub bone_names: Cow<'a, [StrRef]>,
    pub bones: Cow<'a, [BoneDef]>,
    pub bone_kf: Cow<'a, [BoneTransform]>,
    pub influences: Cow<'a, [Influence]>,

    pub physics: Cow<'a, [PhysicsDef]>,
    pub physics_inputs: Cow<'a, [PhysicsInputDef]>,
    pub physics_outputs: Cow<'a, [PhysicsOutputDef]>,
    pub physics_particles: Cow<'a, [PhysicsParticle]>,

    pub glues: Cow<'a, [GlueDef]>,
    pub glue_nodes: Cow<'a, [GlueNode]>,
    pub glue_kf_intensity: Cow<'a, [f32]>,

    pub blends: Cow<'a, [BlendDef]>,
    pub blend_keys: Cow<'a, [f32]>,
    pub blend_weights: Cow<'a, [f32]>,
    pub blend_deltas: Cow<'a, [f32]>,
    /// Blend shape indices, grouped per mesh and per warp.
    pub blend_refs: Cow<'a, [u32]>,
}

impl CompiledModel<'_> {
    /// The text a name reference points at. A reference outside `strings`
    /// or not on a character boundary reads as empty;
    /// [`CompiledModel::check`] rejects those.
    pub fn str(&self, r: StrRef) -> &str {
        let start = r.offset as usize;
        start
            .checked_add(r.len as usize)
            .and_then(|end| self.strings.get(start..end))
            .and_then(|b| std::str::from_utf8(b).ok())
            .unwrap_or("")
    }

    pub fn parameter_name(&self, i: usize) -> &str {
        self.str(self.parameter_names[i])
    }

    pub fn mesh_name(&self, i: usize) -> &str {
        self.str(self.mesh_names[i])
    }

    pub fn bone_name(&self, i: usize) -> &str {
        self.str(self.bone_names[i])
    }

    pub fn physics_name(&self, i: usize) -> &str {
        self.str(self.physics[i].name)
    }

    pub fn parameter_index(&self, name: &str) -> Option<usize> {
        (0..self.parameter_names.len()).find(|&i| self.parameter_name(i) == name)
    }

    pub fn mesh_index(&self, name: &str) -> Option<usize> {
        (0..self.mesh_names.len()).find(|&i| self.mesh_name(i) == name)
    }

    pub fn vertex_count(&self) -> usize {
        self.mesh_base_positions.len() / 2
    }

    pub fn binding_axes(&self, b: &BindingDef) -> &[AxisDef] {
        &self.axes[b.axis_start as usize..(b.axis_start + b.axis_count) as usize]
    }

    pub fn axis_keys_of(&self, a: &AxisDef) -> &[f32] {
        &self.axis_keys[a.key_start as usize..(a.key_start + a.key_count) as usize]
    }

    pub fn blend_keys_of(&self, b: &BlendDef) -> &[f32] {
        &self.blend_keys[b.key_start as usize..(b.key_start + b.key_count) as usize]
    }

    pub fn blend_weights_of(&self, b: &BlendDef) -> &[f32] {
        &self.blend_weights[b.key_start as usize..(b.key_start + b.key_count) as usize]
    }

    /// Indices of the blend shapes on a mesh.
    pub fn mesh_blends(&self, m: &MeshDef) -> &[u32] {
        &self.blend_refs[m.blend_start as usize..(m.blend_start + m.blend_count) as usize]
    }

    /// Indices of the blend shapes on a warp.
    pub fn warp_blends(&self, w: &WarpDef) -> &[u32] {
        &self.blend_refs[w.blend_start as usize..(w.blend_start + w.blend_count) as usize]
    }
}

fn flatten(points: &[[f32; 2]], out: &mut Cow<'_, [f32]>) {
    out.to_mut().extend(points.iter().flatten());
}

/// Appends `text` to the string table and returns its reference.
fn push_str(strings: &mut Cow<'_, [u8]>, text: &str) -> StrRef {
    let r = StrRef {
        offset: idx(strings.len()),
        len: idx(text.len()),
    };
    strings.to_mut().extend_from_slice(text.as_bytes());
    r
}

impl Model {
    /// Flattens the model for evaluation. Physics is compiled too, but
    /// its state lives in the frame.
    pub fn compile(&self) -> Result<CompiledModel<'static>, CompileError> {
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(CompileError::Invalid(errors));
        }
        let mut c = CompiledModel::default();

        let mut param_index = HashMap::new();
        for (i, (id, name, p)) in self.parameters.iter().enumerate() {
            param_index.insert(id, i);
            let name = push_str(&mut c.strings, name);
            c.parameter_names.to_mut().push(name);
            c.parameters.to_mut().push(ParamDef {
                min: p.min,
                max: p.max,
                default: p.default,
                repeat: p.repeat.into(),
            });
        }

        compile_drivers(self, &param_index, &mut c)?;
        compile_physics(self, &param_index, &mut c)?;

        let mut binding_index: HashMap<BindingId, usize> = HashMap::new();
        for (i, (id, binding)) in self.bindings.iter().enumerate() {
            binding_index.insert(id, i);
            let axis_start = idx(c.axes.len());
            for a in binding.axes() {
                let key_start = idx(c.axis_keys.len());
                c.axis_keys.to_mut().extend_from_slice(&a.keys);
                c.axes.to_mut().push(AxisDef {
                    parameter: idx(param_index[&a.parameter]),
                    key_start,
                    key_count: idx(a.keys.len()),
                });
            }
            let max_corners = binding
                .axes()
                .iter()
                .map(|a| a.keys.len().min(2))
                .fold(1usize, usize::saturating_mul);
            c.bindings.to_mut().push(BindingDef {
                axis_start,
                axis_count: idx(binding.axes().len()),
                keyform_count: idx(binding.keyform_count()),
                corner_start: c.corner_capacity,
                max_corners: idx(max_corners),
            });
            c.corner_capacity = c.corner_capacity.saturating_add(idx(max_corners));
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
                .to_mut()
                .push(part.parent.map_or(NONE, |parent| idx(part_index[&parent])));
            c.part_opacity.to_mut().push(part.opacity);
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
            binding: idx(binding_index[&grid_binding]),
            first: idx(first),
        };

        let bone_index = compile_bones(self, &binding_index, &mut c)?;

        for (_, name, mesh) in self.meshes.iter() {
            let vertex_start = c.mesh_base_positions.len() / 2;
            flatten(&mesh.positions, &mut c.mesh_base_positions);
            flatten(&mesh.uvs, &mut c.mesh_uvs);
            let index_start = c.mesh_indices.len();
            c.mesh_indices.to_mut().extend(&mesh.indices);
            let mask_start = c.mesh_masks.len();
            c.mesh_masks
                .to_mut()
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
                    c.mesh_kf_attrs.to_mut().push(attrs(
                        kf.opacity,
                        kf.draw_order,
                        kf.multiply_color,
                        kf.screen_color,
                    ));
                }
            }
            let influence_start = if mesh.influences.is_empty() {
                NONE
            } else {
                let start = c.influences.len();
                c.influences
                    .to_mut()
                    .extend(mesh.influences.iter().map(|inf| {
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
                idx(start)
            };
            let name = push_str(&mut c.strings, name);
            c.mesh_names.to_mut().push(name);
            c.meshes.to_mut().push(MeshDef {
                vertex_start: idx(vertex_start),
                vertex_count: idx(mesh.positions.len()),
                parent: parent_ref(mesh.parent).into(),
                part: idx(part_index[&mesh.part]),
                base_attrs,
                keyforms: keyforms.into(),
                kf_positions: idx(kf_positions),
                blend_start: 0,
                blend_count: 0,
                index_start: idx(index_start),
                index_count: idx(mesh.indices.len()),
                texture: mesh.texture,
                blend_mode: mesh.blend_mode.to_u32(),
                mask_start: idx(mask_start),
                mask_count: idx(mesh.masks.len()),
                mask_inverted: mesh.mask_inverted.into(),
                double_sided: mesh.double_sided.into(),
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
                    c.warp_kf_opacity.to_mut().push(kf.opacity);
                }
            }
            c.warps.to_mut().push(WarpDef {
                rows: warp.rows,
                columns: warp.columns,
                parent: parent_ref(warp.parent).into(),
                point_start: idx(point_start),
                point_count: idx(warp.base_points.len()),
                base_opacity: 1.0,
                keyforms: keyforms.into(),
                kf_points: idx(kf_points),
                blend_start: 0,
                blend_count: 0,
            });
        }

        for (_, _, rotation) in self.rotations.iter() {
            let mut keyforms = None;
            if let Some(grid) = &rotation.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.rotation_kf.len()));
                c.rotation_kf
                    .to_mut()
                    .extend(grid.keyforms.iter().map(|&k| RotationKf::from(k)));
            }
            c.rotations.to_mut().push(RotationDef {
                parent: parent_ref(rotation.parent).into(),
                base: rotation.base.into(),
                keyforms: keyforms.into(),
            });
        }

        for (_, _, glue) in self.glues.iter() {
            let node_start = c.glue_nodes.len();
            c.glue_nodes.to_mut().extend(glue.nodes.iter().copied());
            let mut keyforms = None;
            if let Some(grid) = &glue.keyforms {
                keyforms = Some(keyform_ref(grid.binding, c.glue_kf_intensity.len()));
                c.glue_kf_intensity
                    .to_mut()
                    .extend(grid.keyforms.iter().copied());
            }
            c.glues.to_mut().push(GlueDef {
                mesh_a: idx(mesh_index[&glue.mesh_a]),
                mesh_b: idx(mesh_index[&glue.mesh_b]),
                node_start: idx(node_start),
                node_count: idx(glue.nodes.len()),
                intensity: glue.intensity,
                keyforms: keyforms.into(),
            });
        }

        let mut mesh_blends: Vec<Vec<u32>> = vec![Vec::new(); c.meshes.len()];
        let mut warp_blends: Vec<Vec<u32>> = vec![Vec::new(); c.warps.len()];
        for (_, _, shape) in self.blend_shapes.iter() {
            let index = idx(c.blends.len());
            let target = match shape.target {
                BlendTarget::Mesh(id) => {
                    let m = mesh_index[&id];
                    mesh_blends[m].push(index);
                    BlendTargetRef::Mesh(m)
                }
                BlendTarget::Warp(id) => {
                    let w = warp_index[&id];
                    warp_blends[w].push(index);
                    BlendTargetRef::Warp(w)
                }
            };
            let delta_start = c.blend_deltas.len();
            flatten(&shape.deltas, &mut c.blend_deltas);
            let key_start = idx(c.blend_keys.len());
            c.blend_keys.to_mut().extend_from_slice(&shape.keys);
            c.blend_weights.to_mut().extend_from_slice(&shape.weights);
            c.blends.to_mut().push(BlendDef {
                target: target.into(),
                parameter: idx(param_index[&shape.parameter]),
                key_start,
                key_count: idx(shape.keys.len()),
                delta_start: idx(delta_start),
            });
        }
        let CompiledModel {
            blend_refs,
            meshes,
            warps,
            ..
        } = &mut c;
        for (def, list) in meshes.to_mut().iter_mut().zip(&mesh_blends) {
            def.blend_start = idx(blend_refs.len());
            def.blend_count = idx(list.len());
            blend_refs.to_mut().extend_from_slice(list);
        }
        for (def, list) in warps.to_mut().iter_mut().zip(&warp_blends) {
            def.blend_start = idx(blend_refs.len());
            def.blend_count = idx(list.len());
            blend_refs.to_mut().extend_from_slice(list);
        }

        // Deformers, parents first: sort by depth in the parent chain.
        let depth = |mut parent: ParentRef| {
            let mut depth = 0;
            loop {
                parent = match parent {
                    ParentRef::None => return depth,
                    ParentRef::Warp(i) => c.warps[i].parent(),
                    ParentRef::Rotation(i) => c.rotations[i].parent(),
                };
                depth += 1;
            }
        };
        let mut order: Vec<ParentRef> = (0..c.warps.len())
            .map(ParentRef::Warp)
            .chain((0..c.rotations.len()).map(ParentRef::Rotation))
            .collect();
        order.sort_by_key(|&d| depth(d));
        c.deformer_order = order.into_iter().map(ParentRaw::from).collect();

        check_bone_spaces(&c)?;
        c.check().map_err(|e| CompileError::Layout(e.to_string()))?;

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
        let start = (idx(c.physics_inputs.len()), idx(c.physics_outputs.len()));
        c.physics_inputs
            .to_mut()
            .extend(inputs.iter().map(|i| PhysicsInputDef {
                parameter: idx(param_index[&i.parameter]),
                channel: i.channel.to_u32(),
                weight: i.weight,
                reflect: i.reflect.into(),
            }));
        c.physics_outputs
            .to_mut()
            .extend(outputs.iter().map(|o| PhysicsOutputDef {
                parameter: idx(param_index[&o.parameter]),
                channel: o.channel.to_u32(),
                particle: idx(o.particle),
                scale: o.scale,
                weight: o.weight,
                reflect: o.reflect.into(),
            }));
        start
    };

    for group in &rig.pendulums {
        let (input_start, output_start) = ios(c, &group.inputs, &group.outputs);
        let particle_start = idx(c.physics_particles.len());
        c.physics_particles
            .to_mut()
            .extend(group.particles.iter().map(|p| PhysicsParticle {
                rest: p.position,
                length: p.radius,
                mobility: p.mobility,
                delay: p.delay,
                acceleration: p.acceleration,
                stiffness: 0.0,
                damping: 0.0,
            }));
        let (p, a) = (group.position_range, group.angle_range);
        let name = push_str(&mut c.strings, &group.name);
        c.physics.to_mut().push(PhysicsDef {
            name,
            kind: PHYSICS_PENDULUM,
            values: [p.min, p.max, p.default, a.min, a.max, a.default],
            input_start,
            input_count: idx(group.inputs.len()),
            output_start,
            output_count: idx(group.outputs.len()),
            particle_start,
            particle_count: idx(group.particles.len()),
        });
    }
    for chain in &rig.chains {
        let (input_start, output_start) = ios(c, &chain.inputs, &chain.outputs);
        let particle_start = idx(c.physics_particles.len());
        c.physics_particles
            .to_mut()
            .extend(chain.joints.iter().map(|j| PhysicsParticle {
                rest: [0.0, 0.0],
                length: j.length,
                mobility: 0.0,
                delay: 0.0,
                acceleration: 0.0,
                stiffness: j.stiffness,
                damping: j.damping,
            }));
        let (g, w) = (chain.gravity, chain.wind);
        let name = push_str(&mut c.strings, &chain.name);
        c.physics.to_mut().push(PhysicsDef {
            name,
            kind: PHYSICS_CHAIN,
            values: [g[0], g[1], w[0], w[1], 0.0, 0.0],
            input_start,
            input_count: idx(chain.inputs.len()),
            output_start,
            output_count: idx(chain.outputs.len()),
            particle_start,
            particle_count: idx(chain.joints.len()),
        });
    }

    for (i, def) in c.physics.iter().enumerate() {
        let outputs = &c.physics_outputs
            [def.output_start as usize..(def.output_start + def.output_count) as usize];
        if outputs
            .iter()
            .any(|o| c.drivers.iter().any(|d| d.output == o.parameter))
        {
            return Err(CompileError::PhysicsOutputConflict {
                group: c.physics_name(i).to_owned(),
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
                binding: idx(binding_index[&grid.binding]),
                first: idx(c.bone_kf.len()),
            };
            c.bone_kf.to_mut().extend(grid.keyforms.iter().copied());
            k
        });
        let name = push_str(&mut c.strings, name);
        c.bone_names.to_mut().push(name);
        c.bones.to_mut().push(BoneDef {
            parent: bone.parent.map_or(NONE, |p| idx(index[&p])),
            base: bone.base,
            keyforms: keyforms.into(),
            rest_inverse,
        });
    }
    Ok(index)
}

/// Every mesh using a bone tree must hang from the same parent (ADR-0021).
fn check_bone_spaces(c: &CompiledModel) -> Result<(), CompileError> {
    let root_of = |mut b: usize| {
        while let Some(p) = c.bones[b].parent() {
            b = p;
        }
        b
    };
    let mut space: HashMap<usize, ParentRef> = HashMap::new();
    for def in c.meshes.iter() {
        let Some(start) = def.influence_start() else {
            continue;
        };
        let used = c.influences[start..start + def.vertex_count as usize]
            .iter()
            .flat_map(|inf| inf.bones)
            .filter(|&b| b != NO_BONE);
        for b in used {
            let b = b as usize;
            if *space.entry(root_of(b)).or_insert(def.parent()) != def.parent() {
                return Err(CompileError::BoneSpaceMismatch {
                    bone: c.bone_name(b).to_owned(),
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
    let by_name: HashMap<String, usize> = (0..c.parameter_names.len())
        .map(|i| (c.parameter_name(i).to_owned(), i))
        .collect();

    let mut defs = Vec::new();
    let mut names = Vec::new();
    let mut writer: HashMap<usize, usize> = HashMap::new();
    let mut ops: Vec<Op> = Vec::new();
    for (_, name, driver) in model.drivers.iter() {
        let output = param_index[&driver.output];
        if writer.insert(output, defs.len()).is_some() {
            return Err(CompileError::DuplicateDriverOutput {
                driver: name.to_owned(),
            });
        }
        let start = ops.len();
        expr::compile(&driver.expression, |n| by_name.get(n).copied(), &mut ops).map_err(
            |error| CompileError::Expression {
                driver: name.to_owned(),
                error,
            },
        )?;
        defs.push(DriverDef {
            output: idx(output),
            start: idx(start),
            len: idx(ops.len() - start),
        });
        names.push(name);
    }

    // shortcut: quadratic ordering, fine for the tens of drivers a rig has;
    // upgrade to Kahn's algorithm if models grow to thousands.
    let range = |def: &DriverDef| def.start as usize..(def.start + def.len) as usize;
    let mut done = vec![false; defs.len()];
    while c.drivers.len() < defs.len() {
        let before = c.drivers.len();
        for (i, def) in defs.iter().enumerate() {
            let ready = !done[i]
                && ops[range(def)].iter().all(|op| match op {
                    Op::Param(p) => writer.get(p).is_none_or(|&w| done[w]),
                    _ => true,
                });
            if ready {
                done[i] = true;
                c.drivers.to_mut().push(*def);
            }
        }
        if c.drivers.len() == before {
            let stuck = done.iter().position(|d| !d).expect("a driver is left");
            return Err(CompileError::DriverCycle {
                driver: names[stuck].to_owned(),
            });
        }
    }
    c.ops = ops.into_iter().map(OpRaw::from).collect();
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
