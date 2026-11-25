//! TLS support for `net/gtcp` — the client connection lane.
//!
//! GoFrame's `gtcp.Conn` transparently wraps `tls.Conn` when the server uses
//! TLS. The Rust port keeps the plain [`super::Conn`] unchanged and adds
//! [`TlsConn`], a rustls-backed connection with the same `send`/`recv`
//! contract (ADR-086):
//!
//! - [`TlsClientConfig`] with the Mozilla roots plus custom PEM/DER roots;
//! - [`TlsConn`] with `send`, `recv`, `set_deadline`, `close`, `remote_addr`
//!   and `local_addr`.
//!
//! Not ported: TLS for `gtcp.Server` (its handler receives a plain [`super::Conn`]),
//! server-side certificate configuration, client certificates and ALPN.

use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use gf_errors::gerror::{self, Error};
use rustls::pki_types::{CertificateDer, ServerName};
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

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

/// A TLS client connection, like Go's `tls.Conn`.
pub struct TlsConn {
    stream: StreamOwned<ClientConnection, TcpStream>,
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
        stream
            .set_read_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set TLS read timeout failed: {error}")))?;
        stream
            .set_write_timeout(Some(timeout))
            .map_err(|error| gerror::new(format!("set TLS write timeout failed: {error}")))?;
        Ok(TlsConn {
            stream: StreamOwned::new(connection, stream),
            buffer_wait_recv: RECEIVE_ALL_WAIT_TIMEOUT,
        })
    }

    /// The local address.
    pub fn local_addr(&self) -> Option<SocketAddr> {
        self.stream.sock.local_addr().ok()
    }

    /// The remote address.
    pub fn remote_addr(&self) -> Option<SocketAddr> {
        self.stream.sock.peer_addr().ok()
    }

    /// Sets the read/write deadline.
    pub fn set_deadline(&mut self, timeout: Option<Duration>) -> Result<(), Error> {
        self.stream
            .sock
            .set_read_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))?;
        self.stream
            .sock
            .set_write_timeout(timeout)
            .map_err(|error| gerror::new(format!("set deadline failed: {error}")))
    }

    /// Writes `data`, retrying on failure, like `Send`.
    pub fn send(&mut self, data: &[u8], retry: Option<Retry>) -> Result<(), Error> {
        let mut retry = retry;
        loop {
            match std::io::Write::write_all(&mut self.stream, data) {
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
                self.stream
                    .sock
                    .set_read_timeout(Some(self.buffer_wait_recv))
                    .map_err(|error| gerror::new(format!("set read deadline failed: {error}")))?;
            }
            if index >= buffer.len() && length <= 0 {
                buffer.resize(buffer.len() + DEFAULT_READ_BUFFER_SIZE, 0);
            }
            match std::io::Read::read(&mut self.stream, &mut buffer[index..]) {
                Ok(0) => {
                    if buffer_wait {
                        let _ = self.stream.sock.set_read_timeout(None);
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
                        self.stream.sock.set_read_timeout(None).map_err(|error| {
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
        self.stream.conn.send_close_notify();
        let _ = std::io::Write::flush(&mut self.stream);
        self.stream
            .sock
            .shutdown(std::net::Shutdown::Both)
            .map_err(|error| gerror::new(format!("close TLS connection failed: {error}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Starts a TLS echo server; returns its port and the certificate DER.
    fn start_echo_tls_server() -> (u16, Vec<u8>) {
        use rcgen::generate_simple_self_signed;

        let certified = generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        let certificate = certified.cert.der().clone();
        let key = rustls::pki_types::PrivatePkcs8KeyDer::from(certified.key_pair.serialize_der());
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.clone()], key.into())
            .expect("server config");
        let config = Arc::new(config);
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                let config = Arc::clone(&config);
                std::thread::spawn(move || {
                    let Ok(connection) = rustls::ServerConnection::new(config) else {
                        return;
                    };
                    let mut stream = StreamOwned::new(connection, stream);
                    let mut buffer = [0u8; 4];
                    let mut read = 0;
                    while read < buffer.len() {
                        match std::io::Read::read(&mut stream, &mut buffer[read..]) {
                            Ok(0) | Err(_) => return,
                            Ok(size) => read += size,
                        }
                    }
                    let _ = std::io::Write::write_all(&mut stream, &buffer);
                    let _ = std::io::Write::flush(&mut stream);
                    stream.conn.send_close_notify();
                });
            }
        });
        (port, certificate.as_ref().to_vec())
    }

    #[test]
    fn echo_round_trip() {
        let (port, certificate) = start_echo_tls_server();
        let mut config = TlsClientConfig::new();
        config.add_root_certificate_der(&certificate).expect("root");
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
    fn untrusted_root_fails_to_handshake() {
        let (port, _certificate) = start_echo_tls_server();
        let config = TlsClientConfig::new();
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &config,
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        assert!(conn.send(b"ping", None).is_err());
    }

    #[test]
    fn pem_roots_and_empty_pem() {
        let (port, certificate) = start_echo_tls_server();
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
        let (port, certificate) = start_echo_tls_server();
        let mut config = TlsClientConfig::new();
        config.add_root_certificate_der(&certificate).expect("root");
        let mut conn = TlsConn::new(
            &format!("127.0.0.1:{port}"),
            "localhost",
            &config,
            Some(Duration::from_secs(5)),
        )
        .expect("connect");
        conn.send(b"ping", None).expect("send");
        // length == 0 returns after the first read.
        assert_eq!(conn.recv(0, None).expect("recv"), b"ping");
        let _ = conn.close();
    }
}
