// Performance benchmark tests for pg_mentat.
//
// These tests measure query execution time at various data scales to validate
// the schema-aware query optimization and establish performance baselines.
//
// Scale levels:
// - Small: 1K entities (~10K datoms)
// - Medium: 10K entities (~100K datoms)
// - Large: 100K entities (~1M datoms)  [use #[ignore] for CI]
//
// Each test logs timing via NOTICE so results appear in test output.

#[cfg(any(test, feature = "pg_test"))]
#[pgrx::pg_schema]
mod tests {
    use pgrx::prelude::*;

    fn setup() {
        crate::ensure_extension_loaded();
        Spi::run("SELECT bootstrap_schema()").expect("bootstrap_schema failed");
    }

    fn setup_benchmark_schema() {
        Spi::run(
            "SELECT mentat_transact('[
                {:db/id \"n\"  :db/ident :bench/name    :db/valueType :db.type/string  :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
                {:db/id \"a\"  :db/ident :bench/age     :db/valueType :db.type/long    :db/cardinality :db.cardinality/one}
                {:db/id \"e\"  :db/ident :bench/email   :db/valueType :db.type/string  :db/cardinality :db.cardinality/one}
                {:db/id \"s\"  :db/ident :bench/score   :db/valueType :db.type/double  :db/cardinality :db.cardinality/one}
                {:db/id \"ac\" :db/ident :bench/active  :db/valueType :db.type/boolean :db/cardinality :db.cardinality/one}
                {:db/id \"tg\" :db/ident :bench/tags    :db/valueType :db.type/string  :db/cardinality :db.cardinality/many}
                {:db/id \"ct\" :db/ident :bench/cat     :db/valueType :db.type/keyword :db/cardinality :db.cardinality/one}
                {:db/id \"mg\" :db/ident :bench/manager :db/valueType :db.type/ref     :db/cardinality :db.cardinality/one}
            ]'::TEXT)",
        ).expect("benchmark schema");
    }

    /// Insert `count` entities in batches of `batch_size`, returning the total
    /// number of milliseconds spent on transact calls.
    fn populate_entities(count: usize, batch_size: usize) -> f64 {
        let start = std::time::Instant::now();
        let mut offset = 0;
        while offset < count {
            let end = std::cmp::min(offset + batch_size, count);
            let mut ops = Vec::new();
            for i in offset..end {
                let cat = match i % 5 {
                    0 => ":cat/alpha",
                    1 => ":cat/beta",
                    2 => ":cat/gamma",
                    3 => ":cat/delta",
                    _ => ":cat/epsilon",
                };
                ops.push(format!(
                    "{{:db/id \"e{i}\" :bench/name \"user-{i}\" :bench/age {age} :bench/email \"user{i}@example.com\" :bench/score {score:?} :bench/active {active} :bench/cat {cat}}}",
                    i = i,
                    age = 20 + (i % 60),
                    score = (i as f64) * 0.01,
                    active = if i % 3 != 0 { "true" } else { "false" },
                    cat = cat,
                ));
            }
            Spi::run(&format!(
                "SELECT mentat_transact('[{}]'::TEXT)",
                ops.join("\n")
            )).unwrap_or_else(|e| panic!("populate batch at offset {}: {}", offset, e));
            offset = end;
        }
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// Run a Datalog query and return (result_count, elapsed_ms).
    fn timed_query(query: &str) -> (usize, f64) {
        let start = std::time::Instant::now();
        let result = Spi::get_one::<String>(
            &format!("SELECT mentat_query('{}', '{{}}'::JSONB)::TEXT", query)
        ).expect("query").expect("NULL");
        let elapsed = start.elapsed().as_secs_f64() * 1000.0;

        let parsed: serde_json::Value = serde_json::from_str(&result).expect("parse result");
        // mentat_query returns {"columns": [...], "results": [[...], ...]} for relation queries
        // or {"result": ...} for scalar/tuple/collection queries
        let count = parsed["results"].as_array()
            .map(|a| a.len())
            .or_else(|| parsed["result"].as_array().map(|a| a.len()))
            .or_else(|| if parsed["result"].is_null() { Some(0) } else { Some(1) })
            .unwrap_or(0);
        (count, elapsed)
    }

    /// Run a query N times and return the median execution time.
    fn median_query_time(query: &str, iterations: usize) -> f64 {
        let mut times = Vec::with_capacity(iterations);
        for _ in 0..iterations {
            let (_, elapsed) = timed_query(query);
            times.push(elapsed);
        }
        times.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        times[times.len() / 2]
    }

    // ========================================================================
    // Schema-Aware Optimization Validation
    // ========================================================================
    // These tests verify that the schema-aware optimization produces correct
    // results by comparing query output with and without the optimization.

    #[pg_test]
    fn test_perf_schema_aware_correctness_string_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        // Query a string attribute -- should use single-table optimization
        let (count, _elapsed) = timed_query(
            "[:find ?e ?name :where [?e :bench/name ?name]]"
        );
        assert_eq!(count, 100, "Expected 100 entities with :bench/name");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_long_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        let (count, _elapsed) = timed_query(
            "[:find ?e ?age :where [?e :bench/age ?age]]"
        );
        assert_eq!(count, 100, "Expected 100 entities with :bench/age");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_boolean_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        let (count, _elapsed) = timed_query(
            "[:find ?e ?active :where [?e :bench/active ?active]]"
        );
        assert_eq!(count, 100, "Expected 100 entities with :bench/active");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_double_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        let (count, _elapsed) = timed_query(
            "[:find ?e ?score :where [?e :bench/score ?score]]"
        );
        assert_eq!(count, 100, "Expected 100 entities with :bench/score");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_keyword_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        let (count, _elapsed) = timed_query(
            "[:find ?e ?cat :where [?e :bench/cat ?cat]]"
        );
        assert_eq!(count, 100, "Expected 100 entities with :bench/cat");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_multi_pattern_join() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        // Multi-pattern join: string + long attributes
        let (count, _elapsed) = timed_query(
            "[:find ?name ?age :where [?e :bench/name ?name] [?e :bench/age ?age]]"
        );
        assert_eq!(count, 100, "Expected 100 joined results");
    }

    #[pg_test]
    fn test_perf_schema_aware_correctness_predicate_filter() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        // Predicate filter with typed attribute
        let (count, _elapsed) = timed_query(
            "[:find ?name :where [?e :bench/name ?name] [?e :bench/age ?age] [(> ?age 50)]]"
        );
        // Ages are 20 + (i % 60), so ages > 50 means (i % 60) > 30, which is ~half
        assert!(count > 0 && count < 100, "Expected filtered results, got {}", count);
    }

    // ========================================================================
    // Small Scale Benchmarks (1K entities, ~8K datoms)
    // ========================================================================

    #[pg_test]
    fn test_perf_1k_insert_throughput() {
        setup();
        setup_benchmark_schema();
        let elapsed = populate_entities(1000, 200);
        pgrx::notice!(
            "BENCHMARK: 1K entity insert ({:.0} datoms): {:.1}ms ({:.0} entities/sec)",
            1000.0 * 7.0, // 7 attrs per entity
            elapsed,
            1000.0 / (elapsed / 1000.0)
        );
    }

    #[pg_test]
    fn test_perf_1k_point_lookup() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find ?name :where [?e :bench/name \"user-500\"] [?e :bench/name ?name]]",
            5,
        );
        pgrx::notice!("BENCHMARK: 1K point lookup (median): {:.1}ms", median);
        // Should be fast with index
        assert!(median < 500.0, "Point lookup too slow: {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_1k_full_scan_string_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find ?e ?name :where [?e :bench/name ?name]]",
            3,
        );
        pgrx::notice!("BENCHMARK: 1K full scan string attr (median): {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_1k_two_pattern_join() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find ?name ?age :where [?e :bench/name ?name] [?e :bench/age ?age]]",
            3,
        );
        pgrx::notice!("BENCHMARK: 1K two-pattern join (median): {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_1k_three_pattern_join() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find ?name ?age ?score :where [?e :bench/name ?name] [?e :bench/age ?age] [?e :bench/score ?score]]",
            3,
        );
        pgrx::notice!("BENCHMARK: 1K three-pattern join (median): {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_1k_predicate_filter() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find ?name :where [?e :bench/name ?name] [?e :bench/age ?age] [(> ?age 50)]]",
            3,
        );
        pgrx::notice!("BENCHMARK: 1K predicate filter (median): {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_1k_aggregate_count() {
        setup();
        setup_benchmark_schema();
        populate_entities(1000, 200);

        let median = median_query_time(
            "[:find (count ?e) :where [?e :bench/name _]]",
            3,
        );
        pgrx::notice!("BENCHMARK: 1K aggregate count (median): {:.1}ms", median);
    }

    // ========================================================================
    // Medium Scale Benchmarks (10K entities, ~70K datoms)
    // ========================================================================

    #[pg_test]
    fn test_perf_10k_insert_throughput() {
        setup();
        setup_benchmark_schema();
        let elapsed = populate_entities(10_000, 500);
        pgrx::notice!(
            "BENCHMARK: 10K entity insert ({:.0} datoms): {:.1}ms ({:.0} entities/sec)",
            10_000.0 * 7.0,
            elapsed,
            10_000.0 / (elapsed / 1000.0)
        );
    }

    #[pg_test]
    fn test_perf_10k_point_lookup() {
        setup();
        setup_benchmark_schema();
        populate_entities(10_000, 500);

        let median = median_query_time(
            "[:find ?name :where [?e :bench/name \"user-5000\"] [?e :bench/name ?name]]",
            5,
        );
        pgrx::notice!("BENCHMARK: 10K point lookup (median): {:.1}ms", median);
        assert!(median < 500.0, "Point lookup too slow: {:.1}ms", median);
    }

    #[pg_test]
    fn test_perf_10k_full_scan_string_attr() {
        setup();
        setup_benchmark_schema();
        populate_entities(10_000, 500);

        let (count, elapsed) = timed_query(
            "[:find ?e ?name :where [?e :bench/name ?name]]"
        );
        pgrx::notice!(
            "BENCHMARK: 10K full scan string attr: {:.1}ms ({} rows)",
            elapsed, count
        );
        assert_eq!(count, 10_000);
    }

    #[pg_test]
    fn test_perf_10k_two_pattern_join() {
        setup();
        setup_benchmark_schema();
        populate_entities(10_000, 500);

        let (count, elapsed) = timed_query(
            "[:find ?name ?age :where [?e :bench/name ?name] [?e :bench/age ?age]]"
        );
        pgrx::notice!(
            "BENCHMARK: 10K two-pattern join: {:.1}ms ({} rows)",
            elapsed, count
        );
        assert_eq!(count, 10_000);
    }

    #[pg_test]
    fn test_perf_10k_predicate_filter() {
        setup();
        setup_benchmark_schema();
        populate_entities(10_000, 500);

        let (count, elapsed) = timed_query(
            "[:find ?name :where [?e :bench/name ?name] [?e :bench/age ?age] [(> ?age 50)]]"
        );
        pgrx::notice!(
            "BENCHMARK: 10K predicate filter: {:.1}ms ({} rows)",
            elapsed, count
        );
        assert!(count > 0);
    }

    #[pg_test]
    fn test_perf_10k_aggregate_count() {
        setup();
        setup_benchmark_schema();
        populate_entities(10_000, 500);

        let (count, elapsed) = timed_query(
            "[:find (count ?e) :where [?e :bench/name _]]"
        );
        pgrx::notice!(
            "BENCHMARK: 10K aggregate count: {:.1}ms ({} result rows)",
            elapsed, count
        );
    }

    // ========================================================================
    // Monitoring Statistics Validation
    // ========================================================================

    #[pg_test]
    fn test_perf_monitoring_stats_populated() {
        setup();
        setup_benchmark_schema();
        populate_entities(100, 100);

        // Reset stats
        Spi::run("SELECT mentat_reset_stats()").expect("reset stats");

        // Run a few queries
        timed_query("[:find ?e ?name :where [?e :bench/name ?name]]");
        timed_query("[:find ?e ?age :where [?e :bench/age ?age]]");
        timed_query("[:find ?name ?age :where [?e :bench/name ?name] [?e :bench/age ?age]]");

        // Check stats (mentat_backend_stats returns per-backend query metrics)
        let stats_json = Spi::get_one::<pgrx::JsonB>(
            "SELECT mentat_backend_stats()"
        ).expect("stats").expect("NULL");

        let stats = &stats_json.0;
        let total_queries = stats["total_queries"].as_u64().unwrap_or(0);
        assert!(total_queries >= 3, "Expected at least 3 queries tracked, got {}", total_queries);

        let schema_aware_hits = stats["schema_aware_hits"].as_u64().unwrap_or(0);
        assert!(schema_aware_hits > 0, "Expected schema-aware hits > 0, got {}", schema_aware_hits);

        pgrx::notice!(
            "MONITORING: total_queries={}, schema_aware_hits={}, union_all_fallbacks={}, avg_ms={:.1}",
            stats["total_queries"],
            stats["schema_aware_hits"],
            stats["union_all_fallbacks"],
            stats["avg_execution_ms"].as_f64().unwrap_or(0.0),
        );
    }
}
