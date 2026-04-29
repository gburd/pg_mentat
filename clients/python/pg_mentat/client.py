"""
pg_mentat Python client -- Direct PostgreSQL access.

Provides an idiomatic Python API with Connection and Database classes.
Connects directly to PostgreSQL and calls pg_mentat extension functions
via standard SQL. No mentatd daemon required.

Requirements:
    pip install psycopg2-binary  # or psycopg2 for production

Usage::

    from pg_mentat import Connection

    conn = Connection("dbname=postgres")
    db = conn.db()
    results = db.q('[:find ?e ?name :where [?e :person/name ?name]]')
    entity = db.pull('[*]', 42)
    conn.close()
"""

from __future__ import annotations

import json
from contextlib import contextmanager
from datetime import datetime
from typing import Any, Dict, Iterator, List, Optional, Sequence, Union

try:
    import psycopg2
    import psycopg2.extras
except ImportError:
    raise ImportError(
        "psycopg2 is required: pip install psycopg2-binary"
    )


class MentatError(Exception):
    """Error raised by pg_mentat operations."""

    def __init__(self, message: str, detail: Optional[str] = None):
        super().__init__(message)
        self.detail = detail


class Database:
    """An immutable database value at a point in time.

    Database objects are lightweight snapshots. They do not hold connections
    or cursors; instead, they borrow the parent Connection when executing
    queries. Obtain a Database via ``Connection.db()`` or by calling
    ``as_of()`` on an existing Database.

    Typical usage::

        db = conn.db()
        results = db.q('[:find ?e :where [?e :person/name "Alice"]]')
        old_db = db.as_of(1000005)
        old_results = old_db.q('[:find ?e :where [?e :person/name "Alice"]]')
    """

    def __init__(
        self,
        connection: "Connection",
        as_of_tx: Optional[int] = None,
        as_of_instant: Optional[datetime] = None,
    ) -> None:
        self._conn = connection
        self._as_of_tx = as_of_tx
        self._as_of_instant = as_of_instant

    def as_of(self, tx_or_instant: Union[int, datetime]) -> "Database":
        """Return a new Database value filtered to a point in time.

        Time-travel queries allow you to see the database as it existed at
        a specific transaction or timestamp.

        Args:
            tx_or_instant: Either a transaction ID (int) or a datetime.

        Returns:
            A new Database that queries data as of that point in time.
        """
        if isinstance(tx_or_instant, datetime):
            return Database(
                self._conn,
                as_of_instant=tx_or_instant,
            )
        return Database(
            self._conn,
            as_of_tx=int(tx_or_instant),
        )

    def q(self, query: str, *inputs: Any) -> Any:
        """Execute a Datalog query.

        Args:
            query: Datalog query string in EDN format, e.g.
                '[:find ?e ?name :where [?e :person/name ?name]]'
            *inputs: Optional positional query inputs. If a single dict is
                passed it is used as the inputs JSONB map. Otherwise inputs
                are passed as a JSON array under the "args" key.

        Returns:
            Query results, typically a list of tuples (lists in Python).
        """
        inputs_dict = self._build_inputs(inputs)
        with self._conn._cursor() as cur:
            cur.execute(
                "SELECT mentat_query(%s, %s::jsonb)",
                (query, json.dumps(inputs_dict)),
            )
            return self._fetch_json(cur, "mentat_query")

    def pull(self, pattern: Union[str, List[Any]], eid: int) -> Dict[str, Any]:
        """Pull entity attributes matching a pattern.

        Args:
            pattern: Pull pattern as a string (EDN) or list.
                Examples: '[*]', '[:person/name :person/email]'
            eid: Entity ID to pull.

        Returns:
            Dict of entity attributes.
        """
        pattern_str = pattern if isinstance(pattern, str) else json.dumps(pattern)
        with self._conn._cursor() as cur:
            cur.execute(
                "SELECT mentat_pull(%s, %s)",
                (pattern_str, int(eid)),
            )
            return self._fetch_json(cur, "mentat_pull")

    def pull_many(
        self, pattern: Union[str, List[Any]], eids: Sequence[int]
    ) -> List[Dict[str, Any]]:
        """Pull attributes for multiple entities.

        Args:
            pattern: Pull pattern string or list.
            eids: Sequence of entity IDs.

        Returns:
            List of entity attribute dicts.
        """
        pattern_str = pattern if isinstance(pattern, str) else json.dumps(pattern)
        with self._conn._cursor() as cur:
            cur.execute(
                "SELECT mentat_pull_many(%s, %s)",
                (pattern_str, list(eids)),
            )
            return self._fetch_json(cur, "mentat_pull_many")

    def entity(self, eid: int) -> Dict[str, Any]:
        """Get all attributes of an entity as a dict.

        Args:
            eid: Entity ID.

        Returns:
            Dict mapping attribute keywords to their values.
        """
        with self._conn._cursor() as cur:
            cur.execute("SELECT mentat_entity(%s)", (int(eid),))
            return self._fetch_json(cur, "mentat_entity")

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _build_inputs(self, inputs: tuple) -> Dict[str, Any]:
        """Build the inputs JSONB for a query, including temporal options."""
        if len(inputs) == 1 and isinstance(inputs[0], dict):
            base = dict(inputs[0])
        elif inputs:
            base = {"args": list(inputs)}
        else:
            base = {}

        # Inject temporal parameters
        if self._as_of_tx is not None:
            base["as_of_tx"] = self._as_of_tx
        elif self._as_of_instant is not None:
            base["as_of_instant"] = self._as_of_instant.isoformat()

        return base

    @staticmethod
    def _fetch_json(cur: Any, func_name: str) -> Any:
        """Fetch a single JSON result from the cursor."""
        row = cur.fetchone()
        if row is None:
            raise MentatError(f"{func_name} returned no result")
        result = row[0]
        if isinstance(result, str):
            return json.loads(result)
        return result


class Connection:
    """A connection to a pg_mentat-enabled PostgreSQL database.

    The Connection wraps a psycopg2 connection and provides methods to
    transact data and obtain immutable Database snapshots for querying.

    Supports use as a context manager::

        with Connection("dbname=postgres") as conn:
            conn.transact('[{:person/name "Alice"}]')
            db = conn.db()
            print(db.q('[:find ?name :where [?e :person/name ?name]]'))
    """

    def __init__(
        self,
        dsn: Optional[str] = None,
        connection: Optional[Any] = None,
        **kwargs: Any,
    ) -> None:
        """Create a connection to pg_mentat.

        Args:
            dsn: PostgreSQL connection string (e.g. "dbname=postgres host=localhost").
            connection: An existing psycopg2 connection to reuse.
                If provided, the Connection will not close it on exit.
            **kwargs: Additional keyword arguments passed to psycopg2.connect().
        """
        if connection is not None:
            self._conn = connection
            self._owns_conn = False
        else:
            self._conn = psycopg2.connect(dsn, **kwargs)
            self._conn.autocommit = True
            self._owns_conn = True

    def db(self) -> Database:
        """Get the current database value.

        Returns an immutable Database snapshot representing the current
        state of the database. Use this for queries.

        Returns:
            A Database instance for querying.
        """
        return Database(self)

    def transact(self, tx_data: Union[str, List[Dict[str, Any]]]) -> Dict[str, Any]:
        """Execute a transaction.

        Args:
            tx_data: Either an EDN string or a list of dicts representing
                transaction data.

                EDN example::

                    conn.transact('[{:person/name "Alice"}]')

                List example::

                    conn.transact([{":db/ident": ":person/name",
                                    ":db/valueType": ":db.type/string",
                                    ":db/cardinality": ":db.cardinality/one"}])

        Returns:
            Transaction report as a dict with tx metadata.
        """
        if isinstance(tx_data, list):
            edn_tx = self._list_to_edn(tx_data)
        else:
            edn_tx = tx_data

        with self._cursor() as cur:
            cur.execute("SELECT mentat_transact(%s)", (edn_tx,))
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_transact returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def close(self) -> None:
        """Close the underlying PostgreSQL connection.

        Only closes if this Connection owns the connection (i.e., it was
        not passed in via the ``connection`` parameter).
        """
        if self._owns_conn and self._conn and not self._conn.closed:
            self._conn.close()

    @property
    def closed(self) -> bool:
        """Whether the underlying connection is closed."""
        return self._conn.closed != 0

    # ------------------------------------------------------------------
    # Context manager
    # ------------------------------------------------------------------

    def __enter__(self) -> "Connection":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    @contextmanager
    def _cursor(self) -> Iterator[Any]:
        """Create and yield a cursor, closing it afterward."""
        cur = self._conn.cursor()
        try:
            yield cur
        finally:
            cur.close()

    @staticmethod
    def _list_to_edn(tx_data: List[Dict[str, Any]]) -> str:
        """Convert a list of dicts to EDN transaction format.

        This provides a convenience for Python users who prefer dicts
        over raw EDN strings. Keys starting with ':' are treated as
        keywords.
        """
        parts = []
        for item in tx_data:
            entries = []
            for k, v in item.items():
                key_str = k if k.startswith(":") else ":" + k
                if isinstance(v, str) and v.startswith(":"):
                    entries.append("{} {}".format(key_str, v))
                elif isinstance(v, str):
                    entries.append('{} "{}"'.format(key_str, v.replace('"', '\\"')))
                elif isinstance(v, bool):
                    entries.append("{} {}".format(key_str, "true" if v else "false"))
                elif isinstance(v, (int, float)):
                    entries.append("{} {}".format(key_str, v))
                elif v is None:
                    entries.append("{} nil".format(key_str))
                else:
                    entries.append('{} "{}"'.format(key_str, str(v)))
            parts.append("{" + " ".join(entries) + "}")
        return "[" + " ".join(parts) + "]"
