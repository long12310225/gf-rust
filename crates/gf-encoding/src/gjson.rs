//! Core of `encoding/gjson`: a wrapper over [`Value`] with path access and
//! format conversion. Content detection tries JSON, YAML, TOML and properties.

use gf_core::Value;
use gf_errors::gerror;
use gf_util::gconv;
use indexmap::IndexMap;

use crate::{gproperties, gtoml, gxml, gyaml};

/// A JSON document over a [`Value`].
#[derive(Clone, Debug, PartialEq)]
pub struct Json {
    value: Value,
}

impl Json {
    /// Creates a document from a value.
    pub fn new(value: Value) -> Json {
        Json { value }
    }

    /// Creates a document from raw content, detecting its format.
    ///
    /// A leading UTF-8 byte-order mark is dropped, like GoFrame's readers do.
    pub fn new_from(content: &[u8]) -> Result<Json, gerror::Error> {
        let content = content.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(content);
        let trimmed = content
            .iter()
            .position(|byte| !byte.is_ascii_whitespace())
            .map_or(content, |index| &content[index..]);
        if trimmed.is_empty() {
            return Ok(Json { value: Value::Null });
        }
        let first = trimmed.first().copied();
        let section_like = first == Some(b'[') && looks_like_ini(trimmed);
        if matches!(first, Some(b'{') | Some(b'[')) {
            match serde_json::from_slice::<Value>(trimmed) {
                Ok(value) => return Ok(Json { value }),
                Err(err) => {
                    if !section_like {
                        return Err(json_error(err));
                    }
                }
            }
        }
        if let Ok(value) = gyaml::decode(trimmed) {
            return Ok(Json { value });
        }
        if let Ok(value) = gtoml::decode(trimmed) {
            return Ok(Json { value });
        }
        if let Ok(value) = crate::gini::decode(trimmed) {
            return Ok(Json { value });
        }
        if trimmed.contains(&b'=')
            && let Ok(value) = gproperties::decode(trimmed)
        {
            let empty = matches!(&value, Value::Map(entries) if entries.is_empty());
            if !empty {
                return Ok(Json { value });
            }
        }
        Err(gerror::Error::new("unrecognized content format for gjson"))
    }

    /// Creates a document from optional content; `None` is a null document.
    pub fn new_from_option(content: Option<&[u8]>) -> Result<Json, gerror::Error> {
        match content {
            Some(content) => Json::new_from(content),
            None => Ok(Json { value: Value::Null }),
        }
    }

    /// Returns the wrapped value.
    pub fn value(&self) -> &Value {
        &self.value
    }

    /// Returns the wrapped value.
    pub fn into_value(self) -> Value {
        self.value
    }

    /// Looks up a dotted path, for example `a.b.0.c`.
    pub fn get(&self, pattern: &str) -> Value {
        let mut current = &self.value;
        for segment in pattern.split('.') {
            if segment.is_empty() {
                return Value::Null;
            }
            match current {
                Value::Map(entries) => match entries.get(segment) {
                    Some(value) => current = value,
                    None => return Value::Null,
                },
                Value::List(items) => match segment.parse::<usize>() {
                    Ok(index) => match items.get(index) {
                        Some(value) => current = value,
                        None => return Value::Null,
                    },
                    Err(_) => return Value::Null,
                },
                _ => return Value::Null,
            }
        }
        current.clone()
    }

    /// Returns the value at `pattern` as a string.
    pub fn get_string(&self, pattern: &str) -> String {
        gconv::string(&self.get(pattern))
    }

    /// Returns the value at `pattern` as an i64.
    pub fn get_int64(&self, pattern: &str) -> i64 {
        gconv::int64_value(&self.get(pattern))
    }

    /// Returns the value at `pattern` as a u64.
    pub fn get_uint64(&self, pattern: &str) -> u64 {
        gconv::uint64_value(&self.get(pattern))
    }

    /// Returns the value at `pattern` as an f64.
    pub fn get_float64(&self, pattern: &str) -> f64 {
        gconv::float64_value(&self.get(pattern))
    }

    /// Returns the value at `pattern` as a bool.
    pub fn get_bool(&self, pattern: &str) -> bool {
        gconv::bool_value(&self.get(pattern))
    }

    /// Returns the value at `pattern` as strings.
    pub fn get_strings(&self, pattern: &str) -> Vec<String> {
        gconv::strings(&self.get(pattern))
    }

    /// Returns the value at `pattern` as integers.
    pub fn get_ints(&self, pattern: &str) -> Vec<i64> {
        gconv::ints(&self.get(pattern))
    }

    /// Returns the value at `pattern` as a map.
    pub fn get_map(&self, pattern: &str) -> IndexMap<String, Value> {
        gconv::map(&self.get(pattern))
    }

    /// Sets a dotted path, creating maps along the way.
    pub fn set(&mut self, pattern: &str, value: Value) {
        let segments: Vec<&str> = pattern.split('.').collect();
        set_path(&mut self.value, &segments, value);
    }

    /// Removes a dotted path.
    pub fn remove(&mut self, pattern: &str) {
        let segments: Vec<&str> = pattern.split('.').collect();
        remove_path(&mut self.value, &segments);
    }

    /// Encodes the document as JSON text.
    pub fn to_json(&self) -> Result<String, gerror::Error> {
        gf_json::marshal_value(&self.value)
    }

    /// Encodes the document as compact JSON text (`String` in Go).
    pub fn string(&self) -> String {
        self.to_json().unwrap_or_default()
    }

    /// Encodes the document as YAML text.
    pub fn to_yaml(&self) -> Result<String, gerror::Error> {
        gyaml::encode_to_string(&self.value)
    }

    /// Encodes the document as TOML text.
    pub fn to_toml(&self) -> Result<String, gerror::Error> {
        gtoml::encode_to_string(&self.value)
    }

    /// Encodes the document as XML text.
    pub fn to_xml(&self) -> Result<String, gerror::Error> {
        let bytes = gxml::encode(&self.value, None);
        String::from_utf8(bytes)
            .map_err(|err| gerror::Error::new(format!("encoded XML is not UTF-8: {err}")))
    }

    /// Encodes the document as properties text.
    pub fn to_properties(&self) -> Result<String, gerror::Error> {
        gproperties::encode_to_string(&self.value)
    }

    /// Encodes the document as INI text.
    pub fn to_ini(&self) -> Result<String, gerror::Error> {
        let bytes = crate::gini::encode(&self.value)?;
        String::from_utf8(bytes)
            .map_err(|err| gerror::Error::new(format!("encoded INI is not UTF-8: {err}")))
    }

    /// Returns the whole document as a map (`Json.Map`).
    ///
    /// A non-map document becomes an empty map, like GoFrame's `Map`.
    pub fn map(&self) -> IndexMap<String, Value> {
        gconv::map(&self.value)
    }

    /// Prints the document as indented JSON, like Go's `Json.Dump`.
    pub fn dump(&self) {
        match gf_json::marshal_indent(&self.value, "", "    ") {
            Ok(bytes) => println!("{}", String::from_utf8_lossy(&bytes)),
            Err(error) => println!("{error}"),
        }
    }

    /// Keeps the GoFrame flag for hierarchical key checking.
    ///
    /// The port's path lookup splits on every dot either way (ADR-045), so the
    /// flag is recorded but changes nothing.
    pub fn set_violence_check(&mut self, _check: bool) {}
}

/// The content types `gjson` can load, like Go's `ContentTypeJson` family.
pub const CONTENT_TYPE_JSON: &str = "json";
/// YAML content type.
pub const CONTENT_TYPE_YAML: &str = "yaml";
/// TOML content type.
pub const CONTENT_TYPE_TOML: &str = "toml";
/// INI content type.
pub const CONTENT_TYPE_INI: &str = "ini";
/// XML content type.
pub const CONTENT_TYPE_XML: &str = "xml";
/// Properties content type.
pub const CONTENT_TYPE_PROPERTIES: &str = "properties";

/// Maps a file extension onto a content type, like Go's `gjson.ContentType`.
pub fn content_type(extension: &str) -> &'static str {
    match extension
        .trim_start_matches('.')
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => CONTENT_TYPE_JSON,
        "yaml" | "yml" => CONTENT_TYPE_YAML,
        "toml" => CONTENT_TYPE_TOML,
        "ini" => CONTENT_TYPE_INI,
        "xml" => CONTENT_TYPE_XML,
        "properties" => CONTENT_TYPE_PROPERTIES,
        _ => "",
    }
}

/// Reports whether `content_type` is one this package can load.
pub fn is_valid_content_type(content_type: &str) -> bool {
    matches!(
        content_type,
        CONTENT_TYPE_JSON
            | CONTENT_TYPE_YAML
            | CONTENT_TYPE_TOML
            | CONTENT_TYPE_INI
            | CONTENT_TYPE_XML
            | CONTENT_TYPE_PROPERTIES
    )
}

/// Loads `content` as `content_type`, like Go's `gjson.LoadContentType`.
pub fn load_content_type(
    content_type: &str,
    content: &[u8],
    _safe: bool,
) -> Result<Json, gerror::Error> {
    let decoded = match content_type {
        CONTENT_TYPE_JSON => {
            if content.iter().all(u8::is_ascii_whitespace) {
                Value::Null
            } else {
                serde_json::from_slice::<Value>(content).map_err(json_error)?
            }
        }
        CONTENT_TYPE_YAML => gyaml::decode(content)?,
        CONTENT_TYPE_TOML => gtoml::decode(content)?,
        CONTENT_TYPE_INI => crate::gini::decode(content)?,
        CONTENT_TYPE_XML => gxml::decode(content)?,
        CONTENT_TYPE_PROPERTIES => gproperties::decode(content)?,
        other => {
            return Err(gerror::Error::new(format!(
                "unsupported content type: {other}"
            )));
        }
    };
    Ok(Json { value: decoded })
}

/// Loads `content`, detecting its format, like Go's `gjson.LoadContent`.
pub fn load_content(content: &[u8], _safe: bool) -> Result<Json, gerror::Error> {
    Json::new_from(content)
}

fn json_error(err: serde_json::Error) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), "json.Unmarshal failed") {
        Some(err) => err,
        None => gerror::Error::new("json.Unmarshal failed"),
    }
}

fn looks_like_ini(content: &[u8]) -> bool {
    let line_end = content
        .iter()
        .position(|&byte| byte == b'\n')
        .unwrap_or(content.len());
    let line = &content[..line_end];
    let trimmed = line
        .iter()
        .position(|byte| !byte.is_ascii_whitespace())
        .map_or(line, |index| &line[index..]);
    trimmed.first() == Some(&b'[') && trimmed.contains(&b']')
}

fn set_path(current: &mut Value, segments: &[&str], value: Value) {
    let Some((segment, rest)) = segments.split_first() else {
        *current = value;
        return;
    };
    match current {
        Value::Map(entries) => {
            let entry = entries.entry((*segment).to_owned()).or_insert_with(|| {
                if rest
                    .first()
                    .is_some_and(|next| next.parse::<usize>().is_ok())
                {
                    Value::List(Vec::new())
                } else {
                    Value::Map(IndexMap::new())
                }
            });
            set_path(entry, rest, value);
        }
        Value::List(items) => {
            let Ok(index) = segment.parse::<usize>() else {
                let mut map = IndexMap::new();
                if rest.is_empty() {
                    map.insert((*segment).to_owned(), value);
                } else {
                    let mut child = Value::Map(IndexMap::new());
                    set_path(&mut child, rest, value);
                    map.insert((*segment).to_owned(), child);
                }
                *current = Value::Map(map);
                return;
            };
            if index < items.len() {
                set_path(&mut items[index], rest, value);
            } else if index == items.len() {
                if rest.is_empty() {
                    items.push(value);
                } else {
                    let mut child = Value::Map(IndexMap::new());
                    set_path(&mut child, rest, value);
                    items.push(child);
                }
            }
        }
        other => {
            let mut map = IndexMap::new();
            if rest.is_empty() {
                map.insert((*segment).to_owned(), value);
            } else {
                let mut child = Value::Map(IndexMap::new());
                set_path(&mut child, rest, value);
                map.insert((*segment).to_owned(), child);
            }
            *other = Value::Map(map);
        }
    }
}

fn remove_path(current: &mut Value, segments: &[&str]) {
    let Some((segment, rest)) = segments.split_first() else {
        return;
    };
    match current {
        Value::Map(entries) => {
            if rest.is_empty() {
                entries.shift_remove(*segment);
            } else if let Some(child) = entries.get_mut(*segment) {
                remove_path(child, rest);
            }
        }
        Value::List(items) => {
            if let Ok(index) = segment.parse::<usize>() {
                if rest.is_empty() {
                    if index < items.len() {
                        items.remove(index);
                    }
                } else if let Some(child) = items.get_mut(index) {
                    remove_path(child, rest);
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str =
        r#"{"users":[{"name":"john","score":90},{"name":"jane","score":80}],"meta":{"count":2}}"#;

    #[test]
    fn basic_access_matches_go() {
        let json = Json::new_from(JSON.as_bytes()).expect("json");
        assert_eq!(json.get_string("users.0.name"), "john");
        assert_eq!(json.get_int64("users.1.score"), 80);
        assert_eq!(json.get_int64("meta.count"), 2);
        assert_eq!(json.get_string("missing.path"), "");
        assert_eq!(
            json.get("users").to_string(),
            "[map[name:john score:90] map[name:jane score:80]]"
        );
        assert_eq!(
            json.get_strings("users.0"),
            vec!["john".to_owned(), "90".to_owned()]
        );
        assert!(json.get_bool("meta"));
    }

    #[test]
    fn set_and_remove() {
        let mut json = Json::new(Value::Null);
        json.set("a.b.c", Value::Int(1));
        json.set("a.b.d", Value::String("x".into()));
        assert_eq!(json.get_int64("a.b.c"), 1);
        assert_eq!(json.get_string("a.b.d"), "x");
        json.remove("a.b.c");
        assert_eq!(json.get("a.b.c"), Value::Null);
        assert_eq!(json.get_string("a.b.d"), "x");

        let mut list = Json::new_from(b"[1,2,3]").expect("json");
        list.set("1", Value::Int(20));
        assert_eq!(list.get_int64("1"), 20);
        list.remove("0");
        assert_eq!(list.get_int64("0"), 20);
    }

    #[test]
    fn encoding_round_trip() {
        let json = Json::new_from(JSON.as_bytes()).expect("json");
        let text = json.to_json().expect("json");
        let back = Json::new_from(text.as_bytes()).expect("json");
        assert_eq!(back.get_string("users.0.name"), "john");

        assert!(json.to_yaml().expect("yaml").contains("users:"));
        let toml = Json::new_from(b"a = 1\nb = \"x\"\n").expect("toml");
        assert_eq!(toml.get_int64("a"), 1);
        assert_eq!(toml.get_string("b"), "x");
        assert!(toml.to_toml().expect("toml").contains("a = 1"));

        let yaml = Json::new_from(b"a: 1\nb: x\n").expect("yaml");
        assert_eq!(yaml.get_int64("a"), 1);
    }

    #[test]
    fn properties_and_ini_documents() {
        let properties = Json::new_from(b"b=2\na=1\n").expect("properties");
        assert_eq!(properties.get_int64("b"), 2);
        assert_eq!(
            properties.to_properties().expect("properties"),
            "a = 1\nb = 2\n"
        );

        let ini = Json::new_from(b"[s]\nk=v\n").expect("ini");
        assert_eq!(ini.get_string("s.k"), "v");
        assert_eq!(ini.to_ini().expect("ini"), "[s]\nk=v\n");
    }

    #[test]
    fn invalid_content_errors() {
        assert!(Json::new_from(b"{invalid").is_err());
        assert!(Json::new_from_option(None).expect("none").value().is_nil());
        assert!(Json::new_from(b"").expect("empty").value().is_nil());
        assert!(Json::new_from(b")))").is_err());
    }
}
