//! Port of `container/gpool`.
//!
//! Go keeps one `gtimer` singleton that walks every pool once a second; the
//! port instead keeps one process-wide sweeper thread that visits the pools
//! still alive through weak references (ADR-038). Everything a pool does is
//! lazy too: `get` drops stale values on its way to the first live one, so a
//! pool that is only read still expires correctly.
//!
//! Divergences from Go, both behaviour-preserving for callers:
//!
//! - `close` releases the pooled values immediately instead of waiting for the
//!   next timer tick (at most one second later in Go).
//! - `get` returns only the error; Go also returns the value type's zero value
//!   beside the error, which `Result` cannot carry.
//! - Go's `gtimer.Exit()` on close stops expiry for *every* pool, because all
//!   pools share one timer entry. The port unregisters only the closed pool.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Once, Weak};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// Creates a value when the pool has nothing to hand out, like Go's
/// `TPoolNewFunc`.
pub type TPoolNewFunc<T> = Box<dyn Fn() -> Result<T, Error> + Send + Sync>;
/// Releases a value the pool will never hand out again, like Go's
/// `TPoolExpireFunc`.
pub type TPoolExpireFunc<T> = Box<dyn Fn(T) + Send + Sync>;

/// How long a pooled value may stay idle, mirroring Go's three
/// `time.Duration` cases for `gpool.New`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ttl {
    /// Never expires (Go: `ttl == 0`).
    Never,
    /// Expires as soon as it is handed out (Go: `ttl < 0`).
    Immediate,
    /// Expires `ttl` after it was put back (Go: `ttl > 0`).
    After(Duration),
}

impl From<Duration> for Ttl {
    fn from(ttl: Duration) -> Ttl {
        if ttl.is_zero() {
            Ttl::Never
        } else {
            Ttl::After(ttl)
        }
    }
}

impl Ttl {
    /// The idle lifetime in milliseconds, or `-1` when the value never
    /// expires. Go computes the expiry stamp in milliseconds as well.
    fn expiry_millis(self, now: i64) -> i64 {
        match self {
            Ttl::Never => 0,
            Ttl::Immediate => now - 1,
            Ttl::After(ttl) => now.saturating_add(duration_millis(ttl)),
        }
    }
}

/// A value waiting in the pool and the timestamp it stops being reusable.
struct PoolItem<T> {
    value: T,
    expire_at: i64,
}

/// The shared half of a pool: the values, the closed flag and the callbacks.
struct Inner<T> {
    items: Mutex<VecDeque<PoolItem<T>>>,
    closed: AtomicBool,
    /// Set once `close` released the values, so the sweeper stops visiting.
    drained: AtomicBool,
    ttl: Ttl,
    new_func: Option<TPoolNewFunc<T>>,
    expire_func: Option<TPoolExpireFunc<T>>,
}

impl<T> Inner<T> {
    fn lock_items(&self) -> std::sync::MutexGuard<'_, VecDeque<PoolItem<T>>> {
        self.items
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn pop_front(&self) -> Option<PoolItem<T>> {
        self.lock_items().pop_front()
    }

    fn push_back(&self, item: PoolItem<T>) {
        self.lock_items().push_back(item);
    }

    fn push_front(&self, item: PoolItem<T>) {
        self.lock_items().push_front(item);
    }

    /// Hands `item` to the expire callback, if any, and drops it otherwise.
    fn retire(&self, item: PoolItem<T>) {
        if let Some(expire) = &self.expire_func {
            expire(item.value);
        }
    }

    /// Removes every idle value, calling the expire callback per value.
    fn release_all(&self) {
        while let Some(item) = self.pop_front() {
            self.retire(item);
        }
    }
}

/// A pool the shared sweeper can visit once per second.
trait Sweep: Send + Sync {
    fn sweep(&self);
}

impl<T: Send + Sync + 'static> Sweep for Inner<T> {
    fn sweep(&self) {
        if self.closed.load(Ordering::SeqCst) {
            // Go clears the values from the timer callback; the port already
            // did it in `close`, so only the first tick after closing drains
            // anything a caller put before the close flag was visible.
            if !self.drained.swap(true, Ordering::SeqCst) {
                self.release_all();
            }
            return;
        }
        if self.ttl == Ttl::Never {
            return;
        }
        let now = now_millis();
        while let Some(item) = self.pop_front() {
            if item.expire_at > now {
                self.push_front(item);
                break;
            }
            self.retire(item);
        }
    }
}

/// The pools registered with the sweeper thread.
static POOLS: Mutex<Vec<Weak<dyn Sweep>>> = Mutex::new(Vec::new());
/// Guards the one-time start of the sweeper thread.
static SWEEPER: Once = Once::new();

/// Registers `pool` for expiry checks and starts the sweeper when needed.
fn register(pool: Weak<dyn Sweep>) {
    SWEEPER.call_once(|| {
        let spawned = std::thread::Builder::new()
            .name("gf-pool-sweeper".to_owned())
            .spawn(|| {
                loop {
                    std::thread::sleep(Duration::from_secs(1));
                    sweep_pools();
                }
            });
        // INFALLIBLE: Go's `gtimer.AddSingleton` cannot fail either; if the OS
        // refuses a thread, pools still expire lazily inside `get`.
        if let Err(error) = spawned {
            eprintln!("gf-pool: sweeper thread unavailable: {error}");
        }
    });
    let mut pools = POOLS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    pools.push(pool);
}

/// Visits every live pool once, dropping the registrations of freed ones.
fn sweep_pools() {
    let live: Vec<Arc<dyn Sweep>> = {
        let mut pools = POOLS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pools.retain(|pool| pool.strong_count() > 0);
        pools.iter().filter_map(Weak::upgrade).collect()
    };
    for pool in live {
        pool.sweep();
    }
}

/// Current wall-clock time in milliseconds, like `gtime.TimestampMilli`.
fn now_millis() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_millis()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

/// Converts a duration to whole milliseconds, saturating like Go's integer
/// division for absurd values.
fn duration_millis(duration: Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

/// An object-reusable, synchronized pool, like Go's `gpool.TPool`.
///
/// Dropping the last handle does not close the pool; call [`TPool::close`] to
/// stop reuse and release the values.
pub struct TPool<T> {
    inner: Arc<Inner<T>>,
}

impl<T: Send + Sync + 'static> TPool<T> {
    /// Creates a pool, like Go's `NewTPool`.
    ///
    /// `new_func` is optional, matching Go's nullable `newFunc` argument.
    pub fn new(
        ttl: impl Into<Ttl>,
        new_func: Option<TPoolNewFunc<T>>,
        expire_func: Option<TPoolExpireFunc<T>>,
    ) -> TPool<T> {
        let inner = Arc::new(Inner {
            items: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            drained: AtomicBool::new(false),
            ttl: ttl.into(),
            new_func,
            expire_func,
        });
        let as_sweep: Arc<dyn Sweep> = inner.clone();
        let weak = Arc::downgrade(&as_sweep);
        register(weak);
        TPool { inner }
    }

    /// The idle lifetime this pool was created with.
    pub fn ttl(&self) -> Ttl {
        self.inner.ttl
    }
}

impl<T> TPool<T> {
    /// Puts a value back into the pool.
    pub fn put(&self, value: T) -> Result<(), Error> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_OPERATION,
                &["pool is closed"],
            ));
        }
        let expire_at = self.inner.ttl.expiry_millis(now_millis());
        self.inner.push_back(PoolItem { value, expire_at });
        Ok(())
    }

    /// Puts a value back, panicking when the pool is closed (Go's `MustPut`).
    pub fn must_put(&self, value: T) {
        if let Err(error) = self.put(value) {
            panic!("{error}");
        }
    }

    /// Picks a value, dropping stale ones first.
    ///
    /// Returns `new_func`'s value when nothing usable is left, and the
    /// "pool is empty" error when no `new_func` was configured.
    pub fn get(&self) -> Result<T, Error> {
        while !self.inner.closed.load(Ordering::SeqCst) {
            match self.inner.pop_front() {
                Some(item) => {
                    if item.expire_at == 0 || item.expire_at > now_millis() {
                        return Ok(item.value);
                    }
                    self.inner.retire(item);
                }
                None => break,
            }
        }
        if let Some(new_func) = &self.inner.new_func {
            return new_func();
        }
        Err(gerror::new_code(
            &gcode::CODE_INVALID_OPERATION,
            &["pool is empty"],
        ))
    }

    /// Removes every idle value, calling the expire callback per value.
    pub fn clear(&self) {
        self.inner.release_all();
    }

    /// The number of values currently idle in the pool.
    pub fn size(&self) -> usize {
        self.inner.lock_items().len()
    }

    /// Closes the pool and releases its idle values.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
        if !self.inner.drained.swap(true, Ordering::SeqCst) {
            self.inner.release_all();
        }
    }
}

impl<T> Clone for TPool<T> {
    /// Shares the same pool, like copying Go's `*TPool`.
    fn clone(&self) -> TPool<T> {
        TPool {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Go's non-generic `gpool.Pool`, a pool of [`Value`]s.
pub type Pool = TPool<Value>;
/// Go's `gpool.NewFunc` alias.
pub type NewFunc = TPoolNewFunc<Value>;
/// Go's `gpool.ExpireFunc` alias.
pub type ExpireFunc = TPoolExpireFunc<Value>;

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};

    use super::*;

    fn counting_expire(counter: Arc<AtomicI64>) -> TPoolExpireFunc<i64> {
        Box::new(move |_value| {
            counter.fetch_add(1, Ordering::SeqCst);
        })
    }

    // Translated from Test_Gpool's `expire = 0` block.
    #[test]
    fn ttl_zero_never_expires() {
        let pool: TPool<&str> = TPool::new(Ttl::Never, Some(Box::new(|| Ok("hello"))), None);
        pool.put("a").expect("put");
        pool.put("b").expect("put");
        std::thread::sleep(Duration::from_millis(1100));
        let first = pool.get().expect("get");
        assert!(first == "a" || first == "b");

        pool.clear();
        assert_eq!(pool.size(), 0);

        assert_eq!(pool.get().expect("new func"), "hello");

        pool.put("c").expect("put");
        pool.put("d").expect("put");
        let first = pool.get().expect("get");
        assert!(first == "c" || first == "d");

        pool.close();
        assert_eq!(pool.get().expect("new func after close"), "hello");
    }

    // Translated from Test_Gpool's `expire > 0` block, shortened from two
    // seconds to one hundred milliseconds.
    #[test]
    fn positive_ttl_expires_stale_values() {
        let pool: TPool<i64> = TPool::new(
            Duration::from_millis(100),
            None,
            Some(Box::new(|value| assert!(value < 10))),
        );
        for index in 0..10 {
            pool.put(index).expect("put");
        }
        assert_eq!(pool.size(), 10);
        assert_eq!(pool.get().expect("first value"), 0);

        std::thread::sleep(Duration::from_millis(150));
        let error = pool.get().expect_err("stale values expire");
        assert_eq!(error.to_string(), "pool is empty");
        assert_eq!(pool.size(), 0);

        for index in 0..10 {
            pool.put(index).expect("put");
        }
        assert_eq!(pool.size(), 10);
        assert_eq!(pool.get().expect("first value"), 0);

        pool.close();
        std::thread::sleep(Duration::from_millis(50));
        let error = pool.put(1).expect_err("closed pool rejects puts");
        assert_eq!(error.to_string(), "pool is closed");
    }

    // Translated from Test_Gpool's `expire < 0` block.
    #[test]
    fn immediate_ttl_expires_on_get() {
        let pool: TPool<i64> = TPool::new(Ttl::Immediate, None, None);
        let error = pool.get().expect_err("no value can be reused");
        assert_eq!(error.to_string(), "pool is empty");

        pool.put(1).expect("put");
        assert_eq!(pool.size(), 1);
        let error = pool.get().expect_err("expired on the way out");
        assert_eq!(error.to_string(), "pool is empty");
        assert_eq!(pool.size(), 0);
    }

    // Translated from Test_Gpool's delayed-expiry block: `get` drops the
    // stale head and hands out the first value that is still live.
    #[test]
    fn get_skips_expired_values_and_keeps_live_ones() {
        let pool: TPool<i64> = TPool::new(Duration::from_millis(50), None, None);
        pool.put(1).expect("put");
        std::thread::sleep(Duration::from_millis(60));
        pool.put(2).expect("put");
        assert_eq!(pool.get().expect("live value"), 2);
        assert_eq!(pool.size(), 0);
    }

    // Translated from Test_Gpool's clear/close callback counting.
    #[test]
    fn clear_and_close_run_the_expire_callback() {
        let expired = Arc::new(AtomicI64::new(0));
        let pool: TPool<i64> = TPool::new(
            Ttl::Never,
            None,
            Some(counting_expire(Arc::clone(&expired))),
        );
        for index in 0..5 {
            pool.put(index).expect("put");
        }
        pool.clear();
        assert_eq!(expired.load(Ordering::SeqCst), 5);

        for index in 0..3 {
            pool.put(index).expect("put");
        }
        pool.close();
        assert_eq!(expired.load(Ordering::SeqCst), 8);
        assert_eq!(pool.size(), 0);
    }

    // The sweeper releases values nobody asked for. The tick is process-wide,
    // so waiting longer than one interval guarantees at least one visit.
    #[test]
    fn the_sweeper_expires_idle_values() {
        let expired = Arc::new(AtomicI64::new(0));
        let pool: TPool<i64> = TPool::new(
            Duration::from_millis(50),
            None,
            Some(counting_expire(Arc::clone(&expired))),
        );
        pool.put(7).expect("put");
        std::thread::sleep(Duration::from_millis(1200));
        assert_eq!(expired.load(Ordering::SeqCst), 1);
        assert_eq!(pool.size(), 0);
    }

    // Translated from Test_TPool_Struct.
    #[test]
    fn generic_pool_round_trips_structs() {
        #[derive(Clone, Debug, PartialEq)]
        struct User {
            id: i64,
            name: String,
        }

        let pool: TPool<User> = TPool::new(Duration::from_secs(3600), None, None);
        let user = User {
            id: 1,
            name: "john".to_owned(),
        };
        pool.put(user.clone()).expect("put");
        assert_eq!(pool.get().expect("get"), user);

        let error = pool.get().expect_err("empty without a new func");
        assert_eq!(error.to_string(), "pool is empty");
    }
}
