#!/usr/bin/env bash
# ban_floats.sh — First-line linter: rejects float/entropy patterns in sim code.
# NOTE: this is the FAST guard. The REAL guard is the cross-platform
# determinism CI job (see .github/workflows/ci.yml). Both must be green.

set -euo pipefail

SCAN_DIRS=(
    "crates/trilateral_fixed/src"
    "crates/sim_core/src"
    "crates/sim_systems/src"
)

BANNED_PATTERNS=(
    '\bf32\b'
    '\bf64\b'
    '\bas f'
    '\bthread_rng\b'
    '\bSystemTime\b'
    '\bInstant\b'
    '\bHashMap\b'
    '\bHashSet\b'
    '[0-9]\.[0-9]+f'
)

# Lines matching these are exempt (cfg-gated bridges, docs, explicit waivers)
EXCEPTION_PATTERNS=(
    'cfg(test)'
    'cfg(feature = "float_bridge")'
    '// FLOAT_EXCEPTION:'
    '^\s*//'
    '^\s*///'
    '^\s*//!'
)

VIOLATIONS=0
LOG=""

for dir in "${SCAN_DIRS[@]}"; do
    [ -d "$dir" ] || continue
    for pattern in "${BANNED_PATTERNS[@]}"; do
        matches=$(grep -rnE "$pattern" "$dir" --include="*.rs" 2>/dev/null || true)
        [ -n "$matches" ] || continue
        while IFS= read -r line; do
            # grep -n emits "path:lineno:source". The EXCEPTION_PATTERNS that
            # exempt comments are anchored with ^ against SOURCE text, so they
            # can never match while the path prefix is still attached — strip
            # the two leading fields before testing, or every doc comment that
            # merely mentions f64 is reported as a violation.
            code="${line#*:}"; code="${code#*:}"
            skip=false
            for exc in "${EXCEPTION_PATTERNS[@]}"; do
                if printf '%s\n' "$code" | grep -qE "$exc"; then skip=true; break; fi
            done
            if [ "$skip" = false ]; then
                VIOLATIONS=$((VIOLATIONS + 1))
                LOG="${LOG}\n  ${line}"
            fi
        done <<< "$matches"
    done
done

if [ "$VIOLATIONS" -gt 0 ]; then
    echo "FLOAT/ENTROPY LINTER: ${VIOLATIONS} violation(s):"
    echo -e "$LOG"
    echo ""
    echo "Sim code uses trilateral_fixed only. Intentional? Add"
    echo "'// FLOAT_EXCEPTION: <reason>' and expect it to be challenged in review."
    exit 1
fi
echo "FLOAT/ENTROPY LINTER: PASSED"
