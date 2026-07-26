# Architecture

## Request pipeline

For every selected server operation, generated routing and extraction follows one
ordered pipeline:

1. Axum matches an Axum 0.8 `{parameter}` route.
2. A generated body-limit layer rejects oversized payloads before buffering.
3. Generated extractors classify transport failures: unsupported media type,
   malformed encoding or JSON, and missing required transport inputs.
4. Typed extraction converts path, query, header, cookie, and body inputs.
5. A generated validation layer checks the normalized operation schema.
6. Only validated values reach the user implementation trait.

Transport and schema failures never enter user handlers. User handlers retain the
existing typed response enums for application-level responses.

## Dependency strategy

- `axum = "0.8"` with only the features required by the generation mode.
- `async-trait = "0.1"` for the generated public trait API, replacing the removed
  `axum::async_trait` re-export without forcing a larger trait redesign.
- `jsonschema = "0.49"` with default features disabled and only local validation
  capabilities enabled. Generated servers must not enable HTTP or filesystem schema
  retrieval.
- `serde`, `serde_json`, `http`, and `tower` are reused where already required.

Exact feature names are verified against the resolved crate before dependency
fragments are changed.

## Schema preparation

The generator owns schema normalization and bundling:

- OpenAPI 3.1 and 3.2 schemas retain JSON Schema 2020-12 semantics.
- OpenAPI 3.0 schemas are normalized before runtime compilation, including nullable
  and OpenAPI-specific vocabulary that does not directly map to JSON Schema.
- Local and external references are resolved during generation and embedded in the
  generated crate. A missing or unsupported reference is a generation error.
- Each selected operation receives a stable validator identifier. Validators are
  compiled once with `OnceLock`/`LazyLock`, not per request.
- JSON Schema format validation is explicitly configured. Regex handling must avoid
  introducing an unbounded backtracking engine into the request path.

The first implementation may emit normalized schemas as private `serde_json::Value`
constants and compile them lazily. A later optimization may generate direct Rust
checks, but must preserve the same public error contract.

## Validation surface

Validation covers:

- JSON request bodies: requiredness, composition, type, enum/const, numeric and
  string limits, object properties, array limits, and formats supported by the
  configured schema engine.
- Path, query, header, and cookie parameters after OpenAPI serialization parsing.
- Content type and supported request-body encodings.
- Existing typed deserialization errors, which are normalized into the same public
  problem family.

Form and multipart validation require media-specific extraction before schema
validation. Unsupported OpenAPI serialization combinations fail generation rather
than silently weakening validation.

## Public error contract

Responses use RFC 9457 Problem Details and `Content-Type:
application/problem+json`. The stable generated model includes:

```json
{
  "type": "https://openapi-to-rust.dev/problems/validation",
  "title": "Request validation failed",
  "status": 422,
  "code": "request_validation_failed",
  "errors": [
    { "code": "required", "location": "/body/name", "message": "is required" }
  ]
}
```

Status taxonomy:

| Status | Public code | Meaning |
| --- | --- | --- |
| 400 | `malformed_request` | Invalid JSON or parameter encoding |
| 413 | `request_body_too_large` | Configured body limit exceeded |
| 415 | `unsupported_media_type` | Content type is absent or unsupported |
| 422 | `request_validation_failed` | Syntactically valid input violates the contract |
| 500 | `generated_contract_error` | Generator/runtime contract mismatch |

Public errors are allowlisted, not copied from `jsonschema`, Axum, Serde, or HTTP
parser display strings. Locations use escaped JSON Pointer tokens rooted at
`/body`, `/path`, `/query`, `/header`, or `/cookie`. At most a configured number of
violations are returned; ordering is deterministic.

Internal diagnostics may log the underlying error with a request correlation ID,
but public responses do not include raw values, raw schemas, Rust type names,
filesystem paths, backtraces, or dependency versions.

## Configuration

The server configuration gains a validation section with secure defaults:

```toml
[server.validation]
enabled = true
max_body_bytes = 2097152
max_errors = 16
```

Validation is on for generated servers by default. A deliberate opt-out may exist
for compatibility, but generation emits a visible warning because it weakens the
OpenAPI contract. Limits are bounded and validated during config parsing.

## Client behavior

Generated clients keep documented operation errors intact. When a response has
`application/problem+json`, the transport error exposes a typed generated
`ProblemDetails` value when decoding succeeds and a sanitized fallback when it does
not. The client must not assume every non-2xx response is a validation problem.

## Security and performance invariants

- No runtime network or filesystem schema resolution.
- No rejected values in response bodies or default logs.
- Body size is enforced before allocation of an unbounded buffer.
- Validation errors are capped and deterministically ordered.
- Validators are compiled once and shared safely.
- Schema compilation failures are detected in generated-code tests and return a
  generic 500 if somehow encountered at runtime.
