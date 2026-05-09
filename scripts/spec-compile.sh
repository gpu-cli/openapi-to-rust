#!/usr/bin/env bash
# Smoke-test that generated clients for every spec under specs/ compile cleanly.
#
# Auto-discovers specs/*.yaml and specs/*.json. Each spec produces a separate
# scratch crate; we run the `openapi-to-rust` generator into it and then
# `cargo check`. Any regression here means a real-world spec stops compiling.
#
# Usage:
#   scripts/spec-compile.sh                        # all specs in specs/
#   scripts/spec-compile.sh anthropic openai       # subset by name
#   SPEC_COMPILE_LIMIT=5 scripts/spec-compile.sh   # first 5 only (CI smoke)
#
# Env:
#   SPEC_COMPILE_KEEP=1     keep tmp/spec-compile/<name>/ on success
#   SPEC_COMPILE_OFFLINE=1  pass --offline to cargo invocations
#   SPEC_COMPILE_LIMIT=N    process only the first N alphabetically-sorted specs
#   SPEC_COMPILE_PARSE_ONLY=1  skip cargo check; only verify the generator
#                              parses+emits without errors. Faster.
set -euo pipefail
cd "$(dirname "$0")/.."

OFFLINE=""
if [ "${SPEC_COMPILE_OFFLINE:-}" = "1" ]; then
  OFFLINE="--offline"
fi

echo "[spec-compile] building openapi-to-rust binary..."
cargo build --bin openapi-to-rust $OFFLINE >/dev/null

GEN_BIN="$(pwd)/target/debug/openapi-to-rust"
WORKSPACE="$(pwd)"

ROOT="$WORKSPACE/tmp/spec-compile"
rm -rf "$ROOT"
mkdir -p "$ROOT"

# Discover specs. Sort for deterministic output.
mapfile -t ALL_SPECS < <(find specs -maxdepth 1 -type f \( -name "*.yaml" -o -name "*.json" \) | sort)

# Filter by command-line whitelist.
WANT=("$@")
SPECS=()
for spec in "${ALL_SPECS[@]}"; do
  name="$(basename "$spec")"
  name="${name%.*}"
  if [ ${#WANT[@]} -gt 0 ]; then
    keep=0
    for w in "${WANT[@]}"; do [ "$w" = "$name" ] && keep=1; done
    [ $keep -eq 0 ] && continue
  fi
  SPECS+=("$name|$spec")
done

if [ -n "${SPEC_COMPILE_LIMIT:-}" ]; then
  SPECS=("${SPECS[@]:0:$SPEC_COMPILE_LIMIT}")
fi

if [ ${#SPECS[@]} -eq 0 ]; then
  echo "[spec-compile] no specs matched"
  exit 0
fi

echo "[spec-compile] running ${#SPECS[@]} spec(s)"
echo

passed=()
failed_gen=()
failed_check=()
skipped=()
for entry in "${SPECS[@]}"; do
  IFS='|' read -r name spec_path <<<"$entry"

  printf "%-30s " "$name"

  # Skip Swagger 2.0 specs — out of scope for this generator. Detect either
  # `"swagger": "2.0"` (JSON) or `swagger: "2.0"` / `swagger: 2.0` (YAML).
  if grep -qE '("swagger"\s*:|swagger\s*:)\s*"?2\.' "$spec_path" 2>/dev/null \
     && ! grep -qE '("openapi"\s*:|openapi\s*:)' "$spec_path" 2>/dev/null; then
    echo "SKIP (Swagger 2.0)"
    skipped+=("$name")
    continue
  fi

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
url = { version = "2", features = ["serde"] }
# Q2 typed-scalar deps (default-on; harmless when unused).
chrono = { version = "0.4", features = ["serde"] }
uuid = { version = "1", features = ["serde", "v4"] }
bytes = { version = "1", features = ["serde"] }
base64 = "0.22"
EOF

  cat >"$dir/src/lib.rs" <<EOF
#![allow(dead_code, unused_imports, clippy::all)]
pub mod generated;
EOF

  # Sanitize module name (replace - with _).
  module_name="$(echo "$name" | tr '-' '_')"

  cat >"$dir/openapi-to-rust.toml" <<EOF
[generator]
spec_path = "$WORKSPACE/$spec_path"
output_dir = "src/generated"
module_name = "$module_name"

[features]
enable_async_client = true

[http_client]
base_url = "https://example.invalid"
timeout_seconds = 60
EOF

  # Generator step
  log="$dir/generate.log"
  if ! ( cd "$dir" && "$GEN_BIN" generate --config openapi-to-rust.toml ) >"$log" 2>&1; then
    echo "GEN-FAIL"
    failed_gen+=("$name")
    continue
  fi

  if [ "${SPEC_COMPILE_PARSE_ONLY:-}" = "1" ]; then
    echo "GEN-OK"
    passed+=("$name")
    [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$dir"
    continue
  fi

  # Cargo check step
  log="$dir/check.log"
  if ! ( cd "$dir" && cargo check $OFFLINE ) >"$log" 2>&1; then
    err_count=$(grep -cE "^error" "$log" || true)
    echo "CHECK-FAIL ($err_count errs)"
    failed_check+=("$name")
    continue
  fi

  echo "PASS"
  passed+=("$name")
  [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$dir"
done

echo
echo "[spec-compile] summary: ${#passed[@]} passed, ${#failed_gen[@]} gen-failed, ${#failed_check[@]} check-failed, ${#skipped[@]} skipped"
[ ${#failed_gen[@]}   -gt 0 ] && echo "  gen-fail:   ${failed_gen[*]}"
[ ${#failed_check[@]} -gt 0 ] && echo "  check-fail: ${failed_check[*]}"
[ ${#skipped[@]}      -gt 0 ] && echo "  skipped:    ${skipped[*]}"

if [ ${#failed_gen[@]} -gt 0 ] || [ ${#failed_check[@]} -gt 0 ]; then
  exit 1
fi
echo "[spec-compile] ✅ all specs compiled cleanly"
