//! The first `database/gdb` execution layer: SQLite over `rusqlite`.
//!
//! The builder ([`crate::gdb_builder`]) renders SQL; this module executes it
//! against a SQLite connection and maps rows to [`Record`]s
//! ([`crate::gdb`]) (ADR-076):
//!
//! - [`Db`] opens a file or in-memory database and exposes `execute`,
//!   `query`, `insert`, `update`, `delete`, `select` and `find`;
//! - `Value` parameters map to SQLite values (maps/lists are JSON-encoded);
//! - rows map back to `Value`s by column type.
//!
//! Not ported: MySQL/PostgreSQL drivers, pooling, transactions, ORM chaining
//! (`Model`) and struct scanning.

use gf_core::Value;
use gf_errors::gerror::{self, Error};
use rusqlite::types::ValueRef;

use crate::gdb::{Record, Result as GfResult, SqlResult};
use crate::gdb_builder::{Builder, Dialect};

/// A SQLite database handle.
pub struct Db {
    connection: rusqlite::Connection,
    dialect: Dialect,
}

impl Db {
    /// Opens (or creates) the SQLite database at `path`.
    pub fn open(path: &str) -> Result<Db, Error> {
        let connection = rusqlite::Connection::open(path).map_err(|error| {
            gerror::new(format!("open SQLite database \"{path}\" failed: {error}"))
        })?;
        Ok(Db {
            connection,
            dialect: Dialect::Sqlite,
        })
    }

    /// Opens an in-memory SQLite database.
    pub fn open_memory() -> Result<Db, Error> {
        let connection = rusqlite::Connection::open_in_memory().map_err(|error| {
            gerror::new(format!("open in-memory SQLite database failed: {error}"))
        })?;
        Ok(Db {
            connection,
            dialect: Dialect::Sqlite,
        })
    }

    /// The builder dialect for this driver.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Executes a statement and returns the affected-row result.
    pub fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        let params = sqlite_params(args)?;
        let affected = self
            .connection
            .execute(sql, rusqlite::params_from_iter(params))
            .map_err(|error| gerror::new(format!("execute SQL failed: {error}")))?;
        Ok(SqlResult::new(affected as i64, None))
    }

    /// Runs a query and returns all rows.
    pub fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error> {
        let params = sqlite_params(args)?;
        let mut statement = self
            .connection
            .prepare(sql)
            .map_err(|error| gerror::new(format!("prepare SQL failed: {error}")))?;
        let names: Vec<String> = statement
            .column_names()
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        let mut rows = statement
            .query(rusqlite::params_from_iter(params))
            .map_err(|error| gerror::new(format!("query SQL failed: {error}")))?;
        let mut result = GfResult::new();
        loop {
            let row = rows
                .next()
                .map_err(|error| gerror::new(format!("read SQL row failed: {error}")))?;
            let Some(row) = row else {
                break;
            };
            let mut record = Record::new();
            for (index, name) in names.iter().enumerate() {
                let value = row.get_ref(index).map_err(|error| {
                    gerror::new(format!("read SQL column \"{name}\" failed: {error}"))
                })?;
                record.insert(name, sqlite_value(value));
            }
            result.push(record);
        }
        Ok(result)
    }

    /// Inserts one row and returns the result (with the last insert id).
    pub fn insert(&self, table: &str, data: &[(String, Value)]) -> Result<SqlResult, Error> {
        let builder = Builder::new(table);
        let (sql, args) = builder.insert_sql(data);
        let params = sqlite_params(&args)?;
        let affected = self
            .connection
            .execute(&sql, rusqlite::params_from_iter(params))
            .map_err(|error| gerror::new(format!("insert SQL failed: {error}")))?;
        Ok(SqlResult::new(
            affected as i64,
            Some(self.connection.last_insert_rowid()),
        ))
    }

    /// Updates rows matching `builder` with `data`.
    pub fn update(&self, builder: &Builder, data: &[(String, Value)]) -> Result<SqlResult, Error> {
        let (sql, args) = builder.update_sql(data);
        self.execute(&sql, &args)
    }

    /// Deletes rows matching `builder`.
    pub fn delete(&self, builder: &Builder) -> Result<SqlResult, Error> {
        let (sql, args) = builder.delete_sql();
        self.execute(&sql, &args)
    }

    /// Selects rows matching `builder`.
    pub fn select(&self, builder: &Builder) -> Result<GfResult, Error> {
        let (sql, args) = builder.select_sql();
        self.query(&sql, &args)
    }

    /// Selects the first row matching `builder`.
    pub fn find(&self, builder: &Builder) -> Result<Option<Record>, Error> {
        let mut builder = builder.clone();
        builder.limit = 1;
        builder.offset = 0;
        let (sql, args) = builder.select_sql();
        Ok(self.query(&sql, &args)?.into_iter().next())
    }

    /// Runs a query and returns the first column of the first row.
    pub fn scalar(&self, sql: &str, args: &[Value]) -> Result<Value, Error> {
        let result = self.query(sql, args)?;
        Ok(result
            .into_iter()
            .next()
            .and_then(|record| record.iter().next().map(|(_, value)| value.clone()))
            .unwrap_or(Value::Null))
    }

    /// The underlying connection (for driver-specific work).
    pub fn connection(&self) -> &rusqlite::Connection {
        &self.connection
    }
}

/// Converts [`Value`] parameters to SQLite values.
fn sqlite_params(args: &[Value]) -> Result<Vec<rusqlite::types::Value>, Error> {
    args.iter().map(sqlite_param).collect()
}

/// Converts one [`Value`] parameter to a SQLite value.
fn sqlite_param(value: &Value) -> Result<rusqlite::types::Value, Error> {
    use rusqlite::types::Value as SqlValue;
    Ok(match value {
        Value::Null => SqlValue::Null,
        Value::Bool(value) => SqlValue::Integer(i64::from(*value)),
        Value::Int(value) => SqlValue::Integer(*value),
        Value::UInt(value) => {
            let converted = i64::try_from(*value).map_err(|_| {
                gerror::new(format!("unsigned value {value} does not fit into i64"))
            })?;
            SqlValue::Integer(converted)
        }
        Value::Float(value) => SqlValue::Real(*value),
        Value::String(value) => SqlValue::Text(value.clone()),
        Value::Bytes(value) => SqlValue::Blob(value.clone()),
        Value::List(_) | Value::Map(_) => {
            let json = gf_json::marshal_value(value)?;
            SqlValue::Text(json)
        }
    })
}

/// Converts a SQLite value reference to a [`Value`].
fn sqlite_value(value: ValueRef<'_>) -> Value {
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(value) => Value::Int(value),
        ValueRef::Real(value) => Value::Float(value),
        ValueRef::Text(value) => Value::String(String::from_utf8_lossy(value).into_owned()),
        ValueRef::Blob(value) => Value::Bytes(value.to_vec()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(text: &str) -> Value {
        Value::String(text.to_owned())
    }

    fn create_user_table(db: &Db) {
        db.execute(
            "CREATE TABLE user (id INTEGER PRIMARY KEY, name TEXT, age INTEGER, score REAL, avatar BLOB, note TEXT)",
            &[],
        )
        .expect("create table");
    }

    fn insert_users(db: &Db) {
        let result = db
            .insert(
                "user",
                &[
                    ("name".to_owned(), value("john")),
                    ("age".to_owned(), Value::Int(18)),
                    ("score".to_owned(), Value::Float(99.5)),
                    ("avatar".to_owned(), Value::Bytes(vec![1, 2, 3])),
                    ("note".to_owned(), Value::Null),
                ],
            )
            .expect("insert");
        assert_eq!(result.rows_affected(), 1);
        assert_eq!(result.last_insert_id(), 1);
        db.insert(
            "user",
            &[
                ("name".to_owned(), value("jane")),
                ("age".to_owned(), Value::Int(30)),
                ("score".to_owned(), Value::Float(88.0)),
                ("avatar".to_owned(), Value::Bytes(Vec::new())),
                ("note".to_owned(), value("vip")),
            ],
        )
        .expect("insert");
    }

    #[test]
    fn insert_and_select_rows() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);
        insert_users(&db);
        let result = db
            .select(&Builder::new("user").order_asc("id"))
            .expect("select");
        assert_eq!(result.len(), 2);
        let first = &result.as_slice()[0];
        assert_eq!(first.string("name"), "john");
        assert_eq!(first.int("age"), 18);
        assert_eq!(first.get("avatar"), Some(&Value::Bytes(vec![1, 2, 3])));
        assert_eq!(first.get("note"), Some(&Value::Null));
        let second = &result.as_slice()[1];
        assert_eq!(second.string("note"), "vip");
    }

    #[test]
    fn builder_where_order_limit() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);
        insert_users(&db);
        let result = db
            .select(
                &Builder::new("user")
                    .where_op(
                        crate::gdb_builder::WhereKind::And,
                        crate::gdb_builder::WhereOperator::Gte,
                        "age",
                        Value::Int(18),
                    )
                    .order_desc("age")
                    .limit(1, 0),
            )
            .expect("select");
        assert_eq!(result.len(), 1);
        assert_eq!(result.as_slice()[0].string("name"), "jane");

        let result = db
            .select(&Builder::new("user").where_like("name", "jo%"))
            .expect("like");
        assert_eq!(result.len(), 1);
        assert_eq!(result.as_slice()[0].string("name"), "john");

        let result = db
            .select(&Builder::new("user").where_in("age", vec![Value::Int(18), Value::Int(30)]))
            .expect("in");
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn find_first_row() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);
        insert_users(&db);
        let record = db
            .find(&Builder::new("user").where_eq("name", value("jane")))
            .expect("find")
            .expect("row");
        assert_eq!(record.int("age"), 30);
        assert!(
            db.find(&Builder::new("user").where_eq("name", value("nobody")))
                .expect("find")
                .is_none()
        );
    }

    #[test]
    fn update_and_delete_rows() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);
        insert_users(&db);
        let updated = db
            .update(
                &Builder::new("user").where_eq("name", value("john")),
                &[("age".to_owned(), Value::Int(19))],
            )
            .expect("update");
        assert_eq!(updated.rows_affected(), 1);
        assert_eq!(
            db.scalar("SELECT age FROM user WHERE name = ?", &[value("john")])
                .expect("scalar"),
            Value::Int(19)
        );
        let deleted = db
            .delete(&Builder::new("user").where_eq("name", value("jane")))
            .expect("delete");
        assert_eq!(deleted.rows_affected(), 1);
        let remaining = db.select(&Builder::new("user")).expect("select");
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining.as_slice()[0].string("name"), "john");
    }

    #[test]
    fn json_parameters_and_scalars() {
        let db = Db::open_memory().expect("db");
        db.execute("CREATE TABLE kv (k TEXT, v TEXT)", &[])
            .expect("create");
        let mut map = indexmap::IndexMap::new();
        map.insert("a".to_owned(), Value::Int(1));
        db.execute(
            "INSERT INTO kv (k, v) VALUES (?, ?)",
            &[value("json"), Value::Map(map)],
        )
        .expect("insert json");
        let stored = db
            .scalar("SELECT v FROM kv WHERE k = ?", &[value("json")])
            .expect("scalar");
        assert_eq!(stored, value(r#"{"a":1}"#));
        assert_eq!(
            db.scalar("SELECT COUNT(*) FROM kv", &[]).expect("count"),
            Value::Int(1)
        );
    }

    #[test]
    fn u64_overflow_is_reported() {
        let db = Db::open_memory().expect("db");
        let error = db
            .execute("CREATE TABLE t (n INTEGER)", &[Value::UInt(u64::MAX)])
            .expect_err("overflow");
        assert!(error.to_string().contains("does not fit"), "got {error}");
    }

    #[test]
    fn invalid_sql_is_reported() {
        let db = Db::open_memory().expect("db");
        assert!(db.execute("NOT SQL", &[]).is_err());
        assert!(db.query("SELECT * FROM missing_table", &[]).is_err());
    }

    #[test]
    fn dialect_is_sqlite() {
        let db = Db::open_memory().expect("db");
        assert_eq!(db.dialect(), Dialect::Sqlite);
        assert_eq!(db.dialect().quote("user"), "\"user\"");
        assert_eq!(db.dialect().placeholder(1), "?");
    }
}
