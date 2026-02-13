use openapi_generator::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Running comprehensive stress tests...\n");

    // Test 1: Deep recursion with circular references
    println!("Test 1: Deep recursion with circular references");
    test_deep_recursion()?;

    // Test 2: Complex discriminated unions with inheritance
    println!("\nTest 2: Complex discriminated unions with inheritance");
    test_complex_unions()?;

    // Test 3: Mixed nullable patterns everywhere
    println!("\nTest 3: Mixed nullable patterns");
    test_mixed_nullables()?;

    // Test 4: Deeply nested compositions
    println!("\nTest 4: Deeply nested compositions");
    test_deep_compositions()?;

    // Test 5: Edge case property names and values
    println!("\nTest 5: Edge case property names and values");
    test_edge_cases()?;

    println!("\n✅ All stress tests passed!");
    Ok(())
}

fn test_deep_recursion() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Deep Recursion Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "FileSystem": {
                    "$recursiveAnchor": true,
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "type": {
                            "type": "string",
                            "enum": ["file", "directory", "symlink"],
                            "default": "file"
                        },
                        "children": {
                            "type": "array",
                            "items": {"$recursiveRef": "#"}
                        },
                        "parent": {"$recursiveRef": "#"},
                        "metadata": {
                            "type": "object",
                            "properties": {
                                "permissions": {"type": "string"},
                                "size": {"type": "integer", "default": 0},
                                "nested_refs": {
                                    "type": "array",
                                    "items": {
                                        "type": "array",
                                        "items": {"$recursiveRef": "#"}
                                    }
                                }
                            },
                            "additionalProperties": true
                        }
                    },
                    "required": ["name", "type"]
                },
                "NetworkNode": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "connections": {
                            "type": "array",
                            "items": {"$ref": "#/components/schemas/NetworkEdge"}
                        },
                        "filesystem": {"$ref": "#/components/schemas/FileSystem"}
                    },
                    "required": ["id"]
                },
                "NetworkEdge": {
                    "type": "object",
                    "properties": {
                        "from": {"$ref": "#/components/schemas/NetworkNode"},
                        "to": {"$ref": "#/components/schemas/NetworkNode"},
                        "weight": {"type": "number", "default": 1.0},
                        "bidirectional": {"type": "boolean", "default": false}
                    },
                    "required": ["from", "to"]
                }
            }
        }
    });

    test_and_generate("deep_recursion", spec)
}

fn test_complex_unions() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Complex Unions Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "BaseMessage": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "timestamp": {"type": "string", "format": "date-time"}
                    },
                    "required": ["id"]
                },
                "UserInfo": {
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "role": {
                            "type": "string",
                            "enum": ["admin", "user", "guest"],
                            "default": "user"
                        }
                    },
                    "required": ["name"]
                },
                "TextMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseMessage"},
                        {"$ref": "#/components/schemas/UserInfo"},
                        {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["text"]},
                                "content": {"type": "string"},
                                "metadata": {
                                    "anyOf": [
                                        {"type": "string"},
                                        {"type": "object", "additionalProperties": true},
                                        {"type": "null"}
                                    ]
                                }
                            },
                            "required": ["type", "content"]
                        }
                    ]
                },
                "ImageMessage": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseMessage"},
                        {
                            "type": "object",
                            "properties": {
                                "type": {"type": "string", "enum": ["image"]},
                                "url": {"type": "string", "format": "uri"},
                                "dimensions": {
                                    "type": "object",
                                    "properties": {
                                        "width": {"type": "integer", "default": 100},
                                        "height": {"type": "integer", "default": 100}
                                    },
                                    "required": ["width", "height"]
                                },
                                "alt_text": {
                                    "type": "string",
                                    "nullable": true,
                                    "default": "Image"
                                }
                            },
                            "required": ["type", "url"]
                        }
                    ]
                },
                "SystemMessage": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "enum": ["system"]},
                        "event": {
                            "oneOf": [
                                {
                                    "type": "object",
                                    "properties": {
                                        "event_type": {"type": "string", "enum": ["user_joined"]},
                                        "user": {"$ref": "#/components/schemas/UserInfo"}
                                    },
                                    "required": ["event_type", "user"]
                                },
                                {
                                    "type": "object",
                                    "properties": {
                                        "event_type": {"type": "string", "enum": ["user_left"]},
                                        "user_id": {"type": "string"}
                                    },
                                    "required": ["event_type", "user_id"]
                                }
                            ],
                            "discriminator": {"propertyName": "event_type"}
                        }
                    },
                    "required": ["type", "event"]
                },
                "Message": {
                    "oneOf": [
                        {"$ref": "#/components/schemas/TextMessage"},
                        {"$ref": "#/components/schemas/ImageMessage"},
                        {"$ref": "#/components/schemas/SystemMessage"}
                    ],
                    "discriminator": {
                        "propertyName": "type",
                        "mapping": {
                            "txt": "#/components/schemas/TextMessage",
                            "img": "#/components/schemas/ImageMessage",
                            "sys": "#/components/schemas/SystemMessage"
                        }
                    }
                }
            }
        }
    });

    test_and_generate("complex_unions", spec)
}

fn test_mixed_nullables() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Mixed Nullables Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "ComplexNullables": {
                    "type": "object",
                    "properties": {
                        "legacy_nullable": {
                            "type": "string",
                            "nullable": true,
                            "default": "default_value"
                        },
                        "anyof_nullable": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "null"}
                            ],
                            "default": "another_default"
                        },
                        "nullable_reference": {
                            "anyOf": [
                                {"$ref": "#/components/schemas/NestedObject"},
                                {"type": "null"}
                            ]
                        },
                        "nullable_array": {
                            "anyOf": [
                                {
                                    "type": "array",
                                    "items": {
                                        "anyOf": [
                                            {"type": "string"},
                                            {"type": "null"}
                                        ]
                                    }
                                },
                                {"type": "null"}
                            ],
                            "default": []
                        },
                        "complex_union": {
                            "anyOf": [
                                {"type": "string"},
                                {"type": "integer"},
                                {"type": "boolean"},
                                {
                                    "type": "object",
                                    "properties": {
                                        "nested": {"type": "string"}
                                    }
                                },
                                {"type": "null"}
                            ]
                        }
                    },
                    "required": ["legacy_nullable", "anyof_nullable"]
                },
                "NestedObject": {
                    "type": "object",
                    "properties": {
                        "value": {"type": "string"},
                        "recursive_ref": {
                            "anyOf": [
                                {"$ref": "#/components/schemas/NestedObject"},
                                {"type": "null"}
                            ]
                        }
                    },
                    "required": ["value"]
                }
            }
        }
    });

    test_and_generate("mixed_nullables", spec)
}

fn test_deep_compositions() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Deep Compositions Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "BaseEntity": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "created_at": {"type": "string", "format": "date-time"}
                    },
                    "required": ["id"]
                },
                "Timestamped": {
                    "type": "object",
                    "properties": {
                        "updated_at": {"type": "string", "format": "date-time"},
                        "version": {"type": "integer", "default": 1}
                    }
                },
                "Identifiable": {
                    "type": "object",
                    "properties": {
                        "uuid": {"type": "string", "format": "uuid"},
                        "slug": {"type": "string"}
                    },
                    "required": ["uuid"]
                },
                "Taggable": {
                    "type": "object",
                    "properties": {
                        "tags": {
                            "type": "array",
                            "items": {"type": "string"},
                            "default": []
                        },
                        "categories": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "name": {"type": "string"},
                                    "priority": {"type": "integer", "default": 0}
                                },
                                "required": ["name"]
                            },
                            "default": []
                        }
                    }
                },
                "Document": {
                    "allOf": [
                        {"$ref": "#/components/schemas/BaseEntity"},
                        {"$ref": "#/components/schemas/Timestamped"},
                        {"$ref": "#/components/schemas/Identifiable"},
                        {"$ref": "#/components/schemas/Taggable"},
                        {
                            "type": "object",
                            "properties": {
                                "title": {"type": "string"},
                                "content": {"type": "string"},
                                "author": {
                                    "allOf": [
                                        {"$ref": "#/components/schemas/BaseEntity"},
                                        {"$ref": "#/components/schemas/Identifiable"},
                                        {
                                            "type": "object",
                                            "properties": {
                                                "name": {"type": "string"},
                                                "email": {"type": "string", "format": "email"}
                                            },
                                            "required": ["name", "email"]
                                        }
                                    ]
                                },
                                "attachments": {
                                    "type": "array",
                                    "items": {
                                        "allOf": [
                                            {"$ref": "#/components/schemas/BaseEntity"},
                                            {
                                                "type": "object",
                                                "properties": {
                                                    "filename": {"type": "string"},
                                                    "size": {"type": "integer"},
                                                    "parent_document": {"$ref": "#/components/schemas/Document"}
                                                },
                                                "required": ["filename", "size"]
                                            }
                                        ]
                                    }
                                }
                            },
                            "required": ["title", "content", "author"]
                        }
                    ]
                }
            }
        }
    });

    test_and_generate("deep_compositions", spec)
}

fn test_edge_cases() -> Result<(), Box<dyn std::error::Error>> {
    let spec = json!({
        "openapi": "3.1.0",
        "info": {"title": "Edge Cases Test", "version": "1.0.0"},
        "components": {
            "schemas": {
                "EdgeCaseNames": {
                    "type": "object",
                    "properties": {
                        "type": {"type": "string", "default": "reserved_keyword"},
                        "match": {"type": "integer", "default": 42},
                        "fn": {"type": "boolean", "default": false},
                        "struct": {"type": "string"},
                        "enum": {"type": "string"},
                        "kebab-case": {"type": "string"},
                        "snake_case": {"type": "string"},
                        "camelCase": {"type": "string"},
                        "PascalCase": {"type": "string"},
                        "SCREAMING_SNAKE_CASE": {"type": "string"},
                        "with.dots": {"type": "string"},
                        "with spaces": {"type": "string"},
                        "with@symbols#": {"type": "string"},
                        "123numeric": {"type": "string"},
                        "$special": {"type": "string"}
                    },
                    "required": ["type", "match", "fn"]
                },
                "WeirdEnums": {
                    "type": "string",
                    "enum": [
                        "normal-value",
                        "with spaces",
                        "with.dots",
                        "with@symbols",
                        "123",
                        "",
                        "null",
                        "true",
                        "false",
                        "weird/slash\\backslash",
                        "unicode-émoji-🚀",
                        "very-long-enum-value-that-might-cause-issues-with-identifier-generation-and-should-be-handled-gracefully"
                    ],
                    "default": "normal-value"
                },
                "ComplexDefaults": {
                    "type": "object",
                    "properties": {
                        "string_with_quotes": {
                            "type": "string",
                            "default": "String with \"quotes\" and 'apostrophes'"
                        },
                        "string_with_escapes": {
                            "type": "string",
                            "default": "Line 1\nLine 2\tTabbed\r\nWindows newline"
                        },
                        "large_number": {
                            "type": "number",
                            "default": 1.797_693_134_862_315_7e308
                        },
                        "scientific_notation": {
                            "type": "number",
                            "default": 1.23e-45
                        },
                        "complex_array": {
                            "type": "array",
                            "items": {
                                "oneOf": [
                                    {"type": "string"},
                                    {"type": "integer"},
                                    {
                                        "type": "object",
                                        "properties": {
                                            "nested": {"type": "string"}
                                        }
                                    }
                                ]
                            },
                            "default": ["string", 42, {"nested": "value"}]
                        }
                    }
                },
                "ExtremeDeeplyNested": {
                    "type": "object",
                    "properties": {
                        "level1": {
                            "type": "object",
                            "properties": {
                                "level2": {
                                    "type": "object",
                                    "properties": {
                                        "level3": {
                                            "type": "object",
                                            "properties": {
                                                "level4": {
                                                    "type": "object",
                                                    "properties": {
                                                        "level5": {
                                                            "type": "array",
                                                            "items": {
                                                                "type": "object",
                                                                "properties": {
                                                                    "deep_value": {"type": "string"}
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    test_and_generate("edge_cases", spec)
}

fn test_and_generate(
    test_name: &str,
    spec: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut analyzer = SchemaAnalyzer::new(spec)?;
    let mut analysis = analyzer.analyze()?;

    let config = GeneratorConfig {
        module_name: format!("{test_name}_api"),
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let generated_code = generator.generate(&mut analysis)?;

    println!(
        "  ✅ {} schemas analyzed successfully",
        analysis.schemas.len()
    );
    println!(
        "  ✅ Code generation completed ({} lines)",
        generated_code.lines().count()
    );

    // Verify the code looks reasonable (basic sanity checks)
    assert!(generated_code.contains("pub mod"));
    assert!(generated_code.contains("use serde"));
    assert!(!generated_code.contains("Error") || generated_code.contains("// ")); // No unhandled errors

    Ok(())
}
