-- Create the mentat schema
-- This must be the first step in extension installation

CREATE SCHEMA mentat;

-- Grant usage to public
GRANT USAGE ON SCHEMA mentat TO PUBLIC;

-- Store metadata table: tracks named stores and their backing schemas
CREATE TABLE IF NOT EXISTS mentat.stores (
    store_name TEXT PRIMARY KEY,
    schema_name TEXT NOT NULL UNIQUE,
    description TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Register the default store
INSERT INTO mentat.stores (store_name, schema_name, description)
VALUES ('default', 'mentat', 'Default mentat store')
ON CONFLICT (store_name) DO NOTHING;

-- Subscription metadata table: tracks active LISTEN/NOTIFY subscriptions
CREATE TABLE IF NOT EXISTS mentat.subscriptions (
    id SERIAL PRIMARY KEY,
    store_name TEXT NOT NULL,
    name TEXT NOT NULL,
    query TEXT NOT NULL,
    created_at TIMESTAMPTZ DEFAULT now(),
    UNIQUE(store_name, name)
);

-- Materialized view metadata table: tracks Datalog-powered materialized views
CREATE TABLE IF NOT EXISTS mentat.materialized_views (
    id SERIAL PRIMARY KEY,
    store_name TEXT NOT NULL,
    view_name TEXT NOT NULL,
    datalog_query TEXT NOT NULL,
    refresh_policy TEXT DEFAULT 'manual',
    created_at TIMESTAMPTZ DEFAULT now(),
    UNIQUE(store_name, view_name)
);
