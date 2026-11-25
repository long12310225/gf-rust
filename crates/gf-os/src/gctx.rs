//! Port of `os/gctx` without `context.Context` (ADR-182).
//!
//! GoFrame wraps `context.Context` to carry a trace id/span and OpenTelemetry
//! propagation. Rust has no context values, so [`Context`] is an explicit
//! value that carries the same ids (plus an arbitrary value map replacing
//! `context.WithValue`) and is passed as a parameter.
//!
//! Not ported: goroutine-local context, OpenTelemetry's global propagator and
//! real span creation/ending; ids are W3C `traceparent`-compatible strings.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gf_core::Value;
use gf_util::grand;

/// The W3C `traceparent` environment/header name.
pub const TRACEPARENT_HEADER: &str = "traceparent";

/// A trace context, like the value GoFrame stores in `context.Context`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Context {
    trace_id: String,
    span_id: String,
    span_name: String,
    values: HashMap<String, Value>,
}

impl Context {
    /// Creates an empty context, like `context.Background`.
    pub fn background() -> Context {
        Context::default()
    }

    /// Creates a context with a new trace id and span, like `gctx.New`.
    pub fn new() -> Context {
        Context {
            trace_id: new_trace_id(),
            span_id: new_span_id(),
            span_name: "gctx.New".to_owned(),
            values: HashMap::new(),
        }
    }

    /// Attaches `name` as the context span, like `WithSpan`.
    ///
    /// GoFrame returns the context unchanged when it already carries a trace
    /// id; the port keeps that behaviour (child spans are out of scope).
    pub fn with_span(&self, name: &str) -> Context {
        if !self.trace_id.is_empty() {
            return self.clone();
        }
        let mut context = Context::new();
        context.span_name = if name.is_empty() {
            "gctx.WithSpan".to_owned()
        } else {
            name.to_owned()
        };
        context.values = self.values.clone();
        context
    }

    /// Returns a copy carrying `value` under `key`, like `context.WithValue`.
    pub fn with_value(&self, key: &str, value: Value) -> Context {
        let mut context = self.clone();
        context.values.insert(key.to_owned(), value);
        context
    }

    /// Reads a value stored by [`Context::with_value`], like `Context.Value`.
    pub fn value(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }

    /// The trace id (empty for a background context).
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// The span id (empty for a background context).
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// The span name.
    pub fn span_name(&self) -> &str {
        &self.span_name
    }

    /// Formats the context as a W3C `traceparent` value.
    pub fn traceparent(&self) -> String {
        format!("00-{}-{}-01", self.trace_id, self.span_id)
    }

    /// Parses a W3C `traceparent` value into a context.
    pub fn from_traceparent(header: &str) -> Option<Context> {
        let parts: Vec<&str> = header.trim().split('-').collect();
        if parts.len() != 4 || parts[0] != "00" {
            return None;
        }
        if !valid_hex(parts[1], 32) || !valid_hex(parts[2], 16) {
            return None;
        }
        Some(Context {
            trace_id: parts[1].to_ascii_lowercase(),
            span_id: parts[2].to_ascii_lowercase(),
            span_name: "gctx.FromTraceparent".to_owned(),
            values: HashMap::new(),
        })
    }
}

/// Creates a traced context, like `gctx.New`.
pub fn new() -> Context {
    Context::new()
}

/// Ensures `context` carries a trace id, like `WithSpan`.
pub fn with_span(context: &Context, span_name: &str) -> Context {
    context.with_span(span_name)
}

/// Returns the trace id, like `CtxId`.
pub fn ctx_id(context: &Context) -> &str {
    context.trace_id()
}

fn init_state() -> &'static Mutex<Context> {
    static INIT: OnceLock<Mutex<Context>> = OnceLock::new();
    INIT.get_or_init(|| {
        let context = std::env::var(TRACEPARENT_HEADER)
            .ok()
            .and_then(|header| Context::from_traceparent(&header))
            .unwrap_or_default();
        Mutex::new(context)
    })
}

/// Replaces the initialization context, like `SetInitCtx`.
pub fn set_init_ctx(context: Context) {
    *init_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = context;
}

/// Returns the initialization context, like `GetInitCtx`.
pub fn get_init_ctx() -> Context {
    init_state()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

fn new_trace_id() -> String {
    hex(&grand::b(16))
}

fn new_span_id() -> String {
    hex(&grand::b(8))
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn valid_hex(text: &str, length: usize) -> bool {
    text.len() == length && text.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_context_has_trace_and_span_ids() {
        let context = new();
        assert_eq!(context.trace_id().len(), 32);
        assert!(context.trace_id().chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(context.span_id().len(), 16);
        assert_eq!(context.span_name(), "gctx.New");
        assert!(!ctx_id(&context).is_empty());

        let same = with_span(&context, "child");
        assert_eq!(same.trace_id(), context.trace_id(), "ids are not replaced");
        assert_eq!(same.span_name(), "gctx.New");
    }

    #[test]
    fn with_span_and_values_on_background() {
        let background = Context::background();
        assert!(background.trace_id().is_empty());
        let context = with_span(&background, "test").with_value("TEST", Value::Int(1));
        assert!(!context.trace_id().is_empty());
        assert_eq!(context.span_name(), "test");
        assert_eq!(context.value("TEST"), Some(&Value::Int(1)));

        let default_name = with_span(&background, "");
        assert_eq!(default_name.span_name(), "gctx.WithSpan");
    }

    #[test]
    fn init_context_and_traceparent() {
        let context = Context::new().with_value("TEST", Value::Int(2));
        set_init_ctx(context.clone());
        assert_eq!(get_init_ctx().trace_id(), context.trace_id());
        assert_eq!(
            get_init_ctx().value("TEST"),
            Some(&Value::Int(2)),
            "the init context keeps its values"
        );

        let header = context.traceparent();
        let parsed = Context::from_traceparent(&header).expect("traceparent");
        assert_eq!(parsed.trace_id(), context.trace_id());
        assert_eq!(parsed.span_id(), context.span_id());
        assert!(Context::from_traceparent("garbage").is_none());
        assert!(Context::from_traceparent("00-xyz-abc-01").is_none());
    }
}
