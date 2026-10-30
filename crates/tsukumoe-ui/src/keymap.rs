// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! Key bindings: actions named by the app, bound to shortcuts, optionally only
//! inside a context (a panel or a tool). The user can change the bindings and
//! save them as JSON.
//!
//! ```text
//! {"bindings": [{"action": "edit.undo", "keys": "Mod+Z"},
//!               {"action": "tool.pan", "context": "viewport", "keys": "Space"}]}
//! ```
//!
//! `Mod` is Command on macOS and Ctrl elsewhere. A binding with a context wins
//! over one without when both match.

use egui::{Context, Key, KeyboardShortcut, Modifiers};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    context: Option<String>,
    keys: String,
}

/// The keymap file.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeymapFile {
    bindings: Vec<BindingFile>,
}

#[derive(Clone, Debug, PartialEq)]
struct Binding {
    action: String,
    context: Option<String>,
    shortcut: KeyboardShortcut,
}

#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<Binding>,
}

/// Reads `Mod+Shift+Z`: modifiers (`Mod`, `Ctrl`, `Cmd`, `Alt`, `Shift`) and
/// a key name, joined by `+`.
pub fn parse_shortcut(text: &str) -> Result<KeyboardShortcut, String> {
    let mut parts: Vec<&str> = text.split('+').map(str::trim).collect();
    let key_name = parts.pop().filter(|k| !k.is_empty()).ok_or("no key")?;
    let mut modifiers = Modifiers::NONE;
    for part in parts {
        modifiers |= match part.to_ascii_lowercase().as_str() {
            "mod" => Modifiers::COMMAND,
            "ctrl" | "control" => Modifiers::CTRL,
            "cmd" | "command" => Modifiers::MAC_CMD,
            "alt" | "option" => Modifiers::ALT,
            "shift" => Modifiers::SHIFT,
            other => return Err(format!("unknown modifier {other:?}")),
        };
    }
    let key = Key::from_name(key_name).ok_or_else(|| format!("unknown key {key_name:?}"))?;
    Ok(KeyboardShortcut::new(modifiers, key))
}

fn modifier_count(m: Modifiers) -> usize {
    [m.alt, m.ctrl, m.shift, m.mac_cmd, m.command]
        .into_iter()
        .filter(|on| *on)
        .count()
}

impl Keymap {
    pub fn new() -> Self {
        Self::default()
    }

    /// Binds `action` (in `context`, or everywhere) to `keys`, replacing any
    /// earlier binding of the same action in the same context.
    pub fn bind(&mut self, action: &str, context: Option<&str>, keys: &str) -> Result<(), String> {
        let shortcut = parse_shortcut(keys).map_err(|e| format!("{action}: {e}"))?;
        self.bindings
            .retain(|b| !(b.action == action && b.context.as_deref() == context));
        self.bindings.push(Binding {
            action: action.to_owned(),
            context: context.map(str::to_owned),
            shortcut,
        });
        Ok(())
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let file: KeymapFile = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut map = Self::new();
        for b in file.bindings {
            map.bind(&b.action, b.context.as_deref(), &b.keys)?;
        }
        Ok(map)
    }

    /// The keymap as JSON text, in binding order.
    pub fn to_json(&self, ctx: &Context) -> String {
        let bindings = self
            .bindings
            .iter()
            .map(|b| BindingFile {
                action: b.action.clone(),
                context: b.context.clone(),
                keys: shortcut_keys(&b.shortcut, ctx),
            })
            .collect();
        serde_json::to_string_pretty(&KeymapFile { bindings }).expect("keymap serializes")
    }

    /// Pairs of actions bound to the same shortcut in the same context.
    pub fn conflicts(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (i, a) in self.bindings.iter().enumerate() {
            for b in &self.bindings[i + 1..] {
                if a.context == b.context && a.shortcut == b.shortcut {
                    out.push((a.action.clone(), b.action.clone()));
                }
            }
        }
        out
    }

    /// The shortcut of `action` as the menu should show it ("⌘Z", "Ctrl+Z").
    pub fn shortcut_text(&self, ctx: &Context, action: &str) -> Option<String> {
        let binding = self
            .bindings
            .iter()
            .filter(|b| b.action == action)
            .min_by_key(|b| b.context.is_some())?;
        Some(ctx.format_shortcut(&binding.shortcut))
    }

    /// The actions whose shortcut was pressed this frame, consuming the keys.
    /// `active` lists the contexts that are active now. A binding in an active
    /// context is tried before a global one, and one with more modifiers before
    /// one with fewer. While a text field has the keyboard, only bindings with
    /// a Ctrl, Alt or Command modifier can fire.
    pub fn triggered(&self, ctx: &Context, active: &[&str]) -> Vec<String> {
        let typing = ctx.egui_wants_keyboard_input();
        let mut candidates: Vec<&Binding> = self
            .bindings
            .iter()
            .filter(|b| match &b.context {
                None => true,
                Some(c) => active.contains(&c.as_str()),
            })
            .filter(|b| {
                let m = b.shortcut.modifiers;
                !typing || m.ctrl || m.alt || m.command || m.mac_cmd
            })
            .collect();
        candidates.sort_by_key(|b| {
            (
                b.context.is_none(),
                std::cmp::Reverse(modifier_count(b.shortcut.modifiers)),
            )
        });
        candidates
            .into_iter()
            .filter(|b| ctx.input_mut(|i| i.consume_shortcut(&b.shortcut)))
            .map(|b| b.action.clone())
            .collect()
    }
}

/// The text form of a shortcut that [`parse_shortcut`] reads back.
fn shortcut_keys(s: &KeyboardShortcut, _ctx: &Context) -> String {
    let m = s.modifiers;
    let mut parts = Vec::new();
    if m.command {
        parts.push("Mod");
    } else {
        if m.ctrl {
            parts.push("Ctrl");
        }
        if m.mac_cmd {
            parts.push("Cmd");
        }
    }
    if m.alt {
        parts.push("Alt");
    }
    if m.shift {
        parts.push("Shift");
    }
    parts.push(s.logical_key.name());
    parts.join("+")
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Event, RawInput};

    fn press(keymap: &Keymap, active: &[&str], key: Key, modifiers: Modifiers) -> Vec<String> {
        let ctx = Context::default();
        let mut input = RawInput::default();
        input.events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers,
        });
        let mut fired = Vec::new();
        let mut output = ctx.run_ui(input, |ui| fired = keymap.triggered(ui.ctx(), active));
        output.textures_delta.clear();
        fired
    }

    fn sample() -> Keymap {
        let mut k = Keymap::new();
        k.bind("edit.undo", None, "Mod+Z").unwrap();
        k.bind("edit.redo", None, "Mod+Shift+Z").unwrap();
        k.bind("tool.pan", Some("viewport"), "Space").unwrap();
        k.bind("list.select-all", Some("list"), "Mod+Z").unwrap();
        k
    }

    #[test]
    fn shortcuts_parse_and_reject_nonsense() {
        let s = parse_shortcut("Mod+Shift+Z").unwrap();
        assert_eq!(s.logical_key, Key::Z);
        assert!(s.modifiers.command && s.modifiers.shift);
        for bad in ["", "Mod+", "Hyper+Z", "Mod+Nope"] {
            assert!(parse_shortcut(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn more_modifiers_are_tried_first() {
        let k = sample();
        let m = Modifiers::COMMAND | Modifiers::SHIFT;
        assert_eq!(press(&k, &[], Key::Z, m), ["edit.redo"]);
        assert_eq!(press(&k, &[], Key::Z, Modifiers::COMMAND), ["edit.undo"]);
    }

    #[test]
    fn a_context_binding_wins_over_a_global_one_and_only_when_active() {
        let k = sample();
        assert_eq!(
            press(&k, &["list"], Key::Z, Modifiers::COMMAND),
            ["list.select-all"]
        );
        assert_eq!(
            press(&k, &["viewport"], Key::Z, Modifiers::COMMAND),
            ["edit.undo"]
        );
        assert_eq!(
            press(&k, &["viewport"], Key::Space, Modifiers::NONE),
            ["tool.pan"]
        );
        assert!(press(&k, &[], Key::Space, Modifiers::NONE).is_empty());
    }

    #[test]
    fn rebinding_replaces_and_conflicts_are_reported() {
        let mut k = sample();
        k.bind("edit.undo", None, "Mod+Y").unwrap();
        assert_eq!(
            press(&k, &[], Key::Z, Modifiers::COMMAND),
            Vec::<String>::new()
        );
        assert_eq!(press(&k, &[], Key::Y, Modifiers::COMMAND), ["edit.undo"]);
        assert!(k.conflicts().is_empty());
        k.bind("edit.redo", None, "Mod+Y").unwrap();
        assert_eq!(
            k.conflicts(),
            [("edit.undo".to_owned(), "edit.redo".to_owned())]
        );
    }

    #[test]
    fn json_round_trips_and_rejects_bad_files() {
        let ctx = Context::default();
        ctx.run_ui(RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let k = sample();
        let again = Keymap::from_json(&k.to_json(&ctx)).unwrap();
        assert_eq!(again.bindings, k.bindings);
        for bad in [
            "not json",
            r#"{"bindings":[{"action":"a","keys":"Hyper+Z"}]}"#,
            r#"{"bindings":[{"action":"a","keys":"Z","extra":1}]}"#,
        ] {
            assert!(Keymap::from_json(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn shortcut_text_prefers_the_global_binding() {
        let ctx = Context::default();
        // Formatting needs fonts, which exist after the first frame.
        ctx.run_ui(RawInput::default(), |_| {})
            .textures_delta
            .clear();
        let t = sample().shortcut_text(&ctx, "edit.undo").unwrap();
        assert!(t.contains('Z'));
        assert!(sample().shortcut_text(&ctx, "nope").is_none());
    }
}
