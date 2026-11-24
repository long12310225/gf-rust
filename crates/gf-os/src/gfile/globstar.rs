//! Port of `gfile_match.go`: `MatchGlob` and its memoized globstar matcher.
//!
//! Go extended `filepath.Match` with a bash-like `**` component. The port keeps
//! the same rules: `**` is a globstar only when it is a complete path
//! component, `a**b` is two `*` wildcards inside one component, and both `/`
//! and `\` are separators.

use std::collections::HashMap;

use gf_errors::gerror::Error;

use super::path_match;

/// Reports whether `name` matches `pattern`, supporting the `**` globstar, like
/// Go's `gfile.MatchGlob`.
pub fn match_glob(pattern: &str, name: &str) -> Result<bool, Error> {
    if !pattern.contains("**") {
        return path_match(pattern, name);
    }
    let pattern = clean_slash(&pattern.replace('\\', "/"));
    let name = clean_slash(&name.replace('\\', "/"));
    if !has_valid_globstar(&pattern) {
        // `**` here means two adjacent `*`, which match like one `*`.
        return path_match(&pattern.replace("**", "*"), &name);
    }
    let mut memo = HashMap::new();
    do_match_globstar_memo(&pattern, &name, &mut memo)
}

/// Collapses `.`, `..` and repeated separators in a `/`-separated path, like
/// `path.Clean`.
fn clean_slash(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().map(|last| *last != "..").unwrap_or(false) {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute {
        format!("/{joined}")
    } else if joined.is_empty() {
        ".".to_owned()
    } else {
        joined
    }
}

/// Reports whether `pattern` holds a `**` that is a complete component.
fn has_valid_globstar(pattern: &str) -> bool {
    find_valid_globstar(pattern).is_some()
}

/// The first valid globstar position, or `None`, like `findValidGlobstar`.
fn find_valid_globstar(pattern: &str) -> Option<usize> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'*' && bytes[index + 1] == b'*' && is_valid_globstar_at(bytes, index) {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// Whether the `**` at `position` is a whole path component, like
/// `isValidGlobstarAt`.
fn is_valid_globstar_at(pattern: &[u8], position: usize) -> bool {
    if position > 0 && pattern[position - 1] != b'/' {
        return false;
    }
    let end = position + 2;
    if end < pattern.len() && pattern[end] != b'/' {
        return false;
    }
    true
}

/// Memoized globstar matching, like `doMatchGlobstarMemo`.
fn do_match_globstar_memo(
    pattern: &str,
    name: &str,
    memo: &mut HashMap<String, bool>,
) -> Result<bool, Error> {
    let key = format!("{pattern}\u{0}{name}");
    if let Some(cached) = memo.get(&key) {
        return Ok(*cached);
    }
    let result = do_match_globstar_core(pattern, name, memo)?;
    memo.insert(key, result);
    Ok(result)
}

/// The core of globstar matching, like `doMatchGlobstarCore`.
fn do_match_globstar_core(
    pattern: &str,
    name: &str,
    memo: &mut HashMap<String, bool>,
) -> Result<bool, Error> {
    let Some(position) = find_valid_globstar(pattern) else {
        return path_match(&pattern.replace("**", "*"), name);
    };
    let prefix = pattern[..position].trim_end_matches('/');
    let suffix = pattern[position + 2..].trim_start_matches('/');
    let mut remaining = name.to_owned();
    if !prefix.is_empty() {
        if !prefix.contains('*') && !prefix.contains('?') && !prefix.contains('[') {
            if !remaining.starts_with(prefix) {
                return Ok(false);
            }
            if remaining.len() == prefix.len() {
                remaining.clear();
            } else {
                if remaining.as_bytes()[prefix.len()] != b'/' {
                    return Ok(false);
                }
                remaining = remaining[prefix.len() + 1..].to_owned();
            }
        } else {
            let prefix_parts: Vec<&str> = prefix.split('/').collect();
            let name_parts: Vec<&str> = remaining.split('/').collect();
            if name_parts.len() < prefix_parts.len() {
                return Ok(false);
            }
            for (pattern_part, name_part) in prefix_parts.iter().zip(name_parts.iter()) {
                if !path_match(pattern_part, name_part)? {
                    return Ok(false);
                }
            }
            remaining = name_parts[prefix_parts.len()..].join("/");
        }
    }
    if suffix.is_empty() {
        return Ok(true);
    }
    if remaining.is_empty() {
        return do_match_globstar_memo(suffix, "", memo);
    }
    let name_parts: Vec<&str> = remaining.split('/').collect();
    for index in 0..=name_parts.len() {
        let rest = name_parts[index..].join("/");
        if do_match_globstar_memo(suffix, &rest, memo)? {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // The examples from Go's MatchGlob doc comment.
    #[test]
    fn globstar_examples() {
        assert!(match_glob("src/**/*.go", "src/foo/bar/main.go").expect("globstar"));
        assert!(match_glob("*.go", "main.go").expect("star"));
        assert!(match_glob("**", "any/path/file.go").expect("all"));
        assert!(match_glob("a**b", "axxb").expect("two stars"));
        assert!(!match_glob("a**b", "a/b").expect("no separator"));
        assert!(match_glob("[abc]", "a").expect("class"));
        assert!(match_glob("[", "a").is_err());
    }

    #[test]
    fn globstar_components() {
        assert!(match_glob("**/a", "x/y/a").expect("leading"));
        assert!(match_glob("**/a", "a").expect("empty prefix"));
        assert!(match_glob("a/**", "a/b/c").expect("trailing"));
        assert!(match_glob("a/**/b", "a/b").expect("zero segments"));
        assert!(match_glob("a/**/b", "a/x/y/b").expect("many segments"));
        assert!(!match_glob("a/**/b", "x/a/b").expect("anchored"));
        assert!(match_glob("**/*.go", "main.go").expect("zero dirs"));
        assert!(match_glob("src/**", "src").expect("dead star"));
        // Separators are normalised before matching.
        assert!(match_glob("src/**/*.go", "src\\pkg\\main.go").expect("backslashes"));
    }

    #[test]
    fn malformed_patterns_surface_errors() {
        assert!(match_glob("a[", "a").is_err());
    }
}
