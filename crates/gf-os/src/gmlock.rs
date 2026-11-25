//! Port of `os/gmlock`.
//!
//! A process-wide, keyed lock manager: callers lock a string key and the lock
//! is released after the guarded closure returns (or when the returned guard is
//! dropped).
//!
//! Divergences, recorded in ADR-049:
//!
//! - Rust already has RAII guards, so [`Locker::lock`] and [`Locker::rlock`]
//!   return owned guards instead of requiring a separate `Unlock(key)` call;
//!   `Unlock`/`RUnlock` are therefore not ported;
//! - `LockFunc`/`RLockFunc`/`TryLockFunc`/`TryRLockFunc` keep their names as
//!   `lock_func`, `rlock_func`, `try_lock_func`, `try_rlock_func`;
//! - the read/write state is implemented locally with a mutex and a condition
//!   variable (writers get preference), so guards are `'static` and can outlive
//!   the locker borrow.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};

/// The read/write state of one key.
#[derive(Default)]
struct KeyState {
    readers: usize,
    writer: bool,
    waiting_writers: usize,
}

/// One keyed lock.
struct KeyLock {
    state: Mutex<KeyState>,
    condition: Condvar,
}

impl KeyLock {
    fn new() -> Arc<KeyLock> {
        Arc::new(KeyLock {
            state: Mutex::new(KeyState::default()),
            condition: Condvar::new(),
        })
    }

    fn lock_write(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.waiting_writers += 1;
        while state.writer || state.readers > 0 {
            state = self
                .condition
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.waiting_writers -= 1;
        state.writer = true;
    }

    fn try_lock_write(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.writer || state.readers > 0 {
            return false;
        }
        state.writer = true;
        true
    }

    fn unlock_write(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.writer = false;
        self.condition.notify_all();
    }

    fn lock_read(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        // Writers get preference so a steady stream of readers cannot starve
        // a waiting writer.
        while state.writer || state.waiting_writers > 0 {
            state = self
                .condition
                .wait(state)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        state.readers += 1;
    }

    fn try_lock_read(&self) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if state.writer || state.waiting_writers > 0 {
            return false;
        }
        state.readers += 1;
        true
    }

    fn unlock_read(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        state.readers -= 1;
        if state.readers == 0 {
            self.condition.notify_all();
        }
    }
}

/// An owned write guard; dropping it releases the key.
pub struct WriteGuard {
    lock: Arc<KeyLock>,
}

impl Drop for WriteGuard {
    fn drop(&mut self) {
        self.lock.unlock_write();
    }
}

/// An owned read guard; dropping it releases the key.
pub struct ReadGuard {
    lock: Arc<KeyLock>,
}

impl Drop for ReadGuard {
    fn drop(&mut self) {
        self.lock.unlock_read();
    }
}

/// A memory-based locker, like Go's `gmlock.Locker`.
///
/// Note that there is no automatic expiry: keys stay in the locker until
/// [`Locker::remove`] or [`Locker::clear`].
#[derive(Default)]
pub struct Locker {
    locks: Mutex<HashMap<String, Arc<KeyLock>>>,
}

impl Locker {
    /// Creates an empty locker, like `New`.
    pub fn new() -> Locker {
        Locker::default()
    }

    /// Returns the lock of `key`, creating it when missing.
    fn get_or_new(&self, key: &str) -> Arc<KeyLock> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Arc::clone(locks.entry(key.to_owned()).or_insert_with(KeyLock::new))
    }

    /// Locks `key` for writing, blocking until it is available, like `Lock`.
    pub fn lock(&self, key: &str) -> WriteGuard {
        let lock = self.get_or_new(key);
        lock.lock_write();
        WriteGuard { lock }
    }

    /// Tries to lock `key` for writing, like `TryLock`.
    pub fn try_lock(&self, key: &str) -> Option<WriteGuard> {
        let lock = self.get_or_new(key);
        if lock.try_lock_write() {
            Some(WriteGuard { lock })
        } else {
            None
        }
    }

    /// Locks `key` for reading, blocking until it is available, like `RLock`.
    pub fn rlock(&self, key: &str) -> ReadGuard {
        let lock = self.get_or_new(key);
        lock.lock_read();
        ReadGuard { lock }
    }

    /// Tries to lock `key` for reading, like `TryRLock`.
    pub fn try_rlock(&self, key: &str) -> Option<ReadGuard> {
        let lock = self.get_or_new(key);
        if lock.try_lock_read() {
            Some(ReadGuard { lock })
        } else {
            None
        }
    }

    /// Runs `function` under the write lock of `key`, like `LockFunc`.
    pub fn lock_func<R>(&self, key: &str, function: impl FnOnce() -> R) -> R {
        let _guard = self.lock(key);
        function()
    }

    /// Runs `function` under the read lock of `key`, like `RLockFunc`.
    pub fn rlock_func<R>(&self, key: &str, function: impl FnOnce() -> R) -> R {
        let _guard = self.rlock(key);
        function()
    }

    /// Runs `function` under the write lock of `key` when possible, like
    /// `TryLockFunc`; returns whether the lock was acquired.
    pub fn try_lock_func(&self, key: &str, function: impl FnOnce()) -> bool {
        match self.try_lock(key) {
            Some(_guard) => {
                function();
                true
            }
            None => false,
        }
    }

    /// Runs `function` under the read lock of `key` when possible, like
    /// `TryRLockFunc`; returns whether the lock was acquired.
    pub fn try_rlock_func(&self, key: &str, function: impl FnOnce()) -> bool {
        match self.try_rlock(key) {
            Some(_guard) => {
                function();
                true
            }
            None => false,
        }
    }

    /// Removes the lock of `key`, like `Remove`.
    pub fn remove(&self, key: &str) {
        self.locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(key);
    }

    /// Removes every lock, like `Clear`.
    pub fn clear(&self) {
        self.locks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

/// The package default locker, like Go's `locker`.
fn default_locker() -> &'static Locker {
    static LOCKER: OnceLock<Locker> = OnceLock::new();
    LOCKER.get_or_init(Locker::new)
}

/// Locks the key with the default locker, like `Lock`.
pub fn lock(key: &str) -> WriteGuard {
    default_locker().lock(key)
}

/// Tries to lock the key with the default locker, like `TryLock`.
pub fn try_lock(key: &str) -> Option<WriteGuard> {
    default_locker().try_lock(key)
}

/// Locks the key for reading with the default locker, like `RLock`.
pub fn rlock(key: &str) -> ReadGuard {
    default_locker().rlock(key)
}

/// Tries to lock the key for reading with the default locker, like
/// `TryRLock`.
pub fn try_rlock(key: &str) -> Option<ReadGuard> {
    default_locker().try_rlock(key)
}

/// Runs `function` under the default locker's write lock, like `LockFunc`.
pub fn lock_func<R>(key: &str, function: impl FnOnce() -> R) -> R {
    default_locker().lock_func(key, function)
}

/// Runs `function` under the default locker's read lock, like `RLockFunc`.
pub fn rlock_func<R>(key: &str, function: impl FnOnce() -> R) -> R {
    default_locker().rlock_func(key, function)
}

/// Runs `function` under the default locker's write lock when possible, like
/// `TryLockFunc`.
pub fn try_lock_func(key: &str, function: impl FnOnce()) -> bool {
    default_locker().try_lock_func(key, function)
}

/// Runs `function` under the default locker's read lock when possible, like
/// `TryRLockFunc`.
pub fn try_rlock_func(key: &str, function: impl FnOnce()) -> bool {
    default_locker().try_rlock_func(key, function)
}

/// Removes the lock of `key` from the default locker.
pub fn remove(key: &str) {
    default_locker().remove(key)
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    // Translated from the gmlock write-lock tests.
    #[test]
    fn write_lock_is_exclusive() {
        let locker = Locker::new();
        let guard = locker.lock("key");
        assert!(locker.try_lock("key").is_none(), "a held write lock blocks");
        drop(guard);
        assert!(locker.try_lock("key").is_some(), "released after drop");
    }

    #[test]
    fn different_keys_do_not_block_each_other() {
        let locker = Locker::new();
        let first = locker.lock("a");
        assert!(locker.try_lock("b").is_some());
        drop(first);
    }

    // Translated from the gmlock read-lock tests.
    #[test]
    fn read_locks_are_shared() {
        let locker = Locker::new();
        let first = locker.rlock("key");
        let second = locker.try_rlock("key");
        assert!(second.is_some(), "readers do not block readers");
        assert!(
            locker.try_lock("key").is_none(),
            "a read lock blocks the writer"
        );
        drop(first);
        assert!(
            locker.try_lock("key").is_none(),
            "the remaining reader still blocks"
        );
        drop(second);
        assert!(locker.try_lock("key").is_some());
    }

    #[test]
    fn waiting_writer_blocks_new_readers() {
        let locker = Arc::new(Locker::new());
        let reader = locker.rlock("key");
        let thread_locker = Arc::clone(&locker);
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            let _guard = thread_locker.lock("key");
            let _ = tx.send(());
        });
        // Give the writer a chance to start waiting.
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            locker.try_rlock("key").is_none(),
            "readers must wait for the pending writer"
        );
        drop(reader);
        rx.recv_timeout(Duration::from_secs(2)).expect("writer ran");
        handle.join().expect("join");
    }

    // Translated from the LockFunc/TryLockFunc tests.
    #[test]
    fn lock_func_variants() {
        let locker = Locker::new();
        let ran = AtomicBool::new(false);
        locker.lock_func("key", || ran.store(true, Ordering::SeqCst));
        assert!(ran.load(Ordering::SeqCst));

        let ran = AtomicBool::new(false);
        assert!(locker.try_lock_func("key", || ran.store(true, Ordering::SeqCst)));
        assert!(ran.load(Ordering::SeqCst));

        // A held lock makes the try variant return false without running.
        let guard = locker.lock("key");
        let ran = AtomicBool::new(false);
        assert!(!locker.try_lock_func("key", || ran.store(true, Ordering::SeqCst)));
        assert!(!ran.load(Ordering::SeqCst));
        drop(guard);

        let value = locker.rlock_func("key", || 42);
        assert_eq!(value, 42);
        assert!(locker.try_rlock_func("key", || ()));
    }

    #[test]
    fn lock_blocks_until_released() {
        let locker = Arc::new(Locker::new());
        let counter = Arc::new(AtomicUsize::new(0));
        let guard = locker.lock("key");
        let thread_locker = Arc::clone(&locker);
        let thread_counter = Arc::clone(&counter);
        let (tx, rx) = mpsc::channel();
        let handle = std::thread::spawn(move || {
            thread_locker.lock_func("key", || {
                thread_counter.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            });
        });
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(counter.load(Ordering::SeqCst), 0, "still blocked");
        drop(guard);
        rx.recv_timeout(Duration::from_secs(2)).expect("acquired");
        handle.join().expect("join");
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn remove_and_clear_forget_keys() {
        let locker = Locker::new();
        locker.lock_func("a", || ());
        locker.lock_func("b", || ());
        locker.remove("a");
        locker.clear();
        assert!(locker.try_lock("a").is_some());
        assert!(locker.try_lock("b").is_some());
    }

    #[test]
    fn default_locker_is_shared() {
        let key = "gf-rust-gmlock-default";
        {
            let _guard = lock(key);
            assert!(try_lock(key).is_none());
        }
        assert!(try_lock(key).is_some());
        remove(key);
    }
}
