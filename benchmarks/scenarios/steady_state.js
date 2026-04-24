/**
 * Steady State Load Test
 *
 * Maintains a constant 50 TPS query load against mentatd for a sustained
 * duration. Validates that the server can maintain the target throughput
 * without latency degradation over time.
 *
 * Performance targets:
 *   - 50 TPS sustained
 *   - p99 latency < 100ms
 *   - p50 latency < 50ms
 *   - Error rate < 0.1%
 *
 * Usage:
 *   k6 run benchmarks/scenarios/steady_state.js
 *   k6 run --out json=results.json benchmarks/scenarios/steady_state.js
 *   k6 run -e BASE_URL=http://localhost:8080 benchmarks/scenarios/steady_state.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:8080";
const TARGET_TPS = parseInt(__ENV.TARGET_TPS || "50");
const DURATION = __ENV.DURATION || "10m";

// Custom metrics
const errorRate = new Rate("mentat_errors");
const queryLatency = new Trend("mentat_query_latency", true);

export const options = {
  scenarios: {
    steady_state: {
      executor: "constant-arrival-rate",
      rate: TARGET_TPS,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 50,
      maxVUs: 100,
    },
  },
  thresholds: {
    http_req_duration: [
      "p(99)<100", // p99 < 100ms
      "p(50)<50",  // p50 < 50ms
    ],
    mentat_errors: ["rate<0.001"], // Error rate < 0.1%
    http_req_failed: ["rate<0.001"],
  },
};

const headers = { "Content-Type": "application/edn" };

const QUERIES = [
  '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}',
  '{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}',
  '{:op :q :args {:query [:find ?name :where [?e :person/name ?name]]}}',
];

export function setup() {
  // Verify server is reachable
  const healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "server is healthy": (r) => r.status === 200,
  });

  // Install schema
  const schemaTx =
    '[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :person/email :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]';

  http.post(`${BASE_URL}/`, `{:op :transact :args {:connection-id "bench" :tx-data ${schemaTx}}}`, {
    headers,
  });

  // Seed test data
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
  const query = QUERIES[Math.floor(Math.random() * QUERIES.length)];

  const res = http.post(`${data.baseUrl}/`, query, { headers });

  const passed = check(res, {
    "status is 200": (r) => r.status === 200,
    "response contains result": (r) => r.body && r.body.includes(":result"),
  });

  errorRate.add(!passed);
  queryLatency.add(res.timings.duration);
}
