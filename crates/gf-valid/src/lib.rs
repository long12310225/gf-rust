//! The port of GoFrame's `util/gvalid`.
//!
//! `util/gvalid` sits above `os/gtime` and `net/gip*` in Go, which the crate
//! graph cannot express inside `gf-util`; it therefore lives in its own crate
//! (ADR-039). The facade re-exports it as `gf::util::gvalid`.
//!
//! [`gvalid::ValidationError`] is the port of Go's `gvalid.Error` value and is
//! returned by value: Go hands the caller an interface backed by the whole
//! field/rule map, and boxing it in Rust would ripple through every accessor
//! for no behavioural gain.
#![expect(
    clippy::result_large_err,
    reason = "ValidationError mirrors Go's error value, which carries the error map"
)]

pub mod gvalid;
