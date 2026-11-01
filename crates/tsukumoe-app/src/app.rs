// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The eframe window around [`Editor`].

use std::path::PathBuf;

use tsukumoe_ui::{Preferences, builtin_themes};

use crate::editor::Editor;
use crate::settings;

struct App {
    editor: Editor,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.editor.ui(ui, frame.wgpu_render_state());
    }
}

/// Opens the window, with `project` loaded when given.
pub fn run(project: Option<PathBuf>) -> Result<(), String> {
    let native = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("Tsukumoe")
            .with_inner_size([1400.0, 900.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tsukumoe-app",
        native,
        Box::new(move |cc| {
            tsukumoe_ui::init(&cc.egui_ctx);
            let config = settings::config_dir();
            // Theme and language are shared with the host's preferences file.
            let prefs = config
                .as_deref()
                .and_then(|d| settings::read(d, "preferences.json"))
                .map(|t| Preferences::from_json(&t))
                .unwrap_or_default();
            prefs.apply(&cc.egui_ctx, &builtin_themes());
            let mut editor = Editor::new(config.as_deref().map(settings::editor_dir));
            if let Some(path) = project {
                editor.open(&path)?;
            }
            Ok(Box::new(App { editor }))
        }),
    )
    .map_err(|e| e.to_string())
}
