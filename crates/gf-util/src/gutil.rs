//! Port of `util/gutil`'s comparator family (`gutil_comparator.go`), the
//! case/symbol-insensitive map key lookup and the map/slice helpers from
//! `gutil_map.go`/`gutil_slice.go` (ADR-133).
//!
//! Go's comparators take `any` and coerce through `gconv`; the port takes
//! [`gf_core::Value`] and uses [`gf_util::gconv`]. `ComparatorTime` waits for
//! the shared time abstraction (`os/gtime`) and is recorded as pending in
//! ADR-032.

use std::cmp::Ordering;

use gf_core::Value;
use indexmap::IndexMap;

use crate::gconv;

/// A `map[string]any` in the port, like GoFrame''s `g.Map`.
pub type Map = IndexMap<String, Value>;

/// Shallow-copies `data`, like `MapCopy`.
pub fn map_copy(data: &Map) -> Map {
    data.clone()
}

/// Reports whether `data` contains `key`, like `MapContains`.
pub fn map_contains(data: &Map, key: &str) -> bool {
    data.contains_key(key)
}

/// Deletes every key in `keys`, like `MapDelete`.
pub fn map_delete(data: &mut Map, keys: &[&str]) {
    for key in keys {
        data.shift_remove(*key);
    }
}

/// Merges `src` into `dst`, later maps overwriting earlier keys, like
/// `MapMerge`; empty source maps are ignored by GoFrame''s `maps.Copy`.
pub fn map_merge(dst: &mut Map, src: &[Map]) {
    for map in src {
        for (key, value) in map {
            dst.insert(key.clone(), value.clone());
        }
    }
}

/// Merges every map into a new one, like `MapMergeCopy`.
pub fn map_merge_copy(src: &[Map]) -> Map {
    let mut result = Map::new();
    map_merge(&mut result, src);
    result
}

/// Deletes every empty value, like `MapOmitEmpty`.
pub fn map_omit_empty(data: &mut Map) {
    data.retain(|_, value| !is_empty_value(value));
}

/// Reports whether `value` is empty (nil, false, zero, empty string/list/map),
/// like GoFrame''s `IsEmpty`.
pub fn is_empty_value(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Bool(value) => !*value,
        Value::Int(value) => *value == 0,
        Value::UInt(value) => *value == 0,
        Value::Float(value) => *value == 0.0,
        Value::String(value) => value.is_empty(),
        Value::Bytes(value) => value.is_empty(),
        Value::List(value) => value.is_empty(),
        Value::Map(value) => value.is_empty(),
    }
}

/// Flattens a map into `[key, value, key, value, ...]`, like `MapToSlice`;
/// a list is returned unchanged and other values produce an empty list.
pub fn map_to_slice(data: &Value) -> Vec<Value> {
    match data {
        Value::Map(map) => {
            let mut result = Vec::with_capacity(map.len() * 2);
            for (key, value) in map {
                result.push(Value::String(key.clone()));
                result.push(value.clone());
            }
            result
        }
        Value::List(list) => list.clone(),
        _ => Vec::new(),
    }
}

/// Shallow-copies a slice, like `SliceCopy`.
pub fn slice_copy(slice: &[Value]) -> Vec<Value> {
    slice.to_vec()
}

/// Inserts `values` before `index`, like `SliceInsertBefore`.
pub fn slice_insert_before(slice: &[Value], index: i64, values: &[Value]) -> Vec<Value> {
    let Ok(index) = usize::try_from(index) else {
        return slice.to_vec();
    };
    if index >= slice.len() {
        return slice.to_vec();
    }
    let mut result = Vec::with_capacity(slice.len() + values.len());
    result.extend_from_slice(&slice[..index]);
    result.extend_from_slice(values);
    result.extend_from_slice(&slice[index..]);
    result
}

/// Inserts `values` after `index`, like `SliceInsertAfter`.
pub fn slice_insert_after(slice: &[Value], index: i64, values: &[Value]) -> Vec<Value> {
    let Ok(index) = usize::try_from(index) else {
        return slice.to_vec();
    };
    if index >= slice.len() {
        return slice.to_vec();
    }
    let mut result = Vec::with_capacity(slice.len() + values.len());
    result.extend_from_slice(&slice[..=index]);
    result.extend_from_slice(values);
    result.extend_from_slice(&slice[index + 1..]);
    result
}

/// Deletes the element at `index`, like `SliceDelete`.
pub fn slice_delete(slice: &[Value], index: i64) -> Vec<Value> {
    let Ok(index) = usize::try_from(index) else {
        return slice.to_vec();
    };
    if index >= slice.len() {
        return slice.to_vec();
    }
    let mut result = Vec::with_capacity(slice.len() - 1);
    result.extend_from_slice(&slice[..index]);
    result.extend_from_slice(&slice[index + 1..]);
    result
}

/// Converts `[key, value, ...]` into a map, like `SliceToMap`; an odd length
/// list or a non-list value returns `None`.
pub fn slice_to_map(slice: &Value) -> Option<Map> {
    let Value::List(items) = slice else {
        return None;
    };
    if items.len() % 2 != 0 {
        return None;
    }
    let mut result = Map::new();
    for pair in items.chunks(2) {
        result.insert(gconv::string(&pair[0]), pair[1].clone());
    }
    Some(result)
}

/// Builds a map keyed by `column` from a list of maps, like
/// `SliceToMapWithColumnAsKey`.
pub fn slice_to_map_with_column_as_key(slice: &Value, column: &Value) -> Map {
    let Value::List(items) = slice else {
        return Map::new();
    };
    let mut result = Map::new();
    for item in items {
        if let Value::Map(map) = item
            && let Some(value) = map.get(&gconv::string(column))
        {
            result.insert(gconv::string(value), item.clone());
        }
    }
    result
}

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

    fn value(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn map_of(entries: &[(&str, Value)]) -> Map {
        entries
            .iter()
            .map(|(key, value)| ((*key).to_owned(), value.clone()))
            .collect()
    }

    #[test]
    fn map_copy_contains_delete() {
        let m1 = map_of(&[("k1", value("v1"))]);
        let mut m2 = map_copy(&m1);
        m2.insert("k2".to_owned(), value("v2"));
        assert_eq!(m1.get("k1"), Some(&value("v1")));
        assert!(!map_contains(&m1, "k2"));
        assert_eq!(m2.get("k1"), Some(&value("v1")));
        assert_eq!(m2.get("k2"), Some(&value("v2")));
        assert!(map_contains(&m1, "k1"));
        assert!(!map_contains(&m1, "K1"));
        assert!(!map_contains(&m1, "k2"));

        let mut m1 = map_of(&[("k1", value("v1"))]);
        map_delete(&mut m1, &["k1"]);
        map_delete(&mut m1, &["K1"]);
        assert!(m1.is_empty());
        map_delete(&mut Map::new(), &["k1"]);
    }

    #[test]
    fn map_merge_and_omit_empty() {
        let m1 = map_of(&[("k1", value("v1"))]);
        let m2 = map_of(&[("k2", value("v2"))]);
        let m3 = map_of(&[("k3", value("v3")), ("k1", value("override"))]);
        let mut merged = map_copy(&m1);
        map_merge(&mut merged, &[m2.clone(), m3.clone()]);
        assert_eq!(merged.get("k1"), Some(&value("override")));
        assert_eq!(merged.get("k2"), Some(&value("v2")));
        assert_eq!(merged.get("k3"), Some(&value("v3")));
        assert!(!m2.contains_key("k1"));
        assert!(!m3.contains_key("k2"));

        let copy = map_merge_copy(&[m1.clone(), m2.clone(), m3.clone()]);
        assert_eq!(copy.len(), 3);
        assert_eq!(m1.len(), 1);

        let mut data = map_of(&[
            ("missing", Value::Null),
            ("false", Value::Bool(false)),
            ("zero", Value::Int(0)),
            ("empty", value("")),
            ("list", Value::List(Vec::new())),
            ("map", Value::Map(Map::new())),
            ("text", value("x")),
            ("number", Value::Int(1)),
        ]);
        map_omit_empty(&mut data);
        assert_eq!(data.len(), 2);
        assert!(data.contains_key("text"));
        assert!(data.contains_key("number"));
    }

    #[test]
    fn map_to_slice_and_slice_helpers() {
        let map = map_of(&[("K1", value("v1")), ("K2", value("v2"))]);
        assert_eq!(
            map_to_slice(&Value::Map(map)),
            vec![value("K1"), value("v1"), value("K2"), value("v2")]
        );
        let list = vec![Value::Int(1), Value::Int(2)];
        assert_eq!(map_to_slice(&Value::List(list.clone())), list);
        assert!(map_to_slice(&Value::Int(1)).is_empty());

        let copied = slice_copy(&list);
        assert_eq!(copied, list);
        assert_eq!(
            slice_insert_before(&list, 1, &[Value::Int(9)]),
            vec![Value::Int(1), Value::Int(9), Value::Int(2)]
        );
        assert_eq!(
            slice_insert_after(&list, 0, &[Value::Int(9)]),
            vec![Value::Int(1), Value::Int(9), Value::Int(2)]
        );
        assert_eq!(
            slice_insert_before(&list, 5, &[Value::Int(9)]),
            list,
            "invalid index returns the original slice"
        );
        assert_eq!(
            slice_delete(&list, 0),
            vec![Value::Int(2)],
            "deleting the first element"
        );
        assert_eq!(
            slice_delete(&list, 1),
            vec![Value::Int(1)],
            "deleting the last element"
        );

        let flat = vec![value("K1"), value("v1"), value("K2"), value("v2")];
        let map = slice_to_map(&Value::List(flat)).expect("even list");
        assert_eq!(map.get("K1"), Some(&value("v1")));
        assert_eq!(map.get("K2"), Some(&value("v2")));
        assert!(slice_to_map(&Value::List(vec![value("K1"), value("v1"), value("K2")])).is_none());
        assert!(slice_to_map(&Value::Int(1)).is_none());
    }

    #[test]
    fn slice_to_map_by_column_table() {
        let slice = Value::List(vec![
            Value::Map(map_of(&[("K1", value("v1")), ("K2", Value::Int(1))])),
            Value::Map(map_of(&[("K1", value("v2")), ("K2", Value::Int(2))])),
        ]);
        let by_k1 = slice_to_map_with_column_as_key(&slice, &value("K1"));
        assert_eq!(by_k1.len(), 2);
        assert_eq!(
            by_k1.get("v1"),
            Some(&Value::Map(map_of(&[
                ("K1", value("v1")),
                ("K2", Value::Int(1))
            ])))
        );
        let by_k2 = slice_to_map_with_column_as_key(&slice, &value("K2"));
        assert!(by_k2.contains_key("1") && by_k2.contains_key("2"));
    }
}
