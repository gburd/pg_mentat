/**
 * Large Queries Load Test
 *
 * Sends complex queries that return large result sets to test mentatd's
 * ability to serialize and transmit large responses under concurrent load.
 * Uses 10 concurrent workers cycling through queries that exercise
 * different code paths (multi-attribute joins, full scans).
 *
 * Performance targets:
 *   - Simple queries: < 50ms p50
 *   - Complex queries (multi-join): < 500ms p99
 *   - Error rate < 0.1%
 *
 * Usage:
 *   k6 run benchmarks/scenarios/large_queries.js
 *   k6 run -e CONCURRENCY=10 -e DURATION=5m benchmarks/scenarios/large_queries.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:8080";
const CONCURRENCY = parseInt(__ENV.CONCURRENCY || "10");
const DURATION = __ENV.DURATION || "5m";

// Custom metrics
const errorRate = new Rate("mentat_errors");
const queryLatency = new Trend("mentat_query_latency", true);
const responseSizeBytes = new Trend("mentat_response_size");
const queriesPerPattern = new Counter("mentat_queries_per_pattern");

export const options = {
  scenarios: {
    large_queries: {
      executor: "constant-vus",
      vus: CONCURRENCY,
      duration: DURATION,
    },
  },
  thresholds: {
    http_req_duration: [
      "p(99)<500",  // Complex queries < 500ms
      "p(50)<50",   // Simple queries < 50ms
    ],
    mentat_errors: ["rate<0.001"],
    http_req_failed: ["rate<0.001"],
  },
};

const headers = { "Content-Type": "application/edn" };

// Variety of query patterns from simple to complex
const QUERY_PATTERNS = [
  {
    name: "all_names",
    body: '{:op :q :args {:query [:find ?name :where [?e :person/name ?name]]}}',
  },
  {
    name: "name_and_age",
    body: '{:op :q :args {:query [:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]}}',
  },
  {
    name: "full_entity",
    body: '{:op :q :args {:query [:find ?e ?name ?age ?email :where [?e :person/name ?name] [?e :person/age ?age] [?e :person/email ?email]]}}',
  },
  {
    name: "entity_ids_only",
    body: '{:op :q :args {:query [:find ?e :where [?e :person/name _]]}}',
  },
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

  // Insert enough data to generate large result sets
  // Batch in groups of 10 for efficiency
  for (let batch = 0; batch < 100; batch++) {
    let txItems = [];
    for (let i = 0; i < 10; i++) {
      const idx = batch * 10 + i + 1;
      txItems.push(
        `{:person/name "Person_${idx}" :person/age ${20 + (idx % 60)} :person/email "person${idx}@example.com"}`
      );
    }
    const txData = `[${txItems.join(" ")}]`;
    http.post(
      `${BASE_URL}/`,
      `{:op :transact :args {:connection-id "bench" :tx-data ${txData}}}`,
      { headers }
    );
  }

  return { baseUrl: BASE_URL };
}

export default function (data) {
  // Cycle through query patterns based on iteration count
  const pattern = QUERY_PATTERNS[__ITER % QUERY_PATTERNS.length];

  const res = http.post(`${data.baseUrl}/`, pattern.body, {
    headers,
    tags: { query_pattern: pattern.name },
  });

  const passed = check(res, {
    "status is 200": (r) => r.status === 200,
    "response contains result": (r) => r.body && r.body.includes(":result"),
    "response is non-trivial": (r) => r.body && r.body.length > 20,
  });

  errorRate.add(!passed);
  queryLatency.add(res.timings.duration);
  responseSizeBytes.add(res.body ? res.body.length : 0);
  queriesPerPattern.add(1, { pattern: pattern.name });
}
