//! The `database/gdb` chainable model over [`crate::gdb_exec::Db`].
//!
//! Go's `Model` accumulates `Fields`/`Where`/`Order`/`Limit`/`Group` state and
//! executes it through the driver. This port wires the builder
//! ([`crate::gdb_builder::Builder`]) to the SQLite driver ([`crate::gdb_exec::Db`])
//! with a value-chain API (ADR-077):
//!
//! - `Model::new(db, table)` plus `fields`, `where_eq`, `where_in`,
//!   `where_like`, `where_between`, `where_null`, `where_not_null`,
//!   `order_asc`/`order_desc`/`order_random`, `group`, `having`, `limit`,
//!   `page`;
//! - `all`/`one`/`find`/`count`/`value`/`exists` for reads;
//! - `insert`/`update`/`delete` for writes.
//!
//! Not ported: joins, sub-queries, transactions, struct scanning, caching,
//! soft deletes and `Model`-level fields/data mapping from structs.

use gf_core::Value;
use gf_errors::gerror::Error;
use gf_util::gconv;

use crate::gdb::{FromRecord, Record, Result as GfResult, SqlResult};
use crate::gdb_builder::{Builder, WhereKind, WhereOperator};
use crate::gdb_exec::Db;
use std::sync::{Arc, Mutex};

use crate::gdb_exec::FieldInfo;
use gf_errors::gerror;
use gf_os::gtime;

/// A chainable model bound to one table and database.
#[derive(Clone)]
pub struct Model<'a> {
    db: &'a Db,
    builder: Builder,
    time_option: TimeOption,
    cache_ttl: Option<std::time::Duration>,
}

/// One cached select result with its expiry time (ADR-117).
struct CacheEntry {
    created: std::time::Instant,
    rows: GfResult,
}

/// The process-wide model cache.
static MODEL_CACHE: Mutex<Option<std::collections::HashMap<String, CacheEntry>>> = Mutex::new(None);

/// A table-sharding handler: receives the sharding value and returns the
/// concrete table name (ADR-130).
pub type ShardHandler = Arc<dyn Fn(&Value) -> Result<String, Error> + Send + Sync>;

/// Database/table sharding configuration, like GoFrame's `ShardingConfig`
/// subset (ADR-130).
#[derive(Clone, Default)]
pub struct ShardingConfig {
    /// Computes the concrete table name from the sharding value.
    pub table: Option<ShardHandler>,
}

/// Time-field and soft-delete behaviour, like GoFrame's time field options
/// (ADR-110).
#[derive(Clone, Debug)]
pub struct TimeOption {
    /// The creation timestamp field name.
    pub created_at: String,
    /// The update timestamp field name.
    pub updated_at: String,
    /// The soft-delete timestamp field name.
    pub deleted_at: String,
    /// Disables automatic timestamp maintenance when true.
    pub maintain_disabled: bool,
    /// Disables soft-delete filtering/writes when true (`Unscoped`).
    pub soft_delete_disabled: bool,
}

impl Default for TimeOption {
    fn default() -> TimeOption {
        TimeOption {
            created_at: "created_at".to_owned(),
            updated_at: "updated_at".to_owned(),
            deleted_at: "deleted_at".to_owned(),
            maintain_disabled: false,
            soft_delete_disabled: false,
        }
    }
}

impl<'a> Model<'a> {
    /// Creates a model for `table`.
    pub fn new(db: &'a Db, table: &str) -> Model<'a> {
        Model {
            db,
            builder: Builder::new(table).with_dialect(db.dialect()),
            time_option: TimeOption::default(),
            cache_ttl: None,
        }
    }

    /// The table name.
    pub fn table(&self) -> &str {
        &self.builder.table
    }

    /// Selects fields.
    pub fn fields(mut self, fields: &str) -> Model<'a> {
        self.builder = self.builder.fields(fields);
        self
    }

    /// Adds `field = value`.
    pub fn where_eq(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self.builder.where_eq(field, value);
        self
    }

    /// Adds an OR `field = value`.
    pub fn or_where_eq(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self.builder.or_where_eq(field, value);
        self
    }

    /// Adds a condition with `operator`.
    pub fn where_op(mut self, operator: WhereOperator, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, operator, field, value);
        self
    }

    /// Adds `field > value`.
    pub fn where_gt(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, WhereOperator::Gt, field, value);
        self
    }

    /// Adds `field >= value`.
    pub fn where_ge(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, WhereOperator::Gte, field, value);
        self
    }

    /// Adds `field < value`.
    pub fn where_lt(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, WhereOperator::Lt, field, value);
        self
    }

    /// Adds `field <= value`.
    pub fn where_le(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, WhereOperator::Lte, field, value);
        self
    }

    /// Adds `field != value`.
    pub fn where_ne(mut self, field: &str, value: Value) -> Model<'a> {
        self.builder = self
            .builder
            .where_op(WhereKind::And, WhereOperator::Ne, field, value);
        self
    }

    /// Adds `field IN (values)`.
    pub fn where_in(mut self, field: &str, values: Vec<Value>) -> Model<'a> {
        self.builder = self.builder.where_in(field, values);
        self
    }

    /// Adds `field NOT IN (values)`.
    pub fn where_not_in(mut self, field: &str, values: Vec<Value>) -> Model<'a> {
        self.builder = self.builder.where_not_in(field, values);
        self
    }

    /// Adds `field LIKE pattern`.
    pub fn where_like(mut self, field: &str, pattern: &str) -> Model<'a> {
        self.builder = self.builder.where_like(field, pattern);
        self
    }

    /// Adds `field BETWEEN lower AND upper`.
    pub fn where_between(mut self, field: &str, lower: Value, upper: Value) -> Model<'a> {
        self.builder = self.builder.where_between(field, lower, upper);
        self
    }

    /// Adds `field IS NULL`.
    pub fn where_null(mut self, field: &str) -> Model<'a> {
        self.builder = self.builder.where_null(field);
        self
    }

    /// Adds `field IS NOT NULL`.
    pub fn where_not_null(mut self, field: &str) -> Model<'a> {
        self.builder = self.builder.where_not_null(field);
        self
    }

    /// Adds an ascending order.
    pub fn order_asc(mut self, field: &str) -> Model<'a> {
        self.builder = self.builder.order_asc(field);
        self
    }

    /// Adds a descending order.
    pub fn order_desc(mut self, field: &str) -> Model<'a> {
        self.builder = self.builder.order_desc(field);
        self
    }

    /// Adds a random order.
    pub fn order_random(mut self) -> Model<'a> {
        self.builder = self.builder.order_random();
        self
    }

    /// Adds a group-by field.
    pub fn group(mut self, field: &str) -> Model<'a> {
        self.builder = self.builder.group(field);
        self
    }

    /// Sets the having fragment.
    pub fn having(mut self, having: &str) -> Model<'a> {
        self.builder = self.builder.having(having);
        self
    }

    /// Sets the limit and offset.
    pub fn limit(mut self, limit: i64, offset: i64) -> Model<'a> {
        self.builder = self.builder.limit(limit, offset);
        self
    }

    /// Sets 1-based pagination.
    pub fn page(mut self, page: i64, size: i64) -> Model<'a> {
        self.builder = self.builder.page(page, size);
        self
    }

    /// The underlying builder (already configured for the driver dialect).
    pub fn builder(&self) -> &Builder {
        &self.builder
    }

    /// Enables result caching for `ttl`, like GoFrame's `Cache` (ADR-117).
    pub fn cache(mut self, ttl: std::time::Duration) -> Model<'a> {
        self.cache_ttl = Some(ttl);
        self
    }

    /// Applies table sharding for `value`, like GoFrame's `Model.Sharding`
    /// (ADR-130); the configured handler returns the concrete table name.
    pub fn sharding(mut self, value: &Value, config: ShardingConfig) -> Result<Model<'a>, Error> {
        let Some(handler) = config.table else {
            return Ok(self);
        };
        let table = handler(value)?;
        if table.trim().is_empty() {
            return Err(gerror::new("sharding table name should not be empty"));
        }
        self.builder.table = table;
        Ok(self)
    }

    /// Overrides the time-field/soft-delete behaviour (ADR-110).
    pub fn with_time_option(mut self, option: TimeOption) -> Model<'a> {
        self.time_option = option;
        self
    }

    /// The cache key prefix for this model (database + table).
    fn cache_prefix(&self) -> String {
        format!("{:p}|{}|", self.db, self.table())
    }

    /// Reads a cached select result, if still fresh.
    fn cached(&self, key: &str) -> Option<GfResult> {
        let ttl = self.cache_ttl?;
        let cache = MODEL_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.as_ref()?.get(key)?;
        if entry.created.elapsed() > ttl {
            return None;
        }
        Some(entry.rows.clone())
    }

    /// Stores a select result in the cache.
    fn store(&self, key: &str, rows: &GfResult) {
        if self.cache_ttl.is_none() {
            return;
        }
        let mut cache = MODEL_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cache = cache.get_or_insert_with(std::collections::HashMap::new);
        cache.insert(
            key.to_owned(),
            CacheEntry {
                created: std::time::Instant::now(),
                rows: rows.clone(),
            },
        );
    }

    /// Removes the cached results of this model's table.
    fn clear_cache(&self) {
        let prefix = self.cache_prefix();
        let mut cache = MODEL_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cache) = cache.as_mut() {
            cache.retain(|key, _| !key.starts_with(&prefix));
        }
    }

    /// Disables soft-delete filtering and writes, like GoFrame's `Unscoped`.
    pub fn unscoped(mut self) -> Model<'a> {
        self.time_option.soft_delete_disabled = true;
        self
    }

    /// Reports whether the table has `name`.
    fn has_field(&self, name: &str) -> Result<bool, Error> {
        Ok(self
            .db
            .table_fields(self.table())?
            .iter()
            .any(|field| field.name == name))
    }

    /// The model builder with the soft-delete filter applied.
    fn soft_filtered_builder(&self) -> Result<Builder, Error> {
        let mut builder = self.builder.clone();
        if !self.time_option.soft_delete_disabled && self.has_field(&self.time_option.deleted_at)? {
            builder = builder.where_null(&self.time_option.deleted_at);
        }
        Ok(builder)
    }

    /// Adds the automatic timestamps to `data` when the table has the fields.
    fn with_time_data(
        &self,
        data: &[(String, Value)],
        inserting: bool,
    ) -> Result<Vec<(String, Value)>, Error> {
        let mut data = data.to_vec();
        if self.time_option.maintain_disabled {
            return Ok(data);
        }
        let fields: Vec<String> = self
            .db
            .table_fields(self.table())?
            .into_iter()
            .map(|field| field.name)
            .collect();
        let has = |name: &str| fields.iter().any(|field| field == name);
        let now = Value::String(gtime::datetime());
        if inserting
            && has(&self.time_option.created_at)
            && !data_has(&data, &self.time_option.created_at)
        {
            data.push((self.time_option.created_at.clone(), now.clone()));
        }
        if has(&self.time_option.updated_at) && !data_has(&data, &self.time_option.updated_at) {
            data.push((self.time_option.updated_at.clone(), now));
        }
        Ok(data)
    }
    /// Returns all matching rows, like `All`/`Scan`.
    pub fn all(&self) -> Result<GfResult, Error> {
        let builder = self.soft_filtered_builder()?;
        let (sql, args) = builder.select_sql();
        let key = format!("{}{}|{:?}", self.cache_prefix(), sql, args);
        if let Some(rows) = self.cached(&key) {
            return Ok(rows);
        }
        let rows = self.db.query(&sql, &args)?;
        self.store(&key, &rows);
        Ok(rows)
    }

    /// Returns the first matching row, like `One`.
    pub fn one(&self) -> Result<Option<Record>, Error> {
        self.db.find(&self.soft_filtered_builder()?)
    }

    /// Alias of [`Model::one`], like `Find`.
    pub fn find(&self) -> Result<Option<Record>, Error> {
        self.one()
    }

    /// Returns the table fields, like GoFrame's `TableFields` (ADR-109).
    pub fn table_fields(&self) -> Result<Vec<FieldInfo>, Error> {
        self.db.table_fields(self.table())
    }

    /// Scans every matching row into `T`, like `Scan` (ADR-100).
    pub fn scan<T: FromRecord>(&self) -> Result<Vec<T>, Error> {
        self.all()?.iter().map(T::from_record).collect()
    }

    /// Scans the first matching row into `T`, like `Scan` for one row.
    pub fn scan_one<T: FromRecord>(&self) -> Result<Option<T>, Error> {
        self.one()?
            .map(|record| T::from_record(&record))
            .transpose()
    }

    /// Iterates the matching rows in pages of `size`, like GoFrame's
    /// `Chunk` (ADR-123). The handler receives each non-empty page; returning
    /// an error aborts the iteration.
    pub fn chunk<F>(&self, size: i64, mut handler: F) -> Result<(), Error>
    where
        F: FnMut(&GfResult) -> Result<(), Error>,
    {
        if size <= 0 {
            return Err(gerror::new("chunk size should be positive"));
        }
        let mut offset = 0i64;
        loop {
            let mut builder = self.soft_filtered_builder()?;
            builder.limit = size;
            builder.offset = offset;
            let (sql, args) = builder.select_sql();
            let rows = self.db.query(&sql, &args)?;
            if rows.is_empty() {
                break;
            }
            let count = rows.len() as i64;
            handler(&rows)?;
            if count < size {
                break;
            }
            offset += size;
        }
        Ok(())
    }

    /// Returns the aggregate of `field` for the matching rows, like
    /// GoFrame's `Sum`/`Avg`/`Max`/`Min` (ADR-121).
    pub fn aggregate(&self, function: &str, field: &str) -> Result<Value, Error> {
        let mut builder = self.soft_filtered_builder()?;
        let quoted = self.db.dialect().quote_path(field);
        builder.fields = format!("{function}({quoted}) AS gf_agg");
        builder.order_by.clear();
        builder.limit = 0;
        builder.offset = 0;
        let (sql, args) = builder.select_sql();
        let rows = self.db.query(&sql, &args)?;
        Ok(rows
            .into_iter()
            .next()
            .and_then(|record| record.iter().next().map(|(_, value)| value.clone()))
            .unwrap_or(Value::Null))
    }

    /// Returns `SUM(field)`, like `Sum`.
    pub fn sum(&self, field: &str) -> Result<Value, Error> {
        self.aggregate("SUM", field)
    }

    /// Returns `AVG(field)`, like `Avg`.
    pub fn avg(&self, field: &str) -> Result<Value, Error> {
        self.aggregate("AVG", field)
    }

    /// Returns `MAX(field)`, like `Max`.
    pub fn max(&self, field: &str) -> Result<Value, Error> {
        self.aggregate("MAX", field)
    }

    /// Returns `MIN(field)`, like `Min`.
    pub fn min(&self, field: &str) -> Result<Value, Error> {
        self.aggregate("MIN", field)
    }

    /// Returns the number of matching rows, like `Count`.
    ///
    /// Order/limit/offset are ignored, like Go's `Count` for a plain query.
    pub fn count(&self) -> Result<i64, Error> {
        let mut builder = self.soft_filtered_builder()?;
        builder.fields = "COUNT(*) AS gf_count".to_owned();
        builder.order_by.clear();
        builder.limit = 0;
        builder.offset = 0;
        let (sql, args) = builder.select_sql();
        let rows = self.db.query(&sql, &args)?;
        Ok(rows
            .into_iter()
            .next()
            .and_then(|record| {
                record
                    .iter()
                    .next()
                    .map(|(_, value)| gconv::int64_value(value))
            })
            .unwrap_or(0))
    }

    /// Returns one column of the first matching row, like `Value`.
    pub fn value(&self, field: &str) -> Result<Value, Error> {
        let mut builder = self.soft_filtered_builder()?;
        builder.fields = self.db.dialect().quote_path(field);
        builder.limit = 1;
        builder.offset = 0;
        let (sql, args) = builder.select_sql();
        Ok(self
            .db
            .query(&sql, &args)?
            .into_iter()
            .next()
            .and_then(|record| record.get(field).cloned())
            .unwrap_or(Value::Null))
    }

    /// Reports whether any row matches, like `Exist`.
    pub fn exists(&self) -> Result<bool, Error> {
        Ok(self.count()? > 0)
    }

    /// Inserts a derived struct, like GoFrame's `Insert` with a struct
    /// argument (ADR-122).
    pub fn insert_record<T: crate::gdb::ToRecord>(self, record: &T) -> Result<SqlResult, Error> {
        self.insert(&record.to_record())
    }

    /// Updates matching rows from a derived struct (ADR-122).
    pub fn update_record<T: crate::gdb::ToRecord>(self, record: &T) -> Result<SqlResult, Error> {
        self.update(&record.to_record())
    }

    /// Inserts `data` and returns the result with the insert id, like
    /// `Insert`.
    pub fn insert(self, data: &[(String, Value)]) -> Result<SqlResult, Error> {
        let data = self.with_time_data(data, true)?;
        let result = self.db.insert(&self.builder.table, &data)?;
        self.clear_cache();
        Ok(result)
    }

    /// Updates matching rows with `data`, like `Update`.
    pub fn update(self, data: &[(String, Value)]) -> Result<SqlResult, Error> {
        let data = self.with_time_data(data, false)?;
        let result = self.db.update(&self.builder, &data)?;
        self.clear_cache();
        Ok(result)
    }

    /// Deletes matching rows, like `Delete`.
    pub fn delete(self) -> Result<SqlResult, Error> {
        if !self.time_option.soft_delete_disabled && self.has_field(&self.time_option.deleted_at)? {
            let data = vec![(
                self.time_option.deleted_at.clone(),
                Value::String(gtime::datetime()),
            )];
            let result = self.db.update(&self.builder, &data)?;
            self.clear_cache();
            return Ok(result);
        }
        let result = self.db.delete(&self.builder)?;
        self.clear_cache();
        Ok(result)
    }

    /// Runs `f` inside a transaction on the model's database, like
    /// `Transaction`; the closure receives a model with the same builder
    /// state.
    pub fn transaction<T, F>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Model<'_>) -> Result<T, Error>,
    {
        self.db.transaction(|db| {
            let mut model = Model::new(db, &self.builder.table);
            model.builder = self.builder.clone();
            f(&model)
        })
    }
}

/// Converts a [`Record`] into insert/update data.
pub fn record_data(record: &Record) -> Vec<(String, Value)> {
    record
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

/// Reports whether `data` contains `name`.
fn data_has(data: &[(String, Value)], name: &str) -> bool {
    data.iter().any(|(key, _)| key == name)
}

/// Converts a [`Map`](crate::gdb::Map) into insert/update data.
pub fn map_data(map: &crate::gdb::Map) -> Vec<(String, Value)> {
    map.iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gdb_builder::WhereOperator;
    use gf_errors::gerror;

    fn value(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn setup() -> Db {
        let db = Db::open_memory().expect("db");
        db.execute(
            "CREATE TABLE user (id INTEGER PRIMARY KEY, name TEXT, age INTEGER, note TEXT)",
            &[],
        )
        .expect("create");
        for (name, age, note) in [
            ("john", 18, Value::Null),
            ("jane", 30, value("vip")),
            ("bob", 25, Value::Null),
        ] {
            db.insert(
                "user",
                &[
                    ("name".to_owned(), value(name)),
                    ("age".to_owned(), Value::Int(age)),
                    ("note".to_owned(), note),
                ],
            )
            .expect("insert");
        }
        db
    }

    #[test]
    fn select_and_where_chains() {
        let db = setup();
        let rows = Model::new(&db, "user")
            .where_op(WhereOperator::Gt, "age", Value::Int(20))
            .order_asc("age")
            .all()
            .expect("all");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.as_slice()[0].string("name"), "bob");
        assert_eq!(rows.as_slice()[1].string("name"), "jane");

        let rows = Model::new(&db, "user")
            .where_in("name", vec![value("john"), value("bob")])
            .order_desc("name")
            .all()
            .expect("in");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.as_slice()[0].string("name"), "john");

        let rows = Model::new(&db, "user")
            .where_like("name", "j%")
            .all()
            .expect("like");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn fields_and_first_row() {
        let db = setup();
        let record = Model::new(&db, "user")
            .fields("name")
            .where_eq("age", Value::Int(30))
            .one()
            .expect("one")
            .expect("row");
        assert_eq!(record.string("name"), "jane");
        assert!(record.get("age").is_none());
        assert!(
            Model::new(&db, "user")
                .where_eq("name", value("nobody"))
                .find()
                .expect("find")
                .is_none()
        );
    }

    #[test]
    fn count_value_and_exists() {
        let db = setup();
        assert_eq!(Model::new(&db, "user").count().expect("count"), 3);
        assert_eq!(
            Model::new(&db, "user")
                .where_not_null("note")
                .count()
                .expect("count"),
            1
        );
        assert_eq!(
            Model::new(&db, "user")
                .where_eq("name", value("jane"))
                .value("age")
                .expect("value"),
            Value::Int(30)
        );
        assert!(
            Model::new(&db, "user")
                .where_eq("name", value("john"))
                .exists()
                .expect("exists")
        );
        assert!(
            !Model::new(&db, "user")
                .where_eq("name", value("nobody"))
                .exists()
                .expect("exists")
        );
    }

    #[test]
    fn limit_page_and_group() {
        let db = setup();
        let rows = Model::new(&db, "user")
            .order_asc("age")
            .limit(1, 1)
            .all()
            .expect("limit");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.as_slice()[0].string("name"), "bob");

        let rows = Model::new(&db, "user")
            .order_asc("age")
            .page(2, 1)
            .all()
            .expect("page");
        assert_eq!(rows.as_slice()[0].string("name"), "bob");

        let rows = Model::new(&db, "user")
            .fields("age")
            .where_null("note")
            .all()
            .expect("null");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn insert_update_delete() {
        let db = setup();
        let inserted = Model::new(&db, "user")
            .insert(&[
                ("name".to_owned(), value("alice")),
                ("age".to_owned(), Value::Int(22)),
                ("note".to_owned(), Value::Null),
            ])
            .expect("insert");
        assert_eq!(inserted.rows_affected(), 1);
        assert_eq!(inserted.last_insert_id(), 4);
        assert_eq!(
            Model::new(&db, "user")
                .where_eq("name", value("alice"))
                .value("age")
                .expect("value"),
            Value::Int(22)
        );

        let updated = Model::new(&db, "user")
            .where_eq("name", value("alice"))
            .update(&[("age".to_owned(), Value::Int(23))])
            .expect("update");
        assert_eq!(updated.rows_affected(), 1);

        let deleted = Model::new(&db, "user")
            .where_eq("name", value("alice"))
            .delete()
            .expect("delete");
        assert_eq!(deleted.rows_affected(), 1);
        assert_eq!(Model::new(&db, "user").count().expect("count"), 3);
    }

    #[test]
    fn between_and_or_conditions() {
        let db = setup();
        let rows = Model::new(&db, "user")
            .where_between("age", Value::Int(18), Value::Int(26))
            .all()
            .expect("between");
        assert_eq!(rows.len(), 2);
        let rows = Model::new(&db, "user")
            .where_eq("name", value("john"))
            .or_where_eq("age", Value::Int(30))
            .order_asc("name")
            .all()
            .expect("or");
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn model_transaction() {
        let db = setup();
        Model::new(&db, "user")
            .where_eq("name", value("john"))
            .transaction(|transaction| {
                assert_eq!(transaction.table(), "user");
                transaction.clone().insert(&[
                    ("name".to_owned(), value("in-tx")),
                    ("age".to_owned(), Value::Int(40)),
                ])?;
                Ok(())
            })
            .expect("commit");
        assert!(
            Model::new(&db, "user")
                .where_eq("name", value("in-tx"))
                .exists()
                .expect("exists")
        );

        let error = Model::new(&db, "user")
            .transaction(|transaction| -> Result<(), Error> {
                transaction.clone().insert(&[
                    ("name".to_owned(), value("rolled")),
                    ("age".to_owned(), Value::Int(41)),
                ])?;
                Err(gerror::new("abort model transaction"))
            })
            .expect_err("rollback");
        assert!(error.to_string().contains("abort"), "got {error}");
        assert!(
            !Model::new(&db, "user")
                .where_eq("name", value("rolled"))
                .exists()
                .expect("exists")
        );
    }

    #[test]
    fn data_helpers_and_table() {
        let db = setup();
        let record = Model::new(&db, "user").one().expect("one").expect("row");
        let data = record_data(&record);
        assert!(data.iter().any(|(key, _)| key == "name"));
        let mut map = crate::gdb::Map::new();
        map.insert("k".to_owned(), Value::Int(1));
        let data = map_data(&map);
        assert_eq!(data.len(), 1);
        assert_eq!(Model::new(&db, "user").table(), "user");
        assert_eq!(
            Model::new(&db, "user").builder().dialect.quote("x"),
            "\"x\""
        );
    }

    #[test]
    fn model_table_fields() {
        let db = setup();
        let fields = Model::new(&db, "user").table_fields().expect("fields");
        let names: Vec<&str> = fields.iter().map(|field| field.name.as_str()).collect();
        assert_eq!(names, vec!["id", "name", "age", "note"]);
        assert!(fields[0].primary);
        assert!(
            Model::new(&db, "missing")
                .table_fields()
                .expect("missing")
                .is_empty()
        );
    }

    fn setup_articles() -> Db {
        let db = Db::open_memory().expect("db");
        db.execute(
            "CREATE TABLE article (id INTEGER PRIMARY KEY, title TEXT, created_at TEXT, updated_at TEXT, deleted_at TEXT)",
            &[],
        )
        .expect("create");
        db
    }

    fn timestamps(db: &Db, id: i64) -> Record {
        Model::new(db, "article")
            .where_eq("id", Value::Int(id))
            .one()
            .expect("one")
            .expect("row")
    }

    #[test]
    fn insert_and_update_maintain_timestamps() {
        let db = setup_articles();
        let inserted = Model::new(&db, "article")
            .insert(&[("title".to_owned(), value("first"))])
            .expect("insert");
        let id = inserted.insert_id().expect("id");
        let record = timestamps(&db, id);
        assert!(!gconv::string(record.get("created_at").expect("created")).is_empty());
        assert!(!gconv::string(record.get("updated_at").expect("updated")).is_empty());

        // An explicit value is not overwritten.
        let inserted = Model::new(&db, "article")
            .insert(&[
                ("title".to_owned(), value("second")),
                ("created_at".to_owned(), value("2000-01-01 00:00:00")),
            ])
            .expect("insert");
        let id = inserted.insert_id().expect("id");
        assert_eq!(
            gconv::string(timestamps(&db, id).get("created_at").expect("created")),
            "2000-01-01 00:00:00"
        );

        Model::new(&db, "article")
            .where_eq("id", Value::Int(id))
            .update(&[("title".to_owned(), value("updated"))])
            .expect("update");
        let record = timestamps(&db, id);
        assert_eq!(
            gconv::string(record.get("title").expect("title")),
            "updated"
        );
        assert!(!gconv::string(record.get("updated_at").expect("updated")).is_empty());
    }

    #[test]
    fn soft_delete_filters_reads_and_can_be_unscoped() {
        let db = setup_articles();
        for title in ["a", "b"] {
            Model::new(&db, "article")
                .insert(&[("title".to_owned(), value(title))])
                .expect("insert");
        }
        Model::new(&db, "article")
            .where_eq("title", value("a"))
            .delete()
            .expect("soft delete");
        assert_eq!(Model::new(&db, "article").all().expect("all").len(), 1);
        assert_eq!(Model::new(&db, "article").count().expect("count"), 1);
        assert_eq!(
            Model::new(&db, "article")
                .unscoped()
                .all()
                .expect("unscoped")
                .len(),
            2
        );
        assert!(
            !Model::new(&db, "article")
                .where_eq("title", value("a"))
                .exists()
                .expect("exists")
        );
    }

    #[test]
    fn time_option_can_disable_maintenance_and_soft_delete() {
        let db = setup_articles();
        let option = TimeOption {
            maintain_disabled: true,
            soft_delete_disabled: true,
            ..TimeOption::default()
        };
        let inserted = Model::new(&db, "article")
            .with_time_option(option)
            .insert(&[("title".to_owned(), value("plain"))])
            .expect("insert");
        let id = inserted.insert_id().expect("id");
        let record = timestamps(&db, id);
        assert_eq!(record.get("created_at"), Some(&Value::Null));
        assert_eq!(record.get("updated_at"), Some(&Value::Null));

        Model::new(&db, "article")
            .with_time_option(TimeOption {
                maintain_disabled: true,
                soft_delete_disabled: true,
                ..TimeOption::default()
            })
            .where_eq("id", Value::Int(id))
            .delete()
            .expect("hard delete");
        assert_eq!(Model::new(&db, "article").count().expect("count"), 0);
    }

    #[test]
    fn model_cache_reuses_and_invalidates_on_writes() {
        let db = setup();
        let cached = || Model::new(&db, "user").cache(std::time::Duration::from_secs(5));
        assert_eq!(cached().all().expect("all").len(), 3);

        // A direct Db insert bypasses the model, so the cached rows stay.
        db.insert(
            "user",
            &[
                ("name".to_owned(), value("eve")),
                ("age".to_owned(), Value::Int(1)),
            ],
        )
        .expect("insert");
        assert_eq!(cached().all().expect("cached").len(), 3);

        // A model write invalidates the table cache.
        Model::new(&db, "user")
            .cache(std::time::Duration::from_secs(5))
            .where_eq("id", Value::Int(1))
            .update(&[("name".to_owned(), value("x"))])
            .expect("update");
        assert_eq!(cached().all().expect("refreshed").len(), 4);
    }

    #[test]
    fn model_cache_expires() {
        let db = setup();
        let ttl = std::time::Duration::from_millis(10);
        assert_eq!(
            Model::new(&db, "user").cache(ttl).all().expect("all").len(),
            3
        );
        db.insert(
            "user",
            &[
                ("name".to_owned(), value("eve")),
                ("age".to_owned(), Value::Int(1)),
            ],
        )
        .expect("insert");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert_eq!(
            Model::new(&db, "user")
                .cache(ttl)
                .all()
                .expect("after ttl")
                .len(),
            4
        );
    }

    #[test]
    fn model_aggregates() {
        let db = setup();
        let model = Model::new(&db, "user");
        assert_eq!(gconv::int64_value(&model.sum("age").expect("sum")), 73);
        assert!((gconv::float64_value(&model.avg("age").expect("avg")) - 24.333_333).abs() < 0.001);
        assert_eq!(gconv::int64_value(&model.max("age").expect("max")), 30);
        assert_eq!(gconv::int64_value(&model.min("age").expect("min")), 18);

        assert_eq!(
            gconv::int64_value(
                &Model::new(&db, "user")
                    .where_gt("age", Value::Int(20))
                    .sum("age")
                    .expect("filtered")
            ),
            55
        );
        assert_eq!(
            Model::new(&db, "user")
                .where_gt("age", Value::Int(100))
                .sum("age")
                .expect("empty"),
            Value::Null
        );
    }

    #[test]
    fn model_chunk_iterates_pages() {
        let db = setup();
        let mut sizes = Vec::new();
        Model::new(&db, "user")
            .order_asc("id")
            .chunk(2, |page| {
                sizes.push(page.len());
                Ok(())
            })
            .expect("chunk");
        assert_eq!(sizes, vec![2, 1]);

        // An error from the handler stops the iteration.
        let mut visited = 0;
        let error = Model::new(&db, "user")
            .chunk(1, |_page| {
                visited += 1;
                Err(gerror::new("stop"))
            })
            .expect_err("handler error");
        assert_eq!(error.to_string(), "stop");
        assert_eq!(visited, 1);

        assert!(
            Model::new(&db, "user")
                .chunk(0, |_page| Ok(()))
                .expect_err("invalid size")
                .to_string()
                .contains("positive")
        );
    }

    #[test]
    fn model_table_sharding() {
        let db = Db::open_memory().expect("db");
        for suffix in ["2024", "2025"] {
            db.execute(
                &format!("CREATE TABLE user_{suffix} (id INTEGER PRIMARY KEY, name TEXT)"),
                &[],
            )
            .expect("create");
            for name in ["a", "b"] {
                db.insert(
                    &format!("user_{suffix}"),
                    &[("name".to_owned(), value(name))],
                )
                .expect("insert");
            }
        }
        let handler: ShardHandler = Arc::new(|shard| Ok(format!("user_{}", gconv::string(shard))));
        let config = ShardingConfig {
            table: Some(handler),
        };
        let model = Model::new(&db, "user")
            .sharding(&Value::String("2024".to_owned()), config.clone())
            .expect("shard");
        assert_eq!(model.table(), "user_2024");
        assert_eq!(model.all().expect("all").len(), 2);
        assert_eq!(
            Model::new(&db, "user")
                .sharding(&value("2025"), config.clone())
                .expect("shard 2025")
                .table(),
            "user_2025"
        );

        let failing: ShardHandler = Arc::new(|_value| Err(gerror::new("shard failed")));
        let error = match Model::new(&db, "user").sharding(
            &value("x"),
            ShardingConfig {
                table: Some(failing),
            },
        ) {
            Ok(_) => panic!("the sharding handler should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("shard failed"));
        let empty: ShardHandler = Arc::new(|_value| Ok(String::new()));
        let error = match Model::new(&db, "user")
            .sharding(&value("x"), ShardingConfig { table: Some(empty) })
        {
            Ok(_) => panic!("an empty table name should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("empty"));
        // No handler keeps the model unchanged.
        assert_eq!(
            Model::new(&db, "user")
                .sharding(&value("x"), ShardingConfig::default())
                .expect("no shard")
                .table(),
            "user"
        );
    }
}
