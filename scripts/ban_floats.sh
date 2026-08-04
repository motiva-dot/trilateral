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

# FILE-LEVEL exemption. A file whose module docs carry
#     //! FLOAT_EXCEPTION_FILE: <reason>
# in a `//!` comment is skipped entirely, and is REPORTED BY NAME on every run
# so the waiver stays visible in CI logs instead of rotting silently.
#
# Reserved for feature-gated bridge modules (currently only
# trilateral_fixed/src/float_bridge.rs). Per-line markers remain the default;
# reach for this only when an entire module is the sanctioned exception and
# repeating one identical reason ten times would just train readers to skim.
FILE_EXEMPTION_MARKER='^[[:space:]]*//!.*FLOAT_EXCEPTION_FILE:'

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
EXEMPT_FILES=""

# Pass 1 — collect file-level exemptions before scanning anything.
for dir in "${SCAN_DIRS[@]}"; do
    [ -d "$dir" ] || continue
    found=$(grep -rlE "$FILE_EXEMPTION_MARKER" "$dir" --include="*.rs" 2>/dev/null || true)
    [ -n "$found" ] && EXEMPT_FILES="${EXEMPT_FILES}${found}"$'\n'
done

is_exempt_file () {
    [ -n "$EXEMPT_FILES" ] || return 1
    printf '%s' "$EXEMPT_FILES" | grep -qxF "$1"
}

# Pass 2 — scan.
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
            if is_exempt_file "${line%%:*}"; then continue; fi
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

report_exemptions () {
    [ -n "$EXEMPT_FILES" ] || return 0
    echo "FLOAT/ENTROPY LINTER: file-level exemptions in force:"
    printf '%s' "$EXEMPT_FILES" | while IFS= read -r f; do
        [ -n "$f" ] || continue
        reason=$(grep -m1 -E "$FILE_EXEMPTION_MARKER" "$f" | sed 's/.*FLOAT_EXCEPTION_FILE:[[:space:]]*//')
        echo "  - $f  ($reason)"
    done
    echo ""
}

report_exemptions

if [ "$VIOLATIONS" -gt 0 ]; then
    echo "FLOAT/ENTROPY LINTER: ${VIOLATIONS} violation(s):"
    echo -e "$LOG"
    echo ""
    echo "Sim code uses trilateral_fixed only. Intentional? Add"
    echo "'// FLOAT_EXCEPTION: <reason>' and expect it to be challenged in review."
    exit 1
fi
echo "FLOAT/ENTROPY LINTER: PASSED"
