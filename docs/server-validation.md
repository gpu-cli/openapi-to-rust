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

The six type URIs resolve to human-readable documentation:

- [Malformed request](https://openapi-to-rust.dev/problems/malformed-request)
- [Malformed parameter](https://openapi-to-rust.dev/problems/malformed-parameter)
- [Validation failure](https://openapi-to-rust.dev/problems/validation)
- [Body too large](https://openapi-to-rust.dev/problems/request-body-too-large)
- [Unsupported media type](https://openapi-to-rust.dev/problems/unsupported-media-type)
- [Generated contract error](https://openapi-to-rust.dev/problems/generated-contract-error)

These identifiers remain stable for previously generated servers. Clients do
not need to fetch the documentation while processing a rejection.

Generated clients preserve their operation-specific typed error and raw body.
`ApiError::problem_details()` lazily decodes this profile only when the response
declares `application/problem+json`.

## Customizing responses

Generated `ProblemDetails` and `InvalidParameter` fields are public, as is the
`RequestValidationRejection.0` field. Generated handlers reject invalid requests
before calling your API trait and immediately convert the rejection into an
Axum response. Implementing your API trait therefore cannot intercept it, and
the generated type already has an `IntoResponse` implementation.

Attach Axum 0.8 response middleware to the generated router instead:

```rust,ignore
let app = build_router(api)
    .layer(axum::middleware::map_response(customize_validation));
```

The [tested middleware example](examples/customize-validation.rs) reads bounded
`application/problem+json` bodies, decodes the generated `ProblemDetails`, and
replaces the project type URI prefix with `https://api.example.com/problems/`.
Copy it into your application and adjust the `crate::generated` import to match
your generated module. It preserves the HTTP status, other headers, and normal
or streaming responses; malformed or unrelated JSON passes through unchanged.
A problem body that exceeds the example's 64 KiB bound or fails to read becomes
an empty HTTP 500 response. Choose an appropriate bound for your service.

You can also change `title` or individual violations before serialization. For
a custom JSON shape, serialize your own type instead and set the response
Content-Type accordingly. Keep the status and body consistent, remove any stale
Content-Length after rewriting, and preserve the sanitized error policy below.
Custom profiles may no longer decode through generated clients'
`problem_details()` helper. Serve documentation for any replacement type URIs
you publish. There is currently no `problem_base_url` TOML option.

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
