//! Port of `util/gconv` for the port's dynamic type.
//!
//! Conversions follow GoFrame's rules for scalars, lists and maps. Go's
//! `Bytes` uses platform memory for numbers; the port uses text and documents
//! that difference (ADR-027). Struct scanning goes through serde.

use gf_core::Value;
use gf_core::float;
use gf_json::marshal_value;
use indexmap::IndexMap;

/// A type that can be built from a [`Value`]; invalid input yields the type's
/// zero value, matching GoFrame's error-free conversion functions.
pub trait FromValue: Sized {
    /// Converts `value`, falling back to the zero value.
    fn from_value(value: &Value) -> Self;
}

fn parse_numeric(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    text.parse::<f64>().ok()
}

/// Converts a value to a string, like `gconv.String`.
///
/// Lists and maps are encoded as compact JSON (sorted keys), matching the Go
/// package's fallback.
pub fn string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => float::format_f(*v),
        Value::String(v) => v.clone(),
        Value::Bytes(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        Value::List(_) | Value::Map(_) => marshal_value(value).unwrap_or_default(),
    }
}

/// Converts a value to a bool, like `gconv.Bool`.
pub fn bool_value(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Int(v) => *v != 0,
        Value::UInt(v) => *v != 0,
        Value::Float(v) => *v != 0.0 && !v.is_nan(),
        Value::String(text) => match text.trim().to_lowercase().as_str() {
            "1" | "t" | "true" | "yes" | "y" | "on" => true,
            "0" | "f" | "false" | "no" | "n" | "off" | "" => false,
            other => parse_numeric(other).is_some_and(|number| number != 0.0 && !number.is_nan()),
        },
        Value::Bytes(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            bool_value(&Value::String(text.into_owned()))
        }
        Value::List(items) => !items.is_empty(),
        Value::Map(entries) => !entries.is_empty(),
    }
}

/// Converts a value to an i64, like `gconv.Int64`.
pub fn int64_value(value: &Value) -> i64 {
    match value {
        Value::Null => 0,
        Value::Bool(v) => i64::from(*v),
        Value::Int(v) => *v,
        Value::UInt(v) => *v as i64,
        Value::Float(v) => float_to_int(*v),
        Value::String(text) => string_to_int(text),
        Value::Bytes(bytes) => {
            let text = String::from_utf8_lossy(bytes);
            string_to_int(&text)
        }
        Value::List(_) | Value::Map(_) => 0,
    }
}

fn string_to_int(text: &str) -> i64 {
    let text = text.trim();
    if text.is_empty() {
        return 0;
    }
    let (sign, digits) = match text.strip_prefix('-') {
        Some(rest) => (-1i64, rest),
        None => (1i64, text.strip_prefix('+').unwrap_or(text)),
    };
    if let Some(hex) = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
    {
        if let Ok(value) = i64::from_str_radix(hex, 16) {
            return value * sign;
        }
        return 0;
    }
    if let Ok(value) = digits.parse::<i64>() {
        return value * sign;
    }
    match parse_numeric(text) {
        Some(number) if number.is_finite() => float_to_int(number),
        _ => 0,
    }
}

fn float_to_int(value: f64) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    value.trunc() as i64
}

/// Converts a value to a u64, like `gconv.Uint64`.
pub fn uint64_value(value: &Value) -> u64 {
    match value {
        Value::Null => 0,
        Value::Bool(v) => u64::from(*v),
        Value::Int(v) => *v as u64,
        Value::UInt(v) => *v,
        Value::Float(v) => {
            if !v.is_finite() {
                0
            } else {
                v.trunc() as u64
            }
        }
        Value::String(text) => int64_value(&Value::String(text.clone())) as u64,
        Value::Bytes(bytes) => {
            let text = String::from_utf8_lossy(bytes).into_owned();
            int64_value(&Value::String(text)) as u64
        }
        Value::List(_) | Value::Map(_) => 0,
    }
}

/// Converts a value to an f64, like `gconv.Float64`.
pub fn float64_value(value: &Value) -> f64 {
    match value {
        Value::Null => 0.0,
        Value::Bool(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        Value::Int(v) => *v as f64,
        Value::UInt(v) => *v as f64,
        Value::Float(v) => *v,
        Value::String(text) => parse_numeric(text).unwrap_or(0.0),
        Value::Bytes(bytes) => parse_numeric(&String::from_utf8_lossy(bytes)).unwrap_or(0.0),
        Value::List(_) | Value::Map(_) => 0.0,
    }
}

/// Converts a value to bytes, like `gconv.Bytes` for textual inputs.
///
/// Numbers use their decimal text (Go uses the platform memory layout, which is
/// not reproducible portably); lists of small integers become their byte
/// values; maps become compact JSON.
pub fn bytes(value: &Value) -> Vec<u8> {
    match value {
        Value::Null => Vec::new(),
        Value::Bool(v) => vec![u8::from(*v)],
        Value::Bytes(bytes) => bytes.clone(),
        Value::String(text) => text.clone().into_bytes(),
        Value::Int(_) | Value::UInt(_) | Value::Float(_) => string(value).into_bytes(),
        Value::List(items) => {
            if !items.is_empty()
                && items
                    .iter()
                    .all(|item| matches!(item, Value::Int(number) if (0..=255).contains(number)))
            {
                items
                    .iter()
                    .map(|item| match item {
                        Value::Int(number) => *number as u8,
                        _ => 0,
                    })
                    .collect()
            } else {
                string(value).into_bytes()
            }
        }
        Value::Map(_) => string(value).into_bytes(),
    }
}

/// Converts a value to a string slice. Map values are converted in order.
pub fn strings(value: &Value) -> Vec<String> {
    match value {
        Value::Null => Vec::new(),
        Value::List(items) => items.iter().map(string).collect(),
        Value::Map(entries) => entries.values().map(string).collect(),
        other => vec![string(other)],
    }
}

/// Converts a value to an integer slice. Map values are converted in order.
pub fn ints(value: &Value) -> Vec<i64> {
    match value {
        Value::Null => Vec::new(),
        Value::List(items) => items.iter().map(int64_value).collect(),
        Value::Map(entries) => entries.values().map(int64_value).collect(),
        Value::String(text) => parse_string_list(text)
            .map(|items| items.iter().map(int64_value).collect())
            .unwrap_or_else(|| vec![int64_value(value)]),
        other => vec![int64_value(other)],
    }
}

/// Converts a value to a float slice. Map values are converted in order.
pub fn floats(value: &Value) -> Vec<f64> {
    match value {
        Value::Null => Vec::new(),
        Value::List(items) => items.iter().map(float64_value).collect(),
        Value::Map(entries) => entries.values().map(float64_value).collect(),
        other => vec![float64_value(other)],
    }
}

/// Converts a value to a bool slice. Map values are converted in order.
pub fn bools(value: &Value) -> Vec<bool> {
    match value {
        Value::Null => Vec::new(),
        Value::List(items) => items.iter().map(bool_value).collect(),
        Value::Map(entries) => entries.values().map(bool_value).collect(),
        Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.starts_with('[') {
                match serde_json::from_str::<Vec<Value>>(trimmed) {
                    Ok(items) => return items.iter().map(bool_value).collect(),
                    Err(_) => return Vec::new(),
                }
            }
            if trimmed.is_empty() {
                return Vec::new();
            }
            vec![bool_value(value)]
        }
        other => vec![bool_value(other)],
    }
}

fn parse_string_list(text: &str) -> Option<Vec<Value>> {
    let trimmed = text.trim();
    if !trimmed.starts_with('[') {
        return None;
    }
    serde_json::from_str::<Vec<Value>>(trimmed).ok()
}

/// Converts a value to an ordered map, like `gconv.Map`.
pub fn map(value: &Value) -> IndexMap<String, Value> {
    match value {
        Value::Map(entries) => entries.clone(),
        Value::String(text) => serde_json::from_str::<IndexMap<String, Value>>(text.trim())
            .ok()
            .unwrap_or_default(),
        _ => IndexMap::new(),
    }
}

/// Converts a value to a list of ordered maps, like `gconv.Maps`.
pub fn maps(value: &Value) -> Vec<IndexMap<String, Value>> {
    match value {
        Value::List(items) => items
            .iter()
            .map(map)
            .filter(|entry| !entry.is_empty())
            .collect(),
        Value::Map(entries) => vec![entries.clone()],
        _ => Vec::new(),
    }
}

/// Converts a value to runes.
pub fn runes(value: &Value) -> Vec<char> {
    string(value).chars().collect()
}

/// Scans a value into a Rust struct through serde.
pub fn scan<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, gf_errors::gerror::Error> {
    let text = marshal_value(value)?;
    serde_json::from_str(&text).map_err(|err| {
        match gf_errors::gerror::wrap_std(Some(Box::new(err)), "json.Unmarshal failed") {
            Some(err) => err,
            None => gf_errors::gerror::Error::new("json.Unmarshal failed"),
        }
    })
}

macro_rules! impl_from_value_int {
    ($($type:ty),* $(,)?) => {
        $(impl FromValue for $type {
            fn from_value(value: &Value) -> Self {
                int64_value(value) as $type
            }
        })*
    };
}

impl_from_value_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl FromValue for f32 {
    fn from_value(value: &Value) -> Self {
        float64_value(value) as f32
    }
}

impl FromValue for f64 {
    fn from_value(value: &Value) -> Self {
        float64_value(value)
    }
}

impl FromValue for bool {
    fn from_value(value: &Value) -> Self {
        bool_value(value)
    }
}

impl FromValue for String {
    fn from_value(value: &Value) -> Self {
        string(value)
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &Value) -> Self {
        bytes(value)
    }
}

impl FromValue for Value {
    fn from_value(value: &Value) -> Self {
        value.clone()
    }
}

impl FromValue for Vec<String> {
    fn from_value(value: &Value) -> Self {
        strings(value)
    }
}

impl FromValue for Vec<i64> {
    fn from_value(value: &Value) -> Self {
        ints(value)
    }
}

impl FromValue for Vec<bool> {
    fn from_value(value: &Value) -> Self {
        bools(value)
    }
}

impl FromValue for Vec<f64> {
    fn from_value(value: &Value) -> Self {
        floats(value)
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &Value) -> Self {
        match value {
            Value::Null => None,
            other => Some(T::from_value(other)),
        }
    }
}

/// Converts `value` into `T` using the port's conversion rules.
pub fn convert<T: FromValue>(value: &Value) -> T {
    T::from_value(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(items: &[Value]) -> Value {
        Value::List(items.to_vec())
    }

    #[test]
    fn string_matches_go() {
        assert_eq!(string(&Value::Null), "");
        assert_eq!(string(&Value::Bool(true)), "true");
        assert_eq!(string(&Value::Int(123)), "123");
        assert_eq!(string(&Value::UInt(123)), "123");
        assert_eq!(string(&Value::Float(123.456)), "123.456");
        // Go's `gconv.String` formats floats with `'f'`, so integers lose the
        // fraction and extreme values keep their full digit expansion.
        assert_eq!(string(&Value::Float(123.0)), "123");
        assert_eq!(string(&Value::Float(1e-7)), "0.0000001");
        assert_eq!(string(&Value::Float(1e300)).len(), 301);
        assert_eq!(string(&Value::Float(f64::NAN)), "NaN");
        assert_eq!(string(&Value::Float(f64::INFINITY)), "+Inf");
        assert_eq!(string(&Value::Float(f64::NEG_INFINITY)), "-Inf");
        assert_eq!(string(&Value::String("Neptune".into())), "Neptune");
        assert_eq!(string(&Value::Bytes(b"abc".to_vec())), "abc");
        assert_eq!(
            string(&list(&[Value::Int(1), Value::Int(2), Value::Int(3)])),
            "[1,2,3]"
        );
        let mut map = IndexMap::new();
        map.insert("Earth".to_owned(), Value::String("太平洋".into()));
        assert_eq!(string(&Value::Map(map)), r#"{"Earth":"太平洋"}"#);
    }

    #[test]
    fn bool_matches_go() {
        for (value, expected) in [
            (Value::Bool(true), true),
            (Value::Int(1), true),
            (Value::Int(0), false),
            (Value::String("1".into()), true),
            (Value::String("0".into()), false),
            (Value::String("true".into()), true),
            (Value::String("false".into()), false),
            (Value::String("on".into()), true),
            (Value::String("off".into()), false),
            (Value::String("t".into()), true),
            (Value::String("F".into()), false),
            (Value::String("yes".into()), true),
            (Value::String("no".into()), false),
            (Value::String("123.456".into()), true),
            (Value::String("".into()), false),
            (Value::Null, false),
            (list(&[]), false),
            (list(&[Value::Int(1)]), true),
        ] {
            assert_eq!(bool_value(&value), expected, "{value:?}");
        }
    }

    #[test]
    fn int_matches_go() {
        for (value, expected) in [
            (Value::Bool(true), 1),
            (Value::Float(123.456), 123),
            (Value::String("123.456".into()), 123),
            (Value::String("-123.456".into()), -123),
            (Value::String("+1".into()), 1),
            (Value::String("0xA".into()), 10),
            (Value::String("-0xA".into()), -10),
            (Value::String("true".into()), 0),
            (Value::String("NaN".into()), 0),
            (Value::UInt(123), 123),
            (Value::Null, 0),
        ] {
            assert_eq!(int64_value(&value), expected, "{value:?}");
        }
        assert_eq!(uint64_value(&Value::String("123".into())), 123);
        assert_eq!(float64_value(&Value::String("123.456".into())), 123.456);
        assert_eq!(float64_value(&Value::Bool(true)), 1.0);
    }

    #[test]
    fn bytes_match_the_port_rules() {
        assert_eq!(bytes(&Value::Null), Vec::<u8>::new());
        assert_eq!(bytes(&Value::Bool(true)), vec![1]);
        assert_eq!(bytes(&Value::Bool(false)), vec![0]);
        assert_eq!(bytes(&Value::String("Uranus".into())), b"Uranus");
        assert_eq!(
            bytes(&list(&[Value::Int(1), Value::Int(2), Value::Int(3)])),
            vec![1, 2, 3]
        );
        assert_eq!(
            bytes(&list(&[
                Value::String("a".into()),
                Value::String("b".into())
            ])),
            br#"["a","b"]"#
        );
    }

    #[test]
    fn slices_and_maps() {
        assert_eq!(
            strings(&list(&[Value::Int(1), Value::Bool(true)])),
            vec!["1".to_owned(), "true".to_owned()]
        );
        assert_eq!(strings(&Value::Null), Vec::<String>::new());
        assert_eq!(ints(&Value::String("[1,2,3]".into())), vec![1, 2, 3]);
        assert_eq!(
            bools(&Value::String("[true,false]".into())),
            vec![true, false]
        );
        assert_eq!(bools(&Value::String("".into())), Vec::<bool>::new());
        let mut map = IndexMap::new();
        map.insert("a".to_owned(), Value::Int(1));
        assert_eq!(map_value(&Value::Map(map.clone())), map);
        assert!(maps(&Value::List(vec![Value::Map(map.clone())])).len() == 1);
    }

    fn map_value(value: &Value) -> IndexMap<String, Value> {
        map(value)
    }

    #[test]
    fn typed_convert_and_scan() {
        assert_eq!(convert::<i32>(&Value::String("42".into())), 42);
        assert!(convert::<bool>(&Value::String("on".into())));
        assert_eq!(convert::<String>(&Value::Int(7)), "7");
        assert_eq!(
            convert::<Vec<i64>>(&list(&[Value::Int(1), Value::Int(2)])),
            vec![1, 2]
        );

        #[derive(serde::Deserialize)]
        struct Config {
            name: String,
            count: i64,
        }
        let mut value = IndexMap::new();
        value.insert("name".to_owned(), Value::String("gf".into()));
        value.insert("count".to_owned(), Value::Int(2));
        let config: Config = scan(&Value::Map(value)).expect("scan");
        assert_eq!(config.name, "gf");
        assert_eq!(config.count, 2);
    }
}
