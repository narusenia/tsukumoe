// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time while dragging a vertex of the largest mesh of the reference
//! model (REQ-PERF-008: input to screen within 16 ms). The model does not
//! change during a drag, so a frame is the tool's preview plus the overlay:
//! building it and tessellating it, which is what egui does before the GPU
//! gets it. The one frame that applies the command on release (the dispatcher
//! and the evaluated scene) is measured too. No GPU is needed; run it in
//! release:
//!
//! `cargo test --release -p tsukumoe-app --test mesh_frame_time -- --nocapture`

use std::time::{Duration, Instant};

use egui::{Context, LayerId, RawInput, Rect, pos2, vec2};
use tsukumoe_app::dispatcher::Dispatcher;
use tsukumoe_app::mesh_tool::{Mapper, MeshTool};
use tsukumoe_app::panels::mesh_overlay;
use tsukumoe_app::scene::{Scene, fit_view};
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_ui::Theme;

const BUDGET: Duration = Duration::from_millis(16);
const FRAMES: usize = 300;
const SIZE: (f32, f32) = (1280.0, 800.0);

#[test]
fn dragging_a_vertex_of_the_largest_reference_mesh_fits_the_frame_budget() {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (_, model) = project(&table).unwrap();
    let (name, mesh) = model
        .meshes()
        .iter()
        .map(|(_, n, m)| (n.to_owned(), m.clone()))
        .max_by_key(|(_, m)| m.positions.len())
        .unwrap();
    let scene = Scene::new(&model).unwrap();
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [SIZE.0, SIZE.1],
        view: fit_view(scene.bounds(), SIZE.0 as u32, SIZE.1 as u32),
    };
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(SIZE.0, SIZE.1));
    let ctx = Context::default();
    let theme = Theme::dark();

    // Zoom in on the mesh so that its edges fill the view, as when editing.
    let b = tsukumoe_app::scene::bounds(mesh.positions.as_flattened()).unwrap();
    let mapper = Mapper {
        view: fit_view(b, SIZE.0 as u32, SIZE.1 as u32),
        ..mapper
    };

    let mut tool = MeshTool::default();
    tool.set_mesh(Some(&name));
    // Box-select every vertex, then drag them all (the heaviest preview).
    tool.press(&mesh.positions, &mesh.indices, &mapper, [0.0, 0.0], false);
    tool.drag([SIZE.0, SIZE.1]);
    tool.release(&name, &mesh.positions, &mapper);
    assert!(!tool.vertices().is_empty());
    let first = *tool.vertices().iter().next().unwrap();
    let start = mapper.to_screen(mesh.positions[first as usize]);
    tool.press(&mesh.positions, &mesh.indices, &mapper, start, true);

    let mut times = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let t0 = Instant::now();
        let sweep = if (i / 60) % 2 == 0 {
            i % 60
        } else {
            60 - i % 60
        } as f32;
        tool.drag([start[0] + sweep, start[1] + sweep * 0.5]);
        let preview = tool.preview(&mesh.positions, &mapper);
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::background())
                .with_clip_rect(rect);
            mesh_overlay::paint(
                &painter,
                &theme,
                &mapper,
                &mesh.positions,
                &mesh.indices,
                &tool,
                preview.as_deref(),
            );
        });
        output.textures_delta.clear();
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
        std::hint::black_box(&meshes);
        times.push(t0.elapsed());
    }
    times.sort();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];

    // The release frame: one command, then the evaluated scene is rebuilt.
    let mut dispatcher = Dispatcher::new(model);
    let t0 = Instant::now();
    let (command, label) = tool.release(&name, &mesh.positions, &mapper).unwrap();
    dispatcher.submit_labeled(command, Some(label)).unwrap();
    let rebuilt = Scene::new(dispatcher.model()).unwrap();
    let release = t0.elapsed();
    std::hint::black_box(&rebuilt);

    eprintln!(
        "mesh_frame_time: '{name}' has {} vertices, {} triangles; drag frame median {:.2} ms, p99 {:.2} ms, max {:.2} ms; release frame {:.2} ms (budget {} ms)",
        mesh.positions.len(),
        mesh.indices.len() / 3,
        p(0.5).as_secs_f64() * 1e3,
        p(0.99).as_secs_f64() * 1e3,
        p(1.0).as_secs_f64() * 1e3,
        release.as_secs_f64() * 1e3,
        BUDGET.as_millis()
    );
    // Debug builds are several times slower: they report but do not judge.
    if !cfg!(debug_assertions) {
        assert!(p(0.99) <= BUDGET, "p99 {:?} is over the budget", p(0.99));
        assert!(
            release <= BUDGET,
            "release frame {release:?} is over the budget"
        );
    }
}
