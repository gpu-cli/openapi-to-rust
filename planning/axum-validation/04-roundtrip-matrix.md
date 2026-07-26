# Phase 4: Round-trip Compatibility Matrix

Depends on Phases 2 and 3.

## Objective

Prove wire compatibility for generated and independent clients across successes and
all public validation failure classes.

## Matrix

| Client | Server | Cases |
| --- | --- | --- |
| Generated Rust client | Generated Axum server | Valid path/query/header/body; typed success; typed Problem Details |
| Hand-written `reqwest` client | Generated Axum server | Valid request; 400, 413, 415, and 422 responses |
| Raw HTTP client | Generated Axum server | Media type, malformed JSON, pointer escaping, and redaction assertions |

## Work

- Build one compact OpenAPI fixture containing a parameterized route, required and
  constrained parameters, constrained JSON, and form content.
- Launch the generated server on an ephemeral loopback port in tests.
- Generate and compile a client from the same specification and exercise it against
  that live server.
- Exercise the same server with hand-written `reqwest` requests that do not share
  generated types.
- Record handler invocation counts to prove rejected requests never cross the trait
  boundary.
- Assert content type, status, stable codes, pointer locations, deterministic error
  ordering, caps, and absence of supplied secret marker values.

## Acceptance

- Every matrix row runs in CI without fixed ports or external network access.
- Generated client success values and typed validation problems round-trip.
- Hand-written clients can consume the standards-based error without knowledge of
  Rust or the validator implementation.

## Validation

```bash
cargo nextest run -p openapi-to-rust -E 'test(/server.*roundtrip|validation.*roundtrip/)'
```
