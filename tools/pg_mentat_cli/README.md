# pg_mentat_cli

Interactive Datalog shell for PostgreSQL with the `pg_mentat` extension.

## Build

```bash
cargo build --release -p pg_mentat_cli
```

## Usage

```bash
# Connect with a PostgreSQL URL
pg_mentat_cli postgresql://localhost/mentat

# Connect with defaults (localhost:5432, database=mentat, user=postgres)
pg_mentat_cli

# Specify connection parameters
pg_mentat_cli --host db.example.com --port 5432 -d mydb -U myuser

# Use a full connection string
pg_mentat_cli -c "host=localhost dbname=mentat user=postgres"

# Execute a query and exit
pg_mentat_cli -q '[:find ?e ?ident :where [?e :db/ident ?ident]]'

# Execute a transaction and exit
pg_mentat_cli -t '[{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]'

# Execute raw SQL and exit
pg_mentat_cli --sql "SELECT count(*) FROM mentat_datoms"

# Disable readline/TTY support (for piping)
echo '[:find ?e :where [?e :db/ident _]]' | pg_mentat_cli --no-tty
```

## REPL Commands

| Command | Description |
|---------|-------------|
| `.help` | Show help message |
| `.schema` | Show all schema attributes |
| `.stats` | Show query and function statistics |
| `.storage` | Show storage statistics |
| `.entity <id>` | Show all datoms for an entity |
| `.timer on\|off` | Toggle query timing |
| `.explain <query>` | Show EXPLAIN ANALYZE plan for a Datalog query |
| `.export <id> ...` | Export entities as JSON (pull [*]) |
| `.import <file>` | Import an EDN transaction file |
| `.cache_stats` | Show prepared statement cache statistics |
| `.clear_cache` | Clear prepared statement cache |
| `.sql <stmt>` | Execute raw SQL |
| `.exit` | Exit the REPL |

## Datalog Input

Enter Datalog queries and transactions directly:

```
mentat=> [:find ?e ?ident :where [?e :db/ident ?ident]]
mentat=> [{:db/ident :person/name :db/valueType :db.type/string :db/cardinality :db.cardinality/one}]
mentat=> (pull 42 [*])
mentat=> (pull 42 [:person/name {:person/friends 3}])
mentat=> (pull-many [:person/name :person/age] [100 101 102])
```

Multi-line input is supported. The REPL waits for balanced brackets before executing.

## Tab Completion

Press Tab to complete:
- **Dot-commands**: `.sch<Tab>` completes to `.schema`
- **Keywords**: `:db/val<Tab>` completes to `:db/valueType`
- **Schema attributes**: `:person/<Tab>` shows all `:person/*` attributes from your schema

Schema idents are loaded on connect and refreshed automatically after transactions.

## SQL Pass-through

SQL statements starting with `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `DROP`, `ALTER`, `EXPLAIN`, or `WITH` are passed directly to PostgreSQL:

```
mentat=> SELECT * FROM mentat.datoms LIMIT 5;
```
