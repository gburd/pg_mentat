#!/usr/bin/env bash
# Automated function renaming for pg_mentat
# Removes redundant mentat_ prefix from all function names

set -e
cd /home/gburd/ws/pg_mentat/pg_mentat

echo "=== Renaming functions in materialized_views.rs ==="
sed -i 's/^pub fn mentat_materialize(/pub fn materialize(/g' src/functions/materialized_views.rs
sed -i 's/^pub fn mentat_refresh(/pub fn refresh(/g' src/functions/materialized_views.rs
sed -i 's/^pub fn mentat_drop_matview(/pub fn drop_matview(/g' src/functions/materialized_views.rs
sed -i 's/^pub fn mentat_list_matviews(/pub fn list_matviews(/g' src/functions/materialized_views.rs

echo "=== Renaming functions in time_travel.rs ==="
sed -i 's/^pub fn mentat_diff(/pub fn diff(/g' src/functions/time_travel.rs
sed -i 's/^pub fn mentat_diff_default(/pub fn diff_default(/g' src/functions/time_travel.rs
sed -i 's/^pub fn mentat_log(/pub fn log(/g' src/functions/time_travel.rs
sed -i 's/^pub fn mentat_log_default(/pub fn log_default(/g' src/functions/time_travel.rs

echo "=== Renaming functions in subscriptions.rs ==="
sed -i 's/^pub fn mentat_subscribe(/pub fn subscribe(/g' src/functions/subscriptions.rs
sed -i 's/^pub fn mentat_unsubscribe(/pub fn unsubscribe(/g' src/functions/subscriptions.rs
sed -i 's/^pub fn mentat_list_subscriptions(/pub fn list_subscriptions(/g' src/functions/subscriptions.rs

echo "=== Renaming functions in recursive_queries.rs ==="
sed -i 's/^pub fn mentat_recursive(/pub fn recursive(/g' src/functions/recursive_queries.rs
sed -i 's/^pub fn mentat_drop_recursive(/pub fn drop_recursive(/g' src/functions/recursive_queries.rs
sed -i 's/^pub fn mentat_list_recursive(/pub fn list_recursive(/g' src/functions/recursive_queries.rs

echo "=== Renaming functions in query.rs ==="
sed -i 's/^pub fn mentat_q_store(/pub fn q(/g' src/functions/query.rs
# Keep mentat_q_full and mentat_q_default as they'll be removed in favor of optional parameters

echo "=== Renaming functions in transact.rs ==="
sed -i 's/^pub fn mentat_transact_full(/pub fn t(/g' src/functions/transact.rs

echo "=== Renaming functions in pull.rs ==="
sed -i 's/^pub fn mentat_pull_in_store(/pub fn pull(/g' src/functions/pull.rs
sed -i 's/^pub fn mentat_pull_many_in_store(/pub fn pull_many(/g' src/functions/pull.rs

echo "=== Renaming functions in entity.rs ==="
sed -i 's/^pub fn mentat_entity_in_store(/pub fn entity(/g' src/functions/entity.rs

echo "=== Renaming functions in schema.rs ==="
sed -i 's/^pub fn mentat_schema_in_store(/pub fn schema(/g' src/functions/schema.rs

echo "=== Renaming functions in virtual_tables.rs ==="
sed -i 's/^pub fn mentat_create_virtual_tables(/pub fn create_virtual_tables(/g' src/functions/virtual_tables.rs

echo "=== Updating internal calls in virtual_tables.rs ==="
sed -i 's/create_store(/create_store(/g' src/functions/virtual_tables.rs

echo "===Done! Function names updated.==="
echo "Note: store_management.rs already updated manually"

cd /home/gburd/ws/pg_mentat
echo "Updating demo script to use new names..."
sed -i 's/mentat\.mentat_create_store/mentat.create_store/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_list_stores/mentat.list_stores/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_transact_full/mentat.t/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_q_store/mentat.q/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_materialize/mentat.materialize/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_list_matviews/mentat.list_matviews/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_diff_default/mentat.diff/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_log_default/mentat.log/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_subscribe/mentat.subscribe/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_list_subscriptions/mentat.list_subscriptions/g' demo_sql_integration.sh
sed -i 's/mentat\.mentat_recursive/mentat.recursive/g' demo_sql_integration.sh

echo "Demo script updated!"
echo ""
echo "Summary of changes:"
echo "  ✓ store_management.rs: 4 functions (done manually earlier)"
echo "  ✓ materialized_views.rs: 4 functions"
echo "  ✓ time_travel.rs: 4 functions"
echo "  ✓ subscriptions.rs: 3 functions"
echo "  ✓ recursive_queries.rs: 3 functions"
echo "  ✓ query.rs: 1 function"
echo "  ✓ transact.rs: 1 function"
echo "  ✓ pull.rs: 2 functions"
echo "  ✓ entity.rs: 1 function"
echo "  ✓ schema.rs: 1 function"
echo "  ✓ virtual_tables.rs: 1 function"
echo "  ✓ demo_sql_integration.sh: updated all function calls"
echo ""
echo "Total: ~25 functions renamed"
