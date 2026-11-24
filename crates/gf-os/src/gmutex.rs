//! Port of `os/gmutex`.
//!
//! GoFrame's `gmutex.Mutex` and `gmutex.RWMutex` embed `sync.Mutex` and
//! `sync.RWMutex` and add callback-shaped helpers. Rust locks are released by
//! dropping a guard, so the wrappers hand out [`MutexGuard`] and
//! [`RwLockReadGuard`]/[`RwLockWriteGuard`] for the plain operations and keep
//! the callback helpers (`lock_func`, `rlock_func` and their `try_` forms) as
//! the reason to use this package at all (ADR-038).
//!
//! Divergences from Go, forced by the language:
//!
//! - `lock`/`rlock` return guards instead of unlocking explicitly.
//! - `try_lock_func`/`try_rlock_func` return `Option<R>` (the callback's
//!   result) instead of `bool`; `None` means the lock was busy and `f` did not
//!   run.
//! - Go's zero-value `var mu gmutex.Mutex` works; Rust callers write
//!   [`Mutex::new`] or `Mutex::default`.
//!
//! Like the rest of the port, mutexes do not poison: a panic inside a
//! callback leaves the lock usable, which matches Go's `sync` package.

use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

/// A mutex with callback helpers, like Go's `gmutex.Mutex`.
#[derive(Default)]
pub struct Mutex {
    inner: std::sync::Mutex<()>,
}

impl Mutex {
    /// Creates an unlocked mutex.
    pub fn new() -> Mutex {
        Mutex {
            inner: std::sync::Mutex::new(()),
        }
    }

    /// Locks the mutex, returning a guard that unlocks on drop.
    pub fn lock(&self) -> MutexGuard<'_, ()> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tries to lock the mutex without blocking.
    pub fn try_lock(&self) -> Option<MutexGuard<'_, ()>> {
        self.inner.try_lock().ok()
    }

    /// Runs `f` while holding the lock, like Go's `LockFunc`.
    pub fn lock_func<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.lock();
        f()
    }

    /// Runs `f` under the lock when it is free, like Go's `TryLockFunc`.
    ///
    /// Returns the callback's result, or `None` when the mutex was busy.
    pub fn try_lock_func<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        let _guard = self.try_lock()?;
        Some(f())
    }
}

/// A read-write mutex with callback helpers, like Go's `gmutex.RWMutex`.
#[derive(Default)]
pub struct RWMutex {
    inner: std::sync::RwLock<()>,
}

impl RWMutex {
    /// Creates an unlocked read-write mutex.
    pub fn new() -> RWMutex {
        RWMutex {
            inner: std::sync::RwLock::new(()),
        }
    }

    /// Locks for writing (Go's `Lock`) and returns the guard.
    pub fn lock(&self) -> RwLockWriteGuard<'_, ()> {
        self.inner
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Locks for reading (Go's `RLock`) and returns the guard.
    pub fn rlock(&self) -> RwLockReadGuard<'_, ()> {
        self.inner
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Tries to lock for writing (Go's `TryLock`) without blocking.
    pub fn try_lock(&self) -> Option<RwLockWriteGuard<'_, ()>> {
        self.inner.try_write().ok()
    }

    /// Tries to lock for reading (Go's `TryRLock`) without blocking.
    pub fn try_rlock(&self) -> Option<RwLockReadGuard<'_, ()>> {
        self.inner.try_read().ok()
    }

    /// Runs `f` while holding the write lock, like Go's `LockFunc`.
    pub fn lock_func<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.lock();
        f()
    }

    /// Runs `f` while holding the read lock, like Go's `RLockFunc`.
    pub fn rlock_func<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.rlock();
        f()
    }

    /// Runs `f` under the write lock when it is free, like `TryLockFunc`.
    pub fn try_lock_func<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        let _guard = self.try_lock()?;
        Some(f())
    }

    /// Runs `f` under the read lock when it is free, like `TryRLockFunc`.
    pub fn try_rlock_func<R>(&self, f: impl FnOnce() -> R) -> Option<R> {
        let _guard = self.try_rlock()?;
        Some(f())
    }
}

/// Creates a new [`RWMutex`], like Go's deprecated `gmutex.New`.
#[deprecated(note = "use Mutex::new or RWMutex::new instead, like Go's gmutex.New")]
pub fn new() -> RWMutex {
    RWMutex::new()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::sync::mpsc;

    use super::*;

    // Translated from Test_Mutex_LockFunc: the callback shape serializes
    // writers. The Go test asserts on sleeps; the port joins the threads.
    #[test]
    fn lock_func_serializes_writers() {
        let mutex = Arc::new(Mutex::new());
        let counter = Arc::new(AtomicI64::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let mutex = Arc::clone(&mutex);
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..100 {
                    mutex.lock_func(|| {
                        let value = counter.load(Ordering::SeqCst);
                        counter.store(value + 1, Ordering::SeqCst);
                    });
                }
            }));
        }
        for handle in handles {
            handle.join().expect("writer thread");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 400);
    }

    // Translated from Test_Mutex_TryLockFunc: a busy mutex skips the callback.
    #[test]
    fn try_lock_func_reports_a_busy_mutex() {
        let mutex = Mutex::new();
        let guard = mutex.lock();
        assert_eq!(mutex.try_lock_func(|| 1), None);
        drop(guard);
        assert_eq!(mutex.try_lock_func(|| 1), Some(1));
    }

    // Translated from Test_RWMutex_TryLockFunc / Test_RWMutex_TryRLockFunc.
    #[test]
    fn rwmutex_try_variants_respect_the_lock_state() {
        let mutex = RWMutex::new();
        assert_eq!(mutex.try_lock_func(|| "w"), Some("w"));
        assert_eq!(mutex.try_rlock_func(|| "r"), Some("r"));

        let write = mutex.lock();
        assert_eq!(mutex.try_lock_func(|| 1), None);
        assert_eq!(mutex.try_rlock_func(|| 1), None);
        drop(write);

        // Readers share; a write attempt while reading is refused.
        let read = mutex.rlock();
        assert_eq!(mutex.try_rlock_func(|| 2), Some(2));
        assert_eq!(mutex.try_lock_func(|| 2), None);
        drop(read);

        assert_eq!(mutex.try_lock_func(|| 3), Some(3));
    }

    // Translated from Test_RWMutex_RUnlock: a reader and a writer queue on the
    // same lock without deadlocking.
    #[test]
    fn rwmutex_reader_then_writer() {
        let mutex = Arc::new(RWMutex::new());
        let (started, started_rx) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();

        let writer = {
            let mutex = Arc::clone(&mutex);
            std::thread::spawn(move || {
                mutex.lock_func(|| {
                    started.send(()).expect("signal");
                    release_rx.recv().expect("wait for the release");
                });
            })
        };

        started_rx.recv().expect("writer holds the lock");
        assert!(mutex.try_lock().is_none());
        release.send(()).expect("release writer");
        writer.join().expect("writer thread");

        assert!(mutex.try_lock_func(|| ()).is_some());
    }

    // Translated from Test_RWMutex_RLockFunc: concurrent readers are allowed.
    #[test]
    fn rwmutex_readers_run_together() {
        let mutex = Arc::new(RWMutex::new());
        let counter = Arc::new(AtomicI64::new(0));
        let mut handles = Vec::new();
        for _ in 0..4 {
            let mutex = Arc::clone(&mutex);
            let counter = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for _ in 0..50 {
                    mutex.rlock_func(|| {
                        counter.fetch_add(1, Ordering::SeqCst);
                    });
                }
            }));
        }
        for handle in handles {
            handle.join().expect("reader thread");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 200);
    }

    // Go's deprecated `New` returns a `*RWMutex`.
    #[test]
    #[expect(deprecated, reason = "the Go API this port mirrors is deprecated too")]
    fn new_returns_an_rwmutex() {
        let mutex = new();
        assert_eq!(mutex.try_lock_func(|| 7), Some(7));
    }
}
