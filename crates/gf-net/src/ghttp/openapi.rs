//! OpenAPI/Swagger serving for `ghttp`, ported from GoFrame's
//! `net/goai` + `ghttp` OpenAPI wiring.
//!
//! The Go server builds the document from handler metadata and serves a
//! Swagger UI (embedded assets) at `/swagger` plus the document at `/api.json`.
//! This port takes a ready [`goai::OpenApiV3`] document and serves it, with a
//! Swagger UI page that loads the UI bundle from a CDN (ADR-080):
//!
//! - [`OpenApiConfig`] holds the document and the two paths;
//! - [`swagger_ui_html`] renders the UI page pointing at the document path;
//! - [`crate::ghttp::server::Server::set_openapi`] wires it into request
//!   handling.

use gf_json::marshal_string;

use super::message::Response;
use crate::goai::OpenApiV3;

/// The default path of the OpenAPI document.
pub const DEFAULT_OPENAPI_PATH: &str = "/api.json";

/// The default path of the Swagger UI.
pub const DEFAULT_SWAGGER_PATH: &str = "/swagger";

/// The OpenAPI serving configuration.
#[derive(Clone, Debug)]
pub struct OpenApiConfig {
    /// The document, with a version filled in when empty.
    pub document: OpenApiV3,
    /// The document path.
    pub openapi_path: String,
    /// The Swagger UI path.
    pub swagger_path: String,
}

impl OpenApiConfig {
    /// Creates a configuration for `document` with the default paths.
    pub fn new(document: OpenApiV3) -> OpenApiConfig {
        let mut document = document;
        if document.openapi.is_empty() {
            document.openapi = "3.0.0".to_owned();
        }
        OpenApiConfig {
            document,
            openapi_path: DEFAULT_OPENAPI_PATH.to_owned(),
            swagger_path: DEFAULT_SWAGGER_PATH.to_owned(),
        }
    }

    /// Overrides the document path.
    pub fn with_openapi_path(mut self, path: &str) -> OpenApiConfig {
        self.openapi_path = path.to_owned();
        self
    }

    /// Overrides the Swagger UI path.
    pub fn with_swagger_path(mut self, path: &str) -> OpenApiConfig {
        self.swagger_path = path.to_owned();
        self
    }

    /// Renders the document as JSON, like the `/api.json` handler.
    pub fn document_json(&self) -> Result<String, gf_errors::gerror::Error> {
        marshal_string(&self.document)
    }

    /// Renders the Swagger UI page.
    pub fn ui_html(&self) -> String {
        swagger_ui_html(&self.openapi_path)
    }

    /// Serves one of the two OpenAPI paths, when `path` matches and the
    /// method is `GET`.
    pub fn serve(&self, method: &str, path: &str) -> Option<Response> {
        if !method.eq_ignore_ascii_case("GET") {
            return None;
        }
        if path == self.openapi_path {
            let mut response = Response::new(200);
            response.headers.set("Content-Type", "application/json");
            match self.document_json() {
                Ok(json) => response.set_text(&json),
                Err(error) => {
                    let mut response = Response::new(500);
                    response.set_text(&error.to_string());
                    return Some(response);
                }
            }
            return Some(response);
        }
        if path == self.swagger_path {
            let mut response = Response::new(200);
            response.set_text(&self.ui_html());
            response
                .headers
                .set("Content-Type", "text/html; charset=utf-8");
            return Some(response);
        }
        None
    }
}

/// Renders a minimal Swagger UI page that loads the document at `spec_path`.
pub fn swagger_ui_html(spec_path: &str) -> String {
    format!(
        r##"<!DOCTYPE html>
<html>
<head>
<title>Swagger UI</title>
<link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
<div id="swagger-ui"></div>
<script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
<script>
window.ui = SwaggerUIBundle({{url: "{spec_path}", dom_id: "#swagger-ui"}});
</script>
</body>
</html>"##
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::goai::Info;

    fn document() -> OpenApiV3 {
        OpenApiV3 {
            info: Info {
                title: "Pet Store".to_owned(),
                version: "1.0.0".to_owned(),
                ..Info::default()
            },
            ..OpenApiV3::default()
        }
    }

    #[test]
    fn document_and_ui_paths() {
        let config = OpenApiConfig::new(document());
        let response = config.serve("GET", DEFAULT_OPENAPI_PATH).expect("spec");
        assert_eq!(response.status, 200);
        let body = response.text();
        assert!(body.contains("\"openapi\":\"3.0.0\""), "{body}");
        assert!(body.contains("\"Pet Store\""), "{body}");

        let response = config.serve("GET", DEFAULT_SWAGGER_PATH).expect("ui");
        assert_eq!(response.status, 200);
        let body = response.text();
        assert!(body.contains("swagger-ui"), "{body}");
        assert!(body.contains("\"/api.json\""), "{body}");
        assert!(
            response
                .headers
                .get("Content-Type")
                .expect("content type")
                .starts_with("text/html")
        );
    }

    #[test]
    fn custom_paths_and_methods() {
        let config = OpenApiConfig::new(document())
            .with_openapi_path("/openapi.json")
            .with_swagger_path("/docs");
        assert!(config.serve("GET", "/api.json").is_none());
        assert!(config.serve("GET", "/openapi.json").is_some());
        let response = config.serve("GET", "/docs").expect("docs");
        assert!(response.text().contains("\"/openapi.json\""));
        assert!(config.serve("POST", "/openapi.json").is_none());
        assert!(config.serve("GET", "/missing").is_none());
    }

    #[test]
    fn ui_html_contains_spec_reference() {
        let html = swagger_ui_html("/my-spec.json");
        assert!(html.contains("SwaggerUIBundle"));
        assert!(html.contains("/my-spec.json"));
    }
}
