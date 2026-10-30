// Copyright 2026 Seimei Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame evaluation. The rules are in `docs/specifications/evaluation.md`.

use crate::compiled::{BindingDef, CompiledModel, KeyformRef, MeshAttrs, ParamDef, ParentRef};
use crate::elements::Point;
use crate::rig::RotationKeyform;

/// Maps a rotation deformer's local point to world space:
/// `[m0 x + m2 y + t0, m1 x + m3 y + t1]`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct Affine {
    m: [f32; 4],
    t: [f32; 2],
}

impl Affine {
    fn apply(&self, p: Point) -> Point {
        [
            self.m[0] * p[0] + self.m[2] * p[1] + self.t[0],
            self.m[1] * p[0] + self.m[3] * p[1] + self.t[1],
        ]
    }
}

impl ParamDef {
    fn apply(&self, value: f32) -> f32 {
        if !value.is_finite() {
            return self.default;
        }
        if self.repeat {
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
    scratch: Vec<f32>,
    positions: Vec<f32>,
    attrs: Vec<MeshAttrs>,
}

impl Frame {
    pub fn new(model: &CompiledModel) -> Self {
        let max_points = model
            .warps
            .iter()
            .map(|w| w.point_count * 2)
            .max()
            .unwrap_or(0);
        Self {
            values: model.parameters.iter().map(|p| p.default).collect(),
            clamped: vec![0.0; model.parameters.len()],
            corners: vec![(0, 0.0); model.corner_capacity],
            corner_counts: vec![0; model.bindings.len()],
            blend_weights: vec![0.0; model.blends.len()],
            part_world: vec![1.0; model.part_opacity.len()],
            warp_local: vec![0.0; model.warp_base_points.len()],
            warp_world: vec![0.0; model.warp_base_points.len()],
            warp_opacity: vec![1.0; model.warps.len()],
            rot_world: vec![Affine::default(); model.rotations.len()],
            rot_opacity: vec![1.0; model.rotations.len()],
            scratch: vec![0.0; max_points],
            positions: vec![0.0; model.mesh_base_positions.len()],
            attrs: vec![[0.0; 8]; model.meshes.len()],
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
        &self.positions[def.vertex_start * 2..(def.vertex_start + def.vertex_count) * 2]
    }

    /// Opacity (final), draw order, multiply color, screen color.
    pub fn mesh_attrs(&self, mesh: usize) -> &MeshAttrs {
        &self.attrs[mesh]
    }

    pub fn evaluate(&mut self, model: &CompiledModel) {
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
            scratch,
            positions,
            attrs,
        } = self;

        for (i, p) in model.parameters.iter().enumerate() {
            clamped[i] = p.apply(values[i]);
        }

        for (b, def) in model.bindings.iter().enumerate() {
            let slice = &mut corners[def.corner_start..def.corner_start + def.max_corners];
            corner_counts[b] = compute_corners(def, clamped, slice);
        }

        for (i, blend) in model.blends.iter().enumerate() {
            blend_weights[i] = piecewise(&blend.keys, &blend.weights, clamped[blend.parameter]);
        }

        for (i, parent) in model.part_parents.iter().enumerate() {
            let inherited = parent.map_or(1.0, |p| part_world[p]);
            part_world[i] = model.part_opacity[i] * inherited;
        }

        for &deformer in &model.deformer_order {
            match deformer {
                ParentRef::None => {}
                ParentRef::Warp(w) => {
                    let def = &model.warps[w];
                    let n = def.point_count * 2;
                    let range = def.point_start..def.point_start + n;
                    let local = &mut warp_local[range.clone()];
                    let mut opacity = def.base_opacity;
                    match def.keyforms {
                        Some(k) => {
                            let cs = corner_slice(model, corners, corner_counts, k);
                            blend(local, &model.warp_kf_points, def.kf_points, cs);
                            opacity = scalar(cs, &model.warp_kf_opacity, k.first);
                        }
                        None => local.copy_from_slice(&model.warp_base_points[range.clone()]),
                    }
                    for &bi in &def.blends {
                        let delta = model.blends[bi].delta_start;
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
                        for i in 0..def.point_count {
                            let q = world.apply(def.parent, [local[2 * i], local[2 * i + 1]]);
                            scratch[2 * i] = q[0];
                            scratch[2 * i + 1] = q[1];
                        }
                    }
                    warp_world[range].copy_from_slice(&scratch[..n]);
                    warp_opacity[w] =
                        opacity * parent_opacity(def.parent, warp_opacity, rot_opacity);
                }
                ParentRef::Rotation(r) => {
                    let def = &model.rotations[r];
                    let kf = match def.keyforms {
                        Some(k) => {
                            let cs = corner_slice(model, corners, corner_counts, k);
                            blend_rotation(cs, &model.rotation_kf, k.first)
                        }
                        None => def.base,
                    };
                    let (sin, cos) = kf.angle_degrees.to_radians().sin_cos();
                    let sx = if kf.reflect_x { -1.0 } else { 1.0 };
                    let sy = if kf.reflect_y { -1.0 } else { 1.0 };
                    let ex = [kf.scale * cos * sx, kf.scale * sin * sx];
                    let ey = [-kf.scale * sin * sy, kf.scale * cos * sy];
                    let affine = match def.parent {
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
                        kf.opacity * parent_opacity(def.parent, warp_opacity, rot_opacity);
                }
            }
        }

        for (mi, def) in model.meshes.iter().enumerate() {
            let n = def.vertex_count * 2;
            let range = def.vertex_start * 2..def.vertex_start * 2 + n;
            let out = &mut positions[range.clone()];
            let mut a = def.base_attrs;
            match def.keyforms {
                Some(k) => {
                    let cs = corner_slice(model, corners, corner_counts, k);
                    blend(out, &model.mesh_kf_positions, def.kf_positions, cs);
                    a = blend_attrs(cs, &model.mesh_kf_attrs, k.first);
                }
                None => out.copy_from_slice(&model.mesh_base_positions[range]),
            }
            for &bi in &def.blends {
                let delta = model.blends[bi].delta_start;
                add_scaled(
                    out,
                    &model.blend_deltas[delta..delta + n],
                    blend_weights[bi],
                );
            }
            if def.parent != ParentRef::None {
                let world = World {
                    model,
                    warp_world,
                    rot_world,
                };
                for v in out.as_chunks_mut::<2>().0 {
                    let q = world.apply(def.parent, [v[0], v[1]]);
                    v[0] = q[0];
                    v[1] = q[1];
                }
            }
            a[0] *= parent_opacity(def.parent, warp_opacity, rot_opacity) * part_world[def.part];
            attrs[mi] = a;
        }

        for glue in &model.glues {
            let intensity = match glue.keyforms {
                Some(k) => scalar(
                    corner_slice(model, corners, corner_counts, k),
                    &model.glue_kf_intensity,
                    k.first,
                ),
                None => glue.intensity,
            };
            let nodes = &model.glue_nodes[glue.node_start..glue.node_start + glue.node_count];
            let (a0, b0) = (
                model.meshes[glue.mesh_a].vertex_start,
                model.meshes[glue.mesh_b].vertex_start,
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
    }
}

/// Offset, in the parent's units, used to find where a rotation deformer's
/// axes go under a non-affine parent.
const AXIS_STEP: f32 = 0.01;

/// Read access to the world transforms of deformers evaluated so far.
struct World<'a> {
    model: &'a CompiledModel,
    warp_world: &'a [f32],
    rot_world: &'a [Affine],
}

impl World<'_> {
    fn apply(&self, parent: ParentRef, p: Point) -> Point {
        match parent {
            ParentRef::None => p,
            ParentRef::Warp(i) => {
                let def = &self.model.warps[i];
                let grid = &self.warp_world[def.point_start..def.point_start + def.point_count * 2];
                warp_point(grid, def.rows, def.columns, p)
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
    if last == 0 || value <= keys[0] {
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
fn compute_corners(def: &BindingDef, values: &[f32], out: &mut [(usize, f32)]) -> usize {
    out[0] = (0, 1.0);
    let mut count = 1;
    for axis in &def.axes {
        let n = axis.keys.len();
        let (i, t) = locate(&axis.keys, values[axis.parameter]);
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
    let def = &model.bindings[keyforms.binding];
    &corners[def.corner_start..def.corner_start + counts[keyforms.binding]]
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

/// Reflection flags are not interpolated: the heaviest grid point wins.
fn blend_rotation(
    corners: &[(usize, f32)],
    data: &[RotationKeyform],
    first: usize,
) -> RotationKeyform {
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
            out.reflect_x = kf.reflect_x;
            out.reflect_y = kf.reflect_y;
        }
    }
    out
}

/// Weight of a blend shape: a polyline through `(keys, weights)`.
fn piecewise(keys: &[f32], weights: &[f32], value: f32) -> f32 {
    let Some(last) = keys.len().checked_sub(1) else {
        return 0.0;
    };
    if value <= keys[0] {
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
    use crate::elements::{Parameter, ParameterId, Part, PartId};
    use crate::keyform::{Axis, Binding, BindingId, KeyformGrid};
    use crate::model::Model;
    use crate::rig::{
        ArtMesh, BlendShape, BlendTarget, DeformerRef, Glue, GlueNode, MeshKeyform,
        RotationDeformer, WarpDeformer, WarpKeyform,
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

    fn run(m: &Model, values: &[(&str, f32)]) -> (CompiledModel, Frame) {
        let c = m.compile().unwrap();
        let mut f = Frame::new(&c);
        for &(name, value) in values {
            f.set_parameter(c.parameter_index(name).unwrap(), value);
        }
        f.evaluate(&c);
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
            repeat: true,
        };
        assert!((p.apply(190.0) - -170.0).abs() < 1e-4);
        assert_eq!(p.apply(f32::NAN), 0.0);
        let q = ParamDef { repeat: false, ..p };
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
