//! Port of `encoding/gbase64`.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use gf_errors::gerror;

fn wrap(err: impl std::error::Error + Send + Sync + 'static, text: &str) -> gerror::Error {
    match gerror::wrap_std(Some(Box::new(err)), text) {
        Some(err) => err,
        None => gerror::Error::new(text),
    }
}

/// Encodes bytes with BASE64.
pub fn encode(src: &[u8]) -> Vec<u8> {
    STANDARD.encode(src).into_bytes()
}

/// Encodes a string with BASE64.
pub fn encode_string(src: &str) -> String {
    STANDARD.encode(src)
}

/// Encodes bytes to a BASE64 string in one allocation.
pub fn encode_to_string(src: &[u8]) -> String {
    STANDARD.encode(src)
}

/// Encodes the content of `path` with BASE64.
pub fn encode_file(path: &str) -> Result<Vec<u8>, gerror::Error> {
    let content = std::fs::read(path)
        .map_err(|err| wrap(err, &format!("os.ReadFile failed for filename \"{path}\"")))?;
    Ok(encode(&content))
}

/// Encodes the content of `path`, panicking on failure like Go's `Must*`.
pub fn must_encode_file(path: &str) -> Vec<u8> {
    match encode_file(path) {
        Ok(result) => result,
        Err(err) => panic!("{err}"),
    }
}

/// Encodes the content of `path` to a BASE64 string.
pub fn encode_file_to_string(path: &str) -> Result<String, gerror::Error> {
    encode_file(path).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Encodes the content of `path` to a BASE64 string, panicking on failure.
pub fn must_encode_file_to_string(path: &str) -> String {
    match encode_file_to_string(path) {
        Ok(result) => result,
        Err(err) => panic!("{err}"),
    }
}

fn without_newlines(data: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if data.iter().all(|&b| b != b'\r' && b != b'\n') {
        return std::borrow::Cow::Borrowed(data);
    }
    let mut cleaned = Vec::with_capacity(data.len());
    for &byte in data {
        if byte != b'\r' && byte != b'\n' {
            cleaned.push(byte);
        }
    }
    std::borrow::Cow::Owned(cleaned)
}

/// Decodes bytes encoded with BASE64. `\r` and `\n` are ignored, as in Go.
pub fn decode(data: &[u8]) -> Result<Vec<u8>, gerror::Error> {
    let cleaned = without_newlines(data);
    STANDARD
        .decode(cleaned.as_ref())
        .map_err(|err| wrap(err, "base64.StdEncoding.Decode failed"))
}

/// Decodes bytes encoded with BASE64, panicking on failure.
pub fn must_decode(data: &[u8]) -> Vec<u8> {
    match decode(data) {
        Ok(result) => result,
        Err(err) => panic!("{err}"),
    }
}

/// Decodes a BASE64 string.
pub fn decode_string(data: &str) -> Result<Vec<u8>, gerror::Error> {
    decode(data.as_bytes())
}

/// Decodes a BASE64 string, panicking on failure.
pub fn must_decode_string(data: &str) -> Vec<u8> {
    match decode_string(data) {
        Ok(result) => result,
        Err(err) => panic!("{err}"),
    }
}

/// Decodes a BASE64 string to a UTF-8 string.
pub fn decode_to_string(data: &str) -> Result<String, gerror::Error> {
    let bytes = decode_string(data)?;
    String::from_utf8(bytes)
        .map_err(|err| gerror::Error::new(format!("decoded bytes are not valid UTF-8: {err}")))
}

/// Decodes a BASE64 string to a UTF-8 string, panicking on failure.
pub fn must_decode_to_string(data: &str) -> String {
    match decode_to_string(data) {
        Ok(result) => result,
        Err(err) => panic!("{err}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BINARY_PAIRS: &[(&[u8], &str)] = &[
        (b"\x14\xfb\x9c\x03\xd9\x7e", "FPucA9l+"),
        (b"\x14\xfb\x9c\x03\xd9", "FPucA9k="),
        (b"\x14\xfb\x9c\x03", "FPucAw=="),
    ];

    const TEXT_PAIRS: &[(&str, &str)] = &[
        ("", ""),
        ("f", "Zg=="),
        ("fo", "Zm8="),
        ("foo", "Zm9v"),
        ("foob", "Zm9vYg=="),
        ("fooba", "Zm9vYmE="),
        ("foobar", "Zm9vYmFy"),
        ("sure.", "c3VyZS4="),
        ("sure", "c3VyZQ=="),
        ("sur", "c3Vy"),
        ("su", "c3U="),
        ("leasure.", "bGVhc3VyZS4="),
        ("easure.", "ZWFzdXJlLg=="),
        ("asure.", "YXN1cmUu"),
    ];

    #[test]
    fn basic_pairs_match_go() {
        for (decoded, encoded) in BINARY_PAIRS {
            assert_eq!(encode(decoded), encoded.as_bytes());
            assert_eq!(encode_to_string(decoded), *encoded);
            assert_eq!(decode(encoded.as_bytes()).expect("decode"), *decoded);
        }
        for (decoded, encoded) in TEXT_PAIRS {
            let decoded_bytes = decoded.as_bytes();
            assert_eq!(encode(decoded_bytes), encoded.as_bytes());
            assert_eq!(encode_to_string(decoded_bytes), *encoded);
            assert_eq!(encode_string(decoded), *encoded);

            assert_eq!(decode(encoded.as_bytes()).expect("decode"), decoded_bytes);
            assert_eq!(decode_string(encoded).expect("decode"), decoded_bytes);
            assert_eq!(decode_to_string(encoded).expect("decode"), *decoded);
        }
    }

    #[test]
    fn newlines_are_ignored() {
        assert_eq!(decode(b"Zm9v\r\nYmFy").expect("decode"), b"foobar");
    }

    #[test]
    fn invalid_input_errors() {
        assert!(decode(b"!!!!").is_err());
        assert!(decode(b"Zg=").is_err());
    }

    #[test]
    fn files_round_trip() {
        let dir = std::env::temp_dir().join("gf-base64-test");
        std::fs::create_dir_all(&dir).expect("create dir");
        let path = dir.join("test");
        std::fs::write(&path, b"test").expect("write");
        let path = path.to_string_lossy().into_owned();

        assert_eq!(encode_file(&path).expect("encode"), b"dGVzdA==");
        assert_eq!(encode_file_to_string(&path).expect("encode"), "dGVzdA==");

        let missing = dir.join("missing").to_string_lossy().into_owned();
        assert!(encode_file(&missing).is_err());
        assert!(encode_file_to_string(&missing).is_err());

        std::fs::remove_file(&path).ok();
    }
}
