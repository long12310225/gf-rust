//! Port of `util/grand`.
//!
//! The Go package fills a buffered channel from `crypto/rand` in a background
//! goroutine. The port keeps a per-thread 1 KiB buffer filled from the system
//! CSPRNG, which avoids both the channel and a shared lock on every draw.

use std::cell::RefCell;
use std::time::Duration;

use ring::rand::{SecureRandom, SystemRandom};

const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";
const SYMBOLS: &[u8] = b"!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";
const DIGITS: &[u8] = b"0123456789";
const CHARACTERS: &[u8] =
    b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~";

struct Buffer {
    bytes: [u8; 1024],
    position: usize,
}

impl Buffer {
    fn new() -> Buffer {
        Buffer {
            bytes: [0; 1024],
            position: 1024,
        }
    }

    fn next_u32(&mut self) -> u32 {
        if self.position + 4 > self.bytes.len() {
            let random = SystemRandom::new();
            if random.fill(&mut self.bytes).is_err() {
                panic!("error reading random buffer from system");
            }
            self.position = 0;
        }
        let start = self.position;
        self.position += 4;
        u32::from_le_bytes([
            self.bytes[start],
            self.bytes[start + 1],
            self.bytes[start + 2],
            self.bytes[start + 3],
        ])
    }

    fn next_u64(&mut self) -> u64 {
        let low = u64::from(self.next_u32());
        let high = u64::from(self.next_u32());
        (high << 32) | low
    }
}

thread_local! {
    static BUFFER: RefCell<Buffer> = RefCell::new(Buffer::new());
}

fn next_u32() -> u32 {
    BUFFER.with(|buffer| buffer.borrow_mut().next_u32())
}

fn next_u64() -> u64 {
    BUFFER.with(|buffer| buffer.borrow_mut().next_u64())
}

/// Returns a number in `[0, max)`; a non-positive `max` is returned unchanged.
pub fn intn(max: i64) -> i64 {
    if max <= 0 {
        return max;
    }
    i64::from(next_u32()) % max
}

/// Returns `n` random bytes.
pub fn b(n: usize) -> Vec<u8> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = vec![0u8; n];
    let mut i = 0;
    while i < n {
        let value = next_u32().to_le_bytes();
        let take = (n - i).min(4);
        out[i..i + take].copy_from_slice(&value[..take]);
        i += 4;
    }
    out
}

/// Returns a number in `[min, max]`; negative bounds are supported.
pub fn n(min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    if min >= 0 {
        return intn(max - min + 1) + min;
    }
    intn(max + (0 - min) + 1) - (0 - min)
}

/// Returns a random string of digits and letters; `symbols` allows symbols.
pub fn s(n: usize, symbols: bool) -> String {
    if n == 0 {
        return String::new();
    }
    let numbers = b(n);
    let table = if symbols {
        CHARACTERS
    } else {
        &CHARACTERS[..62]
    };
    let mut out = String::with_capacity(n);
    for &number in &numbers {
        out.push(char::from(table[usize::from(number) % table.len()]));
    }
    out
}

/// Returns `n` random bytes that contain only digits.
pub fn digits(n: usize) -> String {
    string_from_table(n, DIGITS)
}

/// Returns `n` random bytes that contain only letters.
pub fn letters(n: usize) -> String {
    string_from_table(n, LETTERS)
}

/// Returns `n` random bytes that contain only symbols.
pub fn symbols(n: usize) -> String {
    string_from_table(n, SYMBOLS)
}

fn string_from_table(n: usize, table: &[u8]) -> String {
    if n == 0 {
        return String::new();
    }
    let numbers = b(n);
    let mut out = String::with_capacity(n);
    for &number in &numbers {
        out.push(char::from(table[usize::from(number) % table.len()]));
    }
    out
}

/// Returns a random duration in `[min, max]`, preserving shared factors of ten.
pub fn d(min: Duration, max: Duration) -> Duration {
    if min >= max {
        return min;
    }
    let mut multiple: i64 = 1;
    let mut low = i64::try_from(min.as_nanos()).unwrap_or(i64::MAX);
    let mut high = i64::try_from(max.as_nanos()).unwrap_or(i64::MAX);
    if low != 0 {
        while low % 10 == 0 && high % 10 == 0 {
            multiple *= 10;
            low /= 10;
            high /= 10;
        }
    }
    let value = uniform_int64(low, high).saturating_mul(multiple);
    Duration::from_nanos(u64::try_from(value).unwrap_or(0))
}

fn uniform_int64(min: i64, max: i64) -> i64 {
    if min >= max {
        return min;
    }
    let span = (max as u64).wrapping_sub(min as u64).wrapping_add(1);
    if span == 0 {
        return next_u64() as i64;
    }
    if span <= u64::from(i32::MAX as u32) {
        return min + intn(span as i64);
    }
    let limit = u64::MAX - (u64::MAX % span);
    loop {
        let value = next_u64();
        if value < limit {
            return (min as u64).wrapping_add(value % span) as i64;
        }
    }
}

/// Picks `n` random runes from `s`.
pub fn str(s: &str, n: usize) -> String {
    if n == 0 {
        return String::new();
    }
    let runes: Vec<char> = s.chars().collect();
    if runes.is_empty() {
        return String::new();
    }
    if runes.len() <= 255 {
        let numbers = b(n);
        let mut out = String::with_capacity(n);
        for &number in &numbers {
            out.push(runes[usize::from(number) % runes.len()]);
        }
        out
    } else {
        let mut out = String::with_capacity(n);
        for _ in 0..n {
            out.push(runes[intn(runes.len() as i64) as usize]);
        }
        out
    }
}

/// Returns a pseudo-random permutation of `0..n`.
pub fn perm(n: usize) -> Vec<i64> {
    let mut values = vec![0i64; n];
    for i in 0..n {
        let j = intn(i as i64 + 1) as usize;
        values[i] = values[j];
        values[j] = i as i64;
    }
    values
}

/// Reports whether `num / total` probability is met.
pub fn meet(num: i64, total: i64) -> bool {
    intn(total) < num
}

/// Reports whether `prob` probability is met.
pub fn meet_prob(prob: f32) -> bool {
    intn(10_000_000) < (prob * 10_000_000.0) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intn_bounds() {
        for _ in 0..1000 {
            let value = intn(10);
            assert!((0..10).contains(&value), "{value}");
        }
        assert_eq!(intn(0), 0);
        assert_eq!(intn(-5), -5);
    }

    #[test]
    fn n_supports_negative_ranges() {
        for _ in 0..1000 {
            let value = n(-10, 10);
            assert!((-10..=10).contains(&value), "{value}");
            let value = n(5, 5);
            assert_eq!(value, 5);
        }
    }

    #[test]
    fn byte_and_string_lengths_and_charsets() {
        assert_eq!(b(0), Vec::<u8>::new());
        assert_eq!(b(13).len(), 13);
        assert_eq!(s(0, false), "");
        assert_eq!(s(32, false).len(), 32);
        assert!(
            s(32, false)
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric())
        );
        assert_eq!(digits(16).len(), 16);
        assert!(digits(16).bytes().all(|byte| byte.is_ascii_digit()));
        assert_eq!(letters(16).len(), 16);
        assert!(letters(16).bytes().all(|byte| byte.is_ascii_alphabetic()));
        assert_eq!(symbols(16).len(), 16);
        assert!(
            symbols(16)
                .bytes()
                .all(|byte| !byte.is_ascii_alphanumeric())
        );
    }

    #[test]
    fn str_handles_unicode() {
        let value = str("你好世界", 8);
        assert_eq!(value.chars().count(), 8);
        assert!(value.chars().all(|ch| "你好世界".contains(ch)));
    }

    #[test]
    fn perm_is_a_permutation() {
        let mut values = perm(16);
        values.sort_unstable();
        assert_eq!(values, (0..16).collect::<Vec<i64>>());
        assert!(perm(0).is_empty());
    }

    #[test]
    fn durations_stay_in_range_and_keep_multiples() {
        for _ in 0..100 {
            let value = d(Duration::from_secs(1), Duration::from_secs(3));
            assert!(value >= Duration::from_secs(1) && value <= Duration::from_secs(3));
            assert_eq!(value.as_nanos() % 1_000_000_000, 0);
        }
        let fixed = d(Duration::from_secs(2), Duration::from_secs(1));
        assert_eq!(fixed, Duration::from_secs(2));
    }

    #[test]
    fn meet_probability_extremes() {
        assert!(!meet(0, 10));
        assert!(meet(10, 10));
        assert!(!meet_prob(0.0));
        assert!(meet_prob(1.0));
    }
}
