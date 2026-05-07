//! Comprehensive end-to-end tests for full HTTP client generation
//!
//! These tests verify that the complete pipeline from OpenAPI spec to
//! working HTTP client code functions correctly.

use openapi_to_rust::{CodeGenerator, GeneratorConfig, analysis::SchemaAnalyzer};
use serde_json::json;
use std::path::PathBuf;

/// Helper to create a minimal OpenAPI spec
fn create_minimal_spec() -> serde_json::Value {
    json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "paths": {
            "/users": {
                "get": {
                    "operationId": "listUsers",
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "type": "array",
                                        "items": {
                                            "$ref": "#/components/schemas/User"
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                "post": {
                    "operationId": "createUser",
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/CreateUserRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "201": {
                            "description": "Created",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        }
                    }
                }
            },
            "/users/{id}": {
                "get": {
                    "operationId": "getUser",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "string"
                            }
                        }
                    ],
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        }
                    }
                },
                "put": {
                    "operationId": "updateUser",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "string"
                            }
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/UpdateUserRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        }
                    }
                },
                "delete": {
                    "operationId": "deleteUser",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "string"
                            }
                        }
                    ],
                    "responses": {
                        "204": {
                            "description": "No Content"
                        }
                    }
                }
            },
            "/users/{id}/profile": {
                "patch": {
                    "operationId": "patchUserProfile",
                    "parameters": [
                        {
                            "name": "id",
                            "in": "path",
                            "required": true,
                            "schema": {
                                "type": "string"
                            }
                        }
                    ],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": {
                                "schema": {
                                    "$ref": "#/components/schemas/PatchUserProfileRequest"
                                }
                            }
                        }
                    },
                    "responses": {
                        "200": {
                            "description": "Success",
                            "content": {
                                "application/json": {
                                    "schema": {
                                        "$ref": "#/components/schemas/User"
                                    }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "required": ["id", "name", "email"],
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string"
                        },
                        "age": {
                            "type": "integer"
                        }
                    }
                },
                "CreateUserRequest": {
                    "type": "object",
                    "required": ["name", "email"],
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string"
                        },
                        "age": {
                            "type": "integer"
                        }
                    }
                },
                "UpdateUserRequest": {
                    "type": "object",
                    "required": ["name", "email"],
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string"
                        },
                        "age": {
                            "type": "integer"
                        }
                    }
                },
                "PatchUserProfileRequest": {
                    "type": "object",
                    "properties": {
                        "name": {
                            "type": "string"
                        },
                        "age": {
                            "type": "integer"
                        }
                    }
                }
            }
        }
    })
}

#[test]
fn test_full_client_generation_minimal() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify the generated code has all the essential parts
    assert!(
        client_code.contains("pub enum HttpError"),
        "Should contain error types"
    );
    assert!(
        client_code.contains("pub struct HttpClient"),
        "Should contain HttpClient struct"
    );
    assert!(
        client_code.contains("impl HttpClient"),
        "Should contain HttpClient impl block"
    );
    assert!(
        client_code.contains("pub async fn list_users"),
        "Should contain list_users method"
    );
    assert!(
        client_code.contains("pub async fn create_user"),
        "Should contain create_user method"
    );
    assert!(
        client_code.contains("pub async fn get_user"),
        "Should contain get_user method"
    );
    assert!(
        client_code.contains("pub async fn update_user"),
        "Should contain update_user method"
    );
    assert!(
        client_code.contains("pub async fn delete_user"),
        "Should contain delete_user method"
    );
    assert!(
        client_code.contains("pub async fn patch_user_profile"),
        "Should contain patch_user_profile method"
    );
}

#[test]
fn test_full_client_generation_with_retry() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_to_rust::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: false,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify retry configuration is included
    assert!(
        client_code.contains("pub struct RetryConfig"),
        "Should include RetryConfig struct"
    );
    assert!(
        client_code.contains("RetryTransientMiddleware"),
        "Should include retry middleware"
    );
    assert!(
        client_code.contains("ExponentialBackoff"),
        "Should use exponential backoff"
    );

    // Verify the main client is still present
    assert!(
        client_code.contains("pub struct HttpClient"),
        "Should contain HttpClient struct"
    );
}

#[test]
fn test_full_client_generation_with_tracing() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: None,
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify tracing middleware is included
    assert!(
        client_code.contains("TracingMiddleware"),
        "Should include tracing middleware"
    );

    // Verify the main client is still present
    assert!(
        client_code.contains("pub struct HttpClient"),
        "Should contain HttpClient struct"
    );
}

#[test]
fn test_full_client_includes_errors() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify transport-error machinery is present
    assert!(
        client_code.contains("pub enum HttpError"),
        "Should include HttpError enum"
    );
    assert!(
        client_code.contains("Network(#[from] reqwest::Error)"),
        "Should include Network error variant"
    );
    assert!(
        client_code.contains("Serialization(String)"),
        "Should include Serialization error variant"
    );
    assert!(
        client_code.contains("pub type HttpResult<T>"),
        "Should include HttpResult type alias"
    );

    // Verify the new typed-response error envelope and operation result
    // wrapper are emitted (replaces the old `HttpError::Http { ... }`
    // variant — see issue #8 for the design).
    assert!(
        client_code.contains("pub struct ApiError"),
        "Should include ApiError envelope struct"
    );
    assert!(
        client_code.contains("pub enum ApiOpError"),
        "Should include ApiOpError enum"
    );
    assert!(
        client_code.contains("Transport(#[from] HttpError)"),
        "ApiOpError should have Transport variant carrying HttpError"
    );

    // Verify helper methods on the new envelope
    assert!(
        client_code.contains("pub fn is_client_error"),
        "Should include is_client_error helper"
    );
    assert!(
        client_code.contains("pub fn is_server_error"),
        "Should include is_server_error helper"
    );
    assert!(
        client_code.contains("pub fn is_retryable"),
        "Should include is_retryable helper"
    );
}

#[test]
fn test_full_client_includes_struct() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify struct definition
    assert!(
        client_code.contains("pub struct HttpClient"),
        "Should include HttpClient struct"
    );
    assert!(
        client_code.contains("base_url: String"),
        "Should have base_url field"
    );
    assert!(
        client_code.contains("api_key: Option<String>"),
        "Should have api_key field"
    );
    assert!(
        client_code.contains("http_client: ClientWithMiddleware"),
        "Should have http_client field with middleware"
    );
    assert!(
        client_code.contains("custom_headers: BTreeMap<String, String>"),
        "Should have custom_headers field"
    );

    // Verify constructor and builder methods
    assert!(
        client_code.contains("pub fn new()"),
        "Should have new() constructor"
    );
    assert!(
        client_code.contains("pub fn with_base_url"),
        "Should have with_base_url builder method"
    );
    assert!(
        client_code.contains("pub fn with_api_key"),
        "Should have with_api_key builder method"
    );
    assert!(
        client_code.contains("pub fn with_header"),
        "Should have with_header builder method"
    );
}

#[test]
fn test_full_client_includes_operations() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify operation methods are generated
    assert!(
        client_code.contains("pub async fn list_users"),
        "Should have list_users operation"
    );
    assert!(
        client_code.contains("pub async fn create_user"),
        "Should have create_user operation"
    );
    assert!(
        client_code.contains("pub async fn get_user"),
        "Should have get_user operation"
    );
    assert!(
        client_code.contains("pub async fn update_user"),
        "Should have update_user operation"
    );
    assert!(
        client_code.contains("pub async fn delete_user"),
        "Should have delete_user operation"
    );
    assert!(
        client_code.contains("pub async fn patch_user_profile"),
        "Should have patch_user_profile operation"
    );
}

#[test]
fn test_all_http_methods_supported() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify all HTTP methods are represented
    assert!(
        client_code.contains(".get(request_url)"),
        "Should have GET method calls"
    );
    assert!(
        client_code.contains(".post(request_url)"),
        "Should have POST method calls"
    );
    assert!(
        client_code.contains(".put(request_url)"),
        "Should have PUT method calls"
    );
    assert!(
        client_code.contains(".delete(request_url)"),
        "Should have DELETE method calls"
    );
    assert!(
        client_code.contains(".patch(request_url)"),
        "Should have PATCH method calls"
    );
}

#[test]
fn test_generated_client_has_proper_imports() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify essential imports are present
    assert!(
        client_code.contains("use super::types::*"),
        "Should import generated types"
    );
    assert!(
        client_code.contains("use thiserror::Error"),
        "Should import thiserror for error types"
    );
}

#[test]
fn test_generated_code_parses_as_valid_rust() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        retry_config: Some(openapi_to_rust::http_config::RetryConfig {
            max_retries: 3,
            initial_delay_ms: 500,
            max_delay_ms: 16000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Try to parse as Rust code
    let parse_result = syn::parse_file(&client_code);
    assert!(
        parse_result.is_ok(),
        "Generated code should parse as valid Rust: {:?}",
        parse_result.err()
    );
}

#[test]
fn test_full_pipeline_with_all_features() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        enable_sse_client: false,
        enable_specta: false,
        retry_config: Some(openapi_to_rust::http_config::RetryConfig {
            max_retries: 5,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
        }),
        tracing_enabled: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);

    // Test full generation pipeline
    let result = generator
        .generate_all(&mut analysis)
        .expect("Failed to generate all files");

    // Verify we got the expected files
    assert!(
        result
            .files
            .iter()
            .any(|f| f.path.to_str() == Some("types.rs")),
        "Should generate types.rs"
    );
    assert!(
        result
            .files
            .iter()
            .any(|f| f.path.to_str() == Some("client.rs")),
        "Should generate client.rs"
    );

    // Verify mod.rs exports both modules
    let mod_content = &result.mod_file.content;
    assert!(
        mod_content.contains("pub mod types"),
        "mod.rs should export types module"
    );
    assert!(
        mod_content.contains("pub mod client"),
        "mod.rs should export client module"
    );

    // Get the client file and verify its contents
    let client_file = result
        .files
        .iter()
        .find(|f| f.path.to_str() == Some("client.rs"))
        .expect("Should have client.rs file");

    let client_code = &client_file.content;

    // Verify all major components are present
    assert!(
        client_code.contains("pub enum HttpError"),
        "Should have error types"
    );
    assert!(
        client_code.contains("pub struct HttpClient"),
        "Should have client struct"
    );
    assert!(
        client_code.contains("pub struct RetryConfig"),
        "Should have retry config"
    );
    assert!(
        client_code.contains("TracingMiddleware"),
        "Should have tracing middleware"
    );
    assert!(
        client_code.contains("pub async fn"),
        "Should have async operation methods"
    );
}

#[test]
fn test_error_handling_methods_present() {
    let spec = create_minimal_spec();
    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let analysis = analyzer.analyze().expect("Failed to analyze spec");

    let config = GeneratorConfig {
        spec_path: PathBuf::from("test.json"),
        output_dir: PathBuf::from("test_output"),
        module_name: "test".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let client_code = generator
        .generate_http_client(&analysis)
        .expect("Failed to generate HTTP client");

    // Verify error handling helpers
    assert!(
        client_code.contains("pub fn is_client_error(&self) -> bool"),
        "Should have is_client_error method"
    );
    assert!(
        client_code.contains("pub fn is_server_error(&self) -> bool"),
        "Should have is_server_error method"
    );
    assert!(
        client_code.contains("pub fn is_retryable(&self) -> bool"),
        "Should have is_retryable method"
    );
}
