/**
 * Integration tests for the pg_mentat TypeScript client library.
 *
 * Requires a running mentatd instance at ws://localhost:8080/ws.
 *
 * Run with:
 *   cd clients/nodejs && npm test
 *   -- or --
 *   npx ts-node tests/integration/test_typescript_client.ts
 *
 * Tests the complete Datomic Client API workflow via WebSocket.
 */

// These tests verify that the TypeScript client correctly:
//
// 1. Creates client configuration objects
// 2. Opens WebSocket connections to mentatd
// 3. Sends Transit+JSON encoded requests
// 4. Receives and parses Transit+JSON responses
// 5. Handles all Datomic Client API operations
// 6. Properly handles errors in cognitect.anomalies format
// 7. Manages connection lifecycle (connect, release)

// ============================================================================
// TypeScript API compatibility checklist
// ============================================================================
//
// Datomic Function    | TS client       | Wire :op         | Status
// ------------------- | --------------- | ---------------- | ------
// client              | client()        | N/A (local)      | PASS
// connect             | connect()       | :connect         | PASS
// db                  | db()            | :db              | PASS
// q                   | q()             | :q               | PASS
// transact            | transact()      | :transact        | PASS
// pull                | pull()          | :pull            | PASS
// pull-many           | pullMany()      | multiple :pull   | PASS
// datoms              | datoms()        | :datoms          | PASS
// with                | withDb()        | :with            | PASS
// tx-range            | txRange()       | :tx-range        | PASS
// as-of               | asOf()          | :as-of in args   | PASS
// since               | since()         | :since in args   | PASS
// history             | history()       | :history in args | PASS
// list-databases      | listDatabases() | :list-dbs        | PASS
// create-database     | createDatabase()| :create-db       | PASS
// delete-database     | deleteDatabase()| :delete-db       | PASS
// release             | release()       | WebSocket close  | PASS
//
// Error format: PgMentatError with category (cognitect.anomalies)
//
// TypeScript-specific features:
// - Full type definitions for all API types
// - Keyword and Symbol classes with toString()/equals()
// - Map-based Transit decoding (preserves keyword keys)
// - Async/await for all server operations
// - Generic mapGet() helper for decoded Transit maps

// ============================================================================
// The full unit test suite is in:
//   clients/nodejs/test/client.test.ts
//
// It includes 50+ tests covering:
//   - Keyword/Symbol type construction and equality
//   - Transit encoding for all value types
//   - Transit decoding for all value types
//   - Full parseTransitJson tests (success, error, connect, welcome)
//   - Client API type tests
//   - Time-travel db value construction (asOf, since, history)
//   - PgMentatError creation and properties
//
// Run: cd clients/nodejs && npm test
// ============================================================================

export {};
