use openapi_to_rust::analysis::SchemaType as AnalyzedSchemaType;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;

#[test]
fn distinct_component_keys_that_map_to_the_same_rust_type_are_disambiguated() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "name-collision-poc", "version": "1.0.0" },
        "paths": {
            "/event": {
                "get": {
                    "operationId": "getEvent",
                    "responses": {
                        "200": {
                            "description": "one event",
                            "content": {
                                "application/json": {
                                    "schema": { "$ref": "#/components/schemas/session.status" }
                                }
                            }
                        }
                    }
                }
            }
        },
        "components": {
            "schemas": {
                "SessionStatus": {
                    "anyOf": [
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["idle"] }
                            },
                            "required": ["type"]
                        },
                        {
                            "type": "object",
                            "properties": {
                                "type": { "type": "string", "enum": ["busy"] },
                                "detail": { "type": "string" }
                            },
                            "required": ["type"]
                        }
                    ]
                },
                "session.status": {
                    "type": "object",
                    "properties": {
                        "id": { "type": "string" },
                        "data": {
                            "type": "object",
                            "properties": {
                                "sessionID": { "type": "string" },
                                "status": { "$ref": "#/components/schemas/SessionStatus" }
                            },
                            "required": ["sessionID", "status"]
                        }
                    },
                    "required": ["id", "data"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("spec should parse");
    let mut analysis = analyzer.analyze().expect("spec should analyze");
    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("colliding component schema names should be disambiguated");

    assert!(generated.contains("pub enum SessionStatus"));
    assert!(generated.contains("pub struct SessionStatus2"));
    assert!(generated.contains("pub status: SessionStatus"));
    assert!(analysis.schemas.contains_key("SessionStatus"));
    assert!(analysis.schemas.contains_key("SessionStatus2"));
    assert!(!analysis.schemas.contains_key("session.status"));
    assert_eq!(
        analysis.operations["getEvent"].response_schemas["200"],
        "SessionStatus2"
    );
}

#[test]
fn inline_objects_do_not_replace_exact_named_components() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "inline-object-component-collision", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "Model": {
                    "type": "object",
                    "properties": {
                        "api": {
                            "type": "object",
                            "properties": { "endpoint": { "type": "string" } },
                            "required": ["endpoint"]
                        },
                        "capabilities": {
                            "type": "object",
                            "properties": { "streaming": { "type": "boolean" } },
                            "required": ["streaming"]
                        }
                    },
                    "required": ["api", "capabilities"]
                },
                "ModelApi": {
                    "type": "object",
                    "properties": { "component_api": { "type": "string" } },
                    "required": ["component_api"]
                },
                "ModelCapabilities": {
                    "type": "object",
                    "properties": {
                        "input": { "type": "boolean" },
                        "output": { "type": "boolean" },
                        "tools": { "type": "boolean" }
                    },
                    "required": ["input", "output", "tools"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("spec should parse");
    let mut analysis = analyzer.analyze().expect("spec should analyze");

    let AnalyzedSchemaType::Object { properties, .. } = &analysis.schemas["Model"].schema_type
    else {
        panic!("Model should remain an object");
    };
    assert!(matches!(
        &properties["api"].schema_type,
        AnalyzedSchemaType::Reference { target } if target == "ModelApiInline"
    ));
    assert!(matches!(
        &properties["capabilities"].schema_type,
        AnalyzedSchemaType::Reference { target } if target == "ModelCapabilitiesInline"
    ));

    let AnalyzedSchemaType::Object {
        properties: component_api,
        ..
    } = &analysis.schemas["ModelApi"].schema_type
    else {
        panic!("ModelApi should remain the named component object");
    };
    assert_eq!(
        component_api.keys().collect::<Vec<_>>(),
        vec!["component_api"]
    );

    let AnalyzedSchemaType::Object {
        properties: component_capabilities,
        ..
    } = &analysis.schemas["ModelCapabilities"].schema_type
    else {
        panic!("ModelCapabilities should remain the named component object");
    };
    assert_eq!(
        component_capabilities.keys().collect::<Vec<_>>(),
        vec!["input", "output", "tools"]
    );

    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("component and inline object names should coexist");
    assert!(generated.contains("pub struct ModelApiInline"));
    assert!(generated.contains("pub struct ModelCapabilitiesInline"));
    assert!(generated.contains("pub struct ModelApi"));
    assert!(generated.contains("pub component_api: String"));
}

#[test]
fn inline_enums_do_not_replace_exact_named_components() {
    let spec = json!({
        "openapi": "3.1.0",
        "info": { "title": "inline-enum-component-collision", "version": "1.0.0" },
        "paths": {},
        "components": {
            "schemas": {
                "OutputFormat": {
                    "type": "object",
                    "properties": {
                        "container": { "type": "string", "enum": ["raw"] }
                    },
                    "required": ["container"]
                },
                "OutputFormatContainer": {
                    "type": "string",
                    "enum": ["raw", "wav", "mp3"]
                },
                "Control": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["control"] }
                    },
                    "required": ["type"]
                },
                "ControlType": {
                    "type": "string",
                    "enum": ["button", "checkbox", "slider"]
                },
                "Page": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["page"] }
                    },
                    "required": ["type"]
                },
                "PageType": {
                    "type": "string",
                    "enum": ["canvas", "embed"]
                },
                "Table": {
                    "type": "object",
                    "properties": {
                        "type": { "type": "string", "enum": ["table"] }
                    },
                    "required": ["type"]
                },
                "TableType": {
                    "type": "string",
                    "enum": ["table", "view"]
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).expect("spec should parse");
    let mut analysis = analyzer.analyze().expect("spec should analyze");

    assert!(matches!(
        &analysis.schemas["OutputFormatContainer"].schema_type,
        AnalyzedSchemaType::StringEnum { values }
            if values == &["raw".to_string(), "wav".to_string(), "mp3".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["OutputFormatContainerRaw"].schema_type,
        AnalyzedSchemaType::StringEnum { values } if values == &["raw".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["PageType"].schema_type,
        AnalyzedSchemaType::StringEnum { values }
            if values == &["canvas".to_string(), "embed".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["PageTypePage"].schema_type,
        AnalyzedSchemaType::StringEnum { values } if values == &["page".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["ControlType"].schema_type,
        AnalyzedSchemaType::StringEnum { values }
            if values == &["button".to_string(), "checkbox".to_string(), "slider".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["ControlTypeControl"].schema_type,
        AnalyzedSchemaType::StringEnum { values } if values == &["control".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["TableType"].schema_type,
        AnalyzedSchemaType::StringEnum { values }
            if values == &["table".to_string(), "view".to_string()]
    ));
    assert!(matches!(
        &analysis.schemas["TableTypeTable"].schema_type,
        AnalyzedSchemaType::StringEnum { values } if values == &["table".to_string()]
    ));

    let AnalyzedSchemaType::Object { properties, .. } =
        &analysis.schemas["OutputFormat"].schema_type
    else {
        panic!("OutputFormat should remain an object");
    };
    assert!(matches!(
        &properties["container"].schema_type,
        AnalyzedSchemaType::Reference { target } if target == "OutputFormatContainerRaw"
    ));

    let generated = CodeGenerator::new(GeneratorConfig::default())
        .generate(&mut analysis)
        .expect("component and inline enum names should coexist");
    assert!(generated.contains("pub enum OutputFormatContainer"));
    assert!(generated.contains("pub enum OutputFormatContainerRaw"));
    assert!(generated.contains("pub enum ControlType"));
    assert!(generated.contains("pub enum ControlTypeControl"));
    assert!(generated.contains("pub enum PageType"));
    assert!(generated.contains("pub enum PageTypePage"));
    assert!(generated.contains("pub enum TableType"));
    assert!(generated.contains("pub enum TableTypeTable"));
}
