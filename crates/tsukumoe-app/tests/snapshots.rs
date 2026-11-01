// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Image comparison of the editor window with the standard panels (no GPU
//! viewport: the panel says so) in the dark and light themes. Update the
//! stored images after an intended change with
//! `UPDATE_SNAPSHOTS=1 cargo test -p tsukumoe-app --test snapshots` and look
//! at them before committing.

use std::path::Path;

use egui_kittest::{Harness, SnapshotOptions};
use tsukumoe_app::editor::Editor;
use tsukumoe_ui::{Language, Mode, set_language, set_mode};

fn snapshot(mode: Mode, name: &str) {
    let mut editor = Editor::new(None);
    editor
        .open(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tsukumoe-format/tests/fixtures/minimal"),
        )
        .unwrap();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 640.0))
        .wgpu()
        .build_ui_state(
            move |ui, editor: &mut Editor| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                editor.ui(ui, None);
            },
            editor,
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn editor_in_the_dark_theme() {
    snapshot(Mode::Dark, "editor_dark");
}

#[test]
fn editor_in_the_light_theme() {
    snapshot(Mode::Light, "editor_light");
}

/// The mesh edit overlay on a small mesh: two selected vertices, a drag box
/// the veil and the grid options box, drawn without the GPU picture behind it.
fn mesh_overlay_snapshot(mode: Mode, name: &str) {
    use egui::Rect;
    use tsukumoe_app::mesh_tool::{Mapper, MeshTool};
    use tsukumoe_app::panels::mesh_overlay;
    use tsukumoe_ui::Theme;

    let mut positions = Vec::new();
    for y in 0..4 {
        for x in 0..5 {
            positions.push([x as f32 + (y % 2) as f32 * 0.2, y as f32]);
        }
    }
    let mut indices = Vec::new();
    for y in 0..3u32 {
        for x in 0..4u32 {
            let a = y * 5 + x;
            indices.extend([a, a + 1, a + 6, a, a + 6, a + 5]);
        }
    }
    let size = egui::vec2(360.0, 260.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [-0.5, -0.4, 4.9, 3.4],
    };
    let mut tool = MeshTool::default();
    tool.set_mesh(Some("Skin"));
    let at = |i: usize| mapper.to_screen(positions[i]);
    tool.click(&positions, &indices, &mapper, at(6), false);
    tool.click(&positions, &indices, &mapper, at(13), true);
    // A box over the lower right, still being dragged.
    tool.press(&positions, &indices, &mapper, [240.0, 130.0], true);
    tool.drag([330.0, 240.0]);
    let mut textures = None;
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, tool: &mut MeshTool| {
            let ctx = ui.ctx().clone();
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            // Stand-ins for the viewport picture: all meshes, and the edited
            // one alone on a transparent texture.
            let (w, h) = (size.x as usize, size.y as usize);
            let edited = |x: usize, y: usize| (70..230).contains(&x) && (50..200).contains(&y);
            let scene = egui::ColorImage::new(
                [w, h],
                (0..w * h)
                    .map(|i| {
                        if edited(i % w, i / w) {
                            egui::Color32::from_rgb(70, 170, 160)
                        } else {
                            egui::Color32::from_rgb(220, 150, 80)
                        }
                    })
                    .collect(),
            );
            let focus = egui::ColorImage::new(
                [w, h],
                (0..w * h)
                    .map(|i| {
                        if edited(i % w, i / w) {
                            egui::Color32::from_rgb(70, 170, 160)
                        } else {
                            egui::Color32::TRANSPARENT
                        }
                    })
                    .collect(),
            );
            let opts = egui::TextureOptions::NEAREST;
            // Uploaded once: a new texture each frame would not show yet.
            let (scene, focus) = textures.get_or_insert_with(|| {
                (
                    ctx.load_texture("mesh-edit-scene", scene, opts),
                    ctx.load_texture("mesh-edit-focus", focus, opts),
                )
            });
            let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(scene.id(), rect, uv, egui::Color32::WHITE);
            mesh_overlay::veil(&painter, &theme, rect, Some(focus.id()));
            mesh_overlay::paint(&painter, &theme, &mapper, &positions, &indices, tool, None);
            mesh_overlay::grid_options(&ctx, rect, tool);
        },
        tool,
    );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn mesh_edit_overlay_in_the_dark_theme() {
    mesh_overlay_snapshot(Mode::Dark, "mesh_edit_dark");
}

#[test]
fn mesh_edit_overlay_in_the_light_theme() {
    mesh_overlay_snapshot(Mode::Light, "mesh_edit_light");
}
