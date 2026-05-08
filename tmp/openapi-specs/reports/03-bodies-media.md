# Audit: Request Body, Response, Media Type, Encoding, and Headers

Scope: how `/Users/jameslal/workspace/jl/openapi-generator` (a reqwest-based Rust client generator) handles OpenAPI 3.1 / 3.2 request bodies, responses, media-type objects, encoding objects, and headers.

Files covered:

- `/Users/jameslal/workspace/jl/openapi-generator/src/openapi.rs`
- `/Users/jameslal/workspace/jl/openapi-generator/src/analysis.rs`
- `/Users/jameslal/workspace/jl/openapi-generator/src/client_generator.rs`
- `/Users/jameslal/workspace/jl/openapi-generator/src/streaming.rs`

Spec sources cross-referenced:

- `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.1.2.md`
- `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.2.0.md`

Status legend: **Supported** = parsed and used in codegen; **Partial** = parsed but only partially used or only used in narrow conditions; **Ignored** = parsed (deserialized into `extra` / available on the type) but never read by codegen; **Unsupported** = not modeled at all in `src/openapi.rs`.

## 1. Summary table

| Area | Field / Concept | Status | Evidence |
| --- | --- | --- | --- |
| Request Body | `content` map | Supported | `openapi.rs:476`, `analysis.rs:3583-3610` |
| Request Body | `required` | Ignored | `openapi.rs:478` (parsed); no readers in `src/` |
| Request Body | `description` | Ignored | `openapi.rs:477`; no readers |
| Request Body | Multiple media types per body (branching) | Unsupported (one is picked) | `openapi.rs:542-561` `best_content` |
| Request Body | `application/json` (incl. `+json`) | Supported | `openapi.rs:490-504`, `client_generator.rs:798-803` |
| Request Body | `application/x-www-form-urlencoded` | Supported (basic) | `openapi.rs:508-516`, `client_generator.rs:804-808` |
| Request Body | `multipart/form-data` | Partial — caller must build `reqwest::multipart::Form` themselves; no schema-derived Form | `client_generator.rs:758-760`, `810-814` |
| Request Body | `application/octet-stream` | Partial — typed as `Vec<u8>` only; no `bytes::Bytes` / `Stream` / `AsyncRead` | `client_generator.rs:761-763`, `815-820` |
| Request Body | `text/plain` | Supported | `client_generator.rs:764-765`, `821-826` |
| Request Body | Other media types (`application/xml`, `image/*`, etc.) | Unsupported | falls off `best_content` PRIORITY list at `openapi.rs:549-559` |
| Media Type Object | `schema` | Supported | `openapi.rs:591` |
| Media Type Object | `example` (singular) | Ignored | only kept in `extra` at `openapi.rs:592-593` |
| Media Type Object | `examples` (map) | Ignored | only kept in `extra` |
| Media Type Object | `encoding` map | Ignored / Unsupported | not modeled anywhere |
| Media Type Object | **3.2** `itemSchema` | Unsupported | not in `MediaType` struct |
| Media Type Object | **3.2** `prefixEncoding` | Unsupported | not in `MediaType` struct |
| Media Type Object | **3.2** `itemEncoding` | Unsupported | not in `MediaType` struct |
| Encoding Object | `contentType` / `headers` / `style` / `explode` / `allowReserved` | Unsupported | no `Encoding` type exists |
| Encoding Object | **3.2** nested `encoding` / `prefixEncoding` / `itemEncoding` | Unsupported | as above |
| Responses | numeric status codes | Supported | `analysis.rs:3613-3637`, `client_generator.rs:925-961` |
| Responses | `default` response | Partial — typed deser is skipped, raw body falls into a generic arm | `client_generator.rs:937` and 964-994 |
| Responses | range codes (`2XX`, `4XX`, `5XX`) | Unsupported (treated as opaque key, no match arm) | `client_generator.rs:942-944` "follow-up" |
| Responses | success picking (200 → 201 → any 2xx) | Supported | `client_generator.rs:838-848` |
| Response Object | `description` | Ignored | `openapi.rs:567`; no readers |
| Response Object | **3.2** `summary` | Unsupported | not modeled |
| Response Object | `content` map | Supported (JSON-only) | `analysis.rs:3613-3637` |
| Response Object | `headers` | Unsupported (response headers are not modeled) | not in `Response` struct (`openapi.rs:566-571`) |
| Response Object | `links` | Unsupported | not modeled |
| Response | non-JSON response bodies (text/plain, octet-stream, XML, image/*) | Unsupported (caller gets `()`; raw body still on error path only) | `analysis.rs:3615` only calls `response.json_schema()` |
| Streaming | `text/event-stream` auto-detection from spec | Unsupported (config defined but never wired) | `streaming.rs:191-228` types declared, never consumed |
| Streaming | SSE codegen via user `StreamingConfig` | Supported | `streaming.rs`, `cli.rs:13-167`, `config.rs:473-526` |
| Header Object (response) | `schema`, `style`, `explode`, `required`, `deprecated`, `description`, `example(s)`, `content` | Unsupported | not modeled at all |
| Header parameters (request, `in: header`) | name, schema | Parsed but dropped during codegen | `analysis.rs:3640-3647`; `client_generator.rs` only emits `path`/`query` (`617`, `723`, `733`, `1035`) |
| Cookie parameters | `in: cookie` | Parsed but dropped | same as above |
| Examples vs example | `example`, `examples` | Ignored | both fall into `MediaType.extra` |
| **3.2** Example Object `dataValue` / `serializedValue` | new shapes for examples | Unsupported | not modeled (Example Object itself is not modeled) |

## 2. Detailed findings with file:line citations

### Request Body Object

OpenAPI types are at `openapi.rs:473-481`:

```rust
pub struct RequestBody {
    pub content: Option<BTreeMap<String, MediaType>>,   // openapi.rs:476
    pub description: Option<String>,                    // openapi.rs:477
    pub required: Option<bool>,                         // openapi.rs:478
    pub extra: BTreeMap<String, Value>,                 // openapi.rs:479-480
}
```

`required` and `description` are deserialized but never read anywhere in `src/`:

- `analysis.rs:3583-3610` — only consults `request_body.best_content()` to pick one media type and schema. `required` is never consulted.
- `client_generator.rs:747-768` — emits the request param using only the picked content; the param is always **non-optional** in the generated method signature, regardless of `required`. So a request body declared `required: false` is still required in Rust.

Selection logic is at `openapi.rs:528-561`:

- `RequestBody::json_schema` — JSON content type (canonical or RFC6839 `+json`).
- `RequestBody::best_content` — JSON first, then a fixed priority list of `application/x-www-form-urlencoded`, `multipart/form-data`, `application/octet-stream`, `text/plain`. Anything else (XML, `image/*`, custom vendor types not `+json`, `text/csv`, etc.) is silently dropped — the operation will be generated as if it has no body.

Codegen for the picked content at `client_generator.rs:747-826`:

| Picked media type | Generated parameter | Generated body call |
| --- | --- | --- |
| `application/json` (and `*+json`) | `request: <Schema>` (`753-756`) | `.body(serde_json::to_vec(&request)?)` + content-type header (`798-803`) |
| `application/x-www-form-urlencoded` | `request: <Schema>` (`753-756`) | `serde_urlencoded::to_string(&request)?` + content-type header (`804-808`) |
| `multipart/form-data` | `form: reqwest::multipart::Form` (`759`) | `.multipart(form)` (`810-814`) |
| `application/octet-stream` | `body: Vec<u8>` (`762`) | `.body(body)` + content-type (`815-820`) |
| `text/plain` | `body: String` (`765`) | `.body(body)` + content-type (`821-826`) |

Multipart: there is **no schema-driven Form construction** — the caller must build `reqwest::multipart::Form` by hand. The multipart schema is therefore effectively unused (no per-field types, no field-name validation, no encoding hints).

Octet-stream: typed as `Vec<u8>`, so the entire body must be in memory; no streaming upload (`Stream<Item = Result<Bytes, _>>`, `reqwest::Body::wrap_stream`, etc.).

### Media Type Object

Types are at `openapi.rs:588-594`:

```rust
pub struct MediaType {
    pub schema: Option<Schema>,                   // openapi.rs:591
    pub extra: BTreeMap<String, Value>,           // openapi.rs:592-593
}
```

Only `schema` is modeled. **Everything else** — `example`, `examples`, `encoding`, plus the new 3.2 fields `itemSchema`, `prefixEncoding`, `itemEncoding` — is parsed into `extra` and never used.

Cross-reference 3.1.2 (`openapi-3.1.2.md:1536-1551`) vs 3.2.0 (`openapi-3.2.0.md:1248-1267`) confirms that `itemSchema`, `prefixEncoding`, `itemEncoding` are **3.2-only**, but the implementation is missing both the 3.1 fields (`example`, `examples`, `encoding`) and the new 3.2 fields uniformly.

### Encoding Object

Not modeled anywhere. There is no `Encoding` struct in `openapi.rs`. Therefore none of `contentType`, `headers`, `style`, `explode`, `allowReserved`, or 3.2's nested `encoding`/`prefixEncoding`/`itemEncoding` are available to codegen.

Concretely: the form-urlencoded body emitted at `client_generator.rs:806` always uses default `serde_urlencoded` serialization. The spec says (`openapi-3.2.0.md:1739-1741`) that `style`/`explode`/`allowReserved` apply to form bodies; the generator silently ignores them. Same for multipart — there is no `Content-Type` per-part hint, no `headers`, no `Content-Transfer-Encoding`.

### Responses Object

Iteration at `analysis.rs:3613-3637`:

```rust
for (status_code, response) in responses {
    if let Some(schema) = response.json_schema() {
        ...
    }
}
```

Three notable things:

1. Only JSON responses are recorded. `response.json_schema()` (`openapi.rs:580-585`) only looks at `application/json` and `*+json`. Any operation whose only declared response is `text/plain`, `text/event-stream`, `application/xml`, `image/*`, `application/octet-stream`, etc. will yield an empty `response_schemas`, and `get_response_type` (`client_generator.rs:851-860`) will return `()`.
2. Range keys (`2XX`, `4XX`, `5XX`) are inserted into the `response_schemas` map verbatim with the literal range string — but `client_generator.rs:937-944` explicitly returns `None` for non-numeric, non-`default` keys ("// Range like \"4XX\" / \"5XX\" — fall through to generic for now; declared-range handling is a follow-up.") so no match arm is generated. The schema for a range-keyed response is therefore unreachable — at runtime, `404` will hit the catch-all and not be deserialized into the typed range schema.
3. `default` is recognized as a key (`client_generator.rs:937, 968`) but is **not** used to choose a typed deserialization. The arm-generator returns `None` for `default`, and the fallback arm only deserializes into `serde_json::Value` when there is no other typed enum in scope. So a spec that only declares `default` for errors does work, but a spec that declares `400` plus `default` will see `default` silently dropped on the typed-deserialization path.

Success picking (`client_generator.rs:838-848`) prefers `200` → `201` → any other 2xx, in that order.

### Response Object

Type at `openapi.rs:564-571`:

```rust
pub struct Response {
    pub description: Option<String>,         // openapi.rs:567
    pub content: Option<BTreeMap<String, MediaType>>,
    pub extra: BTreeMap<String, Value>,
}
```

Missing fields:

- `headers` (3.1.2 `openapi-3.1.2.md:2011`, 3.2.0 `openapi-3.2.0.md:2162`) — not modeled. Response header schemas, types, and rate-limit headers like `X-Rate-Limit-*` (the spec example at `openapi-3.2.0.md:2202-2213`) are invisible to codegen. Generated clients return `headers: response.headers().clone()` only on the **error** path (`client_generator.rs:902, 914`). Successful responses give the caller no typed access to declared headers.
- `links` — not modeled.
- 3.2's new `summary` field (`openapi-3.2.0.md:2160`) — not modeled.

`description` is parsed but never used in doc-comment generation. `client_generator.rs:676-684` builds the operation doc comment from method+path only, ignoring response descriptions.

### Header Object

Not modeled at all. There is no `Header` struct in `openapi.rs`. The spec defines Header Object at `openapi-3.2.0.md:2656-2705` and `openapi-3.1.2.md:2586`. Because `Response` has no `headers` field and there is no Header type, the entire serialization concern (`style: simple`, `explode`, `schema` vs `content`) is a non-feature.

For **request** headers (parameters with `in: header`), the parameter is parsed by `analyze_parameter` at `analysis.rs:3756-3804`, but the resulting `ParameterInfo` is filtered out of method generation. Searching `client_generator.rs:617, 723, 733, 1035` shows the only locations consulted are `"path"` and `"query"`. Header (and cookie) parameters are silently dropped — operations whose required behavior depends on a header parameter (e.g. `Idempotency-Key`, `X-API-Version`, `Accept-Language`) cannot be invoked correctly through the generated client.

The streaming module at `streaming.rs:60-65, 175-188` defines `required_headers` / `auth_header` / `optional_headers` separately, but those are user-config inputs to the SSE generator, not derived from spec headers.

### Examples vs `example`

3.1+ deprecates singular `example` in favor of plural `examples` (Example Object map). The generator handles **neither**: `MediaType.extra` (`openapi.rs:592-593`) catches both, but no codegen reads them. `Parameter.extra` (`openapi.rs:469-470`) likewise. There is no Example Object struct at all, so `summary`, `description`, `value`, `externalValue`, and 3.2's `dataValue` / `serializedValue` are entirely unsupported.

Practically: examples never become Rust doc-comments, never become test fixtures, never feed into validation. This is fine for emitting a working client but is a missed opportunity.

### Streaming / SSE

`streaming.rs:1-239` defines a rich `StreamingConfig`/`StreamingEndpoint`/`EventFlow` model. Wiring through `cli.rs:13-167` and `config.rs:473-526` shows it is fed exclusively from external configuration (the user's `[streaming]` config section).

What is **not** there:

- `analysis.rs` does not scan `responses[*].content` for `text/event-stream` to auto-mark operations as streaming. `OperationInfo.supports_streaming` is hardcoded to `false` at `analysis.rs:3579-3580` (the comment explicitly says "Will be determined by StreamingConfig, not spec").
- `streaming.rs:191-228` defines `StreamingDetectionConfig`, `DetectedStreamingEndpoint`, and `StreamingDetectionResult`, but `grep -rn DetectedStreamingEndpoint src/` shows zero consumers. These types are declared but unused.

So a spec that conforms to OpenAPI by declaring `responses.200.content.text/event-stream.schema` (as anthropic-style and many modern APIs do) gets **no SSE codegen** unless the user separately configures `[streaming]` in the generator's config. From the operation side, the response will just appear as `()` because `text/event-stream` is filtered out by the JSON-only `response.json_schema()`.

## 3. Media-type / content negotiation support matrix

| Media type / pattern | Request body | Response body |
| --- | --- | --- |
| `application/json` | Supported (typed param) | Supported (typed return) |
| `application/<sub>+json` (RFC 6839) | Supported (treated as JSON) | Supported (treated as JSON) |
| `application/x-www-form-urlencoded` | Supported via `serde_urlencoded` | Unsupported (returns `()`) |
| `multipart/form-data` | Partial — caller hands in `reqwest::multipart::Form` themselves; schema unused | Unsupported |
| `application/octet-stream` | Partial — `Vec<u8>` only; no streaming | Unsupported |
| `text/plain` | Supported (`String` body) | Unsupported (returns `()`) |
| `text/event-stream` | Unsupported as a request body | Unsupported on the operation method; SSE codegen exists separately and is **only** driven by user config, not by spec content |
| `application/xml`, `image/*`, `audio/*`, `video/*`, `application/pdf`, vendor types not `+json` | Unsupported | Unsupported |
| `*/*`, `text/*`, `application/*` (range keys) | Unsupported (not in `best_content` priority) | Unsupported |
| `application/linkset` (3.2 link header content) | Unsupported | Unsupported |

Content negotiation: the generator does **not** branch. A request body with both `application/json` and `application/xml` will only emit the JSON path. A response with both `application/json` and `text/event-stream` will only see the JSON path (and SSE only if user-configured).

## 4. 3.2-specific deltas in this area

Direct quotes / paraphrase from `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.2.0.md`:

1. **Media Type Object — `itemSchema`** (`openapi-3.2.0.md:1262, 1318-1322`):
   > A schema describing each item within a sequential media type.
   > Unlike `schema`, which is applied to the complete content, `itemSchema` MUST be applied to each item in the stream independently, which supports processing each item as it is read from the stream.
   Not modeled. Generator has no notion of sequential / streaming media types per the spec; SSE is shoehorned through external config.

2. **Media Type Object — `prefixEncoding`** (`openapi-3.2.0.md:1266`):
   > An array of positional encoding information ... The `prefixEncoding` field SHALL only apply when the media type is `multipart`. ... This field MUST NOT be present if `encoding` is present.
   Not modeled.

3. **Media Type Object — `itemEncoding`** (`openapi-3.2.0.md:1267`):
   > A single Encoding Object that provides encoding information for multiple array items ... only apply when the media type is `multipart`.
   Not modeled.

4. **Encoding Object nested `encoding` / `prefixEncoding` / `itemEncoding`** (`openapi-3.2.0.md:1711-1713`):
   > Applies nested Encoding Objects in the same manner as the Media Type Object's `encoding` / `prefixEncoding` / `itemEncoding` field.
   The Encoding Object isn't modeled at all.

5. **Response Object `summary`** (`openapi-3.2.0.md:2160`):
   > A short summary of the meaning of the response.
   New in 3.2 (3.1.2 `Response Object` table at `openapi-3.1.2.md:2008-2013` does not list it). Not modeled.

6. **Response Object `description` is no longer REQUIRED in 3.2.0** (`openapi-3.2.0.md:2161` lacks the `**REQUIRED**.` marker that `openapi-3.1.2.md:2010` carries). Codegen does not validate either way, so this is a no-op for the implementation but worth flagging for spec-validation tools.

7. **Example Object new fields `dataValue` / `serializedValue`** (`openapi-3.2.0.md:2332-2335`) and the deprecation note on `value`:
   > **Deprecated for non-JSON serialization targets:** Use `dataValue` and/or `serializedValue` ...
   The Example Object isn't modeled at all. 3.2's deprecation does not affect this generator because it never reads examples, but emitting them as Rust doc strings is now a 3.2-aware feature this codebase entirely misses.

8. **Header Object example/examples explicit table** (`openapi-3.2.0.md:2666-2705`) — the table layout differs slightly from 3.1.2 with the `example`/`examples` rows now in a "Common Fixed Fields" subsection, plus an explicit split between schema-mode and content-mode header serialization. Not modeled either way, since Header Object is missing.

9. **Encoding `headers` field clarified to "ignored if media type is not `multipart`"** (`openapi-3.2.0.md:1710`) — the same restriction is in 3.1.2 (`openapi-3.1.2.md:1684`), so this is a clarification rather than a hard delta. Encoding is not modeled regardless.

## 5. Top gaps ranked by likely real-world impact

1. **Header parameters on requests are silently dropped.** This is the highest-impact bug. Many real APIs require headers like `Idempotency-Key`, `Stripe-Account`, `Anthropic-Version`, `X-API-Version`, `Accept-Language`. `analyze_parameter` records them (`analysis.rs:3756`), but the generator only emits `path` and `query` (`client_generator.rs:617, 723, 733, 1035`). Operations are silently miscompiled — they will compile but make wire calls missing required headers.
2. **Response headers are entirely invisible.** No Header Object, no `Response.headers`. Rate limits, pagination cursors (`Link`, `X-Next-Page-Token`), and `Idempotency-Key` echoes are unreachable on the success path. Response headers are only available on the **error** path via raw `response.headers().clone()` (`client_generator.rs:902, 914`).
3. **Non-JSON success bodies → `()`.** Because `analysis.rs:3615` only consults `response.json_schema()`, an operation whose 200 response is `text/plain`, `application/octet-stream`, `application/xml`, `image/*`, or `text/event-stream` is generated as returning `()`. The body is consumed (`client_generator.rs:903`) but discarded. For binary downloads, file-export endpoints, and any plain-text endpoint, this is a hard blocker.
4. **Range response codes (`2XX`, `4XX`, `5XX`) generate no match arms.** `client_generator.rs:942-944` explicitly bails. This is widespread in modern specs (e.g. `4XX → ProblemDetails`). Today such schemas register but never deserialize. Stripe-style single-`default`-error specs work; range-style specs silently degrade to untyped errors.
5. **SSE auto-detection is missing.** The SSE codegen is fully spec-blind; it requires a user-maintained `[streaming]` config block. A perfectly valid OpenAPI spec that declares `text/event-stream` responses generates a non-streaming `()` method. Worse, the dead types `StreamingDetectionConfig` / `DetectedStreamingEndpoint` (`streaming.rs:191-228`) suggest detection was planned but never wired.
6. **`multipart/form-data` schemas are unused.** Caller is required to assemble `reqwest::multipart::Form` by hand (`client_generator.rs:759`). All field-name correctness, types, encoding hints, per-part `Content-Type` (the central use case for the Encoding Object) is up to the caller. For file-upload-heavy APIs (DocuSign, S3-style, Slack files, OpenAI file upload) this turns the generated client into a thin reqwest wrapper.
7. **`application/octet-stream` upload is buffered.** `body: Vec<u8>` (`client_generator.rs:762`) forces full buffering. Large-file uploads cannot stream. A `reqwest::Body::wrap_stream` / `bytes::Bytes` path is missing.
8. **`required: false` request bodies are emitted as required Rust parameters.** `RequestBody.required` is parsed (`openapi.rs:478`) but never read. The method signature always takes the body as a non-`Option`. Operations with optional bodies are over-constrained.
9. **`default` response is parsed but contributes no typed deserialization arm.** `client_generator.rs:937` returns `None` for `"default"`. When other typed errors exist, `default` silently degrades to "no typed payload, no parse_error" instead of being wired to the operation's error enum's `Default` variant. The variant identifier function (`client_generator.rs:544`) maps `"default"` → `Default`, but no arm ever populates it.
10. **Encoding Object is unmodeled.** All form / multipart `style`, `explode`, `allowReserved`, per-property `contentType`, per-part `headers` are inert. For form-encoded APIs that use deep-object serialization or per-field content types, the generator emits incorrect wire format.
11. **`application/xml` and other non-JSON, non-form, non-octet content** is dropped in both directions.
12. **3.2 streaming fields (`itemSchema` / `itemEncoding` / `prefixEncoding`)** — none of the 3.2 sequential/streaming-media-type machinery is supported. Specs that adopt 3.2 streaming idioms will look identical to non-streaming specs to this generator.
13. **Examples never reach generated code.** `MediaType.example`, `MediaType.examples`, `Parameter.example(s)`, and 3.2's `Example.dataValue` / `serializedValue` could become Rust doc-comments or test fixtures. Today they're stripped into `extra` and discarded.

## Appendix: representative file:line index

- `openapi.rs:475-481` — `RequestBody`
- `openapi.rs:490-516` — `is_json_media_type`, `is_form_urlencoded_media_type`
- `openapi.rs:518-561` — `find_json_content`, `RequestBody::json_schema`, `RequestBody::best_content`
- `openapi.rs:564-585` — `Response`, `Response::json_schema`
- `openapi.rs:588-594` — `MediaType`
- `analysis.rs:100-122` — `OperationInfo`
- `analysis.rs:124-143` — `RequestBodyContent` enum (5 variants)
- `analysis.rs:3583-3610` — request body content-type dispatch
- `analysis.rs:3613-3637` — response schema extraction (JSON only)
- `client_generator.rs:540-565` — error variant naming and op-error type token
- `client_generator.rs:617, 723, 733, 1035` — only `path`/`query` are consumed
- `client_generator.rs:747-826` — request param + body codegen by content type
- `client_generator.rs:838-860` — success picking and response type
- `client_generator.rs:870-1011` — error handling, match arms, default arm
- `streaming.rs:5-93` — user-supplied `StreamingConfig` / `StreamingEndpoint`
- `streaming.rs:191-228` — declared-but-unused detection types
