// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The undo history: the applied commands in order, the ones undone and still
//! redoable dimmed, and a click on any row returns to that point.

use egui::Ui;
use tsukumoe_ui::{selectable_row, tr};

use crate::dispatcher::Dispatcher;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Applied,
    /// The newest applied command: where the model is now.
    Current,
    Undone,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Jumping here leaves this many commands applied.
    pub position: usize,
    /// `None` for the initial state, before any command.
    pub label: Option<String>,
    pub state: RowState,
}

/// Row 0 is the initial state; row `n` is the state after the n-th command.
pub fn rows(dispatcher: &Dispatcher) -> Vec<Row> {
    let cursor = dispatcher.cursor();
    let mut rows = vec![Row {
        position: 0,
        label: None,
        state: if cursor == 0 {
            RowState::Current
        } else {
            RowState::Applied
        },
    }];
    for i in 0..dispatcher.log().len() {
        let position = i + 1;
        rows.push(Row {
            position,
            label: dispatcher.name_of(i),
            state: match position.cmp(&cursor) {
                std::cmp::Ordering::Less => RowState::Applied,
                std::cmp::Ordering::Equal => RowState::Current,
                std::cmp::Ordering::Greater => RowState::Undone,
            },
        });
    }
    rows
}

/// The words that name what a command does; the rest name what it is done to.
const VERBS: [&str; 15] = [
    "add",
    "remove",
    "set",
    "move",
    "create",
    "delete",
    "rename",
    "duplicate",
    "reset",
    "bind",
    "unbind",
    "import",
    "capture",
    "copy",
    "clear",
];

/// A history label such as `set_mesh_opacity: Face` in the language of the UI:
/// the words of the command name are translated and put in that language's
/// order, then the detail follows.
fn humanize(ctx: &egui::Context, label: &str) -> String {
    let (head, detail) = label
        .split_once(": ")
        .map_or((label, None), |(h, d)| (h, Some(d)));
    if head.is_empty()
        || !head
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return label.to_owned();
    }
    let word = |w: &str| {
        let key = format!("history-word-{w}");
        let text = tr(ctx, &key);
        if text == key { w.to_owned() } else { text }
    };
    let words: Vec<&str> = head.split('_').collect();
    let verb_at = words.iter().position(|w| VERBS.contains(w)).unwrap_or(0);
    let verb = word(words[verb_at]);
    let rest: String = words
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != verb_at)
        .map(|(_, w)| word(w))
        .collect::<Vec<_>>()
        .join(&tr(ctx, "history-separator"));
    let mut op = tsukumoe_ui::tr_args(
        ctx,
        "history-compose",
        &[
            ("verb", verb.as_str().into()),
            ("rest", rest.as_str().into()),
        ],
    );
    // Without a noun the composed text can carry a dangling space.
    op = op.trim().to_owned();
    if let Some(first) = op.chars().next().filter(char::is_ascii_lowercase) {
        op.replace_range(..1, &first.to_ascii_uppercase().to_string());
    }
    match detail {
        Some(d) => tsukumoe_ui::tr_args(
            ctx,
            "history-entry",
            &[("op", op.as_str().into()), ("detail", d.into())],
        ),
        None => op,
    }
}

pub fn show(ui: &mut Ui, dispatcher: &mut Dispatcher) {
    let ctx = ui.ctx().clone();
    let mut jump = None;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for row in rows(dispatcher) {
                let text = row
                    .label
                    .as_deref()
                    .map_or_else(|| tr(&ctx, "history-initial"), |l| humanize(&ctx, l));
                let response = selectable_row(ui, row.state == RowState::Current, None, &text);
                if response.clicked() {
                    jump = Some(row.position);
                }
            }
        });
    if let Some(position) = jump {
        dispatcher.jump_to(position);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tsukumoe_core::command::Command;
    use tsukumoe_core::model::Model;

    fn part(name: &str) -> Command {
        Command::AddPart {
            name: name.into(),
            parent: None,
        }
    }

    #[test]
    fn rows_mark_applied_current_and_undone() {
        let mut d = Dispatcher::new(Model::new());
        assert_eq!(rows(&d).len(), 1);
        assert_eq!(rows(&d)[0].state, RowState::Current);
        for n in ["a", "b", "c"] {
            d.submit(part(n)).unwrap();
        }
        d.jump_to(2);
        let states: Vec<_> = rows(&d).iter().map(|r| r.state).collect();
        use RowState::*;
        assert_eq!(states, [Applied, Applied, Current, Undone]);
        assert_eq!(rows(&d)[3].label.as_deref(), Some("add_part: c"));
        assert_eq!(rows(&d)[3].position, 3);
        d.submit_labeled(part("d"), Some("named".into())).unwrap();
        assert_eq!(rows(&d)[3].label.as_deref(), Some("named"));
    }

    #[test]
    fn a_label_reads_in_the_language_of_the_ui() {
        let ctx = egui::Context::default();
        tsukumoe_ui::set_language(&ctx, tsukumoe_ui::Language::En);
        assert_eq!(
            humanize(&ctx, "set_mesh_opacity: Face"),
            "Set mesh opacity: Face"
        );
        assert_eq!(humanize(&ctx, "named by hand"), "named by hand");
        tsukumoe_ui::set_language(&ctx, tsukumoe_ui::Language::Ja);
        assert_eq!(
            humanize(&ctx, "set_mesh_opacity: Face"),
            "メッシュ不透明度設定: Face"
        );
        assert_eq!(humanize(&ctx, "add_part: c"), "パーツ追加: c");
        assert_eq!(humanize(&ctx, "param_add_key: P"), "パラメータキー追加: P");
    }
}
