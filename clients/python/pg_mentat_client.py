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

    # -- Native SQL view access ------------------------------------------

    def facts(self, entity_id=None, attribute=None, store="default"):
        """Query the facts view for human-readable EAVT data.

        Args:
            entity_id: Optional entity ID filter.
            attribute: Optional attribute ident filter (e.g. ':person/name').
            store: Store name (default: "default").

        Returns:
            List of fact dicts with keys: entity_id, attribute, value,
            value_type, tx, tx_time.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, attribute, value, value_type, tx, tx_time FROM {}.facts".format(schema)
        params = []
        wheres = []
        if entity_id is not None:
            wheres.append("entity_id = %s")
            params.append(entity_id)
        if attribute is not None:
            wheres.append("attribute = %s")
            params.append(attribute)
        if wheres:
            sql += " WHERE " + " AND ".join(wheres)
        sql += " ORDER BY entity_id, attribute"
        with self._cursor() as cur:
            cur.execute(sql, params)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def text_values(self, attribute=None, store="default"):
        """Query text_values view.

        Args:
            attribute: Optional attribute ident filter.
            store: Store name.

        Returns:
            List of dicts with keys: entity_id, attribute, value, tx.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, attribute, value, tx FROM {}.text_values".format(schema)
        params = []
        if attribute is not None:
            sql += " WHERE attribute = %s"
            params.append(attribute)
        with self._cursor() as cur:
            cur.execute(sql, params)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def numeric_values(self, attribute=None, store="default"):
        """Query numeric_values view.

        Args:
            attribute: Optional attribute ident filter.
            store: Store name.

        Returns:
            List of dicts with keys: entity_id, attribute, value, tx.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, attribute, value, tx FROM {}.numeric_values".format(schema)
        params = []
        if attribute is not None:
            sql += " WHERE attribute = %s"
            params.append(attribute)
        with self._cursor() as cur:
            cur.execute(sql, params)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def entity_references(self, source=None, target=None, store="default"):
        """Query entity_references view for relationship navigation.

        Args:
            source: Optional source entity ID filter.
            target: Optional target entity ID filter.
            store: Store name.

        Returns:
            List of dicts with keys: source_entity, attribute,
            target_entity, target_ident, tx.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT source_entity, attribute, target_entity, target_ident, tx FROM {}.entity_references".format(schema)
        params = []
        wheres = []
        if source is not None:
            wheres.append("source_entity = %s")
            params.append(source)
        if target is not None:
            wheres.append("target_entity = %s")
            params.append(target)
        if wheres:
            sql += " WHERE " + " AND ".join(wheres)
        with self._cursor() as cur:
            cur.execute(sql, params)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def entity_history(self, entity_id=None, store="default"):
        """Query entity_history view for temporal data.

        Args:
            entity_id: Optional entity ID filter.
            store: Store name.

        Returns:
            List of dicts with keys: entity_id, attribute, value,
            value_type, tx, tx_time, operation.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, attribute, value, value_type, tx, tx_time, operation FROM {}.entity_history".format(schema)
        params = []
        if entity_id is not None:
            sql += " WHERE entity_id = %s"
            params.append(entity_id)
        sql += " ORDER BY tx DESC"
        with self._cursor() as cur:
            cur.execute(sql, params)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def tx_log(self, limit=100, store="default"):
        """Query tx_log view for transaction history.

        Args:
            limit: Maximum number of transactions to return.
            store: Store name.

        Returns:
            List of dicts with keys: tx, tx_time, datom_count.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT tx, tx_time, datom_count FROM {}.tx_log ORDER BY tx DESC LIMIT %s".format(schema)
        with self._cursor() as cur:
            cur.execute(sql, (limit,))
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def schema_summary(self, store="default"):
        """Query schema_summary view for attribute usage statistics.

        Args:
            store: Store name.

        Returns:
            List of dicts with attribute usage info.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT * FROM {}.schema_summary".format(schema)
        with self._cursor() as cur:
            cur.execute(sql)
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def lookup_entity(self, attribute, value, store="default"):
        """Find entities by attribute value using the lookup_entity function.

        Args:
            attribute: Attribute ident (e.g. ':person/name').
            value: Value to search for (as string).
            store: Store name.

        Returns:
            List of dicts with keys: entity_id, tx.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, tx FROM {}.lookup_entity(%s, %s)".format(schema)
        with self._cursor() as cur:
            cur.execute(sql, (attribute, value))
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    def entity_value(self, entity_id, attribute, store="default"):
        """Get a single attribute value for an entity.

        Args:
            entity_id: Entity ID.
            attribute: Attribute ident (e.g. ':person/name').
            store: Store name.

        Returns:
            The value as a string, or None if not found.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT {}.entity_value(%s, %s)".format(schema)
        with self._cursor() as cur:
            cur.execute(sql, (entity_id, attribute))
            row = cur.fetchone()
            return row[0] if row else None

    def find_text(self, search_query, store="default"):
        """Full-text search across all text values.

        Args:
            search_query: Search query string.
            store: Store name.

        Returns:
            List of dicts with keys: entity_id, attribute, value, rank.
        """
        schema = self._schema_for_store(store)
        sql = "SELECT entity_id, attribute, value, rank FROM {}.find_text(%s)".format(schema)
        with self._cursor() as cur:
            cur.execute(sql, (search_query,))
            cols = [d[0] for d in cur.description]
            return [dict(zip(cols, row)) for row in cur.fetchall()]

    # -- Helpers ---------------------------------------------------------

    @staticmethod
    def _schema_for_store(store_name):
        """Derive the PostgreSQL schema name for a store."""
        if store_name == "default":
            return "mentat"
        return "mentat_{}".format(store_name)

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
