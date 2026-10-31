// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The native side of the parity test (`docs/implementation/M3-runtime/parity.md`):
//! plays a case on the runtime and writes `<out>.rgba` (premultiplied RGBA8, no
//! header, top row first) and `<out>.json` (the draw list).
//! `parity_export <models-dir> <case.json> <out-prefix>`; `tests/parity/export.sh`
//! runs every case. A case file is:
//!
//! ```json
//! { "model": "fixture.tmod", "size": [512, 512], "view": [0, 0, 1, 1],
//!   "steps": [ { "params": {"P": 0.5}, "motion": "rise", "expression": "half_q",
//!                "dt": 0.1, "repeat": 3 } ] }
//! ```
//!
//! The model is loaded and `update(0)` is called once before the first step.
//! Each step applies `params` (kept until set again), starts `motion` (layer 0,
//! normal priority) and `expression`, then calls `update(dt)` `repeat` times
//! (default 1). The same list of calls must be made in every environment.

use std::collections::BTreeMap;

use serde::Deserialize;
use tsukumoe_cli::parity::DrawListFile;
use tsukumoe_gpu::{Headless, Texture};
use tsukumoe_runtime::{Priority, Runtime};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    model: String,
    size: [u32; 2],
    view: [f32; 4],
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    #[serde(default)]
    params: BTreeMap<String, f32>,
    motion: Option<String>,
    expression: Option<String>,
    dt: f32,
    #[serde(default = "one")]
    repeat: u32,
}

fn one() -> u32 {
    1
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let [_, models, case, out] = &a[..] else {
        panic!("usage: parity_export <models-dir> <case.json> <out-prefix>");
    };
    let case: Case = serde_json::from_slice(&std::fs::read(case).unwrap()).unwrap();
    let bytes = std::fs::read(std::path::Path::new(models).join(&case.model)).unwrap();

    let mut runtime = Runtime::from_bytes(&bytes).unwrap();
    // Loading evaluates the default frame once in every environment (the WASM
    // `Model` does it in its constructor); physics starts from that frame.
    runtime.update(0.0);
    for step in &case.steps {
        for (name, &value) in &step.params {
            let i = runtime
                .parameter_index(name)
                .unwrap_or_else(|| panic!("no parameter {name}"));
            runtime.set_parameter(i, value);
        }
        if let Some(m) = &step.motion {
            runtime.play_motion(m, 0, Priority::Normal).unwrap();
        }
        if let Some(e) = &step.expression {
            runtime.play_expression(e).unwrap();
        }
        for _ in 0..step.repeat {
            runtime.update(step.dt);
        }
    }

    let list = runtime.draw_list();
    std::fs::write(
        format!("{out}.json"),
        serde_json::to_vec(&DrawListFile::from_draw_list(&list)).unwrap(),
    )
    .unwrap();

    let buf = tsukumoe_core::tmod::AlignedBuf::from_bytes(&bytes);
    let tmod = tsukumoe_core::tmod::Tmod::parse(&buf).unwrap();
    let textures: Vec<Texture> = (0..tmod.texture_count())
        .map(|i| Texture::from_png(tmod.texture(i).unwrap()).unwrap())
        .collect();
    let mut gpu = Headless::new().unwrap();
    gpu.set_textures(&textures).unwrap();
    let image = gpu
        .render(&list, case.view, case.size[0], case.size[1])
        .unwrap();
    std::fs::write(format!("{out}.rgba"), image.rgba).unwrap();
}
