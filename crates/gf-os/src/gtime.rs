//! Port of `os/gtime`: a time wrapper with PHP-style formatting, lenient
//! string parsing, zones and SQL/JSON bridges.
//!
//! Ported Go files:
//!
//! * `gtime.go` —timestamps, `StrToTime`, `ConvertZone`, `ParseDuration`.
//! * `gtime_time.go` —construction, `Time` accessors and arithmetic.
//! * `gtime_format.go` —PHP `Format`, `Layout`, layout conversion.
//! * `gtime_time_zone.go` —zones, `ToZone`, `ToLocation`, `Local`, UTC.
//! * `gtime_sql.go` —`Scan`/`Value`.
//! * `gtime_time_wrapper.go` —`String`.
//!
//! # Representation
//!
//! Go's `*gtime.Time` is a pointer that may be `nil`, and Go's zero
//! `time.Time` (January 1, year 1, 00:00:00 UTC) is a distinct value that is
//! *not* nil but reports `IsZero()`. The port keeps both states in one struct:
//!
//! ```text
//! Time { inner: None }                  // Go's nil *Time
//! Time { inner: Some(zero instant) }    // Go's zero time.Time
//! Time { inner: Some(instant) }         // a real time
//! ```
//!
//! [`Time::is_nil`] reports the first, [`Time::is_zero`] reports the first two,
//! and [`Time::default`] is the nil time. Every method accepts nil and returns
//! the value Go's nil receiver checks (or the zero value where Go would panic);
//! this is documented as a divergence below.
//!
//! The wall clock is stored as [`chrono::DateTime<FixedOffset>`] plus a
//! [`Location`] describing where the offset came from (`Local`, `Utc`, a named
//! IANA zone, or a fixed zone with an optional name). Keeping the zone kind is
//! what makes `Format("T")`, `Location()` and `ToZone` behave like Go.
//!
//! # Divergences from Go
//!
//! * **nil versus `Option`.** Go returns `nil` on parse failures; the port
//!   returns `Result<_, gf_errors::gerror::Error>` for fallible functions and
//!   `Option<Time>` for the `NewFromStr*` helpers that Go documents as
//!   "returns nil on error".
//! * **Errors instead of panics.** `LayoutNew`/`LayoutTo` return `Option`
//!   instead of panicking, and parse failures return a typed
//!   `CodeInvalidParameter` error.
//! * **Zone abbreviations.** Named zones (`Asia/Shanghai`) use chrono-tz and
//!   keep their abbreviation (`CST`). `Local` has no portable abbreviation in
//!   chrono, so its `MST`/`T` rendering is the numeric offset (`+08:00`);
//!   fixed zones keep the name passed to `Location::fixed`. A parsed `MST`
//!   token does not resolve a named zone and keeps the default offset.
//! * **Process time zone.** `SetTimeZone` records the zone used by future
//!   [`Time::local`] conversions; it does not mutate the process `TZ`.
//! * **AddDate and DST.** Wall-clock arithmetic is re-resolved in the stored
//!   zone, so named zones follow DST; absolute `Add` recomputes the offset the
//!   same way.
//! * **SQL values.** Go's `Value` returns a `time.Time`; the port returns a
//!   formatted string (or the time-only string for year-zero values).
//! * **`New()`** keeps Go's behavior: no argument returns the *zero* time, not
//!   "now"; [`now`] is the current time.
//! * `AddStr` accepts the `d` suffix through [`parse_duration`]; Go's
//!   `AddStr` uses `time.ParseDuration`, which has no `d` unit.

pub mod layout;

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use std::sync::{LazyLock, RwLock};
use std::time::Instant as StdInstant;

use chrono::{
    DateTime, Datelike, Days, FixedOffset, Local, LocalResult, NaiveDate, NaiveDateTime, TimeDelta,
    TimeZone, Timelike, Utc, Weekday,
};
use chrono_tz::{OffsetName, Tz};
use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::Error;
use regex::Regex;

use layout::{LayoutParts, format_go_layout, format_to_go_time, format_to_regex_pattern};

/// One day as a [`Duration`].
pub const D: Duration = 24 * H;
/// One hour as a [`Duration`].
pub const H: Duration = 60 * M;
/// One minute as a [`Duration`].
pub const M: Duration = 60 * S;
/// One second as a [`Duration`].
pub const S: Duration = 1_000 * MS;
/// One millisecond as a [`Duration`].
pub const MS: Duration = 1_000 * US;
/// One microsecond as a [`Duration`].
pub const US: Duration = 1_000 * NS;
/// One nanosecond as a [`Duration`].
pub const NS: Duration = 1;

/// A duration in nanoseconds, Go's `time.Duration`.
pub type Duration = i64;

/// Go's first "date" regex (numeric separators `-`, `/`, `.`).
const TIME_PATTERN1: &str = r"(\d{4}[-/\.]\d{1,2}[-/\.]\d{1,2})[:\sT-]*(\d{0,2}:{0,1}\d{0,2}:{0,1}\d{0,2}){0,1}\.{0,1}(\d{0,9})([\sZ]{0,1})([\+\-]{0,1})([:\d]*)";
/// Go's second "date" regex (month names).
const TIME_PATTERN2: &str = r"(\d{1,2}[-/\.][A-Za-z]{3,}[-/\.]\d{4})[:\sT-]*(\d{0,2}:{0,1}\d{0,2}:{0,1}\d{0,2}){0,1}\.{0,1}(\d{0,9})([\sZ]{0,1})([\+\-]{0,1})([:\d]*)";
/// Go's "time only" regex.
const TIME_PATTERN3: &str = r"(\d{2}):(\d{2}):(\d{2})\.{0,1}(\d{0,9})";

/// Compiled numeric date pattern.
static TIME_REGEX1: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(TIME_PATTERN1).ok());
/// Compiled month-name date pattern.
static TIME_REGEX2: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(TIME_PATTERN2).ok());
/// Compiled time-only pattern.
static TIME_REGEX3: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(TIME_PATTERN3).ok());

/// The IANA zone configured through [`set_time_zone`].
static PROCESS_TIME_ZONE: LazyLock<RwLock<Option<String>>> = LazyLock::new(|| RwLock::new(None));

/// Where a [`Time`]'s offset comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Location {
    /// The host's local zone, `time.Local`.
    Local,
    /// UTC, `time.UTC`.
    Utc,
    /// A fixed offset, `time.FixedZone`. The name is the abbreviation printed
    /// by the `MST` layout token; an empty name matches `FixedZone("", off)`.
    Fixed {
        /// Optional zone abbreviation.
        name: Option<String>,
        /// Offset from UTC in seconds.
        offset_seconds: i32,
    },
    /// A named IANA zone loaded from the system zone database.
    Named(String),
}

impl Location {
    /// Creates a fixed zone, like `time.FixedZone(name, offset)`.
    pub fn fixed(name: impl Into<String>, offset_seconds: i32) -> Location {
        Location::Fixed {
            name: Some(name.into()),
            offset_seconds,
        }
    }

    /// Creates a named IANA zone location.
    pub fn named(name: impl Into<String>) -> Location {
        Location::Named(name.into())
    }

    /// Returns the name Go's `Location.String` would print.
    ///
    /// Local zones print `"Local"` because chrono does not expose the IANA
    /// identifier reliably; unnamed fixed zones print `""`.
    pub fn name(&self) -> &str {
        match self {
            Location::Local => "Local",
            Location::Utc => "UTC",
            Location::Fixed { name, .. } => name.as_deref().unwrap_or(""),
            Location::Named(name) => name,
        }
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A time value with GoFrame's extra behavior.
///
/// `Time { inner: None }` is Go's nil `*gtime.Time`; the zero instant is a
/// separate `Some` value. See the module docs.
#[derive(Clone, Debug)]
pub struct Time {
    inner: Option<DateTime<FixedOffset>>,
    location: Location,
    layout: Option<String>,
}

impl Default for Time {
    /// Returns the nil time, Go's nil `*Time`.
    fn default() -> Time {
        Time::nil()
    }
}

impl PartialEq for Time {
    fn eq(&self, other: &Time) -> bool {
        match (self.inner, other.inner) {
            (None, None) => true,
            (Some(a), Some(b)) => a.naive_utc() == b.naive_utc(),
            _ => false,
        }
    }
}

impl Eq for Time {}

impl PartialOrd for Time {
    fn partial_cmp(&self, other: &Time) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Time {
    fn cmp(&self, other: &Time) -> Ordering {
        match (self.inner, other.inner) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(a), Some(b)) => a.naive_utc().cmp(&b.naive_utc()),
        }
    }
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

impl Time {
    /// Returns the nil time.
    pub fn nil() -> Time {
        Time {
            inner: None,
            location: Location::Utc,
            layout: None,
        }
    }

    /// Returns Go's zero time, January 1, year 1, 00:00:00 UTC.
    pub fn zero() -> Time {
        Time {
            inner: zero_datetime(),
            location: Location::Utc,
            layout: None,
        }
    }

    /// Creates a zero time, like `gtime.New()` with no argument.
    ///
    /// Note that Go's `New()` returns the zero time, not the current time; use
    /// [`Time::now`] for that.
    pub fn new() -> Time {
        Time::zero()
    }

    /// Creates the current time in the local zone.
    pub fn now() -> Time {
        Time {
            inner: Some(Local::now().fixed_offset()),
            location: Location::Local,
            layout: None,
        }
    }

    /// Creates the current time in UTC.
    pub fn now_utc() -> Time {
        Time {
            inner: Some(Utc::now().fixed_offset()),
            location: Location::Utc,
            layout: None,
        }
    }

    /// Wraps a `chrono` datetime, keeping its offset as a fixed zone.
    pub fn new_from_time(time: DateTime<FixedOffset>) -> Time {
        let offset = time.offset().local_minus_utc();
        let location = if offset == 0 {
            Location::Utc
        } else {
            Location::Fixed {
                name: None,
                offset_seconds: offset,
            }
        };
        Time {
            inner: Some(time),
            location,
            layout: None,
        }
    }

    /// Wraps a UTC instant.
    pub fn from_instant(time: DateTime<Utc>) -> Time {
        Time {
            inner: Some(time.fixed_offset()),
            location: Location::Utc,
            layout: None,
        }
    }

    /// Parses `str` using Go's lenient rules; returns `None` on failure, like
    /// Go's `NewFromStr`.
    pub fn new_from_str(str: &str) -> Option<Time> {
        str_to_time(str, None).ok()
    }

    /// Parses `str` using a PHP-style `format`; returns `None` on failure.
    pub fn new_from_str_format(str: &str, format: &str) -> Option<Time> {
        str_to_time_format(str, format).ok()
    }

    /// Parses `str` using a Go `layout`; returns `None` on failure.
    pub fn new_from_str_layout(str: &str, layout: &str) -> Option<Time> {
        str_to_time_layout(str, layout).ok()
    }

    /// Creates a time from a timestamp in seconds, milliseconds, microseconds
    /// or nanoseconds, like Go's `NewFromTimeStamp`.
    pub fn new_from_time_stamp(timestamp: i64) -> Time {
        if timestamp == 0 {
            return Time::zero();
        }
        let (mut sec, mut nano) = (timestamp, 0i64);
        if timestamp > 1_000_000_000 {
            let mut scaled = timestamp;
            while scaled < 1_000_000_000_000_000_000 {
                scaled = match scaled.checked_mul(10) {
                    Some(value) => value,
                    None => break,
                };
            }
            sec = scaled / 1_000_000_000;
            nano = scaled % 1_000_000_000;
        }
        from_unix(sec, nano)
    }

    /// Creates a time from a Unix timestamp in seconds.
    pub fn new_from_timestamp(timestamp: i64) -> Time {
        if timestamp == 0 {
            return Time::zero();
        }
        from_unix(timestamp, 0)
    }

    /// Creates a time from a Unix timestamp in milliseconds.
    pub fn new_from_timestamp_milli(timestamp: i64) -> Time {
        from_unix(
            timestamp.div_euclid(1_000),
            timestamp.rem_euclid(1_000) * 1_000_000,
        )
    }

    /// Creates a time from a Unix timestamp in microseconds.
    pub fn new_from_timestamp_micro(timestamp: i64) -> Time {
        from_unix(
            timestamp.div_euclid(1_000_000),
            timestamp.rem_euclid(1_000_000) * 1_000,
        )
    }

    /// Creates a time from a Unix timestamp in nanoseconds.
    pub fn new_from_timestamp_nano(timestamp: i64) -> Time {
        from_unix(
            timestamp.div_euclid(1_000_000_000),
            timestamp.rem_euclid(1_000_000_000),
        )
    }

    /// Reports whether the value is Go's nil `*Time`.
    pub fn is_nil(&self) -> bool {
        self.inner.is_none()
    }

    /// Reports whether the value is nil or Go's zero time.
    pub fn is_zero(&self) -> bool {
        match self.inner {
            None => true,
            Some(inner) => zero_naive_utc().is_some_and(|zero| inner.naive_utc() == zero),
        }
    }

    /// Returns the stored layout, if [`Time::set_layout`] was called.
    pub fn stored_layout(&self) -> Option<&str> {
        self.layout.as_deref()
    }

    /// Stores a layout string for later retrieval.
    pub fn set_layout(&mut self, layout: &str) {
        self.layout = Some(layout.to_string());
    }

    /// Returns the year.
    pub fn year(&self) -> i32 {
        self.inner.map(|inner| inner.year()).unwrap_or(0)
    }

    /// Returns the month, or 0 for a zero/nil time like Go's `Month`.
    pub fn month(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        self.inner.map(|inner| inner.month()).unwrap_or(0)
    }

    /// Returns the day of the month.
    pub fn day(&self) -> u32 {
        self.inner.map(|inner| inner.day()).unwrap_or(0)
    }

    /// Returns the hour.
    pub fn hour(&self) -> u32 {
        self.inner.map(|inner| inner.hour()).unwrap_or(0)
    }

    /// Returns the minute.
    pub fn minute(&self) -> u32 {
        self.inner.map(|inner| inner.minute()).unwrap_or(0)
    }

    /// Returns the second, or 0 for nil/zero like Go's `Second`.
    pub fn second(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        self.inner.map(|inner| inner.second()).unwrap_or(0)
    }

    /// Returns the millisecond within the second.
    pub fn millisecond(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        self.inner
            .map(|inner| inner.nanosecond() / 1_000_000)
            .unwrap_or(0)
    }

    /// Returns the microsecond within the second.
    pub fn microsecond(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        self.inner
            .map(|inner| inner.nanosecond() / 1_000)
            .unwrap_or(0)
    }

    /// Returns the nanosecond within the second.
    pub fn nanosecond(&self) -> u32 {
        if self.is_zero() {
            return 0;
        }
        self.inner.map(|inner| inner.nanosecond()).unwrap_or(0)
    }

    /// Returns the weekday.
    pub fn weekday(&self) -> Weekday {
        self.inner
            .map(|inner| inner.weekday())
            .unwrap_or(Weekday::Mon)
    }

    /// Returns the day of the year, 1-based.
    pub fn year_day(&self) -> u32 {
        self.inner.map(|inner| inner.ordinal()).unwrap_or(0)
    }

    /// Returns the year, month and day.
    pub fn date(&self) -> (i32, u32, u32) {
        match self.inner {
            Some(inner) => (inner.year(), inner.month(), inner.day()),
            None => (0, 0, 0),
        }
    }

    /// Returns the time of day as `(hour, minute, second, nanosecond)`.
    pub fn time(&self) -> (u32, u32, u32, u32) {
        (self.hour(), self.minute(), self.second(), self.nanosecond())
    }

    /// Returns the zone abbreviation.
    pub fn zone(&self) -> String {
        self.zone_abbreviation()
    }

    /// Returns the location the time is expressed in.
    pub fn location(&self) -> &Location {
        &self.location
    }

    /// Returns the timestamp in seconds, or 0 for nil/zero.
    pub fn timestamp(&self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        self.inner.map(|inner| inner.timestamp()).unwrap_or(0)
    }

    /// Returns the timestamp in milliseconds, rounding down.
    pub fn timestamp_milli(&self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        self.inner
            .map(|inner| inner.timestamp_millis())
            .unwrap_or(0)
    }

    /// Returns the timestamp in microseconds, rounding down.
    pub fn timestamp_micro(&self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        self.inner
            .map(|inner| inner.timestamp_micros())
            .unwrap_or(0)
    }

    /// Returns the timestamp in nanoseconds.
    pub fn timestamp_nano(&self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        self.inner
            .and_then(|inner| inner.timestamp_nanos_opt())
            .unwrap_or(0)
    }

    /// Returns the timestamp in seconds as a string, or `""` for nil/zero.
    pub fn timestamp_str(&self) -> String {
        if self.is_zero() {
            return String::new();
        }
        self.timestamp().to_string()
    }

    /// Returns the timestamp in milliseconds as a string.
    pub fn timestamp_milli_str(&self) -> String {
        if self.is_zero() {
            return String::new();
        }
        self.timestamp_milli().to_string()
    }

    /// Returns the timestamp in microseconds as a string.
    pub fn timestamp_micro_str(&self) -> String {
        if self.is_zero() {
            return String::new();
        }
        self.timestamp_micro().to_string()
    }

    /// Returns the timestamp in nanoseconds as a string.
    pub fn timestamp_nano_str(&self) -> String {
        if self.is_zero() {
            return String::new();
        }
        self.timestamp_nano().to_string()
    }

    /// Returns the time as `"2006-01-02 15:04:05"`, or `""` for nil/zero.
    ///
    /// A time with year 0 is rendered as `"15:04:05"`, matching Go's wrapper.
    pub fn string(&self) -> String {
        if self.is_zero() {
            return String::new();
        }
        let Some(inner) = self.inner else {
            return String::new();
        };
        let layout = if inner.year() == 0 {
            "15:04:05"
        } else {
            "2006-01-02 15:04:05"
        };
        self.layout(layout)
    }

    /// Returns the date as `"2006-01-02"`.
    pub fn date_string(&self) -> String {
        self.layout("2006-01-02")
    }

    /// Returns the datetime as `"2006-01-02 15:04:05"`.
    pub fn datetime_string(&self) -> String {
        self.layout("2006-01-02 15:04:05")
    }

    /// Returns the time in ISO 8601 form.
    pub fn iso8601(&self) -> String {
        self.layout("2006-01-02T15:04:05-07:00")
    }

    /// Returns the time in RFC 822 form.
    pub fn rfc822(&self) -> String {
        self.layout("Mon, 02 Jan 06 15:04 MST")
    }

    /// Adds `duration` nanoseconds.
    pub fn add(&self, duration: Duration) -> Time {
        let Some(inner) = self.inner else {
            return self.clone();
        };
        let delta = TimeDelta::nanoseconds(duration);
        match inner.checked_add_signed(delta) {
            Some(next) => self.at(next.with_timezone(&Utc)),
            None => self.clone(),
        }
    }

    /// Parses `duration` and adds it, accepting `d` for days.
    pub fn add_str(&self, duration: &str) -> Result<Time, Error> {
        let value = parse_duration(duration)?;
        Ok(self.add(value))
    }

    /// Subtracts `duration` nanoseconds.
    pub fn sub(&self, other: &Time) -> Duration {
        match (self.inner, other.inner) {
            (Some(a), Some(b)) => a
                .signed_duration_since(b)
                .num_nanoseconds()
                .unwrap_or_else(|| if a < b { i64::MIN } else { i64::MAX }),
            _ => 0,
        }
    }

    /// Parses `duration` and subtracts it.
    pub fn sub_str(&self, duration: &str) -> Result<Time, Error> {
        let value = parse_duration(duration)?;
        Ok(self.add(-value))
    }

    /// Adds years, months and days, normalising overflow like Go's `AddDate`.
    pub fn add_date(&self, years: i32, months: i32, days: i32) -> Time {
        let Some(inner) = self.inner else {
            return self.clone();
        };
        let naive = inner.naive_local();
        let mut year = naive.year() + years;
        let mut month0 = naive.month0() as i32 + months;
        year += month0.div_euclid(12);
        month0 = month0.rem_euclid(12);
        let Some(first) = NaiveDate::from_ymd_opt(year, month0 as u32 + 1, 1) else {
            return self.clone();
        };
        let Some(first) = first.checked_add_days(Days::new(u64::from(naive.day() - 1))) else {
            return self.clone();
        };
        let date = if days >= 0 {
            first.checked_add_days(Days::new(days as u64))
        } else {
            first.checked_sub_days(Days::new((-days) as u64))
        };
        match date
            .and_then(|date| Time::build_wall(date.and_time(naive.time()), self.location.clone()))
        {
            Some(time) => time,
            None => self.clone(),
        }
    }

    /// Converts to UTC, keeping the instant.
    pub fn utc(&self) -> Time {
        let Some(inner) = self.inner else {
            return self.clone();
        };
        Time {
            inner: Some(inner.with_timezone(&Utc).fixed_offset()),
            location: Location::Utc,
            layout: self.layout.clone(),
        }
    }

    /// Converts to the local zone (or the zone set by [`set_time_zone`]).
    pub fn local(&self) -> Time {
        let location = match process_time_zone() {
            Some(zone) => Location::Named(zone),
            None => Location::Local,
        };
        self.with_location(location)
    }

    /// Converts to `location`, keeping the instant.
    pub fn in_location(&self, location: &Location) -> Time {
        self.with_location(location.clone())
    }

    /// Converts to `location`, keeping the instant (Go's `ToLocation`).
    pub fn to_location(&self, location: &Location) -> Time {
        self.with_location(location.clone())
    }

    /// Converts to an IANA zone by name.
    pub fn to_zone(&self, zone: &str) -> Result<Time, Error> {
        let location = load_location(zone)?;
        Ok(self.with_location(location))
    }

    /// Converts to an IANA zone held in a [`Location`].
    pub fn to_zone_by_location(&self, location: &Location) -> Result<Time, Error> {
        match location {
            Location::Named(name) => self.to_zone(name),
            other => Ok(self.with_location(other.clone())),
        }
    }

    /// Rounds to the nearest multiple of `duration` since Go's zero time.
    pub fn round(&self, duration: Duration) -> Time {
        if duration <= 0 {
            return self.clone();
        }
        self.scaled(duration, true)
    }

    /// Truncates down to a multiple of `duration` since Go's zero time.
    pub fn truncate(&self, duration: Duration) -> Time {
        if duration <= 0 {
            return self.clone();
        }
        self.scaled(duration, false)
    }

    /// Returns the start of the minute.
    pub fn start_of_minute(&self) -> Time {
        self.wall_with(|naive| {
            naive
                .with_second(0)
                .and_then(|value| value.with_nanosecond(0))
        })
    }

    /// Returns the start of the hour.
    pub fn start_of_hour(&self) -> Time {
        self.wall_with(|naive| {
            naive
                .with_minute(0)
                .and_then(|value| value.with_second(0))
                .and_then(|value| value.with_nanosecond(0))
        })
    }

    /// Returns the start of the day.
    pub fn start_of_day(&self) -> Time {
        self.wall_with(|naive| {
            NaiveDate::from_ymd_opt(naive.year(), naive.month(), naive.day())
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
    }

    /// Returns the start of the week (Sunday).
    pub fn start_of_week(&self) -> Time {
        let weekday = self.weekday().num_days_from_sunday() as i32;
        self.start_of_day().add_date(0, 0, -weekday)
    }

    /// Returns the start of the month.
    pub fn start_of_month(&self) -> Time {
        self.wall_with(|naive| {
            NaiveDate::from_ymd_opt(naive.year(), naive.month(), 1)
                .and_then(|date| date.and_hms_opt(0, 0, 0))
        })
    }

    /// Returns the start of the quarter.
    pub fn start_of_quarter(&self) -> Time {
        let month = self.start_of_month();
        let offset = (month.month().saturating_sub(1) % 3) as i32;
        month.add_date(0, -offset, 0)
    }

    /// Returns the start of the half-year.
    pub fn start_of_half(&self) -> Time {
        let month = self.start_of_month();
        let offset = (month.month().saturating_sub(1) % 6) as i32;
        month.add_date(0, -offset, 0)
    }

    /// Returns the start of the year.
    pub fn start_of_year(&self) -> Time {
        self.wall_with(|naive| {
            NaiveDate::from_ymd_opt(naive.year(), 1, 1).and_then(|date| date.and_hms_opt(0, 0, 0))
        })
    }

    /// Returns the end of the minute, optionally with nanosecond precision.
    pub fn end_of_minute(&self, nano_precision: bool) -> Time {
        self.start_of_minute()
            .add(M - precision_delta(nano_precision))
    }

    /// Returns the end of the hour, optionally with nanosecond precision.
    pub fn end_of_hour(&self, nano_precision: bool) -> Time {
        self.start_of_hour()
            .add(H - precision_delta(nano_precision))
    }

    /// Returns the end of the day, optionally with nanosecond precision.
    pub fn end_of_day(&self, nano_precision: bool) -> Time {
        let nanos = if nano_precision { 999_999_999 } else { 0 };
        self.wall_with(|naive| {
            NaiveDate::from_ymd_opt(naive.year(), naive.month(), naive.day())
                .and_then(|date| date.and_hms_nano_opt(23, 59, 59, nanos))
        })
    }

    /// Returns the end of the week, optionally with nanosecond precision.
    pub fn end_of_week(&self, nano_precision: bool) -> Time {
        self.start_of_week()
            .add_date(0, 0, 7)
            .add(-precision_delta(nano_precision))
    }

    /// Returns the end of the month, optionally with nanosecond precision.
    pub fn end_of_month(&self, nano_precision: bool) -> Time {
        self.start_of_month()
            .add_date(0, 1, 0)
            .add(-precision_delta(nano_precision))
    }

    /// Returns the end of the quarter, optionally with nanosecond precision.
    pub fn end_of_quarter(&self, nano_precision: bool) -> Time {
        self.start_of_quarter()
            .add_date(0, 3, 0)
            .add(-precision_delta(nano_precision))
    }

    /// Returns the end of the half-year, optionally with nanosecond precision.
    pub fn end_of_half(&self, nano_precision: bool) -> Time {
        self.start_of_half()
            .add_date(0, 6, 0)
            .add(-precision_delta(nano_precision))
    }

    /// Returns the end of the year, optionally with nanosecond precision.
    pub fn end_of_year(&self, nano_precision: bool) -> Time {
        self.start_of_year()
            .add_date(1, 0, 0)
            .add(-precision_delta(nano_precision))
    }

    /// Reports whether two times are the same instant, treating nil as equal
    /// to nil.
    pub fn equal(&self, other: &Time) -> bool {
        self == other
    }

    /// Reports whether the instant is before `other`.
    pub fn before(&self, other: &Time) -> bool {
        self.cmp(other) == Ordering::Less
    }

    /// Reports whether the instant is after `other`. A nil time is never after
    /// anything and a real time is always after nil, matching Go's `After`.
    pub fn after(&self, other: &Time) -> bool {
        match (self.inner, other.inner) {
            (None, _) => false,
            (Some(_), None) => true,
            (Some(a), Some(b)) => a > b,
        }
    }

    /// Compares instants, returning -1, 0 or 1. Nil sorts before real times.
    pub fn compare(&self, other: &Time) -> i32 {
        match self.cmp(other) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    /// Reports whether the instant is within `begin..=end`.
    pub fn between(&self, begin: &Time, end: &Time) -> bool {
        self.cmp(begin) != Ordering::Less && self.cmp(end) != Ordering::Greater
    }

    /// Reports whether the year is a leap year.
    pub fn is_leap_year(&self) -> bool {
        let year = self.year();
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// Returns the zero-based day of the year.
    pub fn day_of_year(&self) -> i32 {
        const DAY_OF_MONTH: [i32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
        let Some(inner) = self.inner else {
            return 0;
        };
        let day = inner.day() as i32;
        let month = inner.month() as usize;
        let base = DAY_OF_MONTH
            .get(month.saturating_sub(1))
            .copied()
            .unwrap_or(0);
        if self.is_leap_year() && month > 2 {
            base + day
        } else {
            base + day - 1
        }
    }

    /// Returns the number of days in the current month.
    pub fn days_in_month(&self) -> i32 {
        let Some(inner) = self.inner else {
            return 0;
        };
        match inner.month() {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => {
                if self.is_leap_year() {
                    29
                } else {
                    28
                }
            }
        }
    }

    /// Returns the ISO week number of the year.
    pub fn weeks_of_year(&self) -> i32 {
        match self.inner {
            Some(inner) => inner.iso_week().week() as i32,
            None => 0,
        }
    }

    /// Formats with a PHP-style `format`, like Go's `Format`.
    pub fn format(&self, format: &str) -> String {
        let Some(parts) = self.layout_parts() else {
            return String::new();
        };
        let chars: Vec<char> = format.chars().collect();
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
            match c {
                'W' => out.push_str(&self.weeks_of_year().to_string()),
                'z' => out.push_str(&self.day_of_year().to_string()),
                't' => out.push_str(&self.days_in_month().to_string()),
                'U' => out.push_str(&self.timestamp().to_string()),
                _ => {
                    if (c as u32) > 255 {
                        out.push(c);
                    } else if let Some(go_layout) = layout::php_go_layout(c) {
                        let result = format_go_layout(&parts, go_layout);
                        match c {
                            'j' => out.push_str(&result.replace("=j=0", "").replace("=j=", "")),
                            'G' => out.push_str(&result.replace("=G=0", "").replace("=G=", "")),
                            'u' => out.push_str(&result.replace("=u=.", "")),
                            'w' => out.push_str(layout::weekday_number(&result)),
                            'N' => {
                                let number = layout::weekday_number(&result);
                                out.push_str(if number == "0" { "7" } else { number });
                            }
                            'S' => out.push_str(layout::month_day_suffix(&result)),
                            _ => out.push_str(&result),
                        }
                    } else {
                        out.push(c);
                    }
                }
            }
            index += 1;
        }
        out
    }

    /// Returns a new time parsed from the formatted result, or `None`.
    pub fn format_new(&self, format: &str) -> Option<Time> {
        if self.is_nil() {
            return None;
        }
        let formatted = self.format(format);
        Time::new_from_str(&formatted)
    }

    /// Returns a new time parsed from the formatted result, or `None`.
    ///
    /// Go's `FormatTo` mutates the receiver; the port returns a new value.
    pub fn format_to(&self, format: &str) -> Option<Time> {
        if self.is_nil() {
            return None;
        }
        self.format_new(format)
    }

    /// Formats with a Go layout, like Go's `Layout`.
    pub fn layout(&self, layout: &str) -> String {
        let Some(parts) = self.layout_parts() else {
            return String::new();
        };
        format_go_layout(&parts, layout)
    }

    /// Formats with a Go layout and parses the result back, or `None`.
    pub fn layout_new(&self, layout: &str) -> Option<Time> {
        if self.is_nil() {
            return None;
        }
        let formatted = self.layout(layout);
        Time::new_from_str_layout(&formatted, layout)
    }

    /// Formats with a Go layout and parses the result back, or `None`.
    ///
    /// Go's `LayoutTo` mutates the receiver; the port returns a new value.
    pub fn layout_to(&self, layout: &str) -> Option<Time> {
        if self.is_nil() {
            return None;
        }
        self.layout_new(layout)
    }

    /// Serializes to JSON as a quoted `"2006-01-02 15:04:05"` string.
    ///
    /// A nil time serializes to `null`, a zero time to `""`.
    pub fn marshal_json(&self) -> String {
        if self.is_nil() {
            return "null".to_string();
        }
        format!("\"{}\"", self.string())
    }

    /// Parses a JSON string value, or clears the value when empty.
    pub fn unmarshal_json(&mut self, data: &str) -> Result<(), Error> {
        if data.is_empty() {
            *self = Time::zero();
            return Ok(());
        }
        let parsed = str_to_time(data.trim_matches('"'), None)?;
        *self = parsed;
        Ok(())
    }

    /// Parses text or a quoted string, like Go's `UnmarshalText`.
    pub fn unmarshal_text(&mut self, data: &[u8]) -> Result<(), Error> {
        let text = String::from_utf8_lossy(data);
        match str_to_time(text.trim_matches('"'), None) {
            Ok(parsed) => {
                *self = parsed;
                Ok(())
            }
            Err(_) => Err(invalid_parameter("invalid time value: %s", &text)),
        }
    }

    /// Scans a database value, like Go's `Scan`.
    pub fn scan(&mut self, value: &Value) -> Result<(), Error> {
        let parsed = match value {
            Value::Null => Time::zero(),
            Value::String(text) => str_to_time(text, None)?,
            Value::Bytes(bytes) => str_to_time(&String::from_utf8_lossy(bytes), None)?,
            Value::Int(number) => Time::new_from_time_stamp(*number),
            Value::UInt(number) => Time::new_from_timestamp(*number as i64),
            _ => Time::zero(),
        };
        *self = parsed;
        Ok(())
    }

    /// Returns a database value, like Go's `Value`.
    ///
    /// Go hands the driver a `time.Time`; the port returns the same formatted
    /// string `String` produces (or the time-only string for year-zero times).
    pub fn value(&self) -> Value {
        if self.is_nil() || self.is_zero() {
            return Value::Null;
        }
        let Some(inner) = self.inner else {
            return Value::Null;
        };
        if inner.year() == 0 {
            Value::String(self.layout("15:04:05"))
        } else {
            Value::String(self.string())
        }
    }

    /// Returns the SQL string form.
    pub fn marshal_sql(&self) -> String {
        self.string()
    }

    /// Returns a deep copy; `None` for nil, matching Go's `DeepCopy`.
    pub fn deep_copy(&self) -> Option<Time> {
        if self.is_nil() {
            None
        } else {
            Some(self.clone())
        }
    }

    /// Builds a time from a wall clock interpreted in `location`.
    fn build_wall(naive: NaiveDateTime, location: Location) -> Option<Time> {
        let inner = match &location {
            Location::Utc => Utc
                .from_local_datetime(&naive)
                .single()
                .map(|value| value.fixed_offset()),
            Location::Fixed { offset_seconds, .. } => FixedOffset::east_opt(*offset_seconds)?
                .from_local_datetime(&naive)
                .single(),
            Location::Local => match Local.from_local_datetime(&naive) {
                LocalResult::Single(value) => Some(value.fixed_offset()),
                LocalResult::Ambiguous(first, _) => Some(first.fixed_offset()),
                LocalResult::None => None,
            },
            Location::Named(name) => {
                let zone = Tz::from_str(name).ok()?;
                match zone.from_local_datetime(&naive) {
                    LocalResult::Single(value) => Some(value.fixed_offset()),
                    LocalResult::Ambiguous(first, _) => Some(first.fixed_offset()),
                    LocalResult::None => None,
                }
            }
        }?;
        Some(Time {
            inner: Some(inner),
            location,
            layout: None,
        })
    }

    /// Converts the stored instant to `location`.
    fn with_location(&self, location: Location) -> Time {
        let Some(inner) = self.inner else {
            return Time {
                inner: None,
                location,
                layout: self.layout.clone(),
            };
        };
        let utc = inner.with_timezone(&Utc);
        match convert_instant(utc, &location) {
            Some(converted) => Time {
                inner: Some(converted),
                location,
                layout: self.layout.clone(),
            },
            None => self.clone(),
        }
    }

    /// Places `utc` in the stored location.
    fn at(&self, utc: DateTime<Utc>) -> Time {
        match convert_instant(utc, &self.location) {
            Some(inner) => Time {
                inner: Some(inner),
                location: self.location.clone(),
                layout: self.layout.clone(),
            },
            None => self.clone(),
        }
    }

    /// Rebuilds the wall clock through `transform`.
    fn wall_with<F>(&self, transform: F) -> Time
    where
        F: FnOnce(NaiveDateTime) -> Option<NaiveDateTime>,
    {
        let Some(inner) = self.inner else {
            return self.clone();
        };
        match transform(inner.naive_local())
            .and_then(|naive| Time::build_wall(naive, self.location.clone()))
        {
            Some(time) => time,
            None => self.clone(),
        }
    }

    /// Rounds or truncates the absolute instant.
    fn scaled(&self, duration: Duration, rounding: bool) -> Time {
        let (Some(inner), Some(anchor)) = (self.inner, zero_naive_utc()) else {
            return self.clone();
        };
        let since = inner.naive_utc().signed_duration_since(anchor);
        let delta =
            i128::from(since.num_seconds()) * 1_000_000_000 + i128::from(since.subsec_nanos());
        let divisor = i128::from(duration);
        let scaled = if rounding {
            (delta + divisor / 2).div_euclid(divisor) * divisor
        } else {
            delta.div_euclid(divisor) * divisor
        };
        let seconds = scaled.div_euclid(1_000_000_000);
        let nanos = scaled.rem_euclid(1_000_000_000);
        let (Ok(seconds), Ok(nanos)) = (i64::try_from(seconds), u32::try_from(nanos)) else {
            return self.clone();
        };
        let Some(naive) = anchor
            .checked_add_signed(TimeDelta::seconds(seconds))
            .and_then(|value| value.with_nanosecond(nanos))
        else {
            return self.clone();
        };
        self.at(Utc.from_utc_datetime(&naive))
    }

    /// Returns the pieces a Go layout formatter needs.
    fn layout_parts(&self) -> Option<LayoutParts> {
        let inner = self.inner?;
        Some(LayoutParts {
            naive: inner.naive_local(),
            offset_seconds: inner.offset().local_minus_utc(),
            zone: self.zone_abbreviation(),
        })
    }

    /// Returns the zone abbreviation for the `MST` token.
    fn zone_abbreviation(&self) -> String {
        match &self.location {
            Location::Utc => "UTC".to_string(),
            Location::Fixed { name, .. } => name.clone().unwrap_or_default(),
            Location::Named(name) => match (self.inner, Tz::from_str(name)) {
                (Some(inner), Ok(zone)) => zone
                    .offset_from_utc_datetime(&inner.naive_utc())
                    .abbreviation()
                    .unwrap_or_default()
                    .to_string(),
                _ => name.clone(),
            },
            Location::Local => self
                .inner
                .map(|inner| offset_short(inner.offset().local_minus_utc()))
                .unwrap_or_default(),
        }
    }
}

/// Returns Go's zero instant as a UTC wall clock.
fn zero_naive_utc() -> Option<NaiveDateTime> {
    NaiveDate::from_ymd_opt(1, 1, 1)?.and_hms_opt(0, 0, 0)
}

/// Returns Go's zero time as a fixed-offset datetime.
fn zero_datetime() -> Option<DateTime<FixedOffset>> {
    Some(DateTime::from_naive_utc_and_offset(
        zero_naive_utc()?,
        FixedOffset::east_opt(0)?,
    ))
}

/// Builds a local time from a Unix second/nanosecond pair.
fn from_unix(sec: i64, nano: i64) -> Time {
    let nano = nano.clamp(0, 999_999_999) as u32;
    match DateTime::from_timestamp(sec, nano) {
        Some(instant) => Time {
            inner: Some(instant.with_timezone(&Local).fixed_offset()),
            location: Location::Local,
            layout: None,
        },
        None => Time::zero(),
    }
}

/// Resolves a wall clock in the local zone into a fixed-offset datetime.
fn local_datetime(naive: &NaiveDateTime) -> Option<DateTime<FixedOffset>> {
    match Local.from_local_datetime(naive) {
        LocalResult::Single(value) => Some(value.fixed_offset()),
        LocalResult::Ambiguous(first, _) => Some(first.fixed_offset()),
        LocalResult::None => None,
    }
}

/// Returns the local offset in seconds at `naive`.
fn local_offset_at(naive: &NaiveDateTime) -> Option<i32> {
    local_datetime(naive).map(|value| value.offset().local_minus_utc())
}

/// Converts a UTC instant to a location's offset.
fn convert_instant(utc: DateTime<Utc>, location: &Location) -> Option<DateTime<FixedOffset>> {
    match location {
        Location::Utc => Some(utc.fixed_offset()),
        Location::Fixed { offset_seconds, .. } => {
            Some(utc.with_timezone(&FixedOffset::east_opt(*offset_seconds)?))
        }
        Location::Local => Some(utc.with_timezone(&Local).fixed_offset()),
        Location::Named(name) => Some(utc.with_timezone(&Tz::from_str(name).ok()?).fixed_offset()),
    }
}

/// Formats an offset as `+HH:MM` for local-zone abbreviations.
fn offset_short(offset_seconds: i32) -> String {
    let sign = if offset_seconds < 0 { '-' } else { '+' };
    let absolute = offset_seconds.unsigned_abs();
    format!("{sign}{:02}:{:02}", absolute / 3600, (absolute % 3600) / 60)
}

/// Returns the precision subtractor used by the `EndOf*` helpers.
fn precision_delta(nano_precision: bool) -> Duration {
    if nano_precision { NS } else { S }
}

/// Loads an IANA zone, treating `""` and `"UTC"` as UTC.
fn load_location(name: &str) -> Result<Location, Error> {
    if name.is_empty() || name.eq_ignore_ascii_case("UTC") {
        return Ok(Location::Utc);
    }
    if name.eq_ignore_ascii_case("Local") {
        return Ok(Location::Local);
    }
    match Tz::from_str(name) {
        Ok(_) => Ok(Location::Named(name.to_string())),
        Err(_) => Err(invalid_parameter(
            "time.LoadLocation failed for name \"%s\"",
            name,
        )),
    }
}

/// Sets the zone [`Time::local`] converts to.
///
/// Unlike Go's `SetTimeZone`, this does not mutate the process `TZ`
/// environment; chrono's local zone is fixed for the process. Calling it twice
/// with different zones returns an error, like Go.
pub fn set_time_zone(zone: &str) -> Result<(), Error> {
    let mut guard = PROCESS_TIME_ZONE
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(current) = guard.as_deref()
        && !current.eq_ignore_ascii_case(zone)
    {
        return Err(Error::new_codef(
            &gcode::CODE_INVALID_OPERATION,
            "process timezone already set using \"%s\"",
            vec![Value::String(current.to_string())],
        ));
    }
    load_location(zone)?;
    *guard = Some(zone.to_string());
    Ok(())
}

/// Returns the zone set by [`set_time_zone`], if any.
fn process_time_zone() -> Option<String> {
    PROCESS_TIME_ZONE
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Builds an invalid-parameter error with one string argument.
fn invalid_parameter(format: &str, value: &str) -> Error {
    Error::new_codef(
        &gcode::CODE_INVALID_PARAMETER,
        format,
        vec![Value::String(value.to_string())],
    )
}

/// Returns whether `s` is a non-empty run of ASCII digits.
fn is_timestamp_str(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

/// Returns the `captures` group `index` as a string, or `""`.
fn capture(caps: &regex::Captures<'_>, index: usize) -> String {
    caps.get(index)
        .map(|matched| matched.as_str().to_string())
        .unwrap_or_default()
}

/// Splits a date string into year, month and day, like Go's `parseDateStr`.
fn parse_date_str(s: &str) -> (i32, u32, u32) {
    let mut parts: Vec<&str> = s.split('-').collect();
    if parts.len() < 3 {
        parts = s.split('/').collect();
    }
    if parts.len() < 3 {
        parts = s.split('.').collect();
    }
    if parts.len() < 3 {
        return (0, 0, 0);
    }
    let second_numeric = !parts[1].is_empty() && parts[1].chars().all(|c| c.is_ascii_digit());
    let atoi = |value: &str| value.parse::<i32>().unwrap_or(0);
    if second_numeric {
        return (atoi(parts[0]), atoi(parts[1]) as u32, atoi(parts[2]) as u32);
    }
    let Some(month) = month_number(parts[1]) else {
        return (0, 0, 0);
    };
    (atoi(parts[2]), month, atoi(parts[0]) as u32)
}

/// Maps a month name to its number.
fn month_number(name: &str) -> Option<u32> {
    let number = match name.to_lowercase().as_str() {
        "jan" | "january" => 1,
        "feb" | "february" => 2,
        "mar" | "march" => 3,
        "apr" | "april" => 4,
        "may" => 5,
        "jun" | "june" => 6,
        "jul" | "july" => 7,
        "aug" | "august" => 8,
        "sep" | "sept" | "september" => 9,
        "oct" | "october" => 10,
        "nov" | "november" => 11,
        "dec" | "december" => 12,
        _ => return None,
    };
    Some(number)
}

/// Converts a string to a [`Time`], using `format` when given.
///
/// An empty string returns the zero time, like Go.
pub fn str_to_time(str: &str, format: Option<&str>) -> Result<Time, Error> {
    if str.is_empty() {
        return Ok(Time::zero());
    }
    if let Some(format) = format {
        return str_to_time_format(str, format);
    }
    if is_timestamp_str(str) {
        let timestamp = str.parse::<i64>().unwrap_or(0);
        return Ok(Time::new_from_time_stamp(timestamp));
    }

    let datetime1 = TIME_REGEX1
        .as_ref()
        .and_then(|regex| regex.captures(str))
        .filter(|caps| !capture(caps, 1).is_empty());
    let datetime2 = TIME_REGEX2
        .as_ref()
        .and_then(|regex| regex.captures(str))
        .filter(|caps| !capture(caps, 1).is_empty());
    let time_only = TIME_REGEX3
        .as_ref()
        .and_then(|regex| regex.captures(str))
        .filter(|caps| !capture(caps, 1).is_empty());

    if let Some(caps) = datetime1.or(datetime2) {
        return parse_matched_datetime(str, &caps);
    }
    if let Some(caps) = time_only {
        return parse_matched_time(str, &caps);
    }
    Err(invalid_parameter(
        "unsupported time converting for string \"%s\"",
        str,
    ))
}

/// Parses the datetime branch of [`str_to_time`].
fn parse_matched_datetime(str: &str, caps: &regex::Captures<'_>) -> Result<Time, Error> {
    let (year, month, day) = parse_date_str(&capture(caps, 1));
    let mut hour = 0i64;
    let mut minute = 0i64;
    let mut second = 0i64;
    let mut nanosecond = 0i64;
    let time_part = capture(caps, 2);
    if !time_part.is_empty() {
        let parts: Vec<&str> = time_part.split(':').collect();
        if let Some(value) = parts.first().filter(|value| !value.is_empty()) {
            hour = value.parse::<i64>().unwrap_or(0);
        }
        if let Some(value) = parts.get(1).filter(|value| !value.is_empty()) {
            minute = value.parse::<i64>().unwrap_or(0);
        }
        if let Some(value) = parts.get(2).filter(|value| !value.is_empty()) {
            second = value.parse::<i64>().unwrap_or(0);
        }
        if hour > 23 || minute > 59 || second > 59 {
            return Err(invalid_parameter("invalid time string \"%s\"", str));
        }
    }
    let fraction = capture(caps, 3);
    if !fraction.is_empty() {
        nanosecond = fraction.parse::<i64>().unwrap_or(0);
        for _ in 0..9usize.saturating_sub(fraction.len()) {
            nanosecond = nanosecond.saturating_mul(10);
        }
    }

    let mut zone_string = capture(caps, 6);
    let zone_marker = capture(caps, 4);
    if !zone_marker.is_empty() && zone_string.is_empty() {
        zone_string = "000000".to_string();
    }
    let mut location = Location::Local;
    let naive = naive_from(year, month, day, hour, minute, second, nanosecond);
    if !zone_string.is_empty() {
        let cleaned: String = zone_string.chars().filter(|c| *c != ':').collect();
        let trimmed = cleaned.trim_start_matches(['+', '-']);
        if trimmed.len() <= 6 {
            let padded = format!("{trimmed:0<6}");
            let hours = padded
                .get(0..2)
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let minutes = padded
                .get(2..4)
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            let seconds = padded
                .get(4..6)
                .and_then(|value| value.parse::<i32>().ok())
                .unwrap_or(0);
            if hours > 24 || minutes > 59 || seconds > 59 {
                return Err(invalid_parameter(
                    "invalid zone string \"%s\"",
                    &zone_string,
                ));
            }
            let sign = if capture(caps, 5) == "+" { 1 } else { -1 };
            let zone_offset = sign * (hours * 3600 + minutes * 60 + seconds);
            let local_offset = naive
                .as_ref()
                .and_then(local_offset_at)
                .unwrap_or(zone_offset);
            if local_offset != zone_offset {
                location = Location::Fixed {
                    name: None,
                    offset_seconds: zone_offset,
                };
            }
        }
    }
    if month == 0 || day == 0 {
        return Err(invalid_parameter("invalid time string \"%s\"", str));
    }
    let Some(naive) = naive else {
        return Err(invalid_parameter("invalid time string \"%s\"", str));
    };
    Time::build_wall(naive, location)
        .ok_or_else(|| invalid_parameter("invalid time string \"%s\"", str))
}

/// Parses the time-only branch of [`str_to_time`].
fn parse_matched_time(str: &str, caps: &regex::Captures<'_>) -> Result<Time, Error> {
    let hour = capture(caps, 1).parse::<i64>().unwrap_or(0);
    let minute = capture(caps, 2).parse::<i64>().unwrap_or(0);
    let second = capture(caps, 3).parse::<i64>().unwrap_or(0);
    let fraction = capture(caps, 4);
    let mut nanosecond = fraction.parse::<i64>().unwrap_or(0);
    for _ in 0..9usize.saturating_sub(fraction.len()) {
        nanosecond = nanosecond.saturating_mul(10);
    }
    if hour > 23 || minute > 59 || second > 59 {
        return Err(invalid_parameter("invalid time string \"%s\"", str));
    }
    let Some(naive) = naive_from(0, 1, 1, hour, minute, second, nanosecond) else {
        return Err(invalid_parameter("invalid time string \"%s\"", str));
    };
    Time::build_wall(naive, Location::Local)
        .ok_or_else(|| invalid_parameter("invalid time string \"%s\"", str))
}

/// Builds a naive wall clock, returning `None` for out-of-range fields.
fn naive_from(
    year: i32,
    month: u32,
    day: u32,
    hour: i64,
    minute: i64,
    second: i64,
    nanosecond: i64,
) -> Option<NaiveDateTime> {
    let date = NaiveDate::from_ymd_opt(year, month, day)?;
    date.and_hms_nano_opt(
        u32::try_from(hour).ok()?,
        u32::try_from(minute).ok()?,
        u32::try_from(second).ok()?,
        u32::try_from(nanosecond).ok()?,
    )
}

/// Parses `str` with a PHP-style `format`.
pub fn str_to_time_format(str: &str, format: &str) -> Result<Time, Error> {
    str_to_time_layout(str, &format_to_go_time(format))
}

/// Parses `str` with a Go `layout`.
pub fn str_to_time_layout(str: &str, layout: &str) -> Result<Time, Error> {
    let message = || {
        Error::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            "time.ParseInLocation failed for layout \"%s\" and value \"%s\"",
            vec![
                Value::String(layout.to_string()),
                Value::String(str.to_string()),
            ],
        )
    };
    let parsed = layout::parse_go_layout(str, layout).ok_or_else(message)?;
    let location = match parsed.offset_seconds {
        Some(offset) => Location::Fixed {
            name: None,
            offset_seconds: offset,
        },
        None => Location::Local,
    };
    Time::build_wall(parsed.naive, location).ok_or_else(message)
}

/// Parses `value` using a PHP-style `format` (an alias for
/// [`str_to_time_format`]).
pub fn parse(value: &str, format: &str) -> Result<Time, Error> {
    str_to_time_format(value, format)
}

/// Parses `value` using a Go `layout` (an alias for [`str_to_time_layout`]).
pub fn parse_layout(value: &str, layout: &str) -> Result<Time, Error> {
    str_to_time_layout(value, layout)
}

/// Converts `str_time` from `from_zone` to `to_zone`.
pub fn convert_zone(str_time: &str, to_zone: &str, from_zone: Option<&str>) -> Result<Time, Error> {
    let mut time = str_to_time(str_time, None)?;
    if let Some(from_zone) = from_zone {
        let location = load_location(from_zone)?;
        let Some(inner) = time.inner else {
            return Err(invalid_parameter("invalid time string \"%s\"", str_time));
        };
        time = Time::build_wall(inner.naive_local(), location)
            .ok_or_else(|| invalid_parameter("invalid time string \"%s\"", str_time))?;
    }
    let location = load_location(to_zone)?;
    Ok(time.to_location(&location))
}

/// Finds the first time in `content`, optionally constrained to `formats`.
pub fn parse_time_from_content(content: &str, formats: Option<&[&str]>) -> Option<Time> {
    if let Some(formats) = formats {
        for format in formats {
            let pattern = format_to_regex_pattern(format);
            let Ok(regex) = Regex::new(&pattern) else {
                continue;
            };
            if let Some(matched) = regex.find(content) {
                return Time::new_from_str_format(matched.as_str(), format);
            }
        }
        return None;
    }
    let found = TIME_REGEX1
        .as_ref()
        .and_then(|regex| regex.find(content))
        .or_else(|| TIME_REGEX2.as_ref().and_then(|regex| regex.find(content)))
        .or_else(|| TIME_REGEX3.as_ref().and_then(|regex| regex.find(content)));
    found.and_then(|matched| Time::new_from_str(trim_content(matched.as_str())))
}

/// Trims the characters Go's `ParseTimeFromContent` removes around a match.
fn trim_content(value: &str) -> &str {
    value.trim_matches(|c| matches!(c, '.' | '/' | '_' | '-' | ' ' | '\n' | '\r'))
}

/// Returns the current Unix timestamp in seconds.
pub fn timestamp() -> i64 {
    Time::now().timestamp()
}

/// Returns the current Unix timestamp in milliseconds.
pub fn timestamp_milli() -> i64 {
    Time::now().timestamp_milli()
}

/// Returns the current Unix timestamp in microseconds.
pub fn timestamp_micro() -> i64 {
    Time::now().timestamp_micro()
}

/// Returns the current Unix timestamp in nanoseconds.
pub fn timestamp_nano() -> i64 {
    Time::now().timestamp_nano()
}

/// Returns the current timestamp in seconds as a string.
pub fn timestamp_str() -> String {
    Time::now().timestamp_str()
}

/// Returns the current timestamp in milliseconds as a string.
pub fn timestamp_milli_str() -> String {
    Time::now().timestamp_milli_str()
}

/// Returns the current timestamp in microseconds as a string.
pub fn timestamp_micro_str() -> String {
    Time::now().timestamp_micro_str()
}

/// Returns the current timestamp in nanoseconds as a string.
pub fn timestamp_nano_str() -> String {
    Time::now().timestamp_nano_str()
}

/// Returns the current date as `"2006-01-02"`.
pub fn date() -> String {
    Time::now().layout("2006-01-02")
}

/// Returns the current datetime as `"2006-01-02 15:04:05"`.
pub fn datetime() -> String {
    Time::now().layout("2006-01-02 15:04:05")
}

/// Returns the current time in ISO 8601 form.
pub fn iso8601() -> String {
    Time::now().layout("2006-01-02T15:04:05-07:00")
}

/// Returns the current time in RFC 822 form.
pub fn rfc822() -> String {
    Time::now().layout("Mon, 02 Jan 06 15:04 MST")
}

/// Measures the cost of `f` in nanoseconds, like Go's `FuncCost`.
pub fn func_cost<F: FnOnce()>(f: F) -> Duration {
    let start = StdInstant::now();
    f();
    let elapsed = start.elapsed().as_nanos().min(i64::MAX as u128);
    elapsed as i64
}

/// Parses a duration string, supporting Go's units plus `d` for days.
///
/// A duration is a possibly signed sequence of decimal numbers with an
/// optional fraction and a unit suffix, such as `"300ms"`, `"-1.5h"`, `"1d"`
/// or `"2h45m"`.
pub fn parse_duration(s: &str) -> Result<Duration, Error> {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_digit()) {
        let value = s
            .parse::<i64>()
            .map_err(|_| invalid_parameter("strconv.ParseInt failed for string \"%s\"", s))?;
        return Ok(value);
    }
    if let Some(day) = parse_day_duration(s) {
        return day;
    }
    parse_std_duration(s)
        .map_err(|_| invalid_parameter("time.ParseDuration failed for string \"%s\"", s))
}

/// Expands a leading `Nd` component, as Go's `ParseDuration` wrapper does.
fn parse_day_duration(s: &str) -> Option<Result<Duration, Error>> {
    let bytes = s.as_bytes();
    let mut index = 0;
    let sign = match bytes.first() {
        Some(b'+') => {
            index = 1;
            1i64
        }
        Some(b'-') => {
            index = 1;
            -1i64
        }
        _ => 1i64,
    };
    let number_start = index;
    while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
        index += 1;
    }
    if index == number_start || index >= bytes.len() || !matches!(bytes[index], b'd' | b'D') {
        return None;
    }
    let number = &s[number_start..index];
    let rest = &s[index + 1..];
    let days = match parse_std_duration(&format!("{number}h")) {
        Ok(value) => value,
        Err(_) => {
            return Some(Err(invalid_parameter(
                "time.ParseDuration failed for string \"%s\"",
                s,
            )));
        }
    };
    if days > i64::MAX / 24 {
        return Some(Err(invalid_parameter(
            "duration out of range for string \"%s\"",
            s,
        )));
    }
    let total_days = days.checked_mul(24)?;
    let rebuilt = if sign < 0 {
        format!("-{}{}", duration_string(total_days), rest)
    } else {
        format!("{}{}", duration_string(total_days), rest)
    };
    Some(
        parse_std_duration(&rebuilt)
            .map_err(|_| invalid_parameter("time.ParseDuration failed for string \"%s\"", s)),
    )
}

/// Parses a duration using Go's standard units (no days).
fn parse_std_duration(s: &str) -> Result<Duration, ()> {
    let bytes = s.as_bytes();
    let mut index = 0;
    let mut negative = false;
    match bytes.first() {
        Some(b'+') => index = 1,
        Some(b'-') => {
            negative = true;
            index = 1;
        }
        _ => {}
    }
    if index == bytes.len() {
        return Err(());
    }
    let mut total: u64 = 0;
    while index < bytes.len() {
        let (value, fraction, scale, next) = parse_duration_number(bytes, index)?;
        index = next;
        let unit = duration_unit(bytes, &mut index)?;
        if value > u64::MAX / unit {
            return Err(());
        }
        let mut part = value * unit;
        if fraction > 0 {
            let frac = (fraction as u128 * unit as u128 / scale as u128) as u64;
            part = part.checked_add(frac).ok_or(())?;
        }
        total = total.checked_add(part).ok_or(())?;
        if total > i64::MAX as u64 + u64::from(negative) {
            return Err(());
        }
    }
    let signed = if negative {
        -(total as i128)
    } else {
        total as i128
    };
    i64::try_from(signed).map_err(|_| ())
}

/// Parses the numeric part of a duration, returning value, fraction, scale
/// and the next index.
fn parse_duration_number(bytes: &[u8], start: usize) -> Result<(u64, u64, u64, usize), ()> {
    let mut index = start;
    let mut value = 0u64;
    let mut digits = 0usize;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        value = value.checked_mul(10).ok_or(())?;
        value = value
            .checked_add(u64::from(bytes[index] - b'0'))
            .ok_or(())?;
        index += 1;
        digits += 1;
    }
    let mut fraction = 0u64;
    let mut scale = 1u64;
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            fraction = fraction.checked_mul(10).ok_or(())?;
            fraction = fraction
                .checked_add(u64::from(bytes[index] - b'0'))
                .ok_or(())?;
            scale = scale.checked_mul(10).ok_or(())?;
            index += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Err(());
    }
    Ok((value, fraction, scale, index))
}

/// Returns the nanosecond multiplier for a duration unit.
fn duration_unit(bytes: &[u8], index: &mut usize) -> Result<u64, ()> {
    let rest = &bytes[*index..];
    let (unit, length) = if rest.starts_with(b"ns") {
        (1, 2)
    } else if rest.starts_with(b"us") {
        (1_000, 2)
    } else if rest.starts_with("µs".as_bytes()) {
        (1_000, "µs".len())
    } else if rest.starts_with("μs".as_bytes()) {
        (1_000, "μs".len())
    } else if rest.starts_with(b"ms") {
        (1_000_000, 2)
    } else if rest.starts_with(b"s") {
        (1_000_000_000, 1)
    } else if rest.starts_with(b"m") {
        (60 * 1_000_000_000, 1)
    } else if rest.starts_with(b"h") {
        (3_600 * 1_000_000_000, 1)
    } else {
        return Err(());
    };
    *index += length;
    Ok(unit)
}

/// Renders a duration the way Go's `Duration.String` does.
pub fn duration_string(duration: Duration) -> String {
    let negative = duration < 0;
    let magnitude = duration.unsigned_abs();
    let mut out = if magnitude < S as u64 {
        if magnitude == 0 {
            return "0s".to_string();
        }
        let (unit, precision) = if magnitude < US as u64 {
            ("ns", 0)
        } else if magnitude < MS as u64 {
            ("µs", 3)
        } else {
            ("ms", 6)
        };
        let (whole, fraction) = split_fraction(magnitude, precision);
        format!("{whole}{fraction}{unit}")
    } else {
        let seconds_total = magnitude / S as u64;
        let nanos = magnitude % S as u64;
        let frac = fraction_text(nanos, 9);
        let mut value = format!("{}{}s", seconds_total % 60, frac);
        let mut minutes = seconds_total / 60;
        if minutes > 0 {
            let hours = minutes / 60;
            minutes %= 60;
            value = format!("{minutes}m{value}");
            if hours > 0 {
                value = format!("{hours}h{value}");
            }
        }
        value
    };
    if negative {
        out.insert(0, '-');
    }
    out
}

/// Splits `value` into an integer part and a trimmed fractional string.
fn split_fraction(value: u64, precision: u32) -> (u64, String) {
    let mut scale = 1u64;
    for _ in 0..precision {
        scale *= 10;
    }
    let whole = value / scale;
    let fraction = value % scale;
    (whole, fraction_text(fraction, precision))
}

/// Renders the fractional digits of `fraction` with `precision`, or `""`.
fn fraction_text(fraction: u64, precision: u32) -> String {
    if precision == 0 || fraction == 0 {
        return String::new();
    }
    let mut text = format!("{fraction:0width$}", width = precision as usize);
    while text.ends_with('0') {
        text.pop();
    }
    format!(".{text}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_core::Value;

    /// Parses a string, panicking with the input on failure (tests only).
    fn parsed(value: &str) -> Time {
        Time::new_from_str(value).unwrap_or_else(|| panic!("failed to parse {value:?}"))
    }

    /// Parses a PHP-formatted string (tests only).
    fn parsed_format(value: &str, format: &str) -> Time {
        Time::new_from_str_format(value, format)
            .unwrap_or_else(|| panic!("failed to parse {value:?} with {format:?}"))
    }

    /// Parses the wall clock in Asia/Shanghai, independent of the host zone.
    fn shanghai(value: &str) -> Time {
        let with_offset = format!("{value}+08:00");
        Time::new_from_str(&with_offset)
            .and_then(|time| time.to_zone("Asia/Shanghai").ok())
            .unwrap_or_else(|| panic!("failed to build Shanghai time from {value:?}"))
    }

    /// Formats a Unix second count the way Go's local `time.Unix` prints it.
    fn local_seconds(timestamp: i64) -> String {
        Local
            .timestamp_opt(timestamp, 0)
            .single()
            .map(|time| time.format("%Y-%m-%d %H:%M:%S").to_string())
            .unwrap_or_default()
    }

    // From Test_TimestampStr.
    #[test]
    fn test_timestamp_str() {
        assert!(!timestamp_milli_str().is_empty());
        assert!(!timestamp_micro_str().is_empty());
        assert!(!timestamp_nano_str().is_empty());
    }

    // From Test_Nanosecond.
    #[test]
    fn test_nanosecond() {
        let nanos = timestamp_nano();
        assert_eq!(nanos, Time::new_from_timestamp_nano(nanos).timestamp_nano());
    }

    // From Test_Microsecond.
    #[test]
    fn test_microsecond() {
        let micros = timestamp_micro();
        assert_eq!(
            micros,
            Time::new_from_timestamp_micro(micros).timestamp_micro()
        );
    }

    // From Test_Millisecond.
    #[test]
    fn test_millisecond() {
        let millis = timestamp_milli();
        assert_eq!(
            millis,
            Time::new_from_timestamp_milli(millis).timestamp_milli()
        );
    }

    // From Test_Second.
    #[test]
    fn test_second() {
        let seconds = timestamp();
        assert_eq!(seconds, Time::new_from_timestamp(seconds).timestamp());
    }

    // From Test_TimestampOfBefore1970.
    #[test]
    fn test_timestamp_of_before_1970() {
        let time = Time::new_from_timestamp_nano(-1_500_000_500);
        assert_eq!(time.timestamp(), -2);
        assert_eq!(time.timestamp_milli(), -1501);
        assert_eq!(time.timestamp_micro(), -1_500_001);
    }

    // From Test_Date.
    #[test]
    fn test_date() {
        assert_eq!(date(), Time::now().layout("2006-01-02"));
    }

    // From Test_Datetime.
    #[test]
    fn test_datetime() {
        let datetime = datetime();
        let time = parsed_format(&datetime, "Y-m-d H:i:s");
        assert_eq!(datetime, time.layout("2006-01-02 15:04:05"));

        let empty = str_to_time("", None).expect("empty string parses");
        assert!(empty.is_zero());
        assert!(str_to_time("2006-01", None).is_err());
    }

    // From Test_ISO8601.
    #[test]
    fn test_iso8601() {
        let value = iso8601();
        assert!(Time::new_from_str_layout(&value, "2006-01-02T15:04:05-07:00").is_some());
    }

    // From Test_RFC822: the local zone is the numeric offset in the port.
    #[test]
    fn test_rfc822() {
        let value = rfc822();
        assert!(value.contains(", "));
        assert!(value.len() >= 10);
    }

    // From Test_StrToTime.
    #[test]
    fn test_str_to_time() {
        let date_times = [
            ("2006-01-02 15:04:05", "2006-01-02 15:04:05"),
            ("2006/01/02 15:04:05", "2006-01-02 15:04:05"),
            ("2006.01.02 15:04:05.000", "2006-01-02 15:04:05"),
            ("2006.01.02 - 15:04:05", "2006-01-02 15:04:05"),
            ("2006.01.02 15:04:05 +0800 CST", "2006-01-02 15:04:05"),
            ("2006-01-02T12:05:05+05:01", "2006-01-02 12:05:05"),
            ("2006-01-02T02:03:05-05:01", "2006-01-02 02:03:05"),
            ("2006-01-02T15:04:05", "2006-01-02 15:04:05"),
            ("02-jan-2006 15:04:05", "2006-01-02 15:04:05"),
            ("02/jan/2006 15:04:05", "2006-01-02 15:04:05"),
            ("02.jan.2006 15:04:05", "2006-01-02 15:04:05"),
            ("02.jan.2006:15:04:05", "2006-01-02 15:04:05"),
        ];
        for (item, expected) in date_times {
            let time = parsed(item);
            assert_eq!(time.format("Y-m-d H:i:s"), expected, "{item}");
        }

        let dates = ["2006.01.02", "2006.01.02 00:00", "2006.01.02 00:00:00.000"];
        for item in dates {
            let time = parsed(item);
            assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 00:00:00", "{item}");
        }

        let time_only = parsed("16:12:01");
        assert_eq!(time_only.string(), "16:12:01");
        let time_only = parsed("16:12:01.789");
        assert_eq!(time_only.format("Y-m-d H:i:s.u"), "0000-01-01 16:12:01.789");

        let formats = [
            ("Y-m-d H:i:s", "2007-01-02 15:04:05"),
            ("\\T\\i\\m\\e Y-m-d H:i:s", "Time 2007-01-02 15:04:05"),
            ("Y-m-d H:i:s\\", "2007-01-02 15:04:05"),
            ("Y-m-j G:i:s.u", "2007-01-02 15:04:05"),
            ("Y-m-j G:i:su", "2007-01-02 15:04:05"),
        ];
        for (format, value) in formats {
            let time = parsed_format(value, format);
            assert_eq!(
                time.format("Y-m-d H:i:s.u"),
                "2007-01-02 15:04:05.000",
                "{format}"
            );
        }

        for item in ["2006.01", "06..02"] {
            assert!(str_to_time(item, None).is_err(), "{item}");
        }

        let special = [
            ("2006-01-02 8:04:05", "2006-01-02 08:04:05"),
            ("2006-01-02 8:4:05", "2006-01-02 08:04:05"),
            ("2006-01-02 8:4:5", "2006-01-02 08:04:05"),
            ("2006-01-02 8:04:05.000", "2006-01-02 08:04:05"),
            ("2006/01/02 8:4:5", "2006-01-02 08:04:05"),
            ("2006.01.02 8:4:5.000", "2006-01-02 08:04:05"),
            ("2006.01.02 - 8:4:5", "2006-01-02 08:04:05"),
            ("2006.01.02 8:4:5 +0800 CST", "2006-01-02 08:04:05"),
            ("2006-01-02T5:5:5+05:01", "2006-01-02 05:05:05"),
            ("2006-01-01T19:3:5-05:01", "2006-01-01 19:03:05"),
            ("2006-01-02T8:4:5", "2006-01-02 08:04:05"),
            ("02-jan-2006 8:4:5", "2006-01-02 08:04:05"),
            ("02/jan/2006 8:4:5", "2006-01-02 08:04:05"),
            ("02.jan.2006 8:4:5", "2006-01-02 08:04:05"),
            ("02.jan.2006:8:4:5", "2006-01-02 08:04:05"),
        ];
        for (item, expected) in special {
            let time = parsed(item);
            assert_eq!(time.format("Y-m-d H:i:s"), expected, "{item}");
        }

        for item in [
            "2006-01-02 28:4:5",
            "2006-01-02 8:60:5",
            "2006-01-02 8:4:60",
            "28:20:20",
            "8:60:20",
            "8:20:60",
        ] {
            assert!(str_to_time(item, None).is_err(), "{item}");
        }

        assert!(str_to_time("2006-01-02 15:04:05", Some("aabbccdd")).is_err());
    }

    // From Test_StrToTime_ZoneOffsetWithDST.
    #[test]
    fn test_str_to_time_zone_offset_with_dst() {
        let time = parsed("1988-07-01T12:00:00+08:00");
        assert_eq!(time.utc().format("Y-m-d H:i:s"), "1988-07-01 04:00:00");
        let time = parsed("1988-07-01T12:00:00+09:00");
        assert_eq!(time.utc().format("Y-m-d H:i:s"), "1988-07-01 03:00:00");
    }

    // From Test_ConvertZone.
    #[test]
    fn test_convert_zone() {
        let now = Time::now_utc();
        let zone = "America/Los_Angeles";
        let converted = convert_zone(&now.layout("2006-01-02 15:04:05"), zone, Some(""))
            .expect("convert to Los Angeles");
        assert_eq!(converted.location().name(), zone);
        assert_eq!(converted.timestamp(), now.timestamp());

        assert!(convert_zone("06..02 15:04:05", "errZone", Some("")).is_err());
        assert!(convert_zone(&now.layout("2006-01-02 15:04:05"), "errZone", Some("")).is_err());
        assert!(
            convert_zone(
                &now.layout("2006-01-02 15:04:05"),
                "errZone",
                Some("errZone")
            )
            .is_err()
        );
    }

    // From Test_ParseDuration.
    #[test]
    fn test_parse_duration() {
        let cases = [
            ("1d", "24h0m0s"),
            ("1d2h3m", "26h3m0s"),
            ("-1d2h3m", "-26h3m0s"),
            ("3m", "3m0s"),
            ("1.5d", "36h0m0s"),
            ("0.5d", "12h0m0s"),
            (".5d", "12h0m0s"),
            ("-1.5d", "-36h0m0s"),
            ("+1d", "24h0m0s"),
            ("+1d2h", "26h0m0s"),
            ("-0d1h", "-1h0m0s"),
            ("1.25D", "30h0m0s"),
            ("0.5d1h", "13h0m0s"),
        ];
        for (input, expected) in cases {
            let value = parse_duration(input).expect("duration parses");
            assert_eq!(duration_string(value), expected, "{input}");
        }

        for input in ["-1dd2h3m", "1..5d", "1.5.2d", "2d3", "--1d", "1000000000d"] {
            assert!(parse_duration(input).is_err(), "{input}");
        }

        let value = parse_duration("106751d").expect("largest day count");
        assert_eq!(duration_string(value), "2562024h0m0s");
        let value = parse_duration("-106751d").expect("largest negative day count");
        assert_eq!(duration_string(value), "-2562024h0m0s");
        for input in ["106752d", "-106752d"] {
            assert!(parse_duration(input).is_err(), "{input}");
        }
    }

    // From Test_ParseTimeFromContent.
    #[test]
    fn test_parse_time_from_content() {
        let time = parse_time_from_content(
            "我是中文2006-01-02 15:04:05我也是中文",
            Some(&["Y-m-d H:i:s"]),
        )
        .expect("content with format");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");

        let time = parse_time_from_content("我是中文2006-01-02 15:04:05我也是中文", None)
            .expect("content without format");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");

        let time = parse_time_from_content("我是中文02.jan.2006 15:04:05我也是中文", None)
            .expect("month-name content");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");

        assert!(parse_time_from_content("我是中文", Some(&["Y-m-d H:i:s"])).is_none());

        let time = parse_time_from_content("2021-1-27 9:10:24", Some(&["Y-n-d g:i:s"]))
            .expect("short content");
        assert_eq!(time.string(), "2021-01-27 09:10:24");
    }

    // From Test_New.
    #[test]
    fn test_new() {
        assert!(Time::new().is_zero());
        assert_eq!(Time::new().string(), "");

        let now = Time::now();
        let from_string = Time::new_from_str(&now.string()).expect("string round trip");
        assert_eq!(
            from_string.layout("2006-01-02 15:04:05"),
            now.layout("2006-01-02 15:04:05")
        );

        let from_micro_string =
            Time::new_from_str(&now.timestamp_micro_str()).expect("micro timestamp string");
        assert_eq!(
            from_micro_string.layout("2006-01-02 15:04:05"),
            now.layout("2006-01-02 15:04:05")
        );

        let from_micro = Time::new_from_time_stamp(now.timestamp_micro());
        assert_eq!(
            from_micro.layout("2006-01-02 15:04:05"),
            now.layout("2006-01-02 15:04:05")
        );

        let short = parsed("2021-2-9 08:01:21");
        assert_eq!(short.format("Y-m-d H:i:s"), "2021-02-09 08:01:21");
        let short = parsed_format("2021-02-09 08:01:21", "Y-m-d H:i:s");
        assert_eq!(short.format("Y-m-d H:i:s"), "2021-02-09 08:01:21");

        for item in ["2006-01.02", "2006.01-02"] {
            assert!(Time::new_from_str(item).is_none(), "{item}");
        }
    }

    // From Test_Nil.
    #[test]
    fn test_nil() {
        assert_eq!(Time::nil().string(), "");
        assert_eq!(Time::zero().string(), "");
        assert!(Time::default().is_nil());
    }

    // From Test_NewFromStr.
    #[test]
    fn test_new_from_str() {
        let time = Time::new_from_str("2006-01-02 15:04:05").expect("parse");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");
        assert!(Time::new_from_str("2006.0102").is_none());
    }

    // From Test_String.
    #[test]
    fn test_string() {
        let time = parsed("2006-01-02 15:04:05");
        assert_eq!(time.string(), "2006-01-02 15:04:05");
        assert_eq!(format!("{}", time), "2006-01-02 15:04:05");
        assert_eq!(format!("{{{}}}", time.string()), "{2006-01-02 15:04:05}");
    }

    // From Test_NewFromStrFormat.
    #[test]
    fn test_new_from_str_format() {
        let time = parsed_format("2006-01-02 15:04:05", "Y-m-d H:i:s");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");
        assert!(Time::new_from_str_format("2006-01-02 15:04:05", "aabbcc").is_none());

        let time = Time::new_from_str_format("2019/2/1", "Y/n/j").expect("parse");
        assert_eq!(time.format("Y-m-d"), "2019-02-01");
        let time = Time::new_from_str_format("2019/10/12", "Y/n/j").expect("parse");
        assert_eq!(time.format("Y-m-d"), "2019-10-12");
    }

    // From Test_NewFromStrLayout.
    #[test]
    fn test_new_from_str_layout() {
        let time =
            Time::new_from_str_layout("2006-01-02 15:04:05", "2006-01-02 15:04:05").expect("parse");
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:05");
        assert!(Time::new_from_str_layout("2006-01-02 15:04:05", "aabbcc").is_none());
    }

    // From Test_NewFromTimeStamp.
    #[test]
    fn test_new_from_time_stamp() {
        let time = Time::new_from_time_stamp(1554459846000);
        assert_eq!(time.string(), local_seconds(1554459846));

        let time = Time::new_from_time_stamp(0);
        assert_eq!(time.format("Y-m-d H:i:s"), "0001-01-01 00:00:00");

        let time = Time::new_from_time_stamp(155445984);
        assert_eq!(time.string(), local_seconds(155445984));
    }

    // From Test_Time_Second plus the fractional accessors.
    #[test]
    fn test_time_second() {
        let time = parsed("2020-12-12 18:24:06.123456789");
        assert_eq!(time.second(), 6);
        assert_eq!(time.minute(), 24);
        assert_eq!(time.hour(), 18);
        assert_eq!(time.day(), 12);
        assert_eq!(time.month(), 12);
        assert_eq!(time.year(), 2020);
        assert_eq!(time.nanosecond(), 123_456_789);
        assert_eq!(time.microsecond(), 123_456);
        assert_eq!(time.millisecond(), 123);
    }

    // From Test_Time_IsZero.
    #[test]
    fn test_time_is_zero() {
        assert!(Time::nil().is_zero());
        assert!(Time::zero().is_zero());
        assert!(!parsed("00:00:00").is_zero());
    }

    // From Test_Time_AddStr.
    #[test]
    fn test_time_add_str() {
        let time = parsed("2018-08-08 08:08:08");
        assert!(time.add_str("10T").is_err());
    }

    // From Test_Time_Equal.
    #[test]
    fn test_time_equal() {
        let nil = Time::nil();
        let real = Time::new();
        assert!(!nil.equal(&real));
        assert!(nil.equal(&nil));
        assert!(!real.equal(&nil));
        assert!(
            parsed("2018-08-08 08:08:08").equal(&parsed("2018-08-08 00:00:08").add(8 * H + 8 * M))
        );
    }

    // From Test_Time_After.
    #[test]
    fn test_time_after() {
        let nil = Time::nil();
        let real = Time::new();
        assert!(!nil.after(&real));
        assert!(real.after(&nil));
    }

    // From Test_Time_Sub.
    #[test]
    fn test_time_sub() {
        let nil = Time::nil();
        let real = Time::new();
        assert_eq!(nil.sub(&real), 0);
        assert_eq!(real.sub(&nil), 0);
        let later = parsed("2018-08-08 08:08:10");
        let earlier = parsed("2018-08-08 08:08:08");
        assert_eq!(later.sub(&earlier), 2 * S);
    }

    // From Test_Clone.
    #[test]
    fn test_clone() {
        let time = Time::now();
        let clone = time.clone();
        assert_eq!(time.timestamp_nano(), clone.timestamp_nano());
    }

    // From Test_Add.
    #[test]
    fn test_add() {
        let time = parsed("2006-01-02 15:04:05").add(S);
        assert_eq!(time.format("Y-m-d H:i:s"), "2006-01-02 15:04:06");
    }

    // From Test_ToZone.
    #[test]
    fn test_to_zone() {
        let time = Time::now()
            .to_zone("America/Los_Angeles")
            .expect("Los Angeles");
        assert_eq!(time.location().name(), "America/Los_Angeles");
        let time = time.to_location(&Location::named("Asia/Shanghai"));
        assert_eq!(time.location().name(), "Asia/Shanghai");
        assert!(Time::now().to_zone("errZone").is_err());
        assert_eq!(Location::fixed("CST", 8 * 3600).name(), "CST");
    }

    // From Test_AddDate.
    #[test]
    fn test_add_date() {
        let time = parsed("2006-01-02 15:04:05").add_date(1, 2, 3);
        assert_eq!(time.format("Y-m-d H:i:s"), "2007-03-05 15:04:05");
    }

    // From Test_UTC and Test_Local.
    #[test]
    fn test_utc_and_local() {
        let time = Time::now();
        assert_eq!(time.utc().timestamp_nano(), time.timestamp_nano());
        assert_eq!(time.local().timestamp_nano(), time.timestamp_nano());
        assert_eq!(time.utc().location().name(), "UTC");
    }

    // From Test_Round.
    #[test]
    fn test_round() {
        let time = parsed("2018-08-08 08:08:08").round(10 * S);
        assert_eq!(time.format("Y-m-d H:i:s"), "2018-08-08 08:08:10");
    }

    // From Test_Truncate.
    #[test]
    fn test_truncate() {
        let time = parsed("2018-08-08 08:08:08").truncate(10 * S);
        assert_eq!(time.format("Y-m-d H:i:s"), "2018-08-08 08:08:00");
    }

    // From Test_StartOfMinute and Test_EndOfMinute.
    #[test]
    fn test_start_end_of_minute() {
        let time = parsed("2020-12-12 18:24:06");
        assert_eq!(
            time.start_of_minute().format("Y-m-d H:i:s"),
            "2020-12-12 18:24:00"
        );
        assert_eq!(
            time.end_of_minute(false).format("Y-m-d H:i:s.u"),
            "2020-12-12 18:24:59.000"
        );
        assert_eq!(
            time.end_of_minute(true).format("Y-m-d H:i:s.u"),
            "2020-12-12 18:24:59.999"
        );
    }

    // From Test_StartOfHour and Test_EndOfHour.
    #[test]
    fn test_start_end_of_hour() {
        let time = parsed("2020-12-12 18:24:06");
        assert_eq!(
            time.start_of_hour().format("Y-m-d H:i:s"),
            "2020-12-12 18:00:00"
        );
        assert_eq!(
            time.end_of_hour(false).format("Y-m-d H:i:s.u"),
            "2020-12-12 18:59:59.000"
        );
        assert_eq!(
            time.end_of_hour(true).format("Y-m-d H:i:s.u"),
            "2020-12-12 18:59:59.999"
        );
    }

    // From Test_StartOfDay and Test_EndOfDay.
    #[test]
    fn test_start_end_of_day() {
        let time = parsed("2020-12-12 18:24:06");
        assert_eq!(
            time.start_of_day().format("Y-m-d H:i:s"),
            "2020-12-12 00:00:00"
        );
        assert_eq!(
            time.end_of_day(false).format("Y-m-d H:i:s.u"),
            "2020-12-12 23:59:59.000"
        );
        assert_eq!(
            time.end_of_day(true).format("Y-m-d H:i:s.u"),
            "2020-12-12 23:59:59.999"
        );
    }

    // From Test_StartOfWeek and Test_EndOfWeek.
    #[test]
    fn test_start_end_of_week() {
        let time = parsed("2020-12-12 18:24:06");
        assert_eq!(
            time.start_of_week().format("Y-m-d H:i:s"),
            "2020-12-06 00:00:00"
        );
        assert_eq!(
            time.end_of_week(false).format("Y-m-d H:i:s.u"),
            "2020-12-12 23:59:59.000"
        );
        assert_eq!(
            time.end_of_week(true).format("Y-m-d H:i:s.u"),
            "2020-12-12 23:59:59.999"
        );
    }

    // From Test_StartOfMonth and Test_EndOfMonth.
    #[test]
    fn test_start_end_of_month() {
        let time = parsed("2020-12-12 18:24:06");
        assert_eq!(
            time.start_of_month().format("Y-m-d H:i:s"),
            "2020-12-01 00:00:00"
        );
        assert_eq!(
            time.end_of_month(false).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.000"
        );
        assert_eq!(
            time.end_of_month(true).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.999"
        );
    }

    // From Test_StartOfQuarter and Test_EndOfQuarter.
    #[test]
    fn test_start_end_of_quarter() {
        let time = parsed("2020-12-06 18:24:06");
        assert_eq!(
            time.start_of_quarter().format("Y-m-d H:i:s"),
            "2020-10-01 00:00:00"
        );
        assert_eq!(
            time.end_of_quarter(false).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.000"
        );
        assert_eq!(
            time.end_of_quarter(true).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.999"
        );
    }

    // From Test_StartOfHalf and Test_EndOfHalf.
    #[test]
    fn test_start_end_of_half() {
        let time = parsed("2020-12-06 18:24:06");
        assert_eq!(
            time.start_of_half().format("Y-m-d H:i:s"),
            "2020-07-01 00:00:00"
        );
        assert_eq!(
            time.end_of_half(false).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.000"
        );
        assert_eq!(
            time.end_of_half(true).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.999"
        );
    }

    // From Test_StartOfYear and Test_EndOfYear.
    #[test]
    fn test_start_end_of_year() {
        let time = parsed("2020-12-06 18:24:06");
        assert_eq!(
            time.start_of_year().format("Y-m-d H:i:s"),
            "2020-01-01 00:00:00"
        );
        assert_eq!(
            time.end_of_year(false).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.000"
        );
        assert_eq!(
            time.end_of_year(true).format("Y-m-d H:i:s.u"),
            "2020-12-31 23:59:59.999"
        );
    }

    // From Test_OnlyTime.
    #[test]
    fn test_only_time() {
        assert_eq!(parsed("18:24:06").string(), "18:24:06");
    }

    // From Test_DeepCopy.
    #[test]
    fn test_deep_copy() {
        assert!(Time::nil().deep_copy().is_none());
        let time = parsed("2022-03-08T03:01:14+08:00");
        let copy = time.deep_copy().expect("deep copy");
        assert!(copy.equal(&time));
    }

    // From Test_UnmarshalJSON.
    #[test]
    fn test_unmarshal_json() {
        let mut time = Time::zero();
        assert!(time.unmarshal_json("{}").is_err());
        assert!(time.unmarshal_json("\"2006-01-02 15:04:05\"").is_ok());
        assert_eq!(time.string(), "2006-01-02 15:04:05");
        assert!(time.unmarshal_json("").is_ok());
        assert!(time.is_zero());
    }

    // From Test_Issue1681.
    #[test]
    fn test_issue1681() {
        let cases = [
            ("2022-03-08T03:01:14-07:00", "2022-03-08T10:01:14Z"),
            ("2022-03-08T03:01:14-08:00", "2022-03-08T11:01:14Z"),
            ("2022-03-08T03:01:14-09:00", "2022-03-08T12:01:14Z"),
            ("2022-03-08T03:01:14+08:00", "2022-03-07T19:01:14Z"),
        ];
        for (left, right) in cases {
            assert_eq!(
                parsed(left).timestamp(),
                parsed(right).timestamp(),
                "{left}"
            );
            assert_eq!(
                parsed(left).local().timestamp(),
                parsed(right).local().timestamp(),
                "{left}"
            );
        }
    }

    // From Test_Issue2803.
    #[test]
    fn test_issue2803() {
        let time = parsed("2023-07-26")
            .layout_to("2006-01")
            .expect("layout round trip");
        assert_eq!(time.year(), 2023);
        assert_eq!(time.month(), 7);
        assert_eq!(time.day(), 1);
        assert_eq!(time.hour(), 0);
        assert_eq!(time.minute(), 0);
        assert_eq!(time.second(), 0);
    }

    // From Test_Issue3558.
    #[test]
    fn test_issue3558() {
        let time = parsed("1880-10-24T00:00:00+08:05");
        assert_eq!(time.year(), 1880);
        assert_eq!(time.month(), 10);
        assert_eq!(time.day(), 24);
        assert_eq!(time.hour(), 0);
        assert_eq!(time.minute(), 0);
        assert_eq!(time.second(), 0);
        assert_eq!(time.format("Y-m-d H:i:s"), "1880-10-24 00:00:00");

        let negative = parsed("1880-10-24T00:00:00-08:05");
        assert_eq!(negative.format("Y-m-d H:i:s"), "1880-10-24 00:00:00");
        assert_eq!(negative.utc().format("Y-m-d H:i:s"), "1880-10-24 08:05:00");
    }

    // From Test_Issue4307: Go accepts a nil *time.Time and returns nil.
    #[test]
    fn test_issue4307() {
        assert!(Time::default().is_nil());
        assert!(Time::default().format("Y-m-d").is_empty());
    }

    // From Test_Format.
    #[test]
    fn test_format() {
        let time = shanghai("2006-01-11 15:04:05");
        assert_eq!(
            time.format("\\T\\i\\m\\e中文Y-m-j G:i:s.u\\"),
            "Time中文2006-01-11 15:04:05.000"
        );
        assert_eq!(time.format("d D j l"), "11 Wed 11 Wednesday");
        assert_eq!(time.format("F m M n"), "January 01 Jan 1");
        assert_eq!(time.format("Y y"), "2006 06");
        assert_eq!(
            time.format("a A g G h H i s u .u"),
            "pm PM 3 15 03 15 04 05 000 .000"
        );
        assert_eq!(time.format("O P T"), "+0800 +08:00 CST");
        assert_eq!(time.format("r"), "Wed, 11 Jan 06 15:04 CST");
        assert_eq!(time.format("c"), "2006-01-11T15:04:05+08:00");

        let padded = parsed_format("2006-01-02 03:04:05", "Y-m-d H:i:s");
        assert_eq!(padded.format("Y-m-d h:i:s"), "2006-01-02 03:04:05");
        let unpadded = parsed_format("2006-01-02 03:04:05", "Y-m-d H:i:s");
        assert_eq!(unpadded.format("Y-n-j G:i:s"), "2006-1-2 3:04:05");
        assert_eq!(unpadded.format("U"), unpadded.timestamp().to_string());

        let cases = [
            ("2019-04-22", "w", "1"),
            ("2019-04-23", "w", "2"),
            ("2019-04-24", "w", "3"),
            ("2019-04-25", "w", "4"),
            ("2019-04-26", "dw", "265"),
            ("2019-04-27", "w", "6"),
            ("2019-03-10", "w", "0"),
            ("2019-03-10", "Y-m-d 星期:w", "2019-03-10 星期:0"),
            ("2019-04-25", "N", "4"),
            ("2019-03-10", "N", "7"),
            ("2019-03-01", "S", "st"),
            ("2019-03-02", "S", "nd"),
            ("2019-03-03", "S", "rd"),
            ("2019-03-05", "S", "th"),
            ("2019-01-01", "第z天", "第0天"),
            ("2019-01-05", "第z天", "第4天"),
            ("2020-05-05", "第z天", "第125天"),
            ("2020-12-31", "第z天", "第365天"),
            ("2020-02-12", "第z天", "第42天"),
            ("2019-02-12", "有t天", "有28天"),
            ("2020-02-12", "20.2有t天", "20.2有29天"),
            ("2019-03-12", "19.3有t天", "19.3有31天"),
            ("2019-11-12", "19.11有t天", "19.11有30天"),
            ("2019-01-01", "第W周", "第1周"),
            ("2017-01-01", "第W周", "第52周"),
            ("2002-01-01", "第W周为星期2", "第1周为星期2"),
            ("2016-01-01", "第W周为星期5", "第53周为星期5"),
            ("2014-01-01", "第W周为星期3", "第1周为星期3"),
            ("2015-01-01", "第W周为星期4", "第1周为星期4"),
        ];
        for (date, format, expected) in cases {
            let time = parsed_format(date, "Y-m-d");
            assert_eq!(time.format(format), expected, "{date} {format}");
        }

        let nil = Time::nil();
        assert_eq!(nil.format("Y-m-d h:i:s"), "");
        assert!(nil.format_new("Y-m-d h:i:s").is_none());
        assert!(nil.format_to("Y-m-d h:i:s").is_none());
        assert_eq!(nil.layout("Y-m-d h:i:s"), "");
        assert!(nil.layout_new("Y-m-d h:i:s").is_none());
        assert!(nil.layout_to("Y-m-d h:i:s").is_none());
    }

    // From Test_Format_ZeroString.
    #[test]
    fn test_format_zero_string() {
        assert!(str_to_time("0000-00-00 00:00:00", None).is_err());
        assert_eq!(Time::nil().string(), "");
    }

    // From Test_FormatTo.
    #[test]
    fn test_format_to() {
        let time = Time::now();
        let month = time.layout("2006-01");
        let formatted = time.format_to("Y-m-01 00:00:01").expect("format to");
        assert_eq!(formatted.string(), format!("{month}-01 00:00:01"));
    }

    // From Test_Layout.
    #[test]
    fn test_layout() {
        let time = Time::now();
        assert_eq!(
            time.layout("2006-01-02 15:04:05"),
            time.layout("2006-01-02 15:04:05")
        );
    }

    // From Test_LayoutTo.
    #[test]
    fn test_layout_to() {
        let time = Time::now();
        let formatted = time.layout_to("2006-01-02 00:00:00").expect("layout to");
        assert_eq!(
            formatted.layout("2006-01-02 00:00:00"),
            time.layout("2006-01-02 00:00:00")
        );
    }

    // From Test_Json_Pointer and Test_Json_Struct.
    #[test]
    fn test_json_round_trip() {
        let time = parsed("2006-01-02 15:04:05");
        assert_eq!(time.marshal_json(), "\"2006-01-02 15:04:05\"");
        assert_eq!(Time::nil().marshal_json(), "null");
        assert_eq!(Time::zero().marshal_json(), "\"\"");
        assert_eq!(parsed("08:08:08").marshal_json(), "\"08:08:08\"");

        let mut target = Time::zero();
        target
            .unmarshal_json(&time.marshal_json())
            .expect("unmarshal json");
        assert_eq!(target.string(), "2006-01-02 15:04:05");
    }

    // From TestTime_Scan.
    #[test]
    fn test_sql_scan() {
        let mut time = Time::zero();
        let value = Time::now().string();
        time.scan(&Value::String(value.clone()))
            .expect("scan string");
        assert_eq!(time.string(), value);

        let nanos = timestamp_nano();
        time.scan(&Value::Int(nanos)).expect("scan int");
        assert_eq!(time.timestamp_nano(), nanos);

        time.scan(&Value::Null).expect("scan null");
        assert!(time.is_zero());
    }

    // From TestTime_Value.
    #[test]
    fn test_sql_value() {
        let time = parsed("2006-01-02 15:04:05");
        assert_eq!(time.value(), Value::String(time.string()));
        assert_eq!(Time::nil().value(), Value::Null);
        assert_eq!(Time::zero().value(), Value::Null);
        assert_eq!(parsed("08:08:08").value(), Value::String("08:08:08".into()));
    }

    // From ExampleNew_curTime.
    #[test]
    fn test_example_new_cur_time() {
        let current = "2018-08-08 08:08:08";
        let from_string = parsed(current);
        let from_format = parsed_format(current, "Y-m-d H:i:s");
        assert_eq!(from_string.string(), "2018-08-08 08:08:08");
        assert_eq!(from_format.string(), "2018-08-08 08:08:08");

        let from_timestamp = Time::new_from_time_stamp(1533686888);
        assert_eq!(from_timestamp.timestamp(), 1533686888);
        assert_eq!(from_timestamp.string(), local_seconds(1533686888));
        assert_eq!(parsed("08:08:08").string(), "08:08:08");
    }

    // From ExampleNew_format.
    #[test]
    fn test_example_new_format() {
        let time = parsed_format("20220629133225", "YmdHis");
        assert_eq!(time.format("Y-m-d H:i:s"), "2022-06-29 13:32:25");
    }

    // From ExampleNewFromTime, ExampleNewFromStr, ExampleNewFromStrFormat and
    // ExampleNewFromStrLayout.
    #[test]
    fn test_example_constructors() {
        assert_eq!(
            Time::new_from_str("2018-08-08 08:08:08")
                .expect("parse")
                .string(),
            "2018-08-08 08:08:08"
        );
        assert_eq!(
            Time::new_from_str("08:08:08").expect("parse").string(),
            "08:08:08"
        );
        assert_eq!(
            parsed_format("2018-08-08 08:08:08", "Y-m-d H:i:s").string(),
            "2018-08-08 08:08:08"
        );
        assert_eq!(
            Time::new_from_str_layout("2018-08-08 08:08:08", "2006-01-02 15:04:05")
                .expect("parse")
                .string(),
            "2018-08-08 08:08:08"
        );
    }

    // From ExampleNewFromTimeStamp.
    #[test]
    fn test_example_new_from_time_stamp() {
        let first = Time::new_from_time_stamp(1533686888);
        let second = Time::new_from_time_stamp(1533686888000);
        assert_eq!(first.string(), second.string());
        assert_eq!(first.string(), local_seconds(1533686888));
    }

    // From ExampleTime_Month, ExampleTime_Second and ExampleTime_String.
    #[test]
    fn test_example_accessors() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(time.month(), 8);
        assert_eq!(time.second(), 8);
        assert_eq!(time.string(), "2018-08-08 08:08:08");
        assert_eq!(parsed("08:08:08").string(), "08:08:08");
    }

    // From ExampleTime_IsZero.
    #[test]
    fn test_example_is_zero() {
        assert!(!parsed("2018-08-08 08:08:08").is_zero());
        assert!(!parsed("00:00:00").is_zero());
    }

    // From ExampleTime_Add, ExampleTime_AddStr and ExampleTime_AddDate.
    #[test]
    fn test_example_add() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(time.add(10 * S).string(), "2018-08-08 08:08:18");
        assert_eq!(
            time.add_str("10s").expect("add str").string(),
            "2018-08-08 08:08:18"
        );
        assert_eq!(time.add_date(1, 2, 3).string(), "2019-10-11 08:08:08");
    }

    // From ExampleTime_Round and ExampleTime_Truncate.
    #[test]
    fn test_example_round_truncate() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(time.round(10 * S).string(), "2018-08-08 08:08:10");
        assert_eq!(time.truncate(10 * S).string(), "2018-08-08 08:08:00");
    }

    // From ExampleTime_Equal, ExampleTime_Before, ExampleTime_After and
    // ExampleTime_Sub.
    #[test]
    fn test_example_comparisons() {
        let first = parsed("2018-08-08 08:08:08");
        let second = parsed("2018-08-08 08:08:08");
        assert!(first.equal(&second));

        let earlier = parsed("2018-08-07");
        let later = parsed("2018-08-08");
        assert!(earlier.before(&later));
        assert!(!earlier.after(&later));

        let target = parsed("2018-08-08 08:08:10");
        assert_eq!(target.sub(&first), 2 * S);
        assert_eq!(duration_string(target.sub(&first)), "2s");
    }

    // From the ExampleTime_StartOf* and ExampleTime_EndOf* functions.
    #[test]
    fn test_example_boundaries() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(time.start_of_minute().string(), "2018-08-08 08:08:00");
        assert_eq!(time.start_of_hour().string(), "2018-08-08 08:00:00");
        assert_eq!(time.start_of_day().string(), "2018-08-08 00:00:00");
        assert_eq!(time.start_of_week().string(), "2018-08-05 00:00:00");
        assert_eq!(time.start_of_quarter().string(), "2018-07-01 00:00:00");
        assert_eq!(time.start_of_half().string(), "2018-07-01 00:00:00");
        assert_eq!(time.start_of_year().string(), "2018-01-01 00:00:00");
        assert_eq!(time.end_of_minute(false).string(), "2018-08-08 08:08:59");
        assert_eq!(time.end_of_hour(false).string(), "2018-08-08 08:59:59");
        assert_eq!(time.end_of_day(false).string(), "2018-08-08 23:59:59");
        assert_eq!(time.end_of_week(false).string(), "2018-08-11 23:59:59");
        assert_eq!(time.end_of_month(false).string(), "2018-08-31 23:59:59");
        assert_eq!(time.end_of_quarter(false).string(), "2018-09-30 23:59:59");
        assert_eq!(time.end_of_half(false).string(), "2018-12-31 23:59:59");
        assert_eq!(time.end_of_year(false).string(), "2018-12-31 23:59:59");
    }

    // From ExampleTime_Format.
    #[test]
    fn test_example_format() {
        let time = shanghai("2018-08-08 08:08:08");
        assert_eq!(time.format("Y-m-d"), "2018-08-08");
        assert_eq!(time.format("l"), "Wednesday");
        assert_eq!(time.format("F j, Y, g:i a"), "August 8, 2018, 8:08 am");
        assert_eq!(time.format("j, n, Y"), "8, 8, 2018");
        assert_eq!(
            time.format("h-i-s, j-m-y, it is w Day z"),
            "08-08-08, 8-08-18, 0831 0808 3 Wedam18 219"
        );
        assert_eq!(time.format("D M j G:i:s T Y"), "Wed Aug 8 8:08:08 CST 2018");
    }

    // From ExampleTime_FormatNew, ExampleTime_FormatTo, ExampleTime_Layout,
    // ExampleTime_LayoutNew and ExampleTime_LayoutTo.
    #[test]
    fn test_example_format_layout() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(
            time.format_new("Y-m-d").expect("format new").string(),
            "2018-08-08 00:00:00"
        );
        assert_eq!(
            time.format_new("Y-m-d H:i").expect("format new").string(),
            "2018-08-08 08:08:00"
        );
        assert_eq!(
            time.format_to("Y-m-d").expect("format to").string(),
            "2018-08-08 00:00:00"
        );
        assert_eq!(time.layout("2006-01-02"), "2018-08-08");
        assert_eq!(
            time.layout_new("2006-01-02").expect("layout new").string(),
            "2018-08-08 00:00:00"
        );
        assert_eq!(
            time.layout_to("2006-01-02").expect("layout to").string(),
            "2018-08-08 00:00:00"
        );
    }

    // From ExampleTime_IsLeapYear, ExampleTime_DayOfYear, ExampleTime_DaysInMonth
    // and ExampleTime_WeeksOfYear.
    #[test]
    fn test_example_calendar() {
        assert!(!parsed("2018-08-08 08:08:08").is_leap_year());
        assert!(parsed("2020-08-08 08:08:08").is_leap_year());
        assert_eq!(parsed("2018-01-08 08:08:08").day_of_year(), 7);
        assert_eq!(parsed("2018-08-08 08:08:08").days_in_month(), 31);
        assert_eq!(parsed("2018-01-08 08:08:08").weeks_of_year(), 2);
    }

    // From ExampleTime_ISO8601 and ExampleTime_RFC822.
    #[test]
    fn test_example_iso_rfc() {
        let time = shanghai("2018-08-08 08:08:08");
        assert_eq!(time.iso8601(), time.format("c"));
        assert_eq!(time.rfc822(), time.format("r"));
        assert_eq!(time.rfc822(), "Wed, 08 Aug 18 08:08 CST");
    }

    // From ExampleStrToTimeFormat, ExampleStrToTimeLayout and
    // ExampleParseDuration.
    #[test]
    fn test_example_parsers() {
        assert_eq!(
            str_to_time_format("2006-01-02 15:04:05", "Y-m-d H:i:s")
                .expect("format parse")
                .string(),
            "2006-01-02 15:04:05"
        );
        assert_eq!(
            str_to_time_layout("2018-08-08", "2006-01-02")
                .expect("layout parse")
                .string(),
            "2018-08-08 00:00:00"
        );
        assert_eq!(
            duration_string(parse_duration("+10h").expect("duration")),
            "10h0m0s"
        );
    }

    // From ExampleConvertZone.
    #[test]
    fn test_example_convert_zone() {
        let time = convert_zone("2006-01-02 15:04:05", "Asia/Tokyo", Some("Asia/Shanghai"))
            .expect("convert zone");
        assert_eq!(time.string(), "2006-01-02 16:04:05");
    }

    // From ExampleTime_ToZone (its output is not fixed, so only the zones).
    #[test]
    fn test_example_to_zone() {
        let now = Time::now();
        let shanghai = now.to_zone("Asia/Shanghai").expect("Shanghai");
        let tokyo = now.to_zone("Asia/Tokyo").expect("Tokyo");
        assert_eq!(shanghai.location().name(), "Asia/Shanghai");
        assert_eq!(tokyo.location().name(), "Asia/Tokyo");
        assert_eq!(tokyo.timestamp(), shanghai.timestamp());
        assert_eq!(
            (tokyo.hour() + 24 - shanghai.hour()) % 24,
            1,
            "Tokyo is one hour ahead of Shanghai"
        );
    }

    // From ExampleTime_MarshalJSON and ExampleTime_UnmarshalJSON.
    #[test]
    fn test_example_json() {
        let time = parsed("2018-08-08 08:08:08");
        assert_eq!(
            format!("{{\"birthday\":{}}}", time.marshal_json()),
            "{\"birthday\":\"2018-08-08 08:08:08\"}"
        );
        let mut target = Time::zero();
        target
            .unmarshal_json("\"2018-08-08 08:08:08\"")
            .expect("unmarshal");
        assert_eq!(target.string(), "2018-08-08 08:08:08");
    }

    // From ExampleTimestamp*: only the shape is fixed for "May Output" ones.
    #[test]
    fn test_example_timestamp_shapes() {
        let now = Time::now();
        assert!(timestamp() >= now.timestamp() - 1);
        assert!(!timestamp_str().is_empty());
        assert_eq!(timestamp_str(), now.timestamp_str());
    }

    // From ExampleDate, ExampleDatetime, ExampleISO8601 and ExampleRFC822.
    #[test]
    fn test_example_module_strings() {
        assert_eq!(date().len(), 10);
        assert_eq!(datetime().len(), 19);
        assert!(iso8601().contains('T'));
        assert!(rfc822().contains(','));
    }

    // Go's AddDate normalizes overflowing days into the next month.
    #[test]
    fn test_add_date_normalization() {
        let time = parsed("2020-01-31").add_date(0, 1, 0);
        assert_eq!(time.format("Y-m-d"), "2020-03-02");
        let time = parsed("2020-01-31").add_date(0, 0, 29);
        assert_eq!(time.format("Y-m-d"), "2020-02-29");
        let time = parsed("2006-01-02 15:04:05").add_date(-1, -2, -3);
        assert_eq!(time.format("Y-m-d H:i:s"), "2004-10-30 15:04:05");
    }

    // Comparisons follow instants, not offsets.
    #[test]
    fn test_compare_across_zones() {
        let utc = parsed("2022-03-08T10:01:14Z");
        let offset = parsed("2022-03-08T03:01:14-07:00");
        assert_eq!(utc.compare(&offset), 0);
        assert!(utc.equal(&offset));
        assert!(utc.between(&offset, &offset));
        assert!(parsed("2022-03-08T11:01:14Z").after(&offset));
        assert!(parsed("2022-03-08T09:01:14Z").before(&offset));
    }

    // From Test_FuncCost: the port must at least measure a closure.
    #[test]
    fn test_func_cost() {
        let cost = func_cost(|| {});
        assert!(cost >= 0);
    }

    // From the Go `formatToStdLayout` cases (also covered in layout.rs).
    #[test]
    fn test_layout_conversion() {
        assert_eq!(format_to_go_time("Y-m-d H:i:s"), "2006-01-02 15:04:05");
        assert_eq!(
            format_to_go_time("\\T\\i\\m\\e Y-m-d H:i:s"),
            "Time 2006-01-02 15:04:05"
        );
        assert_eq!(format_to_go_time("Y-m-d H:i:s\\"), "2006-01-02 15:04:05");
        assert_eq!(format_to_go_time("Y-m-d H:i:s\\u"), "2006-01-02 15:04:05u");
    }

    // The zero time formats like Go's zero `time.Time`, while `String` is empty.
    #[test]
    fn test_zero_format() {
        let zero = Time::zero();
        assert_eq!(zero.string(), "");
        assert_eq!(zero.format("Y-m-d H:i:s"), "0001-01-01 00:00:00");
        assert_eq!(zero.layout("2006-01-02"), "0001-01-01");
        assert_eq!(zero.timestamp_milli(), 0);
        assert_eq!(zero.timestamp_milli_str(), "");
    }

    // Layout parsing keeps Go's defaults for absent fields.
    #[test]
    fn test_layout_defaults() {
        let time = str_to_time_layout("2023-07", "2006-01").expect("parse");
        assert_eq!(time.format("Y-m-d H:i:s"), "2023-07-01 00:00:00");
        let time = str_to_time_layout("16:12:01", "15:04:05").expect("parse");
        assert_eq!(time.string(), "16:12:01");
        assert!(str_to_time_layout("2006-01-02 15:04:05", "aabbcc").is_err());
    }

    // `Time::scan` accepts byte slices; unsupported scalars clear the value,
    // as Go's `New` returns its zero time for them.
    #[test]
    fn test_scan_values() {
        let mut time = Time::zero();
        time.scan(&Value::Bytes(b"2018-08-08 08:08:08".to_vec()))
            .expect("scan bytes");
        assert_eq!(time.string(), "2018-08-08 08:08:08");
        time.scan(&Value::Bool(true)).expect("scan bool");
        assert!(time.is_zero());
        time.unmarshal_text(b"2018-08-08 08:08:08")
            .expect("unmarshal text");
        assert_eq!(time.string(), "2018-08-08 08:08:08");
        assert!(time.unmarshal_text(b"nope").is_err());
    }

    // Stored layouts are a port extension; they do not take part in equality.
    #[test]
    fn test_stored_layout() {
        let mut time = parsed("2018-08-08 08:08:08");
        assert!(time.stored_layout().is_none());
        time.set_layout("Y-m-d");
        assert_eq!(time.stored_layout(), Some("Y-m-d"));
        assert!(time.equal(&parsed("2018-08-08 08:08:08")));
    }
}
