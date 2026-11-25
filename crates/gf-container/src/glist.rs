//! Port of `container/glist`.
//!
//! `List<T>` is a locked double-ended queue. Go hands out element pointers for
//! insertion and removal; the port addresses positions by index (ADR-030).

use std::collections::VecDeque;
use std::sync::RwLock;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// A synchronized double-ended list.
pub struct List<T> {
    inner: RwLock<VecDeque<T>>,
}

impl<T: Clone> List<T> {
    /// Creates an empty list.
    pub fn new() -> List<T> {
        List {
            inner: RwLock::new(VecDeque::new()),
        }
    }

    /// Creates a list from values.
    pub fn new_from(values: Vec<T>) -> List<T> {
        List {
            inner: RwLock::new(values.into_iter().collect()),
        }
    }

    /// Prepends one value.
    pub fn push_front(&self, value: T) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.push_front(value);
    }

    /// Appends one value.
    pub fn push_back(&self, value: T) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.push_back(value);
    }

    /// Prepends values (the last one becomes the new front).
    pub fn push_fronts(&self, values: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for value in values {
            guard.push_front(value);
        }
    }

    /// Appends values.
    pub fn push_backs(&self, values: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.extend(values);
    }

    /// Removes and returns the front value.
    pub fn pop_front(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.pop_front()
    }

    /// Removes and returns the back value.
    pub fn pop_back(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.pop_back()
    }

    /// Removes up to `max` values from the front.
    pub fn pop_fronts(&self, max: usize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let take = max.min(guard.len());
        guard.drain(..take).collect()
    }

    /// Removes up to `max` values from the back.
    pub fn pop_backs(&self, max: usize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out = Vec::with_capacity(max.min(guard.len()));
        for _ in 0..max {
            match guard.pop_back() {
                Some(value) => out.push(value),
                None => break,
            }
        }
        out
    }

    /// Removes every value from the back.
    pub fn pop_back_all(&self) -> Vec<T> {
        self.pop_backs(self.len())
    }

    /// Removes every value from the front.
    pub fn pop_front_all(&self) -> Vec<T> {
        self.pop_fronts(self.len())
    }

    /// Returns the front value.
    pub fn front_value(&self) -> Option<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .front()
            .cloned()
    }

    /// Returns the back value.
    pub fn back_value(&self) -> Option<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .back()
            .cloned()
    }

    /// Returns values from front to back.
    pub fn front_all(&self) -> Vec<T> {
        self.slice()
    }

    /// Returns values from back to front.
    pub fn back_all(&self) -> Vec<T> {
        self.slice().into_iter().rev().collect()
    }

    /// Returns the number of values.
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Reports whether the list is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every value.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
    }

    /// Returns a snapshot from front to back.
    pub fn slice(&self) -> Vec<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    /// Removes and returns the value at `index`.
    pub fn remove_at(&self, index: usize) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.remove(index)
    }

    /// Inserts `value` at `index`.
    pub fn insert_at(&self, index: usize, value: T) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if index > guard.len() {
            return false;
        }
        guard.insert(index, value);
        true
    }

    /// Returns the front value without removing it, like `Front`.
    pub fn front(&self) -> Option<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .front()
            .cloned()
    }

    /// Returns the back value without removing it, like `Back`.
    pub fn back(&self) -> Option<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .back()
            .cloned()
    }

    /// Inserts `value` before `index`, like `InsertBefore`.
    pub fn insert_before(&self, index: i64, value: T) -> Result<(), Error> {
        self.insert_by_index(index, value, false)
    }

    /// Inserts `value` after `index`, like `InsertAfter`.
    pub fn insert_after(&self, index: i64, value: T) -> Result<(), Error> {
        self.insert_by_index(index, value, true)
    }

    fn insert_by_index(&self, index: i64, value: T, after: bool) -> Result<(), Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(position) = usize::try_from(index) else {
            return Err(invalid_index(index, guard.len()));
        };
        if position >= guard.len() {
            return Err(invalid_index(index, guard.len()));
        }
        guard.insert(if after { position + 1 } else { position }, value);
        Ok(())
    }

    /// Moves the value at `index` to the front, like `MoveToFront`.
    pub fn move_to_front(&self, index: i64) -> Result<(), Error> {
        let value = self.take_at(index)?;
        self.push_front(value);
        Ok(())
    }

    /// Moves the value at `index` to the back, like `MoveToBack`.
    pub fn move_to_back(&self, index: i64) -> Result<(), Error> {
        let value = self.take_at(index)?;
        self.push_back(value);
        Ok(())
    }

    /// Moves the value at `index` before the value at `mark`, like
    /// `MoveBefore`.
    pub fn move_before(&self, index: i64, mark: i64) -> Result<(), Error> {
        self.move_relative(index, mark, false)
    }

    /// Moves the value at `index` after the value at `mark`, like `MoveAfter`.
    pub fn move_after(&self, index: i64, mark: i64) -> Result<(), Error> {
        self.move_relative(index, mark, true)
    }

    fn move_relative(&self, index: i64, mark: i64, after: bool) -> Result<(), Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(position) = usize::try_from(index) else {
            return Err(invalid_index(index, guard.len()));
        };
        let Ok(mark_position) = usize::try_from(mark) else {
            return Err(invalid_index(mark, guard.len()));
        };
        if position >= guard.len() || mark_position >= guard.len() {
            return Err(invalid_index(index.max(mark), guard.len()));
        }
        if position == mark_position {
            return Ok(());
        }
        let value = guard.remove(position).expect("checked index");
        let mark_position = if position < mark_position {
            mark_position - 1
        } else {
            mark_position
        };
        guard.insert(
            if after {
                mark_position + 1
            } else {
                mark_position
            },
            value,
        );
        Ok(())
    }

    /// Removes and returns the value at `index`, erroring when out of range.
    fn take_at(&self, index: i64) -> Result<T, Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(position) = usize::try_from(index) else {
            return Err(invalid_index(index, guard.len()));
        };
        if position >= guard.len() {
            return Err(invalid_index(index, guard.len()));
        }
        Ok(guard.remove(position).expect("checked index"))
    }

    /// Removes every value, like `RemoveAll`.
    pub fn remove_all(&self) {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Removes every occurrence of each value in `values`, like `Removes`.
    pub fn removes(&self, values: &[T])
    where
        T: PartialEq,
    {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.retain(|item| !values.iter().any(|value| value == item));
    }

    /// Deep-copies the list, like `DeepCopy`.
    pub fn deep_copy(&self) -> List<T> {
        List::new_from(self.slice())
    }
}

/// Builds an invalid-index error, like GoFrame's `CodeInvalidParameter`.
fn invalid_index(index: i64, length: usize) -> Error {
    gerror::new_code(
        &gcode::CODE_INVALID_PARAMETER,
        &[format!("index {index} out of list range {length}").as_str()],
    )
}

impl<T: Clone + std::fmt::Display> List<T> {
    /// Joins the values with `glue`.
    pub fn join(&self, glue: &str) -> String {
        self.slice()
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(glue)
    }
}

impl<T: Clone> Default for List<T> {
    fn default() -> Self {
        List::new()
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for List<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.fmt(formatter)
    }
}

/// `list of any`.
pub type AnyList = List<gf_core::Value>;
/// `list of string`.
pub type StrList = List<String>;
/// `list of int`.
pub type IntList = List<i64>;

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn front_back_insert_move_remove() {
        let list: StrList = List::new_from(vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]);
        assert_eq!(list.front().as_deref(), Some("a"));
        assert_eq!(list.back().as_deref(), Some("c"));

        list.insert_before(1, "x".to_owned()).expect("before");
        assert_eq!(list.slice(), strings(&["a", "x", "b", "c"]));
        list.insert_after(3, "y".to_owned()).expect("after");
        assert_eq!(list.slice(), strings(&["a", "x", "b", "c", "y"]));
        assert!(list.insert_before(9, "z".to_owned()).is_err());
        assert!(list.insert_after(-1, "z".to_owned()).is_err());

        list.remove_all();
        assert!(list.is_empty());

        let list: StrList = List::new_from(vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ]);
        list.move_to_front(2).expect("to front");
        assert_eq!(list.slice(), strings(&["c", "a", "b", "d"]));
        list.move_to_back(0).expect("to back");
        assert_eq!(list.slice(), strings(&["a", "b", "d", "c"]));

        let list: StrList = List::new_from(vec![
            "a".to_owned(),
            "b".to_owned(),
            "c".to_owned(),
            "d".to_owned(),
        ]);
        list.move_before(3, 1).expect("move before");
        assert_eq!(list.slice(), strings(&["a", "d", "b", "c"]));
        list.move_after(0, 2).expect("move after");
        assert_eq!(list.slice(), strings(&["d", "b", "a", "c"]));
        assert!(list.move_before(9, 0).is_err());

        list.removes(&["b".to_owned(), "c".to_owned()]);
        assert_eq!(list.slice(), strings(&["d", "a"]));
        let copy = list.deep_copy();
        copy.push_back("z".to_owned());
        assert_eq!(list.len(), 2);
        assert_eq!(copy.len(), 3);
    }

    #[test]
    fn basic_list_operations() {
        let list: IntList = List::new();
        list.push_back(1);
        list.push_back(2);
        list.push_front(0);
        assert_eq!(list.slice(), vec![0, 1, 2]);
        assert_eq!(list.len(), 3);
        assert_eq!(list.front_value(), Some(0));
        assert_eq!(list.back_value(), Some(2));
        assert_eq!(list.back_all(), vec![2, 1, 0]);
        assert_eq!(list.pop_front(), Some(0));
        assert_eq!(list.pop_back(), Some(2));
        assert_eq!(list.pop_back(), Some(1));
        assert!(list.is_empty());
    }

    #[test]
    fn batch_operations_and_join() {
        let list: StrList = List::new();
        list.push_backs(["b".to_owned(), "c".to_owned()]);
        list.push_fronts(["a".to_owned()]);
        assert_eq!(
            list.slice(),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
        assert_eq!(list.join("-"), "a-b-c");
        assert_eq!(list.pop_fronts(2), vec!["a".to_owned(), "b".to_owned()]);
        list.push_backs(["d".to_owned()]);
        assert_eq!(list.pop_backs(5), vec!["d".to_owned(), "c".to_owned()]);

        list.push_backs(["x".to_owned(), "y".to_owned()]);
        assert!(list.insert_at(1, "m".to_owned()));
        assert_eq!(
            list.slice(),
            vec!["x".to_owned(), "m".to_owned(), "y".to_owned()]
        );
        assert_eq!(list.remove_at(1), Some("m".to_owned()));
        assert!(!list.insert_at(9, "z".to_owned()));
        assert_eq!(list.pop_front_all().len(), 2);
    }
}
