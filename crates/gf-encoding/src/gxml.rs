//! Port of `encoding/gxml`.
//!
//! XML is decoded with mxj's conventions: the root element becomes a map key,
//! child elements nest, repeated names become lists, attributes are prefixed
//! with `-` and mixed text becomes `#text`. Encoding writes sorted keys, like
//! mxj, and respects the global escape switch.

use std::sync::atomic::{AtomicBool, Ordering};

use gf_core::Value;
use gf_errors::gerror;
use indexmap::IndexMap;
use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::gcharset;

static ESCAPE_CHARS: AtomicBool = AtomicBool::new(false);

/// Turns XML character escaping on or off, like `XMLEscapeChars`.
pub fn xml_escape_chars(enabled: bool) {
    ESCAPE_CHARS.store(enabled, Ordering::Relaxed);
}

fn escape_enabled() -> bool {
    ESCAPE_CHARS.load(Ordering::Relaxed)
}

#[derive(Default)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    text: String,
}

fn parse_nodes(content: &[u8]) -> Result<Option<Node>, gerror::Error> {
    let mut reader = Reader::from_reader(content);
    let mut stack: Vec<Node> = Vec::new();
    let mut root: Option<Node> = None;
    loop {
        match reader.read_event() {
            Ok(Event::Start(start)) => {
                let name = String::from_utf8_lossy(start.name().as_ref()).into_owned();
                let mut node = Node {
                    name,
                    ..Node::default()
                };
                for attribute in start.attributes() {
                    let attribute = attribute.map_err(parse_error)?;
                    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                    let value = attribute
                        .unescape_value()
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    node.attributes.push((key, value));
                }
                stack.push(node);
            }
            Ok(Event::Empty(empty)) => {
                let name = String::from_utf8_lossy(empty.name().as_ref()).into_owned();
                let mut node = Node {
                    name,
                    ..Node::default()
                };
                for attribute in empty.attributes() {
                    let attribute = attribute.map_err(parse_error)?;
                    let key = String::from_utf8_lossy(attribute.key.as_ref()).into_owned();
                    let value = attribute
                        .unescape_value()
                        .map(|value| value.into_owned())
                        .unwrap_or_default();
                    node.attributes.push((key, value));
                }
                push_node(&mut stack, &mut root, node);
            }
            Ok(Event::End(_)) => {
                if let Some(node) = stack.pop() {
                    push_node(&mut stack, &mut root, node);
                }
            }
            Ok(Event::Text(text)) => {
                if let Some(current) = stack.last_mut() {
                    match text.unescape() {
                        Ok(value) => current.text.push_str(&value),
                        Err(_) => current
                            .text
                            .push_str(&String::from_utf8_lossy(text.as_ref())),
                    }
                }
            }
            Ok(Event::CData(text)) => {
                if let Some(current) = stack.last_mut() {
                    current
                        .text
                        .push_str(&String::from_utf8_lossy(text.as_ref()));
                }
            }
            Ok(Event::Eof) => break,
            Ok(_) => {}
            Err(err) => return Err(parse_error(err)),
        }
    }
    Ok(root)
}

fn push_node(stack: &mut [Node], root: &mut Option<Node>, node: Node) {
    match stack.last_mut() {
        Some(parent) => parent.children.push(node),
        None => {
            if root.is_none() {
                *root = Some(node);
            }
        }
    }
}

fn parse_error(err: impl std::error::Error + Send + Sync + 'static) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), "mxj.NewMapXml failed") {
        Some(err) => err,
        None => gerror::Error::new("mxj.NewMapXml failed"),
    }
}

fn node_to_value(node: Node) -> Value {
    let text = node.text.trim();
    if node.children.is_empty() && node.attributes.is_empty() {
        return Value::String(text.to_owned());
    }
    let mut map: IndexMap<String, Value> = IndexMap::new();
    for (key, value) in node.attributes {
        map.insert(format!("-{key}"), Value::String(value));
    }
    if !text.is_empty() {
        map.insert("#text".to_owned(), Value::String(text.to_owned()));
    }
    for child in node.children {
        let name = child.name.clone();
        let value = node_to_value(child);
        match map.get_mut(&name) {
            Some(Value::List(items)) => items.push(value),
            Some(existing) => {
                let previous = existing.clone();
                *existing = Value::List(vec![previous, value]);
            }
            None => {
                map.insert(name, value);
            }
        }
    }
    Value::Map(map)
}

/// Detects and strips the XML declaration, converting the remaining content to
/// UTF-8 when the declaration names another encoding.
fn convert(content: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let text = String::from_utf8_lossy(content);
    let Some(start) = text.find("<?xml") else {
        return Ok(content.to_vec());
    };
    let Some(end_offset) = text[start..].find("?>") else {
        return Ok(content.to_vec());
    };
    let declaration = &text[start..start + end_offset + 2];
    let mut without = String::with_capacity(text.len());
    without.push_str(&text[..start]);
    without.push_str(&text[start + declaration.len()..]);
    let encoding = declaration
        .find("encoding")
        .and_then(|index| {
            let rest = &declaration[index + "encoding".len()..];
            let equals = rest.find('=')?;
            let rest = rest[equals + 1..].trim_start();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let rest = &rest[1..];
            let end = rest.find(quote)?;
            Some(rest[..end].to_owned())
        })
        .unwrap_or_else(|| "UTF-8".to_owned());
    if encoding.eq_ignore_ascii_case("UTF-8") || encoding.eq_ignore_ascii_case("UTF8") {
        return Ok(without.into_bytes());
    }
    gcharset::convert_bytes("UTF-8", &encoding, without.as_bytes())
}

/// Parses XML content into `{root: value}`.
pub fn decode(content: &[u8]) -> Result<Value, gerror::Error> {
    let converted = convert(content)?;
    let root = parse_nodes(&converted)?
        .ok_or_else(|| gerror::Error::new("mxj.NewMapXml failed: no root element"))?;
    let name = root.name.clone();
    let mut map = IndexMap::new();
    map.insert(name, node_to_value(root));
    Ok(Value::Map(map))
}

/// Parses XML content and returns the root element's value.
pub fn decode_without_root(content: &[u8]) -> Result<Value, gerror::Error> {
    let value = decode(content)?;
    let Value::Map(map) = value else {
        return Ok(value);
    };
    if let Some(inner) = map.values().find(|value| matches!(value, Value::Map(_))) {
        return Ok(inner.clone());
    }
    Ok(Value::Map(map))
}

fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            ch => out.push(ch),
        }
    }
    out
}

fn write_value(
    out: &mut String,
    name: &str,
    value: &Value,
    indent: Option<&str>,
    depth: usize,
    escape: bool,
) {
    let pad = |out: &mut String, depth: usize| {
        if let Some(indent) = indent {
            out.push('\n');
            for _ in 0..depth {
                out.push_str(indent);
            }
        }
    };
    match value {
        Value::List(items) => {
            for item in items {
                write_value(out, name, item, indent, depth, escape);
            }
        }
        Value::Map(map) => {
            let mut text = None;
            let mut attributes = Vec::new();
            let mut children: Vec<(&String, &Value)> = Vec::new();
            for (key, item) in map {
                if let Some(attribute) = key.strip_prefix('-') {
                    attributes.push((attribute, item));
                } else if key == "#text" {
                    text = Some(item);
                } else {
                    children.push((key, item));
                }
            }
            children.sort_by(|a, b| a.0.cmp(b.0));
            pad(out, depth);
            out.push('<');
            out.push_str(name);
            for (key, item) in attributes {
                out.push(' ');
                out.push_str(key);
                out.push_str("=\"");
                let rendered = item.to_string();
                if escape {
                    out.push_str(&escape_text(&rendered));
                } else {
                    out.push_str(&rendered);
                }
                out.push('"');
            }
            out.push('>');
            if let Some(text) = text {
                let rendered = text.to_string();
                if escape {
                    out.push_str(&escape_text(&rendered));
                } else {
                    out.push_str(&rendered);
                }
            }
            let has_children = !children.is_empty();
            for (key, item) in children {
                write_value(out, key, item, indent, depth + 1, escape);
            }
            if indent.is_some() && has_children {
                pad(out, depth);
            }
            out.push_str("</");
            out.push_str(name);
            out.push('>');
        }
        Value::Null => {
            pad(out, depth);
            out.push('<');
            out.push_str(name);
            out.push_str("></");
            out.push_str(name);
            out.push('>');
        }
        other => {
            pad(out, depth);
            out.push('<');
            out.push_str(name);
            out.push('>');
            let rendered = other.to_string();
            if escape {
                out.push_str(&escape_text(&rendered));
            } else {
                out.push_str(&rendered);
            }
            out.push_str("</");
            out.push_str(name);
            out.push('>');
        }
    }
}

fn encode_with(m: &Value, root_tag: Option<&str>, indent: Option<&str>) -> Vec<u8> {
    let escape = escape_enabled();
    let mut out = String::new();
    match m {
        Value::Map(map) => {
            if let Some(root) = root_tag {
                write_value(&mut out, root, &Value::Map(map.clone()), indent, 0, escape);
            } else if map.len() == 1 {
                let (name, value) = map.iter().next().expect("one entry");
                write_value(&mut out, name, value, indent, 0, escape);
            } else {
                write_value(&mut out, "doc", &Value::Map(map.clone()), indent, 0, escape);
            }
        }
        other => {
            write_value(
                &mut out,
                root_tag.unwrap_or("doc"),
                other,
                indent,
                0,
                escape,
            );
        }
    }
    out.into_bytes()
}

/// Encodes a map value as XML, using `root_tag` or the single top-level key.
pub fn encode(m: &Value, root_tag: Option<&str>) -> Vec<u8> {
    encode_with(m, root_tag, None)
}

/// Encodes a map value as indented XML (tab indent).
pub fn encode_with_indent(m: &Value, root_tag: Option<&str>) -> Vec<u8> {
    encode_with(m, root_tag, Some("\t"))
}

/// Decodes XML content and encodes it as JSON text.
pub fn to_json(content: &[u8]) -> Result<String, gerror::Error> {
    let value = decode(content)?;
    gf_json::marshal_value(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, Value)]) -> Value {
        let mut map = IndexMap::new();
        for (key, value) in entries {
            map.insert((*key).to_owned(), value.clone());
        }
        Value::Map(map)
    }

    #[test]
    fn decode_matches_go() {
        let content = "\n<?xml version=\"1.0\" encoding=\"UTF-8\"?><doc><username>johngcn</username><password1>123456</password1><password2>123456</password2></doc>\n";
        let decoded = decode(content.as_bytes()).expect("decode");
        let Value::Map(root) = &decoded else {
            panic!("expected map");
        };
        let Value::Map(doc) = &root["doc"] else {
            panic!("expected doc");
        };
        assert_eq!(doc["username"], Value::String("johngcn".into()));
        assert_eq!(doc["password1"], Value::String("123456".into()));

        let inner = decode_without_root(content.as_bytes()).expect("decode");
        let Value::Map(inner) = inner else {
            panic!("expected map");
        };
        assert_eq!(inner["username"], Value::String("johngcn".into()));
    }

    #[test]
    fn decode_repeats_attributes_and_entities() {
        let content = br#"<root a="1"><item>x</item><item>y</item><name>&lt;&gt;&amp;&apos;&quot;AAA</name></root>"#;
        let decoded = decode(content).expect("decode");
        let Value::Map(root) = decoded else {
            panic!("expected map");
        };
        let Value::Map(node) = &root["root"] else {
            panic!("expected root map");
        };
        assert_eq!(
            node["item"],
            Value::List(vec![Value::String("x".into()), Value::String("y".into())])
        );
        assert_eq!(node["-a"], Value::String("1".into()));
        assert_eq!(node["name"], Value::String("<>&'\"AAA".into()));
    }

    #[test]
    fn encode_matches_go() {
        let value = map(&[(
            "root",
            map(&[
                ("string", Value::String("hello world".into())),
                ("int", Value::Int(123)),
                ("float", Value::Float(100.92)),
                ("bool", Value::Bool(true)),
            ]),
        )]);
        assert_eq!(
            String::from_utf8(encode(&value, None)).expect("utf8"),
            "<root><bool>true</bool><float>100.92</float><int>123</int><string>hello world</string></root>"
        );

        xml_escape_chars(true);
        let escaped = map(&[(
            "Person",
            map(&[
                ("Name", Value::String("<>&'\"AAA".into())),
                ("Email", Value::String("john.doe@example.com".into())),
            ]),
        )]);
        assert_eq!(
            String::from_utf8(encode(&escaped, None)).expect("utf8"),
            "<Person><Email>john.doe@example.com</Email><Name>&lt;&gt;&amp;&apos;&quot;AAA</Name></Person>"
        );
        xml_escape_chars(false);

        assert!(!encode_with_indent(&value, Some("xml")).is_empty());
    }

    #[test]
    fn malformed_xml_errors() {
        let bad = b"<root><bool>true</bool>";
        assert!(decode(bad).is_err());
        assert!(to_json(bad).is_err());
        assert!(decode(b"<root>").is_err());
    }

    #[test]
    fn json_output_uses_decoded_values() {
        let content = b"<root><a>1</a><b>x</b></root>";
        assert_eq!(
            to_json(content).expect("json"),
            r#"{"root":{"a":"1","b":"x"}}"#
        );
    }

    #[test]
    fn encoding_declaration_is_converted() {
        let source = "<root><a>Hello</a></root>";
        let declared = format!("<?xml version=\"1.0\" encoding=\"GBK\"?>{source}");
        let gbk = gcharset::utf8_to("gbk", &declared).expect("encode gbk");
        let decoded = decode(&gbk).expect("decode");
        assert_eq!(
            decoded,
            decode(format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>{source}").as_bytes())
                .expect("decode utf8")
        );
    }

    #[test]
    fn attributes_and_text_round_trip() {
        let value = map(&[(
            "root",
            map(&[
                ("-id", Value::String("7".into())),
                ("#text", Value::String("hello".into())),
            ]),
        )]);
        let encoded = String::from_utf8(encode(&value, None)).expect("utf8");
        assert_eq!(encoded, "<root id=\"7\">hello</root>");
        let decoded = decode(encoded.as_bytes()).expect("decode");
        let Value::Map(root) = &decoded else {
            panic!("expected map");
        };
        let Value::Map(node) = &root["root"] else {
            panic!("expected root map");
        };
        assert_eq!(node["-id"], Value::String("7".into()));
        assert_eq!(node["#text"], Value::String("hello".into()));
    }
}
