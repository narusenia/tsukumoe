// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame evaluation. The rules are in `docs/specifications/evaluation.md`.

use crate::affine::Affine;
use crate::compiled::{
    BindingDef, CompiledModel, Influence, KeyformRef, MeshAttrs, NO_BONE, NONE, ParamDef,
    ParentRef, RotationKf,
};
use crate::elements::Point;
use crate::expr;
use crate::rig::{BoneTransform, RotationKeyform};
use crate::simulate;

impl ParamDef {
    pub(crate) fn apply(&self, value: f32) -> f32 {
        if !value.is_finite() {
            return self.default;
        }
        if self.repeat != 0 {
            self.min + (value - self.min).rem_euclid(self.max - self.min)
        } else {
            value.clamp(self.min, self.max)
        }
    }
}

/// Per-instance working memory. Sized once from the compiled model; evaluating
/// allocates nothing.
pub struct Frame {
    values: Vec<f32>,
    clamped: Vec<f32>,
    corners: Vec<(usize, f32)>,
    corner_counts: Vec<usize>,
    blend_weights: Vec<f32>,
    part_world: Vec<f32>,
    warp_local: Vec<f32>,
    warp_world: Vec<f32>,
    warp_opacity: Vec<f32>,
    rot_world: Vec<Affine>,
    rot_opacity: Vec<f32>,
    bone_world: Vec<Affine>,
    bone_skin: Vec<Affine>,
    scratch: Vec<f32>,
    positions: Vec<f32>,
    attrs: Vec<MeshAttrs>,
    // The previous frame, for the draw list's change flags.
    prev_positions: Vec<f32>,
    prev_attrs: Vec<MeshAttrs>,
    /// Mesh indices in drawing order (see `draw.rs`) and the previous frame's.
    pub(crate) order: Vec<u32>,
    prev_order: Vec<u32>,
    evaluated: bool,
    physics: simulate::State,
    pub(crate) order_changed: bool,
    pub(crate) geometry_changed: Vec<bool>,
    pub(crate) style_changed: Vec<bool>,
}

impl Frame {
    pub fn new(model: &CompiledModel) -> Self {
        let max_points = model
            .warps
            .iter()
            .map(|w| w.point_count as usize * 2)
            .max()
            .unwrap_or(0);
        Self {
            values: model.parameters.iter().map(|p| p.default).collect(),
            clamped: vec![0.0; model.parameters.len()],
            corners: vec![(0, 0.0); model.corner_capacity as usize],
            corner_counts: vec![0; model.bindings.len()],
            blend_weights: vec![0.0; model.blends.len()],
            part_world: vec![1.0; model.part_opacity.len()],
            warp_local: vec![0.0; model.warp_base_points.len()],
            warp_world: vec![0.0; model.warp_base_points.len()],
            warp_opacity: vec![1.0; model.warps.len()],
            rot_world: vec![Affine::default(); model.rotations.len()],
            rot_opacity: vec![1.0; model.rotations.len()],
            bone_world: vec![Affine::IDENTITY; model.bones.len()],
            bone_skin: vec![Affine::IDENTITY; model.bones.len()],
            scratch: vec![0.0; max_points],
            positions: vec![0.0; model.mesh_base_positions.len()],
            attrs: vec![[0.0; 8]; model.meshes.len()],
            // NaN never equals itself, so the first frame is all changed.
            prev_positions: vec![f32::NAN; model.mesh_base_positions.len()],
            prev_attrs: vec![[f32::NAN; 8]; model.meshes.len()],
            order: (0..model.meshes.len() as u32).collect(),
            prev_order: (0..model.meshes.len() as u32).collect(),
            evaluated: false,
            physics: simulate::State::new(model),
            order_changed: true,
            geometry_changed: vec![true; model.meshes.len()],
            style_changed: vec![true; model.meshes.len()],
        }
    }

    pub fn set_parameter(&mut self, index: usize, value: f32) {
        self.values[index] = value;
    }

    pub fn parameters_mut(&mut self) -> &mut [f32] {
        &mut self.values
    }

    /// Vertex positions of all meshes in drawing space, `x, y` interleaved.
    pub fn positions(&self) -> &[f32] {
        &self.positions
    }

    pub fn mesh_positions(&self, model: &CompiledModel, mesh: usize) -> &[f32] {
        let def = &model.meshes[mesh];
        let (start, count) = (def.vertex_start as usize, def.vertex_count as usize);
        &self.positions[start * 2..(start + count) * 2]
    }

    /// Opacity (final), draw order, multiply color, screen color.
    pub fn mesh_attrs(&self, mesh: usize) -> &MeshAttrs {
        &self.attrs[mesh]
    }

    /// Evaluates without advancing time: physics holds its pose.
    pub fn evaluate(&mut self, model: &CompiledModel) {
        self.advance(model, 0.0);
    }

    /// Evaluates after `dt` seconds. Physics runs in fixed steps and keeps
    /// the time left over (ADR-0022).
    pub fn advance(&mut self, model: &CompiledModel, dt: f32) {
        let Frame {
            values,
            clamped,
            corners,
            corner_counts,
            blend_weights,
            part_world,
            warp_local,
            warp_world,
            warp_opacity,
            rot_world,
            rot_opacity,
            bone_world,
            bone_skin,
            scratch,
            positions,
            attrs,
            prev_positions,
            prev_attrs,
            order,
            prev_order,
            evaluated,
            physics,
            order_changed,
            geometry_changed,
            style_changed,
        } = self;

        std::mem::swap(positions, prev_positions);
        std::mem::swap(attrs, prev_attrs);

        for d in model.drivers.iter() {
            let ops = &model.ops[d.start as usize..(d.start + d.len) as usize];
            values[d.output as usize] = expr::eval(ops, values);
        }

        simulate::advance(model, physics, values, dt);

        for (i, p) in model.parameters.iter().enumerate() {
            clamped[i] = p.apply(values[i]);
        }

        for (b, def) in model.bindings.iter().enumerate() {
            let start = def.corner_start as usize;
            let slice = &mut corners[start..start + def.max_corners as usize];
            corner_counts[b] = compute_corners(model, def, clamped, slice);
        }

        for (i, blend) in model.blends.iter().enumerate() {
            blend_weights[i] = piecewise(
                model.blend_keys_of(blend),
                model.blend_weights_of(blend),
                clamped[blend.parameter as usize],
            );
        }

        for (i, &parent) in model.part_parents.iter().enumerate() {
            let inherited = if parent == NONE {
                1.0
            } else {
                part_world[parent as usize]
            };
            part_world[i] = model.part_opacity[i] * inherited;
        }

        for (i, bone) in model.bones.iter().enumerate() {
            let pose = match bone.keyforms.get() {
                Some(k) => {
                    let cs = corner_slice(model, corners, corner_counts, k);
                    blend_bone(cs, &model.bone_kf, k.first as usize)
                }
                None => bone.base,
            };
            let local = Affine::from_bone(&pose);
            let world = match bone.parent() {
                Some(p) => bone_world[p].then_after(&local),
                None => local,
            };
            bone_world[i] = world;
            bone_skin[i] = world.then_after(&bone.rest_inverse);
        }

        for deformer in model.deformer_order.iter().map(|d| d.get()) {
            match deformer {
                ParentRef::None => {}
                ParentRef::Warp(w) => {
                    let def = &model.warps[w];
                    let n = def.point_count as usize * 2;
                    let start = def.point_start as usize;
                    let range = start..start + n;
                    let local = &mut warp_local[range.clone()];
                    let mut opacity = def.base_opacity;
                    match def.keyforms() {
                        Some(k) => {
                            let cs = corner_slice(model, corners, corner_counts, k);
                            blend(local, &model.warp_kf_points, def.kf_points as usize, cs);
                            opacity = scalar(cs, &model.warp_kf_opacity, k.first as usize);
                        }
                        None => local.copy_from_slice(&model.warp_base_points[range.clone()]),
                    }
                    for &bi in model.warp_blends(def) {
                        let bi = bi as usize;
                        let delta = model.blends[bi].delta_start as usize;
                        add_scaled(
                            local,
                            &model.blend_deltas[delta..delta + n],
                            blend_weights[bi],
                        );
                    }
                    {
                        let world = World {
                            model,
                            warp_world,
                            rot_world,
                        };
                        for i in 0..def.point_count as usize {
                            let q = world.apply(def.parent(), [local[2 * i], local[2 * i + 1]]);
                            scratch[2 * i] = q[0];
                            scratch[2 * i + 1] = q[1];
                        }
                    }
                    warp_world[range].copy_from_slice(&scratch[..n]);
                    warp_opacity[w] =
                        opacity * parent_opacity(def.parent(), warp_opacity, rot_opacity);
                }
                ParentRef::Rotation(r) => {
                    let def = &model.rotations[r];
                    let kf = match def.keyforms() {
                        Some(k) => {
                            let cs = corner_slice(model, corners, corner_counts, k);
                            blend_rotation(cs, &model.rotation_kf, k.first as usize)
                        }
                        None => def.base.into(),
                    };
                    let (sin, cos) = kf.angle_degrees.to_radians().sin_cos();
                    let sx = if kf.reflect_x { -1.0 } else { 1.0 };
                    let sy = if kf.reflect_y { -1.0 } else { 1.0 };
                    let ex = [kf.scale * cos * sx, kf.scale * sin * sx];
                    let ey = [-kf.scale * sin * sy, kf.scale * cos * sy];
                    let affine = match def.parent() {
                        ParentRef::None => Affine {
                            m: [ex[0], ex[1], ey[0], ey[1]],
                            t: kf.origin,
                        },
                        parent => {
                            let world = World {
                                model,
                                warp_world,
                                rot_world,
                            };
                            let o = world.apply(parent, kf.origin);
                            let step = |axis: [f32; 2]| {
                                let p = world.apply(
                                    parent,
                                    [
                                        kf.origin[0] + AXIS_STEP * axis[0],
                                        kf.origin[1] + AXIS_STEP * axis[1],
                                    ],
                                );
                                [(p[0] - o[0]) / AXIS_STEP, (p[1] - o[1]) / AXIS_STEP]
                            };
                            let (x, y) = (step(ex), step(ey));
                            Affine {
                                m: [x[0], x[1], y[0], y[1]],
                                t: o,
                            }
                        }
                    };
                    rot_world[r] = affine;
                    rot_opacity[r] =
                        kf.opacity * parent_opacity(def.parent(), warp_opacity, rot_opacity);
                }
            }
        }

        for (mi, def) in model.meshes.iter().enumerate() {
            let n = def.vertex_count as usize * 2;
            let range = def.vertex_start as usize * 2..def.vertex_start as usize * 2 + n;
            let out = &mut positions[range.clone()];
            let mut a = def.base_attrs;
            match def.keyforms() {
                Some(k) => {
                    let cs = corner_slice(model, corners, corner_counts, k);
                    blend(out, &model.mesh_kf_positions, def.kf_positions as usize, cs);
                    a = blend_attrs(cs, &model.mesh_kf_attrs, k.first as usize);
                }
                None => out.copy_from_slice(&model.mesh_base_positions[range]),
            }
            for &bi in model.mesh_blends(def) {
                let bi = bi as usize;
                let delta = model.blends[bi].delta_start as usize;
                add_scaled(
                    out,
                    &model.blend_deltas[delta..delta + n],
                    blend_weights[bi],
                );
            }
            if let Some(start) = def.influence_start() {
                let influences = &model.influences[start..start + def.vertex_count as usize];
                for (v, inf) in out.as_chunks_mut::<2>().0.iter_mut().zip(influences) {
                    let q = skin(inf, bone_skin, [v[0], v[1]]);
                    v[0] = q[0];
                    v[1] = q[1];
                }
            }
            if def.parent() != ParentRef::None {
                let world = World {
                    model,
                    warp_world,
                    rot_world,
                };
                for v in out.as_chunks_mut::<2>().0 {
                    let q = world.apply(def.parent(), [v[0], v[1]]);
                    v[0] = q[0];
                    v[1] = q[1];
                }
            }
            a[0] *= parent_opacity(def.parent(), warp_opacity, rot_opacity)
                * part_world[def.part as usize];
            attrs[mi] = a;
        }

        for glue in model.glues.iter() {
            let intensity = match glue.keyforms() {
                Some(k) => scalar(
                    corner_slice(model, corners, corner_counts, k),
                    &model.glue_kf_intensity,
                    k.first as usize,
                ),
                None => glue.intensity,
            };
            let node_start = glue.node_start as usize;
            let nodes = &model.glue_nodes[node_start..node_start + glue.node_count as usize];
            let (a0, b0) = (
                model.meshes[glue.mesh_a as usize].vertex_start as usize,
                model.meshes[glue.mesh_b as usize].vertex_start as usize,
            );
            for node in nodes {
                let ia = (a0 + node.index_a as usize) * 2;
                let ib = (b0 + node.index_b as usize) * 2;
                let pa = [positions[ia], positions[ia + 1]];
                let pb = [positions[ib], positions[ib + 1]];
                for axis in 0..2 {
                    let meet = pa[axis] + node.weight * (pb[axis] - pa[axis]);
                    positions[ia + axis] = pa[axis] + intensity * (meet - pa[axis]);
                    positions[ib + axis] = pb[axis] + intensity * (meet - pb[axis]);
                }
            }
        }

        for (i, def) in model.meshes.iter().enumerate() {
            let (start, count) = (def.vertex_start as usize, def.vertex_count as usize);
            let range = start * 2..(start + count) * 2;
            geometry_changed[i] = positions[range.clone()] != prev_positions[range];
            style_changed[i] = attrs[i] != prev_attrs[i];
        }

        // Draw order ascending, ties by mesh index. Sorting in place keeps the
        // frame allocation-free; the old order is the starting point.
        std::mem::swap(order, prev_order);
        order.sort_unstable_by(|&a, &b| {
            let (a, b) = (a as usize, b as usize);
            attrs[a][1].total_cmp(&attrs[b][1]).then(a.cmp(&b))
        });
        *order_changed = !*evaluated || order != prev_order;
        *evaluated = true;
    }
}

/// Offset, in the parent's units, used to find where a rotation deformer's
/// axes go under a non-affine parent.
const AXIS_STEP: f32 = 0.01;

/// Read access to the world transforms of deformers evaluated so far.
struct World<'a> {
    model: &'a CompiledModel<'a>,
    warp_world: &'a [f32],
    rot_world: &'a [Affine],
}

impl World<'_> {
    fn apply(&self, parent: ParentRef, p: Point) -> Point {
        match parent {
            ParentRef::None => p,
            ParentRef::Warp(i) => {
                let def = &self.model.warps[i];
                let start = def.point_start as usize;
                let grid = &self.warp_world[start..start + def.point_count as usize * 2];
                warp_point(grid, def.rows as usize, def.columns as usize, p)
            }
            ParentRef::Rotation(i) => self.rot_world[i].apply(p),
        }
    }
}

/// Bilinear lookup in a warp's world grid; points outside the unit square
/// extend the nearest cell.
fn warp_point(grid: &[f32], rows: usize, columns: usize, p: Point) -> Point {
    let fu = p[0] * columns as f32;
    let fv = p[1] * rows as f32;
    let i = (fu.floor().max(0.0) as usize).min(columns - 1);
    let j = (fv.floor().max(0.0) as usize).min(rows - 1);
    let (tu, tv) = (fu - i as f32, fv - j as f32);
    let at = |r: usize, c: usize| (r * (columns + 1) + c) * 2;
    let (a, b, c, d) = (at(j, i), at(j, i + 1), at(j + 1, i), at(j + 1, i + 1));
    let mix = |k: usize| {
        (1.0 - tu) * (1.0 - tv) * grid[a + k]
            + tu * (1.0 - tv) * grid[b + k]
            + (1.0 - tu) * tv * grid[c + k]
            + tu * tv * grid[d + k]
    };
    [mix(0), mix(1)]
}

fn parent_opacity(parent: ParentRef, warp_opacity: &[f32], rot_opacity: &[f32]) -> f32 {
    match parent {
        ParentRef::None => 1.0,
        ParentRef::Warp(i) => warp_opacity[i],
        ParentRef::Rotation(i) => rot_opacity[i],
    }
}

/// Key segment containing `value` and the position within it. Values outside
/// the keys stick to the end key.
fn locate(keys: &[f32], value: f32) -> (usize, f32) {
    let last = keys.len() - 1;
    if last == 0 || value.is_nan() || value <= keys[0] {
        return (0, 0.0);
    }
    if value >= keys[last] {
        return (last - 1, 1.0);
    }
    let hi = keys.partition_point(|&k| k <= value);
    let lo = hi - 1;
    (lo, (value - keys[lo]) / (keys[hi] - keys[lo]))
}

/// Fills `out` with the grid points that weigh in and returns how many.
fn compute_corners(
    model: &CompiledModel,
    def: &BindingDef,
    values: &[f32],
    out: &mut [(usize, f32)],
) -> usize {
    out[0] = (0, 1.0);
    let mut count = 1;
    for axis in model.binding_axes(def) {
        let keys = model.axis_keys_of(axis);
        let n = keys.len();
        let (i, t) = locate(keys, values[axis.parameter as usize]);
        let existing = count;
        for k in 0..existing {
            let (index, weight) = out[k];
            out[k] = (index * n + i, weight * (1.0 - t));
            if t > 0.0 {
                out[count] = (index * n + i + 1, weight * t);
                count += 1;
            }
        }
    }
    count
}

fn corner_slice<'a>(
    model: &CompiledModel,
    corners: &'a [(usize, f32)],
    counts: &[usize],
    keyforms: KeyformRef,
) -> &'a [(usize, f32)] {
    let def = &model.bindings[keyforms.binding as usize];
    let start = def.corner_start as usize;
    &corners[start..start + counts[keyforms.binding as usize]]
}

/// `out = sum(weight * keyform)` over the weighing grid points; keyform `k`
/// starts at `first + k * out.len()` in `data`.
fn blend(out: &mut [f32], data: &[f32], first: usize, corners: &[(usize, f32)]) {
    out.fill(0.0);
    let n = out.len();
    for &(k, weight) in corners {
        let src = &data[first + k * n..first + (k + 1) * n];
        for (o, s) in out.iter_mut().zip(src) {
            *o += weight * s;
        }
    }
}

fn add_scaled(out: &mut [f32], delta: &[f32], weight: f32) {
    if weight != 0.0 {
        for (o, d) in out.iter_mut().zip(delta) {
            *o += weight * d;
        }
    }
}

fn scalar(corners: &[(usize, f32)], data: &[f32], first: usize) -> f32 {
    corners.iter().map(|&(k, w)| w * data[first + k]).sum()
}

fn blend_attrs(corners: &[(usize, f32)], data: &[MeshAttrs], first: usize) -> MeshAttrs {
    let mut out = [0.0; 8];
    for &(k, weight) in corners {
        for (o, s) in out.iter_mut().zip(&data[first + k]) {
            *o += weight * s;
        }
    }
    out
}

fn blend_bone(corners: &[(usize, f32)], data: &[BoneTransform], first: usize) -> BoneTransform {
    let mut out = BoneTransform {
        translation: [0.0; 2],
        rotation_degrees: 0.0,
        scale: [0.0; 2],
    };
    for &(k, w) in corners {
        let b = &data[first + k];
        for axis in 0..2 {
            out.translation[axis] += w * b.translation[axis];
            out.scale[axis] += w * b.scale[axis];
        }
        out.rotation_degrees += w * b.rotation_degrees;
    }
    out
}

/// `sum(w * skin * p)` plus the share of `p` the weights leave over; weights
/// summing past 1 are scaled down to 1 (ADR-0021).
fn skin(inf: &Influence, bone_skin: &[Affine], p: Point) -> Point {
    let sum: f32 = inf.weights.iter().sum();
    let scale = if sum > 1.0 { 1.0 / sum } else { 1.0 };
    let rest = (1.0 - sum).max(0.0);
    let mut out = [rest * p[0], rest * p[1]];
    for (&b, &w) in inf.bones.iter().zip(&inf.weights) {
        if b != NO_BONE {
            let q = bone_skin[b as usize].apply(p);
            out[0] += w * scale * q[0];
            out[1] += w * scale * q[1];
        }
    }
    out
}

/// Reflection flags are not interpolated: the heaviest grid point wins.
fn blend_rotation(corners: &[(usize, f32)], data: &[RotationKf], first: usize) -> RotationKeyform {
    let mut out = RotationKeyform {
        origin: [0.0, 0.0],
        angle_degrees: 0.0,
        scale: 0.0,
        reflect_x: false,
        reflect_y: false,
        opacity: 0.0,
    };
    let mut heaviest = 0.0;
    for &(k, weight) in corners {
        let kf = &data[first + k];
        out.origin[0] += weight * kf.origin[0];
        out.origin[1] += weight * kf.origin[1];
        out.angle_degrees += weight * kf.angle_degrees;
        out.scale += weight * kf.scale;
        out.opacity += weight * kf.opacity;
        if weight > heaviest {
            heaviest = weight;
            out.reflect_x = kf.reflect_x != 0;
            out.reflect_y = kf.reflect_y != 0;
        }
    }
    out
}

/// Weight of a blend shape: a polyline through `(keys, weights)`.
fn piecewise(keys: &[f32], weights: &[f32], value: f32) -> f32 {
    let Some(last) = keys.len().checked_sub(1) else {
        return 0.0;
    };
    if value.is_nan() || value <= keys[0] {
        return weights[0];
    }
    if value >= keys[last] {
        return weights[last];
    }
    let hi = keys.partition_point(|&k| k <= value);
    let lo = hi - 1;
    let span = keys[hi] - keys[lo];
    if span <= 0.0 {
        return weights[lo];
    }
    weights[lo] + (weights[hi] - weights[lo]) * (value - keys[lo]) / span
}

#[cfg(test)]
mod tests {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::cell::Cell;

    use super::*;
    use crate::compiled::CompileError;
    use crate::driver::Driver;
    use crate::elements::{Parameter, ParameterId, Part, PartId};
    use crate::keyform::{Axis, Binding, BindingId, KeyformGrid};
    use crate::model::Model;
    use crate::rig::{
        ArtMesh, BlendShape, BlendTarget, Bone, BoneTransform, DeformerRef, Glue, GlueNode,
        MeshKeyform, RotationDeformer, VertexInfluences, WarpDeformer, WarpKeyform,
    };

    thread_local! {
        static ALLOCS: Cell<usize> = const { Cell::new(0) };
    }

    /// Counts allocations per thread so a test can assert there are none.
    struct Counting;

    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            ALLOCS.with(|c| c.set(c.get() + 1));
            unsafe { System.alloc(layout) }
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            unsafe { System.dealloc(ptr, layout) }
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;

    fn new_model() -> (Model, PartId) {
        let mut m = Model::new();
        let part = m.parts.insert("Part", Part::default()).unwrap();
        (m, part)
    }

    fn param(m: &mut Model, name: &str) -> ParameterId {
        m.parameters
            .insert(name, Parameter::new(-1.0, 1.0, 0.0))
            .unwrap()
    }

    fn binding(m: &mut Model, axes: &[(ParameterId, &[f32])]) -> BindingId {
        let axes = axes
            .iter()
            .map(|&(parameter, keys)| Axis {
                parameter,
                keys: keys.to_vec(),
            })
            .collect();
        m.bindings.insert(Binding::new(axes).unwrap())
    }

    fn kf(positions: &[Point]) -> MeshKeyform {
        MeshKeyform {
            positions: positions.to_vec(),
            opacity: 1.0,
            draw_order: 500.0,
            multiply_color: [1.0; 3],
            screen_color: [0.0; 3],
        }
    }

    fn mesh(part: PartId, positions: &[Point]) -> ArtMesh {
        ArtMesh::new(part, positions.to_vec(), positions.to_vec(), vec![])
    }

    const UNIT_GRID: [Point; 4] = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];

    fn warp(part: PartId, parent: Option<DeformerRef>, points: &[Point]) -> WarpDeformer {
        WarpDeformer {
            part,
            parent,
            rows: 1,
            columns: 1,
            bezier: false,
            base_points: points.to_vec(),
            keyforms: None,
        }
    }

    fn rotation(
        part: PartId,
        parent: Option<DeformerRef>,
        origin: Point,
        angle_degrees: f32,
    ) -> RotationDeformer {
        RotationDeformer {
            part,
            parent,
            base: RotationKeyform {
                origin,
                angle_degrees,
                ..RotationKeyform::default()
            },
            keyforms: None,
        }
    }

    fn run(m: &Model, values: &[(&str, f32)]) -> (CompiledModel<'static>, Frame) {
        let c = m.compile().unwrap();
        let mut f = Frame::new(&c);
        for &(name, value) in values {
            f.set_parameter(c.parameter_index(name).unwrap(), value);
        }
        f.evaluate(&c);
        crate::tmod::tests::assert_roundtrip(&c);
        (c, f)
    }

    fn near(a: &[f32], b: &[f32]) {
        assert_eq!(a.len(), b.len(), "{a:?} vs {b:?}");
        for (x, y) in a.iter().zip(b) {
            assert!((x - y).abs() < 1e-3, "{a:?} != {b:?}");
        }
    }

    #[test]
    fn one_axis_interpolation_clamps_outside_the_keys() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let b = binding(&mut m, &[(p, &[-1.0, 1.0])]);
        let mut skin = mesh(part, &[[0.0, 0.0]]);
        skin.keyforms = Some(KeyformGrid::new(
            b,
            vec![kf(&[[0.0, 0.0]]), kf(&[[10.0, 0.0]])],
        ));
        m.meshes.insert("Skin", skin).unwrap();

        for (value, expected) in [
            (0.0, 5.0),
            (-1.0, 0.0),
            (0.5, 7.5),
            (5.0, 10.0),
            (-5.0, 0.0),
        ] {
            let (c, f) = run(&m, &[("P", value)]);
            near(f.mesh_positions(&c, 0), &[expected, 0.0]);
        }
    }

    #[test]
    fn two_axis_interpolation_is_multilinear() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let q = param(&mut m, "Q");
        let b = binding(&mut m, &[(p, &[0.0, 1.0]), (q, &[0.0, 1.0])]);
        let mut skin = mesh(part, &[[0.0, 0.0]]);
        // Row-major: P is the slow axis, Q the fast one.
        let keyforms = [0.0, 10.0, 20.0, 30.0].map(|x| kf(&[[x, 0.0]])).to_vec();
        skin.keyforms = Some(KeyformGrid::new(b, keyforms));
        m.meshes.insert("Skin", skin).unwrap();

        for (p, q, expected) in [
            (0.0, 0.0, 0.0),
            (1.0, 0.0, 20.0),
            (0.0, 1.0, 10.0),
            (0.5, 0.5, 15.0),
            (1.0, 1.0, 30.0),
        ] {
            let (c, f) = run(&m, &[("P", p), ("Q", q)]);
            near(f.mesh_positions(&c, 0), &[expected, 0.0]);
        }
    }

    #[test]
    fn blend_shape_adds_weighted_delta() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let skin = m.meshes.insert("Skin", mesh(part, &[[1.0, 1.0]])).unwrap();
        m.blend_shapes
            .insert(
                "Shape",
                BlendShape {
                    target: BlendTarget::Mesh(skin),
                    parameter: p,
                    keys: vec![0.0, 1.0],
                    weights: vec![0.0, 1.0],
                    deltas: vec![[2.0, 3.0]],
                },
            )
            .unwrap();
        let (c, f) = run(&m, &[("P", 0.5)]);
        near(f.mesh_positions(&c, 0), &[2.0, 2.5]);
        let (c, f) = run(&m, &[("P", -1.0)]);
        near(f.mesh_positions(&c, 0), &[1.0, 1.0]);
    }

    #[test]
    fn warp_parent_maps_unit_square_and_extrapolates() {
        let (mut m, part) = new_model();
        let w = m
            .warps
            .insert(
                "W",
                warp(
                    part,
                    None,
                    &[[0.0, 0.0], [10.0, 0.0], [0.0, 10.0], [10.0, 10.0]],
                ),
            )
            .unwrap();
        let mut skin = mesh(part, &[[0.5, 0.5], [0.0, 1.0], [1.5, 0.5]]);
        skin.parent = Some(DeformerRef::Warp(w));
        m.meshes.insert("Skin", skin).unwrap();
        let (c, f) = run(&m, &[]);
        near(f.mesh_positions(&c, 0), &[5.0, 5.0, 0.0, 10.0, 15.0, 5.0]);
    }

    #[test]
    fn rotation_parent_turns_about_its_origin() {
        let (mut m, part) = new_model();
        let r = m
            .rotations
            .insert("R", rotation(part, None, [1.0, 1.0], 90.0))
            .unwrap();
        let mut skin = mesh(part, &[[1.0, 0.0]]);
        skin.parent = Some(DeformerRef::Rotation(r));
        m.meshes.insert("Skin", skin).unwrap();
        let (c, f) = run(&m, &[]);
        near(f.mesh_positions(&c, 0), &[1.0, 2.0]);
    }

    #[test]
    fn rotation_keyform_angle_interpolates() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let b = binding(&mut m, &[(p, &[-1.0, 1.0])]);
        let at = |angle_degrees| RotationKeyform {
            angle_degrees,
            ..RotationKeyform::default()
        };
        let mut rot = rotation(part, None, [0.0, 0.0], 0.0);
        rot.keyforms = Some(KeyformGrid::new(b, vec![at(0.0), at(180.0)]));
        let r = m.rotations.insert("R", rot).unwrap();
        let mut skin = mesh(part, &[[1.0, 0.0]]);
        skin.parent = Some(DeformerRef::Rotation(r));
        m.meshes.insert("Skin", skin).unwrap();
        let (c, f) = run(&m, &[("P", 0.0)]);
        near(f.mesh_positions(&c, 0), &[0.0, 1.0]);
    }

    #[test]
    fn rotation_under_a_scaling_warp_follows_the_warp() {
        let (mut m, part) = new_model();
        let w = m
            .warps
            .insert(
                "W",
                warp(
                    part,
                    None,
                    &[[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
                ),
            )
            .unwrap();
        let r = m
            .rotations
            .insert(
                "R",
                rotation(part, Some(DeformerRef::Warp(w)), [0.5, 0.5], 0.0),
            )
            .unwrap();
        let mut skin = mesh(part, &[[1.0, 1.0]]);
        skin.parent = Some(DeformerRef::Rotation(r));
        m.meshes.insert("Skin", skin).unwrap();
        // Origin (0.5, 0.5) goes to (1, 1); each axis is stretched twofold.
        let (c, f) = run(&m, &[]);
        near(f.mesh_positions(&c, 0), &[3.0, 3.0]);
    }

    #[test]
    fn a_parent_is_evaluated_before_its_child_whatever_the_table_order() {
        let (mut m, part) = new_model();
        // The warp is inserted first, so it comes first in the table, but it
        // hangs from the rotation inserted after it.
        let w = m.warps.insert("W", warp(part, None, &UNIT_GRID)).unwrap();
        let r = m
            .rotations
            .insert("R", rotation(part, None, [0.0, 0.0], 90.0))
            .unwrap();
        m.warps.get_mut(w).unwrap().parent = Some(DeformerRef::Rotation(r));
        let mut skin = mesh(part, &[[1.0, 0.0]]);
        skin.parent = Some(DeformerRef::Warp(w));
        m.meshes.insert("Skin", skin).unwrap();
        let (c, f) = run(&m, &[]);
        near(f.mesh_positions(&c, 0), &[0.0, 1.0]);
    }

    #[test]
    fn glue_pulls_vertices_toward_the_meeting_point() {
        let (mut m, part) = new_model();
        let a = m.meshes.insert("A", mesh(part, &[[0.0, 0.0]])).unwrap();
        let b = m.meshes.insert("B", mesh(part, &[[10.0, 0.0]])).unwrap();
        let glue = |intensity| Glue {
            mesh_a: a,
            mesh_b: b,
            nodes: vec![GlueNode {
                index_a: 0,
                index_b: 0,
                weight: 0.5,
            }],
            intensity,
            keyforms: None,
        };
        let id = m.glues.insert("G", glue(1.0)).unwrap();
        let (c, f) = run(&m, &[]);
        near(f.positions(), &[5.0, 0.0, 5.0, 0.0]);

        m.glues.get_mut(id).unwrap().intensity = 0.5;
        let (c2, f2) = run(&m, &[]);
        near(f2.positions(), &[2.5, 0.0, 7.5, 0.0]);
        assert_eq!(c.vertex_count(), c2.vertex_count());
    }

    #[test]
    fn opacity_multiplies_along_deformers_and_parts() {
        let (mut m, part) = new_model();
        m.parts.get_mut(part).unwrap().opacity = 0.5;
        let mut rot = rotation(part, None, [0.0, 0.0], 0.0);
        rot.base.opacity = 0.5;
        let r = m.rotations.insert("R", rot).unwrap();
        let mut skin = mesh(part, &[[0.0, 0.0]]);
        skin.parent = Some(DeformerRef::Rotation(r));
        m.meshes.insert("Skin", skin).unwrap();
        let (_, f) = run(&m, &[]);
        assert!((f.mesh_attrs(0)[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn draw_order_and_colors_interpolate() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let b = binding(&mut m, &[(p, &[-1.0, 1.0])]);
        let mut low = kf(&[[0.0, 0.0]]);
        low.draw_order = 100.0;
        low.multiply_color = [0.0; 3];
        let mut high = kf(&[[0.0, 0.0]]);
        high.draw_order = 900.0;
        let mut skin = mesh(part, &[[0.0, 0.0]]);
        skin.keyforms = Some(KeyformGrid::new(b, vec![low, high]));
        m.meshes.insert("Skin", skin).unwrap();
        let (_, f) = run(&m, &[("P", 0.0)]);
        let attrs = f.mesh_attrs(0);
        assert!((attrs[1] - 500.0).abs() < 1e-3);
        near(&attrs[2..5], &[0.5, 0.5, 0.5]);
    }

    #[test]
    fn repeat_parameters_wrap_and_bad_values_fall_back_to_default() {
        let p = ParamDef {
            min: -180.0,
            max: 180.0,
            default: 0.0,
            repeat: 1,
        };
        assert!((p.apply(190.0) - -170.0).abs() < 1e-4);
        assert_eq!(p.apply(f32::NAN), 0.0);
        let q = ParamDef { repeat: 0, ..p };
        assert_eq!(q.apply(190.0), 180.0);
    }

    #[test]
    fn an_inconsistent_model_does_not_compile() {
        let (mut m, part) = new_model();
        m.meshes.insert("Skin", mesh(part, &[[0.0, 0.0]])).unwrap();
        m.parts.remove(part);
        assert!(matches!(
            m.compile(),
            Err(crate::compiled::CompileError::Invalid(_))
        ));
    }

    fn rich_model() -> Model {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let q = param(&mut m, "Q");
        let b = binding(&mut m, &[(p, &[-1.0, 1.0]), (q, &[-1.0, 1.0])]);
        let mut rot = rotation(part, None, [0.0, 0.0], 0.0);
        let angles = (0..4).map(|i| RotationKeyform {
            angle_degrees: i as f32 * 10.0,
            ..RotationKeyform::default()
        });
        rot.keyforms = Some(KeyformGrid::new(b, angles.collect()));
        let rot = m.rotations.insert("R", rot).unwrap();
        let mut w = warp(part, Some(DeformerRef::Rotation(rot)), &UNIT_GRID);
        let warp_kf = WarpKeyform {
            points: UNIT_GRID.to_vec(),
            opacity: 1.0,
        };
        w.keyforms = Some(KeyformGrid::new(b, vec![warp_kf; 4]));
        let w = m.warps.insert("W", w).unwrap();
        let positions = [[0.2, 0.2], [0.8, 0.2], [0.5, 0.9]];
        let mut a = mesh(part, &positions);
        a.parent = Some(DeformerRef::Warp(w));
        a.keyforms = Some(KeyformGrid::new(b, vec![kf(&positions); 4]));
        let a = m.meshes.insert("A", a).unwrap();
        let mut other = mesh(part, &positions);
        other.parent = Some(DeformerRef::Rotation(rot));
        let other = m.meshes.insert("B", other).unwrap();
        let node = GlueNode {
            index_a: 0,
            index_b: 0,
            weight: 0.5,
        };
        let glue = Glue {
            mesh_a: a,
            mesh_b: other,
            nodes: vec![node],
            intensity: 1.0,
            keyforms: None,
        };
        m.glues.insert("G", glue).unwrap();
        let shape = BlendShape {
            target: BlendTarget::Mesh(a),
            parameter: q,
            keys: vec![0.0, 1.0],
            weights: vec![0.0, 1.0],
            deltas: vec![[0.1, 0.0]; 3],
        };
        m.blend_shapes.insert("S", shape).unwrap();
        assert!(m.validate().is_empty());
        m
    }

    fn driver(m: &mut Model, name: &str, output: &str, expression: &str) {
        let output = m.parameters.find(output).unwrap();
        let driver = Driver {
            output,
            expression: expression.into(),
        };
        m.drivers.insert(name, driver).unwrap();
    }

    /// `Out` (keys 0 and 1) moves a vertex from x = 0 to x = 10.
    fn driven_model() -> Model {
        let (mut m, part) = new_model();
        param(&mut m, "In");
        let out = m
            .parameters
            .insert("Out", Parameter::new(0.0, 1.0, 0.0))
            .unwrap();
        let b = binding(&mut m, &[(out, &[0.0, 1.0])]);
        let mut skin = mesh(part, &[[0.0, 0.0]]);
        skin.keyforms = Some(KeyformGrid::new(
            b,
            vec![kf(&[[0.0, 0.0]]), kf(&[[10.0, 0.0]])],
        ));
        m.meshes.insert("Skin", skin).unwrap();
        m
    }

    #[test]
    fn driver_output_feeds_keyform_interpolation() {
        let mut m = driven_model();
        driver(&mut m, "D", "Out", "In / 2");
        let (c, f) = run(&m, &[("In", 1.0)]);
        near(f.mesh_positions(&c, 0), &[5.0, 0.0]);
        // The caller's own value for a driven parameter is overwritten.
        let (c, f) = run(&m, &[("In", 1.0), ("Out", 1.0)]);
        near(f.mesh_positions(&c, 0), &[5.0, 0.0]);
        // A driven value outside the parameter's range is clamped afterwards.
        let (c, f) = run(&m, &[("In", 100.0)]);
        near(f.mesh_positions(&c, 0), &[10.0, 0.0]);
    }

    #[test]
    fn drivers_run_after_the_drivers_they_read() {
        // Inserted in the opposite order of their dependency.
        let mut m = driven_model();
        m.parameters
            .insert("Mid", Parameter::new(-10.0, 10.0, 0.0))
            .unwrap();
        driver(&mut m, "A", "Out", "Mid * 0.5");
        driver(&mut m, "B", "Mid", "In + 1");
        let (c, f) = run(&m, &[("In", 0.5)]);
        near(f.mesh_positions(&c, 0), &[7.5, 0.0]);
        let order: Vec<_> = c
            .drivers
            .iter()
            .map(|d| c.parameter_name(d.output as usize))
            .collect();
        assert_eq!(order, ["Mid", "Out"]);
    }

    #[test]
    fn driver_compile_errors() {
        let mut m = driven_model();
        driver(&mut m, "Bad", "Out", "In +");
        assert!(matches!(
            m.compile(),
            Err(CompileError::Expression { driver, .. }) if driver == "Bad"
        ));

        let mut m = driven_model();
        driver(&mut m, "Bad", "Out", "Nope");
        assert!(matches!(
            m.compile(),
            Err(CompileError::Expression { error, .. })
                if error.kind == expr::ExprErrorKind::UnknownName("Nope".into())
        ));

        let mut m = driven_model();
        driver(&mut m, "One", "Out", "1");
        driver(&mut m, "Two", "Out", "2");
        assert!(matches!(
            m.compile(),
            Err(CompileError::DuplicateDriverOutput { driver }) if driver == "Two"
        ));

        let mut m = driven_model();
        driver(&mut m, "Self", "Out", "Out + 1");
        assert!(matches!(m.compile(), Err(CompileError::DriverCycle { .. })));

        let mut m = driven_model();
        driver(&mut m, "A", "Out", "In");
        driver(&mut m, "B", "In", "Out");
        assert!(matches!(m.compile(), Err(CompileError::DriverCycle { .. })));
    }

    #[test]
    fn steady_state_evaluation_with_drivers_allocates_nothing() {
        let mut m = rich_model();
        driver(&mut m, "D", "Q", "clamp(P * 2, -1, 1) + sin(P * 90)");
        let c = m.compile().unwrap();
        let mut frame = Frame::new(&c);
        frame.evaluate(&c);

        let before = ALLOCS.with(Cell::get);
        for i in 0..100 {
            frame.set_parameter(0, i as f32 * 0.02 - 1.0);
            frame.evaluate(&c);
        }
        assert_eq!(ALLOCS.with(Cell::get) - before, 0);
        assert!(frame.positions().iter().all(|v| v.is_finite()));
    }

    fn at(x: f32, y: f32) -> BoneTransform {
        BoneTransform {
            translation: [x, y],
            ..BoneTransform::default()
        }
    }

    fn turned(degrees: f32) -> BoneTransform {
        BoneTransform {
            rotation_degrees: degrees,
            ..BoneTransform::default()
        }
    }

    /// One vertex at `(1, 0)` skinned to `bones` with the given weights.
    fn skinned(part: PartId, bones: &[(crate::rig::BoneId, f32)]) -> ArtMesh {
        let mut skin = mesh(part, &[[1.0, 0.0]]);
        let mut inf = VertexInfluences::default();
        for (slot, &(bone, weight)) in bones.iter().enumerate() {
            inf.bones[slot] = Some(bone);
            inf.weights[slot] = weight;
        }
        skin.influences = vec![inf];
        skin
    }

    /// A bone that moves by `(2, 0)` as `P` goes from -1 to 1.
    fn driven_bone(
        m: &mut Model,
        name: &str,
        p: ParameterId,
        base: BoneTransform,
    ) -> crate::rig::BoneId {
        let b = binding(m, &[(p, &[-1.0, 1.0])]);
        m.bones
            .insert(
                name,
                Bone {
                    parent: None,
                    base,
                    keyforms: Some(KeyformGrid::new(
                        b,
                        vec![
                            base,
                            BoneTransform {
                                translation: [base.translation[0] + 2.0, base.translation[1]],
                                ..base
                            },
                        ],
                    )),
                },
            )
            .unwrap()
    }

    #[test]
    fn a_bone_at_rest_leaves_the_vertex_alone() {
        let (mut m, part) = new_model();
        let bone = m
            .bones
            .insert(
                "B",
                Bone {
                    parent: None,
                    base: at(5.0, 3.0),
                    keyforms: None,
                },
            )
            .unwrap();
        m.meshes
            .insert("Skin", skinned(part, &[(bone, 1.0)]))
            .unwrap();
        let (c, f) = run(&m, &[]);
        near(f.mesh_positions(&c, 0), &[1.0, 0.0]);
    }

    #[test]
    fn bone_keyforms_move_weighted_vertices() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let bone = driven_bone(&mut m, "B", p, at(0.0, 0.0));
        m.meshes
            .insert("Full", skinned(part, &[(bone, 1.0)]))
            .unwrap();
        m.meshes
            .insert("Half", skinned(part, &[(bone, 0.5)]))
            .unwrap();
        for (value, full, half) in [(-1.0, 1.0, 1.0), (0.0, 2.0, 1.5), (1.0, 3.0, 2.0)] {
            let (c, f) = run(&m, &[("P", value)]);
            near(f.mesh_positions(&c, 0), &[full, 0.0]);
            near(f.mesh_positions(&c, 1), &[half, 0.0]);
        }
    }

    #[test]
    fn weights_past_one_are_scaled_down() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let a = driven_bone(&mut m, "A", p, at(0.0, 0.0));
        let b = driven_bone(&mut m, "B", p, at(0.0, 0.0));
        m.meshes
            .insert("Skin", skinned(part, &[(a, 1.0), (b, 1.0)]))
            .unwrap();
        let (c, f) = run(&m, &[("P", 1.0)]);
        near(f.mesh_positions(&c, 0), &[3.0, 0.0]);
    }

    #[test]
    fn a_child_bone_follows_its_parent() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let b = binding(&mut m, &[(p, &[-1.0, 1.0])]);
        let parent = m
            .bones
            .insert(
                "Parent",
                Bone {
                    parent: None,
                    base: BoneTransform::default(),
                    keyforms: Some(KeyformGrid::new(b, vec![turned(0.0), turned(90.0)])),
                },
            )
            .unwrap();
        let child = m
            .bones
            .insert(
                "Child",
                Bone {
                    parent: Some(parent),
                    base: at(1.0, 0.0),
                    keyforms: None,
                },
            )
            .unwrap();
        m.meshes
            .insert("Skin", skinned(part, &[(child, 1.0)]))
            .unwrap();
        let (c, f) = run(&m, &[("P", 1.0)]);
        near(f.mesh_positions(&c, 0), &[0.0, 1.0]);
    }

    #[test]
    fn skinning_comes_before_the_parent_deformer() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let bone = driven_bone(&mut m, "B", p, at(0.0, 0.0));
        let rot = m
            .rotations
            .insert("R", rotation(part, None, [0.0, 0.0], 90.0))
            .unwrap();
        let mut skin = skinned(part, &[(bone, 1.0)]);
        skin.parent = Some(DeformerRef::Rotation(rot));
        m.meshes.insert("Skin", skin).unwrap();
        // (1, 0) + (2, 0) in the deformer's space, then turned a quarter.
        let (c, f) = run(&m, &[("P", 1.0)]);
        near(f.mesh_positions(&c, 0), &[0.0, 3.0]);
    }

    #[test]
    fn bone_compile_errors() {
        let (mut m, part) = new_model();
        let flat = BoneTransform {
            scale: [0.0, 1.0],
            ..BoneTransform::default()
        };
        m.bones
            .insert(
                "Flat",
                Bone {
                    parent: None,
                    base: flat,
                    keyforms: None,
                },
            )
            .unwrap();
        m.meshes.insert("Skin", mesh(part, &[[0.0, 0.0]])).unwrap();
        assert!(matches!(
            m.compile(),
            Err(CompileError::DegenerateBone { bone }) if bone == "Flat"
        ));

        let (mut m, part) = new_model();
        let bone = m
            .bones
            .insert(
                "B",
                Bone {
                    parent: None,
                    base: at(0.0, 0.0),
                    keyforms: None,
                },
            )
            .unwrap();
        let rot = m
            .rotations
            .insert("R", rotation(part, None, [0.0, 0.0], 0.0))
            .unwrap();
        m.meshes
            .insert("Free", skinned(part, &[(bone, 1.0)]))
            .unwrap();
        let mut under = skinned(part, &[(bone, 1.0)]);
        under.parent = Some(DeformerRef::Rotation(rot));
        m.meshes.insert("Under", under).unwrap();
        assert!(matches!(
            m.compile(),
            Err(CompileError::BoneSpaceMismatch { bone }) if bone == "B"
        ));
    }

    #[test]
    fn steady_state_evaluation_with_bones_allocates_nothing() {
        let (mut m, part) = new_model();
        let p = param(&mut m, "P");
        let bone = driven_bone(&mut m, "B", p, at(0.0, 0.0));
        m.meshes
            .insert("Skin", skinned(part, &[(bone, 1.0)]))
            .unwrap();
        let c = m.compile().unwrap();
        let mut frame = Frame::new(&c);
        frame.evaluate(&c);
        let before = ALLOCS.with(Cell::get);
        for i in 0..100 {
            frame.set_parameter(0, i as f32 * 0.02 - 1.0);
            frame.evaluate(&c);
        }
        assert_eq!(ALLOCS.with(Cell::get) - before, 0);
    }

    #[test]
    fn steady_state_evaluation_with_physics_allocates_nothing() {
        let c = crate::simulate::sample_model(0.95).compile().unwrap();
        let mut frame = Frame::new(&c);
        frame.evaluate(&c);
        let before = ALLOCS.with(Cell::get);
        for i in 0..100 {
            frame.set_parameter(0, (i % 7) as f32 * 0.3 - 1.0);
            frame.advance(&c, 0.02);
        }
        assert_eq!(ALLOCS.with(Cell::get) - before, 0);
    }

    #[test]
    fn steady_state_evaluation_allocates_nothing() {
        let c = rich_model().compile().unwrap();
        let mut frame = Frame::new(&c);
        frame.evaluate(&c);

        let before = ALLOCS.with(Cell::get);
        for i in 0..100 {
            frame.set_parameter(0, i as f32 * 0.02 - 1.0);
            frame.set_parameter(1, 0.3);
            frame.evaluate(&c);
        }
        assert_eq!(ALLOCS.with(Cell::get) - before, 0);
        assert!(frame.positions().iter().all(|v| v.is_finite()));
    }
}
