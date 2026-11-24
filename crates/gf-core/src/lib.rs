//! Shared primitives for the GoFrame Rust port.
//!
//! The Go framework passes `any` through most of its public surface. This crate
//! defines the single dynamic type the port uses in those positions, so every
//! unit agrees on representation, ordering and formatting.

pub mod float;
pub mod value;

#[cfg(feature = "serde")]
mod serde_impl;

pub use value::Value;

/// The GoFrame version this port tracks.
///
/// Kept here rather than in the facade so `gf-os` (`gbuild`) can read it
/// without depending on the facade crate (decided with the `gf-os` split).
pub const VERSION: &str = "v2.10.5";
