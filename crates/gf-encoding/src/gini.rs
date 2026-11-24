//! Port of `encoding/gini`.

use gf_core::Value;
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;
use indexmap::IndexMap;

/// Decodes INI content into `{section: {key: value}}`.
pub fn decode(data: &[u8]) -> Result<Value, gerror::Error> {
    let text = String::from_utf8_lossy(data);
    let mut root: IndexMap<String, Value> = IndexMap::new();
    let mut fields: IndexMap<String, Value> = IndexMap::new();
    let mut section = String::new();
    let mut last_section = String::new();
    let mut have_section = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        let begin = line.find('[');
        let end = line.find(']');
        if let (Some(begin), Some(end)) = (begin, end)
            && end >= 2
            && end > begin
        {
            section = line[begin + 1..end].to_owned();
            if last_section.is_empty() {
                last_section = section.clone();
            } else if last_section != section {
                last_section = section.clone();
                fields = IndexMap::new();
            }
            have_section = true;
            if !root.contains_key(&section) {
                root.insert(section.clone(), Value::Map(IndexMap::new()));
            }
        } else if !have_section {
            continue;
        }
        if have_section && line.contains('=') {
            let mut parts = line.splitn(2, '=');
            let key = parts.next().unwrap_or_default().trim().to_owned();
            let value = parts.next().unwrap_or_default().trim().to_owned();
            fields.insert(key, Value::String(value));
            root.insert(section.clone(), Value::Map(fields.clone()));
        }
    }
    if !have_section {
        return Err(gerror::Error::new_code(
            &CODE_INVALID_PARAMETER,
            &["failed to parse INI file, section not found"],
        ));
    }
    Ok(Value::Map(root))
}

/// Encodes `{section: {key: value}}` (or a flat map) as INI text.
pub fn encode(data: &Value) -> Result<Vec<u8>, gerror::Error> {
    let Value::Map(map) = data else {
        return Ok(Vec::new());
    };
    let mut out = String::new();
    if map.values().any(|value| matches!(value, Value::Map(_))) {
        for (section, item) in map {
            let Value::Map(fields) = item else {
                continue;
            };
            out.push('[');
            out.push_str(section);
            out.push_str("]\n");
            for (key, value) in fields {
                out.push_str(key);
                out.push('=');
                out.push_str(&value.to_string());
                out.push('\n');
            }
        }
    } else {
        for (key, value) in map {
            out.push_str(key);
            out.push('=');
            out.push_str(&value.to_string());
            out.push('\n');
        }
    }
    Ok(out.into_bytes())
}

/// Decodes INI content and encodes it as JSON text.
pub fn to_json(data: &[u8]) -> Result<String, gerror::Error> {
    let value = decode(data)?;
    gf_json::marshal_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const INI: &str = "\n; comment\n[redis]\n    disk  = \"127.0.0.1:6379,0\"\n    cache = \"127.0.0.1:6379,1\"\n\n# mysql\n[mysql]\n    user  = root\n    pass  = 123456\n    protocol = tcp\n    address  = \"127.0.0.1:3306\"\n";

    #[test]
    fn decode_matches_go() {
        let value = decode(INI.as_bytes()).expect("decode");
        let Value::Map(map) = &value else {
            panic!("expected map");
        };
        let Value::Map(redis) = &map["redis"] else {
            panic!("expected redis");
        };
        assert_eq!(redis["disk"], Value::String("\"127.0.0.1:6379,0\"".into()));
        let Value::Map(mysql) = &map["mysql"] else {
            panic!("expected mysql");
        };
        assert_eq!(mysql["user"], Value::String("root".into()));
        assert_eq!(mysql["address"], Value::String("\"127.0.0.1:3306\"".into()));
    }

    #[test]
    fn decode_errors_without_section() {
        let err = decode(b"a=1\n").expect_err("no section");
        assert_eq!(
            err.to_string(),
            "failed to parse INI file, section not found"
        );
    }

    #[test]
    fn section_and_flat_encoding() {
        let mut fields = IndexMap::new();
        fields.insert("user".to_owned(), Value::String("root".into()));
        let mut map = IndexMap::new();
        map.insert("mysql".to_owned(), Value::Map(fields));
        let encoded = String::from_utf8(encode(&Value::Map(map)).expect("encode")).expect("utf8");
        assert_eq!(encoded, "[mysql]\nuser=root\n");

        let mut flat = IndexMap::new();
        flat.insert("a".to_owned(), Value::String("1".into()));
        let encoded = String::from_utf8(encode(&Value::Map(flat)).expect("encode")).expect("utf8");
        assert_eq!(encoded, "a=1\n");
    }

    #[test]
    fn json_round_trip() {
        let value = decode(INI.as_bytes()).expect("decode");
        assert_eq!(
            to_json(INI.as_bytes()).expect("json"),
            gf_json::marshal_value(&value).expect("json")
        );
    }
}
