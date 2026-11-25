//! Port of `os/gcache`.
//!
//! A concurrent in-memory cache with optional TTL and LRU capacity, plus the
//! `Cache` wrapper that talks to an adapter. GoFrame's Redis adapter lives in
//! `contrib/` and is out of scope here.
//!
//! Divergences, recorded in ADR-048:
//!
//! - no `context.Context`; cache functions take no context;
//! - Go's signed `time.Duration` (negative means "delete") becomes [`Ttl`];
//! - Go's `any` keys become [`Key`], a canonical typed key derived from a
//!   [`Value`], so `1` and `"1"` stay distinct;
//! - `Data` returns `Vec<(Value, Value)>` because `Value` cannot be a map key;
//! - `GetExpire` returns `Option<Duration>` (`None` = missing, `Some(ZERO)` =
//!   never expires) instead of Go's `-1` sentinel;
//! - the `*FuncLock` variants are aliases of the plain `*Func` ones: the port
//!   never runs user callbacks under the data lock (ADR-007).

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::Duration;

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gerror::Error;
use indexmap::IndexMap;

use crate::gtime;

/// The internal expire timestamp for items that never expire, like Go's
/// `defaultMaxExpire`.
pub const DEFAULT_MAX_EXPIRE: i64 = 9_223_372_036_854;

/// How long a cache item lives.
///
/// Go uses a signed `time.Duration` where `0` means "never" and a negative
/// value means "delete the key"; Rust durations cannot be negative, so the
/// three cases are named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Ttl {
    /// Never expires (Go: `duration == 0`).
    #[default]
    Never,
    /// Deletes the key (Go: a negative duration).
    Remove,
    /// Expires after the given time (Go: a positive duration).
    After(Duration),
}

impl From<Duration> for Ttl {
    fn from(duration: Duration) -> Ttl {
        if duration.is_zero() {
            Ttl::Never
        } else {
            Ttl::After(duration)
        }
    }
}

impl Ttl {
    /// The absolute expire timestamp for this ttl.
    fn expire_at(self) -> i64 {
        match self {
            Ttl::Never => DEFAULT_MAX_EXPIRE,
            Ttl::Remove => gtime::timestamp_milli() - 1000,
            Ttl::After(duration) => gtime::timestamp_milli() + duration.as_millis() as i64,
        }
    }
}

/// A canonical cache key built from any value, like Go's `any` key.
///
/// The key keeps the value's type in its canonical text, so `1` and `"1"` do
/// not collide, and it keeps the original value so `Keys`/`Data` can return it.
#[derive(Clone, Debug)]
pub struct Key {
    canonical: String,
    value: Value,
}

impl Key {
    /// Builds a key from a value.
    pub fn from_value(value: &Value) -> Key {
        let tag = match value {
            Value::Null => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::UInt(_) => "uint",
            Value::Float(_) => "float",
            Value::String(_) => "string",
            Value::Bytes(_) => "bytes",
            Value::List(_) => "list",
            Value::Map(_) => "map",
        };
        Key {
            canonical: format!("{tag}:{}", gf_util::gconv::string(value)),
            value: value.clone(),
        }
    }

    /// The canonical text of this key.
    pub fn as_str(&self) -> &str {
        &self.canonical
    }

    /// The original typed value of this key.
    pub fn value(&self) -> &Value {
        &self.value
    }
}

impl PartialEq for Key {
    fn eq(&self, other: &Key) -> bool {
        self.canonical == other.canonical
    }
}

impl Eq for Key {}

impl std::hash::Hash for Key {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.canonical.hash(state);
    }
}

impl From<&str> for Key {
    fn from(value: &str) -> Key {
        Key::from_value(&Value::String(value.to_owned()))
    }
}

impl From<&String> for Key {
    fn from(value: &String) -> Key {
        Key::from_value(&Value::String(value.clone()))
    }
}

impl From<String> for Key {
    fn from(value: String) -> Key {
        Key::from_value(&Value::String(value))
    }
}

impl From<i64> for Key {
    fn from(value: i64) -> Key {
        Key::from_value(&Value::Int(value))
    }
}

impl From<u64> for Key {
    fn from(value: u64) -> Key {
        Key::from_value(&Value::UInt(value))
    }
}

impl From<&Value> for Key {
    fn from(value: &Value) -> Key {
        Key::from_value(value)
    }
}

impl From<Value> for Key {
    fn from(value: Value) -> Key {
        Key::from_value(&value)
    }
}

/// A cache function that computes the value to store, like Go's `Func`.
pub type Func = Arc<dyn Fn() -> Result<Value, Error> + Send + Sync>;

/// One cache entry.
#[derive(Clone, Debug)]
struct Entry {
    key: Value,
    value: Value,
    expire_at: i64,
}

impl Entry {
    fn is_expired(&self, now: i64) -> bool {
        self.expire_at <= now
    }

    fn remaining(&self, now: i64) -> Duration {
        if self.expire_at >= DEFAULT_MAX_EXPIRE {
            return Duration::ZERO;
        }
        let millis = (self.expire_at - now).max(0) as u64;
        Duration::from_millis(millis)
    }
}

/// The LRU bookkeeping, like Go's `memoryLru`.
struct Lru {
    capacity: usize,
    /// Most recently used keys at the front.
    order: VecDeque<Key>,
}

impl Lru {
    fn new(capacity: usize) -> Lru {
        Lru {
            capacity,
            order: VecDeque::new(),
        }
    }

    /// Marks `key` as recently used, returning the evicted key when the
    /// capacity is exceeded.
    fn touch(&mut self, key: &Key) -> Option<Key> {
        if let Some(position) = self.order.iter().position(|item| item == key) {
            self.order.remove(position);
        }
        self.order.push_front(key.clone());
        if self.order.len() > self.capacity {
            self.order.pop_back()
        } else {
            None
        }
    }
}

/// The shared state of the memory adapter, kept behind an `Arc` so the
/// background cleanup thread can reach it.
struct MemoryInner {
    data: Mutex<IndexMap<Key, Entry>>,
    lru: Option<Mutex<Lru>>,
    closed: AtomicBool,
}

impl MemoryInner {
    fn new(capacity: Option<usize>) -> Arc<MemoryInner> {
        Arc::new(MemoryInner {
            data: Mutex::new(IndexMap::new()),
            lru: capacity.map(|capacity| Mutex::new(Lru::new(capacity))),
            closed: AtomicBool::new(false),
        })
    }

    fn now() -> i64 {
        gtime::timestamp_milli()
    }

    /// Removes every expired entry.
    fn cleanup_expired(&self) {
        let now = Self::now();
        let mut data = self
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        data.retain(|_, entry| !entry.is_expired(now));
    }

    /// Touches the LRU entry, removing the evicted key from the data.
    fn touch_lru(&self, key: &Key) {
        let Some(lru) = &self.lru else {
            return;
        };
        let evicted = lru
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .touch(key);
        if let Some(evicted) = evicted {
            self.data
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .shift_remove(&evicted);
        }
    }

    /// Removes the key from the LRU order.
    fn forget_lru(&self, key: &Key) {
        let Some(lru) = &self.lru else {
            return;
        };
        let mut lru = lru.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(position) = lru.order.iter().position(|item| item == key) {
            lru.order.remove(position);
        }
    }

    fn clear_lru(&self) {
        if let Some(lru) = &self.lru {
            lru.lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .order
                .clear();
        }
    }
}

/// The in-memory cache adapter, like Go's `AdapterMemory`.
pub struct AdapterMemory {
    inner: Arc<MemoryInner>,
}

impl AdapterMemory {
    /// Creates a memory adapter without LRU.
    pub fn new() -> AdapterMemory {
        AdapterMemory::with_capacity(None)
    }

    /// Creates a memory adapter with an LRU capacity, like
    /// `NewAdapterMemoryLru`.
    pub fn new_lru(capacity: usize) -> AdapterMemory {
        AdapterMemory::with_capacity(Some(capacity))
    }

    fn with_capacity(capacity: Option<usize>) -> AdapterMemory {
        let inner = MemoryInner::new(capacity);
        // One background thread per adapter, like Go's gtimer loop: expired
        // entries disappear even without a reader.
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("gf-gcache-memory".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    if inner.closed.load(Ordering::SeqCst) {
                        break;
                    }
                    inner.cleanup_expired();
                }
            })
            .ok();
        AdapterMemory { inner }
    }
}

impl Default for AdapterMemory {
    fn default() -> AdapterMemory {
        AdapterMemory::new()
    }
}

/// A cache adapter, like Go's `gcache.Adapter`.
pub trait Adapter: Send + Sync {
    /// Sets `key` to `value`.
    fn set(&self, key: Key, value: Value, ttl: Ttl) -> Result<(), Error>;

    /// Batch sets several keys.
    fn set_map(&self, data: Vec<(Key, Value)>, ttl: Ttl) -> Result<(), Error>;

    /// Sets `key` only when it is absent, returning whether it was stored.
    fn set_if_not_exist(&self, key: Key, value: Value, ttl: Ttl) -> Result<bool, Error>;

    /// Sets `key` from `function` only when it is absent.
    fn set_if_not_exist_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<bool, Error>;

    /// Returns the value of `key` when present and not expired.
    fn get(&self, key: &Key) -> Result<Option<Var>, Error>;

    /// Returns the value of `key`, setting it from `value` when absent.
    fn get_or_set(&self, key: Key, value: Value, ttl: Ttl) -> Result<Option<Var>, Error>;

    /// Returns the value of `key`, computing it with `function` when absent.
    fn get_or_set_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<Option<Var>, Error>;

    /// Reports whether `key` exists.
    fn contains(&self, key: &Key) -> Result<bool, Error>;

    /// Returns the remaining lifetime of `key`.
    fn get_expire(&self, key: &Key) -> Result<Option<Duration>, Error>;

    /// Removes `keys`, returning the last removed value.
    fn remove(&self, keys: &[Key]) -> Result<Option<Var>, Error>;

    /// Updates the value of `key`, keeping its expiry.
    fn update(&self, key: &Key, value: Value) -> Result<(Option<Var>, bool), Error>;

    /// Updates the expiry of `key`, returning the previous one.
    fn update_expire(&self, key: &Key, ttl: Ttl) -> Result<Option<Duration>, Error>;

    /// Returns the number of live entries.
    fn size(&self) -> Result<usize, Error>;

    /// Returns a snapshot of the live entries.
    fn data(&self) -> Result<Vec<(Value, Value)>, Error>;

    /// Returns the live keys.
    fn keys(&self) -> Result<Vec<Value>, Error>;

    /// Returns the live values.
    fn values(&self) -> Result<Vec<Value>, Error>;

    /// Removes every entry.
    fn clear(&self) -> Result<(), Error>;

    /// Stops the adapter.
    fn close(&self) -> Result<(), Error>;
}

impl Adapter for AdapterMemory {
    fn set(&self, key: Key, value: Value, ttl: Ttl) -> Result<(), Error> {
        if matches!(ttl, Ttl::Remove) || value.is_nil() {
            self.remove(&[key])?;
            return Ok(());
        }
        let expire_at = ttl.expire_at();
        self.inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                key.clone(),
                Entry {
                    key: key.value().clone(),
                    value,
                    expire_at,
                },
            );
        self.inner.touch_lru(&key);
        Ok(())
    }

    fn set_map(&self, data: Vec<(Key, Value)>, ttl: Ttl) -> Result<(), Error> {
        for (key, value) in data {
            self.set(key, value, ttl)?;
        }
        Ok(())
    }

    fn set_if_not_exist(&self, key: Key, value: Value, ttl: Ttl) -> Result<bool, Error> {
        if self.get(&key)?.is_some() {
            return Ok(false);
        }
        self.set(key, value, ttl)?;
        Ok(true)
    }

    fn set_if_not_exist_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<bool, Error> {
        if self.get(&key)?.is_some() {
            return Ok(false);
        }
        let value = function()?;
        if !value.is_nil() {
            self.set(key, value, ttl)?;
        }
        Ok(true)
    }

    fn get(&self, key: &Key) -> Result<Option<Var>, Error> {
        let now = MemoryInner::now();
        let entry = {
            let mut data = self
                .inner
                .data
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match data.get(key) {
                Some(entry) if !entry.is_expired(now) => Some(entry.clone()),
                Some(_) => {
                    data.shift_remove(key);
                    None
                }
                None => None,
            }
        };
        match entry {
            Some(entry) => {
                self.inner.touch_lru(key);
                Ok(Some(Var::new(entry.value)))
            }
            None => Ok(None),
        }
    }

    fn get_or_set(&self, key: Key, value: Value, ttl: Ttl) -> Result<Option<Var>, Error> {
        if let Some(value) = self.get(&key)? {
            return Ok(Some(value));
        }
        self.set(key.clone(), value, ttl)?;
        self.get(&key)
    }

    fn get_or_set_func(&self, key: Key, function: Func, ttl: Ttl) -> Result<Option<Var>, Error> {
        if let Some(value) = self.get(&key)? {
            return Ok(Some(value));
        }
        let value = function()?;
        if value.is_nil() {
            return Ok(None);
        }
        self.set(key.clone(), value, ttl)?;
        self.get(&key)
    }

    fn contains(&self, key: &Key) -> Result<bool, Error> {
        Ok(self.get(key)?.is_some())
    }

    fn get_expire(&self, key: &Key) -> Result<Option<Duration>, Error> {
        let now = MemoryInner::now();
        let entry = self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(key)
            .cloned();
        match entry {
            Some(entry) if !entry.is_expired(now) => Ok(Some(entry.remaining(now))),
            Some(_) => Ok(None),
            None => Ok(None),
        }
    }

    fn remove(&self, keys: &[Key]) -> Result<Option<Var>, Error> {
        let mut last = None;
        let mut data = self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for key in keys {
            if let Some(entry) = data.shift_remove(key) {
                last = Some(entry.value);
            }
        }
        drop(data);
        for key in keys {
            self.inner.forget_lru(key);
        }
        Ok(last.map(Var::new))
    }

    fn update(&self, key: &Key, value: Value) -> Result<(Option<Var>, bool), Error> {
        if value.is_nil() {
            let existed = self.contains(key)?;
            let old = self.remove(std::slice::from_ref(key))?;
            return Ok((old, existed));
        }
        let mut data = self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = data.get_mut(key) else {
            return Ok((None, false));
        };
        let old = entry.value.clone();
        entry.value = value;
        drop(data);
        self.inner.touch_lru(key);
        Ok((Some(Var::new(old)), true))
    }

    fn update_expire(&self, key: &Key, ttl: Ttl) -> Result<Option<Duration>, Error> {
        let now = MemoryInner::now();
        if matches!(ttl, Ttl::Remove) {
            let old = self.get_expire(key)?;
            self.remove(std::slice::from_ref(key))?;
            return Ok(old);
        }
        let mut data = self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(entry) = data.get_mut(key) else {
            return Ok(None);
        };
        let old = entry.remaining(now);
        entry.expire_at = ttl.expire_at();
        drop(data);
        self.inner.touch_lru(key);
        Ok(Some(old))
    }

    fn size(&self) -> Result<usize, Error> {
        let now = MemoryInner::now();
        Ok(self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|entry| !entry.is_expired(now))
            .count())
    }

    fn data(&self) -> Result<Vec<(Value, Value)>, Error> {
        let now = MemoryInner::now();
        Ok(self
            .inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .filter(|entry| !entry.is_expired(now))
            .map(|entry| (entry.key.clone(), entry.value.clone()))
            .collect())
    }

    fn keys(&self) -> Result<Vec<Value>, Error> {
        Ok(self.data()?.into_iter().map(|(key, _)| key).collect())
    }

    fn values(&self) -> Result<Vec<Value>, Error> {
        Ok(self.data()?.into_iter().map(|(_, value)| value).collect())
    }

    fn clear(&self) -> Result<(), Error> {
        self.inner
            .data
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.inner.clear_lru();
        Ok(())
    }

    fn close(&self) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

/// The cache wrapper, like Go's `gcache.Cache`.
pub struct Cache {
    adapter: RwLock<Arc<dyn Adapter>>,
}

impl Cache {
    /// Creates a cache over the default memory adapter, like `New`.
    pub fn new() -> Cache {
        Cache {
            adapter: RwLock::new(Arc::new(AdapterMemory::new())),
        }
    }

    /// Creates a memory cache with an LRU capacity.
    pub fn new_lru(capacity: usize) -> Cache {
        Cache {
            adapter: RwLock::new(Arc::new(AdapterMemory::new_lru(capacity))),
        }
    }

    /// Creates a cache over `adapter`, like `NewWithAdapter`.
    pub fn with_adapter(adapter: Arc<dyn Adapter>) -> Cache {
        Cache {
            adapter: RwLock::new(adapter),
        }
    }

    /// Replaces the adapter.
    pub fn set_adapter(&self, adapter: Arc<dyn Adapter>) {
        *self
            .adapter
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = adapter;
    }

    /// Returns the current adapter.
    pub fn adapter(&self) -> Arc<dyn Adapter> {
        Arc::clone(
            &self
                .adapter
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    /// Sets `key` to `value`, like `Set`.
    pub fn set(&self, key: impl Into<Key>, value: Value, ttl: Ttl) -> Result<(), Error> {
        self.adapter().set(key.into(), value, ttl)
    }

    /// Batch sets `data`, like `SetMap`.
    pub fn set_map(
        &self,
        data: impl IntoIterator<Item = (Key, Value)>,
        ttl: Ttl,
    ) -> Result<(), Error> {
        self.adapter().set_map(data.into_iter().collect(), ttl)
    }

    /// Sets `key` only when absent, like `SetIfNotExist`.
    pub fn set_if_not_exist(
        &self,
        key: impl Into<Key>,
        value: Value,
        ttl: Ttl,
    ) -> Result<bool, Error> {
        self.adapter().set_if_not_exist(key.into(), value, ttl)
    }

    /// Sets `key` from a function only when absent, like
    /// `SetIfNotExistFunc`.
    pub fn set_if_not_exist_func(
        &self,
        key: impl Into<Key>,
        function: Func,
        ttl: Ttl,
    ) -> Result<bool, Error> {
        self.adapter()
            .set_if_not_exist_func(key.into(), function, ttl)
    }

    /// Like [`Cache::set_if_not_exist_func`]; kept for API parity.
    pub fn set_if_not_exist_func_lock(
        &self,
        key: impl Into<Key>,
        function: Func,
        ttl: Ttl,
    ) -> Result<bool, Error> {
        self.set_if_not_exist_func(key, function, ttl)
    }

    /// Returns the value of `key`, like `Get`.
    pub fn get(&self, key: impl Into<Key>) -> Result<Option<Var>, Error> {
        self.adapter().get(&key.into())
    }

    /// Returns the value of `key`, setting it when absent, like `GetOrSet`.
    pub fn get_or_set(
        &self,
        key: impl Into<Key>,
        value: Value,
        ttl: Ttl,
    ) -> Result<Option<Var>, Error> {
        self.adapter().get_or_set(key.into(), value, ttl)
    }

    /// Returns the value of `key`, computing it when absent, like
    /// `GetOrSetFunc`.
    pub fn get_or_set_func(
        &self,
        key: impl Into<Key>,
        function: Func,
        ttl: Ttl,
    ) -> Result<Option<Var>, Error> {
        self.adapter().get_or_set_func(key.into(), function, ttl)
    }

    /// Like [`Cache::get_or_set_func`]; kept for API parity.
    pub fn get_or_set_func_lock(
        &self,
        key: impl Into<Key>,
        function: Func,
        ttl: Ttl,
    ) -> Result<Option<Var>, Error> {
        self.get_or_set_func(key, function, ttl)
    }

    /// Reports whether `key` exists, like `Contains`.
    pub fn contains(&self, key: impl Into<Key>) -> Result<bool, Error> {
        self.adapter().contains(&key.into())
    }

    /// Returns the remaining lifetime of `key`, like `GetExpire`.
    pub fn get_expire(&self, key: impl Into<Key>) -> Result<Option<Duration>, Error> {
        self.adapter().get_expire(&key.into())
    }

    /// Removes `keys`, returning the last removed value, like `Remove`.
    pub fn remove(&self, keys: impl IntoIterator<Item = Key>) -> Result<Option<Var>, Error> {
        self.adapter().remove(&keys.into_iter().collect::<Vec<_>>())
    }

    /// Removes `keys`, ignoring the returned value, like `Removes`.
    pub fn removes(&self, keys: impl IntoIterator<Item = Key>) -> Result<(), Error> {
        self.remove(keys).map(|_| ())
    }

    /// Updates the value of `key`, keeping its expiry, like `Update`.
    pub fn update(&self, key: impl Into<Key>, value: Value) -> Result<(Option<Var>, bool), Error> {
        self.adapter().update(&key.into(), value)
    }

    /// Updates the expiry of `key`, like `UpdateExpire`.
    pub fn update_expire(&self, key: impl Into<Key>, ttl: Ttl) -> Result<Option<Duration>, Error> {
        self.adapter().update_expire(&key.into(), ttl)
    }

    /// Returns the number of live entries, like `Size`.
    pub fn size(&self) -> Result<usize, Error> {
        self.adapter().size()
    }

    /// Returns a snapshot of live entries, like `Data`.
    pub fn data(&self) -> Result<Vec<(Value, Value)>, Error> {
        self.adapter().data()
    }

    /// Returns the live keys, like `Keys`.
    pub fn keys(&self) -> Result<Vec<Value>, Error> {
        self.adapter().keys()
    }

    /// Returns the live keys as strings, like `KeyStrings`.
    pub fn key_strings(&self) -> Result<Vec<String>, Error> {
        Ok(self.keys()?.iter().map(gf_util::gconv::string).collect())
    }

    /// Returns the live values, like `Values`.
    pub fn values(&self) -> Result<Vec<Value>, Error> {
        self.adapter().values()
    }

    /// Removes every entry, like `Clear`.
    pub fn clear(&self) -> Result<(), Error> {
        self.adapter().clear()
    }

    /// Stops the adapter, like `Close`.
    pub fn close(&self) -> Result<(), Error> {
        self.adapter().close()
    }

    /// Like [`Cache::get`], but panics on error.
    pub fn must_get(&self, key: impl Into<Key>) -> Option<Var> {
        self.get(key).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::get_or_set`], but panics on error.
    pub fn must_get_or_set(&self, key: impl Into<Key>, value: Value, ttl: Ttl) -> Option<Var> {
        self.get_or_set(key, value, ttl)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::get_or_set_func`], but panics on error.
    pub fn must_get_or_set_func(
        &self,
        key: impl Into<Key>,
        function: Func,
        ttl: Ttl,
    ) -> Option<Var> {
        self.get_or_set_func(key, function, ttl)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::contains`], but panics on error.
    pub fn must_contains(&self, key: impl Into<Key>) -> bool {
        self.contains(key).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::get_expire`], but panics on error.
    pub fn must_get_expire(&self, key: impl Into<Key>) -> Option<Duration> {
        self.get_expire(key)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::size`], but panics on error.
    pub fn must_size(&self) -> usize {
        self.size().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::keys`], but panics on error.
    pub fn must_keys(&self) -> Vec<Value> {
        self.keys().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Cache::values`], but panics on error.
    pub fn must_values(&self) -> Vec<Value> {
        self.values().unwrap_or_else(|error| panic!("{error}"))
    }
}

impl Default for Cache {
    fn default() -> Cache {
        Cache::new()
    }
}

/// The package default cache, like Go's `defaultCache`.
fn default_cache() -> &'static Arc<Cache> {
    static DEFAULT: OnceLock<Arc<Cache>> = OnceLock::new();
    DEFAULT.get_or_init(|| Arc::new(Cache::new()))
}

/// Sets a key on the default cache.
pub fn set(key: impl Into<Key>, value: Value, ttl: Ttl) -> Result<(), Error> {
    default_cache().set(key, value, ttl)
}

/// Batch sets keys on the default cache.
pub fn set_map(data: impl IntoIterator<Item = (Key, Value)>, ttl: Ttl) -> Result<(), Error> {
    default_cache().set_map(data, ttl)
}

/// Sets a key only when absent on the default cache.
pub fn set_if_not_exist(key: impl Into<Key>, value: Value, ttl: Ttl) -> Result<bool, Error> {
    default_cache().set_if_not_exist(key, value, ttl)
}

/// Gets a value from the default cache.
pub fn get(key: impl Into<Key>) -> Result<Option<Var>, Error> {
    default_cache().get(key)
}

/// Returns or sets a value on the default cache.
pub fn get_or_set(key: impl Into<Key>, value: Value, ttl: Ttl) -> Result<Option<Var>, Error> {
    default_cache().get_or_set(key, value, ttl)
}

/// Returns or computes a value on the default cache.
pub fn get_or_set_func(
    key: impl Into<Key>,
    function: Func,
    ttl: Ttl,
) -> Result<Option<Var>, Error> {
    default_cache().get_or_set_func(key, function, ttl)
}

/// Reports whether a key exists on the default cache.
pub fn contains(key: impl Into<Key>) -> Result<bool, Error> {
    default_cache().contains(key)
}

/// Removes keys from the default cache.
pub fn remove(keys: impl IntoIterator<Item = Key>) -> Result<Option<Var>, Error> {
    default_cache().remove(keys)
}

/// Updates a value on the default cache.
pub fn update(key: impl Into<Key>, value: Value) -> Result<(Option<Var>, bool), Error> {
    default_cache().update(key, value)
}

/// Updates a key's expiry on the default cache.
pub fn update_expire(key: impl Into<Key>, ttl: Ttl) -> Result<Option<Duration>, Error> {
    default_cache().update_expire(key, ttl)
}

/// Returns the default cache size.
pub fn size() -> Result<usize, Error> {
    default_cache().size()
}

/// Returns the default cache keys.
pub fn keys() -> Result<Vec<Value>, Error> {
    default_cache().keys()
}

/// Returns the default cache values.
pub fn values() -> Result<Vec<Value>, Error> {
    default_cache().values()
}

/// Clears the default cache.
pub fn clear() -> Result<(), Error> {
    default_cache().clear()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn memory_cache() -> Cache {
        Cache::new()
    }

    // Translated from TestCache_Set / TestCache_Basic.
    #[test]
    fn set_get_contains() {
        let cache = memory_cache();
        cache.set(1_i64, Value::Int(11), Ttl::Never).expect("set");
        assert_eq!(cache.get(1_i64).expect("get").expect("value").int64(), 11);
        assert!(cache.contains(1_i64).expect("contains"));
        assert!(cache.get(2_i64).expect("get").is_none());
        cache.close().expect("close");
    }

    // Translated from TestCache_Set_Expire.
    #[test]
    fn items_expire() {
        let cache = memory_cache();
        cache
            .set(
                2_i64,
                Value::Int(22),
                Ttl::After(Duration::from_millis(100)),
            )
            .expect("set");
        assert_eq!(cache.get(2_i64).expect("get").expect("value").int64(), 22);
        std::thread::sleep(Duration::from_millis(200));
        assert!(cache.get(2_i64).expect("get").is_none());
        std::thread::sleep(Duration::from_millis(1100));
        assert_eq!(cache.size().expect("size"), 0);
        cache.close().expect("close");
    }

    // Translated from TestCache_Update.
    #[test]
    fn update_keeps_the_expiry() {
        let cache = memory_cache();
        cache
            .set(1_i64, Value::Int(11), Ttl::After(Duration::from_secs(3)))
            .expect("set");
        let expire_before = cache.get_expire(1_i64).expect("expire").expect("some");
        let (old, exist) = cache.update(1_i64, Value::Int(12)).expect("update");
        assert!(exist);
        assert_eq!(old.expect("old").int64(), 11);
        let expire_after = cache.get_expire(1_i64).expect("expire").expect("some");
        assert_eq!(
            expire_before.as_secs(),
            expire_after.as_secs(),
            "the expiry must be kept"
        );
        assert_eq!(cache.get(1_i64).expect("get").expect("value").int64(), 12);
    }

    // Translated from TestCache_UpdateExpire.
    #[test]
    fn update_expire_changes_the_lifetime() {
        let cache = memory_cache();
        cache
            .set(1_i64, Value::Int(11), Ttl::After(Duration::from_secs(3)))
            .expect("set");
        let old = cache
            .update_expire(1_i64, Ttl::After(Duration::from_secs(10)))
            .expect("update")
            .expect("some");
        assert!(old.as_secs() >= 2 && old.as_secs() <= 3, "got {old:?}");
        let new = cache.get_expire(1_i64).expect("expire").expect("some");
        assert!(new.as_secs() >= 9 && new.as_secs() <= 10, "got {new:?}");
        assert!(
            cache
                .update_expire(2_i64, Ttl::After(Duration::from_secs(1)))
                .expect("update missing")
                .is_none()
        );
    }

    // Translated from TestCache_Keys_Values.
    #[test]
    fn keys_and_values() {
        let cache = memory_cache();
        for index in 0..10_i64 {
            cache
                .set(index, Value::Int(index * 10), Ttl::Never)
                .expect("set");
        }
        assert_eq!(cache.keys().expect("keys").len(), 10);
        assert_eq!(cache.values().expect("values").len(), 10);
        assert_eq!(cache.size().expect("size"), 10);
        assert_eq!(cache.key_strings().expect("strings").len(), 10);
    }

    // Translated from TestCache_LRU.
    #[test]
    fn lru_evicts_the_oldest_key() {
        let cache = Cache::new_lru(2);
        cache.set(1_i64, Value::Int(1), Ttl::Never).expect("set");
        cache.set(2_i64, Value::Int(2), Ttl::Never).expect("set");
        // Touch 1 so that 2 becomes the least recently used.
        assert!(cache.get(1_i64).expect("get").is_some());
        cache.set(3_i64, Value::Int(3), Ttl::Never).expect("set");
        assert!(cache.contains(1_i64).expect("contains"));
        assert!(
            !cache.contains(2_i64).expect("contains"),
            "2 must be evicted"
        );
        assert!(cache.contains(3_i64).expect("contains"));
        assert_eq!(cache.size().expect("size"), 2);
    }

    // Translated from TestCache_LRU_MissedKeysDoNotEvict.
    #[test]
    fn lru_missed_keys_do_not_evict() {
        let cache = Cache::new_lru(2);
        cache.set(1_i64, Value::Int(1), Ttl::Never).expect("set");
        cache.set(2_i64, Value::Int(2), Ttl::Never).expect("set");
        assert!(cache.get(100_i64).expect("miss").is_none());
        assert!(cache.get(101_i64).expect("miss").is_none());
        assert!(cache.contains(1_i64).expect("contains"));
        assert!(cache.contains(2_i64).expect("contains"));
    }

    // Translated from TestCache_SetIfNotExist and the Func variants.
    #[test]
    fn set_if_not_exist_variants() {
        let cache = memory_cache();
        assert!(
            cache
                .set_if_not_exist(1_i64, Value::Int(11), Ttl::Never)
                .expect("first")
        );
        assert!(
            !cache
                .set_if_not_exist(1_i64, Value::Int(12), Ttl::Never)
                .expect("second")
        );
        assert_eq!(cache.get(1_i64).expect("get").expect("value").int64(), 11);

        let called = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&called);
        let stored = cache
            .set_if_not_exist_func(
                2_i64,
                Arc::new(move || {
                    *counter.lock().unwrap() += 1;
                    Ok(Value::Int(22))
                }),
                Ttl::Never,
            )
            .expect("func");
        assert!(stored);
        assert_eq!(*called.lock().unwrap(), 1);
        assert_eq!(cache.get(2_i64).expect("get").expect("value").int64(), 22);

        // The function result `Null` stores nothing but still reports true.
        assert!(
            cache
                .set_if_not_exist_func(3_i64, Arc::new(|| Ok(Value::Null)), Ttl::Never)
                .expect("nil func")
        );
        assert!(!cache.contains(3_i64).expect("contains"));
    }

    // Translated from TestCache_SetMap.
    #[test]
    fn set_map_sets_every_pair() {
        let cache = memory_cache();
        cache
            .set_map(
                vec![
                    (Key::from(1_i64), Value::Int(1)),
                    (Key::from(2_i64), Value::Int(2)),
                ],
                Ttl::Never,
            )
            .expect("set map");
        assert_eq!(cache.size().expect("size"), 2);
        assert_eq!(cache.get(2_i64).expect("get").expect("value").int64(), 2);
    }

    // Translated from TestCache_GetOrSet and the Func variants.
    #[test]
    fn get_or_set_variants() {
        let cache = memory_cache();
        assert_eq!(
            cache
                .get_or_set(1_i64, Value::Int(11), Ttl::Never)
                .expect("get or set")
                .expect("value")
                .int64(),
            11
        );
        assert_eq!(
            cache
                .get_or_set(1_i64, Value::Int(12), Ttl::Never)
                .expect("get or set")
                .expect("value")
                .int64(),
            11
        );

        let called = Arc::new(Mutex::new(0));
        let counter = Arc::clone(&called);
        let value = cache
            .get_or_set_func(
                2_i64,
                Arc::new(move || {
                    *counter.lock().unwrap() += 1;
                    Ok(Value::Int(22))
                }),
                Ttl::Never,
            )
            .expect("func")
            .expect("value");
        assert_eq!(value.int64(), 22);
        assert_eq!(*called.lock().unwrap(), 1);
        // A `Null` function result is not stored and does not error.
        assert!(
            cache
                .get_or_set_func(3_i64, Arc::new(|| Ok(Value::Null)), Ttl::Never)
                .expect("nil")
                .is_none()
        );
        assert!(!cache.contains(3_i64).expect("contains"));
    }

    // Translated from TestCache_Clear.
    #[test]
    fn clear_empties_the_cache() {
        let cache = memory_cache();
        cache.set(1_i64, Value::Int(1), Ttl::Never).expect("set");
        cache.set(2_i64, Value::Int(2), Ttl::Never).expect("set");
        cache.clear().expect("clear");
        assert_eq!(cache.size().expect("size"), 0);
        assert!(cache.keys().expect("keys").is_empty());
    }

    // Translated from TestCache_Removes.
    #[test]
    fn remove_returns_the_last_value() {
        let cache = memory_cache();
        for key in 1..=3_i64 {
            cache
                .set(key, Value::Int(key * 10), Ttl::Never)
                .expect("set");
        }
        let last = cache
            .remove(vec![Key::from(1_i64), Key::from(2_i64)])
            .expect("remove")
            .expect("value");
        assert_eq!(last.int64(), 20);
        assert_eq!(cache.size().expect("size"), 1);
        cache.removes(vec![Key::from(3_i64)]).expect("removes");
        assert_eq!(cache.size().expect("size"), 0);
        assert!(
            cache
                .remove(vec![Key::from(9_i64)])
                .expect("remove")
                .is_none()
        );
    }

    // Translated from TestCache_SetConcurrency.
    #[test]
    fn concurrent_writers_are_safe() {
        let cache = Arc::new(memory_cache());
        let mut handles = Vec::new();
        for index in 0..10_i64 {
            let cache = Arc::clone(&cache);
            handles.push(std::thread::spawn(move || {
                for offset in 0..100_i64 {
                    cache
                        .set(index * 100 + offset, Value::Int(offset), Ttl::Never)
                        .expect("set");
                }
            }));
        }
        for handle in handles {
            handle.join().expect("join");
        }
        assert_eq!(cache.size().expect("size"), 1000);
    }

    // Translated from TestCache_Basic_Must.
    #[test]
    fn must_variants_return_values() {
        let cache = memory_cache();
        cache.set(1_i64, Value::Int(11), Ttl::Never).expect("set");
        assert_eq!(cache.must_get(1_i64).expect("value").int64(), 11);
        assert!(cache.must_contains(1_i64));
        assert_eq!(cache.must_size(), 1);
        assert_eq!(cache.must_keys().len(), 1);
        assert_eq!(cache.must_values().len(), 1);
        assert!(cache.must_get(9_i64).is_none());
    }

    #[test]
    fn typed_keys_stay_distinct() {
        let cache = memory_cache();
        cache.set(1_i64, Value::Int(11), Ttl::Never).expect("set");
        cache.set("1", Value::Int(22), Ttl::Never).expect("set");
        assert_eq!(cache.size().expect("size"), 2);
        assert_eq!(cache.get(1_i64).expect("get").expect("value").int64(), 11);
        assert_eq!(cache.get("1").expect("get").expect("value").int64(), 22);
    }
}
