#!/usr/bin/env bash
# Lazily fetches the APIs.guru openapi-directory into ./apis-guru/.
# We do NOT track this as a submodule — it's ~hundreds of MB and too large for
# the default clone. Run this once to enable the APIs.guru smoke runner.
#
# Usage:
#   tests/conformance/external/apis-guru-sync.sh
#   APIS_GURU_SMOKE=1 cargo test --test conformance_apis_guru
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
DEST="$HERE/apis-guru"
if [ -d "$DEST/.git" ]; then
  echo "Updating $DEST..."
  git -C "$DEST" pull --ff-only
else
  echo "Cloning APIs.guru openapi-directory (shallow)..."
  git clone --depth 1 https://github.com/APIs-guru/openapi-directory.git "$DEST"
fi
echo "Done. Run: APIS_GURU_SMOKE=1 cargo test --test conformance_apis_guru"
