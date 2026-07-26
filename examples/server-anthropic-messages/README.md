# server-anthropic-messages

End-to-end demo of the server codegen: scaffold `POST /v1/messages`
from the Anthropic spec, implement the trait, serve it via axum.

```bash
# From repo root:
cargo run -p openapi-to-rust -- generate \
  --config examples/server-anthropic-messages/openapi-to-rust.toml

cargo run --manifest-path examples/server-anthropic-messages/Cargo.toml

# In another shell:
curl -s http://127.0.0.1:3001/v1/messages \
  -H 'content-type: application/json' \
  -d '{"model":"claude-x","max_tokens":50,"messages":[{"role":"user","content":"hi"}]}'

# Stream the response as server-sent events:
curl -N -s http://127.0.0.1:3001/v1/messages \
  -H 'content-type: application/json' \
  -d '{"model":"claude-x","max_tokens":50,"stream":true,"messages":[{"role":"user","content":"hi"}]}'
```

Anthropic's published OpenAPI spec does not declare `text/event-stream` on the
200 response. The example config applies `sse-overlay.json` through
`generator.schema_extensions`, adding that response media type before analysis
so generation emits both `Ok(Message)` and `OkStream` response variants.

The integration test in `tests/server_examples_test.rs` regenerates and tests
the example. PR CI additionally calls the generated server with the pinned
official Anthropic Python SDK, covering both unary and streaming responses.
