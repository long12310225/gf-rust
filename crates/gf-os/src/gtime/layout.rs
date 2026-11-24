//! Port of `os/gtime`'s Go layout translation and PHP-style format support.
//!
//! This module ports the pieces of `gtime_format.go` that do not own a
//! [`crate::gtime::Time`] value:
//!
//! * [`format_go_layout`] renders a time using Go's reference layout language
//!   (`2006-01-02 15:04:05`) instead of `chrono`'s `%`-specifiers, because the
//!   PHP format table is defined in terms of Go layouts and translating the
//!   sentinels (`=j=`, `=G=`, `=u=`) is simplest when the Go formatter is
//!   available verbatim.
//! * [`parse_go_layout`] is the inverse, used by `StrToTimeLayout`,
//!   `NewFromStrLayout`, `LayoutTo` and `LayoutNew`.
//! * [`format_to_go_time`] converts a PHP-style format (`Y-m-d H:i:s`) into a
//!   Go layout, exactly like Go's `formatToStdLayout`.
//! * [`format_to_regex_pattern`] builds the pattern
//!   `ParseTimeFromContent` uses to find a formatted time inside text.
//!
//! Only the Go layout tokens that exist in the standard library's reference
//! layout are recognised. A layout without a zone token parses into a wall
//! clock with no offset, and a layout without a date token defaults to year 0,
//! month 1 and day 1, matching `time.ParseInLocation`.

use chrono::{Datelike, NaiveDate, NaiveDateTime, Timelike};

/// The pieces of a [`crate::gtime::Time`] that a Go layout needs.
pub struct LayoutParts {
    /// Wall clock in the time's own zone.
    pub naive: NaiveDateTime,
    /// Offset from UTC in seconds.
    pub offset_seconds: i32,
    /// Zone abbreviation printed by the `MST` token.
    pub zone: String,
}

/// The wall clock and optional zone offset a layout parsed from a string.
pub struct ParseResult {
    /// Wall clock as written in the input.
    pub naive: NaiveDateTime,
    /// Offset in seconds when the layout contained a zone token.
    pub offset_seconds: Option<i32>,
}

/// Tokens of Go's reference layout, longest first so prefixes such as
/// `2006`/`2` and `-07:00:00`/`-07` resolve to the longest match.
const TOKENS: &[&str] = &[
    ".000000000",
    ",000000000",
    ".999999999",
    ",999999999",
    "-07:00:00",
    "Z07:00:00",
    "January",
    "Monday",
    ".000000",
    ",000000",
    ".999999",
    ",999999",
    "-07:00",
    "Z07:00",
    "2006",
    "002",
    "__2",
    "Jan",
    "Mon",
    "MST",
    "-0700",
    "Z0700",
    ".000",
    ",000",
    ".999",
    ",999",
    "06",
    "01",
    "02",
    "_2",
    "15",
    "03",
    "04",
    "05",
    "PM",
    "pm",
    "-07",
    "Z07",
    "1",
    "2",
    "3",
    "4",
    "5",
];

/// Long month names, January first.
const MONTH_LONG: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Short month names, January first.
const MONTH_SHORT: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Weekday names indexed by Sunday = 0.
const WEEKDAY_LONG: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

/// Short weekday names indexed by Sunday = 0.
const WEEKDAY_SHORT: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// PHP format specifier to Go layout mapping, from `gtime_format.go`.
///
/// `W`, `z`, `t` and `U` are handled by the PHP formatter before this mapping
/// is consulted; they map to the empty layout here so the layout translator
/// drops them, as Go's `formatToStdLayout` does.
pub fn php_go_layout(c: char) -> Option<&'static str> {
    let layout = match c {
        'd' => "02",
        'D' => "Mon",
        'w' => "Monday",
        'N' => "Monday",
        'j' => "=j=02",
        'S' => "02",
        'l' => "Monday",
        'z' => "",
        'W' => "",
        'F' => "January",
        'm' => "01",
        'M' => "Jan",
        'n' => "1",
        't' => "",
        'Y' => "2006",
        'y' => "06",
        'a' => "pm",
        'A' => "PM",
        'g' => "3",
        'G' => "=G=15",
        'h' => "03",
        'H' => "15",
        'i' => "04",
        's' => "05",
        'u' => "=u=.000",
        'U' => "",
        'O' => "-0700",
        'P' => "-07:00",
        'T' => "MST",
        'c' => "2006-01-02T15:04:05-07:00",
        'r' => "Mon, 02 Jan 06 15:04 MST",
        _ => return None,
    };
    Some(layout)
}

/// Returns the Go layout token that starts at `chars[index]`, if any.
fn match_token(chars: &[char], index: usize) -> Option<&'static str> {
    let rest = chars.get(index..)?;
    TOKENS.iter().copied().find(|token| {
        let token_chars: Vec<char> = token.chars().collect();
        rest.len() >= token_chars.len() && rest.iter().zip(token_chars.iter()).all(|(a, b)| a == b)
    })
}

/// Renders `layout` for the fields of `parts`.
pub fn format_go_layout(parts: &LayoutParts, layout: &str) -> String {
    let chars: Vec<char> = layout.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        match match_token(&chars, index) {
            Some(token) => {
                out.push_str(&format_token(token, parts));
                index += token.chars().count();
            }
            None => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }
    out
}

/// Renders one Go layout token.
fn format_token(token: &str, parts: &LayoutParts) -> String {
    let naive = parts.naive;
    match token {
        "2006" => format_year(naive.year()),
        "06" => format!("{:02}", naive.year().rem_euclid(100)),
        "January" => month_name(naive.month(), true),
        "Jan" => month_name(naive.month(), false),
        "01" => format!("{:02}", naive.month()),
        "1" => naive.month().to_string(),
        "02" => format!("{:02}", naive.day()),
        "2" => naive.day().to_string(),
        "_2" => format!("{:>2}", naive.day()),
        "002" => format!("{:03}", naive.ordinal()),
        "__2" => format!("{:>3}", naive.ordinal()),
        "Monday" => weekday_name(naive, true),
        "Mon" => weekday_name(naive, false),
        "15" => format!("{:02}", naive.hour()),
        "03" => format!("{:02}", hour12(naive)),
        "3" => hour12(naive).to_string(),
        "04" => format!("{:02}", naive.minute()),
        "4" => naive.minute().to_string(),
        "05" => format!("{:02}", naive.second()),
        "5" => naive.second().to_string(),
        "PM" => {
            if naive.hour() < 12 {
                "AM".to_string()
            } else {
                "PM".to_string()
            }
        }
        "pm" => {
            if naive.hour() < 12 {
                "am".to_string()
            } else {
                "pm".to_string()
            }
        }
        "MST" => parts.zone.clone(),
        "-0700" => format_offset(parts.offset_seconds, false, false, false),
        "-07:00" => format_offset(parts.offset_seconds, true, false, false),
        "-07" => format_offset(parts.offset_seconds, false, false, true),
        "-07:00:00" => format_offset(parts.offset_seconds, true, true, false),
        "Z0700" => format_offset(parts.offset_seconds, false, false, true),
        "Z07:00" => format_offset(parts.offset_seconds, true, false, true),
        "Z07" => format_offset_hour(parts.offset_seconds),
        "Z07:00:00" => format_offset(parts.offset_seconds, true, true, true),
        token if token.starts_with('.') || token.starts_with(',') => {
            format_fraction(token, naive.nanosecond())
        }
        _ => String::new(),
    }
}

/// Formats a year with Go's four-digit minimum.
fn format_year(year: i32) -> String {
    if year < 0 {
        format!("-{:04}", -year)
    } else {
        format!("{:04}", year)
    }
}

/// Returns a month name, full or three letters.
fn month_name(month: u32, full: bool) -> String {
    let index = month.saturating_sub(1) as usize;
    let table = if full { &MONTH_LONG } else { &MONTH_SHORT };
    table.get(index).copied().unwrap_or("").to_string()
}

/// Returns a weekday name, full or three letters.
fn weekday_name(naive: NaiveDateTime, full: bool) -> String {
    let index = naive.weekday().num_days_from_sunday() as usize;
    let table = if full { &WEEKDAY_LONG } else { &WEEKDAY_SHORT };
    table.get(index).copied().unwrap_or("").to_string()
}

/// Returns the 12-hour clock value, with midnight and noon as 12.
fn hour12(naive: NaiveDateTime) -> u32 {
    let hour = naive.hour() % 12;
    if hour == 0 { 12 } else { hour }
}

/// Formats `HHMM`/`HH:MM`/`HH:MM:SS` and the `Z`-prefixed variants.
fn format_offset(offset_seconds: i32, colon: bool, seconds: bool, zulu: bool) -> String {
    if zulu && offset_seconds == 0 {
        return "Z".to_string();
    }
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute = offset_seconds.unsigned_abs();
    let hours = absolute / 3600;
    let minutes = (absolute % 3600) / 60;
    let secs = absolute % 60;
    let separator = if colon { ":" } else { "" };
    if seconds {
        format!("{sign}{hours:02}{separator}{minutes:02}{separator}{secs:02}")
    } else {
        format!("{sign}{hours:02}{separator}{minutes:02}")
    }
}

/// Formats the hour-only offset token (`-07`, `Z07`).
fn format_offset_hour(offset_seconds: i32) -> String {
    if offset_seconds == 0 {
        return "Z".to_string();
    }
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    format!("{sign}{:02}", offset_seconds.unsigned_abs() / 3600)
}

/// Formats a fractional-second token such as `.000`, `.999` or `,000000`.
fn format_fraction(token: &str, nanos: u32) -> String {
    let separator = token.chars().next().unwrap_or('.');
    let digits = token.chars().count().saturating_sub(1);
    let exact = token.contains('0');
    let nine = format!("{nanos:09}");
    let width = digits.min(9);
    let taken: String = nine.chars().take(width).collect();
    if exact {
        return format!("{separator}{taken}");
    }
    let trimmed = taken.trim_end_matches('0');
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{separator}{trimmed}")
    }
}

/// Scans `value` for a fraction of at most `max_digits` digits.
fn take_fraction(chars: &[char], pos: &mut usize, max_digits: usize) -> Option<u32> {
    if *pos >= chars.len() || !matches!(chars[*pos], '.' | ',') {
        return None;
    }
    let start = *pos + 1;
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() && end - start < max_digits {
        end += 1;
    }
    if end == start {
        return None;
    }
    let mut nanos = 0u32;
    for index in 0..9 {
        let digit = if start + index < end {
            chars[start + index].to_digit(10)?
        } else {
            0
        };
        nanos = nanos * 10 + digit;
    }
    *pos = end;
    Some(nanos)
}

/// Field accumulator used while parsing a Go layout.
#[derive(Default)]
struct Fields {
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
    nano: u32,
    hour12: Option<u32>,
    pm: Option<bool>,
    offset_seconds: Option<i32>,
}

/// Parses `value` using a Go layout. See [`ParseResult`].
pub fn parse_go_layout(value: &str, layout: &str) -> Option<ParseResult> {
    let layout_chars: Vec<char> = layout.chars().collect();
    let value_chars: Vec<char> = value.chars().collect();
    let mut fields = Fields {
        year: 0,
        month: 1,
        day: 1,
        ..Fields::default()
    };
    let mut layout_index = 0;
    let mut pos = 0;
    let mut saw_fraction = false;
    while layout_index < layout_chars.len() {
        match match_token(&layout_chars, layout_index) {
            Some(token) => {
                if !parse_token(
                    token,
                    &value_chars,
                    &mut pos,
                    &mut fields,
                    &mut saw_fraction,
                ) {
                    return None;
                }
                layout_index += token.chars().count();
            }
            None => {
                let expected = layout_chars[layout_index];
                if expected.is_whitespace() {
                    if pos < value_chars.len() && value_chars[pos].is_whitespace() {
                        pos += 1;
                    }
                } else {
                    if pos >= value_chars.len() || value_chars[pos] != expected {
                        return None;
                    }
                    pos += 1;
                }
                layout_index += 1;
            }
        }
    }
    if !saw_fraction
        && pos + 1 < value_chars.len()
        && matches!(value_chars[pos], '.' | ',')
        && value_chars[pos + 1].is_ascii_digit()
    {
        let _ = take_fraction(&value_chars, &mut pos, 9);
    }
    if pos != value_chars.len() {
        return None;
    }
    let hour = match fields.hour12 {
        Some(value) => {
            let base = value % 12;
            if fields.pm == Some(true) {
                base + 12
            } else {
                base
            }
        }
        None => fields.hour,
    };
    let date = NaiveDate::from_ymd_opt(fields.year, fields.month, fields.day)?;
    let naive = date.and_hms_nano_opt(hour, fields.minute, fields.second, fields.nano)?;
    Some(ParseResult {
        naive,
        offset_seconds: fields.offset_seconds,
    })
}

/// Parses one Go layout token from `chars` into `fields`.
fn parse_token(
    token: &str,
    chars: &[char],
    pos: &mut usize,
    fields: &mut Fields,
    saw_fraction: &mut bool,
) -> bool {
    match token {
        "2006" => take_number(chars, pos, 4, 4)
            .map(|value| fields.year = value as i32)
            .is_some(),
        "06" => take_number(chars, pos, 2, 2)
            .map(|value| fields.year = value as i32)
            .is_some(),
        "January" => take_month(chars, pos, true)
            .map(|value| fields.month = value)
            .is_some(),
        "Jan" => take_month(chars, pos, false)
            .map(|value| fields.month = value)
            .is_some(),
        "01" => take_number(chars, pos, 2, 2)
            .map(|value| fields.month = value as u32)
            .is_some(),
        "1" => take_number(chars, pos, 1, 2)
            .map(|value| fields.month = value as u32)
            .is_some(),
        "02" => take_number(chars, pos, 2, 2)
            .map(|value| fields.day = value as u32)
            .is_some(),
        "2" => take_number(chars, pos, 1, 2)
            .map(|value| fields.day = value as u32)
            .is_some(),
        "_2" => {
            skip_spaces(chars, pos);
            take_number(chars, pos, 1, 2)
                .map(|value| fields.day = value as u32)
                .is_some()
        }
        "002" => take_number(chars, pos, 3, 3).is_some(),
        "__2" => {
            skip_spaces(chars, pos);
            take_number(chars, pos, 1, 3).is_some()
        }
        "Monday" => take_weekday(chars, pos, true),
        "Mon" => take_weekday(chars, pos, false),
        "15" => take_number(chars, pos, 2, 2)
            .map(|value| fields.hour = value as u32)
            .is_some(),
        "03" => take_number(chars, pos, 2, 2)
            .map(|value| fields.hour12 = Some(value as u32))
            .is_some(),
        "3" => take_number(chars, pos, 1, 2)
            .map(|value| fields.hour12 = Some(value as u32))
            .is_some(),
        "04" => take_number(chars, pos, 2, 2)
            .map(|value| fields.minute = value as u32)
            .is_some(),
        "4" => take_number(chars, pos, 1, 2)
            .map(|value| fields.minute = value as u32)
            .is_some(),
        "05" => take_number(chars, pos, 2, 2)
            .map(|value| fields.second = value as u32)
            .is_some(),
        "5" => take_number(chars, pos, 1, 2)
            .map(|value| fields.second = value as u32)
            .is_some(),
        "PM" | "pm" => take_meridiem(chars, pos)
            .map(|value| fields.pm = Some(value))
            .is_some(),
        "MST" => take_zone_name(chars, pos),
        "-0700" => parse_offset(chars, pos)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "-07:00" => parse_offset_colon(chars, pos, false)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "-07:00:00" => parse_offset_colon(chars, pos, true)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "-07" => parse_offset_hour(chars, pos)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "Z0700" => parse_zulu(chars, pos, false, false)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "Z07:00" => parse_zulu(chars, pos, true, false)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "Z07:00:00" => parse_zulu(chars, pos, true, true)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        "Z07" => parse_zulu_hour(chars, pos)
            .map(|value| fields.offset_seconds = Some(value))
            .is_some(),
        token if token.starts_with('.') || token.starts_with(',') => {
            *saw_fraction = true;
            let digits = token.chars().count().saturating_sub(1);
            if let Some(nanos) = take_fraction(chars, pos, digits) {
                fields.nano = nanos;
            }
            true
        }
        _ => false,
    }
}

/// Consumes `count`-ish digits from `chars`, advancing `pos`.
fn take_number(chars: &[char], pos: &mut usize, min: usize, max: usize) -> Option<i64> {
    let start = *pos;
    let mut end = start;
    while end < chars.len() && chars[end].is_ascii_digit() && end - start < max {
        end += 1;
    }
    if end - start < min {
        return None;
    }
    let mut value = 0i64;
    for c in &chars[start..end] {
        value = value
            .checked_mul(10)?
            .checked_add(i64::from(c.to_digit(10)?))?;
    }
    *pos = end;
    Some(value)
}

/// Skips up to two spaces, for the `_2` and `__2` tokens.
fn skip_spaces(chars: &[char], pos: &mut usize) {
    let mut skipped = 0;
    while *pos < chars.len() && chars[*pos] == ' ' && skipped < 2 {
        *pos += 1;
        skipped += 1;
    }
}

/// Matches a month name and returns its 1-based number.
fn take_month(chars: &[char], pos: &mut usize, full: bool) -> Option<u32> {
    let table = if full { &MONTH_LONG } else { &MONTH_SHORT };
    let index = match_name(chars, pos, table)?;
    Some(index as u32 + 1)
}

/// Matches a weekday name (the value itself is not needed).
fn take_weekday(chars: &[char], pos: &mut usize, full: bool) -> bool {
    let table = if full { &WEEKDAY_LONG } else { &WEEKDAY_SHORT };
    match_name(chars, pos, table).is_some()
}

/// Matches any of `names` case-insensitively, advancing `pos` past it.
fn match_name(chars: &[char], pos: &mut usize, names: &[&str]) -> Option<usize> {
    for (index, name) in names.iter().enumerate() {
        let name_chars: Vec<char> = name.chars().collect();
        if chars.len() - *pos < name_chars.len() {
            continue;
        }
        let matches = chars[*pos..*pos + name_chars.len()]
            .iter()
            .zip(name_chars.iter())
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
        if matches {
            *pos += name_chars.len();
            return Some(index);
        }
    }
    None
}

/// Matches `AM`/`PM` in either case; returns whether it is post-meridiem.
fn take_meridiem(chars: &[char], pos: &mut usize) -> Option<bool> {
    if *pos + 2 > chars.len() {
        return None;
    }
    let first = chars[*pos];
    let second = chars[*pos + 1];
    if !second.eq_ignore_ascii_case(&'M') {
        return None;
    }
    let pm = if first.eq_ignore_ascii_case(&'P') {
        true
    } else if first.eq_ignore_ascii_case(&'A') {
        false
    } else {
        return None;
    };
    *pos += 2;
    Some(pm)
}

/// Consumes a zone abbreviation such as `CST`. The zone itself is not
/// resolved, so a parsed abbreviation keeps the layout's default offset.
fn take_zone_name(chars: &[char], pos: &mut usize) -> bool {
    let start = *pos;
    while *pos < chars.len() && chars[*pos].is_ascii_alphabetic() && *pos - start < 5 {
        *pos += 1;
    }
    *pos - start >= 3
}

/// Parses `+hhmm` / `-hhmm`.
fn parse_offset(chars: &[char], pos: &mut usize) -> Option<i32> {
    let sign = take_sign(chars, pos)?;
    let hours = take_number(chars, pos, 2, 2)? as i32;
    let minutes = take_number(chars, pos, 2, 2)? as i32;
    Some(sign * (hours * 3600 + minutes * 60))
}

/// Parses `+hh:mm` and, when `with_seconds`, `+hh:mm:ss`.
fn parse_offset_colon(chars: &[char], pos: &mut usize, with_seconds: bool) -> Option<i32> {
    let sign = take_sign(chars, pos)?;
    let hours = take_number(chars, pos, 2, 2)? as i32;
    expect_char(chars, pos, ':')?;
    let minutes = take_number(chars, pos, 2, 2)? as i32;
    let mut seconds = 0i32;
    if with_seconds {
        expect_char(chars, pos, ':')?;
        seconds = take_number(chars, pos, 2, 2)? as i32;
    }
    Some(sign * (hours * 3600 + minutes * 60 + seconds))
}

/// Parses `+hh` / `-hh`.
fn parse_offset_hour(chars: &[char], pos: &mut usize) -> Option<i32> {
    let sign = take_sign(chars, pos)?;
    let hours = take_number(chars, pos, 2, 2)? as i32;
    Some(sign * hours * 3600)
}

/// Parses the `Z`-prefixed offsets, accepting `Z` as UTC.
fn parse_zulu(chars: &[char], pos: &mut usize, colon: bool, with_seconds: bool) -> Option<i32> {
    if *pos < chars.len() && matches!(chars[*pos], 'Z' | 'z') {
        *pos += 1;
        return Some(0);
    }
    if colon {
        parse_offset_colon(chars, pos, with_seconds)
    } else if with_seconds {
        None
    } else {
        parse_offset(chars, pos)
    }
}

/// Parses the `Z07` token, accepting `Z` as UTC.
fn parse_zulu_hour(chars: &[char], pos: &mut usize) -> Option<i32> {
    if *pos < chars.len() && matches!(chars[*pos], 'Z' | 'z') {
        *pos += 1;
        return Some(0);
    }
    parse_offset_hour(chars, pos)
}

/// Consumes a `+` or `-` and returns its sign.
fn take_sign(chars: &[char], pos: &mut usize) -> Option<i32> {
    let sign = match chars.get(*pos)? {
        '+' => 1,
        '-' => -1,
        _ => return None,
    };
    *pos += 1;
    Some(sign)
}

/// Requires `expected` at `pos` and consumes it.
fn expect_char(chars: &[char], pos: &mut usize, expected: char) -> Option<()> {
    if chars.get(*pos) == Some(&expected) {
        *pos += 1;
        Some(())
    } else {
        None
    }
}

/// Converts a PHP-style format to a Go layout, like Go's `formatToStdLayout`.
pub fn format_to_go_time(php_format: &str) -> String {
    let chars: Vec<char> = php_format.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        if c == '\\' {
            if index + 1 < chars.len() {
                out.push(chars[index + 1]);
                index += 2;
                continue;
            }
            return out;
        }
        match php_go_layout(c) {
            Some(layout) => match c {
                'j' => out.push('2'),
                'G' => out.push_str("15"),
                'u' => {
                    if index > 0 && chars[index - 1] == '.' {
                        out.push_str("000");
                    } else {
                        out.push_str(".000");
                    }
                }
                _ => out.push_str(layout),
            },
            None => out.push(c),
        }
        index += 1;
    }
    out
}

/// Builds the regular expression `ParseTimeFromContent` uses for `format`.
///
/// This mirrors Go's three sequential replacements: digits, then letters, then
/// whitespace runs.
pub fn format_to_regex_pattern(php_format: &str) -> String {
    let layout = go_quote_meta(&format_to_go_time(php_format));
    let mut out = String::with_capacity(layout.len() * 4);
    let mut in_space = false;
    for c in layout.chars() {
        if c.is_ascii_digit() {
            in_space = false;
            out.push_str("[0-9]");
        } else if c.is_ascii_alphabetic() {
            in_space = false;
            out.push_str("[A-Za-z]");
        } else if c.is_whitespace() {
            if !in_space {
                out.push_str(r"\s+");
                in_space = true;
            }
        } else {
            in_space = false;
            out.push(c);
        }
    }
    out
}

/// Escapes regular-expression metacharacters exactly like Go's
/// `regexp.QuoteMeta` (notably leaving `-` unescaped).
fn go_quote_meta(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(
            c,
            '\\' | '.' | '+' | '*' | '?' | '(' | ')' | '|' | '[' | ']' | '{' | '}' | '^' | '$'
        ) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Returns the English ordinal suffix for a two-digit day string.
pub fn month_day_suffix(day: &str) -> &'static str {
    match day {
        "01" | "21" | "31" => "st",
        "02" | "22" => "nd",
        "03" | "23" => "rd",
        _ => "th",
    }
}

/// Returns the PHP `w` number (Sunday = 0) for a full weekday name.
pub fn weekday_number(name: &str) -> &'static str {
    match name {
        "Sunday" => "0",
        "Monday" => "1",
        "Tuesday" => "2",
        "Wednesday" => "3",
        "Thursday" => "4",
        "Friday" => "5",
        "Saturday" => "6",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> LayoutParts {
        LayoutParts {
            naive: NaiveDate::from_ymd_opt(2018, 8, 8)
                .and_then(|date| date.and_hms_opt(8, 8, 8))
                .unwrap_or_default(),
            offset_seconds: 8 * 3600,
            zone: "CST".to_string(),
        }
    }

    #[test]
    fn formats_standard_layouts() {
        let parts = parts();
        assert_eq!(
            format_go_layout(&parts, "2006-01-02 15:04:05"),
            "2018-08-08 08:08:08"
        );
        assert_eq!(
            format_go_layout(&parts, "2006-01-02T15:04:05-07:00"),
            "2018-08-08T08:08:08+08:00"
        );
        assert_eq!(
            format_go_layout(&parts, "Mon, 02 Jan 06 15:04 MST"),
            "Wed, 08 Aug 18 08:08 CST"
        );
        assert_eq!(format_go_layout(&parts, "15:04:05"), "08:08:08");
    }

    #[test]
    fn formats_padding_variants() {
        let parts = LayoutParts {
            naive: NaiveDate::from_ymd_opt(2018, 1, 5)
                .and_then(|date| date.and_hms_nano_opt(3, 4, 5, 123_000_000))
                .unwrap_or_default(),
            offset_seconds: 0,
            zone: "UTC".to_string(),
        };
        assert_eq!(format_go_layout(&parts, "2006-1-2 3:4:5"), "2018-1-5 3:4:5");
        assert_eq!(
            format_go_layout(&parts, "2006-01-02 03:04:05.000"),
            "2018-01-05 03:04:05.123"
        );
        assert_eq!(
            format_go_layout(&parts, "2006-01-02 03:04:05.999"),
            "2018-01-05 03:04:05.123"
        );
        assert_eq!(format_go_layout(&parts, "002 __2"), "005   5");
        assert_eq!(format_go_layout(&parts, "Z07:00 Z0700"), "Z Z");
    }

    #[test]
    fn parses_standard_layouts() {
        let parsed = parse_go_layout("2018-08-08 08:08:08", "2006-01-02 15:04:05");
        let parsed = parsed.unwrap_or_else(|| panic!("expected a parse"));
        assert_eq!(parsed.naive.to_string(), "2018-08-08 08:08:08");
        assert_eq!(parsed.offset_seconds, None);

        let parsed = parse_go_layout("2018-08-08T08:08:08+08:00", "2006-01-02T15:04:05-07:00");
        let parsed = parsed.unwrap_or_else(|| panic!("expected a parse"));
        assert_eq!(parsed.offset_seconds, Some(8 * 3600));
    }

    #[test]
    fn parses_defaults_for_missing_fields() {
        let parsed = parse_go_layout("2023-07", "2006-01");
        let parsed = parsed.unwrap_or_else(|| panic!("expected a parse"));
        assert_eq!(parsed.naive.to_string(), "2023-07-01 00:00:00");
    }

    #[test]
    fn rejects_mismatched_literals() {
        assert!(parse_go_layout("2006-01-02 15:04:05", "aabbccdd").is_none());
        assert!(parse_go_layout("2006.01", "2006-01-02").is_none());
    }

    #[test]
    fn converts_php_formats() {
        assert_eq!(format_to_go_time("Y-m-d H:i:s"), "2006-01-02 15:04:05");
        assert_eq!(format_to_go_time("\\T\\i\\m\\e Y-m-d"), "Time 2006-01-02");
        assert_eq!(format_to_go_time("Y-m-j G:i:s.u"), "2006-01-2 15:04:05.000");
        assert_eq!(format_to_go_time("Y-m-j G:i:su"), "2006-01-2 15:04:05.000");
        assert_eq!(format_to_go_time("YmdHis"), "20060102150405");
    }

    #[test]
    fn builds_regex_patterns() {
        assert_eq!(
            format_to_regex_pattern("Y-m-d H:i:s"),
            r"[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\s+[0-9][0-9]:[0-9][0-9]:[0-9][0-9]"
        );
    }

    #[test]
    fn suffixes_and_weekdays() {
        assert_eq!(month_day_suffix("01"), "st");
        assert_eq!(month_day_suffix("02"), "nd");
        assert_eq!(month_day_suffix("03"), "rd");
        assert_eq!(month_day_suffix("11"), "th");
        assert_eq!(weekday_number("Sunday"), "0");
        assert_eq!(weekday_number("Saturday"), "6");
    }
}
