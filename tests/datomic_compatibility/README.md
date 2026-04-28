# Datomic Compatibility Test Suite

Tests verifying that pg_mentat client libraries are drop-in replacements
for Datomic clients.

## Test Categories

### 1. Wire Protocol (Transit+JSON)

All clients must correctly encode/decode Transit+JSON:

- Keywords: `"~:db/name"` <-> `:db/name`
- Symbols: `"~$?e"` <-> `?e`
- Large integers: `"~i9999999999"` <-> `9999999999`
- UUIDs: `"~u550e8400-..."` <-> UUID object
- Instants: `"~m1714000000000"` <-> Date/Instant
- Maps (cmap): `["^ ", k1, v1, ...]` <-> hash-map
- Vectors: `[1, 2, 3]` <-> vector
- Lists: `["~#list", [1, 2, 3]]` <-> list
- Sets: `["~#set", [1, 2, 3]]` <-> set
- Special floats: `"~zNaN"`, `"~zINF"`, `"~z-INF"`
- Escaped strings: `"~~"` prefix for tilde, `"~^"` for caret
- Null: JSON `null` <-> nil/None/null

### 2. Session Protocol

- Client connects to `ws://host:port/ws`
- Server sends welcome: `{:type :datomic.client/session, :session-id "...", :protocol-version 1}`
- Client sends requests with `:op`, `:args`, optional `:request-id`
- Server responds with `:result` or `:error` (cognitect.anomalies format)
- Request-id correlation for multiplexed requests

### 3. API Operations

Each operation tested with exact request/response format:

| Operation | Request `:op` | Required `:args` |
|-----------|--------------|------------------|
| list-dbs | `:list-dbs` | none |
| create-db | `:create-db` | `:db-name` |
| delete-db | `:delete-db` | `:db-name` |
| connect | `:connect` | `:db-name` |
| db | `:db` | `:db-name` |
| q | `:q` | `:query`, `:args` |
| transact | `:transact` | `:connection-id`, `:tx-data` |
| pull | `:pull` | `:pattern`, `:entity-id` |
| datoms | `:datoms` | `:index`, `:components` |
| with | `:with` | `:tx-data` |
| tx-range | `:tx-range` | optional `:start`, `:end` |
| as-of | `:as-of` | `:query`, `:args`, `:t` |
| since | `:since` | `:query`, `:args`, `:t` |
| history | `:history` | `:query`, `:args` |

### 4. Error Format (cognitect.anomalies)

All errors must follow the cognitect.anomalies format:

```edn
{:cognitect.anomalies/category :cognitect.anomalies/<category>
 :cognitect.anomalies/message "<message>"
 :db/error :<error-code>}
```

Categories: `:incorrect`, `:forbidden`, `:not-found`, `:unavailable`, `:interrupted`, `:fault`

### 5. Time-Travel

- `as-of(db, t)` returns a filtered database value
- `since(db, t)` returns changes since t
- `history(db)` returns full history including retractions
- Time-travel db values work with `q`, `pull`, `datoms`

## Running Tests

### Clojure
```bash
cd clients/clojure
clj -X:test
# Integration tests only:
clj -X:test :selector :integration
```

### Python
```bash
cd clients/python
pip install -e ".[dev]"
# Unit tests (no server needed):
pytest tests/test_transit.py -v
# Integration tests (requires mentatd):
pytest tests/ -v -m integration
```

### TypeScript/Node.js
```bash
cd clients/nodejs
npm install
npm test
```
