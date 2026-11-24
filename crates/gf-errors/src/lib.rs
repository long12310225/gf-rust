//! The port of GoFrame's `errors/` packages.
//!
//! Modules mirror the Go package paths: `errors/gcode` becomes [`gcode`],
//! `errors/gerror` becomes [`gerror`].

pub mod gcode;
pub mod gerror;
mod gfmt;
