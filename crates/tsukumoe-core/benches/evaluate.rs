// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame evaluation on a synthetic model of reference-model size: 100 meshes
//! of 196 vertices (about 20k vertices) under one warp deformer, every mesh
//! driven by two parameters. The CI budget is in `benches/budget.json`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tsukumoe_core::compiled::CompiledModel;
use tsukumoe_core::elements::{Parameter, Part, Point};
use tsukumoe_core::eval::Frame;
use tsukumoe_core::keyform::{Axis, Binding, KeyformGrid};
use tsukumoe_core::model::{Model, ModelParts};
use tsukumoe_core::rig::{ArtMesh, DeformerRef, MeshKeyform, WarpDeformer, WarpKeyform};

const MESHES: usize = 100;
const SIDE: usize = 14;

fn grid(scale: f32) -> Vec<Point> {
    let n = SIDE as f32 - 1.0;
    (0..SIDE * SIDE)
        .map(|i| [(i % SIDE) as f32 / n * scale, (i / SIDE) as f32 / n * scale])
        .collect()
}

fn triangles() -> Vec<u32> {
    let mut indices = Vec::new();
    for y in 0..SIDE - 1 {
        for x in 0..SIDE - 1 {
            let i = (y * SIDE + x) as u32;
            let s = SIDE as u32;
            indices.extend([i, i + 1, i + s + 1, i, i + s + 1, i + s]);
        }
    }
    indices
}

fn model() -> Model {
    let mut p = ModelParts::default();
    let part = p.parts.insert("Part", Part::default()).unwrap();
    let a = p
        .parameters
        .insert("A", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    let b = p
        .parameters
        .insert("B", Parameter::new(0.0, 1.0, 0.0))
        .unwrap();
    let axis = |parameter| Axis {
        parameter,
        keys: vec![0.0, 1.0],
    };
    let one_axis = p.bindings.insert(Binding::new(vec![axis(a)]).unwrap());
    let two_axes = p
        .bindings
        .insert(Binding::new(vec![axis(a), axis(b)]).unwrap());

    let warp_points = |scale: f32| -> Vec<Point> {
        (0..9)
            .map(|i| [(i % 3) as f32 * 0.5 * scale, (i / 3) as f32 * 0.5 * scale])
            .collect()
    };
    let warp_keyform = |scale| WarpKeyform {
        points: warp_points(scale),
        opacity: 1.0,
    };
    let warp = p
        .warps
        .insert(
            "Warp",
            WarpDeformer {
                part,
                parent: None,
                rows: 2,
                columns: 2,
                bezier: true,
                base_points: warp_points(1.0),
                rest_points: None,
                keyforms: Some(KeyformGrid::new(
                    one_axis,
                    vec![warp_keyform(1.0), warp_keyform(1.1)],
                )),
            },
        )
        .unwrap();

    let keyform = |scale: f32| MeshKeyform {
        positions: grid(scale),
        opacity: 1.0,
        draw_order: 500.0,
        multiply_color: [1.0; 3],
        screen_color: [0.0; 3],
    };
    for i in 0..MESHES {
        let mut mesh = ArtMesh::new(part, grid(1.0), grid(1.0), triangles());
        mesh.parent = Some(DeformerRef::Warp(warp));
        mesh.draw_order = (i % 7) as f32;
        mesh.keyforms = Some(KeyformGrid::new(
            two_axes,
            vec![keyform(1.0), keyform(1.05), keyform(0.95), keyform(1.1)],
        ));
        p.meshes.insert(format!("Mesh{i}"), mesh).unwrap();
    }
    Model::from_parts(p).unwrap()
}

fn compiled() -> CompiledModel<'static> {
    model().compile().unwrap()
}

fn benches(c: &mut Criterion) {
    let model = model();
    c.bench_function("compile", |b| {
        b.iter(|| black_box(model.compile().unwrap()))
    });

    let compiled = compiled();
    let mut frame = Frame::new(&compiled);
    // Guard: the parameters must move the vertices, or nothing is measured.
    let positions_at = |frame: &mut Frame, v: f32| {
        frame.set_parameter(0, v);
        frame.set_parameter(1, v);
        frame.evaluate(&compiled);
        frame.draw_list(&compiled).positions().to_vec()
    };
    assert_ne!(positions_at(&mut frame, 0.0), positions_at(&mut frame, 1.0));
    let mut t = 0.0_f32;
    let mut sweep = move |frame: &mut Frame| {
        // Both parameters change every frame, so nothing is cached.
        t = (t + 0.013) % 1.0;
        frame.set_parameter(0, t);
        frame.set_parameter(1, 1.0 - t);
    };
    c.bench_function("evaluate", |b| {
        b.iter(|| {
            sweep(&mut frame);
            frame.evaluate(&compiled);
        })
    });
    c.bench_function("evaluate_draw_list", |b| {
        b.iter(|| {
            sweep(&mut frame);
            frame.evaluate(&compiled);
            let list = frame.draw_list(&compiled);
            black_box((list.positions(), list.order()));
        })
    });
    c.bench_function("advance_16ms", |b| {
        b.iter(|| {
            sweep(&mut frame);
            frame.advance(&compiled, 1.0 / 60.0);
        })
    });
}

criterion_group!(group, benches);
criterion_main!(group);
