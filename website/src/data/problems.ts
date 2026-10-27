/** Published identifiers must remain stable for previously generated servers. */
export const problems = [
  {
    slug: "malformed-request", title: "Malformed request", status: 400, code: "malformed_request",
    description: "Understand malformed-request errors from generated Axum servers: HTTP 400, invalid body syntax or encoding, and how to correct your request before retrying.",
    meaning: "The request body could not be read or decoded. Examples include invalid JSON syntax and malformed form encoding.",
    action: "Check the body syntax and encoding against the operation's declared request media type, then send a corrected request.",
  },
  {
    slug: "malformed-parameter", title: "Malformed request parameter", status: 400, code: "malformed_parameter",
    description: "Understand malformed-parameter errors from generated Axum servers: HTTP 400, invalid transport parameters, error locations, and how to correct the request.",
    meaning: "A path, query, header, or cookie parameter could not be decoded into its transport type, or its encoding was malformed.",
    action: "Use the errors array to find the parameter. Check percent encoding and the expected scalar type, such as an integer or boolean.",
  },
  {
    slug: "validation", title: "Request validation failed", status: 422, code: "request_validation_failed",
    description: "Understand validation errors from generated Axum servers: HTTP 422, OpenAPI schema constraints, missing required inputs, and the sanitized errors array.",
    meaning: "A required input is missing, or a decoded value violates the operation's OpenAPI schema. Constraints can include length, range, enum, and pattern.",
    action: "Use each violation's location and code to correct the input against the OpenAPI schema. Missing inputs use the required violation code.",
  },
  {
    slug: "request-body-too-large", title: "Request body too large", status: 413, code: "request_body_too_large",
    description: "Understand request-body-too-large errors from generated Axum servers: HTTP 413, configured request size limits, and options for clients and service owners.",
    meaning: "The request body exceeds the generated server's configured byte limit. The default max_body_bytes is 2,097,152 bytes (2 MiB).",
    action: "Send a smaller body. Service owners can adjust server.validation.max_body_bytes within the supported range of 1 through 67,108,864 bytes.",
  },
  {
    slug: "unsupported-media-type", title: "Unsupported media type", status: 415, code: "unsupported_media_type",
    description: "Understand unsupported-media-type errors from generated Axum servers: HTTP 415, request Content-Type matching, and the operation's selected media type.",
    meaning: "The request Content-Type is missing, invalid, or does not match the operation's selected request media type.",
    action: "Set Content-Type to the operation's selected media type, including any required media type parameters, and encode the body accordingly.",
  },
  {
    slug: "generated-contract-error", title: "Internal server error", status: 500, code: "generated_contract_error",
    description: "Understand generated-contract-error responses from Axum servers: HTTP 500, internal validation contract failures, and safe investigation by service owners.",
    meaning: "The generated server encountered an internal contract failure, such as an unavailable schema validator or a Rust type that cannot decode a schema-valid value.",
    action: "Report the failure to the service owner. Owners should check server diagnostics and regenerate from the current OpenAPI document; changing client input is not the expected fix.",
  },
] as const;
