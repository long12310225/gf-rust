//! Gzip response compression for `ghttp`, ported from
//! `ghttp_middleware_gzip.go` (ADR-103).
//!
//! GoFrame's middleware paints the `Accept-Encoding` check and compresses the
//! buffered response after the next handlers ran. The port's handler chain is
//! flat, so [`middleware_gzip`] only arms a flag on the [`Context`]; the
//! compression itself runs once [`Context::proceed`] finishes:
//!
//! - no compression when the client does not accept gzip, the response is
//!   already encoded, or the body is smaller than [`GZIP_MIN_LENGTH`] (1 KiB);
//! - otherwise the body is gzip-compressed and `Content-Encoding: gzip` set
//!   (GoFrame's `ClearBuffer` + header rewrite).

use gf_errors::gerror::Error;

use super::router::Context;

/// The minimum body size that is compressed (1 KiB), like GoFrame.
pub const GZIP_MIN_LENGTH: usize = 1024;

/// The gzip middleware, like `MiddlewareGzip`; bind it with
/// `Server::bind_middleware`.
pub fn middleware_gzip(context: &mut Context) -> Result<(), Error> {
    let accepts_gzip = context
        .request
        .headers
        .get("Accept-Encoding")
        .map(|value| value.contains("gzip"))
        .unwrap_or(false);
    if accepts_gzip {
        context.gzip = true;
    }
    Ok(())
}

/// Compresses the buffered response when the middleware armed it.
pub(crate) fn finish(context: &mut Context) {
    if !context.gzip {
        return;
    }
    context.gzip = false;
    if context.response.headers.get("Content-Encoding").is_some() {
        return;
    }
    if context.response.body.len() < GZIP_MIN_LENGTH {
        return;
    }
    let Ok(compressed) = gf_encoding::gcompress::gzip(&context.response.body, Some(9)) else {
        return;
    };
    context.response.set_body(&compressed);
    context.response.headers.set("Content-Encoding", "gzip");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::ghttp::message::{Header, Request, Response};
    use crate::ghttp::router::Handler;

    fn request(headers: &[(&str, &str)]) -> Request {
        let mut request = Request {
            method: "GET".to_owned(),
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

    fn run(body: &[u8], headers: &[(&str, &str)]) -> Context {
        let body = body.to_vec();
        let handler: Handler = Arc::new(move |context: &mut Context| {
            context.response = Response::new(200);
            context.response.set_body(&body);
            Ok(())
        });
        let mut context = Context::new(request(headers), vec![Arc::new(middleware_gzip), handler]);
        context.proceed().expect("chain");
        context
    }

    #[test]
    fn compresses_large_bodies_when_accepted() {
        let body = vec![b'a'; GZIP_MIN_LENGTH + 1];
        let context = run(&body, &[("Accept-Encoding", "gzip, deflate")]);
        assert_eq!(
            context.response.headers.get("Content-Encoding"),
            Some("gzip")
        );
        assert!(context.response.body.len() < body.len());
        let decompressed = gf_encoding::gcompress::un_gzip(&context.response.body).expect("ungzip");
        assert_eq!(decompressed, body);
        assert_eq!(
            context.response.headers.get("Content-Length"),
            Some(context.response.body.len().to_string().as_str())
        );
    }

    #[test]
    fn skips_small_bodies_and_other_clients() {
        let small = vec![b'a'; 10];
        let context = run(&small, &[("Accept-Encoding", "gzip")]);
        assert_eq!(context.response.headers.get("Content-Encoding"), None);
        assert_eq!(context.response.body, small);

        let body = vec![b'a'; GZIP_MIN_LENGTH + 1];
        let context = run(&body, &[]);
        assert_eq!(context.response.headers.get("Content-Encoding"), None);
        assert_eq!(context.response.body, body);
    }

    #[test]
    fn skips_already_encoded_responses() {
        let body = vec![b'a'; GZIP_MIN_LENGTH + 1];
        let handler: Handler = Arc::new(move |context: &mut Context| {
            context.response = Response::new(200);
            context.response.set_body(&body);
            context.response.headers.set("Content-Encoding", "br");
            Ok(())
        });
        let mut context = Context::new(
            request(&[("Accept-Encoding", "gzip")]),
            vec![Arc::new(middleware_gzip), handler],
        );
        context.proceed().expect("chain");
        assert_eq!(context.response.headers.get("Content-Encoding"), Some("br"));
    }
}
