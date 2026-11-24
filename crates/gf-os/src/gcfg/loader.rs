//! The generic struct loader, like `gcfg_loader.go`.

use std::sync::{Arc, Mutex, RwLock};

use gf_core::Value;
use gf_errors::gerror::{self, Error};
use gf_util::gconv;
use serde::de::DeserializeOwned;

use super::{Adapter, Config, OperationContext, WatcherFunc};

/// The handler called after a successful reload, like Go's `onChange`.
pub type ChangeFunc<T> = Arc<dyn Fn(&T) -> Result<(), Error> + Send + Sync>;
/// The custom converter, like Go's `converter` field.
pub type ConverterFunc<T> = Arc<dyn Fn(&Value, &mut T) -> Result<(), Error> + Send + Sync>;
/// The watch error handler, like Go's `watchErrorFunc`.
pub type WatchErrorFunc = Arc<dyn Fn(&Error) + Send + Sync>;

/// The mutable half of a loader, shared with its watcher callback.
struct LoaderState<T> {
    target: RwLock<T>,
    on_change: Mutex<Option<ChangeFunc<T>>>,
    converter: Mutex<Option<ConverterFunc<T>>>,
    watch_error: Mutex<Option<WatchErrorFunc>>,
}

/// A generic configuration loader, like Go's `Loader[T]`.
///
/// The loader reads a property key (or the whole document) from a [`Config`],
/// scans it into `T`, and can watch the adapter for changes that trigger a
/// reload. The port scans with `gconv`, so `T` implements `DeserializeOwned`
/// instead of Go's reflective `Scan`.
pub struct Loader<T: Default + DeserializeOwned + Clone + Send + Sync + 'static> {
    config: Arc<Config>,
    property_key: String,
    state: Arc<LoaderState<T>>,

    watcher_name: RwLock<String>,
}

impl<T: Default + DeserializeOwned + Clone + Send + Sync + 'static> Loader<T> {
    /// Creates a loader, like Go's `NewLoader`.
    pub fn new(config: Arc<Config>, property_key: &str, target: Option<T>) -> Loader<T> {
        Loader {
            config,
            property_key: property_key.to_owned(),
            state: Arc::new(LoaderState {
                // Like Go's `new(T)`, a missing target starts from `Default`.
                target: RwLock::new(target.unwrap_or_default()),
                on_change: Mutex::new(None),
                converter: Mutex::new(None),
                watch_error: Mutex::new(None),
            }),

            watcher_name: RwLock::new(String::new()),
        }
    }

    /// Creates a loader over an adapter, like `NewLoaderWithAdapter`.
    pub fn with_adapter(
        adapter: Arc<dyn Adapter>,
        property_key: &str,
        target: Option<T>,
    ) -> Loader<T> {
        Loader::new(
            Arc::new(Config::with_adapter(adapter)),
            property_key,
            target,
        )
    }

    /// Sets the reload callback, like Go's `OnChange`.
    pub fn on_change(&self, function: ChangeFunc<T>) -> &Self {
        *self
            .state
            .on_change
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(function);
        self
    }

    /// Sets a custom converter, like Go's `SetConverter`.
    pub fn set_converter(&self, converter: ConverterFunc<T>) {
        *self
            .state
            .converter
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(converter);
    }

    /// Sets the watch error handler, like `SetWatchErrorHandler`.
    pub fn set_watch_error_handler(&self, function: WatchErrorFunc) {
        *self
            .state
            .watch_error
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(function);
    }

    /// Keeps the flag Go uses to reuse the target struct, like
    /// `SetReuseTargetStruct`.
    ///
    /// The port always scans into a fresh value and swaps it in, which is the
    /// safe behaviour, so the flag has no effect (ADR-045).
    pub fn set_reuse_target_struct(&self, _reuse: bool) {}

    /// Loads the configuration into the target, like Go's `Load`.
    pub fn load(&self) -> Result<(), Error> {
        load_into(&self.config, &self.property_key, &self.state)
    }

    /// Like [`Loader::load`], but panics on error.
    pub fn must_load(&self) {
        if let Err(error) = self.load() {
            panic!("{error}");
        }
    }

    /// Starts watching for changes, like Go's `Watch`.
    pub fn watch(&self, name: &str) -> Result<(), Error> {
        if name.is_empty() {
            return Err(gerror::new("watcher name cannot be empty"));
        }
        let config = Arc::clone(&self.config);
        let property_key = self.property_key.clone();
        let state = Arc::clone(&self.state);
        let watcher: WatcherFunc = Arc::new(move |_context: &OperationContext| {
            if let Err(error) = load_into(&config, &property_key, &state) {
                let handler = state
                    .watch_error
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone();
                match handler {
                    Some(handler) => handler(&error),
                    None => eprintln!("gf-gcfg: configuration reload failed: {error}"),
                }
            }
        });
        self.config.adapter().add_watcher(name, watcher);
        *self
            .watcher_name
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = name.to_owned();
        Ok(())
    }

    /// Like [`Loader::watch`], but panics on error.
    pub fn must_watch(&self, name: &str) {
        if let Err(error) = self.watch(name) {
            panic!("{error}");
        }
    }

    /// Loads and watches, like `MustLoadAndWatch`.
    pub fn must_load_and_watch(&self, name: &str) {
        self.must_load();
        self.must_watch(name);
    }

    /// Returns a copy of the current target, like Go's `Get`.
    pub fn get(&self) -> T {
        self.state
            .target
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Stops watching, like Go's `StopWatch`.
    pub fn stop_watch(&self) -> Result<bool, Error> {
        let name = self
            .watcher_name
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if name.is_empty() {
            return Err(gerror::new("no watcher name specified"));
        }
        self.config.adapter().remove_watcher(&name);
        Ok(true)
    }

    /// Reports whether the loader is watching, like Go's `IsWatching`.
    pub fn is_watching(&self) -> bool {
        let name = self
            .watcher_name
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if name.is_empty() {
            return false;
        }
        self.config.adapter().is_watching(&name)
    }
}

/// Reads `property_key` from `config` and stores the scanned value in `state`.
fn load_into<T: Default + DeserializeOwned + Clone + Send + Sync + 'static>(
    config: &Config,
    property_key: &str,
    state: &LoaderState<T>,
) -> Result<(), Error> {
    let data = if property_key.is_empty() || property_key == "." {
        Value::Map(config.data()?)
    } else {
        match config.get(property_key)? {
            Some(value) => value.val(),
            None => Value::Null,
        }
    };
    let mut target = state
        .target
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let converter = state
        .converter
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    match converter {
        Some(converter) => converter(&data, &mut target)?,
        // Go scans a nil document without touching the struct; the same keeps
        // the loader's defaults when a key is missing.
        None if data.is_nil() => {}
        None => {
            *target = gconv::scan::<T>(&data)?;
        }
    }
    let snapshot = target.clone();
    drop(target);
    let change = state
        .on_change
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    if let Some(change) = change {
        return change(&snapshot);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde::Deserialize;

    use super::*;
    use crate::gcfg::{AdapterContent, Config};

    /// The struct the Go loader tests scan into.
    #[derive(Clone, Debug, Default, Deserialize, PartialEq)]
    struct AppConfig {
        name: String,
        port: u64,
    }

    fn content_config(content: &str) -> (Arc<AdapterContent>, Arc<Config>) {
        let adapter = Arc::new(AdapterContent::new(Some(content)).expect("adapter"));
        let config = Arc::new(Config::with_adapter(
            Arc::clone(&adapter) as Arc<dyn Adapter>
        ));
        (adapter, config)
    }

    // Translated from TestLoader_Load.
    #[test]
    fn loader_loads_the_whole_document() {
        let (_adapter, config) = content_config(r#"{"name":"app","port":9000}"#);
        let loader = Loader::<AppConfig>::new(config, "", None);
        loader.load().expect("load");
        assert_eq!(loader.get().name, "app");
        assert_eq!(loader.get().port, 9000);
    }

    // Translated from TestLoader_LoadWithPropertyKey.
    #[test]
    fn loader_loads_a_property_key() {
        let (_adapter, config) = content_config(r#"{"app":{"name":"nested","port":81}}"#);
        let loader = Loader::<AppConfig>::new(config, "app", None);
        loader.load().expect("load");
        assert_eq!(loader.get().name, "nested");
        assert_eq!(loader.get().port, 81);
    }

    // Translated from TestLoader_LoadWithDefaultValues: a missing key leaves
    // the target untouched instead of failing.
    #[test]
    fn loader_keeps_defaults_for_missing_values() {
        let target = AppConfig {
            name: "default".to_owned(),
            port: 1,
        };
        let (_adapter, config) = content_config("{}");
        let loader = Loader::<AppConfig>::new(config, "app", Some(target));
        loader.load().expect("load");
        assert_eq!(loader.get().name, "default");
        assert_eq!(loader.get().port, 1);
    }

    // Translated from TestLoader_WatchAndOnChange.
    #[test]
    fn loader_reloads_when_the_adapter_changes() {
        let (adapter, config) = content_config(r#"{"name":"before","port":1}"#);
        let loader = Loader::<AppConfig>::new(config, "", None);
        loader.load().expect("load");
        let (tx, rx) = std::sync::mpsc::channel();
        loader.on_change(Arc::new(move |updated: &AppConfig| {
            let _ = tx.send(updated.name.clone());
            Ok(())
        }));
        loader.watch("reload").expect("watch");
        assert!(loader.is_watching());

        adapter
            .set_content(r#"{"name":"after","port":2}"#)
            .expect("set content");

        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).expect("change"),
            "after"
        );
        assert_eq!(loader.get().port, 2);
    }

    // Translated from TestLoader_SetConverter.
    #[test]
    fn loader_uses_a_custom_converter() {
        let (_adapter, config) = content_config(r#"{"name":"x"}"#);
        let loader = Loader::<AppConfig>::new(config, "app", None);
        loader.set_converter(Arc::new(|_data: &Value, target: &mut AppConfig| {
            target.name = "converted".to_owned();
            target.port = 42;
            Ok(())
        }));
        loader.load().expect("load");
        assert_eq!(loader.get().name, "converted");
        assert_eq!(loader.get().port, 42);
    }

    // Translated from TestLoader_IsWatchingAndStopWatch and
    // TestLoader_StopWatchWithoutWatcher.
    #[test]
    fn loader_stop_watch() {
        let (_adapter, config) = content_config("{}");
        let loader = Loader::<AppConfig>::new(config, "", None);
        assert!(loader.stop_watch().is_err());
        loader.watch("stop").expect("watch");
        assert!(loader.is_watching());
        assert!(loader.stop_watch().expect("stop"));
        assert!(!loader.is_watching());
    }

    // Translated from TestLoader_SetWatchErrorHandler: a reload failure goes to
    // the configured handler.
    #[test]
    fn loader_reports_reload_errors_to_the_handler() {
        let (adapter, config) = content_config(r#"{"name":"ok","port":1}"#);
        let loader = Loader::<AppConfig>::new(config, "", None);
        let (tx, rx) = std::sync::mpsc::channel();
        let notify = Arc::new(Mutex::new(tx));
        loader.set_watch_error_handler(Arc::new(move |error: &Error| {
            let _ = notify
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .send(error.to_string());
        }));
        loader.set_converter(Arc::new(|_data: &Value, _target: &mut AppConfig| {
            Err(gerror::new("converter failed"))
        }));
        loader.watch("errors").expect("watch");
        // A change triggers a reload, whose converter fails and reaches the
        // error handler instead of panicking.
        adapter
            .set_content(r#"{"name":"next","port":2}"#)
            .expect("set content");
        let message = rx.recv_timeout(Duration::from_secs(5)).expect("error");
        assert!(message.contains("converter failed"), "got {message}");
        // The target keeps its previous value after the failed reload.
        assert_eq!(loader.get().name, "");
    }
}
