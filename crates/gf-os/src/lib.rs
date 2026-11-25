//! The port of GoFrame's `os/` packages.
//!
//! Modules mirror the Go package paths: `os/gtime` becomes [`gtime`],
//! `os/genv` becomes [`genv`], and so on.

pub mod gbuild;
pub mod gcache;
pub mod gcfg;
pub mod gcmd;
pub mod gcron;
pub mod genv;
pub mod gfile;
pub mod gfpool;
pub mod gfsnotify;
pub mod glog;
pub mod gmetric;
pub mod gmlock;
pub mod gmutex;
pub mod gproc;
pub mod gres;
pub mod grpool;
pub mod gsession;
pub mod gspath;
pub mod gtime;
pub mod gtimer;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Serializes tests that replace the process-wide command-line state.
    pub fn command_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}
