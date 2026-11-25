//! A SQLite connection pool for `database/gdb`.
//!
//! GoFrame's `gdb` keeps an internal link pool (`MaxIdle`/`MaxOpen`/
//! `MaxLifetime`). The port's [`crate::gdb_exec::Db`] owns one connection, so
//! this module adds a small blocking pool (ADR-096):
//!
//! - [`DbPool::open`]/[`DbPool::open_with_size`] create `size` connections to
//!   the same database file;
//! - [`DbPool::get`] blocks until a connection is free and returns a
//!   [`PooledDb`] guard that returns it on drop; [`DbPool::try_get`] is the
//!   non-blocking form;
//! - `size`/`idle`/`path` report the pool state.
//!
//! Not ported: connection lifetime/max-idle reaping, health checks, async
//! acquisition and MySQL/PostgreSQL backends. An in-memory pool gives each
//! slot its own database, so use a file path when the pool must share state.

use std::ops::{Deref, DerefMut};
use std::sync::{Condvar, Mutex};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

use crate::gdb_exec::Db;

/// The default pool size, matching GoFrame's `MaxIdle` default of 10 rounded to
/// a practical small pool.
pub const DEFAULT_POOL_SIZE: usize = 4;

/// A blocking pool of SQLite connections to one database.
pub struct DbPool {
    connections: Mutex<Vec<Db>>,
    available: Condvar,
    size: usize,
    path: String,
}

impl DbPool {
    /// Opens a pool of [`DEFAULT_POOL_SIZE`] connections to `path`.
    pub fn open(path: &str) -> Result<DbPool, Error> {
        DbPool::open_with_size(path, DEFAULT_POOL_SIZE)
    }

    /// Opens a pool of `size` connections to `path`.
    pub fn open_with_size(path: &str, size: usize) -> Result<DbPool, Error> {
        if size == 0 {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["connection pool size must be greater than zero"],
            ));
        }
        let mut connections = Vec::with_capacity(size);
        for _ in 0..size {
            connections.push(Db::open(path)?);
        }
        Ok(DbPool {
            connections: Mutex::new(connections),
            available: Condvar::new(),
            size,
            path: path.to_owned(),
        })
    }

    /// The database path.
    pub fn path(&self) -> &str {
        &self.path
    }

    /// The configured pool size.
    pub fn size(&self) -> usize {
        self.size
    }

    /// The number of idle connections.
    pub fn idle(&self) -> usize {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }

    /// Acquires a connection, blocking until one is free.
    pub fn get(&self) -> PooledDb<'_> {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        while connections.is_empty() {
            connections = self
                .available
                .wait(connections)
                .unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        let connection = connections
            .pop()
            .expect("a pooled connection is available after the wait");
        PooledDb {
            pool: self,
            connection: Some(connection),
        }
    }

    /// Acquires a connection without blocking; `None` when the pool is empty.
    pub fn try_get(&self) -> Option<PooledDb<'_>> {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        connections.pop().map(|connection| PooledDb {
            pool: self,
            connection: Some(connection),
        })
    }

    /// Returns a connection to the pool.
    fn release(&self, connection: Db) {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        connections.push(connection);
        self.available.notify_one();
    }
}

/// A pool guard that dereferences to [`Db`] and returns the connection on
/// drop.
pub struct PooledDb<'a> {
    pool: &'a DbPool,
    connection: Option<Db>,
}

impl PooledDb<'_> {
    /// The pooled connection.
    pub fn db(&self) -> &Db {
        self.connection
            .as_ref()
            .expect("pooled connection is present until drop")
    }
}

impl Deref for PooledDb<'_> {
    type Target = Db;

    fn deref(&self) -> &Self::Target {
        self.db()
    }
}

impl DerefMut for PooledDb<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection
            .as_mut()
            .expect("pooled connection is present until drop")
    }
}

impl Drop for PooledDb<'_> {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            self.pool.release(connection);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::gdb_builder::Builder;

    fn database_path(name: &str) -> String {
        let path = std::env::temp_dir().join(format!("gf-dbpool-{name}-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&path);
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn pool_hands_out_and_reclaims_connections() {
        let path = database_path("reclaim");
        let pool = DbPool::open_with_size(&path, 2).expect("pool");
        assert_eq!(pool.size(), 2);
        assert_eq!(pool.idle(), 2);
        assert_eq!(pool.path(), path);
        {
            let first = pool.get();
            assert_eq!(pool.idle(), 1);
            let second = pool.get();
            assert_eq!(pool.idle(), 0);
            assert!(pool.try_get().is_none());
            first
                .execute("CREATE TABLE t (n INTEGER)", &[])
                .expect("create");
            second
                .execute("INSERT INTO t VALUES (1)", &[])
                .expect("insert");
        }
        assert_eq!(pool.idle(), 2);
        let connection = pool.get();
        assert_eq!(
            connection
                .scalar("SELECT COUNT(*) FROM t", &[])
                .expect("count"),
            gf_core::Value::Int(1)
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pool_blocks_until_a_connection_is_returned() {
        let path = database_path("block");
        let pool = DbPool::open_with_size(&path, 1).expect("pool");
        let held = pool.get();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                std::thread::sleep(Duration::from_millis(50));
                drop(held);
            });
            let connection = pool.get();
            assert_eq!(
                connection.scalar("SELECT 1+1", &[]).expect("query"),
                gf_core::Value::Int(2)
            );
        });
        assert_eq!(pool.idle(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pooled_connections_share_the_file_database() {
        let path = database_path("shared");
        let pool = DbPool::open_with_size(&path, 2).expect("pool");
        pool.get()
            .execute("CREATE TABLE user (id INTEGER PRIMARY KEY, name TEXT)", &[])
            .expect("create");
        pool.get()
            .insert(
                "user",
                &[("name".to_owned(), gf_core::Value::String("john".to_owned()))],
            )
            .expect("insert");
        let rows = pool.get().select(&Builder::new("user")).expect("select");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows.as_slice()[0].string("name"), "john");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn zero_size_and_missing_directories_are_errors() {
        let path = database_path("errors");
        assert!(DbPool::open_with_size(&path, 0).is_err());
        assert!(DbPool::open("/definitely/missing/dir/db.sqlite").is_err());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drop_sends_the_connection_back_to_the_pool() {
        let path = database_path("drop");
        let pool = DbPool::open_with_size(&path, 1).expect("pool");
        {
            let connection = pool.get();
            connection
                .execute("CREATE TABLE t (n INTEGER)", &[])
                .expect("create");
        }
        assert_eq!(pool.idle(), 1);
        let (sender, receiver) = mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(|| {
                let _connection = pool.get();
                sender.send(()).expect("send");
            });
            receiver
                .recv_timeout(Duration::from_secs(2))
                .expect("reacquire");
        });
        let _ = std::fs::remove_file(&path);
    }
}
