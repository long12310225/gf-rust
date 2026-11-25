//! Port of `container/garray` (the generic array, not the sorted variants).
//!
//! `Array<T>` is a locked `Vec<T>`; index-based API replaces Go's pointer
//! elements. Sorted arrays are separate units (ADR-030).

use std::sync::RwLock;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// A synchronized, appendable array.
pub struct Array<T> {
    inner: RwLock<Vec<T>>,
}

impl<T: Clone> Array<T> {
    /// Creates an empty array.
    pub fn new() -> Array<T> {
        Array {
            inner: RwLock::new(Vec::new()),
        }
    }

    /// Creates an array from `values`.
    pub fn new_from(values: Vec<T>) -> Array<T> {
        Array {
            inner: RwLock::new(values),
        }
    }

    /// Replaces the contents.
    pub fn set_array(&self, values: Vec<T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
    }

    /// Appends values.
    pub fn append(&self, values: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.extend(values);
    }

    /// Replaces the first items with `array`, like `Replace`.
    pub fn replace(&self, array: &[T]) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let max = guard.len().min(array.len());
        for index in 0..max {
            guard[index] = array[index].clone();
        }
    }

    /// Fills `num` items from `start_index`, appending when past the end, like
    /// `Fill`.
    pub fn fill(&self, start_index: i64, num: i64, value: T) -> Result<(), Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(start) = usize::try_from(start_index) else {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[format!("index {start_index} out of array range {}", guard.len()).as_str()],
            ));
        };
        if start > guard.len() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[format!("index {start_index} out of array range {}", guard.len()).as_str()],
            ));
        }
        for offset in 0..num.max(0) as usize {
            let position = start + offset;
            if position < guard.len() {
                guard[position] = value.clone();
            } else {
                guard.push(value.clone());
            }
        }
        Ok(())
    }

    /// Inserts values before `index`, like `InsertBefore`.
    pub fn insert_before(&self, index: i64, values: &[T]) -> Result<(), Error> {
        self.insert_at(index, values, false)
    }

    /// Inserts values after `index`, like `InsertAfter`.
    pub fn insert_after(&self, index: i64, values: &[T]) -> Result<(), Error> {
        self.insert_at(index, values, true)
    }

    fn insert_at(&self, index: i64, values: &[T], after: bool) -> Result<(), Error> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Ok(index) = usize::try_from(index) else {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[format!("index {index} out of array range {}", guard.len()).as_str()],
            ));
        };
        if index >= guard.len() {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[format!("index {index} out of array range {}", guard.len()).as_str()],
            ));
        }
        let at = if after { index + 1 } else { index };
        for (offset, value) in values.iter().enumerate() {
            guard.insert(at + offset, value.clone());
        }
        Ok(())
    }

    /// Pads to `abs(size)` items with `value`, right for positive and left for
    /// negative sizes, like `Pad`.
    pub fn pad(&self, size: i64, value: T) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if size == 0 {
            return;
        }
        let target = size.unsigned_abs() as usize;
        if target <= guard.len() {
            return;
        }
        let padding = target - guard.len();
        if size > 0 {
            guard.extend(std::iter::repeat_n(value, padding));
        } else {
            let mut prefix: Vec<T> = std::iter::repeat_n(value, padding).collect();
            prefix.append(&mut guard);
            *guard = prefix;
        }
    }

    /// Reverses the array in place, like `Reverse`.
    pub fn reverse(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.reverse();
    }

    /// Shuffles the array in place, like `Shuffle`.
    pub fn shuffle(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let length = guard.len();
        for index in 0..length {
            let swap = gf_util::grand::intn(length as i64) as usize;
            guard.swap(index, swap);
        }
    }

    /// Prepends values.
    pub fn prepend(&self, values: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let items: Vec<T> = values.into_iter().collect();
        let old = std::mem::take(&mut *guard);
        guard.extend(items);
        guard.extend(old);
    }

    /// Appends one value.
    pub fn push(&self, value: T) {
        self.append([value]);
    }

    /// Removes and returns the first value.
    pub fn pop_left(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            None
        } else {
            Some(guard.remove(0))
        }
    }

    /// Removes and returns the last value.
    pub fn pop_right(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.pop()
    }

    /// Removes up to `size` values from the left.
    pub fn pop_lefts(&self, size: usize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let take = size.min(guard.len());
        guard.drain(..take).collect()
    }

    /// Removes up to `size` values from the right.
    pub fn pop_rights(&self, size: usize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let take = size.min(guard.len());
        let start = guard.len() - take;
        guard.drain(start..).collect()
    }

    /// Returns the value at `index`.
    pub fn get(&self, index: usize) -> Option<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(index)
            .cloned()
    }

    /// Returns the value at `index`; panics when out of bounds, like Go.
    pub fn at(&self, index: usize) -> T {
        match self.get(index) {
            Some(value) => value,
            None => panic!("array index out of bounds: {index}"),
        }
    }

    /// Replaces the value at `index`.
    pub fn set(&self, index: usize, value: T) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.get_mut(index) {
            Some(slot) => {
                *slot = value;
                true
            }
            None => false,
        }
    }

    /// Removes and returns the value at `index`.
    pub fn remove(&self, index: usize) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if index < guard.len() {
            Some(guard.remove(index))
        } else {
            None
        }
    }

    /// Returns the number of values.
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Reports whether the array is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every value.
    pub fn clear(&self) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
    }

    /// Returns a snapshot.
    pub fn slice(&self) -> Vec<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Clones the array.
    pub fn clone_array(&self) -> Array<T> {
        Array::new_from(self.slice())
    }

    /// Returns `[start, end)` clamped to the length.
    pub fn range(&self, start: usize, end: Option<usize>) -> Vec<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let end = end.unwrap_or(guard.len()).min(guard.len());
        if start >= end {
            return Vec::new();
        }
        guard[start..end].to_vec()
    }

    /// Returns `length` values starting at `offset`.
    pub fn sub_slice(&self, offset: usize, length: Option<usize>) -> Vec<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if offset >= guard.len() {
            return Vec::new();
        }
        let end = (offset + length.unwrap_or(guard.len())).min(guard.len());
        guard[offset..end].to_vec()
    }

    /// Splits the values into chunks.
    pub fn chunk(&self, size: usize) -> Vec<Vec<T>> {
        if size == 0 {
            return Vec::new();
        }
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.chunks(size).map(<[T]>::to_vec).collect()
    }
}

impl<T: Clone + PartialEq> Array<T> {
    /// Reports whether `value` exists.
    pub fn contains(&self, value: &T) -> bool {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(value)
    }

    /// Returns the first index of `value`.
    pub fn search(&self, value: &T) -> isize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .position(|item| item == value)
            .map_or(-1, |index| index as isize)
    }

    /// Removes the first occurrence of `value`.
    pub fn remove_value(&self, value: &T) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.iter().position(|item| item == value) {
            Some(index) => {
                guard.remove(index);
                true
            }
            None => false,
        }
    }
}

impl<T: Clone + PartialEq + std::fmt::Display> Array<T> {
    /// Joins the values with `glue` using their display form.
    pub fn join(&self, glue: &str) -> String {
        self.slice()
            .into_iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(glue)
    }
}

impl<T: Clone> Default for Array<T> {
    fn default() -> Self {
        Array::new()
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for Array<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.fmt(formatter)
    }
}

/// `[]any`.
pub type AnyArray = Array<gf_core::Value>;
/// `[]string`.
pub type StrArray = Array<String>;
/// `[]int`.
pub type IntArray = Array<i64>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_array_operations() {
        let array: IntArray = Array::new();
        array.append([1, 2, 3]);
        array.push(4);
        array.prepend([0]);
        assert_eq!(array.slice(), vec![0, 1, 2, 3, 4]);
        assert_eq!(array.len(), 5);
        assert_eq!(array.at(2), 2);
        assert_eq!(array.get(9), None);
        assert!(array.contains(&3));
        assert_eq!(array.search(&3), 3);
        assert!(array.remove_value(&1));
        assert_eq!(array.slice(), vec![0, 2, 3, 4]);
        assert_eq!(array.remove(0), Some(0));
        assert!(array.set(0, 9));
        assert_eq!(array.slice(), vec![9, 3, 4]);
        assert_eq!(array.range(1, Some(3)), vec![3, 4]);
        assert_eq!(array.sub_slice(2, Some(2)), vec![4]);
        assert_eq!(array.chunk(2).len(), 2);
        assert_eq!(array.pop_left(), Some(9));
        assert_eq!(array.pop_right(), Some(4));
        assert_eq!(array.pop_lefts(1), vec![3]);
        array.clear();
        assert!(array.is_empty());
    }

    #[test]
    fn strings_and_clone() {
        let array: StrArray = Array::new_from(vec!["a".to_owned(), "b".to_owned()]);
        assert_eq!(array.join("-"), "a-b");
        let copy = array.clone_array();
        copy.push("c".to_owned());
        assert_eq!(array.slice().len(), 2);
        assert_eq!(copy.slice().len(), 3);
    }
}

// --- Sorted arrays ---

use std::cmp::Ordering;
use std::hash::Hash;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering as AtomicOrdering};

use gf_core::Value;
use gf_errors::gerror::Error as GError;
use gf_internal::empty;
use gf_util::{gconv, grand, gutil};
use indexmap::IndexMap;

/// Compares two values: negative when `a < b`, zero when they are equal and
/// positive when `a > b`.
pub type Comparator<T> = Arc<dyn Fn(&T, &T) -> i32 + Send + Sync>;

/// Sorts a whole vector in place with the given comparator (`SortedTArray.sorter`).
pub type Sorter<T> = Arc<dyn Fn(&mut Vec<T>, &dyn Fn(&T, &T) -> i32) + Send + Sync>;

/// Wraps a comparator function as a shareable [`Comparator`].
///
/// Constructors take `Option<Comparator<T>>`; pass `Some(comparator(f))` to
/// install an explicit Go-style comparator and `None` for the default one.
pub fn comparator<T, F>(func: F) -> Comparator<T>
where
    F: Fn(&T, &T) -> i32 + Send + Sync + 'static,
{
    Arc::new(func)
}

/// The default comparator per value type, matching Go's typed constructors.
///
/// `SortedIntArray` compares numerically (Go's `defaultComparatorInt`), while
/// `SortedArray`/`SortedStrArray` compare text (Go's `ComparatorTStr` /
/// `defaultComparatorStr`).
pub trait DefaultComparator: ToString + Clone + 'static {
    /// Returns the comparator used when a constructor is given `None`.
    fn default_comparator() -> Comparator<Self> {
        comparator(gutil::comparator_t_str::<Self>)
    }
}

impl DefaultComparator for String {}
impl DefaultComparator for gf_core::Value {}
impl DefaultComparator for i64 {
    fn default_comparator() -> Comparator<Self> {
        comparator(gutil::comparator_t::<i64>)
    }
}
impl DefaultComparator for u64 {
    fn default_comparator() -> Comparator<Self> {
        comparator(gutil::comparator_t::<u64>)
    }
}
impl DefaultComparator for i32 {
    fn default_comparator() -> Comparator<Self> {
        comparator(gutil::comparator_t::<i32>)
    }
}

/// The default comparator of `T`.
fn default_comparator<T: DefaultComparator + 'static>() -> Comparator<T> {
    T::default_comparator()
}

/// Turns a Go comparator result into an [`Ordering`].
fn ordering(result: i32) -> Ordering {
    match result.signum() {
        -1 => Ordering::Less,
        1 => Ordering::Greater,
        _ => Ordering::Equal,
    }
}

/// Sorts `values` with the custom sorter when there is one, or the default stable sort.
fn sort_with<T>(values: &mut Vec<T>, comparator: &Comparator<T>, sorter: Option<&Sorter<T>>) {
    match sorter {
        Some(sorter) => sorter(values, comparator.as_ref()),
        None => values.sort_by(|a, b| ordering(comparator(a, b))),
    }
}

/// Go's `binSearch`: returns the last compared index and comparison result.
fn bin_search<T>(array: &[T], value: &T, comparator: &Comparator<T>) -> (isize, i32) {
    if array.is_empty() {
        return (-1, -2);
    }
    let mut min: isize = 0;
    let mut max: isize = array.len() as isize - 1;
    let mut mid: isize = 0;
    let mut result = -2;
    while min <= max {
        mid = min + (max - min) / 2;
        result = comparator(value, &array[mid as usize]);
        match ordering(result) {
            Ordering::Less => max = mid - 1,
            Ordering::Greater => min = mid + 1,
            Ordering::Equal => return (mid, result),
        }
    }
    (mid, result)
}

/// Go's `gstr.IsNumeric`: an optionally signed integer or decimal number.
fn is_numeric(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut index = 0;
    if matches!(bytes.first(), Some(b'+') | Some(b'-')) {
        index = 1;
    }
    if index >= bytes.len() {
        return false;
    }
    let mut digits_before = 0;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
        digits_before += 1;
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'.' {
        index += 1;
        let mut digits_after = 0;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            digits_after += 1;
            index += 1;
        }
        return index == bytes.len() && (digits_before > 0 || digits_after > 0);
    }
    index == bytes.len() && digits_before > 0
}

/// Escapes `"` and `\` the way Go's `gstr.QuoteMeta(s, "\"")` does.
fn quote_meta_json(text: &str) -> String {
    let mut quoted = String::with_capacity(text.len());
    for ch in text.chars() {
        if ch == '"' || ch == '\\' {
            quoted.push('\\');
        }
        quoted.push(ch);
    }
    quoted
}

/// Reports whether a value is empty the way Go's `empty.IsEmpty` does.
pub trait EmptyCheck {
    /// Returns `true` for the empty value of the type.
    fn is_go_empty(&self) -> bool;
}

macro_rules! impl_empty_numeric {
    ($($type:ty),* $(,)?) => {
        $(
            impl EmptyCheck for $type {
                fn is_go_empty(&self) -> bool {
                    *self == <$type>::default()
                }
            }
        )*
    };
}

impl_empty_numeric!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize, f32, f64);

impl EmptyCheck for bool {
    fn is_go_empty(&self) -> bool {
        !*self
    }
}

impl EmptyCheck for String {
    fn is_go_empty(&self) -> bool {
        self.is_empty()
    }
}

impl EmptyCheck for Value {
    fn is_go_empty(&self) -> bool {
        empty::is_empty(self)
    }
}

/// Selects the map key [`SortedArray::count_values`] uses for a value type.
pub trait CountKey {
    /// The hashable key type stored in the count map.
    type Key: Eq + Hash + Clone;

    /// Returns the count-map key for `self`.
    fn count_key(&self) -> Self::Key;
}

impl CountKey for i64 {
    type Key = i64;

    fn count_key(&self) -> i64 {
        *self
    }
}

impl CountKey for String {
    type Key = String;

    fn count_key(&self) -> String {
        self.clone()
    }
}

impl CountKey for Value {
    type Key = String;

    fn count_key(&self) -> String {
        gconv::string(self)
    }
}

/// Converts a value to the [`Value`] used when encoding a sorted array as JSON.
pub trait ToJsonValue {
    /// Returns the JSON value for `self`.
    fn to_json_value(&self) -> Value;
}

impl ToJsonValue for Value {
    fn to_json_value(&self) -> Value {
        self.clone()
    }
}

impl ToJsonValue for String {
    fn to_json_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl ToJsonValue for i64 {
    fn to_json_value(&self) -> Value {
        Value::Int(*self)
    }
}

/// Converts a decoded JSON [`Value`] into an array element.
pub trait FromJsonValue: Sized {
    /// Returns the element for `value`, or `None` when it cannot be converted.
    fn from_json_value(value: &Value) -> Option<Self>;
}

impl FromJsonValue for Value {
    fn from_json_value(value: &Value) -> Option<Self> {
        Some(value.clone())
    }
}

impl FromJsonValue for String {
    fn from_json_value(value: &Value) -> Option<Self> {
        Some(gconv::string(value))
    }
}

impl FromJsonValue for i64 {
    fn from_json_value(value: &Value) -> Option<Self> {
        Some(gconv::int64_value(value))
    }
}

/// A synchronized array that always stays sorted (`SortedTArray[T]` in Go).
///
/// The default comparator is `gutil.ComparatorTStr`, the text form of `T`, so a
/// `T` must be [`ToString`]. Equality is always `comparator(a, b) == 0`; the port
/// does not require [`PartialEq`].
pub struct SortedArray<T> {
    inner: RwLock<Vec<T>>,
    comparator: Comparator<T>,
    sorter: Option<Sorter<T>>,
    unique: AtomicBool,
}

impl<T: Clone + ToString + DefaultComparator + 'static> SortedArray<T> {
    /// Creates an empty sorted array; `None` installs the default comparator.
    pub fn new(comparator: Option<Comparator<T>>) -> SortedArray<T> {
        SortedArray::new_with_capacity(0, comparator)
    }

    /// Creates an empty sorted array with the given capacity.
    pub fn new_with_capacity(capacity: usize, comparator: Option<Comparator<T>>) -> SortedArray<T> {
        SortedArray {
            inner: RwLock::new(Vec::with_capacity(capacity)),
            comparator: comparator.unwrap_or_else(default_comparator),
            sorter: None,
            unique: AtomicBool::new(false),
        }
    }

    /// Creates a sorted array from `values`, sorting the owned vector in place.
    pub fn new_from(values: Vec<T>, comparator: Option<Comparator<T>>) -> SortedArray<T> {
        let comparator = comparator.unwrap_or_else(default_comparator);
        let mut values = values;
        sort_with(&mut values, &comparator, None);
        SortedArray {
            inner: RwLock::new(values),
            comparator,
            sorter: None,
            unique: AtomicBool::new(false),
        }
    }

    /// Creates a sorted array from a copy of `values`.
    pub fn new_from_copy(values: &[T], comparator: Option<Comparator<T>>) -> SortedArray<T> {
        SortedArray::new_from(values.to_vec(), comparator)
    }

    /// Replaces the contents and re-sorts them.
    pub fn set_array(&self, values: Vec<T>) -> &Self {
        let mut values = values;
        sort_with(&mut values, &self.comparator, self.sorter.as_ref());
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
        self
    }

    /// Sets the comparator and re-sorts; `None` restores the default comparator.
    pub fn set_comparator(&mut self, comparator: Option<Comparator<T>>) -> &Self {
        self.comparator = comparator.unwrap_or_else(default_comparator);
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sort_with(&mut guard, &self.comparator, self.sorter.as_ref());
        self
    }

    /// Sets the sorter and re-sorts; `None` restores the default stable sort.
    pub fn set_sorter(&mut self, sorter: Option<Sorter<T>>) -> &Self {
        self.sorter = sorter;
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sort_with(&mut guard, &self.comparator, self.sorter.as_ref());
        self
    }

    /// Sorts the array with the current sorter and comparator.
    pub fn sort(&self) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        sort_with(&mut guard, &self.comparator, self.sorter.as_ref());
        self
    }

    /// Adds values, keeping the array sorted; alias of [`SortedArray::append`].
    pub fn add(&self, values: impl IntoIterator<Item = T>) -> &Self {
        self.append(values)
    }

    /// Inserts values at their sorted positions, reproducing Go's `binSearch` ties.
    pub fn append(&self, values: impl IntoIterator<Item = T>) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for value in values {
            let (index, result) = bin_search(&guard, &value, &self.comparator);
            if self.unique.load(AtomicOrdering::Relaxed) && result == 0 {
                continue;
            }
            if index < 0 {
                guard.push(value);
                continue;
            }
            let mut position = index as usize;
            if result > 0 {
                position += 1;
            }
            guard.insert(position, value);
        }
        self
    }

    /// Returns the value at `index`, negative indices included as not found.
    pub fn get(&self, index: isize) -> Option<T> {
        if index < 0 {
            return None;
        }
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(index as usize)
            .cloned()
    }

    /// Returns the value at `index`; panics when out of range (Go returns the zero value).
    pub fn at(&self, index: isize) -> T {
        match self.get(index) {
            Some(value) => value,
            None => panic!("sorted array index out of bounds: {index}"),
        }
    }

    /// Removes and returns the value at `index`.
    pub fn remove(&self, index: isize) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if index < 0 || index as usize >= guard.len() {
            None
        } else {
            Some(guard.remove(index as usize))
        }
    }

    /// Removes the value equal to `value` under the comparator.
    pub fn remove_value(&self, value: &T) -> bool {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (index, result) = bin_search(&guard, value, &self.comparator);
        if result == 0 {
            guard.remove(index as usize);
            true
        } else {
            false
        }
    }

    /// Removes every listed value that exists.
    pub fn remove_values(&self, values: impl IntoIterator<Item = T>) {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for value in values {
            let (index, result) = bin_search(&guard, &value, &self.comparator);
            if result == 0 {
                guard.remove(index as usize);
            }
        }
    }

    /// Removes and returns the first value.
    pub fn pop_left(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            None
        } else {
            Some(guard.remove(0))
        }
    }

    /// Removes and returns the last value.
    pub fn pop_right(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.pop()
    }

    /// Removes and returns up to `size` values from the beginning.
    pub fn pop_lefts(&self, size: isize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if size <= 0 || guard.is_empty() {
            return Vec::new();
        }
        if size as usize >= guard.len() {
            return std::mem::take(&mut *guard);
        }
        guard.drain(..size as usize).collect()
    }

    /// Removes and returns up to `size` values from the end.
    pub fn pop_rights(&self, size: isize) -> Vec<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if size <= 0 || guard.is_empty() {
            return Vec::new();
        }
        let index = guard.len() as isize - size;
        if index <= 0 {
            return std::mem::take(&mut *guard);
        }
        guard.drain(index as usize..).collect()
    }

    /// Removes and returns a random value.
    pub fn pop_rand(&self) -> Option<T> {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            return None;
        }
        let index = grand::intn(guard.len() as i64) as usize;
        Some(guard.remove(index))
    }

    /// Removes and returns up to `size` random values.
    pub fn pop_rands(&self, size: isize) -> Vec<T> {
        if size <= 0 {
            return Vec::new();
        }
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            return Vec::new();
        }
        let take = (size as usize).min(guard.len());
        let mut values = Vec::with_capacity(take);
        for _ in 0..take {
            let index = grand::intn(guard.len() as i64) as usize;
            values.push(guard.remove(index));
        }
        values
    }

    /// Returns `[start, end)` with Go's negative-`end` and clamping rules.
    pub fn range(&self, start: isize, end: Option<isize>) -> Vec<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let length = guard.len() as isize;
        let mut offset_end = length;
        if let Some(end) = end.filter(|end| *end < offset_end) {
            offset_end = end;
            if offset_end < 0 {
                offset_end += length;
            }
        }
        let start = if start < 0 { 0 } else { start };
        if start > offset_end {
            return Vec::new();
        }
        guard[start as usize..offset_end as usize].to_vec()
    }

    /// Returns a slice by `offset`/`length` with Go's negative-index rules.
    pub fn sub_slice(&self, offset: isize, length: Option<isize>) -> Vec<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let length_total = guard.len() as isize;
        let mut size = length.unwrap_or(length_total);
        if offset > length_total {
            return Vec::new();
        }
        let mut offset = offset;
        if offset < 0 {
            offset += length_total;
            if offset < 0 {
                return Vec::new();
            }
        }
        if size < 0 {
            offset += size;
            size = -size;
            if offset < 0 {
                return Vec::new();
            }
        }
        let mut end = offset + size;
        if end > length_total {
            end = length_total;
        }
        guard[offset as usize..end as usize].to_vec()
    }

    /// Returns the sum of the values converted to `i64` through their text form.
    pub fn sum(&self) -> i64 {
        self.slice()
            .iter()
            .map(|value| gconv::int64_value(&Value::String(value.to_string())))
            .sum()
    }

    /// Returns the number of values.
    pub fn len(&self) -> usize {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Reports whether the array is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a snapshot of the values.
    pub fn slice(&self) -> Vec<T> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Reports whether `value` exists, searching by comparator.
    pub fn contains(&self, value: &T) -> bool {
        self.search(value) != -1
    }

    /// Returns the index of `value`, or `-1` when absent.
    pub fn search(&self, value: &T) -> isize {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (index, result) = bin_search(&guard, value, &self.comparator);
        if result == 0 { index } else { -1 }
    }

    /// Enables or disables unique insertion; enabling dedups when it changes state.
    pub fn set_unique(&self, unique: bool) -> &Self {
        let previous = self.unique.swap(unique, AtomicOrdering::Relaxed);
        if unique && !previous {
            self.unique();
        }
        self
    }

    /// Removes repeated values, comparing by comparator.
    pub fn unique(&self) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            return self;
        }
        let comparator = Arc::clone(&self.comparator);
        guard.dedup_by(|a, b| comparator(a, b) == 0);
        self
    }

    /// Returns a sorted copy of the array with the same comparator and sorter.
    pub fn clone_array(&self) -> SortedArray<T> {
        let mut values = self.slice();
        sort_with(&mut values, &self.comparator, self.sorter.as_ref());
        SortedArray {
            inner: RwLock::new(values),
            comparator: Arc::clone(&self.comparator),
            sorter: self.sorter.clone(),
            unique: AtomicBool::new(self.unique.load(AtomicOrdering::Relaxed)),
        }
    }

    /// Removes every value.
    pub fn clear(&self) -> &Self {
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.clear();
        self
    }

    /// Runs `func` on a copy of the values, then stores it back and re-sorts.
    ///
    /// Divergence from Go: the lock is dropped while `func` runs (ADR-007), so
    /// concurrent writes during the callback are overwritten. Go holds its lock
    /// across the callback; this port cannot run user code under the lock.
    pub fn lock_func(&self, func: impl FnOnce(&mut Vec<T>)) -> &Self {
        let mut values = self.slice();
        func(&mut values);
        sort_with(&mut values, &self.comparator, self.sorter.as_ref());
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
        self
    }

    /// Runs `func` on a snapshot and writes the result back.
    ///
    /// Go hands the live backing array to `func`; the port snapshots, runs
    /// `func` after dropping the lock (ADR-007), then stores the result. It
    /// does not re-sort, matching Go's read-lock variant.
    pub fn rlock_func(&self, func: impl FnOnce(&mut Vec<T>)) -> &Self {
        let mut values = self.slice();
        func(&mut values);
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
        self
    }

    /// Merges any value source into the array.
    pub fn merge(&self, values: impl IntoIterator<Item = T>) -> &Self {
        let values: Vec<T> = values.into_iter().collect();
        self.append(values)
    }

    /// Splits the array into chunks of `size`; empty for `size < 1`.
    pub fn chunk(&self, size: isize) -> Vec<Vec<T>> {
        if size < 1 {
            return Vec::new();
        }
        self.slice()
            .chunks(size as usize)
            .map(<[T]>::to_vec)
            .collect()
    }

    /// Returns a random value without removing it.
    pub fn rand(&self) -> Option<T> {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            return None;
        }
        Some(guard[grand::intn(guard.len() as i64) as usize].clone())
    }

    /// Returns `size` random values without removing them.
    pub fn rands(&self, size: isize) -> Vec<T> {
        if size <= 0 {
            return Vec::new();
        }
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if guard.is_empty() {
            return Vec::new();
        }
        (0..size as usize)
            .map(|_| guard[grand::intn(guard.len() as i64) as usize].clone())
            .collect()
    }

    /// Joins the display forms of the values with `glue`.
    pub fn join(&self, glue: &str) -> String {
        let values = self.slice();
        if values.is_empty() {
            return String::new();
        }
        values
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<String>>()
            .join(glue)
    }

    /// Iterates in ascending order; alias of [`SortedArray::iterator_asc`].
    pub fn iterator(&self, func: impl FnMut(usize, &T) -> bool) {
        self.iterator_asc(func);
    }

    /// Iterates in ascending order on a snapshot, stopping when `func` returns `false`.
    pub fn iterator_asc(&self, mut func: impl FnMut(usize, &T) -> bool) {
        let values = self.slice();
        for (index, value) in values.iter().enumerate() {
            if !func(index, value) {
                break;
            }
        }
    }

    /// Iterates in descending order on a snapshot, stopping when `func` returns `false`.
    pub fn iterator_desc(&self, mut func: impl FnMut(usize, &T) -> bool) {
        let values = self.slice();
        for (index, value) in values.iter().enumerate().rev() {
            if !func(index, value) {
                break;
            }
        }
    }

    /// Returns Go's JSON-like text: numeric text unquoted, everything else quoted.
    pub fn string(&self) -> String {
        let values = self.slice();
        let mut text = String::from("[");
        for (index, value) in values.iter().enumerate() {
            let item = value.to_string();
            if is_numeric(&item) {
                text.push_str(&item);
            } else {
                text.push('"');
                text.push_str(&quote_meta_json(&item));
                text.push('"');
            }
            if index + 1 != values.len() {
                text.push(',');
            }
        }
        text.push(']');
        text
    }

    /// Filters values, removing those for which `func` returns `true`.
    ///
    /// The callback runs on a snapshot, never under the lock (ADR-007).
    pub fn filter(&self, mut func: impl FnMut(usize, &T) -> bool) -> &Self {
        let values = self.slice();
        let mut kept = Vec::with_capacity(values.len());
        for (index, value) in values.into_iter().enumerate() {
            if !func(index, &value) {
                kept.push(value);
            }
        }
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = kept;
        self
    }

    /// Applies `func` to every value, then stores the results and re-sorts.
    ///
    /// The callback runs on a snapshot, never under the lock (ADR-007).
    pub fn walk(&self, func: impl FnMut(&T) -> T) -> &Self {
        let mut values: Vec<T> = self.slice().iter().map(func).collect();
        sort_with(&mut values, &self.comparator, self.sorter.as_ref());
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
        self
    }

    /// Returns a copy with each value cloned; [`Value`] already owns its data.
    pub fn deep_copy(&self) -> SortedArray<T> {
        SortedArray {
            inner: RwLock::new(self.slice()),
            comparator: Arc::clone(&self.comparator),
            sorter: self.sorter.clone(),
            unique: AtomicBool::new(self.unique.load(AtomicOrdering::Relaxed)),
        }
    }
}

impl<T: Clone + ToString + EmptyCheck + DefaultComparator + 'static> SortedArray<T> {
    /// Removes values considered empty by Go's `empty.IsEmpty`.
    pub fn filter_empty(&self) -> &Self {
        let kept: Vec<T> = self
            .slice()
            .into_iter()
            .filter(|value| !value.is_go_empty())
            .collect();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = kept;
        self
    }
}

impl<T: Clone + ToString + CountKey + DefaultComparator + 'static> SortedArray<T> {
    /// Counts occurrences of each value, keeping first-seen order.
    ///
    /// The map key is `T` for `i64` and `String`; for [`Value`] it is the
    /// value's text form because [`Value`] has neither `Eq` nor `Hash`.
    pub fn count_values(&self) -> IndexMap<T::Key, i64> {
        let mut counts = IndexMap::new();
        for value in self.slice() {
            *counts.entry(value.count_key()).or_insert(0) += 1;
        }
        counts
    }
}

impl SortedArray<Value> {
    /// Removes `nil` values; only [`Value`] can represent `nil`.
    pub fn filter_nil(&self) -> &Self {
        let kept: Vec<Value> = self
            .slice()
            .into_iter()
            .filter(|value| !empty::is_nil(value))
            .collect();
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = kept;
        self
    }
}

impl SortedArray<String> {
    /// Reports whether `value` exists ignoring case.
    pub fn contains_i(&self, value: &str) -> bool {
        let needle = value.to_lowercase();
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .any(|item| item.to_lowercase() == needle)
    }
}

impl<T: Clone + ToString + DefaultComparator + ToJsonValue + FromJsonValue + 'static>
    SortedArray<T>
{
    /// Encodes the values as a JSON array, like Go's `MarshalJSON`.
    pub fn marshal_json(&self) -> Result<String, GError> {
        let items: Vec<Value> = self
            .slice()
            .iter()
            .map(ToJsonValue::to_json_value)
            .collect();
        gf_json::marshal_value(&Value::List(items))
    }

    /// Decodes a JSON array and re-sorts, like Go's `UnmarshalJSON`.
    pub fn unmarshal_json(&self, data: &[u8]) -> Result<(), GError> {
        let value = gf_json::unmarshal_value(data)?;
        self.load_value(&value)
    }

    /// Sets the array from any value, decoding strings/bytes as JSON.
    pub fn unmarshal_value(&self, value: &Value) -> Result<(), GError> {
        let source = match value {
            Value::String(text) => gf_json::unmarshal_value(text.as_bytes())?,
            Value::Bytes(bytes) => gf_json::unmarshal_value(bytes)?,
            other => other.clone(),
        };
        self.load_value(&source)
    }

    /// Converts a decoded list into values, stores them and re-sorts.
    fn load_value(&self, source: &Value) -> Result<(), GError> {
        let Value::List(items) = source else {
            return Err(GError::new("json.Unmarshal failed"));
        };
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            match T::from_json_value(item) {
                Some(value) => values.push(value),
                None => return Err(GError::new("json.Unmarshal failed")),
            }
        }
        sort_with(&mut values, &self.comparator, self.sorter.as_ref());
        let mut guard = self
            .inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = values;
        Ok(())
    }
}

impl<T: Clone + ToString + DefaultComparator + 'static> IntoIterator for &SortedArray<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.slice().into_iter()
    }
}

impl<T: Clone + std::fmt::Debug> std::fmt::Debug for SortedArray<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self
            .inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.fmt(formatter)
    }
}

/// Go's generic `SortedTArray[T]`.
pub type SortedTArray<T> = SortedArray<T>;
/// Go's `SortedIntArray`; sorts numerically like `defaultComparatorInt`.
pub type SortedIntArray = SortedArray<i64>;
/// Go's `SortedStrArray`.
pub type SortedStrArray = SortedArray<String>;
/// Go's `SortedArray` (`[]any`).
pub type SortedAnyArray = SortedArray<gf_core::Value>;

/// Creates a sorted array with the type's default comparator (`SortedArray`).
pub fn new_sorted_array<T>(comparator: Option<Comparator<T>>) -> SortedArray<T>
where
    T: Clone + ToString + DefaultComparator + 'static,
{
    SortedArray::new(comparator)
}

/// Creates a sorted array with an explicit comparator
/// (`NewSortedArrayByComparator`).
pub fn new_sorted_array_by<T>(comparator: Comparator<T>) -> SortedArray<T>
where
    T: Clone + ToString + DefaultComparator + 'static,
{
    SortedArray::new(Some(comparator))
}

/// Creates a sorted array from a copy with an explicit comparator
/// (`NewSortedArrayByComparatorFromCopy`).
pub fn new_sorted_array_by_from_copy<T>(values: &[T], comparator: Comparator<T>) -> SortedArray<T>
where
    T: Clone + ToString + DefaultComparator + 'static,
{
    SortedArray::new_from_copy(values, Some(comparator))
}

#[cfg(test)]
mod sorted_tests {
    use std::cmp::Ordering;

    use super::*;
    use gf_core::Value;
    use gf_util::gutil;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    fn sorted_str(items: &[&str]) -> SortedStrArray {
        SortedStrArray::new_from(strings(items), None)
    }

    fn sorted_int(items: &[i64]) -> SortedIntArray {
        SortedIntArray::new_from(items.to_vec(), None)
    }

    fn any_strings(items: &[&str]) -> Vec<Value> {
        items
            .iter()
            .map(|item| Value::String((*item).to_owned()))
            .collect()
    }

    fn any_ints(items: &[i64]) -> Vec<Value> {
        items.iter().map(|item| Value::Int(*item)).collect()
    }

    fn any_array(items: Vec<Value>, compare: Comparator<Value>) -> SortedAnyArray {
        SortedAnyArray::new_from(items, Some(compare))
    }

    fn string_comparator() -> Comparator<String> {
        comparator(|a: &String, b: &String| gutil::comparator_t(a, b))
    }

    fn always_less() -> Comparator<String> {
        comparator(|_: &String, _: &String| -1)
    }

    fn int_comparator() -> Comparator<i64> {
        comparator(|a: &i64, b: &i64| gutil::comparator_t(a, b))
    }

    fn value_string_comparator() -> Comparator<Value> {
        comparator(gutil::comparator_string)
    }

    fn value_int_comparator() -> Comparator<Value> {
        comparator(gutil::comparator_int)
    }

    #[test]
    fn test_sorted_t_array_new_sorted_t_array_from() {
        let array1 =
            SortedTArray::<String>::new_from(strings(&["a", "f", "c"]), Some(string_comparator()));
        assert_eq!(array1.len(), 3);
        assert_eq!(array1.slice(), strings(&["a", "c", "f"]));

        let array2 =
            SortedTArray::<String>::new_from(strings(&["h", "j", "i", "k"]), Some(always_less()));
        assert_eq!(array2.len(), 4);
        assert_eq!(array2.slice(), strings(&["k", "i", "j", "h"]));
    }

    #[test]
    fn test_new_sorted_t_array_from_copy() {
        let array1 = SortedTArray::<String>::new_from_copy(
            &strings(&["a", "f", "c"]),
            Some(string_comparator()),
        );
        let array2 =
            SortedTArray::<String>::new_from_copy(&strings(&["a", "f", "c"]), Some(always_less()));
        assert_eq!(array1.len(), 3);
        assert_eq!(array1.slice(), strings(&["a", "c", "f"]));
        assert_eq!(array2.slice(), strings(&["c", "f", "a"]));
    }

    #[test]
    fn test_sorted_t_array_set_array() {
        let array = sorted_str(&["a", "f", "c"]);
        array.set_array(strings(&["e", "h", "g", "k"]));
        assert_eq!(array.len(), 4);
        assert_eq!(array.slice(), strings(&["e", "g", "h", "k"]));
    }

    #[test]
    fn test_sorted_t_array_sort() {
        let array = SortedTArray::<String>::new_from(strings(&["a", "f", "c"]), None);
        array.sort();
        assert_eq!(array.len(), 3);
        assert_eq!(array.slice(), strings(&["a", "c", "f"]));
    }

    #[test]
    fn test_sorted_t_array_get_and_at() {
        let array = sorted_str(&["a", "f", "c"]);
        assert_eq!(array.get(2), Some("f".to_owned()));
        assert_eq!(array.get(1), Some("c".to_owned()));
        assert_eq!(array.get(99), None);
        assert_eq!(array.at(2), "f");
    }

    #[test]
    fn test_sorted_t_array_remove() {
        let array = sorted_str(&["a", "d", "c", "b"]);
        assert_eq!(array.remove(1), Some("b".to_owned()));
        assert_eq!(array.len(), 3);
        assert!(!array.contains(&"b".to_owned()));
        assert_eq!(array.remove(-1), None);
        assert_eq!(array.remove(100000), None);
        assert_eq!(array.remove(0), Some("a".to_owned()));
        assert_eq!(array.len(), 2);
        assert!(!array.contains(&"a".to_owned()));
        assert_eq!(array.remove(1), Some("d".to_owned()));
        assert_eq!(array.len(), 1);
        assert!(!array.contains(&"d".to_owned()));
    }

    #[test]
    fn test_sorted_t_array_pop_left_and_right() {
        let array = sorted_str(&["a", "d", "c", "b"]);
        assert_eq!(array.pop_left(), Some("a".to_owned()));
        assert_eq!(array.len(), 3);
        assert_eq!(array.slice(), strings(&["b", "c", "d"]));
        assert_eq!(array.pop_right(), Some("d".to_owned()));
        assert_eq!(array.slice(), strings(&["b", "c"]));

        let array = SortedTArray::<i64>::new_from(vec![1, 2, 3], None);
        assert_eq!(array.pop_left(), Some(1));
        assert_eq!(array.pop_left(), Some(2));
        assert_eq!(array.pop_left(), Some(3));
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_sorted_t_array_pop_rand_and_rands() {
        let array = sorted_str(&["a", "d", "c", "b"]);
        let value = array.pop_rand().expect("popped value");
        assert!(strings(&["a", "d", "c", "b"]).contains(&value));
        assert_eq!(array.len(), 3);

        let array = sorted_str(&["a", "d", "c", "b"]);
        let values = array.pop_rands(2);
        assert_eq!(values.len(), 2);
        assert!(
            values
                .iter()
                .all(|value| strings(&["a", "d", "c", "b"]).contains(value))
        );
        assert_eq!(array.len(), 2);
        assert_eq!(array.pop_rands(3).len(), 2);
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_sorted_t_array_empty() {
        let array = SortedTArray::<i64>::new(None);
        assert_eq!(array.pop_left(), None);
        assert!(array.pop_lefts(10).is_empty());
        assert_eq!(array.pop_right(), None);
        assert!(array.pop_rights(10).is_empty());
        assert_eq!(array.pop_rand(), None);
        assert!(array.pop_rands(10).is_empty());
    }

    #[test]
    fn test_sorted_t_array_pop_lefts_and_rights() {
        let array = sorted_str(&["a", "d", "c", "b", "e", "f"]);
        let lefts = array.pop_lefts(2);
        assert_eq!(lefts.len(), 2);
        assert_eq!(array.len(), 4);
        assert_eq!(lefts, strings(&["a", "b"]));

        let rest = array.pop_lefts(5);
        assert_eq!(rest.len(), 4);
        assert_eq!(array.len(), 0);

        let array = sorted_str(&["a", "d", "c", "b", "e", "f"]);
        assert_eq!(array.pop_rights(2), strings(&["e", "f"]));
        assert_eq!(array.len(), 4);
        assert_eq!(array.pop_rights(10).len(), 4);
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_sorted_t_array_range() {
        let array = sorted_str(&["a", "d", "c", "b", "e", "f"]);
        assert_eq!(array.range(2, Some(5)), strings(&["c", "d", "e"]));
        assert_eq!(array.len(), 6);
        assert!(array.range(7, Some(5)).is_empty());
        assert_eq!(array.range(-1, Some(2)), strings(&["a", "b"]));
        assert_eq!(array.range(4, Some(10)), strings(&["e", "f"]));
    }

    #[test]
    fn test_sorted_t_array_sum() {
        let array = sorted_str(&["a", "d", "c", "b", "e", "f"]);
        let numeric = sorted_str(&["1", "2", "3", "b", "e", "f"]);
        let small = sorted_str(&["4", "5", "6"]);
        assert_eq!(array.sum(), 0);
        assert_eq!(numeric.sum(), 6);
        assert_eq!(small.sum(), 15);
    }

    #[test]
    fn test_sorted_t_array_clone_and_clear() {
        let array = sorted_str(&["a", "d", "c", "b", "e", "f"]);
        let clone = array.clone_array();
        assert_eq!(clone.slice(), array.slice());
        array.remove(1);
        assert_ne!(clone.slice(), array.slice());
        assert_eq!(array.len(), 5);
        array.clear();
        assert_eq!(array.len(), 0);
    }

    #[test]
    fn test_sorted_t_array_chunk() {
        let array = sorted_str(&["a", "d", "c", "b", "e"]);
        let chunks = array.chunk(2);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], strings(&["a", "b"]));
        assert_eq!(chunks[2], strings(&["e"]));
        assert!(array.chunk(0).is_empty());

        let array = SortedTArray::<i64>::new_from(vec![1, 2, 3, 4, 5], None);
        let chunks = array.chunk(3);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], vec![1, 2, 3]);
        assert_eq!(chunks[1], vec![4, 5]);
    }

    #[test]
    fn test_sorted_t_array_sub_slice() {
        let array = sorted_str(&["a", "d", "c", "b", "e"]);
        assert_eq!(array.sub_slice(2, Some(3)), strings(&["c", "d", "e"]));
        assert_eq!(array.sub_slice(2, Some(6)), strings(&["c", "d", "e"]));
        assert!(array.sub_slice(7, Some(2)).is_empty());
        assert!(array.sub_slice(1, Some(-2)).is_empty());
        assert!(array.sub_slice(-9, Some(2)).is_empty());
        assert_eq!(array.sub_slice(-1, Some(1)), strings(&["e"]));
    }

    #[test]
    fn test_sorted_t_array_rand_and_rands() {
        let array = sorted_str(&["a", "d", "c"]);
        let value = array.rand().expect("random value");
        assert!(strings(&["a", "d", "c"]).contains(&value));
        assert_eq!(array.len(), 3);
        let values = array.rands(2);
        assert_eq!(values.len(), 2);
        assert_eq!(array.rands(4).len(), 4);

        let empty = sorted_str(&[]);
        assert_eq!(empty.rand(), None);
        assert!(empty.rands(1).is_empty());
    }

    #[test]
    fn test_sorted_t_array_join() {
        let array = sorted_str(&["a", "d", "c"]);
        assert_eq!(array.join(","), "a,c,d");
        assert_eq!(array.join("."), "a.c.d");

        let quoted = sorted_str(&["0", "1", "\"a\"", "\\a"]);
        assert_eq!(quoted.join("."), "\"a\".0.1.\\a");
        assert_eq!(sorted_str(&[]).join("."), "");
    }

    #[test]
    fn test_sorted_t_array_string() {
        let array = sorted_str(&["0", "1", "a", "b"]);
        assert_eq!(array.string(), "[0,1,\"a\",\"b\"]");

        let escaped = SortedTArray::<String>::new_from(strings(&["a\"b\\c"]), None);
        assert_eq!(escaped.string(), "[\"a\\\"b\\\\c\"]");
    }

    #[test]
    fn test_sorted_t_array_count_values() {
        let array = sorted_str(&["a", "d", "c", "c"]);
        let counts = array.count_values();
        assert_eq!(counts.len(), 3);
        assert_eq!(counts.get("c"), Some(&2));
        assert_eq!(counts.get("a"), Some(&1));
    }

    #[test]
    fn test_sorted_t_array_set_unique_and_unique() {
        let array = sorted_int(&[1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5]);
        array.set_unique(true);
        assert_eq!(array.len(), 5);
        assert_eq!(array.slice(), vec![1, 2, 3, 4, 5]);

        let array = sorted_int(&[1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5]);
        array.unique();
        assert_eq!(array.len(), 5);
        assert_eq!(array.slice(), vec![1, 2, 3, 4, 5]);

        let empty = sorted_int(&[]);
        empty.unique();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_sorted_t_array_lock_func_and_rlock_func() {
        let array = sorted_str(&["b", "c", "a"]);
        array.lock_func(|values| {
            let last = values.len() - 1;
            values[last] = "GF fans".to_owned();
        });
        assert_eq!(array.slice(), strings(&["GF fans", "a", "b"]));
        assert!(array.contains(&"GF fans".to_owned()));

        array.rlock_func(|values| {
            values[0] = "zzz".to_owned();
            assert_eq!(values[0], "zzz");
        });
        // Like Go, the read-lock variant keeps the callback's mutations and
        // does not re-sort.
        assert_eq!(array.slice(), strings(&["zzz", "a", "b"]));
    }

    #[test]
    fn test_sorted_t_array_merge() {
        let array = sorted_str(&["a", "b", "c", "d"]);
        assert_eq!(array.merge(strings(&["e", "f"])).len(), 6);
        assert_eq!(array.merge(&array).len(), 12);
        assert_eq!(array.merge(["x".to_owned()]).len(), 13);

        let ints = sorted_int(&[1, 2, 3]);
        assert_eq!(ints.merge([4, 5]).len(), 5);
        assert_eq!(ints.merge(vec![6]).len(), 6);
    }

    #[test]
    fn test_sorted_t_array_json() {
        let array = sorted_str(&["a", "b", "d", "c"]);
        let encoded = array.marshal_json().expect("marshal");
        let expected = gf_json::marshal_string(&strings(&["a", "b", "c", "d"])).expect("marshal");
        assert_eq!(encoded, expected);

        let decoded = SortedStrArray::new(None);
        decoded
            .unmarshal_json(br#"["a","b","d","c"]"#)
            .expect("unmarshal");
        assert_eq!(decoded.slice(), strings(&["a", "b", "c", "d"]));

        let ints = SortedIntArray::new_from(vec![1, 4, 3, 2], Some(int_comparator()));
        assert_eq!(
            ints.marshal_json().expect("marshal"),
            gf_json::marshal_string(&[1_i64, 2, 3, 4]).expect("marshal")
        );
        let target = SortedIntArray::new(Some(int_comparator()));
        target.unmarshal_json(b"[1,4,3,2]").expect("unmarshal");
        assert_eq!(target.slice(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_sorted_t_array_iterator() {
        let array = sorted_str(&["a", "b", "d", "c"]);
        let mut ascending = Vec::new();
        array.iterator_asc(|index, value| {
            ascending.push((index, value.clone()));
            true
        });
        assert_eq!(
            ascending,
            vec![
                (0, "a".to_owned()),
                (1, "b".to_owned()),
                (2, "c".to_owned()),
                (3, "d".to_owned()),
            ]
        );

        let mut descending = Vec::new();
        array.iterator_desc(|index, value| {
            descending.push((index, value.clone()));
            true
        });
        assert_eq!(descending[0], (3, "d".to_owned()));

        let mut count = 0;
        array.iterator(|_, _| {
            count += 1;
            false
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sorted_t_array_remove_value_and_values() {
        let array = sorted_str(&["a", "b", "d", "c"]);
        assert!(!array.remove_value(&"e".to_owned()));
        assert!(array.remove_value(&"b".to_owned()));
        assert!(array.remove_value(&"a".to_owned()));
        assert!(array.remove_value(&"c".to_owned()));
        assert!(!array.remove_value(&"f".to_owned()));

        let array = sorted_str(&["a", "b", "d", "c"]);
        array.remove_values(strings(&["a", "b", "c"]));
        assert_eq!(array.slice(), strings(&["d"]));
    }

    #[test]
    fn test_sorted_t_array_unmarshal_value() {
        let array = SortedIntArray::new(Some(int_comparator()));
        array
            .unmarshal_value(&Value::Bytes(b"[2,3,1]".to_vec()))
            .expect("bytes");
        assert_eq!(array.slice(), vec![1, 2, 3]);

        let array = SortedIntArray::new(Some(int_comparator()));
        array
            .unmarshal_value(&Value::List(vec![
                Value::Int(2),
                Value::Int(3),
                Value::Int(1),
            ]))
            .expect("list");
        assert_eq!(array.slice(), vec![1, 2, 3]);
    }

    #[test]
    fn test_sorted_t_array_filter() {
        let array = SortedIntArray::new_from_copy(&[0, 1, 2, 3, 4, -1, -2], None);
        array.filter(|_, value| *value < 0);
        assert_eq!(array.slice(), vec![0, 1, 2, 3, 4]);

        let array = SortedIntArray::new_from_copy(&[-1, 1, 2, 3, 4, -2], None);
        array.filter(|_, value| *value < 0);
        assert_eq!(array.slice(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_sorted_t_array_filter_empty() {
        let array = SortedIntArray::new_from(vec![0, 1, 2, 3, 4, 0, 0], None);
        array.filter_empty();
        assert_eq!(array.slice(), vec![1, 2, 3, 4]);

        let array = sorted_str(&["a", "", "b", "c", ""]);
        array.filter_empty();
        assert_eq!(array.slice(), strings(&["a", "b", "c"]));
    }

    #[test]
    fn test_sorted_t_array_walk() {
        let array = sorted_str(&["1", "2"]);
        array.walk(|value| format!("key-{value}"));
        assert_eq!(array.slice(), strings(&["key-1", "key-2"]));
    }

    #[test]
    fn test_sorted_t_array_is_empty() {
        assert!(sorted_str(&[]).is_empty());
        assert!(!sorted_str(&["a"]).is_empty());
    }

    #[test]
    fn test_sorted_t_array_deep_copy() {
        let array = sorted_int(&[1, 2, 3, 4, 5]);
        let copy = array.deep_copy();
        array.add([6]);
        copy.add([7]);
        assert_eq!(array.get(5), Some(6));
        assert_eq!(copy.get(5), Some(7));
        assert_ne!(array.get(5), copy.get(5));
    }

    #[test]
    fn test_sorted_t_array_set_comparator_and_sorter() {
        let mut array = sorted_int(&[1, 3, 2]);
        array.set_comparator(Some(comparator(|a: &i64, b: &i64| {
            gutil::comparator_t(b, a)
        })));
        assert_eq!(array.slice(), vec![3, 2, 1]);

        let mut array = sorted_int(&[1, 3, 2]);
        let descending: Sorter<i64> = Arc::new(|values, compare| {
            values.sort_by(|a, b| match compare(b, a).signum() {
                -1 => Ordering::Less,
                1 => Ordering::Greater,
                _ => Ordering::Equal,
            });
        });
        array.set_sorter(Some(descending));
        assert_eq!(array.slice(), vec![3, 2, 1]);
    }

    #[test]
    fn test_sorted_int_array_basic() {
        let array = SortedIntArray::new(Some(comparator(|a: &i64, b: &i64| (a - b) as i32)));
        array.append([0, 3, 2, 1, 4, 5, 6]);
        assert_eq!(array.len(), 7);
        assert_eq!(array.slice(), vec![0, 1, 2, 3, 4, 5, 6]);

        let array = SortedIntArray::new_from(vec![0, 3, 2, 1, 4, 5, 6], Some(int_comparator()));
        assert_eq!(array.join("."), "0.1.2.3.4.5.6");
        assert_eq!(array.at(1), 1);

        let array = SortedIntArray::new_from(vec![0, 1, 2, 3], Some(int_comparator()));
        array.set_array(vec![4, 5, 6]);
        assert_eq!(array.len(), 3);
        assert_eq!(array.search(&3), -1);
        assert_eq!(array.search(&5), 1);
        assert_eq!(array.search(&6), 2);

        let array = SortedIntArray::new_from(vec![1, 3, 5, 0], Some(int_comparator()));
        array.sort();
        assert_eq!(array.get(0), Some(0));
        assert_eq!(array.get(3), Some(5));
        assert_eq!(array.get(99), None);
        assert_eq!(array.remove(-1), None);
        assert_eq!(array.remove(-100000), None);
        assert_eq!(array.remove(2), Some(3));
        assert_eq!(array.search(&5), 2);
        assert_eq!(array.remove(0), Some(0));
        assert_eq!(array.search(&5), 1);
    }

    #[test]
    fn test_sorted_int_array_range() {
        let array = SortedIntArray::new_from(vec![1, 3, 5, 2, 6, 7], Some(int_comparator()));
        assert_eq!(array.range(1, Some(4)), vec![2, 3, 5]);
        assert!(array.range(5, Some(4)).is_empty());
        assert_eq!(array.range(-1, Some(4)), vec![1, 2, 3, 5]);
        assert_eq!(array.range(5, Some(8)), vec![7]);
        assert_eq!(array.range(0, Some(-1)), vec![1, 2, 3, 5, 6]);
        assert_eq!(array.range(-2, Some(-4)), vec![1, 2]);
        assert!(array.range(0, Some(-10)).is_empty());
        assert!(array.range(-2, Some(-10)).is_empty());
        assert_eq!(array.range(4, Some(-1)), vec![6]);
    }

    #[test]
    fn test_sorted_int_array_chunk_and_sub_slice() {
        let array = sorted_int(&[1, 2, 3, 4, 5]);
        let chunks = array.chunk(2);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], vec![1, 2]);
        assert_eq!(chunks[2], vec![5]);
        assert!(array.chunk(-1).is_empty());

        let chunks = array.chunk(3);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], vec![1, 2, 3]);
        assert_eq!(chunks[1], vec![4, 5]);

        assert_eq!(array.sub_slice(1, Some(2)), vec![2, 3]);
        assert!(array.sub_slice(7, Some(2)).is_empty());
        assert_eq!(array.sub_slice(3, Some(5)), vec![4, 5]);
        assert_eq!(array.sub_slice(3, Some(1)), vec![4]);
        assert_eq!(array.sub_slice(-1, Some(1)), vec![5]);
        assert!(array.sub_slice(-9, Some(1)).is_empty());
        assert!(array.sub_slice(1, Some(-9)).is_empty());
    }

    #[test]
    fn test_sorted_int_array_aggregates() {
        let array = SortedIntArray::new_from(vec![1, 3, 5], Some(int_comparator()));
        assert_eq!(array.sum(), 9);
        assert_eq!(array.join("."), "1.3.5");
        assert_eq!(array.string(), "[1,3,5]");
        assert!(!array.contains(&4));
        assert_eq!(array.join("."), "1.3.5");

        let empty = SortedIntArray::new_from(Vec::new(), Some(int_comparator()));
        assert_eq!(empty.join("."), "");

        let counts =
            SortedIntArray::new_from(vec![1, 2, 3, 4, 5, 3], Some(int_comparator())).count_values();
        assert_eq!(counts.len(), 5);
        assert_eq!(counts.get(&2), Some(&1));
        assert_eq!(counts.get(&3), Some(&2));
    }

    #[test]
    fn test_sorted_int_array_unique() {
        let array = SortedIntArray::new_from(
            vec![1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5],
            Some(int_comparator()),
        );
        array.unique();
        assert_eq!(array.len(), 5);
        assert_eq!(array.slice(), vec![1, 2, 3, 4, 5]);

        let array = SortedIntArray::new_from(
            vec![1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5],
            Some(int_comparator()),
        );
        array.set_unique(true);
        assert_eq!(array.slice(), vec![1, 2, 3, 4, 5]);
        assert_eq!(sorted_int(&[]).len(), 0);
    }

    #[test]
    fn test_sorted_int_array_json() {
        let array = SortedIntArray::new_from(vec![1, 4, 3, 2], Some(int_comparator()));
        assert_eq!(
            array.marshal_json().expect("marshal"),
            gf_json::marshal_string(&[1_i64, 2, 3, 4]).expect("marshal")
        );
        let target = SortedIntArray::new(Some(int_comparator()));
        target.unmarshal_json(b"[1,4,3,2]").expect("unmarshal");
        assert_eq!(target.slice(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_sorted_int_array_iterator() {
        let array = SortedIntArray::new_from(vec![10, 20, 30, 40], Some(int_comparator()));
        let mut ascending = Vec::new();
        array.iterator_asc(|_, value| {
            ascending.push(*value);
            true
        });
        assert_eq!(ascending, vec![10, 20, 30, 40]);

        let mut descending = Vec::new();
        array.iterator_desc(|_, value| {
            descending.push(*value);
            true
        });
        assert_eq!(descending, vec![40, 30, 20, 10]);

        let mut count = 0;
        array.iterator_asc(|_, _| {
            count += 1;
            false
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sorted_int_array_remove_value_and_values() {
        let array = SortedIntArray::new_from(vec![10, 20, 30, 40], Some(int_comparator()));
        assert!(!array.remove_value(&99));
        assert!(array.remove_value(&20));
        assert!(array.remove_value(&10));
        assert!(!array.remove_value(&20));
        assert!(!array.remove_value(&88));
        assert_eq!(array.len(), 2);

        let array = SortedIntArray::new_from(vec![10, 20, 30, 40], Some(int_comparator()));
        array.remove_values([10, 40, 20]);
        assert_eq!(array.slice(), vec![30]);
    }

    #[test]
    fn test_sorted_int_array_filter_and_empty() {
        let array = SortedIntArray::new_from(vec![0, 1, 2, 3, 4, 0], Some(int_comparator()));
        array.filter(|_, value| *value == 0);
        assert_eq!(array.slice(), vec![1, 2, 3, 4]);

        let array = SortedIntArray::new_from(vec![0, 1, -1, 2, 3, 4, 0], Some(int_comparator()));
        array.filter_empty();
        assert_eq!(array.slice(), vec![-1, 1, 2, 3, 4]);

        let mut array = SortedIntArray::new_from(vec![0, 0, 0, 0, 0, 0, 1], Some(int_comparator()));
        array.set_comparator(Some(comparator(|a: &i64, b: &i64| {
            gutil::comparator_t(b, a)
        })));
        array.filter_empty();
        assert_eq!(array.slice(), vec![1]);

        let array = SortedIntArray::new_from(vec![1, 2, 3, 4], Some(int_comparator()));
        array.filter_empty();
        assert_eq!(array.slice(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_sorted_int_array_walk_and_deep_copy() {
        let array = SortedIntArray::new_from(vec![1, 2], Some(int_comparator()));
        array.walk(|value| 10 + value);
        assert_eq!(array.slice(), vec![11, 12]);

        let array = SortedIntArray::new_from(vec![1, 2, 3, 4, 5], Some(int_comparator()));
        let copy = array.deep_copy();
        array.add([6]);
        copy.add([7]);
        assert_ne!(array.get(5), copy.get(5));
    }

    #[test]
    fn test_sorted_str_array_basic() {
        let array = SortedStrArray::new(Some(string_comparator()));
        array.add(strings(&["a", "d", "c", "b"]));
        assert_eq!(array.len(), 4);
        assert_eq!(array.slice(), strings(&["a", "b", "c", "d"]));

        assert_eq!(
            sorted_str(&["a", "d", "c", "b"]).slice(),
            strings(&["a", "b", "c", "d"])
        );

        let array = SortedStrArray::new_from_copy(&strings(&["a", "d", "c", "b"]), None);
        assert_eq!(array.len(), 4);
        assert_eq!(array.slice(), strings(&["a", "b", "c", "d"]));

        let array = sorted_str(&["a", "d", "c", "b"]);
        array.set_array(strings(&["f", "g", "h"]));
        assert_eq!(array.len(), 3);
        assert!(!array.contains(&"d".to_owned()));
        assert!(!array.contains(&"b".to_owned()));
        assert!(array.contains(&"g".to_owned()));

        let array = sorted_str(&["a", "d", "c", "b"]);
        array.sort();
        assert_eq!(array.len(), 4);
        assert!(array.contains(&"c".to_owned()));
        assert_eq!(array.slice(), strings(&["a", "b", "c", "d"]));

        assert_eq!(array.get(2), Some("c".to_owned()));
        assert_eq!(array.get(0), Some("a".to_owned()));
        assert_eq!(array.get(99), None);

        assert_eq!(array.remove(-1), None);
        assert_eq!(array.remove(100000), None);
        assert_eq!(array.remove(2), Some("c".to_owned()));
        assert_eq!(array.get(2), Some("d".to_owned()));
        assert!(!array.contains(&"c".to_owned()));
        assert_eq!(array.remove(0), Some("a".to_owned()));
        assert_eq!(array.remove(1), Some("d".to_owned()));
        assert_eq!(array.len(), 1);
    }

    #[test]
    fn test_sorted_str_array_pops() {
        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_left(), Some("a".to_owned()));
        assert_eq!(array.len(), 4);
        assert!(!array.contains(&"a".to_owned()));

        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_right(), Some("e".to_owned()));
        assert_eq!(array.len(), 4);

        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        let value = array.pop_rand().expect("random value");
        assert!(strings(&["e", "a", "d", "c", "b"]).contains(&value));
        assert_eq!(array.len(), 4);
        assert!(!array.contains(&value));

        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        let values = array.pop_rands(2);
        assert_eq!(values.len(), 2);
        assert_eq!(array.len(), 3);
        assert_eq!(array.pop_rands(4).len(), 3);

        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_lefts(2), strings(&["a", "b"]));
        assert_eq!(array.len(), 3);
        assert_eq!(array.pop_lefts(4), strings(&["c", "d", "e"]));
        assert_eq!(array.len(), 0);

        let array = sorted_str(&["e", "a", "d", "c", "b", "f", "g"]);
        assert_eq!(array.pop_rights(2), strings(&["f", "g"]));
        assert_eq!(array.len(), 5);
        assert_eq!(array.pop_rights(6), strings(&["a", "b", "c", "d", "e"]));
        assert_eq!(array.len(), 0);

        let empty = sorted_str(&[]);
        assert_eq!(empty.pop_left(), None);
        assert!(empty.pop_lefts(10).is_empty());
        assert_eq!(empty.pop_right(), None);
        assert!(empty.pop_rights(10).is_empty());
        assert_eq!(empty.pop_rand(), None);
        assert!(empty.pop_rands(10).is_empty());
    }

    #[test]
    fn test_sorted_str_array_range_and_sub_slice() {
        let array = sorted_str(&["e", "a", "d", "c", "b", "f", "g"]);
        assert_eq!(array.range(2, Some(4)), strings(&["c", "d"]));
        assert_eq!(array.range(-1, Some(2)), strings(&["a", "b"]));
        assert_eq!(array.range(4, Some(8)), strings(&["e", "f", "g"]));
        assert!(array.range(10, Some(2)).is_empty());

        assert_eq!(array.sub_slice(1, Some(3)), strings(&["b", "c", "d"]));
        assert_eq!(array.len(), 7);
        assert_eq!(array.sub_slice(1, Some(10)).len(), 6);
        assert!(array.sub_slice(10, Some(2)).is_empty());
        assert_eq!(array.sub_slice(-5, Some(2)), strings(&["c", "d"]));
        assert!(array.sub_slice(-10, Some(2)).is_empty());
        assert!(array.sub_slice(1, Some(-2)).is_empty());
        assert_eq!(array.len(), 7);
    }

    #[test]
    fn test_sorted_str_array_aggregates() {
        let array = sorted_str(&["e", "a", "d", "c", "b", "f", "g"]);
        assert_eq!(array.sum(), 0);
        assert_eq!(sorted_str(&["1", "2", "3", "4", "a"]).sum(), 10);
        assert_eq!(array.len(), 7);

        let a = sorted_str(&["e", "a", "d"]);
        assert_eq!(a.join(","), "a,d,e");
        assert_eq!(a.join("."), "a.d.e");
        assert_eq!(a.string(), "[\"a\",\"d\",\"e\"]");

        let quoted = sorted_str(&["a", "\"b\"", "\\c"]);
        assert_eq!(quoted.join("."), "\"b\".\\c.a");
        assert_eq!(sorted_str(&[]).join("."), "");

        let counts = sorted_str(&["e", "a", "d", "a", "c"]).count_values();
        assert_eq!(counts.get("a"), Some(&2));
        assert_eq!(counts.get("d"), Some(&1));

        let clone = array.clone_array();
        assert_eq!(clone.slice(), array.slice());
        let clone = sorted_str(&["e", "a", "d", "c", "b", "f", "g"]);
        let original = clone.clone_array();
        clone.remove(1);
        assert_eq!(original.len(), 7);
        clone.clear();
        assert_eq!(clone.len(), 0);
    }

    #[test]
    fn test_sorted_str_array_unique() {
        let array = sorted_str(&["1", "1", "2", "2", "3", "3", "2", "2"]);
        array.set_unique(true);
        assert_eq!(array.len(), 3);
        assert_eq!(array.slice(), strings(&["1", "2", "3"]));

        let array = sorted_str(&["1", "1", "2", "2", "3", "3", "2", "2"]);
        array.unique();
        assert_eq!(array.len(), 3);
        assert_eq!(array.slice(), strings(&["1", "2", "3"]));

        let empty = sorted_str(&[]);
        empty.unique();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_sorted_str_array_contains_i() {
        let array = SortedStrArray::new(None);
        array.append(strings(&["a", "b", "C"]));
        assert!(!array.contains(&"A".to_owned()));
        assert!(array.contains(&"a".to_owned()));
        assert!(array.contains_i("A"));

        assert!(!sorted_str(&[]).contains(&"A".to_owned()));
    }

    #[test]
    fn test_sorted_str_array_json() {
        let array = sorted_str(&["a", "b", "d", "c"]);
        assert_eq!(
            array.marshal_json().expect("marshal"),
            gf_json::marshal_string(&strings(&["a", "b", "c", "d"])).expect("marshal")
        );
        let target = SortedStrArray::new(None);
        target
            .unmarshal_json(br#"["a","b","d","c"]"#)
            .expect("unmarshal");
        assert_eq!(target.slice(), strings(&["a", "b", "c", "d"]));
    }

    #[test]
    fn test_sorted_str_array_iterator() {
        let array = sorted_str(&["a", "b", "d", "c"]);
        let mut values = Vec::new();
        array.iterator(|_, value| {
            values.push(value.clone());
            true
        });
        assert_eq!(values, strings(&["a", "b", "c", "d"]));

        let mut descending = Vec::new();
        array.iterator_desc(|_, value| {
            descending.push(value.clone());
            true
        });
        assert_eq!(descending, strings(&["d", "c", "b", "a"]));

        let mut count = 0;
        array.iterator(|_, _| {
            count += 1;
            false
        });
        assert_eq!(count, 1);
    }

    #[test]
    fn test_sorted_str_array_filter_and_empty() {
        let array = sorted_str(&["", "1", "2", "0"]);
        array.filter(|_, value| value.is_empty());
        assert_eq!(array.slice(), strings(&["0", "1", "2"]));

        let array = sorted_str(&["1", "2"]);
        array.filter(|_, value| value.is_empty());
        assert_eq!(array.slice(), strings(&["1", "2"]));

        let array = sorted_str(&["", "1", "", "2", "0", ""]);
        array.filter_empty();
        assert_eq!(array.slice(), strings(&["0", "1", "2"]));

        let mut array = sorted_str(&["", "", "", "2", "0", "a", "b"]);
        array.set_comparator(Some(comparator(|a: &String, b: &String| {
            gutil::comparator_t(b, a)
        })));
        array.filter_empty();
        assert_eq!(array.slice(), strings(&["b", "a", "2", "0"]));

        let array = sorted_str(&["1", "2"]);
        array.filter_empty();
        assert_eq!(array.slice(), strings(&["1", "2"]));
    }

    #[test]
    fn test_sorted_str_array_walk_and_deep_copy() {
        let array = sorted_str(&["1", "2"]);
        array.walk(|value| format!("key-{value}"));
        assert_eq!(array.slice(), strings(&["key-1", "key-2"]));

        let array = sorted_str(&["a", "b", "c", "d"]);
        let copy = array.deep_copy();
        array.add(["e".to_owned()]);
        copy.add(["f".to_owned()]);
        assert_ne!(array.get(4), copy.get(4));
    }

    #[test]
    fn test_sorted_any_array_new_from() {
        let array1 = any_array(any_strings(&["a", "f", "c"]), value_string_comparator());
        assert_eq!(array1.len(), 3);
        assert_eq!(array1.slice(), any_strings(&["a", "c", "f"]));

        let array2 = any_array(
            any_strings(&["h", "j", "i", "k"]),
            comparator(|_: &Value, _: &Value| -1),
        );
        assert_eq!(array2.len(), 4);
        assert_eq!(array2.slice(), any_strings(&["k", "i", "j", "h"]));

        let array3 = any_array(
            any_strings(&["b", "a", "b", "c"]),
            comparator(|a: &Value, b: &Value| gutil::comparator_string(b, a)),
        );
        assert_eq!(array3.slice(), any_strings(&["c", "b", "b", "a"]));

        let copy = SortedAnyArray::new_from_copy(
            &any_strings(&["a", "f", "c"]),
            Some(value_string_comparator()),
        );
        assert_eq!(copy.slice(), any_strings(&["a", "c", "f"]));

        let ints = any_array(any_ints(&[3, 1, 2]), value_int_comparator());
        assert_eq!(ints.at(2), Value::Int(3));
        assert_eq!(ints.get(99), None);
    }

    #[test]
    fn test_sorted_any_array_range_chunk() {
        let array = any_array(
            any_strings(&["a", "d", "c", "b", "e", "f"]),
            value_string_comparator(),
        );
        assert_eq!(array.range(2, Some(5)), any_strings(&["c", "d", "e"]));
        assert_eq!(array.len(), 6);
        assert!(array.range(7, Some(5)).is_empty());
        assert_eq!(array.sub_slice(2, Some(3)), any_strings(&["c", "d", "e"]));
        assert!(array.sub_slice(1, Some(-2)).is_empty());

        let chunks = array.chunk(2);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], any_strings(&["a", "b"]));
        assert_eq!(chunks[2], any_strings(&["e", "f"]));

        assert_eq!(array.pop_lefts(2), any_strings(&["a", "b"]));
        assert_eq!(array.pop_rights(2), any_strings(&["e", "f"]));
        assert_eq!(array.len(), 2);
    }

    #[test]
    fn test_sorted_any_array_unique_and_search() {
        let array = any_array(
            any_ints(&[1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5]),
            value_int_comparator(),
        );
        array.set_unique(true);
        assert_eq!(array.len(), 5);
        assert_eq!(array.slice(), any_ints(&[1, 2, 3, 4, 5]));
        assert_eq!(array.search(&Value::Int(3)), 2);

        let array = any_array(
            any_ints(&[1, 2, 3, 4, 5, 3, 2, 2, 3, 5, 5]),
            value_int_comparator(),
        );
        array.unique();
        assert_eq!(array.slice(), any_ints(&[1, 2, 3, 4, 5]));

        let empty = any_array(Vec::new(), value_int_comparator());
        empty.unique();
        assert_eq!(empty.len(), 0);
    }

    #[test]
    fn test_sorted_any_array_json() {
        let array = any_array(
            any_strings(&["a", "b", "d", "c"]),
            value_string_comparator(),
        );
        assert_eq!(
            array.marshal_json().expect("marshal"),
            gf_json::marshal_string(&strings(&["a", "b", "c", "d"])).expect("marshal")
        );

        let target = SortedAnyArray::new(Some(value_string_comparator()));
        target
            .unmarshal_json(br#"["a","b","d","c"]"#)
            .expect("unmarshal");
        assert_eq!(target.slice(), any_strings(&["a", "b", "c", "d"]));

        let ints = any_array(any_ints(&[4, 2]), value_int_comparator());
        assert_eq!(ints.marshal_json().expect("marshal"), "[2,4]");
        let target = SortedAnyArray::new(Some(value_int_comparator()));
        target.unmarshal_json(b"[99,100,98]").expect("unmarshal");
        assert_eq!(
            target.slice(),
            vec![Value::UInt(98), Value::UInt(99), Value::UInt(100)]
        );
    }

    #[test]
    fn test_sorted_any_array_filter_nil_and_empty() {
        let values = vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
            Value::String(String::new()),
            Value::Null,
            Value::List(Vec::new()),
        ];
        let array = SortedAnyArray::new_from(values.clone(), Some(value_int_comparator()));
        assert_eq!(
            array.filter_nil().slice(),
            vec![
                Value::Int(0),
                Value::String(String::new()),
                Value::List(Vec::new()),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]
        );

        let array = SortedAnyArray::new_from(
            vec![
                Value::Null,
                Value::Int(1),
                Value::Int(2),
                Value::Null,
                Value::Int(3),
                Value::Int(4),
                Value::Null,
            ],
            Some(value_int_comparator()),
        );
        assert_eq!(array.filter_nil().slice(), any_ints(&[1, 2, 3, 4]));

        let values = vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(3),
            Value::Int(4),
            Value::String(String::new()),
            Value::List(Vec::new()),
        ];
        let array = SortedAnyArray::new_from(values.clone(), Some(value_int_comparator()));
        array.filter(|_, value| value.is_nil());
        assert_eq!(
            array.slice(),
            vec![
                Value::Int(0),
                Value::String(String::new()),
                Value::List(Vec::new()),
                Value::Int(1),
                Value::Int(2),
                Value::Int(3),
                Value::Int(4),
            ]
        );

        let array = SortedAnyArray::new_from(values, Some(value_int_comparator()));
        array.filter(|_, value| empty::is_empty(value));
        assert_eq!(array.slice(), any_ints(&[1, 2, 3, 4]));

        let values = vec![
            Value::Int(0),
            Value::Int(1),
            Value::Int(2),
            Value::Int(0),
            Value::Int(-1),
            Value::Int(3),
            Value::Int(4),
            Value::String(String::new()),
            Value::List(Vec::new()),
        ];
        let array = SortedAnyArray::new_from(values, Some(value_int_comparator()));
        array.filter_empty();
        assert_eq!(array.slice(), any_ints(&[-1, 1, 2, 3, 4]));
    }

    #[test]
    fn test_sorted_any_array_string_join_sum() {
        let array = any_array(
            any_strings(&["0", "1", "a", "b"]),
            value_string_comparator(),
        );
        assert_eq!(array.string(), "[0,1,\"a\",\"b\"]");

        let array = any_array(
            vec![
                Value::from(0_i64),
                Value::from(1_i64),
                Value::from("\"a\""),
                Value::from("\\a"),
            ],
            value_string_comparator(),
        );
        assert_eq!(array.join("."), "\"a\".0.1.\\a");

        let array = any_array(any_strings(&["4", "5", "6"]), value_string_comparator());
        assert_eq!(array.sum(), 15);
        assert_eq!(any_array(Vec::new(), value_string_comparator()).sum(), 0);
    }

    #[test]
    fn test_sorted_any_array_count_values() {
        let array = any_array(
            any_strings(&["a", "d", "c", "c"]),
            value_string_comparator(),
        );
        let counts = array.count_values();
        assert_eq!(counts.len(), 3);
        assert_eq!(counts.get("c"), Some(&2));
        assert_eq!(counts.get("a"), Some(&1));
    }

    #[test]
    fn test_sorted_any_array_is_empty() {
        assert!(any_array(Vec::new(), value_string_comparator()).is_empty());
    }

    #[test]
    fn test_example_sorted_int_array_string_and_sum() {
        let array = SortedIntArray::new_from(vec![1, 3, 5], Some(int_comparator()));
        assert_eq!(array.string(), "[1,3,5]");
        assert_eq!(array.sum(), 9);

        let array = SortedTArray::<String>::new_from(strings(&["5", "3", "2"]), None);
        assert_eq!(array.string(), "[2,3,5]");
        assert_eq!(array.sum(), 10);
    }

    #[test]
    fn test_example_sorted_t_array_walk_and_lock_func() {
        let array = SortedTArray::<String>::new_from(strings(&["user", "user_detail"]), None);
        array.walk(|value| format!("gf_{value}"));
        assert_eq!(array.slice(), strings(&["gf_user", "gf_user_detail"]));

        let array = sorted_str(&["b", "c", "a"]);
        array.lock_func(|values| {
            let last = values.len() - 1;
            values[last] = "GF fans".to_owned();
        });
        assert_eq!(array.string(), "[\"GF fans\",\"a\",\"b\"]");
    }

    #[test]
    fn test_example_sorted_str_array_string() {
        let array = sorted_str(&["b", "c", "a"]);
        assert_eq!(array.string(), "[\"a\",\"b\",\"c\"]");
        assert_eq!(
            sorted_str(&["1", "2", "3"]).slice(),
            strings(&["1", "2", "3"])
        );
    }

    #[test]
    fn test_example_sorted_str_array_numeric_divergence() {
        // Go's SortedStrArray always quotes; the shared generic struct follows
        // SortedTArray and leaves numeric text unquoted.
        let array = sorted_str(&["5", "3", "2"]);
        assert_eq!(array.string(), "[2,3,5]");
    }

    #[test]
    fn test_sorted_str_array_pop_left_right() {
        // TestSortedStrArray_PopLeft.
        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_left(), Some("a".to_owned()));
        assert_eq!(array.len(), 4);
        assert!(!array.contains(&"a".to_owned()));

        // TestSortedStrArray_PopRight.
        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_right(), Some("e".to_owned()));
        assert_eq!(array.len(), 4);
        assert!(!array.contains(&"e".to_owned()));

        // Empty arrays: nothing to pop.
        let empty = sorted_str(&[]);
        assert_eq!(empty.pop_left(), None);
        assert_eq!(empty.pop_right(), None);
        assert!(empty.pop_lefts(10).is_empty());
        assert!(empty.pop_rights(10).is_empty());
    }

    #[test]
    fn test_sorted_str_array_pop_lefts_rights() {
        // TestSortedStrArray_PopLefts.
        let array = sorted_str(&["e", "a", "d", "c", "b"]);
        assert_eq!(array.pop_lefts(2), strings(&["a", "b"]));
        assert_eq!(array.len(), 3);
        assert_eq!(array.pop_lefts(4), strings(&["c", "d", "e"]));
        assert_eq!(array.len(), 0);
        assert!(array.pop_lefts(10).is_empty());

        // TestSortedStrArray_PopRights.
        let array = sorted_str(&["e", "a", "d", "c", "b", "f", "g"]);
        assert_eq!(array.pop_rights(2), strings(&["f", "g"]));
        assert_eq!(array.len(), 5);
        assert_eq!(array.pop_rights(6), strings(&["a", "b", "c", "d", "e"]));
        assert_eq!(array.len(), 0);

        // ExampleSortedTArray_PopRights and PopLefts.
        let array = sorted_str(&["c", "b", "a", "d", "f", "e", "h", "g"]);
        assert_eq!(array.pop_rights(2), strings(&["g", "h"]));
        assert_eq!(array.slice(), strings(&["a", "b", "c", "d", "e", "f"]));
        let array = sorted_str(&["c", "b", "a", "d", "f", "e", "h", "g"]);
        assert_eq!(array.pop_lefts(2), strings(&["a", "b"]));
        assert_eq!(array.slice(), strings(&["c", "d", "e", "f", "g", "h"]));
    }

    #[test]
    fn replace_fill_insert_pad_reverse_shuffle() {
        let array: IntArray = Array::new();
        array.append([1, 2, 3]);
        array.replace(&[4, 5]);
        assert_eq!(array.slice(), vec![4, 5, 3]);
        array.replace(&[6, 7, 8, 9]);
        assert_eq!(array.slice(), vec![6, 7, 8]);

        let array: IntArray = Array::new();
        array.append([1, 2, 3]);
        array.fill(1, 2, 9).expect("overwrite fill");
        assert_eq!(array.slice(), vec![1, 9, 9]);
        array.fill(2, 3, 7).expect("append fill");
        assert_eq!(array.slice(), vec![1, 9, 7, 7, 7]);
        assert!(array.fill(10, 1, 0).is_err());
        assert!(array.fill(-1, 1, 0).is_err());

        let array: IntArray = Array::new();
        array.append([1, 2, 3]);
        array.insert_before(1, &[8]).expect("insert before");
        assert_eq!(array.slice(), vec![1, 8, 2, 3]);
        array.insert_after(2, &[9]).expect("insert after");
        assert_eq!(array.slice(), vec![1, 8, 2, 9, 3]);
        assert!(array.insert_before(9, &[0]).is_err());
        assert!(array.insert_after(-1, &[0]).is_err());

        let array: IntArray = Array::new();
        array.append([1, 2]);
        array.pad(5, 0);
        assert_eq!(array.slice(), vec![1, 2, 0, 0, 0]);
        let array: IntArray = Array::new();
        array.append([1, 2]);
        array.pad(-5, 0);
        assert_eq!(array.slice(), vec![0, 0, 0, 1, 2]);
        array.pad(2, 9);
        assert_eq!(array.slice(), vec![0, 0, 0, 1, 2]);

        let array: IntArray = Array::new();
        array.append([1, 2, 3]);
        array.reverse();
        assert_eq!(array.slice(), vec![3, 2, 1]);

        let array: IntArray = Array::new();
        array.append([1, 2, 3, 4, 5]);
        array.shuffle();
        let mut shuffled = array.slice();
        shuffled.sort_unstable();
        assert_eq!(shuffled, vec![1, 2, 3, 4, 5]);
    }
}
