#!/usr/bin/env bash
# Script to rename all mentat_* functions to remove redundant prefix
set -e

echo "Renaming functions in store_management.rs..."
sed -i 's/pub fn mentat_create_store(/pub fn create_store(/g' pg_mentat/src/functions/store_management.rs
sed -i 's/pub fn mentat_drop_store(/pub fn drop_store(/g' pg_mentat/src/functions/store_management.rs
sed -i 's/pub fn mentat_list_stores(/pub fn list_stores(/g' pg_mentat/src/functions/store_management.rs
sed -i 's/pub fn mentat_rename_store(/pub fn rename_store(/g' pg_mentat/src/functions/store_management.rs

echo "Renaming functions in query.rs..."
sed -i 's/pub fn mentat_q_store(/pub fn q(/g' pg_mentat/src/functions/query.rs
sed -i 's/pub fn mentat_q_full(/pub fn q_full(/g' pg_mentat/src/functions/query.rs
sed -i 's/pub fn mentat_q_default(/pub fn q_default(/g' pg_mentat/src/functions/query.rs

echo "Renaming functions in transact.rs..."
sed -i 's/pub fn mentat_transact_full(/pub fn t_store(/g' pg_mentat/src/functions/transact.rs

echo "Renaming functions in pull.rs..."
sed -i 's/pub fn mentat_pull_in_store(/pub fn pull(/g' pg_mentat/src/functions/pull.rs
sed -i 's/pub fn mentat_pull_many_in_store(/pub fn pull_many(/g' pg_mentat/src/functions/pull.rs

echo "Renaming functions in entity.rs..."
sed -i 's/pub fn mentat_entity_in_store(/pub fn entity(/g' pg_mentat/src/functions/entity.rs

echo "Renaming functions in schema.rs..."
sed -i 's/pub fn mentat_schema_in_store(/pub fn schema(/g' pg_mentat/src/functions/schema.rs

echo "Renaming functions in materialized_views.rs..."
sed -i 's/pub fn mentat_materialize(/pub fn materialize(/g' pg_mentat/src/functions/materialized_views.rs
sed -i 's/pub fn mentat_refresh(/pub fn refresh(/g' pg_mentat/src/functions/materialized_views.rs
sed -i 's/pub fn mentat_drop_matview(/pub fn drop_matview(/g' pg_mentat/src/functions/materialized_views.rs
sed -i 's/pub fn mentat_list_matviews(/pub fn list_matviews(/g' pg_mentat/src/functions/materialized_views.rs

echo "Renaming functions in time_travel.rs..."
sed -i 's/pub fn mentat_diff(/pub fn diff(/g' pg_mentat/src/functions/time_travel.rs
sed -i 's/pub fn mentat_diff_default(/pub fn diff_default(/g' pg_mentat/src/functions/time_travel.rs
sed -i 's/pub fn mentat_log(/pub fn log(/g' pg_mentat/src/functions/time_travel.rs
sed -i 's/pub fn mentat_log_default(/pub fn log_default(/g' pg_mentat/src/functions/time_travel.rs

echo "Renaming functions in subscriptions.rs..."
sed -i 's/pub fn mentat_subscribe(/pub fn subscribe(/g' pg_mentat/src/functions/subscriptions.rs
sed -i 's/pub fn mentat_unsubscribe(/pub fn unsubscribe(/g' pg_mentat/src/functions/subscriptions.rs
sed -i 's/pub fn mentat_list_subscriptions(/pub fn list_subscriptions(/g' pg_mentat/src/functions/subscriptions.rs

echo "Renaming functions in recursive_queries.rs..."
sed -i 's/pub fn mentat_recursive(/pub fn recursive(/g' pg_mentat/src/functions/recursive_queries.rs
sed -i 's/pub fn mentat_drop_recursive(/pub fn drop_recursive(/g' pg_mentat/src/functions/recursive_queries.rs
sed -i 's/pub fn mentat_list_recursive(/pub fn list_recursive(/g' pg_mentat/src/functions/recursive_queries.rs

echo "Renaming functions in virtual_tables.rs..."
sed -i 's/pub fn mentat_create_virtual_tables(/pub fn create_virtual_tables(/g' pg_mentat/src/functions/virtual_tables.rs

echo "Updating internal function calls..."
# Update calls in store_management.rs
sed -i 's/mentat_create_store/create_store/g' pg_mentat/src/functions/virtual_tables.rs

echo "Updating demo script..."
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

# Remove ::jsonb casts where they're not needed
sed -i 's/)::jsonb)/)/g' demo_sql_integration.sh
sed -i 's/\[::jsonb\]/[]/g' demo_sql_integration.sh

echo "Done! Functions renamed."
echo "Next steps:"
echo "  1. Review changes: git diff"
echo "  2. Build: cd pg_mentat && cargo pgrx install --release"
echo "  3. Test: ./demo_sql_integration.sh"
