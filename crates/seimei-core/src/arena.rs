//! Generational arena and typed ids (ADR-0018).

use std::fmt;
use std::hash::{Hash, Hasher};
use std::marker::PhantomData;

/// Typed handle into an [`Arena`] or a [`Table`](crate::table::Table).
///
/// The index is reused after a removal, the generation is not: an id that
/// outlived its element no longer resolves.
pub struct Id<T> {
    index: u32,
    generation: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> Id<T> {
    pub(crate) fn cast<U>(self) -> Id<U> {
        Id {
            index: self.index,
            generation: self.generation,
            _marker: PhantomData,
        }
    }
}

impl<T> Clone for Id<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Id<T> {}

impl<T> PartialEq for Id<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Id<T> {}

impl<T> Hash for Id<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Id<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({}v{})", self.index, self.generation)
    }
}

struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// Slot storage with generation-checked handles.
pub struct Arena<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, value: T) -> Id<T> {
        self.len += 1;
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.value = Some(value);
            return Id {
                index,
                generation: slot.generation,
                _marker: PhantomData,
            };
        }
        let index = u32::try_from(self.slots.len()).expect("arena exceeds u32::MAX slots");
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        Id {
            index,
            generation: 0,
            _marker: PhantomData,
        }
    }

    pub fn get(&self, id: Id<T>) -> Option<&T> {
        self.slots
            .get(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.value.as_ref())
    }

    pub fn get_mut(&mut self, id: Id<T>) -> Option<&mut T> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|slot| slot.generation == id.generation)
            .and_then(|slot| slot.value.as_mut())
    }

    pub fn remove(&mut self, id: Id<T>) -> Option<T> {
        let slot = self.slots.get_mut(id.index as usize)?;
        if slot.generation != id.generation {
            return None;
        }
        let value = slot.value.take()?;
        slot.generation = slot.generation.wrapping_add(1);
        self.free.push(id.index);
        self.len -= 1;
        Some(value)
    }

    pub fn contains(&self, id: Id<T>) -> bool {
        self.get(id).is_some()
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn iter(&self) -> impl Iterator<Item = (Id<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            let value = slot.value.as_ref()?;
            let id = Id {
                index: index as u32,
                generation: slot.generation,
                _marker: PhantomData,
            };
            Some((id, value))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut arena = Arena::new();
        let a = arena.insert("a");
        let b = arena.insert("b");
        assert_eq!(arena.get(a), Some(&"a"));
        assert_eq!(arena.get(b), Some(&"b"));
        assert_eq!(arena.len(), 2);
    }

    #[test]
    fn removed_id_no_longer_resolves() {
        let mut arena = Arena::new();
        let a = arena.insert(1);
        assert_eq!(arena.remove(a), Some(1));
        assert_eq!(arena.get(a), None);
        assert_eq!(arena.remove(a), None);
        assert!(arena.is_empty());
    }

    #[test]
    fn reused_slot_does_not_revive_old_id() {
        let mut arena = Arena::new();
        let old = arena.insert(1);
        arena.remove(old);
        let new = arena.insert(2);
        assert_ne!(old, new);
        assert_eq!(arena.get(old), None);
        assert_eq!(arena.get(new), Some(&2));
    }

    #[test]
    fn iter_skips_removed() {
        let mut arena = Arena::new();
        let a = arena.insert(1);
        let b = arena.insert(2);
        arena.remove(a);
        let ids: Vec<_> = arena.iter().map(|(id, v)| (id, *v)).collect();
        assert_eq!(ids, vec![(b, 2)]);
    }
}
