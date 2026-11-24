//! Port of `internal/intlog` (`intlog.go`).
//!
//! GoFrame's internal debug logger writes `time [INTE] file:line content` when
//! the debug flag is on. The port keeps the output shape and reads the caller
//! from `std::backtrace` instead of `gdebug` (ADR-034), because `gf-debug`
//! depends on `gf-encoding`, which depends on this crate.
//!
//! Divergences: there is no `context.Context` parameter (OpenTelemetry trace
//! ids do not exist in the port), and `print`/`printf`/`error`/`errorf`
//! receive text that the caller has already formatted.

use chrono::Local;

use crate::utils;

/// The stack filter key, like Go's `stackFilterKey`.
const STACK_FILTER_KEY: &str = "intlog";

/// Prints a line of debug text.
pub fn print(content: &str) {
    if !utils::is_debug_enabled() {
        return;
    }
    do_print(content, false);
}

/// Prints a line of debug text; the caller formats it (ADR-034).
pub fn printf(content: &str) {
    if !utils::is_debug_enabled() {
        return;
    }
    do_print(content, false);
}

/// Prints a line of debug error text.
pub fn error(content: &str) {
    if !utils::is_debug_enabled() {
        return;
    }
    do_print(content, true);
}

/// Prints a line of debug error text; the caller formats it (ADR-034).
pub fn errorf(content: &str) {
    if !utils::is_debug_enabled() {
        return;
    }
    do_print(content, true);
}

/// Prints the output of `f`; `f` only runs when debug is enabled.
pub fn print_func(f: impl FnOnce() -> String) {
    if !utils::is_debug_enabled() {
        return;
    }
    let content = f();
    if content.is_empty() {
        return;
    }
    do_print(&content, false);
}

/// Prints the output of `f` as an error; `f` only runs when debug is enabled.
pub fn error_func(f: impl FnOnce() -> String) {
    if !utils::is_debug_enabled() {
        return;
    }
    let content = f();
    if content.is_empty() {
        return;
    }
    do_print(&content, true);
}

/// Renders the log line for `content`; `stack` appends the caller stack.
pub fn render(content: &str, stack: bool) -> String {
    let mut buffer = String::new();
    buffer.push_str(&Local::now().format("%Y-%m-%d %H:%M:%S%.3f").to_string());
    buffer.push_str(" [INTE] ");
    buffer.push_str(&file());
    buffer.push(' ');
    buffer.push_str(content);
    buffer.push('\n');
    if stack {
        buffer.push_str("Caller Stack:\n");
        buffer.push_str(&stack_text());
    }
    buffer
}

fn do_print(content: &str, stack: bool) {
    if !utils::is_debug_enabled() {
        return;
    }
    print!("{}", render(content, stack));
}

/// Returns the caller file name along with its line number.
fn file() -> String {
    match caller_from_backtrace() {
        Some(frame) => format!("{}:{}", frame.file, frame.line),
        None => "unknown:0".to_owned(),
    }
}

/// Returns a numbered stack in GoFrame's format, skipping intlog frames.
fn stack_text() -> String {
    let mut output = String::new();
    for (position, frame) in frames().into_iter().enumerate() {
        let index = position + 1;
        let space = if index > 9 { " " } else { "  " };
        output.push_str(&format!(
            "{index}.{space}{}\n    {}:{}\n",
            frame.function, frame.file, frame.line
        ));
    }
    output
}

/// One captured caller frame.
struct Frame {
    function: String,
    file: String,
    line: u32,
}

/// Reports whether a frame belongs to the log machinery itself.
fn is_filtered(function: &str, file: &str) -> bool {
    function.contains("gf_internal::intlog")
        || function.contains("intlog::")
        || function.contains("stdlib::backtrace")
        || function.contains("std::backtrace")
        || file.contains(STACK_FILTER_KEY)
}

/// Captures the caller frames, skipping the log machinery.
///
/// `std::backtrace::Backtrace::frames` is unstable, so the stable `Display`
/// rendering is parsed instead.
fn frames() -> Vec<Frame> {
    use std::backtrace::Backtrace;

    let rendered = Backtrace::force_capture().to_string();
    let mut result = Vec::new();
    let mut pending: Option<(String, Option<(String, u32)>)> = None;
    for line in rendered.lines() {
        let trimmed = line.trim_start();
        if let Some(function) = frame_function(trimmed) {
            if let Some((function, location)) = pending.take() {
                push_frame(&mut result, function, location);
            }
            pending = Some((function, None));
        } else if let Some(location) = trimmed.strip_prefix("at ")
            && let Some((file, number)) = parse_location(location)
            && let Some((_, slot)) = pending.as_mut()
        {
            *slot = Some((file, number));
        }
    }
    if let Some((function, location)) = pending.take() {
        push_frame(&mut result, function, location);
    }
    result
}

/// Parses a rendered frame header (`"3: crate::function"`) into its function.
fn frame_function(line: &str) -> Option<String> {
    let (number, function) = line.split_once(": ")?;
    number.trim().parse::<u32>().ok()?;
    Some(function.trim().to_owned())
}

/// Parses a rendered location (`"path:line:column"`) into file and line.
fn parse_location(location: &str) -> Option<(String, u32)> {
    let mut parts = location.rsplitn(3, ':');
    let _column = parts.next()?;
    let line = parts.next()?.trim().parse::<u32>().ok()?;
    let file = parts.next()?.trim().to_owned();
    if file.is_empty() {
        return None;
    }
    Some((file, line))
}

/// Adds a frame unless it belongs to the log machinery, keeping only the file name.
fn push_frame(result: &mut Vec<Frame>, function: String, location: Option<(String, u32)>) {
    let (file, line) = location.unwrap_or_default();
    if is_filtered(&function, &file) {
        return;
    }
    result.push(Frame {
        function,
        file: file.rsplit(['/', '\\']).next().unwrap_or(&file).to_owned(),
        line,
    });
}

/// Returns the first non-intlog caller frame.
fn caller_from_backtrace() -> Option<Frame> {
    frames().into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_has_the_go_shape() {
        let line = render("hello", false);
        assert!(line.contains(" [INTE] "));
        assert!(line.ends_with("hello\n"));
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn render_appends_the_stack() {
        let line = render("boom", true);
        assert!(line.contains("Caller Stack:\n"));
    }

    #[test]
    fn debug_flag_gates_the_output() {
        let previous = utils::is_debug_enabled();
        utils::set_debug_enabled(true);
        assert!(utils::is_debug_enabled());
        print("intlog test line");
        utils::set_debug_enabled(previous);
    }

    #[test]
    fn caller_is_reported_in_debug_builds() {
        let frame = caller_from_backtrace();
        assert!(frame.is_some());
    }
}
