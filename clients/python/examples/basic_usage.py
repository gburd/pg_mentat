#!/usr/bin/env python3
"""
Basic usage examples for the pg_mentat Python client.

This demonstrates both access paths:
  1. Direct PostgreSQL (recommended for new projects)
  2. Datomic-compatible API via mentatd WebSocket

Prerequisites:
  For direct PostgreSQL:
    pip install psycopg2-binary
    PostgreSQL running with pg_mentat extension

  For Datomic-compatible API:
    pip install websocket-client
    mentatd running at ws://localhost:8080/ws

Run:
    python basic_usage.py [--direct | --mentatd]
"""

import json
import sys

# ===========================================================================
# PATH 1: Direct PostgreSQL (recommended)
# ===========================================================================


def direct_postgresql_example():
    """
    Connect directly to PostgreSQL and call pg_mentat SQL functions.
    No mentatd daemon required. This is the recommended approach.
    """
    # Add parent directory to path for the direct client
    sys.path.insert(0, "..")
    from pg_mentat_client import MentatClient

    print("=== Direct PostgreSQL Access ===\n")

    # --- 1. Connect ---
    # Uses standard psycopg2 connection string.
    # Supports all psycopg2.connect() keyword arguments.
    with MentatClient("dbname=postgres host=localhost") as m:

        # --- 2. Define Schema ---
        print("--- Schema Definition ---")
        m.transact(
            """[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/doc "A person's full name"}

            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}

            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}

            {:db/ident :person/active
             :db/valueType :db.type/boolean
             :db/cardinality :db.cardinality/one}

            {:db/ident :person/friends
             :db/valueType :db.type/ref
             :db/cardinality :db.cardinality/many}
        ]"""
        )
        print("Schema created.\n")

        # --- 3. Transact Data ---
        print("--- Transacting Data ---")
        result = m.transact(
            """[
            {:db/id "alice"
             :person/name "Alice Johnson"
             :person/email "alice@example.com"
             :person/age 35
             :person/active true}

            {:db/id "bob"
             :person/name "Bob Smith"
             :person/email "bob@example.com"
             :person/age 42
             :person/active true
             :person/friends "alice"}

            {:person/name "Carol Williams"
             :person/email "carol@example.com"
             :person/age 28
             :person/active false}
        ]"""
        )
        print(f"Transaction result keys: {list(result.keys()) if isinstance(result, dict) else type(result)}\n")

        # --- 4. Query ---
        print("--- Queries ---")

        # Simple query
        print("\nAll people:")
        results = m.query("[:find ?name :where [?e :person/name ?name]]")
        for row in results:
            print(f"  - {row[0]}")

        # Query with joins
        print("\nPeople with emails:")
        results = m.query(
            """[:find ?name ?email
                :where
                [?e :person/name ?name]
                [?e :person/email ?email]]"""
        )
        for name, email in results:
            print(f"  - {name} <{email}>")

        # Query with input parameters (via JSON inputs)
        print("\nPeople older than 30:")
        results = m.query(
            """[:find ?name ?age
                :in $ ?min-age
                :where
                [?e :person/name ?name]
                [?e :person/age ?age]
                [(>= ?age ?min-age)]]""",
            inputs={"args": [30]},
        )
        for name, age in results:
            print(f"  - {name} (age {age})")

        # Aggregate query
        print("\nAge statistics:")
        results = m.query(
            """[:find (count ?e) (avg ?age) (min ?age) (max ?age)
                :where [?e :person/age ?age]]"""
        )
        if results:
            cnt, avg_age, min_age, max_age = results[0]
            print(f"  Count: {cnt}, Avg: {avg_age}, Min: {min_age}, Max: {max_age}")

        # --- 5. Pull ---
        print("\n--- Pull API ---")

        # Find Alice's entity ID first
        alice_results = m.query(
            '[:find ?e . :where [?e :person/email "alice@example.com"]]'
        )
        if alice_results:
            alice_id = alice_results
            print(f"\nAlice (entity {alice_id}):")
            entity = m.pull("[*]", alice_id)
            print(f"  {json.dumps(entity, indent=2, default=str)}")

        # --- 6. Entity ---
        print("\n--- Entity ---")
        if alice_results:
            entity = m.entity(alice_results)
            print(f"Alice entity: {json.dumps(entity, indent=2, default=str)}")

        # --- 7. Schema ---
        print("\n--- Schema ---")
        schema = m.schema()
        print(f"Schema type: {type(schema)}")
        if isinstance(schema, dict):
            print(f"Attributes: {list(schema.keys())[:5]}...")

        # --- 8. Explain ---
        print("\n--- Query Explain ---")
        plan = m.explain("[:find ?name :where [?e :person/name ?name]]")
        print(f"Execution plan: {json.dumps(plan, indent=2, default=str)[:200]}...")

        # --- 9. Time Travel (via inputs) ---
        print("\n--- Time Travel ---")

        # as-of query
        results = m.query(
            "[:find ?name :where [?e :person/name ?name]]",
            inputs={"asOf": 268435460},
        )
        print(f"Names as-of tx 268435460: {results}")

        # History query
        results = m.query(
            """[:find ?e ?name ?tx ?added
                :where [?e :person/name ?name ?tx ?added]]""",
            inputs={"history": True},
        )
        print(f"History rows: {len(results) if results else 0}")

        # --- 10. Native SQL View Access ---
        print("\n--- Native SQL Views ---")

        # Look up entities by attribute value
        alices = m.lookup_entity(":person/name", "Alice Johnson")
        print(f"Lookup 'Alice Johnson': {alices}")

        # Get a single value
        if alices:
            val = m.entity_value(alices[0]["entity_id"], ":person/email")
            print(f"Alice's email: {val}")

        # Full-text search
        print("\nFull-text search for 'alice':")
        text_results = m.find_text("alice")
        for r in text_results:
            print(f"  entity={r['entity_id']} attr={r['attribute']} rank={r['rank']}")

        # Browse facts
        if alices:
            print(f"\nFacts for entity {alices[0]['entity_id']}:")
            facts = m.facts(entity_id=alices[0]["entity_id"])
            for f in facts:
                print(f"  {f['attribute']} = {f['value']} ({f['value_type']})")

        # Transaction log
        print("\nRecent transactions:")
        txs = m.tx_log(limit=3)
        for tx in txs:
            print(f"  tx={tx['tx']} datoms={tx['datom_count']}")

    print("\n--- Connection auto-closed by context manager ---")


# ===========================================================================
# PATH 2: Datomic-Compatible API (via mentatd)
# ===========================================================================


def datomic_compatible_example():
    """
    Connect to mentatd via WebSocket using the Datomic-compatible API.
    Use this path when migrating from Datomic.
    """
    sys.path.insert(0, "..")
    import pg_mentat

    print("=== Datomic-Compatible API (via mentatd) ===\n")

    # --- 1. Create Client and Connect ---
    print("--- Connecting ---")
    c = pg_mentat.client(endpoint="ws://localhost:8080/ws")
    conn = pg_mentat.connect(c, db_name="example-db")
    print(f"Connected to: {conn.db_name}")

    try:
        # --- 2. Get Database Value ---
        database = pg_mentat.db(conn)
        print(f"Database t={database.t}")

        # --- 3. Schema ---
        print("\n--- Schema ---")
        pg_mentat.transact(
            conn,
            tx_data="""[
            {:db/ident :person/name
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one}
            {:db/ident :person/email
             :db/valueType :db.type/string
             :db/cardinality :db.cardinality/one
             :db/unique :db.unique/identity}
            {:db/ident :person/age
             :db/valueType :db.type/long
             :db/cardinality :db.cardinality/one}
        ]""",
        )
        print("Schema transacted.")

        # --- 4. Transact Data ---
        print("\n--- Transacting ---")
        pg_mentat.transact(
            conn,
            tx_data="""[
            {:person/name "Alice" :person/email "alice@example.com" :person/age 35}
            {:person/name "Bob" :person/email "bob@example.com" :person/age 42}
        ]""",
        )
        print("Data transacted.")

        # --- 5. Query ---
        print("\n--- Query ---")
        database = pg_mentat.db(conn)  # refresh to see new data

        results = pg_mentat.q(
            '[:find ?name ?email :where [?e :person/name ?name] [?e :person/email ?email]]',
            database,
        )
        print(f"Results: {results}")

        # --- 6. Pull ---
        print("\n--- Pull ---")
        # Find entity ID first
        eid_result = pg_mentat.q(
            '[:find ?e . :where [?e :person/email "alice@example.com"]]',
            database,
        )
        if eid_result:
            entity = pg_mentat.pull(database, "[*]", eid_result)
            print(f"Alice: {entity}")

        # --- 7. Time Travel ---
        print("\n--- Time Travel ---")
        old_db = pg_mentat.as_of(database, database.t - 1)
        old_results = pg_mentat.q(
            "[:find ?name :where [?e :person/name ?name]]",
            old_db,
        )
        print(f"Names as-of t-1: {old_results}")

        # History
        hist_db = pg_mentat.history(database)
        hist_results = pg_mentat.q(
            "[:find ?e ?name ?tx ?added :where [?e :person/name ?name ?tx ?added]]",
            hist_db,
        )
        print(f"History entries: {len(hist_results) if hist_results else 0}")

        # --- 8. List Databases ---
        print("\n--- Catalog ---")
        dbs = pg_mentat.list_databases(c)
        print(f"Available databases: {dbs}")

    finally:
        # --- 9. Cleanup ---
        conn.close()
        print("\nConnection closed.")


# ===========================================================================
# Main
# ===========================================================================

if __name__ == "__main__":
    mode = sys.argv[1] if len(sys.argv) > 1 else "--direct"

    if mode == "--mentatd":
        datomic_compatible_example()
    elif mode == "--direct":
        direct_postgresql_example()
    else:
        print("Usage: python basic_usage.py [--direct | --mentatd]")
        print()
        print("  --direct   Use direct PostgreSQL access (default, recommended)")
        print("  --mentatd  Use Datomic-compatible API via mentatd WebSocket")
        sys.exit(1)
