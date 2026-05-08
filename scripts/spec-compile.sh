#!/usr/bin/env bash
# Smoke-test that generated clients for our reference specs compile cleanly.
# Each spec listed below produces a separate scratch crate; we run the
# `openapi-to-rust` generator into it and then `cargo check`. Any
# regression here means a real-world spec stops compiling.
#
# Usage:
#   scripts/spec-compile.sh                    # run all specs in SPECS
#   scripts/spec-compile.sh anthropic openai   # run a subset
#
# Env:
#   SPEC_COMPILE_KEEP=1   keep the scratch directory under tmp/spec-compile/
#   SPEC_COMPILE_OFFLINE=1 pass --offline to cargo invocations
set -euo pipefail
cd "$(dirname "$0")/.."

# (spec_name, spec_path, base_url, auth_type, auth_header)
SPECS=(
  "anthropic|specs/anthropic.yaml|https://api.anthropic.com|ApiKey|x-api-key"
  "openai|specs/openai.yaml|https://api.openai.com/v1|Bearer|Authorization"
)

# If args are given, treat them as a whitelist of spec names.
WANT=("$@")

OFFLINE=""
if [ "${SPEC_COMPILE_OFFLINE:-}" = "1" ]; then
  OFFLINE="--offline"
fi

echo "[spec-compile] building openapi-to-rust binary..."
cargo build --bin openapi-to-rust $OFFLINE >/dev/null

GEN_BIN="$(pwd)/target/debug/openapi-to-rust"

ROOT="$(pwd)/tmp/spec-compile"
rm -rf "$ROOT"
mkdir -p "$ROOT"

failed=()
for entry in "${SPECS[@]}"; do
  IFS='|' read -r name spec_path base_url auth_type auth_header <<<"$entry"
  if [ ${#WANT[@]} -gt 0 ]; then
    skip=1
    for w in "${WANT[@]}"; do [ "$w" = "$name" ] && skip=0; done
    [ $skip -eq 1 ] && continue
  fi

  echo
  echo "==> $name (spec: $spec_path)"
  dir="$ROOT/$name"
  mkdir -p "$dir/src/generated"

  cat >"$dir/Cargo.toml" <<EOF
[package]
name = "spec-compile-$name"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
serde_urlencoded = "0.7"
reqwest = { version = "0.12", features = ["json", "stream", "multipart"] }
reqwest-middleware = { version = "0.4", features = ["multipart"] }
reqwest-retry = "0.7"
reqwest-tracing = "0.5"
thiserror = "1"
url = "2"
EOF

  cat >"$dir/src/lib.rs" <<EOF
#![allow(dead_code, unused_imports, clippy::all)]
pub mod generated;
EOF

  cat >"$dir/openapi-to-rust.toml" <<EOF
[generator]
spec_path = "$(pwd)/$spec_path"
output_dir = "src/generated"
module_name = "$name"

[features]
enable_async_client = true

[http_client]
base_url = "$base_url"
timeout_seconds = 60

[http_client.auth]
type = "$auth_type"
header_name = "$auth_header"
EOF

  (
    cd "$dir"
    "$GEN_BIN" generate --config openapi-to-rust.toml >/dev/null
    if ! cargo check $OFFLINE 2>&1 | tail -200; then
      echo "[spec-compile] $name FAILED to compile" >&2
      exit 1
    fi
  ) || failed+=("$name")
done

if [ "${SPEC_COMPILE_KEEP:-}" != "1" ]; then
  rm -rf "$ROOT"
fi

if [ ${#failed[@]} -gt 0 ]; then
  echo
  echo "[spec-compile] FAILED: ${failed[*]}" >&2
  exit 1
fi

echo
echo "[spec-compile] ✅ all specs compiled cleanly"
