// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time while dragging control points of the biggest warp deformer of
//! the reference model (REQ-PERF-008: input to screen within 16 ms). The model
//! does not change during a drag, so a frame is the tool's preview plus the
//! overlay: building it (with the parent and child deformers around) and
//! tessellating it, which is what egui does before the GPU gets it. No GPU is
//! needed; run it in release:
//!
//! `cargo test --release -p tsukumoe-app --test deformer_frame_time -- --nocapture`
//!
//! The reference model's deformers have keyforms, so the release of the drag
//! is refused by the model; that refusal is checked too.

use std::time::{Duration, Instant};

use egui::{Context, LayerId, RawInput, Rect, pos2, vec2};
use tsukumoe_app::deformer_tool::{DeformerTool, Shape, related};
use tsukumoe_app::dispatcher::Dispatcher;
use tsukumoe_app::mesh_tool::Mapper;
use tsukumoe_app::panels::deformer_overlay;
use tsukumoe_app::scene::{Scene, fit_view};
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_ui::Theme;

const BUDGET: Duration = Duration::from_millis(16);
const FRAMES: usize = 300;
const SIZE: (f32, f32) = (1280.0, 800.0);

#[test]
fn dragging_warp_points_of_the_reference_model_fits_the_frame_budget() {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (_, model) = project(&table).unwrap();
    let (name, _) = model
        .warps()
        .iter()
        .map(|(_, n, w)| (n.to_owned(), w.base_points.len()))
        .max_by_key(|&(_, points)| points)
        .expect("the reference model has warp deformers");
    let scene = Scene::new(&model).unwrap();
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [SIZE.0, SIZE.1],
        view: fit_view(scene.bounds(), SIZE.0 as u32, SIZE.1 as u32),
    };
    let radius = (mapper.view[2] - mapper.view[0]) * 0.05;
    let shape = Shape::of(&model, &name, radius).unwrap();
    let around = related(&model, &name, radius);
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(SIZE.0, SIZE.1));
    let ctx = Context::default();
    let theme = Theme::dark();

    let mut tool = DeformerTool::default();
    tool.set_deformer(Some(&name));
    // Box-select every point, then drag them all (the heaviest preview).
    tool.press(&shape, &mapper, [0.0, 0.0], false);
    tool.drag([SIZE.0, SIZE.1]);
    tool.release(&name, &shape, &mapper);
    assert!(!tool.points().is_empty());
    let first = *tool.points().iter().next().unwrap();
    let start = mapper.to_screen(shape.world_points()[first as usize]);
    tool.press(&shape, &mapper, start, true);

    let mut times = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let t0 = Instant::now();
        let sweep = if (i / 60) % 2 == 0 {
            i % 60
        } else {
            60 - i % 60
        } as f32;
        tool.drag([start[0] + sweep, start[1] + sweep * 0.5]);
        let preview = tool.preview(&shape, &mapper);
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::background())
                .with_clip_rect(rect);
            deformer_overlay::paint(
                &painter,
                &theme,
                &mapper,
                preview.as_ref().unwrap_or(&shape),
                &around,
                &tool,
            );
        });
        output.textures_delta.clear();
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
        std::hint::black_box(&meshes);
        times.push(t0.elapsed());
    }
    times.sort();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];

    // The release: one command, refused because the warp has keyforms.
    let (command, label) = tool.release(&name, &shape, &mapper).unwrap();
    let mut dispatcher = Dispatcher::new(model);
    let refused = dispatcher.submit_labeled(command, Some(label)).unwrap_err();
    assert!(refused.contains("keyforms"), "{refused}");

    eprintln!(
        "deformer_frame_time: '{name}' has {} points; drag frame median {:.2} ms, p99 {:.2} ms, max {:.2} ms (budget {} ms)",
        shape.world_points().len(),
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
