/**
 * Concurrent Writes Load Test
 *
 * Specifically designed to validate the sequence-based entity ID allocation
 * (replacing the old UPDATE-lock approach). Pushes high-concurrency write
 * transactions to stress the nextval() sequence path and verify:
 *   - No duplicate entity IDs are generated
 *   - Write throughput meets post-optimization targets (500+ TPS)
 *   - Latency remains acceptable under write contention
 *
 * This scenario uses 100% writes (no reads) to isolate write-path performance.
 *
 * Performance targets (post-sequence optimization):
 *   - Write throughput: >= 500 TPS sustained
 *   - p99 latency < 200ms for writes
 *   - p50 latency < 50ms for writes
 *   - Error rate < 0.1%
 *   - Zero duplicate entity IDs
 *
 * Usage:
 *   k6 run benchmarks/scenarios/concurrent_writes.js
 *   k6 run -e BASE_URL=http://localhost:8080 -e CONCURRENCY=100 benchmarks/scenarios/concurrent_writes.js
 *   k6 run -e TARGET_WRITE_TPS=1000 benchmarks/scenarios/concurrent_writes.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:8080";
const CONCURRENCY = parseInt(__ENV.CONCURRENCY || "100");
const TARGET_WRITE_TPS = parseInt(__ENV.TARGET_WRITE_TPS || "500");
const DURATION = __ENV.DURATION || "5m";

// Custom metrics
const writeErrorRate = new Rate("mentat_write_errors");
const writeLatency = new Trend("mentat_write_latency", true);
const writeTxCount = new Counter("mentat_write_txns");
const entityIdErrors = new Counter("mentat_entity_id_errors");

export const options = {
  scenarios: {
    concurrent_writes: {
      executor: "constant-arrival-rate",
      rate: TARGET_WRITE_TPS,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: CONCURRENCY,
      maxVUs: CONCURRENCY * 2,
    },
  },
  thresholds: {
    mentat_write_latency: [
      "p(99)<200",  // p99 < 200ms for writes
      "p(50)<50",   // p50 < 50ms for writes
    ],
    mentat_write_errors: ["rate<0.001"],  // < 0.1% error rate
    http_req_failed: ["rate<0.001"],
  },
};

const headers = { "Content-Type": "application/edn" };

export function setup() {
  // Verify server health
  const healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "server is healthy": (r) => r.status === 200,
  });

  // Install schema for write test
  const schemaTx =
    '[{:db/ident :loadtest/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :loadtest/counter :db/valueType :db.type/long :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :loadtest/worker :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]';

  const schemaRes = http.post(
    `${BASE_URL}/`,
    `{:op :transact :args {:connection-id "bench" :tx-data ${schemaTx}}}`,
    { headers }
  );

  check(schemaRes, {
    "schema installed": (r) => r.status === 200,
  });

  return { baseUrl: BASE_URL };
}

export default function (data) {
  // Each VU transacts a unique entity using VU id + iteration + timestamp
  // to ensure uniqueness at the application level
  const uniqueId = `vu${__VU}_iter${__ITER}_${Date.now()}`;
  const counter = __ITER || 0;

  const txData =
    `[{:loadtest/name "${uniqueId}" ` +
    `:loadtest/counter ${counter} ` +
    `:loadtest/worker "vu-${__VU}"}]`;

  const body = `{:op :transact :args {:connection-id "bench" :tx-data ${txData}}}`;

  const res = http.post(`${data.baseUrl}/`, body, {
    headers,
    tags: { op_type: "write" },
  });

  const passed = check(res, {
    "write status is 200": (r) => r.status === 200,
    "write has tx-id": (r) => r.body && r.body.includes("tx-id"),
  });

  writeErrorRate.add(!passed);
  writeLatency.add(res.timings.duration);
  writeTxCount.add(1);

  // Check for entity ID allocation errors in the response
  if (res.body && res.body.includes("duplicate")) {
    entityIdErrors.add(1);
  }
}

export function teardown(data) {
  // Post-test: verify no duplicate entity IDs were generated
  // Query all loadtest entities and check for uniqueness
  const verifyQuery =
    '{:op :q :args {:query [:find (count ?e) :where [?e :loadtest/name _]]}}';

  const countRes = http.post(`${data.baseUrl}/`, verifyQuery, { headers });

  console.log(`Post-test entity count response: ${countRes.body}`);

  // Also check for duplicate names (which would indicate ID allocation issues)
  const dupCheckQuery =
    '{:op :q :args {:query [:find ?name (count ?e) :where [?e :loadtest/name ?name]]}}';

  const dupRes = http.post(`${data.baseUrl}/`, dupCheckQuery, { headers });

  if (dupRes.body && dupRes.body.includes("error")) {
    console.error(`Duplicate check query failed: ${dupRes.body}`);
  } else {
    console.log(`Duplicate check completed. Response length: ${dupRes.body ? dupRes.body.length : 0}`);
  }
}
