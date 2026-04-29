"""Tests for the pg_mentat Python native client.

These tests validate the client API surface, type conversions, and internal
helpers. Tests that require a live PostgreSQL server with pg_mentat are
marked and can be skipped with ``pytest -m "not integration"``.
"""

from __future__ import annotations

import json
from datetime import datetime
from unittest.mock import MagicMock, patch, PropertyMock

import pytest

from pg_mentat.client import Connection, Database, MentatError


# ---------------------------------------------------------------------------
# MentatError
# ---------------------------------------------------------------------------


class TestMentatError:
    def test_basic_error(self):
        err = MentatError("something went wrong")
        assert str(err) == "something went wrong"
        assert err.detail is None

    def test_error_with_detail(self):
        err = MentatError("failed", detail="SQL error: relation does not exist")
        assert str(err) == "failed"
        assert err.detail == "SQL error: relation does not exist"

    def test_is_exception(self):
        assert issubclass(MentatError, Exception)


# ---------------------------------------------------------------------------
# Connection -- construction and lifecycle
# ---------------------------------------------------------------------------


class TestConnectionInit:
    @patch("pg_mentat.client.psycopg2")
    def test_creates_connection_from_dsn(self, mock_psycopg2):
        mock_conn = MagicMock()
        mock_psycopg2.connect.return_value = mock_conn

        conn = Connection("dbname=test")

        mock_psycopg2.connect.assert_called_once_with("dbname=test")
        assert mock_conn.autocommit is True
        assert conn._owns_conn is True

    @patch("pg_mentat.client.psycopg2")
    def test_creates_connection_with_kwargs(self, mock_psycopg2):
        mock_conn = MagicMock()
        mock_psycopg2.connect.return_value = mock_conn

        conn = Connection("dbname=test", port=5433)

        mock_psycopg2.connect.assert_called_once_with("dbname=test", port=5433)

    def test_reuses_existing_connection(self):
        mock_conn = MagicMock()

        conn = Connection(connection=mock_conn)

        assert conn._conn is mock_conn
        assert conn._owns_conn is False

    @patch("pg_mentat.client.psycopg2")
    def test_close_owned_connection(self, mock_psycopg2):
        mock_conn = MagicMock()
        mock_conn.closed = 0
        mock_psycopg2.connect.return_value = mock_conn

        conn = Connection("dbname=test")
        conn.close()

        mock_conn.close.assert_called_once()

    def test_close_borrowed_connection_is_noop(self):
        mock_conn = MagicMock()
        mock_conn.closed = 0

        conn = Connection(connection=mock_conn)
        conn.close()

        mock_conn.close.assert_not_called()

    @patch("pg_mentat.client.psycopg2")
    def test_context_manager(self, mock_psycopg2):
        mock_conn = MagicMock()
        mock_conn.closed = 0
        mock_psycopg2.connect.return_value = mock_conn

        with Connection("dbname=test") as conn:
            assert conn is not None

        mock_conn.close.assert_called_once()

    @patch("pg_mentat.client.psycopg2")
    def test_closed_property(self, mock_psycopg2):
        mock_conn = MagicMock()
        mock_psycopg2.connect.return_value = mock_conn

        conn = Connection("dbname=test")

        mock_conn.closed = 0
        assert conn.closed is False

        mock_conn.closed = 1
        assert conn.closed is True


# ---------------------------------------------------------------------------
# Connection.db()
# ---------------------------------------------------------------------------


class TestConnectionDb:
    def test_db_returns_database(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)

        db = conn.db()

        assert isinstance(db, Database)
        assert db._conn is conn
        assert db._as_of_tx is None
        assert db._as_of_instant is None


# ---------------------------------------------------------------------------
# Connection.transact()
# ---------------------------------------------------------------------------


class TestConnectionTransact:
    def test_transact_edn_string(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = ('{"tx": 1000}',)
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)
        result = conn.transact('[{:person/name "Alice"}]')

        mock_cursor.execute.assert_called_once_with(
            "SELECT mentat_transact(%s)",
            ('[{:person/name "Alice"}]',),
        )
        assert result == {"tx": 1000}

    def test_transact_list_of_dicts(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = ('{"tx": 1001}',)
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)
        result = conn.transact([{":person/name": "Bob"}])

        call_args = mock_cursor.execute.call_args
        edn_arg = call_args[0][1][0]
        assert ":person/name" in edn_arg
        assert '"Bob"' in edn_arg
        assert result == {"tx": 1001}

    def test_transact_returns_jsonb_directly(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = ({"tx": 1002},)
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)
        result = conn.transact('[{:person/name "Carol"}]')

        assert result == {"tx": 1002}

    def test_transact_raises_on_no_result(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = None
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)

        with pytest.raises(MentatError, match="mentat_transact returned no result"):
            conn.transact('[{:person/name "Dave"}]')


# ---------------------------------------------------------------------------
# Database.q()
# ---------------------------------------------------------------------------


class TestDatabaseQ:
    def _make_db(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_pg_conn.cursor.return_value = mock_cursor
        conn = Connection(connection=mock_pg_conn)
        return conn.db(), mock_cursor

    def test_basic_query(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('[["Alice"], ["Bob"]]',)

        result = db.q('[:find ?name :where [?e :person/name ?name]]')

        call_args = cursor.execute.call_args
        assert call_args[0][0] == "SELECT mentat_query(%s, %s::jsonb)"
        assert call_args[0][1][0] == '[:find ?name :where [?e :person/name ?name]]'
        inputs = json.loads(call_args[0][1][1])
        assert inputs == {}
        assert result == [["Alice"], ["Bob"]]

    def test_query_with_dict_inputs(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('[[42]]',)

        result = db.q(
            '[:find ?e :in $ ?name :where [?e :person/name ?name]]',
            {"name": "Alice"},
        )

        call_args = cursor.execute.call_args
        inputs = json.loads(call_args[0][1][1])
        assert inputs["name"] == "Alice"

    def test_query_with_positional_inputs(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('[[42]]',)

        result = db.q(
            '[:find ?e :in $ ?name :where [?e :person/name ?name]]',
            "Alice",
        )

        call_args = cursor.execute.call_args
        inputs = json.loads(call_args[0][1][1])
        assert inputs["args"] == ["Alice"]

    def test_query_returns_jsonb_directly(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ([["Alice"]],)

        result = db.q('[:find ?name :where [?e :person/name ?name]]')
        assert result == [["Alice"]]

    def test_query_raises_on_no_result(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = None

        with pytest.raises(MentatError, match="mentat_query returned no result"):
            db.q('[:find ?e :where [?e :person/name]]')


# ---------------------------------------------------------------------------
# Database.pull()
# ---------------------------------------------------------------------------


class TestDatabasePull:
    def _make_db(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_pg_conn.cursor.return_value = mock_cursor
        conn = Connection(connection=mock_pg_conn)
        return conn.db(), mock_cursor

    def test_pull_string_pattern(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('{"person/name": "Alice"}',)

        result = db.pull('[*]', 42)

        cursor.execute.assert_called_once_with(
            "SELECT mentat_pull(%s, %s)",
            ('[*]', 42),
        )
        assert result == {"person/name": "Alice"}

    def test_pull_list_pattern(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('{"person/name": "Alice"}',)

        result = db.pull(["*"], 42)

        call_args = cursor.execute.call_args
        assert call_args[0][1][0] == '["*"]'

    def test_pull_raises_on_no_result(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = None

        with pytest.raises(MentatError, match="mentat_pull returned no result"):
            db.pull('[*]', 42)


# ---------------------------------------------------------------------------
# Database.pull_many()
# ---------------------------------------------------------------------------


class TestDatabasePullMany:
    def _make_db(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_pg_conn.cursor.return_value = mock_cursor
        conn = Connection(connection=mock_pg_conn)
        return conn.db(), mock_cursor

    def test_pull_many(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('[{"person/name": "Alice"}, {"person/name": "Bob"}]',)

        result = db.pull_many('[*]', [42, 43])

        cursor.execute.assert_called_once_with(
            "SELECT mentat_pull_many(%s, %s)",
            ('[*]', [42, 43]),
        )
        assert len(result) == 2

    def test_pull_many_raises_on_no_result(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = None

        with pytest.raises(MentatError, match="mentat_pull_many returned no result"):
            db.pull_many('[*]', [42])


# ---------------------------------------------------------------------------
# Database.entity()
# ---------------------------------------------------------------------------


class TestDatabaseEntity:
    def _make_db(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_pg_conn.cursor.return_value = mock_cursor
        conn = Connection(connection=mock_pg_conn)
        return conn.db(), mock_cursor

    def test_entity(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = ('{"db/id": 42, "person/name": "Alice"}',)

        result = db.entity(42)

        cursor.execute.assert_called_once_with(
            "SELECT mentat_entity(%s)", (42,)
        )
        assert result["person/name"] == "Alice"

    def test_entity_raises_on_no_result(self):
        db, cursor = self._make_db()
        cursor.fetchone.return_value = None

        with pytest.raises(MentatError, match="mentat_entity returned no result"):
            db.entity(42)


# ---------------------------------------------------------------------------
# Database.as_of() -- time travel
# ---------------------------------------------------------------------------


class TestDatabaseAsOf:
    def test_as_of_tx(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        old_db = db.as_of(1000005)

        assert isinstance(old_db, Database)
        assert old_db._as_of_tx == 1000005
        assert old_db._as_of_instant is None
        assert old_db._conn is conn

    def test_as_of_datetime(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        dt = datetime(2024, 6, 15, 12, 0, 0)
        old_db = db.as_of(dt)

        assert old_db._as_of_instant == dt
        assert old_db._as_of_tx is None

    def test_as_of_injects_temporal_params_tx(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = ('[]',)
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)
        db = conn.db().as_of(1000005)

        db.q('[:find ?e :where [?e :person/name]]')

        call_args = mock_cursor.execute.call_args
        inputs = json.loads(call_args[0][1][1])
        assert inputs["as_of_tx"] == 1000005

    def test_as_of_injects_temporal_params_instant(self):
        mock_pg_conn = MagicMock()
        mock_cursor = MagicMock()
        mock_cursor.fetchone.return_value = ('[]',)
        mock_pg_conn.cursor.return_value = mock_cursor

        conn = Connection(connection=mock_pg_conn)
        dt = datetime(2024, 6, 15, 12, 0, 0)
        db = conn.db().as_of(dt)

        db.q('[:find ?e :where [?e :person/name]]')

        call_args = mock_cursor.execute.call_args
        inputs = json.loads(call_args[0][1][1])
        assert "as_of_instant" in inputs

    def test_as_of_does_not_mutate_original(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        old_db = db.as_of(1000005)

        assert db._as_of_tx is None
        assert old_db._as_of_tx == 1000005

    def test_chained_as_of(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        db1 = db.as_of(100)
        db2 = db1.as_of(200)

        assert db1._as_of_tx == 100
        assert db2._as_of_tx == 200


# ---------------------------------------------------------------------------
# Connection._list_to_edn() -- EDN conversion
# ---------------------------------------------------------------------------


class TestListToEdn:
    def test_simple_string_value(self):
        edn = Connection._list_to_edn([{":person/name": "Alice"}])
        assert edn == '[{:person/name "Alice"}]'

    def test_keyword_value(self):
        edn = Connection._list_to_edn([{":db/valueType": ":db.type/string"}])
        assert edn == "[{:db/valueType :db.type/string}]"

    def test_integer_value(self):
        edn = Connection._list_to_edn([{":person/age": 30}])
        assert edn == "[{:person/age 30}]"

    def test_float_value(self):
        edn = Connection._list_to_edn([{":measurement/weight": 72.5}])
        assert edn == "[{:measurement/weight 72.5}]"

    def test_boolean_value(self):
        edn = Connection._list_to_edn([{":person/active": True}])
        assert edn == "[{:person/active true}]"

    def test_none_value(self):
        edn = Connection._list_to_edn([{":person/notes": None}])
        assert edn == "[{:person/notes nil}]"

    def test_multiple_entries(self):
        edn = Connection._list_to_edn([
            {":person/name": "Alice"},
            {":person/name": "Bob"},
        ])
        assert edn == '[{:person/name "Alice"} {:person/name "Bob"}]'

    def test_multiple_attrs(self):
        edn = Connection._list_to_edn([{
            ":person/name": "Alice",
            ":person/age": 30,
        }])
        assert ":person/name" in edn
        assert ":person/age" in edn
        assert '"Alice"' in edn
        assert "30" in edn

    def test_key_without_colon_prefix(self):
        edn = Connection._list_to_edn([{"person/name": "Alice"}])
        assert edn == '[{:person/name "Alice"}]'

    def test_string_with_quotes(self):
        edn = Connection._list_to_edn([{":person/bio": 'She said "hello"'}])
        assert r'\"hello\"' in edn

    def test_empty_list(self):
        edn = Connection._list_to_edn([])
        assert edn == "[]"


# ---------------------------------------------------------------------------
# Database._build_inputs() -- internal
# ---------------------------------------------------------------------------


class TestBuildInputs:
    def test_no_inputs(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        result = db._build_inputs(())
        assert result == {}

    def test_dict_input(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        result = db._build_inputs(({"name": "Alice"},))
        assert result == {"name": "Alice"}

    def test_positional_inputs(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db()

        result = db._build_inputs(("Alice", 30))
        assert result == {"args": ["Alice", 30]}

    def test_as_of_tx_injected(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db().as_of(1000)

        result = db._build_inputs(())
        assert result == {"as_of_tx": 1000}

    def test_as_of_instant_injected(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        dt = datetime(2024, 1, 15)
        db = conn.db().as_of(dt)

        result = db._build_inputs(())
        assert "as_of_instant" in result
        assert "2024-01-15" in result["as_of_instant"]

    def test_as_of_combined_with_inputs(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db().as_of(500)

        result = db._build_inputs(({"name": "Alice"},))
        assert result["name"] == "Alice"
        assert result["as_of_tx"] == 500

    def test_does_not_mutate_input_dict(self):
        mock_conn = MagicMock()
        conn = Connection(connection=mock_conn)
        db = conn.db().as_of(500)

        original = {"name": "Alice"}
        result = db._build_inputs((original,))

        assert "as_of_tx" not in original
        assert result["as_of_tx"] == 500


# ---------------------------------------------------------------------------
# Import and __all__
# ---------------------------------------------------------------------------


class TestImports:
    def test_import_from_package(self):
        from pg_mentat import Connection, Database, MentatError

        assert Connection is not None
        assert Database is not None
        assert MentatError is not None

    def test_version(self):
        import pg_mentat

        assert hasattr(pg_mentat, "__version__")
        assert pg_mentat.__version__ == "0.1.0"
