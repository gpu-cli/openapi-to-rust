# Schema Object & JSON Schema — Audit (slice 01)

Audit target: `/Users/jameslal/workspace/jl/openapi-generator` (Rust generator that
claims OpenAPI 3.1 support).

Method: read-only inspection of `src/openapi.rs` (Schema model), `src/analysis.rs`
(schema analyzer), `src/generator.rs` (Rust emitter), `src/patterns.rs`
(union/discriminator detection). Cross-checked against
`tmp/openapi-specs/openapi-3.1.2.md` and `tmp/openapi-specs/openapi-3.2.0.md`.

Status legend:
- **Supported** — feature is parsed and used end-to-end in code generation.
- **Partial** — parsed and partly honored; some sub-cases dropped, mis-typed, or
  silently coerced.
- **Unsupported** — not parsed; deserialization either fails or strips the
  keyword.
- **Ignored** — parsed/tolerated (lands in `extra: BTreeMap<String, Value>`),
  but the analyzer/generator never reads it.


## 1. Summary table

### Type & nullability

| Feature | Status | Evidence |
| --- | --- | --- |
| `type` as string | Supported | `openapi.rs:90-100` `SchemaType` enum (`#[serde(rename_all = "lowercase")]` for `string`/`integer`/`number`/`boolean`/`array`/`object`/`null`); used in `analysis.rs:1074-1116`. |
| `type` as **array** (3.1's `["string","null"]`) | **Unsupported** | `openapi.rs:69-74` declares `schema_type: SchemaType` (single value, not `Vec<SchemaType>`). With `#[serde(untagged)]` (`openapi.rs:34`) the variant fails to match and the schema falls into `Schema::Untyped` with the `type` value swallowed into `extra`. No code path inspects `extra["type"]` for arrays. |
| 3.0 `nullable` (should NOT be honored in 3.1) | **Honored anyway** (back-compat) | `openapi.rs:105` `pub nullable: Option<bool>`; used in `SchemaDetails::is_nullable` (`openapi.rs:342-344`) and in `analysis.rs:1044, 1307, 1919`, controlling `Option<T>` wrapping in `generator.rs:1343-1353` and serde `skip_serializing_if` at `generator.rs:1376-1378`. The 3.1 spec dropped `nullable`; this generator silently keeps it. |
| `null` in `type: ["string","null"]` form | **Unsupported** (see above) — only the `oneOf/anyOf` `{type: "null"}` shape is recognized. | `openapi.rs:98` accepts `SchemaType::Null` (rename `"null"`). `Schema::is_nullable_pattern` (`openapi.rs:292-302`) only triggers when one of two `oneOf`/`anyOf` variants has `type: "null"`. |
| `null` as one variant of `oneOf`/`anyOf` | Supported | `openapi.rs:292-302, 304-317`; collapsed in `analysis.rs:1947-1962` (oneOf two-element nullable) and `analysis.rs:3119-3146` (anyOf strips null variants). |
| `format` keyword — `int32`/`int64` | Supported | `analysis.rs:3091-3098` maps to `i32`/`i64`. |
| `format` keyword — `float`/`double` | Supported | `analysis.rs:3100-3106` maps to `f32`/`f64`. |
| `format` — `date`, `date-time`, `byte`, `binary`, `uuid`, `email`, `uri`, `hostname`, `ipv4`, `ipv6`, `password`, `decimal`, etc. | **Ignored** | `details.format` is consulted only at `analysis.rs:3093, 3102` for the four numeric formats. No string format produces `chrono::DateTime`, `uuid::Uuid`, `Vec<u8>`, `url::Url`, or anything other than `String` (string fallback in `analysis.rs:1080-1082`). |

### Composition

| Feature | Status | Evidence |
| --- | --- | --- |
| `allOf` | Partial | `openapi.rs:75-81` `Schema::AllOf`; merged in `analysis.rs:1760-1894`. Object properties merge correctly; single-`$ref` `allOf` is collapsed to a type alias (`analysis.rs:1767-1776`). However the merger only honors `properties`/`required`/`description` — non-object constraints in an `allOf` branch (e.g. extra `enum`, `pattern`, `minimum`, `not`) are dropped. There is no validation that branches are mutually compatible. |
| `oneOf` | Supported | `openapi.rs:50-57` `Schema::OneOf`; analyzed in `analysis.rs:1938-2272`. With a discriminator → `SchemaType::DiscriminatedUnion` (tagged enum, `generator.rs:1019-1149` ish). Without → analyzed via `analyze_untagged_oneof_union` (`analysis.rs:2273+`). |
| `anyOf` | Partial | `openapi.rs:58-67` `Schema::AnyOf`; analyzed in `analysis.rs:3112-3389`. Three patterns are recognized: nullable (collapsed), discriminated (treated like `oneOf`), and "all const string" → `StringEnum`/`ExtensibleEnum` (`analysis.rs:3345-3387`). Anything else falls back to an untagged Rust enum, which is semantically `oneOf` (exclusive), not `anyOf` (any-or-many) — accepting multiple matches at once is not modeled. |
| `discriminator` (`propertyName` + `mapping`) | Supported | `openapi.rs:151-158` `Discriminator`; explicit mapping consumed at `analysis.rs:2025-2046`. Implicit single-value-`type` field discrimination is auto-detected (`analysis.rs:803-856`, `patterns.rs:52-159`). Tagged-enum emission at `generator.rs:1019-1149`. |
| `discriminator.defaultMapping` (3.2 only) | **Unsupported** | `openapi.rs:151-158` declares only `property_name` and `mapping`. `defaultMapping` lands in `Discriminator.extra` and is never read. The emitted tagged enum has no fallback variant for missing/unknown discriminator values, so payloads matching the 3.2 "optional discriminating property" pattern will fail to deserialize. |
| `allOf` + `discriminator` (parent-with-discriminator polymorphism) | **Unsupported** | `Schema::AllOf` (`openapi.rs:75-81`) has no `discriminator` field. `analyze_allof_composition` (`analysis.rs:1760-1894`) makes no attempt to detect a discriminator on a parent referenced in `allOf`. The "Pet → Cat/Dog/Lizard via `allOf`" pattern from the 3.1.2/3.2.0 spec example (3.1.2 line 3422+, 3.2.0 line 3747+) generates flat structs with no enum dispatch. |
| `not` | **Unsupported** | No `Schema::Not` variant in `openapi.rs`. No code grep hit for `\"not\"` outside of comments. The keyword is silently dropped into `Untyped` schemas' `extra` map. |

### Validation keywords

| Feature | Status | Evidence |
| --- | --- | --- |
| `enum` | Supported (string only) | `openapi.rs:113` `enum_values: Option<Vec<Value>>`; `string_enum_values` extracts only string entries (`openapi.rs:360-371`). Numeric/boolean enums silently degrade to `i64`/`bool`. |
| `const` | Partial | `openapi.rs:117` `const_value`. String `const` is treated as a 1-variant enum (`openapi.rs:351-378`, behavior introduced in commit `f75aa5a` for issue #10). Non-string `const` (numeric, boolean, object) is parsed but never emitted as a Rust constraint. |
| `multipleOf` | **Unsupported** | Not in `SchemaDetails`; no grep hit. Lands in `extra`, never consumed. |
| `minimum` / `maximum` | **Ignored** | Parsed at `openapi.rs:129-130`, never read by analysis or generator. |
| `exclusiveMinimum` / `exclusiveMaximum` | **Unsupported** | Not in `SchemaDetails`. The 3.1 numeric form (vs 3.0 boolean) is moot — neither shape is honored. |
| `minLength` / `maxLength` | **Ignored** | `openapi.rs:133-136`, never read. |
| `pattern` | **Ignored** | `openapi.rs:137`, never read. No `regex` crate import in generated output. |
| `contentEncoding` | **Unsupported** | Not in `SchemaDetails`. The 3.1 binary-data pattern (`type: string` + `contentMediaType` + `contentEncoding: base64`) cannot be detected; field becomes `String`. |
| `contentMediaType` | **Unsupported** | Same — not modeled, no `Vec<u8>` mapping. |
| `contentSchema` | **Unsupported** | Not modeled. |
| `items` | Supported | `openapi.rs:126`; analyzed in `analyze_array_schema` referenced from `analysis.rs:1098-1101, 2926+`. |
| `prefixItems` (3.1+ tuples) | **Unsupported** | Not in `SchemaDetails`. No grep hit anywhere except as a documentation comment in the 3.2 spec text. Tuple types degenerate to `Vec<serde_json::Value>`. |
| `minItems` / `maxItems` | **Unsupported** | Not in `SchemaDetails`. |
| `uniqueItems` | **Unsupported** | Not modeled. (No `HashSet` variant in `SchemaType`.) |
| `contains` / `minContains` / `maxContains` | **Unsupported** | Not modeled. |
| `unevaluatedItems` | **Unsupported** | Not modeled. |
| `properties` | Supported | `openapi.rs:120`. |
| `required` | Supported | `openapi.rs:121`; controls `Option<T>` wrapping at `generator.rs:1343-1353`. |
| `additionalProperties: bool` | Supported | `openapi.rs:144-149` `AdditionalProperties::Boolean`; emits a `#[serde(flatten)] additional_properties: BTreeMap<String, serde_json::Value>` field at `generator.rs:975-980`. |
| `additionalProperties: <schema>` | Partial (typed map collapsed) | `openapi.rs:148` `AdditionalProperties::Schema(Box<Schema>)` is parsed, but `analysis.rs:1328-1332` explicitly treats it as `true` with a `TODO` comment ("Could analyze the schema to determine the value type"). The schema is never inspected — typed dictionaries become `BTreeMap<String, serde_json::Value>` rather than `BTreeMap<String, T>`. |
| `patternProperties` | **Ignored** | Only consulted in `analysis.rs:3461` to suppress the "empty object → `serde_json::Value`" heuristic. The actual pattern map is never modeled. |
| `propertyNames` | **Ignored** | Same as above (`analysis.rs:3463`). |
| `minProperties` / `maxProperties` | **Ignored** | `analysis.rs:3465-3466` consulted only as a "this object is structured" hint; values never used. |
| `dependentSchemas` / `dependentRequired` / `dependencies` | **Ignored** | `analysis.rs:3468` consulted (only the legacy `dependencies` key); `dependentSchemas`/`dependentRequired` not even sniffed. |
| `unevaluatedProperties` | **Unsupported** | Not modeled. |

### References & dialect

| Feature | Status | Evidence |
| --- | --- | --- |
| `$ref` | Supported (intra-document only) | `openapi.rs:36-42` `Schema::Reference`. Resolved via `extract_schema_name` (used pervasively). External-document refs (URI form) and JSON-pointer refs not pointing into `#/components/schemas/...` will fail at `GeneratorError::UnresolvedReference` (`analysis.rs:1051`). |
| `$recursiveRef` / `$recursiveAnchor` (older 3.1 draft) | Partial | `openapi.rs:43-49` `Schema::RecursiveRef`, `openapi.rs:108-109` `recursive_anchor`. `analysis.rs:1056-1072` and `find_recursive_anchor_schema` (`analysis.rs:3388-3402`) handle the `"#"` self-ref case only — other recursive-ref shapes fall through to a stringly-typed lookup. |
| `$dynamicRef` / `$dynamicAnchor` (current 3.1, 2020-12) | **Unsupported** | No `Schema::DynamicRef` variant. No grep hit anywhere in `src/`. The keywords are silently swallowed into `extra`. The 3.1.2 spec explicitly recommends these for "Generic (Template) Data Structures" (line 2840-2847); the 3.2 spec strengthens `MAY` to `SHOULD` at line 3263. |
| `$id` | **Ignored** | Not in `SchemaDetails`. Lands in `extra`. Base-URI scoping for nested schemas does not happen — the analyzer assumes flat `#/components/schemas/...` namespacing. |
| `$schema` | **Ignored** | Not modeled. The OAS dialect is assumed implicitly. |
| `$defs` | **Unsupported** | Not modeled. Schemas defined under `$defs` (anywhere except `#/components/schemas`) are unreachable — they're not enumerated in `extract_schemas` (`analysis.rs:641-655`). |
| `jsonSchemaDialect` (root field) | **Ignored** | Not in `OpenApiSpec` (`openapi.rs:6-14` only declares `openapi`/`info`/`paths`/`components`/`extra`). Field lands in `extra` and is never read. |

### Annotations

| Feature | Status | Evidence |
| --- | --- | --- |
| `default` | Partial | Parsed at `openapi.rs:115`. Used in `generator.rs:856-872` to pick a default enum variant, and at `generator.rs:1383-1388` to add `#[serde(default)]` on required fields whose type implements `Default`. Numeric/object/array defaults are never emitted as actual Rust default initializers — the value is only consulted for those two cases. |
| `examples` (3.1+ array) | **Ignored** | Not in `SchemaDetails`. No grep hit. |
| `example` (deprecated singular) | **Ignored** | Not in `SchemaDetails`. |
| `title` | **Ignored** | Not in `SchemaDetails`. Has no effect on doc comments. |
| `description` | Supported | `openapi.rs:104`. Emitted as `#[doc = ...]` at `generator.rs:705-709, 746-751, 888-892, 958-963, 983-987`. CommonMark sanitization at `generator.rs:1614+`. |
| `deprecated` | **Unsupported** | Not in `SchemaDetails`; no `#[deprecated]` emission anywhere. |
| `readOnly` | **Unsupported** | Not modeled. The 3.1 `readOnly`-on-required-fields semantics (response-only field) cannot be expressed; client structs require the field on both ingest and emit. |
| `writeOnly` | **Unsupported** | Not modeled. Same impact in reverse. |


## 2. Detailed findings

### 2.1 Type system shape (the big one)

`SchemaType` (`openapi.rs:89-100`) is a single-valued enum and the `Schema::Typed`
variant carries one `SchemaType`, not a `Vec<SchemaType>`. The 3.1 spec lifts
JSON Schema's union-type form (`type: ["string", "null"]`, `type: ["integer", "string"]`),
which is the canonical 3.1 nullability idiom. With `#[serde(untagged)]` on
`Schema` (`openapi.rs:34`) and no custom deserializer, a JSON value of
`{"type": ["string","null"], …}` does **not** match `Schema::Typed` (the
`SchemaType` deserializer expects a string). It falls through to
`Schema::Untyped`, where `type` lands in `extra` and is never inspected.

Net effect: every property typed as `["X","null"]` collapses to
`serde_json::Value` — losing both the type information **and** the nullability
hint. The analyzer's nullable-pattern detection (`is_nullable_pattern` in
`openapi.rs:292-302`) only fires on the `oneOf`/`anyOf` form.

The 3.0-era `nullable: bool` keyword is still parsed (`openapi.rs:105`) and
honored end-to-end. This is back-compat with 3.0, but on a strict 3.1 reading
it is incorrect and may mask spec-author mistakes.

### 2.2 Discriminator paths

Three modes are supported:

1. Explicit `oneOf`/`anyOf` + `discriminator: {propertyName, mapping}` →
   `SchemaType::DiscriminatedUnion` → tagged-enum emission. Variant lookup
   at `analysis.rs:2025-2057` walks the explicit mapping first, then falls
   back to extracting a `const`/single-`enum` value from the variant schema's
   discriminator property (`extract_discriminator_value_for_field`,
   `analysis.rs:932-962`).
2. Inline `oneOf`/`anyOf` variants whose discriminator lives in `extra` keyed
   `"const"` or `"x-stainless-const"` (`analysis.rs:951-957`,
   `analysis.rs:2531-2540`). The Stainless extension is hardcoded.
3. Implicit auto-detection: variants without an explicit `discriminator` are
   scanned for any common property whose value is constant across all variants
   (`detect_discriminator_field`, `analysis.rs:803-856`). Triggers only when
   `variants.len() > 2` (`analysis.rs:780`), which silently degrades 2-variant
   discriminated unions to untagged enums.

Gaps:
- **3.2 `defaultMapping`** is silently dropped — `Discriminator` (`openapi.rs:151-158`)
  has no field for it, only `mapping`.
- **3.2 "optional discriminating property"** (the case `defaultMapping` exists for)
  has no support: the emitted enum has no fallback variant.
- **`allOf`-based polymorphism** (Pet → Cat/Dog/Lizard via children referencing
  parent in `allOf`, see 3.1.2 spec line 3422 and 3.2.0 spec line 3747) is not
  detected. `analyze_allof_composition` (`analysis.rs:1760-1894`) only merges
  properties; it never inspects whether a referenced parent declares a
  `discriminator` and never enumerates "siblings via reverse `allOf`".

### 2.3 Validation keywords are deserialized but not enforced

`SchemaDetails` (`openapi.rs:102-142`) declares fields for `minimum`, `maximum`,
`min_length`, `max_length`, `pattern`. These are **parsed**, but a full grep of
`src/` shows no read site for `details.minimum`, `details.maximum`,
`details.min_length`, `details.max_length`, or `details.pattern`. The codegen
never inserts validators, never picks bounded integer types (`u8`/`u16`/`u32`)
based on `maximum`, never adds `regex`-backed deserializers for `pattern`. They
are dead fields.

3.1's spec-mandated change of `exclusiveMinimum`/`exclusiveMaximum` from boolean
to number is moot here — neither shape is honored.

### 2.4 `additionalProperties` typed schemas collapse to Value

`AdditionalProperties::Schema(_)` is parsed (`openapi.rs:144-149`) but
`analysis.rs:1328-1332` reduces it to a boolean `true` with a self-flagged
`TODO`:

```rust
Some(crate::openapi::AdditionalProperties::Schema(_)) => {
    // For now, treat schema-based additionalProperties as true
    // TODO: Could analyze the schema to determine the value type
    true
}
```

Result: `additionalProperties: { $ref: "#/components/schemas/Foo" }` → emitted
as `BTreeMap<String, serde_json::Value>`, never `BTreeMap<String, Foo>`.

### 2.5 References — limited surface

`extract_schema_name` (only used for `#/components/schemas/<name>` form)
and `find_json_content` etc. only resolve to that prefix. Out-of-scope:
- `$defs` blocks anywhere in the document (the 2020-12 standard location).
- Refs into `#/components/parameters`, `#/components/responses`, etc. (some
  patches at `analysis.rs:3736-3748` resolve `#/components/parameters/...` but
  only for parameter resolution, not for schemas).
- External-document refs (`$ref: "https://other.example/schema.json#/Foo"`).
- Refs anchored by `$id`/`$anchor`. `$id` lands in `extra` and is never used to
  rewrite resolution context — meaning a single-document spec authored with
  proper JSON-Schema base-URI scoping would mis-resolve.

### 2.6 `$dynamicRef` / `$dynamicAnchor` completely missing

The repo handles **only** the older draft's `$recursiveRef` / `$recursiveAnchor`
(`openapi.rs:43-49, 108-109`). Both 3.1.x and 3.2.0 specs explicitly call out
`$dynamicRef` as the supported template/generic mechanism (3.2 line 3263-3268,
3.1.2 line 2840-2847). A spec using `$dynamicRef` will silently lose those
references — they fall into `Schema::Untyped`'s `extra` map and the schema
becomes a `serde_json::Value` blob.


## 3. 3.2-specific schema deltas

Spec text comparison (`openapi-3.1.2.md` §3322 vs `openapi-3.2.0.md` §3601):

1. **`Discriminator.defaultMapping` (new field)** — 3.2 line 3617:
   > `defaultMapping` | `string` | The schema name or URI reference to a schema
   > that is expected to validate the structure of the model when the
   > discriminating property is not present in the payload or contains a value
   > for which there is no explicit or implicit mapping.

   Not present in 3.1.2's table at line 3330-3337. **Repo: not modeled.**

2. **Optional discriminating property** — 3.2 line 3615 changed `propertyName`'s
   description from 3.1.2's "This property SHOULD be required in the payload
   schema, as the behavior when the property is absent is undefined" to
   "**MAY be defined as required or optional**, but when defined as optional the
   Discriminator Object MUST include a `defaultMapping` field…". 3.2 also adds
   a whole "Optional Discriminating Property" subsection (line 3645-3664) with
   the `not: enum: [...]` example (line 3655-3662) for the `defaultMapping`
   schema. **Repo: not modeled** — there is no support for emitting a fallback
   enum variant, and there is no support for the `not` keyword used in the
   recommended pattern.

3. **`Discriminator.mapping` URI semantics tightened** — 3.2 line 3639:
   > The behavior of a `mapping` value or `defaultMapping` value that is both a
   > valid schema name and a valid relative URI reference is implementation-
   > defined…

   3.1.2 (line 3357) covers only `mapping`. **Repo: explicit-mapping is treated
   as a literal string discriminator value; URI-form mapping values are not
   resolved (`analysis.rs:2025-2046` only treats them as schema-name lookups).**

4. **3.2 schema text moves "Composition and Inheritance" inline-ref** to
   reference both `oneOf`/`anyOf`/`allOf` for the discriminator (line 3252-3254);
   3.1.2 only mentioned `allOf` at line 2832. The behavior expected (parent +
   child via `allOf`) is the same in both versions; semantics did not change.

5. **3.2 Schema Object adds wording about `$dynamicAnchor`/`$dynamicRef`** at
   line 3263-3268 (changed `MAY` to `SHOULD`). The 3.1.2 wording at line
   2840-2847 says `MAY`. Editorial — no new field. **Repo: still missing.**

6. **3.2 reaffirms `nullable` is gone** — neither 3.1.2 nor 3.2.0 list `nullable`
   in the Schema Object Fixed Fields. **Repo: still parses and honors it.**

7. **3.2 adds `Format Registry` reference** (line 3016) for `format` keyword
   extensibility. The base-format list at 3023-3030 is unchanged from 3.1.2:
   `int32`, `int64`, `float`, `double`, `password`. **Repo: handles `int32`,
   `int64`, `float`, `double`; ignores `password`** (string fallback). Identical
   gap in both versions.

8. **3.2 Schema text on parsing** (lines 3033-3165) walks through how
   implementations handle `type: [string, number]` arrays inside `allOf`,
   reinforcing that array-form `type` is mainstream in 3.1+. **Repo: type-as-
   array still unsupported (see §2.1).**

No new Schema Object Fixed Fields appeared between 3.1.2 and 3.2.0 — only the
discriminator delta is a structural change.


## 4. Top gaps ranked by likely real-world impact

1. **`type` as array (`["X","null"]` / `["integer","string"]`)** —
   `openapi.rs:69-74`. This is the **canonical** 3.1 nullability idiom. Any
   OpenAPI 3.1 spec authored "by the book" instead of using the legacy
   `nullable: true` will lose property types and nullability. High-value APIs
   (Stripe, GitHub, FastAPI-emitted) increasingly use this form. **Severity:
   critical.**

2. **`additionalProperties: <schema>` typed dictionaries collapse to
   `BTreeMap<String, serde_json::Value>`** — `analysis.rs:1328-1332` (self-
   flagged TODO). Common pattern in any "tags", "metadata", "customFields" map.
   **Severity: high.**

3. **`allOf` + parent-side `discriminator` (3.1/3.2 polymorphism via
   inheritance)** — `analysis.rs:1760-1894` and `Schema::AllOf`
   (`openapi.rs:75-81`) ignore parent discriminators. The Pet/Cat/Dog/Lizard
   example from both specs (3.1.2:3422+, 3.2.0:3747+) cannot be modeled.
   **Severity: high** for legacy 3.0/3.1 specs that retained this pattern.

4. **String `format` keywords beyond numeric (date-time, uuid, byte, binary,
   email, uri, password)** — `analysis.rs:3091-3110`. Every datetime field,
   every UUID, every binary blob is a `String`. Forces hand-written conversion
   on every consumer. **Severity: high** ergonomically (wrong types compile
   but pollute every call site).

5. **`discriminator.defaultMapping` (3.2 new) + optional discriminating
   property** — `openapi.rs:151-158` only models `propertyName` and `mapping`.
   The 3.2 ergonomic upgrade ("extend an existing oneOf without breaking old
   clients") is impossible to express. **Severity: medium** today, rising as
   3.2 adoption grows.

6. **`$dynamicRef` / `$dynamicAnchor`** — completely absent. Generic/template
   schemas (paged-list-of-Foo, Result<T,E>) cannot be modeled. **Severity:
   medium-high** for libraries that lean on this.

7. **Validation keywords entirely ignored** (`pattern`, `minimum`, `maximum`,
   `minLength`, `maxLength`, `minItems`, `maxItems`, `uniqueItems`,
   `multipleOf`, `exclusiveMinimum`/`Maximum`, `contains`, `minContains`,
   `maxContains`, `unevaluatedItems/Properties`, `dependentRequired`,
   `dependentSchemas`, `propertyNames`, `patternProperties`, `prefixItems`).
   Even the deserialized ones (`minimum`, `maximum`, `minLength`, `maxLength`,
   `pattern`) are dead weight. Generated structs accept arbitrary input.
   **Severity: medium** — many users tolerate this, but it's a correctness
   foot-gun for security-sensitive APIs.

8. **`readOnly` / `writeOnly`** — not modeled. Every API with separate
   read/write representations (a common Stripe pattern) over-requires fields
   on the client side. **Severity: medium.**

9. **`prefixItems` (tuple types)** — not modeled. Any spec describing a
   coordinate pair or fixed-length tuple gets `Vec<serde_json::Value>`.
   **Severity: low-medium.**

10. **`not` keyword** — not modeled. Required to express the 3.2-recommended
    `defaultMapping` schema (line 3655-3662). **Severity: low** in isolation,
    but blocks adoption of 3.2's optional-discriminator pattern.

11. **`examples` / `example` / `title` / `deprecated`** — not modeled. Doc-
    quality regression but does not affect correctness. **Severity: low.**

12. **3.0 `nullable` still honored on 3.1+ specs** — `openapi.rs:105`,
    `analysis.rs:1044`. Defensive, but masks spec-author bugs and conflates
    the two versions. **Severity: low** (technical debt, not blocking).

13. **`$defs`** — schemas defined anywhere outside `#/components/schemas` are
    invisible to `extract_schemas` (`analysis.rs:641-655`). **Severity: low**
    in OAS-flavored specs (most authors stick to `components/schemas`).
