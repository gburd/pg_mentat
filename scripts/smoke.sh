#!/usr/bin/env bash
# scripts/smoke.sh — install pg_mentat and run the smoke-test SQL.
#
# Usage:
#   bash scripts/smoke.sh            # use pgrx-managed PG 16 at ~/.pgrx/data-16
#   PG_HOST=/tmp PG_PORT=5432 bash scripts/smoke.sh   # existing server
#   CI=1 bash scripts/smoke.sh       # CI mode (expects PGHOST/PGPORT in env)
#
# Exits 0 on PASS and non-zero on FAIL. Prints a single summary line either way.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SMOKE_SQL="${REPO_ROOT}/pg_mentat/tests/smoke.sql"

if [[ ! -f "${SMOKE_SQL}" ]]; then
    echo "smoke: expected ${SMOKE_SQL} to exist" >&2
    exit 2
fi

PG_VERSION="${PG_VERSION:-pg16}"
DB_NAME="${DB_NAME:-pg_mentat_smoke}"

# ---------------------------------------------------------------------------
# Pick a psql + connection.
#
# Local mode:  use the pgrx-managed cluster at ~/.pgrx (socket there, port
#              configured in ~/.pgrx/config.toml, defaults to 288<NN>).
# CI mode:     PGHOST / PGPORT / PGUSER set by the workflow; use the
#              system psql. Skip `cargo pgrx start` since the GitHub
#              Actions postgres service container is already running.
# ---------------------------------------------------------------------------

if [[ -n "${CI:-}" || -n "${PGHOST:-}" ]]; then
    MODE="ci"
    PSQL="${PSQL:-psql}"
    : "${PGHOST:=localhost}"
    : "${PGPORT:=5432}"
    : "${PGUSER:=postgres}"
    export PGHOST PGPORT PGUSER
    PSQL_ARGS=()
else
    MODE="local"
    PGRX_HOME="${PGRX_HOME:-$HOME/.pgrx}"

    # Find pg_config and psql under ~/.pgrx/<version>/pgrx-install/bin.
    PG_MAJOR="${PG_VERSION#pg}"
    PG_INSTALL_DIR="$(find "${PGRX_HOME}" -maxdepth 2 -type d -name 'pgrx-install' \
        -path "*${PG_MAJOR}.*" | head -n1 || true)"
    if [[ -z "${PG_INSTALL_DIR}" ]]; then
        echo "smoke: no pgrx install dir for ${PG_VERSION} under ${PGRX_HOME}" >&2
        exit 2
    fi
    PSQL="${PG_INSTALL_DIR}/bin/psql"
    PG_CONFIG="${PG_INSTALL_DIR}/bin/pg_config"

    # Socket lives directly under ~/.pgrx; port is 288<NN> by pgrx convention.
    PGHOST="${PGHOST:-${PGRX_HOME}}"
    PGPORT="${PGPORT:-288${PG_MAJOR#1}6}"   # pg16 -> 28816, pg15 -> 28856 etc.
    # Fall back to scanning for an actual socket if the derived port is wrong.
    if [[ ! -S "${PGHOST}/.s.PGSQL.${PGPORT}" ]]; then
        for s in "${PGHOST}"/.s.PGSQL.*; do
            [[ -S "${s}" ]] || continue
            PGPORT="${s##*.}"
            break
        done
    fi
    export PGHOST PGPORT
    PSQL_ARGS=()

    echo "smoke: local mode (PGHOST=${PGHOST} PGPORT=${PGPORT})"

    # Install the extension into the pgrx cluster. Requires pgrx already init'd.
    #
    # Honour a caller-provided CARGO_HOME (workaround: the nix devshell sets
    # CARGO_HOME to a read-only store path, which breaks `cargo pgrx install`;
    # local contributors typically override to $HOME/.cargo_pg_mentat).
    (
        cd "${REPO_ROOT}/pg_mentat"
        CARGO_HOME="${CARGO_HOME:-${HOME}/.cargo_pg_mentat}" \
        cargo pgrx install \
            --no-default-features --features "${PG_VERSION}" \
            --pg-config "${PG_CONFIG}"
    )
fi

run_psql() {
    "${PSQL}" -v ON_ERROR_STOP=1 "${PSQL_ARGS[@]}" "$@"
}

# ---------------------------------------------------------------------------
# Create a clean database for the smoke test.
# ---------------------------------------------------------------------------
echo "smoke: resetting database ${DB_NAME}"
run_psql -d postgres -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null
run_psql -d postgres -c "CREATE DATABASE ${DB_NAME};"         >/dev/null

# ---------------------------------------------------------------------------
# Run the smoke script. Pipe so we both see output and capture exit status.
# ---------------------------------------------------------------------------
echo "smoke: running ${SMOKE_SQL}"
set +e
run_psql -d "${DB_NAME}" -f "${SMOKE_SQL}"
status=$?
set -e

# Drop the scratch DB even on failure so reruns are idempotent.
run_psql -d postgres -c "DROP DATABASE IF EXISTS ${DB_NAME};" >/dev/null || true

if [[ "${status}" -eq 0 ]]; then
    echo "smoke: PASS (${MODE} mode)"
    exit 0
else
    echo "smoke: FAIL (${MODE} mode, psql exit ${status})" >&2
    exit "${status}"
fi
