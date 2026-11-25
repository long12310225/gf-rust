//! HTTP support types ported from `net/ghttp`.
//!
//! The Go packages wrap `net/http`; Rust has no such standard server API, so
//! these types are defined over a small [`ResponseWriter`] trait that the
//! future `ghttp` server will implement (ADR-063).

pub mod cors;
pub mod file_server;
pub mod gzip;
pub mod message;
pub mod middleware;
pub mod openapi;
pub mod response;
pub mod router;
pub mod server;
