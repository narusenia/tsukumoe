// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The viewport: the model drawn by `tsukumoe-gpu` into a texture on egui's
//! own wgpu device, shown as an image. It redraws only while something moves
//! (input, a parameter, physics settling), as the host does (ADR-0027).
//! Pan and zoom live here; tools plug in through the input layer later.

use std::time::{Duration, Instant};

use eframe::egui_wgpu::RenderState;
use egui::{PointerButton, Rect, Sense, TextureId, Ui, pos2};
use tsukumoe_gpu::{Renderer, Texture};
use tsukumoe_ui::{hint, tr};

use crate::drag::DragAssignment;
use crate::project::Document;
use crate::scene::{Scene, fit_view};
use crate::view::ViewState;

/// How long to keep drawing after a change, so physics can settle.
const SETTLE: Duration = Duration::from_secs(3);

struct Target {
    view: wgpu::TextureView,
    id: TextureId,
    size: (u32, u32),
}

pub struct Viewport {
    doc_id: u64,
    model_revision: u64,
    view_revision: u64,
    scene: Option<Scene>,
    renderer: Renderer,
    target: Option<Target>,
    last_frame: Instant,
    active_until: Instant,
    pub error: Option<String>,
}

impl Viewport {
    pub fn new(rs: &RenderState, doc: &Document) -> Result<Self, String> {
        let textures = doc
            .project()
            .files
            .iter()
            .filter(|(path, _)| path.starts_with("textures/") && path.ends_with(".png"))
            .map(|(path, bytes)| Texture::from_png(bytes).map_err(|e| format!("{path}: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        let mut renderer = Renderer::new(&rs.device);
        renderer
            .set_textures(&rs.device, &rs.queue, &textures)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            doc_id: doc.id,
            model_revision: u64::MAX,
            view_revision: u64::MAX,
            scene: None,
            renderer,
            target: None,
            last_frame: Instant::now(),
            active_until: Instant::now(),
            error: None,
        })
    }

    pub fn doc_id(&self) -> u64 {
        self.doc_id
    }

    fn wake(&mut self) {
        self.active_until = Instant::now() + SETTLE;
    }

    fn ensure_target(&mut self, rs: &RenderState, size: (u32, u32)) {
        if self.target.as_ref().map(|t| t.size) == Some(size) {
            return;
        }
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
        self.wake();
    }

    /// Draws the viewport into the free space of `ui`; returns its response.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rs: &RenderState,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
    ) -> egui::Response {
        let avail = ui.available_size();
        let ppp = ui.ctx().pixels_per_point();
        let size = (
            ((avail.x * ppp) as u32).max(1),
            ((avail.y * ppp) as u32).max(1),
        );
        let resized = self.target.as_ref().map(|t| t.size) != Some(size);
        self.ensure_target(rs, size);
        let (rect, response) = ui.allocate_exact_size(avail, Sense::click_and_drag());

        // Rebuild the evaluated model when the edited model changed.
        let model_rev = doc.dispatcher.revision();
        if self.scene.is_none() || self.model_revision != model_rev {
            match Scene::new(doc.dispatcher.model()) {
                Ok(scene) => {
                    self.scene = Some(scene);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.model_revision = model_rev;
            self.view_revision = u64::MAX;
        }
        if self.view_revision != view.revision() {
            if let Some(scene) = &mut self.scene {
                scene.apply(view);
            }
            self.view_revision = view.revision();
            self.wake();
        }
        self.navigate(ui, &response, rect, view);
        self.drag_parameters(ui, &response, doc, view, drag);

        let now = Instant::now();
        if (resized || now < self.active_until)
            && let Some(scene) = &mut self.scene
        {
            let dt = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
            scene.advance(dt);
            let target = self.target.as_ref().expect("target was just ensured");
            let fitted = fit_view(scene.bounds(), size.0, size.1);
            let list = scene.frame.draw_list(&scene.model);
            if let Err(e) = self.renderer.render_to_view(
                &rs.device,
                &rs.queue,
                &list,
                view.camera.view(fitted),
                &target.view,
                size,
            ) {
                self.error = Some(e.to_string());
            }
            ui.ctx().request_repaint();
        }
        self.last_frame = now;
        if let Some(target) = &self.target {
            ui.painter().image(
                target.id,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        if let Some(e) = &self.error {
            hint(ui, e.clone());
        }
        response
    }

    /// Modifier + drag moves the assigned parameters (view state only).
    fn drag_parameters(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
    ) {
        let modifiers = ui.input(|i| i.modifiers);
        if !response.dragged_by(PointerButton::Primary) || !drag.modifier_down(modifiers) {
            return;
        }
        let d = ui.input(|i| i.pointer.delta());
        let model = doc.dispatcher.model();
        let params = model.parameters();
        let updates = drag.drive([d.x, d.y], |name| {
            let p = params.get(params.find(name)?)?;
            Some((p.clone(), view.param(name, p)))
        });
        for (name, value) in updates {
            if let Some(p) = params.find(&name).and_then(|id| params.get(id)) {
                view.set_param(&name, p, value);
            }
        }
        self.wake();
    }

    /// Pan (middle drag, or Shift + drag) and zoom (wheel, pinch) the camera.
    fn navigate(&mut self, ui: &Ui, response: &egui::Response, rect: Rect, view: &mut ViewState) {
        let size = [rect.width(), rect.height()];
        let shift = ui.input(|i| i.modifiers.shift);
        if response.dragged_by(PointerButton::Middle)
            || (shift && response.dragged_by(PointerButton::Primary))
        {
            let d = ui.input(|i| i.pointer.delta());
            view.camera.pan([d.x, d.y], size);
            view.touch();
            self.wake();
        }
        if response.hovered() {
            let (scroll, pinch, hover) = ui.input(|i| {
                (
                    i.smooth_scroll_delta.y,
                    i.zoom_delta(),
                    i.pointer.hover_pos(),
                )
            });
            let factor = (scroll * 0.002).exp() * pinch;
            if (factor - 1.0).abs() > 1e-4
                && let Some(p) = hover
            {
                let anchor = [
                    (p.x - rect.left()) / size[0].max(1.0),
                    (p.y - rect.top()) / size[1].max(1.0),
                ];
                view.camera.zoom_at(factor, anchor);
                view.touch();
                self.wake();
            }
        }
    }
}

/// The panel body: the viewport when there is a GPU and a project, else why not.
pub fn placeholder(ui: &mut Ui, has_doc: bool) {
    let key = if has_doc {
        "viewport-no-gpu"
    } else {
        "app-no-project"
    };
    hint(ui, tr(ui.ctx(), key));
}
