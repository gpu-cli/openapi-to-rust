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
use gen::server::{
    CreateResponseResponse, ListInputItemsResponse, ResponsesApi, UsageApi, UsageCostsResponse,
    build_router, sse_response,
};
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

    async fn list_input_items(
        &self,
        response_id: String,
        limit: Option<i64>,
        order: Option<gen::ListInputItemsOrder>,
        _after: Option<String>,
        _include: Option<String>,
    ) -> ListInputItemsResponse {
        // Echo the params back in a minimal valid ResponseItemList so
        // the test can introspect what the handler saw. Real services
        // would hit storage here.
        let _ = (response_id, limit, order);
        let body: gen::ResponseItemList = serde_json::from_value(serde_json::json!({
            "data": [],
            "object": "list",
            "first_id": "",
            "last_id": "",
            "has_more": false,
        }))
        .expect("ResponseItemList must deserialize");
        ListInputItemsResponse::Ok(body)
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

#[axum::async_trait]
impl UsageApi for AppState {
    /// `start_time` is `i64` (not `Option`) — the generated handler
    /// short-circuits with 400 if the client omits it, so by the
    /// time we get here the value is guaranteed present.
    async fn usage_costs(
        &self,
        start_time: i64,
        _end_time: Option<i64>,
        _bucket_width: Option<gen::UsageCostsBucketWidth>,
        _project_ids: Option<String>,
        _group_by: Option<String>,
        _limit: Option<i64>,
        _page: Option<String>,
    ) -> UsageCostsResponse {
        let body: gen::UsageResponse = serde_json::from_value(serde_json::json!({
            "object": "page",
            "data": [],
            "has_more": false,
            "next_page": "",
        }))
        .unwrap_or_else(|e| panic!("UsageResponse must deserialize: {e}; start_time={start_time}"));
        UsageCostsResponse::Ok(body)
    }
}

#[tokio::main]
async fn main() {
    // Single combined router that takes both trait impls. Each tag's
    // routes get mounted on the same axum::Router via `.merge()`.
    let app = build_router(AppState, AppState);
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

    #[tokio::test]
    async fn list_input_items_accepts_path_and_query_params() {
        let r = AppState
            .list_input_items("resp_abc".into(), Some(50), None, None, None)
            .await;
        assert!(matches!(r, ListInputItemsResponse::Ok(_)));
    }

    #[tokio::test]
    async fn usage_costs_required_param_arrives_as_unwrapped() {
        // start_time is `i64` not `Option<i64>` — the generated
        // handler ensures it's always present.
        let r = AppState
            .usage_costs(1_700_000_000, None, None, None, None, None, None)
            .await;
        assert!(matches!(r, UsageCostsResponse::Ok(_)));
    }
}
