use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    pub paths: Option<BTreeMap<String, PathItem>>,
    pub components: Option<Components>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Info {
    pub title: String,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Components {
    pub schemas: Option<BTreeMap<String, Schema>>,
    pub parameters: Option<BTreeMap<String, Parameter>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
    /// Recursive reference (OpenAPI 3.1)
    RecursiveRef {
        #[serde(rename = "$recursiveRef")]
        recursive_ref: String,
        #[serde(flatten)]
        extra: BTreeMap<String, Value>,
    },
    /// OneOf union
    OneOf {
        #[serde(rename = "oneOf")]
        one_of: Vec<Schema>,
        discriminator: Option<Discriminator>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// AnyOf union (must come before Typed to handle type + anyOf patterns)
    AnyOf {
        #[serde(rename = "type")]
        schema_type: Option<SchemaType>,
        #[serde(rename = "anyOf")]
        any_of: Vec<Schema>,
        discriminator: Option<Discriminator>,
        #[serde(flatten)]
        details: SchemaDetails,
    },
    /// Schema with explicit type
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SchemaDetails {
    pub description: Option<String>,
    pub nullable: Option<bool>,

    // OpenAPI 3.1 recursive support
    #[serde(rename = "$recursiveAnchor")]
    pub recursive_anchor: Option<bool>,

    // String-specific
    #[serde(rename = "enum")]
    pub enum_values: Option<Vec<Value>>,
    pub format: Option<String>,
    pub default: Option<Value>,
    #[serde(rename = "const")]
    pub const_value: Option<Value>,

    // Object-specific
    pub properties: Option<BTreeMap<String, Schema>>,
    pub required: Option<Vec<String>>,
    #[serde(rename = "additionalProperties")]
    pub additional_properties: Option<AdditionalProperties>,

    // Array-specific
    pub items: Option<Box<Schema>>,

    // Number-specific
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,

    // Validation
    #[serde(rename = "minLength")]
    pub min_length: Option<u64>,
    #[serde(rename = "maxLength")]
    pub max_length: Option<u64>,
    pub pattern: Option<String>,

    // Extensions and unknown fields
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum AdditionalProperties {
    Boolean(bool),
    Schema(Box<Schema>),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Discriminator {
    #[serde(rename = "propertyName")]
    pub property_name: String,
    pub mapping: Option<BTreeMap<String, String>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Schema {
    /// Get the schema type if explicitly set
    pub fn schema_type(&self) -> Option<&SchemaType> {
        match self {
            Schema::Typed { schema_type, .. } => Some(schema_type),
            _ => None,
        }
    }

    /// Get schema details
    pub fn details(&self) -> &SchemaDetails {
        match self {
            Schema::Typed { details, .. } => details,
            Schema::Reference { .. } => {
                static EMPTY_DETAILS: Lazy<SchemaDetails> = Lazy::new(|| SchemaDetails {
                    description: None,
                    nullable: None,
                    recursive_anchor: None,
                    enum_values: None,
                    format: None,
                    default: None,
                    const_value: None,
                    properties: None,
                    required: None,
                    additional_properties: None,
                    items: None,
                    minimum: None,
                    maximum: None,
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    extra: BTreeMap::new(),
                });
                &EMPTY_DETAILS
            }
            Schema::RecursiveRef { .. } => {
                static EMPTY_DETAILS_RECURSIVE: Lazy<SchemaDetails> = Lazy::new(|| SchemaDetails {
                    description: None,
                    nullable: None,
                    recursive_anchor: None,
                    enum_values: None,
                    format: None,
                    default: None,
                    const_value: None,
                    properties: None,
                    required: None,
                    additional_properties: None,
                    items: None,
                    minimum: None,
                    maximum: None,
                    min_length: None,
                    max_length: None,
                    pattern: None,
                    extra: BTreeMap::new(),
                });
                &EMPTY_DETAILS_RECURSIVE
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
            Schema::Reference { .. } => {
                // Cannot mutate reference details
                panic!("Cannot get mutable details for reference schema")
            }
            Schema::RecursiveRef { .. } => {
                // Cannot mutate recursive reference details
                panic!("Cannot get mutable details for recursive reference schema")
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

    /// Check if this appears to be a nullable pattern (anyOf with null)
    pub fn is_nullable_pattern(&self) -> bool {
        match self {
            Schema::AnyOf { any_of, .. } => {
                any_of.len() == 2
                    && any_of
                        .iter()
                        .any(|s| matches!(s.schema_type(), Some(SchemaType::Null)))
            }
            _ => false,
        }
    }

    /// Get the non-null variant from a nullable pattern
    pub fn non_null_variant(&self) -> Option<&Schema> {
        if self.is_nullable_pattern() {
            if let Schema::AnyOf { any_of, .. } = self {
                return any_of
                    .iter()
                    .find(|s| !matches!(s.schema_type(), Some(SchemaType::Null)));
            }
        }
        None
    }

    /// Infer schema type from structure if not explicitly set
    pub fn inferred_type(&self) -> Option<SchemaType> {
        match self {
            Schema::Typed { schema_type, .. } => Some(schema_type.clone()),
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
    pub fn is_string_enum(&self) -> bool {
        self.enum_values.is_some()
    }

    /// Get enum values as strings if this is a string enum
    pub fn string_enum_values(&self) -> Option<Vec<String>> {
        self.enum_values.as_ref().map(|values| {
            values
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect()
        })
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
    #[serde(rename = "get")]
    pub get: Option<Operation>,
    #[serde(rename = "put")]
    pub put: Option<Operation>,
    #[serde(rename = "post")]
    pub post: Option<Operation>,
    #[serde(rename = "delete")]
    pub delete: Option<Operation>,
    #[serde(rename = "options")]
    pub options: Option<Operation>,
    #[serde(rename = "head")]
    pub head: Option<Operation>,
    #[serde(rename = "patch")]
    pub patch: Option<Operation>,
    #[serde(rename = "trace")]
    pub trace: Option<Operation>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl PathItem {
    /// Get all operations in this path item
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
        ops
    }
}

/// OpenAPI Operation Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Operation {
    #[serde(rename = "operationId")]
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(rename = "requestBody")]
    pub request_body: Option<RequestBody>,
    pub responses: Option<BTreeMap<String, Response>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// OpenAPI Parameter Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Parameter {
    pub name: Option<String>,
    #[serde(rename = "in")]
    pub location: Option<String>,
    pub required: Option<bool>,
    pub schema: Option<Schema>,
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// OpenAPI Request Body Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RequestBody {
    pub content: Option<BTreeMap<String, MediaType>>,
    pub description: Option<String>,
    pub required: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl RequestBody {
    /// Get schema for application/json content type
    pub fn json_schema(&self) -> Option<&Schema> {
        self.content
            .as_ref()
            .and_then(|content| content.get("application/json"))
            .and_then(|media_type| media_type.schema.as_ref())
    }

    /// Get the best content type and its schema, preferring JSON over others
    pub fn best_content(&self) -> Option<(&str, Option<&Schema>)> {
        let content = self.content.as_ref()?;
        const PRIORITY: &[&str] = &[
            "application/json",
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
    pub description: Option<String>,
    pub content: Option<BTreeMap<String, MediaType>>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Response {
    /// Get schema for application/json content type
    pub fn json_schema(&self) -> Option<&Schema> {
        self.content
            .as_ref()
            .and_then(|content| content.get("application/json"))
            .and_then(|media_type| media_type.schema.as_ref())
    }
}

/// OpenAPI Media Type Object
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MediaType {
    pub schema: Option<Schema>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
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
}
