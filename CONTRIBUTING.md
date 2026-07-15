# Contributing to openapi-to-rust

Thanks for helping make real-world OpenAPI documents easier to use from Rust.
Bug reproductions with a small schema are especially valuable: generated-code
problems are much faster to review when the wire shape is visible in a fixture.

## Before you start

- Use [GitHub Discussions](https://github.com/gpu-cli/openapi-to-rust/discussions)
  for usage questions and design exploration.
- Use the issue forms for reproducible bugs and concrete feature requests.
- For a larger generated-API or configuration change, open an issue before
  investing in an implementation so compatibility tradeoffs can be discussed.
- Follow the [Code of Conduct](CODE_OF_CONDUCT.md) and report vulnerabilities
  through [SECURITY.md](SECURITY.md), not a public issue.

## Development setup

The project requires Rust 1.88 or newer, Git, and Bash. Clone with submodules so
the vendored JSON Schema conformance corpus is available:

```bash
git clone --recurse-submodules https://github.com/gpu-cli/openapi-to-rust.git
cd openapi-to-rust
cargo test
```

External contributors do not need the maintainers' Beads issue-tracking tool.
Reference the public GitHub issue in your pull request when one exists.

## Repository map

- `src/analysis.rs` converts OpenAPI schemas and operations into generator IR.
- `src/generator.rs` emits models and coordinates generated files.
- `src/client_generator.rs` emits HTTP/SSE client operations.
- `src/server/` emits and manages opt-in Axum scaffolding.
- `src/type_mapping.rs` owns format strategies and dependency requirements.
- `tests/fixtures/` contains focused regression documents.
- `tests/conformance/` contains the compatibility catalog and reports.
- `specs/` contains the real-world corpus used by the compile gate.

## Making a change

1. Add the smallest fixture that reproduces the OpenAPI shape.
2. Add a behavioral assertion, snapshot, or generated scratch-crate compile
   test. Prefer behavior assertions when a full-file snapshot would be noisy.
3. Implement the change without hand-editing checked-in generated examples.
4. Run the checks proportional to the change.
5. Explain generated API or wire-format compatibility in the pull request.

For `insta` snapshots:

```bash
cargo insta test
cargo insta review
```

Review every changed snapshot. Do not accept broad snapshot churn without
explaining why unrelated generated output changed.

## Checks

Run the standard gate before opening a pull request:

```bash
cargo fmt --check
cargo clippy --all-features -- -D warnings
cargo test --all-features
RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --all-features
```

Also run the relevant distribution or corpus gate when touching these areas:

```bash
scripts/install-smoke.sh                 # packaging, CLI, or dependencies
scripts/spec-compile.sh anthropic openai # generator/client output
scripts/spec-compile.sh                  # broad generator/type changes
```

The full corpus generates and compile-checks 54 OpenAPI documents and can take
several minutes. CI runs a fast generation tier on pull requests and the full
compile tier weekly or on manual dispatch.

## Compatibility expectations

Until 1.0, a minor release may correct generated Rust APIs that were incomplete
or wrong on the wire. Even so, changes should be additive where practical.
Call out all of the following in the pull request when applicable:

- generated method or model signature changes;
- serialized query, path, header, or body changes;
- new generated runtime dependencies or features;
- configuration migrations or default changes;
- OpenAPI constructs that remain unsupported.

Keep commits focused and use clear imperative messages. Maintainers may squash
on merge.
