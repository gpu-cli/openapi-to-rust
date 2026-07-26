# Phase 0: Axum 0.8 Foundation

## Objective

Make generated server code and exact dependency fragments consistently target the
Axum 0.8 line, directly fixing issue #38 before validation is added.

## Work

- Change generated Axum dependency fragments from `0.7` to `0.8`.
- Replace `#[axum::async_trait]` with a supported generated trait strategy and add
  its exact dependency only when server generation requires it.
- Audit router, extractor, response, SSE, and test APIs for Axum 0.8 changes.
- Centralize OpenAPI-path to Axum-path conversion and reject unsupported or malformed
  templates during generation.
- Add a compile-and-serve regression using a real parameterized route such as
  `/pets/{pet_id}`; a static route is insufficient.
- Update examples and expected dependency fixtures.

## Acceptance

- The generated fixture declares `axum = "0.8"` and compiles without compatibility
  shims from Axum 0.7.
- A request to a generated `{parameter}` route reaches the intended handler and
  binds its value.
- Existing JSON, typed response, custom-method, and SSE server tests remain green.

## Validation

```bash
cargo nextest run -p openapi-to-rust -E 'test(/server|generation_requirements/)'
cargo check -p openapi-to-rust
```
