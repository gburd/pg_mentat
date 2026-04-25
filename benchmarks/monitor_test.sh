#!/usr/bin/env bash
#
# Real-time Test Monitor
# Displays live metrics during load test execution
#
set -euo pipefail

RESULTS_DIR="${1:-./results}"
REFRESH_INTERVAL="${2:-5}"  # seconds

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

clear_screen() {
    printf "\033[2J\033[H"
}

format_number() {
    printf "%'.0f" "$1" 2>/dev/null || echo "$1"
}

monitor_loop() {
    local test_dir="$1"
    local start_time=$(date +%s)

    while true; do
        clear_screen

        echo -e "${CYAN}╔══════════════════════════════════════════════════════╗${NC}"
        echo -e "${CYAN}║         pg_mentat Load Test Monitor                  ║${NC}"
        echo -e "${CYAN}╚══════════════════════════════════════════════════════╝${NC}"
        echo

        local current_time=$(date +%s)
        local elapsed=$((current_time - start_time))
        local minutes=$((elapsed / 60))
        local seconds=$((elapsed % 60))

        echo -e "${BLUE}Test Directory:${NC} $test_dir"
        echo -e "${BLUE}Elapsed Time:${NC} ${minutes}m ${seconds}s"
        echo -e "${BLUE}Last Update:${NC} $(date '+%H:%M:%S')"
        echo

        # Find most recent data file
        local latest_dat=$(find "$test_dir" -name "*.dat" -type f 2>/dev/null | head -1)

        if [ -n "$latest_dat" ] && [ -f "$latest_dat" ]; then
            # Count requests
            local total_requests=$(wc -l < "$latest_dat" 2>/dev/null || echo 0)
            local errors=$(awk '$2 != 200 {count++} END {print count+0}' "$latest_dat" 2>/dev/null || echo 0)
            local success=$((total_requests - errors))

            # Calculate throughput
            local tps=0
            if [ $elapsed -gt 0 ]; then
                tps=$(echo "scale=2; $total_requests / $elapsed" | bc 2>/dev/null || echo 0)
            fi

            # Calculate latencies
            local p50=$(awk '{lat[NR]=$3} END {asort(lat); idx=int(NR*0.5); print lat[idx]+0}' "$latest_dat" 2>/dev/null || echo 0)
            local p99=$(awk '{lat[NR]=$3} END {asort(lat); idx=int(NR*0.99); print lat[idx]+0}' "$latest_dat" 2>/dev/null || echo 0)
            local avg=$(awk '{sum+=$3; count++} END {if(count>0) print sum/count; else print 0}' "$latest_dat" 2>/dev/null || echo 0)

            echo -e "${YELLOW}═══ Live Metrics ═══════════════════════════════════════${NC}"
            echo

            # Requests
            echo -e "${GREEN}Requests:${NC}"
            printf "  Total:     %'d\n" $total_requests
            printf "  Success:   %'d\n" $success
            printf "  Errors:    %'d (%.2f%%)\n" $errors $(echo "scale=4; $errors * 100 / ($total_requests + 0.001)" | bc)
            echo

            # Throughput
            echo -e "${GREEN}Throughput:${NC}"
            printf "  Current:   %.2f TPS" $tps

            # Color code based on target
            if (( $(echo "$tps >= 50" | bc -l) )); then
                echo -e " ${GREEN}✅${NC}"
            elif (( $(echo "$tps >= 40" | bc -l) )); then
                echo -e " ${YELLOW}⚠️${NC}"
            else
                echo -e " ${RED}❌${NC}"
            fi
            echo

            # Latency
            echo -e "${GREEN}Latency (ms):${NC}"
            printf "  Average:   %.2f\n" $avg
            printf "  p50:       %.2f" $p50

            # Color code p50
            if (( $(echo "$p50 <= 50" | bc -l) )); then
                echo -e " ${GREEN}✅${NC}"
            elif (( $(echo "$p50 <= 100" | bc -l) )); then
                echo -e " ${YELLOW}⚠️${NC}"
            else
                echo -e " ${RED}❌${NC}"
            fi

            printf "  p99:       %.2f" $p99

            # Color code p99
            if (( $(echo "$p99 <= 100" | bc -l) )); then
                echo -e " ${GREEN}✅${NC}"
            elif (( $(echo "$p99 <= 200" | bc -l) )); then
                echo -e " ${YELLOW}⚠️${NC}"
            else
                echo -e " ${RED}❌${NC}"
            fi
            echo

            # Target comparison
            echo -e "${YELLOW}═══ Target Comparison ══════════════════════════════════${NC}"
            echo
            echo -e "  Throughput: $(printf '%6.1f' $tps) / 50 TPS"
            echo -e "  p50 Latency: $(printf '%6.1f' $p50) / 50 ms"
            echo -e "  p99 Latency: $(printf '%6.1f' $p99) / 100 ms"

        else
            echo -e "${YELLOW}Waiting for test data...${NC}"
        fi

        echo
        echo -e "${CYAN}Press Ctrl+C to stop monitoring${NC}"

        sleep $REFRESH_INTERVAL
    done
}

# Main
if [ $# -lt 1 ]; then
    echo "Usage: $0 <test_results_dir> [refresh_interval_seconds]"
    echo "Example: $0 ./results/phase0_steady 5"
    exit 1
fi

if [ ! -d "$RESULTS_DIR" ]; then
    echo "Error: Directory not found: $RESULTS_DIR"
    exit 1
fi

echo -e "${CYAN}Starting load test monitor...${NC}"
echo -e "${CYAN}Monitoring: $RESULTS_DIR${NC}"
echo -e "${CYAN}Refresh: Every ${REFRESH_INTERVAL}s${NC}"
echo
sleep 2

trap "echo -e '\n${CYAN}Monitor stopped.${NC}'; exit 0" INT TERM

monitor_loop "$RESULTS_DIR"