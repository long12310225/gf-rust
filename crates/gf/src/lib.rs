//! The facade crate of the GoFrame Rust port.
//!
//! Package paths mirror `github.com/gogf/gf/v2`: `gf::encoding::ghash` is
//! `encoding/ghash`, `gf::errors::gcode` is `errors/gcode`, and so on.

/// The GoFrame version this port tracks.
pub use gf_core::VERSION;

pub use gf_container as container;
pub use gf_core::Value;
pub use gf_crypto as crypto;
pub use gf_debug as debug;
pub use gf_encoding as encoding;
pub use gf_errors as errors;
pub use gf_internal as internal;
pub use gf_json as json;
pub use gf_net as net;
pub use gf_os as os;
pub use gf_text as text;

/// The `util/` packages. `util/gvalid` lives in its own crate because it sits
/// above `os/gtime` and `net/gip*` (ADR-039); the module keeps Go's paths.
pub mod util {
    pub use gf_util::*;
    pub use gf_valid::gvalid;
}

#[cfg(test)]
mod tests {
    use crate::errors::gcode::Code;

    #[test]
    fn version_matches_the_go_source() {
        assert_eq!(crate::VERSION, "v2.10.5");
    }

    #[test]
    fn package_paths_are_reachable() {
        assert_eq!(
            crate::encoding::ghash::bkdr(b"abc"),
            crate::encoding::ghash::bkdr(b"abc")
        );
        assert_eq!(crate::errors::gcode::CODE_OK.code(), 0);
        assert_eq!(crate::internal::consts::CONFIG_NODE_NAME_SERVER, "server");
        // `util/gvalid` lives in its own crate but keeps Go's path (ADR-039).
        assert!(
            crate::util::gvalid::check_value(
                "name",
                &crate::Value::String("john".to_owned()),
                "required"
            )
            .is_ok()
        );
        assert!(crate::util::gvalid::check_value("name", &crate::Value::Null, "required").is_err());
    }
}
