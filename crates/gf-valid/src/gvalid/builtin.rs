//! Port of `util/gvalid/internal/builtin`.
//!
//! Go registers one small type per rule through `init()`; the port keeps one
//! function per rule in a static table, and the rule's default message next to
//! it. `internal/` in Go means "not importable outside `gvalid`", which the
//! port mirrors by keeping this module private to [`crate::gvalid`].
//!
//! Two Go inputs are replaced here: `reflect.Type` (only the `enums` rule needs
//! it, and the port receives an optional type id instead) and `any` (values are
//! [`Value`] wrapped in [`Var`]).

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_internal::empty;
use gf_net::{gipv4, gipv6};
use gf_os::gtime;
use gf_text::{gregex, gstr};
use gf_util::{gconv, gutil};
use indexmap::IndexMap;

/// The input of one builtin rule run, like Go's `builtin.RunInput`.
pub(crate) struct RunInput<'a> {
    /// The rule name, like `max` in `max:6`.
    pub rule_key: &'a str,
    /// The rule's parameter text, like `6` in `max:6`.
    pub rule_pattern: &'a str,
    /// The value being validated.
    pub value: Var,
    /// The whole validation data, for rules that compare with another field.
    pub data: Var,
    /// The message chosen for this rule before it ran.
    pub message: String,
    /// Whether comparison rules ignore case (`ci`).
    pub case_insensitive: bool,
    /// The Go type id of the value when the caller knows it (`enums`).
    pub value_type: Option<&'a str>,
}

/// A builtin rule: its name, default message and check.
pub(crate) struct Rule {
    pub name: &'static str,
    pub message: &'static str,
    pub run: fn(&RunInput<'_>) -> Result<(), Error>,
}

/// Every builtin rule, in the order GoFrame documents them.
pub(crate) static RULES: &[Rule] = &[
    Rule {
        name: "after",
        message: "The {field} value `{value}` must be after field {field1} value `{value1}`",
        run: rule_after,
    },
    Rule {
        name: "after-equal",
        message: "The {field} value `{value}` must be after or equal to field {field1} value `{value1}`",
        run: rule_after_equal,
    },
    Rule {
        name: "alpha",
        message: "The {field} value `{value}` must contain only alphabetic characters",
        run: rule_alpha,
    },
    Rule {
        name: "alpha-dash",
        message: "The {field} value `{value}` must contain only alpha-numeric characters, hyphens, and underscores",
        run: rule_alpha_dash,
    },
    Rule {
        name: "alpha-num",
        message: "The {field} value `{value}` must contain only alpha-numeric characters",
        run: rule_alpha_num,
    },
    Rule {
        name: "array",
        message: "The {field} value `{value}` is not of valid array type",
        run: rule_array,
    },
    Rule {
        name: "bail",
        message: "",
        run: rule_decorative,
    },
    Rule {
        name: "bank-card",
        message: "The {field} value `{value}` is not a valid bank card number",
        run: rule_bank_card,
    },
    Rule {
        name: "before",
        message: "The {field} value `{value}` must be before field {field1} value `{value1}`",
        run: rule_before,
    },
    Rule {
        name: "before-equal",
        message: "The {field} value `{value}` must be before or equal to field {pattern}",
        run: rule_before_equal,
    },
    Rule {
        name: "between",
        message: "The {field} value `{value}` must be between {min} and {max}",
        run: rule_between,
    },
    Rule {
        name: "boolean",
        message: "The {field} value `{value}` field must be true or false",
        run: rule_boolean,
    },
    Rule {
        name: "ci",
        message: "",
        run: rule_decorative,
    },
    Rule {
        name: "date",
        message: "The {field} value `{value}` is not a valid date",
        run: rule_date,
    },
    Rule {
        name: "date-format",
        message: "The {field} value `{value}` does not match the format: {pattern}",
        run: rule_date_format,
    },
    Rule {
        name: "datetime",
        message: "The {field} value `{value}` is not a valid datetime",
        run: rule_datetime,
    },
    Rule {
        name: "different",
        message: "The {field} value `{value}` must be different from field {field1} value `{value1}`",
        run: rule_different,
    },
    Rule {
        name: "domain",
        message: "The {field} value `{value}` is not a valid domain format",
        run: rule_domain,
    },
    Rule {
        name: "email",
        message: "The {field} value `{value}` is not a valid email address",
        run: rule_email,
    },
    Rule {
        name: "enums",
        message: "The {field} value `{value}` should be in enums of: {enums}",
        run: rule_enums,
    },
    Rule {
        name: "eq",
        message: "The {field} value `{value}` must be equal to field {field1} value `{value1}`",
        run: rule_same,
    },
    Rule {
        name: "float",
        message: "The {field} value `{value}` is not of valid float type",
        run: rule_float,
    },
    Rule {
        name: "foreach",
        message: "",
        run: rule_decorative,
    },
    Rule {
        name: "gt",
        message: "The {field} value `{value}` must be greater than field {field1} value `{value1}`",
        run: rule_gt,
    },
    Rule {
        name: "gte",
        message: "The {field} value `{value}` must be greater than or equal to field {field1} value `{value1}`",
        run: rule_gte,
    },
    Rule {
        name: "in",
        message: "The {field} value `{value}` is not in acceptable range: {pattern}",
        run: rule_in,
    },
    Rule {
        name: "integer",
        message: "The {field} value `{value}` is not an integer",
        run: rule_integer,
    },
    Rule {
        name: "ip",
        message: "The {field} value `{value}` is not a valid IP address",
        run: rule_ip,
    },
    Rule {
        name: "ipv4",
        message: "The {field} value `{value}` is not a valid IPv4 address",
        run: rule_ipv4,
    },
    Rule {
        name: "ipv6",
        message: "The {field} value `{value}` is not a valid IPv6 address",
        run: rule_ipv6,
    },
    Rule {
        name: "json",
        message: "The {field} value `{value}` is not a valid JSON string",
        run: rule_json,
    },
    Rule {
        name: "length",
        message: "The {field} value `{value}` length must be between {min} and {max}",
        run: rule_length,
    },
    Rule {
        name: "lowercase",
        message: "The {field} value `{value}` must be lowercase",
        run: rule_lowercase,
    },
    Rule {
        name: "lt",
        message: "The {field} value `{value}` must be lesser than field {field1} value `{value1}`",
        run: rule_lt,
    },
    Rule {
        name: "lte",
        message: "The {field} value `{value}` must be lesser than or equal to field {field1} value `{value1}`",
        run: rule_lte,
    },
    Rule {
        name: "mac",
        message: "The {field} value `{value}` is not a valid MAC address",
        run: rule_mac,
    },
    Rule {
        name: "max",
        message: "The {field} value `{value}` must be equal or lesser than {max}",
        run: rule_max,
    },
    Rule {
        name: "max-length",
        message: "The {field} value `{value}` length must be equal or lesser than {max}",
        run: rule_max_length,
    },
    Rule {
        name: "min",
        message: "The {field} value `{value}` must be equal or greater than {min}",
        run: rule_min,
    },
    Rule {
        name: "min-length",
        message: "The {field} value `{value}` length must be equal or greater than {min}",
        run: rule_min_length,
    },
    Rule {
        name: "not-eq",
        message: "The {field} value `{value}` must not be equal to field {field1} value `{value1}`",
        run: rule_different,
    },
    Rule {
        name: "not-in",
        message: "The {field} value `{value}` must not be in range: {pattern}",
        run: rule_not_in,
    },
    Rule {
        name: "not-regex",
        message: "The {field} value `{value}` should not be in regex of: {pattern}",
        run: rule_not_regex,
    },
    Rule {
        name: "numeric",
        message: "The {field} value `{value}` must be numeric",
        run: rule_numeric,
    },
    Rule {
        name: "passport",
        message: "The {field} value `{value}` is not a valid passport format",
        run: rule_passport,
    },
    Rule {
        name: "password",
        message: "The {field} value `{value}` is not a valid password format",
        run: rule_password,
    },
    Rule {
        name: "password2",
        message: "The {field} value `{value}` is not a valid password2 format",
        run: rule_password2,
    },
    Rule {
        name: "password3",
        message: "The {field} value `{value}` is not a valid password3 format",
        run: rule_password3,
    },
    Rule {
        name: "phone",
        message: "The {field} value `{value}` is not a valid phone number",
        run: rule_phone,
    },
    Rule {
        name: "phone-loose",
        message: "The {field} value `{value}` is not a valid phone number",
        run: rule_phone_loose,
    },
    Rule {
        name: "postcode",
        message: "The {field} value `{value}` is not a valid postcode format",
        run: rule_postcode,
    },
    Rule {
        name: "qq",
        message: "The {field} value `{value}` is not a valid QQ number",
        run: rule_qq,
    },
    Rule {
        name: "regex",
        message: "The {field} value `{value}` must be in regex of: {pattern}",
        run: rule_regex,
    },
    Rule {
        name: "required",
        message: "The {field} field is required",
        run: rule_required,
    },
    Rule {
        name: "required-if",
        message: "The {field} field is required",
        run: rule_required_if,
    },
    Rule {
        name: "required-if-all",
        message: "The {field} field is required",
        run: rule_required_if_all,
    },
    Rule {
        name: "required-unless",
        message: "The {field} field is required",
        run: rule_required_unless,
    },
    Rule {
        name: "required-with",
        message: "The {field} field is required",
        run: rule_required_with,
    },
    Rule {
        name: "required-with-all",
        message: "The {field} field is required",
        run: rule_required_with_all,
    },
    Rule {
        name: "required-without",
        message: "The {field} field is required",
        run: rule_required_without,
    },
    Rule {
        name: "required-without-all",
        message: "The {field} field is required",
        run: rule_required_without_all,
    },
    Rule {
        name: "resident-id",
        message: "The {field} value `{value}` is not a valid resident id number",
        run: rule_resident_id,
    },
    Rule {
        name: "same",
        message: "The {field} value `{value}` must be the same as field {field1} value `{value1}`",
        run: rule_same,
    },
    Rule {
        name: "size",
        message: "The {field} value `{value}` length must be {size}",
        run: rule_size,
    },
    Rule {
        name: "telephone",
        message: "The {field} value `{value}` is not a valid telephone number",
        run: rule_telephone,
    },
    Rule {
        name: "uppercase",
        message: "The {field} value `{value}` must be uppercase",
        run: rule_uppercase,
    },
    Rule {
        name: "url",
        message: "The {field} value `{value}` is not a valid URL address",
        run: rule_url,
    },
];

/// Looks a rule up by name, like Go's `builtin.GetRule`.
pub(crate) fn get_rule(name: &str) -> Option<&'static Rule> {
    RULES.iter().find(|rule| rule.name == name)
}

/// The length rules share this notion: container length for lists and maps,
/// unicode rune count for everything else (`valueLength`).
fn value_length(value: &Value) -> usize {
    match value {
        Value::Bytes(_) => gconv::runes(value).len(),
        Value::List(items) => items.len(),
        Value::Map(entries) => entries.len(),
        other => gconv::runes(other).len(),
    }
}

/// Go's `isRequiredEmpty`: containers are empty when they hold nothing, any
/// other value when its text is empty. A zero integer is not empty.
fn is_required_empty(value: &Value) -> bool {
    match value {
        Value::String(text) => text.is_empty(),
        Value::Bytes(bytes) => bytes.is_empty(),
        Value::List(items) => items.is_empty(),
        Value::Map(entries) => entries.is_empty(),
        other => gconv::string(other).is_empty(),
    }
}

/// A "message only" rule failure, like Go's `errors.New(in.Message)`.
fn fail(input: &RunInput<'_>) -> Error {
    gerror::new(input.message.clone())
}

/// Replaces one placeholder, like Go's `gstr.Replace`.
fn replace_one(message: &str, key: &str, value: &str) -> String {
    gstr::replace(message, key, value, None)
}

/// Replaces `{field1}` and `{value1}` with the compared field, like Go's
/// `gstr.ReplaceByMap` call in the field comparison rules.
fn compared_message(input: &RunInput<'_>, field_name: &str, field_value: &Value) -> Error {
    let mut replaces = IndexMap::new();
    replaces.insert("{field1}".to_owned(), field_name.to_owned());
    replaces.insert("{value1}".to_owned(), gconv::string(field_value));
    gerror::new(gstr::replace_by_map(&input.message, &replaces))
}

/// Looks up `pattern` in the validation data, returning the matched key and
/// value (`gutil.MapPossibleItemByKey`).
fn data_item(input: &RunInput<'_>, pattern: &str) -> (String, Value) {
    let data = input.data.map();
    match gutil::map_possible_item_by_key(&data, pattern) {
        Some((key, value)) => (key.clone(), value.clone()),
        None => (String::new(), Value::Null),
    }
}

/// A parsed pair pattern (`a,b,c,d`) used by the `required-*` rules.
fn pairs(input: &RunInput<'_>) -> Vec<(String, String)> {
    let parts = gstr::split(input.rule_pattern, ",");
    let mut pairs = Vec::with_capacity(parts.len() / 2);
    let mut index = 0;
    while index + 1 < parts.len() {
        pairs.push((
            parts[index].trim().to_owned(),
            parts[index + 1].trim().to_owned(),
        ));
        index += 2;
    }
    pairs
}

/// Compares two texts, honouring the `ci` option.
fn text_eq(input: &RunInput<'_>, left: &str, right: &str) -> bool {
    if input.case_insensitive {
        left.to_lowercase() == right.to_lowercase()
    } else {
        left == right
    }
}

/// Parses a time from its text, returning `None` where Go's `gconv.Time`
/// returns nil.
fn parse_time(text: &str) -> Option<gtime::Time> {
    gtime::str_to_time(text, None).ok()
}

/// The rules that only mark behaviour (`bail`, `ci`, `foreach`).
fn rule_decorative(_input: &RunInput<'_>) -> Result<(), Error> {
    Ok(())
}

/// `required` comparison helper shared by the conditional rules.
fn require_if(condition: bool, input: &RunInput<'_>) -> Result<(), Error> {
    if condition && is_required_empty(&input.value.val()) {
        return Err(fail(input));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Date and time rules
// ---------------------------------------------------------------------------

fn rule_after(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    match (
        parse_time(&input.value.string()),
        parse_time(&gconv::string(&field_value)),
    ) {
        (Some(value), Some(field)) if value.after(&field) => Ok(()),
        _ => Err(compared_message(input, &field_name, &field_value)),
    }
}

fn rule_after_equal(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    match (
        parse_time(&input.value.string()),
        parse_time(&gconv::string(&field_value)),
    ) {
        (Some(value), Some(field)) if value.after(&field) || value.equal(&field) => Ok(()),
        _ => Err(compared_message(input, &field_name, &field_value)),
    }
}

fn rule_before(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    match (
        parse_time(&input.value.string()),
        parse_time(&gconv::string(&field_value)),
    ) {
        (Some(value), Some(field)) if value.before(&field) => Ok(()),
        _ => Err(compared_message(input, &field_name, &field_value)),
    }
}

fn rule_before_equal(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    match (
        parse_time(&input.value.string()),
        parse_time(&gconv::string(&field_value)),
    ) {
        (Some(value), Some(field)) if value.before(&field) || value.equal(&field) => Ok(()),
        _ => Err(compared_message(input, &field_name, &field_value)),
    }
}

fn rule_date(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    for layout in ["Ymd", "Y-m-d", "Y.m.d", "Y/m/d"] {
        if gtime::str_to_time_format(&value, layout).is_ok() {
            return Ok(());
        }
    }
    Err(fail(input))
}

fn rule_date_format(input: &RunInput<'_>) -> Result<(), Error> {
    if gtime::str_to_time_format(&input.value.string(), input.rule_pattern).is_ok() {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_datetime(input: &RunInput<'_>) -> Result<(), Error> {
    if gtime::str_to_time_format(&input.value.string(), "Y-m-d H:i:s").is_ok() {
        return Ok(());
    }
    Err(fail(input))
}

// ---------------------------------------------------------------------------
// Character class rules
// ---------------------------------------------------------------------------

fn matches(input: &RunInput<'_>, pattern: &str) -> bool {
    gregex::is_match_string(pattern, &input.value.string())
}

fn rule_alpha(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[a-zA-Z]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_alpha_dash(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[a-zA-Z0-9_\-]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_alpha_num(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[a-zA-Z0-9]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_lowercase(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[a-z]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_uppercase(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[A-Z]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_numeric(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[0-9]+$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_domain(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(
        input,
        r"^([0-9a-zA-Z][0-9a-zA-Z\-]{0,62}\.)+([a-zA-Z]{0,62})$",
    ) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_email(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(
        input,
        r"^[a-zA-Z0-9_\-\.]+@[a-zA-Z0-9_\-]+(\.[a-zA-Z0-9_\-]+)+$",
    ) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_mac(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^([0-9A-Fa-f]{2}[\-:]){5}[0-9A-Fa-f]{2}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_passport(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[a-zA-Z]{1}\w{5,17}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_password(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[\w\S]{6,18}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_password2(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    let ok = gregex::is_match_string(r"^[\w\S]{6,18}$", &value)
        && gregex::is_match_string("[a-z]+", &value)
        && gregex::is_match_string("[A-Z]+", &value)
        && gregex::is_match_string(r"\d+", &value);
    if ok {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_password3(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    let ok = gregex::is_match_string(r"^[\w\S]{6,18}$", &value)
        && gregex::is_match_string("[a-z]+", &value)
        && gregex::is_match_string("[A-Z]+", &value)
        && gregex::is_match_string(r"\d+", &value)
        && gregex::is_match_string("[^a-zA-Z0-9]+", &value);
    if ok {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_phone(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(
        input,
        r"^13[\d]{9}$|^14[5,7]{1}\d{8}$|^15[^4]{1}\d{8}$|^16[\d]{9}$|^17[0,1,2,3,5,6,7,8]{1}\d{8}$|^18[\d]{9}$|^19[\d]{9}$",
    ) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_phone_loose(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^1(3|4|5|6|7|8|9)\d{9}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_postcode(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^\d{6}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_qq(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^[1-9][0-9]{4,}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_telephone(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, r"^((\d{3,4})|\d{3,4}-)?\d{7,8}$") {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_url(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(
        input,
        r"(https?|ftp|file)://[-A-Za-z0-9+&@#/%?=~_|!:,.;]+[-A-Za-z0-9+&@#/%=~_|]",
    ) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_regex(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, input.rule_pattern) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_not_regex(input: &RunInput<'_>) -> Result<(), Error> {
    if matches(input, input.rule_pattern) {
        return Err(fail(input));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Number and container rules
// ---------------------------------------------------------------------------

fn rule_array(input: &RunInput<'_>) -> Result<(), Error> {
    if matches!(input.value.val(), Value::List(_)) {
        return Ok(());
    }
    let bytes = input.value.bytes();
    if gf_json::valid(&bytes) {
        let text = input.value.string();
        if text.len() > 1 && text.starts_with('[') && text.ends_with(']') {
            return Ok(());
        }
    }
    Err(fail(input))
}

fn parse_pair(pattern: &str) -> (f64, f64) {
    let parts = gstr::split(pattern, ",");
    let mut numbers = (0.0_f64, 0.0_f64);
    if let Some(value) = parts
        .first()
        .and_then(|first| first.trim().parse::<f64>().ok())
    {
        numbers.0 = value;
    }
    if let Some(value) = parts
        .get(1)
        .and_then(|second| second.trim().parse::<f64>().ok())
    {
        numbers.1 = value;
    }
    numbers
}

/// The integer variant Go uses for the length rules (`strconv.Atoi`).
fn parse_int_pair(pattern: &str) -> (i64, i64) {
    let parts = gstr::split(pattern, ",");
    let mut numbers = (0_i64, 0_i64);
    if let Some(value) = parts
        .first()
        .and_then(|first| first.trim().parse::<i64>().ok())
    {
        numbers.0 = value;
    }
    if let Some(value) = parts
        .get(1)
        .and_then(|second| second.trim().parse::<i64>().ok())
    {
        numbers.1 = value;
    }
    numbers
}

fn rule_between(input: &RunInput<'_>) -> Result<(), Error> {
    let (min, max) = parse_pair(input.rule_pattern);
    match input.value.string().parse::<f64>() {
        Ok(value) if value >= min && value <= max => Ok(()),
        _ => {
            let mut replaces = IndexMap::new();
            replaces.insert("{min}".to_owned(), gf_core::float::format_f(min));
            replaces.insert("{max}".to_owned(), gf_core::float::format_f(max));
            Err(gerror::new(gstr::replace_by_map(&input.message, &replaces)))
        }
    }
}

fn rule_length(input: &RunInput<'_>) -> Result<(), Error> {
    let (min, max) = parse_int_pair(input.rule_pattern);
    let value_len = value_length(&input.value.val()) as i64;
    if value_len >= min && value_len <= max {
        return Ok(());
    }
    let mut replaces = IndexMap::new();
    replaces.insert("{min}".to_owned(), min.to_string());
    replaces.insert("{max}".to_owned(), max.to_string());
    Err(gerror::new(gstr::replace_by_map(&input.message, &replaces)))
}

fn rule_min_length(input: &RunInput<'_>) -> Result<(), Error> {
    let min = input.rule_pattern.trim().parse::<usize>().unwrap_or(0);
    if value_length(&input.value.val()) < min {
        return Err(gerror::new(replace_one(
            &input.message,
            "{min}",
            &min.to_string(),
        )));
    }
    Ok(())
}

fn rule_max_length(input: &RunInput<'_>) -> Result<(), Error> {
    let max = input.rule_pattern.trim().parse::<usize>().unwrap_or(0);
    if value_length(&input.value.val()) > max {
        return Err(gerror::new(replace_one(
            &input.message,
            "{max}",
            &max.to_string(),
        )));
    }
    Ok(())
}

fn rule_size(input: &RunInput<'_>) -> Result<(), Error> {
    let size = input.rule_pattern.trim().parse::<usize>().unwrap_or(0);
    if value_length(&input.value.val()) != size {
        return Err(gerror::new(replace_one(
            &input.message,
            "{size}",
            &size.to_string(),
        )));
    }
    Ok(())
}

fn rule_min(input: &RunInput<'_>) -> Result<(), Error> {
    let min = input.rule_pattern.trim().parse::<f64>().unwrap_or(0.0);
    match input.value.string().parse::<f64>() {
        Ok(value) if value >= min => Ok(()),
        _ => Err(gerror::new(replace_one(
            &input.message,
            "{min}",
            &gf_core::float::format_f(min),
        ))),
    }
}

fn rule_max(input: &RunInput<'_>) -> Result<(), Error> {
    let max = input.rule_pattern.trim().parse::<f64>().unwrap_or(0.0);
    match input.value.string().parse::<f64>() {
        Ok(value) if value <= max => Ok(()),
        _ => Err(gerror::new(replace_one(
            &input.message,
            "{max}",
            &gf_core::float::format_f(max),
        ))),
    }
}

fn rule_float(input: &RunInput<'_>) -> Result<(), Error> {
    if input.value.string().parse::<f64>().is_ok() {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_integer(input: &RunInput<'_>) -> Result<(), Error> {
    if input.value.string().parse::<i64>().is_ok() {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_boolean(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string().to_lowercase();
    if matches!(
        value.as_str(),
        "1" | "true" | "on" | "yes" | "" | "0" | "false" | "off" | "no"
    ) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_in(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    for candidate in gstr::split_and_trim(input.rule_pattern, ",", None) {
        if text_eq(input, &value, candidate.trim()) {
            return Ok(());
        }
    }
    Err(fail(input))
}

fn rule_not_in(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    for candidate in gstr::split_and_trim(input.rule_pattern, ",", None) {
        if text_eq(input, &value, candidate.trim()) {
            return Err(fail(input));
        }
    }
    Ok(())
}

fn rule_ip(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    if gipv4::validate(&value) || gipv6::validate(&value) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_ipv4(input: &RunInput<'_>) -> Result<(), Error> {
    if gipv4::validate(&input.value.string()) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_ipv6(input: &RunInput<'_>) -> Result<(), Error> {
    if gipv6::validate(&input.value.string()) {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_json(input: &RunInput<'_>) -> Result<(), Error> {
    if gf_json::valid(&input.value.bytes()) {
        return Ok(());
    }
    Err(fail(input))
}

// ---------------------------------------------------------------------------
// Field comparison rules
// ---------------------------------------------------------------------------

fn rule_same(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    if !field_value.is_nil() && text_eq(input, &input.value.string(), &gconv::string(&field_value))
    {
        return Ok(());
    }
    Err(compared_message(input, &field_name, &field_value))
}

fn rule_different(input: &RunInput<'_>) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    if field_value.is_nil() || !text_eq(input, &input.value.string(), &gconv::string(&field_value))
    {
        return Ok(());
    }
    Err(compared_message(input, &field_name, &field_value))
}

fn compare_with_field(input: &RunInput<'_>, accept: fn(f64, f64) -> bool) -> Result<(), Error> {
    let (field_name, field_value) = data_item(input, input.rule_pattern);
    let field_number = gconv::string(&field_value).parse::<f64>();
    let value_number = input.value.string().parse::<f64>();
    match (value_number, field_number) {
        (Ok(value), Ok(field)) if accept(value, field) => Ok(()),
        _ => Err(compared_message(input, &field_name, &field_value)),
    }
}

fn rule_gt(input: &RunInput<'_>) -> Result<(), Error> {
    compare_with_field(input, |value, field| value > field)
}

fn rule_gte(input: &RunInput<'_>) -> Result<(), Error> {
    compare_with_field(input, |value, field| value >= field)
}

fn rule_lt(input: &RunInput<'_>) -> Result<(), Error> {
    compare_with_field(input, |value, field| value < field)
}

fn rule_lte(input: &RunInput<'_>) -> Result<(), Error> {
    compare_with_field(input, |value, field| value <= field)
}

// ---------------------------------------------------------------------------
// Required rules
// ---------------------------------------------------------------------------

fn rule_required(input: &RunInput<'_>) -> Result<(), Error> {
    require_if(true, input)
}

fn rule_required_if(input: &RunInput<'_>) -> Result<(), Error> {
    let parts = gstr::split(input.rule_pattern, ",");
    if !parts.len().is_multiple_of(2) {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "invalid \"{}\" rule pattern: {}",
                input.rule_key, input.rule_pattern
            ),
            Vec::new(),
        ));
    }
    let mut required = false;
    for (field, expected) in pairs(input) {
        let (_, found) = data_item(input, &field);
        required = text_eq(input, &expected, &gconv::string(&found));
        if required {
            break;
        }
    }
    require_if(required, input)
}

fn rule_required_if_all(input: &RunInput<'_>) -> Result<(), Error> {
    let parts = gstr::split(input.rule_pattern, ",");
    if !parts.len().is_multiple_of(2) {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "invalid \"{}\" rule pattern: {}",
                input.rule_key, input.rule_pattern
            ),
            Vec::new(),
        ));
    }
    let mut required = true;
    for (field, expected) in pairs(input) {
        let (_, found) = data_item(input, &field);
        if !text_eq(input, &expected, &gconv::string(&found)) {
            required = false;
            break;
        }
    }
    require_if(required, input)
}

fn rule_required_unless(input: &RunInput<'_>) -> Result<(), Error> {
    let parts = gstr::split(input.rule_pattern, ",");
    let mut required = true;
    if parts.len().is_multiple_of(2) {
        for (field, expected) in pairs(input) {
            let (_, found) = data_item(input, &field);
            required = !text_eq(input, &expected, &gconv::string(&found));
            if !required {
                break;
            }
        }
    }
    require_if(required, input)
}

fn rule_required_with(input: &RunInput<'_>) -> Result<(), Error> {
    let mut required = false;
    for field in gstr::split(input.rule_pattern, ",") {
        let (_, found) = data_item(input, field.trim());
        if !empty::is_empty(&found) {
            required = true;
            break;
        }
    }
    require_if(required, input)
}

fn rule_required_with_all(input: &RunInput<'_>) -> Result<(), Error> {
    let mut required = true;
    for field in gstr::split(input.rule_pattern, ",") {
        let (_, found) = data_item(input, field.trim());
        if empty::is_empty(&found) {
            required = false;
            break;
        }
    }
    require_if(required, input)
}

fn rule_required_without(input: &RunInput<'_>) -> Result<(), Error> {
    let mut required = false;
    for field in gstr::split(input.rule_pattern, ",") {
        let (_, found) = data_item(input, field.trim());
        if empty::is_empty(&found) {
            required = true;
            break;
        }
    }
    require_if(required, input)
}

fn rule_required_without_all(input: &RunInput<'_>) -> Result<(), Error> {
    let mut required = true;
    for field in gstr::split(input.rule_pattern, ",") {
        let (_, found) = data_item(input, field.trim());
        if !empty::is_empty(&found) {
            required = false;
            break;
        }
    }
    require_if(required, input)
}

// ---------------------------------------------------------------------------
// Algorithmic rules
// ---------------------------------------------------------------------------

fn rule_bank_card(input: &RunInput<'_>) -> Result<(), Error> {
    let value = input.value.string();
    let digits = value.as_bytes();
    let parity = digits.len() % 2;
    let mut sum = 0i64;
    for (index, byte) in digits.iter().enumerate() {
        let mut digit = i64::from(*byte) - 48;
        if index % 2 == parity {
            digit *= 2;
            if digit > 9 {
                digit -= 9;
            }
        }
        sum += digit;
    }
    if sum % 10 == 0 {
        return Ok(());
    }
    Err(fail(input))
}

fn rule_resident_id(input: &RunInput<'_>) -> Result<(), Error> {
    let id = input.value.string().trim().to_uppercase();
    if id.len() != 18 {
        return Err(fail(input));
    }
    const WEIGHTS: [i64; 17] = [7, 9, 10, 5, 8, 4, 2, 1, 6, 3, 7, 9, 10, 5, 8, 4, 2];
    const CHECK_CODES: [u8; 11] = *b"10X98765432";
    let bytes = id.as_bytes();
    let mut sum = 0i64;
    for index in 0..17 {
        let digit = bytes[index];
        if !digit.is_ascii_digit() {
            return Err(fail(input));
        }
        sum += i64::from(digit - b'0') * WEIGHTS[index];
    }
    if CHECK_CODES[(sum % 11) as usize] != bytes[17] {
        return Err(fail(input));
    }
    let ok = gregex::is_match_string(
        r"(^[1-9]\d{5}(18|19|([23]\d))\d{2}((0[1-9])|(10|11|12))(([0-2][1-9])|10|20|30|31)\d{3}[0-9Xx]$)|(^[1-9]\d{5}\d{2}((0[1-9])|(10|11|12))(([0-2][1-9])|10|20|30|31)\d{3}$)",
        &id,
    );
    if ok {
        return Ok(());
    }
    Err(fail(input))
}

// ---------------------------------------------------------------------------
// Enum rule
// ---------------------------------------------------------------------------

fn rule_enums(input: &RunInput<'_>) -> Result<(), Error> {
    if input.value_type.is_none() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &[r#"value type cannot be empty to use validation rule "enums""#],
        ));
    }
    let type_id = input.value_type.unwrap_or_default();
    let tag_enums = gf_util::gtag::get_enums_by_type(type_id);
    if tag_enums.is_empty() {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_OPERATION,
            format!(
                r#"no enums found for type "{}", missing using command "gf gen enums"?"#,
                type_id
            ),
            Vec::new(),
        ));
    }
    let parsed = gf_json::unmarshal_value(tag_enums.as_bytes())?;
    let names = gconv::strings(&parsed);
    if gstr::in_array(&names, &input.value.string()) {
        return Ok(());
    }
    Err(gerror::new(gstr::replace(
        &input.message,
        "{enums}",
        &tag_enums,
        None,
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_container::gvar::Var;

    fn input<'a>(rule_key: &'a str, pattern: &'a str, value: Value, data: Value) -> RunInput<'a> {
        RunInput {
            rule_key,
            rule_pattern: pattern,
            value: Var::new(value),
            data: Var::new(data),
            message: get_rule(rule_key)
                .map(|rule| rule.message.to_owned())
                .unwrap_or_default(),
            case_insensitive: false,
            value_type: None,
        }
    }

    fn text(value: &str) -> Value {
        Value::String(value.to_owned())
    }

    fn map_of(pairs: &[(&str, Value)]) -> Value {
        let mut map = IndexMap::new();
        for (key, value) in pairs {
            map.insert((*key).to_owned(), value.clone());
        }
        Value::Map(map)
    }

    fn run(rule: &str, pattern: &str, value: Value) -> bool {
        let rule_def = get_rule(rule).expect("known rule");
        (rule_def.run)(&input(rule, pattern, value, Value::Null)).is_ok()
    }

    #[test]
    fn every_rule_is_registered_once() {
        for (index, rule) in RULES.iter().enumerate() {
            assert!(
                RULES
                    .iter()
                    .skip(index + 1)
                    .all(|other| other.name != rule.name)
            );
            assert_eq!(get_rule(rule.name).map(|found| found.name), Some(rule.name));
        }
        assert_eq!(RULES.len(), 67);
        assert!(get_rule("not-a-rule").is_none());
    }

    #[test]
    fn character_rules() {
        assert!(run("alpha", "", text("GoFrame")));
        assert!(!run("alpha", "", text("GoFrame2")));
        assert!(run("alpha-num", "", text("GoFrame2")));
        assert!(!run("alpha-num", "", text("Go Frame")));
        assert!(run("alpha-dash", "", text("go-frame_2")));
        assert!(!run("alpha-dash", "", text("go frame")));
        assert!(run("lowercase", "", text("goframe")));
        assert!(!run("lowercase", "", text("GoFrame")));
        assert!(run("uppercase", "", text("GOFRAME")));
        assert!(run("numeric", "", text("12345")));
        assert!(!run("numeric", "", text("12.3")));
        assert!(run("integer", "", text("-12")));
        assert!(!run("integer", "", text("12.3")));
        assert!(run("float", "", text("12.3")));
        assert!(!run("float", "", text("12a")));
    }

    #[test]
    fn format_rules() {
        assert!(run("email", "", text("john@goframe.org")));
        assert!(!run("email", "", text("john@goframe")));
        assert!(run("domain", "", text("goframe.org")));
        assert!(!run("domain", "", text("goframe")));
        assert!(run("url", "", text("https://goframe.org/docs")));
        assert!(!run("url", "", text("goframe.org")));
        assert!(run("ipv4", "", text("192.168.1.1")));
        assert!(!run("ipv4", "", text("::1")));
        assert!(run("ipv6", "", text("::1")));
        assert!(run("ip", "", text("127.0.0.1")));
        assert!(!run("ip", "", text("127.0.0")));
        assert!(run("mac", "", text("00:11:22:33:44:55")));
        assert!(!run("mac", "", text("00:11:22:33:44")));
        assert!(run("postcode", "", text("100000")));
        assert!(!run("postcode", "", text("10000")));
        assert!(run("qq", "", text("12345")));
        assert!(!run("qq", "", text("012345")));
        assert!(run("telephone", "", text("010-12345678")));
        assert!(!run("telephone", "", text("010-123")));
        assert!(run("phone", "", text("13800138000")));
        assert!(!run("phone", "", text("12800138000")));
        assert!(run("phone-loose", "", text("13800138000")));
        assert!(run("passport", "", text("goframe")));
        assert!(!run("passport", "", text("1goframe")));
        assert!(run("password", "", text("123456")));
        assert!(!run("password", "", text("12345")));
        assert!(run("password2", "", text("GoFrame123")));
        assert!(!run("password2", "", text("goframe123")));
        assert!(run("password3", "", text("GoFrame123!")));
        assert!(!run("password3", "", text("GoFrame123")));
        assert!(run("boolean", "", text("YES")));
        assert!(!run("boolean", "", text("yesno")));
        assert!(run("json", "", text(r#"{"a":1}"#)));
        assert!(!run("json", "", text("{a:1}")));
        assert!(run("array", "", Value::List(vec![Value::Int(1)])));
        assert!(!run("array", "", text("abc")));
        assert!(run("regex", r"^\d+$", text("123")));
        assert!(!run("regex", r"^\d+$", text("12a")));
        assert!(run("not-regex", r"^\d+$", text("12a")));
        assert!(!run("not-regex", r"^\d+$", text("123")));
    }

    #[test]
    fn range_and_length_rules() {
        assert!(run("between", "1,100", text("50")));
        assert!(!run("between", "1,100", text("101")));
        assert!(!run("between", "1,100", text("abc")));
        assert!(run("min", "10", text("10")));
        assert!(run("max", "10", text("10")));
        assert!(!run("max", "10", text("11")));
        assert!(run("length", "2,4", text("abc")));
        assert!(!run("length", "2,4", text("abcde")));
        assert!(run("min-length", "2", text("ab")));
        assert!(run("max-length", "2", text("ab")));
        assert!(!run("max-length", "2", text("abc")));
        assert!(run("size", "3", text("abc")));
        assert!(!run("size", "3", text("ab")));
        // Containers are measured by element count.
        assert!(run(
            "size",
            "2",
            Value::List(vec![Value::Int(1), Value::Int(2)])
        ));
        assert!(run(
            "min-length",
            "2",
            map_of(&[("a", Value::Int(1)), ("b", Value::Int(2))])
        ));
    }

    #[test]
    fn membership_rules() {
        assert!(run("in", "1,2,3", text("2")));
        assert!(!run("in", "1,2,3", text("4")));
        assert!(run("not-in", "1,2,3", text("4")));
        assert!(!run("not-in", "1,2,3", text("2")));

        let mut case_input = input("in", "abc,def", text("ABC"), Value::Null);
        case_input.case_insensitive = true;
        assert!((get_rule("in").expect("rule").run)(&case_input).is_ok());
    }

    #[test]
    fn required_rules_see_the_whole_data() {
        let data = map_of(&[
            ("id", Value::Int(1)),
            ("age", Value::Int(18)),
            ("name", Value::String(String::new())),
        ]);
        let run = |name: &str, pattern: &str, value: Value| {
            let rule = get_rule(name).expect("rule");
            (rule.run)(&input(name, pattern, value, data.clone()))
        };
        assert!(run("required", "", Value::Null).is_err());
        assert!(run("required", "", Value::Int(0)).is_ok());

        assert!(run("required-if", "id,1", Value::String(String::new())).is_err());
        assert!(run("required-if", "id,2", Value::String(String::new())).is_ok());
        assert!(run("required-if", "id", Value::String(String::new())).is_err());

        assert!(run("required-if-all", "id,1,age,18", Value::Null).is_err());
        assert!(run("required-if-all", "id,1,age,19", Value::Null).is_ok());

        assert!(run("required-unless", "id,1", Value::Null).is_ok());

        assert!(run("required-with", "id", Value::Null).is_err());
        assert!(run("required-with-all", "id,missing", Value::Null).is_ok());
        assert!(run("required-without", "missing", Value::Null).is_err());
        assert!(run("required-without-all", "id,missing", Value::Null).is_ok());
    }

    #[test]
    fn comparison_rules() {
        let data = map_of(&[("other", text("5")), ("same", text("abc"))]);
        let rule = get_rule("same").expect("rule");
        let mut same_input = input("same", "same", text("abc"), data.clone());
        same_input.message = rule.message.to_owned();
        assert!((rule.run)(&same_input).is_ok());
        let mut different_input = input("same", "same", text("abd"), data.clone());
        different_input.message = rule.message.to_owned();
        assert!((rule.run)(&different_input).is_err());
        assert!(different_input.message.contains("must be the same"));

        let rule = get_rule("different").expect("rule");
        let mut input_diff = input("different", "same", text("abd"), data.clone());
        input_diff.message = rule.message.to_owned();
        assert!((rule.run)(&input_diff).is_ok());

        for (name, value, expected) in [
            ("gt", "6", true),
            ("gt", "5", false),
            ("gte", "5", true),
            ("lt", "4", true),
            ("lt", "5", false),
            ("lte", "5", true),
        ] {
            let rule = get_rule(name).expect("rule");
            let mut item = input(name, "other", text(value), data.clone());
            item.message = rule.message.to_owned();
            assert_eq!((rule.run)(&item).is_ok(), expected, "rule {name}");
        }
    }

    #[test]
    fn algorithm_and_date_rules() {
        assert!(run("bank-card", "", text("4111111111111111")));
        assert!(!run("bank-card", "", text("4111111111111112")));
        assert!(run("resident-id", "", text("11010519491231002X")));
        assert!(!run("resident-id", "", text("110105194912310021")));
        assert!(run("date", "", text("2024-01-02")));
        assert!(run("date", "", text("20240102")));
        assert!(!run("date", "", text("2024-13-02")));
        assert!(run("datetime", "", text("2024-01-02 12:00:00")));
        assert!(!run("datetime", "", text("2024-01-02")));
        assert!(run("date-format", "Y-m-d", text("2024-01-02")));
        assert!(!run("date-format", "Y-m-d", text("2024/01/02")));
    }

    #[test]
    fn time_comparison_rules() {
        let data = map_of(&[("start", text("2024-01-01 00:00:00"))]);
        let rule = get_rule("after").expect("rule");
        let mut item = input("after", "start", text("2024-01-02 00:00:00"), data.clone());
        item.message = rule.message.to_owned();
        assert!((rule.run)(&item).is_ok());
        let mut item = input("after", "start", text("2023-12-31 00:00:00"), data.clone());
        item.message = rule.message.to_owned();
        assert!((rule.run)(&item).is_err());
        let mut item = input("after", "start", text("not a time"), data);
        item.message = rule.message.to_owned();
        assert!((rule.run)(&item).is_err());
    }

    #[test]
    fn enums_need_a_type_id() {
        let error =
            (get_rule("enums").expect("rule").run)(&input("enums", "", text("1"), Value::Null))
                .expect_err("no type id");
        assert!(error.to_string().contains("value type cannot be empty"));
    }
}
