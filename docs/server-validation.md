# Generated Axum request validation

Generated Axum 0.8 servers validate selected operation inputs at the HTTP
boundary before invoking an implementation trait. The original OpenAPI schemas,
not Rust derive attributes, remain the contract source of truth.

## Configuration

```toml
[server.validation]
enabled = true
max_body_bytes = 2097152
max_errors = 16
```

Validation is enabled by default. `max_body_bytes` accepts 1 through 67,108,864
bytes; `max_errors` accepts 1 through 100. These bounds are checked both while
loading TOML and at the code-generation boundary for programmatically-created
configuration. Disabling validation is an explicit compatibility escape hatch;
typed header, cookie, and form extraction currently requires validation.

The generator selects Draft 4 semantics for OpenAPI 3.0 and Draft 2020-12 for
OpenAPI 3.1/3.2. It embeds the transitive local component-schema closure into one
offline document and rejects external or unsupported references. Generated
projects use:

```toml
jsonschema = { version = "0.49", default-features = false }
mime = "0.3"
```

Disabling default features prevents runtime HTTP or file schema resolution.

## Public error contract

Validation failures use RFC 9457 Problem Details media type
`application/problem+json` with this generated profile:

```json
{
  "type": "https://openapi-to-rust.dev/problems/validation",
  "title": "Request validation failed",
  "status": 422,
  "code": "request_validation_failed",
  "errors": [
    {
      "code": "required",
      "location": "/body/name",
      "message": "is required"
    }
  ]
}
```

`location` is a JSON Pointer rooted at `/body`, `/path`, `/query`, `/header`, or
`/cookie`. Violations are deduplicated and capped while the validator is
traversed, then sorted before the bounded response is returned.

| Status | Stable code | Meaning |
| --- | --- | --- |
| 400 | `malformed_request` / `malformed_parameter` | Syntax, encoding, or typed transport decoding failed |
| 413 | `request_body_too_large` | The configured body limit was exceeded |
| 415 | `unsupported_media_type` | The request does not use the operation's selected media type |
| 422 | `request_validation_failed` | A decoded value violates the OpenAPI schema |
| 500 | `generated_contract_error` | Generated Rust types disagree with a schema that already accepted the value |

Generated clients preserve their operation-specific typed error and raw body.
`ApiError::problem_details()` lazily decodes this profile only when the response
declares `application/problem+json`.

## Information disclosure and logging

Public errors never include submitted values, Serde or validator diagnostics,
schema implementation paths, Rust type names, backtraces, or internal failures.
Applications may log richer diagnostics on the trusted server side, but should
not log request bodies, authorization/cookie headers, or raw validator instances
without an explicit redaction policy. A request/correlation ID is preferable for
joining the public response to internal telemetry.

## Supported inputs

The generated server validates JSON bodies and supported scalar path, query,
header, and cookie parameters. Form-urlencoded bodies are supported for closed,
flat objects with scalar fields. Unsupported aggregate serialization shapes and
selected multipart, text, or octet-stream bodies produce generation errors rather
than silently weakening the contract.
