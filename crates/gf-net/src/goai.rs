//! Port of `net/goai` — the OpenAPI v3 document model.
//!
//! GoFrame's `goai` builds an OpenAPI 3.0 document from Go structs and route
//! functions through reflection (`gmeta`/`gstructs`). Rust has no runtime
//! struct reflection, so this port keeps the full document model and provides
//! a [`Value`]-based schema builder ([`schema_from_value`]) instead of the
//! struct-tag builder (ADR-065). The reflection-driven `Add`/`addPath`/
//! `addSchema` helpers are not ported yet.
//!
//! Divergences:
//!
//! - the document model is serialized with `serde` (camelCase field names,
//!   `$ref`-or-value references, flattened `x-` extensions);
//! - arbitrary JSON payloads (`example`, `default`, `enum`, link parameters,
//!   ...) use `serde_json::Value`;
//! - refs are one generic [`RefOr`] type instead of one type per model.

use gf_core::Value;
use gf_errors::gerror::{self, Error};
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

/// The custom `x-` extensions, spread into the parent object on output.
pub type XExtensions = IndexMap<String, serde_json::Value>;

/// Masks. Arbitrary JSON payloads use `serde_json::Value`.
pub type AnyValue = serde_json::Value;

/// A reference-or-value pair, like Go's `XxxRef` types.
///
/// When [`RefOr::reference`] is non-empty it serializes as `{"$ref": ...}`,
/// otherwise the nested value is serialized directly.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RefOr<T> {
    /// The reference path, e.g. `#/components/schemas/Pet`.
    pub reference: String,
    /// The inline value used when `reference` is empty.
    ///
    /// Boxed because schemas refer to schemas recursively.
    pub value: Box<T>,
}

impl<T: Default> RefOr<T> {
    /// Creates a `$ref` entry.
    pub fn new_ref(reference: &str) -> RefOr<T> {
        RefOr {
            reference: reference.to_owned(),
            value: Box::new(T::default()),
        }
    }

    /// Creates an inline value entry.
    pub fn new_value(value: T) -> RefOr<T> {
        RefOr {
            reference: String::new(),
            value: Box::new(value),
        }
    }

    /// The inline value.
    pub fn value_ref(&self) -> &T {
        &self.value
    }

    /// Takes the inline value.
    pub fn into_value(self) -> T {
        *self.value
    }
}

impl<T> RefOr<T> {
    /// Reports whether this is a `$ref` entry.
    pub fn is_ref(&self) -> bool {
        !self.reference.is_empty()
    }
}

impl<T: Serialize> Serialize for RefOr<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if self.is_ref() {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry("$ref", &self.reference)?;
            map.end()
        } else {
            self.value.serialize(serializer)
        }
    }
}

/// The document root, like Go's `OpenApiV3`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct OpenApiV3 {
    /// The OpenAPI version.
    pub openapi: String,
    /// Reusable components.
    #[serde(skip_serializing_if = "Components::is_empty")]
    pub components: Components,
    /// The API information.
    pub info: Info,
    /// The API paths.
    pub paths: Paths,
    /// Global security requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityRequirements>,
    /// Servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<Servers>,
    /// Tags.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Tags>,
    /// External documentation.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
}

/// The document configuration, like Go's `Config`.
#[derive(Clone, Debug)]
pub struct Config {
    /// Read content types.
    pub read_content_types: Vec<String>,
    /// Write content types.
    pub write_content_types: Vec<String>,
    /// Ignore the Go package path when formatting schema names.
    pub ignore_pkg_path: bool,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            read_content_types: vec!["application/json".to_owned()],
            write_content_types: vec!["application/json".to_owned()],
            ignore_pkg_path: false,
        }
    }
}

/// The schema type constants, like Go's `Type*`.
pub const TYPE_INTEGER: &str = "integer";
/// The number type.
pub const TYPE_NUMBER: &str = "number";
/// The boolean type.
pub const TYPE_BOOLEAN: &str = "boolean";
/// The array type.
pub const TYPE_ARRAY: &str = "array";
/// The string type.
pub const TYPE_STRING: &str = "string";
/// The file type.
pub const TYPE_FILE: &str = "file";
/// The object type.
pub const TYPE_OBJECT: &str = "object";
/// The int32 format.
pub const FORMAT_INT32: &str = "int32";
/// The int64 format.
pub const FORMAT_INT64: &str = "int64";
/// The double format.
pub const FORMAT_DOUBLE: &str = "double";
/// The byte format.
pub const FORMAT_BYTE: &str = "byte";
/// The binary format.
pub const FORMAT_BINARY: &str = "binary";
/// The date format.
pub const FORMAT_DATE: &str = "date";
/// The date-time format.
pub const FORMAT_DATE_TIME: &str = "date-time";
/// The password format.
pub const FORMAT_PASSWORD: &str = "password";

/// The parameter-in constants, like Go's `ParameterIn*`.
pub const PARAMETER_IN_HEADER: &str = "header";
/// Path parameters.
pub const PARAMETER_IN_PATH: &str = "path";
/// Query parameters.
pub const PARAMETER_IN_QUERY: &str = "query";
/// Cookie parameters.
pub const PARAMETER_IN_COOKIE: &str = "cookie";

impl OpenApiV3 {
    /// Creates a document with the defaults, like `New`.
    pub fn new() -> OpenApiV3 {
        OpenApiV3 {
            openapi: "3.0.0".to_owned(),
            ..OpenApiV3::default()
        }
    }

    /// The parse configuration (kept out of the JSON output).
    pub fn config(&self) -> Config {
        Config::default()
    }

    /// Inserts or replaces one schema under `components/schemas`.
    pub fn add_schema(&mut self, name: &str, schema: RefOr<Schema>) {
        self.components.schemas.insert(name.to_owned(), schema);
    }

    /// Inserts or replaces one path.
    pub fn add_path(&mut self, path: &str, item: Path) {
        self.paths.insert(path.to_owned(), item);
    }

    /// Adds or replaces one operation, the manual port of Go's `Add`
    /// (`addPath` assignment) without reflection (ADR-085).
    pub fn add_operation(
        &mut self,
        path: &str,
        method: &str,
        operation: Operation,
    ) -> Result<(), Error> {
        self.path_mut(path).set_operation(method, operation)
    }

    /// Returns one operation by path and method.
    pub fn find_operation(&self, path: &str, method: &str) -> Option<&Operation> {
        self.paths.get(path)?.operation(method)
    }

    /// Appends one server, like Go's server registration in `Add`.
    pub fn add_server(&mut self, server: Server) {
        self.servers.get_or_insert_with(Vec::new).push(server);
    }

    /// Appends one tag.
    pub fn add_tag(&mut self, tag: Tag) {
        self.tags.get_or_insert_with(Vec::new).push(tag);
    }

    /// Returns the mutable path item, creating it when missing, like Go's
    /// path lookup before assigning an operation.
    pub fn path_mut(&mut self, path: &str) -> &mut Path {
        self.paths.entry(path.to_owned()).or_default()
    }

    /// Serializes the document as compact JSON, like Go's `String`.
    pub fn to_json(&self) -> Result<String, Error> {
        serde_json::to_string(self)
            .map_err(|error| gerror::new(format!("marshal OpenAPI document failed: {error}")))
    }

    /// Serializes the document as pretty JSON.
    pub fn to_pretty_json(&self) -> Result<String, Error> {
        serde_json::to_string_pretty(self)
            .map_err(|error| gerror::new(format!("marshal OpenAPI document failed: {error}")))
    }
}

impl std::fmt::Display for OpenApiV3 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.to_json().unwrap_or_default())
    }
}

/// The API information, like Go's `Info`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Info {
    /// The title.
    pub title: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The terms of service URL.
    #[serde(rename = "termsOfService", skip_serializing_if = "String::is_empty")]
    pub terms_of_service: String,
    /// The contact.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contact: Option<Contact>,
    /// The license.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<License>,
    /// The API version.
    pub version: String,
}

/// The contact information, like Go's `Contact`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Contact {
    /// The name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The URL.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// The email.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub email: String,
}

/// The license information, like Go's `License`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct License {
    /// The license name.
    pub name: String,
    /// The license URL.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
}

/// External documentation, like Go's `ExternalDocs`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ExternalDocs {
    /// The URL.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub url: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// One tag, like Go's `Tag`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Tag {
    /// The tag name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// External documentation.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
}

/// The tag list.
pub type Tags = Vec<Tag>;

/// One server, like Go's `Server`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Server {
    /// The server URL.
    pub url: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Server variables.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub variables: Option<IndexMap<String, ServerVariable>>,
}

/// One server variable, like Go's `ServerVariable`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ServerVariable {
    /// The enum values.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub r#enum: Vec<String>,
    /// The default value.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub default: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
}

/// The server list.
pub type Servers = Vec<Server>;

/// One path item, like Go's `Path`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Path {
    /// The `$ref`.
    #[serde(rename = "$ref", skip_serializing_if = "String::is_empty")]
    pub reference: String,
    /// The summary.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// CONNECT operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect: Option<Operation>,
    /// DELETE operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete: Option<Operation>,
    /// GET operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub get: Option<Operation>,
    /// HEAD operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head: Option<Operation>,
    /// OPTIONS operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Operation>,
    /// PATCH operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<Operation>,
    /// POST operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post: Option<Operation>,
    /// PUT operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub put: Option<Operation>,
    /// TRACE operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Operation>,
    /// Path-level servers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Servers,
    /// Path-level parameters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Parameters,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

impl Path {
    /// Sets the operation for `method` (lowercase HTTP method), like Go's
    /// method switch in `addPath`.
    pub fn set_operation(&mut self, method: &str, operation: Operation) -> Result<(), Error> {
        let slot = match method.to_ascii_lowercase().as_str() {
            "connect" => &mut self.connect,
            "delete" => &mut self.delete,
            "get" => &mut self.get,
            "head" => &mut self.head,
            "options" => &mut self.options,
            "patch" => &mut self.patch,
            "post" => &mut self.post,
            "put" => &mut self.put,
            "trace" => &mut self.trace,
            other => {
                return Err(gerror::new(format!(
                    "unsupported HTTP method \"{other}\" for OpenAPI path"
                )));
            }
        };
        *slot = Some(operation);
        Ok(())
    }

    /// Returns the operation for `method`, if set.
    pub fn operation(&self, method: &str) -> Option<&Operation> {
        match method.to_ascii_lowercase().as_str() {
            "connect" => self.connect.as_ref(),
            "delete" => self.delete.as_ref(),
            "get" => self.get.as_ref(),
            "head" => self.head.as_ref(),
            "options" => self.options.as_ref(),
            "patch" => self.patch.as_ref(),
            "post" => self.post.as_ref(),
            "put" => self.put.as_ref(),
            "trace" => self.trace.as_ref(),
            _ => None,
        }
    }
}

/// The path collection.
pub type Paths = IndexMap<String, Path>;

/// One operation, like Go's `Operation`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Operation {
    /// Tags.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The summary.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The operation id.
    #[serde(rename = "operationId", skip_serializing_if = "String::is_empty")]
    pub operation_id: String,
    /// The parameters.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Parameters,
    /// The request body.
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<RefOr<RequestBody>>,
    /// The responses.
    pub responses: Responses,
    /// Whether the operation is deprecated.
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
    /// Callbacks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub callbacks: Option<Callbacks>,
    /// Security requirements.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<SecurityRequirements>,
    /// Servers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub servers: Option<Servers>,
    /// External documentation.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The `parameter` references.
pub type Parameters = Vec<RefOr<Parameter>>;

/// One parameter, like Go's `Parameter`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Parameter {
    /// The parameter name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Where the parameter is (`query`, `path`, `header`, `cookie`).
    #[serde(rename = "in", skip_serializing_if = "String::is_empty")]
    pub location: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The style.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// Whether to explode arrays/objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,
    /// Whether an empty value is allowed.
    #[serde(rename = "allowEmptyValue", skip_serializing_if = "is_false")]
    pub allow_empty_value: bool,
    /// Whether reserved characters are allowed.
    #[serde(rename = "allowReserved", skip_serializing_if = "is_false")]
    pub allow_reserved: bool,
    /// Whether the parameter is deprecated.
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
    /// Whether the parameter is required.
    #[serde(skip_serializing_if = "is_false")]
    pub required: bool,
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<RefOr<Schema>>,
    /// The example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<AnyValue>,
    /// The examples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Examples>,
    /// The content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The `requestBody` references.
pub type RequestBodies = IndexMap<String, RefOr<RequestBody>>;

/// One request body, like Go's `RequestBody`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct RequestBody {
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Whether the body is required.
    #[serde(skip_serializing_if = "is_false")]
    pub required: bool,
    /// The content.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub content: Content,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The response collection.
pub type Responses = IndexMap<String, RefOr<Response>>;

/// One response, like Go's `Response`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Response {
    /// The description.
    pub description: String,
    /// The headers.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub headers: Headers,
    /// The content.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub content: Content,
    /// The links.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub links: Links,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The header collection.
pub type Headers = IndexMap<String, RefOr<Header>>;

/// One header, like Go's `Header`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Header {
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Whether the header is required.
    #[serde(skip_serializing_if = "is_false")]
    pub required: bool,
    /// Whether the header is deprecated.
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
    /// The style.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// Whether to explode arrays/objects.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<RefOr<Schema>>,
    /// The example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<AnyValue>,
    /// The examples.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Examples>,
    /// The content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<Content>,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The link collection.
pub type Links = IndexMap<String, RefOr<Link>>;

/// One link, like Go's `Link`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Link {
    /// The operation id.
    #[serde(rename = "operationId", skip_serializing_if = "String::is_empty")]
    pub operation_id: String,
    /// The operation reference.
    #[serde(rename = "operationRef", skip_serializing_if = "String::is_empty")]
    pub operation_ref: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The parameters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parameters: Option<IndexMap<String, AnyValue>>,
    /// The server.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<Server>,
    /// The request body.
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<AnyValue>,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The example collection.
pub type Examples = IndexMap<String, RefOr<Example>>;

/// One example, like Go's `Example`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Example {
    /// The summary.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub summary: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<AnyValue>,
    /// The external value URL.
    #[serde(rename = "externalValue", skip_serializing_if = "String::is_empty")]
    pub external_value: String,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The callback collection.
pub type Callbacks = IndexMap<String, RefOr<Callback>>;

/// A callback map, like Go's `Callback`.
pub type Callback = IndexMap<String, Path>;

/// The content collection.
pub type Content = IndexMap<String, MediaType>;

/// One media type, like Go's `MediaType`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct MediaType {
    /// The schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schema: Option<RefOr<Schema>>,
    /// The example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<AnyValue>,
    /// The examples.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub examples: Examples,
    /// The encoding.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encoding: Option<IndexMap<String, Encoding>>,
}

/// One encoding, like Go's `Encoding`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Encoding {
    /// The content type.
    #[serde(rename = "contentType", skip_serializing_if = "String::is_empty")]
    pub content_type: String,
    /// The headers.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub headers: Headers,
    /// The style.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// Whether to explode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,
    /// Whether reserved characters are allowed.
    #[serde(rename = "allowReserved", skip_serializing_if = "is_false")]
    pub allow_reserved: bool,
}

/// The schema collection.
pub type Schemas = IndexMap<String, RefOr<Schema>>;

/// One schema, like Go's `Schema`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Schema {
    /// Any-of schemas.
    #[serde(rename = "oneOf", skip_serializing_if = "Vec::is_empty")]
    pub one_of: SchemaRefs,
    /// All-of schemas.
    #[serde(rename = "anyOf", skip_serializing_if = "Vec::is_empty")]
    pub any_of: SchemaRefs,
    /// All-of schemas.
    #[serde(rename = "allOf", skip_serializing_if = "Vec::is_empty")]
    pub all_of: SchemaRefs,
    /// Negated schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not: Option<RefOr<Schema>>,
    /// The type.
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub schema_type: String,
    /// The title.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    /// The format.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub format: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The enum values.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub r#enum: Vec<AnyValue>,
    /// The default value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<AnyValue>,
    /// The example.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<AnyValue>,
    /// External documentation.
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<ExternalDocs>,
    /// Whether items are unique.
    #[serde(rename = "uniqueItems", skip_serializing_if = "is_false")]
    pub unique_items: bool,
    /// Exclusive minimum.
    #[serde(rename = "exclusiveMinimum", skip_serializing_if = "is_false")]
    pub exclusive_min: bool,
    /// Exclusive maximum.
    #[serde(rename = "exclusiveMaximum", skip_serializing_if = "is_false")]
    pub exclusive_max: bool,
    /// Whether the value is nullable.
    #[serde(skip_serializing_if = "is_false")]
    pub nullable: bool,
    /// Whether the value is read-only.
    #[serde(rename = "readOnly", skip_serializing_if = "is_false")]
    pub read_only: bool,
    /// Whether the value is write-only.
    #[serde(rename = "writeOnly", skip_serializing_if = "is_false")]
    pub write_only: bool,
    /// Whether an empty value is allowed.
    #[serde(rename = "allowEmptyValue", skip_serializing_if = "is_false")]
    pub allow_empty_value: bool,
    /// XML metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub xml: Option<AnyValue>,
    /// Whether the schema is deprecated.
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
    /// The minimum.
    #[serde(rename = "minimum", skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    /// The maximum.
    #[serde(rename = "maximum", skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    /// The multiple.
    #[serde(rename = "multipleOf", skip_serializing_if = "Option::is_none")]
    pub multiple_of: Option<f64>,
    /// The minimum length.
    #[serde(rename = "minLength", skip_serializing_if = "is_zero_u64")]
    pub min_length: u64,
    /// The maximum length.
    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,
    /// The pattern.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub pattern: String,
    /// The minimum item count.
    #[serde(rename = "minItems", skip_serializing_if = "is_zero_u64")]
    pub min_items: u64,
    /// The maximum item count.
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u64>,
    /// The items schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<RefOr<Schema>>,
    /// The required property names.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    /// The property schemas.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<Schemas>,
    /// The minimum property count.
    #[serde(rename = "minProperties", skip_serializing_if = "is_zero_u64")]
    pub min_props: u64,
    /// The maximum property count.
    #[serde(rename = "maxProperties", skip_serializing_if = "Option::is_none")]
    pub max_props: Option<u64>,
    /// Additional properties.
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<RefOr<Schema>>,
    /// The discriminator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discriminator: Option<Discriminator>,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// Reports whether a boolean is false (serde skip helper).
fn is_false(value: &bool) -> bool {
    !*value
}

/// The schema references.
pub type SchemaRefs = Vec<RefOr<Schema>>;

/// The discriminator, like Go's `Discriminator`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Discriminator {
    /// The discriminating property name.
    #[serde(rename = "propertyName")]
    pub property_name: String,
    /// The mapping.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mapping: Option<IndexMap<String, String>>,
}

/// The reusable components, like Go's `Components`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Components {
    /// The schemas.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub schemas: Schemas,
    /// The parameters.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub parameters: IndexMap<String, RefOr<Parameter>>,
    /// The headers.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub headers: Headers,
    /// The request bodies.
    #[serde(rename = "requestBodies", skip_serializing_if = "IndexMap::is_empty")]
    pub request_bodies: RequestBodies,
    /// The responses.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub responses: Responses,
    /// The security schemes.
    #[serde(rename = "securitySchemes", skip_serializing_if = "IndexMap::is_empty")]
    pub security_schemes: SecuritySchemes,
    /// The examples.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub examples: Examples,
    /// The links.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub links: Links,
    /// The callbacks.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub callbacks: Callbacks,
}

impl Components {
    fn is_empty(&self) -> bool {
        self.schemas.is_empty()
            && self.parameters.is_empty()
            && self.headers.is_empty()
            && self.request_bodies.is_empty()
            && self.responses.is_empty()
            && self.security_schemes.is_empty()
            && self.examples.is_empty()
            && self.links.is_empty()
            && self.callbacks.is_empty()
    }
}

/// The security scheme collection.
pub type SecuritySchemes = IndexMap<String, RefOr<SecurityScheme>>;

/// One security scheme, like Go's `SecurityScheme`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SecurityScheme {
    /// The type.
    #[serde(rename = "type", skip_serializing_if = "String::is_empty")]
    pub scheme_type: String,
    /// The description.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// The parameter name.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Where the API key is (`query`, `header`, `cookie`).
    #[serde(rename = "in", skip_serializing_if = "String::is_empty")]
    pub location: String,
    /// The HTTP auth scheme.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub scheme: String,
    /// The bearer format.
    #[serde(rename = "bearerFormat", skip_serializing_if = "String::is_empty")]
    pub bearer_format: String,
    /// The OAuth flows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flows: Option<OAuthFlows>,
    /// The OpenID Connect URL.
    #[serde(rename = "openIdConnectUrl", skip_serializing_if = "String::is_empty")]
    pub open_id_connect_url: String,
    /// Custom `x-` extensions.
    #[serde(flatten)]
    pub x_extensions: XExtensions,
}

/// The OAuth flows, like Go's `OAuthFlows`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct OAuthFlows {
    /// The implicit flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit: Option<OAuthFlow>,
    /// The password flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<OAuthFlow>,
    /// The client credentials flow.
    #[serde(rename = "clientCredentials", skip_serializing_if = "Option::is_none")]
    pub client_credentials: Option<OAuthFlow>,
    /// The authorization code flow.
    #[serde(rename = "authorizationCode", skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<OAuthFlow>,
}

/// One OAuth flow, like Go's `OAuthFlow`.
#[derive(Clone, Debug, Default, Serialize)]
pub struct OAuthFlow {
    /// The authorization URL.
    #[serde(rename = "authorizationUrl", skip_serializing_if = "String::is_empty")]
    pub authorization_url: String,
    /// The token URL.
    #[serde(rename = "tokenUrl", skip_serializing_if = "String::is_empty")]
    pub token_url: String,
    /// The refresh URL.
    #[serde(rename = "refreshUrl", skip_serializing_if = "String::is_empty")]
    pub refresh_url: String,
    /// The scopes.
    pub scopes: IndexMap<String, String>,
}

/// One security requirement, like Go's `SecurityRequirement`.
pub type SecurityRequirement = IndexMap<String, Vec<String>>;

/// The security requirements.
pub type SecurityRequirements = Vec<SecurityRequirement>;

/// Builds a schema from a [`Value`], replacing Go's reflection-based
/// builder for the dynamic-data use case.
///
/// Maps become `object` schemas with `properties`, lists become `array`
/// schemas with `items`, and scalars map to their OpenAPI type.
pub fn schema_from_value(value: &Value) -> RefOr<Schema> {
    let schema = match value {
        Value::Null => Schema {
            nullable: true,
            ..Schema::default()
        },
        Value::Bool(_) => Schema {
            schema_type: TYPE_BOOLEAN.to_owned(),
            ..Schema::default()
        },
        Value::Int(_) | Value::UInt(_) => Schema {
            schema_type: TYPE_INTEGER.to_owned(),
            ..Schema::default()
        },
        Value::Float(_) => Schema {
            schema_type: TYPE_NUMBER.to_owned(),
            ..Schema::default()
        },
        Value::String(_) => Schema {
            schema_type: TYPE_STRING.to_owned(),
            ..Schema::default()
        },
        Value::Bytes(_) => Schema {
            schema_type: TYPE_STRING.to_owned(),
            format: FORMAT_BINARY.to_owned(),
            ..Schema::default()
        },
        Value::List(items) => Schema {
            schema_type: TYPE_ARRAY.to_owned(),
            items: items.first().map(schema_from_value),
            ..Schema::default()
        },
        Value::Map(map) => {
            let mut properties = Schemas::new();
            for (key, item) in map {
                properties.insert(key.clone(), schema_from_value(item));
            }
            Schema {
                schema_type: TYPE_OBJECT.to_owned(),
                properties: Some(properties),
                ..Schema::default()
            }
        }
    };
    RefOr::new_value(schema)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn schema_ref() -> RefOr<Schema> {
        RefOr::new_ref("#/components/schemas/Pet")
    }

    #[test]
    fn document_defaults_and_json() {
        let mut document = OpenApiV3::new();
        assert_eq!(document.openapi, "3.0.0");
        assert_eq!(document.config().read_content_types, ["application/json"]);
        document.info = Info {
            title: "GoFrame API".to_owned(),
            version: "v1.0.0".to_owned(),
            ..Info::default()
        };
        document.add_schema(
            "Pet",
            RefOr::new_value(Schema {
                schema_type: TYPE_OBJECT.to_owned(),
                properties: Some(Schemas::from([(
                    "name".to_owned(),
                    schema_from_value(&Value::String("x".to_owned())),
                )])),
                required: vec!["name".to_owned()],
                ..Schema::default()
            }),
        );
        let mut path = Path::default();
        path.set_operation(
            "get",
            Operation {
                summary: "Get pet".to_owned(),
                responses: Responses::from([(
                    "200".to_owned(),
                    RefOr::new_ref("#/components/responses/Pet"),
                )]),
                ..Operation::default()
            },
        )
        .expect("operation");
        document.add_path("/pet", path);
        let json = document.to_json().expect("json");
        assert!(json.contains("\"openapi\":\"3.0.0\""), "{json}");
        assert!(json.contains("\"title\":\"GoFrame API\""), "{json}");
        assert!(json.contains("\"Pet\":{\"type\":\"object\""), "{json}");
        assert!(
            json.contains("\"$ref\":\"#/components/responses/Pet\""),
            "{json}"
        );
        assert!(json.contains("\"get\":{\"summary\":\"Get pet\""), "{json}");
    }

    #[test]
    fn reference_or_value_serialization() {
        assert_eq!(
            serde_json::to_value(schema_ref()).expect("ref"),
            serde_json::json!({"$ref": "#/components/schemas/Pet"})
        );
        assert_eq!(
            serde_json::to_value(RefOr::new_value(Schema {
                schema_type: TYPE_STRING.to_owned(),
                ..Schema::default()
            }))
            .expect("value"),
            serde_json::json!({"type": "string"})
        );
    }

    #[test]
    fn schema_from_value_builds_types() {
        let mut properties = IndexMap::new();
        properties.insert("name".to_owned(), Value::String("john".to_owned()));
        properties.insert("age".to_owned(), Value::Int(18));
        let schema = schema_from_value(&Value::Map(properties));
        let json = serde_json::to_value(schema).expect("schema");
        assert_eq!(json["type"], "object");
        assert_eq!(json["properties"]["name"]["type"], "string");
        assert_eq!(json["properties"]["age"]["type"], "integer");
        let array = serde_json::to_value(schema_from_value(&Value::List(vec![Value::Bool(true)])))
            .expect("array");
        assert_eq!(array["type"], "array");
        assert_eq!(array["items"]["type"], "boolean");
    }

    #[test]
    fn operation_and_parameter_output() {
        let operation = Operation {
            tags: vec!["pet".to_owned()],
            operation_id: "getPet".to_owned(),
            parameters: vec![RefOr::new_value(Parameter {
                name: "id".to_owned(),
                location: PARAMETER_IN_PATH.to_owned(),
                required: true,
                schema: Some(RefOr::new_value(Schema {
                    schema_type: TYPE_INTEGER.to_owned(),
                    format: FORMAT_INT64.to_owned(),
                    ..Schema::default()
                })),
                ..Parameter::default()
            })],
            request_body: Some(RefOr::new_value(RequestBody {
                required: true,
                content: Content::from([(
                    "application/json".to_owned(),
                    MediaType {
                        schema: Some(schema_ref()),
                        ..MediaType::default()
                    },
                )]),
                ..RequestBody::default()
            })),
            ..Operation::default()
        };
        let json = serde_json::to_value(operation).expect("operation");
        assert_eq!(json["operationId"], "getPet");
        assert_eq!(json["parameters"][0]["in"], "path");
        assert_eq!(
            json["requestBody"]["content"]["application/json"]["schema"]["$ref"],
            "#/components/schemas/Pet"
        );
    }

    #[test]
    fn security_and_servers() {
        let scheme = RefOr::new_value(SecurityScheme {
            scheme_type: "oauth2".to_owned(),
            flows: Some(OAuthFlows {
                authorization_code: Some(OAuthFlow {
                    authorization_url: "https://example.com/auth".to_owned(),
                    token_url: "https://example.com/token".to_owned(),
                    scopes: IndexMap::from([("read".to_owned(), "read access".to_owned())]),
                    ..OAuthFlow::default()
                }),
                ..OAuthFlows::default()
            }),
            ..SecurityScheme::default()
        });
        let document = OpenApiV3 {
            security: Some(vec![IndexMap::from([(
                "oauth2".to_owned(),
                vec!["read".to_owned()],
            )])]),
            servers: Some(vec![Server {
                url: "https://example.com".to_owned(),
                ..Server::default()
            }]),
            ..OpenApiV3::new()
        };
        let json = serde_json::to_value(document).expect("document");
        assert_eq!(json["security"][0]["oauth2"][0], "read");
        assert_eq!(json["servers"][0]["url"], "https://example.com");
        let json = serde_json::to_value(scheme).expect("scheme");
        assert_eq!(json["type"], "oauth2");
        assert_eq!(
            json["flows"]["authorizationCode"]["scopes"]["read"],
            "read access"
        );
    }

    #[test]
    fn extensions_are_flattened() {
        let mut extension = XExtensions::new();
        extension.insert("x-custom".to_owned(), serde_json::json!("value"));
        let operation = Operation {
            x_extensions: extension,
            ..Operation::default()
        };
        let json = serde_json::to_value(operation).expect("operation");
        assert_eq!(json["x-custom"], "value");
    }

    #[test]
    fn add_operation_helpers_build_paths() {
        let mut document = OpenApiV3::new();
        document
            .add_operation(
                "/pet",
                "GET",
                Operation {
                    summary: "List pets".to_owned(),
                    responses: Responses::from([("200".to_owned(), RefOr::default())]),
                    ..Operation::default()
                },
            )
            .expect("get");
        document
            .add_operation(
                "/pet",
                "post",
                Operation {
                    summary: "Create pet".to_owned(),
                    responses: Responses::from([("201".to_owned(), RefOr::default())]),
                    ..Operation::default()
                },
            )
            .expect("post");
        assert_eq!(
            document.find_operation("/pet", "get").expect("get").summary,
            "List pets"
        );
        assert_eq!(
            document
                .find_operation("/pet", "POST")
                .expect("post")
                .summary,
            "Create pet"
        );
        assert!(document.find_operation("/pet", "delete").is_none());
        assert!(document.find_operation("/missing", "get").is_none());
        assert!(
            document
                .add_operation("/pet", "bogus", Operation::default())
                .is_err()
        );
        let json = document.to_json().expect("json");
        assert!(
            json.contains("\"get\":{\"summary\":\"List pets\""),
            "{json}"
        );
        assert!(
            json.contains("\"post\":{\"summary\":\"Create pet\""),
            "{json}"
        );
    }

    #[test]
    fn add_server_and_tag_helpers() {
        let mut document = OpenApiV3::new();
        document.add_server(Server {
            url: "https://api.example.com".to_owned(),
            ..Server::default()
        });
        document.add_server(Server {
            url: "https://staging.example.com".to_owned(),
            ..Server::default()
        });
        document.add_tag(Tag {
            name: "pet".to_owned(),
            ..Tag::default()
        });
        let json = serde_json::to_value(&document).expect("json");
        assert_eq!(json["servers"][0]["url"], "https://api.example.com");
        assert_eq!(json["servers"][1]["url"], "https://staging.example.com");
        assert_eq!(json["tags"][0]["name"], "pet");
    }

    #[test]
    fn path_operations_accessors() {
        let mut path = Path::default();
        path.set_operation("POST", Operation::default())
            .expect("post");
        assert!(path.operation("post").is_some());
        assert!(path.operation("get").is_none());
        assert!(path.set_operation("bogus", Operation::default()).is_err());
        let mut document = OpenApiV3::new();
        document.path_mut("/pet").summary = "Pets".to_owned();
        assert_eq!(document.paths["/pet"].summary, "Pets");
    }

    #[test]
    fn pretty_print_round_trips() {
        let document = OpenApiV3::new();
        let pretty = document.to_pretty_json().expect("pretty");
        let parsed: serde_json::Value = serde_json::from_str(&pretty).expect("parse");
        assert_eq!(parsed["openapi"], "3.0.0");
        assert!(!document.to_string().is_empty());
    }
}
