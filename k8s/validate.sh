#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Kubernetes Manifest Validation Script
# =============================================================================
#
# Validates that the raw Kubernetes manifests in k8s/ are syntactically
# correct and contain the expected resources.
#
# Usage:
#   ./k8s/validate.sh
#
# Prerequisites:
#   - kubectl (for --dry-run validation)
#   - OR: just checks file structure if kubectl is not available

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

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

echo "============================================"
echo "Kubernetes Manifest Validation"
echo "============================================"
echo

# --- Required files ---
echo "--- Required manifest files ---"
for f in namespace.yaml configmap.yaml secret.yaml deployment.yaml \
         statefulset.yaml service.yaml ingress.yaml hpa.yaml pdb.yaml \
         networkpolicy.yaml; do
    check "$f exists" test -f "$SCRIPT_DIR/$f"
done

# --- YAML syntax validation ---
echo
echo "--- YAML syntax ---"
if command -v python3 &> /dev/null; then
    for f in "$SCRIPT_DIR"/*.yaml; do
        fname=$(basename "$f")
        check "Valid YAML: $fname" python3 -c "
import yaml, sys
with open('$f') as fh:
    list(yaml.safe_load_all(fh))
"
    done
elif command -v ruby &> /dev/null; then
    for f in "$SCRIPT_DIR"/*.yaml; do
        fname=$(basename "$f")
        check "Valid YAML: $fname" ruby -ryaml -e "YAML.load_stream(File.read('$f'))"
    done
else
    echo -e "${YELLOW}[SKIP]${NC} YAML validation (no python3 or ruby found)"
fi

# --- kubectl dry-run validation ---
echo
echo "--- kubectl dry-run ---"
if command -v kubectl &> /dev/null; then
    for f in "$SCRIPT_DIR"/*.yaml; do
        fname=$(basename "$f")
        check "kubectl dry-run: $fname" kubectl apply --dry-run=client -f "$f"
    done
else
    echo -e "${YELLOW}[SKIP]${NC} kubectl dry-run (kubectl not found)"
fi

# --- Content checks ---
echo
echo "--- Security best practices ---"

deployment_content=$(cat "$SCRIPT_DIR/deployment.yaml")
statefulset_content=$(cat "$SCRIPT_DIR/statefulset.yaml")

check_pattern() {
    local name="$1"
    local content="$2"
    local pattern="$3"
    if echo "$content" | grep -q "$pattern"; then
        echo -e "${GREEN}[PASS]${NC} $name"
        pass_count=$((pass_count + 1))
    else
        echo -e "${RED}[FAIL]${NC} $name"
        fail_count=$((fail_count + 1))
    fi
}

check_pattern "Deployment: runAsNonRoot" "$deployment_content" "runAsNonRoot: true"
check_pattern "Deployment: resource limits" "$deployment_content" "limits:"
check_pattern "Deployment: liveness probe" "$deployment_content" "livenessProbe:"
check_pattern "Deployment: readiness probe" "$deployment_content" "readinessProbe:"
check_pattern "Deployment: startup probe" "$deployment_content" "startupProbe:"
check_pattern "Deployment: RollingUpdate strategy" "$deployment_content" "RollingUpdate"
check_pattern "Deployment: maxUnavailable 0" "$deployment_content" "maxUnavailable: 0"
check_pattern "StatefulSet: PVC template" "$statefulset_content" "volumeClaimTemplates"
check_pattern "StatefulSet: init container" "$statefulset_content" "initContainers"

# --- Namespace consistency ---
echo
echo "--- Namespace consistency ---"
expected_ns="pg-mentat"
for f in "$SCRIPT_DIR"/*.yaml; do
    fname=$(basename "$f")
    if grep -q "namespace:" "$f"; then
        if grep "namespace:" "$f" | grep -q "$expected_ns"; then
            echo -e "${GREEN}[PASS]${NC} $fname uses namespace $expected_ns"
            pass_count=$((pass_count + 1))
        else
            echo -e "${RED}[FAIL]${NC} $fname has wrong namespace"
            fail_count=$((fail_count + 1))
        fi
    fi
done

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
