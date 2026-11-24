//! Port of `errors/gerror`: errors with a code, deferred text arguments, a
//! wrapped cause and a captured stack.
//!
//! Design decisions are recorded in ADR-012 of `DECISIONS.md`: the chain is a
//! `std::error::Error` chain, formatting uses the [`gfmt`] verb subset, and
//! stack frames come from [`Backtrace`].

use std::backtrace::Backtrace;
use std::borrow::Cow;
use std::error::Error as StdError;
use std::fmt;
use std::sync::Arc;

use gf_core::Value;
use gf_internal::errors::is_stack_mode_brief;

use crate::gcode::{self, Code, LocalCode};
use crate::gfmt;

const COMMA_SEPARATOR_SPACE: &str = ", ";

/// An error with GoFrame's extra information.
pub struct Error {
    inner: Option<Box<dyn StdError + Send + Sync>>,
    text: String,
    args: Box<[Value]>,
    code: LocalCode,
    stack: Option<Arc<Backtrace>>,
    skip: usize,
}

/// Options for [`new_with_option`], replacing Go's `gerror.Option`.
pub struct ErrorOption {
    /// Wrapped error, if any.
    pub error: Option<Box<dyn StdError + Send + Sync>>,
    /// Whether to record stack information.
    pub stack: bool,
    /// Error text.
    pub text: String,
    /// Text arguments.
    pub args: Vec<Value>,
    /// Error code.
    pub code: LocalCode,
}

fn capture_stack() -> Arc<Backtrace> {
    Arc::new(Backtrace::force_capture())
}

impl Error {
    /// Creates an error with `text` and a captured stack.
    pub fn new(text: impl Into<String>) -> Error {
        Error {
            inner: None,
            text: text.into(),
            args: Box::new([]),
            code: gcode::CODE_NIL.clone(),
            stack: Some(capture_stack()),
            skip: 0,
        }
    }

    /// Creates an error whose text is formatted from `format` and `args`.
    pub fn newf(format: impl Into<String>, args: Vec<Value>) -> Error {
        Error {
            inner: None,
            text: format.into(),
            args: args.into_boxed_slice(),
            code: gcode::CODE_NIL.clone(),
            stack: Some(capture_stack()),
            skip: 0,
        }
    }

    /// Creates an error, skipping `skip` caller frames when the stack is
    /// rendered.
    pub fn new_skip(skip: usize, text: impl Into<String>) -> Error {
        let mut err = Error::new(text);
        err.skip = skip;
        err
    }

    /// Creates a formatted error, skipping `skip` caller frames.
    pub fn new_skipf(skip: usize, format: impl Into<String>, args: Vec<Value>) -> Error {
        let mut err = Error::newf(format, args);
        err.skip = skip;
        err
    }

    /// Creates an error with `code` and the given texts joined by `", "`.
    pub fn new_code(code: &LocalCode, texts: &[&str]) -> Error {
        Error {
            inner: None,
            text: texts.join(COMMA_SEPARATOR_SPACE),
            args: Box::new([]),
            code: code.clone(),
            stack: Some(capture_stack()),
            skip: 0,
        }
    }

    /// Creates a formatted error with `code`.
    pub fn new_codef(code: &LocalCode, format: impl Into<String>, args: Vec<Value>) -> Error {
        Error {
            inner: None,
            text: format.into(),
            args: args.into_boxed_slice(),
            code: code.clone(),
            stack: Some(capture_stack()),
            skip: 0,
        }
    }

    /// Creates an error with `code`, skipping `skip` caller frames.
    pub fn new_code_skip(code: &LocalCode, skip: usize, texts: &[&str]) -> Error {
        let mut err = Error::new_code(code, texts);
        err.skip = skip;
        err
    }

    /// Creates a formatted error with `code`, skipping `skip` caller frames.
    pub fn new_code_skipf(
        code: &LocalCode,
        skip: usize,
        format: impl Into<String>,
        args: Vec<Value>,
    ) -> Error {
        let mut err = Error::new_codef(code, format, args);
        err.skip = skip;
        err
    }

    /// Returns the current level's rendered text.
    pub fn error_string(&self) -> String {
        let mut out = String::with_capacity(self.text.len() + 16);
        self.push_error_text(&mut out);
        out
    }

    fn push_error_text(&self, out: &mut String) {
        let text = self.text_with_args_cow();
        let text = if text.is_empty() {
            Cow::Borrowed(self.code.message())
        } else {
            text
        };
        out.push_str(&text);
        let Some(inner) = self.inner.as_deref() else {
            return;
        };
        if !text.is_empty() {
            out.push_str(": ");
        }
        match inner.downcast_ref::<Error>() {
            Some(inner) => inner.push_error_text(out),
            None => {
                use std::fmt::Write as _;
                // INFALLIBLE: writing to a String cannot fail.
                let _ = write!(out, "{inner}");
            }
        }
    }

    /// Returns the raw text of this level.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Returns the text arguments of this level.
    pub fn args(&self) -> &[Value] {
        &self.args
    }

    /// Returns the text formatted with its arguments.
    pub fn text_with_args(&self) -> String {
        self.text_with_args_cow().into_owned()
    }

    fn text_with_args_cow(&self) -> Cow<'_, str> {
        if self.args.is_empty() {
            Cow::Borrowed(&self.text)
        } else {
            Cow::Owned(gfmt::sprintf(&self.text, &self.args))
        }
    }

    /// Returns this level's error code.
    pub fn code(&self) -> &LocalCode {
        &self.code
    }

    /// Replaces this level's error code.
    pub fn set_code(&mut self, code: LocalCode) {
        self.code = code;
    }

    /// Returns this level's text without the wrapped error.
    pub fn current(&self) -> Error {
        Error {
            inner: None,
            text: self.text.clone(),
            args: self.args.to_vec().into_boxed_slice(),
            code: self.code.clone(),
            stack: self.stack.clone(),
            skip: self.skip,
        }
    }

    /// Returns this level's shown text: formatted text, or the full error when
    /// the text is empty.
    fn top_text(&self) -> String {
        if self.text.is_empty() {
            self.error_string()
        } else {
            self.text_with_args()
        }
    }

    /// Returns the rendered stack of this level and its gerror layers.
    pub fn stack(&self) -> String {
        let brief = is_stack_mode_brief();
        let mut infos: Vec<(String, Vec<Frame>)> = Vec::new();
        let mut current = self;
        loop {
            infos.push((current.top_text(), current.frames(brief)));
            match current.inner.as_deref() {
                Some(inner) => match inner.downcast_ref::<Error>() {
                    Some(next) => current = next,
                    None => {
                        infos.push((inner.to_string(), Vec::new()));
                        break;
                    }
                },
                None => break,
            }
        }
        deduplicate_frames(&mut infos);
        format_stack_infos(&infos)
    }

    /// Returns the error string followed by the stack.
    pub fn verbose(&self) -> String {
        format!("{}\n{}", self.error_string(), self.stack())
    }

    /// Serializes the error as a JSON string of its rendered text.
    pub fn marshal_json(&self) -> String {
        gfmt::quote(&self.error_string())
    }

    fn frames(&self, brief: bool) -> Vec<Frame> {
        let Some(backtrace) = self.stack.as_deref() else {
            return Vec::new();
        };
        let rendered = backtrace.to_string();
        let mut frames = Vec::new();
        let mut pending_function: Option<String> = None;
        for line in rendered.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("at ") {
                if let Some(function) = pending_function.take() {
                    frames.push(Frame {
                        function,
                        file_line: rest.to_owned(),
                    });
                }
                continue;
            }
            let Some((index, function)) = trimmed.split_once(':') else {
                continue;
            };
            if index.trim().parse::<usize>().is_ok() {
                let function = function.trim().to_owned();
                if function.is_empty() {
                    continue;
                }
                if keep_frame(&function, brief) {
                    pending_function = Some(function);
                } else {
                    pending_function = None;
                }
            }
        }
        frames.into_iter().skip(self.skip).collect()
    }
}

fn keep_frame(function: &str, brief: bool) -> bool {
    if function.contains('<') {
        return false;
    }
    if brief {
        return !function.starts_with("gf_");
    }
    !function.starts_with("gf_errors::gerror")
}

#[derive(Clone, Debug)]
struct Frame {
    function: String,
    file_line: String,
}

fn deduplicate_frames(infos: &mut [(String, Vec<Frame>)]) {
    let mut seen = std::collections::BTreeSet::new();
    for (_, frames) in infos.iter_mut().rev() {
        frames.retain(|frame| seen.insert(frame.file_line.clone()));
    }
}

fn format_stack_infos(infos: &[(String, Vec<Frame>)]) -> String {
    let mut out = String::new();
    for (i, (message, frames)) in infos.iter().enumerate() {
        out.push_str(&format!("{}. {}\n", i + 1, message));
        for (j, frame) in frames.iter().enumerate() {
            let space = if j >= 9 { " " } else { "  " };
            out.push_str(&format!(
                "   {}).{}{}\n        {}\n",
                j + 1,
                space,
                frame.function,
                frame.file_line
            ));
        }
    }
    out
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error_string())
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error_string())
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        self.inner
            .as_deref()
            .map(|inner| inner as &(dyn StdError + 'static))
    }
}

/// Creates an error with `text`.
pub fn new(text: impl Into<String>) -> Error {
    Error::new(text)
}

/// Creates a formatted error.
pub fn newf(format: impl Into<String>, args: Vec<Value>) -> Error {
    Error::newf(format, args)
}

/// Creates an error skipping `skip` caller frames.
pub fn new_skip(skip: usize, text: impl Into<String>) -> Error {
    Error::new_skip(skip, text)
}

/// Creates a formatted error skipping `skip` caller frames.
pub fn new_skipf(skip: usize, format: impl Into<String>, args: Vec<Value>) -> Error {
    Error::new_skipf(skip, format, args)
}

/// Creates an error with `code` and texts joined by `", "`.
pub fn new_code(code: &LocalCode, texts: &[&str]) -> Error {
    Error::new_code(code, texts)
}

/// Creates a formatted error with `code`.
pub fn new_codef(code: &LocalCode, format: impl Into<String>, args: Vec<Value>) -> Error {
    Error::new_codef(code, format, args)
}

/// Creates an error with `code` skipping `skip` caller frames.
pub fn new_code_skip(code: &LocalCode, skip: usize, texts: &[&str]) -> Error {
    Error::new_code_skip(code, skip, texts)
}

/// Creates a formatted error with `code` skipping `skip` caller frames.
pub fn new_code_skipf(
    code: &LocalCode,
    skip: usize,
    format: impl Into<String>,
    args: Vec<Value>,
) -> Error {
    Error::new_code_skipf(code, skip, format, args)
}

/// Creates a custom error from [`ErrorOption`].
pub fn new_with_option(option: ErrorOption) -> Error {
    Error {
        inner: option.error,
        text: option.text,
        args: option.args.into_boxed_slice(),
        code: option.code,
        stack: if option.stack {
            Some(capture_stack())
        } else {
            None
        },
        skip: 0,
    }
}

/// Wraps `err`, inheriting its code. Returns `None` when `err` is `None`.
pub fn wrap(err: Option<Error>, text: impl Into<String>) -> Option<Error> {
    err.map(|err| {
        let code = code(&err);
        Error {
            inner: Some(Box::new(err)),
            text: text.into(),
            args: Box::new([]),
            code,
            stack: Some(capture_stack()),
            skip: 0,
        }
    })
}

/// Wraps `err` with formatted text, inheriting its code.
pub fn wrapf(err: Option<Error>, format: impl Into<String>, args: Vec<Value>) -> Option<Error> {
    err.map(|err| {
        let code = code(&err);
        Error {
            inner: Some(Box::new(err)),
            text: format.into(),
            args: args.into_boxed_slice(),
            code,
            stack: Some(capture_stack()),
            skip: 0,
        }
    })
}

/// Wraps a non-GoFrame error. Returns `None` when `err` is `None`.
pub fn wrap_std(
    err: Option<Box<dyn StdError + Send + Sync>>,
    text: impl Into<String>,
) -> Option<Error> {
    err.map(|err| Error {
        inner: Some(err),
        text: text.into(),
        args: Box::new([]),
        code: gcode::CODE_NIL.clone(),
        stack: Some(capture_stack()),
        skip: 0,
    })
}

/// Wraps `err` with `code`.
pub fn wrap_code(code: &LocalCode, err: Option<Error>, texts: &[&str]) -> Option<Error> {
    err.map(|err| Error {
        inner: Some(Box::new(err)),
        text: texts.join(COMMA_SEPARATOR_SPACE),
        args: Box::new([]),
        code: code.clone(),
        stack: Some(capture_stack()),
        skip: 0,
    })
}

/// Wraps `err` with `code` and formatted text.
pub fn wrap_codef(
    code: &LocalCode,
    err: Option<Error>,
    format: impl Into<String>,
    args: Vec<Value>,
) -> Option<Error> {
    err.map(|err| Error {
        inner: Some(Box::new(err)),
        text: format.into(),
        args: args.into_boxed_slice(),
        code: code.clone(),
        stack: Some(capture_stack()),
        skip: 0,
    })
}

/// Wraps `err` with `code`, skipping `skip` caller frames.
pub fn wrap_code_skip(
    code: &LocalCode,
    skip: usize,
    err: Option<Error>,
    texts: &[&str],
) -> Option<Error> {
    err.map(|err| Error {
        inner: Some(Box::new(err)),
        text: texts.join(COMMA_SEPARATOR_SPACE),
        args: Box::new([]),
        code: code.clone(),
        stack: Some(capture_stack()),
        skip,
    })
}

/// Wraps `err` with `code` and formatted text, skipping `skip` caller frames.
pub fn wrap_code_skipf(
    code: &LocalCode,
    skip: usize,
    err: Option<Error>,
    format: impl Into<String>,
    args: Vec<Value>,
) -> Option<Error> {
    err.map(|err| Error {
        inner: Some(Box::new(err)),
        text: format.into(),
        args: args.into_boxed_slice(),
        code: code.clone(),
        stack: Some(capture_stack()),
        skip,
    })
}

/// Returns the effective error code: this level's code, or the wrapped error's.
pub fn code(err: &Error) -> LocalCode {
    if err.code != gcode::CODE_NIL {
        return err.code.clone();
    }
    match err.inner.as_deref() {
        Some(inner) => match inner.downcast_ref::<Error>() {
            Some(inner) => code(inner),
            None => gcode::CODE_NIL.clone(),
        },
        None => gcode::CODE_NIL.clone(),
    }
}

/// Returns whether `err` carries `code` at any level.
pub fn has_code(err: &Error, code: &LocalCode) -> bool {
    let mut current = err;
    loop {
        if &current.code == code {
            return true;
        }
        match current
            .inner
            .as_deref()
            .and_then(|inner| inner.downcast_ref::<Error>())
        {
            Some(next) => current = next,
            None => return false,
        }
    }
}

/// Returns the root cause rendered as an error.
pub fn cause(err: &Error) -> Error {
    let mut deepest = err;
    while let Some(next) = deepest
        .inner
        .as_deref()
        .and_then(|inner| inner.downcast_ref::<Error>())
    {
        deepest = next;
    }
    match deepest.inner.as_deref() {
        Some(inner) => Error::new(inner.to_string()),
        None => Error::new(deepest.text_with_args()),
    }
}

/// Returns the next level error.
pub fn unwrap(err: &Error) -> Option<&(dyn StdError + 'static)> {
    err.source()
}

/// Returns this level without its wrapped error.
pub fn current(err: &Error) -> Error {
    err.current()
}

/// Returns whether `err` supports stacks, which every [`Error`] does.
pub fn has_stack(_err: &Error) -> bool {
    true
}

/// Returns the rendered stack of `err`.
pub fn stack(err: &Error) -> String {
    err.stack()
}

/// Reports whether two errors have the same code and current-level text.
pub fn equal(err: &Error, target: &Error) -> bool {
    err.code == target.code && err.text_with_args() == target.top_text()
}

/// Reports whether `target` appears in `err`'s chain under [`equal`].
pub fn is(err: &Error, target: &Error) -> bool {
    let mut current = err;
    loop {
        if equal(current, target) {
            return true;
        }
        match current
            .inner
            .as_deref()
            .and_then(|inner| inner.downcast_ref::<Error>())
        {
            Some(next) => current = next,
            None => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gcode::{
        CODE_INTERNAL_ERROR, CODE_INVALID_PARAMETER, CODE_NOT_AUTHORIZED, CODE_NOT_FOUND,
        CODE_UNKNOWN, CODE_VALIDATION_FAILED,
    };

    #[derive(Debug)]
    struct PlainError(&'static str);

    impl fmt::Display for PlainError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str(self.0)
        }
    }

    impl StdError for PlainError {}

    fn chain(texts: &[&str]) -> Error {
        let mut err = Error::new(texts[0]);
        for text in &texts[1..] {
            err = wrap(Some(err), *text).expect("wrap");
        }
        err
    }

    #[test]
    fn error_size_stays_small() {
        assert!(
            std::mem::size_of::<Error>() <= 128,
            "Error size is {}",
            std::mem::size_of::<Error>()
        );
    }

    #[test]
    fn new_and_newf() {
        assert_eq!(Error::new("1").to_string(), "1");
        assert_eq!(newf("%d", vec![Value::Int(1)]).to_string(), "1");
        assert_eq!(
            newf(
                "name=%s age=%d",
                vec![Value::String("张三".into()), Value::Int(30)]
            )
            .to_string(),
            "name=张三 age=30"
        );
        assert_eq!(new_skip(1, "1").to_string(), "1");
        assert_eq!(new_skipf(1, "%d", vec![Value::Int(1)]).to_string(), "1");
    }

    #[test]
    fn wrap_builds_the_chain() {
        assert!(wrap(None, "test").is_none());
        assert_eq!(chain(&["1", "2", "3"]).to_string(), "3: 2: 1");
        assert_eq!(chain(&["1", ""]).to_string(), "1");
        assert_eq!(
            wrapf(Some(Error::new("1")), "%d", vec![Value::Int(2)])
                .expect("wrapf")
                .to_string(),
            "2: 1"
        );
        assert_eq!(
            wrap_std(Some(Box::new(PlainError("1"))), "2")
                .expect("wrap_std")
                .to_string(),
            "2: 1"
        );
    }

    #[test]
    fn cause_is_the_root() {
        assert_eq!(cause(&Error::new("1")).to_string(), "1");
        assert_eq!(cause(&chain(&["1", "2", "3"])).to_string(), "1");
        assert_eq!(
            cause(&wrap_std(Some(Box::new(PlainError("root"))), "outer").expect("wrap"))
                .to_string(),
            "root"
        );
    }

    #[test]
    fn current_drops_the_wrapped_error() {
        let err = chain(&["1", "2", "3"]);
        assert_eq!(err.to_string(), "3: 2: 1");
        let cur = err.current();
        assert_eq!(cur.to_string(), "3");
        assert!(cur.source().is_none());
        assert_eq!(current(&err).to_string(), "3");
    }

    #[test]
    fn unwrap_walks_one_level() {
        let err = chain(&["1", "2", "3"]);
        let first = unwrap(&err)
            .expect("source")
            .downcast_ref::<Error>()
            .expect("gerror");
        assert_eq!(first.to_string(), "2: 1");
        let second = unwrap(first)
            .expect("source")
            .downcast_ref::<Error>()
            .expect("gerror");
        assert_eq!(second.to_string(), "1");
        assert!(unwrap(second).is_none());
    }

    #[test]
    fn code_defaults_wraps_and_overrides() {
        assert_eq!(code(&Error::new("x")), gcode::CODE_NIL);
        assert_eq!(
            code(&Error::new_code(&CODE_UNKNOWN, &["123"])),
            CODE_UNKNOWN
        );
        let custom = gcode::new(1, "", None);
        assert_eq!(
            code(&Error::new_codef(
                &custom,
                "%s",
                vec![Value::String("123".into())]
            ))
            .code(),
            1
        );

        let inherited =
            wrap(Some(Error::new_code(&CODE_NOT_AUTHORIZED, &["1"])), "2").expect("wrap");
        assert_eq!(code(&inherited), CODE_NOT_AUTHORIZED);
        assert_eq!(inherited.to_string(), "2: 1");

        let overridden = wrap_code(&CODE_INVALID_PARAMETER, Some(inherited), &["5"]).expect("wrap");
        assert_eq!(code(&overridden), CODE_INVALID_PARAMETER);
        assert_eq!(overridden.to_string(), "5: 2: 1");

        assert!(wrap_code(&CODE_INVALID_PARAMETER, None, &["x"]).is_none());
    }

    #[test]
    fn has_code_walks_the_chain() {
        let base = Error::new("1");
        assert!(!has_code(&base, &CODE_NOT_AUTHORIZED));
        let with_code = wrap_code(&CODE_NOT_AUTHORIZED, Some(base), &["2"]).expect("wrap");
        let outer = chain_with(with_code, &["3", "4"]);
        assert!(has_code(&outer, &CODE_NOT_AUTHORIZED));
        let top = wrap_code(&CODE_INVALID_PARAMETER, Some(outer), &["5"]).expect("wrap");
        assert!(has_code(&top, &CODE_NOT_AUTHORIZED));
        assert!(has_code(&top, &CODE_INVALID_PARAMETER));
        assert!(!has_code(&top, &CODE_INTERNAL_ERROR));
    }

    fn chain_with(inner: Error, texts: &[&str]) -> Error {
        let mut err = inner;
        for text in texts {
            err = wrap(Some(err), *text).expect("wrap");
        }
        err
    }

    #[test]
    fn set_code_changes_only_the_code() {
        let mut err = Error::new("123");
        assert_eq!(code(&err), gcode::CODE_NIL);
        err.set_code(CODE_VALIDATION_FAILED.clone());
        assert_eq!(code(&err), CODE_VALIDATION_FAILED);
        assert_eq!(err.to_string(), "123");
    }

    #[test]
    fn equal_compares_code_and_current_text() {
        let a = Error::new("1");
        let b = Error::new("1");
        let c = Error::new("4");
        assert!(equal(&a, &b));
        assert!(!equal(&a, &c));
        let coded = Error::new_code(&CODE_UNKNOWN, &["1"]);
        assert!(!equal(&a, &coded));
        assert!(equal(&coded, &coded));
    }

    #[test]
    fn is_finds_equal_layers() {
        let root = Error::new("1");
        let err = chain(&["1", "2", "3"]);
        assert!(is(&err, &root));
        assert!(!is(&err, &Error::new("not found")));
        let coded = Error::new_code(&CODE_NOT_FOUND, &["1"]);
        assert!(is(&coded, &Error::new_code(&CODE_NOT_FOUND, &["1"])));
        assert!(!is(&coded, &Error::new("1")));
    }

    #[test]
    fn json_marshals_the_error_string() {
        assert_eq!(chain(&["1", "2"]).marshal_json(), "\"2: 1\"");
        assert_eq!(Error::new("test").marshal_json(), "\"test\"");
        assert_eq!(Error::new("test \"\"").marshal_json(), "\"test \\\"\\\"\"");
        assert_eq!(
            Error::new("line1\nline2").marshal_json(),
            "\"line1\\nline2\""
        );
    }

    #[test]
    fn text_and_args_are_kept_apart() {
        let err = Error::new("text");
        assert_eq!(err.text(), "text");
        assert!(err.args().is_empty());

        let err = newf("text: %s", vec![Value::String("arg1".into())]);
        assert_eq!(err.text(), "text: %s");
        assert_eq!(err.args().to_vec(), vec![Value::String("arg1".into())]);
        assert_eq!(err.text_with_args(), "text: arg1");

        let wrapped = wrapf(
            Some(Error::new("text")),
            "wrap: %s",
            vec![Value::String("arg1".into())],
        )
        .expect("wrapf");
        assert_eq!(wrapped.text(), "wrap: %s");
        assert_eq!(wrapped.args().to_vec(), vec![Value::String("arg1".into())]);
        assert_eq!(wrapped.to_string(), "wrap: arg1: text");
    }

    #[test]
    fn empty_text_falls_back_to_the_code_message() {
        assert_eq!(
            Error::new_code(&CODE_INTERNAL_ERROR, &[]).to_string(),
            "Internal Error"
        );
        let wrapped =
            wrap_code(&CODE_INTERNAL_ERROR, Some(Error::new("inner")), &[]).expect("wrap_code");
        assert_eq!(wrapped.to_string(), "Internal Error: inner");
    }

    #[test]
    fn multiple_texts_join_with_comma_space() {
        assert_eq!(
            Error::new_code(&CODE_INTERNAL_ERROR, &["text1", "text2", "text3"]).to_string(),
            "text1, text2, text3"
        );
        assert_eq!(
            Error::new_code_skip(&CODE_INTERNAL_ERROR, 0, &["text1", "text2"]).to_string(),
            "text1, text2"
        );
        assert_eq!(
            wrap_code(
                &CODE_INTERNAL_ERROR,
                Some(Error::new("inner")),
                &["text1", "text2"]
            )
            .expect("wrap_code")
            .to_string(),
            "text1, text2: inner"
        );
    }

    #[test]
    fn stack_numbers_every_layer() {
        let err = wrap_std(Some(Box::new(PlainError("standard error"))), "wrapped").expect("wrap");
        let rendered = stack(&err);
        assert!(rendered.contains("1. wrapped\n"), "{rendered}");
        assert!(rendered.contains("2. standard error\n"), "{rendered}");

        let nested = wrap(
            wrap_std(Some(Box::new(PlainError("standard error"))), "mid"),
            "outer",
        )
        .expect("wrap");
        let rendered = stack(&nested);
        assert!(rendered.contains("1. outer\n"), "{rendered}");
        assert!(rendered.contains("2. mid\n"), "{rendered}");
        assert!(rendered.contains("3. standard error\n"), "{rendered}");
    }

    #[test]
    fn stack_is_not_the_bare_error_string() {
        let err = Error::new("test error");
        let rendered = err.stack();
        assert!(rendered.starts_with("1. test error\n"));
        assert_ne!(rendered, "test error");
        assert!(has_stack(&err));
    }

    #[test]
    fn verbose_appends_the_stack() {
        let err = chain(&["1", "2"]);
        let verbose = err.verbose();
        assert!(verbose.starts_with("2: 1\n1. 2\n"));
    }

    #[test]
    fn option_controls_stack_capture() {
        let without_stack = new_with_option(ErrorOption {
            error: None,
            stack: false,
            text: "no stack".into(),
            args: Vec::new(),
            code: gcode::CODE_NIL.clone(),
        });
        assert_eq!(without_stack.stack(), "1. no stack\n");

        let with_stack = new_with_option(ErrorOption {
            error: Some(Box::new(PlainError("base"))),
            stack: true,
            text: "text".into(),
            args: Vec::new(),
            code: CODE_NOT_AUTHORIZED.clone(),
        });
        assert_eq!(code(&with_stack), CODE_NOT_AUTHORIZED);
        assert_eq!(with_stack.to_string(), "text: base");
        assert!(with_stack.stack().contains("1. text\n"));
    }

    #[test]
    fn std_error_source_exposes_the_chain() {
        fn root(err: &Error) -> &(dyn StdError + 'static) {
            let mut current: &dyn StdError = err;
            while let Some(next) = current.source() {
                current = next;
            }
            current
        }
        let err = chain(&["1", "2", "3"]);
        assert_eq!(root(&err).to_string(), "1");
    }

    #[test]
    fn long_chains_render_and_find_the_root() {
        let mut texts: Vec<String> = vec!["base".to_owned()];
        texts.extend((0..10).map(|i| format!("w{i}")));
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let err = chain(&refs);
        assert!(err.to_string().starts_with("w9: w8:"));
        assert!(err.to_string().ends_with(": base"));
        assert_eq!(cause(&err).to_string(), "base");
        assert!(is(&err, &Error::new("w5")));
        assert!(!is(&err, &Error::new("w99")));
    }
}
