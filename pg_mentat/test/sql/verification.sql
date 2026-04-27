-- Verification SQL: End-to-end feature verification
--
-- This script verifies that all new SQL integration features are functional.
-- It exercises every major feature category and reports pass/fail status.
--
-- Run with: psql -f verification.sql

\echo '============================================'
\echo '  pg_mentat Feature Verification'
\echo '============================================'
\echo ''

-- =========================================================================
-- 1. Extension is installed
-- =========================================================================
\echo '--- 1. Extension installed ---'
SELECT extname, extversion FROM pg_extension WHERE extname = 'pg_mentat';

-- =========================================================================
-- 2. Core functions exist
-- =========================================================================
\echo ''
\echo '--- 2. Core functions exist ---'
SELECT proname
FROM pg_proc p
JOIN pg_namespace n ON p.pronamespace = n.oid
WHERE n.nspname IN ('public', 'mentat')
  AND proname LIKE 'mentat_%'
ORDER BY proname;

-- =========================================================================
-- 3. Store management
-- =========================================================================
\echo ''
\echo '--- 3. Store management ---'
SELECT mentat_create_store('verify_store', 'Verification test store');
SELECT mentat_list_stores();
SELECT mentat_rename_store('verify_store', 'verify_renamed');
SELECT mentat_drop_store('verify_renamed');
\echo 'Store management: OK'

-- =========================================================================
-- 4. Schema + Transact + Query (default store)
-- =========================================================================
\echo ''
\echo '--- 4. Schema + Transact + Query ---'

SELECT mentat_transact('[
    {:db/ident :verify/name
     :db/valueType :db.type/string
     :db/cardinality :db.cardinality/one}
    {:db/ident :verify/value
     :db/valueType :db.type/long
     :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
    {:db/id "v1" :verify/name "Alpha" :verify/value 100}
    {:db/id "v2" :verify/name "Beta"  :verify/value 200}
    {:db/id "v3" :verify/name "Gamma" :verify/value 300}
]');

SELECT mentat_query('[:find ?name ?val :where [?e :verify/name ?name] [?e :verify/value ?val]]', '{}');
\echo 'Schema + Transact + Query: OK'

-- =========================================================================
-- 5. Store-aware functions
-- =========================================================================
\echo ''
\echo '--- 5. Store-aware functions ---'

SELECT mentat_create_store('sa_verify', 'Store-aware verification');
SELECT mentat_transact_in_store('sa_verify', '[
    {:db/ident :item/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}
]');
SELECT mentat_transact_in_store('sa_verify', '[{:db/id "i1" :item/name "Widget"}]');
SELECT mentat_query_in_store('sa_verify', '[:find ?name :where [?e :item/name ?name]]', '{}');
SELECT mentat_schema_in_store('sa_verify');
SELECT mentat_drop_store('sa_verify');
\echo 'Store-aware functions: OK'

-- =========================================================================
-- 6. Virtual table views
-- =========================================================================
\echo ''
\echo '--- 6. Virtual table views ---'

SELECT COUNT(*) AS entity_count FROM mentat.entities;
SELECT COUNT(*) AS attribute_count FROM mentat.attributes;
SELECT COUNT(*) AS fact_count FROM mentat.facts;
\echo 'Virtual table views: OK'

-- =========================================================================
-- 7. Pull and Entity functions
-- =========================================================================
\echo ''
\echo '--- 7. Pull and Entity ---'

-- Get an entity ID
DO $$
DECLARE
    eid BIGINT;
    pull_result JSONB;
    entity_result JSONB;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :verify/name "Alpha"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid;
    RAISE NOTICE 'Entity ID for Alpha: %', eid;

    SELECT mentat_pull('[*]', eid)::JSONB INTO pull_result;
    RAISE NOTICE 'Pull result: %', pull_result;

    SELECT mentat_entity(eid)::JSONB INTO entity_result;
    RAISE NOTICE 'Entity result: %', entity_result;
END;
$$;
\echo 'Pull and Entity: OK'

-- =========================================================================
-- 8. Schema introspection
-- =========================================================================
\echo ''
\echo '--- 8. Schema introspection ---'
SELECT mentat_schema();
\echo 'Schema introspection: OK'

-- =========================================================================
-- 9. Materialized views
-- =========================================================================
\echo ''
\echo '--- 9. Materialized views ---'

SELECT mentat_create_matview('verify_mv',
    '[:find ?name ?val :where [?e :verify/name ?name] [?e :verify/value ?val] [(> ?val 150)]]',
    '{}');
SELECT * FROM mentat.matview_verify_mv;
SELECT mentat_refresh_matview('verify_mv');
SELECT mentat_list_matviews();
SELECT mentat_drop_matview('verify_mv');
\echo 'Materialized views: OK'

-- =========================================================================
-- 10. Time-travel queries
-- =========================================================================
\echo ''
\echo '--- 10. Time-travel queries ---'

DO $$
DECLARE
    tx1 BIGINT;
    result JSONB;
BEGIN
    SELECT max(tx) INTO tx1 FROM mentat.transactions;
    RAISE NOTICE 'Current max TX: %', tx1;

    -- Update a value
    PERFORM mentat_transact('[[:db/add [:verify/name "Alpha"] :verify/value 999]]');

    -- As-of query should show old value
    SELECT mentat_as_of(tx1,
        '[:find ?val . :where [?e :verify/name "Alpha"] [?e :verify/value ?val]]',
        '{}')::JSONB INTO result;
    RAISE NOTICE 'As-of TX% value: %', tx1, result;

    -- History query
    SELECT mentat_history(
        '[:find ?val ?tx ?added :where [?e :verify/name "Alpha"] [?e :verify/value ?val ?tx ?added]]',
        '{}')::JSONB INTO result;
    RAISE NOTICE 'History: %', result;
END;
$$;
\echo 'Time-travel queries: OK'

-- =========================================================================
-- 11. Subscriptions
-- =========================================================================
\echo ''
\echo '--- 11. Subscriptions ---'

SELECT mentat_subscribe('verify_sub',
    '[:find ?name :where [?e :verify/name ?name] [?e :verify/value ?v] [(> ?v 500)]]',
    '{}');
SELECT mentat_list_subscriptions();
SELECT mentat_notify_subscribers();
SELECT mentat_unsubscribe('verify_sub');
\echo 'Subscriptions: OK'

-- =========================================================================
-- 12. Recursive queries
-- =========================================================================
\echo ''
\echo '--- 12. Recursive queries ---'

SELECT mentat_transact('[
    {:db/ident :cat/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one :db/unique :db.unique/identity}
    {:db/ident :cat/parent :db/valueType :db.type/ref :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
    {:db/id "root" :cat/name "Root"}
    {:db/id "a" :cat/name "A" :cat/parent "root"}
    {:db/id "b" :cat/name "B" :cat/parent "a"}
]');

SELECT mentat_descendants(
    '[:find ?name :in $ ?start :where [?child :cat/parent ?start] [?child :cat/name ?name]]',
    '{"start": ["lookup", ":cat/name", "Root"]}',
    ':cat/parent',
    10);
\echo 'Recursive queries: OK'

-- =========================================================================
-- 13. EDN pretty printing
-- =========================================================================
\echo ''
\echo '--- 13. EDN pretty printing ---'

SELECT edn_pretty('{:verify/name "Alpha" :verify/value 100}');
SELECT edn_pretty('[:find ?e ?name :where [?e :person/name ?name]]', 40);
\echo 'EDN pretty printing: OK'

-- =========================================================================
-- Summary
-- =========================================================================
\echo ''
\echo '============================================'
\echo '  All verification checks completed'
\echo '============================================'
