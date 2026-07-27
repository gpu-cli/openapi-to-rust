#!/usr/bin/env bash
# Smoke-test that generated clients for every spec under specs/ compile cleanly.
#
# Auto-discovers specs/*.yaml and specs/*.json. Each spec produces a separate
# scratch crate; we run the `openapi-to-rust` generator into it, then check
# each crate against its emitted REQUIRED_DEPS.toml. Checks stay isolated so
# Cargo feature unification cannot hide a missing per-spec feature.
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
#   SPEC_COMPILE_FORCE_CHECK=1 also cargo check the specs in
#                              GENERATE_ONLY_SPECS (see below), which are
#                              skipped by default because their generated
#                              crate exceeds CI runner memory.
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

# Specs whose generated crate is too large to `cargo check` inside a standard
# CI runner. They are still generated (which catches the majority of generator
# defects); only the compile step is skipped, and the summary reports them
# separately so a green run is never mistaken for full verification.
#
# Set SPEC_COMPILE_FORCE_CHECK=1 to check them anyway on a machine with the
# headroom. Measured peaks, `cargo check`, single rustc process:
#   microsoft-graph  2.4M lines generated  ~14.3 GB RSS  (16,153 operations)
# A GitHub-hosted ubuntu-latest runner has 16 GB total, so it is killed with
# SIGTERM partway through. Raising cargo parallelism does not help — the memory
# is one rustc type-checking one crate.
GENERATE_ONLY_SPECS=("microsoft-graph")

is_generate_only() {
  [ "${SPEC_COMPILE_FORCE_CHECK:-}" = "1" ] && return 1
  for entry in "${GENERATE_ONLY_SPECS[@]}"; do
    [ "$entry" = "$1" ] && return 0
  done
  return 1
}

generate_only_reason() {
  case "$1" in
    microsoft-graph) echo "~14.3 GB RSS, exceeds CI runner memory" ;;
    *) echo "exceeds CI runner resources" ;;
  esac
}

# ---- Phase 1: generate a scratch crate per spec -------------------------
passed=()
failed_gen=()
failed_check=()
skipped=()
generate_only=()
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

  deps="$dir/src/generated/REQUIRED_DEPS.toml"
  if [ ! -f "$deps" ]; then
    echo "GEN-FAIL (missing REQUIRED_DEPS.toml)"
    failed_gen+=("$name")
    continue
  fi
  {
    # Empty [workspace] keeps the scratch crate out of the repo's workspace;
    # without it cargo walks up, finds the root manifest, and refuses.
    echo "[workspace]"
    echo
    echo "[package]"
    echo "name = \"spec-compile-$name\""
    echo "version = \"0.0.0\""
    echo "edition = \"2024\""
    echo "publish = false"
    echo
    cat "$deps"
  } >"$dir/Cargo.toml"

  echo "GEN-OK"
  gen_ok+=("$name")
done

if [ "${SPEC_COMPILE_PARSE_ONLY:-}" = "1" ]; then
  passed=("${gen_ok[@]}")
  [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$ROOT"
elif [ ${#gen_ok[@]} -gt 0 ]; then
  # ---- Phase 2: check every exact generated manifest ---------------------
  echo
  echo "[spec-compile] cargo check (${#gen_ok[@]} isolated manifest(s))..."
  for name in "${gen_ok[@]}"; do
    if is_generate_only "$name"; then
      printf "%-30s GEN-ONLY (cargo check skipped: %s)\n" "$name" "$(generate_only_reason "$name")"
      generate_only+=("$name")
      continue
    fi
    log="$ROOT/$name/check.log"
    if ( cd "$ROOT/$name" && CARGO_TARGET_DIR="$SCRATCH_TARGET" cargo check $OFFLINE ) >"$log" 2>&1; then
      printf "%-30s PASS\n" "$name"
      passed+=("$name")
    else
      err_count=$(grep -cE "^error" "$log" || true)
      printf "%-30s CHECK-FAIL (%s errs)\n" "$name" "$err_count"
      failed_check+=("$name")
    fi
  done
  [ "${SPEC_COMPILE_KEEP:-}" != "1" ] && rm -rf "$ROOT"
fi

echo
echo "[spec-compile] summary: ${#passed[@]} passed, ${#failed_gen[@]} gen-failed, ${#failed_check[@]} check-failed, ${#generate_only[@]} generate-only, ${#skipped[@]} skipped"
[ ${#failed_gen[@]}   -gt 0 ] && echo "  gen-fail:   ${failed_gen[*]}"
[ ${#failed_check[@]} -gt 0 ] && echo "  check-fail: ${failed_check[*]}"
[ ${#skipped[@]}      -gt 0 ] && echo "  skipped:    ${skipped[*]}"
if [ ${#generate_only[@]} -gt 0 ]; then
  echo "  generate-only (NOT compile-verified): ${generate_only[*]}"
  echo "  ^ these generated cleanly but were never compiled. Run them locally"
  echo "    on a machine with enough RAM: scripts/spec-compile.sh ${generate_only[*]}"
fi

if [ ${#failed_gen[@]} -gt 0 ] || [ ${#failed_check[@]} -gt 0 ]; then
  exit 1
fi
if [ ${#generate_only[@]} -gt 0 ]; then
  echo "[spec-compile] ✅ ${#passed[@]} spec(s) compiled cleanly; ${#generate_only[@]} generated but not compiled"
else
  echo "[spec-compile] ✅ all specs compiled cleanly"
fi
