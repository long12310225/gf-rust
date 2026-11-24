//! Port of `internal/consts`.

/// Configuration node name for the database.
pub const CONFIG_NODE_NAME_DATABASE: &str = "database";
/// Configuration node name for the logger.
pub const CONFIG_NODE_NAME_LOGGER: &str = "logger";
/// Configuration node name for redis.
pub const CONFIG_NODE_NAME_REDIS: &str = "redis";
/// Configuration node name for the viewer.
pub const CONFIG_NODE_NAME_VIEWER: &str = "viewer";
/// General server configuration node name.
pub const CONFIG_NODE_NAME_SERVER: &str = "server";
/// v2 server configuration node name.
pub const CONFIG_NODE_NAME_SERVER_SECONDARY: &str = "httpserver";
/// Stack filtering key for every GoFrame module path.
pub const STACK_FILTER_KEY_FOR_GO_FRAME: &str = "github.com/gogf/gf/";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_names_match_go() {
        assert_eq!(CONFIG_NODE_NAME_DATABASE, "database");
        assert_eq!(CONFIG_NODE_NAME_LOGGER, "logger");
        assert_eq!(CONFIG_NODE_NAME_REDIS, "redis");
        assert_eq!(CONFIG_NODE_NAME_VIEWER, "viewer");
        assert_eq!(CONFIG_NODE_NAME_SERVER, "server");
        assert_eq!(CONFIG_NODE_NAME_SERVER_SECONDARY, "httpserver");
        assert_eq!(STACK_FILTER_KEY_FOR_GO_FRAME, "github.com/gogf/gf/");
    }
}
