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
use tsukumoe_ui::{Arg, Keymap, Theme, hint, tr, tr_args};

use crate::actions::Tool;
use crate::bone_tool::{self, BoneTool, Mode};
use crate::deformer_tool::{self, DeformerTool};
use crate::drag::DragAssignment;
use crate::mesh_tool::{self, Hit, MENU_ACTIONS, Mapper, MeshTool, Op, ToolError};
use crate::panels::{
    bone_overlay, deformer_overlay, mesh_overlay, overlay, proportional_overlay, selection_overlay,
    toolbar, tree,
};
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
    /// The meshes the scene was built to show alone (see `ViewState::solo`).
    solo: Option<Vec<String>>,
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
    /// What the select tool's right-click menu asked of the tree; the editor sends it.
    pub menu_outs: Vec<tree::Out>,
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
            solo: None,
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
            menu_outs: Vec::new(),
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
        bone: &mut BoneTool,
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
        let solo = view.solo_meshes(doc.dispatcher.model());
        if self.scene.is_none() || self.model_revision != model_rev || self.solo != solo {
            match Scene::new_solo(doc.dispatcher.model(), solo.as_deref()) {
                Ok(scene) => {
                    self.scene = Some(scene);
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
            self.model_revision = model_rev;
            self.solo = solo;
            self.view_revision = u64::MAX;
        }
        if self.view_revision != view.revision() {
            if let Some(scene) = &mut self.scene {
                scene.apply(view);
            }
            self.view_revision = view.revision();
            self.wake();
        }
        // Turned to the fixed radius: take its size from the view as it is now.
        if view.proportional.fixed
            && view.proportional.model_radius.is_none()
            && let Some(scene) = &self.scene
        {
            let rect = ui.available_rect_before_wrap();
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            view.proportional.settle(&mapper);
        }
        mesh.proportional = view.proportional;
        deformer.proportional = view.proportional;
        // The wheel sets the influence radius while proportional edit drags.
        let radius_wheel = view.proportional.on
            && match view.tool {
                Tool::Mesh => mesh.is_moving(),
                Tool::Deformer => deformer.is_moving_points(),
                Tool::Select | Tool::Bone => false,
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
        paint_background(
            &ui.painter_at(rect),
            rect,
            crate::options::tuning().background,
        );
        if let Some(target) = &self.target {
            ui.painter().image(
                target.id,
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
        }
        if self.solo.is_some() {
            paint_solo_badge(ui, rect, view);
        }
        let (ops, mut actions) = match view.tool {
            Tool::Mesh => {
                self.mesh_tool_frame(ui, &response, rect, doc, view, drag, mesh, size, keymap)
            }
            Tool::Deformer => self
                .deformer_tool_frame(ui, &response, rect, doc, view, drag, deformer, size, keymap),
            Tool::Bone => self.bone_tool_frame(ui, &response, rect, doc, view, drag, bone, size),
            Tool::Select => {
                self.select_tool_frame(ui, &response, rect, view, drag, size);
                self.select_lasso(ui, &response, rect, view, drag, size);
                let mut actions = Vec::new();
                self.select_menu(&response, rect, doc, view, size, &mut actions);
                (Vec::new(), actions)
            }
        };
        if let Some(action) = toolbar::show(ui.ctx(), rect, view.tool, view.proportional.on, keymap)
        {
            actions.push(action);
        }
        // What the pointer rests on, in the viewport or in a tree row.
        if selection_overlay::shown_with(view.tool)
            && let Some(scene) = &self.scene
            && let Some(hover) = view.hover(ui.ctx())
        {
            let names = selection_overlay::hover_meshes(doc.dispatcher.model(), hover);
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            let painter = ui.painter_at(rect);
            let theme = Theme::get(ui.ctx());
            selection_overlay::paint_hover(&painter, &theme, &mapper, scene, &names);
            if matches!(hover, Selection::Part(_)) {
                selection_overlay::paint_hover_box(&painter, &theme, &mapper, scene, &names);
            }
            if let Some(Selection::Mesh(name)) = view.hover_card(ui.ctx()) {
                selection_overlay::show_card(ui.ctx(), doc.dispatcher.model(), &view.atlas, name);
            }
        }
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
        // The node lines of a selected glue, with any tool.
        if let Some(scene) = &self.scene {
            let segments = selection_overlay::glue_segments(scene, doc.dispatcher.model(), view);
            if !segments.is_empty() {
                let mapper = Mapper {
                    origin: [rect.min.x, rect.min.y],
                    size: [rect.width(), rect.height()],
                    view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
                };
                let theme = Theme::get(ui.ctx());
                selection_overlay::paint_glues(&ui.painter_at(rect), &theme, &mapper, &segments);
            }
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
        let at = crate::keyed::values(model, view);
        let lift = |previews: Vec<Preview>| -> Vec<Preview> {
            previews
                .into_iter()
                .map(|p| crate::keyed::lift_preview(model, &at, p))
                .collect()
        };
        match view.tool {
            Tool::Mesh => (|| {
                let name = mesh.mesh()?;
                let m = model.meshes().get(model.meshes().find(name)?)?;
                let space = Space::of_at(model, m.parent, &at).ok()?;
                let stored = crate::keyed::mesh_stored(model, name, &at)?;
                let positions = mesh.dragging_stored(&stored, &mapper, &space)?;
                Some(lift(vec![Preview::Mesh {
                    name: name.to_owned(),
                    positions,
                }]))
            })()
            .unwrap_or_default(),
            Tool::Deformer => (|| {
                let name = deformer.deformer()?;
                let radius = (mapper.view[2] - mapper.view[0]) * 0.05;
                let shape = deformer_tool::Shape::of_at(model, name, radius, &at)?;
                // Dragging a rotation's origin moves the axis and corrects what hangs
                // from it: preview all of it, exactly as it would be committed.
                if let Some(axis) = deformer.axis_drag(name, &shape, &mapper, model) {
                    return Some(match axis {
                        Ok((command, _)) => lift(crate::deformer_ops::previews_of(&command)),
                        Err(_) => Vec::new(),
                    });
                }
                Some(lift(vec![
                    deformer.dragging(&shape, &mapper)?.as_preview(name),
                ]))
            })()
            .unwrap_or_default(),
            Tool::Select | Tool::Bone => Vec::new(),
        }
    }

    /// The select tool's right-click menu: the mesh under the pointer is
    /// selected first (unless it already is), then the row menu of the tree
    /// acts on it, with the deformer makers below.
    fn select_menu(
        &mut self,
        response: &egui::Response,
        rect: Rect,
        doc: &Document,
        view: &mut ViewState,
        size: (u32, u32),
        actions: &mut Vec<&'static str>,
    ) {
        if response.secondary_clicked()
            && let Some(scene) = &self.scene
            && let Some(at) = response.interact_pointer_pos()
        {
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            if let Some(name) = mesh_tool::pick_drawn(
                &scene.frame.draw_list(&scene.model),
                mapper.to_model([at.x, at.y]),
            ) {
                let item = Selection::Mesh(name);
                if !view.is_selected(&item) {
                    view.select_only(item);
                }
            }
        }
        let model = doc.dispatcher.model();
        let panel = crate::tree_model::Panel::Parts;
        let Some(item) = view.selection.clone() else {
            // Nothing to act on: no menu (an empty menu would still pop up).
            response.context_menu(|ui| ui.close());
            return;
        };
        let rows = crate::tree_model::rows(model, view.tree(panel), panel);
        let Some(row) = rows.iter().find(|r| r.selection() == item) else {
            response.context_menu(|ui| ui.close());
            return;
        };
        let lists = tree::Lists::of(model);
        let ctx = response.ctx.clone();
        response.context_menu(|ui| {
            tree::row_menu(ui, model, view, panel, row, &lists, &mut self.menu_outs);
            ui.separator();
            for (key, action) in [
                ("deformer-make-warp", "deformer.create-warp"),
                ("deformer-make-rotation", "deformer.create-rotation"),
            ] {
                if ui.button(tr(&ctx, key)).clicked() {
                    actions.push(action);
                    ui.close();
                }
            }
        });
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
        let Some(scene) = &self.scene else { return };
        if let Some(at) = response.hover_pos() {
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            if let Some(name) = mesh_tool::pick_drawn(
                &scene.frame.draw_list(&scene.model),
                mapper.to_model([at.x, at.y]),
            ) {
                view.set_hover(ui.ctx(), Selection::Mesh(name));
            }
        }
        if !response.clicked_by(PointerButton::Primary) {
            return;
        }
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

    /// The select tool's lasso: a drag on the viewport draws a curve, and the
    /// meshes whose centre it encloses become the selection (Shift adds).
    fn select_lasso(
        &self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        view: &mut ViewState,
        drag: &DragAssignment,
        size: (u32, u32),
    ) {
        let Some(scene) = &self.scene else {
            return;
        };
        let modifiers = ui.input(|i| i.modifiers);
        let id = response.id.with("select-lasso");
        let primary = PointerButton::Primary;
        if response.dragged_by(primary)
            && !drag.modifier_down(modifiers)
            && let Some(p) = response.interact_pointer_pos()
        {
            let path = ui.data_mut(|d| {
                let path = d.get_temp_mut_or_default::<Vec<[f32; 2]>>(id);
                if path
                    .last()
                    .is_none_or(|l| (p.x - l[0]).hypot(p.y - l[1]) >= 2.0)
                {
                    path.push([p.x, p.y]);
                }
                path.clone()
            });
            let theme = Theme::get(ui.ctx());
            let painter = ui.painter_at(rect);
            let points: Vec<egui::Pos2> = path.iter().map(|q| pos2(q[0], q[1])).collect();
            painter.add(egui::Shape::convex_polygon(
                points.clone(),
                theme.colors.accent.gamma_multiply(0.15),
                egui::Stroke::NONE,
            ));
            painter.add(egui::Shape::closed_line(
                points,
                egui::Stroke::new(1.0, theme.colors.accent),
            ));
        }
        if response.drag_stopped_by(primary) {
            let path = ui.data_mut(|d| d.remove_temp::<Vec<[f32; 2]>>(id));
            let Some(path) = path.filter(|p| p.len() > 2) else {
                return;
            };
            let mapper = Mapper {
                origin: [rect.min.x, rect.min.y],
                size: [rect.width(), rect.height()],
                view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
            };
            let inside =
                mesh_tool::pick_in_lasso(&scene.frame.draw_list(&scene.model), &mapper, &path);
            let mut items: Vec<Selection> = if modifiers.shift {
                view.selected()
            } else {
                Vec::new()
            };
            for name in inside {
                let s = Selection::Mesh(name);
                if !items.contains(&s) {
                    items.push(s);
                }
            }
            view.select_all(items);
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
        // The parameter values the picture shows: shapes are taken at them.
        let param_at = crate::keyed::values(model, view);
        let shape = name
            .as_deref()
            .and_then(|n| deformer_tool::Shape::of_at(model, n, radius, &param_at));
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
                _ => match deformer_tool::pick_deformer_at(model, &mapper, at, radius, &param_at) {
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
                let related = deformer_tool::related_at(model, name, radius, &param_at);
                let preview = tool.preview(shape, &mapper);
                deformer_overlay::paint(
                    &painter,
                    &theme,
                    &mapper,
                    preview.as_ref().unwrap_or(shape),
                    &related,
                    tool,
                );
                overlay::help(
                    ui.ctx(),
                    rect,
                    &with_mod(ui.ctx(), "deformer-hint"),
                    crate::keyed::deformer_place(model, name, &|id| param_at(id)).as_ref(),
                );
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
                        tool.proportional.screen_radius(&mapper),
                        &moved,
                        weights.as_deref().map(|w| (&screen[..], w)),
                    );
                }
            }
            _ => overlay::help(ui.ctx(), rect, &tr(ui.ctx(), "deformer-pick"), None),
        }
        overlay::flyout(ui.ctx(), rect, |ui| {
            actions.extend(deformer_overlay::options(ui, tool));
            proportional_overlay::options(ui, &mut view.proportional);
        });
        tool.proportional = view.proportional;
        (ops, actions)
    }

    /// The bone tool for one frame: pointer input, then the overlay.
    #[allow(clippy::too_many_arguments)]
    fn bone_tool_frame(
        &self,
        ui: &Ui,
        response: &egui::Response,
        rect: Rect,
        doc: &Document,
        view: &mut ViewState,
        drag: &DragAssignment,
        tool: &mut BoneTool,
        size: (u32, u32),
    ) -> (Vec<Result<Op, ToolError>>, Vec<&'static str>) {
        let mut ops = Vec::new();
        let Some(scene) = &self.scene else {
            return (ops, Vec::new());
        };
        let theme = Theme::get(ui.ctx());
        let painter = ui.painter_at(rect);
        let mapper = Mapper {
            origin: [rect.min.x, rect.min.y],
            size: [rect.width(), rect.height()],
            view: view.camera.view(fit_view(scene.bounds(), size.0, size.1)),
        };
        let model = doc.dispatcher.model();
        tool.follow(model, view.last_mesh(), view.last_bone());
        let at = crate::keyed::values(model, view);
        let space = bone_tool::space_of(model, tool.mesh.as_deref(), &at);
        let joints = bone_tool::joints(model, &space, &at);
        let (modifiers, escape) = ui.input(|i| (i.modifiers, i.key_pressed(egui::Key::Escape)));
        let arrow = |p: Option<egui::Pos2>| p.map(|p| [p.x, p.y]);
        let pressed = arrow(ui.input(|i| i.pointer.press_origin()));
        let now = arrow(response.interact_pointer_pos());
        let primary = PointerButton::Primary;
        let tool_input = !drag.modifier_down(modifiers);
        if escape {
            tool.cancel();
        }
        let clicked = tool_input && response.clicked_by(primary);
        let picked = now.and_then(|p| bone_tool::pick_joint(&joints, &mapper, p));
        let target = tool.mesh.clone().zip(tool.bone.clone());
        let mut stroke_vertices = None;
        match tool.mode {
            Mode::Bones => {
                if tool_input
                    && response.drag_started_by(primary)
                    && let Some(at) = pressed
                {
                    tool.press_joint(&joints, &mapper, at);
                }
                if response.dragged_by(primary)
                    && let Some(at) = now
                {
                    tool.drag_to(at);
                }
                if response.drag_stopped_by(primary)
                    && let Some(made) = tool.release_joint(model, &space, &mapper, &at)
                {
                    ops.push(made);
                }
                if clicked && let Some(p) = now {
                    match picked {
                        Some(name) => view.select_only(Selection::Bone(name)),
                        None => match BoneTool::add_command(
                            model,
                            &space,
                            &mapper,
                            p,
                            tool.bone.as_deref(),
                            &at,
                        ) {
                            Ok((name, op)) => {
                                view.select_only(Selection::Bone(name));
                                ops.push(Ok(op));
                            }
                            Err(e) => ops.push(Err(e)),
                        },
                    }
                }
            }
            Mode::Weights => {
                if clicked && let Some(name) = picked.clone() {
                    view.select_only(Selection::Bone(name));
                } else if let Some((mesh, bone)) = &target {
                    let on_screen: Vec<[f32; 2]> = scene
                        .drawn_positions(mesh)
                        .unwrap_or_default()
                        .into_iter()
                        .map(|p| mapper.to_screen(p))
                        .collect();
                    let painting = tool_input
                        && (response.drag_started_by(primary)
                            || response.dragged_by(primary)
                            || response.clicked_by(primary));
                    if painting && let Some(p) = now {
                        tool.stamp(model, mesh, bone, &on_screen, p);
                    }
                    if (response.drag_stopped_by(primary) || response.clicked_by(primary))
                        && let Some(op) = tool.release_stroke()
                    {
                        ops.push(Ok(op));
                    }
                    stroke_vertices = Some(on_screen);
                }
            }
        }
        if tool.mode == Mode::Weights
            && let Some((mesh, bone)) = &target
        {
            {
                let painted = tool
                    .stroke()
                    .filter(|(m, _)| m == mesh)
                    .map(|(_, w)| w.clone());
                let weights = painted.or_else(|| bone_tool::weights_of(model, mesh));
                if let (Some(weights), Some(vertices)) = (weights, scene.drawn_positions(mesh)) {
                    let edges = model
                        .meshes()
                        .find(mesh)
                        .and_then(|id| model.meshes().get(id))
                        .map(|m| {
                            m.indices
                                .as_chunks::<3>()
                                .0
                                .iter()
                                .flat_map(|t| [[t[0], t[1]], [t[1], t[2]], [t[2], t[0]]])
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    bone_overlay::paint_weights(
                        &painter,
                        &theme,
                        &mapper,
                        &vertices,
                        &edges,
                        &bone_tool::heat(&weights, bone),
                    );
                }
                if let (Some(hover), true) =
                    (ui.input(|i| i.pointer.hover_pos()), response.hovered())
                    && stroke_vertices.is_some()
                {
                    bone_overlay::paint_brush(&painter, &theme, hover, tool.brush.radius);
                }
            }
        }
        bone_overlay::paint_bones(
            &painter,
            &theme,
            &mapper,
            &joints,
            tool.bone.as_deref(),
            tool.dragged_joint(),
        );
        let status = format!(
            "{} / {}",
            tool.mesh.as_deref().unwrap_or("\u{2014}"),
            tool.bone.as_deref().unwrap_or("\u{2014}")
        );
        overlay::status(ui.ctx(), rect, &status);
        overlay::flyout(ui.ctx(), rect, |ui| bone_overlay::options(ui, tool));
        (ops, Vec::new())
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
        let at = crate::keyed::values(model, view);
        let space = mesh
            .as_ref()
            .and_then(|(_, m)| Space::of_at(model, m.parent, &at).ok())
            .unwrap_or(Space::Drawing);
        // What a drag changes: the keyform at the key the sliders are on, else the base shape.
        let stored: Vec<[f32; 2]> = mesh
            .as_ref()
            .and_then(|(n, _)| crate::keyed::mesh_stored(model, n, &at))
            .unwrap_or_default();
        let place = tool
            .mesh()
            .and_then(|n| crate::keyed::mesh_place(model, n, &at));
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
            tool.center = crate::mirror::center(model, &view.mirror_center);
            if escape {
                tool.cancel();
            }
            if tool.pen {
                // The pen draws from the press vertex to the release point.
                if tool_input
                    && response.drag_started_by(primary)
                    && let Some(at) = pressed
                {
                    tool.pen_press(&shown, &mesh.indices, &mapper, at);
                }
                if tool_input
                    && response.drag_stopped_by(primary)
                    && let Some(at) = arrow(ui.input(|i| i.pointer.latest_pos()))
                    && let Some(g) = mesh_tool::geometry_of(model, name)
                {
                    match tool.pen_click(name, &g, &shown, &mapper, at, &space) {
                        Ok(Some(op)) => ops.push(Ok(op)),
                        Ok(None) => {}
                        Err(e) => ops.push(Err(e)),
                    }
                }
                if response.secondary_clicked() {
                    tool.pen_end();
                }
            } else {
                if tool_input
                    && response.drag_started_by(primary)
                    && let Some(at) = pressed
                {
                    tool.lasso = modifiers.command;
                    tool.press(&shown, &mesh.indices, &mapper, at, modifiers.shift);
                }
                if response.dragged_by(primary)
                    && let Some(at) = now
                {
                    tool.drag(at);
                }
                if response.drag_stopped_by(primary)
                    && let Some(op) = tool.release_in(name, &stored, &shown, &mapper, &space)
                {
                    ops.push(Ok(op));
                }
            }
        }
        if tool_input
            && response.clicked_by(primary)
            && let Some(at) = now
        {
            let positions = &shown[..];
            let indices = mesh.as_ref().map_or(&[][..], |(_, m)| &m.indices[..]);
            let empty = mesh_tool::hit_test(positions, indices, &mapper, at) == Hit::Nothing;
            let picked = (empty && !modifiers.shift && !modifiers.command && !tool.pen)
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
                (Some((name, _)), _) if tool.pen => {
                    if let Some(g) = mesh_tool::geometry_of(model, name) {
                        match tool.pen_click(name, &g, &shown, &mapper, at, &space) {
                            Ok(Some(op)) => ops.push(Ok(op)),
                            Ok(None) => {}
                            Err(e) => ops.push(Err(e)),
                        }
                    }
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
            overlay::flyout(ui.ctx(), rect, |ui| {
                actions.extend(mesh_overlay::grid_options(ui, tool));
                proportional_overlay::options(ui, &mut view.proportional);
            });
            tool.proportional = view.proportional;
        }
        // The right-click menu: a vertex or edge under the pointer is selected
        // first, so the menu acts on it. Right click is not used for panning.
        if let Some((_, m)) = &mesh {
            if response.secondary_clicked()
                && !tool.pen
                && let Some(at) = arrow(response.interact_pointer_pos())
            {
                tool.context_select(&shown, &m.indices, &mapper, at);
            }
            let ctx = ui.ctx().clone();
            response.context_menu(|ui| {
                // The pen ends a chain with a right click instead.
                if tool.pen {
                    ui.close();
                    return;
                }
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
                        tool.proportional.screen_radius(&mapper),
                        &moved,
                        weights.as_deref().map(|w| (&screen[..], w)),
                    );
                }
                if tool.pen {
                    mesh_overlay::pen_preview(
                        &painter,
                        &theme,
                        &mapper,
                        &shown,
                        tool,
                        response.hover_pos(),
                    );
                    ui.ctx().request_repaint();
                }
                let hint = if tool.pen {
                    "mesh-edit-pen-hint"
                } else {
                    "mesh-edit-hint"
                };
                overlay::help(ui.ctx(), rect, &with_mod(ui.ctx(), hint), place.as_ref());
            }
            None => overlay::help(ui.ctx(), rect, &tr(ui.ctx(), "mesh-edit-pick"), None),
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
            if view.grid_live && wheel != 0.0 {
                view.grid_wheel += wheel;
                while view.grid_wheel.abs() >= 40.0 {
                    view.grid_steps.push(view.grid_wheel.signum() as i32);
                    view.grid_wheel -= 40.0 * view.grid_wheel.signum();
                }
                self.wake();
                return;
            }
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

/// A note in the corner that only part of the model is drawn.
fn paint_solo_badge(ui: &Ui, rect: Rect, view: &mut ViewState) {
    let name = match &view.solo {
        Some(
            Selection::Part(n) | Selection::Mesh(n) | Selection::Warp(n) | Selection::Rotation(n),
        ) => n.as_str(),
        _ => return,
    };
    let text = format!("{}: {name}", tr(ui.ctx(), "viewport-solo"));
    if overlay::status_closable(ui.ctx(), rect, &text, &tr(ui.ctx(), "viewport-solo-end")) {
        view.solo = None;
        view.touch();
    }
}

/// What is behind the picture: nothing (the theme's surface), a flat colour, or a checker.
fn paint_background(painter: &egui::Painter, rect: Rect, background: crate::options::Background) {
    use crate::options::Background;
    match background {
        Background::Theme => {}
        Background::White => {
            painter.rect_filled(rect, 0.0, egui::Color32::WHITE);
        }
        Background::Black => {
            painter.rect_filled(rect, 0.0, egui::Color32::BLACK);
        }
        Background::Checker => {
            const TILE: f32 = 16.0;
            painter.rect_filled(rect, 0.0, egui::Color32::from_gray(204));
            let dark = egui::Color32::from_gray(153);
            let (cols, rows) = (
                (rect.width() / TILE).ceil() as i32,
                (rect.height() / TILE).ceil() as i32,
            );
            let tiles: Vec<egui::Shape> = (0..rows)
                .flat_map(|r| (0..cols).map(move |c| (r, c)))
                .filter(|(r, c)| (r + c) % 2 == 0)
                .map(|(r, c)| {
                    let min = rect.min + egui::vec2(c as f32 * TILE, r as f32 * TILE);
                    let tile = Rect::from_min_size(min, egui::vec2(TILE, TILE)).intersect(rect);
                    egui::Shape::rect_filled(tile, 0.0, dark)
                })
                .collect();
            painter.extend(tiles);
        }
    }
}

/// A hint whose `{ $mod }` is the platform's command key as the user sees it
/// (⌘ or Ctrl).
fn with_mod(ctx: &egui::Context, key: &str) -> String {
    let name = ctx.format_modifiers(egui::Modifiers::COMMAND);
    tr_args(ctx, key, &[("mod", Arg::from(name.as_str()))])
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
