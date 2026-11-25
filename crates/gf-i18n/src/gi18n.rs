//! Port of `i18n/gi18n` — internationalization and localization.
//!
//! The Go manager loads translation files from a directory (or embedded
//! resource), keys them by language and translates `{#key}` placeholders. This
//! port keeps the file lane and the translation surface, without the context
//! and resource lanes (ADR-078):
//!
//! - [`Options`]/[`Manager`] with lazy loading, `set_path`, `set_language`,
//!   `set_delimiters`, `translate`, `translate_with_language`,
//!   `translate_format`, `get_content`;
//! - language = first directory under the root, or the file stem for flat
//!   files, like Go;
//! - nested maps are flattened into dotted keys (`resourceUsage.workflow`);
//! - the named instance registry and the package-level default-manager
//!   functions.
//!
//! Not ported: `gres` resource paths, `context.Context` language values
//! (explicit `*_with_language` methods are provided instead), hot reload on
//! file changes and the Go `fmt` verb surface beyond `%s`/`%v`/`%d`/`%%`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_util::gconv;

/// The default instance name.
pub const DEFAULT_NAME: &str = "default";

/// The default language when none is configured.
pub const DEFAULT_LANGUAGE: &str = "en";

/// The default delimiters around translation keys.
pub const DEFAULT_DELIMITERS: (&str, &str) = ("{#", "}");

/// The default folders searched for i18n files.
pub const SEARCH_FOLDERS: [&str; 3] = ["manifest/i18n", "manifest/config/i18n", "i18n"];

/// One file from an embedded resource, with its path relative to the i18n
/// root.
#[derive(Clone, Debug)]
pub struct ResourceFile {
    /// The relative path (for example `zh-CN.json`).
    pub name: String,
    /// The file bytes.
    pub content: Vec<u8>,
}

/// A packed-resource source for i18n files, the port's interface to
/// `os/gres` without a crate cycle (ADR-101).
pub trait Resource: std::fmt::Debug + Send + Sync {
    /// Reports whether the resource contains `path`.
    fn contains(&self, path: &str) -> bool;

    /// Returns every file under `path` with names relative to it.
    fn files(&self, path: &str) -> Vec<ResourceFile>;
}

/// Options for an i18n manager, like GoFrame's `Options`.
#[derive(Clone, Debug)]
pub struct Options {
    /// The directory holding translation files (empty to auto-discover).
    pub path: String,
    /// The active language.
    pub language: String,
    /// The left/right delimiters.
    pub delimiters: (String, String),
    /// An optional packed resource used instead of the filesystem.
    pub resource: Option<Arc<dyn Resource>>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            path: String::new(),
            language: DEFAULT_LANGUAGE.to_owned(),
            delimiters: (
                DEFAULT_DELIMITERS.0.to_owned(),
                DEFAULT_DELIMITERS.1.to_owned(),
            ),
            resource: None,
        }
    }
}

impl Options {
    /// Creates options for `path`.
    pub fn new(path: &str) -> Options {
        Options {
            path: path.to_owned(),
            ..Options::default()
        }
    }

    /// Sets the language.
    pub fn with_language(mut self, language: &str) -> Options {
        self.language = language.to_owned();
        self
    }

    /// Sets the packed resource used for file lookup.
    pub fn with_resource(mut self, resource: Arc<dyn Resource>) -> Options {
        self.resource = Some(resource);
        self
    }
}

/// The managed state, replaced on path changes.
struct State {
    options: Options,
    /// `None` until loaded.
    data: Option<HashMap<String, HashMap<String, String>>>,
}

/// The i18n manager, like GoFrame's `Manager`.
pub struct Manager {
    state: Mutex<State>,
}

impl Default for Manager {
    fn default() -> Manager {
        Manager::new(Options::default())
    }
}

impl Manager {
    /// Creates a manager; an empty path auto-discovers [`SEARCH_FOLDERS`],
    /// first in the packed resource and then on disk.
    pub fn new(mut options: Options) -> Manager {
        if options.path.is_empty() {
            for folder in SEARCH_FOLDERS {
                let in_resource = options
                    .resource
                    .as_ref()
                    .map(|resource| resource.contains(folder))
                    .unwrap_or(false);
                if in_resource || Path::new(folder).is_dir() {
                    options.path = folder.to_owned();
                    break;
                }
            }
        }
        Manager {
            state: Mutex::new(State {
                options,
                data: None,
            }),
        }
    }

    /// Sets the translation file directory, like `SetPath`; the path may be a
    /// directory on disk or a packed-resource path.
    pub fn set_path(&self, path: &str) -> Result<(), Error> {
        let in_resource = self
            .lock()
            .options
            .resource
            .as_ref()
            .map(|resource| resource.contains(path))
            .unwrap_or(false);
        if path.is_empty() || (!in_resource && !Path::new(path).is_dir()) {
            let message = format!("{path} does not exist");
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[message.as_str()],
            ));
        }
        let mut state = self.lock();
        state.options.path = path.to_owned();
        state.data = None;
        Ok(())
    }

    /// Sets the active language, like `SetLanguage`.
    pub fn set_language(&self, language: &str) {
        self.lock().options.language = language.to_owned();
    }

    /// The active language.
    pub fn language(&self) -> String {
        self.lock().options.language.clone()
    }

    /// Sets the delimiters, like `SetDelimiters`.
    pub fn set_delimiters(&self, left: &str, right: &str) {
        let mut state = self.lock();
        state.options.delimiters = (left.to_owned(), right.to_owned());
    }

    /// The delimiter pair.
    pub fn delimiters(&self) -> (String, String) {
        self.lock().options.delimiters.clone()
    }

    /// Translates `content` with the active language, like `Translate`.
    pub fn translate(&self, content: &str) -> String {
        let language = self.language();
        self.translate_with_language(content, &language)
    }

    /// Translates `content` with an explicit language (the port of the
    /// context language override).
    pub fn translate_with_language(&self, content: &str, language: &str) -> String {
        self.ensure_loaded();
        let state = self.lock();
        let Some(data) = state.data.as_ref().and_then(|data| data.get(language)) else {
            return content.to_owned();
        };
        if let Some(value) = data.get(content) {
            return value.clone();
        }
        let (left, right) = &state.options.delimiters;
        replace_placeholders(content, left, right, |key| data.get(key).cloned())
    }

    /// Translates and formats `format` with `values`, like
    /// `TranslateFormat` (`%s`/`%v`/`%d`/`%%`).
    pub fn translate_format(&self, format: &str, values: &[Value]) -> String {
        let translated = self.translate(format);
        format_values(&translated, values)
    }

    /// Returns the translation of `key`, like `GetContent`.
    pub fn get_content(&self, key: &str) -> String {
        let language = self.language();
        self.get_content_with_language(key, &language)
    }

    /// Returns the translation of `key` for an explicit language.
    pub fn get_content_with_language(&self, key: &str, language: &str) -> String {
        self.ensure_loaded();
        let state = self.lock();
        state
            .data
            .as_ref()
            .and_then(|data| data.get(language))
            .and_then(|language_data| language_data.get(key))
            .cloned()
            .unwrap_or_default()
    }

    /// Returns a copy of the loaded data (empty before loading).
    pub fn content(&self) -> HashMap<String, HashMap<String, String>> {
        self.ensure_loaded();
        self.lock().data.clone().unwrap_or_default()
    }

    /// Loads the files once.
    fn ensure_loaded(&self) {
        {
            let state = self.lock();
            if state.data.is_some() {
                return;
            }
        }
        let mut state = self.lock();
        if state.data.is_some() {
            return;
        }
        state.data = Some(load_data(&state.options));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Loads all translation files under the options path (packed resource first,
/// then the filesystem).
fn load_data(options: &Options) -> HashMap<String, HashMap<String, String>> {
    let mut data = HashMap::new();
    let path = options.path.as_str();
    if path.is_empty() {
        return data;
    }
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    if let Some(resource) = &options.resource
        && resource.contains(path)
    {
        for file in resource.files(path) {
            files.push((file.name, file.content));
        }
    } else {
        let root = Path::new(path);
        let mut paths = Vec::new();
        collect_files(root, &mut paths);
        paths.sort();
        for file in paths {
            let relative = file.strip_prefix(root).unwrap_or(&file);
            let Ok(bytes) = std::fs::read(&file) else {
                continue;
            };
            files.push((relative.to_string_lossy().into_owned(), bytes));
        }
    }
    files.sort_by(|left, right| left.0.cmp(&right.0));
    for (relative, bytes) in files {
        let language = language_of(Path::new(&relative));
        let Ok(json) = gf_encoding::gjson::Json::new_from(&bytes) else {
            continue;
        };
        let entry = data.entry(language).or_insert_with(HashMap::new);
        for (key, value) in json.map() {
            flatten_value(&key, &value, entry);
        }
    }
    data
}

/// Recursively collects regular files under `directory`.
fn collect_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, files);
        } else if path.is_file() {
            files.push(path);
        }
    }
}

/// Derives the language from the path relative to the i18n root.
fn language_of(relative: &Path) -> String {
    let segments: Vec<String> = relative
        .components()
        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned))
        .collect();
    match segments.len() {
        0 => String::new(),
        1 => relative
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_owned(),
        _ => segments[0].clone(),
    }
}

/// Flattens nested maps into dotted keys, converting leaves to strings.
fn flatten_value(prefix: &str, value: &Value, out: &mut HashMap<String, String>) {
    match value {
        Value::Map(map) => {
            for (key, value) in map {
                let nested = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                flatten_value(&nested, value, out);
            }
        }
        other => {
            out.insert(prefix.to_owned(), gconv::string(other));
        }
    }
}

/// Replaces `left key right` placeholders using `lookup`.
fn replace_placeholders(
    content: &str,
    left: &str,
    right: &str,
    lookup: impl Fn(&str) -> Option<String>,
) -> String {
    if left.is_empty() || right.is_empty() {
        return content.to_owned();
    }
    let mut result = String::with_capacity(content.len());
    let mut cursor = 0;
    while let Some(start) = content[cursor..].find(left) {
        let start = cursor + start;
        let key_start = start + left.len();
        let Some(end) = content[key_start..].find(right) else {
            break;
        };
        let end = key_start + end;
        result.push_str(&content[cursor..start]);
        let key = &content[key_start..end];
        match lookup(key) {
            Some(value) => result.push_str(&value),
            None => result.push_str(&content[start..end + right.len()]),
        }
        cursor = end + right.len();
    }
    result.push_str(&content[cursor..]);
    result
}

/// Formats translated text with Go-like `%s`/`%v`/`%d`/`%%` verbs.
fn format_values(format: &str, values: &[Value]) -> String {
    let mut result = String::with_capacity(format.len());
    let mut chars = format.chars().peekable();
    let mut index = 0;
    while let Some(character) = chars.next() {
        if character != '%' {
            result.push(character);
            continue;
        }
        let Some(verb) = chars.next() else {
            result.push('%');
            break;
        };
        match verb {
            '%' => result.push('%'),
            's' | 'v' => {
                if let Some(value) = values.get(index) {
                    result.push_str(&gconv::string(value));
                    index += 1;
                } else {
                    result.push_str("%s");
                }
            }
            'd' => {
                if let Some(value) = values.get(index) {
                    result.push_str(&gconv::int64_value(value).to_string());
                    index += 1;
                } else {
                    result.push_str("%d");
                }
            }
            other => {
                result.push('%');
                result.push(other);
            }
        }
    }
    result
}

/// The named instance registry.
fn instances() -> &'static Mutex<HashMap<String, Arc<Manager>>> {
    static INSTANCES: OnceLock<Mutex<HashMap<String, Arc<Manager>>>> = OnceLock::new();
    INSTANCES.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the named manager, creating it on first use, like `Instance`.
pub fn instance(name: Option<&str>) -> Arc<Manager> {
    let key = name.filter(|name| !name.is_empty()).unwrap_or(DEFAULT_NAME);
    instances()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .entry(key.to_owned())
        .or_insert_with(|| Arc::new(Manager::new(Options::default())))
        .clone()
}

/// Registers a manager under `name`, like setting `gins`' i18n instance.
pub fn set_instance(name: &str, manager: Arc<Manager>) {
    let key = if name.is_empty() { DEFAULT_NAME } else { name };
    instances()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(key.to_owned(), manager);
}

/// The default manager, like package-level `Instance()`.
pub fn default_manager() -> Arc<Manager> {
    instance(None)
}

/// Sets the default manager's path, like package-level `SetPath`.
pub fn set_path(path: &str) -> Result<(), Error> {
    default_manager().set_path(path)
}

/// Sets the default manager's language, like package-level `SetLanguage`.
pub fn set_language(language: &str) {
    default_manager().set_language(language);
}

/// Sets the default manager's delimiters.
pub fn set_delimiters(left: &str, right: &str) {
    default_manager().set_delimiters(left, right);
}

/// Translates with the default manager, like package-level `T`.
pub fn t(content: &str) -> String {
    default_manager().translate(content)
}

/// Translates and formats with the default manager, like package-level `Tf`.
pub fn tf(format: &str, values: &[Value]) -> String {
    default_manager().translate_format(format, values)
}

/// Returns a translation from the default manager, like `GetContent`.
pub fn get_content(key: &str) -> String {
    default_manager().get_content(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("gf-i18n-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("create dir");
        directory
    }

    fn write(directory: &Path, name: &str, content: &str) {
        let path = directory.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write file");
    }

    fn setup(directory: &Path) {
        write(
            directory,
            "en.json",
            r#"{"hello":"Hello","world":"World","你好":"hello","世界":"world"}"#,
        );
        write(
            directory,
            "ja.json",
            r#"{"hello":"こんにちは","world":"世界"}"#,
        );
        write(
            directory,
            "zh-CN.json",
            r#"{"hello":"你好","world":"世界","你好":"hello","世界":"world"}"#,
        );
    }

    #[test]
    fn basic_translation() {
        let directory = temp_dir("basic");
        setup(&directory);
        let manager = Manager::new(Options::new(&directory.to_string_lossy()));
        manager.set_language("none");
        assert_eq!(manager.translate("{#hello}{#world}"), "{#hello}{#world}");
        manager.set_language("ja");
        assert_eq!(manager.translate("{#hello}{#world}"), "こんにちは世界");
        manager.set_language("zh-CN");
        assert_eq!(manager.translate("{#hello}{#world}"), "你好世界");
        assert_eq!(manager.translate("{#你好} {#世界}"), "hello world");
        assert_eq!(manager.translate("{#missing}"), "{#missing}");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn delimiters() {
        let directory = temp_dir("delimiters");
        setup(&directory);
        let manager = Manager::new(Options::new(&directory.to_string_lossy()));
        manager.set_language("zh-CN");
        manager.set_delimiters("{$", "}");
        assert_eq!(manager.translate("{#hello}{#world}"), "{#hello}{#world}");
        assert_eq!(manager.translate("{$hello}{$world}"), "你好世界");
        assert_eq!(manager.delimiters(), ("{$".to_owned(), "}".to_owned()));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn translate_format() {
        let directory = temp_dir("format");
        setup(&directory);
        let manager = Manager::new(Options::new(&directory.to_string_lossy()));
        manager.set_language("ja");
        assert_eq!(
            manager.translate_format("{#hello}{#world} %d", &[Value::Int(2020)]),
            "こんにちは世界 2020"
        );
        assert_eq!(
            manager.translate_format("%s-%v-%%", &[Value::Int(1), Value::Int(2)]),
            "1-2-%"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn explicit_language_and_content() {
        let directory = temp_dir("content");
        setup(&directory);
        let manager = Manager::new(Options::new(&directory.to_string_lossy()));
        assert_eq!(manager.get_content("hello"), "Hello");
        assert_eq!(manager.get_content_with_language("hello", "zh-CN"), "你好");
        assert_eq!(manager.get_content_with_language("hello", "unknown"), "");
        assert_eq!(
            manager.translate_with_language("{#hello}{#world}", "ja"),
            "こんにちは世界"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn nested_language_directories() {
        let directory = temp_dir("nested");
        write(
            &directory,
            "zh/data.yaml",
            "resourceUsage:\n  workflow: workflow\n",
        );
        write(&directory, "ja/manifest.toml", "hello = \"こんにちは\"\n");
        let manager = Manager::new(Options::new(&directory.to_string_lossy()));
        manager.set_language("zh");
        assert_eq!(manager.translate("{#resourceUsage.workflow}"), "workflow");
        assert_eq!(manager.translate("{#hello}"), "{#hello}");
        manager.set_language("ja");
        assert_eq!(manager.translate("{#hello}"), "こんにちは");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn set_path_errors_and_reload() {
        let manager = Manager::new(Options::default());
        assert!(manager.set_path("i18n-does-not-exist").is_err());
        assert!(manager.set_path("").is_err());
        let directory = temp_dir("reload");
        setup(&directory);
        manager
            .set_path(&directory.to_string_lossy())
            .expect("set path");
        manager.set_language("en");
        assert_eq!(manager.translate("{#hello}"), "Hello");
        // A new path reloads content.
        let other = temp_dir("reload-other");
        write(&other, "en.toml", "hello = \"Hi\"\n");
        manager.set_path(&other.to_string_lossy()).expect("path");
        assert_eq!(manager.translate("{#hello}"), "Hi");
        let _ = std::fs::remove_dir_all(&directory);
        let _ = std::fs::remove_dir_all(&other);
    }

    #[test]
    fn named_instances() {
        let directory = temp_dir("instance");
        setup(&directory);
        let manager = Arc::new(Manager::new(Options::new(&directory.to_string_lossy())));
        manager.set_language("zh-CN");
        set_instance("gf-i18n-test", Arc::clone(&manager));
        let same = instance(Some("gf-i18n-test"));
        assert_eq!(same.translate("{#hello}{#world}"), "你好世界");
        assert!(Arc::ptr_eq(&manager, &same));
        let default = default_manager();
        assert_eq!(default.language(), DEFAULT_LANGUAGE);
        let _ = std::fs::remove_dir_all(&directory);
    }

    /// A packed-resource stand-in for `gres`.
    #[derive(Debug, Default)]
    struct FakeResource {
        files: HashMap<String, Vec<u8>>,
    }

    impl Resource for FakeResource {
        fn contains(&self, path: &str) -> bool {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            self.files.keys().any(|name| name.starts_with(&prefix))
        }

        fn files(&self, path: &str) -> Vec<ResourceFile> {
            let prefix = format!("{}/", path.trim_end_matches('/'));
            let mut files: Vec<ResourceFile> = self
                .files
                .iter()
                .filter_map(|(name, content)| {
                    name.strip_prefix(&prefix).map(|relative| ResourceFile {
                        name: relative.to_owned(),
                        content: content.clone(),
                    })
                })
                .collect();
            files.sort_by(|left, right| left.name.cmp(&right.name));
            files
        }
    }

    #[test]
    fn resource_lane_loads_packed_files() {
        let mut files = HashMap::new();
        files.insert(
            "i18n/zh-CN.json".to_owned(),
            r#"{"hello":"你好"}"#.as_bytes().to_vec(),
        );
        files.insert(
            "i18n/en.json".to_owned(),
            r#"{"hello":"Hello"}"#.as_bytes().to_vec(),
        );
        files.insert(
            "i18n/nested/ja.json".to_owned(),
            r#"{"hello":"こんにちは"}"#.as_bytes().to_vec(),
        );
        let resource: Arc<dyn Resource> = Arc::new(FakeResource { files });
        let options = Options::new("i18n").with_resource(Arc::clone(&resource));
        let manager = Manager::new(options);
        assert_eq!(manager.translate_with_language("{#hello}", "zh-CN"), "你好");
        assert_eq!(manager.translate_with_language("{#hello}", "en"), "Hello");
        assert_eq!(
            manager.translate_with_language("{#hello}", "nested/ja"),
            "{#hello}"
        );
        // `set_path` accepts a resource path as well.
        assert!(manager.set_path("i18n").is_ok());
        assert!(manager.set_path("missing").is_err());
    }
}
