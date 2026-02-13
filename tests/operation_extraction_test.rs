use openapi_generator::analysis::{RequestBodyContent, SchemaAnalyzer};

#[test]
fn test_extract_simple_get() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/simple_get.json").unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    // Should extract 1 operation
    assert_eq!(analysis.operations.len(), 1);

    let op = analysis
        .operations
        .get("listItems")
        .expect("listItems operation not found");
    assert_eq!(op.operation_id, "listItems");
    assert_eq!(op.method, "GET");
    assert_eq!(op.path, "/items");

    // Snapshot the operations
    insta::assert_yaml_snapshot!("simple_get_operations", analysis.operations);
}

#[test]
fn test_extract_post_with_body() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/post_with_body.json").unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    assert_eq!(analysis.operations.len(), 1);

    let op = analysis
        .operations
        .get("createItem")
        .expect("createItem operation not found");
    assert_eq!(op.operation_id, "createItem");
    assert_eq!(op.method, "POST");
    assert!(op.request_body.is_some());
    assert_eq!(
        op.request_body.as_ref().and_then(|rb| rb.schema_name()),
        Some("CreateItemRequest")
    );
    assert!(!op.response_schemas.is_empty());

    insta::assert_yaml_snapshot!("post_with_body_operations", analysis.operations);
}

#[test]
fn test_extract_path_params() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/path_params.json").unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    assert_eq!(analysis.operations.len(), 1);

    let op = analysis
        .operations
        .get("getItem")
        .expect("getItem operation not found");
    assert_eq!(op.path, "/items/{itemId}");
    assert!(!op.parameters.is_empty());
    assert_eq!(op.parameters.len(), 1);
    assert_eq!(op.parameters[0].name, "itemId");
    assert_eq!(op.parameters[0].location, "path");
    assert!(op.parameters[0].required);

    insta::assert_yaml_snapshot!("path_params_operations", analysis.operations);
}

#[test]
fn test_extract_multiple_operations() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/multiple_operations.json")
            .unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    // Should extract 4 operations (GET, POST, PUT, DELETE)
    assert_eq!(analysis.operations.len(), 4);

    let methods: Vec<&str> = analysis
        .operations
        .values()
        .map(|op| op.method.as_str())
        .collect();
    assert!(methods.contains(&"GET"));
    assert!(methods.contains(&"POST"));
    assert!(methods.contains(&"PUT"));
    assert!(methods.contains(&"DELETE"));

    // Verify specific operations
    assert!(analysis.operations.contains_key("getItem"));
    assert!(analysis.operations.contains_key("updateItem"));
    assert!(analysis.operations.contains_key("deleteItem"));
    assert!(analysis.operations.contains_key("duplicateItem"));

    insta::assert_yaml_snapshot!("multiple_operations", analysis.operations);
}

#[test]
fn test_mcp_registry_operations_without_operation_ids() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/mcp_registry_subset.json")
            .unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    // Should extract 2 operations even without explicit operationId
    assert_eq!(analysis.operations.len(), 2);

    // Verify generated operation IDs follow pattern
    let operation_ids: Vec<&str> = analysis.operations.keys().map(|s| s.as_str()).collect();
    assert!(operation_ids.contains(&"getV0Servers"));
    assert!(operation_ids.contains(&"getV0ServersServerId"));

    // Verify specific operations
    let list_servers = analysis
        .operations
        .get("getV0Servers")
        .expect("getV0Servers operation not found");
    assert_eq!(list_servers.path, "/v0/servers");
    assert_eq!(list_servers.method, "GET");

    let get_server = analysis
        .operations
        .get("getV0ServersServerId")
        .expect("getV0ServersServerId operation not found");
    assert_eq!(get_server.path, "/v0/servers/{serverId}");
    assert_eq!(get_server.method, "GET");
    assert_eq!(get_server.parameters.len(), 1);

    insta::assert_yaml_snapshot!("mcp_registry_operations", analysis.operations);
}

#[test]
fn test_operation_id_generation() {
    // Test the operation ID generation function directly through analysis
    let test_cases = vec![
        ("/items", "get", "getItems"),
        ("/items/{id}", "get", "getItemsId"),
        ("/v0/servers", "get", "getV0Servers"),
        ("/v0/servers/{serverId}", "get", "getV0ServersServerId"),
        (
            "/v0/servers/{serverName}/versions",
            "get",
            "getV0ServersServerNameVersions",
        ),
        (
            "/users/{userId}/posts/{postId}",
            "delete",
            "deleteUsersUserIdPostsPostId",
        ),
        ("/api/v1/resources", "post", "postApiV1Resources"),
    ];

    for (path, method, expected_id) in test_cases {
        let spec_json = serde_json::json!({
            "openapi": "3.1.0",
            "info": {"title": "Test", "version": "1.0.0"},
            "paths": {
                path: {
                    method: {
                        "responses": {
                            "200": {"description": "Success"}
                        }
                    }
                }
            },
            "components": {
                "schemas": {
                    "DummySchema": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"}
                        }
                    }
                }
            }
        });

        let mut analyzer = SchemaAnalyzer::new(spec_json).unwrap();
        let analysis = analyzer.analyze().unwrap();

        assert_eq!(
            analysis.operations.len(),
            1,
            "Failed for {} {}",
            method,
            path
        );
        assert!(
            analysis.operations.contains_key(expected_id),
            "Expected operation ID '{}' not found for {} {}. Found: {:?}",
            expected_id,
            method,
            path,
            analysis.operations.keys().collect::<Vec<_>>()
        );
    }
}

#[test]
fn test_mixed_explicit_and_generated_operation_ids() {
    let spec = serde_json::json!({
        "openapi": "3.1.0",
        "info": {"title": "Test", "version": "1.0.0"},
        "paths": {
            "/items": {
                "get": {
                    "operationId": "listItems",
                    "responses": {"200": {"description": "Success"}}
                },
                "post": {
                    "responses": {"201": {"description": "Created"}}
                }
            },
            "/items/{id}": {
                "get": {
                    "operationId": "getItem",
                    "responses": {"200": {"description": "Success"}}
                },
                "delete": {
                    "responses": {"204": {"description": "No Content"}}
                }
            }
        },
        "components": {
            "schemas": {
                "DummySchema": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"}
                    }
                }
            }
        }
    });

    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let analysis = analyzer.analyze().unwrap();

    assert_eq!(analysis.operations.len(), 4);

    // Explicit operation IDs should be preserved
    assert!(analysis.operations.contains_key("listItems"));
    assert!(analysis.operations.contains_key("getItem"));

    // Missing operation IDs should be generated
    assert!(analysis.operations.contains_key("postItems"));
    assert!(analysis.operations.contains_key("deleteItemsId"));
}

#[test]
fn test_extract_form_urlencoded_body() {
    let spec = std::fs::read_to_string("tests/fixtures/operation_extraction/form_urlencoded.json")
        .unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let op = analysis
        .operations
        .get("createToken")
        .expect("createToken operation not found");
    assert!(op.request_body.is_some());
    assert!(matches!(
        op.request_body.as_ref().unwrap(),
        RequestBodyContent::FormUrlEncoded { schema_name } if schema_name == "TokenRequest"
    ));
}

#[test]
fn test_extract_multipart_body() {
    let spec = std::fs::read_to_string("tests/fixtures/operation_extraction/multipart_upload.json")
        .unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let op = analysis
        .operations
        .get("uploadFile")
        .expect("uploadFile operation not found");
    assert!(op.request_body.is_some());
    assert!(matches!(
        op.request_body.as_ref().unwrap(),
        RequestBodyContent::Multipart
    ));
}

#[test]
fn test_extract_octet_stream_body() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/octet_stream.json").unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let op = analysis
        .operations
        .get("uploadData")
        .expect("uploadData operation not found");
    assert!(op.request_body.is_some());
    assert!(matches!(
        op.request_body.as_ref().unwrap(),
        RequestBodyContent::OctetStream
    ));
}

#[test]
fn test_extract_text_plain_body() {
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/text_plain.json").unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let op = analysis
        .operations
        .get("echo")
        .expect("echo operation not found");
    assert!(op.request_body.is_some());
    assert!(matches!(
        op.request_body.as_ref().unwrap(),
        RequestBodyContent::TextPlain
    ));
}

#[test]
fn test_content_type_priority() {
    // When both JSON and multipart are present, JSON should win
    let spec =
        std::fs::read_to_string("tests/fixtures/operation_extraction/multi_content_type.json")
            .unwrap();
    let spec_value: serde_json::Value = serde_json::from_str(&spec).unwrap();

    let mut analyzer = SchemaAnalyzer::new(spec_value).unwrap();
    let analysis = analyzer.analyze().unwrap();

    let op = analysis
        .operations
        .get("createItem")
        .expect("createItem operation not found");
    assert!(op.request_body.is_some());
    assert!(matches!(
        op.request_body.as_ref().unwrap(),
        RequestBodyContent::Json { schema_name } if schema_name == "CreateItemRequest"
    ));
}
