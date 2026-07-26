# Phase 1: Validation and Error Contract

Depends on Phase 0.

## Objective

Add the shared generated runtime foundation: secure configuration, embedded schema
registration, normalized violations, and RFC 9457 responses.

## Work

- Add and validate `[server.validation]` configuration with secure defaults.
- Preserve or derive the raw operation schemas and parameter constraints needed by
  server generation.
- Normalize OpenAPI 3.0 schema keywords and bundle all selected references at
  generation time; retain 2020-12 behavior for OpenAPI 3.1+.
- Emit private, lazily compiled `jsonschema` validators with network and filesystem
  resolution unavailable.
- Generate `ProblemDetails` and `InvalidParameter` types plus an `IntoResponse`
  rejection wrapper.
- Map only allowlisted validator keywords to stable codes and safe messages.
- Cap, sort, and JSON-Pointer-escape all returned violations.
- Add unit and snapshot tests for status, content type, pointer escaping, error caps,
  deterministic ordering, and redaction of secrets/internal strings.

## Acceptance

- Validation can be enabled without recompiling a schema on every request.
- All generated problem responses conform to one documented shape.
- Tests prove raw input values and raw dependency errors cannot appear in public
  responses.
- Invalid limits and unresolved schemas fail during configuration or generation.

## Validation

```bash
cargo nextest run -p openapi-to-rust -E 'test(/server.*valid|config|analysis/)'
cargo check -p openapi-to-rust
```
