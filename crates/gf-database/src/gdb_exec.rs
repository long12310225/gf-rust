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
//! PostgreSQL is supported through `Db::open_postgres` (ADR-187); MySQL is\n//! not ported. Transactions, pooling and `Model` live in their own modules.

use gf_core::Value;
use gf_errors::gerror::{self, Error};
use mysql::prelude::Queryable;
use rusqlite::types::ValueRef;

use crate::gdb::{Record, Result as GfResult, SqlResult};
use crate::gdb_builder::{Builder, Dialect};

/// The execution backend behind [`Db`] (ADR-187).
trait Backend: Send {
    /// Executes a statement and returns the affected-row result.
    fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error>;
    /// Runs a query and returns all rows.
    fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error>;
    /// Inserts one row and returns the result.
    fn insert(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error>;
    /// Reports whether a transaction is open.
    fn in_transaction(&self) -> bool;
    /// The underlying SQLite connection, when this is the SQLite backend.
    fn sqlite_connection(&self) -> Option<&rusqlite::Connection> {
        None
    }
}

/// The SQLite backend.
struct SqliteBackend {
    connection: rusqlite::Connection,
}

impl Backend for SqliteBackend {
    fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        let params = sqlite_params(args)?;
        let affected = self
            .connection
            .execute(sql, rusqlite::params_from_iter(params))
            .map_err(|error| gerror::new(format!("execute SQL failed: {error}")))?;
        Ok(SqlResult::new(affected as i64, None))
    }

    fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error> {
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

    fn insert(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        let params = sqlite_params(args)?;
        let affected = self
            .connection
            .execute(sql, rusqlite::params_from_iter(params))
            .map_err(|error| gerror::new(format!("insert SQL failed: {error}")))?;
        Ok(SqlResult::new(
            affected as i64,
            Some(self.connection.last_insert_rowid()),
        ))
    }

    fn in_transaction(&self) -> bool {
        !self.connection.is_autocommit()
    }

    fn sqlite_connection(&self) -> Option<&rusqlite::Connection> {
        Some(&self.connection)
    }
}

/// The PostgreSQL backend.
struct PostgresBackend {
    client: std::sync::Mutex<postgres::Client>,
}

impl PostgresBackend {
    fn connect(dsn: &str) -> Result<PostgresBackend, Error> {
        let mut config: postgres::Config = dsn
            .parse()
            .map_err(|error| gerror::new(format!("parse PostgreSQL DSN failed: {error}")))?;
        config.connect_timeout(std::time::Duration::from_secs(10));
        let client = config
            .connect(postgres::NoTls)
            .map_err(|error| gerror::new(format!("connect PostgreSQL failed: {error}")))?;
        Ok(PostgresBackend {
            client: std::sync::Mutex::new(client),
        })
    }
}

impl Backend for PostgresBackend {
    fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        let params = postgres_params(args)?;
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|param| param.as_ref()).collect();
        let mut client = self
            .client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let affected = client
            .execute(sql, &refs)
            .map_err(|error| gerror::new(format!("execute SQL failed: {error}")))?;
        Ok(SqlResult::new(affected as i64, None))
    }

    fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error> {
        let params = postgres_params(args)?;
        let refs: Vec<&(dyn postgres::types::ToSql + Sync)> =
            params.iter().map(|param| param.as_ref()).collect();
        let mut client = self
            .client
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let rows = client
            .query(sql, &refs)
            .map_err(|error| gerror::new(format!("query SQL failed: {error}")))?;
        let mut result = GfResult::new();
        for row in rows {
            let mut record = Record::new();
            for (index, column) in row.columns().iter().enumerate() {
                record.insert(column.name(), postgres_value(&row, index, column));
            }
            result.push(record);
        }
        Ok(result)
    }

    fn insert(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        self.execute(sql, args)
    }

    fn in_transaction(&self) -> bool {
        false
    }
}

/// The MySQL backend (ADR-189).
struct MysqlBackend {
    pool: mysql::Pool,
}

impl MysqlBackend {
    fn connect(dsn: &str) -> Result<MysqlBackend, Error> {
        let options = mysql_options(dsn)?;
        let pool = mysql::Pool::new(options)
            .map_err(|error| gerror::new(format!("connect MySQL failed: {error}")))?;
        Ok(MysqlBackend { pool })
    }
}

impl Backend for MysqlBackend {
    fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        let params = mysql_params(args)?;
        let mut connection = self
            .pool
            .get_conn()
            .map_err(|error| gerror::new(format!("get MySQL connection failed: {error}")))?;
        connection
            .exec_drop(sql, params)
            .map_err(|error| gerror::new(format!("execute SQL failed: {error}")))?;
        Ok(SqlResult::new(
            connection.affected_rows() as i64,
            Some(connection.last_insert_id() as i64),
        ))
    }

    fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error> {
        let params = mysql_params(args)?;
        let mut connection = self
            .pool
            .get_conn()
            .map_err(|error| gerror::new(format!("get MySQL connection failed: {error}")))?;
        let rows: Vec<mysql::Row> = connection
            .exec(sql, params)
            .map_err(|error| gerror::new(format!("query SQL failed: {error}")))?;
        let mut result = GfResult::new();
        for row in rows {
            let names: Vec<String> = row
                .columns_ref()
                .iter()
                .map(|column| column.name_str().into_owned())
                .collect();
            let mut record = Record::new();
            for (index, value) in row.unwrap().into_iter().enumerate() {
                if let Some(name) = names.get(index) {
                    record.insert(name, mysql_value(value));
                }
            }
            result.push(record);
        }
        Ok(result)
    }

    fn insert(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        self.execute(sql, args)
    }

    fn in_transaction(&self) -> bool {
        false
    }
}
/// A database handle over one backend (SQLite, PostgreSQL or MySQL).
pub struct Db {
    backend: Box<dyn Backend>,
    dialect: Dialect,
}

impl Db {
    /// Opens (or creates) the SQLite database at `path`.
    pub fn open(path: &str) -> Result<Db, Error> {
        let connection = rusqlite::Connection::open(path).map_err(|error| {
            gerror::new(format!("open SQLite database \"{path}\" failed: {error}"))
        })?;
        Ok(Db {
            backend: Box::new(SqliteBackend { connection }),
            dialect: Dialect::Sqlite,
        })
    }

    /// Opens an in-memory SQLite database.
    pub fn open_memory() -> Result<Db, Error> {
        let connection = rusqlite::Connection::open_in_memory().map_err(|error| {
            gerror::new(format!("open in-memory SQLite database failed: {error}"))
        })?;
        Ok(Db {
            backend: Box::new(SqliteBackend { connection }),
            dialect: Dialect::Sqlite,
        })
    }

    /// Connects to PostgreSQL with a libpq-style DSN, like
    /// `gdb.New(Config{Type: "pgsql"})` (ADR-187).
    pub fn open_postgres(dsn: &str) -> Result<Db, Error> {
        Ok(Db {
            backend: Box::new(PostgresBackend::connect(dsn)?),
            dialect: Dialect::Postgres,
        })
    }

    /// Connects to MySQL with a DSN (`mysql://user:pass@host:port/db` or the
    /// GoFrame form `user:pass@tcp(host:port)/db`), like
    /// `gdb.New(Config{Type: "mysql"})` (ADR-189).
    pub fn open_mysql(dsn: &str) -> Result<Db, Error> {
        Ok(Db {
            backend: Box::new(MysqlBackend::connect(dsn)?),
            dialect: Dialect::MySql,
        })
    }

    /// The builder dialect for this driver.
    pub fn dialect(&self) -> Dialect {
        self.dialect
    }

    /// Executes a statement and returns the affected-row result.
    pub fn execute(&self, sql: &str, args: &[Value]) -> Result<SqlResult, Error> {
        self.backend.execute(sql, args)
    }

    /// Runs a query and returns all rows.
    pub fn query(&self, sql: &str, args: &[Value]) -> Result<GfResult, Error> {
        self.backend.query(sql, args)
    }

    /// Inserts one row and returns the result (with the last insert id on
    /// SQLite; PostgreSQL returns `None` because it has no equivalent).
    pub fn insert(&self, table: &str, data: &[(String, Value)]) -> Result<SqlResult, Error> {
        let (sql, args) = Builder::new(table).insert_sql(data);
        self.backend.insert(&sql, &args)
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

    /// Starts a transaction (`BEGIN`), like `Begin`.
    pub fn begin(&self) -> Result<(), Error> {
        self.execute("BEGIN", &[]).map(|_| ())
    }

    /// Commits the current transaction, like `Commit`.
    pub fn commit(&self) -> Result<(), Error> {
        self.execute("COMMIT", &[]).map(|_| ())
    }

    /// Rolls the current transaction back, like `Rollback`.
    pub fn rollback(&self) -> Result<(), Error> {
        self.execute("ROLLBACK", &[]).map(|_| ())
    }

    /// Reports whether a transaction is open, like `IsTransaction`.
    pub fn is_transaction(&self) -> bool {
        self.backend.in_transaction()
    }

    /// Runs `f` inside a transaction, committing on success and rolling back
    /// on error, like `Transaction`.
    pub fn transaction<T, F>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce(&Db) -> Result<T, Error>,
    {
        self.begin()?;
        match f(self) {
            Ok(value) => {
                self.commit()?;
                Ok(value)
            }
            Err(error) => {
                self.rollback()?;
                Err(error)
            }
        }
    }

    /// The underlying SQLite connection, when this handle uses SQLite.
    pub fn sqlite_connection(&self) -> Option<&rusqlite::Connection> {
        self.backend.sqlite_connection()
    }

    /// Introspects the fields of `table` like GoFrame's `TableFields`; the
    /// SQLite lane reads `PRAGMA table_info` (ADR-109) and the PostgreSQL
    /// lane reads `information_schema` (ADR-187).
    pub fn table_fields(&self, table: &str) -> Result<Vec<FieldInfo>, Error> {
        match self.dialect {
            Dialect::Postgres => return self.postgres_table_fields(table),
            Dialect::MySql => return self.mysql_table_fields(table),
            _ => {}
        }
        let quoted = table.replace('"', "\"\"");
        let result = self.query(&format!("PRAGMA table_info(\"{quoted}\")"), &[])?;
        let mut fields = Vec::new();
        for record in result {
            let name = record
                .get("name")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let type_name = record
                .get("type")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let not_null = record
                .get("notnull")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let primary = record
                .get("pk")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let default = record
                .get("dflt_value")
                .filter(|value| !matches!(value, Value::Null))
                .map(gf_util::gconv::string);
            if name.is_empty() {
                continue;
            }
            fields.push(FieldInfo {
                name,
                type_name,
                nullable: !not_null && !primary,
                primary,
                default,
            });
        }
        Ok(fields)
    }

    /// Introspects a PostgreSQL table through `information_schema`.
    fn postgres_table_fields(&self, table: &str) -> Result<Vec<FieldInfo>, Error> {
        let result = self.query(
            "SELECT c.column_name AS name, c.data_type AS type_name,
                    (c.is_nullable = 'NO')::int AS not_null,
                    EXISTS (
                        SELECT 1
                        FROM information_schema.table_constraints tc
                        JOIN information_schema.key_column_usage kcu
                          ON tc.constraint_name = kcu.constraint_name
                         AND tc.table_schema = kcu.table_schema
                        WHERE tc.constraint_type = 'PRIMARY KEY'
                          AND kcu.table_name = c.table_name
                          AND kcu.column_name = c.column_name
                    )::int AS pk,
                    c.column_default AS dflt_value
             FROM information_schema.columns c
             WHERE c.table_name = $1
             ORDER BY c.ordinal_position",
            &[Value::String(table.to_owned())],
        )?;
        let mut fields = Vec::new();
        for record in result {
            let name = record
                .get("name")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let type_name = record
                .get("type_name")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let not_null = record
                .get("not_null")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let primary = record
                .get("pk")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let default = record
                .get("dflt_value")
                .filter(|value| !matches!(value, Value::Null))
                .map(gf_util::gconv::string);
            if name.is_empty() {
                continue;
            }
            fields.push(FieldInfo {
                name,
                type_name,
                nullable: !not_null && !primary,
                primary,
                default,
            });
        }
        Ok(fields)
    }

    /// Introspects a MySQL table through `information_schema`.
    fn mysql_table_fields(&self, table: &str) -> Result<Vec<FieldInfo>, Error> {
        let result = self.query(
            "SELECT COLUMN_NAME AS name, COLUMN_TYPE AS type_name,
                    (IS_NULLABLE = 'NO') AS not_null,
                    (COLUMN_KEY = 'PRI') AS pk,
                    COLUMN_DEFAULT AS dflt_value
             FROM information_schema.columns
             WHERE TABLE_NAME = ?
             ORDER BY ORDINAL_POSITION",
            &[Value::String(table.to_owned())],
        )?;
        let mut fields = Vec::new();
        for record in result {
            let name = record
                .get("name")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let type_name = record
                .get("type_name")
                .map(gf_util::gconv::string)
                .unwrap_or_default();
            let not_null = record
                .get("not_null")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let primary = record
                .get("pk")
                .map(gf_util::gconv::int64_value)
                .unwrap_or_default()
                != 0;
            let default = record
                .get("dflt_value")
                .filter(|value| !matches!(value, Value::Null))
                .map(gf_util::gconv::string);
            if name.is_empty() {
                continue;
            }
            fields.push(FieldInfo {
                name,
                type_name,
                nullable: !not_null && !primary,
                primary,
                default,
            });
        }
        Ok(fields)
    }
}

/// One table field, like GoFrame's `TableField`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldInfo {
    /// The field name.
    pub name: String,
    /// The declared SQL type (for example `INTEGER`).
    pub type_name: String,
    /// Whether the column accepts `NULL` (primary keys are non-null).
    pub nullable: bool,
    /// Whether the column is part of the primary key.
    pub primary: bool,
    /// The column default, when declared.
    pub default: Option<String>,
}

/// Parses a MySQL DSN: either a `mysql://` URL or the GoFrame form
/// `user:pass@tcp(host:port)/db` (ADR-189).
fn mysql_options(dsn: &str) -> Result<mysql::Opts, Error> {
    if dsn.starts_with("mysql://") || dsn.starts_with("mysqls://") {
        return mysql::Opts::from_url(dsn)
            .map_err(|error| gerror::new(format!("parse MySQL DSN failed: {error}")));
    }
    let (credentials, rest) = dsn
        .split_once('@')
        .ok_or_else(|| gerror::new(format!("invalid MySQL DSN \"{dsn}\": missing '@'")))?;
    let (address, database) = rest
        .split_once('/')
        .ok_or_else(|| gerror::new(format!("invalid MySQL DSN \"{dsn}\": missing database")))?;
    let address = address.trim();
    let address = address
        .strip_prefix("tcp(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or(address);
    let (host, port) = match address.rsplit_once(':') {
        Some((host, port)) => (
            host,
            port.parse::<u16>()
                .map_err(|error| gerror::new(format!("invalid MySQL port: {error}")))?,
        ),
        None => (address, 3306),
    };
    let (user, password) = credentials.split_once(':').unwrap_or((credentials, ""));
    let database = database.split('?').next().unwrap_or(database);
    Ok(mysql::OptsBuilder::new()
        .ip_or_hostname(Some(host.to_owned()))
        .tcp_port(port)
        .user(Some(user.to_owned()))
        .pass(Some(password.to_owned()))
        .db_name(Some(database.to_owned()))
        .into())
}

/// Converts [`Value`] parameters to MySQL parameters (ADR-189).
fn mysql_params(args: &[Value]) -> Result<mysql::Params, Error> {
    let mut values = Vec::with_capacity(args.len());
    for arg in args {
        values.push(match arg {
            Value::Null => mysql::Value::NULL,
            Value::Bool(value) => mysql::Value::Int(i64::from(*value)),
            Value::Int(value) => mysql::Value::Int(*value),
            Value::UInt(value) => mysql::Value::UInt(*value),
            Value::Float(value) => mysql::Value::Double(*value),
            Value::String(value) => mysql::Value::Bytes(value.clone().into_bytes()),
            Value::Bytes(value) => mysql::Value::Bytes(value.clone()),
            Value::List(_) | Value::Map(_) => {
                mysql::Value::Bytes(gf_json::marshal_value(arg)?.into_bytes())
            }
        });
    }
    Ok(mysql::Params::Positional(values))
}

/// Converts one MySQL cell to a [`Value`] (ADR-189).
fn mysql_value(value: mysql::Value) -> Value {
    match value {
        mysql::Value::NULL => Value::Null,
        mysql::Value::Int(value) => Value::Int(value),
        mysql::Value::UInt(value) => Value::UInt(value),
        mysql::Value::Float(value) => Value::Float(f64::from(value)),
        mysql::Value::Double(value) => Value::Float(value),
        mysql::Value::Bytes(bytes) => match String::from_utf8(bytes.clone()) {
            Ok(text) => Value::String(text),
            Err(_) => Value::Bytes(bytes),
        },
        mysql::Value::Date(year, month, day, hour, minute, second, micros) => Value::String(
            format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}.{micros:06}"),
        ),
        mysql::Value::Time(negative, days, hours, minutes, seconds, micros) => {
            let sign = if negative { "-" } else { "" };
            Value::String(format!(
                "{sign}{days:02} {hours:02}:{minutes:02}:{seconds:02}.{micros:06}"
            ))
        }
    }
}
/// Converts [`Value`] parameters to PostgreSQL parameters (ADR-187).
fn postgres_params(args: &[Value]) -> Result<Vec<Box<dyn postgres::types::ToSql + Sync>>, Error> {
    let mut params: Vec<Box<dyn postgres::types::ToSql + Sync>> = Vec::with_capacity(args.len());
    for arg in args {
        let param: Box<dyn postgres::types::ToSql + Sync> = match arg {
            Value::Null => Box::new(Option::<String>::None),
            Value::Bool(value) => Box::new(*value),
            Value::Int(value) => Box::new(*value),
            Value::UInt(value) => {
                let converted = i64::try_from(*value).map_err(|_| {
                    gerror::new(format!("unsigned value {value} does not fit into i64"))
                })?;
                Box::new(converted)
            }
            Value::Float(value) => Box::new(*value),
            Value::String(value) => Box::new(value.clone()),
            Value::Bytes(value) => Box::new(value.clone()),
            Value::List(_) | Value::Map(_) => Box::new(gf_json::marshal_value(arg)?),
        };
        params.push(param);
    }
    Ok(params)
}

/// Converts one PostgreSQL cell to a [`Value`] (ADR-187).
fn postgres_value(row: &postgres::Row, index: usize, column: &postgres::Column) -> Value {
    let value = match column.type_().name() {
        "bool" => row
            .try_get::<_, Option<bool>>(index)
            .ok()
            .flatten()
            .map(Value::Bool),
        "int2" => row
            .try_get::<_, Option<i16>>(index)
            .ok()
            .flatten()
            .map(|value| Value::Int(i64::from(value))),
        "int4" => row
            .try_get::<_, Option<i32>>(index)
            .ok()
            .flatten()
            .map(|value| Value::Int(i64::from(value))),
        "int8" => row
            .try_get::<_, Option<i64>>(index)
            .ok()
            .flatten()
            .map(Value::Int),
        "float4" => row
            .try_get::<_, Option<f32>>(index)
            .ok()
            .flatten()
            .map(|value| Value::Float(f64::from(value))),
        "float8" => row
            .try_get::<_, Option<f64>>(index)
            .ok()
            .flatten()
            .map(Value::Float),
        "bytea" => row
            .try_get::<_, Option<Vec<u8>>>(index)
            .ok()
            .flatten()
            .map(Value::Bytes),
        _ => row
            .try_get::<_, Option<String>>(index)
            .ok()
            .flatten()
            .map(Value::String),
    };
    value.unwrap_or(Value::Null)
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
    fn transactions_commit_and_rollback() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);

        db.transaction(|transaction| {
            assert!(transaction.is_transaction());
            transaction.insert(
                "user",
                &[
                    ("name".to_owned(), value("committed")),
                    ("age".to_owned(), Value::Int(1)),
                ],
            )?;
            Ok(())
        })
        .expect("commit");
        assert!(!db.is_transaction());
        assert_eq!(db.select(&Builder::new("user")).expect("select").len(), 1);

        let error = db
            .transaction(|transaction| -> Result<(), Error> {
                transaction.insert(
                    "user",
                    &[
                        ("name".to_owned(), value("rolled-back")),
                        ("age".to_owned(), Value::Int(2)),
                    ],
                )?;
                Err(gerror::new("abort"))
            })
            .expect_err("rollback");
        assert!(error.to_string().contains("abort"), "got {error}");
        assert_eq!(db.select(&Builder::new("user")).expect("select").len(), 1);
    }

    #[test]
    fn manual_transaction_helpers() {
        let db = Db::open_memory().expect("db");
        create_user_table(&db);
        db.begin().expect("begin");
        db.insert(
            "user",
            &[
                ("name".to_owned(), value("manual")),
                ("age".to_owned(), Value::Int(3)),
            ],
        )
        .expect("insert");
        db.rollback().expect("rollback");
        assert!(!db.is_transaction());
        assert_eq!(db.select(&Builder::new("user")).expect("select").len(), 0);

        db.begin().expect("begin");
        db.insert(
            "user",
            &[
                ("name".to_owned(), value("kept")),
                ("age".to_owned(), Value::Int(4)),
            ],
        )
        .expect("insert");
        db.commit().expect("commit");
        assert_eq!(db.select(&Builder::new("user")).expect("select").len(), 1);
    }

    #[test]
    fn dialect_is_sqlite() {
        let db = Db::open_memory().expect("db");
        assert_eq!(db.dialect(), Dialect::Sqlite);
        assert_eq!(db.dialect().quote("user"), "\"user\"");
        assert_eq!(db.dialect().placeholder(1), "?");
    }

    #[test]
    fn table_fields_introspection() {
        let db = Db::open_memory().expect("db");
        db.execute(
            "CREATE TABLE demo (id INTEGER PRIMARY KEY, name TEXT NOT NULL DEFAULT 'anon', age INTEGER)",
            &[],
        )
        .expect("create");
        let fields = db.table_fields("demo").expect("fields");
        assert_eq!(fields.len(), 3);
        assert_eq!(fields[0].name, "id");
        assert_eq!(fields[0].type_name, "INTEGER");
        assert!(fields[0].primary);
        assert!(!fields[0].nullable);
        assert_eq!(fields[1].name, "name");
        assert!(!fields[1].nullable);
        assert_eq!(fields[1].default.as_deref(), Some("'anon'"));
        assert!(!fields[1].primary);
        assert_eq!(fields[2].name, "age");
        assert!(fields[2].nullable);
        assert_eq!(fields[2].default, None);

        assert!(db.table_fields("missing").expect("missing").is_empty());

        // The table name is quoted, so embedded double quotes are safe.
        db.execute("CREATE TABLE \"we\"\"ird\" (x TEXT)", &[])
            .expect("quoted create");
        let fields = db.table_fields("we\"ird").expect("quoted fields");
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "x");
    }

    #[test]
    fn mysql_dsn_parsing() {
        let options = mysql_options("root:secret@tcp(127.0.0.1:3307)/demo").expect("dsn");
        assert_eq!(options.get_ip_or_hostname(), "127.0.0.1");
        assert_eq!(options.get_tcp_port(), 3307);
        assert_eq!(options.get_user(), Some("root"));
        assert_eq!(options.get_pass(), Some("secret"));
        assert_eq!(options.get_db_name(), Some("demo"));

        let url = mysql_options("mysql://root:secret@127.0.0.1:3306/demo").expect("url");
        assert_eq!(url.get_ip_or_hostname(), "127.0.0.1");
        assert_eq!(url.get_db_name(), Some("demo"));
        assert!(mysql_options("not-a-dsn").is_err());
    }

    #[test]
    fn mysql_open_reports_connection_errors() {
        let error = match Db::open_mysql("gf:gf@tcp(127.0.0.1:1)/gf") {
            Ok(db) => match db.execute("SELECT 1", &[]) {
                Ok(_) => panic!("no MySQL server is listening"),
                Err(error) => error,
            },
            Err(error) => error,
        };
        assert!(
            error.text().contains("MySQL"),
            "unexpected error: {}",
            error.text()
        );
    }

    #[test]
    #[ignore = "requires a live MySQL server in GF_MYSQL_DSN"]
    fn mysql_live_round_trip() {
        let dsn = std::env::var("GF_MYSQL_DSN").expect("GF_MYSQL_DSN");
        let db = Db::open_mysql(&dsn).expect("connect MySQL");
        assert_eq!(db.dialect(), Dialect::MySql);
        db.execute("DROP TABLE IF EXISTS gf_demo", &[])
            .expect("drop");
        db.execute(
            "CREATE TABLE gf_demo (id BIGINT PRIMARY KEY AUTO_INCREMENT, name TEXT NOT NULL, age INT)",
            &[],
        )
        .expect("create");
        let inserted = db
            .insert(
                "gf_demo",
                &[
                    ("name".to_owned(), Value::String("john".to_owned())),
                    ("age".to_owned(), Value::Int(18)),
                ],
            )
            .expect("insert");
        assert_eq!(inserted.rows_affected(), 1);
        let rows = db
            .query(
                "SELECT name, age FROM gf_demo WHERE age = ?",
                &[Value::Int(18)],
            )
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("name"), Some(&Value::String("john".to_owned())));
        assert_eq!(rows[0].get("age"), Some(&Value::Int(18)));
        let fields = db.table_fields("gf_demo").expect("table fields");
        assert!(
            fields
                .iter()
                .any(|field| field.name == "id" && field.primary)
        );
        db.execute("DROP TABLE gf_demo", &[]).expect("cleanup");
    }

    #[test]
    fn postgres_open_reports_connection_errors() {
        let error = match Db::open_postgres("host=127.0.0.1 port=1 user=gf password=gf dbname=gf") {
            Ok(_) => panic!("no PostgreSQL server is listening"),
            Err(error) => error,
        };
        assert!(
            error.text().contains("PostgreSQL"),
            "unexpected error: {}",
            error.text()
        );
    }

    #[test]
    #[ignore = "requires a live PostgreSQL server in GF_POSTGRES_DSN"]
    fn postgres_live_round_trip() {
        let dsn = std::env::var("GF_POSTGRES_DSN").expect("GF_POSTGRES_DSN");
        let db = Db::open_postgres(&dsn).expect("connect PostgreSQL");
        assert_eq!(db.dialect(), Dialect::Postgres);
        assert_eq!(db.dialect().placeholder(1), "$1");
        db.execute("DROP TABLE IF EXISTS gf_demo", &[])
            .expect("drop");
        db.execute(
            "CREATE TABLE gf_demo (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, age INT)",
            &[],
        )
        .expect("create");
        let inserted = db
            .insert(
                "gf_demo",
                &[
                    ("name".to_owned(), Value::String("john".to_owned())),
                    ("age".to_owned(), Value::Int(18)),
                ],
            )
            .expect("insert");
        assert_eq!(inserted.rows_affected(), 1);
        let rows = db
            .query(
                "SELECT name, age FROM gf_demo WHERE age = $1",
                &[Value::Int(18)],
            )
            .expect("query");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].get("name"), Some(&Value::String("john".to_owned())));
        assert_eq!(rows[0].get("age"), Some(&Value::Int(18)));
        let fields = db.table_fields("gf_demo").expect("table fields");
        assert!(
            fields
                .iter()
                .any(|field| field.name == "id" && field.primary)
        );
        db.execute("DROP TABLE gf_demo", &[]).expect("cleanup");
    }
}
