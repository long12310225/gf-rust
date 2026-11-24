//! Port of `util/gutil`'s comparator family (`gutil_comparator.go`) and the
//! case/symbol-insensitive map key lookup (`gutil_map.go`).
//!
//! Go's comparators take `any` and coerce through `gconv`; the port takes
//! [`gf_core::Value`] and uses [`gf_util::gconv`]. `ComparatorTime` waits for
//! the shared time abstraction (`os/gtime`) and is recorded as pending in
//! ADR-032.

use std::cmp::Ordering;

use gf_core::Value;
use indexmap::IndexMap;

use crate::gconv;

/// Compares two values and returns a negative number, zero or a positive
/// number when `a < b`, `a == b` or `a > b`.
pub type Comparator = fn(&Value, &Value) -> i32;

/// Turns an [`Ordering`] into Go's `-1`/`0`/`1` result.
fn ordering_to_int(ordering: Ordering) -> i32 {
    match ordering {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// Compares floats the way Go's `cmp.Compare` does: NaN sorts before every
/// other value, and two NaNs compare equal.
fn compare_f64(a: f64, b: f64) -> i32 {
    if a < b {
        -1
    } else if a > b {
        1
    } else if a == b || (a.is_nan() && b.is_nan()) {
        0
    } else if a.is_nan() {
        -1
    } else {
        1
    }
}

/// Provides a fast comparison on strings (`ComparatorString`).
pub fn comparator_string(a: &Value, b: &Value) -> i32 {
    ordering_to_int(gconv::string(a).cmp(&gconv::string(b)))
}

/// Provides a basic comparison on int (`ComparatorInt`).
pub fn comparator_int(a: &Value, b: &Value) -> i32 {
    ordering_to_int(gconv::int64_value(a).cmp(&gconv::int64_value(b)))
}

/// Provides a basic comparison on int8 (`ComparatorInt8`).
pub fn comparator_int8(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::int64_value(a) as i8).cmp(&(gconv::int64_value(b) as i8)))
}

/// Provides a basic comparison on int16 (`ComparatorInt16`).
pub fn comparator_int16(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::int64_value(a) as i16).cmp(&(gconv::int64_value(b) as i16)))
}

/// Provides a basic comparison on int32 (`ComparatorInt32`).
pub fn comparator_int32(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::int64_value(a) as i32).cmp(&(gconv::int64_value(b) as i32)))
}

/// Provides a basic comparison on int64 (`ComparatorInt64`).
pub fn comparator_int64(a: &Value, b: &Value) -> i32 {
    ordering_to_int(gconv::int64_value(a).cmp(&gconv::int64_value(b)))
}

/// Provides a basic comparison on uint (`ComparatorUint`).
pub fn comparator_uint(a: &Value, b: &Value) -> i32 {
    ordering_to_int(gconv::uint64_value(a).cmp(&gconv::uint64_value(b)))
}

/// Provides a basic comparison on uint8 (`ComparatorUint8`).
pub fn comparator_uint8(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::uint64_value(a) as u8).cmp(&(gconv::uint64_value(b) as u8)))
}

/// Provides a basic comparison on uint16 (`ComparatorUint16`).
pub fn comparator_uint16(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::uint64_value(a) as u16).cmp(&(gconv::uint64_value(b) as u16)))
}

/// Provides a basic comparison on uint32 (`ComparatorUint32`).
pub fn comparator_uint32(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::uint64_value(a) as u32).cmp(&(gconv::uint64_value(b) as u32)))
}

/// Provides a basic comparison on uint64 (`ComparatorUint64`).
pub fn comparator_uint64(a: &Value, b: &Value) -> i32 {
    ordering_to_int(gconv::uint64_value(a).cmp(&gconv::uint64_value(b)))
}

/// Provides a basic comparison on float32 (`ComparatorFloat32`).
pub fn comparator_float32(a: &Value, b: &Value) -> i32 {
    compare_f64(
        gconv::float64_value(a) as f32 as f64,
        gconv::float64_value(b) as f32 as f64,
    )
}

/// Provides a basic comparison on float64 (`ComparatorFloat64`).
pub fn comparator_float64(a: &Value, b: &Value) -> i32 {
    compare_f64(gconv::float64_value(a), gconv::float64_value(b))
}

/// Provides a basic comparison on byte (`ComparatorByte`).
pub fn comparator_byte(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::uint64_value(a) as u8).cmp(&(gconv::uint64_value(b) as u8)))
}

/// Provides a basic comparison on rune (`ComparatorRune`).
pub fn comparator_rune(a: &Value, b: &Value) -> i32 {
    ordering_to_int((gconv::int64_value(a) as u32).cmp(&(gconv::int64_value(b) as u32)))
}

/// Provides a generic comparison for ordered types (`ComparatorT`).
pub fn comparator_t<T: Ord>(a: &T, b: &T) -> i32 {
    ordering_to_int(a.cmp(b))
}

/// Provides a generic comparison that compares the text forms of two values
/// (`ComparatorTStr`).
pub fn comparator_t_str<T: ToString>(a: &T, b: &T) -> i32 {
    ordering_to_int(a.to_string().cmp(&b.to_string()))
}

/// Finds the entry of `key` in `data`, ignoring case and symbols
/// (`MapPossibleItemByKey`).
///
/// An exact key wins; otherwise the first key whose text equals `key` after
/// stripping symbols and folding case is returned. Go iterates the map in
/// unspecified order, the port scans in insertion order.
pub fn map_possible_item_by_key<'a>(
    data: &'a IndexMap<String, Value>,
    key: &str,
) -> Option<(&'a String, &'a Value)> {
    if data.is_empty() {
        return None;
    }
    if let Some(value) = data.get_key_value(key) {
        return Some(value);
    }
    data.iter()
        .find(|(candidate, _)| gf_internal::utils::equal_fold_without_chars(candidate, key))
}

/// Reports whether [`map_possible_item_by_key`] finds `key`
/// (`MapContainsPossibleKey`).
pub fn map_contains_possible_key(data: &IndexMap<String, Value>, key: &str) -> bool {
    map_possible_item_by_key(data, key).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(value: i64) -> Value {
        Value::Int(value)
    }

    fn f(value: f64) -> Value {
        Value::Float(value)
    }

    #[test]
    fn strings_and_ints() {
        // From Test_ComparatorString and Test_ComparatorInt.
        assert_eq!(comparator_string(&v(1), &v(1)), 0);
        assert_eq!(comparator_string(&v(1), &v(2)), -1);
        assert_eq!(comparator_string(&v(2), &v(1)), 1);
        assert_eq!(comparator_int(&v(1), &v(1)), 0);
        assert_eq!(comparator_int(&v(1), &v(2)), -1);
        assert_eq!(comparator_int(&v(2), &v(1)), 1);
    }

    #[test]
    fn every_integer_width() {
        assert_eq!(comparator_int8(&v(1), &v(2)), -1);
        assert_eq!(comparator_int16(&v(1), &v(2)), -1);
        assert_eq!(comparator_int32(&v(1), &v(2)), -1);
        assert_eq!(comparator_int64(&v(1), &v(2)), -1);
        assert_eq!(comparator_uint(&v(1), &v(2)), -1);
        assert_eq!(comparator_uint8(&v(2), &v(6)), -1);
        assert_eq!(comparator_uint16(&v(1), &v(2)), -1);
        // gconv.Uint32(-1000) wraps to 4294966296.
        assert_eq!(comparator_uint32(&v(-1000), &v(2147483640)), 1);
        assert_eq!(comparator_uint64(&v(1), &v(2)), -1);
        assert_eq!(comparator_byte(&v(1), &v(2)), -1);
        assert_eq!(comparator_rune(&v(1), &v(2)), -1);
    }

    #[test]
    fn float_comparisons() {
        assert_eq!(comparator_float32(&f(0.1), &f(0.1)), 0);
        assert_eq!(comparator_float32(&f(1.1), &f(2.1)), -1);
        assert_eq!(comparator_float64(&f(2.1), &f(1.1)), 1);
        assert_eq!(comparator_float64(&f(f64::NAN), &f(1.0)), -1);
        assert_eq!(comparator_float64(&f(1.0), &f(f64::NAN)), 1);
        assert_eq!(comparator_float64(&f(f64::NAN), &f(f64::NAN)), 0);
    }

    #[test]
    fn integer_overflow_extremes() {
        assert_eq!(comparator_int(&v(i64::MIN), &v(i64::MAX)), -1);
        assert_eq!(comparator_int64(&v(i64::MAX), &v(i64::MIN)), 1);
        assert_eq!(comparator_uint64(&Value::UInt(u64::MAX), &v(1)), 1);
        assert_eq!(comparator_uint64(&v(1), &Value::UInt(u64::MAX)), -1);
    }

    #[test]
    fn generic_forms() {
        assert_eq!(comparator_t(&1_i64, &2_i64), -1);
        assert_eq!(comparator_t(&"a".to_owned(), &"b".to_owned()), -1);
        assert_eq!(comparator_t_str(&10_i64, &9_i64), -1);
        let mut values = vec![i64::MAX, 0, i64::MIN, 1, -1];
        values.sort_by(|a, b| match comparator_t(a, b) {
            n if n < 0 => Ordering::Less,
            n if n > 0 => Ordering::Greater,
            _ => Ordering::Equal,
        });
        assert_eq!(values, vec![i64::MIN, -1, 0, 1, i64::MAX]);
    }

    // Translated from the Go tests for MapPossibleItemByKey /
    // MapContainsPossibleKey: exact key first, then case/symbol folding.
    #[test]
    fn possible_map_keys() {
        let mut map = IndexMap::new();
        map.insert("name".to_owned(), Value::Int(1));
        map.insert("user_name".to_owned(), Value::Int(2));
        assert_eq!(
            map_possible_item_by_key(&map, "name").map(|(key, _)| key.as_str()),
            Some("name")
        );
        assert_eq!(
            map_possible_item_by_key(&map, "NAME").map(|(key, _)| key.as_str()),
            Some("name")
        );
        // Symbols are ignored: `username` and `user-name` both fold to
        // `user_name`.
        assert_eq!(
            map_possible_item_by_key(&map, "username").map(|(key, _)| key.as_str()),
            Some("user_name")
        );
        assert_eq!(
            map_possible_item_by_key(&map, "user-name").map(|(key, _)| key.as_str()),
            Some("user_name")
        );
        assert!(map_possible_item_by_key(&map, "missing").is_none());
        assert!(map_contains_possible_key(&map, "USERNAME"));
        assert!(!map_contains_possible_key(&map, "missing"));
        assert!(map_possible_item_by_key(&IndexMap::new(), "name").is_none());
    }
}
