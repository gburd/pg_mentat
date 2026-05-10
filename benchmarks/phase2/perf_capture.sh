#!/usr/bin/env bash
# benchmarks/phase2/perf_capture.sh — CPU flamegraph capture for the hot path.
#
# Runs `perf record` while replaying the query workload at the middle scale
# (300K datoms), then collapses stacks and produces an SVG flamegraph.
#
# Prerequisites:
#   - Linux with `perf` installed (linux-tools-$(uname -r))
#   - Dataset already loaded (run run.sh first, or at least the 300k scenario)
#   - PostgreSQL backend accessible at the configured PGHOST/PGPORT
#   - User has permissions for `perf record` (or use sudo/sysctl)
#
# Output:
#   benchmarks/results/phase2-<timestamp>/flamegraph.svg
#   benchmarks/results/phase2-<timestamp>/perf.data
#
# Usage:
#   CARGO_HOME=$HOME/.cargo_pg_mentat bash benchmarks/phase2/perf_capture.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT_DIR="${REPO_ROOT}/benchmarks/results/phase2-perf-${STAMP}"
mkdir -p "${OUT_DIR}"

PSQL="${PSQL:-${HOME}/.pgrx/16.13/pgrx-install/bin/psql}"
PGHOST="${PGHOST:-${HOME}/.pgrx}"
PGPORT="${PGPORT:-28816}"
PGDATABASE="${PGDATABASE:-postgres}"
export PGHOST PGPORT PGDATABASE

TOOLS_DIR="${REPO_ROOT}/benchmarks/phase2/tools"
STACKCOLLAPSE="${TOOLS_DIR}/stackcollapse-perf.pl"
FLAMEGRAPH="${TOOLS_DIR}/flamegraph.pl"

if [[ ! -x "${STACKCOLLAPSE}" ]] || [[ ! -x "${FLAMEGRAPH}" ]]; then
    echo "perf_capture: FlameGraph scripts not found or not executable in ${TOOLS_DIR}" >&2
    echo "  Expected: stackcollapse-perf.pl, flamegraph.pl" >&2
    exit 2
fi

if ! command -v perf &> /dev/null; then
    echo "perf_capture: 'perf' not found. Install linux-tools-$(uname -r)" >&2
    exit 2
fi

# Find the PostgreSQL backend PID serving our session.
# We start a long-running query and grab the backend PID, then kill it.
BACKEND_PID=$("${PSQL}" -X -t -A -c "SELECT pg_backend_pid();")
echo "perf_capture: target backend PID = ${BACKEND_PID}"

# Build the replay workload (30 repetitions of all 4 queries)
REPLAY_SQL=$(mktemp)
trap "rm -f ${REPLAY_SQL}" EXIT

for rep in $(seq 1 30); do
    for q_edn in "${REPO_ROOT}/benchmarks/phase2/queries/"q*.edn; do
        edn=$(cat "${q_edn}" | sed "s/'/''/g")
        echo "SELECT mentat_query('${edn}', '{}'::jsonb);" >> "${REPLAY_SQL}"
    done
done

echo "perf_capture: starting perf record (120 query executions)..."

# Record at 99Hz for the duration of the replay.
# Use -g for call-graph (dwarf or fp depending on build).
perf record -F 99 -g -p "${BACKEND_PID}" -o "${OUT_DIR}/perf.data" -- \
    sleep 0 &
PERF_PID=$!

# Give perf a moment to attach
sleep 0.2

# Run the replay workload through the same backend
"${PSQL}" -X -v ON_ERROR_STOP=1 -q -f "${REPLAY_SQL}" > /dev/null 2>&1

# Stop perf
kill "${PERF_PID}" 2>/dev/null || true
wait "${PERF_PID}" 2>/dev/null || true

echo "perf_capture: collapsing stacks..."
perf script -i "${OUT_DIR}/perf.data" | \
    "${STACKCOLLAPSE}" > "${OUT_DIR}/stacks.folded"

echo "perf_capture: generating flamegraph SVG..."
"${FLAMEGRAPH}" \
    --title "pg_mentat query hot path (300k datoms, 4 queries x 30 reps)" \
    --width 1600 \
    "${OUT_DIR}/stacks.folded" > "${OUT_DIR}/flamegraph.svg"

echo ""
echo "perf_capture: done"
echo "  perf.data:    ${OUT_DIR}/perf.data"
echo "  stacks:       ${OUT_DIR}/stacks.folded"
echo "  flamegraph:   ${OUT_DIR}/flamegraph.svg"
