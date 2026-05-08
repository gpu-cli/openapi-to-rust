# Audit Report: Servers and Security Schemes

Repo: `/Users/jameslal/workspace/jl/openapi-generator`
Slice: **Servers and Security Schemes**
Specs reviewed: `tmp/openapi-specs/openapi-3.1.2.md`, `tmp/openapi-specs/openapi-3.2.0.md`

---

## 1. Headline Finding (read this first)

**The generator does not parse `servers` or `security` / `securitySchemes` from the OpenAPI document at all.** None of these fields appear on `OpenApiSpec`, `Components`, `PathItem`, or `Operation` in `src/openapi.rs:6-457`. The `OpenApiSpec` struct only declares `openapi`, `info`, `paths`, `components`, and a `#[serde(flatten)] extra` bag (`src/openapi.rs:6-14`). Components only declares `schemas`, `parameters`, `extra` (`src/openapi.rs:25-31`). Operation only declares `operationId`, `summary`, `description`, `parameters`, `requestBody`, `responses`, `extra` (`src/openapi.rs:446-458`). Anything spec-defined for servers or auth lands in `extra` and is never read — `grep -r '"servers"' src/` and `grep -r '"security"' src/` both return empty.

All "auth" support is therefore **manual configuration that the user types into TOML or builder calls** — it is not derived from, or validated against, the OpenAPI document.

---

## 2. Summary Table

| Capability | Spec-level support (parsed from spec) | Runtime support in generated client | Evidence |
|---|---|---|---|
| `servers[]` (root, path, operation) | **Ignored** — no struct field | Partial — single base URL via `with_base_url()` builder | `src/openapi.rs:6-14`; `src/client_generator.rs:347-350` |
| Server `url` templating with `{var}` | **Ignored** | Not handled in base URL (path-param `{}` substitution exists separately) | `src/client_generator.rs:1014-1068` |
| Server Variables (`default`, `enum`, `description`) | **Ignored** | None | n/a — no parser |
| Per-Path / Per-Operation server override | **Ignored** | None — single `self.base_url` for every op | `src/client_generator.rs:1020,1060,1065` |
| `securitySchemes` (Components) | **Ignored** | None auto-generated | `src/openapi.rs:25-31` |
| `security` requirement (root / operation) | **Ignored** | None — every op unconditionally calls `bearer_auth(api_key)` if a key is set | `src/client_generator.rs:596-599` |
| `apiKey` (header / query / cookie) | **Ignored** at parse time | Partial — only via TOML `[http_client.auth] type = "ApiKey"` (mapped to `AuthConfig::ApiKey`) but the variant is never read by the codegen | `src/http_config.rs:38-42`; `src/config.rs:461-463`; **dead code**: `auth_config` field at `src/generator.rs:49` is never consulted by `client_generator.rs` |
| `http` `basic` | **Ignored** | **Not supported** — no `basic_auth` codepath anywhere | `grep -rn basic_auth src/` returns 0 hits |
| `http` `bearer` (with `bearerFormat`) | **Ignored** | Hard-coded — `req = req.bearer_auth(api_key)` is emitted unconditionally for every operation, regardless of spec/config | `src/client_generator.rs:597-599` |
| `http` `digest` | **Ignored** | **Not supported** | n/a |
| `oauth2` (any flow) | **Ignored** | **Not supported** — no token endpoint, no scope plumbing, no flow types | n/a |
| `openIdConnect` (`openIdConnectUrl`) | **Ignored** | **Not supported** | n/a |
| `mutualTLS` (3.1+) | **Ignored** | **Not supported** — no client-cert config on `HttpClient` | `src/client_generator.rs:189-200` (struct fields: `base_url`, `api_key`, `http_client`, `custom_headers`) |
| 3.2 `oauth2MetadataUrl` | **Ignored** | **Not supported** | n/a |
| 3.2 `deviceAuthorization` flow | **Ignored** | **Not supported** | n/a |
| 3.2 `deviceAuthorizationUrl` | **Ignored** | **Not supported** | n/a |
| 3.2 Security Scheme `name` (server) / `deprecated` | **Ignored** | **Not supported** | n/a |
| Security Requirement AND (multiple keys in one obj) | **Ignored** | **Not supported** | n/a |
| Security Requirement OR (array of objects) | **Ignored** | **Not supported** — every op gets the one configured token | n/a |
| `security: []` (anonymous on op) | **Ignored** | Token still attached if configured (cannot opt-out per op) | `src/client_generator.rs:596-599` |
| Empty `{}` requirement (anonymous-allowed) | **Ignored** | Same — token always attached | `src/client_generator.rs:596-599` |
| Scopes propagation (oauth2 / openIdConnect) | **Ignored** | **Not supported** | n/a |
| Custom header / prefix auth | n/a | `AuthConfig::Custom { header_name, header_value_prefix }` exists in the type but **`header_value_prefix` is hard-coded to `None`** when built from TOML and never used by codegen | `src/http_config.rs:43-49`; `src/config.rs:464-467` |

Legend: **Ignored** = field is not deserialized; **Not supported** = no runtime equivalent exists; **Partial** = some manual workaround.

---

## 3. Detailed Findings

### 3.1 `OpenApiSpec` model omits `servers` and `security`

`src/openapi.rs:6-14`:

```rust
pub struct OpenApiSpec {
    pub openapi: String,
    pub info: Info,
    pub paths: Option<BTreeMap<String, PathItem>>,
    pub components: Option<Components>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
```

The OAS 3.1.2 / 3.2.0 root-level `servers: [...]` (3.2.0 spec line 296) and `security: [...]` arrays end up in the `extra` bag and are **never consulted** by the analyzer or the generator. There is no `Server`, `ServerVariable`, `SecurityScheme`, `OAuthFlows`, `OAuthFlow`, or `SecurityRequirement` type defined anywhere in `src/openapi.rs` (full file inspected; `grep -in 'Server\|Security\|OAuth\|AuthScheme' src/openapi.rs` finds zero matches outside of doc comments).

`Components` (`src/openapi.rs:25-31`) similarly declares only `schemas` and `parameters` — `securitySchemes`, `responses`, `headers`, `links`, `callbacks`, `pathItems`, `mediaTypes`, `examples`, `requestBodies` (per 3.2.0 spec line 410) are all dropped.

`PathItem` (`src/openapi.rs:390-411`) and `Operation` (`src/openapi.rs:446-458`) likewise have no `servers` or `security` fields. So per-path / per-operation server selection and per-operation security override (both first-class OAS features) are unrepresented.

### 3.2 Generated client: a single `base_url` String

`src/client_generator.rs:189-200`:

```rust
pub struct HttpClient {
    base_url: String,
    api_key: Option<String>,
    http_client: ClientWithMiddleware,
    custom_headers: BTreeMap<String, String>,
}
```

- `with_base_url(impl Into<String>)` (`src/client_generator.rs:347-350`) is the only knob — caller must hand-pick a URL. Variables, enums, defaults, multiple servers, and per-op overrides all collapse to "user supplies one string". The default is `String::new()` (`src/client_generator.rs:307,331`).
- URL construction at `src/client_generator.rs:1020`, `1060`, `1065` is `format!("{}{}", self.base_url, path)` — there is no template substitution against the `base_url` itself. If a user-supplied `base_url` happens to contain `{var}` placeholders, they are forwarded verbatim. Path-parameter substitution (`src/client_generator.rs:1026-1068`) only operates on the OAS `path` segment.

### 3.3 Generated client: hard-coded bearer auth, regardless of `auth_config`

`src/client_generator.rs:596-599` (inside every generated operation method):

```rust
// Add API key if configured
if let Some(api_key) = &self.api_key {
    req = req.bearer_auth(api_key);
}
```

This is unconditional. `bearer_auth` is the **only** auth verb emitted by the generator — `grep -rn 'bearer_auth\|basic_auth' src/` finds exactly one match, this line.

`AuthConfig` exists in the type system (`src/http_config.rs:32-50`) with `Bearer`, `ApiKey`, `Custom { header_value_prefix }` variants, and `GeneratorConfig.auth_config: Option<AuthConfig>` exists (`src/generator.rs:49`). The TOML loader populates it (`src/config.rs:453-471`). **But nothing in the generator reads it.** Confirmed by `grep -rn 'self.config().auth_config\|config.auth_config\|\.auth_config\b' src/`, which only finds the field declaration, the test-helper `auth_config: None`, and the TOML→config wiring — no read in `client_generator.rs` or `generator.rs` codegen paths. So a user who writes:

```toml
[http_client.auth]
type = "ApiKey"
header_name = "X-API-Key"
```

…still gets an `Authorization: Bearer <token>` header at runtime, **not** `X-API-Key: <token>`. The TOML knob is silently ignored.

Workaround currently required: the user must call `.with_header("X-API-Key", "...")` and not use `with_api_key`.

### 3.4 SSE streaming client: a separate, more flexible auth path

`src/streaming.rs:96-103` defines `enum AuthHeader { Bearer(String), ApiKey(String) }`, and `src/generator.rs:2418-2443` emits the corresponding header. This honors `endpoint.auth_header` (set programmatically on `StreamingEndpoint`) — but it is **also** not derived from the OAS `securitySchemes`, and it is per-endpoint rather than per-operation-from-spec. It also has no Basic/OAuth2/Digest/mTLS support. Default is hard-coded `Authorization: Bearer {api_key}` (`src/generator.rs:2437-2443`).

So the codebase has **two independent, inconsistent auth implementations** (REST client vs SSE client), neither of which sees the spec.

### 3.5 No security validation, scope wiring, or per-operation override

Because `security: [...]` and `securityRequirement` are never parsed:

- AND across schemes (multiple keys in one Security Requirement Object — 3.2.0 spec line 4691) is not modeled.
- OR across alternatives (array of Security Requirement Objects — 3.2.0 spec line 4694) is not modeled — every op receives identical auth handling.
- Empty `{}` requirement signaling anonymous-allowed (3.2.0 spec line 4697) cannot be honored. Token is attached regardless.
- `security: []` to remove security on a specific op (e.g., a public `/healthz` next to authenticated endpoints) is not honored — token is still attached. There is no per-op opt-out generated.
- `oauth2` and `openIdConnect` `scopes` (3.2.0 spec line 4659) cannot be propagated; the generator has no concept of scopes anywhere (`grep -rn 'scope' src/` returns only unrelated hits).

### 3.6 README claim vs reality

`README.md:15`: "**HTTP client generation** — async clients with retry logic, tracing, and **auth middleware**".
`README.md:138`: "**Authentication** — Bearer token, API key, or custom header auth".
`README.md:228`: TOML key `type = "Bearer | ApiKey | Custom"`.

These are **misleading**. The generated client only supports Bearer-on-Authorization. The TOML `auth.type` and `auth.header_name` knobs are validated (`src/config.rs:322-337`) and serialized into `GeneratorConfig.auth_config`, but the client generator never reads them — the emitted code always calls `req.bearer_auth(self.api_key)`. "Custom header auth" is true only insofar as `with_header()` lets the user attach arbitrary static headers; there is no per-request key plumbing for non-Bearer schemes.

---

## 4. 3.2-Specific Deltas (verbatim from `openapi-3.2.0.md`)

The following are present in 3.2.0 sections covered by this audit and absent in 3.1.2. **All are unsupported** (because the entire surface area is unsupported), but they expand the gap.

1. **Server Object — new `name` field** (3.2.0 line 272):
   > "An optional unique string to refer to the host designated by the URL."
   3.1.2 Server Object (line 451-463) has only `url`, `description`, `variables`.

2. **Relative-server example with `$self` document identifier** (3.2.0 lines 277-303). 3.1.2 does not define `$self` semantics for server URL determination.

3. **Security Scheme Object — new field `oauth2MetadataUrl`** (3.2.0 line 4582):
   > "URL to the OAuth2 authorization server metadata RFC8414. TLS is required."
   Applies to `oauth2`. Not present in 3.1.2 (compare 3.1.2 lines 3953-3963).

4. **Security Scheme Object — new field `deprecated`** (3.2.0 line 4583):
   > "Declares this security scheme to be deprecated. Consumers SHOULD refrain from usage of the declared scheme. Default value is `false`."

5. **Security Scheme Object — `oauth2` now spans more flows.** Intro paragraph (3.2.0 line 4567):
   > "OAuth2's common flows (implicit, password, client credentials and authorization code) as defined in RFC6749, **OAuth2 device authorization flow as defined in RFC8628**, and OpenID-Connect-Core."
   3.1.2 (line 3948) omits the device-authorization clause.

6. **OAuth Flows Object — new field `deviceAuthorization`** (3.2.0 line 4643):
   > "Configuration for the OAuth Device Authorization flow."
   Not present in 3.1.2 OAuth Flows Object (3.1.2 line 4055-4068).

7. **OAuth Flow Object — new field `deviceAuthorizationUrl`** (3.2.0 line 4656):
   > "**REQUIRED**. The device authorization URL to be used for this flow… The OAuth2 standard requires the use of TLS."
   Applies to `oauth2` (`"deviceAuthorization"`).

8. **OAuth Flow Object — `tokenUrl` Applies-To list expanded** (3.2.0 line 4657):
   > "`oauth2` (`"password"`, `"clientCredentials"`, `"authorizationCode"`, **`"deviceAuthorization"`**)"
   3.1.2 (line 4079) omits `deviceAuthorization`.

9. **Security Scheme `scheme` reference updated to RFC9110** (3.2.0 line 4578):
   > "the Authorization header as defined in **RFC9110**…"
   3.1.2 (line 3959) referenced RFC7235.

10. **Security Requirement Object — names may be URIs as well as component names** (3.2.0 lines 4685-4689):
    > "The name used for each property MUST either correspond to a security scheme declared in the Security Schemes under the Components Object, **or be the URI of a Security Scheme Object**. Property names that are identical to a component name under the Components Object MUST be treated as a component name. To reference a Security Scheme with a single-segment relative URI reference (e.g. `foo`) that collides with a component name (e.g. `#/components/securitySchemes/foo`), use the `.` path segment (e.g. `./foo`)."
    3.1.2 (line 4129) only allows component names.

11. **Security Considerations — new ambiguity warning** (3.2.0 lines 4775-4779):
    > "It is implementation-defined whether a component name used by a Security Requirement Object in a referenced document is resolved from the entry document (RECOMMENDED) or the referenced document."
    > "A Security Requirement Object that uses a URI to identify a Security Scheme Object can have the URI resolution hijacked by providing a Security Scheme component name identical to the URI…"
    Not present in the 3.1.2 Security Considerations section (3.1.2 line 4227+).

---

## 5. Top Gaps Ranked by Likely Real-World Impact

| Rank | Gap | Why it bites | Files to change |
|---|---|---|---|
| 1 | **No `servers[]` parsing** — generator forces user to hand-type a base URL even when the spec already declares one. Most public APIs (OpenAI, Stripe, Anthropic, etc.) ship a default in `servers[0].url`. | First-time UX is "why doesn't `with_base_url()` have a sensible default?"; users copy-paste URLs that drift from the spec. | `src/openapi.rs:6-14` (add `servers: Option<Vec<Server>>`); use it in `client_generator.rs:299-340` to seed `base_url` default. |
| 2 | **`auth_config` is dead code** — TOML accepts `type = "ApiKey"`/`"Custom"` and validates it, but the generator emits hard-coded `bearer_auth(api_key)`. Silent-misbehavior, not just unsupported. | Users following the README will produce clients that silently send the wrong auth header; debugging this requires reading the generated source. | `src/client_generator.rs:596-599` (branch on `self.config().auth_config`); also document the gap in `README.md:138, 227-229`. |
| 3 | **No `security` per-operation override** — public endpoints next to authenticated ones still receive the bearer header; protected endpoints receive nothing if `with_api_key` was forgotten — there is no compile-time hint. | Causes 401s on protected ops and leaks tokens to public/health endpoints. | Requires `Operation.security` field (`src/openapi.rs:446-458`) plus codegen-time per-op decision in `src/client_generator.rs:582-610`. |
| 4 | **No `securitySchemes` parsing → no scheme-aware generation** — `apiKey in: header/query/cookie`, `http basic`, `http digest`, `oauth2`, `openIdConnect`, `mutualTLS` all collapse into "Bearer or DIY". | Specs like Stripe (basic), Google (OAuth2), AWS (sig v4 / mTLS) cannot be consumed without large hand-written shims. | New `SecurityScheme` enum in `src/openapi.rs`; emit corresponding `with_*` builders + per-op header insertion in `src/client_generator.rs`. |
| 5 | **Server Variables (`default`, `enum`) ignored** — APIs with regional/tenant subdomains (e.g. `https://{region}.api.example.com`) cannot be configured without typing the full string. | Users miss defaults; enum-typed regions become free-form strings with no validation. | `src/openapi.rs` add `ServerVariable`; emit a typed builder (`with_region(Region)` for enum, `with_region(impl Into<String>)` otherwise) plus URL templating in `src/client_generator.rs:1014-1068`. |
| 6 | **OAuth2 / OpenID Connect / mTLS** — entire authn families are absent. | Any spec using these (most enterprise APIs) requires the user to bypass the generated client and use raw `reqwest`. | Net-new module, possibly `src/auth_generator.rs`. |
| 7 | **Two parallel auth paths (REST vs SSE) with different shapes** — `client_generator.rs:596-599` vs `streaming.rs::AuthHeader` + `generator.rs:2418-2443`. | Behavioral drift (e.g., adding `Custom` will need to be done twice; bug fixes won't carry over). | Unify behind a single `AuthApplier` consumed by both generators. |
| 8 | **README/TOML overstate auth support** — `README.md:138, 227-229` advertise `Bearer | ApiKey | Custom` but only Bearer works. | Trust gap; also blocks anyone from filing a "bug" because the TOML *parses fine*. | Either fix gap #2 or update `README.md` and `src/config.rs:322-337` to reject the unsupported variants until they work. |
| 9 | **3.2 device-authorization flow + `oauth2MetadataUrl` + scheme `deprecated`** — entirely new surface area. | Not blocking yet (3.2 adoption is early), but compounds gap #4. | Same plan as #4. |
| 10 | **Security Requirement OR/AND semantics, scopes, and 3.2 URI-based scheme references** | Once #4 lands, these are the next correctness traps (silently picking the wrong alternative; missing required scopes). | Modeled in `Operation` and root, requires resolver. |

---

## 6. File:Line Cheat Sheet

- `src/openapi.rs:6-14` — `OpenApiSpec` (no `servers`, no `security`)
- `src/openapi.rs:25-31` — `Components` (no `securitySchemes`)
- `src/openapi.rs:390-411` — `PathItem` (no `servers`)
- `src/openapi.rs:446-458` — `Operation` (no `servers`, no `security`)
- `src/http_config.rs:32-50` — `AuthConfig` enum (typed but unused by codegen)
- `src/config.rs:194-209` — `HttpClientSection` (only `base_url`, `auth`, `headers`, `retry`, `tracing`)
- `src/config.rs:245-251` — `AuthConfigSection` (`auth_type`, `header_name`)
- `src/config.rs:322-337` — `validate_auth_type` accepts `Bearer | ApiKey | Custom`
- `src/config.rs:452-471` — TOML→`AuthConfig` mapping (populates dead-code `auth_config`)
- `src/config.rs:464-467` — `Custom` always sets `header_value_prefix: None`
- `src/generator.rs:49` — `pub auth_config: Option<AuthConfig>` (declared, never read in codegen)
- `src/client_generator.rs:189-200` — generated `HttpClient` struct (only `base_url`, `api_key`, `custom_headers`)
- `src/client_generator.rs:347-369` — builders: `with_base_url`, `with_api_key`, `with_header`, `with_headers` (no `with_basic_auth`, `with_oauth_token`, etc.)
- `src/client_generator.rs:596-599` — hard-coded `req.bearer_auth(api_key)` per op
- `src/client_generator.rs:1014-1068` — URL construction (no server-variable substitution)
- `src/streaming.rs:96-103` — second auth path: `enum AuthHeader { Bearer, ApiKey }`
- `src/generator.rs:2418-2443` — SSE auth header emission (default Bearer on `Authorization`)
- `README.md:15, 138, 227-229` — claims about auth that exceed actual behavior

No source code was modified.
