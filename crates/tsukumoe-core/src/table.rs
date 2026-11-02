// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! An arena whose elements also carry a persistent, unique string id.
//!
//! The internal [`Id`] changes between loads; the persistent name is what
//! commands and project files refer to (ADR-0018).

use std::collections::HashMap;

use crate::arena::{Arena, Id};

struct Entry<T> {
    name: String,
    value: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertError {
    EmptyName,
    DuplicateName,
}

pub struct Table<T> {
    arena: Arena<Entry<T>>,
    names: HashMap<String, Id<T>>,
    /// Iteration order: insertion order until `move_to` changes it.
    order: Vec<Id<T>>,
    /// Where a removed element stood, so that undo puts it back there.
    removed_at: HashMap<Id<T>, usize>,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            arena: Arena::new(),
            names: HashMap::new(),
            order: Vec::new(),
            removed_at: HashMap::new(),
        }
    }
}

impl<T> Table<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, value: T) -> Result<Id<T>, InsertError> {
        let name = name.into();
        if name.is_empty() {
            return Err(InsertError::EmptyName);
        }
        if self.names.contains_key(&name) {
            return Err(InsertError::DuplicateName);
        }
        let id = self
            .arena
            .insert(Entry {
                name: name.clone(),
                value,
            })
            .cast();
        self.names.insert(name, id);
        self.order.push(id);
        Ok(id)
    }

    /// Writes an element at exactly `id` under `name`, replacing whatever is
    /// there. Used to restore history; `name` must not belong to another id.
    pub(crate) fn put(&mut self, id: Id<T>, name: &str, value: T) {
        let entry = Entry {
            name: name.to_owned(),
            value,
        };
        if let Some(old) = self.arena.restore(id.cast(), entry) {
            self.names.remove(&old.name);
        }
        self.names.insert(name.to_owned(), id);
        if !self.order.contains(&id) {
            let at = self.removed_at.remove(&id).unwrap_or(usize::MAX);
            self.order.insert(at.min(self.order.len()), id);
        }
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.arena.get(id.cast()).map(|entry| &entry.value)
    }

    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        self.arena.get_mut(id.cast()).map(|entry| &mut entry.value)
    }

    pub fn name(&self, id: Id<T>) -> Option<&str> {
        self.arena.get(id.cast()).map(|entry| entry.name.as_str())
    }

    pub fn find(&self, name: &str) -> Option<Id<T>> {
        self.names.get(name).copied()
    }

    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let entry = self.arena.remove(id.cast())?;
        self.names.remove(&entry.name);
        if let Some(at) = self.order.iter().position(|&o| o == id) {
            self.order.remove(at);
            self.removed_at.insert(id, at);
        }
        Some(entry.value)
    }

    /// Moves `id` to position `to` (clamped) in the iteration order. `false`
    /// if `id` is not in the table.
    pub fn move_to(&mut self, id: Id<T>, to: usize) -> bool {
        let Some(at) = self.order.iter().position(|&o| o == id) else {
            return false;
        };
        self.order.remove(at);
        self.order.insert(to.min(self.order.len()), id);
        true
    }

    /// The position of `id` in the iteration order.
    pub fn position(&self, id: Id<T>) -> Option<usize> {
        self.order.iter().position(|&o| o == id)
    }

    pub fn contains(&self, id: Id<T>) -> bool {
        self.arena.contains(id.cast())
    }

    pub fn len(&self) -> usize {
        self.arena.len()
    }

    pub fn is_empty(&self) -> bool {
        self.arena.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &str, &T)> {
        self.order.iter().filter_map(|&id| {
            let entry = self.arena.get(id.cast())?;
            Some((id, entry.name.as_str(), &entry.value))
        })
    }
}

impl<T: PartialEq> PartialEq for Table<T> {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().eq(other.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_restores_id_and_name() {
        let mut table = Table::new();
        let id = table.insert("a", 1).unwrap();
        table.remove(id);
        table.put(id, "a", 1);
        assert_eq!(table.find("a"), Some(id));
        assert_eq!(table.get(id), Some(&1));
        table.put(id, "a", 5);
        assert_eq!(table.get(id), Some(&5));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn find_by_persistent_name() {
        let mut table = Table::new();
        let id = table.insert("ParamAngleX", 1).unwrap();
        assert_eq!(table.find("ParamAngleX"), Some(id));
        assert_eq!(table.name(id), Some("ParamAngleX"));
        assert_eq!(table.get(id), Some(&1));
    }

    #[test]
    fn duplicate_and_empty_names_are_rejected() {
        let mut table = Table::new();
        table.insert("a", 1).unwrap();
        assert_eq!(table.insert("a", 2), Err(InsertError::DuplicateName));
        assert_eq!(table.insert("", 2), Err(InsertError::EmptyName));
        assert_eq!(table.len(), 1);
    }

    #[test]
    fn name_is_released_on_remove() {
        let mut table = Table::new();
        let old = table.insert("a", 1).unwrap();
        assert_eq!(table.remove(old), Some(1));
        assert_eq!(table.find("a"), None);
        let new = table.insert("a", 2).unwrap();
        assert_ne!(old, new);
        assert_eq!(table.get(old), None);
        assert_eq!(table.name(old), None);
    }

    #[test]
    fn move_to_reorders_iteration() {
        let mut table = Table::new();
        let a = table.insert("a", 1).unwrap();
        table.insert("b", 2).unwrap();
        table.insert("c", 3).unwrap();
        assert!(table.move_to(a, 2));
        let names: Vec<_> = table.iter().map(|(_, n, _)| n).collect();
        assert_eq!(names, ["b", "c", "a"]);
        assert_eq!(table.position(a), Some(2));
    }

    #[test]
    fn put_after_remove_returns_to_the_old_position() {
        let mut table = Table::new();
        table.insert("a", 1).unwrap();
        let b = table.insert("b", 2).unwrap();
        table.insert("c", 3).unwrap();
        let v = table.remove(b).unwrap();
        table.put(b, "b", v);
        let names: Vec<_> = table.iter().map(|(_, n, _)| n).collect();
        assert_eq!(names, ["a", "b", "c"]);
    }
}
