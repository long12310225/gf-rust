//! Port of `util/gmode`.
//!
//! The Go package auto-detects develop mode by checking that the caller's
//! source file exists. The port uses `debug_assertions` for the same intent
//! (a development build carries sources), recorded in ADR-026.

use std::sync::{Mutex, OnceLock};

use gf_internal::command;

/// Mode not set.
pub const NOT_SET: &str = "not-set";
/// Development mode.
pub const DEVELOP: &str = "develop";
/// Testing mode.
pub const TESTING: &str = "testing";
/// Staging mode.
pub const STAGING: &str = "staging";
/// Production mode.
pub const PRODUCT: &str = "product";

const COMMAND_ENV_KEY: &str = "gf.gmode";

fn current() -> &'static Mutex<Option<String>> {
    static MODE: OnceLock<Mutex<Option<String>>> = OnceLock::new();
    MODE.get_or_init(|| Mutex::new(None))
}

/// Sets the mode for the current application.
pub fn set(mode: &str) {
    let mut guard = current()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = Some(mode.to_owned());
}

/// Sets develop mode.
pub fn set_develop() {
    set(DEVELOP);
}

/// Sets testing mode.
pub fn set_testing() {
    set(TESTING);
}

/// Sets staging mode.
pub fn set_staging() {
    set(STAGING);
}

/// Sets production mode.
pub fn set_product() {
    set(PRODUCT);
}

/// Returns the current mode, auto-detecting it once when unset.
pub fn mode() -> String {
    let mut guard = current()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(mode) = guard.as_ref() {
        return mode.clone();
    }
    let detected = {
        let configured = command::get_opt_with_env(COMMAND_ENV_KEY, None);
        if !configured.is_empty() {
            configured
        } else if cfg!(debug_assertions) {
            DEVELOP.to_owned()
        } else {
            PRODUCT.to_owned()
        }
    };
    *guard = Some(detected.clone());
    detected
}

/// Reports whether the current mode is develop.
pub fn is_develop() -> bool {
    mode() == DEVELOP
}

/// Reports whether the current mode is testing.
pub fn is_testing() -> bool {
    mode() == TESTING
}

/// Reports whether the current mode is staging.
pub fn is_staging() -> bool {
    mode() == STAGING
}

/// Reports whether the current mode is production.
pub fn is_product() -> bool {
    mode() == PRODUCT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_match_go_constants() {
        assert_eq!(NOT_SET, "not-set");
        assert_eq!(DEVELOP, "develop");
        assert_eq!(TESTING, "testing");
        assert_eq!(STAGING, "staging");
        assert_eq!(PRODUCT, "product");
    }

    #[test]
    fn set_and_query() {
        set_testing();
        assert_eq!(mode(), TESTING);
        assert!(is_testing());
        assert!(!is_develop());

        set_staging();
        assert!(is_staging());

        set_product();
        assert!(is_product());

        set_develop();
        assert!(is_develop());
    }
}
