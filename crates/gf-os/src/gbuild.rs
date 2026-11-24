//! Port of `os/gbuild` (`gbuild.go`).
//!
//! GoFrame's `gf build` injects a BASE64-encoded JSON string into
//! `builtInVarStr` through `-ldflags`. The port reads the same payload from the
//! `GF_BUILD_DATA` environment variable at compile time
//! (`option_env!("GF_BUILD_DATA")`), so a build can do:
//!
//! ```text
//! $env:GF_BUILD_DATA = [Convert]::ToBase64String(...); cargo build --release
//! ```
//!
//! `Golang` has no Rust runtime equivalent, so it reports `rustc <rust-version>`
//! of this workspace (documented divergence).

use std::sync::OnceLock;

use gf_container::gvar::Var;
use gf_core::{VERSION, Value};
use gf_encoding::gbase64;
use gf_internal::intlog;
use indexmap::IndexMap;

/// The built-in variable name of the GoFrame version.
pub const GF_VERSION: &str = "gfVersion";
/// The built-in variable name of the runtime version.
pub const GO_VERSION: &str = "goVersion";
/// The built-in variable name of the git commit and datetime.
pub const BUILT_GIT: &str = "builtGit";
/// The built-in variable name of the build datetime.
pub const BUILT_TIME: &str = "builtTime";
/// The built-in variable name of the application version.
pub const BUILT_VERSION: &str = "builtVersion";

/// The raw BASE64 variable string injected by the build flags.
pub const BUILT_IN_VAR_STR: &str = match option_env!("GF_BUILD_DATA") {
    Some(value) => value,
    None => "",
};

/// The decoded built-in variables.
static BUILT_IN_VAR_MAP: OnceLock<IndexMap<String, Value>> = OnceLock::new();

/// Maintains the built info of the current binary.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildInfo {
    /// GoFrame version used to build.
    pub go_frame: String,
    /// Runtime version used to build.
    pub golang: String,
    /// Git repository commit id and datetime used to build.
    pub git: String,
    /// Build datetime.
    pub time: String,
    /// Built version.
    pub version: String,
    /// All custom built data key-value pairs.
    pub data: IndexMap<String, Value>,
}

/// Returns the basic built information of the binary.
pub fn info() -> BuildInfo {
    BuildInfo {
        go_frame: get(GF_VERSION, None).map_or_else(String::new, |var| var.string()),
        golang: get(GO_VERSION, None).map_or_else(String::new, |var| var.string()),
        git: get(BUILT_GIT, None).map_or_else(String::new, |var| var.string()),
        time: get(BUILT_TIME, None).map_or_else(String::new, |var| var.string()),
        version: get(BUILT_VERSION, None).map_or_else(String::new, |var| var.string()),
        data: data(),
    }
}

/// Returns the built-in binary variable with the given name.
pub fn get(name: &str, def: Option<Value>) -> Option<Var> {
    if let Some(value) = built_in_var_map().get(name) {
        return Some(Var::new(value.clone()));
    }
    def.map(Var::new)
}

/// Returns the custom built-in variables as a map.
pub fn data() -> IndexMap<String, Value> {
    built_in_var_map().clone()
}

/// Returns the decoded variables, running Go's `init` once.
fn built_in_var_map() -> &'static IndexMap<String, Value> {
    BUILT_IN_VAR_MAP.get_or_init(|| {
        let mut variables = IndexMap::new();
        if BUILT_IN_VAR_STR.is_empty() {
            intlog::print("no build variables");
            return variables;
        }
        match gbase64::decode(BUILT_IN_VAR_STR.as_bytes()) {
            Ok(decoded) => match gf_json::unmarshal_use_number::<IndexMap<String, Value>>(&decoded)
            {
                Ok(parsed) => variables = parsed,
                Err(error) => intlog::errorf(&format!("{error:?}")),
            },
            Err(error) => intlog::errorf(&format!("{error:?}")),
        }
        variables.insert(GF_VERSION.to_owned(), Value::String(VERSION.to_owned()));
        variables.insert(
            GO_VERSION.to_owned(),
            Value::String(format!("rustc {}", env!("CARGO_PKG_RUST_VERSION"))),
        );
        intlog::printf(&format!("build variables: {variables:?}"));
        variables
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_container::gmap::StrAnyMap;

    // From Test_Info: without an injected payload every field is empty.
    #[test]
    fn info_is_empty_without_injection() {
        let info = info();
        assert_eq!(info.go_frame, "");
        assert_eq!(info.golang, "");
        assert_eq!(info.git, "");
        assert_eq!(info.time, "");
        assert_eq!(info.version, "");
        assert!(info.data.is_empty());
    }

    // From Test_Get.
    #[test]
    fn get_with_and_without_default() {
        assert!(get("none", None).is_none());
        assert_eq!(
            get("none", Some(Value::Int(1))).map(|var| var.int64()),
            Some(1)
        );
    }

    // From Test_Map.
    #[test]
    fn data_is_empty_without_injection() {
        let map: StrAnyMap = StrAnyMap::new();
        assert_eq!(data().len(), map.size());
    }
}
