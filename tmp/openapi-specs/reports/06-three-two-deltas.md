# Report 06: OpenAPI 3.2.0 Deltas vs 3.1.2

Audit scope: Identify only what is **new in 3.2.0** relative to 3.1.2, then assess
how the Rust generator at `/Users/jameslal/workspace/jl/openapi-generator` handles
each new feature.

Sources:
- 3.1.2 spec: `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.1.2.md`
- 3.2.0 spec: `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.2.0.md`
- Spec diff dump: `/tmp/oas-diff.txt` (5,795 line diff)
- Revision history: 3.2.0 spec lines 4804-4827 (very brief; no detailed change log,
  so deltas are derived from a structural read of the diff)

Status legend:
- **a** handles correctly
- **b** parses but ignores (no error, but feature is silently dropped)
- **c** fails to parse
- **d** unknown / would need a real test to be certain

## 1. Summary

I identified **15 distinct 3.2-only features** (plus 1 reclassified-as-deprecated
field) that are new vs. 3.1.2. None of them are explicitly handled by the
generator. Of these, the Rust code base universally falls into category **(b)
parses but ignores**, because every parsing struct uses
`#[serde(flatten)] pub extra: BTreeMap<String, Value>` to absorb unknown
fields and **no struct uses `#[serde(deny_unknown_fields)]`**
(verified via `grep -n "deny_unknown" src/openapi.rs` returning zero hits).

Because of that catch-all, a 3.2-only document **will parse** — even if its
`openapi: "3.2.0"` header is unrecognized — and code generation will simply
proceed using the 3.1-style subset, silently dropping the 3.2-only fields.

## 2. Per-feature delta table

| # | Feature | Spec section / line | Status | Evidence |
|---|---------|--------------------|--------|----------|
| 1 | `$self` field on OpenAPI Object (self-assigned base URI for ref resolution) | `### OpenAPI Object` line 98 (3.2) | b | `OpenApiSpec` struct in `src/openapi.rs:7-14` defines only `openapi`, `info`, `paths`, `components`; `$self` falls into `extra: BTreeMap<String, Value>` (line 12-13). No code reads `$self`. References are resolved as raw strings without base-URI logic. |
| 2 | `query` HTTP method on Path Item (RFC-draft QUERY method with body) | `### Path Item Object` line 607 (3.2) | b | `PathItem` struct in `src/openapi.rs:391-411` enumerates only `get`/`put`/`post`/`delete`/`options`/`head`/`patch`/`trace`. `pub fn operations()` at `src/openapi.rs:415-442` iterates only those 8 methods. A `query` operation lands in `extra` and is never visited by `analyze_operations` (`src/analysis.rs:3493-3520`). No client method will be generated for it. |
| 3 | `additionalOperations` map on Path Item (extension HTTP methods e.g. `COPY`, `MKCOL`) | `### Path Item Object` line 608 (3.2) | b | Same evidence as #2. The map is absorbed into `PathItem.extra` and never iterated. |
| 4 | `name` field on Server Object (machine-readable server identifier) | `### Server Object` line 272 (3.2) | b | The codebase has **no `Server` struct at all** (`grep -n "Server\|servers" src/openapi.rs` returns 0 hits). `servers` arrays go into `OpenApiSpec.extra`. No URL-routing logic exists; clients use a single base URL passed at runtime. |
| 5 | `webhooks` map on OpenAPI Object | (already in 3.1; no change) | n/a | False alarm — `webhooks` is 3.1, not new in 3.2. Listed here for completeness; same handling status as #4 (absorbed into `extra`). |
| 6 | `summary` field on Tag Object | `### Tag Object` line 2898 (3.2) | b | No `Tag` struct exists in `src/openapi.rs`; `tags` array on root and on operations goes into `extra`. `grep -rn "tags" src/analysis.rs src/generator.rs src/client_generator.rs` shows no consumer of tag data. |
| 7 | `parent` field on Tag Object (tag hierarchy / nesting) | `### Tag Object` line 2901 (3.2) | b | Same as #6. |
| 8 | `kind` field on Tag Object (e.g. `nav`, `badge`, `audience`) | `### Tag Object` line 2902 (3.2) | b | Same as #6. |
| 9 | `summary` field on Info Object | `### Info Object` line 193 (3.2) | b | `Info` struct (`src/openapi.rs:17-23`) declares only `title` and `version`; `summary` falls into `Info.extra`. Generator never references info beyond title/version (verbose print at `src/cli.rs:111-115`). |
| 10 | `summary` field on Response Object | `### Response Object` line 2160 (3.2) | b | `Response` struct (`src/openapi.rs:566-571`) declares `description`, `content`; `summary` falls into `Response.extra`. |
| 11 | `in: "querystring"` parameter location (treats whole query string as one value, used with `content`) | `### Parameter Object` line 770, 789 (3.2) | b → potentially c | `Parameter.location` is typed as `Option<String>` (`src/openapi.rs:464-465`), so the literal `"querystring"` will deserialize fine. However, `analysis.rs` and `client_generator.rs` switch on the canonical four locations (`path`/`query`/`header`/`cookie`); a `querystring` parameter will likely be treated as unknown and silently skipped, and the associated `content` schema lost. Closer to **(b)** — parses, ignored — but generated client output for such operations will be wrong (no querystring serialization). |
| 12 | `style: "cookie"` value (new dedicated cookie style) | `#### Style Values` line 847 (3.2) | b | Style is unused in generator (parameter style is in `Parameter.extra` since `Parameter` struct has no `style` field — `src/openapi.rs:462-471`). |
| 13 | `itemSchema` on Media Type Object (per-item schema for sequential / streaming media types like JSON-Seq, NDJSON, SSE) | `### Media Type Object` line 1262 (3.2) | b | `MediaType` struct (`src/openapi.rs:590-594`) has only `schema`; `itemSchema` is absorbed into `extra`. Streaming module (`src/streaming.rs`) exists but is driven by a separate config (`StreamingConfig`), not by reading `itemSchema`. |
| 14 | `prefixEncoding` array on Media Type Object (positional encoding for `multipart` parts) | `### Media Type Object` line 1266 (3.2) | b | Same as #13 — `prefixEncoding` and `itemEncoding` lost into `MediaType.extra`. The codebase has no `Encoding` struct at all (`grep` for `Encoding` in `src/openapi.rs` returns 0 hits). |
| 15 | `itemEncoding` on Media Type Object (single Encoding Object applied to remaining items) | `### Media Type Object` line 1267 (3.2) | b | Same as #14. |
| 16 | `prefixEncoding` / `itemEncoding` on Encoding Object (nested) | `### Encoding Object` line 1712-1713 (3.2) | b | No `Encoding` struct in code. |
| 17 | Reusable `mediaTypes` map on Components Object | `### Components Object` line 414 (3.2) | b | `Components` struct (`src/openapi.rs:26-31`) declares only `schemas` and `parameters`; `mediaTypes` and the eight other component maps go into `Components.extra`. References to `#/components/mediaTypes/...` cannot be resolved. |
| 18 | `oauth2MetadataUrl` on Security Scheme Object (RFC 8414 metadata URL) | `### Security Scheme Object` line 4582 (3.2) | b | No `SecurityScheme` struct in `src/openapi.rs` (`grep` returns 0 hits). All security configuration is absorbed into `OpenApiSpec.extra` / `Components.extra`. The generator has its own `SecurityScheme` typing in `src/generator.rs` driven by config, not spec parsing. |
| 19 | `deprecated` on Security Scheme Object (whole scheme can be marked deprecated) | `### Security Scheme Object` line 4583 (3.2) | b | Same as #18. |
| 20 | `deviceAuthorization` flow on OAuth Flows Object + `deviceAuthorizationUrl` on OAuth Flow Object (RFC 8628) | `### OAuth Flows Object` line 4643, OAuth Flow Object line 4656 (3.2) | b | No OAuth Flow struct in spec parser; routed through `extra`. |
| 21 | `dataValue` and `serializedValue` on Example Object (preferred over now-deprecated `value` for non-JSON serializations) | `### Example Object` line 2332-2335 (3.2) | b | No `Example` struct in code; examples are not consumed by the generator at all. Both fields silently ignored. |
| 22 | Examples `examples` field added to Operation Object via header/parameter additions; new `Set-Cookie` modeling rules | various, e.g. `### Header Object` 2768-2786 (3.2) | b | No `Header` struct; headers in responses are inside `Response.extra` / `MediaType.extra`. |
| 23 | `allowEmptyValue` formally **deprecated** in 3.2 (was just NOT RECOMMENDED in 3.1) | `### Parameter Object` line 793 (3.2) | n/a | The codebase already does not act on `allowEmptyValue`; nothing to change. |
| 24 | Updated `text/event-stream` (Server-Sent Events) handling guidance with required parsing per HTML spec, `contentSchema` for data field | `#### Special Considerations for Server-Sent Events` line 1331-1357 (3.2) | b | `src/streaming.rs` handles SSE separately based on user config; the new spec-level guidance is not consumed automatically. |
| 25 | Reference to OpenAPI Description (OAD) multi-document parsing rules (`Resolving Implicit Connections`, `Establishing the Base URI`) | `### OpenAPI Object` lines 114-181 (3.2) | b | No multi-document support in the codebase — `parse_spec` (in `src/cli.rs`) loads a single document and references are stringly resolved against components from the same file. 3.2's stricter base-URI rules are silently ignored. |

(Items 5 and 23 are not new features; they are noted only for completeness.)

### Net distinct **net-new 3.2** features handled by the codebase

| Status | Count | Note |
|--------|-------|------|
| (a) handled | 0 | nothing |
| (b) parsed-but-ignored | ~15 | every new field |
| (c) fails to parse | 0 | because of pervasive `extra: BTreeMap<String, Value>` |
| (d) unknown | 0 | |

## 3. Highest-priority adds to support

Ranked by likelihood of appearing in real-world 3.2 specs and impact on a Rust
client generator:

1. **`additionalOperations` and `query` method on Path Item** (table rows #2, #3). 
   These are the most visible 3.2 changes for clients: any spec using QUERY,
   COPY, MKCOL, etc. will silently lose those operations from the generated
   client. **High impact** (whole operations missing) and **high prominence**
   in the spec.

2. **`in: "querystring"` parameter location** (#11). Real-world specs that use
   complex querystrings (e.g. GraphQL-over-GET, search APIs) will have
   parameter definitions that cannot be expressed in 3.1. The generator
   parses but ignores; runtime requests will be malformed.

3. **`itemSchema` on Media Type Object** (#13). Streaming APIs (JSON-Seq,
   NDJSON, SSE, multipart-mixed) increasingly use this. The generator already
   has a separate streaming subsystem; teaching it to read `itemSchema`
   instead of relying on user-supplied config would let streaming "just work".

4. **OAuth `deviceAuthorization` flow + `oauth2MetadataUrl`** (#18, #20).
   Common in IoT / CLI scenarios. The generator's auth subsystem could pick
   these up if it parsed Security Schemes from the spec rather than from
   external config.

5. **Tag `kind` and `parent`** (#7, #8). Useful for organizing generated
   client modules / docs by tag hierarchy. Lower runtime impact, but
   cosmetically nice.

6. **`$self`** (#1). Required for interoperable multi-document specs in 3.2.
   Low priority unless multi-document support is added.

7. **`mediaTypes` component reuse** (#17). Lower impact for code generation
   but breaks `$ref: "#/components/mediaTypes/..."` resolution today.

8. **Server `name`** (#4). Currently the generator does not consume `servers`
   at all, so this is moot until server-array handling is added.

## 4. Risk: Will a 3.2-only spec parse today?

**Yes, it will parse — silently and incorrectly.** Three reasons:

1. **No `openapi` version validation.** `OpenApiSpec.openapi` is `pub openapi: String`
   (`src/openapi.rs:8`) and is only used in a verbose print of `info.version`
   (`src/cli.rs:114`). Nothing rejects `"3.2.0"`, `"3.99.foo"`, or any other value.

2. **No `deny_unknown_fields` anywhere.** A `grep -n "deny_unknown" src/openapi.rs`
   returns zero hits. Every parsing struct (`OpenApiSpec`, `Info`, `Components`,
   `PathItem`, `Operation`, `Parameter`, `RequestBody`, `Response`, `MediaType`,
   `SchemaDetails`) has a `#[serde(flatten)] pub extra: BTreeMap<String, Value>`
   field that absorbs every unknown field as an opaque JSON value.

3. **Many root-level 3.2 features have no struct at all** (Servers, Tags,
   Webhooks, SecuritySchemes, OAuth flows, Encoding). Those are pure
   `BTreeMap<String, Value>` blobs that flatten into `OpenApiSpec.extra`.

What this means in practice for a 3.2 spec passed to the generator:

- The spec parses without error.
- `$self`, new tag fields, `webhooks`, server names, security scheme additions,
  itemSchema, prefixEncoding, etc. are all silently dropped.
- Operations on Path Items declared via `additionalOperations` or `query` are
  invisible to `analyze_operations`, so **whole API endpoints will be missing
  from the generated client**.
- `in: "querystring"` parameters will be parsed as a stringly-typed `location`
  value, then likely classified as "unknown location" and skipped, so request
  serialization for those operations will be wrong.
- The generator will emit a client that **looks fine but is incomplete** —
  worst-case behavior because there is no warning surface to alert the user.

**Recommendation (out of scope, read-only review):** the cheapest risk
mitigation is to (a) validate the `openapi` version string with a clear
error/warning when it begins with `"3.2"`, and (b) at minimum add
`additionalOperations` + `query` to `PathItem` so real-world 3.2 specs do
not silently lose operations.
