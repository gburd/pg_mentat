"""Unit tests for Transit+JSON encoding/decoding in the pg_mentat Python client.

These tests run without a server -- they verify the wire format layer.
"""

import json
import math
import uuid

import pytest

from pg_mentat.transit import (
    Keyword,
    Symbol,
    PgMentatError,
    transit_encode,
    transit_decode,
    _transit_encode_value,
    _transit_decode_tagged,
    _parse_transit_json,
)


# ---------------------------------------------------------------------------
# Keyword / Symbol types
# ---------------------------------------------------------------------------


class TestKeyword:
    def test_simple_keyword(self):
        kw = Keyword("name")
        assert kw.name == "name"
        assert kw.namespace is None
        assert str(kw) == "name"

    def test_namespaced_keyword(self):
        kw = Keyword("person/name")
        assert kw.name == "name"
        assert kw.namespace == "person"
        assert str(kw) == "person/name"

    def test_explicit_namespace(self):
        kw = Keyword("name", "person")
        assert kw.name == "name"
        assert kw.namespace == "person"

    def test_equality(self):
        assert Keyword("person/name") == Keyword("name", "person")
        assert Keyword("name") != Keyword("other")

    def test_hashable(self):
        s = {Keyword("a"), Keyword("b"), Keyword("a")}
        assert len(s) == 2


class TestSymbol:
    def test_symbol(self):
        sym = Symbol("?e")
        assert sym.name == "?e"
        assert str(sym) == "?e"

    def test_equality(self):
        assert Symbol("?e") == Symbol("?e")
        assert Symbol("?e") != Symbol("?a")


# ---------------------------------------------------------------------------
# Transit encoding
# ---------------------------------------------------------------------------


class TestTransitEncode:
    def test_encode_none(self):
        assert json.loads(transit_encode(None)) is None

    def test_encode_bool(self):
        assert json.loads(transit_encode(True)) is True
        assert json.loads(transit_encode(False)) is False

    def test_encode_small_int(self):
        assert json.loads(transit_encode(42)) == 42
        assert json.loads(transit_encode(-1)) == -1

    def test_encode_large_int(self):
        encoded = transit_encode(9_999_999_999)
        assert "~i9999999999" in encoded

    def test_encode_float(self):
        result = json.loads(transit_encode(3.14))
        assert abs(result - 3.14) < 1e-10

    def test_encode_string(self):
        assert json.loads(transit_encode("hello")) == "hello"

    def test_encode_string_with_tilde(self):
        encoded = transit_encode("~special")
        assert "~~special" in encoded

    def test_encode_string_with_caret(self):
        encoded = transit_encode("^start")
        assert "~^start" in encoded

    def test_encode_keyword(self):
        encoded = transit_encode(Keyword("person/name"))
        assert "~:person/name" in encoded

    def test_encode_symbol(self):
        encoded = transit_encode(Symbol("?e"))
        assert "~$?e" in encoded

    def test_encode_uuid(self):
        u = uuid.UUID("550e8400-e29b-41d4-a716-446655440000")
        encoded = transit_encode(u)
        assert "~u550e8400-e29b-41d4-a716-446655440000" in encoded

    def test_encode_list(self):
        encoded = json.loads(transit_encode([1, 2, 3]))
        assert encoded == [1, 2, 3]

    def test_encode_set(self):
        encoded = json.loads(transit_encode({1, 2}))
        assert encoded[0] == "~#set"
        assert sorted(encoded[1]) == [1, 2]

    def test_encode_dict(self):
        encoded = json.loads(transit_encode({"name": "Alice"}))
        assert encoded[0] == "^ "
        assert "name" in encoded
        assert "Alice" in encoded

    def test_encode_keyword_dict(self):
        data = {Keyword("op"): Keyword("q")}
        encoded = json.loads(transit_encode(data))
        assert "^ " in encoded
        assert "~:op" in encoded
        assert "~:q" in encoded


# ---------------------------------------------------------------------------
# Transit decoding
# ---------------------------------------------------------------------------


class TestTransitDecode:
    def test_decode_none(self):
        assert transit_decode(None) is None

    def test_decode_bool(self):
        assert transit_decode(True) is True
        assert transit_decode(False) is False

    def test_decode_int(self):
        assert transit_decode(42) == 42

    def test_decode_float(self):
        assert transit_decode(3.14) == 3.14

    def test_decode_plain_string(self):
        assert transit_decode("hello") == "hello"

    def test_decode_keyword(self):
        result = transit_decode("~:person/name")
        assert isinstance(result, Keyword)
        assert result.namespace == "person"
        assert result.name == "name"

    def test_decode_simple_keyword(self):
        result = transit_decode("~:name")
        assert isinstance(result, Keyword)
        assert result.namespace is None
        assert result.name == "name"

    def test_decode_symbol(self):
        result = transit_decode("~$?e")
        assert isinstance(result, Symbol)
        assert result.name == "?e"

    def test_decode_large_int(self):
        assert transit_decode("~i9999999999") == 9_999_999_999

    def test_decode_uuid(self):
        result = transit_decode("~u550e8400-e29b-41d4-a716-446655440000")
        assert isinstance(result, uuid.UUID)
        assert str(result) == "550e8400-e29b-41d4-a716-446655440000"

    def test_decode_instant(self):
        result = transit_decode("~m1714000000000")
        assert result == 1714000000000

    def test_decode_nan(self):
        result = transit_decode("~zNaN")
        assert math.isnan(result)

    def test_decode_inf(self):
        assert transit_decode("~zINF") == float("inf")

    def test_decode_neg_inf(self):
        assert transit_decode("~z-INF") == float("-inf")

    def test_decode_escaped_tilde(self):
        assert transit_decode("~~hello") == "~hello"

    def test_decode_escaped_caret(self):
        assert transit_decode("~^hello") == "^hello"

    def test_decode_cmap(self):
        result = transit_decode(["^ ", "~:name", "Alice", "~:age", 30])
        assert isinstance(result, dict)
        assert result[Keyword("name")] == "Alice"
        assert result[Keyword("age")] == 30

    def test_decode_nested_cmap(self):
        result = transit_decode(
            ["^ ", "~:result", ["^ ", "~:db-name", "test-db", "~:t", 1000]]
        )
        assert isinstance(result, dict)
        inner = result[Keyword("result")]
        assert isinstance(inner, dict)
        assert inner[Keyword("db-name")] == "test-db"
        assert inner[Keyword("t")] == 1000

    def test_decode_vector_of_vectors(self):
        result = transit_decode([[42, "Alice"], [43, "Bob"]])
        assert len(result) == 2
        assert result[0] == [42, "Alice"]
        assert result[1] == [43, "Bob"]

    def test_decode_tagged_list(self):
        result = transit_decode(["~#list", [1, 2, 3]])
        assert result == [1, 2, 3]

    def test_decode_tagged_set(self):
        result = transit_decode(["~#set", [1, 2, 3]])
        assert isinstance(result, set)
        assert result == {1, 2, 3}


# ---------------------------------------------------------------------------
# Full Transit+JSON parsing
# ---------------------------------------------------------------------------


class TestParseTransitJson:
    def test_success_response(self):
        result = _parse_transit_json('["^ ","~:result",42]')
        assert isinstance(result, dict)
        assert result[Keyword("result")] == 42

    def test_error_response(self):
        json_str = (
            '["^ ","~:error",'
            '["^ ",'
            '"~:cognitect.anomalies/category","~:cognitect.anomalies/not-found",'
            '"~:cognitect.anomalies/message","Database not found"]]'
        )
        result = _parse_transit_json(json_str)
        assert isinstance(result, dict)
        error = result[Keyword("error")]
        assert isinstance(error, dict)
        assert error[Keyword("cognitect.anomalies/category")] == Keyword(
            "cognitect.anomalies/not-found"
        )
        assert error[Keyword("cognitect.anomalies/message")] == "Database not found"

    def test_query_result(self):
        result = _parse_transit_json(
            '["^ ","~:result",[[42,"Alice"],[43,"Bob"]]]'
        )
        assert isinstance(result, dict)
        rows = result[Keyword("result")]
        assert len(rows) == 2
        assert rows[0] == [42, "Alice"]
        assert rows[1] == [43, "Bob"]

    def test_connect_response(self):
        json_str = (
            '["^ ","~:result",'
            '["^ ",'
            '"~:db-name","test-db",'
            '"~:database-id","conn-123",'
            '"~:t",1000,'
            '"~:next-t",1001,'
            '"~:type","~:datomic.client/connection"]]'
        )
        result = _parse_transit_json(json_str)
        inner = result[Keyword("result")]
        assert inner[Keyword("db-name")] == "test-db"
        assert inner[Keyword("database-id")] == "conn-123"
        assert inner[Keyword("t")] == 1000
        assert inner[Keyword("next-t")] == 1001
        assert inner[Keyword("type")] == Keyword("datomic.client/connection")

    def test_welcome_message(self):
        json_str = (
            '["^ ",'
            '"~:type","~:datomic.client/session",'
            '"~:session-id","abc-123",'
            '"~:protocol-version",1]'
        )
        result = _parse_transit_json(json_str)
        assert result[Keyword("type")] == Keyword("datomic.client/session")
        assert result[Keyword("session-id")] == "abc-123"
        assert result[Keyword("protocol-version")] == 1


# ---------------------------------------------------------------------------
# Request encoding (full round-trip format check)
# ---------------------------------------------------------------------------


class TestRequestEncoding:
    def test_health_request(self):
        request = {Keyword("op"): Keyword("health")}
        encoded = transit_encode(request)
        parsed = json.loads(encoded)
        # Should be a cmap: ["^ ", "~:op", "~:health"]
        assert parsed[0] == "^ "
        assert "~:op" in parsed
        assert "~:health" in parsed

    def test_connect_request(self):
        request = {
            Keyword("op"): Keyword("connect"),
            Keyword("args"): {
                Keyword("db-name"): "my-db",
            },
        }
        encoded = transit_encode(request)
        parsed = json.loads(encoded)
        assert "~:op" in parsed
        assert "~:connect" in parsed
        assert "~:args" in parsed

    def test_query_request(self):
        request = {
            Keyword("op"): Keyword("q"),
            Keyword("args"): {
                Keyword("query"): "[:find ?e :where [?e :name]]",
                Keyword("args"): [],
            },
        }
        encoded = transit_encode(request)
        assert "~:q" in encoded
        assert "[:find ?e :where [?e :name]]" in encoded

    def test_transact_request(self):
        request = {
            Keyword("op"): Keyword("transact"),
            Keyword("args"): {
                Keyword("connection-id"): "conn-123",
                Keyword("tx-data"): '[{:person/name "Alice"}]',
            },
        }
        encoded = transit_encode(request)
        assert "~:transact" in encoded
        assert "conn-123" in encoded

    def test_request_with_request_id(self):
        request = {
            Keyword("op"): Keyword("health"),
            Keyword("request-id"): "req-456",
        }
        encoded = transit_encode(request)
        assert "req-456" in encoded


# ---------------------------------------------------------------------------
# Client data types
# ---------------------------------------------------------------------------


class TestClientTypes:
    def test_client_creation(self):
        from pg_mentat.transit import client, Client

        c = client(endpoint="ws://localhost:8080/ws")
        assert isinstance(c, Client)
        assert c.endpoint == "ws://localhost:8080/ws"

    def test_client_with_api_key(self):
        from pg_mentat.transit import client

        c = client(endpoint="ws://localhost:8080/ws", api_key="secret")
        assert c.api_key == "secret"

    def test_as_of(self):
        from pg_mentat.transit import Db, as_of

        database = Db(
            connection=None,  # type: ignore
            db_name="test",
            database_id="id",
            t=1000,
            next_t=1001,
        )
        result = as_of(database, 500)
        assert result.as_of_t == 500
        assert result.since_t is None
        assert result.is_history is False

    def test_since(self):
        from pg_mentat.transit import Db, since

        database = Db(
            connection=None,  # type: ignore
            db_name="test",
            database_id="id",
            t=1000,
            next_t=1001,
        )
        result = since(database, 500)
        assert result.since_t == 500
        assert result.as_of_t is None
        assert result.is_history is False

    def test_history(self):
        from pg_mentat.transit import Db, history

        database = Db(
            connection=None,  # type: ignore
            db_name="test",
            database_id="id",
            t=1000,
            next_t=1001,
        )
        result = history(database)
        assert result.is_history is True
        assert result.as_of_t is None
        assert result.since_t is None


# ---------------------------------------------------------------------------
# Error handling
# ---------------------------------------------------------------------------


class TestPgMentatError:
    def test_error_creation(self):
        err = PgMentatError("not found", category="not-found")
        assert str(err) == "not found"
        assert err.category == "not-found"

    def test_error_with_response(self):
        err = PgMentatError("fail", category="fault", response={"key": "val"})
        assert err.response == {"key": "val"}
