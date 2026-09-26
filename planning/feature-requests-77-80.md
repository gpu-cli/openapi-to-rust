# Investigation of feature requests #77–#80

Reviewed against `36660a728e3c57d521bbf18a42994827c07d1d39` (0.18.0) on 2026-09-25.
This is an implementation recommendation, not an implemented feature contract.
Implementation work is tracked in Beads; the upstream requests remain open.

## Recommendation and order

| Request | Recommendation | Scope | Bead |
| --- | --- | --- | --- |
| [#77: multipart filenames](https://github.com/gpu-cli/openapi-to-rust/issues/77) | Implement first | Small additive client feature after shared method/argument planning | `openapi-generator-5g5` |
| [#78: response representations and binary streams](https://github.com/gpu-cli/openapi-to-rust/issues/78) | Implement in stages | Larger analysis and transport change; preserve current default methods | `openapi-generator-lgq` |
| [#79: Overlay 1.1](https://github.com/gpu-cli/openapi-to-rust/issues/79) | Implement independently | Separate preprocessing engine, then optional effective-spec output | `openapi-generator-qer` |
| [#80: binding metadata](https://github.com/gpu-cli/openapi-to-rust/issues/80) | Implement after the planning foundation | Opt-in versioned artifact and library API with explicit coverage | `openapi-generator-ai9` |

The shared planning foundation is `openapi-generator-ee6`.
Introduce a modest client method plan first. It should own final method and argument
names, source operation identity, request construction, response representation,
and consumption mode. Use it for rendering, builders, filename variants, and
eventually metadata. Avoid a wholesale generator rewrite. #80 need not wait for
all of #78, provided representation identity is designed into the initial plan.

## Evidence from current generation

A scratch OpenAPI document combined the multipart fragment from #77, the mixed
JSON/SSE/binary response from #78, a binary-only download, and the enum parameter
and model from #80. The existing 0.18.0 CLI generated it successfully:

```rust
create_upload(request: CreateUploadRequest) -> Result<(), ApiOpError<serde_json::Value>>
render() -> Result<RenderResponse, ApiOpError<serde_json::Value>>
download() -> Result<bytes::Bytes, ApiOpError<serde_json::Value>>
get_item(item_id: impl AsRef<str>, mode: Option<GetItemMode>)
    -> Result<Item, ApiOpError<serde_json::Value>>
```

These are abbreviated signatures; all methods are async and receive `&self`.
Multipart rendering contains no `Part::file_name`. `render` exposes only JSON.
`download` uses `__read_bounded_response_body`, so it buffers. Output consists of
`types.rs`, `client.rs`, `mod.rs`, and `REQUIRED_DEPS.toml`, without binding metadata.
Two model properties, `item-id` and `item_id`, become `item_id: Option<String>` and
`item_id_2: Option<i64>` with their original serde wire names. This demonstrates why
metadata must follow actual emission rather than just serialize source schemas.

## #77: request-local multipart filenames

The gap is in `generate_typed_multipart_form` in
[client_generator.rs](../src/client_generator.rs). Binary parts are created with
`Part::bytes(value.to_vec())`, without a filename. Existing nullable/optional
branches already decide when a part is present, so filename handling can be
inserted at part construction without changing request models or client state.

Recommended API: preserve the base operation and add a method such as
`create_upload_with_multipart_filenames(request, &[ ("document", "report.pdf") ])`.
The borrowed slice is simple, requires no new runtime dependency, and is owned by
the individual call. Allocate its method and argument names through the shared
plan. The example spelling is a preference, not a semantic identifier.

Only typed multipart operations with eligible binary fields need the extra method.
Use OpenAPI wire names for lookups. Reject duplicate keys and unknown/non-binary
keys with an inspectable configuration error. An override for a declared but
absent optional field should not create a part. Unspecified fields retain their
existing behavior. Builders should offer a request-local setter and delegate to
the same plan rather than introduce a second implementation.

The [referenced fork PR](https://github.com/adriendellagaspera/openapi-to-rust/pull/2)
is a useful design starting point, but should be ported selectively:

- It reserves real operation names before allocating additive method names.
- It appends a fixed `multipart_filenames` argument without reserving an existing
  parameter of that name; that spec would produce duplicate Rust arguments.
- It applies filenames only to `RawBytes`. With the conservative string mapping,
  a wire `format: binary` field is classified as text and its override is ignored.
  Decide filename eligibility from the wire schema and support that mapping too.
- Its test depends on multipart capability work in its fork base, and the patch
  includes corpus manifest churn. Neither belongs in a blind cherry-pick.

Verification should compile generated methods and builders and inspect multipart
requests on a local server: two independent filenames, renamed fields, an omitted
optional binary field, byte and conservative string mappings, concurrent calls,
method/argument collisions, and unchanged base-method wire behavior. The fork's
source-string assertions alone do not establish these properties.

## #78: retain representations, then expose consumption modes

`OperationResponse` in [analysis.rs](../src/analysis.rs) retains one preferred body
per response status, plus an SSE boolean. `analyze_single_operation` chooses one
JSON schema or one text/binary body. Thus adding renderer variants alone cannot
recover all media types or their potentially distinct schemas.

First add a representation collection keyed by response status and exact media
type. Preserve existing preferred-body fields and selection so current client and
server output remain compatible. Analyze alternate inline schemas with allocated
names, add their dependencies to pruning roots, and update schema-name rewrites.
Preserve unsupported-media diagnostics rather than treating every media type as
decodable bytes. The server currently consumes the preferred response view and
can continue doing so in this client feature.

Then plan additional methods by representation and consumption mode. JSON, text,
buffered binary, parsed SSE, and live binary are explicit plan values, independent
of generated suffixes. Each method shares the source operation, parameters,
request serializer, authentication, cookies, and error schema with the base call.
Set an appropriate concrete `Accept` header and define response Content-Type
validation, including wildcard behavior. Two distinct JSON media types may carry
different schemas, so a single generic `_json` variant is not always sufficient.

Keep status contracts attached to representations. When the same media type has
incompatible success schemas at different statuses, use a status-tagged result or
a clear unsupported-case diagnostic for the new API. A method name cannot force
the server to choose a status. Preserve the existing base method's status checks.

For live binary, validate success before consuming the body and return
`impl Stream<Item = Result<bytes::Bytes, reqwest::Error>>` from
`response.bytes_stream()`. This matches an existing primitive in the client;
[reqwest documents the stream API](https://docs.rs/reqwest/latest/reqwest/struct.Response.html#method.bytes_stream).
Retain the buffered `bytes::Bytes` method and bounded error-body handling.
Document that a live success stream has no total buffered-body ceiling. Do not
add a blanket `Send` bound that would prevent wasm consumers.

SSE needs separate treatment. The existing SSE-only HTTP method returns raw byte
chunks; the configured `SseClient` supplies event parsing and reconnection through
`stream_raw`, `stream_json`, and their reconnecting variants. Preserve both
existing APIs. Route new parsed SSE methods through that transport using the
shared request plan. Use raw SSE events when the spec does not identify a typed
event payload; retain explicit configuration for typed event unions and request
stream switches. Declaring SSE does not specify how to set `stream: true`.

Release stages can be complete representation analysis and buffered variants,
live binary, then parsed SSE integration. Acceptance must cover real negotiation,
alternate inline schemas retained after pruning, typed errors, existing methods,
collision-safe builders/variants, and native/wasm compilation. A delayed chunked
server must prove that the binary method returns and yields the first chunk before
the final chunk is sent; a small finite-response test would not prove streaming.

## #79: an independent Overlay preprocessing stage

`merge_schema_extensions` has project-specific union replacement rules and allows
replacement across JSON value kinds. It must remain available unchanged. It is
not an Overlay engine.

The [Overlay 1.1 specification](https://spec.openapis.org/overlay/v1.1.0.html)
requires sequential actions, RFC 9535 JSONPath, recursive object merges, array
concatenation/appending, primitive replacement, and compatible value kinds.
An unmatched target is a successful no-op. A copy selects one source node.
These rules need dedicated conformance cases, including type conflicts and
array-element removals. Resolve ambiguous combined action fields against the
standard rather than inventing update/copy precedence.

Proposed order: load the raw document, apply existing `schema_extensions` in their
current order, apply configured overlays in order, validate the effective OpenAPI
document, derive server defaults, then analyze and generate. This lets an overlay
remove something an extension added. Currently `run_generate` validates and reads
server defaults before extension merging; preprocessing should move ahead of
those decisions. Share this path with relevant CLI analysis commands and expose
it for library consumers. Materialize before analyzer name normalization mutates
the document.

Add explicit `[generator].overlays` paths resolved by `ConfigFile::load` relative
to the configuration, with no implicit reinterpretation of `schema_extensions`.
An optional effective-spec filename inside the output directory can join the
existing artifact map, preserving `--check` and `--dry-run` behavior. Deterministic
JSON is a reasonable first output format. Both the artifact and the analyzer must
consume the same transformed value. Treat `extends` as declared target identity;
do not silently replace the configured input or introduce automatic fetching.

Evaluate `serde_json_path` rather than writing a JSONPath parser. Its
[documented located-query API](https://docs.rs/serde_json_path/0.7.2/serde_json_path/)
and [normalized JSON Pointer conversion](https://docs.rs/serde_json_path/0.7.2/serde_json_path/struct.NormalizedPath.html)
are useful for mutation, but its stated intent to follow RFC 9535 is not proof of
full compliance. Verify the official JSONPath compliance corpus, required
functions, MSRV, and wasm compatibility before choosing it.

Snapshot target locations for an action, copy its source value before mutation,
and remove array indices in descending order within each parent. Define and test
duplicate/overlapping selections and root removal; these need particular care.
Errors should identify the overlay file and action index. Also verify ordered
files, configuration-relative paths, effective server defaults, and stale/missing
materialized output. Do not advertise full 1.1 support for a JSONPath subset.

## #80: metadata from emission decisions

The existing operation registry describes HTTP operations, not exact Rust
bindings. The client-sync manifest describes generation inputs. Neither can
substitute for this request. `EmittedObjectProperty` in
[generator.rs](../src/generator.rs) is already a shared projection for field
identifiers and types, while client method rendering and builders currently make
some decisions separately. Extend these planning boundaries rather than parsing
generated Rust in production or exporting `SchemaAnalysis` as the public format.

A proposed `bindings.json` v1 includes schema and generator versions; module and
re-export paths; emitted structs, fields, enums and payloads, aliases, and public
helper symbols; method receiver, argument names/types, generics and bounds, async
signature and result type; source operation identity; and explicit response
media/status/consumption descriptors. Record serde wire names, `Option` nesting,
recursive `Box`, user type mappings, and actual collision suffixes. Preserve
signatures using the same token/AST values used for rendering.

Capture source identity during operation ingestion, before ID disambiguation and
path normalization: source location such as `/paths/~1items~1{item-id}/get`, HTTP
method, original path or webhook key, and original optional `operationId`.
`operation_id_aliases` helps today but is not a complete source locator. Multiple
generated methods share that identity without inferring semantics from their
Rust names. Refer to the effective document when overlays are used.

Provide an additive library entry point returning an existing `GenerationResult`
plus metadata in a new wrapper. Adding a public field to `GenerationResult`
would break existing exhaustive struct literals even if the field were optional.
Opt-in CLI metadata should join the artifact map without entering generated
module exports or dependency inference. Keep sorting stable and omit timestamps
and machine-dependent paths.

Start with models and HTTP client output, including enabled builders, parameter
enums, error enums, and public runtime symbols. Declare coverage in the format
and reject unsupported generation modes during the initial rollout; silently
claiming complete bindings while omitting server/registry/SSE symbols would be
misleading. Extend coverage in later versions without changing Rust output.
Resolve this coverage contract before freezing the schema.

Verify deterministic output and config/type/selection/pruning variations. A
test may parse generated Rust to compare its public symbols and signatures with
metadata; production extraction should still use the shared emission plans.
Keep metadata-off output byte-identical, check CLI stale-output behavior, and
validate distinct method variants against their common source operation.

## Validation performed

The scratch CLI generation above succeeded. All 24 existing
`operation_generation_test` cases and the generated-client runtime test in
`non_json_response_test` passed. Test execution first hit sandbox restrictions on
the compiler cache and nested Cargo network access; the successful retry disabled
the compiler wrapper, used cached dependencies, and allowed the local HTTP server:

```sh
RUSTC_WRAPPER= CARGO_NET_OFFLINE=true cargo test --test operation_generation_test --test non_json_response_test
```

These verify the existing baseline, not the proposed features. No generator
implementation was changed.
