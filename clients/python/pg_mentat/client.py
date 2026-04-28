"""
Datomic-compatible Python client for pg_mentat.

Implements the Datomic Client API protocol over WebSocket connections
using Transit+JSON encoding. Provides both synchronous and async interfaces.

Usage::

    import pg_mentat

    c = pg_mentat.client(endpoint="ws://localhost:8080/ws")
    conn = pg_mentat.connect(c, db_name="my-db")
    database = pg_mentat.db(conn)
    results = pg_mentat.q('[:find ?e ?name :where [?e :person/name ?name]]', database)
    pg_mentat.transact(conn, tx_data='[{:person/name "Alice"}]')
"""

from __future__ import annotations

import json
import threading
import uuid
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any, Optional, Union

try:
    import websocket as _ws_mod  # websocket-client package
except ImportError:
    _ws_mod = None

try:
    import websockets  # websockets package (async)
except ImportError:
    websockets = None


# ---------------------------------------------------------------------------
# Transit+JSON encoding / decoding
# ---------------------------------------------------------------------------

class Keyword:
    """Represents a Clojure/EDN keyword like :db/name."""

    __slots__ = ("namespace", "name")

    def __init__(self, name: str, namespace: str | None = None):
        if namespace is None and "/" in name and not name.startswith("/"):
            parts = name.split("/", 1)
            self.namespace = parts[0]
            self.name = parts[1]
        else:
            self.namespace = namespace
            self.name = name

    def __repr__(self) -> str:
        return f":{self}" if self.namespace else f":{self.name}"

    def __str__(self) -> str:
        return f"{self.namespace}/{self.name}" if self.namespace else self.name

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Keyword):
            return self.namespace == other.namespace and self.name == other.name
        return NotImplemented

    def __hash__(self) -> int:
        return hash((self.namespace, self.name))


class Symbol:
    """Represents a Clojure/EDN symbol."""

    __slots__ = ("name",)

    def __init__(self, name: str):
        self.name = name

    def __repr__(self) -> str:
        return self.name

    def __eq__(self, other: object) -> bool:
        if isinstance(other, Symbol):
            return self.name == other.name
        return NotImplemented

    def __hash__(self) -> int:
        return hash(self.name)


def _transit_encode_value(v: Any) -> Any:
    """Encode a Python value as a Transit+JSON-compatible structure."""
    if v is None:
        return None
    if isinstance(v, bool):
        return v
    if isinstance(v, Keyword):
        return f"~:{v}"
    if isinstance(v, Symbol):
        return f"~${v.name}"
    if isinstance(v, int):
        if v > 2_147_483_647 or v < -2_147_483_648:
            return f"~i{v}"
        return v
    if isinstance(v, float):
        return v
    if isinstance(v, str):
        if v.startswith("~") or v.startswith("^"):
            return f"~{v}"
        return v
    if isinstance(v, uuid.UUID):
        return f"~u{v}"
    if isinstance(v, dict):
        result = ["^ "]
        for k, val in v.items():
            result.append(_transit_encode_value(k))
            result.append(_transit_encode_value(val))
        return result
    if isinstance(v, (list, tuple)):
        return [_transit_encode_value(item) for item in v]
    if isinstance(v, set):
        return ["~#set", [_transit_encode_value(item) for item in v]]
    return str(v)


def transit_encode(m: Any) -> str:
    """Encode a value as a Transit+JSON string."""
    return json.dumps(_transit_encode_value(m), separators=(",", ":"))


def _transit_decode_tagged(s: str) -> Any:
    """Decode a Transit tagged string."""
    if s.startswith("~:"):
        return Keyword(s[2:])
    if s.startswith("~$"):
        return Symbol(s[2:])
    if s.startswith("~i"):
        return int(s[2:])
    if s.startswith("~u"):
        return uuid.UUID(s[2:])
    if s.startswith("~m"):
        return int(s[2:])  # milliseconds since epoch
    if s == "~zNaN":
        return float("nan")
    if s == "~zINF":
        return float("inf")
    if s == "~z-INF":
        return float("-inf")
    if s.startswith("~~"):
        return s[1:]  # escaped tilde
    if s.startswith("~^"):
        return "^" + s[2:]  # escaped caret
    return s


def transit_decode(v: Any) -> Any:
    """Decode a Transit+JSON-parsed value to Python data."""
    if v is None:
        return None
    if isinstance(v, bool):
        return v
    if isinstance(v, (int, float)):
        return v
    if isinstance(v, str):
        return _transit_decode_tagged(v)
    if isinstance(v, list):
        if len(v) > 0 and v[0] == "^ ":
            # cmap: ["^ ", k1, v1, k2, v2, ...]
            result = {}
            i = 1
            while i + 1 < len(v):
                key = transit_decode(v[i])
                val = transit_decode(v[i + 1])
                result[key] = val
                i += 2
            return result
        if len(v) == 2 and isinstance(v[0], str):
            tag = v[0]
            if tag == "~#list":
                return [transit_decode(item) for item in v[1]]
            if tag == "~#set":
                return {transit_decode(item) for item in v[1]}
        return [transit_decode(item) for item in v]
    if isinstance(v, dict):
        return {transit_decode(k): transit_decode(val) for k, val in v.items()}
    return v


def _parse_transit_json(s: str) -> Any:
    """Parse a Transit+JSON string to Python data."""
    return transit_decode(json.loads(s))


# ---------------------------------------------------------------------------
# WebSocket connection (synchronous, using websocket-client)
# ---------------------------------------------------------------------------

class _WsConnection:
    """WebSocket connection that handles Transit+JSON messages."""

    def __init__(self, endpoint: str, api_key: str | None = None):
        if _ws_mod is None:
            raise ImportError(
                "websocket-client package required. Install with: "
                "pip install websocket-client"
            )
        self._endpoint = endpoint
        self._api_key = api_key
        self._pending: dict[str, threading.Event] = {}
        self._results: dict[str, Any] = {}
        self._general_queue: list[Any] = []
        self._lock = threading.Lock()
        self._closed = False
        self._session_id: str | None = None

        header = []
        if api_key:
            header.append(f"Authorization: Bearer {api_key}")

        self._ws = _ws_mod.WebSocketApp(
            endpoint,
            header=header or None,
            on_message=self._on_message,
            on_error=self._on_error,
            on_close=self._on_close,
        )

        self._thread = threading.Thread(target=self._ws.run_forever, daemon=True)
        self._thread.start()

        # Wait for welcome message
        self._welcome_event = threading.Event()
        if not self._welcome_event.wait(timeout=10):
            raise ConnectionError(
                f"Timeout waiting for WebSocket welcome from {endpoint}"
            )

    def _on_message(self, ws: Any, message: str) -> None:
        parsed = _parse_transit_json(message)
        # Check for welcome message
        if isinstance(parsed, dict):
            type_val = parsed.get(Keyword("type"))
            if type_val == Keyword("datomic.client/session"):
                self._session_id = parsed.get(Keyword("session-id"))
                self._welcome_event.set()
                return

        # Route by request-id
        rid = None
        if isinstance(parsed, dict):
            rid = parsed.get(Keyword("request-id"))
            if rid is None:
                # Try string key too
                rid = parsed.get("request-id")
        if rid:
            with self._lock:
                self._results[rid] = parsed
                evt = self._pending.get(rid)
                if evt:
                    evt.set()
        else:
            with self._lock:
                self._general_queue.append(parsed)
                # Also signal welcome in case it was a plain result
                if not self._welcome_event.is_set():
                    self._welcome_event.set()

    def _on_error(self, ws: Any, error: Exception) -> None:
        pass  # Errors will surface as timeouts

    def _on_close(self, ws: Any, close_status_code: int | None,
                  close_msg: str | None) -> None:
        self._closed = True
        # Wake up any pending requests
        with self._lock:
            for evt in self._pending.values():
                evt.set()

    def send_request(self, request: dict[str, Any],
                     timeout: float = 30.0) -> Any:
        """Send a Transit+JSON request and wait for the response."""
        if self._closed:
            raise ConnectionError("WebSocket connection is closed")

        request_id = str(uuid.uuid4())
        request[Keyword("request-id")] = request_id

        evt = threading.Event()
        with self._lock:
            self._pending[request_id] = evt

        msg = transit_encode(request)
        self._ws.send(msg)

        if not evt.wait(timeout=timeout):
            with self._lock:
                self._pending.pop(request_id, None)
            raise TimeoutError(f"Request timed out after {timeout}s")

        with self._lock:
            self._pending.pop(request_id, None)
            result = self._results.pop(request_id, None)

        if result is None:
            raise ConnectionError("Connection closed before response received")

        # Check for error
        error = result.get(Keyword("error"))
        if error:
            msg_text = "Server error"
            category = "fault"
            if isinstance(error, dict):
                msg_text = error.get(
                    Keyword("cognitect.anomalies/message"), msg_text
                )
                cat = error.get(Keyword("cognitect.anomalies/category"))
                if cat:
                    category = str(cat)
            raise PgMentatError(msg_text, category=category, response=result)

        return result.get(Keyword("result"))

    def close(self) -> None:
        """Close the WebSocket connection."""
        if not self._closed:
            self._closed = True
            self._ws.close()


# ---------------------------------------------------------------------------
# Async WebSocket connection (using websockets package)
# ---------------------------------------------------------------------------

class _AsyncWsConnection:
    """Async WebSocket connection using the websockets library."""

    def __init__(self) -> None:
        self._ws = None
        self._session_id: str | None = None

    async def connect(self, endpoint: str,
                      api_key: str | None = None) -> None:
        if websockets is None:
            raise ImportError(
                "websockets package required for async. Install with: "
                "pip install websockets"
            )
        extra_headers = {}
        if api_key:
            extra_headers["Authorization"] = f"Bearer {api_key}"

        self._ws = await websockets.connect(
            endpoint,
            additional_headers=extra_headers or None,
        )
        # Read welcome message
        raw = await self._ws.recv()
        welcome = _parse_transit_json(raw)
        if isinstance(welcome, dict):
            self._session_id = welcome.get(Keyword("session-id"))

    async def send_request(self, request: dict[str, Any]) -> Any:
        """Send request and wait for response."""
        if self._ws is None:
            raise ConnectionError("Not connected")

        request_id = str(uuid.uuid4())
        request[Keyword("request-id")] = request_id

        msg = transit_encode(request)
        await self._ws.send(msg)

        raw = await self._ws.recv()
        result = _parse_transit_json(raw)

        # Check for error
        if isinstance(result, dict):
            error = result.get(Keyword("error"))
            if error and isinstance(error, dict):
                msg_text = error.get(
                    Keyword("cognitect.anomalies/message"), "Server error"
                )
                category = str(
                    error.get(
                        Keyword("cognitect.anomalies/category"),
                        "fault",
                    )
                )
                raise PgMentatError(
                    msg_text, category=category, response=result
                )

        if isinstance(result, dict):
            return result.get(Keyword("result"))
        return result

    async def close(self) -> None:
        if self._ws:
            await self._ws.close()
            self._ws = None


# ---------------------------------------------------------------------------
# Error types
# ---------------------------------------------------------------------------

class PgMentatError(Exception):
    """Error from the pg_mentat server."""

    def __init__(self, message: str, category: str = "fault",
                 response: Any = None):
        super().__init__(message)
        self.category = category
        self.response = response


# ---------------------------------------------------------------------------
# Datomic Client API data types
# ---------------------------------------------------------------------------

@dataclass
class Client:
    """A pg_mentat client configuration."""
    endpoint: str
    api_key: str | None = None


@dataclass
class Connection:
    """A connection to a specific database."""
    client: Client
    _ws: _WsConnection = field(repr=False)
    db_name: str = ""
    connection_id: str = ""

    def close(self) -> None:
        """Close the connection."""
        self._ws.close()


@dataclass
class Db:
    """An immutable database value at a point in time."""
    connection: Connection
    db_name: str = ""
    database_id: str = ""
    t: int = 0
    next_t: int = 0
    as_of_t: int | None = None
    since_t: int | None = None
    is_history: bool = False


# ---------------------------------------------------------------------------
# Datomic Client API functions
# ---------------------------------------------------------------------------

def client(*, endpoint: str, api_key: str | None = None,
           **kwargs: Any) -> Client:
    """Create a pg_mentat client.

    Drop-in replacement for datomic.client.api/client.

    Args:
        endpoint: WebSocket URL (e.g., "ws://localhost:8080/ws").
        api_key: Optional API key for authentication.

    Returns:
        A Client object for use with connect(), list_databases(), etc.
    """
    return Client(endpoint=endpoint, api_key=api_key)


def connect(c: Client, *, db_name: str) -> Connection:
    """Connect to a database.

    Drop-in replacement for datomic.client.api/connect.

    Args:
        c: Client from client().
        db_name: Name of the database to connect to.

    Returns:
        A Connection object.
    """
    ws = _WsConnection(c.endpoint, api_key=c.api_key)
    result = ws.send_request({
        Keyword("op"): Keyword("connect"),
        Keyword("args"): {
            Keyword("db-name"): db_name,
        },
    })
    conn_id = ""
    if isinstance(result, dict):
        conn_id = str(result.get(Keyword("database-id"), ""))
    return Connection(client=c, _ws=ws, db_name=db_name,
                      connection_id=conn_id)


def db(conn: Connection) -> Db:
    """Get the current database value.

    Drop-in replacement for datomic.client.api/db.

    Args:
        conn: Connection from connect().

    Returns:
        An immutable Db value for use with q(), pull(), etc.
    """
    result = conn._ws.send_request({
        Keyword("op"): Keyword("db"),
        Keyword("args"): {
            Keyword("db-name"): conn.db_name,
        },
    })
    t = 0
    next_t = 0
    database_id = ""
    if isinstance(result, dict):
        t = result.get(Keyword("t"), 0)
        next_t = result.get(Keyword("next-t"), 0)
        database_id = str(result.get(Keyword("database-id"), ""))
    return Db(
        connection=conn,
        db_name=conn.db_name,
        database_id=database_id,
        t=t,
        next_t=next_t,
    )


def q(query: str, database: Db, *inputs: Any,
      timeout: float | None = None) -> Any:
    """Execute a Datalog query.

    Drop-in replacement for datomic.client.api/q.

    Args:
        query: Datalog query string (EDN format).
        database: Database value from db().
        *inputs: Optional query input values.
        timeout: Optional timeout in seconds.

    Returns:
        Query results (typically a list of tuples).
    """
    args: dict[Any, Any] = {
        Keyword("query"): query,
        Keyword("args"): list(inputs),
    }
    if database.as_of_t is not None:
        args[Keyword("as-of")] = database.as_of_t
    if database.since_t is not None:
        args[Keyword("since")] = database.since_t
    if database.is_history:
        args[Keyword("history")] = True

    request = {
        Keyword("op"): Keyword("q"),
        Keyword("args"): args,
    }
    return database.connection._ws.send_request(
        request, timeout=timeout or 30.0
    )


def transact(conn: Connection, *, tx_data: str) -> Any:
    """Execute a transaction.

    Drop-in replacement for datomic.client.api/transact.

    Args:
        conn: Connection from connect().
        tx_data: EDN string of transaction data.

    Returns:
        Transaction result with :db-before, :db-after, :tx-data, :tempids.
    """
    return conn._ws.send_request({
        Keyword("op"): Keyword("transact"),
        Keyword("args"): {
            Keyword("connection-id"): conn.connection_id,
            Keyword("tx-data"): tx_data,
        },
    })


def pull(database: Db, pattern: str, eid: int) -> Any:
    """Pull entity attributes.

    Drop-in replacement for datomic.client.api/pull.

    Args:
        database: Database value from db().
        pattern: Pull pattern string (EDN format, e.g. "[*]").
        eid: Entity ID.

    Returns:
        Map of entity attributes.
    """
    return database.connection._ws.send_request({
        Keyword("op"): Keyword("pull"),
        Keyword("args"): {
            Keyword("pattern"): pattern,
            Keyword("entity-id"): eid,
        },
    })


def pull_many(database: Db, pattern: str, eids: list[int]) -> list[Any]:
    """Pull attributes for multiple entities.

    Args:
        database: Database value.
        pattern: Pull pattern string.
        eids: List of entity IDs.

    Returns:
        List of entity attribute maps.
    """
    return [pull(database, pattern, eid) for eid in eids]


def datoms(database: Db, *, index: str,
           components: list[str] | None = None) -> Any:
    """Access raw datoms from an index.

    Drop-in replacement for datomic.client.api/datoms.

    Args:
        database: Database value.
        index: Index name (":eavt", ":aevt", ":avet", ":vaet").
        components: Optional index component filters.

    Returns:
        Collection of datom tuples.
    """
    return database.connection._ws.send_request({
        Keyword("op"): Keyword("datoms"),
        Keyword("args"): {
            Keyword("index"): index,
            Keyword("components"): components or [],
        },
    })


def with_db(database: Db, *, tx_data: str) -> Any:
    """Speculative transaction (Datomic d/with).

    Applies tx_data speculatively without committing.

    Args:
        database: Database value.
        tx_data: EDN string of transaction data.

    Returns:
        Speculative result with :db-after and :tx-data.
    """
    return database.connection._ws.send_request({
        Keyword("op"): Keyword("with"),
        Keyword("args"): {
            Keyword("tx-data"): tx_data,
        },
    })


def tx_range(conn: Connection, *, start: int | None = None,
             end: int | None = None) -> Any:
    """Query the transaction log.

    Args:
        conn: Connection.
        start: Optional start transaction ID.
        end: Optional end transaction ID.

    Returns:
        Collection of transaction log entries.
    """
    args: dict[Any, Any] = {}
    if start is not None:
        args[Keyword("start")] = start
    if end is not None:
        args[Keyword("end")] = end
    return conn._ws.send_request({
        Keyword("op"): Keyword("tx-range"),
        Keyword("args"): args,
    })


# ---------------------------------------------------------------------------
# Time-travel database values
# ---------------------------------------------------------------------------

def as_of(database: Db, t: int) -> Db:
    """Return a database value as of a specific transaction.

    Args:
        database: Database value.
        t: Transaction ID.

    Returns:
        New Db filtered to that point in time.
    """
    return Db(
        connection=database.connection,
        db_name=database.db_name,
        database_id=database.database_id,
        t=database.t,
        next_t=database.next_t,
        as_of_t=t,
        since_t=None,
        is_history=False,
    )


def since(database: Db, t: int) -> Db:
    """Return a database value showing only changes since a transaction.

    Args:
        database: Database value.
        t: Transaction ID.

    Returns:
        New Db filtered to changes since t.
    """
    return Db(
        connection=database.connection,
        db_name=database.db_name,
        database_id=database.database_id,
        t=database.t,
        next_t=database.next_t,
        as_of_t=None,
        since_t=t,
        is_history=False,
    )


def history(database: Db) -> Db:
    """Return a database value including all history.

    Args:
        database: Database value.

    Returns:
        New Db with full history (assertions and retractions).
    """
    return Db(
        connection=database.connection,
        db_name=database.db_name,
        database_id=database.database_id,
        t=database.t,
        next_t=database.next_t,
        as_of_t=None,
        since_t=None,
        is_history=True,
    )


# ---------------------------------------------------------------------------
# Catalog operations
# ---------------------------------------------------------------------------

def list_databases(c: Client) -> Any:
    """List available databases.

    Args:
        c: Client.

    Returns:
        List of database name strings.
    """
    ws = _WsConnection(c.endpoint, api_key=c.api_key)
    try:
        return ws.send_request({
            Keyword("op"): Keyword("list-dbs"),
            Keyword("args"): {},
        })
    finally:
        ws.close()


def create_database(c: Client, *, db_name: str) -> Any:
    """Create a new database.

    Args:
        c: Client.
        db_name: Name for the new database.

    Returns:
        True on success.
    """
    ws = _WsConnection(c.endpoint, api_key=c.api_key)
    try:
        return ws.send_request({
            Keyword("op"): Keyword("create-db"),
            Keyword("args"): {
                Keyword("db-name"): db_name,
            },
        })
    finally:
        ws.close()


def delete_database(c: Client, *, db_name: str) -> Any:
    """Delete a database.

    Args:
        c: Client.
        db_name: Name of the database to delete.

    Returns:
        True on success.
    """
    ws = _WsConnection(c.endpoint, api_key=c.api_key)
    try:
        return ws.send_request({
            Keyword("op"): Keyword("delete-db"),
            Keyword("args"): {
                Keyword("db-name"): db_name,
            },
        })
    finally:
        ws.close()
