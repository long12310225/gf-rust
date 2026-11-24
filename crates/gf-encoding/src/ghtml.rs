//! Port of `encoding/ghtml`.

use gf_core::Value;
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;
use indexmap::IndexMap;

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn find_ignore_case(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle))
}

/// Strips HTML tags and comments, returning only text.
///
/// Script and style bodies are dropped; textarea/title content is text and is
/// kept. A `<` that does not start markup (for example `I <3 Ponies`) is kept.
pub fn strip_tags(html: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    let mut text_start = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        let next = bytes.get(i + 1).copied();
        let starts_markup = matches!(
            next,
            Some(byte) if byte.is_ascii_alphabetic() || byte == b'!' || byte == b'/' || byte == b'?'
        );
        if !starts_markup {
            i += 1;
            continue;
        }
        out.push_str(&html[text_start..i]);
        if bytes[i..].starts_with(b"<!--") {
            i = match find(&bytes[i + 4..], b"-->") {
                Some(offset) => i + 4 + offset + 3,
                None => bytes.len(),
            };
            text_start = i;
            continue;
        }
        let closing = next == Some(b'/');
        let name_start = if closing { i + 2 } else { i + 1 };
        let name_end = bytes[name_start..]
            .iter()
            .position(|byte| !(byte.is_ascii_alphanumeric()))
            .map_or(bytes.len(), |offset| name_start + offset);
        let name = &bytes[name_start..name_end];
        let mut j = i + 1;
        let mut quote = 0u8;
        while j < bytes.len() {
            let byte = bytes[j];
            if quote != 0 {
                if byte == quote {
                    quote = 0;
                }
            } else if byte == b'"' || byte == b'\'' {
                quote = byte;
            } else if byte == b'>' {
                break;
            }
            j += 1;
        }
        let tag_end = if j < bytes.len() { j + 1 } else { bytes.len() };
        if !closing && (name.eq_ignore_ascii_case(b"script") || name.eq_ignore_ascii_case(b"style"))
        {
            let mut marker = Vec::with_capacity(name.len() + 2);
            marker.extend_from_slice(b"</");
            marker.extend_from_slice(name);
            i = find_ignore_case(&bytes[tag_end..], &marker)
                .map_or(bytes.len(), |offset| tag_end + offset);
            text_start = i;
            continue;
        }
        if !closing
            && (name.eq_ignore_ascii_case(b"textarea") || name.eq_ignore_ascii_case(b"title"))
        {
            let mut marker = Vec::with_capacity(name.len() + 2);
            marker.extend_from_slice(b"</");
            marker.extend_from_slice(name);
            match find_ignore_case(&bytes[tag_end..], &marker) {
                Some(offset) => {
                    let closing_start = tag_end + offset;
                    out.push_str(&html[tag_end..closing_start]);
                    i = closing_start;
                }
                None => {
                    out.push_str(&html[tag_end..]);
                    i = bytes.len();
                }
            }
            text_start = i;
            continue;
        }
        i = tag_end;
        text_start = i;
    }
    out.push_str(&html[text_start..]);
    out
}

/// Escapes the five HTML special characters, like Go's `html.EscapeString`.
pub fn entities(s: &str) -> String {
    let bytes = s.as_bytes();
    if !bytes
        .iter()
        .any(|&byte| matches!(byte, b'&' | b'\'' | b'<' | b'>' | b'"'))
    {
        return s.to_owned();
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    let mut last = 0;
    for (index, &byte) in bytes.iter().enumerate() {
        let replacement = match byte {
            b'&' => "&amp;",
            b'\'' => "&#39;",
            b'<' => "&lt;",
            b'>' => "&gt;",
            b'"' => "&#34;",
            _ => continue,
        };
        out.push_str(&s[last..index]);
        out.push_str(replacement);
        last = index + 1;
    }
    out.push_str(&s[last..]);
    out
}

/// Alias of [`entities`], as in the Go package.
pub fn special_chars(s: &str) -> String {
    entities(s)
}

/// Decodes the five entities written by [`entities`].
pub fn special_chars_decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp..];
        let replacement = [
            ("&amp;", "&"),
            ("&lt;", "<"),
            ("&gt;", ">"),
            ("&#34;", "\""),
            ("&#39;", "'"),
        ]
        .into_iter()
        .find(|(entity, _)| tail.starts_with(entity));
        match replacement {
            Some((entity, value)) => {
                out.push_str(value);
                rest = &tail[entity.len()..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

const NAMED_ENTITIES: &[(&str, &str)] = &[
    ("amp", "&"),
    ("AMP", "&"),
    ("lt", "<"),
    ("LT", "<"),
    ("gt", ">"),
    ("GT", ">"),
    ("quot", "\""),
    ("QUOT", "\""),
    ("apos", "'"),
    ("nbsp", "\u{00A0}"),
    ("iexcl", "¡"),
    ("cent", "¢"),
    ("pound", "£"),
    ("curren", "¤"),
    ("yen", "¥"),
    ("brvbar", "¦"),
    ("sect", "§"),
    ("uml", "¨"),
    ("copy", "©"),
    ("ordf", "ª"),
    ("laquo", "«"),
    ("not", "¬"),
    ("shy", "\u{00AD}"),
    ("reg", "®"),
    ("macr", "¯"),
    ("deg", "°"),
    ("plusmn", "±"),
    ("sup2", "²"),
    ("sup3", "³"),
    ("acute", "´"),
    ("micro", "µ"),
    ("para", "¶"),
    ("middot", "·"),
    ("cedil", "¸"),
    ("sup1", "¹"),
    ("ordm", "º"),
    ("raquo", "»"),
    ("frac14", "¼"),
    ("frac12", "½"),
    ("frac34", "¾"),
    ("iquest", "¿"),
    ("Agrave", "À"),
    ("Aacute", "Á"),
    ("Acirc", "Â"),
    ("Atilde", "Ã"),
    ("Auml", "Ä"),
    ("Aring", "Å"),
    ("AElig", "Æ"),
    ("Ccedil", "Ç"),
    ("Egrave", "È"),
    ("Eacute", "É"),
    ("Ecirc", "Ê"),
    ("Euml", "Ë"),
    ("Igrave", "Ì"),
    ("Iacute", "Í"),
    ("Icirc", "Î"),
    ("Iuml", "Ï"),
    ("ETH", "Ð"),
    ("Ntilde", "Ñ"),
    ("Ograve", "Ò"),
    ("Oacute", "Ó"),
    ("Ocirc", "Ô"),
    ("Otilde", "Õ"),
    ("Ouml", "Ö"),
    ("times", "×"),
    ("Oslash", "Ø"),
    ("Ugrave", "Ù"),
    ("Uacute", "Ú"),
    ("Ucirc", "Û"),
    ("Uuml", "Ü"),
    ("Yacute", "Ý"),
    ("THORN", "Þ"),
    ("szlig", "ß"),
    ("agrave", "à"),
    ("aacute", "á"),
    ("acirc", "â"),
    ("atilde", "ã"),
    ("auml", "ä"),
    ("aring", "å"),
    ("aelig", "æ"),
    ("ccedil", "ç"),
    ("egrave", "è"),
    ("eacute", "é"),
    ("ecirc", "ê"),
    ("euml", "ë"),
    ("igrave", "ì"),
    ("iacute", "í"),
    ("icirc", "î"),
    ("iuml", "ï"),
    ("eth", "ð"),
    ("ntilde", "ñ"),
    ("ograve", "ò"),
    ("oacute", "ó"),
    ("ocirc", "ô"),
    ("otilde", "õ"),
    ("ouml", "ö"),
    ("divide", "÷"),
    ("oslash", "ø"),
    ("ugrave", "ù"),
    ("uacute", "ú"),
    ("ucirc", "û"),
    ("uuml", "ü"),
    ("yacute", "ý"),
    ("thorn", "þ"),
    ("yuml", "ÿ"),
    ("ndash", "–"),
    ("mdash", "—"),
    ("lsquo", "‘"),
    ("rsquo", "’"),
    ("ldquo", "“"),
    ("rdquo", "”"),
    ("bull", "•"),
    ("hellip", "…"),
    ("prime", "′"),
    ("Prime", "″"),
    ("euro", "€"),
    ("trade", "™"),
    ("larr", "←"),
    ("uarr", "↑"),
    ("rarr", "→"),
    ("darr", "↓"),
    ("harr", "↔"),
    ("lArr", "⇐"),
    ("rArr", "⇒"),
    ("hArr", "⇔"),
    ("forall", "∀"),
    ("part", "∂"),
    ("exist", "∃"),
    ("empty", "∅"),
    ("nabla", "∇"),
    ("isin", "∈"),
    ("notin", "∉"),
    ("ni", "∋"),
    ("prod", "∏"),
    ("sum", "∑"),
    ("minus", "−"),
    ("lowast", "∗"),
    ("radic", "√"),
    ("prop", "∝"),
    ("infin", "∞"),
    ("and", "∧"),
    ("or", "∨"),
    ("cap", "∩"),
    ("cup", "∪"),
    ("int", "∫"),
    ("there4", "∴"),
    ("sim", "∼"),
    ("cong", "≅"),
    ("asymp", "≈"),
    ("ne", "≠"),
    ("equiv", "≡"),
    ("le", "≤"),
    ("ge", "≥"),
    ("sub", "⊂"),
    ("sup", "⊃"),
    ("nsub", "⊄"),
    ("sube", "⊆"),
    ("supe", "⊇"),
    ("oplus", "⊕"),
    ("otimes", "⊗"),
    ("perp", "⊥"),
    ("sdot", "⋅"),
    ("loz", "◊"),
    ("spades", "♠"),
    ("clubs", "♣"),
    ("hearts", "♥"),
    ("diams", "♦"),
    ("ensp", "\u{2002}"),
    ("emsp", "\u{2003}"),
    ("thinsp", "\u{2009}"),
    ("zwnj", "\u{200C}"),
    ("zwj", "\u{200D}"),
    ("lrm", "\u{200E}"),
    ("rlm", "\u{200F}"),
];

fn named_entity(name: &str) -> Option<&'static str> {
    NAMED_ENTITIES
        .iter()
        .find(|(candidate, _)| *candidate == name)
        .map(|(_, value)| *value)
}

fn numeric_entity(digits: &str, radix: u32) -> Option<char> {
    let value = u32::from_str_radix(digits, radix).ok()?;
    if value == 0 || value > 0x10FFFF || (0xD800..=0xDFFF).contains(&value) {
        return Some('\u{FFFD}');
    }
    Some(char::from_u32(value).unwrap_or('\u{FFFD}'))
}

/// Decodes HTML entities, numeric or named, like Go's `html.UnescapeString`.
pub fn entities_decode(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let tail = &rest[amp + 1..];
        if let Some(after_hash) = tail.strip_prefix('#') {
            let (radix, digits_start) = match after_hash.as_bytes().first() {
                Some(b'x') | Some(b'X') => (16u32, 1),
                _ => (10u32, 0),
            };
            let digits = after_hash[digits_start..]
                .bytes()
                .take_while(|byte| {
                    if radix == 16 {
                        byte.is_ascii_hexdigit()
                    } else {
                        byte.is_ascii_digit()
                    }
                })
                .count();
            if digits > 0 {
                let number = &after_hash[digits_start..digits_start + digits];
                if let Some(ch) = numeric_entity(number, radix) {
                    out.push(ch);
                    let mut consumed = 1 + digits_start + digits;
                    if after_hash.as_bytes().get(digits_start + digits) == Some(&b';') {
                        consumed += 1;
                    }
                    rest = &tail[consumed..];
                    continue;
                }
            }
            out.push('&');
            rest = tail;
            continue;
        }
        let name_len = tail
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric())
            .count();
        if name_len > 0 {
            let name = &tail[..name_len];
            let has_semicolon = tail.as_bytes().get(name_len) == Some(&b';');
            let legacy = matches!(
                name,
                "amp" | "AMP" | "lt" | "LT" | "gt" | "GT" | "quot" | "QUOT"
            );
            if (has_semicolon || legacy)
                && let Some(value) = named_entity(name)
            {
                out.push_str(value);
                let mut consumed = name_len;
                if has_semicolon {
                    consumed += 1;
                }
                rest = &tail[consumed..];
                continue;
            }
        }
        out.push('&');
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Escapes the string values of a map, like Go's `SpecialCharsMapOrStruct`
/// restricted to its map form.
pub fn special_chars_map(map: &mut IndexMap<String, Value>) {
    for value in map.values_mut() {
        if let Value::String(text) = value {
            *text = entities(text);
        }
    }
}

/// Escapes the string values of a map value; other kinds return Go's
/// `CodeInvalidParameter` error.
pub fn special_chars_map_or_struct(value: &mut Value) -> Result<(), gerror::Error> {
    match value {
        Value::Map(map) => {
            special_chars_map(map);
            Ok(())
        }
        other => Err(gerror::Error::new_codef(
            &CODE_INVALID_PARAMETER,
            "invalid input parameter type \"%s\"",
            vec![Value::String(other.go_type_name().to_owned())],
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"A 'quote' "is" <b>bold</b>"#;
    const DST: &str = "A &#39;quote&#39; &#34;is&#34; &lt;b&gt;bold&lt;/b&gt;";

    #[test]
    fn strip_tags_matches_go() {
        assert_eq!(
            strip_tags(
                r##"<p>Test paragraph.</p><!-- Comment -->  <a href="#fragment">Other text</a>"##
            ),
            "Test paragraph.  Other text"
        );
        assert_eq!(
            strip_tags(r##"<b>&iexcl;Hi!</b> <script>var x = "<div>";</script>after"##),
            "&iexcl;Hi! after"
        );
        assert_eq!(
            strip_tags(r##"<div title="1>2">x</div><style>p{}</style>"##),
            "x"
        );
        assert_eq!(strip_tags("I <3 Ponies!"), "I <3 Ponies!");
        assert_eq!(strip_tags("<textarea>a<b>c</b></textarea>"), "a<b>c</b>");
        assert_eq!(strip_tags("plain"), "plain");
        assert_eq!(strip_tags("<a href='x"), "");
    }

    #[test]
    fn entities_round_trip() {
        assert_eq!(entities(SRC), DST);
        assert_eq!(entities_decode(DST), SRC);
        assert_eq!(special_chars(SRC), DST);
        assert_eq!(special_chars_decode(DST), SRC);
        assert_eq!(entities("plain 中文"), "plain 中文");
    }

    #[test]
    fn entities_decode_named_and_numeric() {
        assert_eq!(entities_decode("&amp;&lt;&gt;&#34;&#39;"), "&<>\"'");
        assert_eq!(entities_decode("&aacute;&nbsp;&copy;"), "á\u{00A0}©");
        assert_eq!(entities_decode("&#65;&#x42;&#X43;"), "ABC");
        assert_eq!(
            entities_decode("&#128;&#x110000;&#0;"),
            "\u{0080}\u{FFFD}\u{FFFD}"
        );
        assert_eq!(
            entities_decode("&unknown; &amp without"),
            "&unknown; & without"
        );
        assert_eq!(entities_decode("100% & no entity"), "100% & no entity");
    }

    #[test]
    fn special_chars_decode_leaves_other_entities() {
        assert_eq!(
            special_chars_decode("&amp; &lt; &copy; &#39;"),
            "& < &copy; '"
        );
    }

    #[test]
    fn special_chars_map_only_touches_strings() {
        let mut map = IndexMap::new();
        map.insert("Title".to_owned(), Value::String("<h1>T</h1>".into()));
        map.insert("Count".to_owned(), Value::Int(1));
        special_chars_map(&mut map);
        assert_eq!(map["Title"], Value::String("&lt;h1&gt;T&lt;/h1&gt;".into()));
        assert_eq!(map["Count"], Value::Int(1));
    }
}
