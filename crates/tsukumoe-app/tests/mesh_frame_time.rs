// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time while dragging a vertex of the largest mesh of the reference
//! model (REQ-PERF-008: input to screen within 16 ms). The model does not
//! change during a drag, so a frame is the tool's preview plus the overlay:
//! building it and tessellating it, which is what egui does before the GPU
//! gets it. The one frame that applies the command on release (the dispatcher
//! and the evaluated scene) and the Delaunay re-triangulation are measured too. No GPU is needed; run it in
//! release:
//!
//! `cargo test --release -p tsukumoe-app --test mesh_frame_time -- --nocapture`
//!
//! A drag frame also shows the drag live in the picture: the preview is laid
//! over the compiled model, the frame is evaluated and, when a hardware GPU
//! is there, the whole model is drawn and waited for.

use std::time::{Duration, Instant};

use egui::{Context, LayerId, RawInput, Rect, pos2, vec2};
use tsukumoe_app::dispatcher::Dispatcher;
use tsukumoe_app::mesh_tool::{Mapper, MeshTool};
use tsukumoe_app::panels::mesh_overlay;
use tsukumoe_app::scene::{Preview, Scene, fit_view};
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_gpu::{Headless, Texture};
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
    let parent_space = tsukumoe_core::deformer_space::Space::of(&model, mesh.parent).unwrap();
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
        // The drag live in the picture: preview, evaluate, draw.
        let stored = tool.dragging_stored(&mesh.positions, &mapper, &parent_space);
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

    scene.set_preview(None);
    // The release frame: one command, then the evaluated scene is rebuilt.
    let mut dispatcher = Dispatcher::new(model);
    let t0 = Instant::now();
    let (command, label) = tool.release(&name, &mesh.positions, &mapper).unwrap();
    dispatcher.submit_labeled(command, Some(label)).unwrap();
    let rebuilt = Scene::new(dispatcher.model()).unwrap();
    let release = t0.elapsed();
    std::hint::black_box(&rebuilt);

    // Delaunay re-triangulation of the whole mesh, one press of the key.
    let geometry = tsukumoe_core::mesh_edit::Geometry {
        positions: mesh.positions.clone(),
        uvs: mesh.uvs.clone(),
        indices: mesh.indices.clone(),
    };
    // Scramble it first so that there is real work: many random flips.
    let mut geometry = geometry;
    let mut state = 12345u64;
    for _ in 0..2000 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let t = (state >> 33) as usize % (geometry.indices.len() / 3);
        let k = (state >> 20) as usize % 3;
        let v = &geometry.indices[t * 3..t * 3 + 3];
        if let Ok(g) = tsukumoe_core::mesh_edit::flip_edge(&geometry, v[k], v[(k + 1) % 3]) {
            geometry = g;
        }
    }
    let t0 = Instant::now();
    let retriangulated = tsukumoe_core::mesh_edit::retriangulate(&geometry, &[]).unwrap();
    let delaunay = t0.elapsed();
    std::hint::black_box(&retriangulated);
    // Grid fill: the default 4 x 4, and the largest 64 x 64 for the record.
    let t0 = Instant::now();
    let filled = tsukumoe_core::mesh_edit::fill_grid(&geometry, 4, 4);
    let grid = t0.elapsed();
    std::hint::black_box(&filled);
    let t0 = Instant::now();
    let big = tsukumoe_core::mesh_edit::fill_grid(&geometry, 64, 64);
    let grid_big = t0.elapsed();
    std::hint::black_box(&big);
    // Contour mesh from a 1024 x 1024 alpha (a blob with a notch and a second
    // island), from the extraction to the command applied: one key press.
    let alpha: Vec<u8> = (0..1024 * 1024)
        .map(|i| {
            let (x, y) = ((i % 1024) as f32, (i / 1024) as f32);
            let blob = (x - 450.0).hypot(y - 500.0) < 380.0 + 40.0 * (x * 0.05).sin();
            let notch = (x - 450.0).hypot(y - 150.0) < 90.0;
            let island = (x - 900.0).hypot(y - 900.0) < 90.0;
            if (blob && !notch) || island { 255 } else { 0 }
        })
        .collect();
    let quad = tsukumoe_core::mesh_edit::Geometry {
        positions: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        uvs: vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        indices: vec![0, 1, 2, 0, 2, 3],
    };
    let mut contour_model = tsukumoe_core::model::Model::new();
    contour_model
        .apply(tsukumoe_core::command::Command::AddPart {
            name: "P".into(),
            parent: None,
        })
        .unwrap();
    contour_model
        .apply(tsukumoe_core::command::Command::AddMesh {
            name: "Q".into(),
            part: "P".into(),
            positions: quad.positions.clone(),
            uvs: quad.uvs.clone(),
            indices: quad.indices.clone(),
        })
        .unwrap();
    let mut contour_dispatcher = Dispatcher::new(contour_model);
    let mut contour_tool = MeshTool::default();
    let image = tsukumoe_app::mesh_tool::AlphaImage {
        width: 1024,
        height: 1024,
        data: alpha,
    };
    let t0 = Instant::now();
    let (command, label) = contour_tool.contour_mesh("Q", &quad, Some(&image)).unwrap();
    contour_dispatcher
        .submit_labeled(command, Some(label))
        .unwrap();
    let contour = t0.elapsed();
    let made = contour_dispatcher
        .model()
        .meshes()
        .get(contour_dispatcher.model().meshes().find("Q").unwrap())
        .unwrap();
    eprintln!(
        "mesh_frame_time: contour 1024x1024 {:.1} ms ({} vertices, {} triangles)",
        contour.as_secs_f64() * 1e3,
        made.positions.len(),
        made.indices.len() / 3
    );
    eprintln!(
        "mesh_frame_time: fill_grid 4x4 {:.2} ms, 64x64 {:.2} ms",
        grid.as_secs_f64() * 1e3,
        grid_big.as_secs_f64() * 1e3
    );
    eprintln!(
        "mesh_frame_time: retriangulate {:.2} ms",
        delaunay.as_secs_f64() * 1e3
    );

    eprintln!(
        "mesh_frame_time: '{name}' has {} vertices, {} triangles; GPU {}; drag frame median {:.2} ms, p99 {:.2} ms, max {:.2} ms; release frame {:.2} ms (budget {} ms)",
        mesh.positions.len(),
        mesh.indices.len() / 3,
        if gpu.is_some() {
            "included"
        } else {
            "not available"
        },
        p(0.5).as_secs_f64() * 1e3,
        p(0.99).as_secs_f64() * 1e3,
        p(1.0).as_secs_f64() * 1e3,
        release.as_secs_f64() * 1e3,
        BUDGET.as_millis()
    );
    // Debug builds are several times slower: they report but do not judge.
    if !cfg!(debug_assertions) {
        assert!(p(0.99) <= BUDGET, "p99 {:?} is over the budget", p(0.99));
        assert!(grid <= BUDGET, "fill_grid 4x4 {grid:?} is over the budget");
        assert!(
            contour <= Duration::from_millis(100),
            "contour {contour:?} is over 100 ms"
        );
        assert!(
            delaunay <= BUDGET,
            "retriangulate {delaunay:?} is over the budget"
        );
        assert!(
            release <= BUDGET,
            "release frame {release:?} is over the budget"
        );
    }
}
