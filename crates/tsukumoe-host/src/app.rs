// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The window: a parameter list on the left, the model on the right.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use eframe::egui::{self, Vec2};
use tsukumoe_ui::{Theme, Variant, button, hint, label, panel, slider};

use crate::viewer::{Viewer, open_project};

/// Pointer travel, in points, that moves a parameter across its whole range.
const DRAG_POINTS: f32 = 400.0;
/// How long to keep drawing after a change, so physics can settle. Drawing
/// stops after that: the host redraws only while something moves.
const SETTLE: Duration = Duration::from_secs(3);
const SIDEBAR_WIDTH: f32 = 300.0;
const READOUT_WIDTH: f32 = 40.0;

struct Target {
    view: wgpu::TextureView,
    id: egui::TextureId,
    size: (u32, u32),
}

struct Host {
    name: String,
    viewer: Viewer,
    target: Option<Target>,
    last_frame: Instant,
    /// Draw until this time.
    active_until: Instant,
    error: Option<String>,
}

impl Host {
    fn wake(&mut self) {
        self.active_until = Instant::now() + SETTLE;
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        let t = Theme::get(ui.ctx());
        panel(ui, |ui| {
            ui.set_width(SIDEBAR_WIDTH - 2.0 * t.metrics.pad);
            ui.horizontal(|ui| {
                label(ui, self.name.clone());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if button(ui, "Reset", Variant::Secondary).clicked() {
                        self.viewer.scene.reset();
                        self.wake();
                    }
                });
            });
            hint(
                ui,
                format!("{} parameters", self.viewer.scene.parameters().len()),
            );
            hint(
                ui,
                "Hold Space and move the mouse over the model: the first two parameters",
            );
            if let Some(e) = &self.error {
                hint(ui, e.clone());
            }
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    for i in 0..self.viewer.scene.parameters().len() {
                        let p = self.viewer.scene.parameters()[i].clone();
                        let mut value = self.viewer.scene.value(i);
                        ui.horizontal(|ui| {
                            ui.allocate_ui_with_layout(
                                Vec2::new(110.0, t.metrics.control),
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| ui.add(egui::Label::new(&p.name).truncate()),
                            );
                            let width = (ui.available_width() - READOUT_WIDTH).max(40.0);
                            let changed = ui
                                .allocate_ui(Vec2::new(width, t.metrics.control), |ui| {
                                    slider(ui, &mut value, p.min..=p.max, &p.name).changed()
                                })
                                .inner;
                            hint(ui, format!("{value:.2}"));
                            if changed {
                                self.viewer.scene.set_parameter(i, value);
                                self.wake();
                            }
                        });
                    }
                });
        });
    }

    /// Makes the render target match `size` (pixels), registering it with egui.
    fn ensure_target(&mut self, frame: &eframe::Frame, size: (u32, u32)) {
        if self.target.as_ref().map(|t| t.size) == Some(size) {
            return;
        }
        let rs = frame
            .wgpu_render_state()
            .expect("the wgpu renderer is selected");
        let mut renderer = rs.renderer.write();
        if let Some(old) = self.target.take() {
            renderer.free_texture(&old.id);
        }
        let texture = rs.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: tsukumoe_gpu::OUTPUT_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let id = renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear);
        self.target = Some(Target { view, id, size });
        // A new target is empty: draw it.
        self.active_until = self.active_until.max(Instant::now());
    }

    fn viewport(&mut self, ui: &mut egui::Ui, frame: &eframe::Frame) {
        let avail = ui.available_size();
        let ppp = ui.ctx().pixels_per_point();
        let size = (
            ((avail.x * ppp) as u32).max(1),
            ((avail.y * ppp) as u32).max(1),
        );
        let resized = self.target.as_ref().map(|t| t.size) != Some(size);
        self.ensure_target(frame, size);
        let (rect, response) = ui.allocate_exact_size(avail, egui::Sense::hover());

        if response.hovered() && ui.input(|i| i.key_down(egui::Key::Space)) {
            let delta = ui.input(|i| i.pointer.delta());
            // Right is +x and up is +y, as on a slider and in the model.
            if self
                .viewer
                .scene
                .drive(delta.x / DRAG_POINTS, -delta.y / DRAG_POINTS)
            {
                self.wake();
            }
        }

        let now = Instant::now();
        if resized || now < self.active_until {
            let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
            let rs = frame
                .wgpu_render_state()
                .expect("the wgpu renderer is selected");
            let target = self.target.as_ref().expect("target was just ensured");
            match self
                .viewer
                .render(&rs.device, &rs.queue, &target.view, size, dt)
            {
                Ok(()) => self.error = None,
                Err(e) => self.error = Some(e),
            }
            ui.ctx().request_repaint();
        }
        self.last_frame = now;
        if let Some(target) = &self.target {
            ui.painter().image(
                target.id,
                rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
    }
}

impl eframe::App for Host {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let Theme {
            colors, metrics, ..
        } = Theme::get(ui.ctx());
        let (background, pad) = (colors.background, metrics.pad);
        egui::Panel::left("parameters")
            .resizable(false)
            .exact_size(SIDEBAR_WIDTH)
            .frame(egui::Frame::NONE.fill(background).inner_margin(pad))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(background))
            .show(ui, |ui| self.viewport(ui, frame));
    }
}

/// Opens `path` and shows it until the window closes.
pub fn run(path: PathBuf) -> Result<(), String> {
    let project = open_project(&path)?;
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Tsukumoe - {name}"))
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "tsukumoe-host",
        options,
        Box::new(move |cc| {
            tsukumoe_ui::init(&cc.egui_ctx);
            let rs = cc
                .wgpu_render_state
                .as_ref()
                .ok_or("no wgpu render state")?;
            let viewer = Viewer::new(&rs.device, &rs.queue, &project)?;
            Ok(Box::new(Host {
                name,
                viewer,
                target: None,
                last_frame: Instant::now(),
                active_until: Instant::now() + SETTLE,
                error: None,
            }))
        }),
    )
    .map_err(|e| e.to_string())
}
