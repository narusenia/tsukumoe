// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame evaluation on the reference model (REQ-PERF-001 / REQ-PERF-002),
//! generated from `assets/reference-model/parts.json`. The CI ceilings are in
//! `scripts/bench-budget.json`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tsukumoe_cli::generate::{Table, model};
use tsukumoe_core::eval::Frame;

fn benches(c: &mut Criterion) {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let compiled = model(&table).unwrap().compile().unwrap();
    let mut frame = Frame::new(&compiled);

    // Guard: the parameters must move the vertices, or nothing is measured.
    let positions_at = |frame: &mut Frame, v: f32| {
        frame.parameters_mut().fill(v);
        frame.evaluate(&compiled);
        frame.draw_list(&compiled).positions().to_vec()
    };
    assert_ne!(positions_at(&mut frame, 0.0), positions_at(&mut frame, 1.0));

    let mut t = 0.0_f32;
    let mut sweep = move |frame: &mut Frame| {
        // Every parameter changes every frame, so nothing is cached.
        t = (t + 0.013) % 1.0;
        for (i, v) in frame.parameters_mut().iter_mut().enumerate() {
            *v = if i % 2 == 0 { t } else { 1.0 - t };
        }
    };
    c.bench_function("reference_evaluate", |b| {
        b.iter(|| {
            sweep(&mut frame);
            frame.evaluate(&compiled);
            let list = frame.draw_list(&compiled);
            black_box((list.positions(), list.order()));
        })
    });
    c.bench_function("reference_advance_16ms", |b| {
        b.iter(|| {
            sweep(&mut frame);
            frame.advance(&compiled, 1.0 / 60.0);
            black_box(frame.draw_list(&compiled).positions());
        })
    });
}

criterion_group!(group, benches);
criterion_main!(group);
