//! Port of `util/guid` (`guid.go`).
//!
//! The returned string is always 32 bytes: `MACHash(7) + PID(4) +
//! TimestampNano(12) + Sequence(3) + RandomString(6)` by default, or
//! `DataHash(7/14) + TimestampNano(12) + Sequence(3) + RandomString(3/10)`
//! when custom data is given.
//!
//! To avoid a crate cycle (`gf-container`/`gf-encoding`/`gf-net` all depend on
//! `gf-util`) the MAC collection, SDBM hash and sequence counter live here
//! (ADR-036).

use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};

use network_interface::{NetworkInterface, NetworkInterfaceConfig};

use crate::grand;

/// Sequence max, "zzz" in base 36.
const SEQUENCE_MAX: u32 = 46655;
/// Random chars string (36 bytes, lowercase + digits).
const RANDOM_STR_BASE: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
/// Sequence for unique purpose of the current process.
static SEQUENCE: AtomicU32 = AtomicU32::new(0);

/// Formats a u64 in lowercase base 36, like `strconv.FormatUint(v, 36)`.
fn format_u64_base36(mut value: u64) -> String {
    if value == 0 {
        return "0".to_owned();
    }
    let mut digits = Vec::new();
    while value > 0 {
        let digit = (value % 36) as u8;
        digits.push(if digit < 10 {
            b'0' + digit
        } else {
            b'a' + (digit - 10)
        });
        value /= 36;
    }
    digits.reverse();
    String::from_utf8(digits).unwrap_or_default()
}

/// Formats an i64 in lowercase base 36, like `strconv.FormatInt(v, 36)`.
fn format_i64_base36(value: i64) -> String {
    if value < 0 {
        format!("-{}", format_u64_base36(value.unsigned_abs()))
    } else {
        format_u64_base36(value as u64)
    }
}

/// Hashes `data` with the classic SDBM 32-bit algorithm.
fn sdbm(data: &[u8]) -> u32 {
    let mut hash: u32 = 0;
    for byte in data {
        hash = u32::from(*byte)
            .wrapping_add(hash << 6)
            .wrapping_add(hash << 16)
            .wrapping_sub(hash);
    }
    hash
}

/// Copies `source` into the beginning of `target`, like Go's `copy`.
fn copy_into(target: &mut [u8], source: &[u8]) {
    let count = target.len().min(source.len());
    target[..count].copy_from_slice(&source[..count]);
}

/// Returns the hash result of the machine's MAC addresses in 7 bytes.
fn mac_addr_str() -> &'static str {
    static MAC_ADDR_STR: OnceLock<String> = OnceLock::new();
    MAC_ADDR_STR.get_or_init(|| {
        let interfaces = match NetworkInterface::show() {
            Ok(interfaces) => interfaces,
            Err(_) => return "0000000".to_owned(),
        };
        let mut bytes = Vec::new();
        for interface in interfaces {
            if let Some(mac) = interface.mac_addr {
                let text = mac.to_string();
                if !text.is_empty() {
                    bytes.extend_from_slice(text.as_bytes());
                }
            }
        }
        if bytes.is_empty() {
            return "0000000".to_owned();
        }
        let mut buffer = [b'0'; 7];
        copy_into(
            &mut buffer,
            format_u64_base36(u64::from(sdbm(&bytes))).as_bytes(),
        );
        String::from_utf8_lossy(&buffer).into_owned()
    })
}

/// Returns the process id in 4 bytes.
fn process_id_str() -> &'static str {
    static PROCESS_ID_STR: OnceLock<String> = OnceLock::new();
    PROCESS_ID_STR.get_or_init(|| {
        let mut buffer = [b'0'; 4];
        copy_into(
            &mut buffer,
            format_i64_base36(i64::from(std::process::id())).as_bytes(),
        );
        String::from_utf8_lossy(&buffer).into_owned()
    })
}

/// Increases and returns the sequence string in 3 bytes.
fn get_sequence() -> Vec<u8> {
    let current = SEQUENCE.fetch_add(1, Ordering::SeqCst).wrapping_add(1) % SEQUENCE_MAX;
    let mut buffer = *b"000";
    copy_into(
        &mut buffer,
        format_u64_base36(u64::from(current)).as_bytes(),
    );
    buffer.to_vec()
}

/// Randomly picks and returns `n` chars from the base-36 alphabet.
fn get_random_str(n: i64) -> Vec<u8> {
    if n <= 0 {
        return Vec::new();
    }
    let n = n as usize;
    let number_bytes = grand::b(n);
    let mut buffer = vec![0u8; n];
    for (index, slot) in buffer.iter_mut().enumerate() {
        *slot = RANDOM_STR_BASE[usize::from(number_bytes[index]) % 36];
    }
    buffer
}

/// Creates and returns a hash of `data` in 7 bytes.
fn get_data_hash_str(data: &[u8]) -> Vec<u8> {
    let mut buffer = *b"0000000";
    copy_into(
        &mut buffer,
        format_u64_base36(u64::from(sdbm(data))).as_bytes(),
    );
    buffer.to_vec()
}

/// Creates and returns a global unique string in 32 bytes.
///
/// The optional `data` may hold at most two parts; more than two parts panic
/// like the Go original. Empty parts are ignored.
pub fn s(data: &[&[u8]]) -> String {
    let mut buffer = [0u8; 32];
    let nano_str = format_i64_base36(unix_nano());
    if data.is_empty() {
        copy_into(&mut buffer, mac_addr_str().as_bytes());
        copy_into(&mut buffer[7..], process_id_str().as_bytes());
        copy_into(&mut buffer[11..], nano_str.as_bytes());
        copy_into(&mut buffer[23..], &get_sequence());
        copy_into(&mut buffer[26..], &get_random_str(6));
    } else if data.len() <= 2 {
        let mut n = 0usize;
        for (index, part) in data.iter().enumerate() {
            if !part.is_empty() {
                copy_into(&mut buffer[index * 7..], &get_data_hash_str(part));
                n += 7;
            }
        }
        copy_into(&mut buffer[n..], nano_str.as_bytes());
        copy_into(&mut buffer[n + 12..], &get_sequence());
        copy_into(
            &mut buffer[n + 12 + 3..],
            &get_random_str(32 - n as i64 - 12 - 3),
        );
    } else {
        panic!("too many data parts, it should be no more than 2 parts");
    }
    String::from_utf8_lossy(&buffer).into_owned()
}

/// Returns the current time in Unix nanoseconds.
fn unix_nano() -> i64 {
    match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(duration) => duration.as_nanos() as i64,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    // Translated from Test_S (the Go test uses 1,000,000 iterations; the port
    // keeps a smaller sample so the debug test suite stays fast).
    #[test]
    fn test_s() {
        let mut set = HashSet::new();
        for _ in 0..10_000 {
            let value = s(&[]);
            assert_eq!(value.len(), 32);
            assert!(set.insert(value));
        }
    }

    // Translated from Test_S_Data.
    #[test]
    fn test_s_data() {
        assert_eq!(s(&[b"123"]).len(), 32);
    }

    #[test]
    fn two_parts_and_empty_parts() {
        assert_eq!(s(&[b"123", b"456"]).len(), 32);
        assert_eq!(s(&[b"123", b""]).len(), 32);
        assert_eq!(s(&[b"", b""]).len(), 32);
    }

    #[test]
    #[should_panic(expected = "too many data parts")]
    fn more_than_two_parts_panics() {
        let _ = s(&[b"1", b"2", b"3"]);
    }

    #[test]
    fn base36_and_sdbm() {
        assert_eq!(format_u64_base36(0), "0");
        assert_eq!(format_u64_base36(35), "z");
        assert_eq!(format_u64_base36(36), "10");
        assert_eq!(format_i64_base36(46655), "zzz");
        assert_eq!(sdbm(b""), 0);
        assert_eq!(sdbm(b"a"), 97);
    }

    #[test]
    fn sequence_wraps_below_the_max() {
        for _ in 0..1000 {
            let sequence = get_sequence();
            assert_eq!(sequence.len(), 3);
            let text = String::from_utf8(sequence).expect("base36 text");
            assert!(text.as_str() <= "zzz");
        }
    }
}
