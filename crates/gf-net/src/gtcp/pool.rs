//! Port of the `net/gtcp` connection pool (`gtcp_pool.go`, `gtcp_pool_pkg.go`).
//!
//! Go's `PoolConn` borrows a connection from a per-address `gpool.Pool` and
//! hands it back on `Close`. The pool stores untyped `any` values, so this
//! port keeps a typed pool per address (ADR-064), which also allows pruning
//! expired connections by `Instant`.
//!
//! Divergences:
//!
//! - [`PoolConn::close`] consumes the connection (Go's `Close` leaves the
//!   object unusable and relies on the caller);
//! - methods take `&mut self` because the retry path can replace the
//!   underlying connection;
//! - the pool TTL defaults to ten seconds like Go and is not configurable
//!   through the public API (use [`pool_ttl`] to inspect it).

use std::collections::HashMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use gf_errors::gerror::Error;

use super::{Conn, PkgOption, Retry};

/// The default connection TTL in the pool, like Go's `defaultPoolExpire`.
pub const DEFAULT_POOL_EXPIRE: Duration = Duration::from_secs(10);

/// The connection status: unknown/closed, like `connStatusUnknown`.
pub const CONN_STATUS_UNKNOWN: i32 = 0;
/// The connection status: active, like `connStatusActive`.
pub const CONN_STATUS_ACTIVE: i32 = 1;
/// The connection status: failed, like `connStatusError`.
pub const CONN_STATUS_ERROR: i32 = 2;

/// One address pool.
struct PoolState {
    address: String,
    timeout: Option<Duration>,
    ttl: Duration,
    connections: Mutex<VecDeque<(Conn, Instant)>>,
}

impl PoolState {
    fn new(address: &str, timeout: Option<Duration>, ttl: Duration) -> PoolState {
        PoolState {
            address: address.to_owned(),
            timeout,
            ttl,
            connections: Mutex::new(VecDeque::new()),
        }
    }

    /// Borrows a connection, creating one when the pool is empty or expired.
    fn get(&self) -> Result<Conn, Error> {
        {
            let mut connections = self
                .connections
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            while let Some((conn, added)) = connections.pop_front() {
                if added.elapsed() <= self.ttl {
                    return Ok(conn);
                }
                let _ = conn.close();
            }
        }
        Conn::new(&self.address, self.timeout)
    }

    /// Returns a connection to the pool.
    fn put(&self, conn: Conn) {
        self.connections
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push_back((conn, Instant::now()));
    }
}

/// The process-wide address-to-pool registry, like `addressPoolMap`.
fn address_pools() -> &'static Mutex<HashMap<String, Arc<PoolState>>> {
    static POOLS: OnceLock<Mutex<HashMap<String, Arc<PoolState>>>> = OnceLock::new();
    POOLS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the pool TTL used by the port, like `defaultPoolExpire`.
pub fn pool_ttl() -> Duration {
    DEFAULT_POOL_EXPIRE
}

/// Returns (or creates) the pool for `address`.
fn address_pool(address: &str, timeout: Option<Duration>) -> Arc<PoolState> {
    let mut pools = address_pools()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(
        pools
            .entry(address.to_owned())
            .or_insert_with(|| Arc::new(PoolState::new(address, timeout, DEFAULT_POOL_EXPIRE))),
    )
}

/// A TCP connection with pool support, like Go's `PoolConn`.
///
/// It is a connection object, not a pool: `close` returns it to the pool.
pub struct PoolConn {
    conn: Conn,
    pool: Arc<PoolState>,
    status: i32,
}

impl PoolConn {
    /// Borrows a pooled connection to `address`, like `NewPoolConn`.
    pub fn new(address: &str, timeout: Option<Duration>) -> Result<PoolConn, Error> {
        let pool = address_pool(address, timeout);
        let conn = pool.get()?;
        Ok(PoolConn {
            conn,
            pool,
            status: CONN_STATUS_ACTIVE,
        })
    }

    /// The underlying connection.
    pub fn conn(&self) -> &Conn {
        &self.conn
    }

    /// The current status.
    pub fn status(&self) -> i32 {
        self.status
    }

    /// Sets the status (for tests and callers that know better).
    pub fn set_status(&mut self, status: i32) {
        self.status = status;
    }

    /// Returns the connection to the pool when it is active, otherwise closes
    /// it, like Go's `Close`.
    pub fn close(self) -> Result<(), Error> {
        if self.status == CONN_STATUS_ACTIVE {
            self.pool.put(self.conn);
            Ok(())
        } else {
            self.conn.close()
        }
    }

    /// Sends data, re-acquiring a connection when a previously closed one
    /// fails, like Go's `Send`.
    pub fn send(&mut self, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
        let mut result = self.conn.send(data, retry);
        if result.is_err() && self.status == CONN_STATUS_UNKNOWN {
            match self.pool.get() {
                Ok(conn) => {
                    self.conn = conn;
                    result = self.conn.send(data, retry);
                }
                Err(error) => result = Err(error),
            }
        }
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives data, like Go's `Recv`.
    pub fn recv(&mut self, length: i32, retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv(length, retry);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives until `\n`, like Go's `RecvLine`.
    pub fn recv_line(&mut self, retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv_line(retry);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives until `til`, like Go's `RecvTill`.
    pub fn recv_till(&mut self, til: &[u8], retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv_till(til, retry);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives with a timeout, like Go's `RecvWithTimeout`.
    pub fn recv_with_timeout(
        &mut self,
        length: i32,
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv_with_timeout(length, timeout, retry);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Sends with a timeout, like Go's `SendWithTimeout`.
    pub fn send_with_timeout(
        &mut self,
        data: &[u8],
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<(), Error> {
        let result = self.conn.send_with_timeout(data, timeout, retry);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Sends and receives, like Go's `SendRecv`.
    pub fn send_recv(
        &mut self,
        data: &[u8],
        receive: i32,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.send(data, retry)?;
        self.recv(receive, retry)
    }

    /// Sends and receives with a timeout, like Go's
    /// `SendRecvWithTimeout`.
    pub fn send_recv_with_timeout(
        &mut self,
        data: &[u8],
        receive: i32,
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.send(data, retry)?;
        self.recv_with_timeout(receive, timeout, retry)
    }

    /// Sends one package, like Go's `SendPkg`.
    pub fn send_pkg(&mut self, data: &[u8], option: Option<PkgOption>) -> Result<(), Error> {
        let mut result = self.conn.send_pkg(data, option.clone());
        if result.is_err() && self.status == CONN_STATUS_UNKNOWN {
            match self.pool.get() {
                Ok(conn) => {
                    self.conn = conn;
                    result = self.conn.send_pkg(data, option);
                }
                Err(error) => result = Err(error),
            }
        }
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives one package, like Go's `RecvPkg`.
    pub fn recv_pkg(&mut self, option: Option<PkgOption>) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv_pkg(option);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Receives one package with a timeout, like Go's
    /// `RecvPkgWithTimeout`.
    pub fn recv_pkg_with_timeout(
        &mut self,
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<Vec<u8>, Error> {
        let result = self.conn.recv_pkg_with_timeout(timeout, option);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Sends one package with a timeout, like Go's
    /// `SendPkgWithTimeout`.
    pub fn send_pkg_with_timeout(
        &mut self,
        data: &[u8],
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<(), Error> {
        let result = self.conn.send_pkg_with_timeout(data, timeout, option);
        self.status = if result.is_ok() {
            CONN_STATUS_ACTIVE
        } else {
            CONN_STATUS_ERROR
        };
        result
    }

    /// Sends and receives one package, like Go's `SendRecvPkg`.
    pub fn send_recv_pkg(
        &mut self,
        data: &[u8],
        option: Option<PkgOption>,
    ) -> Result<Vec<u8>, Error> {
        self.send_pkg(data, option.clone())?;
        self.recv_pkg(option)
    }

    /// Sends and receives one package with a timeout, like Go's
    /// `SendRecvPkgWithTimeout`.
    pub fn send_recv_pkg_with_timeout(
        &mut self,
        data: &[u8],
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<Vec<u8>, Error> {
        self.send_pkg(data, option.clone())?;
        self.recv_pkg_with_timeout(timeout, option)
    }
}

/// Ensures the error type is referenced for the trait/doc surface.
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::gtcp::{self, Server, ServerHandler};

    fn start_server(handler: ServerHandler) -> Server {
        let server = Server::new(gtcp::FREE_PORT_ADDRESS, Some(handler), None);
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        server
    }

    fn echo_handler() -> ServerHandler {
        Arc::new(|conn: &Conn| {
            while let Ok(data) = conn.recv(-1, None) {
                if data.is_empty() {
                    continue;
                }
                if conn.send(&data, None).is_err() {
                    break;
                }
            }
        })
    }

    fn client(address: &str) -> PoolConn {
        let conn = PoolConn::new(address, Some(Duration::from_secs(5))).expect("pool conn");
        conn.conn()
            .set_deadline(Some(Duration::from_secs(5)))
            .expect("deadline");
        conn
    }

    // Translated from Test_Pool_Basic1 / Test_Pool_Basic2.
    #[test]
    fn pool_conn_reuses_connections() {
        let server = start_server(echo_handler());
        let address = server.listened_address();
        let mut conn = client(&address);
        conn.send(b"hello", None).expect("send");
        assert_eq!(conn.recv(5, None).expect("recv"), b"hello");
        assert_eq!(conn.send_recv(b"world", -1, None).expect("recv"), b"world");
        let local = conn.conn().local_addr().expect("local");
        conn.close().expect("close");

        let conn = client(&address);
        assert_eq!(
            conn.conn().local_addr().expect("local").port(),
            local.port(),
            "closed connection goes back to the pool"
        );
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from Test_Pool_RecvLine / Test_Pool_RecvTill.
    #[test]
    fn pool_conn_line_and_till() {
        let server = start_server(Arc::new(|conn: &Conn| {
            let _ = conn.send(b"line1\nline2", None);
        }));
        let mut conn = client(&server.listened_address());
        assert_eq!(conn.recv_line(None).expect("line"), b"line1");
        assert_eq!(conn.recv_till(b"2", None).expect("till"), b"line2");
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from Test_Pool_Package_Basic.
    #[test]
    fn pool_conn_packages() {
        let server = start_server(Arc::new(|conn: &Conn| {
            while let Ok(data) = conn.recv_pkg(None) {
                if conn.send_pkg(&data, None).is_err() {
                    break;
                }
            }
        }));
        let mut conn = client(&server.listened_address());
        for index in 0..5 {
            let data = index.to_string().into_bytes();
            assert_eq!(conn.send_recv_pkg(&data, None).expect("pkg"), data);
        }
        assert_eq!(
            conn.send_recv_pkg_with_timeout(b"7", Duration::from_secs(2), None)
                .expect("pkg timeout"),
            b"7"
        );
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from Test_Pool_SendWithTimeout /
    // Test_Pool_RecvWithTimeout / Test_Pool_SendRecvWithTimeout.
    #[test]
    fn pool_conn_timeouts_succeed() {
        let server = start_server(echo_handler());
        let mut conn = client(&server.listened_address());
        conn.send_with_timeout(b"hello", Duration::from_secs(2), None)
            .expect("send timeout");
        assert_eq!(
            conn.recv_with_timeout(5, Duration::from_secs(2), None)
                .expect("recv timeout"),
            b"hello"
        );
        assert_eq!(
            conn.send_recv_with_timeout(b"hi", -1, Duration::from_secs(2), None)
                .expect("send recv timeout"),
            b"hi"
        );
        conn.close().expect("close");
        server.close().expect("stop");
    }

    #[test]
    fn pool_errors_and_ttl() {
        assert_eq!(pool_ttl(), DEFAULT_POOL_EXPIRE);
        assert_eq!(CONN_STATUS_UNKNOWN, 0);
        assert_eq!(CONN_STATUS_ACTIVE, 1);
        assert_eq!(CONN_STATUS_ERROR, 2);
        assert!(PoolConn::new("127.0.0.1:1", Some(Duration::from_millis(100))).is_err());
    }
}
