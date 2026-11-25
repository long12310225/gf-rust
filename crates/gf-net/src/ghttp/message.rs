//! Port of the HTTP message primitives of `net/ghttp`.
//!
//! GoFrame's `ghttp` is built on `net/http`: `Request` wraps
//! `*http.Request`, `Response` wraps the response writer, and cookies/headers
//! follow the `net/http` helpers. This port defines the equivalent message
//! model over strings so the future server can parse requests off
//! [`crate::gtcp`] connections (ADR-066):
//!
//! - [`Header`] — an insertion-ordered, case-insensitive multi-value header
//!   map with canonical key casing;
//! - [`Cookie`]/[`Cookies`] — parsing `Cookie` and `Set-Cookie` and formatting;
//! - [`Query`] — an ordered multi-value URL query/form map (`+` and `%XX`
//!   decoding, form encoding);
//! - [`Request`] — request line, headers, body and parsed query/cookies;
//! - [`Response`] — status, headers and body, serialized as HTTP/1.1.
//!
//! Multipart forms are parsed by [`Request::multipart_form`] (ADR-154);\n//! chunked transfer encoding and trailers are not ported yet.

use std::io::{BufRead, Write};

use gf_core::Value;

use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use indexmap::IndexMap;

/// A multi-value HTTP header map, like Go's `http.Header`.
///
/// Keys are canonicalized (`content-type` → `Content-Type`) and lookups are
/// case-insensitive; the insertion order is preserved.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Header {
    values: IndexMap<String, Vec<String>>,
}

impl Header {
    /// Creates an empty header map.
    pub fn new() -> Header {
        Header::default()
    }

    /// Sets one value, replacing existing ones, like `Header.Set`.
    pub fn set(&mut self, key: &str, value: &str) {
        self.values
            .insert(canonical_key(key), vec![value.to_owned()]);
    }

    /// Appends one value, like `Header.Add`.
    pub fn add(&mut self, key: &str, value: &str) {
        self.values
            .entry(canonical_key(key))
            .or_default()
            .push(value.to_owned());
    }

    /// Returns the first value, like `Header.Get`.
    pub fn get(&self, key: &str) -> Option<&str> {
        let key = canonical_key(key);
        self.values
            .get(&key)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Returns all values for `key`.
    pub fn values(&self, key: &str) -> Option<&[String]> {
        self.values
            .get(&canonical_key(key))
            .map(|values| values.as_slice())
    }

    /// Removes `key`, like `Header.Del`.
    pub fn del(&mut self, key: &str) {
        self.values.shift_remove(&canonical_key(key));
    }

    /// Reports whether `key` is present.
    pub fn contains(&self, key: &str) -> bool {
        self.values.contains_key(&canonical_key(key))
    }

    /// The number of distinct keys.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether there are no headers.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Iterates over `(key, values)` pairs in insertion order.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.values.iter()
    }
}

/// Canonicalizes a header key, like Go's `textproto.CanonicalMIMEHeaderKey`.
pub fn canonical_key(key: &str) -> String {
    let mut output = String::with_capacity(key.len());
    let mut upper = true;
    for character in key.chars() {
        if character == '-' {
            output.push('-');
            upper = true;
        } else if upper {
            output.push(character.to_ascii_uppercase());
            upper = false;
        } else {
            output.push(character.to_ascii_lowercase());
        }
    }
    output
}

/// One cookie, like Go's `http.Cookie`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cookie {
    /// The cookie name.
    pub name: String,
    /// The cookie value.
    pub value: String,
    /// The `Path` attribute.
    pub path: String,
    /// The `Domain` attribute.
    pub domain: String,
    /// The `Max-Age` attribute, when set.
    pub max_age: Option<i64>,
    /// Whether the `Secure` attribute is present.
    pub secure: bool,
    /// Whether the `HttpOnly` attribute is present.
    pub http_only: bool,
    /// The `SameSite` attribute (`Strict`, `Lax` or `None`).
    pub same_site: String,
}

impl Cookie {
    /// Creates a cookie with `name`/`value`.
    pub fn new(name: &str, value: &str) -> Cookie {
        Cookie {
            name: name.to_owned(),
            value: value.to_owned(),
            ..Cookie::default()
        }
    }

    /// Sets the `Path` attribute.
    pub fn path(mut self, path: &str) -> Cookie {
        self.path = path.to_owned();
        self
    }

    /// Sets the `Domain` attribute.
    pub fn domain(mut self, domain: &str) -> Cookie {
        self.domain = domain.to_owned();
        self
    }

    /// Sets the `Max-Age` attribute.
    pub fn max_age(mut self, seconds: i64) -> Cookie {
        self.max_age = Some(seconds);
        self
    }

    /// Sets the `Secure` flag.
    pub fn secure(mut self, secure: bool) -> Cookie {
        self.secure = secure;
        self
    }

    /// Sets the `HttpOnly` flag.
    pub fn http_only(mut self, http_only: bool) -> Cookie {
        self.http_only = http_only;
        self
    }

    /// Sets the `SameSite` attribute.
    pub fn same_site(mut self, same_site: &str) -> Cookie {
        self.same_site = same_site.to_owned();
        self
    }

    /// Formats the cookie as a `Set-Cookie` value, like `Cookie.String`.
    pub fn to_set_cookie(&self) -> String {
        let mut parts = vec![format!("{}={}", self.name, self.value)];
        if !self.path.is_empty() {
            parts.push(format!("Path={}", self.path));
        }
        if !self.domain.is_empty() {
            parts.push(format!("Domain={}", self.domain));
        }
        if let Some(max_age) = self.max_age {
            parts.push(format!("Max-Age={max_age}"));
        }
        if self.secure {
            parts.push("Secure".to_owned());
        }
        if self.http_only {
            parts.push("HttpOnly".to_owned());
        }
        if !self.same_site.is_empty() {
            parts.push(format!("SameSite={}", self.same_site));
        }
        parts.join("; ")
    }
}

/// A set of request cookies, like GoFrame's `Cookies`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cookies {
    values: IndexMap<String, String>,
}

impl Cookies {
    /// Creates an empty cookie set.
    pub fn new() -> Cookies {
        Cookies::default()
    }

    /// Returns the value of `name`.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    /// Sets one cookie value.
    pub fn set(&mut self, name: &str, value: &str) {
        self.values.insert(name.to_owned(), value.to_owned());
    }

    /// Removes `name`.
    pub fn remove(&mut self, name: &str) {
        self.values.shift_remove(name);
    }

    /// Iterates over the cookies.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &String)> {
        self.values.iter()
    }

    /// The number of cookies.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether there are no cookies.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Parses a `Cookie` request header value, like `Request.Cookie`.
    pub fn parse(header_value: &str) -> Cookies {
        let mut cookies = Cookies::new();
        for part in header_value.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((name, value)) = part.split_once('=') {
                let name = name.trim();
                if name.is_empty() {
                    continue;
                }
                cookies.set(name, value.trim());
            }
        }
        cookies
    }
}

/// Parses one `Set-Cookie` value, like `http.ParseSetCookie`.
pub fn parse_set_cookie(value: &str) -> Cookie {
    let mut cookie = Cookie::default();
    for (index, part) in value.split(';').enumerate() {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, attribute) = match part.split_once('=') {
            Some((key, attribute)) => (key.trim(), attribute.trim()),
            None => (part, ""),
        };
        if index == 0 {
            cookie.name = key.to_owned();
            cookie.value = attribute.to_owned();
            continue;
        }
        match key.to_ascii_lowercase().as_str() {
            "path" => cookie.path = attribute.to_owned(),
            "domain" => cookie.domain = attribute.to_owned(),
            "max-age" => cookie.max_age = attribute.parse::<i64>().ok(),
            "secure" => cookie.secure = true,
            "httponly" => cookie.http_only = true,
            "samesite" => cookie.same_site = attribute.to_owned(),
            _ => {}
        }
    }
    cookie
}

/// An ordered multi-value query/form map, like Go's `url.Values`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    values: IndexMap<String, Vec<String>>,
}

impl Query {
    /// Parses an URL-encoded query string (`a=1&b=2`), like `url.ParseQuery`.
    pub fn parse(raw: &str) -> Query {
        let mut query = Query::default();
        for pair in raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = match pair.split_once('=') {
                Some((key, value)) => (key, value),
                None => (pair, ""),
            };
            let key = url_decode(key).unwrap_or_else(|_| key.to_owned());
            let value = url_decode(value).unwrap_or_else(|_| value.to_owned());
            query.values.entry(key).or_default().push(value);
        }
        query
    }

    /// Returns the first value of `key`, like `Get`.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .get(key)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    /// Returns every value of `key`, like `Values`.
    pub fn values(&self, key: &str) -> &[String] {
        self.values.get(key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Adds one pair.
    pub fn add(&mut self, key: &str, value: &str) {
        self.values
            .entry(key.to_owned())
            .or_default()
            .push(value.to_owned());
    }

    /// The number of distinct keys.
    pub fn len(&self) -> usize {
        self.values.len()
    }

    /// Reports whether the query is empty.
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    /// Iterates over `(key, values)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Vec<String>)> {
        self.values.iter()
    }

    /// Formats the query with form encoding.
    pub fn encode(&self) -> String {
        let mut parts = Vec::new();
        for (key, values) in &self.values {
            for value in values {
                parts.push(format!("{}={}", url_encode(key), url_encode(value)));
            }
        }
        parts.join("&")
    }
}

/// Percent-decodes `value`, turning `+` into a space, like `url.QueryUnescape`.
pub fn url_decode(value: &str) -> Result<String, Error> {
    let bytes = value.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                output.push(b' ');
                index += 1;
            }
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err(gerror::new_code(
                        &gcode::CODE_INVALID_PARAMETER,
                        &["invalid URL escape"],
                    ));
                }
                let high = (bytes[index + 1] as char).to_digit(16).ok_or_else(|| {
                    gerror::new_code(&gcode::CODE_INVALID_PARAMETER, &["invalid URL escape"])
                })?;
                let low = (bytes[index + 2] as char).to_digit(16).ok_or_else(|| {
                    gerror::new_code(&gcode::CODE_INVALID_PARAMETER, &["invalid URL escape"])
                })?;
                output.push((high * 16 + low) as u8);
                index += 3;
            }
            byte => {
                output.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(output).map_err(|error| {
        let message = error.to_string();
        gerror::new_code(&gcode::CODE_INVALID_PARAMETER, &[message.as_str()])
    })
}

/// Form-encodes `value`, like `url.QueryEscape`.
pub fn url_encode(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                output.push(byte as char);
            }
            b' ' => output.push('+'),
            byte => output.push_str(&format!("%{byte:02X}")),
        }
    }
    output
}

/// A parsed HTTP request, like GoFrame's `Request`.
#[derive(Clone, Debug, Default)]
pub struct Request {
    /// The HTTP method (`GET`, `POST`, ...).
    pub method: String,
    /// The request path without the query.
    pub path: String,
    /// The raw query string.
    pub raw_query: String,
    /// The parsed query parameters.
    pub query: Query,
    /// The headers.
    pub headers: Header,
    /// The request cookies.
    pub cookies: Cookies,
    /// The body.
    pub body: Vec<u8>,
    /// The protocol version (`HTTP/1.1`).
    pub protocol: String,
    /// The peer address (`ip:port`) when the server filled it.
    pub remote_addr: String,
}

impl Request {
    /// The client IP, preferring proxy headers like GoFrame's `GetClientIp`.
    pub fn client_ip(&self) -> String {
        if let Some(forwarded) = self.headers.get("X-Forwarded-For") {
            let first = forwarded.split(',').next().unwrap_or("").trim();
            if !first.is_empty() && !first.eq_ignore_ascii_case("unknown") {
                return first.to_owned();
            }
        }
        for name in [
            "Proxy-Client-Ip",
            "Wl-Proxy-Client-Ip",
            "Http_client_ip",
            "Http-X-Forwarded-For",
            "X-Real-Ip",
        ] {
            if let Some(value) = self.headers.get(name)
                && !value.is_empty()
            {
                return value.to_owned();
            }
        }
        self.remote_ip()
    }

    /// The IP part of [`Request::remote_addr`], like `GetRemoteIp`.
    pub fn remote_ip(&self) -> String {
        let address = self.remote_addr.trim();
        if let Some((host, port)) = address.rsplit_once(':')
            && !host.is_empty()
            && port.parse::<u16>().is_ok()
        {
            return host.trim_matches(|ch| ch == '[' || ch == ']').to_owned();
        }
        address.to_owned()
    }

    /// The request scheme (`http`/`https`), like `GetSchema`.
    pub fn schema(&self) -> &'static str {
        let forwarded = self
            .headers
            .get("X-Forwarded-Proto")
            .map(|value| value.eq_ignore_ascii_case("https"))
            .unwrap_or(false);
        if forwarded { "https" } else { "http" }
    }

    /// The full request URL, like `GetUrl`.
    pub fn url(&self) -> String {
        let host = self.headers.get("Host").unwrap_or("");
        if self.raw_query.is_empty() {
            format!("{}://{host}{}", self.schema(), self.path)
        } else {
            format!("{}://{host}{}?{}", self.schema(), self.path, self.raw_query)
        }
    }

    /// Reports whether the request is an AJAX request, like `IsAjaxRequest`.
    pub fn is_ajax_request(&self) -> bool {
        self.headers
            .get("X-Requested-With")
            .map(|value| value.eq_ignore_ascii_case("XMLHttpRequest"))
            .unwrap_or(false)
    }

    /// The `Referer` header, like `GetReferer`.
    pub fn referer(&self) -> String {
        self.headers
            .get("Referer")
            .map(str::to_owned)
            .unwrap_or_default()
    }

    /// Parses one HTTP/1.1 request from `reader`, reading the body according to
    /// `Content-Length`.
    pub fn parse(reader: &mut impl BufRead) -> Result<Request, Error> {
        let mut request = Request::default();
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| gerror::new(format!("read request line failed: {error}")))?;
        if line.is_empty() {
            return Err(gerror::new("empty HTTP request"));
        }
        let parts: Vec<&str> = line.trim_end().split(' ').collect();
        if parts.len() != 3 {
            return Err(gerror::new(format!(
                "invalid request line \"{}\"",
                line.trim_end()
            )));
        }
        request.method = parts[0].to_owned();
        request.protocol = parts[2].to_owned();
        let (path, raw_query) = match parts[1].split_once('?') {
            Some((path, query)) => (path, query),
            None => (parts[1], ""),
        };
        request.path = path.to_owned();
        request.raw_query = raw_query.to_owned();
        request.query = Query::parse(raw_query);

        loop {
            line.clear();
            reader
                .read_line(&mut line)
                .map_err(|error| gerror::new(format!("read header failed: {error}")))?;
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break;
            }
            let (key, value) = trimmed
                .split_once(':')
                .ok_or_else(|| gerror::new(format!("invalid header line \"{trimmed}\"")))?;
            request.headers.add(key.trim(), value.trim());
        }
        if let Some(cookie) = request.headers.get("Cookie") {
            request.cookies = Cookies::parse(cookie);
        }
        let chunked = request
            .headers
            .get("Transfer-Encoding")
            .map(|value| value.to_ascii_lowercase().contains("chunked"))
            .unwrap_or(false);
        if chunked {
            request.body = read_chunked_body(reader)?;
        } else if let Some(length) = request
            .headers
            .get("Content-Length")
            .and_then(|value| value.parse::<usize>().ok())
        {
            request.body = vec![0u8; length];
            reader
                .read_exact(&mut request.body)
                .map_err(|error| gerror::new(format!("read body failed: {error}")))?;
        }
        Ok(request)
    }

    /// Returns the first query value of `key`.
    pub fn query_value(&self, key: &str) -> Option<&str> {
        self.query.get(key)
    }

    /// Parses the body as form data when the content type is
    /// `application/x-www-form-urlencoded`.
    pub fn form(&self) -> Query {
        if self
            .headers
            .get("Content-Type")
            .map(|value| value.starts_with("application/x-www-form-urlencoded"))
            .unwrap_or(false)
        {
            Query::parse(&String::from_utf8_lossy(&self.body))
        } else {
            Query::default()
        }
    }

    /// Returns the request header `name` (ADR-154).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(name)
    }

    /// Returns the body as lossy UTF-8 text (ADR-154).
    pub fn body_string(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Returns the `Host` header, like Go's `Request.Host` (ADR-154).
    pub fn host(&self) -> String {
        self.headers.get("Host").unwrap_or_default().to_owned()
    }

    /// Returns the request URI (path plus the encoded query), like `URL`.
    pub fn uri(&self) -> String {
        if self.query.is_empty() {
            self.path.clone()
        } else {
            format!("{}?{}", self.path, self.query.encode())
        }
    }

    /// Parses a `multipart/form-data` body into its parts (ADR-154).
    pub fn multipart_form(&self) -> Option<MultipartForm> {
        let content_type = self.headers.get("Content-Type")?;
        if !content_type.starts_with("multipart/form-data") {
            return None;
        }
        let boundary = multipart_boundary(content_type)?;
        let delimiter = format!("--{boundary}");
        let separator = format!("\r\n--{boundary}");
        let body = &self.body;
        let mut position = find_bytes(body, delimiter.as_bytes())? + delimiter.len();
        let mut form = MultipartForm {
            boundary,
            parts: Vec::new(),
        };
        loop {
            if body[position..].starts_with(b"\r\n") {
                position += 2;
            }
            if body[position..].starts_with(b"--") {
                break;
            }
            let headers_end = position + find_bytes(&body[position..], b"\r\n\r\n")?;
            let header_text = String::from_utf8_lossy(&body[position..headers_end]).into_owned();
            let data_start = headers_end + 4;
            let data_end = data_start + find_bytes(&body[data_start..], separator.as_bytes())?;
            let (name, file_name, part_type) = parse_part_headers(&header_text);
            form.parts.push(MultipartPart {
                name,
                file_name,
                content_type: part_type,
                data: body[data_start..data_end].to_vec(),
            });
            position = data_end + separator.len();
            if body[position..].starts_with(b"--") {
                break;
            }
        }
        Some(form)
    }
}

/// One parsed part of a `multipart/form-data` body, like GoFrame's upload
/// file view (ADR-154).
#[derive(Clone, Debug, Default)]
pub struct MultipartPart {
    /// The form field name.
    pub name: String,
    /// The uploaded file name (empty for plain fields).
    pub file_name: String,
    /// The part content type.
    pub content_type: String,
    /// The raw part bytes.
    pub data: Vec<u8>,
}

/// A parsed `multipart/form-data` body (ADR-154).
#[derive(Clone, Debug, Default)]
pub struct MultipartForm {
    /// The boundary used by the body.
    pub boundary: String,
    /// Every part in body order.
    pub parts: Vec<MultipartPart>,
}

impl MultipartForm {
    /// Returns the first plain field named `name`.
    pub fn value(&self, name: &str) -> Option<&[u8]> {
        self.parts
            .iter()
            .find(|part| part.name == name && part.file_name.is_empty())
            .map(|part| part.data.as_slice())
    }

    /// Returns the first uploaded file named `name`.
    pub fn file(&self, name: &str) -> Option<&MultipartPart> {
        self.parts
            .iter()
            .find(|part| part.name == name && !part.file_name.is_empty())
    }
}

/// Extracts the boundary from a `multipart/form-data` content type.
fn multipart_boundary(content_type: &str) -> Option<String> {
    for parameter in content_type.split(';').skip(1) {
        if let Some(value) = parameter.trim().strip_prefix("boundary=") {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// Finds `needle` in `haystack`.
fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// Parses a multipart part's `(name, file_name, content_type)`.
fn parse_part_headers(text: &str) -> (String, String, String) {
    let mut name = String::new();
    let mut file_name = String::new();
    let mut content_type = String::new();
    for line in text.split("\r\n") {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().to_ascii_lowercase();
        if key == "content-disposition" {
            for piece in value.split(';').skip(1) {
                let piece = piece.trim();
                if let Some(found) = piece.strip_prefix("name=") {
                    name = found.trim().trim_matches('"').to_owned();
                } else if let Some(found) = piece.strip_prefix("filename=") {
                    file_name = found.trim().trim_matches('"').to_owned();
                }
            }
        } else if key == "content-type" {
            content_type = value.trim().to_owned();
        }
    }
    (name, file_name, content_type)
}

/// Reads a chunked transfer-encoded body (ADR-155).
fn read_chunked_body(reader: &mut impl BufRead) -> Result<Vec<u8>, Error> {
    let mut body = Vec::new();
    loop {
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .map_err(|error| gerror::new(format!("read chunk size failed: {error}")))?;
        let size_text = line.trim().split(';').next().unwrap_or_default().trim();
        let size = usize::from_str_radix(size_text, 16)
            .map_err(|_| gerror::new(format!("invalid chunk size \"{size_text}\"")))?;
        if size == 0 {
            loop {
                let mut trailer = String::new();
                reader
                    .read_line(&mut trailer)
                    .map_err(|error| gerror::new(format!("read trailer failed: {error}")))?;
                if trailer.trim().is_empty() {
                    break;
                }
            }
            break;
        }
        let mut chunk = vec![0u8; size];
        reader
            .read_exact(&mut chunk)
            .map_err(|error| gerror::new(format!("read chunk failed: {error}")))?;
        body.extend_from_slice(&chunk);
        let mut crlf = [0u8; 2];
        reader
            .read_exact(&mut crlf)
            .map_err(|error| gerror::new(format!("read chunk terminator failed: {error}")))?;
        if &crlf != b"\r\n" {
            return Err(gerror::new("invalid chunk terminator"));
        }
    }
    Ok(body)
}

/// One HTTP response, like GoFrame's `Response`.
#[derive(Clone, Debug)]
pub struct Response {
    /// The status code.
    pub status: i32,
    /// The headers.
    pub headers: Header,
    /// The body.
    pub body: Vec<u8>,
}

impl Default for Response {
    fn default() -> Response {
        Response {
            status: 200,
            headers: Header::new(),
            body: Vec::new(),
        }
    }
}

impl Response {
    /// Appends `bytes` to the buffer, like `Write` (ADR-148).
    pub fn write(&mut self, bytes: impl AsRef<[u8]>) {
        self.body.extend_from_slice(bytes.as_ref());
    }

    /// Appends JSON-encoded `value` to the buffer, like `WriteJson`
    /// (ADR-148).
    pub fn write_json(&mut self, value: &Value) -> Result<(), Error> {
        let text = gf_json::marshal_value(value)?;
        self.headers.set("Content-Type", "application/json");
        self.body.extend_from_slice(text.as_bytes());
        Ok(())
    }

    /// Sets the status and appends `content`, like `WriteStatus` (ADR-148).
    pub fn write_status(&mut self, status: i32, content: &str) {
        self.status = status;
        self.body.extend_from_slice(content.as_bytes());
    }

    /// Returns a copy of the buffer, like `Buffer` (ADR-148).
    pub fn buffer(&self) -> Vec<u8> {
        self.body.clone()
    }

    /// Returns the buffer length, like `BufferLength` (ADR-148).
    pub fn buffer_length(&self) -> usize {
        self.body.len()
    }

    /// Empties the buffer, like `ClearBuffer` (ADR-148).
    pub fn clear_buffer(&mut self) {
        self.body.clear();
    }

    /// Sets the status code, like `SetStatus`.
    pub fn set_status(&mut self, status: i32) {
        self.status = status;
    }

    /// The status code, like `Status`.
    pub fn status_code(&self) -> i32 {
        self.status
    }

    /// Returns the response header `name`.
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers.get(name).map(str::to_owned)
    }

    /// Sets the response header `name` to `value`.
    pub fn set_header(&mut self, name: &str, value: &str) {
        self.headers.set(name, value);
    }

    /// Creates a response with `status`.
    pub fn new(status: i32) -> Response {
        Response {
            status,
            ..Response::default()
        }
    }

    /// Sets the body and the `Content-Length` header.
    pub fn set_body(&mut self, body: &[u8]) {
        self.body = body.to_vec();
        self.headers
            .set("Content-Length", &self.body.len().to_string());
    }

    /// Sets a `text/plain; charset=utf-8` body.
    pub fn set_text(&mut self, body: &str) {
        self.headers
            .set("Content-Type", "text/plain; charset=utf-8");
        self.set_body(body.as_bytes());
    }

    /// Returns the body as text.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// Sets an `application/json` body.
    pub fn set_json(&mut self, body: &str) {
        self.headers
            .set("Content-Type", "application/json; charset=utf-8");
        self.set_body(body.as_bytes());
    }

    /// Appends a `Set-Cookie` header, like `Response.SetCookie`.
    pub fn add_cookie(&mut self, cookie: &Cookie) {
        self.headers.add("Set-Cookie", &cookie.to_set_cookie());
    }

    /// Alias of [`Response::add_cookie`].
    pub fn set_cookie(&mut self, cookie: &Cookie) {
        self.add_cookie(cookie);
    }

    /// Parses the response `Set-Cookie` headers.
    pub fn cookies(&self) -> Vec<Cookie> {
        self.headers
            .values("Set-Cookie")
            .map(|values| values.iter().map(|value| parse_set_cookie(value)).collect())
            .unwrap_or_default()
    }

    /// Writes the response as HTTP/1.1, like GoFrame's response writer.
    pub fn write_to(&self, writer: &mut impl Write) -> Result<(), Error> {
        let mut head = format!(
            "HTTP/1.1 {} {}\r\n",
            self.status,
            crate::ghttp::response::status_text(self.status)
        );
        let mut headers = self.headers.clone();
        if !headers.contains("Content-Length") {
            headers.set("Content-Length", &self.body.len().to_string());
        }
        for (key, values) in headers.iter() {
            for value in values {
                head.push_str(&format!("{key}: {value}\r\n"));
            }
        }
        head.push_str("\r\n");
        writer
            .write_all(head.as_bytes())
            .and_then(|_| writer.write_all(&self.body))
            .and_then(|_| writer.flush())
            .map_err(|error| gerror::new(format!("write response failed: {error}")))
    }

    /// The serialized response as a string (for tests and inspection).
    pub fn to_http_string(&self) -> String {
        let mut buffer = Vec::new();
        self.write_to(&mut buffer)
            .expect("response write to buffer");
        String::from_utf8_lossy(&buffer).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    #[test]
    fn header_is_case_insensitive_and_ordered() {
        let mut header = Header::new();
        header.set("content-type", "text/plain");
        header.add("X-Test", "a");
        header.add("x-test", "b");
        assert_eq!(header.get("Content-Type"), Some("text/plain"));
        assert_eq!(header.get("CONTENT-TYPE"), Some("text/plain"));
        assert_eq!(header.values("X-Test").expect("values"), ["a", "b"]);
        assert_eq!(header.len(), 2);
        header.del("x-test");
        assert!(!header.contains("X-Test"));
        assert_eq!(canonical_key("content-TYPE"), "Content-Type");
        assert_eq!(canonical_key("x-custom-header"), "X-Custom-Header");
    }

    #[test]
    fn cookies_parse_and_format() {
        let cookies = Cookies::parse("a=1; b=hello%20world; empty=");
        assert_eq!(cookies.get("a"), Some("1"));
        assert_eq!(cookies.get("b"), Some("hello%20world"));
        assert_eq!(cookies.get("missing"), None);
        assert_eq!(cookies.len(), 3);

        let cookie = parse_set_cookie(
            "session=abc123; Path=/; Domain=example.com; Max-Age=3600; Secure; HttpOnly; SameSite=Lax",
        );
        assert_eq!(cookie.name, "session");
        assert_eq!(cookie.value, "abc123");
        assert_eq!(cookie.path, "/");
        assert_eq!(cookie.domain, "example.com");
        assert_eq!(cookie.max_age, Some(3600));
        assert!(cookie.secure);
        assert!(cookie.http_only);
        assert_eq!(
            cookie.to_set_cookie(),
            "session=abc123; Path=/; Domain=example.com; Max-Age=3600; Secure; HttpOnly; SameSite=Lax"
        );
    }

    #[test]
    fn query_parsing_and_encoding() {
        let query = Query::parse("name=john%20doe&tags=a&tags=b&empty=");
        assert_eq!(query.get("name"), Some("john doe"));
        assert_eq!(query.values("tags"), ["a", "b"]);
        assert_eq!(query.get("empty"), Some(""));
        assert_eq!(query.len(), 3);
        assert_eq!(query.encode(), "name=john+doe&tags=a&tags=b&empty=");
        assert_eq!(url_encode("a b&c"), "a+b%26c");
        assert_eq!(url_decode("a+b%26c").expect("decode"), "a b&c");
        assert!(url_decode("%zz").is_err());
        assert!(url_decode("%2").is_err());
    }

    #[test]
    fn request_parsing_with_query_and_body() {
        let raw = "POST /api/user?id=100&name=john HTTP/1.1\r\n\
                   Host: example.com\r\n\
                   Content-Type: application/x-www-form-urlencoded\r\n\
                   Content-Length: 9\r\n\
                   Cookie: a=1; b=2\r\n\
                   \r\n\
                   x=1&y=abc";
        let mut reader = Cursor::new(raw.as_bytes());
        let request = Request::parse(&mut reader).expect("request");
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/api/user");
        assert_eq!(request.protocol, "HTTP/1.1");
        assert_eq!(request.query_value("id"), Some("100"));
        assert_eq!(request.query_value("name"), Some("john"));
        assert_eq!(request.cookies.get("b"), Some("2"));
        assert_eq!(request.body, b"x=1&y=abc");
        let form = request.form();
        assert_eq!(form.get("x"), Some("1"));
        assert_eq!(form.get("y"), Some("abc"));
    }

    #[test]
    fn request_rejects_malformed_input() {
        let mut reader = Cursor::new(b"not-a-request\r\n\r\n");
        assert!(Request::parse(&mut reader).is_err());
        let mut reader = Cursor::new(b"GET / HTTP/1.1\r\nBadHeader\r\n\r\n");
        assert!(Request::parse(&mut reader).is_err());
        let mut reader = Cursor::new(b"");
        assert!(Request::parse(&mut reader).is_err());
    }

    #[test]
    fn response_serialization() {
        let mut response = Response::new(200);
        response.set_json(r#"{"ok":true}"#);
        let text = response.to_http_string();
        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"), "{text}");
        assert!(text.contains("Content-Type: application/json; charset=utf-8\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"), "{text}");

        let mut response = Response::new(404);
        response.set_text("missing");
        let text = response.to_http_string();
        assert!(text.starts_with("HTTP/1.1 404 Not Found\r\n"), "{text}");
    }

    #[test]
    fn response_writes_to_arbitrary_writer() {
        let response = Response::new(204);
        let mut buffer: Vec<u8> = Vec::new();
        response.write_to(&mut buffer).expect("write");
        assert!(buffer.starts_with(b"HTTP/1.1 204 No Content\r\n"));
        assert!(buffer.ends_with(b"\r\n\r\n"));
    }

    #[test]
    fn cookie_options_and_response_cookies() {
        let cookie = Cookie::new("session", "abc")
            .path("/")
            .domain("example.com")
            .max_age(60)
            .secure(true)
            .http_only(true)
            .same_site("Lax");
        assert_eq!(
            cookie.to_set_cookie(),
            "session=abc; Path=/; Domain=example.com; Max-Age=60; Secure; HttpOnly; SameSite=Lax"
        );
        let mut response = Response::new(200);
        response.set_cookie(&cookie);
        response.add_cookie(&Cookie::new("theme", "dark"));
        let cookies = response.cookies();
        assert_eq!(cookies.len(), 2);
        assert_eq!(cookies[0].name, "session");
        assert!(cookies[0].http_only && cookies[0].secure);
        assert_eq!(cookies[1].name, "theme");
        assert_eq!(
            response
                .headers
                .values("Set-Cookie")
                .expect("set-cookie")
                .len(),
            2
        );
    }

    #[test]
    fn request_client_ip_and_helpers() {
        let mut request = Request {
            method: "GET".to_owned(),
            path: "/api/user".to_owned(),
            raw_query: "a=1".to_owned(),
            remote_addr: "10.0.0.5:5555".to_owned(),
            ..Request::default()
        };
        request.headers.set("Host", "example.com");
        assert_eq!(request.remote_ip(), "10.0.0.5");
        assert_eq!(request.client_ip(), "10.0.0.5");
        assert_eq!(request.url(), "http://example.com/api/user?a=1");
        assert!(!request.is_ajax_request());
        assert_eq!(request.referer(), "");

        request.headers.set("X-Forwarded-For", " 1.1.1.1, 2.2.2.2");
        assert_eq!(request.client_ip(), "1.1.1.1");
        request.headers.set("X-Forwarded-For", "unknown");
        request.headers.set("X-Real-Ip", "3.3.3.3");
        assert_eq!(request.client_ip(), "3.3.3.3");
        // Proxy-Client-Ip wins over X-Real-Ip in the fallback order.
        request.headers.set("Proxy-Client-Ip", "4.4.4.4");
        assert_eq!(request.client_ip(), "4.4.4.4");

        request.headers.set("X-Forwarded-Proto", "https");
        assert_eq!(request.schema(), "https");
        assert_eq!(request.url(), "https://example.com/api/user?a=1");

        request.headers.set("X-Requested-With", "XMLHttpRequest");
        assert!(request.is_ajax_request());
        request.headers.set("Referer", "https://ref.example");
        assert_eq!(request.referer(), "https://ref.example");
    }

    #[test]
    fn remote_ip_handles_brackets_and_bare_addresses() {
        let mut request = Request {
            remote_addr: "[::1]:8080".to_owned(),
            ..Request::default()
        };
        assert_eq!(request.remote_ip(), "::1");
        request.remote_addr = "192.168.1.1".to_owned();
        assert_eq!(request.remote_ip(), "192.168.1.1");
        request.remote_addr = String::new();
        assert_eq!(request.remote_ip(), "");
    }

    #[test]
    fn response_write_buffer_helpers() {
        let mut response = Response::new(200);
        response.write("abc");
        assert_eq!(response.buffer(), b"abc");
        assert_eq!(response.buffer_length(), 3);

        response.write_status(201, "de");
        assert_eq!(response.status_code(), 201);
        assert_eq!(response.buffer(), b"abcde");

        response.clear_buffer();
        assert_eq!(response.buffer_length(), 0);

        response
            .write_json(&Value::Map(IndexMap::from([(
                "k".to_owned(),
                Value::Int(1),
            )])))
            .expect("json");
        assert_eq!(
            response.header("Content-Type").as_deref(),
            Some("application/json")
        );
        assert_eq!(response.text(), r#"{"k":1}"#);

        response.set_status(204);
        assert_eq!(response.status, 204);
        response.set_header("X-Test", "yes");
        assert_eq!(response.header("X-Test").as_deref(), Some("yes"));
    }

    #[test]
    fn multipart_form_and_request_helpers() {
        let body = "--X\r\nContent-Disposition: form-data; name=\"field\"\r\n\r\nvalue\r\n\
                    --X\r\nContent-Disposition: form-data; name=\"upload\"; filename=\"a.txt\"\r\n\
                    Content-Type: text/plain\r\n\r\nfiledata\r\n--X--\r\n";
        let mut request = Request {
            method: "POST".to_owned(),
            path: "/upload".to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        };
        request
            .headers
            .set("Content-Type", "multipart/form-data; boundary=X");
        request.headers.set("Host", "example.com");
        request.body = body.as_bytes().to_vec();

        let form = request.multipart_form().expect("form");
        assert_eq!(form.boundary, "X");
        assert_eq!(form.parts.len(), 2);
        assert_eq!(form.value("field"), Some(b"value".as_slice()));
        let file = form.file("upload").expect("file");
        assert_eq!(file.file_name, "a.txt");
        assert_eq!(file.content_type, "text/plain");
        assert_eq!(file.data, b"filedata");

        assert_eq!(request.header("host"), Some("example.com"));
        assert_eq!(request.host(), "example.com");
        assert_eq!(request.body_string(), body);
        assert_eq!(request.uri(), "/upload");
        request.query.add("q", "1");
        assert_eq!(request.uri(), "/upload?q=1");

        request.headers.set("Content-Type", "application/json");
        assert!(request.multipart_form().is_none());
    }

    #[test]
    fn chunked_request_body() {
        let raw = "POST /upload HTTP/1.1\r\nHost: example.com\r\n\
                   Transfer-Encoding: chunked\r\n\r\n4\r\nWiki\r\n5\r\npedia\r\n0\r\n\r\n";
        let mut reader = std::io::Cursor::new(raw.as_bytes());
        let request = Request::parse(&mut reader).expect("parse");
        assert_eq!(request.body, b"Wikipedia");
        assert_eq!(request.host(), "example.com");
        assert_eq!(request.uri(), "/upload");

        let bad = "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\nzz\r\n";
        assert!(Request::parse(&mut std::io::Cursor::new(bad.as_bytes())).is_err());
    }
}
