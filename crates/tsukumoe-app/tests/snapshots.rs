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
