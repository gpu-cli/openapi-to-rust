# openapi-to-rust — OpenAPI generator for Rust

[![CI](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/openapi-to-rust.svg)](https://crates.io/crates/openapi-to-rust)
[![docs.rs](https://docs.rs/openapi-to-rust/badge.svg)](https://docs.rs/openapi-to-rust)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

`openapi-to-rust` is an OpenAPI generator for Rust that turns OpenAPI 3.0/3.1 (and experimental 3.2) specifications into strongly-typed structs, async HTTP clients, SSE streaming clients, **and opt-in Axum server scaffolding** — including for the messy, real-world specs everyone actually ships.

Read the [guides and documentation](https://openapi-to-rust.dev/) for the quickest path from an OpenAPI document to compiling Rust.

## 5-second trial

Paste your spec into the **[browser playground](https://openapi-to-rust.dev/playground)** —
the real generator compiled to WebAssembly. See the generated Rust instantly and
download it as a complete, compilable crate. Nothing is uploaded.

## 30-second trial

Install the CLI from crates.io, then generate a tiny client from a stable,
hosted fixture:

```bash
cargo install --locked openapi-to-rust
openapi-to-rust generate https://raw.githubusercontent.com/gpu-cli/openapi-to-rust/v0.8.0/tests/fixtures/operation_extraction/simple_get.json
```

This writes `src/generated/{types,client,mod}.rs` and the exact dependencies
needed to compile them in `src/generated/REQUIRED_DEPS.toml`. Use `--dry-run`
to inspect the plan without writing, and `--check` in CI to detect stale
committed output.

We originally built this internally at [GPU CLI](https://gpu-cli.sh) to generate typed Rust clients for OpenAI, Anthropic, Cloudflare, and other large APIs. After battle-testing it against real-world specs with complex union types, discriminated enums, streaming endpoints, and the occasional spec/API drift, we decided to open source it.

The repository contains **55 real-world specs**. The supported OpenAPI corpus is
54 specs (one Gitea document is Swagger 2.0 and intentionally skipped). Pull
requests compile-check the OpenAI and Anthropic production specs; a scheduled
and manually runnable CI tier checks all 54 OpenAPI specs.

Release history and breaking changes live in the [changelog](CHANGELOG.md).

## Highlights

- **OpenAPI 3.0 and 3.1, with experimental 3.2 support** — handles `type: ["X", "null"]`, `anyOf`/`oneOf`/`allOf`, discriminated unions, `const`, inline objects, and accepts paths-less specs (components-only or webhooks-only).
- **Generates clients *and* servers** — pick client calls with `[client]`, hosted operations with `[server]`, or keep the default all-operation client. Both share the same `types.rs`.
- **Typed scalars** — `format: date-time` → `chrono::DateTime<chrono::Utc>`, `uri` → `url::Url`, `binary` → `bytes::Bytes`, `uuid` → `uuid::Uuid`, `byte` → `Vec<u8>` + base64 codec, unsigned-int formats → `u32`/`u64`. All opt-out per-format in TOML.
- **Async HTTP client** — typed methods per operation, retry/backoff via `reqwest-retry`, distributed tracing via `reqwest-tracing`, Bearer / API-key / custom auth (honored at runtime), default headers, path-template percent-encoding.
- **Axum 0.8 server scaffolding** — trait per tag, status-code-typed response enum, SSE-ready `OkStream` variant, OpenAPI request validation, and a combined `build_router(...)` factory for multi-tag selections.
- **SSE streaming clients** — first-class Server-Sent Events with reconnection.
- **Smart discriminated unions** — auto-detects implicit discriminators from `const` properties, falls back to `#[serde(untagged)]` when a union mixes scalar and object branches (e.g. `"auto"` *or* a tagged object).
- **Per-operation typed errors** — each operation gets its own error enum with `Status4xx(...)` typed bodies; you can match on the exact API error shape.
- **Typed `additionalProperties`** — extra keys become `BTreeMap<String, T>` instead of falling to `serde_json::Value` when the spec gives a value-type schema.
- **Constraints at the server boundary** — `minLength`/`maxLength`/`minimum`/`pattern` remain useful model documentation, and selected Axum operations validate their request schemas at runtime with an offline `jsonschema` bundle.
- **TOML configuration** with overrides for spec quirks (nullable, extensible enums, type aliases).
- **Snapshot testing** — `insta` snapshots for generated output.
- **Optional `specta::Type` derives** for cross-language type sharing.

## Install

Rust users with Rust 1.88 or newer can install the CLI from crates.io:

```bash
cargo install --locked openapi-to-rust
```

This compiles the CLI locally and requires a Rust toolchain. Prebuilt binaries
are not currently published. If you use the generator as a Rust library instead,
run `cargo add openapi-to-rust`.

## Try it in one command

Generate types and an async client directly from a local document or HTTPS URL:

```bash
openapi-to-rust generate openapi.yaml
# Writes src/generated/{types,client,mod}.rs and REQUIRED_DEPS.toml.
```

The async client is the default. Use `--types-only` to omit it, `--dry-run` to
analyze without writing, or `--check` in CI to fail when committed output is
stale. Remote fetches require HTTPS (except loopback development), reject URL
credentials and redirects, and enforce time and response-size limits.

To keep a reusable config:

```bash
openapi-to-rust init openapi.yaml
openapi-to-rust generate
```

## Quick start — client

`openapi-to-rust.toml`:

```toml
[generator]
spec_path = "openapi.json"
output_dir = "src/generated"
module_name = "api"
# All paths above are relative to this TOML file, not the shell's working directory.

[generator.builders]
# Add `*_builder()` when an operation has more than three optional values.
enabled = true
threshold = 3

[features]
enable_async_client = true

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30

[http_client.retry]
max_retries = 3

[http_client.auth]
type = "Bearer"
header_name = "Authorization"
```

Then:

```bash
openapi-to-rust generate --config openapi-to-rust.toml
```

### Select only the client operations you use

`[client]` is optional. Without it—or with an empty `operations` list—the HTTP client keeps every operation for backward compatibility. Selectors share the server grammar: exact `operationId`, exact `METHOD /path`, or `tag:<name>`.

```toml
[client]
operations = [
  "createResponse",
  "GET /v1/models",
  "tag:Files",
]
prune_models = true
```

`prune_models = true` also trims `types.rs`. When client and server selections coexist, the generator retains the union of schemas reachable from both scopes, including inline generated request, response, and parameter types. `[client]` is ignored when `enable_async_client = false`, and it never filters an enabled operation registry.

## Quick start — Axum server

Pick the operations you want to host. The generator emits a trait, a typed response enum, and a router factory. You implement the trait; `axum` does the rest.

```toml
[generator]
spec_path = "openai.yaml"
output_dir = "src/gen"
module_name = "openai"

[features]
enable_async_client = false        # server-only

[server]
framework = "axum"
operations = ["createResponse", "listInputItems"]
# Drop schemas not reachable from the picked operations. Safe when
# you're not also generating the HTTP client (the client would lose types).
prune_models = true

[server.validation]
enabled = true                 # default
max_body_bytes = 2097152       # 2 MiB; 1..=64 MiB
max_errors = 16                # deterministic cap; 1..=100
```

Generated handlers validate JSON bodies and supported path, query, header,
cookie, and form inputs before invoking your trait. Public failures use
`application/problem+json`: malformed transport is `400`, oversized bodies are
`413`, undeclared media types are `415`, and schema violations are `422`.
Messages contain stable codes and JSON Pointer locations, never submitted values,
Serde/validator text, schema paths, or Rust internals. Generated clients preserve
documented typed errors and expose this profile lazily through
`ApiError::problem_details()`.

See [Generated Axum request validation](docs/server-validation.md) for the full
schema dialect, status, error profile, supported-input, and logging contract.

Discover and scaffold operations from the CLI:

```bash
# List operations in the spec (filter by tag / method / substring)
openapi-to-rust server list --tag Responses

# Add an operation to [server].operations (preserves TOML formatting)
openapi-to-rust server add createResponse
openapi-to-rust server add --all-tag Responses
openapi-to-rust server add createResponse --regenerate   # re-run codegen

# Remove
openapi-to-rust server remove createResponse
```

Then implement the trait:

```rust
use gen::server::{ResponsesApi, CreateResponseResponse, build_router, sse_response};
use gen::CreateResponse;
use axum::response::sse::Event;
use futures_util::stream;

#[derive(Clone)]
struct AppState;

#[async_trait::async_trait]
impl ResponsesApi for AppState {
    async fn create_response(&self, body: CreateResponse) -> CreateResponseResponse {
        if body.stream == Some(true) {
            // SSE branch — the generated `sse_response` helper takes any
            // Stream<Item = Result<Event, Infallible>> and returns the
            // exact payload the OkStream variant expects.
            CreateResponseResponse::OkStream(sse_response(stream::iter(vec![
                Ok(Event::default().event("response.created").data("{}")),
                Ok(Event::default().event("response.completed").data("{}")),
            ])))
        } else {
            // Unary branch — typed body, typed response.
            CreateResponseResponse::Ok(/* construct gen::Response */ todo!())
        }
    }
}

#[tokio::main]
async fn main() {
    let app = build_router(AppState);
    let lis = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    axum::serve(lis, app).await.unwrap();
}
```

Two complete examples are in the repo:

- [`examples/server-openai-responses`](examples/server-openai-responses/) — `createResponse` + `listInputItems` + `usage-costs` (multi-tag, body + SSE, four query params, required param 400)
- [`examples/server-anthropic-messages`](examples/server-anthropic-messages/) — `messages_post` with a small overlay that declares `text/event-stream` on the 200 (Anthropic's published spec omits it)

## Generated Output

| File | Description |
|------|-------------|
| `types.rs` | All struct/enum definitions from OpenAPI schemas |
| `client.rs` | Async HTTP client with typed methods per operation (when `enable_async_client`) |
| `streaming.rs` | SSE streaming **client** with event parsing (when configured) |
| `server/mod.rs` | Module re-exports for the server (when `[server]` is set) |
| `server/api.rs` | `trait <Tag>Api { async fn <op>(&self, …) -> <Op>Response; }` per tag |
| `server/errors.rs` | Status-typed response enums with JSON, bodyless, wildcard/default, and status-correct SSE variants plus `IntoResponse` |
| `server/router.rs` | Per-tag `Router` factory; combined `build_router<…>(…)` for multi-tag selections |
| `server/validation.rs` | Offline JSON Schema validators and sanitized Problem Details rejections |
| `mod.rs` | Module declarations + re-exports |
| `REQUIRED_DEPS.toml` | Complete direct dependencies and crate features for the exact generated modes and selected operations — append or merge into the consuming `Cargo.toml` |

### Generated client usage

```rust
use crate::generated::client::HttpClient;
use crate::generated::types::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClient::new()
        .with_base_url("https://api.example.com")
        .with_api_key(std::env::var("API_KEY")?);

    let req = CreateResourceRequest { /* … */ };
    let resource = client.create_resource(req).await?;
    Ok(())
}
```

## What the generated types look like

A tour of patterns the generator emits, from real outputs.

### Typed scalars

```rust
// format: date-time → chrono::DateTime<chrono::Utc>
pub created_at: chrono::DateTime<chrono::Utc>,
pub archived_at: Option<chrono::DateTime<chrono::Utc>>,

// format: uri → url::Url
pub url: url::Url,
pub callback_url: Option<url::Url>,

// format: binary (multipart) → bytes::Bytes
Binary(bytes::Bytes),

// format: uuid → uuid::Uuid
pub request_id: uuid::Uuid,
```

### Typed `additionalProperties`

```rust
pub additional_properties: std::collections::BTreeMap<String, f64>,    // usage maps
pub additional_properties: std::collections::BTreeMap<String, String>, // labels
```

### Constraints as doc comments

```rust
///Constraint: minLength=1, maxLength=64, pattern=`^[a-zA-Z0-9_-]{1,64}$`
pub custom_id: String,

///Constraint: minimum=0, maximum=1
pub temperature: Option<f64>,
```

> Generated model types do not grow `validator`/`garde` derives or perform
> validation during construction. When Axum server generation is enabled,
> request constraints are instead enforced once at the HTTP boundary from the
> original OpenAPI/JSON Schema source of truth.

### Discriminated unions (tagged enums)

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text(TextContent),
    Image(ImageContent),
}
```

### Hybrid string-or-object unions

When an `anyOf`/`oneOf` mixes a string-enum branch with tagged-object branches (a common OpenAI pattern), the generator emits an **untagged** enum so both forms deserialize:

```rust
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]                                   // not #[serde(tag="type")]
pub enum ToolChoiceParam {
    ToolChoiceOptions(ToolChoiceOptions),            // string-enum: "none"|"auto"|"required"
    ToolChoiceFunction(ToolChoiceFunction),
    ToolChoiceMCP(ToolChoiceMCP),
    // …
}
```

### Extensible enums (with `Custom(String)` fallback)

When the spec declares an `anyOf` of `const` strings plus an open `string` branch (or you opt in via `[extensible_enums]`, see below), the enum has a `Custom(String)` arm so unknown values still deserialize:

```rust
pub enum Model {
    ClaudeSonnet46,
    ClaudeOpus46,
    ClaudeHaiku45,
    Claude3Haiku20240307,
    Custom(String),         // ← anything not in the known set
}
```

### Per-operation typed errors

Each operation has its own error enum that wraps the typed body of each documented response code:

```rust
let resp = client.create_response(req).await;
match resp {
    Ok(body) => { /* … */ }
    Err(ApiOpError::Api(err)) => match err.typed {
        Some(CreateResponseApiError::Status4xx(typed)) => {
            // typed is the spec's typed 4xx body, e.g. ResponseInfo {
            //   code: 10042,
            //   message: "Please enable R2 through the Cloudflare Dashboard.",
            // }
        }
        _ => eprintln!("raw body: {}", err.body),
    },
    Err(ApiOpError::Transport(e)) => eprintln!("transport: {}", e),
}
```

## Streaming (SSE)

```rust
use openapi_to_rust::streaming::*;

let streaming_config = StreamingConfig {
    endpoints: vec![StreamingEndpoint {
        operation_id: "createChatCompletion".to_string(),
        path: "chat/completions".to_string(),
        stream_parameter: "stream".to_string(),
        event_union_type: "ChatCompletionStreamEvent".to_string(),
        event_flow: EventFlow::StartDeltaStop {
            start_events: vec!["response.created".to_string()],
            delta_events: vec!["response.output_text.delta".to_string()],
            stop_events: vec!["response.completed".to_string()],
        },
        ..Default::default()
    }],
    reconnection_config: Some(ReconnectionConfig {
        max_retries: 5,
        initial_delay_ms: 500,
        max_delay_ms: 16000,
        backoff_multiplier: 2.0,
    }),
    ..Default::default()
};
```

Generated event types are tagged enums you can match on directly:

```rust
match serde_json::from_str::<ResponseStreamEvent>(&data)? {
    ResponseStreamEvent::TextDelta(d)  => out.push_str(&d.delta),
    ResponseStreamEvent::Completed(_)  => break,
    _                                   => {}
}
```

The generator also **auto-detects** streaming endpoints: any response declaring `content: text/event-stream` flips `supports_streaming = true` automatically. Explicit `[[streaming.endpoints]]` config still wins when present.

## Spec-quirk overrides

Real specs lie. These TOML knobs let you patch quirks without forking the spec.

### `schema_extensions` — overlay JSON/YAML fragments onto the spec

A list of files whose top-level objects are deep-merged into the main spec before analysis. Use it to add a `text/event-stream` content entry, add a missing operation, or tweak a schema without forking the upstream spec. Example: the Anthropic server example overlays `sse-overlay.json` so `messages_post` gets an SSE response variant.

```toml
[generator]
# Relative paths are resolved from the directory containing this TOML file.
schema_extensions = ["sse-overlay.json"]
```

### `nullable_overrides` — force a field to `Option<T>`

When a spec marks a field as required + non-nullable but the API actually returns `null`. Format: `"SchemaName.fieldName" = true`.

```toml
[nullable_overrides]
# OpenAI's spec uses a bare $ref for `error`; the API actually returns null on success.
"Response.error" = true
```

### `extensible_enums` — force a closed enum to accept unknown values

When the spec declares a fixed enum but the API actually returns values outside the set (real-world drift). Renders the enum with a `Custom(String)` fallback variant. Accepts either the raw spec name or the rendered Rust type name.

```toml
[extensible_enums]
# CF spec declares lowercase ["apac", ..., "wnam"] but the API returns "WNAM".
"R2BucketLocation" = true
# OpenAI spec declares ["in-memory", "24h"] but the API returns "in_memory".
"ModelResponsePropertiesPromptCacheRetention" = true
```

### `type_mappings` — override a primitive's Rust type

```toml
[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"
```

### `[generator.types]` — typed-scalar strategy per format

Opt out of any individual typed scalar (e.g. fall back to `String` for date-times if you don't want `chrono`):

```toml
[generator.types]
date_time = "string"        # default: "chrono"
uri       = "string"        # default: "url"
binary    = "string"        # default: "bytes"
```

The CLI also supports `--types-conservative`, which collapses every typed scalar to `String`/`i64`/etc. Use it when you want zero optional-crate dependencies.

### `[generator.builders]` — additive operation builders

Large operations can keep their existing flat async method and also expose a `*_builder()` entry point:

```toml
[generator.builders]
enabled = true
threshold = 3
```

A builder is emitted when its optional query/header/body values and reachable optional request-body fields exceed `threshold`. Required path, query, header, and request-model values stay explicit. Setters own their values, and `.send().await` delegates to the existing flat method, so request serialization and response errors stay identical. Builder generation is disabled by default to preserve existing generated call sites.

## OpenAPI 3.1 / 3.2 support

The generator accepts 3.0.x and 3.1.x specs; 3.2.x parses with an
`experimental` warning. Unknown fields are retained for compatibility, but an
unrecognized field may be ignored by analysis and code generation. Add a
focused fixture when relying on a less-common OpenAPI or JSON Schema keyword.

**3.1 (JSON Schema 2020-12) keywords now modeled and read from typed fields:**

| Keyword group | Status |
|---|---|
| `type` as array (e.g. `["string", "null"]`) | typed + used for nullability |
| `prefixItems`, `unevaluatedItems`, `contains` / `minContains` / `maxContains` | typed |
| `patternProperties`, `propertyNames`, `unevaluatedProperties` | typed |
| `dependentRequired`, `dependentSchemas`, `if` / `then` / `else` | typed |
| `contentEncoding`, `contentMediaType`, `contentSchema` | typed |
| `$dynamicRef`, `$dynamicAnchor`, `$defs`, `$id`, `$schema`, `$comment` | typed (anchor-scope resolution is a follow-up) |
| Path Item `$ref` resolution | resolved at analysis time |
| Webhooks (`webhooks:`) | ingested as operations |
| `examples`, `example`, `title`, `deprecated`, `readOnly`/`writeOnly` | typed |

**3.2 deltas (experimental):**

| Delta | Status |
|---|---|
| `query` HTTP method + `PathItem.additionalOperations` | parses + emits client methods via `reqwest::Method::from_bytes(...)` |
| OAuth `deviceAuthorization` flow + `oauth2MetadataUrl` | typed |
| `Server.name`, `Tag.parent`/`kind`/`summary` | typed |
| `Discriminator.defaultMapping` | typed (captured; `_Other(Value)` fallback emission is a follow-up) |
| `MediaType.itemSchema`, `prefixEncoding`, `itemEncoding` | typed |
| `mediaTypes` in Components | typed |

**Modeled Components family:** `Server`, `ServerVariable`, `SecurityScheme` (apiKey / http / mutualTLS / oauth2 / openIdConnect with all flows), `OAuthFlows`, `Encoding`, `Header`, `Example`, `Link`, `Callback`, `Tag`, `ExternalDocs`, `Discriminator`. Modeling does not imply runtime support for every security or linking behavior; for example, mutual TLS credentials are not configured by generated clients.

The fixture harness under `tests/conformance/` currently gates lossless L0
parsing; its later L1–L5 markers are tracked but deferred. The vendored JSON
Schema Test Suite runner enforces a no-regression ceiling for parse failures
and lossless round trips; it does not yet enforce semantic, feature-level pass
thresholds.

## CLI

```bash
# Direct mode: client by default; use --types-only to omit it.
openapi-to-rust generate openapi.yaml
openapi-to-rust generate https://example.com/openapi.json --dry-run --json
openapi-to-rust generate openapi.yaml --check --quiet

# Create a starter config, then generate with the config defaults.
openapi-to-rust init openapi.yaml
openapi-to-rust generate

# Explicit config mode.
openapi-to-rust generate --config openapi-to-rust.toml
openapi-to-rust generate --config openapi-to-rust.toml --types-conservative
openapi-to-rust validate --config openapi-to-rust.toml

# Server scope management — preserves TOML formatting (toml_edit)
openapi-to-rust server list                    # all operations
openapi-to-rust server list --tag Responses    # filter by tag
openapi-to-rust server list --method POST --grep response
openapi-to-rust server list --json             # JSON output
openapi-to-rust server add createResponse                 # add one
openapi-to-rust server add --all-tag Responses            # expand a tag
openapi-to-rust server add createResponse --dry-run       # preview
openapi-to-rust server add createResponse --regenerate    # add + regenerate
openapi-to-rust server remove createResponse              # remove
```

Selectors are forgiving — `operationId`, `METHOD /path`, or `tag:<name>`. Typos surface Levenshtein-based "Did you mean …?" suggestions.

## TOML reference

```toml
[generator]
spec_path = "openapi.json"              # required
output_dir = "src/generated"            # required
module_name = "types"                   # informational label, not a directory
schema_extensions = []                  # optional list of JSON/YAML overlays merged into the spec

[features]
enable_sse_client = false               # generate SSE streaming client (requires [[streaming.endpoints]])
enable_async_client = true              # generate HTTP REST client
enable_specta = false                   # add specta::Type derives
enable_registry = false                 # generate static operation registry (CLI/proxy routing)
registry_only = false                   # only generate the registry (skip types/client/streaming)

[http_client]
base_url = "https://api.example.com"
timeout_seconds = 30                    # 1-3600

[http_client.retry]
max_retries = 3                         # 0-10
initial_delay_ms = 500                  # 100-10000
max_delay_ms = 16000                    # 1000-300000

[http_client.tracing]
enabled = true

[http_client.auth]
type = "Bearer"                         # Bearer | ApiKey | Custom (honored at runtime)
header_name = "Authorization"

[[http_client.headers]]
name  = "content-type"
value = "application/json"

[[streaming.endpoints]]
operation_id     = "createChatCompletion"
path             = "chat/completions"
http_method      = "POST"
stream_parameter = "stream"
event_union_type = "ChatCompletionStreamEvent"
content_type     = "text/event-stream"

[streaming.endpoints.event_flow]
type          = "StartDeltaStop"        # or "Continuous"
start_events  = ["response.created"]
delta_events  = ["response.output_text.delta"]
stop_events   = ["response.completed"]

[server]
framework  = "axum"                     # only axum supported today
operations = [                          # selectors: operationId | "METHOD /path" | "tag:<name>"
  "createResponse",
  "POST /v1/messages",
  "tag:Responses",
]
prune_models = false                    # drop schemas outside the combined selected
                                        # client/server operation closure

[server.validation]
enabled = true
max_body_bytes = 2097152
max_errors = 16

[client]
operations = [                          # absent or empty means every client operation
  "createResponse",
  "GET /v1/models",
  "tag:Files",
]
prune_models = false                    # opt in to the same union model pruning

[nullable_overrides]
"Response.error" = true                 # see "Spec-quirk overrides" above

[extensible_enums]
"R2BucketLocation" = true               # see "Spec-quirk overrides" above

[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"

[generator.types]
date_time = "chrono"                     # chrono (default) | time | string
uri       = "url"                        # url     (default) | string
binary    = "bytes"                      # bytes   (default) | vec_u8 | string
uuid      = "uuid"                       # uuid    (default) | string
byte      = "base64"                     # base64 (default) | base64_url_unpadded | vec_u8 | string
unsigned  = true                         # uint32/uint64 -> u32/u64
float_precision = "f64"                  # f64 (default) | f32 — see below

[generator.types.shape]
additional_properties_typed = true
```

**`format: float` maps to `f64`, not `f32`.** JSON carries no binary32, so the
declared format describes the server's storage rather than the transport. A
price sent on the wire as `0.03` parses losslessly into `f64`, but through
`f32` it becomes `0.029999999329447746` — a real hazard when the field is
money. Set `float_precision = "f32"` to map strictly by declared format.

## Testing

```bash
cargo test                # unit + integration tests
cargo insta test          # snapshot tests
cargo insta review        # review snapshot diffs
scripts/spec-compile.sh   # generate + cargo-check every spec in specs/ (full corpus)
```

The compile tiers are intentionally different:

- Every pull request and push to `main` generates all 54 supported OpenAPI
  specs, then compile-checks the Anthropic and OpenAI production specs against
  each generated `REQUIRED_DEPS.toml`.
- Weekly scheduled CI and manual workflow runs compile-check all 54 supported
  OpenAPI specs. The bundled Gitea Swagger 2.0 document is reported as skipped.
- Local `scripts/spec-compile.sh` runs the same full tier; pass spec names as
  arguments for a smaller targeted run.
- Every pull request also starts the generated OpenAI Responses and Anthropic
  Messages Axum examples on loopback and exercises them through pinned official
  Python SDKs. The compatibility gates cover unary and streaming responses,
  with the OpenAI row also covering input-item path/query parameters and the
  organization costs query.

## Examples

```bash
# Library / generator examples
cargo run --example basic_generation
cargo run --example client_generation_example
cargo run --example discriminated_unions
cargo run --example anyof_unions
cargo run --example allof_composition
cargo run --example openai_patterns
cargo run --example toml_config_example

# End-to-end Axum server examples (generate + run)
cargo run -p openapi-to-rust -- generate --config examples/server-openai-responses/openapi-to-rust.toml
cargo run --manifest-path examples/server-openai-responses/Cargo.toml

cargo run -p openapi-to-rust -- generate --config examples/server-anthropic-messages/openapi-to-rust.toml
cargo run --manifest-path examples/server-anthropic-messages/Cargo.toml
```

## Versioning

Until 1.0.0, a minor version may change generated Rust APIs when correcting
output that was wrong or incomplete on the wire. Regenerating lets the compiler
identify affected call sites. See the [changelog](CHANGELOG.md) for release
history and migration notes.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, targeted test commands,
snapshot guidance, and the generated-API compatibility checklist. Questions
belong in [GitHub Discussions](https://github.com/gpu-cli/openapi-to-rust/discussions);
bugs and concrete feature requests use the repository issue forms.

By participating, you agree to the [Code of Conduct](CODE_OF_CONDUCT.md).
Security reports follow [SECURITY.md](SECURITY.md); please do not file a public
issue for a suspected vulnerability.

## License

MIT
