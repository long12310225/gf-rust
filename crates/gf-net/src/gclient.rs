//! Port of `net/gclient` — the HTTP client.
//!
//! GoFrame's client wraps `net/http` with a prefix, default headers, cookies,
//! timeouts and middleware. This port keeps the request-building surface and
//! talks HTTP/1.1 over [`crate::gtcp`] using [`crate::ghttp::message`]
//! (ADR-069):
//!
//! - [`Client`] with `set_prefix`, `set_header`, `set_cookie`, `set_timeout`,
//!   `set_retries` and `get`/`post`/`put`/`delete`/`head`/`patch`;
//! - request bodies (`post_form`, `post_json`, `request` with raw body);
//! - query parameters, response parsing (status line, headers, body) and
//!   [`Content`] conversion (JSON/text).
//!
//! Not ported: TLS, redirects, chunked responses, compression, proxies,
//! middleware chains and the cookie jar beyond the explicitly set cookies.

use std::time::Duration;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use gf_util::gconv;
use indexmap::IndexMap;

use crate::ghttp::message::{Cookies, Header, Query, Response};
use crate::gtcp::Conn;

/// Request parameters, like GoFrame's `Params`.
pub type Params = IndexMap<String, gf_core::Value>;

/// The HTTP client, like GoFrame's `Client`.
#[derive(Clone, Debug)]
pub struct Client {
    prefix: String,
    headers: Header,
    cookies: Cookies,
    timeout: Duration,
    retries: u32,
}

impl Default for Client {
    fn default() -> Client {
        Client::new()
    }
}

impl Client {
    /// Creates a client with a 30-second timeout, like `New`.
    pub fn new() -> Client {
        Client {
            prefix: String::new(),
            headers: Header::new(),
            cookies: Cookies::new(),
            timeout: Duration::from_secs(30),
            retries: 0,
        }
    }

    /// The current URL prefix.
    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// Sets the URL prefix, like `SetPrefix`.
    pub fn set_prefix(&self, prefix: &str) -> Client {
        let mut client = self.clone();
        client.prefix = prefix.trim_end_matches('/').to_owned();
        client
    }

    /// Sets a default header, like `SetHeader`.
    pub fn set_header(&self, key: &str, value: &str) -> Client {
        let mut client = self.clone();
        client.headers.set(key, value);
        client
    }

    /// Adds a default header, like `SetHeader` with append semantics.
    pub fn add_header(&self, key: &str, value: &str) -> Client {
        let mut client = self.clone();
        client.headers.add(key, value);
        client
    }

    /// Sets one cookie, like `SetCookie`.
    pub fn set_cookie(&self, name: &str, value: &str) -> Client {
        let mut client = self.clone();
        client.cookies.set(name, value);
        client
    }

    /// Sets the request timeout, like `SetTimeout`.
    pub fn set_timeout(&self, timeout: Duration) -> Client {
        let mut client = self.clone();
        client.timeout = timeout;
        client
    }

    /// Sets the retry count for connection/write failures, like `SetRetry`.
    pub fn set_retries(&self, retries: u32) -> Client {
        let mut client = self.clone();
        client.retries = retries;
        client
    }

    /// Sends a GET request, like `Get`.
    pub fn get(&self, path: &str, params: Option<&Params>) -> Result<Response, Error> {
        self.request("GET", path, params, None)
    }

    /// Sends a GET request and returns the body text, like `GetContent`.
    pub fn get_content(&self, path: &str, params: Option<&Params>) -> Result<String, Error> {
        Ok(self.get(path, params)?.text())
    }

    /// Sends a POST request with a raw body.
    pub fn post(
        &self,
        path: &str,
        params: Option<&Params>,
        body: Option<(&[u8], &str)>,
    ) -> Result<Response, Error> {
        self.request("POST", path, params, body)
    }

    /// Sends a POST with a form body, like `Post` with `PostForm`.
    pub fn post_form(&self, path: &str, form: &Query) -> Result<Response, Error> {
        let body = form.encode();
        self.request(
            "POST",
            path,
            None,
            Some((body.as_bytes(), "application/x-www-form-urlencoded")),
        )
    }

    /// Sends a POST with a JSON body.
    pub fn post_json(&self, path: &str, json: &str) -> Result<Response, Error> {
        self.request(
            "POST",
            path,
            None,
            Some((json.as_bytes(), "application/json; charset=utf-8")),
        )
    }

    /// Sends a PUT request.
    pub fn put(
        &self,
        path: &str,
        params: Option<&Params>,
        body: Option<(&[u8], &str)>,
    ) -> Result<Response, Error> {
        self.request("PUT", path, params, body)
    }

    /// Sends a DELETE request.
    pub fn delete(&self, path: &str, params: Option<&Params>) -> Result<Response, Error> {
        self.request("DELETE", path, params, None)
    }

    /// Sends a HEAD request (the body is not returned by the server).
    pub fn head(&self, path: &str, params: Option<&Params>) -> Result<Response, Error> {
        self.request("HEAD", path, params, None)
    }

    /// Sends a PATCH request.
    pub fn patch(
        &self,
        path: &str,
        params: Option<&Params>,
        body: Option<(&[u8], &str)>,
    ) -> Result<Response, Error> {
        self.request("PATCH", path, params, body)
    }

    /// Sends one HTTP/1.1 request, like `DoRequest`.
    pub fn request(
        &self,
        method: &str,
        path: &str,
        params: Option<&Params>,
        body: Option<(&[u8], &str)>,
    ) -> Result<Response, Error> {
        let url = build_url(&self.prefix, path, params);
        let (host, port, target) = split_url(&url)?;
        let mut last_error: Option<Error> = None;
        for _attempt in 0..=self.retries {
            match self.try_request(method, &host, port, &target, body) {
                Ok(response) => return Ok(response),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| gerror::new("HTTP request failed")))
    }

    /// Runs one HTTP request attempt.
    fn try_request(
        &self,
        method: &str,
        host: &str,
        port: u16,
        target: &str,
        body: Option<(&[u8], &str)>,
    ) -> Result<Response, Error> {
        let conn = Conn::new(&format!("{host}:{port}"), Some(self.timeout))?;
        conn.set_deadline(Some(self.timeout))
            .map_err(|error| gerror::new(format!("set HTTP deadline failed: {error}")))?;
        let mut request = format!("{method} {target} HTTP/1.1\r\nHost: {host}\r\n");
        let mut headers = self.headers.clone();
        if let Some((bytes, content_type)) = body {
            headers.set("Content-Type", content_type);
            headers.set("Content-Length", &bytes.len().to_string());
        }
        if !self.cookies.is_empty() {
            let cookies: Vec<String> = self
                .cookies
                .iter()
                .map(|(name, value)| format!("{name}={value}"))
                .collect();
            headers.set("Cookie", &cookies.join("; "));
        }
        headers.set("Connection", "close");
        for (key, values) in headers.iter() {
            for value in values {
                request.push_str(&format!("{key}: {value}\r\n"));
            }
        }
        request.push_str("\r\n");
        let mut payload = request.into_bytes();
        if let Some((bytes, _)) = body {
            payload.extend_from_slice(bytes);
        }
        conn.send(&payload, None)?;
        let response = read_response(&conn)?;
        let _ = conn.close();
        Ok(response)
    }
}

/// Builds the request URL from the prefix, path and query parameters.
pub fn build_url(prefix: &str, path: &str, params: Option<&Params>) -> String {
    let mut url = format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let mut query = Query::default();
    if let Some(raw) = path.split_once('?').map(|(_, query)| query) {
        query = Query::parse(raw);
    }
    if let Some(params) = params {
        for (key, value) in params {
            query.add(key, &gconv::string(value));
        }
    }
    if !query.is_empty() {
        url = format!(
            "{}?{}",
            url.split('?').next().unwrap_or(&url),
            query.encode()
        );
    }
    url
}

/// Splits a URL into `(host, port, request target)`.
fn split_url(url: &str) -> Result<(String, u16, String), Error> {
    let without_scheme = match url.split_once("://") {
        Some((scheme, rest)) => {
            if scheme != "http" {
                return Err(gerror::new_code(
                    &gcode::CODE_NOT_SUPPORTED,
                    &["only http:// URLs are supported (no TLS yet)"],
                ));
            }
            rest
        }
        None => url,
    };
    let (authority, target) = match without_scheme.find('/') {
        Some(index) => (&without_scheme[..index], &without_scheme[index..]),
        None => (without_scheme, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (
            host.to_owned(),
            port.parse::<u16>().map_err(|_| {
                gerror::new_code(&gcode::CODE_INVALID_PARAMETER, &["invalid URL port"])
            })?,
        ),
        None => (authority.to_owned(), 80),
    };
    if host.is_empty() {
        return Err(gerror::new_code(
            &gcode::CODE_INVALID_PARAMETER,
            &["invalid URL host"],
        ));
    }
    Ok((host, port, target.to_owned()))
}

/// Reads one HTTP/1.1 response from `conn`.
fn read_response(conn: &Conn) -> Result<Response, Error> {
    let mut buffer = Vec::new();
    loop {
        match conn.recv(-1, None) {
            Ok(chunk) if !chunk.is_empty() => buffer.extend_from_slice(&chunk),
            Ok(_) => break,
            Err(_) => break,
        }
    }
    parse_response(&buffer)
}

/// Parses a raw HTTP/1.1 response.
pub fn parse_response(buffer: &[u8]) -> Result<Response, Error> {
    let split = buffer
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or_else(|| gerror::new("invalid HTTP response: missing header terminator"))?;
    let head = String::from_utf8_lossy(&buffer[..split]).into_owned();
    let body = buffer[split + 4..].to_vec();
    let mut lines = head.split("\r\n");
    let status_line = lines
        .next()
        .ok_or_else(|| gerror::new("invalid HTTP response: missing status line"))?;
    let mut parts = status_line.split_whitespace();
    let protocol = parts.next().unwrap_or_default();
    if !protocol.starts_with("HTTP/") {
        return Err(gerror::new(format!(
            "invalid HTTP response status line \"{status_line}\""
        )));
    }
    let status = parts
        .next()
        .and_then(|code| code.parse::<i32>().ok())
        .ok_or_else(|| gerror::new("invalid HTTP response: missing status code"))?;
    let mut headers = Header::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            headers.add(key.trim(), value.trim());
        }
    }
    let mut response = Response::new(status);
    response.headers = headers;
    response.body = body;
    Ok(response)
}

/// A response body view, like GoFrame's `*gclient.Response` content helpers.
#[derive(Clone, Debug)]
pub struct Content {
    /// The parsed body value.
    pub value: gf_core::Value,
}

impl Default for Content {
    fn default() -> Content {
        Content {
            value: gf_core::Value::Null,
        }
    }
}

impl Content {
    /// Parses `response` as JSON when the content type says so, otherwise as
    /// text, like `Response.ReadAll`/`Content`.
    pub fn from_response(response: &Response) -> Content {
        let text = response.text();
        let is_json = response
            .headers
            .get("Content-Type")
            .map(|value| value.contains("application/json") || value.contains("+json"))
            .unwrap_or(false);
        if is_json {
            match gf_json::unmarshal_value(text.as_bytes()) {
                Ok(value) => Content { value },
                Err(_) => Content {
                    value: gf_core::Value::String(text),
                },
            }
        } else {
            Content {
                value: gf_core::Value::String(text),
            }
        }
    }

    /// The string form.
    pub fn to_string_value(&self) -> String {
        gconv::string(&self.value)
    }
}

/// Ensures the response type is exported from the crate root path used by
/// callers.
pub use crate::ghttp::message::Response as HttpResponse;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ghttp::message::Header as ResponseHeader;
    use crate::ghttp::router::Context;
    use crate::ghttp::server::Server;

    fn start_server() -> Server {
        let server = Server::new();
        server.bind_handler(
            "/hello",
            Arc::new(|context: &mut Context| {
                let name = context
                    .request
                    .query_value("name")
                    .unwrap_or("world")
                    .to_owned();
                context.response.set_text(&format!("hello {name}"));
                Ok(())
            }),
        );
        server.bind_handler(
            "/json",
            Arc::new(|context: &mut Context| {
                context.response.set_json(r#"{"ok":true}"#);
                Ok(())
            }),
        );
        server.bind_handler(
            "POST:/echo",
            Arc::new(|context: &mut Context| {
                let body = String::from_utf8_lossy(&context.request.body).into_owned();
                context.response.set_text(&body);
                Ok(())
            }),
        );
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

    fn client_for(server: &Server) -> Client {
        Client::new().set_prefix(&format!("http://127.0.0.1:{}", server.listened_port()))
    }

    #[test]
    fn get_and_query_parameters() {
        let server = start_server();
        let client = client_for(&server);
        assert_eq!(
            client.get_content("/hello", None).expect("get"),
            "hello world"
        );
        let mut params = Params::new();
        params.insert("name".to_owned(), gf_core::Value::String("gf".to_owned()));
        let response = client.get("/hello", Some(&params)).expect("get");
        assert_eq!(response.status, 200);
        assert_eq!(response.text(), "hello gf");
        assert_eq!(
            response.headers.get("Content-Type"),
            Some("text/plain; charset=utf-8")
        );
        server.close().expect("close");
    }

    #[test]
    fn json_content_and_status_codes() {
        let server = start_server();
        let client = client_for(&server);
        let response = client.get("/json", None).expect("json");
        let content = Content::from_response(&response);
        match &content.value {
            gf_core::Value::Map(map) => {
                assert_eq!(map.get("ok"), Some(&gf_core::Value::Bool(true)))
            }
            other => panic!("expected a JSON object, got {other:?}"),
        }
        let response = client.get("/missing", None).expect("404");
        assert_eq!(response.status, 404);
        server.close().expect("close");
    }

    #[test]
    fn post_form_and_json() {
        let server = start_server();
        let client = client_for(&server);
        let mut form = Query::default();
        form.add("a", "1");
        form.add("b", "hello world");
        let response = client.post_form("/echo", &form).expect("form");
        assert_eq!(response.text(), "a=1&b=hello+world");
        let response = client.post_json("/echo", r#"{"x":1}"#).expect("json post");
        assert_eq!(response.text(), r#"{"x":1}"#);
        server.close().expect("close");
    }

    #[test]
    fn headers_and_cookies_are_sent() {
        let server = Server::new();
        server.bind_handler(
            "/inspect",
            Arc::new(|context: &mut Context| {
                let header = context
                    .request
                    .headers
                    .get("X-Test")
                    .unwrap_or("")
                    .to_owned();
                let cookie = context
                    .request
                    .cookies
                    .get("session")
                    .unwrap_or("")
                    .to_owned();
                context.response.set_text(&format!("{header}|{cookie}"));
                Ok(())
            }),
        );
        let running = server.clone();
        std::thread::spawn(move || {
            let _ = running.run();
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while server.listened_port() <= 0 && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let client = client_for(&server)
            .set_header("X-Test", "value")
            .set_cookie("session", "abc");
        assert_eq!(
            client.get_content("/inspect", None).expect("inspect"),
            "value|abc"
        );
        server.close().expect("close");
    }

    #[test]
    fn url_building() {
        assert_eq!(
            build_url("http://example.com", "/api", None),
            "http://example.com/api"
        );
        let mut params = Params::new();
        params.insert("a".to_owned(), gf_core::Value::Int(1));
        params.insert("b".to_owned(), gf_core::Value::String("x y".to_owned()));
        assert_eq!(
            build_url("http://example.com/", "api?c=3", Some(&params)),
            "http://example.com/api?c=3&a=1&b=x+y"
        );
        assert!(split_url("https://example.com").is_err());
        let (host, port, target) = split_url("http://example.com:8080/a").expect("split");
        assert_eq!(host, "example.com");
        assert_eq!(port, 8080);
        assert_eq!(target, "/a");
    }

    #[test]
    fn response_parsing_and_errors() {
        let raw =
            b"HTTP/1.1 201 Created\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nok";
        let response = parse_response(raw).expect("parse");
        assert_eq!(response.status, 201);
        assert_eq!(response.text(), "ok");
        assert_eq!(response.headers.get("Content-Type"), Some("text/plain"));
        assert!(parse_response(b"garbage").is_err());
        assert!(parse_response(b"HTTP/1.1\r\n\r\n").is_err());
    }

    #[test]
    fn response_header_helper_is_visible() {
        let mut headers = ResponseHeader::new();
        headers.set("X", "1");
        let mut response = Response::new(200);
        response.headers = headers;
        assert_eq!(response.headers.get("X"), Some("1"));
    }
}
