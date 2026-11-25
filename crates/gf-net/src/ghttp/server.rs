//! Port of the `net/ghttp` server (`ghttp_server*.go` basics).
//!
//! The server binds a TCP listener (through `std::net` like [`crate::gtcp`]),
//! parses one HTTP/1.1 request per connection with
//! [`crate::ghttp::message::Request`], runs the matched handler chain from
//! [`crate::ghttp::router::Router`] and writes the response back (ADR-067).
//!
//! Not ported yet: keep-alive reuse, TLS, graceful restart, per-domain
//! routing, route hooks, static file serving and the admin/swagger APIs.

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use gf_errors::gerror::{self, Error};

use super::file_server::{self as static_files, StaticConfig};
use super::message::Request;
use super::openapi::OpenApiConfig;
use super::router::{Context, Handler, Router, internal_error, not_found};

/// The server is stopped.
pub const SERVER_STATUS_STOPPED: i32 = 0;
/// The server is running.
pub const SERVER_STATUS_RUNNING: i32 = 1;

/// The named server registry, like GoFrame's `serverMapping`.
fn server_registry() -> &'static Mutex<HashMap<String, Server>> {
    static SERVERS: OnceLock<Mutex<HashMap<String, Server>>> = OnceLock::new();
    SERVERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns a named server, creating it on first use, like `g.Server(name)`.
pub fn get_server(name: Option<&str>) -> Server {
    let name = name.filter(|name| !name.is_empty()).unwrap_or("default");
    let mut registry = server_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    registry.entry(name.to_owned()).or_default().clone()
}

/// Registers a server under `name`, replacing any existing one.
pub fn set_server(name: &str, server: Server) {
    server_registry()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(name.to_owned(), server);
}

/// The server state shared with the accept loop and close.
struct ServerInner {
    address: Mutex<String>,
    router: Arc<Router>,
    middlewares: RwLock<Vec<Handler>>,
    local: Mutex<Option<SocketAddr>>,
    closed: AtomicBool,
    status: AtomicI32,
    in_flight: AtomicUsize,
    statics: Mutex<StaticConfig>,
    status_handlers: Mutex<HashMap<i32, Vec<Handler>>>,
    session_manager: Mutex<Option<Arc<gf_os::gsession::Manager>>>,
    session_cookie_name: Mutex<String>,
    openapi: Mutex<Option<OpenApiConfig>>,
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
                status: AtomicI32::new(SERVER_STATUS_STOPPED),
                in_flight: AtomicUsize::new(0),
                statics: Mutex::new(StaticConfig::default()),
                status_handlers: Mutex::new(HashMap::new()),
                session_manager: Mutex::new(None),
                session_cookie_name: Mutex::new("gfsessionid".to_owned()),
                openapi: Mutex::new(None),
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

    /// Sets the static document root, like `SetServerRoot`.
    pub fn set_server_root(&self, root: &str) -> Result<(), Error> {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .set_server_root(root)
    }

    /// Adds a static search path, like `AddSearchPath`.
    pub fn add_search_path(&self, path: &str) -> Result<(), Error> {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .add_search_path(path)
    }

    /// Maps a URI prefix to a directory, like `AddStaticPath`.
    pub fn add_static_path(&self, prefix: &str, path: &str) -> Result<(), Error> {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .add_static_path(prefix, path)
    }

    /// Sets the index file names, like `SetIndexFiles`.
    pub fn set_index_files(&self, index_files: Vec<String>) {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .index_files = index_files;
    }

    /// Enables or disables directory listings, like `SetIndexFolder`.
    pub fn set_index_folder(&self, enabled: bool) {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .index_folder = enabled;
    }

    /// Enables or disables the static file service, like
    /// `SetFileServerEnabled`.
    pub fn set_file_server_enabled(&self, enabled: bool) {
        self.inner
            .statics
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .enabled = enabled;
    }

    /// Registers the OpenAPI document and Swagger UI paths, like the
    /// GoFrame OpenAPI wiring.
    pub fn set_openapi(&self, config: OpenApiConfig) {
        *self
            .inner
            .openapi
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(config);
    }

    /// Returns the server status, like `Status`.
    pub fn status(&self) -> i32 {
        self.inner.status.load(Ordering::SeqCst)
    }

    /// Registers a handler for a response status, like `BindStatusHandler`.
    pub fn bind_status_handler(&self, status: i32, handler: Handler) {
        self.inner
            .status_handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(status)
            .or_default()
            .push(handler);
    }

    /// Registers status handlers from a map, like `BindStatusHandlerByMap`.
    pub fn bind_status_handler_by_map(&self, handlers: HashMap<i32, Handler>) {
        for (status, handler) in handlers {
            self.bind_status_handler(status, handler);
        }
    }

    /// Stops accepting connections and waits up to `timeout` for in-flight
    /// requests, like `Shutdown`.
    pub fn shutdown(&self, timeout: Duration) -> Result<(), Error> {
        self.inner.closed.store(true, Ordering::SeqCst);
        let deadline = Instant::now() + timeout;
        while self.inner.in_flight.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(())
    }

    /// Applies registered status handlers to `response`.
    fn apply_status_handlers(
        &self,
        request: Request,
        response: super::message::Response,
    ) -> super::message::Response {
        let handlers = self
            .inner
            .status_handlers
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&response.status)
            .cloned()
            .unwrap_or_default();
        if handlers.is_empty() {
            return response;
        }
        let mut context = Context::new(request, handlers);
        context.response = response;
        match context.proceed() {
            Ok(()) => context.response,
            Err(error) => internal_error(&error),
        }
    }

    /// Sets the session manager, like GoFrame's `SetSessionStorage` wiring.
    pub fn set_session_manager(&self, manager: Arc<gf_os::gsession::Manager>) {
        *self
            .inner
            .session_manager
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(manager);
    }

    /// Sets the session cookie name (default `gfsessionid`).
    pub fn set_session_cookie_name(&self, name: &str) {
        *self
            .inner
            .session_cookie_name
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = name.to_owned();
    }

    /// Attaches the request session to `context` and persists it afterwards.
    fn open_session(&self, request: &Request) -> Option<gf_os::gsession::Session> {
        let manager = self
            .inner
            .session_manager
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()?;
        let cookie_name = self
            .inner
            .session_cookie_name
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let session_id = request.cookies.get(&cookie_name);
        Some(manager.new_session(session_id))
    }

    /// Closes `session`, writing the session cookie when the id changed.
    fn close_session(
        &self,
        mut session: gf_os::gsession::Session,
        request: &Request,
        response: &mut super::message::Response,
    ) {
        let cookie_name = self
            .inner
            .session_cookie_name
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        let previous_id = request.cookies.get(&cookie_name).map(str::to_owned);
        if session.close().is_err() {
            return;
        }
        let Ok(session_id) = session.id() else {
            return;
        };
        if session_id.is_empty() {
            return;
        }
        if previous_id.as_deref() != Some(session_id.as_str()) {
            let cookie = super::message::Cookie::new(&cookie_name, &session_id)
                .path("/")
                .http_only(true);
            response.add_cookie(&cookie);
        }
    }

    /// Serves one parsed request without a network connection (used by tests
    /// and by `run`).
    pub fn serve(&self, request: Request) -> super::message::Response {
        let method = request.method.clone();
        let path = request.path.clone();
        let context_request = request.clone();
        if let Some(response) = self
            .inner
            .openapi
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .and_then(|config| config.serve(&method, &path))
        {
            return self.apply_status_handlers(context_request, response);
        }
        let matched = self.inner.router.lookup(&method, &path);
        let Some((route_handlers, params)) = matched else {
            let response = {
                let config = self
                    .inner
                    .statics
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match static_files::serve(&config, &request.path) {
                    Some(result) => static_files::apply(result),
                    None => not_found(),
                }
            };
            return self.apply_status_handlers(context_request, response);
        };
        let mut handlers = self
            .inner
            .middlewares
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        handlers.extend(route_handlers);
        let session = self.open_session(&request);
        let mut context = Context::new(request, handlers);
        context.params = params;
        context.session = session;
        let mut response = match context.proceed() {
            Ok(()) => context.response,
            Err(error) => internal_error(&error),
        };
        if let Some(session) = context.session.take() {
            self.close_session(session, &context_request, &mut response);
        }
        self.apply_status_handlers(context_request, response)
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
        self.inner
            .status
            .store(SERVER_STATUS_RUNNING, Ordering::SeqCst);
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
                    self.inner
                        .status
                        .store(SERVER_STATUS_STOPPED, Ordering::SeqCst);
                    return Err(gerror::new(format!(
                        "accept HTTP connection failed: {error}"
                    )));
                }
            }
        }
        self.inner
            .status
            .store(SERVER_STATUS_STOPPED, Ordering::SeqCst);
        Ok(())
    }

    /// Serves one accepted connection, honouring HTTP/1.1 keep-alive until the
    /// client asks to close or the server stops.
    fn serve_stream(&self, mut stream: TcpStream) -> Result<(), Error> {
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|error| gerror::new(format!("set read timeout failed: {error}")))?;
        let mut reader = BufReader::new(
            stream
                .try_clone()
                .map_err(|error| gerror::new(format!("clone stream failed: {error}")))?,
        );
        loop {
            if self.is_closed() {
                break;
            }
            let request = match Request::parse(&mut reader) {
                Ok(request) => request,
                Err(_) => break,
            };
            let keep_alive = request
                .headers
                .get("Connection")
                .map(|value| !value.eq_ignore_ascii_case("close"))
                .unwrap_or(true);
            self.inner.in_flight.fetch_add(1, Ordering::SeqCst);
            let response = self.serve(request);
            self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            response.write_to(&mut stream)?;
            stream
                .flush()
                .map_err(|error| gerror::new(format!("flush response failed: {error}")))?;
            if !keep_alive {
                break;
            }
        }
        Ok(())
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
    fn static_files_are_served_when_no_route_matches() {
        let root = std::env::temp_dir().join(format!(
            "gf-rust-ghttp-server-static-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root");
        std::fs::write(root.join("hello.txt"), "static hello").expect("file");
        let server = Server::new();
        server
            .set_server_root(&root.to_string_lossy())
            .expect("server root");
        server.bind_handler("/route", text_handler("route"));
        assert_eq!(server.serve(get_request("/route")).body, b"route");
        let response = server.serve(get_request("/hello.txt"));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"static hello");
        assert_eq!(
            response.headers.get("Content-Type"),
            Some("text/plain; charset=utf-8")
        );
        assert_eq!(server.serve(get_request("/missing.txt")).status, 404);
        let _ = std::fs::remove_dir_all(&root);
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
        conn.send(
            b"GET /hello HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            None,
        )
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

    /// Reads one framed HTTP response (headers + Content-Length body).
    fn read_response(conn: &gtcp::Conn) -> String {
        let mut buffer = Vec::new();
        loop {
            match conn.recv(-1, None) {
                Ok(chunk) if !chunk.is_empty() => {
                    buffer.extend_from_slice(&chunk);
                    if let Some(end) = buffer
                        .windows(4)
                        .position(|window| window == b"\\r\\n\\r\\n")
                    {
                        let head = String::from_utf8_lossy(&buffer[..end]).into_owned();
                        let length = head
                            .lines()
                            .find_map(|line| {
                                line.strip_prefix("Content-Length:")
                                    .and_then(|value| value.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if buffer.len() >= end + 4 + length {
                            return String::from_utf8_lossy(&buffer).into_owned();
                        }
                    }
                }
                _ => break,
            }
        }
        String::from_utf8_lossy(&buffer).into_owned()
    }

    #[test]
    fn keep_alive_serves_multiple_requests() {
        let server = Server::new();
        server.bind_handler("/one", text_handler("one"));
        server.bind_handler("/two", text_handler("two"));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let conn = gtcp::Conn::new(&server.listened_address(), Some(Duration::from_secs(5)))
            .expect("conn");
        conn.set_deadline(Some(Duration::from_millis(500)))
            .expect("deadline");
        conn.send(b"GET /one HTTP/1.1\r\nHost: localhost\r\n\r\n", None)
            .expect("first");
        let first = read_response(&conn);
        assert!(first.ends_with("one"), "{first}");
        conn.send(b"GET /two HTTP/1.1\r\nHost: localhost\r\n\r\n", None)
            .expect("second");
        let second = read_response(&conn);
        assert!(second.ends_with("two"), "{second}");
        conn.send(
            b"GET /one HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            None,
        )
        .expect("third");
        let third = read_response(&conn);
        assert!(third.ends_with("one"), "{third}");
        conn.close().expect("close");
        server.close().expect("stop");
    }

    #[test]
    fn status_handlers_override_bodies() {
        let server = Server::new();
        server.bind_status_handler(
            404,
            Arc::new(|context: &mut Context| {
                context.response.set_text("custom 404");
                Ok(())
            }),
        );
        server.bind_status_handler(
            404,
            Arc::new(|context: &mut Context| {
                context.response.body.extend_from_slice(b"!");
                Ok(())
            }),
        );
        server.bind_handler(
            "/fail",
            Arc::new(|context: &mut Context| {
                context.response = crate::ghttp::message::Response::new(502);
                context.response.set_text("bad gateway");
                Ok(())
            }),
        );
        server.bind_status_handler(
            502,
            Arc::new(|context: &mut Context| {
                context.response.set_text("custom 502");
                Ok(())
            }),
        );
        let response = server.serve(get_request("/missing"));
        assert_eq!(response.status, 404);
        assert_eq!(response.body, b"custom 404!");
        let response = server.serve(get_request("/fail"));
        assert_eq!(response.status, 502);
        assert_eq!(response.body, b"custom 502");
    }

    #[test]
    fn openapi_paths_are_served() {
        use crate::goai::{Info, OpenApiV3};

        let server = Server::new();
        let document = OpenApiV3 {
            info: Info {
                title: "User API".to_owned(),
                version: "1.0.0".to_owned(),
                ..Info::default()
            },
            ..OpenApiV3::default()
        };
        server.set_openapi(OpenApiConfig::new(document));
        let response = server.serve(get_request("/api.json"));
        assert_eq!(response.status, 200);
        assert!(
            response.text().contains("\"User API\""),
            "{}",
            response.text()
        );
        let response = server.serve(get_request("/swagger"));
        assert_eq!(response.status, 200);
        assert!(response.text().contains("/api.json"));
        assert_eq!(server.serve(get_request("/missing")).status, 404);
    }

    #[test]
    fn session_persists_across_requests() {
        use gf_os::gsession::{Manager, StorageMemory};

        let manager = Arc::new(
            Manager::new(
                Duration::from_secs(60),
                Some(Arc::new(StorageMemory::new())),
            )
            .expect("manager"),
        );
        let server = Server::new();
        server.set_session_manager(manager);
        server.bind_handler(
            "POST:/login",
            Arc::new(|context: &mut Context| {
                if let Some(session) = context.session_mut() {
                    session
                        .set("user", gf_core::Value::String("john".to_owned()))
                        .expect("set session");
                }
                context.response.set_text("ok");
                Ok(())
            }),
        );
        server.bind_handler(
            "GET:/who",
            Arc::new(|context: &mut Context| {
                let value = context
                    .session_mut()
                    .and_then(|session| session.get("user", None).ok().flatten())
                    .map(|value| value.string())
                    .unwrap_or_default();
                context.response.set_text(&value);
                Ok(())
            }),
        );
        let mut login = get_request("/login");
        login.method = "POST".to_owned();
        let response = server.serve(login);
        assert_eq!(response.status, 200);
        let cookie = response.cookies().first().cloned().expect("session cookie");
        assert_eq!(cookie.name, "gfsessionid");
        assert_eq!(cookie.path, "/");
        assert!(cookie.http_only);

        let mut who = get_request("/who");
        who.cookies =
            crate::ghttp::message::Cookies::parse(&format!("{}={}", cookie.name, cookie.value));
        let response = server.serve(who);
        assert_eq!(response.body, b"john");
    }

    #[test]
    fn status_transitions_and_shutdown() {
        let server = Server::new();
        assert_eq!(server.status(), SERVER_STATUS_STOPPED);
        server.bind_handler("/hello", text_handler("world"));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(server.status(), SERVER_STATUS_RUNNING);
        server.shutdown(Duration::from_secs(1)).expect("shutdown");
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while server.status() != SERVER_STATUS_STOPPED && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(server.status(), SERVER_STATUS_STOPPED);
    }
}
