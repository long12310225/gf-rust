//! Port of `util/gtag`: struct tag names, a global tag-value store and the
//! enum description store.

use std::collections::{BTreeMap, HashMap};
use std::sync::{OnceLock, RwLock};

use gf_errors::gerror;
use gf_json::RawMessage;

/// Default value tag.
pub const DEFAULT: &str = "default";
/// Short name of [`DEFAULT`].
pub const DEFAULT_SHORT: &str = "d";
/// Parameter name tag.
pub const PARAM: &str = "param";
/// Short name of [`PARAM`].
pub const PARAM_SHORT: &str = "p";
/// Validation rule tag.
pub const VALID: &str = "valid";
/// Short name of [`VALID`].
pub const VALID_SHORT: &str = "v";
/// No-validation tag.
pub const NO_VALIDATION: &str = "nv";
/// ORM tag.
pub const ORM: &str = "orm";
/// Command argument tag.
pub const ARG: &str = "arg";
/// Brief description tag.
pub const BRIEF: &str = "brief";
/// Root command tag.
pub const ROOT: &str = "root";
/// Additional description tag.
pub const ADDITIONAL: &str = "additional";
/// Short name of [`ADDITIONAL`].
pub const ADDITIONAL_SHORT: &str = "ad";
/// Route path tag.
pub const PATH: &str = "path";
/// Route method tag.
pub const METHOD: &str = "method";
/// Route domain tag.
pub const DOMAIN: &str = "domain";
/// MIME type tag.
pub const MIME: &str = "mime";
/// Request MIME type tag.
pub const CONSUMES: &str = "consumes";
/// Summary tag.
pub const SUMMARY: &str = "summary";
/// Short name of [`SUMMARY`].
pub const SUMMARY_SHORT: &str = "sm";
/// Second short name of [`SUMMARY`].
pub const SUMMARY_SHORT2: &str = "sum";
/// Description tag.
pub const DESCRIPTION: &str = "description";
/// Short name of [`DESCRIPTION`].
pub const DESCRIPTION_SHORT: &str = "dc";
/// Second short name of [`DESCRIPTION`].
pub const DESCRIPTION_SHORT2: &str = "des";
/// Example tag.
pub const EXAMPLE: &str = "example";
/// Short name of [`EXAMPLE`].
pub const EXAMPLE_SHORT: &str = "eg";
/// Examples tag.
pub const EXAMPLES: &str = "examples";
/// Short name of [`EXAMPLES`].
pub const EXAMPLES_SHORT: &str = "egs";
/// External docs tag.
pub const EXTERNAL_DOCS: &str = "externalDocs";
/// Short name of [`EXTERNAL_DOCS`].
pub const EXTERNAL_DOCS_SHORT: &str = "ed";
/// Conversion target tag.
pub const GCONV: &str = "gconv";
/// Short name of [`GCONV`].
pub const GCONV_SHORT: &str = "c";
/// JSON tag.
pub const JSON: &str = "json";
/// Security tag.
pub const SECURITY: &str = "security";
/// Parameter location tag.
pub const IN: &str = "in";
/// Whole-body tag.
pub const BODY: &str = "body";
/// Value marking a whole-body field.
pub const BODY_VALUE_ALL: &str = "*";
/// Required tag.
pub const REQUIRED: &str = "required";
/// Response status tag.
pub const STATUS: &str = "status";
/// Response example tag.
pub const RESPONSE_EXAMPLE: &str = "responseExample";
/// Short name of [`RESPONSE_EXAMPLE`].
pub const RESPONSE_EXAMPLE_SHORT: &str = "resEg";

/// Tag priority used by map/struct conversion.
pub const STRUCT_TAG_PRIORITY: &[&str] = &[GCONV, PARAM, GCONV_SHORT, PARAM_SHORT, JSON];

fn data() -> &'static RwLock<HashMap<String, String>> {
    static DATA: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
    DATA.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Stores a tag value; panics when `name` already exists, like Go.
pub fn set(name: &str, value: &str) {
    let mut guard = data()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.contains_key(name) {
        panic!(
            "{}",
            gerror::newf(
                "value for tag name \"%s\" already exists",
                vec![gf_core::Value::String(name.to_owned())],
            )
        );
    }
    guard.insert(name.to_owned(), value.to_owned());
}

/// Stores a tag value, overwriting an existing one.
pub fn set_over(name: &str, value: &str) {
    let mut guard = data()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard.insert(name.to_owned(), value.to_owned());
}

/// Stores several tag values; panics on the first duplicate.
pub fn sets(values: &HashMap<String, String>) {
    for (name, value) in values {
        set(name, value);
    }
}

/// Stores several tag values, overwriting existing ones.
pub fn sets_over(values: &HashMap<String, String>) {
    for (name, value) in values {
        set_over(name, value);
    }
}

/// Returns the stored value for `name`, or `""`.
pub fn get(name: &str) -> String {
    data()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(name)
        .cloned()
        .unwrap_or_default()
}

/// Replaces every `{name}` whose name is stored, leaving the rest unchanged.
pub fn parse(content: &str) -> String {
    if !content.contains('{') {
        return content.to_owned();
    }
    let guard = data()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut out = String::with_capacity(content.len());
    let mut rest = content;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        match tail.find('}') {
            Some(close) if close > 0 => {
                let name = &tail[..close];
                match guard.get(name) {
                    Some(value) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &tail[close + 1..];
            }
            _ => {
                out.push('{');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

fn enums() -> &'static RwLock<BTreeMap<String, RawMessage>> {
    static ENUMS: OnceLock<RwLock<BTreeMap<String, RawMessage>>> = OnceLock::new();
    ENUMS.get_or_init(|| RwLock::new(BTreeMap::new()))
}

/// Replaces the global enums from a JSON object.
pub fn set_global_enums(enums_json: &str) -> Result<(), gerror::Error> {
    let parsed: BTreeMap<String, RawMessage> = gf_json::unmarshal(enums_json.as_bytes())?;
    let mut guard = enums()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = parsed;
    Ok(())
}

/// Returns the global enums as JSON text with sorted keys.
pub fn get_global_enums() -> Result<String, gerror::Error> {
    let guard = enums()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    gf_json::marshal_string(&*guard)
}

/// Returns the stored enum JSON for `type_name`, or `""`.
pub fn get_enums_by_type(type_name: &str) -> String {
    enums()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(type_name)
        .map(|raw| raw.get().to_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_parse() {
        set("demo_parse", "content");
        assert_eq!(get("demo_parse"), "content");
        assert_eq!(get("missing_tag"), "");
        assert_eq!(parse("This is {demo_parse}"), "This is content");
        assert_eq!(parse("no placeholder"), "no placeholder");
        assert_eq!(parse("{}"), "{}");
        assert_eq!(parse("{missing_tag}"), "{missing_tag}");
        assert_eq!(parse("{{demo_parse}}"), "{{demo_parse}}");
        assert_eq!(parse("{demo_parse}{demo_parse}"), "contentcontent");
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn set_twice_panics() {
        set("demo_dup", "a");
        set("demo_dup", "b");
    }

    #[test]
    fn set_over_and_sets() {
        set_over("demo_over", "a");
        set_over("demo_over", "b");
        assert_eq!(get("demo_over"), "b");

        let values = HashMap::from([
            ("demo_sets_a".to_owned(), "1".to_owned()),
            ("demo_sets_b".to_owned(), "2".to_owned()),
        ]);
        sets(&values);
        assert_eq!(get("demo_sets_a"), "1");
        sets_over(&values);
        assert_eq!(get("demo_sets_b"), "2");
    }

    #[test]
    fn global_enums_round_trip() {
        set_global_enums(r#"{"uint":1,"string":"s","obj":{"a":1},"bool":true}"#).expect("enums");
        assert_eq!(
            get_global_enums().expect("enums json"),
            r#"{"bool":true,"obj":{"a":1},"string":"s","uint":1}"#
        );
        assert_eq!(get_enums_by_type("uint"), "1");
        assert_eq!(get_enums_by_type("obj"), r#"{"a":1}"#);
        assert_eq!(get_enums_by_type("missing"), "");
        assert!(set_global_enums("[1,2]").is_err());
    }

    #[test]
    fn constants_match_go() {
        assert_eq!(DEFAULT, "default");
        assert_eq!(GCONV_SHORT, "c");
        assert_eq!(BODY_VALUE_ALL, "*");
        assert_eq!(STRUCT_TAG_PRIORITY, ["gconv", "param", "c", "p", "json"]);
    }
}
