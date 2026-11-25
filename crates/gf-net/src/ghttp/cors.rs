//! CORS support for `ghttp`, ported from `ghttp_response_cors.go` and
//! `ghttp_middleware_cors.go` (ADR-102).
//!
//! [`CorOptions`] mirrors GoFrame's `CORSOptions`; [`ContextExt`] methods
//! apply the headers to the response, and [`middleware_cors`] is the default
//! middleware (it sets the headers and, for `OPTIONS`, stops the chain with
//! status 200). The request-side default options derive the allowed origin
//! from `Origin`/`Referer` and append client-requested headers.

use gf_errors::gerror::Error;

use super::message::Request;
use super::router::Context;

/// The default allowed headers, like GoFrame's `defaultAllowHeaders`.
pub const DEFAULT_ALLOW_HEADERS: &str =
    "Origin,Content-Type,Accept,User-Agent,Cookie,Authorization,X-Auth-Token,X-Requested-With";

/// The default allowed methods, like GoFrame's `supportedHttpMethods`.
pub const DEFAULT_ALLOW_METHODS: &str = "GET,PUT,POST,DELETE,PATCH,HEAD,CONNECT,OPTIONS,TRACE";

/// The default max age (30 days), like GoFrame.
pub const DEFAULT_MAX_AGE: i64 = 3628800;

/// CORS options, like GoFrame's `CORSOptions`.
#[derive(Clone, Debug, Default)]
pub struct CorOptions {
    /// Allowed custom domains (empty means any).
    pub allow_domain: Vec<String>,
    /// `Access-Control-Allow-Origin`.
    pub allow_origin: String,
    /// `Access-Control-Allow-Credentials`.
    pub allow_credentials: String,
    /// `Access-Control-Expose-Headers`.
    pub expose_headers: String,
    /// `Access-Control-Max-Age`.
    pub max_age: i64,
    /// `Access-Control-Allow-Methods`.
    pub allow_methods: String,
    /// `Access-Control-Allow-Headers`.
    pub allow_headers: String,
}

/// Builds the default options for `request`, like `DefaultCORSOptions`.
pub fn default_options(request: &Request) -> CorOptions {
    let mut options = CorOptions {
        allow_origin: "*".to_owned(),
        allow_methods: DEFAULT_ALLOW_METHODS.to_owned(),
        allow_credentials: "true".to_owned(),
        allow_headers: DEFAULT_ALLOW_HEADERS.to_owned(),
        max_age: DEFAULT_MAX_AGE,
        ..CorOptions::default()
    };
    if let Some(headers) = request.headers.get("Access-Control-Request-Headers") {
        for header in headers.split(',') {
            let header = header.trim();
            if !header.is_empty() && !DEFAULT_ALLOW_HEADERS.contains(header) {
                options.allow_headers.push(',');
                options.allow_headers.push_str(header);
            }
        }
    }
    if let Some(origin) = request.headers.get("Origin") {
        if !origin.is_empty() {
            options.allow_origin = origin.to_owned();
        }
    } else if let Some(referer) = request.headers.get("Referer")
        && !referer.is_empty()
    {
        options.allow_origin = origin_of(referer).unwrap_or_else(|| referer.to_owned());
    }
    options
}

/// Extracts `scheme://host` from a referer URL.
fn origin_of(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let authority = rest.split('/').next().unwrap_or(rest);
    Some(format!("{scheme}://{authority}"))
}

/// Reports whether the request origin is allowed by `options`, like
/// `CORSAllowedOrigin`.
pub fn allowed_origin(request: &Request, options: &CorOptions) -> bool {
    if options.allow_domain.is_empty() {
        return true;
    }
    let Some(origin) = request.headers.get("Origin") else {
        return true;
    };
    let host = origin
        .split_once("://")
        .map(|(_, rest)| rest.split('/').next().unwrap_or(rest))
        .unwrap_or(origin);
    options
        .allow_domain
        .iter()
        .any(|domain| is_sub_domain(host, domain))
}

/// Reports whether `host` equals `domain` or is one of its subdomains.
fn is_sub_domain(host: &str, domain: &str) -> bool {
    host == domain || host.ends_with(&format!(".{domain}"))
}

impl Context {
    /// Applies default CORS options, like `Response.CORSDefault`.
    pub fn cors_default(&mut self) {
        let options = default_options(&self.request);
        self.cors(&options);
    }

    /// Applies custom CORS options, like `Response.CORS`.
    pub fn cors(&mut self, options: &CorOptions) {
        if allowed_origin(&self.request, options) {
            self.response
                .headers
                .set("Access-Control-Allow-Origin", &options.allow_origin);
        }
        if !options.allow_credentials.is_empty() {
            self.response.headers.set(
                "Access-Control-Allow-Credentials",
                &options.allow_credentials,
            );
        }
        if !options.expose_headers.is_empty() {
            self.response
                .headers
                .set("Access-Control-Expose-Headers", &options.expose_headers);
        }
        if options.max_age != 0 {
            self.response
                .headers
                .set("Access-Control-Max-Age", &options.max_age.to_string());
        }
        if !options.allow_methods.is_empty() {
            self.response
                .headers
                .set("Access-Control-Allow-Methods", &options.allow_methods);
        }
        if !options.allow_headers.is_empty() {
            self.response
                .headers
                .set("Access-Control-Allow-Headers", &options.allow_headers);
        }
        if self.request.method.eq_ignore_ascii_case("OPTIONS") {
            if self.response.status == 200 {
                self.response.status = 200;
            }
            self.exit_all();
        }
    }
}

/// The default CORS middleware, like `MiddlewareCORS`; bind it with
/// `Server::bind_middleware`.
pub fn middleware_cors(context: &mut Context) -> Result<(), Error> {
    context.cors_default();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghttp::message::Header;

    fn request(method: &str, headers: &[(&str, &str)]) -> Request {
        let mut request = Request {
            method: method.to_owned(),
            path: "/".to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        };
        let mut parsed = Header::new();
        for (name, value) in headers {
            parsed.set(name, value);
        }
        request.headers = parsed;
        request
    }

    fn make_context(request: &Request, handlers: Vec<super::super::router::Handler>) -> Context {
        Context::new(request.clone(), handlers)
    }

    #[test]
    fn defaults_use_origin_and_request_headers() {
        let request = request(
            "GET",
            &[
                ("Origin", "https://app.example.com"),
                ("Access-Control-Request-Headers", "X-Custom"),
            ],
        );
        let options = default_options(&request);
        assert_eq!(options.allow_origin, "https://app.example.com");
        assert_eq!(options.allow_credentials, "true");
        assert_eq!(options.max_age, DEFAULT_MAX_AGE);
        assert!(options.allow_headers.contains("X-Custom"));
    }

    #[test]
    fn referer_origin_is_extracted() {
        let request = request("GET", &[("Referer", "https://ref.example.com/a/b?c=1")]);
        let options = default_options(&request);
        assert_eq!(options.allow_origin, "https://ref.example.com");
    }

    #[test]
    fn custom_options_filter_origins() {
        let mut options = CorOptions {
            allow_domain: vec!["example.com".to_owned()],
            allow_origin: "https://example.com".to_owned(),
            allow_methods: "GET,POST".to_owned(),
            max_age: 600,
            ..CorOptions::default()
        };
        let request = request("GET", &[("Origin", "https://api.example.com")]);
        assert!(allowed_origin(&request, &options));
        let mut context = make_context(&request, Vec::new());
        context.cors(&options);
        assert_eq!(
            context.response.headers.get("Access-Control-Allow-Origin"),
            Some("https://example.com")
        );
        assert_eq!(
            context.response.headers.get("Access-Control-Max-Age"),
            Some("600")
        );

        options.allow_domain = vec!["other.com".to_owned()];
        let request = super::tests::request("GET", &[("Origin", "https://evil.com")]);
        assert!(!allowed_origin(&request, &options));
        let mut context = make_context(&request, Vec::new());
        context.cors(&options);
        assert_eq!(
            context.response.headers.get("Access-Control-Allow-Origin"),
            None
        );
    }

    #[test]
    fn options_request_exits_the_chain() {
        use std::sync::Arc;

        let request = request("OPTIONS", &[("Origin", "https://app.example.com")]);
        let ran = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&ran);
        let handler: super::super::router::Handler = Arc::new(move |_context: &mut Context| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        });
        let mut context = make_context(&request, vec![handler]);
        context.cors_default();
        assert!(!ran.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(
            context.response.headers.get("Access-Control-Allow-Origin"),
            Some("https://app.example.com")
        );
        assert_eq!(context.response.status, 200);
    }
}
