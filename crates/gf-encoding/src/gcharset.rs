//! Port of `encoding/gcharset`.
//!
//! Conversion works on bytes, because target encodings like GBK do not produce
//! valid UTF-8 (`String` cannot hold them). `convert_string` is the UTF-8-only
//! convenience form. HZ-GB-2312 is implemented on top of GBK, matching Go's
//! alias of `GB2312` to `HZ-GB-2312`.

use std::borrow::Cow;

use encoding_rs::{Encoding, GBK, UTF_16BE, UTF_16LE};
use gf_errors::gcode::CODE_INVALID_PARAMETER;
use gf_errors::gerror;

fn alias(charset: &str) -> &str {
    match charset {
        "HZGB2312" | "hzgb2312" | "GB2312" | "gb2312" => "HZ-GB-2312",
        other => other,
    }
}

fn is_utf8(name: &str) -> bool {
    name.eq_ignore_ascii_case("utf-8") || name.eq_ignore_ascii_case("utf8")
}

fn is_hz(name: &str) -> bool {
    name.eq_ignore_ascii_case("HZ-GB-2312")
}

fn is_utf16(name: &str) -> bool {
    name.eq_ignore_ascii_case("utf-16")
}

fn encoding_for(charset: &str) -> Option<&'static Encoding> {
    Encoding::for_label(alias(charset).as_bytes())
}

/// Reports whether `charset` is supported.
pub fn supported(charset: &str) -> bool {
    is_utf8(charset)
        || is_hz(alias(charset))
        || is_utf16(charset)
        || encoding_for(charset).is_some()
}

fn unsupported(kind: &str, charset: &str) -> gerror::Error {
    gerror::Error::new_codef(
        &CODE_INVALID_PARAMETER,
        format!("unsupported {kind} \"%s\""),
        vec![gf_core::Value::String(charset.to_owned())],
    )
}

fn hz_to_gbk(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut in_gb = false;
    let mut i = 0;
    while i < bytes.len() {
        let byte = bytes[i];
        if byte == b'~' {
            match bytes.get(i + 1).copied() {
                Some(b'~') => {
                    out.push(if in_gb { 0x7E | 0x80 } else { b'~' });
                    i += 2;
                }
                Some(b'{') => {
                    in_gb = true;
                    i += 2;
                }
                Some(b'}') => {
                    in_gb = false;
                    i += 2;
                }
                Some(b'\n') => i += 2,
                _ => i += 1,
            }
            continue;
        }
        out.push(if in_gb { byte | 0x80 } else { byte });
        i += 1;
    }
    out
}

fn hz_from_gbk(gbk: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(gbk.len() + 8);
    let mut in_gb = false;
    let mut i = 0;
    while i < gbk.len() {
        let byte = gbk[i];
        if byte < 0x80 {
            if in_gb {
                out.extend_from_slice(b"~}");
                in_gb = false;
            }
            if byte == b'~' {
                out.extend_from_slice(b"~~");
            } else {
                out.push(byte);
            }
            i += 1;
            continue;
        }
        if !in_gb {
            out.extend_from_slice(b"~{");
            in_gb = true;
        }
        let stripped = byte & 0x7F;
        if stripped == b'~' {
            out.extend_from_slice(b"~~");
        } else {
            out.push(stripped);
        }
        i += 1;
    }
    out
}

fn utf16_bytes(input: &str, big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len() * 2);
    for unit in input.encode_utf16() {
        if big_endian {
            out.extend_from_slice(&unit.to_be_bytes());
        } else {
            out.extend_from_slice(&unit.to_le_bytes());
        }
    }
    out
}

fn decode_with(charset: &str, input: &[u8]) -> Result<String, gerror::Error> {
    if is_utf8(charset) {
        return Ok(String::from_utf8_lossy(input).into_owned());
    }
    if is_hz(alias(charset)) {
        let gbk = hz_to_gbk(input);
        return Ok(GBK.decode(&gbk).0.into_owned());
    }
    if is_utf16(charset) {
        if input.starts_with(&[0xFE, 0xFF]) {
            return Ok(UTF_16BE.decode(&input[2..]).0.into_owned());
        }
        if input.starts_with(&[0xFF, 0xFE]) {
            return Ok(UTF_16LE.decode(&input[2..]).0.into_owned());
        }
        return Ok(UTF_16BE.decode(input).0.into_owned());
    }
    let encoding = encoding_for(charset).ok_or_else(|| unsupported("srcCharset", charset))?;
    Ok(encoding.decode(input).0.into_owned())
}

fn encode_with(charset: &str, input: &str) -> Result<Vec<u8>, gerror::Error> {
    if is_utf8(charset) {
        return Ok(input.as_bytes().to_vec());
    }
    if is_hz(alias(charset)) {
        let (gbk, _, _) = GBK.encode(input);
        return Ok(hz_from_gbk(&gbk));
    }
    if charset.eq_ignore_ascii_case("utf-16le") {
        return Ok(utf16_bytes(input, false));
    }
    if charset.eq_ignore_ascii_case("utf-16be") {
        return Ok(utf16_bytes(input, true));
    }
    if is_utf16(charset) {
        let mut out = Vec::with_capacity(input.len() * 2 + 2);
        out.extend_from_slice(&[0xFE, 0xFF]);
        out.extend_from_slice(&utf16_bytes(input, true));
        return Ok(out);
    }
    let encoding = encoding_for(charset).ok_or_else(|| unsupported("dstCharset", charset))?;
    Ok(encoding.encode(input).0.into_owned())
}

/// Converts `input` from `src_charset` to `dst_charset`, returning bytes.
pub fn convert_bytes(
    dst_charset: &str,
    src_charset: &str,
    input: &[u8],
) -> Result<Vec<u8>, gerror::Error> {
    if dst_charset == src_charset {
        return Ok(input.to_vec());
    }
    let text = decode_with(src_charset, input)?;
    encode_with(dst_charset, &text)
}

/// Converts `input` to UTF-8 text.
pub fn to_utf8(src_charset: &str, input: &[u8]) -> Result<String, gerror::Error> {
    decode_with(src_charset, input)
}

/// Converts UTF-8 text to `dst_charset` bytes.
pub fn utf8_to(dst_charset: &str, input: &str) -> Result<Vec<u8>, gerror::Error> {
    encode_with(dst_charset, input)
}

/// Converts a UTF-8 source string to UTF-8 (or back to UTF-8), the Go
/// `Convert` shape for textual destinations.
pub fn convert_string(
    dst_charset: &str,
    src_charset: &str,
    input: &str,
) -> Result<String, gerror::Error> {
    let bytes = convert_bytes(dst_charset, src_charset, input.as_bytes())?;
    if is_utf8(dst_charset) {
        return Ok(String::from_utf8_lossy(&bytes).into_owned());
    }
    String::from_utf8(bytes).map_err(|err| {
        gerror::Error::new(format!(
            "converted bytes are not valid UTF-8, use convert_bytes: {err}"
        ))
    })
}

/// Converts `input` from `src_charset` to `dst_charset`, returning bytes.
pub fn convert(
    dst_charset: &str,
    src_charset: &str,
    input: &[u8],
) -> Result<Vec<u8>, gerror::Error> {
    convert_bytes(dst_charset, src_charset, input)
}

/// Returns the decoded text as a borrowed string when the input is already
/// UTF-8, avoiding an allocation.
pub fn decode_cow<'a>(charset: &str, input: &'a [u8]) -> Result<Cow<'a, str>, gerror::Error> {
    if is_utf8(charset) {
        return Ok(String::from_utf8_lossy(input));
    }
    Ok(Cow::Owned(decode_with(charset, input)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_and_utf16_round_trip() {
        assert!(supported("UTF-8"));
        assert!(!supported("UTF-80"));
        assert!(supported("gbk"));
        assert!(supported("GB2312"));

        let text = "これは漢字です。";
        let le = b"\x53\x30\x8c\x30\x6f\x30\x22\x6f\x57\x5b\x67\x30\x59\x30\x02\x30";
        assert_eq!(to_utf8("UTF-16LE", le).expect("decode"), text);
        assert_eq!(utf8_to("UTF-16LE", text).expect("encode"), le);

        let be = b"\x30\x53\x30\x8c\x30\x6f\x6f\x22\x5b\x57\x30\x67\x30\x59\x30\x02";
        assert_eq!(to_utf8("UTF-16BE", be).expect("decode"), text);
        assert_eq!(utf8_to("UTF-16BE", text).expect("encode"), be);

        let with_bom = b"\xfe\xff\x30\x53\x30\x8c\x30\x6f\x6f\x22\x5b\x57\x30\x67\x30\x59\x30\x02";
        assert_eq!(to_utf8("UTF-16", with_bom).expect("decode"), text);
        assert_eq!(utf8_to("UTF-16", text).expect("encode"), with_bom);

        let astral = "𝄢𝄞𝄪𝄫";
        let mut bom = vec![0xFE, 0xFF];
        bom.extend_from_slice(&utf8_to("UTF-16BE", astral).expect("encode"));
        assert_eq!(to_utf8("UTF-16", &bom).expect("decode"), astral);
    }

    #[test]
    fn single_byte_and_cjk_encodings() {
        assert_eq!(
            to_utf8("ISO-8859-15", b"\xa41 is cheap").expect("decode"),
            "€1 is cheap"
        );
        assert_eq!(
            utf8_to("ISO-8859-15", "€1 is cheap").expect("encode"),
            b"\xa41 is cheap"
        );
        assert_eq!(
            to_utf8("windows-1252", b"R\xe9sum\xe9").expect("decode"),
            "Résumé"
        );
        assert_eq!(
            utf8_to("KOI8-R", "русский").expect("encode"),
            b"\xd2\xd5\xd3\xd3\xcb\xc9\xca"
        );
        assert_eq!(
            to_utf8("KOI8-R", b"\xd2\xd5\xd3\xd3\xcb\xc9\xca").expect("decode"),
            "русский"
        );

        let big5: &[u8] = &[
            0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0xb1, 0x60, 0xa5, 0xce, 0xb0, 0xea, 0xa6, 0x72,
            0xbc, 0xd0, 0xb7, 0xc7, 0xa6, 0x72, 0xc5, 0xe9, 0xaa, 0xed,
        ];
        assert_eq!(
            to_utf8("big5", big5).expect("decode"),
            "Hello 常用國字標準字體表"
        );
        assert_eq!(
            utf8_to("big5", "Hello 常用國字標準字體表").expect("encode"),
            big5
        );
        let gbk: &[u8] = &[
            0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0xb3, 0xa3, 0xd3, 0xc3, 0x87, 0xf8, 0xd7, 0xd6,
            0x98, 0xcb, 0x9c, 0xca, 0xd7, 0xd6, 0xf3, 0x77, 0xb1, 0xed,
        ];
        assert_eq!(
            utf8_to("gbk", "Hello 常用國字標準字體表").expect("encode"),
            gbk
        );
        let gb18030 = b"\x82\x31\x89\x38";
        assert_eq!(to_utf8("gb18030", gb18030).expect("decode"), "㧯");
        assert_eq!(utf8_to("gb18030", "㧯").expect("encode"), gb18030);

        assert_eq!(convert_bytes("gbk", "big5", big5).expect("convert"), gbk);

        let euc_jp = b"\xa4\xb3\xa4\xec\xa4\xcf\xb4\xc1\xbb\xfa\xa4\xc7\xa4\xb9\xa1\xa3";
        assert_eq!(
            to_utf8("EUC-JP", euc_jp).expect("decode"),
            "これは漢字です。"
        );
        assert_eq!(
            utf8_to("EUC-JP", "これは漢字です。").expect("encode"),
            euc_jp
        );
    }

    #[test]
    fn hz_alias_and_round_trip() {
        let hz = b"~{;(<dR;:x>F#,6@WCN^O`GW!#";
        assert_eq!(
            to_utf8("GB2312", hz).expect("decode"),
            "花间一壶酒，独酌无相亲。"
        );
        assert_eq!(
            to_utf8("HZGB2312", hz).expect("decode"),
            "花间一壶酒，独酌无相亲。"
        );
        assert_eq!(
            utf8_to("GB2312", "花间一壶酒，独酌无相亲。").expect("encode"),
            hz
        );
        assert_eq!(
            utf8_to("gbk", "Hello~ world").expect("encode"),
            utf8_to("gbk", "Hello~ world").expect("encode")
        );
    }

    #[test]
    fn errors_and_identity() {
        let big5: &[u8] = &[0x48, 0x65, 0x6c, 0x6c, 0x6f];
        assert_eq!(convert_bytes("big5", "big5", big5).expect("identity"), big5);
        assert!(convert_bytes("gbk", "no this charset", big5).is_err());
        assert!(convert_bytes("no this charset", "big5", big5).is_err());
        assert_eq!(
            convert_bytes("gbk", "no this charset", big5)
                .expect_err("unsupported")
                .to_string(),
            "unsupported srcCharset \"no this charset\""
        );
        assert_eq!(
            convert_string("UTF-8", "big5", "Hello").expect("text"),
            "Hello"
        );
    }
}
