// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's actions (the names the keymap and the pie menu bind) and the
//! keymap presets, which are JSON data files.
//!
//! PLACEHOLDER: the "Cubism compatible" preset (`keymaps/cubism_compatible.json`)
//! is a short list of the obvious shortcuts (undo / redo, zoom, tool switch,
//! open / save). The keys are not confirmed against the real application;
//! the maintainer must check and correct them from their own experience
//! (the plan allows only assignments the maintainer can vouch for).

use tsukumoe_ui::Keymap;

/// Every action the editor handles, in the order the shortcut list shows them.
pub const ACTIONS: [&str; 49] = [
    "file.open",
    "file.save",
    "edit.undo",
    "edit.redo",
    "edit.repeat",
    "view.palette",
    "view.settings",
    "view.pie-menu",
    "tool.select",
    "tool.mesh-edit",
    "tool.deformer",
    "tool.bone",
    "mesh.delete-vertices",
    "mesh.connect",
    "mesh.flip-edge",
    "mesh.edge-pen",
    "mesh.delete-edge",
    "mesh.weld",
    "mesh.weld-near",
    "mesh.glue-source",
    "mesh.glue",
    "mesh.symmetry",
    "mesh.retriangulate",
    "mesh.fill-grid",
    "mesh.contour",
    "mesh.even-outline",
    "mesh.reset",
    "deformer.create-warp",
    "deformer.create-rotation",
    "deformer.reset",
    "proportional.toggle",
    "panel.show.viewport",
    "panel.show.tree",
    "panel.show.deformers",
    "panel.show.parameters",
    "panel.show.bones",
    "panel.show.keys",
    "panel.show.inspector",
    "panel.show.history",
    "panel.show.timeline",
    "panel.show.expressions",
    "panel.show.physics",
    "panel.show.model_settings",
    "panel.show.ai_diff",
    "viewport.zoom-in",
    "viewport.zoom-out",
    "viewport.reset-camera",
    "viewport.solo",
    "timeline.play-toggle",
];

/// The context a binding of `action` belongs to (`None` is global).
pub fn context_of(action: &str) -> Option<&'static str> {
    (action == "edit.repeat"
        || action.starts_with("viewport.")
        || action.starts_with("mesh.")
        || action.starts_with("deformer.")
        || action.starts_with("proportional."))
    .then_some("viewport")
}

/// The icon an action's menu entry shows, if it has one.
pub fn icon_of(action: &str) -> Option<tsukumoe_ui::Icon> {
    use tsukumoe_ui::Icon;
    Some(match action {
        "file.open" => Icon::Folder,
        "file.save" => Icon::Save,
        "edit.undo" => Icon::Undo,
        "edit.redo" => Icon::Redo,
        "edit.repeat" => Icon::Rotate,
        "view.palette" => Icon::Search,
        "view.settings" => Icon::Settings,
        "tool.select" => Icon::MousePointer,
        "tool.mesh-edit" | "mesh.edge-pen" => Icon::PenLine,
        "mesh.delete-vertices" | "mesh.delete-edge" => Icon::Trash,
        "mesh.weld" | "mesh.weld-near" | "mesh.glue-source" | "mesh.glue" => Icon::Link,
        "mesh.symmetry" => Icon::Mirror,
        "mesh.fill-grid" => Icon::Grid,
        "mesh.contour" => Icon::BoxSelect,
        "mesh.reset" | "deformer.reset" | "viewport.reset-camera" => Icon::RotateCcw,
        "viewport.zoom-in" => Icon::ZoomIn,
        "viewport.zoom-out" => Icon::ZoomOut,
        "viewport.solo" => Icon::Eye,
        "timeline.play-toggle" => Icon::Play,
        _ => return None,
    })
}

/// The translation key of an action's name: `edit.undo` -> `action-edit-undo`.
pub fn label_key(action: &str) -> String {
    format!("action-{}", action.replace('.', "-"))
}

/// The tools the viewport can switch between. Mesh editing is the first tool;
/// the others come with later units.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Mesh,
    Deformer,
    Bone,
}

impl Tool {
    pub const ALL: [Self; 4] = [Tool::Select, Tool::Mesh, Tool::Deformer, Tool::Bone];

    /// The name a pie menu setting uses for this tool.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Mesh => "mesh",
            Tool::Deformer => "deformer",
            Tool::Bone => "bone",
        }
    }

    pub fn action(self) -> &'static str {
        match self {
            Tool::Select => "tool.select",
            Tool::Mesh => "tool.mesh-edit",
            Tool::Deformer => "tool.deformer",
            Tool::Bone => "tool.bone",
        }
    }

    pub fn from_action(action: &str) -> Option<Self> {
        Tool::ALL.into_iter().find(|t| t.action() == action)
    }
}

pub fn cubism_compatible() -> Keymap {
    Keymap::from_json(include_str!("../keymaps/cubism_compatible.json"))
        .expect("the bundled keymap is valid (tested)")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_i18n::{I18n, Language};

    #[test]
    fn the_preset_loads_binds_only_known_actions_and_has_no_conflicts() {
        let json = include_str!("../keymaps/cubism_compatible.json");
        let value: serde_json::Value = serde_json::from_str(json).unwrap();
        for b in value["bindings"].as_array().unwrap() {
            let action = b["action"].as_str().unwrap();
            assert!(ACTIONS.contains(&action), "{action}");
            assert_eq!(b["context"].as_str(), context_of(action), "{action}");
        }
        let map = cubism_compatible();
        assert!(map.conflicts().is_empty());
        for a in ACTIONS {
            // Showing a panel has no default key; the menu and the keymap dialog reach it.
            // The glue actions have no Cubism key to copy.
            if !a.starts_with("panel.show.")
                && !["deformer.reset", "mesh.glue-source", "mesh.glue"].contains(&a)
            {
                assert!(map.shortcut(a).is_some(), "{a} has no default key");
            }
        }
    }

    #[test]
    fn every_action_has_a_name_in_every_language() {
        for lang in Language::ALL {
            let t = I18n::new(lang);
            for a in ACTIONS {
                let key = label_key(a);
                assert_ne!(t.tr(&key), key, "{lang:?} {key}");
            }
        }
    }

    #[test]
    fn tools_map_to_actions_and_back() {
        for t in Tool::ALL {
            assert_eq!(Tool::from_action(t.action()), Some(t));
        }
        assert_eq!(Tool::from_action("edit.undo"), None);
    }
}
