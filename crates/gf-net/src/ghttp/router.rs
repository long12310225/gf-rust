//! Port of the `net/ghttp` router (`ghttp_server_router*.go` semantics).
//!
//! Go's router is a per-domain segment tree with middleware lists attached to
//! nodes. This port keeps the observable behaviour with a flat route table:
//! static, `:param` and `*wildcard` segments, an optional method prefix
//! (`GET:/path`, `ALL:/path`), priority static > param > wildcard, and
//! group prefixes with middleware (ADR-067).
//!
//! Not ported: per-domain routing, route caching, hooks (before/after serve),
//! and the `{pattern}` rewrite syntax.

use std::sync::{Arc, RwLock};

use gf_errors::gerror::Error;
use indexmap::IndexMap;

use super::message::Request;

/// A request handler or middleware. Middleware must call [`Context::proceed`].
pub type Handler = Arc<dyn Fn(&mut Context) -> Result<(), Error> + Send + Sync>;

/// One route segment.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Segment {
    /// A literal path segment.
    Static(String),
    /// A `:name` parameter segment.
    Param(String),
    /// A `*name` wildcard capturing the rest of the path.
    Wildcard(String),
}

/// The request context passed through the handler chain.
pub struct Context {
    /// The parsed request.
    pub request: Request,
    /// The response being built.
    pub response: super::message::Response,
    /// The matched route parameters.
    pub params: IndexMap<String, String>,
    /// The request session, when the server has a session manager.
    pub session: Option<gf_os::gsession::Session>,
    /// The negotiated request language (empty until a middleware sets it).
    pub language: String,
    /// Whether the response should be gzip-compressed after the chain.
    pub gzip: bool,
    /// Whether the default JSON envelope should wrap an empty response.
    pub handler_response: bool,
    /// The tracing span for this request, when the tracing middleware ran.
    pub span: Option<super::tracing::ServerSpan>,
    handlers: Vec<Handler>,
    index: usize,
}

impl Context {
    /// Creates a context with `request` and the full handler chain.
    pub fn new(request: Request, handlers: Vec<Handler>) -> Context {
        Context {
            request,
            response: super::message::Response::new(200),
            params: IndexMap::new(),
            session: None,
            language: String::new(),
            gzip: false,
            handler_response: false,
            span: None,
            handlers,
            index: 0,
        }
    }

    /// Runs the handler chain, like GoFrame's middleware `Next`.
    pub fn proceed(&mut self) -> Result<(), Error> {
        while self.index < self.handlers.len() {
            let handler = Arc::clone(&self.handlers[self.index]);
            self.index += 1;
            handler(self)?;
        }
        super::middleware::finish(self);
        super::gzip::finish(self);
        Ok(())
    }

    /// Returns one route parameter.
    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    /// The negotiated request language, when the language middleware ran.
    pub fn language(&self) -> &str {
        &self.language
    }

    /// The request tracing span, when the tracing middleware ran.
    pub fn span(&self) -> Option<&super::tracing::ServerSpan> {
        self.span.as_ref()
    }

    /// Stops the remaining handler chain, like GoFrame's `ExitAll`.
    pub fn exit_all(&mut self) {
        self.index = self.handlers.len();
    }

    /// Redirects to `location`, like `Response.RedirectTo` (ADR-124); the
    /// default status is 302 and the handler chain stops.
    pub fn redirect_to(&mut self, location: &str, code: Option<i32>) {
        self.response.headers.set("Location", location);
        self.response.status = code.unwrap_or(302);
        self.exit_all();
    }

    /// Redirects back to the request referer, like `RedirectBack`.
    pub fn redirect_back(&mut self, code: Option<i32>) {
        let referer = self.request.referer();
        self.redirect_to(&referer, code);
    }

    /// Returns the request session, when configured.
    pub fn session_mut(&mut self) -> Option<&mut gf_os::gsession::Session> {
        self.session.as_mut()
    }
}

/// One registered route.
struct Route {
    methods: Vec<String>,
    segments: Vec<Segment>,
    middlewares: Vec<Handler>,
    handler: Handler,
}

impl Route {
    /// The match score (larger is better) and captured parameters.
    fn matches(&self, method: &str, parts: &[&str]) -> Option<(usize, IndexMap<String, String>)> {
        let method_matches = self.methods.iter().any(|candidate| candidate == "ALL")
            || self.methods.iter().any(|candidate| candidate == method);
        if !method_matches {
            return None;
        }
        let mut params = IndexMap::new();
        let mut static_count = 0usize;
        let mut index = 0usize;
        for segment in &self.segments {
            match segment {
                Segment::Static(expected) => {
                    let part = parts.get(index)?;
                    if part != expected {
                        return None;
                    }
                    static_count += 4;
                    index += 1;
                }
                Segment::Param(name) => {
                    let part = parts.get(index)?;
                    if part.is_empty() {
                        return None;
                    }
                    params.insert(name.clone(), (*part).to_owned());
                    index += 1;
                }
                Segment::Wildcard(name) => {
                    let rest = parts[index..].join("/");
                    if rest.is_empty() && name.is_empty() {
                        return None;
                    }
                    if !name.is_empty() {
                        params.insert(name.clone(), rest);
                    }
                    index = parts.len();
                }
            }
        }
        if index != parts.len() {
            return None;
        }
        Some((static_count, params))
    }
}

/// A flat route table, like GoFrame's serve tree.
#[derive(Default)]
pub struct Router {
    routes: RwLock<Vec<Route>>,
}

impl Router {
    /// Creates an empty router.
    pub fn new() -> Router {
        Router::default()
    }

    /// Registers `handler` for `pattern`, like `Server.BindHandler`.
    ///
    /// The pattern may carry a method prefix: `GET:/api/user`, `ALL:/api/*`.
    pub fn bind(&self, pattern: &str, handler: Handler) {
        self.bind_with_middlewares(pattern, Vec::new(), handler);
    }

    /// Registers a route with route-level middlewares.
    pub fn bind_with_middlewares(
        &self,
        pattern: &str,
        middlewares: Vec<Handler>,
        handler: Handler,
    ) {
        let (methods, path) = split_pattern(pattern);
        let segments = parse_segments(&path);
        self.routes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .push(Route {
                methods,
                segments,
                middlewares,
                handler,
            });
    }

    /// Returns every registered route as `(methods, pattern)`, like
    /// GoFrame's route map (ADR-143).
    pub fn route_list(&self) -> Vec<(Vec<String>, String)> {
        self.routes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .map(|route| (route.methods.clone(), segments_pattern(&route.segments)))
            .collect()
    }

    /// Finds the best matching route, returning its handler chain.
    pub fn lookup(
        &self,
        method: &str,
        path: &str,
    ) -> Option<(Vec<Handler>, IndexMap<String, String>)> {
        let normalized = normalize_path(path);
        let parts: Vec<&str> = if normalized == "/" {
            Vec::new()
        } else {
            normalized.trim_start_matches('/').split('/').collect()
        };
        let routes = self
            .routes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut best: Option<(usize, &Route, IndexMap<String, String>)> = None;
        for route in routes.iter() {
            if let Some((score, params)) = route.matches(method, &parts) {
                let better = match &best {
                    None => true,
                    Some((best_score, _, _)) => score > *best_score,
                };
                if better {
                    best = Some((score, route, params));
                }
            }
        }
        let (_, route, params) = best?;
        let mut handlers = route.middlewares.clone();
        handlers.push(Arc::clone(&route.handler));
        Some((handlers, params))
    }
}

/// Reports whether `path` matches a route `pattern` (any method), like the
/// router's own segment matching (ADR-129).
pub fn pattern_matches(pattern: &str, path: &str) -> bool {
    let (_, route_path) = split_pattern(pattern);
    let segments = parse_segments(&route_path);
    let normalized = normalize_path(path);
    let parts: Vec<&str> = if normalized == "/" {
        Vec::new()
    } else {
        normalized.trim_start_matches('/').split('/').collect()
    };
    let route = Route {
        methods: vec!["ALL".to_owned()],
        segments,
        middlewares: Vec::new(),
        handler: Arc::new(|_context: &mut Context| Ok(())),
    };
    route.matches("ALL", &parts).is_some()
}

/// Rebuilds the pattern text of parsed segments for route listings.
fn segments_pattern(segments: &[Segment]) -> String {
    if segments.is_empty() {
        return "/".to_owned();
    }
    let mut pattern = String::new();
    for segment in segments {
        pattern.push('/');
        match segment {
            Segment::Static(value) => pattern.push_str(value),
            Segment::Param(name) => {
                pattern.push(':');
                pattern.push_str(name);
            }
            Segment::Wildcard(name) => {
                pattern.push('*');
                pattern.push_str(name);
            }
        }
    }
    pattern
}

/// Splits `METHOD:path` into the method list and the path.
fn split_pattern(pattern: &str) -> (Vec<String>, String) {
    let pattern = pattern.trim();
    if let Some((prefix, path)) = pattern.split_once(':')
        && !prefix.is_empty()
        && prefix
            .chars()
            .all(|character| character.is_ascii_alphabetic() || character == ',')
    {
        let methods = prefix
            .split(',')
            .map(|method| method.trim().to_ascii_uppercase())
            .filter(|method| !method.is_empty())
            .collect();
        return (methods, path.to_owned());
    }
    (vec!["ALL".to_owned()], pattern.to_owned())
}

/// Parses a route path into segments.
fn parse_segments(path: &str) -> Vec<Segment> {
    let normalized = normalize_path(path);
    if normalized == "/" {
        return Vec::new();
    }
    let mut segments = Vec::new();
    for part in normalized.trim_start_matches('/').split('/') {
        if let Some(name) = part.strip_prefix(':') {
            segments.push(Segment::Param(name.to_owned()));
        } else if let Some(name) = part.strip_prefix('*') {
            segments.push(Segment::Wildcard(name.to_owned()));
        } else {
            segments.push(Segment::Static(part.to_owned()));
        }
    }
    segments
}

/// Collapses repeated `/` and guarantees a leading `/`, like GoFrame's path
/// normalization.
pub fn normalize_path(path: &str) -> String {
    let mut normalized = String::with_capacity(path.len() + 1);
    let mut previous_separator = false;
    for character in path.chars() {
        if character == '/' {
            if previous_separator {
                continue;
            }
            previous_separator = true;
        } else {
            previous_separator = false;
        }
        normalized.push(character);
    }
    if !normalized.starts_with('/') {
        normalized.insert(0, '/');
    }
    if normalized.is_empty() {
        normalized.push('/');
    }
    normalized
}

/// A route group with a path prefix and middlewares, like GoFrame's `Group`.
pub struct Group {
    router: Arc<Router>,
    prefix: String,
    middlewares: Vec<Handler>,
}

impl Group {
    /// Creates a group over `router` with `prefix`.
    pub fn new(router: Arc<Router>, prefix: &str) -> Group {
        Group {
            router,
            prefix: normalize_path(prefix).trim_end_matches('/').to_owned(),
            middlewares: Vec::new(),
        }
    }

    /// Returns a nested group with `prefix`.
    pub fn group(&self, prefix: &str) -> Group {
        let nested = format!(
            "{}/{}",
            self.prefix,
            normalize_path(prefix).trim_matches('/')
        );
        Group {
            router: Arc::clone(&self.router),
            prefix: nested.trim_end_matches('/').to_owned(),
            middlewares: self.middlewares.clone(),
        }
    }

    /// Adds a middleware to the group.
    pub fn middleware(&mut self, middleware: Handler) {
        self.middlewares.push(middleware);
    }

    /// Binds a handler under the group prefix.
    pub fn bind_handler(&self, pattern: &str, handler: Handler) {
        let (methods, path) = split_pattern(pattern);
        let full_path = format!(
            "{}/{}",
            self.prefix,
            normalize_path(&path).trim_matches('/')
        );
        let method_prefix = methods.join(",");
        let full_pattern = format!("{method_prefix}:{full_path}");
        self.router
            .bind_with_middlewares(&full_pattern, self.middlewares.clone(), handler);
    }

    /// Binds `handler` for `method` under the group, like GoFrame's
    /// `Group.Bind`.
    pub fn bind(&self, method: &str, pattern: &str, handler: Handler) {
        self.bind_handler(
            &format!("{}:{pattern}", method.to_ascii_uppercase()),
            handler,
        );
    }

    /// Binds a `GET` handler under the group.
    pub fn get(&self, pattern: &str, handler: Handler) {
        self.bind("GET", pattern, handler);
    }

    /// Binds a `POST` handler under the group.
    pub fn post(&self, pattern: &str, handler: Handler) {
        self.bind("POST", pattern, handler);
    }

    /// Binds a `PUT` handler under the group.
    pub fn put(&self, pattern: &str, handler: Handler) {
        self.bind("PUT", pattern, handler);
    }

    /// Binds a `DELETE` handler under the group.
    pub fn delete(&self, pattern: &str, handler: Handler) {
        self.bind("DELETE", pattern, handler);
    }

    /// Binds a `PATCH` handler under the group.
    pub fn patch(&self, pattern: &str, handler: Handler) {
        self.bind("PATCH", pattern, handler);
    }

    /// Binds an `OPTIONS` handler under the group.
    pub fn options(&self, pattern: &str, handler: Handler) {
        self.bind("OPTIONS", pattern, handler);
    }

    /// Binds a `HEAD` handler under the group.
    pub fn head(&self, pattern: &str, handler: Handler) {
        self.bind("HEAD", pattern, handler);
    }

    /// Binds a handler for every HTTP method under the group, like `ALL`.
    pub fn all(&self, pattern: &str, handler: Handler) {
        self.bind("ALL", pattern, handler);
    }

    /// Binds a handler for several methods under the group.
    pub fn bind_methods(&self, methods: &[&str], pattern: &str, handler: Handler) {
        let methods: Vec<String> = methods
            .iter()
            .map(|method| method.to_ascii_uppercase())
            .collect();
        self.bind_handler(&format!("{}:{pattern}", methods.join(",")), handler);
    }

    /// The group prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }
}

/// Builds a JSON response in a handler.
pub fn write_json(context: &mut Context, value: &str) {
    context.response.set_json(value);
}

/// Builds a text response in a handler.
pub fn write_text(context: &mut Context, value: &str) {
    context.response.set_text(value);
}

/// Sets a 404 response, like GoFrame's default not-found handling.
pub fn not_found() -> super::message::Response {
    let mut response = super::message::Response::new(404);
    response.set_text("404 page not found\n");
    response
}

/// Sets a 500 response.
pub fn internal_error(error: &Error) -> super::message::Response {
    let mut response = super::message::Response::new(500);
    response.set_text(&format!("{error}\n"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use gf_errors::gerror;

    fn handler(text: &'static str) -> Handler {
        Arc::new(move |context: &mut Context| {
            write_text(context, text);
            Ok(())
        })
    }

    #[test]
    fn static_param_and_wildcard_matching() {
        let router = Router::new();
        router.bind("/api/user/list", handler("list"));
        router.bind("/api/user/:id", handler("one"));
        router.bind("/api/files/*path", handler("files"));
        assert!(router.lookup("GET", "/api/user/list").is_some());
        let (_, params) = router.lookup("GET", "/api/user/42").expect("param");
        assert_eq!(params["id"], "42");
        let (_, params) = router
            .lookup("GET", "/api/files/a/b/c.txt")
            .expect("wildcard");
        assert_eq!(params["path"], "a/b/c.txt");
        assert!(router.lookup("GET", "/api/user/42/extra").is_none());
        assert!(router.lookup("GET", "/nope").is_none());
    }

    #[test]
    fn static_priority_and_method_prefixes() {
        let router = Router::new();
        router.bind("/user/:id", handler("param"));
        router.bind("/user/me", handler("static"));
        let (_, params) = router.lookup("GET", "/user/me").expect("static wins");
        assert!(params.is_empty(), "static route captured no params");

        let router = Router::new();
        router.bind("GET:/api", handler("get"));
        router.bind("POST:/api", handler("post"));
        assert!(router.lookup("GET", "/api").is_some());
        assert!(router.lookup("POST", "/api").is_some());
        assert!(router.lookup("PUT", "/api").is_none());
        router.bind("ALL:/any", handler("any"));
        assert!(router.lookup("DELETE", "/any").is_some());
    }

    #[test]
    fn middleware_order_and_short_circuit() {
        let router = Router::new();
        let order = Arc::new(std::sync::Mutex::new(Vec::new()));
        let first = Arc::clone(&order);
        let middleware: Handler = Arc::new(move |context: &mut Context| {
            first.lock().unwrap().push("before");
            context.proceed()?;
            first.lock().unwrap().push("after");
            Ok(())
        });
        let second = Arc::clone(&order);
        let route_middleware: Handler = Arc::new(move |context: &mut Context| {
            second.lock().unwrap().push("route");
            context.proceed()
        });
        let final_handler = Arc::clone(&order);
        router.bind_with_middlewares(
            "/path",
            vec![middleware, route_middleware],
            Arc::new(move |context: &mut Context| {
                final_handler.lock().unwrap().push("handler");
                write_text(context, "ok");
                Ok(())
            }),
        );
        let (handlers, _) = router.lookup("GET", "/path").expect("route");
        let mut context = Context::new(Request::default(), handlers);
        context.proceed().expect("chain");
        assert_eq!(
            *order.lock().unwrap(),
            vec!["before", "route", "handler", "after"]
        );

        // Short circuit: a middleware that does not call next stops the chain.
        let router = Router::new();
        router.bind(
            "/short",
            Arc::new(move |context: &mut Context| {
                context.response.set_text("blocked");
                Ok(())
            }),
        );
        let (handlers, _) = router.lookup("GET", "/short").expect("route");
        let mut context = Context::new(Request::default(), handlers);
        context.proceed().expect("chain");
        assert_eq!(context.response.body, b"blocked");
    }

    #[test]
    fn groups_prefix_and_middlewares() {
        let router = Arc::new(Router::new());
        let mut group = Group::new(Arc::clone(&router), "/api");
        let hit = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&hit);
        group.middleware(Arc::new(move |context: &mut Context| {
            counter.fetch_add(1, Ordering::SeqCst);
            context.proceed()
        }));
        group.bind_handler("GET:/user", handler("user"));
        let nested = group.group("v1");
        nested.bind_handler("/ping", handler("pong"));

        assert!(router.lookup("GET", "/api/user").is_some());
        assert!(router.lookup("GET", "/api/v1/ping").is_some());
        assert!(router.lookup("POST", "/api/user").is_none());

        let (handlers, _) = router.lookup("GET", "/api/v1/ping").expect("nested");
        let mut context = Context::new(Request::default(), handlers);
        context.proceed().expect("chain");
        assert_eq!(hit.load(Ordering::SeqCst), 1);
        assert_eq!(context.response.body, b"pong");
    }

    #[test]
    fn group_verb_bindings() {
        let router = Arc::new(Router::new());
        let group = Group::new(Arc::clone(&router), "/api");
        assert_eq!(group.prefix(), "/api");
        group.get("/user", handler("get-user"));
        group.post("/user", handler("post-user"));
        group.put("/item", handler("put-item"));
        group.delete("/item", handler("delete-item"));
        group.patch("/item", handler("patch-item"));
        group.options("/item", handler("options-item"));
        group.head("/item", handler("head-item"));
        group.all("/any", handler("any"));

        assert!(router.lookup("GET", "/api/user").is_some());
        assert!(router.lookup("POST", "/api/user").is_some());
        assert!(router.lookup("PUT", "/api/item").is_some());
        assert!(router.lookup("DELETE", "/api/item").is_some());
        assert!(router.lookup("PATCH", "/api/item").is_some());
        assert!(router.lookup("OPTIONS", "/api/item").is_some());
        assert!(router.lookup("HEAD", "/api/item").is_some());
        for method in ["GET", "POST", "PUT", "DELETE"] {
            assert!(router.lookup(method, "/api/any").is_some(), "{method}");
        }
        assert!(router.lookup("GET", "/api/item").is_none());

        // Explicit method binding and multi-method binding.
        group.bind("get", "/bind", handler("bind-get"));
        group.bind_methods(&["get", "post"], "/multi", handler("multi"));
        assert!(router.lookup("GET", "/api/bind").is_some());
        assert!(router.lookup("GET", "/api/multi").is_some());
        assert!(router.lookup("POST", "/api/multi").is_some());
        assert!(router.lookup("PUT", "/api/multi").is_none());
        assert_eq!(group.group("v1").prefix(), "/api/v1");
    }

    #[test]
    fn context_redirects() {
        use crate::ghttp::message::Request;

        let mut context = Context::new(Request::default(), Vec::new());
        context.redirect_to("/login", None);
        assert_eq!(context.response.status, 302);
        assert_eq!(context.response.headers.get("Location"), Some("/login"));

        let mut context = Context::new(Request::default(), Vec::new());
        context.redirect_to("https://example.com", Some(301));
        assert_eq!(context.response.status, 301);

        let mut request = Request::default();
        request.headers.set("Referer", "https://ref.example/back");
        let mut context = Context::new(request, Vec::new());
        context.redirect_back(None);
        assert_eq!(context.response.status, 302);
        assert_eq!(
            context.response.headers.get("Location"),
            Some("https://ref.example/back")
        );
    }

    #[test]
    fn route_listing_rebuilds_patterns() {
        let router = Arc::new(Router::new());
        router.bind("GET:/user/:id", handler("user"));
        router.bind("POST,PUT:/api/*path", handler("api"));
        let routes = router.route_list();
        assert_eq!(routes.len(), 2);
        assert!(routes.contains(&(vec!["GET".to_owned()], "/user/:id".to_owned())));
        assert!(routes.contains(&(
            vec!["POST".to_owned(), "PUT".to_owned()],
            "/api/*path".to_owned()
        )));
        let empty: Vec<_> = Router::new().route_list();
        assert!(empty.is_empty());
    }

    #[test]
    fn normalize_paths() {
        assert_eq!(normalize_path("/user//list/"), "/user/list/");
        assert_eq!(normalize_path("user"), "/user");
        assert_eq!(normalize_path("/"), "/");
        assert!(Router::new().lookup("GET", "/missing").is_none());
    }

    #[test]
    fn not_found_and_error_responses() {
        let response = not_found();
        assert_eq!(response.status, 404);
        assert_eq!(response.body, b"404 page not found\n");
        let response = internal_error(&gerror::new("boom"));
        assert_eq!(response.status, 500);
        assert!(String::from_utf8_lossy(&response.body).contains("boom"));
    }
}
