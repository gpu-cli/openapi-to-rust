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

The stub handler streams canned SDK-compatible SSE events. A real handler swaps
the `stream::iter(...)` for a stream piped out of your model server.

The integration test in `tests/server_examples_test.rs` runs both
steps (generate + build) and is the canonical guarantee the example
keeps working.

`tests/openai_sdk_compat_test.rs` goes one step further: it starts this
generated server on an ephemeral loopback port and calls it with the official
OpenAI Python SDK pinned in `tests/python/openai_sdk_compat/requirements.txt`.
It covers unary and streaming Responses, input-item path/query serialization,
and the organization costs query. The same generation also emits and compiles
the Rust client for those three production-spec operations; generic tests own
the duplicate Rust client/server wire roundtrip. CI runs the SDK test on every
pull request; locally, install the requirements and run:

```bash
OPENAI_COMPAT_PYTHON=python3 cargo test --test openai_sdk_compat_test \
  official_openai_python_sdk_matches_generated_axum_server -- \
  --ignored --exact --nocapture
```
