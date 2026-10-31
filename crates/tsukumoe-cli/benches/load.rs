// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Loading the reference model's `.tmod` (REQ-PERF-005): reading the file and
//! validating it. The embedded textures stay PNG bytes, so nothing is decoded.
//! The CI ceilings are in `scripts/bench-budget.json`.

use std::hint::black_box;

use criterion::{Criterion, criterion_group, criterion_main};
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_core::tmod::{self, AlignedBuf, PlaybackData, Tmod};

fn benches(c: &mut Criterion) {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (project, model) = project(&table).unwrap();
    let compiled = model.compile().unwrap();
    let textures: Vec<&[u8]> = project
        .files
        .iter()
        .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
        .map(|(_, bytes)| bytes.as_slice())
        .collect();
    let bytes = tmod::write_with(
        &compiled,
        &PlaybackData::from_model(&model).unwrap(),
        &textures,
    )
    .unwrap();
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("reference.tmod");
    std::fs::write(&path, &bytes).unwrap();
    let aligned = AlignedBuf::from_bytes(&bytes);

    // The file is in the page cache after the first run, as for a second load.
    c.bench_function("reference_load", |b| {
        b.iter(|| {
            let buf = AlignedBuf::read_file(&path).unwrap();
            black_box(Tmod::parse(&buf).unwrap().model.meshes.len());
        })
    });
    c.bench_function("reference_parse", |b| {
        b.iter(|| black_box(Tmod::parse(&aligned).unwrap().model.meshes.len()))
    });
}

criterion_group!(group, benches);
criterion_main!(group);
