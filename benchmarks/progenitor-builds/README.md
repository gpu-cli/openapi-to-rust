# Progenitor build-workflow benchmark

This benchmark compares the normal ahead-of-time `openapi-to-rust` workflow
with Progenitor's default-enabled `generate_api!` procedural-macro workflow.
It is a workflow benchmark, not a claim that the two tools generate equivalent
clients.

The harness creates the same synthetic OpenAPI 3.0.3 document for both tools,
pins Progenitor to 0.14.0 and the direct consuming dependencies used by the
checked-in result, downloads dependencies before timing, and measures:

- a clean `cargo check` with fresh Cargo build and target directories;
- an unchanged, no-op `cargo check`;
- `cargo check` after touching ordinary Rust source; and
- the complete spec-change loop after changing a schema description: regenerate
  then check for `openapi-to-rust`, and check (including macro expansion) for
  Progenitor.

## Run

```bash
cargo build --release --bin openapi-to-rust
node benchmarks/progenitor-builds/run.mjs \
  --samples 5 \
  --operations 120 \
  --output /tmp/progenitor-build-results.json
```

The first run needs network access to resolve and fetch the two dependency
graphs. Timed Cargo commands run offline. The harness uses Cargo's
`build.build-dir` setting, so reproduce the checked-in result with Cargo 1.97
or newer.

## Interpretation limits

- Clean-build measurements include each generated client's current dependency
  graph (`reqwest` 0.13 for both generators). They do not
  isolate procedural-macro overhead.
- Filesystem and Cargo registry caches are warm, but every clean sample uses
  fresh compiled-artifact directories.
- The fixture is synthetic and intentionally limited to features both tools
  accept. It measures scaling with operations and models, not compatibility
  with difficult real-world schemas.
- No-op builds should be effectively equal because Cargo does no generation
  work when the consuming crate is fresh.
- Results should always be published with tool versions, hardware, raw samples,
  and a verification date. Do not generalize one machine's result into a
  universal build-time guarantee.

The dated result used for the initial comparison is in
[`results/2026-07-28-m4-max.json`](results/2026-07-28-m4-max.json).
