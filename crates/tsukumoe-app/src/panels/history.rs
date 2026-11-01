// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The undo history: the applied commands in order, the ones undone and still
//! redoable dimmed, and a click on any row returns to that point.

use egui::Ui;
use tsukumoe_ui::{selectable_row, tr};

use crate::dispatcher::{Dispatcher, describe};

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
    for (i, command) in dispatcher.log().iter().enumerate() {
        let position = i + 1;
        rows.push(Row {
            position,
            label: Some(describe(command)),
            state: match position.cmp(&cursor) {
                std::cmp::Ordering::Less => RowState::Applied,
                std::cmp::Ordering::Equal => RowState::Current,
                std::cmp::Ordering::Greater => RowState::Undone,
            },
        });
    }
    rows
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
                    .clone()
                    .unwrap_or_else(|| tr(&ctx, "history-initial"));
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
    }
}
