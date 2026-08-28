use serde_json::Value;

#[test]
fn closed_gitpod_components_do_not_require_undeclared_wire_names() {
    let source = std::fs::read_to_string("specs/gitpod.yaml").expect("read Gitpod fixture");
    let document: Value = serde_yaml::from_str(&source).expect("Gitpod fixture is valid YAML");
    let schemas = document["components"]["schemas"]
        .as_object()
        .expect("Gitpod component schemas");

    let mut mismatches = Vec::new();
    for (name, schema) in schemas {
        if schema["additionalProperties"].as_bool() != Some(false) {
            continue;
        }
        let Some(required) = schema["required"].as_array() else {
            continue;
        };
        let properties = schema["properties"].as_object();
        for required_name in required.iter().filter_map(Value::as_str) {
            if properties.is_none_or(|properties| !properties.contains_key(required_name)) {
                mismatches.push(format!("{name}.{required_name}"));
            }
        }
    }

    assert!(
        mismatches.is_empty(),
        "closed Gitpod schemas require undeclared JSON property names: {mismatches:?}"
    );
    for component in ["gitpod.v1.Service", "gitpod.v1.Task"] {
        let schema = &schemas[component];
        assert!(schema["properties"].get("environmentId").is_some());
        assert!(
            schema["required"]
                .as_array()
                .is_some_and(|required| required.iter().any(|name| name == "environmentId"))
        );
    }
}
