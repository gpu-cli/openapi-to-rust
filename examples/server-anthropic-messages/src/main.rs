//! Example: host a perfect-replica of Anthropic's `POST /v1/messages`.
//!
//! Exercises both branches of the typed response enum:
//!   - `body.stream == Some(true)`  → `OkStream(Sse<...>)`
//!   - otherwise                    → `Ok(Message)` (single JSON body)
//!
//! NOTE: Anthropic's published spec declares only `application/json`
//! on the 200 response. This example pulls in a small overlay
//! (`sse-overlay.json`) via the generator's `schema_extensions`
//! mechanism, which declares `text/event-stream` on the 200 so the
//! generated trait gets the `OkStream` variant.
//!
//! Run:
//!   1. `cargo run -p openapi-to-rust -- generate \
//!         --config examples/server-anthropic-messages/openapi-to-rust.toml`
//!   2. `cargo run --manifest-path examples/server-anthropic-messages/Cargo.toml`
//!   3. Unary:
//!      `curl -s http://127.0.0.1:3001/v1/messages \
//!         -H 'content-type: application/json' \
//!         -d '{"model":"claude-x","max_tokens":50,
//!              "messages":[{"role":"user","content":"hi"}]}'`
//!   4. SSE:
//!      `curl -N -s http://127.0.0.1:3001/v1/messages \
//!         -H 'content-type: application/json' \
//!         -d '{"model":"claude-x","max_tokens":50,"stream":true,
//!              "messages":[{"role":"user","content":"hi"}]}'`

pub mod gen;

use axum::response::sse::Event;
use futures_util::stream;
use gen::CreateMessageParams;
use gen::server::{MessagesPostResponse, ServerApi, server_api_router, sse_response};
use std::convert::Infallible;

#[derive(Clone)]
struct AppState;

#[async_trait::async_trait]
impl ServerApi for AppState {
    async fn messages_post(
        &self,
        _anthropic_version: Option<String>,
        body: CreateMessageParams,
    ) -> MessagesPostResponse {
        if body.stream == Some(true) {
            messages_streaming()
        } else {
            messages_unary()
        }
    }
}

fn messages_unary() -> MessagesPostResponse {
    let msg = gen::Message {
        container: None,
        content: vec![gen::ContentBlock::TextBlock(gen::ResponseTextBlock {
            citations: None,
            text: "hello (unary)".into(),
        })],
        id: "msg_demo".into(),
        model: gen::Model::Custom("claude-demo".into()),
        role: gen::MessageRole::Assistant,
        stop_details: None,
        stop_reason: None,
        stop_sequence: None,
        r#type: gen::MessageType::Message,
        usage: gen::Usage {
            cache_creation: None,
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            inference_geo: None,
            input_tokens: 0,
            output_tokens: 0,
            server_tool_use: None,
            service_tier: None,
        },
    };
    MessagesPostResponse::Ok(msg)
}

fn messages_streaming() -> MessagesPostResponse {
    // The real Anthropic stream emits message_start →
    // content_block_start → content_block_delta* → content_block_stop
    // → message_delta → message_stop. The example fires a short
    // subset; production code mirrors the full sequence from the
    // upstream model.
    let events = stream::iter(vec![
        sse_event("message_start", r#"{"type":"message_start","message":{"id":"msg_demo"}}"#),
        sse_event(
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ),
        sse_event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hello "}}"#,
        ),
        sse_event(
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"world"}}"#,
        ),
        sse_event("content_block_stop", r#"{"type":"content_block_stop","index":0}"#),
        sse_event("message_stop", r#"{"type":"message_stop"}"#),
    ]);
    MessagesPostResponse::OkStream(sse_response(events))
}

fn sse_event(name: &str, data: &str) -> Result<Event, Infallible> {
    Ok(Event::default().event(name).data(data))
}

#[tokio::main]
async fn main() {
    let app = server_api_router(AppState);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
    println!("listening on http://{}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_body(stream: Option<bool>) -> CreateMessageParams {
        let mut json = serde_json::json!({
            "model": "claude-x",
            "max_tokens": 50,
            "messages": [{"role": "user", "content": "hi"}],
        });
        if let Some(s) = stream {
            json["stream"] = serde_json::Value::Bool(s);
        }
        serde_json::from_value(json).expect("minimal CreateMessageParams must deserialize")
    }

    #[tokio::test]
    async fn unary_path_returns_ok_variant() {
        let r = AppState.messages_post(None, make_body(None)).await;
        assert!(matches!(r, MessagesPostResponse::Ok(_)));
    }

    #[tokio::test]
    async fn stream_path_returns_ok_stream_variant() {
        let r = AppState.messages_post(None, make_body(Some(true))).await;
        assert!(matches!(r, MessagesPostResponse::OkStream(_)));
    }
}
