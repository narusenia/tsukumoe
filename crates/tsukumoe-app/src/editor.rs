// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's state and its window layout: menus, workspace tabs, the dock
//! with the standard panels, dialogs and the status line. It knows nothing of
//! eframe's window, so a test can drive it with a harness.

use std::path::{Path, PathBuf};

use crate::actions::{self, ACTIONS, Tool};
use crate::deformer_ops::{self, Target};
use crate::deformer_tool::DeformerTool;
use crate::drag::{AxisTarget, DragAssignment, Modifier};
use crate::menu;
use crate::mesh_tool::{self, MeshTool, Op, ToolError};
use crate::panels::palette::Palette;
use crate::panels::{history, inspector, parameters, titlebar, tree, viewport};
use crate::picker::{FilePicker, PickKind, PickRequest};
use crate::pie::{Entry, PieConfig};
use crate::tree_model::{self, TreeView};
use crate::tree_ops;
use eframe::egui_wgpu::RenderState;
use egui::{Context, Ui};
use tsukumoe_ui::dock::{self, PanelHost, TabStyle};
use tsukumoe_ui::{
    Arg, Icon, Keymap, Toasts, Tone, Variant, button, checkbox, hint, label, text_input, tr,
    tr_args,
};

use crate::panels::viewport::Viewport;
use crate::project::{Document, Recent};
use crate::psd_import::{PsdImport, RowKind, Stage};
use crate::settings;
use crate::view::{Selection, ViewState};
use crate::workspace::{Layouts, Preset, Workspace};

const LAYOUTS_FILE: &str = "layouts.json";
const STATE_FILE: &str = "state.json";
const TREE_FILE: &str = "tree.json";
const KEYMAP_FILE: &str = "keymap.json";
const DRAG_FILE: &str = "drag.json";
const PIE_FILE: &str = "pie.json";

pub enum Dialog {
    Open(String),
    SaveAs(String),
    SaveLayout(String),
    ConfirmPreset(Preset),
    /// Deleting these takes `count` elements with it.
    ConfirmDelete(Vec<Selection>, usize),
    /// The shortcut list: the text of each action's field, and the last error.
    Keymap(Vec<String>, Option<String>),
    /// The drag-to-parameter assignment being edited.
    Drag,
    /// The pie menu slots being edited.
    Pie,
    /// File > Import PSD.
    PsdImport(Box<PsdImport>),
    /// A folder for a new project (the path-input form of File > New).
    NewProject(String),
    /// A PSD and a folder for a new project (the path-input form).
    NewFromPsd(String, String),
    /// Where to save as a folder project (the path-input form).
    SaveAsFolder(String),
}

pub struct Editor {
    pub doc: Option<Document>,
    pub workspace: Workspace,
    pub layouts: Layouts,
    pub recent: Recent,
    /// Where the editor's settings are saved; `None` without a home folder.
    pub settings_dir: Option<PathBuf>,
    pub dialog: Option<Dialog>,
    pub toasts: Toasts,
    pub view: ViewState,
    pub palette: Palette,
    pub keymap: Keymap,
    pub drag: DragAssignment,
    /// The tree panels' state of each workspace (the open one is in `view`),
    /// and what was last written to `tree.json`.
    trees: [(TreeView, TreeView); 2],
    trees_saved: String,
    /// How the dock writes its tab headings; saved with the editor's state.
    pub tab_style: TabStyle,
    /// The mesh edit tool's selection and drag; view state, not model.
    pub mesh_tool: MeshTool,
    /// The deformer tool's selection and drag; view state, not model.
    pub deformer_tool: DeformerTool,
    pub pie_config: PieConfig,
    /// Whether the system draws the menus (macOS), leaving only the tabs here.
    pub native_menu: bool,
    /// The open pie menu, while its key is held.
    pie: Option<tsukumoe_ui::PieMenu>,
    /// Whether the pointer was over the viewport last frame (a keymap context).
    viewport_hovered: bool,
    /// The window title last sent to the OS.
    window_title: String,
    viewport: Option<Viewport>,
    layouts_dirty: bool,
    /// How files and folders are chosen; replaced by a scripted one in tests.
    pub picker: Box<dyn FilePicker>,
    /// The folder the last file dialog was used in; where the next one starts.
    pub last_dir: Option<PathBuf>,
    /// Actions that need a file dialog, run by [`Editor::process_pending`]
    /// where the window handle is at hand.
    pending: Vec<String>,
    /// The document that was open when a new project from a PSD was started,
    /// kept until the import is confirmed so that cancelling brings it back.
    stash: Option<Document>,
}

impl Editor {
    /// Loads the saved layouts and state from `settings_dir`; anything missing
    /// or damaged gives the defaults.
    pub fn new(settings_dir: Option<PathBuf>) -> Self {
        let read = |name| {
            settings_dir
                .as_deref()
                .and_then(|d| settings::read(d, name))
        };
        let layouts = read(LAYOUTS_FILE).map_or_else(Layouts::default, |t| Layouts::from_json(&t));
        let state: serde_json::Value = read(STATE_FILE)
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let keymap = read(KEYMAP_FILE)
            .and_then(|t| Keymap::from_json(&t).ok())
            .unwrap_or_else(actions::cubism_compatible);
        let drag =
            read(DRAG_FILE).map_or_else(DragAssignment::default, |t| DragAssignment::from_json(&t));
        let pie_config =
            read(PIE_FILE).map_or_else(PieConfig::default, |t| PieConfig::from_json(&t));
        let tree_file: serde_json::Value = read(TREE_FILE)
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let mut editor = Self {
            doc: None,
            workspace: serde_json::from_value(state["workspace"].clone()).unwrap_or_default(),
            layouts,
            recent: Recent::from_json(&state["recent"]),
            settings_dir,
            dialog: None,
            toasts: Toasts::default(),
            view: ViewState::default(),
            palette: Palette::default(),
            keymap,
            drag,
            trees: Default::default(),
            trees_saved: String::new(),
            tab_style: state["tab_style"]
                .as_str()
                .and_then(TabStyle::from_key)
                .unwrap_or_default(),
            mesh_tool: MeshTool::default(),
            deformer_tool: DeformerTool::default(),
            pie_config,
            native_menu: false,
            pie: None,
            viewport_hovered: false,
            window_title: String::new(),
            viewport: None,
            layouts_dirty: false,
            picker: crate::picker::default_picker(),
            last_dir: state["last_dir"].as_str().map(PathBuf::from),
            pending: Vec::new(),
            stash: None,
        };
        for (i, w) in Workspace::ALL.iter().enumerate() {
            let saved = &tree_file[w.key()];
            editor.trees[i] = (
                TreeView::from_json(&saved["parts"]),
                TreeView::from_json(&saved["deformers"]),
            );
        }
        editor.load_tree_views();
        editor.trees_saved = editor.tree_json();
        editor
    }

    fn workspace_index(&self) -> usize {
        Workspace::ALL
            .iter()
            .position(|w| *w == self.workspace)
            .unwrap_or(0)
    }

    /// Takes the open workspace's tree state into the view.
    fn load_tree_views(&mut self) {
        let (parts, deformers) = self.trees[self.workspace_index()].clone();
        self.view.parts = parts;
        self.view.deformers = deformers;
    }

    /// Puts the view's tree state back into the open workspace's slot.
    fn store_tree_views(&mut self) {
        let i = self.workspace_index();
        self.trees[i] = (self.view.parts.clone(), self.view.deformers.clone());
    }

    /// The tree state of every workspace as the file holds it.
    fn tree_json(&self) -> String {
        let mut file = serde_json::Map::new();
        for (i, w) in Workspace::ALL.iter().enumerate() {
            let (parts, deformers) = &self.trees[i];
            file.insert(
                w.key().to_owned(),
                serde_json::json!({"parts": parts.to_json(), "deformers": deformers.to_json()}),
            );
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(file)).unwrap_or_default()
    }

    /// Writes the tree state when it changed (not while a mouse button is down).
    fn save_trees(&mut self, ctx: &Context) {
        if let Some(doc) = &self.doc {
            let model = doc.dispatcher.model();
            self.view.parts.prune(model, tree_model::Panel::Parts);
            self.view
                .deformers
                .prune(model, tree_model::Panel::Deformers);
        }
        self.store_tree_views();
        let json = self.tree_json();
        if json == self.trees_saved || ctx.input(|i| i.pointer.any_down()) {
            return;
        }
        if let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, TREE_FILE, &json)
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
        self.trees_saved = json;
    }

    fn persist_state(&self) -> Result<(), String> {
        let Some(dir) = &self.settings_dir else {
            return Ok(());
        };
        let state = serde_json::json!({
            "workspace": self.workspace,
            "recent": self.recent.to_json(),
            "last_dir": self.last_dir,
            "tab_style": self.tab_style.key(),
        });
        settings::write(
            dir,
            STATE_FILE,
            &serde_json::to_string_pretty(&state).unwrap_or_default(),
        )
    }

    fn persist_layouts(&mut self) -> Result<(), String> {
        self.layouts_dirty = false;
        match &self.settings_dir {
            Some(dir) => settings::write(dir, LAYOUTS_FILE, &self.layouts.to_json()),
            None => Ok(()),
        }
    }

    pub fn open(&mut self, path: &Path) -> Result<(), String> {
        match Document::open(path) {
            Ok(doc) => {
                self.doc = Some(doc);
                self.store_tree_views();
                self.view = ViewState::default();
                self.load_tree_views();
                self.mesh_tool = MeshTool::default();
                self.deformer_tool = DeformerTool::default();
                self.recent.push(path);
                self.persist_state()
            }
            Err(e) => {
                self.recent.remove(path);
                let _ = self.persist_state();
                Err(e)
            }
        }
    }

    pub fn save(&mut self) -> Result<(), String> {
        self.doc.as_mut().map_or(Ok(()), Document::save)
    }

    pub fn save_as(&mut self, path: &Path) -> Result<(), String> {
        let Some(doc) = self.doc.as_mut() else {
            return Ok(());
        };
        doc.save_to(path)?;
        self.recent.push(path);
        self.persist_state()
    }

    /// The single entry for UI edits: sends `command` to the dispatcher and
    /// shows a failure as an error toast (the model stays as it was).
    pub fn submit(&mut self, ctx: &Context, command: tsukumoe_core::command::Command) -> bool {
        self.submit_labeled(ctx, command, None)
    }

    /// [`Self::submit`] with a history name for the command.
    pub fn submit_labeled(
        &mut self,
        ctx: &Context,
        command: tsukumoe_core::command::Command,
        label: Option<String>,
    ) -> bool {
        let Some(doc) = self.doc.as_mut() else {
            return false;
        };
        match doc.dispatcher.submit_labeled(command, label) {
            Ok(()) => true,
            Err(e) => {
                self.toasts.push(ctx, Tone::Error, e);
                false
            }
        }
    }

    /// Sends the result of a mesh tool operation, or shows why it failed.
    pub fn apply_mesh_op(&mut self, ctx: &Context, op: Result<Op, ToolError>) {
        match op {
            Ok((command, label)) => {
                self.submit_labeled(ctx, command, Some(label));
            }
            Err(e) => self.toasts.push(ctx, Tone::Error, tr(ctx, e.key())),
        }
    }

    /// The elements the selection names, for making a deformer around them: a
    /// mesh, every mesh under a part, or a deformer.
    fn deformer_targets(&self) -> Vec<Target> {
        let Some(doc) = &self.doc else {
            return Vec::new();
        };
        let model = doc.dispatcher.model();
        let mut out: Vec<Target> = Vec::new();
        for item in self.view.selected() {
            let found = match item {
                Selection::Mesh(n) => vec![Target::Mesh(n)],
                Selection::Warp(n) | Selection::Rotation(n) => vec![Target::Deformer(n)],
                Selection::Part(n) => tree_model::meshes_under_part(model, &n)
                    .into_iter()
                    .map(Target::Mesh)
                    .collect(),
                Selection::Parameter(_) => Vec::new(),
            };
            for t in found {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        out
    }

    /// Makes a warp or a rotation deformer around the selection and selects it.
    fn run_deformer_action(&mut self, ctx: &Context, action: &str) {
        let targets = self.deformer_targets();
        let Some(doc) = &self.doc else {
            return;
        };
        let model = doc.dispatcher.model();
        let warp = action == "deformer.create-warp";
        let result = if warp {
            let (rows, columns) = self.deformer_tool.grid;
            deformer_ops::create_warp(model, &targets, rows, columns)
        } else {
            deformer_ops::create_rotation(model, &targets)
        };
        match result {
            Ok((command, label)) => {
                let name = label.rsplit(": ").next().unwrap_or_default().to_owned();
                if self.submit_labeled(ctx, command, Some(label)) {
                    self.view.select_only(if warp {
                        Selection::Warp(name)
                    } else {
                        Selection::Rotation(name)
                    });
                    self.view.tool = Tool::Deformer;
                    self.view.touch();
                }
            }
            Err(e) => self.toasts.push(ctx, Tone::Error, tr(ctx, e.key())),
        }
    }

    /// What a tree panel asked for, sent through the one door.
    pub fn apply_tree_out(&mut self, ctx: &Context, out: tree::Out) {
        match out {
            tree::Out::Command(command) => {
                self.submit(ctx, command);
            }
            tree::Out::Reparent(target, parent) => {
                let made = self.doc.as_ref().map(|d| {
                    deformer_ops::set_parent(d.dispatcher.model(), &target, parent.as_deref())
                });
                match made {
                    Some(Ok((command, label))) => {
                        self.submit_labeled(ctx, command, Some(label));
                    }
                    Some(Err(e)) => self.toasts.push(ctx, Tone::Error, tr(ctx, e.key())),
                    None => {}
                }
            }
            tree::Out::Rename { kind, from, to } => {
                let command = tree_ops::rename_command(kind, &from, &to);
                let old = tree_ops::renamed(kind, &from);
                let was_selected = self.view.is_selected(&old);
                let renamed =
                    self.submit_labeled(ctx, command, Some(format!("tree_rename: {from} -> {to}")));
                if renamed && was_selected {
                    self.view.select_only(tree_ops::renamed(kind, &to));
                }
                // Anything outside the editor that names it (the runtime API, a .tmod) is
                // left as it was: say so, once.
                if renamed {
                    let text = tr(ctx, "tree-rename-external");
                    self.toasts.push(ctx, Tone::Neutral, text);
                }
            }
            tree::Out::Reset(items) => self.reset_deformers(ctx, &items),
            tree::Out::Delete(items) => {
                let Some(doc) = &self.doc else { return };
                let plan = tree_ops::delete_plan(doc.dispatcher.model(), &items);
                if plan.commands.is_empty() {
                    return;
                }
                if plan.needs_confirm {
                    self.dialog = Some(Dialog::ConfirmDelete(items, plan.count));
                } else {
                    self.delete_items(ctx, &items);
                }
            }
            tree::Out::Duplicate(items) => {
                let Some(doc) = &self.doc else { return };
                match tree_ops::duplicate_commands(doc.dispatcher.model(), &items) {
                    Ok((commands, made)) if !commands.is_empty() => {
                        let label = match made.as_slice() {
                            [one] => format!("tree_duplicate: {}", selection_name(one)),
                            many => format!("tree_duplicate: {} items", many.len()),
                        };
                        if self.submit_labeled(
                            ctx,
                            tsukumoe_core::command::Command::Batch { commands },
                            Some(label),
                        ) {
                            self.view.select_all(made);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        let text = format!("{} ({e})", tr(ctx, e.key()));
                        self.toasts.push(ctx, Tone::Error, text);
                    }
                }
            }
            tree::Out::SetPart(item, part) => {
                if let Some(command) = tree_ops::set_part_command(&item, part.as_deref()) {
                    let label = format!("tree_set_part: {}", selection_name(&item));
                    self.submit_labeled(ctx, command, Some(label));
                }
            }
        }
    }

    /// Moves the origin of the rotation `name` and keeps what hangs from it where it is drawn.
    pub fn move_axis(&mut self, ctx: &Context, name: &str, origin: [f32; 2]) {
        let Some(doc) = &self.doc else { return };
        match deformer_ops::move_axis(doc.dispatcher.model(), name, origin) {
            Ok((command, label)) => {
                self.submit_labeled(ctx, command, Some(label));
            }
            Err(e) => {
                let text = format!("{} ({e})", tr(ctx, e.key()));
                self.toasts.push(ctx, Tone::Error, text);
            }
        }
    }

    /// Puts the base shape of the deformers in `items` back to its initial
    /// state, as one undo step.
    pub fn reset_deformers(&mut self, ctx: &Context, items: &[Selection]) {
        let Some(doc) = &self.doc else { return };
        match deformer_ops::reset(doc.dispatcher.model(), items) {
            Ok((command, label)) => {
                self.submit_labeled(ctx, command, Some(label));
            }
            Err(e) => self.toasts.push(ctx, Tone::Error, tr(ctx, e.key())),
        }
    }

    /// Deletes `items` (a part with what is under it) as one undo step.
    fn delete_items(&mut self, ctx: &Context, items: &[Selection]) {
        let Some(doc) = &self.doc else { return };
        let plan = tree_ops::delete_plan(doc.dispatcher.model(), items);
        if plan.commands.is_empty() {
            return;
        }
        let label = match items {
            [one] => format!("tree_delete: {}", selection_name(one)),
            many => format!("tree_delete: {} items", many.len()),
        };
        if self.submit_labeled(
            ctx,
            tsukumoe_core::command::Command::Batch {
                commands: plan.commands,
            },
            Some(label),
        ) {
            self.view.clear_selection();
        }
    }

    /// A mesh tool key (delete, connect, flip) on the mesh being edited.
    fn run_mesh_action(&mut self, ctx: &Context, action: &str) {
        // D also switches to the deformer tool; keep that outside the mesh tool.
        // G is bound to nothing else.
        if action == "mesh.retriangulate" && self.view.tool != Tool::Mesh {
            self.view.tool = Tool::Deformer;
            return;
        }
        let Some(name) = self.mesh_tool.mesh().map(str::to_owned) else {
            return;
        };
        let Some(g) = self
            .doc
            .as_ref()
            .and_then(|d| mesh_tool::geometry_of(d.dispatcher.model(), &name))
        else {
            return;
        };
        if self.view.tool != Tool::Mesh {
            return;
        }
        let op = match action {
            "mesh.delete-vertices" => self.mesh_tool.delete_selected(&name, &g),
            "mesh.connect" => self.mesh_tool.connect_selected(&name, &g),
            "mesh.retriangulate" => self.mesh_tool.retriangulate(&name, &g),
            "mesh.fill-grid" => self.mesh_tool.fill_grid(&name, &g),
            "mesh.contour" => {
                let alpha = self.doc.as_ref().and_then(|d| {
                    let model = d.dispatcher.model();
                    let texture = model.meshes().get(model.meshes().find(&name)?)?.texture;
                    let (width, height, data) = d.texture_alpha(texture).ok()?;
                    Some(mesh_tool::AlphaImage {
                        width,
                        height,
                        data,
                    })
                });
                self.mesh_tool.contour_mesh(&name, &g, alpha.as_ref())
            }
            _ => self.mesh_tool.flip_selected(&name, &g),
        };
        self.apply_mesh_op(ctx, op);
    }

    fn save_keymap(&mut self, ctx: &Context) {
        if let Some(dir) = &self.settings_dir
            && let Err(e) = settings::write(dir, KEYMAP_FILE, &self.keymap.to_json(ctx))
        {
            self.toasts.push(ctx, Tone::Error, e);
        }
    }

    /// Runs an action by name (a key press, a menu item, a pie menu item) and
    /// says whether it knows the name. Unknown names, e.g. from a hand-edited
    /// keymap, do nothing. `file.open-recent:<path>` and `layout.load:<name>`
    /// carry their argument after the colon.
    pub fn run_action(&mut self, ctx: &Context, action: &str) -> bool {
        match action {
            "file.open" | "file.open-folder" | "file.new" | "file.new-from-psd"
                if self.picker.is_native() =>
            {
                self.pending.push(action.to_owned());
            }
            "file.save-as" | "file.save-as-folder" | "file.import-psd"
                if self.picker.is_native() =>
            {
                if self.doc.is_some() {
                    self.pending.push(action.to_owned());
                }
            }
            "file.open" | "file.open-folder" => self.dialog = Some(Dialog::Open(String::new())),
            "file.new" => self.dialog = Some(Dialog::NewProject(String::new())),
            "file.new-from-psd" => {
                self.dialog = Some(Dialog::NewFromPsd(String::new(), String::new()));
            }
            "file.save-as" => {
                if self.doc.is_some() {
                    self.dialog = Some(Dialog::SaveAs(String::new()));
                }
            }
            "file.save-as-folder" => {
                if self.doc.is_some() {
                    self.dialog = Some(Dialog::SaveAsFolder(String::new()));
                }
            }
            "file.save" => {
                let r = self.save();
                self.report(ctx, r, Some(tr(ctx, "app-saved")));
            }
            "file.import-psd" => {
                if self.doc.is_some() {
                    self.dialog = Some(Dialog::PsdImport(Box::default()));
                }
            }
            "edit.undo" => drop(self.undo()),
            "edit.redo" => drop(self.redo()),
            "view.palette" => self.open_palette(),
            "view.pie-settings" => self.dialog = Some(Dialog::Pie),
            "view.drag-settings" => self.dialog = Some(Dialog::Drag),
            "view.keymap-settings" => {
                let texts = ACTIONS
                    .iter()
                    .map(|a| {
                        self.keymap
                            .keys_text(a, actions::context_of(a))
                            .unwrap_or_default()
                    })
                    .collect();
                self.dialog = Some(Dialog::Keymap(texts, None));
            }
            "layout.save" => self.dialog = Some(Dialog::SaveLayout(String::new())),
            // Holding the key repeats the press; reopening would move the menu
            // to the pointer every time.
            "view.pie-menu" => {
                if self.pie.is_none() {
                    let at = ctx.pointer_latest_pos().unwrap_or_default();
                    self.pie = Some(tsukumoe_ui::PieMenu::open(at, self.pie_config.items(ctx)));
                }
            }
            "mesh.delete-vertices"
            | "mesh.connect"
            | "mesh.flip-edge"
            | "mesh.retriangulate"
            | "mesh.fill-grid"
            | "mesh.contour" => {
                self.run_mesh_action(ctx, action);
            }
            "deformer.create-warp" | "deformer.create-rotation" => {
                self.run_deformer_action(ctx, action);
            }
            "proportional.toggle" => {
                self.view.proportional.on = !self.view.proportional.on;
            }
            "deformer.reset" => {
                let items = self.view.selected();
                self.reset_deformers(ctx, &items);
            }
            "viewport.zoom-in" | "viewport.zoom-out" => {
                let f = if action.ends_with("in") { 1.25 } else { 0.8 };
                self.view.camera.zoom_at(f, [0.5, 0.5]);
                self.view.touch();
            }
            "viewport.reset-camera" => {
                self.view.camera.reset();
                self.view.touch();
            }
            other => {
                if let Some(id) = other
                    .strip_prefix("panel.show.")
                    .filter(|id| crate::workspace::PANELS.contains(id))
                {
                    self.layouts.current_mut(self.workspace).show_panel(id);
                    self.layouts_dirty = true;
                } else if let Some(style) = other
                    .strip_prefix("view.tab-style.")
                    .and_then(TabStyle::from_key)
                {
                    self.tab_style = style;
                    let _ = self.persist_state();
                } else if let Some(tool) = Tool::from_action(other) {
                    self.view.tool = tool;
                } else if let Some(path) = other.strip_prefix("file.open-recent:") {
                    let path = PathBuf::from(path);
                    let r = self.open(&path);
                    let name = path.display().to_string();
                    let ok = tr_args(ctx, "app-opened", &[("name", Arg::from(name.as_str()))]);
                    self.report(ctx, r, Some(ok));
                } else if let Some(name) = other.strip_prefix("layout.load:") {
                    if self.layouts.load_named(self.workspace, name) {
                        self.layouts_dirty = true;
                    }
                } else if let Some(preset) = other
                    .strip_prefix("layout.preset.")
                    .and_then(|k| Preset::ALL.into_iter().find(|p| p.key() == k))
                {
                    self.dialog = Some(Dialog::ConfirmPreset(preset));
                } else {
                    return false;
                }
            }
        }
        true
    }

    /// Runs the actions that wait for a file dialog. Call it once a frame
    /// where the window (`parent`) is at hand; the dialog blocks until closed.
    pub fn process_pending(&mut self, ctx: &Context, parent: Option<&eframe::Frame>) {
        for action in std::mem::take(&mut self.pending) {
            self.run_picked(ctx, &action, parent);
        }
    }

    /// Asks the picker; remembers the folder it was answered in.
    fn pick(
        &mut self,
        ctx: &Context,
        kind: PickKind,
        title_key: &str,
        parent: Option<&eframe::Frame>,
    ) -> Option<PathBuf> {
        let request = PickRequest {
            kind,
            title: tr(ctx, title_key),
            start_dir: self.last_dir.clone(),
        };
        let path = self.picker.pick(&request, parent)?;
        let folder = match kind {
            PickKind::OpenFolder | PickKind::SaveFolder | PickKind::NewFolder => Some(&*path),
            _ => path.parent(),
        };
        if let Some(folder) = folder.filter(|f| !f.as_os_str().is_empty()) {
            self.last_dir = Some(folder.to_owned());
            let _ = self.persist_state();
        }
        Some(path)
    }

    fn run_picked(&mut self, ctx: &Context, action: &str, parent: Option<&eframe::Frame>) {
        match action {
            "file.open" | "file.open-folder" => {
                let (kind, title) = if action == "file.open" {
                    (PickKind::OpenProject, "pick-open-title")
                } else {
                    (PickKind::OpenFolder, "pick-open-folder-title")
                };
                if let Some(path) = self.pick(ctx, kind, title, parent) {
                    self.open_and_report(ctx, &path);
                }
            }
            "file.save-as" => {
                if let Some(mut path) =
                    self.pick(ctx, PickKind::SaveProject, "pick-save-title", parent)
                {
                    if path.extension().is_none() {
                        path.set_extension("tsukumoe");
                    }
                    let r = self.save_as(&path);
                    self.report(ctx, r, Some(tr(ctx, "app-saved")));
                }
            }
            "file.save-as-folder" => {
                if let Some(path) =
                    self.pick(ctx, PickKind::SaveFolder, "pick-save-folder-title", parent)
                {
                    let r = self.save_as_folder(&path);
                    self.report(ctx, r, Some(tr(ctx, "app-saved")));
                }
            }
            "file.new" => {
                if let Some(path) =
                    self.pick(ctx, PickKind::NewFolder, "pick-new-folder-title", parent)
                {
                    let r = self.create_and_open(&path);
                    let ok = tr_args(
                        ctx,
                        "app-opened",
                        &[("name", Arg::from(path.display().to_string().as_str()))],
                    );
                    self.report(ctx, r, Some(ok));
                }
            }
            "file.new-from-psd" => {
                let Some(psd) = self.pick(ctx, PickKind::Psd, "pick-psd-title", parent) else {
                    return;
                };
                let Some(dir) =
                    self.pick(ctx, PickKind::NewFolder, "pick-new-folder-title", parent)
                else {
                    return;
                };
                let r = self.new_from_psd(&psd, &dir);
                self.report(ctx, r, None);
            }
            "file.import-psd" => {
                if let Some(psd) = self.pick(ctx, PickKind::Psd, "pick-psd-title", parent) {
                    let mut state = PsdImport::new();
                    state.path = psd.display().to_string();
                    state.start();
                    self.dialog = Some(Dialog::PsdImport(Box::new(state)));
                }
            }
            _ => {}
        }
    }

    fn open_and_report(&mut self, ctx: &Context, path: &Path) {
        let r = self.open(path);
        let ok = tr_args(
            ctx,
            "app-opened",
            &[("name", Arg::from(path.display().to_string().as_str()))],
        );
        self.report(ctx, r, Some(ok));
    }

    /// Saves the open document as a folder project.
    pub fn save_as_folder(&mut self, path: &Path) -> Result<(), String> {
        let Some(doc) = self.doc.as_mut() else {
            return Ok(());
        };
        doc.save_to_folder(path)?;
        self.recent.push(path);
        self.persist_state()
    }

    /// Makes an empty folder project in `dir` and opens it.
    pub fn create_and_open(&mut self, dir: &Path) -> Result<(), String> {
        let written = crate::project::create_project(dir)?;
        self.open(dir).inspect_err(|_| {
            crate::project::discard_project(dir, &written);
        })
    }

    /// Makes an empty folder project in `dir`, opens it and shows the import
    /// dialog for `psd`. Cancelling the dialog removes the project again and
    /// brings back the document that was open.
    pub fn new_from_psd(&mut self, psd: &Path, dir: &Path) -> Result<(), String> {
        let written = crate::project::create_project(dir)?;
        let previous = self.doc.take();
        if let Err(e) = self.open(dir) {
            self.doc = previous;
            crate::project::discard_project(dir, &written);
            return Err(e);
        }
        self.stash = previous;
        let mut state = PsdImport::new();
        state.path = psd.display().to_string();
        state.created = Some((dir.to_owned(), written));
        state.start();
        self.dialog = Some(Dialog::PsdImport(Box::new(state)));
        Ok(())
    }

    /// The import dialog closed. A new project whose import was not confirmed
    /// is removed and the earlier document comes back.
    fn psd_dialog_closed(&mut self, state: &PsdImport) {
        let Some((dir, written)) = &state.created else {
            return;
        };
        if state.confirmed {
            self.stash = None;
            return;
        }
        crate::project::discard_project(dir, written);
        self.recent.remove(dir);
        self.doc = self.stash.take();
        self.store_tree_views();
        self.view = ViewState::default();
        self.load_tree_views();
        self.viewport = None;
        let _ = self.persist_state();
    }

    /// Draws the open pie menu; the key's release picks the item under the
    /// pointer (or cancels at the center).
    fn pie_frame(&mut self, ctx: &Context) {
        let Some(pie) = self.pie.as_mut() else {
            return;
        };
        let released = match self.keymap.shortcut("view.pie-menu") {
            Some(s) => ctx.input(|i| i.key_released(s.logical_key)),
            None => true,
        };
        match pie.show(ctx, released) {
            tsukumoe_ui::PieEvent::Open => {}
            tsukumoe_ui::PieEvent::Chosen(action) => {
                self.pie = None;
                self.run_action(ctx, &action);
            }
            tsukumoe_ui::PieEvent::Cancelled => self.pie = None,
        }
    }

    /// Shows the command palette panel and puts the cursor in its search field.
    pub fn open_palette(&mut self) {
        self.layouts.current_mut(self.workspace).reveal("palette");
        self.layouts_dirty = true;
        self.palette.focus = true;
    }

    pub fn undo(&mut self) -> bool {
        self.doc.as_mut().is_some_and(|d| d.dispatcher.undo())
    }

    pub fn redo(&mut self) -> bool {
        self.doc.as_mut().is_some_and(|d| d.dispatcher.redo())
    }

    pub fn switch_workspace(&mut self, workspace: Workspace) {
        if self.workspace != workspace {
            self.store_tree_views();
            self.workspace = workspace;
            self.load_tree_views();
            let _ = self.persist_state();
        }
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.layouts.apply_preset(self.workspace, preset);
        self.layouts_dirty = true;
    }

    fn report(&mut self, ctx: &Context, result: Result<(), String>, ok: Option<String>) {
        match result {
            Ok(()) => {
                if let Some(text) = ok {
                    self.toasts.push(ctx, Tone::Success, text);
                }
            }
            Err(e) => self.toasts.push(ctx, Tone::Error, e),
        }
    }

    /// One frame of the whole editor window.
    /// `rs` is the wgpu state for the viewport; without it the viewport says so.
    pub fn ui(&mut self, ui: &mut Ui, rs: Option<&RenderState>) {
        let ctx = ui.ctx().clone();
        let active: &[&str] = if self.viewport_hovered {
            &["viewport"]
        } else {
            &[]
        };
        self.viewport_hovered = false;
        for action in self.keymap.triggered(&ctx, active) {
            self.run_action(&ctx, &action);
        }
        self.pie_frame(&ctx);
        let maximized = titlebar::is_maximized(&ctx);
        egui::Panel::top("tsukumoe-menu").show(ui, |ui| self.menu_bar(ui, maximized));
        if let Some(op) = titlebar::frame(&ctx, titlebar::CHROME, maximized) {
            ctx.send_viewport_cmd(titlebar::command(op, maximized));
        }
        self.sync_window_title(&ctx);
        egui::Panel::bottom("tsukumoe-status").show(ui, |ui| self.status_bar(ui));
        egui::CentralPanel::default().show(ui, |ui| {
            let mut layout = self.layouts.current(self.workspace).clone();
            let before = layout.clone();
            dock::show(ui, &mut layout, &mut Panels { editor: self, rs });
            if layout != before {
                *self.layouts.current_mut(self.workspace) = layout;
                self.layouts_dirty = true;
            }
        });
        self.dialogs(&ctx);
        self.save_trees(&ctx);
        // Writing while a divider is dragged would spam the disk.
        if self.layouts_dirty && !ctx.input(|i| i.pointer.any_down()) {
            let result = self.persist_layouts();
            self.report(&ctx, result, None);
        }
        self.toasts.show(&ctx);
    }

    /// The menu as shown now, from [`menu::MENU`] and the editor's state.
    pub fn menu_entries(&self, ctx: &Context) -> Vec<menu::Entry> {
        let (can_undo, can_redo) = self.doc.as_ref().map_or((false, false), |d| {
            (d.dispatcher.can_undo(), d.dispatcher.can_redo())
        });
        let state = menu::State {
            has_doc: self.doc.is_some(),
            can_undo,
            can_redo,
            recent: self
                .recent
                .paths()
                .iter()
                .map(|p| p.display().to_string())
                .collect(),
            saved_layouts: self
                .layouts
                .saved
                .iter()
                .filter(|s| s.workspace == self.workspace)
                .map(|s| s.name.clone())
                .collect(),
            visible_panels: crate::workspace::PANELS
                .iter()
                .filter(|id| self.layouts.current(self.workspace).is_visible(id))
                .map(|id| (*id).to_owned())
                .collect(),
            tab_style: self.tab_style,
        };
        menu::resolve(menu::MENU, &state, &|key| tr(ctx, key), &|action| {
            self.keymap.shortcut_text(ctx, action)
        })
    }

    /// The title bar row: names, the menu (not with the system menu bar) and
    /// the workspace tabs, with the window buttons at their side.
    fn menu_bar(&mut self, ui: &mut Ui, maximized: bool) {
        let ctx = ui.ctx().clone();
        let t = |key: &str| tr(&ctx, key);
        let entries = self.menu_entries(&ctx);
        let mut chosen = None;
        let mut switch = None;
        let name = self.doc.as_ref().map(|d| (d.name().to_owned(), d.dirty()));
        let native = self.native_menu;
        let workspace = self.workspace;
        let op = titlebar::show(ui, titlebar::CHROME, maximized, |ui| {
            ui.label(egui::RichText::new("Tsukumoe").strong());
            ui.add_space(4.0);
            if let Some((name, dirty)) = &name {
                hint(
                    ui,
                    format!("{name}{}", if *dirty { " \u{25cf}" } else { "" }),
                );
            }
            ui.separator();
            egui::MenuBar::new().ui(ui, |ui| {
                for entry in entries.iter().filter(|_| !native) {
                    if let menu::Entry::Sub { label, entries } = entry {
                        ui.menu_button(label, |ui| show_entries(ui, entries, &mut chosen));
                    }
                }
                if !native {
                    ui.separator();
                }
                let mut selected = Workspace::ALL
                    .iter()
                    .position(|w| *w == workspace)
                    .unwrap_or(0);
                let names: Vec<String> = Workspace::ALL
                    .iter()
                    .map(|w| t(&format!("workspace-{}", w.key())))
                    .collect();
                let names_ref: Vec<&str> = names.iter().map(String::as_str).collect();
                if tsukumoe_ui::tabs(ui, &mut selected, &names_ref).changed() {
                    switch = Some(Workspace::ALL[selected]);
                }
            });
        });
        if let Some(op) = op {
            ctx.send_viewport_cmd(titlebar::command(op, maximized));
        }
        if let Some(w) = switch {
            self.switch_workspace(w);
        }
        if let Some(action) = chosen {
            self.run_action(&ctx, &action);
        }
    }

    /// The OS title (task bar, Mission Control) follows the project.
    fn sync_window_title(&mut self, ctx: &Context) {
        let title = match &self.doc {
            Some(d) => format!(
                "{}{} - Tsukumoe",
                d.name(),
                if d.dirty() { " *" } else { "" }
            ),
            None => "Tsukumoe".to_owned(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.horizontal(|ui| match &self.doc {
            Some(doc) => {
                hint(ui, doc.name());
                if doc.dirty() {
                    tsukumoe_ui::badge(ui, &tr(&ctx, "app-unsaved"), Tone::Warning);
                }
                if let Some(e) = doc.dispatcher.error() {
                    tsukumoe_ui::badge(ui, e, Tone::Error);
                }
            }
            None => {
                hint(ui, tr(&ctx, "app-no-project"));
            }
        });
    }

    fn dialogs(&mut self, ctx: &Context) {
        let Some(mut dialog) = self.dialog.take() else {
            return;
        };
        let t = |key: &str| tr(ctx, key);
        if matches!(dialog, Dialog::Pie) {
            let mut open = true;
            if self.pie_dialog(ctx, &mut open)
                && let Some(dir) = &self.settings_dir
                && let Err(e) = settings::write(dir, PIE_FILE, &self.pie_config.to_json())
            {
                self.toasts.push(ctx, Tone::Error, e);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        if matches!(dialog, Dialog::Drag) {
            let mut open = true;
            if self.drag_dialog(ctx, &mut open)
                && let Some(dir) = &self.settings_dir
                && let Err(e) = settings::write(dir, DRAG_FILE, &self.drag.to_json())
            {
                self.toasts.push(ctx, Tone::Error, e);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        if let Dialog::PsdImport(state) = &mut dialog {
            let mut open = true;
            self.psd_dialog(ctx, state, &mut open);
            if open {
                self.dialog = Some(dialog);
            } else if let Dialog::PsdImport(state) = &dialog {
                self.psd_dialog_closed(state);
            }
            return;
        }
        if let Dialog::Keymap(texts, error) = &mut dialog {
            let mut open = true;
            let changed = self.keymap_dialog(ctx, texts, error, &mut open);
            if changed {
                self.save_keymap(ctx);
            }
            if open {
                self.dialog = Some(dialog);
            }
            return;
        }
        let mut open = true;
        let mut done = false;
        let mut cancel = false;
        let (title, id) = match &dialog {
            Dialog::Open(_) => (t("dialog-open-title"), "dialog-open"),
            Dialog::SaveAs(_) => (t("dialog-save-as-title"), "dialog-save-as"),
            Dialog::NewProject(_) => (t("dialog-new-title"), "dialog-new"),
            Dialog::NewFromPsd(..) => (t("dialog-new-from-psd-title"), "dialog-new-from-psd"),
            Dialog::SaveAsFolder(_) => (t("dialog-save-folder-title"), "dialog-save-folder"),
            Dialog::SaveLayout(_) => (t("layout-save-title"), "dialog-save-layout"),
            Dialog::ConfirmPreset(_) => (t("layout-confirm-title"), "dialog-confirm-preset"),
            Dialog::ConfirmDelete(..) => (t("tree-delete-title"), "dialog-confirm-delete"),
            Dialog::Keymap(..) | Dialog::Drag | Dialog::Pie | Dialog::PsdImport(_) => {
                unreachable!("handled above")
            }
        };
        tsukumoe_ui::dialog(ctx, id, &title, &mut open, |ui| {
            match &mut dialog {
                Dialog::Open(text) | Dialog::SaveAs(text) => {
                    let name = t("dialog-path");
                    text_input(ui, text, &name, &name).request_focus();
                }
                Dialog::NewProject(text) | Dialog::SaveAsFolder(text) => {
                    let name = t("dialog-folder-path");
                    text_input(ui, text, &name, &name).request_focus();
                }
                Dialog::NewFromPsd(psd, folder) => {
                    let name = t("dialog-psd-file-path");
                    text_input(ui, psd, &name, &name).request_focus();
                    let name = t("dialog-folder-path");
                    text_input(ui, folder, &name, &name);
                }
                Dialog::SaveLayout(text) => {
                    let name = t("layout-name");
                    text_input(ui, text, &name, &name).request_focus();
                }
                Dialog::ConfirmPreset(_) => {
                    hint(ui, t("layout-confirm-body"));
                }
                Dialog::ConfirmDelete(_, count) => {
                    hint(
                        ui,
                        tr_args(ctx, "tree-delete-body", &[("count", Arg::from(*count))]),
                    );
                }
                Dialog::Keymap(..) | Dialog::Drag | Dialog::Pie | Dialog::PsdImport(_) => {}
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                done = button(ui, &t("dialog-ok"), Variant::Primary).clicked();
                if button(ui, &t("dialog-cancel"), Variant::Secondary).clicked() {
                    cancel = true;
                }
            });
        });
        open &= !cancel;
        if done {
            match &dialog {
                Dialog::Open(text) if !text.trim().is_empty() => {
                    let path = PathBuf::from(text.trim());
                    let r = self.open(&path);
                    let ok = tr_args(
                        ctx,
                        "app-opened",
                        &[("name", Arg::from(path.display().to_string().as_str()))],
                    );
                    self.report(ctx, r, Some(ok));
                }
                Dialog::SaveAs(text) if !text.trim().is_empty() => {
                    let r = self.save_as(Path::new(text.trim()));
                    self.report(ctx, r, Some(t("app-saved")));
                }
                Dialog::NewProject(text) if !text.trim().is_empty() => {
                    let path = PathBuf::from(text.trim());
                    let r = self.create_and_open(&path);
                    let ok = tr_args(
                        ctx,
                        "app-opened",
                        &[("name", Arg::from(path.display().to_string().as_str()))],
                    );
                    self.report(ctx, r, Some(ok));
                }
                Dialog::NewFromPsd(psd, folder)
                    if !psd.trim().is_empty() && !folder.trim().is_empty() =>
                {
                    let r = self.new_from_psd(Path::new(psd.trim()), Path::new(folder.trim()));
                    self.report(ctx, r, None);
                }
                Dialog::SaveAsFolder(text) if !text.trim().is_empty() => {
                    let r = self.save_as_folder(Path::new(text.trim()));
                    self.report(ctx, r, Some(t("app-saved")));
                }
                Dialog::SaveLayout(text) => {
                    if self.layouts.save_named(self.workspace, text) {
                        self.layouts_dirty = true;
                    }
                }
                Dialog::ConfirmPreset(preset) => self.apply_preset(*preset),
                Dialog::ConfirmDelete(items, _) => self.delete_items(ctx, items),
                _ => {}
            }
            open = false;
        }
        if open {
            self.dialog = Some(dialog);
        }
    }
}

impl Editor {
    /// File > Import PSD: path, then the diff with the choices, then one
    /// command. The state is `PsdImport`; this only draws and forwards.
    fn psd_dialog(&mut self, ctx: &Context, state: &mut PsdImport, open: &mut bool) {
        let t = |key: &str| tr(ctx, key);
        let Some(doc) = self.doc.as_mut() else {
            *open = false;
            return;
        };
        if state.poll(doc) || state.stage == Stage::Loading {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        let candidates = state.match_candidates();
        let rows = state.rows();
        let mut load = false;
        let mut confirm = false;
        let mut cancel = false;
        let mut remove: Option<(String, bool)> = None;
        let mut matched: Option<(String, Option<String>)> = None;
        let none = t("psd-none");
        let title = t("psd-title");
        tsukumoe_ui::dialog_sized(ctx, "dialog-psd-import", &title, 640.0, open, |ui| {
            match state.stage {
                Stage::Path => {
                    let name = t("psd-path");
                    let response = text_input(ui, &mut state.path, &name, &name);
                    response.request_focus();
                    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        load = true;
                    }
                }
                Stage::Loading => {
                    hint(ui, t("psd-loading"));
                }
                Stage::Review => {
                    hint(ui, state.path.clone());
                    if rows.is_empty() {
                        hint(ui, t("psd-nothing"));
                    }
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for row in &rows {
                                ui.horizontal_wrapped(|ui| {
                                    let (key, tone) = match row.kind {
                                        RowKind::AddGroup => ("psd-row-add-group", Tone::Success),
                                        RowKind::AddLayer => ("psd-row-add-layer", Tone::Success),
                                        RowKind::ImageChanged => ("psd-row-image", Tone::Warning),
                                        RowKind::Updated => ("psd-row-updated", Tone::Neutral),
                                        RowKind::Missing { .. } => ("psd-row-missing", Tone::Error),
                                    };
                                    tsukumoe_ui::badge(ui, &t(key), tone);
                                    label(ui, row.key.clone());
                                    if !row.detail.is_empty() {
                                        hint(ui, row.detail.clone());
                                    }
                                    if let RowKind::Missing { remove: current } = row.kind {
                                        let mut value = current;
                                        if checkbox(ui, &mut value, &t("psd-remove")).changed() {
                                            remove = Some((row.key.clone(), value));
                                        }
                                    }
                                    let can_match = !candidates.is_empty()
                                        && (row.kind == RowKind::AddLayer
                                            || row.matched_with.is_some());
                                    if can_match && state.is_new_layer(&row.key) {
                                        let mut value = row.matched_with.clone();
                                        let mut options: Vec<(Option<String>, String)> =
                                            vec![(None, none.clone())];
                                        options.extend(
                                            candidates.iter().map(|c| (Some(c.clone()), c.clone())),
                                        );
                                        let refs: Vec<(Option<String>, &str)> = options
                                            .iter()
                                            .map(|(v, n)| (v.clone(), n.as_str()))
                                            .collect();
                                        if tsukumoe_ui::select(
                                            ui,
                                            ("psd-match", &row.key),
                                            &t("psd-match"),
                                            &mut value,
                                            &refs,
                                        )
                                        .changed()
                                        {
                                            matched = Some((row.key.clone(), value));
                                        }
                                    }
                                });
                            }
                        });
                    for note in state.notes() {
                        hint(ui, format!("{}: {}", note.layer, note.message));
                    }
                }
            }
            if let Some(e) = &state.error {
                tsukumoe_ui::badge(ui, e, Tone::Error);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                match state.stage {
                    Stage::Path => load |= button(ui, &t("psd-read"), Variant::Primary).clicked(),
                    Stage::Loading => {}
                    Stage::Review => {
                        ui.add_enabled_ui(state.can_confirm(), |ui| {
                            confirm = button(ui, &t("psd-import"), Variant::Primary).clicked();
                        });
                    }
                }
                cancel = button(ui, &t("dialog-cancel"), Variant::Secondary).clicked();
            });
        });
        if let Some((key, value)) = remove {
            state.set_remove(&key, value, doc);
        }
        if let Some((new, old)) = matched {
            state.set_match(&new, old.as_deref(), doc);
        }
        if load {
            state.start();
        }
        if confirm {
            match state.confirm(doc) {
                Ok(()) => {
                    let layers = state.plan().map_or(0, |p| p.items.len());
                    self.toasts.push(
                        ctx,
                        Tone::Success,
                        tr_args(ctx, "psd-imported", &[("count", Arg::from(layers))]),
                    );
                    // The viewport reads its textures when it is made.
                    self.viewport = None;
                    *open = false;
                }
                Err(e) => state.error = Some(e),
            }
        }
        if cancel {
            *open = false;
        }
    }
}

impl Editor {
    /// Which action each pie slot runs; returns whether anything changed.
    fn pie_dialog(&mut self, ctx: &Context, open: &mut bool) -> bool {
        let t = |key: &str| tr(ctx, key);
        let before = self.pie_config.clone();
        let names: Vec<(String, String)> = ACTIONS
            .iter()
            .map(|a| ((*a).to_owned(), t(&actions::label_key(a))))
            .collect();
        let options: Vec<(String, &str)> =
            names.iter().map(|(a, n)| (a.clone(), n.as_str())).collect();
        let config = &mut self.pie_config;
        let mut reset = false;
        tsukumoe_ui::dialog(ctx, "dialog-pie", &t("pie-title"), open, |ui| {
            hint(ui, t("pie-hint"));
            fn slots(
                ui: &mut Ui,
                entries: &mut [Entry],
                options: &[(String, &str)],
                path: &str,
                t: &dyn Fn(&str) -> String,
            ) {
                for (i, entry) in entries.iter_mut().enumerate() {
                    let id = format!("{path}/{i}");
                    match entry {
                        Entry::Action { action } => {
                            tsukumoe_ui::select(ui, id, &format!("{}", i + 1), action, options);
                        }
                        Entry::Group { label, items } => {
                            label_text(ui, &t(label));
                            ui.indent(id.clone(), |ui| slots(ui, items, options, &id, t));
                        }
                    }
                }
            }
            fn label_text(ui: &mut Ui, text: &str) {
                label(ui, text);
            }
            slots(ui, &mut config.items, &options, "pie", &t);
            reset = button(ui, &t("pie-reset"), Variant::Secondary).clicked();
        });
        if reset {
            *config = PieConfig::default();
        }
        *config != before
    }

    /// The drag-to-parameter settings; returns whether they changed.
    fn drag_dialog(&mut self, ctx: &Context, open: &mut bool) -> bool {
        let t = |key: &str| tr(ctx, key);
        let names: Vec<String> = self.doc.as_ref().map_or_else(Vec::new, |d| {
            d.dispatcher
                .model()
                .parameters()
                .iter()
                .map(|(_, n, _)| n.to_owned())
                .collect()
        });
        let before = self.drag.clone();
        let drag = &mut self.drag;
        tsukumoe_ui::dialog(ctx, "dialog-drag", &t("drag-title"), open, |ui| {
            hint(ui, t("drag-hint"));
            let modifiers = [
                (Modifier::Alt, "Alt".to_owned()),
                (Modifier::Ctrl, "Ctrl".to_owned()),
                (Modifier::Cmd, "Cmd".to_owned()),
            ];
            let options: Vec<(Modifier, &str)> =
                modifiers.iter().map(|(m, n)| (*m, n.as_str())).collect();
            tsukumoe_ui::select(
                ui,
                "drag-modifier",
                &t("drag-modifier"),
                &mut drag.modifier,
                &options,
            );
            for (key, axis) in [("drag-x", &mut drag.x), ("drag-y", &mut drag.y)] {
                ui.separator();
                label(ui, t(key));
                let mut choice: Option<String> = axis.as_ref().map(|a| a.parameter.clone());
                let mut options: Vec<(Option<String>, &str)> = vec![(None, "-")];
                options.extend(names.iter().map(|n| (Some(n.clone()), n.as_str())));
                let none = t("drag-none");
                options[0].1 = &none;
                if tsukumoe_ui::select(ui, key, &t("drag-parameter"), &mut choice, &options)
                    .changed()
                {
                    *axis = choice.map(|parameter| match axis.take() {
                        Some(a) => AxisTarget { parameter, ..a },
                        None => AxisTarget {
                            parameter,
                            points_per_range: 400.0,
                            invert: false,
                        },
                    });
                }
                if let Some(a) = axis {
                    ui.horizontal(|ui| {
                        label(ui, t("drag-sensitivity"));
                        tsukumoe_ui::number(
                            ui,
                            &mut a.points_per_range,
                            20.0..=4000.0,
                            5.0,
                            &t("drag-sensitivity"),
                        );
                        checkbox(ui, &mut a.invert, &t("drag-invert"));
                    });
                }
            }
        });
        *drag != before
    }

    /// The shortcut list. Each field applies when it loses focus; returns
    /// whether the keymap changed.
    fn keymap_dialog(
        &mut self,
        ctx: &Context,
        texts: &mut [String],
        error: &mut Option<String>,
        open: &mut bool,
    ) -> bool {
        let t = |key: &str| tr(ctx, key);
        let mut changed = false;
        let mut reset = false;
        tsukumoe_ui::dialog(ctx, "dialog-keymap", &t("keymap-title"), open, |ui| {
            hint(ui, t("keymap-hint"));
            for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                ui.horizontal(|ui| {
                    ui.set_min_width(420.0);
                    ui.label(t(&actions::label_key(action)));
                    let r = text_input(ui, text, action, "");
                    if r.lost_focus() {
                        let context = actions::context_of(action);
                        let text = text.trim();
                        if text.is_empty() {
                            changed |= self.keymap.unbind(action, context);
                            *error = None;
                        } else {
                            match self.keymap.bind(action, context, text) {
                                Ok(()) => {
                                    changed = true;
                                    *error = None;
                                }
                                Err(_) => *error = Some(t("keymap-bad")),
                            }
                        }
                    }
                });
            }
            for (a, b) in self.keymap.conflicts() {
                let name = |id: &str| t(&actions::label_key(id));
                hint(
                    ui,
                    tr_args(
                        ctx,
                        "keymap-conflict",
                        &[
                            ("a", Arg::from(name(&a).as_str())),
                            ("b", Arg::from(name(&b).as_str())),
                        ],
                    ),
                );
            }
            if let Some(e) = error.as_ref() {
                hint(ui, e.clone());
            }
            reset = button(ui, &t("keymap-reset"), Variant::Secondary).clicked();
        });
        if reset {
            self.keymap = actions::cubism_compatible();
            for (action, text) in ACTIONS.iter().zip(texts.iter_mut()) {
                *text = self
                    .keymap
                    .keys_text(action, actions::context_of(action))
                    .unwrap_or_default();
            }
            changed = true;
        }
        changed
    }
}

/// The dock's view of the editor: panel names and their contents.
/// Draws `entries` as egui menu items; the action of a clicked one goes to `chosen`.
fn show_entries(ui: &mut Ui, entries: &[menu::Entry], chosen: &mut Option<String>) {
    for entry in entries {
        match entry {
            menu::Entry::Separator => {
                ui.separator();
            }
            menu::Entry::Sub { label, entries } => {
                ui.menu_button(label, |ui| show_entries(ui, entries, chosen));
            }
            menu::Entry::Item {
                action,
                label,
                enabled,
                shortcut,
                checked,
            } => {
                ui.add_enabled_ui(*enabled, |ui| {
                    if tsukumoe_ui::menu_item_checked(ui, label, shortcut.as_deref(), *checked)
                        .clicked()
                    {
                        *chosen = Some(action.clone());
                        ui.close();
                    }
                });
            }
        }
    }
}

struct Panels<'a> {
    editor: &'a mut Editor,
    rs: Option<&'a RenderState>,
}

impl PanelHost for Panels<'_> {
    fn title(&self, ctx: &Context, id: &str) -> String {
        tr(ctx, &format!("panel-{id}"))
    }

    fn tab_style(&self) -> TabStyle {
        self.editor.tab_style
    }

    fn icon(&self, id: &str) -> Option<Icon> {
        Some(match id {
            "viewport" => Icon::Image,
            "tree" => Icon::ListTree,
            "deformers" => Icon::Grid,
            "parameters" => Icon::Sliders,
            "inspector" => Icon::Info,
            "history" => Icon::History,
            "palette" => Icon::Search,
            "ai_diff" => Icon::Sparkles,
            _ => return None,
        })
    }

    fn ui(&mut self, ui: &mut Ui, id: &str) {
        let ctx = ui.ctx().clone();
        let editor = &mut *self.editor;
        let Some(doc) = editor.doc.as_mut() else {
            match id {
                "viewport" => viewport::placeholder(ui, false),
                _ => {
                    hint(ui, tr(&ctx, "history-empty"));
                }
            }
            return;
        };
        match id {
            "history" => history::show(ui, &mut doc.dispatcher),
            "tree" | "deformers" => {
                let panel = if id == "tree" {
                    tree_model::Panel::Parts
                } else {
                    tree_model::Panel::Deformers
                };
                for out in tree::show(ui, doc.dispatcher.model(), &mut editor.view, panel) {
                    editor.apply_tree_out(&ctx, out);
                }
            }
            "parameters" => parameters::show(ui, doc.dispatcher.model(), &mut editor.view),
            "inspector" => {
                let mut requests = inspector::Requests::default();
                let command = inspector::show_with(
                    ui,
                    doc.dispatcher.model(),
                    editor.view.selection.as_ref(),
                    Some(&editor.deformer_tool),
                    &mut requests,
                );
                if let Some((name, origin)) = requests.move_axis.take() {
                    editor.move_axis(&ctx, &name, origin);
                }
                if requests.reset {
                    let items = editor.view.selected();
                    editor.reset_deformers(&ctx, &items);
                }
                if let Some(command) = command {
                    editor.submit(&ctx, command);
                }
            }
            "palette" => {
                if let Some(command) = editor.palette.show(ui) {
                    editor.submit(&ctx, command);
                }
            }
            "viewport" => match self.rs {
                Some(rs) => {
                    if editor.viewport.as_ref().map(Viewport::doc_id) != Some(doc.id) {
                        match Viewport::new(rs, doc) {
                            Ok(v) => editor.viewport = Some(v),
                            Err(e) => {
                                editor.viewport = None;
                                hint(ui, e);
                            }
                        }
                    }
                    if let Some(vp) = editor.viewport.as_mut() {
                        // The tools work on the mesh and the deformer chosen last.
                        let mesh = editor.view.last_mesh();
                        editor.mesh_tool.set_mesh(mesh.as_deref());
                        let deformer = editor.view.last_deformer();
                        editor.deformer_tool.set_deformer(deformer.as_deref());
                        let (response, ops, actions) = vp.show(
                            ui,
                            rs,
                            doc,
                            &mut editor.view,
                            &editor.drag,
                            &mut editor.mesh_tool,
                            &mut editor.deformer_tool,
                            &editor.keymap,
                        );
                        editor.viewport_hovered = response.hovered();
                        for op in ops {
                            editor.apply_mesh_op(&ctx, op);
                        }
                        for action in actions {
                            editor.run_action(&ctx, action);
                        }
                    }
                }
                None => viewport::placeholder(ui, true),
            },
            _ => {
                hint(ui, tr(&ctx, "panel-empty"));
            }
        }
    }
}

/// The name an element of the selection has.
fn selection_name(s: &Selection) -> &str {
    match s {
        Selection::Part(n)
        | Selection::Mesh(n)
        | Selection::Warp(n)
        | Selection::Rotation(n)
        | Selection::Parameter(n) => n,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tsukumoe-app-editor-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn harness(editor: Editor) -> Harness<'static, Editor> {
        Harness::builder()
            .with_size(egui::vec2(1200.0, 700.0))
            .build_ui_state(
                |ui, editor: &mut Editor| {
                    tsukumoe_ui::init(ui.ctx());
                    editor.ui(ui, None);
                },
                editor,
            )
    }

    #[test]
    fn switching_workspaces_keeps_the_document_and_separate_layouts() {
        let dir = scratch("ws");
        let mut editor = Editor::new(Some(dir.clone()));
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        editor
            .doc
            .as_mut()
            .unwrap()
            .dispatcher
            .submit(tsukumoe_core::command::Command::AddPart {
                name: "P".into(),
                parent: None,
            })
            .unwrap();
        let mut h = harness(editor);
        h.run();
        h.get_by_label("Animation").click();
        h.run();
        let e = h.state();
        assert_eq!(e.workspace, Workspace::Animation);
        let d = e.doc.as_ref().unwrap();
        assert!(
            d.dirty() && d.dispatcher.can_undo(),
            "model and history survive"
        );
        assert_ne!(
            e.layouts.current(Workspace::Modeling),
            e.layouts.current(Workspace::Animation)
        );
        // the choice is remembered
        assert_eq!(
            Editor::new(Some(dir.clone())).workspace,
            Workspace::Animation
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_preset_change_is_saved_and_restored() {
        let dir = scratch("preset");
        let mut editor = Editor::new(Some(dir.clone()));
        editor.apply_preset(Preset::Cubism);
        let mut h = harness(editor);
        h.run();
        let again = Editor::new(Some(dir.clone()));
        assert_eq!(
            again.layouts.current(Workspace::Modeling),
            &crate::workspace::builtin_layout(Preset::Cubism, Workspace::Modeling)
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_proportional_action_flips_the_option_and_the_key_is_o() {
        let mut editor = Editor::new(None);
        let ctx = Context::default();
        assert!(!editor.view.proportional.on);
        assert!(editor.run_action(&ctx, "proportional.toggle"));
        assert!(editor.view.proportional.on);
        assert!(editor.run_action(&ctx, "proportional.toggle"));
        assert!(!editor.view.proportional.on);
        assert_eq!(
            editor.keymap.shortcut("proportional.toggle"),
            Some(egui::KeyboardShortcut::new(
                egui::Modifiers::NONE,
                egui::Key::O
            ))
        );
    }

    #[test]
    fn edits_go_through_one_door_and_failures_surface() {
        use tsukumoe_core::command::Command;
        let mut editor = Editor::new(None);
        let ctx = Context::default();
        assert!(!editor.submit(
            &ctx,
            Command::AddPart {
                name: "x".into(),
                parent: None
            }
        ));
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        let add = || Command::AddPart {
            name: "P".into(),
            parent: None,
        };
        assert!(editor.submit(&ctx, add()));
        let parts = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.model().parts().len();
        let n = parts(&editor);
        assert!(!editor.submit(&ctx, add()), "duplicate name fails");
        assert_eq!(parts(&editor), n);
        assert!(!editor.toasts.is_empty());
        assert!(editor.doc.as_ref().unwrap().dispatcher.error().is_some());
        assert!(editor.undo() && parts(&editor) == n - 1);
        assert!(editor.redo() && parts(&editor) == n);
    }

    /// An editor that opens its own path-input dialogs on every OS.
    fn plain() -> Editor {
        let mut editor = Editor::new(None);
        editor.picker = Box::new(crate::picker::PathInputPicker);
        editor
    }

    fn open_minimal() -> Editor {
        let mut editor = plain();
        editor
            .open(
                &Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../tsukumoe-format/tests/fixtures/minimal"),
            )
            .unwrap();
        editor
    }

    #[test]
    fn importing_a_psd_through_the_dialog_changes_the_document_in_one_step() {
        use tsukumoe_psd::writer::{LayerSpec, Node, PsdSpec, write};
        let dir = scratch("psd");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("a.psd");
        let layer = Node::Layer(LayerSpec::solid("bg", 0, 0, 8, 8, [1, 2, 3, 255]));
        std::fs::write(&file, write(&PsdSpec::new(8, 8, vec![layer]))).unwrap();

        let ctx = Context::default();
        let mut none = plain();
        assert!(none.run_action(&ctx, "file.import-psd"));
        assert!(none.dialog.is_none(), "needs a project");

        let mut editor = open_minimal();
        assert!(editor.run_action(&ctx, "file.import-psd"));
        let Some(Dialog::PsdImport(mut state)) = editor.dialog.take() else {
            panic!("the dialog did not open");
        };
        state.path = file.display().to_string();
        state.load_blocking(editor.doc.as_ref().unwrap());
        assert!(state.can_confirm());
        editor.dialog = Some(Dialog::PsdImport(state));
        // The dialog draws in the review stage.
        let mut harness = harness(editor);
        harness.run();
        let mut editor = harness.into_state();
        let Some(Dialog::PsdImport(mut state)) = editor.dialog.take() else {
            panic!("the dialog closed");
        };
        let doc = editor.doc.as_mut().unwrap();
        let before = doc.dispatcher.model().meshes().len();
        state.confirm(doc).unwrap();
        assert_eq!(doc.dispatcher.model().meshes().len(), before + 1);
        assert!(doc.dispatcher.undo());
        assert_eq!(doc.dispatcher.model().meshes().len(), before);
    }

    #[test]
    fn menu_actions_open_their_dialogs_and_unknown_names_are_refused() {
        let ctx = Context::default();
        let mut editor = plain();
        assert!(editor.run_action(&ctx, "file.save-as"));
        assert!(editor.dialog.is_none(), "nothing to save without a project");
        let mut editor = open_minimal();
        assert!(editor.run_action(&ctx, "file.save-as"));
        assert!(matches!(editor.dialog, Some(Dialog::SaveAs(_))));
        assert!(editor.run_action(&ctx, "layout.preset.cubism"));
        assert!(matches!(
            editor.dialog,
            Some(Dialog::ConfirmPreset(Preset::Cubism))
        ));
        assert!(editor.run_action(&ctx, "view.keymap-settings"));
        assert!(matches!(editor.dialog, Some(Dialog::Keymap(..))));
        assert!(editor.run_action(&ctx, "layout.save"));
        assert!(matches!(editor.dialog, Some(Dialog::SaveLayout(_))));
        assert!(!editor.run_action(&ctx, "layout.preset.nope"));
        assert!(!editor.run_action(&ctx, "nonsense.action"));
    }

    #[test]
    fn mesh_keys_send_one_named_command_and_show_why_they_fail() {
        use crate::mesh_tool::Mapper;
        use tsukumoe_core::command::Command;
        let mut editor = open_minimal();
        let ctx = Context::default();
        // A 3 x 3 grid, so an inner vertex can be removed.
        let (mut positions, mut indices) = (Vec::new(), Vec::new());
        for y in 0..3 {
            for x in 0..3 {
                positions.push([x as f32, y as f32]);
            }
        }
        for y in 0..2u32 {
            for x in 0..2u32 {
                let a = y * 3 + x;
                indices.extend([a, a + 1, a + 4, a, a + 4, a + 3]);
            }
        }
        for command in [
            Command::AddPart {
                name: "MeshEditPart".into(),
                parent: None,
            },
            Command::AddMesh {
                name: "MeshEditGrid".into(),
                part: "MeshEditPart".into(),
                positions: positions.clone(),
                uvs: positions.clone(),
                indices,
            },
        ] {
            assert!(editor.submit(&ctx, command));
        }
        editor.view.tool = Tool::Mesh;
        editor.mesh_tool.set_mesh(Some("MeshEditGrid"));
        let steps = editor.doc.as_ref().unwrap().dispatcher.log().len();
        // Nothing selected: refused with a message, no command.
        assert!(editor.run_action(&ctx, "mesh.delete-vertices"));
        assert_eq!(editor.doc.as_ref().unwrap().dispatcher.log().len(), steps);
        // Select the middle vertex and delete it.
        let m = Mapper {
            origin: [0.0, 0.0],
            size: [200.0, 200.0],
            view: [0.0, 0.0, 2.0, 2.0],
        };
        let indices = {
            let d = &editor.doc.as_ref().unwrap().dispatcher;
            d.model()
                .meshes()
                .get(d.model().meshes().find("MeshEditGrid").unwrap())
                .unwrap()
                .indices
                .clone()
        };
        editor
            .mesh_tool
            .click(&positions, &indices, &m, [100.0, 100.0], false);
        assert!(editor.run_action(&ctx, "mesh.delete-vertices"));
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert_eq!(d.log().len(), steps + 1);
        assert_eq!(
            d.name_of(steps).as_deref(),
            Some("mesh_remove_vertices: MeshEditGrid")
        );
        let count = |d: &crate::dispatcher::Dispatcher| {
            let model = d.model();
            model
                .meshes()
                .get(model.meshes().find("MeshEditGrid").unwrap())
                .unwrap()
                .positions
                .len()
        };
        assert_eq!(count(d), 8);
        assert!(d.undo());
        assert_eq!(count(d), 9);
        // C needs the texture of the mesh; the test project has none, so it
        // says so and sends nothing.
        let before = editor.doc.as_ref().unwrap().dispatcher.log().len();
        assert!(editor.run_action(&ctx, "mesh.contour"));
        assert_eq!(editor.doc.as_ref().unwrap().dispatcher.log().len(), before);
        // G fills a grid: one more step, named, and one undo returns.
        assert!(editor.run_action(&ctx, "mesh.fill-grid"));
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert_eq!(d.log().len(), steps + 1);
        assert_eq!(
            d.name_of(steps).as_deref(),
            Some("mesh_fill_grid: MeshEditGrid")
        );
        assert!(count(d) > 9);
        assert!(d.undo());
        assert_eq!(count(d), 9);
        // Other tools ignore the keys.
        editor.view.tool = Tool::Select;
        editor
            .mesh_tool
            .click(&positions, &indices, &m, [100.0, 100.0], false);
        assert!(editor.run_action(&ctx, "mesh.delete-vertices"));
        assert_eq!(count(&editor.doc.as_ref().unwrap().dispatcher), 9);
        // D outside the mesh tool still picks the deformer tool.
        assert!(editor.run_action(&ctx, "mesh.retriangulate"));
        assert_eq!(editor.view.tool, Tool::Deformer);
    }

    #[test]
    fn the_deformer_keys_wrap_the_selection_and_undo_in_one_step() {
        use crate::view::Selection;
        use tsukumoe_core::command::Command;
        let mut editor = open_minimal();
        let ctx = Context::default();
        for command in [
            Command::AddPart {
                name: "DefPart".into(),
                parent: None,
            },
            Command::AddMesh {
                name: "DefMesh".into(),
                part: "DefPart".into(),
                positions: vec![[0.0, 0.0], [2.0, 0.0], [2.0, 3.0]],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            },
        ] {
            assert!(editor.submit(&ctx, command));
        }
        let steps = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.log().len();
        let base = steps(&editor);
        // Nothing selected: a message and no command.
        assert!(editor.run_action(&ctx, "deformer.create-warp"));
        assert_eq!(steps(&editor), base);
        // A mesh selected: one command, the warp is selected and the tool switches.
        editor.view.selection = Some(Selection::Mesh("DefMesh".into()));
        editor.deformer_tool.grid = (2, 3);
        assert!(editor.run_action(&ctx, "deformer.create-warp"));
        assert_eq!(steps(&editor), base + 1);
        assert_eq!(editor.view.selection, Some(Selection::Warp("Warp".into())));
        assert_eq!(editor.view.tool, Tool::Deformer);
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert_eq!(
            d.name_of(base).as_deref(),
            Some("deformer_create_warp: Warp")
        );
        let warp = d.model().warps().find("Warp").unwrap();
        assert_eq!(d.model().warps().get(warp).unwrap().base_points.len(), 12);
        assert!(d.undo());
        assert!(d.model().warps().find("Warp").is_none());
        // Several meshes selected: one warp around all of them, one undo step.
        assert!(editor.submit(
            &ctx,
            Command::AddMesh {
                name: "DefMesh2".into(),
                part: "DefPart".into(),
                positions: vec![[8.0, 0.0], [10.0, 0.0], [10.0, 3.0]],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            }
        ));
        editor
            .view
            .select_meshes(vec!["DefMesh".into(), "DefMesh2".into()]);
        let before = steps(&editor);
        assert!(editor.run_action(&ctx, "deformer.create-warp"));
        assert_eq!(steps(&editor), before + 1);
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        let warp = d.model().warps().find("Warp").unwrap();
        let mesh_parent = |d: &crate::dispatcher::Dispatcher, n: &str| {
            let id = d.model().meshes().find(n).unwrap();
            d.model().meshes().get(id).unwrap().parent
        };
        use tsukumoe_core::rig::DeformerRef;
        assert_eq!(mesh_parent(d, "DefMesh"), Some(DeformerRef::Warp(warp)));
        assert_eq!(mesh_parent(d, "DefMesh2"), Some(DeformerRef::Warp(warp)));
        let grid = &d.model().warps().get(warp).unwrap().base_points;
        assert!(grid[0][0] < 0.0 && grid.last().unwrap()[0] > 10.0);
        assert!(d.undo());
        assert_eq!(mesh_parent(d, "DefMesh"), None);
        assert_eq!(mesh_parent(d, "DefMesh2"), None);
        // A part selected: the rotation deformer wraps the meshes under it.
        editor.view.selection = Some(Selection::Part("DefPart".into()));
        assert!(editor.run_action(&ctx, "deformer.create-rotation"));
        assert_eq!(
            editor.view.selection,
            Some(Selection::Rotation("Rotation".into()))
        );
        let d = &editor.doc.as_ref().unwrap().dispatcher;
        let mesh = d.model().meshes().find("DefMesh").unwrap();
        assert!(d.model().meshes().get(mesh).unwrap().parent.is_some());
    }

    #[test]
    fn the_tree_renames_deletes_duplicates_and_moves_through_the_one_door() {
        use crate::tree_model::Kind;
        use tsukumoe_core::command::Command;
        let mut editor = open_minimal();
        let ctx = Context::default();
        for command in [
            Command::AddPart {
                name: "TreeA".into(),
                parent: None,
            },
            Command::AddPart {
                name: "TreeB".into(),
                parent: Some("TreeA".into()),
            },
            Command::AddMesh {
                name: "TreeMesh".into(),
                part: "TreeB".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            },
        ] {
            assert!(editor.submit(&ctx, command));
        }
        let steps = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.log().len();
        let model = |e: &Editor| {
            let d = &e.doc.as_ref().unwrap().dispatcher;
            (
                d.model().meshes().find("TreeMesh").is_some(),
                d.model().parts().find("TreeB").is_some(),
            )
        };
        let base = steps(&editor);
        // Rename: one step, named, and the selection follows.
        editor.view.select_only(Selection::Mesh("TreeMesh".into()));
        editor.apply_tree_out(
            &ctx,
            tree::Out::Rename {
                kind: Kind::Mesh,
                from: "TreeMesh".into(),
                to: "Renamed".into(),
            },
        );
        assert_eq!(steps(&editor), base + 1);
        assert_eq!(
            editor.view.selection,
            Some(Selection::Mesh("Renamed".into()))
        );
        let d = &editor.doc.as_ref().unwrap().dispatcher;
        assert_eq!(
            d.name_of(base).as_deref(),
            Some("tree_rename: TreeMesh -> Renamed")
        );
        // A rename says once that outside users of the old name are not updated.
        assert!(!editor.toasts.is_empty());
        // A taken name is refused and the model stays.
        editor.apply_tree_out(
            &ctx,
            tree::Out::Rename {
                kind: Kind::Part,
                from: "TreeB".into(),
                to: "TreeA".into(),
            },
        );
        assert_eq!(steps(&editor), base + 1);
        // Duplicate: the copy is selected, one step.
        editor.apply_tree_out(
            &ctx,
            tree::Out::Duplicate(vec![Selection::Mesh("Renamed".into())]),
        );
        assert_eq!(steps(&editor), base + 2);
        assert_eq!(
            editor.view.selection,
            Some(Selection::Mesh("Renamed.copy".into()))
        );
        // Move to another part.
        editor.apply_tree_out(
            &ctx,
            tree::Out::SetPart(Selection::Mesh("Renamed.copy".into()), Some("TreeA".into())),
        );
        assert_eq!(steps(&editor), base + 3);
        // A part with something under it asks first; nothing goes until it is confirmed.
        editor.apply_tree_out(
            &ctx,
            tree::Out::Delete(vec![Selection::Part("TreeB".into())]),
        );
        assert!(matches!(editor.dialog, Some(Dialog::ConfirmDelete(_, 2))));
        assert_eq!(steps(&editor), base + 3);
        assert_eq!(model(&editor), (false, true));
        editor.delete_items(&ctx, &[Selection::Part("TreeB".into())]);
        assert_eq!(steps(&editor), base + 4);
        assert!(editor.view.selection.is_none());
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert_eq!(d.name_of(base + 3).as_deref(), Some("tree_delete: TreeB"));
        // One undo brings the part and its mesh back.
        assert!(d.undo());
        assert!(
            d.model().parts().find("TreeB").is_some()
                && d.model().meshes().find("Renamed").is_some()
        );
        assert!(d.model().validate().is_empty());
        // A mesh goes at once, without asking.
        editor.dialog = None;
        editor.apply_tree_out(
            &ctx,
            tree::Out::Delete(vec![Selection::Mesh("Renamed.copy".into())]),
        );
        assert!(editor.dialog.is_none());
        assert_eq!(steps(&editor), base + 4);
    }

    #[test]
    fn the_panel_actions_bring_a_panel_to_the_front_in_this_workspace_only() {
        use crate::workspace::PANELS;
        let ctx = Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |_| {});
        out.textures_delta.clear();
        let mut editor = Editor::new(None);
        for id in PANELS {
            assert!(editor.run_action(&ctx, &format!("panel.show.{id}")), "{id}");
            let layout = editor.layouts.current(editor.workspace);
            assert!(layout.is_visible(id), "{id} shows after its action");
        }
        // A tab group shows one tab at a time: selecting one hides the other.
        editor.run_action(&ctx, "panel.show.tree");
        editor.run_action(&ctx, "panel.show.deformers");
        let layout = editor.layouts.current(editor.workspace);
        assert!(layout.is_visible("deformers") && !layout.is_visible("tree"));
        // The menu says so too.
        let entries = editor.menu_entries(&ctx);
        let checked = |action: &str| {
            fn find(entries: &[menu::Entry], action: &str) -> Option<Option<bool>> {
                entries.iter().find_map(|e| match e {
                    menu::Entry::Item {
                        action: a, checked, ..
                    } if a == action => Some(*checked),
                    menu::Entry::Sub { entries, .. } => find(entries, action),
                    _ => None,
                })
            }
            find(&entries, action).flatten()
        };
        assert_eq!(checked("panel.show.deformers"), Some(true));
        assert_eq!(checked("panel.show.tree"), Some(false));
        // The other workspace keeps its own layout.
        let other = Workspace::ALL
            .iter()
            .copied()
            .find(|w| *w != editor.workspace)
            .unwrap();
        let before = editor.layouts.current(other).clone();
        editor.run_action(&ctx, "panel.show.history");
        assert_eq!(*editor.layouts.current(other), before);
        // A layout that lacks the panel gets it in the right strip.
        let layout = editor.layouts.current_mut(editor.workspace);
        layout.remove("palette");
        assert!(!layout.contains("palette"));
        editor.run_action(&ctx, "panel.show.palette");
        let layout = editor.layouts.current(editor.workspace);
        assert!(
            layout.right.panels.contains(&"palette".to_owned()) && layout.is_visible("palette")
        );
        // An unknown panel is not an action.
        assert!(!editor.run_action(&ctx, "panel.show.nope"));
    }

    #[test]
    fn the_tab_style_is_chosen_from_the_menu_and_kept_in_the_settings() {
        let dir = scratch("tab-style");
        let ctx = Context::default();
        let mut editor = Editor::new(Some(dir.clone()));
        assert_eq!(editor.tab_style, TabStyle::Compact);
        assert!(editor.run_action(&ctx, "view.tab-style.full"));
        assert_eq!(editor.tab_style, TabStyle::Full);
        // A new editor reads it back.
        assert_eq!(Editor::new(Some(dir.clone())).tab_style, TabStyle::Full);
        assert!(editor.run_action(&ctx, "view.tab-style.compact"));
        assert_eq!(Editor::new(Some(dir.clone())).tab_style, TabStyle::Compact);
        // A damaged setting gives the default.
        crate::settings::write(&dir, STATE_FILE, r#"{"tab_style":"wide"}"#).unwrap();
        assert_eq!(Editor::new(Some(dir.clone())).tab_style, TabStyle::Compact);
        assert!(!editor.run_action(&ctx, "view.tab-style.wide"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_tree_state_is_saved_per_workspace_and_comes_back() {
        use crate::tree_model::{Kind, Sort};
        let dir = scratch("tree-state");
        let ctx = Context::default();
        let mut editor = Editor::new(Some(dir.clone()));
        editor.view.parts.filter = "eye".into();
        editor.view.parts.sort = Sort::Name;
        editor.view.deformers.toggle_kind(Kind::Rotation);
        editor.save_trees(&ctx);
        // The other workspace has its own state, starting from the default.
        let other = Workspace::ALL
            .iter()
            .copied()
            .find(|w| *w != editor.workspace)
            .unwrap();
        let first = editor.workspace;
        editor.switch_workspace(other);
        assert_eq!(editor.view.parts.filter, "");
        editor.view.parts.hidden_only = true;
        editor.save_trees(&ctx);
        editor.switch_workspace(first);
        assert_eq!(editor.view.parts.filter, "eye");
        assert!(!editor.view.parts.hidden_only);
        // A new editor reads both back.
        let mut again = Editor::new(Some(dir.clone()));
        assert_eq!(again.workspace, first);
        assert_eq!(again.view.parts.filter, "eye");
        assert_eq!(again.view.parts.sort, Sort::Name);
        assert!(again.view.deformers.kinds.contains(&Kind::Rotation));
        again.switch_workspace(other);
        assert!(again.view.parts.hidden_only && again.view.parts.filter.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn closed_folders_survive_opening_a_project_and_forget_names_that_are_gone() {
        let dir = scratch("tree-folders");
        let ctx = Context::default();
        let mut editor = open_minimal();
        editor.settings_dir = Some(dir.clone());
        // `Fold` is a part with a part in it; `Gone` does not exist.
        for (name, parent) in [("Fold", None), ("Inside", Some("Fold"))] {
            assert!(editor.submit(
                &ctx,
                tsukumoe_core::command::Command::AddPart {
                    name: name.into(),
                    parent: parent.map(Into::into),
                }
            ));
        }
        editor.view.parts.set_open("p:Fold", false);
        editor.view.parts.set_open("p:Gone", false);
        editor.save_trees(&ctx);
        assert!(editor.view.parts.collapsed.contains("p:Fold"));
        assert!(!editor.view.parts.collapsed.contains("p:Gone"));
        // Reopening the project keeps what was closed.
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tsukumoe-format/tests/fixtures/minimal");
        // (The reopened project has no `Fold`, so the closed state goes with it.)
        editor.view.parts.set_open("p:Face", false);
        editor.open(&path).unwrap();
        assert!(editor.view.parts.collapsed.contains("p:Face"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_damaged_tree_file_gives_the_defaults_without_failing() {
        let dir = scratch("tree-damaged");
        for text in [
            "",
            "not json",
            "[1,2]",
            r#"{"modeling":7,"animation":{"parts":{"sort":"x"}}}"#,
        ] {
            crate::settings::write(&dir, TREE_FILE, text).unwrap();
            let editor = Editor::new(Some(dir.clone()));
            assert!(editor.view.parts.collapsed.is_empty() && editor.view.parts.filter.is_empty());
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn reset_puts_the_selected_deformers_back_in_one_step_and_says_when_there_is_nothing_to_do() {
        use tsukumoe_core::command::{Command, RotationBase};
        let mut editor = open_minimal();
        let ctx = Context::default();
        for command in [
            Command::AddPart {
                name: "ResetPart".into(),
                parent: None,
            },
            Command::AddWarpDeformer {
                name: "ResetWarp".into(),
                part: "ResetPart".into(),
                parent: None,
                rows: 1,
                columns: 1,
                base_points: vec![[0.0, 0.0], [2.0, 0.0], [0.0, 2.0], [2.0, 2.0]],
            },
            // Bent after it was made: a reset goes back to the made grid.
            Command::SetWarpBasePoints {
                name: "ResetWarp".into(),
                points: vec![[0.0, 0.0], [2.0, 0.3], [0.0, 2.0], [2.0, 2.0]],
            },
            Command::AddRotationDeformer {
                name: "ResetTurn".into(),
                part: "ResetPart".into(),
                parent: None,
                base: RotationBase {
                    origin: [1.0, 1.0],
                    angle_degrees: 25.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
        ] {
            assert!(editor.submit(&ctx, command));
        }
        let steps = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.log().len();
        let base = steps(&editor);
        // Nothing selected: a message, no step.
        assert!(editor.run_action(&ctx, "deformer.reset"));
        assert_eq!(steps(&editor), base);
        // Both selected: one step.
        editor.view.select_all(vec![
            Selection::Warp("ResetWarp".into()),
            Selection::Rotation("ResetTurn".into()),
        ]);
        assert!(editor.run_action(&ctx, "deformer.reset"));
        assert_eq!(steps(&editor), base + 1);
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert_eq!(d.name_of(base).as_deref(), Some("deformer_reset: 2 items"));
        let turn = d
            .model()
            .rotations()
            .get(d.model().rotations().find("ResetTurn").unwrap())
            .unwrap();
        assert_eq!(
            (turn.base.angle_degrees, turn.base.origin),
            (0.0, [1.0, 1.0])
        );
        let warp = d
            .model()
            .warps()
            .get(d.model().warps().find("ResetWarp").unwrap())
            .unwrap();
        assert_eq!(warp.base_points[1], [2.0, 0.0]);
        // Reset again: already initial, so nothing happens.
        assert!(editor.run_action(&ctx, "deformer.reset"));
        assert_eq!(steps(&editor), base + 1);
        // The tree's menu asks the same way.
        let d = &mut editor.doc.as_mut().unwrap().dispatcher;
        assert!(d.undo());
        editor.apply_tree_out(
            &ctx,
            tree::Out::Reset(vec![Selection::Warp("ResetWarp".into())]),
        );
        assert_eq!(steps(&editor), base + 1);
        let d = &editor.doc.as_ref().unwrap().dispatcher;
        assert_eq!(
            d.name_of(base).as_deref(),
            Some("deformer_reset: ResetWarp")
        );
    }

    #[test]
    fn moving_an_axis_is_one_named_step_that_keeps_the_children_and_a_failure_says_why() {
        use tsukumoe_core::command::{Command, RotationBase};
        let mut editor = open_minimal();
        let ctx = Context::default();
        for command in [
            Command::AddPart {
                name: "AxisPart".into(),
                parent: None,
            },
            Command::AddRotationDeformer {
                name: "AxisRot".into(),
                part: "AxisPart".into(),
                parent: None,
                base: RotationBase {
                    origin: [1.0, 1.0],
                    angle_degrees: 30.0,
                    scale: 1.0,
                    reflect_x: false,
                    reflect_y: false,
                },
            },
            Command::AddMesh {
                name: "AxisMesh".into(),
                part: "AxisPart".into(),
                positions: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
                uvs: vec![[0.0, 0.0]; 3],
                indices: vec![0, 1, 2],
            },
            Command::SetMeshParent {
                name: "AxisMesh".into(),
                parent: Some("AxisRot".into()),
            },
        ] {
            assert!(editor.submit(&ctx, command));
        }
        let steps = |e: &Editor| e.doc.as_ref().unwrap().dispatcher.log().len();
        let base = steps(&editor);
        let drawn = |e: &Editor| {
            let m = e.doc.as_ref().unwrap().dispatcher.model();
            let mesh = m
                .meshes()
                .get(m.meshes().find("AxisMesh").unwrap())
                .unwrap();
            let space = tsukumoe_core::deformer_space::Space::of(m, mesh.parent).unwrap();
            mesh.positions
                .iter()
                .map(|&p| space.point_to_world(p))
                .collect::<Vec<_>>()
        };
        let before = drawn(&editor);
        editor.move_axis(&ctx, "AxisRot", [3.0, 0.5]);
        assert_eq!(steps(&editor), base + 1);
        let d = &editor.doc.as_ref().unwrap().dispatcher;
        assert_eq!(
            d.name_of(base).as_deref(),
            Some("deformer_move_axis: AxisRot")
        );
        for (a, b) in before.iter().zip(drawn(&editor)) {
            assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3);
        }
        // An unknown rotation: a message, no step.
        editor.move_axis(&ctx, "Nope", [0.0, 0.0]);
        assert_eq!(steps(&editor), base + 1);
    }

    #[test]
    fn every_menu_action_is_a_known_action() {
        let ctx = Context::default();
        let mut editor = open_minimal();
        for action in crate::mesh_tool::MENU_ACTIONS {
            assert!(editor.run_action(&ctx, action), "{action}");
        }
        for action in crate::deformer_tool::MENU_ACTIONS {
            assert!(actions::ACTIONS.contains(&action), "{action}");
            assert!(editor.run_action(&ctx, action), "{action}");
        }
    }

    #[test]
    fn a_shortcut_runs_its_action_and_tools_switch() {
        use tsukumoe_core::command::Command;
        let mut editor = open_minimal();
        let ctx = Context::default();
        assert!(editor.submit(
            &ctx,
            Command::AddPart {
                name: "P".into(),
                parent: None
            }
        ));
        let mut h = harness(editor);
        h.run();
        h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
        h.run();
        assert!(
            !h.state().doc.as_ref().unwrap().dispatcher.can_undo(),
            "Mod+Z undid it"
        );
        h.key_press(egui::Key::M);
        h.run();
        assert_eq!(h.state().view.tool, Tool::Mesh);
        h.state_mut().run_action(&ctx, "nonsense.action");
    }

    fn frame(ctx: &Context, events: Vec<egui::Event>, mut f: impl FnMut(&Context)) {
        let input = egui::RawInput {
            events,
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| f(ui.ctx()));
        out.textures_delta.clear();
    }

    #[test]
    fn the_pie_menu_opens_on_its_key_and_releasing_toward_an_item_runs_it() {
        let ctx = Context::default();
        let mut editor = Editor::new(None);
        editor.pie_config = PieConfig::from_json(
            r#"{"items":[{"action":"tool.mesh-edit"},{"action":"tool.deformer"},{"action":"tool.select"},{"action":"edit.redo"}]}"#,
        );
        let at = egui::pos2(400.0, 400.0);
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        assert!(editor.pie.is_some(), "open while the key is held");
        // release straight up: the first item
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Q,
            physical_key: Some(egui::Key::Q),
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(at + egui::vec2(0.0, -60.0)),
                key(false),
            ],
            |ctx| {
                editor.pie_frame(ctx);
            },
        );
        assert!(editor.pie.is_none());
        assert_eq!(editor.view.tool, Tool::Mesh);
        // releasing at the center cancels without running anything
        editor.view.tool = Tool::Select;
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        frame(&ctx, vec![key(false)], |ctx| editor.pie_frame(ctx));
        assert!(editor.pie.is_none());
        assert_eq!(editor.view.tool, Tool::Select);
    }

    #[test]
    fn a_repeated_key_press_does_not_move_the_open_pie_menu() {
        let ctx = Context::default();
        let mut editor = Editor::new(None);
        editor.pie_config = PieConfig::from_json(
            r#"{"items":[{"action":"tool.mesh-edit"},{"action":"tool.deformer"},{"action":"tool.select"},{"action":"edit.redo"}]}"#,
        );
        let at = egui::pos2(400.0, 400.0);
        frame(&ctx, vec![egui::Event::PointerMoved(at)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        // the key repeats while the pointer is elsewhere
        let away = at + egui::vec2(300.0, 0.0);
        frame(&ctx, vec![egui::Event::PointerMoved(away)], |ctx| {
            editor.run_action(ctx, "view.pie-menu");
            editor.pie_frame(ctx);
        });
        // straight up from where it first opened still picks the first item
        let release = egui::Event::Key {
            key: egui::Key::Q,
            physical_key: Some(egui::Key::Q),
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(at + egui::vec2(0.0, -60.0)),
                release,
            ],
            |ctx| editor.pie_frame(ctx),
        );
        assert_eq!(editor.view.tool, Tool::Mesh);
    }

    #[test]
    fn user_key_assignments_are_saved_and_restored() {
        let dir = scratch("keymap");
        let ctx = Context::default();
        let mut editor = Editor::new(Some(dir.clone()));
        editor.keymap.bind("edit.undo", None, "Mod+Y").unwrap();
        editor.save_keymap(&ctx);
        let again = Editor::new(Some(dir.clone()));
        assert_eq!(
            again.keymap.keys_text("edit.undo", None).as_deref(),
            Some("Mod+Y")
        );
        // a damaged file falls back to the preset
        settings::write(&dir, KEYMAP_FILE, "{").unwrap();
        let broken = Editor::new(Some(dir.clone()));
        assert_eq!(
            broken.keymap.keys_text("edit.undo", None).as_deref(),
            Some("Mod+Z")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn opening_a_missing_project_reports_and_keeps_going() {
        let mut editor = Editor::new(None);
        assert!(editor.open(Path::new("/no/such")).is_err());
        assert!(editor.doc.is_none() && editor.recent.paths().is_empty());
    }

    fn fixture() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tsukumoe-format/tests/fixtures/minimal")
    }

    fn with_fake(settings: Option<PathBuf>) -> (Editor, crate::picker::Fake, Context) {
        let mut editor = Editor::new(settings);
        let fake = crate::picker::Fake::default();
        editor.picker = Box::new(fake.clone());
        (editor, fake, Context::default())
    }

    fn run(editor: &mut Editor, ctx: &Context, action: &str) {
        assert!(editor.run_action(ctx, action));
        editor.process_pending(ctx, None);
    }

    #[test]
    fn a_cancelled_dialog_does_nothing() {
        let (mut editor, fake, ctx) = with_fake(None);
        for action in [
            "file.new",
            "file.new-from-psd",
            "file.open",
            "file.open-folder",
        ] {
            run(&mut editor, &ctx, action);
        }
        assert!(editor.doc.is_none() && editor.dialog.is_none());
        assert!(editor.last_dir.is_none());
        assert_eq!(
            fake.0.borrow().requests.len(),
            4,
            "new-from-psd stops after the PSD"
        );
    }

    #[test]
    fn new_project_creates_opens_and_remembers_the_folder() {
        let dir = scratch("newproj");
        std::fs::create_dir_all(&dir).unwrap();
        let (mut editor, fake, ctx) = with_fake(Some(dir.join("settings")));
        fake.answer(dir.join("p"));
        run(&mut editor, &ctx, "file.new");
        let doc = editor.doc.as_ref().unwrap();
        assert!(doc.dispatcher.model().meshes().is_empty());
        assert!(dir.join("p/manifest.json").is_file());
        assert_eq!(
            fake.0.borrow().requests[0].kind,
            crate::picker::PickKind::NewFolder
        );
        assert_eq!(editor.last_dir.as_deref(), Some(dir.join("p").as_path()));
        // The next dialog starts there, also after a restart.
        fake.answer(dir.join("p2"));
        run(&mut editor, &ctx, "file.new");
        assert_eq!(
            fake.0.borrow().requests[1].start_dir.as_deref(),
            Some(dir.join("p").as_path())
        );
        let again = Editor::new(Some(dir.join("settings")));
        assert_eq!(again.last_dir, editor.last_dir);
    }

    #[test]
    fn a_folder_that_is_not_empty_is_refused_and_nothing_changes() {
        let dir = scratch("newfull");
        std::fs::create_dir_all(dir.join("full")).unwrap();
        std::fs::write(dir.join("full/keep.txt"), b"x").unwrap();
        let (mut editor, fake, ctx) = with_fake(None);
        editor.open(&fixture()).unwrap();
        fake.answer(dir.join("full"));
        run(&mut editor, &ctx, "file.new");
        assert_eq!(editor.doc.as_ref().unwrap().name(), "minimal");
        assert_eq!(std::fs::read_dir(dir.join("full")).unwrap().count(), 1);
    }

    #[test]
    fn open_save_and_import_use_the_picked_paths() {
        let dir = scratch("picked");
        std::fs::create_dir_all(&dir).unwrap();
        let (mut editor, fake, ctx) = with_fake(None);
        fake.answer(fixture());
        run(&mut editor, &ctx, "file.open-folder");
        assert_eq!(editor.doc.as_ref().unwrap().name(), "minimal");
        assert_eq!(
            fake.0.borrow().requests[0].kind,
            crate::picker::PickKind::OpenFolder
        );
        // Save as: the archive gets its extension; as folder: a folder even with a dot.
        fake.answer(dir.join("saved"));
        run(&mut editor, &ctx, "file.save-as");
        assert!(dir.join("saved.tsukumoe").is_file());
        fake.answer(dir.join("f.v1"));
        run(&mut editor, &ctx, "file.save-as-folder");
        assert!(dir.join("f.v1/manifest.json").is_file());
        // Import PSD: the dialog opens on the picked file.
        fake.answer(dir.join("x.psd"));
        run(&mut editor, &ctx, "file.import-psd");
        let Some(Dialog::PsdImport(state)) = &editor.dialog else {
            panic!("no import dialog");
        };
        assert!(state.path.ends_with("x.psd"));
        // Saving needs a project.
        let (mut none, fake, ctx) = with_fake(None);
        fake.answer(dir.join("y"));
        run(&mut none, &ctx, "file.save-as");
        run(&mut none, &ctx, "file.import-psd");
        assert!(fake.0.borrow().requests.is_empty());
    }

    fn psd_file(dir: &Path) -> PathBuf {
        use tsukumoe_psd::writer::{LayerSpec, Node, PsdSpec, write};
        let layer = Node::Layer(LayerSpec::solid("bg", 0, 0, 8, 8, [1, 2, 3, 255]));
        let file = dir.join("a.psd");
        std::fs::write(&file, write(&PsdSpec::new(8, 8, vec![layer]))).unwrap();
        file
    }

    #[test]
    fn a_new_project_from_a_psd_imports_in_one_step_or_leaves_nothing() {
        let dir = scratch("fromps");
        std::fs::create_dir_all(&dir).unwrap();
        let psd = psd_file(&dir);
        let (mut editor, fake, ctx) = with_fake(None);
        editor.open(&fixture()).unwrap();

        // Confirmed: one undo reverts the import; the project folder stays.
        fake.answer(&psd);
        fake.answer(dir.join("p"));
        run(&mut editor, &ctx, "file.new-from-psd");
        let Some(Dialog::PsdImport(mut state)) = editor.dialog.take() else {
            panic!("no import dialog");
        };
        state.load_blocking(editor.doc.as_ref().unwrap());
        state.confirm(editor.doc.as_mut().unwrap()).unwrap();
        editor.psd_dialog_closed(&state);
        let doc = editor.doc.as_mut().unwrap();
        assert_eq!(doc.dispatcher.model().meshes().len(), 1);
        assert!(doc.dispatcher.undo());
        assert!(doc.dispatcher.model().meshes().is_empty());
        assert!(editor.stash.is_none() && dir.join("p/manifest.json").is_file());

        // Cancelled: the folder is gone and the earlier project is back.
        let (mut editor, fake, _) = with_fake(None);
        editor.open(&fixture()).unwrap();
        fake.answer(&psd);
        fake.answer(dir.join("q"));
        run(&mut editor, &ctx, "file.new-from-psd");
        assert!(dir.join("q").is_dir());
        let Some(Dialog::PsdImport(state)) = editor.dialog.take() else {
            panic!("no import dialog");
        };
        editor.psd_dialog_closed(&state);
        assert!(!dir.join("q").exists());
        assert_eq!(editor.doc.as_ref().unwrap().name(), "minimal");
    }
}
