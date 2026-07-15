#!/usr/bin/env bash
# Verify the source-install experience promised to users. This deliberately
# exercises `cargo install` instead of `cargo run`, so accidental public binary
# targets and missing packaged files are caught before release.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SMOKE_ROOT="$(mktemp -d "${TMPDIR:-/tmp}/openapi-to-rust-install-smoke.XXXXXX")"
INSTALL_ROOT="$SMOKE_ROOT/install"
PROJECT_ROOT="$SMOKE_ROOT/project"
PACKAGE_TARGET="$SMOKE_ROOT/package-target"
TARGET_DIR="${CARGO_TARGET_DIR:-$SMOKE_ROOT/target}"

cleanup() {
  if [ "${INSTALL_SMOKE_KEEP:-0}" = "1" ]; then
    echo "[install-smoke] kept temporary files at $SMOKE_ROOT"
  else
    rm -rf "$SMOKE_ROOT"
  fi
}
trap cleanup EXIT

echo "[install-smoke] packaging the exact crate contents that users receive"
CARGO_TARGET_DIR="$PACKAGE_TARGET" cargo package \
  --manifest-path "$REPO_ROOT/Cargo.toml" \
  --locked \
  --allow-dirty \
  --no-verify

archives=("$PACKAGE_TARGET"/package/openapi-to-rust-*.crate)
if [ "${#archives[@]}" -ne 1 ] || [ ! -f "${archives[0]}" ]; then
  echo "[install-smoke] expected exactly one packaged crate archive" >&2
  find "$PACKAGE_TARGET/package" -maxdepth 1 -type f -print >&2
  exit 1
fi

mkdir -p "$SMOKE_ROOT/package-source"
tar -xzf "${archives[0]}" -C "$SMOKE_ROOT/package-source"
package_dirs=("$SMOKE_ROOT"/package-source/openapi-to-rust-*)
if [ "${#package_dirs[@]}" -ne 1 ] || [ ! -d "${package_dirs[0]}" ]; then
  echo "[install-smoke] expected exactly one unpacked crate directory" >&2
  exit 1
fi
PACKAGE_ROOT="${package_dirs[0]}"

echo "[install-smoke] checking the packaged default dependency tree"
dependency_tree="$(cargo tree \
  --manifest-path "$PACKAGE_ROOT/Cargo.toml" \
  --locked \
  --edges normal)"
if printf '%s\n' "$dependency_tree" \
  | grep -E 'reqwest v0\.11|hyper v0\.14|insta v|tempfile v' >/dev/null; then
  echo "[install-smoke] default dependency tree contains a forbidden dependency" >&2
  printf '%s\n' "$dependency_tree" >&2
  exit 1
fi
if ! printf '%s\n' "$dependency_tree" | grep -E 'reqwest v0\.12' >/dev/null; then
  echo "[install-smoke] expected reqwest 0.12 in the default dependency tree" >&2
  exit 1
fi

echo "[install-smoke] installing the packaged crate into a temporary root"
CARGO_TARGET_DIR="$TARGET_DIR" cargo install \
  --path "$PACKAGE_ROOT" \
  --locked \
  --root "$INSTALL_ROOT"

BIN_DIR="$INSTALL_ROOT/bin"
BIN="$BIN_DIR/openapi-to-rust"
installed_count="$(find "$BIN_DIR" -maxdepth 1 -type f | wc -l | tr -d '[:space:]')"
if [ "$installed_count" -ne 1 ] || [ ! -x "$BIN" ]; then
  echo "[install-smoke] expected exactly one executable named openapi-to-rust" >&2
  find "$BIN_DIR" -maxdepth 1 -type f -print >&2
  exit 1
fi

version_output="$($BIN --version)"
package_version="$(awk -F'"' '/^version = / { print $2; exit }' "$PACKAGE_ROOT/Cargo.toml")"
expected_version="openapi-to-rust $package_version"
if [ "$version_output" != "$expected_version" ]; then
  echo "[install-smoke] expected '$expected_version', got '$version_output'" >&2
  exit 1
fi
echo "[install-smoke] $version_output"

mkdir -p "$PROJECT_ROOT"
cat >"$PROJECT_ROOT/openapi.json" <<'JSON'
{
  "openapi": "3.1.0",
  "info": { "title": "Install smoke", "version": "1.0.0" },
  "paths": {},
  "components": {
    "schemas": {
      "Greeting": {
        "type": "object",
        "required": ["message"],
        "properties": { "message": { "type": "string" } }
      }
    }
  }
}
JSON

cat >"$PROJECT_ROOT/openapi-to-rust.toml" <<EOF
[generator]
spec_path = "$PROJECT_ROOT/openapi.json"
output_dir = "$PROJECT_ROOT/src/generated"
module_name = "generated"

[features]
enable_async_client = false
enable_sse_client = false
EOF

"$BIN" generate --config "$PROJECT_ROOT/openapi-to-rust.toml"
test -s "$PROJECT_ROOT/src/generated/types.rs"
test -s "$PROJECT_ROOT/src/generated/mod.rs"

echo "[install-smoke] PASS: one executable installed, version reported, generation succeeded"
