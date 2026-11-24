//! Port of `container/gmap` for the port's value types.
//!
//! Go ships one type per key/value combination. The port uses one generic
//! `Map<K, V>` over an insertion-ordered map behind a lock; the Go names are
//! type aliases (ADR-030). Always synchronized, like every container
//! (ADR-007).
//!
//! `ListKVMap` keeps insertion order over a vector of pairs rather than an
//! `IndexMap`, because Go's list map accepts any key type and
//! [`gf_core::Value`] has no `Hash` implementation (ADR-033). Lookups are
//! linear; the Go original is O(1).

use std::hash::Hash;
use std::sync::{Arc, RwLock};

use gf_core::Value;
use gf_errors::gerror::Error;
use gf_internal::empty;
use indexmap::IndexMap;
use serde::de::DeserializeOwned;

use crate::gvar::Var;

/// An insertion-ordered, synchronized map.
pub struct Map<K, V> {
    inner: RwLock<IndexMap<K, V>>,
}

impl<K, V> Map<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    /// Creates an empty map.
    pub fn new() -> Map<K, V> {
        Map {
            inner: RwLock::new(IndexMap::new()),
        }
    }

    /// Creates an empty map with the given capacity.
    pub fn with_capacity(capacity: usize) -> Map<K, V> {
        Map {
            inner: RwLock::new(IndexMap::with_capacity(capacity)),
        }
    }

    /// Inserts or replaces `key`.
    pub fn set(&self, key: K, value: V) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.insert(key, value);
    }

    /// Returns the value for `key`.
    pub fn get(&self, key: &K) -> Option<V> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned()
    }

    /// Removes `key` and returns its value.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.shift_remove(key)
    }

    /// Reports whether `key` exists.
    pub fn contains_key(&self, key: &K) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(key)
    }

    /// Returns the number of entries.
    pub fn size(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Reports whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Removes every entry.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
    }

    /// Returns the keys in insertion order.
    pub fn keys(&self) -> Vec<K> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Returns the values in insertion order.
    pub fn values(&self) -> Vec<V> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Returns a snapshot of the entries.
    pub fn map(&self) -> IndexMap<K, V> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Returns the value for `key`, inserting `default` when absent.
    pub fn get_or_set(&self, key: K, default: V) -> V {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.entry(key).or_insert(default).clone()
    }

    /// Runs `func` with write access.
    pub fn with_write<R>(&self, func: impl FnOnce(&mut IndexMap<K, V>) -> R) -> R {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        func(&mut guard)
    }

    /// Runs `func` with read access.
    pub fn with_read<R>(&self, func: impl FnOnce(&IndexMap<K, V>) -> R) -> R {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        func(&guard)
    }
}

impl<K, V> Default for Map<K, V>
where
    K: Eq + Hash + Clone,
    V: Clone,
{
    fn default() -> Self {
        Map::new()
    }
}

impl<K, V> std::fmt::Debug for Map<K, V>
where
    K: Eq + Hash + Clone + std::fmt::Debug,
    V: Clone + std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::fmt::Debug::fmt(&*guard, formatter)
    }
}

/// `map[string]any`.
pub type StrAnyMap = Map<String, Value>;
/// `map[string]string`.
pub type StrStrMap = Map<String, String>;
/// `map[string]int`.
pub type StrIntMap = Map<String, i64>;
/// `map[int]int`.
pub type IntIntMap = Map<i64, i64>;
/// `map[int]string`.
pub type IntStrMap = Map<i64, String>;
/// `map[int]any`.
pub type IntAnyMap = Map<i64, Value>;
/// Go's generic hash map type (`gmap.KVMap`).
pub type KVMap<K, V> = Map<K, V>;

/// Reports whether a value is Go's `nil` or empty for a map entry.
///
/// `Option<T>` is nil when `None`; [`Value::Null`] is nil; other value types
/// report their zero value as empty.
pub trait NilValue {
    /// Reports whether the value is nil.
    fn is_nil(&self) -> bool {
        false
    }

    /// Reports whether the value is empty (`0`, `false`, `""`, empty bytes).
    fn is_empty(&self) -> bool {
        false
    }
}

impl<T> NilValue for Option<T> {
    fn is_nil(&self) -> bool {
        self.is_none()
    }

    fn is_empty(&self) -> bool {
        self.is_none()
    }
}

impl NilValue for Value {
    fn is_nil(&self) -> bool {
        matches!(self, Value::Null)
    }

    fn is_empty(&self) -> bool {
        empty::is_empty(self)
    }
}

impl NilValue for String {
    fn is_empty(&self) -> bool {
        String::is_empty(self)
    }
}

impl NilValue for i64 {
    fn is_empty(&self) -> bool {
        *self == 0
    }
}

impl NilValue for u64 {
    fn is_empty(&self) -> bool {
        *self == 0
    }
}

impl NilValue for f64 {
    fn is_empty(&self) -> bool {
        *self == 0.0
    }
}

impl NilValue for bool {
    fn is_empty(&self) -> bool {
        !*self
    }
}

impl NilValue for Vec<u8> {
    fn is_empty(&self) -> bool {
        Vec::is_empty(self)
    }
}

impl<K: Clone, V: Clone> NilValue for IndexMap<K, V> {
    fn is_empty(&self) -> bool {
        IndexMap::is_empty(self)
    }
}

impl<K, V> Map<K, V>
where
    K: Eq + Hash + Clone + ToString,
    V: Clone + NilValue,
{
    /// Returns the value and whether it exists (`KVMap.Search`).
    pub fn search(&self, key: &K) -> Option<V> {
        self.get(key)
    }

    /// Sets every entry of `data`.
    pub fn sets(&self, data: IndexMap<K, V>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, value) in data {
            guard.insert(key, value);
        }
    }

    /// Returns a shallow copy of the entries (`KVMap.MapCopy`).
    pub fn map_copy(&self) -> IndexMap<K, V> {
        self.map()
    }

    /// Returns the entries keyed by their text form (`KVMap.MapStrAny`).
    pub fn map_str_any(&self) -> IndexMap<String, Value>
    where
        V: Into<Value>,
    {
        self.map()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into()))
            .collect()
    }

    /// Reports whether the map is a sub-map of `other` (`KVMap.IsSubOf`).
    pub fn is_sub_of(&self, other: &Map<K, V>) -> bool
    where
        V: PartialEq,
    {
        self.with_read(|mine| {
            other.with_read(|theirs| {
                mine.iter()
                    .all(|(key, value)| theirs.get(key).is_some_and(|other| other == value))
            })
        })
    }

    /// Returns the keys added, removed and updated between `self` and `other`
    /// (`KVMap.Diff`).
    ///
    /// `added` are keys only in `other`, `removed` only in `self`, and
    /// `updated` keys whose values differ.
    pub fn diff(&self, other: &Map<K, V>) -> (Vec<K>, Vec<K>, Vec<K>)
    where
        V: PartialEq,
    {
        self.with_read(|mine| {
            other.with_read(|theirs| {
                let mut added = Vec::new();
                let mut removed = Vec::new();
                let mut updated = Vec::new();
                for (key, value) in mine {
                    match theirs.get(key) {
                        None => removed.push(key.clone()),
                        Some(theirs_value) if theirs_value != value => updated.push(key.clone()),
                        Some(_) => {}
                    }
                }
                for key in theirs.keys() {
                    if !mine.contains_key(key) {
                        added.push(key.clone());
                    }
                }
                (added, removed, updated)
            })
        })
    }

    /// Removes and returns one entry (`KVMap.Pop`).
    pub fn pop(&self) -> Option<(K, V)> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.shift_remove_index(0)
    }

    /// Removes and returns up to `size` entries; `-1` means all (`KVMap.Pops`).
    pub fn pops(&self, size: i64) -> IndexMap<K, V> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let wanted = if size < 0 {
            guard.len()
        } else {
            (size as usize).min(guard.len())
        };
        let mut removed = IndexMap::with_capacity(wanted);
        for _ in 0..wanted {
            if let Some((key, value)) = guard.shift_remove_index(0) {
                removed.insert(key, value);
            }
        }
        removed
    }

    /// Sets `value` only when `key` is absent; nil values are not stored.
    pub fn set_if_not_exist(&self, key: K, value: V) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.contains_key(&key) {
            return false;
        }
        if !value.is_nil() {
            guard.insert(key, value);
        }
        true
    }

    /// Sets the callback's value when `key` is absent; the callback runs without
    /// the lock.
    pub fn set_if_not_exist_func(&self, key: K, func: impl FnOnce() -> V) -> bool {
        if self.contains_key(&key) {
            return false;
        }
        self.set_if_not_exist(key, func())
    }

    /// Sets the callback's value when `key` is absent, running the callback
    /// while holding the write lock like Go's `SetIfNotExistFuncLock`.
    ///
    /// The callback must not touch this map: Rust locks are not re-entrant and a
    /// callback that calls back into the same map deadlocks (documented
    /// divergence, ADR-007).
    pub fn set_if_not_exist_func_lock(&self, key: K, func: impl FnOnce() -> V) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.contains_key(&key) {
            return false;
        }
        let value = func();
        if !value.is_nil() {
            guard.insert(key, value);
        }
        true
    }

    /// Sets the callback's value when `key` is absent, propagating callback
    /// errors without storing anything.
    pub fn set_if_not_exist_func_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<bool, Error> {
        if self.contains_key(&key) {
            return Ok(false);
        }
        let value = func()?;
        if value.is_nil() {
            return Ok(true);
        }
        Ok(self.set_if_not_exist(key, value))
    }

    /// Like [`Map::set_if_not_exist_func_with_error`], with the callback under
    /// the write lock.
    pub fn set_if_not_exist_func_lock_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<bool, Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.contains_key(&key) {
            return Ok(false);
        }
        let value = func()?;
        if !value.is_nil() {
            guard.insert(key, value);
        }
        Ok(true)
    }

    /// Returns the callback's value when `key` is absent, storing it; the
    /// callback runs without the lock.
    pub fn get_or_set_func(&self, key: K, func: impl FnOnce() -> V) -> V {
        if let Some(value) = self.get(&key) {
            return value;
        }
        self.do_set_with_lock_check(key, func())
    }

    /// Like [`Map::get_or_set_func`], with the callback under the write lock.
    pub fn get_or_set_func_lock(&self, key: K, func: impl FnOnce() -> V) -> V {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(value) = guard.get(&key) {
            return value.clone();
        }
        let value = func();
        if !value.is_nil() {
            guard.insert(key, value.clone());
        }
        value
    }

    /// Returns the callback's value when `key` is absent, propagating callback
    /// errors.
    pub fn get_or_set_func_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<V, Error> {
        if let Some(value) = self.get(&key) {
            return Ok(value);
        }
        let value = func()?;
        Ok(self.do_set_with_lock_check(key, value))
    }

    /// Like [`Map::get_or_set_func_with_error`], with the callback under the
    /// write lock.
    pub fn get_or_set_func_lock_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<V, Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(value) = guard.get(&key) {
            return Ok(value.clone());
        }
        let value = func()?;
        if !value.is_nil() {
            guard.insert(key, value.clone());
        }
        Ok(value)
    }

    /// Inserts `value` unless the key appeared meanwhile, and returns the
    /// current value.
    fn do_set_with_lock_check(&self, key: K, value: V) -> V {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(existing) = guard.get(&key) {
            return existing.clone();
        }
        if !value.is_nil() {
            guard.insert(key, value.clone());
        }
        value
    }

    /// Returns the value wrapped in a [`Var`] (`KVMap.GetVar`).
    pub fn get_var(&self, key: &K) -> Option<Var>
    where
        V: Into<Value>,
    {
        self.get(key).map(|value| Var::new(value.into()))
    }

    /// Returns the value as a [`Var`], setting `value` when absent.
    pub fn get_var_or_set(&self, key: K, value: V) -> Var
    where
        V: Into<Value>,
    {
        Var::new(self.get_or_set(key, value).into())
    }

    /// Removes every entry whose value is empty, like Go's `FilterEmpty`.
    pub fn filter_empty(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.retain(|_, value| !value.is_empty());
    }

    /// Removes every entry whose value is nil, like Go's `FilterNil`.
    pub fn filter_nil(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.retain(|_, value| !value.is_nil());
    }

    /// Removes several keys.
    pub fn removes(&self, keys: &[K]) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for key in keys {
            guard.shift_remove(key);
        }
    }

    /// Replaces the entries.
    pub fn replace(&self, data: IndexMap<K, V>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = data;
    }

    /// Clones the map (`KVMap.Clone`).
    pub fn clone_map(&self) -> Map<K, V> {
        Map::new_from(self.map())
    }

    /// Creates a map from `data`.
    pub fn new_from(data: IndexMap<K, V>) -> Map<K, V> {
        let map = Map::new();
        map.sets(data);
        map
    }

    /// Merges `other` into `self` (`KVMap.Merge`).
    pub fn merge(&self, other: &Map<K, V>) {
        self.sets(other.map());
    }

    /// Iterates a snapshot in insertion order; `false` from `func` stops.
    pub fn iterator(&self, func: impl FnMut(&K, &V) -> bool) {
        self.iterator_asc(func);
    }

    /// Iterates a snapshot in insertion order; `false` from `func` stops.
    pub fn iterator_asc(&self, mut func: impl FnMut(&K, &V) -> bool) {
        for (key, value) in self.map() {
            if !func(&key, &value) {
                break;
            }
        }
    }

    /// Iterates a snapshot in reverse insertion order; `false` stops.
    pub fn iterator_desc(&self, mut func: impl FnMut(&K, &V) -> bool) {
        for (key, value) in self.map().into_iter().rev() {
            if !func(&key, &value) {
                break;
            }
        }
    }

    /// Runs `func` with write access to a snapshot and stores the result.
    ///
    /// The callback never runs while the lock is held (ADR-007).
    pub fn lock_func(&self, func: impl FnOnce(&mut IndexMap<K, V>)) {
        let mut entries = self.map();
        func(&mut entries);
        self.replace(entries);
    }

    /// Runs `func` with read access to a snapshot.
    pub fn rlock_func(&self, func: impl FnOnce(&IndexMap<K, V>)) {
        func(&self.map());
    }

    /// Returns the map as compact JSON with sorted keys (`KVMap.String`).
    pub fn string(&self) -> String
    where
        V: Into<Value>,
    {
        match self.marshal_json() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    }

    /// Marshals the map as JSON with sorted keys.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error>
    where
        V: Into<Value>,
    {
        let mut sorted = std::collections::BTreeMap::new();
        for (key, value) in self.map() {
            sorted.insert(key.to_string(), value.into());
        }
        let value = Value::Map(sorted.into_iter().collect());
        gf_json::marshal(&value)
    }

    /// Replaces the entries from JSON object text.
    pub fn unmarshal_json(&self, data: &[u8]) -> Result<(), Error>
    where
        K: From<String>,
        V: DeserializeOwned,
    {
        let parsed: IndexMap<String, V> = gf_json::unmarshal_use_number(data)?;
        let mut entries = IndexMap::with_capacity(parsed.len());
        for (key, value) in parsed {
            entries.insert(K::from(key), value);
        }
        self.replace(entries);
        Ok(())
    }

    /// Replaces the entries from a [`Value`] map.
    pub fn unmarshal_value(&self, value: &Value) -> Result<(), Error>
    where
        K: From<String>,
        V: DeserializeOwned,
    {
        let text = gf_json::marshal_value(value)?;
        self.unmarshal_json(text.as_bytes())
    }

    /// Deep-copies the entries into a new map.
    pub fn deep_copy(&self) -> Map<K, V> {
        self.clone_map()
    }
}

/// A nil checker for [`ListKVMap`] values.
pub type ListNilChecker<V> = Arc<dyn Fn(&V) -> bool + Send + Sync>;

/// The locked state of a [`ListKVMap`].
struct ListState<K, V> {
    entries: Vec<(K, V)>,
    nil_checker: Option<ListNilChecker<V>>,
}

/// An insertion-ordered map (`gmap.ListKVMap`).
///
/// Go backs it with a hash table plus a doubly-linked list; the port uses a
/// vector of pairs, because [`gf_core::Value`] keys cannot be hashed (ADR-033).
/// Positions are kept on update, like Go's list.
pub struct ListKVMap<K, V> {
    inner: RwLock<ListState<K, V>>,
}

impl<K, V> ListKVMap<K, V>
where
    K: PartialEq + Clone + ToString,
    V: Clone,
{
    /// Creates an empty map.
    pub fn new() -> ListKVMap<K, V> {
        ListKVMap {
            inner: RwLock::new(ListState {
                entries: Vec::new(),
                nil_checker: None,
            }),
        }
    }

    /// Creates an empty map with a custom nil checker.
    pub fn new_with_checker(
        checker: impl Fn(&V) -> bool + Send + Sync + 'static,
    ) -> ListKVMap<K, V> {
        ListKVMap {
            inner: RwLock::new(ListState {
                entries: Vec::new(),
                nil_checker: Some(Arc::new(checker)),
            }),
        }
    }

    /// Creates a map from ordered pairs.
    pub fn new_from(data: Vec<(K, V)>) -> ListKVMap<K, V> {
        let map = ListKVMap::new();
        map.sets(data);
        map
    }

    /// Registers a custom nil checker.
    pub fn set_nil_checker(&self, checker: impl Fn(&V) -> bool + Send + Sync + 'static) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.nil_checker = Some(Arc::new(checker));
    }

    /// Sets one entry, keeping its original position when the key exists.
    pub fn set(&self, key: K, value: V) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.entries.iter_mut().find(|(entry, _)| *entry == key) {
            Some((_, slot)) => *slot = value,
            None => guard.entries.push((key, value)),
        }
    }

    /// Sets every entry of `data`.
    pub fn sets(&self, data: Vec<(K, V)>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for (key, value) in data {
            match guard.entries.iter_mut().find(|(entry, _)| *entry == key) {
                Some((_, slot)) => *slot = value,
                None => guard.entries.push((key, value)),
            }
        }
    }

    /// Returns the value and whether it exists.
    pub fn search(&self, key: &K) -> Option<V> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .find(|(entry, _)| entry == key)
            .map(|(_, value)| value.clone())
    }

    /// Returns the value for `key`.
    pub fn get(&self, key: &K) -> Option<V> {
        self.search(key)
    }

    /// Removes the first entry in insertion order.
    pub fn pop(&self) -> Option<(K, V)> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.entries.is_empty() {
            None
        } else {
            Some(guard.entries.remove(0))
        }
    }

    /// Removes up to `size` entries in insertion order; `-1` means all.
    pub fn pops(&self, size: i64) -> Vec<(K, V)> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let wanted = if size < 0 {
            guard.entries.len()
        } else {
            (size as usize).min(guard.entries.len())
        };
        guard.entries.drain(..wanted).collect()
    }

    /// Removes `key` and returns its value.
    pub fn remove(&self, key: &K) -> Option<V> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .entries
            .iter()
            .position(|(entry, _)| entry == key)
            .map(|index| guard.entries.remove(index).1)
    }

    /// Removes several keys.
    pub fn removes(&self, keys: &[K]) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard
            .entries
            .retain(|(entry, _)| !keys.iter().any(|key| key == entry));
    }

    /// Returns the keys in insertion order.
    pub fn keys(&self) -> Vec<K> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .map(|(key, _)| key.clone())
            .collect()
    }

    /// Returns the values in insertion order.
    pub fn values(&self) -> Vec<V> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .map(|(_, value)| value.clone())
            .collect()
    }

    /// Reports whether `key` exists.
    pub fn contains(&self, key: &K) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .iter()
            .any(|(entry, _)| entry == key)
    }

    /// Returns the number of entries.
    pub fn size(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .len()
    }

    /// Reports whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.size() == 0
    }

    /// Removes every entry.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.entries.clear();
    }

    /// Replaces the entries.
    pub fn replace(&self, data: Vec<(K, V)>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.entries = data;
    }

    /// Returns a snapshot in insertion order.
    pub fn map(&self) -> Vec<(K, V)> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entries
            .clone()
    }

    /// Returns the entries keyed by their text form.
    pub fn map_str_any(&self) -> IndexMap<String, Value>
    where
        V: Into<Value>,
    {
        self.map()
            .into_iter()
            .map(|(key, value)| (key.to_string(), value.into()))
            .collect()
    }

    /// Iterates a snapshot in insertion order; `false` from `func` stops.
    pub fn iterator(&self, func: impl FnMut(&K, &V) -> bool) {
        self.iterator_asc(func);
    }

    /// Iterates a snapshot in insertion order; `false` from `func` stops.
    pub fn iterator_asc(&self, mut func: impl FnMut(&K, &V) -> bool) {
        for (key, value) in self.map() {
            if !func(&key, &value) {
                break;
            }
        }
    }

    /// Iterates a snapshot in reverse insertion order; `false` stops.
    pub fn iterator_desc(&self, mut func: impl FnMut(&K, &V) -> bool) {
        for (key, value) in self.map().into_iter().rev() {
            if !func(&key, &value) {
                break;
            }
        }
    }

    /// Clones the map.
    pub fn clone_map(&self) -> ListKVMap<K, V> {
        ListKVMap::new_from(self.map())
    }

    /// Merges `other` into `self`.
    pub fn merge(&self, other: &ListKVMap<K, V>) {
        self.sets(other.map());
    }

    /// Returns the map as compact JSON in insertion order.
    pub fn string(&self) -> String
    where
        V: Into<Value>,
    {
        match self.marshal_json() {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => String::new(),
        }
    }

    /// Marshals the map as JSON in insertion order.
    pub fn marshal_json(&self) -> Result<Vec<u8>, Error>
    where
        V: Into<Value>,
    {
        let mut output = String::from("{");
        for (index, (key, value)) in self.map().into_iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            output.push('"');
            escape_json_key(&mut output, &key.to_string());
            output.push_str("\":");
            output.push_str(&gf_json::marshal_value(&value.into())?);
        }
        output.push('}');
        Ok(output.into_bytes())
    }

    /// Replaces the entries from JSON object text.
    pub fn unmarshal_json(&self, data: &[u8]) -> Result<(), Error>
    where
        K: From<String>,
        V: DeserializeOwned,
    {
        let parsed: IndexMap<String, V> = gf_json::unmarshal_use_number(data)?;
        let entries = parsed
            .into_iter()
            .map(|(key, value)| (K::from(key), value))
            .collect();
        self.replace(entries);
        Ok(())
    }

    /// Replaces the entries from a [`Value`] map.
    pub fn unmarshal_value(&self, value: &Value) -> Result<(), Error>
    where
        K: From<String>,
        V: DeserializeOwned,
    {
        let text = gf_json::marshal_value(value)?;
        self.unmarshal_json(text.as_bytes())
    }

    /// Deep-copies the entries into a new map.
    pub fn deep_copy(&self) -> ListKVMap<K, V> {
        self.clone_map()
    }
}

impl<K, V> ListKVMap<K, V>
where
    K: PartialEq + Clone + ToString,
    V: Clone + NilValue + Into<Value>,
{
    /// Returns the value wrapped in a [`Var`].
    pub fn get_var(&self, key: &K) -> Option<Var> {
        self.get(key).map(|value| Var::new(value.into()))
    }

    /// Returns the value as a [`Var`], setting `value` when absent.
    pub fn get_var_or_set(&self, key: K, value: V) -> Var {
        Var::new(self.get_or_set(key, value).into())
    }
}

impl<K, V> ListKVMap<K, V>
where
    K: PartialEq + Clone + ToString,
    V: Clone + NilValue,
{
    /// Returns the value for `key`, storing `value` when absent.
    ///
    /// Nil values are not stored, and an existing value wins.
    pub fn get_or_set(&self, key: K, value: V) -> V {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, existing)) = guard.entries.iter().find(|(entry, _)| *entry == key) {
            return existing.clone();
        }
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value.clone()));
        }
        value
    }

    /// Returns the callback's value when `key` is absent; the callback runs
    /// without the lock.
    pub fn get_or_set_func(&self, key: K, func: impl FnOnce() -> V) -> V {
        if let Some(value) = self.get(&key) {
            return value;
        }
        self.get_or_set(key, func())
    }

    /// Like [`ListKVMap::get_or_set_func`], with the callback under the write
    /// lock (Go's `GetOrSetFuncLock`); the callback must not touch this map.
    pub fn get_or_set_func_lock(&self, key: K, func: impl FnOnce() -> V) -> V {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, existing)) = guard.entries.iter().find(|(entry, _)| *entry == key) {
            return existing.clone();
        }
        let value = func();
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value.clone()));
        }
        value
    }

    /// Returns the callback's value when `key` is absent, propagating errors.
    pub fn get_or_set_func_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<V, Error> {
        if let Some(value) = self.get(&key) {
            return Ok(value);
        }
        let value = func()?;
        Ok(self.get_or_set(key, value))
    }

    /// Like [`ListKVMap::get_or_set_func_with_error`], with the callback under
    /// the write lock.
    pub fn get_or_set_func_lock_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<V, Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((_, existing)) = guard.entries.iter().find(|(entry, _)| *entry == key) {
            return Ok(existing.clone());
        }
        let value = func()?;
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value.clone()));
        }
        Ok(value)
    }

    /// Sets `value` when `key` is absent; nil values are not stored.
    pub fn set_if_not_exist(&self, key: K, value: V) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.entries.iter().any(|(entry, _)| *entry == key) {
            return false;
        }
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value));
        }
        true
    }

    /// Sets the callback's value when `key` is absent; the callback runs
    /// without the lock.
    pub fn set_if_not_exist_func(&self, key: K, func: impl FnOnce() -> V) -> bool {
        if self.contains(&key) {
            return false;
        }
        self.set_if_not_exist(key, func())
    }

    /// Like [`ListKVMap::set_if_not_exist_func`], with the callback under the
    /// write lock.
    pub fn set_if_not_exist_func_lock(&self, key: K, func: impl FnOnce() -> V) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.entries.iter().any(|(entry, _)| *entry == key) {
            return false;
        }
        let value = func();
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value));
        }
        true
    }

    /// Sets the callback's value when `key` is absent, propagating errors.
    pub fn set_if_not_exist_func_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<bool, Error> {
        if self.contains(&key) {
            return Ok(false);
        }
        let value = func()?;
        if value.is_nil() {
            return Ok(true);
        }
        Ok(self.set_if_not_exist(key, value))
    }

    /// Like [`ListKVMap::set_if_not_exist_func_with_error`], with the callback
    /// under the write lock.
    pub fn set_if_not_exist_func_lock_with_error(
        &self,
        key: K,
        func: impl FnOnce() -> Result<V, Error>,
    ) -> Result<bool, Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.entries.iter().any(|(entry, _)| *entry == key) {
            return Ok(false);
        }
        let value = func()?;
        if !is_nil_locked(&guard, &value) {
            guard.entries.push((key, value));
        }
        Ok(true)
    }

    /// Removes every entry whose value is empty.
    pub fn filter_empty(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.entries.retain(|(_, value)| !value.is_empty());
    }
}

impl ListKVMap<Value, Value> {
    /// Exchanges keys and values (`ListMap.Flip`).
    pub fn flip(&self) {
        let snapshot = self.map();
        self.clear();
        for (key, value) in snapshot {
            self.set(value, key);
        }
    }
}

/// Reports whether `value` is nil, respecting a custom checker.
fn is_nil_locked<K, V: NilValue>(state: &ListState<K, V>, value: &V) -> bool {
    match &state.nil_checker {
        Some(checker) => checker(value),
        None => value.is_nil(),
    }
}

impl<K, V> Default for ListKVMap<K, V>
where
    K: PartialEq + Clone + ToString,
    V: Clone,
{
    fn default() -> Self {
        ListKVMap::new()
    }
}

impl<K, V> std::fmt::Debug for ListKVMap<K, V>
where
    K: PartialEq + Clone + ToString + std::fmt::Debug,
    V: Clone + std::fmt::Debug,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        std::fmt::Debug::fmt(&guard.entries, formatter)
    }
}

/// Escapes a JSON object key.
fn escape_json_key(output: &mut String, key: &str) {
    for character in key.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            control if control.is_control() => {
                output.push_str(&format!("\\u{:04x}", control as u32));
            }
            other => output.push(other),
        }
    }
}

/// `map[any]any` preserving insertion order (`gmap.ListMap`).
pub type ListMap = ListKVMap<Value, Value>;

/// Go's tree map (`gmap.TreeMap`), a red-black tree keyed by [`Value`].
pub type TreeMap = crate::gtree::RedBlackTree;
/// Go's generic tree map (`gmap.TreeKVMap`), a red-black tree.
pub type TreeKVMap<K, V> = crate::gtree::RedBlackKVTree<K, V>;

/// Creates a tree map with the given comparator.
pub fn new_tree_map(comparator: Option<crate::gtree::TreeComparator<Value>>) -> TreeMap {
    TreeMap::new(comparator)
}

/// Creates a tree map from ordered pairs.
pub fn new_tree_map_from(
    comparator: Option<crate::gtree::TreeComparator<Value>>,
    data: impl IntoIterator<Item = (Value, Value)>,
) -> TreeMap {
    TreeMap::new_from(comparator, data)
}

/// Creates a generic tree map with the given comparator.
pub fn new_tree_kv_map<K, V>(comparator: Option<crate::gtree::TreeComparator<K>>) -> TreeKVMap<K, V>
where
    K: crate::gtree::TreeKey,
    V: crate::gtree::TreeValue + Send + Sync + 'static,
{
    TreeKVMap::new(comparator)
}

/// Creates a generic tree map from ordered pairs.
pub fn new_tree_kv_map_from<K, V>(
    comparator: Option<crate::gtree::TreeComparator<K>>,
    data: impl IntoIterator<Item = (K, V)>,
) -> TreeKVMap<K, V>
where
    K: crate::gtree::TreeKey,
    V: crate::gtree::TreeValue + Send + Sync + 'static,
{
    TreeKVMap::new_from(comparator, data)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    #[test]
    fn basic_operations_keep_order() {
        let map: StrAnyMap = Map::new();
        assert!(map.is_empty());
        map.set("b".to_owned(), Value::Int(2));
        map.set("a".to_owned(), Value::Int(1));
        map.set("c".to_owned(), Value::String("x".into()));
        assert_eq!(map.size(), 3);
        assert_eq!(
            map.keys(),
            vec!["b".to_owned(), "a".to_owned(), "c".to_owned()]
        );
        assert_eq!(map.get(&"a".to_owned()), Some(Value::Int(1)));
        assert!(map.contains_key(&"c".to_owned()));
        assert_eq!(map.get(&"missing".to_owned()), None);
        assert_eq!(map.remove(&"b".to_owned()), Some(Value::Int(2)));
        assert_eq!(map.keys(), vec!["a".to_owned(), "c".to_owned()]);
        assert_eq!(map.values().len(), 2);
        assert_eq!(map.map().len(), 2);
        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn typed_maps_and_get_or_set() {
        let map: StrIntMap = Map::new();
        assert_eq!(map.get_or_set("a".to_owned(), 7), 7);
        assert_eq!(map.get_or_set("a".to_owned(), 9), 7);
        assert_eq!(map.size(), 1);

        let map: IntStrMap = Map::new();
        map.set(1, "one".to_owned());
        assert_eq!(map.get(&1), Some("one".to_owned()));
        let map: IntIntMap = Map::new();
        map.set(2, 3);
        assert_eq!(map.get(&2), Some(3));
    }

    #[test]
    fn concurrent_writes_do_not_lose_updates() {
        let map: Arc<StrIntMap> = Arc::new(Map::with_capacity(1));
        map.set("n".to_owned(), 0);
        let mut handles = Vec::new();
        for _ in 0..4 {
            let map = Arc::clone(&map);
            handles.push(std::thread::spawn(move || {
                for _ in 0..1000 {
                    map.with_write(|entries| {
                        if let Some(value) = entries.get_mut("n") {
                            *value += 1;
                        }
                    });
                }
            }));
        }
        for handle in handles {
            handle.join().expect("thread");
        }
        assert_eq!(map.get(&"n".to_owned()), Some(4000));
    }

    #[test]
    fn read_access() {
        let map: StrStrMap = Map::new();
        map.set("k".to_owned(), "v".to_owned());
        let joined = map.with_read(|entries| {
            entries
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect::<Vec<_>>()
                .join(",")
        });
        assert_eq!(joined, "k=v");
    }

    fn pair(key: &str, value: &str) -> (Value, Value) {
        (
            Value::String(key.to_owned()),
            Value::String(value.to_owned()),
        )
    }

    // Translated from Test_ListMap_Basic / _Order / _Batch / _Clone / _Merge.
    #[test]
    fn test_list_map_basic() {
        let map: ListMap = ListMap::new();
        map.set("key1".into(), "val1".into());
        assert_eq!(map.keys(), vec![Value::String("key1".into())]);
        assert_eq!(map.get(&"key1".into()), Some(Value::String("val1".into())));
        assert_eq!(map.size(), 1);
        assert!(!map.is_empty());
        assert_eq!(map.get_or_set("key2".into(), "val2".into()), "val2".into());
        assert!(!map.set_if_not_exist("key2".into(), "val2".into()));
        assert!(map.set_if_not_exist("key3".into(), "val3".into()));
        assert_eq!(
            map.remove(&"key2".into()),
            Some(Value::String("val2".into()))
        );
        assert!(!map.contains(&"key2".into()));
        assert!(map.keys().contains(&"key3".into()));
        assert!(map.values().contains(&"val3".into()));

        map.flip();
        let flipped = map.map();
        assert_eq!(flipped.len(), 2);
        assert!(flipped.contains(&pair("val3", "key3")));
        assert!(flipped.contains(&pair("val1", "key1")));
        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn test_list_map_batch_and_order() {
        let map: ListMap = ListMap::new();
        map.sets(vec![
            (Value::Int(1), Value::Int(1)),
            ("key1".into(), "val1".into()),
            ("key2".into(), "val2".into()),
            ("key3".into(), "val3".into()),
        ]);
        assert_eq!(map.size(), 4);
        map.removes(&[Value::String("key1".into()), Value::Int(1)]);
        assert_eq!(map.map(), vec![pair("key2", "val2"), pair("key3", "val3")]);

        let ordered: ListMap = ListMap::new();
        ordered.set("k1".into(), "v1".into());
        ordered.set("k2".into(), "v2".into());
        ordered.set("k3".into(), "v3".into());
        assert_eq!(
            ordered.keys(),
            vec![
                Value::String("k1".into()),
                Value::String("k2".into()),
                Value::String("k3".into())
            ]
        );
    }

    #[test]
    fn test_list_map_pop_and_filter() {
        let map: ListMap = ListMap::new_from(vec![pair("k1", "v1"), pair("k2", "v2")]);
        let (key, value) = map.pop().expect("pop");
        assert_eq!(key, "k1".into());
        assert_eq!(value, "v1".into());
        assert_eq!(map.size(), 1);
        let removed = map.pops(1);
        assert_eq!(removed.len(), 1);
        assert_eq!(map.size(), 0);
        assert!(map.pops(1).is_empty());

        let map: ListMap = ListMap::new();
        map.set(Value::Int(1), "".into());
        map.set(Value::Int(2), "2".into());
        map.filter_empty();
        assert_eq!(map.size(), 1);
        assert_eq!(map.get(&Value::Int(2)), Some(Value::String("2".into())));
    }

    // Translated from TestListMap_String / _MarshalJSON / Test_ListMap_Json_Sequence.
    #[test]
    fn test_list_map_json() {
        let map: ListMap = ListMap::new();
        map.set(Value::Int(1), "".into());
        map.set(Value::Int(2), "2".into());
        assert_eq!(map.string(), r#"{"1":"","2":"2"}"#);
        assert_eq!(map.marshal_json().expect("json"), br#"{"1":"","2":"2"}"#);

        let empty: ListMap = ListMap::new();
        assert_eq!(empty.string(), "{}");

        let sequence: ListMap = ListMap::new();
        for code in (b'a'..=b'z').rev() {
            sequence.set(
                Value::String((code as char).to_string()),
                Value::Int(i64::from(code)),
            );
        }
        let expected = r#"{"z":122,"y":121,"x":120,"w":119,"v":118,"u":117,"t":116,"s":115,"r":114,"q":113,"p":112,"o":111,"n":110,"m":109,"l":108,"k":107,"j":106,"i":105,"h":104,"g":103,"f":102,"e":101,"d":100,"c":99,"b":98,"a":97}"#;
        assert_eq!(sequence.string(), expected);
    }

    #[test]
    fn test_list_map_unmarshal_json() {
        let map: ListMap = ListMap::new();
        map.unmarshal_json(br#"{"k1":"v1","k2":"v2"}"#)
            .expect("json");
        assert_eq!(map.size(), 2);
        assert_eq!(map.get(&"k1".into()), Some(Value::String("v1".into())));
        assert_eq!(map.get(&"k2".into()), Some(Value::String("v2".into())));

        let map: ListMap = ListMap::new();
        map.set(Value::Int(1), "1".into());
        let copy = map.deep_copy();
        copy.set(Value::Int(1), "val1".into());
        assert_ne!(map.get(&Value::Int(1)), copy.get(&Value::Int(1)));
    }

    // Translated from Test_ListMap_Iterator and Test_ListMap_Basic_Merge.
    #[test]
    fn test_list_map_iterator_and_merge() {
        let map: ListMap = ListMap::new_from(vec![pair("k1", "v1"), pair("k2", "v2")]);
        let mut count = 0;
        map.iterator(|key, _| {
            assert!(map.contains(key));
            count += 1;
            true
        });
        assert_eq!(count, 2);
        let mut stopped = 0;
        map.iterator(|_, _| {
            stopped += 1;
            false
        });
        assert_eq!(stopped, 1);

        let left: ListMap = ListMap::new_from(vec![pair("key1", "val1")]);
        let right: ListMap = ListMap::new_from(vec![pair("key2", "val2")]);
        left.merge(&right);
        assert_eq!(left.map(), vec![pair("key1", "val1"), pair("key2", "val2")]);
    }

    // Translated from the *_WithError tests of gmap_z_unit_list_k_v_map_with_error_test.go.
    #[test]
    fn test_list_map_with_error() {
        let map: ListKVMap<String, String> = ListKVMap::new();
        let value = map
            .get_or_set_func_with_error("k1".into(), || Ok("val1".to_owned()))
            .expect("value");
        assert_eq!(value, "val1");
        assert_eq!(map.get(&"k1".to_owned()), Some("val1".to_owned()));

        map.set("k2".to_owned(), "existing".to_owned());
        let mut called = false;
        let value = map
            .get_or_set_func_lock_with_error("k2".to_owned(), || {
                called = true;
                Ok("new".to_owned())
            })
            .expect("value");
        assert_eq!(value, "existing");
        assert!(!called);

        let error = map
            .get_or_set_func_with_error("k3".to_owned(), || Err(Error::new("load error")))
            .expect_err("error");
        assert_eq!(error.to_string(), "load error");
        assert!(!map.contains(&"k3".to_owned()));

        let map: ListKVMap<String, Option<String>> = ListKVMap::new();
        let value = map
            .get_or_set_func_with_error("nil".to_owned(), || Ok(None))
            .expect("value");
        assert_eq!(value, None);
        assert!(!map.contains(&"nil".to_owned()));

        let map: ListKVMap<String, Option<i64>> = ListKVMap::new();
        let stored = map
            .set_if_not_exist_func_with_error("k1".to_owned(), || Ok(None))
            .expect("bool");
        assert!(stored);
        assert!(!map.contains(&"k1".to_owned()));
        let stored = map
            .set_if_not_exist_func_with_error("k1".to_owned(), || Ok(Some(7)))
            .expect("bool");
        assert!(stored);
        assert_eq!(map.get(&"k1".to_owned()), Some(Some(7)));

        let map: ListKVMap<String, i64> = ListKVMap::new();
        map.set("a".to_owned(), 1);
        map.set("b".to_owned(), 2);
        let stored = map
            .set_if_not_exist_func_lock_with_error("c".to_owned(), || Ok(3))
            .expect("bool");
        assert!(stored);
        assert_eq!(
            map.keys(),
            vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        );
    }

    #[test]
    fn test_list_map_custom_nil_checker() {
        let map: ListKVMap<String, Option<i64>> =
            ListKVMap::new_with_checker(|value: &Option<i64>| value.is_none() || *value == Some(0));
        // Go returns true for "the key was absent", even when the value is nil
        // and is therefore not stored.
        assert!(map.set_if_not_exist("zero".to_owned(), Some(0)));
        assert!(!map.contains(&"zero".to_owned()));
        assert!(map.set_if_not_exist("one".to_owned(), Some(1)));
        assert!(map.contains(&"one".to_owned()));
    }

    // Translated from the KVMap tests (hash map with error variants).
    #[test]
    fn test_kv_map_extras() {
        let map: StrStrMap = StrStrMap::new();
        assert!(map.set_if_not_exist("k1".to_owned(), "v1".to_owned()));
        assert!(!map.set_if_not_exist("k1".to_owned(), "v2".to_owned()));
        assert_eq!(map.search(&"k1".to_owned()), Some("v1".to_owned()));
        assert_eq!(
            map.get_var(&"k1".to_owned()).map(|var| var.string()),
            Some("v1".to_owned())
        );

        let popped = map.pop().expect("pop");
        assert_eq!(popped, ("k1".to_owned(), "v1".to_owned()));
        assert!(map.pop().is_none());

        map.sets(
            [
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned()),
            ]
            .into_iter()
            .collect(),
        );
        assert_eq!(map.pops(-1).len(), 2);
        assert!(map.pops(1).is_empty());
    }

    // Translated from Test_TreeMap_Basic and Test_TreeMap_Var.
    #[test]
    fn test_tree_map_basic() {
        use crate::gtree::TreeComparator;

        let comparator: TreeComparator<Value> = Arc::new(gf_util::gutil::comparator_string);
        let map = TreeMap::new(Some(Arc::clone(&comparator)));
        map.set("key1".into(), "val1".into());
        assert_eq!(map.keys(), vec![Value::String("key1".into())]);
        assert_eq!(map.get(&"key1".into()), Some(Value::String("val1".into())));
        assert_eq!(map.size(), 1);
        assert!(!map.is_empty());

        assert_eq!(
            map.get_or_set("key2".into(), "val2".into()),
            Value::String("val2".into())
        );
        assert!(!map.set_if_not_exist("key2".into(), "val2".into()));
        assert!(map.set_if_not_exist("key3".into(), "val3".into()));
        assert_eq!(
            map.remove(&"key2".into()),
            Some(Value::String("val2".into()))
        );
        assert!(!map.contains(&"key2".into()));
        assert!(map.keys().contains(&Value::String("key3".into())));

        map.flip(None);
        let flipped = map.entries();
        assert_eq!(flipped.len(), 2);
        assert!(flipped.contains(&("val3".into(), "key3".into())));
        assert!(flipped.contains(&("val1".into(), "key1".into())));

        map.clear();
        assert!(map.is_empty());
    }

    #[test]
    fn test_tree_map_keys_are_sorted_by_comparator() {
        use crate::gtree::TreeComparator;

        let comparator: TreeComparator<Value> = Arc::new(gf_util::gutil::comparator_int);
        let map = new_tree_map_from(
            Some(comparator),
            vec![
                (Value::Int(3), Value::Int(3)),
                (Value::Int(1), Value::Int(1)),
                (Value::Int(2), Value::Int(2)),
            ],
        );
        assert_eq!(
            map.keys(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
    }

    #[test]
    fn test_kv_map_temp_structure_operations() {
        let left: StrStrMap = StrStrMap::new();
        left.set("a".to_owned(), "1".to_owned());
        left.set("b".to_owned(), "2".to_owned());
        let right: StrStrMap = StrStrMap::new();
        right.set("b".to_owned(), "3".to_owned());
        right.set("c".to_owned(), "4".to_owned());

        let (added, removed, updated) = left.diff(&right);
        assert_eq!(added, vec!["c".to_owned()]);
        assert_eq!(removed, vec!["a".to_owned()]);
        assert_eq!(updated, vec!["b".to_owned()]);

        let sub: StrStrMap = StrStrMap::new();
        sub.set("b".to_owned(), "2".to_owned());
        assert!(sub.is_sub_of(&left));
        assert!(!sub.is_sub_of(&right));

        let json_map: StrIntMap = StrIntMap::new();
        json_map.set("b".to_owned(), 2);
        json_map.set("a".to_owned(), 1);
        assert_eq!(json_map.string(), r#"{"a":1,"b":2}"#);
        json_map
            .unmarshal_json(br#"{"x":7}"#)
            .expect("json numbers become ints");
        assert_eq!(json_map.get(&"x".to_owned()), Some(7));
    }

    #[test]
    fn test_kv_map_iterator_and_merge() {
        let map: StrIntMap = StrIntMap::new();
        map.set("a".to_owned(), 1);
        map.set("b".to_owned(), 2);
        let mut seen = Vec::new();
        map.iterator(|key, value| {
            seen.push((key.clone(), *value));
            true
        });
        assert_eq!(seen, vec![("a".to_owned(), 1), ("b".to_owned(), 2)]);
        let mut stopped = 0;
        map.iterator(|_, _| {
            stopped += 1;
            false
        });
        assert_eq!(stopped, 1);

        let other: StrIntMap = StrIntMap::new();
        other.set("c".to_owned(), 3);
        map.merge(&other);
        assert_eq!(map.size(), 3);
        assert_eq!(map.get(&"c".to_owned()), Some(3));
    }
}
