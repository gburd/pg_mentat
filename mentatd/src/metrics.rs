use lazy_static::lazy_static;
use prometheus::{
    Encoder, Gauge, Histogram, HistogramOpts, IntCounter, Registry, TextEncoder,
};

lazy_static! {
    pub static ref REGISTRY: Registry = Registry::new();
    pub static ref QUERY_COUNT: IntCounter =
        IntCounter::new("mentatd_query_total", "Total number of queries executed")
            .expect("metric can be created");
    pub static ref QUERY_DURATION: Histogram = Histogram::with_opts(
        HistogramOpts::new("mentatd_query_duration_seconds", "Query execution duration in seconds")
            .buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0])
    )
    .expect("metric can be created");
    pub static ref CACHE_HITS: IntCounter =
        IntCounter::new("mentatd_cache_hits_total", "Total number of query cache hits")
            .expect("metric can be created");
    pub static ref CACHE_MISSES: IntCounter =
        IntCounter::new("mentatd_cache_misses_total", "Total number of query cache misses")
            .expect("metric can be created");
    pub static ref TRANSACTION_COUNT: IntCounter =
        IntCounter::new("mentatd_transactions_total", "Total number of transactions executed")
            .expect("metric can be created");
    pub static ref CONNECTION_POOL_SIZE: Gauge = Gauge::new(
        "mentatd_connection_pool_size",
        "Current number of connections in the pool"
    )
    .expect("metric can be created");
    pub static ref REQUEST_COUNT: IntCounter =
        IntCounter::new("mentatd_requests_total", "Total number of HTTP requests received")
            .expect("metric can be created");
    pub static ref ERROR_COUNT: IntCounter =
        IntCounter::new("mentatd_errors_total", "Total number of errors")
            .expect("metric can be created");
}

/// Register all metrics with the global registry. Call once at startup.
pub fn register_metrics() {
    REGISTRY
        .register(Box::new(QUERY_COUNT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(QUERY_DURATION.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(CACHE_HITS.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(CACHE_MISSES.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(TRANSACTION_COUNT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(CONNECTION_POOL_SIZE.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(REQUEST_COUNT.clone()))
        .expect("collector can be registered");
    REGISTRY
        .register(Box::new(ERROR_COUNT.clone()))
        .expect("collector can be registered");
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
}
