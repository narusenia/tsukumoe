// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The published JSON Schema (`schema/*.json`) must match the Rust types.
//! Regenerate with `TSUKUMOE_UPDATE_SCHEMA=1 cargo test -p tsukumoe-format --test schema`.

use std::fs;
use std::path::{Path, PathBuf};

use schemars::{JsonSchema, schema_for};
use tsukumoe_format::dto::{ExpressionFile, ModelFile, MotionFile, PhysicsFile};
use tsukumoe_format::{Manifest, Project, read_model};

fn schema_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema")
}

fn check<T: JsonSchema>(file: &str) {
    let mut text = serde_json::to_string_pretty(&schema_for!(T)).unwrap();
    text.push('\n');
    let path = schema_dir().join(file);
    if std::env::var_os("TSUKUMOE_UPDATE_SCHEMA").is_some() {
        fs::create_dir_all(schema_dir()).unwrap();
        fs::write(&path, &text).unwrap();
    }
    let committed = fs::read_to_string(&path).unwrap_or_default();
    assert!(
        committed == text,
        "{file} is out of date; run with TSUKUMOE_UPDATE_SCHEMA=1 to regenerate"
    );
}

#[test]
fn published_schemas_match_the_types() {
    check::<Manifest>("manifest.schema.json");
    check::<ModelFile>("model.schema.json");
    check::<PhysicsFile>("physics.schema.json");
    check::<MotionFile>("motion.schema.json");
    check::<ExpressionFile>("expression.schema.json");
}

#[test]
fn the_hand_written_fixture_loads() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/minimal");
    let model = read_model(&Project::read_dir(&dir).unwrap()).unwrap();
    assert_eq!(model.parameters().len(), 2);
}
