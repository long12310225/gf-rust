//! Port of `os/gsession`.
//!
//! A session manager over a pluggable [`Storage`]: the in-memory storage keeps
//! session maps in `os/gcache`, the file storage keeps one `.session` file per
//! id (an 8-byte millisecond timestamp followed by JSON) with atomic writes and
//! path-escape checks.
//!
//! Divergences, recorded in ADR-055:
//!
//! - no `context.Context`;
//! - Go's `gmap.StrAnyMap` becomes `IndexMap<String, Value>`;
//! - the Redis storage is not ported (needs a Redis client);
//! - the AES option of the file storage is not ported; files are plain JSON by
//!   default like Go's default configuration;
//! - the file storage updates TTLs directly instead of queueing them for its
//!   ten-second batch timer, and expired files are cleaned by a background
//!   thread once an hour like Go;
//! - `RegenerateId` keeps the data and optionally deletes the old file.

use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_util::guid;
use indexmap::IndexMap;

use crate::gcache::{Cache, Key, Ttl};
use crate::gfile;
use crate::gtime;

/// The default storage folder, like Go's `DefaultStorageFilePath`.
pub fn default_storage_file_path() -> String {
    gfile::temp(&["gsessions"])
}

/// The error returned by storage methods a storage does not implement, like
/// Go's `ErrorDisabled`.
pub fn error_disabled() -> Error {
    gerror::new_code(
        &gcode::CODE_NOT_SUPPORTED,
        &["this feature is disabled in this storage"],
    )
}

/// Reports whether `error` is the "not supported" storage error.
pub fn is_error_disabled(error: &Error) -> bool {
    gerror::code(error) == gcode::CODE_NOT_SUPPORTED
}

/// Creates a new session id, like `NewSessionId`.
pub fn new_session_id() -> String {
    guid::s(&[])
}

/// The session storage interface, like Go's `Storage` (without the context
/// parameter).
///
/// Every method has a default implementation returning [`error_disabled`], so a
/// storage overrides only what it supports.
pub trait Storage: Send + Sync {
    /// Creates a session id, like `New`.
    fn new_session(&self, _ttl: Duration) -> Result<String, Error> {
        Err(error_disabled())
    }

    /// Returns one value of the session, like `Get`.
    fn get(&self, _session_id: &str, _key: &str) -> Result<Option<Value>, Error> {
        Err(error_disabled())
    }

    /// Returns the number of key-value pairs, like `GetSize`.
    fn get_size(&self, _session_id: &str) -> Result<usize, Error> {
        Err(error_disabled())
    }

    /// Returns all key-value pairs, like `Data`.
    fn data(&self, _session_id: &str) -> Result<Option<IndexMap<String, Value>>, Error> {
        Err(error_disabled())
    }

    /// Sets one key-value pair, like `Set`.
    fn set(
        &self,
        _session_id: &str,
        _key: &str,
        _value: Value,
        _ttl: Duration,
    ) -> Result<(), Error> {
        Err(error_disabled())
    }

    /// Sets several key-value pairs, like `SetMap`.
    fn set_map(
        &self,
        _session_id: &str,
        _data: IndexMap<String, Value>,
        _ttl: Duration,
    ) -> Result<(), Error> {
        Err(error_disabled())
    }

    /// Removes one key, like `Remove`.
    fn remove(&self, _session_id: &str, _key: &str) -> Result<(), Error> {
        Err(error_disabled())
    }

    /// Removes the whole session, like `RemoveAll`.
    fn remove_all(&self, _session_id: &str) -> Result<(), Error> {
        Err(error_disabled())
    }

    /// Loads the session on start, like `GetSession`.
    fn get_session(
        &self,
        _session_id: &str,
        _ttl: Duration,
    ) -> Result<Option<IndexMap<String, Value>>, Error> {
        Err(error_disabled())
    }

    /// Persists the session on close, like `SetSession`.
    fn set_session(
        &self,
        _session_id: &str,
        _data: &IndexMap<String, Value>,
        _ttl: Duration,
    ) -> Result<(), Error> {
        Err(error_disabled())
    }

    /// Refreshes the session TTL, like `UpdateTTL`.
    fn update_ttl(&self, _session_id: &str, _ttl: Duration) -> Result<(), Error> {
        Err(error_disabled())
    }
}

// ---------------------------------------------------------------------------
// Memory storage
// ---------------------------------------------------------------------------

/// The in-memory storage, like Go's `StorageMemory`.
pub struct StorageMemory {
    cache: Cache,
}

impl Default for StorageMemory {
    fn default() -> StorageMemory {
        StorageMemory::new()
    }
}

impl StorageMemory {
    /// Creates an in-memory storage.
    pub fn new() -> StorageMemory {
        StorageMemory {
            cache: Cache::new(),
        }
    }
}

impl Storage for StorageMemory {
    fn remove_all(&self, session_id: &str) -> Result<(), Error> {
        self.cache.remove([Key::from(session_id)]).map(|_| ())
    }

    fn get_session(
        &self,
        session_id: &str,
        _ttl: Duration,
    ) -> Result<Option<IndexMap<String, Value>>, Error> {
        match self.cache.get(session_id)? {
            Some(value) => match value.val() {
                Value::Map(map) => Ok(Some(map)),
                other => Ok(Some(gf_util::gconv::map(&other))),
            },
            None => Ok(Some(IndexMap::new())),
        }
    }

    fn set_session(
        &self,
        session_id: &str,
        data: &IndexMap<String, Value>,
        ttl: Duration,
    ) -> Result<(), Error> {
        self.cache.set(
            session_id,
            Value::Map(data.clone()),
            if ttl.is_zero() {
                Ttl::Never
            } else {
                Ttl::After(ttl)
            },
        )
    }

    fn update_ttl(&self, session_id: &str, ttl: Duration) -> Result<(), Error> {
        let ttl = if ttl.is_zero() {
            Ttl::Never
        } else {
            Ttl::After(ttl)
        };
        self.cache.update_expire(session_id, ttl).map(|_| ())
    }
}

// ---------------------------------------------------------------------------
// File storage
// ---------------------------------------------------------------------------

/// The file storage internals shared with its cleanup thread.
struct FileInner {
    path: PathBuf,
    ttl: Duration,
    closed: AtomicBool,
    crypto_enabled: AtomicBool,
    crypto_key: Mutex<Vec<u8>>,
}

/// The default session storage crypto key, like
/// `DefaultStorageFileCryptoKey` (ADR-172).
pub const DEFAULT_STORAGE_FILE_CRYPTO_KEY: &[u8] = b"Session storage file crypto key!";

/// The file-backed storage, like Go's `StorageFile`.
pub struct StorageFile {
    inner: Arc<FileInner>,
    sequence: AtomicI64,
}

impl StorageFile {
    /// Creates a file storage under `path` (the default when empty), like
    /// `NewStorageFile`.
    pub fn new(path: Option<&str>, ttl: Duration) -> Result<StorageFile, Error> {
        let storage_path = match path.filter(|path| !path.is_empty()) {
            Some(path) => gfile::search(path, &[])?,
            None => default_storage_file_path(),
        };
        if !gfile::is_writable(&storage_path) {
            gfile::mkdir(&storage_path)?;
        }
        let inner = Arc::new(FileInner {
            path: PathBuf::from(&storage_path),
            ttl,
            closed: AtomicBool::new(false),
            crypto_enabled: AtomicBool::new(false),
            crypto_key: Mutex::new(DEFAULT_STORAGE_FILE_CRYPTO_KEY.to_vec()),
        });
        let storage = StorageFile {
            inner: Arc::clone(&inner),
            sequence: AtomicI64::new(0),
        };
        // Hourly cleanup of expired session files, like Go's gtimer entry.
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("gf-gsession-file-clean".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_secs(3600));
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    if inner.closed.load(Ordering::SeqCst) {
                        break;
                    }
                    clear_expired_files(&inner);
                }
            })
            .ok();
        Ok(storage)
    }

    /// Enables or disables AES encryption of session files, like
    /// `SetCryptoEnabled` (ADR-172).
    pub fn set_crypto_enabled(&self, enabled: bool) {
        self.inner.crypto_enabled.store(enabled, Ordering::SeqCst);
    }

    /// Replaces the AES key used for session files, like `SetCryptoKey`
    /// (ADR-172).
    pub fn set_crypto_key(&self, key: &[u8]) {
        *self
            .inner
            .crypto_key
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = key.to_vec();
    }

    /// The storage directory.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Resolves the file path of `session_id`, rejecting escapes, like
    /// `sessionFilePath` + `checkSessionFilePath`.
    fn session_file_path(&self, session_id: &str) -> Result<PathBuf, Error> {
        if session_id.is_empty() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["session id is empty"],
            ));
        }
        let candidate = Path::new(session_id);
        if candidate.is_absolute() {
            return Err(escape_error());
        }
        for component in candidate.components() {
            match component {
                Component::Normal(_) => {}
                _ => return Err(escape_error()),
            }
        }
        let root = std::fs::canonicalize(&self.inner.path)
            .map_err(|error| gerror::new(error.to_string()))?;
        let file = format!("{session_id}.session");
        let full = self.inner.path.join(file);
        // Every existing ancestor, including the leaf, must stay under the
        // canonical storage root; this also catches symbolic links.
        let mut ancestor: &Path = &full;
        loop {
            if ancestor.exists() {
                let resolved = std::fs::canonicalize(ancestor)
                    .map_err(|error| gerror::new(error.to_string()))?;
                if !resolved.starts_with(&root) {
                    return Err(escape_error());
                }
                break;
            }
            match ancestor.parent() {
                Some(parent) => ancestor = parent,
                None => break,
            }
        }
        Ok(full)
    }

    /// Writes the session file atomically (temporary file + rename).
    fn write_atomic(&self, path: &Path, content: &[u8]) -> Result<(), Error> {
        let sequence = self.sequence.fetch_add(1, Ordering::SeqCst);
        let temporary =
            path.with_extension(format!("session.tmp-{}-{sequence}", std::process::id()));
        gfile::put_bytes(&temporary.to_string_lossy(), content)?;
        std::fs::rename(&temporary, path).map_err(|error| {
            let _ = gfile::remove_file(&temporary.to_string_lossy());
            gerror::new(error.to_string())
        })
    }

    /// Updates the timestamp of an existing session file.
    fn touch(&self, path: &Path) -> Result<(), Error> {
        use std::io::{Seek, SeekFrom, Write};
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(|error| gerror::new(error.to_string()))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| gerror::new(error.to_string()))?;
        file.write_all(&gtime::timestamp_milli().to_be_bytes())
            .map_err(|error| gerror::new(error.to_string()))
    }
}

impl Drop for StorageFile {
    fn drop(&mut self) {
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

/// The path-escape error.
fn escape_error() -> Error {
    gerror::new_code(
        &gcode::CODE_INVALID_PARAMETER,
        &["session id escapes the storage directory"],
    )
}

/// Removes expired session files.
fn clear_expired_files(inner: &Arc<FileInner>) {
    let path = inner.path.to_string_lossy().into_owned();
    let Ok(files) = gfile::scan_dir_file(&path, "*.session", false) else {
        return;
    };
    let now = gtime::timestamp_milli();
    for file in files {
        let Ok(metadata) = std::fs::metadata(&file) else {
            continue;
        };
        let modified = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|elapsed| elapsed.as_millis() as i64)
            .unwrap_or(0);
        if modified + (inner.ttl.as_millis() as i64) < now {
            let _ = gfile::remove_file(&file);
        }
    }
}

impl Storage for StorageFile {
    fn remove_all(&self, session_id: &str) -> Result<(), Error> {
        let path = self.session_file_path(session_id)?;
        if path.exists() {
            gfile::remove_file(&path.to_string_lossy())?;
        }
        Ok(())
    }

    fn get_session(
        &self,
        session_id: &str,
        ttl: Duration,
    ) -> Result<Option<IndexMap<String, Value>>, Error> {
        let path = self.session_file_path(session_id)?;
        let content = gfile::get_bytes(&path.to_string_lossy());
        if content.len() <= 8 {
            return Ok(None);
        }
        let stamp = i64::from_be_bytes(content[..8].try_into().unwrap_or([0; 8]));
        if stamp + (ttl.as_millis() as i64) < gtime::timestamp_milli() {
            return Ok(None);
        }
        let mut plain = content[8..].to_vec();
        if self.inner.crypto_enabled.load(Ordering::SeqCst) {
            plain = gf_crypto::gaes::decrypt(
                &plain,
                &self
                    .inner
                    .crypto_key
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                None,
            )?;
        }
        let json = gf_encoding::gjson::Json::new_from(&plain)?;
        match json.into_value() {
            Value::Map(map) => Ok(Some(map)),
            Value::Null => Ok(None),
            other => Ok(Some(gf_util::gconv::map(&other))),
        }
    }

    fn set_session(
        &self,
        session_id: &str,
        data: &IndexMap<String, Value>,
        _ttl: Duration,
    ) -> Result<(), Error> {
        let path = self.session_file_path(session_id)?;
        let body = gf_json::marshal_value(&Value::Map(data.clone()))?;
        let mut payload = body.into_bytes();
        if self.inner.crypto_enabled.load(Ordering::SeqCst) {
            payload = gf_crypto::gaes::encrypt(
                &payload,
                &self
                    .inner
                    .crypto_key
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()),
                None,
            )?;
        }
        let mut content = Vec::with_capacity(8 + payload.len());
        content.extend_from_slice(&gtime::timestamp_milli().to_be_bytes());
        content.extend_from_slice(&payload);
        self.write_atomic(&path, &content)
    }

    fn update_ttl(&self, session_id: &str, _ttl: Duration) -> Result<(), Error> {
        let path = self.session_file_path(session_id)?;
        if path.exists() {
            self.touch(&path)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Manager and session
// ---------------------------------------------------------------------------

/// The session manager, like Go's `Manager`.
pub struct Manager {
    ttl: RwLock<Duration>,
    storage: RwLock<Arc<dyn Storage>>,
}

impl Manager {
    /// Creates a manager, like `New`.
    ///
    /// With no storage it uses [`StorageFile`] under the default path.
    pub fn new(ttl: Duration, storage: Option<Arc<dyn Storage>>) -> Result<Manager, Error> {
        let storage = match storage {
            Some(storage) => storage,
            None => Arc::new(StorageFile::new(None, ttl)?),
        };
        Ok(Manager {
            ttl: RwLock::new(ttl),
            storage: RwLock::new(storage),
        })
    }

    /// Creates a session for `session_id` (a new id is generated when empty),
    /// like `Manager.New`.
    pub fn new_session(self: &Arc<Self>, session_id: Option<&str>) -> Session {
        Session {
            id: session_id.unwrap_or_default().to_owned(),
            data: IndexMap::new(),
            dirty: false,
            started: false,
            manager: Arc::clone(self),
            id_func: None,
        }
    }

    /// Replaces the storage.
    pub fn set_storage(&self, storage: Arc<dyn Storage>) {
        *self
            .storage
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = storage;
    }

    /// Returns the storage.
    pub fn storage(&self) -> Arc<dyn Storage> {
        Arc::clone(
            &self
                .storage
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
        )
    }

    /// Sets the session TTL.
    pub fn set_ttl(&self, ttl: Duration) {
        *self
            .ttl
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = ttl;
    }

    /// Returns the session TTL.
    pub fn ttl(&self) -> Duration {
        *self
            .ttl
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// A custom id generator, like Go's `idFunc`.
pub type IdFunc = Arc<dyn Fn(Duration) -> String + Send + Sync>;

/// One session bound to a request, like Go's `Session`.
pub struct Session {
    id: String,
    data: IndexMap<String, Value>,
    dirty: bool,
    started: bool,
    manager: Arc<Manager>,
    id_func: Option<IdFunc>,
}

impl Session {
    /// Lazy initialization: loads the stored session or creates an id, like
    /// `init`.
    fn init(&mut self) -> Result<(), Error> {
        if self.started {
            return Ok(());
        }
        let ttl = self.manager.ttl();
        if !self.id.is_empty() {
            let storage = self.manager.storage();
            match storage.get_session(&self.id, ttl) {
                Ok(Some(data)) => self.data = data,
                Ok(None) => {}
                Err(error) if is_error_disabled(&error) => {}
                Err(error) => return Err(error),
            }
        }
        if self.id.is_empty() {
            if let Some(id_func) = &self.id_func {
                self.id = id_func(ttl);
            } else {
                let storage = self.manager.storage();
                match storage.new_session(ttl) {
                    Ok(id) => self.id = id,
                    Err(error) if is_error_disabled(&error) => {}
                    Err(error) => return Err(error),
                }
                if self.id.is_empty() {
                    self.id = new_session_id();
                }
            }
        }
        self.started = true;
        Ok(())
    }

    /// Persists the session on close, like `Close`.
    pub fn close(&mut self) -> Result<(), Error> {
        if !self.started || self.id.is_empty() {
            return Ok(());
        }
        let storage = self.manager.storage();
        if self.dirty {
            storage.set_session(&self.id, &self.data, self.manager.ttl())?;
        } else if !self.data.is_empty() {
            storage.update_ttl(&self.id, self.manager.ttl())?;
        }
        Ok(())
    }

    /// Sets a key-value pair, like `Set`.
    pub fn set(&mut self, key: &str, value: Value) -> Result<(), Error> {
        self.init()?;
        let storage = self.manager.storage();
        if let Err(error) = storage.set(&self.id, key, value.clone(), self.manager.ttl()) {
            if !is_error_disabled(&error) {
                return Err(error);
            }
            self.data.insert(key.to_owned(), value);
        }
        self.dirty = true;
        Ok(())
    }

    /// Sets several pairs, like `SetMap`.
    pub fn set_map(&mut self, data: IndexMap<String, Value>) -> Result<(), Error> {
        self.init()?;
        let storage = self.manager.storage();
        if let Err(error) = storage.set_map(&self.id, data.clone(), self.manager.ttl()) {
            if !is_error_disabled(&error) {
                return Err(error);
            }
            for (key, value) in data {
                self.data.insert(key, value);
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// Removes keys, like `Remove`.
    pub fn remove(&mut self, keys: &[&str]) -> Result<(), Error> {
        if self.id.is_empty() {
            return Ok(());
        }
        self.init()?;
        for key in keys {
            let storage = self.manager.storage();
            if let Err(error) = storage.remove(&self.id, key) {
                if !is_error_disabled(&error) {
                    return Err(error);
                }
                self.data.shift_remove(*key);
            }
        }
        self.dirty = true;
        Ok(())
    }

    /// Removes every pair, like `RemoveAll`.
    pub fn remove_all(&mut self) -> Result<(), Error> {
        if self.id.is_empty() {
            return Ok(());
        }
        self.init()?;
        let storage = self.manager.storage();
        if let Err(error) = storage.remove_all(&self.id)
            && !is_error_disabled(&error)
        {
            return Err(error);
        }
        self.data.clear();
        self.dirty = true;
        Ok(())
    }

    /// Returns the session id, creating it when needed, like `Id`.
    pub fn id(&mut self) -> Result<String, Error> {
        self.init()?;
        Ok(self.id.clone())
    }

    /// Set a custom id before the session starts, like `SetId`.
    pub fn set_id(&mut self, id: &str) -> Result<(), Error> {
        if self.started {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_OPERATION,
                &["session already started"],
            ));
        }
        self.id = id.to_owned();
        Ok(())
    }

    /// Sets a custom id generator before the session starts, like `SetIdFunc`.
    pub fn set_id_func(&mut self, function: IdFunc) -> Result<(), Error> {
        if self.started {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_OPERATION,
                &["session already started"],
            ));
        }
        self.id_func = Some(function);
        Ok(())
    }

    /// Returns all pairs, like `Data`.
    pub fn data(&mut self) -> Result<IndexMap<String, Value>, Error> {
        if self.id.is_empty() {
            return Ok(IndexMap::new());
        }
        self.init()?;
        let storage = self.manager.storage();
        match storage.data(&self.id) {
            Ok(Some(data)) => Ok(data),
            _ => Ok(self.data.clone()),
        }
    }

    /// Returns the pair count, like `Size`.
    pub fn size(&mut self) -> Result<usize, Error> {
        if self.id.is_empty() {
            return Ok(0);
        }
        self.init()?;
        let storage = self.manager.storage();
        match storage.get_size(&self.id) {
            Ok(size) if size > 0 => Ok(size),
            _ => Ok(self.data.len()),
        }
    }

    /// Reports whether `key` exists, like `Contains`.
    pub fn contains(&mut self, key: &str) -> Result<bool, Error> {
        if self.id.is_empty() {
            return Ok(false);
        }
        Ok(self.get(key, None)?.is_some())
    }

    /// Reports whether the session was modified, like `IsDirty`.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Returns a value, like `Get`.
    pub fn get(&mut self, key: &str, default: Option<Value>) -> Result<Option<Var>, Error> {
        if self.id.is_empty() {
            return Ok(None);
        }
        self.init()?;
        let storage = self.manager.storage();
        match storage.get(&self.id, key) {
            Ok(Some(value)) => return Ok(Some(Var::new(value))),
            Ok(None) => {}
            Err(error) if !is_error_disabled(&error) => return Err(error),
            Err(_) => {}
        }
        if let Some(value) = self.data.get(key) {
            return Ok(Some(Var::new(value.clone())));
        }
        Ok(default.map(Var::new))
    }

    /// Like [`Session::id`], but panics on error.
    pub fn must_id(&mut self) -> String {
        self.id().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Session::get`], but panics on error.
    pub fn must_get(&mut self, key: &str, default: Option<Value>) -> Option<Var> {
        self.get(key, default)
            .unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Session::set`], but panics on error.
    pub fn must_set(&mut self, key: &str, value: Value) {
        self.set(key, value)
            .unwrap_or_else(|error| panic!("{error}"));
    }

    /// Like [`Session::contains`], but panics on error.
    pub fn must_contains(&mut self, key: &str) -> bool {
        self.contains(key).unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Session::data`], but panics on error.
    pub fn must_data(&mut self) -> IndexMap<String, Value> {
        self.data().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Like [`Session::size`], but panics on error.
    pub fn must_size(&mut self) -> usize {
        self.size().unwrap_or_else(|error| panic!("{error}"))
    }

    /// Regenerates the session id, keeping the data, like `RegenerateId`.
    pub fn regenerate_id(&mut self, delete_old: bool) -> Result<String, Error> {
        self.init()?;
        let ttl = self.manager.ttl();
        let new_id = match &self.id_func {
            Some(id_func) => id_func(ttl),
            None => {
                let storage = self.manager.storage();
                match storage.new_session(ttl) {
                    Ok(id) if !id.is_empty() => id,
                    _ => new_session_id(),
                }
            }
        };
        let storage = self.manager.storage();
        if let Err(error) = storage.set_session(&new_id, &self.data, ttl)
            && !is_error_disabled(&error)
        {
            return Err(error);
        }
        if delete_old
            && let Err(error) = storage.remove_all(&self.id)
            && !is_error_disabled(&error)
        {
            return Err(error);
        }
        self.id = new_id.clone();
        self.dirty = true;
        Ok(new_id)
    }

    /// Like [`Session::regenerate_id`], but panics on error.
    pub fn must_regenerate_id(&mut self, delete_old: bool) -> String {
        self.regenerate_id(delete_old)
            .unwrap_or_else(|error| panic!("{error}"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn memory_manager(ttl: Duration) -> Arc<Manager> {
        Arc::new(Manager::new(ttl, Some(Arc::new(StorageMemory::new()))).expect("manager"))
    }

    fn sandbox(name: &str) -> String {
        let path = gfile::temp(&["gf-rust-gsession-tests", name]);
        let _ = gfile::remove_all(&path);
        gfile::mkdir(&path).expect("sandbox");
        path
    }

    // Translated from Test_Session (memory storage).
    #[test]
    fn memory_session_round_trip() {
        let manager = memory_manager(Duration::from_secs(600));
        let mut session = manager.new_session(None);
        session
            .set("name", Value::String("john".to_owned()))
            .expect("set");
        session.set("age", Value::Int(18)).expect("set");
        assert_eq!(session.size().expect("size"), 2);
        assert!(session.contains("name").expect("contains"));
        assert_eq!(
            session
                .get("name", None)
                .expect("get")
                .expect("value")
                .string(),
            "john"
        );
        let id = session.id().expect("id");
        assert!(!id.is_empty());
        session.close().expect("close");

        // A new session with the same id loads the stored data.
        let mut restored = manager.new_session(Some(&id));
        assert_eq!(
            restored
                .get("age", None)
                .expect("get")
                .expect("value")
                .int64(),
            18
        );
        assert_eq!(restored.data().expect("data").len(), 2);
        restored.close().expect("close");
    }

    #[test]
    fn memory_session_remove_and_clear() {
        let manager = memory_manager(Duration::from_secs(600));
        let mut session = manager.new_session(None);
        session.set("a", Value::Int(1)).expect("set");
        session.set("b", Value::Int(2)).expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");

        let mut restored = manager.new_session(Some(&id));
        restored.remove(&["a"]).expect("remove");
        restored.close().expect("close");
        let mut restored = manager.new_session(Some(&id));
        assert!(restored.get("a", None).expect("get").is_none());
        assert!(restored.contains("b").expect("contains"));
        restored.remove_all().expect("remove all");
        restored.close().expect("close");
        let mut restored = manager.new_session(Some(&id));
        assert_eq!(restored.size().expect("size"), 0);
    }

    #[test]
    fn id_management_and_custom_id_func() {
        let manager = memory_manager(Duration::from_secs(600));
        let mut session = manager.new_session(None);
        session.set_id("custom-id").expect("set id");
        assert_eq!(session.id().expect("id"), "custom-id");
        assert!(
            session.set_id("other").is_err(),
            "started sessions are fixed"
        );

        let mut session = manager.new_session(None);
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        session
            .set_id_func(Arc::new(move |_ttl| {
                counter.fetch_add(1, Ordering::SeqCst);
                "generated-id".to_owned()
            }))
            .expect("id func");
        assert_eq!(session.id().expect("id"), "generated-id");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(session.set_id_func(Arc::new(|_| "x".to_owned())).is_err());
    }

    // Translated from Test_Session_RegenerateId.
    #[test]
    fn regenerate_id_keeps_the_data() {
        let manager = memory_manager(Duration::from_secs(600));
        let mut session = manager.new_session(None);
        session
            .set("key", Value::String("value".to_owned()))
            .expect("set");
        let old_id = session.id().expect("id");
        let new_id = session.regenerate_id(true).expect("regenerate");
        assert_ne!(old_id, new_id);
        assert_eq!(
            session
                .get("key", None)
                .expect("get")
                .expect("value")
                .string(),
            "value"
        );
        session.close().expect("close");

        // The old id was deleted, the new id keeps the data.
        let mut old_session = manager.new_session(Some(&old_id));
        assert!(old_session.get("key", None).expect("get").is_none());
        let mut new_session = manager.new_session(Some(&new_id));
        assert_eq!(
            new_session
                .get("key", None)
                .expect("get")
                .expect("value")
                .string(),
            "value"
        );
    }

    // Translated from Test_StorageMemory.
    #[test]
    fn memory_storage_ttl_expires() {
        let manager = memory_manager(Duration::from_millis(50));
        let mut session = manager.new_session(None);
        session.set("k", Value::Int(1)).expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");
        std::thread::sleep(Duration::from_millis(120));
        let mut expired = manager.new_session(Some(&id));
        assert!(expired.get("k", None).expect("get").is_none());
    }

    // Translated from Test_StorageFile.
    #[test]
    fn file_storage_round_trip() {
        let base = sandbox("file");
        let storage =
            Arc::new(StorageFile::new(Some(&base), Duration::from_secs(600)).expect("storage"));
        let manager = Arc::new(
            Manager::new(
                Duration::from_secs(600),
                Some(Arc::clone(&storage) as Arc<dyn Storage>),
            )
            .expect("manager"),
        );
        let mut session = manager.new_session(None);
        session
            .set("user", Value::String("gf".to_owned()))
            .expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");
        assert!(Path::new(&base).join(format!("{id}.session")).exists());

        let mut restored = manager.new_session(Some(&id));
        assert_eq!(
            restored
                .get("user", None)
                .expect("get")
                .expect("value")
                .string(),
            "gf"
        );
        restored.remove_all().expect("remove all");
        assert!(!Path::new(&base).join(format!("{id}.session")).exists());
        gfile::remove_all(&base).expect("cleanup");
    }

    #[test]
    fn file_storage_encrypts_session_files() {
        let base = sandbox("crypto");
        let storage =
            Arc::new(StorageFile::new(Some(&base), Duration::from_secs(600)).expect("storage"));
        storage.set_crypto_enabled(true);
        storage.set_crypto_key(b"0123456789abcdef");
        let manager = Arc::new(
            Manager::new(
                Duration::from_secs(600),
                Some(Arc::clone(&storage) as Arc<dyn Storage>),
            )
            .expect("manager"),
        );
        let mut session = manager.new_session(None);
        session
            .set("user", Value::String("secret-value".to_owned()))
            .expect("set");
        let id = session.id().expect("id");
        session.close().expect("close");

        let raw = std::fs::read(std::path::Path::new(&base).join(format!("{id}.session")))
            .expect("read session file");
        assert!(
            !raw.windows(b"secret-value".len())
                .any(|window| window == b"secret-value"),
            "the session file must not contain the plaintext"
        );

        let mut restored = manager.new_session(Some(&id));
        assert_eq!(
            restored
                .get("user", None)
                .expect("get")
                .expect("value")
                .string(),
            "secret-value"
        );
        gfile::remove_all(&base).expect("cleanup");
    }

    #[test]
    fn file_storage_rejects_traversal_ids() {
        let base = sandbox("traversal");
        let storage = StorageFile::new(Some(&base), Duration::from_secs(600)).expect("storage");
        for id in ["../escape", "a/../../escape", "/absolute/path"] {
            assert!(
                storage.get_session(id, Duration::from_secs(1)).is_err(),
                "{id} must be rejected"
            );
            assert!(storage.remove_all(id).is_err(), "{id} must be rejected");
        }
        gfile::remove_all(&base).expect("cleanup");
    }

    #[test]
    fn file_storage_expires_by_timestamp() {
        let base = sandbox("expire");
        let storage = StorageFile::new(Some(&base), Duration::from_millis(30)).expect("storage");
        let mut data = IndexMap::new();
        data.insert("k".to_owned(), Value::Int(1));
        storage
            .set_session("expiring", &data, Duration::from_millis(30))
            .expect("set");
        assert!(
            storage
                .get_session("expiring", Duration::from_millis(30))
                .expect("get")
                .is_some()
        );
        std::thread::sleep(Duration::from_millis(60));
        assert!(
            storage
                .get_session("expiring", Duration::from_millis(30))
                .expect("get")
                .is_none()
        );
        gfile::remove_all(&base).expect("cleanup");
    }

    #[test]
    fn session_ids_are_unique() {
        let first = new_session_id();
        let second = new_session_id();
        assert_eq!(first.len(), 32);
        assert_ne!(first, second);
    }

    #[test]
    fn storage_label_round_trips() {
        let storage = StorageMemory::new();
        let mut data = IndexMap::new();
        data.insert("a".to_owned(), Value::Int(1));
        storage
            .set_session("id", &data, Duration::from_secs(10))
            .expect("set");
        assert_eq!(
            storage
                .get_session("id", Duration::from_secs(10))
                .expect("get")
                .expect("data")
                .len(),
            1
        );
    }
}
