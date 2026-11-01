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
    let size = egui::vec2(480.0, 260.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [-0.8, -0.4, 5.2, 3.4],
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

/// The right-click menu of the mesh tool with one vertex selected: the
/// whole-mesh items and delete are on, drawing an edge and flipping are off.
fn menu_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::mesh_tool::{MENU_ACTIONS, Mapper, MeshTool};
    use tsukumoe_app::panels::mesh_overlay;

    let positions = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    let indices = vec![0, 1, 2, 0, 2, 3];
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [100.0, 100.0],
        view: [0.0, 0.0, 1.0, 1.0],
    };
    let mut tool = MeshTool::default();
    tool.set_mesh(Some("Skin"));
    tool.click(&positions, &indices, &mapper, [100.0, 100.0], false);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(330.0, 245.0))
        .wgpu()
        .build_ui_state(
            move |ui, tool: &mut MeshTool| {
                let ctx = ui.ctx().clone();
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let keys = |a: &str| match a {
                    "mesh.retriangulate" => Some("D".to_owned()),
                    "mesh.fill-grid" => Some("G".to_owned()),
                    "mesh.contour" => Some("C".to_owned()),
                    "mesh.connect" => Some("E".to_owned()),
                    "mesh.flip-edge" => Some("F".to_owned()),
                    "mesh.delete-vertices" => Some("Delete".to_owned()),
                    "deformer.create-warp" => Some("W".to_owned()),
                    "deformer.create-rotation" => Some("R".to_owned()),
                    _ => None,
                };
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    mesh_overlay::menu_ui(ui, &MENU_ACTIONS, &|a| tool.menu_enabled(a), &keys);
                });
            },
            tool,
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn mesh_edit_menu_in_the_dark_theme() {
    menu_snapshot(Mode::Dark, "mesh_edit_menu_dark");
}

#[test]
fn mesh_edit_menu_in_the_light_theme() {
    menu_snapshot(Mode::Light, "mesh_edit_menu_light");
}

/// The deformer tool's overlay: a 3 x 3 warp with two points selected and
/// bent, a rotation deformer holding it, a child warp, the drag box, and the
/// options box.
fn deformer_overlay_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::deformer_tool::{DeformerTool, Shape, related};
    use tsukumoe_app::mesh_tool::Mapper;
    use tsukumoe_app::panels::deformer_overlay;
    use tsukumoe_core::command::{Command, RotationBase};
    use tsukumoe_core::model::Model;
    use tsukumoe_ui::Theme;

    let mut grid = Vec::new();
    for r in 0..4 {
        for c in 0..4 {
            grid.push([c as f32 * 0.9 + (r % 2) as f32 * 0.1, r as f32 * 0.8]);
        }
    }
    let mut model = Model::new();
    for command in [
        Command::AddPart {
            name: "P".into(),
            parent: None,
        },
        Command::AddRotationDeformer {
            name: "Rot".into(),
            part: "P".into(),
            parent: None,
            base: RotationBase {
                origin: [1.2, 1.2],
                angle_degrees: 15.0,
                scale: 1.0,
                reflect_x: false,
                reflect_y: false,
            },
        },
        Command::AddWarpDeformer {
            name: "Warp".into(),
            part: "P".into(),
            parent: Some("Rot".into()),
            rows: 3,
            columns: 3,
            base_points: grid,
        },
        Command::AddWarpDeformer {
            name: "Child".into(),
            part: "P".into(),
            parent: Some("Warp".into()),
            rows: 1,
            columns: 1,
            base_points: vec![[0.2, 0.2], [0.5, 0.2], [0.2, 0.5], [0.5, 0.5]],
        },
    ] {
        model.apply(command).unwrap();
    }
    let size = egui::vec2(480.0, 300.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [-0.8, -0.4, 5.2, 4.2],
    };
    let shape = Shape::of(&model, "Warp", 0.4).unwrap();
    let related = related(&model, "Warp", 0.4);
    let mut tool = DeformerTool::default();
    tool.set_deformer(Some("Warp"));
    let world = shape.world_points();
    tool.click(&shape, &mapper, mapper.to_screen(world[5]), false);
    tool.click(&shape, &mapper, mapper.to_screen(world[10]), true);
    // A box being dragged over the lower left.
    tool.press(&shape, &mapper, [30.0, 200.0], true);
    tool.drag([120.0, 280.0]);
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, tool: &mut DeformerTool| {
            let ctx = ui.ctx().clone();
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme.colors.surface_raised);
            deformer_overlay::paint(&painter, &theme, &mapper, &shape, &related, tool);
            deformer_overlay::options(&ctx, rect, tool);
        },
        tool,
    );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn deformer_overlay_in_the_dark_theme() {
    deformer_overlay_snapshot(Mode::Dark, "deformer_dark");
}

#[test]
fn deformer_overlay_in_the_light_theme() {
    deformer_overlay_snapshot(Mode::Light, "deformer_light");
}

/// The parts panel or the deformers panel over a model with nested parts and
/// deformers: eye column, indent, guide lines (the selected row's ancestors
/// stronger), a closed folder, a hidden mesh, a selected mesh and a second selected one.
fn tree_snapshot(mode: Mode, panel: tsukumoe_app::tree_model::Panel, name: &str) {
    use tsukumoe_app::panels::tree;
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::{Command, RotationBase};
    use tsukumoe_core::model::Model;

    let tri = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let base = RotationBase {
        origin: [0.0, 0.0],
        angle_degrees: 0.0,
        scale: 1.0,
        reflect_x: false,
        reflect_y: false,
    };
    let mut model = Model::new();
    let mut commands = vec![
        Command::AddPart {
            name: "Body".into(),
            parent: None,
        },
        Command::AddPart {
            name: "Head".into(),
            parent: Some("Body".into()),
        },
        Command::AddPart {
            name: "Hair".into(),
            parent: Some("Head".into()),
        },
        Command::AddPart {
            name: "Arm".into(),
            parent: Some("Body".into()),
        },
        Command::AddWarpDeformer {
            name: "FaceWarp".into(),
            part: "Head".into(),
            parent: None,
            rows: 1,
            columns: 1,
            base_points: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]],
        },
        Command::AddRotationDeformer {
            name: "HeadRotation".into(),
            part: "Head".into(),
            parent: Some("FaceWarp".into()),
            base,
        },
        Command::AddRotationDeformer {
            name: "ArmRotation".into(),
            part: "Arm".into(),
            parent: None,
            base,
        },
    ];
    for (mesh, part, parent) in [
        ("HairFront", "Hair", Some("HeadRotation")),
        ("HairBack", "Hair", None),
        ("Eye", "Head", Some("HeadRotation")),
        ("Mouth", "Head", Some("FaceWarp")),
        ("Sleeve", "Arm", Some("ArmRotation")),
    ] {
        commands.push(Command::AddMesh {
            name: mesh.into(),
            part: part.into(),
            positions: tri.clone(),
            uvs: tri.clone(),
            indices: vec![0, 1, 2],
        });
        if let Some(p) = parent {
            commands.push(Command::SetMeshParent {
                name: mesh.into(),
                parent: Some(p.into()),
            });
        }
    }
    commands.push(Command::SetMeshVisible {
        name: "HairBack".into(),
        visible: false,
    });
    for c in commands {
        model.apply(c).unwrap();
    }
    let mut view = ViewState::default();
    let pick = |n: &str| Selection::Mesh(n.into());
    view.select_all(vec![pick("Eye"), pick("HairFront")]);
    view.parts.set_open("p:Arm", false);
    view.tree_mut(panel).seen = view.selection.clone();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(300.0, 330.0))
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut (Model, ViewState)| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                tree::show(ui, &state.0, &mut state.1, panel);
            },
            (model, view),
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn parts_panel_in_the_dark_theme() {
    tree_snapshot(
        Mode::Dark,
        tsukumoe_app::tree_model::Panel::Parts,
        "tree_parts_dark",
    );
}

#[test]
fn parts_panel_in_the_light_theme() {
    tree_snapshot(
        Mode::Light,
        tsukumoe_app::tree_model::Panel::Parts,
        "tree_parts_light",
    );
}

#[test]
fn deformers_panel_in_the_dark_theme() {
    tree_snapshot(
        Mode::Dark,
        tsukumoe_app::tree_model::Panel::Deformers,
        "tree_deformers_dark",
    );
}

#[test]
fn deformers_panel_in_the_light_theme() {
    tree_snapshot(
        Mode::Light,
        tsukumoe_app::tree_model::Panel::Deformers,
        "tree_deformers_light",
    );
}

/// The box around the selection over a stand-in picture of two meshes.
fn bounds_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::mesh_tool::Mapper;
    use tsukumoe_app::panels::selection_overlay;
    use tsukumoe_ui::Theme;

    let size = egui::vec2(300.0, 200.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [0.0, 0.0, 6.0, 4.0],
    };
    let mut harness = Harness::builder()
        .with_size(size)
        .wgpu()
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme.colors.surface_raised);
            // Two meshes, as flat blocks; the box goes around the pair.
            for block in [[0.8, 0.9, 2.4, 2.8], [3.1, 1.2, 4.9, 3.0]] {
                let (a, b) = (
                    mapper.to_screen([block[0], block[1]]),
                    mapper.to_screen([block[2], block[3]]),
                );
                painter.rect_filled(
                    egui::Rect::from_two_pos(egui::pos2(a[0], a[1]), egui::pos2(b[0], b[1])),
                    2.0,
                    theme.colors.control_active,
                );
            }
            selection_overlay::paint(&painter, &theme, &mapper, [0.8, 0.9, 4.9, 3.0]);
        });
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn selection_box_in_the_dark_theme() {
    bounds_snapshot(Mode::Dark, "selection_box_dark");
}

#[test]
fn selection_box_in_the_light_theme() {
    bounds_snapshot(Mode::Light, "selection_box_light");
}
