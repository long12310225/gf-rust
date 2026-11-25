//! Port of `net/gsvc` — service registry and discovery definitions.
//!
//! The port keeps the interface layout (registry/discovery/watcher, service,
//! endpoint, metadata) and the `LocalService` implementation. There is no
//! context (ADR-034), so the trait methods take no `context.Context`.
//!
//! Divergences, recorded in ADR-062:
//!
//! - no `context.Context`;
//! - `NewEndpoint` returns a `Result` instead of panicking (the panic variant
//!   is [`Endpoint::must_new`]);
//! - `NewEndpoints` returns a `Result` too, because each address is validated;
//! - the default registry is an `Arc<dyn Registry>` in a `RwLock`; the registry
//!   timeout wrapper is not ported.

use std::sync::{Arc, OnceLock, RwLock};

use gf_container::gvar::Var;
use gf_core::Value;
use gf_errors::gcode;
use gf_errors::gerror::{self, Error};
use indexmap::IndexMap;

/// The service schema, like Go's `Schema`.
pub const SCHEMA: &str = "service";
/// The default service head, like Go's `DefaultHead`.
pub const DEFAULT_HEAD: &str = "service";
/// The default deployment, like Go's `DefaultDeployment`.
pub const DEFAULT_DEPLOYMENT: &str = "default";
/// The default namespace, like Go's `DefaultNamespace`.
pub const DEFAULT_NAMESPACE: &str = "default";
/// The default version, like Go's `DefaultVersion`.
pub const DEFAULT_VERSION: &str = "latest";
/// The environment prefix for the service head.
pub const ENV_PREFIX: &str = "GF_GSVC_PREFIX";
/// The environment prefix for the deployment.
pub const ENV_DEPLOYMENT: &str = "GF_GSVC_DEPLOYMENT";
/// The environment prefix for the namespace.
pub const ENV_NAMESPACE: &str = "GF_GSVC_NAMESPACE";
/// The environment prefix for the service name.
pub const ENV_NAME: &str = "GF_GSVC_Name";
/// The environment prefix for the version.
pub const ENV_VERSION: &str = "GF_GSVC_VERSION";
/// The metadata key for the protocol.
pub const MD_PROTOCOL: &str = "protocol";
/// The metadata key for insecure.
pub const MD_INSECURE: &str = "insecure";
/// The metadata key for weight.
pub const MD_WEIGHT: &str = "weight";
/// The default protocol.
pub const DEFAULT_PROTOCOL: &str = "http";
/// The default separator.
pub const DEFAULT_SEPARATOR: &str = "/";
/// The host/port delimiter.
pub const ENDPOINT_HOST_PORT_DELIMITER: &str = ":";
/// The endpoints delimiter.
pub const ENDPOINTS_DELIMITER: &str = ",";

/// One service endpoint, like Go's `Endpoint`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    host: String,
    port: i32,
}

impl Endpoint {
    /// Parses an address like `192.168.1.100:80`, like `NewEndpoint`.
    pub fn new(address: &str) -> Result<Endpoint, Error> {
        let parts: Vec<&str> = address
            .split(ENDPOINT_HOST_PORT_DELIMITER)
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect();
        if parts.len() != 2 {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!(
                    "invalid address \"{address}\" for creating endpoint, endpoint address is like \"ip:port\""
                ),
                Vec::new(),
            ));
        }
        let port = parts[1].parse::<i32>().map_err(|_| {
            gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid port in endpoint address \"{address}\""),
                Vec::new(),
            )
        })?;
        Ok(Endpoint {
            host: parts[0].to_owned(),
            port,
        })
    }

    /// Parses an address, panicking on an invalid one, like `NewEndpoint`.
    pub fn must_new(address: &str) -> Endpoint {
        Endpoint::new(address).unwrap_or_else(|error| panic!("{error}"))
    }

    /// The host.
    pub fn host(&self) -> &str {
        &self.host
    }

    /// The port.
    pub fn port(&self) -> i32 {
        self.port
    }
}

impl std::fmt::Display for Endpoint {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}:{}", self.host, self.port)
    }
}

/// A list of endpoints, like Go's `Endpoints`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Endpoints(Vec<Endpoint>);

impl Endpoints {
    /// Parses a comma-separated address list, like `NewEndpoints`.
    pub fn new(addresses: &str) -> Result<Endpoints, Error> {
        let mut endpoints = Vec::new();
        for address in addresses
            .split(ENDPOINTS_DELIMITER)
            .map(str::trim)
            .filter(|address| !address.is_empty())
        {
            endpoints.push(Endpoint::new(address)?);
        }
        Ok(Endpoints(endpoints))
    }

    /// Returns the endpoints.
    pub fn as_slice(&self) -> &[Endpoint] {
        &self.0
    }

    /// The number of endpoints.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Reports whether there are no endpoints.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for Endpoints {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            &self
                .0
                .iter()
                .map(Endpoint::to_string)
                .collect::<Vec<String>>()
                .join(ENDPOINTS_DELIMITER),
        )
    }
}

/// Service metadata, like Go's `Metadata`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Metadata(IndexMap<String, Value>);

impl Metadata {
    /// Creates empty metadata.
    pub fn new() -> Metadata {
        Metadata::default()
    }

    /// Sets one key, like `Set`.
    pub fn set(&mut self, key: &str, value: Value) {
        self.0.insert(key.to_owned(), value);
    }

    /// Sets several keys, like `Sets`.
    pub fn sets(&mut self, values: IndexMap<String, Value>) {
        for (key, value) in values {
            self.0.insert(key, value);
        }
    }

    /// Returns one value as a `Var`, like `Get`.
    pub fn get(&self, key: &str) -> Option<Var> {
        self.0.get(key).cloned().map(Var::new)
    }

    /// Reports whether the metadata is empty, like `IsEmpty`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Returns the values.
    pub fn values(&self) -> &IndexMap<String, Value> {
        &self.0
    }
}

/// The service definition, like Go's `Service`.
pub trait Service: Send + Sync {
    /// The service name.
    fn get_name(&self) -> &str;
    /// The service version.
    fn get_version(&self) -> &str;
    /// The unique key.
    fn get_key(&self) -> String;
    /// The value (metadata as JSON).
    fn get_value(&self) -> String;
    /// The key prefix.
    fn get_prefix(&self) -> String;
    /// The metadata.
    fn get_metadata(&self) -> &Metadata;
    /// The endpoints.
    fn get_endpoints(&self) -> &Endpoints;
}

/// The default service implementation, like Go's `LocalService`.
#[derive(Clone, Debug, Default)]
pub struct LocalService {
    /// Custom head string in the service key.
    pub head: String,
    /// Deployment name.
    pub deployment: String,
    /// Namespace.
    pub namespace: String,
    /// Service name.
    pub name: String,
    /// Service version.
    pub version: String,
    /// Service endpoints.
    pub endpoints: Endpoints,
    /// Custom metadata.
    pub metadata: Metadata,
}

impl LocalService {
    /// Creates a service by name and fills the defaults, like
    /// `NewServiceWithName`.
    pub fn new_service_with_name(name: &str) -> LocalService {
        let mut service = LocalService {
            name: name.to_owned(),
            ..LocalService::default()
        };
        service.auto_fill_default_attributes();
        service
    }

    /// Creates a service from a key and a JSON value, like `NewServiceWithKV`.
    pub fn new_service_with_kv(key: &str, value: &str) -> Result<LocalService, Error> {
        let trimmed = key.trim_matches('/');
        let parts: Vec<&str> = trimmed.split(DEFAULT_SEPARATOR).collect();
        if parts.len() < 6 {
            return Err(gerror::new_codef(
                &gcode::CODE_INVALID_PARAMETER,
                format!("invalid service key \"{key}\""),
                Vec::new(),
            ));
        }
        let mut service = LocalService {
            head: parts[0].to_owned(),
            deployment: parts[1].to_owned(),
            namespace: parts[2].to_owned(),
            name: parts[3].to_owned(),
            version: parts[4].to_owned(),
            endpoints: Endpoints::new(parts[5])?,
            metadata: Metadata::new(),
        };
        service.auto_fill_default_attributes();
        if !value.is_empty() {
            let parsed = gf_json::unmarshal_value(value.as_bytes())?;
            if let Value::Map(map) = parsed {
                service.metadata.sets(map);
            }
        }
        Ok(service)
    }

    /// Fills the empty attributes from the environment, like
    /// `autoFillDefaultAttributes`.
    fn auto_fill_default_attributes(&mut self) {
        if self.head.is_empty() {
            self.head = env_or(ENV_PREFIX, DEFAULT_HEAD);
        }
        if self.deployment.is_empty() {
            self.deployment = env_or(ENV_DEPLOYMENT, DEFAULT_DEPLOYMENT);
        }
        if self.namespace.is_empty() {
            self.namespace = env_or(ENV_NAMESPACE, DEFAULT_NAMESPACE);
        }
        if self.name.is_empty() {
            self.name = std::env::var(ENV_NAME).unwrap_or_default();
        }
        if self.version.is_empty() {
            self.version = env_or(ENV_VERSION, DEFAULT_VERSION);
        }
    }
}

/// Reads an environment variable with a default.
fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| default.to_owned())
}

impl Service for LocalService {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn get_version(&self) -> &str {
        &self.version
    }

    fn get_key(&self) -> String {
        let mut value = self.get_prefix();
        value.push_str(DEFAULT_SEPARATOR);
        value.push_str(&self.endpoints.to_string());
        value
    }

    fn get_value(&self) -> String {
        match gf_json::marshal_value(&Value::Map(self.metadata.values().clone())) {
            Ok(value) => value,
            Err(_) => "{}".to_owned(),
        }
    }

    fn get_prefix(&self) -> String {
        format!(
            "{sep}{head}{sep}{deployment}{sep}{namespace}{sep}{name}{sep}{version}",
            sep = DEFAULT_SEPARATOR,
            head = self.head,
            deployment = self.deployment,
            namespace = self.namespace,
            name = self.name,
            version = self.version,
        )
    }

    fn get_metadata(&self) -> &Metadata {
        &self.metadata
    }

    fn get_endpoints(&self) -> &Endpoints {
        &self.endpoints
    }
}

/// The search input, like Go's `SearchInput`.
#[derive(Clone, Debug, Default)]
pub struct SearchInput {
    /// Search by key prefix.
    pub prefix: String,
    /// Search by service name.
    pub name: String,
    /// Search by service version.
    pub version: String,
    /// Filter by metadata.
    pub metadata: Metadata,
}

/// The registrar interface, like Go's `Registrar`.
pub trait Registrar: Send + Sync {
    /// Registers a service.
    fn register(&self, service: Arc<dyn Service>) -> Result<Arc<dyn Service>, Error>;
    /// Removes a service.
    fn deregister(&self, service: &Arc<dyn Service>) -> Result<(), Error>;
}

/// The watcher interface, like Go's `Watcher`.
pub trait Watcher: Send + Sync {
    /// Blocks until the watched services change.
    fn proceed(&self) -> Result<Vec<Arc<dyn Service>>, Error>;
    /// Closes the watcher.
    fn close(&self) -> Result<(), Error>;
}

/// The discovery interface, like Go's `Discovery`.
pub trait Discovery: Send + Sync {
    /// Searches services.
    fn search(&self, input: SearchInput) -> Result<Vec<Arc<dyn Service>>, Error>;
    /// Watches a key prefix.
    fn watch(&self, key: &str) -> Result<Box<dyn Watcher>, Error>;
}

/// The registry interface, like Go's `Registry`.
pub trait Registry: Registrar + Discovery {}

/// The global default registry holder.
fn registry_slot() -> &'static RwLock<Option<Arc<dyn Registry>>> {
    static REGISTRY: OnceLock<RwLock<Option<Arc<dyn Registry>>>> = OnceLock::new();
    REGISTRY.get_or_init(|| RwLock::new(None))
}

/// Sets the default registry, like `SetRegistry`.
pub fn set_registry(registry: Arc<dyn Registry>) {
    *registry_slot()
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(registry);
}

/// Returns the default registry, like `GetRegistry`.
pub fn get_registry() -> Option<Arc<dyn Registry>> {
    registry_slot()
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone()
}

/// Registers a service on the default registry, like `Register`.
pub fn register(service: Arc<dyn Service>) -> Result<Arc<dyn Service>, Error> {
    match get_registry() {
        Some(registry) => registry.register(service),
        None => Err(gerror::new_code(
            &gcode::CODE_NOT_IMPLEMENTED,
            &["no Registry is registered"],
        )),
    }
}

/// Deregisters a service from the default registry, like `Deregister`.
pub fn deregister(service: &Arc<dyn Service>) -> Result<(), Error> {
    match get_registry() {
        Some(registry) => registry.deregister(service),
        None => Err(gerror::new_code(
            &gcode::CODE_NOT_IMPLEMENTED,
            &["no Registry is registered"],
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_parsing_and_display() {
        let endpoint = Endpoint::new("192.168.1.100:80").expect("endpoint");
        assert_eq!(endpoint.host(), "192.168.1.100");
        assert_eq!(endpoint.port(), 80);
        assert_eq!(endpoint.to_string(), "192.168.1.100:80");
        assert!(Endpoint::new("bad-address").is_err());
        assert!(Endpoint::new("host:not-a-port").is_err());
        assert_eq!(
            Endpoints::new("192.168.1.100:80, 192.168.1.101:81")
                .expect("endpoints")
                .to_string(),
            "192.168.1.100:80,192.168.1.101:81"
        );
    }

    #[test]
    fn local_service_key_prefix_and_value() {
        let mut service = LocalService::new_service_with_name("hello");
        service.endpoints = Endpoints::new("127.0.0.1:8000").expect("endpoints");
        service.metadata.set("weight", Value::Int(10));
        assert_eq!(service.get_name(), "hello");
        assert_eq!(service.get_version(), DEFAULT_VERSION);
        assert_eq!(
            service.get_prefix(),
            "/service/default/default/hello/latest"
        );
        assert_eq!(
            service.get_key(),
            "/service/default/default/hello/latest/127.0.0.1:8000"
        );
        let value = service.get_value();
        assert!(value.contains("\"weight\""), "got {value}");
        assert!(!service.get_metadata().is_empty());
    }

    #[test]
    fn service_from_key_value() {
        let service = LocalService::new_service_with_kv(
            "/service/default/default/hello/latest/127.0.0.1:8000",
            r#"{"weight":10,"protocol":"http"}"#,
        )
        .expect("service");
        assert_eq!(service.get_name(), "hello");
        assert_eq!(service.get_version(), "latest");
        assert_eq!(service.get_endpoints().to_string(), "127.0.0.1:8000");
        assert_eq!(
            service
                .get_metadata()
                .get(MD_WEIGHT)
                .expect("weight")
                .int64(),
            10
        );
        assert!(LocalService::new_service_with_kv("too/short", "").is_err());
        assert!(
            LocalService::new_service_with_kv("/service/default/default/hello/latest/bad", "")
                .is_err()
        );
    }

    #[test]
    fn metadata_helpers() {
        let mut metadata = Metadata::new();
        assert!(metadata.is_empty());
        metadata.set("a", Value::Int(1));
        let mut extra = IndexMap::new();
        extra.insert("b".to_owned(), Value::String("2".to_owned()));
        metadata.sets(extra);
        assert_eq!(metadata.get("a").expect("a").int64(), 1);
        assert_eq!(metadata.get("b").expect("b").string(), "2");
        assert!(metadata.get("missing").is_none());
        assert!(!metadata.is_empty());
    }

    #[test]
    fn default_registry_round_trip() {
        struct TestRegistry;
        impl Registrar for TestRegistry {
            fn register(&self, service: Arc<dyn Service>) -> Result<Arc<dyn Service>, Error> {
                Ok(service)
            }

            fn deregister(&self, _service: &Arc<dyn Service>) -> Result<(), Error> {
                Ok(())
            }
        }
        impl Discovery for TestRegistry {
            fn search(&self, _input: SearchInput) -> Result<Vec<Arc<dyn Service>>, Error> {
                Ok(Vec::new())
            }

            fn watch(&self, _key: &str) -> Result<Box<dyn Watcher>, Error> {
                Err(gerror::new("watch not supported"))
            }
        }
        impl Registry for TestRegistry {}

        // The global slot is process-wide; set it for this test only.
        let registry: Arc<dyn Registry> = Arc::new(TestRegistry);
        set_registry(Arc::clone(&registry));
        assert!(get_registry().is_some());
        let service: Arc<dyn Service> = Arc::new(LocalService::new_service_with_name("x"));
        assert!(register(Arc::clone(&service)).is_ok());
        assert!(deregister(&service).is_ok());
    }
}
