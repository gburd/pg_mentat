#!/usr/bin/env bash
# benchmarks/phase2/run.sh — Phase 2 benchmark driver.
#
# Loads the issue-tracker dataset into both pg_mentat and the plain-EAV
# baseline at 100K / 300K / 1M datoms, replays four query shapes 30 times
# each, captures p50/p95/p99 plus EXPLAIN plans and pg_stat_statements
# output, and writes everything to benchmarks/results/phase2-<timestamp>/.
#
# Does not run perf/flamegraph — that's perf_capture.sh (separate because
# perf needs a different invocation + produces large intermediate files).
#
# Exits non-zero if any step fails. All psql calls use ON_ERROR_STOP=on.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
OUT_DIR="${REPO_ROOT}/benchmarks/results/phase2-${STAMP}"
DATA_DIR="${OUT_DIR}/data"
PLAN_DIR="${OUT_DIR}/plans"
mkdir -p "${DATA_DIR}" "${PLAN_DIR}"

PSQL="${PSQL:-${HOME}/.pgrx/16.13/pgrx-install/bin/psql}"
PGHOST="${PGHOST:-${HOME}/.pgrx}"
PGPORT="${PGPORT:-28816}"
PGDATABASE="${PGDATABASE:-postgres}"
PG_CONFIG="${PG_CONFIG:-${HOME}/.pgrx/16.13/pgrx-install/bin/pg_config}"
export PGHOST PGPORT PGDATABASE
export PGOPTIONS="--client-min-messages=warning"

# Dataset sizes, expressed as (n_users, n_issues, n_labels).
# Users & labels scale sublinearly with issues (realistic ratio).
# n_datoms ≈ 2*users + labels + 6*issues + ~1.5*issues for labels.
SCENARIOS=(
    "100k:200:16000:50"     # ≈ 100K datoms
    "300k:600:49000:100"    # ≈ 300K datoms
    "1M:2000:162000:200"    # ≈ 1M datoms
)
# Override with BENCH_SCENARIOS env var for quick local testing.
if [[ -n "${BENCH_SCENARIOS:-}" ]]; then
    IFS=',' read -ra SCENARIOS <<<"${BENCH_SCENARIOS}"
fi

q()       { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -t -A -c "$1" 2>/dev/null | tail -1; }
q_file()  { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -f "$1" > /dev/null; }
q_exec()  { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -c "$1" > /dev/null; }
q_log()   { "${PSQL}" -X -v ON_ERROR_STOP=1 -c "$1" 2>&1; }

# ---- metadata ------------------------------------------------------------
{
    echo "date:            $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host:            $(hostname)"
    echo "kernel:          $(uname -r)"
    echo "cpu_model:       $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | sed 's/^[^:]*: //' || echo unknown)"
    echo "cpu_cores:       $(nproc 2>/dev/null || echo unknown)"
    echo "memory_gb:       $(awk '/MemTotal/{printf "%.1f", $2/1024/1024}' /proc/meminfo 2>/dev/null || echo unknown)"
    echo "postgres_version: $(${PSQL} -V | head -1)"
    echo "pg_mentat_commit:  $(git -C "${REPO_ROOT}" rev-parse --short HEAD)"
    echo "pg_mentat_branch:  $(git -C "${REPO_ROOT}" branch --show-current)"
    echo ""
    echo "scenarios:"
    for s in "${SCENARIOS[@]}"; do echo "  - ${s}"; done
    echo ""
    echo "queries:"
    for q in "${REPO_ROOT}/benchmarks/phase2/queries/"q*.edn; do
        echo "  - $(basename "${q}" .edn): $(head -1 "${q}" | sed 's/^;;\s*//')"
    done
} > "${OUT_DIR}/env.txt"

echo "phase2: results -> ${OUT_DIR}"
cat "${OUT_DIR}/env.txt"
echo ""

# ---- install + enable pg_stat_statements --------------------------------
echo "phase2: (re)installing pg_mentat extension"
(cd "${REPO_ROOT}/pg_mentat" && CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo_pg_mentat}" \
    cargo pgrx install --no-default-features --features pg16 --pg-config "${PG_CONFIG}" \
    2>&1 | tail -2)

echo "phase2: resetting databases"
q_exec "DROP EXTENSION IF EXISTS pg_mentat CASCADE;
        DROP SCHEMA    IF EXISTS mentat  CASCADE;
        DROP SCHEMA    IF EXISTS eav     CASCADE;
        CREATE EXTENSION pg_mentat;"

# pg_stat_statements is optional — it only works if the DBA has put it in
# shared_preload_libraries (the pgrx dev cluster does not by default).
# Detect by trying a function call that requires the shared-preload side.
PGSTAT_OK=yes
"${PSQL}" -X -q -v ON_ERROR_STOP=1 -c 'CREATE EXTENSION IF NOT EXISTS pg_stat_statements;' > /dev/null 2>&1 || true
if ! "${PSQL}" -X -q -v ON_ERROR_STOP=1 -c 'SELECT pg_stat_statements_reset();' > /dev/null 2>&1; then
    PGSTAT_OK=no
    echo "phase2: pg_stat_statements not preloaded; stmt-stats-*.txt will be empty"
fi

# ---- load eav baseline schema -------------------------------------------
q_file "${REPO_ROOT}/benchmarks/phase2/eav_baseline/schema.sql"

# ---- define mentat schema (runs once; it's the same across scenarios) ---
q_exec "SET search_path = mentat, public;
        SELECT mentat_transact(pg_read_file('${REPO_ROOT}/benchmarks/phase2/schema.edn'));"

# ---- CSV header ---------------------------------------------------------
CSV="${OUT_DIR}/timings.csv"
echo "scenario,n_datoms,engine,op,p50_ms,p95_ms,p99_ms" > "${CSV}"

# Time a single query N times and print p50, p95, p99 to stdout.
time_query() {
    local sql="$1"
    local samples=30
    local warm=3
    local total=$((samples + warm))
    local times=()
    for i in $(seq 1 ${total}); do
        local t
        t=$("${PSQL}" -X -v ON_ERROR_STOP=1 -q -t -A \
            -c "\\timing on" -c "SELECT count(*) FROM (${sql}) _t;" 2>&1 \
            | grep -oE 'Time: [0-9]+\.[0-9]+' | head -1 | awk '{print $2}')
        times+=("$t")
    done
    # drop first ${warm}; sort the rest; p50 = idx 15 of 30, p95 = idx 28, p99 = idx 30
    local sorted
    sorted=$(printf '%s\n' "${times[@]:${warm}}" | sort -n)
    local p50 p95 p99
    p50=$(echo "$sorted" | awk 'NR==15')
    p95=$(echo "$sorted" | awk 'NR==28')
    p99=$(echo "$sorted" | tail -1)
    echo "${p50} ${p95} ${p99}"
}

# ---- run each scenario --------------------------------------------------
for SPEC in "${SCENARIOS[@]}"; do
    IFS=':' read -r label n_users n_issues n_labels <<<"${SPEC}"
    echo ""
    echo "phase2: === scenario ${label} (users=${n_users}, issues=${n_issues}, labels=${n_labels}) ==="

    # Clean both storage back-ends (keep schema definitions intact)
    q_exec "TRUNCATE mentat.datoms_ref_new, mentat.datoms_long_new,
                     mentat.datoms_text_new, mentat.datoms_keyword_new,
                     mentat.datoms_instant_new, mentat.datoms_double_new,
                     mentat.datoms_uuid_new, mentat.datoms_bytes_new,
                     mentat.datoms_boolean_new;
            TRUNCATE eav.long, eav.text, eav.keyword, eav.ref, eav.instant;"
    if [[ "${PGSTAT_OK}" == "yes" ]]; then
        q_exec "SELECT pg_stat_statements_reset();"
    fi

    # Generate dataset (deterministic; same data goes into both)
    python3 "${REPO_ROOT}/benchmarks/phase2/gen_dataset.py" \
        "${n_users}" "${n_issues}" "${n_labels}" "${DATA_DIR}/${label}/"
    n_datoms=$(grep '^n_datoms:' "${DATA_DIR}/${label}/meta.txt" | awk '{print $2}')

    # Load mentat
    echo "phase2: ${label} loading mentat..."
    T0=$(date +%s%N)
    q_file "${DATA_DIR}/${label}/mentat_tx.sql"
    T1=$(date +%s%N)
    MENTAT_LOAD_MS=$(( (T1 - T0) / 1000000 ))
    echo "${label},${n_datoms},mentat,load,${MENTAT_LOAD_MS},${MENTAT_LOAD_MS},${MENTAT_LOAD_MS}" >> "${CSV}"

    # Load EAV
    echo "phase2: ${label} loading EAV..."
    T0=$(date +%s%N)
    q_file "${DATA_DIR}/${label}/eav_load.sql"
    T1=$(date +%s%N)
    EAV_LOAD_MS=$(( (T1 - T0) / 1000000 ))
    echo "${label},${n_datoms},eav,load,${EAV_LOAD_MS},${EAV_LOAD_MS},${EAV_LOAD_MS}" >> "${CSV}"

    # ANALYZE both sides so the planner has fresh stats
    q_exec "ANALYZE mentat.datoms_ref_new, mentat.datoms_long_new,
                    mentat.datoms_text_new, mentat.datoms_keyword_new,
                    mentat.datoms_instant_new;
            ANALYZE eav.long, eav.text, eav.keyword, eav.ref, eav.instant;"

    echo "phase2: ${label}  load_mentat=${MENTAT_LOAD_MS}ms load_eav=${EAV_LOAD_MS}ms  datoms=${n_datoms}"

    # Per-query timings
    for q_edn in "${REPO_ROOT}/benchmarks/phase2/queries/"q*.edn; do
        q_base=$(basename "${q_edn}" .edn)
        q_sql="${REPO_ROOT}/benchmarks/phase2/queries/${q_base}.sql"

        # mentat
        edn=$(cat "${q_edn}" | sed "s/'/''/g")
        mentat_expr="mentat_query('${edn}', '{}'::jsonb)::text"
        read mp50 mp95 mp99 < <(time_query "SELECT ${mentat_expr} AS r")
        echo "${label},${n_datoms},mentat,${q_base},${mp50},${mp95},${mp99}" >> "${CSV}"

        # EAV
        eav_sql=$(cat "${q_sql}")
        read ep50 ep95 ep99 < <(time_query "${eav_sql}")
        echo "${label},${n_datoms},eav,${q_base},${ep50},${ep95},${ep99}" >> "${CSV}"

        echo "phase2: ${label}  ${q_base}  mentat p50=${mp50}ms  eav p50=${ep50}ms"

        # Capture EXPLAIN for both at each scale
        q_log "SET search_path = mentat, public;
               SELECT (mentat_explain('${edn}', '{}'::jsonb)->>'explain_plan') AS plan;
              " > "${PLAN_DIR}/${label}-${q_base}-mentat.txt" 2>&1 || true
        q_log "EXPLAIN (ANALYZE, BUFFERS, VERBOSE) ${eav_sql};" \
              > "${PLAN_DIR}/${label}-${q_base}-eav.txt" 2>&1 || true
    done

    # Table and index sizes
    q_log "SELECT relname,
                  pg_size_pretty(pg_total_relation_size(c.oid))   AS total_size,
                  pg_size_pretty(pg_relation_size(c.oid))         AS heap_size,
                  pg_size_pretty(pg_indexes_size(c.oid))          AS idx_size
             FROM pg_class c
             JOIN pg_namespace n ON n.oid = c.relnamespace
            WHERE n.nspname IN ('mentat', 'eav')
              AND c.relkind = 'r'
              AND pg_total_relation_size(c.oid) > 16384
            ORDER BY pg_total_relation_size(c.oid) DESC;" \
        > "${OUT_DIR}/sizes-${label}.txt"

    # pg_stat_statements top-10 by mean_exec_time
    if [[ "${PGSTAT_OK}" == "yes" ]]; then
        q_log "SELECT round(mean_exec_time::numeric, 2) AS mean_ms,
                      round(total_exec_time::numeric, 2) AS total_ms,
                      calls,
                      left(regexp_replace(query, '\\s+', ' ', 'g'), 140) AS q
                 FROM pg_stat_statements
                WHERE query !~* 'pg_stat_statements|ANALYZE|TRUNCATE'
                ORDER BY total_exec_time DESC
                LIMIT 10;" \
            > "${OUT_DIR}/stmt-stats-${label}.txt"
    else
        echo "pg_stat_statements not available" > "${OUT_DIR}/stmt-stats-${label}.txt"
    fi
done

echo ""
echo "phase2: done"
echo ""
column -s, -t < "${CSV}"
echo ""
echo "phase2: artefacts:"
echo "  timings:  ${CSV}"
echo "  plans:    ${PLAN_DIR}/"
echo "  sizes:    ${OUT_DIR}/sizes-*.txt"
echo "  stmts:    ${OUT_DIR}/stmt-stats-*.txt"
echo "  env:      ${OUT_DIR}/env.txt"
