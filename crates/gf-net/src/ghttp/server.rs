//! Port of the `net/ghttp` server (`ghttp_server*.go` basics).
//!
//! The server binds a TCP listener (through `std::net` like [`crate::gtcp`]),
//! parses one HTTP/1.1 request per connection with
//! [`crate::ghttp::message::Request`], runs the matched handler chain from
//! [`crate::ghttp::router::Router`] and writes the response back (ADR-067).
//!
//! Not ported yet: graceful restart/fd passing, HTTP/2 and the
//! process-manager graceful lane (ADR-067); per-domain routing lives in
//! [`crate::ghttp::router`] (ADR-180) and admin endpoints in
//! [`crate::ghttp::admin`] (ADR-185).

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, Instant};

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_util::gconv;

use super::file_server::{self as static_files, StaticConfig};
use super::message::Request;
use super::openapi::OpenApiConfig;
use super::router::{Context, Handler, Router, internal_error, not_found, pattern_matches};

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
    cookie: Mutex<CookieConfig>,
    max_header_bytes: Mutex<Option<usize>>,
    openapi: Mutex<Option<OpenApiConfig>>,
    client_max_body_size: Mutex<Option<usize>>,
    hooks: Mutex<HashMap<HookName, Vec<Handler>>>,
    pattern_hooks: Mutex<Vec<(String, HookName, Handler)>>,
    tls: Mutex<Option<Arc<rustls::ServerConfig>>>,
    reuse_port: AtomicBool,
}

/// Cookie attributes applied to server-issued session cookies, like
/// GoFrame's server cookie config (ADR-144).
#[derive(Clone, Debug)]
pub struct CookieConfig {
    /// The cookie domain (empty for host-only).
    pub domain: String,
    /// The cookie path.
    pub path: String,
    /// The max age in seconds (0 for a session cookie).
    pub max_age: i64,
    /// The `Secure` flag.
    pub secure: bool,
    /// The `HttpOnly` flag.
    pub http_only: bool,
    /// The `SameSite` attribute (`Lax`/`Strict`/`None`).
    pub same_site: String,
}

impl Default for CookieConfig {
    fn default() -> CookieConfig {
        CookieConfig {
            domain: String::new(),
            path: "/".to_owned(),
            max_age: 0,
            secure: false,
            http_only: true,
            same_site: String::new(),
        }
    }
}

/// The server hook points, like GoFrame's `HookName` (ADR-127).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HookName {
    /// Runs before the middlewares and the route handler.
    BeforeServe,
    /// Runs after the route handler.
    AfterServe,
    /// Runs just before the response output is finalized.
    BeforeOutput,
    /// Runs last, after the output hooks.
    AfterOutput,
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
                cookie: Mutex::new(CookieConfig::default()),
                max_header_bytes: Mutex::new(None),
                openapi: Mutex::new(None),
                client_max_body_size: Mutex::new(None),
                hooks: Mutex::new(HashMap::new()),
                pattern_hooks: Mutex::new(Vec::new()),
                tls: Mutex::new(None),
                reuse_port: AtomicBool::new(false),
            }),
        }
    }

    /// Enables `SO_REUSEPORT` binding on Unix so a new process can bind the
    /// same address while the old one drains, like the socket option GoFrame's
    /// graceful restart relies on (ADR-188). Ignored on non-Unix platforms.
    pub fn set_reuse_port(&self, enabled: bool) {
        self.inner.reuse_port.store(enabled, Ordering::SeqCst);
    }

    /// Whether `SO_REUSEPORT` binding is enabled.
    pub fn reuse_port(&self) -> bool {
        self.inner.reuse_port.load(Ordering::SeqCst)
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

    /// Binds a global hook handler for `hook`, like `BindHookHandler`
    /// without a route pattern (ADR-127).
    pub fn bind_hook_handler(&self, hook: HookName, handler: Handler) {
        self.inner
            .hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(hook)
            .or_default()
            .push(handler);
    }

    /// Binds a hook handler for routes matching `pattern`, like GoFrame's
    /// `BindHookHandler(pattern, hook, handler)` (ADR-129).
    pub fn bind_hook_handler_for(&self, pattern: &str, hook: HookName, handler: Handler) {
        self.inner
            .pattern_hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push((pattern.to_owned(), hook, handler));
    }

    /// Collects the global and pattern-matching hook handlers for `hook`.
    fn hook_handlers(&self, hook: HookName, path: &str) -> Vec<Handler> {
        let mut handlers = self
            .inner
            .hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&hook)
            .cloned()
            .unwrap_or_default();
        let pattern_hooks = self
            .inner
            .pattern_hooks
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        for (pattern, hook_name, handler) in pattern_hooks {
            if hook_name == hook && pattern_matches(&pattern, path) {
                handlers.push(handler);
            }
        }
        handlers
    }

    /// Returns every registered route as `(methods, pattern)`, like
    /// GoFrame's `GetRoutes` (ADR-143).
    pub fn routes(&self) -> Vec<(Vec<String>, String)> {
        self.inner.router.route_list()
    }

    /// Limits the request body size in bytes, like GoFrame's
    /// `ClientMaxBodySize` (ADR-125); exceeding it returns 413.
    pub fn set_client_max_body_size(&self, size: usize) {
        *self
            .inner
            .client_max_body_size
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(size);
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

    /// Applies a server configuration map, like `SetConfigWithMap` (a subset:
    /// `address`, `serverRoot`, `indexFiles`, `indexFolder`, `staticPaths`,
    /// `openapiPath`, `swaggerPath`); unknown keys are ignored.
    pub fn set_config_with_map(&self, config: &gf_core::Value) -> Result<(), Error> {
        let gf_core::Value::Map(map) = config else {
            return Err(gerror::new_code(
                &gcode::CODE_INVALID_PARAMETER,
                &["server config must be a map"],
            ));
        };
        for (key, value) in map {
            match key.to_ascii_lowercase().as_str() {
                "address" | "addr" => self.set_address(&gconv::string(value)),
                "serverroot" => {
                    let root = gconv::string(value);
                    if !root.is_empty() {
                        self.set_server_root(&root)?;
                    }
                }
                "indexfiles" => self.set_index_files(gconv::strings(value)),
                "indexfolder" => self.set_index_folder(gconv::bool_value(value)),
                "clientmaxbodysize" => {
                    let size = gconv::int64_value(value);
                    if size > 0 {
                        self.set_client_max_body_size(size as usize);
                    }
                }
                "staticpaths" => {
                    if let gf_core::Value::Map(entries) = value {
                        for (prefix, path) in entries {
                            self.add_static_path(prefix, &gconv::string(path))?;
                        }
                    }
                }
                "openapipath" | "swaggerpath" => {
                    let openapi_path = map
                        .get("openapiPath")
                        .or_else(|| map.get("openapipath"))
                        .map(gconv::string)
                        .unwrap_or_default();
                    let swagger_path = map
                        .get("swaggerPath")
                        .or_else(|| map.get("swaggerpath"))
                        .map(gconv::string)
                        .unwrap_or_default();
                    self.set_openapi_paths(&openapi_path, &swagger_path);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Overrides the OpenAPI document/UI paths when a document is registered.
    pub fn set_openapi_paths(&self, openapi_path: &str, swagger_path: &str) {
        let mut guard = self
            .inner
            .openapi
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(mut config) = guard.take() {
            if !openapi_path.is_empty() {
                config.openapi_path = openapi_path.to_owned();
            }
            if !swagger_path.is_empty() {
                config.swagger_path = swagger_path.to_owned();
            }
            *guard = Some(config);
        }
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

    /// Enables TLS with a PEM certificate chain and private key; `run` then
    /// serves HTTPS, like GoFrame's TLS configuration (ADR-084).
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

    /// Reports whether TLS is enabled.
    pub fn is_tls(&self) -> bool {
        self.inner
            .tls
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_some()
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

    /// Limits the request header block size in bytes, like
    /// `SetMaxHeaderBytes` (ADR-170).
    pub fn set_max_header_bytes(&self, bytes: usize) {
        *self
            .inner
            .max_header_bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(bytes);
    }

    /// The configured header limit, if any.
    pub fn max_header_bytes(&self) -> Option<usize> {
        *self
            .inner
            .max_header_bytes
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The effective header limit (`usize::MAX` when unset).
    fn header_limit(&self) -> usize {
        self.max_header_bytes().unwrap_or(usize::MAX)
    }

    /// Replaces the session cookie attributes, like the `SetCookie*`
    /// methods (ADR-144).
    pub fn set_cookie_config(&self, config: CookieConfig) {
        *self
            .inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = config;
    }

    /// The current cookie attributes.
    pub fn cookie_config(&self) -> CookieConfig {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Sets the cookie domain.
    pub fn set_cookie_domain(&self, domain: &str) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .domain = domain.to_owned();
    }

    /// The cookie domain.
    pub fn cookie_domain(&self) -> String {
        self.cookie_config().domain
    }

    /// Sets the cookie path.
    pub fn set_cookie_path(&self, path: &str) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .path = path.to_owned();
    }

    /// Sets the cookie max age (seconds, 0 for session cookies).
    pub fn set_cookie_max_age(&self, max_age: i64) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .max_age = max_age;
    }

    /// Sets the cookie `Secure` flag.
    pub fn set_cookie_secure(&self, secure: bool) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .secure = secure;
    }

    /// Sets the cookie `HttpOnly` flag.
    pub fn set_cookie_http_only(&self, http_only: bool) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .http_only = http_only;
    }

    /// Sets the cookie `SameSite` attribute.
    pub fn set_cookie_same_site(&self, same_site: &str) {
        self.inner
            .cookie
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .same_site = same_site.to_owned();
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
            let config = self
                .inner
                .cookie
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone();
            let mut cookie = super::message::Cookie::new(&cookie_name, &session_id)
                .path(if config.path.is_empty() {
                    "/"
                } else {
                    &config.path
                })
                .http_only(config.http_only);
            if !config.domain.is_empty() {
                cookie = cookie.domain(&config.domain);
            }
            if config.secure {
                cookie = cookie.secure(true);
            }
            if config.max_age != 0 {
                cookie = cookie.max_age(config.max_age);
            }
            if !config.same_site.is_empty() {
                cookie = cookie.same_site(&config.same_site);
            }
            response.add_cookie(&cookie);
        }
    }

    /// Serves one parsed request without a network connection (used by tests
    /// and by `run`).
    pub fn serve(&self, request: Request) -> super::message::Response {
        if let Some(max) = *self
            .inner
            .client_max_body_size
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            && request.body.len() > max
        {
            let mut response = super::message::Response::new(413);
            response.set_text("Request Entity Too Large");
            return response;
        }
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
        let host = request.host();
        let matched = self.inner.router.lookup(&method, &path, &host);
        let Some((route_handlers, params)) = matched else {
            let response = {
                let config = self
                    .inner
                    .statics
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                match static_files::serve(&config, &request.path) {
                    Some(result) => static_files::apply_conditional(result, &request.headers),
                    None => not_found(),
                }
            };
            return self.apply_status_handlers(context_request, response);
        };
        let mut handlers = self.hook_handlers(HookName::BeforeServe, &path);
        handlers.extend(
            self.inner
                .middlewares
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone(),
        );
        handlers.extend(route_handlers);
        for hook in [
            HookName::AfterServe,
            HookName::BeforeOutput,
            HookName::AfterOutput,
        ] {
            handlers.extend(self.hook_handlers(hook, &path));
        }
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

    /// Binds `address`, honoring [`Server::reuse_port`] on Unix (ADR-188).
    fn bind_listener(&self, address: &str) -> Result<TcpListener, Error> {
        #[cfg(unix)]
        if self.reuse_port() {
            return bind_reuse_port(address);
        }
        TcpListener::bind(address)
            .map_err(|error| gerror::new(format!("listen HTTP at \"{address}\" failed: {error}")))
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
        let listener = self.bind_listener(&bind_address)?;
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
                    let tls = self
                        .inner
                        .tls
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .clone();
                    std::thread::spawn(move || match tls {
                        Some(config) => {
                            let _ = server.serve_stream_tls(stream, config);
                        }
                        None => {
                            let _ = server.serve_stream(stream);
                        }
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

    /// Serves one accepted TLS connection, honouring HTTP/1.1 keep-alive.
    fn serve_stream_tls(
        &self,
        stream: TcpStream,
        config: Arc<rustls::ServerConfig>,
    ) -> Result<(), Error> {
        stream
            .set_read_timeout(Some(Duration::from_secs(30)))
            .map_err(|error| gerror::new(format!("set read timeout failed: {error}")))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(30)))
            .map_err(|error| gerror::new(format!("set write timeout failed: {error}")))?;
        let connection = rustls::ServerConnection::new(config)
            .map_err(|error| gerror::new(format!("start TLS server connection failed: {error}")))?;
        let peer_addr = stream
            .peer_addr()
            .map(|address| address.to_string())
            .unwrap_or_default();
        let mut reader = BufReader::new(rustls::StreamOwned::new(connection, stream));
        loop {
            if self.is_closed() {
                break;
            }
            let limit = self.header_limit();
            let mut request = match Request::parse_with_limit(&mut reader, limit) {
                Ok(request) => request,
                Err(error) => {
                    if error.text().contains("header too large") {
                        let inner = reader.get_mut();
                        let _ = inner.write_all(
                            b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        let _ = inner.flush();
                    }
                    break;
                }
            };
            request.remote_addr = peer_addr.clone();
            let keep_alive = request
                .headers
                .get("Connection")
                .map(|value| !value.eq_ignore_ascii_case("close"))
                .unwrap_or(true);
            self.inner.in_flight.fetch_add(1, Ordering::SeqCst);
            let response = self.serve(request);
            self.inner.in_flight.fetch_sub(1, Ordering::SeqCst);
            let inner = reader.get_mut();
            response.write_to(inner)?;
            inner
                .flush()
                .map_err(|error| gerror::new(format!("flush response failed: {error}")))?;
            if !keep_alive {
                break;
            }
        }
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
            let limit = self.header_limit();
            let mut request = match Request::parse_with_limit(&mut reader, limit) {
                Ok(request) => request,
                Err(error) => {
                    if error.text().contains("header too large") {
                        let _ = stream.write_all(
                            b"HTTP/1.1 431 Request Header Fields Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        );
                        let _ = stream.flush();
                    }
                    break;
                }
            };
            request.remote_addr = stream
                .peer_addr()
                .map(|address| address.to_string())
                .unwrap_or_default();
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

/// Binds `address` with `SO_REUSEPORT` (Unix only), the socket-level half of
/// a zero-downtime restart: a new process binds the same port and the old one
/// drains through [`Server::shutdown`] (ADR-188).
#[cfg(unix)]
fn bind_reuse_port(address: &str) -> Result<TcpListener, Error> {
    use socket2::{Domain, Protocol, Socket, Type};

    let socket_address: SocketAddr = address.parse().map_err(|error| {
        gerror::new(format!(
            "parse HTTP listen address \"{address}\" failed: {error}"
        ))
    })?;
    let domain = if socket_address.is_ipv6() {
        Domain::IPV6
    } else {
        Domain::IPV4
    };
    let socket = Socket::new(domain, Type::STREAM, Some(Protocol::TCP))
        .map_err(|error| gerror::new(format!("create HTTP listen socket failed: {error}")))?;
    socket
        .set_reuse_address(true)
        .map_err(|error| gerror::new(format!("set SO_REUSEADDR failed: {error}")))?;
    socket
        .set_reuse_port(true)
        .map_err(|error| gerror::new(format!("set SO_REUSEPORT failed: {error}")))?;
    socket
        .bind(&socket_address.into())
        .map_err(|error| gerror::new(format!("listen HTTP at \"{address}\" failed: {error}")))?;
    socket
        .listen(1024)
        .map_err(|error| gerror::new(format!("listen HTTP at \"{address}\" failed: {error}")))?;
    socket
        .set_nonblocking(true)
        .map_err(|error| gerror::new(format!("set listener non-blocking failed: {error}")))?;
    Ok(socket.into())
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
    fn routes_listing() {
        let server = Server::new();
        server.bind_handler("GET:/user/:id", text_handler("user"));
        server.bind_handler("POST:/api/*path", text_handler("api"));
        let routes = server.routes();
        assert!(routes.contains(&(vec!["GET".to_owned()], "/user/:id".to_owned())));
        assert!(routes.contains(&(vec!["POST".to_owned()], "/api/*path".to_owned())));
        assert_eq!(routes.len(), 2);
    }

    #[test]
    fn hooks_run_in_order() {
        let server = Server::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        for (hook, label) in [
            (HookName::BeforeServe, "before"),
            (HookName::AfterServe, "after"),
            (HookName::BeforeOutput, "output"),
            (HookName::AfterOutput, "after_output"),
        ] {
            let order = Arc::clone(&order);
            server.bind_hook_handler(
                hook,
                Arc::new(move |context: &mut Context| {
                    order.lock().unwrap().push(label);
                    if label == "before" {
                        context.response.headers.set("X-Hook", "1");
                    }
                    Ok(())
                }),
            );
        }
        let handler_order = Arc::clone(&order);
        server.bind_handler(
            "/hooked",
            Arc::new(move |context: &mut Context| {
                handler_order.lock().unwrap().push("handler");
                context.response.set_text("body");
                Ok(())
            }),
        );
        let response = server.serve(get_request("/hooked"));
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"body");
        assert_eq!(response.headers.get("X-Hook"), Some("1"));
        assert_eq!(
            *order.lock().unwrap(),
            vec!["before", "handler", "after", "output", "after_output"]
        );
    }

    #[test]
    fn pattern_hooks_match_routes() {
        let server = Server::new();
        server.bind_hook_handler_for(
            "/api/*",
            HookName::BeforeServe,
            Arc::new(|context: &mut Context| {
                context.response.headers.set("X-API", "1");
                Ok(())
            }),
        );
        server.bind_hook_handler_for(
            "/admin/*",
            HookName::AfterServe,
            Arc::new(|context: &mut Context| {
                context.response.headers.set("X-ADMIN", "1");
                Ok(())
            }),
        );
        server.bind_handler("/api/user", text_handler("api"));
        server.bind_handler("/other", text_handler("other"));
        server.bind_handler("/admin/panel", text_handler("admin"));

        let api = server.serve(get_request("/api/user"));
        assert_eq!(api.headers.get("X-API"), Some("1"));
        assert_eq!(api.headers.get("X-ADMIN"), None);

        let other = server.serve(get_request("/other"));
        assert_eq!(other.headers.get("X-API"), None);

        let admin = server.serve(get_request("/admin/panel"));
        assert_eq!(admin.headers.get("X-ADMIN"), Some("1"));
        assert_eq!(admin.headers.get("X-API"), None);
    }

    #[test]
    fn config_map_sets_body_limit() {
        let server = Server::new();
        let mut map = indexmap::IndexMap::new();
        map.insert("clientMaxBodySize".to_owned(), gf_core::Value::Int(4));
        server
            .set_config_with_map(&gf_core::Value::Map(map))
            .expect("config");
        server.bind_handler("/echo", text_handler("ok"));
        let mut request = get_request("/echo");
        request.method = "POST".to_owned();
        request.body = vec![b'a'; 8];
        assert_eq!(server.serve(request).status, 413);
    }

    #[test]
    fn client_max_body_size_is_enforced() {
        let server = Server::new();
        server.set_client_max_body_size(10);
        server.bind_handler("/echo", text_handler("ok"));
        let mut request = get_request("/echo");
        request.method = "POST".to_owned();
        request.body = vec![b'a'; 20];
        let response = server.serve(request);
        assert_eq!(response.status, 413);
        assert_eq!(response.text(), "Request Entity Too Large");

        let mut request = get_request("/echo");
        request.method = "POST".to_owned();
        request.body = vec![b'a'; 5];
        let response = server.serve(request);
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"ok");
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
    fn config_map_applies_address_and_static() {
        let directory =
            std::env::temp_dir().join(format!("gf-ghttp-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("dir");
        std::fs::write(directory.join("hello.txt"), "static-config").expect("file");
        let server = Server::new();
        let config = gf_core::Value::Map(indexmap::IndexMap::from([
            (
                "address".to_owned(),
                gf_core::Value::String("127.0.0.1:0".to_owned()),
            ),
            (
                "serverRoot".to_owned(),
                gf_core::Value::String(directory.to_string_lossy().into_owned()),
            ),
        ]));
        server.set_config_with_map(&config).expect("config");
        assert_eq!(server.address(), "127.0.0.1:0");
        let response = server.serve(get_request("/hello.txt"));
        assert_eq!(response.status, 200);
        assert_eq!(response.text(), "static-config");
        assert!(
            server
                .set_config_with_map(&gf_core::Value::String("bad".to_owned()))
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn config_map_applies_openapi_paths() {
        use crate::goai::{Info, OpenApiV3};

        let server = Server::new();
        server.set_openapi(OpenApiConfig::new(OpenApiV3 {
            info: Info {
                title: "Configured API".to_owned(),
                version: "1.0.0".to_owned(),
                ..Info::default()
            },
            ..OpenApiV3::default()
        }));
        let config = gf_core::Value::Map(indexmap::IndexMap::from([
            (
                "openapiPath".to_owned(),
                gf_core::Value::String("/spec.json".to_owned()),
            ),
            (
                "swaggerPath".to_owned(),
                gf_core::Value::String("/docs".to_owned()),
            ),
        ]));
        server.set_config_with_map(&config).expect("config");
        assert_eq!(server.serve(get_request("/api.json")).status, 404);
        assert_eq!(server.serve(get_request("/spec.json")).status, 200);
        let response = server.serve(get_request("/docs"));
        assert!(response.text().contains("/spec.json"));
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
    fn https_server_serves_tls() {
        let server = Server::new();
        server.bind_handler("/hello", text_handler("secure"));
        let (certificate, key) = tls_pem();
        server.set_tls_pem(&certificate, &key).expect("set tls");
        assert!(server.is_tls());
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let mut reader = std::io::BufReader::new(certificate.as_bytes());
        let certificates: Vec<_> = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, _>>()
            .expect("certs");
        let client = crate::gclient::Client::new()
            .add_root_certificate_der(certificates[0].as_ref())
            .set_prefix(&format!("https://localhost:{}", server.listened_port()));
        let response = client.get("/hello", None).expect("https get");
        assert_eq!(response.status, 200);
        assert_eq!(response.text(), "secure");
        server.close().expect("close");
    }

    #[test]
    fn plain_http_client_against_tls_server_fails() {
        let server = Server::new();
        server.bind_handler("/hello", text_handler("secure"));
        let (certificate, key) = tls_pem();
        server.set_tls_pem(&certificate, &key).expect("set tls");
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let client = crate::gclient::Client::new()
            .set_prefix(&format!("http://127.0.0.1:{}", server.listened_port()));
        assert!(client.get("/hello", None).is_err());
        server.close().expect("close");
    }

    /// Generates a self-signed certificate/key PEM pair for `localhost`.
    fn tls_pem() -> (String, String) {
        let certified =
            rcgen::generate_simple_self_signed(vec!["localhost".to_owned()]).expect("cert");
        (certified.cert.pem(), certified.key_pair.serialize_pem())
    }

    #[test]
    fn session_cookie_uses_cookie_config() {
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
        server.set_cookie_domain(".example.com");
        server.set_cookie_path("/app");
        server.set_cookie_max_age(3600);
        server.set_cookie_secure(true);
        server.set_cookie_same_site("Lax");
        assert_eq!(server.cookie_domain(), ".example.com");
        server.bind_handler(
            "POST:/login",
            Arc::new(|context: &mut Context| {
                if let Some(session) = context.session_mut() {
                    session
                        .set("user", gf_core::Value::String("john".to_owned()))
                        .expect("set session");
                }
                Ok(())
            }),
        );
        let mut login = get_request("/login");
        login.method = "POST".to_owned();
        let response = server.serve(login);
        let cookie = response.cookies().first().cloned().expect("session cookie");
        assert_eq!(cookie.name, "gfsessionid");
        assert_eq!(cookie.domain, ".example.com");
        assert_eq!(cookie.path, "/app");
        assert_eq!(cookie.max_age, Some(3600));
        assert!(cookie.secure);
        assert!(cookie.http_only);
        assert_eq!(cookie.same_site, "Lax");
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
    fn header_limit_returns_431() {
        let server = Server::new();
        server.set_max_header_bytes(40);
        assert_eq!(server.max_header_bytes(), Some(40));
        server.bind_handler("/hello", text_handler("world"));
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let address = server.listened_address();
        let conn = gtcp::Conn::new(&address, Some(Duration::from_secs(5))).expect("conn");
        conn.set_deadline(Some(Duration::from_secs(5)))
            .expect("deadline");
        conn.send(
            b"GET /hello HTTP/1.1\r\nHost: localhost\r\nX-Long: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n",
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
        assert!(
            text.starts_with("HTTP/1.1 431 Request Header Fields Too Large"),
            "{text}"
        );
        let _ = conn.close();
        let _ = server.close();
    }

    #[test]
    fn reuse_port_allows_a_second_listener() {
        if !cfg!(unix) {
            return;
        }
        let first = Server::new();
        first.set_address("127.0.0.1:0");
        first.set_reuse_port(true);
        let running = first.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while first.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let port = first.listened_port();
        assert!(port > 0, "the first listener must bind a port");

        let second = Server::new();
        second.set_address(&format!("127.0.0.1:{port}"));
        second.set_reuse_port(true);
        let running = second.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while second.listened_port() != port && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(
            second.listened_port(),
            port,
            "SO_REUSEPORT must let a second listener share the port"
        );
        second.close().expect("close second");
        first.close().expect("close first");
    }
    #[test]
    fn domain_routing_in_server() {
        let server = Server::new();
        server.bind_handler("GET:/who", text_handler("default"));
        server.bind_handler("GET:/who@api.example.com", text_handler("api"));

        assert_eq!(server.serve(get_request("/who")).text(), "default");
        let mut api = get_request("/who");
        api.headers.set("Host", "api.example.com");
        assert_eq!(server.serve(api).text(), "api");

        let mut other = get_request("/who");
        other.headers.set("Host", "other.example.com");
        assert_eq!(server.serve(other).text(), "default");
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
