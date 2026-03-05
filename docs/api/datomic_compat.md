# Datomic Compatibility Matrix

Comprehensive guide to Datomic compatibility for pg_mentat and mentatd.

## Overview

mentatd implements the Datomic wire protocol, allowing existing Datomic clients to connect to a PostgreSQL-backed Mentat database. This document outlines what works, what doesn't, and the differences between Datomic and Mentat.

## Protocol Support

### HTTP Protocol (mentatd)

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| EDN serialization | ✓ | ✓ | Complete | Fully supported |
| Transit serialization | ✓ | ✗ | Planned | Phase 2 |
| HTTP/1.1 | ✓ | ✓ | Complete | |
| HTTP/2 | ✓ | ✗ | Future | |
| Streaming responses | ✓ | ✗ | Planned | |
| Pagination | ✓ | Partial | In Progress | Basic support |
| Compression | ✓ | ✗ | Future | |

### Authentication

| Method | Datomic | mentatd | Status | Notes |
|--------|---------|---------|--------|-------|
| No auth | ✓ | ✓ | Complete | Localhost only |
| Token-based | ✓ | ✗ | Planned | Phase 2 |
| AWS SigV4 | ✓ | ✗ | Not Planned | Datomic Cloud only |
| Mutual TLS | ✗ | ✗ | Planned | Security enhancement |

## Database Operations

### Management

| Operation | Datomic | mentatd | Status | Notes |
|-----------|---------|---------|--------|-------|
| list-databases | ✓ | ✓ | Complete | |
| create-database | ✓ | ✓ | Complete | |
| delete-database | ✓ | ✓ | Complete | |
| connect | ✓ | ✓ | Complete | |
| db | ✓ | ✓ | Complete | Get current DB value |
| with-db | ✓ | ✗ | Planned | Speculative DB value |

### Transactions

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| transact | ✓ | ✓ | Complete | Add/retract facts |
| with | ✓ | ✗ | Planned | Speculative transactions |
| :db/add | ✓ | ✓ | Complete | |
| :db/retract | ✓ | ✓ | Complete | |
| :db/retractEntity | ✓ | ✗ | Planned | Retract all entity attrs |
| :db/cas | ✓ | ✗ | Planned | Compare-and-swap |
| :db/fn | ✓ | ✗ | Not Planned | Database functions |
| Map notation | ✓ | ✓ | Complete | Implicit adds |
| Tempids | ✓ | ✓ | Complete | String tempids |
| Lookup refs | ✓ | Partial | In Progress | Basic support |
| Upsert | ✓ | ✓ | Complete | Via unique attrs |
| Transaction metadata | ✓ | Partial | In Progress | Limited support |

## Query Operations

### Datalog Queries

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| :find relation | ✓ | ✓ | Complete | Multiple rows/cols |
| :find collection | ✓ | ✗ | Planned | Single column |
| :find tuple | ✓ | ✗ | Planned | Single row |
| :find scalar | ✓ | ✗ | Planned | Single value |
| :where patterns | ✓ | ✓ | Complete | Basic patterns |
| :in binding | ✓ | Partial | In Progress | Limited support |
| Rules | ✓ | ✗ | Planned | Recursive queries |
| Aggregates | ✓ | ✗ | Planned | count, sum, avg, etc. |
| Predicates | ✓ | Partial | In Progress | =, <, >, <=, >= |
| Expression clauses | ✓ | ✗ | Planned | Complex expressions |
| Query timeout | ✓ | ✓ | Complete | |
| Query limit | ✓ | ✗ | Planned | Result limiting |
| Query offset | ✓ | ✗ | Planned | Pagination |

### Query Functions

| Function | Datomic | mentatd | Status | Notes |
|----------|---------|---------|--------|-------|
| get-else | ✓ | ✗ | Planned | Default values |
| get-some | ✓ | ✗ | Planned | First non-nil |
| missing? | ✓ | ✗ | Planned | Check absence |
| ground | ✓ | ✗ | Planned | Bind constants |
| tuple | ✓ | ✗ | Planned | Create tuples |
| untuple | ✓ | ✗ | Planned | Destructure tuples |

### Aggregates

| Aggregate | Datomic | mentatd | Status | Notes |
|-----------|---------|---------|--------|-------|
| count | ✓ | ✗ | Planned | Count results |
| count-distinct | ✓ | ✗ | Planned | Unique count |
| sum | ✓ | ✗ | Planned | Sum values |
| min | ✓ | ✗ | Planned | Minimum value |
| max | ✓ | ✗ | Planned | Maximum value |
| avg | ✓ | ✗ | Planned | Average value |
| median | ✓ | ✗ | Planned | Median value |
| variance | ✓ | ✗ | Planned | Variance |
| stddev | ✓ | ✗ | Planned | Standard deviation |
| rand | ✓ | ✗ | Planned | Random sample |
| sample | ✓ | ✗ | Planned | Sample N items |
| distinct | ✓ | ✗ | Planned | Unique values |

## Entity Operations

### Pull API

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| pull | ✓ | Stub | In Progress | Basic entity fetch |
| Attribute selection | ✓ | ✗ | Planned | [:attr1 :attr2] |
| Wildcard | ✓ | ✗ | Planned | [*] |
| Namespace wildcard | ✓ | ✗ | Planned | [:person/*] |
| Recursive pulls | ✓ | ✗ | Planned | {:attr ...} |
| Limited recursion | ✓ | ✗ | Planned | {:attr 3} |
| Default values | ✓ | ✗ | Planned | (:attr :default val) |
| Cardinality limit | ✓ | ✗ | Planned | (:attr :limit n) |
| Reverse lookup | ✓ | ✗ | Planned | :entity/_attr |
| Component entities | ✓ | ✗ | Planned | :db/isComponent |
| Lookup refs | ✓ | ✗ | Planned | [:unique/attr val] |
| pull-many | ✓ | ✗ | Planned | Batch pulls |

### Entity Access

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| entity | ✓ | ✓ | Complete | Get entity map |
| Attribute access | ✓ | ✓ | Complete | Via JSON operators |
| Touch | ✓ | N/A | N/A | Not needed |
| Key iteration | ✓ | ✓ | Complete | Via JSON keys |

## Index Operations

### Index Access

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| datoms | ✓ | ✗ | Planned | Index scan |
| :eavt index | ✓ | ✗ | Planned | Entity-first |
| :aevt index | ✓ | ✗ | Planned | Attribute-first |
| :avet index | ✓ | ✗ | Planned | Value lookup |
| :vaet index | ✓ | ✗ | Planned | Reverse refs |
| seek-datoms | ✓ | ✗ | Planned | Positioned scan |
| index-range | ✓ | ✗ | Planned | Range scan |

## Time-Travel Operations

### Temporal Queries

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| as-of | ✓ | ✗ | Planned | Point-in-time |
| since | ✓ | ✗ | Planned | Changes since |
| history | ✓ | ✗ | Planned | All history |
| tx-range | ✓ | ✗ | Planned | Transaction log |
| Transaction time | ✓ | ✓ | Complete | :db/txInstant |
| Basis-t | ✓ | ✓ | Complete | Transaction ID |

## Schema Features

### Schema Definition

| Feature | Datomic | mentatd | Status | Notes |
|---------|---------|---------|--------|-------|
| :db/ident | ✓ | ✓ | Complete | Attribute identifier |
| :db/valueType | ✓ | ✓ | Complete | Type specification |
| :db/cardinality | ✓ | ✓ | Complete | one/many |
| :db/unique | ✓ | ✓ | Complete | value/identity |
| :db/doc | ✓ | ✓ | Complete | Documentation |
| :db/index | ✓ | ✓ | Complete | Index flag |
| :db/fulltext | ✓ | ✗ | Planned | Full-text search |
| :db/isComponent | ✓ | ✗ | Planned | Component entities |
| :db/noHistory | ✓ | ✗ | Planned | Skip history |
| :db.install/attribute | ✓ | N/A | N/A | Different mechanism |
| Schema alteration | ✓ | ✓ | Complete | Add new attrs |
| Schema retraction | ✓ | ✗ | Planned | Remove attrs |

### Value Types

| Type | Datomic | mentatd | Status | Notes |
|------|---------|---------|--------|-------|
| :db.type/boolean | ✓ | ✓ | Complete | |
| :db.type/long | ✓ | ✓ | Complete | i64 |
| :db.type/double | ✓ | Partial | In Progress | f64 |
| :db.type/string | ✓ | ✓ | Complete | UTF-8 |
| :db.type/keyword | ✓ | ✓ | Complete | |
| :db.type/ref | ✓ | ✓ | Complete | Entity refs |
| :db.type/instant | ✓ | Partial | In Progress | Timestamps |
| :db.type/uuid | ✓ | Partial | In Progress | UUIDs |
| :db.type/bytes | ✓ | ✗ | Planned | Binary data |
| :db.type/bigint | ✓ | ✗ | Planned | Arbitrary precision |
| :db.type/bigdec | ✓ | ✗ | Planned | Decimal numbers |
| :db.type/uri | ✓ | ✗ | Not Planned | Use string |
| :db.type/tuple | ✓ | ✗ | Not Planned | Datomic Cloud only |

## Data Model Differences

### Entity IDs

| Aspect | Datomic | Mentat | Impact |
|--------|---------|--------|--------|
| ID format | 64-bit with partition | Sequential i64 | High - Different ID scheme |
| ID allocation | Partition-based | Sequence-based | Medium - No partitions |
| Tempid resolution | Automatic | Automatic | Low - Compatible |
| Negative tempids | ✓ | ✗ | Low - Use strings |

### Transaction IDs

| Aspect | Datomic | Mentat | Impact |
|--------|---------|--------|--------|
| TX as entity | ✓ | Partial | Medium - Different metadata |
| TX attributes | Full entity | Limited | Medium - Fewer TX attrs |
| TX time | :db/txInstant | :db/txInstant | Low - Compatible |
| TX log | Full history | Full history | Low - Compatible |

### Partitions

| Feature | Datomic | Mentat | Impact |
|---------|---------|--------|--------|
| Partitions | ✓ | ✗ | Low - Not needed |
| :db.part/db | ✓ | N/A | Low - Schema different |
| :db.part/tx | ✓ | N/A | Low - Auto-managed |
| :db.part/user | ✓ | N/A | Low - Default behavior |
| Custom partitions | ✓ | ✗ | Low - Rare feature |

## Storage Differences

### Backend Storage

| Aspect | Datomic | Mentat | Impact |
|--------|---------|--------|--------|
| Storage engine | Pluggable | PostgreSQL | High - Different architecture |
| Immutable segments | ✓ | ✗ | Medium - Different model |
| MVCC | ✓ | ✓ | Low - PostgreSQL MVCC |
| Index segments | ✓ | ✗ | Medium - Uses PG indexes |
| Peer cache | ✓ | ✗ | High - No client cache |

### Full-Text Search

| Feature | Datomic | Mentat | Impact |
|---------|---------|---------|--------|
| Fulltext search | Lucene | PostgreSQL FTS | Medium - Different engine |
| :db/fulltext | ✓ | Planned | Medium - API same |
| Stemming | ✓ | ✓ | Low - PostgreSQL FTS |
| Stop words | ✓ | ✓ | Low - Configurable |

## Client Compatibility

### Official Clients

| Client | Language | Compatible | Notes |
|--------|----------|------------|-------|
| datomic-pro | Clojure | Partial | Peer mode not supported |
| datomic-client | Clojure | Yes | Client API supported |
| datomic-client-js | JavaScript | Yes | Tested with mentatd |
| datomic-client-py | Python | Untested | Should work |

### Third-Party Clients

| Library | Language | Compatible | Notes |
|---------|----------|------------|-------|
| datascript | JavaScript | Partial | Query syntax compatible |
| datahike | Clojure | Partial | Different storage |
| asami | Clojure | Partial | Graph-oriented |

## Migration Considerations

### From Datomic

**What Works:**
- Schema definitions transfer directly
- Basic queries work unchanged
- Transactions use same syntax
- Entity IDs can be mapped

**What Needs Changes:**
- Database functions (:db/fn) - Rewrite as application logic
- Partition references - Remove partition specifications
- Advanced pull patterns - Use simplified patterns
- Peer caching - Handle in application layer

**Example Migration:**

```clojure
;; Datomic schema
[{:db/id #db/id[:db.part/db]
  :db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]

;; Mentat schema (same!)
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]
```

### From SQLite Mentat

**What Changes:**
- Storage backend (SQLite → PostgreSQL)
- Connection management (embedded → server)
- Query interface (Rust API → SQL functions)

**What Stays Same:**
- Data model
- Schema format
- Query language
- Transaction format

## Known Incompatibilities

### Cannot Be Implemented

| Feature | Reason | Workaround |
|---------|--------|------------|
| :db/fn | Security risk | Application logic |
| Peer caching | Architecture | HTTP caching |
| Index segments | Storage model | PostgreSQL indexes |
| Excision | Not supported | Use retract |
| Datomic Analytics | Proprietary | Use PostgreSQL tools |

### Different Semantics

| Feature | Datomic Behavior | Mentat Behavior | Compatibility |
|---------|------------------|-----------------|---------------|
| Transaction IDs | Entity IDs | Sequence values | Low impact |
| Entity ID format | Partitioned | Sequential | Medium impact |
| Time basis | Transaction number | Timestamp-based | Low impact |
| Fulltext | Lucene queries | PostgreSQL FTS | Medium impact |

## Testing Compatibility

### Test Corpus

Standard Datomic test queries that work:

```clojure
;; Schema transaction
[{:db/ident :person/name
  :db/valueType :db.type/string
  :db/cardinality :db.cardinality/one}]

;; Data transaction
[{:db/id "alice"
  :person/name "Alice"
  :person/age 30}]

;; Query
[:find ?name ?age
 :where
 [?e :person/name ?name]
 [?e :person/age ?age]]

;; Pull
(pull [:person/name :person/age]
      [:person/email "alice@example.com"])
```

### Validation Checklist

Use this checklist to verify compatibility:

- [ ] Schema definition transactions
- [ ] Data transactions (add/retract)
- [ ] Basic where-clause queries
- [ ] Queries with input bindings
- [ ] Entity lookups
- [ ] Unique attribute upserts
- [ ] Transaction metadata
- [ ] Cardinality-many attributes
- [ ] Reference attributes
- [ ] Query predicates (>, <, >=, <=)

## Performance Comparison

### Query Performance

| Scenario | Datomic | Mentat | Notes |
|----------|---------|--------|-------|
| Simple patterns | Baseline | Similar | PostgreSQL optimized |
| Joins | Baseline | Similar | Good index support |
| Aggregates | Fast | Pending | Not yet implemented |
| Rules | Fast | Pending | Recursive queries |
| Large result sets | Fast | Similar | Network bound |

### Transaction Performance

| Operation | Datomic | Mentat | Notes |
|-----------|---------|--------|-------|
| Small TX | Baseline | Similar | <100 facts |
| Large TX | Baseline | Faster | PostgreSQL bulk insert |
| Concurrent TX | Serialized | Serialized | PostgreSQL MVCC |
| Upsert | Baseline | Similar | Unique constraints |

## Future Compatibility

### Planned Enhancements

- [ ] Transit serialization
- [ ] Complete pull API
- [ ] Aggregates and rules
- [ ] Time-travel queries
- [ ] Full index access
- [ ] Transaction functions (safe subset)
- [ ] Streaming responses
- [ ] Advanced predicates

### Not Planned

- Peer mode (architecture incompatible)
- Database functions (:db/fn) - security risk
- Datomic Analytics - proprietary
- Index segments - different storage model
- Excision - not in roadmap

## Getting Help

**Documentation:**
- [Datomic Protocol Specification](../architecture/datomic_protocol.md)
- [SQL Function API](./sql_functions.md)
- [Migration Guide](../guides/migration_guide.md)

**Issues:**
- Report compatibility issues on GitHub
- Tag with "datomic-compat"
- Include test case and expected behavior

**Community:**
- Discuss on GitHub Discussions
- Compare with Datomic documentation
- Share migration experiences
