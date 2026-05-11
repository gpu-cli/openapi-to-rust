# server-openai-responses

End-to-end demo of the server codegen: scaffold `POST /v1/responses`
from the OpenAI spec, implement the trait, serve it via axum.

```bash
# From repo root:
cargo run -p openapi-to-rust -- generate \
  --config examples/server-openai-responses/openapi-to-rust.toml

cargo run --manifest-path examples/server-openai-responses/Cargo.toml

# In another shell:
curl -N -s http://127.0.0.1:3000/responses \
  -H 'content-type: application/json' \
  -d '{"model":"gpt-x","input":"hi","stream":true}'
```

The stub handler streams four canned SSE events. A real handler swaps
the `stream::iter(...)` for a stream piped out of your model server.

The integration test in `tests/server_examples_test.rs` runs both
steps (generate + build) and is the canonical guarantee the example
keeps working.
