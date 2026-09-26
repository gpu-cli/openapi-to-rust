use openapi_to_rust::config::{BuildersSection, ClientSection};
use openapi_to_rust::type_mapping::TypeMappingConfig;
use openapi_to_rust::{CodeGenerator, GeneratorConfig, SchemaAnalyzer};
use serde_json::json;
use std::process::Command;

fn spec() -> serde_json::Value {
    let mut document = json!({
        "openapi": "3.1.0", "info": {"title":"transports", "version":"1"},
        "components":{"schemas":{"BinaryFile":{"type":"string","format":"binary"},"Problem":{"type":"object","required":["message"],"properties":{"message":{"type":"string"}}}}},
        "paths": {
            "/upload": {"post": {
                "operationId":"upload", "parameters":[{"name":"multipart_filenames","in":"query","schema":{"type":"string"}}],
                "requestBody":{"required":true,"content":{"multipart/form-data":{"schema":{"type":"object","additionalProperties":false,"required":["front-page","back"],"properties":{
                    "front-page":{"type":"string","format":"binary"},"back":{"type":"string","format":"binary"},"omitted":{"type":"string","format":"binary"},"nullable":{"anyOf":[{"type":"string","format":"binary"},{"type":"null"}]},"referenced":{"$ref":"#/components/schemas/BinaryFile"},"caption":{"type":"string"}
                }}}}},"responses":{"204":{"description":"ok"}}
            }},
            "/real": {"get":{"operationId":"uploadWithMultipartFilenames","responses":{"204":{"description":"ok"}}}},
            "/render": {"get": {
                "operationId":"render", "parameters":[{"name":"fail","in":"query","schema":{"type":"string"}}],
                "responses":{
                    "200":{"description":"representations","content":{
                        "application/json":{"schema":{"type":"object","required":["value"],"properties":{"value":{"type":"string"}}}},
                        "application/vnd.alternate+json":{"schema":{"type":"object","required":["alternate"],"properties":{"alternate":{"type":"integer"}}}},
                        "text/plain":{"schema":{"type":"string"}},
                        "application/octet-stream":{"schema":{"type":"string","format":"binary"}},
                        "text/event-stream":{"schema":{"type":"string"}}
                    }},
                    "400":{"description":"bad","content":{"application/json":{"schema":{"$ref":"#/components/schemas/Problem"}}}}
                }
            }},
            "/status": {"get":{"operationId":"status","responses":{
                "200":{"description":"one","content":{"application/json":{"schema":{"type":"object","required":["one"],"properties":{"one":{"type":"string"}}}}}},
                "201":{"description":"two","content":{"application/json":{"schema":{"type":"object","required":["two"],"properties":{"two":{"type":"integer"}}}}}}
            }}}
        }
    });
    let mut upload = document["paths"]["/upload"]["post"].clone();
    upload["operationId"] = json!("uploadCollision");
    upload["parameters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"multipart_filenames","in":"header","schema":{"type":"string"}}));
    document["paths"]["/collision"] = json!({"post":upload});
    let mut streaming_upload = document["paths"]["/upload"]["post"].clone();
    streaming_upload["operationId"] = json!("uploadStream");
    streaming_upload["responses"] = json!({"200":{"description":"mixed", "content":{
        "application/json":{"schema":{"type":"object","properties":{"accepted":{"type":"boolean"}}}},
        "application/octet-stream":{"schema":{"type":"string","format":"binary"}}
    }}});
    document["paths"]["/upload-stream"] = json!({"post":streaming_upload});
    document["paths"]["/constructor"] =
        json!({"get":{"operationId":"new","responses":{"204":{"description":"ok"}}}});
    document["paths"]["/keyword"] = json!({"get":{"operationId":"type","responses":{"200":{"description":"ok", "content":{"application/octet-stream":{"schema":{"type":"string","format":"binary"}}}}}}});
    document["paths"]["/keyword-real"] =
        json!({"get":{"operationId":"typeBinaryStream","responses":{"204":{"description":"ok"}}}});
    document["paths"]["/identifiers"] = json!({"get":{"operationId":"identifiers","parameters":[
        {"name":"self","in":"query","required":true,"schema":{"type":"string"}},
        {"name":"self_param","in":"query","required":true,"schema":{"type":"string"}}
    ],"responses":{"204":{"description":"ok"}}}});
    document["paths"]["/range"] = json!({"get":{"operationId":"range","responses":{
        "200":{"description":"one","content":{"application/json":{"schema":{"type":"object","required":["one"],"properties":{"one":{"type":"string"}}}}}},
        "2XX":{"description":"two","content":{"application/json":{"schema":{"type":"object","required":["two"],"properties":{"two":{"type":"integer"}}}}}}
    }}});
    document["paths"]["/profile"] = json!({"get":{"operationId":"profile","responses":{"200":{"description":"profiles","content":{
        "application/json; profile=A":{"schema":{"type":"object","required":["a"],"properties":{"a":{"type":"string"}}}},
        "application/json; profile=B":{"schema":{"type":"object","required":["b"],"properties":{"b":{"type":"string"}}}}
    }}}}});
    document["paths"]["/quoted-profile"] = json!({"get":{"operationId":"quotedProfile","parameters":[{"name":"matching","in":"query","schema":{"type":"boolean"}}],"responses":{"200":{"description":"quoted profiles","content":{
        "application/json; profile=\"a\\\";b\"":{"schema":{"type":"object","required":["b"],"properties":{"b":{"type":"string"}}}},
        "application/json; profile=\"a\\\";c\"":{"schema":{"type":"object","required":["c"],"properties":{"c":{"type":"string"}}}}
    }}}}});
    document
}

#[test]
fn generated_variants_negotiate_stream_and_keep_filenames_request_local() {
    let temp = tempfile::TempDir::new().unwrap();
    let mut dependencies = String::new();
    for (module, types, parsed) in [
        ("typed", TypeMappingConfig::default(), true),
        ("strings", TypeMappingConfig::conservative(), false),
    ] {
        let mut analyzer = SchemaAnalyzer::with_type_mapper(
            spec(),
            openapi_to_rust::type_mapping::TypeMapper::new(types.clone()),
        )
        .unwrap();
        let mut analysis = analyzer.analyze().unwrap();
        let generator = CodeGenerator::new(GeneratorConfig {
            output_dir: temp.path().join("src").join(module),
            types,
            enable_async_client: true,
            enable_sse_client: parsed,
            tracing_enabled: false,
            builders: BuildersSection {
                enabled: true,
                threshold: 0,
            },
            client: Some(ClientSection {
                operations: vec![
                    "upload".into(),
                    "uploadWithMultipartFilenames".into(),
                    "render".into(),
                    "status".into(),
                    "uploadCollision".into(),
                    "new".into(),
                    "type".into(),
                    "uploadStream".into(),
                    "typeBinaryStream".into(),
                    "identifiers".into(),
                    "range".into(),
                    "profile".into(),
                    "quotedProfile".into(),
                ],
                prune_models: true,
            }),
            ..Default::default()
        });
        let result = generator.generate_all(&mut analysis).unwrap();
        generator.write_files(&result).unwrap();
        if parsed {
            dependencies =
                std::fs::read_to_string(temp.path().join("src/typed/REQUIRED_DEPS.toml")).unwrap();
        }
        let client = result
            .files
            .iter()
            .find(|f| f.path.to_str() == Some("client.rs"))
            .unwrap();
        assert!(
            client
                .content
                .contains("multipart_filenames_2: &[(&str, &str)]"),
            "{}",
            client.content
        );
        assert!(
            client
                .content
                .contains("pub async fn upload_with_multipart_filenames_2")
        );
        assert!(client.content.contains("pub async fn render_binary_stream"));
        assert!(
            client
                .content
                .contains("multipart_filenames_3: &[(&str, &str)]")
        );
        assert!(client.content.contains("pub async fn new_2"));
        assert!(client.content.contains("pub async fn r#type"));
        assert!(
            client
                .content
                .contains("pub async fn status_json_status_201")
        );
        assert!(!client.content.contains("pub async fn upload_json"));
        assert_eq!(
            analysis.operation_sources["render"].json_pointer,
            "/paths/~1render/get"
        );
        assert_eq!(
            analysis.operation_responses["render"]["200"]
                .representations
                .len(),
            5
        );
    }
    std::fs::write(
        temp.path().join("Cargo.toml"),
        format!(
            r#"[package]
name = "generated-client-transports"
version = "0.0.0"
edition = "2024"
publish = false
{dependencies}
[dev-dependencies]
axum = {{version="0.8", features=["multipart"]}}
tokio = {{version="1", features=["macros","net","rt-multi-thread","sync","time"]}}
"#
        ),
    )
    .unwrap();
    std::fs::write(temp.path().join("src/lib.rs"), RUNTIME).unwrap();
    let output = Command::new("cargo")
        .args(["test", "--quiet"])
        .current_dir(temp.path())
        .env(
            "CARGO_TARGET_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/generated-client-transports"),
        )
        .env(
            "CARGO_BUILD_BUILD_DIR",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("target/generated-client-transports/build"),
        )
        .env_remove("CC_aarch64_apple_darwin")
        .env_remove("CXX_aarch64_apple_darwin")
        .env("CC", "cc")
        .env("CXX", "c++")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "generated runtime failed:\n{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .is_ok_and(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == "wasm32-unknown-unknown")
        })
    {
        let output = Command::new("cargo")
            .args([
                "check",
                "--quiet",
                "--lib",
                "--target",
                "wasm32-unknown-unknown",
            ])
            .current_dir(temp.path())
            .env(
                "CARGO_TARGET_DIR",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/generated-client-transports"),
            )
            .env(
                "CARGO_BUILD_BUILD_DIR",
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("target/generated-client-transports/build"),
            )
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "generated wasm client compile failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

const RUNTIME: &str = r###"pub mod typed;
pub mod strings;
#[cfg(test)]
mod tests {
    use super::typed::{client::{HttpClient, ApiOpError}, types::UploadRequest};
    use axum::{body::Body, extract::{Multipart, Query, State}, http::{HeaderMap, StatusCode}, response::Response, routing::{get, post}, Router};
    use futures_util::StreamExt;
    use std::{collections::BTreeMap, sync::{Arc, Mutex}, time::Duration};
    use tokio::sync::{mpsc, Notify};
    type Captures = Arc<Mutex<Vec<BTreeMap<String,(Option<String>,Vec<u8>)>>>>;
    #[derive(Clone, Default)]
    struct ServerState { captured: Captures, release: Arc<Notify> }
    async fn upload(State(state): State<ServerState>, Query(query): Query<BTreeMap<String,String>>, mut multipart: Multipart) -> StatusCode {
        assert_eq!(query.get("multipart_filenames").map(String::as_str), Some("query"));
        let mut parts = BTreeMap::new();
        while let Some(field) = multipart.next_field().await.unwrap() {
            let name = field.name().unwrap().to_string();
            let filename = field.file_name().map(str::to_string);
            parts.insert(name, (filename, field.bytes().await.unwrap().to_vec()));
        }
        state.captured.lock().unwrap().push(parts);
        StatusCode::NO_CONTENT
    }
    async fn render(State(state): State<ServerState>, Query(query): Query<BTreeMap<String,String>>, headers: HeaderMap) -> Response {
        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret");
        assert_eq!(headers.get("x-client").unwrap(), "shared");
        if query.get("fail").is_some_and(|s| s == "typed") {
            return Response::builder().status(400).header("content-type","application/json").body(Body::from(r#"{"message":"bad"}"#)).unwrap();
        }
        if query.get("fail").is_some_and(|s| s == "large") {
            return Response::builder().status(400).body(Body::from(vec![b'x'; 2048])).unwrap();
        }
        let media = headers.get("accept").unwrap().to_str().unwrap();
        if query.get("fail").is_some_and(|s| s == "content") {
            let body = Body::from_stream(futures_util::stream::once(async { Ok::<_,std::io::Error>(bytes::Bytes::from_static(b"wrong")) }).chain(futures_util::stream::pending()));
            return Response::builder().header("content-type","text/plain").body(body).unwrap();
        }
        let body = match media {
            "application/json" => Body::from(r#"{"value":"default"}"#),
            "application/vnd.alternate+json" => Body::from(r#"{"alternate":42}"#),
            "text/plain" => Body::from("text"),
            "text/event-stream" => Body::from("id: 7\nevent: update\ndata: hello\ndata: world\n\n"),
            "application/octet-stream" => {
                let (tx, rx) = mpsc::channel::<Result<bytes::Bytes,std::io::Error>>(2);
                tokio::spawn(async move {
                    tx.send(Ok(bytes::Bytes::from_static(b"first"))).await.unwrap();
                    state.release.notified().await;
                    tx.send(Ok(bytes::Bytes::from_static(b"last"))).await.unwrap();
                });
                Body::from_stream(futures_util::stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|item| (item,rx)) }))
            },
            other => panic!("unexpected accept {other}"),
        };
        Response::builder().header("content-type", format!("{media}; charset=utf-8")).body(body).unwrap()
    }
    fn request() -> UploadRequest { UploadRequest { front_page: bytes::Bytes::from_static(b"front"), back: bytes::Bytes::from_static(b"back"), omitted: None, nullable: Some(None), referenced: None, caption: Some("caption".into()) } }
    #[allow(dead_code)]
    async fn collision_signatures(client: &HttpClient, request: super::typed::types::UploadCollisionRequest) {
        let _ = client.upload_collision_with_multipart_filenames(None::<&str>, None::<&str>, request, &[]).await;
        let _ = client.upload_stream_binary_stream_builder(bytes::Bytes::from_static(b"back"), bytes::Bytes::from_static(b"front"))
            .multipart_filenames("query").with_multipart_filenames(&[("front-page","a.pdf")]).send().await;
        let _ = client.new_2().await;
        let _ = client.r#type().await;
        let _ = client.type_binary_stream().await;
        let _ = client.r#type_binary_stream_2().await;
    }
    #[tokio::test]
    async fn transports() {
        let state = ServerState::default();
        let app = Router::new().route("/upload", post(upload)).route("/render", get(render))
            .route("/status", get(|| async { (StatusCode::CREATED, axum::Json(serde_json::json!({"two":2}))) }))
            .route("/range", get(|| async { axum::Json(serde_json::json!({"one":"exact"})) }))
            .route("/identifiers", get(|Query(query):Query<BTreeMap<String,String>>| async move {
                assert_eq!(query["self"],"first"); assert_eq!(query["self_param"],"second"); StatusCode::NO_CONTENT
            }))
            .route("/profile", get(|| async { Response::builder().header("content-type","application/json; profile=A; charset=utf-8").body(Body::from(r#"{"b":"should not decode"}"#)).unwrap() }))
            .route("/quoted-profile", get(|Query(query):Query<BTreeMap<String,String>>, headers:HeaderMap| async move {
                assert_eq!(headers.get("accept").unwrap(), r#"application/json; profile="a\";c""#);
                let media = if query.get("matching").is_some_and(|value| value=="true") { r#"application/json; profile="a\";c"; charset=utf-8"# } else { r#"application/json; profile="a\";b""# };
                Response::builder().header("content-type",media).body(Body::from(r#"{"c":"matched"}"#)).unwrap()
            }))
            .with_state(state.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = HttpClient::new().with_base_url(format!("http://{address}")).with_api_key("secret").with_header("x-client", "shared");
        client.upload(Some("query"), request()).await.unwrap();
        assert!(state.captured.lock().unwrap()[0].values().all(|(filename,_)| filename.is_none()));
        let (one,two) = tokio::join!(
            client.upload_with_multipart_filenames_2(Some("query"), request(), &[("front-page","a.pdf"),("back","b.pdf"),("omitted","absent.pdf"),("nullable","null.pdf"),("referenced","ref.pdf")]),
            client.upload_with_multipart_filenames_2(Some("query"), request(), &[("front-page","c.pdf"),("back","d.pdf")])
        );
        one.unwrap(); two.unwrap();
        let captures = state.captured.lock().unwrap().clone();
        assert_eq!(captures.len(),3);
        for capture in &captures[1..] {
            assert!(!capture.contains_key("omitted"));
            assert!(!capture.contains_key("nullable"));
            assert!(!capture.contains_key("referenced"));
            let front = capture["front-page"].0.as_deref().unwrap();
            let back = capture["back"].0.as_deref().unwrap();
            assert!(matches!((front,back),("a.pdf","b.pdf")|("c.pdf","d.pdf")));
            assert_eq!(capture["front-page"].1,b"front");
        }
        for filenames in [&[("front-page","a"),("front-page","b")][..], &[("caption","a")][..], &[("unknown","a")][..]] {
            assert!(matches!(client.upload_with_multipart_filenames_2(Some("query"), request(), filenames).await, Err(ApiOpError::Transport(super::typed::HttpError::Config(_)))));
        }
        client.upload_builder(bytes::Bytes::from_static(b"back"), bytes::Bytes::from_static(b"front"))
            .multipart_filenames("query").with_multipart_filenames(&[("front-page","builder.pdf")]).send().await.unwrap();
        let strings = super::strings::client::HttpClient::new().with_base_url(format!("http://{address}"));
        strings.upload_with_multipart_filenames_2(Some("query"), super::strings::types::UploadRequest { front_page:"string".into(),back:"back".into(),omitted:None,nullable:Some(None),referenced:None,caption:None }, &[("front-page","string.pdf")]).await.unwrap();
        assert_eq!(state.captured.lock().unwrap().last().unwrap()["front-page"],(Some("string.pdf".into()),b"string".to_vec()));
        assert_eq!(client.render(None::<&str>).await.unwrap().value,"default");
        assert_eq!(client.render_json_application_vnd_alternate_json(None::<&str>).await.unwrap().alternate,42);
        assert_eq!(client.render_text(None::<&str>).await.unwrap(),"text");
        let mut stream = Box::pin(tokio::time::timeout(Duration::from_secs(2),client.render_binary_stream(None::<&str>)).await.expect("method buffered live response").unwrap());
        assert_eq!(tokio::time::timeout(Duration::from_secs(2),stream.next()).await.unwrap().unwrap().unwrap(),b"first"[..]);
        state.release.notify_one();
        assert_eq!(stream.next().await.unwrap().unwrap(),b"last"[..]);
        assert!(stream.next().await.is_none());
        let mut events = client.render_sse(None::<&str>).await.unwrap();
        let event = events.next().await.unwrap().unwrap();
        assert_eq!(event.data,"hello\nworld");
        assert_eq!(event.id.as_deref(),Some("7"));
        let error = client.render_binary_stream(Some("typed")).await.err().unwrap();
        assert!(matches!(error,ApiOpError::Api(ref error) if error.status==400 && error.typed.is_some()));
        let limited = client.clone().with_max_response_body_bytes(16);
        assert!(matches!(limited.render_binary_stream(Some("large")).await,Err(ApiOpError::Transport(_))));
        assert!(matches!(tokio::time::timeout(Duration::from_secs(2),client.render_binary_stream(Some("content"))).await.expect("invalid live media buffered indefinitely"),Err(ApiOpError::Api(ref error)) if error.parse_error.as_ref().unwrap().contains("Content-Type")));
        client.identifiers("first","second").await.unwrap();
        assert!(matches!(client.range_json_status_2xx().await,Err(ApiOpError::Api(ref error)) if error.parse_error.as_ref().unwrap().contains("unexpected successful status")));
        assert!(matches!(client.profile_json_application_json_profile_b().await,Err(ApiOpError::Api(ref error)) if error.parse_error.as_ref().unwrap().contains("Content-Type")));
        assert!(matches!(client.quoted_profile_json_application_json_profile_a_c(None).await,Err(ApiOpError::Api(ref error)) if error.parse_error.as_ref().unwrap().contains("Content-Type")));
        assert_eq!(client.quoted_profile_json_application_json_profile_a_c(Some(true)).await.unwrap().c,"matched");
        assert_eq!(client.status_json_status_201().await.unwrap().two,2);
        assert!(matches!(client.status().await, Err(ApiOpError::Api(ref error)) if error.status==201));
    }
}
"###;

#[test]
fn operation_sources_address_effective_document_nodes() {
    let document = json!({
        "openapi":"3.2.0", "info":{"title":"source", "version":"1"},
        "components":{"pathItems":{"shared":{"get":{"operationId":"shared", "responses":{"204":{"description":"ok"}}}}}},
        "paths":{
            "/a/~b":{"$ref":"#/components/pathItems/shared"},
            "/custom":{"additionalOperations":{"PURGE":{"operationId":"purge", "responses":{"204":{"description":"ok"}}}}}
        },
        "webhooks":{"event/~name":{"post":{"operationId":"event", "responses":{"204":{"description":"ok"}}}}}
    });
    let analysis = SchemaAnalyzer::new(document.clone())
        .unwrap()
        .analyze()
        .unwrap();
    assert_eq!(
        analysis.operation_sources["shared"].json_pointer,
        "/components/pathItems/shared/get"
    );
    assert_eq!(analysis.operation_sources["shared"].path, "/a/~b");
    assert_eq!(
        analysis.operation_sources["purge"].json_pointer,
        "/paths/~1custom/additionalOperations/PURGE"
    );
    assert_eq!(
        analysis.operation_sources["event"].json_pointer,
        "/webhooks/event~1~0name/post"
    );
    for source in analysis.operation_sources.values() {
        assert!(
            document.pointer(&source.json_pointer).is_some(),
            "{}",
            source.json_pointer
        );
    }
}
