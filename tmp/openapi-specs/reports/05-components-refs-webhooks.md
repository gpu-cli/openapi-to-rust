# Audit: Components, References, Webhooks, Callbacks, Links, Tags

Repo: `/Users/jameslal/workspace/jl/openapi-generator` (Rust generator that
advertises "OpenAPI 3.1 support" — README.md:8, line 14).

Specs reviewed:
- `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.1.2.md`
- `/Users/jameslal/workspace/jl/openapi-generator/tmp/openapi-specs/openapi-3.2.0.md`

Read-only audit. No source modified.

---

## 1. Summary table

Status legend: **S** = supported, **P** = partial, **U** = unsupported (claim
absent or feature degraded), **I** = ignored (silently dropped into a flatten
catch-all).

### Top-level OpenAPI Object

| Field | 3.1+ / 3.2 | Status | Evidence |
| --- | --- | --- | --- |
| `openapi` | required | S | `openapi.rs:8` |
| `info` | required | P (only `title`/`version` typed; rest `extra`) | `openapi.rs:16-23` |
| `jsonSchemaDialect` | 3.1+ | I | not in `OpenApiSpec` (`openapi.rs:7-14`); falls into `extra` |
| `servers` | 3.0+ | I | not in `OpenApiSpec`; falls into `extra` |
| `paths` | optional in 3.1+ | S | `openapi.rs:10` |
| `webhooks` | 3.1+ | **U / I** | not in `OpenApiSpec`; falls into `extra` |
| `components` | 3.1+ | P (see next table) | `openapi.rs:11`, `openapi.rs:25-31` |
| `security` | 3.0+ | I | not in `OpenApiSpec`; falls into `extra` |
| `tags` | 3.0+ | I | not in `OpenApiSpec`; falls into `extra` |
| `externalDocs` | 3.0+ | I | not in `OpenApiSpec`; falls into `extra` |
| `$self` | 3.2 only | U / I | not in `OpenApiSpec`; falls into `extra`; no base-URI logic |
| Components-only spec (no paths, no webhooks) | 3.1+ allowed | **U (errors out)** | `analysis.rs:646-648` raises `InvalidSchema("No schemas found in OpenAPI spec")` if `components.schemas` is missing — even though the spec only requires *one of* `paths`/`webhooks`/`components` |

### Components Object buckets

| Bucket | 3.1.2 | 3.2.0 | Status | Evidence |
| --- | --- | --- | --- | --- |
| `schemas` | yes | yes | S | `openapi.rs:27`; consumed at `analysis.rs:641-655`, `549-554` |
| `parameters` | yes | yes | P (parsed; only inline parameter schemas consumed; `$ref` resolution to `#/components/parameters/...` at the operation level not exercised — see Findings §2) | `openapi.rs:28`; `analysis.rs:540`, `549-554` |
| `responses` | yes | yes | **U / I** | not in `Components` (`openapi.rs:25-31`); no consumer |
| `examples` | yes | yes | I | not in `Components`; no consumer |
| `requestBodies` | yes | yes | **U / I** | not in `Components`; no consumer |
| `headers` | yes | yes | I | not in `Components`; no consumer |
| `securitySchemes` | yes | yes | **U / I** | not in `Components`; no consumer |
| `links` | yes | yes | I | not in `Components`; no consumer |
| `callbacks` | yes | yes | **U / I** | not in `Components`; no consumer |
| `pathItems` | yes (3.1+) | yes | **U / I** | not in `Components`; no consumer |
| `mediaTypes` | — | **new in 3.2** | **U / I** | not in `Components`; no consumer |

`Components` keeps a `#[serde(flatten)] extra: BTreeMap<String, Value>`
(`openapi.rs:29-30`) which silently swallows every bucket except `schemas` and
`parameters`. Likewise `OpenApiSpec.extra` swallows `webhooks`, `tags`,
`servers`, `security`, `externalDocs`, `jsonSchemaDialect`, `$self`, etc.

### References

| Concern | Status | Evidence |
| --- | --- | --- |
| Internal `$ref` to `#/components/schemas/...` | S | `openapi.rs:37-42`; `analysis.rs:982-984`, `1048-1054` |
| `$ref` siblings: 3.1 Reference Object `summary`/`description` overrides | **U** | `Schema::Reference` only carries `reference` + `extra` (`openapi.rs:36-42`); `Parameter`, `RequestBody`, etc. are not modeled as `Reference \| Object`; siblings would deserialize into `extra` and never be emitted on doc comments |
| External-document `$ref` (e.g. `Pet.yaml`, `definitions.yaml#/Pet`) — examples in spec at 3.2:2953-2963 | **U** | `extract_schema_name` (`analysis.rs:974-995`) grabs the last URL segment ("Pet") and the analyzer then does `self.schemas.get(name)` (`analysis.rs:1006-1010`) which fails with `UnresolvedReference`. There is no document loader/resolver. The unrelated `merge_schema_extensions` (`analysis.rs:308-320`) is an opt-in JSON merge, not a `$ref` resolver. |
| Cyclical refs / mutual recursion | S | `analysis.rs:221-246` (`detect_recursive_schemas` + mutual-recursion sweep); placeholder cache at `analysis.rs:1012-1026` prevents infinite recursion |
| `$recursiveRef` / `$recursiveAnchor` (JSON Schema 2019-09) | S | `openapi.rs:43-49`, `openapi.rs:108-109`; resolved at `analysis.rs:1056-1072`, `3388-3405` |
| `$dynamicRef` / `$dynamicAnchor` (JSON Schema 2020-12 — the dialect 3.1+ uses) | **U** | No occurrences anywhere in `src/` (verified by grep: `grep -rn dynamicRef src/` returns nothing). The spec calls these out at 3.2:3265-3266, 3.1.2:2844-2845. |
| Self-reference `$ref: "#"` | partial | handled only as `$recursiveRef: "#"` (`analysis.rs:1058-1063`); a plain `$ref: "#"` would fall through `extract_schema_name` (`analysis.rs:975-977` short-circuits to `None`) and then error |
| Path Item `$ref` (allowed in 3.0+) | **U** | `PathItem` (`openapi.rs:390-411`) has no `$ref` field; if a path is itself a `$ref`, it deserializes with all eight HTTP-method fields `None`, the `$ref` lands in `extra`, and `path_item.operations()` (`openapi.rs:415-442`) returns an empty list — silently dropping the path |

### Callbacks

| Concern | Status | Evidence |
| --- | --- | --- |
| Operation `callbacks` field parsed | I | `Operation` (`openapi.rs:446-458`) does not list `callbacks`; it lands in `extra` |
| `components.callbacks` parsed | I | not in `Components` |
| Callback path-items keyed by runtime expression generated as code | **U** | nothing in `analysis.rs` / `client_generator.rs` walks callbacks |

### Links

| Concern | Status | Evidence |
| --- | --- | --- |
| Response `links` parsed | I | `Response` (`openapi.rs:565-571`) has no `links`; lands in `extra` |
| `components.links` parsed | I | not in `Components` |
| Runtime expressions / `operationRef` / `operationId` resolution | **U** | no consumer anywhere |
| Generated client uses links to wire follow-up calls | **U** | `client_generator.rs` emits flat methods on a single `HttpClient`; no link plumbing |

### Webhooks (3.1+)

| Concern | Status | Evidence |
| --- | --- | --- |
| Top-level `webhooks` parsed | **U / I** | `OpenApiSpec` (`openapi.rs:7-14`) has no `webhooks`; lands in `extra` |
| Operations under webhooks emitted (server-side handlers, deserializers, type registry) | **U** | `analyze_operations` (`analysis.rs:3493-3519`) iterates only `spec.paths`. Real specs in this repo's own corpus (`specs/lithic.yaml`, `specs/imagekit.yaml`, `specs/openai.yaml`) declare `webhooks:` at top level — all silently dropped. |
| Webhook `Path Item` may be a Reference Object | **U** | even if the field were typed, the same `PathItem-as-$ref` gap (above) would apply |

### Tags

| Concern | Status | Evidence |
| --- | --- | --- |
| Top-level `tags` parsed (Tag Object: `name`, `description`, `externalDocs`) | I | not in `OpenApiSpec`; lands in `extra` |
| Operation-level `tags` (array of strings) parsed | I | `Operation` (`openapi.rs:446-458`) doesn't carry `tags`; lands in `extra` |
| Tags used to organize generated client (per-tag modules / namespaces) | **U** | `client_generator.rs` emits one flat `HttpClient` with all operations as inherent methods (see preview in module doc, `client_generator.rs:35-46`). `OperationInfo` (`analysis.rs:100-122`) and `OperationDef` (`registry_generator.rs:120-138`) both lack a `tags` field. `grep -n "tags" src/{client_generator,generator,registry_generator}.rs` returns nothing. |
| External docs propagated to rustdoc | **U** | `generate_operation_doc_comment` (`client_generator.rs:676`) uses only `summary`/`description` fields it already has on `Operation` |

### External Documentation Object

Not modeled anywhere — neither at root, on `Tag`, on `Operation`, nor on
`Schema`. Falls into `extra` everywhere. `grep -rn externalDocs src/` returns
zero hits.

---

## 2. Detailed findings (file:line citations)

### 2.1 The data model is the single biggest gap

`OpenApiSpec` defines exactly four typed top-level fields plus a flatten
catch-all (`openapi.rs:7-14`):

```
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    pub paths: Option<BTreeMap<String, PathItem>>,
    pub components: Option<Components>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
```

`Components` is similarly minimal (`openapi.rs:25-31`) — only `schemas` and
`parameters` are typed; everything else (responses, requestBodies, headers,
examples, securitySchemes, links, callbacks, pathItems, and 3.2's mediaTypes)
goes into `extra`. Because of `serde(flatten)`, deserialization succeeds, but
**no downstream consumer ever inspects `extra`** for these buckets. A grep
across `src/` for `webhook`, `callback`, `pathItems`, `jsonSchemaDialect`,
`dynamicRef`, `$self`, `securityScheme`, `externalDocs`, `mediaTypes` returns
zero hits outside of test fixtures, snapshots, and HTTP header strings.

Net effect: every one of these features is *silently ignored* rather than
explicitly rejected.

### 2.2 Components-only spec rejected

`SchemaAnalyzer::extract_schemas` (`analysis.rs:641-655`) hard-errors with
`InvalidSchema("No schemas found in OpenAPI spec")` if `components.schemas` is
absent. Per 3.1+ §"OpenAPI Object" (3.2.0:93, 3.1.2:323-334), a valid
document only needs *one of* `paths` / `webhooks` / `components`, and
`components` need not contain `schemas`. The generator therefore refuses
documents that are valid OpenAPI but happen to ship reusable parameters or
responses without any schema bucket.

### 2.3 `$ref` resolver is `#/components/schemas/...`-only

`SchemaAnalyzer::extract_schema_name` (`analysis.rs:974-995`) accepts the
canonical `#/components/schemas/{name}` shape and otherwise returns the last
URL path segment (numeric segments excluded). That is then looked up in
`self.schemas` (the `components.schemas` map) at `analysis.rs:1006-1010`. So:

- `$ref: "#/components/parameters/Foo"` → returns "Foo" → `schemas.get("Foo")`
  → `UnresolvedReference`. (Component parameters *are* loaded into a separate
  map at `analysis.rs:540, 549-554`, but I see no path that resolves a `$ref`
  to that map; the parameter map is only consumed when an operation already
  carries an inline `Parameter` whose schema is `$ref` — and the schema-side
  ref still goes through the schemas map.)
- `$ref: "Pet.yaml"` (3.2:2953-2957 example) → `parts.last() = "Pet.yaml"` →
  `schemas.get("Pet.yaml")` → `UnresolvedReference`.
- `$ref: "definitions.yaml#/Pet"` (3.2:2959-2963 example) → returns "Pet" →
  may *accidentally* resolve to a same-named schema in the current document,
  which is a silent-correctness hazard.
- `$ref: "https://example.com/api/shared/foo#/components/requestBodies/Foo"`
  (3.2 Appendix F example, 3.2.0:5285-5290) → returns "Foo" → likely
  `UnresolvedReference`.

There is no document loader, no base-URI tracking (3.2 `$self`, 3.2.0:98), and
no support for the resolution rules in Appendix F (3.2.0:5235+) or Appendix G
(3.2.0:5450+).

`merge_schema_extensions` (`analysis.rs:308-329`) is *not* a substitute: it is
an opt-in JSON merge over caller-supplied extra files (`new_with_extensions`
at `analysis.rs:566-572`), with custom `x-replacements` semantics
(`analysis.rs:344-367`) — useful for the GPU CLI internal workflow, but
unrelated to OpenAPI's specified ref-resolution model.

### 2.4 Reference Object `summary` / `description` overrides (3.1+)

The 3.1.2 / 3.2.0 Reference Object (3.2.0:2927-2945) explicitly adds
`summary` and `description` siblings that "by default SHOULD override" the
referenced component's. The generator's `Schema::Reference` variant
(`openapi.rs:37-42`) collects only `reference` and an `extra: BTreeMap`. Worse,
non-schema reference uses (Parameter Object, Response Object, etc.) are not
modelled as `Reference | Object` sum types at all — `Parameter`
(`openapi.rs:461-471`), `RequestBody` (`openapi.rs:473-481`), and `Response`
(`openapi.rs:564-571`) are plain object structs. A `$ref` shows up as an
unknown key in `extra` and the rest of the fields parse as defaults.

So the generator (a) silently swallows the override `summary`/`description`,
and (b) treats a `$ref`'d Parameter/RequestBody/Response as an empty object,
not as a reference.

### 2.5 Path Item `$ref`

`PathItem` (`openapi.rs:390-411`) has no `$ref` field. A pathItem-shaped value
of `{ "$ref": "#/components/pathItems/Common" }` parses with every operation
field `None`, and `path_item.operations()` (`openapi.rs:415-442`) returns
`[]`. The path is silently dropped from `analyze_operations`
(`analysis.rs:3499`). Same problem applies if `webhooks` or
`components.callbacks` were ever wired up — all of those are
`Path Item | Reference` per 3.2.0:412 and the spec.

### 2.6 `$dynamicRef` / `$dynamicAnchor`

`grep -rn -E "dynamicRef|dynamicAnchor" /Users/jameslal/workspace/jl/openapi-generator/src/`
returns no matches. The spec defines these at 3.2.0:3265-3266 (and 3.1.2:2844-
2845) as the JSON Schema 2020-12 successors to the deprecated
`$recursiveRef`/`$recursiveAnchor` that the codebase *does* support
(`openapi.rs:43-49, 108-109`; `analysis.rs:1056-1072, 3388-3405`). Specs
authored against the modern 2020-12 dialect (which is what 3.1.x mandates) use
`$dynamicRef`. The generator will fall through `Schema::Untyped { details }`
and the `$dynamicRef` keyword will sit in `SchemaDetails.extra`
(`openapi.rs:140-141`), unused.

### 2.7 Webhooks completely ignored

Webhooks were the headline feature of OpenAPI 3.1 (3.2.0:103). The repo has
none of the wiring:

- `OpenApiSpec` lacks the `webhooks` field (`openapi.rs:7-14`).
- `analyze_operations` (`analysis.rs:3493-3519`) only walks `spec.paths`.
- The registry types in `registry_generator.rs` have no notion of webhook
  vs. request-initiated direction.
- `client_generator.rs` only emits client-side request methods, not handlers.

This repo's own test corpus contains real specs that declare `webhooks:` at
top level — `specs/lithic.yaml`, `specs/imagekit.yaml`, `specs/openai.yaml`
(verified by `grep -l '^webhooks:' specs/*.yaml`). For all three, the webhook
section parses into `OpenApiSpec.extra` and is then dropped on the floor.

### 2.8 Callbacks completely ignored

`Operation` (`openapi.rs:446-458`) does not declare `callbacks`; nor does
`Components`. Callback objects (3.2.0:2222 ff.) are nested
`{ runtime-expression: PathItem }` maps. None of the runtime-expression
machinery (3.2 `$request.*` / `$response.*`) is implemented, and the schemas
inside callback path items are not even fed to the schema analyzer.

### 2.9 Links completely ignored

`Response` (`openapi.rs:565-571`) does not carry `links`. Neither
`operationRef` nor `operationId` resolution is implemented, and the
generator's flat-method client design (`client_generator.rs:35-46`) provides
no surface where a "follow this link" helper could live.

### 2.10 Tags ignored, no tag-based modularization

`tags` is missing both at the root and on `Operation` (`openapi.rs:446-458`).
`OperationInfo` (`analysis.rs:100-122`) and `OperationDef`
(`registry_generator.rs:120-138`) have no tags field. `client_generator.rs`
emits all operations as inherent methods on a single `HttpClient`. There is
no per-tag module split, no `Tag Object` description propagated to rustdoc,
and no `externalDocs` link emitted.

For real specs (any of the GitHub/Stripe/OpenAI-class APIs in `specs/`), this
puts hundreds of `pub async fn` methods on one struct.

### 2.11 `$self` (3.2 only) — base URI

`$self` is the centerpiece of the 3.2 reference-resolution model
(3.2.0:98, 134, 5235-5290). Not modelled, not consulted, no base-URI
tracking. Documents that rely on `$self` for cross-document resolution
(3.2.0:5285-5290) will silently misresolve.

### 2.12 `jsonSchemaDialect`

3.1+ field (3.2.0:100, 3.1.2:327). Not parsed, not respected. Means the
generator cannot tell whether a schema uses the OAS dialect, vanilla JSON
Schema 2020-12, or some custom dialect. In practice the analyzer is
hard-coded to the OAS-flavored 2020-12-ish subset it already knows.

### 2.13 README claim vs. reality

`README.md:8, 14, 17` advertises:

- "OpenAPI 3.1 specifications"
- "OpenAPI 3.1 support — objects, arrays, enums, `oneOf`, `anyOf`, `allOf`,
  discriminated unions"
- "Smart `$ref` resolution — handles circular references and deep nesting"

These claims are accurate for the *schema* slice (which is what the README
bullet limits itself to). They are **not** accurate for the rest of the 3.1
surface area covered by this audit: webhooks, callbacks, links, the new
component buckets (`pathItems`, etc.), Reference Object overrides, or
`$dynamicRef`. None of those are wired through.

---

## 3. 3.2-specific deltas in this audit area

Verbatim items new in 3.2.0 vs. 3.1.2 that touch components / refs / webhooks
/ callbacks / links / tags:

1. **Top-level `$self`** (3.2.0:98) — entirely new field; the spec's
   reference-resolution Appendix F (3.2.0:5235-5290) is built around it.
   *Generator status:* not modelled; no base-URI logic.

2. **Constraint that "at least one of `components`, `paths`, or `webhooks`
   MUST be present"** (3.2.0:93). This is not a new requirement (3.1 had it
   too), but in 3.2 it is stated in the OpenAPI Object section directly.
   *Generator status:* violated — `analysis.rs:646-648` requires
   `components.schemas` and rejects otherwise-valid documents.

3. **New components bucket: `mediaTypes`** (3.2.0:414):
   `Map[string, Media Type Object | Reference Object]`. Not present in 3.1.2.
   *Generator status:* not in `Components` (`openapi.rs:25-31`); ignored.

4. **Tag Object new fields** (3.2.0:2897-2902):
   - `summary` (new in 3.2)
   - `parent` (new in 3.2 — enables tag nesting; "circular references between
     parent and child tags MUST NOT be used")
   - `kind` (new in 3.2 — common values: `nav`, `badge`, `audience`)
   3.1.2's Tag Object (3.1.2:2688-2690) only has `name`, `description`,
   `externalDocs`.
   *Generator status:* the entire Tag Object is ignored regardless of
   version, so all five fields are equally absent.

5. **Components/Tag name resolution recommendation** (3.2.0:176): "For
   resolving Components Object and Tag Object names from a referenced
   (non-entry) document, it is RECOMMENDED that tools resolve from the entry
   document, rather than the current document." Multi-document semantics.
   *Generator status:* moot — no multi-document loader.

6. **Operation-resolution recommendation** (3.2.0:177): "For resolving an
   Operation Object based on an `operationId`, it is RECOMMENDED to consider
   all Operation Objects from all parsed documents."
   *Generator status:* moot — no multi-document loader; and Link Object's
   `operationId` resolution is unimplemented anyway.

7. **Reference targets list** (3.2.0:134): "Reference targets are defined by
   fields including the OpenAPI Object's `$self` field and the Schema
   Object's `$id`, `$anchor`, and `$dynamicAnchor` keywords." Confirms the
   2020-12 dialect.
   *Generator status:* `$id`, `$anchor`, `$dynamicAnchor` not modelled.

8. **Appendix F: Examples of Base URI Determination and Reference
   Resolution** (3.2.0:5235+) and **Appendix G: Parsing and Resolution
   Guidance** (3.2.0:5450+). New normative-ish guidance covering relative
   `$self` / `$id` resolution against the retrieval URI fallback chain.
   *Generator status:* none of this resolution chain is implemented.

9. **Reference Object additional-property restriction** (3.2.0:2943): "This
   object cannot be extended with additional properties, and any properties
   added SHALL be ignored. Note that this restriction on additional
   properties is a difference between Reference Objects and Schema Objects
   that contain a `$ref` keyword." Stronger language than 3.1.
   *Generator status:* Schema-side `$ref` siblings already drop into `extra`
   and are ignored, which happens to be correct behavior for Reference
   Objects but wrong for Schema Objects — the inverse of what the spec
   prescribes (Schema Objects with `$ref` *do* allow siblings since 3.1, see
   3.2.0:2945).

---

## 4. Top gaps ranked by likely real-world impact

1. **Webhooks dropped on the floor (P0).**
   3.1's flagship feature. The repo's *own* sample corpus
   (`specs/openai.yaml`, `specs/lithic.yaml`, `specs/imagekit.yaml`) declares
   webhooks; for those APIs, the generator silently emits zero typed
   handlers, deserializers, or registry entries for incoming events.
   Evidence: `openapi.rs:7-14` (no `webhooks` field), `analysis.rs:3493-3519`
   (only iterates `paths`).

2. **No tag-based modularization (P0 for any non-trivial API).**
   Every operation lands as an inherent method on a single `HttpClient`
   struct (`client_generator.rs:35-46`). For Stripe/GitHub/etc. that's
   hundreds of methods on one type with no namespacing. `tags` aren't even
   parsed. Evidence: zero hits for `tags` in
   `client_generator.rs / generator.rs / registry_generator.rs`.

3. **Components-only / parameters-only / responses-only specs error out
   (P1).** `analysis.rs:646-648` rejects any spec that doesn't ship
   `components.schemas`, even though the spec only requires one of
   `paths`/`webhooks`/`components` and a components-only document is a
   valid shared-library OAD pattern (3.2.0:93).

4. **`pathItems` component bucket unsupported (P1).** Combined with the
   missing `PathItem.$ref`, the entire 3.1+ "reusable Path Item" pattern
   silently no-ops. Evidence: `Components` (`openapi.rs:25-31`) and
   `PathItem` (`openapi.rs:390-411`) both lack the relevant fields.

5. **`$dynamicRef` / `$dynamicAnchor` unsupported (P1 for modern specs).**
   The 2020-12 dialect that 3.1+ mandates uses these in place of the
   deprecated `$recursiveRef`/`$recursiveAnchor` that the generator *does*
   support. Real schemas using generic-array patterns (3.2.0:3559-3575) will
   silently misresolve. Evidence: zero hits for `dynamicRef` in `src/`.

6. **External-file `$ref` and base-URI / `$self` resolution unsupported
   (P1).** `extract_schema_name` (`analysis.rs:974-995`) is a path-segment
   heuristic, not a URI resolver. The 3.2 `$self` mechanism (3.2.0:98,
   5235-5290) is not modelled. Documents that split components across files
   — common for large APIs — will collapse to `UnresolvedReference` errors
   or, worse, silently match an unrelated same-named schema in the current
   document.

7. **Reference Object `summary`/`description` overrides ignored (P2).**
   3.1+ feature for richer rustdoc generation. The Schema-Reference variant
   only carries `reference` (`openapi.rs:37-42`), and Parameter / RequestBody
   / Response are not modelled as `Reference | Object` sum types at all
   (`openapi.rs:461-481, 564-571`).

8. **Callbacks and Links unimplemented (P2-P3).** Less critical for a
   client-side codegen, but documented as part of the 3.1 surface and
   relevant for SDK callers that want typed link traversal or for callback
   payload deserialization.

9. **3.2 deltas (P3 for now, P1 once 3.2 spreads).** New components bucket
   `mediaTypes` (3.2.0:414), Tag Object new fields `summary` / `parent` /
   `kind` (3.2.0:2898-2902), and `$self` base-URI semantics (3.2.0:98) are
   all missing — but as of today most public specs are still 3.0 / 3.1.

10. **`jsonSchemaDialect` ignored (P3).** Means the analyzer cannot adapt to
    custom dialects, but in practice everyone uses the OAS dialect or
    vanilla 2020-12 anyway.

---

## Notes for follow-up audits

- The *schema* slice (`Schema`, `SchemaDetails`, discriminator handling) is
  out of scope here but is the most polished part of the codebase
  (`openapi.rs:33-338`, `analysis.rs:1037+`).
- `OperationInfo` (`analysis.rs:100-122`) is the single most useful place to
  add `tags: Vec<String>` and a `webhook: bool` flag if/when those features
  get wired up — both `client_generator.rs` and `registry_generator.rs` read
  from this struct, so propagation would be local.
- The README's 3.1 claim (README.md:8, 14) is broadly defensible only if
  scoped to "schema-side 3.1 features"; for webhooks / pathItems / Reference
  overrides / dynamic refs / `$self`, it overstates support.
