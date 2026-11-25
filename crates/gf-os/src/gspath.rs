//! Port of `os/gspath`.
//!
//! Go's package keeps a list of search roots and, when caching is enabled, a
//! file index kept current by `gfsnotify`. The port implements both lanes
//! (ADR-091): the direct search lane and the indexed cache lane, which scans
//! the roots recursively and refreshes entries from `gfsnotify` events.
//!
//! Search results must stay under the root they were found in, which the port
//! checks with the same prefix comparison Go uses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_text::gstr;

use crate::{gfile, gfsnotify};

/// One cached index entry.
struct CacheEntry {
    path: String,
    is_dir: bool,
}

/// The indexed cache shared with the watcher callbacks.
type Cache = Arc<Mutex<HashMap<String, CacheEntry>>>;

/// Manages a list of directories searched in insertion order, like Go's
/// `gspath.SPath`.
pub struct SPath {
    paths: RwLock<Vec<String>>,
    cache: Option<Cache>,
    monitors: Mutex<Vec<Arc<gfsnotify::Callback>>>,
}

impl SPath {
    /// Creates a manager for `path`, like Go's `gspath.New`.
    pub fn new(path: &str) -> SPath {
        SPath::new_with_cache(path, false)
    }

    /// Creates a manager, enabling the indexed cache lane when `cache` is
    /// true, like Go's `gspath.New(path, cache)`.
    pub fn new_with_cache(path: &str, cache: bool) -> SPath {
        let manager = SPath {
            paths: RwLock::new(Vec::new()),
            cache: if cache {
                Some(Arc::new(Mutex::new(HashMap::new())))
            } else {
                None
            },
            monitors: Mutex::new(Vec::new()),
        };
        if !path.is_empty() {
            let _ = manager.add(path);
        }
        manager
    }

    /// Adds every file under `root` to the cache and starts monitoring it.
    fn refresh_cache(&self, root: &str) {
        let Some(cache) = &self.cache else {
            return;
        };
        {
            let mut entries = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            entries.clear();
            add_to_cache(&mut entries, root, root);
        }
        let watched = Arc::clone(cache);
        let root_path = root.to_owned();
        let handler: gfsnotify::EventHandler = Arc::new(move |event: &gfsnotify::Event| {
            let mut entries = watched
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if event.is_create() {
                add_to_cache(&mut entries, &event.path, &root_path);
            } else if event.is_remove() || (event.is_rename() && !gfile::exists(&event.path)) {
                entries.remove(&cache_name(&event.path, &root_path));
            }
        });
        if let Ok(Some(callback)) = gfsnotify::add(root, handler, gfsnotify::WatchOption::default())
        {
            self.monitors
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(callback);
        }
    }

    /// Removes `path` from the cache, like Go's `Remove`.
    pub fn remove(&self, path: &str) {
        let Some(cache) = &self.cache else {
            return;
        };
        let mut entries = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let roots = self
            .paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for root in roots {
            if path.starts_with(root.as_str()) {
                entries.remove(&cache_name(path, &root));
                return;
            }
        }
        entries.remove(&format_cache_name(path));
    }

    /// Replaces the roots with `path`, like Go's `Set`.
    pub fn set(&self, path: &str) -> Result<String, Error> {
        let real_path = self.resolve(path);
        if real_path.is_empty() {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("path \"{path}\" does not exist"),
                Vec::new(),
            ));
        }
        if !gfile::is_dir(&real_path) {
            let message = format!("{path} should be a folder");
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[&message],
            ));
        }
        let trimmed = gstr::trim_right(&real_path, Some(&gfile::separator()));
        let mut paths = self
            .paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        paths.clear();
        paths.push(trimmed.clone());
        drop(paths);
        self.refresh_cache(&trimmed);
        Ok(trimmed)
    }

    /// Adds a search root, like Go's `Add`.
    ///
    /// Adding the same directory twice is a no-op.
    pub fn add(&self, path: &str) -> Result<String, Error> {
        let real_path = self.resolve(path);
        if real_path.is_empty() {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("path \"{path}\" does not exist"),
                Vec::new(),
            ));
        }
        if !gfile::is_dir(&real_path) {
            let message = format!("{path} should be a folder");
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[&message],
            ));
        }
        let trimmed = gstr::trim_right(&real_path, Some(&gfile::separator()));
        let mut paths = self
            .paths
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !paths.contains(&trimmed) {
            paths.push(trimmed.clone());
        }
        drop(paths);
        self.refresh_cache(&trimmed);
        Ok(trimmed)
    }

    /// Searches `name` under every root in order, like Go's `SPath.Search`.
    ///
    /// When the result is a directory, `index_files` are tried inside it and a
    /// found index file is returned instead.
    pub fn search(&self, name: &str, index_files: &[&str]) -> (String, bool) {
        if let Some(cache) = &self.cache {
            let key = format_cache_name(name);
            let entries = cache
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(entry) = entries.get(&key) else {
                return (String::new(), false);
            };
            let mut file_path = entry.path.clone();
            let mut is_dir = entry.is_dir;
            if !index_files.is_empty() && is_dir {
                let base = if key == "/" { "" } else { key.as_str() };
                for file in index_files {
                    if let Some(found) = entries.get(&format!("{base}/{file}")) {
                        return (found.path.clone(), found.is_dir);
                    }
                }
            }
            file_path = gfile::abs(&file_path);
            is_dir = is_dir && gfile::is_dir(&file_path);
            return (file_path, is_dir);
        }
        let mut file_path = String::new();
        let mut is_dir = false;
        {
            let paths = self
                .paths
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for root in paths.iter() {
                let candidate = gfile::join(&[root, name]);
                let Ok(metadata) = std::fs::metadata(&candidate) else {
                    continue;
                };
                let absolute = gfile::abs(&candidate);
                // Security check: the result must stay under this root.
                if absolute.len() >= root.len() && absolute.starts_with(root.as_str()) {
                    file_path = absolute;
                    is_dir = metadata.is_dir();
                    break;
                }
            }
        }
        if !index_files.is_empty() && is_dir {
            for file in index_files {
                let candidate = gfile::join(&[&file_path, file]);
                if gfile::exists(&candidate) {
                    file_path = candidate;
                    is_dir = false;
                    break;
                }
            }
        }
        (file_path, is_dir)
    }

    /// Returns the search roots, like Go's `Paths`.
    pub fn paths(&self) -> Vec<String> {
        self.paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Returns the cached paths, like Go's `AllPaths`.
    ///
    /// The cache keys are sorted; empty when the cache lane is disabled.
    pub fn all_paths(&self) -> Vec<String> {
        let Some(cache) = &self.cache else {
            return Vec::new();
        };
        let entries = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut keys: Vec<String> = entries.keys().cloned().collect();
        keys.sort();
        keys
    }

    /// Returns the number of search roots, like Go's `Size`.
    pub fn size(&self) -> usize {
        self.paths
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Resolves `path` the way Go's `Set`/`Add` do: absolute if it exists,
    /// else relative to the current roots, else relative to the working
    /// directory.
    fn resolve(&self, path: &str) -> String {
        let mut real_path = gfile::real_path(path);
        if real_path.is_empty() {
            real_path = self.search(path, &[]).0;
            if real_path.is_empty() {
                real_path = gfile::real_path(&gfile::join(&[&gfile::pwd(), path]));
            }
        }
        real_path
    }
}

/// Formats `name` as a cache key, like Go's `formatCacheName`.
fn format_cache_name(name: &str) -> String {
    let normalized = name.replace('\\', "/");
    format!("/{}", gstr::trim(&normalized, Some("./")))
}

/// Computes the cache key of `file_path` under `root_path`.
fn cache_name(file_path: &str, root_path: &str) -> String {
    format_cache_name(&file_path.replace(root_path, ""))
}

/// Adds `file_path` and its descendants to `entries`.
fn add_to_cache(entries: &mut HashMap<String, CacheEntry>, file_path: &str, root_path: &str) {
    let is_dir = gfile::is_dir(file_path);
    entries.insert(
        cache_name(file_path, root_path),
        CacheEntry {
            path: file_path.to_owned(),
            is_dir,
        },
    );
    if !is_dir {
        return;
    }
    let mut files = Vec::new();
    scan_recursive(std::path::Path::new(file_path), &mut files);
    for path in files {
        let entry_is_dir = gfile::is_dir(&path);
        entries.insert(
            cache_name(&path, root_path),
            CacheEntry {
                path,
                is_dir: entry_is_dir,
            },
        );
    }
}

/// Recursively collects files and directories under `directory`.
fn scan_recursive(directory: &std::path::Path, files: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(text) = path.to_str() {
            files.push(text.to_owned());
        }
        if path.is_dir() {
            scan_recursive(&path, files);
        }
    }
}

/// The process-wide managers, one per root, like Go's `pathsMap`.
fn managers() -> &'static Mutex<HashMap<String, Arc<SPath>>> {
    static MANAGERS: OnceLock<Mutex<HashMap<String, Arc<SPath>>>> = OnceLock::new();
    MANAGERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the shared manager for `root`, like Go's `gspath.Get`.
pub fn get(root: &str) -> Arc<SPath> {
    let root = if root.is_empty() { "/" } else { root };
    let mut managers = managers()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(manager) = managers.get(root) {
        return Arc::clone(manager);
    }
    let manager = Arc::new(SPath::new(root));
    managers.insert(root.to_owned(), Arc::clone(&manager));
    manager
}

/// Searches `name` under `root`, like Go's package-level `Search`.
pub fn search(root: &str, name: &str, index_files: &[&str]) -> (String, bool) {
    get(root).search(name, index_files)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gspath-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    // Translated from TestSPath_Api: a directory hit and an index-file hit.
    #[test]
    fn search_finds_directories_and_index_files() {
        let root = sandbox("api");
        let nested = gfile::join(&[&root, "gf_tmp"]);
        put_contents(&gfile::join(&[&nested, "gf.txt"]), "").expect("file");

        let (path, is_dir) = search(&root, "gf_tmp", &[]);
        assert_eq!(path, nested);
        assert!(is_dir);

        let (path, is_dir) = search(&root, "gf_tmp", &["gf.txt"]);
        assert_eq!(path, gfile::join(&[&nested, "gf.txt"]));
        assert!(!is_dir);

        remove_all(&root).expect("cleanup");
    }

    // Translated from TestSPath_Basic.
    #[test]
    fn add_set_and_paths() {
        let root = sandbox("basic");
        let nested = gfile::join(&[&root, "gf_tmp"]);
        put_contents(&gfile::join(&[&nested, "gf.txt"]), "").expect("file");

        let manager = SPath::new("");
        assert_eq!(manager.size(), 0);
        assert_eq!(manager.add(&nested).expect("add"), nested);
        assert_eq!(manager.add(&nested).expect("add twice"), nested);
        assert_eq!(manager.size(), 1);
        assert!(manager.add(&gfile::join(&[&nested, "gf.txt"])).is_err());
        assert!(manager.add("does-not-exist").is_err());

        assert_eq!(manager.paths(), vec![nested.clone()]);
        assert!(manager.all_paths().is_empty());

        assert!(manager.set(&gfile::join(&[&nested, "gf.txt"])).is_err());
        assert!(manager.set("does-not-exist").is_err());
        assert_eq!(manager.set(&root).expect("set"), root);
        assert_eq!(manager.size(), 1);
        // `set` replaces every root, so adding another one grows the list.
        assert_eq!(manager.add(&nested).expect("add after set"), nested);
        assert_eq!(manager.size(), 2);

        let (path, is_dir) = manager.search("gf_tmp", &[]);
        assert_eq!(path, nested);
        assert!(is_dir);
        let (path, is_dir) = manager.search("gf_tmp", &["gf.txt"]);
        assert_eq!(path, gfile::join(&[&nested, "gf.txt"]));
        assert!(!is_dir);

        // Searching for the root itself through "/" returns the root.
        let (path, is_dir) = manager.search("/", &[]);
        assert_eq!(path, root);
        assert!(is_dir);

        remove_all(&root).expect("cleanup");
    }

    #[test]
    fn cache_lane_indexes_and_searches() {
        let root = sandbox("cache-index");
        let nested = gfile::join(&[&root, "gf_tmp"]);
        let file = gfile::join(&[&nested, "gf.txt"]);
        put_contents(&file, "").expect("file");

        let manager = SPath::new_with_cache(&root, true);
        let (path, is_dir) = manager.search("gf_tmp", &[]);
        assert_eq!(path, nested);
        assert!(is_dir);
        let (path, is_dir) = manager.search("gf_tmp", &["gf.txt"]);
        assert_eq!(path, file);
        assert!(!is_dir);
        let (path, is_dir) = manager.search("gf_tmp/gf.txt", &[]);
        assert_eq!(path, file);
        assert!(!is_dir);
        let (path, is_dir) = manager.search("/", &[]);
        assert_eq!(path, root);
        assert!(is_dir);
        assert_eq!(
            manager.all_paths(),
            vec![
                "/".to_owned(),
                "/gf_tmp".to_owned(),
                "/gf_tmp/gf.txt".to_owned()
            ]
        );
        // A cache miss does not fall back to the filesystem.
        let (path, _) = manager.search("missing", &[]);
        assert!(path.is_empty());

        remove_all(&root).expect("cleanup");
    }

    #[test]
    fn cache_lane_tracks_created_and_removed_files() {
        let root = sandbox("cache-watch");
        let manager = SPath::new_with_cache(&root, true);
        let file = gfile::join(&[&root, "watched.txt"]);
        put_contents(&file, "watch").expect("file");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while manager.search("watched.txt", &[]).0.is_empty()
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(manager.search("watched.txt", &[]).0, file);

        gfile::remove(&file).expect("remove file");
        manager.remove(&file);
        assert!(manager.search("watched.txt", &[]).0.is_empty());
        remove_all(&root).expect("cleanup");
    }

    #[test]
    fn get_shares_one_manager_per_root() {
        let root = sandbox("shared");
        let first = get(&root);
        let second = get(&root);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.size(), 1);
        remove_all(&root).expect("cleanup");
    }
}
