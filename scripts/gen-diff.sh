#!/usr/bin/env bash
# Diff the code this generator emits for every pinned spec against the code the
# generator at some earlier ref emitted for the same specs.
#
# Nothing generated is checked in. The specs in specs/ are pinned and generation
# is byte-deterministic, so the old output is reconstructed on demand: build the
# generator at <base-ref> in a throwaway worktree, regenerate the corpus, diff.
# The whole corpus regenerates in well under a minute, and the base side is
# cached per commit, so repeat runs against the same base only pay for HEAD.
#
# Usage:
#   scripts/gen-diff.sh                 # vs merge-base with main
#   scripts/gen-diff.sh v0.15.0         # vs a release tag
#   scripts/gen-diff.sh HEAD~1
#
# Env:
#   GEN_DIFF_SPECS="anthropic openai"   restrict to these specs
#   GEN_DIFF_PROFILE=debug              build without --release
#   GEN_DIFF_REFRESH=1                  ignore the cached base corpus
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib/corpus.sh

ROOT="tmp/gen-diff"
PROFILE="${GEN_DIFF_PROFILE:-release}"
read -r -a SPEC_FILTER <<<"${GEN_DIFF_SPECS:-}"

BASE_REF="${1:-}"
if [ -z "$BASE_REF" ]; then
  BASE_REF="$(git merge-base HEAD main 2>/dev/null || echo main)"
fi
BASE_SHA="$(git rev-parse --verify "${BASE_REF}^{commit}")"
echo "[gen-diff] base $BASE_REF ($(git log -1 --format='%h %s' "$BASE_SHA"))"

BASE_OUT="$ROOT/base/$BASE_SHA"
HEAD_OUT="$ROOT/head"
REPORT="$ROOT/report"
rm -rf "$HEAD_OUT" "$REPORT"
mkdir -p "$HEAD_OUT" "$REPORT"

# ---- base side (cached per commit) --------------------------------------
if [ "${GEN_DIFF_REFRESH:-}" = "1" ]; then
  rm -rf "$BASE_OUT"
fi
if [ -f "$BASE_OUT/.corpus-complete" ]; then
  echo "[gen-diff] reusing cached base corpus ($BASE_OUT)"
else
  rm -rf "$BASE_OUT"
  mkdir -p "$BASE_OUT"
  WT="$ROOT/worktrees/$BASE_SHA"
  rm -rf "$WT"
  git worktree add --detach --quiet "$WT" "$BASE_SHA"
  # Remove the worktree even if the build or generation fails, or the next run
  # trips over a stale registration.
  trap 'git worktree remove --force "$WT" >/dev/null 2>&1 || true' EXIT
  echo "[gen-diff] building generator at $BASE_SHA..."
  BASE_BIN="$(corpus_build "$PWD/$WT" "$PWD/$ROOT/target-base" "$PROFILE")"
  echo "[gen-diff] generating base corpus..."
  corpus_generate "$BASE_BIN" "$BASE_OUT" "${SPEC_FILTER[@]}" || true
  touch "$BASE_OUT/.corpus-complete"
  git worktree remove --force "$WT" >/dev/null 2>&1 || true
  trap - EXIT
fi

# ---- head side (always regenerated; the working tree may be dirty) -------
echo "[gen-diff] building generator at working tree..."
HEAD_BIN="$(corpus_build "$PWD" "$PWD/target" "$PROFILE")"
echo "[gen-diff] generating working-tree corpus..."
corpus_generate "$HEAD_BIN" "$HEAD_OUT" "${SPEC_FILTER[@]}" || true

# ---- compare -------------------------------------------------------------
# Top-level items are what a consumer of the generated crate actually sees, so
# an added/removed name is worth far more attention than a churned line count.
items() {
  local dir="$1"
  [ -d "$dir" ] || return 0
  find "$dir" -name '*.rs' -exec cat {} + 2>/dev/null \
    | grep -oE '^pub (struct|enum|trait|type|fn|const) [A-Za-z0-9_]+' \
    | awk '{print $2, $3}' | sort -u
}

names="$(
  { find "$BASE_OUT" -maxdepth 1 -mindepth 1 -type d -exec basename {} \;
    find "$HEAD_OUT" -maxdepth 1 -mindepth 1 -type d -exec basename {} \; ; } | sort -u
)"

rows=""
changed=0
: >"$REPORT/items.txt"
for name in $names; do
  b="$BASE_OUT/$name"
  h="$HEAD_OUT/$name"
  mkdir -p "$b" "$h"

  # `git diff --no-index` between two directories lists every file, including
  # identical ones (their paths differ, so it reports them as 0/0 renames);
  # only rows with real churn count as a changed file.
  numstat="$(git diff --no-index --numstat "$b" "$h" 2>/dev/null || true)"
  added=$(echo "$numstat" | awk '{a += $1} END {print a + 0}')
  removed=$(echo "$numstat" | awk '{r += $2} END {print r + 0}')
  files=$(echo "$numstat" | awk '($1 + $2) > 0' | grep -c . || true)

  new_items="$(comm -13 <(items "$b") <(items "$h"))"
  gone_items="$(comm -23 <(items "$b") <(items "$h"))"
  n_new=$(echo "$new_items" | grep -c . || true)
  n_gone=$(echo "$gone_items" | grep -c . || true)

  [ "$files" -eq 0 ] && continue
  changed=$((changed + 1))
  git diff --no-index "$b" "$h" >"$REPORT/$name.diff" 2>/dev/null || true
  rows+="$((added + removed))|$name|$files|$added|$removed|$n_new|$n_gone"$'\n'
  if [ -n "$new_items$gone_items" ]; then
    {
      echo "=== $name ==="
      [ -n "$new_items" ] && echo "$new_items" | sed 's/^/  + /'
      [ -n "$gone_items" ] && echo "$gone_items" | sed 's/^/  - /'
    } >>"$REPORT/items.txt"
  fi
done

echo
if [ "$changed" -eq 0 ]; then
  echo "[gen-diff] no change: every spec generates identical code at both refs."
  exit 0
fi

printf '%-24s %6s %9s %9s %8s %8s\n' SPEC FILES +LINES -LINES +ITEMS -ITEMS
printf '%-24s %6s %9s %9s %8s %8s\n' ------------------------ ------ --------- --------- -------- --------
echo "$rows" | grep . | sort -t'|' -k1,1nr | while IFS='|' read -r _ name files added removed n_new n_gone; do
  printf '%-24s %6s %9s %9s %8s %8s\n' "$name" "$files" "$added" "$removed" "$n_new" "$n_gone"
done

echo
echo "[gen-diff] $changed spec(s) changed."
echo "  per-spec diffs:   $REPORT/<spec>.diff"
if [ -s "$REPORT/items.txt" ]; then
  echo "  item add/remove:  $REPORT/items.txt"
  echo
  head -40 "$REPORT/items.txt"
fi
