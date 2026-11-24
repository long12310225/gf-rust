//! Port of `container/gtree`.
//!
//! Go ships an AVL tree, a red-black tree and a B-tree for both `any`/`any`
//! and generic key/value trees. The port keeps one generic type per tree and
//! exposes the Go names as aliases (ADR-030), and every tree is always
//! synchronized behind a lock (ADR-007).
//!
//! The AVL and red-black trees share one ordered-map implementation: the key
//! is wrapped in [`CmpKey`], whose ordering delegates to the tree comparator,
//! and the payload lives in a [`BTreeMap`]. The B-tree is a faithful port of
//! the `gods` algorithm GoFrame uses, so `Height` and `String` match.
//!
//! Divergence (ADR-007): Go's `*FuncLock` variants run the callback while
//! holding the write lock. The Rust port never calls a user callback under a
//! lock; it reads the current value, releases the lock, calls the closure and
//! then inserts under a fresh write lock.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use gf_core::Value;
use gf_util::gconv;
use gf_util::gutil::comparator_t_str;
use indexmap::IndexMap;

use crate::gvar::Var;

/// The comparator a tree orders its keys with: a negative number, zero or a
/// positive number when `a < b`, `a == b` or `a > b`.
pub type TreeComparator<K> = Arc<dyn Fn(&K, &K) -> i32 + Send + Sync>;

/// A tree value, which knows whether it must be treated like Go's `nil`.
///
/// `set`/`set_from` ignore nil values and return them unchanged, mirroring the
/// Go implementation's `empty.IsNil` check.
pub trait TreeValue: Clone {
    /// Reports whether the value is nil; defaults to `false`.
    fn is_nil(&self) -> bool {
        false
    }
}

impl TreeValue for Value {
    fn is_nil(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl<T: Clone> TreeValue for Option<T> {
    fn is_nil(&self) -> bool {
        self.is_none()
    }
}

impl TreeValue for String {}
impl TreeValue for i64 {}
impl TreeValue for i32 {}
impl TreeValue for u64 {}
impl TreeValue for u32 {}
impl TreeValue for f64 {}
impl TreeValue for bool {}
impl TreeValue for Vec<u8> {}

/// A key usable by a tree.
///
/// Besides the cloneable, thread-safe bound every tree key needs, this exposes
/// the index used by `iterator_*_from` when `match` is `false` and the text
/// form used by `map_str_any`/`marshal_json`.
pub trait TreeKey: Clone + Send + Sync + ToString + 'static {
    /// The zero-based position used by index-mode `iterator_*_from`.
    ///
    /// Go only honours an `int` key here; every type returns `None` unless it
    /// is an integer (and [`Value::Int`] for the dynamic key type).
    fn tree_index(&self) -> Option<usize> {
        None
    }

    /// The key text used by `map_str_any` and `marshal_json`.
    fn map_key_string(&self) -> String {
        self.to_string()
    }
}

impl TreeKey for Value {
    fn tree_index(&self) -> Option<usize> {
        match self {
            Value::Int(value) => usize::try_from(*value).ok(),
            _ => None,
        }
    }

    fn map_key_string(&self) -> String {
        gconv::string(self)
    }
}

impl TreeKey for String {}

impl TreeKey for i64 {
    fn tree_index(&self) -> Option<usize> {
        usize::try_from(*self).ok()
    }
}

impl TreeKey for i32 {
    fn tree_index(&self) -> Option<usize> {
        usize::try_from(*self).ok()
    }
}

impl TreeKey for u64 {
    fn tree_index(&self) -> Option<usize> {
        usize::try_from(*self).ok()
    }
}

impl TreeKey for u32 {
    fn tree_index(&self) -> Option<usize> {
        usize::try_from(*self).ok()
    }
}

impl TreeKey for usize {
    fn tree_index(&self) -> Option<usize> {
        Some(*self)
    }
}

impl TreeKey for isize {
    fn tree_index(&self) -> Option<usize> {
        usize::try_from(*self).ok()
    }
}

/// Resolves the optional comparator, falling back to Go's `ComparatorTStr`.
fn resolve_comparator<K: ToString>(comparator: Option<TreeComparator<K>>) -> TreeComparator<K> {
    match comparator {
        Some(comparator) => comparator,
        None => Arc::new(|a: &K, b: &K| comparator_t_str(a, b)),
    }
}

/// A map key whose ordering is delegated to the owning tree's comparator.
///
/// Two keys compare equal exactly when the comparator returns `0`, so the map
/// has the comparator's semantics while still being a [`BTreeMap`].
#[derive(Clone)]
struct CmpKey<K> {
    key: K,
    cmp: TreeComparator<K>,
}

impl<K> PartialEq for CmpKey<K> {
    fn eq(&self, other: &Self) -> bool {
        (self.cmp)(&self.key, &other.key) == 0
    }
}

impl<K> Eq for CmpKey<K> {}

impl<K> PartialOrd for CmpKey<K> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K> Ord for CmpKey<K> {
    fn cmp(&self, other: &Self) -> Ordering {
        let ordering = (self.cmp)(&self.key, &other.key);
        if ordering < 0 {
            Ordering::Less
        } else if ordering > 0 {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    }
}

/// The shared storage of the ordered (AVL and red-black) trees.
#[derive(Clone)]
struct OrderedState<K, V> {
    comparator: TreeComparator<K>,
    map: BTreeMap<CmpKey<K>, V>,
}

impl<K, V> OrderedState<K, V> {
    fn new(comparator: TreeComparator<K>) -> OrderedState<K, V> {
        OrderedState {
            comparator,
            map: BTreeMap::new(),
        }
    }
}

/// Writes `keys` as an in-order listing with four spaces of indentation per
/// balanced-tree depth, one key per line.
fn balanced_listing<K: ToString>(keys: &[K]) -> String {
    fn visit<K: ToString>(keys: &[K], low: usize, high: usize, depth: usize, out: &mut String) {
        if low >= high {
            return;
        }
        let middle = low + (high - low) / 2;
        visit(keys, low, middle, depth + 1, out);
        for _ in 0..depth {
            out.push_str("    ");
        }
        out.push_str(&keys[middle].to_string());
        out.push('\n');
        visit(keys, middle + 1, high, depth + 1, out);
    }

    let mut out = String::new();
    visit(keys, 0, keys.len(), 0, &mut out);
    out
}

/// Generates the identical implementation for the AVL and red-black trees.
macro_rules! impl_ordered_tree {
    ($tree:ident, $node:ident) => {
        impl<K, V> $tree<K, V>
        where
            K: TreeKey,
            V: TreeValue + Send + Sync + 'static,
        {
            /// Creates an empty tree.
            pub fn new(comparator: Option<TreeComparator<K>>) -> Self {
                $tree {
                    inner: RwLock::new(OrderedState::new(resolve_comparator(comparator))),
                }
            }

            /// Creates a tree from key-value pairs, ignoring nil values.
            pub fn new_from(
                comparator: Option<TreeComparator<K>>,
                data: impl IntoIterator<Item = (K, V)>,
            ) -> Self {
                let tree = Self::new(comparator);
                tree.sets(data);
                tree
            }

            /// Changes the ordering comparator and re-inserts the entries under
            /// it, like Go's `SetComparator`.
            ///
            /// Go accepts a `nil` comparator here and then panics on the next
            /// insertion; Rust callers pass the comparator they want, such as
            /// [`comparator_t_str`] for the default string ordering.
            pub fn set_comparator(&self, comparator: TreeComparator<K>) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let old = std::mem::take(&mut guard.map);
                let mut map = BTreeMap::new();
                for (key, value) in old {
                    map.insert(
                        CmpKey {
                            key: key.key,
                            cmp: Arc::clone(&comparator),
                        },
                        value,
                    );
                }
                guard.comparator = comparator;
                guard.map = map;
            }

            /// Inserts or replaces a key-value pair; nil values are ignored.
            pub fn set(&self, key: K, value: V) {
                if value.is_nil() {
                    return;
                }
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                guard.map.insert(CmpKey { key, cmp }, value);
            }

            /// Inserts many pairs, ignoring nil values.
            pub fn sets(&self, data: impl IntoIterator<Item = (K, V)>) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                for (key, value) in data {
                    if value.is_nil() {
                        continue;
                    }
                    guard.map.insert(
                        CmpKey {
                            key,
                            cmp: Arc::clone(&cmp),
                        },
                        value,
                    );
                }
            }

            /// Sets `value` when `key` is absent and returns whether it was
            /// absent. A nil value is not stored but still reports `true`.
            pub fn set_if_not_exist(&self, key: K, value: V) -> bool {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                match guard.map.entry(CmpKey { key, cmp }) {
                    std::collections::btree_map::Entry::Occupied(_) => false,
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        if !value.is_nil() {
                            slot.insert(value);
                        }
                        true
                    }
                }
            }

            /// Sets the closure's result when `key` is absent and returns
            /// whether it was absent. The closure runs before the write lock.
            pub fn set_if_not_exist_func(&self, key: K, f: impl FnOnce() -> V) -> bool {
                if self.contains(&key) {
                    return false;
                }
                let value = f();
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                match guard.map.entry(CmpKey { key, cmp }) {
                    std::collections::btree_map::Entry::Occupied(_) => false,
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        if !value.is_nil() {
                            slot.insert(value);
                        }
                        true
                    }
                }
            }

            /// Go's lock-holding variant. The Rust port never runs the
            /// callback under the lock (ADR-007 divergence).
            pub fn set_if_not_exist_func_lock(&self, key: K, f: impl FnOnce() -> V) -> bool {
                self.set_if_not_exist_func(key, f)
            }

            /// Returns the value for `key`, or `None` when absent.
            pub fn get(&self, key: &K) -> Option<V> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let probe = CmpKey {
                    key: key.clone(),
                    cmp: Arc::clone(&guard.comparator),
                };
                guard.map.get(&probe).cloned()
            }

            /// Searches for `key`; `None` means it was not found.
            pub fn search(&self, key: &K) -> Option<V> {
                self.get(key)
            }

            /// Reports whether `key` exists.
            pub fn contains(&self, key: &K) -> bool {
                self.get(key).is_some()
            }

            /// Returns the value for `key`, inserting `value` when absent.
            pub fn get_or_set(&self, key: K, value: V) -> V {
                if let Some(current) = self.get(&key) {
                    return current;
                }
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                match guard.map.entry(CmpKey { key, cmp }) {
                    std::collections::btree_map::Entry::Occupied(slot) => slot.get().clone(),
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        if value.is_nil() {
                            return value;
                        }
                        slot.insert(value.clone());
                        value
                    }
                }
            }

            /// Returns the value for `key`, or inserts the closure's result.
            /// The closure runs before the write lock.
            pub fn get_or_set_func(&self, key: K, f: impl FnOnce() -> V) -> V {
                if let Some(current) = self.get(&key) {
                    return current;
                }
                let value = f();
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                match guard.map.entry(CmpKey { key, cmp }) {
                    std::collections::btree_map::Entry::Occupied(slot) => slot.get().clone(),
                    std::collections::btree_map::Entry::Vacant(slot) => {
                        if value.is_nil() {
                            return value;
                        }
                        slot.insert(value.clone());
                        value
                    }
                }
            }

            /// Go's lock-holding variant. The Rust port never runs the
            /// callback under the lock (ADR-007 divergence).
            pub fn get_or_set_func_lock(&self, key: K, f: impl FnOnce() -> V) -> V {
                self.get_or_set_func(key, f)
            }

            /// Removes `key` and returns its value.
            pub fn remove(&self, key: &K) -> Option<V> {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let probe = CmpKey {
                    key: key.clone(),
                    cmp: Arc::clone(&guard.comparator),
                };
                guard.map.remove(&probe)
            }

            /// Removes many keys.
            pub fn removes(&self, keys: impl IntoIterator<Item = K>) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                for key in keys {
                    let probe = CmpKey {
                        key,
                        cmp: Arc::clone(&cmp),
                    };
                    guard.map.remove(&probe);
                }
            }

            /// Removes every entry.
            pub fn clear(&self) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.clear();
            }

            /// Returns the number of entries.
            pub fn size(&self) -> usize {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.len()
            }

            /// Reports whether the tree is empty.
            pub fn is_empty(&self) -> bool {
                self.size() == 0
            }

            /// Returns the keys in ascending comparator order.
            pub fn keys(&self) -> Vec<K> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.keys().map(|entry| entry.key.clone()).collect()
            }

            /// Returns the values ordered by their keys.
            pub fn values(&self) -> Vec<V> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.values().cloned().collect()
            }

            /// Clears the tree and sets the given pairs.
            pub fn replace(&self, data: impl IntoIterator<Item = (K, V)>) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.clear();
                let cmp = Arc::clone(&guard.comparator);
                for (key, value) in data {
                    if value.is_nil() {
                        continue;
                    }
                    guard.map.insert(
                        CmpKey {
                            key,
                            cmp: Arc::clone(&cmp),
                        },
                        value,
                    );
                }
            }

            /// Returns every pair as an insertion-ordered map, in ascending
            /// key order.
            ///
            /// `IndexMap` needs a hashable key; use [`Self::entries`] for an
            /// ordered snapshot of any key type, including [`Value`].
            pub fn map(&self) -> IndexMap<K, V>
            where
                K: std::hash::Hash + Eq,
            {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut out = IndexMap::new();
                for (key, value) in &guard.map {
                    out.insert(key.key.clone(), value.clone());
                }
                out
            }

            /// Returns every pair in ascending key order as a vector.
            ///
            /// This is the hash-free counterpart of [`Self::map`] and is what
            /// the dynamic (any/any) aliases use where Go returns `Map()`.
            pub fn entries(&self) -> Vec<(K, V)> {
                self.snapshot()
            }

            /// Returns a locked snapshot of every pair in ascending order.
            fn snapshot(&self) -> Vec<(K, V)> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard
                    .map
                    .iter()
                    .map(|(key, value)| (key.key.clone(), value.clone()))
                    .collect()
            }

            /// Iterates in ascending order; the callback runs on a snapshot.
            pub fn iterator<F>(&self, f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                self.iterator_asc(f);
            }

            /// Iterates in ascending order; returning `false` stops.
            pub fn iterator_asc<F>(&self, mut f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                let snapshot = self.snapshot();
                for (key, value) in snapshot {
                    if !f(key, value) {
                        break;
                    }
                }
            }

            /// Iterates in descending order; returning `false` stops.
            pub fn iterator_desc<F>(&self, mut f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                let snapshot = self.snapshot();
                for (key, value) in snapshot.into_iter().rev() {
                    if !f(key, value) {
                        break;
                    }
                }
            }

            /// Alias of [`Self::iterator_asc_from`].
            pub fn iterator_from<F>(&self, key: &K, match_key: bool, f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                self.iterator_asc_from(key, match_key, f);
            }

            /// Iterates ascending from `key`.
            ///
            /// With `match_key` the key is looked up with the comparator;
            /// without it the key is treated as an integer index into the
            /// current keys, exactly like Go's `iteratorFromGetIndexT`.
            pub fn iterator_asc_from<F>(&self, key: &K, match_key: bool, mut f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                let snapshot = self.snapshot();
                let start = if match_key {
                    let guard = self
                        .inner
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let cmp = Arc::clone(&guard.comparator);
                    snapshot
                        .iter()
                        .position(|(current, _)| cmp(current, key) == 0)
                } else {
                    key.tree_index()
                };
                let Some(start) = start else {
                    return;
                };
                for (current, value) in snapshot.into_iter().skip(start) {
                    if !f(current, value) {
                        break;
                    }
                }
            }

            /// Iterates descending from `key`.
            ///
            /// With `match_key` the key is looked up with the comparator;
            /// without it the key is treated as an integer index into the
            /// current keys, exactly like Go's `iteratorFromGetIndexT`.
            pub fn iterator_desc_from<F>(&self, key: &K, match_key: bool, mut f: F)
            where
                F: FnMut(K, V) -> bool,
            {
                let snapshot = self.snapshot();
                let start = if match_key {
                    let guard = self
                        .inner
                        .read()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let cmp = Arc::clone(&guard.comparator);
                    snapshot
                        .iter()
                        .position(|(current, _)| cmp(current, key) == 0)
                } else {
                    key.tree_index()
                };
                let Some(mut index) = start else {
                    return;
                };
                if snapshot.is_empty() {
                    return;
                }
                index = index.min(snapshot.len() - 1);
                loop {
                    let (current, value) = snapshot[index].clone();
                    if !f(current, value) {
                        break;
                    }
                    if index == 0 {
                        break;
                    }
                    index -= 1;
                }
            }

            /// Returns the smallest node, or `None` when the tree is empty.
            pub fn left(&self) -> Option<$node<K, V>> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.first_key_value().map(|(key, value)| $node {
                    key: key.key.clone(),
                    value: value.clone(),
                })
            }

            /// Returns the largest node, or `None` when the tree is empty.
            pub fn right(&self) -> Option<$node<K, V>> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.last_key_value().map(|(key, value)| $node {
                    key: key.key.clone(),
                    value: value.clone(),
                })
            }

            /// Removes and returns the smallest node, like Go's `PopLeft`.
            ///
            /// Go exposes this on the red-black trees only; the shared ordered
            /// implementation offers it on the AVL tree too.
            pub fn pop_left(&self) -> Option<$node<K, V>> {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.pop_first().map(|(key, value)| $node {
                    key: key.key,
                    value,
                })
            }

            /// Removes and returns the largest node, like Go's `PopRight`.
            ///
            /// Go exposes this on the red-black trees only; the shared ordered
            /// implementation offers it on the AVL tree too.
            pub fn pop_right(&self) -> Option<$node<K, V>> {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                guard.map.pop_last().map(|(key, value)| $node {
                    key: key.key,
                    value,
                })
            }

            /// Returns the largest node whose key is less than or equal to
            /// `key`.
            pub fn floor(&self, key: &K) -> Option<$node<K, V>> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let probe = CmpKey {
                    key: key.clone(),
                    cmp: Arc::clone(&guard.comparator),
                };
                guard
                    .map
                    .range(..=probe)
                    .next_back()
                    .map(|(key, value)| $node {
                        key: key.key.clone(),
                        value: value.clone(),
                    })
            }

            /// Returns the smallest node whose key is greater than or equal to
            /// `key`.
            pub fn ceiling(&self, key: &K) -> Option<$node<K, V>> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let probe = CmpKey {
                    key: key.clone(),
                    cmp: Arc::clone(&guard.comparator),
                };
                guard.map.range(probe..).next().map(|(key, value)| $node {
                    key: key.key.clone(),
                    value: value.clone(),
                })
            }

            /// Returns the in-order listing used by Go's `String`.
            pub fn string(&self) -> String {
                balanced_listing(&self.keys())
            }

            /// Prints [`Self::string`] to stdout with a trailing newline.
            pub fn print(&self) {
                println!("{}", self.string());
            }
        }

        impl<K> $tree<K, Value>
        where
            K: TreeKey,
        {
            /// Returns every pair keyed by the string form of the key.
            pub fn map_str_any(&self) -> IndexMap<String, Value> {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let mut out = IndexMap::new();
                for (key, value) in &guard.map {
                    out.insert(key.key.map_key_string(), value.clone());
                }
                out
            }

            /// Encodes the tree as JSON text, like Go's `MarshalJSON`.
            pub fn marshal_json(&self) -> Result<String, gf_errors::gerror::Error> {
                let map = self.map_str_any();
                gf_json::marshal_value(&Value::Map(map))
            }

            /// Returns the value for `key` wrapped in a [`Var`].
            pub fn get_var(&self, key: &K) -> Var {
                Var::new(self.get(key).unwrap_or(Value::Null))
            }

            /// Returns the value for `key`, inserting `value` when absent.
            pub fn get_var_or_set(&self, key: K, value: Value) -> Var {
                Var::new(self.get_or_set(key, value))
            }

            /// Returns the value for `key`, or the closure's result, wrapped
            /// in a [`Var`].
            pub fn get_var_or_set_func(&self, key: K, f: impl FnOnce() -> Value) -> Var {
                Var::new(self.get_or_set_func(key, f))
            }

            /// Go's lock-holding variant, wrapped in a [`Var`]. The callback
            /// never runs under the lock (ADR-007 divergence).
            pub fn get_var_or_set_func_lock(&self, key: K, f: impl FnOnce() -> Value) -> Var {
                Var::new(self.get_or_set_func_lock(key, f))
            }
        }

        impl $tree<Value, Value> {
            /// Exchanges keys and values, using `comparator` when supplied.
            pub fn flip(&self, comparator: Option<TreeComparator<Value>>) {
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = match comparator {
                    Some(comparator) => comparator,
                    None => Arc::clone(&guard.comparator),
                };
                let old = std::mem::take(&mut guard.map);
                let mut map = BTreeMap::new();
                for (key, value) in old {
                    if key.key.is_nil() {
                        continue;
                    }
                    map.insert(
                        CmpKey {
                            key: value,
                            cmp: Arc::clone(&cmp),
                        },
                        key.key,
                    );
                }
                guard.comparator = cmp;
                guard.map = map;
            }

            /// Adds the object's entries, like Go's `UnmarshalJSON`.
            ///
            /// Go keeps the existing entries and converts each JSON key with
            /// its default comparator; the port keeps the entries as well and
            /// uses the string form of each key, which is the identity for the
            /// dynamic [`Value`] key type.
            pub fn unmarshal_json(&self, data: &[u8]) -> Result<(), gf_errors::gerror::Error> {
                let value = gf_json::unmarshal_value(data)?;
                self.unmarshal_value(&value)
            }

            /// Adds the entries of a [`Value::Map`], like Go's `UnmarshalValue`.
            pub fn unmarshal_value(&self, value: &Value) -> Result<(), gf_errors::gerror::Error> {
                let Value::Map(entries) = value else {
                    return Err(gf_errors::gerror::new(
                        "tree: cannot unmarshal non-object JSON into a tree",
                    ));
                };
                let mut guard = self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let cmp = Arc::clone(&guard.comparator);
                for (key, value) in entries {
                    if value.is_nil() {
                        continue;
                    }
                    guard.map.insert(
                        CmpKey {
                            key: Value::String(key.clone()),
                            cmp: Arc::clone(&cmp),
                        },
                        value.clone(),
                    );
                }
                Ok(())
            }
        }

        impl<K, V> Clone for $tree<K, V>
        where
            K: Clone,
            V: Clone,
        {
            fn clone(&self) -> Self {
                let guard = self
                    .inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                $tree {
                    inner: RwLock::new(guard.clone()),
                }
            }
        }
    };
}

impl_ordered_tree!(AVLKVTree, AVLKVTreeNode);
impl_ordered_tree!(RedBlackKVTree, RedBlackKVTreeNode);

/// An AVL tree node.
#[derive(Clone)]
pub struct AVLKVTreeNode<K, V> {
    /// The node's key.
    pub key: K,
    /// The node's value.
    pub value: V,
}

/// An AVL tree over a comparator-ordered key.
pub struct AVLKVTree<K, V> {
    inner: RwLock<OrderedState<K, V>>,
}

/// `AVLTree` with dynamic keys and values.
pub type AVLTree = AVLKVTree<Value, Value>;
/// `AVLTreeNode` with dynamic keys and values.
pub type AVLTreeNode = AVLKVTreeNode<Value, Value>;

/// A red-black tree node.
#[derive(Clone)]
pub struct RedBlackKVTreeNode<K, V> {
    /// The node's key.
    pub key: K,
    /// The node's value.
    pub value: V,
}

/// A red-black tree over a comparator-ordered key.
pub struct RedBlackKVTree<K, V> {
    inner: RwLock<OrderedState<K, V>>,
}

/// `RedBlackTree` with dynamic keys and values.
pub type RedBlackTree = RedBlackKVTree<Value, Value>;
/// `RedBlackTreeNode` with dynamic keys and values.
pub type RedBlackTreeNode = RedBlackKVTreeNode<Value, Value>;

/// A B-tree entry.
#[derive(Clone)]
pub struct BKVTreeEntry<K, V> {
    /// The entry's key.
    pub key: K,
    /// The entry's value.
    pub value: V,
}

/// A single B-tree node: entries plus child indices in the arena.
#[derive(Clone)]
struct BNode<K, V> {
    entries: Vec<BKVTreeEntry<K, V>>,
    children: Vec<usize>,
    parent: Option<usize>,
}

/// The B-tree storage: an arena of nodes plus the order and comparator.
#[derive(Clone)]
struct BTreeState<K, V> {
    order: usize,
    root: Option<usize>,
    nodes: Vec<BNode<K, V>>,
    size: usize,
    comparator: TreeComparator<K>,
}

impl<K, V> BTreeState<K, V>
where
    K: Clone + ToString,
    V: TreeValue,
{
    /// Returns the maximum number of children.
    fn max_children(&self) -> usize {
        self.order
    }

    /// Returns the minimum number of children of a non-root node.
    fn min_children(&self) -> usize {
        self.order.div_ceil(2)
    }

    /// Returns the maximum number of entries in a node.
    fn max_entries(&self) -> usize {
        self.max_children() - 1
    }

    /// Returns the minimum number of entries of a non-root node.
    fn min_entries(&self) -> usize {
        self.min_children() - 1
    }

    /// Returns the split position that favors right nodes with more keys.
    fn middle(&self) -> usize {
        (self.order - 1) / 2
    }

    /// Allocates a node and returns its arena index.
    fn alloc_node(
        &mut self,
        entries: Vec<BKVTreeEntry<K, V>>,
        children: Vec<usize>,
        parent: Option<usize>,
    ) -> usize {
        let index = self.nodes.len();
        self.nodes.push(BNode {
            entries,
            children,
            parent,
        });
        index
    }

    /// Reports whether a node is a leaf.
    fn is_leaf(&self, node: usize) -> bool {
        self.nodes[node].children.is_empty()
    }

    /// Searches one node, returning the insert position and whether the key
    /// was found.
    fn search(&self, node: usize, key: &K) -> (usize, bool) {
        let entries = &self.nodes[node].entries;
        let mut low: isize = 0;
        let mut high: isize = entries.len() as isize - 1;
        while low <= high {
            let middle = ((low + high) / 2) as usize;
            let ordering = (self.comparator)(key, &entries[middle].key);
            if ordering > 0 {
                low = middle as isize + 1;
            } else if ordering < 0 {
                high = middle as isize - 1;
            } else {
                return (middle, true);
            }
        }
        (low as usize, false)
    }

    /// Searches the tree recursively, returning the node and entry index.
    fn search_recursively(&self, key: &K) -> Option<(usize, usize)> {
        let mut node = self.root?;
        loop {
            let (index, found) = self.search(node, key);
            if found {
                return Some((node, index));
            }
            if self.is_leaf(node) {
                return None;
            }
            node = *self.nodes[node].children.get(index)?;
        }
    }

    /// Returns the value for `key`, or `None` when absent.
    fn get(&self, key: &K) -> Option<V> {
        let (node, index) = self.search_recursively(key)?;
        Some(self.nodes[node].entries[index].value.clone())
    }

    /// Inserts or replaces a pair.
    fn put(&mut self, key: K, value: V) {
        let entry = BKVTreeEntry { key, value };
        match self.root {
            None => {
                let root = self.alloc_node(vec![entry], Vec::new(), None);
                self.root = Some(root);
                self.size += 1;
            }
            Some(root) => {
                if self.insert(root, entry) {
                    self.size += 1;
                }
            }
        }
    }

    /// Inserts a pair below `node`.
    fn insert(&mut self, node: usize, entry: BKVTreeEntry<K, V>) -> bool {
        if self.is_leaf(node) {
            self.insert_into_leaf(node, entry)
        } else {
            self.insert_into_internal(node, entry)
        }
    }

    /// Inserts a pair into a leaf, splitting it when full.
    fn insert_into_leaf(&mut self, node: usize, entry: BKVTreeEntry<K, V>) -> bool {
        let (position, found) = self.search(node, &entry.key);
        if found {
            self.nodes[node].entries[position] = entry;
            return false;
        }
        self.nodes[node].entries.insert(position, entry);
        self.split(node);
        true
    }

    /// Inserts a pair through an internal node.
    fn insert_into_internal(&mut self, node: usize, entry: BKVTreeEntry<K, V>) -> bool {
        let (position, found) = self.search(node, &entry.key);
        if found {
            self.nodes[node].entries[position] = entry;
            return false;
        }
        let child = match self.nodes[node].children.get(position) {
            Some(child) => *child,
            None => return false,
        };
        self.insert(child, entry)
    }

    /// Splits a node when it has too many entries.
    fn split(&mut self, node: usize) {
        if self.nodes[node].entries.len() <= self.max_entries() {
            return;
        }
        if self.root == Some(node) {
            self.split_root();
        } else {
            self.split_non_root(node);
        }
    }

    /// Splits a non-root node and promotes its middle entry.
    fn split_non_root(&mut self, node: usize) {
        let middle = self.middle();
        let parent = match self.nodes[node].parent {
            Some(parent) => parent,
            None => return,
        };
        let mut entries = std::mem::take(&mut self.nodes[node].entries);
        let right_entries = entries.split_off(middle + 1);
        let middle_entry = match entries.pop() {
            Some(entry) => entry,
            None => return,
        };
        let mut children = std::mem::take(&mut self.nodes[node].children);
        let right_children = if children.is_empty() {
            Vec::new()
        } else {
            children.split_off(middle + 1)
        };
        self.nodes[node].entries = entries;
        self.nodes[node].children = children;
        self.nodes[node].parent = Some(parent);
        let right = self.alloc_node(right_entries, right_children, Some(parent));
        for child in self.nodes[node].children.clone() {
            self.nodes[child].parent = Some(node);
        }
        for child in self.nodes[right].children.clone() {
            self.nodes[child].parent = Some(right);
        }
        let (position, _) = self.search(parent, &middle_entry.key);
        self.nodes[parent].entries.insert(position, middle_entry);
        if position < self.nodes[parent].children.len() {
            self.nodes[parent].children[position] = node;
        }
        self.nodes[parent].children.insert(position + 1, right);
        self.split(parent);
    }

    /// Splits the root and creates a new root holding the middle entry.
    fn split_root(&mut self) {
        let root = match self.root {
            Some(root) => root,
            None => return,
        };
        let middle = self.middle();
        let mut entries = std::mem::take(&mut self.nodes[root].entries);
        let right_entries = entries.split_off(middle + 1);
        let middle_entry = match entries.pop() {
            Some(entry) => entry,
            None => return,
        };
        let mut children = std::mem::take(&mut self.nodes[root].children);
        let right_children = if children.is_empty() {
            Vec::new()
        } else {
            children.split_off(middle + 1)
        };
        self.nodes[root].entries = entries;
        self.nodes[root].children = children;
        self.nodes[root].parent = None;
        let right = self.alloc_node(right_entries, right_children, None);
        for child in self.nodes[root].children.clone() {
            self.nodes[child].parent = Some(root);
        }
        for child in self.nodes[right].children.clone() {
            self.nodes[child].parent = Some(right);
        }
        let new_root = self.alloc_node(vec![middle_entry], vec![root, right], None);
        self.nodes[root].parent = Some(new_root);
        self.nodes[right].parent = Some(new_root);
        self.root = Some(new_root);
    }

    /// Removes a pair, returning its value.
    fn remove(&mut self, key: &K) -> Option<V> {
        let (node, index) = self.search_recursively(key)?;
        let value = self.nodes[node].entries[index].value.clone();
        self.delete(node, index);
        self.size = self.size.saturating_sub(1);
        Some(value)
    }

    /// Deletes the entry at `index` from `node`, rebalancing as needed.
    fn delete(&mut self, node: usize, index: usize) {
        if self.is_leaf(node) {
            let deleted_key = self.nodes[node].entries[index].key.clone();
            self.delete_entry(node, index);
            self.rebalance(Some(node), &deleted_key);
            if let Some(root) = self.root
                && self.nodes[root].entries.is_empty()
            {
                self.root = None;
            }
            return;
        }
        let child = match self.nodes[node].children.get(index) {
            Some(child) => *child,
            None => return,
        };
        let largest = self.right_from(child);
        let last_index = match self.nodes[largest].entries.len().checked_sub(1) {
            Some(index) => index,
            None => return,
        };
        let moved = self.nodes[largest].entries[last_index].clone();
        let deleted_key = moved.key.clone();
        self.nodes[node].entries[index] = moved;
        self.delete_entry(largest, last_index);
        self.rebalance(Some(largest), &deleted_key);
    }

    /// Rebalances after deleting `deleted_key` from `node`.
    fn rebalance(&mut self, node: Option<usize>, deleted_key: &K) {
        let Some(node) = node else {
            return;
        };
        if self.nodes[node].entries.len() >= self.min_entries() {
            return;
        }
        let minimum = self.min_entries();

        if let Some((left, left_index)) = self.left_sibling(node, deleted_key)
            && self.nodes[left].entries.len() > minimum
        {
            let parent = match self.nodes[node].parent {
                Some(parent) => parent,
                None => return,
            };
            let separator = self.nodes[parent].entries[left_index].clone();
            let borrowed = match self.nodes[left].entries.last() {
                Some(entry) => entry.clone(),
                None => return,
            };
            self.nodes[node].entries.insert(0, separator);
            self.nodes[parent].entries[left_index] = borrowed;
            self.nodes[left].entries.pop();
            if !self.is_leaf(left) {
                let child = match self.nodes[left].children.pop() {
                    Some(child) => child,
                    None => return,
                };
                self.nodes[child].parent = Some(node);
                self.nodes[node].children.insert(0, child);
            }
            return;
        }

        if let Some((right, right_index)) = self.right_sibling(node, deleted_key)
            && self.nodes[right].entries.len() > minimum
        {
            let parent = match self.nodes[node].parent {
                Some(parent) => parent,
                None => return,
            };
            let separator = self.nodes[parent].entries[right_index - 1].clone();
            let borrowed = match self.nodes[right].entries.first() {
                Some(entry) => entry.clone(),
                None => return,
            };
            self.nodes[node].entries.push(separator);
            self.nodes[parent].entries[right_index - 1] = borrowed;
            self.nodes[right].entries.remove(0);
            if !self.is_leaf(right) {
                let child = match self.nodes[right].children.first().copied() {
                    Some(child) => child,
                    None => return,
                };
                self.nodes[right].children.remove(0);
                self.nodes[child].parent = Some(node);
                self.nodes[node].children.push(child);
            }
            return;
        }

        let mut next_deleted = deleted_key.clone();

        if let Some((right, right_index)) = self.right_sibling(node, deleted_key) {
            let parent = match self.nodes[node].parent {
                Some(parent) => parent,
                None => return,
            };
            let separator = self.nodes[parent].entries[right_index - 1].clone();
            let right_entries = std::mem::take(&mut self.nodes[right].entries);
            self.nodes[node].entries.push(separator);
            self.nodes[node].entries.extend(right_entries);
            next_deleted = self.nodes[parent].entries[right_index - 1].key.clone();
            self.delete_entry(parent, right_index - 1);
            let right_children = std::mem::take(&mut self.nodes[right].children);
            for child in right_children.iter().copied() {
                self.nodes[child].parent = Some(node);
            }
            self.nodes[node].children.extend(right_children);
            self.delete_child(parent, right_index);
        } else if let Some((left, left_index)) = self.left_sibling(node, deleted_key) {
            let parent = match self.nodes[node].parent {
                Some(parent) => parent,
                None => return,
            };
            let mut merged = std::mem::take(&mut self.nodes[left].entries);
            let separator = self.nodes[parent].entries[left_index].clone();
            merged.push(separator);
            let node_entries = std::mem::take(&mut self.nodes[node].entries);
            merged.extend(node_entries);
            self.nodes[node].entries = merged;
            next_deleted = self.nodes[parent].entries[left_index].key.clone();
            self.delete_entry(parent, left_index);
            let mut children = std::mem::take(&mut self.nodes[left].children);
            let node_children = std::mem::take(&mut self.nodes[node].children);
            for child in children.iter().copied() {
                self.nodes[child].parent = Some(node);
            }
            children.extend(node_children);
            self.nodes[node].children = children;
            self.delete_child(parent, left_index);
        }

        if let Some(root) = self.root
            && self.nodes[node].parent == Some(root)
            && self.nodes[root].entries.is_empty()
        {
            self.root = Some(node);
            self.nodes[node].parent = None;
            return;
        }

        let parent = self.nodes[node].parent;
        self.rebalance(parent, &next_deleted);
    }

    /// Finds the left sibling of `node` and its index in the parent.
    fn left_sibling(&self, node: usize, key: &K) -> Option<(usize, usize)> {
        let parent = self.nodes[node].parent?;
        let (position, _) = self.search(parent, key);
        if position == 0 {
            return None;
        }
        let index = position - 1;
        let sibling = *self.nodes[parent].children.get(index)?;
        Some((sibling, index))
    }

    /// Finds the right sibling of `node` and its index in the parent.
    fn right_sibling(&self, node: usize, key: &K) -> Option<(usize, usize)> {
        let parent = self.nodes[node].parent?;
        let (position, _) = self.search(parent, key);
        let index = position + 1;
        let sibling = *self.nodes[parent].children.get(index)?;
        Some((sibling, index))
    }

    /// Removes an entry from a node.
    fn delete_entry(&mut self, node: usize, index: usize) {
        if index < self.nodes[node].entries.len() {
            self.nodes[node].entries.remove(index);
        }
    }

    /// Removes a child from a node.
    fn delete_child(&mut self, node: usize, index: usize) {
        if index < self.nodes[node].children.len() {
            self.nodes[node].children.remove(index);
        }
    }

    /// Returns the right-most leaf of the subtree rooted at `node`.
    fn right_from(&self, node: usize) -> usize {
        let mut current = node;
        while !self.nodes[current].children.is_empty() {
            let last = self.nodes[current].children.len() - 1;
            current = self.nodes[current].children[last];
        }
        current
    }

    /// Returns the left-most leaf, or `None` when the tree is empty.
    fn left_node(&self) -> Option<usize> {
        let mut node = self.root?;
        while !self.nodes[node].children.is_empty() {
            node = self.nodes[node].children[0];
        }
        Some(node)
    }

    /// Returns the right-most leaf, or `None` when the tree is empty.
    fn right_node(&self) -> Option<usize> {
        let mut node = self.root?;
        while !self.nodes[node].children.is_empty() {
            let last = self.nodes[node].children.len() - 1;
            node = self.nodes[node].children[last];
        }
        Some(node)
    }

    /// Returns the tree height; `0` for an empty tree.
    fn height(&self) -> usize {
        let mut node = match self.root {
            Some(root) => root,
            None => return 0,
        };
        let mut height = 0;
        loop {
            height += 1;
            if self.nodes[node].children.is_empty() {
                break;
            }
            node = self.nodes[node].children[0];
        }
        height
    }

    /// Collects every pair in ascending key order.
    fn collect_entries(&self) -> Vec<(K, V)> {
        let mut out = Vec::with_capacity(self.size);
        if let Some(root) = self.root {
            self.collect_node(root, &mut out);
        }
        out
    }

    /// Appends the subtree rooted at `node` in key order.
    fn collect_node(&self, node: usize, out: &mut Vec<(K, V)>) {
        let entries = self.nodes[node].entries.len();
        let children = self.nodes[node].children.len();
        for index in 0..=entries {
            if index < children {
                self.collect_node(self.nodes[node].children[index], out);
            }
            if index < entries {
                let entry = &self.nodes[node].entries[index];
                out.push((entry.key.clone(), entry.value.clone()));
            }
        }
    }

    /// Returns the keys in ascending order.
    fn keys(&self) -> Vec<K> {
        self.collect_entries()
            .into_iter()
            .map(|(key, _)| key)
            .collect()
    }

    /// Returns the values ordered by their keys.
    fn values(&self) -> Vec<V> {
        self.collect_entries()
            .into_iter()
            .map(|(_, value)| value)
            .collect()
    }

    /// Writes the `gods` in-order node listing.
    fn write_output(&self, node: usize, level: usize, out: &mut String) {
        let entries = self.nodes[node].entries.len();
        let children = self.nodes[node].children.len();
        for index in 0..=entries {
            if index < children {
                self.write_output(self.nodes[node].children[index], level + 1, out);
            }
            if index < entries {
                for _ in 0..level {
                    out.push_str("    ");
                }
                out.push_str(&self.nodes[node].entries[index].key.to_string());
                out.push('\n');
            }
        }
    }

    /// Returns the `gods` string form without its `BTree\n` prefix.
    fn string(&self) -> String {
        let mut out = String::new();
        if let Some(root) = self.root {
            self.write_output(root, 0, &mut out);
        }
        out
    }
}

/// A B-tree over a comparator-ordered key, ported from `gods`.
pub struct BKVTree<K, V> {
    inner: RwLock<BTreeState<K, V>>,
}

/// `BTree` with dynamic keys and values.
pub type BTree = BKVTree<Value, Value>;
/// `BTreeEntry` with dynamic keys and values.
pub type BTreeEntry = BKVTreeEntry<Value, Value>;

impl<K, V> BKVTree<K, V>
where
    K: TreeKey,
    V: TreeValue + Send + Sync + 'static,
{
    /// Creates an empty B-tree with `order` (the maximum number of children).
    ///
    /// Panics when `order < 3`, like Go.
    pub fn new(order: usize, comparator: Option<TreeComparator<K>>) -> Self {
        if order < 3 {
            panic!("m must be greater than or equal to 3");
        }
        BKVTree {
            inner: RwLock::new(BTreeState {
                order,
                root: None,
                nodes: Vec::new(),
                size: 0,
                comparator: resolve_comparator(comparator),
            }),
        }
    }

    /// Creates a B-tree from key-value pairs, ignoring nil values.
    pub fn new_from(
        order: usize,
        comparator: Option<TreeComparator<K>>,
        data: impl IntoIterator<Item = (K, V)>,
    ) -> Self {
        let tree = Self::new(order, comparator);
        tree.sets(data);
        tree
    }

    /// Inserts or replaces a key-value pair; nil values are ignored.
    pub fn set(&self, key: K, value: V) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !value.is_nil() {
            guard.put(key, value);
        }
    }

    /// Inserts many pairs, ignoring nil values.
    pub fn sets(&self, data: impl IntoIterator<Item = (K, V)>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, value) in data {
            if !value.is_nil() {
                guard.put(key, value);
            }
        }
    }

    /// Sets `value` when `key` is absent and returns whether it was absent.
    pub fn set_if_not_exist(&self, key: K, value: V) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.get(&key).is_some() {
            return false;
        }
        if !value.is_nil() {
            guard.put(key, value);
        }
        true
    }

    /// Sets the closure's result when `key` is absent. The closure runs
    /// before the write lock.
    pub fn set_if_not_exist_func(&self, key: K, f: impl FnOnce() -> V) -> bool {
        if self.contains(&key) {
            return false;
        }
        let value = f();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.get(&key).is_some() {
            return false;
        }
        if !value.is_nil() {
            guard.put(key, value);
        }
        true
    }

    /// Go's lock-holding variant. The Rust port never runs the callback under
    /// the lock (ADR-007 divergence).
    pub fn set_if_not_exist_func_lock(&self, key: K, f: impl FnOnce() -> V) -> bool {
        self.set_if_not_exist_func(key, f)
    }

    /// Returns the value for `key`, or `None` when absent.
    pub fn get(&self, key: &K) -> Option<V> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.get(key)
    }

    /// Searches for `key`; `None` means it was not found.
    pub fn search(&self, key: &K) -> Option<V> {
        self.get(key)
    }

    /// Reports whether `key` exists.
    pub fn contains(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    /// Returns the value for `key`, inserting `value` when absent.
    pub fn get_or_set(&self, key: K, value: V) -> V {
        if let Some(current) = self.get(&key) {
            return current;
        }
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = guard.get(&key) {
            return current;
        }
        if value.is_nil() {
            return value;
        }
        guard.put(key, value.clone());
        value
    }

    /// Returns the value for `key`, or inserts the closure's result. The
    /// closure runs before the write lock.
    pub fn get_or_set_func(&self, key: K, f: impl FnOnce() -> V) -> V {
        if let Some(current) = self.get(&key) {
            return current;
        }
        let value = f();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(current) = guard.get(&key) {
            return current;
        }
        if value.is_nil() {
            return value;
        }
        guard.put(key, value.clone());
        value
    }

    /// Go's lock-holding variant. The Rust port never runs the callback under
    /// the lock (ADR-007 divergence).
    pub fn get_or_set_func_lock(&self, key: K, f: impl FnOnce() -> V) -> V {
        self.get_or_set_func(key, f)
    }

    /// Removes `key` and returns its value.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.remove(key)
    }

    /// Removes many keys.
    pub fn removes(&self, keys: impl IntoIterator<Item = K>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for key in keys {
            guard.remove(&key);
        }
    }

    /// Removes every entry.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.root = None;
        guard.nodes.clear();
        guard.size = 0;
    }

    /// Returns the keys in ascending order.
    pub fn keys(&self) -> Vec<K> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.keys()
    }

    /// Returns the values ordered by their keys.
    pub fn values(&self) -> Vec<V> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.values()
    }

    /// Clears the tree and sets the given pairs.
    pub fn replace(&self, data: impl IntoIterator<Item = (K, V)>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.root = None;
        guard.nodes.clear();
        guard.size = 0;
        for (key, value) in data {
            if !value.is_nil() {
                guard.put(key, value);
            }
        }
    }

    /// Returns every pair as an insertion-ordered map, in ascending order.
    ///
    /// `IndexMap` needs a hashable key; use [`Self::entries`] for an ordered
    /// snapshot of any key type, including [`Value`].
    pub fn map(&self) -> IndexMap<K, V>
    where
        K: std::hash::Hash + Eq,
    {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out = IndexMap::new();
        for (key, value) in guard.collect_entries() {
            out.insert(key, value);
        }
        out
    }

    /// Returns every pair in ascending order as a vector.
    ///
    /// This is the hash-free counterpart of [`Self::map`] and is what the
    /// dynamic (any/any) alias uses where Go returns `Map()`.
    pub fn entries(&self) -> Vec<(K, V)> {
        self.snapshot()
    }

    /// Returns a locked snapshot of every pair in ascending order.
    fn snapshot(&self) -> Vec<(K, V)> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.collect_entries()
    }

    /// Iterates in ascending order; the callback runs on a snapshot.
    pub fn iterator<F>(&self, f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        self.iterator_asc(f);
    }

    /// Iterates in ascending order; returning `false` stops.
    pub fn iterator_asc<F>(&self, mut f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        let snapshot = self.snapshot();
        for (key, value) in snapshot {
            if !f(key, value) {
                break;
            }
        }
    }

    /// Iterates in descending order; returning `false` stops.
    pub fn iterator_desc<F>(&self, mut f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        let snapshot = self.snapshot();
        for (key, value) in snapshot.into_iter().rev() {
            if !f(key, value) {
                break;
            }
        }
    }

    /// Alias of [`Self::iterator_asc_from`].
    pub fn iterator_from<F>(&self, key: &K, match_key: bool, f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        self.iterator_asc_from(key, match_key, f);
    }

    /// Iterates ascending from `key`.
    ///
    /// With `match_key` the key is looked up with the comparator; without it
    /// the key is treated as an integer index into the current keys.
    pub fn iterator_asc_from<F>(&self, key: &K, match_key: bool, mut f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        let snapshot = self.snapshot();
        let start = if match_key {
            let guard = self
                .inner
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let comparator = Arc::clone(&guard.comparator);
            snapshot
                .iter()
                .position(|(current, _)| comparator(current, key) == 0)
        } else {
            key.tree_index()
        };
        let Some(start) = start else {
            return;
        };
        for (current, value) in snapshot.into_iter().skip(start) {
            if !f(current, value) {
                break;
            }
        }
    }

    /// Iterates descending from `key`.
    ///
    /// With `match_key` the key is looked up with the comparator; without it
    /// the key is treated as an integer index into the current keys.
    pub fn iterator_desc_from<F>(&self, key: &K, match_key: bool, mut f: F)
    where
        F: FnMut(K, V) -> bool,
    {
        let snapshot = self.snapshot();
        let start = if match_key {
            let guard = self
                .inner
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let comparator = Arc::clone(&guard.comparator);
            snapshot
                .iter()
                .position(|(current, _)| comparator(current, key) == 0)
        } else {
            key.tree_index()
        };
        let Some(mut index) = start else {
            return;
        };
        if snapshot.is_empty() {
            return;
        }
        index = index.min(snapshot.len() - 1);
        loop {
            let (current, value) = snapshot[index].clone();
            if !f(current, value) {
                break;
            }
            if index == 0 {
                break;
            }
            index -= 1;
        }
    }

    /// Returns the number of entries.
    pub fn size(&self) -> usize {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.size
    }

    /// Reports whether the tree is empty.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Returns the height of the tree; `0` when empty.
    pub fn height(&self) -> usize {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.height()
    }

    /// Returns the smallest entry, or `None` when the tree is empty.
    pub fn left(&self) -> Option<BKVTreeEntry<K, V>> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let node = guard.left_node()?;
        let entry = guard.nodes[node].entries.first()?;
        Some(BKVTreeEntry {
            key: entry.key.clone(),
            value: entry.value.clone(),
        })
    }

    /// Returns the largest entry, or `None` when the tree is empty.
    pub fn right(&self) -> Option<BKVTreeEntry<K, V>> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let node = guard.right_node()?;
        let entry = guard.nodes[node].entries.last()?;
        Some(BKVTreeEntry {
            key: entry.key.clone(),
            value: entry.value.clone(),
        })
    }

    /// Returns the `gods` in-order node listing.
    pub fn string(&self) -> String {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.string()
    }

    /// Prints [`Self::string`] to stdout with a trailing newline.
    pub fn print(&self) {
        println!("{}", self.string());
    }
}

impl<K> BKVTree<K, Value>
where
    K: TreeKey,
{
    /// Returns every pair keyed by the string form of the key.
    pub fn map_str_any(&self) -> IndexMap<String, Value> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut out = IndexMap::new();
        for (key, value) in guard.collect_entries() {
            out.insert(key.map_key_string(), value);
        }
        out
    }

    /// Encodes the tree as JSON text, like Go's `MarshalJSON`.
    pub fn marshal_json(&self) -> Result<String, gf_errors::gerror::Error> {
        let map = self.map_str_any();
        gf_json::marshal_value(&Value::Map(map))
    }

    /// Returns the value for `key` wrapped in a [`Var`].
    pub fn get_var(&self, key: &K) -> Var {
        Var::new(self.get(key).unwrap_or(Value::Null))
    }

    /// Returns the value for `key`, inserting `value` when absent.
    pub fn get_var_or_set(&self, key: K, value: Value) -> Var {
        Var::new(self.get_or_set(key, value))
    }

    /// Returns the value for `key`, or the closure's result, wrapped in a
    /// [`Var`].
    pub fn get_var_or_set_func(&self, key: K, f: impl FnOnce() -> Value) -> Var {
        Var::new(self.get_or_set_func(key, f))
    }

    /// Go's lock-holding variant, wrapped in a [`Var`]. The callback never
    /// runs under the lock (ADR-007 divergence).
    pub fn get_var_or_set_func_lock(&self, key: K, f: impl FnOnce() -> Value) -> Var {
        Var::new(self.get_or_set_func_lock(key, f))
    }
}

impl<K, V> Clone for BKVTree<K, V>
where
    K: Clone,
    V: Clone,
{
    fn clone(&self) -> Self {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        BKVTree {
            inner: RwLock::new(guard.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use gf_util::gutil::{comparator_int, comparator_string, comparator_t};

    use super::*;

    fn text(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    fn int(value: i64) -> Value {
        Value::Int(value)
    }

    fn cmp_str() -> TreeComparator<Value> {
        Arc::new(|a: &Value, b: &Value| comparator_string(a, b))
    }

    fn cmp_int() -> TreeComparator<Value> {
        Arc::new(|a: &Value, b: &Value| comparator_int(a, b))
    }

    fn cmp_i64() -> TreeComparator<i64> {
        Arc::new(|a: &i64, b: &i64| comparator_t(a, b))
    }

    #[test]
    fn test_avl_tree_basic() {
        let tree = AVLTree::new(Some(cmp_str()));
        tree.set(text("key1"), text("val1"));
        assert_eq!(tree.keys(), vec![text("key1")]);

        assert_eq!(tree.get(&text("key1")), Some(text("val1")));
        assert_eq!(tree.size(), 1);
        assert!(!tree.is_empty());

        assert_eq!(tree.get_or_set(text("key2"), text("val2")), text("val2"));
        assert_eq!(tree.get_or_set(text("key2"), text("val2")), text("val2"));
        assert!(!tree.set_if_not_exist(text("key2"), text("val2")));
        assert!(tree.set_if_not_exist(text("key3"), text("val3")));

        assert_eq!(tree.remove(&text("key2")), Some(text("val2")));
        assert!(!tree.contains(&text("key2")));
        assert!(tree.keys().contains(&text("key3")));
        assert!(tree.values().contains(&text("val1")));

        tree.sets([(text("key3"), text("val3")), (text("key1"), text("val1"))]);

        tree.flip(None);
        assert!(tree.entries().contains(&(text("val3"), text("key3"))));
        assert!(tree.entries().contains(&(text("val1"), text("key1"))));

        tree.flip(Some(cmp_str()));
        assert!(tree.entries().contains(&(text("key3"), text("val3"))));
        assert!(tree.entries().contains(&(text("key1"), text("val1"))));

        tree.clear();
        assert_eq!(tree.size(), 0);
        assert!(tree.is_empty());

        let from = AVLTree::new_from(
            Some(cmp_str()),
            [(int(1), int(1)), (text("key1"), text("val1"))],
        );
        assert_eq!(from.entries().len(), 2);
    }

    #[test]
    fn test_red_black_tree_basic() {
        let tree = RedBlackTree::new(Some(cmp_str()));
        tree.set(text("key1"), text("val1"));
        assert_eq!(tree.keys(), vec![text("key1")]);

        assert_eq!(tree.get(&text("key1")), Some(text("val1")));
        assert_eq!(tree.size(), 1);
        assert!(!tree.is_empty());

        assert_eq!(tree.get_or_set(text("key2"), text("val2")), text("val2"));
        assert!(!tree.set_if_not_exist(text("key2"), text("val2")));
        assert!(tree.set_if_not_exist(text("key3"), text("val3")));

        assert_eq!(tree.remove(&text("key2")), Some(text("val2")));
        assert!(!tree.contains(&text("key2")));
        assert!(tree.keys().contains(&text("key3")));

        tree.sets([(text("key3"), text("val3")), (text("key1"), text("val1"))]);
        tree.flip(None);
        assert!(tree.entries().contains(&(text("val3"), text("key3"))));
        tree.flip(Some(cmp_str()));
        assert!(tree.entries().contains(&(text("key3"), text("val3"))));

        tree.clear();
        assert_eq!(tree.size(), 0);

        let from = RedBlackTree::new_from(
            Some(cmp_str()),
            [(int(1), int(1)), (text("key1"), text("val1"))],
        );
        assert_eq!(from.entries().len(), 2);
    }

    #[test]
    fn test_btree_basic() {
        let tree = BTree::new(3, Some(cmp_str()));
        tree.set(text("key1"), text("val1"));
        assert_eq!(tree.height(), 1);

        assert_eq!(tree.keys(), vec![text("key1")]);
        assert_eq!(tree.get(&text("key1")), Some(text("val1")));
        assert_eq!(tree.size(), 1);
        assert!(!tree.is_empty());

        assert_eq!(tree.get_or_set(text("key2"), text("val2")), text("val2"));
        assert!(!tree.set_if_not_exist(text("key2"), text("val2")));
        assert!(tree.set_if_not_exist(text("key3"), text("val3")));

        assert_eq!(tree.remove(&text("key2")), Some(text("val2")));
        assert!(!tree.contains(&text("key2")));
        assert!(tree.keys().contains(&text("key3")));
        assert!(tree.values().contains(&text("val1")));

        tree.clear();
        assert_eq!(tree.size(), 0);
        assert!(tree.is_empty());

        let from = BTree::new_from(
            3,
            Some(cmp_str()),
            [(int(1), int(1)), (text("key1"), text("val1"))],
        );
        assert_eq!(from.entries().len(), 2);
    }

    #[test]
    fn test_avl_tree_set_fun() {
        let tree = AVLTree::new(Some(cmp_str()));
        assert_eq!(tree.get_or_set_func(text("fun"), || int(3)), int(3));
        assert_eq!(tree.get_or_set_func(text("fun"), || int(3)), int(3));
        assert_eq!(
            tree.get_or_set_func_lock(text("funlock"), || int(3)),
            int(3)
        );
        assert_eq!(
            tree.get_or_set_func_lock(text("funlock"), || int(3)),
            int(3)
        );
        assert_eq!(tree.get(&text("funlock")), Some(int(3)));
        assert_eq!(tree.get(&text("fun")), Some(int(3)));

        let tree = AVLTree::new(Some(cmp_str()));
        assert!(tree.set_if_not_exist_func(text("fun"), || int(3)));
        assert!(!tree.set_if_not_exist_func(text("fun"), || int(3)));
        assert!(tree.set_if_not_exist_func_lock(text("funlock"), || int(3)));
        assert!(!tree.set_if_not_exist_func_lock(text("funlock"), || int(3)));
        assert_eq!(tree.get(&text("funlock")), Some(int(3)));
        assert_eq!(tree.get(&text("fun")), Some(int(3)));
    }

    #[test]
    fn test_btree_set_fun() {
        let tree = BTree::new(3, Some(cmp_str()));
        assert_eq!(tree.get_or_set_func(text("fun"), || int(3)), int(3));
        assert_eq!(tree.get_or_set_func(text("fun"), || int(3)), int(3));
        assert_eq!(
            tree.get_or_set_func_lock(text("funlock"), || int(3)),
            int(3)
        );
        assert_eq!(tree.get(&text("funlock")), Some(int(3)));

        let tree = BTree::new(3, Some(cmp_str()));
        assert!(tree.set_if_not_exist_func(text("fun"), || int(3)));
        assert!(!tree.set_if_not_exist_func(text("fun"), || int(3)));
        assert!(tree.set_if_not_exist_func_lock(text("funlock"), || int(3)));
        assert!(!tree.set_if_not_exist_func_lock(text("funlock"), || int(3)));
    }

    #[test]
    fn test_avl_tree_get_set_var() {
        let tree = AVLTree::new(Some(cmp_str()));
        assert!(tree.set_if_not_exist(text("key1"), text("val1")));
        assert!(!tree.set_if_not_exist(text("key1"), text("val1")));
        assert_eq!(
            tree.get_var_or_set(text("key1"), text("val1")).string(),
            "val1".to_owned()
        );
        assert_eq!(tree.get_var(&text("key1")).string(), "val1".to_owned());

        let tree = AVLTree::new(Some(cmp_str()));
        assert_eq!(tree.get_var_or_set_func(text("fun"), || int(3)).int64(), 3);
        assert_eq!(
            tree.get_var_or_set_func_lock(text("funlock"), || int(3))
                .int64(),
            3
        );
    }

    #[test]
    fn test_btree_get_set_var() {
        let tree = BTree::new(3, Some(cmp_str()));
        assert!(tree.set_if_not_exist(text("key1"), text("val1")));
        assert!(!tree.set_if_not_exist(text("key1"), text("val1")));
        assert_eq!(
            tree.get_var_or_set(text("key1"), text("val1")).string(),
            "val1".to_owned()
        );
        assert_eq!(tree.get_var(&text("key1")).string(), "val1".to_owned());
        assert_eq!(tree.get_var_or_set_func(text("fun"), || int(3)).int64(), 3);
        assert_eq!(
            tree.get_var_or_set_func_lock(text("funlock"), || int(3))
                .int64(),
            3
        );
    }

    #[test]
    fn test_avl_tree_batch() {
        let tree = AVLTree::new(Some(cmp_str()));
        tree.sets([
            (int(1), int(1)),
            (text("key1"), text("val1")),
            (text("key2"), text("val2")),
            (text("key3"), text("val3")),
        ]);
        assert_eq!(tree.size(), 4);
        tree.removes([text("key1"), int(1)]);
        assert_eq!(tree.size(), 2);
        assert!(tree.entries().contains(&(text("key2"), text("val2"))));
        assert!(tree.entries().contains(&(text("key3"), text("val3"))));
    }

    #[test]
    fn test_btree_batch() {
        let tree = BTree::new(3, Some(cmp_str()));
        tree.sets([
            (int(1), int(1)),
            (text("key1"), text("val1")),
            (text("key2"), text("val2")),
            (text("key3"), text("val3")),
        ]);
        assert_eq!(tree.size(), 4);
        tree.removes([text("key1"), int(1)]);
        assert_eq!(tree.size(), 2);
    }

    #[test]
    fn test_avl_tree_iterator() {
        let tree = AVLTree::new_from(
            Some(cmp_str()),
            [
                (text("1"), int(1)),
                (text("key1"), text("val1")),
                (text("key2"), text("val2")),
                (text("key3"), text("val3")),
                (text("key4"), text("val4")),
            ],
        );

        let mut ascending = Vec::new();
        tree.iterator(|key, _| {
            ascending.push(key);
            true
        });
        assert_eq!(
            ascending,
            vec![
                text("1"),
                text("key1"),
                text("key2"),
                text("key3"),
                text("key4")
            ]
        );

        let mut descending = Vec::new();
        tree.iterator_desc(|key, _| {
            descending.push(key);
            true
        });
        assert_eq!(
            descending,
            vec![
                text("key4"),
                text("key3"),
                text("key2"),
                text("key1"),
                text("1")
            ]
        );

        let mut all = 0;
        tree.iterator(|_, _| {
            all += 1;
            true
        });
        assert_eq!(all, 5);

        let mut stopped = 0;
        tree.iterator(|_, _| {
            stopped += 1;
            false
        });
        assert_eq!(stopped, 1);

        let mut stopped_desc = 0;
        tree.iterator_desc(|_, _| {
            stopped_desc += 1;
            false
        });
        assert_eq!(stopped_desc, 1);
    }

    #[test]
    fn test_tree_iterator_from_match() {
        let tree = AVLTree::new(Some(cmp_int()));
        for index in 1..=10 {
            tree.set(int(index), int(index * 10));
        }

        let mut keys = Vec::new();
        tree.iterator_from(&int(5), true, |key, value| {
            assert_eq!(value, int(key_int(&key) * 10));
            keys.push(key);
            true
        });
        assert_eq!(keys, (5..=10).map(int).collect::<Vec<_>>());

        let mut keys = Vec::new();
        tree.iterator_asc_from(&int(5), true, |key, _| {
            keys.push(key);
            true
        });
        assert_eq!(keys, (5..=10).map(int).collect::<Vec<_>>());

        let mut keys = Vec::new();
        tree.iterator_desc_from(&int(5), true, |key, _| {
            keys.push(key);
            true
        });
        assert_eq!(keys, vec![int(5), int(4), int(3), int(2), int(1)]);

        let mut keys = Vec::new();
        tree.iterator_asc_from(&int(99), true, |key, _| {
            keys.push(key);
            true
        });
        assert!(keys.is_empty());
    }

    #[test]
    fn test_tree_iterator_from_index() {
        let tree = BTree::new(3, Some(cmp_int()));
        for index in 1..=10 {
            tree.set(int(index), int(index * 10));
        }

        let mut keys = Vec::new();
        tree.iterator_asc_from(&int(0), false, |key, _| {
            keys.push(key);
            true
        });
        assert_eq!(keys, (1..=10).map(int).collect::<Vec<_>>());

        let mut keys = Vec::new();
        tree.iterator_desc_from(&int(5), false, |key, _| {
            keys.push(key);
            true
        });
        assert_eq!(keys, vec![int(6), int(5), int(4), int(3), int(2), int(1)]);
    }

    fn key_int(key: &Value) -> i64 {
        match key {
            Value::Int(value) => *value,
            other => panic!("expected an int key, got {other}"),
        }
    }

    #[test]
    fn test_btree_iterator_stop() {
        let tree = BTree::new_from(
            3,
            Some(cmp_str()),
            [
                (text("key1"), text("val1")),
                (text("key2"), text("val2")),
                (text("key3"), text("val3")),
            ],
        );
        let mut seen = Vec::new();
        tree.iterator_desc(|key, _| {
            seen.push(key);
            seen.len() < 2
        });
        assert_eq!(seen, vec![text("key3"), text("key2")]);

        let mut keys = Vec::new();
        tree.iterator_asc_from(&text("key2"), true, |key, _| {
            keys.push(key);
            true
        });
        assert_eq!(keys, vec![text("key2"), text("key3")]);
    }

    #[test]
    fn test_avl_tree_clone() {
        let tree = AVLTree::new_from(
            Some(cmp_str()),
            [(int(1), int(1)), (text("key1"), text("val1"))],
        );
        let copy = tree.clone();
        tree.remove(&int(1));
        assert!(copy.keys().contains(&int(1)));

        copy.remove(&text("key1"));
        assert!(tree.keys().contains(&text("key1")));
    }

    #[test]
    fn test_btree_clone() {
        let tree = BTree::new_from(
            3,
            Some(cmp_str()),
            [(int(1), int(1)), (text("key1"), text("val1"))],
        );
        let copy = tree.clone();
        tree.remove(&int(1));
        assert!(copy.keys().contains(&int(1)));

        copy.remove(&text("key1"));
        assert!(tree.keys().contains(&text("key1")));
    }

    #[test]
    fn test_avl_tree_lr_node() {
        let tree = AVLTree::new_from(
            Some(cmp_str()),
            [
                (text("key1"), text("val1")),
                (text("key2"), text("val2")),
                (text("key3"), text("val3")),
                (text("key4"), text("val4")),
            ],
        );
        assert_eq!(left_key(tree.left()), Some(text("key1")));
        assert_eq!(left_key(tree.right()), Some(text("key4")));
    }

    #[test]
    fn test_btree_lr_node() {
        let tree = BTree::new_from(
            3,
            Some(cmp_str()),
            [
                (text("key1"), text("val1")),
                (text("key2"), text("val2")),
                (text("key3"), text("val3")),
                (text("key4"), text("val4")),
            ],
        );
        assert_eq!(tree.left().map(|entry| entry.key), Some(text("key1")));
        assert_eq!(tree.right().map(|entry| entry.key), Some(text("key4")));

        let empty = BTree::new(3, Some(cmp_int()));
        assert!(empty.left().is_none());
        assert!(empty.right().is_none());
    }

    fn left_key<K, V>(node: Option<AVLKVTreeNode<K, V>>) -> Option<K> {
        node.map(|node| node.key)
    }

    #[test]
    fn test_avl_tree_ceiling_floor() {
        let tree = AVLTree::new(Some(cmp_int()));
        for (key, value) in [
            (20, "val20"),
            (6, "val6"),
            (10, "val10"),
            (12, "val12"),
            (1, "val1"),
            (15, "val15"),
            (19, "val19"),
            (8, "val8"),
            (4, "val4"),
        ] {
            tree.set(int(key), text(value));
        }

        let ceiling = tree.ceiling(&int(8));
        assert_eq!(
            ceiling.as_ref().map(|node| node.value.clone()),
            Some(text("val8"))
        );
        let floor = tree.floor(&int(20));
        assert_eq!(
            floor.as_ref().map(|node| node.value.clone()),
            Some(text("val20"))
        );

        let ceiling = tree.ceiling(&int(9));
        assert_eq!(
            ceiling.as_ref().map(|node| node.value.clone()),
            Some(text("val10"))
        );
        let floor = tree.floor(&int(5));
        assert_eq!(
            floor.as_ref().map(|node| node.value.clone()),
            Some(text("val4"))
        );

        assert!(tree.ceiling(&int(21)).is_none());
        assert!(tree.floor(&int(-1)).is_none());
    }

    #[test]
    fn test_red_black_tree_ceiling_floor() {
        let tree = RedBlackTree::new(Some(cmp_int()));
        for (key, value) in [
            (20, "val20"),
            (6, "val6"),
            (10, "val10"),
            (1, "val1"),
            (8, "val8"),
            (4, "val4"),
        ] {
            tree.set(int(key), text(value));
        }
        assert_eq!(
            tree.ceiling(&int(9)).map(|node| node.value),
            Some(text("val10"))
        );
        assert_eq!(
            tree.floor(&int(5)).map(|node| node.value),
            Some(text("val4"))
        );
        assert!(tree.ceiling(&int(21)).is_none());
        assert!(tree.floor(&int(-1)).is_none());
    }

    // Go has no test for the tree pop operations; these pin the documented
    // behaviour: remove and return the extreme node.
    #[test]
    fn test_red_black_tree_pop_left_and_right() {
        let tree = RedBlackTree::new(Some(cmp_int()));
        assert!(tree.pop_left().is_none());
        assert!(tree.pop_right().is_none());
        for key in [5, 2, 8, 1, 9] {
            tree.set(int(key), text(&format!("val{key}")));
        }
        let left = tree.pop_left().expect("left node");
        assert_eq!(left.key, int(1));
        assert_eq!(left.value, text("val1"));
        let right = tree.pop_right().expect("right node");
        assert_eq!(right.key, int(9));
        assert_eq!(right.value, text("val9"));
        assert_eq!(tree.keys(), vec![int(2), int(5), int(8)]);
    }

    // Translated from ExampleRedBlackTree_SetComparator: changing the
    // comparator re-orders the entries already in the tree.
    #[test]
    fn test_red_black_tree_set_comparator_reorders() {
        let tree = RedBlackTree::new(Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        assert_eq!(
            tree.keys(),
            (0..6)
                .map(|index| text(&format!("key{index}")))
                .collect::<Vec<_>>()
        );

        tree.set_comparator(Arc::new(|a: &Value, b: &Value| comparator_string(b, a)));
        assert_eq!(
            tree.keys(),
            (0..6)
                .rev()
                .map(|index| text(&format!("key{index}")))
                .collect::<Vec<_>>()
        );
        assert_eq!(tree.size(), 6);
    }

    // Translated from ExampleRedBlackTree_UnmarshalJSON/UnmarshalValue.
    #[test]
    fn test_red_black_tree_unmarshal_json() {
        let tree = RedBlackTree::new(Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        let encoded = tree.marshal_json().expect("marshal");

        let other = RedBlackTree::new(Some(cmp_str()));
        other.unmarshal_json(encoded.as_bytes()).expect("unmarshal");
        assert_eq!(other.entries(), tree.entries());
        assert_eq!(other.marshal_json().expect("marshal"), encoded);

        // Existing entries are kept, nil values are skipped (Go's `doSet`).
        other
            .unmarshal_value(&Value::Map(IndexMap::from([
                ("key9".to_owned(), Value::Null),
                ("key10".to_owned(), text("val10")),
            ])))
            .expect("unmarshal value");
        assert_eq!(other.get(&text("key9")), None);
        assert_eq!(other.get(&text("key10")), Some(text("val10")));
        assert_eq!(other.size(), 7);

        let error = other
            .unmarshal_value(&Value::List(vec![int(1)]))
            .expect_err("a list is not an object");
        assert!(error.to_string().contains("non-object"));
    }

    #[test]
    fn test_avl_tree_remove_all() {
        let tree = AVLTree::new(Some(cmp_int()));
        for index in 1..=50 {
            tree.set(int(index), text(&format!("val{index}")));
        }
        let expected = tree.entries();
        for (key, value) in expected {
            let copy = tree.clone();
            assert_eq!(copy.remove(&key), Some(value));
            assert_eq!(copy.remove(&key), None);
        }
    }

    #[test]
    fn test_btree_remove_all() {
        let tree = BTree::new(3, Some(cmp_int()));
        for index in 1..=100 {
            tree.set(int(index), text(&format!("val{index}")));
        }
        let expected = tree.entries();
        for (key, value) in expected {
            let copy = tree.clone();
            assert_eq!(copy.remove(&key), Some(value));
            assert_eq!(copy.remove(&key), None);
        }
    }

    #[test]
    fn test_btree_remove_sequentially() {
        let tree = BTree::new(3, Some(cmp_int()));
        for index in 0..200 {
            tree.set(int(index), int(index));
        }
        for index in (0..200).step_by(2) {
            assert_eq!(tree.remove(&int(index)), Some(int(index)));
        }
        assert_eq!(tree.size(), 100);
        let remaining = tree.keys();
        for (position, key) in remaining.into_iter().enumerate() {
            assert_eq!(key, int(position as i64 * 2 + 1));
        }
        for index in (1..200).step_by(2) {
            assert_eq!(tree.remove(&int(index)), Some(int(index)));
        }
        assert!(tree.is_empty());
        assert_eq!(tree.height(), 0);
        assert!(tree.left().is_none());
    }

    #[test]
    fn test_btree_height() {
        let tree = BTree::new(3, Some(cmp_int()));
        tree.set(int(0), int(0));
        assert_eq!(tree.height(), 1);

        let tree = BTree::new(3, Some(cmp_int()));
        for index in 0..100 {
            tree.set(int(index), int(index));
        }
        assert_eq!(tree.height(), 6);
    }

    #[test]
    fn test_btree_string() {
        let tree = BTree::new(3, Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        assert_eq!(
            tree.string(),
            "    key0\nkey1\n    key2\nkey3\n    key4\n    key5\n"
        );
    }

    #[test]
    fn test_avl_tree_string() {
        let tree = AVLTree::new(Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        assert_eq!(
            tree.string(),
            "        key0\n    key1\n        key2\nkey3\n        key4\n    key5\n"
        );
    }

    #[test]
    fn test_red_black_tree_string() {
        let tree = RedBlackTree::new(Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        assert_eq!(
            tree.string(),
            "        key0\n    key1\n        key2\nkey3\n        key4\n    key5\n"
        );
    }

    #[test]
    fn test_btree_marshal_json() {
        let tree = BTree::new(3, Some(cmp_str()));
        for index in 0..6 {
            tree.set(text(&format!("key{index}")), text(&format!("val{index}")));
        }
        assert_eq!(
            tree.marshal_json().expect("json"),
            "{\"key0\":\"val0\",\"key1\":\"val1\",\"key2\":\"val2\",\"key3\":\"val3\",\"key4\":\"val4\",\"key5\":\"val5\"}"
        );
    }

    #[test]
    fn test_btree_map_str_any() {
        let tree = BTree::new(3, Some(cmp_int()));
        for index in 1000..1006 {
            tree.set(int(index), text(&format!("val{}", index - 1000)));
        }
        let map = tree.map_str_any();
        assert_eq!(map.len(), 6);
        assert_eq!(map.get("1000"), Some(&text("val0")));
        assert_eq!(map.get("1005"), Some(&text("val5")));
    }

    #[test]
    fn test_generic_map() {
        let tree: AVLKVTree<i64, i64> = AVLKVTree::new(Some(cmp_i64()));
        tree.set(1, 10);
        tree.set(2, 20);
        let map = tree.map();
        assert_eq!(map.get(&1), Some(&10));
        assert_eq!(map.get(&2), Some(&20));
        assert_eq!(map.keys().copied().collect::<Vec<_>>(), vec![1, 2]);
    }

    #[test]
    fn test_typed_nil_values_are_ignored() {
        let tree: AVLKVTree<i64, Option<i64>> = AVLKVTree::new(Some(cmp_i64()));
        tree.set(1, None);
        tree.set(2, Some(5));
        assert_eq!(tree.size(), 1);
        assert!(tree.contains(&2));
        assert!(!tree.contains(&1));
    }

    #[test]
    #[should_panic(expected = "m must be greater than or equal to 3")]
    fn test_btree_order_panic() {
        let _ = BTree::new(2, Some(cmp_str()));
    }
}
