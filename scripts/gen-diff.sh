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
#   GEN_DIFF_MAX_DIFF_BYTES=0           do not truncate per-spec diffs (5MB cap)
#   GEN_DIFF_REFRESH=1                  ignore the cached base corpus
set -euo pipefail
cd "$(dirname "$0")/.."
source scripts/lib/corpus.sh

ROOT="tmp/gen-diff"
PROFILE="${GEN_DIFF_PROFILE:-release}"
MAX_DIFF_BYTES="${GEN_DIFF_MAX_DIFF_BYTES:-5000000}"
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
# What the run needs to compare. Taking the spec list from corpus_specs rather
# than from whatever directories exist keeps a cached wider base corpus from
# reading as deletions against a filtered head.
WANT_SPECS="$(corpus_specs "${SPEC_FILTER[@]}" | cut -d'|' -f1 | sort)"

# The cache marker records which specs the cached corpus actually holds: keyed
# on the commit alone, a corpus left behind by an earlier GEN_DIFF_SPECS run
# would be reused for a wider run and every spec it lacks would look new.
CACHE_OK=0
if [ -f "$BASE_OUT/.corpus-specs" ] \
   && [ -z "$(comm -23 <(echo "$WANT_SPECS") <(sort "$BASE_OUT/.corpus-specs"))" ]; then
  CACHE_OK=1
fi

if [ "$CACHE_OK" = "1" ]; then
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
  # Both sides build into the workspace target dir: the dependency graph is
  # identical, so reqwest and friends compile once instead of twice. Only the
  # crate itself is rebuilt per side, and the base binary is stashed first
  # because the head build overwrites it in place.
  BASE_BIN="$(corpus_build "$PWD/$WT" "$PWD/target" "$PROFILE")"
  cp "$BASE_BIN" "$ROOT/openapi-to-rust-$BASE_SHA"
  BASE_BIN="$PWD/$ROOT/openapi-to-rust-$BASE_SHA"
  echo "[gen-diff] generating base corpus..."
  corpus_generate "$BASE_BIN" "$BASE_OUT" "${SPEC_FILTER[@]}" || true
  echo "$WANT_SPECS" >"$BASE_OUT/.corpus-specs"
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

rows=""
changed=0
: >"$REPORT/items.txt"
for name in $WANT_SPECS; do
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
  # A sweeping change can diff hundreds of megabytes; keep a report a reviewer
  # (and an artifact upload) can actually handle.
  if [ "$MAX_DIFF_BYTES" -gt 0 ] \
     && [ "$(wc -c <"$REPORT/$name.diff")" -gt "$MAX_DIFF_BYTES" ]; then
    head -c "$MAX_DIFF_BYTES" "$REPORT/$name.diff" >"$REPORT/$name.diff.cut"
    mv "$REPORT/$name.diff.cut" "$REPORT/$name.diff"
    echo "... [truncated at $MAX_DIFF_BYTES bytes; rerun with" \
         "GEN_DIFF_SPECS=$name GEN_DIFF_MAX_DIFF_BYTES=0]" >>"$REPORT/$name.diff"
  fi
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
