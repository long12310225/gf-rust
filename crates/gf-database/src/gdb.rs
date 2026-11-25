//! Port of the `database/gdb` value layer (`gdb_type_record.go`,
//! `gdb_type_result.go`, `gdb_result.go`).
//!
//! The Go ORM hands rows around as `Record` (`map[string]*gvar.Var`), `Result`
//! (`[]Record`), `Map`, `List` and `Array`. This port keeps those shapes as
//! newtypes over [`gf_core::Value`] and `IndexMap` (ADR-071):
//!
//! - [`Record`] — an ordered field map with JSON/XML/map conversions;
//! - [`Result`] — a record list with `len`/`size`/`chunk`/`list`/`array`, the
//!   `MapKey*` and `RecordKey*` projections;
//! - [`SqlResult`] — affected rows and last insert id.
//!
//! Not ported yet: `Struct`/`Structs`/`ScanList` (they require runtime struct
//! reflection), the SQL builder, dialects, drivers and the `Model` API.

use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

pub use gf_core::Value;
use gf_errors::gerror;

pub use gf_errors::gerror::Error;
use gf_util::gconv;
use indexmap::IndexMap;

/// A record map, like Go's `Map`/`Record` payload.
pub type Map = IndexMap<String, Value>;

/// A list of records, like Go's `List`.
pub type List = Vec<Map>;

/// A value array, like Go's `Array` (`[]any`).
pub type Array = Vec<Value>;

/// One database row, like GoFrame's `Record`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record(IndexMap<String, Value>);

impl Record {
    /// Creates an empty record.
    pub fn new() -> Record {
        Record::default()
    }

    /// Creates a record from a map.
    pub fn from_map(map: Map) -> Record {
        Record(map)
    }

    /// Inserts a field value, like `Record[key] = value`.
    pub fn insert(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    /// Returns one field value.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.get(key)
    }

    /// Returns one field value as a string.
    pub fn string(&self, key: &str) -> String {
        self.get(key).map(gconv::string).unwrap_or_default()
    }

    /// Returns one field value as an integer.
    pub fn int(&self, key: &str) -> i64 {
        self.get(key).map(gconv::int64_value).unwrap_or_default()
    }

    /// Returns one field value as an unsigned integer.
    pub fn uint(&self, key: &str) -> u64 {
        self.get(key).map(gconv::uint64_value).unwrap_or_default()
    }

    /// Converts the record to a [`Map`], like `Record.Map`.
    pub fn map(&self) -> Map {
        self.0.clone()
    }

    /// Converts the record to JSON, like `Record.Json`.
    pub fn json(&self) -> String {
        gf_json::marshal_value(&Value::Map(self.0.clone())).unwrap_or_default()
    }

    /// Converts the record to XML, like `Record.Xml`.
    pub fn xml(&self) -> String {
        gf_encoding::gjson::Json::new(Value::Map(self.0.clone()))
            .to_xml()
            .unwrap_or_default()
    }

    /// Reports whether the record is empty, like `Record.IsEmpty`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of fields.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Iterates over the fields.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.0.iter()
    }

    /// The underlying map.
    pub fn as_map(&self) -> &Map {
        &self.0
    }

    /// The underlying map, mutably.
    pub fn as_map_mut(&mut self) -> &mut Map {
        &mut self.0
    }
}

impl From<Map> for Record {
    fn from(map: Map) -> Record {
        Record(map)
    }
}

impl From<Record> for Map {
    fn from(record: Record) -> Map {
        record.0
    }
}

impl Deref for Record {
    type Target = IndexMap<String, Value>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Record {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

/// A list of records, like GoFrame's `Result`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Result(Vec<Record>);

impl Result {
    /// Creates an empty result.
    pub fn new() -> Result {
        Result::default()
    }

    /// Creates a result from records.
    pub fn from_records(records: Vec<Record>) -> Result {
        Result(records)
    }

    /// Appends a record.
    pub fn push(&mut self, record: Record) {
        self.0.push(record);
    }

    /// Reports whether the result is empty, like `Result.IsEmpty`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The number of records, like `Result.Len`.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Alias of [`Result::len`], like `Result.Size`.
    pub fn size(&self) -> usize {
        self.len()
    }

    /// Splits the result into chunks, like `Result.Chunk`.
    pub fn chunk(&self, size: usize) -> Vec<Result> {
        if size == 0 {
            return Vec::new();
        }
        self.0
            .chunks(size)
            .map(|chunk| Result(chunk.to_vec()))
            .collect()
    }

    /// Converts the result to JSON, like `Result.Json`.
    pub fn json(&self) -> String {
        gf_json::marshal_value(&Value::List(
            self.0
                .iter()
                .map(|record| Value::Map(record.map()))
                .collect(),
        ))
        .unwrap_or_default()
    }

    /// Converts the result to XML, like `Result.Xml`.
    pub fn xml(&self) -> String {
        gf_encoding::gjson::Json::new(Value::List(
            self.0
                .iter()
                .map(|record| Value::Map(record.map()))
                .collect(),
        ))
        .to_xml()
        .unwrap_or_default()
    }

    /// Converts the result to a [`List`], like `Result.List`.
    pub fn list(&self) -> List {
        self.0.iter().map(Record::map).collect()
    }

    /// Extracts one column, like `Result.Array`.
    ///
    /// Without `field`, the first field name of the first record is used.
    pub fn array(&self, field: Option<&str>) -> Array {
        let key = match field {
            Some(field) if !field.is_empty() => field.to_owned(),
            _ => match self.0.first().and_then(|record| record.keys().next()) {
                Some(key) => key.clone(),
                None => return Vec::new(),
            },
        };
        self.0
            .iter()
            .map(|record| record.get(&key).cloned().unwrap_or(Value::Null))
            .collect()
    }

    /// Groups records by `key` into a `map[string]Value`, like
    /// `Result.MapKeyValue`; duplicate keys produce a list value.
    pub fn map_key_value(&self, key: &str) -> IndexMap<String, Value> {
        let mut grouped: IndexMap<String, Vec<&Record>> = IndexMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                grouped
                    .entry(gconv::string(value))
                    .or_default()
                    .push(record);
            }
        }
        let multi = grouped.values().any(|records| records.len() > 1);
        grouped
            .into_iter()
            .map(|(group_key, records)| {
                let value = if multi {
                    Value::List(
                        records
                            .iter()
                            .map(|record| Value::Map(record.map()))
                            .collect(),
                    )
                } else {
                    Value::Map(records[0].map())
                };
                (group_key, value)
            })
            .collect()
    }

    /// Groups records by a string key, like `Result.MapKeyStr`.
    pub fn map_key_str(&self, key: &str) -> IndexMap<String, Map> {
        let mut result = IndexMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::string(value), record.map());
            }
        }
        result
    }

    /// Groups records by an integer key, like `Result.MapKeyInt`.
    pub fn map_key_int(&self, key: &str) -> HashMap<i64, Map> {
        let mut result = HashMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::int64_value(value), record.map());
            }
        }
        result
    }

    /// Groups records by an unsigned key, like `Result.MapKeyUint`.
    pub fn map_key_uint(&self, key: &str) -> HashMap<u64, Map> {
        let mut result = HashMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::uint64_value(value), record.map());
            }
        }
        result
    }

    /// Groups records by a string key keeping records, like
    /// `Result.RecordKeyStr`.
    pub fn record_key_str(&self, key: &str) -> IndexMap<String, Record> {
        let mut result = IndexMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::string(value), record.clone());
            }
        }
        result
    }

    /// Groups records by an integer key keeping records, like
    /// `Result.RecordKeyInt`.
    pub fn record_key_int(&self, key: &str) -> HashMap<i64, Record> {
        let mut result = HashMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::int64_value(value), record.clone());
            }
        }
        result
    }

    /// Groups records by an unsigned key keeping records, like
    /// `Result.RecordKeyUint`.
    pub fn record_key_uint(&self, key: &str) -> HashMap<u64, Record> {
        let mut result = HashMap::new();
        for record in &self.0 {
            if let Some(value) = record.get(key) {
                result.insert(gconv::uint64_value(value), record.clone());
            }
        }
        result
    }

    /// Iterates over the records.
    pub fn iter(&self) -> impl Iterator<Item = &Record> {
        self.0.iter()
    }

    /// The underlying records.
    pub fn as_slice(&self) -> &[Record] {
        &self.0
    }
}

impl From<Vec<Record>> for Result {
    fn from(records: Vec<Record>) -> Result {
        Result(records)
    }
}

impl Deref for Result {
    type Target = Vec<Record>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for Result {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl IntoIterator for Result {
    type Item = Record;
    type IntoIter = std::vec::IntoIter<Record>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Converts one [`Value`] into a typed field, the counterpart of GoFrame's
/// struct scanning (per-value `gconv` coercion). Usually used through
/// `#[derive(FromRecord)]` (ADR-100).
pub trait FieldFromValue: Sized {
    /// Converts the optional column value.
    fn field_from_value(value: Option<&Value>) -> std::result::Result<Self, Error>;
}

impl FieldFromValue for Value {
    fn field_from_value(value: Option<&Value>) -> std::result::Result<Value, Error> {
        Ok(value.cloned().unwrap_or(Value::Null))
    }
}

impl FieldFromValue for String {
    fn field_from_value(value: Option<&Value>) -> std::result::Result<String, Error> {
        Ok(value.map(gconv::string).unwrap_or_default())
    }
}

impl FieldFromValue for bool {
    fn field_from_value(value: Option<&Value>) -> std::result::Result<bool, Error> {
        Ok(value.map(gconv::bool_value).unwrap_or(false))
    }
}

macro_rules! impl_field_from_int {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldFromValue for $type {
                fn field_from_value(value: Option<&Value>) -> std::result::Result<Self, Error> {
                    Ok(value.map(gconv::int64_value).unwrap_or_default() as $type)
                }
            }
        )*
    };
}

impl_field_from_int!(i8, i16, i32, i64, isize);

macro_rules! impl_field_from_uint {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldFromValue for $type {
                fn field_from_value(value: Option<&Value>) -> std::result::Result<Self, Error> {
                    Ok(value.map(gconv::uint64_value).unwrap_or_default() as $type)
                }
            }
        )*
    };
}

impl_field_from_uint!(u8, u16, u32, u64, usize);

macro_rules! impl_field_from_float {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldFromValue for $type {
                fn field_from_value(value: Option<&Value>) -> std::result::Result<Self, Error> {
                    Ok(value.map(gconv::float64_value).unwrap_or_default() as $type)
                }
            }
        )*
    };
}

impl_field_from_float!(f32, f64);

impl<T: FieldFromValue> FieldFromValue for Option<T> {
    fn field_from_value(value: Option<&Value>) -> std::result::Result<Option<T>, Error> {
        match value {
            None | Some(Value::Null) => Ok(None),
            Some(value) => T::field_from_value(Some(value)).map(Some),
        }
    }
}

impl FieldFromValue for Vec<String> {
    fn field_from_value(value: Option<&Value>) -> std::result::Result<Vec<String>, Error> {
        match value {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::List(items)) => Ok(items.iter().map(gconv::string).collect()),
            Some(other) => Ok(vec![gconv::string(other)]),
        }
    }
}

/// A row-shaped struct, like GoFrame's struct scanning; usually derived with
/// `#[derive(FromRecord)]` (ADR-100).
pub trait FromRecord: Sized {
    /// Builds the struct from one record.
    fn from_record(record: &Record) -> std::result::Result<Self, Error>;
}

/// Converts one Rust value into a column [`Value`], used by
/// `#[derive(ToRecord)]` (ADR-122).
pub trait ToColumnValue {
    /// Converts `self` for an INSERT/UPDATE.
    fn to_column_value(&self) -> Value;
}

impl ToColumnValue for Value {
    fn to_column_value(&self) -> Value {
        self.clone()
    }
}

impl ToColumnValue for String {
    fn to_column_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl ToColumnValue for str {
    fn to_column_value(&self) -> Value {
        Value::String(self.to_owned())
    }
}

impl ToColumnValue for bool {
    fn to_column_value(&self) -> Value {
        Value::Bool(*self)
    }
}

macro_rules! impl_to_column_int {
    ($($type:ty),* $(,)?) => {
        $(
            impl ToColumnValue for $type {
                fn to_column_value(&self) -> Value {
                    Value::Int(*self as i64)
                }
            }
        )*
    };
}

impl_to_column_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

macro_rules! impl_to_column_float {
    ($($type:ty),* $(,)?) => {
        $(
            impl ToColumnValue for $type {
                fn to_column_value(&self) -> Value {
                    Value::Float(*self as f64)
                }
            }
        )*
    };
}

impl_to_column_float!(f32, f64);

impl<T: ToColumnValue> ToColumnValue for Option<T> {
    fn to_column_value(&self) -> Value {
        match self {
            Some(value) => value.to_column_value(),
            None => Value::Null,
        }
    }
}

impl ToColumnValue for Vec<String> {
    fn to_column_value(&self) -> Value {
        Value::List(self.iter().cloned().map(Value::String).collect())
    }
}

/// A struct that maps to INSERT/UPDATE data, usually derived with
/// `#[derive(ToRecord)]` (ADR-122).
pub trait ToRecord {
    /// Builds the `(column, value)` pairs for a write.
    fn to_record(&self) -> Vec<(String, Value)>;
}

/// The execution result of a SQL statement, like GoFrame's `SqlResult`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SqlResult {
    /// The affected row count (overrides the driver value when positive).
    pub affected: i64,
    /// The last insert id.
    pub last_insert_id: Option<i64>,
}

impl SqlResult {
    /// Creates a result with `affected` rows and an optional insert id.
    pub fn new(affected: i64, last_insert_id: Option<i64>) -> SqlResult {
        SqlResult {
            affected,
            last_insert_id,
        }
    }

    /// Returns the affected row count, like `RowsAffected`.
    pub fn rows_affected(&self) -> i64 {
        self.affected.max(0)
    }

    /// Returns the last insert id, like `LastInsertId`.
    pub fn last_insert_id(&self) -> i64 {
        self.last_insert_id.unwrap_or_default()
    }

    /// Like [`SqlResult::rows_affected`], but panics on error (Go parity).
    pub fn must_get_affected(&self) -> i64 {
        self.rows_affected()
    }

    /// Like [`SqlResult::last_insert_id`], but panics on error (Go parity).
    pub fn must_get_insert_id(&self) -> i64 {
        self.last_insert_id()
    }

    /// Converts a missing insert id into an error, like Go's driver error path.
    pub fn insert_id(&self) -> std::result::Result<i64, Error> {
        match self.last_insert_id {
            Some(value) => Ok(value),
            None => Err(gerror::new("last insert id is not available")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: i64, name: &str) -> Record {
        let mut record = Record::new();
        record.insert("id", Value::Int(id));
        record.insert("name", Value::String(name.to_owned()));
        record
    }

    fn result() -> Result {
        Result::from_records(vec![record(1, "a"), record(2, "b"), record(3, "c")])
    }

    #[test]
    fn record_conversions() {
        let record = record(1, "john");
        assert!(!record.is_empty());
        assert_eq!(record.len(), 2);
        assert_eq!(record.string("name"), "john");
        assert_eq!(record.int("id"), 1);
        assert_eq!(record.uint("id"), 1);
        assert_eq!(record.map().len(), 2);
        let json = record.json();
        assert!(json.contains("\"id\":1"), "{json}");
        assert!(json.contains("\"name\":\"john\""), "{json}");
        let xml = record.xml();
        assert!(!xml.is_empty());
        assert!(Record::new().is_empty());
    }

    #[test]
    fn result_basics() {
        let result = result();
        assert_eq!(result.len(), 3);
        assert_eq!(result.size(), 3);
        assert!(!result.is_empty());
        assert_eq!(result.list().len(), 3);
        let json = result.json();
        assert!(json.starts_with('['), "{json}");
        assert!(json.contains("\"name\":\"b\""), "{json}");
        assert!(Result::new().is_empty());
    }

    #[test]
    fn result_chunking() {
        let result = result();
        let chunks = result.chunk(2);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].len(), 2);
        assert_eq!(chunks[1].len(), 1);
        assert!(result.chunk(0).is_empty());
        assert_eq!(result.chunk(10).len(), 1);
    }

    #[test]
    fn result_array_projection() {
        let result = result();
        assert_eq!(
            result.array(Some("name")),
            vec![
                Value::String("a".to_owned()),
                Value::String("b".to_owned()),
                Value::String("c".to_owned()),
            ]
        );
        // Default field: the first key of the first record (id).
        assert_eq!(
            result.array(None),
            vec![Value::Int(1), Value::Int(2), Value::Int(3)]
        );
        assert!(Result::new().array(None).is_empty());
    }

    #[test]
    fn result_grouping() {
        let records = vec![record(1, "a"), record(1, "b"), record(2, "c")];
        let result = Result::from_records(records);
        let grouped = result.map_key_value("id");
        assert_eq!(grouped.len(), 2);
        match &grouped["1"] {
            Value::List(items) => assert_eq!(items.len(), 2),
            other => panic!("expected list, got {other:?}"),
        }
        let single = Result::from_records(vec![record(1, "a")]);
        match &single.map_key_value("id")["1"] {
            Value::Map(map) => assert_eq!(map["name"], Value::String("a".to_owned())),
            other => panic!("expected map, got {other:?}"),
        }
        assert_eq!(result.map_key_str("id").len(), 2);
        assert_eq!(result.map_key_int("id").len(), 2);
        assert_eq!(result.map_key_uint("id").len(), 2);
        assert_eq!(result.record_key_str("name").len(), 3);
        assert_eq!(result.record_key_int("id").len(), 2);
        assert_eq!(result.record_key_uint("id").len(), 2);
        assert_eq!(result.record_key_str("name")["b"].string("name"), "b");
    }

    #[test]
    fn sql_result_helpers() {
        let result = SqlResult::new(3, Some(10));
        assert_eq!(result.rows_affected(), 3);
        assert_eq!(result.last_insert_id(), 10);
        assert_eq!(result.must_get_affected(), 3);
        assert_eq!(result.must_get_insert_id(), 10);
        let result = SqlResult::new(0, None);
        assert_eq!(result.rows_affected(), 0);
        assert_eq!(result.last_insert_id(), 0);
        assert!(result.insert_id().is_err());
        assert_eq!(SqlResult::new(0, Some(7)).insert_id().expect("id"), 7);
    }
}
