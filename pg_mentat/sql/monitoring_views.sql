-- Monitoring views for pg_mentat production operations.
--
-- These views provide operational visibility into index health, table
-- statistics, and maintenance status without requiring external tools.

-- mentat.index_health: Per-index statistics including bloat estimates
CREATE OR REPLACE VIEW mentat.index_health AS
SELECT
    i.schemaname || '.' || i.relname AS table_name,
    i.indexrelname AS index_name,
    pg_size_pretty(pg_relation_size(i.indexrelid)) AS index_size,
    pg_size_pretty(pg_relation_size(i.relid)) AS table_size,
    i.idx_scan AS index_scans,
    i.idx_tup_fetch AS rows_fetched,
    t.n_dead_tup AS dead_tuples,
    t.n_live_tup AS live_tuples,
    CASE
        WHEN t.n_live_tup + t.n_dead_tup > 0 THEN
            ROUND(
                (t.n_dead_tup::NUMERIC / (t.n_live_tup + t.n_dead_tup)) * 100,
                1
            )
        ELSE 0
    END AS bloat_estimate_pct,
    t.last_vacuum,
    t.last_autovacuum,
    t.last_analyze,
    t.last_autoanalyze
FROM pg_stat_user_indexes i
JOIN pg_stat_user_tables t ON t.relid = i.relid
WHERE i.schemaname = 'mentat'
ORDER BY pg_relation_size(i.indexrelid) DESC;

-- mentat.table_health: Per-table statistics and maintenance status
CREATE OR REPLACE VIEW mentat.table_health AS
SELECT
    schemaname || '.' || relname AS table_name,
    n_live_tup AS live_tuples,
    n_dead_tup AS dead_tuples,
    CASE
        WHEN n_live_tup + n_dead_tup > 0 THEN
            ROUND(
                (n_dead_tup::NUMERIC / (n_live_tup + n_dead_tup)) * 100,
                1
            )
        ELSE 0
    END AS dead_pct,
    pg_size_pretty(pg_relation_size(relid)) AS table_size,
    pg_size_pretty(pg_total_relation_size(relid)) AS total_size,
    last_vacuum,
    last_autovacuum,
    last_analyze,
    last_autoanalyze,
    vacuum_count,
    autovacuum_count,
    analyze_count,
    autoanalyze_count
FROM pg_stat_user_tables
WHERE schemaname = 'mentat'
ORDER BY pg_total_relation_size(relid) DESC;

-- mentat.store_overview: Per-store datom counts by value type.
--
-- Aggregates live datoms grouped by `value_type_tag` from the current
-- (wide-row) `mentat.datoms` table. This view will be rewritten when the
-- narrow-leaf-partition storage redesign lands (see docs/STORAGE_REDESIGN.md).
CREATE OR REPLACE VIEW mentat.store_overview AS
WITH counts AS (
    SELECT value_type_tag, count(*) AS cnt
    FROM mentat.datoms
    WHERE added = TRUE
    GROUP BY value_type_tag
)
SELECT
    s.store_name,
    s.store_id,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 0), 0)::BIGINT  AS ref_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 1), 0)::BIGINT  AS boolean_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 2), 0)::BIGINT  AS long_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 3), 0)::BIGINT  AS double_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 4), 0)::BIGINT  AS instant_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 7), 0)::BIGINT  AS text_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 8), 0)::BIGINT  AS keyword_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 10), 0)::BIGINT AS uuid_datoms,
    COALESCE((SELECT cnt FROM counts WHERE value_type_tag = 11), 0)::BIGINT AS bytes_datoms,
    COALESCE((SELECT SUM(cnt) FROM counts), 0)::BIGINT                      AS total_datoms
FROM mentat.stores s
ORDER BY s.store_name;
