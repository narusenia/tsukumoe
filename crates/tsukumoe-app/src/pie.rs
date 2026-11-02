// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! What the pie menu holds: keymap actions, one group level at most. The
//! assignment is data the user can change and save; the call key is the
//! `view.pie-menu` action of the keymap.
//!
//! PLACEHOLDER: the initial slots and the call key (hold Q) are not decided
//! (open item in the plan); the maintainer confirms them.

use serde::{Deserialize, Serialize};
use tsukumoe_ui::{PieItem, tr};

use crate::actions::{self, ACTIONS};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Entry {
    Action {
        action: String,
    },
    /// A submenu; `label` is a translation key.
    Group {
        label: String,
        items: Vec<Entry>,
    },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct PieConfig {
    pub items: Vec<Entry>,
    /// Slots of their own for a tool (`select`, `mesh`, `deformer`); a tool
    /// with no entry uses `items`.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub tools: std::collections::BTreeMap<String, Vec<Entry>>,
}

fn action(name: &str) -> Entry {
    Entry::Action {
        action: name.to_owned(),
    }
}

impl Default for PieConfig {
    fn default() -> Self {
        Self {
            tools: Default::default(),
            items: vec![
                action("tool.select"),
                action("edit.undo"),
                action("tool.mesh-edit"),
                action("view.palette"),
                action("tool.deformer"),
                action("edit.redo"),
                Entry::Group {
                    label: "pie-group-view".to_owned(),
                    items: vec![
                        action("viewport.zoom-in"),
                        action("viewport.zoom-out"),
                        action("viewport.reset-camera"),
                    ],
                },
            ],
        }
    }
}

impl PieConfig {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("pie config serializes")
    }

    /// A damaged file, or one with no usable slot, gives the default.
    pub fn from_json(json: &str) -> Self {
        serde_json::from_str::<Self>(json)
            .ok()
            .filter(|c| !c.build_items(&|k| k.to_owned()).is_empty())
            .unwrap_or_default()
    }

    /// The menu items. Actions the editor does not know (a hand-edited file)
    /// are dropped, as are groups that end up empty; groups are one level deep.
    pub fn build_items(&self, text: &dyn Fn(&str) -> String) -> Vec<PieItem> {
        self.build_entries(&self.items, text)
    }

    /// The slots a tool shows: its own when it has them, else the common ones.
    pub fn entries_for(&self, tool: &str) -> &[Entry] {
        self.tools.get(tool).map_or(&self.items, Vec::as_slice)
    }

    fn build_entries(&self, entries: &[Entry], text: &dyn Fn(&str) -> String) -> Vec<PieItem> {
        fn build(entries: &[Entry], text: &dyn Fn(&str) -> String, depth: usize) -> Vec<PieItem> {
            entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Action { action } => ACTIONS
                        .contains(&action.as_str())
                        .then(|| PieItem::new(action, &text(&actions::label_key(action)))),
                    Entry::Group { label, items } if depth == 0 => {
                        let children = build(items, text, depth + 1);
                        (!children.is_empty())
                            .then(|| PieItem::new(label, &text(label)).with_children(children))
                    }
                    Entry::Group { .. } => None,
                })
                .collect()
        }
        build(entries, text, 0)
    }

    /// The menu to open while `tool` is the active tool.
    pub fn items(&self, ctx: &egui::Context, tool: &str) -> Vec<PieItem> {
        let items = self.build_entries(self.entries_for(tool), &|key| tr(ctx, key));
        if items.is_empty() {
            self.build_items(&|key| tr(ctx, key))
        } else {
            items
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tool_with_its_own_slots_uses_them_and_others_use_the_common_ones() {
        let mut c = PieConfig::default();
        c.tools.insert("mesh".into(), vec![action("edit.undo")]);
        assert_eq!(c.entries_for("mesh"), [action("edit.undo")]);
        assert_eq!(c.entries_for("select"), c.items.as_slice());
        // An old file without `tools` still loads, and `tools` round-trips.
        assert_eq!(PieConfig::from_json(&c.to_json()), c);
        let old = PieConfig::from_json(r#"{"items":[{"action":"edit.undo"}]}"#);
        assert!(old.tools.is_empty());
    }

    use super::*;

    fn ids(items: &[PieItem]) -> Vec<&str> {
        items.iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn the_default_has_actions_and_one_group() {
        let items = PieConfig::default().build_items(&|k| k.to_owned());
        assert_eq!(items.len(), 7);
        assert_eq!(items[0].id, "tool.select");
        assert_eq!(
            ids(&items[6].children),
            [
                "viewport.zoom-in",
                "viewport.zoom-out",
                "viewport.reset-camera"
            ]
        );
    }

    #[test]
    fn unknown_actions_and_empty_or_nested_groups_are_dropped() {
        let json = r#"{"items":[
            {"action":"edit.undo"},{"action":"bogus"},
            {"label":"g","items":[{"action":"bogus"}]},
            {"label":"h","items":[{"action":"tool.mesh-edit"},{"label":"deep","items":[{"action":"edit.redo"}]}]}]}"#;
        let items = PieConfig::from_json(json).build_items(&|k| k.to_owned());
        assert_eq!(ids(&items), ["edit.undo", "h"]);
        assert_eq!(ids(&items[1].children), ["tool.mesh-edit"]);
    }

    #[test]
    fn it_round_trips_and_damage_gives_the_default() {
        let c = PieConfig {
            items: vec![action("edit.undo"), action("edit.redo")],
            tools: Default::default(),
        };
        assert_eq!(PieConfig::from_json(&c.to_json()), c);
        for bad in [
            "{",
            "[]",
            r#"{"items":[]}"#,
            r#"{"items":[{"action":"bogus"}]}"#,
        ] {
            assert_eq!(PieConfig::from_json(bad), PieConfig::default(), "{bad}");
        }
    }
}
