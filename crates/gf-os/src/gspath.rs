//! Port of `os/gspath`.
//!
//! Go's package keeps a list of search roots and, when caching is enabled, a
//! file index kept current by `gfsnotify`. The port keeps the ordered root list
//! and the direct search lane; the indexed cache lane waits for `gfsnotify`
//! (ADR-041), so [`SPath::new_with_cache`] accepts Go's flag but behaves like
//! the direct lane and [`SPath::all_paths`] stays empty.
//!
//! Search results must stay under the root they were found in, which the port
//! checks with the same prefix comparison Go uses.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, RwLock};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_text::gstr;

use crate::gfile;

/// Manages a list of directories searched in insertion order, like Go's
/// `gspath.SPath`.
pub struct SPath {
    paths: RwLock<Vec<String>>,
}

impl SPath {
    /// Creates a manager for `path`, like Go's `gspath.New`.
    pub fn new(path: &str) -> SPath {
        let manager = SPath {
            paths: RwLock::new(Vec::new()),
        };
        if !path.is_empty() {
            let _ = manager.add(path);
        }
        manager
    }

    /// Creates a manager, accepting Go's `cache` flag for API parity.
    ///
    /// The indexed cache lane is not ported (ADR-041); when `cache` is true the
    /// manager still searches the filesystem directly.
    pub fn new_with_cache(path: &str, cache: bool) -> SPath {
        let _ = cache;
        SPath::new(path)
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
        Ok(trimmed)
    }

    /// Searches `name` under every root in order, like Go's `SPath.Search`.
    ///
    /// When the result is a directory, `index_files` are tried inside it and a
    /// found index file is returned instead.
    pub fn search(&self, name: &str, index_files: &[&str]) -> (String, bool) {
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
    /// Always empty until the cache lane is ported (ADR-041).
    pub fn all_paths(&self) -> Vec<String> {
        Vec::new()
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
    fn get_shares_one_manager_per_root() {
        let root = sandbox("shared");
        let first = get(&root);
        let second = get(&root);
        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(first.size(), 1);
        remove_all(&root).expect("cleanup");
    }
}
