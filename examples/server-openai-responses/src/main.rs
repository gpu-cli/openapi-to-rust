//! Example: host a perfect-replica of OpenAI's `POST /v1/responses`.
//!
//! Exercises both branches of the typed response enum:
//!   - `body.stream == Some(true)`  → `OkStream(Sse<...>)`
//!   - otherwise                    → `Ok(Response)` (single JSON body)
//!
//! Run:
//!   1. `cargo run -p openapi-to-rust -- generate \
//!         --config examples/server-openai-responses/openapi-to-rust.toml`
//!   2. `cargo run --manifest-path examples/server-openai-responses/Cargo.toml`
//!   3. Unary:
//!      `curl -s http://127.0.0.1:3000/responses \
//!         -H 'content-type: application/json' \
//!         -d '{"model":"gpt-x","input":"hi"}'`
//!   4. SSE:
//!      `curl -N -s http://127.0.0.1:3000/responses \
//!         -H 'content-type: application/json' \
//!         -d '{"model":"gpt-x","input":"hi","stream":true}'`

pub mod gen;

use axum::response::sse::Event;
use futures_util::stream;
use gen::CreateResponse;
use gen::server::{CreateResponseResponse, ResponsesApi, responses_api_router, sse_response};
use std::convert::Infallible;

#[derive(Clone)]
struct AppState;

#[axum::async_trait]
impl ResponsesApi for AppState {
    async fn create_response(&self, body: CreateResponse) -> CreateResponseResponse {
        if body.stream == Some(true) {
            create_response_streaming()
        } else {
            create_response_unary(body)
        }
    }
}

/// Unary path: return a single 200 JSON body. We build the `Response`
/// via serde_json so the example doesn't have to enumerate ~30 nested
/// fields of an OpenAI spec type. A real handler builds Response from
/// the model output directly.
fn create_response_unary(body: CreateResponse) -> CreateResponseResponse {
    // Build a minimal `Response` via serde_json so the example
    // doesn't need to enumerate every field. Required fields are
    // populated to the smallest valid shape; everything else stays
    // implicit-None. Spec quirks: `error` is non-nullable in the
    // spec even when conceptually optional, and `output` is required
    // but may be empty.
    let resp: gen::Response = serde_json::from_value(serde_json::json!({
        "id": "resp_demo",
        "object": "response",
        "created_at": 0.0,
        "model": body.model,
        "metadata": {},
        "error": { "code": "server_error", "message": "" },
        "output": [],
        "parallel_tool_calls": false,
        "tool_choice": "auto",
        "tools": [],
    }))
    .expect("static demo response must deserialize against the generated schema");
    CreateResponseResponse::Ok(resp)
}

/// Streaming path: four canned SSE events. Production swaps
/// `stream::iter(...)` for a stream piped from your model server.
fn create_response_streaming() -> CreateResponseResponse {
    let events = stream::iter(vec![
        sse_event("response.created", r#"{"id":"resp_demo"}"#),
        sse_event("response.output_text.delta", r#"{"delta":"hello "}"#),
        sse_event("response.output_text.delta", r#"{"delta":"world"}"#),
        sse_event("response.completed", r#"{"id":"resp_demo"}"#),
    ]);
    // `sse_response` is the generated helper — wraps any
    // `Stream<Item = Result<Event, Infallible>>` so the user
    // doesn't have to import axum::Sse or `Box::pin` it manually.
    CreateResponseResponse::OkStream(sse_response(events))
}

fn sse_event(name: &str, data: &str) -> Result<Event, Infallible> {
    Ok(Event::default().event(name).data(data))
}

#[tokio::main]
async fn main() {
    let app = responses_api_router(AppState);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_body(stream: Option<bool>) -> CreateResponse {
        // Round-trip a minimal valid CreateResponse through serde so
        // we don't have to enumerate every field of the spec type.
        let mut json = serde_json::json!({
            "model": "gpt-x",
            "input": "hi",
        });
        if let Some(s) = stream {
            json["stream"] = serde_json::Value::Bool(s);
        }
        serde_json::from_value(json).expect("minimal CreateResponse must deserialize")
    }

    #[tokio::test]
    async fn unary_path_returns_ok_variant() {
        let r = AppState.create_response(make_body(None)).await;
        assert!(matches!(r, CreateResponseResponse::Ok(_)));
    }

    #[tokio::test]
    async fn stream_path_returns_ok_stream_variant() {
        let r = AppState.create_response(make_body(Some(true))).await;
        assert!(matches!(r, CreateResponseResponse::OkStream(_)));
    }
}
