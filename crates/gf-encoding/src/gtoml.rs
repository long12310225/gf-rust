//! Port of `encoding/gtoml`.

use gf_core::Value;
use gf_errors::gerror;
use indexmap::IndexMap;

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

fn toml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            ch if (ch as u32) < 0x20 || (ch as u32) == 0x7F => {
                out.push_str(&format!("\\u{:04X}", ch as u32));
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn format_float(value: f64) -> String {
    let text = value.to_string();
    if text.contains(['.', 'e', 'E', 'n', 'i']) {
        text
    } else {
        format!("{text}.0")
    }
}

fn render_inline(value: &Value) -> Result<String, String> {
    Ok(match value {
        Value::Null => return Err("toml: cannot encode a nil value".to_owned()),
        Value::Bool(v) => v.to_string(),
        Value::Int(v) => v.to_string(),
        Value::UInt(v) => v.to_string(),
        Value::Float(v) => format_float(*v),
        Value::String(v) => toml_escape(v),
        Value::Bytes(bytes) => {
            let items: Vec<String> = bytes.iter().map(u8::to_string).collect();
            format!("[{}]", items.join(", "))
        }
        Value::List(items) => {
            let mut rendered = Vec::with_capacity(items.len());
            for item in items {
                rendered.push(render_inline(item)?);
            }
            format!("[{}]", rendered.join(", "))
        }
        Value::Map(entries) => {
            let mut rendered = Vec::with_capacity(entries.len());
            for (key, item) in entries {
                rendered.push(format!("{key} = {}", render_inline(item)?));
            }
            format!("{{ {} }}", rendered.join(", "))
        }
    })
}

fn to_toml(value: &Value) -> Result<toml::Value, String> {
    Ok(match value {
        Value::Null => return Err("toml: cannot encode a nil value".to_owned()),
        Value::Bool(v) => toml::Value::Boolean(*v),
        Value::Int(v) => toml::Value::Integer(*v),
        Value::UInt(v) => {
            toml::Value::Integer(i64::try_from(*v).map_err(|_| {
                "toml: unsigned integer does not fit into a TOML integer".to_owned()
            })?)
        }
        Value::Float(v) => toml::Value::Float(*v),
        Value::String(v) => toml::Value::String(v.clone()),
        Value::Bytes(bytes) => toml::Value::Array(
            bytes
                .iter()
                .map(|byte| toml::Value::Integer(i64::from(*byte)))
                .collect(),
        ),
        Value::List(items) => {
            let mut array = Vec::with_capacity(items.len());
            for item in items {
                array.push(to_toml(item)?);
            }
            toml::Value::Array(array)
        }
        Value::Map(entries) => {
            let mut table = toml::map::Map::with_capacity(entries.len());
            for (key, item) in entries {
                table.insert(key.clone(), to_toml(item)?);
            }
            toml::Value::Table(table)
        }
    })
}

fn from_toml(value: toml::Value) -> Value {
    match value {
        toml::Value::String(v) => Value::String(v),
        toml::Value::Integer(v) => Value::Int(v),
        toml::Value::Float(v) => Value::Float(v),
        toml::Value::Boolean(v) => Value::Bool(v),
        toml::Value::Datetime(v) => Value::String(v.to_string()),
        toml::Value::Array(items) => Value::List(items.into_iter().map(from_toml).collect()),
        toml::Value::Table(table) => {
            let mut entries = IndexMap::with_capacity(table.len());
            for (key, item) in table {
                entries.insert(key, from_toml(item));
            }
            Value::Map(entries)
        }
    }
}

/// Encodes `value` as TOML.
///
/// A `nil` value panics like the Go original (BurntSushi's encoder panics on
/// `nil` inside its reflection path); a top-level scalar is written as a TOML
/// value, which is what the Go encoder does.
pub fn encode(value: &Value) -> Result<Vec<u8>, gerror::Error> {
    match value {
        Value::Null => panic!("toml: cannot encode a nil value"),
        Value::Map(_) => {
            let table = to_toml(value).map_err(gerror::Error::new)?;
            toml::to_string(&table)
                .map(String::into_bytes)
                .map_err(|err| wrap(err, "toml.Encoder.Encode failed"))
        }
        Value::String(v) => Ok(toml_escape(v).into_bytes()),
        Value::Bool(v) => Ok(v.to_string().into_bytes()),
        Value::Int(v) => Ok(v.to_string().into_bytes()),
        Value::UInt(v) => Ok(v.to_string().into_bytes()),
        Value::Float(v) => Ok(format_float(*v).into_bytes()),
        Value::List(_) | Value::Bytes(_) => Ok(render_inline(value)
            .map_err(gerror::Error::new)?
            .into_bytes()),
    }
}

/// Encodes `value` as TOML text.
pub fn encode_to_string(value: &Value) -> Result<String, gerror::Error> {
    let bytes = encode(value)?;
    String::from_utf8(bytes).map_err(|err| wrap(err, "toml.Encoder.Encode failed"))
}

/// Decodes TOML content into a [`Value`].
pub fn decode(content: &[u8]) -> Result<Value, gerror::Error> {
    let text = String::from_utf8_lossy(content);
    let parsed: toml::Value =
        toml::from_str(&text).map_err(|err| wrap(err, "toml.Unmarshal failed"))?;
    Ok(from_toml(parsed))
}

/// Decodes TOML content into `T`.
pub fn decode_to<T: serde::de::DeserializeOwned>(content: &[u8]) -> Result<T, gerror::Error> {
    let text = String::from_utf8_lossy(content);
    toml::from_str(&text).map_err(|err| wrap(err, "toml.Unmarshal failed"))
}

/// Decodes TOML content and encodes it as JSON text.
pub fn to_json(content: &[u8]) -> Result<String, gerror::Error> {
    let value = decode(content)?;
    gf_json::marshal_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML_STR: &str = "\n# 模板引擎目录\nviewpath = \"/home/www/templates/\"\n# MySQL数据库配置\n[redis]\n    disk  = \"127.0.0.1:6379,0\"\n    cache = \"127.0.0.1:6379,1\"\n";

    #[test]
    fn encode_string_matches_go() {
        assert_eq!(
            encode(&Value::String("a\nb".into())).expect("encode"),
            b"\"a\\nb\""
        );
        assert_eq!(
            encode(&Value::Map(IndexMap::from([(
                "k".to_owned(),
                Value::String("v".into())
            )])))
            .expect("encode"),
            b"k = \"v\"\n"
        );
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut redis = IndexMap::new();
        redis.insert("disk".to_owned(), Value::String("127.0.0.1:6379,0".into()));
        redis.insert("cache".to_owned(), Value::String("127.0.0.1:6379,1".into()));
        let mut value = IndexMap::new();
        value.insert(
            "toml".to_owned(),
            Value::String(TOML_STR.trim_start_matches('\n').into()),
        );
        value.insert("redis".to_owned(), Value::Map(redis));
        let encoded = encode_to_string(&Value::Map(value)).expect("encode");
        let decoded = decode(encoded.as_bytes()).expect("decode");
        let Value::Map(map) = decoded else {
            panic!("expected map");
        };
        assert!(map.contains_key("toml"));
        assert!(map.contains_key("redis"));

        let array = Value::List(vec![Value::String("a".into()), Value::String("b".into())]);
        assert_eq!(encode_to_string(&array).expect("encode"), r#"["a", "b"]"#);
    }

    #[test]
    fn duplicate_tables_fail() {
        let content = "[redis]\ndd = 11\n[redis]\ndisk = \"x\"\n";
        assert!(decode(content.as_bytes()).is_err());
        assert!(to_json(content.as_bytes()).is_err());
    }

    #[test]
    fn to_json_matches_gf_json() {
        let content = "b = 1\na = \"x\"\n";
        assert_eq!(
            to_json(content.as_bytes()).expect("json"),
            gf_json::marshal_value(&decode(content.as_bytes()).expect("decode")).expect("json")
        );
    }

    #[test]
    fn scalars_and_datetimes() {
        assert_eq!(
            decode(b"x = 3.14\ny = true\nz = 1979-05-27T07:32:00Z\n")
                .expect("decode")
                .to_string(),
            "map[x:3.14 y:true z:1979-05-27T07:32:00Z]"
        );
    }
}
