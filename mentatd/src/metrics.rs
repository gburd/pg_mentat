use lazy_static::lazy_static;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, HistogramVec, IntCounter, IntCounterVec, IntGauge,
    Opts, Registry, TextEncoder,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();

    // -- Request-level metrics --------------------------------------------------
    pub static ref REQUEST_COUNT: IntCounter =
        IntCounter::new("mentatd_requests_total", "Total number of HTTP requests received")
            .expect("metric can be created");
    pub static ref ERROR_COUNT: IntCounter =
        IntCounter::new("mentatd_errors_total", "Total number of errors")
            .expect("metric can be created");

    // -- Query metrics ----------------------------------------------------------
    pub static ref QUERY_COUNT: IntCounter =
        IntCounter::new("mentatd_query_total", "Total number of queries executed")
            .expect("metric can be created");
    pub static ref QUERY_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mentatd_query_duration_seconds", "Query execution duration in seconds")
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
    )
    .expect("metric can be created");

    // -- Per-operation duration histograms --------------------------------------
    pub static ref OPERATION_DURATION: HistogramVec = HistogramVec::new(
        HistogramOpts::new(
            "mentatd_operation_duration_seconds",
            "Duration of operations by type"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]),
        &["operation"]
    )
    .expect("metric can be created");

    // -- Per-operation counters -------------------------------------------------
    pub static ref OPERATION_COUNT: IntCounterVec = IntCounterVec::new(
        Opts::new("mentatd_operations_total", "Total operations by type"),
        &["operation"]
    )
    .expect("metric can be created");

    // -- Cache metrics ----------------------------------------------------------
    pub static ref CACHE_HITS: IntCounter =
        IntCounter::new("mentatd_cache_hits_total", "Total number of query cache hits")
            .expect("metric can be created");
    pub static ref CACHE_MISSES: IntCounter =
        IntCounter::new("mentatd_cache_misses_total", "Total number of query cache misses")
            .expect("metric can be created");
    pub static ref CACHE_SIZE: IntGauge =
        IntGauge::new("mentatd_cache_entries", "Current number of entries in the query cache")
            .expect("metric can be created");
    pub static ref CACHE_TARGETED_INVALIDATIONS: IntCounter =
        IntCounter::new(
            "mentatd_cache_targeted_invalidations_total",
            "Total entity-level cache invalidations"
        )
        .expect("metric can be created");
    pub static ref CACHE_FULL_INVALIDATIONS: IntCounter =
        IntCounter::new(
            "mentatd_cache_full_invalidations_total",
            "Total full cache invalidations"
        )
        .expect("metric can be created");
    pub static ref CACHE_TRACKED_ENTRIES: IntGauge =
        IntGauge::new(
            "mentatd_cache_tracked_entries",
            "Number of cache entries with entity dependency tracking"
        )
        .expect("metric can be created");
    pub static ref CACHE_HIT_RATE: Gauge =
        Gauge::new("mentatd_cache_hit_rate", "Query cache hit rate")
            .expect("metric can be created");
    pub static ref CACHE_AVG_DEPS: Gauge =
        Gauge::new(
            "mentatd_cache_avg_dependency_count",
            "Average number of entity dependencies per tracked cache entry"
        )
        .expect("metric can be created");

    // -- Transaction metrics ----------------------------------------------------
    pub static ref TRANSACTION_COUNT: IntCounter =
        IntCounter::new("mentatd_transactions_total", "Total number of transactions executed")
            .expect("metric can be created");
    pub static ref TRANSACTION_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new(
            "mentatd_transaction_duration_seconds",
            "Transaction execution duration in seconds"
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
    )
    .expect("metric can be created");

    // -- Conflict / retry metrics -----------------------------------------------
    pub static ref TRANSACTION_CONFLICTS: IntCounter =
        IntCounter::new(
            "mentatd_transaction_conflicts_total",
            "Total serialization conflicts (SQLSTATE 40001) encountered"
        )
        .expect("metric can be created");
    pub static ref TRANSACTION_RETRIES: IntCounter =
        IntCounter::new(
            "mentatd_transaction_retries_total",
            "Total transaction retry attempts after serialization conflicts"
        )
        .expect("metric can be created");
    pub static ref TRANSACTION_RETRY_EXHAUSTED: IntCounter =
        IntCounter::new(
            "mentatd_transaction_retry_exhausted_total",
            "Transactions that failed after exhausting all retry attempts"
        )
        .expect("metric can be created");
    pub static ref TRANSACTION_UNIQUE_VIOLATIONS: IntCounter =
        IntCounter::new(
            "mentatd_transaction_unique_violations_total",
            "Total unique constraint violations (SQLSTATE 23505) encountered"
        )
        .expect("metric can be created");

    // -- Connection pool metrics ------------------------------------------------
    pub static ref CONNECTION_POOL_SIZE: Gauge = Gauge::new(
        "mentatd_connection_pool_size",
        "Current number of connections in the pool"
    )
    .expect("metric can be created");
    pub static ref CONNECTION_POOL_AVAILABLE: IntGauge = IntGauge::new(
        "mentatd_connection_pool_available",
        "Number of idle connections available in the pool"
    )
    .expect("metric can be created");
    pub static ref CONNECTION_POOL_WAITING: IntGauge = IntGauge::new(
        "mentatd_connection_pool_waiting",
        "Number of tasks waiting for a connection from the pool"
    )
    .expect("metric can be created");

    // -- Streaming metrics ------------------------------------------------------
    pub static ref STREAM_QUERY_COUNT: IntCounter =
        IntCounter::new("mentatd_stream_queries_total", "Total number of streaming queries")
            .expect("metric can be created");
    pub static ref STREAM_ROWS_SENT: IntCounter =
        IntCounter::new("mentatd_stream_rows_sent_total", "Total number of rows sent via streaming")
            .expect("metric can be created");
    pub static ref STREAM_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mentatd_stream_duration_seconds", "Streaming query duration in seconds")
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0])
    )
    .expect("metric can be created");
}

/// Register all metrics with the global registry. Call once at startup.
pub fn register_metrics() {
    let collectors: Vec<Box<dyn prometheus::core::Collector>> = vec![
        Box::new(REQUEST_COUNT.clone()),
        Box::new(ERROR_COUNT.clone()),
        Box::new(QUERY_COUNT.clone()),
        Box::new(QUERY_DURATION.clone()),
        Box::new(OPERATION_DURATION.clone()),
        Box::new(OPERATION_COUNT.clone()),
        Box::new(CACHE_HITS.clone()),
        Box::new(CACHE_MISSES.clone()),
        Box::new(CACHE_SIZE.clone()),
        Box::new(CACHE_TARGETED_INVALIDATIONS.clone()),
        Box::new(CACHE_FULL_INVALIDATIONS.clone()),
        Box::new(CACHE_TRACKED_ENTRIES.clone()),
        Box::new(CACHE_HIT_RATE.clone()),
        Box::new(CACHE_AVG_DEPS.clone()),
        Box::new(TRANSACTION_COUNT.clone()),
        Box::new(TRANSACTION_DURATION.clone()),
        Box::new(TRANSACTION_CONFLICTS.clone()),
        Box::new(TRANSACTION_RETRIES.clone()),
        Box::new(TRANSACTION_RETRY_EXHAUSTED.clone()),
        Box::new(TRANSACTION_UNIQUE_VIOLATIONS.clone()),
        Box::new(CONNECTION_POOL_SIZE.clone()),
        Box::new(CONNECTION_POOL_AVAILABLE.clone()),
        Box::new(CONNECTION_POOL_WAITING.clone()),
        Box::new(STREAM_QUERY_COUNT.clone()),
        Box::new(STREAM_ROWS_SENT.clone()),
        Box::new(STREAM_DURATION.clone()),
    ];

    for collector in collectors {
        REGISTRY
            .register(collector)
            .expect("collector can be registered");
    }
}

/// Render all registered metrics in Prometheus text exposition format.
pub fn render_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    encoder
        .encode(&metric_families, &mut buffer)
        .expect("encoding metrics should not fail");
    String::from_utf8(buffer).unwrap_or_default()
}

/// Record an operation's duration and increment its counter.
pub fn observe_operation(operation: &str, duration_secs: f64) {
    OPERATION_DURATION
        .with_label_values(&[operation])
        .observe(duration_secs);
    OPERATION_COUNT.with_label_values(&[operation]).inc();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_render_metrics() {
        // Create a separate registry for testing to avoid conflicts
        let registry = Registry::new();
        let counter =
            IntCounter::new("test_counter", "A test counter").expect("metric can be created");
        registry
            .register(Box::new(counter.clone()))
            .expect("collector can be registered");

        counter.inc();

        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("encoding should succeed");
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("test_counter"));
        assert!(output.contains("1"));
    }

    #[test]
    fn test_histogram_observation() {
        let registry = Registry::new();
        let histogram = Histogram::with_opts(
            HistogramOpts::new("test_hist", "A test histogram").buckets(vec![0.1, 0.5, 1.0]),
        )
        .expect("metric can be created");
        registry
            .register(Box::new(histogram.clone()))
            .expect("collector can be registered");

        histogram.observe(0.25);
        histogram.observe(0.75);

        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("encoding should succeed");
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("test_hist"));
        assert!(output.contains("_count 2"));
    }

    #[test]
    fn test_operation_duration_histogram_vec() {
        let registry = Registry::new();
        let hv = HistogramVec::new(
            HistogramOpts::new("test_op_duration", "op duration").buckets(vec![0.01, 0.1, 1.0]),
            &["operation"],
        )
        .expect("metric can be created");
        registry
            .register(Box::new(hv.clone()))
            .expect("collector can be registered");

        hv.with_label_values(&["query"]).observe(0.05);
        hv.with_label_values(&["transact"]).observe(0.2);
        hv.with_label_values(&["query"]).observe(0.08);

        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("encoding should succeed");
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("test_op_duration"));
        assert!(output.contains(r#"operation="query""#));
        assert!(output.contains(r#"operation="transact""#));
    }

    #[test]
    fn test_operation_counter_vec() {
        let registry = Registry::new();
        let cv = IntCounterVec::new(Opts::new("test_op_count", "op count"), &["operation"])
            .expect("metric can be created");
        registry
            .register(Box::new(cv.clone()))
            .expect("collector can be registered");

        cv.with_label_values(&["query"]).inc();
        cv.with_label_values(&["query"]).inc();
        cv.with_label_values(&["transact"]).inc();

        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("encoding should succeed");
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("test_op_count"));
        assert!(output.contains(r#"operation="query""#));
        assert!(output.contains(r#"operation="transact""#));
    }

    #[test]
    fn test_observe_operation_helper() {
        // Verify the helper does not panic and label values are accepted.
        observe_operation("query", 0.05);
        observe_operation("transact", 0.1);
        observe_operation("pull", 0.01);
        observe_operation("datoms", 0.02);
    }

    #[test]
    fn test_cache_size_gauge() {
        let registry = Registry::new();
        let gauge = IntGauge::new("test_cache_size", "cache size").expect("metric can be created");
        registry
            .register(Box::new(gauge.clone()))
            .expect("collector can be registered");

        gauge.set(42);

        let encoder = TextEncoder::new();
        let metric_families = registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("encoding should succeed");
        let output = String::from_utf8(buffer).unwrap_or_default();

        assert!(output.contains("test_cache_size"));
        assert!(output.contains("42"));
    }
}
