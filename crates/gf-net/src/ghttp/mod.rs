//! HTTP support types ported from `net/ghttp`.
//!
//! The Go packages wrap `net/http`; Rust has no such standard server API, so
//! these types are defined over a small [`ResponseWriter`] trait that the
//! future `ghttp` server will implement (ADR-063).

pub mod response;
