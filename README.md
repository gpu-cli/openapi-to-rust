# openapi-generator

A Rust code generator that creates strongly-typed structs, HTTP clients, and SSE streaming clients from OpenAPI 3.1 specifications.

We originally built this internally at [GPU CLI](https://gpu-cli.sh) to generate typed Rust clients for OpenAI, Anthropic, and other APIs. After battle-testing it against real-world specs with complex union types, discriminated enums, and streaming endpoints, we decided to open source it.

## Features

- **OpenAPI 3.1 support** — objects, arrays, enums, `oneOf`, `anyOf`, `allOf`, discriminated unions
- **HTTP client generation** — async clients with retry logic, tracing, and auth middleware
- **SSE streaming clients** — first-class Server-Sent Events support with reconnection
- **Smart `$ref` resolution** — handles circular references and deep nesting
- **Discriminator detection** — auto-detects tagged unions from `oneOf`/`anyOf` with const properties
- **TOML configuration** — declarative config as an alternative to the Rust API
- **Snapshot testing** — built-in `insta` snapshot tests for generated output
- **Optional [specta](https://github.com/oscartbeaumont/specta) support** — add `specta::Type` derives for TypeScript codegen

## Install

Add to your `Cargo.toml`:

```toml
[dependencies]
openapi-generator = "0.1"
```

Or install the CLI:

```bash
cargo install openapi-generator
```

## Quick Start

### CLI (TOML config)

Create `openapi-generator.toml`:

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

Then generate:

```bash
openapi-gen generate --config openapi-generator.toml
```

### Library API

```rust
use openapi_generator::{SchemaAnalyzer, CodeGenerator, GeneratorConfig};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let spec_content = std::fs::read_to_string("openapi.json")?;
    let spec_value: serde_json::Value = serde_json::from_str(&spec_content)?;

    let mut analyzer = SchemaAnalyzer::new(spec_value)?;
    let mut analysis = analyzer.analyze()?;

    let config = GeneratorConfig {
        spec_path: PathBuf::from("openapi.json"),
        output_dir: PathBuf::from("src/generated"),
        module_name: "api".to_string(),
        enable_sse_client: true,
        enable_async_client: true,
        ..Default::default()
    };

    let generator = CodeGenerator::new(config);
    let result = generator.generate_all(&mut analysis)?;
    generator.write_files(&result)?;

    println!("Generated {} files", result.files.len());
    Ok(())
}
```

## Generated Output

The generator produces up to four files:

| File | Description |
|------|-------------|
| `types.rs` | All struct/enum definitions from OpenAPI schemas |
| `client.rs` | Async HTTP client with typed methods per operation |
| `streaming.rs` | SSE streaming client with event parsing |
| `mod.rs` | Module declarations |

### Generated Client Usage

```rust
use crate::generated::client::HttpClient;
use crate::generated::types::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = HttpClient::new()
        .with_base_url("https://api.example.com")
        .with_api_key("your-api-key");

    let request = CreateResourceRequest { /* ... */ };
    let response = client.create_resource(request).await?;
    Ok(())
}
```

## HTTP Client Features

The generated HTTP client includes:

- **All HTTP methods** — GET, POST, PUT, DELETE, PATCH
- **Retry with backoff** — exponential backoff via `reqwest-retry` (retries 429, 500, 502, 503, 504)
- **Distributed tracing** — automatic request/response spans via `reqwest-tracing`
- **Authentication** — Bearer token, API key, or custom header auth
- **Default headers** — configurable per-client headers
- **Middleware stack** — composable request/response middleware

## SSE Streaming

First-class support for Server-Sent Events endpoints:

```rust
use openapi_generator::streaming::*;

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

## Discriminated Unions

Automatically detects and generates tagged enums from OpenAPI discriminator patterns:

```yaml
# OpenAPI spec
MessageContent:
  oneOf:
    - $ref: '#/components/schemas/TextContent'
    - $ref: '#/components/schemas/ImageContent'
  discriminator:
    propertyName: type
    mapping:
      text: '#/components/schemas/TextContent'
      image: '#/components/schemas/ImageContent'
```

Generates:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text(TextContent),
    Image(ImageContent),
}
```

Also handles untagged unions (`anyOf` without discriminator), `allOf` composition, extensible enums, and inline variant schemas.

## TOML Configuration Reference

```toml
[generator]
spec_path = "openapi.json"              # Path to OpenAPI spec (required)
output_dir = "src/generated"            # Output directory (required)
module_name = "types"                   # Module name (required)

[features]
enable_sse_client = true                # Generate SSE streaming client
enable_async_client = true              # Generate HTTP REST client
enable_specta = false                   # Add specta::Type derives

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
name = "content-type"
value = "application/json"

[nullable_overrides]
"Response.error" = true                 # Override nullable detection

[type_mappings]
"DateTime" = "chrono::DateTime<chrono::Utc>"
```

## Testing

```bash
# Run all tests
cargo test

# Run snapshot tests
cargo insta test

# Review snapshot changes
cargo insta review
```

## Examples

The `examples/` directory contains working examples for:

- Basic type generation
- HTTP client generation
- Discriminated unions and `anyOf`/`oneOf` patterns
- `allOf` composition
- Inline objects and enums
- Nullable field handling
- Recursive schemas
- TOML configuration

```bash
cargo run --example basic_generation
cargo run --example client_generation_example
cargo run --example discriminated_unions
```

## Contributing

1. Fork the repo
2. Add your OpenAPI spec pattern to `examples/` or `tests/fixtures/`
3. Write a test using snapshot testing (`insta`)
4. Run `cargo insta test` and review the output
5. Submit a PR

## License

MIT
