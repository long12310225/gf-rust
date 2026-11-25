//! The port of GoFrame's `database/` packages.
//!
//! `gredis` is a minimal RESP2 client (ADR-070); the `gdb` ORM has not been
//! started yet.

pub mod gdb;
pub mod gdb_builder;
pub mod gredis;
