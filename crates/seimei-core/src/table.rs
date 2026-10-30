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
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            arena: Arena::new(),
            names: HashMap::new(),
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
        Ok(id)
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
        Some(entry.value)
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
        self.arena
            .iter()
            .map(|(id, entry)| (id.cast(), entry.name.as_str(), &entry.value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
