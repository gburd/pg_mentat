"""pg_mentat -- Idiomatic Python client for pg_mentat.

Direct PostgreSQL access using psycopg2. No mentatd daemon required.

Usage::

    from pg_mentat import Connection

    conn = Connection("dbname=postgres")
    conn.transact('[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]')
    conn.transact('[{:person/name "Alice"}]')

    db = conn.db()
    results = db.q('[:find ?e ?name :where [?e :person/name ?name]]')
    print(results)

    conn.close()
"""

from pg_mentat.client import (
    Connection,
    Database,
    MentatError,
)

__all__ = [
    "Connection",
    "Database",
    "MentatError",
]

__version__ = "0.1.0"
