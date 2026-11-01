// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The one door every edit goes through (ADR-0019): UI actions today, an
//! external channel (MCP, CLI) later. `submit` hands a [`Command`] to
//! [`Model::apply`]; a failure leaves the model as it was and is kept in
//! `error` for the UI. The dispatcher also keeps the list of applied commands
//! and the undo position, which the history panel shows and jumps within.

use tsukumoe_core::command::Command;
use tsukumoe_core::model::Model;

pub struct Dispatcher {
    model: Model,
    /// Every command that is applied or undone-but-redoable, oldest first.
    log: Vec<Command>,
    /// A name for each entry of `log` that says more than its command does.
    labels: Vec<Option<String>>,
    /// `log[..cursor]` is applied.
    cursor: usize,
    /// The cursor when the document was last saved; `None` once that state is
    /// no longer reachable by undo / redo.
    saved: Option<usize>,
    error: Option<String>,
    revision: u64,
}

impl Dispatcher {
    pub fn new(model: Model) -> Self {
        Self {
            model,
            log: Vec::new(),
            labels: Vec::new(),
            cursor: 0,
            saved: Some(0),
            error: None,
            revision: 0,
        }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Increases whenever the model changes; views rebuild from it.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Applies `command` as one undo step, dropping any redo steps. On failure
    /// the model is unchanged and the message is kept in [`Self::error`].
    pub fn submit(&mut self, command: Command) -> Result<(), String> {
        self.submit_labeled(command, None)
    }

    /// Like [`Self::submit`], with a name for the history list (a command such
    /// as `set_mesh_geometry` covers several operations).
    pub fn submit_labeled(
        &mut self,
        command: Command,
        label: Option<String>,
    ) -> Result<(), String> {
        match self.model.apply(command.clone()) {
            Ok(()) => {
                self.error = None;
                self.log.truncate(self.cursor);
                self.labels.truncate(self.cursor);
                if self.saved.is_some_and(|s| s > self.cursor) {
                    self.saved = None;
                }
                self.log.push(command);
                self.labels.push(label);
                self.cursor += 1;
                self.revision += 1;
                Ok(())
            }
            Err(e) => {
                let message = e.to_string();
                self.error = Some(message.clone());
                Err(message)
            }
        }
    }

    pub fn undo(&mut self) -> bool {
        if self.cursor == 0 || !self.model.undo() {
            return false;
        }
        self.cursor -= 1;
        self.revision += 1;
        true
    }

    pub fn redo(&mut self) -> bool {
        if self.cursor >= self.log.len() || !self.model.redo() {
            return false;
        }
        self.cursor += 1;
        self.revision += 1;
        true
    }

    /// Moves to the state after `position` commands (0 is the start).
    pub fn jump_to(&mut self, position: usize) {
        let position = position.min(self.log.len());
        while self.cursor > position && self.undo() {}
        while self.cursor < position && self.redo() {}
    }

    pub fn log(&self) -> &[Command] {
        &self.log
    }

    /// The history name of the `index`-th command: its label, else its description.
    pub fn name_of(&self, index: usize) -> Option<String> {
        let command = self.log.get(index)?;
        Some(match self.labels.get(index) {
            Some(Some(label)) => label.clone(),
            _ => describe(command),
        })
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn can_undo(&self) -> bool {
        self.cursor > 0
    }

    pub fn can_redo(&self) -> bool {
        self.cursor < self.log.len()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn clear_error(&mut self) {
        self.error = None;
    }

    pub fn dirty(&self) -> bool {
        self.saved != Some(self.cursor)
    }

    pub fn mark_saved(&mut self) {
        self.saved = Some(self.cursor);
    }
}

/// A short name for a command in the history list: its type and target.
pub fn describe(command: &Command) -> String {
    let value = serde_json::to_value(command).unwrap_or_default();
    let kind = value["type"].as_str().unwrap_or("?");
    match value["name"].as_str() {
        Some(name) => format!("{kind}: {name}"),
        None => kind.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(name: &str) -> Command {
        Command::AddParameter {
            name: name.into(),
            min: 0.0,
            max: 1.0,
            default: 0.0,
            repeat: false,
        }
    }

    fn names(d: &Dispatcher) -> Vec<String> {
        d.model()
            .parameters()
            .iter()
            .map(|(_, n, _)| n.to_owned())
            .collect()
    }

    #[test]
    fn a_failed_command_leaves_the_model_and_log_alone_and_reports() {
        let mut d = Dispatcher::new(Model::new());
        d.submit(add("a")).unwrap();
        let rev = d.revision();
        let err = d.submit(add("a")).unwrap_err();
        assert!(err.contains("already exists"), "{err}");
        assert_eq!(d.error(), Some(err.as_str()));
        assert_eq!(
            (names(&d), d.log().len(), d.revision()),
            (vec!["a".to_owned()], 1, rev)
        );
        d.submit(add("b")).unwrap();
        assert!(d.error().is_none());
    }

    #[test]
    fn undo_redo_and_jump_reach_any_point() {
        let mut d = Dispatcher::new(Model::new());
        for n in ["a", "b", "c"] {
            d.submit(add(n)).unwrap();
        }
        d.jump_to(1);
        assert_eq!((names(&d), d.cursor()), (vec!["a".to_owned()], 1));
        assert!(d.can_redo());
        d.jump_to(3);
        assert_eq!(names(&d).len(), 3);
        d.jump_to(0);
        assert!(names(&d).is_empty() && !d.can_undo());
        assert!(!d.undo());
        d.jump_to(99);
        assert_eq!(d.cursor(), 3);
        assert!(!d.redo());
    }

    #[test]
    fn a_new_command_drops_the_redo_steps() {
        let mut d = Dispatcher::new(Model::new());
        d.submit(add("a")).unwrap();
        d.submit(add("b")).unwrap();
        d.jump_to(1);
        d.submit(add("c")).unwrap();
        assert_eq!(d.log().len(), 2);
        assert!(!d.can_redo());
        assert_eq!(names(&d), ["a", "c"]);
    }

    #[test]
    fn dirty_follows_the_saved_position() {
        let mut d = Dispatcher::new(Model::new());
        assert!(!d.dirty());
        d.submit(add("a")).unwrap();
        assert!(d.dirty());
        d.mark_saved();
        assert!(!d.dirty());
        d.undo();
        assert!(d.dirty());
        d.redo();
        assert!(!d.dirty());
        // diverging from the saved state makes it unreachable
        d.undo();
        d.submit(add("x")).unwrap();
        d.undo();
        d.submit(add("y")).unwrap();
        assert!(d.dirty());
        d.jump_to(0);
        d.jump_to(1);
        assert!(d.dirty());
    }

    #[test]
    fn commands_are_described_by_type_and_name() {
        assert_eq!(describe(&add("p")), "add_parameter: p");
        assert_eq!(describe(&Command::Batch { commands: vec![] }), "batch");
    }
}
