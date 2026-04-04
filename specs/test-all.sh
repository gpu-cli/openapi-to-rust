#!/usr/bin/env bash
# Test registry generation against all downloaded specs
set -uo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
GENERATOR="$DIR/../target/release/openapi-to-rust"
OUT_DIR="/tmp/registry-test-all"
rm -rf "$OUT_DIR"
mkdir -p "$OUT_DIR"

pass=0
fail=0
results=""

for spec in "$DIR"/*.json "$DIR"/*.yaml; do
    [ -f "$spec" ] || continue
    name=$(basename "$spec" | sed 's/\.\(json\|yaml\)$//')
    outdir="$OUT_DIR/$name"
    mkdir -p "$outdir"

    # Create a temp config
    cat > "$outdir/config.toml" <<EOF
[generator]
spec_path = "$spec"
output_dir = "$outdir/gen"
module_name = "$name"

[features]
enable_async_client = false
enable_registry = false
registry_only = true
EOF

    # Run generator, capture time
    start=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')
    output=$("$GENERATOR" generate --config "$outdir/config.toml" 2>&1)
    exit_code=$?
    end=$(date +%s%N 2>/dev/null || python3 -c 'import time; print(int(time.time()*1e9))')

    duration_ms=$(( (end - start) / 1000000 ))

    if [ $exit_code -eq 0 ]; then
        # Count operations and lines in registry
        if [ -f "$outdir/gen/registry.rs" ]; then
            ops=$(grep -c "OperationDef {" "$outdir/gen/registry.rs" 2>/dev/null || echo "0")
            lines=$(wc -l < "$outdir/gen/registry.rs" | tr -d ' ')
        else
            ops="0"
            lines="0"
        fi
        schemas=$(echo "$output" | grep "Found.*schemas" | grep -o '[0-9]*' || echo "?")
        results+="  [PASS] $name — ${ops} ops, ${lines} lines, ${schemas} schemas, ${duration_ms}ms\n"
        pass=$((pass + 1))
    else
        error=$(echo "$output" | tail -1)
        results+="  [FAIL] $name — $error (${duration_ms}ms)\n"
        fail=$((fail + 1))
    fi
done

echo "=== Registry Generation Results ==="
echo ""
echo -e "$results" | sort
echo ""
echo "=== Summary ==="
echo "  Pass: $pass"
echo "  Fail: $fail"
echo "  Total: $((pass + fail))"
