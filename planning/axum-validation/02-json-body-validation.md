# Phase 2: JSON Body Validation

Depends on Phase 1.

## Objective

Guarantee that a syntactically valid but schema-invalid JSON request never reaches
the generated server trait implementation.

## Work

- Add a generated JSON extractor that enforces the body limit and content type,
  parses JSON once, validates the value, then deserializes the typed model.
- Preserve the distinction among malformed JSON (400), oversized bodies (413),
  unsupported media type (415), and schema violations (422).
- Validate required bodies and optional/null bodies according to the operation.
- Cover `$ref`, composition, enum, const, additional properties, arrays, numeric and
  string bounds, patterns, and configured formats.
- Ensure deserialization mismatches not expressible by the runtime schema map to a
  sanitized generated-contract response.
- Verify that the handler is not invoked on every rejection path.

## Acceptance

- Valid JSON reaches the implementation as the existing generated Rust model.
- Invalid JSON produces the documented sanitized problem response and does not call
  the implementation.
- Reference-heavy OpenAPI 3.0 and 3.1 fixtures compile and validate offline.

## Validation

```bash
cargo nextest run -p openapi-to-rust -E 'test(/server.*body|validation|fixture/)'
```
