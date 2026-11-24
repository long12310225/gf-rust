//! The remaining `text/gstr` families: positions with offsets, substring,
//! replace, case conversion, conversions, parsing, similarity and versions.

use std::sync::OnceLock;

use gf_core::Value;
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;
use indexmap::IndexMap;
use regex::Captures;
use regex::Regex;

use super::{NOT_FOUND_INDEX, count, equal, is_numeric, pos, to_lower, to_upper, trim, trim_left};

fn rune_count_prefix(haystack: &str, byte_index: isize) -> isize {
    if byte_index < 0 {
        return byte_index;
    }
    haystack
        .chars()
        .take_while({
            let mut consumed = 0usize;
            move |ch| {
                let start = consumed;
                consumed += ch.len_utf8();
                start < byte_index as usize
            }
        })
        .count() as isize
}

/// Returns the first match from `offset` (byte index, negative counts from end).
pub fn pos_offset(haystack: &str, needle: &str, offset: isize) -> isize {
    let length = haystack.len() as isize;
    if length == 0 || offset > length || -offset > length {
        return NOT_FOUND_INDEX;
    }
    let offset = if offset < 0 { offset + length } else { offset };
    match haystack[offset as usize..].find(needle) {
        Some(position) => position as isize + offset,
        None => NOT_FOUND_INDEX,
    }
}

/// Rune-index variant of [`pos_offset`].
pub fn pos_rune(haystack: &str, needle: &str, offset: isize) -> isize {
    let position = pos_offset(haystack, needle, offset);
    if position < 2 {
        return position;
    }
    rune_count_prefix(haystack, position)
}

/// Case-insensitive [`pos_offset`].
pub fn pos_i_offset(haystack: &str, needle: &str, offset: isize) -> isize {
    let length = haystack.len() as isize;
    if length == 0 || offset > length || -offset > length {
        return NOT_FOUND_INDEX;
    }
    let offset = if offset < 0 { offset + length } else { offset };
    let tail = &haystack[offset as usize..];
    let position = pos(&to_lower(tail), &to_lower(needle));
    if position == NOT_FOUND_INDEX {
        NOT_FOUND_INDEX
    } else {
        position + offset
    }
}

/// Rune-index variant of [`pos_i_offset`].
pub fn pos_i_rune(haystack: &str, needle: &str, offset: isize) -> isize {
    let position = pos_i_offset(haystack, needle, offset);
    if position < 2 {
        return position;
    }
    rune_count_prefix(haystack, position)
}

/// Returns the last match from `offset`.
pub fn pos_r(haystack: &str, needle: &str, offset: isize) -> isize {
    let length = haystack.len() as isize;
    if length == 0 || offset > length || -offset > length {
        return NOT_FOUND_INDEX;
    }
    let (slice, base) = if offset < 0 {
        (&haystack[..(offset + length + 1) as usize], 0)
    } else {
        (&haystack[offset as usize..], offset)
    };
    match slice.rfind(needle) {
        Some(position) => position as isize + if offset > 0 { base } else { 0 },
        None => NOT_FOUND_INDEX,
    }
}

/// Rune-index variant of [`pos_r`].
pub fn pos_r_rune(haystack: &str, needle: &str, offset: isize) -> isize {
    let position = pos_r(haystack, needle, offset);
    if position < 2 {
        return position;
    }
    rune_count_prefix(haystack, position)
}

/// Case-insensitive [`pos_r`].
pub fn pos_ri(haystack: &str, needle: &str, offset: isize) -> isize {
    let length = haystack.len() as isize;
    if length == 0 || offset > length || -offset > length {
        return NOT_FOUND_INDEX;
    }
    let (slice, base) = if offset < 0 {
        (&haystack[..(offset + length + 1) as usize], 0)
    } else {
        (&haystack[offset as usize..], offset)
    };
    let position = pos_r(&to_lower(slice), &to_lower(needle), 0);
    if position == NOT_FOUND_INDEX {
        NOT_FOUND_INDEX
    } else {
        position + if offset > 0 { base } else { 0 }
    }
}

/// Rune-index variant of [`pos_ri`].
pub fn pos_ri_rune(haystack: &str, needle: &str, offset: isize) -> isize {
    let position = pos_ri(haystack, needle, offset);
    if position < 2 {
        return position;
    }
    rune_count_prefix(haystack, position)
}

/// Returns the part from the first occurrence of `needle` (including its last
/// byte), like Go's `Str`.
pub fn str_part(haystack: &str, needle: &str) -> String {
    if needle.is_empty() {
        return String::new();
    }
    match haystack.find(needle) {
        Some(position) => haystack[position + needle.len() - 1..].to_owned(),
        None => String::new(),
    }
}

/// Like [`str_part`] but dropping the first byte of the result.
pub fn str_part_ex(haystack: &str, needle: &str) -> String {
    let part = str_part(haystack, needle);
    if part.is_empty() {
        String::new()
    } else {
        part[1..].to_owned()
    }
}

/// Returns the prefix ending with the first occurrence of `needle`.
pub fn str_till(haystack: &str, needle: &str) -> String {
    match haystack.find(needle) {
        Some(position) => haystack[..position + needle.len()].to_owned(),
        None => String::new(),
    }
}

/// Like [`str_till`] but excluding `needle`.
pub fn str_till_ex(haystack: &str, needle: &str) -> String {
    match haystack.find(needle) {
        Some(position) if position != 0 => haystack[..position].to_owned(),
        _ => String::new(),
    }
}

/// Byte-based substring with Go's negative start/length rules.
pub fn sub_str(s: &str, start: isize, length: Option<isize>) -> String {
    let length_total = s.len() as isize;
    if start < 0 {
        if -start > length_total {
            return sub_str(s, 0, Some(0));
        }
        return sub_str(s, length_total + start, length);
    }
    if start > length_total {
        return String::new();
    }
    let mut real_length = match length {
        Some(value) if value < 0 => {
            if -value > length_total - start {
                0
            } else {
                length_total - start + value
            }
        }
        Some(value) => value.min(length_total - start),
        None => length_total - start,
    };
    if real_length < 0 {
        real_length = 0;
    }
    if real_length == length_total {
        return s.to_owned();
    }
    let end = start + real_length;
    s[start as usize..end as usize].to_owned()
}

/// Rune-based substring with Go's negative start/length rules.
pub fn sub_str_rune(s: &str, start: isize, length: Option<isize>) -> String {
    let runes: Vec<char> = s.chars().collect();
    let total = runes.len() as isize;
    if start < 0 {
        if -start > total {
            return runes[..0].iter().collect();
        }
        return sub_str_rune(s, total + start, length);
    }
    if start > total {
        return String::new();
    }
    let real_length = match length {
        Some(value) if value < 0 => {
            if -value > total - start {
                0
            } else {
                total - start + value
            }
        }
        Some(value) => value.min(total - start),
        None => total - start,
    } as usize;
    runes[start as usize..start as usize + real_length]
        .iter()
        .collect()
}

/// Truncates `s` to `length` bytes with a suffix.
pub fn str_limit(s: &str, length: usize, suffix: Option<&str>) -> String {
    if s.len() <= length {
        return s.to_owned();
    }
    format!("{}{}", &s[..length], suffix.unwrap_or("..."))
}

/// Truncates `s` to `length` runes with a suffix.
pub fn str_limit_rune(s: &str, length: usize, suffix: Option<&str>) -> String {
    let runes: Vec<char> = s.chars().collect();
    if runes.len() <= length {
        return s.to_owned();
    }
    let prefix: String = runes[..length].iter().collect();
    format!("{}{}", prefix, suffix.unwrap_or("..."))
}

/// Returns the suffix from the first occurrence of `need` (inclusive).
pub fn sub_str_from(s: &str, need: &str) -> String {
    match s.find(need) {
        Some(position) => s[position..].to_owned(),
        None => String::new(),
    }
}

/// Returns the suffix from the first occurrence of `need` (exclusive).
pub fn sub_str_from_ex(s: &str, need: &str) -> String {
    match s.find(need) {
        Some(position) => s[position + need.len()..].to_owned(),
        None => String::new(),
    }
}

/// Returns the suffix from the last occurrence of `need` (inclusive).
pub fn sub_str_from_r(s: &str, need: &str) -> String {
    match s.rfind(need) {
        Some(position) => s[position..].to_owned(),
        None => String::new(),
    }
}

/// Returns the suffix from the last occurrence of `need` (exclusive).
pub fn sub_str_from_r_ex(s: &str, need: &str) -> String {
    match s.rfind(need) {
        Some(position) => s[position + need.len()..].to_owned(),
        None => String::new(),
    }
}

/// Replaces up to `count` non-overlapping occurrences (`None` means all).
pub fn replace(origin: &str, search: &str, replace: &str, count: Option<usize>) -> String {
    match count {
        Some(count) => origin.replacen(search, replace, count),
        None => origin.replace(search, replace),
    }
}

/// Case-insensitive replacement with the same `count` semantics.
pub fn replace_i(origin: &str, search: &str, replacement: &str, count: Option<usize>) -> String {
    let mut limit = count.unwrap_or(usize::MAX);
    if limit == 0 || search.is_empty() {
        return origin.to_owned();
    }
    let search_lower = to_lower(search);
    let mut result = String::with_capacity(origin.len());
    let mut rest = origin;
    loop {
        let origin_lower = to_lower(rest);
        let position = pos(&origin_lower, &search_lower);
        if position == NOT_FOUND_INDEX || limit == 0 {
            result.push_str(rest);
            break;
        }
        result.push_str(&rest[..position as usize]);
        result.push_str(replacement);
        rest = &rest[position as usize + search.len()..];
        limit = limit.saturating_sub(1);
    }
    result
}

/// Replaces search/replace pairs in order.
pub fn replace_by_array(origin: &str, array: &[String]) -> String {
    let mut result = origin.to_owned();
    for pair in array.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        result = replace(&result, &pair[0], &pair[1], None);
    }
    result
}

/// Case-insensitive [`replace_by_array`].
pub fn replace_i_by_array(origin: &str, array: &[String]) -> String {
    let mut result = origin.to_owned();
    for pair in array.chunks(2) {
        if pair.len() < 2 {
            break;
        }
        result = replace_i(&result, &pair[0], &pair[1], None);
    }
    result
}

/// Replaces every key of `replaces` in map order.
pub fn replace_by_map(origin: &str, replaces: &IndexMap<String, String>) -> String {
    let mut result = origin.to_owned();
    for (key, value) in replaces {
        result = replace(&result, key, value, None);
    }
    result
}

/// Case-insensitive [`replace_by_map`].
pub fn replace_i_by_map(origin: &str, replaces: &IndexMap<String, String>) -> String {
    let mut result = origin.to_owned();
    for (key, value) in replaces {
        result = replace_i(&result, key, value, None);
    }
    result
}

/// Replaces every occurrence with the result of `func`.
pub fn replace_func(origin: &str, search: &str, mut func: impl FnMut(&str) -> String) -> String {
    if search.is_empty() || search.len() > origin.len() {
        return origin.to_owned();
    }
    let mut result = String::with_capacity(origin.len());
    let mut last = 0;
    let mut current = 0;
    while current < origin.len() {
        let Some(found) = origin[current..].find(search) else {
            break;
        };
        let position = current + found;
        result.push_str(&origin[last..position]);
        result.push_str(&func(&origin[position..position + search.len()]));
        last = position + search.len();
        current = last;
    }
    result.push_str(&origin[last..]);
    result
}

/// Case-insensitive [`replace_func`].
pub fn replace_i_func(origin: &str, search: &str, mut func: impl FnMut(&str) -> String) -> String {
    if search.is_empty() || search.len() > origin.len() {
        return origin.to_owned();
    }
    let mut result = String::with_capacity(origin.len());
    let mut last = 0;
    let mut current = 0;
    while current < origin.len() {
        let position = pos_i_offset(origin, search, current as isize);
        if position == NOT_FOUND_INDEX {
            break;
        }
        let position = position as usize;
        result.push_str(&origin[last..position]);
        result.push_str(&func(&origin[position..position + search.len()]));
        last = position + search.len();
        current = last;
    }
    result.push_str(&origin[last..]);
    result
}

/// Naming convention family, like Go's `CaseType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseType {
    /// `CamelCase`.
    Camel,
    /// `camelCase`.
    CamelLower,
    /// `snake_case`.
    Snake,
    /// `snake_Case`.
    SnakeFirstUpper,
    /// `SNAKE_CASE`.
    SnakeScreaming,
    /// `kebab-case`.
    Kebab,
    /// `KEBAB-CASE`.
    KebabScreaming,
    /// lower case.
    Lower,
}

impl CaseType {
    /// The Go string form.
    pub fn as_str(self) -> &'static str {
        match self {
            CaseType::Camel => "Camel",
            CaseType::CamelLower => "CamelLower",
            CaseType::Snake => "Snake",
            CaseType::SnakeFirstUpper => "SnakeFirstUpper",
            CaseType::SnakeScreaming => "SnakeScreaming",
            CaseType::Kebab => "Kebab",
            CaseType::KebabScreaming => "KebabScreaming",
            CaseType::Lower => "Lower",
        }
    }
}

/// Matches a case name case-insensitively.
pub fn case_type_match(name: &str) -> Option<CaseType> {
    [
        CaseType::Camel,
        CaseType::CamelLower,
        CaseType::Snake,
        CaseType::SnakeFirstUpper,
        CaseType::SnakeScreaming,
        CaseType::Kebab,
        CaseType::KebabScreaming,
        CaseType::Lower,
    ]
    .into_iter()
    .find(|case| equal(name, case.as_str()))
}

/// Converts `s` to the requested case.
pub fn case_convert(s: &str, case: CaseType) -> String {
    if s.is_empty() {
        return s.to_owned();
    }
    match case {
        CaseType::Camel => case_camel(s),
        CaseType::CamelLower => case_camel_lower(s),
        CaseType::Snake => case_snake(s),
        CaseType::SnakeFirstUpper => case_snake_first_upper(s, None),
        CaseType::SnakeScreaming => case_snake_screaming(s),
        CaseType::Kebab => case_kebab(s),
        CaseType::KebabScreaming => case_kebab_screaming(s),
        CaseType::Lower => to_lower(s),
    }
}

fn number_sequence() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"([a-zA-Z]{0,1})([0-9]+)([a-zA-Z]{0,1})")
            .unwrap_or_else(|err| panic!("case number regex is invalid: {err}"))
    })
}

fn add_word_boundaries_to_numbers(s: &str) -> String {
    number_sequence()
        .replace_all(s, |captures: &Captures<'_>| {
            let mut out = String::new();
            if let Some(prefix) = captures.get(1)
                && !prefix.as_str().is_empty()
            {
                out.push_str(prefix.as_str());
                out.push(' ');
            }
            out.push_str(captures.get(2).map_or("", |m| m.as_str()));
            if let Some(suffix) = captures.get(3)
                && !suffix.as_str().is_empty()
            {
                out.push(' ');
                out.push_str(suffix.as_str());
            }
            out
        })
        .into_owned()
}

fn to_camel_init_case(s: &str, init_case: bool) -> String {
    let s = add_word_boundaries_to_numbers(s);
    let s = s.trim_matches(' ');
    let mut out = String::with_capacity(s.len());
    let mut capitalize_next = init_case;
    for ch in s.chars() {
        if ch.is_ascii_uppercase() || ch.is_ascii_digit() {
            out.push(ch);
        } else if ch.is_ascii_lowercase() {
            if capitalize_next {
                out.push(ch.to_ascii_uppercase());
            } else {
                out.push(ch);
            }
        }
        capitalize_next = matches!(ch, '_' | ' ' | '-' | '.');
    }
    out
}

/// Converts to `CamelCase`.
pub fn case_camel(s: &str) -> String {
    to_camel_init_case(s, true)
}

/// Converts to `camelCase`.
pub fn case_camel_lower(s: &str) -> String {
    let lower = match s.chars().next() {
        Some(ch) if ch.is_ascii_uppercase() => {
            let mut out = String::with_capacity(s.len());
            out.push(ch.to_ascii_lowercase());
            out.push_str(&s[ch.len_utf8()..]);
            out
        }
        _ => s.to_owned(),
    };
    to_camel_init_case(&lower, false)
}

/// Converts to `snake_case`.
pub fn case_snake(s: &str) -> String {
    case_delimited(s, b'_')
}

/// Converts to `SNAKE_CASE`.
pub fn case_snake_screaming(s: &str) -> String {
    case_delimited_screaming(s, b'_', true)
}

/// Converts to `kebab-case`.
pub fn case_kebab(s: &str) -> String {
    case_delimited(s, b'-')
}

/// Converts to `KEBAB-CASE`.
pub fn case_kebab_screaming(s: &str) -> String {
    case_delimited_screaming(s, b'-', true)
}

/// Converts to a delimited case.
pub fn case_delimited(s: &str, delimiter: u8) -> String {
    case_delimited_screaming(s, delimiter, false)
}

/// Converts to a delimited case, optionally screaming.
pub fn case_delimited_screaming(s: &str, delimiter: u8, screaming: bool) -> String {
    let s = add_word_boundaries_to_numbers(s);
    let s = s.trim_matches(' ');
    let delimiter = char::from(delimiter);
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    for (index, &ch) in chars.iter().enumerate() {
        let next_case_changed = chars.get(index + 1).is_some_and(|&next| {
            (ch.is_ascii_uppercase() && next.is_ascii_lowercase())
                || (ch.is_ascii_lowercase() && next.is_ascii_uppercase())
        });
        if index > 0 && !out.ends_with(delimiter) && next_case_changed {
            if ch.is_ascii_uppercase() {
                out.push(delimiter);
                out.push(ch);
            } else if ch.is_ascii_lowercase() {
                out.push(ch);
                out.push(delimiter);
            }
        } else if matches!(ch, ' ' | '_' | '-' | '.') {
            out.push(delimiter);
        } else {
            out.push(ch);
        }
    }
    if screaming {
        to_upper(&out)
    } else {
        to_lower(&out)
    }
}

fn first_camel_start() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"([A-Z]+)([A-Z]?[_a-z0-9]+)")
            .unwrap_or_else(|err| panic!("case start regex is invalid: {err}"))
    })
}

fn first_camel_end() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r"(?s)^([\s\S]*?)(_?[A-Z]+)$")
            .unwrap_or_else(|err| panic!("case end regex is invalid: {err}"))
    })
}

/// Converts `RGBCodeMd5` to `rgb_code_md5`.
pub fn case_snake_first_upper(word: &str, underscore: Option<&str>) -> String {
    let replace = underscore.unwrap_or("_");
    let mut word = word.to_owned();
    if let Some(captures) = first_camel_end().captures(&word) {
        let head = captures.get(1).map_or("", |m| m.as_str());
        let tail = captures.get(2).map_or("", |m| m.as_str());
        let tail = trim_left(&to_lower(tail), Some(replace));
        word = format!("{head}{replace}{tail}");
    }
    while let Some(captures) = first_camel_start().captures(&word) {
        let group = captures.get(1).map_or("", |m| m.as_str());
        if group.is_empty() {
            break;
        }
        let lower = to_lower(group);
        let mut replacement = String::with_capacity(lower.len() + replace.len());
        replacement.push_str(&lower[..lower.len() - 1]);
        replacement.push_str(replace);
        replacement.push_str(&lower[lower.len() - 1..]);
        word = word.replacen(group, &replacement, 1);
    }
    trim_left(&word, Some(replace))
}

/// Returns the character for `ascii % 256`.
pub fn chr(ascii: i64) -> String {
    let byte = (ascii % 256) as u8;
    String::from_utf8_lossy(&[byte]).into_owned()
}

/// Returns the first byte value of `char`; panics on an empty string.
pub fn ord(text: &str) -> u8 {
    text.as_bytes()
        .first()
        .copied()
        .unwrap_or_else(|| panic!("ord: cannot be an empty string"))
}

/// Converts octal escapes such as `\346\200\241` to their bytes.
pub fn oct_str(s: &str) -> String {
    let mut bytes = Vec::with_capacity(s.len());
    let mut index = 0;
    let chars: Vec<char> = s.chars().collect();
    while index < chars.len() {
        if chars[index] == '\\'
            && index + 3 < chars.len()
            && chars[index + 1].is_ascii_digit()
            && chars[index + 2].is_ascii_digit()
            && chars[index + 3].is_ascii_digit()
        {
            let digits: String = chars[index + 1..=index + 3].iter().collect();
            if digits.chars().all(|ch| ('0'..='7').contains(&ch))
                && let Ok(value) = u8::from_str_radix(&digits, 8)
            {
                bytes.push(value);
                index += 4;
                continue;
            }
        }
        let mut buffer = [0u8; 4];
        bytes.extend_from_slice(chars[index].encode_utf8(&mut buffer).as_bytes());
        index += 1;
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Reverses the runes of `s`.
pub fn reverse(s: &str) -> String {
    s.chars().rev().collect()
}

/// Formats a number with grouped thousands.
pub fn number_format(number: f64, decimals: usize, dec_point: &str, thousands_sep: &str) -> String {
    let negative = number < 0.0;
    let number = number.abs();
    let formatted = format!("{number:.decimals$}");
    let (prefix, suffix) = if decimals > 0 {
        let split = formatted.len() - decimals - 1;
        (&formatted[..split], &formatted[split + 1..])
    } else {
        (formatted.as_str(), "")
    };
    let separator_length = thousands_sep.len();
    let groups = (prefix.len() - 1) / 3;
    let mut buffer = vec![0u8; prefix.len() + separator_length * groups];
    let mut position = buffer.len() as isize - 1;
    for (count, byte) in prefix.bytes().rev().enumerate() {
        if separator_length > 0 && count > 0 && count.is_multiple_of(3) {
            for sep in thousands_sep.bytes().rev() {
                buffer[position as usize] = sep;
                position -= 1;
            }
        }
        buffer[position as usize] = byte;
        position -= 1;
    }
    let mut out = String::from_utf8_lossy(&buffer).into_owned();
    if decimals > 0 {
        out.push_str(dec_point);
        out.push_str(suffix);
    }
    if negative {
        out.insert(0, '-');
    }
    out
}

/// Shuffles the runes of `s`.
pub fn shuffle(s: &str) -> String {
    let runes: Vec<char> = s.chars().collect();
    let permutation = gf_util::grand::perm(runes.len());
    let mut out = String::with_capacity(s.len());
    for index in permutation {
        out.push(runes[index as usize]);
    }
    out
}

/// Hides `percent` of the string from the middle with `hide`.
pub fn hide_str(s: &str, percent: i64, hide: &str) -> String {
    let mut suffix = String::new();
    let mut text = s;
    if let Some(index) = text.find('@') {
        suffix = text[index..].to_owned();
        text = &text[..index];
    }
    if text.is_empty() || percent <= 0 {
        return format!("{text}{suffix}");
    }
    let runes: Vec<char> = text.chars().collect();
    if percent >= 100 {
        return format!("{}{suffix}", hide.repeat(runes.len()));
    }
    let length = runes.len();
    let hide_len = length * percent as usize / 100;
    if hide_len == 0 {
        return format!("{text}{suffix}");
    }
    let middle = length / 2;
    let mut start = middle.saturating_sub(hide_len / 2);
    let mut end = start + hide_len;
    if end > length {
        end = length;
        start = length.saturating_sub(hide_len);
    }
    let mut out: String = runes[..start].iter().collect();
    out.push_str(&hide.repeat(hide_len));
    out.extend(&runes[end..]);
    out.push_str(&suffix);
    out
}

/// Converts newlines to `<br>` (or `<br />`).
pub fn nl2br(s: &str, xhtml: Option<bool>) -> String {
    let br = if xhtml.unwrap_or(false) {
        "<br />"
    } else {
        "<br>"
    };
    let runes: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut skip = false;
    for (index, &ch) in runes.iter().enumerate() {
        if skip {
            skip = false;
            continue;
        }
        match ch {
            '\n' | '\r' => {
                if let Some(&next) = runes.get(index + 1)
                    && ((ch == '\r' && next == '\n') || (ch == '\n' && next == '\r'))
                {
                    out.push_str(br);
                    skip = true;
                    continue;
                }
                out.push_str(br);
            }
            _ => out.push(ch),
        }
    }
    out
}

fn is_punctuation(ch: char) -> bool {
    matches!(
        ch,
        ';' | '.' | ',' | ':' | '~' | '；' | '，' | '。' | '：' | '？' | '！' | '…' | '、'
    )
}

/// Wraps `s` at `width` units using `br` (default `\n`).
pub fn word_wrap(s: &str, width: usize, br: &str) -> String {
    let br = if br.is_empty() { "\n" } else { br };
    let mut current = 0usize;
    let mut word = String::new();
    let mut space = String::new();
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == '\n' {
            if word.is_empty() {
                if current + space.len() <= width {
                    out.push_str(&space);
                }
                space.clear();
            } else {
                out.push_str(&space);
                space.clear();
                out.push_str(&word);
                word.clear();
            }
            out.push(ch);
            current = 0;
        } else if ch.is_whitespace() {
            if space.is_empty() || !word.is_empty() {
                current += space.len() + word.len();
                out.push_str(&space);
                space.clear();
                out.push_str(&word);
                word.clear();
            }
            space.push(ch);
        } else if is_punctuation(ch) {
            word.push(ch);
            if space.is_empty() || !word.is_empty() {
                current += space.len() + word.len();
                out.push_str(&space);
                space.clear();
                out.push_str(&word);
                word.clear();
            }
        } else {
            word.push(ch);
            if current + space.len() + word.len() > width && word.len() < width {
                out.push_str(br);
                current = 0;
                space.clear();
            }
        }
    }
    if word.is_empty() {
        if current + space.len() <= width {
            out.push_str(&space);
        }
    } else {
        out.push_str(&space);
        out.push_str(&word);
    }
    out
}

fn unescape(s: &str) -> Result<String, gerror::Error> {
    if !s
        .as_bytes()
        .iter()
        .any(|&byte| byte == b'%' || byte == b'+')
    {
        return Ok(s.to_owned());
    }
    let bytes = gf_encoding::gurl::decode_bytes(s).map_err(|err| {
        match gerror::wrap(
            Some(err),
            format!("url.QueryUnescape failed for string \"{s}\""),
        ) {
            Some(err) => err,
            None => gerror::Error::new(format!("url.QueryUnescape failed for string \"{s}\"")),
        }
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Parses an ampersand form string into nested maps and lists.
pub fn parse(s: &str) -> Result<Value, gerror::Error> {
    if s.is_empty() {
        return Ok(Value::Null);
    }
    let mut result = IndexMap::new();
    for part in s.split('&') {
        let Some(position) = part.find('=') else {
            continue;
        };
        if position == 0 {
            continue;
        }
        let mut key = unescape(&part[..position])?;
        while key.starts_with(' ') {
            key.remove(0);
        }
        if key.is_empty() || key.starts_with('[') {
            continue;
        }
        let value = unescape(&part[position + 1..])?;
        let mut keys: Vec<String> = Vec::new();
        let chars: Vec<char> = key.chars().collect();
        let mut left = 0usize;
        let mut index = 0usize;
        while index < chars.len() {
            let ch = chars[index];
            if ch == '[' && left == 0 {
                left = index;
            } else if ch == ']' && left > 0 {
                if keys.is_empty() {
                    keys.push(chars[..left].iter().collect());
                }
                keys.push(chars[left + 1..index].iter().collect());
                left = 0;
                if index + 1 < chars.len() && chars[index + 1] != '[' {
                    break;
                }
            }
            index += 1;
        }
        if keys.is_empty() {
            keys.push(key.clone());
        }
        let mut first = String::new();
        let first_key: Vec<char> = keys[0].chars().collect();
        for (position, ch) in first_key.iter().enumerate() {
            if matches!(ch, ' ' | '.' | '[') {
                first.push('_');
            } else {
                first.push(*ch);
            }
            if *ch == '[' {
                first.extend(&first_key[position + 1..]);
                break;
            }
        }
        keys[0] = first;
        build(&mut result, &keys, Value::String(value))?;
    }
    Ok(Value::Map(result))
}

fn build(
    result: &mut IndexMap<String, Value>,
    keys: &[String],
    value: Value,
) -> Result<(), gerror::Error> {
    let length = keys.len();
    let key = keys[0]
        .trim_matches(|ch| ch == '\'' || ch == '"')
        .to_owned();
    if length == 1 {
        result.insert(key, value);
        return Ok(());
    }
    if keys[1].is_empty() && length == 2 {
        if key.is_empty() {
            return Ok(());
        }
        match result.get_mut(&key) {
            None => {
                result.insert(key, Value::List(vec![value]));
            }
            Some(Value::List(items)) => items.push(value),
            Some(other) => {
                return Err(gerror::Error::new_codef(
                    &CODE_INVALID_PARAMETER,
                    "expected type '[]any' for key '%s', but got '%s'",
                    vec![
                        Value::String(key),
                        Value::String(other.go_type_name().to_owned()),
                    ],
                ));
            }
        }
        return Ok(());
    }
    if keys[1].is_empty() && length > 2 && !keys[2].is_empty() {
        let entry = result
            .entry(key.clone())
            .or_insert_with(|| Value::List(Vec::new()));
        let Value::List(items) = entry else {
            return Err(gerror::Error::new_codef(
                &CODE_INVALID_PARAMETER,
                "expected type '[]any' for key '%s', but got '%s'",
                vec![
                    Value::String(key),
                    Value::String(entry.go_type_name().to_owned()),
                ],
            ));
        };
        if let Some(Value::Map(child)) = items.last_mut()
            && !child.contains_key(&keys[2])
        {
            build(child, &keys[2..], value)?;
            return Ok(());
        }
        let mut child = IndexMap::new();
        build(&mut child, &keys[2..], value)?;
        items.push(Value::Map(child));
        return Ok(());
    }
    let entry = result
        .entry(key.clone())
        .or_insert_with(|| Value::Map(IndexMap::new()));
    let Value::Map(children) = entry else {
        return Err(gerror::Error::new_codef(
            &CODE_INVALID_PARAMETER,
            "expected type 'map[string]any' for key '%s', but got '%s'",
            vec![
                Value::String(key),
                Value::String(entry.go_type_name().to_owned()),
            ],
        ));
    };
    build(children, &keys[1..], value)
}

/// Levenshtein distance with explicit costs; -1 above 255 bytes.
pub fn levenshtein(first: &str, second: &str, cost_ins: i32, cost_rep: i32, cost_del: i32) -> i32 {
    let a = first.as_bytes();
    let b = second.as_bytes();
    if a.is_empty() {
        return b.len() as i32 * cost_ins;
    }
    if b.is_empty() {
        return a.len() as i32 * cost_del;
    }
    if a.len() > 255 || b.len() > 255 {
        return -1;
    }
    let mut previous: Vec<i32> = (0..=b.len()).map(|index| index as i32 * cost_ins).collect();
    let mut current = vec![0i32; b.len() + 1];
    for &left in a {
        current[0] = previous[0] + cost_del;
        for (offset, &right) in b.iter().enumerate() {
            let mut cost = if left == right {
                previous[offset]
            } else {
                previous[offset] + cost_rep
            };
            cost = cost.min(previous[offset + 1] + cost_del);
            cost = cost.min(current[offset] + cost_ins);
            current[offset + 1] = cost;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[b.len()]
}

fn similar_bytes(first: &[u8], second: &[u8]) -> usize {
    let len1 = first.len();
    let len2 = second.len();
    let mut max = 0usize;
    let mut pos1 = 0usize;
    let mut pos2 = 0usize;
    for i in 0..len1 {
        for j in 0..len2 {
            let mut l = 0usize;
            while i + l < len1 && j + l < len2 && first[i + l] == second[j + l] {
                if l + 1 > max {
                    max = l + 1;
                    pos1 = i;
                    pos2 = j;
                }
                l += 1;
            }
        }
    }
    let mut sum = max;
    if sum > 0 {
        if pos1 > 0 && pos2 > 0 {
            sum += similar_bytes(&first[..pos1], &second[..pos2]);
        }
        if pos1 + max < len1 && pos2 + max < len2 {
            sum += similar_bytes(&first[pos1 + max..], &second[pos2 + max..]);
        }
    }
    sum
}

/// PHP-style similar_text; returns the match length and the percentage.
pub fn similar_text(first: &str, second: &str) -> (usize, f64) {
    let a = first.as_bytes();
    let b = second.as_bytes();
    if a.len() + b.len() == 0 {
        return (0, 0.0);
    }
    let similar = similar_bytes(a, b);
    let percent = similar as f64 * 200.0 / (a.len() + b.len()) as f64;
    (similar, percent)
}

/// Soundex key of `s`; panics on an empty string.
pub fn soundex(s: &str) -> String {
    if s.is_empty() {
        panic!("str: cannot be an empty string");
    }
    const TABLE: &[u8] = b"01230120022455012623010202";
    let mut last: i32 = -1;
    let mut small = 0usize;
    let mut out = [b'0'; 4];
    for &byte in s.as_bytes() {
        if small >= 4 {
            break;
        }
        let code = if byte < 0x7F && byte.is_ascii_lowercase() {
            byte - b'a' + b'A'
        } else {
            byte
        };
        if code.is_ascii_uppercase() {
            if small == 0 {
                out[small] = code;
                small += 1;
                last = i32::from(TABLE[usize::from(code - b'A')]);
            } else {
                let value = i32::from(TABLE[usize::from(code - b'A')]);
                if value != last {
                    if value != i32::from(b'0') {
                        out[small] = value as u8;
                        small += 1;
                    }
                    last = value;
                }
            }
        }
    }
    for item in out.iter_mut().skip(small) {
        *item = b'0';
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Reports whether `version` is a GNU-style version.
pub fn is_gnu_version(version: &str) -> bool {
    let version = version
        .strip_prefix('v')
        .or_else(|| version.strip_prefix('V'))
        .unwrap_or(version);
    if version.is_empty() {
        return false;
    }
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() > 3 {
        return false;
    }
    parts.iter().all(|part| {
        !part.is_empty() && is_numeric(part) && !part.starts_with('-') && !part.starts_with('+')
    })
}

/// Compares two GNU versions.
pub fn compare_version(a: &str, b: &str) -> i32 {
    let a = a.strip_prefix('v').unwrap_or(a);
    let b = b.strip_prefix('v').unwrap_or(b);
    let mut a_parts: Vec<i64> = a.split('.').map(gconv_value).collect();
    let mut b_parts: Vec<i64> = b.split('.').map(gconv_value).collect();
    while a_parts.len() < b_parts.len() {
        a_parts.push(0);
    }
    while b_parts.len() < a_parts.len() {
        b_parts.push(0);
    }
    for (left, right) in a_parts.iter().zip(&b_parts) {
        if left > right {
            return 1;
        }
        if left < right {
            return -1;
        }
    }
    0
}

/// Compares two Go module versions.
pub fn compare_version_go(a: &str, b: &str) -> i32 {
    let a = trim(a, None);
    let b = trim(b, None);
    let a = a.strip_prefix('v').unwrap_or(&a);
    let b = b.strip_prefix('v').unwrap_or(&b);
    let raw_a = a.to_owned();
    let raw_b = b.to_owned();
    let mut a = a.to_owned();
    let mut b = b.to_owned();
    if count(&a, "-") > 1
        && let Some(index) = a.rfind('-')
    {
        a.truncate(index);
    }
    if count(&b, "-") > 1
        && let Some(index) = b.rfind('-')
    {
        b.truncate(index);
    }
    if let Some(index) = a.find('+') {
        a.truncate(index);
    }
    if let Some(index) = b.find('+') {
        b.truncate(index);
    }
    let a = replace(&a, "-", ".", None);
    let b = replace(&b, "-", ".", None);
    let mut a_parts: Vec<i64> = a.split('.').map(gconv_value).collect();
    let mut b_parts: Vec<i64> = b.split('.').map(gconv_value).collect();
    while a_parts.len() < b_parts.len() {
        a_parts.push(0);
    }
    while b_parts.len() < a_parts.len() {
        b_parts.push(0);
    }
    for (index, (left, right)) in a_parts.iter().zip(&b_parts).enumerate() {
        if index == 4 && left != right && (*left == 0 || *right == 0) {
            return if left > right { -1 } else { 1 };
        }
        if left > right {
            return 1;
        }
        if left < right {
            return -1;
        }
    }
    let in_a = raw_a.contains("+incompatible");
    let in_b = raw_b.contains("+incompatible");
    if in_a && !in_b {
        return -1;
    }
    if !in_a && in_b {
        return 1;
    }
    0
}

fn gconv_value(text: &str) -> i64 {
    gf_util::gconv::int64_value(&Value::String(text.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positions_with_offsets() {
        assert_eq!(pos_offset("abcabc", "b", 0), 1);
        assert_eq!(pos_offset("abcabc", "b", 2), 4);
        assert_eq!(pos_offset("abcabc", "b", -3), 4);
        assert_eq!(pos_r("abcabc", "b", 0), 4);
        assert_eq!(pos_i_offset("AbC", "bc", 0), 1);
        assert_eq!(pos_ri("AbCAbC", "BC", 0), 4);
        assert_eq!(pos_rune("测试abc", "abc", 0), 2);
    }

    #[test]
    fn substring_family() {
        assert_eq!(str_part("av.mp4", "."), ".mp4");
        assert_eq!(str_part_ex("av.mp4", "."), "mp4");
        assert_eq!(str_till("av.mp4", "."), "av.");
        assert_eq!(str_till_ex("av.mp4", "."), "av");
        assert_eq!(sub_str("123456", 1, Some(2)), "23");
        assert_eq!(sub_str("123456", -3, Some(2)), "45");
        assert_eq!(sub_str_rune("一起学习吧！", 2, Some(2)), "学习");
        assert_eq!(str_limit("123456", 3, None), "123...");
        assert_eq!(str_limit_rune("一起学习吧！", 2, Some("~")), "一起~");
        assert_eq!(sub_str_from("av.mp4", "."), ".mp4");
        assert_eq!(sub_str_from_ex("av.mp4", "."), "mp4");
        assert_eq!(sub_str_from_r("/dev/vda", "/"), "/vda");
        assert_eq!(sub_str_from_r_ex("/dev/vda", "/"), "vda");
    }

    #[test]
    fn replace_family() {
        assert_eq!(replace("hello", "l", "L", Some(1)), "heLlo");
        assert_eq!(replace_i("GoFrame", "frame", "X", None), "GoX");
        assert_eq!(
            replace_by_array(
                "abc",
                &[
                    "a".to_owned(),
                    "1".to_owned(),
                    "b".to_owned(),
                    "2".to_owned()
                ]
            ),
            "12c"
        );
        let mut map = IndexMap::new();
        map.insert("a".to_owned(), "1".to_owned());
        assert_eq!(replace_by_map("abc", &map), "1bc");
        assert_eq!(
            replace_func("a-b-c", "-", |matched| matched.to_owned() + "!"),
            "a-!b-!c"
        );
        assert_eq!(
            replace_i_func("A-b", "a", |matched| matched.to_uppercase()),
            "A-b"
        );
    }

    #[test]
    fn case_family() {
        assert_eq!(case_type_match("snake"), Some(CaseType::Snake));
        assert_eq!(case_type_match("unknown"), None);
        assert_eq!(case_camel("any_kind_of_string"), "AnyKindOfString");
        assert_eq!(case_camel_lower("AnyKindOfString"), "anyKindOfString");
        assert_eq!(case_snake("AnyKindOfString"), "any_kind_of_string");
        assert_eq!(
            case_snake_screaming("AnyKindOfString"),
            "ANY_KIND_OF_STRING"
        );
        assert_eq!(case_kebab("AnyKindOfString"), "any-kind-of-string");
        assert_eq!(
            case_kebab_screaming("AnyKindOfString"),
            "ANY-KIND-OF-STRING"
        );
        assert_eq!(case_snake_first_upper("RGBCodeMd5", None), "rgb_code_md5");
        assert_eq!(case_snake("JSONData"), "json_data");
        assert_eq!(
            case_convert("any_kind_of_string", CaseType::CamelLower),
            "anyKindOfString"
        );
    }

    #[test]
    fn conversions() {
        assert_eq!(chr(65), "A");
        assert_eq!(ord("A"), 65);
        assert_eq!(oct_str(r"\346\200\241"), "怡");
        assert_eq!(reverse("123456"), "654321");
        assert_eq!(number_format(1234.56, 2, ",", " "), "1 234,56");
        assert_eq!(hide_str("13812345678", 40, "*"), "138****5678");
        assert_eq!(
            hide_str("john.doe@example.com", 40, "*"),
            "joh***oe@example.com"
        );
        assert_eq!(nl2br("a\nb\r\nc", None), "a<br>b<br>c");
        assert_eq!(
            word_wrap("hello world from goframe", 10, "\n"),
            "hello\nworld from\ngoframe"
        );
        assert_eq!(shuffle("123456").len(), 6);
    }

    #[test]
    fn parse_builds_nested_values() {
        let value = parse("v1=m&v2=n").expect("parse");
        assert_eq!(value.to_string(), "map[v1:m v2:n]");

        let value = parse("v[a]=m&v[b]=n").expect("parse");
        assert_eq!(value.to_string(), "map[v:map[a:m b:n]]");

        let value = parse("v[]=m&v[]=n").expect("parse");
        assert_eq!(value.to_string(), "map[v:[m n]]");

        assert!(parse("v=m&v[a]=n").is_err());
        assert!(parse("").expect("empty").is_nil());
    }

    #[test]
    fn similarity_and_versions() {
        assert_eq!(levenshtein("kitten", "sitting", 1, 1, 1), 3);
        let (similar, percent) = similar_text("Hello", "World");
        assert_eq!(similar, 1);
        assert!((percent - 20.0).abs() < 0.01);
        assert_eq!(similar_text("Hello", "Hello").0, 5);
        assert_eq!(soundex("goframe"), "G165");
        assert_eq!(soundex("Hello"), "H400");
        assert!(is_gnu_version("v1.0.1"));
        assert!(!is_gnu_version("1.0.0.1"));
        assert_eq!(compare_version("v1.0.1", "v1.0.2"), -1);
        assert_eq!(compare_version("1.0", "1"), 0);
        assert_eq!(compare_version_go("v4.20.1+incompatible", "v4.20.1"), -1);
    }
}
