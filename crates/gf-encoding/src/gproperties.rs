//! Port of `encoding/gproperties`.
//!
//! Parses Java properties with magiconair's rules: `#`/`!` comments, `=|:`
//! or whitespace separators, `\` continuations, `\uXXXX` escapes and
//! `${name}` references (with cycle and malformed-expression errors).

use gf_core::Value;
use gf_errors::gerror;
use indexmap::IndexMap;

const LOAD_FAILED: &str = "Lib magiconair load Properties data failed.";

fn load_error(detail: &str) -> gerror::Error {
    match gerror::wrap(Some(gerror::Error::new(detail)), LOAD_FAILED) {
        Some(err) => err,
        None => gerror::Error::new(LOAD_FAILED),
    }
}

fn unescape(text: &str) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(next) = chars.next() else {
            out.push('\\');
            break;
        };
        match next {
            't' => out.push('\t'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            'f' => out.push('\u{000C}'),
            'u' => {
                let mut code = String::with_capacity(4);
                for _ in 0..4 {
                    match chars.next() {
                        Some(ch) if ch.is_ascii_hexdigit() => code.push(ch),
                        _ => return Err("invalid unicode literal".to_owned()),
                    }
                }
                let value = u32::from_str_radix(&code, 16)
                    .map_err(|_| "invalid unicode literal".to_owned())?;
                out.push(char::from_u32(value).unwrap_or('\u{FFFD}'));
            }
            other => out.push(other),
        }
    }
    Ok(out)
}

fn is_blank(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\u{000C}')
}

fn logical_lines(text: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for raw in text.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        if !current.is_empty() {
            current.push_str(raw.trim_start_matches(is_blank));
        } else {
            current.push_str(raw);
        }
        let trailing_backslashes = current.chars().rev().take_while(|&ch| ch == '\\').count();
        if trailing_backslashes % 2 == 1 {
            current.pop();
            continue;
        }
        lines.push(std::mem::take(&mut current));
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

fn split_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start_matches(is_blank);
    if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with('!') {
        return None;
    }
    let chars: Vec<char> = trimmed.chars().collect();
    let mut key_end = chars.len();
    let mut escaped = false;
    let mut separator_index = None;
    for (index, &ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if ch == '=' || ch == ':' || is_blank(ch) {
            key_end = index;
            separator_index = Some(index);
            break;
        }
    }
    let key = &trimmed[..byte_index(trimmed, key_end)];
    let mut rest = &trimmed[byte_index(trimmed, key_end)..];
    if let Some(index) = separator_index {
        let separator = chars[index];
        rest = &trimmed[byte_index(trimmed, index)..];
        if is_blank(separator) {
            rest = rest.trim_start_matches(is_blank);
            if rest.starts_with('=') || rest.starts_with(':') {
                rest = &rest[1..];
            }
        } else {
            rest = &rest[1..];
        }
        rest = rest.trim_start_matches(is_blank);
    }
    Some((key.to_owned(), rest.to_owned()))
}

fn byte_index(text: &str, char_index: usize) -> usize {
    text.char_indices()
        .nth(char_index)
        .map_or(text.len(), |(index, _)| index)
}

fn expand(
    key: &str,
    entries: &IndexMap<String, String>,
    stack: &mut Vec<String>,
    resolved: &mut IndexMap<String, String>,
) -> Result<String, String> {
    if let Some(value) = resolved.get(key) {
        return Ok(value.clone());
    }
    let raw = entries
        .get(key)
        .cloned()
        .ok_or_else(|| "unknown property".to_owned())?;
    if stack.iter().any(|existing| existing == key) {
        return Err("circular reference in:".to_owned());
    }
    stack.push(key.to_owned());
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw.as_str();
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let tail = &rest[start + 2..];
        let Some(end) = tail.find('}') else {
            return Err("malformed expression".to_owned());
        };
        let expression = &tail[..end];
        let (name, default) = match expression.split_once(':') {
            Some((name, default)) => (name, Some(default)),
            None => (expression, None),
        };
        if entries.contains_key(name) {
            out.push_str(&expand(name, entries, stack, resolved)?);
        } else if let Some(default) = default {
            out.push_str(default);
        } else {
            out.push_str("${");
            out.push_str(expression);
            out.push('}');
        }
        rest = &tail[end + 1..];
    }
    out.push_str(rest);
    stack.pop();
    resolved.insert(key.to_owned(), out.clone());
    Ok(out)
}

/// Decodes properties content into a map, nesting keys on `.`.
pub fn decode(content: &[u8]) -> Result<Value, gerror::Error> {
    let text = String::from_utf8_lossy(content);
    let mut entries: IndexMap<String, String> = IndexMap::new();
    for line in logical_lines(&text) {
        let Some((key, value)) = split_line(&line) else {
            continue;
        };
        let key = unescape(&key).map_err(|err| load_error(&err))?;
        let value = unescape(&value).map_err(|err| load_error(&err))?;
        entries.insert(key, value);
    }
    let mut resolved: IndexMap<String, String> = IndexMap::new();
    for key in entries.keys().cloned().collect::<Vec<_>>() {
        let mut stack = Vec::new();
        expand(&key, &entries, &mut stack, &mut resolved).map_err(|err| load_error(&err))?;
    }

    let mut root = IndexMap::new();
    for (key, value) in &resolved {
        let path: Vec<&str> = key.split('.').collect();
        let last = path[path.len() - 1].to_lowercase();
        let mut map = &mut root;
        for segment in &path[..path.len() - 1] {
            let entry = map
                .entry((*segment).to_owned())
                .or_insert_with(|| Value::Map(IndexMap::new()));
            if !matches!(entry, Value::Map(_)) {
                *entry = Value::Map(IndexMap::new());
            }
            let Value::Map(inner) = entry else {
                unreachable!("entry was just made a map");
            };
            map = inner;
        }
        map.insert(last, Value::String(value.clone()));
    }
    Ok(Value::Map(root))
}

fn flatten(shadow: &mut IndexMap<String, Value>, map: &IndexMap<String, Value>, prefix: &str) {
    if !prefix.is_empty() && shadow.contains_key(prefix) {
        return;
    }
    let prefix = if prefix.is_empty() {
        String::new()
    } else {
        format!("{prefix}.")
    };
    for (key, value) in map {
        let full_key = format!("{prefix}{key}");
        match value {
            Value::Map(inner) => flatten(shadow, inner, &full_key),
            _ => {
                shadow.insert(full_key.to_lowercase(), value.clone());
            }
        }
    }
}

fn escape_value(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for (index, ch) in value.chars().enumerate() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{000C}' => out.push_str("\\f"),
            '\\' => out.push_str("\\\\"),
            ' ' if index == 0 => out.push_str("\\ "),
            ch => out.push(ch),
        }
    }
    out
}

/// Encodes a map value as properties text with sorted, lower-case keys.
pub fn encode(data: &Value) -> Result<Vec<u8>, gerror::Error> {
    let Value::Map(map) = data else {
        return Ok(Vec::new());
    };
    let mut flattened = IndexMap::new();
    flatten(&mut flattened, map, "");
    let mut keys: Vec<&String> = flattened.keys().collect();
    keys.sort();
    let mut out = String::with_capacity(flattened.len() * 24);
    for key in keys {
        out.push_str(key);
        out.push_str(" = ");
        out.push_str(&escape_value(&flattened[key].to_string()));
        out.push('\n');
    }
    Ok(out.into_bytes())
}

/// Encodes a map value as properties text.
pub fn encode_to_string(data: &Value) -> Result<String, gerror::Error> {
    let bytes = encode(data)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// Decodes properties content and encodes it as JSON text.
pub fn to_json(content: &[u8]) -> Result<String, gerror::Error> {
    let value = decode(content)?;
    gf_json::marshal_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const P_STR: &str = "\n# 模板引擎目录\nviewpath = \"/home/www/templates/\"\n# redis数据库配置\nredis.disk  = \"127.0.0.1:6379,0\"\nredis.cache = \"127.0.0.1:6379,1\"\n#SQL配置\nsql.mysql.0.type = mysql\nsql.mysql.0.ip = 127.0.0.1\nsql.mysql.0.user = root\n";

    fn map_of(value: &Value, key: &str) -> Value {
        let Value::Map(map) = value else {
            panic!("expected map");
        };
        map[key].clone()
    }

    #[test]
    fn round_trip_matches_go() {
        let mut outer = IndexMap::new();
        outer.insert("properties".to_owned(), Value::String(P_STR.to_owned()));
        let encoded = encode_to_string(&Value::Map(outer)).expect("encode");
        let decoded = decode(encoded.as_bytes()).expect("decode");
        assert_eq!(
            map_of(&decoded, "properties"),
            Value::String(P_STR.to_owned())
        );
    }

    #[test]
    fn decode_builds_nested_maps_and_unescapes() {
        let decoded = decode(
            b"Title = Go Frame\nDb.Host=127.0.0.1\nDb.Port: 6379\n# comment\nescaped\\ key = a\\tb\\u0041\n",
        )
        .expect("decode");
        let Value::Map(map) = &decoded else {
            panic!("expected map");
        };
        assert_eq!(map["title"], Value::String("Go Frame".into()));
        let Value::Map(db) = &map["Db"] else {
            panic!("expected nested map");
        };
        assert_eq!(db["host"], Value::String("127.0.0.1".into()));
        assert_eq!(db["port"], Value::String("6379".into()));
        assert_eq!(map["escaped key"], Value::String("a\tbA".into()));
    }

    #[test]
    fn references_expand_or_fail() {
        let decoded = decode(b"host = example.com\nurl = http://${host}/\n").expect("decode");
        let Value::Map(map) = decoded else {
            panic!("expected map");
        };
        assert_eq!(map["url"], Value::String("http://example.com/".into()));

        for (input, expected) in [
            ("key=${key}", "circular reference"),
            ("key1=${key2}\nkey2=${key1}", "circular reference"),
            ("key=${ke", "malformed expression"),
            ("key\\u1 = value", "invalid unicode literal"),
            ("key\\u123g = value", "invalid unicode literal"),
        ] {
            let err = decode(input.as_bytes()).expect_err(input);
            let text = err.to_string();
            assert!(text.contains(LOAD_FAILED), "{text}");
            assert!(text.contains(expected), "{text}");
        }
    }

    #[test]
    fn encode_sorts_lower_case_keys() {
        let mut sql = IndexMap::new();
        sql.insert("userName".to_owned(), Value::String("admin".into()));
        sql.insert("password".to_owned(), Value::String("123456".into()));
        let mut map = IndexMap::new();
        map.insert("user".to_owned(), Value::String("admin".into()));
        map.insert("no".to_owned(), Value::Int(123));
        map.insert("sql".to_owned(), Value::Map(sql));
        let encoded = encode_to_string(&Value::Map(map)).expect("encode");
        assert_eq!(
            encoded,
            "no = 123\nsql.password = 123456\nsql.username = admin\nuser = admin\n"
        );
    }

    #[test]
    fn to_json_matches_go() {
        assert_eq!(
            to_json(b"b=2\na=1\n").expect("json"),
            r#"{"a":"1","b":"2"}"#
        );
        let decoded = decode(P_STR.as_bytes()).expect("decode");
        assert_eq!(
            to_json(P_STR.as_bytes()).expect("json"),
            gf_json::marshal_value(&decoded).expect("json")
        );
    }
}
