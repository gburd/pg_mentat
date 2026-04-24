# mentatd HTTP Protocol Reference

mentatd is a Datomic-compatible HTTP server that provides access to a pg_mentat
database over HTTP. It accepts EDN-encoded requests and returns responses in
EDN, Transit+JSON, or Transit+MessagePack formats.

---

## Server Configuration

mentatd is configured via a TOML file (`mentatd.toml`) or environment variables.

**TOML Configuration:**

```toml
[server]
host = "127.0.0.1"
port = 8080
timeout = 30

[database]
connection_string = "postgresql://user:password@localhost:5432/mentat"
pool_size = 10
max_lifetime_secs = 1800

[logging]
level = "info"
format = "compact"   # compact, json, or pretty

[cache]
enabled = true
capacity = 1000
ttl_secs = 300
```

**Environment Variables:**

| Variable                | Default                              | Description                  |
|-------------------------|--------------------------------------|------------------------------|
| `MENTATD_CONFIG`        | `mentatd.toml`                       | Path to config file.         |
| `MENTATD_HOST`          | `127.0.0.1`                          | Listen address.              |
| `MENTATD_PORT`          | `8080`                               | Listen port.                 |
| `MENTATD_TIMEOUT`       | `30`                                 | Request timeout (seconds).   |
| `DATABASE_URL`          | `postgresql://localhost/mentat`       | PostgreSQL connection string.|
| `DATABASE_POOL_SIZE`    | `10`                                 | Connection pool size.        |
| `DATABASE_MAX_LIFETIME` | `1800`                               | Max connection lifetime (s). |
| `RUST_LOG`              | `info`                               | Log level.                   |
| `LOG_FORMAT`            | `compact`                            | Log format.                  |
| `MENTATD_CACHE_ENABLED` | `true`                               | Enable query cache.          |
| `MENTATD_CACHE_CAPACITY`| `1000`                               | Max cached queries.          |
| `MENTATD_CACHE_TTL`     | `300`                                | Cache TTL (seconds).         |

---

## Endpoints

| Method | Path             | Description                        |
|--------|------------------|------------------------------------|
| POST   | `/`              | Execute an operation               |
| POST   | `/stream/query`  | Execute a streaming query (SSE)    |
| GET    | `/health`        | Health check                       |
| GET    | `/metrics`       | Prometheus metrics                 |

---

## Request Format

All operation requests are sent as `POST /` with an EDN-encoded body.

**General Structure:**

```edn
{:op :operation-name
 :args {:key1 value1
        :key2 value2}}
```

Every request must include an `:op` keyword identifying the operation. Most
operations also require an `:args` map with operation-specific parameters.

---

## Response Formats

### Content Negotiation

The response format is determined by the `Accept` header:

| Accept Header                          | Response Format      |
|----------------------------------------|----------------------|
| `application/edn` (or absent)         | EDN                  |
| `application/transit+json`            | Transit+JSON         |
| `application/transit+msgpack`         | Transit+MessagePack  |

### Success Response

```edn
{:result <value>}
```

### Error Response

```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/incorrect
         :cognitect.anomalies/message "Error description"
         :db/error ::db.error/invalid-request}}
```

**Anomaly Categories:**

| Category                                | HTTP Analogy | Description                 |
|-----------------------------------------|--------------|-----------------------------|
| `:cognitect.anomalies/incorrect`        | 400          | Client sent invalid request.|
| `:cognitect.anomalies/forbidden`        | 403          | Operation not permitted.    |
| `:cognitect.anomalies/not-found`        | 404          | Resource not found.         |
| `:cognitect.anomalies/unavailable`      | 503          | Database unavailable.       |
| `:cognitect.anomalies/interrupted`      | 504          | Operation timed out.        |
| `:cognitect.anomalies/fault`            | 500          | Internal server error.      |

---

## Operations

### :health

Health check.

**Request:**
```edn
{:op :health}
```

**Response:**
```edn
{:result "healthy"}
```

---

### :list-dbs

List all databases on the server.

**Request:**
```edn
{:op :list-dbs}
```

**Aliases:** `:datomic.catalog/list-dbs`

**Response:**
```edn
{:result ["db1" "db2" "mentat"]}
```

---

### :create-db

Create a new database.

**Request:**
```edn
{:op :create-db
 :args {:db-name "my_database"}}
```

**Aliases:** `:datomic.catalog/create-db`

**Parameters:**

| Key        | Type   | Required | Description               |
|------------|--------|----------|---------------------------|
| `:db-name` | string | Yes      | Database name (alphanumeric and underscores only, must start with letter, max 63 chars). |

**Response:**
```edn
{:result true}
```

---

### :delete-db

Delete a database.

**Request:**
```edn
{:op :delete-db
 :args {:db-name "my_database"}}
```

**Aliases:** `:datomic.catalog/delete-db`

**Parameters:**

| Key        | Type   | Required | Description       |
|------------|--------|----------|-------------------|
| `:db-name` | string | Yes      | Database to drop. |

**Response:**
```edn
{:result true}
```

---

### :connect

Connect to a database and obtain a connection ID.

**Request:**
```edn
{:op :connect
 :args {:db-name "mentat"}}
```

**Parameters:**

| Key        | Type   | Required | Description            |
|------------|--------|----------|------------------------|
| `:db-name` | string | Yes      | Database to connect to.|

**Response:**
```edn
{:result {:connection-id "550e8400-e29b-41d4-a716-446655440000"
          :db-name "mentat"
          :status "connected"}}
```

---

### :db

Get the current database state for a connection.

**Request:**
```edn
{:op :db
 :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"}}
```

**Parameters:**

| Key              | Type   | Required | Description      |
|------------------|--------|----------|------------------|
| `:connection-id` | UUID   | Yes      | Connection UUID. |

**Response:**
```edn
{:result {:connection-id "550e8400-e29b-41d4-a716-446655440000"
          :status "active"}}
```

---

### :q

Execute a Datalog query.

**Request:**
```edn
{:op :q
 :args {:query [:find ?e ?name
                :where [?e :person/name ?name]]
        :args []}}
```

**Parameters:**

| Key        | Type    | Required | Description                               |
|------------|---------|----------|-------------------------------------------|
| `:query`   | any     | Yes      | Datalog query (EDN vector or string).     |
| `:args`    | vector  | No       | Input bindings for `:in` clause.          |
| `:timeout` | integer | No       | Query timeout (ms).                       |
| `:limit`   | integer | No       | Max result rows.                          |
| `:offset`  | integer | No       | Skip this many result rows.               |

**Response:**
```edn
{:result [[10001 "Alice"] [10002 "Bob"]]}
```

**Example with Input Bindings:**
```edn
{:op :q
 :args {:query [:find ?name
                :in ?min-age
                :where
                [?e :person/name ?name]
                [?e :person/age ?age]
                [(>= ?age ?min-age)]]
        :args [30]}}
```

**Notes:**
- Query results are cached. The cache is invalidated after each successful
  transaction.
- For large result sets, consider using the streaming endpoint (`/stream/query`).

---

### :transact

Execute a transaction.

**Request:**
```edn
{:op :transact
 :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"
        :tx-data [{:db/id "alice"
                   :person/name "Alice"
                   :person/age 30}]}}
```

**Parameters:**

| Key              | Type   | Required | Description                          |
|------------------|--------|----------|--------------------------------------|
| `:connection-id` | string | Yes      | Connection identifier.               |
| `:tx-data`       | any    | Yes      | Transaction data (EDN vector).       |

**Response:**
```edn
{:result {:tx-id 1000005
          :tx-instant nil
          :tempids {:alice 10003}
          :datoms-inserted 2
          :status "committed"}}
```

**Notes:**
- Successful transactions invalidate the query cache.
- Transaction data supports all formats accepted by `mentat_transact`
  (vector form, map form, lookup refs, `:db.fn/cas`, etc.).

---

### :pull

Pull entity data using a pull pattern.

**Request:**
```edn
{:op :pull
 :args {:pattern [:person/name :person/age]
        :entity-id 10001}}
```

**Parameters:**

| Key           | Type    | Required | Description                  |
|---------------|---------|----------|------------------------------|
| `:pattern`    | any     | Yes      | Pull pattern (EDN vector).   |
| `:entity-id`  | integer | Yes      | Entity ID to pull.           |

**Response:**
```edn
{:result {:db/id 10001
          :person/name "Alice"
          :person/age 30}}
```

---

### :datoms

Access the raw datom index.

**Request:**
```edn
{:op :datoms
 :args {:index :eavt
        :components [10001]}}
```

**Parameters:**

| Key           | Type    | Required | Description                                    |
|---------------|---------|----------|------------------------------------------------|
| `:index`      | keyword | Yes      | Index to use: `:eavt`, `:aevt`, `:avet`, `:vaet`. |
| `:components` | vector  | No       | Filter components (index-order dependent).     |

**Component Order by Index:**

| Index   | Component 1 | Component 2 | Component 3 |
|---------|-------------|-------------|-------------|
| `:eavt` | entity      | attribute   | value       |
| `:aevt` | attribute   | entity      | value       |
| `:avet` | attribute   | value       | entity      |
| `:vaet` | value       | attribute   | entity      |

**Response:**
```edn
{:result [[10001 65 "Alice" 7 1000005 true]
          [10001 66 30 2 1000005 true]]}
```

Each datom is a vector: `[entity attribute value type-tag transaction added]`.

---

### :as-of

Execute a query as of a specific transaction.

**Request:**
```edn
{:op :as-of
 :args {:query [:find ?name :where [?e :person/name ?name]]
        :args []
        :t 1000005}}
```

**Parameters:**

| Key      | Type    | Required | Description                              |
|----------|---------|----------|------------------------------------------|
| `:query` | any     | Yes      | Datalog query.                           |
| `:args`  | vector  | No       | Input bindings.                          |
| `:t`     | integer | Yes      | Transaction ID to query as of.           |

**Response:** Same format as `:q`.

---

### :since

Execute a query for changes since a specific transaction.

**Request:**
```edn
{:op :since
 :args {:query [:find ?e ?name :where [?e :person/name ?name]]
        :args []
        :t 1000003}}
```

**Parameters:**

| Key      | Type    | Required | Description                              |
|----------|---------|----------|------------------------------------------|
| `:query` | any     | Yes      | Datalog query.                           |
| `:args`  | vector  | No       | Input bindings.                          |
| `:t`     | integer | Yes      | Only include datoms with `tx > t`.       |

**Response:** Same format as `:q`.

---

### :history

Execute a query including retracted datoms.

**Request:**
```edn
{:op :history
 :args {:query [:find ?e ?name ?tx ?added
                :where [?e :person/name ?name ?tx ?added]]
        :args []}}
```

**Parameters:**

| Key      | Type    | Required | Description                              |
|----------|---------|----------|------------------------------------------|
| `:query` | any     | Yes      | Datalog query.                           |
| `:args`  | vector  | No       | Input bindings.                          |

**Response:** Same format as `:q`, but results include retracted datoms.

---

### :tx-range

Query the transaction log for a range of transactions.

**Request:**
```edn
{:op :tx-range
 :args {:start 1000001
        :end 1000010}}
```

**Parameters:**

| Key      | Type    | Required | Description                |
|----------|---------|----------|----------------------------|
| `:start` | integer | No       | Start transaction ID.      |
| `:end`   | integer | No       | End transaction ID.        |

If both `:start` and `:end` are omitted, all transactions are returned.

**Response:**
```edn
{:result [{:tx 1000001 :tx-instant "2025-01-15T10:00:00Z"}
          {:tx 1000002 :tx-instant "2025-01-15T10:05:00Z"}]}
```

---

## Streaming Endpoint

### POST /stream/query

Execute a query and receive results incrementally via Server-Sent Events (SSE).
This is useful for large result sets where loading everything into memory would
be impractical.

**Request:** Same EDN body format as the main `/` endpoint. Only query
operations are supported: `:q`, `:as-of`, `:since`, `:history`.

**SSE Event Types:**

| Event     | Sent     | Description                                    |
|-----------|----------|------------------------------------------------|
| `columns` | Once     | Column names from the query.                   |
| `batch`   | 0+ times | A batch of result rows (default 1000 per batch).|
| `done`    | Once     | Summary with total rows, batches, and duration.|
| `error`   | 0-1 time | Error anomaly if query fails.                  |

**Example SSE Stream:**

```
event: columns
data: ["?name" "?age"]

event: batch
data: {:result [["Alice" 30] ["Bob" 25] ...]}

event: batch
data: {:result [["Zara" 42]]}

event: done
data: {:total-rows 1001 :batches 2 :duration-ms 45.2}
```

**Batch Format:**

Each `batch` event carries data in the same EDN format as a regular query
response:

```edn
{:result [[val1 val2] [val3 val4] ...]}
```

**Error Event:**

```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/incorrect
         :cognitect.anomalies/message "Error message"}}
```

**Example Request:**

```bash
curl -N -X POST http://localhost:8080/stream/query \
  -H "Content-Type: application/edn" \
  -d '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]]}}'
```

---

## Health Check

### GET /health

**Response:** `200 OK` with body `mentatd ready`.

---

## Metrics

### GET /metrics

Returns Prometheus-format metrics.

**Available Metrics:**

| Metric                              | Type      | Description                     |
|-------------------------------------|-----------|---------------------------------|
| `mentatd_request_total`             | Counter   | Total requests received.        |
| `mentatd_error_total`               | Counter   | Total errors.                   |
| `mentatd_query_total`               | Counter   | Total query operations.         |
| `mentatd_query_duration_seconds`    | Histogram | Query execution duration.       |
| `mentatd_transaction_total`         | Counter   | Total transactions.             |
| `mentatd_transaction_duration_seconds`| Histogram | Transaction execution duration.|
| `mentatd_cache_hits_total`          | Counter   | Query cache hits.               |
| `mentatd_cache_misses_total`        | Counter   | Query cache misses.             |
| `mentatd_cache_size`                | Gauge     | Current cache size.             |
| `mentatd_connection_pool_size`      | Gauge     | Current pool size.              |
| `mentatd_connection_pool_available` | Gauge     | Available connections.          |
| `mentatd_connection_pool_waiting`   | Gauge     | Clients waiting for connection. |
| `mentatd_stream_query_total`        | Counter   | Total streaming queries.        |
| `mentatd_stream_rows_sent_total`    | Counter   | Total rows sent via streaming.  |
| `mentatd_stream_duration_seconds`   | Histogram | Streaming query duration.       |
| `mentatd_operation_duration_seconds`| Histogram | Per-operation duration (labeled).|
| `mentatd_operation_total`           | Counter   | Per-operation count (labeled).  |

---

## Client Examples

### curl (EDN)

```bash
# Health check
curl http://localhost:8080/health

# Query
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -d '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]] :args []}}'

# Transact
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -d '{:op :transact :args {:connection-id "default" :tx-data [{:db/id "alice" :person/name "Alice" :person/age 30}]}}'

# Pull
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -d '{:op :pull :args {:pattern [:person/name :person/age] :entity-id 10001}}'
```

### curl (Transit+JSON)

```bash
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -H "Accept: application/transit+json" \
  -d '{:op :q :args {:query [:find ?e ?name :where [?e :person/name ?name]] :args []}}'
```

### curl (Transit+MessagePack)

```bash
curl -X POST http://localhost:8080/ \
  -H "Content-Type: application/edn" \
  -H "Accept: application/transit+msgpack" \
  -d '{:op :q :args {:query [:find ?e :where [?e :person/name]] :args []}}' \
  --output response.msgpack
```

---

## Error Handling

All errors are returned as anomaly maps following the Cognitect anomaly pattern.
The HTTP status code is always `200 OK`; the error is encoded in the response body.

**Parse Errors:**

| Error Type        | Category    | Example Cause                           |
|-------------------|-------------|-----------------------------------------|
| EDN parse error   | `:incorrect`| Malformed EDN in request body.          |
| Missing field     | `:incorrect`| Required `:op` or `:args` field absent. |
| Invalid operation | `:incorrect`| Unknown `:op` keyword.                  |
| Invalid type      | `:incorrect`| Wrong type for a field (e.g., non-UUID connection-id). |

**Server Errors:**

| Error Type          | Category       | Example Cause                      |
|---------------------|----------------|------------------------------------|
| Database pool error | `:unavailable` | Cannot acquire connection.         |
| Database error      | `:unavailable` | PostgreSQL query failure.          |
| Internal error      | `:fault`       | Unexpected server-side error.      |

---

## See Also

- [PostgreSQL Functions Reference](./POSTGRESQL_FUNCTIONS.md) -- Direct SQL API
- [Datalog Reference](./DATALOG_REFERENCE.md) -- Query language details
