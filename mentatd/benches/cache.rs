//! Criterion benchmarks for mentatd query cache.
//!
//! Measures cache hit/miss/insert/invalidation performance under various
//! conditions. These run without a database.
//!
//! Run with: cargo bench -p mentatd --bench cache

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use mentatd::cache::QueryCache;
use std::time::Duration;

fn make_query(i: usize) -> String {
    format!("[:find ?e :where [?e :name \"Person_{}\"]]", i)
}

fn make_result(i: usize) -> String {
    format!(r#"{{"columns":["?e"],"results":[[{}]]}}"#, 10000 + i)
}

// ---------------------------------------------------------------------------
// Cache lookup benchmarks
// ---------------------------------------------------------------------------

fn bench_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_hit");

    for capacity in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("lookup", capacity),
            &capacity,
            |b, &capacity| {
                let cache = QueryCache::new(capacity, Duration::from_secs(300));
                // Pre-populate
                for i in 0..capacity {
                    cache.insert(&make_query(i), "[]", make_result(i));
                }
                // Benchmark: look up a known key
                let query = make_query(capacity / 2);
                b.iter(|| cache.get(black_box(&query), black_box("[]")))
            },
        );
    }

    group.finish();
}

fn bench_cache_miss(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_miss");

    for capacity in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("lookup", capacity),
            &capacity,
            |b, &capacity| {
                let cache = QueryCache::new(capacity, Duration::from_secs(300));
                // Pre-populate with different keys
                for i in 0..capacity {
                    cache.insert(&make_query(i), "[]", make_result(i));
                }
                // Benchmark: look up a key that does not exist
                let query = make_query(capacity + 999);
                b.iter(|| cache.get(black_box(&query), black_box("[]")))
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Cache insert benchmarks
// ---------------------------------------------------------------------------

fn bench_cache_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_insert");

    for capacity in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("insert_new", capacity),
            &capacity,
            |b, &capacity| {
                let cache = QueryCache::new(capacity, Duration::from_secs(300));
                let mut counter = 0usize;
                b.iter(|| {
                    let q = make_query(counter % (capacity * 2));
                    let r = make_result(counter);
                    cache.insert(black_box(&q), black_box("[]"), r);
                    counter += 1;
                })
            },
        );
    }

    group.finish();
}

fn bench_cache_insert_with_eviction(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_insert_eviction");

    // Small cache that will constantly evict
    let capacity = 100;
    group.bench_function("evict_at_100", |b| {
        let cache = QueryCache::new(capacity, Duration::from_secs(300));
        // Fill the cache
        for i in 0..capacity {
            cache.insert(&make_query(i), "[]", make_result(i));
        }
        let mut counter = capacity;
        b.iter(|| {
            let q = make_query(counter);
            let r = make_result(counter);
            cache.insert(black_box(&q), black_box("[]"), r);
            counter += 1;
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Cache invalidation benchmarks
// ---------------------------------------------------------------------------

fn bench_cache_invalidate(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_invalidate");

    for capacity in [100, 1000, 10000] {
        group.bench_with_input(
            BenchmarkId::new("clear", capacity),
            &capacity,
            |b, &capacity| {
                let cache = QueryCache::new(capacity, Duration::from_secs(300));
                b.iter(|| {
                    // Re-populate and then invalidate
                    for i in 0..capacity {
                        cache.insert(&make_query(i), "[]", make_result(i));
                    }
                    cache.invalidate();
                })
            },
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Cache with varying argument combinations
// ---------------------------------------------------------------------------

fn bench_cache_varying_args(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_varying_args");

    group.bench_function("same_query_different_args", |b| {
        let cache = QueryCache::new(1000, Duration::from_secs(300));
        let query = "[:find ?e :in $ ?name :where [?e :name ?name]]";
        // Pre-populate with different args
        for i in 0..100 {
            let args = format!(r#"["Person_{}"]"#, i);
            cache.insert(query, &args, make_result(i));
        }
        let mut counter = 0usize;
        b.iter(|| {
            let args = format!(r#"["Person_{}"]"#, counter % 100);
            cache.get(black_box(query), black_box(&args));
            counter += 1;
        })
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Disabled cache (capacity=0) overhead
// ---------------------------------------------------------------------------

fn bench_cache_disabled(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_disabled");

    group.bench_function("get_noop", |b| {
        let cache = QueryCache::new(0, Duration::from_secs(300));
        b.iter(|| cache.get(black_box("query"), black_box("[]")))
    });

    group.bench_function("insert_noop", |b| {
        let cache = QueryCache::new(0, Duration::from_secs(300));
        b.iter(|| cache.insert(black_box("query"), black_box("[]"), "result".to_string()))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Register all benchmark groups
// ---------------------------------------------------------------------------

criterion_group!(
    cache_benches,
    bench_cache_hit,
    bench_cache_miss,
    bench_cache_insert,
    bench_cache_insert_with_eviction,
    bench_cache_invalidate,
    bench_cache_varying_args,
    bench_cache_disabled,
);

criterion_main!(cache_benches);
