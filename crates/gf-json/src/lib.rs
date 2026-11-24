//! Port of `internal/json`.
//!
//! Go's `encoding/json` escapes `<`, `>`, `&`, U+2028 and U+2029 inside
//! strings and encodes `[]byte` as BASE64. Both behaviors are reproduced here
//! so the port's JSON matches the original byte for byte.

use std::io;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gf_core::Value;
use gf_errors::gerror;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde::ser::{SerializeMap, SerializeSeq};
use serde_json::ser::{Formatter, PrettyFormatter};

/// A raw encoded JSON value, equivalent to Go's `json.RawMessage`.
pub type RawMessage = Box<serde_json::value::RawValue>;

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

struct GoFormatter;

impl Formatter for GoFormatter {
    fn write_string_fragment<W: ?Sized + io::Write>(
        &mut self,
        writer: &mut W,
        fragment: &str,
    ) -> io::Result<()> {
        write_go_string(writer, fragment)
    }
}

fn write_go_string<W: ?Sized + io::Write>(writer: &mut W, fragment: &str) -> io::Result<()> {
    let bytes = fragment.as_bytes();
    let mut last = 0;
    for (index, ch) in fragment.char_indices() {
        let escaped = match ch {
            '<' => "\\u003c",
            '>' => "\\u003e",
            '&' => "\\u0026",
            '\u{2028}' => "\\u2028",
            '\u{2029}' => "\\u2029",
            _ => continue,
        };
        writer.write_all(&bytes[last..index])?;
        writer.write_all(escaped.as_bytes())?;
        last = index + ch.len_utf8();
    }
    writer.write_all(&bytes[last..])
}

fn to_vec_go<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, serde_json::Error> {
    let mut buffer = Vec::with_capacity(128);
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, GoFormatter);
    value.serialize(&mut serializer)?;
    Ok(buffer)
}

/// A [`Value`] serialized the way Go's `encoding/json` serializes `any`.
struct GoValue<'a>(&'a Value);

impl Serialize for GoValue<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.0 {
            Value::Null => serializer.serialize_unit(),
            Value::Bool(v) => serializer.serialize_bool(*v),
            Value::Int(v) => serializer.serialize_i64(*v),
            Value::UInt(v) => serializer.serialize_u64(*v),
            Value::Float(v) => serializer.serialize_f64(*v),
            Value::String(v) => serializer.serialize_str(v),
            Value::Bytes(v) => serializer.serialize_str(&STANDARD.encode(v)),
            Value::List(items) => {
                let mut sequence = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    sequence.serialize_element(&GoValue(item))?;
                }
                sequence.end()
            }
            Value::Map(entries) => {
                let mut keys: Vec<&String> = entries.keys().collect();
                keys.sort();
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for key in keys {
                    map.serialize_entry(key, &GoValue(&entries[key]))?;
                }
                map.end()
            }
        }
    }
}

/// Encodes `value` as JSON bytes.
pub fn marshal<T: Serialize + ?Sized>(value: &T) -> Result<Vec<u8>, gerror::Error> {
    to_vec_go(value).map_err(|err| wrap(err, "json.Marshal failed"))
}

/// Encodes `value` as JSON text.
pub fn marshal_string<T: Serialize + ?Sized>(value: &T) -> Result<String, gerror::Error> {
    let bytes = marshal(value)?;
    String::from_utf8(bytes).map_err(|err| wrap(err, "json.Marshal failed"))
}

/// Encodes a [`Value`] as JSON text with Go's `[]byte` and map-key behavior.
pub fn marshal_value(value: &Value) -> Result<String, gerror::Error> {
    let bytes = to_vec_go(&GoValue(value)).map_err(|err| wrap(err, "json.Marshal failed"))?;
    String::from_utf8(bytes).map_err(|err| wrap(err, "json.Marshal failed"))
}

/// Encodes `value` as indented JSON bytes, prefixing every line after the first
/// with `prefix`.
pub fn marshal_indent<T: Serialize + ?Sized>(
    value: &T,
    prefix: &str,
    indent: &str,
) -> Result<Vec<u8>, gerror::Error> {
    let formatter = PrettyFormatter::with_indent(indent.as_bytes());
    let mut buffer = Vec::with_capacity(128);
    let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
    value
        .serialize(&mut serializer)
        .map_err(|err| wrap(err, "json.MarshalIndent failed"))?;
    let text = String::from_utf8(buffer).map_err(|err| wrap(err, "json.MarshalIndent failed"))?;
    let escaped = escape_html_json(&text);
    if prefix.is_empty() {
        return Ok(escaped.into_bytes());
    }
    let mut out = String::with_capacity(escaped.len() + prefix.len() * 8);
    for (index, segment) in escaped.split_inclusive('\n').enumerate() {
        if index > 0 {
            out.push_str(prefix);
        }
        out.push_str(segment);
    }
    Ok(out.into_bytes())
}

fn escape_html_json(text: &str) -> String {
    if !text.contains(['<', '>', '&', '\u{2028}', '\u{2029}']) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 8);
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if !in_string {
            if ch == '"' {
                in_string = true;
            }
            out.push(ch);
            continue;
        }
        if escaped {
            escaped = false;
            out.push(ch);
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
                out.push(ch);
            }
            '"' => {
                in_string = false;
                out.push(ch);
            }
            '<' => out.push_str("\\u003c"),
            '>' => out.push_str("\\u003e"),
            '&' => out.push_str("\\u0026"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            _ => out.push(ch),
        }
    }
    out
}

/// Decodes JSON bytes into `T`.
pub fn unmarshal<T: DeserializeOwned>(data: &[u8]) -> Result<T, gerror::Error> {
    serde_json::from_slice(data).map_err(|err| wrap(err, "json.Unmarshal failed"))
}

/// Decodes JSON bytes into a [`Value`].
pub fn unmarshal_value(data: &[u8]) -> Result<Value, gerror::Error> {
    unmarshal(data)
}

/// Decodes JSON bytes without changing number handling; [`Value`] already
/// keeps integers and floats apart, so this matches `UnmarshalUseNumber`.
pub fn unmarshal_use_number<T: DeserializeOwned>(data: &[u8]) -> Result<T, gerror::Error> {
    serde_json::from_slice(data).map_err(|err| wrap(err, "json.UnmarshalUseNumber failed"))
}

/// Reports whether `data` is valid JSON.
pub fn valid(data: &[u8]) -> bool {
    serde_json::from_slice::<serde::de::IgnoredAny>(data).is_ok()
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use serde::Deserialize;

    use super::*;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct Sample {
        name: String,
        count: i64,
        tags: Vec<String>,
    }

    fn value_map(entries: &[(&str, Value)]) -> Value {
        let mut map = IndexMap::new();
        for (key, value) in entries {
            map.insert((*key).to_owned(), value.clone());
        }
        Value::Map(map)
    }

    #[test]
    fn marshal_matches_go_escaping() {
        assert_eq!(
            marshal_string("a<b>&c").expect("marshal"),
            "\"a\\u003cb\\u003e\\u0026c\""
        );
        assert_eq!(
            marshal_string("line\u{2028}sep").expect("marshal"),
            "\"line\\u2028sep\""
        );
        assert_eq!(marshal_string("测试").expect("marshal"), "\"测试\"");
    }

    #[test]
    fn marshal_value_sorts_keys_and_encodes_bytes() {
        let value = value_map(&[("b", Value::Int(1)), ("a", Value::String("<x>".into()))]);
        assert_eq!(
            marshal_value(&value).expect("marshal"),
            r#"{"a":"\u003cx\u003e","b":1}"#
        );

        assert_eq!(
            marshal_value(&Value::Bytes(vec![1, 2, 3])).expect("marshal"),
            "\"AQID\""
        );

        let nested = value_map(&[(
            "list",
            Value::List(vec![Value::Null, Value::Bool(true), Value::Float(1.5)]),
        )]);
        assert_eq!(
            marshal_value(&nested).expect("marshal"),
            r#"{"list":[null,true,1.5]}"#
        );
    }

    #[test]
    fn marshal_indent_uses_prefix_and_indent() {
        let value = value_map(&[("a", Value::Int(1))]);
        let text = String::from_utf8(marshal_indent(&GoValue(&value), ">>", "  ").expect("indent"))
            .expect("utf8");
        assert_eq!(text, "{\n>>  \"a\": 1\n>>}");
    }

    #[test]
    fn unmarshal_reads_into_values_and_structs() {
        let value =
            unmarshal_value(br#"{"n":-7,"u":18446744073709551615,"f":2.5}"#).expect("unmarshal");
        let Value::Map(map) = value else {
            panic!("expected map");
        };
        assert_eq!(map["n"], Value::Int(-7));
        assert_eq!(map["u"], Value::UInt(u64::MAX));
        assert_eq!(map["f"], Value::Float(2.5));

        let sample: Sample =
            unmarshal(br#"{"name":"n","count":2,"tags":["a"]}"#).expect("unmarshal");
        assert_eq!(
            sample,
            Sample {
                name: "n".to_owned(),
                count: 2,
                tags: vec!["a".to_owned()],
            }
        );

        assert!(unmarshal::<Sample>(b"{").is_err());
        assert!(unmarshal_use_number::<Sample>(b"{").is_err());
    }

    #[test]
    fn round_trip_and_valid() {
        let value = value_map(&[
            ("s", Value::String("测试 <ok>".into())),
            ("l", Value::List(vec![Value::Int(1), Value::UInt(2)])),
        ]);
        let text = marshal_value(&value).expect("marshal");
        let back = unmarshal_value(text.as_bytes()).expect("unmarshal");
        assert_eq!(marshal_value(&back).expect("marshal"), text);

        let Value::Map(map) = &back else {
            panic!("expected map");
        };
        assert_eq!(map["s"], Value::String("测试 <ok>".into()));
        assert_eq!(map["l"], Value::List(vec![Value::UInt(1), Value::UInt(2)]));

        assert!(valid(br#"{"a":[1,2,3]}"#));
        assert!(!valid(b"{\"a\":}"));
    }

    #[test]
    fn raw_message_preserves_its_text() {
        let raw: RawMessage = serde_json::from_str(r#"{"b": 1, "a": 2}"#).expect("raw");
        assert_eq!(raw.get(), r#"{"b": 1, "a": 2}"#);
    }
}
