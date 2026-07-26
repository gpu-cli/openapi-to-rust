# Phase 3: Parameter and Media Validation

Depends on Phase 1 and may proceed alongside Phase 2.

## Objective

Validate non-body request inputs and correctly extract supported non-JSON media
types without weakening the OpenAPI contract.

## Work

- Carry parameter schemas, requiredness, style, explode, and allow-reserved metadata
  from analysis into server code generation.
- Generate typed path, query, header, and cookie extraction with stable locations in
  failures.
- Validate scalar and aggregate parameter constraints after OpenAPI serialization
  parsing.
- Replace string-only header extraction with generated types where the specification
  permits typed conversion.
- Correct form-urlencoded handling so it uses form extraction rather than JSON.
- Add explicit multipart, text, and octet-stream behavior; fail generation for
  unsupported combinations instead of ignoring bodies.
- Normalize malformed encoding to 400 and schema violations to 422.

## Acceptance

- Required, malformed, and schema-invalid path/query/header/cookie cases have distinct
  sanitized failures.
- Form-urlencoded requests use their declared transport and validate successfully.
- No selected request body is silently omitted from a generated trait method.

## Validation

```bash
cargo nextest run -p openapi-to-rust -E 'test(/server.*query|server.*route|server.*header|server.*form|validation/)'
```
