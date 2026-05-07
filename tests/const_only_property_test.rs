//! Regression test for https://github.com/gpu-cli/openapi-to-rust/issues/10
//!
//! A property with a string `const` and no `enum` array should generate a
//! single-variant enum, not a bare `Option<String>`.

#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn const_only_string_property_generates_single_variant_enum() {
        let spec = minimal_spec(json!({
            "ConstModifier": {
                "type": "object",
                "properties": {
                    "someConstant": {
                        "type": "string",
                        "const": "TheOnlyValidValue"
                    }
                }
            }
        }));

        let result = test_generation("const_only_property", spec).expect("Generation failed");

        assert!(
            result.contains("pub enum ConstModifierSomeConstant"),
            "expected ConstModifierSomeConstant enum to be generated; got:\n{result}"
        );
        assert!(
            result.contains("#[serde(rename = \"TheOnlyValidValue\")]"),
            "expected serde rename for the const value; got:\n{result}"
        );
        assert!(
            result.contains("TheOnlyValidValue,"),
            "expected TheOnlyValidValue variant; got:\n{result}"
        );

        assert!(
            result.contains("pub some_constant: Option<ConstModifierSomeConstant>"),
            "field should reference the generated enum, not String; got:\n{result}"
        );
        assert!(
            !result.contains("pub some_constant: Option<String>"),
            "field should NOT be Option<String>; got:\n{result}"
        );
    }

    #[test]
    fn required_const_only_property_is_not_optional() {
        let spec = minimal_spec(json!({
            "RequiredConst": {
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "const": "fixed"
                    }
                },
                "required": ["kind"]
            }
        }));

        let result = test_generation("const_only_required", spec).expect("Generation failed");

        assert!(
            result.contains("pub enum RequiredConstKind"),
            "expected RequiredConstKind enum; got:\n{result}"
        );
        assert!(
            result.contains("pub kind: RequiredConstKind"),
            "required const field should be non-optional; got:\n{result}"
        );
    }
}
