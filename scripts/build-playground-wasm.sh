#!/usr/bin/env bash
# Build the playground WASM bundle and stage it for the website.
#
# Output: website/public/playground/pkg/ (gitignored build artifact).
# wasm-pack runs wasm-opt -Oz itself (configured in wasm/Cargo.toml), and the
# env vars below give the release profile size-focused settings without
# touching the workspace profile used by the CLI.
set -euo pipefail
cd "$(dirname "$0")/.."

DEST="website/public/playground/pkg"

CARGO_PROFILE_RELEASE_OPT_LEVEL=z \
CARGO_PROFILE_RELEASE_LTO=true \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 \
  wasm-pack build wasm --release --target web --out-dir "../$DEST"

# Mark the bundle as a local build so fetch-playground-wasm.mjs never
# clobbers it with the lock-pinned release asset.
echo "local" > "$DEST/.source-tag"

echo
echo "Playground WASM staged in $DEST:"
ls -lh "$DEST" | awk 'NR>1 {print "  " $9 " (" $5 ")"}'
