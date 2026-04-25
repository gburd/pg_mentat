#!/usr/bin/env bash
# Quick pg_mentat Demo - 5 minutes
# set -e disabled for demo

clear
echo "================================================"
echo "  pg_mentat: Datalog Queries for PostgreSQL"
echo "================================================"
echo ""
echo "A Datomic-compatible query engine built on"
echo "PostgreSQL, bringing immutable facts and"
echo "time-travel queries to your database."
echo ""
sleep 3

echo "Let's see it in action!"
sleep 2

# Setup
echo ""
echo "▶ Creating demo database..."
dropdb -h /tmp -p 28816 pg_mentat_demo 2>/dev/null; createdb -h /tmp -p 28816 pg_mentat_demo
sleep 1

echo "▶ Installing pg_mentat extension..."
psql -X -h /tmp -p 28816 pg_mentat_demo -c "CREATE EXTENSION pg_mentat;" 2>&1 | grep -v "NOTICE"
sleep 1

echo "▶ Verifying installation..."
psql -X -h /tmp -p 28816 pg_mentat_demo -c "\dx pg_mentat" | grep -A2 "List of"
sleep 2

echo ""
echo "================================================"
echo "  Demo: Music Database with Datalog Queries"
echo "================================================"
sleep 2

echo ""
echo "▶ Step 1: Define Schema"
echo "   Creating attributes for artists and albums..."
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL' 2>&1 | grep -v "NOTICE"
SELECT mentat_transact('[
  {:db/ident :artist/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :artist/country
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :album/title
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}

  {:db/ident :album/artist
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}

  {:db/ident :album/year
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
]') AS transaction_result \gx
SQL

echo "  ✓ Schema defined!"
sleep 2

echo ""
echo "▶ Step 2: Insert Data"
echo "   Adding The Beatles, Pink Floyd, and their albums..."
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL' 2>&1 | grep -v "NOTICE"
SELECT mentat_transact('[
  {:db/id "beatles"
   :artist/name "The Beatles"
   :artist/country "UK"}

  {:db/id "floyd"
   :artist/name "Pink Floyd"
   :artist/country "UK"}

  {:album/title "Abbey Road"
   :album/artist "beatles"
   :album/year 1969}

  {:album/title "Dark Side of the Moon"
   :album/artist "floyd"
   :album/year 1973}

  {:album/title "Sgt Pepper"
   :album/artist "beatles"
   :album/year 1967}
]') AS transaction_result \gx
SQL

echo "  ✓ Data loaded!"
sleep 2

echo ""
echo "▶ Step 3: Datalog Queries"
echo ""
echo "Query 1: Find all artists"
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL'
SELECT mentat_query(
  '[:find ?name ?country
    :where
    [?artist :artist/name ?name]
    [?artist :artist/country ?country]]',
  '{}'::jsonb
) AS results;
SQL

sleep 3

echo ""
echo "Query 2: Find albums from the 1960s with their artists"
echo "         (Note the predicate filters and ref joins)"
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL'
SELECT mentat_query(
  '[:find ?album ?artist ?year
    :where
    [?a :album/title ?album]
    [?a :album/year ?year]
    [(>= ?year 1960)]
    [(< ?year 1970)]
    [?a :album/artist ?artist-ent]
    [?artist-ent :artist/name ?artist]]',
  '{}'::jsonb
) AS results;
SQL

sleep 3

echo ""
echo "Query 3: Count albums per artist"
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL'
SELECT mentat_query(
  '[:find ?artist (count ?album)
    :where
    [?a :album/title ?album]
    [?a :album/artist ?artist-ent]
    [?artist-ent :artist/name ?artist]]',
  '{}'::jsonb
) AS results;
SQL

sleep 3

echo ""
echo "================================================"
echo "  Time Travel: Immutable Facts"
echo "================================================"
sleep 1

echo ""
echo "▶ Let's update Abbey Road's year and see the history..."
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL' 2>&1 | grep -v "NOTICE"
SELECT mentat_transact('[
  [:db/add [:album/title "Abbey Road"] :album/year 1970]
]') AS update_result \gx
SQL

sleep 1

echo ""
echo "Current value:"
psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL'
SELECT mentat_query(
  '[:find ?title ?year
    :where
    [?a :album/title "Abbey Road"]
    [?a :album/title ?title]
    [?a :album/year ?year]]',
  '{}'::jsonb
) AS results;
SQL

sleep 2

echo ""
echo "Full history (both 1969 and 1970 are preserved!):"
psql -X -h /tmp -p 28816 pg_mentat_demo -c "
SELECT
  v_long AS year,
  added AS is_current,
  tx AS transaction_id
FROM mentat.datoms
WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/year')
  AND e = (SELECT e FROM mentat.datoms
           WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/title')
           AND v_text = 'Abbey Road'
           LIMIT 1)
ORDER BY tx;"

sleep 3

echo ""
echo "================================================"
echo "  Python Client Demo"
echo "================================================"
sleep 1

echo ""
echo "▶ Creating Python client to query via direct PostgreSQL..."
sleep 1

cat > /tmp/demo_client.py << 'PY'
import psycopg2
import json

# Connect directly to PostgreSQL (no mentatd needed!)
conn = psycopg2.connect("host=localhost port=28816 dbname=pg_mentat_demo")
cur = conn.cursor()

print("\n🐍 Python Client Demo\n")
print("=" * 50)

# Query 1: All artists
print("\n1. Query all artists:")
cur.execute("""
    SELECT mentat_query(
        '[:find ?name ?country
          :where
          [?e :artist/name ?name]
          [?e :artist/country ?country]]',
        '{}'::jsonb
    )
""")
results = json.loads(cur.fetchone()[0])
for row in results:
    print(f"   • {row[0]} ({row[1]})")

# Query 2: Albums with predicates
print("\n2. Albums from 1970s:")
cur.execute("""
    SELECT mentat_query(
        '[:find ?title ?year
          :where
          [?a :album/title ?title]
          [?a :album/year ?year]
          [(>= ?year 1970)]]',
        '{}'::jsonb
    )
""")
results = json.loads(cur.fetchone()[0])
for row in results:
    print(f"   • {row[0]} ({row[1]})")

print("\n" + "=" * 50)
print("✓ Python client successfully queried pg_mentat!")
print("  (Direct PostgreSQL connection, no HTTP gateway needed)")

conn.close()
PY

python3 /tmp/demo_client.py

sleep 3

echo ""
echo "================================================"
echo "  Database Statistics"
echo "================================================"
sleep 1

psql -X -h /tmp -p 28816 pg_mentat_demo << 'SQL'
SELECT
  'Total Facts' AS metric,
  COUNT(*) AS count
FROM mentat.datoms WHERE added = true
UNION ALL
SELECT
  'Schema Attributes',
  COUNT(*)
FROM mentat.schema
WHERE ident LIKE ':%/%'
UNION ALL
SELECT
  'Transactions',
  COUNT(*)
FROM mentat.transactions;
SQL

sleep 3

echo ""
echo "================================================"
echo "  Demo Complete!"
echo "================================================"
echo ""
echo "What we showed:"
echo "  ✓ Schema definition with types and cardinality"
echo "  ✓ Datalog queries with predicates and joins"
echo "  ✓ Immutable facts with full history"
echo "  ✓ Direct PostgreSQL client access"
echo "  ✓ No separate daemon required!"
echo ""
echo "Next steps:"
echo "  • Try temporal queries (as-of, since, history)"
echo "  • Add pg_vector for semantic search"
echo "  • Use mentatd for Datomic client compatibility"
echo "  • Check docs/ops/ for production setup"
echo ""
echo "GitHub: github.com/mozilla/mentat"
echo "================================================"
sleep 2
