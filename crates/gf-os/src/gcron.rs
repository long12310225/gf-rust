//! Port of `os/gcron`.
//!
//! Cron decides "does this second match the pattern?" every second (through
//! `os/gtimer`) and runs the matching jobs. The pattern parser supports the
//! six-part crontab form (`second minute hour day month week`), the `#` seconds
//! marker for standard five-part patterns, month/week names, `@every 1s` and
//! the predefined `@yearly`-style patterns.
//!
//! Divergences, recorded in ADR-054:
//!
//! - no `context.Context`: jobs are `JobFunc = Arc<dyn Fn() + Send + Sync>`;
//! - Go's `time.Time` arithmetic becomes `chrono::DateTime<Local>`;
//! - `StopGracefullyNonBlocking` returns a [`StopWaiter`] instead of a context
//!   (Rust has no cancellation context here);
//! - a panicking job is contained and logged (Go recovers and logs it too, but
//!   re-panics from the deferred function in the timer);
//! - the entry job name is the job pointer (`{:p}`) because Rust has no
//!   function reflection for names.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::time::{Duration, SystemTime};

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Timelike};
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use indexmap::IndexMap;

use crate::glog;
use crate::gtime;
use crate::gtimer;

/// The job is ready to run.
pub const STATUS_READY: i32 = gtimer::STATUS_READY;
/// The job is running.
pub const STATUS_RUNNING: i32 = gtimer::STATUS_RUNNING;
/// The job is stopped.
pub const STATUS_STOPPED: i32 = gtimer::STATUS_STOPPED;
/// The job is closed.
pub const STATUS_CLOSED: i32 = gtimer::STATUS_CLOSED;

/// A cron job, like Go's `JobFunc` (without the context parameter).
pub type JobFunc = Arc<dyn Fn() + Send + Sync>;

/// The field types of a cron pattern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PatternItemType {
    Second,
    Minute,
    Hour,
    Day,
    Week,
    Month,
}

/// The parsed cron schedule, like Go's `cronSchedule`.
pub struct CronSchedule {
    create_timestamp: i64,
    every_seconds: i64,
    pattern: String,
    ignore_seconds: bool,
    seconds: HashSet<u32>,
    minutes: HashSet<u32>,
    hours: HashSet<u32>,
    days: HashSet<u32>,
    weeks: HashSet<u32>,
    months: HashSet<u32>,
    last_meet: AtomicI64,
    last_check: AtomicI64,
}

/// The predefined patterns, like Go's `predefinedPatternMap`.
fn predefined_pattern(pattern: &str) -> Option<&'static str> {
    match pattern.to_lowercase().as_str() {
        "@yearly" | "@annually" => Some("# 0 0 1 1 *"),
        "@monthly" => Some("# 0 0 1 * *"),
        "@weekly" => Some("# 0 0 * * 0"),
        "@daily" | "@midnight" => Some("# 0 0 * * *"),
        "@hourly" => Some("# 0 * * * *"),
        _ => None,
    }
}

/// The short week names.
fn week_short(name: &str) -> Option<u32> {
    Some(match name {
        "sun" => 0,
        "mon" => 1,
        "tue" => 2,
        "wed" => 3,
        "thu" => 4,
        "fri" => 5,
        "sat" => 6,
        _ => return None,
    })
}

/// The full week names.
fn week_full(name: &str) -> Option<u32> {
    Some(match name {
        "sunday" => 0,
        "monday" => 1,
        "tuesday" => 2,
        "wednesday" => 3,
        "thursday" => 4,
        "friday" => 5,
        "saturday" => 6,
        _ => return None,
    })
}

/// The short month names.
fn month_short(name: &str) -> Option<u32> {
    Some(match name {
        "jan" => 1,
        "feb" => 2,
        "mar" => 3,
        "apr" => 4,
        "may" => 5,
        "jun" => 6,
        "jul" => 7,
        "aug" => 8,
        "sep" => 9,
        "oct" => 10,
        "nov" => 11,
        "dec" => 12,
        _ => return None,
    })
}

/// The full month names.
fn month_full(name: &str) -> Option<u32> {
    Some(match name {
        "january" => 1,
        "february" => 2,
        "march" => 3,
        "april" => 4,
        "may" => 5,
        "june" => 6,
        "july" => 7,
        "august" => 8,
        "september" => 9,
        "october" => 10,
        "november" => 11,
        "december" => 12,
        _ => return None,
    })
}

/// Parses a field value to a number, resolving week/month names, like
/// `parseWeekAndMonthNameToInt`.
fn parse_name_or_number(value: &str, item_type: PatternItemType) -> Result<u32, Error> {
    if value.chars().all(|character| character.is_ascii_digit()) {
        return value.parse::<u32>().map_err(|_| {
            gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid pattern value: \"{value}\""),
                Vec::new(),
            )
        });
    }
    let lower = value.to_lowercase();
    let number = match item_type {
        PatternItemType::Week => week_short(&lower).or_else(|| week_full(&lower)),
        PatternItemType::Month => month_short(&lower).or_else(|| month_full(&lower)),
        _ => None,
    };
    number.ok_or_else(|| {
        gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!("invalid pattern value: \"{value}\""),
            Vec::new(),
        )
    })
}

/// Parses one pattern item into the set of matching numbers, like
/// `parsePatternItem`.
fn parse_pattern_item(
    item: &str,
    min: u32,
    max: u32,
    allow_question_mark: bool,
    item_type: PatternItemType,
) -> Result<HashSet<u32>, Error> {
    let mut values = HashSet::new();
    if item == "*" || (allow_question_mark && item == "?") {
        for value in min..=max {
            values.insert(value);
        }
        return Ok(values);
    }
    for element in item.split(',') {
        let mut interval = 1u32;
        let interval_parts: Vec<&str> = element.split('/').collect();
        if interval_parts.len() == 2 {
            let parsed = interval_parts[1].parse::<u32>().map_err(|_| {
                gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("invalid pattern item: \"{element}\""),
                    Vec::new(),
                )
            })?;
            if parsed < 1 {
                return Err(gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("invalid pattern item: \"{element}\""),
                    Vec::new(),
                ));
            }
            interval = parsed;
        } else if interval_parts.len() > 2 {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid pattern item: \"{element}\""),
                Vec::new(),
            ));
        }
        let range_parts: Vec<&str> = interval_parts[0].split('-').collect();
        let (mut range_min, mut range_max) = (min, max);
        if range_parts[0] != "*" {
            range_min = parse_name_or_number(range_parts[0], item_type)?;
            if interval_parts.len() == 1 {
                range_max = range_min;
            }
        }
        if range_parts.len() == 2 {
            range_max = parse_name_or_number(range_parts[1], item_type)?;
        } else if range_parts.len() > 2 {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid pattern item: \"{element}\""),
                Vec::new(),
            ));
        }
        let mut value = range_min;
        while value <= range_max {
            values.insert(value);
            value = match value.checked_add(interval) {
                Some(next) => next,
                None => break,
            };
        }
    }
    Ok(values)
}

/// Builds a schedule from a pattern, like `newSchedule`.
pub fn new_schedule(pattern: &str) -> Result<Arc<CronSchedule>, Error> {
    let current_timestamp = Local::now().timestamp();
    let mut used_pattern = pattern.trim().to_owned();
    // Predefined patterns and `@every`.
    if used_pattern.starts_with('@') {
        let mut parts = used_pattern.split_whitespace();
        let key = parts.next().unwrap_or_default().to_lowercase();
        let argument = parts.next().unwrap_or_default();
        if let Some(predefined) = predefined_pattern(&key) {
            used_pattern = predefined.to_owned();
        } else if key == "@every" {
            if argument.is_empty() {
                return Err(gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("invalid pattern: \"{pattern}\""),
                    Vec::new(),
                ));
            }
            let duration_nanos = gtime::parse_duration(argument)?;
            return Ok(Arc::new(CronSchedule {
                create_timestamp: current_timestamp,
                every_seconds: duration_nanos / 1_000_000_000,
                pattern: pattern.to_owned(),
                ignore_seconds: true,
                seconds: HashSet::new(),
                minutes: HashSet::new(),
                hours: HashSet::new(),
                days: HashSet::new(),
                weeks: HashSet::new(),
                months: HashSet::new(),
                last_meet: AtomicI64::new(current_timestamp),
                last_check: AtomicI64::new(current_timestamp),
            }));
        } else {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid pattern: \"{pattern}\""),
                Vec::new(),
            ));
        }
    }
    let parts: Vec<&str> = used_pattern.split_whitespace().collect();
    if parts.len() != 6 {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!("invalid pattern: \"{pattern}\""),
            Vec::new(),
        ));
    }
    let ignore_seconds = parts[0] == "#";
    let seconds = if ignore_seconds {
        HashSet::new()
    } else {
        parse_pattern_item(parts[0], 0, 59, false, PatternItemType::Second)?
    };
    Ok(Arc::new(CronSchedule {
        create_timestamp: current_timestamp,
        every_seconds: 0,
        pattern: pattern.to_owned(),
        ignore_seconds,
        seconds,
        minutes: parse_pattern_item(parts[1], 0, 59, false, PatternItemType::Minute)?,
        hours: parse_pattern_item(parts[2], 0, 23, false, PatternItemType::Hour)?,
        days: parse_pattern_item(parts[3], 1, 31, true, PatternItemType::Day)?,
        weeks: parse_pattern_item(parts[5], 0, 6, true, PatternItemType::Week)?,
        months: parse_pattern_item(parts[4], 1, 12, false, PatternItemType::Month)?,
        last_meet: AtomicI64::new(current_timestamp),
        last_check: AtomicI64::new(current_timestamp),
    }))
}

impl CronSchedule {
    /// The raw pattern string.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    fn meet_second(&self, last_meet: i64, now: &DateTime<Local>) -> bool {
        if self.ignore_seconds {
            if now.timestamp() - last_meet < 60 {
                return false;
            }
        } else {
            // A precise pattern never runs twice in the same second.
            if self.seconds.len() == 1 && last_meet == now.timestamp() {
                return false;
            }
            if !self.seconds.contains(&now.second()) {
                return false;
            }
        }
        true
    }

    fn meet_minute(&self, now: &DateTime<Local>) -> bool {
        self.minutes.contains(&now.minute())
    }

    fn meet_hour(&self, now: &DateTime<Local>) -> bool {
        self.hours.contains(&now.hour())
    }

    fn meet_day(&self, now: &DateTime<Local>) -> bool {
        self.days.contains(&now.day())
    }

    fn meet_month(&self, now: &DateTime<Local>) -> bool {
        self.months.contains(&now.month())
    }

    fn meet_week(&self, now: &DateTime<Local>) -> bool {
        self.weeks.contains(&now.weekday().num_days_from_sunday())
    }

    /// Checks whether `now` matches and records the last meet, like
    /// `checkMeetAndUpdateLastSeconds`.
    pub fn check_meet_and_update_last_seconds(&self, now: DateTime<Local>) -> bool {
        let last_check = self.get_and_update_last_check_timestamp(now.timestamp());
        let last_meet = self.last_meet.load(Ordering::SeqCst);
        let meets = self.check_min_interval_and_item_maps(last_meet, last_check, &now);
        if meets {
            self.last_meet.store(now.timestamp(), Ordering::SeqCst);
        }
        meets
    }

    /// The interval / field matching core, like
    /// `checkMinIntervalAndItemMapMeet`.
    fn check_min_interval_and_item_maps(
        &self,
        last_meet: i64,
        last_check: i64,
        now: &DateTime<Local>,
    ) -> bool {
        if self.every_seconds != 0 {
            let seconds_after_created = last_check - self.create_timestamp;
            if seconds_after_created > 0 {
                return seconds_after_created % self.every_seconds == 0;
            }
            return false;
        }
        self.meet_second(last_meet, now)
            && self.meet_minute(now)
            && self.meet_hour(now)
            && self.meet_day(now)
            && self.meet_month(now)
            && self.meet_week(now)
    }

    /// Fixes and returns the last check timestamp, like
    /// `getAndUpdateLastCheckTimestamp`.
    fn get_and_update_last_check_timestamp(&self, current: i64) -> i64 {
        let mut last = self.last_check.load(Ordering::SeqCst);
        if last == current {
            last += 1;
        } else if last == current - 1 {
            last = current;
        } else if last == current - 2 || last == current - 3 {
            last += 1;
        } else {
            // Too much latency; the job might be missed, so resync.
            last = current;
        }
        self.last_check.store(last, Ordering::SeqCst);
        last
    }

    /// Computes the next activation time, like `Next`.
    pub fn next(&self, last_meet: DateTime<Local>) -> DateTime<Local> {
        if self.every_seconds != 0 {
            let diff = last_meet.timestamp() - self.create_timestamp;
            let count = diff / self.every_seconds + 1;
            return last_meet + chrono::Duration::seconds(count * self.every_seconds);
        }
        let mut current = if self.ignore_seconds {
            truncate_second(last_meet) + chrono::Duration::minutes(1)
        } else {
            truncate_second(last_meet) + chrono::Duration::seconds(1)
        };
        let year_limit = current.year() + 5;
        'wrap: loop {
            if current.year() > year_limit {
                return current;
            }
            while !self.meet_month(&current) {
                current = start_of_next_month(current);
                if current.month() == 1 {
                    continue 'wrap;
                }
            }
            while !self.meet_week(&current) || !self.meet_day(&current) {
                current = start_of_next_day(current);
                if current.day() == 1 {
                    continue 'wrap;
                }
            }
            while !self.meet_hour(&current) {
                current = truncate_hour(current) + chrono::Duration::hours(1);
                if current.hour() == 0 {
                    continue 'wrap;
                }
            }
            while !self.meet_minute(&current) {
                current = truncate_minute(current) + chrono::Duration::minutes(1);
                if current.minute() == 0 {
                    continue 'wrap;
                }
            }
            while !self.meet_second(last_meet.timestamp(), &current) {
                current += chrono::Duration::seconds(1);
                if current.second() == 0 {
                    continue 'wrap;
                }
            }
            return current;
        }
    }
}

/// Truncates to the second.
fn truncate_second(time: DateTime<Local>) -> DateTime<Local> {
    time.with_nanosecond(0).unwrap_or(time)
}

/// Truncates to the minute.
fn truncate_minute(time: DateTime<Local>) -> DateTime<Local> {
    time.with_second(0)
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(time)
}

/// Truncates to the hour.
fn truncate_hour(time: DateTime<Local>) -> DateTime<Local> {
    time.with_minute(0)
        .and_then(|time| time.with_second(0))
        .and_then(|time| time.with_nanosecond(0))
        .unwrap_or(time)
}

/// Builds a local time from date parts, falling back to the earliest value on
/// a daylight-saving gap.
fn local_datetime(date: NaiveDate, hour: u32, minute: u32, second: u32) -> DateTime<Local> {
    let naive = date
        .and_hms_opt(hour, minute, second)
        .unwrap_or_else(|| date.and_hms_opt(0, 0, 0).expect("midnight"));
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&naive).latest())
        .unwrap_or_else(Local::now)
}

/// The first day of the next month at midnight.
fn start_of_next_month(time: DateTime<Local>) -> DateTime<Local> {
    let (year, month) = if time.month() == 12 {
        (time.year() + 1, 1)
    } else {
        (time.year(), time.month() + 1)
    };
    let date = NaiveDate::from_ymd_opt(year, month, 1).unwrap_or_else(|| time.date_naive());
    local_datetime(date, 0, 0, 0)
}

/// The next day at midnight.
fn start_of_next_day(time: DateTime<Local>) -> DateTime<Local> {
    let date = time.date_naive() + chrono::Duration::days(1);
    local_datetime(date, 0, 0, 0)
}

/// Parsed entry parameters.
struct AddEntryInput {
    name: Option<String>,
    job: JobFunc,
    times: i64,
    pattern: String,
    singleton: bool,
    infinite: bool,
}

/// The runtime state shared by the cron and its entries.
struct CronInner {
    id_gen: AtomicI64,
    status: AtomicI32,
    entries: Mutex<IndexMap<String, Arc<Entry>>>,
    logger: Mutex<Option<Arc<glog::Logger>>>,
    running: Mutex<bool>,
    running_jobs: Mutex<usize>,
    job_finished: Condvar,
}

impl CronInner {
    fn status(&self) -> i32 {
        self.status.load(Ordering::SeqCst)
    }

    /// Marks a job as running, returning false when the cron is not running.
    fn begin_job(&self) -> bool {
        let running = self
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !*running {
            return false;
        }
        drop(running);
        *self
            .running_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) += 1;
        true
    }

    /// Marks a job as finished and wakes graceful waiters.
    fn end_job(&self) {
        let mut running = self
            .running_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *running = running.saturating_sub(1);
        if *running == 0 {
            self.job_finished.notify_all();
        }
    }

    fn log_debug(&self, message: &str) {
        if let Some(logger) = self
            .logger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            logger.debug(message);
        }
    }

    fn log_error(&self, message: &str) {
        match self
            .logger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            Some(logger) => logger.error(message),
            None => glog::error(message),
        }
    }
}

/// A cron manager, like Go's `Cron`.
pub struct Cron {
    inner: Arc<CronInner>,
}

impl Default for Cron {
    fn default() -> Cron {
        Cron::new()
    }
}

impl Cron {
    /// Creates a running cron, like `New`.
    pub fn new() -> Cron {
        Cron {
            inner: Arc::new(CronInner {
                id_gen: AtomicI64::new(0),
                status: AtomicI32::new(STATUS_RUNNING),
                entries: Mutex::new(IndexMap::new()),
                logger: Mutex::new(None),
                running: Mutex::new(true),
                running_jobs: Mutex::new(0),
                job_finished: Condvar::new(),
            }),
        }
    }

    /// Sets the logger used by the cron.
    pub fn set_logger(&self, logger: Arc<glog::Logger>) {
        *self
            .inner
            .logger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(logger);
    }

    /// Returns the logger used by the cron.
    pub fn logger(&self) -> Option<Arc<glog::Logger>> {
        self.inner
            .logger
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Adds an entry with detailed parameters, like `AddEntry`.
    pub fn add_entry(
        &self,
        pattern: &str,
        job: JobFunc,
        times: i64,
        singleton: bool,
        name: Option<&str>,
    ) -> Result<Arc<Entry>, Error> {
        self.do_add_entry(AddEntryInput {
            name: name.map(str::to_owned),
            job,
            times,
            pattern: pattern.to_owned(),
            singleton,
            infinite: times <= 0,
        })
    }

    /// Adds a repeating entry, like `Add`.
    pub fn add(
        &self,
        pattern: &str,
        job: JobFunc,
        name: Option<&str>,
    ) -> Result<Arc<Entry>, Error> {
        self.add_entry(pattern, job, -1, false, name)
    }

    /// Adds a singleton entry, like `AddSingleton`.
    pub fn add_singleton(
        &self,
        pattern: &str,
        job: JobFunc,
        name: Option<&str>,
    ) -> Result<Arc<Entry>, Error> {
        self.add_entry(pattern, job, -1, true, name)
    }

    /// Adds an entry running `times` times, like `AddTimes`.
    pub fn add_times(
        &self,
        pattern: &str,
        times: i64,
        job: JobFunc,
        name: Option<&str>,
    ) -> Result<Arc<Entry>, Error> {
        self.add_entry(pattern, job, times, false, name)
    }

    /// Adds a one-shot entry, like `AddOnce`.
    pub fn add_once(
        &self,
        pattern: &str,
        job: JobFunc,
        name: Option<&str>,
    ) -> Result<Arc<Entry>, Error> {
        self.add_entry(pattern, job, 1, false, name)
    }

    /// Adds an entry after `delay`, like `DelayAdd`.
    pub fn delay_add(&self, delay: Duration, pattern: &str, job: JobFunc, name: Option<&str>) {
        let cron = self.handle();
        let pattern = pattern.to_owned();
        let name = name.map(str::to_owned);
        gtimer::add_once(
            delay,
            Arc::new(move || {
                if let Err(error) = cron.add(&pattern, Arc::clone(&job), name.as_deref()) {
                    panic!("{error}");
                }
            }),
        );
    }

    /// Adds a singleton entry after `delay`, like `DelayAddSingleton`.
    pub fn delay_add_singleton(
        &self,
        delay: Duration,
        pattern: &str,
        job: JobFunc,
        name: Option<&str>,
    ) {
        let cron = self.handle();
        let pattern = pattern.to_owned();
        let name = name.map(str::to_owned);
        gtimer::add_once(
            delay,
            Arc::new(move || {
                if let Err(error) = cron.add_singleton(&pattern, Arc::clone(&job), name.as_deref())
                {
                    panic!("{error}");
                }
            }),
        );
    }

    /// Adds a one-shot entry after `delay`, like `DelayAddOnce`.
    pub fn delay_add_once(&self, delay: Duration, pattern: &str, job: JobFunc, name: Option<&str>) {
        let cron = self.handle();
        let pattern = pattern.to_owned();
        let name = name.map(str::to_owned);
        gtimer::add_once(
            delay,
            Arc::new(move || {
                if let Err(error) = cron.add_once(&pattern, Arc::clone(&job), name.as_deref()) {
                    panic!("{error}");
                }
            }),
        );
    }

    /// Adds a times-limited entry after `delay`, like `DelayAddTimes`.
    pub fn delay_add_times(
        &self,
        delay: Duration,
        pattern: &str,
        times: i64,
        job: JobFunc,
        name: Option<&str>,
    ) {
        let cron = self.handle();
        let pattern = pattern.to_owned();
        let name = name.map(str::to_owned);
        gtimer::add_once(
            delay,
            Arc::new(move || {
                if let Err(error) =
                    cron.add_times(&pattern, times, Arc::clone(&job), name.as_deref())
                {
                    panic!("{error}");
                }
            }),
        );
    }

    /// Returns a cheap clone of this cron.
    fn handle(&self) -> Cron {
        Cron {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Creates and registers an entry, like `doAddEntry`.
    fn do_add_entry(&self, input: AddEntryInput) -> Result<Arc<Entry>, Error> {
        if let Some(name) = &input.name
            && self.search(name).is_some()
        {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_OPERATION,
                format!("duplicated cron job name \"{name}\", already exists"),
                Vec::new(),
            ));
        }
        let schedule = new_schedule(&input.pattern)?;
        let name = match input.name {
            Some(name) => name,
            None => format!(
                "cron-{}",
                self.inner.id_gen.fetch_add(1, Ordering::SeqCst) + 1
            ),
        };
        let job_name = format!("{:p}", Arc::as_ptr(&input.job));
        let timer_target: Arc<Mutex<Option<Weak<Entry>>>> = Arc::new(Mutex::new(None));
        let timer_entry = gtimer::add_entry(
            Duration::from_secs(1),
            Arc::new({
                let slot = Arc::clone(&timer_target);
                move || {
                    let entry = slot
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .as_ref()
                        .and_then(Weak::upgrade);
                    if let Some(entry) = entry {
                        entry.check_and_run();
                    }
                }
            }),
            input.singleton,
            -1,
            STATUS_STOPPED,
        );
        let entry = Arc::new(Entry {
            cron: Arc::downgrade(&self.inner),
            timer_entry,
            schedule,
            name: name.clone(),
            register_time: SystemTime::now(),
            times: AtomicI64::new(input.times),
            infinite: AtomicBool::new(input.infinite),
            job: input.job,
            job_name,
            timer_target,
        });
        // Point the timer callback at the entry, then start it while the entry
        // is already registered so a running job never sees a missing entry.
        *entry
            .timer_target
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::downgrade(&entry));
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(name, Arc::clone(&entry));
        entry.start();
        Ok(entry)
    }

    /// Returns the entry named `name`, like `Search`.
    pub fn search(&self, name: &str) -> Option<Arc<Entry>> {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .cloned()
    }

    /// Starts the whole cron or the named entries, like `Start`.
    pub fn start(&self, names: &[&str]) {
        if names.is_empty() {
            *self
                .inner
                .running
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = true;
            self.inner.status.store(STATUS_READY, Ordering::SeqCst);
            return;
        }
        for name in names {
            if let Some(entry) = self.search(name) {
                entry.start();
            }
        }
    }

    /// Stops the whole cron or the named entries, like `Stop`.
    pub fn stop(&self, names: &[&str]) {
        if names.is_empty() {
            *self
                .inner
                .running
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
            self.inner.status.store(STATUS_STOPPED, Ordering::SeqCst);
            return;
        }
        for name in names {
            if let Some(entry) = self.search(name) {
                entry.stop();
            }
        }
    }

    /// Stops the cron and waits for the running jobs, like `StopGracefully`.
    pub fn stop_gracefully(&self) {
        self.stop(&[]);
        self.wait_jobs_done();
    }

    /// Stops the cron without blocking, returning a waiter, like
    /// `StopGracefullyNonBlocking`.
    pub fn stop_gracefully_non_blocking(&self) -> StopWaiter {
        self.stop(&[]);
        StopWaiter {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Waits until no job is running.
    fn wait_jobs_done(&self) {
        let mut running = self
            .inner
            .running_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *running > 0 {
            running = self
                .inner
                .job_finished
                .wait(running)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }

    /// Removes the entry named `name`, like `Remove`.
    pub fn remove(&self, name: &str) {
        if let Some(entry) = self.search(name) {
            entry.close();
        }
    }

    /// Stops and closes the cron, like `Close`.
    pub fn close(&self) {
        *self
            .inner
            .running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
        self.inner.status.store(STATUS_CLOSED, Ordering::SeqCst);
    }

    /// Returns the entry count, like `Size`.
    pub fn size(&self) -> usize {
        self.inner
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Returns all entries ordered by registration time, like `Entries`.
    pub fn entries(&self) -> Vec<Arc<Entry>> {
        let mut entries: Vec<Arc<Entry>> = self
            .inner
            .entries
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect();
        entries.sort_by_key(|entry| entry.register_time);
        entries
    }
}

/// A handle that waits for a graceful stop, like the context returned by
/// `StopGracefullyNonBlocking`.
pub struct StopWaiter {
    inner: Arc<CronInner>,
}

impl StopWaiter {
    /// Blocks until every running job finished.
    pub fn wait(&self) {
        let mut running = self
            .inner
            .running_jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while *running > 0 {
            running = self
                .inner
                .job_finished
                .wait(running)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
    }
}

/// One cron entry, like Go's `Entry`.
pub struct Entry {
    cron: Weak<CronInner>,
    timer_entry: gtimer::Entry,
    schedule: Arc<CronSchedule>,
    name: String,
    register_time: SystemTime,
    times: AtomicI64,
    infinite: AtomicBool,
    job: JobFunc,
    job_name: String,
    timer_target: Arc<Mutex<Option<Weak<Entry>>>>,
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Entry")
            .field("name", &self.name)
            .field("pattern", &self.schedule.pattern)
            .finish()
    }
}

impl Entry {
    /// The entry name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The raw cron pattern.
    pub fn pattern(&self) -> &str {
        self.schedule.pattern()
    }

    /// The registration time.
    pub fn register_time(&self) -> SystemTime {
        self.register_time
    }

    /// Reports whether the entry runs in singleton mode, like `IsSingleton`.
    pub fn is_singleton(&self) -> bool {
        self.timer_entry.is_singleton()
    }

    /// Sets the singleton mode, like `SetSingleton`.
    pub fn set_singleton(&self, enabled: bool) {
        self.timer_entry.set_singleton(enabled);
    }

    /// Sets the running times limit, like `SetTimes`.
    pub fn set_times(&self, times: i64) {
        self.times.store(times, Ordering::SeqCst);
        self.infinite.store(false, Ordering::SeqCst);
    }

    /// The entry status, like `Status`.
    pub fn status(&self) -> i32 {
        self.timer_entry.status()
    }

    /// Sets the entry status, returning the previous one, like `SetStatus`.
    pub fn set_status(&self, status: i32) -> i32 {
        self.timer_entry.set_status(status)
    }

    /// Starts the entry, like `Start`.
    pub fn start(&self) {
        self.timer_entry.start();
    }

    /// Stops the entry, like `Stop`.
    pub fn stop(&self) {
        self.timer_entry.stop();
    }

    /// Removes the entry from its cron and closes it, like `Close`.
    pub fn close(&self) {
        if let Some(cron) = self.cron.upgrade() {
            cron.entries
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .shift_remove(&self.name);
        }
        self.timer_entry.close();
    }

    /// The per-second check, like `checkAndRun`.
    fn check_and_run(self: &Arc<Self>) {
        let now = Local::now();
        if !self.schedule.check_meet_and_update_last_seconds(now) {
            return;
        }
        let Some(cron) = self.cron.upgrade() else {
            return;
        };
        match cron.status() {
            STATUS_STOPPED => {}
            STATUS_CLOSED => {
                cron.log_debug(&format!("cron job \"{}\" is removed", self.label()));
                self.close();
            }
            STATUS_READY | STATUS_RUNNING => {
                if !cron.begin_job() {
                    cron.log_debug(&format!("cron job \"{}\" stopped or closed", self.label()));
                    return;
                }
                if !self.infinite.load(Ordering::SeqCst) {
                    let left = self.times.fetch_sub(1, Ordering::SeqCst) - 1;
                    if left <= 0 {
                        let previous = self.timer_entry.set_status(STATUS_CLOSED);
                        if previous == STATUS_CLOSED || left < 0 {
                            self.close();
                            cron.end_job();
                            return;
                        }
                    }
                }
                cron.log_debug(&format!("cron job \"{}\" starts", self.label()));
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.job)()));
                match result {
                    Ok(()) => cron.log_debug(&format!("cron job \"{}\" ends", self.label())),
                    Err(payload) => {
                        cron.log_error(&format!(
                            "cron job \"{}({})\" end with error: {}",
                            self.job_name,
                            self.schedule.pattern,
                            panic_text(&payload)
                        ));
                    }
                }
                cron.end_job();
                if self.timer_entry.status() == STATUS_CLOSED {
                    self.close();
                }
            }
            _ => {}
        }
    }

    /// `jobName(pattern)` for log messages.
    fn label(&self) -> String {
        format!("{}({})", self.job_name, self.schedule.pattern)
    }
}

/// Renders a panic payload for logging.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(message) = payload.downcast_ref::<String>() {
        return message.clone();
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return (*message).to_owned();
    }
    if let Some(error) = payload.downcast_ref::<Error>() {
        return error.to_string();
    }
    "panic".to_owned()
}

/// The package default cron, like Go's `defaultCron`.
fn default_cron() -> &'static Cron {
    static DEFAULT: OnceLock<Cron> = OnceLock::new();
    DEFAULT.get_or_init(Cron::new)
}

/// Sets the default cron logger.
pub fn set_logger(logger: Arc<glog::Logger>) {
    default_cron().set_logger(logger);
}

/// Returns the default cron logger.
pub fn get_logger() -> Option<Arc<glog::Logger>> {
    default_cron().logger()
}

/// Adds an entry to the default cron.
pub fn add(pattern: &str, job: JobFunc, name: Option<&str>) -> Result<Arc<Entry>, Error> {
    default_cron().add(pattern, job, name)
}

/// Adds a singleton entry to the default cron.
pub fn add_singleton(pattern: &str, job: JobFunc, name: Option<&str>) -> Result<Arc<Entry>, Error> {
    default_cron().add_singleton(pattern, job, name)
}

/// Adds a one-shot entry to the default cron.
pub fn add_once(pattern: &str, job: JobFunc, name: Option<&str>) -> Result<Arc<Entry>, Error> {
    default_cron().add_once(pattern, job, name)
}

/// Adds a times-limited entry to the default cron.
pub fn add_times(
    pattern: &str,
    times: i64,
    job: JobFunc,
    name: Option<&str>,
) -> Result<Arc<Entry>, Error> {
    default_cron().add_times(pattern, times, job, name)
}

/// Adds an entry to the default cron after `delay`.
pub fn delay_add(delay: Duration, pattern: &str, job: JobFunc, name: Option<&str>) {
    default_cron().delay_add(delay, pattern, job, name);
}

/// Adds a singleton entry to the default cron after `delay`.
pub fn delay_add_singleton(delay: Duration, pattern: &str, job: JobFunc, name: Option<&str>) {
    default_cron().delay_add_singleton(delay, pattern, job, name);
}

/// Adds a one-shot entry to the default cron after `delay`.
pub fn delay_add_once(delay: Duration, pattern: &str, job: JobFunc, name: Option<&str>) {
    default_cron().delay_add_once(delay, pattern, job, name);
}

/// Adds a times-limited entry to the default cron after `delay`.
pub fn delay_add_times(
    delay: Duration,
    pattern: &str,
    times: i64,
    job: JobFunc,
    name: Option<&str>,
) {
    default_cron().delay_add_times(delay, pattern, times, job, name);
}

/// Returns the default cron entry named `name`.
pub fn search(name: &str) -> Option<Arc<Entry>> {
    default_cron().search(name)
}

/// Removes the default cron entry named `name`.
pub fn remove(name: &str) {
    default_cron().remove(name)
}

/// Returns the default cron size.
pub fn size() -> usize {
    default_cron().size()
}

/// Returns the default cron entries.
pub fn entries() -> Vec<Arc<Entry>> {
    default_cron().entries()
}

/// Starts the default cron or the named entries.
pub fn start(names: &[&str]) {
    default_cron().start(names)
}

/// Stops the default cron or the named entries.
pub fn stop(names: &[&str]) {
    default_cron().stop(names)
}

/// Stops the default cron gracefully.
pub fn stop_gracefully() {
    default_cron().stop_gracefully()
}

/// Stops the default cron gracefully without blocking.
pub fn stop_gracefully_non_blocking() -> StopWaiter {
    default_cron().stop_gracefully_non_blocking()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicI64;

    use super::*;

    fn time_of(text: &str) -> DateTime<Local> {
        for format in ["%a %b %e %H:%M:%S %Y", "%a %b %e %H:%M %Y"] {
            if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(text, format)
                && let Some(local) = Local.from_local_datetime(&naive).earliest()
            {
                return local;
            }
        }
        panic!("cannot parse time {text}");
    }

    /// Waits until `condition` holds, for at most five seconds.
    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(6);
        while std::time::Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    // Translated from TestSlash.
    #[test]
    fn slash_expands_week_ranges() {
        let schedule = new_schedule("0 0 0 * Feb Mon/2").expect("schedule");
        let mut weeks: Vec<u32> = schedule.weeks.iter().copied().collect();
        weeks.sort_unstable();
        assert_eq!(weeks, vec![1, 3, 5]);

        let schedule = new_schedule("0 0 0 * Feb *").expect("schedule");
        assert_eq!(schedule.weeks.len(), 7);
    }

    // Translated from TestSlash_InvalidInterval.
    #[test]
    fn slash_rejects_zero_interval() {
        for pattern in ["*/0 * * * * *", "* 1-30/0 * * * *", "* * */-1 * * *"] {
            assert!(new_schedule(pattern).is_err(), "{pattern} must be invalid");
        }
    }

    // Translated from a subset of TestNext.
    #[test]
    fn next_matches_go_expectations() {
        let cases = [
            (
                "Mon Jul 9 14:45 2012",
                "0 0/15 * * * *",
                "Mon Jul 9 15:00 2012",
            ),
            (
                "Mon Jul 9 14:59:59 2012",
                "0 0/15 * * * *",
                "Mon Jul 9 15:00 2012",
            ),
            (
                "Mon Jul 9 15:45 2012",
                "0 20-35/15 * * * *",
                "Mon Jul 9 16:20 2012",
            ),
            (
                "Mon Jul 9 23:46 2012",
                "0 */15 * * * *",
                "Tue Jul 10 00:00 2012",
            ),
            (
                "Mon Jul 9 23:35:51 2012",
                "15/35 20-35/15 * * * *",
                "Tue Jul 10 00:20:15 2012",
            ),
            (
                "Mon Jul 9 23:35:51 2012",
                "15/35 20-35/15 1/2 * * *",
                "Tue Jul 10 01:20:15 2012",
            ),
            (
                "Mon Jul 9 23:35 2012",
                "0 0 0 9 Apr-Oct ?",
                "Thu Aug 9 00:00 2012",
            ),
            (
                "Mon Jul 9 23:35 2012",
                "0 0 0 * Feb Mon",
                "Mon Feb 4 00:00 2013",
            ),
            (
                "Mon Dec 31 23:59:45 2012",
                "0 * * * * *",
                "Tue Jan 1 00:00:00 2013",
            ),
            (
                "Mon Jul 9 23:35 2012",
                "0 0 0 29 Feb ?",
                "Mon Feb 29 00:00 2016",
            ),
            ("Mon Jul 9 23:35 2012", "@daily", "Tue Jul 10 00:00:00 2012"),
            (
                "Mon Jul 9 23:35 2012",
                "@weekly",
                "Sun Jul 15 00:00:00 2012",
            ),
            (
                "Mon Jul 9 23:35 2012",
                "# * * * * *",
                "Mon Jul 9 23:36 2012",
            ),
        ];
        for (start, pattern, expected) in cases {
            let schedule = new_schedule(pattern).expect(pattern);
            let next = schedule.next(time_of(start));
            assert_eq!(
                next.timestamp(),
                time_of(expected).timestamp(),
                "{start} \"{pattern}\""
            );
        }
    }

    #[test]
    fn every_pattern_moves_forward_by_the_interval() {
        let schedule = new_schedule("@every 2s").expect("schedule");
        let start = Local::now();
        let next = schedule.next(start);
        let delta = (next - start).num_seconds();
        assert!((1..=2).contains(&delta), "unexpected delta {delta}");
        assert!(new_schedule("@every").is_err());
        assert!(new_schedule("@bogus").is_err());
    }

    #[test]
    fn cron_add_search_and_remove() {
        let cron = Cron::new();
        let job: JobFunc = Arc::new(|| {});
        let entry = cron
            .add("0 0 0 1 1 *", Arc::clone(&job), Some("yearly"))
            .expect("add");
        assert_eq!(entry.name(), "yearly");
        assert_eq!(cron.size(), 1);
        assert!(cron.search("yearly").is_some());
        cron.remove("yearly");
        assert_eq!(cron.size(), 0);
        assert!(cron.search("yearly").is_none());
        cron.close();
    }

    #[test]
    fn cron_rejects_duplicate_names() {
        let cron = Cron::new();
        let job: JobFunc = Arc::new(|| {});
        cron.add("0 0 0 1 1 *", Arc::clone(&job), Some("job"))
            .expect("first");
        let error = cron
            .add("0 0 0 1 1 *", job, Some("job"))
            .expect_err("duplicate");
        assert!(error.to_string().contains("duplicated"), "got {error}");
        cron.close();
    }

    #[test]
    fn cron_rejects_invalid_pattern() {
        let cron = Cron::new();
        assert!(
            cron.add("bad pattern", Arc::new(|| {}), None).is_err(),
            "invalid pattern must fail"
        );
        cron.close();
    }

    // Translated from TestCron_AddOnce.
    #[test]
    fn add_once_runs_once() {
        let cron = Cron::new();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        cron.add_once(
            "* * * * * *",
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            Some("once"),
        )
        .expect("add once");
        assert!(wait_until(|| count.load(Ordering::SeqCst) == 1));
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(count.load(Ordering::SeqCst), 1, "runs once");
        cron.close();
    }

    // Translated from TestCron_AddTimes.
    #[test]
    fn add_times_limits_runs() {
        let cron = Cron::new();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        cron.add_times(
            "* * * * * *",
            2,
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
            Some("times"),
        )
        .expect("add times");
        assert!(wait_until(|| count.load(Ordering::SeqCst) == 2));
        std::thread::sleep(Duration::from_millis(1500));
        assert_eq!(count.load(Ordering::SeqCst), 2);
        cron.close();
    }

    // Translated from TestCron_Stop_Start.
    #[test]
    fn stop_and_start_entries() {
        let cron = Cron::new();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        let entry = cron
            .add(
                "* * * * * *",
                Arc::new(move || {
                    counter.fetch_add(1, Ordering::SeqCst);
                }),
                Some("toggle"),
            )
            .expect("add");
        assert!(wait_until(|| count.load(Ordering::SeqCst) >= 1));
        cron.stop(&["toggle"]);
        std::thread::sleep(Duration::from_millis(1500));
        let stopped = count.load(Ordering::SeqCst);
        cron.start(&["toggle"]);
        assert!(wait_until(|| count.load(Ordering::SeqCst) > stopped));
        entry.close();
        cron.close();
    }

    // Translated from the graceful-stop tests.
    #[test]
    fn stop_gracefully_without_jobs_returns() {
        let cron = Cron::new();
        let started = std::time::Instant::now();
        cron.stop_gracefully();
        assert!(started.elapsed() < Duration::from_secs(2));
        let waiter = cron.stop_gracefully_non_blocking();
        waiter.wait();
    }

    #[test]
    fn entries_are_ordered_by_registration() {
        let cron = Cron::new();
        let job: JobFunc = Arc::new(|| {});
        cron.add("0 0 0 1 1 *", Arc::clone(&job), Some("a"))
            .expect("a");
        cron.add("0 0 0 1 1 *", Arc::clone(&job), Some("b"))
            .expect("b");
        let entries = cron.entries();
        assert_eq!(entries.len(), 2);
        assert!(entries[0].register_time() <= entries[1].register_time());
        cron.close();
    }

    #[test]
    fn default_cron_functions() {
        let job: JobFunc = Arc::new(|| {});
        let entry = add("0 0 0 1 1 *", job, Some("default-test")).expect("add");
        assert!(search("default-test").is_some());
        assert!(size() >= 1);
        assert!(entries().iter().any(|item| item.name() == "default-test"));
        remove("default-test");
        assert!(search("default-test").is_none());
        drop(entry);
        let _ = stop_gracefully_non_blocking();
    }
}
