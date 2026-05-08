# pg_mentat Docker Demo

Run a PostgreSQL instance with the pg_mentat Datalog extension pre-installed.

## Quick Start

```bash
# Build the image
docker build -t pg_mentat .

# Run the container (demo data loads automatically on first start)
docker run -d --name pg_mentat -p 5432:5432 pg_mentat

# Connect
psql -h localhost -U postgres
```

## Using Podman

```bash
podman build -t pg_mentat .
podman run -d --name pg_mentat -p 5432:5432 pg_mentat
psql -h localhost -U postgres
```

## What the Demo Does

The `demo.sql` script runs on first container start and:

1. Creates the `pg_mentat` extension
2. Defines schema attributes (`:person/name`, `:person/age`, `:person/email`)
3. Inserts sample data (Alice, Bob, Carol)
4. Runs example queries

## Try It Yourself

After connecting with `psql`:

```sql
-- Transact new data
SELECT mentat_transact('[
  [:db/add "dave" :person/name "Dave"]
  [:db/add "dave" :person/age 40]
]'::TEXT);

-- Query: find all people and their ages
SELECT mentat_query(
  '[:find ?name ?age :where [?e :person/name ?name] [?e :person/age ?age]]'::TEXT,
  '{}'::jsonb
);

-- Query with parameter binding: find person with age 40
SELECT mentat_query(
  '[:find ?name :in ?age :where [?e :person/age ?age] [?e :person/name ?name]]'::TEXT,
  '{"inputs": [40]}'::jsonb
);

-- View an entity by ID (replace 65 with an actual entity ID from query results)
-- SELECT mentat_entity(65);

-- Pull specific attributes for an entity
-- SELECT mentat_pull('[:person/name :person/age]', 65);

-- View the full schema
SELECT mentat_schema();
```

## Available Functions

| Function | Description |
|---|---|
| `mentat_transact(edn TEXT)` | Execute an EDN transaction (add/retract datoms) |
| `mentat_query(query TEXT, inputs JSONB)` | Run a Datalog query with optional input bindings |
| `mentat_entity(entity_id BIGINT)` | Fetch all attributes for an entity as JSON |
| `mentat_pull(pattern TEXT, entity_id BIGINT)` | Pull specific attributes for an entity |
| `mentat_schema()` | View the complete schema definition |

## Configuration

Set a password for the postgres user:

```bash
docker run -d --name pg_mentat -p 5432:5432 -e POSTGRES_PASSWORD=secret pg_mentat
```

## Rebuild

If you modify the source code, rebuild the image:

```bash
docker build --no-cache -t pg_mentat .
docker rm -f pg_mentat
docker run -d --name pg_mentat -p 5432:5432 pg_mentat
```
