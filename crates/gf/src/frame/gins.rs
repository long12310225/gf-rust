//! Port of `frame/gins` — the instance container behind [`super::g`].
//!
//! Go's `gins` owns the named component singletons used by the facade
//! (`g.Server()`, `g.View()`, `g.Log()`, ...). This port delegates to the
//! registries that already exist in the underlying crates and adds override
//! setters where the Go container allows replacing an instance (ADR-081):
//!
//! - `server(name)`, `set_server(name, server)` — `ghttp` named registry;
//! - `log(name)` — `glog` instance registry (no setter, like the crate);
//! - `config(name)` — `gcfg` instance registry;
//! - `i18n(name)`, `set_i18n(name, manager)` — `gi18n` named registry;
//! - `view(name)`, `set_view(name, view)` — a container-level view registry.
//!
//! Not provided: `database`, `redis` and `resource` accessors (the database
//! needs a configured driver instance, Redis needs a configured client and
//! the `gres` resource lane is not wired into this container).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use gf_i18n::gi18n;
use gf_net::ghttp::server::{Server as HttpServer, get_server, set_server as register_server};
use gf_os::gcfg::{self, Config};
use gf_os::glog::{self, Logger};
use gf_os::gview::View;

/// The component name prefixes used by the Go container.
pub const FRAME_CORE_COMPONENT_NAME_VIEWER: &str = "gf.core.component.viewer";
/// The HTTP server component name.
pub const FRAME_CORE_COMPONENT_NAME_SERVER: &str = "gf.core.component.server";
/// The logger component name.
pub const FRAME_CORE_COMPONENT_NAME_LOGGER: &str = "gf.core.component.logger";
/// The config component name.
pub const FRAME_CORE_COMPONENT_NAME_CONFIG: &str = "gf.core.component.config";
/// The i18n component name.
pub const FRAME_CORE_COMPONENT_NAME_I18N: &str = "gf.core.component.i18n";

/// Returns the named HTTP server, creating it on first use, like
/// `gins.Server`.
pub fn server(name: Option<&str>) -> HttpServer {
    get_server(name)
}

/// Registers a server under `name`, like replacing a `gins` instance.
pub fn set_server(name: &str, server: HttpServer) {
    register_server(name, server);
}

/// Returns the named logger, like `gins.Log`.
pub fn log(name: Option<&str>) -> Arc<Logger> {
    glog::instance(name)
}

/// Returns the named configuration, like `gins.Config`.
pub fn config(name: Option<&str>) -> Option<Arc<Config>> {
    gcfg::instance(name)
}

/// Returns the named i18n manager, like `gins.I18n`.
pub fn i18n(name: Option<&str>) -> Arc<gi18n::Manager> {
    gi18n::instance(name)
}

/// Registers an i18n manager under `name`.
pub fn set_i18n(name: &str, manager: Arc<gi18n::Manager>) {
    gi18n::set_instance(name, manager);
}

/// The container-level view registry.
fn views() -> &'static Mutex<HashMap<String, View>> {
    static VIEWS: OnceLock<Mutex<HashMap<String, View>>> = OnceLock::new();
    VIEWS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Returns the named view, like `gins.View`; the default view when unnamed.
pub fn view(name: Option<&str>) -> View {
    match name.filter(|name| !name.is_empty()) {
        None => View::instance().clone(),
        Some(name) => views()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .entry(name.to_owned())
            .or_default()
            .clone(),
    }
}

/// Registers a view under `name`.
pub fn set_view(name: &str, view: View) {
    views()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(name.to_owned(), view);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn servers_are_named_singletons() {
        let http_server = server(Some("gf-gins-http"));
        http_server.set_address("127.0.0.1:18991");
        assert_eq!(server(Some("gf-gins-http")).address(), "127.0.0.1:18991");

        let replacement = HttpServer::with_address("127.0.0.1:18992");
        set_server("gf-gins-http", replacement);
        assert_eq!(server(Some("gf-gins-http")).address(), "127.0.0.1:18992");
        assert_eq!(server(Some("gf-gins-http-other")).address(), "");
    }

    #[test]
    fn views_and_i18n_are_named() {
        let directory = std::env::temp_dir().join(format!("gf-gins-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).expect("dir");
        let named_view = View::new();
        named_view
            .set_path(&directory.to_string_lossy())
            .expect("path");
        set_view("gf-gins-view", named_view.clone());
        assert!(
            view(Some("gf-gins-view"))
                .paths()
                .iter()
                .any(|path| path.contains("gf-gins-view-"))
        );
        let _ = std::fs::remove_dir_all(&directory);

        let manager = Arc::new(gi18n::Manager::new(gi18n::Options::default()));
        manager.set_language("zh-CN");
        set_i18n("gf-gins-i18n", Arc::clone(&manager));
        assert!(Arc::ptr_eq(&manager, &i18n(Some("gf-gins-i18n"))));
        assert_eq!(i18n(Some("gf-gins-i18n")).language(), "zh-CN");
    }

    #[test]
    fn log_and_config_instances() {
        let logger = log(Some("gf-gins-log"));
        assert!(Arc::ptr_eq(&logger, &log(Some("gf-gins-log"))));
        assert!(config(None).is_some());
        assert_eq!(FRAME_CORE_COMPONENT_NAME_SERVER, "gf.core.component.server");
        assert_eq!(FRAME_CORE_COMPONENT_NAME_LOGGER, "gf.core.component.logger");
        assert_eq!(FRAME_CORE_COMPONENT_NAME_CONFIG, "gf.core.component.config");
        assert_eq!(FRAME_CORE_COMPONENT_NAME_I18N, "gf.core.component.i18n");
        assert_eq!(FRAME_CORE_COMPONENT_NAME_VIEWER, "gf.core.component.viewer");
    }
}
