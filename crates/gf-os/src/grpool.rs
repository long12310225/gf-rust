//! Port of `os/grpool`.
//!
//! A reusable worker pool with a capacity limit, a paused state and a
//! supervisor that forks workers when jobs are waiting.
//!
//! Divergences, recorded in ADR-050:
//!
//! - no `context.Context`: jobs are `Fn()` and the recover callback receives
//!   the panic as an [`Error`];
//! - Go's `gtimer` supervisor becomes one background thread per pool that wakes
//!   twice a second and forks workers for pending jobs;
//! - `AddWithRecover` catches the panic with `catch_unwind` (Go uses
//!   `recover`); plain `Add` lets the panic kill the worker, like Go;
//! - the pool state lives behind an `Arc`, so [`Pool`] is cheap to clone and
//!   the worker threads can reach it.

use std::collections::VecDeque;
use std::panic::AssertUnwindSafe;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// A pool job, like Go's `grpool.Func` (without the context parameter).
pub type Func = Arc<dyn Fn() + Send + Sync>;

/// The panic callback of [`Pool::add_with_recover`], like `RecoverFunc`.
pub type RecoverFunc = Arc<dyn Fn(&Error) + Send + Sync>;

/// Updates the pool limit dynamically, like `LimitChangerFunc`.
pub type LimitChangerFunc = Arc<dyn Fn(&AtomicI64) -> bool + Send + Sync>;

/// Options for [`Pool::with_option`], like `PoolOption`.
#[derive(Default)]
pub struct PoolOption {
    /// Maximum worker count; `None` means unlimited.
    pub limit: Option<usize>,
    /// Dynamic limit changer.
    pub limit_changer: Option<LimitChangerFunc>,
}

/// The shared pool state.
struct PoolInner {
    limit: AtomicI64,
    count: AtomicI64,
    jobs: Mutex<VecDeque<Func>>,
    closed: AtomicBool,
    paused: AtomicBool,
    limit_changed: AtomicBool,
    limit_changer: Mutex<Option<LimitChangerFunc>>,
}

impl PoolInner {
    /// Forks a worker when the limit allows it, like
    /// `checkAndForkNewGoroutineWorker`.
    fn check_and_fork(self: &Arc<Self>) {
        if self.paused.load(Ordering::SeqCst) {
            return;
        }
        loop {
            let count = self.count.load(Ordering::SeqCst);
            let limit = self.limit.load(Ordering::SeqCst);
            if limit != -1 && count >= limit {
                return;
            }
            if self
                .count
                .compare_exchange(count, count + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                break;
            }
        }
        let inner = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("gf-grpool-worker".to_owned())
            .spawn(move || worker(inner));
        if let Err(error) = spawned {
            // The worker was counted but never started.
            self.count.fetch_sub(1, Ordering::SeqCst);
            eprintln!("gf-grpool: worker thread unavailable: {error}");
        }
    }

    /// Runs pending jobs by forking workers, like `supervisor`.
    fn supervise(self: &Arc<Self>) {
        if self.closed.load(Ordering::SeqCst) {
            return;
        }
        if self.paused.load(Ordering::SeqCst) {
            return;
        }
        let mut changed = self.limit_changed.swap(false, Ordering::SeqCst);
        if let Some(changer) = self
            .limit_changer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
        {
            if changer(&self.limit) {
                changed = true;
            }
            let limit = self.limit.load(Ordering::SeqCst);
            if limit <= 0 && limit != -1 {
                self.limit.store(-1, Ordering::SeqCst);
                changed = true;
            }
        }
        if !changed && self.count.load(Ordering::SeqCst) == 0 {
            changed = true;
        }
        if !changed {
            return;
        }
        let pending = self.pending();
        if pending == 0 {
            return;
        }
        let limit = self.limit.load(Ordering::SeqCst);
        if limit == -1 {
            for _ in 0..pending {
                self.check_and_fork();
            }
            return;
        }
        let available = limit - self.count.load(Ordering::SeqCst);
        if available <= 0 {
            return;
        }
        for _ in 0..pending.min(available as usize) {
            self.check_and_fork();
        }
    }

    /// The number of queued jobs.
    fn pending(&self) -> usize {
        self.jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

/// Decrements the worker count when the worker exits.
struct CountGuard<'a>(&'a AtomicI64);

impl Drop for CountGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The worker loop, like `asynchronousWorker`.
fn worker(inner: Arc<PoolInner>) {
    let _guard = CountGuard(&inner.count);
    loop {
        if inner.closed.load(Ordering::SeqCst) {
            return;
        }
        if inner.paused.load(Ordering::SeqCst) {
            return;
        }
        let job = inner
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_back();
        let Some(job) = job else {
            return;
        };
        if inner.paused.load(Ordering::SeqCst) {
            // Put the job back so a resumed pool still runs it.
            inner
                .jobs
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push_back(job);
            return;
        }
        job();
        // The count guard performs the single decrement when the worker exits,
        // like Go.s deferred `count.Add`.
        let count = inner.count.load(Ordering::SeqCst);
        let limit = inner.limit.load(Ordering::SeqCst);
        if limit > 0 && count > limit {
            return;
        }
    }
}

/// A reusable goroutine pool, like Go's `grpool.Pool`.
#[derive(Clone)]
pub struct Pool {
    inner: Arc<PoolInner>,
}

impl Pool {
    /// Creates a pool, like `New`.
    pub fn new(limit: Option<usize>) -> Pool {
        Pool::with_option(PoolOption {
            limit,
            limit_changer: None,
        })
    }

    /// Creates a pool from options, like `NewWithOption`.
    pub fn with_option(option: PoolOption) -> Pool {
        let limit = match option.limit {
            Some(limit) if limit > 0 => limit as i64,
            _ => -1,
        };
        let inner = Arc::new(PoolInner {
            limit: AtomicI64::new(limit),
            count: AtomicI64::new(0),
            jobs: Mutex::new(VecDeque::new()),
            closed: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            limit_changed: AtomicBool::new(option.limit_changer.is_some()),
            limit_changer: Mutex::new(option.limit_changer),
        });
        // Supervisor loop: forks workers for queued jobs, like `gtimer`.
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("gf-grpool-supervisor".to_owned())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(500));
                    let Some(inner) = weak.upgrade() else {
                        break;
                    };
                    if inner.closed.load(Ordering::SeqCst) {
                        break;
                    }
                    inner.supervise();
                }
            })
            .ok();
        Pool { inner }
    }

    /// Pushes a job to the pool, like `Add`.
    pub fn add(&self, function: Func) -> Result<(), Error> {
        if self.inner.closed.load(Ordering::SeqCst) {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_OPERATION,
                &["goroutine pool is already closed"],
            ));
        }
        self.inner
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_front(function);
        self.inner.check_and_fork();
        Ok(())
    }

    /// Pushes a job with a panic recover callback, like `AddWithRecover`.
    pub fn add_with_recover(
        &self,
        user_function: Func,
        recover_function: Option<RecoverFunc>,
    ) -> Result<(), Error> {
        let wrapped: Func = Arc::new(move || {
            let result = std::panic::catch_unwind(AssertUnwindSafe(|| user_function()));
            if let Err(payload) = result
                && let Some(recover) = &recover_function
            {
                recover(&panic_error(payload));
            }
        });
        self.add(wrapped)
    }

    /// Returns the pool capacity, or `-1` when unlimited, like `Cap`.
    pub fn cap(&self) -> i64 {
        self.inner.limit.load(Ordering::SeqCst)
    }

    /// Changes the capacity, returning the previous one, like `SetCap`.
    pub fn set_cap(&self, cap: i64) -> i64 {
        let cap = if cap <= 0 { -1 } else { cap };
        let old = self.inner.limit.swap(cap, Ordering::SeqCst);
        if cap != old {
            self.inner.limit_changed.store(true, Ordering::SeqCst);
        }
        old
    }

    /// Returns the current worker count, like `Size`.
    pub fn size(&self) -> i64 {
        self.inner.count.load(Ordering::SeqCst)
    }

    /// Returns the queued job count, like `Jobs`.
    pub fn jobs(&self) -> i64 {
        self.inner.pending() as i64
    }

    /// Clears queued jobs, returning how many were removed, like `ClearJobs`.
    pub fn clear_jobs(&self) -> usize {
        let mut jobs = self
            .inner
            .jobs
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let count = jobs.len();
        jobs.clear();
        count
    }

    /// Pauses the pool, like `Pause`.
    pub fn pause(&self) -> bool {
        if self.is_closed() {
            return false;
        }
        self.inner.paused.store(true, Ordering::SeqCst);
        true
    }

    /// Reports whether the pool is paused, like `IsPaused`.
    pub fn is_paused(&self) -> bool {
        self.inner.paused.load(Ordering::SeqCst)
    }

    /// Resumes the pool, like `Resume`.
    pub fn resume(&self) -> bool {
        if self.is_closed() {
            return false;
        }
        if self.inner.paused.swap(false, Ordering::SeqCst) {
            self.inner.limit_changed.store(true, Ordering::SeqCst);
        }
        true
    }

    /// Reports whether the pool is closed, like `IsClosed`.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Closes the pool, like `Close`.
    pub fn close(&self) {
        self.inner.closed.store(true, Ordering::SeqCst);
    }
}

/// Converts a panic payload into an error, like GoFrame's recover handling.
fn panic_error(payload: Box<dyn std::any::Any + Send>) -> Error {
    match payload.downcast::<Error>() {
        Ok(error) => *error,
        Err(payload) => match payload.downcast::<String>() {
            Ok(message) => gerror::new_codef(
                &gcode::CODE_INTERNAL_PANIC,
                format!("exception recovered: {message}"),
                Vec::new(),
            ),
            Err(payload) => match payload.downcast::<&str>() {
                Ok(message) => gerror::new_codef(
                    &gcode::CODE_INTERNAL_PANIC,
                    format!("exception recovered: {message}"),
                    Vec::new(),
                ),
                Err(_) => gerror::new_code(&gcode::CODE_INTERNAL_PANIC, &["exception recovered"]),
            },
        },
    }
}

/// The package default pool, like Go's `defaultPool`.
fn default_pool() -> &'static Pool {
    static DEFAULT: OnceLock<Pool> = OnceLock::new();
    DEFAULT.get_or_init(|| Pool::new(None))
}

/// Adds a job to the default pool, like `Add`.
pub fn add(function: Func) -> Result<(), Error> {
    default_pool().add(function)
}

/// Adds a job with a recover callback to the default pool, like
/// `AddWithRecover`.
pub fn add_with_recover(
    user_function: Func,
    recover_function: Option<RecoverFunc>,
) -> Result<(), Error> {
    default_pool().add_with_recover(user_function, recover_function)
}

/// Returns the default pool size, like `Size`.
pub fn size() -> i64 {
    default_pool().size()
}

/// Returns the default pool job count, like `Jobs`.
pub fn jobs() -> i64 {
    default_pool().jobs()
}

/// Pauses the default pool, like `Pause`.
pub fn pause() -> bool {
    default_pool().pause()
}

/// Reports whether the default pool is paused, like `IsPaused`.
pub fn is_paused() -> bool {
    default_pool().is_paused()
}

/// Resumes the default pool, like `Resume`.
pub fn resume() -> bool {
    default_pool().resume()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;

    use super::*;

    // Translated from Test_Pool_Add.
    #[test]
    fn add_runs_the_job() {
        let pool = Pool::new(None);
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(Mutex::new(tx));
        pool.add(Arc::new(move || {
            let _ = sender.lock().unwrap().send(());
        }))
        .expect("add");
        rx.recv_timeout(Duration::from_secs(2)).expect("job ran");
        pool.close();
    }

    // Translated from Test_Pool_Limit.
    #[test]
    fn limit_caps_concurrency() {
        let pool = Pool::new(Some(2));
        let running = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(Mutex::new(tx));
        for _ in 0..10 {
            let running = Arc::clone(&running);
            let maximum = Arc::clone(&maximum);
            let sender = Arc::clone(&sender);
            pool.add(Arc::new(move || {
                let current = running.fetch_add(1, Ordering::SeqCst) + 1;
                maximum.fetch_max(current, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(20));
                running.fetch_sub(1, Ordering::SeqCst);
                let _ = sender.lock().unwrap().send(());
            }))
            .expect("add");
        }
        for _ in 0..10 {
            rx.recv_timeout(Duration::from_secs(5)).expect("job");
        }
        assert!(
            maximum.load(Ordering::SeqCst) <= 2,
            "concurrency exceeded the limit: {}",
            maximum.load(Ordering::SeqCst)
        );
        pool.close();
    }

    // Translated from Test_Pool_Jobs_Size.
    #[test]
    fn jobs_and_size_accounting() {
        let pool = Pool::new(Some(1));
        pool.pause();
        let ran = Arc::new(AtomicUsize::new(0));
        for _ in 0..5 {
            let ran = Arc::clone(&ran);
            pool.add(Arc::new(move || {
                ran.fetch_add(1, Ordering::SeqCst);
            }))
            .expect("add");
        }
        assert_eq!(pool.jobs(), 5, "paused jobs stay queued");
        assert_eq!(pool.size(), 0, "no worker for a paused pool");
        assert_eq!(pool.clear_jobs(), 5);
        assert_eq!(pool.jobs(), 0);
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        pool.close();
    }

    // Translated from Test_Pool_Pause_Resume.
    #[test]
    fn pause_and_resume() {
        let pool = Pool::new(None);
        assert!(pool.pause());
        assert!(pool.is_paused());
        let ran = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&ran);
        pool.add(Arc::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        }))
        .expect("add");
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(ran.load(Ordering::SeqCst), 0, "paused pool runs nothing");
        assert!(pool.resume());
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while ran.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(ran.load(Ordering::SeqCst), 1);
        pool.close();
    }

    #[test]
    fn add_after_close_fails() {
        let pool = Pool::new(None);
        pool.close();
        assert!(pool.is_closed());
        let error = pool.add(Arc::new(|| {})).expect_err("closed pool");
        assert_eq!(gerror::code(&error), gcode::CODE_INVALID_OPERATION);
    }

    // Translated from Test_Pool_AddWithRecover.
    #[test]
    fn add_with_recover_catches_panics() {
        let pool = Pool::new(None);
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(Mutex::new(tx));
        let notify = Arc::clone(&sender);
        pool.add_with_recover(
            Arc::new(|| panic!("boom")),
            Some(Arc::new(move |error: &Error| {
                let _ = notify.lock().unwrap().send(error.to_string());
            })),
        )
        .expect("add");
        let message = rx.recv_timeout(Duration::from_secs(2)).expect("recover");
        assert!(message.contains("boom"), "got {message}");
        pool.close();
    }

    // Translated from Test_Pool_SetCap.
    #[test]
    fn set_cap_returns_the_previous_limit() {
        let pool = Pool::new(Some(5));
        assert_eq!(pool.cap(), 5);
        assert_eq!(pool.set_cap(10), 5);
        assert_eq!(pool.cap(), 10);
        assert_eq!(pool.set_cap(0), 10);
        assert_eq!(pool.cap(), -1);
        pool.close();
    }

    #[test]
    fn default_pool_accepts_jobs() {
        let (tx, rx) = mpsc::channel();
        let sender = Arc::new(Mutex::new(tx));
        add(Arc::new(move || {
            let _ = sender.lock().unwrap().send(());
        }))
        .expect("add");
        rx.recv_timeout(Duration::from_secs(2)).expect("job ran");
        let _ = size();
        let _ = jobs();
        let _ = is_paused();
    }
}
