//! Middleware helpers for `ghttp`, ported from
//! `ghttp_middleware_json_body.go` and `ghttp_middleware_handler_response.go`
//! (ADR-104).
//!
//! - [`middleware_json_body`] rejects non-JSON request bodies with a 400 before
//!   the route handler runs;
//! - [`middleware_handler_response`] fills an otherwise empty response with the
//!   default JSON envelope (`code`/`message`/`data`); responses that already
//!   have a body, or use a streaming content type, are left untouched.
//!
//! Divergence: GoFrame's envelope wraps a handler-returned result object; the
//! port's handlers write responses directly, so the envelope covers only the
//! empty-response case with `data: null`.

use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gcode::Code;
use gf_errors::gerror::Error;

use super::message::Response;
use super::router::Context;

/// The error text used for a non-JSON request body, like `ErrNeedJsonBody`.
pub const NEED_JSON_BODY: &str = "need json body";

/// The default JSON response envelope, like `DefaultHandlerResponse`.
#[derive(Clone, Debug, PartialEq)]
pub struct DefaultHandlerResponse {
    /// The error code.
    pub code: i64,
    /// The error message.
    pub message: String,
    /// The result data.
    pub data: Value,
}

impl DefaultHandlerResponse {
    /// The envelope as a JSON object value.
    pub fn to_value(&self) -> Value {
        let mut map = indexmap::IndexMap::new();
        map.insert("code".to_owned(), Value::Int(self.code));
        map.insert("message".to_owned(), Value::String(self.message.clone()));
        map.insert("data".to_owned(), self.data.clone());
        Value::Map(map)
    }
}

/// Rejects a non-JSON request body with a 400, like `MiddlewareJsonBody`.
pub fn middleware_json_body(context: &mut Context) -> Result<(), Error> {
    let body = &context.request.body;
    if !body.is_empty() && !gf_json::valid(body) {
        context.response = Response::new(400);
        context.response.set_text(NEED_JSON_BODY);
        context.exit_all();
    }
    Ok(())
}

/// Arms the default JSON envelope; the wrap-up runs after the handler chain,
/// like `MiddlewareHandlerResponse`.
pub fn middleware_handler_response(context: &mut Context) -> Result<(), Error> {
    context.handler_response = true;
    Ok(())
}

/// Fills the response when the middleware armed the envelope.
pub(crate) fn finish(context: &mut Context) {
    if !context.handler_response {
        return;
    }
    context.handler_response = false;
    if !context.response.body.is_empty() {
        return;
    }
    if let Some(content_type) = context.response.headers.get("Content-Type")
        && is_stream_content_type(content_type)
    {
        return;
    }
    let (code, message) = match context.response.status {
        200 => (gcode::CODE_OK.code() as i64, "OK".to_owned()),
        404 => (gcode::CODE_NOT_FOUND.code() as i64, "Not Found".to_owned()),
        403 => (
            gcode::CODE_NOT_AUTHORIZED.code() as i64,
            "Not Authorized".to_owned(),
        ),
        status if status >= 400 => (
            gcode::CODE_UNKNOWN.code() as i64,
            "Unknown Error".to_owned(),
        ),
        _ => (gcode::CODE_OK.code() as i64, "OK".to_owned()),
    };
    let envelope = DefaultHandlerResponse {
        code,
        message,
        data: Value::Null,
    };
    let mut response = Response::new(context.response.status);
    match gf_json::marshal_value(&envelope.to_value()) {
        Ok(json) => response.set_json(&json),
        Err(_) => return,
    }
    for (name, values) in context.response.headers.iter() {
        if !name.eq_ignore_ascii_case("Content-Type")
            && !name.eq_ignore_ascii_case("Content-Length")
        {
            for value in values {
                response.headers.add(name.as_str(), value.as_str());
            }
        }
    }
    context.response = response;
}

/// Reports whether `content_type` is one of GoFrame's streaming types.
fn is_stream_content_type(content_type: &str) -> bool {
    let media_type = content_type.split(';').next().unwrap_or("").trim();
    matches!(
        media_type,
        "text/event-stream" | "application/octet-stream" | "multipart/x-mixed-replace"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use super::super::message::{Header, Request};
    use super::super::router::Handler;

    fn request(method: &str, body: &[u8], headers: &[(&str, &str)]) -> Request {
        let mut request = Request {
            method: method.to_owned(),
            path: "/".to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            body: body.to_vec(),
            ..Request::default()
        };
        let mut parsed = Header::new();
        for (name, value) in headers {
            parsed.set(name, value);
        }
        request.headers = parsed;
        request
    }

    fn run(request: Request, middlewares: Vec<Handler>) -> Context {
        let mut chain = middlewares;
        chain.push(Arc::new(|context: &mut Context| {
            if context.response.body.is_empty() && context.response.status == 200 {
                context.response.set_text("handler");
            }
            Ok(())
        }));
        let mut context = Context::new(request, chain);
        context.proceed().expect("chain");
        context
    }

    #[test]
    fn invalid_json_body_is_rejected() {
        let context = run(
            request("POST", b"not json", &[]),
            vec![Arc::new(middleware_json_body)],
        );
        assert_eq!(context.response.status, 400);
        assert_eq!(context.response.text(), NEED_JSON_BODY);
    }

    #[test]
    fn valid_json_body_continues() {
        let context = run(
            request("POST", br#"{"a":1}"#, &[]),
            vec![Arc::new(middleware_json_body)],
        );
        assert_eq!(context.response.status, 200);
        assert_eq!(context.response.text(), "handler");
    }

    #[test]
    fn empty_response_gets_the_default_envelope() {
        let handler: Handler = Arc::new(|_context: &mut Context| Ok(()));
        let mut context = Context::new(
            request("GET", b"", &[]),
            vec![Arc::new(middleware_handler_response), handler],
        );
        context.proceed().expect("chain");
        assert_eq!(context.response.status, 200);
        let value: serde_json::Value =
            serde_json::from_slice(&context.response.body).expect("json");
        assert_eq!(value["code"], 0);
        assert_eq!(value["message"], "OK");
        assert!(value["data"].is_null());
        assert_eq!(
            context.response.headers.get("Content-Type"),
            Some("application/json; charset=utf-8")
        );
    }

    #[test]
    fn existing_bodies_and_streams_are_untouched() {
        let handler: Handler = Arc::new(|context: &mut Context| {
            context.response.set_text("custom");
            Ok(())
        });
        let mut context = Context::new(
            request("GET", b"", &[]),
            vec![Arc::new(middleware_handler_response), handler],
        );
        context.proceed().expect("chain");
        assert_eq!(context.response.text(), "custom");

        let handler: Handler = Arc::new(|context: &mut Context| {
            context
                .response
                .headers
                .set("Content-Type", "text/event-stream");
            Ok(())
        });
        let mut context = Context::new(
            request("GET", b"", &[]),
            vec![Arc::new(middleware_handler_response), handler],
        );
        context.proceed().expect("chain");
        assert!(context.response.body.is_empty());
    }

    #[test]
    fn non_ok_status_maps_to_envelope_code() {
        let handler: Handler = Arc::new(|context: &mut Context| {
            context.response = Response::new(404);
            Ok(())
        });
        let mut context = Context::new(
            request("GET", b"", &[]),
            vec![Arc::new(middleware_handler_response), handler],
        );
        context.proceed().expect("chain");
        let value: serde_json::Value =
            serde_json::from_slice(&context.response.body).expect("json");
        assert_eq!(value["code"], gcode::CODE_NOT_FOUND.code() as i64);
        assert_eq!(value["message"], "Not Found");
    }
}
