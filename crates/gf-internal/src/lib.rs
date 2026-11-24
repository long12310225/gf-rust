//! The port of GoFrame's `internal/` packages.
//!
//! Modules mirror the Go package paths: `internal/consts` becomes
//! [`consts`], `internal/command` becomes [`command`], and so on.
//!
//! Go-runtime-only helpers are replaced rather than ported: `internal/mutex`
//! and `internal/rwmutex` map to Rust's locks (ADR-007), and
//! `internal/reflection` has no analogue because the port uses [`gf_core::Value`]
//! instead of Go runtime reflection (ADR-011).

pub mod command;
pub mod consts;
pub mod deepcopy;
pub mod empty;
pub mod errors;
pub mod fileinfo;
pub mod intlog;
pub mod utils;
