//! Core of `text/gstr`: case, containment, counting, splitting, trimming and
//! domain/version helpers. The larger conversion and parsing families follow
//! in later units.

use std::collections::HashMap;

use gf_core::Value;
use gf_internal::utils;
use gf_util::gconv;

mod gstr_more;

pub use gstr_more::*;

/// Returned by search functions when nothing matches.
pub const NOT_FOUND_INDEX: isize = -1;

/// Returns the byte length of `s`.
pub fn len(s: &str) -> usize {
    s.len()
}

/// Returns the rune count of `s`.
pub fn len_rune(s: &str) -> usize {
    s.chars().count()
}

/// Reports whether `s` is numeric (see `internal/utils`).
pub fn is_numeric(s: &str) -> bool {
    utils::is_numeric(s)
}

/// Repeats `input` `multiplier` times.
pub fn repeat(input: &str, multiplier: usize) -> String {
    input.repeat(multiplier)
}

/// Compares two strings byte-wise, returning -1, 0 or 1.
pub fn compare(a: &str, b: &str) -> i32 {
    match a.cmp(b) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }
}

/// Compares two strings case-insensitively.
pub fn equal(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    a.chars()
        .flat_map(char::to_lowercase)
        .eq(b.chars().flat_map(char::to_lowercase))
}

/// Reports whether `s` contains `substr`.
pub fn contains(s: &str, substr: &str) -> bool {
    s.contains(substr)
}

/// Returns the byte index of `substr` in `s`, or -1.
pub fn pos(s: &str, substr: &str) -> isize {
    s.find(substr)
        .map_or(NOT_FOUND_INDEX, |index| index as isize)
}

/// Returns the byte index of `substr` in `s`, case-insensitively.
pub fn pos_i(s: &str, substr: &str) -> isize {
    if s.is_ascii() && substr.is_ascii() {
        let haystack = s.as_bytes();
        let needle = substr.as_bytes();
        if needle.is_empty() {
            return 0;
        }
        if needle.len() > haystack.len() {
            return NOT_FOUND_INDEX;
        }
        return haystack
            .windows(needle.len())
            .position(|window| window.eq_ignore_ascii_case(needle))
            .map_or(NOT_FOUND_INDEX, |index| index as isize);
    }
    pos(&to_lower(s), &to_lower(substr))
}

/// Reports whether `s` contains `substr`, case-insensitively.
pub fn contains_i(s: &str, substr: &str) -> bool {
    pos_i(s, substr) != NOT_FOUND_INDEX
}

/// Reports whether `s` contains any rune of `chars`.
pub fn contains_any(s: &str, chars: &str) -> bool {
    s.chars().any(|ch| chars.contains(ch))
}

/// Returns the index of `s` in `array`, or -1.
pub fn search_array(array: &[String], s: &str) -> isize {
    array
        .iter()
        .position(|item| item == s)
        .map_or(NOT_FOUND_INDEX, |index| index as isize)
}

/// Reports whether `array` contains `s`.
pub fn in_array(array: &[String], s: &str) -> bool {
    search_array(array, s) != NOT_FOUND_INDEX
}

/// Prefixes every element of `array` with `prefix`.
pub fn prefix_array(array: &mut [String], prefix: &str) {
    for item in array {
        *item = format!("{prefix}{item}");
    }
}

/// Escapes single quotes, double quotes and backslashes.
pub fn add_slashes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if matches!(ch, '\'' | '"' | '\\') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Un-quotes a string (see `internal/utils`).
pub fn strip_slashes(s: &str) -> String {
    utils::strip_slashes(s)
}

/// Escapes regular expression metacharacters, or only the runes of `chars`.
pub fn quote_meta(s: &str, chars: Option<&str>) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let escape = match chars {
            Some(chars) => chars.contains(ch),
            None => matches!(
                ch,
                '.' | '+' | '\\' | '(' | '$' | ')' | '[' | '^' | ']' | '*' | '?'
            ),
        };
        if escape {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

/// Reports whether `sub_domain` belongs to `main_domain` (supports `*`).
pub fn is_sub_domain(sub_domain: &str, main_domain: &str) -> bool {
    let sub_domain = sub_domain.split(':').next().unwrap_or_default();
    let main_domain = main_domain.split(':').next().unwrap_or_default();
    let sub_array: Vec<&str> = sub_domain.split('.').collect();
    let main_array: Vec<&str> = main_domain.split('.').collect();
    let sub_length = sub_array.len();
    let main_length = main_array.len();
    if main_length > sub_length {
        for item in &main_array[..main_length - sub_length] {
            if *item != "*" {
                return false;
            }
        }
    }
    if main_length > 2 && sub_length > main_length {
        return false;
    }
    let min_length = sub_length.min(main_length);
    for index in (1..=min_length).rev() {
        let main = main_array[main_length - index];
        if main == "*" {
            continue;
        }
        if main != sub_array[sub_length - index] {
            return false;
        }
    }
    true
}

/// Lower-cases `s` (Unicode aware).
pub fn to_lower(s: &str) -> String {
    s.to_lowercase()
}

/// Upper-cases `s` (Unicode aware).
pub fn to_upper(s: &str) -> String {
    s.to_uppercase()
}

/// Upper-cases the first ASCII letter.
pub fn uc_first(s: &str) -> String {
    utils::uc_first(s)
}

/// Lower-cases the first ASCII letter.
pub fn lc_first(s: &str) -> String {
    let bytes = s.as_bytes();
    let Some(&first) = bytes.first() else {
        return s.to_owned();
    };
    if !first.is_ascii_uppercase() {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    out.push(char::from(first + 32));
    out.push_str(&s[1..]);
    out
}

/// Title-cases words, lower-casing the rest of each word.
pub fn uc_words(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut at_word_start = true;
    for ch in s.chars() {
        if ch.is_alphanumeric() {
            if at_word_start {
                out.extend(ch.to_uppercase());
            } else {
                out.extend(ch.to_lowercase());
            }
            at_word_start = false;
        } else {
            out.push(ch);
            at_word_start = true;
        }
    }
    out
}

/// Reports whether `byte` is an ASCII lower-case letter.
pub fn is_letter_lower(byte: u8) -> bool {
    utils::is_letter_lower(byte)
}

/// Reports whether `byte` is an ASCII upper-case letter.
pub fn is_letter_upper(byte: u8) -> bool {
    utils::is_letter_upper(byte)
}

/// Counts non-overlapping occurrences of `substr`.
pub fn count(s: &str, substr: &str) -> usize {
    s.matches(substr).count()
}

/// Counts occurrences of `substr`, case-insensitively.
pub fn count_i(s: &str, substr: &str) -> usize {
    count(&to_lower(s), &to_lower(substr))
}

/// Counts words separated by Unicode spaces.
pub fn count_words(s: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for word in s.split_whitespace() {
        *counts.entry(word.to_owned()).or_insert(0) += 1;
    }
    counts
}

/// Counts characters, optionally skipping spaces.
pub fn count_chars(s: &str, no_space: Option<bool>) -> HashMap<String, usize> {
    let skip_space = no_space.unwrap_or(false);
    let mut counts = HashMap::new();
    for ch in s.chars() {
        if skip_space && ch.is_whitespace() {
            continue;
        }
        *counts.entry(ch.to_string()).or_insert(0) += 1;
    }
    counts
}

/// Splits `s` at the first `delimiter` occurrences into two parts.
pub fn list2(s: &str, delimiter: &str) -> (String, String) {
    list2_from(split(s, delimiter), delimiter)
}

/// Splits and trims `s` into two parts.
pub fn list_and_trim2(s: &str, delimiter: &str) -> (String, String) {
    list2_from(split_and_trim(s, delimiter, None), delimiter)
}

fn list2_from(array: Vec<String>, delimiter: &str) -> (String, String) {
    match array.len() {
        0 => (String::new(), String::new()),
        1 => (array[0].clone(), String::new()),
        2 => (array[0].clone(), array[1].clone()),
        _ => (array[0].clone(), join(&array[1..], delimiter)),
    }
}

/// Splits `s` into three parts.
pub fn list3(s: &str, delimiter: &str) -> (String, String, String) {
    list3_from(split(s, delimiter), delimiter)
}

/// Splits and trims `s` into three parts.
pub fn list_and_trim3(s: &str, delimiter: &str) -> (String, String, String) {
    list3_from(split_and_trim(s, delimiter, None), delimiter)
}

fn list3_from(array: Vec<String>, delimiter: &str) -> (String, String, String) {
    match array.len() {
        0 => (String::new(), String::new(), String::new()),
        1 => (array[0].clone(), String::new(), String::new()),
        2 => (array[0].clone(), array[1].clone(), String::new()),
        3 => (array[0].clone(), array[1].clone(), array[2].clone()),
        _ => (
            array[0].clone(),
            array[1].clone(),
            join(&array[2..], delimiter),
        ),
    }
}

/// Splits `s` by `delimiter`; an empty delimiter splits into runes.
pub fn split(s: &str, delimiter: &str) -> Vec<String> {
    if delimiter.is_empty() {
        return s.chars().map(|ch| ch.to_string()).collect();
    }
    s.split(delimiter).map(str::to_owned).collect()
}

/// Splits `s`, trims every piece and drops the empty ones.
pub fn split_and_trim(s: &str, delimiter: &str, character_mask: Option<&str>) -> Vec<String> {
    split(s, delimiter)
        .into_iter()
        .map(|piece| trim(&piece, character_mask))
        .filter(|piece| !piece.is_empty())
        .collect()
}

/// Joins the pieces with `sep`.
pub fn join(array: &[String], sep: &str) -> String {
    array.join(sep)
}

/// Joins the string form of `value`'s elements with `sep`.
pub fn join_any(value: &Value, sep: &str) -> String {
    gconv::strings(value).join(sep)
}

/// Alias of [`split`].
pub fn explode(delimiter: &str, s: &str) -> Vec<String> {
    split(s, delimiter)
}

/// Alias of [`join`].
pub fn implode(glue: &str, pieces: &[String]) -> String {
    join(pieces, glue)
}

/// Splits `body` into rune chunks ending with `end` (default `\r\n`).
pub fn chunk_split(body: &str, chunk_len: usize, end: Option<&str>) -> String {
    let end = end.unwrap_or("\r\n");
    let runes: Vec<char> = body.chars().collect();
    if runes.len() <= 1 || runes.len() < chunk_len {
        return format!("{body}{end}");
    }
    let mut out = String::with_capacity(body.len() + end.len() * 4);
    let mut index = 0;
    while index < runes.len() {
        let take = chunk_len.min(runes.len() - index);
        out.extend(&runes[index..index + take]);
        out.push_str(end);
        index += take;
    }
    out
}

/// Splits on Unicode whitespace.
pub fn fields(s: &str) -> Vec<String> {
    s.split_whitespace().map(str::to_owned).collect()
}

/// Trims default whitespace plus `character_mask` from both ends.
pub fn trim(s: &str, character_mask: Option<&str>) -> String {
    utils::trim(s, character_mask)
}

/// Removes every leading `suffix` occurrence, like Go's `TrimLeftStr`.
pub fn trim_left_str(origin: &str, suffix: &str) -> String {
    if suffix.is_empty() {
        return origin.to_owned();
    }
    let mut result = origin;
    while let Some(rest) = result.strip_prefix(suffix) {
        result = rest;
    }
    result.to_owned()
}

/// Trims from the left.
pub fn trim_left(s: &str, character_mask: Option<&str>) -> String {
    s.trim_start_matches(|ch: char| {
        utils::DEFAULT_TRIM_CHARS.contains(&ch)
            || character_mask.is_some_and(|mask| mask.contains(ch))
    })
    .to_owned()
}

/// Trims from the right.
pub fn trim_right(s: &str, character_mask: Option<&str>) -> String {
    s.trim_end_matches(|ch: char| {
        utils::DEFAULT_TRIM_CHARS.contains(&ch)
            || character_mask.is_some_and(|mask| mask.contains(ch))
    })
    .to_owned()
}

/// Removes every character of `character_mask` from `s`.
pub fn trim_all(s: &str, character_mask: &str) -> String {
    s.chars()
        .filter(|ch| !character_mask.contains(*ch))
        .collect()
}

/// Returns the bytes of `s`.
pub fn bytes(s: &str) -> Vec<u8> {
    s.as_bytes().to_vec()
}

/// Returns the runes of `s`.
pub fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_and_positions() {
        assert_eq!(len("测试"), 6);
        assert_eq!(len_rune("测试"), 2);
        assert_eq!(pos("abcabc", "c"), 2);
        assert_eq!(pos_i("AbC", "bc"), 1);
        assert_eq!(pos("abc", "z"), NOT_FOUND_INDEX);
        assert!(contains_i("GoFrame", "frame"));
        assert!(contains_any("goframe", "xyzf"));
    }

    #[test]
    fn trim_left_str_removes_leading_occurrences() {
        // From GoFrame's own TrimLeftStr tests.
        assert_eq!(trim_left_str("prefix-body", "prefix"), "-body");
        assert_eq!(trim_left_str("aaabody", "a"), "body");
        assert_eq!(trim_left_str("body", "x"), "body");
        assert_eq!(trim_left_str("body", ""), "body");
    }

    #[test]
    fn case_functions() {
        assert_eq!(to_lower("GoFrame"), "goframe");
        assert_eq!(to_upper("GoFrame"), "GOFRAME");
        assert_eq!(uc_first("gf"), "Gf");
        assert_eq!(lc_first("Gf"), "gf");
        assert_eq!(uc_words("hello world HELLO"), "Hello World Hello");
        assert!(equal("GoFrame", "goframe"));
        assert_eq!(compare("a", "b"), -1);
    }

    #[test]
    fn split_join_and_lists() {
        assert_eq!(split("a,b", ","), vec!["a", "b"]);
        assert_eq!(split("ab", ""), vec!["a", "b"]);
        assert_eq!(
            split_and_trim(" 1, 2 ,, 3 ", ",", None),
            vec!["1", "2", "3"]
        );
        assert_eq!(join(&["a".to_owned(), "b".to_owned()], "-"), "a-b");
        assert_eq!(list2("a-b-c", "-"), ("a".to_owned(), "b-c".to_owned()));
        assert_eq!(
            list3("a-b-c-d", "-"),
            ("a".to_owned(), "b".to_owned(), "c-d".to_owned())
        );
        assert_eq!(list2("a", "-"), ("a".to_owned(), String::new()));
        assert_eq!(fields(" a  b\tc "), vec!["a", "b", "c"]);
        assert_eq!(chunk_split("abcdefg", 3, None), "abc\r\ndef\r\ng\r\n");
        assert_eq!(chunk_split("ab", 5, Some("\n")), "ab\n");
    }

    #[test]
    fn counting_and_trims() {
        assert_eq!(count("abcabc", "abc"), 2);
        assert_eq!(count_i("AbcABC", "abc"), 2);
        assert_eq!(count_words("a b a").get("a"), Some(&2));
        assert_eq!(count_chars("a a", Some(true)).get(" "), None);
        assert_eq!(trim("  gf  ", None), "gf");
        assert_eq!(trim_left("--gf--", Some("-")), "gf--");
        assert_eq!(trim_right("--gf--", Some("-")), "--gf");
        assert_eq!(trim_all("a-b-c", "-"), "abc");
    }

    #[test]
    fn slashes_and_arrays() {
        assert_eq!(add_slashes(r#"a'b"c\d"#), r#"a\'b\"c\\d"#);
        assert_eq!(strip_slashes(r"a\\b"), r"a\b");
        assert_eq!(quote_meta("a.b*c", None), r"a\.b\*c");
        assert_eq!(quote_meta("a-b", Some("-")), r"a\-b");

        let mut array = vec!["a".to_owned(), "b".to_owned()];
        assert_eq!(search_array(&array, "b"), 1);
        assert!(in_array(&array, "a"));
        prefix_array(&mut array, "x");
        assert_eq!(array, vec!["xa", "xb"]);
    }

    #[test]
    fn domains() {
        assert!(is_sub_domain("s.goframe.org", "goframe.org"));
        assert!(is_sub_domain("s.goframe.org", "*.org"));
        assert!(is_sub_domain("a.b.goframe.org", "goframe.org"));
        assert!(is_sub_domain("goframe.org:8080", "goframe.org"));
        assert!(!is_sub_domain("goframe.org", "abc.goframe.org"));
        assert!(!is_sub_domain("s.goframe.org", "google.com"));
        assert!(!is_sub_domain("s.goframe.org", "a.b.goframe.org"));
    }

    #[test]
    fn conversion_helpers() {
        assert_eq!(bytes("abc"), b"abc");
        assert_eq!(chars("ab"), vec!['a', 'b']);
        assert_eq!(repeat("ab", 3), "ababab");
        assert_eq!(
            join_any(&Value::List(vec![Value::Int(1), Value::Bool(true)]), ","),
            "1,true"
        );
    }
}
