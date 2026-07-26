# Phase 5: Release and Quality Gates

Depends on Phase 4.

## Objective

Ship the compatibility change with exact generated dependencies, trustworthy docs,
and comprehensive repository verification.

## Work

- Update README and server documentation to describe runtime validation rather than
  the former documentation-only constraint behavior.
- Document the public Problem Details schema, status taxonomy, configuration, schema
  dialect behavior, limits, and safe logging guidance.
- Update examples and generated dependency fragments for Axum 0.8, `async-trait`,
  and the feature-minimal `jsonschema` dependency.
- Add a changelog entry and bump the intended generator minor version to 0.9.0.
- Run formatting, linting, complete tests, fixture compilation, and dependency
  auditing available in the repository.
- Reconcile the remaining legacy server Beads whose acceptance criteria are covered
  by the new round-trip and extractor work.

## Acceptance

- Exact dependency-fragment tests compile every generation mode.
- Documentation contains no claim that generated servers omit runtime validation.
- Full repository quality gates pass from a clean checkout.
- All implementation Beads are closed with validation evidence and pushed.

## Validation

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo nextest run --workspace
cargo check --workspace
```
