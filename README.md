# openapi-to-rust

[![CI](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/openapi-to-rust.svg)](https://crates.io/crates/openapi-to-rust)
[![docs.rs](https://docs.rs/openapi-to-rust/badge.svg)](https://docs.rs/openapi-to-rust)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A Rust code generator that turns OpenAPI 3.1 (and 3.2-experimental) specifications into strongly-typed structs, async HTTP clients, SSE streaming clients, **and opt-in Axum server scaffolding** — including for the messy, real-world specs everyone actually ships.

We originally built this internally at [GPU CLI](https://gpu-cli.sh) to generate typed Rust clients for OpenAI, Anthropic, Cloudflare, and other large APIs. After battle-testing it against real-world specs with complex union types, discriminated enums, streaming endpoints, and the occasional spec/API drift, we decided to open source it.

It currently compiles cleanly against **54 real-world specs** in `specs/` (Stripe, OpenAI, Anthropic, Cloudflare's 14k-schema spec, GitHub, Discord, Microsoft Graph, Spotify, Twilio, …), guarded by CI.

## What's new in 0.6

- **Typed query parameter serialization** ([#27](https://github.com/gpu-cli/openapi-to-rust/issues/27)) — object and array query params are generated per their OAS `style`/`explode`: form-exploded objects become struct arguments serialized as `?color=red&size=5`, explode=false objects comma-join, deepObject objects emit `?filter[color]=red`, and form arrays become `Vec<T>` (repeated or comma-joined). **Breaking for regenerated clients** — the old `Option<impl AsRef<str>>` passthrough put a single opaque `name=<string>` pair on the wire, which no server expecting the declared style could parse. See [Breaking changes (pre-1.0)](#breaking-changes-pre-10).

## What's new in 0.5

- **Server codegen (Axum)** — opt-in `[server]` section emits a trait per tag, a status-code-typed response enum (with `IntoResponse`), an SSE-aware variant, and a `Router` factory. Pick operations one-by-one or `--all-tag`. Two end-to-end examples ship in `examples/server-{openai-responses,anthropic-messages}/`.
- **OpenAPI 3.1 + 3.2 conformance** — strict spec-extension parsing (typed `Extensions` newtype rejects unknown non-`x-` keys), full Components family (`Server`/`SecurityScheme`/`Header`/`Example`/`Link`/`Callback`/`Tag`/`ExternalDocs`/`Encoding`), JSON Schema 2020-12 keywords (`prefixItems`, `patternProperties`, `propertyNames`, `unevaluatedItems`/`Properties`, `dependentRequired`/`Schemas`, `contains`/`min`/`maxContains`, `contentEncoding`/`MediaType`/`Schema`, `if`/`then`/`else`, `$dynamicRef`/`$dynamicAnchor`, `$defs`), and 3.2 deltas (`query` HTTP method, `additionalOperations`, OAuth `deviceAuthorization`, `Server.name`, `Tag.parent`/`kind`/`summary`, `Discriminator.defaultMapping`, `mediaTypes` components, item-level encoding).
- **Real-world spec compile guarantee** — fixed a long tail of generator bugs surfaced by 54 specs in CI (`r#self` panics, operationId collisions, exclusiveMinimum bool-vs-number, signed enum variants, `<`/`>` Twilio param sanitization, self-referential union sizing, $ref-typed params, optional request bodies, and more). The full corpus runs locally; CI gates on the gold list.
- **Codegen quality fixes** — header params wired through the signature, HEAD/OPTIONS/PATCH/TRACE methods emitted, $ref-typed parameters resolved to their referenced enum types, path-templating percent-encoded per RFC 3986, range status codes (`2XX`/`4XX`/`5XX`) matched correctly, auth scheme (`Bearer`/`ApiKey`/`Custom`) honored at runtime, optional request bodies wrapped in `Option<T>`, operationId collisions detected loudly.
- **Webhook ingest** — operations declared under top-level `webhooks:` flow through analysis like any other operation.
- **SSE auto-detect** — responses with `text/event-stream` automatically mark the operation as streaming; the streaming config still wins when present.

See the [full release notes](#release-notes) at the bottom of this README for the per-area breakdown.

## Highlights

- **OpenAPI 3.1 first, 3.2 experimental** — handles `type: ["X", "null"]`, `anyOf`/`oneOf`/`allOf`, discriminated unions, `const`, inline objects, and accepts paths-less specs (components-only or webhooks-only).
- **Generates clients *and* servers** — pick client calls with `[client]`, hosted operations with `[server]`, or keep the default all-operation client. Both share the same `types.rs`.
- **Typed scalars** — `format: date-time` → `chrono::DateTime<chrono::Utc>`, `uri` → `url::Url`, `binary` → `bytes::Bytes`, `uuid` → `uuid::Uuid`, `byte` → `Vec<u8>` + base64 codec, unsigned-int formats → `u32`/`u64`. All opt-out per-format in TOML.
- **Async HTTP client** — typed methods per operation, retry/backoff via `reqwest-retry`, distributed tracing via `reqwest-tracing`, Bearer / API-key / custom auth (honored at runtime), default headers, path-template percent-encoding.
- **Axum server scaffolding** — trait per tag, status-code-typed response enum, SSE-ready `OkStream` variant, required-param HTTP 400 short-circuit at the handler boundary, combined `build_router(...)` factory for multi-tag selections.
- **SSE streaming clients** — first-class Server-Sent Events with reconnection.
- **Smart discriminated unions** — auto-detects implicit discriminators from `const` properties, falls back to `#[serde(untagged)]` when a union mixes scalar and object branches (e.g. `"auto"` *or* a tagged object).
- **Per-operation typed errors** — each operation gets its own error enum with `Status4xx(...)` typed bodies; you can match on the exact API error shape.
- **Typed `additionalProperties`** — extra keys become `BTreeMap<String, T>` instead of falling to `serde_json::Value` when the spec gives a value-type schema.
- **Constraint-as-doc** — `minLength`/`maxLength`/`minimum`/`pattern` etc. are emitted as `/// Constraint: …` doc comments. **No runtime validation is added**, so generated code stays free of validator-crate dependencies.
- **TOML configuration** with overrides for spec quirks (nullable, extensible enums, type aliases).
- **Snapshot testing** — `insta` snapshots for generated output.
- **Optional `specta::Type` derives** for cross-language type sharing.

## Install

```toml
[dependencies]
openapi-to-rust = "0.6"
```

Or as a CLI:

```bash
cargo install openapi-to-rust
```

## Quick start — client

`openapi-to-rust.toml`:

```toml
[generator]
spec_path = "openapi.json"
output_dir = "src/generated"
module_name = "api"
# All paths above are relative to this TOML file, not the shell's working directory.

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
```

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

#[axum::async_trait]
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
| `server/errors.rs` | `enum <Op>Response { Ok(T), BadRequest(E), …, OkStream(Sse<…>) }` with `IntoResponse` |
| `server/router.rs` | Per-tag `Router` factory; combined `build_router<…>(…)` for multi-tag selections |
| `mod.rs` | Module declarations + re-exports |
| `REQUIRED_DEPS.toml` | Optional crates the generated code references (chrono, uuid, url, bytes, base64) — copy into your consuming crate's `Cargo.toml` |

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

> **No runtime validation is generated.** The generator never adds the `validator` crate or `#[validate(...)]` attributes — constraints are documentation only. Validate at boundaries you control.

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

## OpenAPI 3.1 / 3.2 support

The generator accepts 3.0.x and 3.1.x specs; 3.2.x parses with an `experimental` warning. Unknown OpenAPI extensions (anything not prefixed with `x-`) surface as a hard error at parse time — silent drops are not a thing here.

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

**Components family (typed end-to-end):** `Server`, `ServerVariable`, `SecurityScheme` (apiKey / http / mutualTLS / oauth2 / openIdConnect with all flows), `OAuthFlows`, `Encoding`, `Header`, `Example`, `Link`, `Callback`, `Tag`, `ExternalDocs`, `Discriminator`. Everything is `Extensions`-strict: unknown non-`x-*` fields fail loudly at deserialize time.

A full conformance harness lives under `tests/conformance/` with fixture files and a `status.toml` recording what's L0-typed vs. L3-flowing-through-codegen. The JSON-Schema-Test-Suite (git submodule) runs as a separate test.

## CLI

```bash
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
byte      = "base64"                     # base64  (default) | vec_u8 | string
unsigned  = true                         # uint32/uint64 -> u32/u64

[generator.types.shape]
additional_properties_typed = true
```

## Testing

```bash
cargo test                # unit + integration tests
cargo insta test          # snapshot tests
cargo insta review        # review snapshot diffs
scripts/spec-compile.sh   # generate + cargo-check every spec in specs/ (full corpus)
```

CI runs the gold list (Anthropic, OpenAI, and a curated set covering the worst-behaved real-world specs) as a regression guard. Local `scripts/spec-compile.sh` runs the full 54-spec corpus.

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

## Breaking changes (pre-1.0)

Until 1.0.0, a minor version bump may change the generated API surface —
usually because the previous output was wrong on the wire. Regenerating makes
the compiler point at every affected call site; there is no silent behavior
change without a signature change.

### 0.6.0

Object- and array-schema **query parameters** are now serialized according to
their OpenAPI `style`/`explode` (issue [#27](https://github.com/gpu-cli/openapi-to-rust/issues/27)).
Previously every such parameter was `Option<impl AsRef<str>>` and the caller's
string went out as a single opaque `name=<string>` pair — which no server
expecting the declared style could parse. Signatures change as follows:

| Parameter shape | Old argument | New argument | Wire format |
|---|---|---|---|
| object, form + explode=true (OAS defaults) | `Option<impl AsRef<str>>` | `Option<Struct>` | `?color=red&size=5` |
| object, form + explode=false | `Option<impl AsRef<str>>` | `Option<Struct>` | `?filter=color,red,size,5` |
| object, deepObject | `Option<impl AsRef<str>>` | `Option<Struct>` | `?filter[color]=red` |
| array, form + explode=true (OAS defaults) | `Option<impl AsRef<str>>` | `Option<Vec<T>>` | `?tags=a&tags=b` |
| array, form + explode=false | `Option<impl AsRef<str>>` | `Option<Vec<T>>` | `?tags=a,b,c` |

`Struct` is the referenced component model for `$ref` schemas or a
synthesized `{Operation}{Param}` struct for inline objects. `T` is the scalar
item type (via the same type mapping as properties) or the referenced
string-enum model. Unchanged (still the opaque string passthrough): deepObject
arrays, `spaceDelimited`/`pipeDelimited`, arrays of objects, and server-side
extraction (tracked separately).

## Release notes

### 0.6 (this release)

**Typed query parameter serialization** ([#27](https://github.com/gpu-cli/openapi-to-rust/issues/27))
- Object and array query parameters are generated per their OAS `style`/`explode` instead of an opaque `Option<impl AsRef<str>>` passthrough: form-exploded objects (`?color=red&size=5`), explode=false objects (`?filter=color,red,size,5`), deepObject objects (`?filter[color]=red`), and `Vec<T>` form arrays (repeated or comma-joined pairs, scalar or string-enum items).
- **Breaking for regenerated clients** — see [Breaking changes (pre-1.0)](#breaking-changes-pre-10) for the full signature table.
- `scripts/spec-compile.sh` checks all scratch crates as one cargo workspace against a shared persistent target dir — full-sweep verification dropped from hours to minutes.
- `[client].operations` selects client methods by operationId, `METHOD /path`, or tag; optional client/server pruning retains the union of reachable models, including inline generated schemas and streaming event roots.

### 0.5

**Server codegen (Axum)**
- `[server]` TOML section with selector grammar (operationId / `METHOD /path` / `tag:<name>`).
- `openapi-to-rust server list / add / remove` with `--all-tag`, `--dry-run`, `--regenerate`, `--json`. Edits preserve TOML formatting via `toml_edit`. Typos get Levenshtein "Did you mean …?" suggestions.
- Emits `server/{mod,api,errors,router}.rs`: trait per tag, status-code-typed response enum with `IntoResponse`, conditional `OkStream(Sse<…>)` variant, per-tag and combined `build_router<…>(…)` factory.
- Query + header parameters wired through the trait. Required query/header params arrive unwrapped; missing-required short-circuits to HTTP 400 + `{"error": "missing required …"}` JSON.
- Generated `sse_response(stream)` helper — wraps any `Stream<Item = Result<Event, Infallible>>` so the SSE branch stays two lines.
- End-of-generate hint prints a paste-ready impl skeleton.
- Two end-to-end examples: `server-openai-responses` (multi-tag, body + SSE, four query params, required-param 400) and `server-anthropic-messages` (SSE overlay).

**OpenAPI 3.1 + 3.2 conformance (38 of 51 beads from #14)**
- Strict spec extensions (`Extensions` newtype rejects unknown non-`x-` keys) + version gate (3.0 / 3.1 accepted, 3.2 experimental, others hard error).
- Full Components family typed end-to-end: `Server`, `ServerVariable`, `SecurityScheme` (apiKey/http/mutualTLS/oauth2/openIdConnect), `OAuthFlows`, `Encoding`, `Header`, `Example`, `Link`, `Callback`, `Tag`, `ExternalDocs`, `Discriminator`.
- JSON Schema 2020-12 keywords typed: `prefixItems`, `patternProperties`, `propertyNames`, `unevaluatedItems`/`Properties`, `dependentRequired`/`Schemas`, `contains`/`min`/`maxContains`, `contentEncoding`/`MediaType`/`Schema`, `if`/`then`/`else`, `$dynamicRef`/`$dynamicAnchor`, `$defs`, `$id`, `$schema`, `$comment`.
- 3.2 deltas: `query` HTTP method, `additionalOperations` (arbitrary verbs), OAuth `deviceAuthorization`, `Server.name`, `Tag.parent`/`kind`/`summary`, `Discriminator.defaultMapping`, `MediaType.itemSchema`/`prefixEncoding`/`itemEncoding`, `mediaTypes` components.
- `type` as array (3.1 canonical nullability — `type: ["string", "null"]`) merges cleanly with the 3.0 `nullable: true` field.
- Webhooks (`webhooks:`) ingested as operations.
- Path Item `$ref` resolution (`$ref: "#/components/pathItems/X"`).
- Conformance harness with fixtures, layered `fails_at:` markers, `status.toml`, and the JSON-Schema-Test-Suite as a git submodule.

**Real-world spec compile guarantee (54 specs in CI)**
- `r#self`/`r#super`/`r#crate`/`r#Self` panic → rename to `<keyword>_field`/`<keyword>_param` (proc_macro2 doesn't accept those as raw idents).
- OperationId collisions → auto-disambiguate by HTTP method (`opId_post`) with a stderr warning, instead of rejecting the whole document.
- `exclusiveMinimum` modeled as `bool | f64` (3.0/Swagger used bool; 3.1 uses number).
- Signed enum variants disambiguated (`Variant1` vs `VariantNeg1`).
- Twilio-style `<` / `>` / `<=` / `>=` filter params sanitized to `_lt`/`_gt`/`_lte`/`_gte`.
- Self-referential union variants boxed to break infinite-size enums.
- Nullable-anyOf wrapper collisions resolved (don't synthesize a wrapper that overwrites the inner `$ref`'s schema).
- `$ref` shape variants accepted (`#/definitions/X` Swagger carry-over, `#/components/parameters/X/schema` falls back to `Value`).
- Per-method parameter ident collisions resolved at analysis time (`exclude_ids` + `exclude-ids` get unique `rust_ident`s).
- Empty/non-string enum values coerced via Display (gitpod's numeric values on a string-typed schema).
- Optional request bodies (`required: false`) wrapped in `Option<T>` and chained with `if let Some(...)` so the request builder typechecks.
- `$ref`-typed parameter types resolved to the referenced enum (instead of silently falling back to `String`).
- String enums emit `as_str()`, `Display`, and `AsRef<str>` impls so the wire form drops cleanly into headers, query strings, and path segments.
- `cargo clippy --all-features -- -D warnings` is clean.

**Codegen quality fixes**
- T1: header request parameters wired through codegen.
- T2: HEAD/OPTIONS/PATCH/TRACE methods emitted explicitly (was a silent `_ => get` fallback).
- T3: `auth_config` `ApiKey` / `Custom` variants honored at runtime — README's multi-scheme-auth claim is now actually true.
- T4: webhooks walked in analysis.
- T5: path templating percent-encoded per RFC 3986 §3.3 via an emitted `__pct_encode_path_segment` helper.
- T6: operationId collisions detected loudly at analysis time.
- T8: range status codes (`1XX`/`2XX`/`3XX`/`4XX`/`5XX`) get guarded match arms instead of falling through to the generic default.
- T10: `$ref`-typed parameter types resolved.
- T11: optional request bodies → `Option<T>`.
- T13: spec `description`/`summary` surfaced as rustdoc on each operation method.
- T15: SSE auto-detection from `text/event-stream` responses.
- F1: strict spec extensions + version gate.
- F2: `type` as array (3.1 nullability).
- F4: paths-less specs (components-only / webhooks-only) accepted.

**Typed scalars (Q2 series)**
- `TypeMapper` chokepoint for format-driven type mapping.
- `format: date-time` → `chrono::DateTime<Utc>`, `uri` → `url::Url`, `binary` → `bytes::Bytes`, `uuid` → `uuid::Uuid`, `byte` → `Vec<u8>` + base64 codec.
- Unsigned-int formats (`uint32`, `uint64`, etc.) → `u32`/`u64`.
- Typed `additionalProperties` → `BTreeMap<String, T>`.
- `x-enum-varnames` honored.
- Constraint doc comments (`minLength`/`maxLength`/`minimum`/`pattern`).
- Per-format strategy opt-out via direct fields in `[generator.types]`; `--types-conservative` flag.
- `REQUIRED_DEPS.toml` emitted listing optional crates the generated code references (+ stderr advisory).
- Clean primitive variants for `anyOf` unions.

**Other**
- Hybrid string-or-object unions, `allOf`-nullable handling, `[extensible_enums]` override.

## Contributing

1. Fork the repo
2. Add your OpenAPI spec or pattern to `specs/` or `tests/fixtures/`
3. Write a snapshot test (`insta`)
4. Run `cargo insta test` and review output
5. Open a PR

## License

MIT
