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
                (t.n_dead_tup::FLOAT / (t.n_live_tup + t.n_dead_tup)) * 100,
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
                (n_dead_tup::FLOAT / (n_live_tup + n_dead_tup)) * 100,
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

-- mentat.store_overview: Per-store datom counts by value type
-- Shows the number of active datoms per store, broken down by type.
-- Useful for monitoring data distribution and detecting type skew.
CREATE OR REPLACE VIEW mentat.store_overview AS
SELECT
    s.store_name,
    s.store_id,
    COALESCE(r.cnt, 0) AS ref_datoms,
    COALESCE(b.cnt, 0) AS boolean_datoms,
    COALESCE(l.cnt, 0) AS long_datoms,
    COALESCE(d.cnt, 0) AS double_datoms,
    COALESCE(t.cnt, 0) AS text_datoms,
    COALESCE(k.cnt, 0) AS keyword_datoms,
    COALESCE(i.cnt, 0) AS instant_datoms,
    COALESCE(u.cnt, 0) AS uuid_datoms,
    COALESCE(by.cnt, 0) AS bytes_datoms,
    COALESCE(r.cnt, 0) + COALESCE(b.cnt, 0) + COALESCE(l.cnt, 0)
        + COALESCE(d.cnt, 0) + COALESCE(t.cnt, 0) + COALESCE(k.cnt, 0)
        + COALESCE(i.cnt, 0) + COALESCE(u.cnt, 0) + COALESCE(by.cnt, 0)
        AS total_datoms
FROM mentat.stores s
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_ref_new WHERE store_id = s.store_id AND added = true) r ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_boolean_new WHERE store_id = s.store_id AND added = true) b ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_long_new WHERE store_id = s.store_id AND added = true) l ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_double_new WHERE store_id = s.store_id AND added = true) d ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_text_new WHERE store_id = s.store_id AND added = true) t ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_keyword_new WHERE store_id = s.store_id AND added = true) k ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_instant_new WHERE store_id = s.store_id AND added = true) i ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_uuid_new WHERE store_id = s.store_id AND added = true) u ON true
LEFT JOIN LATERAL (SELECT count(*) AS cnt FROM mentat.datoms_bytes_new WHERE store_id = s.store_id AND added = true) by ON true
ORDER BY s.store_name;
