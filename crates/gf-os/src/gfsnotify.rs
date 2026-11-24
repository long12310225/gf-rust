//! Port of `os/gfsnotify`.
//!
//! Go wraps `github.com/fsnotify/fsnotify` with a callback registry, recursive
//! directory watching and a one-millisecond burst filter. The port uses the
//! `notify` crate for the OS events (ADR-042) and keeps GoFrame's API shape:
//! [`add`]/[`add_once`]/[`remove`]/[`remove_callback`] on a shared default
//! watcher, [`Callback`] handles, [`Op`] bit flags and [`exit`] for a callback
//! that removes itself.
//!
//! Divergences, recorded in ADR-042:
//!
//! - recursive watching uses the OS recursive mode instead of watching every
//!   sub-directory one by one, so one root is one watch;
//! - callbacks run on their own thread like Go's goroutines, so delivery is
//!   asynchronous;
//! - `Event` carries no `Watcher` pointer; the callback closure can capture the
//!   watcher it needs;
//! - [`exit`] is a thread-local flag checked after the callback returns, not a
//!   panic the loop recovers from.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use notify::{RecursiveMode, Watcher as NotifyWatcher};

/// A create event.
pub const CREATE: Op = Op(1 << 0);
/// A write event.
pub const WRITE: Op = Op(1 << 1);
/// A remove event.
pub const REMOVE: Op = Op(1 << 2);
/// A rename event.
pub const RENAME: Op = Op(1 << 3);
/// A permission change event.
pub const CHMOD: Op = Op(1 << 4);

/// Repeated events for the same path and operation within this window are
/// dropped, like GoFrame's `repeatEventFilterDuration`.
const REPEAT_FILTER: Duration = Duration::from_millis(1);

/// The file operations of one event, like Go's `gfsnotify.Op`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Op(u32);

impl Op {
    /// Reports whether every flag of `other` is set.
    pub fn contains(self, other: Op) -> bool {
        self.0 & other.0 == other.0
    }

    /// Reports whether no flag is set.
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The raw bit mask.
    pub fn bits(self) -> u32 {
        self.0
    }
}

impl std::ops::BitOr for Op {
    type Output = Op;

    fn bitor(self, other: Op) -> Op {
        Op(self.0 | other.0)
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut names = Vec::new();
        for (flag, name) in [
            (CREATE, "CREATE"),
            (WRITE, "WRITE"),
            (REMOVE, "REMOVE"),
            (RENAME, "RENAME"),
            (CHMOD, "CHMOD"),
        ] {
            if self.contains(flag) {
                names.push(name);
            }
        }
        formatter.write_str(&names.join("|"))
    }
}

/// One file system change, like Go's `gfsnotify.Event`.
#[derive(Clone, Debug)]
pub struct Event {
    /// Absolute path of the changed file or directory.
    pub path: String,
    /// The operations the OS reported.
    pub op: Op,
}

impl Event {
    /// Reports whether the event contains a create operation.
    pub fn is_create(&self) -> bool {
        self.op.contains(CREATE)
    }

    /// Reports whether the event contains a write operation.
    pub fn is_write(&self) -> bool {
        self.op.contains(WRITE)
    }

    /// Reports whether the event contains a remove operation.
    pub fn is_remove(&self) -> bool {
        self.op.contains(REMOVE)
    }

    /// Reports whether the event contains a rename operation.
    pub fn is_rename(&self) -> bool {
        self.op.contains(RENAME)
    }

    /// Reports whether the event contains a chmod operation.
    pub fn is_chmod(&self) -> bool {
        self.op.contains(CHMOD)
    }
}

impl std::fmt::Display for Event {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} {}", self.op, self.path)
    }
}

/// Per-watching options, like Go's `gfsnotify.WatchOption`.
#[derive(Clone, Copy, Debug, Default)]
pub struct WatchOption {
    /// Watches only `path` itself instead of its subtree.
    pub no_recursive: bool,
}

/// A callback handler, like Go's `func(event *Event)`.
pub type EventHandler = Arc<dyn Fn(&Event) + Send + Sync>;

/// The callback of one watch, like Go's `gfsnotify.Callback`.
pub struct Callback {
    /// Unique identifier.
    pub id: i32,
    /// Absolute watched path.
    pub path: String,
    handler: EventHandler,
    name: String,
    recursive: bool,
}

impl Callback {
    /// The handler this callback runs.
    pub fn invoke(&self, event: &Event) {
        (self.handler)(event);
    }
}

impl std::fmt::Debug for Callback {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Callback")
            .field("id", &self.id)
            .field("path", &self.path)
            .field("recursive", &self.recursive)
            .finish()
    }
}

thread_local! {
    /// Set by [`exit`] and checked after a callback returns.
    static EXIT_REQUESTED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Asks the watcher to remove the calling callback once it returns, like Go's
/// `gfsnotify.Exit`.
pub fn exit() {
    EXIT_REQUESTED.with(|flag| flag.set(true));
}

/// The shared registry behind a watcher; the notification thread only needs
/// this half.
#[derive(Default)]
struct Inner {
    callbacks: Mutex<HashMap<String, Vec<Arc<Callback>>>>,
    ids: Mutex<HashMap<i32, Arc<Callback>>>,
    names: Mutex<HashSet<String>>,
    recent: Mutex<HashMap<String, Instant>>,
    id_generator: AtomicI32,
    closed: AtomicBool,
    owner: OnceLock<Weak<Watcher>>,
}

impl Inner {
    /// Maps a `notify` event onto one or more GoFrame events and dispatches
    /// them, like GoFrame's `watchLoop`.
    fn handle(&self, event: notify::Event) {
        let mut op = Op::default();
        match event.kind {
            notify::EventKind::Create(_) => op = op | CREATE,
            notify::EventKind::Modify(notify::event::ModifyKind::Name(_)) => op = op | RENAME,
            notify::EventKind::Modify(_) => op = op | WRITE,
            notify::EventKind::Remove(_) => op = op | REMOVE,
            notify::EventKind::Access(_) | notify::EventKind::Any | notify::EventKind::Other => {}
        }
        if op.is_empty() {
            return;
        }
        for path in event.paths {
            let path = path.to_string_lossy().into_owned();
            if self.is_repeated(&path, op) {
                continue;
            }
            self.dispatch(&path, op);
        }
    }

    /// Drops identical events that arrive within [`REPEAT_FILTER`].
    fn is_repeated(&self, path: &str, op: Op) -> bool {
        let key = format!("{op} {path}");
        let now = Instant::now();
        let mut recent = self
            .recent
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        recent.retain(|_, at| now.duration_since(*at) < REPEAT_FILTER);
        if recent.contains_key(&key) {
            return true;
        }
        recent.insert(key, now);
        false
    }

    /// Runs every callback that watches `path`, like `getCallbacksForPath`.
    fn dispatch(&self, path: &str, op: Op) {
        let event = Arc::new(Event {
            path: path.to_owned(),
            op,
        });
        for callback in self.callbacks_for(path) {
            let handler_event = Arc::clone(&event);
            let weak = self.owner.get().cloned();
            let id = callback.id;
            let spawned = std::thread::Builder::new()
                .name("gf-fsnotify-callback".to_owned())
                .spawn(move || {
                    callback.invoke(&handler_event);
                    let requested = EXIT_REQUESTED.with(|flag| flag.replace(false));
                    if requested && let Some(watcher) = weak.and_then(|weak| weak.upgrade()) {
                        watcher.remove_callback(id);
                    }
                });
            if let Err(error) = spawned {
                eprintln!("gf-fsnotify: callback thread unavailable: {error}");
            }
        }
    }

    /// Collects the callbacks of `path`: itself, its direct parent, and the
    /// recursive ancestors, exactly like Go.
    fn callbacks_for(&self, path: &str) -> Vec<Arc<Callback>> {
        let callbacks = self
            .callbacks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut found = Vec::new();
        if let Some(list) = callbacks.get(path) {
            found.extend(list.iter().cloned());
        }
        let mut directory = parent_dir(path);
        if let Some(list) = callbacks.get(&directory) {
            found.extend(list.iter().cloned());
        }
        loop {
            let parent = parent_dir(&directory);
            if parent == directory {
                break;
            }
            if let Some(list) = callbacks.get(&parent) {
                for callback in list {
                    if callback.recursive {
                        found.push(Arc::clone(callback));
                    }
                }
            }
            directory = parent;
        }
        found
    }
}

/// The parent directory of `path`, like Go's `filepath.Dir` for an absolute
/// path.
fn parent_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches(std::path::MAIN_SEPARATOR);
    match trimmed.rfind(std::path::MAIN_SEPARATOR) {
        None => trimmed.to_owned(),
        Some(0) => std::path::MAIN_SEPARATOR.to_string(),
        Some(_) if trimmed.len() == 1 => trimmed.to_owned(),
        Some(2) if trimmed.as_bytes().get(1) == Some(&b':') => trimmed[..2].to_owned(),
        Some(index) => trimmed[..index].to_owned(),
    }
}

/// Watches a file tree and runs callbacks on changes, like Go's
/// `gfsnotify.Watcher`.
pub struct Watcher {
    inner: Arc<Inner>,
    watcher: Mutex<notify::RecommendedWatcher>,
}

impl Watcher {
    /// Creates a watcher, like Go's `gfsnotify.New`.
    pub fn new() -> Result<Arc<Watcher>, Error> {
        let inner = Arc::new(Inner::default());
        let handler_inner = Arc::clone(&inner);
        let notify_watcher =
            notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                if let Ok(event) = result {
                    handler_inner.handle(event);
                }
            })
            .map_err(|error| {
                gerror::new_codef(
                    &gcode::CODE_INTERNAL_ERROR,
                    format!("create watcher failed: {error}"),
                    Vec::new(),
                )
            })?;
        let watcher = Arc::new(Watcher {
            inner,
            watcher: Mutex::new(notify_watcher),
        });
        let _ = watcher.inner.owner.set(Arc::downgrade(&watcher));
        Ok(watcher)
    }

    /// Watches `path`, like Go's `Watcher.Add`.
    pub fn add(
        &self,
        path: &str,
        handler: EventHandler,
        option: WatchOption,
    ) -> Result<Option<Arc<Callback>>, Error> {
        self.add_once("", path, handler, option)
    }

    /// Watches `path` once under `name`, like Go's `Watcher.AddOnce`.
    ///
    /// Returns `None` when `name` was already registered; an empty name always
    /// registers.
    pub fn add_once(
        &self,
        name: &str,
        path: &str,
        handler: EventHandler,
        option: WatchOption,
    ) -> Result<Option<Arc<Callback>>, Error> {
        if !name.is_empty() {
            let mut names = self
                .inner
                .names
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if names.contains(name) {
                return Ok(None);
            }
            names.insert(name.to_owned());
        }
        let absolute = crate::gfile::real_path(path);
        if absolute.is_empty() {
            if !name.is_empty() {
                self.inner
                    .names
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(name);
            }
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("\"{path}\" does not exist"),
                Vec::new(),
            ));
        }
        let callback = Arc::new(Callback {
            id: self.inner.id_generator.fetch_add(1, Ordering::SeqCst) + 1,
            path: absolute.clone(),
            handler,
            name: name.to_owned(),
            recursive: !option.no_recursive,
        });
        self.inner
            .callbacks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(absolute.clone())
            .or_default()
            .push(Arc::clone(&callback));
        self.inner
            .ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(callback.id, Arc::clone(&callback));
        let mode = if option.no_recursive {
            RecursiveMode::NonRecursive
        } else {
            RecursiveMode::Recursive
        };
        self.watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .watch(std::path::Path::new(&absolute), mode)
            .map_err(|error| {
                gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("add watch failed for path \"{absolute}\": {error}"),
                    Vec::new(),
                )
            })?;
        Ok(Some(callback))
    }

    /// Stops watching `path` and drops its callbacks, like Go's `Remove`.
    pub fn remove(&self, path: &str) -> Result<(), Error> {
        let absolute = super::gfile::real_path(path);
        let absolute = if absolute.is_empty() {
            path.to_owned()
        } else {
            absolute
        };
        let removed = {
            let mut callbacks = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            callbacks.remove(&absolute).unwrap_or_default()
        };
        {
            let mut ids = self
                .inner
                .ids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let mut names = self
                .inner
                .names
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for callback in &removed {
                ids.remove(&callback.id);
                if !callback.name.is_empty() {
                    names.remove(&callback.name);
                }
            }
        }
        self.watcher
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .unwatch(std::path::Path::new(&absolute))
            .map_err(|error| {
                gerror::new_codef(
                    &gcode::CODE_INVALID_PARAMETER,
                    format!("remove watch failed for path \"{absolute}\": {error}"),
                    Vec::new(),
                )
            })
    }

    /// Removes one callback by id, unwatching the path when it was the last
    /// one, like Go's `RemoveCallback`.
    pub fn remove_callback(&self, callback_id: i32) {
        let callback = {
            let mut ids = self
                .inner
                .ids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            ids.remove(&callback_id)
        };
        let Some(callback) = callback else {
            return;
        };
        if !callback.name.is_empty() {
            self.inner
                .names
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .remove(&callback.name);
        }
        let empty_path = {
            let mut callbacks = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(list) = callbacks.get_mut(&callback.path) else {
                return;
            };
            list.retain(|item| item.id != callback_id);
            if list.is_empty() {
                callbacks.remove(&callback.path);
                true
            } else {
                false
            }
        };
        if empty_path {
            let _ = self
                .watcher
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .unwatch(std::path::Path::new(&callback.path));
        }
    }

    /// Stops the watcher, like Go's `Watcher.Close`.
    pub fn close(&self) {
        if self.inner.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        let paths: Vec<String> = {
            let callbacks = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            callbacks.keys().cloned().collect()
        };
        {
            let mut watcher = self
                .watcher
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for path in paths {
                let _ = watcher.unwatch(std::path::Path::new(&path));
            }
        }
        self.inner
            .callbacks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.inner
            .ids
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
        self.inner
            .names
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }
}

/// The process-wide watcher, created on first use.
fn default_watcher() -> Result<Arc<Watcher>, Error> {
    static DEFAULT: OnceLock<Mutex<Option<Arc<Watcher>>>> = OnceLock::new();
    let slot = DEFAULT.get_or_init(|| Mutex::new(None));
    let mut guard = slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(watcher) = guard.as_ref() {
        return Ok(Arc::clone(watcher));
    }
    let watcher = Watcher::new()?;
    *guard = Some(Arc::clone(&watcher));
    Ok(watcher)
}

/// Watches `path` with the default watcher, like Go's `gfsnotify.Add`.
pub fn add(
    path: &str,
    handler: EventHandler,
    option: WatchOption,
) -> Result<Option<Arc<Callback>>, Error> {
    default_watcher()?.add(path, handler, option)
}

/// Watches `path` once under `name`, like Go's `gfsnotify.AddOnce`.
pub fn add_once(
    name: &str,
    path: &str,
    handler: EventHandler,
    option: WatchOption,
) -> Result<Option<Arc<Callback>>, Error> {
    default_watcher()?.add_once(name, path, handler, option)
}

/// Removes every callback of `path`, like Go's `gfsnotify.Remove`.
pub fn remove(path: &str) -> Result<(), Error> {
    default_watcher()?.remove(path)
}

/// Removes one callback by id, like Go's `gfsnotify.RemoveCallback`.
pub fn remove_callback(callback_id: i32) -> Result<(), Error> {
    default_watcher()?.remove_callback(callback_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use super::*;
    use crate::gfile::{mkdir, put_contents, remove_all, temp};

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfsnotify-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    /// Waits for one event that satisfies `wanted`, for at most five seconds.
    fn wait_for(rx: &mpsc::Receiver<Event>, wanted: impl Fn(&Event) -> bool) -> Option<Event> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(event) if wanted(&event) => return Some(event),
                Ok(_) => {}
                Err(_) => {}
            }
        }
        None
    }

    #[test]
    fn watch_receives_create_and_write_events() {
        let base = sandbox("events");
        let watcher = Watcher::new().expect("watcher");
        let (tx, rx) = mpsc::channel();
        watcher
            .add(
                &base,
                Arc::new(move |event: &Event| {
                    let _ = tx.send(event.clone());
                }),
                WatchOption::default(),
            )
            .expect("add")
            .expect("callback");

        let file = crate::gfile::join(&[&base, "watched.txt"]);
        put_contents(&file, "hello").expect("write");
        let event = wait_for(&rx, |event| event.is_create() || event.is_write())
            .expect("create or write event");
        assert!(event.path.ends_with("watched.txt"), "got {event}");

        crate::gfile::remove_file(&file).expect("delete");
        let event = wait_for(&rx, |event| event.is_remove()).expect("remove event");
        assert!(event.is_remove(), "got {event}");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn add_validates_and_add_once_deduplicates() {
        let base = sandbox("options");
        let watcher = Watcher::new().expect("watcher");
        let error = watcher
            .add(
                &crate::gfile::join(&[&base, "missing"]),
                Arc::new(|_: &Event| {}),
                WatchOption::default(),
            )
            .expect_err("missing path");
        assert!(error.to_string().contains("does not exist"));

        let first = watcher
            .add_once(
                "unique-name",
                &base,
                Arc::new(|_: &Event| {}),
                WatchOption::default(),
            )
            .expect("add once")
            .expect("first callback");
        assert!(first.id > 0);
        let second = watcher
            .add_once(
                "unique-name",
                &base,
                Arc::new(|_: &Event| {}),
                WatchOption::default(),
            )
            .expect("duplicate add once");
        assert!(second.is_none());
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn remove_callback_stops_delivery() {
        let base = sandbox("remove");
        let watcher = Watcher::new().expect("watcher");
        let (tx, rx) = mpsc::channel();
        let callback = watcher
            .add(
                &base,
                Arc::new(move |event: &Event| {
                    let _ = tx.send(event.clone());
                }),
                WatchOption::default(),
            )
            .expect("add")
            .expect("callback");
        watcher.remove_callback(callback.id);

        put_contents(&crate::gfile::join(&[&base, "ignored.txt"]), "x").expect("write");
        assert!(
            wait_for(&rx, |_| true).is_none(),
            "no event after the callback was removed"
        );
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn exit_removes_the_calling_callback() {
        let base = sandbox("exit");
        let watcher = Watcher::new().expect("watcher");
        let callback = watcher
            .add(&base, Arc::new(|_: &Event| exit()), WatchOption::default())
            .expect("add")
            .expect("callback");
        put_contents(&crate::gfile::join(&[&base, "one.txt"]), "x").expect("write");

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let still_registered = watcher
                .inner
                .ids
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains_key(&callback.id);
            if !still_registered {
                break;
            }
            assert!(Instant::now() < deadline, "callback was never removed");
            std::thread::sleep(Duration::from_millis(100));
        }
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn op_flags_render_like_go() {
        assert_eq!((CREATE | WRITE).to_string(), "CREATE|WRITE");
        assert!(CREATE.contains(CREATE));
        assert!(!CREATE.contains(WRITE));
        assert!(Op::default().is_empty());
    }
}
