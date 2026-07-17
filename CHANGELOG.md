# Changelog

All notable changes are recorded here. This project follows semantic versioning,
with one pre-1.0 qualification: a minor release may change generated Rust APIs
when correcting output that was wrong or incomplete on the wire.

## [Unreleased]

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

[Unreleased]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.6.0...HEAD
[0.6.0]: https://github.com/gpu-cli/openapi-to-rust/releases/tag/v0.6.0
[0.5.3]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.2...v0.5.3
[0.5.2]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.1...v0.5.2
[0.5.1]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/gpu-cli/openapi-to-rust/tree/v0.5.0
