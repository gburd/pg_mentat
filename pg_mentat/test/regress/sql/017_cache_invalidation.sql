-- Test cross-backend cache invalidation
-- Phase B: Generation counter check-on-access

-- Verify cache_generation table exists and has default row
SELECT gen FROM mentat.cache_generation WHERE store_name = 'default';

-- Transact a schema change (should bump generation)
SELECT mentat_transact('[
  [:db/add "attr" :db/ident :cache-test/field]
  [:db/add "attr" :db/valueType :db.type/string]
  [:db/add "attr" :db/cardinality :db.cardinality/one]
]');

-- Verify generation was bumped (should be > 1)
SELECT gen FROM mentat.cache_generation WHERE store_name = 'default';

-- Transact pure data (should NOT bump generation)
SELECT mentat_transact('[[:db/add "e1" :cache-test/field "hello"]]');

-- Verify generation stayed the same
SELECT gen FROM mentat.cache_generation WHERE store_name = 'default';

-- Query the data to verify cache works
SELECT mentat_query(
  '[:find ?v :where [?e :cache-test/field ?v]]',
  '{}'
);
