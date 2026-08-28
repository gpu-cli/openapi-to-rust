use serde_json::{Value, json};
use std::{fs, path::PathBuf};

fn load_vercel_fixture() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("specs/vercel.json");
    let source = fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    serde_json::from_str(&source)
        .unwrap_or_else(|error| panic!("failed to parse {} as JSON: {error}", path.display()))
}

#[test]
fn closed_srv_txt_and_https_record_branches_declare_required_name() {
    let fixture = load_vercel_fixture();
    let branches = fixture
        .pointer(
            "/paths/~1v2~1domains~1{domain}~1records/post/requestBody/content/application~1json/schema/anyOf",
        )
        .and_then(Value::as_array)
        .expect("POST /v2/domains/{domain}/records should declare request anyOf branches");

    // MX immediately precedes SRV and uses the common record-name contract
    // shared by the other non-NS branches.
    let canonical_name = &branches[5]["properties"]["name"];
    assert_eq!(
        canonical_name,
        &json!({
            "description": "A subdomain name or an empty string for the root domain.",
            "type": "string",
            "example": "subdomain"
        })
    );

    for (index, record_type) in [(6, "SRV"), (7, "TXT"), (9, "HTTPS")] {
        let branch = &branches[index];
        assert_eq!(
            branch["properties"]["type"]["enum"],
            json!([record_type]),
            "branch {index} is no longer the expected {record_type} record"
        );
        assert_eq!(
            branch["additionalProperties"],
            Value::Bool(false),
            "{record_type} branch should remain closed"
        );

        let properties = branch["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{record_type} branch properties should be an object"));
        let required = branch["required"]
            .as_array()
            .unwrap_or_else(|| panic!("{record_type} branch required should be an array"));
        for member in required {
            let member = member
                .as_str()
                .unwrap_or_else(|| panic!("{record_type} branch required contains a non-string"));
            assert!(
                properties.contains_key(member),
                "{record_type} branch requires undeclared member {member:?} while additionalProperties is false"
            );
        }

        assert_eq!(
            &branch["properties"]["name"], canonical_name,
            "{record_type} name should match adjacent DNS record branches"
        );
    }
}
