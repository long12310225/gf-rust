//! Port of `gfile_cache.go`.
//!
//! Go caches file contents through `os/gcache` with a TTL and evicts entries
//! with `os/gfsnotify` when the file changes. `gcache` is not ported yet, so
//! the port keeps the small TTL map this lane actually needs (ADR-043) and
//! uses `gfsnotify` for eviction, exactly like Go: one watch per cache key,
//! removed after the first change.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::gfsnotify;

/// The environment or command key configuring the default cache duration, like
/// Go's `commandEnvKeyForCache`.
pub const CACHE_ENV_KEY: &str = "gf.gfile.cache";

/// The default cache duration, like Go's `defaultCacheDuration`.
pub const DEFAULT_CACHE_DURATION: Duration = Duration::from_secs(60);

/// How long a cached content stays valid, like Go's optional `duration`
/// argument. Go uses a signed `time.Duration`; Rust durations cannot be
/// negative, so the three cases are named.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CacheDuration {
    /// Uses the `gf.gfile.cache` setting (one minute by default).
    #[default]
    Default,
    /// Reads through without caching (Go: a negative duration).
    None,
    /// Caches without expiration (Go: a zero duration).
    Forever,
    /// Caches for the given time (Go: a positive duration).
    After(Duration),
}

impl From<Duration> for CacheDuration {
    fn from(duration: Duration) -> CacheDuration {
        if duration.is_zero() {
            CacheDuration::Forever
        } else {
            CacheDuration::After(duration)
        }
    }
}

/// One cached file content.
struct CacheEntry {
    content: Vec<u8>,
    inserted_at: Instant,
    expire: Option<Duration>,
}

impl CacheEntry {
    fn is_expired(&self, now: Instant) -> bool {
        match self.expire {
            Some(expire) => now.duration_since(self.inserted_at) >= expire,
            None => false,
        }
    }
}

/// The process-wide content cache.
fn cache() -> &'static Mutex<HashMap<String, CacheEntry>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CacheEntry>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Removes one cache entry, used by the watcher callback.
fn remove_entry(key: &str) {
    cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(key);
}

/// Returns the cached content of `path`, reading it on a miss, like Go's
/// `GetContentsWithCache`.
pub fn get_contents_with_cache(path: &str, duration: CacheDuration) -> String {
    String::from_utf8_lossy(&get_bytes_with_cache(path, duration)).into_owned()
}

/// Returns the cached content of `path`, like Go's `GetBytesWithCache`.
pub fn get_bytes_with_cache(path: &str, duration: CacheDuration) -> Vec<u8> {
    let expire = match duration {
        CacheDuration::Default => default_cache_duration(),
        CacheDuration::None => return super::get_bytes(path),
        CacheDuration::Forever => Duration::ZERO,
        CacheDuration::After(duration) => duration,
    };
    let key = format!("{CACHE_ENV_KEY}{path}");
    {
        let mut entries = cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(entry) = entries
            .get(&key)
            .filter(|entry| !entry.is_expired(Instant::now()))
        {
            return entry.content.clone();
        }
        entries.remove(&key);
    }
    let content = super::get_bytes(path);
    if content.is_empty() && !super::exists(path) {
        return content;
    }
    {
        let mut entries = cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        entries.insert(
            key.clone(),
            CacheEntry {
                content: content.clone(),
                inserted_at: Instant::now(),
                expire: if expire.is_zero() { None } else { Some(expire) },
            },
        );
    }
    // Evict the entry when the file changes. `add_once` keeps one watch per
    // cache key even when the content is requested repeatedly.
    let watch_key = key.clone();
    let _ = gfsnotify::add_once(
        &key,
        path,
        Arc::new(move |_event: &gfsnotify::Event| {
            remove_entry(&watch_key);
            gfsnotify::exit();
        }),
        gfsnotify::WatchOption::default(),
    );
    content
}

/// The configured cache duration, like Go's `getCacheDuration`.
///
/// Go panics on a malformed value; the port falls back to the default and
/// leaves the invalid text unused.
pub fn default_cache_duration() -> Duration {
    match std::env::var(CACHE_ENV_KEY)
        .ok()
        .filter(|value| !value.is_empty())
    {
        Some(value) => parse_duration(&value).unwrap_or(DEFAULT_CACHE_DURATION),
        None => DEFAULT_CACHE_DURATION,
    }
}

/// Parses Go's duration text (`300ms`, `1m30s`, ...).
fn parse_duration(text: &str) -> Option<Duration> {
    let mut total = Duration::ZERO;
    let mut rest = text.trim();
    if rest.is_empty() {
        return None;
    }
    while !rest.is_empty() {
        let number_end = rest
            .find(|character: char| !character.is_ascii_digit() && character != '.')
            .unwrap_or(rest.len());
        if number_end == 0 {
            return None;
        }
        let number: f64 = rest[..number_end].parse().ok()?;
        rest = &rest[number_end..];
        let unit_end = rest
            .find(|character: char| character.is_ascii_digit() || character == '.')
            .unwrap_or(rest.len());
        let unit = &rest[..unit_end];
        rest = &rest[unit_end..];
        let seconds = match unit {
            "ns" => number / 1_000_000_000.0,
            "us" | "µs" => number / 1_000_000.0,
            "ms" => number / 1_000.0,
            "s" => number,
            "m" => number * 60.0,
            "h" => number * 3600.0,
            _ => return None,
        };
        total += Duration::from_secs_f64(seconds);
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gfile::{join, mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfile-cache-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    /// Waits until `condition` holds, for at most five seconds.
    fn eventually(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    #[test]
    fn contents_are_cached_and_evicted_on_change() {
        let base = sandbox("cached");
        let file = join(&[&base, "config.toml"]);
        put_contents(&file, "first").expect("write");

        assert_eq!(
            get_contents_with_cache(&file, CacheDuration::Default),
            "first"
        );
        // The watch evicts the cached entry, so a later read sees the new
        // content (the watcher runs asynchronously).
        put_contents(&file, "second").expect("rewrite");
        assert!(
            eventually(|| get_contents_with_cache(&file, CacheDuration::Default) == "second"),
            "the watch should have dropped the cached content"
        );
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn no_cache_duration_reads_through() {
        let base = sandbox("negative");
        let file = join(&[&base, "data.txt"]);
        put_contents(&file, "one").expect("write");
        assert_eq!(get_contents_with_cache(&file, CacheDuration::None), "one");
        put_contents(&file, "two").expect("rewrite");
        assert_eq!(get_contents_with_cache(&file, CacheDuration::None), "two");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn duration_text_parses_like_go() {
        assert_eq!(parse_duration("1m"), Some(Duration::from_secs(60)));
        assert_eq!(parse_duration("1m30s"), Some(Duration::from_secs(90)));
        assert_eq!(parse_duration("500ms"), Some(Duration::from_millis(500)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("abc"), None);
    }

    #[test]
    fn missing_files_are_not_cached() {
        let base = sandbox("missing");
        let file = join(&[&base, "absent.txt"]);
        assert!(get_contents_with_cache(&file, CacheDuration::Default).is_empty());
        put_contents(&file, "now here").expect("write");
        assert_eq!(
            get_contents_with_cache(&file, CacheDuration::Default),
            "now here"
        );
        remove_all(&base).expect("cleanup");
    }
}
