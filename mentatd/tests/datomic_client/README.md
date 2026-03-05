# Datomic Client Testing

This directory contains scripts and instructions for testing mentatd with an actual Datomic client.

## Prerequisites

1. Datomic Free or Pro JAR files
2. Java Runtime Environment (JRE 8 or later)
3. Running mentatd server
4. PostgreSQL database

## Setup

### 1. Download Datomic

Download Datomic Free from [Datomic Downloads](https://www.datomic.com/get-datomic.html):

```bash
# Extract Datomic
unzip datomic-free-0.9.5697.zip
cd datomic-free-0.9.5697
```

### 2. Start mentatd

```bash
# In terminal 1
cd /Users/gregburd/src/mentat/mentatd
export DATABASE_URL="postgresql://localhost:5432/mentat"
cargo run
```

### 3. Run Test Client

```bash
# In terminal 2
cd tests/datomic_client
./test_client.sh
```

## Test Scripts

### `test_client.sh` - Basic Connection Test

Tests basic connectivity:
- Connect to mentatd on localhost:8080
- Create database
- List databases
- Execute simple query
- Transact data
- Delete database

### `test_queries.clj` - Comprehensive Query Tests

Clojure script testing:
- Schema installation
- Entity creation
- Datalog queries
- Pull API
- History queries
- Transaction functions

### `test_java_client.java` - Java Client Test

Java program demonstrating:
- Datomic Peer API usage
- Connection management
- Query execution
- Transaction handling
- Error handling

## Running Tests

### Basic Shell Script Test

```bash
./test_client.sh
```

Expected output:
```
Testing mentatd with Datomic client...
1. Connecting to mentatd at localhost:8080
2. Creating database: test_db
3. Listing databases
4. Querying database
5. Transacting data
6. Cleaning up

Test Results:
✓ Connection successful
✓ Database created
✓ Query executed
✓ Transaction committed
```

### Clojure REPL Test

```bash
cd datomic-free-0.9.5697
bin/repl

# In REPL:
(load-file "../tests/datomic_client/test_queries.clj")
(run-tests)
```

### Java Client Test

```bash
javac -cp datomic-free-0.9.5697/lib/datomic-free-0.9.5697.jar test_java_client.java
java -cp .:datomic-free-0.9.5697/lib/* TestDatomicClient
```

## Protocol Compatibility

### Supported Operations

- ✓ `connect` - Database connection
- ✓ `db` - Database handle
- ✓ `q` - Datalog query
- ✓ `transact` - Write transactions
- ✓ `create-database` - Database creation
- ✓ `delete-database` - Database deletion
- ✓ `list-databases` - Database enumeration

### Partial Support

- ⚠ `pull` - Basic pull patterns
- ⚠ `entity` - Entity API
- ⚠ `history` - Time-travel queries

### Not Yet Supported

- ✗ Transaction functions
- ✗ Excision
- ✗ Index range queries
- ✗ Seek datoms

## Test Results

Document your test results here:

### Test Run: [Date]

**Environment:**
- mentatd version: 0.1.0
- Datomic version: [version]
- PostgreSQL version: [version]

**Results:**
- [ ] Basic connection
- [ ] Database creation
- [ ] Query execution
- [ ] Transaction commit
- [ ] Pull API
- [ ] History queries

**Issues Found:**
- [List any issues or incompatibilities]

**Notes:**
- [Additional observations]

## Troubleshooting

### Connection refused

Ensure mentatd is running:
```bash
curl http://localhost:8080/health
```

### Invalid response format

Check mentatd logs:
```bash
RUST_LOG=debug cargo run
```

### Transaction failures

Verify PostgreSQL connection:
```bash
psql $DATABASE_URL -c "SELECT version();"
```

## Contributing

When adding new test cases:
1. Document the test in this README
2. Include expected vs actual behavior
3. Note any protocol differences from Datomic
4. Add error handling examples
