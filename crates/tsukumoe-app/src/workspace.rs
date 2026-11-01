// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Workspaces, their dock layouts and the layout presets. Presets are JSON
//! data files (`layouts/*.json`) embedded at build time, not code.

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tsukumoe_ui::dock::Layout;

/// The standard panels, by id. Every layout must stay within this set.
pub const PANELS: [&str; 8] = [
    "viewport",
    "tree",
    "deformers",
    "parameters",
    "inspector",
    "history",
    "palette",
    "ai_diff",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Workspace {
    #[default]
    Modeling,
    Animation,
}

impl Workspace {
    pub const ALL: [Self; 2] = [Self::Modeling, Self::Animation];

    pub fn key(self) -> &'static str {
        match self {
            Self::Modeling => "modeling",
            Self::Animation => "animation",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Tsukumoe,
    Cubism,
}

impl Preset {
    pub const ALL: [Self; 2] = [Self::Tsukumoe, Self::Cubism];

    pub fn key(self) -> &'static str {
        match self {
            Self::Tsukumoe => "tsukumoe",
            Self::Cubism => "cubism",
        }
    }
}

fn preset_source(preset: Preset, workspace: Workspace) -> &'static str {
    match (preset, workspace) {
        (Preset::Tsukumoe, Workspace::Modeling) => {
            include_str!("../layouts/tsukumoe_modeling.json")
        }
        (Preset::Tsukumoe, Workspace::Animation) => {
            include_str!("../layouts/tsukumoe_animation.json")
        }
        (Preset::Cubism, Workspace::Modeling) => include_str!("../layouts/cubism_modeling.json"),
        (Preset::Cubism, Workspace::Animation) => include_str!("../layouts/cubism_animation.json"),
    }
}

/// The shipped layout; a test keeps the files valid, so this cannot fail at run time.
pub fn builtin_layout(preset: Preset, workspace: Workspace) -> Layout {
    Layout::from_json(preset_source(preset, workspace), &PANELS)
        .expect("built-in layouts are valid (tested)")
}

/// A layout the user named and saved, for one workspace.
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Saved {
    pub name: String,
    pub workspace: Workspace,
    pub layout: Layout,
}

/// The current layout of each workspace and the user's named layouts.
#[derive(Clone, Debug, PartialEq)]
pub struct Layouts {
    current: [Layout; 2],
    pub saved: Vec<Saved>,
}

impl Default for Layouts {
    fn default() -> Self {
        Self {
            current: Workspace::ALL.map(|w| builtin_layout(Preset::Tsukumoe, w)),
            saved: Vec::new(),
        }
    }
}

/// A layout that lacks a standard panel gets it back in the right strip, so a
/// panel can never be lost.
fn with_all_panels(mut layout: Layout) -> Layout {
    let missing: Vec<&str> = PANELS.into_iter().filter(|p| !layout.contains(p)).collect();
    layout
        .right
        .panels
        .extend(missing.into_iter().map(str::to_owned));
    layout
}

impl Layouts {
    pub fn current(&self, workspace: Workspace) -> &Layout {
        &self.current[workspace.index()]
    }

    pub fn current_mut(&mut self, workspace: Workspace) -> &mut Layout {
        &mut self.current[workspace.index()]
    }

    /// Replaces the workspace's layout with the preset's.
    pub fn apply_preset(&mut self, workspace: Workspace, preset: Preset) {
        self.current[workspace.index()] = builtin_layout(preset, workspace);
    }

    /// Saves the workspace's current layout under `name`, replacing a saved
    /// layout of the same name and workspace. Empty names are refused.
    pub fn save_named(&mut self, workspace: Workspace, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() {
            return false;
        }
        let layout = self.current(workspace).clone();
        self.saved
            .retain(|s| !(s.workspace == workspace && s.name == name));
        self.saved.push(Saved {
            name: name.to_owned(),
            workspace,
            layout,
        });
        true
    }

    pub fn load_named(&mut self, workspace: Workspace, name: &str) -> bool {
        let Some(saved) = self
            .saved
            .iter()
            .find(|s| s.workspace == workspace && s.name == name)
        else {
            return false;
        };
        self.current[workspace.index()] = saved.layout.clone();
        true
    }

    pub fn to_json(&self) -> String {
        let value = json!({
            "current": {
                "modeling": self.current(Workspace::Modeling),
                "animation": self.current(Workspace::Animation),
            },
            "saved": self.saved,
        });
        serde_json::to_string_pretty(&value).expect("layouts serialize")
    }

    /// Reads saved layouts. Each part is checked on its own: a damaged or
    /// unknown layout falls back to the default of its workspace, a damaged
    /// named layout is dropped, and nothing here can fail.
    pub fn from_json(json: &str) -> Self {
        let mut layouts = Self::default();
        let Ok(root) = serde_json::from_str::<Value>(json) else {
            return layouts;
        };
        let layout_of = |value: &Value| {
            Layout::from_json(&value.to_string(), &PANELS)
                .ok()
                .map(with_all_panels)
        };
        for w in Workspace::ALL {
            if let Some(layout) = root["current"].get(w.key()).and_then(layout_of) {
                layouts.current[w.index()] = layout;
            }
        }
        for entry in root["saved"].as_array().into_iter().flatten() {
            let Some(layout) = entry.get("layout").and_then(layout_of) else {
                continue;
            };
            let workspace = serde_json::from_value(entry["workspace"].clone());
            let name = entry["name"].as_str().filter(|n| !n.trim().is_empty());
            if let (Ok(workspace), Some(name)) = (workspace, name) {
                layouts.saved.push(Saved {
                    name: name.to_owned(),
                    workspace,
                    layout,
                });
            }
        }
        layouts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_ui::dock::{Side, Target};

    #[test]
    fn every_preset_loads_and_has_every_standard_panel_once() {
        for preset in Preset::ALL {
            for w in Workspace::ALL {
                let layout = builtin_layout(preset, w);
                let mut panels = layout.panels();
                panels.sort_unstable();
                let mut all = PANELS.to_vec();
                all.sort_unstable();
                assert_eq!(panels, all, "{} {}", preset.key(), w.key());
            }
        }
    }

    #[test]
    fn switching_presets_back_and_forth_keeps_layouts_valid() {
        let mut l = Layouts::default();
        for _ in 0..2 {
            for preset in [Preset::Cubism, Preset::Tsukumoe] {
                l.apply_preset(Workspace::Modeling, preset);
                assert_eq!(
                    l.current(Workspace::Modeling),
                    &builtin_layout(preset, Workspace::Modeling)
                );
            }
        }
    }

    #[test]
    fn workspaces_keep_independent_layouts_through_json() {
        let mut l = Layouts::default();
        l.current_mut(Workspace::Animation)
            .move_panel("history", Target::Strip(Side::Left));
        assert!(l.current(Workspace::Modeling).left.panels.is_empty());
        let again = Layouts::from_json(&l.to_json());
        assert_eq!(again, l);
        assert_eq!(again.current(Workspace::Animation).left.panels, ["history"]);
    }

    #[test]
    fn named_layouts_round_trip() {
        let mut l = Layouts::default();
        l.current_mut(Workspace::Modeling)
            .move_panel("tree", Target::Strip(Side::Left));
        assert!(l.save_named(Workspace::Modeling, " mine "));
        assert!(!l.save_named(Workspace::Modeling, "  "));
        l.apply_preset(Workspace::Modeling, Preset::Cubism);
        assert!(l.load_named(Workspace::Modeling, "mine"));
        assert_eq!(l.current(Workspace::Modeling).left.panels, ["tree"]);
        assert!(!l.load_named(Workspace::Animation, "mine"));
        assert_eq!(Layouts::from_json(&l.to_json()).saved, l.saved);
        // saving the same name again replaces it
        l.save_named(Workspace::Modeling, "mine");
        assert_eq!(l.saved.len(), 1);
    }

    #[test]
    fn damaged_files_fall_back_part_by_part() {
        let d = Layouts::default();
        assert_eq!(Layouts::from_json("garbage"), d);
        assert_eq!(Layouts::from_json("[]"), d);
        let bad = r#"{"current":{"modeling":{"root":{"type":"tabs","tabs":["nope"]}},"animation":3},
                      "saved":[{"name":"x","workspace":"modeling","layout":{"root":1}},{"name":"","workspace":"modeling","layout":{"root":{"type":"tabs","tabs":["tree"]}}}]}"#;
        let l = Layouts::from_json(bad);
        assert_eq!(
            l.current(Workspace::Modeling),
            d.current(Workspace::Modeling)
        );
        assert!(l.saved.is_empty());
    }

    #[test]
    fn a_layout_missing_panels_gets_them_back_in_the_right_strip() {
        let json = r#"{"current":{"modeling":{"root":{"type":"tabs","tabs":["viewport"]}}}}"#;
        let l = Layouts::from_json(json);
        let layout = l.current(Workspace::Modeling);
        assert_eq!(layout.panels().len(), PANELS.len());
        assert!(layout.right.panels.contains(&"tree".to_owned()));
    }

    #[test]
    fn both_presets_hold_the_deformers_panel_next_to_the_parts_panel() {
        for preset in Preset::ALL {
            for workspace in Workspace::ALL {
                let layout = builtin_layout(preset, workspace);
                assert!(layout.contains("deformers"), "{preset:?} {workspace:?}");
                assert_eq!(layout.panels().len(), PANELS.len());
            }
        }
    }

    #[test]
    fn an_old_layout_without_the_deformers_panel_gets_it_in_the_right_strip() {
        // Saved before the panel existed: seven panels, none of them the new one.
        let old = r#"{"current":{"modeling":{"root":{"type":"split","axis":"horizontal","ratio":0.5,
            "a":{"type":"tabs","tabs":["viewport","tree"]},
            "b":{"type":"tabs","tabs":["parameters","inspector","history","palette","ai_diff"]}}}}}"#;
        let l = Layouts::from_json(old);
        let layout = l.current(Workspace::Modeling);
        assert!(layout.right.panels.contains(&"deformers".to_owned()));
        assert_eq!(layout.panels().len(), PANELS.len());
    }
}
