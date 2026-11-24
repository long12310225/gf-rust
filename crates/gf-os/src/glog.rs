//! Port of `os/glog`.
//!
//! GoFrame's logger buffers a header (time, level, prefix, caller) plus the
//! content and values, pipes it through middleware-style handlers, and writes
//! the result to stdout, a rotating file and/or a custom writer.
//!
//! Divergences, recorded in ADR-046:
//!
//! - no `context.Context`, so `TraceId`, `CtxStr` and `CtxKeys` are absent;
//! - `*f` formatting variants are dropped: Rust formats at the call site;
//! - file handles are opened per write with a per-path lock instead of
//!   `os/gfpool` plus `os/gmlock`, and rotation checks run lazily on write
//!   instead of on a timer;
//! - `io.Writer` becomes a byte-sink closure ([`Writer`]);
//! - color codes are emitted inline (no `fatih/color`).

use std::collections::BTreeMap;
use std::io::Write as IoWrite;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use gf_core::Value;
use gf_debug::gdebug;
use gf_encoding::gcompress;
use gf_encoding::gjson;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_internal::command;
use gf_json;
use gf_text::gregex;
use indexmap::IndexMap;

use crate::gfile;
use crate::gtime;

/// Formats `now` with a GoFrame PHP-style layout like `Y-m-d H:i:s`.
fn format_time(now: SystemTime, layout: &str) -> String {
    time_of(now).format(layout)
}

/// Formats `now` with a Go layout like `2006-01-02 15:04:05`.
fn format_go_time(now: SystemTime, layout: &str) -> String {
    time_of(now).layout(layout)
}

/// Converts a [`SystemTime`] into the port time type.
fn time_of(now: SystemTime) -> gtime::Time {
    let millis = now
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0);
    gtime::Time::new_from_timestamp_milli(millis)
}

// ---------------------------------------------------------------------------
// Levels, flags and colors
// ---------------------------------------------------------------------------

/// All output levels, like Go's `LEVEL_ALL`.
pub const LEVEL_ALL: i32 =
    LEVEL_DEBU | LEVEL_INFO | LEVEL_NOTI | LEVEL_WARN | LEVEL_ERRO | LEVEL_CRIT;
/// The development level set (same as [`LEVEL_ALL`]).
pub const LEVEL_DEV: i32 = LEVEL_ALL;
/// The production level set, like Go's `LEVEL_PROD`.
pub const LEVEL_PROD: i32 = LEVEL_WARN | LEVEL_ERRO | LEVEL_CRIT;
/// No level at all.
pub const LEVEL_NONE: i32 = 0;
/// Debug level bit.
pub const LEVEL_DEBU: i32 = 1 << 4;
/// Info level bit.
pub const LEVEL_INFO: i32 = 1 << 5;
/// Notice level bit.
pub const LEVEL_NOTI: i32 = 1 << 6;
/// Warning level bit.
pub const LEVEL_WARN: i32 = 1 << 7;
/// Error level bit.
pub const LEVEL_ERRO: i32 = 1 << 8;
/// Critical level bit.
pub const LEVEL_CRIT: i32 = 1 << 9;
/// Panic level bit.
pub const LEVEL_PANI: i32 = 1 << 10;
/// Fatal level bit.
pub const LEVEL_FATA: i32 = 1 << 11;

/// Prints asynchronously, like Go's `F_ASYNC`.
pub const F_ASYNC: i32 = 1 << 0;
/// Prints the full caller path.
pub const F_FILE_LONG: i32 = 1 << 1;
/// Prints the short caller path (overrides `F_FILE_LONG`).
pub const F_FILE_SHORT: i32 = 1 << 2;
/// Prints the date.
pub const F_TIME_DATE: i32 = 1 << 3;
/// Prints the time.
pub const F_TIME_TIME: i32 = 1 << 4;
/// Prints the time with milliseconds.
pub const F_TIME_MILLI: i32 = 1 << 5;
/// Prints the caller function name.
pub const F_CALLER_FN: i32 = 1 << 6;
/// The default date plus milliseconds layout.
pub const F_TIME_STD: i32 = F_TIME_DATE | F_TIME_MILLI;

/// Black foreground.
pub const COLOR_BLACK: i32 = 30;
/// Red foreground.
pub const COLOR_RED: i32 = 31;
/// Green foreground.
pub const COLOR_GREEN: i32 = 32;
/// Yellow foreground.
pub const COLOR_YELLOW: i32 = 33;
/// Blue foreground.
pub const COLOR_BLUE: i32 = 34;
/// Magenta foreground.
pub const COLOR_MAGENTA: i32 = 35;
/// Cyan foreground.
pub const COLOR_CYAN: i32 = 36;
/// White foreground.
pub const COLOR_WHITE: i32 = 37;
/// Bright black foreground.
pub const COLOR_HI_BLACK: i32 = 90;
/// Bright red foreground.
pub const COLOR_HI_RED: i32 = 91;
/// Bright green foreground.
pub const COLOR_HI_GREEN: i32 = 92;
/// Bright yellow foreground.
pub const COLOR_HI_YELLOW: i32 = 93;
/// Bright blue foreground.
pub const COLOR_HI_BLUE: i32 = 94;
/// Bright magenta foreground.
pub const COLOR_HI_MAGENTA: i32 = 95;
/// Bright cyan foreground.
pub const COLOR_HI_CYAN: i32 = 96;
/// Bright white foreground.
pub const COLOR_HI_WHITE: i32 = 97;

/// The default file name pattern, like Go's `defaultFileFormat`.
pub const DEFAULT_FILE_FORMAT: &str = "{Y-m-d}.log";
/// The default time layout, like Go's `defaultTimeFormat`.
pub const DEFAULT_TIME_FORMAT: &str = "2006-01-02T15:04:05.000Z07:00";
/// The default level-to-prefix mapping.
pub fn default_level_prefixes() -> BTreeMap<i32, String> {
    let mut prefixes = BTreeMap::new();
    prefixes.insert(LEVEL_DEBU, "DEBU".to_owned());
    prefixes.insert(LEVEL_INFO, "INFO".to_owned());
    prefixes.insert(LEVEL_NOTI, "NOTI".to_owned());
    prefixes.insert(LEVEL_WARN, "WARN".to_owned());
    prefixes.insert(LEVEL_ERRO, "ERRO".to_owned());
    prefixes.insert(LEVEL_CRIT, "CRIT".to_owned());
    prefixes.insert(LEVEL_PANI, "PANI".to_owned());
    prefixes.insert(LEVEL_FATA, "FATA".to_owned());
    prefixes
}

/// The default level-to-color mapping.
pub fn default_level_color(level: i32) -> i32 {
    match level {
        LEVEL_DEBU => COLOR_YELLOW,
        LEVEL_INFO => COLOR_GREEN,
        LEVEL_NOTI => COLOR_CYAN,
        LEVEL_WARN => COLOR_MAGENTA,
        LEVEL_ERRO => COLOR_RED,
        LEVEL_CRIT | LEVEL_PANI | LEVEL_FATA => COLOR_HI_RED,
        _ => 0,
    }
}

/// Parses a level string into its bit set, like Go's `levelStringMap`.
pub fn parse_level(text: &str) -> Option<i32> {
    match text.to_uppercase().as_str() {
        "ALL" | "DEV" | "DEVELOP" => Some(LEVEL_ALL),
        "PROD" | "PRODUCT" => Some(LEVEL_PROD),
        "DEBU" | "DEBUG" => Some(LEVEL_ALL),
        "INFO" => Some(LEVEL_INFO | LEVEL_NOTI | LEVEL_WARN | LEVEL_ERRO | LEVEL_CRIT),
        "NOTI" | "NOTICE" => Some(LEVEL_NOTI | LEVEL_WARN | LEVEL_ERRO | LEVEL_CRIT),
        "WARN" | "WARNING" => Some(LEVEL_WARN | LEVEL_ERRO | LEVEL_CRIT),
        "ERRO" | "ERROR" => Some(LEVEL_ERRO | LEVEL_CRIT),
        "CRIT" | "CRITICAL" => Some(LEVEL_CRIT),
        _ => None,
    }
}

/// Wraps `text` in an ANSI color when `color` is non-zero.
pub fn colorize(color: i32, text: &str) -> String {
    if color == 0 {
        text.to_owned()
    } else {
        format!("\x1B[{color}m{text}\x1B[0m")
    }
}

/// A byte sink, like Go's `io.Writer`.
pub type Writer = Arc<dyn Fn(&str) + Send + Sync>;

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Logger configuration, like Go's `glog.Config`.
#[derive(Clone, Default)]
pub struct Config {
    /// Middleware handlers; when empty the package default is used.
    pub handlers: Vec<Handler>,
    /// Customized output writer.
    pub writer: Option<Writer>,
    /// Header/layout flags.
    pub flags: i32,
    /// Time layout used when set, overriding the flags.
    pub time_format: String,
    /// Directory for file logging.
    pub path: String,
    /// File name pattern, like `{Y-m-d}.log`.
    pub file: String,
    /// Enabled level bits.
    pub level: i32,
    /// Prefix printed in the header.
    pub prefix: String,
    /// Stack skip count.
    pub st_skip: usize,
    /// Whether the stack is printed for error-level records.
    pub st_status: bool,
    /// Extra stack filter.
    pub st_filter: String,
    /// Whether the header is printed.
    pub header_print: bool,
    /// Whether stdout is written.
    pub stdout_print: bool,
    /// Whether the level string is printed.
    pub level_print: bool,
    /// Level-to-prefix mapping.
    pub level_prefixes: BTreeMap<i32, String>,
    /// Rotate the file when it grows beyond this size.
    pub rotate_size: i64,
    /// Rotate the file when it is older than this.
    pub rotate_expire: Duration,
    /// Maximum number of rotated backups.
    pub rotate_backup_limit: usize,
    /// Maximum age of a rotated backup.
    pub rotate_backup_expire: Duration,
    /// Gzip level for rotated backups (0 disables compression).
    pub rotate_backup_compress: u32,
    /// Interval between background rotation checks.
    pub rotate_check_interval: Duration,
    /// Whether the writing side gets color codes.
    pub writer_color_enable: bool,
    /// Whether stdout color is suppressed.
    pub stdout_color_disabled: bool,
}

impl Config {
    /// The default configuration, like Go's `DefaultConfig`.
    pub fn new() -> Config {
        let mut level = LEVEL_ALL;
        if !default_debug() {
            level &= !LEVEL_DEBU;
        }
        Config {
            handlers: Vec::new(),
            writer: None,
            flags: F_TIME_STD,
            time_format: DEFAULT_TIME_FORMAT.to_owned(),
            path: String::new(),
            file: DEFAULT_FILE_FORMAT.to_owned(),
            level,
            prefix: String::new(),
            st_skip: 0,
            st_status: true,
            st_filter: String::new(),
            header_print: true,
            stdout_print: true,
            level_print: true,
            level_prefixes: default_level_prefixes(),
            rotate_size: 0,
            rotate_expire: Duration::ZERO,
            rotate_backup_limit: 0,
            rotate_backup_expire: Duration::ZERO,
            rotate_backup_compress: 0,
            rotate_check_interval: Duration::from_secs(3600),
            writer_color_enable: false,
            stdout_color_disabled: false,
        }
    }
}

/// Whether the debug level is enabled by default, like Go's `defaultDebug`.
fn default_debug() -> bool {
    static DEFAULT: OnceLock<bool> = OnceLock::new();
    *DEFAULT.get_or_init(|| {
        let value = command::get_opt_with_env("gf.glog.debug", Some("true"));
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "no"
        )
    })
}

// ---------------------------------------------------------------------------
// Handler input
// ---------------------------------------------------------------------------

/// The logging record passed through handlers, like Go's `HandlerInput`.
pub struct HandlerInput {
    /// Formatted time text.
    pub time_format: String,
    /// Formatted level prefix.
    pub level_format: String,
    /// Caller function name.
    pub caller_func: String,
    /// Caller path with its line number.
    pub caller_path: String,
    /// Custom prefix.
    pub prefix: String,
    /// Custom content.
    pub content: String,
    /// Passed values.
    pub values: Vec<Value>,
    /// Stack text.
    pub stack: String,
    /// Level bit.
    pub level: i32,
    /// ANSI color for the level.
    pub color: i32,
    /// Output buffer; a handler may prepend content.
    pub buffer: String,
    index: usize,
    handlers: Vec<Handler>,
}

impl HandlerInput {
    /// Calls the next handler in the chain, like Go's `HandlerInput.Next`.
    ///
    /// Go starts at index -1 and increments before the call; the port starts
    /// at 0 and calls the current index first.
    pub fn next(&mut self) {
        let index = self.index;
        self.index += 1;
        if let Some(handler) = self.handlers.get(index).cloned() {
            handler(self);
        }
    }

    /// Joins the values into content the way Go does.
    pub fn values_content(&self) -> String {
        let mut buffer = String::new();
        for value in &self.values {
            let text = gconv_string(value);
            if text.is_empty() {
                continue;
            }
            if buffer.is_empty() {
                buffer.push_str(&text);
                continue;
            }
            if !buffer.ends_with('\n') {
                buffer.push(' ');
                buffer.push_str(&text);
                continue;
            }
            let text = text.strip_prefix('\n').unwrap_or(&text);
            buffer.push_str(text);
        }
        buffer
    }

    /// The default text layout, like Go's `getDefaultBuffer`.
    pub fn default_buffer(&self, header: bool, level_print: bool, with_color: bool) -> String {
        let mut buffer = String::new();
        if header {
            if !self.time_format.is_empty() {
                push_part(&mut buffer, &self.time_format);
            }
            if level_print && !self.level_format.is_empty() {
                let text = format!("[{}]", self.level_format);
                if with_color {
                    push_part(&mut buffer, &colorize(self.color, &text));
                } else {
                    push_part(&mut buffer, &text);
                }
            }
        }
        if header {
            if !self.prefix.is_empty() {
                push_part(&mut buffer, &self.prefix);
            }
            if !self.caller_func.is_empty() {
                push_part(&mut buffer, &self.caller_func);
            }
            if !self.caller_path.is_empty() {
                push_part(&mut buffer, &self.caller_path);
            }
        }
        if !self.content.is_empty() {
            push_part(&mut buffer, &self.content);
        }
        if !self.values.is_empty() {
            push_part(&mut buffer, &self.values_content());
        }
        if !self.stack.is_empty() {
            push_part(&mut buffer, &format!("\nStack:\n{}", self.stack));
        }
        buffer.push('\n');
        buffer
    }

    /// The buffer used for real output, like `getRealBuffer`.
    pub fn real_buffer(&self, header: bool, level_print: bool, with_color: bool) -> String {
        if !self.buffer.is_empty() {
            return self.buffer.clone();
        }
        self.default_buffer(header, level_print, with_color)
    }

    /// The rendered content as a string, like Go's `HandlerInput.String`.
    pub fn string(&self, with_color: bool) -> String {
        self.default_buffer(true, true, with_color)
    }
}

/// Appends a space-separated part to `buffer`.
fn push_part(buffer: &mut String, text: &str) {
    if !buffer.is_empty() {
        buffer.push(' ');
    }
    buffer.push_str(text);
}

/// A logging handler, like Go's `Handler`.
pub type Handler = Arc<dyn Fn(&mut HandlerInput) + Send + Sync>;

/// The package-wide default handler, like Go's `defaultHandler`.
fn default_handler_slot() -> &'static Mutex<Option<Handler>> {
    static HANDLER: OnceLock<Mutex<Option<Handler>>> = OnceLock::new();
    HANDLER.get_or_init(|| Mutex::new(None))
}

/// Sets the package default handler.
pub fn set_default_handler(handler: Handler) {
    *default_handler_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handler);
}

/// Returns the package default handler.
pub fn get_default_handler() -> Option<Handler> {
    default_handler_slot()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

// ---------------------------------------------------------------------------
// Logger
// ---------------------------------------------------------------------------

/// The logger, like Go's `glog.Logger`.
///
/// Cloning a logger copies its configuration, so chained setters never change
/// the logger they were derived from.
#[derive(Clone)]
pub struct Logger {
    config: Arc<RwLock<Config>>,
    last_rotate_check: Arc<Mutex<Option<Instant>>>,
    rotate_initialized: Arc<AtomicBool>,
}

impl Default for Logger {
    fn default() -> Logger {
        Logger::new()
    }
}

impl Logger {
    /// Creates a logger with the default configuration, like `New`.
    pub fn new() -> Logger {
        Logger {
            config: Arc::new(RwLock::new(Config::new())),
            last_rotate_check: Arc::new(Mutex::new(None)),
            rotate_initialized: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Creates a logger over a writer, like `NewWithWriter`.
    pub fn new_with_writer(writer: Writer) -> Logger {
        let logger = Logger::new();
        logger.set_writer(writer);
        logger
    }

    /// Returns a snapshot of the configuration.
    pub fn config(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Replaces the configuration, like Go's `SetConfig`.
    pub fn set_config(&self, config: Config) -> Result<(), Error> {
        if !config.path.is_empty() {
            self.set_path(&config.path)?;
            let mut current = self
                .config
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let path = current.path.clone();
            *current = config;
            current.path = path;
        } else {
            *self
                .config
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
        }
        Ok(())
    }

    /// Sets the logging directory, creating it when missing.
    pub fn set_path(&self, path: &str) -> Result<(), Error> {
        if path.is_empty() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["logging path is empty"],
            ));
        }
        if !gfile::exists(path) {
            gfile::mkdir(path).map_err(|error| {
                gerror::wrap(
                    Some(error),
                    format!("Mkdir \"{path}\" failed in PWD \"{}\"", gfile::pwd()),
                )
                .unwrap_or_else(|| gerror::new("mkdir failed"))
            })?;
        }
        self.config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .path = path.trim_end_matches(['/', '\\']).to_owned();
        Ok(())
    }

    /// Returns the logging directory.
    pub fn path(&self) -> String {
        self.read().path
    }

    /// Sets the file name pattern.
    pub fn set_file(&self, pattern: &str) {
        self.write().file = pattern.to_owned();
    }

    /// Returns the file name pattern.
    pub fn file(&self) -> String {
        self.read().file
    }

    /// Sets the time layout.
    pub fn set_time_format(&self, format: &str) {
        self.write().time_format = format.to_owned();
    }

    /// Returns the time layout.
    pub fn time_format(&self) -> String {
        self.read().time_format
    }

    /// Sets the enabled level bits, always keeping critical levels on.
    pub fn set_level(&self, level: i32) {
        self.write().level = level | LEVEL_CRIT | LEVEL_PANI | LEVEL_FATA;
    }

    /// Returns the enabled level bits.
    pub fn level(&self) -> i32 {
        self.read().level
    }

    /// Sets the level by name, like Go's `SetLevelStr`.
    pub fn set_level_str(&self, level: &str) -> Result<(), Error> {
        match parse_level(level) {
            Some(bits) => {
                self.write().level = bits;
                Ok(())
            }
            None => Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid level string: {level}"),
                Vec::new(),
            )),
        }
    }

    /// Enables or disables the debug bit.
    pub fn set_debug(&self, debug: bool) {
        let mut config = self.write();
        if debug {
            config.level |= LEVEL_DEBU;
        } else {
            config.level &= !LEVEL_DEBU;
        }
    }

    /// Sets the extra flags.
    pub fn set_flags(&self, flags: i32) {
        self.write().flags = flags;
    }

    /// Returns the flags.
    pub fn flags(&self) -> i32 {
        self.read().flags
    }

    /// Enables or disables asynchronous output.
    pub fn set_async(&self, enabled: bool) {
        let mut config = self.write();
        if enabled {
            config.flags |= F_ASYNC;
        } else {
            config.flags &= !F_ASYNC;
        }
    }

    /// Enables or disables stack output.
    pub fn set_stack(&self, enabled: bool) {
        self.write().st_status = enabled;
    }

    /// Sets the stack skip count.
    pub fn set_stack_skip(&self, skip: usize) {
        self.write().st_skip = skip;
    }

    /// Sets the stack filter.
    pub fn set_stack_filter(&self, filter: &str) {
        self.write().st_filter = filter.to_owned();
    }

    /// Sets the header prefix.
    pub fn set_prefix(&self, prefix: &str) {
        self.write().prefix = prefix.to_owned();
    }

    /// Returns the header prefix.
    pub fn prefix(&self) -> String {
        self.read().prefix
    }

    /// Sets the per-level prefix.
    pub fn set_level_prefix(&self, level: i32, prefix: &str) {
        self.write().level_prefixes.insert(level, prefix.to_owned());
    }

    /// Sets several per-level prefixes.
    pub fn set_level_prefixes(&self, prefixes: BTreeMap<i32, String>) {
        let mut config = self.write();
        for (level, prefix) in prefixes {
            config.level_prefixes.insert(level, prefix);
        }
    }

    /// Returns the prefix of `level`.
    pub fn level_prefix(&self, level: i32) -> String {
        self.read()
            .level_prefixes
            .get(&level)
            .cloned()
            .unwrap_or_default()
    }

    /// Enables or disables stdout output.
    pub fn set_stdout_print(&self, enabled: bool) {
        self.write().stdout_print = enabled;
    }

    /// Whether stdout output is enabled.
    pub fn stdout_print(&self) -> bool {
        self.read().stdout_print
    }

    /// Enables or disables the header.
    pub fn set_header_print(&self, enabled: bool) {
        self.write().header_print = enabled;
    }

    /// Whether the header is enabled.
    pub fn header_print(&self) -> bool {
        self.read().header_print
    }

    /// Enables or disables the level string.
    pub fn set_level_print(&self, enabled: bool) {
        self.write().level_print = enabled;
    }

    /// Sets the custom writer.
    pub fn set_writer(&self, writer: Writer) {
        self.write().writer = Some(writer);
    }

    /// Returns the custom writer.
    pub fn writer(&self) -> Option<Writer> {
        self.read().writer.clone()
    }

    /// Sets the handlers.
    pub fn set_handlers(&self, handlers: Vec<Handler>) {
        self.write().handlers = handlers;
    }

    /// Enables or disables color for the writer output.
    pub fn set_writer_color_enable(&self, enabled: bool) {
        self.write().writer_color_enable = enabled;
    }

    /// Disables or enables color for stdout.
    pub fn set_stdout_color_disabled(&self, disabled: bool) {
        self.write().stdout_color_disabled = disabled;
    }

    /// Sets the rotate size in bytes.
    pub fn set_rotate_size(&self, size: i64) {
        self.write().rotate_size = size;
    }

    /// Sets the rotate expire duration.
    pub fn set_rotate_expire(&self, expire: Duration) {
        self.write().rotate_expire = expire;
    }

    /// Sets the rotate backup limit.
    pub fn set_rotate_backup_limit(&self, limit: usize) {
        self.write().rotate_backup_limit = limit;
    }

    /// Sets the rotate backup expire.
    pub fn set_rotate_backup_expire(&self, expire: Duration) {
        self.write().rotate_backup_expire = expire;
    }

    /// Sets the rotate backup compression level.
    pub fn set_rotate_backup_compress(&self, compress: u32) {
        self.write().rotate_backup_compress = compress;
    }

    /// Sets the rotate check interval.
    pub fn set_rotate_check_interval(&self, interval: Duration) {
        self.write().rotate_check_interval = interval;
    }

    /// Reports whether `level` is enabled, like `checkLevel`.
    pub fn check_level(&self, level: i32) -> bool {
        self.read().level & level > 0
    }

    /// Returns the logging file path for `now`, like `getFilePath`.
    pub fn get_file_path(&self, now: SystemTime) -> String {
        let config = self.read();
        let pattern = gregex::replace_string_func(r"\{.+?\}", &config.file, |matched| {
            let layout = matched.trim_matches(['{', '}']);
            format_time(now, layout)
        })
        .unwrap_or_else(|_| config.file.clone());
        gfile::join(&[config.path.as_str(), pattern.as_str()])
    }

    /// Returns the stack text, like Go's `GetStack`.
    pub fn get_stack(&self, skip: usize) -> String {
        let config = self.read();
        // Go filters its own package frames out here; the port keeps the code
        // and its tests in one file, so only the configured filter applies.
        let mut filters = Vec::new();
        if !config.st_filter.is_empty() {
            filters.push(config.st_filter.clone());
        }
        gdebug::stack(&filters, config.st_skip + skip)
    }

    /// Prints `values` at `level`, like Go's `print`.
    pub fn print(&self, level: i32, values: Vec<Value>) {
        let config = self.config();
        // Lazy rotation checks, like Go's initial `rotateChecksTimely`.
        if config.rotate_size > 0 || !config.rotate_expire.is_zero() {
            self.maybe_check_rotation(&config);
        }
        let now = SystemTime::now();
        let mut time_text = String::new();
        if !config.time_format.is_empty() {
            time_text = format_go_time(now, &config.time_format);
        } else {
            let mut layout = String::new();
            if config.flags & F_TIME_DATE > 0 {
                layout.push_str("Y-m-d");
            }
            if config.flags & F_TIME_TIME > 0 {
                if !layout.is_empty() {
                    layout.push(' ');
                }
                layout.push_str("H:i:s");
            }
            if config.flags & F_TIME_MILLI > 0 {
                if !layout.is_empty() {
                    layout.push(' ');
                }
                layout.push_str("H:i:s.v");
            }
            if !layout.is_empty() {
                time_text = format_time(now, &layout);
            }
        }
        let mut caller_func = String::new();
        let mut caller_path = String::new();
        if config.flags & (F_FILE_LONG | F_FILE_SHORT | F_CALLER_FN) > 0 {
            let (function, path, line) = gdebug::caller_with_filter(
                &["/os/glog/glog".to_owned(), "glog.rs".to_owned()],
                config.st_skip,
            );
            if config.flags & F_CALLER_FN > 0 && function.len() > 2 {
                caller_func = format!("[{function}]");
            }
            if line > 0 && path.len() > 1 {
                if config.flags & F_FILE_LONG > 0 {
                    caller_path = format!("{path}:{line}:");
                }
                if config.flags & F_FILE_SHORT > 0 {
                    caller_path = format!("{}:{line}:", gfile::basename(&path));
                }
            }
        }
        // Go.s `printErr` computes the stack for the error levels.
        let stack = if config.st_status && level >= LEVEL_ERRO {
            self.get_stack(0)
        } else {
            String::new()
        };
        let mut handlers = if config.handlers.is_empty() {
            match get_default_handler() {
                Some(handler) => vec![handler],
                None => Vec::new(),
            }
        } else {
            config.handlers.clone()
        };
        let final_config = config.clone();
        handlers.push(Arc::new(move |input: &mut HandlerInput| {
            let content = do_final_print(&final_config, input);
            if input.buffer.is_empty() {
                input.buffer = content;
            }
        }));
        let mut input = HandlerInput {
            time_format: time_text,
            level_format: self.level_prefix(level),
            caller_func,
            caller_path,
            prefix: config.prefix.clone(),
            content: String::new(),
            values,
            stack,
            level,
            color: default_level_color(level),
            buffer: String::new(),
            index: 0,
            handlers,
        };
        input.next();
    }

    /// Prints the values with no level, like Go's `Print`.
    pub fn print_std(&self, values: Vec<Value>) {
        self.print(LEVEL_NONE, values);
    }

    /// Prints an error-level record with the stack, like `printErr`.
    pub fn print_err(&self, level: i32, values: Vec<Value>) {
        self.print(level, values);
    }

    /// Logs at the debug level.
    pub fn debug(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_DEBU) {
            self.print(LEVEL_DEBU, values.into_values());
        }
    }

    /// Logs at the info level.
    pub fn info(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_INFO) {
            self.print(LEVEL_INFO, values.into_values());
        }
    }

    /// Logs at the notice level.
    pub fn notice(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_NOTI) {
            self.print(LEVEL_NOTI, values.into_values());
        }
    }

    /// Logs at the warning level.
    pub fn warning(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_WARN) {
            self.print(LEVEL_WARN, values.into_values());
        }
    }

    /// Logs at the error level.
    pub fn error(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_ERRO) {
            self.print_err(LEVEL_ERRO, values.into_values());
        }
    }

    /// Logs at the critical level.
    pub fn critical(&self, values: impl IntoValues) {
        if self.check_level(LEVEL_CRIT) {
            self.print_err(LEVEL_CRIT, values.into_values());
        }
    }

    /// Logs at the fatal level and exits the process, like `Fatal`.
    pub fn fatal(&self, values: impl IntoValues) -> ! {
        self.print_err(LEVEL_FATA, values.into_values());
        std::process::exit(1);
    }

    /// Logs at the panic level and panics, like `Panic`.
    pub fn panic(&self, values: impl IntoValues) -> ! {
        let text = gconv_string(&Value::String(
            values
                .into_values()
                .iter()
                .map(gconv_string)
                .collect::<Vec<_>>()
                .join(" "),
        ));
        self.print_err(LEVEL_PANI, vec![Value::String(text.clone())]);
        std::panic::panic_any(text);
    }

    /// Prints the caller stack, like `PrintStack`.
    pub fn print_stack(&self, skip: usize) {
        let stack = self.get_stack(skip);
        if stack.is_empty() {
            self.print_std(Vec::new());
        } else {
            self.print_std(vec![Value::String(format!("Stack:\n{stack}"))]);
        }
    }

    /// Chaining: sets the path, like `Path`.
    pub fn with_path(&self, path: &str) -> Logger {
        let logger = self.clone();
        if !path.is_empty()
            && let Err(error) = logger.set_path(path)
        {
            panic!("{error}");
        }
        logger
    }

    /// Chaining: appends a category to the path, like `Cat`.
    pub fn with_cat(&self, category: &str) -> Logger {
        let logger = self.clone();
        let path = logger.path();
        if !path.is_empty()
            && let Err(error) = logger.set_path(&gfile::join(&[&path, category]))
        {
            panic!("{error}");
        }
        logger
    }

    /// Chaining: sets the file pattern, like `File`.
    pub fn with_file(&self, file: &str) -> Logger {
        if file.is_empty() {
            return self.clone();
        }
        let logger = self.clone();
        logger.set_file(file);
        logger
    }

    /// Chaining: sets the level, like `Level`.
    pub fn with_level(&self, level: i32) -> Logger {
        let logger = self.clone();
        logger.set_level(level);
        logger
    }

    /// Chaining: sets the level by name, like `LevelStr`.
    pub fn with_level_str(&self, level: &str) -> Logger {
        let logger = self.clone();
        if let Err(error) = logger.set_level_str(level) {
            panic!("{error}");
        }
        logger
    }

    /// Chaining: enables or disables stdout, like `Stdout`.
    pub fn with_stdout(&self, enabled: bool) -> Logger {
        let logger = self.clone();
        logger.set_stdout_print(enabled);
        logger
    }

    /// Chaining: enables or disables the header, like `Header`.
    pub fn with_header(&self, enabled: bool) -> Logger {
        let logger = self.clone();
        logger.set_header_print(enabled);
        logger
    }

    /// Chaining: enables or disables the caller line, like `Line`.
    pub fn with_line(&self, long: bool) -> Logger {
        let logger = self.clone();
        let mut config = logger.write();
        if long {
            config.flags |= F_FILE_LONG;
        } else {
            config.flags |= F_FILE_SHORT;
        }
        logger.clone()
    }

    /// Chaining: enables or disables async output, like `Async`.
    pub fn with_async(&self, enabled: bool) -> Logger {
        let logger = self.clone();
        logger.set_async(enabled);
        logger
    }

    /// Chaining: redirects output to `writer`, like `To`.
    pub fn with_writer(&self, writer: Writer) -> Logger {
        let logger = self.clone();
        logger.set_writer(writer);
        logger
    }

    /// Chaining: sets stack behavior, like `Stack`.
    pub fn with_stack(&self, enabled: bool, skip: Option<usize>) -> Logger {
        let logger = self.clone();
        logger.set_stack(enabled);
        if let Some(skip) = skip {
            logger.set_stack_skip(skip);
        }
        logger
    }

    /// Chaining: sets the stack filter, like `StackWithFilter`.
    pub fn with_stack_filter(&self, filter: &str) -> Logger {
        let logger = self.clone();
        logger.set_stack(true);
        logger.set_stack_filter(filter);
        logger
    }

    /// Reads a configuration snapshot.
    fn read(&self) -> Config {
        self.config
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Writes the configuration.
    fn write(&self) -> std::sync::RwLockWriteGuard<'_, Config> {
        self.config
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs the rotation checks when the interval elapsed.
    fn maybe_check_rotation(&self, config: &Config) {
        if !self.rotate_initialized.swap(true, Ordering::SeqCst) {
            rotate::checks_timely(config);
            *self
                .last_rotate_check
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
            return;
        }
        let due = {
            let last = *self
                .last_rotate_check
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match last {
                Some(last) => last.elapsed() >= config.rotate_check_interval,
                None => true,
            }
        };
        if due {
            rotate::checks_timely(config);
            *self
                .last_rotate_check
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
        }
    }
}

/// Outputs the record to every configured destination, like `doFinalPrint`.
fn do_final_print(config: &Config, input: &mut HandlerInput) -> String {
    let mut result = String::new();
    if config.stdout_print {
        let buffer = input.real_buffer(
            config.header_print,
            config.level_print,
            !config.stdout_color_disabled,
        );
        print!("{buffer}");
        let _ = std::io::stdout().flush();
        result = buffer;
    }
    if !config.path.is_empty() {
        let buffer = input.real_buffer(
            config.header_print,
            config.level_print,
            config.writer_color_enable,
        );
        let path = rotate::file_path(config, SystemTime::now());
        rotate::write_file(config, &path, &buffer);
        result = buffer;
    }
    if let Some(writer) = &config.writer {
        let buffer = input.real_buffer(
            config.header_print,
            config.level_print,
            config.writer_color_enable,
        );
        writer(&buffer);
        result = buffer;
    }
    result
}

/// Converts a value to its GoFrame string form.
fn gconv_string(value: &Value) -> String {
    gf_util::gconv::string(value)
}

/// Values accepted by the logging methods.
///
/// Go's variadic `...any` becomes this trait, so `logger.info("text")`,
/// `logger.info(value)` and `logger.info(vec![...])` all work.
pub trait IntoValues {
    /// Converts into the value list.
    fn into_values(self) -> Vec<Value>;
}

impl IntoValues for Value {
    fn into_values(self) -> Vec<Value> {
        vec![self]
    }
}

impl IntoValues for &Value {
    fn into_values(self) -> Vec<Value> {
        vec![self.clone()]
    }
}

impl IntoValues for Vec<Value> {
    fn into_values(self) -> Vec<Value> {
        self
    }
}

impl IntoValues for &[Value] {
    fn into_values(self) -> Vec<Value> {
        self.to_vec()
    }
}

impl IntoValues for &str {
    fn into_values(self) -> Vec<Value> {
        vec![Value::String(self.to_owned())]
    }
}

impl IntoValues for String {
    fn into_values(self) -> Vec<Value> {
        vec![Value::String(self)]
    }
}

impl IntoValues for &String {
    fn into_values(self) -> Vec<Value> {
        vec![Value::String(self.clone())]
    }
}

impl IntoValues for i64 {
    fn into_values(self) -> Vec<Value> {
        vec![Value::Int(self)]
    }
}

impl IntoValues for u64 {
    fn into_values(self) -> Vec<Value> {
        vec![Value::UInt(self)]
    }
}

impl IntoValues for f64 {
    fn into_values(self) -> Vec<Value> {
        vec![Value::Float(self)]
    }
}

impl IntoValues for bool {
    fn into_values(self) -> Vec<Value> {
        vec![Value::Bool(self)]
    }
}

impl IntoValues for () {
    fn into_values(self) -> Vec<Value> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

/// File rotation, like `glog_logger_rotate.go`.
mod rotate {
    use super::*;

    /// Locks held per path, replacing Go's `gmlock`.
    fn locks() -> &'static Mutex<Vec<PathBuf>> {
        static LOCKS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
        LOCKS.get_or_init(|| Mutex::new(Vec::new()))
    }

    /// A per-path lock guard; dropping it releases the path.
    pub struct PathLock {
        path: PathBuf,
    }

    impl Drop for PathLock {
        fn drop(&mut self) {
            locks()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .retain(|path| path != &self.path);
        }
    }

    /// Tries to lock `path` without blocking, like `gmlock.TryLock`.
    pub fn try_lock(path: &str) -> Option<PathLock> {
        let path = PathBuf::from(path);
        let mut locks = locks()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if locks.contains(&path) {
            return None;
        }
        locks.push(path.clone());
        Some(PathLock { path })
    }

    /// The logging file path for `now`.
    pub fn file_path(config: &Config, now: SystemTime) -> String {
        let pattern = gregex::replace_string_func(r"\{.+?\}", &config.file, |matched| {
            format_time(now, matched.trim_matches(['{', '}']))
        })
        .unwrap_or_else(|_| config.file.clone());
        gfile::join(&[config.path.as_str(), pattern.as_str()])
    }

    /// Appends `content` to `path`, rotating first when the size is exceeded.
    pub fn write_file(config: &Config, path: &str, content: &str) {
        let Some(_guard) = try_lock(path) else {
            return;
        };
        if config.rotate_size > 0 && gfile::size(path) > config.rotate_size {
            rotate_by_size(config, path);
        }
        match std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            Ok(mut file) => {
                if let Err(error) = file.write_all(content.as_bytes()) {
                    eprintln!("glog: write \"{path}\" failed: {error}");
                }
            }
            Err(error) => eprintln!("glog: open \"{path}\" failed: {error}"),
        }
    }

    /// Rotates `path` when its size exceeds the configured limit.
    fn rotate_by_size(config: &Config, path: &str) {
        if config.rotate_size <= 0 {
            return;
        }
        if let Err(error) = do_rotate_file(config, path) {
            eprintln!("glog: rotate \"{path}\" failed: {error}");
        }
    }

    /// Rotates one logging file, like `doRotateFile`.
    pub fn do_rotate_file(config: &Config, path: &str) -> Result<(), Error> {
        let Some(_guard) = try_lock(&format!("rotate:{path}")) else {
            return Ok(());
        };
        if config.rotate_backup_limit == 0 {
            return gfile::remove_file(path);
        }
        let dir = gfile::dir(path);
        let name = gfile::name(path);
        let extension = gfile::ext_name(path);
        let mut attempt = 0u32;
        let new_path = loop {
            let now = SystemTime::now();
            let micros = now
                .duration_since(UNIX_EPOCH)
                .map(|elapsed| elapsed.subsec_micros() % 1000)
                .unwrap_or(0);
            let micro = if micros == 0 { 101 } else { micros.max(100) };
            let stamp = format_time(now, "YmdHisu");
            let candidate = gfile::join(&[&dir, &format!("{name}.{stamp}{micro}.{extension}")]);
            if !gfile::exists(&candidate) || attempt > 100 {
                break candidate;
            }
            attempt += 1;
        };
        gfile::rename(path, &new_path)
    }

    /// Runs the periodic checks, like `rotateChecksTimely`.
    pub fn checks_timely(config: &Config) {
        if config.rotate_size <= 0 && config.rotate_expire.is_zero() {
            return;
        }
        if config.path.is_empty() {
            return;
        }
        let Some(_guard) = try_lock(&format!("checks:{}", config.path)) else {
            return;
        };
        let mut files = match gfile::scan_dir_file(&config.path, "*.log, *.gz", true) {
            Ok(files) => files,
            Err(error) => {
                eprintln!("glog: rotation scan failed: {error}");
                return;
            }
        };
        // Expired rotation.
        if !config.rotate_expire.is_zero() {
            let now = gtime::Time::now();
            let mut rotated = false;
            for file in &files {
                let base = gfile::basename(file);
                if is_backup_name(&base) || gfile::ext_name(file) == "gz" {
                    continue;
                }
                if !matches_pattern(config, file) {
                    continue;
                }
                let mtime = gfile::mtime(file);
                let age = Duration::from_nanos(now.sub(&mtime).max(0) as u64);
                if age > config.rotate_expire {
                    rotated = true;
                    if let Err(error) = do_rotate_file(config, file) {
                        eprintln!("glog: rotate expired \"{file}\" failed: {error}");
                    }
                }
            }
            if rotated {
                files = gfile::scan_dir_file(&config.path, "*.log, *.gz", true).unwrap_or_default();
            }
        }
        // Compress backups.
        if config.rotate_backup_compress > 0 {
            let mut compressed = false;
            for file in &files {
                if gfile::ext_name(file) == "gz" || !is_backup_name(&gfile::basename(file)) {
                    continue;
                }
                let target = format!("{file}.gz");
                if gcompress::gzip_file(file, &target, Some(config.rotate_backup_compress)).is_ok()
                {
                    let _ = gfile::remove_file(file);
                    compressed = true;
                }
            }
            if compressed {
                files = gfile::scan_dir_file(&config.path, "*.log, *.gz", true).unwrap_or_default();
            }
        }
        // Backup limit and expiration.
        if config.rotate_backup_limit > 0 || !config.rotate_backup_expire.is_zero() {
            let mut backups: Vec<String> = files
                .iter()
                .filter(|file| is_backup_name(&gfile::basename(file)))
                .cloned()
                .collect();
            backups.sort_by_key(|file| gfile::mtime(file).timestamp_milli());
            if backups.len() > config.rotate_backup_limit {
                let remove_count = backups.len() - config.rotate_backup_limit;
                for file in backups.drain(..remove_count) {
                    let _ = gfile::remove_file(&file);
                }
            }
            if !config.rotate_backup_expire.is_zero() {
                let now = gtime::Time::now();
                for file in &backups {
                    let age = Duration::from_nanos(now.sub(&gfile::mtime(file)).max(0) as u64);
                    if age <= config.rotate_backup_expire {
                        break;
                    }
                    let _ = gfile::remove_file(file);
                }
            }
        }
    }

    /// Whether `name` looks like a rotated backup (`name.20digits.log`).
    fn is_backup_name(name: &str) -> bool {
        let parts: Vec<&str> = name.split('.').collect();
        if parts.len() < 3 {
            return false;
        }
        let stamp = parts[parts.len() - 2];
        stamp.len() == 20 && stamp.chars().all(|ch| ch.is_ascii_digit())
    }

    /// Whether `file` matches the configured file pattern.
    fn matches_pattern(config: &Config, file: &str) -> bool {
        let pattern = gregex::replace_string(r"\{.+?\}", "$", &config.file).unwrap_or_default();
        let pattern = gregex::quote(&pattern);
        let pattern = pattern.replace("\\$", "(.+?)");
        gregex::is_match_string(&pattern, file)
    }
}

// ---------------------------------------------------------------------------
// Package API
// ---------------------------------------------------------------------------

/// The default instance name, like Go's `DefaultName`.
pub const DEFAULT_NAME: &str = "default";

/// The package default logger.
fn default_logger_slot() -> &'static RwLock<Arc<Logger>> {
    static LOGGER: OnceLock<RwLock<Arc<Logger>>> = OnceLock::new();
    LOGGER.get_or_init(|| RwLock::new(Arc::new(Logger::new())))
}

/// Returns the package default logger, like `DefaultLogger`.
pub fn default_logger() -> Arc<Logger> {
    Arc::clone(
        &default_logger_slot()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
    )
}

/// Replaces the package default logger, like `SetDefaultLogger`.
pub fn set_default_logger(logger: Arc<Logger>) {
    *default_logger_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = logger;
}

/// The process-wide named instances, like Go's `instances`.
fn instances() -> &'static Mutex<BTreeMap<String, Arc<Logger>>> {
    static INSTANCES: OnceLock<Mutex<BTreeMap<String, Arc<Logger>>>> = OnceLock::new();
    INSTANCES.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Returns a named logger instance, like Go's `Instance`.
pub fn instance(name: Option<&str>) -> Arc<Logger> {
    let key = match name {
        Some(name) if !name.is_empty() => name,
        _ => DEFAULT_NAME,
    };
    let mut instances = instances()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(logger) = instances.get(key) {
        return Arc::clone(logger);
    }
    let logger = Arc::new(Logger::new());
    instances.insert(key.to_owned(), Arc::clone(&logger));
    logger
}

/// Logs at the debug level on the default logger.
pub fn debug(values: impl IntoValues) {
    default_logger().debug(values);
}

/// Logs at the info level on the default logger.
pub fn info(values: impl IntoValues) {
    default_logger().info(values);
}

/// Logs at the notice level on the default logger.
pub fn notice(values: impl IntoValues) {
    default_logger().notice(values);
}

/// Logs at the warning level on the default logger.
pub fn warning(values: impl IntoValues) {
    default_logger().warning(values);
}

/// Logs at the error level on the default logger.
pub fn error(values: impl IntoValues) {
    default_logger().error(values);
}

/// Logs at the critical level on the default logger.
pub fn critical(values: impl IntoValues) {
    default_logger().critical(values);
}

/// Logs with no level on the default logger.
pub fn print(values: impl IntoValues) {
    default_logger().print_std(values.into_values());
}

/// Logs at the fatal level and exits.
pub fn fatal(values: impl IntoValues) -> ! {
    default_logger().fatal(values)
}

/// Logs at the panic level and panics.
pub fn panic(values: impl IntoValues) -> ! {
    default_logger().panic(values)
}

/// Sets the default logger path.
pub fn set_path(path: &str) -> Result<(), Error> {
    default_logger().set_path(path)
}

/// Returns the default logger path.
pub fn get_path() -> String {
    default_logger().path()
}

/// Sets the default logger level.
pub fn set_level(level: i32) {
    default_logger().set_level(level);
}

/// Sets the default logger level by name.
pub fn set_level_str(level: &str) -> Result<(), Error> {
    default_logger().set_level_str(level)
}

/// Sets the default logger file pattern.
pub fn set_file(pattern: &str) {
    default_logger().set_file(pattern);
}

/// Sets the default logger prefix.
pub fn set_prefix(prefix: &str) {
    default_logger().set_prefix(prefix);
}

/// Sets the default logger flags.
pub fn set_flags(flags: i32) {
    default_logger().set_flags(flags);
}

/// Sets the default logger handlers.
pub fn set_handlers(handlers: Vec<Handler>) {
    default_logger().set_handlers(handlers);
}

/// Sets the default logger config.
pub fn set_config(config: Config) -> Result<(), Error> {
    default_logger().set_config(config)
}

/// Sets the default logger debug bit.
pub fn set_debug(debug: bool) {
    default_logger().set_debug(debug);
}

/// Sets the default logger stdout output.
pub fn set_stdout_print(enabled: bool) {
    default_logger().set_stdout_print(enabled);
}

/// Sets the default logger header output.
pub fn set_header_print(enabled: bool) {
    default_logger().set_header_print(enabled);
}

/// Sets the default logger level string output.
pub fn set_level_print(enabled: bool) {
    default_logger().set_level_print(enabled);
}

/// Sets the default logger writer.
pub fn set_writer(writer: Writer) {
    default_logger().set_writer(writer);
}

// ---------------------------------------------------------------------------
// Built-in handlers
// ---------------------------------------------------------------------------

/// The `Time` key used by the structured handlers.
pub const STRUCTURE_KEY_TIME: &str = "Time";
/// The `Level` key.
pub const STRUCTURE_KEY_LEVEL: &str = "Level";
/// The `Prefix` key.
pub const STRUCTURE_KEY_PREFIX: &str = "Prefix";
/// The `Content` key.
pub const STRUCTURE_KEY_CONTENT: &str = "Content";
/// The `TraceId` key.
pub const STRUCTURE_KEY_TRACE_ID: &str = "TraceId";
/// The `CallerFunc` key.
pub const STRUCTURE_KEY_CALLER_FUNC: &str = "CallerFunc";
/// The `CallerPath` key.
pub const STRUCTURE_KEY_CALLER_PATH: &str = "CallerPath";
/// The `CtxStr` key.
pub const STRUCTURE_KEY_CTX_STR: &str = "CtxStr";
/// The `Stack` key.
pub const STRUCTURE_KEY_STACK: &str = "Stack";

/// The keys a handler never lets content overwrite.
fn reserved_keys() -> [&'static str; 9] {
    [
        STRUCTURE_KEY_TIME,
        STRUCTURE_KEY_TRACE_ID,
        STRUCTURE_KEY_CTX_STR,
        STRUCTURE_KEY_LEVEL,
        STRUCTURE_KEY_CALLER_FUNC,
        STRUCTURE_KEY_CALLER_PATH,
        STRUCTURE_KEY_PREFIX,
        STRUCTURE_KEY_CONTENT,
        STRUCTURE_KEY_STACK,
    ]
}

/// A handler that prints one JSON object per record, like `HandlerJson`.
pub fn handler_json() -> Handler {
    Arc::new(|input: &mut HandlerInput| {
        let mut output: IndexMap<String, Value> = IndexMap::new();
        output.insert(
            STRUCTURE_KEY_TIME.to_owned(),
            Value::String(input.time_format.clone()),
        );
        for (key, value) in [
            (STRUCTURE_KEY_LEVEL, input.level_format.clone()),
            (STRUCTURE_KEY_CALLER_FUNC, input.caller_func.clone()),
            (STRUCTURE_KEY_CALLER_PATH, input.caller_path.clone()),
            (STRUCTURE_KEY_PREFIX, input.prefix.clone()),
            (STRUCTURE_KEY_STACK, input.stack.clone()),
        ] {
            if !value.is_empty() {
                output.insert(key.to_owned(), Value::String(value));
            }
        }
        let mut content = input.content.clone();
        if !input.values.is_empty() {
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(&input.values_content());
        }
        let mut flattened = false;
        if !content.is_empty()
            && let Ok(json) = gjson::load_content(content.as_bytes(), true)
            && let Value::Map(entries) = json.into_value()
        {
            for (key, value) in entries {
                if reserved_keys().contains(&key.as_str()) || output.contains_key(&key) {
                    continue;
                }
                output.insert(key, value);
            }
            flattened = true;
        }
        if !flattened {
            output.insert(STRUCTURE_KEY_CONTENT.to_owned(), Value::String(content));
        }
        if let Ok(text) = gf_json::marshal_value(&Value::Map(output)) {
            input.buffer.push_str(&text);
            input.buffer.push('\n');
        }
        input.next();
    })
}

/// A handler that prints `Key=Value` pairs, like `HandlerStructure`.
pub fn handler_structure() -> Handler {
    Arc::new(|input: &mut HandlerInput| {
        let mut parts: Vec<String> = Vec::new();
        add_structure_value(&mut parts, STRUCTURE_KEY_TIME, &input.time_format);
        if !input.level_format.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_LEVEL, &input.level_format);
        }
        if !input.caller_path.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_CALLER_PATH, &input.caller_path);
        }
        if !input.caller_func.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_CALLER_FUNC, &input.caller_func);
        }
        if !input.prefix.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_PREFIX, &input.prefix);
        }
        let mut values = input.values.clone();
        let mut content = input.content.clone();
        // An odd value count moves the first value into the content.
        if !values.len().is_multiple_of(2) {
            if !content.is_empty() {
                content.push(' ');
            }
            content.push_str(&gconv_string(&values[0]));
            values.remove(0);
        }
        if !content.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_CONTENT, &content);
        }
        for pair in values.chunks(2) {
            add_structure_value(&mut parts, &gconv_string(&pair[0]), &gconv_string(&pair[1]));
        }
        if !input.stack.is_empty() {
            add_structure_value(&mut parts, STRUCTURE_KEY_STACK, &input.stack);
        }
        let content = parts.join(" ").replace('\n', " ");
        input.buffer.push_str(&content);
        input.buffer.push('\n');
        input.next();
    })
}

/// Appends one `key=value` pair, quoting both sides when needed.
fn add_structure_value(parts: &mut Vec<String>, key: &str, value: &str) {
    let key = if needs_quoting(key) {
        go_quote(key)
    } else {
        key.to_owned()
    };
    let value = if needs_quoting(value) {
        go_quote(value)
    } else {
        value.to_owned()
    };
    parts.push(format!("{key}={value}"));
}

/// Reports whether `text` must be quoted in the structured output.
fn needs_quoting(text: &str) -> bool {
    if text.is_empty() {
        return true;
    }
    for character in text.chars() {
        let code = character as u32;
        if code < 0x80 {
            if matches!(character, ' ' | '=' | '"' | '\\') || code < 0x20 || code == 0x7F {
                return true;
            }
        } else if character.is_whitespace() || character.is_control() {
            return true;
        }
    }
    false
}

/// Quotes `text` like Go's `strconv.Quote` for the common escapes.
fn go_quote(text: &str) -> String {
    let mut output = String::from("\"");
    for character in text.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\t' => output.push_str("\\t"),
            '\r' => output.push_str("\\r"),
            character if (character as u32) < 0x20 => {
                output.push_str(&format!("\\u{:04x}", character as u32));
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::gfile::{mkdir, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-glog-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    /// A logger that writes into a channel instead of stdout.
    fn capturing_logger() -> (Logger, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel();
        let logger = Logger::new();
        logger.set_stdout_print(false);
        logger.set_writer(Arc::new(move |text: &str| {
            let _ = tx.send(text.to_owned());
        }));
        (logger, rx)
    }

    // Translated from the header/layout expectations of the Go tests.
    #[test]
    fn logger_header_starts_with_time_and_level() {
        let (logger, rx) = capturing_logger();
        logger.info("hello");
        let content = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("log content");
        assert!(content.ends_with("[INFO] hello\n"), "got {content}");
        assert_eq!(content.matches('[').count(), 1, "got {content}");
        // The default time layout begins with the year.
        assert!(content.starts_with("20"), "got {content}");
    }

    // Translated from Test_Level / level filtering behaviour.
    #[test]
    fn logger_filters_by_level() {
        let (logger, rx) = capturing_logger();
        logger.set_level_str("warn").expect("level string");
        logger.debug("debug");
        logger.info("info");
        logger.warning("warn");
        logger.error("error");
        let warn = rx.recv_timeout(Duration::from_secs(2)).expect("warn");
        assert!(warn.contains("[WARN] warn"));
        let error = rx.recv_timeout(Duration::from_secs(2)).expect("error");
        assert!(error.contains("[ERRO] error"));
        assert!(rx.try_recv().is_err(), "debug and info must be filtered");
    }

    #[test]
    fn logger_prefix_and_header_toggle() {
        let (logger, rx) = capturing_logger();
        logger.set_prefix("gf");
        logger.info("with-prefix");
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .expect("content")
                .contains("[INFO] gf with-prefix")
        );

        logger.set_header_print(false);
        logger.info("no-header");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).expect("content"),
            "no-header\n"
        );
    }

    #[test]
    fn logger_custom_level_prefix_and_no_level() {
        let (logger, rx) = capturing_logger();
        logger.set_level_prefix(LEVEL_INFO, "I");
        logger.info("x");
        assert!(
            rx.recv_timeout(Duration::from_secs(2))
                .expect("content")
                .contains("[I] x")
        );

        logger.print_std(vec![Value::String("plain".to_owned())]);
        let plain = rx.recv_timeout(Duration::from_secs(2)).expect("content");
        assert!(plain.ends_with("] plain\n") || plain.ends_with("plain\n"));
    }

    // Translated from Test_SetLevelStr errors.
    #[test]
    fn logger_level_string_validation() {
        let logger = Logger::new();
        logger.set_level_str("all").expect("all");
        assert_eq!(logger.level() & LEVEL_ALL, LEVEL_ALL);
        logger.set_level_str("prod").expect("prod");
        assert_eq!(logger.level() & LEVEL_ALL, LEVEL_PROD);
        assert!(logger.set_level_str("bogus").is_err());
    }

    // Translated from the JSON handler tests: the record is one JSON object.
    #[test]
    fn json_handler_outputs_structured_record() {
        let (logger, rx) = capturing_logger();
        logger.set_handlers(vec![handler_json()]);
        logger.set_prefix("p");
        logger.info("hello json");
        let content = rx.recv_timeout(Duration::from_secs(2)).expect("json");
        let trimmed = content.trim_end();
        assert!(
            trimmed.starts_with('{') && trimmed.ends_with('}'),
            "got {trimmed}"
        );
        assert!(trimmed.contains("\"Level\":\"INFO\""), "got {trimmed}");
        assert!(trimmed.contains("\"Prefix\":\"p\""), "got {trimmed}");
        assert!(
            trimmed.contains("\"Content\":\"hello json\""),
            "got {trimmed}"
        );
    }

    #[test]
    fn structure_handler_outputs_pairs() {
        let (logger, rx) = capturing_logger();
        logger.set_handlers(vec![handler_structure()]);
        logger.set_prefix("p q");
        logger.info(vec![
            Value::String("key".to_owned()),
            Value::String("value".to_owned()),
        ]);
        let content = rx.recv_timeout(Duration::from_secs(2)).expect("structure");
        assert!(content.contains("Level=INFO"), "got {content}");
        assert!(content.contains("Prefix=\"p q\""), "got {content}");
        assert!(content.contains("key=value"), "got {content}");
        assert!(content.ends_with('\n'));
    }

    // Translated from Test_SetPath and the file writing behaviour.
    #[test]
    fn logger_writes_to_file_and_rotates() {
        let base = sandbox("files");
        let logger = Logger::new();
        logger.set_stdout_print(false);
        logger.set_path(&base).expect("set path");
        logger.set_file("app.log");
        logger.info("first");
        let content = gfile::get_contents(&gfile::join(&[&base, "app.log"]));
        assert!(content.contains("[INFO] first"), "got {content}");

        // Rotation with one backup keeps the rotated file.
        logger.set_rotate_size(1);
        logger.set_rotate_backup_limit(1);
        logger.info("second");
        logger.info("third");
        let files = gfile::scan_dir_file(&base, "*.log", true).expect("scan");
        assert!(files.len() >= 2, "expected a rotated backup, got {files:?}");
        let backups: Vec<String> = files
            .iter()
            .filter(|file| {
                let name = gfile::basename(file);
                name.contains(".log.") || name.split('.').count() > 2
            })
            .cloned()
            .collect();
        assert!(
            !backups.is_empty(),
            "expected a rotated file, got {files:?}"
        );
        remove_all(&base).expect("cleanup");
    }

    // Translated from Test_Cat path chaining.
    #[test]
    fn logger_chaining_cat_changes_directory() {
        let base = sandbox("cat");
        let logger = Logger::new();
        logger.set_stdout_print(false);
        logger.set_path(&base).expect("set path");
        logger.set_file("cat.log");
        let chained = logger.with_cat("module");
        chained.info("chained");
        let expected = gfile::join(&[&base, "module", "cat.log"]);
        assert!(gfile::exists(&expected), "expected {expected}");
        assert!(gfile::get_contents(&expected).contains("chained"));
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn logger_instance_is_shared() {
        let first = instance(None);
        let second = instance(None);
        assert!(Arc::ptr_eq(&first, &second));
        let named = instance(Some("named"));
        assert!(!Arc::ptr_eq(&first, &named));
    }

    #[test]
    fn logger_get_stack_contains_frames() {
        let logger = Logger::new();
        let stack = logger.get_stack(0);
        assert!(!stack.is_empty(), "expected stack frames");
    }

    #[test]
    fn get_file_path_formats_the_pattern() {
        let logger = Logger::new();
        logger.set_path("/tmp/logs").expect("set path");
        logger.set_file("access-{Y-m-d}.log");
        let path = logger.get_file_path(SystemTime::now());
        assert!(path.contains("access-20"), "got {path}");
        assert!(path.ends_with(".log"), "got {path}");
    }
}
