//! Port of `os/gcfg`.
//!
//! GoFrame's configuration manager reads a file (or in-memory content) through
//! a `gjson` document, caches it per file name, watches the file through
//! `gfsnotify`, and lets applications register logical watchers that reload
//! their settings. The port keeps that structure and drops Go's
//! `context.Context` (ADR-034): the information Go stores as context values
//! travels as an explicit [`OperationContext`], and watchers receive it
//! directly.
//!
//! Divergences, recorded in ADR-045:
//!
//! - no `context.Context`; [`OperationContext`] replaces the context keys;
//! - one `Adapter` trait carries the watcher methods with default no-op
//!   implementations, instead of `Adapter` plus a separate `WatcherAdapter`;
//! - `gfile::MainPkgPath` is absent, so the automatic main-package search path
//!   is not added;
//! - the file watcher keeps one registration per file and does not retry
//!   re-arming with a sleep loop; it re-adds the watch when an event finds the
//!   file present.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use gf_container::gvar::Var;
use gf_core::Value;
use gf_encoding::gjson::{self, Json};
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_internal::{command, utils};
use indexmap::IndexMap;

mod file;
mod loader;

pub use file::AdapterFile;
pub use loader::Loader;

use crate::genv;

/// The default instance name, like Go's `DefaultInstanceName`.
pub const DEFAULT_INSTANCE_NAME: &str = "config";
/// The default configuration file name, like `DefaultConfigFileName`.
pub const DEFAULT_CONFIG_FILE_NAME: &str = "config";

/// The type of a configuration operation, like Go's `OperationType`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OperationType {
    /// A value was set through the API.
    Set,
    /// The underlying file was written.
    Write,
    /// The underlying file was renamed.
    Rename,
    /// The underlying file was removed.
    Remove,
    /// The underlying file was created.
    Create,
    /// The underlying file permissions changed.
    Chmod,
    /// The configuration cache was cleared.
    Clear,
    /// The configuration was updated.
    Update,
    /// No operation was recorded.
    #[default]
    None,
}

impl OperationType {
    /// The text GoFrame stores in its operation context.
    pub fn as_str(&self) -> &'static str {
        match self {
            OperationType::Set => "set",
            OperationType::Write => "write",
            OperationType::Rename => "rename",
            OperationType::Remove => "remove",
            OperationType::Create => "create",
            OperationType::Chmod => "chmod",
            OperationType::Clear => "clear",
            OperationType::Update => "update",
            OperationType::None => "",
        }
    }
}

impl std::fmt::Display for OperationType {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// What a watcher is told about the change, replacing Go's context values.
#[derive(Clone, Debug, Default)]
pub struct OperationContext {
    /// The configuration file name (or file path) the change belongs to.
    pub file_name: String,
    /// The absolute file path, when one was involved.
    pub file_path: String,
    /// The file type (extension without the dot).
    pub file_type: String,
    /// The operation that triggered the notification.
    pub operation: OperationType,
    /// The key set through the API, for `Set`.
    pub key: String,
    /// The value set through the API, for `Set`.
    pub value: Option<Value>,
    /// The content set through the API, for `SetContent`.
    pub content: Option<Value>,
}

/// A logical configuration watcher, like Go's `WatcherFunc`.
pub type WatcherFunc = Arc<dyn Fn(&OperationContext) + Send + Sync>;

/// Retrieves configuration values, like Go's `Adapter` plus `WatcherAdapter`.
///
/// The watcher methods default to doing nothing, so an adapter that cannot
/// watch needs no extra code.
pub trait Adapter: Send + Sync {
    /// Reports whether the adapter has configuration available.
    fn available(&self, resource: Option<&str>) -> bool;

    /// Returns the value at `pattern`, if any.
    fn get(&self, pattern: &str) -> Result<Option<Value>, Error>;

    /// Returns the whole configuration document as a map.
    fn data(&self) -> Result<IndexMap<String, Value>, Error>;

    /// Registers a logical watcher.
    fn add_watcher(&self, _name: &str, _function: WatcherFunc) {}

    /// Removes a logical watcher.
    fn remove_watcher(&self, _name: &str) {}

    /// Returns the registered watcher names.
    fn watcher_names(&self) -> Vec<String> {
        Vec::new()
    }

    /// Reports whether a watcher with `name` is registered.
    fn is_watching(&self, _name: &str) -> bool {
        false
    }
}

/// The shared watcher registry, like Go's `WatcherRegistry`.
#[derive(Default)]
pub struct WatcherRegistry {
    watchers: Mutex<IndexMap<String, WatcherFunc>>,
}

impl WatcherRegistry {
    /// Creates an empty registry.
    pub fn new() -> WatcherRegistry {
        WatcherRegistry::default()
    }

    /// Reports whether `name` is registered.
    pub fn is_watching(&self, name: &str) -> bool {
        self.watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(name)
    }

    /// Registers or replaces the watcher `name`.
    pub fn add(&self, name: &str, function: WatcherFunc) {
        self.watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.to_owned(), function);
    }

    /// Removes the watcher `name`.
    pub fn remove(&self, name: &str) {
        self.watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shift_remove(name);
    }

    /// Returns every registered name.
    pub fn names(&self) -> Vec<String> {
        self.watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect()
    }

    /// Runs every watcher with `context`, like Go's `Notify`.
    ///
    /// Each watcher runs on its own thread, so a slow or panicking watcher
    /// cannot affect the others.
    pub fn notify(&self, context: &OperationContext) {
        let functions: Vec<WatcherFunc> = self
            .watchers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect();
        for function in functions {
            let context = context.clone();
            let spawned = std::thread::Builder::new()
                .name("gf-gcfg-watcher".to_owned())
                .spawn(move || function(&context));
            if let Err(error) = spawned {
                eprintln!("gf-gcfg: watcher thread unavailable: {error}");
            }
        }
    }
}

/// The configuration manager, like Go's `gcfg.Config`.
pub struct Config {
    adapter: RwLock<Arc<dyn Adapter>>,
}

impl Config {
    /// Creates a configuration manager over the file adapter, like `New`.
    pub fn new() -> Result<Config, Error> {
        let adapter: Arc<dyn Adapter> = AdapterFile::new(None)?;
        Ok(Config {
            adapter: RwLock::new(adapter),
        })
    }

    /// Creates a configuration manager over `adapter`, like `NewWithAdapter`.
    pub fn with_adapter(adapter: Arc<dyn Adapter>) -> Config {
        Config {
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

    /// Reports whether configuration is available, like `Available`.
    pub fn available(&self, resource: Option<&str>) -> bool {
        self.adapter().available(resource)
    }

    /// Returns the value at `pattern`, like Go's `Get`.
    pub fn get(&self, pattern: &str) -> Result<Option<Var>, Error> {
        Ok(self.adapter().get(pattern)?.map(Var::new))
    }

    /// Returns the value at `pattern` or `default`, like `Get` with `def`.
    pub fn get_or(&self, pattern: &str, default: Value) -> Result<Var, Error> {
        Ok(self.get(pattern)?.unwrap_or_else(|| Var::new(default)))
    }

    /// Returns the configuration value, falling back to the environment, like
    /// Go's `GetWithEnv`.
    pub fn get_with_env(
        &self,
        pattern: &str,
        default: Option<Value>,
    ) -> Result<Option<Var>, Error> {
        let value = self.get(&utils::format_cmd_key(pattern))?;
        if let Some(value) = value {
            return Ok(Some(value));
        }
        if let Some(value) = genv::get(&utils::format_env_key(pattern), None) {
            return Ok(Some(value));
        }
        Ok(default.map(Var::new))
    }

    /// Returns the configuration value, falling back to the command line, like
    /// Go's `GetWithCmd`.
    pub fn get_with_cmd(
        &self,
        pattern: &str,
        default: Option<Value>,
    ) -> Result<Option<Var>, Error> {
        let value = self.get(&utils::format_cmd_key(pattern))?;
        if let Some(value) = value {
            return Ok(Some(value));
        }
        let option = command::get_opt(&utils::format_cmd_key(pattern), None);
        if !option.is_empty() {
            return Ok(Some(Var::new(Value::String(option))));
        }
        Ok(default.map(Var::new))
    }

    /// Returns the value with the standard priority: command line, then
    /// environment, then configuration, then `default`, like `GetEffective`.
    pub fn get_effective(
        &self,
        pattern: &str,
        default: Option<Value>,
    ) -> Result<Option<Var>, Error> {
        let command_key = utils::format_cmd_key(pattern);
        if command::contains_opt(&command_key) {
            return Ok(Some(Var::new(Value::String(command::get_opt(
                &command_key,
                None,
            )))));
        }
        if let Some(value) = genv::get(&utils::format_env_key(pattern), None) {
            return Ok(Some(value));
        }
        if let Some(value) = self.get(&command_key)? {
            return Ok(Some(value));
        }
        Ok(default.map(Var::new))
    }

    /// Returns the whole configuration as a map, like `Data`.
    pub fn data(&self) -> Result<IndexMap<String, Value>, Error> {
        self.adapter().data()
    }

    /// Like [`Config::get`], but panics on error (Go's `MustGet`).
    pub fn must_get(&self, pattern: &str) -> Option<Var> {
        match self.get(pattern) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        }
    }

    /// Like [`Config::get_with_env`], but panics on error.
    pub fn must_get_with_env(&self, pattern: &str) -> Option<Var> {
        match self.get_with_env(pattern, None) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        }
    }

    /// Like [`Config::get_with_cmd`], but panics on error.
    pub fn must_get_with_cmd(&self, pattern: &str) -> Option<Var> {
        match self.get_with_cmd(pattern, None) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        }
    }

    /// Like [`Config::get_effective`], but panics on error.
    pub fn must_get_effective(&self, pattern: &str) -> Option<Var> {
        match self.get_effective(pattern, None) {
            Ok(value) => value,
            Err(error) => panic!("{error}"),
        }
    }

    /// Like [`Config::data`], but panics on error.
    pub fn must_data(&self) -> IndexMap<String, Value> {
        match self.data() {
            Ok(data) => data,
            Err(error) => panic!("{error}"),
        }
    }
}

/// The process-wide configuration instances, like Go's `localInstances`.
fn local_instances() -> &'static Mutex<HashMap<String, Arc<Config>>> {
    static INSTANCES: OnceLock<Mutex<HashMap<String, Arc<Config>>>> = OnceLock::new();
    INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the shared configuration instance, like Go's `Instance`.
///
/// A non-default name also becomes the default configuration file name, as Go
/// does (the first successful instance wins).
pub fn instance(name: Option<&str>) -> Option<Arc<Config>> {
    let instance_name = match name {
        Some(name) if !name.is_empty() => name,
        _ => DEFAULT_INSTANCE_NAME,
    };
    let mut instances = local_instances()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(config) = instances.get(instance_name) {
        return Some(Arc::clone(config));
    }
    let adapter = AdapterFile::new(None).ok()?;
    if instance_name != DEFAULT_INSTANCE_NAME {
        adapter.set_file_name(instance_name);
    }
    let config = Arc::new(Config::with_adapter(adapter));
    instances.insert(instance_name.to_owned(), Arc::clone(&config));
    Some(config)
}

/// The configuration adapter backed by in-memory content, like
/// `AdapterContent`.
pub struct AdapterContent {
    json: RwLock<Option<Arc<Json>>>,
    watchers: WatcherRegistry,
}

impl Default for AdapterContent {
    fn default() -> AdapterContent {
        AdapterContent {
            json: RwLock::new(None),
            watchers: WatcherRegistry::new(),
        }
    }
}

impl AdapterContent {
    /// Creates an adapter, optionally with initial content, like
    /// `NewAdapterContent`.
    pub fn new(content: Option<&str>) -> Result<AdapterContent, Error> {
        let adapter = AdapterContent::default();
        if let Some(content) = content {
            adapter.set_content(content)?;
        }
        Ok(adapter)
    }

    /// Replaces the configuration content, like Go's `SetContent`.
    pub fn set_content(&self, content: &str) -> Result<(), Error> {
        let json = gjson::load_content(content.as_bytes(), true).map_err(|error| {
            gerror::wrap(Some(error), "load configuration content failed".to_owned())
                .unwrap_or_else(|| gerror::new("load configuration content failed"))
        })?;
        *self
            .json
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(json));
        let context = OperationContext {
            operation: OperationType::Set,
            content: Some(Value::String(content.to_owned())),
            ..OperationContext::default()
        };
        self.watchers.notify(&context);
        Ok(())
    }
}

impl Adapter for AdapterContent {
    fn available(&self, _resource: Option<&str>) -> bool {
        self.json
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
    }

    fn get(&self, pattern: &str) -> Result<Option<Value>, Error> {
        Ok(self
            .json
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|json| json.get(pattern))
            .filter(|value| !value.is_nil()))
    }

    fn data(&self) -> Result<IndexMap<String, Value>, Error> {
        Ok(self
            .json
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|json| json.map())
            .unwrap_or_default())
    }

    fn add_watcher(&self, name: &str, function: WatcherFunc) {
        self.watchers.add(name, function);
    }

    fn remove_watcher(&self, name: &str) {
        self.watchers.remove(name);
    }

    fn watcher_names(&self) -> Vec<String> {
        self.watchers.names()
    }

    fn is_watching(&self, name: &str) -> bool {
        self.watchers.is_watching(name)
    }
}

/// Reports the "not found" code of a missing configuration file, so callers
/// can tell it apart from real failures like Go's `gcode.CodeNotFound`.
pub fn is_not_found(error: &Error) -> bool {
    gerror::code(error) == gcode::CODE_NOT_FOUND
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content_adapter(content: &str) -> Arc<AdapterContent> {
        Arc::new(AdapterContent::new(Some(content)).expect("adapter"))
    }

    // Translated from gvalid_z_unit_adapter_content_test.go.
    #[test]
    fn content_adapter_reads_patterns() {
        let adapter = content_adapter(r#"{"app":{"name":"goframe","port":8080},"list":["a","b"]}"#);
        assert!(adapter.available(None));
        assert_eq!(
            adapter.get("app.name").expect("get"),
            Some(Value::String("goframe".to_owned()))
        );
        assert!(matches!(
            adapter.get("app.port").expect("get"),
            Some(Value::UInt(8080))
        ));
        assert_eq!(
            adapter.get("list.1").expect("get"),
            Some(Value::String("b".to_owned()))
        );
        assert_eq!(adapter.get("missing").expect("get"), None);
        let data = adapter.data().expect("data");
        assert_eq!(data.len(), 2);
    }

    #[test]
    fn content_adapter_notifies_watchers() {
        let adapter = content_adapter("{}");
        let (tx, rx) = std::sync::mpsc::channel();
        adapter.add_watcher(
            "test",
            Arc::new(move |context: &OperationContext| {
                let _ = tx.send(context.operation);
            }),
        );
        assert!(adapter.is_watching("test"));
        assert_eq!(adapter.watcher_names(), vec!["test".to_owned()]);
        adapter.set_content(r#"{"a":1}"#).expect("set content");
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .expect("event"),
            OperationType::Set
        );
        assert!(matches!(
            adapter.get("a").expect("get"),
            Some(Value::UInt(1))
        ));
        adapter.remove_watcher("test");
        assert!(!adapter.is_watching("test"));
    }

    #[test]
    fn config_wraps_adapter_results() {
        let config = Config::with_adapter(content_adapter(r#"{"a":{"b":2}}"#));
        assert!(config.available(None));
        assert_eq!(
            config.get("a.b").expect("get").map(|value| value.int64()),
            Some(2)
        );
        assert_eq!(
            config
                .get_or("missing", Value::Int(7))
                .expect("get or")
                .int64(),
            7
        );
        assert!(config.get("missing").expect("get").is_none());
        assert_eq!(config.data().expect("data").len(), 1);
        assert_eq!(config.must_get("a.b").map(|value| value.int64()), Some(2));
    }

    #[test]
    fn config_priority_env_then_config() {
        let config = Config::with_adapter(content_adapter(r#"{"gf":{"test":{"name":"file"}}}"#));
        // `GetWithEnv` prefers the file and falls back to the environment.
        let from_file = config.get_with_env("gf.test.name", None).expect("get");
        assert_eq!(
            from_file.map(|value| value.string()).as_deref(),
            Some("file")
        );
        assert_eq!(
            config
                .get_with_env("gf.test.missing", Some(Value::String("def".to_owned())))
                .expect("get")
                .map(|value| value.string()),
            Some("def".to_owned())
        );
    }
}
