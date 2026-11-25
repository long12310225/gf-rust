//! Port of `net/ghttp` static file serving (`ghttp_server_config_static.go`
//! and the static half of `ghttp_server_handler.go`).
//!
//! Search order matches GoFrame: static path mappings (longest prefix first),
//! then the server root and search paths. A directory serves its configured
//! index file, or a listing when `index_folder` is enabled, otherwise 403.
//! File paths are percent-decoded, `..` is rejected and the resolved path must
//! stay under its root (ADR-068).
//!
//! Not ported: `gres` resources for static files, `ServeContent` range
//! requests; `Last-Modified`/`ETag` and conditional `If-None-Match`/
//! `If-Modified-Since` responses are supported (ADR-113).

use std::path::{Path, PathBuf};

use gf_errors::gerror::{self, Error};

use super::message::{Header, Response, url_decode};

/// One URI-prefix to directory mapping, like Go's `staticPathItem`.
#[derive(Clone, Debug)]
pub struct StaticPath {
    /// The URI prefix.
    pub prefix: String,
    /// The directory path.
    pub path: String,
}

/// The static file configuration, like GoFrame's `config` static fields.
#[derive(Clone, Debug)]
pub struct StaticConfig {
    /// The document root; search paths holds it first.
    pub server_root: Option<String>,
    /// Additional search paths.
    pub search_paths: Vec<String>,
    /// URI prefix mappings, sorted by prefix length descending.
    pub static_paths: Vec<StaticPath>,
    /// Index file names.
    pub index_files: Vec<String>,
    /// Whether directory listings are enabled.
    pub index_folder: bool,
    /// Whether the static file service is enabled.
    pub enabled: bool,
}

impl Default for StaticConfig {
    fn default() -> StaticConfig {
        StaticConfig {
            server_root: None,
            search_paths: Vec::new(),
            static_paths: Vec::new(),
            index_files: vec!["index.html".to_owned()],
            index_folder: false,
            enabled: false,
        }
    }
}

/// A found static target.
#[derive(Clone, Debug)]
struct FoundFile {
    path: PathBuf,
    is_dir: bool,
}

/// The result of serving one static request.
pub enum ServeResult {
    /// Serve the file.
    File {
        /// The file content.
        body: Vec<u8>,
        /// The response content type.
        content_type: String,
        /// The modification time as Unix seconds, when available.
        modified: Option<u64>,
    },
    /// Serve a directory listing.
    Listing {
        /// The HTML listing.
        body: String,
    },
    /// The request is forbidden.
    Forbidden,
}

/// Canonicalizes a directory, returning an error when it does not exist.
fn resolve_directory(path: &str, operation: &str) -> Result<String, Error> {
    let canonical = std::fs::canonicalize(path).map_err(|error| {
        gerror::new(format!(
            "{operation} failed: path \"{path}\" does not exist: {error}"
        ))
    })?;
    if !canonical.is_dir() {
        return Err(gerror::new(format!(
            "{operation} failed: path \"{path}\" is not a directory"
        )));
    }
    Ok(canonical.to_string_lossy().into_owned())
}

impl StaticConfig {
    /// Sets the document root, like `SetServerRoot`.
    pub fn set_server_root(&mut self, root: &str) -> Result<(), Error> {
        let real = resolve_directory(root, "SetServerRoot")?;
        self.server_root = Some(real.clone());
        self.search_paths = vec![real];
        self.enabled = true;
        Ok(())
    }

    /// Adds a search path, like `AddSearchPath`.
    pub fn add_search_path(&mut self, path: &str) -> Result<(), Error> {
        let real = resolve_directory(path, "AddSearchPath")?;
        self.search_paths.push(real);
        self.enabled = true;
        Ok(())
    }

    /// Maps a URI prefix to a directory, like `AddStaticPath`.
    pub fn add_static_path(&mut self, prefix: &str, path: &str) -> Result<(), Error> {
        let real = resolve_directory(path, "AddStaticPath")?;
        self.static_paths.push(StaticPath {
            prefix: prefix.trim_end_matches('/').to_owned(),
            path: real,
        });
        self.static_paths.sort_by(|left, right| {
            right
                .prefix
                .len()
                .cmp(&left.prefix.len())
                .then_with(|| right.prefix.cmp(&left.prefix))
        });
        self.enabled = true;
        Ok(())
    }
}

/// Serves the static request for `uri`, returning `None` when nothing matched.
pub fn serve(config: &StaticConfig, uri: &str) -> Option<ServeResult> {
    if !config.enabled {
        return None;
    }
    let found = search(config, uri)?;
    Some(serve_found(config, uri, found))
}

/// Searches the configured roots for `uri`.
fn search(config: &StaticConfig, uri: &str) -> Option<FoundFile> {
    let decoded = decode_uri_path(uri)?;
    for mapping in &config.static_paths {
        if let Some(rest) = strip_prefix_ci(&decoded, &mapping.prefix) {
            if !rest.is_empty() && !rest.starts_with('/') {
                continue;
            }
            if let Some(found) = resolve(&mapping.path, rest, &config.index_files) {
                return Some(found);
            }
        }
    }
    for base in &config.search_paths {
        if let Some(found) = resolve(base, &decoded, &config.index_files) {
            return Some(found);
        }
    }
    None
}

/// Resolves `relative` under `root`, rejecting escapes.
fn resolve(root: &str, relative: &str, index_files: &[String]) -> Option<FoundFile> {
    let root_path = Path::new(root);
    let canonical_root = std::fs::canonicalize(root_path).ok()?;
    let candidate = root_path.join(relative.trim_start_matches('/'));
    if !candidate.starts_with(root_path) {
        return None;
    }
    // Every existing ancestor must stay under the canonical root (symlinks).
    let mut ancestor = candidate.as_path();
    loop {
        if ancestor.exists() {
            let resolved = std::fs::canonicalize(ancestor).ok()?;
            if !resolved.starts_with(&canonical_root) {
                return None;
            }
            break;
        }
        match ancestor.parent() {
            Some(parent) => ancestor = parent,
            None => break,
        }
    }
    if !candidate.exists() {
        return None;
    }
    if candidate.is_dir() {
        for index_file in index_files {
            let index = candidate.join(index_file);
            if index.is_file() {
                return Some(FoundFile {
                    path: index,
                    is_dir: false,
                });
            }
        }
        return Some(FoundFile {
            path: candidate,
            is_dir: true,
        });
    }
    Some(FoundFile {
        path: candidate,
        is_dir: false,
    })
}

/// Serves a found file or directory.
fn serve_found(config: &StaticConfig, uri: &str, found: FoundFile) -> ServeResult {
    if found.is_dir {
        if !config.index_folder {
            return ServeResult::Forbidden;
        }
        return match list_dir(uri, &found.path) {
            Ok(body) => ServeResult::Listing { body },
            Err(_) => ServeResult::Forbidden,
        };
    }
    let modified = std::fs::metadata(&found.path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs());
    match std::fs::read(&found.path) {
        Ok(body) => ServeResult::File {
            body,
            content_type: content_type(&found.path),
            modified,
        },
        Err(_) => ServeResult::Forbidden,
    }
}

/// Decodes `uri`, rejecting traversal segments.
fn decode_uri_path(uri: &str) -> Option<String> {
    let decoded = url_decode(uri).ok()?;
    if decoded.split('/').any(|segment| segment == "..") {
        return None;
    }
    Some(decoded)
}

/// Strips `prefix` case-insensitively from `path`.
fn strip_prefix_ci<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    if path.len() >= prefix.len() && path[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&path[prefix.len()..])
    } else {
        None
    }
}

/// Builds the directory listing HTML, like GoFrame's `listDir`.
fn list_dir(uri: &str, directory: &Path) -> Result<String, std::io::Error> {
    let mut entries: Vec<(String, bool, u64)> = Vec::new();
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let metadata = entry.metadata()?;
        entries.push((
            entry.file_name().to_string_lossy().into_owned(),
            metadata.is_dir(),
            metadata.len(),
        ));
    }
    entries.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    let mut html = String::new();
    html.push_str("<html><head><style>");
    html.push_str(
        "body {font-family:Consolas, Monaco, \"Andale Mono\", \"Ubuntu Mono\", monospace;}",
    );
    html.push_str("</style></head><body>");
    html.push_str(&format!("<h1>Index of {}</h1><hr />", html_escape(uri)));
    html.push_str("<table>");
    let prefix = uri.trim_end_matches('/');
    if uri != "/" {
        let parent = match prefix.rfind('/') {
            Some(0) | None => "/".to_owned(),
            Some(index) => prefix[..index].to_owned(),
        };
        html.push_str(&format!(
            "<tr><td><a href=\"{}\">..</a></td></tr>",
            dir_href(&parent)
        ));
    }
    for (name, is_dir, size) in entries {
        let display = if is_dir {
            format!("{name}/")
        } else {
            name.clone()
        };
        let size_text = if is_dir {
            "-".to_owned()
        } else {
            format!("{size} B")
        };
        html.push_str("<tr>");
        html.push_str(&format!(
            "<td><a href=\"{}\">{}</a></td>",
            dir_href(&format!("{prefix}/{display}")),
            html_escape(&display)
        ));
        html.push_str(&format!(
            "<td style=\"width:80px;text-align:right;\">{size_text}</td>"
        ));
        html.push_str("</tr>");
    }
    html.push_str("</table></body></html>");
    Ok(html)
}

/// Formats a directory href: URL path-escaped and then HTML-escaped.
fn dir_href(path: &str) -> String {
    html_escape(&path_escape(path))
}

/// Escapes a URL path like Go's `url.URL{Path}.String()`.
pub fn path_escape(path: &str) -> String {
    let mut output = String::with_capacity(path.len());
    for byte in path.bytes() {
        let allowed = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'-' | b'.'
                    | b'_'
                    | b'~'
                    | b'$'
                    | b'&'
                    | b'+'
                    | b','
                    | b'/'
                    | b':'
                    | b';'
                    | b'='
                    | b'@'
            );
        if allowed {
            output.push(byte as char);
        } else {
            output.push_str(&format!("%{byte:02X}"));
        }
    }
    output
}

/// Escapes HTML text like Go's `html.EscapeString`.
pub fn html_escape(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' => output.push_str("&#34;"),
            '\'' => output.push_str("&#39;"),
            other => output.push(other),
        }
    }
    output
}

/// Returns the content type from the file extension.
fn content_type(path: &Path) -> String {
    let extension = path
        .extension()
        .map(|extension| extension.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let content_type = match extension.as_str() {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "application/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "txt" | "md" => "text/plain; charset=utf-8",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "ico" => "image/x-icon",
        "pdf" => "application/pdf",
        "wasm" => "application/wasm",
        _ => "application/octet-stream",
    };
    content_type.to_owned()
}

/// Applies a [`ServeResult`] to a response.
pub fn apply(result: ServeResult) -> Response {
    match result {
        ServeResult::File {
            body,
            content_type,
            modified,
        } => {
            let mut response = Response::new(200);
            response.headers.set("Content-Type", &content_type);
            if let Some(modified) = modified {
                response.headers.set("Last-Modified", &http_date(modified));
            }
            response
                .headers
                .set("ETag", &etag_for(&content_type, &body, modified));
            response.set_body(&body);
            response
        }
        ServeResult::Listing { body } => {
            let mut response = Response::new(200);
            response
                .headers
                .set("Content-Type", "text/html; charset=utf-8");
            response.set_body(body.as_bytes());
            response
        }
        ServeResult::Forbidden => Response::new(403),
    }
}

/// Applies a [`ServeResult`] with conditional-GET handling (ADR-113).
pub fn apply_conditional(result: ServeResult, request_headers: &Header) -> Response {
    let modified = match &result {
        ServeResult::File { modified, .. } => *modified,
        _ => None,
    };
    let response = apply(result);
    if response.status != 200 {
        return response;
    }
    if let Some(etag) = response.headers.get("ETag")
        && let Some(condition) = request_headers.get("If-None-Match")
        && condition
            .split(',')
            .any(|candidate| candidate.trim() == etag)
    {
        return not_modified(response);
    }
    if let Some(modified) = modified
        && let Some(since) = request_headers
            .get("If-Modified-Since")
            .and_then(parse_http_date)
        && since >= modified
    {
        return not_modified(response);
    }
    response
}

/// Turns a response into `304 Not Modified`, keeping the validators.
fn not_modified(mut response: Response) -> Response {
    response.status = 304;
    response.body.clear();
    response.headers.set("Content-Length", "0");
    response
}

/// Builds a weak `ETag` from the content type, length and modification time.
fn etag_for(content_type: &str, body: &[u8], modified: Option<u64>) -> String {
    let stamp = modified.unwrap_or_default();
    format!(
        "W/\"{:x}-{:x}-{:x}\"",
        content_type.len(),
        body.len(),
        stamp
    )
}

/// Formats Unix seconds as an IMF-fixdate, like `http.TimeFormat`.
pub fn http_date(timestamp: u64) -> String {
    const WEEKDAYS: [&str; 7] = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"];
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let days = (timestamp / 86_400) as i64;
    let seconds = timestamp % 86_400;
    let (year, month, day) = civil_from_days(days);
    let weekday = WEEKDAYS[days.rem_euclid(7) as usize];
    format!(
        "{weekday}, {day:02} {} {year:04} {:02}:{:02}:{:02} GMT",
        MONTHS[(month - 1) as usize],
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Parses an IMF-fixdate (`Sun, 06 Nov 1994 08:49:37 GMT`) into Unix seconds.
pub fn parse_http_date(text: &str) -> Option<u64> {
    let text = text.trim();
    let rest = text.split_once(", ").map(|(_, rest)| rest).unwrap_or(text);
    let mut parts = rest.split_whitespace();
    let day = parts.next()?.parse::<u32>().ok()?;
    let month = month_number(parts.next()?)?;
    let year = parts.next()?.parse::<i64>().ok()?;
    let time = parts.next()?;
    let mut clock = time.split(':');
    let hour = clock.next()?.parse::<u64>().ok()?;
    let minute = clock.next()?.parse::<u64>().ok()?;
    let second = clock.next()?.parse::<u64>().ok()?;
    if hour > 23 || minute > 59 || second > 60 || day == 0 || day > 31 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if days < 0 {
        return None;
    }
    Some(days as u64 * 86_400 + hour * 3600 + minute * 60 + second)
}

/// Maps a three-letter month name to its number.
fn month_number(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    MONTHS
        .iter()
        .position(|month| month.eq_ignore_ascii_case(name))
        .map(|index| index as u32 + 1)
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The civil date for days since 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = (month_prime + 2) % 12 + 1;
    (
        if month <= 2 { year + 1 } else { year },
        month as u32,
        day as u32,
    )
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    fn sandbox(name: &str) -> String {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "gf-rust-ghttp-static-{}-{}-{}",
            std::process::id(),
            name,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("sandbox");
        path.to_string_lossy().into_owned()
    }

    fn write(path: &str, content: &str) {
        std::fs::write(path, content).expect("write");
    }

    fn join(base: &str, name: &str) -> String {
        Path::new(base).join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn serves_files_with_content_type() {
        let root = sandbox("files");
        write(&join(&root, "index.html"), "<h1>home</h1>");
        write(&join(&root, "style.css"), "body{}");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");
        match serve(&config, "/").expect("index") {
            ServeResult::File {
                body, content_type, ..
            } => {
                assert_eq!(body, b"<h1>home</h1>");
                assert_eq!(content_type, "text/html; charset=utf-8");
            }
            _ => panic!("expected a file"),
        }
        match serve(&config, "/style.css").expect("css") {
            ServeResult::File { content_type, .. } => {
                assert_eq!(content_type, "text/css; charset=utf-8")
            }
            _ => panic!("expected a file"),
        }
        assert!(serve(&config, "/missing.txt").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn directory_without_index_is_forbidden() {
        let root = sandbox("forbidden");
        std::fs::create_dir_all(join(&root, "sub")).expect("sub");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");
        assert!(matches!(
            serve(&config, "/sub").expect("dir"),
            ServeResult::Forbidden
        ));
        config.index_folder = true;
        assert!(matches!(
            serve(&config, "/sub").expect("dir"),
            ServeResult::Listing { .. }
        ));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn listing_escapes_names_and_hrefs() {
        let root = sandbox("listing");
        write(&join(&root, "a&b #c.txt"), "test");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");
        config.index_folder = true;
        match serve(&config, "/").expect("listing") {
            ServeResult::Listing { body } => {
                assert!(body.contains("Index of /"), "{body}");
                assert!(body.contains("href=\"/a&amp;b%20%23c.txt\""), "{body}");
                assert!(body.contains("a&amp;b #c.txt"), "{body}");
            }
            _ => panic!("expected a listing"),
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn traversal_is_rejected() {
        let base = sandbox("traversal");
        let root = join(&base, "root");
        std::fs::create_dir_all(&root).expect("root");
        write(&join(&base, "secret.txt"), "secret");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");
        assert!(serve(&config, "/../secret.txt").is_none());
        assert!(serve(&config, "/%2e%2e/secret.txt").is_none());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn static_path_prefix_mapping() {
        let root = sandbox("prefix");
        write(&join(&root, "app.js"), "console.log(1)");
        let mut config = StaticConfig::default();
        config.add_static_path("/static", &root).expect("mapping");
        match serve(&config, "/static/app.js").expect("mapped") {
            ServeResult::File { body, .. } => assert_eq!(body, b"console.log(1)"),
            _ => panic!("expected a file"),
        }
        assert!(serve(&config, "/staticx/app.js").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn disabled_or_missing_root_serves_nothing() {
        let root = sandbox("disabled");
        write(&join(&root, "a.txt"), "a");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");
        config.enabled = false;
        assert!(serve(&config, "/a.txt").is_none());
        let mut config = StaticConfig::default();
        assert!(config.set_server_root("definitely-missing-root").is_err());
        assert!(
            config
                .add_static_path("/x", "definitely-missing-root")
                .is_err()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn escaping_helpers_match_go() {
        assert_eq!(
            path_escape("/<img src=x onerror=alert(1)>/x\"y.txt"),
            "/%3Cimg%20src=x%20onerror=alert%281%29%3E/x%22y.txt"
        );
        assert_eq!(path_escape("/a&b #c.txt"), "/a&b%20%23c.txt");
        assert_eq!(
            html_escape("<img src=x onerror=alert(1)>"),
            "&lt;img src=x onerror=alert(1)&gt;"
        );
        assert_eq!(html_escape("a&b"), "a&amp;b");
    }

    #[test]
    fn http_date_roundtrip() {
        assert_eq!(http_date(784_111_777), "Sun, 06 Nov 1994 08:49:37 GMT");
        assert_eq!(
            parse_http_date("Sun, 06 Nov 1994 08:49:37 GMT"),
            Some(784_111_777)
        );
        assert_eq!(parse_http_date("Thu, 01 Jan 1970 00:00:00 GMT"), Some(0));
        assert_eq!(parse_http_date("not a date"), None);
        assert_eq!(parse_http_date("Sun, 32 Nov 1994 08:49:37 GMT"), None);
    }

    #[test]
    fn conditional_get_returns_not_modified() {
        let root = sandbox("conditional");
        write(&join(&root, "index.html"), "<h1>home</h1>");
        let mut config = StaticConfig::default();
        config.set_server_root(&root).expect("root");

        let result = serve(&config, "/").expect("index");
        let response = apply_conditional(result, &Header::new());
        assert_eq!(response.status, 200);
        let etag = response.headers.get("ETag").expect("etag").to_owned();
        let last_modified = response
            .headers
            .get("Last-Modified")
            .expect("last-modified")
            .to_owned();
        assert!(last_modified.ends_with(" GMT"));

        let mut headers = Header::new();
        headers.set("If-None-Match", &etag);
        let result = serve(&config, "/").expect("index");
        let response = apply_conditional(result, &headers);
        assert_eq!(response.status, 304);
        assert!(response.body.is_empty());
        assert_eq!(response.headers.get("ETag"), Some(etag.as_str()));

        let mut headers = Header::new();
        headers.set("If-Modified-Since", &last_modified);
        let result = serve(&config, "/").expect("index");
        assert_eq!(apply_conditional(result, &headers).status, 304);

        let modified = parse_http_date(&last_modified).expect("parse");
        let mut headers = Header::new();
        headers.set("If-Modified-Since", &http_date(modified.saturating_sub(10)));
        let result = serve(&config, "/").expect("index");
        assert_eq!(apply_conditional(result, &headers).status, 200);

        let mut headers = Header::new();
        headers.set("If-None-Match", "W/\"other\"");
        let result = serve(&config, "/").expect("index");
        assert_eq!(apply_conditional(result, &headers).status, 200);
        let _ = std::fs::remove_dir_all(&root);
    }
}
