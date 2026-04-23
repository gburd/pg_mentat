# pg_mentat_cli

Interactive Datalog shell for PostgreSQL with the `pg_mentat` extension.

## Build

```bash
cargo build --release -p pg_mentat_cli
```

## Usage

```bash
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
```

Multi-line input is supported. The REPL waits for balanced brackets before executing.

## SQL Pass-through

SQL statements starting with `SELECT`, `INSERT`, `UPDATE`, `DELETE`, `CREATE`, `DROP`, `ALTER`, `EXPLAIN`, or `WITH` are passed directly to PostgreSQL:

```
mentat=> SELECT * FROM mentat_datoms LIMIT 5;
```
