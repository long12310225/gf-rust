//! Port of `encoding/gyaml`.

use gf_core::Value;
use gf_errors::gerror;

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

/// Encodes `value` as YAML.
pub fn encode(value: &Value) -> Result<Vec<u8>, gerror::Error> {
    let text = serde_yaml::to_string(value).map_err(|err| wrap(err, "yaml.Marshal failed"))?;
    Ok(text.into_bytes())
}

/// Encodes `value` as YAML text.
pub fn encode_to_string(value: &Value) -> Result<String, gerror::Error> {
    let bytes = encode(value)?;
    String::from_utf8(bytes).map_err(|err| wrap(err, "yaml.Marshal failed"))
}

/// Encodes `value` as YAML with `indent` before every line.
pub fn encode_indent(value: &Value, indent: &str) -> Result<Vec<u8>, gerror::Error> {
    let mut out = encode_to_string(value)?;
    if !indent.is_empty() {
        let trimmed = out.trim();
        let mut indented = String::with_capacity(trimmed.len() + indent.len() * 8);
        for line in trimmed.split('\n') {
            indented.push_str(indent);
            indented.push_str(line);
            indented.push('\n');
        }
        out = indented;
    }
    Ok(out.into_bytes())
}

/// Decodes YAML content into a map value.
///
/// The Go original unmarshals into `map[string]any`, so a document whose root
/// is not a mapping is an error.
pub fn decode(content: &[u8]) -> Result<Value, gerror::Error> {
    let value: Value =
        serde_yaml::from_slice(content).map_err(|err| wrap(err, "yaml.Unmarshal failed"))?;
    match value {
        Value::Map(_) => Ok(value),
        other => Err(gerror::Error::new(format!(
            "yaml: cannot unmarshal {} into map[string]interface {{}}",
            other.go_type_name()
        ))),
    }
}

/// Decodes YAML content into `T`.
pub fn decode_to<T: serde::de::DeserializeOwned>(content: &[u8]) -> Result<T, gerror::Error> {
    serde_yaml::from_slice(content).map_err(|err| wrap(err, "yaml.Unmarshal failed"))
}

/// Decodes YAML content and encodes it as JSON text.
pub fn to_json(content: &[u8]) -> Result<String, gerror::Error> {
    let value = decode(content)?;
    gf_json::marshal_value(&value)
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;

    use super::*;

    const YAML_STR: &str = "\n#即表示url属性值；\nurl: https://goframe.org\n\n#数组，即表示server为[a,b,c]\nserver:\n    - 120.168.117.21\n    - 120.168.117.22\n#常量\npi: 3.14   #定义一个数值3.14\nhasChild: true  #定义一个boolean值\nname: '你好YAML'   #定义一个字符串\n";

    #[test]
    fn encode_map_and_array_match_go() {
        let mut map = IndexMap::new();
        map.insert("k".to_owned(), Value::String("v".into()));
        assert_eq!(
            encode_to_string(&Value::Map(map)).expect("encode"),
            "k: v\n"
        );

        let array = Value::List(vec![
            Value::String("a".into()),
            Value::String("b".into()),
            Value::String("c".into()),
        ]);
        assert_eq!(encode_to_string(&array).expect("encode"), "- a\n- b\n- c\n");
        assert_eq!(
            String::from_utf8(encode_indent(&array, "####").expect("encode")).expect("utf8"),
            "####- a\n####- b\n####- c\n"
        );
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "the fixture uses 3.14, copied from the Go test"
    )]
    fn decode_matches_go() {
        let value = decode(YAML_STR.as_bytes()).expect("decode");
        let Value::Map(map) = value else {
            panic!("expected map");
        };
        assert_eq!(map["url"], Value::String("https://goframe.org".into()));
        assert_eq!(
            map["server"],
            Value::List(vec![
                Value::String("120.168.117.21".into()),
                Value::String("120.168.117.22".into()),
            ])
        );
        assert_eq!(map["pi"], Value::Float(3.14));
        assert_eq!(map["hasChild"], Value::Bool(true));
        assert_eq!(map["name"], Value::String("你好YAML".into()));
    }

    #[test]
    fn invalid_documents_fail() {
        let bad = "[redis]\ndd = 11\n[redis]\ndisk = \"x\"\n";
        assert!(decode(bad.as_bytes()).is_err());
        assert!(to_json(bad.as_bytes()).is_err());

        assert!(decode(b"- a\n- b\n").is_err());
        assert!(decode_to::<IndexMap<String, String>>(b"pi: [").is_err());
    }

    #[test]
    fn decoded_map_serializes_like_go() {
        let value = decode(b"m:\n k: v\n").expect("decode");
        assert_eq!(
            gf_json::marshal_value(&value).expect("json"),
            r#"{"m":{"k":"v"}}"#
        );
    }

    #[test]
    #[expect(
        clippy::approx_constant,
        reason = "the fixture uses 3.14, copied from the Go test"
    )]
    fn decode_to_reads_structs() {
        #[derive(serde::Deserialize, Debug, PartialEq)]
        struct Config {
            url: String,
            pi: f64,
        }
        let config: Config = decode_to(b"url: https://goframe.org\npi: 3.14\n").expect("decode");
        assert_eq!(
            config,
            Config {
                url: "https://goframe.org".to_owned(),
                pi: 3.14,
            }
        );
    }
}
