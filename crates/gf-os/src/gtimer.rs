//! Port of `os/gtimer`.
//!
//! A tick-based timer: entries live in a priority queue keyed by the tick they
//! must run at, and one loop thread increments the tick counter at the
//! configured interval, running every due job on its own thread.
//!
//! Divergences, recorded in ADR-053:
//!
//! - no `context.Context`: jobs are `Fn()` and take nothing;
//! - Go's `panic(panicExit)` sentinel for [`exit`] becomes a thread-local
//!   "current entry" that [`exit`] marks closed;
//! - a panicking job is contained: the entry returns to `Ready` (Go lets the
//!   job goroutine die and re-panics);
//! - the priority queue uses `BinaryHeap` with a reversed `Ord`, and
//!   `next_priority` is cached atomically like Go.

use std::collections::BinaryHeap;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicI64, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::Duration;

use gf_errors::gerror::Error;
use gf_internal::command;

/// The job is ready to run.
pub const STATUS_READY: i32 = 0;
/// The job is running.
pub const STATUS_RUNNING: i32 = 1;
/// The job is stopped.
pub const STATUS_STOPPED: i32 = 2;
/// The job is closed and will be removed from its timer.
pub const STATUS_CLOSED: i32 = -1;

/// The environment/command key configuring the default tick interval, like
/// Go's `commandEnvKeyForInterval`.
pub const INTERVAL_ENV_KEY: &str = "gf.gtimer.interval";

/// A timing job, like Go's `JobFunc` (without the context parameter).
pub type JobFunc = Arc<dyn Fn() + Send + Sync>;

/// Timer options, like `TimerOptions`.
#[derive(Clone, Copy, Debug)]
pub struct TimerOptions {
    /// The tick interval of the timer.
    pub interval: Duration,
    /// Runs jobs on the first tick instead of waiting one interval.
    pub quick: bool,
}

impl Default for TimerOptions {
    fn default() -> TimerOptions {
        TimerOptions {
            interval: default_interval(),
            quick: false,
        }
    }
}

/// The default tick interval, like Go's `getDefaultInterval`.
fn default_interval() -> Duration {
    static DEFAULT: OnceLock<Duration> = OnceLock::new();
    *DEFAULT.get_or_init(|| {
        let text = command::get_opt_with_env(INTERVAL_ENV_KEY, Some("100"));
        match text.trim().parse::<u64>() {
            Ok(millis) if millis > 0 => Duration::from_millis(millis),
            _ => Duration::from_millis(100),
        }
    })
}

/// The state shared between a timer, its entries and the loop thread.
struct TimerInner {
    status: AtomicI32,
    ticks: AtomicI64,
    queue: Mutex<BinaryHeap<QueueItem>>,
    next_priority: AtomicI64,
    options: TimerOptions,
}

/// One queued entry, ordered so the earliest `next_ticks` is popped first.
struct QueueItem {
    next_ticks: i64,
    sequence: u64,
    entry: Arc<EntryState>,
}

impl PartialEq for QueueItem {
    fn eq(&self, other: &QueueItem) -> bool {
        self.next_ticks == other.next_ticks && self.sequence == other.sequence
    }
}

impl Eq for QueueItem {}

impl PartialOrd for QueueItem {
    fn partial_cmp(&self, other: &QueueItem) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for QueueItem {
    fn cmp(&self, other: &QueueItem) -> std::cmp::Ordering {
        // Reversed so `BinaryHeap` (a max-heap) yields the minimum.
        other
            .next_ticks
            .cmp(&self.next_ticks)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

/// The mutable state of one timing job.
struct EntryState {
    job: JobFunc,
    ticks: i64,
    times: AtomicI64,
    status: AtomicI32,
    singleton: AtomicBool,
    next_ticks: AtomicI64,
    infinite: AtomicBool,
    timer: Weak<TimerInner>,
}

thread_local! {
    /// The entry whose job is running on this thread, for [`exit`].
    static CURRENT_ENTRY: std::cell::RefCell<Option<Arc<EntryState>>> =
        const { std::cell::RefCell::new(None) };
}

impl EntryState {
    fn status(&self) -> i32 {
        self.status.load(Ordering::SeqCst)
    }

    fn set_status(&self, status: i32) -> i32 {
        self.status.swap(status, Ordering::SeqCst)
    }

    /// Runs the job, like Go's `Run`.
    fn run(self: &Arc<Self>) {
        if !self.infinite.load(Ordering::SeqCst) {
            let left = self.times.fetch_sub(1, Ordering::SeqCst) - 1;
            if left < 0 {
                self.set_status(STATUS_CLOSED);
                return;
            }
        }
        let entry = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("gf-gtimer-job".to_owned())
            .spawn(move || entry.call_job());
        if let Err(error) = spawned {
            eprintln!("gf-gtimer: job thread unavailable: {error}");
        }
    }

    /// Executes the job and restores the entry status, like `callJobFunc`.
    fn call_job(self: Arc<Self>) {
        CURRENT_ENTRY.with(|current| {
            *current.borrow_mut() = Some(Arc::clone(&self));
        });
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.job)()));
        CURRENT_ENTRY.with(|current| {
            *current.borrow_mut() = None;
        });
        if let Err(payload) = result {
            eprintln!(
                "gf-gtimer: job panicked: {}",
                gf_util::gconv::string(&payload_message(payload.as_ref()))
            );
        }
        if self.status() == STATUS_RUNNING {
            self.set_status(STATUS_READY);
        }
    }

    /// Checks the tick and runs the job when due, like
    /// `doCheckAndRunByTicks`.
    fn check_and_run(self: &Arc<Self>, current_ticks: i64) {
        if current_ticks < self.next_ticks.load(Ordering::SeqCst) {
            return;
        }
        self.next_ticks
            .store(current_ticks + self.ticks, Ordering::SeqCst);
        match self.status() {
            STATUS_RUNNING => {
                if self.singleton.load(Ordering::SeqCst) {
                    return;
                }
            }
            STATUS_READY => {
                if self
                    .status
                    .compare_exchange(
                        STATUS_READY,
                        STATUS_RUNNING,
                        Ordering::SeqCst,
                        Ordering::SeqCst,
                    )
                    .is_err()
                {
                    return;
                }
            }
            STATUS_STOPPED | STATUS_CLOSED => return,
            _ => return,
        }
        self.run();
    }
}

/// Extracts a printable message from a panic payload.
fn payload_message(payload: &(dyn std::any::Any + Send)) -> gf_core::Value {
    if let Some(message) = payload.downcast_ref::<String>() {
        return gf_core::Value::String(message.clone());
    }
    if let Some(message) = payload.downcast_ref::<&str>() {
        return gf_core::Value::String((*message).to_owned());
    }
    if let Some(error) = payload.downcast_ref::<Error>() {
        return gf_core::Value::String(error.to_string());
    }
    gf_core::Value::String("panic".to_owned())
}

/// The timer manager, like Go's `Timer`.
pub struct Timer {
    inner: Arc<TimerInner>,
}

impl Timer {
    /// Creates a timer with `options`, like `New`.
    pub fn new(options: Option<TimerOptions>) -> Timer {
        let mut options = options.unwrap_or_default();
        if options.interval.is_zero() {
            options.interval = default_interval();
        }
        let inner = Arc::new(TimerInner {
            status: AtomicI32::new(STATUS_RUNNING),
            ticks: AtomicI64::new(0),
            queue: Mutex::new(BinaryHeap::new()),
            next_priority: AtomicI64::new(i64::MAX),
            options,
        });
        let weak = Arc::downgrade(&inner);
        std::thread::Builder::new()
            .name("gf-gtimer-loop".to_owned())
            .spawn(move || loop_thread(weak))
            .ok();
        Timer { inner }
    }

    /// Adds a job running every `interval`, like `Add`.
    pub fn add(&self, interval: Duration, job: JobFunc) -> Entry {
        self.create_entry(interval, job, false, -1, STATUS_READY)
    }

    /// Adds a job with detailed parameters, like `AddEntry`.
    pub fn add_entry(
        &self,
        interval: Duration,
        job: JobFunc,
        singleton: bool,
        times: i64,
        status: i32,
    ) -> Entry {
        self.create_entry(interval, job, singleton, times, status)
    }

    /// Adds a singleton job, like `AddSingleton`.
    pub fn add_singleton(&self, interval: Duration, job: JobFunc) -> Entry {
        self.create_entry(interval, job, true, -1, STATUS_READY)
    }

    /// Adds a job that runs once, like `AddOnce`.
    pub fn add_once(&self, interval: Duration, job: JobFunc) -> Entry {
        self.create_entry(interval, job, true, 1, STATUS_READY)
    }

    /// Adds a job limited to `times` runs, like `AddTimes`.
    pub fn add_times(&self, interval: Duration, times: i64, job: JobFunc) -> Entry {
        self.create_entry(interval, job, true, times, STATUS_READY)
    }

    /// Adds a job after `delay`, like `DelayAdd`.
    pub fn delay_add(&self, delay: Duration, interval: Duration, job: JobFunc) {
        let timer = self.handle();
        self.add_once(
            delay,
            Arc::new(move || {
                timer.add(interval, Arc::clone(&job));
            }),
        );
    }

    /// Adds a job after `delay` with detailed parameters, like
    /// `DelayAddEntry`.
    pub fn delay_add_entry(
        &self,
        delay: Duration,
        interval: Duration,
        job: JobFunc,
        singleton: bool,
        times: i64,
        status: i32,
    ) {
        let timer = self.handle();
        self.add_once(
            delay,
            Arc::new(move || {
                timer.add_entry(interval, Arc::clone(&job), singleton, times, status);
            }),
        );
    }

    /// Adds a singleton job after `delay`, like `DelayAddSingleton`.
    pub fn delay_add_singleton(&self, delay: Duration, interval: Duration, job: JobFunc) {
        let timer = self.handle();
        self.add_once(
            delay,
            Arc::new(move || {
                timer.add_singleton(interval, Arc::clone(&job));
            }),
        );
    }

    /// Adds a one-shot job after `delay`, like `DelayAddOnce`.
    pub fn delay_add_once(&self, delay: Duration, interval: Duration, job: JobFunc) {
        let timer = self.handle();
        self.add_once(
            delay,
            Arc::new(move || {
                timer.add_once(interval, Arc::clone(&job));
            }),
        );
    }

    /// Adds a times-limited job after `delay`, like `DelayAddTimes`.
    pub fn delay_add_times(&self, delay: Duration, interval: Duration, times: i64, job: JobFunc) {
        let timer = self.handle();
        self.add_once(
            delay,
            Arc::new(move || {
                timer.add_times(interval, times, Arc::clone(&job));
            }),
        );
    }

    /// Starts the timer, like `Start`.
    pub fn start(&self) {
        self.inner.status.store(STATUS_RUNNING, Ordering::SeqCst);
    }

    /// Stops the timer, like `Stop`.
    pub fn stop(&self) {
        self.inner.status.store(STATUS_STOPPED, Ordering::SeqCst);
    }

    /// Closes the timer and stops its loop, like `Close`.
    pub fn close(&self) {
        self.inner.status.store(STATUS_CLOSED, Ordering::SeqCst);
    }

    /// Returns a cheap clone of this timer.
    fn handle(&self) -> Timer {
        Timer {
            inner: Arc::clone(&self.inner),
        }
    }

    /// Creates and queues an entry, like `createEntry`.
    fn create_entry(
        &self,
        interval: Duration,
        job: JobFunc,
        singleton: bool,
        times: i64,
        status: i32,
    ) -> Entry {
        let infinite = times <= 0;
        let mut interval_ticks =
            (interval.as_millis() as i64) / (self.inner.options.interval.as_millis() as i64);
        if interval_ticks == 0 {
            interval_ticks = 1;
        }
        let next_ticks = if self.inner.options.quick {
            self.inner.ticks.load(Ordering::SeqCst)
        } else {
            self.inner.ticks.load(Ordering::SeqCst) + interval_ticks
        };
        let state = Arc::new(EntryState {
            job,
            ticks: interval_ticks,
            times: AtomicI64::new(times),
            status: AtomicI32::new(status),
            singleton: AtomicBool::new(singleton),
            next_ticks: AtomicI64::new(next_ticks),
            infinite: AtomicBool::new(infinite),
            timer: Arc::downgrade(&self.inner),
        });
        self.push(Arc::clone(&state), next_ticks);
        Entry { state }
    }

    /// Pushes an entry and refreshes the cached next priority.
    fn push(&self, entry: Arc<EntryState>, priority: i64) {
        let mut queue = self
            .inner
            .queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let sequence = queue.len() as u64 + self.inner.ticks.load(Ordering::SeqCst) as u64;
        queue.push(QueueItem {
            next_ticks: priority,
            sequence,
            entry,
        });
        if priority < self.inner.next_priority.load(Ordering::SeqCst) {
            self.inner.next_priority.store(priority, Ordering::SeqCst);
        }
    }

    /// Proceeds the due entries, like `proceed`.
    fn proceed(&self, current_ticks: i64) {
        loop {
            let item = {
                let mut queue = self
                    .inner
                    .queue
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                let item = queue.pop();
                let next = queue.peek().map(|item| item.next_ticks).unwrap_or(i64::MAX);
                self.inner.next_priority.store(next, Ordering::SeqCst);
                item
            };
            let Some(item) = item else {
                break;
            };
            let entry = item.entry;
            let next_ticks = entry.next_ticks.load(Ordering::SeqCst);
            if current_ticks < next_ticks {
                self.push(entry, next_ticks);
                break;
            }
            entry.check_and_run(current_ticks);
            if entry.status() != STATUS_CLOSED {
                self.push(Arc::clone(&entry), entry.next_ticks.load(Ordering::SeqCst));
            }
        }
    }
}

/// The loop thread, like Go's `loop`.
fn loop_thread(weak: Weak<TimerInner>) {
    loop {
        let Some(inner) = weak.upgrade() else {
            break;
        };
        std::thread::sleep(inner.options.interval);
        match inner.status.load(Ordering::SeqCst) {
            STATUS_RUNNING => {
                let current = inner.ticks.fetch_add(1, Ordering::SeqCst) + 1;
                if current >= inner.next_priority.load(Ordering::SeqCst) {
                    Timer {
                        inner: Arc::clone(&inner),
                    }
                    .proceed(current);
                }
            }
            STATUS_STOPPED => {}
            STATUS_CLOSED => break,
            _ => {}
        }
    }
}

/// One timing job, like Go's `Entry`.
#[derive(Clone)]
pub struct Entry {
    state: Arc<EntryState>,
}

impl Entry {
    /// Returns the job status, like `Status`.
    pub fn status(&self) -> i32 {
        self.state.status()
    }

    /// Runs the job asynchronously, like `Run`.
    pub fn run(&self) {
        self.state.run();
    }

    /// Sets the job status, returning the previous one, like `SetStatus`.
    pub fn set_status(&self, status: i32) -> i32 {
        self.state.set_status(status)
    }

    /// Starts the job, like `Start`.
    pub fn start(&self) {
        self.state.set_status(STATUS_READY);
    }

    /// Stops the job, like `Stop`.
    pub fn stop(&self) {
        self.state.set_status(STATUS_STOPPED);
    }

    /// Closes the job, like `Close`.
    pub fn close(&self) {
        self.state.set_status(STATUS_CLOSED);
    }

    /// Resets the next run tick, like `Reset`.
    pub fn reset(&self) {
        if let Some(timer) = self.state.timer.upgrade() {
            self.state.next_ticks.store(
                timer.ticks.load(Ordering::SeqCst) + self.state.ticks,
                Ordering::SeqCst,
            );
        }
    }

    /// Reports whether the job runs in singleton mode, like `IsSingleton`.
    pub fn is_singleton(&self) -> bool {
        self.state.singleton.load(Ordering::SeqCst)
    }

    /// Sets the singleton mode, like `SetSingleton`.
    pub fn set_singleton(&self, enabled: bool) {
        self.state.singleton.store(enabled, Ordering::SeqCst);
    }

    /// Returns the job function, like `Job`.
    pub fn job(&self) -> JobFunc {
        Arc::clone(&self.state.job)
    }

    /// Sets the running times limit, like `SetTimes`.
    pub fn set_times(&self, times: i64) {
        self.state.times.store(times, Ordering::SeqCst);
        self.state.infinite.store(false, Ordering::SeqCst);
    }
}

/// Asks the running job to close itself, like `Exit`.
///
/// Go panics with an internal sentinel; the port marks the current entry
/// closed instead, which the timer removes on its next pass.
pub fn exit() {
    CURRENT_ENTRY.with(|current| {
        if let Some(entry) = current.borrow().as_ref() {
            entry.set_status(STATUS_CLOSED);
        }
    });
}

/// The package default timer, like Go's `defaultTimer`.
fn default_timer() -> &'static Timer {
    static DEFAULT: OnceLock<Timer> = OnceLock::new();
    DEFAULT.get_or_init(|| Timer::new(None))
}

/// Runs `job` once after `delay` on the default timer, like `SetTimeout`.
pub fn set_timeout(delay: Duration, job: JobFunc) {
    default_timer().add_once(delay, job);
}

/// Runs `job` every `interval` on the default timer, like `SetInterval`.
pub fn set_interval(interval: Duration, job: JobFunc) {
    default_timer().add(interval, job);
}

/// Adds a job to the default timer, like `Add`.
pub fn add(interval: Duration, job: JobFunc) -> Entry {
    default_timer().add(interval, job)
}

/// Adds a detailed job to the default timer, like `AddEntry`.
pub fn add_entry(
    interval: Duration,
    job: JobFunc,
    singleton: bool,
    times: i64,
    status: i32,
) -> Entry {
    default_timer().add_entry(interval, job, singleton, times, status)
}

/// Adds a singleton job to the default timer, like `AddSingleton`.
pub fn add_singleton(interval: Duration, job: JobFunc) -> Entry {
    default_timer().add_singleton(interval, job)
}

/// Adds a one-shot job to the default timer, like `AddOnce`.
pub fn add_once(interval: Duration, job: JobFunc) -> Entry {
    default_timer().add_once(interval, job)
}

/// Adds a times-limited job to the default timer, like `AddTimes`.
pub fn add_times(interval: Duration, times: i64, job: JobFunc) -> Entry {
    default_timer().add_times(interval, times, job)
}

/// Adds a job after `delay` on the default timer, like `DelayAdd`.
pub fn delay_add(delay: Duration, interval: Duration, job: JobFunc) {
    default_timer().delay_add(delay, interval, job);
}

/// Adds a detailed job after `delay`, like `DelayAddEntry`.
pub fn delay_add_entry(
    delay: Duration,
    interval: Duration,
    job: JobFunc,
    singleton: bool,
    times: i64,
    status: i32,
) {
    default_timer().delay_add_entry(delay, interval, job, singleton, times, status);
}

/// Adds a singleton job after `delay`, like `DelayAddSingleton`.
pub fn delay_add_singleton(delay: Duration, interval: Duration, job: JobFunc) {
    default_timer().delay_add_singleton(delay, interval, job);
}

/// Adds a one-shot job after `delay`, like `DelayAddOnce`.
pub fn delay_add_once(delay: Duration, interval: Duration, job: JobFunc) {
    default_timer().delay_add_once(delay, interval, job);
}

/// Adds a times-limited job after `delay`, like `DelayAddTimes`.
pub fn delay_add_times(delay: Duration, interval: Duration, times: i64, job: JobFunc) {
    default_timer().delay_add_times(delay, interval, times, job);
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicI64, AtomicUsize};

    use super::*;

    /// Waits until `condition` holds or the deadline passes.
    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            if condition() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    fn fast_timer() -> Timer {
        Timer::new(Some(TimerOptions {
            interval: Duration::from_millis(20),
            quick: false,
        }))
    }

    // Translated from Test_AddOnce.
    #[test]
    fn add_once_runs_one_time() {
        let timer = fast_timer();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        timer.add_once(
            Duration::from_millis(40),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) == 1));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(count.load(Ordering::SeqCst), 1, "runs only once");
        timer.close();
    }

    // Translated from Test_Add.
    #[test]
    fn add_repeats_until_closed() {
        let timer = fast_timer();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        let entry = timer.add(
            Duration::from_millis(40),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) >= 3));
        entry.close();
        std::thread::sleep(Duration::from_millis(150));
        let after = count.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(count.load(Ordering::SeqCst), after, "no runs after close");
        timer.close();
    }

    // Translated from Test_AddTimes.
    #[test]
    fn add_times_limits_the_runs() {
        let timer = fast_timer();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        timer.add_times(
            Duration::from_millis(30),
            3,
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) == 3));
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(count.load(Ordering::SeqCst), 3);
        timer.close();
    }

    // Translated from Test_Entry_Stop_Start.
    #[test]
    fn entry_stop_and_start() {
        let timer = fast_timer();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        let entry = timer.add(
            Duration::from_millis(30),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) >= 1));
        entry.stop();
        assert_eq!(entry.status(), STATUS_STOPPED);
        std::thread::sleep(Duration::from_millis(120));
        let stopped = count.load(Ordering::SeqCst);
        entry.start();
        assert!(wait_until(|| count.load(Ordering::SeqCst) > stopped));
        entry.close();
        timer.close();
    }

    // Translated from Test_Add_Singleton: the job never overlaps itself.
    #[test]
    fn singleton_jobs_do_not_overlap() {
        let timer = fast_timer();
        let running = Arc::new(AtomicUsize::new(0));
        let maximum = Arc::new(AtomicUsize::new(0));
        let runs = Arc::new(AtomicI64::new(0));
        let running_job = Arc::clone(&running);
        let maximum_job = Arc::clone(&maximum);
        let runs_job = Arc::clone(&runs);
        let entry = timer.add_singleton(
            Duration::from_millis(20),
            Arc::new(move || {
                let current = running_job.fetch_add(1, Ordering::SeqCst) + 1;
                maximum_job.fetch_max(current, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(80));
                running_job.fetch_sub(1, Ordering::SeqCst);
                runs_job.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| runs.load(Ordering::SeqCst) >= 2));
        assert_eq!(
            maximum.load(Ordering::SeqCst),
            1,
            "singleton jobs must not overlap"
        );
        entry.close();
        timer.close();
    }

    // Translated from Test_DelayAddOnce.
    #[test]
    fn delay_add_runs_after_the_delay() {
        let timer = fast_timer();
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        timer.delay_add_once(
            Duration::from_millis(120),
            Duration::from_millis(20),
            Arc::new(move || {
                flag.store(true, Ordering::SeqCst);
            }),
        );
        std::thread::sleep(Duration::from_millis(40));
        assert!(!ran.load(Ordering::SeqCst), "runs after the delay");
        assert!(wait_until(|| ran.load(Ordering::SeqCst)));
        timer.close();
    }

    // Translated from the gtimer Exit tests.
    #[test]
    fn exit_closes_the_running_entry() {
        let timer = fast_timer();
        let runs = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&runs);
        timer.add(
            Duration::from_millis(20),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                exit();
            }),
        );
        assert!(wait_until(|| runs.load(Ordering::SeqCst) == 1));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(
            runs.load(Ordering::SeqCst),
            1,
            "exit removes the entry after one run"
        );
        timer.close();
    }

    // Translated from Test_Timer_Stop_Start.
    #[test]
    fn timer_stop_and_start() {
        let timer = fast_timer();
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        timer.add(
            Duration::from_millis(20),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) >= 1));
        timer.stop();
        std::thread::sleep(Duration::from_millis(80));
        let stopped = count.load(Ordering::SeqCst);
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(
            count.load(Ordering::SeqCst),
            stopped,
            "stopped timer runs nothing"
        );
        timer.start();
        assert!(wait_until(|| count.load(Ordering::SeqCst) > stopped));
        timer.close();
    }

    #[test]
    fn quick_option_runs_within_one_tick() {
        let timer = Timer::new(Some(TimerOptions {
            interval: Duration::from_millis(20),
            quick: true,
        }));
        let ran = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        timer.add_once(
            Duration::from_secs(10),
            Arc::new(move || {
                flag.store(true, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| ran.load(Ordering::SeqCst)));
        timer.close();
    }

    // Translated from Test_SetTimeout / SetInterval on the default timer.
    #[test]
    fn default_timer_functions() {
        let count = Arc::new(AtomicI64::new(0));
        let counter = Arc::clone(&count);
        set_timeout(
            Duration::from_millis(30),
            Arc::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| count.load(Ordering::SeqCst) == 1));
        let interval_count = Arc::new(AtomicI64::new(0));
        let interval_counter = Arc::clone(&interval_count);
        let interval_entry = default_timer().add(
            Duration::from_millis(30),
            Arc::new(move || {
                interval_counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        assert!(wait_until(|| interval_count.load(Ordering::SeqCst) >= 2));
        interval_entry.close();
        let entry = add_entry(
            Duration::from_millis(100),
            Arc::new(|| {}),
            false,
            1,
            STATUS_READY,
        );
        entry.close();
        let entry = add_singleton(Duration::from_millis(100), Arc::new(|| {}));
        entry.close();
        let entry = add_times(Duration::from_millis(100), 1, Arc::new(|| {}));
        entry.close();
        let entry = add(Duration::from_millis(100), Arc::new(|| {}));
        entry.close();
        delay_add(
            Duration::from_millis(50),
            Duration::from_millis(100),
            Arc::new(|| {}),
        );
        delay_add_singleton(
            Duration::from_millis(50),
            Duration::from_millis(100),
            Arc::new(|| {}),
        );
        delay_add_times(
            Duration::from_millis(50),
            Duration::from_millis(100),
            1,
            Arc::new(|| {}),
        );
    }
}
