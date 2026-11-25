//! The port of GoFrame's `database/` packages.
//!
//! `gredis` is a RESP2 client with pipelines, transactions, pub/sub and
//! scripting (ADR-070/097/098/099). `gdb` provides the value layer, SQL
//! builder, SQLite execution, chained model, transactions, connection pool and
//! derive-based struct scanning (ADR-071/072/076/077/079/096/100).

// The `#[derive(FromRecord)]` expansion refers to `::gf_database::...`, so the
// crate must resolve its own name (ADR-100).
extern crate self as gf_database;

pub mod gcache_redis;
pub mod gdb;
pub mod gdb_builder;
pub mod gdb_exec;
pub mod gdb_model;
pub mod gdb_pool;
pub mod gredis;
