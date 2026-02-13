use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

#[test]
fn test_specta_derives_enabled() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "name": {
                            "type": "string"
                        },
                        "email": {
                            "type": "string"
                        }
                    },
                    "required": ["id", "name"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify Specta derives are present
    assert!(result.contains("#[cfg_attr(feature = \"specta\", derive(specta::Type))]"));

    // Note: We use snake_case everywhere (matching the OpenAPI spec) for consistency
    // between Rust, JSON API, and TypeScript - no rename_all needed
    assert!(!result.contains("rename_all"));

    // Verify struct is still properly generated
    assert!(result.contains("pub struct User"));
    assert!(result.contains("pub id: String"));
    assert!(result.contains("pub name: String"));
    assert!(result.contains("pub email: Option<String>"));
}

#[test]
fn test_specta_derives_disabled() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "User": {
                    "type": "object",
                    "properties": {
                        "id": {
                            "type": "string"
                        },
                        "name": {
                            "type": "string"
                        }
                    },
                    "required": ["id", "name"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: false,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify NO Specta derives are present
    assert!(!result.contains("specta::Type"));
    assert!(!result.contains("specta(rename_all"));

    // Verify struct is still properly generated
    assert!(result.contains("pub struct User"));
    assert!(result.contains("#[derive(Debug, Clone, Deserialize, Serialize)]"));
}

#[test]
fn test_specta_enum_derives() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Status": {
                    "type": "string",
                    "enum": ["active", "inactive", "pending"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify Specta derives are present for enums
    assert!(result.contains("#[cfg_attr(feature = \"specta\", derive(specta::Type))]"));

    // Verify enum is properly generated
    assert!(result.contains("pub enum Status"));
    assert!(result.contains("Active"));
    assert!(result.contains("Inactive"));
    assert!(result.contains("Pending"));
}

#[test]
fn test_specta_discriminated_union() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "Shape": {
                    "oneOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "const": "circle"
                                },
                                "radius": {
                                    "type": "number"
                                }
                            },
                            "required": ["type", "radius"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": {
                                    "type": "string",
                                    "const": "rectangle"
                                },
                                "width": {
                                    "type": "number"
                                },
                                "height": {
                                    "type": "number"
                                }
                            },
                            "required": ["type", "width", "height"]
                        }
                    ],
                    "discriminator": {
                        "propertyName": "type"
                    }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify Specta derives are present
    assert!(result.contains("#[cfg_attr(feature = \"specta\", derive(specta::Type))]"));

    // Verify discriminated union is properly generated
    assert!(result.contains("pub enum Shape"));
}

#[test]
fn test_specta_rename_all() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "UserProfile": {
                    "type": "object",
                    "properties": {
                        "userId": {
                            "type": "string"
                        },
                        "firstName": {
                            "type": "string"
                        },
                        "lastName": {
                            "type": "string"
                        }
                    },
                    "required": ["userId"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify Specta derives are present
    assert!(result.contains("#[cfg_attr(feature = \"specta\", derive(specta::Type))]"));

    // Note: We use snake_case everywhere (matching the OpenAPI spec) for consistency
    // between Rust, JSON API, and TypeScript - no rename_all needed
    assert!(!result.contains("rename_all"));

    // Verify struct is properly generated with snake_case fields
    assert!(result.contains("pub struct UserProfile"));
    // Fields should use snake_case to match OpenAPI spec
    assert!(result.contains("pub user_id: String"));
    assert!(result.contains("pub first_name: Option<String>"));
    assert!(result.contains("pub last_name: Option<String>"));
}

#[test]
fn test_specta_camel_case_rename() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "UserData": {
                    "type": "object",
                    "properties": {
                        "user_id": {
                            "type": "string"
                        },
                        "created_at": {
                            "type": "string"
                        },
                        "is_active": {
                            "type": "boolean"
                        },
                        "profile_image_url": {
                            "type": "string"
                        }
                    },
                    "required": ["user_id", "created_at", "is_active"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify Specta derives are present
    assert!(result.contains("#[cfg_attr(feature = \"specta\", derive(specta::Type))]"));

    // Verify struct has snake_case field names (Rust convention)
    assert!(result.contains("pub user_id: String"));
    assert!(result.contains("pub created_at: String"));
    assert!(result.contains("pub is_active: bool"));
    assert!(result.contains("pub profile_image_url: Option<String>"));

    // Since JSON field names are already snake_case and match Rust field names,
    // serde rename is NOT added (it's only added when they differ)

    // Verify specta renames to camelCase for TypeScript
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"userId\"))]"));
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"createdAt\"))]"));
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"isActive\"))]"));
    assert!(
        result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"profileImageUrl\"))]")
    );
}

#[test]
fn test_specta_camel_case_with_mixed_casing() {
    let spec = json!({
        "openapi": "3.0.0",
        "info": {
            "title": "Test API",
            "version": "1.0.0"
        },
        "components": {
            "schemas": {
                "ApiResponse": {
                    "type": "object",
                    "properties": {
                        "api_key": {
                            "type": "string"
                        },
                        "APIVersion": {
                            "type": "string"
                        },
                        "some_URL": {
                            "type": "string"
                        }
                    },
                    "required": ["api_key"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("Failed to create analyzer");
    let mut analysis = analyzer.analyze().expect("Failed to analyze schema");

    let config = GeneratorConfig {
        enable_specta: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator
        .generate(&mut analysis)
        .expect("Failed to generate code");

    // Verify specta renames handle mixed casing correctly
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"apiKey\"))]"));
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"apiversion\"))]"));
    assert!(result.contains("#[cfg_attr(feature = \"specta\", specta(rename = \"someUrl\"))]"));
}
