//! Port of the `database/gdb` clause builder (`gdb_model_builder*.go`,
//! `gdb_model_where*.go`, `gdb_model_order_group.go`, `gdb_model_option.go`).
//!
//! Go's builder works on `any` where parameters (strings, maps, structs) and
//! delegates quoting/placeholder formatting to the active driver. This port
//! keeps the holder model and renders SQL for a small dialect enum using
//! [`gf_core::Value`] parameters (ADR-072):
//!
//! - [`WhereHolder`]/[`WhereOperator`] — `=`, `!=`, `<`, `<=`, `>`, `>=`,
//!   `IN`, `NOT IN`, `LIKE`, `NOT LIKE`, `BETWEEN`, `NOT BETWEEN`, `IS NULL`,
//!   `IS NOT NULL` and raw fragments;
//! - [`Builder`] — where/order/group/having/limit/offset/page/fields plus
//!   select/insert/update/delete rendering;
//! - [`Dialect`] — MySQL (backtick + `?`) and PostgreSQL (double quote + `$n`).
//!
//! Not ported: driver execution, `Model` chaining, struct/map where parsing
//! (Rust maps are passed as [`Value`]), sub-queries, joins and dialect-specific
//! upserts.

use gf_core::Value;

use gf_util::gconv;

/// A SQL dialect for quoting and placeholders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Dialect {
    /// MySQL: `` `name` `` and `?`.
    #[default]
    MySql,
    /// PostgreSQL: `"name"` and `$n`.
    Postgres,
    /// SQLite: `"name"` and `?`.
    Sqlite,
}

impl Dialect {
    /// Quotes one identifier.
    pub fn quote(&self, name: &str) -> String {
        match self {
            Dialect::MySql => format!("`{name}`"),
            Dialect::Postgres | Dialect::Sqlite => format!("\"{name}\""),
        }
    }

    /// Quotes a possibly dotted path, leaving expressions untouched.
    pub fn quote_path(&self, path: &str) -> String {
        if path.is_empty() || path == "*" || path.contains('(') || path.contains(' ') {
            return path.to_owned();
        }
        path.split('.')
            .map(|part| self.quote(part))
            .collect::<Vec<String>>()
            .join(".")
    }

    /// Returns the next placeholder for `index` (1-based).
    pub fn placeholder(&self, index: usize) -> String {
        match self {
            Dialect::MySql | Dialect::Sqlite => "?".to_owned(),
            Dialect::Postgres => format!("${index}"),
        }
    }

    /// The random-order expression.
    pub fn random(&self) -> &'static str {
        match self {
            Dialect::MySql => "RAND()",
            Dialect::Postgres | Dialect::Sqlite => "RANDOM()",
        }
    }
}

/// The logical connector of a where holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhereKind {
    /// AND.
    And,
    /// OR.
    Or,
}

/// The comparison operators supported by the builder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WhereOperator {
    /// `=`.
    Eq,
    /// `!=`.
    Ne,
    /// `<`.
    Lt,
    /// `<=`.
    Lte,
    /// `>`.
    Gt,
    /// `>=`.
    Gte,
    /// `IN (...)`.
    In,
    /// `NOT IN (...)`.
    NotIn,
    /// `LIKE`.
    Like,
    /// `NOT LIKE`.
    NotLike,
    /// `BETWEEN ... AND ...`.
    Between,
    /// `NOT BETWEEN ... AND ...`.
    NotBetween,
    /// `IS NULL`.
    Null,
    /// `IS NOT NULL`.
    NotNull,
    /// A raw SQL fragment.
    Raw,
}

/// One where condition holder, like GoFrame's `WhereHolder`.
#[derive(Clone, Debug)]
pub struct WhereHolder {
    /// The connector.
    pub kind: WhereKind,
    /// The operator.
    pub operator: WhereOperator,
    /// The field path.
    pub field: String,
    /// The first value (or value list for IN).
    pub value: Value,
    /// The second value (BETWEEN upper bound).
    pub upper: Value,
    /// A raw fragment for [`WhereOperator::Raw`].
    pub raw: String,
    /// A field prefix such as `u.`.
    pub prefix: String,
    /// Whether nil values are skipped.
    pub omit_nil: bool,
    /// Whether empty values are skipped.
    pub omit_empty: bool,
}

impl WhereHolder {
    fn new(kind: WhereKind, operator: WhereOperator, field: &str) -> WhereHolder {
        WhereHolder {
            kind,
            operator,
            field: field.to_owned(),
            value: Value::Null,
            upper: Value::Null,
            raw: String::new(),
            prefix: String::new(),
            omit_nil: false,
            omit_empty: false,
        }
    }

    /// Reports whether a value should be skipped by the omit options.
    fn skipped(&self) -> bool {
        if self.operator == WhereOperator::Raw
            || matches!(self.operator, WhereOperator::Null | WhereOperator::NotNull)
        {
            return false;
        }
        if self.omit_nil && matches!(self.value, Value::Null) {
            return true;
        }
        if self.omit_empty && is_empty_value(&self.value) {
            return true;
        }
        false
    }
}

/// Reports whether a value is "empty" for `omit_empty`.
fn is_empty_value(value: &Value) -> bool {
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

/// The supported join kinds, like GoFrame's `LeftJoin`/`RightJoin`/`Join`
/// (ADR-147).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinKind {
    /// `INNER JOIN`.
    Inner,
    /// `LEFT JOIN`.
    Left,
    /// `RIGHT JOIN`.
    Right,
}

impl JoinKind {
    /// The SQL keywords.
    pub fn sql(self) -> &'static str {
        match self {
            JoinKind::Inner => "INNER JOIN",
            JoinKind::Left => "LEFT JOIN",
            JoinKind::Right => "RIGHT JOIN",
        }
    }
}

/// One join clause: a kind, a table and a raw `ON` condition (ADR-147).
#[derive(Clone, Debug)]
pub struct JoinClause {
    /// The join kind.
    pub kind: JoinKind,
    /// The joined table.
    pub table: String,
    /// The raw `ON` condition.
    pub on: String,
}

/// The SQL clause builder, like GoFrame's `WhereBuilder` plus the
/// order/group/limit state of `Model`.
#[derive(Clone, Debug)]
pub struct Builder {
    /// The dialect.
    pub dialect: Dialect,
    /// The table name.
    pub table: String,
    /// The selected fields (`*` by default).
    pub fields: String,
    /// Whether `SELECT DISTINCT` is used (ADR-160).
    pub distinct: bool,
    /// The join clauses (ADR-147).
    pub joins: Vec<JoinClause>,
    /// The where holders.
    pub where_holders: Vec<WhereHolder>,
    /// The order-by clauses.
    pub order_by: Vec<String>,
    /// The group-by fields.
    pub group_by: Vec<String>,
    /// The having fragment.
    pub having: String,
    /// The limit; zero or negative means none.
    pub limit: i64,
    /// The offset.
    pub offset: i64,
}

impl Builder {
    /// Creates a builder for `table`.
    pub fn new(table: &str) -> Builder {
        Builder {
            dialect: Dialect::MySql,
            table: table.to_owned(),
            fields: "*".to_owned(),
            distinct: false,
            joins: Vec::new(),
            where_holders: Vec::new(),
            order_by: Vec::new(),
            group_by: Vec::new(),
            having: String::new(),
            limit: 0,
            offset: 0,
        }
    }

    /// Enables `SELECT DISTINCT`, like GoFrame's `Distinct` (ADR-160).
    pub fn distinct(mut self) -> Builder {
        self.distinct = true;
        self
    }

    /// Adds a join clause (ADR-147).
    pub fn join(mut self, kind: JoinKind, table: &str, on: &str) -> Builder {
        self.joins.push(JoinClause {
            kind,
            table: table.to_owned(),
            on: on.to_owned(),
        });
        self
    }

    /// Adds a `LEFT JOIN`.
    pub fn left_join(self, table: &str, on: &str) -> Builder {
        self.join(JoinKind::Left, table, on)
    }

    /// Adds a `RIGHT JOIN`.
    pub fn right_join(self, table: &str, on: &str) -> Builder {
        self.join(JoinKind::Right, table, on)
    }

    /// Adds an `INNER JOIN`.
    pub fn inner_join(self, table: &str, on: &str) -> Builder {
        self.join(JoinKind::Inner, table, on)
    }

    /// Uses `dialect`.
    pub fn with_dialect(mut self, dialect: Dialect) -> Builder {
        self.dialect = dialect;
        self
    }

    /// Sets the field prefix applied to new holders, like `WherePrefix`.
    pub fn prefix(mut self, prefix: &str) -> Builder {
        for holder in &mut self.where_holders {
            if holder.prefix.is_empty() {
                holder.prefix = prefix.to_owned();
            }
        }
        self
    }

    /// Selects fields.
    pub fn fields(mut self, fields: &str) -> Builder {
        self.fields = fields.to_owned();
        self
    }

    /// Adds one AND condition.
    pub fn where_eq(mut self, field: &str, value: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::Eq, field);
        holder.value = value;
        self.where_holders.push(holder);
        self
    }

    /// Adds one OR condition.
    pub fn or_where_eq(mut self, field: &str, value: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::Or, WhereOperator::Eq, field);
        holder.value = value;
        self.where_holders.push(holder);
        self
    }

    /// Adds one condition with `operator`.
    pub fn where_op(
        mut self,
        kind: WhereKind,
        operator: WhereOperator,
        field: &str,
        value: Value,
    ) -> Builder {
        let mut holder = WhereHolder::new(kind, operator, field);
        holder.value = value;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field IN (values)`.
    pub fn where_in(mut self, field: &str, values: Vec<Value>) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::In, field);
        holder.value = Value::List(values);
        self.where_holders.push(holder);
        self
    }

    /// Adds `field NOT IN (values)`.
    pub fn where_not_in(mut self, field: &str, values: Vec<Value>) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::NotIn, field);
        holder.value = Value::List(values);
        self.where_holders.push(holder);
        self
    }

    /// Adds `field LIKE pattern`.
    pub fn where_like(self, field: &str, pattern: &str) -> Builder {
        self.where_op(
            WhereKind::And,
            WhereOperator::Like,
            field,
            Value::String(pattern.to_owned()),
        )
    }

    /// Adds `field NOT LIKE pattern`.
    pub fn where_not_like(self, field: &str, pattern: &str) -> Builder {
        self.where_op(
            WhereKind::And,
            WhereOperator::NotLike,
            field,
            Value::String(pattern.to_owned()),
        )
    }

    /// Adds `field BETWEEN lower AND upper`.
    pub fn where_between(mut self, field: &str, lower: Value, upper: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::Between, field);
        holder.value = lower;
        holder.upper = upper;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field IS NULL`.
    pub fn where_null(mut self, field: &str) -> Builder {
        self.where_holders
            .push(WhereHolder::new(WhereKind::And, WhereOperator::Null, field));
        self
    }

    /// Adds `field IS NOT NULL`.
    pub fn where_not_null(mut self, field: &str) -> Builder {
        self.where_holders.push(WhereHolder::new(
            WhereKind::And,
            WhereOperator::NotNull,
            field,
        ));
        self
    }

    /// Adds `field LIKE 'prefix%'`, like `WherePrefix` (ADR-161).
    pub fn where_prefix(self, field: &str, prefix: &str) -> Builder {
        self.where_op(
            WhereKind::And,
            WhereOperator::Like,
            field,
            Value::String(format!("{prefix}%")),
        )
    }

    /// Adds `field LIKE '%suffix'`, like `WhereSuffix` (ADR-161).
    pub fn where_suffix(self, field: &str, suffix: &str) -> Builder {
        self.where_op(
            WhereKind::And,
            WhereOperator::Like,
            field,
            Value::String(format!("%{suffix}")),
        )
    }

    /// Adds `field LIKE 'prefix%'` as an `OR` condition.
    pub fn or_where_prefix(self, field: &str, prefix: &str) -> Builder {
        self.or_where_op(
            WhereOperator::Like,
            field,
            Value::String(format!("{prefix}%")),
        )
    }

    /// Adds `field LIKE '%suffix'` as an `OR` condition.
    pub fn or_where_suffix(self, field: &str, suffix: &str) -> Builder {
        self.or_where_op(
            WhereOperator::Like,
            field,
            Value::String(format!("%{suffix}")),
        )
    }

    /// Adds a raw AND fragment.
    pub fn where_raw(mut self, raw: &str) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::Raw, "");
        holder.raw = raw.to_owned();
        self.where_holders.push(holder);
        self
    }

    /// Adds a raw OR fragment.
    pub fn or_where_raw(mut self, raw: &str) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::Or, WhereOperator::Raw, "");
        holder.raw = raw.to_owned();
        self.where_holders.push(holder);
        self
    }

    /// Adds one `OR` condition with `operator` (ADR-142).
    pub fn or_where_op(mut self, operator: WhereOperator, field: &str, value: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::Or, operator, field);
        holder.value = value;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field NOT BETWEEN lower AND upper`.
    pub fn where_not_between(mut self, field: &str, lower: Value, upper: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::NotBetween, field);
        holder.value = lower;
        holder.upper = upper;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field IN (values)` as an `OR` condition.
    pub fn or_where_in(self, field: &str, values: Vec<Value>) -> Builder {
        self.or_where_op(WhereOperator::In, field, Value::List(values))
    }

    /// Adds `field NOT IN (values)` as an `OR` condition.
    pub fn or_where_not_in(self, field: &str, values: Vec<Value>) -> Builder {
        self.or_where_op(WhereOperator::NotIn, field, Value::List(values))
    }

    /// Adds `field LIKE pattern` as an `OR` condition.
    pub fn or_where_like(self, field: &str, pattern: &str) -> Builder {
        self.or_where_op(
            WhereOperator::Like,
            field,
            Value::String(pattern.to_owned()),
        )
    }

    /// Adds `field NOT LIKE pattern` as an `OR` condition.
    pub fn or_where_not_like(self, field: &str, pattern: &str) -> Builder {
        self.or_where_op(
            WhereOperator::NotLike,
            field,
            Value::String(pattern.to_owned()),
        )
    }

    /// Adds `field BETWEEN lower AND upper` as an `OR` condition.
    pub fn or_where_between(mut self, field: &str, lower: Value, upper: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::Or, WhereOperator::Between, field);
        holder.value = lower;
        holder.upper = upper;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field NOT BETWEEN lower AND upper` as an `OR` condition.
    pub fn or_where_not_between(mut self, field: &str, lower: Value, upper: Value) -> Builder {
        let mut holder = WhereHolder::new(WhereKind::Or, WhereOperator::NotBetween, field);
        holder.value = lower;
        holder.upper = upper;
        self.where_holders.push(holder);
        self
    }

    /// Adds `field IS NULL` as an `OR` condition.
    pub fn or_where_null(mut self, field: &str) -> Builder {
        self.where_holders
            .push(WhereHolder::new(WhereKind::Or, WhereOperator::Null, field));
        self
    }

    /// Adds `field IS NOT NULL` as an `OR` condition.
    pub fn or_where_not_null(mut self, field: &str) -> Builder {
        self.where_holders.push(WhereHolder::new(
            WhereKind::Or,
            WhereOperator::NotNull,
            field,
        ));
        self
    }

    /// Adds an ascending order.
    pub fn order_asc(mut self, field: &str) -> Builder {
        self.order_by
            .push(format!("{} ASC", self.dialect.quote_path(field)));
        self
    }

    /// Adds a descending order.
    pub fn order_desc(mut self, field: &str) -> Builder {
        self.order_by
            .push(format!("{} DESC", self.dialect.quote_path(field)));
        self
    }

    /// Adds a random order.
    pub fn order_random(mut self) -> Builder {
        self.order_by.push(self.dialect.random().to_owned());
        self
    }

    /// Adds a group-by field.
    pub fn group(mut self, field: &str) -> Builder {
        self.group_by.push(self.dialect.quote_path(field));
        self
    }

    /// Sets the having fragment.
    pub fn having(mut self, having: &str) -> Builder {
        self.having = having.to_owned();
        self
    }

    /// Sets the limit and offset.
    pub fn limit(mut self, limit: i64, offset: i64) -> Builder {
        self.limit = limit;
        self.offset = offset;
        self
    }

    /// Sets pagination (`page` is 1-based), like `Page`.
    pub fn page(mut self, page: i64, size: i64) -> Builder {
        if page < 1 {
            self.limit = 0;
            self.offset = 0;
            return self;
        }
        self.limit = size;
        self.offset = (page - 1) * size;
        self
    }

    /// Renders the where condition without the `WHERE` keyword, like
    /// `WhereBuilder.Build`.
    pub fn build_where(&self) -> (String, Vec<Value>) {
        let mut condition = String::new();
        let mut args = Vec::new();
        for holder in &self.where_holders {
            if holder.skipped() {
                continue;
            }
            let (fragment, mut holder_args) = self.render_holder(holder, args.len() + 1);
            if fragment.is_empty() {
                continue;
            }
            if condition.is_empty() {
                condition = fragment;
            } else {
                match holder.kind {
                    WhereKind::And => {
                        condition.push_str(" AND ");
                        condition.push_str(&fragment);
                    }
                    WhereKind::Or => {
                        condition.push_str(" OR ");
                        condition.push_str(&fragment);
                    }
                }
            }
            args.append(&mut holder_args);
        }
        (condition, args)
    }

    /// Renders one holder.
    fn render_holder(&self, holder: &WhereHolder, first_index: usize) -> (String, Vec<Value>) {
        if holder.operator == WhereOperator::Raw {
            return (holder.raw.clone(), Vec::new());
        }
        let mut field = holder.field.clone();
        if !holder.prefix.is_empty() && !field.contains('.') {
            field = format!("{}{}", holder.prefix, field);
        }
        let field = self.dialect.quote_path(&field);
        let mut args = Vec::new();
        let placeholders = |values: &[Value], args: &mut Vec<Value>| {
            let mut rendered = Vec::new();
            for value in values {
                rendered.push(self.dialect.placeholder(first_index + args.len()));
                args.push(value.clone());
            }
            rendered
        };
        match holder.operator {
            WhereOperator::Eq
            | WhereOperator::Ne
            | WhereOperator::Lt
            | WhereOperator::Lte
            | WhereOperator::Gt
            | WhereOperator::Gte => {
                if matches!(holder.value, Value::Null) {
                    return match holder.operator {
                        WhereOperator::Eq => (format!("{field} IS NULL"), Vec::new()),
                        WhereOperator::Ne => (format!("{field} IS NOT NULL"), Vec::new()),
                        _ => (String::new(), Vec::new()),
                    };
                }
                let operator = match holder.operator {
                    WhereOperator::Eq => "=",
                    WhereOperator::Ne => "!=",
                    WhereOperator::Lt => "<",
                    WhereOperator::Lte => "<=",
                    WhereOperator::Gt => ">",
                    _ => ">=",
                };
                let placeholder = placeholders(std::slice::from_ref(&holder.value), &mut args);
                (format!("{field} {operator} {}", placeholder[0]), args)
            }
            WhereOperator::In | WhereOperator::NotIn => {
                let values = match &holder.value {
                    Value::List(values) => values.clone(),
                    other => vec![other.clone()],
                };
                if values.is_empty() {
                    return match holder.operator {
                        WhereOperator::In => ("0=1".to_owned(), Vec::new()),
                        _ => ("1=1".to_owned(), Vec::new()),
                    };
                }
                let rendered = placeholders(&values, &mut args);
                let operator = match holder.operator {
                    WhereOperator::In => "IN",
                    _ => "NOT IN",
                };
                (format!("{field} {operator} ({})", rendered.join(",")), args)
            }
            WhereOperator::Like | WhereOperator::NotLike => {
                let placeholder = placeholders(std::slice::from_ref(&holder.value), &mut args);
                let operator = match holder.operator {
                    WhereOperator::Like => "LIKE",
                    _ => "NOT LIKE",
                };
                (format!("{field} {operator} {}", placeholder[0]), args)
            }
            WhereOperator::Between | WhereOperator::NotBetween => {
                let values = vec![holder.value.clone(), holder.upper.clone()];
                let rendered = placeholders(&values, &mut args);
                let operator = match holder.operator {
                    WhereOperator::Between => "BETWEEN",
                    _ => "NOT BETWEEN",
                };
                (
                    format!("{field} {operator} {} AND {}", rendered[0], rendered[1]),
                    args,
                )
            }
            WhereOperator::Null => (format!("{field} IS NULL"), Vec::new()),
            WhereOperator::NotNull => (format!("{field} IS NOT NULL"), Vec::new()),
            WhereOperator::Raw => (holder.raw.clone(), Vec::new()),
        }
    }

    /// Renders the common clause tail (where/group/having/order/limit).
    fn render_tail(&self) -> (Vec<String>, Vec<Value>) {
        let mut clauses = Vec::new();
        let mut args = Vec::new();
        let (condition, where_args) = self.build_where();
        if !condition.is_empty() {
            clauses.push(format!("WHERE {condition}"));
            args.extend(where_args);
        }
        if !self.group_by.is_empty() {
            clauses.push(format!("GROUP BY {}", self.group_by.join(",")));
        }
        if !self.having.is_empty() {
            clauses.push(format!("HAVING {}", self.having));
        }
        if !self.order_by.is_empty() {
            clauses.push(format!("ORDER BY {}", self.order_by.join(",")));
        }
        if self.limit > 0 {
            clauses.push(format!("LIMIT {}", self.limit));
        }
        if self.offset > 0 {
            clauses.push(format!("OFFSET {}", self.offset));
        }
        (clauses, args)
    }

    /// Renders a SELECT statement.
    pub fn select_sql(&self) -> (String, Vec<Value>) {
        let mut sql = format!(
            "SELECT {}{} FROM {}",
            if self.distinct { "DISTINCT " } else { "" },
            self.fields,
            self.dialect.quote_path(&self.table)
        );
        for join in &self.joins {
            sql.push_str(&format!(
                " {} {} ON {}",
                join.kind.sql(),
                self.dialect.quote_path(&join.table),
                join.on
            ));
        }
        let (clauses, args) = self.render_tail();
        for clause in clauses {
            sql.push(' ');
            sql.push_str(&clause);
        }
        (sql, args)
    }

    /// Renders an `INSERT ... IGNORE`/`ON CONFLICT DO NOTHING` statement,
    /// like GoFrame's `InsertIgnore` (ADR-149).
    pub fn insert_ignore_sql(&self, data: &[(String, Value)]) -> (String, Vec<Value>) {
        let (sql, args) = self.insert_sql(data);
        let sql = match self.dialect {
            Dialect::MySql => sql.replacen("INSERT INTO", "INSERT IGNORE INTO", 1),
            Dialect::Sqlite => sql.replacen("INSERT INTO", "INSERT OR IGNORE INTO", 1),
            Dialect::Postgres => format!("{sql} ON CONFLICT DO NOTHING"),
        };
        (sql, args)
    }

    /// Renders an INSERT statement for `data`.
    pub fn insert_sql(&self, data: &[(String, Value)]) -> (String, Vec<Value>) {
        let mut columns = Vec::new();
        let mut values = Vec::new();
        let mut args = Vec::new();
        for (index, (column, value)) in data.iter().enumerate() {
            columns.push(self.dialect.quote_path(column));
            values.push(self.dialect.placeholder(index + 1));
            args.push(value.clone());
        }
        (
            format!(
                "INSERT INTO {} ({}) VALUES ({})",
                self.dialect.quote_path(&self.table),
                columns.join(","),
                values.join(",")
            ),
            args,
        )
    }

    /// Renders an UPDATE statement for `data`.
    pub fn update_sql(&self, data: &[(String, Value)]) -> (String, Vec<Value>) {
        let mut assignments = Vec::new();
        let mut args = Vec::new();
        for (index, (column, value)) in data.iter().enumerate() {
            assignments.push(format!(
                "{}={}",
                self.dialect.quote_path(column),
                self.dialect.placeholder(index + 1)
            ));
            args.push(value.clone());
        }
        let (condition, where_args) = self.build_where();
        let mut sql = format!(
            "UPDATE {} SET {}",
            self.dialect.quote_path(&self.table),
            assignments.join(",")
        );
        if !condition.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&condition);
            args.extend(where_args);
        }
        (sql, args)
    }

    /// Renders a DELETE statement.
    pub fn delete_sql(&self) -> (String, Vec<Value>) {
        let mut sql = format!("DELETE FROM {}", self.dialect.quote_path(&self.table));
        let (condition, args) = self.build_where();
        if !condition.is_empty() {
            sql.push_str(" WHERE ");
            sql.push_str(&condition);
        }
        (sql, args)
    }
}

/// Reports whether a value is nil, like `empty.IsNil`.
pub fn is_nil(value: &Value) -> bool {
    matches!(value, Value::Null)
}

/// Formats a value as a SQL literal (for diagnostics).
pub fn literal(value: &Value) -> String {
    match value {
        Value::Null => "NULL".to_owned(),
        Value::String(value) => format!("'{}'", value.replace('\'', "''")),
        other => gconv::string(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    #[test]
    fn where_equality_and_operators() {
        let (sql, args) = Builder::new("user")
            .where_eq("id", Value::Int(1))
            .where_op(WhereKind::And, WhereOperator::Gt, "age", Value::Int(18))
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `id` = ? AND `age` > ?");
        assert_eq!(args, vec![Value::Int(1), Value::Int(18)]);

        let (sql, args) = Builder::new("user")
            .where_eq("name", Value::Null)
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `name` IS NULL");
        assert!(args.is_empty());
        let (sql, _) = Builder::new("user")
            .where_op(WhereKind::And, WhereOperator::Ne, "name", Value::Null)
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `name` IS NOT NULL");
    }

    #[test]
    fn in_and_like_and_between() {
        let (sql, args) = Builder::new("user")
            .where_in("id", vec![Value::Int(1), Value::Int(2)])
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `id` IN (?,?)");
        assert_eq!(args, vec![Value::Int(1), Value::Int(2)]);

        let (sql, _) = Builder::new("user").where_in("id", Vec::new()).select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE 0=1");
        let (sql, _) = Builder::new("user")
            .where_not_in("id", Vec::new())
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE 1=1");

        let (sql, args) = Builder::new("user")
            .where_like("name", "john%")
            .where_not_null("email")
            .select_sql();
        assert_eq!(
            sql,
            "SELECT * FROM `user` WHERE `name` LIKE ? AND `email` IS NOT NULL"
        );
        assert_eq!(args, vec![value("john%")]);

        let (sql, args) = Builder::new("user")
            .where_between("age", Value::Int(18), Value::Int(30))
            .select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `age` BETWEEN ? AND ?");
        assert_eq!(args, vec![Value::Int(18), Value::Int(30)]);
    }

    #[test]
    fn omit_nil_conditions() {
        let mut builder = Builder::new("user").where_eq("id", Value::Null);
        builder.where_holders[0].operator = WhereOperator::Lt;
        builder.where_holders[0].omit_nil = true;
        let (sql, args) = builder.select_sql();
        assert_eq!(sql, "SELECT * FROM `user`");
        assert!(args.is_empty());
    }

    #[test]
    fn or_conditions_and_raw_prefix() {
        let (sql, args) = Builder::new("user")
            .where_eq("a", Value::Int(1))
            .or_where_eq("b", Value::Int(2))
            .where_raw("c > 0")
            .select_sql();
        assert_eq!(
            sql,
            "SELECT * FROM `user` WHERE `a` = ? OR `b` = ? AND c > 0"
        );
        assert_eq!(args, vec![Value::Int(1), Value::Int(2)]);

        let mut builder = Builder::new("user");
        builder.where_holders.push({
            let mut holder = WhereHolder::new(WhereKind::And, WhereOperator::Eq, "id");
            holder.value = Value::Int(1);
            holder
        });
        let builder = builder.prefix("u.");
        let (sql, _) = builder.select_sql();
        assert_eq!(sql, "SELECT * FROM `user` WHERE `u`.`id` = ?");
    }

    #[test]
    fn order_group_having_limit_page() {
        let (sql, _) = Builder::new("user")
            .fields("id,name")
            .group("name")
            .having("COUNT(*) > 1")
            .order_desc("id")
            .limit(10, 20)
            .select_sql();
        assert_eq!(
            sql,
            "SELECT id,name FROM `user` GROUP BY `name` HAVING COUNT(*) > 1 ORDER BY `id` DESC LIMIT 10 OFFSET 20"
        );
        let (sql, _) = Builder::new("user").page(3, 10).select_sql();
        assert_eq!(sql, "SELECT * FROM `user` LIMIT 10 OFFSET 20");
        let (sql, _) = Builder::new("user").page(0, 10).select_sql();
        assert_eq!(sql, "SELECT * FROM `user`");
        let (sql, _) = Builder::new("user").order_random().select_sql();
        assert_eq!(sql, "SELECT * FROM `user` ORDER BY RAND()");
    }

    #[test]
    fn insert_update_delete() {
        let data = vec![
            ("name".to_owned(), value("john")),
            ("age".to_owned(), Value::Int(18)),
        ];
        let (sql, args) = Builder::new("user").insert_sql(&data);
        assert_eq!(sql, "INSERT INTO `user` (`name`,`age`) VALUES (?,?)");
        assert_eq!(args, vec![value("john"), Value::Int(18)]);

        let (sql, args) = Builder::new("user")
            .where_eq("id", Value::Int(1))
            .update_sql(&[("name".to_owned(), value("jane"))]);
        assert_eq!(sql, "UPDATE `user` SET `name`=? WHERE `id` = ?");
        assert_eq!(args, vec![value("jane"), Value::Int(1)]);

        let (sql, args) = Builder::new("user")
            .where_eq("id", Value::Int(1))
            .delete_sql();
        assert_eq!(sql, "DELETE FROM `user` WHERE `id` = ?");
        assert_eq!(args, vec![Value::Int(1)]);
        let (sql, _) = Builder::new("user").delete_sql();
        assert_eq!(sql, "DELETE FROM `user`");
    }

    #[test]
    fn postgres_dialect() {
        let (sql, args) = Builder::new("user")
            .with_dialect(Dialect::Postgres)
            .where_eq("id", Value::Int(1))
            .where_in("name", vec![value("a"), value("b")])
            .select_sql();
        assert_eq!(
            sql,
            "SELECT * FROM \"user\" WHERE \"id\" = $1 AND \"name\" IN ($2,$3)"
        );
        assert_eq!(args, vec![Value::Int(1), value("a"), value("b")]);
    }

    #[test]
    fn helpers() {
        assert!(is_nil(&Value::Null));
        assert!(!is_nil(&Value::Int(0)));
        assert_eq!(literal(&value("o'hara")), "'o''hara'");
        assert_eq!(literal(&Value::Int(3)), "3");
        assert!(Dialect::MySql.quote_path("u.id") == "`u`.`id`");
        assert!(Dialect::MySql.quote_path("COUNT(*)") == "COUNT(*)");
    }
}
