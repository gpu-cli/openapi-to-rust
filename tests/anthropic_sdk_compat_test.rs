//! Official Anthropic Python SDK compatibility smoke test.
//!
//! This is ignored for normal local `cargo test` because it requires the
//! dependencies in `tests/python/anthropic_sdk_compat/requirements.txt`. CI
//! runs it on every pull request with the SDK pinned to that file's version.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn generator_binary() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_openapi-to-rust"))
}

fn regenerate(example_dir: &Path) {
    let output = Command::new(generator_binary())
        .current_dir(example_dir)
        .args(["generate", "--config"])
        .arg(example_dir.join("openapi-to-rust.toml"))
        .output()
        .expect("failed to run openapi-to-rust generate");
    assert!(
        output.status.success(),
        "Anthropic example generation failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

fn build_and_test_example(example_dir: &Path) {
    for action in ["test", "build"] {
        let output = Command::new(env!("CARGO"))
            .current_dir(example_dir)
            .env("CARGO_TARGET_DIR", example_dir.join("target"))
            .arg(action)
            .output()
            .unwrap_or_else(|error| panic!("failed to run cargo {action}: {error}"));
        assert!(
            output.status.success(),
            "Anthropic server example cargo {action} failed:\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_server(example_dir: &Path) -> (Server, String) {
    let binary = example_dir.join("target").join("debug").join(format!(
        "server-anthropic-messages{}",
        std::env::consts::EXE_SUFFIX
    ));
    let mut child = Command::new(&binary)
        .env("OPENAPI_SERVER_ADDR", "127.0.0.1:0")
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap_or_else(|error| panic!("failed to start {}: {error}", binary.display()));
    let stdout = child.stdout.take().expect("server stdout was not piped");
    let server = Server(child);

    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let line = receiver
        .recv_timeout(Duration::from_secs(30))
        .expect("generated server did not announce its address within 30 seconds")
        .expect("failed to read generated server address");
    let base_url = line
        .trim()
        .strip_prefix("OPENAPI_SERVER_URL=")
        .unwrap_or_else(|| panic!("unexpected generated server output: {line:?}"))
        .to_owned();
    (server, base_url)
}

#[test]
#[ignore = "requires the pinned Python dependencies in tests/python/anthropic_sdk_compat"]
fn official_anthropic_python_sdk_matches_generated_axum_server() {
    let example_dir = repo_root().join("examples/server-anthropic-messages");
    regenerate(&example_dir);
    build_and_test_example(&example_dir);
    let (_server, base_url) = start_server(&example_dir);

    let python = std::env::var_os("ANTHROPIC_COMPAT_PYTHON").unwrap_or_else(|| "python3".into());
    let output = Command::new(python)
        .current_dir(repo_root())
        .arg("tests/python/anthropic_sdk_compat/smoke.py")
        .arg(base_url)
        .output()
        .expect("failed to run official Anthropic Python SDK smoke script");
    assert!(
        output.status.success(),
        "official Anthropic Python SDK compatibility smoke failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    print!("{}", String::from_utf8_lossy(&output.stdout));
}
