#!/usr/bin/env bash
# Run SQL integration tests for pg_mentat extension
set -e

PGHOST=/tmp
PGPORT=28816
DB=test_pg_mentat_integration

echo "=============================================="
echo "pg_mentat SQL Integration Test Suite"
echo "=============================================="
echo

# Create fresh test database
echo "Creating test database..."
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true
createdb -h $PGHOST -p $PGPORT $DB
psql -h $PGHOST -p $PGPORT $DB -c "CREATE EXTENSION pg_mentat;"
echo "✓ Extension installed"
echo

# Count test files
test_dir="pg_mentat/sql/tests"
total_tests=$(ls -1 $test_dir/test_*.sql 2>/dev/null | wc -l)
passed=0
failed=0

echo "Running $total_tests SQL integration test files..."
echo

# Run each test file
for test_file in $test_dir/test_*.sql; do
    test_name=$(basename "$test_file" .sql)
    echo -n "Testing $test_name... "

    if psql -X -h $PGHOST -p $PGPORT $DB -f "$test_file" > /tmp/test_${test_name}.log 2>&1; then
        echo "✓ PASS"
        ((passed++))
    else
        echo "✗ FAIL"
        echo "  Error log: /tmp/test_${test_name}.log"
        ((failed++))
        # Show first few lines of error
        echo "  First error lines:"
        head -20 /tmp/test_${test_name}.log | sed 's/^/    /'
    fi
done

echo
echo "=============================================="
echo "Test Summary:"
echo "  Total:  $total_tests"
echo "  Passed: $passed"
echo "  Failed: $failed"
echo "=============================================="

# Clean up
dropdb -h $PGHOST -p $PGPORT $DB 2>/dev/null || true

if [ $failed -eq 0 ]; then
    echo "✓ All integration tests passed!"
    exit 0
else
    echo "✗ Some tests failed"
    exit 1
fi
