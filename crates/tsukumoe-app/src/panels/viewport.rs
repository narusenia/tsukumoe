// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The viewport: the model drawn by `tsukumoe-gpu` into a texture on egui's
//! own wgpu device, shown as an image. It redraws only while something moves
//! (input, a parameter, physics settling), as the host does (ADR-0027).
//! Pan and zoom live here; tools plug in through the input layer later.

use std::time::{Duration, Instant};

use eframe::egui_wgpu::RenderState;
use egui::{
    Event, MouseWheelSource, MouseWheelUnit, PointerButton, Rect, Sense, TextureId, Ui, pos2,
};
use tsukumoe_gpu::{Renderer, Texture};
use tsukumoe_ui::{Theme, hint, tr};

use crate::actions::Tool;
use crate::drag::DragAssignment;
use crate::mesh_tool::{self, Hit, Mapper, MeshTool, Op, ToolError};
use crate::panels::mesh_overlay;
use crate::project::Document;
use crate::scene::{Scene, fit_view};
use crate::view::{Selection, ViewState};

/// How long to keep drawing after a change, so physics can settle.
const SETTLE: Duration = Duration::from_secs(3);

/// What scrolling does in the viewport. A trackpad (and the momentum after it)
/// pans; a mouse wheel zooms; pinch always zooms. Where the platform does not
/// say which device it is, points mean a trackpad and lines a wheel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scroll {
    Pan,
    Zoom,
}

/// The mode after this frame's events. A scroll keeps gliding for a few frames
/// after its last event, so without a new event the mode stays as it was.
fn scroll_mode(events: &[Event], before: Scroll) -> Scroll {
    events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::MouseWheel { unit, source, .. } => Some(match (source, unit) {
                (MouseWheelSource::Trackpad | MouseWheelSource::Momentum, _) => Scroll::Pan,
                (MouseWheelSource::Wheel, _) => Scroll::Zoom,
                (MouseWheelSource::Unknown, MouseWheelUnit::Point) => Scroll::Pan,
                (MouseWheelSource::Unknown, _) => Scroll::Zoom,
            }),
            _ => None,
        })
        .unwrap_or(before)
}

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
    scroll: Scroll,
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
            scroll: Scroll::Zoom,
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

    /// Draws the viewport into the free space of `ui`; returns its response and
    /// what the mesh edit tool asks for (commands to send, or why it cannot).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rs: &RenderState,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
        mesh: &mut MeshTool,
    ) -> (egui::Response, Vec<Result<Op, ToolError>>) {
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
        let ops = if view.tool == Tool::Mesh {
            self.mesh_tool_frame(ui, &response, rect, doc, view, drag, mesh, size)
        } else {
            Vec::new()
        };
        if let Some(e) = &self.error {
            hint(ui, e.clone());
        }
        (response, ops)
    }

    /// The mesh edit tool for one frame: pointer input, then the overlay.
    #[allow(clippy::too_many_arguments)]
    fn mesh_tool_frame(
        &self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
        tool: &mut MeshTool,
        size: (u32, u32),
    ) -> Vec<Result<Op, ToolError>> {
        let mut ops = Vec::new();
        let Some(scene) = &self.scene else {
            return ops;
        };
        let theme = Theme::get(ui.ctx());
        let painter = ui.painter_at(rect);
        let mapper = Mapper {
            origin: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
        };
        let model = doc.dispatcher.model();
        let mesh = tool
            .mesh()
            .and_then(|n| Some((n.to_owned(), model.meshes().get(model.meshes().find(n)?)?)));
        mesh_overlay::veil(&painter, &theme, rect);
        let (modifiers, escape) = ui.input(|i| (i.modifiers, i.key_pressed(egui::Key::Escape)));
        let arrow = |p: Option<egui::Pos2>| p.map(|p| [p.x, p.y]);
        // The press position: a drag is reported only after it has moved.
        let pressed = arrow(ui.input(|i| i.pointer.press_origin()));
        let now = arrow(response.interact_pointer_pos());
        let primary = PointerButton::Primary;
        let tool_input = !drag.modifier_down(modifiers);
        if let Some((name, mesh)) = &mesh {
            tool.sanitize(mesh.positions.len());
            if escape {
                tool.cancel();
            }
            if tool_input
                && response.drag_started_by(primary)
                && let Some(at) = pressed
            {
                tool.press(&mesh.positions, &mesh.indices, &mapper, at, modifiers.shift);
            }
            if response.dragged_by(primary)
                && let Some(at) = now
            {
                tool.drag(at);
            }
            if response.drag_stopped_by(primary)
                && let Some(op) = tool.release(name, &mesh.positions, &mapper)
            {
                ops.push(Ok(op));
            }
        }
        if tool_input
            && response.clicked_by(primary)
            && let Some(at) = now
        {
            let positions = mesh.as_ref().map_or(&[][..], |(_, m)| &m.positions[..]);
            let indices = mesh.as_ref().map_or(&[][..], |(_, m)| &m.indices[..]);
            let empty = mesh_tool::hit_test(positions, indices, &mapper, at) == Hit::Nothing;
            let picked = (empty && !modifiers.shift && !modifiers.command)
                .then(|| mesh_tool::pick_mesh(model, mapper.to_model(at)))
                .flatten();
            match (&mesh, picked) {
                (Some((name, _)), Some(p)) if &p != name => {
                    view.selection = Some(Selection::Mesh(p));
                    view.touch();
                }
                (None, Some(p)) => {
                    view.selection = Some(Selection::Mesh(p));
                    view.touch();
                }
                (Some((name, _)), _) if modifiers.command => {
                    if let Some(g) = mesh_tool::geometry_of(model, name) {
                        ops.push(tool.add_at(name, &g, &mapper, at));
                    }
                }
                (Some(_), _) => tool.click(positions, indices, &mapper, at, modifiers.shift),
                (None, None) => {}
            }
        }
        match &mesh {
            Some((_, m)) => {
                let preview = tool.preview(&m.positions, &mapper);
                mesh_overlay::paint(
                    &painter,
                    &theme,
                    &mapper,
                    &m.positions,
                    &m.indices,
                    tool,
                    preview.as_deref(),
                );
                mesh_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "mesh-edit-hint"));
            }
            None => mesh_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "mesh-edit-pick")),
        }
        ops
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

    /// Pan (trackpad scroll, middle drag, or Shift + drag) and zoom (mouse wheel,
    /// pinch) the camera.
    fn navigate(&mut self, ui: &Ui, response: &egui::Response, rect: Rect, view: &mut ViewState) {
        let size = [rect.width(), rect.height()];
        // The mesh tool takes Shift + drag for its box selection.
        let shift = ui.input(|i| i.modifiers.shift) && view.tool != Tool::Mesh;
        if response.dragged_by(PointerButton::Middle)
            || (shift && response.dragged_by(PointerButton::Primary))
        {
            let d = ui.input(|i| i.pointer.delta());
            view.camera.pan([d.x, d.y], size);
            view.touch();
            self.wake();
        }
        if response.hovered() {
            self.scroll = ui.input(|i| scroll_mode(&i.events, self.scroll));
            let (scroll, pinch, hover) =
                ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if self.scroll == Scroll::Pan && scroll != egui::Vec2::ZERO {
                view.camera.pan([scroll.x, scroll.y], size);
                view.touch();
                self.wake();
            }
            let wheel = if self.scroll == Scroll::Zoom {
                scroll.y
            } else {
                0.0
            };
            let factor = (wheel * 0.002).exp() * pinch;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll(unit: MouseWheelUnit, source: MouseWheelSource) -> Event {
        Event::MouseWheel {
            unit,
            delta: egui::vec2(0.0, 1.0),
            phase: egui::TouchPhase::Move,
            source,
            modifiers: egui::Modifiers::NONE,
        }
    }

    #[test]
    fn a_trackpad_pans_and_a_wheel_zooms() {
        use MouseWheelSource::*;
        use MouseWheelUnit::*;
        // macOS reports the device; a trackpad can scroll in points or lines
        for unit in [Point, Line] {
            assert_eq!(
                scroll_mode(&[scroll(unit, Trackpad)], Scroll::Zoom),
                Scroll::Pan
            );
            assert_eq!(
                scroll_mode(&[scroll(unit, Momentum)], Scroll::Zoom),
                Scroll::Pan
            );
            assert_eq!(
                scroll_mode(&[scroll(unit, Wheel)], Scroll::Pan),
                Scroll::Zoom
            );
        }
        // elsewhere the unit decides
        assert_eq!(
            scroll_mode(&[scroll(Point, Unknown)], Scroll::Zoom),
            Scroll::Pan
        );
        assert_eq!(
            scroll_mode(&[scroll(Line, Unknown)], Scroll::Pan),
            Scroll::Zoom
        );
        assert_eq!(
            scroll_mode(&[scroll(Page, Unknown)], Scroll::Pan),
            Scroll::Zoom
        );
    }

    #[test]
    fn the_mode_stays_while_a_scroll_glides_without_new_events() {
        use MouseWheelSource::*;
        use MouseWheelUnit::*;
        assert_eq!(scroll_mode(&[], Scroll::Pan), Scroll::Pan);
        assert_eq!(scroll_mode(&[], Scroll::Zoom), Scroll::Zoom);
        // the latest wheel event of the frame decides
        let mixed = [scroll(Line, Wheel), scroll(Point, Trackpad)];
        assert_eq!(scroll_mode(&mixed, Scroll::Zoom), Scroll::Pan);
    }
}
