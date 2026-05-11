//! End-to-end smoke test for both server-codegen examples.
//!
//! Steps per example:
//!   1. Build the workspace `openapi-to-rust` binary (once).
//!   2. Run it against the example's TOML config to populate
//!      `examples/<name>/src/gen/`.
//!   3. Run `cargo build` against the example crate and assert exit 0.
//!
//! These tests are gated behind `--ignored` by default because they
//! take ~30s each (transitive `cargo build` of generated types). Run
//! with `cargo test --test server_examples_test -- --ignored` or in
//! CI with the `examples` job.

use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");
    PathBuf::from(manifest)
}

fn workspace_binary() -> PathBuf {
    // Use the same target/debug dir cargo built into. CARGO_TARGET_DIR
    // overrides this if set, but the default suffices for local + CI.
    repo_root()
        .join("target")
        .join("debug")
        .join("openapi-to-rust")
}

fn ensure_binary_built() {
    let bin = workspace_binary();
    if bin.exists() {
        return;
    }
    let status = Command::new(env!("CARGO"))
        .current_dir(repo_root())
        .args(["build", "--bin", "openapi-to-rust"])
        .status()
        .expect("failed to spawn cargo build");
    assert!(status.success(), "cargo build of openapi-to-rust failed");
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
    ensure_binary_built();
    let dir = repo_root().join("examples").join("server-openai-responses");
    regenerate(&dir);
    cargo_test(&dir);
}

#[test]
#[ignore]
fn anthropic_messages_example_builds() {
    ensure_binary_built();
    let dir = repo_root()
        .join("examples")
        .join("server-anthropic-messages");
    regenerate(&dir);
    cargo_test(&dir);
}
