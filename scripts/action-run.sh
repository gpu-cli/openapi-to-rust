#!/usr/bin/env bash
set -uo pipefail

report="${RUNNER_TEMP:-/tmp}/openapi-to-rust-clients.json"
args=(clients "$CLIENT_COMMAND" --manifest "$CLIENT_MANIFEST" --json)
if [ -n "${CLIENT_NAME:-}" ]; then
  args+=(--client "$CLIENT_NAME")
fi

set +e
"$OPENAPI_TO_RUST_BIN" "${args[@]}" >"$report"
exit_code=$?
set -e

cat "$report"
node "$GITHUB_ACTION_PATH/scripts/action-report.mjs" "$report" "$exit_code"
exit 0
