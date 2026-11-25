//! Port of `os/genv` (`genv.go`, `genv_must.go`).
//!
//! The Go package wraps `os.Environ`/`os.Setenv`. Edition 2024 marks
//! `std::env::set_var`/`remove_var` unsafe because another thread may read the
//! environment concurrently, so every mutation in this module is serialized by
//! one process-wide lock and wrapped in a single documented `unsafe` block
//! (ADR-036). Callers must not read or mutate the process environment while
//! this module is changing it, which is the same process-global constraint the
//! Go original has.
//!
//! `map_from_env` returns `Result` where Go panics on an entry without `=`.

use std::sync::Mutex;

use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::Error;
use gf_internal::command;
use gf_internal::utils;
use indexmap::IndexMap;

use gf_container::gvar::Var;

/// Serializes process-environment mutations.
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Returns a copy of the environment as `key=value` strings.
pub fn all() -> Vec<String> {
    std::env::vars_os()
        .map(|(key, value)| format!("{}={}", key.to_string_lossy(), value.to_string_lossy()))
        .collect()
}

/// Returns a copy of the environment as a map.
pub fn map() -> IndexMap<String, String> {
    map_from_env_lossy(&all())
}

/// Returns the value for `key`, or `def` when the variable does not exist.
pub fn get(key: &str, def: Option<Value>) -> Option<Var> {
    match env_value(key) {
        Some(value) => Some(Var::new(Value::String(value))),
        None => def.map(Var::new),
    }
}

/// Reports whether the environment variable `key` exists.
pub fn contains(key: &str) -> bool {
    std::env::var_os(key).is_some()
}

/// Sets the environment variable `key` to `value`.
pub fn set(key: &str, value: &str) -> Result<(), Error> {
    let result = write_env(key, value);
    result.map_err(|message| {
        Error::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &[&format!(
                "set environment key-value failed with key \"{key}\", value \"{value}\": {message}"
            )],
        )
    })
}

/// Sets every entry of `data` as an environment variable.
pub fn set_map(data: &IndexMap<String, String>) -> Result<(), Error> {
    for (key, value) in data {
        set(key, value)?;
    }
    Ok(())
}

/// Sets `key` and panics on failure (`genv.MustSet`).
pub fn must_set(key: &str, value: &str) {
    if let Err(error) = set(key, value) {
        panic!("{error}");
    }
}

/// Deletes one or more environment variables.
pub fn remove(keys: &[&str]) -> Result<(), Error> {
    for key in keys {
        let result = delete_env(key);
        if let Err(message) = result {
            return Err(Error::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &[&format!(
                    "delete environment key failed with key \"{key}\": {message}"
                )],
            ));
        }
    }
    Ok(())
}

/// Deletes `keys` and panics on failure (`genv.MustRemove`).
pub fn must_remove(keys: &[&str]) {
    if let Err(error) = remove(keys) {
        panic!("{error}");
    }
}

/// Returns the environment value for `key`, falling back to the command line.
///
/// Environment keys are uppercase (`GF_<PACKAGE>_<VARIABLE>`), command line
/// options lowercase (`gf.<package>.<variable>`).
pub fn get_with_cmd(key: &str, def: Option<Value>) -> Option<Var> {
    let env_key = utils::format_env_key(key);
    if let Some(value) = env_value(&env_key)
        && !value.is_empty()
    {
        return Some(Var::new(Value::String(value)));
    }
    let cmd_key = utils::format_cmd_key(key);
    let value = command::get_opt(&cmd_key, None);
    if !value.is_empty() {
        return Some(Var::new(Value::String(value)));
    }
    def.map(Var::new)
}

/// Builds `key=value` strings from a map.
pub fn build(data: &IndexMap<String, String>) -> Vec<String> {
    data.iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

/// Converts `key=value` strings into a map.
///
/// An entry without `=` is an error; the Go original panics on it.
pub fn map_from_env(envs: &[String]) -> Result<IndexMap<String, String>, Error> {
    let mut entries = IndexMap::new();
    for entry in envs {
        match entry.split_once('=') {
            Some((key, value)) => {
                entries.insert(key.to_owned(), value.to_owned());
            }
            None => {
                return Err(Error::new_code(
                    &gcode::CODE_INVALID_PARAMETER,
                    &[&format!("invalid environment variable string: {entry}")],
                ));
            }
        }
    }
    Ok(entries)
}

/// Converts a map into `key=value` strings.
pub fn map_to_env(data: &IndexMap<String, String>) -> Vec<String> {
    data.iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect()
}

/// Filters repeated keys from `envs`, keeping the last value.
pub fn filter(envs: &[String]) -> Vec<String> {
    map_to_env(&map_from_env_lossy(envs))
}

/// Reads the environment variable, tolerating non-UTF-8 values.
fn env_value(key: &str) -> Option<String> {
    std::env::var_os(key).map(|value| value.to_string_lossy().into_owned())
}

/// Like [`map_from_env`], but entries without `=` are skipped.
fn map_from_env_lossy(envs: &[String]) -> IndexMap<String, String> {
    let mut entries = IndexMap::new();
    for entry in envs {
        if let Some((key, value)) = entry.split_once('=') {
            entries.insert(key.to_owned(), value.to_owned());
        }
    }
    entries
}

/// Validates and writes one environment variable.
fn write_env(key: &str, value: &str) -> Result<(), String> {
    validate_key(key)?;
    if value.contains('\0') {
        return Err("value contains a NUL byte".to_owned());
    }
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: edition 2024 marks environment mutation unsafe because concurrent
    // readers may observe a torn update. Every mutation in this module is
    // serialized by ENV_LOCK, and the port documents the process-global
    // constraint shared with Go's os.Setenv.
    unsafe { std::env::set_var(key, value) };
    Ok(())
}

/// Validates and deletes one environment variable.
fn delete_env(key: &str) -> Result<(), String> {
    validate_key(key)?;
    let _guard = ENV_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    // SAFETY: see write_env; deletion is serialized by ENV_LOCK.
    unsafe { std::env::remove_var(key) };
    Ok(())
}

/// Rejects keys the platform cannot represent.
fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() {
        return Err("key is empty".to_owned());
    }
    if key.contains('=') {
        return Err("key contains `=`".to_owned());
    }
    if key.contains('\0') {
        return Err("key contains a NUL byte".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair(key: &str, value: &str) -> (String, String) {
        (key.to_owned(), value.to_owned())
    }

    // From Test_GEnv_All / Test_GEnv_Map / Test_GEnv_Get / Test_GEnv_Contains /
    // Test_GEnv_Set / Test_GEnv_SetMap / Test_GEnv_Build / Test_GEnv_Remove.
    // One test mutates the environment so parallel tests never interleave.
    #[test]
    fn environment_round_trip() {
        let key = "GF_RUST_TEST_ENV";
        let _ = remove(&[key]);
        assert!(!contains(key));
        assert!(get(key, None).is_none());

        set(key, "TEST").expect("set");
        assert!(contains(key));
        assert_eq!(
            get(key, None).map(|var| var.string()),
            Some("TEST".to_owned())
        );
        assert_eq!(map().get(key), Some(&"TEST".to_owned()));
        assert!(all().iter().any(|entry| entry == "GF_RUST_TEST_ENV=TEST"));

        let mut data = IndexMap::new();
        data.insert(pair("GF_RUST_TEST_K1", "1").0, "1".to_owned());
        data.insert(pair("GF_RUST_TEST_K2", "2").0, "2".to_owned());
        set_map(&data).expect("set_map");
        assert_eq!(std::env::var("GF_RUST_TEST_K1").ok(), Some("1".to_owned()));
        assert_eq!(std::env::var("GF_RUST_TEST_K2").ok(), Some("2".to_owned()));

        let built = build(&data);
        assert!(built.contains(&"GF_RUST_TEST_K1=1".to_owned()));
        assert!(built.contains(&"GF_RUST_TEST_K2=2".to_owned()));

        remove(&[key, "GF_RUST_TEST_K1", "GF_RUST_TEST_K2"]).expect("remove");
        assert!(!contains(key));
        assert_eq!(std::env::var("GF_RUST_TEST_K1").ok(), None);
    }

    // From Test_MapFromEnv / Test_MapToEnv / Test_Filter.
    #[test]
    fn environment_slices_and_maps() {
        let entries = vec!["a=1".to_owned(), "b=2".to_owned()];
        let map = map_from_env(&entries).expect("valid");
        assert_eq!(map.get("a"), Some(&"1".to_owned()));
        assert_eq!(map.get("b"), Some(&"2".to_owned()));

        let mut data = IndexMap::new();
        data.insert("a".to_owned(), "1".to_owned());
        assert_eq!(map_to_env(&data), vec!["a=1".to_owned()]);

        assert_eq!(
            filter(&["a=1".to_owned(), "a=3".to_owned()]),
            vec!["a=3".to_owned()]
        );

        let error = map_from_env(&["novalue".to_owned()]).expect_err("invalid entry");
        assert!(
            error
                .to_string()
                .contains("invalid environment variable string")
        );
    }

    #[test]
    fn invalid_keys_are_rejected() {
        assert!(set("", "x").is_err());
        assert!(set("A=B", "x").is_err());
        assert!(remove(&["A\0B"]).is_err());
    }

    #[test]
    fn defaults_are_used_when_missing() {
        let value = get("GF_RUST_DEFINITELY_MISSING", Some(Value::Int(7)));
        assert_eq!(value.map(|var| var.int64()), Some(7));
    }

    // From Test_GetWithCmd.
    #[test]
    fn command_line_fallback() {
        let _guard = crate::test_support::command_lock();
        let _ = remove(&["GF_CMD_TEST"]);
        command::init(&["-test".to_owned(), "2".to_owned()]);
        assert_eq!(
            get_with_cmd("TEST", None).map(|var| var.string()),
            Some("2".to_owned())
        );
        // The environment wins over the command line, matching the Go test.
        set("TEST", "1").expect("set");
        assert_eq!(
            get_with_cmd("test", None).map(|var| var.string()),
            Some("1".to_owned())
        );
        remove(&["TEST"]).expect("remove");
    }
}
