# Datomic Wire Protocol Specification

**Version:** Draft 0.1  
**Date:** 2026-03-05  
**Status:** Research Phase - Protocol Reverse Engineering

## Executive Summary

This document specifies the Datomic client-server wire protocol based on analysis of:
- Official Datomic client API documentation
- Open-source client implementations (JavaScript, Python, Clojure)
- Datomic wire spec gist (v0.1.21) by Richard Newman
- Transit and EDN serialization format specifications
- Community knowledge and architectural analyses

The goal is to enable a Rust-based `mentatd` server that can accept connections from existing Datomic clients while using Mentat's PostgreSQL-backed storage engine.

---

## 1. Architecture Overview

### 1.1 Datomic System Components

Datomic architecture consists of:

1. **Peers/Clients** - Application processes that query and transact
2. **Transactor** - Serializes writes, maintains ACID guarantees
3. **Storage Service** - Persistent storage (DynamoDB, PostgreSQL, etc.)
4. **Peer Server** - HTTP gateway for non-JVM clients
5. **Cloud** - AWS-native deployment with API Gateway

### 1.2 Communication Models

**Datomic Cloud:**
- REST/HTTP over AWS API Gateway
- EDN or Transit serialization
- AWS SigV4 authentication
- Request/response paradigm

**Datomic Peer Server:**
- HTTP endpoint (typically localhost for dev)
- EDN or Transit for serialization
- Token-based authentication
- Synchronous request/response

**Target for mentatd:**
Implement Peer Server-compatible protocol with optional Cloud compatibility.

---

## 2. Serialization Formats

### 2.1 EDN (Extensible Data Notation)

EDN is the primary format for Datomic data exchange. Mentat already has a complete EDN parser.

#### Core Types

| EDN Type | Example | Rust Equivalent |
|----------|---------|-----------------|
| nil | `nil` | `Option::None` |
| Boolean | `true`, `false` | `bool` |
| String | `"hello"` | `String` |
| Character | `\c`, `\newline` | `char` |
| Integer | `42`, `42N` | `i64`, `BigInt` |
| Float | `3.14`, `3.14M` | `f64`, `BigDecimal` |
| Keyword | `:db/id`, `:person/name` | `Keyword` |
| Symbol | `my-fn`, `clojure.core/map` | `Symbol` |
| Vector | `[1 2 3]` | `Vec<Value>` |
| List | `(+ 1 2)` | `LinkedList<Value>` |
| Map | `{:a 1 :b 2}` | `BTreeMap<Value, Value>` |
| Set | `#{1 2 3}` | `BTreeSet<Value>` |

#### Tagged Elements

```edn
#inst "1985-04-12T23:20:50.52Z"   ; RFC-3339 timestamp
#uuid "f81d4fae-7dec-11d0-a765-00a0c91e6bf6"
#db/id [:person/email "alice@example.com"]  ; Entity lookup
```

### 2.2 Transit Format

Transit is an optimized binary/JSON format with caching. For mentatd v1, Transit support is optional - focus on EDN first.

**Key Features:**
- Tag-based encoding: `["~#point", [10, 20]]`
- Cache codes for repeated values: `"^0"`, `"^1"`
- JSON and MessagePack backends
- Semantic types preserved across languages

---

## 3. Client API Operations

### 3.1 Request Structure

All operations follow this pattern:

```edn
{:op :operation-name
 :args {:param1 value1
        :param2 value2}}
```

### 3.2 Database Management

#### list-databases

**Request:**
```edn
{:op :datomic.catalog/list-dbs}
```

**Response:**
```edn
{:result ["my-db" "test-db" "prod-db"]}
```

#### create-database

**Request:**
```edn
{:op :datomic.catalog/create-db
 :args {:db-name "new-database"}}
```

**Response:**
```edn
{:result true}
```

#### delete-database

**Request:**
```edn
{:op :datomic.catalog/delete-db
 :args {:db-name "old-database"}}
```

**Response:**
```edn
{:result true}
```

### 3.3 Connection Operations

#### connect

**Request:**
```edn
{:op :connect
 :args {:db-name "my-db"}}
```

**Response:**
```edn
{:result {:connection-id "conn-uuid-here"
          :db {:database-id "db-uuid"
               :t 1000
               :next-t 1001
               :as-of nil
               :since nil
               :history false}}}
```

#### db (get current database value)

**Request:**
```edn
{:op :db
 :args {:connection-id "conn-uuid"}}
```

**Response:**
```edn
{:result {:database-id "db-uuid"
          :t 1500
          :next-t 1501}}
```

### 3.4 Query Operations

#### q (query)

**Request (map form):**
```edn
{:op :q
 :args {:query {:find [?name ?age]
                :in [$ ?min-age]
                :where [[?e :person/name ?name]
                        [?e :person/age ?age]
                        [(>= ?age ?min-age)]]}
        :args [{:database-id "db-uuid" :t 1500} 18]
        :timeout 5000
        :limit 1000
        :offset 0}}
```

**Request (list form):**
```edn
{:op :q
 :args {:query [:find ?name ?age
                :in $ ?min-age
                :where [?e :person/name ?name]
                       [?e :person/age ?age]
                       [(>= ?age ?min-age)]]
        :args [{:database-id "db-uuid" :t 1500} 18]}}
```

**Response:**
```edn
{:result [["Alice" 30]
          ["Bob" 25]
          ["Carol" 22]]}
```

**Pagination Support:**
- `:limit` (1-10000) - max results per page
- `:offset` - skip first N results
- `:chunk` - results per chunk for streaming

#### qseq (lazy/streaming query)

Same as `q` but returns results with pagination token:

**Response:**
```edn
{:result {:data [["Alice" 30] ["Bob" 25]]
          :next-token "page-token-123"}}
```

### 3.5 Entity Operations

#### pull

**Request:**
```edn
{:op :pull
 :args {:db {:database-id "db-uuid" :t 1500}
        :selector [:person/name
                   :person/email
                   {:person/friend [:person/name]}]
        :eid 17592186045418}}
```

**Alternative with lookup ref:**
```edn
{:op :pull
 :args {:db {:database-id "db-uuid" :t 1500}
        :selector [:person/name :person/email]
        :eid [:person/email "alice@example.com"]}}
```

**Response:**
```edn
{:result {:db/id 17592186045418
          :person/name "Alice"
          :person/email "alice@example.com"
          :person/friend [{:person/name "Bob"}
                          {:person/name "Carol"}]}}
```

#### datoms

**Request:**
```edn
{:op :datoms
 :args {:db {:database-id "db-uuid" :t 1500}
        :index :eavt
        :components [17592186045418]
        :limit 1000
        :offset 0}}
```

**Available indexes:**
- `:eavt` - Entity-Attribute-Value-Transaction
- `:aevt` - Attribute-Entity-Value-Transaction
- `:avet` - Attribute-Value-Entity-Transaction
- `:vaet` - Value-Attribute-Entity-Transaction

**Response:**
```edn
{:result [[17592186045418 :person/name "Alice" 13194139534312 true]
          [17592186045418 :person/age 30 13194139534312 true]
          [17592186045418 :person/email "alice@example.com" 13194139534313 true]]}
```

Datom format: `[entity attribute value transaction added]`

### 3.6 Transaction Operations

#### transact

**Request:**
```edn
{:op :transact
 :args {:connection-id "conn-uuid"
        :tx-data [[:db/add "temp-1" :person/name "Alice"]
                  [:db/add "temp-1" :person/email "alice@example.com"]
                  {:db/id "temp-2"
                   :person/name "Bob"
                   :person/friend "temp-1"}]}}
```

**Response:**
```edn
{:result {:db-before {:database-id "db-uuid" :t 1500}
          :db-after {:database-id "db-uuid" :t 1501}
          :tx-data [[17592186045418 :person/name "Alice" 13194139534314 true]
                    [17592186045418 :person/email "alice@example.com" 13194139534314 true]
                    [17592186045419 :person/name "Bob" 13194139534314 true]
                    [17592186045419 :person/friend 17592186045418 13194139534314 true]
                    [13194139534314 :db/txInstant #inst "2026-03-05T10:30:00.000Z" 13194139534314 true]]
          :tempids {"temp-1" 17592186045418
                    "temp-2" 17592186045419}}}
```

#### with (speculative transaction)

Same as `transact` but takes `:db` instead of `:connection-id` and doesn't persist:

**Request:**
```edn
{:op :with
 :args {:db {:database-id "db-uuid" :t 1500}
        :tx-data [[:db/add "temp-1" :person/name "Charlie"]]}}
```

**Response:**
Same structure as `transact` but changes aren't persisted.

### 3.7 Time Travel Operations

#### as-of

**Request:**
```edn
{:op :as-of
 :args {:db {:database-id "db-uuid" :t 1500}
        :t 1450}}
```

**Response:**
```edn
{:result {:database-id "db-uuid"
          :t 1450
          :as-of 1450}}
```

#### since

**Request:**
```edn
{:op :since
 :args {:db {:database-id "db-uuid" :t 1500}
        :t 1400}}
```

**Response:**
```edn
{:result {:database-id "db-uuid"
          :t 1500
          :since 1400}}
```

#### history

**Request:**
```edn
{:op :history
 :args {:db {:database-id "db-uuid" :t 1500}}}
```

**Response:**
```edn
{:result {:database-id "db-uuid"
          :t 1500
          :history true}}
```

### 3.8 Index Operations

#### index-range

**Request:**
```edn
{:op :index-range
 :args {:db {:database-id "db-uuid" :t 1500}
        :attrid :person/age
        :start 18
        :end 65
        :limit 1000}}
```

**Response:**
```edn
{:result [[17592186045418 :person/age 30 13194139534312 true]
          [17592186045420 :person/age 25 13194139534315 true]
          [17592186045421 :person/age 22 13194139534316 true]]}
```

#### tx-range

**Request:**
```edn
{:op :tx-range
 :args {:connection-id "conn-uuid"
        :start 1000
        :end 1100
        :limit 100}}
```

**Response:**
```edn
{:result [{:t 1001 :data [[...]]}
          {:t 1005 :data [[...]]}
          {:t 1012 :data [[...]]}]}
```

### 3.9 Statistics

#### db-stats

**Request:**
```edn
{:op :db-stats
 :args {:db {:database-id "db-uuid" :t 1500}}}
```

**Response:**
```edn
{:result {:datoms 15420
          :attrs 127}}
```

---

## 4. HTTP Protocol Layer

### 4.1 Endpoint Structure

**Peer Server:**
```
POST http://localhost:8998/
Content-Type: application/edn
Accept: application/edn

{:op :q :args {:query [:find ?e :where [?e :person/name]] :args [db]}}
```

**Datomic Cloud:**
```
POST https://api-gateway-id.execute-api.us-east-1.amazonaws.com/dev
Content-Type: application/transit+json
Authorization: AWS4-HMAC-SHA256 ...

["^ ","~:op","~:q","~:args",["^ ","~:query",...]]
```

### 4.2 Authentication

**Peer Server:**
- HTTP Basic Auth with access-key:secret
- Token-based (bearer token in header)

**Cloud:**
- AWS SigV4 request signing
- IAM role-based authentication

**mentatd Target:**
- Start with no auth (localhost only)
- Add token-based auth
- Consider mutual TLS for production

### 4.3 Error Response Format

```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/incorrect
         :cognitect.anomalies/message "Query parse error"
         :db/error :db.error/invalid-query}}
```

Standard anomaly categories:
- `:cognitect.anomalies/incorrect` - client error (400)
- `:cognitect.anomalies/forbidden` - auth error (403)
- `:cognitect.anomalies/not-found` - resource not found (404)
- `:cognitect.anomalies/unavailable` - service unavailable (503)
- `:cognitect.anomalies/interrupted` - timeout
- `:cognitect.anomalies/fault` - server error (500)

---

## 5. Protocol Design for Rust

### 5.1 Core Types

```rust
// mentatd/src/protocol/mod.rs

use edn::{Value, Keyword};
use uuid::Uuid;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub enum Operation {
    // Database management
    ListDatabases,
    CreateDatabase { db_name: String },
    DeleteDatabase { db_name: String },
    
    // Connection
    Connect { db_name: String },
    Db { connection_id: Uuid },
    
    // Query
    Query { query: Query, args: Vec<Value>, timeout: Option<u64>, limit: Option<usize>, offset: Option<usize> },
    Pull { db: DbId, selector: Vec<Value>, eid: EntityId },
    Datoms { db: DbId, index: Index, components: Vec<Value>, limit: Option<usize>, offset: Option<usize> },
    
    // Transaction
    Transact { connection_id: Uuid, tx_data: Vec<TxOp> },
    With { db: DbId, tx_data: Vec<TxOp> },
    
    // Time travel
    AsOf { db: DbId, t: i64 },
    Since { db: DbId, t: i64 },
    History { db: DbId },
    
    // Index
    IndexRange { db: DbId, attrid: Keyword, start: Option<Value>, end: Option<Value>, limit: Option<usize> },
    TxRange { connection_id: Uuid, start: Option<i64>, end: Option<i64>, limit: Option<usize> },
    
    // Stats
    DbStats { db: DbId },
}

#[derive(Debug, Clone)]
pub struct DbId {
    pub database_id: String,
    pub t: i64,
    pub next_t: Option<i64>,
    pub as_of: Option<i64>,
    pub since: Option<i64>,
    pub history: bool,
}

#[derive(Debug, Clone)]
pub enum EntityId {
    Entid(i64),
    LookupRef(Keyword, Value),
    TempId(String),
}

#[derive(Debug, Clone)]
pub enum Index {
    EAVT,
    AEVT,
    AVET,
    VAET,
}

#[derive(Debug, Clone)]
pub enum TxOp {
    Add { entity: EntityId, attribute: Keyword, value: Value },
    Retract { entity: EntityId, attribute: Keyword, value: Value },
    RetractEntity { entity: EntityId },
    Map(BTreeMap<Keyword, Value>),
}

#[derive(Debug, Clone)]
pub struct Request {
    pub op: Operation,
}

#[derive(Debug, Clone)]
pub enum Response {
    Success { result: Value },
    Error { anomaly: Anomaly },
}

#[derive(Debug, Clone)]
pub struct Anomaly {
    pub category: AnomalyCategory,
    pub message: String,
    pub db_error: Option<Keyword>,
}

#[derive(Debug, Clone, Copy)]
pub enum AnomalyCategory {
    Incorrect,
    Forbidden,
    NotFound,
    Unavailable,
    Interrupted,
    Fault,
}
```

### 5.2 Parser/Serializer Interface

```rust
// mentatd/src/protocol/codec.rs

use edn::Value;
use crate::protocol::{Request, Response};

pub trait Codec {
    fn decode_request(&self, input: &[u8]) -> Result<Request, CodecError>;
    fn encode_response(&self, response: &Response) -> Result<Vec<u8>, CodecError>;
}

pub struct EdnCodec;

impl Codec for EdnCodec {
    fn decode_request(&self, input: &[u8]) -> Result<Request, CodecError> {
        let s = std::str::from_utf8(input)?;
        let value = edn::parse::value(s)?;
        parse_request(&value)
    }
    
    fn encode_response(&self, response: &Response) -> Result<Vec<u8>, CodecError> {
        let value = serialize_response(response);
        Ok(edn::pretty_print::pretty_print(&value).into_bytes())
    }
}

// Future: TransitCodec for binary/JSON Transit format
```

### 5.3 Server Architecture

```rust
// mentatd/src/server.rs

use axum::{Router, routing::post, Json};
use crate::protocol::{Request, Response, Operation};
use crate::storage::PostgresBackend;

pub struct MentatServer {
    storage: PostgresBackend,
    codec: Box<dyn Codec>,
}

impl MentatServer {
    pub async fn handle_request(&self, req: Request) -> Response {
        match req.op {
            Operation::Query { query, args, .. } => {
                self.execute_query(query, args).await
            }
            Operation::Transact { connection_id, tx_data } => {
                self.execute_transaction(connection_id, tx_data).await
            }
            // ... handle other operations
        }
    }
    
    pub fn router() -> Router {
        Router::new()
            .route("/", post(handle_post))
    }
}

async fn handle_post(
    body: String,
) -> Result<String, StatusCode> {
    // Parse EDN request, dispatch to handler, serialize response
}
```

---

## 6. Test Corpus

### 6.1 Schema Transaction

```edn
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one
  :db/doc "A person's full name"}
 
 {:db/ident :person/email
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/many
  :db/unique :db.unique/identity
  :db/doc "Email addresses"}
 
 {:db/ident :person/age
  :db/valueType :db.type/long
  :db/cardinality :db.cardinality/one
  :db/index true}
 
 {:db/ident :person/friend
  :db/valueType :db.type/ref
  :db/cardinality :db.cardinality/many
  :db/doc "Friends are other people"}]
```

### 6.2 Data Transaction

```edn
[{:db/id "alice"
  :person/name "Alice Anderson"
  :person/email "alice@example.com"
  :person/age 30}
 
 {:db/id "bob"
  :person/name "Bob Brown"
  :person/email ["bob@example.com" "bobby@work.com"]
  :person/age 25
  :person/friend "alice"}
 
 [:db/add "alice" :person/friend "bob"]]
```

### 6.3 Query Examples

**Simple pattern:**
```edn
[:find ?name ?email
 :where [?e :person/name ?name]
        [?e :person/email ?email]]
```

**With input:**
```edn
[:find ?name
 :in $ ?min-age
 :where [?e :person/name ?name]
        [?e :person/age ?age]
        [(>= ?age ?min-age)]]
```

**Aggregation:**
```edn
[:find (count ?e) (avg ?age)
 :where [?e :person/age ?age]]
```

**Pull in find:**
```edn
[:find (pull ?e [:person/name :person/email])
 :where [?e :person/age ?age]
        [(>= ?age 18)]]
```

**Recursive rules:**
```edn
[:find ?ancestor-name
 :in $ ?person
 :where [?person :person/name ?name]
        (ancestor ?person ?ancestor)
        [?ancestor :person/name ?ancestor-name]]

;; Rules
[[(ancestor ?p ?a)
  [?p :person/parent ?a]]
 [(ancestor ?p ?a)
  [?p :person/parent ?x]
  (ancestor ?x ?a)]]
```

### 6.4 Pull Patterns

```edn
;; Basic
[:person/name :person/email]

;; Wildcard
[*]

;; Nested
[:person/name 
 {:person/friend [:person/name :person/email]}]

;; Recursive
[:person/name 
 {:person/friend ...}]

;; Limited recursion
[:person/name 
 {:person/friend 3}]

;; With defaults
[:person/name 
 (:person/nickname :default "Unknown")]

;; Limit cardinality
[(:person/email :limit 1)]

;; Component entities
[:order/id 
 {:order/items [:item/name :item/price]}]
```

---

## 7. Compatibility Matrix

### 7.1 Supported Operations (Phase 1)

| Operation | Priority | Status | Notes |
|-----------|----------|--------|-------|
| list-databases | High | Planned | |
| create-database | High | Planned | |
| connect | High | Planned | |
| db | High | Planned | |
| q | Critical | Planned | Core query engine |
| pull | Critical | Planned | Already in Mentat |
| datoms | High | Planned | Index access |
| transact | Critical | Planned | Core writes |
| with | Medium | Planned | Speculative transactions |
| as-of | Medium | Planned | Time travel |
| since | Medium | Planned | Time travel |
| history | Low | Future | Full history access |
| index-range | Low | Future | Optimization |
| tx-range | Low | Future | Transaction log |
| db-stats | Low | Planned | Monitoring |

### 7.2 Known Limitations

1. **No peer caching** - Mentat doesn't implement Datomic's peer cache architecture
2. **No index segments** - Different storage model (PostgreSQL vs Datomic's immutable index chunks)
3. **Limited Transit support** - Phase 1 focuses on EDN only
4. **No distributed transactions** - Single PostgreSQL instance
5. **Different transaction IDs** - Mentat uses different ID scheme
6. **No excision** - Mentat doesn't support excision (permanent data removal)
7. **Different fulltext** - Uses PostgreSQL FTS instead of Lucene
8. **No analytics** - Datomic Analytics (Presto/Spark) not supported
9. **Different backup/restore** - Uses PostgreSQL tooling

### 7.3 Incompatibilities

| Feature | Datomic | Mentat | Impact |
|---------|---------|--------|--------|
| Entity IDs | 64-bit with partition bits | SQLite ROWID-based | High - ID format differs |
| Transaction IDs | Entities in database | Separate sequence | Medium - tx log differs |
| Time basis | Transaction number (t) | Timestamp-based | Medium - different semantics |
| Partitions | Explicit partitioning | No partitions | Low - schema difference |
| :db/fn | Database functions | Not supported | High - can't run custom fns |
| Excision | Supported | Not supported | Low - rare feature |
| Reified transactions | Full entity | Limited metadata | Medium - different tx-data |

---

## 8. Implementation Roadmap

### Phase 1: Core Protocol (Weeks 1-2)
- [ ] EDN codec (request parser, response serializer)
- [ ] HTTP server with axum
- [ ] Basic operation routing
- [ ] list-databases, create-database, connect
- [ ] Error handling and anomaly format

### Phase 2: Query Operations (Weeks 3-4)
- [ ] q operation integration with existing Mentat query engine
- [ ] pull operation
- [ ] datoms index access
- [ ] Query timeout handling
- [ ] Pagination support

### Phase 3: Transaction Operations (Weeks 5-6)
- [ ] transact operation
- [ ] with (speculative) operation
- [ ] Transaction response formatting
- [ ] Tempid resolution
- [ ] Schema validation

### Phase 4: Time Travel (Week 7)
- [ ] as-of operation
- [ ] since operation
- [ ] Database value filtering
- [ ] Time basis management

### Phase 5: Testing & Docs (Week 8)
- [ ] Integration tests with official Datomic client
- [ ] Test corpus validation
- [ ] Performance benchmarking
- [ ] API documentation
- [ ] Migration guide

### Phase 6: Advanced Features (Future)
- [ ] Transit codec
- [ ] history operation
- [ ] index-range optimization
- [ ] tx-range operation
- [ ] Authentication
- [ ] Connection pooling
- [ ] Metrics and monitoring

---

## 9. References

### Documentation
- [Datomic Client API](https://docs.datomic.com/client-api/)
- [EDN Specification](https://github.com/edn-format/edn)
- [Transit Format](https://github.com/cognitect/transit-format)
- [Datomic Wire Spec Gist](https://gist.github.com/rnewman/fe3051d309ef5abd68e890ed13b38acd)

### Implementations
- [datomic-client-js](https://github.com/csm/datomic-client-js) - JavaScript client
- [pydatomic](https://github.com/patrsc/pydatomic) - Python client
- [datomic-clj-client](https://github.com/rnewman/datomic-clj-client) - Clojure client

### Articles
- [Datomic as a Protocol](https://tonsky.me/blog/datomic-as-protocol/)
- [Architecture of Datomic](https://vvvvalvalval.github.io/posts/2016-01-03-architecture-datomic-branching-reality.html)

### Related
- [Mentat README](https://github.com/qpdb/mentat)
- [DataScript](https://github.com/tonsky/datascript)

---

## Appendix A: Example Request/Response Flows

### A.1 Complete Query Session

**1. List databases:**
```
→ {:op :datomic.catalog/list-dbs}
← {:result ["inventory" "customers" "orders"]}
```

**2. Connect:**
```
→ {:op :connect :args {:db-name "inventory"}}
← {:result {:connection-id "550e8400-e29b-41d4-a716-446655440000"
            :db {:database-id "inv-db-001" :t 1000}}}
```

**3. Query:**
```
→ {:op :q
   :args {:query [:find ?name ?sku
                  :where [?e :product/name ?name]
                         [?e :product/sku ?sku]]
          :args [{:database-id "inv-db-001" :t 1000}]}}
← {:result [["Widget" "WDG-001"]
            ["Gadget" "GDG-002"]]}
```

**4. Pull entity:**
```
→ {:op :pull
   :args {:db {:database-id "inv-db-001" :t 1000}
          :selector [:product/name :product/sku :product/price]
          :eid [:product/sku "WDG-001"]}}
← {:result {:db/id 17592186045418
            :product/name "Widget"
            :product/sku "WDG-001"
            :product/price 29.99}}
```

### A.2 Transaction Flow

**1. Schema transaction:**
```
→ {:op :transact
   :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"
          :tx-data [{:db/ident :item/name
                     :db/valueType :db.type/string
                     :db/cardinality :db.cardinality/one}]}}
← {:result {:db-before {:t 1000}
            :db-after {:t 1001}
            :tx-data [[63 :db/ident :item/name 13194139534312 true]
                      [63 :db/valueType 23 13194139534312 true]
                      [63 :db/cardinality 35 13194139534312 true]]
            :tempids {}}}
```

**2. Data transaction:**
```
→ {:op :transact
   :args {:connection-id "550e8400-e29b-41d4-a716-446655440000"
          :tx-data [{:db/id "new-item"
                     :item/name "Sprocket"}]}}
← {:result {:db-before {:t 1001}
            :db-after {:t 1002}
            :tx-data [[17592186045418 :item/name "Sprocket" 13194139534313 true]]
            :tempids {"new-item" 17592186045418}}}
```

**3. Speculative transaction:**
```
→ {:op :with
   :args {:db {:database-id "inv-db-001" :t 1002}
          :tx-data [[:db/add [:item/name "Sprocket"] :item/price 5.99]]}}
← {:result {:db-before {:t 1002}
            :db-after {:t 1002 :as-of 1002}
            :tx-data [[17592186045418 :item/price 5.99 13194139534314 true]]
            :tempids {}}}
```

---

## Appendix B: Error Examples

### B.1 Parse Error
```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/incorrect
         :cognitect.anomalies/message "Query parse error at line 3"
         :db/error :db.error/invalid-query
         :query "[:find ?e :where [?e :nosuch/attr]]"}}
```

### B.2 Not Found
```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/not-found
         :cognitect.anomalies/message "Database 'missing-db' not found"}}
```

### B.3 Unavailable
```edn
{:error {:cognitect.anomalies/category :cognitect.anomalies/unavailable
         :cognitect.anomalies/message "Storage backend unavailable"
         :db/error :db.error/unavailable}}
```

---

**End of Protocol Specification**
