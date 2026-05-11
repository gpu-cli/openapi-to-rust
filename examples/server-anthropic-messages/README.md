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
```

**Streaming caveat.** Anthropic's published OpenAPI spec doesn't
declare `text/event-stream` on the 200 response, so the generator
can't emit an `OkStream` variant. Tracked as `openapi-generator-in6`
— the planned fix is a schema-extension overlay that adds the
streaming content type, after which this example will gain a
streaming branch with no manual changes.

The integration test in `tests/server_examples_test.rs` guarantees
the example keeps building.
