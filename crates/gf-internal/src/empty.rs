//! Port of `internal/empty`: emptiness checks over [`Value`].
//!
//! The Go package inspects `any` with type assertions and reflection, covering
//! channels, functions, structs and pointer chains. Those have no counterpart in
//! the port's value model; the cases that do map are ported exactly.

use gf_core::Value;

/// Returns whether `value` is empty: `nil`, zero, `false`, `""`, or a
/// zero-length bytes/list/map.
pub fn is_empty(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(v) => !*v,
        Value::Int(v) => *v == 0,
        Value::UInt(v) => *v == 0,
        Value::Float(v) => *v == 0.0,
        Value::String(v) => v.is_empty(),
        Value::Bytes(v) => v.is_empty(),
        Value::List(v) => v.is_empty(),
        Value::Map(v) => v.is_empty(),
    }
}

/// Returns whether `value` is nil.
///
/// In the Go original a nil map/slice/pointer is nil while an empty one is not;
/// [`Value`] represents all of those as their empty variant, and `nil` itself as
/// [`Value::Null`].
pub fn is_nil(value: &Value) -> bool {
    value.is_nil()
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    #[test]
    fn empty_values() {
        assert!(is_empty(&Value::Null));
        assert!(is_empty(&Value::Bool(false)));
        assert!(is_empty(&Value::Int(0)));
        assert!(is_empty(&Value::UInt(0)));
        assert!(is_empty(&Value::Float(0.0)));
        assert!(is_empty(&Value::Float(-0.0)));
        assert!(is_empty(&Value::String(String::new())));
        assert!(is_empty(&Value::Bytes(Vec::new())));
        assert!(is_empty(&Value::List(Vec::new())));
        assert!(is_empty(&Value::Map(IndexMap::new())));
    }

    #[test]
    fn non_empty_values() {
        assert!(!is_empty(&Value::Int(1)));
        assert!(!is_empty(&Value::UInt(1)));
        assert!(!is_empty(&Value::Float(0.1)));
        assert!(!is_empty(&Value::String("0".into())));
        assert!(!is_empty(&Value::Bytes(vec![0])));
        assert!(!is_empty(&Value::List(vec![Value::Int(0)])));
        let mut map = IndexMap::new();
        map.insert("k".to_owned(), Value::Null);
        assert!(!is_empty(&Value::Map(map)));
    }

    #[test]
    fn only_null_is_nil() {
        assert!(is_nil(&Value::Null));
        assert!(!is_nil(&Value::Int(0)));
        assert!(!is_nil(&Value::Float(0.1)));
        assert!(!is_nil(&Value::Map(IndexMap::new())));
        assert!(!is_nil(&Value::List(Vec::new())));
    }
}
