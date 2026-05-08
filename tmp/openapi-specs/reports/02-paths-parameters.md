# Audit: Paths, Operations, and Parameter Object (incl. serialization)

Slice scope: OpenAPI 3.1.2 / 3.2.0 -> Rust HTTP-client generator.
Repo: `/Users/jameslal/workspace/jl/openapi-generator`.
All source citations are absolute paths in this repo. **Read-only**: no source was modified.

---

## 1. Summary table

Legend: **S** = Supported, **P** = Partial, **U** = Unsupported (recognized but no codegen), **I** = Ignored (not parsed; silently dropped via `#[serde(flatten)] extra`).

### Path Item / Paths

| Feature | Status | Evidence |
|---|---|---|
| Paths Object root map | S | `src/openapi.rs:10` (`pub paths: Option<BTreeMap<String, PathItem>>`) |
| Path templating `{var}` | P | `src/client_generator.rs:1014-1067` (single-pass `format!`) |
| Multiple `{var}` in one path | P | `src/client_generator.rs:1038-1055` (string-replace per param) |
| Path-level `parameters` | S | `src/openapi.rs:408`; merged at `src/analysis.rs:3650-3667` |
| Path-level `servers` | I | absorbed into `extra` (`src/openapi.rs:409-410`); never read |
| Path-level `summary` / `description` | I | absorbed into `extra`; never used in codegen |
| Path-level `$ref` | I | absorbed into `extra`; not resolved |
| HTTP `get` | S | `src/openapi.rs:392-393`, `src/client_generator.rs:705-712` |
| HTTP `put` | S | same |
| HTTP `post` | S | same |
| HTTP `delete` | S | same |
| HTTP `patch` | S | same |
| HTTP `options` | P | parsed (`src/openapi.rs:401`); maps to `get` in client at `src/client_generator.rs:711` (default fallback) |
| HTTP `head` | P | parsed; maps to `get` (same fallback) |
| HTTP `trace` | P | parsed; maps to `get` (same fallback) |
| **HTTP `query` (3.2 NEW)** | U | not parsed in `PathItem` (`src/openapi.rs:391-411`) -> dropped into `extra`; `path_item.operations()` won't yield it (`src/openapi.rs:415-442`) |
| **`additionalOperations` (3.2 NEW)** | U | not parsed; dropped into `extra` |

### Operation Object

| Feature | Status | Evidence |
|---|---|---|
| `operationId` | S | `src/openapi.rs:448-449`, `src/analysis.rs:3501-3504` |
| Synthetic operationId fallback when missing | S | `src/analysis.rs:3523-3558` (method+PascalCase path) |
| `operationId` uniqueness (across files / case) | P | `analysis.operations` is keyed by id (`src/analysis.rs:3514`); duplicates silently overwrite. No conflict detection or warning. Snake-case collisions (`getFoo` vs `get_foo`) collide downstream at `src/client_generator.rs:687-700`. |
| `summary` | S | `src/openapi.rs:450`, surfaced to `OperationInfo.summary` (`src/analysis.rs:3574`); used in registry / not in client doc-comment. |
| `description` | S | same; not surfaced in client doc-comment (see `generate_operation_doc_comment`, `src/client_generator.rs:676-684`) |
| `tags` | I | absorbed into `Operation.extra` (`src/openapi.rs:456-457`); no tag-grouped client modules |
| `deprecated` | I | absorbed into `extra`; no `#[deprecated]` attribute emitted |
| `externalDocs` | I | absorbed into `extra` |
| `requestBody` | S | `src/openapi.rs:453-454`, `src/analysis.rs:3583-3610` |
| `responses` | S | `src/openapi.rs:455`, `src/analysis.rs:3612-3638` |
| `callbacks` | I | absorbed into `extra`; no codegen |
| `security` | I | absorbed into `extra`; client has a single global API-key/Bearer (`src/client_generator.rs:597-599`) |
| `servers` (op-level override) | I | absorbed into `extra` |

### Parameter Object — common fields

| Field | Status | Evidence |
|---|---|---|
| `name` | S | `src/openapi.rs:463`, `src/analysis.rs:3763` |
| `in: path` | S | `src/analysis.rs:3764`, `src/client_generator.rs:721-729`, `src/client_generator.rs:1031-1055` |
| `in: query` | S | `src/client_generator.rs:613-672`, `src/client_generator.rs:731-745` |
| `in: header` | U | recognized location string (`src/registry_generator.rs:191`) but no per-op header serialization is generated; only `custom_headers` map at runtime (`src/client_generator.rs:601-604`) |
| `in: cookie` | U | location string passes through analysis (`src/analysis.rs:3764`); registry maps unknown -> `Query` (`src/registry_generator.rs:192`); no codegen, never set on `req` |
| `required` | S | `src/openapi.rs:466`, `src/analysis.rs:3765`, drives Option<T> wrap at `src/client_generator.rs:738-744` |
| `deprecated` (param) | I | absorbed into `Parameter.extra` (`src/openapi.rs:469-470`); no attribute |
| `allowEmptyValue` | I | absorbed into `extra`; never inspected |
| `example` / `examples` | I | absorbed into `extra` |
| `description` | S | `src/openapi.rs:468`, surfaced to `ParameterInfo.description`, only ends up in registry, not in client doc-comments. |
| `schema` | S | `src/openapi.rs:467`, `src/analysis.rs:3771-3797` (primitive types + string enum/const) |
| `content` (content-based parameter) | U | absorbed into `extra` (`src/openapi.rs:469-470`); generator only inspects `param.schema`. A `content`-based param is silently typed `String` (default at `src/analysis.rs:3767`). |
| `$ref` to `#/components/parameters/...` | S | `src/analysis.rs:3736-3748` (resolves via `extra["$ref"]`) |

### Parameter Object — schema-mode RFC6570 controls

| Field | Status | Evidence |
|---|---|---|
| `style` | I | absorbed into `Parameter.extra` (`src/openapi.rs:469-470`); never read anywhere in `src/`. Defaults silently assumed (effectively `simple`/`form` only). |
| `explode` | I | absorbed into `extra`; never read |
| `allowReserved` | I | absorbed into `extra`; never read |

### 3.2-only fields

| Field | Status | Evidence |
|---|---|---|
| `in: querystring` (3.2 NEW) | U | passes through `analyze_parameter` as a string but no recognition; registry default-maps to `Query` (`src/registry_generator.rs:192`); semantically wrong. |
| `style: cookie` (3.2 NEW) | I | absorbed into `extra` |

---

## 2. Detailed findings

### 2.1 Path Item parsing — fixed-field set is closed and stale vs 3.2

`src/openapi.rs:391-411` declares `PathItem` with explicit fixed methods `get/put/post/delete/options/head/patch/trace` and `parameters`, plus `#[serde(flatten)] extra: BTreeMap<String, Value>`. Everything else — `summary`, `description`, `$ref`, `servers`, the **3.2-new `query`**, and the **3.2-new `additionalOperations`** map — falls into `extra`.

`PathItem::operations()` (`src/openapi.rs:415-442`) iterates only the eight typed fixed-fields. Therefore:

- A `query` operation defined in a 3.2 spec is **invisible to the generator** — no client method, no registry entry, no error/warning.
- Any custom HTTP method declared via `additionalOperations` (e.g. `COPY`, `LINK`, `LOCK` from the 3.2 example, line 646 of the spec doc) is silently dropped.
- A path-item-level `$ref` is silently dropped — the path item is treated as empty.

### 2.2 HTTP method downgrade for `options`/`head`/`trace`

`src/client_generator.rs:704-715`:

```rust
let method = match op.method.to_uppercase().as_str() {
    "GET" => "get",
    "POST" => "post",
    "PUT" => "put",
    "DELETE" => "delete",
    "PATCH" => "patch",
    _ => "get", // Default fallback
};
```

Although `PathItem` parses `options`/`head`/`trace`, when reaching the client emitter they all fall into the `_ => "get"` branch — a silent semantic corruption. `OperationInfo.method` is uppercased at `src/analysis.rs:3572`, so `OPTIONS`, `HEAD`, `TRACE` arrive intact but get rewritten to GET in the emitted client. The registry generator (`src/registry_generator.rs:163-170`) hits the same fallback, defaulting unknown methods to `HttpMethod::Get`.

### 2.3 Path templating

`src/client_generator.rs:1026-1067`:

- Builds a `format!` string by `String::replace("{name}", "{}")` for each declared path parameter.
- Path values pass through `format!`/`Display` only — no percent-encoding, no enforcement of the RFC3986 forbidden-character rule (`/`, `?`, `#`) called out at line 521 of the 3.2 spec ("MUST NOT contain any unescaped 'generic syntax' characters").
- Order of substitution is by iteration order of `op.parameters`, not by lexical order in the path template. Today this happens to be safe because each `{name}` is unique within the path, but if a user re-uses a placeholder (forbidden by spec) the behavior is implementation-defined.
- Multiple variables are correctly handled because `format_args` is a `Vec` aligned with the `format_string`. Path templating with literal text inside a single segment (e.g. `/v{version}/users`) works because the placeholder is matched textually.

`src/openapi.rs:391-411` does **not** validate that template expressions in the path map to declared path-item or operation parameters. A `/users/{id}` with no matching parameter declaration will produce `format_args` empty and a literal `{id}` baked into the generated URL.

### 2.4 Path-item-level parameters — merge correctness

`src/analysis.rs:3640-3667`. Operation-level params are pushed first, then path-item-level params are merged with operation taking precedence on the `(name, location)` key. This matches 3.2 spec lines 681 and 610. The merge is correct.

Caveat: parameters that come from `path_item.extra` (i.e., 3.2 `query` operation) are not iterated — see 2.1.

### 2.5 Operation Object — ignored fields

The `Operation` struct (`src/openapi.rs:447-458`) keeps only `operationId`, `summary`, `description`, `parameters`, `requestBody`, `responses`. Every other Operation Object fixed field listed in spec lines 676-687 of 3.2.0 (and 975-986 of 3.1.2) is dropped into `extra`:

- `tags` — no per-tag client module / namespacing emitted.
- `deprecated` — no `#[deprecated = "..."]` attribute on the generated method.
- `externalDocs` — not surfaced.
- `callbacks` — entire async-callback channel ignored.
- `security` — per-op auth requirement ignored; runtime auth is global Bearer via `self.api_key` (`src/client_generator.rs:596-599`).
- `servers` — operation-level base URL override ignored.

### 2.6 Operation doc-comment is minimal

`src/client_generator.rs:676-684`:

```rust
fn generate_operation_doc_comment(&self, op: &OperationInfo) -> TokenStream {
    let method = op.method.to_uppercase();
    let path = &op.path;
    let doc = format!("{} {}", method, path);
    quote! { #[doc = #doc] }
}
```

`OperationInfo.summary` and `OperationInfo.description` are populated at `src/analysis.rs:3574-3575` but never consumed by the client emitter, only by the registry. Generated rustdoc therefore says "GET /v1/users" with no human description.

### 2.7 Operation ID handling

- Source field: `src/openapi.rs:448-449`.
- Synthetic id when missing: `src/analysis.rs:3523-3558` builds `<method><PascalCasePathSegments>`, brace-stripping path placeholders. Two paths that differ only in placeholder name (which the 3.2 spec at line 559 declares identical) would receive the same synthetic id.
- Final method ident: `src/client_generator.rs:686-701` snake-cases the id; literal `.` characters are first replaced by `_` upstream (`src/analysis.rs:3680`, `3706`).
- **Uniqueness**: not enforced. `analysis.operations.insert(operation_id, op_info)` (`src/analysis.rs:3514`) silently overwrites duplicates.

### 2.8 Parameter `in` value handling

`src/analysis.rs:3756-3808` reads `param.location` as a free-form string. The downstream consumers split on this string:

- `src/client_generator.rs:617` — keeps only `"query"` for query-string emission.
- `src/client_generator.rs:723, 733, 1035` — keeps only `"path"` for path templating and signature.
- `src/registry_generator.rs:188-193` — recognizes `"path"`, `"query"`, `"header"`; everything else (including `"cookie"` and the new 3.2 `"querystring"`) defaults to `Query`.

Headers are accepted into `OperationInfo.parameters` but **never emitted** as per-op `req.header(...)` calls — only the global `custom_headers` map (`src/client_generator.rs:601-604`) sets headers, and that is populated by hand at runtime, not from the spec. A spec like:

```yaml
parameters:
  - name: X-Trace-Id
    in: header
    required: true
    schema: { type: string }
```

produces a parameter slot in `OperationInfo` that is invisible to both `generate_request_param` (filters on `path`/`query` only) and the URL builder. Effectively, **header parameters are silently dropped** from the generated client method signature and from request emission.

`in: cookie` and `in: querystring` are even worse — never recognized in any branch of the client emitter.

### 2.9 Parameter `schema` handling

`src/analysis.rs:3771-3797`:

- Detects `$ref` schema and stores the schema name in `schema_ref` (which is then unused in the client emitter — see 2.10).
- Maps primitives `boolean`/`integer`/`number`/`string` -> `bool`/`i64`/`f64`/`String`.
- Anything else (`array`, `object`, `null`, untyped) silently degrades to `String` (default at `src/analysis.rs:3767`).
- String enums and string `const` are emitted as a synthetic enum type alongside the operation method (issue #10 follow-up).

Concrete consequences:

- An `array` query parameter (very common: `?ids=1&ids=2`) becomes `String`. The user has to pre-serialize the array themselves; `style/explode` in the spec are not honored.
- An `object` query parameter (e.g. `style: deepObject`) becomes `String`.
- `format` hints like `int32`, `int64`, `uuid`, `date-time` are dropped — every integer is `i64`.
- Numeric ranges (`minimum`/`maximum`), `pattern`, `minLength`/`maxLength` on parameters are not enforced.

### 2.10 Parameter `schema_ref` is dead code in the client emitter

`src/analysis.rs:3772-3773` populates `ParameterInfo.schema_ref` from a `$ref` schema, but `src/client_generator.rs:778-791` (`get_param_rust_type`) only consults `param.rust_type`, which for a `$ref` parameter is left at the default `String` (`src/analysis.rs:3767`). So a parameter declared via:

```yaml
- name: status
  in: query
  schema: { $ref: '#/components/schemas/StatusEnum' }
```

is typed as `impl AsRef<str>` in the generated method, **not** as `StatusEnum`. The generated string-enum path-and-query enums (`src/client_generator.rs:407-471`) only fire for **inline** string-enum schemas with declared `enum`/`const`, never for `$ref`-ed enum schemas.

### 2.11 Content-based parameters are unsupported

The 3.2 spec mandates "either a `content` field or a `schema` field, but not both" (line 777). `Parameter` (`src/openapi.rs:462-471`) exposes only `schema`; `content` falls into `extra`. `analyze_parameter` only inspects `param.schema` (`src/analysis.rs:3771`). For a content-mode parameter, `rust_type` stays at the default `String`. There is no JSON-encoding-and-then-percent-encoding path — the user gets a `String` slot with no help. This breaks the 3.2 examples at spec lines 1072-1192 (`coordinates` JSON-in-query, `selector` JSONPath-in-querystring, etc.).

### 2.12 `allowEmptyValue`, `deprecated`, `example`/`examples` on parameters

All four fields are absorbed into `Parameter.extra` (`src/openapi.rs:469-470`) and never inspected. The spec says `allowEmptyValue` is "valid only for `query` parameters" and is deprecated in 3.2 — ignoring is mostly safe. `deprecated` and `examples` ignoring is a documentation-quality gap, not a correctness bug.

### 2.13 Components.parameters resolution

`src/analysis.rs:3736-3748` resolves `$ref` into `#/components/parameters/...` via the `extra["$ref"]` field, not via a dedicated `Reference` variant on `Parameter`. The cache is built once at `src/analysis.rs:549-554`. The lookup is local: external refs (cross-document `$ref: 'common.yaml#/components/parameters/Foo'`) are silently ignored — `strip_prefix("#/components/parameters/")` returns `None`.

Ref + sibling fields: in 3.1/3.2 a `Parameter` `$ref` may have sibling fields. `resolve_parameter` returns the resolved component verbatim and discards any sibling overrides (e.g., a per-call `description` override on the operation parameter). The 3.2 Path Item `$ref` cautionary note at spec line 596 about adjacent properties applies analogously to parameters.

### 2.14 Query-parameter emission

`src/client_generator.rs:613-672` builds a `Vec<(&str, String)>` and calls `req.query(&query_params)`. This is RFC6570 `style: form, explode: true` for primitives (one `name=value` pair). For arrays/objects:

- Arrays are not produced — the Rust type is `String`, so the user passes a single string.
- The `req.query(&query_params)` call uses reqwest's `serde_urlencoded::Serializer`, which percent-encodes `+`, `space`, `%`, etc. according to `application/x-www-form-urlencoded` rules. This matches the `form-style explode=true` default for primitives only.

There is no path for `spaceDelimited`, `pipeDelimited`, `deepObject`, or non-default `explode=false` joining behavior.

### 2.15 Multiple parameters with the same name

`src/openapi.rs:391-411` and `src/analysis.rs:3640-3667` both rely on `(name, location)` uniqueness but don't enforce it (no error, no warning). A spec authoring mistake produces an emitted method with two parameters of the same name, which is a Rust compile error.

---

## 3. Serialization-style support matrix

For each `(style, explode, in)` combination defined at spec lines 914-930 (3.2.0), what does the generator emit?

| `style` | `explode` | `in` | Emitted | Notes |
|---|---|---|---|---|
| matrix | false | path | NOT EMITTED | `style` ignored; path templating uses bare `{}` substitution. |
| matrix | true | path | NOT EMITTED | same |
| label | false | path | NOT EMITTED | same |
| label | true | path | NOT EMITTED | same |
| simple | false | path | OK for primitives | bare value via `format!`; no percent-encoding (gap vs spec line 521) |
| simple | true | path | NOT EMITTED for arrays/objects | rust_type degrades to `String`; user must pre-encode |
| simple | false | header | NOT EMITTED | header parameters silently dropped (see 2.8) |
| simple | true | header | NOT EMITTED | same |
| form | false | query | Wrong for arrays | array would need `name=v1,v2,v3`; emitted as a single user-supplied string |
| form | true | query | Correct for primitives | `req.query(&[(name, val)])` produces `name=val`; for arrays correct *iff user pre-supplies multiple pairs* but the Rust API only takes a single value |
| form | false | cookie | NOT EMITTED | cookie params silently dropped |
| form | true | cookie | NOT EMITTED | same |
| spaceDelimited | false | query | NOT EMITTED | rust_type=String; user pre-encodes |
| pipeDelimited | false | query | NOT EMITTED | same |
| deepObject | _n/a_ | query | NOT EMITTED | same |
| cookie (3.2 NEW) | false | cookie | NOT EMITTED | location not recognized |
| cookie (3.2 NEW) | true | cookie | NOT EMITTED | same |

Net: only the **default form-style query for primitives** and **simple-style path for primitives (without percent-encoding)** are functionally correct. Everything else is either silently broken or silently discarded.

`allowReserved` is never read.
`allowEmptyValue` is never read.

---

## 4. 3.2-specific deltas in this slice (verbatim from 3.2.0)

These items appear in OpenAPI 3.2.0 but not in 3.1.2 within the Paths/Operations/Parameter scope:

1. **Path Item Object: `query` operation** (3.2.0 line 607)
   > query | A definition of a QUERY operation, as defined in the most recent IETF draft (draft-ietf-httpbis-safe-method-w-body-08 ... ) or its RFC successor, on this path.

   3.1.2 (lines 870-877) lists only get/put/post/delete/options/head/patch/trace. **Generator status: U (silently dropped).**

2. **Path Item Object: `additionalOperations` map** (3.2.0 line 608)
   > additionalOperations | Map[string, Operation Object] | A map of additional operations on this path. The map key is the HTTP method ... This map MUST NOT contain any entry for the methods that can be defined by other fixed fields with Operation Object values.

   Not present in 3.1.2. **Generator status: U (silently dropped).**

3. **Parameter Object: new `in` value `"querystring"`** (3.2.0 line 770)
   > querystring - A parameter that treats the entire URL query string as a value which MUST be specified using the `content` field, most often with media type `application/x-www-form-urlencoded` ... MUST NOT appear more than once, and MUST NOT appear in the same operation (or in the operation's path-item) as any in: "query" parameters.

   3.1.2 (lines 1132-1137) lists only path/query/header/cookie. **Generator status: U (registry maps to `Query`; client emits nothing because schema-typed analysis still fires String).**

4. **Parameter Object: new `style` value `"cookie"`** (3.2.0 line 847)
   > cookie | primitive, array, object | cookie | Analogous to form, but following [RFC6265] Cookie syntax rules ... no percent-encoding or other escaping is applied; data values that require any sort of escaping MUST be provided in escaped form.

   3.1.2 (lines 1197-1205) does not include `cookie` style. **Generator status: I (`style` itself is never read).**

5. **Parameter Object: new default for `style: cookie` and `style: form` w/ cookie** (3.2.0 line 818, 817)
   > When `style` is `"form"` or `"cookie"`, the default value [for explode] is `true`. ...
   > for `"cookie"` - `"form"` (for compatibility reasons; note that `style: "cookie"` SHOULD be used with `in: "cookie"`; see Appendix D for details).

   3.1.2 (line 1175-1176) defaults cookie to `style: form`. **Generator status: I.**

6. **Parameter Object: `name` field special cases** (3.2.0 line 788)
   > If `in` is `"querystring"`, or for certain combinations of `style` and `explode`, the value of `name` is not used in the parameter serialization.

   3.1.2 has no equivalent clause. **Generator status: I.**

7. **Parameter Object: clarified handling for header/cookie schema-mode** (3.2.0 lines 807-813)
   > When serializing these values, URI percent-encoding MUST NOT be applied. When parsing these parameters, any apparent percent-encoding MUST NOT be decoded. If using an RFC6570 implementation that automatically performs encoding or decoding steps, the steps MUST be undone before use.

   Stronger and more explicit than 3.1.2 lines 1168-1171. **Generator status: I (no header/cookie codegen at all).**

8. **`example`/`examples` mutual exclusion** (3.2.0 line 784)
   > The `example` and `examples` fields are mutually exclusive; see Working with Examples for guidance on validation requirements.

   Implicit-only in 3.1.2 lines 1166. **Generator status: I.**

9. **`allowEmptyValue` formally deprecated** (3.2.0 line 793)
   > Deprecated: Use of this field is NOT RECOMMENDED, and it is likely to be removed in a later revision.

   3.1.2 line 1156 says "NOT RECOMMENDED" but does not mark deprecated. **Generator status: I either way.**

10. **`name` is allowed to use Unicode codepoints outside the RFC6570 variable name set** (3.2.0 line 531, ABNF)
    > template-expression-param-name = 1*( %x00-7A / %x7C / %x7E-10FFFF ) ; every Unicode character except { and }

    Adds an explicit ABNF; 3.1.2 has no formal ABNF for path templates. **Generator status: P.** `format!`-based path templating with `String::replace("{name}", "{}")` will pass non-ASCII names through `format_args.push(quote! { #param_ident })`, but `param_name.to_snake_case()` (`src/client_generator.rs:1045`) and the `Ident` constructor on the result will reject most non-ASCII characters at codegen time with a `proc-macro` panic.

11. **Path Item Object: `summary` and `description`** (3.2.0 lines 597-598; also present in 3.1.2 lines 868-869).
    Not a 3.2 delta, but **generator status: I** in both versions.

---

## 5. Top gaps ranked by likely real-world impact

1. **Header parameters silently dropped** (`src/client_generator.rs:613-674`, 717-775).
   Critical. Pagination cursors, idempotency keys, API-version headers (`X-API-Version`), trace correlation headers, and request-scoped auth tokens are all expressed as `in: header` in real-world specs (Stripe, Anthropic — see `tests/fixtures/anthropic.yml:14,51,62,...`). Today the generator parses them, gives them a `ParameterInfo`, then **never emits anything** for them. Both signature and request-builder steps filter on `path`/`query` only.

2. **`options`/`head`/`trace` operations silently rewritten as GET** (`src/client_generator.rs:704-715`).
   Critical for any spec that defines preflight/health-check semantics distinct from GET. There's no error or warning; the wrong HTTP verb is sent at runtime.

3. **`array` and `object` parameters degrade to `String`** (`src/analysis.rs:3767-3782`).
   High impact. Specs that declare `?ids=1&ids=2&ids=3` (form/explode=true on array) cannot be expressed; the user gets a single `String` slot and must hand-construct query syntax. Same for `deepObject`, `spaceDelimited`, `pipeDelimited`.

4. **`$ref`-typed parameters lose their type** (`src/analysis.rs:3771-3797`, `src/client_generator.rs:778-791`).
   High impact. A widely shared `StatusEnum` referenced from multiple operations becomes `impl AsRef<str>` in every method signature. The `ParameterInfo.schema_ref` field is populated but never consumed.

5. **Cookie parameters silently dropped** (`src/registry_generator.rs:188-193`, `src/client_generator.rs:613-674`).
   Medium impact. Less common than header but still appears in session-based APIs.

6. **3.2 `query` HTTP method invisible** (`src/openapi.rs:391-411`, 415-442).
   Medium-and-rising. As 3.2 specs roll out, any operation defined under `query:` in a Path Item is silently dropped.

7. **3.2 `additionalOperations` map invisible** (same location).
   Medium. Specs that use WebDAV-style verbs or vendor verbs (`COPY`, `LINK`) lose those operations entirely.

8. **No path-templating percent-encoding** (`src/client_generator.rs:1014-1067`).
   Medium correctness bug. A path parameter containing `/`, `?`, `#`, or non-ASCII characters is interpolated verbatim — the spec at 3.2.0 line 521 explicitly forbids unescaped generic-syntax characters and tools are expected to escape on the user's behalf.

9. **`operationId` collisions silently overwrite** (`src/analysis.rs:3514`).
   Medium. With case-folding to snake_case, names like `getUser` and `get_user` collide downstream as well.

10. **Operation `tags` / `deprecated` / per-op `security` / `servers` ignored** (`src/openapi.rs:447-458`).
    Low–medium. Affects ergonomics and correctness for multi-tenant or partially-deprecated APIs but does not usually break basic codegen.

11. **`content`-based parameters silently typed `String`** (`src/openapi.rs:462-471`, `src/analysis.rs:3771`).
    Low–medium. Rare pattern in mainstream specs but strictly required for the 3.2 querystring examples (lines 1072-1192).

12. **Operation `summary`/`description` not surfaced into rustdoc** (`src/client_generator.rs:676-684`).
    Low. Documentation-quality gap; data is already in `OperationInfo`.

13. **`style`/`explode`/`allowReserved` never parsed** (`src/openapi.rs:462-471`).
    Strategic. Until these are surfaced into the `Parameter` struct, none of items 3, 5, 6 above can be properly addressed. This is the foundational gap for spec-faithful serialization.

---

## Appendix: file:line index for the auditors

- `Parameter` struct: `src/openapi.rs:461-471`
- `Operation` struct: `src/openapi.rs:446-458`
- `PathItem` struct: `src/openapi.rs:391-411`
- `PathItem::operations()`: `src/openapi.rs:415-442`
- `ParameterInfo` (analysis): `src/analysis.rs:147-167`
- `OperationInfo` (analysis): `src/analysis.rs:100-122`
- `analyze_operations`: `src/analysis.rs:3493-3519`
- `generate_operation_id` (synthetic): `src/analysis.rs:3523-3558`
- `analyze_single_operation`: `src/analysis.rs:3561-3670`
- `resolve_parameter` (component-ref): `src/analysis.rs:3736-3748`
- `analyze_parameter`: `src/analysis.rs:3756-3808`
- `generate_query_params`: `src/client_generator.rs:613-673`
- `generate_request_param` (signature): `src/client_generator.rs:718-775`
- `get_param_rust_type`: `src/client_generator.rs:778-791`
- `get_http_method` (verb downgrade): `src/client_generator.rs:704-715`
- `generate_url_construction` / `generate_url_with_params`: `src/client_generator.rs:1014-1068`
- `generate_operation_doc_comment`: `src/client_generator.rs:676-684`
- Per-op enum emission (string-enum/const inline): `src/client_generator.rs:407-471`
- `ParamLocation` (registry, drops cookie/querystring): `src/registry_generator.rs:66-72`, mapping at `src/registry_generator.rs:188-193`
