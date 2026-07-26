//! End-to-end smoke test for both server-codegen examples.
//!
//! Steps per example:
//!   1. Run the Cargo-provided `openapi-to-rust` test binary against the
//!      example's TOML config to populate
//!      `examples/<name>/src/gen/`.
//!   2. Run `cargo test` against the example crate and assert exit 0.
//!
//! These tests are gated behind `--ignored` by default because they
//! compile generated types and their transitive dependencies. Run with
//! `cargo test --test server_examples_test -- --ignored`. PR CI exercises
//! each example through its official Python SDK compatibility job.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
}

fn workspace_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openapi-to-rust"))
}

fn regenerate(example_dir: &Path) {
    let config = example_dir.join("openapi-to-rust.toml");
    let output = Command::new(workspace_binary())
        .current_dir(example_dir)
        .arg("generate")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("failed to spawn openapi-to-rust generate");
    assert!(
        output.status.success(),
        "generate failed for {}:\nstdout: {}\nstderr: {}",
        example_dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

/// Run the example's own `cargo test`. The example's unit tests
/// invoke the trait method for both `stream: true` and `stream: false`
/// to prove both response variants are constructable at runtime.
/// (Cargo test implies cargo build, so this also catches build errors.)
fn cargo_test(example_dir: &Path) {
    let output = Command::new(env!("CARGO"))
        .current_dir(example_dir)
        .args(["test"])
        .output()
        .expect("failed to spawn cargo test");
    assert!(
        output.status.success(),
        "cargo test failed for {}:\nstdout: {}\nstderr: {}",
        example_dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

#[test]
#[ignore]
fn openai_responses_example_builds() {
    let dir = repo_root().join("examples").join("server-openai-responses");
    regenerate(&dir);
    cargo_test(&dir);
}

#[test]
#[ignore]
fn anthropic_messages_example_builds() {
    let dir = repo_root()
        .join("examples")
        .join("server-anthropic-messages");
    regenerate(&dir);
    cargo_test(&dir);
}
