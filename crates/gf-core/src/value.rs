//! The dynamic value type that stands in for Go's `any`.
//!
//! GoFrame containers (`gvar`, `gmap`, `garray`, ...) and the conversion layer
//! (`gconv`) hand `any` values around, preserving the dynamic type through
//! copies. A Rust generic cannot do that once a map holds mixed values, so the
//! port stores one [`Value`] enum instead.
//!
//! Maps keep insertion order (Go's raw `map` does not, but most of the
//! framework's observable output sorts keys or uses ordered structures).

use std::fmt;

use indexmap::IndexMap;

/// A GoFrame dynamic value.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// `nil`.
    Null,
    /// `bool`.
    Bool(bool),
    /// `int` and the signed integer types.
    Int(i64),
    /// `uint` and the unsigned integer types.
    UInt(u64),
    /// `float32` / `float64`.
    Float(f64),
    /// `string`.
    String(String),
    /// `[]byte`.
    Bytes(Vec<u8>),
    /// `[]T` / `[]any`.
    List(Vec<Value>),
    /// `map[string]T` / `map[string]any`.
    Map(IndexMap<String, Value>),
}

impl Value {
    /// Returns whether the value is `nil`.
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Null)
    }

    /// Returns the Go type name, matching `reflect.TypeOf(v).String()` for the
    /// representations this enum covers.
    pub fn go_type_name(&self) -> &'static str {
        match self {
            Value::Null => "nil",
            Value::Bool(_) => "bool",
            Value::Int(_) => "int",
            Value::UInt(_) => "uint",
            Value::Float(_) => "float64",
            Value::String(_) => "string",
            Value::Bytes(_) => "[]uint8",
            Value::List(_) => "[]interface {}",
            Value::Map(_) => "map[string]interface {}",
        }
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Self {
        Value::Bool(v)
    }
}

impl From<i64> for Value {
    fn from(v: i64) -> Self {
        Value::Int(v)
    }
}

impl From<u64> for Value {
    fn from(v: u64) -> Self {
        Value::UInt(v)
    }
}

impl From<f64> for Value {
    fn from(v: f64) -> Self {
        Value::Float(v)
    }
}

impl From<String> for Value {
    fn from(v: String) -> Self {
        Value::String(v)
    }
}

impl From<&str> for Value {
    fn from(v: &str) -> Self {
        Value::String(v.to_owned())
    }
}

impl From<Vec<u8>> for Value {
    fn from(v: Vec<u8>) -> Self {
        Value::Bytes(v)
    }
}

/// Formats like Go's `%v` for the same dynamic values.
///
/// Floats go through [`crate::float::format_g`], which is the `'g'` shape
/// `fmt` uses, so exponents match Go for very large and very small numbers.
impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Null => f.write_str("<nil>"),
            Value::Bool(v) => write!(f, "{v}"),
            Value::Int(v) => write!(f, "{v}"),
            Value::UInt(v) => write!(f, "{v}"),
            Value::Float(v) => f.write_str(&crate::float::format_g(*v)),
            Value::String(v) => f.write_str(v),
            Value::Bytes(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::List(items) => {
                f.write_str("[")?;
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{item}")?;
                }
                f.write_str("]")
            }
            Value::Map(entries) => {
                let mut keys: Vec<&String> = entries.keys().collect();
                keys.sort();
                f.write_str("map[")?;
                for (i, key) in keys.into_iter().enumerate() {
                    if i > 0 {
                        f.write_str(" ")?;
                    }
                    write!(f, "{key}:{}", entries[key])?;
                }
                f.write_str("]")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Value;
    use indexmap::IndexMap;

    #[test]
    fn type_names_match_go() {
        assert_eq!(Value::Null.go_type_name(), "nil");
        assert_eq!(Value::Bool(true).go_type_name(), "bool");
        assert_eq!(Value::Int(1).go_type_name(), "int");
        assert_eq!(Value::UInt(1).go_type_name(), "uint");
        assert_eq!(Value::Float(1.0).go_type_name(), "float64");
        assert_eq!(Value::String(String::new()).go_type_name(), "string");
        assert_eq!(Value::Bytes(Vec::new()).go_type_name(), "[]uint8");
        assert_eq!(Value::List(Vec::new()).go_type_name(), "[]interface {}");
        assert_eq!(
            Value::Map(IndexMap::new()).go_type_name(),
            "map[string]interface {}"
        );
    }

    #[test]
    fn display_matches_go_verb() {
        assert_eq!(Value::Null.to_string(), "<nil>");
        assert_eq!(Value::Bool(false).to_string(), "false");
        assert_eq!(Value::Int(-7).to_string(), "-7");
        assert_eq!(Value::Float(1.5).to_string(), "1.5");
        // `fmt` uses the `'g'` shape for floats, not the flat expansion.
        assert_eq!(Value::Float(1e21).to_string(), "1e+21");
        assert_eq!(Value::Float(1e-7).to_string(), "1e-07");
        assert_eq!(Value::String("abc".into()).to_string(), "abc");
        assert_eq!(Value::Bytes(vec![104, 105]).to_string(), "[104 105]");
        assert_eq!(
            Value::List(vec![Value::Int(1), Value::String("a".into())]).to_string(),
            "[1 a]"
        );
        let mut map = IndexMap::new();
        map.insert("b".to_owned(), Value::Int(2));
        map.insert("a".to_owned(), Value::Int(1));
        assert_eq!(Value::Map(map).to_string(), "map[a:1 b:2]");
    }

    #[test]
    fn nil_only_for_null() {
        assert!(Value::Null.is_nil());
        assert!(!Value::String(String::new()).is_nil());
    }
}
