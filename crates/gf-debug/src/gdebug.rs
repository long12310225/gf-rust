//! Port of `debug/gdebug` using `std::backtrace`.
//!
//! Go reports `runtime.Caller` names and absolute source paths. Rust reports
//! native symbols and the paths recorded at compile time, so `caller_*` keeps
//! the same shape but different text (ADR-029). There are no goroutines, so
//! [`goroutine_id`] is always zero.

use std::backtrace::Backtrace;
use std::path::Path;

/// Maximum frames inspected, like Go's `maxCallerDepth`.
const MAX_CALLER_DEPTH: usize = 1000;

#[derive(Clone, Debug)]
struct Frame {
    function: String,
    file: String,
    line: u32,
}

fn capture_frames() -> Vec<Frame> {
    let backtrace = Backtrace::force_capture();
    let rendered = backtrace.to_string();
    let mut frames = Vec::new();
    let mut pending: Option<String> = None;
    for line in rendered.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("at ") {
            if let Some(function) = pending.take() {
                let (file, line) = match rest.rsplit_once(':') {
                    Some((file, line)) => {
                        (file.to_owned(), line.parse::<u32>().unwrap_or_default())
                    }
                    None => (rest.to_owned(), 0),
                };
                frames.push(Frame {
                    function,
                    file,
                    line,
                });
            }
            continue;
        }
        let Some((index, function)) = trimmed.split_once(':') else {
            continue;
        };
        if index.trim().parse::<usize>().is_ok() {
            let function = function.trim().to_owned();
            if !function.is_empty() {
                pending = Some(function);
            }
        }
    }
    frames
}

fn is_filtered(frame: &Frame, filters: &[String]) -> bool {
    if frame.file.is_empty() {
        return true;
    }
    let normalized = frame.file.replace('\\', "/");
    if frame.function.starts_with("gf_debug::gdebug::") && !frame.function.contains("::tests::") {
        return true;
    }
    if normalized.contains("/rustc/") {
        return true;
    }
    filters
        .iter()
        .any(|filter| !filter.is_empty() && normalized.contains(filter.as_str()))
}

fn caller_frame(filters: &[String], skip: usize) -> Option<Frame> {
    capture_frames()
        .into_iter()
        .filter(|frame| !is_filtered(frame, filters))
        .take(MAX_CALLER_DEPTH)
        .nth(skip)
}

/// Returns the caller's function name, file path and line number.
pub fn caller(skip: usize) -> (String, String, u32) {
    match caller_frame(&[], skip) {
        Some(frame) => (frame.function, frame.file, frame.line),
        None => (String::new(), String::new(), 0),
    }
}

/// Like [`caller`] but skipping frames whose path contains any filter.
pub fn caller_with_filter(filters: &[String], skip: usize) -> (String, String, u32) {
    match caller_frame(filters, skip) {
        Some(frame) => (frame.function, frame.file, frame.line),
        None => (String::new(), String::new(), 0),
    }
}

/// Returns the package part of the caller's function path.
pub fn caller_package() -> String {
    let (function, _, _) = caller(0);
    package_from_function(&function)
}

fn package_from_function(function: &str) -> String {
    match function.rsplit_once("::") {
        Some((package, _)) => package.to_owned(),
        None => function.to_owned(),
    }
}

/// Returns the caller's function name without its module path.
pub fn caller_function() -> String {
    let (function, _, _) = caller(0);
    function
        .rsplit_once("::")
        .map_or(function.as_str(), |(_, name)| name)
        .to_owned()
}

/// Returns the caller's source file path.
pub fn caller_file_path() -> String {
    caller(0).1
}

/// Returns the caller's source directory.
pub fn caller_directory() -> String {
    let path = caller_file_path();
    Path::new(&path)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Returns `file:line` for the caller.
pub fn caller_file_line() -> String {
    let (_, path, line) = caller(0);
    format!("{path}:{line}")
}

/// Returns `file_name:line` for the caller.
pub fn caller_file_line_short() -> String {
    let (_, path, line) = caller(0);
    let name = Path::new(&path)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(path);
    format!("{name}:{line}")
}

/// Returns a numbered stack trace in GoFrame's format.
pub fn stack(filters: &[String], skip: usize) -> String {
    let mut out = String::new();
    let frames: Vec<Frame> = capture_frames()
        .into_iter()
        .filter(|frame| !is_filtered(frame, filters))
        .skip(skip)
        .take(MAX_CALLER_DEPTH)
        .collect();
    for (offset, frame) in frames.into_iter().enumerate() {
        let index = offset + 1;
        let space = if index > 9 { " " } else { "  " };
        out.push_str(&format!(
            "{index}.{space}{}\n    {}:{}\n",
            frame.function, frame.file, frame.line
        ));
    }
    out
}

/// Prints [`stack`] to standard output.
pub fn print_stack(filters: &[String], skip: usize) {
    print!("{}", stack(filters, skip));
}

/// Returns the BKDR-based base36 version of the running binary.
pub fn bin_version() -> String {
    let Ok(path) = std::env::current_exe() else {
        return String::new();
    };
    let Ok(content) = std::fs::read(path) else {
        return String::new();
    };
    base36(u64::from(gf_encoding::ghash::bkdr(&content)))
}

fn base36(mut value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// Returns the MD5 of the running binary.
pub fn bin_version_md5() -> String {
    let Ok(path) = std::env::current_exe() else {
        return String::new();
    };
    gf_crypto::gmd5::encrypt_file(&path.to_string_lossy()).unwrap_or_default()
}

/// Returns zero: the port has threads, not goroutines.
pub fn goroutine_id() -> i64 {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caller_reports_this_file() {
        let (function, path, line) = caller(0);
        assert!(!function.is_empty());
        assert!(path.ends_with("gdebug.rs"), "{path}");
        assert!(line > 0);
        assert!(caller_file_line().contains(':'));
        assert!(caller_file_line_short().starts_with("gdebug.rs:"));
        assert!(caller_directory().ends_with("src"));
        assert!(!caller_function().is_empty());
        assert!(!caller_package().is_empty());
    }

    #[test]
    fn filter_skips_matching_paths() {
        let (_, path, _) = caller_with_filter(&["gdebug.rs".to_owned()], 0);
        assert!(!path.ends_with("gdebug.rs"), "{path}");
    }

    #[test]
    fn stack_is_numbered_and_skips() {
        let rendered = stack(&[], 0);
        assert!(rendered.starts_with("1.  "), "{rendered}");
        assert!(rendered.contains("gdebug.rs:"));
        let skipped = stack(&[], 1);
        assert_ne!(skipped, rendered);
    }

    #[test]
    fn binary_versions_are_computed() {
        assert!(!bin_version().is_empty());
        assert_eq!(bin_version_md5().len(), 32);
        assert_eq!(goroutine_id(), 0);
    }

    #[test]
    fn base36_matches_go() {
        assert_eq!(base36(0), "0");
        assert_eq!(base36(35), "z");
        assert_eq!(base36(36), "10");
    }
}
