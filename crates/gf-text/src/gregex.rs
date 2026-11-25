//! Port of `text/gregex`.
//!
//! Patterns are compiled once and cached, like the Go package. `regex` is built
//! with Unicode off so `\w`, `\d`, `\s` and case folding are ASCII, matching
//! Go's `regexp`. Byte APIs treat their input as UTF-8 text, which is how Go's
//! engine consumes bytes.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use gf_errors::gerror;
use regex::{Captures, Regex};

fn cache() -> &'static RwLock<HashMap<String, Arc<Regex>>> {
    static CACHE: OnceLock<RwLock<HashMap<String, Arc<Regex>>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

fn get_regexp(pattern: &str) -> Result<Arc<Regex>, gerror::Error> {
    if let Ok(guard) = cache().read()
        && let Some(regex) = guard.get(pattern)
    {
        return Ok(regex.clone());
    }
    let translated = translate(pattern);
    let compiled = Regex::new(&translated).map_err(|err| wrap(err, pattern))?;
    let compiled = Arc::new(compiled);
    if let Ok(mut guard) = cache().write() {
        guard.insert(pattern.to_owned(), compiled.clone());
    }
    Ok(compiled)
}

/// Rewrites Go's ASCII Perl classes (`\w`, `\d`, `\s` and complements) into
/// explicit classes, and expands `\Q...\E`, so the Unicode-aware engine keeps
/// Go's meaning. Inside character classes only the positive forms are
/// rewritten; a complemented class such as `[\W]` keeps the engine's Unicode
/// meaning.
fn translate(pattern: &str) -> String {
    if !pattern.contains('\\') {
        return pattern.to_owned();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut i = 0;
    let mut in_class = false;
    let mut class_start = false;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '[' if !in_class => {
                in_class = true;
                class_start = true;
                out.push('[');
                i += 1;
                if chars.get(i) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
                if chars.get(i) == Some(&']') {
                    out.push_str("\\]");
                    i += 1;
                    class_start = false;
                }
                continue;
            }
            ']' if in_class && !class_start => {
                in_class = false;
                out.push(']');
                i += 1;
                continue;
            }
            '\\' => {
                let Some(&next) = chars.get(i + 1) else {
                    out.push('\\');
                    i += 1;
                    continue;
                };
                if next == 'Q' {
                    i += 2;
                    while i < chars.len() {
                        if chars[i] == '\\' && chars.get(i + 1) == Some(&'E') {
                            i += 2;
                            break;
                        }
                        if regex_syntax_escape(chars[i]) {
                            out.push('\\');
                        }
                        out.push(chars[i]);
                        i += 1;
                    }
                    continue;
                }
                let replacement = if in_class {
                    match next {
                        'w' => Some("0-9A-Za-z_"),
                        'd' => Some("0-9"),
                        's' => Some(" \\t\\n\\f\\r"),
                        _ => None,
                    }
                } else {
                    match next {
                        'w' => Some("[0-9A-Za-z_]"),
                        'W' => Some("[^0-9A-Za-z_]"),
                        'd' => Some("[0-9]"),
                        'D' => Some("[^0-9]"),
                        's' => Some("[ \\t\\n\\f\\r]"),
                        'S' => Some("[^ \\t\\n\\f\\r]"),
                        _ => None,
                    }
                };
                match replacement {
                    Some(class) => out.push_str(class),
                    None => {
                        out.push('\\');
                        out.push(next);
                    }
                }
                i += 2;
                class_start = false;
                continue;
            }
            _ => out.push(ch),
        }
        if in_class {
            class_start = false;
        }
        i += 1;
    }
    out
}

fn regex_syntax_escape(ch: char) -> bool {
    matches!(
        ch,
        '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
    )
}

fn wrap(err: regex::Error, pattern: &str) -> gerror::Error {
    let text = format!("regexp.Compile failed for pattern \"{pattern}\"");
    match gerror::wrap_std(Some(Box::new(err)), text.clone()) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Escapes regular expression metacharacters, like Go's `regexp.QuoteMeta`.
pub fn quote(s: &str) -> String {
    const SPECIALS: &[u8] = b"\\.+*?()|[]{}^$";
    if !s.bytes().any(|byte| SPECIALS.contains(&byte)) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for byte in s.bytes() {
        if SPECIALS.contains(&byte) {
            out.push('\\');
        }
        out.push(char::from(byte));
    }
    out
}

/// Reports whether `pattern` compiles.
pub fn validate(pattern: &str) -> Result<(), gerror::Error> {
    get_regexp(pattern).map(|_| ())
}

/// Reports whether `src` matches `pattern`; an invalid pattern is `false`.
pub fn is_match(pattern: &str, src: &[u8]) -> bool {
    is_match_string(pattern, &lossy(src))
}

/// Reports whether `src` matches `pattern`; an invalid pattern is `false`.
pub fn is_match_string(pattern: &str, src: &str) -> bool {
    match get_regexp(pattern) {
        Ok(regex) => regex.is_match(src),
        Err(_) => false,
    }
}

fn capture_strings(captures: &Captures<'_>) -> Vec<String> {
    captures
        .iter()
        .map(|item| item.map_or_else(String::new, |m| m.as_str().to_owned()))
        .collect()
}

/// Returns the full match and its submatches.
pub fn match_string(pattern: &str, src: &str) -> Result<Option<Vec<String>>, gerror::Error> {
    let regex = get_regexp(pattern)?;
    Ok(regex
        .captures(src)
        .map(|captures| capture_strings(&captures)))
}

/// Returns the full match and its submatches as bytes.
pub fn match_bytes(pattern: &str, src: &[u8]) -> Result<Option<Vec<Vec<u8>>>, gerror::Error> {
    let text = lossy(src);
    Ok(match_string(pattern, &text)?
        .map(|matched| matched.into_iter().map(String::into_bytes).collect()))
}

/// Returns every match and its submatches.
pub fn match_all_string(pattern: &str, src: &str) -> Result<Vec<Vec<String>>, gerror::Error> {
    let regex = get_regexp(pattern)?;
    Ok(regex
        .captures_iter(src)
        .map(|captures| capture_strings(&captures))
        .collect())
}

/// Returns every match and its submatches as bytes.
pub fn match_all_bytes(pattern: &str, src: &[u8]) -> Result<Vec<Vec<Vec<u8>>>, gerror::Error> {
    let text = lossy(src);
    Ok(match_all_string(pattern, &text)?
        .into_iter()
        .map(|matched| matched.into_iter().map(String::into_bytes).collect())
        .collect())
}

/// Replaces every match with `replacement`, expanding `$1` and `${name}`.
pub fn replace_string(
    pattern: &str,
    replacement: &str,
    src: &str,
) -> Result<String, gerror::Error> {
    let regex = get_regexp(pattern)?;
    Ok(regex.replace_all(src, replacement).into_owned())
}

/// Replaces every match with `replacement`, expanding `$1` and `${name}`.
pub fn replace(pattern: &str, replacement: &[u8], src: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let text = lossy(src);
    Ok(replace_string(pattern, &lossy(replacement), &text)?.into_bytes())
}

/// Replaces every match with the result of `replace_func`.
pub fn replace_string_func(
    pattern: &str,
    src: &str,
    mut replace_func: impl FnMut(&str) -> String,
) -> Result<String, gerror::Error> {
    let regex = get_regexp(pattern)?;
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for matched in regex.find_iter(src) {
        out.push_str(&src[last..matched.start()]);
        out.push_str(&replace_func(matched.as_str()));
        last = matched.end();
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// Replaces every match with the result of `replace_func` over bytes.
pub fn replace_func(
    pattern: &str,
    src: &[u8],
    mut replace_func: impl FnMut(&[u8]) -> Vec<u8>,
) -> Result<Vec<u8>, gerror::Error> {
    let text = lossy(src);
    Ok(replace_string_func(pattern, &text, |matched| {
        lossy(&replace_func(matched.as_bytes()))
    })?
    .into_bytes())
}

/// Replaces every match with the result of `replace_func` over its submatches.
pub fn replace_string_func_match(
    pattern: &str,
    src: &str,
    mut replace_func: impl FnMut(&[String]) -> String,
) -> Result<String, gerror::Error> {
    let regex = get_regexp(pattern)?;
    let mut out = String::with_capacity(src.len());
    let mut last = 0;
    for captures in regex.captures_iter(src) {
        let Some(full) = captures.get(0) else {
            continue;
        };
        out.push_str(&src[last..full.start()]);
        let matched = capture_strings(&captures);
        out.push_str(&replace_func(&matched));
        last = full.end();
    }
    out.push_str(&src[last..]);
    Ok(out)
}

/// Replaces every match with the result of `replace_func` over submatch bytes.
pub fn replace_func_match(
    pattern: &str,
    src: &[u8],
    mut replace_func: impl FnMut(&[Vec<u8>]) -> Vec<u8>,
) -> Result<Vec<u8>, gerror::Error> {
    let text = lossy(src);
    Ok(replace_string_func_match(pattern, &text, |matched| {
        let matched: Vec<Vec<u8>> = matched.iter().map(|s| s.as_bytes().to_vec()).collect();
        lossy(&replace_func(&matched))
    })?
    .into_bytes())
}

/// Splits `src` around matches, with Go's `regexp.Split` semantics.
pub fn split(pattern: &str, src: &str) -> Result<Vec<String>, gerror::Error> {
    let regex = get_regexp(pattern)?;
    let mut out = Vec::new();
    let mut begin = 0;
    let mut last_match_start = 0;
    for matched in regex.find_iter(src) {
        last_match_start = matched.start();
        if matched.end() != 0 {
            out.push(src[begin..matched.start()].to_owned());
        }
        begin = matched.end();
    }
    if last_match_start != src.len() {
        out.push(src[begin..].to_owned());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATTERN_ERR: &str = r"([\d+";

    #[test]
    fn quote_matches_go() {
        assert_eq!(quote("[foo]"), r"\[foo\]");
        assert_eq!(quote(r"a.b*c+d"), r"a\.b\*c\+d");
        assert_eq!(quote("abc-123"), "abc-123");
    }

    #[test]
    fn validate_detects_bad_patterns() {
        assert!(validate(r"(.+):(\d+)").is_ok());
        assert!(validate(r"((.+):(\d+)").is_err());
        assert!(validate(PATTERN_ERR).is_err());
    }

    #[test]
    fn is_match_matches_go_cases() {
        let pattern = r"(.+):(\d+)";
        assert!(is_match(pattern, b"sfs:2323"));
        assert!(!is_match(pattern, b"sfs2323"));
        assert!(!is_match(pattern, b"sfs:"));
        assert!(!is_match(PATTERN_ERR, b"sfs:"));
        assert!(is_match_string(pattern, "sfs:2323"));
    }

    #[test]
    fn match_keeps_unset_tail() {
        let matched = match_string("a(a+b+)b", "acbbaaabbdd")
            .expect("pattern")
            .expect("match");
        assert_eq!(matched, ["aaabb", "aab"]);

        // Go's FindStringSubmatch keeps unmatched trailing groups as "".
        let matched = match_string("(a)(b)?", "ac")
            .expect("pattern")
            .expect("match");
        assert_eq!(matched, ["a", "a", ""]);

        let matched = match_string("(a)?(b)", "b")
            .expect("pattern")
            .expect("match");
        assert_eq!(matched, ["b", "", "b"]);

        let all = match_all_string("(a)(b)?", "aab").expect("pattern");
        assert_eq!(all, vec![vec!["a", "a", ""], vec!["ab", "a", "b"]]);

        assert!(match_string(r"zzz", "abc").expect("pattern").is_none());
        assert!(match_string(PATTERN_ERR, "abc").is_err());

        let bytes = match_bytes("a(a+b+)b", b"acbbaaabbdd")
            .expect("pattern")
            .expect("match");
        assert_eq!(bytes[0], b"aaabb");
        assert_eq!(bytes[1], b"aab");
    }

    #[test]
    fn match_all_matches_go_cases() {
        let src = "acbbaaabbdd其他的acbbaaabbdd";
        let all = match_all_string("a(a+b+)b", src).expect("pattern");
        assert_eq!(all.len(), 2);
        assert_eq!(all[0], ["aaabb", "aab"]);
        assert_eq!(all[1], ["aaabb", "aab"]);

        let all = match_all_bytes("a(a+b+)b", src.as_bytes()).expect("pattern");
        assert_eq!(all[0][0], b"aaabb");
        assert!(match_all_string(PATTERN_ERR, src).is_err());
    }

    #[test]
    fn replace_matches_go_cases() {
        let src = "acbbaaabbdd";
        assert_eq!(
            replace_string("a(a+b+)b", "12345", src).expect("pattern"),
            "acbb12345dd"
        );
        assert_eq!(
            replace("a(a+b+)b", b"12345", src.as_bytes()).expect("pattern"),
            b"acbb12345dd"
        );
        assert!(replace_string(PATTERN_ERR, "x", src).is_err());
    }

    #[test]
    fn replace_func_matches_go_cases() {
        let src = "acbbaaabbdd";
        let replaced = replace_string_func("a(a+b+)b", src, |matched| {
            if matched.contains("aaa") {
                "3个a".to_owned()
            } else {
                format!("[x{matched}y]")
            }
        })
        .expect("pattern");
        assert_eq!(replaced, "acbb3个add");

        let replaced = replace_func("a(a+b+)b", src.as_bytes(), |matched| {
            if lossy(matched).contains("aaa") {
                "3个a".as_bytes().to_vec()
            } else {
                let mut out = b"[x".to_vec();
                out.extend_from_slice(matched);
                out.push(b'y');
                out.push(b']');
                out
            }
        })
        .expect("pattern");
        assert_eq!(replaced, "acbb3个add".as_bytes());

        assert!(replace_string_func(PATTERN_ERR, src, |_| String::new()).is_err());
    }

    #[test]
    fn replace_func_match_exposes_groups() {
        let src = "1234567890";
        let pattern = r"(\d{3})(\d{3})(.+)";

        let replaced =
            replace_string_func_match(pattern, src, |matched| matched[0].clone()).expect("pattern");
        assert_eq!(replaced, src);
        for (index, expected) in [(1, "123"), (2, "456"), (3, "7890")] {
            let replaced =
                replace_string_func_match(pattern, src, |matched| matched[index].clone())
                    .expect("pattern");
            assert_eq!(replaced, expected);
        }

        let replaced = replace_func_match(pattern, src.as_bytes(), |matched| matched[2].clone())
            .expect("pattern");
        assert_eq!(replaced, b"456");
        assert!(replace_string_func_match(PATTERN_ERR, src, |_| String::new()).is_err());
    }

    #[test]
    fn split_matches_go_semantics() {
        assert_eq!(
            split("a(a+b+)b", "acbbaaabbdd").expect("pattern"),
            ["acbb", "dd"]
        );
        assert_eq!(
            split("a(a+b+)b", "acbbaaxbbdd").expect("pattern"),
            ["acbbaaxbbdd"]
        );
        assert_eq!(split(",", "a,b,").expect("pattern"), ["a", "b", ""]);
        assert_eq!(split("", "abc").expect("pattern"), ["a", "b", "c"]);
        assert!(split(PATTERN_ERR, "abc").is_err());
    }

    #[test]
    fn ascii_classes_match_go() {
        assert!(is_match_string(r"^\w+$", "abc_123"));
        assert!(!is_match_string(r"^\w+$", "测试"));
        assert!(is_match_string(r"^\d+$", "123"));
        assert!(is_match_string(r"^.$", "测"));
    }

    #[test]
    fn translate_rewrites_go_classes() {
        assert_eq!(translate(r"^\w+\d$"), "^[0-9A-Za-z_]+[0-9]$");
        assert_eq!(translate(r"[\w.-]+"), "[0-9A-Za-z_.-]+");
        assert_eq!(translate(r"\Qa.b\E"), r"a\.b");
        assert_eq!(translate(r"\s+"), "[ \\t\\n\\f\\r]+");
        assert_eq!(translate(r"[^]]"), "[^]]");
    }
}
