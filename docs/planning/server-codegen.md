# Server Codegen — Design

Status: design approved, not yet implemented.
Target framework: Axum (single framework for v1; config leaves room for more).

## Goal

Generate Axum server scaffolding for a **user-selected subset** of an OpenAPI
spec's operations. The generated code defines the seam (traits, router factory,
typed responses); the user supplies the implementation as a hand-written
`impl Trait for State` in their own crate. Regeneration only ever touches
generated files — never user code.

The motivating case is hosting a perfect replica of someone else's API
(e.g. OpenAI `POST /v1/chat/completions`) without inheriting their entire
spec surface.

## Core principles

1. **Opt-in per operation.** No default "generate the whole spec." Empty
   `[server].operations` ⇒ zero server code.
2. **Trait at the seam.** User implementation lives outside `generated/`.
   The trait method signature is the contract; the type checker enforces it.
3. **No scaffold file.** Nothing is "emit once then user owns it." Every
   generated file is regenerable; every user file is hand-written. Clean line.
4. **Spec drift surfaces loudly.** If an upstream `operationId` disappears,
   regeneration fails. User decides whether to follow the rename.
5. **No client-side validation in emitted code** (existing project rule —
   no `validator` derives, constraints stay as doc comments).

## Selection model

Three selector forms, priority order:

| Form              | Example                      | Notes                                |
| ----------------- | ---------------------------- | ------------------------------------ |
| `operationId`     | `createChatCompletion`       | Recommended — stable, unique.        |
| `METHOD PATH`     | `POST /v1/embeddings`        | Fallback when spec has no opId.      |
| `tag:<name>`      | `tag:Files`                  | Bulk pick a feature area.            |

`tag:` is convenient but coarse — upstream adding an op to that tag silently
expands the trait. Docs should steer production users toward explicit opIds.

## TOML config

```toml
[server]
framework = "axum"
operations = [
  "createChatCompletion",
  "tag:Embeddings",
]
```

Section absent or list empty ⇒ no server emission.

## CLI commands

### `server list`

Read-only discovery. Tabular by default, `--json` for scripting.

```
$ openapi-to-rust server list --spec specs/openai.yaml [--tag Chat] [--grep completion] [--method POST]

TAG         OP_ID                    METHOD  PATH
Chat        createChatCompletion     POST    /v1/chat/completions
...
86 operations across 14 tags.
```

### `server add <selector>`

Edits the TOML, does NOT regenerate (lets user batch adds).
Flags: `--regenerate`, `--dry-run`, `--all-tag <name>`.

```
$ openapi-to-rust server add createChatCompletion
✓ Added 'createChatCompletion' to [server].operations.
  POST /v1/chat/completions  (tag: Chat)
  Request:  CreateChatCompletionRequest
  Response: CreateChatCompletionResponse  (200) | ErrorResponse (4xx, 5xx)
  Streaming: yes (text/event-stream when request.stream=true)
```

Fuzzy-suggest on miss:
```
✗ No operation named 'createChatCompltion'.
  Did you mean: createChatCompletion (POST /v1/chat/completions)
```

### `server remove <selector>`

Updates TOML only; warns about potentially dead handler code.

## Generated file layout

```
src/generated/
  mod.rs              ← always regenerated
  models.rs           ← always regenerated; pruned to reachable types
  server/
    mod.rs
    api.rs            ← one trait per tag (or single trait if no tags)
    router.rs         ← combined Router factory
    extractors.rs     ← BearerAuth + typed header/query/path wrappers
    errors.rs         ← per-operation response enums (one variant per documented status)
```

User-owned, untouched by generator:

```
src/
  handlers.rs         ← impl ChatApi for State { ... }
  main.rs             ← wires router + state + listener
```

## Trait shape

```rust
#[axum::async_trait]
pub trait ChatApi: Send + Sync + 'static {
    async fn create_chat_completion(
        &self,
        auth: BearerAuth,
        body: CreateChatCompletionRequest,
    ) -> CreateChatCompletionResponse;
}
```

## Typed response enums

One variant per documented status. Variant carries the documented body type.
`IntoResponse` impl maps variant → `(StatusCode, Json)`.

```rust
pub enum CreateChatCompletionResponse {
    Ok(ChatCompletionResponse),
    BadRequest(ErrorResponse),
    TooManyRequests(ErrorResponse),
}
```

Status codes are picked by picking the variant — no stringly-typed status, no
forgotten error paths.

## Streaming (SSE)

For responses with `content: text/event-stream`, the response variant carries
an `Sse<BoxStream<'static, Result<Event, Infallible>>>`. Reuses event types
already generated for the client.

For endpoints where streaming is conditional on a body field (e.g.
`stream: bool` on chat completions), the response enum has both `Ok(...)`
and `OkStream(Sse<...>)` variants — user returns whichever matches.

## Router factory

```rust
pub fn router<C, E>(chat: C, embeddings: E) -> axum::Router
where
    C: ChatApi + Clone,
    E: EmbeddingsApi + Clone,
{ /* ... */ }
```

Single tag ⇒ single generic. Multiple tags ⇒ one generic per trait.

## Model pruning

Only types transitively reachable from picked operations land in `models.rs`.
Reuses the reachability pass that the client generator already has.

## Tradeoffs / open questions

- **`tag:` expansion is silently breaking.** Adding an op to a tag upstream
  changes the trait the user implements. Mitigation: doc guidance + warning
  on `generate` when a tag selector expands vs. prior run.
- **Combined router signature noisy at many tags.** Not a real-world concern
  yet; if it becomes one, offer `router_split()` returning a struct of
  sub-routers the user `.merge()`s.
- **`#[axum::async_trait]` for now**; can swap to native AFIT once MSRV
  permits.

## Implementation sequence

1. `server list` — read-only, lowest risk, builds analysis primitives.
2. Config schema + selector resolution (parser, fuzzy-match, errors).
3. `server add` / `server remove` — TOML edits.
4. Trait + response-enum emitter.
5. Router factory + extractors + reachability pruning.
6. Snapshot tests against `specs/openai.yaml`, canonical case
   `createChatCompletion`.

Each phase ships standalone and is reviewable independently.
