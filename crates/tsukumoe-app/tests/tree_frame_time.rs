// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Frame time of the parts and deformers panels on the reference model (631
//! meshes): a frame after one operation (open everything, close everything,
//! type a filter, choose a kind, select a row) draws the panel and
//! tessellates it. No GPU is needed; run it in release:
//!
//! `cargo test --release -p tsukumoe-app --test tree_frame_time -- --nocapture`

use std::time::{Duration, Instant};

use egui::{Context, RawInput, vec2};
use tsukumoe_app::panels::tree;
use tsukumoe_app::tree_model::{Kind, Panel};
use tsukumoe_app::view::{Selection, ViewState};
use tsukumoe_core::model::Model;

const BUDGET: Duration = Duration::from_millis(16);

fn frame(ctx: &Context, model: &Model, view: &mut ViewState, panel: Panel) -> Duration {
    let start = Instant::now();
    let raw = RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            vec2(320.0, 700.0),
        )),
        ..RawInput::default()
    };
    let mut output = ctx.run_ui(raw, |ui| {
        let _ = tree::show(ui, model, view, panel);
    });
    output.textures_delta.clear();
    let meshes = ctx.tessellate(std::mem::take(&mut output.shapes), 1.0);
    std::hint::black_box(&meshes);
    start.elapsed()
}

/// 631 meshes in 24 parts (nested in threes), with 24 warps holding a share of them.
fn big_model() -> Model {
    use tsukumoe_core::command::Command;
    let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let mut model = Model::new();
    let mut commands = Vec::new();
    for p in 0..24 {
        commands.push(Command::AddPart {
            name: format!("Part{p}"),
            parent: (p % 3 != 0).then(|| format!("Part{}", p - p % 3)),
        });
    }
    for w in 0..24 {
        commands.push(Command::AddWarpDeformer {
            name: format!("Warp{w}"),
            part: format!("Part{w}"),
            parent: (w % 4 != 0).then(|| format!("Warp{}", w - w % 4)),
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        });
    }
    for i in 0..631 {
        commands.push(Command::AddMesh {
            name: format!("Mesh{i}"),
            part: format!("Part{}", i % 24),
            positions: tri.clone(),
            uvs: tri.clone(),
            indices: vec![0, 1, 2],
        });
        if i % 2 == 0 {
            commands.push(Command::SetMeshParent {
                name: format!("Mesh{i}"),
                parent: Some(format!("Warp{}", i % 24)),
            });
        }
    }
    for c in commands {
        model.apply(c).unwrap();
    }
    model
}

#[test]
fn one_operation_on_the_631_mesh_tree_fits_the_frame_budget() {
    let model = big_model();
    assert_eq!(model.meshes().len(), 631);
    let ctx = Context::default();
    tsukumoe_ui::init(&ctx);
    let mut worst = Duration::ZERO;
    let mut report = Vec::new();
    for panel in [Panel::Parts, Panel::Deformers] {
        let mut view = ViewState::default();
        // Warm up: fonts, icons, the first layout.
        for _ in 0..3 {
            frame(&ctx, &model, &mut view, panel);
        }
        let first_mesh = model
            .meshes()
            .iter()
            .next()
            .map(|(_, n, _)| n.to_owned())
            .unwrap();
        type Op = Box<dyn Fn(&mut ViewState)>;
        let ops: Vec<(&str, Op)> = vec![
            ("open all", Box::new(move |v| v.tree_mut(panel).open_all())),
            (
                "filter",
                Box::new(move |v| v.tree_mut(panel).filter = "Mesh1".into()),
            ),
            (
                "clear filter",
                Box::new(move |v| v.tree_mut(panel).filter.clear()),
            ),
            (
                "kind",
                Box::new(move |v| v.tree_mut(panel).toggle_kind(Kind::Mesh)),
            ),
            (
                "kind off",
                Box::new(move |v| v.tree_mut(panel).toggle_kind(Kind::Mesh)),
            ),
            (
                "hidden only",
                Box::new(move |v| v.tree_mut(panel).hidden_only = true),
            ),
            (
                "hidden off",
                Box::new(move |v| v.tree_mut(panel).hidden_only = false),
            ),
            (
                "select",
                Box::new(move |v| v.select_only(Selection::Mesh(first_mesh.clone()))),
            ),
        ];
        for (label, op) in &ops {
            op(&mut view);
            let times: Vec<Duration> = (0..5)
                .map(|_| frame(&ctx, &model, &mut view, panel))
                .collect();
            let first = times[0];
            worst = worst.max(first);
            report.push(format!(
                "{panel:?} {label}: {:.2} ms",
                first.as_secs_f64() * 1e3
            ));
        }
        // Close every folder through the state function.
        view.tree_mut(panel).close_all(&model, panel);
        let t = frame(&ctx, &model, &mut view, panel);
        worst = worst.max(t);
        report.push(format!(
            "{panel:?} close all: {:.2} ms",
            t.as_secs_f64() * 1e3
        ));
    }
    eprintln!(
        "tree_frame_time ({} meshes):\n  {}\n  worst {:.2} ms (budget {} ms)",
        model.meshes().len(),
        report.join("\n  "),
        worst.as_secs_f64() * 1e3,
        BUDGET.as_millis()
    );
    // Debug builds are several times slower: they report but do not judge.
    if !cfg!(debug_assertions) {
        assert!(worst <= BUDGET, "worst {worst:?} is over the budget");
    }
}
