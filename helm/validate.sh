#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Helm Chart Validation Script
# =============================================================================
#
# Validates the pg-mentat Helm chart templates render correctly and
# contain required Kubernetes resources.
#
# Usage:
#   ./helm/validate.sh
#
# Prerequisites:
#   - helm (v3.x)
#   - kubectl (for dry-run validation, optional)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CHART_DIR="$SCRIPT_DIR/pg-mentat"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

pass_count=0
fail_count=0

check() {
    local name="$1"
    shift
    if "$@" > /dev/null 2>&1; then
        echo -e "${GREEN}[PASS]${NC} $name"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}[FAIL]${NC} $name"
        fail_count=$((fail_count + 1))
    fi
}

check_contains() {
    local name="$1"
    local rendered="$2"
    local pattern="$3"
    if echo "$rendered" | grep -q "$pattern"; then
        echo -e "${GREEN}[PASS]${NC} $name"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}[FAIL]${NC} $name (pattern not found: $pattern)"
        fail_count=$((fail_count + 1))
    fi
}

echo "============================================"
echo "Helm Chart Validation: pg-mentat"
echo "============================================"
echo

# --- Prerequisites ---
if ! command -v helm &> /dev/null; then
    echo -e "${RED}helm not found. Install Helm v3.x to run this script.${NC}"
    exit 1
fi

# --- Chart.yaml validation ---
check "Chart.yaml exists" test -f "$CHART_DIR/Chart.yaml"
check "values.yaml exists" test -f "$CHART_DIR/values.yaml"
check "templates directory exists" test -d "$CHART_DIR/templates"

# --- Helm lint ---
echo
echo "--- Helm lint ---"
check "Helm lint passes" helm lint "$CHART_DIR"

# --- Template rendering ---
echo
echo "--- Template rendering (default values) ---"
RENDERED=$(helm template test-release "$CHART_DIR" 2>&1)
check "Helm template renders without errors" test $? -eq 0

# --- Required resources ---
echo
echo "--- Required Kubernetes resources ---"
check_contains "Deployment for mentatd" "$RENDERED" "kind: Deployment"
check_contains "StatefulSet for PostgreSQL" "$RENDERED" "kind: StatefulSet"
check_contains "Service" "$RENDERED" "kind: Service"
check_contains "ConfigMap" "$RENDERED" "kind: ConfigMap"
check_contains "Secret" "$RENDERED" "kind: Secret"
check_contains "ServiceAccount" "$RENDERED" "kind: ServiceAccount"
check_contains "HorizontalPodAutoscaler" "$RENDERED" "kind: HorizontalPodAutoscaler"
check_contains "PodDisruptionBudget" "$RENDERED" "kind: PodDisruptionBudget"

# --- Security ---
echo
echo "--- Security settings ---"
check_contains "runAsNonRoot" "$RENDERED" "runAsNonRoot: true"
check_contains "Container security context" "$RENDERED" "readOnlyRootFilesystem: true"
check_contains "Health check endpoint" "$RENDERED" "/health"

# --- Probes ---
echo
echo "--- Health probes ---"
check_contains "Liveness probe" "$RENDERED" "livenessProbe"
check_contains "Readiness probe" "$RENDERED" "readinessProbe"
check_contains "Startup probe" "$RENDERED" "startupProbe"

# --- Resource limits ---
echo
echo "--- Resource management ---"
check_contains "CPU requests" "$RENDERED" "cpu:"
check_contains "Memory limits" "$RENDERED" "memory:"

# --- Custom values rendering ---
echo
echo "--- Custom values override ---"
CUSTOM_RENDERED=$(helm template test-release "$CHART_DIR" \
    --set mentatd.replicaCount=5 \
    --set autoscaling.maxReplicas=20 \
    --set ingress.enabled=true \
    2>&1)
check_contains "Custom replica count" "$CUSTOM_RENDERED" "replicas: 5"
check_contains "Ingress enabled" "$CUSTOM_RENDERED" "kind: Ingress"

# --- Network policy rendering ---
echo
echo "--- Optional features ---"
NP_RENDERED=$(helm template test-release "$CHART_DIR" \
    --set networkPolicy.enabled=true 2>&1)
check_contains "NetworkPolicy when enabled" "$NP_RENDERED" "kind: NetworkPolicy"

# --- Summary ---
echo
echo "============================================"
echo "Validation Summary"
echo "============================================"
echo -e "Passed: ${GREEN}$pass_count${NC}"
echo -e "Failed: ${RED}$fail_count${NC}"
echo

if [ "$fail_count" -eq 0 ]; then
    echo -e "${GREEN}All validations passed!${NC}"
    exit 0
else
    echo -e "${RED}Some validations failed.${NC}"
    exit 1
fi
