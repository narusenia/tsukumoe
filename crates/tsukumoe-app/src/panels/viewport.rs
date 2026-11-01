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
use tsukumoe_gpu::{DrawOptions, Renderer, Texture};
use tsukumoe_ui::{Keymap, Theme, hint, tr};

use crate::actions::Tool;
use crate::deformer_tool::{self, DeformerTool};
use crate::drag::DragAssignment;
use crate::mesh_tool::{self, Hit, MENU_ACTIONS, Mapper, MeshTool, Op, ToolError};
use crate::panels::{deformer_overlay, mesh_overlay, proportional_overlay, selection_overlay};
use crate::project::Document;
use crate::scene::{Preview, Scene, fit_view};
use crate::view::{Selection, ViewState};
use tsukumoe_core::deformer_space::Space;

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
    /// The mesh being edited drawn alone on a transparent target, shown above
    /// the veil so that it is not dimmed; `focus_mesh` names what it holds.
    focus: Option<Target>,
    focus_mesh: Option<String>,
    /// Whether the last try drew `focus_mesh` into the focus target.
    focus_drawn: bool,
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
            focus: None,
            focus_mesh: None,
            focus_drawn: false,
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
        for old in [self.target.take(), self.focus.take()]
            .into_iter()
            .flatten()
        {
            renderer.free_texture(&old.id);
        }
        let mut make = || {
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
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            let id = renderer.register_native_texture(&rs.device, &view, wgpu::FilterMode::Linear);
            Target { view, id, size }
        };
        self.target = Some(make());
        self.focus = Some(make());
        // The new focus target is empty.
        self.focus_mesh = None;
        self.focus_drawn = false;
        drop(renderer);
        self.wake();
    }

    /// Draws the viewport into the free space of `ui`; returns its response and
    /// what the mesh edit tool asks for (commands to send, or why it cannot).
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rs: &RenderState,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
        mesh: &mut MeshTool,
        deformer: &mut DeformerTool,
        keymap: &Keymap,
    ) -> (
        egui::Response,
        Vec<Result<Op, ToolError>>,
        Vec<&'static str>,
    ) {
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
        mesh.proportional = view.proportional;
        deformer.proportional = view.proportional;
        // The wheel sets the influence radius while proportional edit drags.
        let radius_wheel = view.proportional.on
            && match view.tool {
                Tool::Mesh => mesh.is_moving(),
                Tool::Deformer => deformer.is_moving_points(),
                Tool::Select => false,
            };
        self.navigate(ui, &response, rect, view, radius_wheel);
        self.drag_parameters(ui, &response, doc, view, drag);
        // A shape being dragged is drawn as it will be committed, before it is.
        let live = self.live_preview(rect, doc, view, mesh, deformer, size);
        if let Some(scene) = &mut self.scene
            && scene.set_previews(&live)
        {
            self.wake();
        }

        let now = Instant::now();
        // The mesh the mesh tool edits, if the focus picture must show it.
        let wanted = (view.tool == Tool::Mesh)
            .then(|| mesh.mesh().map(str::to_owned))
            .flatten();
        if (resized || now < self.active_until || self.focus_mesh != wanted)
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
            // The edited mesh alone, even if hidden (the model is not changed).
            let focus = wanted
                .as_deref()
                .and_then(|n| scene.model.mesh_index(n))
                .zip(self.focus.as_ref());
            match focus {
                Some((index, focus)) => {
                    let list = scene.frame.draw_list(&scene.model);
                    let options = DrawOptions {
                        only: Some(index as u32),
                        ..Default::default()
                    };
                    self.focus_mesh = wanted.clone();
                    self.focus_drawn = false;
                    match self.renderer.render_to_view_with(
                        &rs.device,
                        &rs.queue,
                        &list,
                        view.camera.view(fitted),
                        &focus.view,
                        size,
                        options,
                    ) {
                        Ok(()) => self.focus_drawn = true,
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
                None => {
                    self.focus_mesh = wanted.clone();
                    self.focus_drawn = false;
                }
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
        let (ops, actions) = match view.tool {
            Tool::Mesh => {
                self.mesh_tool_frame(ui, &response, rect, doc, view, drag, mesh, size, keymap)
            }
            Tool::Deformer => self
                .deformer_tool_frame(ui, &response, rect, doc, view, drag, deformer, size, keymap),
            Tool::Select => {
                self.select_tool_frame(ui, &response, rect, view, drag, size);
                (Vec::new(), Vec::new())
            }
        };
        // The box around the selection, where no vertex-level overlay is open.
        if selection_overlay::shown_with(view.tool)
            && let Some(scene) = &self.scene
            && let Some(bounds) =
                selection_overlay::selection_bounds(scene, doc.dispatcher.model(), view)
        {
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            let painter = ui.painter_at(rect);
            let theme = Theme::get(ui.ctx());
            selection_overlay::paint_outlines(&painter, &theme, &mapper, scene, view);
            selection_overlay::paint(&painter, &theme, &mapper, bounds);
            selection_overlay::paint_badge(
                &painter,
                &theme,
                &mapper,
                bounds,
                view.selected_count(),
            );
        }
        if let Some(e) = &self.error {
            hint(ui, e.clone());
        }
        (response, ops, actions)
    }

    /// The drag in progress as a [`Preview`], from the active tool's state.
    fn live_preview(
        &self,
        rect: Rect,
        doc: &Document,
        view: &ViewState,
        mesh: &MeshTool,
        deformer: &DeformerTool,
        size: (u32, u32),
    ) -> Vec<Preview> {
        let Some(scene) = self.scene.as_ref() else {
            return Vec::new();
        };
        let model = doc.dispatcher.model();
        let mapper = Mapper {
            origin: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
        };
        match view.tool {
            Tool::Mesh => (|| {
                let name = mesh.mesh()?;
                let m = model.meshes().get(model.meshes().find(name)?)?;
                let space = Space::of(model, m.parent).ok()?;
                let positions = mesh.dragging_stored(&m.positions, &mapper, &space)?;
                Some(vec![Preview::Mesh {
                    name: name.to_owned(),
                    positions,
                }])
            })()
            .unwrap_or_default(),
            Tool::Deformer => (|| {
                let name = deformer.deformer()?;
                let radius = (mapper.view[2] - mapper.view[0]) * 0.05;
                let shape = deformer_tool::Shape::of(model, name, radius)?;
                // Dragging a rotation's origin moves the axis and corrects what hangs
                // from it: preview all of it, exactly as it would be committed.
                if let Some(axis) = deformer.axis_drag(name, &shape, &mapper, model) {
                    return Some(match axis {
                        Ok((command, _)) => crate::deformer_ops::previews_of(&command),
                        Err(_) => Vec::new(),
                    });
                }
                Some(vec![deformer.dragging(&shape, &mapper)?.as_preview(name)])
            })()
            .unwrap_or_default(),
            Tool::Select => Vec::new(),
        }
    }

    /// The select tool: a click picks the front mesh under the pointer, Shift
    /// adds it to (or takes it out of) the selection, and empty space clears it.
    #[allow(clippy::too_many_arguments)]
    fn select_tool_frame(
        &self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        view: &mut ViewState,
        drag: &DragAssignment,
        size: (u32, u32),
    ) {
        let (Some(scene), true) = (&self.scene, response.clicked_by(PointerButton::Primary)) else {
            return;
        };
        let modifiers = ui.input(|i| i.modifiers);
        let Some(p) = response.interact_pointer_pos() else {
            return;
        };
        if drag.modifier_down(modifiers) {
            return;
        }
        let mapper = Mapper {
            origin: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
        };
        let picked = mesh_tool::pick_drawn(
            &scene.frame.draw_list(&scene.model),
            mapper.to_model([p.x, p.y]),
        );
        match picked {
            Some(name) if modifiers.shift => view.toggle_mesh(&name),
            Some(name) => view.select_only(Selection::Mesh(name)),
            None if !modifiers.shift => {
                view.selection = None;
                view.touch();
            }
            None => {}
        }
    }

    /// The deformer tool for one frame: pointer input, then the overlay.
    #[allow(clippy::too_many_arguments)]
    fn deformer_tool_frame(
        &self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
        tool: &mut DeformerTool,
        size: (u32, u32),
        keymap: &Keymap,
    ) -> (Vec<Result<Op, ToolError>>, Vec<&'static str>) {
        let mut ops = Vec::new();
        let mut actions = Vec::new();
        let Some(scene) = &self.scene else {
            return (ops, actions);
        };
        let theme = Theme::get(ui.ctx());
        let painter = ui.painter_at(rect);
        let mapper = Mapper {
            origin: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
        };
        let model = doc.dispatcher.model();
        // Rotation handles are a twentieth of the view wide.
        let radius = (mapper.view[2] - mapper.view[0]) * 0.05;
        let name = tool.deformer().map(str::to_owned);
        let shape = name
            .as_deref()
            .and_then(|n| deformer_tool::Shape::of(model, n, radius));
        let (modifiers, escape) = ui.input(|i| (i.modifiers, i.key_pressed(egui::Key::Escape)));
        let arrow = |p: Option<egui::Pos2>| p.map(|p| [p.x, p.y]);
        let pressed = arrow(ui.input(|i| i.pointer.press_origin()));
        let now = arrow(response.interact_pointer_pos());
        let primary = PointerButton::Primary;
        let tool_input = !drag.modifier_down(modifiers);
        // Cmd / Ctrl on the origin handle moves the whole rotation, not just its axis.
        tool.whole = modifiers.command;
        if let (Some(name), Some(shape)) = (&name, &shape) {
            tool.sanitize(shape.world_points().len());
            if escape {
                tool.cancel();
            }
            if tool_input
                && response.drag_started_by(primary)
                && let Some(at) = pressed
            {
                tool.press(shape, &mapper, at, modifiers.shift);
            }
            if response.dragged_by(primary)
                && let Some(at) = now
            {
                tool.drag(at);
            }
            if response.drag_stopped_by(primary)
                && let Some(made) = tool.release_in(name, shape, &mapper, model)
            {
                ops.push(made.map_err(|e| ToolError::Deformer(e.key())));
            }
        }
        if tool_input
            && response.clicked_by(primary)
            && let Some(at) = now
        {
            let on_shape = shape.as_ref().is_some_and(|s| {
                deformer_tool::hit_test(s, &mapper, at) != deformer_tool::Hit::Nothing
            });
            match (&shape, on_shape) {
                (Some(shape), true) => tool.click(shape, &mapper, at, modifiers.shift),
                _ => match deformer_tool::pick_deformer(model, &mapper, at, radius) {
                    Some(picked) if !modifiers.shift => {
                        view.select_only(if model.warps().find(&picked).is_some() {
                            Selection::Warp(picked)
                        } else {
                            Selection::Rotation(picked)
                        });
                    }
                    _ => {
                        if let Some(shape) = &shape {
                            tool.click(shape, &mapper, at, modifiers.shift);
                        }
                    }
                },
            }
        }
        let ctx = ui.ctx().clone();
        response.context_menu(|ui| {
            let shortcut = |a: &str| keymap.shortcut_text(&ctx, a);
            if let Some(action) =
                mesh_overlay::menu_ui(ui, &deformer_tool::MENU_ACTIONS, &|_| true, &shortcut)
            {
                actions.push(action);
                ui.close();
            }
        });
        match (&name, &shape) {
            (Some(name), Some(shape)) => {
                let related = deformer_tool::related(model, name, radius);
                let preview = tool.preview(shape, &mapper);
                deformer_overlay::paint(
                    &painter,
                    &theme,
                    &mapper,
                    preview.as_ref().unwrap_or(shape),
                    &related,
                    tool,
                );
                deformer_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "deformer-hint"));
                if tool.proportional.on && matches!(shape, deformer_tool::Shape::Warp { .. }) {
                    let screen: Vec<egui::Pos2> = preview
                        .as_ref()
                        .unwrap_or(shape)
                        .world_points()
                        .iter()
                        .map(|&p| {
                            let s = mapper.to_screen(p);
                            pos2(s[0], s[1])
                        })
                        .collect();
                    let moved: Vec<egui::Pos2> = tool
                        .points()
                        .iter()
                        .filter_map(|&i| screen.get(i as usize).copied())
                        .collect();
                    let weights = tool.drag_weights(shape, &mapper);
                    proportional_overlay::paint(
                        &painter,
                        &theme,
                        &tool.proportional,
                        &moved,
                        weights.as_deref().map(|w| (&screen[..], w)),
                    );
                }
            }
            _ => deformer_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "deformer-pick")),
        }
        deformer_overlay::options(ui.ctx(), rect, tool);
        proportional_overlay::options(ui.ctx(), rect, &mut view.proportional);
        tool.proportional = view.proportional;
        (ops, actions)
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
        keymap: &Keymap,
    ) -> (Vec<Result<Op, ToolError>>, Vec<&'static str>) {
        let mut ops = Vec::new();
        let mut actions = Vec::new();
        let Some(scene) = &self.scene else {
            return (ops, actions);
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
        // The vertices as drawn (deformers, keyforms and parameters applied) are
        // what the pointer meets; the stored ones are only for the commands.
        let shown: Vec<[f32; 2]> = mesh
            .as_ref()
            .map(|(n, m)| {
                scene
                    .drawn_positions(n)
                    .filter(|p| p.len() == m.positions.len())
                    .unwrap_or_else(|| m.positions.clone())
            })
            .unwrap_or_default();
        let space = mesh
            .as_ref()
            .and_then(|(_, m)| Space::of(model, m.parent).ok())
            .unwrap_or(Space::Drawing);
        // The edited mesh is drawn again above the veil so it stays bright.
        let focus = self
            .focus
            .as_ref()
            .filter(|_| self.focus_drawn && self.focus_mesh.as_deref() == tool.mesh())
            .map(|f| f.id);
        mesh_overlay::veil(&painter, &theme, rect, focus);
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
                tool.press(&shown, &mesh.indices, &mapper, at, modifiers.shift);
            }
            if response.dragged_by(primary)
                && let Some(at) = now
            {
                tool.drag(at);
            }
            if response.drag_stopped_by(primary)
                && let Some(op) = tool.release_in(name, &mesh.positions, &shown, &mapper, &space)
            {
                ops.push(Ok(op));
            }
        }
        if tool_input
            && response.clicked_by(primary)
            && let Some(at) = now
        {
            let positions = &shown[..];
            let indices = mesh.as_ref().map_or(&[][..], |(_, m)| &m.indices[..]);
            let empty = mesh_tool::hit_test(positions, indices, &mapper, at) == Hit::Nothing;
            let picked = (empty && !modifiers.shift && !modifiers.command)
                .then(|| {
                    mesh_tool::pick_drawn(&scene.frame.draw_list(&scene.model), mapper.to_model(at))
                })
                .flatten();
            match (&mesh, picked) {
                (Some((name, _)), Some(p)) if &p != name => {
                    view.select_only(Selection::Mesh(p));
                }
                (None, Some(p)) => {
                    view.select_only(Selection::Mesh(p));
                }
                (Some((name, _)), _) if modifiers.command => {
                    if let Some(g) = mesh_tool::geometry_of(model, name) {
                        ops.push(tool.add_at_in(name, &g, &shown, &mapper, at, &space));
                    }
                }
                (Some(_), _) => tool.click(positions, indices, &mapper, at, modifiers.shift),
                (None, None) => {}
            }
        }
        if mesh.is_some() {
            mesh_overlay::grid_options(ui.ctx(), rect, tool);
            proportional_overlay::options(ui.ctx(), rect, &mut view.proportional);
            tool.proportional = view.proportional;
        }
        // The right-click menu: a vertex or edge under the pointer is selected
        // first, so the menu acts on it. Right click is not used for panning.
        if let Some((_, m)) = &mesh {
            if response.secondary_clicked()
                && let Some(at) = arrow(response.interact_pointer_pos())
            {
                tool.context_select(&shown, &m.indices, &mapper, at);
            }
            let ctx = ui.ctx().clone();
            response.context_menu(|ui| {
                let shortcut = |a: &str| keymap.shortcut_text(&ctx, a);
                if let Some(action) =
                    mesh_overlay::menu_ui(ui, &MENU_ACTIONS, &|a| tool.menu_enabled(a), &shortcut)
                {
                    actions.push(action);
                    ui.close();
                }
            });
        }
        match &mesh {
            Some((_, m)) => {
                // The scene already shows the drag live, so `shown` is moved; the
                // overlay needs no preview offset of its own.
                mesh_overlay::paint(&painter, &theme, &mapper, &shown, &m.indices, tool, None);
                if tool.proportional.on {
                    let screen: Vec<egui::Pos2> = shown
                        .iter()
                        .map(|&p| {
                            let s = mapper.to_screen(p);
                            pos2(s[0], s[1])
                        })
                        .collect();
                    let moved: Vec<egui::Pos2> = tool
                        .vertices()
                        .iter()
                        .filter_map(|&v| screen.get(v as usize).copied())
                        .collect();
                    let weights = tool.drag_weights(&mapper);
                    proportional_overlay::paint(
                        &painter,
                        &theme,
                        &tool.proportional,
                        &moved,
                        weights.as_deref().map(|w| (&screen[..], w)),
                    );
                }
                mesh_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "mesh-edit-hint"));
            }
            None => mesh_overlay::caption(&painter, &theme, rect, &tr(ui.ctx(), "mesh-edit-pick")),
        }
        (ops, actions)
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
    fn navigate(
        &mut self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        view: &mut ViewState,
        radius_wheel: bool,
    ) {
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
        if radius_wheel {
            let wheel = ui.input(|i| i.smooth_scroll_delta.y);
            if wheel != 0.0 {
                view.proportional.scrolled(wheel);
                self.wake();
            }
        } else if response.hovered() {
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
