#!/usr/bin/env bash
# pg_mentat SQL Integration Demo - Shows new SQL integration features
set -e

PGHOST=/tmp
PGPORT=28816
DB=demo_sql_integration

# Setup (silent)
echo "Setting up database..."
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
createdb -h $PGHOST -p $PGPORT $DB >/dev/null 2>&1
psql -X -h $PGHOST -p $PGPORT $DB -c "CREATE EXTENSION pg_mentat;" >/dev/null 2>&1
echo "✓ Database ready with pg_mentat extension"
echo
sleep 2

clear
echo "============================================"
echo "  pg_mentat - SQL Integration Features"
echo "============================================"
echo
sleep 2

echo "Step 1: Add Sample Data"
echo "------------------------"
echo "$ SELECT mentat.t('[
  [:db/add \"alice\" :person/name \"Alice Johnson\"]
  [:db/add \"alice\" :person/age 30]
  [:db/add \"alice\" :person/email \"alice@example.com\"]
  [:db/add \"bob\" :person/name \"Bob Smith\"]
  [:db/add \"bob\" :person/age 25]
  [:db/add \"bob\" :person/email \"bob@example.com\"]
]')"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- First define schema
SELECT mentat.mentat_transact('[
  {:db/ident :person/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :person/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

-- Now add data using SHORT ALIAS mentat.t()
SELECT mentat.t('[
  [:db/add "alice" :person/name "Alice Johnson"]
  [:db/add "alice" :person/age 30]
  [:db/add "alice" :person/email "alice@example.com"]
  [:db/add "bob" :person/name "Bob Smith"]
  [:db/add "bob" :person/age 25]
  [:db/add "bob" :person/email "bob@example.com"]
]');
EOF
echo
echo "✓ Data added using mentat.t() alias (was mentat.mentat_transact())"
sleep 4

echo
echo "Step 2: Query with Short Alias"
echo "-------------------------------"
echo "OLD: SELECT mentat.mentat_query('[:find ?e ?name ...]', '{}');"
echo "NEW: SELECT mentat.q('[:find ?e ?name ...]');"
echo
echo "$ SELECT mentat.q('[:find ?e ?name :where [?e :person/name ?name]]');"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
SELECT mentat.q('[:find ?e ?name :where [?e :person/name ?name]]');
EOF
echo
echo "✓ mentat.q() is much more concise!"
sleep 5

echo
echo "Step 3: EDN Pretty Printing"
echo "----------------------------"
echo "$ SELECT mentat.edn_pretty(mentat.q(...)::text);"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\pset format unaligned
SELECT mentat.edn_pretty(
  mentat.q('[:find ?e ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]')::text
);
EOF
echo
echo "✓ EDN output is nicely formatted (like jsonb_pretty())"
sleep 5

echo
echo "Step 4: EDN Functions"
echo "---------------------"
echo "Extract and manipulate EDN values without casting to JSONB"
echo
echo "$ SELECT edn_get_key(result, ':results') FROM query_result;"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
WITH query_result AS (
  SELECT mentat.q('[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]') as result
)
SELECT
  edn_typeof(result) as result_type,
  edn_exists(result, ':results') as has_results_key,
  edn_array_length(edn_get_key(result, ':results')) as result_count
FROM query_result;
EOF
echo
echo "✓ Native EDN functions: edn_get_key(), edn_typeof(), edn_exists(), edn_array_length()"
sleep 6

echo
echo "Step 5: Datalog-Backed SQL VIEWs"
echo "--------------------------------"
echo "Create SQL VIEWs from Datalog queries!"
echo
echo "$ SELECT mentat.create_datalog_view('people',
    '[:find ?e ?name ?age ?email
      :where
      [?e :person/name ?name]
      [?e :person/age ?age]
      [?e :person/email ?email]]');"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create VIEW from Datalog query
SELECT mentat.create_datalog_view('people',
  '[:find ?e ?name ?age ?email
    :where
    [?e :person/name ?name]
    [?e :person/age ?age]
    [?e :person/email ?email]]');

-- Query it like a regular table!
SELECT * FROM people ORDER BY age;
EOF
echo
echo "✓ Datalog queries appear as SQL tables!"
sleep 6

echo
echo "Step 6: SQL Operations on Datalog VIEWs"
echo "----------------------------------------"
echo "Use standard SQL on Datalog-backed VIEWs"
echo
echo "$ SELECT name, age FROM people WHERE age > 25;"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Standard SQL WHERE clause
SELECT name, age FROM people WHERE age > 25;

-- Aggregations work too
SELECT
  COUNT(*) as total_people,
  AVG(age::numeric) as avg_age,
  MIN(age) as min_age,
  MAX(age) as max_age
FROM people;
EOF
echo
echo "✓ Full SQL power with Datalog data!"
sleep 6

echo
echo "Step 7: Datom Helper Functions"
echo "--------------------------------"
echo "Easy SQL queries on raw datom storage"
echo
echo "$ SELECT * FROM mentat.datom_text_like(':person/email', '%@example.com');"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Find all emails matching pattern
SELECT entity_id, value as email FROM mentat.datom_text_like(':person/email', '%@example.com');

-- Range queries on age
SELECT entity_id, value as age FROM mentat.datom_long_between(':person/age', 20, 30);
EOF
echo
echo "✓ Direct datom queries without verbose WHERE clauses"
sleep 5

echo
echo "Step 8: More Function Aliases"
echo "------------------------------"
echo "All major functions have short names:"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
SELECT
  'mentat.q()' as "Query Alias",
  'mentat.t()' as "Transact Alias",
  'mentat.pull()' as "Pull Alias",
  'mentat.entity()' as "Entity Alias",
  'mentat.schema()' as "Schema Alias",
  'mentat.stats()' as "Stats Alias";
EOF
echo
echo "$ SELECT mentat.stats();"
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
SELECT jsonb_pretty(mentat.stats()::jsonb);
EOF
sleep 6

echo
echo "============================================"
echo "SQL Integration Demo Complete!"
echo "============================================"
echo
echo "New Features Demonstrated:"
echo "  ✓ Function Aliases (mentat.q(), mentat.t(), etc.)"
echo "  ✓ EDN Pretty Printing (edn_pretty())"
echo "  ✓ EDN Functions (edn_get_key(), edn_typeof(), etc.)"
echo "  ✓ Datalog VIEWs (create_datalog_view())"
echo "  ✓ SQL Operations on VIEWs (WHERE, JOIN, aggregations)"
echo "  ✓ Datom Helpers (datom_text_like(), datom_long_between())"
echo
echo "Key Benefits:"
echo "  • Native PostgreSQL feel (like PostGIS or JSONB)"
echo "  • Short, convenient function names"
echo "  • EDN as native PostgreSQL type"
echo "  • Seamless SQL + Datalog integration"
echo "  • Performance via decomposed columns"
echo
echo "Documentation:"
echo "  • README.md - Quick start"
echo "  • docs/SQL_INTEGRATION.md - Complete guide"
echo "  • docs/EDN_TYPE.md - EDN type reference"
echo
echo "============================================"

# Cleanup
echo
echo "Cleaning up..."
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
echo "Done!"
