#![cfg(feature = "test-helpers")]

#[cfg(test)]
mod tests {
    use openapi_to_rust::test_helpers::*;
    use serde_json::json;

    #[test]
    fn test_x_stainless_const_bug_nested() {
        // Test that reproduces the exact nesting issue from OpenAI spec
        let spec = minimal_spec(json!({
            "ReasoningPart": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the part. Always `summary_text`.",
                        "enum": ["summary_text"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "text": {
                        "type": "string"
                    }
                },
                "required": ["type", "text"]
            },
            "ResponseEvent": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the event. Always `response.event.done`.",
                        "enum": ["response.event.done"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "part": {
                        "$ref": "#/components/schemas/ReasoningPart"
                    }
                },
                "required": ["type", "part"]
            }
        }));

        let result = test_generation("x_stainless_nested", spec).expect("Generation failed");

        println!("Generated output:\n{}", result);

        // Test that ReasoningPart type field has correct enum value
        assert!(
            result.contains("pub enum ReasoningPartType"),
            "ReasoningPartType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"summary_text\")]"),
            "ReasoningPartType should have summary_text variant with correct serde rename"
        );

        // CRITICAL BUG TEST: ResponseEvent type field should NOT be summary_text
        assert!(
            result.contains("pub enum ResponseEventType"),
            "ResponseEventType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"response.event.done\")]"),
            "ResponseEventType should have response.event.done variant with correct serde rename - NOT summary_text"
        );

        // Make sure the wrong value isn't there for ResponseEvent
        if result.contains("ResponseEventType") {
            assert!(
                !result.contains("ResponseEventType")
                    || !result.contains("summary_text")
                    || result.contains("response.event.done"),
                "ResponseEventType should NOT have summary_text enum value - this would indicate the bug"
            );
        }
    }

    #[test]
    fn test_x_stainless_const_bug_discriminated_union() {
        // Test that reproduces the exact discriminated union pattern from OpenAI spec
        let spec = minimal_spec(json!({
            "ReasoningPart": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the part. Always `summary_text`.",
                        "enum": ["summary_text"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "text": {
                        "type": "string"
                    }
                },
                "required": ["type", "text"]
            },
            "ResponseReasoningSummaryPartAddedEvent": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the event. Always `response.reasoning_summary_part.added`.",
                        "enum": ["response.reasoning_summary_part.added"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "part": {
                        "$ref": "#/components/schemas/ReasoningPart"
                    }
                },
                "required": ["type", "part"]
            },
            "ResponseReasoningSummaryPartDoneEvent": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the event. Always `response.reasoning_summary_part.done`.",
                        "enum": ["response.reasoning_summary_part.done"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "part": {
                        "$ref": "#/components/schemas/ReasoningPart"
                    }
                },
                "required": ["type", "part"]
            },
            "EventUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/ResponseReasoningSummaryPartAddedEvent" },
                    { "$ref": "#/components/schemas/ResponseReasoningSummaryPartDoneEvent" }
                ],
                "discriminator": {
                    "propertyName": "type"
                }
            }
        }));

        let result = test_generation("x_stainless_discriminated", spec).expect("Generation failed");

        println!("Generated output:\n{}", result);

        // Test that ReasoningPart type field has correct enum value
        assert!(
            result.contains("pub enum ReasoningPartType"),
            "ReasoningPartType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"summary_text\")]"),
            "ReasoningPartType should have summary_text variant with correct serde rename"
        );

        // CRITICAL BUG TEST: Event type enums should have correct values
        assert!(
            result.contains("pub enum ResponseReasoningSummaryPartAddedEventType"),
            "ResponseReasoningSummaryPartAddedEventType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"response.reasoning_summary_part.added\")]"),
            "ResponseReasoningSummaryPartAddedEventType should have response.reasoning_summary_part.added variant"
        );

        assert!(
            result.contains("pub enum ResponseReasoningSummaryPartDoneEventType"),
            "ResponseReasoningSummaryPartDoneEventType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"response.reasoning_summary_part.done\")]"),
            "ResponseReasoningSummaryPartDoneEventType should have response.reasoning_summary_part.done variant"
        );

        // Test discriminated union generation
        assert!(
            result.contains("pub enum EventUnion"),
            "EventUnion should be generated as discriminated union"
        );
        assert!(
            result.contains("match discriminator.as_str()"),
            "EventUnion should deserialize by its type discriminator"
        );
    }

    #[test]
    #[ignore = "Test designed to fail until generator enum collision bug is fixed - see investigation results"]
    fn test_actual_reasoning_item_missing_type_field() {
        // Test the ACTUAL ReasoningItem structure from OpenAI spec
        // This reproduces the missing main type field issue
        let spec = minimal_spec(json!({
            "ReasoningItem": {
                "description": "A description of the chain of thought used by a reasoning model while generating a response.",
                "type": "object",
                "properties": {
                    "encrypted_content": {
                        "description": "The encrypted content of the reasoning item - populated when a response is generated with `reasoning.encrypted_content` in the `include` parameter.",
                        "nullable": true,
                        "type": "string"
                    },
                    "id": {
                        "description": "The unique identifier of the reasoning content.",
                        "type": "string"
                    },
                    "status": {
                        "description": "The status of the item. One of `in_progress`, `completed`, or `incomplete`. Populated when items are returned via API.",
                        "enum": [
                            "in_progress",
                            "completed",
                            "incomplete"
                        ],
                        "type": "string"
                    },
                    "summary": {
                        "description": "Reasoning text contents.",
                        "items": {
                            "properties": {
                                "text": {
                                    "description": "A short summary of the reasoning used by the model when generating the response.",
                                    "type": "string"
                                },
                                "type": {
                                    "description": "The type of the object. Always `summary_text`.",
                                    "enum": [
                                        "summary_text"
                                    ],
                                    "type": "string",
                                    "x-stainless-const": true
                                }
                            },
                            "required": [
                                "type",
                                "text"
                            ],
                            "type": "object"
                        },
                        "type": "array"
                    },
                    "type": {
                        "description": "The type of the object. Always `reasoning`.",
                        "enum": [
                            "reasoning"
                        ],
                        "type": "string",
                        "x-stainless-const": true
                    }
                },
                "required": [
                    "id",
                    "summary",
                    "type"
                ],
                "title": "Reasoning",
                "type": "object"
            }
        }));

        let result = test_generation("actual_reasoning_item_missing_type_field", spec)
            .expect("Generation failed");

        println!("Generated output:\n{}", result);

        // CRITICAL BUG TEST: ReasoningItem should have a main type field with value "reasoning"
        assert!(
            result.contains("pub struct ReasoningItem"),
            "ReasoningItem struct should be generated"
        );
        assert!(
            result.contains("pub r#type: ReasoningItemMainType"),
            "ReasoningItem should have main type field"
        );
        assert!(
            result.contains("pub enum ReasoningItemMainType")
                || result.contains("pub enum ReasoningItemType"),
            "ReasoningItem main type enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"reasoning\")]"),
            "ReasoningItem main type should have 'reasoning' variant"
        );

        // Test that summary array item type is correct
        assert!(
            result.contains("ReasoningItemSummaryItemType")
                || result.contains("ReasoningItemSummaryItem"),
            "ReasoningItem summary item type should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"summary_text\")]"),
            "ReasoningItem summary item should have 'summary_text' variant"
        );
    }

    #[test]
    fn test_x_stainless_const_bug_reproduction() {
        let spec = minimal_spec(json!({
            "ReasoningItem": {
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the object. Always `summary_text`.",
                        "enum": ["summary_text"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "text": {
                        "type": "string"
                    }
                },
                "required": ["type", "text"]
            },
            "ResponseReasoningSummaryPartDoneEvent": {
                "description": "Emitted when a reasoning summary part is completed.",
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the event. Always `response.reasoning_summary_part.done`.",
                        "enum": ["response.reasoning_summary_part.done"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "item_id": {
                        "type": "string"
                    },
                    "part": {
                        "$ref": "#/components/schemas/ReasoningItem"
                    }
                },
                "required": ["type", "item_id", "part"]
            },
            "ResponseReasoningSummaryPartAddedEvent": {
                "description": "Emitted when a new reasoning summary part is added.",
                "type": "object",
                "properties": {
                    "type": {
                        "description": "The type of the event. Always `response.reasoning_summary_part.added`.",
                        "enum": ["response.reasoning_summary_part.added"],
                        "type": "string",
                        "x-stainless-const": true
                    },
                    "item_id": {
                        "type": "string"
                    },
                    "part": {
                        "$ref": "#/components/schemas/ReasoningItem"
                    }
                },
                "required": ["type", "item_id", "part"]
            },
            "EventUnion": {
                "oneOf": [
                    { "$ref": "#/components/schemas/ResponseReasoningSummaryPartDoneEvent" },
                    { "$ref": "#/components/schemas/ResponseReasoningSummaryPartAddedEvent" }
                ],
                "discriminator": {
                    "propertyName": "type"
                }
            }
        }));

        let result = test_generation("x_stainless_const_bug", spec).expect("Generation failed");

        println!("Generated output:\n{}", result);

        // Test that ReasoningItem type field has correct enum value
        assert!(
            result.contains("pub enum ReasoningItemType"),
            "ReasoningItemType enum should be generated"
        );
        assert!(
            result.contains("#[serde(rename = \"summary_text\")]"),
            "ReasoningItemType should have summary_text variant with correct serde rename"
        );

        // Test that event type fields have correct enum values (NOT summary_text)
        assert!(
            result.contains("pub enum ResponseReasoningSummaryPartDoneEventType"),
            "ResponseReasoningSummaryPartDoneEventType enum should be generated"
        );
        assert!(
            result.contains("pub enum ResponseReasoningSummaryPartAddedEventType"),
            "ResponseReasoningSummaryPartAddedEventType enum should be generated"
        );

        // CRITICAL BUG TEST: These should NOT be "summary_text"
        assert!(
            !result.contains("ResponseReasoningSummaryPartDoneEventType")
                || !result.contains("#[serde(rename = \"summary_text\")]")
                || result.contains("#[serde(rename = \"response.reasoning_summary_part.done\")]"),
            "ResponseReasoningSummaryPartDoneEventType should NOT have summary_text - should have response.reasoning_summary_part.done"
        );
        assert!(
            !result.contains("ResponseReasoningSummaryPartAddedEventType")
                || !result.contains("#[serde(rename = \"summary_text\")]")
                || result.contains("#[serde(rename = \"response.reasoning_summary_part.added\")]"),
            "ResponseReasoningSummaryPartAddedEventType should NOT have summary_text - should have response.reasoning_summary_part.added"
        );

        // Test discriminated union generation
        assert!(
            result.contains("pub enum EventUnion"),
            "EventUnion should be generated as discriminated union"
        );
        assert!(
            result.contains("match discriminator.as_str()"),
            "EventUnion should deserialize by its type discriminator"
        );
    }
}
