#!/usr/bin/env bash
# run_desync_trace.sh — Root-cause a determinism failure. NO GUESSING.
#
# Modes:
#   ./scripts/run_desync_trace.sh                    # self-check: same seed twice
#   ./scripts/run_desync_trace.sh logA.rpl logB.rpl  # cross-client: two saved logs
#
# Pipeline (implemented by tools/desync_trace, this script orchestrates):
#   1. Locate first divergent hash checkpoint (bisect within the 1k window).
#   2. Re-run both to the exact divergent tick; dump full per-entity JSON.
#   3. Diff → report (tick, entity, component, valueA, valueB).
#   4. Re-run the divergent tick with per-system hash sampling to name the
#      last-writing system.

set -euo pipefail
OUT="target/desync_traces/$(date +%Y%m%d_%H%M%S)"
mkdir -p "$OUT"

if [ "$#" -eq 2 ]; then
    echo "=== CROSS-CLIENT TRACE: $1 vs $2 ==="
    cargo run --release -p tools --bin desync_trace -- \
        --log-a "$1" --log-b "$2" --out "$OUT"
else
    echo "=== SELF-CHECK TRACE (seed 42, CI arena scenario) ==="
    cargo run --release -p tools --bin desync_trace -- \
        --self-check --arena ci --seed 42 --out "$OUT"
fi

echo ""
echo "Report: $OUT/report.md"
echo "State dumps: $OUT/state_a.json  $OUT/state_b.json"
echo ""
echo "Rule: fix the ROOT CAUSE named in the report (system + component)."
echo "Do not re-seed, re-order, or fudge tolerances to make hashes agree."
