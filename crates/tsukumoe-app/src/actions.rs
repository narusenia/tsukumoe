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
pub const ACTIONS: [&str; 28] = [
    "file.open",
    "file.save",
    "edit.undo",
    "edit.redo",
    "view.palette",
    "view.pie-menu",
    "tool.select",
    "tool.mesh-edit",
    "tool.deformer",
    "mesh.delete-vertices",
    "mesh.connect",
    "mesh.flip-edge",
    "mesh.retriangulate",
    "mesh.fill-grid",
    "mesh.contour",
    "deformer.create-warp",
    "deformer.create-rotation",
    "panel.show.viewport",
    "panel.show.tree",
    "panel.show.deformers",
    "panel.show.parameters",
    "panel.show.inspector",
    "panel.show.history",
    "panel.show.palette",
    "panel.show.ai_diff",
    "viewport.zoom-in",
    "viewport.zoom-out",
    "viewport.reset-camera",
];

/// The context a binding of `action` belongs to (`None` is global).
pub fn context_of(action: &str) -> Option<&'static str> {
    (action.starts_with("viewport.")
        || action.starts_with("mesh.")
        || action.starts_with("deformer."))
    .then_some("viewport")
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
}

impl Tool {
    pub fn action(self) -> &'static str {
        match self {
            Tool::Select => "tool.select",
            Tool::Mesh => "tool.mesh-edit",
            Tool::Deformer => "tool.deformer",
        }
    }

    pub fn from_action(action: &str) -> Option<Self> {
        [Tool::Select, Tool::Mesh, Tool::Deformer]
            .into_iter()
            .find(|t| t.action() == action)
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
            if !a.starts_with("panel.show.") {
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
        for t in [Tool::Select, Tool::Mesh, Tool::Deformer] {
            assert_eq!(Tool::from_action(t.action()), Some(t));
        }
        assert_eq!(Tool::from_action("edit.undo"), None);
    }
}
