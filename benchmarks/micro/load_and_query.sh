#!/usr/bin/env bash
# benchmarks/micro/load_and_query.sh
#
# Honest micro-benchmark on the local dev machine. Not a performance
# claim. Not publishable at scale. Just: "does the extension's performance
# degrade gracefully as you load more data"?
#
# Loads N random person records and runs three queries:
#   scan       -> full-table attribute scan (how does p50 scale with N?)
#   predicate  -> selective range predicate (does AEVT index help?)
#   group_by   -> aggregate over a grouping attribute
#
# Output: CSV under benchmarks/results/<timestamp>/timings.csv.
# Exits 0 always (this is a measurement, not a test).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
STAMP="$(date -u +%Y-%m-%dT%H%M%SZ)"
RESULTS_DIR="${REPO_ROOT}/benchmarks/results/${STAMP}"
mkdir -p "${RESULTS_DIR}"

PSQL="${PSQL:-${HOME}/.pgrx/16.13/pgrx-install/bin/psql}"
PGHOST="${PGHOST:-${HOME}/.pgrx}"
PGPORT="${PGPORT:-28816}"
PGDATABASE="${PGDATABASE:-postgres}"
export PGHOST PGPORT PGDATABASE PGOPTIONS="--client-min-messages=warning"

CSV="${RESULTS_DIR}/timings.csv"
META="${RESULTS_DIR}/env.txt"

# Write environment metadata so the CSV is reproducible.
{
    echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
    echo "host: $(hostname)"
    echo "kernel: $(uname -r)"
    echo "cpu_model: $(grep -m1 'model name' /proc/cpuinfo 2>/dev/null | sed 's/^[^:]*: //' || echo unknown)"
    echo "cpu_cores: $(nproc 2>/dev/null || echo unknown)"
    echo "memory_gb: $(awk '/MemTotal/{printf \"%.1f\", $2/1024/1024}' /proc/meminfo 2>/dev/null || echo unknown)"
    echo "postgres_version: $(psql -V | head -1)"
    echo "pg_mentat_commit: $(git -C "${REPO_ROOT}" rev-parse --short HEAD)"
    echo "pg_mentat_branch: $(git -C "${REPO_ROOT}" branch --show-current)"
} > "${META}"

echo "n_people,n_datoms,op,p50_ms,p95_ms" > "${CSV}"

# Quiet psql runner that returns only the result value for -t -A -c.
# -X disables reading .psqlrc so stray \timing settings don't leak.
q() { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -t -A -c "$1" 2>/dev/null | tail -1; }
q_file() { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -f "$1" > /dev/null 2>&1; }
q_exec() { "${PSQL}" -X -v ON_ERROR_STOP=1 -q -c "$1" > /dev/null 2>&1; }

echo "benchmark: host=$(hostname) cpu=$(nproc) mem=$(awk '/MemTotal/{printf "%.1fGB", $2/1024/1024}' /proc/meminfo) commit=$(git -C "${REPO_ROOT}" rev-parse --short HEAD)"
echo "benchmark: results -> ${RESULTS_DIR}/"

q_exec "DROP EXTENSION IF EXISTS pg_mentat CASCADE; DROP SCHEMA IF EXISTS mentat CASCADE; CREATE EXTENSION pg_mentat;"
q_exec "SET search_path = mentat, public; SELECT mentat_transact('[
  {:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :person/age  :db/valueType :db.type/long   :db/cardinality :db.cardinality/one}
  {:db/ident :person/city :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');"

# Time a single query expression N times; print p50 and p95 in ms.
time_query() {
    local q="$1"
    local times=()
    for _ in 1 2 3 4 5 6 7; do
        # psql \timing prints 'Time: 12.345 ms'; capture just the number.
        # -X avoids reading .psqlrc so we control \timing ourselves.
        local t
        t=$("${PSQL}" -X -v ON_ERROR_STOP=1 -q -t -A -c "\\timing on" -c "SELECT length(${q}::text);" 2>&1 \
            | grep -oE 'Time: [0-9]+\.[0-9]+' | head -1 | awk '{print $2}')
        times+=("$t")
    done
    # Drop first (warmup). Of remaining 6 sorted, p50 = idx 3 (1-based), p95 = max.
    local p50 p95
    p50=$(printf '%s\n' "${times[@]:1}" | sort -n | awk 'NR==3')
    p95=$(printf '%s\n' "${times[@]:1}" | sort -n | tail -1)
    echo "${p50} ${p95}"
}

for N in 1000 10000 100000; do
    echo "benchmark: loading ${N} people..."
    python3 - "$N" <<'PY' > /tmp/pg_mentat_bench_load.sql
import sys
n = int(sys.argv[1])
batch = 2500
cities = ["Boston","Chicago","Denver","Austin","Seattle","Portland","Miami","Dallas"]
print("SET search_path = mentat, public;")
for start in range(0, n, batch):
    end = min(start + batch, n)
    parts = []
    for i in range(start, end):
        city = cities[i % len(cities)]
        age = 18 + (i * 7) % 70
        parts.append(f'{{:db/id "p{i}" :person/name "Person{i}" :person/age {age} :person/city "{city}"}}')
    edn = "[" + " ".join(parts) + "]"
    print(f"SELECT mentat_transact('{edn}');")
PY
    T0=$(date +%s%N)
    q_file /tmp/pg_mentat_bench_load.sql
    T1=$(date +%s%N)
    LOAD_MS=$(( (T1 - T0) / 1000000 ))

    q_exec "ANALYZE mentat.datoms_text_new, mentat.datoms_long_new, mentat.datoms_keyword_new, mentat.datoms_ref_new;"

    NDATOMS=$(q "SELECT
        (SELECT COUNT(*) FROM mentat.datoms_text_new) +
        (SELECT COUNT(*) FROM mentat.datoms_long_new) +
        (SELECT COUNT(*) FROM mentat.datoms_keyword_new) +
        (SELECT COUNT(*) FROM mentat.datoms_ref_new) +
        (SELECT COUNT(*) FROM mentat.datoms_instant_new)")

    echo "benchmark: N=${N} datoms=${NDATOMS} load_time=${LOAD_MS}ms"

    read P50 P95 < <(time_query "mentat_query('[:find ?n :where [?e :person/name ?n]]', '{}'::jsonb)")
    echo "  scan       p50=${P50}ms p95=${P95}ms"
    echo "${N},${NDATOMS},scan,${P50},${P95}" >> "${CSV}"

    read P50 P95 < <(time_query "mentat_query('[:find ?n :where [?e :person/name ?n] [?e :person/age ?a] [(>= ?a 50)]]', '{}'::jsonb)")
    echo "  predicate  p50=${P50}ms p95=${P95}ms"
    echo "${N},${NDATOMS},predicate,${P50},${P95}" >> "${CSV}"

    read P50 P95 < <(time_query "mentat_query('[:find ?city (count ?e) :where [?e :person/city ?city]]', '{}'::jsonb)")
    echo "  group_by   p50=${P50}ms p95=${P95}ms"
    echo "${N},${NDATOMS},group_by,${P50},${P95}" >> "${CSV}"
done

echo ""
echo "benchmark: done."
echo ""
column -s, -t < "${CSV}"
echo ""
echo "env:"
cat "${META}"
