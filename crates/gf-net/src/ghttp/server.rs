//! Port of the `net/ghttp` server (`ghttp_server*.go` basics).
//!
//! The server binds a TCP listener (through `std::net` like [`crate::gtcp`]),
//! parses one HTTP/1.1 request per connection with
//! [`crate::ghttp::message::Request`], runs the matched handler chain from
//! [`crate::ghttp::router::Router`] and writes the response back (ADR-067).
//!
//! Not ported yet: keep-alive reuse, TLS, graceful restart, per-domain
//! routing, route hooks, static file serving and the admin/swagger APIs.

use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use gf_errors::gerror::{self, Error};

use super::message::Request;
use super::router::{Context, Handler, Router, internal_error, not_found};

/// The server state shared with the accept loop and close.
struct ServerInner {
    address: Mutex<String>,
    router: Arc<Router>,
    middlewares: RwLock<Vec<Handler>>,
    local: Mutex<Option<SocketAddr>>,
    closed: AtomicBool,
}

/// The HTTP server, like GoFrame's `Server`.
#[derive(Clone)]
pub struct Server {
    inner: Arc<ServerInner>,
}

impl Default for Server {
    fn default() -> Server {
        Server::new()
    }
}

impl Server {
    /// Creates a server with an empty address (any free port when run), like
    /// `New`.
    pub fn new() -> Server {
        Server::with_address("")
    }

    /// Creates a server listening on `address` when run.
    pub fn with_address(address: &str) -> Server {
        Server {
            inner: Arc::new(ServerInner {
                address: Mutex::new(address.to_owned()),
                router: Arc::new(Router::new()),
                middlewares: RwLock::new(Vec::new()),
                local: Mutex::new(None),
                closed: AtomicBool::new(false),
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

    /// The router.
    pub fn router(&self) -> Arc<Router> {
        Arc::clone(&self.inner.router)
    }

    /// Registers a handler, like `BindHandler`.
    pub fn bind_handler(&self, pattern: &str, handler: Handler) {
        self.inner.router.bind(pattern, handler);
    }

    /// Adds a global middleware, like `BindMiddleware`.
    pub fn bind_middleware(&self, middleware: Handler) {
        self.inner
            .middlewares
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(middleware);
    }

    /// Creates a route group, like `Group`.
    pub fn group(&self, prefix: &str) -> super::router::Group {
        super::router::Group::new(Arc::clone(&self.inner.router), prefix)
    }

    /// Reports whether the server is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::SeqCst)
    }

    /// Closes the server, unblocking `run`, like `Close`.
    pub fn close(&self) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Serves one parsed request without a network connection (used by tests
    /// and by `run`).
    pub fn serve(&self, request: Request) -> super::message::Response {
        let method = request.method.clone();
        let path = request.path.clone();
        let matched = self.inner.router.lookup(&method, &path);
        let Some((route_handlers, params)) = matched else {
            return not_found();
        };
        let mut handlers = self
            .inner
            .middlewares
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        handlers.extend(route_handlers);
        let mut context = Context::new(request, handlers);
        context.params = params;
        match context.proceed() {
            Ok(()) => context.response,
            Err(error) => internal_error(&error),
        }
    }

    /// Starts listening and blocks until closed, like GoFrame's `Run`.
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
        let listener = TcpListener::bind(&bind_address).map_err(|error| {
            gerror::new(format!("listen HTTP at \"{bind_address}\" failed: {error}"))
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
                    if let Err(error) = stream.set_nonblocking(false) {
                        return Err(gerror::new(format!(
                            "reset accepted stream to blocking failed: {error}"
                        )));
                    }
                    let server = self.clone();
                    std::thread::spawn(move || {
                        let _ = server.serve_stream(stream);
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => {
                    if self.is_closed() {
                        break;
                    }
                    return Err(gerror::new(format!(
                        "accept HTTP connection failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }

    /// Serves one accepted connection.
    fn serve_stream(&self, mut stream: TcpStream) -> Result<(), Error> {
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|error| gerror::new(format!("set read timeout failed: {error}")))?;
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| gerror::new(format!("clone stream failed: {error}")))?,
        );
        let request = match Request::parse(&mut reader) {
            Ok(request) => request,
            Err(_) => {
                let response = super::router::internal_error(&gerror::new("invalid HTTP request"));
                response.write_to(&mut stream)?;
                return Ok(());
            }
        };
        let response = self.serve(request);
        response.write_to(&mut stream)?;
        stream
            .flush()
            .map_err(|error| gerror::new(format!("flush response failed: {error}")))
    }

    /// The listened address with the concrete port substituted, like
    /// `GetListenedAddress`.
    pub fn listened_address(&self) -> String {
        let address = self.address();
        let port = self.listened_port();
        if address.is_empty() {
            return format!("127.0.0.1:{port}");
        }
        if !address.contains(":0") {
            return address;
        }
        if address == ":0" {
            return format!("127.0.0.1:{port}");
        }
        address.replace(":0", &format!(":{port}"))
    }

    /// The listened port, or `-1` when not running, like `GetListenedPort`.
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    use super::*;
    use crate::ghttp::message::Header;
    use crate::gtcp;

    fn text_handler(text: &'static str) -> Handler {
        Arc::new(move |context: &mut Context| {
            context.response.set_text(text);
            Ok(())
        })
    }

    fn get_request(path: &str) -> Request {
        let mut headers = Header::new();
        headers.set("Host", "localhost");
        Request {
            method: "GET".to_owned(),
            path: path.to_owned(),
            headers,
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        }
    }

    #[test]
    fn serve_matches_routes_and_returns_404() {
        let server = Server::new();
        server.bind_handler("/hello", text_handler("world"));
        server.bind_handler(
            "/user/:id",
            Arc::new(|context: &mut Context| {
                let id = context.param("id").unwrap_or_default().to_owned();
                context.response.set_text(&format!("user:{id}"));
                Ok(())
            }),
        );
        let response = server.serve(get_request("/hello"));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"world");
        let response = server.serve(get_request("/user/42"));
        assert_eq!(response.body, b"user:42");
        let response = server.serve(get_request("/missing"));
        assert_eq!(response.status, 404);
        assert_eq!(response.body, b"404 page not found\n");
    }

    #[test]
    fn global_middleware_runs_before_handler() {
        let server = Server::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        server.bind_middleware(Arc::new(move |context: &mut Context| {
            counter.fetch_add(1, Ordering::SeqCst);
            context.response.headers.set("X-Middleware", "1");
            context.proceed()
        }));
        server.bind_handler("/path", text_handler("ok"));
        let response = server.serve(get_request("/path"));
        assert_eq!(response.body, b"ok");
        assert_eq!(response.headers.get("X-Middleware"), Some("1"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn handler_errors_become_500() {
        let server = Server::new();
        server.bind_handler(
            "/fail",
            Arc::new(|_context: &mut Context| Err(gerror::new("boom"))),
        );
        let response = server.serve(get_request("/fail"));
        assert_eq!(response.status, 500);
        assert!(String::from_utf8_lossy(&response.body).contains("boom"));
    }

    #[test]
    fn groups_and_router_work_through_server() {
        let server = Server::new();
        let group = server.group("/api");
        group.bind_handler("GET:/ping", text_handler("pong"));
        assert_eq!(server.serve(get_request("/api/ping")).body, b"pong");
        assert_eq!(server.serve(get_request("/api/other")).status, 404);
    }

    #[test]
    fn run_serves_over_tcp() {
        let server = Server::new();
        server.bind_handler("/hello", text_handler("world"));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_ne!(server.listened_port(), -1);
        let address = server.listened_address();
        let conn = gtcp::Conn::new(&address, Some(Duration::from_secs(5))).expect("conn");
        conn.set_deadline(Some(Duration::from_secs(5)))
            .expect("deadline");
        conn.send(b"GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n", None)
            .expect("send");
        let mut response = Vec::new();
        loop {
            match conn.recv(-1, None) {
                Ok(chunk) if !chunk.is_empty() => response.extend_from_slice(&chunk),
                Ok(_) => break,
                Err(_) => break,
            }
        }
        let text = String::from_utf8_lossy(&response);
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.ends_with("\r\n\r\nworld"), "{text}");
        conn.close().expect("close");
        server.close().expect("stop");
    }
}
