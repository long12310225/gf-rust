//! Port of `os/gfpool`.
//!
//! Reuses open file handles so that frequent writers (for example the logger)
//! do not reopen the same file for every write.
//!
//! Divergences, recorded in ADR-051:
//!
//! - Go's integer `os.O_*` flag becomes [`FileOptions`] (a small, hashable set
//!   of booleans) plus an optional Unix permission;
//! - Go's signed `time.Duration` ttl becomes [`Ttl`] from `os/gcache`;
//! - the `gfsnotify` watcher that recreates a pool after a rename/remove is not
//!   ported: [`Pool::file`] re-checks the path and reopens when the file was
//!   replaced, which covers rotation without an extra watch;
//! - [`File::close`] returns the handle to the pool, [`File::close_now`]
//!   closes the operating-system handle, replacing `Close(close ...bool)`.

use std::collections::VecDeque;
use std::fs::{File as StdFile, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use gf_errors::gerror::{self, Error};

use crate::gcache::Ttl;

/// The options used to open a pooled file, replacing Go's `os.O_*` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct FileOptions {
    /// Open for reading (`O_RDONLY`).
    pub read: bool,
    /// Open for writing (`O_WRONLY`).
    pub write: bool,
    /// Append on every write (`O_APPEND`).
    pub append: bool,
    /// Create the file when missing (`O_CREATE`).
    pub create: bool,
    /// Truncate the file on open (`O_TRUNC`).
    pub truncate: bool,
}

impl FileOptions {
    /// Read-only options.
    pub fn read_only() -> FileOptions {
        FileOptions {
            read: true,
            ..FileOptions::default()
        }
    }

    /// Write-only, create and append options (the logger's mode).
    pub fn append_create() -> FileOptions {
        FileOptions {
            write: true,
            append: true,
            create: true,
            ..FileOptions::default()
        }
    }

    /// Write-only, create and truncate options.
    pub fn write_truncate() -> FileOptions {
        FileOptions {
            write: true,
            create: true,
            truncate: true,
            ..FileOptions::default()
        }
    }

    fn to_open_options(self) -> OpenOptions {
        let mut options = OpenOptions::new();
        // A file opened for writing is writable even when `read` is false.
        options.read(self.read).write(self.write);
        options.append(self.append);
        options.create(self.create);
        options.truncate(self.truncate);
        options
    }
}

/// One pooled file handle, like Go's `gfpool.File`.
pub struct File {
    inner: Option<StdFile>,
    path: PathBuf,
    pool: Arc<PoolInner>,
    /// Whether the item was returned to its pool already.
    returned: bool,
}

impl File {
    /// The absolute path of this handle.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the handle to its pool, like `Close()`.
    ///
    /// The operating-system handle stays open for the next caller.
    pub fn close(&mut self) {
        if self.returned {
            return;
        }
        self.returned = true;
        if matches!(self.pool.ttl, Ttl::Remove) {
            // `ttl < 0` in Go: expire immediately, so close the real handle.
            self.inner.take();
            return;
        }
        let mut pool = self
            .pool
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pool.push_back(PooledFile {
            file: self.inner.take(),
            last_used: Instant::now(),
        });
    }

    /// Closes the operating-system handle, like `Close(true)`.
    pub fn close_now(&mut self) {
        self.returned = true;
        self.inner.take();
    }

    /// Reports whether the handle can still be used.
    pub fn is_open(&self) -> bool {
        self.inner.is_some()
    }
}

impl Read for File {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self.inner.as_mut() {
            Some(file) => file.read(buffer),
            None => Ok(0),
        }
    }
}

impl Write for File {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        match self.inner.as_mut() {
            Some(file) => file.write(buffer),
            None => Ok(0),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        match self.inner.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

impl Seek for File {
    fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
        match self.inner.as_mut() {
            Some(file) => file.seek(position),
            None => Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "the pooled file handle is closed",
            )),
        }
    }
}

/// A handle waiting in the pool.
struct PooledFile {
    file: Option<StdFile>,
    last_used: Instant,
}

/// The shared state of one pool.
struct PoolInner {
    path: PathBuf,
    options: FileOptions,
    ttl: Ttl,
    free: Mutex<VecDeque<PooledFile>>,
}

impl PoolInner {
    /// Creates a new operating-system handle.
    fn create(&self) -> Result<StdFile, Error> {
        self.options
            .to_open_options()
            .open(&self.path)
            .map_err(|error| {
                gerror::wrap(
                    Some(gerror::new(error.to_string())),
                    format!("open file \"{}\" failed", self.path.display()),
                )
                .unwrap_or_else(|| gerror::new("open file failed"))
            })
    }
}

/// A file pointer pool, like Go's `gfpool.Pool`.
pub struct Pool {
    inner: Arc<PoolInner>,
}

impl Pool {
    /// Creates a pool for `path` with `options` and `ttl`, like `New`.
    pub fn new(path: &str, options: FileOptions, ttl: Ttl) -> Result<Pool, Error> {
        let pool = Pool {
            inner: Arc::new(PoolInner {
                path: PathBuf::from(path),
                options,
                ttl,
                free: Mutex::new(VecDeque::new()),
            }),
        };
        // Fail fast on an unusable path, like Go's `os.OpenFile`.
        let file = pool.inner.create()?;
        pool.inner
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back(PooledFile {
                file: Some(file),
                last_used: Instant::now(),
            });
        Ok(pool)
    }

    /// Retrieves a handle, creating one when the pool is empty, like `File`.
    pub fn file(&self) -> Result<File, Error> {
        let mut pooled = self
            .inner
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .pop_back();
        let expired = pooled
            .as_ref()
            .is_some_and(|pooled| self.is_expired(pooled));
        if expired {
            pooled = None;
        }
        let file = match pooled {
            Some(pooled) => self.redial_if_replaced(pooled.file)?,
            None => self.inner.create()?,
        };
        if self.inner.options.truncate {
            file.set_len(0)
                .map_err(|error| gerror::new(error.to_string()))?;
        }
        let mut file = File {
            inner: Some(file),
            path: self.inner.path.clone(),
            pool: Arc::clone(&self.inner),
            returned: false,
        };
        if self.inner.options.append {
            file.seek(SeekFrom::End(0))
                .map_err(|error| gerror::new(error.to_string()))?;
        } else {
            file.seek(SeekFrom::Start(0))
                .map_err(|error| gerror::new(error.to_string()))?;
        }
        Ok(file)
    }

    /// Closes every handle in the pool, like `Pool.Close`.
    pub fn close(&self) {
        self.inner
            .free
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clear();
    }

    /// Reports whether a pooled handle exceeded its ttl.
    fn is_expired(&self, pooled: &PooledFile) -> bool {
        match self.inner.ttl {
            Ttl::After(duration) => pooled.last_used.elapsed() >= duration,
            Ttl::Remove => true,
            Ttl::Never => false,
        }
    }

    /// Reopens the handle when the path now names another file, which is what
    /// a log rotation looks like from the pool's point of view.
    fn redial_if_replaced(&self, file: Option<StdFile>) -> Result<StdFile, Error> {
        let Some(file) = file else {
            return self.inner.create();
        };
        let replaced = match (file.metadata(), std::fs::metadata(&self.inner.path)) {
            (Ok(opened), Ok(current)) => !same_file(&opened, &current),
            (_, Err(_)) => true,
            _ => false,
        };
        if replaced && self.inner.options.create {
            drop(file);
            return self.inner.create();
        }
        Ok(file)
    }
}

/// Compares two metadata values for the same underlying file.
fn same_file(opened: &std::fs::Metadata, current: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        opened.dev() == current.dev() && opened.ino() == current.ino()
    }
    #[cfg(windows)]
    {
        // Windows has no stable portable file identity through `std`, so the
        // size and modification time are compared instead.
        opened.len() == current.len() && opened.modified().ok() == current.modified().ok()
    }
    #[cfg(not(any(unix, windows)))]
    {
        opened.len() == current.len() && opened.modified().ok() == current.modified().ok()
    }
}

/// The process-wide pools, like Go's `pools` map.
fn pools() -> &'static Mutex<std::collections::HashMap<String, Arc<Pool>>> {
    static POOLS: OnceLock<Mutex<std::collections::HashMap<String, Arc<Pool>>>> = OnceLock::new();
    POOLS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// The pool key, mirroring Go's `path&flag&ttl&perm` string.
fn pool_key(path: &str, options: FileOptions, ttl: Ttl) -> String {
    format!("{path}&{options:?}&{ttl:?}")
}

/// Opens a pooled file, creating the pool on first use, like `Open`.
pub fn open(path: &str, options: FileOptions, ttl: Ttl) -> Result<File, Error> {
    let key = pool_key(path, options, ttl);
    let pool = {
        let mut pools = pools()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(pool) = pools.get(&key) {
            Arc::clone(pool)
        } else {
            let pool = Arc::new(Pool::new(path, options, ttl)?);
            pools.insert(key, Arc::clone(&pool));
            pool
        }
    };
    pool.file()
}

/// Retrieves a file from an existing pool, like `Get`.
///
/// Returns `Ok(None)` when no pool was created for these arguments.
pub fn get(path: &str, options: FileOptions, ttl: Ttl) -> Result<Option<File>, Error> {
    let key = pool_key(path, options, ttl);
    let pool = {
        let pools = pools()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        pools.get(&key).cloned()
    };
    match pool {
        Some(pool) => pool.file().map(Some),
        None => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use crate::gfile::{get_contents, join, mkdir, put_contents, remove_all, temp};

    use super::*;

    fn sandbox(name: &str) -> String {
        let path = temp(&["gf-rust-gfpool-tests", name]);
        let _ = remove_all(&path);
        mkdir(&path).expect("sandbox");
        path
    }

    // Translated from Test_Open and the append behaviour.
    #[test]
    fn open_appends_through_the_pool() {
        let base = sandbox("append");
        let path = join(&[&base, "app.log"]);
        let options = FileOptions::append_create();
        for message in ["one\n", "two\n"] {
            let mut file = open(&path, options, Ttl::Never).expect("open");
            file.write_all(message.as_bytes()).expect("write");
            file.flush().expect("flush");
            file.close();
        }
        assert_eq!(get_contents(&path), "one\ntwo\n");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn closed_handles_are_reused() {
        let base = sandbox("reuse");
        let path = join(&[&base, "reuse.log"]);
        let options = FileOptions::append_create();
        let mut first = open(&path, options, Ttl::Never).expect("open");
        assert!(first.is_open());
        first.close();
        assert_eq!(
            first
                .pool
                .free
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .len(),
            1,
            "a closed handle stays in the pool"
        );
        let second = open(&path, options, Ttl::Never).expect("open");
        assert!(second.is_open());
        remove_all(&base).expect("cleanup");
    }

    // Translated from the `Close(true)` behaviour: the real handle is closed.
    #[test]
    fn close_now_closes_the_handle() {
        let base = sandbox("close");
        let path = join(&[&base, "close.log"]);
        let mut file = open(&path, FileOptions::append_create(), Ttl::Never).expect("open");
        file.write_all(b"data").expect("write");
        file.close_now();
        assert!(!file.is_open());
        assert_eq!(get_contents(&path), "data");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn immediate_ttl_does_not_keep_handles() {
        let base = sandbox("ttl");
        let path = join(&[&base, "ttl.log"]);
        let mut file = open(&path, FileOptions::append_create(), Ttl::Remove).expect("open");
        file.write_all(b"first").expect("write");
        file.flush().expect("flush");
        file.close();
        assert!(!file.is_open());
        let mut second = open(&path, FileOptions::append_create(), Ttl::Remove).expect("open");
        second.write_all(b"-second").expect("write");
        second.flush().expect("flush");
        second.close();
        assert_eq!(get_contents(&path), "first-second");
        remove_all(&base).expect("cleanup");
    }

    // Translated from the rotation handling: a replaced file is reopened.
    #[test]
    fn replaced_file_is_reopened() {
        let base = sandbox("rotate");
        let path = join(&[&base, "rotate.log"]);
        let mut file = open(&path, FileOptions::append_create(), Ttl::Never).expect("open");
        file.write_all(b"old").expect("write");
        file.flush().expect("flush");
        file.close();
        // Simulate rotation: the old file is gone and a new one was created.
        std::fs::remove_file(&path).expect("remove");
        put_contents(&path, "new-").expect("write new");
        let mut reopened = open(&path, FileOptions::append_create(), Ttl::Never).expect("open");
        reopened.write_all(b"content").expect("write");
        reopened.flush().expect("flush");
        reopened.close();
        assert_eq!(get_contents(&path), "new-content");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn truncate_option_empties_the_file() {
        let base = sandbox("truncate");
        let path = join(&[&base, "truncate.log"]);
        put_contents(&path, "old content").expect("write");
        let mut file = open(&path, FileOptions::write_truncate(), Ttl::Never).expect("open");
        file.write_all(b"new").expect("write");
        file.flush().expect("flush");
        file.close();
        assert_eq!(get_contents(&path), "new");
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn get_requires_an_existing_pool() {
        let base = sandbox("get");
        let path = join(&[&base, "missing.log"]);
        assert!(
            get(&path, FileOptions::append_create(), Ttl::Never)
                .expect("get")
                .is_none()
        );
        let _ = open(&path, FileOptions::append_create(), Ttl::Never).expect("open");
        assert!(
            get(&path, FileOptions::append_create(), Ttl::Never)
                .expect("get")
                .is_some()
        );
        remove_all(&base).expect("cleanup");
    }

    #[test]
    fn read_only_options_read_the_file() {
        let base = sandbox("read");
        let path = join(&[&base, "read.log"]);
        put_contents(&path, "hello pool").expect("write");
        let mut file = open(&path, FileOptions::read_only(), Ttl::Never).expect("open");
        let mut content = String::new();
        file.read_to_string(&mut content).expect("read");
        assert_eq!(content, "hello pool");
        file.close();
        remove_all(&base).expect("cleanup");
    }
}
