//! Port of `encoding/gurl`.
//!
//! Encoding follows Go's `net/url`: query escaping keeps `A-Za-z0-9-_.~`,
//! writes a space as `+` (or `%20` for the raw form) and everything else as
//! upper-case `%XX`. Decoding reports the first invalid escape; the strict
//! string variants additionally reject invalid UTF-8.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};

use gf_errors::gerror;

fn is_unreserved(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~')
}

fn hex_digit(value: u8) -> char {
    if value < 10 {
        char::from(b'0' + value)
    } else {
        char::from(b'A' + value - 10)
    }
}

fn encode_with(s: &str, space: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.iter().all(|&byte| is_unreserved(byte)) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(bytes.len() + 8);
    for &byte in bytes {
        if is_unreserved(byte) {
            out.push(char::from(byte));
        } else if byte == b' ' {
            out.push_str(space);
        } else {
            out.push('%');
            out.push(hex_digit(byte >> 4));
            out.push(hex_digit(byte & 0x0F));
        }
    }
    out
}

/// Escapes a string for use in a URL query; a space becomes `+`.
pub fn encode(s: &str) -> String {
    encode_with(s, "+")
}

/// Escapes a string per RFC 3986; a space becomes `%20`.
pub fn raw_encode(s: &str) -> String {
    encode_with(s, "%20")
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn invalid_escape(s: &str, index: usize) -> gerror::Error {
    gerror::Error::new(format!("invalid URL escape \"{}\"", &s[index..]))
}

fn unescape_bytes(s: &str, plus_as_space: bool) -> Result<Vec<u8>, gerror::Error> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let high = bytes.get(i + 1).copied().and_then(hex_value);
                let low = bytes.get(i + 2).copied().and_then(hex_value);
                match (high, low) {
                    (Some(high), Some(low)) => {
                        out.push((high << 4) | low);
                        i += 3;
                    }
                    _ => return Err(invalid_escape(s, i)),
                }
            }
            b'+' if plus_as_space => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    Ok(out)
}

fn bytes_to_string(bytes: Vec<u8>) -> Result<String, gerror::Error> {
    String::from_utf8(bytes)
        .map_err(|err| gerror::Error::new(format!("decoded URL is not valid UTF-8: {err}")))
}

fn unescape_cow(s: &str, plus_as_space: bool) -> Result<Cow<'_, str>, gerror::Error> {
    let needs_work = s
        .as_bytes()
        .iter()
        .any(|&byte| byte == b'%' || (plus_as_space && byte == b'+'));
    if !needs_work {
        return Ok(Cow::Borrowed(s));
    }
    bytes_to_string(unescape_bytes(s, plus_as_space)?).map(Cow::Owned)
}

/// Decodes a query-escaped string (`+` becomes a space).
pub fn decode(s: &str) -> Result<String, gerror::Error> {
    Ok(unescape_cow(s, true)?.into_owned())
}

/// Decodes a query-escaped string into its bytes.
pub fn decode_bytes(s: &str) -> Result<Vec<u8>, gerror::Error> {
    unescape_bytes(s, true)
}

/// Decodes a raw (RFC 3986) escaped string; `+` is kept literally.
pub fn raw_decode(s: &str) -> Result<String, gerror::Error> {
    Ok(unescape_cow(s, false)?.into_owned())
}

/// Decodes a raw (RFC 3986) escaped string into its bytes.
pub fn raw_decode_bytes(s: &str) -> Result<Vec<u8>, gerror::Error> {
    unescape_bytes(s, false)
}

/// Builds a URL-encoded query string.
///
/// Keys are emitted in map order, which for [`BTreeMap`] is sorted like Go's
/// `url.Values.Encode`; each key keeps its value order.
pub fn build_query(values: &BTreeMap<String, Vec<String>>) -> String {
    let mut out = String::new();
    for (key, values) in values {
        let key_encoded = encode(key);
        for value in values {
            if !out.is_empty() {
                out.push('&');
            }
            out.push_str(&key_encoded);
            out.push('=');
            out.push_str(&encode(value));
        }
    }
    out
}

struct ParsedUrl<'a> {
    scheme: &'a str,
    user: Cow<'a, str>,
    pass: Cow<'a, str>,
    host: Cow<'a, str>,
    port: Cow<'a, str>,
    path: Cow<'a, str>,
    raw_query: &'a str,
    fragment: Cow<'a, str>,
}

fn split_scheme(input: &str) -> (&str, &str) {
    let Some(colon) = input.find(':') else {
        return ("", input);
    };
    let scheme = &input[..colon];
    let valid = scheme
        .bytes()
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic())
        && scheme
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'-' | b'.'));
    if !valid {
        return ("", input);
    }
    (scheme, &input[colon + 1..])
}

type Authority<'a> = (Cow<'a, str>, Cow<'a, str>, Cow<'a, str>, Cow<'a, str>);

fn parse_authority(authority: &str) -> Result<Authority<'_>, gerror::Error> {
    let (userinfo, hostport) = match authority.rfind('@') {
        Some(at) => (Some(&authority[..at]), &authority[at + 1..]),
        None => (None, authority),
    };
    let (user, pass) = match userinfo {
        Some(info) => match info.split_once(':') {
            Some((user, pass)) => (unescape_cow(user, false)?, unescape_cow(pass, false)?),
            None => (unescape_cow(info, false)?, Cow::Borrowed("")),
        },
        None => (Cow::Borrowed(""), Cow::Borrowed("")),
    };
    let (host, port) = if let Some(stripped) = hostport.strip_prefix('[') {
        match stripped.find(']') {
            Some(end) => {
                let after = &stripped[end + 1..];
                let port = after.strip_prefix(':').unwrap_or("");
                (Cow::Borrowed(&stripped[..end]), Cow::Borrowed(port))
            }
            None => (Cow::Borrowed(hostport), Cow::Borrowed("")),
        }
    } else {
        match hostport.rfind(':') {
            Some(colon) if hostport[colon + 1..].bytes().all(|b| b.is_ascii_digit()) => (
                Cow::Borrowed(&hostport[..colon]),
                Cow::Borrowed(&hostport[colon + 1..]),
            ),
            _ => (Cow::Borrowed(hostport), Cow::Borrowed("")),
        }
    };
    Ok((user, pass, host, port))
}

fn parse_url_inner(input: &str) -> Result<ParsedUrl<'_>, gerror::Error> {
    let (rest, fragment) = match input.split_once('#') {
        Some((rest, fragment)) => (rest, Some(fragment)),
        None => (input, None),
    };
    let (rest, raw_query) = match rest.split_once('?') {
        Some((rest, query)) => (rest, query),
        None => (rest, ""),
    };
    let (scheme, rest) = split_scheme(rest);
    let (authority, path) = if let Some(after) = rest.strip_prefix("//") {
        let end = after.find(['/', '?', '#']).unwrap_or(after.len());
        (&after[..end], &after[end..])
    } else {
        ("", rest)
    };
    let (user, pass, host, port) = parse_authority(authority)?;
    let path = unescape_cow(path, false)?;
    let fragment = match fragment {
        Some(fragment) => unescape_cow(fragment, false)?,
        None => Cow::Borrowed(""),
    };
    Ok(ParsedUrl {
        scheme,
        user,
        pass,
        host,
        port,
        path,
        raw_query,
        fragment,
    })
}

/// Parses a URL and returns the selected components.
///
/// `component` uses the Go bit mask: `-1` for all, `1` scheme, `2` host,
/// `4` port, `8` user, `16` pass, `32` path, `64` query, `128` fragment.
///
/// Values borrow `input` unless percent-decoding was required, and keys are
/// static, so a parse allocates only for the map and for decoded components.
pub fn parse_url<'a>(
    input: &'a str,
    component: i32,
) -> Result<HashMap<&'static str, Cow<'a, str>>, gerror::Error> {
    let parsed = parse_url_inner(input).map_err(|err| {
        match gerror::wrap(Some(err), format!("url.Parse failed for URL \"{input}\"")) {
            Some(err) => err,
            None => gerror::Error::new(format!("url.Parse failed for URL \"{input}\"")),
        }
    })?;
    let component = if component == -1 {
        1 | 2 | 4 | 8 | 16 | 32 | 64 | 128
    } else {
        component
    };
    let mut components = HashMap::with_capacity(8);
    if component & 1 == 1 {
        components.insert("scheme", Cow::Borrowed(parsed.scheme));
    }
    if component & 2 == 2 {
        components.insert("host", parsed.host);
    }
    if component & 4 == 4 {
        components.insert("port", parsed.port);
    }
    if component & 8 == 8 {
        components.insert("user", parsed.user);
    }
    if component & 16 == 16 {
        components.insert("pass", parsed.pass);
    }
    if component & 32 == 32 {
        components.insert("path", parsed.path);
    }
    if component & 64 == 64 {
        components.insert("query", Cow::Borrowed(parsed.raw_query));
    }
    if component & 128 == 128 {
        components.insert("fragment", parsed.fragment);
    }
    Ok(components)
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL_STR: &str = "https://golang.org/x/crypto?go-get=1 +";
    const URL_ENCODE: &str = "https%3A%2F%2Fgolang.org%2Fx%2Fcrypto%3Fgo-get%3D1+%2B";
    const RAW_URL_ENCODE: &str = "https%3A%2F%2Fgolang.org%2Fx%2Fcrypto%3Fgo-get%3D1%20%2B";

    #[test]
    fn encode_and_decode_match_go() {
        assert_eq!(encode(URL_STR), URL_ENCODE);
        assert_eq!(decode(URL_ENCODE).expect("decode"), URL_STR);
        assert_eq!(encode("abc-_.~09AZ"), "abc-_.~09AZ");
        assert_eq!(encode("a b+c&d=e"), "a+b%2Bc%26d%3De");
        assert_eq!(encode("测试"), "%E6%B5%8B%E8%AF%95");
    }

    #[test]
    fn raw_encode_and_decode_match_php() {
        assert_eq!(raw_encode(URL_STR), RAW_URL_ENCODE);
        assert_eq!(raw_decode(RAW_URL_ENCODE).expect("decode"), URL_STR);
        assert_eq!(raw_decode("a+b%20c%2Bd").expect("decode"), "a+b c+d");
        assert_eq!(raw_encode("a b"), "a%20b");
    }

    #[test]
    fn invalid_escapes_are_reported() {
        assert!(raw_decode("%zz").is_err());
        assert!(decode("%").is_err());
        assert!(decode("%2").is_err());
        assert!(decode("%GG").is_err());
        assert_eq!(decode_bytes("%FF").expect("bytes"), vec![0xFF]);
        assert!(decode("%FF").is_err());
    }

    #[test]
    fn build_query_sorts_keys_and_keeps_values() {
        let mut values = BTreeMap::new();
        values.insert("a".to_owned(), vec!["a2".to_owned(), "a1".to_owned()]);
        values.insert("b".to_owned(), vec!["b2".to_owned(), "b1".to_owned()]);
        values.insert("c".to_owned(), vec!["c1".to_owned(), "c2".to_owned()]);
        assert_eq!(build_query(&values), "a=a2&a=a1&b=b2&b=b1&c=c1&c=c2");
        let mut spaced = BTreeMap::new();
        spaced.insert("k y".to_owned(), vec!["v+1".to_owned()]);
        assert_eq!(build_query(&spaced), "k+y=v%2B1");
    }

    #[test]
    fn parse_url_returns_the_go_components() {
        let src = "http://username:password@hostname:9090/path?arg=value#anchor";
        let all = parse_url(src, -1).expect("parse");
        assert_eq!(all.len(), 8);
        for (key, value) in [
            ("scheme", "http"),
            ("host", "hostname"),
            ("port", "9090"),
            ("user", "username"),
            ("pass", "password"),
            ("path", "/path"),
            ("query", "arg=value"),
            ("fragment", "anchor"),
        ] {
            assert_eq!(all[key].as_ref(), value);
        }

        for (bit, key, value) in [
            (1, "scheme", "http"),
            (2, "host", "hostname"),
            (4, "port", "9090"),
            (8, "user", "username"),
            (16, "pass", "password"),
            (32, "path", "/path"),
            (64, "query", "arg=value"),
            (128, "fragment", "anchor"),
        ] {
            let components = parse_url(src, bit).expect("parse");
            assert_eq!(components.len(), 1);
            assert_eq!(components[key].as_ref(), value);
        }
    }

    #[test]
    fn parse_url_handles_ipv6_decoding_and_missing_parts() {
        let components = parse_url("https://[::1]:8443/a%20b?x=%2B#f%2Bg", -1).expect("parse");
        assert_eq!(components["host"].as_ref(), "::1");
        assert_eq!(components["port"].as_ref(), "8443");
        assert_eq!(components["path"].as_ref(), "/a b");
        assert_eq!(components["query"].as_ref(), "x=%2B");
        assert_eq!(components["fragment"].as_ref(), "f+g");
        assert_eq!(components["user"].as_ref(), "");

        let relative = parse_url("/just/path", -1).expect("parse");
        assert_eq!(relative["scheme"], "");
        assert_eq!(relative["host"], "");
        assert_eq!(relative["path"], "/just/path");

        assert!(parse_url("http://host/%zz", -1).is_err());
    }
}
