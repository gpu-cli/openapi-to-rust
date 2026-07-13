#!/usr/bin/env bash
# Smoke-test that generated clients for every spec under specs/ compile cleanly.
#
# Auto-discovers specs/*.yaml and specs/*.json. Each spec produces a separate
# scratch crate; we run the `openapi-to-rust` generator into it, then check
# all scratch crates as ONE cargo workspace: a single dependency resolution,
# a single target-dir lock, and cargo schedules the per-crate checks across
# all cores itself. Any regression here means a real-world spec stops
# compiling.
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
#   SPEC_COMPILE_TARGET_DIR=path  shared cargo target dir for the scratch
#                              workspace (default tmp/spec-compile-target).
#                              Dependency artifacts (reqwest, chrono, …)
#                              compile once and are reused by all specs — and
#                              by later runs, since the dir survives this
#                              script's per-run cleanup. Wipe it to force a
#                              cold build.
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

# Shared target dir for the scratch workspace. Deliberately OUTSIDE $ROOT so
# it survives the rm -rf above and stays warm across runs. Only exported for
# the `cargo check` step — the generator build above must keep using the
# workspace target/.
SCRATCH_TARGET="${SPEC_COMPILE_TARGET_DIR:-$WORKSPACE/tmp/spec-compile-target}"
mkdir -p "$SCRATCH_TARGET"

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

# ---- Phase 1: generate a scratch crate per spec -------------------------
passed=()
failed_gen=()
failed_check=()
skipped=()
gen_ok=()
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

  echo "GEN-OK"
  gen_ok+=("$name")
done

if [ "${SPEC_COMPILE_PARSE_ONLY:-}" = "1" ]; then
  passed=("${gen_ok[@]}")
  [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$ROOT"
elif [ ${#gen_ok[@]} -gt 0 ]; then
  # ---- Phase 2: check everything as one workspace ------------------------
  {
    echo "[workspace]"
    echo "resolver = \"2\""
    echo "members = ["
    for name in "${gen_ok[@]}"; do
      echo "  \"$name\","
    done
    echo "]"
  } >"$ROOT/Cargo.toml"

  echo
  echo "[spec-compile] cargo check (workspace of ${#gen_ok[@]} crates)..."
  ws_log="$ROOT/check.log"
  if ( cd "$ROOT" && CARGO_TARGET_DIR="$SCRATCH_TARGET" cargo check --workspace --keep-going $OFFLINE ) >"$ws_log" 2>&1; then
    passed=("${gen_ok[@]}")
    for name in "${gen_ok[@]}"; do
      printf "%-30s PASS\n" "$name"
    done
    [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$ROOT"
  else
    # Attribute failures per crate. Everything that compiles is already
    # cached from the workspace pass, so these re-checks are cheap. Passing
    # crates are cleaned up only after the loop — they must stay on disk
    # while they're still members of the workspace being checked.
    for name in "${gen_ok[@]}"; do
      log="$ROOT/$name/check.log"
      if ( cd "$ROOT" && CARGO_TARGET_DIR="$SCRATCH_TARGET" cargo check -p "spec-compile-$name" $OFFLINE ) >"$log" 2>&1; then
        printf "%-30s PASS\n" "$name"
        passed+=("$name")
      else
        err_count=$(grep -cE "^error" "$log" || true)
        printf "%-30s CHECK-FAIL (%s errs)\n" "$name" "$err_count"
        failed_check+=("$name")
      fi
    done
    if [ "${SPEC_COMPILE_KEEP:-}" != "1" ]; then
      for name in "${passed[@]}"; do
        rm -rf "$ROOT/$name"
      done
    fi
  fi
fi

echo
echo "[spec-compile] summary: ${#passed[@]} passed, ${#failed_gen[@]} gen-failed, ${#failed_check[@]} check-failed, ${#skipped[@]} skipped"
[ ${#failed_gen[@]}   -gt 0 ] && echo "  gen-fail:   ${failed_gen[*]}"
[ ${#failed_check[@]} -gt 0 ] && echo "  check-fail: ${failed_check[*]}"
[ ${#skipped[@]}      -gt 0 ] && echo "  skipped:    ${skipped[*]}"

if [ ${#failed_gen[@]} -gt 0 ] || [ ${#failed_check[@]} -gt 0 ]; then
  exit 1
fi
echo "[spec-compile] ✅ all specs compiled cleanly"
