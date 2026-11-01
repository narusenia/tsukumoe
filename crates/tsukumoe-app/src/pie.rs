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
}

fn action(name: &str) -> Entry {
    Entry::Action {
        action: name.to_owned(),
    }
}

impl Default for PieConfig {
    fn default() -> Self {
        Self {
            items: vec![
                action("tool.select"),
                action("edit.undo"),
                action("tool.mesh"),
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
        build(&self.items, text, 0)
    }

    pub fn items(&self, ctx: &egui::Context) -> Vec<PieItem> {
        self.build_items(&|key| tr(ctx, key))
    }
}

#[cfg(test)]
mod tests {
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
            {"label":"h","items":[{"action":"tool.mesh"},{"label":"deep","items":[{"action":"edit.redo"}]}]}]}"#;
        let items = PieConfig::from_json(json).build_items(&|k| k.to_owned());
        assert_eq!(ids(&items), ["edit.undo", "h"]);
        assert_eq!(ids(&items[1].children), ["tool.mesh"]);
    }

    #[test]
    fn it_round_trips_and_damage_gives_the_default() {
        let c = PieConfig {
            items: vec![action("edit.undo"), action("edit.redo")],
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
