//! Port of `net/gtcp` — TCP connections, package protocol and server.
//!
//! The port keeps GoFrame's connection API on `std::net::TcpStream`:
//! `Send`/`Recv` (all/once/fixed length), `RecvLine`, `RecvTill`, timeout
//! variants, the simple package protocol (1–4 byte big-endian length header),
//! the package helpers, the named server registry and the free-port helpers.
//!
//! Divergences, recorded in ADR-061:
//!
//! - deadlines are relative `Duration`s (std sockets expose timeouts);
//! - `TLS` (`NewConnTLS`, `NewConnKeyCrt`, `NewServerTLS`, `NewServerKeyCrt`)
//!   and `ConnPool`/`Pool` are not ported (they need a TLS stack and a pool
//!   policy pass);
//! - a closed server connection surfaces as an `Error`;
//! - the server loops with a non-blocking listener plus a 5 ms poll so `close`
//!   stops it promptly.

pub mod pool;
pub mod tls;

use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};

/// The address that asks the system for a free port, like `FreePortAddress`.
pub const FREE_PORT_ADDRESS: &str = ":0";

/// The default connection timeout, like Go's `defaultConnTimeout`.
pub const DEFAULT_CONN_TIMEOUT: Duration = Duration::from_secs(30);

/// The default retry interval, like Go's `defaultRetryInternal`.
pub const DEFAULT_RETRY_INTERVAL: Duration = Duration::from_millis(100);

/// The default read buffer size, like Go's `defaultReadBufferSize`.
pub const DEFAULT_READ_BUFFER_SIZE: usize = 128;

/// The default buffer wait, like Go's `receiveAllWaitTimeout`.
pub const RECEIVE_ALL_WAIT_TIMEOUT: Duration = Duration::from_millis(1);

/// The default package header size in bytes, like `pkgHeaderSizeDefault`.
pub const PKG_HEADER_SIZE_DEFAULT: usize = 2;

/// The maximum package header size in bytes, like `pkgHeaderSizeMax`.
pub const PKG_HEADER_SIZE_MAX: usize = 4;

/// The maximum accepted data size (`math.MaxInt32`), like Go's check.
pub const PKG_MAX_DATA_SIZE: usize = 0x7FFF_FFFF;

/// The retry options, like Go's `Retry`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Retry {
    /// Retry count.
    pub count: u32,
    /// Retry interval; zero means [`DEFAULT_RETRY_INTERVAL`].
    pub interval: Duration,
}

impl Retry {
    /// Creates retry options.
    pub fn new(count: u32, interval: Duration) -> Retry {
        Retry { count, interval }
    }
}

/// Reports whether an I/O error is a timeout.
fn is_timeout(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

/// Resolves one address string.
fn resolve(address: &str) -> Result<SocketAddr, Error> {
    address
        .to_socket_addrs()
        .map_err(|error| gerror::new(format!("resolve TCP address \"{address}\" failed: {error}")))?
        .next()
        .ok_or_else(|| gerror::new(format!("resolve TCP address \"{address}\" failed")))
}

/// The TCP connection, like Go's `Conn`.
pub struct Conn {
    stream: TcpStream,
    buffer_wait_recv: Duration,
}

impl Conn {
    /// Creates a connection to `address`, like `NewConn`.
    pub fn new(address: &str, timeout: Option<Duration>) -> Result<Conn, Error> {
        let timeout = timeout.unwrap_or(DEFAULT_CONN_TIMEOUT);
        let socket = resolve(address)?;
        let stream = TcpStream::connect_timeout(&socket, timeout).map_err(|error| {
            gerror::new(format!(
                "connect TCP address \"{address}\" with timeout \"{timeout:?}\" failed: {error}"
            ))
        })?;
        Ok(Conn::from_stream(stream))
    }

    /// Wraps an accepted stream, like `NewConnByNetConn`.
    pub fn from_stream(stream: TcpStream) -> Conn {
        Conn {
            stream,
            buffer_wait_recv: RECEIVE_ALL_WAIT_TIMEOUT,
        }
    }

    /// The local address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.stream.local_addr().ok()
    }

    /// The remote address.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.stream.peer_addr().ok()
    }

    /// Closes the connection, like `Close`.
    pub fn close(&self) -> Result<(), Error> {
        self.stream
            .shutdown(Shutdown::Both)
            .map_err(|error| gerror::new(format!("close TCP connection failed: {error}")))
    }

    /// Writes `data`, retrying on failure, like `Send`.
    pub fn send(&self, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
        let mut retry = retry;
        loop {
            match (&self.stream).write_all(data) {
                Ok(()) => return Ok(()),
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

    /// Receives data, like `Recv`.
    ///
    /// - `length > 0`: reads exactly `length` bytes (or until error);
    /// - `length == 0`: returns once the current buffer content is read;
    /// - `length < 0`: reads until no data arrives for the buffer wait.
    pub fn recv(&self, length: i32, retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let mut buffer = if length > 0 {
            vec![0u8; length as usize]
        } else {
            vec![0u8; DEFAULT_READ_BUFFER_SIZE]
        };
        let mut index = 0usize;
        let mut buffer_wait = false;
        let mut retry = retry;
        loop {
            if length < 0 && index > 0 {
                buffer_wait = true;
                self.stream
                    .set_read_timeout(Some(self.buffer_wait_recv))
                    .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))?;
            }
            if index >= buffer.len() && length <= 0 {
                buffer.resize(buffer.len() + DEFAULT_READ_BUFFER_SIZE, 0);
            }
            match (&self.stream).read(&mut buffer[index..]) {
                Ok(0) => {
                    if buffer_wait {
                        let _ = self.stream.set_read_timeout(None);
                        return Ok(buffer[..index].to_vec());
                    }
                    return Err(gerror::new("EOF"));
                }
                Ok(size) => {
                    index += size;
                    let done = if length > 0 {
                        index >= length as usize
                    } else if length == 0 {
                        true
                    } else {
                        index < buffer.len() && !buffer_wait
                    };
                    if done {
                        return Ok(buffer[..index].to_vec());
                    }
                }
                Err(error) => {
                    if buffer_wait && is_timeout(&error) {
                        self.stream.set_read_timeout(None).map_err(|error| {
                            gerror::new(format!("set read deadline failed: {error}"))
                        })?;
                        return Ok(buffer[..index].to_vec());
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
                            return Err(gerror::new(format!("read from TCP failed: {error}")));
                        }
                    }
                }
            }
        }
    }

    /// Reads until `\n`, like `RecvLine`.
    pub fn recv_line(&self, retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let mut data = Vec::new();
        loop {
            let buffer = self.recv(1, retry)?;
            if buffer.is_empty() {
                return Ok(data);
            }
            if buffer[0] == b'\n' {
                return Ok(data);
            }
            data.extend_from_slice(&buffer);
        }
    }

    /// Reads until `til` (inclusive), like `RecvTill`.
    pub fn recv_till(&self, til: &[u8], retry: Option<Retry>) -> Result<Vec<u8>, Error> {
        let mut data = Vec::new();
        let length = til.len();
        loop {
            let buffer = self.recv(1, retry)?;
            if buffer.is_empty() {
                return Ok(data);
            }
            if length > 0
                && data.len() >= length - 1
                && buffer[0] == til[length - 1]
                && data[data.len() - length + 1..].eq_ignore_ascii_case(&til[..length - 1])
            {
                data.extend_from_slice(&buffer);
                return Ok(data);
            }
            data.extend_from_slice(&buffer);
        }
    }

    /// Reads with a timeout, like `RecvWithTimeout`.
    pub fn recv_with_timeout(
        &self,
        length: i32,
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.set_deadline_recv(Some(timeout))?;
        let result = self.recv(length, retry);
        self.set_deadline_recv(None)?;
        result
    }

    /// Writes with a timeout, like `SendWithTimeout`.
    pub fn send_with_timeout(
        &self,
        data: &[u8],
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<(), Error> {
        self.set_deadline_send(Some(timeout))?;
        let result = self.send(data, retry);
        self.set_deadline_send(None)?;
        result
    }

    /// Writes and reads a response, like `SendRecv`.
    pub fn send_recv(
        &self,
        data: &[u8],
        length: i32,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.send(data, retry)?;
        self.recv(length, retry)
    }

    /// Writes and reads a response with a timeout, like
    /// `SendRecvWithTimeout`.
    pub fn send_recv_with_timeout(
        &self,
        data: &[u8],
        length: i32,
        timeout: Duration,
        retry: Option<Retry>,
    ) -> Result<Vec<u8>, Error> {
        self.send(data, retry)?;
        self.recv_with_timeout(length, timeout, retry)
    }

    /// Sets both timeouts, like `SetDeadline`.
    pub fn set_deadline(&self, timeout: Option<Duration>) -> Result<(), Error> {
        self.stream
            .set_read_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))?;
        self.stream
            .set_write_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))
    }

    /// Sets the read timeout, like `SetDeadlineRecv`.
    pub fn set_deadline_recv(&self, timeout: Option<Duration>) -> Result<(), Error> {
        self.stream
            .set_read_timeout(timeout)
            .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))
    }

    /// Sets the write timeout, like `SetDeadlineSend`.
    pub fn set_deadline_send(&self, timeout: Option<Duration>) -> Result<(), Error> {
        self.stream
            .set_write_timeout(timeout)
            .map_err(|error| gerror::new(format!("set write deadline failed: {error}")))
    }

    /// Sets the buffer wait used by `recv(length < 0)`, like
    /// `SetBufferWaitRecv`.
    pub fn set_buffer_wait_recv(&mut self, wait: Duration) {
        self.buffer_wait_recv = wait;
    }

    /// Sends one package, like `SendPkg`.
    pub fn send_pkg(&self, data: &[u8], option: Option<PkgOption>) -> Result<(), Error> {
        let option = get_pkg_option(option)?;
        if data.len() > option.max_data_size {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!(
                    "data too long, data size {} exceeds allowed max data size {}",
                    data.len(),
                    option.max_data_size
                ),
                Vec::new(),
            ));
        }
        let offset = PKG_HEADER_SIZE_MAX - option.header_size;
        let mut buffer = vec![0u8; PKG_HEADER_SIZE_MAX + data.len()];
        buffer[..4].copy_from_slice(&(data.len() as u32).to_be_bytes());
        buffer[PKG_HEADER_SIZE_MAX..].copy_from_slice(data);
        self.send(&buffer[offset..], Some(option.retry))
    }

    /// Sends one package with a timeout, like `SendPkgWithTimeout`.
    pub fn send_pkg_with_timeout(
        &self,
        data: &[u8],
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<(), Error> {
        self.set_deadline_send(Some(timeout))?;
        let result = self.send_pkg(data, option);
        self.set_deadline_send(None)?;
        result
    }

    /// Sends one package and reads the response package, like `SendRecvPkg`.
    pub fn send_recv_pkg(&self, data: &[u8], option: Option<PkgOption>) -> Result<Vec<u8>, Error> {
        self.send_pkg(data, option.clone())?;
        self.recv_pkg(option)
    }

    /// Sends one package and reads the response with a timeout, like
    /// `SendRecvPkgWithTimeout`.
    pub fn send_recv_pkg_with_timeout(
        &self,
        data: &[u8],
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<Vec<u8>, Error> {
        self.send_pkg(data, option.clone())?;
        self.recv_pkg_with_timeout(timeout, option)
    }

    /// Reads one package, like `RecvPkg`.
    pub fn recv_pkg(&self, option: Option<PkgOption>) -> Result<Vec<u8>, Error> {
        let option = get_pkg_option(option)?;
        let header = self.recv(option.header_size as i32, Some(option.retry))?;
        if header.len() != option.header_size {
            return Err(gerror::new("invalid package header"));
        }
        let mut length_bytes = [0u8; 4];
        length_bytes[4 - option.header_size..].copy_from_slice(&header);
        let length = u32::from_be_bytes(length_bytes) as usize;
        if length > option.max_data_size {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid package size {length}"),
                Vec::new(),
            ));
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        self.recv(length as i32, Some(option.retry))
    }

    /// Reads one package with a timeout, like `RecvPkgWithTimeout`.
    pub fn recv_pkg_with_timeout(
        &self,
        timeout: Duration,
        option: Option<PkgOption>,
    ) -> Result<Vec<u8>, Error> {
        self.set_deadline_recv(Some(timeout))?;
        let result = self.recv_pkg(option);
        self.set_deadline_recv(None)?;
        result
    }
}

/// The simple package protocol option, like Go's `PkgOption`.
#[derive(Clone, Debug, Default)]
pub struct PkgOption {
    /// Header size in bytes; `0` means [`PKG_HEADER_SIZE_DEFAULT`].
    pub header_size: usize,
    /// Maximum data size; `0` means the header-size default.
    pub max_data_size: usize,
    /// Retry policy.
    pub retry: Retry,
}

/// Normalises and validates a package option, like `getPkgOption`.
pub fn get_pkg_option(option: Option<PkgOption>) -> Result<PkgOption, Error> {
    let mut option = option.unwrap_or_default();
    if option.header_size == 0 {
        option.header_size = PKG_HEADER_SIZE_DEFAULT;
    }
    if option.header_size > PKG_HEADER_SIZE_MAX {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "package header size {} definition exceeds max header size {}",
                option.header_size, PKG_HEADER_SIZE_MAX
            ),
            Vec::new(),
        ));
    }
    if option.max_data_size == 0 {
        option.max_data_size = match option.header_size {
            1 => 0xFF,
            2 => 0xFFFF,
            3 => 0xFF_FFFF,
            _ => PKG_MAX_DATA_SIZE,
        };
    }
    if option.max_data_size > PKG_MAX_DATA_SIZE {
        return Err(gerror::new_codef(
            &gcode::CODE_INVALID_PARAMETER,
            format!(
                "package data size {} definition exceeds allowed max data size {}",
                option.max_data_size, PKG_MAX_DATA_SIZE
            ),
            Vec::new(),
        ));
    }
    Ok(option)
}

/// The connection handler, like `func(*Conn)`.
pub type ServerHandler = Arc<dyn Fn(&Conn) + Send + Sync>;

/// The server state shared with its accept loop.
struct ServerInner {
    address: Mutex<String>,
    handler: Mutex<Option<ServerHandler>>,
    local: Mutex<Option<SocketAddr>>,
    closed: AtomicBool,
}

/// The TCP server, like Go's `Server`.
pub struct Server {
    inner: Arc<ServerInner>,
}

impl Clone for Server {
    fn clone(&self) -> Server {
        Server {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl Server {
    /// Creates a server, optionally registering it under `name`, like
    /// `NewServer`.
    pub fn new(address: &str, handler: Option<ServerHandler>, name: Option<&str>) -> Server {
        let server = Server {
            inner: Arc::new(ServerInner {
                address: Mutex::new(address.to_owned()),
                handler: Mutex::new(handler),
                local: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
        };
        if let Some(name) = name.filter(|name| !name.is_empty()) {
            server_registry()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(name.to_owned(), server.clone());
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

    /// The listening address.
    pub fn address(&self) -> String {
        self.inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
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

    /// Closes the listener, like `Close`.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Starts the accept loop on the calling thread, like `Run`.
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
        let address = self.address();
        let bind_address = if address.is_empty() || address == FREE_PORT_ADDRESS {
            "0.0.0.0:0".to_owned()
        } else if address.starts_with(':') {
            format!("0.0.0.0{address}")
        } else {
            address
        };
        let listener = TcpListener::bind(&bind_address).map_err(|error| {
            gerror::new(format!("listen TCP at \"{bind_address}\" failed: {error}"))
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| gerror::new(format!("set listener non-blocking failed: {error}")))?;
        *self
            .inner
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = listener.local_addr().ok();
        while !self.is_closed() {
            match listener.accept() {
                Ok((stream, _)) => {
                    // A non-blocking listener can hand out non-blocking
                    // streams; the handler wants blocking I/O like Go.
                    if let Err(error) = stream.set_nonblocking(false) {
                        return Err(gerror::new(format!(
                            "reset accepted stream to blocking failed: {error}"
                        )));
                    }
                    let handler = Arc::clone(&handler);
                    std::thread::spawn(move || {
                        let conn = Conn::from_stream(stream);
                        handler(&conn);
                    });
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    if self.is_closed() {
                        break;
                    }
                    return Err(gerror::new(format!(
                        "accept TCP connection failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// The listened address with the concrete port substituted, like
    /// `GetListenedAddress`.
    pub fn listened_address(&self) -> String {
        let address = self.address();
        if !address.contains(FREE_PORT_ADDRESS) {
            return address;
        }
        let port = self.listened_port();
        if address == FREE_PORT_ADDRESS {
            return format!("127.0.0.1:{port}");
        }
        address.replace(FREE_PORT_ADDRESS, &format!(":{port}"))
    }

    /// The listened port, like `GetListenedPort`.
    pub fn listened_port(&self) -> i32 {
        self.inner
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .map(|address| address.port() as i32)
            .unwrap_or(-1)
    }

    /// The local address of the listener.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self
            .inner
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The named server registry, like Go's `serverMapping`.
fn server_registry() -> &'static Mutex<std::collections::HashMap<String, Server>> {
    static SERVERS: OnceLock<Mutex<std::collections::HashMap<String, Server>>> = OnceLock::new();
    SERVERS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
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

/// Connects to `address`, like `NewConn`.
pub fn new_conn(address: &str, timeout: Option<Duration>) -> Result<Conn, Error> {
    Conn::new(address, timeout)
}

/// Writes `data` to `address` and closes, like `Send`.
pub fn send(address: &str, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send(data, retry);
    let _ = conn.close();
    result
}

/// Writes `data` to `address`, reads a response and closes, like `SendRecv`.
pub fn send_recv(
    address: &str,
    data: &[u8],
    length: i32,
    retry: Option<Retry>,
) -> Result<Vec<u8>, Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_recv(data, length, retry);
    let _ = conn.close();
    result
}

/// Writes `data` with a timeout and closes, like `SendWithTimeout`.
pub fn send_with_timeout(
    address: &str,
    data: &[u8],
    timeout: Duration,
    retry: Option<Retry>,
) -> Result<(), Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_with_timeout(data, timeout, retry);
    let _ = conn.close();
    result
}

/// Writes, reads a response with a timeout and closes, like
/// `SendRecvWithTimeout`.
pub fn send_recv_with_timeout(
    address: &str,
    data: &[u8],
    receive: i32,
    timeout: Duration,
    retry: Option<Retry>,
) -> Result<Vec<u8>, Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_recv_with_timeout(data, receive, timeout, retry);
    let _ = conn.close();
    result
}

/// Sends one package to `address` and closes, like `SendPkg`.
pub fn send_pkg(address: &str, data: &[u8], option: Option<PkgOption>) -> Result<(), Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_pkg(data, option);
    let _ = conn.close();
    result
}

/// Sends one package and reads the response package, like `SendRecvPkg`.
pub fn send_recv_pkg(
    address: &str,
    data: &[u8],
    option: Option<PkgOption>,
) -> Result<Vec<u8>, Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_recv_pkg(data, option);
    let _ = conn.close();
    result
}

/// Sends one package with a timeout, like `SendPkgWithTimeout`.
pub fn send_pkg_with_timeout(
    address: &str,
    data: &[u8],
    timeout: Duration,
    option: Option<PkgOption>,
) -> Result<(), Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_pkg_with_timeout(data, timeout, option);
    let _ = conn.close();
    result
}

/// Sends one package and reads the response with a timeout, like
/// `SendRecvPkgWithTimeout`.
pub fn send_recv_pkg_with_timeout(
    address: &str,
    data: &[u8],
    timeout: Duration,
    option: Option<PkgOption>,
) -> Result<Vec<u8>, Error> {
    let conn = Conn::new(address, None)?;
    let result = conn.send_recv_pkg_with_timeout(data, timeout, option);
    let _ = conn.close();
    result
}

/// Returns one free TCP port, like `GetFreePort`.
pub fn get_free_port() -> Result<i32, Error> {
    Ok(get_free_ports(1)?.first().copied().unwrap_or_default())
}

/// Returns `count` free TCP ports, like `GetFreePorts`.
pub fn get_free_ports(count: usize) -> Result<Vec<i32>, Error> {
    let mut ports = Vec::with_capacity(count);
    for _ in 0..count {
        let listener = TcpListener::bind("0.0.0.0:0")
            .map_err(|error| gerror::new(format!("listen TCP for a free port failed: {error}")))?;
        ports.push(
            listener
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

    fn start_server(handler: ServerHandler) -> Server {
        let server = Server::new(FREE_PORT_ADDRESS, Some(handler), None);
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        server
    }

    fn echo_handler() -> ServerHandler {
        Arc::new(|conn: &Conn| {
            loop {
                match conn.recv(-1, None) {
                    Ok(data) if !data.is_empty() => {
                        if conn.send(&data, None).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        })
    }

    fn client(address: &str) -> Conn {
        let conn = Conn::new(address, Some(Duration::from_secs(5))).expect("client");
        conn.set_deadline(Some(Duration::from_secs(5)))
            .expect("deadline");
        conn
    }

    // Translated from TestNewConn / TestConn_Send / TestConn_SendRecv.
    #[test]
    fn conn_send_and_recv() {
        let server = start_server(echo_handler());
        let address = server.listened_address();
        let conn = client(&address);
        conn.send(b"hello", None).expect("send");
        assert_eq!(conn.recv(5, None).expect("recv"), b"hello");
        assert_eq!(
            conn.send_recv(b"world", -1, None).expect("send_recv"),
            b"world"
        );
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from TestConn_RecvLine / TestConn_RecvTill.
    #[test]
    fn recv_line_and_till() {
        let server = start_server(Arc::new(|conn: &Conn| {
            let _ = conn.send(b"line1\nline2", None);
        }));
        let conn = client(&server.listened_address());
        assert_eq!(conn.recv_line(None).expect("line"), b"line1");
        assert_eq!(conn.recv_till(b"2", None).expect("till"), b"line2");
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from TestConn_RecvWithTimeout / TestConn_SendWithTimeout.
    #[test]
    fn timeouts_surface() {
        let server = start_server(Arc::new(|_: &Conn| {
            std::thread::sleep(Duration::from_millis(500));
        }));
        let address = server.listened_address();
        let conn = client(&address);
        let started = std::time::Instant::now();
        assert!(
            conn.recv_with_timeout(-1, Duration::from_millis(50), None)
                .is_err()
        );
        assert!(started.elapsed() < Duration::from_secs(2));
        conn.send_with_timeout(b"x", Duration::from_millis(500), None)
            .expect("send timeout");
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from Test_Package_Basic.
    #[test]
    fn package_round_trip() {
        let server = start_server(Arc::new(|conn: &Conn| {
            while let Ok(data) = conn.recv_pkg(None) {
                if conn.send_pkg(&data, None).is_err() {
                    break;
                }
            }
        }));
        let address = server.listened_address();
        let conn = client(&address);
        for index in 0..10 {
            let data = index.to_string().into_bytes();
            assert_eq!(conn.send_recv_pkg(&data, None).expect("pkg"), data);
            assert_eq!(
                conn.send_recv_pkg_with_timeout(&data, Duration::from_secs(2), None)
                    .expect("pkg timeout"),
                data
            );
        }
        // 65536 bytes exceed the default 2-byte header maximum.
        let big = vec![0u8; 65536];
        assert!(conn.send_pkg(&big, None).is_err());
        // 65500 bytes fit.
        let mut fit = vec![0u8; 65500];
        fit[100] = b'A';
        fit[65400] = b'U';
        assert_eq!(conn.send_recv_pkg(&fit, None).expect("fit"), fit);
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from Test_Package_Option / Test_Package_Option_HeadSize3 /
    // Test_Package_Option_HeadSize4.
    #[test]
    fn package_header_sizes() {
        for header_size in [1usize, 3, 4] {
            let option_server = Some(PkgOption {
                header_size,
                ..PkgOption::default()
            });
            let server = start_server({
                let option = option_server.clone();
                Arc::new(move |conn: &Conn| {
                    while let Ok(data) = conn.recv_pkg(option.clone()) {
                        if conn.send_pkg(&data, option.clone()).is_err() {
                            break;
                        }
                    }
                })
            });
            let conn = client(&server.listened_address());
            let data = vec![b'x'; 0xFF];
            assert_eq!(
                conn.send_recv_pkg(&data, option_server.clone())
                    .expect("header size"),
                data
            );
            if header_size == 1 {
                let too_long = vec![0u8; 0x100];
                assert!(conn.send_recv_pkg(&too_long, option_server).is_err());
            }
            conn.close().expect("close");
            server.close().expect("stop");
        }
    }

    // Translated from Test_Package_Timeout.
    #[test]
    fn package_timeout() {
        let server = start_server(Arc::new(|conn: &Conn| {
            while let Ok(data) = conn.recv_pkg(None) {
                std::thread::sleep(Duration::from_millis(200));
                if conn.send_pkg(&data, None).is_err() {
                    break;
                }
            }
        }));
        let conn = client(&server.listened_address());
        assert!(
            conn.send_recv_pkg_with_timeout(b"10000", Duration::from_millis(50), None)
                .is_err()
        );
        conn.close().expect("close");

        let conn = client(&server.listened_address());
        assert_eq!(
            conn.send_recv_pkg_with_timeout(b"10000", Duration::from_secs(2), None)
                .expect("slow response"),
            b"10000"
        );
        conn.close().expect("close");
        // In-flight response of the first client is irrelevant for this one.
        assert!(
            conn.send_recv_pkg_with_timeout(
                b"10000",
                Duration::from_secs(1),
                Some(PkgOption {
                    header_size: 5,
                    ..PkgOption::default()
                })
            )
            .is_err()
        );
        server.close().expect("stop");
    }

    // Translated from Test_Conn_RecvPkgError.
    #[test]
    fn invalid_package_option_fails() {
        let server = start_server(Arc::new(|conn: &Conn| {
            let option = Some(PkgOption {
                header_size: 5,
                ..PkgOption::default()
            });
            let _ = conn.recv_pkg(option);
        }));
        let conn = client(&server.listened_address());
        let data = vec![0u8; 65536];
        assert!(conn.send_recv_pkg(&data, None).is_err());
        conn.close().expect("close");
        server.close().expect("stop");
    }

    // Translated from TestGetFreePorts / TestMustGetFreePort.
    #[test]
    fn free_ports() {
        let ports = get_free_ports(2).expect("ports");
        assert_eq!(ports.len(), 2);
        assert!(ports.iter().all(|port| *port > 0));
        assert!(get_free_port().expect("port") > 0);
    }

    // Translated from TestGetServer / TestServer_SetAddress / TestServer_SetHandler.
    #[test]
    fn server_registry_and_errors() {
        let handler: ServerHandler = Arc::new(|_: &Conn| {});
        Server::new(
            "127.0.0.1:0",
            Some(Arc::clone(&handler)),
            Some("gf-tcp-test"),
        );
        let server = get_server(Some("gf-tcp-test"));
        assert_eq!(server.address(), "127.0.0.1:0");
        server.set_address("127.0.0.1:18888");
        assert_eq!(server.address(), "127.0.0.1:18888");
        let other = get_server(Some("gf-tcp-default"));
        assert_eq!(other.address(), "");
        assert_eq!(other.listened_port(), -1);
        other.set_handler(Arc::clone(&handler));
        assert!(Server::new(FREE_PORT_ADDRESS, None, None).run().is_err());
    }

    // Translated from TestSend / TestSendRecv / TestConn_SetDeadline.
    #[test]
    fn package_helpers_and_deadline() {
        let server = start_server(echo_handler());
        let address = server.listened_address();
        send(&address, b"raw", None).expect("send");
        assert_eq!(send_recv(&address, b"raw", -1, None).expect("recv"), b"raw");
        send_with_timeout(&address, b"raw", Duration::from_secs(2), None).expect("send timeout");
        assert_eq!(
            send_recv_with_timeout(&address, b"raw", -1, Duration::from_secs(2), None)
                .expect("recv timeout"),
            b"raw"
        );
        assert!(Conn::new("127.0.0.1:1", Some(Duration::from_millis(100))).is_err());
        server.close().expect("stop");
    }
}
