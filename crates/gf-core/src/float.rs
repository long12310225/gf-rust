//! Go-compatible float text.
//!
//! GoFrame prints floats two ways: `gtype` uses `strconv.FormatFloat(v, 'g',
//! -1, 64)` while `gconv.String` uses the `'f'` form (ADR-012's
//! `format_g`/`format_f` split, see ADR-037). Rust's `Display` for floats is
//! already the `'f'` shape — shortest round-trip digits, no exponent — so the
//! `'g'` form is derived from it here.

/// The largest decimal exponent still written without an exponent part.
///
/// Go uses the shortest representation when choosing between `%f` and `%e`,
/// and then compares against a fixed precision of six.
const EXPONENT_LIMIT: i32 = 6;

/// The smallest decimal exponent still written without an exponent part.
const DECIMAL_LIMIT: i32 = -4;

/// Formats `value` like Go's `strconv.FormatFloat(value, 'g', -1, 64)`.
///
/// This is the text `gtype.Float64.String` and its JSON encoding produce.
pub fn format_g(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf".to_owned()
        } else {
            "+Inf".to_owned()
        };
    }
    format_shortest_g(&value.to_string())
}

/// Formats `value` like Go's `strconv.FormatFloat(float64(value), 'g', -1, 32)`.
///
/// This is the text `gtype.Float32.String` and its JSON encoding produce: the
/// digits are the shortest ones that round-trip as a `float32`.
pub fn format_g32(value: f32) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf".to_owned()
        } else {
            "+Inf".to_owned()
        };
    }
    format_shortest_g(&value.to_string())
}

/// Formats `value` like Go's `strconv.FormatFloat(value, 'f', -1, 64)`.
///
/// This is the text `gconv.String` produces for floats: never exponential, so
/// `1e300` becomes its full 301 digit form.
pub fn format_f(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_owned();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Inf".to_owned()
        } else {
            "+Inf".to_owned()
        };
    }
    value.to_string()
}

/// Rewrites Rust's shortest flat decimal text into Go's `'g'` form.
///
/// `flat` is a decimal (`"0.5"`, `"123"`, `"0.0000001"`), never exponential
/// and never carrying trailing zeros, which is exactly what `f64::to_string`
/// returns for the shortest round-trip digits.
fn format_shortest_g(flat: &str) -> String {
    let (sign, body) = match flat.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", flat),
    };
    let (integer, fraction) = match body.split_once('.') {
        Some((integer, fraction)) => (integer, fraction),
        None => (body, ""),
    };
    // Locate the first significant digit and its decimal exponent.
    let (digits, exponent) = if integer != "0" {
        let mut digits = String::with_capacity(integer.len() + fraction.len());
        digits.push_str(integer);
        digits.push_str(fraction);
        trim_trailing_zeros(&mut digits);
        (digits, integer.len() as i32 - 1)
    } else {
        match fraction.find(|digit| digit != '0') {
            None => return format!("{sign}0"),
            Some(index) => {
                let mut digits = fraction[index..].to_owned();
                trim_trailing_zeros(&mut digits);
                (digits, -(index as i32) - 1)
            }
        }
    };
    if !(DECIMAL_LIMIT..EXPONENT_LIMIT).contains(&exponent) {
        scientific(sign, &digits, exponent)
    } else if exponent >= 0 {
        positional(sign, &digits, exponent as usize)
    } else {
        leading_zero_division(sign, &digits, exponent)
    }
}

/// Drops the zeros a shortest representation never needs, keeping one digit.
fn trim_trailing_zeros(digits: &mut String) {
    while digits.len() > 1 && digits.ends_with('0') {
        digits.pop();
    }
}

/// Builds the `%e` shape: one digit, optional fraction, signed exponent with
/// at least two digits (`1e+06`, `1.234567e+06`, `5e-324`).
fn scientific(sign: &str, digits: &str, exponent: i32) -> String {
    let mut out = String::with_capacity(sign.len() + digits.len() + 6);
    out.push_str(sign);
    out.push_str(&digits[..1]);
    if digits.len() > 1 {
        out.push('.');
        out.push_str(&digits[1..]);
    }
    out.push('e');
    out.push(if exponent < 0 { '-' } else { '+' });
    let magnitude = exponent.unsigned_abs();
    if magnitude < 10 {
        out.push('0');
    }
    out.push_str(&magnitude.to_string());
    out
}

/// Builds a plain decimal with a non-negative exponent (`123`, `123.456`).
fn positional(sign: &str, digits: &str, exponent: usize) -> String {
    let point = exponent + 1;
    let mut out = String::with_capacity(sign.len() + point.max(digits.len()) + 1);
    out.push_str(sign);
    if digits.len() <= point {
        out.push_str(digits);
        out.push_str(&"0".repeat(point - digits.len()));
    } else {
        out.push_str(&digits[..point]);
        out.push('.');
        out.push_str(&digits[point..]);
    }
    out
}

/// Builds a plain decimal below one (`0.5`, `0.0001`).
fn leading_zero_division(sign: &str, digits: &str, exponent: i32) -> String {
    let mut out = String::with_capacity(sign.len() + digits.len() + 3);
    out.push_str(sign);
    out.push_str("0.");
    out.push_str(&"0".repeat((-exponent - 1) as usize));
    out.push_str(digits);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every vector was produced by `strconv.FormatFloat(v, 'g', -1, 64)` in
    /// Go 1.25.
    #[test]
    fn format_g_matches_go() {
        let cases: &[(f64, &str)] = &[
            (0.0, "0"),
            (1.0, "1"),
            (-1.0, "-1"),
            (0.5, "0.5"),
            (5.5, "5.5"),
            (123.456, "123.456"),
            (1.0 / 3.0, "0.3333333333333333"),
            (100_000.0, "100000"),
            (999_999.0, "999999"),
            (1_000_000.0, "1e+06"),
            (123_456.0, "123456"),
            (1_234_567.0, "1.234567e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (0.0001234, "0.0001234"),
            (-2.5, "-2.5"),
            (1e15, "1e+15"),
            (1e16, "1e+16"),
            (1e20, "1e+20"),
            (1e21, "1e+21"),
            (1e300, "1e+300"),
            (1e-7, "1e-07"),
            (1e-300, "1e-300"),
            (f64::MAX, "1.7976931348623157e+308"),
            (f64::MIN_POSITIVE, "2.2250738585072014e-308"),
            (123_456_789_012_345_678.0, "1.2345678901234568e+17"),
        ];
        for (value, expected) in cases {
            assert_eq!(
                format_g(*value),
                *expected,
                "format_g({value:?}) diverged from Go"
            );
        }
    }

    #[test]
    fn format_g_handles_non_finite_values() {
        assert_eq!(format_g(f64::NAN), "NaN");
        assert_eq!(format_g(f64::INFINITY), "+Inf");
        assert_eq!(format_g(f64::NEG_INFINITY), "-Inf");
        assert_eq!(format_g32(f32::NAN), "NaN");
        assert_eq!(format_g32(f32::INFINITY), "+Inf");
        assert_eq!(format_g32(f32::NEG_INFINITY), "-Inf");
        assert_eq!(format_g(-0.0), "-0");
    }

    /// Vectors from `strconv.FormatFloat(float64(v), 'g', -1, 32)`.
    #[test]
    fn format_g32_uses_float32_digits() {
        assert_eq!(format_g32(1e30), "1e+30");
        assert_eq!(format_g32(f32::MAX), "3.4028235e+38");
        assert_eq!(format_g32(1e-7), "1e-07");
        assert_eq!(format_g32(0.1), "0.1");
        assert_eq!(format_g32(123.456), "123.456");
    }

    /// `'f'` never goes exponential: the 301 digit form of `1e300` and the
    /// `0.0000001` form of `1e-7` both come straight from Rust's `Display`.
    #[test]
    fn format_f_stays_flat() {
        assert_eq!(format_f(123.0), "123");
        assert_eq!(format_f(1e300).len(), 301);
        assert!(format_f(1e300).ends_with('0'));
        assert_eq!(format_f(1e-7), "0.0000001");
        assert_eq!(format_f(f64::NAN), "NaN");
        assert_eq!(format_f(f64::INFINITY), "+Inf");
    }
}
