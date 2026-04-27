#!/usr/bin/env bash
# pg_mentat Advanced SQL Integration Demo
# Demonstrates: Multi-store, Virtual Tables, Materialized Views, Time-Travel, Subscriptions, Recursive Queries

set -e

PGHOST=/tmp
PGPORT=28816
DB=demo_sql_advanced

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo_step() {
    echo -e "${GREEN}=== $1 ===${NC}"
}

pause() {
    sleep ${1:-2}
}

clear
echo "=========================================="
echo "pg_mentat - Advanced SQL Integration Demo"
echo "=========================================="
echo
pause 1

echo_step "Step 1: Setup Database"
echo "Creating fresh database and installing extension..."
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
createdb -h $PGHOST -p $PGPORT $DB
psql -X -h $PGHOST -p $PGPORT $DB -c "CREATE EXTENSION pg_mentat;" -q
echo "✓ Extension installed with default store"
pause 2

echo
echo_step "Step 2: Create Multiple Stores"
echo "Creating isolated stores for different domains..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create stores for different use cases
SELECT mentat.mentat_create_store('analytics', 'Analytics and metrics data');
SELECT mentat.mentat_create_store('users', 'User and profile data');

-- List all stores
\echo ''
\echo 'All stores:'
SELECT jsonb_pretty(mentat.mentat_list_stores()::jsonb);
EOF
pause 3

echo
echo_step "Step 3: Define Schema and Load Data"
echo "Adding schema and sample data to 'users' store..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Define user schema in 'users' store
SELECT mentat.mentat_transact_full('users', '[
  {:db/ident :user/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/email
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :user/age
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/role
   :db/valueType :db.type/keyword
   :db/cardinality :db.cardinality/one}
  {:db/ident :user/manager
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one}
]');

-- Add sample users
SELECT mentat.mentat_transact_full('users', '[
  {:user/name "Alice Smith"
   :user/email "alice@example.com"
   :user/age 32
   :user/role :role/manager}
  {:user/name "Bob Jones"
   :user/email "bob@example.com"
   :user/age 28
   :user/role :role/engineer
   :user/manager [:user/email "alice@example.com"]}
  {:user/name "Carol Davis"
   :user/email "carol@example.com"
   :user/age 25
   :user/role :role/engineer
   :user/manager [:user/email "alice@example.com"]}
]');

\echo '✓ Schema and data loaded'
EOF
pause 3

echo
echo_step "Step 4: Query Using Virtual Tables"
echo "Virtual tables provide SQL views of the datom data..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\echo 'All entities in users store:'
SELECT entity_id, created_at, last_modified_at
FROM mentat_users.entities
LIMIT 5;

\echo ''
\echo 'User attributes from schema:'
SELECT attribute, value_type, cardinality
FROM mentat_users.attributes
WHERE attribute LIKE 'user/%';

\echo ''
\echo 'All user names (using type-specific view):'
SELECT entity, attribute, value
FROM mentat_users.text_values
WHERE attribute = ':user/name';
EOF
pause 4

echo
echo_step "Step 5: Store-Aware Datalog Queries"
echo "Query the 'users' store using Datalog..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\echo 'Find all engineers:'
SELECT jsonb_pretty(
  mentat.mentat_q_store('users',
    '[:find ?name ?email
      :where
      [?u :user/name ?name]
      [?u :user/email ?email]
      [?u :user/role :role/engineer]]',
    '{}'::jsonb
  )::jsonb
);

\echo ''
\echo 'Find manager-report relationships:'
SELECT jsonb_pretty(
  mentat.mentat_q_store('users',
    '[:find ?emp-name ?mgr-name
      :where
      [?emp :user/name ?emp-name]
      [?emp :user/manager ?mgr]
      [?mgr :user/name ?mgr-name]]',
    '{}'::jsonb
  )::jsonb
);
EOF
pause 4

echo
echo_step "Step 6: Create Materialized View"
echo "Cache expensive queries as materialized views..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create a materialized view of all engineers
SELECT mentat.mentat_materialize(
  'users',
  'engineers_mv',
  '[:find ?name ?email ?age
    :where
    [?u :user/name ?name]
    [?u :user/email ?email]
    [?u :user/age ?age]
    [?u :user/role :role/engineer]]',
  'manual'
);

\echo ''
\echo 'Query the materialized view (fast!):'
SELECT * FROM mentat_users.engineers_mv;

\echo ''
\echo 'List all materialized views:'
SELECT jsonb_pretty(mentat.mentat_list_matviews('users')::jsonb);
EOF
pause 4

echo
echo_step "Step 7: Time-Travel Queries"
echo "Query historical state and track changes..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Get current transaction
\set curr_tx (SELECT MAX(tx) FROM mentat_users.transactions)

-- Make a change
SELECT mentat.mentat_transact_full('users', '[
  [:db/add [:user/email "bob@example.com"] :user/age 29]
]');

\echo 'Transaction log showing the age change:'
SELECT jsonb_pretty(mentat.mentat_log_default(
  :curr_tx,
  (SELECT MAX(tx) FROM mentat_users.transactions)
)::jsonb) AS log_entries;

\echo ''
\echo 'Compare before and after:'
SELECT jsonb_pretty(mentat.mentat_diff_default(
  :curr_tx,
  (SELECT MAX(tx) FROM mentat_users.transactions),
  '[:find ?name ?age :where [?u :user/name ?name] [?u :user/age ?age]]',
  '{}'::jsonb
)::jsonb);
EOF
pause 5

echo
echo_step "Step 8: Streaming Subscriptions"
echo "Set up real-time notifications on data changes..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create subscription for engineer changes
SELECT mentat.mentat_subscribe(
  'users',
  'engineer_changes',
  '[:find ?name :where [?u :user/name ?name] [?u :user/role :role/engineer]]'
);

\echo ''
\echo 'Subscription created. Channel: mentat_engineer_changes'
\echo 'Active subscriptions:'
SELECT jsonb_pretty(mentat.mentat_list_subscriptions('users')::jsonb);
EOF
pause 3

echo
echo_step "Step 9: Recursive Queries"
echo "Create organizational hierarchy view..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
-- Create recursive view for management hierarchy
SELECT mentat.mentat_recursive(
  'users',
  'reports_hierarchy',
  'reports',
  -- Base: direct reports
  'SELECT d.e AS employee, d.v_ref AS manager
   FROM mentat_users.datoms d
   JOIN mentat_users.idents i ON i.entid = d.a
   WHERE i.ident = '':user/manager'' AND d.value_type_tag = 0 AND d.added = true',
  -- Recursive: indirect reports
  'SELECT r.employee, d.v_ref AS manager
   FROM reports r
   JOIN mentat_users.datoms d ON d.e = r.manager
   JOIN mentat_users.idents i ON i.entid = d.a
   WHERE i.ident = '':user/manager'' AND d.value_type_tag = 0 AND d.added = true',
  100  -- max depth
);

\echo ''
\echo 'Management hierarchy (recursive query):'
WITH hierarchy AS (
  SELECT employee, manager, 1 as level
  FROM mentat_users.reports_hierarchy
)
SELECT h.level,
       (SELECT value FROM mentat_users.text_values WHERE entity = h.employee AND attribute = ':user/name') as employee_name,
       (SELECT value FROM mentat_users.text_values WHERE entity = h.manager AND attribute = ':user/name') as manager_name
FROM hierarchy h
ORDER BY h.level, employee_name;
EOF
pause 4

echo
echo_step "Step 10: SQL-Native Experience"
echo "Query like regular SQL tables (hiding EAVT complexity)..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\echo 'Full-text search on names:'
SELECT entity, attribute, text_value
FROM mentat_users.searchable_text
WHERE text_value ILIKE '%Smith%';

\echo ''
\echo 'Reference relationships (who reports to whom):'
SELECT from_entity, relationship, to_entity
FROM mentat_users.references
WHERE relationship = ':user/manager';

\echo ''
\echo 'Aggregate query - average age by role:'
WITH user_data AS (
  SELECT DISTINCT ON (n.entity)
    k.value as role,
    n.value::int as age
  FROM mentat_users.text_values n
  JOIN mentat_users.keyword_values k ON k.entity = n.entity
  WHERE n.attribute = ':user/name'
    AND k.attribute = ':user/role'
)
SELECT role, AVG(age) as avg_age, COUNT(*) as count
FROM user_data
GROUP BY role;
EOF
pause 4

echo
echo_step "Step 11: Store Isolation Verification"
echo "Confirming data isolation between stores..."
psql -X -h $PGHOST -p $PGPORT $DB << 'EOF'
\echo 'Entities in default store:'
SELECT COUNT(*) as count FROM mentat.entities;

\echo ''
\echo 'Entities in users store:'
SELECT COUNT(*) as count FROM mentat_users.entities;

\echo ''
\echo 'Entities in analytics store (empty):'
SELECT COUNT(*) as count FROM mentat_analytics.entities;

\echo ''
\echo 'Each store is completely isolated!'
EOF
pause 3

echo
echo "=========================================="
echo "Demo Complete!"
echo
echo -e "${GREEN}Features Demonstrated:${NC}"
echo "  ✓ Multi-store support with schema isolation"
echo "  ✓ Virtual tables (10+ views per store)"
echo "  ✓ Store-aware Datalog queries"
echo "  ✓ Materialized views with Datalog"
echo "  ✓ Time-travel queries and diffs"
echo "  ✓ Real-time subscriptions (LISTEN/NOTIFY)"
echo "  ✓ Recursive queries (WITH RECURSIVE)"
echo "  ✓ SQL-native query experience"
echo "  ✓ Full-text search integration"
echo "  ✓ Complete data isolation"
echo
echo -e "${YELLOW}New SQL Integration Capabilities:${NC}"
echo "  • mentat.mentat_create_store() - Create isolated stores"
echo "  • mentat.mentat_list_stores() - List all stores"
echo "  • <schema>.entities/attributes/facts - Virtual table views"
echo "  • mentat.mentat_q_store() - Store-aware queries"
echo "  • mentat.mentat_materialize() - Materialized views"
echo "  • mentat.mentat_diff() - Time-travel diffs"
echo "  • mentat.mentat_subscribe() - Real-time notifications"
echo "  • mentat.mentat_recursive() - Hierarchical queries"
echo
echo -e "${GREEN}pg_mentat now provides a true 'database within a database' experience!${NC}"
echo "=========================================="
echo
echo "Cleaning up..."
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
echo "Done!"
