"""
pg_mentat Python client -- Direct PostgreSQL access.

No mentatd daemon required. Connect directly to PostgreSQL and call
the pg_mentat extension functions via standard SQL.

Requirements:
    pip install psycopg2-binary  # or psycopg2 for production

Usage:
    from pg_mentat_client import MentatClient

    client = MentatClient("dbname=postgres")
    client.transact('''[
      {:db/ident :person/name
       :db/valueType :db.type/string
       :db/cardinality :db.cardinality/one}
    ]''')
    results = client.query('[:find ?name :where [?e :person/name ?name]]')
"""

import json
from contextlib import contextmanager

try:
    import psycopg2
    import psycopg2.extras
except ImportError:
    raise ImportError(
        "psycopg2 is required: pip install psycopg2-binary"
    )


class MentatError(Exception):
    """Error from a pg_mentat SQL function call."""
    pass


class MentatClient:
    """Direct PostgreSQL client for pg_mentat.

    Calls mentat_transact(), mentat_query(), mentat_pull(), mentat_entity(),
    and mentat_schema() as SQL functions -- no HTTP daemon needed.
    """

    def __init__(self, dsn=None, connection=None, **kwargs):
        """Create a client.

        Args:
            dsn: PostgreSQL connection string, e.g. "dbname=postgres host=localhost".
            connection: An existing psycopg2 connection to reuse.
            **kwargs: Additional keyword arguments passed to psycopg2.connect().
        """
        if connection is not None:
            self._conn = connection
            self._owns_conn = False
        else:
            self._conn = psycopg2.connect(dsn, **kwargs)
            self._conn.autocommit = True
            self._owns_conn = True

    def close(self):
        if self._owns_conn and self._conn and not self._conn.closed:
            self._conn.close()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()

    # -- Core API --------------------------------------------------------

    def transact(self, edn_tx):
        """Execute an EDN transaction.

        Args:
            edn_tx: EDN string, e.g. '[{:db/ident :person/name ...}]'

        Returns:
            Transaction report as a Python dict.
        """
        with self._cursor() as cur:
            cur.execute("SELECT mentat_transact(%s)", (edn_tx,))
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_transact returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def query(self, datalog, inputs=None):
        """Execute a Datalog query.

        Args:
            datalog: Datalog query string.
            inputs: Optional dict of query inputs (passed as JSONB).

        Returns:
            Query results as a Python object (typically a list of tuples).
        """
        inputs_json = json.dumps(inputs or {})
        with self._cursor() as cur:
            cur.execute(
                "SELECT mentat_query(%s, %s::jsonb)",
                (datalog, inputs_json),
            )
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_query returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def pull(self, pattern, entity_id):
        """Pull attributes for an entity.

        Args:
            pattern: Pull pattern string, e.g. '[:person/name :person/email]'
            entity_id: Integer entity ID.

        Returns:
            Entity attributes as a Python dict.
        """
        with self._cursor() as cur:
            cur.execute(
                "SELECT mentat_pull(%s, %s)",
                (pattern, int(entity_id)),
            )
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_pull returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def pull_many(self, pattern, entity_ids):
        """Pull attributes for multiple entities.

        Args:
            pattern: Pull pattern string.
            entity_ids: List of integer entity IDs.

        Returns:
            List of entity attribute dicts.
        """
        with self._cursor() as cur:
            cur.execute(
                "SELECT mentat_pull_many(%s, %s)",
                (pattern, list(entity_ids)),
            )
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_pull_many returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def entity(self, entity_id):
        """Get all attributes of an entity.

        Args:
            entity_id: Integer entity ID.

        Returns:
            Entity as a Python dict.
        """
        with self._cursor() as cur:
            cur.execute("SELECT mentat_entity(%s)", (int(entity_id),))
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_entity returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def schema(self):
        """Return the current schema.

        Returns:
            Schema as a Python dict.
        """
        with self._cursor() as cur:
            cur.execute("SELECT mentat_schema()")
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_schema returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    def explain(self, datalog, inputs=None):
        """Return the query execution plan.

        Args:
            datalog: Datalog query string.
            inputs: Optional dict of query inputs.

        Returns:
            Execution plan as a Python object.
        """
        inputs_json = json.dumps(inputs or {})
        with self._cursor() as cur:
            cur.execute(
                "SELECT mentat_explain(%s, %s::jsonb)",
                (datalog, inputs_json),
            )
            row = cur.fetchone()
            if row is None:
                raise MentatError("mentat_explain returned no result")
            result = row[0]
            if isinstance(result, str):
                return json.loads(result)
            return result

    # -- Helpers ---------------------------------------------------------

    @contextmanager
    def _cursor(self):
        cur = self._conn.cursor()
        try:
            yield cur
        finally:
            cur.close()


# ---------------------------------------------------------------------------
# Example usage
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    import sys

    dsn = sys.argv[1] if len(sys.argv) > 1 else "dbname=postgres"

    with MentatClient(dsn) as m:
        # Define schema
        m.transact("""[
          {:db/ident :person/name
           :db/valueType :db.type/string
           :db/cardinality :db.cardinality/one}
          {:db/ident :person/email
           :db/valueType :db.type/string
           :db/cardinality :db.cardinality/one
           :db/unique :db.unique/identity}
        ]""")

        # Transact data
        m.transact("""[
          {:person/name "Alice"
           :person/email "alice@example.com"}
          {:person/name "Bob"
           :person/email "bob@example.com"}
        ]""")

        # Query
        results = m.query("""
          [:find ?name ?email
           :where
           [?e :person/name ?name]
           [?e :person/email ?email]]
        """)
        print("Query results:", json.dumps(results, indent=2))

        # Schema
        schema = m.schema()
        print("Schema:", json.dumps(schema, indent=2))
