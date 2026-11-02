// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time of a drag on a shape with keyforms, at a key (M5-06b,
//! REQ-PERF-008: input to screen within 16 ms): a vertex drag of the largest
//! mesh of the reference model (bound to a parameter here if it has no
//! keyforms), the same with proportional edit, and a control point drag of the
//! largest warp (the reference model's warps have keyforms). A frame is what the
//! viewport does while the pointer moves: the parameter values and the key
//! place, the stored shape, the drag as the preview of the keyform, the scene
//! evaluated and (with a hardware GPU) drawn, and the overlay built and
//! tessellated. Run it in release:
//!
//! `cargo test --release -p tsukumoe-app --test keyform_frame_time -- --nocapture`

use std::time::{Duration, Instant};

use egui::{Context, LayerId, RawInput, Rect, pos2, vec2};
use tsukumoe_app::deformer_tool::{DeformerTool, Shape};
use tsukumoe_app::keyed::{self, Place};
use tsukumoe_app::mesh_tool::{Mapper, MeshTool};
use tsukumoe_app::panels::{deformer_overlay, mesh_overlay};
use tsukumoe_app::scene::{Preview, Scene, fit_view};
use tsukumoe_app::view::ViewState;
use tsukumoe_cli::generate::{Table, project};
use tsukumoe_core::command::{Command, ElementRef};
use tsukumoe_core::deformer_space::Space;
use tsukumoe_core::keyform::Binding;
use tsukumoe_core::model::Model;
use tsukumoe_gpu::{Headless, Texture};
use tsukumoe_ui::Theme;

const BUDGET: Duration = Duration::from_millis(16);
const FRAMES: usize = 300;
const SIZE: (f32, f32) = (1280.0, 800.0);

struct Setup {
    model: Model,
    scene: Scene,
    view: ViewState,
    gpu: Option<Headless>,
}

fn setup() -> (Setup, Vec<(String, usize, usize)>) {
    let table = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/reference-model/parts.json"
    );
    let table: Table = serde_json::from_slice(&std::fs::read(table).unwrap()).unwrap();
    let (project, mut model) = project(&table).unwrap();
    let mesh = model
        .meshes()
        .iter()
        .max_by_key(|(_, _, m)| m.positions.len())
        .map(|(_, n, _)| n.to_owned())
        .unwrap();
    let keyed_already = model
        .meshes()
        .get(model.meshes().find(&mesh).unwrap())
        .unwrap()
        .keyforms
        .is_some();
    if !keyed_already {
        let (_, parameter, _) = model.parameters().iter().next().unwrap();
        let parameter = parameter.to_owned();
        model
            .apply(Command::BindParameter {
                parameter,
                elements: vec![ElementRef::Mesh { name: mesh.clone() }],
            })
            .unwrap();
    }
    let warp = model
        .warps()
        .iter()
        .filter(|(_, _, w)| w.keyforms.is_some())
        .max_by_key(|(_, _, w)| w.base_points.len())
        .map(|(_, n, _)| n.to_owned())
        .expect("a keyed warp");
    let scene = Scene::new(&model).unwrap();
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
    let _ = warp;
    (
        Setup {
            model,
            scene,
            view: ViewState::default(),
            gpu,
        },
        Vec::new(),
    )
}

/// Puts every parameter of `binding` on its first key.
fn on_first_keys(s: &mut Setup, binding: &Binding) {
    for axis in binding.axes() {
        let name = s
            .model
            .parameters()
            .name(axis.parameter)
            .unwrap()
            .to_owned();
        let p = s.model.parameters().get(axis.parameter).unwrap().clone();
        s.view.set_param(&name, &p, axis.keys[0]);
    }
    s.scene.apply(&s.view);
}

fn report(label: &str, mut times: Vec<Duration>, gpu: bool) {
    times.sort();
    let p = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
    eprintln!(
        "keyform_frame_time {label}: GPU {}; median {:.2} ms, p99 {:.2} ms, max {:.2} ms (budget {} ms)",
        if gpu { "included" } else { "not available" },
        p(0.5).as_secs_f64() * 1e3,
        p(0.99).as_secs_f64() * 1e3,
        times.last().unwrap().as_secs_f64() * 1e3,
        BUDGET.as_millis()
    );
    if cfg!(not(debug_assertions)) {
        assert!(p(0.99) <= BUDGET, "{label}: p99 {:?}", p(0.99));
    }
}

fn mesh_drag(proportional: bool) {
    let (mut s, _) = setup();
    let name = s
        .model
        .meshes()
        .iter()
        .max_by_key(|(_, _, m)| m.positions.len())
        .map(|(_, n, _)| n.to_owned())
        .unwrap();
    let mesh = s
        .model
        .meshes()
        .get(s.model.meshes().find(&name).unwrap())
        .unwrap();
    let binding = s
        .model
        .bindings()
        .get(mesh.keyforms.as_ref().unwrap().binding)
        .unwrap()
        .clone();
    let indices = mesh.indices.clone();
    let parent = mesh.parent;
    on_first_keys(&mut s, &binding);
    let full_view = fit_view(s.scene.bounds(), SIZE.0 as u32, SIZE.1 as u32);
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(SIZE.0, SIZE.1));
    let ctx = Context::default();
    let theme = Theme::dark();
    s.scene.advance(0.0);
    let shown = s.scene.drawn_positions(&name).unwrap();
    let b = tsukumoe_app::scene::bounds(shown.as_flattened()).unwrap();
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [SIZE.0, SIZE.1],
        view: fit_view(b, SIZE.0 as u32, SIZE.1 as u32),
    };
    let mut tool = MeshTool::default();
    tool.set_mesh(Some(&name));
    tool.proportional.on = proportional;
    tool.proportional.radius = 80.0;
    tool.press(&shown, &indices, &mapper, [0.0, 0.0], false);
    tool.drag([SIZE.0, SIZE.1]);
    let at0 = keyed::values(&s.model, &s.view);
    let stored0 = keyed::mesh_stored(&s.model, &name, &at0).unwrap();
    let space0 = Space::of_at(&s.model, parent, &at0).unwrap();
    tool.release_in(&name, &stored0, &shown, &mapper, &space0);
    let first = if proportional {
        // One vertex: its neighbours follow.
        tool.press(&shown, &indices, &mapper, mapper.to_screen(shown[0]), false);
        tool.drag([
            mapper.to_screen(shown[0])[0] + 5.0,
            mapper.to_screen(shown[0])[1],
        ]);
        tool.release_in(&name, &stored0, &shown, &mapper, &space0);
        0
    } else {
        *tool.vertices().iter().next().unwrap() as usize
    };
    let start = mapper.to_screen(shown[first]);
    tool.press(&shown, &indices, &mapper, start, true);
    assert!(matches!(
        keyed::mesh_place(&s.model, &name, &at0),
        Some(Place::Key { .. })
    ));
    let mut times = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let t0 = Instant::now();
        let sweep = if (i / 60) % 2 == 0 {
            i % 60
        } else {
            60 - i % 60
        } as f32;
        tool.drag([start[0] + sweep, start[1] + sweep * 0.5]);
        let at = keyed::values(&s.model, &s.view);
        let stored = keyed::mesh_stored(&s.model, &name, &at).unwrap();
        let space = Space::of_at(&s.model, parent, &at).unwrap();
        let live = tool
            .dragging_stored(&stored, &mapper, &space)
            .map(|positions| {
                keyed::lift_preview(
                    &s.model,
                    &at,
                    Preview::Mesh {
                        name: name.clone(),
                        positions,
                    },
                )
            });
        s.scene.set_previews(live.as_slice());
        s.scene.advance(1.0 / 60.0);
        let shown_now = s.scene.drawn_positions(&name).unwrap();
        let weights = tool.drag_weights(&mapper);
        std::hint::black_box(&weights);
        if let Some(g) = &mut s.gpu {
            let list = s.scene.frame.draw_list(&s.scene.model);
            g.draw(&list, full_view, true, SIZE.0 as u32, SIZE.1 as u32)
                .unwrap();
            g.wait().unwrap();
        }
        let mut output = ctx.run_ui(RawInput::default(), |ui| {
            let painter = ui
                .ctx()
                .layer_painter(LayerId::background())
                .with_clip_rect(rect);
            mesh_overlay::paint(&painter, &theme, &mapper, &shown_now, &indices, &tool, None);
        });
        output.textures_delta.clear();
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
        std::hint::black_box(&meshes);
        times.push(t0.elapsed());
    }
    let label = if proportional {
        format!("'{name}' vertex drag with proportional edit")
    } else {
        format!("'{name}' vertex drag")
    };
    report(&label, times, s.gpu.is_some());
}

#[test]
fn dragging_vertices_of_a_keyed_mesh_fits_the_frame_budget() {
    mesh_drag(false);
}

#[test]
fn dragging_a_vertex_of_a_keyed_mesh_with_proportional_edit_fits_the_frame_budget() {
    mesh_drag(true);
}

#[test]
fn dragging_points_of_a_keyed_warp_fits_the_frame_budget() {
    let (mut s, _) = setup();
    let name = s
        .model
        .warps()
        .iter()
        .filter(|(_, _, w)| w.keyforms.is_some())
        .max_by_key(|(_, _, w)| w.base_points.len())
        .map(|(_, n, _)| n.to_owned())
        .unwrap();
    let w = s
        .model
        .warps()
        .get(s.model.warps().find(&name).unwrap())
        .unwrap();
    let binding = s
        .model
        .bindings()
        .get(w.keyforms.as_ref().unwrap().binding)
        .unwrap()
        .clone();
    on_first_keys(&mut s, &binding);
    let full_view = fit_view(s.scene.bounds(), SIZE.0 as u32, SIZE.1 as u32);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [SIZE.0, SIZE.1],
        view: full_view,
    };
    let radius = (mapper.view[2] - mapper.view[0]) * 0.05;
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(SIZE.0, SIZE.1));
    let ctx = Context::default();
    let theme = Theme::dark();
    let at0 = keyed::values(&s.model, &s.view);
    let shape0 = Shape::of_at(&s.model, &name, radius, &at0).unwrap();
    let mut tool = DeformerTool::default();
    tool.set_deformer(Some(&name));
    tool.proportional.on = true;
    tool.proportional.radius = 80.0;
    tool.press(&shape0, &mapper, [0.0, 0.0], false);
    tool.drag([SIZE.0, SIZE.1]);
    tool.release(&name, &shape0, &mapper);
    let first = *tool.points().iter().next().unwrap();
    let start = mapper.to_screen(shape0.world_points()[first as usize]);
    tool.press(&shape0, &mapper, start, true);
    let mut times = Vec::with_capacity(FRAMES);
    for i in 0..FRAMES {
        let t0 = Instant::now();
        let sweep = if (i / 60) % 2 == 0 {
            i % 60
        } else {
            60 - i % 60
        } as f32;
        tool.drag([start[0] + sweep, start[1] + sweep * 0.5]);
        let at = keyed::values(&s.model, &s.view);
        let shape = Shape::of_at(&s.model, &name, radius, &at).unwrap();
        let preview = tool.preview(&shape, &mapper);
        let live = tool
            .dragging(&shape, &mapper)
            .map(|d| keyed::lift_preview(&s.model, &at, d.as_preview(&name)));
        s.scene.set_previews(live.as_slice());
        s.scene.advance(1.0 / 60.0);
        if let Some(g) = &mut s.gpu {
            let list = s.scene.frame.draw_list(&s.scene.model);
            g.draw(&list, full_view, true, SIZE.0 as u32, SIZE.1 as u32)
                .unwrap();
            g.wait().unwrap();
        }
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
                &[],
                &tool,
            );
        });
        output.textures_delta.clear();
        let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
        std::hint::black_box(&meshes);
        times.push(t0.elapsed());
    }
    report(
        &format!("'{name}' grid point drag with proportional edit"),
        times,
        s.gpu.is_some(),
    );
}
