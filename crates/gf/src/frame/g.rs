//! Port of `frame/g` — the convenience facade.
//!
//! Go's `g` package exposes frequently used types and singleton accessors
//! (`g.Server()`, `g.Log()`, `g.View()`, ...). This module keeps the accessor
//! shape over the ported crates (ADR-075). The Go handlers that need parts of
//! the framework that are not ported (DB/Redis/i18n) are not provided.
//!
//! Divergences:
//!
//! - no `Ctx`/`Meta` aliases (no `context.Context`, no reflection metadata);
//! - `Map` is `IndexMap<String, Value>` and `Slice`/`Array` are
//!   `Vec<Value>`; the `map[any]any`-style aliases are omitted because Rust
//!   maps need concrete key types;
//! - accessors take `Option<&str>` names instead of Go's variadic `...string`.

use std::sync::Arc;

use gf_core::Value;
use gf_net::gclient::Client;
use gf_net::ghttp::server::Server as HttpServer;
use gf_net::gtcp::Server as TcpServer;
use gf_net::gudp::Server as UdpServer;
use gf_os::gcfg::{self, Config};
use gf_os::glog::{self, Logger};
use gf_os::gview::View;
use gf_valid::gvalid::Validator;
use indexmap::IndexMap;

/// The universal variable type, like `g.Var`.
pub type Var = gf_container::gvar::Var;

/// A string-keyed map, like `g.Map`.
pub type Map = IndexMap<String, Value>;

/// A list of maps, like `g.List`.
pub type List = Vec<Map>;

/// A value slice, like `g.Slice`.
pub type Slice = Vec<Value>;

/// A value array, like `g.Array`.
pub type Array = Vec<Value>;

/// A string slice, like `g.SliceStr`.
pub type SliceStr = Vec<String>;

/// An integer slice, like `g.SliceInt`.
pub type SliceInt = Vec<i64>;

/// A string array, like `g.ArrayStr`.
pub type ArrayStr = Vec<String>;

/// An integer array, like `g.ArrayInt`.
pub type ArrayInt = Vec<i64>;

/// Creates a variable, like `g.NewVar`.
pub fn new_var(value: Value) -> Var {
    Var::new(value)
}

/// Creates an HTTP client, like `g.Client`.
pub fn client() -> Client {
    Client::new()
}

/// Returns the named HTTP server, like `g.Server`.
pub fn server(name: Option<&str>) -> HttpServer {
    super::gins::server(name)
}

/// Returns the named TCP server, like `g.TCPServer`.
pub fn tcp_server(name: Option<&str>) -> TcpServer {
    gf_net::gtcp::get_server(name)
}

/// Returns the named UDP server, like `g.UDPServer`.
pub fn udp_server(name: Option<&str>) -> UdpServer {
    gf_net::gudp::get_server(name)
}

/// Returns the named logger, like `g.Log`.
pub fn log(name: Option<&str>) -> Arc<Logger> {
    glog::instance(name)
}

/// Returns the named configuration, like `g.Cfg`.
pub fn cfg(name: Option<&str>) -> Option<Arc<Config>> {
    gcfg::instance(name)
}

/// Returns the named i18n manager, like `g.I18n`.
pub fn i18n(name: Option<&str>) -> Arc<gf_i18n::gi18n::Manager> {
    super::gins::i18n(name)
}

/// Creates a validator, like `g.Validator`.
pub fn validator() -> Validator {
    Validator::new()
}

/// Returns the named template view, like `g.View`; the default view when no
/// name is given.
pub fn view(name: Option<&str>) -> View {
    super::gins::view(name)
}

/// Enables or disables the internal debug logging, like `g.SetDebug`.
pub fn set_debug(enabled: bool) {
    gf_internal::utils::set_debug_enabled(enabled);
}

#[cfg(test)]
mod tests {
    use super::*;
    use gf_os::gfile;

    #[test]
    fn type_aliases_and_new_var() {
        let mut map = Map::new();
        map.insert("name".to_owned(), Value::String("gf".to_owned()));
        let list: List = vec![map.clone()];
        assert_eq!(list.len(), 1);
        let slice: Slice = vec![Value::Int(1)];
        assert_eq!(slice.len(), 1);
        let array: Array = vec![Value::Int(2)];
        assert_eq!(array.len(), 1);
        assert_eq!(SliceStr::from(["a".to_owned()]), vec!["a".to_owned()]);
        assert_eq!(SliceInt::from([1i64]), vec![1]);
        assert_eq!(ArrayStr::from(["b".to_owned()]), vec!["b".to_owned()]);
        assert_eq!(ArrayInt::from([2i64]), vec![2]);
        assert_eq!(new_var(Value::Int(7)).int64(), 7);
    }

    #[test]
    fn named_servers_are_singletons() {
        let http_server = server(Some("gf-frame-g-http"));
        http_server.set_address("127.0.0.1:18889");
        let again = server(Some("gf-frame-g-http"));
        assert_eq!(again.address(), "127.0.0.1:18889");
        // A different name is a different instance.
        assert_eq!(server(Some("gf-frame-g-http-2")).address(), "");

        let tcp = tcp_server(Some("gf-frame-g-tcp"));
        tcp.set_address("127.0.0.1:18890");
        assert_eq!(
            tcp_server(Some("gf-frame-g-tcp")).address(),
            "127.0.0.1:18890"
        );

        let udp = udp_server(Some("gf-frame-g-udp"));
        udp.set_address("127.0.0.1:18891");
        assert_eq!(
            udp_server(Some("gf-frame-g-udp")).listened_address(),
            "127.0.0.1:18891"
        );
    }

    #[test]
    fn named_views_and_loggers() {
        let directory =
            std::env::temp_dir().join(format!("gf-frame-g-view-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        gfile::mkdir(&directory.to_string_lossy()).expect("view dir");
        let named_view = view(Some("gf-frame-g-view"));
        named_view
            .set_path(&directory.to_string_lossy())
            .expect("path");
        let again = view(Some("gf-frame-g-view"));
        assert_eq!(again.paths().len(), 1);
        assert!(std::path::Path::new(&again.paths()[0]).is_dir());
        let _ = std::fs::remove_dir_all(&directory);

        let logger = log(Some("gf-frame-g-log"));
        let same = log(Some("gf-frame-g-log"));
        assert!(Arc::ptr_eq(&logger, &same));
    }

    #[test]
    fn client_config_and_validator() {
        let client = client();
        assert_eq!(
            client.set_prefix("http://127.0.0.1:1").prefix(),
            "http://127.0.0.1:1"
        );
        assert!(cfg(Some("gf-frame-g-missing")).is_some());
        let validator = validator();
        assert!(
            validator
                .data(Value::String("john".to_owned()))
                .rules("required")
                .run()
                .is_ok()
        );
    }
}
