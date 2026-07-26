use openapi_to_rust::analysis::{OperationInfo, RequestBodyContent, SchemaAnalysis};
use openapi_to_rust::generator::{CodeGenerator, GeneratorConfig};
use std::collections::BTreeMap;

fn create_test_config() -> GeneratorConfig {
    GeneratorConfig {
        spec_path: "test.json".into(),
        output_dir: "test_output".into(),
        module_name: "test_api".to_string(),
        enable_async_client: true,
        ..Default::default()
    }
}

fn create_test_analysis_with_operations(operations: Vec<OperationInfo>) -> SchemaAnalysis {
    let mut ops_map = BTreeMap::new();
    for op in operations {
        ops_map.insert(op.operation_id.clone(), op);
    }

    SchemaAnalysis {
        schemas: BTreeMap::new(),
        dependencies: openapi_to_rust::analysis::DependencyGraph::new(),
        patterns: openapi_to_rust::analysis::DetectedPatterns {
            tagged_enum_schemas: Default::default(),
            untagged_enum_schemas: Default::default(),
            type_mappings: Default::default(),
        },
        operations: ops_map,
        operation_responses: BTreeMap::new(),
        operation_id_aliases: BTreeMap::new(),
        used_type_features: Default::default(),
        enum_extensions: BTreeMap::new(),
        validation_context: Default::default(),
    }
}

#[test]
fn test_generate_get_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "getUser".to_string(),
        method: "GET".to_string(),
        path: "/users/{id}".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "User".to_string());
            map
        },
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify method name is snake_case
    assert!(result_str.contains("get_user"));
    // Verify HTTP method is GET
    assert!(result_str.contains(". get (request_url)"));
    // Verify response type — see issue #8 for the ApiOpError envelope.
    assert!(result_str.contains("Result < User , ApiOpError <"));
    // Verify no request body parameter
    assert!(!result_str.contains("request :"));
}

#[test]
fn test_generate_post_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "createUser".to_string(),
        method: "POST".to_string(),
        path: "/users".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "CreateUserRequest".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("201".to_string(), "User".to_string());
            map
        },
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify method name
    assert!(result_str.contains("create_user"));
    // Verify HTTP method is POST
    assert!(result_str.contains(". post (request_url)"));
    // Verify request parameter
    assert!(result_str.contains("request : CreateUserRequest"));
    // Verify request body serialization (middleware-compatible)
    assert!(result_str.contains("serde_json :: to_vec (& request)"));
    assert!(result_str.contains("application/json"));
    // Verify response type — see issue #8 for the ApiOpError envelope.
    assert!(result_str.contains("Result < User , ApiOpError <"));
}

#[test]
fn schema_less_request_content_preserves_client_operation_without_a_body() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);
    let operation = OperationInfo {
        operation_id: "triggerAction".to_string(),
        method: "POST".to_string(),
        path: "/actions".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::SchemaLess {
            media_type: "application/json".to_string(),
        }),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis).to_string();

    assert!(result.contains("trigger_action (& self"), "{result}");
    assert!(result.contains(". post (request_url)"), "{result}");
    assert!(!result.contains("request :"), "{result}");
    assert!(!result.contains(". body ("), "{result}");
}

#[test]
fn test_generate_put_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "updateUser".to_string(),
        method: "PUT".to_string(),
        path: "/users/{id}".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "UpdateUserRequest".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "User".to_string());
            map
        },
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify HTTP method is PUT
    assert!(result_str.contains(". put (request_url)"));
    // Verify request parameter
    assert!(result_str.contains("request : UpdateUserRequest"));
    // Verify request body serialization (middleware-compatible)
    assert!(result_str.contains("serde_json :: to_vec (& request)"));
    assert!(result_str.contains("application/json"));
}

#[test]
fn test_generate_delete_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "deleteUser".to_string(),
        method: "DELETE".to_string(),
        path: "/users/{id}".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify HTTP method is DELETE
    assert!(result_str.contains(". delete (request_url)"));
    // Verify no request body in the request (not looking at error handling)
    assert!(!result_str.contains(". json (& request)"));
}

#[test]
fn test_generate_patch_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "patchUser".to_string(),
        method: "PATCH".to_string(),
        path: "/users/{id}".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "PatchUserRequest".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "User".to_string());
            map
        },
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify HTTP method is PATCH
    assert!(result_str.contains(". patch (request_url)"));
    // Verify request parameter
    assert!(result_str.contains("request : PatchUserRequest"));
}

#[test]
fn test_method_name_from_operation_id() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let test_cases = vec![
        ("getUserById", "get_user_by_id"),
        ("createNewUser", "create_new_user"),
        ("listAllItems", "list_all_items"),
        ("updateUserProfile", "update_user_profile"),
    ];

    for (operation_id, expected_name) in test_cases {
        let operation = OperationInfo {
            operation_id: operation_id.to_string(),
            method: "GET".to_string(),
            path: "/test".to_string(),
            summary: None,
            description: None,
            request_body: None,
            response_schemas: BTreeMap::new(),
            parameters: vec![],
            request_body_required: false,
            supports_streaming: false,
            stream_parameter: None,
            tags: Vec::new(),
        };

        let analysis = create_test_analysis_with_operations(vec![operation]);
        let result = generator.generate_operation_methods(&analysis);
        let result_str = result.to_string();

        assert!(
            result_str.contains(expected_name),
            "Expected method name '{}' not found for operation ID '{}'",
            expected_name,
            operation_id
        );
    }
}

#[test]
fn test_method_with_response_type() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "getData".to_string(),
        method: "GET".to_string(),
        path: "/data".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "DataResponse".to_string());
            map
        },
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify response type is correctly mapped — see issue #8 for the
    // ApiOpError envelope.
    assert!(result_str.contains("Result < DataResponse , ApiOpError <"));
}

#[test]
fn test_method_without_response_type() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "noResponse".to_string(),
        method: "POST".to_string(),
        path: "/no-response".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "Request".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify unit type for no response — see issue #8 for the ApiOpError
    // envelope.
    assert!(result_str.contains("Result < () , ApiOpError <"));

    // Verify no deserialization for empty response (should use Ok(()) directly)
    assert!(
        !result_str.contains("failed to deserialize 2xx response body"),
        "Empty response type should not attempt JSON deserialization"
    );
    assert!(
        result_str.contains("Ok (())"),
        "Empty response type should return Ok(())"
    );
}

#[test]
fn test_error_handling_generation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "testOp".to_string(),
        method: "GET".to_string(),
        path: "/test".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "Response".to_string());
            map
        },
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify error handling logic — the new envelope reads the body to a
    // string before deserializing and wraps non-2xx (and 2xx parse failures)
    // in `ApiOpError::Api(ApiError { ... })`. See issue #8.
    assert!(result_str.contains("let status = response . status ()"));
    assert!(result_str.contains("if status . is_success ()"));
    assert!(result_str.contains("response . text ()"));
    assert!(result_str.contains("ApiOpError :: Api (ApiError"));
}

#[test]
fn test_url_construction() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    // Test without path parameters
    let operation1 = OperationInfo {
        operation_id: "simpleOp".to_string(),
        method: "GET".to_string(),
        path: "/users".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis1 = create_test_analysis_with_operations(vec![operation1]);
    let result1 = generator.generate_operation_methods(&analysis1);
    let result_str1 = result1.to_string();

    // Just check that URL construction exists - the exact format may vary
    assert!(result_str1.contains("self . base_url"));
    assert!(result_str1.contains("\"/users\""));

    // Test with path parameters
    let operation2 = OperationInfo {
        operation_id: "paramOp".to_string(),
        method: "GET".to_string(),
        path: "/users/{id}".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis2 = create_test_analysis_with_operations(vec![operation2]);
    let result2 = generator.generate_operation_methods(&analysis2);
    let result_str2 = result2.to_string();

    // Should still generate URL construction (parameters will be handled in future phases)
    assert!(result_str2.contains("self . base_url"));
    assert!(result_str2.contains("\"/users/{id}\""));
}

#[test]
fn test_bearer_auth_injection() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "authOp".to_string(),
        method: "GET".to_string(),
        path: "/secure".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify API key bearer auth logic
    assert!(result_str.contains("if let Some (api_key) = & self . api_key"));
    assert!(result_str.contains(". bearer_auth (api_key)"));
}

#[test]
fn test_custom_headers_injection() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "headerOp".to_string(),
        method: "POST".to_string(),
        path: "/headers".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "Request".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify custom headers logic
    assert!(result_str.contains("for (name , value) in & self . custom_headers"));
    assert!(result_str.contains(". header (name , value)"));
}

#[test]
fn test_multiple_operations() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operations = vec![
        OperationInfo {
            operation_id: "getUsers".to_string(),
            method: "GET".to_string(),
            path: "/users".to_string(),
            summary: None,
            description: None,
            request_body: None,
            response_schemas: {
                let mut map = BTreeMap::new();
                map.insert("200".to_string(), "UserList".to_string());
                map
            },
            parameters: vec![],
            request_body_required: false,
            supports_streaming: false,
            stream_parameter: None,
            tags: Vec::new(),
        },
        OperationInfo {
            operation_id: "createUser".to_string(),
            method: "POST".to_string(),
            path: "/users".to_string(),
            summary: None,
            description: None,
            request_body: Some(RequestBodyContent::Json {
                schema_name: "CreateUserRequest".to_string(),
                media_type: "application/json".to_string(),
                validation_schema: serde_json::Value::Null,
            }),
            response_schemas: {
                let mut map = BTreeMap::new();
                map.insert("201".to_string(), "User".to_string());
                map
            },
            parameters: vec![],
            request_body_required: true,
            supports_streaming: false,
            stream_parameter: None,
            tags: Vec::new(),
        },
        OperationInfo {
            operation_id: "deleteUser".to_string(),
            method: "DELETE".to_string(),
            path: "/users/{id}".to_string(),
            summary: None,
            description: None,
            request_body: None,
            response_schemas: BTreeMap::new(),
            parameters: vec![],
            request_body_required: false,
            supports_streaming: false,
            stream_parameter: None,
            tags: Vec::new(),
        },
    ];

    let analysis = create_test_analysis_with_operations(operations);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify all operations are generated
    assert!(result_str.contains("get_users"));
    assert!(result_str.contains("create_user"));
    assert!(result_str.contains("delete_user"));

    // Verify each has correct HTTP method
    assert!(result_str.contains(". get (request_url)"));
    assert!(result_str.contains(". post (request_url)"));
    assert!(result_str.contains(". delete (request_url)"));
}

#[test]
fn test_doc_comment_generation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "testOperation".to_string(),
        method: "POST".to_string(),
        path: "/api/v1/test".to_string(),
        summary: None,
        description: None,
        request_body: None,
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: false,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // T13: rustdoc now wraps the method+path in backticks for nicer rendering.
    assert!(result_str.contains("`POST /api/v1/test`"));
}

#[test]
fn test_generate_form_urlencoded_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "createToken".to_string(),
        method: "POST".to_string(),
        path: "/token".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::FormUrlEncoded {
            schema_name: "TokenRequest".to_string(),
            media_type: "application/x-www-form-urlencoded".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: {
            let mut map = BTreeMap::new();
            map.insert("200".to_string(), "TokenResponse".to_string());
            map
        },
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify request parameter uses typed struct
    assert!(result_str.contains("request : TokenRequest"));
    // Verify url-encoded serialization
    assert!(result_str.contains("serde_urlencoded :: to_string (& request)"));
    // Verify content-type header
    assert!(result_str.contains("application/x-www-form-urlencoded"));
}

#[test]
fn test_generate_multipart_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "uploadFile".to_string(),
        method: "POST".to_string(),
        path: "/upload".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Multipart),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify parameter is reqwest multipart form
    assert!(result_str.contains("form : reqwest :: multipart :: Form"));
    // Verify .multipart(form) call
    assert!(result_str.contains(". multipart (form)"));
}

#[test]
fn test_generate_octet_stream_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "uploadData".to_string(),
        method: "POST".to_string(),
        path: "/data".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::OctetStream),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify parameter is Vec<u8>
    assert!(result_str.contains("body : Vec < u8 >"));
    // Verify body is used directly
    assert!(result_str.contains(". body (body)"));
    // Verify content-type header
    assert!(result_str.contains("application/octet-stream"));
}

#[test]
fn test_generate_text_plain_operation() {
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let operation = OperationInfo {
        operation_id: "echo".to_string(),
        method: "POST".to_string(),
        path: "/echo".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::TextPlain),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![operation]);
    let result = generator.generate_operation_methods(&analysis);
    let result_str = result.to_string();

    // Verify parameter is String
    assert!(result_str.contains("body : String"));
    // Verify body is used directly
    assert!(result_str.contains(". body (body)"));
    // Verify content-type header
    assert!(result_str.contains("text/plain"));
}

#[test]
fn test_request_body_schema_name_pascal_cased() {
    // Specs like Latitude.sh declare components/schemas with snake_case
    // names (e.g. `update_api_key`). The struct definitions go through
    // `to_rust_type_name` and become `UpdateApiKey`, but the request-body
    // parameter type used to be wired in raw, producing
    // `request: update_api_key` and a borked compile.
    let config = create_test_config();
    let generator = CodeGenerator::new(config);

    let json_op = OperationInfo {
        operation_id: "patchApiKey".to_string(),
        method: "PATCH".to_string(),
        path: "/auth/api_keys/{id}".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::Json {
            schema_name: "update_api_key".to_string(),
            media_type: "application/json".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let form_op = OperationInfo {
        operation_id: "createToken".to_string(),
        method: "POST".to_string(),
        path: "/oauth/token".to_string(),
        summary: None,
        description: None,
        request_body: Some(RequestBodyContent::FormUrlEncoded {
            schema_name: "create_oauth_token".to_string(),
            media_type: "application/x-www-form-urlencoded".to_string(),
            validation_schema: serde_json::Value::Null,
        }),
        response_schemas: BTreeMap::new(),
        parameters: vec![],
        request_body_required: true,
        supports_streaming: false,
        stream_parameter: None,
        tags: Vec::new(),
    };

    let analysis = create_test_analysis_with_operations(vec![json_op, form_op]);
    let result_str = generator.generate_operation_methods(&analysis).to_string();

    assert!(
        result_str.contains("request : UpdateApiKey"),
        "expected PascalCase JSON request body type, got: {result_str}"
    );
    assert!(
        !result_str.contains("request : update_api_key"),
        "raw snake_case schema name leaked into JSON method signature: {result_str}"
    );
    assert!(
        result_str.contains("request : CreateOauthToken"),
        "expected PascalCase form-urlencoded request body type, got: {result_str}"
    );
}
