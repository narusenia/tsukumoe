// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time of a proportional edit drag (M5-04b, REQ-PERF-008: input to
//! screen within 16 ms), weights included. A frame is what the viewport does
//! while the pointer moves: the weights and the stored positions for the
//! live picture (`dragging_stored`), the weights again for the overlay's
//! shading (`drag_weights`), the overlay built and tessellated and, for the
//! reference model, the scene evaluated and (with a hardware GPU) drawn. Two
//! meshes: the reference model's largest and a synthetic one of 10 000
//! vertices. The release frame is measured too. Run it in release:
//!
//! `cargo test --release -p tsukumoe-app --test proportional_frame_time -- --nocapture`

use std::time::{Duration, Instant};

use egui::{Context, LayerId, RawInput, Rect, pos2, vec2};
use tsukumoe_app::dispatcher::Dispatcher;
use tsukumoe_app::mesh_tool::{Mapper, MeshTool, Op};
use tsukumoe_app::panels::{mesh_overlay, proportional_overlay};
use tsukumoe_app::scene::{Preview, Scene, bounds, fit_view};
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_core::deformer_space::Space;
use tsukumoe_core::elements::Point;
use tsukumoe_gpu::{Headless, Texture};
use tsukumoe_ui::Theme;

const BUDGET: Duration = Duration::from_millis(16);
const FRAMES: usize = 300;
const SIZE: (f32, f32) = (1280.0, 800.0);

struct Run {
    p50: Duration,
    p99: Duration,
    max: Duration,
    release: Duration,
    /// The vertices that followed the drag at the end.
    moved: usize,
}

/// Drags the middle vertex of `positions` with proportional edit on and
/// returns the frame times. `live` gets the stored positions of every frame
/// (the reference case lays them over the scene); it returns once its own
/// work (evaluating, drawing) is done.
fn drag(
    name: &str,
    positions: &[Point],
    indices: &[u32],
    radius: f32,
    mut live: impl FnMut(Option<Vec<Point>>),
) -> (Run, Option<Op>) {
    let b = bounds(positions.as_flattened()).unwrap();
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [SIZE.0, SIZE.1],
        view: fit_view(b, SIZE.0 as u32, SIZE.1 as u32),
    };
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(SIZE.0, SIZE.1));
    let ctx = Context::default();
    let theme = Theme::dark();
    let mut tool = MeshTool::default();
    tool.set_mesh(Some(name));
    tool.proportional.on = true;
    tool.proportional.radius = radius;
    // Pick the vertex nearest the centre of the view and drag it.
    let centre = [SIZE.0 / 2.0, SIZE.1 / 2.0];
    let nearest = (0..positions.len())
        .min_by(|&a, &b| {
            let d = |i: usize| {
                let s = mapper.to_screen(positions[i]);
                (s[0] - centre[0]).hypot(s[1] - centre[1])
            };
            d(a).total_cmp(&d(b))
        })
        .unwrap();
    let start = mapper.to_screen(positions[nearest]);
    tool.press(positions, indices, &mapper, start, false);

    let mut times = Vec::with_capacity(FRAMES);
    let mut moved = 0;
    for i in 0..FRAMES {
        let t0 = Instant::now();
        let sweep = if (i / 60) % 2 == 0 {
            i % 60
        } else {
            60 - i % 60
        } as f32;
        tool.drag([start[0] + sweep, start[1] + sweep * 0.5]);
        let stored = tool.dragging_stored(positions, &mapper, &Space::Drawing);
        if let Some(s) = &stored {
            moved = s.iter().zip(positions).filter(|(a, b)| a != b).count();
        }
        live(stored.clone());
        let shown = stored.unwrap_or_else(|| positions.to_vec());
        let weights = tool.drag_weights(&mapper);
        let screen: Vec<egui::Pos2> = shown
            .iter()
            .map(|&p| {
                let s = mapper.to_screen(p);
                pos2(s[0], s[1])
            })
            .collect();
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::background())
                .with_clip_rect(rect);
            mesh_overlay::paint(&painter, &theme, &mapper, &shown, indices, &tool, None);
            proportional_overlay::paint(
                &painter,
                &theme,
                &tool.proportional,
                &[pos2(start[0], start[1])],
                weights.as_deref().map(|w| (&screen[..], w)),
            );
        });
        output.textures_delta.clear();
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
        std::hint::black_box(&meshes);
        times.push(t0.elapsed());
    }
    times.sort();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    // The release: the weights once more, then the one command.
    let t0 = Instant::now();
    let made = tool.release(name, positions, &mapper);
    let release = t0.elapsed();
    let run = Run {
        p50: p(0.5),
        p99: p(0.99),
        max: *times.last().unwrap(),
        release,
        moved,
    };
    (run, made)
}

fn report(label: &str, run: &Run) {
    println!(
        "{label}: p50 {:?}, p99 {:?}, max {:?}, release {:?}, {} vertices followed",
        run.p50, run.p99, run.max, run.release, run.moved
    );
}

#[test]
fn a_proportional_drag_of_the_largest_reference_mesh_fits_the_frame_budget() {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (project, model) = project(&table).unwrap();
    let (name, mesh) = model
        .meshes()
        .iter()
        .map(|(_, n, m)| (n.to_owned(), m.clone()))
        .max_by_key(|(_, m)| m.positions.len())
        .unwrap();
    let mut scene = Scene::new(&model).unwrap();
    let full_view = fit_view(scene.bounds(), SIZE.0 as u32, SIZE.1 as u32);
    let mut gpu = Headless::hardware().ok();
    if let Some(g) = &mut gpu {
        let textures: Vec<Texture> = project
            .files
            .iter()
            .filter(|(p, _)| p.starts_with("textures/") && p.ends_with(".png"))
            .map(|(_, b)| Texture::from_png(b).unwrap())
            .collect();
        g.set_textures(&textures).unwrap();
    }
    println!(
        "{name}: {} vertices, GPU {}",
        mesh.positions.len(),
        if gpu.is_some() { "yes" } else { "no" }
    );
    for radius in [80.0, 800.0] {
        let (run, made) = drag(&name, &mesh.positions, &mesh.indices, radius, |stored| {
            scene.set_preview(
                stored
                    .map(|positions| Preview::Mesh {
                        name: name.clone(),
                        positions,
                    })
                    .as_ref(),
            );
            scene.advance(1.0 / 60.0);
            if let Some(g) = &mut gpu {
                let list = scene.frame.draw_list(&scene.model);
                g.draw(&list, full_view, true, SIZE.0 as u32, SIZE.1 as u32)
                    .unwrap();
                g.wait().unwrap();
            }
        });
        report(&format!("reference, radius {radius} pt"), &run);
        assert!(run.moved > 1, "the neighbours follow");
        // The release frame: the command, then the evaluated scene is rebuilt.
        scene.set_preview(None);
        let mut dispatcher = Dispatcher::new(project_model(&table));
        let t0 = Instant::now();
        let (command, label) = made.unwrap();
        dispatcher.submit_labeled(command, Some(label)).unwrap();
        let rebuilt = Scene::new(dispatcher.model()).unwrap();
        println!("release with apply and rebuild: {:?}", t0.elapsed());
        std::hint::black_box(&rebuilt);
        if cfg!(not(debug_assertions)) {
            assert!(run.p99 <= BUDGET, "{:?}", run.p99);
        }
    }
}

fn project_model(table: &Table) -> tsukumoe_core::model::Model {
    project(table).unwrap().1
}

#[test]
fn a_proportional_drag_of_a_ten_thousand_vertex_mesh_fits_the_frame_budget() {
    let side = 100u32;
    let positions: Vec<Point> = (0..side * side)
        .map(|i| [(i % side) as f32, (i / side) as f32])
        .collect();
    let mut indices = Vec::new();
    for y in 0..side - 1 {
        for x in 0..side - 1 {
            let a = y * side + x;
            indices.extend([a, a + 1, a + side + 1, a, a + side + 1, a + side]);
        }
    }
    for radius in [80.0, 800.0] {
        let (run, made) = drag("Big", &positions, &indices, radius, |_| {});
        report(&format!("10000 vertices, radius {radius} pt"), &run);
        assert!(made.is_some() && run.moved > 1);
        if cfg!(not(debug_assertions)) {
            assert!(run.p99 <= BUDGET, "{:?}", run.p99);
        }
    }
}
