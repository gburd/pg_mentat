#!/usr/bin/env bash
# pg_mentat SQL Integration Demo - With Correct/Simplified API
# This shows what the API SHOULD look like after function naming cleanup

set -e

PGHOST=/tmp
PGPORT=28816
DB=demo_sql_clean

GREEN='\033[0;32m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

echo_step() {
    echo -e "${GREEN}=== $1 ===${NC}"
}

clear
echo "=========================================="
echo "pg_mentat SQL Integration"
echo "Clean API Demo"
echo "=========================================="
echo
sleep 1

echo_step "Setup Database"
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
createdb -h $PGHOST -p $PGPORT $DB
psql -X -h $PGHOST -p $PGPORT $DB -c "CREATE EXTENSION pg_mentat;" -q
echo "✓ Extension installed"
sleep 2

echo
echo_step "Multi-Store Support"
echo "Simple, clean function names:"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create stores with clean API
SELECT mentat.create_store('users', 'User data store');
SELECT mentat.create_store('analytics', 'Analytics data');

-- List stores
SELECT jsonb_pretty(mentat.list_stores()::jsonb);
EOF
sleep 3

echo
echo_step "Define Schema & Load Data"
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Define schema (using short alias mentat.t)
SELECT mentat.t('[
  {:db/ident :user/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/role
   :db/valueType :db.type/keyword
   :db/cardinality :db.cardinality/one}
]', store := 'users');

-- Add data to specific store
SELECT mentat.t('[
  {:user/name "Alice Smith"
   :user/email "alice@example.com"
   :user/age 32
   :user/role :role/manager}
  {:user/name "Bob Jones"
   :user/email "bob@example.com"
   :user/age 28
   :user/role :role/engineer}
]', store := 'users');
EOF
sleep 3

echo
echo_step "Query with Clean API"
echo "Single mentat.q() function with optional parameters:"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Query specific store (using named parameters)
SELECT jsonb_pretty(
  mentat.q(
    '[:find ?name ?email
      :where
      [?u :user/name ?name]
      [?u :user/email ?email]]',
    store := 'users'
  )::jsonb
);

\echo ''
\echo 'Query default store (omit store parameter):'
SELECT jsonb_pretty(
  mentat.q('[:find ?e ?ident :where [?e :db/ident ?ident]]')::jsonb
) AS default_store_query;
EOF
sleep 4

echo
echo_step "Virtual Tables"
echo "SQL-native querying:"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\echo 'All entities:'
SELECT * FROM mentat_users.entities LIMIT 3;

\echo ''
\echo 'Text values:'
SELECT entity, value FROM mentat_users.text_values
WHERE attribute = ':user/name';

\echo ''
\echo 'Schema attributes:'
SELECT attribute, value_type, cardinality
FROM mentat_users.attributes
WHERE attribute LIKE 'user/%';
EOF
sleep 4

echo
echo_step "Materialized Views - Clean API"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create materialized view (no "mentat_" prefix!)
SELECT mentat.materialize(
  'users',
  'engineers',
  '[:find ?name ?email
    :where
    [?u :user/name ?name]
    [?u :user/email ?email]
    [?u :user/role :role/engineer]]',
  refresh_policy := 'manual'
);

\echo ''
\echo 'Query the materialized view:'
SELECT * FROM mentat_users.engineers;

\echo ''
\echo 'List materialized views:'
SELECT jsonb_pretty(mentat.list_matviews('users')::jsonb);

\echo ''
\echo 'Refresh:'
SELECT mentat.refresh('users', 'engineers');
EOF
sleep 4

echo
echo_step "Time-Travel Queries"
echo
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Get current transaction
\set curr_tx (SELECT MAX(tx) FROM mentat_users.transactions)

-- Make a change
SELECT mentat.t('[
  [:db/add [:user/email "bob@example.com"] :user/age 29]
]', store := 'users');

\echo 'Query historical state (clean API):'
SELECT jsonb_pretty(
  mentat.q(
    '[:find ?name ?age
      :where [?u :user/name ?name] [?u :user/age ?age]]',
    store := 'users',
    as_of_tx := :curr_tx  -- Time-travel!
  )::jsonb
);

\echo ''
\echo 'Diff between transactions:'
SELECT jsonb_pretty(
  mentat.diff(
    'users',
    :curr_tx,
    (SELECT MAX(tx) FROM mentat_users.transactions),
    '[:find ?name ?age :where [?u :user/name ?name] [?u :user/age ?age]]'
  )::jsonb
);

\echo ''
\echo 'Audit log:'
SELECT jsonb_pretty(
  mentat.log(
    'users',
    :curr_tx,
    (SELECT MAX(tx) FROM mentat_users.transactions)
  )::jsonb
);
EOF
sleep 5

echo
echo_step "Subscriptions - Clean API"
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Subscribe (no "mentat_" prefix)
SELECT mentat.subscribe(
  'users',
  'user_changes',
  '[:find ?name :where [?u :user/name ?name]]'
);

\echo ''
\echo 'List subscriptions:'
SELECT jsonb_pretty(mentat.list_subscriptions('users')::jsonb);

-- Note: In your app, you would: LISTEN mentat_user_changes;
EOF
sleep 3

echo
echo_step "Recursive Queries"
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- First add manager relationship
SELECT mentat.t('[
  [:db/add [:user/email "bob@example.com"]
   :user/manager [:user/email "alice@example.com"]]
]', store := 'users');

-- Add manager attribute to schema
SELECT mentat.t('[
  {:db/ident :user/manager
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}
]', store := 'users');

-- Create recursive view (clean API)
SELECT mentat.recursive(
  'users',
  'org_hierarchy',
  'reports',
  'SELECT e AS employee, v_ref AS manager
   FROM mentat_users.datoms
   WHERE value_type_tag = 0 AND added = true',
  'SELECT r.employee, d.v_ref AS manager
   FROM reports r
   JOIN mentat_users.datoms d ON d.e = r.manager',
  max_depth := 100
);

\echo ''
\echo 'Query org hierarchy:'
SELECT * FROM mentat_users.org_hierarchy;
EOF
sleep 4

echo
echo
echo "=========================================="
echo -e "${GREEN}Clean API Summary${NC}"
echo "=========================================="
echo
cat << 'SUMMARY'
Function Naming - Before vs After:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Store Management:
  × mentat.mentat_create_store()    ✓ mentat.create_store()
  × mentat.mentat_list_stores()     ✓ mentat.list_stores()
  × mentat.mentat_drop_store()      ✓ mentat.drop_store()

Query Functions:
  × mentat.mentat_q_store()         ✓ mentat.q(store := ...)
  × mentat.mentat_q_full()          ✓ mentat.q(as_of_tx := ...)

Materialized Views:
  × mentat.mentat_materialize()     ✓ mentat.materialize()
  × mentat.mentat_refresh()         ✓ mentat.refresh()
  × mentat.mentat_list_matviews()   ✓ mentat.list_matviews()

Time-Travel:
  × mentat.mentat_diff()            ✓ mentat.diff()
  × mentat.mentat_log()             ✓ mentat.log()

Subscriptions:
  × mentat.mentat_subscribe()       ✓ mentat.subscribe()
  × mentat.mentat_unsubscribe()     ✓ mentat.unsubscribe()
  × mentat.mentat_list_subscriptions() ✓ mentat.list_subscriptions()

Recursive:
  × mentat.mentat_recursive()       ✓ mentat.recursive()

Benefits of Clean API:
  • Less typing
  • More intuitive
  • Consistent with PostgreSQL conventions
  • Schema namespace already says "mentat"
SUMMARY
echo
sleep 3

echo "=========================================="
echo "Next Steps:"
echo "  1. Refactor Rust code to use clean names"
echo "  2. Keep old names as deprecated aliases"
echo "  3. Update all tests and documentation"
echo "  4. cargo pgrx install --release"
echo "=========================================="
echo
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
echo "Done!"
