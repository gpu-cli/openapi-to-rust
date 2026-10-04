//! Live check against Cloudflare: upload a Worker made of two modules through
//! the client generated from the pinned `specs/cloudflare.yaml`.
//!
//! The upload is a `metadata` JSON part and one `files` part per module, each
//! a `MultipartFile` with its own filename and content type. `worker.js`
//! imports `./lib.js`, so Cloudflare only accepts the upload if it finds the
//! second module by its filename. The control renames that file and must be
//! rejected, which shows the filename is what matches it.
//!
//! It needs an account and a token with Workers Scripts: Edit, so it's ignored
//! by default:
//!
//! ```sh
//! CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... \
//!   cargo test --test cloudflare_workers_upload_live_test -- --ignored
//! ```
//!
//! It deletes the Workers it creates, and prints neither variable.

use openapi_to_rust::config::ClientSection;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use std::process::Command;

#[test]
#[ignore = "uploads to a real Cloudflare account; needs CLOUDFLARE_API_TOKEN and CLOUDFLARE_ACCOUNT_ID"]
fn generated_client_uploads_a_two_module_worker() {
    for variable in ["CLOUDFLARE_API_TOKEN", "CLOUDFLARE_ACCOUNT_ID"] {
        assert!(std::env::var_os(variable).is_some(), "{variable} isn't set");
    }
    let spec = std::fs::read_to_string("specs/cloudflare.yaml").unwrap();
    let spec: serde_json::Value = serde_yaml::from_str(&spec).unwrap();
    let temp = tempfile::TempDir::new().unwrap();
    let mut analyzer = SchemaAnalyzer::new(spec).unwrap();
    let mut analysis = analyzer.analyze().unwrap();
    let generator = CodeGenerator::new(GeneratorConfig {
        output_dir: temp.path().join("src/cloudflare"),
        enable_async_client: true,
        tracing_enabled: false,
        client: Some(ClientSection {
            operations: vec![
                "worker-script-upload-worker-module".into(),
                "worker-script-delete-worker".into(),
            ],
            prune_models: true,
        }),
        ..Default::default()
    });
    let result = generator.generate_all(&mut analysis).unwrap();
    generator.write_files(&result).unwrap();

    let dependencies =
        std::fs::read_to_string(temp.path().join("src/cloudflare/REQUIRED_DEPS.toml")).unwrap();
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "cloudflare-workers-upload"
version = "0.0.0"
edition = "2024"
publish = false
{dependencies}
[dev-dependencies]
tokio = {{version="1", features=["macros","rt-multi-thread"]}}
"#
        ),
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), RUNTIME).unwrap();

    let output = Command::new("cargo")
        .args(["test", "--quiet", "--", "--nocapture"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/cloudflare-workers-upload"),
        )
        .output()
        .unwrap();
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert!(
        output.status.success(),
        "live upload failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const RUNTIME: &str = r###"pub mod cloudflare;
#[cfg(test)]
mod tests {
    use super::cloudflare::{client::{ApiOpError, HttpClient}, types::*};

    const WORKER: &str = "import { greet } from \"./lib.js\";\nexport default { fetch() { return new Response(greet(\"a second module\")); } };\n";
    const LIB: &str = "export function greet(what) { return `hello from ${what}`; }\n";

    fn script(lib_file_name: &str) -> WorkersMultipartScript {
        let module = |source: &'static str, file_name: &str| {
            MultipartFile::from(source)
                .with_file_name(file_name)
                .with_content_type("application/javascript+module")
        };
        WorkersMultipartScript {
            files: Some(vec![module(WORKER, "worker.js"), module(LIB, lib_file_name)]),
            metadata: WorkersMultipartScriptMetadata {
                main_module: Some("worker.js".into()),
                compatibility_date: Some("2026-09-01".into()),
                ..Default::default()
            },
        }
    }

    #[tokio::test]
    async fn upload_two_modules() {
        let account = std::env::var("CLOUDFLARE_ACCOUNT_ID").unwrap();
        let client = HttpClient::new()
            .with_base_url("https://api.cloudflare.com/client/v4")
            .with_api_key(std::env::var("CLOUDFLARE_API_TOKEN").unwrap());
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let name = format!("otr-multipart-files-{suffix}");
        let control = format!("{name}-control");

        let uploaded = client.worker_script_upload_worker_module(&account, &name, script("lib.js")).await;
        let renamed = client.worker_script_upload_worker_module(&account, &control, script("other.js")).await;
        for script in [&name, &control] {
            let deleted = client.worker_script_delete_worker(&account, script, Some(true)).await;
            println!("delete {}: {}", if script == &name { "worker" } else { "control" }, if deleted.is_ok() { "ok" } else { "failed or absent" });
        }

        match uploaded {
            Ok(response) => println!("upload with lib.js: success={}", response.success),
            Err(error) => panic!("upload with lib.js failed: {}", describe(&error)),
        }
        match renamed {
            Ok(_) => panic!("upload with lib.js renamed to other.js succeeded, so modules aren't matched by filename"),
            Err(error) => {
                let error = describe(&error);
                println!("upload with other.js: rejected: {error}");
                assert!(error.contains("No such module"), "rejected for another reason: {error}");
            }
        }
    }

    fn describe<E: std::fmt::Debug>(error: &ApiOpError<E>) -> String {
        match error {
            ApiOpError::Api(error) => format!("HTTP {}: {}", error.status, error.body),
            other => format!("{other:?}"),
        }
    }
}
"###;
