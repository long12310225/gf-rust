//! Port of `util/gvalid`.
//!
//! Go's package validates maps, structs and single values, where the rules
//! come from struct tags (`gvalid:"required|length:2,20"`), a rules map or a
//! sequence of tags. Rust has no reflection, so the port keeps the map, value
//! and tag-sequence lanes and replaces struct-tag scanning with an explicit
//! rules map (ADR-039); `CheckStruct` has no Rust counterpart and callers pass
//! a [`Rules::Map`] instead.
//!
//! Divergences, all recorded in ADR-039:
//!
//! - no i18n manager: custom messages and the builtin English messages are
//!   used, and `gf.gvalid.rule.*` translations are not consulted;
//! - `RuleFuncInput` has no `reflect.Type`; the `enums` builtin gets an
//!   optional type id instead, which the value/map lanes leave empty;
//! - the parsed-tag cache (`gvalid.New(true)`) is not implemented because the
//!   parser is a single regex match per tag;
//! - `Error` is a concrete [`ValidationError`] instead of an interface.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};

use gf_container::gvar::Var;
pub use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gcode::Code as _;
use gf_errors::gerror::{self, Error};
use gf_text::{gregex, gstr};
use gf_util::gconv;
pub use indexmap::IndexMap;

mod builtin;

/// Rule name reported for an internal rule-parsing error.
pub const INTERNAL_RULES_RULE_NAME: &str = "InvalidRules";
/// Rule name reported for invalid validation parameters.
pub const INTERNAL_PARAMS_RULE_NAME: &str = "InvalidParams";
/// Rule name reported for an invalid validation object.
pub const INTERNAL_OBJECT_RULE_NAME: &str = "InvalidObject";
/// Field key used for internal errors inside the error map.
pub const INTERNAL_ERROR_MAP_KEY: &str = "__InternalError__";
/// Message used when a rule has no message of its own.
const DEFAULT_INVALID_MESSAGE: &str = "The {field} value `{value}` is invalid";
/// Prefix of the rules that must run even for an empty value.
const REQUIRED_RULES_PREFIX: &str = "required";

/// The custom validation function, like Go's `gvalid.RuleFunc`.
pub type RuleFunc = Arc<dyn Fn(&RuleFuncInput) -> Result<(), Error> + Send + Sync>;

/// The input of a custom rule, like Go's `gvalid.RuleFuncInput`.
///
/// Go also passes the value's `reflect.Type`; the port has no runtime type
/// information and leaves it out.
pub struct RuleFuncInput {
    /// The whole rule text, like `between:1,100`.
    pub rule: String,
    /// The message chosen for this rule.
    pub message: String,
    /// The field being validated.
    pub field: String,
    /// The value being validated.
    pub value: Var,
    /// The validation data the validator was given.
    pub data: Var,
}

/// The rules of one validation, like Go's `rules any`.
#[derive(Clone, Debug)]
pub enum Rules {
    /// One validation tag string, like `required|between:1,100`.
    Text(String),
    /// `map[field]rule`, in insertion order.
    Map(IndexMap<String, String>),
    /// Tag sequence, which keeps the error order.
    Sequence(Vec<String>),
}

impl From<&str> for Rules {
    fn from(rules: &str) -> Rules {
        Rules::Text(rules.to_owned())
    }
}

impl From<String> for Rules {
    fn from(rules: String) -> Rules {
        Rules::Text(rules)
    }
}

impl From<IndexMap<String, String>> for Rules {
    fn from(rules: IndexMap<String, String>) -> Rules {
        Rules::Map(rules)
    }
}

impl From<Vec<String>> for Rules {
    fn from(rules: Vec<String>) -> Rules {
        Rules::Sequence(rules)
    }
}

/// A struct that exposes its validation rules and data, like Go's struct-tag
/// lane; usually derived with `#[derive(Validate)]` (ADR-092).
pub trait Validatable {
    /// The field rules in declaration order.
    fn validation_rules() -> Rules;

    /// The field data as a map value.
    fn validation_data(&self) -> Value;
}

/// Converts one field value into the [`Value`] map used by validation; the
/// derive emits calls to it.
pub trait FieldValue {
    /// The field's [`Value`] representation.
    fn field_value(&self) -> Value;
}

impl FieldValue for Value {
    fn field_value(&self) -> Value {
        self.clone()
    }
}

impl FieldValue for String {
    fn field_value(&self) -> Value {
        Value::String(self.clone())
    }
}

impl FieldValue for str {
    fn field_value(&self) -> Value {
        Value::String(self.to_owned())
    }
}

impl FieldValue for bool {
    fn field_value(&self) -> Value {
        Value::Bool(*self)
    }
}

macro_rules! impl_field_value_int {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldValue for $type {
                fn field_value(&self) -> Value {
                    Value::Int(*self as i64)
                }
            }
        )*
    };
}

impl_field_value_int!(i8, i16, i32, i64, isize);

macro_rules! impl_field_value_uint {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldValue for $type {
                fn field_value(&self) -> Value {
                    Value::UInt(*self as u64)
                }
            }
        )*
    };
}

impl_field_value_uint!(u8, u16, u32, u64, usize);

macro_rules! impl_field_value_float {
    ($($type:ty),* $(,)?) => {
        $(
            impl FieldValue for $type {
                fn field_value(&self) -> Value {
                    Value::Float(*self as f64)
                }
            }
        )*
    };
}

impl_field_value_float!(f32, f64);

impl<T: FieldValue> FieldValue for Option<T> {
    fn field_value(&self) -> Value {
        match self {
            Some(value) => value.field_value(),
            None => Value::Null,
        }
    }
}

impl FieldValue for Vec<String> {
    fn field_value(&self) -> Value {
        Value::List(
            self.iter()
                .map(|item| Value::String(item.clone()))
                .collect(),
        )
    }
}

impl Validator {
    /// Validates a struct through [`Validatable`], the Rust counterpart of
    /// Go's struct-tag lane (ADR-092).
    pub fn check_struct<T: Validatable>(&self, value: &T) -> Result<(), ValidationError> {
        let mut validator = self.clone();
        validator.data = Some(value.validation_data());
        validator.rules = Some(T::validation_rules());
        validator.run()
    }
}

/// Reads [`Rules`] out of a [`Value`], like Go's type switch on `rules any`.
pub fn rules_from_value(value: &Value) -> Option<Rules> {
    match value {
        Value::String(text) => Some(Rules::Text(text.clone())),
        Value::List(items) => Some(Rules::Sequence(
            items.iter().map(gconv::string).collect::<Vec<_>>(),
        )),
        Value::Map(entries) => Some(Rules::Map(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), gconv::string(value)))
                .collect(),
        )),
        _ => None,
    }
}

/// One validated field and its rule text, used to keep the error order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldRule {
    /// The field name.
    pub name: String,
    /// The field's rule text.
    pub rule: String,
}

/// The result of a failed validation, like Go's `gvalid.Error`.
///
/// Go exposes it through an interface and as a `map[field]map[rule]error`.
/// The port keeps one concrete type with the same accessors.
pub struct ValidationError {
    code: gcode::LocalCode,
    rules: Vec<FieldRule>,
    errors: IndexMap<String, IndexMap<String, Error>>,
}

impl ValidationError {
    /// Builds an error from the collected per-field, per-rule messages.
    fn new(
        code: gcode::LocalCode,
        rules: Vec<FieldRule>,
        errors: IndexMap<String, IndexMap<String, Error>>,
    ) -> ValidationError {
        ValidationError {
            code,
            rules,
            errors,
        }
    }

    /// Builds the single-entry error Go creates for internal failures.
    fn internal(key: &str, error: Error) -> ValidationError {
        let mut rule_errors = IndexMap::new();
        rule_errors.insert(key.to_owned(), error);
        let mut errors = IndexMap::new();
        errors.insert(INTERNAL_ERROR_MAP_KEY.to_owned(), rule_errors);
        ValidationError::new(gcode::CODE_INTERNAL_ERROR, Vec::new(), errors)
    }

    /// The error code, like Go's `Error.Code`.
    pub fn code(&self) -> &gcode::LocalCode {
        &self.code
    }

    /// Every field and its rule errors, like Go's `Error.Maps`.
    pub fn maps(&self) -> &IndexMap<String, IndexMap<String, Error>> {
        &self.errors
    }

    /// The first field's rule errors, like Go's `Error.Map`.
    pub fn map(&self) -> &IndexMap<String, Error> {
        self.first_item().1
    }

    /// One entry per field in rule order, like Go's `Error.Items`.
    pub fn items(&self) -> Vec<(String, &IndexMap<String, Error>)> {
        let mut items = Vec::new();
        if self.rules.is_empty() {
            for (name, rule_errors) in &self.errors {
                items.push((name.clone(), rule_errors));
            }
            return items;
        }
        for rule in &self.rules {
            if let Some(rule_errors) = self.errors.get(&rule.name) {
                items.push((rule.name.clone(), rule_errors));
            }
        }
        items
    }

    /// The first failing field and its rule errors, like `FirstItem`.
    pub fn first_item(&self) -> (String, &IndexMap<String, Error>) {
        if !self.rules.is_empty() {
            for rule in &self.rules {
                if let Some(rule_errors) = self.errors.get(&rule.name) {
                    return (rule.name.clone(), rule_errors);
                }
            }
        }
        match self.errors.first() {
            Some((name, rule_errors)) => (name.clone(), rule_errors),
            None => (String::new(), empty_rule_errors()),
        }
    }

    /// Consumes the error and returns the first field with its rule errors.
    fn into_first_item(mut self) -> (String, IndexMap<String, Error>) {
        let name = self.first_item().0;
        let rules = std::mem::take(&mut self.errors);
        match rules.into_iter().find(|(field, _)| *field == name) {
            Some((field, rule_errors)) => (field, rule_errors),
            None => (String::new(), IndexMap::new()),
        }
    }

    /// The first failing rule and its error, like Go's `FirstRule`.
    pub fn first_rule(&self) -> (String, Option<&Error>) {
        if !self.rules.is_empty() {
            for field in &self.rules {
                if let Some(rule_errors) = self.errors.get(&field.name) {
                    for rule_item in gstr::split(&field.rule, "|") {
                        let rule_key = gstr::split(&rule_item, ":")
                            .first()
                            .map(|key| key.trim().to_owned())
                            .unwrap_or_default();
                        if let Some(error) = rule_errors.get(&rule_key) {
                            return (rule_key, Some(error));
                        }
                    }
                }
            }
        }
        if let Some((rule_key, error)) = self
            .errors
            .values()
            .find_map(|rule_errors| rule_errors.iter().next())
        {
            return (rule_key.clone(), Some(error));
        }
        (String::new(), None)
    }

    /// The first error message, like Go's `FirstError`.
    pub fn first_error(&self) -> Option<&Error> {
        self.first_rule().1
    }

    /// Every message in rule order, like Go's `Strings`.
    pub fn strings(&self) -> Vec<String> {
        let mut messages = Vec::new();
        if self.rules.is_empty() {
            for rule_errors in self.errors.values() {
                for error in rule_errors.values() {
                    messages.push(error.to_string());
                }
            }
            return messages;
        }
        for field in &self.rules {
            let Some(rule_errors) = self.errors.get(&field.name) else {
                continue;
            };
            for rule_item in gstr::split(&field.rule, "|") {
                let rule_key = gstr::split(&rule_item, ":")
                    .first()
                    .map(|key| key.trim().to_owned())
                    .unwrap_or_default();
                if let Some(error) = rule_errors.get(&rule_key) {
                    messages.push(error.to_string());
                }
            }
            for internal in [
                INTERNAL_RULES_RULE_NAME,
                INTERNAL_PARAMS_RULE_NAME,
                INTERNAL_OBJECT_RULE_NAME,
            ] {
                if let Some(error) = rule_errors.get(internal) {
                    messages.push(error.to_string());
                }
            }
        }
        messages
    }

    /// The messages joined with `"; "`, like Go's `Error.String`.
    pub fn message(&self) -> String {
        self.strings().join("; ")
    }
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message())
    }
}

impl std::fmt::Debug for ValidationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ValidationError")
            .field("code", &self.code.code())
            .field("message", &self.message())
            .finish()
    }
}

/// An empty rule-error map for the no-error case of [`ValidationError::first_item`].
fn empty_rule_errors() -> &'static IndexMap<String, Error> {
    static EMPTY: OnceLock<IndexMap<String, Error>> = OnceLock::new();
    EMPTY.get_or_init(IndexMap::new)
}

/// The messages a `check_value` call uses, like Go's `messages any`.
#[derive(Clone, Default)]
struct CustomMessages {
    /// Messages by rule index, from a `|` separated string.
    sequence: Vec<String>,
    /// Messages by rule name, from a map.
    by_rule: IndexMap<String, String>,
}

impl CustomMessages {
    /// Normalizes the `messages` argument the way Go's `doCheckValue` does.
    fn from_value(messages: Option<&Value>) -> CustomMessages {
        let mut parsed = CustomMessages::default();
        match messages {
            Some(Value::String(text)) => {
                parsed.sequence = gstr::split(text, "|");
            }
            Some(Value::Map(entries)) => {
                for (rule, message) in entries {
                    parsed.by_rule.insert(rule.clone(), gconv::string(message));
                }
            }
            Some(other) => {
                let map = gconv::map(other);
                for (rule, message) in map {
                    parsed.by_rule.insert(rule, gconv::string(&message));
                }
            }
            None => {}
        }
        parsed
    }
}

/// The global custom rules registered through [`register_rule`].
fn custom_rules() -> &'static RwLock<HashMap<String, RuleFunc>> {
    static RULES: OnceLock<RwLock<HashMap<String, RuleFunc>>> = OnceLock::new();
    RULES.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Registers a package-level custom rule, like Go's `RegisterRule`.
pub fn register_rule(rule: impl Into<String>, function: RuleFunc) {
    let mut rules = custom_rules()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    rules.insert(rule.into(), function);
}

/// Registers several package-level custom rules, like `RegisterRuleByMap`.
pub fn register_rule_by_map(rules: HashMap<String, RuleFunc>) {
    let mut registered = custom_rules()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for (rule, function) in rules {
        registered.insert(rule, function);
    }
}

/// Returns a copy of the package-level custom rules, like `GetRegisteredRuleMap`.
pub fn get_registered_rule_map() -> HashMap<String, RuleFunc> {
    custom_rules()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Deletes package-level custom rules, like Go's `DeleteRule`.
pub fn delete_rule(rules: &[&str]) {
    let mut registered = custom_rules()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for rule in rules {
        registered.remove(*rule);
    }
}

/// Parses one validation tag into field, rule and message, like
/// `ParseTagValue`.
///
/// The sequence tag is `[alias@]rule[#msg]`, for example
/// `name@required|length:2,20#name is required`.
pub fn parse_tag_value(tag: &str) -> (String, String, String) {
    let pattern = r"\s*((\w+)\s*@){0,1}\s*([^#]+)\s*(#\s*(.*)){0,1}\s*";
    // Unmatched optional groups come back as "", so the message group is
    // read by position like Go does.
    match gregex::match_string(pattern, tag) {
        Ok(Some(captures)) if !captures.is_empty() => (
            captures
                .get(2)
                .map(|text| text.trim())
                .unwrap_or_default()
                .to_owned(),
            captures
                .get(3)
                .map(|text| text.trim())
                .unwrap_or_default()
                .to_owned(),
            captures
                .get(5)
                .map(|text| text.trim())
                .unwrap_or_default()
                .to_owned(),
        ),
        _ => (String::new(), String::new(), String::new()),
    }
}

/// Splits one rule item into its name and pattern, like `parseRuleItem`.
///
/// The name is the leading ASCII `[\w-]+` run; `:` starts the pattern, and
/// anything else (including `|`) stays in it.
pub fn parse_rule_item(rule_item: &str) -> (String, String) {
    let bytes = rule_item.as_bytes();
    let mut end = 0;
    while end < bytes.len() {
        let byte = bytes[end];
        let name_byte = byte.is_ascii_digit()
            || byte.is_ascii_uppercase()
            || byte.is_ascii_lowercase()
            || byte == b'_'
            || byte == b'-';
        if !name_byte {
            break;
        }
        end += 1;
    }
    if end > 0 {
        let mut pattern = "";
        if end < bytes.len() && bytes[end] == b':' {
            pattern = &rule_item[end + 1..];
        } else if end < bytes.len() {
            pattern = &rule_item[end..];
        }
        return (
            gstr::trim(&rule_item[..end], None),
            gstr::trim(pattern, None),
        );
    }
    if let Some(index) = rule_item.find(':') {
        return (
            gstr::trim(&rule_item[..index], None),
            gstr::trim(&rule_item[index + 1..], None),
        );
    }
    (gstr::trim(rule_item, None), String::new())
}

/// The validation manager, like Go's `gvalid.Validator`.
#[derive(Clone, Default)]
pub struct Validator {
    data: Option<Value>,
    assoc: Option<Value>,
    rules: Option<Rules>,
    messages: Option<Value>,
    rule_func_map: HashMap<String, RuleFunc>,
    bail: bool,
    foreach: bool,
    case_insensitive: bool,
}

impl Validator {
    /// Creates a validator with the package-level custom rules available.
    ///
    /// Go's optional `cached` argument is not ported (ADR-039).
    pub fn new() -> Validator {
        Validator::default()
    }

    /// Sets the data to validate, ignoring a null value like Go's `Data`.
    pub fn data(mut self, data: Value) -> Validator {
        if !data.is_nil() {
            self.data = Some(data);
        }
        self
    }

    /// Sets the associated data, like Go's `Assoc`.
    pub fn assoc(mut self, assoc: Value) -> Validator {
        if !assoc.is_nil() {
            self.assoc = Some(assoc);
        }
        self
    }

    /// Sets the rules, like Go's `Rules`.
    pub fn rules(mut self, rules: impl Into<Rules>) -> Validator {
        self.rules = Some(rules.into());
        self
    }

    /// Sets custom messages, like Go's `Messages`.
    pub fn messages(mut self, messages: Value) -> Validator {
        if !messages.is_nil() {
            self.messages = Some(messages);
        }
        self
    }

    /// Stops at the first validation error, like Go's `Bail`.
    pub fn bail(mut self) -> Validator {
        self.bail = true;
        self
    }

    /// Validates each element of the next rule's value, like `Foreach`.
    pub fn foreach(mut self) -> Validator {
        self.foreach = true;
        self
    }

    /// Makes comparison rules ignore case, like Go's `Ci`.
    pub fn ci(mut self) -> Validator {
        self.case_insensitive = true;
        self
    }

    /// Registers one custom rule on this validator, like `RuleFunc`.
    pub fn rule_func(mut self, rule: impl Into<String>, function: RuleFunc) -> Validator {
        self.rule_func_map.insert(rule.into(), function);
        self
    }

    /// Registers several custom rules on this validator, like `RuleFuncMap`.
    pub fn rule_func_map(
        mut self,
        rules: impl IntoIterator<Item = (String, RuleFunc)>,
    ) -> Validator {
        for (rule, function) in rules {
            self.rule_func_map.insert(rule, function);
        }
        self
    }

    /// The custom rule for `rule`: this validator's first, then the package one.
    fn custom_rule(&self, rule: &str) -> Option<RuleFunc> {
        if let Some(function) = self.rule_func_map.get(rule) {
            return Some(Arc::clone(function));
        }
        custom_rules()
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(rule)
            .map(Arc::clone)
    }

    /// Reports whether `rule` must run for an empty value, like
    /// `checkRuleRequired`.
    fn check_rule_required(&self, rule: &str) -> bool {
        rule.starts_with(REQUIRED_RULES_PREFIX)
            || custom_rules()
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(rule)
            || self.rule_func_map.contains_key(rule)
    }

    /// The message for `rule`: custom first, then the builtin default.
    fn message_for(&self, rule: &str, custom: &CustomMessages) -> String {
        if let Some(message) = custom.by_rule.get(rule).filter(|text| !text.is_empty()) {
            return message.clone();
        }
        if let Some(rule) = builtin::get_rule(rule).filter(|rule| !rule.message.is_empty()) {
            return rule.message.to_owned();
        }
        DEFAULT_INVALID_MESSAGE.to_owned()
    }

    /// Validates the data set with [`Validator::data`], like Go's `Run`.
    pub fn run(&self) -> Result<(), ValidationError> {
        let Some(data) = &self.data else {
            return Err(ValidationError::internal(
                INTERNAL_PARAMS_RULE_NAME,
                gerror::new("no data passed for validation"),
            ));
        };
        let map_lane = matches!(
            self.rules,
            None | Some(Rules::Map(_)) | Some(Rules::Sequence(_))
        );
        if map_lane && matches!(data, Value::Map(_)) {
            return self.do_check_map(data);
        }
        let rule = match &self.rules {
            Some(Rules::Text(text)) => text.clone(),
            _ => String::new(),
        };
        if rule.trim().is_empty() {
            return Ok(());
        }
        let assoc = self.assoc.clone().unwrap_or(Value::Null);
        self.do_check_value(
            "",
            data,
            &rule,
            &CustomMessages::from_value(self.messages.as_ref()),
            &assoc,
        )
    }

    /// Validates `data` against this validator's rules, like `CheckMap`.
    pub fn check_map(&self, data: &Value) -> Result<(), ValidationError> {
        self.do_check_map(data)
    }

    /// Validates one value against one rule, like Go's `CheckValue`.
    pub fn check_value(
        &self,
        name: &str,
        value: &Value,
        rule: &str,
    ) -> Result<(), ValidationError> {
        let assoc = self.assoc.clone().unwrap_or(Value::Null);
        self.do_check_value(
            name,
            value,
            rule,
            &CustomMessages::from_value(self.messages.as_ref()),
            &assoc,
        )
    }

    /// The map lane of [`Validator::run`], like Go's `doCheckMap`.
    fn do_check_map(&self, data: &Value) -> Result<(), ValidationError> {
        let params = gconv::map(data);
        if params.is_empty() {
            return Err(ValidationError::internal(
                INTERNAL_PARAMS_RULE_NAME,
                gerror::new("invalid params type: convert to map failed"),
            ));
        }
        let mut check_rules: Vec<FieldRule> = Vec::new();
        let mut custom_message: IndexMap<String, IndexMap<String, String>> = IndexMap::new();
        match &self.rules {
            Some(Rules::Sequence(tags)) => {
                for tag in tags {
                    let (name, rule, message) = parse_tag_value(tag);
                    if name.is_empty() {
                        continue;
                    }
                    if !message.is_empty() {
                        let messages = gstr::split(&message, "|");
                        let rule_items = gstr::split(&rule, "|");
                        for (index, rule_item) in rule_items.iter().enumerate() {
                            let Some(message) = messages.get(index) else {
                                continue;
                            };
                            if message.is_empty() {
                                continue;
                            }
                            let rule_key = gstr::split(rule_item, ":")
                                .first()
                                .map(|key| key.trim().to_owned())
                                .unwrap_or_default();
                            custom_message
                                .entry(name.clone())
                                .or_default()
                                .insert(rule_key, message.trim().to_owned());
                        }
                    }
                    check_rules.push(FieldRule { name, rule });
                }
            }
            Some(Rules::Map(entries)) => {
                for (name, rule) in entries {
                    check_rules.push(FieldRule {
                        name: name.clone(),
                        rule: rule.clone(),
                    });
                }
            }
            _ => {}
        }
        // `Messages` may carry per-field rule messages, like Go's CustomMsg.
        if let Some(Value::Map(messages)) = &self.messages {
            for (field, message) in messages {
                let rules = custom_message.entry(field.clone()).or_default();
                match message {
                    Value::String(text) => {
                        rules.insert("__message__".to_owned(), text.clone());
                    }
                    Value::Map(entries) => {
                        for (rule, text) in entries {
                            rules.insert(rule.clone(), gconv::string(text));
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut error_maps: IndexMap<String, IndexMap<String, Error>> = IndexMap::new();
        for check_rule in &check_rules {
            if check_rule.rule.is_empty() {
                continue;
            }
            let value = params.get(&check_rule.name).cloned().unwrap_or(Value::Null);
            let messages = custom_message
                .get(&check_rule.name)
                .map(|rules| CustomMessages {
                    sequence: Vec::new(),
                    by_rule: rules.clone(),
                })
                .unwrap_or_default();
            if let Err(validated) =
                self.do_check_value(&check_rule.name, &value, &check_rule.rule, &messages, data)
            {
                let (_, error_item) = validated.into_first_item();
                // In map validation an empty value clears the error of every
                // rule except `required*` and custom rules.
                if gconv::string(&value).is_empty() {
                    let mut required = false;
                    for rule_key in error_item.keys() {
                        if self.check_rule_required(rule_key) {
                            required = true;
                            break;
                        }
                    }
                    if !required {
                        continue;
                    }
                }
                let entry = error_maps.entry(check_rule.name.clone()).or_default();
                for (rule_key, error) in error_item {
                    entry.insert(rule_key, error);
                }
                if self.bail {
                    break;
                }
            }
        }
        if error_maps.is_empty() {
            return Ok(());
        }
        Err(ValidationError::new(
            gcode::CODE_VALIDATION_FAILED,
            check_rules,
            error_maps,
        ))
    }

    /// Runs one rule chain against one value, like Go's `doCheckValue`.
    fn do_check_value(
        &self,
        name: &str,
        value: &Value,
        rule: &str,
        messages: &CustomMessages,
        data_raw: &Value,
    ) -> Result<(), ValidationError> {
        if rule.is_empty() {
            return Ok(());
        }
        let mut rule_items: Vec<String> = gstr::split(rule.trim(), "|");
        self.merge_regex_items(&mut rule_items)?;

        let mut has_bail = self.bail;
        let mut has_foreach = self.foreach;
        let mut has_case_insensitive = self.case_insensitive;
        let mut custom = messages.clone();
        let mut rule_errors: IndexMap<String, Error> = IndexMap::new();
        let mut index = 0;
        'rules: while index < rule_items.len() {
            let (rule_key, rule_pattern) = parse_rule_item(&rule_items[index]);
            if !has_bail && rule_key == "bail" {
                has_bail = true;
            }
            if !has_foreach && rule_key == "foreach" {
                has_foreach = true;
            }
            if !has_case_insensitive && rule_key == "ci" {
                has_case_insensitive = true;
            }
            if builtin::get_rule(&rule_key)
                .map(|rule| rule.message.is_empty())
                .unwrap_or(false)
            {
                // `bail`, `ci` and `foreach` only mark behaviour.
                index += 1;
                continue;
            }
            if index < custom.sequence.len() {
                custom
                    .by_rule
                    .insert(rule_key.clone(), custom.sequence[index].trim().to_owned());
            }
            let message = self.message_for(&rule_key, &custom);
            let values = if has_foreach {
                has_foreach = false;
                interfaces(value)
            } else {
                vec![value.clone()]
            };
            for item in values {
                let custom_rule = self.custom_rule(&rule_key);
                let builtin_rule = builtin::get_rule(&rule_key);
                let result = if let Some(function) = &custom_rule {
                    function(&RuleFuncInput {
                        rule: rule_items[index].clone(),
                        message: message.clone(),
                        field: name.to_owned(),
                        value: Var::new(item.clone()),
                        data: Var::new(data_raw.clone()),
                    })
                } else if let Some(rule) = builtin_rule {
                    (rule.run)(&builtin::RunInput {
                        rule_key: &rule_key,
                        rule_pattern: &rule_pattern,
                        value: Var::new(item.clone()),
                        data: Var::new(data_raw.clone()),
                        message: message.clone(),
                        case_insensitive: has_case_insensitive,
                        value_type: None,
                    })
                } else {
                    Ok(())
                };
                if let Err(error) = result {
                    let mut text = error.to_string();
                    if gstr::contains(&text, "{") {
                        let mut replaces = IndexMap::new();
                        replaces.insert("{field}".to_owned(), name.to_owned());
                        replaces.insert("{value}".to_owned(), gconv::string(&item));
                        replaces.insert("{pattern}".to_owned(), rule_pattern.clone());
                        replaces.insert("{attribute}".to_owned(), name.to_owned());
                        text = gstr::replace_by_map(&text, &replaces);
                        if let Ok(collapsed) = gregex::replace_string(r"\s{2,}", " ", &text) {
                            text = collapsed;
                        }
                    }
                    let code = gerror::code(&error);
                    let error = if code == gcode::CODE_NIL {
                        gerror::new_code(&gcode::CODE_VALIDATION_FAILED, &[&text])
                    } else {
                        gerror::new_code(&code, &[&text])
                    };
                    rule_errors.insert(rule_key.clone(), error);
                    if has_bail {
                        break 'rules;
                    }
                }
            }
            index += 1;
        }
        if rule_errors.is_empty() {
            return Ok(());
        }
        let mut errors = IndexMap::new();
        errors.insert(name.to_owned(), rule_errors);
        Err(ValidationError::new(
            gcode::CODE_VALIDATION_FAILED,
            vec![FieldRule {
                name: name.to_owned(),
                rule: rule.to_owned(),
            }],
            errors,
        ))
    }

    /// Merges the pieces of a `regex`/`not-regex` pattern that a `|` split
    /// broke apart, like Go's special case in `doCheckValue`.
    fn merge_regex_items(&self, rule_items: &mut Vec<String>) -> Result<(), ValidationError> {
        let mut index = 0;
        loop {
            let rule_key = gstr::split(&rule_items[index], ":")
                .first()
                .map(|key| key.trim().to_owned())
                .unwrap_or_default();
            let known =
                builtin::get_rule(&rule_key).is_some() || self.custom_rule(&rule_key).is_some();
            if !known {
                let merges = index > 0 && {
                    let previous = &rule_items[index - 1];
                    previous.starts_with("regex") || previous.starts_with("not-regex")
                };
                if merges {
                    let tail = rule_items.remove(index);
                    rule_items[index - 1] = format!("{}|{}", rule_items[index - 1], tail);
                } else {
                    let mut rule_errors = IndexMap::new();
                    rule_errors.insert(
                        INTERNAL_RULES_RULE_NAME.to_owned(),
                        gerror::new(format!(
                            "{}: {}",
                            INTERNAL_RULES_RULE_NAME, rule_items[index]
                        )),
                    );
                    let mut errors = IndexMap::new();
                    errors.insert(INTERNAL_ERROR_MAP_KEY.to_owned(), rule_errors);
                    return Err(ValidationError::new(
                        gcode::CODE_INTERNAL_ERROR,
                        Vec::new(),
                        errors,
                    ));
                }
            } else {
                index += 1;
            }
            if index == rule_items.len() {
                return Ok(());
            }
        }
    }
}

/// Expands one value into the elements a `foreach` rule validates, like Go's
/// `gconv.Interfaces`.
fn interfaces(value: &Value) -> Vec<Value> {
    match value {
        Value::List(items) => items.clone(),
        Value::Bytes(bytes) => match gf_json::unmarshal_value(bytes) {
            Ok(Value::List(items)) => items,
            _ => vec![value.clone()],
        },
        Value::String(text) => match gf_json::unmarshal_value(text.as_bytes()) {
            Ok(Value::List(items)) => items,
            _ => gstr::split(text, ",")
                .into_iter()
                .map(Value::String)
                .collect(),
        },
        Value::Map(entries) => entries.values().cloned().collect(),
        other => vec![other.clone()],
    }
}

/// Validates `data` with `rules`, like Go's package-level `Check`.
pub fn check(data: &Value, rules: impl Into<Rules>) -> Result<(), ValidationError> {
    Validator::new().data(data.clone()).rules(rules).run()
}

/// Validates `data` with `rules` and custom `messages`, like `Check` with the
/// variadic message argument.
pub fn check_with_messages(
    data: &Value,
    rules: impl Into<Rules>,
    messages: Value,
) -> Result<(), ValidationError> {
    Validator::new()
        .data(data.clone())
        .rules(rules)
        .messages(messages)
        .run()
}

/// Validates map `data` with `rules`, like Go's package-level `CheckMap`.
pub fn check_map(data: &Value, rules: impl Into<Rules>) -> Result<(), ValidationError> {
    Validator::new().rules(rules).check_map(data)
}

/// Validates one value, like Go's package-level `CheckValue`.
pub fn check_value(name: &str, value: &Value, rule: &str) -> Result<(), ValidationError> {
    Validator::new().check_value(name, value, rule)
}

/// Reports whether `data` passes `rules`, like Go's `IsValid`.
pub fn is_valid(data: &Value, rules: impl Into<Rules>) -> bool {
    check(data, rules).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn rules_map(pairs: &[(&str, &str)]) -> Rules {
        let mut map = IndexMap::new();
        for (field, rule) in pairs {
            map.insert((*field).to_owned(), (*rule).to_owned());
        }
        Rules::Map(map)
    }

    // Translated from the required/rule chains of
    // gvalid_z_unit_feature_rule_test.go.
    #[test]
    fn check_value_runs_rule_chains() {
        let error = check_value("name", &text(""), "required").expect_err("empty");
        assert_eq!(error.message(), "The name field is required");
        assert_eq!(error.first_rule().0, "required");
        assert_eq!(error.code(), &gcode::CODE_VALIDATION_FAILED);
        assert!(check_value("name", &text("john"), "required").is_ok());

        let error =
            check_value("age", &Value::Int(101), "required|between:1,100").expect_err("range");
        assert_eq!(
            error.message(),
            "The age value `101` must be between 1 and 100"
        );
        assert_eq!(error.first_rule().0, "between");
    }

    #[test]
    fn custom_messages_by_index_and_map() {
        let validator = Validator::new().messages(text("name is required|too short"));
        let error = validator
            .check_value("name", &text("abc"), "required|min-length:6")
            .expect_err("short");
        assert_eq!(error.message(), "too short");

        let validator =
            Validator::new().messages(map_of(&[("min-length", text("min length is 6"))]));
        let error = validator
            .check_value("name", &text("abc"), "required|min-length:6")
            .expect_err("short");
        assert_eq!(error.message(), "min length is 6");
    }

    #[test]
    fn bail_stops_after_the_first_error() {
        let error = Validator::new()
            .bail()
            .check_value("name", &text(""), "required|between:1,100")
            .expect_err("first error");
        assert_eq!(error.strings().len(), 1);
        assert_eq!(error.first_rule().0, "required");
    }

    #[test]
    fn foreach_validates_every_element() {
        let error = Validator::new()
            .foreach()
            .check_value(
                "ids",
                &Value::List(vec![Value::Int(1), Value::Int(4)]),
                "foreach|in:1,2,3",
            )
            .expect_err("4 is out of range");
        assert!(error.message().contains("`4`"));

        let values = Value::List(vec![Value::Int(1), Value::Int(2)]);
        assert!(
            Validator::new()
                .foreach()
                .check_value("ids", &values, "foreach|in:1,2,3")
                .is_ok()
        );
    }

    #[test]
    fn case_insensitive_comparisons() {
        assert!(check_value("code", &text("ABC"), "in:abc,def").is_err());
        assert!(
            Validator::new()
                .ci()
                .check_value("code", &text("ABC"), "in:abc,def")
                .is_ok()
        );
    }

    #[test]
    fn unknown_rules_are_internal_errors() {
        let error = check_value("name", &text("x"), "not-a-rule").expect_err("unknown");
        assert_eq!(error.first_rule().0, INTERNAL_RULES_RULE_NAME);
    }

    // Translated from gvalid_z_unit_feature_checkmap_test.go.
    #[test]
    fn check_map_reports_per_field_errors() {
        let data = map_of(&[
            ("name", text("")),
            ("age", Value::Int(200)),
            ("passport", text("abc")),
        ]);
        let error = check_map(
            &data,
            rules_map(&[
                ("name", "required"),
                ("age", "required|between:1,100"),
                ("passport", "required|length:6,16"),
            ]),
        )
        .expect_err("three fields fail");
        let maps = error.maps();
        assert_eq!(maps.len(), 3);
        assert!(maps.contains_key("name"));
        assert!(maps.contains_key("age"));
        assert!(maps.contains_key("passport"));
        assert_eq!(
            maps["age"]["between"].to_string(),
            "The age value `200` must be between 1 and 100"
        );
    }

    // In map validation an empty value with only optional rules is skipped.
    #[test]
    fn check_map_skips_optional_empty_values() {
        let data = map_of(&[("name", text("")), ("age", Value::Int(10))]);
        assert!(
            check_map(
                &data,
                rules_map(&[("name", "min-length:2"), ("age", "required|between:1,100")])
            )
            .is_ok()
        );
    }

    #[test]
    fn sequence_rules_keep_their_order() {
        let data = map_of(&[("a", text("")), ("b", text(""))]);
        let error = check_map(
            &data,
            Rules::Sequence(vec!["b@required".to_owned(), "a@required".to_owned()]),
        )
        .expect_err("both required");
        let items = error.items();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].0, "b");
        assert_eq!(items[1].0, "a");
        assert_eq!(
            error.strings(),
            vec![
                "The b field is required".to_owned(),
                "The a field is required".to_owned()
            ]
        );
    }

    #[test]
    fn run_dispatches_between_value_and_map_lanes() {
        let data = map_of(&[("name", text(""))]);
        let error = Validator::new()
            .data(data)
            .rules(rules_map(&[("name", "required")]))
            .run()
            .expect_err("required");
        assert!(error.message().contains("name"));

        let error = Validator::new()
            .data(text(""))
            .rules("required")
            .run()
            .expect_err("required");
        assert_eq!(error.message(), "The field is required");

        assert!(
            Validator::new()
                .data(text("john"))
                .rules("required")
                .run()
                .is_ok()
        );

        let error = Validator::new().run().expect_err("no data");
        assert!(error.message().contains("no data passed"));
    }

    #[test]
    fn custom_rules_run_per_validator_and_globally() {
        let even = Arc::new(|input: &RuleFuncInput| {
            if input.value.int64() % 2 == 0 {
                Ok(())
            } else {
                Err(gerror::new("must be even"))
            }
        });
        let validator = Validator::new().rule_func("even", even);
        assert!(validator.check_value("n", &Value::Int(2), "even").is_ok());
        let error = validator
            .check_value("n", &Value::Int(3), "even")
            .expect_err("odd");
        assert_eq!(error.message(), "must be even");

        register_rule("always-ok", Arc::new(|_: &RuleFuncInput| Ok(())));
        assert!(check_value("n", &Value::Int(3), "always-ok").is_ok());
        delete_rule(&["always-ok"]);
        assert!(check_value("n", &Value::Int(3), "always-ok").is_err());
    }

    #[test]
    fn parse_helpers_match_go() {
        assert_eq!(
            parse_tag_value("name@required|length:2,20#Name is required"),
            (
                "name".to_owned(),
                "required|length:2,20".to_owned(),
                "Name is required".to_owned()
            )
        );
        assert_eq!(
            parse_tag_value("required#only a message"),
            (
                String::new(),
                "required".to_owned(),
                "only a message".to_owned()
            )
        );
        assert_eq!(
            parse_rule_item("required"),
            ("required".to_owned(), String::new())
        );
        assert_eq!(
            parse_rule_item("between:1,100"),
            ("between".to_owned(), "1,100".to_owned())
        );
        assert_eq!(
            parse_rule_item(r"regex:^\d+:\w+$"),
            ("regex".to_owned(), r"^\d+:\w+$".to_owned())
        );
        assert_eq!(
            parse_rule_item("not-regex|foo"),
            ("not-regex".to_owned(), "|foo".to_owned())
        );
    }

    #[test]
    fn regex_patterns_survive_the_pipe_split() {
        let error = check_value("code", &text("xyz"), r"regex:^a|b$").expect_err("no match");
        // The pattern is merged back into the previous rule, so the error is
        // the rule's own message, not an InvalidRules internal error.
        assert!(error.maps().contains_key("code"));
        assert_eq!(error.first_rule().0, "regex");
        assert!(check_value("code", &text("ab"), r"regex:^a|b$").is_ok());
    }

    #[test]
    fn error_accessors() {
        let data = map_of(&[("name", text(""))]);
        let error = check_map(&data, rules_map(&[("name", "required")])).expect_err("required");
        assert_eq!(error.maps().len(), 1);
        assert_eq!(error.map().len(), 1);
        assert_eq!(error.first_item().0, "name");
        assert!(error.first_error().is_some());
        assert_eq!(error.message(), "The name field is required");
        assert_eq!(error.to_string(), error.message());
    }

    #[test]
    fn translated_rule_tables() {
        let check =
            |value: &str, rule: &str| Validator::new().data(text(value)).rules(rule).run().is_ok();

        for (value, expected) in [
            ("1361990897", false),
            ("13619908979", true),
            ("16719908979", true),
            ("19719908989", true),
        ] {
            assert_eq!(check(value, "phone"), expected, "phone {value}");
        }
        for (value, expected) in [
            ("13333333333", true),
            ("15555555555", true),
            ("16666666666", true),
            ("23333333333", false),
            ("1333333333", false),
            ("10333333333", false),
        ] {
            assert_eq!(check(value, "phone-loose"), expected, "phone-loose {value}");
        }
        for (value, expected) in [
            ("869265", false),
            ("028-869265", false),
            ("86292651", true),
            ("028-8692651", true),
            ("0830-8692651", true),
        ] {
            assert_eq!(check(value, "telephone"), expected, "telephone {value}");
        }
        assert!(!check("12345", "postcode"));
        assert!(check("610036", "postcode"));
        for (value, expected) in [
            ("100", false),
            ("1", false),
            ("10000", true),
            ("38996181", true),
            ("389961817", true),
        ] {
            assert_eq!(check(value, "qq"), expected, "qq {value}");
        }
        for (value, expected) in [
            ("12345", false),
            ("aaaaa", false),
            ("a12345-6", true),
            ("a123_456", true),
        ] {
            assert_eq!(check(value, "password"), expected, "password {value}");
        }
        for (value, expected) in [
            ("Naaaa", false),
            ("a12345-6", false),
            ("Nant1986", true),
            ("Nant1986!", true),
        ] {
            assert_eq!(check(value, "password2"), expected, "password2 {value}");
        }
        for (value, expected) in [("Nant1986", false), ("Nant1986!", true)] {
            assert_eq!(check(value, "password3"), expected, "password3 {value}");
        }
        for (value, expected) in [
            ("11111111111111", false),
            ("1111111111111111", false),
            ("510521198607185367", false),
            ("51052119860718536x", true),
        ] {
            assert_eq!(check(value, "resident-id"), expected, "resident-id {value}");
        }
        assert!(!check("6230514630000424470", "bank-card"));
        assert!(check("6230514630000424473", "bank-card"));

        for (value, expected) in [
            ("2010", false),
            ("201011", false),
            ("20101101", true),
            ("2010-11-01", true),
            ("2010.11.01", true),
            ("2010/11/01", true),
            ("2010=11=01", false),
            ("2026-13-33", false),
            ("2026-02-29", false),
            ("2024-02-29", true),
        ] {
            assert_eq!(check(value, "date"), expected, "date {value}");
        }
        for (value, expected) in [
            ("2010", false),
            ("2010-11-01", false),
            ("2010-11-01 12:00", false),
            ("2010-11-01 12:00:00", true),
        ] {
            assert_eq!(check(value, "datetime"), expected, "datetime {value}");
        }
        for (value, rule) in [
            ("2010", "date-format:Y"),
            ("201011", "date-format:Ym"),
            ("2010.11", "date-format:Y.m"),
            ("201011-01", "date-format:Ym-d"),
            ("2010~11~01", "date-format:Y~m~d"),
            (
                "2023-09-10T19:46:31Z",
                "date-format:2006-01-02\\T15:04:05Z07:00",
            ),
        ] {
            assert!(check(value, rule), "date-format {value} / {rule}");
        }
        assert!(!check("2010-11~01", "date-format:Y~m~d"));

        let required = |rule: &str, pairs: &[(&str, Value)]| {
            Validator::new()
                .data(text(""))
                .rules(rule)
                .assoc(map_of(pairs))
                .run()
                .is_ok()
        };
        let one = [("id", Value::Int(1))];
        let zero = [("id", Value::Int(0))];
        let age = [("age", Value::Int(18))];
        let older = [("age", Value::Int(20))];
        assert!(!required("required-if:id,1,age,18", &one));
        assert!(required("required-if:id,1,age,18", &zero));
        assert!(!required("required-if:id,1,age,18", &age));
        assert!(required("required-if:id,1,age,18", &older));
        assert!(required("required-if-all:id,1,age,18", &one));
        assert!(required("required-if-all:id,1,age,18", &zero));
        let both = [("id", Value::Int(1)), ("age", Value::Int(18))];
        assert!(!required("required-if-all:id,1,age,18", &both));
        assert!(!required("required-unless:id,1,age,18", &zero));
        assert!(required("required-unless:id,1,age,18", &one));

        let with = [("id", Value::Int(100)), ("name", text("john"))];
        let id_only = [("id", Value::Int(100))];
        assert!(required("required-with:id,name", &age));
        assert!(!required("required-with:id,name", &id_only));
        assert!(!required("required-with:id,name", &with));
        assert!(required("required-with-all:id,name", &id_only));
        assert!(!required("required-with-all:id,name", &with));
        assert!(!required("required-without:id,name", &age));
        assert!(!required("required-without:id,name", &id_only));
        assert!(required("required-without:id,name", &with));
        assert!(!required("required-without-all:id,name", &age));
        assert!(required("required-without-all:id,name", &id_only));
        assert!(required("required-without-all:id,name", &with));
    }
}
