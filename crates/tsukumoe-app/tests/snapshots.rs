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
    use tsukumoe_app::panels::{mesh_overlay, overlay};
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
            tsukumoe_ui::init(&ctx);
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
            overlay::flyout(&ctx, rect, |ui| {
                mesh_overlay::grid_options(ui, tool);
            });
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
        .with_size(egui::vec2(330.0, 460.0))
        .wgpu()
        .build_ui_state(
            move |ui, tool: &mut MeshTool| {
                let ctx = ui.ctx().clone();
                tsukumoe_ui::init(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let keys = |a: &str| match a {
                    "mesh.retriangulate" => Some("D".to_owned()),
                    "mesh.fill-grid" => Some("G".to_owned()),
                    "mesh.contour" => Some("C".to_owned()),
                    "mesh.connect" => Some("E".to_owned()),
                    "mesh.flip-edge" => Some("F".to_owned()),
                    "mesh.edge-pen" => Some("P".to_owned()),
                    "mesh.delete-edge" => Some("X".to_owned()),
                    "mesh.weld" => Some("J".to_owned()),
                    "mesh.weld-near" => Some("Shift+J".to_owned()),
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
    use tsukumoe_app::panels::{deformer_overlay, overlay};
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
            overlay::flyout(&ctx, rect, |ui| {
                deformer_overlay::options(ui, tool);
            });
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
/// stronger), a closed folder, a hidden mesh, selected meshes and a selected warp, the parent
/// badges on mesh rows in the parts panel and the linked rows lit in each panel.
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
    view.select_all(vec![
        Selection::Warp("FaceWarp".into()),
        pick("Eye"),
        pick("HairFront"),
    ]);
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

/// Several selected meshes: each outlined, the last one bold, the box around
/// all of them with the count badge.
fn multi_selection_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::mesh_tool::Mapper;
    use tsukumoe_app::panels::selection_overlay;
    use tsukumoe_app::scene::{Scene, fit_view};
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::Command;
    use tsukumoe_core::model::Model;
    use tsukumoe_ui::Theme;

    let mut model = Model::new();
    let mut commands = vec![Command::AddPart {
        name: "P".into(),
        parent: None,
    }];
    for (mesh, x, y) in [("A", 0.0, 0.0), ("B", 3.0, 0.5), ("C", 1.2, 2.5)] {
        let pts = vec![[x, y], [x + 2.0, y], [x + 2.0, y + 1.5], [x, y + 1.5]];
        commands.push(Command::AddMesh {
            name: mesh.into(),
            part: "P".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2, 0, 2, 3],
        });
    }
    for c in commands {
        model.apply(c).unwrap();
    }
    let mut scene = Scene::new(&model).unwrap();
    scene.advance(0.0);
    let size = egui::vec2(360.0, 240.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: fit_view(scene.bounds(), size.x as u32, size.y as u32),
    };
    let mut view = ViewState::default();
    view.select_all(vec![
        Selection::Mesh("A".into()),
        Selection::Mesh("B".into()),
        Selection::Mesh("C".into()),
    ]);
    let bounds = selection_overlay::selection_bounds(&scene, &model, &view).unwrap();
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
            selection_overlay::paint_outlines(&painter, &theme, &mapper, &scene, &view);
            selection_overlay::paint(&painter, &theme, &mapper, bounds);
            selection_overlay::paint_badge(
                &painter,
                &theme,
                &mapper,
                bounds,
                view.selected_count(),
            );
        });
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn multi_selection_in_the_dark_theme() {
    multi_selection_snapshot(Mode::Dark, "selection_multi_dark");
}

#[test]
fn multi_selection_in_the_light_theme() {
    multi_selection_snapshot(Mode::Light, "selection_multi_light");
}

/// Proportional edit mid drag: the options box (on), the circle of influence
/// around the dragged vertex and the weight dots on its neighbours, drawn
/// over the moved mesh without the GPU picture behind it.
fn proportional_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::mesh_tool::{Mapper, MeshTool};
    use tsukumoe_app::panels::{mesh_overlay, overlay, proportional_overlay};
    use tsukumoe_app::proportional::Proportional;
    use tsukumoe_core::deformer_space::Space;
    use tsukumoe_ui::Theme;

    let mut positions = Vec::new();
    for y in 0..4 {
        for x in 0..5 {
            positions.push([x as f32, y as f32]);
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
    tool.proportional = Proportional {
        on: true,
        radius: 90.0,
        ..Proportional::default()
    };
    let start = mapper.to_screen(positions[7]);
    tool.press(&positions, &indices, &mapper, start, false);
    tool.drag([start[0] + 18.0, start[1] - 24.0]);
    let shown = tool
        .dragging_stored(&positions, &mapper, &Space::Drawing)
        .unwrap();
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, tool: &mut MeshTool| {
            let ctx = ui.ctx().clone();
            egui_extras::install_image_loaders(&ctx);
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            mesh_overlay::paint(&painter, &theme, &mapper, &shown, &indices, tool, None);
            let screen: Vec<egui::Pos2> = shown
                .iter()
                .map(|&p| {
                    let s = mapper.to_screen(p);
                    egui::pos2(s[0], s[1])
                })
                .collect();
            let weights = tool.drag_weights(&mapper).unwrap();
            let moved = [screen[7]];
            proportional_overlay::paint(
                &painter,
                &theme,
                tool.proportional.screen_radius(&mapper),
                &moved,
                Some((&screen, &weights)),
            );
            let mut options = tool.proportional;
            overlay::flyout(&ctx, rect, |ui| {
                proportional_overlay::options(ui, &mut options);
            });
        },
        tool,
    );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn proportional_edit_in_the_dark_theme() {
    proportional_snapshot(Mode::Dark, "proportional_dark");
}

#[test]
fn proportional_edit_in_the_light_theme() {
    proportional_snapshot(Mode::Light, "proportional_light");
}

/// The tool bar with the mesh tool and proportional edit pressed.
fn toolbar_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::actions::{Tool, cubism_compatible};
    use tsukumoe_app::panels::toolbar;

    let keymap = cubism_compatible();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(120.0, 260.0))
        .wgpu()
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            egui_extras::install_image_loaders(&ctx);
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            toolbar::show(&ctx, ui.max_rect(), Tool::Mesh, true, &keymap);
        });
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn toolbar_in_the_dark_theme() {
    toolbar_snapshot(Mode::Dark, "toolbar_dark");
}

#[test]
fn toolbar_in_the_light_theme() {
    toolbar_snapshot(Mode::Light, "toolbar_light");
}

/// The parameter panel with a bound mesh selected: the key ticks on the
/// slider, the edit section of the focused parameter and the key buttons.
fn parameters_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::panels::parameters;
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::{Command, ElementRef};
    use tsukumoe_core::model::Model;

    let mut model = Model::new();
    let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    for command in [
        Command::AddParameter {
            name: "ParamAngleX".into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        },
        Command::AddParameter {
            name: "ParamEyeOpen".into(),
            min: 0.0,
            max: 1.0,
            default: 1.0,
            repeat: false,
        },
        Command::AddPart {
            name: "Part".into(),
            parent: None,
        },
        Command::AddMesh {
            name: "Skin".into(),
            part: "Part".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2],
        },
        Command::BindParameter {
            parameter: "ParamAngleX".into(),
            elements: vec![ElementRef::Mesh {
                name: "Skin".into(),
            }],
        },
        Command::AddParameterKey {
            parameter: "ParamAngleX".into(),
            value: 12.0,
            element: ElementRef::Mesh {
                name: "Skin".into(),
            },
        },
        // A second axis: the panel shows the key grid.
        Command::BindParameter {
            parameter: "ParamEyeOpen".into(),
            elements: vec![ElementRef::Mesh {
                name: "Skin".into(),
            }],
        },
        Command::SetMeshKeyform {
            name: "Skin".into(),
            key: vec![3, 0],
            positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
            opacity: 1.0,
            draw_order: 500.0,
        },
    ] {
        model.apply(command).unwrap();
    }
    let mut view = ViewState::default();
    // Linked: shown as one pad with a number field for each.
    view.param_links = vec![("ParamAngleX".into(), "ParamEyeOpen".into())];
    view.select_only(Selection::Mesh("Skin".into()));
    view.param_focus = Some("ParamAngleX".into());
    let id = model.parameters().find("ParamAngleX").unwrap();
    let p = model.parameters().get(id).unwrap().clone();
    view.set_param("ParamAngleX", &p, 12.0);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 300.0))
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut (Model, ViewState)| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let (model, view) = state;
                parameters::show(ui, model, view);
            },
            (model, view),
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

fn keys_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::panels::keys;
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::{Command, ElementRef};
    use tsukumoe_core::model::Model;

    let mut model = Model::new();
    let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    for command in [
        Command::AddParameter {
            name: "ParamAngleX".into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        },
        Command::AddParameter {
            name: "ParamEyeOpen".into(),
            min: 0.0,
            max: 1.0,
            default: 1.0,
            repeat: false,
        },
        Command::AddPart {
            name: "Part".into(),
            parent: None,
        },
        Command::AddMesh {
            name: "Skin".into(),
            part: "Part".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2],
        },
        Command::BindParameter {
            parameter: "ParamAngleX".into(),
            elements: vec![ElementRef::Mesh {
                name: "Skin".into(),
            }],
        },
        Command::AddParameterKey {
            parameter: "ParamAngleX".into(),
            value: 12.0,
            element: ElementRef::Mesh {
                name: "Skin".into(),
            },
        },
        // A second axis: the panel shows the key grid.
        Command::BindParameter {
            parameter: "ParamEyeOpen".into(),
            elements: vec![ElementRef::Mesh {
                name: "Skin".into(),
            }],
        },
        Command::SetMeshKeyform {
            name: "Skin".into(),
            key: vec![3, 0],
            positions: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]],
            opacity: 1.0,
            draw_order: 500.0,
        },
    ] {
        model.apply(command).unwrap();
    }
    let mut view = ViewState::default();
    view.select_only(Selection::Mesh("Skin".into()));
    view.param_focus = Some("ParamAngleX".into());
    let id = model.parameters().find("ParamAngleX").unwrap();
    let p = model.parameters().get(id).unwrap().clone();
    view.set_param("ParamAngleX", &p, 12.0);
    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 480.0))
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut (Model, ViewState)| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let (model, view) = state;
                keys::show(ui, model, view);
            },
            (model, view),
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn parameters_panel_in_the_dark_theme() {
    parameters_snapshot(Mode::Dark, "parameters_dark");
}

#[test]
fn parameters_panel_in_the_light_theme() {
    parameters_snapshot(Mode::Light, "parameters_light");
}

#[test]
fn keys_panel_in_the_dark_theme() {
    keys_snapshot(Mode::Dark, "keys_dark");
}

#[test]
fn keys_panel_in_the_light_theme() {
    keys_snapshot(Mode::Light, "keys_light");
}

/// The help box at the bottom left with the key being edited. The off-key
/// line is covered by the unit test in `overlay`.
fn keyform_caption_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::keyed::Place;
    use tsukumoe_app::panels::overlay;
    use tsukumoe_ui::Theme;

    let size = egui::vec2(420.0, 110.0);
    let mut harness = Harness::builder()
        .with_size(size)
        .wgpu()
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            egui_extras::install_image_loaders(&ctx);
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let painter = ui.painter_at(ui.max_rect());
            painter.rect_filled(ui.max_rect(), 0.0, theme.colors.background);
            let on = Place::Key {
                key: vec![2],
                flat: 2,
                label: "ParamAngleX = 30.00".into(),
            };
            overlay::help(
                &ctx,
                ui.max_rect(),
                "Click to select, Shift to add.",
                Some(&on),
            );
        });
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn keyform_caption_in_the_dark_theme() {
    keyform_caption_snapshot(Mode::Dark, "keyform_caption_dark");
}

#[test]
fn keyform_caption_in_the_light_theme() {
    keyform_caption_snapshot(Mode::Light, "keyform_caption_light");
}

/// The settings dialog on its pie tab: tall enough for the slots, a picture of
/// the pie beside the numbered list.
#[test]
fn the_pie_tab_of_the_settings_dialog_in_the_light_theme() {
    let mut editor = Editor::new(None);
    assert!(editor.run_action(&egui::Context::default(), "view.pie-settings"));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .wgpu()
        .build_ui_state(
            |ui, editor: &mut Editor| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, Mode::Light);
                editor.ui(ui, None);
            },
            editor,
        );
    harness.run();
    harness.snapshot_options("pie_settings_light", &SnapshotOptions::new().threshold(3.0));
}

/// The shortcuts tab: the names in one column, the fields in another.
#[test]
fn the_shortcuts_tab_of_the_settings_dialog_in_the_dark_theme() {
    let mut editor = Editor::new(None);
    assert!(editor.run_action(&egui::Context::default(), "view.keymap-settings"));
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 800.0))
        .wgpu()
        .build_ui_state(
            |ui, editor: &mut Editor| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::Ja);
                set_mode(&ctx, Mode::Dark);
                editor.ui(ui, None);
            },
            editor,
        );
    harness.run();
    harness.snapshot_options("shortcuts_dark", &SnapshotOptions::new().threshold(3.0));
}

/// The view tab (a drop-down and number fields): the names share one column, so
/// the fields line up.
#[test]
fn the_view_tab_of_the_settings_dialog_in_the_dark_theme() {
    let mut editor = Editor::new(None);
    // The view tab has no action of its own: open the settings and switch to it.
    assert!(editor.run_action(&egui::Context::default(), "view.settings"));
    if let Some(tsukumoe_app::editor::Dialog::Settings(state)) = &mut editor.dialog {
        state.tab = tsukumoe_app::editor::SettingsTab::View;
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1100.0, 600.0))
        .wgpu()
        .build_ui_state(
            |ui, editor: &mut Editor| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::Ja);
                set_mode(&ctx, Mode::Dark);
                editor.ui(ui, None);
            },
            editor,
        );
    harness.run();
    harness.snapshot_options("settings_view_dark", &SnapshotOptions::new().threshold(3.0));
}

fn model_settings_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::panels::model_settings;
    use tsukumoe_app::view::ViewState;
    use tsukumoe_core::command::Command;
    use tsukumoe_core::model::Model;
    use tsukumoe_core::settings::StandardRole;

    let mut model = Model::new();
    let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let mut steps = vec![Command::AddPart {
        name: "Part".into(),
        parent: None,
    }];
    for parameter in ["ParamAngleX", "ParamAngleY", "ParamMouthOpenY"] {
        steps.push(Command::AddParameter {
            name: parameter.into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        });
    }
    for mesh in ["Face", "Body"] {
        steps.push(Command::AddMesh {
            name: mesh.into(),
            part: "Part".into(),
            positions: pts.clone(),
            uvs: pts.clone(),
            indices: vec![0, 1, 2],
        });
    }
    steps.extend([
        Command::SetStandardParameter {
            role: StandardRole::AngleX,
            parameter: Some("ParamAngleX".into()),
        },
        Command::SetStandardParameter {
            role: StandardRole::MouthOpen,
            parameter: Some("ParamMouthOpenY".into()),
        },
        Command::AddHitArea {
            name: "Head".into(),
            mesh: "Face".into(),
        },
        Command::AddHitArea {
            name: "Body".into(),
            mesh: "Body".into(),
        },
        Command::SetModelInfo {
            title: "Sample".into(),
            author: "Someone".into(),
            license: "CC0".into(),
            notes: String::new(),
        },
    ]);
    for command in steps {
        model.apply(command).unwrap();
    }
    let mut harness = Harness::builder()
        .with_size(egui::vec2(420.0, 760.0))
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut (Model, ViewState)| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let (model, view) = state;
                model_settings::show(ui, model, view);
            },
            (model, ViewState::default()),
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn model_settings_panel_in_the_dark_theme() {
    model_settings_snapshot(Mode::Dark, "model_settings_dark");
}

#[test]
fn model_settings_panel_in_the_light_theme() {
    model_settings_snapshot(Mode::Light, "model_settings_light");
}

/// The bones panel, and the bone tool's overlay: the bones over a mesh whose
/// vertices are coloured by the weight of the picked bone, with the brush.
fn bones_snapshot(mode: Mode, name: &str, overlay: bool) {
    use tsukumoe_app::bone_tool::{self, BoneTool};
    use tsukumoe_app::mesh_tool::Mapper;
    use tsukumoe_app::panels::{bone_overlay, bones, overlay};
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::{BoneBase, BoneWeight, Command};
    use tsukumoe_core::deformer_space::Space;
    use tsukumoe_core::model::Model;
    use tsukumoe_ui::Theme;

    let base = |x: f32, y: f32| BoneBase {
        translation: [x, y],
        rotation_degrees: 0.0,
        scale: [1.0, 1.0],
    };
    let mut positions = Vec::new();
    for y in 0..4 {
        for x in 0..5 {
            positions.push([x as f32, y as f32]);
        }
    }
    let mut indices = Vec::new();
    for y in 0..3u32 {
        for x in 0..4u32 {
            let a = y * 5 + x;
            indices.extend([a, a + 1, a + 6, a, a + 6, a + 5]);
        }
    }
    let weights: Vec<Vec<BoneWeight>> = positions
        .iter()
        .map(|p| {
            let arm = (p[0] / 4.0).clamp(0.0, 1.0);
            vec![
                BoneWeight {
                    bone: "Arm".into(),
                    weight: arm,
                },
                BoneWeight {
                    bone: "Root".into(),
                    weight: 1.0 - arm,
                },
            ]
        })
        .collect();
    let build = || {
        let mut model = Model::new();
        for command in [
            Command::AddPart {
                name: "P".into(),
                parent: None,
            },
            Command::AddMesh {
                name: "Skin".into(),
                part: "P".into(),
                positions: positions.clone(),
                uvs: positions.clone(),
                indices: indices.clone(),
            },
            Command::AddBone {
                name: "Root".into(),
                parent: None,
                base: base(0.5, 1.5),
            },
            Command::AddBone {
                name: "Arm".into(),
                parent: Some("Root".into()),
                base: base(3.0, 0.0),
            },
            Command::SetMeshInfluences {
                name: "Skin".into(),
                influences: weights.clone(),
            },
        ] {
            model.apply(command).unwrap();
        }
        model
    };
    let model = build();

    // The panel.
    let mut view = ViewState::default();
    view.select_only(Selection::Bone("Arm".into()));
    let mut panel = Harness::builder()
        .with_size(egui::vec2(420.0, 320.0))
        .wgpu()
        .build_ui_state(
            move |ui, state: &mut (Model, ViewState)| {
                let ctx = ui.ctx().clone();
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::En);
                set_mode(&ctx, mode);
                let (model, view) = state;
                bones::show(ui, model, view);
            },
            (build(), view),
        );
    if !overlay {
        panel.run();
        panel.snapshot_options(
            format!("{name}_panel"),
            &SnapshotOptions::new().threshold(3.0),
        );
        return;
    }

    // The overlay.
    let size = egui::vec2(480.0, 300.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [-0.8, -0.4, 5.2, 3.6],
    };
    let at = |_: tsukumoe_core::elements::ParameterId| 0.0;
    let joints = bone_tool::joints(&model, &Space::Drawing, &at);
    let heat = bone_tool::heat(&bone_tool::weights_of(&model, "Skin").unwrap(), "Arm");
    let vertices: Vec<[f32; 2]> = (0..4)
        .flat_map(|y| (0..5).map(move |x| [x as f32, y as f32]))
        .collect();
    let edges: Vec<[u32; 2]> = (0..3u32)
        .flat_map(|y| {
            (0..4u32).flat_map(move |x| {
                let a = y * 5 + x;
                [[a, a + 1], [a, a + 5]]
            })
        })
        .collect();
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, tool: &mut BoneTool| {
            let ctx = ui.ctx().clone();
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme.colors.surface_raised);
            bone_overlay::paint_weights(&painter, &theme, &mapper, &vertices, &edges, &heat);
            bone_overlay::paint_bones(&painter, &theme, &mapper, &joints, Some("Arm"), None);
            bone_overlay::paint_brush(
                &painter,
                &theme,
                egui::pos2(300.0, 120.0),
                tool.brush.radius,
            );
            overlay::flyout(&ctx, rect, |ui| bone_overlay::options(ui, tool));
        },
        {
            let mut tool = BoneTool::default();
            tool.mode = bone_tool::Mode::Weights;
            tool
        },
    );
    harness.run();
    harness.snapshot_options(
        format!("{name}_overlay"),
        &SnapshotOptions::new().threshold(3.0),
    );
}

#[test]
fn bones_panel_in_the_dark_theme() {
    bones_snapshot(Mode::Dark, "bones_dark", false);
}

#[test]
fn bones_panel_in_the_light_theme() {
    bones_snapshot(Mode::Light, "bones_light", false);
}

#[test]
fn bone_weights_overlay_in_the_dark_theme() {
    bones_snapshot(Mode::Dark, "bones_dark", true);
}

#[test]
fn bone_weights_overlay_in_the_light_theme() {
    bones_snapshot(Mode::Light, "bones_light", true);
}

/// The view aids over a plain ground: grid, guides and rulers, and the box.
fn view_aids_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::mesh_tool::Mapper;
    use tsukumoe_app::panels::{overlay, view_overlay};
    use tsukumoe_app::view_aids::ViewAids;
    use tsukumoe_ui::Theme;

    let size = egui::vec2(480.0, 320.0);
    let mapper = Mapper {
        origin: [0.0, 0.0],
        size: [size.x, size.y],
        view: [-50.0, -30.0, 430.0, 290.0],
    };
    let aids = ViewAids {
        grid: true,
        rulers: true,
        snap: true,
        guides_x: vec![120.0],
        guides_y: vec![80.0],
        ..ViewAids::default()
    };
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, state: &mut (ViewAids, bool)| {
            let ctx = ui.ctx().clone();
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let theme = Theme::get(&ctx);
            let rect = ui.max_rect();
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, theme.colors.surface_raised);
            let (aids, open) = state;
            view_overlay::paint_grid(&painter, &theme, rect, &mapper, aids);
            view_overlay::paint_guides(&painter, &theme, rect, &mapper, aids);
            view_overlay::paint_rulers(
                &painter,
                &theme,
                rect,
                &mapper,
                Some(egui::pos2(200.0, 150.0)),
            );
            overlay::top_right(&ctx, rect, |ui| {
                view_overlay::options(ui, aids, open, false);
            });
        },
        (aids, true),
    );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn view_aids_in_the_dark_theme() {
    view_aids_snapshot(Mode::Dark, "view_aids_dark");
}

#[test]
fn view_aids_in_the_light_theme() {
    view_aids_snapshot(Mode::Light, "view_aids_light");
}

/// One of the lower or side panels over a small model with a motion, an
/// expression, a pendulum and a mesh selected.
fn panel_snapshot(mode: Mode, name: &str, size: egui::Vec2, panel: &'static str) {
    use tsukumoe_app::panels::{expressions, inspector, physics, timeline};
    use tsukumoe_app::view::{Selection, ViewState};
    use tsukumoe_core::command::Command;
    use tsukumoe_core::model::Model;
    use tsukumoe_core::playback::Segment;

    let mut model = Model::new();
    let pts = vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]];
    let steps = [
        Command::AddParameter {
            name: "ParamAngleX".into(),
            min: -30.0,
            max: 30.0,
            default: 0.0,
            repeat: false,
        },
        Command::AddPart {
            name: "Head".into(),
            parent: None,
        },
        Command::AddMesh {
            name: "Face".into(),
            part: "Head".into(),
            positions: pts.clone(),
            uvs: pts,
            indices: vec![0, 1, 2],
        },
        Command::AddMotion {
            name: "Idle".into(),
            duration: 3.0,
            fps: 30.0,
            looped: true,
        },
        Command::AddMotionCurve {
            motion: "Idle".into(),
            target: tsukumoe_core::command::MotionTarget::Parameter {
                name: "ParamAngleX".into(),
            },
        },
        Command::SetMotionCurve {
            motion: "Idle".into(),
            curve: 0,
            start: [0.0, 0.0],
            segments: vec![Segment::Linear([1.5, 20.0]), Segment::Linear([3.0, 0.0])],
        },
        Command::AddExpression {
            name: "Smile".into(),
            fade_in: 0.5,
            fade_out: 0.5,
        },
        Command::AddPendulum {
            name: "Hair".into(),
        },
    ];
    for command in steps {
        model.apply(command).unwrap();
    }
    let mut view = ViewState::default();
    view.timeline.motion = Some("Idle".into());
    if panel == "curve" {
        view.timeline.curve = Some(0);
        view.timeline.key = Some(1);
    }
    view.expression.name = Some("Smile".into());
    view.physics.selected = Some((physics::Kind::Pendulum, "Hair".into()));
    view.select_only(Selection::Mesh("Face".into()));
    let mut harness = Harness::builder().with_size(size).wgpu().build_ui_state(
        move |ui, state: &mut (Model, ViewState)| {
            let ctx = ui.ctx().clone();
            egui_extras::install_image_loaders(&ctx);
            tsukumoe_ui::install_cjk_font(&ctx);
            set_language(&ctx, Language::En);
            set_mode(&ctx, mode);
            let (model, view) = state;
            match panel {
                "timeline" | "curve" => drop(timeline::show(ui, model, view)),
                "expressions" => drop(expressions::show(ui, model, view)),
                "physics" => drop(physics::show(ui, model, view)),
                "history" => {
                    let mut dispatcher = tsukumoe_app::dispatcher::Dispatcher::new(
                        tsukumoe_core::model::Model::new(),
                    );
                    for name in ["Head", "Body", "Arm"] {
                        let _ = dispatcher.submit(tsukumoe_core::command::Command::AddPart {
                            name: name.into(),
                            parent: None,
                        });
                    }
                    let _ = dispatcher.undo();
                    tsukumoe_app::panels::history::show(ui, &mut dispatcher);
                }
                _ => drop(inspector::show(ui, model, view.selection.as_ref(), None)),
            }
        },
        (model, view),
    );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn timeline_panel_in_the_dark_theme() {
    panel_snapshot(
        Mode::Dark,
        "timeline_dark",
        egui::vec2(840.0, 300.0),
        "timeline",
    );
}

#[test]
fn expressions_panel_in_the_dark_theme() {
    panel_snapshot(
        Mode::Dark,
        "expressions_dark",
        egui::vec2(640.0, 300.0),
        "expressions",
    );
}

#[test]
fn physics_panel_in_the_dark_theme() {
    panel_snapshot(
        Mode::Dark,
        "physics_dark",
        egui::vec2(640.0, 460.0),
        "physics",
    );
}

#[test]
fn inspector_panel_in_the_dark_theme() {
    panel_snapshot(
        Mode::Dark,
        "inspector_dark",
        egui::vec2(240.0, 460.0),
        "inspector",
    );
}

/// The command palette open at a point, with a query typed in.
fn palette_snapshot(mode: Mode, name: &str) {
    use tsukumoe_app::panels::palette::Palette;

    let mut palette = Palette::default();
    palette.open_at(egui::pos2(24.0, 24.0));
    palette.query = "mesh".into();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(520.0, 420.0))
        .wgpu()
        .build_ui_state(
            move |ui, palette: &mut Palette| {
                let ctx = ui.ctx().clone();
                egui_extras::install_image_loaders(&ctx);
                tsukumoe_ui::install_cjk_font(&ctx);
                set_language(&ctx, Language::Ja);
                set_mode(&ctx, mode);
                palette.show(&ctx, &tsukumoe_app::actions::cubism_compatible());
            },
            palette,
        );
    harness.run();
    harness.snapshot_options(name, &SnapshotOptions::new().threshold(3.0));
}

#[test]
fn palette_in_the_dark_theme() {
    palette_snapshot(Mode::Dark, "palette_dark");
}

#[test]
fn palette_in_the_light_theme() {
    palette_snapshot(Mode::Light, "palette_light");
}

#[test]
fn curve_editor_in_the_dark_theme() {
    panel_snapshot(Mode::Dark, "curve_dark", egui::vec2(840.0, 520.0), "curve");
}

#[test]
fn history_panel_in_the_dark_theme() {
    panel_snapshot(
        Mode::Dark,
        "history_dark",
        egui::vec2(400.0, 220.0),
        "history",
    );
}
