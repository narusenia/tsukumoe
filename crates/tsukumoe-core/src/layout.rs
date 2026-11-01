// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Consistency checks for [`CompiledModel`]: every offset, count and index
//! points inside its array, enum values are known, numbers are finite and
//! orders hold. A model that passes can be evaluated without any panic, so a
//! `.tmod` loader checks once and the evaluator does not defend itself.
//! The limits below are part of the runtime format (`docs/specifications/file-formats.md`).

use std::fmt;

use crate::compiled::{
    BlendTargetRef, CompiledModel, KeyformRef, NONE, PHYSICS_CHAIN, PHYSICS_PENDULUM, ParentRaw,
    StrRef,
};
use crate::expr::{MAX_STACK, Op};
use crate::physics::Channel;
use crate::rig::BlendMode;

/// Most axes one binding may have.
pub const MAX_AXES: u32 = 16;
/// Most corner scratch entries a frame may need in total.
pub const MAX_CORNERS: u32 = 1 << 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutError {
    pub array: &'static str,
    pub index: usize,
    pub reason: &'static str,
}

impl fmt::Display for LayoutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}[{}]: {}", self.array, self.index, self.reason)
    }
}

impl std::error::Error for LayoutError {}

type Check = Result<(), LayoutError>;

fn ensure(ok: bool, array: &'static str, index: usize, reason: &'static str) -> Check {
    if ok {
        Ok(())
    } else {
        Err(LayoutError {
            array,
            index,
            reason,
        })
    }
}

/// `start + count <= len`, without overflow.
fn within(start: u32, count: u64, len: usize) -> bool {
    start != NONE && u64::from(start) + count <= len as u64
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}

fn flag(v: u32) -> bool {
    v <= 1
}

fn ascending(keys: &[f32]) -> bool {
    finite(keys) && keys.windows(2).all(|w| w[0] < w[1])
}

impl CompiledModel<'_> {
    /// Checks every invariant the evaluator relies on.
    pub fn check(&self) -> Check {
        self.check_lengths()?;
        self.check_names()?;
        self.check_parameters()?;
        self.check_bindings()?;
        self.check_drivers()?;
        self.check_parts()?;
        self.check_meshes()?;
        self.check_warps()?;
        self.check_rotations()?;
        self.check_deformer_order()?;
        self.check_bones()?;
        self.check_physics()?;
        self.check_glues()?;
        self.check_blends()?;
        Ok(())
    }

    fn check_lengths(&self) -> Check {
        macro_rules! small {
            ($($f:ident),*) => {$(
                ensure(self.$f.len() < NONE as usize, stringify!($f), 0, "too many elements")?;
            )*};
        }
        small!(
            strings,
            parameters,
            bindings,
            axes,
            axis_keys,
            drivers,
            ops,
            part_parents,
            meshes,
            mesh_base_positions,
            mesh_indices,
            mesh_masks,
            mesh_kf_positions,
            mesh_kf_attrs,
            warps,
            warp_base_points,
            warp_kf_points,
            rotations,
            bones,
            influences,
            physics,
            physics_inputs,
            physics_outputs,
            physics_particles,
            glues,
            glue_nodes,
            blends,
            blend_keys,
            blend_deltas,
            blend_refs
        );
        ensure(
            self.parameter_names.len() == self.parameters.len(),
            "parameter_names",
            0,
            "one name per parameter",
        )?;
        ensure(
            self.mesh_names.len() == self.meshes.len(),
            "mesh_names",
            0,
            "one name per mesh",
        )?;
        ensure(
            self.bone_names.len() == self.bones.len(),
            "bone_names",
            0,
            "one name per bone",
        )?;
        ensure(
            self.part_opacity.len() == self.part_parents.len(),
            "part_opacity",
            0,
            "one opacity per part",
        )?;
        ensure(
            self.mesh_hidden.windows(2).all(|w| w[0] < w[1])
                && self
                    .mesh_hidden
                    .iter()
                    .all(|&m| (m as usize) < self.meshes.len()),
            "mesh_hidden",
            0,
            "not ascending mesh indices",
        )?;
        ensure(
            self.mesh_base_positions.len().is_multiple_of(2),
            "mesh_base_positions",
            0,
            "odd float count",
        )?;
        ensure(
            self.mesh_uvs.len() == self.mesh_base_positions.len(),
            "mesh_uvs",
            0,
            "one uv per vertex",
        )?;
        ensure(
            self.mesh_kf_attrs.iter().all(|a| finite(a)),
            "mesh_kf_attrs",
            0,
            "not finite",
        )?;
        ensure(
            self.blend_weights.len() == self.blend_keys.len(),
            "blend_weights",
            0,
            "one weight per key",
        )?;
        ensure(
            finite(&self.mesh_base_positions)
                && finite(&self.mesh_uvs)
                && finite(&self.mesh_kf_positions)
                && finite(&self.warp_base_points)
                && finite(&self.warp_kf_points)
                && finite(&self.warp_kf_opacity)
                && finite(&self.part_opacity)
                && finite(&self.glue_kf_intensity)
                && finite(&self.blend_weights)
                && finite(&self.blend_deltas),
            "floats",
            0,
            "not finite",
        )?;
        ensure(
            self.rotation_kf.iter().all(|k| {
                finite(&k.origin)
                    && finite(&[k.angle_degrees, k.scale, k.opacity])
                    && flag(k.reflect_x)
                    && flag(k.reflect_y)
            }),
            "rotation_kf",
            0,
            "not finite or bad flag",
        )?;
        ensure(
            self.bone_kf.iter().all(|k| {
                finite(&k.translation) && finite(&k.scale) && k.rotation_degrees.is_finite()
            }),
            "bone_kf",
            0,
            "not finite",
        )?;
        ensure(
            self.glue_nodes.iter().all(|n| n.weight.is_finite()),
            "glue_nodes",
            0,
            "not finite",
        )?;
        ensure(
            self.influences.iter().all(|i| finite(&i.weights)),
            "influences",
            0,
            "not finite",
        )?;
        ensure(
            self.physics_particles.iter().all(|p| {
                finite(&p.rest)
                    && finite(&[
                        p.length,
                        p.mobility,
                        p.delay,
                        p.acceleration,
                        p.stiffness,
                        p.damping,
                    ])
            }),
            "physics_particles",
            0,
            "not finite",
        )
    }

    fn check_names(&self) -> Check {
        let ok = |r: &StrRef| {
            within(r.offset, u64::from(r.len), self.strings.len())
                && std::str::from_utf8(
                    &self.strings[r.offset as usize..(r.offset + r.len) as usize],
                )
                .is_ok()
        };
        for (array, names) in [
            ("parameter_names", &self.parameter_names),
            ("mesh_names", &self.mesh_names),
            ("bone_names", &self.bone_names),
        ] {
            if let Some(i) = names.iter().position(|r| !ok(r)) {
                return ensure(
                    false,
                    array,
                    i,
                    "name outside the string table or not UTF-8",
                );
            }
        }
        match self.physics.iter().position(|p| !ok(&p.name)) {
            Some(i) => ensure(
                false,
                "physics",
                i,
                "name outside the string table or not UTF-8",
            ),
            None => Ok(()),
        }
    }

    fn check_parameters(&self) -> Check {
        for (i, p) in self.parameters.iter().enumerate() {
            ensure(
                finite(&[p.min, p.max, p.default]) && p.min <= p.max,
                "parameters",
                i,
                "range is not finite or min > max",
            )?;
            ensure(flag(p.repeat), "parameters", i, "bad flag")?;
            ensure(
                p.repeat == 0 || p.min < p.max,
                "parameters",
                i,
                "a repeating parameter needs a range",
            )?;
        }
        Ok(())
    }

    fn check_bindings(&self) -> Check {
        let mut corners = 0u64;
        for (i, b) in self.bindings.iter().enumerate() {
            ensure(
                b.axis_count <= MAX_AXES
                    && within(b.axis_start, u64::from(b.axis_count), self.axes.len()),
                "bindings",
                i,
                "axes outside the array or over the limit",
            )?;
            let (mut keyforms, mut max_corners) = (1u64, 1u64);
            for a in self.binding_axes(b) {
                ensure(
                    (a.parameter as usize) < self.parameters.len()
                        && a.key_count >= 1
                        && within(a.key_start, u64::from(a.key_count), self.axis_keys.len()),
                    "axes",
                    i,
                    "parameter or keys out of range",
                )?;
                ensure(
                    ascending(self.axis_keys_of(a)),
                    "axes",
                    i,
                    "keys are not strictly ascending numbers",
                )?;
                keyforms *= u64::from(a.key_count);
                max_corners *= u64::from(a.key_count.min(2));
            }
            ensure(
                keyforms == u64::from(b.keyform_count) && max_corners == u64::from(b.max_corners),
                "bindings",
                i,
                "keyform or corner count does not match the axes",
            )?;
            ensure(
                u64::from(b.corner_start) == corners,
                "bindings",
                i,
                "corner scratch is not consecutive",
            )?;
            corners += max_corners;
        }
        ensure(
            corners == u64::from(self.corner_capacity) && corners <= u64::from(MAX_CORNERS),
            "corner_capacity",
            0,
            "does not match the bindings or over the limit",
        )
    }

    fn check_drivers(&self) -> Check {
        for (i, d) in self.drivers.iter().enumerate() {
            ensure(
                (d.output as usize) < self.parameters.len()
                    && within(d.start, u64::from(d.len), self.ops.len()),
                "drivers",
                i,
                "output or instructions out of range",
            )?;
            let mut sp = 0i32;
            for raw in &self.ops[d.start as usize..(d.start + d.len) as usize] {
                let Some(op) = Op::from_raw(*raw) else {
                    return ensure(false, "ops", i, "unknown instruction");
                };
                match op {
                    Op::Const(c) => ensure(c.is_finite(), "ops", i, "constant is not finite")?,
                    Op::Param(p) => ensure(
                        p < self.parameters.len(),
                        "ops",
                        i,
                        "parameter out of range",
                    )?,
                    _ => {}
                }
                ensure(sp >= op.pops(), "ops", i, "stack underflow")?;
                sp += op.stack_delta();
                ensure(sp as usize <= MAX_STACK, "ops", i, "stack too deep")?;
            }
            ensure(sp == 1, "ops", i, "an expression must leave one value")?;
        }
        Ok(())
    }

    fn check_parts(&self) -> Check {
        for (i, &p) in self.part_parents.iter().enumerate() {
            ensure(
                p == NONE || (p as usize) < i,
                "part_parents",
                i,
                "parent must come before the part",
            )?;
        }
        Ok(())
    }

    fn check_keyforms(
        &self,
        array: &'static str,
        i: usize,
        k: KeyformRef,
        len: usize,
    ) -> Result<Option<u64>, LayoutError> {
        let Some(k) = k.get() else { return Ok(None) };
        ensure(
            (k.binding as usize) < self.bindings.len(),
            array,
            i,
            "binding out of range",
        )?;
        let n = u64::from(self.bindings[k.binding as usize].keyform_count);
        ensure(
            within(k.first, n, len),
            array,
            i,
            "keyforms outside the array",
        )?;
        Ok(Some(n))
    }

    fn check_parent(&self, array: &'static str, i: usize, p: ParentRaw) -> Check {
        let ok = match p.kind {
            0 => p.index == 0,
            1 => (p.index as usize) < self.warps.len(),
            2 => (p.index as usize) < self.rotations.len(),
            _ => false,
        };
        ensure(ok, array, i, "bad parent")
    }

    /// Blend shapes listed for one target: in range and aimed at it.
    fn check_blend_list(
        &self,
        array: &'static str,
        i: usize,
        start: u32,
        count: u32,
        target: BlendTargetRef,
    ) -> Check {
        ensure(
            within(start, u64::from(count), self.blend_refs.len()),
            array,
            i,
            "blend list outside the array",
        )?;
        for &b in &self.blend_refs[start as usize..(start + count) as usize] {
            ensure(
                self.blends
                    .get(b as usize)
                    .is_some_and(|d| d.target.get() == Some(target)),
                array,
                i,
                "blend shape missing or aimed elsewhere",
            )?;
        }
        Ok(())
    }

    fn check_meshes(&self) -> Check {
        let vertices = self.mesh_base_positions.len() / 2;
        for (i, m) in self.meshes.iter().enumerate() {
            let n = u64::from(m.vertex_count);
            ensure(
                within(m.vertex_start, n, vertices),
                "meshes",
                i,
                "vertices outside the array",
            )?;
            self.check_parent("meshes", i, m.parent)?;
            ensure(
                (m.part as usize) < self.part_parents.len(),
                "meshes",
                i,
                "part out of range",
            )?;
            ensure(finite(&m.base_attrs), "meshes", i, "attributes not finite")?;
            if let Some(kfs) =
                self.check_keyforms("meshes", i, m.keyforms, self.mesh_kf_attrs.len())?
            {
                ensure(
                    within(m.kf_positions, kfs * n * 2, self.mesh_kf_positions.len()),
                    "meshes",
                    i,
                    "keyform positions outside the array",
                )?;
            }
            self.check_blend_list(
                "meshes",
                i,
                m.blend_start,
                m.blend_count,
                BlendTargetRef::Mesh(i),
            )?;
            ensure(
                within(
                    m.index_start,
                    u64::from(m.index_count),
                    self.mesh_indices.len(),
                ) && self.mesh_indices
                    [m.index_start as usize..(m.index_start + m.index_count) as usize]
                    .iter()
                    .all(|&v| u64::from(v) < n),
                "meshes",
                i,
                "indices outside the array or past the vertices",
            )?;
            ensure(
                BlendMode::from_u32(m.blend_mode).is_some()
                    && flag(m.mask_inverted)
                    && flag(m.double_sided),
                "meshes",
                i,
                "unknown blend mode or bad flag",
            )?;
            ensure(
                within(m.mask_start, u64::from(m.mask_count), self.mesh_masks.len())
                    && self.mesh_masks
                        [m.mask_start as usize..(m.mask_start + m.mask_count) as usize]
                        .iter()
                        .all(|&v| (v as usize) < self.meshes.len()),
                "meshes",
                i,
                "masks outside the array or not meshes",
            )?;
            if m.influence_start != NONE {
                ensure(
                    within(m.influence_start, n, self.influences.len())
                        && self.influences[m.influence_start as usize
                            ..(m.influence_start + m.vertex_count) as usize]
                            .iter()
                            .all(|inf| {
                                inf.bones
                                    .iter()
                                    .all(|&b| b == NONE || (b as usize) < self.bones.len())
                            }),
                    "meshes",
                    i,
                    "influences outside the array or naming no bone",
                )?;
            }
        }
        Ok(())
    }

    fn check_warps(&self) -> Check {
        for (i, w) in self.warps.iter().enumerate() {
            let points = (u64::from(w.rows) + 1) * (u64::from(w.columns) + 1);
            ensure(
                w.rows >= 1 && w.columns >= 1 && u64::from(w.point_count) == points,
                "warps",
                i,
                "grid size does not match the point count",
            )?;
            let n = points * 2;
            ensure(
                within(w.point_start, n, self.warp_base_points.len()),
                "warps",
                i,
                "points outside the array",
            )?;
            self.check_parent("warps", i, w.parent)?;
            ensure(w.base_opacity.is_finite(), "warps", i, "opacity not finite")?;
            if let Some(kfs) =
                self.check_keyforms("warps", i, w.keyforms, self.warp_kf_opacity.len())?
            {
                ensure(
                    within(w.kf_points, kfs * n, self.warp_kf_points.len()),
                    "warps",
                    i,
                    "keyform points outside the array",
                )?;
            }
            self.check_blend_list(
                "warps",
                i,
                w.blend_start,
                w.blend_count,
                BlendTargetRef::Warp(i),
            )?;
        }
        Ok(())
    }

    fn check_rotations(&self) -> Check {
        for (i, r) in self.rotations.iter().enumerate() {
            self.check_parent("rotations", i, r.parent)?;
            let b = r.base;
            ensure(
                finite(&b.origin)
                    && finite(&[b.angle_degrees, b.scale, b.opacity])
                    && flag(b.reflect_x)
                    && flag(b.reflect_y),
                "rotations",
                i,
                "base not finite or bad flag",
            )?;
            self.check_keyforms("rotations", i, r.keyforms, self.rotation_kf.len())?;
        }
        Ok(())
    }

    fn check_deformer_order(&self) -> Check {
        ensure(
            self.deformer_order.len() == self.warps.len() + self.rotations.len(),
            "deformer_order",
            0,
            "must list every warp and rotation once",
        )?;
        let mut seen_warp = vec![false; self.warps.len()];
        let mut seen_rot = vec![false; self.rotations.len()];
        for (i, d) in self.deformer_order.iter().enumerate() {
            let (seen, parent) = match (d.kind, d.index as usize) {
                (1, w) if w < self.warps.len() => (&mut seen_warp[w], self.warps[w].parent),
                (2, r) if r < self.rotations.len() => (&mut seen_rot[r], self.rotations[r].parent),
                _ => return ensure(false, "deformer_order", i, "not a deformer"),
            };
            ensure(!*seen, "deformer_order", i, "listed twice")?;
            *seen = true;
            let parent_first = match parent.kind {
                1 => seen_warp[parent.index as usize],
                2 => seen_rot[parent.index as usize],
                _ => true,
            };
            ensure(
                parent_first,
                "deformer_order",
                i,
                "a parent must come first",
            )?;
        }
        Ok(())
    }

    fn check_bones(&self) -> Check {
        for (i, b) in self.bones.iter().enumerate() {
            ensure(
                b.parent == NONE || (b.parent as usize) < i,
                "bones",
                i,
                "parent must come before the bone",
            )?;
            ensure(
                finite(&b.base.translation)
                    && finite(&b.base.scale)
                    && b.base.rotation_degrees.is_finite()
                    && finite(&b.rest_inverse.m)
                    && finite(&b.rest_inverse.t),
                "bones",
                i,
                "not finite",
            )?;
            self.check_keyforms("bones", i, b.keyforms, self.bone_kf.len())?;
        }
        Ok(())
    }

    fn check_physics(&self) -> Check {
        for (i, g) in self.physics.iter().enumerate() {
            ensure(
                (g.kind == PHYSICS_PENDULUM || g.kind == PHYSICS_CHAIN) && finite(&g.values),
                "physics",
                i,
                "unknown kind or not finite",
            )?;
            ensure(
                within(
                    g.input_start,
                    u64::from(g.input_count),
                    self.physics_inputs.len(),
                ) && within(
                    g.output_start,
                    u64::from(g.output_count),
                    self.physics_outputs.len(),
                ) && within(
                    g.particle_start,
                    u64::from(g.particle_count),
                    self.physics_particles.len(),
                ),
                "physics",
                i,
                "inputs, outputs or particles outside the array",
            )?;
            let inputs = &self.physics_inputs
                [g.input_start as usize..(g.input_start + g.input_count) as usize];
            ensure(
                inputs.iter().all(|x| {
                    (x.parameter as usize) < self.parameters.len()
                        && Channel::from_u32(x.channel).is_some()
                        && flag(x.reflect)
                        && x.weight.is_finite()
                }),
                "physics",
                i,
                "bad input",
            )?;
            let outputs = &self.physics_outputs
                [g.output_start as usize..(g.output_start + g.output_count) as usize];
            ensure(
                outputs.iter().all(|x| {
                    (x.parameter as usize) < self.parameters.len()
                        && Channel::from_u32(x.channel).is_some()
                        && flag(x.reflect)
                        && x.particle < g.particle_count
                        && finite(&[x.scale, x.weight])
                }),
                "physics",
                i,
                "bad output",
            )?;
        }
        Ok(())
    }

    fn check_glues(&self) -> Check {
        for (i, g) in self.glues.iter().enumerate() {
            let (Some(a), Some(b)) = (
                self.meshes.get(g.mesh_a as usize),
                self.meshes.get(g.mesh_b as usize),
            ) else {
                return ensure(false, "glues", i, "mesh out of range");
            };
            ensure(
                within(g.node_start, u64::from(g.node_count), self.glue_nodes.len())
                    && self.glue_nodes
                        [g.node_start as usize..(g.node_start + g.node_count) as usize]
                        .iter()
                        .all(|n| n.index_a < a.vertex_count && n.index_b < b.vertex_count),
                "glues",
                i,
                "nodes outside the array or past the vertices",
            )?;
            ensure(g.intensity.is_finite(), "glues", i, "intensity not finite")?;
            self.check_keyforms("glues", i, g.keyforms, self.glue_kf_intensity.len())?;
        }
        Ok(())
    }

    fn check_blends(&self) -> Check {
        for (i, b) in self.blends.iter().enumerate() {
            let target_ok = match b.target.get() {
                Some(BlendTargetRef::Mesh(m)) => m < self.meshes.len(),
                Some(BlendTargetRef::Warp(w)) => w < self.warps.len(),
                None => false,
            };
            ensure(target_ok, "blends", i, "bad target")?;
            ensure(
                (b.parameter as usize) < self.parameters.len()
                    && within(b.key_start, u64::from(b.key_count), self.blend_keys.len()),
                "blends",
                i,
                "parameter or keys out of range",
            )?;
            ensure(
                ascending(self.blend_keys_of(b)),
                "blends",
                i,
                "keys are not strictly ascending numbers",
            )?;
            let n = match b.target.get() {
                Some(BlendTargetRef::Mesh(m)) => u64::from(self.meshes[m].vertex_count),
                Some(BlendTargetRef::Warp(w)) => u64::from(self.warps[w].point_count),
                None => 0,
            };
            ensure(
                within(b.delta_start, n * 2, self.blend_deltas.len()),
                "blends",
                i,
                "deltas outside the array",
            )?;
        }
        Ok(())
    }
}
