//! Port of `gfile_replace.go`.

use gf_errors::gerror::Error;
use gf_text::gstr;

use super::{get_contents, put_contents, scan_dir_file};

/// Replaces `search` with `replace` in `path`, like Go's `ReplaceFile`.
pub fn replace_file(search: &str, replace: &str, path: &str) -> Result<(), Error> {
    put_contents(
        path,
        &gstr::replace(&get_contents(path), search, replace, None),
    )
}

/// Rewrites `path` through `f`, like Go's `ReplaceFileFunc`.
pub fn replace_file_func(f: impl Fn(&str, &str) -> String, path: &str) -> Result<(), Error> {
    let data = get_contents(path);
    let result = f(path, &data);
    if data != result {
        return put_contents(path, &result);
    }
    Ok(())
}

/// Replaces text in every matching file under `path`, like `ReplaceDir`.
pub fn replace_dir(
    search: &str,
    replace: &str,
    path: &str,
    pattern: &str,
    recursive: bool,
) -> Result<(), Error> {
    for file in scan_dir_file(path, pattern, recursive)? {
        replace_file(search, replace, &file)?;
    }
    Ok(())
}

/// Rewrites every matching file under `path` through `f`, like
/// `ReplaceDirFunc`.
pub fn replace_dir_func(
    f: impl Fn(&str, &str) -> String,
    path: &str,
    pattern: &str,
    recursive: bool,
) -> Result<(), Error> {
    for file in scan_dir_file(path, pattern, recursive)? {
        replace_file_func(&f, &file)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{get_contents, join, mkdir, remove_all, temp};

    #[test]
    fn replace_file_and_directory() {
        let base = temp(&["gf-rust-gfile-replace-tests", "replace"]);
        let _ = remove_all(&base);
        mkdir(&base).expect("sandbox");
        let file = join(&[&base, "a.txt"]);
        put_contents(&file, "hello world").expect("put");

        replace_file("world", "gf", &file).expect("replace");
        assert_eq!(get_contents(&file), "hello gf");

        replace_file_func(|_, content| content.to_uppercase(), &file).expect("func");
        assert_eq!(get_contents(&file), "HELLO GF");

        replace_dir("GF", "GoFrame", &base, "*.txt", false).expect("dir");
        assert_eq!(get_contents(&file), "HELLO GoFrame");

        replace_dir_func(
            |path, content| format!("{content}#{}", crate::gfile::basename(path)),
            &base,
            "*.txt",
            false,
        )
        .expect("dir func");
        assert!(get_contents(&file).ends_with("#a.txt"));
        remove_all(&base).expect("cleanup");
    }
}
