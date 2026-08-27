# Changelog

All notable changes are recorded here. This project follows semantic versioning,
with one pre-1.0 qualification: a minor release may change generated Rust APIs
when correcting output that was wrong or incomplete on the wire.

## [Unreleased]

### Added

- The 55-spec compile gate now generates deterministic JSON instances for
  representable component schemas, validates them against the source JSON
  Schema, hydrates and serializes the exact generated Rust models, validates
  their output, and requires a stable second round trip. Targeted runs such as
  `scripts/spec-compile.sh anthropic` use the same gate and report sample and
  skip coverage.

### Changed

- **Breaking (generated API).** Structs used as discriminated-union variants
  retain their discriminator fields, so constructing one directly now requires
  the same tag its standalone component schema requires. Parent unions perform
  explicit discriminator-directed Serde dispatch instead of stripping the
  field and relying on an internally tagged derive. This keeps direct values,
  arrays, and union payloads on one schema-valid wire shape.

### Fixed

- Required nullable fields serialize `None` as explicit JSON `null` instead of
  omitting a key listed by the schema's `required` array. Nullable component
  schemas referenced by a property now propagate that nullability to the field.
- A composition nested as one branch of an outer `anyOf` is retained as a named
  Rust union variant instead of being silently dropped.

- Boolean subschemas (`true` and `false`) parse wherever JSON Schema 2020-12
  allows one — a property, a `$defs` entry, `not`, `if`/`then`/`else`,
  `contains`, `propertyNames`, `patternProperties`, `dependentSchemas`, a
  `oneOf` branch. `properties: {extra: true}` is how a spec says "this key
  exists, any value"; one of those anywhere in a document used to fail the whole
  thing with "data did not match any variant of untagged enum Schema" (#63).

  `true` generates `serde_json::Value` and `false` a value that cannot occur —
  both reported as faithful by `--report-untyped`. In a union, a `true` branch
  makes the union unconstrained and a `false` branch is dropped, so
  `oneOf: [A, false]` is `A`.
- Integer keywords written as decimals — `maxItems: 2.0`, which JSON Schema
  permits and the 2020-12 suite exercises — are read as the counts they are
  rather than rejecting the document. A fractional value like `2.5` is still an
  error.

  Together these take the vendored JSON Schema 2020-12 corpus from 38 parse
  failures to zero, with no round-trip loss.


## [0.14.0] - 2026-08-27

### Added

- `openapi-to-rust generate --report-untyped` reports every generated field that
  carries `serde_json::Value`, grouped by why, and marks each **faithful** (the
  schema declared an unconstrained value) or **recoverable** (the generator
  dropped type information the schema carried). `--json` emits the findings with
  paths for tooling.
- `scripts/untyped-census.sh` runs that across `specs/` and rewrites
  `tests/conformance/untyped-report.md`, so a change that alters which fields
  get typed shows its corpus delta in review; `--check` fails when the report is
  stale.
- Generated extensible enums now expose `as_str` and implement `Display` and
  `AsRef<str>`, matching what generated string enums already had. Needed because
  a multipart form field, query parameter, or header can now be one.

### Changed

- **Breaking (generated API).** Positional item schemas — 2020-12
  `prefixItems` and the draft-04 `items: [A, B]` spelling — now generate typed
  Rust tuples instead of `Vec<serde_json::Value>`, when the spec pins the
  array's length (`minItems`/`maxItems`, `items: false`, or
  `additionalItems: false`). A `[string, integer]` pair becomes
  `(String, i64)`; a `$ref` position keeps its named type, and an inline object
  position is hoisted to one. When no extras are allowed but the length varies
  and every position shares a type, the array becomes `Vec<T>`.

  An *open* `prefixItems` still generates `Vec<serde_json::Value>` on purpose:
  it permits extra elements of any type, and a fixed-arity tuple would reject
  payloads the spec allows (#62).

- **Breaking (library API).** `SchemaType` gained `Untyped { shape, reason }`,
  which replaces the stringly-typed `Primitive { rust_type: "serde_json::Value" }`
  fallbacks and carries why a value could not be typed, and `Tuple {
  element_types }` for fixed-length positional items. `SchemaType::Object`
  gained a `variant` field holding a union declared alongside its properties.
  Exhaustive matches and struct literals need updating.
- **Breaking (library API).** `Schema::OneOf` gained a `schema_type` field, so a
  union that also declares `type` keeps it; `Items` gained a `Bool` variant for
  2020-12 boolean schemas. `SchemaAnalysis::untyped_fields()` returns the
  census.

### Fixed

- Schemas that carried enough information to type no longer degrade to
  `serde_json::Value`. Across the 57-spec corpus this types every field the
  census could attribute to a dropped type — 3,347 of them — taking the total
  untyped surface from 13,226 fields to 8,829, all of which are schemas that
  genuinely declared an unconstrained value (#62, #65):
  - `anyOf: [$ref, {type: object, nullable: true}]` — how OData spells "that
    type, or null" — becomes `Option<T>` instead of an untyped union (2,127
    fields in Microsoft Graph alone);
  - an inline object, union, enum, or merged `allOf` in a field or element
    position is hoisted to a named type instead of being dropped by the
    generator, which could not render one inline;
  - `allOf` with a single member takes that member's type, and `allOf` inside
    array items is analyzed instead of ignored;
  - a `$ref` to any local JSON Pointer resolves — a parameter's schema, one
    member of another schema's composition — not only
    `#/components/schemas/<name>`;
  - `type: null` becomes `()`, which serde reads and writes as `null`;
  - a union whose branch list is empty takes the schema's declared type; a
    union of one branch is that branch; branches differing only in constraints
    share one type; branches that only alternate `required` describe the
    object their properties declare; and branches that are local pointers are
    expanded before the union is built;
  - a schema declaring `properties` *and* a union — "these fields, and one of
    these shapes" — generates the struct with the union in a
    `#[serde(flatten)]` field, instead of discarding both halves (#65).
- `items: false` and `items: true` — 2020-12 boolean schemas, and the canonical
  way to close a tuple — now parse instead of failing the document with "data
  did not match any variant of untagged enum Schema" (#62).

- Reference cycles that run through a synthesized type — a hoisted union, a
  hoisted property type — are detected, so the generated enum is boxed instead
  of having infinite size. Typing a field that was previously
  `serde_json::Value` can close a cycle the untyped value had been breaking by
  accident.

## [0.13.0] - 2026-08-26

### Changed

- **Breaking (library API).** `SchemaDetails.items` is now `Option<Items>`
  rather than `Option<Box<Schema>>`, so the keyword can hold either the
  2020-12 single-schema form or a draft-04 positional tuple. Read the former
  through `SchemaDetails::item_schema()` and the latter — unified with
  `prefixItems` — through `SchemaDetails::positional_items()`. Generated code
  is unaffected.
- **Breaking (library API).** `GeneratorError` gained a `ParseErrorAt`
  variant carrying the JSON Pointer of a located parse failure. Exhaustive
  matches over the enum need a new arm.

### Fixed

- The draft-04 positional tuple form `items: [A, B]` — still emitted under
  `openapi: "3.1.0"` by FastAPI/pydantic v1 — now parses instead of failing the
  whole document, generating what the 2020-12 spelling `prefixItems: [A, B]`
  generates. Generated Axum validators receive the canonical spelling, so the
  positions are actually checked at runtime (#60).
- Document parse failures now name the offending node by JSON Pointer, e.g.
  `Failed to parse OpenAPI spec at #/components/schemas/Body/properties/pair/items`,
  instead of reporting only "data did not match any variant of untagged enum
  Schema" with no way to find it in a large spec (#60).

## [0.12.3] - 2026-08-22

### Fixed

- OpenAPI 3.1 schemas with multiple non-null types now generate proper Rust
  unions instead of being treated as nullable versions of their first type;
  array and object members retain their declared shapes.

## [0.12.2] - 2026-08-18

### Fixed

- Schemas that pair `allOf` with a redundant sibling `type: object` are now
  parsed as compositions instead of plain typed objects, so the composed
  members are merged into the generated struct rather than dropped.

## [0.12.1] - 2026-08-07

### Fixed

- Distinct OpenAPI component keys that normalize to the same Rust identifier
  are deterministically disambiguated instead of silently dropping a model;
  references, discriminator mappings, dependencies, and operation schemas are
  rewritten to the emitted names.
- Implicit discriminators are selected only when every union branch has a
  unique constant value. Ambiguous unions now remain untagged, preserving
  nested constant fields so Serde can distinguish branches at runtime.

### Changed

- The real-world compile corpus now includes the OpenCode OpenAPI 3.1 document.

## [0.12.0] - 2026-07-29

This release broadens the set of real-world protocols that generated clients
and servers can represent, and replaces the old inline SSE helpers with a
reusable typed transport. Regenerated clients may have new request/response
types, a new `ApiError::raw_body` field, and newer HTTP dependencies; review
generated-code diffs when upgrading.

### Added

- SSE-enabled output now includes a standalone `sse.rs` transport with
  `SseClient`, raw `SseEvent<String>` streams, typed JSON `SseEvent<T>` streams,
  and the backwards-compatible payload-only stream. Event name, ID, and
  server-provided `retry:` delay remain available to callers.
- SSE streams can reconnect with bounded exponential backoff, honor the
  server's `retry:` value, and send the most recently observed event ID as
  `Last-Event-ID`. HTTP 429, 5xx, connection failures, and early EOF are
  retryable; invalid content types and other terminal errors are not. The
  generated runtime was exercised against live OpenAI- and
  Anthropic-compatible streaming endpoints.
- Flat `multipart/form-data` object schemas generate typed reqwest clients and
  Axum extractors, including required and optional binary/scalar fields,
  configured body limits, validation, and deterministic rejection of shapes
  the generator cannot encode symmetrically.
- Generated clients and servers support bounded binary and text request bodies,
  including `application/octet-stream`, `application/pdf`, `text/plain`, XML,
  and `application/jwt`. Non-JSON responses preserve exact bytes, and
  `ApiError<E>::raw_body` exposes the unmodified response alongside its lossy
  text rendering.
- Buffered client responses and SSE error responses have an 8 MiB default
  limit, configurable through `http_client.max_response_body_bytes` and the
  generated runtime builders. Oversized bodies return `ResponseTooLarge`
  without buffering beyond the limit.
- Component-level Request Body Object references are resolved during operation
  analysis, so `requestBody: { $ref: ... }` participates in normal client and
  server generation.
- The checked-in corpus now includes Storyden, and the documentation includes a
  dated Progenitor workflow comparison with a reproducible compile benchmark.

### Changed

- Generated HTTP dependencies now target `reqwest` 0.13,
  `reqwest-middleware` 0.5, `reqwest-retry` 0.9, `reqwest-tracing` 0.7, and
  `thiserror` 2. Required reqwest and middleware features are inferred from the
  selected operations, including query, form, multipart, streaming, and JSON
  usage; rustls builds use reqwest 0.13's `rustls` feature.
- AWS query-protocol operations can use bounded nested object/array form
  encodings, while simple array headers and path parameters with literal
  prefixes or suffixes now have matching typed client/server serialization.
- Text and binary response media produce `String` and `bytes::Bytes` values
  instead of being forced through JSON or lossy UTF-8 conversion. Generated
  server response variants retain their declared media type.
- The real-world corpus contains 56 documents: 55 supported OpenAPI specs and
  one intentionally skipped Swagger 2.0 Gitea document. The ordinary full tier
  compiles 54 and reports Microsoft Graph as generate-only because its generated
  crate exceeds CI memory; `SPEC_COMPILE_FORCE_CHECK=1` enables local
  compile-verification on larger machines.

### Fixed

- Bodyless operations that declare request-content semantics send
  `Content-Length: 0`; ordinary methods without content semantics remain
  unchanged, and optional bodies add the header only when absent.
- Media selection recognizes PDF as binary and XML, `+xml`, and JWT as text,
  prefers schema-bearing vendor JSON over schema-less canonical JSON, and
  rejects wildcard or proprietary request media instead of emitting an invalid
  `Content-Type`.
- Recursive annotation-only `allOf` references are aliases again, avoiding
  expansion overflows, and generated server code consistently uses canonical
  Rust model names while avoiding response-enum name collisions.
- Spec parsing tolerates literal tabs in YAML block-scalar prose and ignores
  extension scalars parked inside `paths`. AWS route fragments are stripped and
  synthetic webhook paths receive a leading slash.
- Server-side validation normalizes common Java POSIX, ECMA Unicode, and legacy
  octal regex syntax; unsupported look-around/backreference patterns no longer
  abort generation. Component keys that resemble JSON Schema keywords are
  namespaced in generated validation bundles.
- Portable scratch directories and complete generated dependency fragments
  keep the expanded corpus and packaged examples compiling on clean CI runners.

## [0.11.0] - 2026-07-27

Nearly everything here was found by generating a client from RunPod's published
OpenAPI document and exercising it against the live API. The spec was accurate;
the generator was not. Two of these defects broke real calls outright, and both
would have passed any amount of spec-diffing.

### Changed

- `format: float` now maps to `f64` instead of `f32`. JSON carries no binary32,
  so the declared format describes the server's storage rather than the
  transport: a value sent as `0.03` survives in `f64` but becomes
  `0.029999999329447746` through `f32`, which matters when the field is money.
  Set `float_precision = "f32"` under `[generator.types]` to map strictly by
  declared format. `--types-conservative` keeps the literal `f32` mapping.

### Fixed

- Parameter-level inline enums honor `x-enum-varnames`. Schema-level enums
  already did, so the same enum produced different Rust variant names depending
  on whether it lived in `components.schemas` or on a parameter. A varnames
  array whose length disagrees with `enum` is ignored rather than applied to a
  prefix.
- Properties that are both `required` and nullable via OpenAPI 3.1's
  `type: ["X", "null"]` now generate `Option<T>` instead of a bare `T`, in plain
  object schemas and in `allOf`-composed ones alike. Previously such a client
  compiled and then failed to deserialize the first real response containing
  `null`. All three nullability spellings (`nullable: true`, the 3.1 type array,
  and an `anyOf`/`oneOf` null branch) now route through one helper.
- Client operations whose only success content is `text/event-stream` return a
  `futures_util::Stream` of bytes instead of `()`. They previously buffered the
  response with `.text()`, which never returns on a live SSE stream and hung the
  caller's task indefinitely.
- Generated clients default `base_url` to the document's `servers[0].url` when
  configuration does not set one, so `HttpClient::new()` targets the real API
  instead of an empty string. Explicit configuration still wins; relative and
  templated server URLs are ignored.
- The `Default(..)` per-operation error variant is now constructed for responses
  matched by the spec's `default` response. It was previously declared but
  unreachable, so a typed `default` body still surfaced as `typed: None`.
- Specs with `multipart/form-data` operations now request reqwest's `multipart`
  feature in `REQUIRED_DEPS.toml`. The feature was enabled for
  `reqwest-middleware` but not for `reqwest` itself, so generated file-upload
  clients failed to compile.

### Internal

- The `full-spec-compile` CI tier passes again. It had been killed with SIGTERM
  roughly 25 minutes into a 240-minute budget, on `main` as well as branches.
  The cause was memory, not time or disk: `microsoft-graph` generates 2.4M lines
  from 16,153 operations and peaks at ~14.3 GB in a single rustc process against
  a 16 GB runner. It is now generated but not compile-checked, reported in its
  own bucket so a green run is never mistaken for full corpus verification.
  `SPEC_COMPILE_FORCE_CHECK=1` checks it where there is headroom.

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

[Unreleased]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.12.0...HEAD
[0.12.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.11.0...v0.12.0
[0.11.0]: https://github.com/gpu-cli/openapi-to-rust/compare/v0.10.0...v0.11.0
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
