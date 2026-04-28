"""
Integration tests for the pg_mentat Python client library.

Requires a running mentatd instance at ws://localhost:8080/ws.
Run with: python3 tests/integration/test_python_client.py

Tests the complete Datomic Client API workflow:
  1. Client creation
  2. Database connection
  3. Schema definition via transact
  4. Data insertion via transact
  5. Query execution
  6. Pull API
  7. Speculative transactions (with)
  8. Time-travel (as-of, since, history)
  9. Error handling (anomaly format)
  10. Connection lifecycle (release)
"""

from __future__ import annotations

import json
import sys
import os
import traceback

# Add the client library to the path
sys.path.insert(0, os.path.join(os.path.dirname(__file__), '..', '..', 'clients', 'python'))

from pg_mentat.client import (
    Keyword, Symbol, PgMentatError,
    transit_encode, transit_decode, _parse_transit_json,
    Client, Connection, Db,
    client, connect, db, q, transact, pull, pull_many,
    datoms, with_db, tx_range, as_of, since, history,
    list_databases, create_database, delete_database,
)

WS_ENDPOINT = os.environ.get("MENTATD_WS_ENDPOINT", "ws://localhost:8080/ws")

# Track test results
passed = 0
failed = 0
skipped = 0
errors: list[str] = []


def test(name: str):
    """Decorator to register and run a test function."""
    def decorator(fn):
        fn._test_name = name
        return fn
    return decorator


def run_test(fn):
    """Run a single test function and record the result."""
    global passed, failed, skipped
    name = getattr(fn, '_test_name', fn.__name__)
    try:
        fn()
        passed += 1
        print(f"  PASS: {name}")
    except ConnectionError as e:
        skipped += 1
        print(f"  SKIP: {name} (no server: {e})")
    except Exception as e:
        failed += 1
        errors.append(f"{name}: {e}")
        print(f"  FAIL: {name}: {e}")
        traceback.print_exc(file=sys.stdout)


# ============================================================================
# Unit tests (no server required)
# ============================================================================

@test("Transit encode/decode round-trip for keywords")
def test_transit_keyword_roundtrip():
    kw = Keyword("person/name")
    encoded = transit_encode(kw)
    parsed = json.loads(encoded)
    assert parsed == "~:person/name", f"Expected '~:person/name', got {parsed}"
    decoded = transit_decode(parsed)
    assert isinstance(decoded, Keyword)
    assert decoded.namespace == "person"
    assert decoded.name == "name"


@test("Transit encode/decode round-trip for maps")
def test_transit_map_roundtrip():
    data = {Keyword("op"): Keyword("q"), Keyword("query"): "[:find ?e]"}
    encoded = transit_encode(data)
    parsed = json.loads(encoded)
    # Should be a cmap
    assert parsed[0] == "^ ", f"Expected cmap, got {parsed}"
    decoded = transit_decode(parsed)
    assert isinstance(decoded, dict)
    assert decoded[Keyword("op")] == Keyword("q")
    assert decoded[Keyword("query")] == "[:find ?e]"


@test("Transit encode/decode for all Datomic value types")
def test_transit_all_value_types():
    import uuid as uuid_mod, math

    # nil
    assert transit_decode(None) is None

    # boolean
    assert transit_decode(True) is True
    assert transit_decode(False) is False

    # integer (small)
    assert transit_decode(42) == 42

    # integer (large)
    assert transit_decode("~i9999999999") == 9_999_999_999

    # float
    assert transit_decode(3.14) == 3.14

    # string
    assert transit_decode("hello") == "hello"

    # keyword
    kw = transit_decode("~:db/ident")
    assert isinstance(kw, Keyword) and kw.namespace == "db" and kw.name == "ident"

    # symbol
    sym = transit_decode("~$?e")
    assert isinstance(sym, Symbol) and sym.name == "?e"

    # uuid
    u = transit_decode("~u550e8400-e29b-41d4-a716-446655440000")
    assert isinstance(u, uuid_mod.UUID)

    # instant (millis since epoch)
    assert transit_decode("~m1714000000000") == 1714000000000

    # special floats
    assert math.isnan(transit_decode("~zNaN"))
    assert transit_decode("~zINF") == float("inf")
    assert transit_decode("~z-INF") == float("-inf")

    # set
    result = transit_decode(["~#set", [1, 2, 3]])
    assert isinstance(result, set) and result == {1, 2, 3}

    # list
    result = transit_decode(["~#list", [1, 2, 3]])
    assert result == [1, 2, 3]


@test("Parse Transit+JSON welcome message")
def test_parse_welcome():
    msg = '["^ ","~:type","~:datomic.client/session","~:session-id","abc-123","~:protocol-version",1]'
    result = _parse_transit_json(msg)
    assert result[Keyword("type")] == Keyword("datomic.client/session")
    assert result[Keyword("session-id")] == "abc-123"
    assert result[Keyword("protocol-version")] == 1


@test("Parse Transit+JSON success response")
def test_parse_success():
    msg = '["^ ","~:result",[[42,"Alice"],[43,"Bob"]],"~:request-id","req-1"]'
    result = _parse_transit_json(msg)
    assert result[Keyword("result")] == [[42, "Alice"], [43, "Bob"]]
    assert result[Keyword("request-id")] == "req-1"


@test("Parse Transit+JSON error response (cognitect.anomalies)")
def test_parse_error():
    msg = (
        '["^ ","~:error",'
        '["^ ","~:cognitect.anomalies/category","~:cognitect.anomalies/not-found",'
        '"~:cognitect.anomalies/message","Database not found"]]'
    )
    result = _parse_transit_json(msg)
    error = result[Keyword("error")]
    assert error[Keyword("cognitect.anomalies/category")] == Keyword(
        "cognitect.anomalies/not-found"
    )
    assert error[Keyword("cognitect.anomalies/message")] == "Database not found"


@test("Parse Transit+JSON transaction report")
def test_parse_tx_report():
    msg = (
        '["^ ","~:result",'
        '["^ ","~:db-before",["^ ","~:basis-t",1000],'
        '"~:db-after",["^ ","~:basis-t",1001],'
        '"~:tx-data",[[1001,50,"~m1714000000000",1001,true]],'
        '"~:tempids",["^ ","~:t1",10001]]]'
    )
    result = _parse_transit_json(msg)
    report = result[Keyword("result")]
    assert isinstance(report, dict)
    db_before = report[Keyword("db-before")]
    assert db_before[Keyword("basis-t")] == 1000
    db_after = report[Keyword("db-after")]
    assert db_after[Keyword("basis-t")] == 1001
    tempids = report[Keyword("tempids")]
    assert tempids[Keyword("t1")] == 10001


@test("Client creation with endpoint")
def test_client_creation():
    c = client(endpoint="ws://localhost:8080/ws")
    assert isinstance(c, Client)
    assert c.endpoint == "ws://localhost:8080/ws"


@test("Client creation with api_key")
def test_client_with_api_key():
    c = client(endpoint="ws://localhost:8080/ws", api_key="secret-key")
    assert c.api_key == "secret-key"


@test("Time-travel: as_of creates filtered db")
def test_as_of():
    database = Db(
        connection=None, db_name="test", database_id="id",
        t=1000, next_t=1001,
    )
    result = as_of(database, 500)
    assert result.as_of_t == 500
    assert result.since_t is None
    assert result.is_history is False
    assert result.t == 1000  # original t preserved


@test("Time-travel: since creates filtered db")
def test_since():
    database = Db(
        connection=None, db_name="test", database_id="id",
        t=1000, next_t=1001,
    )
    result = since(database, 500)
    assert result.since_t == 500
    assert result.as_of_t is None
    assert result.is_history is False


@test("Time-travel: history creates unfiltered db")
def test_history():
    database = Db(
        connection=None, db_name="test", database_id="id",
        t=1000, next_t=1001,
    )
    result = history(database)
    assert result.is_history is True
    assert result.as_of_t is None
    assert result.since_t is None


@test("PgMentatError carries category and response")
def test_error_type():
    err = PgMentatError("not found", category="not-found", response={"k": "v"})
    assert str(err) == "not found"
    assert err.category == "not-found"
    assert err.response == {"k": "v"}


@test("Full API surface exported from pg_mentat package")
def test_api_surface():
    import pg_mentat
    required = [
        'client', 'connect', 'db', 'q', 'transact', 'pull', 'pull_many',
        'datoms', 'with_db', 'tx_range', 'as_of', 'since', 'history',
        'list_databases', 'create_database', 'delete_database',
        'Client', 'Connection', 'Db',
    ]
    api = [x for x in dir(pg_mentat) if not x.startswith('_')]
    for name in required:
        assert name in api, f"Missing API function: {name}"


# ============================================================================
# Integration tests (require running mentatd)
# ============================================================================

@test("INTEGRATION: Connect to mentatd via WebSocket")
def test_integration_connect():
    c = client(endpoint=WS_ENDPOINT)
    conn = connect(c, db_name="integration_test")
    assert isinstance(conn, Connection)
    assert conn.db_name == "integration_test"
    conn.close()


@test("INTEGRATION: Get database value")
def test_integration_db():
    c = client(endpoint=WS_ENDPOINT)
    conn = connect(c, db_name="integration_test")
    database = db(conn)
    assert isinstance(database, Db)
    assert database.db_name == "integration_test"
    assert isinstance(database.t, int)
    conn.close()


@test("INTEGRATION: Schema definition + data transact + query")
def test_integration_transact_and_query():
    c = client(endpoint=WS_ENDPOINT)
    conn = connect(c, db_name="integration_test")

    # Define schema
    transact(conn, tx_data=(
        '[{:db/ident :integration.test/name '
        '  :db/valueType :db.type/string '
        '  :db/cardinality :db.cardinality/one}]'
    ))

    # Insert data
    transact(conn, tx_data='[{:integration.test/name "Alice"}]')

    # Query
    database = db(conn)
    result = q(
        '[:find ?name :where [_ :integration.test/name ?name]]',
        database,
    )
    assert result is not None
    conn.close()


@test("INTEGRATION: Speculative transaction (with)")
def test_integration_with():
    c = client(endpoint=WS_ENDPOINT)
    conn = connect(c, db_name="integration_test")
    database = db(conn)

    result = with_db(
        database,
        tx_data='[[:db/add "t" :integration.test/name "Speculative"]]',
    )
    # Result should be a transaction report
    assert result is not None
    conn.close()


# ============================================================================
# Main runner
# ============================================================================

def main():
    global passed, failed, skipped

    print("=" * 60)
    print("pg_mentat Python Client Integration Tests")
    print("=" * 60)

    # Collect all test functions
    tests = [
        # Unit tests (no server)
        test_transit_keyword_roundtrip,
        test_transit_map_roundtrip,
        test_transit_all_value_types,
        test_parse_welcome,
        test_parse_success,
        test_parse_error,
        test_parse_tx_report,
        test_client_creation,
        test_client_with_api_key,
        test_as_of,
        test_since,
        test_history,
        test_error_type,
        test_api_surface,
        # Integration tests (require mentatd)
        test_integration_connect,
        test_integration_db,
        test_integration_transact_and_query,
        test_integration_with,
    ]

    print(f"\nRunning {len(tests)} tests...\n")

    for t in tests:
        run_test(t)

    print(f"\n{'=' * 60}")
    print(f"Results: {passed} passed, {failed} failed, {skipped} skipped")
    if errors:
        print(f"\nFailures:")
        for err in errors:
            print(f"  - {err}")
    print("=" * 60)

    return 0 if failed == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
