//! Port of `container/gring`.
//!
//! A fixed-capacity circular buffer with a cursor. Go returns linked `*Ring`
//! nodes from `Next`/`Prev`; the port keeps one buffer and moves the cursor,
//! so `next()`/`prev()` return the ring itself (ADR-030).

use std::sync::RwLock;

struct State<T> {
    values: Vec<Option<T>>,
    index: usize,
    length: usize,
}

/// A synchronized circular buffer.
pub struct Ring<T> {
    inner: RwLock<State<T>>,
}

impl<T: Clone> Ring<T> {
    /// Creates a ring with `capacity` slots (at least one).
    pub fn new(capacity: usize) -> Ring<T> {
        let capacity = capacity.max(1);
        Ring {
            inner: RwLock::new(State {
                values: std::iter::repeat_with(|| None).take(capacity).collect(),
                index: 0,
                length: 0,
            }),
        }
    }

    /// Returns the value at the cursor.
    pub fn val(&self) -> Option<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.values[guard.index].clone()
    }

    /// Returns the number of stored values.
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .length
    }

    /// Returns whether no value is stored.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the capacity.
    pub fn cap(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values
            .len()
    }

    /// Replaces the value at the cursor.
    pub fn set(&self, value: T) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let index = guard.index;
        guard.values[index] = Some(value);
        guard.length = guard.length.max(1);
        self
    }

    /// Stores `value` at the cursor and advances.
    pub fn put(&self, value: T) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let index = guard.index;
        guard.values[index] = Some(value);
        guard.length = (guard.length + 1).min(guard.values.len());
        guard.index = (index + 1) % guard.values.len();
        self
    }

    /// Moves the cursor by `n` slots.
    pub fn move_by(&self, n: isize) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let capacity = guard.values.len() as isize;
        let mut index = (guard.index as isize + n) % capacity;
        if index < 0 {
            index += capacity;
        }
        guard.index = index as usize;
        self
    }

    /// Moves the cursor back one slot.
    pub fn prev(&self) -> &Self {
        self.move_by(-1)
    }

    /// Moves the cursor forward one slot.
    pub fn next(&self) -> &Self {
        self.move_by(1)
    }

    /// Returns values from the cursor forward.
    pub fn slice_next(&self) -> Vec<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let capacity = guard.values.len();
        let mut out = Vec::with_capacity(guard.length);
        for offset in 0..capacity {
            if out.len() >= guard.length {
                break;
            }
            if let Some(value) = &guard.values[(guard.index + offset) % capacity] {
                out.push(value.clone());
            }
        }
        out
    }

    /// Returns values from the cursor backward.
    pub fn slice_prev(&self) -> Vec<T> {
        let mut values = self.slice_next();
        values.reverse();
        values
    }
}

impl<T: Clone> Default for Ring<T> {
    fn default() -> Self {
        Ring::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_and_wrap() {
        let ring: Ring<i64> = Ring::new(3);
        assert_eq!(ring.cap(), 3);
        assert_eq!(ring.len(), 0);
        ring.put(1).put(2).put(3);
        assert_eq!(ring.len(), 3);
        assert_eq!(ring.val(), Some(1));
        ring.next();
        assert_eq!(ring.val(), Some(2));
        ring.prev();
        assert_eq!(ring.val(), Some(1));
        ring.move_by(-1);
        assert_eq!(ring.val(), Some(3));
        ring.put(4);
        assert_eq!(ring.slice_next(), vec![1, 2, 4]);
        assert_eq!(ring.slice_prev(), vec![4, 2, 1]);
        ring.next();
        assert_eq!(ring.val(), Some(2));
        ring.set(9);
        assert_eq!(ring.val(), Some(9));
    }

    #[test]
    fn single_slot_ring() {
        let ring: Ring<String> = Ring::new(0);
        assert_eq!(ring.cap(), 1);
        ring.put("a".to_owned());
        assert_eq!(ring.val(), Some("a".to_owned()));
        ring.put("b".to_owned());
        assert_eq!(ring.val(), Some("b".to_owned()));
        assert_eq!(ring.len(), 1);
    }
}
