#!/usr/bin/env bash
# pg_mentat Demo Script for asciinema
# Demonstrates: Build, Installation, Datalog Queries, mentatd, Clojure client

set -e

# Colors for terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo_header() {
    echo -e "\n${BLUE}========================================${NC}"
    echo -e "${BLUE}$1${NC}"
    echo -e "${BLUE}========================================${NC}\n"
}

echo_step() {
    echo -e "${GREEN}▶ $1${NC}"
}

echo_info() {
    echo -e "${YELLOW}ℹ $1${NC}"
}

# Intro
clear
echo_header "Welcome to pg_mentat!"
echo_info "pg_mentat is a Datomic-compatible Datalog query engine for PostgreSQL"
echo_info ""
echo_info "What makes it special:"
echo_info "  • Immutable facts with full history"
echo_info "  • Datalog queries (find, where clauses)"
echo_info "  • Schema-on-write with types and cardinality"
echo_info "  • Time-travel queries (as-of, since, history)"
echo_info "  • Compatible with Datomic clients"
echo_info ""
echo_info "In this demo, we'll:"
echo_info "  1. Build and install pg_mentat"
echo_info "  2. Load music data and run Datalog queries"
echo_info "  3. Show temporal queries and statistics"
echo_info "  4. Demonstrate the mentatd gateway"
echo_info "  5. Connect with a Clojure client"
sleep 3

# 1. Build
echo_header "Step 1: Building pg_mentat"
echo_step "First, let's build the PostgreSQL extension using cargo-pgrx"
echo_info "This compiles the Rust code and generates PostgreSQL bindings..."
sleep 2
cd /home/gburd/ws/pg_mentat/pg_mentat
cargo pgrx install --release

# 2. Start PostgreSQL and create database
echo_header "Step 2: Starting PostgreSQL"
echo_step "Starting PostgreSQL server in the background..."
sleep 1
pg_ctl -D ~/.pgrx/data-16 -l ~/.pgrx/16.log start || true
sleep 2
echo_step "Creating test database 'mentat_demo'..."
createdb mentat_demo || true
sleep 1

# 3. Install extension
echo_header "Step 3: Installing pg_mentat Extension"
echo_step "Let's verify the extension is available and install it..."
sleep 1
psql mentat_demo << 'EOF'
-- Show available extensions
\dx

-- Create the pg_mentat extension
CREATE EXTENSION IF NOT EXISTS pg_mentat;

-- Verify installation
\dx pg_mentat

-- Show mentat schema
\dn mentat

-- List mentat functions
\df mentat.*
EOF
sleep 2

# 4. Load demo data
echo_header "Step 4: Loading Music Data"
echo_step "Let's define a schema for artists, albums, and tracks..."
sleep 1
psql mentat_demo << 'EOF'
-- Define schema for music database
SELECT mentat_transact('[
  {:db/ident :artist/name
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "Artist name"}

  {:db/ident :artist/country
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "Country of origin"}

  {:db/ident :album/title
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "Album title"}

  {:db/ident :album/artist
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one
   :db/doc "Album artist (ref to artist entity)"}

  {:db/ident :album/year
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one
   :db/doc "Release year"}

  {:db/ident :track/title
   :db/valueType :db.type/string
   :db/cardinality :db.cardinality/one
   :db/doc "Track title"}

  {:db/ident :track/album
   :db/valueType :db.type/ref
   :db/cardinality :db.cardinality/one
   :db/doc "Track album (ref)"}

  {:db/ident :track/duration
   :db/valueType :db.type/long
   :db/cardinality :db.cardinality/one
   :db/doc "Duration in seconds"}
]');
EOF
sleep 2

echo_step "Now let's add some artists and albums..."
sleep 1
psql mentat_demo << 'EOF'
-- Add artists and albums
SELECT mentat_transact('[
  {:db/id "beatles"
   :artist/name "The Beatles"
   :artist/country "UK"}

  {:db/id "floyd"
   :artist/name "Pink Floyd"
   :artist/country "UK"}

  {:db/id "abbey-road"
   :album/title "Abbey Road"
   :album/artist "beatles"
   :album/year 1969}

  {:db/id "dark-side"
   :album/title "The Dark Side of the Moon"
   :album/artist "floyd"
   :album/year 1973}

  {:track/title "Come Together"
   :track/album "abbey-road"
   :track/duration 259}

  {:track/title "Something"
   :track/album "abbey-road"
   :track/duration 182}

  {:track/title "Time"
   :track/album "dark-side"
   :track/duration 413}

  {:track/title "Money"
   :track/album "dark-side"
   :track/duration 382}
]');
EOF
sleep 2

# 5. Datalog Queries
echo_header "Step 5: Datalog Queries"
echo_step "Let's query the data using Datalog syntax..."
echo_info "Find all artists and their countries:"
sleep 1
psql mentat_demo << 'EOF'
SELECT mentat_query('[:find ?name ?country
                      :where
                      [?artist :artist/name ?name]
                      [?artist :artist/country ?country]]',
                    '{}'::jsonb);
EOF
sleep 3

echo_info "Find albums from the 1970s with their artists:"
sleep 1
psql mentat_demo << 'EOF'
SELECT mentat_query('[:find ?album-title ?artist-name ?year
                      :where
                      [?album :album/title ?album-title]
                      [?album :album/year ?year]
                      [(>= ?year 1970)]
                      [(<= ?year 1979)]
                      [?album :album/artist ?artist]
                      [?artist :artist/name ?artist-name]]',
                    '{}'::jsonb);
EOF
sleep 3

echo_info "Find all tracks with their album and artist (join across refs):"
sleep 1
psql mentat_demo << 'EOF'
SELECT mentat_query('[:find ?track-title ?album-title ?artist-name ?duration
                      :where
                      [?track :track/title ?track-title]
                      [?track :track/duration ?duration]
                      [?track :track/album ?album]
                      [?album :album/title ?album-title]
                      [?album :album/artist ?artist]
                      [?artist :artist/name ?artist-name]]',
                    '{}'::jsonb);
EOF
sleep 3

# 6. Join with regular PostgreSQL
echo_header "Step 6: Hybrid Queries - Joining Mentat with Regular SQL"
echo_step "Create a regular PostgreSQL table with playlist data..."
sleep 1
psql mentat_demo << 'EOF'
-- Create a regular SQL table
CREATE TABLE IF NOT EXISTS playlists (
    id SERIAL PRIMARY KEY,
    name VARCHAR(100),
    track_title VARCHAR(100),
    added_date DATE DEFAULT CURRENT_DATE
);

INSERT INTO playlists (name, track_title) VALUES
    ('Favorites', 'Come Together'),
    ('Favorites', 'Time'),
    ('Road Trip', 'Money');

-- Show the SQL table
SELECT * FROM playlists;
EOF
sleep 2

echo_step "Now join Datalog results with SQL using a subquery..."
sleep 1
psql mentat_demo << 'EOF'
WITH mentat_tracks AS (
  SELECT
    result->0->>0 AS track_title,
    result->1->>0 AS album_title,
    result->2->>0 AS artist_name,
    (result->3->>0)::int AS duration
  FROM
    mentat_query('[:find ?track ?album ?artist ?duration
                   :where
                   [?track-ent :track/title ?track]
                   [?track-ent :track/duration ?duration]
                   [?track-ent :track/album ?album-ent]
                   [?album-ent :album/title ?album]
                   [?album-ent :album/artist ?artist-ent]
                   [?artist-ent :artist/name ?artist]]',
                 '{}'::jsonb) AS result
)
SELECT
  p.name AS playlist_name,
  p.track_title,
  m.artist_name,
  m.album_title,
  m.duration || ' seconds' AS duration,
  p.added_date
FROM playlists p
JOIN mentat_tracks m ON p.track_title = m.track_title
ORDER BY p.name, p.added_date;
EOF
sleep 3

# 7. Temporal Queries
echo_header "Step 7: Time Travel Queries"
echo_step "pg_mentat tracks full history - let's update an album year..."
sleep 1
psql mentat_demo << 'EOF'
-- Update album year (creates new fact, doesn't delete old one)
SELECT mentat_transact('[
  [:db/add [:album/title "Abbey Road"] :album/year 1970]
]');

-- Query current state
SELECT mentat_query('[:find ?title ?year
                      :where
                      [?album :album/title ?title]
                      [?album :album/year ?year]
                      [?album :album/title "Abbey Road"]]',
                    '{}'::jsonb);
EOF
sleep 2

echo_info "Now let's query the full history of this album's year attribute..."
sleep 1
psql mentat_demo << 'EOF'
-- Show all historical values (both 1969 and 1970)
SELECT
  e AS entity_id,
  a AS attribute,
  v_long AS value,
  tx AS transaction,
  added AS is_current
FROM mentat.datoms
WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/year')
  AND e IN (SELECT e FROM mentat.datoms WHERE a = (SELECT entid FROM mentat.idents WHERE ident = ':album/title') AND v_text = 'Abbey Road')
ORDER BY tx;
EOF
sleep 3

# 8. Statistics
echo_header "Step 8: Database Statistics"
echo_step "Let's explore some pg_mentat statistics..."
sleep 1
psql mentat_demo << 'EOF'
-- Schema overview
SELECT ident, cardinality::text, valuetype::text
FROM mentat.schema
WHERE ident LIKE ':artist/%' OR ident LIKE ':album/%' OR ident LIKE ':track/%'
ORDER BY ident;

-- Fact counts
SELECT
  'Total facts (datoms)' AS metric,
  COUNT(*) AS count
FROM mentat.datoms WHERE added = true
UNION ALL
SELECT
  'Total entities' AS metric,
  COUNT(DISTINCT e) AS count
FROM mentat.datoms WHERE added = true
UNION ALL
SELECT
  'Total transactions' AS metric,
  COUNT(*) AS count
FROM mentat.transactions
UNION ALL
SELECT
  'Schema attributes' AS metric,
  COUNT(*) AS count
FROM mentat.schema
WHERE ident LIKE ':%/%';
EOF
sleep 3

# 9. mentatd Gateway
echo_header "Step 9: Starting mentatd Gateway"
echo_step "mentatd provides an HTTP gateway with Datomic protocol compatibility..."
sleep 1
echo_info "Creating mentatd configuration..."
cd /home/gburd/ws/pg_mentat
cat > /tmp/mentatd_config.toml << 'TOMLEOF'
[server]
host = "127.0.0.1"
port = 8080

[database]
connection_string = "host=localhost dbname=mentat_demo"
pool_size = 10

[logging]
level = "info"
format = "compact"
TOMLEOF

echo_step "Building and starting mentatd..."
cd mentatd
cargo build --release 2>&1 | head -20
sleep 1

echo_step "Starting mentatd in background..."
./target/release/mentatd --config /tmp/mentatd_config.toml &
MENTATD_PID=$!
sleep 3

echo_step "Testing mentatd health check..."
curl -s http://localhost:8080/health
echo ""
sleep 2

echo_step "Query via HTTP (EDN format):"
curl -X POST http://localhost:8080/query \
  -H "Content-Type: application/edn" \
  -d '[:find ?name :where [?e :artist/name ?name]]' | head -100
echo ""
sleep 3

# 10. Clojure Client Demo
echo_header "Step 10: Clojure Client Demo"
echo_step "Let's create a simple Clojure client to demonstrate Datomic compatibility..."
sleep 1

cd /home/gburd/ws/pg_mentat
mkdir -p demo_clojure
cd demo_clojure

cat > deps.edn << 'CLJEDN'
{:deps {org.clojure/clojure {:mvn/version "1.11.1"}
        clj-http {:mvn/version "3.12.3"}
        cheshire {:mvn/version "5.11.0"}}}
CLJEDN

cat > demo.clj << 'CLJEOF'
(ns demo
  (:require [clj-http.client :as http]
            [cheshire.core :as json]))

(def mentatd-url "http://localhost:8080")

(defn query [q]
  (let [response (http/post (str mentatd-url "/query")
                           {:headers {"Content-Type" "application/edn"}
                            :body (pr-str q)
                            :as :json})]
    (:body response)))

(println "\n=== Clojure Client Demo ===\n")

(println "1. Query all artists:")
(let [results (query '[:find ?name ?country
                       :where
                       [?e :artist/name ?name]
                       [?e :artist/country ?country]])]
  (doseq [row results]
    (println "  " (first row) "-" (second row))))

(println "\n2. Query tracks with duration > 200 seconds:")
(let [results (query '[:find ?title ?duration
                       :where
                       [?e :track/title ?title]
                       [?e :track/duration ?duration]
                       [(> ?duration 200)]])]
  (doseq [row results]
    (println "  " (first row) "-" (second row) "seconds")))

(println "\n3. Complex join query (tracks with artist info):")
(let [results (query '[:find ?track ?artist ?album
                       :where
                       [?t :track/title ?track]
                       [?t :track/album ?a]
                       [?a :album/title ?album]
                       [?a :album/artist ?ar]
                       [?ar :artist/name ?artist]])]
  (doseq [row results]
    (apply println "  Track:" row)))

(println "\n✓ Clojure client successfully connected to mentatd!")
(println "  pg_mentat is Datomic-compatible!\n")
CLJEOF

echo_step "Running Clojure client..."
clojure -M demo.clj
sleep 3

# Cleanup
echo_header "Demo Complete!"
echo_info "Cleaning up..."
kill $MENTATD_PID 2>/dev/null || true
sleep 1

echo ""
echo_info "What we demonstrated:"
echo_info "  ✓ Built and installed pg_mentat extension"
echo_info "  ✓ Defined schema and loaded data"
echo_info "  ✓ Ran Datalog queries with joins across refs"
echo_info "  ✓ Joined Mentat data with regular SQL tables"
echo_info "  ✓ Queried temporal history"
echo_info "  ✓ Viewed database statistics"
echo_info "  ✓ Used mentatd HTTP gateway"
echo_info "  ✓ Connected with Datomic-compatible Clojure client"
echo ""
echo_info "Next steps:"
echo_info "  • Try pg_vector for semantic search"
echo_info "  • Add pg_textscale for BM25 full-text search"
echo_info "  • Connect via MCP for LLM integration"
echo_info "  • Read docs/ops/ for production deployment"
echo ""
echo_header "Thank you for watching!"
sleep 2
