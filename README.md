# openapi-to-rust

[![CI](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml/badge.svg)](https://github.com/gpu-cli/openapi-to-rust/actions/workflows/ci.yml)
[![Crates.io](https://img.shields.io/crates/v/openapi-to-rust.svg)](https://crates.io/crates/openapi-to-rust)
[![docs.rs](https://docs.rs/openapi-to-rust/badge.svg)](https://docs.rs/openapi-to-rust)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A Rust code generator that turns OpenAPI 3.1 specifications into strongly-typed structs, async HTTP clients, and SSE streaming clients — including for the messy, real-world specs everyone actually ships.

We originally built this internally at [GPU CLI](https://gpu-cli.sh) to generate typed Rust clients for OpenAI, Anthropic, Cloudflare, and other large APIs. After battle-testing it against real-world specs with complex union types, discriminated enums, streaming endpoints, and the occasional spec/API drift, we decided to open source it.

It currently compiles cleanly against 54 specs in `specs/` (Stripe, OpenAI, Anthropic, Cloudflare 14k-schema spec, GitHub, Discord, Microsoft Graph, Spotify, …).

## Highlights

- **OpenAPI 3.1 first** — handles `type: ["X", "null"]`, `anyOf`/`oneOf`/`allOf`, discriminated unions, `const`, and inline objects.
- **Typed scalars** — `format: date-time` → `chrono::DateTime<chrono::Utc>`, `uri` → `url::Url`, `binary` → `bytes::Bytes`, `uuid` → `uuid::Uuid`, `byte` → `Vec<u8>` + base64 codec, unsigned-int formats → `u32`/`u64`. All opt-out per-format in TOML.
- **Async HTTP client** — typed methods per operation, retry/backoff via `reqwest-retry`, distributed tracing via `reqwest-tracing`, Bearer / API-key / custom auth, default headers.
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
openapi-to-rust = "0.4"
```

Or as a CLI:

```bash
cargo install openapi-to-rust
```

## Quick Start

### CLI (TOML config)

`openapi-to-rust.toml`:

```toml
[generator]
spec_path = "openapi.json"
output_dir = "src/generated"
module_name = "api"

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

### Library API

```rust
use openapi_to_rust::{SchemaAnalyzer, CodeGenerator, GeneratorConfig};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec = std::fs::read_to_string("openapi.json")?;
    let value: serde_json::Value = serde_json::from_str(&spec)?;

    let mut analyzer = SchemaAnalyzer::new(value)?;
    let mut analysis = analyzer.analyze()?;

    let config = GeneratorConfig {
        spec_path: PathBuf::from("openapi.json"),
        output_dir: PathBuf::from("src/generated"),
        module_name: "api".to_string(),
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis)?;
    generator.write_files(&result)?;
    Ok(())
}
```

## Generated Output

| File | Description |
|------|-------------|
| `types.rs` | All struct/enum definitions from OpenAPI schemas |
| `client.rs` | Async HTTP client with typed methods per operation |
| `streaming.rs` | SSE streaming client with event parsing (when configured) |
| `mod.rs` | Module declarations + re-exports |
| `REQUIRED_DEPS.toml` | List of optional crates the generated code references (chrono, uuid, url, bytes, base64) — copy into your consuming crate's `Cargo.toml` |

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

## Spec-quirk overrides

Real specs lie. These TOML knobs let you patch quirks without forking the spec.

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
[generator.types.strategies]
"date-time" = "string"     # default: "chrono"
"uri"       = "string"     # default: "url"
"binary"    = "string"     # default: "bytes"
```

The CLI also supports `--types-conservative`, which collapses every typed scalar to `String`/`i64`/etc. Use it when you want zero optional-crate dependencies.

## TOML reference

```toml
[generator]
spec_path = "openapi.json"              # required
output_dir = "src/generated"            # required
module_name = "types"                   # required
schema_extensions = []                  # optional list of JSON files merged into the spec

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
type = "Bearer"                         # Bearer | ApiKey | Custom
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

[nullable_overrides]
"Response.error" = true                 # see "Spec-quirk overrides" above

[extensible_enums]
"R2BucketLocation" = true               # see "Spec-quirk overrides" above

[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"

[generator.types.strategies]
"date-time" = "chrono"                  # chrono (default) | string
"uri"       = "url"                     # url     (default) | string
"binary"    = "bytes"                   # bytes   (default) | string
"uuid"      = "uuid"                    # uuid    (default) | string
"byte"      = "vec_u8_base64"           # default; encodes/decodes via base64
```

## Testing

```bash
cargo test                # unit + integration tests
cargo insta test          # snapshot tests
cargo insta review        # review snapshot diffs
scripts/spec-compile.sh   # generate + cargo-check every spec in specs/
```

## Examples

```bash
cargo run --example basic_generation
cargo run --example client_generation_example
cargo run --example discriminated_unions
cargo run --example anyof_unions
cargo run --example allof_composition
cargo run --example openai_patterns
cargo run --example toml_config_example
```

## Contributing

1. Fork the repo
2. Add your OpenAPI spec or pattern to `specs/` or `tests/fixtures/`
3. Write a snapshot test (`insta`)
4. Run `cargo insta test` and review output
5. Open a PR

## License

MIT
