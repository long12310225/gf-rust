//! Port of `gfile_search.go`.

use gf_errors::gerror::{self, Error};

use super::{SEPARATOR, pwd, real_path, self_dir};

/// Searches for `name` in the priority paths, the working directory and the
/// directory of the running binary, like Go's `Search`.
///
/// Go also searches `MainPkgPath`; the port has no equivalent (ADR-040). On
/// failure the error lists every searched path, exactly like Go.
pub fn search(name: &str, priority_search_paths: &[&str]) -> Result<String, Error> {
    let absolute = real_path(name);
    if !absolute.is_empty() {
        return Ok(absolute);
    }
    let mut directories: Vec<String> = Vec::new();
    for path in priority_search_paths {
        if !path.is_empty() && !directories.contains(&(*path).to_owned()) {
            directories.push((*path).to_owned());
        }
    }
    for path in [pwd(), self_dir()] {
        if !path.is_empty() && !directories.contains(&path) {
            directories.push(path);
        }
    }
    for directory in &directories {
        let candidate = real_path(&format!("{directory}{SEPARATOR}{name}"));
        if !candidate.is_empty() {
            return Ok(candidate);
        }
    }
    let mut message = format!("cannot find \"{name}\" in following paths:");
    for (index, directory) in directories.iter().enumerate() {
        message.push_str(&format!("\n{}. {directory}", index + 1));
    }
    Err(gerror::new(message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    #[test]
    fn search_finds_files_and_reports_misses() {
        let base = temp(&["gf-rust-gfile-search-tests", "search"]);
        let _ = remove_all(&base);
        mkdir(&base).expect("sandbox");
        put_contents(&crate::gfile::join(&[&base, "found.txt"]), "x").expect("put");

        let found = search("found.txt", &[&base]).expect("found");
        assert!(found.ends_with("found.txt"));

        let error = search("missing-xyz.txt", &[&base]).expect_err("missing");
        let message = error.to_string();
        assert!(message.contains("cannot find \"missing-xyz.txt\""));
        assert!(message.contains("1. "));
        remove_all(&base).expect("cleanup");
    }
}
