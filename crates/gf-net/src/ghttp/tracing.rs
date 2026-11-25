//! A lightweight server tracing middleware for `ghttp`, following
//! `ghttp_middleware_tracing.go` without the OpenTelemetry SDK (ADR-105).
//!
//! [`middleware_tracing`] extracts the W3C `traceparent` header, starts a
//! server span (new span id, inherited trace id/parent span) or creates a new
//! trace, and stores it on [`Context`]. [`traceparent_header`] formats a span
//! back for outbound propagation.

use gf_errors::gerror::Error;
use gf_util::grand;

use super::router::Context;

/// The W3C trace context header name.
pub const TRACEPARENT_HEADER: &str = "traceparent";

/// One server span, the port's `context.Context` tracing payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSpan {
    /// The 32-hex-digit trace id.
    pub trace_id: String,
    /// The 16-hex-digit span id of this request.
    pub span_id: String,
    /// The caller's span id when a `traceparent` was supplied.
    pub parent_span_id: Option<String>,
    /// The sampled flag from the incoming traceparent (default true).
    pub sampled: bool,
}

impl ServerSpan {
    /// Creates a root span with fresh ids.
    pub fn new() -> ServerSpan {
        ServerSpan {
            trace_id: random_hex(16),
            span_id: random_hex(8),
            parent_span_id: None,
            sampled: true,
        }
    }
}

impl Default for ServerSpan {
    fn default() -> ServerSpan {
        ServerSpan::new()
    }
}

/// Parses `version-traceid-spanid-flags`, like the W3C trace context.
pub fn parse_traceparent(value: &str) -> Option<(String, String, bool)> {
    let parts: Vec<&str> = value.trim().split('-').collect();
    if parts.len() != 4 {
        return None;
    }
    let version = parts[0];
    if version.len() != 2 || !is_hex(version) {
        return None;
    }
    let trace_id = parts[1];
    let span_id = parts[2];
    let flags = parts[3];
    if trace_id.len() != 32
        || span_id.len() != 16
        || flags.len() != 2
        || !is_hex(trace_id)
        || !is_hex(span_id)
        || !is_hex(flags)
    {
        return None;
    }
    if trace_id.chars().all(|ch| ch == '0') || span_id.chars().all(|ch| ch == '0') {
        return None;
    }
    let sampled = u8::from_str_radix(flags, 16)
        .map(|flags| flags & 1 == 1)
        .ok()?;
    Some((trace_id.to_owned(), span_id.to_owned(), sampled))
}

/// Formats a span as a `traceparent` header value.
pub fn traceparent_header(span: &ServerSpan) -> String {
    format!(
        "00-{}-{}-{:02x}",
        span.trace_id,
        span.span_id,
        if span.sampled { 1 } else { 0 }
    )
}

/// The tracing middleware: starts a server span for the request.
pub fn middleware_tracing(context: &mut Context) -> Result<(), Error> {
    if context.span.is_some() {
        return Ok(());
    }
    let incoming = context
        .request
        .headers
        .get(TRACEPARENT_HEADER)
        .and_then(parse_traceparent);
    let span = match incoming {
        Some((trace_id, parent_span_id, sampled)) => ServerSpan {
            trace_id,
            span_id: random_hex(8),
            parent_span_id: Some(parent_span_id),
            sampled,
        },
        None => ServerSpan {
            trace_id: random_hex(16),
            span_id: random_hex(8),
            parent_span_id: None,
            sampled: true,
        },
    };
    context.span = Some(span);
    Ok(())
}

/// Generates `bytes` random bytes as lowercase hex.
fn random_hex(bytes: usize) -> String {
    let mut hex = String::with_capacity(bytes * 2);
    for byte in grand::b(bytes) {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// Reports whether `value` is non-empty lowercase/uppercase hex.
fn is_hex(value: &str) -> bool {
    !value.is_empty() && value.chars().all(|ch| ch.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ghttp::message::{Header, Request};
    use crate::ghttp::router::Handler;

    fn request(headers: &[(&str, &str)]) -> Request {
        let mut request = Request {
            method: "GET".to_owned(),
            path: "/api".to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        };
        let mut parsed = Header::new();
        for (name, value) in headers {
            parsed.set(name, value);
        }
        request.headers = parsed;
        request
    }

    #[test]
    fn parses_valid_traceparent() {
        let (trace_id, span_id, sampled) =
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
                .expect("valid");
        assert_eq!(trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
        assert_eq!(span_id, "00f067aa0ba902b7");
        assert!(sampled);
        assert!(parse_traceparent("01").is_none());
        assert!(
            parse_traceparent("00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7").is_none()
        );
        assert!(parse_traceparent("00-zzz-00f067aa0ba902b7-01").is_none());
        assert!(
            parse_traceparent("00-00000000000000000000000000000000-00f067aa0ba902b7-01").is_none()
        );
    }

    #[test]
    fn middleware_inherits_parent_trace() {
        let incoming = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_owned();
        let handler: Handler = Arc::new(|context: &mut Context| {
            let span = context.span().expect("span");
            assert_eq!(span.trace_id, "4bf92f3577b34da6a3ce929d0e0e4736");
            assert_eq!(span.parent_span_id.as_deref(), Some("00f067aa0ba902b7"));
            assert_ne!(span.span_id, "00f067aa0ba902b7");
            Ok(())
        });
        let mut context = Context::new(
            request(&[("traceparent", &incoming)]),
            vec![Arc::new(middleware_tracing), handler],
        );
        context.proceed().expect("chain");
    }

    #[test]
    fn middleware_creates_a_trace_without_header() {
        let handler: Handler = Arc::new(|context: &mut Context| {
            let span = context.span().expect("span");
            assert_eq!(span.trace_id.len(), 32);
            assert_eq!(span.span_id.len(), 16);
            assert_eq!(span.parent_span_id, None);
            assert!(span.sampled);
            let header = traceparent_header(span);
            assert!(header.starts_with("00-"));
            assert!(header.ends_with("-01"));
            Ok(())
        });
        let mut context = Context::new(request(&[]), vec![Arc::new(middleware_tracing), handler]);
        context.proceed().expect("chain");
    }

    #[test]
    fn middleware_is_idempotent() {
        let handler: Handler = Arc::new(|context: &mut Context| {
            let first = context.span().expect("span").clone();
            middleware_tracing(context).expect("trace twice");
            assert_eq!(context.span().expect("span"), &first);
            Ok(())
        });
        let mut context = Context::new(request(&[]), vec![Arc::new(middleware_tracing), handler]);
        context.proceed().expect("chain");
    }
}
