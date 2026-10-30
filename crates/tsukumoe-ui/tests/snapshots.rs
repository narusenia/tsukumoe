// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Image comparison of the gallery: each section in the dark and light themes
//! and in both densities. A change in how a component looks fails here.
//!
//! Update the stored images after an intended change with
//! `UPDATE_SNAPSHOTS=1 cargo test -p tsukumoe-ui --test snapshots`, and look
//! at the images before committing them.

use egui_kittest::{Harness, SnapshotResults};
use tsukumoe_ui::showcase::{Section, Showcase};
use tsukumoe_ui::{Density, Mode, Theme, set_density, set_mode};

fn name(section: Section) -> String {
    format!("{section:?}").to_lowercase()
}

fn snapshot(
    results: &mut SnapshotResults,
    section: Section,
    mode: Mode,
    density: Density,
    tag: &str,
) {
    let mut showcase = Showcase::new();
    let mut harness = Harness::builder()
        .with_size(egui::vec2(560.0, 200.0))
        .wgpu()
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            // Idempotent; must happen before the first frame draws an icon.
            egui_extras::install_image_loaders(&ctx);
            tsukumoe_ui::install_cjk_font(&ctx);
            set_density(&ctx, density);
            set_mode(&ctx, mode);
            let t = Theme::get(&ctx);
            egui::Frame::NONE
                .fill(t.colors.background)
                .inner_margin(t.metrics.pad)
                .show(ui, |ui| showcase.show(ui, section));
        });
    harness.fit_contents();
    harness.run();
    harness.fit_contents();
    harness.snapshot(format!("{}_{tag}", name(section)));
    results.extend_harness(&mut harness);
}

#[test]
fn sections_in_the_dark_theme() {
    let mut results = SnapshotResults::new();
    for section in Section::ALL {
        snapshot(&mut results, section, Mode::Dark, Density::Compact, "dark");
    }
    results.unwrap();
}

#[test]
fn sections_in_the_light_theme() {
    let mut results = SnapshotResults::new();
    for section in Section::ALL {
        snapshot(
            &mut results,
            section,
            Mode::Light,
            Density::Compact,
            "light",
        );
    }
    results.unwrap();
}

#[test]
fn the_regular_density() {
    let mut results = SnapshotResults::new();
    for section in [Section::Buttons, Section::Fields, Section::Structure] {
        snapshot(
            &mut results,
            section,
            Mode::Dark,
            Density::Regular,
            "dark_regular",
        );
    }
    results.unwrap();
}

#[test]
fn the_built_in_named_themes() {
    let mut results = SnapshotResults::new();
    for named in tsukumoe_ui::builtin_themes() {
        for section in [Section::Buttons, Section::Fields, Section::Structure] {
            let mut showcase = Showcase::new();
            let theme = named.theme;
            let mut harness = Harness::builder()
                .with_size(egui::vec2(560.0, 200.0))
                .wgpu()
                .build_ui(move |ui| {
                    let ctx = ui.ctx().clone();
                    egui_extras::install_image_loaders(&ctx);
                    tsukumoe_ui::install_cjk_font(&ctx);
                    tsukumoe_ui::set_theme(&ctx, theme);
                    egui::Frame::NONE
                        .fill(theme.colors.background)
                        .inner_margin(theme.metrics.pad)
                        .show(ui, |ui| showcase.show(ui, section));
                });
            harness.fit_contents();
            harness.run();
            harness.fit_contents();
            let theme_name = named.name.to_lowercase().replace(' ', "_");
            harness.snapshot(format!("{}_{theme_name}", name(section)));
            results.extend_harness(&mut harness);
        }
    }
    results.unwrap();
}

#[test]
fn the_theme_section_in_both_languages() {
    let mut results = SnapshotResults::new();
    for language in [tsukumoe_ui::Language::En, tsukumoe_ui::Language::Ja] {
        let mut showcase = Showcase::new();
        let mut harness = Harness::builder()
            .with_size(egui::vec2(560.0, 200.0))
            .wgpu()
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                tsukumoe_ui::install_cjk_font(&ctx);
                set_mode(&ctx, Mode::Dark);
                tsukumoe_ui::set_language(&ctx, language);
                let t = Theme::get(&ctx);
                egui::Frame::NONE
                    .fill(t.colors.background)
                    .inner_margin(t.metrics.pad)
                    .show(ui, |ui| showcase.show(ui, Section::Theme));
            });
        harness.fit_contents();
        harness.run();
        harness.fit_contents();
        harness.snapshot(format!("theme_{}", language.code()));
        results.extend_harness(&mut harness);
    }
    results.unwrap();
}
