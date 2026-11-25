//! The admin endpoints, like GoFrame's `EnableAdmin` (ADR-185).
//!
//! GoFrame binds an object (`Index`, `Restart`, `Shutdown`) at
//! `/debug/admin`. The port binds the same three routes explicitly:
//!
//! - `GET {base}` renders a small HTML page with the pid, executable path and
//!   links;
//! - `GET {base}/shutdown` closes the accept loop and answers
//!   `server shutdown`;
//! - `GET {base}/restart` answers 501: replacing the running process is
//!   platform-specific and not ported.
//!
//! Not ported: object-method binding, process restart/`RestartAllServer`, the
//! pprof endpoints, and the 1-second delayed shutdown.

use std::sync::Arc;

use super::router::{Context, Handler};
use super::server::Server;

/// The default admin path, like GoFrame's `/debug/admin`.
pub const DEFAULT_ADMIN_PATH: &str = "/debug/admin";

impl Server {
    /// Enables the admin endpoints at `pattern` (default [`DEFAULT_ADMIN_PATH`]),
    /// like `EnableAdmin`.
    pub fn enable_admin(&self, pattern: Option<&str>) {
        let base = pattern
            .unwrap_or(DEFAULT_ADMIN_PATH)
            .trim_end_matches('/')
            .to_owned();
        if base.is_empty() {
            return;
        }

        let index_base = base.clone();
        let index: Handler = Arc::new(move |context: &mut Context| {
            context.response.set_text(&admin_page(&index_base));
            Ok(())
        });
        self.bind_handler(&format!("GET:{base}"), index);

        self.bind_handler(
            &format!("GET:{base}/restart"),
            Arc::new(|context: &mut Context| {
                context.response.status = 501;
                context
                    .response
                    .set_text("server restart is not supported in this port");
                Ok(())
            }),
        );

        let server = self.clone();
        self.bind_handler(
            &format!("GET:{base}/shutdown"),
            Arc::new(move |context: &mut Context| {
                context.response.set_text("server shutdown");
                let _ = server.close();
                Ok(())
            }),
        );
    }
}

/// Renders the admin index page, like `utilAdmin.Index`.
fn admin_page(base: &str) -> String {
    let pid = std::process::id();
    let path = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    format!(
        "<html>
<head><title>GoFrame Web Server Admin</title></head>
<body>
<p>Pid: {pid}</p>
<p>File Path: {path}</p>
<p><a href=\"{base}/restart\">Restart</a> (not supported in this port)</p>
<p><a href=\"{base}/shutdown\">Shutdown</a> graceful shutdown the server</p>
</body>
</html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ghttp::message::Request;

    fn request(path: &str) -> Request {
        Request {
            method: "GET".to_owned(),
            path: path.to_owned(),
            protocol: "HTTP/1.1".to_owned(),
            ..Request::default()
        }
    }

    #[test]
    fn admin_index_restart_and_shutdown() {
        let server = Server::new();
        server.enable_admin(None);

        let response = server.serve(request("/debug/admin"));
        assert_eq!(response.status, 200);
        let body = response.text();
        assert!(body.contains("GoFrame Web Server Admin"), "{body}");
        assert!(body.contains(&std::process::id().to_string()), "{body}");
        assert!(body.contains("/debug/admin/shutdown"), "{body}");

        let response = server.serve(request("/debug/admin/restart"));
        assert_eq!(response.status, 501);
        assert!(response.text().contains("not supported"));

        assert!(!server.is_closed());
        let response = server.serve(request("/debug/admin/shutdown"));
        assert_eq!(response.text(), "server shutdown");
        assert!(server.is_closed(), "shutdown closes the accept loop");
    }

    #[test]
    fn admin_custom_pattern() {
        let server = Server::new();
        server.enable_admin(Some("/admin/"));
        assert_eq!(server.serve(request("/admin")).status, 200);
        assert_eq!(server.serve(request("/admin/restart")).status, 501);
        assert!(!server.is_closed());
        assert_eq!(
            server.serve(request("/admin/shutdown")).text(),
            "server shutdown"
        );
        assert!(server.is_closed());
    }
}
