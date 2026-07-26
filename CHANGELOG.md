# Changelog

All notable changes are recorded here. This project follows semantic versioning,
with one pre-1.0 qualification: a minor release may change generated Rust APIs
when correcting output that was wrong or incomplete on the wire.

## [0.10.0] - 2026-07-26

### Added

- Every-PR compatibility coverage for the generated Anthropic Messages server
  through the pinned official Python SDK, including unary and SSE responses.

### Changed

- Regenerated server response enums now use the declared status in bodyless and
  SSE variant names and require a runtime status for wildcard/default variants.
  This is a source-breaking correction for existing server trait implementations.

### Fixed

- Config-driven `server list` and `server add` now apply
  `generator.schema_extensions`, so overlay-provided operations and SSE media
  types match generation.
- Schema extensions accept the documented JSON, YAML, and YML formats with
  path-rich parse errors.
- Generated server response enums retain reusable Response Object references,
  including structurally compatible local refs stored outside
  `components.responses`, plus bodyless status codes, vendor/problem JSON media
  types, SSE status codes, and runtime status values for wildcard/default
  responses.
- Server generation rejects response sets that contain only unsupported media
  types and reports normalized Rust identifier collisions between distinct tags.
- Server example tests use Cargo's current integration-test binary instead of
  a potentially stale hard-coded `target/debug` executable.

## [0.9.1] - 2026-07-26

### Fixed

- Restored client generation for operations whose selected JSON or form request
  content declares no schema. These operations keep their historical no-body
  client signature, while server generation fails with an actionable error
  because there is no request contract to validate.

## [0.9.0] - 2026-07-26

### Added

- Default-on request validation for generated Axum servers, compiled offline
  from the selected OpenAPI/JSON Schema contract with bounded body and error
  limits.
- Sanitized `application/problem+json` responses for malformed input (`400`),
  oversized bodies (`413`), undeclared media types (`415`), schema violations
  (`422`), and generated contract mismatches (`500`).
- Typed validation for supported path, query, header, cookie, JSON, and
  form-urlencoded inputs, plus lazy `ApiError::problem_details()` decoding in
  generated clients without replacing documented typed errors.
- Live generated-client/server and independent-client compatibility tests that
  verify status codes, stable JSON Pointer locations, redaction, deterministic
  error caps, and handler isolation.

### Changed

- Generated servers and exact dependency fragments now target Axum 0.8 and its
  `{parameter}` route syntax consistently. Trait implementations use the direct
  `async-trait` dependency because Axum 0.8 no longer re-exports the attribute
  macro (#38).
- Server request constraints now use `jsonschema` 0.49 with remote file/HTTP
  resolution disabled. Model types remain free of validation derives.
- Unsupported aggregate parameter encodings and selected multipart, text, or
  octet-stream server bodies fail generation explicitly instead of being
  silently omitted.

### Fixed

- OpenAPI schema serialization now omits absent optional keywords while
  preserving an explicit `const: null`, preventing missing keywords from
  becoming unintended null constraints or otherwise disabling validation.
- Vendor JSON media types are retained end to end, and generated servers match
  the media type selected from the operation rather than accepting every
  `application/*+json` body.

## [0.8.0] - 2026-07-19

### Added

- An in-browser WASM playground at
  [openapi-to-rust.dev/playground](https://openapi-to-rust.dev/playground):
  paste a spec or fetch one by URL and get the exact generated file set —
  byte-identical to `openapi-to-rust generate <SOURCE>` — with a downloadable
  runnable crate.
- A default-on `cli` feature gating clap and reqwest. With
  `--no-default-features` the library compiles on `wasm32-unknown-unknown`;
  URL policy and spec parsing moved into the shared `spec_source` module.

### Fixed

- `ApiError` display output now bounds large response-body previews and includes
  typed error details or typed-body parse failures when available (#29).

## [0.7.0] - 2026-07-17

### Added

- Direct generation from a local OpenAPI document or bounded HTTPS URL:
  `openapi-to-rust generate <SOURCE>`.
- `openapi-to-rust init <SOURCE>`, plus deterministic `--dry-run`, `--check`,
  `--quiet`, and `--json` generation modes.
- Optional `[client].operations` selection and model pruning shared with the
  server operation scope.
- `Default` for all-optional request models, required-field constructors,
  fluent optional setters, and opt-in operation builders.
- A complete `REQUIRED_DEPS.toml` for the exact generated output.
- `base64_url_unpadded` as a spec-wide `format: byte` strategy for RFC 7515
  URL-safe, unpadded data.
- Contributor, support, security, conduct, issue-form, and pull-request
  scaffolding, plus a docs.rs library overview and compile-checked example.
- The public CLI as Cargo's default binary, so plain `cargo run -- ...` works
  even though feature-gated internal maintenance binaries are declared.

### Fixed

- Array items with an inline string enum now generate a named enum
  (`{Parent}Item`) instead of collapsing to `Vec<String>`, including
  `anyOf`-nullable arrays and typeless OpenAPI 3.1 enums (#33).
- README compatibility, corpus, and conformance claims; pull-request and
  scheduled full-corpus CI tiers; and release preflight checks.
- Canonical `[generator.types]` configuration parsing, strict unknown-field
  rejection, config-relative paths, and actionable migration errors.
- `cargo install --locked openapi-to-rust` packaging: only the public CLI is
  installed, packaged inputs are complete, and obsolete duplicate dependency
  versions were removed.
- Server query extraction now mirrors generated client serialization for typed
  form, repeated-array, comma-delimited, and deep-object query parameters.
- Generated client requests now support non-JSON bodies, optional bodies,
  typed headers, path encoding, and collision-safe operation signatures.

## [0.6.0] - 2026-07-13

### Added

- OpenAPI `style`/`explode`-aware client serialization for object and array
  query parameters, including form-exploded objects, comma-joined form values,
  deep-object parameters, and repeated arrays.
- Shared-target full-corpus compile tooling in `scripts/spec-compile.sh`.

### Changed

- Regenerated client signatures use typed objects and arrays instead of opaque
  `Option<impl AsRef<str>>` arguments for the supported query styles. This is a
  source-breaking correction for regenerated pre-1.0 clients.

## [0.5.3] - 2026-07-11

- Added working generated serde codecs for `time::Date` and `time::Time`.

## [0.5.2] - 2026-07-11

- Honored integer and number formats for query and path parameters.

## [0.5.1] - 2026-07-07

- Restricted the crates.io package to the source, manifest, README, and license.

## [0.5.0] - 2026-07-07

### Added

- Opt-in Axum server generation with operation selectors, per-tag traits,
  typed response enums, router factories, SSE response support, and model
  pruning.
- OpenAPI 3.1 modeling and experimental parsing for selected OpenAPI 3.2
  fields and methods.
- Typed scalar strategies, typed `additionalProperties`, operation-level typed
  errors, strict extension parsing, webhook ingestion, and SSE auto-detection.
- End-to-end OpenAI Responses and Anthropic Messages server examples.

### Fixed

- Numerous real-spec generation failures involving operation identifiers,
  signed enum values, recursive unions, parameter collisions, optional request
  bodies, range response codes, and path-segment encoding.

[Unreleased]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.10.0...HEAD
[0.10.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.9.1...v0.10.0
[0.9.1]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.9.0...v0.9.1
[0.9.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.8.0...v0.9.0
[0.8.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/gpu-cli/openapi-to-rust/releases/tag/v0.6.0
[0.5.3]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/gpu-cli/openapi-to-rust/tree/v0.5.0
