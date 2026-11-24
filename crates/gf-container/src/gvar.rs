//! Port of `container/gvar`.
//!
//! `Var` holds one [`Value`] under a lock; Go's `safe` flag is accepted and
//! ignored because the port is always synchronized (ADR-007).

use std::sync::RwLock;

use gf_core::Value;
use gf_errors::gerror;
use gf_util::gconv;
use indexmap::IndexMap;

/// A locked dynamic value.
pub struct Var {
    value: RwLock<Value>,
}

impl Var {
    /// Creates a variable holding `value`.
    pub fn new(value: Value) -> Var {
        Var {
            value: RwLock::new(value),
        }
    }

    /// Creates a variable; the Go `safe` flag is accepted and ignored.
    pub fn new_safe(value: Value, _safe: bool) -> Var {
        Var::new(value)
    }

    /// Replaces the held value.
    pub fn set(&self, value: Value) {
        let mut guard = self
            .value
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = value;
    }

    /// Returns a copy of the held value.
    pub fn val(&self) -> Value {
        self.value
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Reports whether the value is nil.
    pub fn is_nil(&self) -> bool {
        self.val().is_nil()
    }

    /// Reports whether the value is empty.
    pub fn is_empty(&self) -> bool {
        gf_internal::empty::is_empty(&self.val())
    }

    /// Returns the value as a string.
    pub fn string(&self) -> String {
        gconv::string(&self.val())
    }

    /// Returns the value as an i64.
    pub fn int64(&self) -> i64 {
        gconv::int64_value(&self.val())
    }

    /// Returns the value as a u64.
    pub fn uint64(&self) -> u64 {
        gconv::uint64_value(&self.val())
    }

    /// Returns the value as an f64.
    pub fn float64(&self) -> f64 {
        gconv::float64_value(&self.val())
    }

    /// Returns the value as a bool.
    pub fn bool(&self) -> bool {
        gconv::bool_value(&self.val())
    }

    /// Returns the value as bytes.
    pub fn bytes(&self) -> Vec<u8> {
        gconv::bytes(&self.val())
    }

    /// Returns the value as strings.
    pub fn strings(&self) -> Vec<String> {
        gconv::strings(&self.val())
    }

    /// Returns the value as integers.
    pub fn ints(&self) -> Vec<i64> {
        gconv::ints(&self.val())
    }

    /// Returns the value as floats.
    pub fn floats(&self) -> Vec<f64> {
        gconv::floats(&self.val())
    }

    /// Returns the value as a map.
    pub fn map(&self) -> IndexMap<String, Value> {
        gconv::map(&self.val())
    }

    /// Encodes the value as JSON text.
    pub fn marshal_json(&self) -> Result<String, gerror::Error> {
        gf_json::marshal_value(&self.val())
    }

    /// Replaces the value from JSON text.
    pub fn unmarshal_json(&self, content: &[u8]) -> Result<(), gerror::Error> {
        let value = gf_json::unmarshal_value(content)?;
        self.set(value);
        Ok(())
    }
}

impl Clone for Var {
    fn clone(&self) -> Self {
        Var::new(self.val())
    }
}

impl std::fmt::Debug for Var {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.string())
    }
}

impl From<Value> for Var {
    fn from(value: Value) -> Self {
        Var::new(value)
    }
}

/// Wraps every value in a [`Var`].
pub fn vars(values: &[Value]) -> Vec<Var> {
    values.iter().cloned().map(Var::new).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_match_go() {
        let var = Var::new(Value::Int(123));
        assert_eq!(var.int64(), 123);
        assert_eq!(var.string(), "123");
        assert_eq!(var.uint64(), 123);
        assert_eq!(var.float64(), 123.0);
        assert!(var.bool());
        assert!(!var.is_nil());
        assert!(!var.is_empty());
        assert_eq!(var.bytes(), b"123");

        var.set(Value::String("off".into()));
        assert!(!var.bool());
        assert_eq!(var.string(), "off");
        assert!(!var.is_nil());

        var.set(Value::Null);
        assert!(var.is_nil());
        assert!(var.is_empty());
        assert_eq!(var.int64(), 0);
    }

    #[test]
    fn collections_and_json() {
        let mut map = IndexMap::new();
        map.insert("a".to_owned(), Value::Int(1));
        map.insert("b".to_owned(), Value::String("x".into()));
        let var = Var::new(Value::Map(map));
        assert_eq!(var.map().len(), 2);
        assert_eq!(var.strings(), vec!["1".to_owned(), "x".to_owned()]);
        assert_eq!(var.marshal_json().expect("json"), r#"{"a":1,"b":"x"}"#);

        let decoded = Var::new(Value::Null);
        decoded.unmarshal_json(br#"{"n":2}"#).expect("unmarshal");
        let map = decoded.map();
        assert_eq!(gconv::int64_value(&map["n"]), 2);
    }

    #[test]
    fn clone_and_vars() {
        let var = Var::new(Value::String("a".into()));
        let copy = var.clone();
        copy.set(Value::String("b".into()));
        assert_eq!(var.string(), "a");
        assert_eq!(copy.string(), "b");

        let list = vars(&[Value::Int(1), Value::Int(2)]);
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].int64(), 2);
    }
}
