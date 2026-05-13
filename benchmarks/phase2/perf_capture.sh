#!/usr/bin/env bash
# benchmarks/phase2/perf_capture.sh — perf record + flamegraph on the
# mentat query hot path.
#
# Methodology:
#   1. Open a long-lived psql session.
#   2. Get the backend PID via pg_backend_pid().
#   3. Load a 300k-datom dataset (or reuse the latest phase2 run).
#   4. Start `perf record -F 99 -g -p <pid>` in the background.
#   5. Run each Phase 2 query in a tight loop (1000 iterations each)
#      against pg_mentat from the same psql session.
#   6. Stop perf, generate the flamegraph SVG via the FlameGraph tools.
#
# Output: benchmarks/results/phase2-perf-<timestamp>/
#   - perf.data
#   - perf.folded
#   - flame.svg                 (the flamegraph)
#   - meta.txt                  (commit, datoms, query count, kernel)
#   - queries.log               (psql output of the replay)
#
# This requires `perf` available and kernel.perf_event_paranoid <= 1.
# On the dev laptop here, paranoid = -1, so unprivileged perf works.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TOOLS="${REPO_ROOT}/benchmarks/phase2/tools"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT_DIR="${REPO_ROOT}/benchmarks/results/phase2-perf-${STAMP}"
mkdir -p "${OUT_DIR}"

PSQL="${PSQL:-${HOME}/.pgrx/16.13/pgrx-install/bin/psql}"
PGHOST="${PGHOST:-${HOME}/.pgrx}"
PGPORT="${PGPORT:-28816}"
PGDATABASE="${PGDATABASE:-postgres}"
export PGHOST PGPORT PGDATABASE PGOPTIONS="--client-min-messages=warning"

DURATION="${PERF_DURATION:-30}"     # seconds of perf record
ITERS="${PERF_ITERS:-1000}"          # query iterations per loop
N_USERS="${PERF_USERS:-600}"         # use 300k-datom scale by default
N_ISSUES="${PERF_ISSUES:-49000}"
N_LABELS="${PERF_LABELS:-100}"

# ---- preflight ----------------------------------------------------------
if [[ ! -r "${TOOLS}/stackcollapse-perf.pl" || ! -r "${TOOLS}/flamegraph.pl" ]]; then
    echo "perf_capture: FlameGraph tools missing in ${TOOLS}/" >&2
    exit 2
fi
if ! command -v perf >/dev/null 2>&1; then
    echo "perf_capture: perf binary not on PATH" >&2
    exit 2
fi
PARANOID=$(cat /proc/sys/kernel/perf_event_paranoid 2>/dev/null || echo 99)
if [[ "${PARANOID}" -gt 1 ]]; then
    echo "perf_capture: kernel.perf_event_paranoid=${PARANOID}; need <= 1 for unprivileged perf record -g" >&2
    exit 2
fi

# ---- ensure mentat schema and a 300k dataset are present ----------------
DATA_DIR="${OUT_DIR}/data"
mkdir -p "${DATA_DIR}"
echo "perf_capture: generating dataset (${N_USERS} users, ${N_ISSUES} issues, ${N_LABELS} labels)..."
python3 "${REPO_ROOT}/benchmarks/phase2/gen_dataset.py" \
    "${N_USERS}" "${N_ISSUES}" "${N_LABELS}" "${DATA_DIR}/"

q() { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -t -A -c "$1" 2>/dev/null | tail -1; }

# Reset and load
echo "perf_capture: resetting + loading mentat..."
"${PSQL}" -X -v ON_ERROR_STOP=1 -q -c "
    DROP EXTENSION IF EXISTS pg_mentat CASCADE;
    DROP SCHEMA    IF EXISTS mentat  CASCADE;
    CREATE EXTENSION pg_mentat;" > /dev/null
"${PSQL}" -X -v ON_ERROR_STOP=1 -q -c "
    SET search_path = mentat, public;
    SELECT length((mentat_transact(pg_read_file('${REPO_ROOT}/benchmarks/phase2/schema.edn')))::text);" > /dev/null
"${PSQL}" -X -v ON_ERROR_STOP=1 -q -f "${DATA_DIR}/mentat_tx.sql" > /dev/null
"${PSQL}" -X -v ON_ERROR_STOP=1 -q -c "
    ANALYZE mentat.datoms_ref_new, mentat.datoms_long_new,
            mentat.datoms_text_new, mentat.datoms_keyword_new,
            mentat.datoms_instant_new;" > /dev/null

N_DATOMS=$(grep '^n_datoms:' "${DATA_DIR}/meta.txt" | awk '{print $2}')
echo "perf_capture: loaded ${N_DATOMS} datoms"

# ---- run the replay: open one psql session, get PID, attach perf -------
QUERY_LOG="${OUT_DIR}/queries.log"
PIDFILE="${OUT_DIR}/backend.pid"

# Use a FIFO so we can stream commands into psql while perf runs.
FIFO="${OUT_DIR}/cmds.fifo"
mkfifo "${FIFO}"

# Start psql in the background, reading from the FIFO. It will keep
# running until we close the FIFO.
"${PSQL}" -X -v ON_ERROR_STOP=1 < "${FIFO}" > "${QUERY_LOG}" 2>&1 &
PSQL_PID=$!

# Open a dedicated FD writer to the FIFO so it stays open even after we
# write our first batch.
exec 3>"${FIFO}"

# Send the PID-fetching prelude.
cat >&3 <<'SQL'
\timing off
SET search_path = mentat, public;
\pset format unaligned
\pset tuples_only on
\o /tmp/phase2-backend-pid
SELECT pg_backend_pid();
\o
SQL

# Give psql a moment to write the PID file
sleep 1
BACKEND_PID=$(cat /tmp/phase2-backend-pid 2>/dev/null | head -1 | tr -d '[:space:]')
if [[ -z "${BACKEND_PID}" ]]; then
    echo "perf_capture: failed to capture backend PID" >&2
    exec 3>&-
    wait "${PSQL_PID}" 2>/dev/null || true
    exit 3
fi
echo "${BACKEND_PID}" > "${PIDFILE}"
echo "perf_capture: backend pid = ${BACKEND_PID}"

# ---- launch perf record in background ----------------------------------
PERF_DATA="${OUT_DIR}/perf.data"
echo "perf_capture: starting perf record for ${DURATION}s (sample 99 Hz, call graph)..."
# `--call-graph fp` (frame pointer) works on this Alder Lake hybrid CPU;
# `--call-graph dwarf` produced 0 samples in testing because of how the
# kernel exposes events on the heterogeneous P/E core split. Plain `-g`
# falls back to fp by default. Pin one of cpu_core or cpu_atom via -e to
# avoid the hybrid mux issue producing empty data.
perf record -F 99 -e cpu_core/cycles/ -g --call-graph fp -o "${PERF_DATA}" -p "${BACKEND_PID}" \
    sleep "${DURATION}" > "${OUT_DIR}/perf.stderr" 2>&1 &
PERF_PID=$!

# ---- replay queries on the same backend until perf finishes ------------
# Keep streaming work into the same psql session so perf samples the right pid.
{
cat <<SQL
-- The four Phase 2 queries, each looped ITERS times.
DO \$\$
DECLARE
    i int;
BEGIN
    FOR i IN 1..${ITERS} LOOP
        PERFORM mentat_query('[:find ?e ?name :where [?e :user/email "user100000@example.com"] [?e :user/name ?name]]', '{}'::jsonb);
        PERFORM mentat_query('[:find ?i ?title ?state :where [?u :user/email "user100000@example.com"] [?i :issue/assignee ?u] [?i :issue/title ?title] [?i :issue/state ?state]]', '{}'::jsonb);
        PERFORM mentat_query('[:find ?state (count ?i) :where [?i :issue/state ?state]]', '{}'::jsonb);
        PERFORM mentat_query('[:find ?i ?title ?priority :where [?i :issue/state :state/open] [?i :issue/priority ?priority] [?i :issue/title ?title] [(>= ?priority 4)]]', '{}'::jsonb);
    END LOOP;
END \$\$;
SQL
} >&3

# Wait for perf to finish
wait "${PERF_PID}" || true

# Close the FIFO so psql exits.
exec 3>&-
wait "${PSQL_PID}" 2>/dev/null || true
rm -f "${FIFO}"

if [[ ! -s "${PERF_DATA}" ]]; then
    echo "perf_capture: perf.data is empty; check ${OUT_DIR}/perf.stderr" >&2
    exit 4
fi

echo "perf_capture: perf.data captured ($(du -h "${PERF_DATA}" | awk '{print $1}'))"

# ---- generate the flamegraph -------------------------------------------
echo "perf_capture: generating folded stacks..."
perf script -i "${PERF_DATA}" 2>/dev/null \
    | perl "${TOOLS}/stackcollapse-perf.pl" \
    > "${OUT_DIR}/perf.folded"

echo "perf_capture: rendering SVG..."
perl "${TOOLS}/flamegraph.pl" \
    --title "pg_mentat query hot path (${N_DATOMS} datoms, ${ITERS} iters x 4 queries)" \
    --subtitle "perf record -F 99 -g, ${DURATION}s window, $(date -u +%Y-%m-%d)" \
    "${OUT_DIR}/perf.folded" \
    > "${OUT_DIR}/flame.svg"

# ---- meta ---------------------------------------------------------------
{
    echo "date:             $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host:             $(hostname)"
    echo "kernel:           $(uname -r)"
    echo "cpu_model:        $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | sed 's/^[^:]*: //' || echo unknown)"
    echo "pg_mentat_commit: $(git -C "${REPO_ROOT}" rev-parse --short HEAD)"
    echo "pg_mentat_branch: $(git -C "${REPO_ROOT}" branch --show-current)"
    echo "perf_event_paranoid: ${PARANOID}"
    echo "perf_record_seconds: ${DURATION}"
    echo "iters_per_query:  ${ITERS}"
    echo "n_users:          ${N_USERS}"
    echo "n_issues:         ${N_ISSUES}"
    echo "n_labels:         ${N_LABELS}"
    echo "n_datoms:         ${N_DATOMS}"
    echo "perf_data_bytes:  $(stat -c%s "${PERF_DATA}")"
    echo "perf_folded_bytes: $(stat -c%s "${OUT_DIR}/perf.folded")"
    echo "flame_svg_bytes:  $(stat -c%s "${OUT_DIR}/flame.svg")"
} > "${OUT_DIR}/meta.txt"

echo ""
echo "perf_capture: artefacts in ${OUT_DIR}/"
echo "  flame.svg       (open in a browser; click frames to zoom)"
echo "  perf.data       (raw samples; replay with: perf report -i perf.data)"
echo "  perf.folded     (one-line-per-stack input to flamegraph.pl)"
echo "  queries.log     (replay output)"
echo "  meta.txt        (env)"
echo ""
cat "${OUT_DIR}/meta.txt"
