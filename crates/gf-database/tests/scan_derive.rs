//! Integration test for `#[derive(FromRecord)]` struct scanning (ADR-100).

use gf_database::gdb::{FieldFromValue, FromRecord, Record, ToRecord, Value};
use gf_database::gdb_exec::Db;
use gf_database::gdb_model::Model;
use gf_database_derive::{FromRecord, ToRecord};

#[derive(FromRecord, Debug, PartialEq)]
struct User {
    id: i64,
    #[gdb(column = "name")]
    user_name: String,
    age: Option<i64>,
    score: f64,
    active: bool,
    #[gdb(skip)]
    cached: Vec<String>,
}

fn setup() -> Db {
    let db = Db::open_memory().expect("db");
    db.execute(
        "CREATE TABLE user (id INTEGER PRIMARY KEY, name TEXT, age INTEGER, score REAL, active INTEGER)",
        &[],
    )
    .expect("create");
    db.insert(
        "user",
        &[
            ("name".to_owned(), Value::String("john".to_owned())),
            ("age".to_owned(), Value::Int(20)),
            ("score".to_owned(), Value::Float(9.5)),
            ("active".to_owned(), Value::Int(1)),
        ],
    )
    .expect("insert");
    db.insert(
        "user",
        &[
            ("name".to_owned(), Value::String("jane".to_owned())),
            ("age".to_owned(), Value::Null),
            ("score".to_owned(), Value::Float(8.0)),
            ("active".to_owned(), Value::Int(0)),
        ],
    )
    .expect("insert");
    db
}

#[test]
fn scan_maps_columns_and_types() {
    let db = setup();
    let users: Vec<User> = Model::new(&db, "user")
        .order_asc("id")
        .scan()
        .expect("scan");
    assert_eq!(users.len(), 2);
    assert_eq!(
        users[0],
        User {
            id: 1,
            user_name: "john".to_owned(),
            age: Some(20),
            score: 9.5,
            active: true,
            cached: Vec::new(),
        }
    );
    assert_eq!(users[1].user_name, "jane");
    assert_eq!(users[1].age, None);
    assert!(!users[1].active);
}

#[test]
fn scan_one_returns_the_first_row() {
    let db = setup();
    let user: Option<User> = Model::new(&db, "user")
        .where_eq("name", Value::String("jane".to_owned()))
        .scan_one()
        .expect("scan one");
    assert_eq!(user.expect("row").id, 2);
    assert!(
        Model::new(&db, "user")
            .where_eq("name", Value::String("nobody".to_owned()))
            .scan_one::<User>()
            .expect("scan one missing")
            .is_none()
    );
}

#[test]
fn record_trait_and_field_conversions() {
    let mut record = Record::new();
    record.insert("id", Value::Int(7));
    record.insert("name", Value::String("amy".to_owned()));
    record.insert("score", Value::Float(1.25));
    let user = User::from_record(&record).expect("from record");
    assert_eq!(user.id, 7);
    assert_eq!(user.user_name, "amy");
    assert_eq!(user.age, None);
    assert_eq!(user.score, 1.25);
    assert_eq!(user.cached, Vec::<String>::new());

    // Field conversions tolerate missing values and lists.
    assert_eq!(
        <i64 as FieldFromValue>::field_from_value(None).expect("int"),
        0
    );
    assert_eq!(
        <String as FieldFromValue>::field_from_value(None).expect("string"),
        ""
    );
    assert_eq!(
        <Vec<String> as FieldFromValue>::field_from_value(Some(&Value::List(vec![Value::String(
            "a".to_owned()
        )])))
        .expect("list"),
        vec!["a".to_owned()]
    );
    assert_eq!(
        <Option<i64> as FieldFromValue>::field_from_value(Some(&Value::Null)).expect("null"),
        None
    );
}

#[test]
fn scan_works_with_builder_order_and_limit() {
    let db = setup();
    let users: Vec<User> = Model::new(&db, "user")
        .where_gt("id", Value::Int(0))
        .order_desc("id")
        .limit(1, 0)
        .scan()
        .expect("scan");
    assert_eq!(users.len(), 1);
    assert_eq!(users[0].user_name, "jane");
    // Missing columns fall back to `Default` values.
    let empty = Record::new();
    let defaulted = User::from_record(&empty).expect("empty record");
    assert_eq!(defaulted.id, 0);
    assert_eq!(defaulted.user_name, "");
}

#[derive(ToRecord)]
struct WriteUser {
    #[expect(dead_code, reason = "keeps the skip attribute covered")]
    #[gdb(skip)]
    id: i64,
    #[gdb(column = "name")]
    user_name: String,
    age: Option<i64>,
    score: f64,
    active: bool,
}

#[test]
fn to_record_maps_fields_and_writes_rows() {
    let write = WriteUser {
        id: 99,
        user_name: "kate".to_owned(),
        age: Some(31),
        score: 7.5,
        active: true,
    };
    let data = write.to_record();
    let columns: Vec<&str> = data.iter().map(|(column, _)| column.as_str()).collect();
    assert_eq!(columns, vec!["name", "age", "score", "active"]);
    assert_eq!(data[0].1, Value::String("kate".to_owned()));
    assert_eq!(data[1].1, Value::Int(31));
    assert_eq!(data[2].1, Value::Float(7.5));
    assert_eq!(data[3].1, Value::Bool(true));

    let db = Db::open_memory().expect("db");
    db.execute(
        "CREATE TABLE typed (id INTEGER PRIMARY KEY, name TEXT, age INTEGER, score REAL, active INTEGER)",
        &[],
    )
    .expect("create");
    let inserted = Model::new(&db, "typed")
        .insert_record(&write)
        .expect("insert record");
    let id = inserted.insert_id().expect("id");
    let row = Model::new(&db, "typed")
        .where_eq("id", Value::Int(id))
        .one()
        .expect("one")
        .expect("row");
    assert_eq!(row.get("name"), Some(&Value::String("kate".to_owned())));
    assert_eq!(row.get("age"), Some(&Value::Int(31)));

    let updated = WriteUser {
        user_name: "kate2".to_owned(),
        ..write
    };
    Model::new(&db, "typed")
        .where_eq("id", Value::Int(id))
        .update_record(&updated)
        .expect("update record");
    let row = Model::new(&db, "typed")
        .where_eq("id", Value::Int(id))
        .one()
        .expect("one")
        .expect("row");
    assert_eq!(row.get("name"), Some(&Value::String("kate2".to_owned())));
}
