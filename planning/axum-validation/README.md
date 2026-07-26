# Axum 0.8 and Runtime Input Validation

Status: implementation plan

## Goal

Generate production-ready Axum 0.8 servers that validate inbound requests against
the selected OpenAPI operations, return useful and stable RFC 9457 problem details,
and never expose schema-engine errors, rejected values, filesystem paths, or other
internal implementation details.

This plan also resolves GitHub issue #38: generated `{parameter}` routes must be
compiled and exercised against the same Axum major version declared by generated
dependencies.

## Success criteria

- Generated dependencies target the latest Axum 0.8 release line and emitted code
  no longer relies on removed Axum re-exports.
- Every selected operation validates JSON bodies plus path, query, header, and
  cookie parameters according to its OpenAPI contract.
- Media type, malformed payload, body-size, and schema-validation failures map to
  deliberate HTTP statuses and `application/problem+json` bodies.
- Public validation errors contain stable codes and JSON Pointer-style locations,
  but omit rejected values, raw validator messages, schema internals, and source
  paths.
- OpenAPI references needed by validation are bundled at generation time. Generated
  servers never fetch schemas from the network or filesystem at request time.
- Generated clients decode validation problem details as a typed error while
  hand-written clients receive standards-based JSON.
- Tests exercise generated-client-to-generated-server and hand-written-client-to-
  generated-server success and failure round trips, including parameterized routes.
- Documentation and dependency fragments describe the validation behavior,
  configuration, limits, and compatibility break.

## Delivery phases

1. [Axum 0.8 foundation](00-axum-08-upgrade.md)
2. [Validation and public error contract](01-validation-contract.md)
3. [JSON body validation](02-json-body-validation.md)
4. [Parameters and media validation](03-parameter-media-validation.md)
5. [Round-trip compatibility matrix](04-roundtrip-matrix.md)
6. [Release and quality gates](05-release-quality-gates.md)

The detailed component boundaries and trust model are in [architecture.md](architecture.md).

## Compatibility

This is a generated-server API change. The intended release is `openapi-to-rust`
0.9.0, targeting the Axum 0.8 line. Client-only generation remains unaffected
except for the additive typed Problem Details decoding surface.
