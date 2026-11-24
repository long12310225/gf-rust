//! Port of `container/gset`.
//!
//! `Set<T>` is a locked insertion-ordered set; Go's `Set`, `IntSet`, `StrSet`
//! and `TSet` share this implementation with type aliases (ADR-030).

use std::hash::Hash;
use std::sync::RwLock;

use indexmap::IndexSet;

/// A synchronized set that keeps insertion order.
pub struct Set<T> {
    inner: RwLock<IndexSet<T>>,
}

impl<T: Eq + Hash + Clone> Set<T> {
    /// Creates an empty set.
    pub fn new() -> Set<T> {
        Set {
            inner: RwLock::new(IndexSet::new()),
        }
    }

    /// Creates a set from items.
    pub fn new_from(items: Vec<T>) -> Set<T> {
        Set {
            inner: RwLock::new(items.into_iter().collect()),
        }
    }

    /// Adds items.
    pub fn add(&self, items: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for item in items {
            guard.insert(item);
        }
    }

    /// Adds `item` when absent and reports whether it was inserted.
    pub fn add_if_not_exist(&self, item: T) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(item)
    }

    /// Reports whether `item` exists.
    pub fn contains(&self, item: &T) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(item)
    }

    /// Removes `item`.
    pub fn remove(&self, item: &T) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.shift_remove(item);
    }

    /// Returns the number of items.
    pub fn size(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Removes every item.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
    }

    /// Returns the items in insertion order.
    pub fn slice(&self) -> Vec<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Joins the items with `glue`.
    pub fn join(&self, glue: &str) -> String
    where
        T: std::fmt::Display,
    {
        self.slice()
            .into_iter()
            .map(|item| item.to_string())
            .collect::<Vec<_>>()
            .join(glue)
    }

    /// Removes and returns one item (the last inserted).
    pub fn pop(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.pop()
    }

    /// Removes and returns up to `size` items (last inserted first).
    pub fn pops(&self, size: usize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out = Vec::with_capacity(size);
        for _ in 0..size {
            match guard.pop() {
                Some(item) => out.push(item),
                None => break,
            }
        }
        out
    }

    /// Removes duplicates while keeping insertion order.
    pub fn unique(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let unique: IndexSet<T> = guard.iter().cloned().collect();
        *guard = unique;
    }

    /// Reports whether both sets contain the same items.
    pub fn equal(&self, other: &Set<T>) -> bool {
        let left = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let right = other
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        left.len() == right.len() && left.iter().all(|item| right.contains(item))
    }

    /// Reports whether every item of this set is in `other`.
    pub fn is_subset_of(&self, other: &Set<T>) -> bool {
        let left = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let right = other
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        left.iter().all(|item| right.contains(item))
    }

    /// Returns the union with `others`, in `self` order then new items.
    pub fn union(&self, others: &[&Set<T>]) -> Set<T> {
        let out = Set::new_from(self.slice());
        for other in others {
            out.add(other.slice());
        }
        out
    }

    /// Returns the items of `self` not present in `others`.
    pub fn diff(&self, others: &[&Set<T>]) -> Set<T> {
        let mut out: IndexSet<T> = self.slice().into_iter().collect();
        for other in others {
            let other_items = other.slice();
            for item in other_items {
                out.shift_remove(&item);
            }
        }
        Set {
            inner: RwLock::new(out),
        }
    }

    /// Returns the items present in `self` and every other set.
    pub fn intersect(&self, others: &[&Set<T>]) -> Set<T> {
        let mut out: IndexSet<T> = self.slice().into_iter().collect();
        for other in others {
            let other_items = other.slice();
            out.retain(|item| other_items.contains(item));
        }
        Set {
            inner: RwLock::new(out),
        }
    }

    /// Returns the items of `full` that are not in `self`.
    pub fn complement(&self, full: &Set<T>) -> Set<T> {
        let full_items = full.slice();
        let self_items: IndexSet<T> = self.slice().into_iter().collect();
        let mut out = IndexSet::new();
        for item in full_items {
            if !self_items.contains(&item) {
                out.insert(item);
            }
        }
        Set {
            inner: RwLock::new(out),
        }
    }

    /// Adds every item of `others` to `self`.
    pub fn merge(&self, others: &[&Set<T>]) -> &Self {
        for other in others {
            self.add(other.slice());
        }
        self
    }
}

impl<T: Eq + Hash + Clone> Default for Set<T> {
    fn default() -> Self {
        Set::new()
    }
}

impl<T: Eq + Hash + Clone + std::fmt::Debug> std::fmt::Debug for Set<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.fmt(formatter)
    }
}

/// `map[any]struct{}` for string values.
pub type StrSet = Set<String>;
/// Integer set.
pub type IntSet = Set<i64>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_set_operations() {
        let set: IntSet = Set::new();
        assert!(set.add_if_not_exist(1));
        assert!(!set.add_if_not_exist(1));
        set.add([2, 3]);
        assert_eq!(set.size(), 3);
        assert!(set.contains(&2));
        assert_eq!(set.slice(), vec![1, 2, 3]);
        assert_eq!(set.join(","), "1,2,3");
        set.remove(&2);
        assert!(!set.contains(&2));
        assert_eq!(set.pop(), Some(3));
        assert_eq!(set.pops(5), vec![1]);
        assert!(set.slice().is_empty());
    }

    #[test]
    fn set_algebra() {
        let a: IntSet = Set::new_from(vec![1, 2, 3]);
        let b: IntSet = Set::new_from(vec![3, 4]);
        let union = a.union(&[&b]);
        assert_eq!(union.slice(), vec![1, 2, 3, 4]);
        assert_eq!(a.diff(&[&b]).slice(), vec![1, 2]);
        assert_eq!(a.intersect(&[&b]).slice(), vec![3]);
        assert_eq!(a.complement(&union).slice(), vec![4]);
        assert!(a.is_subset_of(&union));
        assert!(a.equal(&Set::new_from(vec![1, 2, 3])));
        let merged: IntSet = Set::new_from(vec![1]);
        merged.merge(&[&b]);
        assert_eq!(merged.size(), 3);
    }

    #[test]
    fn string_set() {
        let set: StrSet = Set::new_from(vec!["b".to_owned(), "a".to_owned()]);
        assert_eq!(set.slice(), vec!["b".to_owned(), "a".to_owned()]);
        assert!(set.contains(&"a".to_owned()));
        assert_eq!(set.join("-"), "b-a");
    }
}
