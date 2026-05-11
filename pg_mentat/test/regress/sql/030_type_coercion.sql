-- pg_mentat regression: cross-type predicate safety
-- Predicates on mismatched types should return empty or handle gracefully,
-- never crash or produce SQL type errors

-- Setup: attributes of various types
\echo Setup: multi-type schema

SELECT mentat_transact('[
  {:db/ident :tc/str
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :tc/num
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
  {:db/ident :tc/flag
   :db/valueType :db.type/boolean
   :db/cardinality :db.cardinality/one}
  {:db/ident :tc/label
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
]');

SELECT mentat_transact('[
  {:tc/str "hello" :tc/num 42 :tc/flag true :tc/label "item-a"}
  {:tc/str "world" :tc/num 99 :tc/flag false :tc/label "item-b"}
  {:tc/str "123" :tc/num 123 :tc/flag true :tc/label "item-c"}
]');

-- Test: numeric comparison on string variable → empty result (not error)
\echo Test: numeric predicate on string var returns empty

SELECT mentat_query(
  '[:find ?s :where [?e :tc/str ?s] [(> ?s 50)]]',
  '{}'::jsonb
);

-- Test: string comparison on numeric variable → empty result (not error)
\echo Test: string predicate on numeric var returns empty

SELECT mentat_query(
  '[:find ?n :where [?e :tc/num ?n] [(like ?n "%abc%")]]',
  '{}'::jsonb
);

-- Test: same-type numeric comparison works correctly
\echo Test: same-type numeric comparison works

SELECT mentat_query(
  '[:find ?n :where [?e :tc/num ?n] [(> ?n 50)] :order (asc ?n)]',
  '{}'::jsonb
);

-- Test: same-type string comparison works correctly
\echo Test: same-type string equality works

SELECT mentat_query(
  '[:find ?s :where [?e :tc/str ?s] [(= ?s "hello")]]',
  '{}'::jsonb
);

-- Test: boolean attribute with equality predicate
\echo Test: boolean equality predicate

SELECT mentat_query(
  '[:find ?label :where [?e :tc/label ?label] [?e :tc/flag true] :order (asc ?label)]',
  '{}'::jsonb
);

-- Test: mixed typed joins are safe
\echo Test: joining across typed tables is safe

SELECT mentat_query(
  '[:find ?s ?n :where [?e :tc/str ?s] [?e :tc/num ?n] [(> ?n 50)] :order (asc ?n)]',
  '{}'::jsonb
);

-- Test: predicate with input binding of wrong type → empty (not crash)
\echo Test: wrong-type input binding returns empty

SELECT mentat_query(
  '[:find ?label :where [?e :tc/label ?label] [?e :tc/num ?n] [(> ?n ?limit)] :in ?limit]',
  '{"?limit": "not-a-number"}'::jsonb
);
