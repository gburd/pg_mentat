#!/run/current-system/sw/bin/fish

# Test script to verify concurrency improvements
# Requires mentatd to be running

set -l BASE_URL "http://localhost:8080"
set -l ITERATIONS 100
set -l CONCURRENCY 50

echo "Testing mentatd concurrency improvements..."
echo "Target: $CONCURRENCY concurrent connections, $ITERATIONS requests each"
echo ""

# Simple query to test caching and concurrency
set -l QUERY '{"op":"query","query":"[:find ?e :where [?e :name _]]","args":[]}'

# Function to make a request
function make_request
    curl -s -X POST \
         -H "Content-Type: application/edn" \
         -d "$QUERY" \
         "$BASE_URL/" > /dev/null 2>&1
    echo -n "."
end

# Check if server is running
if not curl -s "$BASE_URL/health" > /dev/null 2>&1
    echo "Error: mentatd server is not running on $BASE_URL"
    echo "Please start the server first with: cargo run --bin mentatd"
    exit 1
end

echo "Server is running. Starting load test..."
set -l start_time (date +%s)

# Run concurrent requests
for i in (seq 1 $CONCURRENCY)
    for j in (seq 1 $ITERATIONS)
        make_request &
    end
end

# Wait for all background jobs to complete
wait

set -l end_time (date +%s)
set -l duration (math $end_time - $start_time)
set -l total_requests (math $CONCURRENCY x $ITERATIONS)
set -l tps (math "$total_requests / $duration")

echo ""
echo ""
echo "Results:"
echo "--------"
echo "Total requests: $total_requests"
echo "Duration: $duration seconds"
echo "Throughput: $tps TPS"
echo ""

# Check metrics
echo "Fetching server metrics..."
curl -s "$BASE_URL/metrics" | grep -E "(mentatd_requests_total|mentatd_cache_hit_rate|mentatd_connection_pool)" | head -10

echo ""
echo "Test complete!"
echo ""
echo "Target metrics:"
echo "- Throughput: 75+ TPS (current: $tps TPS)"
echo "- Connection pool utilization: Should scale linearly"
echo "- Cache hit rate: Should increase over time"