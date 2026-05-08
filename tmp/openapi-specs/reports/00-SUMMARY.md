# OpenAPI 3.1 / 3.2 Conformance Audit — Consolidated Summary

Source specs (downloaded into `tmp/openapi-specs/`):
- `openapi-3.1.0.md` (2021-02-15)
- `openapi-3.1.2.md` (2025-09-19, latest 3.1)
- `openapi-3.2.0.md` (2025-09-19, latest)

Six per-area reports live alongside this file. This is the cross-cutting executive view.

| # | Area | Report |
|---|------|--------|
| 1 | Schema / JSON Schema 2020-12 | `01-schema.md` |
| 2 | Paths, Operations, Parameters | `02-paths-parameters.md` |
| 3 | Request/Response/Media/Encoding | `03-bodies-media.md` |
| 4 | Servers & Security | `04-servers-security.md` |
| 5 | Components, Refs, Webhooks, Callbacks, Links, Tags | `05-components-refs-webhooks.md` |
| 6 | 3.2-only deltas vs 3.1.x | `06-three-two-deltas.md` |

---

## The headline

The README claims OpenAPI 3.1 support. In practice the generator implements **a subset of OpenAPI 3.0 with JSON-Schema-2020-12 superficially layered on top**. A 3.1- or 3.2-only spec parses without error, **silently** drops most of its semantics, and may emit a client missing whole operations or required headers.

There is no version check (`openapi` field is read once at `src/cli.rs:114` for a verbose print) and no `deny_unknown_fields` anywhere. Every parsing struct in `src/openapi.rs` ends with `#[serde(flatten)] pub extra: BTreeMap<String, Value>`, which is the architectural reason most gaps are invisible.

---

## The cross-cutting root causes

1. **`serde(flatten) extra` everywhere → silent swallowing.** Affects every section. Verified: zero `deny_unknown_fields` in `src/`. Six audits independently arrived at the same conclusion.
2. **Whole top-level OAS objects have no Rust struct.** `Server`, `SecurityScheme`, `OAuth Flows`, `Encoding`, `Header`, `Example`, `Link`, `Callback`, `Tag`, `ExternalDocs`, `Webhooks`, `pathItems` components — none modeled. They live in `OpenApiSpec.extra` / `Components` `extra`.
3. **Many fields are parsed into structs but never read.** `auth_config` variants, validation keywords (`min_length`, `pattern`, `minimum`, …), `Parameter.style/explode/allowReserved`, `summary`/`description` for rustdoc, `tags`, `deprecated`, per-op `security`/`servers`/`callbacks`/`externalDocs`. Dead-on-arrival fields.
4. **HTTP method coverage is hardcoded to 3.0's eight verbs.** Even within those, `options`/`head`/`trace` fall through to `_ => "get"` at `src/client_generator.rs:704-715`. 3.2's new `query` method and `additionalOperations` map disappear entirely.

---

## Top severity-ranked silent failures

Ranked by likelihood of producing a wrong but compilable client on real-world specs.

| Rank | Symptom | Location | Spec source |
|------|---------|----------|-------------|
| 1 | **Header request parameters silently dropped.** Parsed into `ParameterInfo` but `generate_query_params` and `generate_request_param` filter only on `path`/`query`. Required headers (`Idempotency-Key`, `X-API-Version`, …) never reach the wire. | `client_generator.rs:613-673, 717-775` | OAS 3.x Parameter `in: header` |
| 2 | **Webhooks (3.1+) ignored.** `analysis.rs:3493-3519` only walks `spec.paths`; `webhooks` is an unread blob. The repo's own `specs/openai.yaml`, `specs/lithic.yaml`, `specs/imagekit.yaml` ship `webhooks:` and lose them. | `analysis.rs:3493-3519` | 3.1 §"OpenAPI Object", 3.2 §87 |
| 3 | **Auth config branches are dead.** `auth_type = ApiKey` / `Custom` validate in TOML and never reach codegen. `client_generator.rs:596-599` emits unconditional `req.bearer_auth(api_key)` for every operation. README claim is misleading. | `config.rs:322-337, 453-471` vs `client_generator.rs:596-599` | 3.x §Security Scheme |
| 4 | **3.1's canonical `type: ["string","null"]` falls through to `Schema::Untyped`.** `Schema::Typed` carries one `SchemaType`, not a `Vec`; the array form is unhandled. The generator still honors 3.0's `nullable` field instead. | `openapi.rs:69-74, 91-101` | 3.1.0 §Schema, JSON Schema 2020-12 |
| 5 | **3.2 `additionalOperations` + `query` operation invisible.** `PathItem.operations()` enumerates the 8 fixed methods. Operations declared with new 3.2 fields are dropped from the generated client without warning. | `openapi.rs:391-411, 415-442` | 3.2.0 §Path Item |
| 6 | **Path Item `$ref` and `pathItems` components unsupported.** No `$ref` field on `PathItem`; refs deserialize as empty operations. | `openapi.rs:390-411` | 3.1+ §Path Item, §Components |
| 7 | **Components-only specs (no `paths`) error out.** Allowed by the OAS spec since 3.1 (and especially with `webhooks:`-only specs in 3.1+). | `analysis.rs:646-648` | 3.1+ §OpenAPI Object |
| 8 | **`additionalProperties: <schema>` collapses to `BTreeMap<String, serde_json::Value>`.** Self-flagged TODO. Typed dictionaries always lose type info. | `analysis.rs:1328-1332` | OAS Schema |
| 9 | **Non-JSON success responses become `()`.** `analyze_single_operation` only calls `response.json_schema()`. Affects `text/plain`, `application/octet-stream`, XML, `text/event-stream`. | `analysis.rs:3615` | OAS Media Type |
| 10 | **Response Header object not modeled.** Headers reachable only on the error path; success path has no typed headers. | `client_generator.rs:902, 914` | OAS Header Object |
| 11 | **Range status codes (`2XX`/`4XX`/`5XX`) explicitly skipped.** `// follow-up` comment marks them unimplemented. | `client_generator.rs:942-944` | OAS Responses |
| 12 | **`format` only handles numeric.** `int32/int64/float/double` mapped; `date-time`, `uuid`, `byte`, `binary`, `email`, `uri` all collapse to `String`. | `analysis.rs:3091-3106` | OAS Schema, JSON Schema |
| 13 | **Validation keywords are read-once-then-ignored.** `min_length`, `max_length`, `pattern`, `minimum`, `maximum` deserialize into `SchemaDetails` but have zero read sites in codegen. `multipleOf`, `prefixItems`, `patternProperties`, `unevaluatedProperties`, `dependent*`, `contains/minContains/maxContains`, `contentEncoding/MediaType/Schema` aren't even in the struct. | `openapi.rs SchemaDetails`, no read sites | JSON Schema 2020-12 |
| 14 | **Path templating does no percent-encoding.** Direct interpolation. Violates 3.2 spec line 521. | `client_generator.rs:1026-1067` | OAS Path Templating |
| 15 | **`operationId` collisions silently overwrite.** | `analysis.rs:3514` | OAS Operation Object |
| 16 | **`$dynamicRef` / `$dynamicAnchor` (JSON Schema 2020-12) not modeled.** Only the obsoleted `$recursiveRef`/`$recursiveAnchor` is. `$defs` is unreachable — `extract_schemas` enumerates only `components/schemas`. | `openapi.rs:43-49, 108-109` | JSON Schema 2020-12 |
| 17 | **Operation `summary`/`description`/`tags`/`deprecated` never make it to rustdoc or module structure.** All operations land as flat methods on a single `HttpClient`. | `client_generator.rs:35-46, 676-684` | OAS Operation, Tag |
| 18 | **`required: false` on request bodies ignored.** Method param is always non-`Option`. | `openapi.rs RequestBody` | OAS Request Body |
| 19 | **All `style`/`explode`/`allowReserved`/`allowEmptyValue`/`content`-typed parameters unimplemented.** Default form/simple for primitives only; `array`/`object` parameters degrade to `String`. | `analysis.rs:3767-3782` | OAS Parameter, RFC6570 |
| 20 | **`$ref`-typed parameters lose their type.** `schema_ref` populated but never consulted in `get_param_rust_type`. Inline string enums work; `$ref`-ed enums don't. | `client_generator.rs:778-791` | OAS Reference |

---

## 3.2-specific deltas (and how the codebase reacts)

3.2.0 was released 2025-09-19, jointly with the 3.1.2 patch. About 15 net-new features. **All currently fall into category (b): parses but silently ignored.** None handled, none cause parse failure.

Highest priority (by spec prominence × real-world likelihood):

1. `additionalOperations` + `query` HTTP method on Path Item
2. `in: "querystring"` parameter location
3. `itemSchema` / `prefixEncoding` / `itemEncoding` on Media Type
4. OAuth `deviceAuthorization` flow + `oauth2MetadataUrl`
5. Tag Object's new `kind` / `parent` / `summary`
6. `$self` keyword (and the new base-URI resolution rules in Appendices F/G)
7. `mediaTypes` components bucket
8. Server `name` field
9. Discriminator's new `defaultMapping`
10. Security Scheme `deprecated`

Worst failure mode: **a 3.2-only spec parses, runs, and emits a client that looks fine but may be missing whole operations** (the `additionalOperations` case). For a code generator this is the worst possible failure mode.

---

## Recommended remediation order

A pragmatic sequence if the goal is "claim 3.1 honestly first, then 3.2":

**Phase 0 — Stop the silent failures (small, high-ROI):**
- Add a version gate that warns on `openapi` outside `^3\.[01]\.`.
- Switch every spec struct to `#[serde(deny_unknown_fields)]` *or* surface a parse-time warning enumerating dropped fields. Without this, no future fix is testable.
- Wire header parameters through `generate_query_params` / `generate_request_param`.
- Emit operations for `options`/`head`/`patch`/`trace` instead of falling back to GET.
- Wire `auth_config` variants (`ApiKey`, `Custom`) through `client_generator.rs:596-599`. Or remove the variants and update README.
- Reject `paths`-less specs with a real error or implement the components-only path.

**Phase 1 — Honest 3.1 support:**
- Accept `type: [...]` arrays and produce nullable types from them; remove the 3.0 `nullable` fallback or document its dual handling.
- Ingest `webhooks:` (same shape as paths). Decide whether to emit handler stubs or just types.
- Resolve `pathItems` components and Path Item `$ref`.
- Handle `additionalProperties: <schema>` as `BTreeMap<String, T>`.
- Fix `format` coverage for `date-time`, `uuid`, `byte`, `binary`, `email`, `uri`.
- Surface `description`/`summary`/`deprecated`/`tags` into rustdoc and (optionally) tag-based module organization.

**Phase 2 — 3.2 support:**
- Add `additionalOperations` + `query` to `PathItem`.
- Add the new Media Type fields (`itemSchema`, `prefixEncoding`, `itemEncoding`).
- Add `$self`; align `$ref` resolution with Appendices F/G.
- Add Tag `kind`/`parent`/`summary`; consider using them for module layout.
- Add Discriminator `defaultMapping`.
- Add OAuth device flow and `oauth2MetadataUrl` (only useful once auth_config is honest).

**Phase 3 — JSON Schema 2020-12 conformance:**
- Replace `$recursiveRef`/`$recursiveAnchor` with `$dynamicRef`/`$dynamicAnchor`.
- Walk `$defs` in addition to `components/schemas`.
- Honor validation keywords either as runtime checks (using `validator` crate already in `Cargo.toml`) or at minimum as rustdoc.
- Add `prefixItems`, `patternProperties`, `propertyNames`, `unevaluatedProperties`, `dependentRequired`/`dependentSchemas`, `contains`/`minContains`/`maxContains`, `contentEncoding`/`contentMediaType`/`contentSchema`.

---

## Verification approach

The audits are static. Before publishing fixes, every claim above should be verified with a fixture-based test that round-trips a spec snippet through `analyze` + codegen and asserts on the generated tokens. Several of these silent failures (e.g. webhooks in `specs/openai.yaml`) are already smoke-testable from in-repo fixtures.
