//! Port of `internal/deepcopy`.
//!
//! Go needs reflection to copy arbitrary values. [`Value`] owns its contents,
//! so `Clone` is already a deep copy and this module keeps the original API
//! contract: the returned value is structurally equal to the source and shares
//! no owned data with it.

use gf_core::Value;

/// A type that knows how to copy itself into a [`Value`].
pub trait Interface {
    /// Returns a deep copy of `self`.
    fn deep_copy(&self) -> Value;
}

impl Interface for Value {
    fn deep_copy(&self) -> Value {
        self.clone()
    }
}

/// Creates a deep copy of `src`.
pub fn copy(src: &Value) -> Value {
    src.deep_copy()
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    fn nested() -> Value {
        let mut inner = IndexMap::new();
        inner.insert(
            "vals".to_owned(),
            Value::List(vec![Value::Int(1), Value::Int(2)]),
        );
        let mut outer = IndexMap::new();
        outer.insert("inner".to_owned(), Value::Map(inner));
        Value::Map(outer)
    }

    #[test]
    fn copy_is_equal_and_owns_its_data() {
        let original = nested();
        let copied = copy(&original);
        assert_eq!(original, copied);

        let mut mutated = copied;
        let Value::Map(outer) = &mut mutated else {
            panic!("expected map");
        };
        let Some(Value::Map(inner)) = outer.get_mut("inner") else {
            panic!("expected nested map");
        };
        let Some(Value::List(list)) = inner.get_mut("vals") else {
            panic!("expected list");
        };
        list.push(Value::Int(3));

        assert_ne!(original, mutated);
        assert_eq!(original.to_string(), "map[inner:map[vals:[1 2]]]");
    }

    #[test]
    fn scalars_copy_by_value() {
        assert_eq!(copy(&Value::Null), Value::Null);
        assert_eq!(copy(&Value::Int(7)), Value::Int(7));
        assert_eq!(copy(&Value::String("s".into())), Value::String("s".into()));
    }
}
