//! Port of `net/gtrace` — the context-free parts.
//!
//! GoFrame's `gtrace` wraps OpenTelemetry: trace/span ids live in a
//! `context.Context`, spans are created by a global `TracerProvider`, and
//! baggage is stored through OTel's baggage API. This port has no context
//! (ADR-034) and no OTel dependency, so it keeps the pieces that do not need
//! either (ADR-059):
//!
//! - [`Carrier`], the text-map carrier used for cross-peer propagation;
//! - [`TraceContext`], an explicit trace/span holder replacing the context
//!   values, with GoFrame's 32-hex trace-id validation (`with_trace_id`,
//!   `with_uuid`, `get_trace_id`, `get_span_id`);
//! - [`Baggage`], an explicit key-value holder replacing OTel baggage;
//! - `SafeContent`/`SafeContentForHttp` and the tracing configuration getters.
//!
//! Not ported: span creation/ending, the default `TracerProvider`, the
//! composite text-map propagator and the HTTP client/server instrumentation —
//! all of them are defined in terms of `context.Context` and OTel interfaces.

use std::sync::OnceLock;

use gf_container::gvar::Var;
use gf_core::Value;
use gf_encoding::gcompress;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_util::gconv;
use indexmap::IndexMap;

use crate::gipv4;

/// The environment key for the maximum tracing content size, like Go's
/// `commandEnvKeyForMaxContentLogSize`.
pub const ENV_KEY_MAX_CONTENT_LOG_SIZE: &str = "gf.gtrace.max.content.log.size";

/// The environment key for internal tracing, like Go's
/// `commandEnvKeyForTracingInternal`.
pub const ENV_KEY_TRACING_INTERNAL: &str = "gf.gtrace.tracing.internal";

/// The default maximum content log size (512 KiB), like Go's default.
pub const DEFAULT_MAX_CONTENT_LOG_SIZE: usize = 512 * 1024;

/// A text-map carrier, like Go's `Carrier`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Carrier {
    values: IndexMap<String, Value>,
}

impl Carrier {
    /// Creates a carrier, like `NewCarrier`.
    pub fn new(data: Option<IndexMap<String, Value>>) -> Carrier {
        Carrier {
            values: data.unwrap_or_default(),
        }
    }

    /// Returns the string value of `key`, like `Get`.
    pub fn get(&self, key: &str) -> String {
        self.values.get(key).map(gconv::string).unwrap_or_default()
    }

    /// Stores a string value, like `Set`.
    pub fn set(&mut self, key: &str, value: &str) {
        self.values
            .insert(key.to_owned(), Value::String(value.to_owned()));
    }

    /// Lists the keys, like `Keys`.
    pub fn keys(&self) -> Vec<String> {
        self.values.keys().cloned().collect()
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether the carrier is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Returns the underlying map.
    pub fn values(&self) -> &IndexMap<String, Value> {
        &self.values
    }

    /// JSON-encodes the carrier, panicking on error, like `MustMarshal`.
    pub fn must_marshal(&self) -> Vec<u8> {
        gf_json::marshal_value(&Value::Map(self.values.clone()))
            .unwrap_or_else(|error| panic!("{error}"))
            .into_bytes()
    }

    /// JSON-encodes the carrier as a string, like `String`.
    pub fn to_json_string(&self) -> String {
        String::from_utf8_lossy(&self.must_marshal()).into_owned()
    }

    /// Replaces the entries from JSON content, like `UnmarshalJSON`.
    ///
    /// Go's implementation parses into a temporary carrier and silently drops
    /// the result; that bug is not reproduced here.
    pub fn unmarshal_json(&mut self, data: &[u8]) -> Result<(), Error> {
        match gf_json::unmarshal_value(data)? {
            Value::Map(map) => {
                self.values = map;
                Ok(())
            }
            _ => Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["carrier JSON must be an object"],
            )),
        }
    }
}

impl std::fmt::Display for Carrier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_json_string())
    }
}

/// The explicit trace/span holder that replaces the context values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TraceContext {
    trace_id: Option<String>,
    span_id: Option<String>,
}

impl TraceContext {
    /// Creates an empty trace context (like `context.Background()`).
    pub fn new() -> TraceContext {
        TraceContext::default()
    }

    /// The trace id, or an empty string when tracing is not activated, like
    /// `GetTraceID`.
    pub fn trace_id(&self) -> String {
        match &self.trace_id {
            Some(trace_id) if is_valid_trace_id(trace_id) => trace_id.clone(),
            _ => String::new(),
        }
    }

    /// The span id, or an empty string when there is none, like `GetSpanID`.
    pub fn span_id(&self) -> String {
        self.span_id.clone().unwrap_or_default()
    }

    /// Reports whether a valid trace id is set.
    pub fn has_trace_id(&self) -> bool {
        !self.trace_id().is_empty()
    }
}

/// Reports whether `trace_id` is a valid, non-zero lowercase hex trace id.
fn is_valid_trace_id(trace_id: &str) -> bool {
    trace_id.len() == 32
        && trace_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
        && trace_id.chars().any(|character| character != '0')
}

/// Sets a custom trace id, like `WithTraceID`.
///
/// Go creates a span first when the context has no trace id; there is no span
/// machinery here, so the id is always taken from `trace_id` after validation.
pub fn with_trace_id(context: &mut TraceContext, trace_id: &str) -> Result<(), Error> {
    if trace_id.len() != 32
        || !trace_id
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "invalid custom traceID \"{trace_id}\", a traceID string should be composed with [0-f] and fixed length 32"
            ),
            Vec::new(),
        ));
    }
    context.trace_id = Some(trace_id.to_ascii_lowercase());
    Ok(())
}

/// Sets a custom trace id from a UUID (dashes removed), like `WithUUID`.
pub fn with_uuid(context: &mut TraceContext, uuid: &str) -> Result<(), Error> {
    with_trace_id(context, &uuid.replace('-', ""))
}

/// Returns the trace id of `context`, like `GetTraceID`.
pub fn get_trace_id(context: &TraceContext) -> String {
    context.trace_id()
}

/// Returns the span id of `context`, like `GetSpanID`.
pub fn get_span_id(context: &TraceContext) -> String {
    context.span_id()
}

/// Explicit baggage values, replacing OTel baggage in the context.
#[derive(Clone, Debug, Default)]
pub struct Baggage {
    values: IndexMap<String, Value>,
}

impl Baggage {
    /// Creates an empty baggage, like `NewBaggage`.
    pub fn new() -> Baggage {
        Baggage::default()
    }

    /// Adds one value, like `SetValue`.
    pub fn set_value(&mut self, key: &str, value: Value) -> &mut Baggage {
        self.values.insert(key.to_owned(), value);
        self
    }

    /// Adds a map of values, like `SetMap`.
    pub fn set_map(&mut self, data: IndexMap<String, Value>) -> &mut Baggage {
        for (key, value) in data {
            self.values.insert(key, value);
        }
        self
    }

    /// Returns all values, like `GetMap`.
    pub fn get_map(&self) -> IndexMap<String, Value> {
        self.values.clone()
    }

    /// Returns one value as a `Var`, like `GetVar`.
    pub fn get_var(&self, key: &str) -> Var {
        Var::new(self.values.get(key).cloned().unwrap_or(Value::Null))
    }
}

/// Reports whether tracing spans of internal components are enabled, like
/// `IsTracingInternal`.
pub fn is_tracing_internal() -> bool {
    static VALUE: OnceLock<bool> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var(ENV_KEY_TRACING_INTERNAL)
            .map(|value| gconv::bool_value(&Value::String(value)))
            .unwrap_or(true)
    })
}

/// The maximum request/response content size kept in traces, like
/// `MaxContentLogSize`.
pub fn max_content_log_size() -> usize {
    static VALUE: OnceLock<usize> = OnceLock::new();
    *VALUE.get_or_init(|| {
        std::env::var(ENV_KEY_MAX_CONTENT_LOG_SIZE)
            .ok()
            .and_then(|value| value.trim().parse::<usize>().ok())
            .filter(|size| *size > 0)
            .unwrap_or(DEFAULT_MAX_CONTENT_LOG_SIZE)
    })
}

/// Common tracing labels: `hostname`, `ip.intranet` and `host.name`, like
/// `CommonLabels`.
pub fn common_labels() -> Vec<(String, String)> {
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let intranet = gipv4::get_intranet_ip_array().unwrap_or_default();
    vec![
        ("hostname".to_owned(), hostname.clone()),
        ("ip.intranet".to_owned(), intranet.join(",")),
        ("host.name".to_owned(), hostname),
    ]
}

/// Reports whether the default trace provider is in use, like
/// `IsUsingDefaultProvider`.
///
/// There is only the default (no-op) provider in this port.
pub fn is_using_default_provider() -> bool {
    true
}

/// Cuts `data` to `max_content_log_size()` runes and appends `...` when it was
/// longer, like `SafeContent`.
pub fn safe_content(data: &[u8]) -> String {
    let content = String::from_utf8_lossy(data).into_owned();
    let limit = max_content_log_size();
    if gf_text::gstr::len_rune(&content) > limit {
        let truncated: String = content.chars().take(limit).collect();
        return format!("{truncated}...");
    }
    content
}

/// Decompresses gzip content when `content_encoding` says so, then cuts it
/// like [`safe_content`], like `SafeContentForHttp`.
pub fn safe_content_for_http(data: &[u8], content_encoding: Option<&str>) -> Result<String, Error> {
    let data = if gzip_accepted(content_encoding) {
        gcompress::un_gzip(data)?
    } else {
        data.to_vec()
    };
    Ok(safe_content(&data))
}

/// Reports whether the content encoding accepts gzip, like `gzipAccepted`.
fn gzip_accepted(content_encoding: Option<&str>) -> bool {
    match content_encoding {
        Some(encoding) => encoding.split(',').any(|part| {
            let part = part.trim();
            part == "gzip" || part.starts_with("gzip;")
        }),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value_string(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    // Translated from TestNewCarrier's carrier half.
    #[test]
    fn carrier_round_trip() {
        let mut carrier = Carrier::new(None);
        assert!(carrier.is_empty());
        carrier.set(
            "traceparent",
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
        );
        assert_eq!(carrier.len(), 1);
        assert_eq!(
            carrier.get("traceparent"),
            "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01"
        );
        assert_eq!(carrier.get("missing"), "");
        assert_eq!(carrier.keys(), vec!["traceparent".to_owned()]);

        let mut restored = Carrier::new(None);
        restored
            .unmarshal_json(&carrier.must_marshal())
            .expect("unmarshal");
        assert_eq!(restored.get("traceparent"), carrier.get("traceparent"));
        assert_eq!(carrier.to_string(), carrier.to_json_string());
        assert!(carrier.to_json_string().starts_with('{'));
    }

    #[test]
    fn carrier_from_existing_map() {
        let mut data = IndexMap::new();
        data.insert("a".to_owned(), value_string("1"));
        data.insert("b".to_owned(), Value::Int(2));
        let carrier = Carrier::new(Some(data));
        assert_eq!(carrier.get("a"), "1");
        assert_eq!(carrier.get("b"), "2");
        assert_eq!(carrier.keys().len(), 2);
        let mut invalid = Carrier::new(None);
        assert!(invalid.unmarshal_json(b"[1,2]").is_err());
    }

    // Translated from TestWithTraceID / TestWithUUID.
    #[test]
    fn trace_id_validation_and_uuids() {
        let mut context = TraceContext::new();
        let uuid = "a323f910-f690-11ec-963d-79c0b7fcf119";
        assert!(
            with_trace_id(&mut context, uuid).is_err(),
            "dashes are invalid"
        );
        assert_eq!(get_trace_id(&context), "");
        assert!(!context.has_trace_id());

        let trace_id = uuid.replace('-', "");
        with_trace_id(&mut context, &trace_id).expect("valid trace id");
        assert_eq!(get_trace_id(&context), trace_id);
        assert_eq!(get_span_id(&context), "");

        let mut context = TraceContext::new();
        with_uuid(&mut context, uuid).expect("uuid");
        assert_eq!(get_trace_id(&context), trace_id);

        // Uppercase hex is normalised to lowercase, like OTel's String().
        let mut context = TraceContext::new();
        with_trace_id(&mut context, &trace_id.to_uppercase()).expect("uppercase");
        assert_eq!(get_trace_id(&context), trace_id);

        // An all-zero id parses but is not valid, so the getter returns "".
        let mut context = TraceContext::new();
        with_trace_id(&mut context, &"0".repeat(32)).expect("zero id");
        assert_eq!(get_trace_id(&context), "");
        assert!(with_trace_id(&mut context, "zzzz").is_err());
    }

    // Translated from TestSafeContent.
    #[test]
    fn safe_content_truncates_by_runes_and_ungzips() {
        let limit = max_content_log_size();
        let short = "中".repeat(limit - 1);
        let standard = "中".repeat(limit);
        let long = "中".repeat(limit + 1);

        assert_eq!(safe_content(short.as_bytes()), short);
        assert_eq!(safe_content(standard.as_bytes()), standard);
        assert_eq!(safe_content(long.as_bytes()), format!("{standard}..."));

        let compressed_short = gcompress::gzip(short.as_bytes(), None).expect("gzip");
        assert_eq!(
            safe_content_for_http(&compressed_short, Some("gzip")).expect("short"),
            short
        );
        let compressed_long = gcompress::gzip(long.as_bytes(), None).expect("gzip");
        assert_eq!(
            safe_content_for_http(&compressed_long, Some("gzip, deflate")).expect("long"),
            format!("{standard}...")
        );
        assert_eq!(
            safe_content_for_http(short.as_bytes(), None).expect("plain"),
            short
        );
        // Not gzip-encoded but marked as gzip: the unzip error surfaces.
        assert!(safe_content_for_http(b"not gzip", Some("gzip")).is_err());
    }

    #[test]
    fn baggage_round_trip() {
        let mut baggage = Baggage::new();
        baggage
            .set_value("traceName", value_string("john"))
            .set_map({
                let mut data = IndexMap::new();
                data.insert("userId".to_owned(), Value::Int(1000));
                data
            });
        let map = baggage.get_map();
        assert_eq!(map.len(), 2);
        assert_eq!(gconv::string(&map["traceName"]), "john");
        assert_eq!(baggage.get_var("userId").int64(), 1000);
        assert_eq!(baggage.get_var("missing").string(), "");
    }

    #[test]
    fn tracing_configuration_and_labels() {
        assert!(is_tracing_internal());
        assert_eq!(max_content_log_size(), DEFAULT_MAX_CONTENT_LOG_SIZE);
        assert!(is_using_default_provider());
        let labels = common_labels();
        assert_eq!(labels[0].0, "hostname");
        assert_eq!(labels[1].0, "ip.intranet");
        assert_eq!(labels[2].0, "host.name");
    }
}
