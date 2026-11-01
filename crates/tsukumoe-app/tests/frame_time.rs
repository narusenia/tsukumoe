// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time while dragging a parameter on the reference model
//! (REQ-PERF-008: input to screen within 16 ms). Headless: it measures what
//! one dragged frame costs outside egui's own UI drawing: the drag mapping,
//! the evaluation, the physics step and the renderer up to the GPU being done.
//! It needs a hardware adapter, skips without one, and is meant for release
//! builds:
//!
//! `cargo test --release -p tsukumoe-app --test frame_time -- --nocapture`

use std::time::{Duration, Instant};

use tsukumoe_app::drag::DragAssignment;
use tsukumoe_app::scene::{Scene, fit_view};
use tsukumoe_app::view::ViewState;
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_gpu::{Headless, Texture};

const BUDGET: Duration = Duration::from_millis(16);
const FRAMES: usize = 300;
const SIZE: (u32, u32) = (1280, 800);

#[test]
fn dragging_a_parameter_on_the_reference_model_fits_the_frame_budget() {
    let Ok(mut gpu) = Headless::hardware() else {
        eprintln!("frame_time: no hardware adapter, skipped");
        return;
    };
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (project, model) = project(&table).unwrap();
    let textures: Vec<Texture> = project
        .files
        .iter()
        .filter(|(p, _)| p.starts_with("textures/") && p.ends_with(".png"))
        .map(|(_, b)| Texture::from_png(b).unwrap())
        .collect();
    gpu.set_textures(&textures).unwrap();

    let mut scene = Scene::new(&model).unwrap();
    let names: Vec<String> = model
        .parameters()
        .iter()
        .map(|(_, n, _)| n.to_owned())
        .take(2)
        .collect();
    let mut assignment = DragAssignment::default();
    for (axis, name) in [&mut assignment.x, &mut assignment.y]
        .into_iter()
        .zip(&names)
    {
        axis.as_mut().unwrap().parameter = name.clone();
    }
    let mut view = ViewState::default();
    let fitted = fit_view(scene.bounds(), SIZE.0, SIZE.1);
    let mut times = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let start = Instant::now();
        // The pointer sweeps back and forth, as a drag does.
        let delta = if (i / 60) % 2 == 0 { 3.0 } else { -3.0 };
        let params = model.parameters();
        let updates = assignment.drive([delta, delta], |n| {
            let p = params.get(params.find(n)?)?;
            Some((p.clone(), view.param(n, p)))
        });
        for (name, value) in updates {
            let p = params.get(params.find(&name).unwrap()).unwrap();
            view.set_param(&name, p, value);
        }
        scene.apply(&view);
        scene.advance(1.0 / 60.0);
        let list = scene.frame.draw_list(&scene.model);
        gpu.draw(&list, view.camera.view(fitted), true, SIZE.0, SIZE.1)
            .unwrap();
        gpu.wait().unwrap();
        times.push(start.elapsed());
    }
    times.sort();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    eprintln!(
        "frame_time on {}: median {:.2} ms, p99 {:.2} ms, max {:.2} ms (budget {} ms)",
        gpu.adapter(),
        p(0.5).as_secs_f64() * 1e3,
        p(0.99).as_secs_f64() * 1e3,
        p(1.0).as_secs_f64() * 1e3,
        BUDGET.as_millis()
    );
    // Debug builds are several times slower: they report but do not judge.
    if !cfg!(debug_assertions) {
        assert!(p(0.99) <= BUDGET, "p99 {:?} is over the budget", p(0.99));
    }
}
