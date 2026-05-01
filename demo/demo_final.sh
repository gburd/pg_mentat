#!/usr/bin/env bash
# pg_mentat Demo - Simplified and reliable

# Connection parameters
export PGHOST=/tmp
export PGPORT=28816

clear
echo "========================================"
echo "  pg_mentat Demo"
echo "  Datalog Queries for PostgreSQL"
echo "========================================"
sleep 2

echo ""
echo "Setting up demo database..."
dropdb pg_mentat_demo 2>/dev/null
createdb pg_mentat_demo
psql -X pg_mentat_demo -c "CREATE EXTENSION pg_mentat;" 2>&1 | grep "CREATE EXTENSION"
sleep 1

echo ""
echo "✓ pg_mentat installed!"
echo ""
echo "Let's query some music data..."
sleep 2

# Define schema and load data in one transaction
psql -X pg_mentat_demo << 'SQL'
-- Schema
SELECT mentat_transact('[
  {:db/ident :artist/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :artist/country :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :album/title :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
  {:db/ident :album/artist :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
  {:db/ident :album/year :db/valueType :db.type/long :db/cardinality :db.cardinality/one}
]');

-- Data
SELECT mentat_transact('[
  {:db/id "beatles" :artist/name "The Beatles" :artist/country "UK"}
  {:db/id "floyd" :artist/name "Pink Floyd" :artist/country "UK"}
  {:album/title "Abbey Road" :album/artist "beatles" :album/year 1969}
  {:album/title "Dark Side of the Moon" :album/artist "floyd" :album/year 1973}
]');
SQL

sleep 2
echo ""
echo "----------------------------------------"
echo "Query 1: Find all artists"
echo "----------------------------------------"
sleep 1

psql -X pg_mentat_demo << 'SQL'
\pset format unaligned
\pset fieldsep ' | '
SELECT mentat_query(
  '[:find ?name ?country
    :where
    [?e :artist/name ?name]
    [?e :artist/country ?country]]',
  '{}'::jsonb
);
SQL

sleep 3
echo ""
echo "----------------------------------------"
echo "Query 2: Albums from the 1960s"
echo "----------------------------------------"
echo "(With predicates and ref joins)"
sleep 1

psql -X pg_mentat_demo << 'SQL'
\pset format unaligned
\pset fieldsep ' | '
SELECT mentat_query(
  '[:find ?title ?artist ?year
    :where
    [?a :album/title ?title]
    [?a :album/year ?year]
    [(>= ?year 1960)]
    [(< ?year 1970)]
    [?a :album/artist ?e]
    [?e :artist/name ?artist]]',
  '{}'::jsonb
);
SQL

sleep 3
echo ""
echo "----------------------------------------"
echo "Time Travel: Update and View History"
echo "----------------------------------------"
sleep 1

psql -X pg_mentat_demo -c "SELECT mentat_transact('[[:db/add [:album/title \"Abbey Road\"] :album/year 1970]]');" 2>&1 | head -1

sleep 1
echo ""
echo "History of Abbey Road year:"
psql -X pg_mentat_demo << 'SQL'
SELECT
  v_long AS year,
  CASE WHEN added THEN 'current' ELSE 'retracted' END AS status
FROM mentat.datoms
WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/year')
  AND e IN (SELECT e FROM mentat.datoms
            WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/title')
            AND v_text = 'Abbey Road')
ORDER BY tx;
SQL

sleep 3
echo ""
echo "----------------------------------------"
echo "Python Client Example"
echo "----------------------------------------"
sleep 1

cat > /tmp/client.py << 'PY'
import psycopg2
import json

conn = psycopg2.connect("host=/tmp port=28816 dbname=pg_mentat_demo")
cur = conn.cursor()

# Query artists
cur.execute("SELECT mentat_query('[:find ?name :where [?e :artist/name ?name]]', '{}'::jsonb)")
artists = json.loads(cur.fetchone()[0])

print("\nArtists from Python:")
for artist in artists:
    print(f"  • {artist[0]}")

conn.close()
PY

echo "  (Code example - install psycopg2 to run)"

sleep 2
echo ""
echo "========================================"
echo "  Demo Complete!"
echo "========================================"
echo ""
echo "Key Features Demonstrated:"
echo "  ✓ Schema with types and cardinality"
echo "  ✓ Datalog queries with joins"
echo "  ✓ Predicates and filters"
echo "  ✓ Immutable history"
echo "  ✓ Direct PostgreSQL access"
echo ""
echo "Learn more:"
echo "  github.com/mozilla/mentat"
echo "  docs/ops/ for production setup"
echo "========================================"
sleep 2
