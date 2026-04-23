//! Criterion benchmarks for mentatd serialization formats.
//!
//! Benchmarks EDN, Transit+JSON, and Transit+MessagePack serialization
//! of various response types and sizes. These run without a database.
//!
//! Run with: cargo bench -p mentatd

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use mentatd::protocol::{
    serializer::serialize_response, transit_serializer::serialize_transit_json,
    transit_serializer::serialize_transit_msgpack, Anomaly, AnomalyCategory, Response,
    ResponseValue,
};

// ---------------------------------------------------------------------------
// Test data generators
// ---------------------------------------------------------------------------

fn make_simple_string() -> Response {
    Response::Success {
        result: ResponseValue::String("hello world".to_string()),
    }
}

fn make_integer() -> Response {
    Response::Success {
        result: ResponseValue::Integer(42),
    }
}

fn make_keyword() -> Response {
    Response::Success {
        result: ResponseValue::Keyword("db/name".to_string()),
    }
}

fn make_small_vector() -> Response {
    Response::Success {
        result: ResponseValue::Vector(vec![
            ResponseValue::Integer(10001),
            ResponseValue::String("Alice".to_string()),
            ResponseValue::Keyword("person/name".to_string()),
            ResponseValue::Boolean(true),
        ]),
    }
}

/// Simulates a typical query result: N rows of [entity-id, name, age].
fn make_query_result(num_rows: usize) -> Response {
    let rows: Vec<ResponseValue> = (0..num_rows)
        .map(|i| {
            ResponseValue::Vector(vec![
                ResponseValue::Integer(10000 + i as i64),
                ResponseValue::String(format!("Person_{}", i)),
                ResponseValue::Integer(20 + (i as i64 % 60)),
            ])
        })
        .collect();
    Response::Success {
        result: ResponseValue::Vector(rows),
    }
}

/// Simulates a transaction report response.
fn make_tx_report() -> Response {
    Response::Success {
        result: ResponseValue::Map(vec![
            (
                ResponseValue::Keyword("tx-id".to_string()),
                ResponseValue::Integer(123_456),
            ),
            (
                ResponseValue::Keyword("tx-instant".to_string()),
                ResponseValue::Nil,
            ),
            (
                ResponseValue::Keyword("tempids".to_string()),
                ResponseValue::Map(vec![
                    (
                        ResponseValue::Keyword("tempid-1".to_string()),
                        ResponseValue::Integer(200_001),
                    ),
                    (
                        ResponseValue::Keyword("tempid-2".to_string()),
                        ResponseValue::Integer(200_002),
                    ),
                    (
                        ResponseValue::Keyword("tempid-3".to_string()),
                        ResponseValue::Integer(200_003),
                    ),
                ]),
            ),
            (
                ResponseValue::Keyword("datoms-inserted".to_string()),
                ResponseValue::Integer(15),
            ),
            (
                ResponseValue::Keyword("status".to_string()),
                ResponseValue::String("committed".to_string()),
            ),
        ]),
    }
}

/// Simulates a connect response.
fn make_connect_response() -> Response {
    Response::Success {
        result: ResponseValue::Map(vec![
            (
                ResponseValue::Keyword("connection-id".to_string()),
                ResponseValue::String("550e8400-e29b-41d4-a716-446655440000".to_string()),
            ),
            (
                ResponseValue::Keyword("db-name".to_string()),
                ResponseValue::String("my_database".to_string()),
            ),
            (
                ResponseValue::Keyword("status".to_string()),
                ResponseValue::String("connected".to_string()),
            ),
        ]),
    }
}

fn make_error_response() -> Response {
    Response::Error {
        anomaly: Anomaly {
            category: AnomalyCategory::NotFound,
            message: "Database 'nonexistent' not found".to_string(),
            db_error: Some("db.error/not-found".to_string()),
        },
    }
}

/// Simulates a deeply nested result (e.g., pull results with sub-entities).
fn make_nested_map() -> Response {
    Response::Success {
        result: ResponseValue::Map(vec![
            (
                ResponseValue::Keyword("db/id".to_string()),
                ResponseValue::Integer(10001),
            ),
            (
                ResponseValue::Keyword("person/name".to_string()),
                ResponseValue::String("Alice".to_string()),
            ),
            (
                ResponseValue::Keyword("person/friends".to_string()),
                ResponseValue::Vector(vec![
                    ResponseValue::Map(vec![
                        (
                            ResponseValue::Keyword("db/id".to_string()),
                            ResponseValue::Integer(10002),
                        ),
                        (
                            ResponseValue::Keyword("person/name".to_string()),
                            ResponseValue::String("Bob".to_string()),
                        ),
                    ]),
                    ResponseValue::Map(vec![
                        (
                            ResponseValue::Keyword("db/id".to_string()),
                            ResponseValue::Integer(10003),
                        ),
                        (
                            ResponseValue::Keyword("person/name".to_string()),
                            ResponseValue::String("Carol".to_string()),
                        ),
                    ]),
                ]),
            ),
        ]),
    }
}

// ---------------------------------------------------------------------------
// Benchmarks: EDN serialization
// ---------------------------------------------------------------------------

fn bench_edn_simple_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("edn_simple");

    group.bench_function("string", |b| {
        let resp = make_simple_string();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("integer", |b| {
        let resp = make_integer();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("keyword", |b| {
        let resp = make_keyword();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("small_vector", |b| {
        let resp = make_small_vector();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("error", |b| {
        let resp = make_error_response();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.finish();
}

fn bench_edn_query_results(c: &mut Criterion) {
    let mut group = c.benchmark_group("edn_query_results");

    for size in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let resp = make_query_result(size);
            b.iter(|| serialize_response(black_box(&resp)))
        });
    }

    group.finish();
}

fn bench_edn_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("edn_complex");

    group.bench_function("tx_report", |b| {
        let resp = make_tx_report();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("connect_response", |b| {
        let resp = make_connect_response();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.bench_function("nested_map", |b| {
        let resp = make_nested_map();
        b.iter(|| serialize_response(black_box(&resp)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: Transit+JSON serialization
// ---------------------------------------------------------------------------

fn bench_transit_json_simple_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_json_simple");

    group.bench_function("string", |b| {
        let resp = make_simple_string();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("integer", |b| {
        let resp = make_integer();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("keyword", |b| {
        let resp = make_keyword();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("small_vector", |b| {
        let resp = make_small_vector();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("error", |b| {
        let resp = make_error_response();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.finish();
}

fn bench_transit_json_query_results(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_json_query_results");

    for size in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let resp = make_query_result(size);
            b.iter(|| serialize_transit_json(black_box(&resp)))
        });
    }

    group.finish();
}

fn bench_transit_json_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_json_complex");

    group.bench_function("tx_report", |b| {
        let resp = make_tx_report();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("connect_response", |b| {
        let resp = make_connect_response();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.bench_function("nested_map", |b| {
        let resp = make_nested_map();
        b.iter(|| serialize_transit_json(black_box(&resp)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: Transit+MessagePack serialization
// ---------------------------------------------------------------------------

fn bench_transit_msgpack_simple_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_msgpack_simple");

    group.bench_function("string", |b| {
        let resp = make_simple_string();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("integer", |b| {
        let resp = make_integer();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("keyword", |b| {
        let resp = make_keyword();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("small_vector", |b| {
        let resp = make_small_vector();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("error", |b| {
        let resp = make_error_response();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.finish();
}

fn bench_transit_msgpack_query_results(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_msgpack_query_results");

    for size in [1, 10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let resp = make_query_result(size);
            b.iter(|| serialize_transit_msgpack(black_box(&resp)))
        });
    }

    group.finish();
}

fn bench_transit_msgpack_complex(c: &mut Criterion) {
    let mut group = c.benchmark_group("transit_msgpack_complex");

    group.bench_function("tx_report", |b| {
        let resp = make_tx_report();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("connect_response", |b| {
        let resp = make_connect_response();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.bench_function("nested_map", |b| {
        let resp = make_nested_map();
        b.iter(|| serialize_transit_msgpack(black_box(&resp)))
    });

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: Format comparison (same data, all three formats)
// ---------------------------------------------------------------------------

fn bench_format_comparison(c: &mut Criterion) {
    let mut group = c.benchmark_group("format_comparison");

    let responses: Vec<(&str, Response)> = vec![
        ("simple_string", make_simple_string()),
        ("query_10_rows", make_query_result(10)),
        ("query_100_rows", make_query_result(100)),
        ("tx_report", make_tx_report()),
        ("nested_map", make_nested_map()),
        ("error", make_error_response()),
    ];

    for (name, resp) in &responses {
        group.bench_with_input(BenchmarkId::new("edn", name), resp, |b, resp| {
            b.iter(|| serialize_response(black_box(resp)))
        });

        group.bench_with_input(BenchmarkId::new("transit_json", name), resp, |b, resp| {
            b.iter(|| serialize_transit_json(black_box(resp)))
        });

        group.bench_with_input(
            BenchmarkId::new("transit_msgpack", name),
            resp,
            |b, resp| b.iter(|| serialize_transit_msgpack(black_box(resp))),
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Benchmarks: Output size comparison
// ---------------------------------------------------------------------------

fn bench_output_sizes(c: &mut Criterion) {
    let mut group = c.benchmark_group("output_size");

    // This benchmark measures serialization AND captures output size in the name.
    // The actual timing is secondary; the goal is documenting payload sizes.
    for size in [1, 10, 100, 1000] {
        let resp = make_query_result(size);

        let edn_size = serialize_response(&resp).len();
        let json_size = serialize_transit_json(&resp).len();
        let msgpack_size = serialize_transit_msgpack(&resp).len();

        group.bench_with_input(
            BenchmarkId::new(format!("edn_{}b", edn_size), size),
            &resp,
            |b, resp| b.iter(|| serialize_response(black_box(resp))),
        );

        group.bench_with_input(
            BenchmarkId::new(format!("transit_json_{}b", json_size), size),
            &resp,
            |b, resp| b.iter(|| serialize_transit_json(black_box(resp))),
        );

        group.bench_with_input(
            BenchmarkId::new(format!("transit_msgpack_{}b", msgpack_size), size),
            &resp,
            |b, resp| b.iter(|| serialize_transit_msgpack(black_box(resp))),
        );
    }

    group.finish();
}

// ---------------------------------------------------------------------------
// Register all benchmark groups
// ---------------------------------------------------------------------------

criterion_group!(
    edn_benches,
    bench_edn_simple_types,
    bench_edn_query_results,
    bench_edn_complex,
);

criterion_group!(
    transit_json_benches,
    bench_transit_json_simple_types,
    bench_transit_json_query_results,
    bench_transit_json_complex,
);

criterion_group!(
    transit_msgpack_benches,
    bench_transit_msgpack_simple_types,
    bench_transit_msgpack_query_results,
    bench_transit_msgpack_complex,
);

criterion_group!(
    comparison_benches,
    bench_format_comparison,
    bench_output_sizes,
);

criterion_main!(
    edn_benches,
    transit_json_benches,
    transit_msgpack_benches,
    comparison_benches,
);
