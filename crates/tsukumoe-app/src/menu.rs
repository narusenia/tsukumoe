// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The editor's menu as data. [`MENU`] lists the items by action name and label
//! key; [`resolve`] turns it, with the editor's state, into the [`Entry`] tree
//! that both the egui menu bar and the macOS native menu draw. Nothing here
//! touches a window, so it is tested on every OS.

/// One item of the menu definition.
pub enum Def {
    Item {
        action: &'static str,
        label: &'static str,
    },
    Separator,
    Sub {
        label: &'static str,
        items: &'static [Def],
    },
    /// The recent projects, one item each (a disabled note when there are none).
    Recent,
    /// The saved layouts of the current workspace, likewise.
    SavedLayouts,
}

const fn item(action: &'static str, label: &'static str) -> Def {
    Def::Item { action, label }
}

/// The menu bar: each top-level entry is a menu.
pub const MENU: &[Def] = &[
    Def::Sub {
        label: "menu-file",
        items: &[
            item("file.open", "menu-open"),
            item("file.save", "menu-save"),
            item("file.save-as", "menu-save-as"),
            item("file.import-psd", "menu-import-psd"),
            Def::Sub {
                label: "menu-recent",
                items: &[Def::Recent],
            },
        ],
    },
    Def::Sub {
        label: "menu-edit",
        items: &[
            item("edit.undo", "menu-undo"),
            item("edit.redo", "menu-redo"),
        ],
    },
    Def::Sub {
        label: "menu-view",
        items: &[
            item("view.palette", "menu-palette"),
            item("view.pie-settings", "pie-title"),
            item("view.drag-settings", "drag-title"),
            item("view.keymap-settings", "keymap-title"),
        ],
    },
    Def::Sub {
        label: "menu-layout",
        items: &[
            item("layout.preset.tsukumoe", "layout-preset-tsukumoe"),
            item("layout.preset.cubism", "layout-preset-cubism"),
            item("layout.save", "layout-save-named"),
            Def::Sub {
                label: "layout-load-named",
                items: &[Def::SavedLayouts],
            },
        ],
    },
];

/// What decides which items are enabled and what the dynamic lists hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub has_doc: bool,
    pub can_undo: bool,
    pub can_redo: bool,
    pub recent: Vec<String>,
    pub saved_layouts: Vec<String>,
}

impl State {
    pub fn enabled(&self, action: &str) -> bool {
        match action {
            "file.save" | "file.save-as" | "file.import-psd" => self.has_doc,
            "edit.undo" => self.can_undo,
            "edit.redo" => self.can_redo,
            _ => true,
        }
    }
}

/// A menu item as it is shown now. Equal trees draw equal menus, so the native
/// menu is rebuilt only when this changes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Item {
        action: String,
        label: String,
        enabled: bool,
        /// Display only ("⌘Z"); the keymap is what fires it.
        shortcut: Option<String>,
    },
    Separator,
    Sub {
        label: String,
        entries: Vec<Entry>,
    },
}

/// Builds the entry tree. `label` translates a key; `shortcut` gives an action's
/// key text.
pub fn resolve(
    defs: &[Def],
    state: &State,
    label: &dyn Fn(&str) -> String,
    shortcut: &dyn Fn(&str) -> Option<String>,
) -> Vec<Entry> {
    let mut out = Vec::new();
    let plain = |action: String, label: String, enabled: bool| Entry::Item {
        action,
        label,
        enabled,
        shortcut: None,
    };
    for def in defs {
        match def {
            Def::Item { action, label: key } => out.push(Entry::Item {
                action: (*action).into(),
                label: label(key),
                enabled: state.enabled(action),
                shortcut: shortcut(action),
            }),
            Def::Separator => out.push(Entry::Separator),
            Def::Sub { label: key, items } => out.push(Entry::Sub {
                label: label(key),
                entries: resolve(items, state, label, shortcut),
            }),
            Def::Recent if state.recent.is_empty() => {
                out.push(plain(String::new(), label("menu-no-recent"), false));
            }
            Def::Recent => out.extend(
                state
                    .recent
                    .iter()
                    .map(|p| plain(format!("file.open-recent:{p}"), p.clone(), true)),
            ),
            Def::SavedLayouts if state.saved_layouts.is_empty() => {
                out.push(plain(String::new(), label("layout-none-saved"), false));
            }
            Def::SavedLayouts => out.extend(
                state
                    .saved_layouts
                    .iter()
                    .map(|n| plain(format!("layout.load:{n}"), n.clone(), true)),
            ),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions;
    use crate::editor::Editor;
    use tsukumoe_i18n::{I18n, Language};

    fn keys(defs: &[Def], out: &mut Vec<(&'static str, Option<&'static str>)>) {
        for d in defs {
            match d {
                Def::Item { action, label } => out.push((label, Some(action))),
                Def::Sub { label, items } => {
                    out.push((label, None));
                    keys(items, out);
                }
                _ => {}
            }
        }
    }

    #[test]
    fn every_item_is_handled_and_every_label_exists_in_both_languages() {
        let mut all = Vec::new();
        keys(MENU, &mut all);
        let ctx = egui::Context::default();
        let mut editor = Editor::new(None);
        for (label, action) in all {
            for lang in Language::ALL {
                let t = I18n::new(lang);
                assert_ne!(t.tr(label), label, "{lang:?} {label}");
            }
            if let Some(action) = action {
                assert!(editor.run_action(&ctx, action), "{action} is not handled");
            }
        }
    }

    #[test]
    fn keymap_actions_in_the_menu_are_known_to_the_keymap() {
        let mut all = Vec::new();
        keys(MENU, &mut all);
        for (_, action) in all {
            let action = action.unwrap_or("");
            if action.starts_with("edit.") || action == "file.open" || action == "file.save" {
                assert!(actions::ACTIONS.contains(&action), "{action}");
            }
        }
    }

    fn tree(state: &State, key: &dyn Fn(&str) -> Option<String>) -> Vec<Entry> {
        resolve(MENU, state, &|k| k.to_string(), key)
    }

    fn find<'a>(entries: &'a [Entry], action: &str) -> Option<&'a Entry> {
        entries.iter().find_map(|e| match e {
            Entry::Item { action: a, .. } if a == action => Some(e),
            Entry::Sub { entries, .. } => find(entries, action),
            _ => None,
        })
    }

    fn is_enabled(entries: &[Entry], action: &str) -> bool {
        matches!(
            find(entries, action),
            Some(Entry::Item { enabled: true, .. })
        )
    }

    #[test]
    fn items_follow_the_document_and_the_history() {
        let none = tree(&State::default(), &|_| None);
        for a in [
            "file.save",
            "file.save-as",
            "file.import-psd",
            "edit.undo",
            "edit.redo",
        ] {
            assert!(!is_enabled(&none, a), "{a}");
        }
        assert!(is_enabled(&none, "file.open"));
        let state = State {
            has_doc: true,
            can_undo: true,
            ..State::default()
        };
        let t = tree(&state, &|_| None);
        assert!(is_enabled(&t, "file.save") && is_enabled(&t, "edit.undo"));
        assert!(!is_enabled(&t, "edit.redo"));
    }

    #[test]
    fn recent_projects_and_saved_layouts_fill_their_submenus() {
        let empty = tree(&State::default(), &|_| None);
        assert!(find(&empty, "").is_some(), "the empty note is shown");
        let state = State {
            recent: vec!["/a".into(), "/b".into()],
            saved_layouts: vec!["mine".into()],
            ..State::default()
        };
        let t = tree(&state, &|_| None);
        assert!(is_enabled(&t, "file.open-recent:/b"));
        assert!(is_enabled(&t, "layout.load:mine"));
        assert!(find(&t, "").is_none());
        assert_ne!(t, empty);
    }

    #[test]
    fn a_changed_key_assignment_changes_the_shown_shortcut() {
        let shown = |keys: &str| {
            let t = tree(&State::default(), &|a| {
                (a == "edit.undo").then(|| keys.to_string())
            });
            find(&t, "edit.undo").cloned()
        };
        assert_ne!(shown("Ctrl+Z"), shown("Ctrl+Y"));
    }
}
