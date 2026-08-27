use serde_json::Value;

const CANONICAL_EXAMPLE: &str = "e3c6ee77-48cb-416b-b204-1b492cc776e3";

fn is_canonical_uuid4(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes[index] == b'-')
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit())
        && bytes[14] == b'4'
        && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
}

fn validate_example_value(value: &Value, path: &str, checked: &mut usize) {
    match value {
        Value::String(example) => {
            assert!(
                is_canonical_uuid4(example),
                "{path} has a noncanonical UUIDv4 example: {example}"
            );
            *checked += 1;
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_example_value(value, &format!("{path}/{index}"), checked);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                validate_example_value(value, &format!("{path}/{key}"), checked);
            }
        }
        Value::Null => {}
        other => panic!("{path} has a non-string UUIDv4 example: {other}"),
    }
}

fn validate_attached_uuid4_examples(value: &Value, path: &str, checked: &mut usize) {
    match value {
        Value::Object(object) => {
            if object.get("format").and_then(Value::as_str) == Some("uuid4") {
                for keyword in ["example", "examples"] {
                    if let Some(examples) = object.get(keyword) {
                        validate_example_value(examples, &format!("{path}/{keyword}"), checked);
                    }
                }
            }
            for (key, value) in object {
                validate_attached_uuid4_examples(value, &format!("{path}/{key}"), checked);
            }
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                validate_attached_uuid4_examples(value, &format!("{path}/{index}"), checked);
            }
        }
        _ => {}
    }
}

#[test]
fn every_direct_gcore_uuid4_example_is_canonical() {
    let source = std::fs::read_to_string("specs/gcore.yaml").expect("read Gcore fixture");
    for malformed in [
        "e3c6ee77-48cb-416b-b204-11b492cc776e3",
        "024a29e-b4b7-4c91-9a46-505be123d9f8",
        "123e4567-e89b-12d3-a456-426614174000",
    ] {
        assert!(
            !source.contains(malformed),
            "Gcore fixture still contains malformed UUID example {malformed}"
        );
    }

    let document: Value = serde_yaml::from_str(&source).expect("Gcore fixture is valid YAML");
    let mut checked = 0;
    validate_attached_uuid4_examples(&document, "", &mut checked);
    assert!(
        checked >= 12,
        "expected to exercise at least the 12 repaired directly attached uuid4 examples, checked {checked}"
    );
}

#[test]
fn nested_component_examples_use_the_repaired_uuid4() {
    let source = std::fs::read_to_string("specs/gcore.yaml").expect("read Gcore fixture");
    let document: Value = serde_yaml::from_str(&source).expect("Gcore fixture is valid YAML");
    for pointer in [
        "/components/schemas/CreateBareMetalSubnetInterfaceSerializer/examples/0/subnet_id",
        "/components/schemas/InstancePricingPreviewV2RequestSerializer/examples/0/interfaces/1/subnet_id",
        "/components/schemas/NewInterfaceSpecificSubnetFipSerializerPydantic/examples/0/subnet_id",
    ] {
        let example = document
            .pointer(pointer)
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("missing nested Gcore UUID example at {pointer}"));
        assert_eq!(example, CANONICAL_EXAMPLE, "{pointer}");
        assert!(is_canonical_uuid4(example), "{pointer}: {example}");
    }
}
