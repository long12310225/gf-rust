//! Port of `encoding/gbinary`.
//!
//! `encode`/`decode` use LittleEndian by default, like the Go package. Integer
//! encoders pick the narrowest width that fits, and typed decoders read a
//! width based on the slice length (or right-pad/truncate), as in Go.

use gf_core::Value;
use gf_errors::gerror;

/// A single bit, 0 or 1.
pub type Bit = u8;

fn read_u16_le(b: &[u8]) -> u16 {
    let mut bytes = [0u8; 2];
    let copy = b.len().min(2);
    bytes[..copy].copy_from_slice(&b[..copy]);
    u16::from_le_bytes(bytes)
}

fn read_u32_le(b: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    let copy = b.len().min(4);
    bytes[..copy].copy_from_slice(&b[..copy]);
    u32::from_le_bytes(bytes)
}

fn read_u64_le(b: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    let copy = b.len().min(8);
    bytes[..copy].copy_from_slice(&b[..copy]);
    u64::from_le_bytes(bytes)
}

fn read_u16_be(b: &[u8]) -> u16 {
    let mut bytes = [0u8; 2];
    let padded = be_fill_up_size(b, 2);
    bytes.copy_from_slice(&padded);
    u16::from_be_bytes(bytes)
}

fn read_u32_be(b: &[u8]) -> u32 {
    let mut bytes = [0u8; 4];
    let padded = be_fill_up_size(b, 4);
    bytes.copy_from_slice(&padded);
    u32::from_be_bytes(bytes)
}

fn read_u64_be(b: &[u8]) -> u64 {
    let mut bytes = [0u8; 8];
    let padded = be_fill_up_size(b, 8);
    bytes.copy_from_slice(&padded);
    u64::from_be_bytes(bytes)
}

fn fill_up_size(b: &[u8], length: usize, right_align: bool) -> Vec<u8> {
    if b.len() >= length {
        return b[..length].to_vec();
    }
    let mut out = vec![0u8; length];
    if right_align {
        out[length - b.len()..].copy_from_slice(b);
    } else {
        out[..b.len()].copy_from_slice(b);
    }
    out
}

/// Left-pads/truncates `b` to `length` bytes for big-endian reads.
pub fn be_fill_up_size(b: &[u8], length: usize) -> Vec<u8> {
    fill_up_size(b, length, true)
}

/// Right-pads/truncates `b` to `length` bytes for little-endian reads.
pub fn le_fill_up_size(b: &[u8], length: usize) -> Vec<u8> {
    fill_up_size(b, length, false)
}

/// Encodes an `int` in the narrowest width that fits, like Go.
pub fn le_encode_int(value: i64) -> Vec<u8> {
    if value <= i64::from(i8::MAX) {
        vec![value as i8 as u8]
    } else if value <= i64::from(i16::MAX) {
        (value as i16).to_le_bytes().to_vec()
    } else if value <= i64::from(i32::MAX) {
        (value as i32).to_le_bytes().to_vec()
    } else {
        value.to_le_bytes().to_vec()
    }
}

/// Encodes a `uint` in the narrowest width that fits.
pub fn le_encode_uint(value: u64) -> Vec<u8> {
    if value <= u64::from(u8::MAX) {
        vec![value as u8]
    } else if value <= u64::from(u16::MAX) {
        (value as u16).to_le_bytes().to_vec()
    } else if value <= u64::from(u32::MAX) {
        (value as u32).to_le_bytes().to_vec()
    } else {
        value.to_le_bytes().to_vec()
    }
}

/// Encodes a bool as one byte.
pub fn le_encode_bool(value: bool) -> Vec<u8> {
    vec![u8::from(value)]
}

/// Encodes a string as its UTF-8 bytes.
pub fn le_encode_string(value: &str) -> Vec<u8> {
    value.as_bytes().to_vec()
}

/// Encodes i8.
pub fn le_encode_int8(value: i8) -> Vec<u8> {
    vec![value as u8]
}

/// Encodes u8.
pub fn le_encode_uint8(value: u8) -> Vec<u8> {
    vec![value]
}

/// Encodes i16.
pub fn le_encode_int16(value: i16) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes u16.
pub fn le_encode_uint16(value: u16) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes i32.
pub fn le_encode_int32(value: i32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes u32.
pub fn le_encode_uint32(value: u32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes i64.
pub fn le_encode_int64(value: i64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes u64.
pub fn le_encode_uint64(value: u64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes f32.
pub fn le_encode_float32(value: f32) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Encodes f64.
pub fn le_encode_float64(value: f64) -> Vec<u8> {
    value.to_le_bytes().to_vec()
}

/// Decodes an int, choosing the width from the slice length.
pub fn le_decode_to_int(b: &[u8]) -> i64 {
    if b.len() < 2 {
        i64::from(le_decode_to_uint8(b))
    } else if b.len() < 3 {
        i64::from(read_u16_le(b))
    } else if b.len() < 5 {
        i64::from(read_u32_le(b))
    } else {
        read_u64_le(b) as i64
    }
}

/// Decodes a uint, choosing the width from the slice length.
pub fn le_decode_to_uint(b: &[u8]) -> u64 {
    if b.len() < 2 {
        u64::from(le_decode_to_uint8(b))
    } else if b.len() < 3 {
        u64::from(read_u16_le(b))
    } else if b.len() < 5 {
        u64::from(read_u32_le(b))
    } else {
        read_u64_le(b)
    }
}

/// Decodes a bool: any non-zero byte is true.
pub fn le_decode_to_bool(b: &[u8]) -> bool {
    b.iter().any(|&byte| byte != 0)
}

/// Decodes i8; panics on an empty slice like Go.
pub fn le_decode_to_int8(b: &[u8]) -> i8 {
    b[0] as i8
}

/// Decodes u8; panics on an empty slice like Go.
pub fn le_decode_to_uint8(b: &[u8]) -> u8 {
    b[0]
}

/// Decodes i16.
pub fn le_decode_to_int16(b: &[u8]) -> i16 {
    read_u16_le(b) as i16
}

/// Decodes u16.
pub fn le_decode_to_uint16(b: &[u8]) -> u16 {
    read_u16_le(b)
}

/// Decodes i32.
pub fn le_decode_to_int32(b: &[u8]) -> i32 {
    read_u32_le(b) as i32
}

/// Decodes u32.
pub fn le_decode_to_uint32(b: &[u8]) -> u32 {
    read_u32_le(b)
}

/// Decodes i64.
pub fn le_decode_to_int64(b: &[u8]) -> i64 {
    read_u64_le(b) as i64
}

/// Decodes u64.
pub fn le_decode_to_uint64(b: &[u8]) -> u64 {
    read_u64_le(b)
}

/// Decodes f32.
pub fn le_decode_to_float32(b: &[u8]) -> f32 {
    f32::from_bits(read_u32_le(b))
}

/// Decodes f64.
pub fn le_decode_to_float64(b: &[u8]) -> f64 {
    f64::from_bits(read_u64_le(b))
}

/// Encodes an `int` in the narrowest width that fits, big-endian.
pub fn be_encode_int(value: i64) -> Vec<u8> {
    be_int_bytes(value)
}

/// Encodes a `uint` in the narrowest width that fits, big-endian.
pub fn be_encode_uint(value: u64) -> Vec<u8> {
    be_uint_bytes(value)
}

/// Encodes a bool as one byte.
pub fn be_encode_bool(value: bool) -> Vec<u8> {
    vec![u8::from(value)]
}

/// Encodes a string as its UTF-8 bytes.
pub fn be_encode_string(value: &str) -> Vec<u8> {
    value.as_bytes().to_vec()
}

/// Encodes i8.
pub fn be_encode_int8(value: i8) -> Vec<u8> {
    vec![value as u8]
}

/// Encodes u8.
pub fn be_encode_uint8(value: u8) -> Vec<u8> {
    vec![value]
}

/// Encodes i16 big-endian.
pub fn be_encode_int16(value: i16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes u16 big-endian.
pub fn be_encode_uint16(value: u16) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes i32 big-endian.
pub fn be_encode_int32(value: i32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes u32 big-endian.
pub fn be_encode_uint32(value: u32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes i64 big-endian.
pub fn be_encode_int64(value: i64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes u64 big-endian.
pub fn be_encode_uint64(value: u64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes f32 big-endian.
pub fn be_encode_float32(value: f32) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Encodes f64 big-endian.
pub fn be_encode_float64(value: f64) -> Vec<u8> {
    value.to_be_bytes().to_vec()
}

/// Decodes an int, choosing the width from the slice length, big-endian.
pub fn be_decode_to_int(b: &[u8]) -> i64 {
    if b.len() < 2 {
        i64::from(b[0])
    } else if b.len() < 3 {
        i64::from(read_u16_be(b))
    } else if b.len() < 5 {
        i64::from(read_u32_be(b))
    } else {
        read_u64_be(b) as i64
    }
}

/// Decodes a uint, choosing the width from the slice length, big-endian.
pub fn be_decode_to_uint(b: &[u8]) -> u64 {
    if b.len() < 2 {
        u64::from(b[0])
    } else if b.len() < 3 {
        u64::from(read_u16_be(b))
    } else if b.len() < 5 {
        u64::from(read_u32_be(b))
    } else {
        read_u64_be(b)
    }
}

/// Decodes a bool: any non-zero byte is true.
pub fn be_decode_to_bool(b: &[u8]) -> bool {
    b.iter().any(|&byte| byte != 0)
}

/// Decodes i8; panics on an empty slice like Go.
pub fn be_decode_to_int8(b: &[u8]) -> i8 {
    b[0] as i8
}

/// Decodes u8; panics on an empty slice like Go.
pub fn be_decode_to_uint8(b: &[u8]) -> u8 {
    b[0]
}

/// Decodes i16 big-endian.
pub fn be_decode_to_int16(b: &[u8]) -> i16 {
    read_u16_be(b) as i16
}

/// Decodes u16 big-endian.
pub fn be_decode_to_uint16(b: &[u8]) -> u16 {
    read_u16_be(b)
}

/// Decodes i32 big-endian.
pub fn be_decode_to_int32(b: &[u8]) -> i32 {
    read_u32_be(b) as i32
}

/// Decodes u32 big-endian.
pub fn be_decode_to_uint32(b: &[u8]) -> u32 {
    read_u32_be(b)
}

/// Decodes i64 big-endian.
pub fn be_decode_to_int64(b: &[u8]) -> i64 {
    read_u64_be(b) as i64
}

/// Decodes u64 big-endian.
pub fn be_decode_to_uint64(b: &[u8]) -> u64 {
    read_u64_be(b)
}

/// Decodes f32 big-endian.
pub fn be_decode_to_float32(b: &[u8]) -> f32 {
    f32::from_bits(read_u32_be(b))
}

/// Decodes f64 big-endian.
pub fn be_decode_to_float64(b: &[u8]) -> f64 {
    f64::from_bits(read_u64_be(b))
}

/// Encodes a value with LittleEndian.
pub fn encode(values: &[Value]) -> Vec<u8> {
    encode_with(values, false)
}

/// Encodes a value with BigEndian.
pub fn be_encode(values: &[Value]) -> Vec<u8> {
    encode_with(values, true)
}

fn encode_with(values: &[Value], big_endian: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(values.len() * 8);
    append_values(&mut out, values, big_endian);
    out
}

fn append_values(out: &mut Vec<u8>, values: &[Value], big_endian: bool) {
    for value in values {
        match value {
            Value::Null => break,
            Value::Bool(v) => out.push(u8::from(*v)),
            Value::Int(v) => push_int(out, *v, big_endian),
            Value::UInt(v) => push_uint(out, *v, big_endian),
            Value::Float(v) => {
                if big_endian {
                    out.extend_from_slice(&v.to_be_bytes());
                } else {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
            Value::String(v) => out.extend_from_slice(v.as_bytes()),
            Value::Bytes(v) => out.extend_from_slice(v),
            Value::List(items) => append_values(out, items, big_endian),
            Value::Map(_) => out.extend_from_slice(value.to_string().as_bytes()),
        }
    }
}

fn push_int(out: &mut Vec<u8>, value: i64, big_endian: bool) {
    let width = if value <= i64::from(i8::MAX) {
        1
    } else if value <= i64::from(i16::MAX) {
        2
    } else if value <= i64::from(i32::MAX) {
        4
    } else {
        8
    };
    if big_endian {
        let full = value.to_be_bytes();
        out.extend_from_slice(&full[8 - width..]);
    } else {
        let full = value.to_le_bytes();
        out.extend_from_slice(&full[..width]);
    }
}

fn push_uint(out: &mut Vec<u8>, value: u64, big_endian: bool) {
    let width = if value <= u64::from(u8::MAX) {
        1
    } else if value <= u64::from(u16::MAX) {
        2
    } else if value <= u64::from(u32::MAX) {
        4
    } else {
        8
    };
    if big_endian {
        let full = value.to_be_bytes();
        out.extend_from_slice(&full[8 - width..]);
    } else {
        let full = value.to_le_bytes();
        out.extend_from_slice(&full[..width]);
    }
}

/// Encodes `value` as big-endian bytes with the narrowest width that fits.
pub fn be_int_bytes(value: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    push_int(&mut out, value, true);
    out
}

/// Encodes `value` as little-endian bytes with the narrowest width that fits.
pub fn le_int_bytes(value: i64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    push_int(&mut out, value, false);
    out
}

/// Encodes `value` as big-endian bytes with the narrowest width that fits.
pub fn be_uint_bytes(value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    push_uint(&mut out, value, true);
    out
}

/// Encodes `value` as little-endian bytes with the narrowest width that fits.
pub fn le_uint_bytes(value: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    push_uint(&mut out, value, false);
    out
}

/// Encodes `values` and pads or truncates to exactly `length` bytes.
pub fn encode_by_length(length: usize, values: &[Value]) -> Vec<u8> {
    let mut out = encode(values);
    out.resize(length, 0);
    out.truncate(length);
    out
}

/// Encodes with BigEndian and pads or truncates to `length` bytes.
pub fn be_encode_by_length(length: usize, values: &[Value]) -> Vec<u8> {
    let mut out = be_encode(values);
    out.resize(length, 0);
    out.truncate(length);
    out
}

/// A destination type for [`decode`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 8-byte signed integer.
    Int,
    /// 8-byte unsigned integer.
    UInt,
    /// 1-byte bool.
    Bool,
    /// 1-byte signed integer.
    Int8,
    /// 1-byte unsigned integer.
    Uint8,
    /// 2-byte signed integer.
    Int16,
    /// 2-byte unsigned integer.
    Uint16,
    /// 4-byte signed integer.
    Int32,
    /// 4-byte unsigned integer.
    Uint32,
    /// 8-byte signed integer.
    Int64,
    /// 8-byte unsigned integer.
    Uint64,
    /// 4-byte float.
    Float32,
    /// 8-byte float.
    Float64,
    /// The remaining bytes as a string.
    String,
}

/// Decodes `b` into values of the given kinds with LittleEndian.
pub fn decode(b: &[u8], kinds: &[Kind]) -> Result<Vec<Value>, gerror::Error> {
    decode_with(b, kinds, false)
}

/// Decodes `b` into values of the given kinds with BigEndian.
pub fn be_decode(b: &[u8], kinds: &[Kind]) -> Result<Vec<Value>, gerror::Error> {
    decode_with(b, kinds, true)
}

fn decode_with(b: &[u8], kinds: &[Kind], big_endian: bool) -> Result<Vec<Value>, gerror::Error> {
    let mut out = Vec::with_capacity(kinds.len());
    let mut offset = 0;
    for kind in kinds {
        let width = match kind {
            Kind::Bool | Kind::Int8 | Kind::Uint8 => 1,
            Kind::Int16 | Kind::Uint16 => 2,
            Kind::Int32 | Kind::Uint32 | Kind::Float32 => 4,
            Kind::Int | Kind::UInt | Kind::Int64 | Kind::Uint64 | Kind::Float64 => 8,
            Kind::String => b.len() - offset,
        };
        if offset + width > b.len() {
            return Err(short_read());
        }
        let slice = &b[offset..offset + width];
        offset += width;
        let value = match kind {
            Kind::Bool => Value::Bool(slice.iter().any(|&byte| byte != 0)),
            Kind::Int8 => Value::Int(i64::from(slice[0] as i8)),
            Kind::Uint8 => Value::UInt(u64::from(slice[0])),
            Kind::Int16 => Value::Int(i64::from(read_i16(slice, big_endian))),
            Kind::Uint16 => Value::UInt(u64::from(read_u16(slice, big_endian))),
            Kind::Int32 => Value::Int(i64::from(read_i32(slice, big_endian))),
            Kind::Uint32 => Value::UInt(u64::from(read_u32(slice, big_endian))),
            Kind::Int | Kind::Int64 => Value::Int(read_i64(slice, big_endian)),
            Kind::UInt | Kind::Uint64 => Value::UInt(read_u64(slice, big_endian)),
            Kind::Float32 => Value::Float(f64::from(f32::from_bits(read_u32(slice, big_endian)))),
            Kind::Float64 => Value::Float(f64::from_bits(read_u64(slice, big_endian))),
            Kind::String => Value::String(String::from_utf8_lossy(slice).into_owned()),
        };
        out.push(value);
    }
    Ok(out)
}

fn short_read() -> gerror::Error {
    match gerror::wrap(
        Some(gerror::Error::new("unexpected EOF")),
        "binary.Read failed",
    ) {
        Some(err) => err,
        None => gerror::Error::new("binary.Read failed"),
    }
}

fn read_u16(b: &[u8], big_endian: bool) -> u16 {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(&b[..2]);
    if big_endian {
        u16::from_be_bytes(bytes)
    } else {
        u16::from_le_bytes(bytes)
    }
}

fn read_u32(b: &[u8], big_endian: bool) -> u32 {
    let mut bytes = [0u8; 4];
    bytes.copy_from_slice(&b[..4]);
    if big_endian {
        u32::from_be_bytes(bytes)
    } else {
        u32::from_le_bytes(bytes)
    }
}

fn read_u64(b: &[u8], big_endian: bool) -> u64 {
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&b[..8]);
    if big_endian {
        u64::from_be_bytes(bytes)
    } else {
        u64::from_le_bytes(bytes)
    }
}

fn read_i16(b: &[u8], big_endian: bool) -> i16 {
    read_u16(b, big_endian) as i16
}

fn read_i32(b: &[u8], big_endian: bool) -> i32 {
    read_u32(b, big_endian) as i32
}

fn read_i64(b: &[u8], big_endian: bool) -> i64 {
    read_u64(b, big_endian) as i64
}

/// Encodes `value` into `length` bits (most significant first) appended to
/// `bits`.
pub fn encode_bits(bits: Option<&[Bit]>, value: u64, length: usize) -> Vec<Bit> {
    let mut appended = vec![0u8; length];
    for index in (0..length).rev() {
        appended[index] = (value >> (length - 1 - index)) as u8 & 1;
    }
    match bits {
        Some(bits) => {
            let mut out = Vec::with_capacity(bits.len() + length);
            out.extend_from_slice(bits);
            out.extend_from_slice(&appended);
            out
        }
        None => appended,
    }
}

/// Packs bits left to right into bytes, zero-padding the tail.
pub fn encode_bits_to_bytes(bits: &[Bit]) -> Vec<u8> {
    if bits.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(bits.len().div_ceil(8));
    for chunk in bits.chunks(8) {
        out.push((decode_bits_to_uint(chunk) << (8 - chunk.len())) as u8);
    }
    out
}

/// Decodes bits to an integer (most significant first).
pub fn decode_bits(bits: &[Bit]) -> i64 {
    decode_bits_to_uint(bits) as i64
}

/// Decodes bits to an unsigned integer (most significant first).
pub fn decode_bits_to_uint(bits: &[Bit]) -> u64 {
    let mut value = 0u64;
    for &bit in bits {
        value = (value << 1) | u64::from(bit & 1);
    }
    value
}

/// Unpacks bytes into 8 bits each.
pub fn decode_bytes_to_bits(bytes: &[u8]) -> Vec<Bit> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &byte in bytes {
        bits.extend_from_slice(&encode_bits(None, u64::from(byte), 8));
    }
    bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_widths_round_trip() {
        for value in [0i64, 1, -99, 123, 999, 22_322, i64::from(i32::MAX)] {
            let bytes = le_encode_int(value);
            let decoded = if value < 0 {
                i64::from(le_decode_to_int8(&bytes))
            } else {
                le_decode_to_int(&bytes)
            };
            assert_eq!(decoded, value, "{value}");
            let bytes = be_encode_int(value);
            let decoded = if value < 0 {
                i64::from(be_decode_to_int8(&bytes))
            } else {
                be_decode_to_int(&bytes)
            };
            assert_eq!(decoded, value, "{value}");
        }
        for value in [0u64, 1, 123, 9999, u64::from(u32::MAX)] {
            assert_eq!(le_decode_to_uint(&le_encode_uint(value)), value, "{value}");
            assert_eq!(be_decode_to_uint(&be_encode_uint(value)), value, "{value}");
        }
    }

    #[test]
    fn typed_values_round_trip() {
        assert!(le_decode_to_bool(&le_encode_bool(true)));
        assert!(!be_decode_to_bool(&[0, 0]));
        assert_eq!(le_decode_to_int8(&le_encode_int8(-99)), -99);
        assert_eq!(be_decode_to_int8(&be_encode_int8(-99)), -99);
        assert_eq!(le_decode_to_uint8(&le_encode_uint8(200)), 200);
        assert_eq!(be_decode_to_uint8(&be_encode_uint8(200)), 200);
        assert_eq!(le_decode_to_int16(&le_encode_int16(123)), 123);
        assert_eq!(be_decode_to_int16(&be_encode_int16(123)), 123);
        assert_eq!(le_decode_to_uint32(&le_encode_uint32(u32::MAX)), u32::MAX);
        assert_eq!(be_decode_to_uint32(&be_encode_uint32(u32::MAX)), u32::MAX);
        assert_eq!(le_decode_to_int64(&le_encode_int64(i64::MIN)), i64::MIN);
        assert_eq!(be_decode_to_int64(&be_encode_int64(i64::MIN)), i64::MIN);
        assert_eq!(le_decode_to_float32(&le_encode_float32(123.456)), 123.456);
        assert_eq!(be_decode_to_float64(&be_encode_float64(123.456)), 123.456);
        assert_eq!(le_decode_to_uint16(&[1]), 1);
        assert_eq!(be_decode_to_uint16(&[1]), 1);
        assert_eq!(le_decode_to_uint16(&[1, 0, 0, 9]), 1);
        assert_eq!(be_decode_to_uint16(&[0, 1, 9]), 1);
    }

    #[test]
    fn encode_values_and_by_length() {
        let bytes = encode(&[
            Value::Int(123),
            Value::String("hehe haha".into()),
            Value::Bool(true),
        ]);
        assert_eq!(&bytes[..1], &[123]);
        assert_eq!(&bytes[1..10], b"hehe haha");
        assert_eq!(bytes[10], 1);
        assert_eq!(encode_by_length(4, &[Value::Int(1)]), vec![1, 0, 0, 0]);
        assert_eq!(encode_by_length(1, &[Value::String("abc".into())]), b"a");
        assert!(encode(&[Value::Null, Value::Int(1)]).is_empty());
    }

    #[test]
    fn decode_sequences_and_short_reads() {
        let bytes = encode(&[Value::Int(1), Value::Bool(true)]);
        let decoded = decode(&bytes, &[Kind::Int8, Kind::Bool]).expect("decode");
        assert_eq!(decoded, vec![Value::Int(1), Value::Bool(true)]);

        let err = decode(&[1], &[Kind::Int64]).expect_err("short");
        assert!(err.to_string().contains("binary.Read failed"));

        let decoded = decode(b"hi", &[Kind::String]).expect("string");
        assert_eq!(decoded, vec![Value::String("hi".into())]);
    }

    #[test]
    fn bits_match_go() {
        assert_eq!(encode_bits_to_bytes(&[]), Vec::<u8>::new());
        assert_eq!(encode_bits_to_bytes(&[1, 1, 0]), vec![0xC0]);
        assert_eq!(encode_bits_to_bytes(&[1, 0, 1, 0, 1, 0, 1, 0]), vec![0xAA]);
        assert_eq!(
            encode_bits_to_bytes(&[1, 0, 1, 0, 1, 0, 1, 0, 1]),
            vec![0xAA, 0x80]
        );
        for value in [0u64, 99, 122, 129, 222, 999, 22_322] {
            let bits = encode_bits(None, value, 64);
            assert_eq!(decode_bits(&bits), value as i64);
            assert_eq!(decode_bytes_to_bits(&encode_bits_to_bytes(&bits)), bits);
        }
    }
}
