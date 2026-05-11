-- pg_mentat regression: large transaction stress test
-- Transact 100+ datoms in a single transaction

-- Setup schema
\echo Setup: schema for large transaction

SELECT mentat_transact('[
  {:db/ident :bulk/id
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one
   :db/unique :db.unique/identity}
  {:db/ident :bulk/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one}
  {:db/ident :bulk/score
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one}
]');

-- Test: transact 100 entities in a single batch
\echo Test: transact 100 entities in one batch

SELECT mentat_transact('[
  {:bulk/id 1 :bulk/name "entity-001" :bulk/score 10}
  {:bulk/id 2 :bulk/name "entity-002" :bulk/score 20}
  {:bulk/id 3 :bulk/name "entity-003" :bulk/score 30}
  {:bulk/id 4 :bulk/name "entity-004" :bulk/score 40}
  {:bulk/id 5 :bulk/name "entity-005" :bulk/score 50}
  {:bulk/id 6 :bulk/name "entity-006" :bulk/score 60}
  {:bulk/id 7 :bulk/name "entity-007" :bulk/score 70}
  {:bulk/id 8 :bulk/name "entity-008" :bulk/score 80}
  {:bulk/id 9 :bulk/name "entity-009" :bulk/score 90}
  {:bulk/id 10 :bulk/name "entity-010" :bulk/score 100}
  {:bulk/id 11 :bulk/name "entity-011" :bulk/score 110}
  {:bulk/id 12 :bulk/name "entity-012" :bulk/score 120}
  {:bulk/id 13 :bulk/name "entity-013" :bulk/score 130}
  {:bulk/id 14 :bulk/name "entity-014" :bulk/score 140}
  {:bulk/id 15 :bulk/name "entity-015" :bulk/score 150}
  {:bulk/id 16 :bulk/name "entity-016" :bulk/score 160}
  {:bulk/id 17 :bulk/name "entity-017" :bulk/score 170}
  {:bulk/id 18 :bulk/name "entity-018" :bulk/score 180}
  {:bulk/id 19 :bulk/name "entity-019" :bulk/score 190}
  {:bulk/id 20 :bulk/name "entity-020" :bulk/score 200}
  {:bulk/id 21 :bulk/name "entity-021" :bulk/score 210}
  {:bulk/id 22 :bulk/name "entity-022" :bulk/score 220}
  {:bulk/id 23 :bulk/name "entity-023" :bulk/score 230}
  {:bulk/id 24 :bulk/name "entity-024" :bulk/score 240}
  {:bulk/id 25 :bulk/name "entity-025" :bulk/score 250}
  {:bulk/id 26 :bulk/name "entity-026" :bulk/score 260}
  {:bulk/id 27 :bulk/name "entity-027" :bulk/score 270}
  {:bulk/id 28 :bulk/name "entity-028" :bulk/score 280}
  {:bulk/id 29 :bulk/name "entity-029" :bulk/score 290}
  {:bulk/id 30 :bulk/name "entity-030" :bulk/score 300}
  {:bulk/id 31 :bulk/name "entity-031" :bulk/score 310}
  {:bulk/id 32 :bulk/name "entity-032" :bulk/score 320}
  {:bulk/id 33 :bulk/name "entity-033" :bulk/score 330}
  {:bulk/id 34 :bulk/name "entity-034" :bulk/score 340}
  {:bulk/id 35 :bulk/name "entity-035" :bulk/score 350}
  {:bulk/id 36 :bulk/name "entity-036" :bulk/score 360}
  {:bulk/id 37 :bulk/name "entity-037" :bulk/score 370}
  {:bulk/id 38 :bulk/name "entity-038" :bulk/score 380}
  {:bulk/id 39 :bulk/name "entity-039" :bulk/score 390}
  {:bulk/id 40 :bulk/name "entity-040" :bulk/score 400}
  {:bulk/id 41 :bulk/name "entity-041" :bulk/score 410}
  {:bulk/id 42 :bulk/name "entity-042" :bulk/score 420}
  {:bulk/id 43 :bulk/name "entity-043" :bulk/score 430}
  {:bulk/id 44 :bulk/name "entity-044" :bulk/score 440}
  {:bulk/id 45 :bulk/name "entity-045" :bulk/score 450}
  {:bulk/id 46 :bulk/name "entity-046" :bulk/score 460}
  {:bulk/id 47 :bulk/name "entity-047" :bulk/score 470}
  {:bulk/id 48 :bulk/name "entity-048" :bulk/score 480}
  {:bulk/id 49 :bulk/name "entity-049" :bulk/score 490}
  {:bulk/id 50 :bulk/name "entity-050" :bulk/score 500}
  {:bulk/id 51 :bulk/name "entity-051" :bulk/score 510}
  {:bulk/id 52 :bulk/name "entity-052" :bulk/score 520}
  {:bulk/id 53 :bulk/name "entity-053" :bulk/score 530}
  {:bulk/id 54 :bulk/name "entity-054" :bulk/score 540}
  {:bulk/id 55 :bulk/name "entity-055" :bulk/score 550}
  {:bulk/id 56 :bulk/name "entity-056" :bulk/score 560}
  {:bulk/id 57 :bulk/name "entity-057" :bulk/score 570}
  {:bulk/id 58 :bulk/name "entity-058" :bulk/score 580}
  {:bulk/id 59 :bulk/name "entity-059" :bulk/score 590}
  {:bulk/id 60 :bulk/name "entity-060" :bulk/score 600}
  {:bulk/id 61 :bulk/name "entity-061" :bulk/score 610}
  {:bulk/id 62 :bulk/name "entity-062" :bulk/score 620}
  {:bulk/id 63 :bulk/name "entity-063" :bulk/score 630}
  {:bulk/id 64 :bulk/name "entity-064" :bulk/score 640}
  {:bulk/id 65 :bulk/name "entity-065" :bulk/score 650}
  {:bulk/id 66 :bulk/name "entity-066" :bulk/score 660}
  {:bulk/id 67 :bulk/name "entity-067" :bulk/score 670}
  {:bulk/id 68 :bulk/name "entity-068" :bulk/score 680}
  {:bulk/id 69 :bulk/name "entity-069" :bulk/score 690}
  {:bulk/id 70 :bulk/name "entity-070" :bulk/score 700}
  {:bulk/id 71 :bulk/name "entity-071" :bulk/score 710}
  {:bulk/id 72 :bulk/name "entity-072" :bulk/score 720}
  {:bulk/id 73 :bulk/name "entity-073" :bulk/score 730}
  {:bulk/id 74 :bulk/name "entity-074" :bulk/score 740}
  {:bulk/id 75 :bulk/name "entity-075" :bulk/score 750}
  {:bulk/id 76 :bulk/name "entity-076" :bulk/score 760}
  {:bulk/id 77 :bulk/name "entity-077" :bulk/score 770}
  {:bulk/id 78 :bulk/name "entity-078" :bulk/score 780}
  {:bulk/id 79 :bulk/name "entity-079" :bulk/score 790}
  {:bulk/id 80 :bulk/name "entity-080" :bulk/score 800}
  {:bulk/id 81 :bulk/name "entity-081" :bulk/score 810}
  {:bulk/id 82 :bulk/name "entity-082" :bulk/score 820}
  {:bulk/id 83 :bulk/name "entity-083" :bulk/score 830}
  {:bulk/id 84 :bulk/name "entity-084" :bulk/score 840}
  {:bulk/id 85 :bulk/name "entity-085" :bulk/score 850}
  {:bulk/id 86 :bulk/name "entity-086" :bulk/score 860}
  {:bulk/id 87 :bulk/name "entity-087" :bulk/score 870}
  {:bulk/id 88 :bulk/name "entity-088" :bulk/score 880}
  {:bulk/id 89 :bulk/name "entity-089" :bulk/score 890}
  {:bulk/id 90 :bulk/name "entity-090" :bulk/score 900}
  {:bulk/id 91 :bulk/name "entity-091" :bulk/score 910}
  {:bulk/id 92 :bulk/name "entity-092" :bulk/score 920}
  {:bulk/id 93 :bulk/name "entity-093" :bulk/score 930}
  {:bulk/id 94 :bulk/name "entity-094" :bulk/score 940}
  {:bulk/id 95 :bulk/name "entity-095" :bulk/score 950}
  {:bulk/id 96 :bulk/name "entity-096" :bulk/score 960}
  {:bulk/id 97 :bulk/name "entity-097" :bulk/score 970}
  {:bulk/id 98 :bulk/name "entity-098" :bulk/score 980}
  {:bulk/id 99 :bulk/name "entity-099" :bulk/score 990}
  {:bulk/id 100 :bulk/name "entity-100" :bulk/score 1000}
]');

-- Verify all 100 entities were stored
\echo Verify: count all bulk entities

SELECT mentat_query(
  '[:find (count ?e) :where [?e :bulk/id _]]',
  '{}'::jsonb
);

-- Verify data integrity: spot-check specific entities
\echo Verify: spot-check entity values

SELECT mentat_query(
  '[:find ?name ?score :where [?e :bulk/id 1] [?e :bulk/name ?name] [?e :bulk/score ?score]]',
  '{}'::jsonb
);

SELECT mentat_query(
  '[:find ?name ?score :where [?e :bulk/id 50] [?e :bulk/name ?name] [?e :bulk/score ?score]]',
  '{}'::jsonb
);

SELECT mentat_query(
  '[:find ?name ?score :where [?e :bulk/id 100] [?e :bulk/name ?name] [?e :bulk/score ?score]]',
  '{}'::jsonb
);

-- Verify aggregate: sum of all scores
\echo Verify: aggregate sum of scores

SELECT mentat_query(
  '[:find (sum ?score) :where [?e :bulk/score ?score] [?e :bulk/id _]]',
  '{}'::jsonb
);

-- Verify predicate query over large dataset
\echo Verify: predicate filter on large dataset

SELECT mentat_query(
  '[:find (count ?e) :where [?e :bulk/score ?s] [(>= ?s 900)]]',
  '{}'::jsonb
);
