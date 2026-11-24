//! Port of `gfile_scan.go`.

use std::fs;

use gf_errors::gerror::{self, Error};
use gf_text::gstr;

use super::{MAX_SCAN_DEPTH, SEPARATOR, abs, is_dir, path_match, wrap_io};

/// Returns the paths under `path` matching `pattern`, sorted, like Go's
/// `ScanDir`.
///
/// `pattern` may hold several comma separated file name patterns.
pub fn scan_dir(path: &str, pattern: &str, recursive: bool) -> Result<Vec<String>, Error> {
    let mut list = do_scan_dir(0, path, pattern, recursive, &mut ignore)?;
    list.sort();
    Ok(list)
}

/// [`scan_dir`] with a filter callback, like Go's `ScanDirFunc`.
///
/// The callback may rewrite the path; returning an empty string drops it.
pub fn scan_dir_func(
    path: &str,
    pattern: &str,
    recursive: bool,
    mut handler: impl FnMut(String) -> String,
) -> Result<Vec<String>, Error> {
    let mut list = do_scan_dir(0, path, pattern, recursive, &mut handler)?;
    list.sort();
    Ok(list)
}

/// [`scan_dir`] returning only files, like Go's `ScanDirFile`.
pub fn scan_dir_file(path: &str, pattern: &str, recursive: bool) -> Result<Vec<String>, Error> {
    let mut list = do_scan_dir(0, path, pattern, recursive, &mut only_files)?;
    list.sort();
    Ok(list)
}

/// [`scan_dir_file`] with a filter callback, like Go's `ScanDirFileFunc`.
pub fn scan_dir_file_func(
    path: &str,
    pattern: &str,
    recursive: bool,
    mut handler: impl FnMut(String) -> String,
) -> Result<Vec<String>, Error> {
    let mut filtered = move |candidate: String| {
        if is_dir(&candidate) {
            String::new()
        } else {
            handler(candidate)
        }
    };
    let mut list = do_scan_dir(0, path, pattern, recursive, &mut filtered)?;
    list.sort();
    Ok(list)
}

/// Keeps every path, the default handler.
fn ignore(path: String) -> String {
    path
}

/// Keeps files only, like the handler `ScanDirFile` installs.
fn only_files(path: String) -> String {
    if is_dir(&path) { String::new() } else { path }
}

/// Scans one directory level, recursing when asked, like `doScanDir`.
fn do_scan_dir(
    depth: usize,
    path: &str,
    pattern: &str,
    recursive: bool,
    handler: &mut dyn FnMut(String) -> String,
) -> Result<Vec<String>, Error> {
    if depth >= MAX_SCAN_DEPTH {
        return Err(gerror::newf(
            format!("directory scanning exceeds max recursive depth: {MAX_SCAN_DEPTH}"),
            Vec::new(),
        ));
    }
    let entries = fs::read_dir(path).map_err(|error| {
        wrap_io(
            error,
            format!("read directory files failed from path \"{path}\""),
        )
    })?;
    let patterns = gstr::split_and_trim(pattern, ",", None);
    let mut list = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| gerror::new(error.to_string()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let mut file_path = format!("{path}{SEPARATOR}{name}");
        if is_dir(&file_path) && recursive {
            let nested = do_scan_dir(depth + 1, &file_path, pattern, true, handler)?;
            list.extend(nested);
        }
        file_path = handler(file_path);
        if file_path.is_empty() {
            continue;
        }
        for candidate in &patterns {
            if path_match(candidate, &name).unwrap_or(false) {
                let absolute = abs(&file_path);
                if !absolute.is_empty() {
                    list.push(absolute);
                }
            }
        }
    }
    Ok(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfile-scan-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    fn layout(name: &str) -> String {
        let base = sandbox(name);
        put_contents(&super::super::join(&[&base, "a.go"]), "").expect("a");
        put_contents(&super::super::join(&[&base, "b.txt"]), "").expect("b");
        mkdir(&super::super::join(&[&base, "nested"])).expect("nested");
        put_contents(&super::super::join(&[&base, "nested", "c.go"]), "").expect("c");
        base
    }

    #[test]
    fn scan_dir_filters_by_pattern() {
        let base = layout("filters");
        let files = scan_dir(&base, "*.go", false).expect("scan");
        assert_eq!(files.len(), 1);
        assert!(files[0].ends_with("a.go"));

        let all = scan_dir(&base, "*", false).expect("scan all");
        assert_eq!(all.len(), 3);
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn scan_dir_recursive_and_files_only() {
        let base = layout("recursive");
        let files = scan_dir(&base, "*.go", true).expect("scan");
        assert_eq!(files.len(), 2);
        let mut sorted = files.clone();
        sorted.sort();
        assert_eq!(files, sorted);

        let files_only = scan_dir_file(&base, "*", true).expect("files");
        assert_eq!(files_only.len(), 3);
        assert!(files_only.iter().all(|path| !is_dir(path)));
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn scan_dir_func_can_drop_and_rewrite() {
        let base = layout("handler");
        let dropped = scan_dir_func(&base, "*.go", false, |_| String::new()).expect("scan");
        assert!(dropped.is_empty());

        let renamed =
            scan_dir_func(&base, "*.go", false, |path| format!("{path}.bak")).expect("scan");
        assert_eq!(renamed.len(), 1);
        assert!(renamed[0].ends_with("a.go.bak"));
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn scan_dir_reports_missing_directories() {
        let base = sandbox("missing");
        let error =
            scan_dir(&super::super::join(&[&base, "nope"]), "*", false).expect_err("missing");
        assert!(error.to_string().contains("read directory files failed"));
        remove_all(&base).expect("cleanup");
    }
}
