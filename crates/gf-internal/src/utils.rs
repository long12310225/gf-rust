//! Port of `internal/utils`: shared string, value and reader helpers.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use gf_core::Value;
use indexmap::IndexMap;

use crate::command;
use crate::empty;

/// Characters stripped by [`trim`] when no mask is given.
///
/// The Go cutset is a byte string ending in the bytes `0x85` and `0xA0`, which
/// are invalid UTF-8 and therefore decode to `U+FFFD` when Go looks them up as
/// runes. The port keeps that observable behavior instead of trimming
/// `U+0085` / `U+00A0`.
pub const DEFAULT_TRIM_CHARS: &[char] = &[
    '\t', '\u{000B}', '\n', '\r', '\u{000C}', ' ', '\u{0000}', '\u{FFFD}',
];

/// Returns whether `byte` is an ASCII upper-case letter.
pub fn is_letter_upper(byte: u8) -> bool {
    byte.is_ascii_uppercase()
}

/// Returns whether `byte` is an ASCII lower-case letter.
pub fn is_letter_lower(byte: u8) -> bool {
    byte.is_ascii_lowercase()
}

/// Returns whether `byte` is an ASCII letter.
pub fn is_letter(byte: u8) -> bool {
    byte.is_ascii_alphabetic()
}

/// Returns whether `s` is numeric, including float strings like `123.456`.
pub fn is_numeric(s: &str) -> bool {
    let bytes = s.as_bytes();
    let length = bytes.len();
    if length == 0 {
        return false;
    }
    let mut dot_count = 0;
    for (i, &byte) in bytes.iter().enumerate() {
        if (byte == b'-' || byte == b'+') && i == 0 {
            if length == 1 {
                return false;
            }
            continue;
        }
        if byte == b'.' {
            dot_count += 1;
            if i > 0 && i < length - 1 && bytes[i - 1].is_ascii_digit() {
                continue;
            }
            return false;
        }
        if !byte.is_ascii_digit() {
            return false;
        }
    }
    dot_count <= 1
}

/// Returns a copy of `s` with its first ASCII lower-case letter upper-cased.
pub fn uc_first(s: &str) -> String {
    let bytes = s.as_bytes();
    let Some(&first) = bytes.first() else {
        return s.to_owned();
    };
    if !is_letter_lower(first) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    out.push(char::from(first - 32));
    out.push_str(&s[1..]);
    out
}

/// Replaces every `key` with its value, in map insertion order.
pub fn replace_by_map(origin: &str, replaces: &IndexMap<String, String>) -> String {
    let mut out = origin.to_owned();
    for (key, value) in replaces {
        out = out.replace(key.as_str(), value);
    }
    out
}

/// Removes every symbol, keeping ASCII letters, ASCII digits and non-ASCII
/// characters.
pub fn remove_symbols(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if (c as u32) > 127 || c.is_ascii_alphanumeric() {
            out.push(c);
        }
    }
    out
}

/// Compares two strings case-insensitively after removing symbols.
pub fn equal_fold_without_chars(s1: &str, s2: &str) -> bool {
    remove_symbols(s1).to_lowercase() == remove_symbols(s2).to_lowercase()
}

/// Splits `s` by `delimiter`, trims every piece and drops the empty ones.
pub fn split_and_trim(s: &str, delimiter: &str, character_mask: Option<&str>) -> Vec<String> {
    let pieces: Vec<String> = if delimiter.is_empty() {
        s.chars().map(|c| c.to_string()).collect()
    } else {
        s.split(delimiter).map(str::to_owned).collect()
    };
    pieces
        .into_iter()
        .map(|piece| trim(&piece, character_mask))
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Strips [`DEFAULT_TRIM_CHARS`] plus the characters of `character_mask` from
/// both ends of `s`.
pub fn trim(s: &str, character_mask: Option<&str>) -> String {
    s.trim_matches(|c: char| {
        DEFAULT_TRIM_CHARS.contains(&c) || character_mask.is_some_and(|mask| mask.contains(c))
    })
    .to_owned()
}

/// Formats a command key: lower case with `_` replaced by `.`.
pub fn format_cmd_key(s: &str) -> String {
    s.replace('_', ".").to_lowercase()
}

/// Formats an environment key: upper case with `.` replaced by `_`.
pub fn format_env_key(s: &str) -> String {
    s.replace('.', "_").to_uppercase()
}

/// Un-quotes a quoted string: a single backslash is dropped, a doubled one is
/// kept once.
pub fn strip_slashes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut skip = false;
    for (i, &c) in chars.iter().enumerate() {
        if skip {
            skip = false;
        } else if c == '\\' {
            if i + 1 < chars.len() && chars[i + 1] == '\\' {
                skip = true;
            }
            continue;
        }
        out.push(c);
    }
    out
}

/// Returns whether every byte of `s` is ASCII.
pub fn is_ascii(s: &str) -> bool {
    s.is_ascii()
}

/// Returns whether `value` is nil.
pub fn is_nil(value: &Value) -> bool {
    empty::is_nil(value)
}

/// Returns whether `value` is empty.
pub fn is_empty(value: &Value) -> bool {
    empty::is_empty(value)
}

/// Returns whether `value` holds a signed integer.
pub fn is_int(value: &Value) -> bool {
    matches!(value, Value::Int(_))
}

/// Returns whether `value` holds an unsigned integer.
pub fn is_uint(value: &Value) -> bool {
    matches!(value, Value::UInt(_))
}

/// Returns whether `value` holds a float.
pub fn is_float(value: &Value) -> bool {
    matches!(value, Value::Float(_))
}

/// Returns whether `value` is a list (Go array or slice).
pub fn is_slice(value: &Value) -> bool {
    matches!(value, Value::List(_))
}

/// Returns whether `value` is an array or slice.
pub fn is_array(value: &Value) -> bool {
    is_slice(value)
}

/// Returns whether `value` is a map.
pub fn is_map(value: &Value) -> bool {
    matches!(value, Value::Map(_))
}

static DEBUG_INIT: OnceLock<()> = OnceLock::new();
static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);

/// Returns whether GoFrame debug mode is enabled (`gf.debug` / `GF_DEBUG`).
pub fn is_debug_enabled() -> bool {
    DEBUG_INIT.get_or_init(|| {
        let value = command::get_opt_with_env("gf.debug", None);
        DEBUG_ENABLED.store(
            !(value.is_empty() || value == "0" || value == "false"),
            Ordering::Relaxed,
        );
    });
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

/// Enables or disables the internal debug flag.
pub fn set_debug_enabled(enabled: bool) {
    let _ = DEBUG_INIT.set(());
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

/// Converts a list of maps to a map keyed by `key`.
///
/// When more than one item shares a key, every value is a list.
pub fn list_to_map_by_key(list: &[Value], key: &str) -> IndexMap<String, Value> {
    let mut grouped: IndexMap<String, Vec<Value>> = IndexMap::new();
    let mut has_multi_values = false;
    for item in list {
        let Value::Map(map) = item else {
            continue;
        };
        let Some(value) = map.get(key) else {
            continue;
        };
        let grouped_key = value.to_string();
        let entry = grouped.entry(grouped_key).or_default();
        entry.push(item.clone());
        if entry.len() > 1 {
            has_multi_values = true;
        }
    }
    let mut out = IndexMap::new();
    for (key, values) in grouped {
        if has_multi_values {
            out.insert(key, Value::List(values));
        } else {
            out.insert(key, values.into_iter().next().unwrap_or(Value::Null));
        }
    }
    out
}

/// Finds the key-value pair matching `key`, ignoring symbols and case.
///
/// Returns `None` when nothing matches. Go returns an empty key and a nil value
/// instead; callers compare the key against `""`.
pub fn map_possible_item_by_key<'a>(
    data: &'a IndexMap<String, Value>,
    key: &str,
) -> Option<(&'a String, &'a Value)> {
    if data.is_empty() {
        return None;
    }
    if let Some(entry) = data.get_key_value(key) {
        return Some(entry);
    }
    data.iter()
        .find(|(candidate, _)| equal_fold_without_chars(candidate, key))
}

/// Returns whether `data` contains a key matching `key`, ignoring symbols and
/// case.
pub fn map_contains_possible_key(data: &IndexMap<String, Value>, key: &str) -> bool {
    map_possible_item_by_key(data, key).is_some_and(|(found, _)| !found.is_empty())
}

/// A reader that can repeat its content, standing in for the Go body reader.
#[derive(Clone, Debug)]
pub struct ReadCloser {
    index: usize,
    content: Vec<u8>,
    repeatable: bool,
}

impl ReadCloser {
    /// Creates a reader over `content`.
    pub fn new(content: impl Into<Vec<u8>>, repeatable: bool) -> ReadCloser {
        ReadCloser {
            index: 0,
            content: content.into(),
            repeatable,
        }
    }

    /// Reads up to `buf.len()` bytes and reports whether the content ended.
    ///
    /// Go returns `io.EOF` together with the final chunk; the flag carries that
    /// second result.
    pub fn read(&mut self, buf: &mut [u8]) -> (usize, bool) {
        if self.index >= self.content.len() && self.repeatable {
            self.index = 0;
        }
        let remaining = &self.content[self.index..];
        let n = remaining.len().min(buf.len());
        buf[..n].copy_from_slice(&remaining[..n]);
        self.index += n;
        (n, self.index >= self.content.len())
    }

    /// Reads until the content ends.
    pub fn read_all(&mut self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.content.len());
        let mut buf = [0u8; 4096];
        loop {
            let (n, eof) = self.read(&mut buf);
            out.extend_from_slice(&buf[..n]);
            if eof {
                return out;
            }
        }
    }

    /// Closes the reader, which does nothing.
    pub fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_closer_returns_chunks_then_ends() {
        let mut body = ReadCloser::new(vec![1u8, 2, 3, 4], false);
        let mut buf = [0u8; 3];
        let (n, eof) = body.read(&mut buf);
        assert_eq!(&buf[..n], &[1, 2, 3]);
        assert!(!eof);
        let (n, eof) = body.read(&mut buf);
        assert_eq!(&buf[..n], &[4]);
        assert!(eof);
        let (n, eof) = body.read(&mut buf);
        assert_eq!(n, 0);
        assert!(eof);
    }

    #[test]
    fn read_closer_read_all_then_empty() {
        let mut body = ReadCloser::new(vec![1u8, 2, 3, 4], false);
        assert_eq!(body.read_all(), vec![1, 2, 3, 4]);
        assert_eq!(body.read_all(), Vec::<u8>::new());
    }

    #[test]
    fn read_closer_repeatable_restarts() {
        let mut body = ReadCloser::new(vec![1u8, 2, 3, 4], true);
        assert_eq!(body.read_all(), vec![1, 2, 3, 4]);
        assert_eq!(body.read_all(), vec![1, 2, 3, 4]);

        let mut buf = [0u8; 3];
        let (n, eof) = body.read(&mut buf);
        assert_eq!(&buf[..n], &[1, 2, 3]);
        assert!(!eof);
        let (n, eof) = body.read(&mut buf);
        assert_eq!(&buf[..n], &[4]);
        assert!(eof);
        let (n, eof) = body.read(&mut buf);
        assert_eq!(&buf[..n], &[1, 2, 3]);
        assert!(!eof);
    }

    #[test]
    fn remove_symbols_keeps_letters_digits_and_non_ascii() {
        assert_eq!(
            remove_symbols("-a-b._a c1!@#$%^&*()_+:\";'.,'01"),
            "abac101"
        );
        assert_eq!(
            remove_symbols("-a-b我._a c1!@#$%^&*是()_+:帅\";'.,哥'01"),
            "ab我ac1是帅哥01"
        );
    }

    #[test]
    fn is_numeric_matches_the_go_cases() {
        for ok in [
            "12345", "-12345", "+12345", "123.45", "-123.45", "+123.45", "0", "0.0",
        ] {
            assert!(is_numeric(ok), "{ok} should be numeric");
        }
        for bad in [
            "1+23",
            "123a45",
            "123.45.67",
            "",
            "1e10",
            "123 45",
            "!!!",
            "-a23",
            "+a23",
            "1-23",
            "123.",
            ".123",
            "123.a",
            "a.123",
            "+",
            "-",
            ".",
            "-.",
            "+.",
            "-.1",
            "+.1",
        ] {
            assert!(!is_numeric(bad), "{bad} should not be numeric");
        }
    }

    #[test]
    fn is_ascii_matches_the_go_cases() {
        assert!(is_ascii("test"));
        assert!(!is_ascii("测试"));
        assert!(!is_ascii("テスト"));
        assert!(!is_ascii("테스트"));
        assert!(!is_ascii("😁😭❤️😓"));
    }

    #[test]
    fn value_predicates_match_the_go_cases() {
        let empty_map = Value::Map(IndexMap::new());
        let non_empty_map = Value::Map(IndexMap::from([(
            "k".to_owned(),
            Value::String("v".into()),
        )]));

        assert!(!is_nil(&Value::Int(0)));
        assert!(is_nil(&Value::Null));
        assert!(!is_nil(&empty_map));
        assert!(!is_nil(&Value::List(Vec::new())));

        assert!(is_empty(&Value::Int(0)));
        assert!(is_empty(&Value::Null));
        assert!(is_empty(&empty_map));
        assert!(is_empty(&Value::List(Vec::new())));
        assert!(!is_empty(&Value::Int(1)));
        assert!(!is_empty(&Value::Float(0.1)));
        assert!(!is_empty(&non_empty_map));
        assert!(!is_empty(&Value::List(vec![Value::Int(0)])));

        assert!(is_int(&Value::Int(0)));
        assert!(is_int(&Value::Int(-1)));
        assert!(!is_int(&Value::Null));
        assert!(!is_int(&Value::Float(0.1)));
        assert!(!is_int(&empty_map));

        assert!(!is_uint(&Value::Int(1)));
        assert!(is_uint(&Value::UInt(1)));
        assert!(!is_uint(&Value::Null));

        assert!(is_float(&Value::Float(0.1)));
        assert!(is_float(&Value::Float(1.0)));
        assert!(!is_float(&Value::Int(1)));
        assert!(!is_float(&empty_map));

        assert!(is_slice(&Value::List(Vec::new())));
        assert!(is_slice(&Value::List(vec![Value::Int(0)])));
        assert!(!is_slice(&Value::Int(0)));
        assert!(!is_slice(&empty_map));
        assert!(is_array(&Value::List(Vec::new())));

        assert!(is_map(&empty_map));
        assert!(is_map(&non_empty_map));
        assert!(!is_map(&Value::List(Vec::new())));
    }

    #[test]
    fn string_helpers_match_go() {
        assert_eq!(uc_first("gf"), "Gf");
        assert_eq!(uc_first("Gf"), "Gf");
        assert_eq!(uc_first(""), "");
        assert_eq!(uc_first("测试"), "测试");

        assert_eq!(format_cmd_key("my_custom_key"), "my.custom.key");
        assert_eq!(format_cmd_key("SERVER_ADDRESS"), "server.address");
        assert_eq!(format_env_key("gf.package.variable"), "GF_PACKAGE_VARIABLE");

        assert!(equal_fold_without_chars("A-b_c", "ab c"));
        assert!(equal_fold_without_chars("A.b", "aB"));
        assert!(!equal_fold_without_chars("ab", "ac"));

        assert_eq!(strip_slashes(r"a\b"), "ab");
        assert_eq!(strip_slashes(r"a\\b"), r"a\b");
        assert_eq!(strip_slashes(r"\a\b"), "ab");

        assert_eq!(trim("  gf  ", None), "gf");
        assert_eq!(trim("--gf--", Some("-")), "gf");
        assert_eq!(trim("\u{0085}gf\u{0085}", None), "\u{0085}gf\u{0085}");
        assert_eq!(trim("\u{FFFD}gf\u{FFFD}", None), "gf");

        assert_eq!(
            split_and_trim(" 1, 2 ,, 3 ", ",", None),
            vec!["1".to_owned(), "2".to_owned(), "3".to_owned()]
        );
        assert_eq!(
            split_and_trim("ab", "", None),
            vec!["a".to_owned(), "b".to_owned()]
        );
    }

    #[test]
    fn replace_by_map_follows_insertion_order() {
        let mut replaces = IndexMap::new();
        replaces.insert("a".to_owned(), "b".to_owned());
        replaces.insert("b".to_owned(), "c".to_owned());
        assert_eq!(replace_by_map("a-b", &replaces), "c-c");
    }

    #[test]
    fn list_to_map_by_key_groups_repeated_keys() {
        let list = vec![
            Value::Map(IndexMap::from([
                ("id".to_owned(), Value::Int(1)),
                ("name".to_owned(), Value::String("a".into())),
            ])),
            Value::Map(IndexMap::from([
                ("id".to_owned(), Value::Int(2)),
                ("name".to_owned(), Value::String("b".into())),
            ])),
        ];
        let single = list_to_map_by_key(&list, "id");
        assert_eq!(single.len(), 2);
        assert!(matches!(single.get("1"), Some(Value::Map(_))));

        let repeated = vec![
            Value::Map(IndexMap::from([("id".to_owned(), Value::Int(1))])),
            Value::Map(IndexMap::from([("id".to_owned(), Value::Int(1))])),
            Value::Map(IndexMap::from([("id".to_owned(), Value::Int(2))])),
        ];
        let grouped = list_to_map_by_key(&repeated, "id");
        assert_eq!(grouped.len(), 2);
        assert!(matches!(grouped.get("1"), Some(Value::List(items)) if items.len() == 2));
        assert!(matches!(grouped.get("2"), Some(Value::List(items)) if items.len() == 1));
    }

    #[test]
    fn map_possible_item_ignores_case_and_symbols() {
        let data = IndexMap::from([
            ("Server-Name".to_owned(), Value::String("gf".into())),
            ("port".to_owned(), Value::Int(8080)),
        ]);
        let (found, value) = map_possible_item_by_key(&data, "server_name").expect("key found");
        assert_eq!(found, "Server-Name");
        assert_eq!(value, &Value::String("gf".into()));
        assert!(map_contains_possible_key(&data, "PORT"));
        assert!(!map_contains_possible_key(&data, "missing"));
        assert!(map_possible_item_by_key(&IndexMap::new(), "x").is_none());
    }

    #[test]
    fn debug_flag_can_be_set() {
        set_debug_enabled(true);
        assert!(is_debug_enabled());
        set_debug_enabled(false);
        assert!(!is_debug_enabled());
    }
}
