/**
 * Spike Load Test
 *
 * Ramps traffic from 10 TPS to 100 TPS over three phases to test how
 * mentatd handles sudden load increases. Validates that the server
 * maintains acceptable latency under rapidly increasing traffic.
 *
 * Phases:
 *   1. Low    - 10 TPS  for 1/3 of duration (baseline)
 *   2. Medium - 50 TPS  for 1/3 of duration (ramp)
 *   3. High   - 100 TPS for 1/3 of duration (spike)
 *
 * Performance targets:
 *   - p99 latency < 100ms during all phases
 *   - Error rate < 0.1% overall
 *   - Graceful degradation (no connection failures)
 *
 * Usage:
 *   k6 run benchmarks/scenarios/spike.js
 *   k6 run -e BASE_URL=http://localhost:8080 -e PHASE_DURATION=60s benchmarks/scenarios/spike.js
 */

import http from "k6/http";
import { check, sleep } from "k6";
import { Rate, Trend, Counter } from "k6/metrics";

const BASE_URL = __ENV.BASE_URL || "http://127.0.0.1:8080";
const PHASE_DURATION = __ENV.PHASE_DURATION || "1m";

// Custom metrics per phase
const errorRate = new Rate("mentat_errors");
const queryLatency = new Trend("mentat_query_latency", true);
const phaseRequests = new Counter("mentat_phase_requests");

export const options = {
  scenarios: {
    spike: {
      executor: "ramping-arrival-rate",
      startRate: 10,
      timeUnit: "1s",
      preAllocatedVUs: 50,
      maxVUs: 200,
      stages: [
        { target: 10, duration: PHASE_DURATION },   // Low: hold 10 TPS
        { target: 100, duration: PHASE_DURATION },   // Ramp: 10 -> 100 TPS
        { target: 100, duration: PHASE_DURATION },   // High: hold 100 TPS
      ],
    },
  },
  thresholds: {
    http_req_duration: [
      "p(99)<100",
      "p(95)<75",
    ],
    mentat_errors: ["rate<0.001"],
    http_req_failed: ["rate<0.001"],
  },
};

const headers = { "Content-Type": "application/edn" };

const QUERY = '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}';

export function setup() {
  const healthRes = http.get(`${BASE_URL}/health`);
  check(healthRes, {
    "server is healthy": (r) => r.status === 200,
  });

  // Install schema
  const schemaTx =
    '[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one} ' +
    '{:db/ident :person/age :db/valueType :db.type/long :db/cardinality :db.cardinality/one}]';

  http.post(`${BASE_URL}/`, `{:op :transact :args {:connection-id "bench" :tx-data ${schemaTx}}}`, {
    headers,
  });

  // Seed test data
  for (let i = 1; i <= 100; i++) {
    const txData = `[{:person/name "Person_${i}" :person/age ${20 + (i % 60)}}]`;
    http.post(
      `${BASE_URL}/`,
      `{:op :transact :args {:connection-id "bench" :tx-data ${txData}}}`,
      { headers }
    );
  }

  return { baseUrl: BASE_URL };
}

export default function (data) {
  const res = http.post(`${data.baseUrl}/`, QUERY, { headers });

  const passed = check(res, {
    "status is 200": (r) => r.status === 200,
    "response contains result": (r) => r.body && r.body.includes(":result"),
  });

  errorRate.add(!passed);
  queryLatency.add(res.timings.duration);
  phaseRequests.add(1);
}
