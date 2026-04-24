/**
 * Mixed Workload Load Test
 *
 * Simulates a realistic production workload with 80% read queries
 * and 20% write transactions. Validates that writes do not degrade
 * read latency excessively and that the server maintains throughput
 * targets under mixed I/O patterns.
 *
 * Performance targets:
 *   - 50 TPS sustained (combined read + write)
 *   - p99 latency < 100ms for reads
 *   - Error rate < 0.1%
 *   - Write throughput: >= 10K datoms/sec
 *
 * Usage:
 *   k6 run benchmarks/scenarios/mixed_workload.js
 *   k6 run -e BASE_URL=http://localhost:8080 benchmarks/scenarios/mixed_workload.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:8080";
const TARGET_TPS = parseInt(__ENV.TARGET_TPS || "50");
const DURATION = __ENV.DURATION || "10m";
const WRITE_RATIO = parseFloat(__ENV.WRITE_RATIO || "0.2"); // 20% writes

// Custom metrics
const readErrorRate = new Rate("mentat_read_errors");
const writeErrorRate = new Rate("mentat_write_errors");
const readLatency = new Trend("mentat_read_latency", true);
const writeLatency = new Trend("mentat_write_latency", true);
const readCount = new Counter("mentat_reads");
const writeCount = new Counter("mentat_writes");

export const options = {
  scenarios: {
    mixed: {
      executor: "constant-arrival-rate",
      rate: TARGET_TPS,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 50,
      maxVUs: 150,
    },
  },
  thresholds: {
    mentat_read_latency: [
      "p(99)<100",
      "p(50)<50",
    ],
    mentat_read_errors: ["rate<0.001"],
    mentat_write_errors: ["rate<0.01"],  // Allow slightly higher error rate for writes
    http_req_failed: ["rate<0.001"],
  },
};

const headers = { "Content-Type": "application/edn" };

const READ_QUERIES = [
  '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}',
  '{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}',
  '{:op :q :args {:query [:find ?name :where [?e :person/name ?name]]}}',
];

export function setup() {
  const healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "server is healthy": (r) => r.status === 200,
  });

  const schemaTx =
    '[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :person/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]';

  http.post(`${BASE_URL}/`, `{:op :transact :args {:connection-id "bench" :tx-data ${schemaTx}}}`, {
    headers,
  });

  // Seed initial data
  for (let i = 1; i <= 100; i++) {
    const txData = `[{:person/name "Person_${i}" :person/age ${20 + (i % 60)} :person/email "person${i}@example.com"}]`;
    http.post(
      `${BASE_URL}/`,
      `{:op :transact :args {:connection-id "bench" :tx-data ${txData}}}`,
      { headers }
    );
  }

  return { baseUrl: BASE_URL };
}

export default function (data) {
  if (Math.random() < WRITE_RATIO) {
    // Write path (20% of requests)
    doWrite(data);
  } else {
    // Read path (80% of requests)
    doRead(data);
  }
}

function doRead(data) {
  const query = READ_QUERIES[Math.floor(Math.random() * READ_QUERIES.length)];
  const res = http.post(`${data.baseUrl}/`, query, {
    headers,
    tags: { op_type: "read" },
  });

  const passed = check(res, {
    "read status is 200": (r) => r.status === 200,
    "read response has result": (r) => r.body && r.body.includes(":result"),
  });

  readErrorRate.add(!passed);
  readLatency.add(res.timings.duration);
  readCount.add(1);
}

function doWrite(data) {
  const id = `${__VU}_${__ITER}_${Date.now()}`;
  const age = 20 + ((__ITER || 0) % 60);
  const txData = `[{:person/name "LoadTest_${id}" :person/age ${age} :person/email "lt_${id}@test.com"}]`;
  const body = `{:op :transact :args {:connection-id "bench" :tx-data ${txData}}}`;

  const res = http.post(`${data.baseUrl}/`, body, {
    headers,
    tags: { op_type: "write" },
  });

  const passed = check(res, {
    "write status is 200": (r) => r.status === 200,
    "write response has tx-id": (r) => r.body && r.body.includes(":tx-id"),
  });

  writeErrorRate.add(!passed);
  writeLatency.add(res.timings.duration);
  writeCount.add(1);
}
