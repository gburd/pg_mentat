#!/usr/bin/env python3
"""
Performance Regression Checker
Compares test results against baseline and targets
"""

import json
import sys
import re
from pathlib import Path
from typing import Dict, Any, Tuple, List, Optional


# Performance targets
TARGETS = {
    "throughput": {"min": 50},  # TPS
    "p50": {"max": 50},         # ms
    "p99": {"max": 100},        # ms
    "error_rate": {"max": 0.001}
}

# Baseline performance (from earlier tests)
BASELINE = {
    "throughput": 29.51,
    "p50": 185.52,
    "p99": 2229.87,
    "error_rate": 0.0
}


def parse_summary_file(summary_path: str) -> Optional[Dict]:
    """Parse test summary file for key metrics."""
    metrics = {}

    try:
        with open(summary_path) as f:
            content = f.read()

            # Parse throughput (looking for "X.XX TPS")
            tps_pattern = r'Throughput:\s+([\d.]+)\s+TPS'
            match = re.search(tps_pattern, content)
            if match:
                metrics["throughput"] = float(match.group(1))

            # Parse p99 latency
            p99_pattern = r'p99:\s+([\d.]+)'
            match = re.search(p99_pattern, content)
            if match:
                metrics["p99"] = float(match.group(1))

            # Parse p50 latency
            p50_pattern = r'p50:\s+([\d.]+)'
            match = re.search(p50_pattern, content)
            if match:
                metrics["p50"] = float(match.group(1))

            # Parse error rate
            error_pattern = r'Error rate:\s+([\d.]+)'
            match = re.search(error_pattern, content, re.IGNORECASE)
            if match:
                metrics["error_rate"] = float(match.group(1))
    except Exception as e:
        print(f"Error parsing file: {e}")
        return None

    return metrics if metrics else None


def check_targets(metrics: Dict) -> Tuple[bool, List[str]]:
    """Check if metrics meet targets."""
    passes = True
    messages = []

    # Check throughput
    if "throughput" in metrics:
        if metrics["throughput"] < TARGETS["throughput"]["min"]:
            passes = False
            messages.append(f"❌ Throughput {metrics['throughput']:.1f} TPS < {TARGETS['throughput']['min']} TPS target")
        else:
            messages.append(f"✅ Throughput {metrics['throughput']:.1f} TPS ≥ {TARGETS['throughput']['min']} TPS target")

    # Check p50
    if "p50" in metrics:
        if metrics["p50"] > TARGETS["p50"]["max"]:
            passes = False
            messages.append(f"❌ p50 {metrics['p50']:.1f}ms > {TARGETS['p50']['max']}ms target")
        else:
            messages.append(f"✅ p50 {metrics['p50']:.1f}ms ≤ {TARGETS['p50']['max']}ms target")

    # Check p99
    if "p99" in metrics:
        if metrics["p99"] > TARGETS["p99"]["max"]:
            passes = False
            messages.append(f"❌ p99 {metrics['p99']:.1f}ms > {TARGETS['p99']['max']}ms target")
        else:
            messages.append(f"✅ p99 {metrics['p99']:.1f}ms ≤ {TARGETS['p99']['max']}ms target")

    # Check error rate
    if "error_rate" in metrics:
        if metrics["error_rate"] > TARGETS["error_rate"]["max"]:
            passes = False
            messages.append(f"❌ Error rate {metrics['error_rate']:.4f} > {TARGETS['error_rate']['max']} target")
        else:
            messages.append(f"✅ Error rate {metrics['error_rate']:.4f} ≤ {TARGETS['error_rate']['max']} target")

    return passes, messages


def compare_to_baseline(metrics: Dict) -> List[str]:
    """Compare metrics to baseline."""
    messages = []

    if "throughput" in metrics and "throughput" in BASELINE:
        if BASELINE["throughput"] > 0:
            improvement = ((metrics["throughput"] - BASELINE["throughput"]) / BASELINE["throughput"]) * 100
            if improvement >= 0:
                messages.append(f"📈 Throughput improved {improvement:.1f}% vs baseline")
            else:
                messages.append(f"📉 Throughput regressed {-improvement:.1f}% vs baseline")

    if "p50" in metrics and "p50" in BASELINE:
        if BASELINE["p50"] > 0:
            reduction = ((BASELINE["p50"] - metrics["p50"]) / BASELINE["p50"]) * 100
            if reduction > 0:
                messages.append(f"📈 p50 latency improved {reduction:.1f}% vs baseline")
            else:
                messages.append(f"📉 p50 latency worsened by {-reduction:.1f}% vs baseline")

    if "p99" in metrics and "p99" in BASELINE:
        if BASELINE["p99"] > 0:
            reduction = ((BASELINE["p99"] - metrics["p99"]) / BASELINE["p99"]) * 100
            if reduction > 0:
                messages.append(f"📈 p99 latency improved {reduction:.1f}% vs baseline")
            else:
                messages.append(f"📉 p99 latency worsened by {-reduction:.1f}% vs baseline")

    return messages


def main():
    if len(sys.argv) < 2:
        print("Usage: check_performance.py <summary_file>")
        return 1

    summary_path = Path(sys.argv[1])

    if not summary_path.exists():
        print(f"Error: File not found: {summary_path}")
        return 1

    metrics = parse_summary_file(str(summary_path))
    if not metrics:
        print("Error: Could not parse metrics from file")
        return 1

    print("\n" + "="*60)
    print("Performance Check Results")
    print("="*60)

    # Check against targets
    passes, target_messages = check_targets(metrics)

    print("\n📊 Target Validation:")
    for msg in target_messages:
        print(f"  {msg}")

    # Compare to baseline
    baseline_messages = compare_to_baseline(metrics)

    if baseline_messages:
        print("\n📈 Baseline Comparison:")
        for msg in baseline_messages:
            print(f"  {msg}")

    # Overall result
    print("\n" + "="*60)
    if passes:
        print("✅ PASS: All performance targets met!")
    else:
        print("❌ FAIL: Some performance targets not met")
    print("="*60 + "\n")

    return 0 if passes else 1


if __name__ == "__main__":
    sys.exit(main())