//! TLS support for `net/gtcp` — client connections and a TLS server.
//!
//! GoFrame's `gtcp.Conn` transparently wraps `tls.Conn` when the server uses
//! TLS, and `gtcp.Server` can be configured with a certificate. The Rust port
//! keeps the plain [`super::Conn`] unchanged and adds a TLS sibling:
//!
//! - [`TlsClientConfig`] with the Mozilla roots plus custom PEM/DER roots;
//! - [`TlsConn`] for both client and server sides, with the same
//!   `send`/`recv` contract as [`super::Conn`];
//! - [`TlsServer`] accepting TLS connections and invoking a `Fn(TlsConn)`
//!   handler per connection.
//!
//! Not ported: pool integration, client certificates, ALPN and certificate
//! hot reload (ADR-086/087).

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gf_errors::gerror::{self, Error};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, ServerConnection, StreamOwned};

use super::{
    DEFAULT_CONN_TIMEOUT, DEFAULT_READ_BUFFER_SIZE, DEFAULT_RETRY_INTERVAL,
    RECEIVE_ALL_WAIT_TIMEOUT, Retry, is_timeout, resolve,
};

/// The client TLS configuration: trusted roots.
pub struct TlsClientConfig {
    roots: RootCertStore,
}

impl Default for TlsClientConfig {
    fn default() -> TlsClientConfig {
        TlsClientConfig::new()
    }
}

impl TlsClientConfig {
    /// Creates a configuration trusting the Mozilla root certificates.
    pub fn new() -> TlsClientConfig {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        TlsClientConfig { roots }
    }

    /// Adds PEM certificates to the trust store.
    pub fn add_root_certificate_pem(&mut self, pem: &str) -> Result<(), Error> {
        let mut reader = std::io::BufReader::new(pem.as_bytes());
        let certificates = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| gerror::new(format!("parse TLS root certificate failed: {error}")))?;
        if certificates.is_empty() {
            return Err(gerror::new("no certificate found in PEM content"));
        }
        for certificate in certificates {
            self.roots.add(certificate).map_err(|error| {
                gerror::new(format!("add TLS root certificate failed: {error}"))
            })?;
        }
        Ok(())
    }

    /// Adds a DER certificate to the trust store.
    pub fn add_root_certificate_der(&mut self, der: &[u8]) -> Result<(), Error> {
        self.roots
            .add(CertificateDer::from(der.to_vec()))
            .map_err(|error| gerror::new(format!("add TLS root certificate failed: {error}")))
    }
}

/// The TLS stream side.
enum TlsInner {
    Client(StreamOwned<ClientConnection, TcpStream>),
    Server(StreamOwned<ServerConnection, TcpStream>),
}

/// A TLS connection, like Go's `tls.Conn`; used for both sides.
pub struct TlsConn {
    inner: TlsInner,
    buffer_wait_recv: Duration,
}

impl TlsConn {
    /// Connects to `address` and verifies the server against `server_name`
    /// using `config`, like `NewConn` for a TLS endpoint.
    pub fn new(
        address: &str,
        server_name: &str,
        config: &TlsClientConfig,
        timeout: Option<Duration>,
    ) -> Result<TlsConn, Error> {
        let timeout = timeout.unwrap_or(DEFAULT_CONN_TIMEOUT);
        let socket = resolve(address)?;
        let stream = TcpStream::connect_timeout(&socket, timeout).map_err(|error| {
            gerror::new(format!(
                "connect TLS address \"{address}\" with timeout \"{timeout:?}\" failed: {error}"
            ))
        })?;
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set TLS read timeout failed: {error}")))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set TLS write timeout failed: {error}")))?;
        let client_config = ClientConfig::builder()
            .with_root_certificates(config.roots.clone())
            .with_no_client_auth();
        let server_name = ServerName::try_from(server_name.to_owned()).map_err(|error| {
            gerror::new(format!(
                "invalid TLS server name \"{server_name}\": {error}"
            ))
        })?;
        let connection = ClientConnection::new(Arc::new(client_config), server_name)
            .map_err(|error| gerror::new(format!("start TLS client failed: {error}")))?;
        Ok(TlsConn {
            inner: TlsInner::Client(StreamOwned::new(connection, stream)),
            buffer_wait_recv: RECEIVE_ALL_WAIT_TIMEOUT,
        })
    }

    /// Wraps an accepted `stream` as the server side of a TLS session.
    pub fn from_accepted(
        stream: TcpStream,
        config: Arc<rustls::ServerConfig>,
    ) -> Result<TlsConn, Error> {
        stream
            .set_read_timeout(Some(DEFAULT_CONN_TIMEOUT))
            .map_err(|error| gerror::new(format!("set TLS read timeout failed: {error}")))?;
        stream
            .set_write_timeout(Some(DEFAULT_CONN_TIMEOUT))
            .map_err(|error| gerror::new(format!("set TLS write timeout failed: {error}")))?;
        let connection = ServerConnection::new(config)
            .map_err(|error| gerror::new(format!("start TLS server connection failed: {error}")))?;
        Ok(TlsConn {
            inner: TlsInner::Server(StreamOwned::new(connection, stream)),
            buffer_wait_recv: RECEIVE_ALL_WAIT_TIMEOUT,
        })
    }

    /// The TCP socket under the TLS layer.
    fn socket(&self) -> &TcpStream {
        match &self.inner {
            TlsInner::Client(stream) => &stream.sock,
            TlsInner::Server(stream) => &stream.sock,
        }
    }

    /// The local address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.socket().local_addr().ok()
    }

    /// The remote address.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.socket().peer_addr().ok()
    }

    /// Sets the read/write deadline.
    pub fn set_deadline(&mut self, timeout: Option<Duration>) -> Result<(), Error> {
        self.socket()
            .set_read_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))?;
        self.socket()
            .set_write_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))
    }

    /// Writes `data`, retrying on failure, like `Send`.
    pub fn send(&mut self, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
        let mut retry = retry;
        loop {
            let result = match &mut self.inner {
                TlsInner::Client(stream) => std::io::Write::write_all(stream, data),
                TlsInner::Server(stream) => std::io::Write::write_all(stream, data),
            };
            match result {
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
                    _ => return Err(gerror::new(format!("write TLS data failed: {error}"))),
                },
            }
        }
    }

    /// Receives data with the [`super::Conn::recv`] length semantics.
    pub fn recv(&mut self, length: i32, retry: Option<Retry>) -> Result<Vec<u8>, Error> {
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
                self.socket()
                    .set_read_timeout(Some(self.buffer_wait_recv))
                    .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))?;
            }
            if index >= buffer.len() && length <= 0 {
                buffer.resize(buffer.len() + DEFAULT_READ_BUFFER_SIZE, 0);
            }
            let result = match &mut self.inner {
                TlsInner::Client(stream) => std::io::Read::read(stream, &mut buffer[index..]),
                TlsInner::Server(stream) => std::io::Read::read(stream, &mut buffer[index..]),
            };
            match result {
                Ok(0) => {
                    if buffer_wait {
                        let _ = self.socket().set_read_timeout(None);
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
                        self.socket().set_read_timeout(None).map_err(|error| {
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
                            return Err(gerror::new(format!("read from TLS failed: {error}")));
                        }
                    }
                }
            }
        }
    }

    /// Closes the connection.
    pub fn close(mut self) -> Result<(), Error> {
        match &mut self.inner {
            TlsInner::Client(stream) => {
                stream.conn.send_close_notify();
                let _ = std::io::Write::flush(stream);
            }
            TlsInner::Server(stream) => {
                stream.conn.send_close_notify();
                let _ = std::io::Write::flush(stream);
            }
        }
        self.socket()
            .shutdown(std::net::Shutdown::Both)
            .map_err(|error| gerror::new(format!("close TLS connection failed: {error}")))
    }
}

/// The TLS server handler type.
pub type TlsServerHandler = Arc<dyn Fn(TlsConn) + Send + Sync>;

/// The server state shared with the accept loop.
struct TlsServerInner {
    address: Mutex<String>,
    local: Mutex<Option<SocketAddr>>,
    closed: AtomicBool,
    handler: Mutex<Option<TlsServerHandler>>,
    tls: Mutex<Option<Arc<rustls::ServerConfig>>>,
}

/// A TLS TCP server, like a TLS-configured GoFrame `gtcp.Server`.
#[derive(Clone)]
pub struct TlsServer {
    inner: Arc<TlsServerInner>,
}

impl TlsServer {
    /// Creates a TLS server for `address`.
    pub fn new(address: &str) -> TlsServer {
        TlsServer {
            inner: Arc::new(TlsServerInner {
                address: Mutex::new(address.to_owned()),
                local: Mutex::new(None),
                closed: AtomicBool::new(false),
                handler: Mutex::new(None),
                tls: Mutex::new(None),
            }),
        }
    }

    /// The configured address.
    pub fn address(&self) -> String {
        self.inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Sets the listening address.
    pub fn set_address(&self, address: &str) {
        *self
            .inner
            .address
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = address.to_owned();
    }

    /// Sets the TLS certificate chain and private key (PEM).
    pub fn set_tls_pem(&self, certificate_pem: &str, private_key_pem: &str) -> Result<(), Error> {
        let mut certificate_reader = std::io::BufReader::new(certificate_pem.as_bytes());
        let certificates = rustls_pemfile::certs(&mut certificate_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| gerror::new(format!("parse TLS certificate failed: {error}")))?;
        if certificates.is_empty() {
            return Err(gerror::new("no certificate found in PEM content"));
        }
        let mut key_reader = std::io::BufReader::new(private_key_pem.as_bytes());
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|error| gerror::new(format!("parse TLS private key failed: {error}")))?
            .ok_or_else(|| gerror::new("no private key found in PEM content"))?;
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certificates, key)
            .map_err(|error| gerror::new(format!("build TLS server config failed: {error}")))?;
        *self
            .inner
            .tls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Arc::new(config));
        Ok(())
    }

    /// Sets the per-connection handler.
    pub fn set_handler(&self, handler: TlsServerHandler) {
        *self
            .inner
            .handler
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handler);
    }

    /// Stops the accept loop.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// The listened port, or -1 before binding.
    pub fn listened_port(&self) -> i32 {
        self.local_addr()
            .map(|address| address.port() as i32)
            .unwrap_or(-1)
    }

    /// The listened address with the concrete port substituted.
    pub fn listened_address(&self) -> String {
        match self.local_addr() {
            Some(address) => address.to_string(),
            None => self.address(),
        }
    }

    /// The local address of the listener.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        *self
            .inner
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Starts accepting and blocks until closed.
    pub fn run(&self) -> Result<(), Error> {
        let address = self.address();
        let bind_address = if address.is_empty() {
            "0.0.0.0:0".to_owned()
        } else if address.starts_with(':') {
            format!("0.0.0.0{address}")
        } else if let Ok(port) = address.parse::<u16>() {
            format!("0.0.0.0:{port}")
        } else {
            address
        };
        let listener = std::net::TcpListener::bind(&bind_address).map_err(|error| {
            gerror::new(format!("listen TLS at \"{bind_address}\" failed: {error}"))
        })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| gerror::new(format!("set listener non-blocking failed: {error}")))?;
        *self
            .inner
            .local
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = listener.local_addr().ok();
        let tls = self
            .inner
            .tls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
            .ok_or_else(|| gerror::new("TLS certificate is not configured"))?;
        while !self.inner.closed.load(Ordering::SeqCst) {
            match listener.accept() {
                Ok((stream, _)) => {
                    if let Err(error) = stream.set_nonblocking(false) {
                        return Err(gerror::new(format!(
                            "reset accepted stream to blocking failed: {error}"
                        )));
                    }
                    let handler = self
                        .inner
                        .handler
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    let Some(handler) = handler else {
                        continue;
                    };
                    let tls = Arc::clone(&tls);
                    std::thread::spawn(move || {
                        let Ok(connection) = TlsConn::from_accepted(stream, tls) else {
                            return;
                        };
                        handler(connection);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    if self.inner.closed.load(Ordering::SeqCst) {
                        break;
                    }
                    return Err(gerror::new(format!(
                        "accept TLS connection failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generates a self-signed certificate/key PEM pair for `localhost`.
    fn tls_pem() -> (String, String) {
        let certified =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        (certified.cert.pem(), certified.key_pair.serialize_pem())
    }

    /// Parses the first certificate of `pem` as DER.
    fn parse_certificate_der(pem: &str) -> Vec<u8> {
        let mut reader = std::io::BufReader::new(pem.as_bytes());
        let certificates: Vec<_> = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("certs");
        certificates[0].as_ref().to_vec()
    }

    fn client_config(certificate: &[u8]) -> TlsClientConfig {
        let mut config = TlsClientConfig::new();
        config.add_root_certificate_der(certificate).expect("root");
        config
    }

    /// Starts a TLS echo server; returns it (kept alive by the caller), its
    /// port and the certificate DER.
    fn start_echo_tls_server() -> (TlsServer, u16, Vec<u8>) {
        let (certificate_pem, key_pem) = tls_pem();
        let certificate_der = parse_certificate_der(&certificate_pem);
        let server = TlsServer::new("127.0.0.1:0");
        server.set_tls_pem(&certificate_pem, &key_pem).expect("tls");
        server.set_handler(Arc::new(|mut connection: TlsConn| {
            let mut buffer = [0u8; 4];
            let mut read = 0;
            while read < buffer.len() {
                match connection.recv((buffer.len() - read) as i32, None) {
                    Ok(data) if !data.is_empty() => {
                        buffer[read..read + data.len()].copy_from_slice(&data);
                        read += data.len();
                    }
                    _ => return,
                }
            }
            let _ = connection.send(&buffer, None);
            let _ = connection.close();
        }));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let port = server.listened_port() as u16;
        (server, port, certificate_der)
    }

    #[test]
    fn echo_round_trip() {
        let (_server, port, certificate) = start_echo_tls_server();
        let config = client_config(&certificate);
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &config,
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        assert!(conn.remote_addr().is_some());
        assert!(conn.local_addr().is_some());
        conn.set_deadline(Some(Duration::from_secs(5)))
            .expect("deadline");
        conn.send(b"ping", None).expect("send");
        assert_eq!(conn.recv(4, None).expect("recv"), b"ping");
        conn.close().expect("close");
    }

    #[test]
    fn tls_server_lifecycle() {
        let (certificate_pem, key_pem) = tls_pem();
        let certificate_der = parse_certificate_der(&certificate_pem);
        let server = TlsServer::new("127.0.0.1:0");
        assert_eq!(server.address(), "127.0.0.1:0");
        server.set_address("127.0.0.1:0");
        server.set_tls_pem(&certificate_pem, &key_pem).expect("tls");
        assert_eq!(server.listened_port(), -1);
        server.set_handler(Arc::new(|mut connection: TlsConn| {
            if let Ok(data) = connection.recv(4, None) {
                let _ = connection.send(&data, None);
            }
            let _ = connection.close();
        }));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(!server.listened_address().is_empty());
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{}", server.listened_port()),
            "localhost",
            &client_config(&certificate_der),
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        conn.send(b"echo", None).expect("send");
        assert_eq!(conn.recv(4, None).expect("recv"), b"echo");
        let _ = conn.close();
        server.close().expect("close");
    }

    #[test]
    fn untrusted_root_fails_to_handshake() {
        let (_server, port, _certificate) = start_echo_tls_server();
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &TlsClientConfig::new(),
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        assert!(conn.send(b"ping", None).is_err());
    }

    #[test]
    fn pem_roots_and_empty_pem() {
        let (_server, port, certificate) = start_echo_tls_server();
        let pem = format!(
            "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----\n",
            String::from_utf8_lossy(&gf_encoding::gbase64::encode(&certificate))
        );
        let mut config = TlsClientConfig::new();
        config.add_root_certificate_pem(&pem).expect("pem");
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &config,
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        conn.send(b"pong", None).expect("send");
        assert_eq!(conn.recv(4, None).expect("recv"), b"pong");
        conn.close().expect("close");
        let mut config = TlsClientConfig::new();
        assert!(config.add_root_certificate_pem("nothing").is_err());
    }

    #[test]
    fn recv_until_close_and_short_lengths() {
        let (_server, port, certificate) = start_echo_tls_server();
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &client_config(&certificate),
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        conn.send(b"ping", None).expect("send");
        // length == 0 returns after the first read.
        assert_eq!(conn.recv(0, None).expect("recv"), b"ping");
        let _ = conn.close();
    }
}
