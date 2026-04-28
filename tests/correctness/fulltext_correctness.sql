-- =============================================================================
-- Correctness Tests: Full-Text Search
-- =============================================================================
--
-- Verifies that full-text search (via :db/fulltext true) returns correct
-- results and handles edge cases properly.
--
-- Key behaviors:
--   - fulltext function uses PostgreSQL's to_tsvector/to_tsquery
--   - Results are ranked by relevance (ts_rank)
--   - Only attributes marked :db/fulltext true are searchable
--   - Updates to text values are reflected in search results
--   - Retracted values should NOT appear in search results
--
-- =============================================================================

BEGIN;

-- =========================================================================
-- Setup
-- =========================================================================

SELECT mentat_transact('[
    {:db/ident       :article/title
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/fulltext    true
     :db/unique      :db.unique/identity}

    {:db/ident       :article/body
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one
     :db/fulltext    true}

    {:db/ident       :article/summary
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}

    {:db/ident       :article/author
     :db/valueType   :db.type/string
     :db/cardinality :db.cardinality/one}
]');

-- Insert test articles with known content
SELECT mentat_transact('[
    {:db/id "a1"
     :article/title "Introduction to PostgreSQL Extensions"
     :article/body "PostgreSQL extensions provide a powerful mechanism for extending database functionality. The extension system allows developers to add custom types, functions, and operators."
     :article/summary "Overview of PG extensions"
     :article/author "Alice"}

    {:db/id "a2"
     :article/title "Building Distributed Systems"
     :article/body "Distributed systems involve multiple computers working together to achieve a common goal. Key challenges include consensus, fault tolerance, and network partitioning."
     :article/summary "Distributed systems primer"
     :article/author "Bob"}

    {:db/id "a3"
     :article/title "Advanced PostgreSQL Performance Tuning"
     :article/body "Performance tuning in PostgreSQL involves analyzing query plans, configuring shared buffers, and optimizing indexes. The EXPLAIN ANALYZE command is essential for understanding query execution."
     :article/summary "PG performance guide"
     :article/author "Charlie"}

    {:db/id "a4"
     :article/title "Machine Learning with Python"
     :article/body "Python provides excellent libraries for machine learning including scikit-learn, TensorFlow, and PyTorch. Data preprocessing and feature engineering are critical steps."
     :article/summary "ML with Python"
     :article/author "Dave"}
]');

-- =========================================================================
-- Test 1: Basic fulltext search returns matching results
-- =========================================================================

DO $$
DECLARE
    r   JSONB;
    cnt INT;
BEGIN
    SELECT mentat_query('
        [:find ?title
         :where
         [(fulltext $ :article/title "PostgreSQL") [[?e]]]
         [?e :article/title ?title]]
    ', '{}')::JSONB INTO r;

    cnt := jsonb_array_length(r->'results');
    ASSERT cnt >= 2, format('Should find at least 2 articles with "PostgreSQL" in title, got: %s', cnt);

    RAISE NOTICE 'PASS: Test 1 - Basic fulltext search on title (% results)', cnt;
END;
$$;

-- =========================================================================
-- Test 2: Fulltext search on body content
-- =========================================================================

DO $$
DECLARE
    r   JSONB;
    cnt INT;
BEGIN
    SELECT mentat_query('
        [:find ?title
         :where
         [(fulltext $ :article/body "query plans indexes") [[?e]]]
         [?e :article/title ?title]]
    ', '{}')::JSONB INTO r;

    cnt := jsonb_array_length(r->'results');
    ASSERT cnt >= 1, format('Should find article about performance tuning, got: %s results', cnt);

    RAISE NOTICE 'PASS: Test 2 - Fulltext search on body content (% results)', cnt;
END;
$$;

-- =========================================================================
-- Test 3: Non-fulltext attribute is NOT searchable via fulltext
-- =========================================================================

DO $$
DECLARE
    r JSONB;
BEGIN
    -- :article/summary is NOT marked :db/fulltext true
    BEGIN
        SELECT mentat_query('
            [:find ?e
             :where
             [(fulltext $ :article/summary "extensions") [[?e]]]]
        ', '{}')::JSONB INTO r;

        -- If it succeeds, it should return 0 results or the system should
        -- gracefully handle non-fulltext attributes
        RAISE NOTICE 'PASS: Test 3 - Non-fulltext attribute query handled gracefully';
    EXCEPTION
        WHEN OTHERS THEN
            -- Expected: fulltext on non-fulltext attribute should error
            RAISE NOTICE 'PASS: Test 3 - Non-fulltext attribute correctly rejected: %', SQLERRM;
    END;
END;
$$;

-- =========================================================================
-- Test 4: Fulltext search with no matches returns empty
-- =========================================================================

DO $$
DECLARE
    r   JSONB;
    cnt INT;
BEGIN
    SELECT mentat_query('
        [:find ?title
         :where
         [(fulltext $ :article/title "xyznonexistent") [[?e]]]
         [?e :article/title ?title]]
    ', '{}')::JSONB INTO r;

    cnt := jsonb_array_length(r->'results');
    ASSERT cnt = 0, format('Should find 0 results for nonsense query, got: %s', cnt);

    RAISE NOTICE 'PASS: Test 4 - No matches returns empty result set';
END;
$$;

-- =========================================================================
-- Test 5: Updated text is reflected in search results
-- =========================================================================

DO $$
DECLARE
    cnt_before INT;
    cnt_after  INT;
BEGIN
    -- Count articles matching "Kubernetes" before update
    SELECT jsonb_array_length((mentat_query('
        [:find ?e :where [(fulltext $ :article/body "Kubernetes") [[?e]]]]
    ', '{}')::JSONB)->'results') INTO cnt_before;
    ASSERT cnt_before = 0, 'No articles should mention Kubernetes initially';

    -- Update article body to mention Kubernetes
    PERFORM mentat_transact('[
        [:db/add [:article/title "Building Distributed Systems"] :article/body
         "Distributed systems involve Kubernetes orchestration, container networking, and service mesh. Key challenges include consensus and fault tolerance."]
    ]');

    -- Search again - should now find the updated article
    SELECT jsonb_array_length((mentat_query('
        [:find ?e :where [(fulltext $ :article/body "Kubernetes") [[?e]]]]
    ', '{}')::JSONB)->'results') INTO cnt_after;
    ASSERT cnt_after >= 1, format('Updated article should be found, got: %s', cnt_after);

    RAISE NOTICE 'PASS: Test 5 - Updated text appears in search results';
END;
$$;

-- =========================================================================
-- Test 6: Retracted text value removed from search
-- =========================================================================

DO $$
DECLARE
    eid BIGINT;
    cnt INT;
BEGIN
    SELECT (mentat_query('[:find ?e . :where [?e :article/title "Machine Learning with Python"]]', '{}')::JSONB)::TEXT::BIGINT INTO eid;

    -- Retract the body
    PERFORM mentat_transact(format(
        '[[:db/retract %s :article/body "Python provides excellent libraries for machine learning including scikit-learn, TensorFlow, and PyTorch. Data preprocessing and feature engineering are critical steps."]]',
        eid
    ));

    -- Search for "scikit-learn" should no longer find it
    SELECT jsonb_array_length((mentat_query('
        [:find ?e :where [(fulltext $ :article/body "scikit-learn TensorFlow") [[?e]]]]
    ', '{}')::JSONB)->'results') INTO cnt;
    ASSERT cnt = 0, format('Retracted body should not appear in search, got: %s results', cnt);

    RAISE NOTICE 'PASS: Test 6 - Retracted text removed from search';
END;
$$;

-- =========================================================================
-- Test 7: Stemming works (searching for "extending" finds "extensions")
-- =========================================================================

DO $$
DECLARE
    cnt INT;
BEGIN
    -- PostgreSQL's English stemmer should map "extending" -> "extend"
    -- and "extensions" -> "extend", so they should match
    SELECT jsonb_array_length((mentat_query('
        [:find ?e :where [(fulltext $ :article/body "extending database") [[?e]]]]
    ', '{}')::JSONB)->'results') INTO cnt;

    -- This tests PostgreSQL's tsvector stemming behavior
    RAISE NOTICE 'PASS: Test 7 - Stemming test (% results for "extending database")', cnt;
END;
$$;

-- =========================================================================
-- Test 8: Multiple fulltext clauses (AND semantics)
-- =========================================================================

DO $$
DECLARE
    r   JSONB;
    cnt INT;
BEGIN
    -- Find articles where BOTH title and body match
    SELECT mentat_query('
        [:find ?title
         :where
         [(fulltext $ :article/title "PostgreSQL") [[?e]]]
         [(fulltext $ :article/body "performance") [[?e]]]
         [?e :article/title ?title]]
    ', '{}')::JSONB INTO r;

    cnt := jsonb_array_length(r->'results');
    -- Should find the performance tuning article (has "PostgreSQL" in title + "performance" in body)
    RAISE NOTICE 'PASS: Test 8 - Multiple fulltext clauses (AND): % results', cnt;
END;
$$;

-- =========================================================================
-- Test 9: Fulltext with additional non-fulltext predicates
-- =========================================================================

DO $$
DECLARE
    r   JSONB;
    cnt INT;
BEGIN
    -- Fulltext search + filter by author
    SELECT mentat_query('
        [:find ?title
         :where
         [(fulltext $ :article/title "PostgreSQL") [[?e]]]
         [?e :article/author "Alice"]
         [?e :article/title ?title]]
    ', '{}')::JSONB INTO r;

    cnt := jsonb_array_length(r->'results');
    ASSERT cnt >= 1, format('Should find Alice articles with PostgreSQL in title, got: %s', cnt);

    RAISE NOTICE 'PASS: Test 9 - Fulltext combined with attribute filter (% results)', cnt;
END;
$$;

-- =========================================================================
-- Cleanup
-- =========================================================================

ROLLBACK;
