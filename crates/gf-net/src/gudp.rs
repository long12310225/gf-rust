//! Port of `net/gudp` — UDP client/server connections.
//!
//! The port keeps GoFrame's shape (client connection, server connection,
//! named server registry, retry options, free-port helpers) on top of
//! `std::net::UdpSocket`.
//!
//! Divergences, recorded in ADR-060:
//!
//! - deadlines are relative `Duration`s (std sockets expose timeouts, not
//!   absolute points in time);
//! - `Recv` returns `(data, remote_addr)`; a closed server connection surfaces
//!   as an `Error` whose text is `connection closed`;
//! - the `Server` runs its handler on the calling thread like Go; `run` returns
//!   once the handler returns;
//! - the server sets a 200 ms read timeout so a closing server unblocks its
//!   handler promptly.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// The address that asks the system to assign a free port, like
/// `FreePortAddress`.
pub const FREE_PORT_ADDRESS: &str = ":0";

/// The retry options, like Go's `Retry`.
#[derive(Clone, Copy, Debug)]
pub struct Retry {
    /// Maximum retry count.
    pub count: u32,
    /// Interval between retries.
    pub interval: Duration,
}

impl Retry {
    /// Creates retry options.
    pub fn new(count: u32, interval: Duration) -> Retry {
        Retry { count, interval }
    }
}

/// The default retry interval, like Go's `defaultRetryInterval`.
const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// The default read buffer size, like Go's `defaultReadBufferSize`.
const DEFAULT_READ_BUFFER_SIZE: i32 = 1024;

/// Reports whether `error` is a timeout (not a hard failure).
fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

/// Resolves one address string to a socket address.
fn resolve(address: &str) -> Result<SocketAddr, Error> {
    address
        .to_socket_addrs()
        .map_err(|error| gerror::new(format!("resolve UDP address \"{address}\" failed: {error}")))?
        .next()
        .ok_or_else(|| gerror::new(format!("resolve UDP address \"{address}\" failed")))
}

/// Creates a connected UDP socket, like `NewNetConn`.
pub fn new_net_socket(
    remote_address: &str,
    local_address: Option<&str>,
) -> Result<UdpSocket, Error> {
    let remote = resolve(remote_address)?;
    let local = match local_address {
        Some(local_address) => resolve(local_address)?,
        None => resolve("0.0.0.0:0")?,
    };
    let socket = UdpSocket::bind(local)
        .map_err(|error| gerror::new(format!("bind UDP socket at \"{local}\" failed: {error}")))?;
    socket.connect(remote).map_err(|error| {
        gerror::new(format!(
            "connect UDP socket to \"{remote}\" failed: {error}"
        ))
    })?;
    Ok(socket)
}

/// The client-side UDP connection, like Go's `ClientConn`.
pub struct ClientConn {
    socket: UdpSocket,
    remote: SocketAddr,
}

impl ClientConn {
    /// Connects to `remote_address`, optionally binding `local_address`, like
    /// `NewClientConn`.
    pub fn new(remote_address: &str, local_address: Option<&str>) -> Result<ClientConn, Error> {
        let socket = new_net_socket(remote_address, local_address)?;
        let remote = resolve(remote_address)?;
        Ok(ClientConn { socket, remote })
    }

    /// The remote address, like `RemoteAddr`.
    pub fn remote_addr(&self) -> SocketAddr {
        self.remote
    }

    /// The local address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.local_addr().ok()
    }

    /// Sets the read and write timeout, like `SetDeadline`.
    pub fn set_deadline(&self, timeout: Duration) -> Result<(), Error> {
        self.set_deadline_recv(timeout)?;
        self.set_deadline_send(timeout)
    }

    /// Sets the read timeout, like `SetDeadlineRecv`.
    pub fn set_deadline_recv(&self, timeout: Duration) -> Result<(), Error> {
        self.socket
            .set_read_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))
    }

    /// Sets the write timeout, like `SetDeadlineSend`.
    pub fn set_deadline_send(&self, timeout: Duration) -> Result<(), Error> {
        self.socket
            .set_write_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set write deadline failed: {error}")))
    }

    /// Sends `data`, retrying on failure, like `Send`.
    pub fn send(&self, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
        let mut retry = retry;
        loop {
            match self.socket.send(data) {
                Ok(_) => return Ok(()),
                Err(error) => match retry.as_mut() {
                    Some(retry) if retry.count > 0 && !is_timeout(&error) => {
                        retry.count -= 1;
                        let interval = if retry.interval.is_zero() {
                            DEFAULT_RETRY_INTERVAL
                        } else {
                            retry.interval
                        };
                        std::thread::sleep(interval);
                    }
                    _ => {
                        return Err(gerror::new(format!("write data failed: {error}")));
                    }
                },
            }
        }
    }

    /// Receives a package, like `Recv`.
    pub fn recv(&self, buffer: i32, retry: Option<Retry>) -> Result<(Vec<u8>, SocketAddr), Error> {
        let size = if buffer > 0 {
            buffer as usize
        } else {
            DEFAULT_READ_BUFFER_SIZE as usize
        };
        let mut data = vec![0u8; size];
        let mut retry = retry;
        loop {
            match self.socket.recv(&mut data) {
                Ok(received) => {
                    data.truncate(received);
                    return Ok((data, self.remote));
                }
                Err(error) => match retry.as_mut() {
                    Some(retry) if retry.count > 0 && !is_timeout(&error) => {
                        retry.count -= 1;
                        let interval = if retry.interval.is_zero() {
                            DEFAULT_RETRY_INTERVAL
                        } else {
                            retry.interval
                        };
                        std::thread::sleep(interval);
                    }
                    _ => {
                        return Err(gerror::new(format!("read from UDP failed: {error}")));
                    }
                },
            }
        }
    }

    /// Sends `data` and waits for a response, like `SendRecv`.
    pub fn send_recv(
        &self,
        data: &[u8],
        receive: i32,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.send(data, retry)?;
        Ok(self.recv(receive, retry)?.0)
    }
}

/// The server-side UDP connection, like Go's `ServerConn`.
pub struct ServerConn {
    socket: Arc<UdpSocket>,
    closed: Arc<AtomicBool>,
}

impl ServerConn {
    /// Wraps a listening socket, like `NewServerConn`.
    pub fn new(socket: UdpSocket) -> ServerConn {
        ServerConn {
            socket: Arc::new(socket),
            closed: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Wraps a shared listening socket with an explicit closed flag.
    fn from_shared(socket: Arc<UdpSocket>, closed: Arc<AtomicBool>) -> ServerConn {
        ServerConn { socket, closed }
    }

    /// Reports whether the owning server was closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// The local address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket.local_addr().ok()
    }

    /// Sets the read and write timeout, like `SetDeadline`.
    pub fn set_deadline(&self, timeout: Duration) -> Result<(), Error> {
        self.set_deadline_recv(timeout)?;
        self.set_deadline_send(timeout)
    }

    /// Sets the read timeout, like `SetDeadlineRecv`.
    pub fn set_deadline_recv(&self, timeout: Duration) -> Result<(), Error> {
        self.socket
            .set_read_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))
    }

    /// Sets the write timeout, like `SetDeadlineSend`.
    pub fn set_deadline_send(&self, timeout: Duration) -> Result<(), Error> {
        self.socket
            .set_write_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set write deadline failed: {error}")))
    }

    /// Receives a package, like `Recv`.
    pub fn recv(&self, buffer: i32, retry: Option<Retry>) -> Result<(Vec<u8>, SocketAddr), Error> {
        let size = if buffer > 0 {
            buffer as usize
        } else {
            DEFAULT_READ_BUFFER_SIZE as usize
        };
        let mut data = vec![0u8; size];
        let mut retry = retry;
        loop {
            if self.is_closed() {
                return Err(gerror::new("connection closed"));
            }
            match self.socket.recv_from(&mut data) {
                Ok((received, remote)) => {
                    data.truncate(received);
                    return Ok((data, remote));
                }
                Err(error) => {
                    if is_timeout(&error) && self.is_closed() {
                        return Err(gerror::new("connection closed"));
                    }
                    match retry.as_mut() {
                        Some(retry) if retry.count > 0 && !is_timeout(&error) => {
                            retry.count -= 1;
                            let interval = if retry.interval.is_zero() {
                                DEFAULT_RETRY_INTERVAL
                            } else {
                                retry.interval
                            };
                            std::thread::sleep(interval);
                        }
                        _ => {
                            return Err(gerror::new(format!("read from UDP failed: {error}")));
                        }
                    }
                }
            }
        }
    }

    /// Sends `data` to `remote`, retrying on failure, like `Send`.
    pub fn send(&self, data: &[u8], remote: SocketAddr, retry: Option<Retry>) -> Result<(), Error> {
        let mut retry = retry;
        loop {
            if self.is_closed() {
                return Err(gerror::new("connection closed"));
            }
            match self.socket.send_to(data, remote) {
                Ok(_) => return Ok(()),
                Err(error) => match retry.as_mut() {
                    Some(retry) if retry.count > 0 && !is_timeout(&error) => {
                        retry.count -= 1;
                        let interval = if retry.interval.is_zero() {
                            DEFAULT_RETRY_INTERVAL
                        } else {
                            retry.interval
                        };
                        std::thread::sleep(interval);
                    }
                    _ => {
                        return Err(gerror::new(format!("write data failed: {error}")));
                    }
                },
            }
        }
    }
}

/// The server handler signature, like `ServerHandler`.
pub type ServerHandler = Arc<dyn Fn(&ServerConn) + Send + Sync>;

/// The server state shared with its handler thread.
struct ServerInner {
    address: Mutex<String>,
    handler: Mutex<Option<ServerHandler>>,
    conn: Mutex<Option<ServerConn>>,
    closed: Arc<AtomicBool>,
}

/// The UDP server, like Go's `Server`.
pub struct Server {
    inner: Arc<ServerInner>,
}

impl Server {
    /// Creates a server, optionally registering it under `name`, like
    /// `NewServer`.
    pub fn new(address: &str, handler: Option<ServerHandler>, name: Option<&str>) -> Server {
        let server = Server {
            inner: Arc::new(ServerInner {
                address: Mutex::new(address.to_owned()),
                handler: Mutex::new(handler),
                conn: Mutex::new(None),
                closed: Arc::new(AtomicBool::new(false)),
            }),
        };
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            server_registry()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(
                    name.to_owned(),
                    Server {
                        inner: Arc::clone(&server.inner),
                    },
                );
        }
        server
    }

    /// Sets the listening address.
    pub fn set_address(&self, address: &str) {
        *self
            .inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = address.to_owned();
    }

    /// Sets the connection handler.
    pub fn set_handler(&self, handler: ServerHandler) {
        *self
            .inner
            .handler
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handler);
    }

    /// Reports whether the server was closed.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Closes the server, unblocking a running handler, like `Close`.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        self.inner
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
        Ok(())
    }

    /// Starts listening and invokes the handler on the calling thread, like
    /// `Run`.
    pub fn run(&self) -> Result<(), Error> {
        let handler = self
            .inner
            .handler
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let Some(handler) = handler else {
            return Err(gerror::new_code(
                &gcode::CODE_MISSING_CONFIGURATION,
                &["start running failed: socket handler not defined"],
            ));
        };
        let address = self
            .inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let bind_address = if address.is_empty() || address == FREE_PORT_ADDRESS {
            "0.0.0.0:0".to_owned()
        } else if address.starts_with(':') {
            format!("0.0.0.0{address}")
        } else {
            address
        };
        let socket = UdpSocket::bind(&bind_address).map_err(|error| {
            gerror::new(format!("listen UDP at \"{bind_address}\" failed: {error}"))
        })?;
        socket
            .set_read_timeout(Some(Duration::from_millis(200)))
            .map_err(|error| gerror::new(format!("set read timeout failed: {error}")))?;
        let conn = ServerConn::from_shared(Arc::new(socket), Arc::clone(&self.inner.closed));
        *self
            .inner
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(conn);
        let conn = self
            .inner
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|conn| ServerConn::from_shared(Arc::clone(&conn.socket), Arc::clone(&conn.closed)))
            .expect("server connection");
        handler(&conn);
        Ok(())
    }

    /// The listened address with the concrete port substituted, like
    /// `GetListenedAddress`.
    pub fn listened_address(&self) -> String {
        let address = self
            .inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        if !address.contains(FREE_PORT_ADDRESS) {
            return address;
        }
        let port = self.listened_port();
        if address == FREE_PORT_ADDRESS {
            // Bind is IPv4 wildcard; hand back a loopback address for clients.
            return format!("127.0.0.1:{port}");
        }
        address.replace(FREE_PORT_ADDRESS, &format!(":{port}"))
    }

    /// The listened port, like `GetListenedPort`.
    pub fn listened_port(&self) -> i32 {
        let conn = self
            .inner
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match conn.as_ref().and_then(ServerConn::local_addr) {
            Some(address) => address.port() as i32,
            None => -1,
        }
    }

    /// The local address of the listening socket.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.inner
            .conn
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(ServerConn::local_addr)
    }
}

/// The named server registry, like Go's `serverMapping`.
fn server_registry() -> &'static Mutex<HashMap<String, Server>> {
    static SERVERS: OnceLock<Mutex<HashMap<String, Server>>> = OnceLock::new();
    SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

impl Clone for Server {
    fn clone(&self) -> Server {
        Server {
            inner: Arc::clone(&self.inner),
        }
    }
}

/// Returns a named server, creating it on first use, like `GetServer`.
pub fn get_server(name: Option<&str>) -> Server {
    let name = name.filter(|name| !name.is_empty()).unwrap_or("default");
    let mut registry = server_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry
        .entry(name.to_owned())
        .or_insert_with(|| Server::new("", None, None))
        .clone()
}

/// Writes `data` to `address` and closes the connection, like `Send`.
pub fn send(address: &str, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
    let conn = ClientConn::new(address, None)?;
    conn.send(data, retry)
}

/// Writes `data` to `address`, reads a response and closes, like `SendRecv`.
pub fn send_recv(
    address: &str,
    data: &[u8],
    receive: i32,
    retry: Option<Retry>,
) -> Result<Vec<u8>, Error> {
    let conn = ClientConn::new(address, None)?;
    conn.send_recv(data, receive, retry)
}

/// Returns one free UDP port, like `GetFreePort`.
pub fn get_free_port() -> Result<i32, Error> {
    Ok(get_free_ports(1)?.first().copied().unwrap_or_default())
}

/// Returns `count` free UDP ports, like `GetFreePorts`.
pub fn get_free_ports(count: usize) -> Result<Vec<i32>, Error> {
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        let socket = UdpSocket::bind("0.0.0.0:0")
            .map_err(|error| gerror::new(format!("listen UDP for a free port failed: {error}")))?;
        ports.push(
            socket
                .local_addr()
                .map_err(|error| gerror::new(format!("read local address failed: {error}")))?
                .port() as i32,
        );
    }
    Ok(ports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// Starts an echo server on a free port and returns it.
    fn start_echo_server(prefix: Option<&'static str>) -> Server {
        let server = Server::new(FREE_PORT_ADDRESS, None, None);
        let handler: ServerHandler = Arc::new(move |conn: &ServerConn| {
            while !conn.is_closed() {
                match conn.recv(-1, None) {
                    Ok((data, remote)) => {
                        if data.is_empty() {
                            continue;
                        }
                        let mut response = Vec::new();
                        if let Some(prefix) = prefix {
                            response.extend_from_slice(prefix.as_bytes());
                        }
                        response.extend_from_slice(&data);
                        let _ = conn.send(&response, remote, None);
                    }
                    Err(_) => {
                        if conn.is_closed() {
                            break;
                        }
                    }
                }
            }
        });
        server.set_handler(handler);
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        // Wait for the socket to be bound.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.local_addr().is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        server
    }

    fn client(address: &str) -> ClientConn {
        let conn = ClientConn::new(address, None).expect("client");
        conn.set_deadline(Duration::from_secs(3)).expect("deadline");
        conn
    }

    // Translated from Test_Basic.
    #[test]
    fn basic_echo_round_trip() {
        let server = start_echo_server(Some("> "));
        let address = server.listened_address();
        assert_ne!(server.listened_port(), -1);

        let conn = client(&address);
        for index in 0..10 {
            conn.send(index.to_string().as_bytes(), None).expect("send");
            let (data, _) = conn.recv(-1, None).expect("recv");
            assert_eq!(String::from_utf8_lossy(&data), format!("> {index}"));
        }
        drop(conn);

        for index in 0..10 {
            let data =
                send_recv(&address, index.to_string().as_bytes(), -1, None).expect("send_recv");
            assert_eq!(String::from_utf8_lossy(&data), format!("> {index}"));
        }

        send(&address, b"hello", None).expect("send");
        server.close().expect("close");
    }

    // Translated from Test_Buffer.
    #[test]
    fn buffer_keeps_the_whole_package() {
        let server = start_echo_server(None);
        let address = server.listened_address();
        assert_eq!(
            String::from_utf8_lossy(&send_recv(&address, b"123", -1, None).expect("123")),
            "123"
        );
        assert_eq!(
            String::from_utf8_lossy(&send_recv(&address, b"456", -1, None).expect("456")),
            "456"
        );
        server.close().expect("close");
    }

    // Translated from Test_NewConn.
    #[test]
    fn client_validation_and_local_address() {
        let server = start_echo_server(None);
        let address = server.listened_address();
        let local_port = get_free_port().expect("free port");
        let conn = ClientConn::new(&address, Some(&format!("127.0.0.1:{local_port}")))
            .expect("client with local address");
        conn.set_deadline(Duration::from_secs(1)).expect("deadline");
        conn.send(b"hello", None).expect("send");
        assert_eq!(conn.local_addr().expect("local").port() as i32, local_port);

        let invalid_port = "127.0.0.1:99999";
        assert!(ClientConn::new(invalid_port, None).is_err());
        assert!(ClientConn::new(&address, Some(invalid_port)).is_err());
        server.close().expect("close");
    }

    // Translated from Test_GetFreePorts.
    #[test]
    fn free_ports_are_available() {
        let ports = get_free_ports(2).expect("free ports");
        assert_eq!(ports.len(), 2);
        assert!(ports.iter().all(|port| *port > 0));
    }

    // Translated from Test_Server.
    #[test]
    fn named_server_registry() {
        let served = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&served);
        new_server_with_name(
            "GoFrameUDPServer",
            Arc::new(move |_conn: &ServerConn| {
                counter.fetch_add(1, Ordering::SeqCst);
            }),
        );
        let server = get_server(Some("GoFrameUDPServer"));
        assert_eq!(server.listened_port(), -1);
        let other = get_server(Some("TestUDPServer"));
        other.set_address("127.0.0.1:8888");
        other.set_handler(Arc::new(|_conn: &ServerConn| {}));
        assert_eq!(server.listened_address(), "");
    }

    /// Registers a named server with a handler, like `NewServer(address, handler, name)`.
    fn new_server_with_name(name: &str, handler: ServerHandler) {
        Server::new("", Some(handler), Some(name));
    }

    #[test]
    fn retry_defaults_and_missing_handler() {
        let retry = Retry::new(2, Duration::ZERO);
        assert_eq!(retry.count, 2);
        assert!(retry.interval.is_zero());
        let server = Server::new(FREE_PORT_ADDRESS, None, None);
        let error = server.run().expect_err("handler required");
        assert!(
            error.to_string().contains("handler not defined"),
            "got {error}"
        );
    }
}
