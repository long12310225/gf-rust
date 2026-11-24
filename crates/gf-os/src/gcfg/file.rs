//! The file-backed configuration adapter, like `gcfg_adapter_file*.go`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};

use gf_core::Value;
use gf_encoding::gjson::{self, Json};
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_internal::command;
use indexmap::IndexMap;

use crate::gfile;
use crate::gfsnotify;
use crate::gres;
use crate::gspath;

use super::{
    Adapter, DEFAULT_CONFIG_FILE_NAME, OperationContext, OperationType, WatcherFunc,
    WatcherRegistry,
};

/// The supported configuration file types, like `supportedFileTypes`.
pub const SUPPORTED_FILE_TYPES: [&str; 7] =
    ["toml", "yaml", "yml", "json", "ini", "xml", "properties"];

/// Prefixes tried inside the resource manager, like `resourceTryFolders`.
const RESOURCE_TRY_FOLDERS: [&str; 9] = [
    "",
    "/",
    "config/",
    "config",
    "/config",
    "/config/",
    "manifest/config/",
    "manifest/config",
    "/manifest/config",
];

/// Prefixes tried in the local file system, like `localSystemTryFolders`.
const LOCAL_SYSTEM_TRY_FOLDERS: [&str; 3] = ["", "config/", "manifest/config"];

/// The command/environment key for the configuration file name.
pub const COMMAND_ENV_KEY_FOR_FILE: &str = "gf.gcfg.file";
/// The command/environment key for the configuration search path.
pub const COMMAND_ENV_KEY_FOR_PATH: &str = "gf.gcfg.path";

/// Customized configuration content, like Go's `customConfigContentMap`.
fn custom_content() -> &'static Mutex<IndexMap<String, String>> {
    static CONTENT: OnceLock<Mutex<IndexMap<String, String>>> = OnceLock::new();
    CONTENT.get_or_init(|| Mutex::new(IndexMap::new()))
}

/// Every adapter created, so content changes can clear their caches.
fn live_adapters() -> &'static Mutex<Vec<Weak<AdapterFile>>> {
    static ADAPTERS: OnceLock<Mutex<Vec<Weak<AdapterFile>>>> = OnceLock::new();
    ADAPTERS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Clears the cached document `name` in every live adapter.
fn clear_instance_caches(name: &str) {
    let mut adapters = live_adapters()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    adapters.retain(|adapter| {
        let Some(adapter) = adapter.upgrade() else {
            return false;
        };
        adapter
            .json_map
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .shift_remove(name);
        true
    });
}

/// One file watcher registration.
struct FileWatcher {
    callback_id: i32,
}

/// The file-backed adapter, like Go's `AdapterFile`.
pub struct AdapterFile {
    file_name: RwLock<String>,
    search_paths: RwLock<Vec<String>>,
    json_map: Arc<RwLock<IndexMap<String, Arc<Json>>>>,
    violence_check: AtomicBool,
    watchers: Arc<WatcherRegistry>,
    registrations: Mutex<HashMap<String, FileWatcher>>,
}

impl AdapterFile {
    /// Creates an adapter, like Go's `NewAdapterFile`.
    ///
    /// The default file name comes from the argument, then from the
    /// `gf.gcfg.file` command/environment key, then `config`. The search path
    /// comes from `gf.gcfg.path` when set, otherwise from the working
    /// directory and the binary directory.
    pub fn new(file_name: Option<&str>) -> Result<Arc<AdapterFile>, Error> {
        let used_name = match file_name {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => {
                let custom = command::get_opt_with_env(COMMAND_ENV_KEY_FOR_FILE, None);
                if custom.is_empty() {
                    DEFAULT_CONFIG_FILE_NAME.to_owned()
                } else {
                    custom
                }
            }
        };
        let adapter = Arc::new(AdapterFile {
            file_name: RwLock::new(used_name),
            search_paths: RwLock::new(Vec::new()),
            json_map: Arc::new(RwLock::new(IndexMap::new())),
            violence_check: AtomicBool::new(false),
            watchers: Arc::new(WatcherRegistry::new()),
            registrations: Mutex::new(HashMap::new()),
        });
        let custom_path = command::get_opt_with_env(COMMAND_ENV_KEY_FOR_PATH, None);
        if !custom_path.is_empty() {
            if !gfile::exists(&custom_path) {
                return Err(gerror::newf(
                    format!("configuration directory path \"{custom_path}\" does not exist"),
                    Vec::new(),
                ));
            }
            adapter.set_path(&custom_path)?;
        } else {
            let _ = adapter.add_path(&[&gfile::pwd()]);
            let self_dir = gfile::self_dir();
            if !self_dir.is_empty() && gfile::exists(&self_dir) {
                let _ = adapter.add_path(&[&self_dir]);
            }
        }
        live_adapters()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Arc::downgrade(&adapter));
        Ok(adapter)
    }

    /// Enables or disables the hierarchical key check, like
    /// `SetViolenceCheck`.
    ///
    /// The port's path lookup always splits on dots (ADR-045), so the flag is
    /// recorded and the cache is cleared.
    pub fn set_violence_check(&self, check: bool) {
        self.violence_check.store(check, Ordering::SeqCst);
        self.clear();
    }

    /// Sets the default configuration file name.
    pub fn set_file_name(&self, file_name: &str) {
        *self
            .file_name
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = file_name.to_owned();
    }

    /// Returns the default configuration file name.
    pub fn file_name(&self) -> String {
        self.file_name
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Sets the value at `pattern` in the cached document, like Go's `Set`.
    pub fn set(&self, pattern: &str, value: Value) -> Result<(), Error> {
        let name = self.file_name();
        if let Some(json) = self.get_json(None)? {
            let mut updated = (*json).clone();
            updated.set(pattern, value.clone());
            self.json_map
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(name.clone(), Arc::new(updated));
        }
        let file_path = self.file_path_internal(&name);
        let context = OperationContext {
            file_name: name.clone(),
            file_path,
            file_type: gfile::ext_name(&name),
            operation: OperationType::Set,
            key: pattern.to_owned(),
            value: Some(value),
            content: None,
        };
        self.watchers.notify(&context);
        Ok(())
    }

    /// Clears the parsed document cache, like Go's `Clear`.
    pub fn clear(&self) {
        self.json_map
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        let name = self.file_name();
        let file_path = self.file_path_internal(&name);
        let context = OperationContext {
            file_name: name.clone(),
            file_path,
            file_type: gfile::ext_name(&name),
            operation: OperationType::Clear,
            ..OperationContext::default()
        };
        self.watchers.notify(&context);
    }

    /// Prints the cached document, like Go's `Dump`.
    pub fn dump(&self) {
        if let Ok(Some(json)) = self.get_json(None) {
            json.dump();
        }
    }

    /// Sets the search path, replacing the current ones, like `SetPath`.
    pub fn set_path(&self, directory_path: &str) -> Result<(), Error> {
        let (real_path, is_dir) = self.resolve_directory(directory_path);
        if real_path.is_empty() {
            return Err(self.directory_error("SetPath", directory_path));
        }
        if !is_dir {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("SetPath failed: path \"{directory_path}\" should be directory type"),
                Vec::new(),
            ));
        }
        {
            let paths = self
                .search_paths
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if paths.contains(&real_path) {
                return Ok(());
            }
        }
        self.json_map
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        *self
            .search_paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = vec![real_path];
        Ok(())
    }

    /// Adds search directories, like `AddPath`.
    pub fn add_path(&self, directories: &[&str]) -> Result<(), Error> {
        for directory in directories {
            let (real_path, is_dir) = self.resolve_directory(directory);
            if real_path.is_empty() {
                return Err(self.directory_error("AddPath", directory));
            }
            if !is_dir {
                return Err(gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("AddPath failed: path \"{directory}\" should be directory type"),
                    Vec::new(),
                ));
            }
            let mut paths = self
                .search_paths
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !paths.contains(&real_path) {
                paths.push(real_path);
            }
        }
        Ok(())
    }

    /// Returns the search directories.
    pub fn get_paths(&self) -> Vec<String> {
        self.search_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Returns the absolute path of a configuration file, like
    /// `GetFilePath`.
    pub fn get_file_path(&self, file_name: Option<&str>) -> Result<String, Error> {
        let used = file_name
            .map(str::to_owned)
            .unwrap_or_else(|| self.file_name());
        let extension = gfile::ext_name(&used);
        let mut found = self.file_path_internal(&used);
        if (found.is_empty() || gfile::is_dir(&found))
            && !SUPPORTED_FILE_TYPES.contains(&extension.as_str())
        {
            for file_type in SUPPORTED_FILE_TYPES {
                let candidate = format!("{used}.{file_type}");
                let path = self.file_path_internal(&candidate);
                if !path.is_empty() {
                    found = path;
                    break;
                }
            }
        }
        if !found.is_empty() {
            return Ok(found);
        }
        Err(self.missing_file_error(&used, &extension))
    }

    /// Sets customized content for `file_name`, like `SetContent`.
    pub fn set_content(&self, content: &str, file_name: Option<&str>) {
        let name = file_name
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_CONFIG_FILE_NAME)
            .to_owned();
        custom_content()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name.clone(), content.to_owned());
        clear_instance_caches(&name);
        let context = OperationContext {
            file_name: name.clone(),
            file_type: gfile::ext_name(&name),
            operation: OperationType::Set,
            content: Some(Value::String(content.to_owned())),
            ..OperationContext::default()
        };
        self.watchers.notify(&context);
    }

    /// Returns customized content for `file_name`, like `GetContent`.
    pub fn get_content(&self, file_name: Option<&str>) -> String {
        let name = file_name
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_CONFIG_FILE_NAME);
        custom_content()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .cloned()
            .unwrap_or_default()
    }

    /// Removes customized content for `file_name`, like `RemoveContent`.
    pub fn remove_content(&self, file_name: Option<&str>) {
        let name = file_name
            .filter(|name| !name.is_empty())
            .unwrap_or(DEFAULT_CONFIG_FILE_NAME);
        {
            let mut content = custom_content()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if content.shift_remove(name).is_some() {
                clear_instance_caches(name);
            }
        }
        let context = OperationContext {
            file_name: name.to_owned(),
            file_type: gfile::ext_name(name),
            operation: OperationType::Remove,
            ..OperationContext::default()
        };
        self.watchers.notify(&context);
    }

    /// Removes every customized content, like `ClearContent`.
    pub fn clear_content(&self) {
        let names: Vec<String> = custom_content()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .keys()
            .cloned()
            .collect();
        custom_content()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        for name in names {
            clear_instance_caches(&name);
        }
        let context = OperationContext {
            operation: OperationType::Clear,
            ..OperationContext::default()
        };
        self.watchers.notify(&context);
    }

    /// Resolves a directory through the resource manager, the file system and
    /// the current search paths.
    fn resolve_directory(&self, directory_path: &str) -> (String, bool) {
        if let Some(file) = gres::get(directory_path) {
            let is_dir = file.file_info().is_dir();
            return (directory_path.to_owned(), is_dir);
        }
        let real_path = gfile::real_path(directory_path);
        if !real_path.is_empty() {
            let is_dir = gfile::is_dir(&real_path);
            return (real_path, is_dir);
        }
        let paths = self
            .search_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for search_path in paths {
            let (found, _) = gspath::search(&search_path, directory_path, &[]);
            if !found.is_empty() {
                let is_dir = gfile::is_dir(&found);
                return (found, is_dir);
            }
        }
        (String::new(), false)
    }

    /// The error `SetPath`/`AddPath` return for a missing directory.
    fn directory_error(&self, operation: &str, directory_path: &str) -> Error {
        let paths = self
            .search_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if paths.is_empty() {
            return gerror::new(format!(
                "{operation} failed: path \"{directory_path}\" does not exist"
            ));
        }
        let mut message = format!(
            "{operation} failed: cannot find directory \"{directory_path}\" in following paths:"
        );
        for (index, path) in paths.iter().enumerate() {
            message.push_str(&format!("\n{}. {path}", index + 1));
        }
        gerror::new(message)
    }

    /// The error `GetFilePath` returns when nothing matched.
    fn missing_file_error(&self, name: &str, extension: &str) -> Error {
        let paths = self
            .search_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let mut message = String::new();
        if !paths.is_empty() {
            if SUPPORTED_FILE_TYPES.contains(&extension) {
                message.push_str(&format!(
                    "specified config file \"{name}\" not found in resource manager or following system searching paths:"
                ));
            } else {
                message.push_str(&format!(
                    "possible config files \"{name}\" or \"{name}.{}\" not found in resource manager or following system searching paths:",
                    SUPPORTED_FILE_TYPES.join("/")
                ));
            }
            let mut index = 1;
            for path in &paths {
                let path = path.trim_end_matches(['/', '\\']);
                for folder in LOCAL_SYSTEM_TRY_FOLDERS {
                    message.push_str(&format!("\n{index}. {}", gfile::join(&[path, folder])));
                    index += 1;
                }
            }
        } else {
            message.push_str(&format!(
                "cannot find config file \"{name}\" with no filePath configured"
            ));
        }
        gerror::new_code(&gcode::CODE_NOT_FOUND, &[&message])
    }

    /// Resolves a file name to a path, like `doGetFilePath`.
    fn file_path_internal(&self, name: &str) -> String {
        if !gres::is_empty() {
            for folder in RESOURCE_TRY_FOLDERS {
                let candidate = format!("{folder}{name}");
                if let Some(file) = gres::get(&candidate)
                    && !file.file_info().is_dir()
                {
                    return file.name().to_owned();
                }
            }
            let paths = self
                .search_paths
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            for search_path in paths {
                for folder in RESOURCE_TRY_FOLDERS {
                    let candidate = format!("{search_path}{folder}{name}");
                    if let Some(file) = gres::get(&candidate)
                        && !file.file_info().is_dir()
                    {
                        return file.name().to_owned();
                    }
                }
            }
        }
        let paths = self
            .search_paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for search_path in paths {
            let search_path = search_path.trim_end_matches(['/', '\\']);
            for folder in LOCAL_SYSTEM_TRY_FOLDERS {
                let relative = gfile::join(&[folder, name]);
                let relative = relative.trim_end_matches(['/', '\\']);
                let (found, _) = gspath::search(search_path, relative, &[]);
                if !found.is_empty() && !gfile::is_dir(&found) {
                    return found;
                }
            }
        }
        let real_path = gfile::real_path(name);
        if !real_path.is_empty() && !gfile::is_dir(&real_path) {
            return real_path;
        }
        String::new()
    }

    /// Returns the cached (or freshly parsed) document, like `getJson`.
    fn get_json(&self, file_name: Option<&str>) -> Result<Option<Arc<Json>>, Error> {
        let used = match file_name {
            Some(name) if !name.is_empty() => name.to_owned(),
            _ => self.file_name(),
        };
        if let Some(cached) = self
            .json_map
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&used)
            .cloned()
        {
            return Ok(Some(cached));
        }
        let mut file_path = String::new();
        let content = self.get_content(Some(&used));
        let from_content = !content.is_empty();
        let content = if from_content {
            content
        } else {
            let path = self.file_path_internal(&used);
            if path.is_empty() {
                return Ok(None);
            }
            file_path = path.clone();
            match gres::get(&path) {
                Some(file) => String::from_utf8_lossy(file.content()).into_owned(),
                None => gfile::get_contents(&path),
            }
        };
        let extension = if file_path.is_empty() {
            gfile::ext_name(&used)
        } else {
            gfile::ext_name(&file_path)
        };
        let content_type = gjson::content_type(&extension);
        let parsed = if gjson::is_valid_content_type(content_type) && !from_content {
            gjson::load_content_type(content_type, content.as_bytes(), true)
        } else {
            gjson::load_content(content.as_bytes(), true)
        };
        let json = match parsed {
            Ok(json) => json,
            Err(error) => {
                let error = if file_path.is_empty() {
                    gerror::wrap(Some(error), "load configuration failed".to_owned())
                } else {
                    gerror::wrap(
                        Some(error),
                        format!("load config file \"{file_path}\" failed"),
                    )
                }
                .unwrap_or_else(|| gerror::new("load configuration failed"));
                if !file_path.is_empty() && !gres::contains(&file_path) {
                    self.add_file_watcher(&used, &file_path);
                }
                return Err(error);
            }
        };
        let json = Arc::new(json);
        self.json_map
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(used.clone(), Arc::clone(&json));
        if !file_path.is_empty() && !gres::contains(&file_path) {
            self.add_file_watcher(&used, &file_path);
        }
        Ok(Some(json))
    }

    /// Watches `file_path` and invalidates `file_name` on changes, like
    /// `addFileWatcher`.
    fn add_file_watcher(&self, file_name: &str, file_path: &str) {
        let key = format!("{file_name}\u{0}{file_path}");
        {
            let registrations = self
                .registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if registrations.contains_key(&key) {
                return;
            }
        }
        let json_map = Arc::clone(&self.json_map);
        let watchers = Arc::clone(&self.watchers);
        let cache_key = file_name.to_owned();
        let path = file_path.to_owned();
        let handler: gfsnotify::EventHandler = Arc::new(move |event: &gfsnotify::Event| {
            json_map
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .shift_remove(&cache_key);
            let operation = if event.is_write() {
                OperationType::Write
            } else if event.is_remove() {
                OperationType::Remove
            } else if event.is_create() {
                OperationType::Create
            } else if event.is_rename() {
                OperationType::Rename
            } else if event.is_chmod() {
                OperationType::Chmod
            } else {
                return;
            };
            let context = OperationContext {
                file_name: cache_key.clone(),
                file_path: path.clone(),
                file_type: gfile::ext_name(&cache_key),
                operation,
                ..OperationContext::default()
            };
            watchers.notify(&context);
        });
        match gfsnotify::add_once(&key, file_path, handler, gfsnotify::WatchOption::default()) {
            Ok(Some(callback)) => {
                self.registrations
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .insert(
                        key,
                        FileWatcher {
                            callback_id: callback.id,
                        },
                    );
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("gf-gcfg: failed to watch \"{file_path}\": {error}");
            }
        }
    }

    /// Removes every file watch this adapter owns.
    pub fn stop_watching(&self) {
        let registrations: HashMap<String, FileWatcher> = {
            let mut registrations = self
                .registrations
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            std::mem::take(&mut *registrations)
        };
        for registration in registrations.values() {
            let _ = gfsnotify::remove_callback(registration.callback_id);
        }
    }
}

impl Adapter for AdapterFile {
    fn available(&self, resource: Option<&str>) -> bool {
        let name = resource
            .map(str::to_owned)
            .unwrap_or_else(|| self.file_name());
        if !self.get_content(Some(&name)).is_empty() {
            return true;
        }
        !self.file_path_internal(&name).is_empty()
    }

    fn get(&self, pattern: &str) -> Result<Option<Value>, Error> {
        let Some(json) = self.get_json(None)? else {
            return Ok(None);
        };
        let value = json.get(pattern);
        if value.is_nil() {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    fn data(&self) -> Result<IndexMap<String, Value>, Error> {
        Ok(self
            .get_json(None)?
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gcfg-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    /// Waits until `condition` holds, for at most five seconds.
    fn eventually(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// An adapter over a temporary directory holding `content` as `name`.
    fn file_adapter(directory: &str, name: &str, content: &str) -> Arc<AdapterFile> {
        put_contents(&gfile::join(&[directory, name]), content).expect("write config");
        let adapter = AdapterFile::new(Some(name)).expect("adapter");
        adapter.set_path(directory).expect("set path");
        adapter
    }

    // Translated from TestAdapterFile_Available and the basic reads.
    #[test]
    fn adapter_file_reads_values() {
        let base = sandbox("read");
        let adapter = file_adapter(&base, "config.toml", "name = \"goframe\"\nport = 8080\n");
        assert!(adapter.available(None));
        assert_eq!(adapter.file_name(), "config.toml");
        assert_eq!(
            adapter.get("name").expect("get"),
            Some(Value::String("goframe".to_owned()))
        );
        assert_eq!(adapter.get("port").expect("get"), Some(Value::Int(8080)));
        assert_eq!(adapter.get("missing").expect("get"), None);
        assert_eq!(adapter.data().expect("data").len(), 2);
        assert!(
            adapter
                .get_file_path(None)
                .expect("path")
                .ends_with("config.toml")
        );
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestAdapterFile_SetPath and TestAdapterFile_AddPath.
    #[test]
    fn adapter_file_paths_validate() {
        let base = sandbox("paths");
        let adapter = AdapterFile::new(Some("config.yml")).expect("adapter");
        adapter.set_path(&base).expect("set path");
        assert_eq!(adapter.get_paths(), vec![base.clone()]);
        assert!(adapter.set_path("not-exist-directory").is_err());

        let file = gfile::join(&[&base, "not-a-directory.yml"]);
        put_contents(&file, "x").expect("write file");
        assert!(adapter.set_path(&file).is_err());
        assert!(adapter.add_path(&["not-exist-directory"]).is_err());

        adapter.add_path(&[&base]).expect("add path");
        assert_eq!(adapter.get_paths().len(), 1);
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestAdapterFile_FilePath.
    #[test]
    fn adapter_file_reports_missing_path_with_not_found() {
        let base = sandbox("filepath");
        let adapter = AdapterFile::new(Some("config.yml")).expect("adapter");
        adapter.set_path(&base).expect("set path");
        let error = adapter
            .get_file_path(Some("tmp"))
            .expect_err("missing file");
        assert!(gf_errors::gerror::code(&error) == gcode::CODE_NOT_FOUND);
        assert!(error.to_string().contains("possible config files"));
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestAdapterFile_Content.
    #[test]
    fn adapter_file_custom_content() {
        let adapter = AdapterFile::new(None).expect("adapter");
        adapter.set_content("gf", Some("config.yml"));
        assert_eq!(adapter.get_content(Some("config.yml")), "gf");
        adapter.set_content("gf1", Some("config.yml"));
        assert_eq!(adapter.get_content(Some("config.yml")), "gf1");
        adapter.remove_content(Some("config.yml"));
        assert_eq!(adapter.get_content(Some("config.yml")), "");
        adapter.clear_content();
        assert_eq!(adapter.get_content(Some("name")), "");
    }

    // Translated from TestAdapterFile_Set.
    #[test]
    fn adapter_file_sets_values_in_the_cache() {
        let base = sandbox("set");
        let adapter = file_adapter(&base, "config.toml", "log-path = \"logs\"\n");
        assert_eq!(
            adapter.get("log-path").expect("get"),
            Some(Value::String("logs".to_owned()))
        );
        adapter
            .set("log-path", Value::String("custom-logs".to_owned()))
            .expect("set");
        assert_eq!(
            adapter.get("log-path").expect("get"),
            Some(Value::String("custom-logs".to_owned()))
        );
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestAdapterFile_With_UTF8_BOM.
    #[test]
    fn adapter_file_reads_utf8_bom() {
        let base = sandbox("bom");
        let mut content = vec![0xEF, 0xBB, 0xBF];
        content.extend_from_slice(b"test = { testInt = 1, testStr = \"test\" }\n");
        std::fs::write(gfile::join(&[&base, "cfg-with-utf8-bom.toml"]), content).expect("write");
        let adapter = AdapterFile::new(Some("cfg-with-utf8-bom.toml")).expect("adapter");
        adapter.set_path(&base).expect("set path");
        assert_eq!(
            adapter.get("test.testInt").expect("get"),
            Some(Value::Int(1))
        );
        assert_eq!(
            adapter.get("test.testStr").expect("get"),
            Some(Value::String("test".to_owned()))
        );
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestWatcher_File_Ctx: a file change reloads the value and
    // notifies the logical watcher.
    #[test]
    fn adapter_file_watcher_reloads_on_change() {
        let base = sandbox("watch");
        let adapter = file_adapter(&base, "config.toml", "name = \"first\"\n");
        assert_eq!(
            adapter.get("name").expect("get"),
            Some(Value::String("first".to_owned()))
        );
        let (tx, rx) = std::sync::mpsc::channel();
        adapter.add_watcher(
            "change",
            Arc::new(move |context: &OperationContext| {
                let _ = tx.send(context.operation);
            }),
        );

        put_contents(&gfile::join(&[&base, "config.toml"]), "name = \"second\"\n")
            .expect("rewrite");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).expect("event"),
            OperationType::Write
        );
        assert!(eventually(|| {
            adapter.get("name").expect("get") == Some(Value::String("second".to_owned()))
        }));
        adapter.stop_watching();
        remove_all(&base).expect("cleanup");
    }

    // Translated from TestWatcher_SetContentNotify / RemoveContentNotify.
    #[test]
    fn adapter_file_content_changes_notify() {
        let adapter = AdapterFile::new(None).expect("adapter");
        let (tx, rx) = std::sync::mpsc::channel();
        adapter.add_watcher(
            "content",
            Arc::new(move |context: &OperationContext| {
                let _ = tx.send(context.operation);
            }),
        );
        adapter.set_content(r#"{"a":1}"#, Some("config.json"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("set"),
            OperationType::Set
        );
        adapter.remove_content(Some("config.json"));
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("remove"),
            OperationType::Remove
        );
        adapter.clear_content();
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("clear"),
            OperationType::Clear
        );
    }
}
