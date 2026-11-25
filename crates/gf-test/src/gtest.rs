//! Port of `test/gtest` — the assertion helpers used by the GoFrame test
//! suite.
//!
//! Go's `gtest` wraps `*testing.T`: an assertion failure panics, `C` recovers
//! it and marks the test failed, and the package-level `Assert*` functions
//! panic directly. Rust's test harness already treats a panic as a failure, so
//! this port keeps the assertion surface and drops the `T`/`C` bookkeeping
//! (ADR-089):
//!
//! - `assert_eq`/`assert_ne` (values),
//! - `assert_gt`/`assert_ge`/`assert_lt`/`assert_le` (ordered values),
//! - `assert_in`/`assert_ni` (membership),
//! - `assert_nil`/`assert_not_nil` (`Option`),
//! - `error`/`fatal` for explicit failures.
//!
//! Every failure panics with the Go-style `[ASSERT] EXPECT ...` message.

use std::cmp::Ordering;
use std::fmt::Debug;

/// Formats and panics with a GoFrame-style assertion message.
fn fail(message: String) -> ! {
    panic!("[ASSERT] {message}")
}

/// Asserts `value == expect`, like `Assert`.
pub fn assert_eq<T: PartialEq + Debug>(value: &T, expect: &T) {
    if value != expect {
        fail(format!("EXPECT {value:?} == {expect:?}"));
    }
}

/// Asserts `value == expect` for values that print with `Display`
/// (GoFrame's string-coerced `AssertEQ`).
pub fn assert_eq_str(value: &str, expect: &str) {
    if value != expect {
        fail(format!("EXPECT {value} == {expect}"));
    }
}

/// Asserts `value != expect`, like `AssertNE`.
pub fn assert_ne<T: PartialEq + Debug>(value: &T, expect: &T) {
    if value == expect {
        fail(format!("EXPECT {value:?} != {expect:?}"));
    }
}

/// Asserts `value > expect`, like `AssertGT`.
pub fn assert_gt<T: PartialOrd + Debug>(value: &T, expect: &T) {
    if value.partial_cmp(expect) != Some(Ordering::Greater) {
        fail(format!("EXPECT {value:?} > {expect:?}"));
    }
}

/// Asserts `value >= expect`, like `AssertGE`.
pub fn assert_ge<T: PartialOrd + Debug>(value: &T, expect: &T) {
    if !matches!(
        value.partial_cmp(expect),
        Some(Ordering::Greater | Ordering::Equal)
    ) {
        fail(format!("EXPECT {value:?} >= {expect:?}"));
    }
}

/// Asserts `value < expect`, like `AssertLT`.
pub fn assert_lt<T: PartialOrd + Debug>(value: &T, expect: &T) {
    if value.partial_cmp(expect) != Some(Ordering::Less) {
        fail(format!("EXPECT {value:?} < {expect:?}"));
    }
}

/// Asserts `value <= expect`, like `AssertLE`.
pub fn assert_le<T: PartialOrd + Debug>(value: &T, expect: &T) {
    if !matches!(
        value.partial_cmp(expect),
        Some(Ordering::Less | Ordering::Equal)
    ) {
        fail(format!("EXPECT {value:?} <= {expect:?}"));
    }
}

/// Asserts `value` is in `expect`, like `AssertIN`.
pub fn assert_in<T: PartialEq + Debug>(value: &T, expect: &[T]) {
    if !expect.contains(value) {
        fail(format!("EXPECT {value:?} IN {expect:?}"));
    }
}

/// Asserts `value` is not in `expect`, like `AssertNI`.
pub fn assert_ni<T: PartialEq + Debug>(value: &T, expect: &[T]) {
    if expect.contains(value) {
        fail(format!("EXPECT {value:?} NOT IN {expect:?}"));
    }
}

/// Asserts `value` is `None`, like `AssertNil`.
pub fn assert_nil<T: Debug>(value: Option<&T>) {
    if value.is_some() {
        fail(format!("EXPECT {value:?} == nil"));
    }
}

/// Asserts `value` is `Some`, like the inverse of `AssertNil`.
pub fn assert_not_nil<T: Debug>(value: Option<&T>) {
    if value.is_none() {
        fail("EXPECT non-nil value".to_owned());
    }
}

/// Fails with `message`, like `T.Error`.
pub fn error(message: &str) -> ! {
    fail(message.to_owned())
}

/// Fails with `message`, like `T.Fatal`.
pub fn fatal(message: &str) -> ! {
    fail(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equality_assertions_pass() {
        assert_eq(&1, &1);
        assert_eq(&String::from("a"), &String::from("a"));
        assert_eq_str("a", "a");
        assert_ne(&1, &2);
        assert_gt(&2, &1);
        assert_ge(&2, &2);
        assert_lt(&1, &2);
        assert_le(&2, &2);
    }

    #[test]
    fn membership_and_nil_assertions_pass() {
        assert_in(&"b", &["a", "b", "c"]);
        assert_ni(&"d", &["a", "b", "c"]);
        assert_nil::<i32>(None);
        let value = 3;
        assert_not_nil(Some(&value));
    }

    #[test]
    #[should_panic(expected = "[ASSERT] EXPECT 1 == 2")]
    fn equality_failure_panics() {
        assert_eq(&1, &2);
    }

    #[test]
    #[should_panic(expected = "EXPECT 2 > 3")]
    fn ordering_failure_panics() {
        assert_gt(&2, &3);
    }

    #[test]
    #[should_panic(expected = "EXPECT \"d\" IN")]
    fn membership_failure_panics() {
        assert_in(&"d", &["a", "b"]);
    }

    #[test]
    #[should_panic(expected = "EXPECT Some(1) == nil")]
    fn nil_failure_panics() {
        assert_nil(Some(&1));
    }

    #[test]
    #[should_panic(expected = "[ASSERT] boom")]
    fn explicit_failures_panic() {
        error("boom");
    }
}
