use crate::extensions::Extensions;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    #[serde(rename = "jsonSchemaDialect", default)]
    pub json_schema_dialect: Option<String>,
    #[serde(default)]
    pub servers: Option<Vec<Server>>,
    #[serde(default)]
    pub paths: Option<BTreeMap<String, PathItem>>,
    #[serde(default)]
    pub webhooks: Option<BTreeMap<String, PathItem>>,
    #[serde(default)]
    pub components: Option<Components>,
    #[serde(default)]
    pub security: Option<Vec<BTreeMap<String, Vec<String>>>>,
    #[serde(default)]
    pub tags: Option<Vec<Tag>>,
    #[serde(rename = "externalDocs", default)]
    pub external_docs: Option<ExternalDocs>,
    /// 3.2 §"$self" — see Appendix F base-URI rules. Captured but not yet used.
    #[serde(rename = "$self", default)]
    pub self_uri: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "termsOfService", default)]
    pub terms_of_service: Option<String>,
    #[serde(default)]
    pub contact: Option<Value>,
    #[serde(default)]
    pub license: Option<Value>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Components {
    #[serde(default)]
    pub schemas: Option<BTreeMap<String, Schema>>,
    #[serde(default)]
    pub responses: Option<BTreeMap<String, Response>>,
    #[serde(default)]
    pub parameters: Option<BTreeMap<String, Parameter>>,
    #[serde(default)]
    pub examples: Option<BTreeMap<String, Example>>,
    #[serde(rename = "requestBodies", default)]
    pub request_bodies: Option<BTreeMap<String, RequestBody>>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, Header>>,
    #[serde(rename = "securitySchemes", default)]
    pub security_schemes: Option<BTreeMap<String, SecurityScheme>>,
    #[serde(default)]
    pub links: Option<BTreeMap<String, Link>>,
    #[serde(default)]
    pub callbacks: Option<BTreeMap<String, Callback>>,
    /// 3.1+ §Components — reusable Path Items.
    #[serde(rename = "pathItems", default)]
    pub path_items: Option<BTreeMap<String, PathItem>>,
    /// 3.2 §Components — reusable Media Types.
    #[serde(rename = "mediaTypes", default)]
    pub media_types: Option<BTreeMap<String, MediaType>>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Schema {
    /// Schema reference
    Reference {
        #[serde(rename = "$ref")]
        reference: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    /// Recursive reference (older draft, kept for OAS 3.0 compatibility)
    RecursiveRef {
        #[serde(rename = "$recursiveRef")]
        recursive_ref: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    /// Dynamic reference per JSON Schema 2020-12 (OAS 3.1+).
    /// `$dynamicRef` resolves against the nearest enclosing `$dynamicAnchor`.
    /// J1: modeled today; full dynamic resolution at analysis time is a
    /// follow-up. Self-references via `$dynamicRef: "#x"` are treated as
    /// recursive references to the schema bearing `$dynamicAnchor: "x"`.
    DynamicRef {
        #[serde(rename = "$dynamicRef")]
        dynamic_ref: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    /// OneOf union
    OneOf {
        #[serde(rename = "oneOf")]
        one_of: Vec<Schema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        discriminator: Option<Discriminator>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// AnyOf union (must come before Typed to handle type + anyOf patterns)
    AnyOf {
        #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
        schema_type: Option<SchemaType>,
        #[serde(rename = "anyOf")]
        any_of: Vec<Schema>,
        #[serde(skip_serializing_if = "Option::is_none")]
        discriminator: Option<Discriminator>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// Schema with `type` as an array (OpenAPI 3.1 / JSON Schema 2020-12).
    /// The canonical 3.1 way to express a nullable type is
    /// `type: ["string", "null"]`. Listed before `Typed` so the array form
    /// matches first.
    TypedMulti {
        #[serde(rename = "type")]
        schema_types: Vec<SchemaType>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// Schema with a single explicit type
    Typed {
        #[serde(rename = "type")]
        schema_type: SchemaType,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// AllOf composition
    AllOf {
        #[serde(rename = "allOf")]
        all_of: Vec<Schema>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// Schema without explicit type (inferred from other fields)
    Untyped {
        #[serde(flatten)]
        details: SchemaDetails,
    },
}

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SchemaType {
    String,
    Integer,
    Number,
    Boolean,
    Array,
    Object,
    #[serde(rename = "null")]
    Null,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct SchemaDetails {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nullable: Option<bool>,

    // OpenAPI 3.0 recursive support (obsoleted by JSON Schema 2020-12).
    #[serde(rename = "$recursiveAnchor", skip_serializing_if = "Option::is_none")]
    pub recursive_anchor: Option<bool>,

    // JSON Schema 2020-12 dynamic anchors (J1).
    #[serde(rename = "$dynamicAnchor", skip_serializing_if = "Option::is_none")]
    pub dynamic_anchor: Option<String>,
    #[serde(rename = "$id", skip_serializing_if = "Option::is_none")]
    pub schema_id: Option<String>,

    // String-specific
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(
        rename = "const",
        default,
        deserialize_with = "deserialize_present_value",
        skip_serializing_if = "Option::is_none"
    )]
    pub const_value: Option<Value>,

    // Object-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub properties: Option<BTreeMap<String, Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required: Option<Vec<String>>,
    #[serde(
        rename = "additionalProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub additional_properties: Option<AdditionalProperties>,

    // Array-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub items: Option<Box<Schema>>,

    // Number-specific
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<f64>,

    // Validation
    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<u64>,
    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// In 3.0/Swagger this was a `bool` flag relative to `minimum`; in 3.1
    /// (JSON Schema 2020-12) it's a number. Accept either to round-trip
    /// real-world specs. (Tracked under J3 — proper validation lowering.)
    #[serde(rename = "exclusiveMinimum", skip_serializing_if = "Option::is_none")]
    pub exclusive_minimum: Option<ExclusiveBound>,
    #[serde(rename = "exclusiveMaximum", skip_serializing_if = "Option::is_none")]
    pub exclusive_maximum: Option<ExclusiveBound>,
    #[serde(rename = "multipleOf", skip_serializing_if = "Option::is_none")]
    pub multiple_of: Option<f64>,
    #[serde(rename = "minItems", skip_serializing_if = "Option::is_none")]
    pub min_items: Option<u64>,
    #[serde(rename = "maxItems", skip_serializing_if = "Option::is_none")]
    pub max_items: Option<u64>,
    #[serde(rename = "uniqueItems", skip_serializing_if = "Option::is_none")]
    pub unique_items: Option<bool>,
    #[serde(rename = "minProperties", skip_serializing_if = "Option::is_none")]
    pub min_properties: Option<u64>,
    #[serde(rename = "maxProperties", skip_serializing_if = "Option::is_none")]
    pub max_properties: Option<u64>,

    // JSON Schema 2020-12 array keywords (J4, J8).
    #[serde(rename = "prefixItems", skip_serializing_if = "Option::is_none")]
    pub prefix_items: Option<Vec<Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub contains: Option<Box<Schema>>,
    #[serde(rename = "minContains", skip_serializing_if = "Option::is_none")]
    pub min_contains: Option<u64>,
    #[serde(rename = "maxContains", skip_serializing_if = "Option::is_none")]
    pub max_contains: Option<u64>,

    // JSON Schema 2020-12 object keywords (J5, J6, J7).
    #[serde(rename = "patternProperties", skip_serializing_if = "Option::is_none")]
    pub pattern_properties: Option<BTreeMap<String, Schema>>,
    #[serde(rename = "propertyNames", skip_serializing_if = "Option::is_none")]
    pub property_names: Option<Box<Schema>>,
    #[serde(
        rename = "unevaluatedProperties",
        skip_serializing_if = "Option::is_none"
    )]
    pub unevaluated_properties: Option<AdditionalProperties>,
    #[serde(rename = "unevaluatedItems", skip_serializing_if = "Option::is_none")]
    pub unevaluated_items: Option<AdditionalProperties>,
    #[serde(rename = "dependentRequired", skip_serializing_if = "Option::is_none")]
    pub dependent_required: Option<BTreeMap<String, Vec<String>>>,
    #[serde(rename = "dependentSchemas", skip_serializing_if = "Option::is_none")]
    pub dependent_schemas: Option<BTreeMap<String, Schema>>,

    // JSON Schema 2020-12 content keywords (J8).
    #[serde(rename = "contentEncoding", skip_serializing_if = "Option::is_none")]
    pub content_encoding: Option<String>,
    #[serde(rename = "contentMediaType", skip_serializing_if = "Option::is_none")]
    pub content_media_type: Option<String>,
    #[serde(rename = "contentSchema", skip_serializing_if = "Option::is_none")]
    pub content_schema: Option<Box<Schema>>,

    // JSON Schema 2020-12 conditional keywords.
    #[serde(rename = "if", skip_serializing_if = "Option::is_none")]
    pub if_schema: Option<Box<Schema>>,
    #[serde(rename = "then", skip_serializing_if = "Option::is_none")]
    pub then_schema: Option<Box<Schema>>,
    #[serde(rename = "else", skip_serializing_if = "Option::is_none")]
    pub else_schema: Option<Box<Schema>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not: Option<Box<Schema>>,

    // 3.0 deprecated annotations now first-class (kept since openai-responses fixture is OAS 3.0).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
    #[serde(rename = "readOnly", skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    #[serde(rename = "writeOnly", skip_serializing_if = "Option::is_none")]
    pub write_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub examples: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    /// JSON Schema annotation `$comment`.
    #[serde(rename = "$comment", skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(rename = "$schema", skip_serializing_if = "Option::is_none")]
    pub schema_keyword: Option<String>,
    #[serde(rename = "$defs", skip_serializing_if = "Option::is_none")]
    pub defs: Option<BTreeMap<String, Schema>>,

    // Extensions and unknown fields. After J5–J8 above this should be x-*-only
    // for well-formed OAS 3.1+ specs.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

fn deserialize_present_value<'de, D>(deserializer: D) -> Result<Option<Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Value::deserialize(deserializer).map(Some)
}

/// 3.0 used `exclusiveMinimum: true` as a bool flag against `minimum`;
/// 3.1 (JSON Schema 2020-12) uses `exclusiveMinimum: <number>`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ExclusiveBound {
    Bool(bool),
    Number(f64),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Boolean(bool),
    Schema(Box<Schema>),
}

/// OpenAPI Example Object (H6).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Example {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// Singular embedded value. Mutually exclusive with `external_value`.
    #[serde(default)]
    pub value: Option<Value>,
    #[serde(rename = "externalValue", default)]
    pub external_value: Option<String>,
    /// 3.2 §"Example Object" — typed pre-serialization data.
    #[serde(rename = "dataValue", default)]
    pub data_value: Option<Value>,
    /// 3.2 §"Example Object" — already-serialized form.
    #[serde(rename = "serializedValue", default)]
    pub serialized_value: Option<String>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Link Object (H7).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Link {
    #[serde(rename = "operationRef", default)]
    pub operation_ref: Option<String>,
    #[serde(rename = "operationId", default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub parameters: Option<BTreeMap<String, Value>>,
    #[serde(rename = "requestBody", default)]
    pub request_body: Option<Value>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub server: Option<Server>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Callback Object (H8). A map keyed by runtime-expression URL
/// templates, with Path Item values.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(transparent)]
pub struct Callback(pub BTreeMap<String, PathItem>);

/// OpenAPI Encoding Object (H4). Used inside `multipart/form-data` and
/// `application/x-www-form-urlencoded` Media Type bodies.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Encoding {
    #[serde(rename = "contentType", default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, Header>>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub explode: Option<bool>,
    #[serde(rename = "allowReserved", default)]
    pub allow_reserved: Option<bool>,
    /// 3.2 §"Encoding Object" — nested encoding for arrays of items.
    #[serde(rename = "itemEncoding", default)]
    pub item_encoding: Option<Box<Encoding>>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Header Object (H5). Structurally a Parameter minus the `name`
/// and `in` fields. Used in Response.headers, Encoding.headers, and
/// Components.headers.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Header {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    #[serde(rename = "allowEmptyValue", default)]
    pub allow_empty_value: Option<bool>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub explode: Option<bool>,
    #[serde(rename = "allowReserved", default)]
    pub allow_reserved: Option<bool>,
    #[serde(default)]
    pub schema: Option<Schema>,
    #[serde(default)]
    pub content: Option<BTreeMap<String, MediaType>>,
    #[serde(default)]
    pub example: Option<Value>,
    #[serde(default)]
    pub examples: Option<Value>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Security Scheme Object (H2). Covers all 3.x scheme types:
/// apiKey, http (basic/bearer/digest), oauth2 (with flows), openIdConnect,
/// and 3.1+ mutualTLS.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type")]
pub enum SecurityScheme {
    #[serde(rename = "apiKey")]
    ApiKey {
        name: String,
        #[serde(rename = "in")]
        location: String, // "query" | "header" | "cookie"
        #[serde(default)]
        description: Option<String>,
        /// 3.2 §"Security Scheme Object" — D10.
        #[serde(default)]
        deprecated: Option<bool>,
        #[serde(flatten, default)]
        extensions: Extensions,
    },
    #[serde(rename = "http")]
    Http {
        scheme: String, // "basic" | "bearer" | "digest" | …
        #[serde(rename = "bearerFormat", default)]
        bearer_format: Option<String>,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        deprecated: Option<bool>,
        #[serde(flatten, default)]
        extensions: Extensions,
    },
    #[serde(rename = "mutualTLS")]
    MutualTls {
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        deprecated: Option<bool>,
        #[serde(flatten, default)]
        extensions: Extensions,
    },
    #[serde(rename = "oauth2")]
    OAuth2 {
        // Boxed to keep the SecurityScheme enum's variants similarly sized
        // (the OAuthFlows tree is ~800 bytes; clippy::large_enum_variant
        // flagged the disparity).
        flows: Box<OAuthFlows>,
        #[serde(default)]
        description: Option<String>,
        /// 3.2 §"Security Scheme Object" — well-known metadata URL (D4).
        #[serde(rename = "oauth2MetadataUrl", default)]
        oauth2_metadata_url: Option<String>,
        #[serde(default)]
        deprecated: Option<bool>,
        #[serde(flatten, default)]
        extensions: Extensions,
    },
    #[serde(rename = "openIdConnect")]
    OpenIdConnect {
        #[serde(rename = "openIdConnectUrl")]
        open_id_connect_url: String,
        #[serde(default)]
        description: Option<String>,
        #[serde(default)]
        deprecated: Option<bool>,
        #[serde(flatten, default)]
        extensions: Extensions,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OAuthFlows {
    #[serde(default)]
    pub implicit: Option<OAuthFlow>,
    #[serde(default)]
    pub password: Option<OAuthFlow>,
    #[serde(rename = "clientCredentials", default)]
    pub client_credentials: Option<OAuthFlow>,
    #[serde(rename = "authorizationCode", default)]
    pub authorization_code: Option<OAuthFlow>,
    /// 3.2 §"OAuth Flows Object" — device authorization flow (D4).
    #[serde(rename = "deviceAuthorization", default)]
    pub device_authorization: Option<OAuthFlow>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OAuthFlow {
    #[serde(rename = "authorizationUrl", default)]
    pub authorization_url: Option<String>,
    #[serde(rename = "tokenUrl", default)]
    pub token_url: Option<String>,
    #[serde(rename = "refreshUrl", default)]
    pub refresh_url: Option<String>,
    /// 3.2 §"OAuth Flow Object" — required for `deviceAuthorization` (D4).
    #[serde(rename = "deviceAuthorizationUrl", default)]
    pub device_authorization_url: Option<String>,
    pub scopes: BTreeMap<String, String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI External Documentation Object (H10).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ExternalDocs {
    pub url: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Tag Object (H9 + D5 — 3.2 added summary/parent/kind).
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tag {
    pub name: String,
    /// 3.2 §"Tag Object" — short summary of the tag.
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    /// 3.2 §"Tag Object" — name of a parent tag for hierarchical organisation.
    #[serde(default)]
    pub parent: Option<String>,
    /// 3.2 §"Tag Object" — categorisation hint (e.g. "feature", "audience",
    /// "compliance"). Free-form string; consumers MAY define their own
    /// vocabulary.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(rename = "externalDocs", default)]
    pub external_docs: Option<ExternalDocs>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Server Object (H1). Multiple servers, server variables, and
/// 3.2's `name` field are all modeled.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Server {
    pub url: String,
    /// 3.2 §"Server Object" — server identifier for runtime selection (D8).
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub variables: Option<BTreeMap<String, ServerVariable>>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ServerVariable {
    /// REQUIRED in 3.0/3.1. In 3.2 this MAY be omitted when `enum` is present.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(rename = "enum", default)]
    pub enum_values: Option<Vec<String>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Discriminator {
    #[serde(rename = "propertyName")]
    pub property_name: String,
    #[serde(default)]
    pub mapping: Option<BTreeMap<String, String>>,
    /// 3.2 §"Discriminator Object" — fallback mapping target when the
    /// discriminator value is unknown (D9). Captured today; a future bead
    /// will emit a `_Other(Value)` enum variant when this is set.
    #[serde(rename = "defaultMapping", default)]
    pub default_mapping: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

impl Schema {
    /// Get the schema type if explicitly set. For `Schema::TypedMulti` the
    /// "primary" non-null type is returned; if the array contained only `null`
    /// then `Some(&SchemaType::Null)` is returned.
    pub fn schema_type(&self) -> Option<&SchemaType> {
        match self {
            Schema::Typed { schema_type, .. } => Some(schema_type),
            Schema::TypedMulti { schema_types, .. } => schema_types
                .iter()
                .find(|t| **t != SchemaType::Null)
                .or_else(|| schema_types.first()),
            _ => None,
        }
    }

    /// True when the schema's type set explicitly contains `null`.
    /// (3.1 canonical nullability via `type: ["X", "null"]`.)
    pub fn type_array_contains_null(&self) -> bool {
        match self {
            Schema::TypedMulti { schema_types, .. } => schema_types.contains(&SchemaType::Null),
            _ => false,
        }
    }

    /// Get schema details
    pub fn details(&self) -> &SchemaDetails {
        static EMPTY_DETAILS: Lazy<SchemaDetails> = Lazy::new(SchemaDetails::default);
        match self {
            Schema::Typed { details, .. } => details,
            Schema::TypedMulti { details, .. } => details,
            Schema::Reference { .. } | Schema::RecursiveRef { .. } | Schema::DynamicRef { .. } => {
                &EMPTY_DETAILS
            }
            Schema::OneOf { details, .. } => details,
            Schema::AnyOf { details, .. } => details,
            Schema::AllOf { details, .. } => details,
            Schema::Untyped { details } => details,
        }
    }

    /// Get mutable schema details
    pub fn details_mut(&mut self) -> &mut SchemaDetails {
        match self {
            Schema::Typed { details, .. } => details,
            Schema::TypedMulti { details, .. } => details,
            Schema::Reference { .. } => {
                panic!("Cannot get mutable details for reference schema")
            }
            Schema::RecursiveRef { .. } => {
                panic!("Cannot get mutable details for recursive reference schema")
            }
            Schema::DynamicRef { .. } => {
                panic!("Cannot get mutable details for dynamic reference schema")
            }
            Schema::OneOf { details, .. } => details,
            Schema::AnyOf { details, .. } => details,
            Schema::AllOf { details, .. } => details,
            Schema::Untyped { details } => details,
        }
    }

    /// Check if this is any kind of reference (regular or recursive)
    pub fn is_reference(&self) -> bool {
        matches!(self, Schema::Reference { .. } | Schema::RecursiveRef { .. })
    }

    /// Get reference string if this is a reference
    pub fn reference(&self) -> Option<&str> {
        match self {
            Schema::Reference { reference, .. } => Some(reference),
            _ => None,
        }
    }

    /// Get recursive reference string if this is a recursive reference
    pub fn recursive_reference(&self) -> Option<&str> {
        match self {
            Schema::RecursiveRef { recursive_ref, .. } => Some(recursive_ref),
            _ => None,
        }
    }

    /// Check if this is a discriminated union
    pub fn is_discriminated_union(&self) -> bool {
        match self {
            Schema::OneOf { discriminator, .. } => discriminator.is_some(),
            Schema::AnyOf { discriminator, .. } => discriminator.is_some(),
            _ => false,
        }
    }

    /// Get discriminator if this is a discriminated union
    pub fn discriminator(&self) -> Option<&Discriminator> {
        match self {
            Schema::OneOf { discriminator, .. } => discriminator.as_ref(),
            Schema::AnyOf { discriminator, .. } => discriminator.as_ref(),
            _ => None,
        }
    }

    /// Get union variants
    pub fn union_variants(&self) -> Option<&[Schema]> {
        match self {
            Schema::OneOf { one_of, .. } => Some(one_of),
            Schema::AnyOf { any_of, .. } => Some(any_of),
            _ => None,
        }
    }

    /// Check if this appears to be a nullable pattern (anyOf or oneOf with null)
    pub fn is_nullable_pattern(&self) -> bool {
        let variants = match self {
            Schema::AnyOf { any_of, .. } => any_of,
            Schema::OneOf { one_of, .. } => one_of,
            _ => return false,
        };
        variants.len() == 2
            && variants
                .iter()
                .any(|s| matches!(s.schema_type(), Some(SchemaType::Null)))
    }

    /// Get the non-null variant from a nullable pattern
    pub fn non_null_variant(&self) -> Option<&Schema> {
        if !self.is_nullable_pattern() {
            return None;
        }
        let variants = match self {
            Schema::AnyOf { any_of, .. } => any_of,
            Schema::OneOf { one_of, .. } => one_of,
            _ => return None,
        };
        variants
            .iter()
            .find(|s| !matches!(s.schema_type(), Some(SchemaType::Null)))
    }

    /// Infer schema type from structure if not explicitly set
    pub fn inferred_type(&self) -> Option<SchemaType> {
        match self {
            Schema::Typed { schema_type, .. } => Some(schema_type.clone()),
            Schema::TypedMulti { .. } => self.schema_type().cloned(),
            Schema::Untyped { details } => {
                // Infer from structure
                if details.properties.is_some() {
                    Some(SchemaType::Object)
                } else if details.items.is_some() {
                    Some(SchemaType::Array)
                } else if details.enum_values.is_some() {
                    Some(SchemaType::String) // Assume string enum
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl SchemaDetails {
    /// Check if this schema is nullable
    pub fn is_nullable(&self) -> bool {
        self.nullable.unwrap_or(false)
    }

    /// Check if this is a string enum
    ///
    /// A standalone string `const` (no `enum` array) is treated as a
    /// degenerate single-value enum so the generator emits a tightly-typed
    /// single-variant enum instead of a bare `String`. See issue #10.
    pub fn is_string_enum(&self) -> bool {
        self.enum_values.is_some() || self.const_string_value().is_some()
    }

    /// Get enum values as strings if this is a string enum.
    ///
    /// Falls back to `[const_value]` when `enum` is absent but `const` is a
    /// string, so a property like `{ "type": "string", "const": "X" }`
    /// produces a single-variant enum.
    pub fn string_enum_values(&self) -> Option<Vec<String>> {
        if let Some(values) = self.enum_values.as_ref() {
            // Tolerate non-string scalars in `enum` for `type: string` schemas
            // (gitpod has `enum: [2000, 5000, ...]` on a string-typed field).
            // Without this, `filter_map(.as_str())` produced an empty Vec
            // and we emitted an empty enum that fails to compile.
            return Some(
                values
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => n.to_string(),
                        Value::Bool(b) => b.to_string(),
                        Value::Null => "null".to_string(),
                        _ => v.to_string(),
                    })
                    .collect(),
            );
        }
        self.const_string_value().map(|s| vec![s])
    }

    fn const_string_value(&self) -> Option<String> {
        self.const_value
            .as_ref()
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Check if a field is required
    pub fn is_field_required(&self, field_name: &str) -> bool {
        self.required
            .as_ref()
            .map(|req| req.contains(&field_name.to_string()))
            .unwrap_or(false)
    }
}

/// OpenAPI Path Item Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct PathItem {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub get: Option<Operation>,
    pub put: Option<Operation>,
    pub post: Option<Operation>,
    pub delete: Option<Operation>,
    pub options: Option<Operation>,
    pub head: Option<Operation>,
    pub patch: Option<Operation>,
    pub trace: Option<Operation>,
    /// 3.2 §"Path Item Object" — `QUERY` HTTP method (D1). Originally
    /// proposed for safe, idempotent reads with a body.
    pub query: Option<Operation>,
    /// 3.2 §"Path Item Object" — extension map for HTTP methods beyond the
    /// well-known ones (e.g. WebDAV's PROPFIND, SEARCH; LINK/UNLINK). Keys
    /// are uppercase method names (D1).
    #[serde(rename = "additionalOperations", default)]
    pub additional_operations: Option<BTreeMap<String, Operation>>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(default)]
    pub servers: Option<Vec<Server>>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

impl PathItem {
    /// Get all operations in this path item, including 3.2's `query`
    /// (D1) and any custom verbs declared in `additionalOperations`.
    pub fn operations(&self) -> Vec<(&str, &Operation)> {
        let mut ops = Vec::new();
        if let Some(ref op) = self.get {
            ops.push(("get", op));
        }
        if let Some(ref op) = self.put {
            ops.push(("put", op));
        }
        if let Some(ref op) = self.post {
            ops.push(("post", op));
        }
        if let Some(ref op) = self.delete {
            ops.push(("delete", op));
        }
        if let Some(ref op) = self.options {
            ops.push(("options", op));
        }
        if let Some(ref op) = self.head {
            ops.push(("head", op));
        }
        if let Some(ref op) = self.patch {
            ops.push(("patch", op));
        }
        if let Some(ref op) = self.trace {
            ops.push(("trace", op));
        }
        if let Some(ref op) = self.query {
            ops.push(("query", op));
        }
        if let Some(map) = &self.additional_operations {
            for (verb, op) in map {
                ops.push((verb.as_str(), op));
            }
        }
        ops
    }
}

/// OpenAPI Operation Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Operation {
    #[serde(rename = "operationId", default)]
    pub operation_id: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(rename = "requestBody")]
    pub request_body: Option<RequestBody>,
    pub responses: Option<BTreeMap<String, Response>>,
    #[serde(default)]
    pub callbacks: Option<BTreeMap<String, Callback>>,
    #[serde(default)]
    pub security: Option<Vec<BTreeMap<String, Vec<String>>>>,
    #[serde(default)]
    pub servers: Option<Vec<Server>>,
    #[serde(rename = "externalDocs", default)]
    pub external_docs: Option<ExternalDocs>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Parameter Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Parameter {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(rename = "in", default)]
    pub location: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub deprecated: Option<bool>,
    #[serde(rename = "allowEmptyValue", default)]
    pub allow_empty_value: Option<bool>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub explode: Option<bool>,
    #[serde(rename = "allowReserved", default)]
    pub allow_reserved: Option<bool>,
    #[serde(default)]
    pub schema: Option<Schema>,
    #[serde(default)]
    pub content: Option<BTreeMap<String, MediaType>>,
    #[serde(default)]
    pub example: Option<Value>,
    #[serde(default)]
    pub examples: Option<BTreeMap<String, Example>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// OpenAPI Request Body Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestBody {
    pub content: Option<BTreeMap<String, MediaType>>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

/// Returns true for media types whose payload is JSON.
///
/// Matches `application/json` exactly, plus any RFC 6839 structured-syntax
/// suffix variant of the form `application/<subtype>+json`
/// (e.g. `application/vnd.api+json`, `application/hal+json`,
/// `application/problem+json`). Trailing parameters such as
/// `; charset=utf-8` are tolerated.
pub fn is_json_media_type(ct: &str) -> bool {
    let essence = ct
        .split(';')
        .next()
        .unwrap_or(ct)
        .trim()
        .to_ascii_lowercase();
    if essence == "application/json" {
        return true;
    }
    if let Some(subtype) = essence.strip_prefix("application/") {
        return subtype.ends_with("+json");
    }
    false
}

/// Returns true for `application/x-www-form-urlencoded` (with optional
/// parameters).
pub fn is_form_urlencoded_media_type(ct: &str) -> bool {
    let essence = ct
        .split(';')
        .next()
        .unwrap_or(ct)
        .trim()
        .to_ascii_lowercase();
    essence == "application/x-www-form-urlencoded"
}

/// Returns true only for the `text/event-stream` media type essence.
///
/// Media type names are ASCII-case-insensitive and parameters do not change
/// the essence, so values such as `Text/Event-Stream; charset=utf-8` match,
/// while similarly prefixed subtypes such as `text/event-streaming` do not.
pub fn is_event_stream_media_type(ct: &str) -> bool {
    ct.split(';')
        .next()
        .unwrap_or(ct)
        .trim()
        .eq_ignore_ascii_case("text/event-stream")
}

fn find_json_content(content: &BTreeMap<String, MediaType>) -> Option<(&str, &MediaType)> {
    if let Some(mt) = content.get("application/json") {
        return Some(("application/json", mt));
    }
    content
        .iter()
        .find(|(ct, _)| is_json_media_type(ct))
        .map(|(ct, mt)| (ct.as_str(), mt))
}

impl RequestBody {
    /// Get schema for any JSON content type
    ///
    /// Prefers the canonical `application/json` entry, then falls back to
    /// any `application/*+json` variant (RFC 6839) such as
    /// `application/vnd.api+json` or `application/hal+json`.
    pub fn json_schema(&self) -> Option<&Schema> {
        self.content
            .as_ref()
            .and_then(find_json_content)
            .and_then(|(_, media_type)| media_type.schema.as_ref())
    }

    /// Get the best content type and its schema, preferring JSON over others
    pub fn best_content(&self) -> Option<(&str, Option<&Schema>)> {
        let content = self.content.as_ref()?;

        if let Some((ct, media_type)) = find_json_content(content) {
            return Some((ct, media_type.schema.as_ref()));
        }

        const PRIORITY: &[&str] = &[
            "application/x-www-form-urlencoded",
            "multipart/form-data",
            "application/octet-stream",
            "text/plain",
        ];
        for ct in PRIORITY {
            if let Some(media_type) = content.get(*ct) {
                return Some((*ct, media_type.schema.as_ref()));
            }
        }
        None
    }
}

/// OpenAPI Response Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Response {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub headers: Option<BTreeMap<String, Header>>,
    #[serde(default)]
    pub content: Option<BTreeMap<String, MediaType>>,
    #[serde(default)]
    pub links: Option<Value>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

impl Response {
    /// Get schema for any JSON content type
    ///
    /// Prefers the canonical `application/json` entry, then falls back to
    /// any `application/*+json` variant (RFC 6839) such as
    /// `application/vnd.api+json`, `application/hal+json`, or
    /// `application/problem+json`.
    pub fn json_schema(&self) -> Option<&Schema> {
        self.content
            .as_ref()
            .and_then(find_json_content)
            .and_then(|(_, media_type)| media_type.schema.as_ref())
    }

    /// Get the preferred JSON-compatible media type and its schema.
    pub fn json_content(&self) -> Option<(&str, &Schema)> {
        self.content
            .as_ref()
            .and_then(find_json_content)
            .and_then(|(content_type, media_type)| {
                media_type
                    .schema
                    .as_ref()
                    .map(|schema| (content_type, schema))
            })
    }
}

/// OpenAPI Media Type Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MediaType {
    #[serde(default)]
    pub schema: Option<Schema>,
    #[serde(default)]
    pub example: Option<Value>,
    #[serde(default)]
    pub examples: Option<BTreeMap<String, Example>>,
    #[serde(default)]
    pub encoding: Option<BTreeMap<String, Encoding>>,
    /// 3.2 §"Media Type Object" — schema for each item when streaming
    /// (D3). Common in `text/event-stream` and JSON-lines payloads.
    #[serde(rename = "itemSchema", default)]
    pub item_schema: Option<Schema>,
    /// 3.2 §"Media Type Object" — encoding for the leading prefix of a
    /// streamed body (D3).
    #[serde(rename = "prefixEncoding", default)]
    pub prefix_encoding: Option<Vec<Encoding>>,
    /// 3.2 §"Media Type Object" — encoding applied to each streamed item
    /// (D3).
    #[serde(rename = "itemEncoding", default)]
    pub item_encoding: Option<Encoding>,
    #[serde(rename = "$ref", default)]
    pub reference: Option<String>,
    #[serde(flatten, default)]
    pub extensions: Extensions,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_parse_simple_object_schema() {
        let schema_json = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "User name"
                },
                "age": {
                    "type": "integer"
                }
            },
            "required": ["name"]
        });

        let schema: Schema = serde_json::from_value(schema_json).unwrap();

        match schema {
            Schema::Typed {
                schema_type: SchemaType::Object,
                details,
            } => {
                assert!(details.properties.is_some());
                assert_eq!(details.required, Some(vec!["name".to_string()]));
                assert!(details.is_field_required("name"));
                assert!(!details.is_field_required("age"));
            }
            _ => panic!("Expected object schema"),
        }
    }

    #[test]
    fn test_parse_string_enum() {
        let schema_json = json!({
            "type": "string",
            "enum": ["active", "inactive", "pending"],
            "description": "User status"
        });

        let schema: Schema = serde_json::from_value(schema_json).unwrap();

        match schema {
            Schema::Typed {
                schema_type: SchemaType::String,
                details,
            } => {
                assert!(details.is_string_enum());
                let values = details.string_enum_values().unwrap();
                assert_eq!(values, vec!["active", "inactive", "pending"]);
            }
            _ => panic!("Expected string enum schema"),
        }
    }

    #[test]
    fn test_parse_reference_schema() {
        let schema_json = json!({
            "$ref": "#/components/schemas/User"
        });

        let schema: Schema = serde_json::from_value(schema_json).unwrap();

        assert!(schema.is_reference());
        assert_eq!(schema.reference(), Some("#/components/schemas/User"));
    }

    #[test]
    fn test_parse_discriminated_union() {
        let schema_json = json!({
            "oneOf": [
                {"$ref": "#/components/schemas/Dog"},
                {"$ref": "#/components/schemas/Cat"}
            ],
            "discriminator": {
                "propertyName": "petType"
            }
        });

        let schema: Schema = serde_json::from_value(schema_json).unwrap();

        assert!(schema.is_discriminated_union());
        let discriminator = schema.discriminator().unwrap();
        assert_eq!(discriminator.property_name, "petType");
    }

    #[test]
    fn test_parse_nullable_pattern() {
        let schema_json = json!({
            "anyOf": [
                {"$ref": "#/components/schemas/User"},
                {"type": "null"}
            ]
        });

        let schema: Schema = serde_json::from_value(schema_json).unwrap();

        assert!(schema.is_nullable_pattern());
        let non_null = schema.non_null_variant().unwrap();
        assert!(non_null.is_reference());
    }

    #[test]
    fn is_json_media_type_accepts_canonical_and_structured_suffix() {
        // Canonical
        assert!(is_json_media_type("application/json"));
        // Parameters tolerated (RFC 7231 §3.1.1.1)
        assert!(is_json_media_type("application/json; charset=utf-8"));
        assert!(is_json_media_type("APPLICATION/JSON"));
        // RFC 6839 +json structured-syntax suffix
        assert!(is_json_media_type("application/vnd.api+json"));
        assert!(is_json_media_type("application/hal+json"));
        assert!(is_json_media_type("application/problem+json"));
        assert!(is_json_media_type("application/ld+json"));
        assert!(is_json_media_type(
            "application/vnd.api+json; charset=utf-8"
        ));
        // Negatives
        assert!(!is_json_media_type("application/xml"));
        assert!(!is_json_media_type("application/x-www-form-urlencoded"));
        assert!(!is_json_media_type("text/plain"));
        assert!(!is_json_media_type("application/jsonbutnotreally"));
        // +json suffix only applies to application/* per RFC 6839
        assert!(!is_json_media_type("text/something+json"));
    }

    #[test]
    fn request_body_json_schema_finds_vnd_api_plus_json() {
        // Mirrors Latitude.sh: request body declared under
        // application/vnd.api+json without a sibling application/json.
        let body_json = json!({
            "required": true,
            "content": {
                "application/vnd.api+json": {
                    "schema": {"$ref": "#/components/schemas/create_api_key"}
                }
            }
        });

        let body: RequestBody = serde_json::from_value(body_json).unwrap();
        let schema = body.json_schema().expect("expected +json schema match");
        assert!(schema.is_reference());
    }

    #[test]
    fn request_body_best_content_prefers_canonical_json_over_plus_json() {
        // When both are present (e.g. Latitude.sh's POST /auth/api_keys),
        // best_content should still pick application/json for backwards
        // compatibility with the existing snapshot suite.
        let body_json = json!({
            "required": true,
            "content": {
                "application/json": {
                    "schema": {"$ref": "#/components/schemas/A"}
                },
                "application/vnd.api+json": {
                    "schema": {"$ref": "#/components/schemas/B"}
                }
            }
        });

        let body: RequestBody = serde_json::from_value(body_json).unwrap();
        let (ct, _) = body.best_content().expect("expected best_content");
        assert_eq!(ct, "application/json");
    }

    #[test]
    fn request_body_best_content_falls_back_to_plus_json() {
        // When only the +json variant is declared, best_content returns
        // it instead of skipping straight to form-urlencoded.
        let body_json = json!({
            "required": true,
            "content": {
                "application/vnd.api+json": {
                    "schema": {"$ref": "#/components/schemas/B"}
                }
            }
        });

        let body: RequestBody = serde_json::from_value(body_json).unwrap();
        let (ct, _) = body.best_content().expect("expected best_content");
        assert_eq!(ct, "application/vnd.api+json");
    }

    #[test]
    fn response_json_schema_finds_vnd_api_plus_json() {
        // Mirrors every Latitude.sh response: schema lives under
        // application/vnd.api+json only.
        let resp_json = json!({
            "description": "OK",
            "content": {
                "application/vnd.api+json": {
                    "schema": {"$ref": "#/components/schemas/api_keys"}
                }
            }
        });

        let resp: Response = serde_json::from_value(resp_json).unwrap();
        let schema = resp.json_schema().expect("expected +json schema match");
        assert!(schema.is_reference());
    }
}
