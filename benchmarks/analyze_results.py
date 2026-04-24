#!/usr/bin/env python3
"""
mentatd Load Test Results Analyzer

Parses raw benchmark data files and JSON reports to produce
human-readable summaries and optional plots.

Usage:
    python3 benchmarks/analyze_results.py <results_dir>
    python3 benchmarks/analyze_results.py <results_dir> --compare <other_dir>
    python3 benchmarks/analyze_results.py <results_dir> --plot

Data format (per line in .dat files):
    worker_id  http_status  latency_ms  response_size_bytes
"""

import argparse
import json
import os
import sys
from collections import defaultdict
from pathlib import Path


# Performance targets
TARGETS = {
    "tps": 50,
    "p99_ms": 100,
    "p50_ms": 50,
    "error_rate": 0.001,
}


def parse_dat_file(filepath: str) -> list[dict]:
    """Parse a raw .dat file into a list of request records."""
    records = []
    with open(filepath) as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            parts = line.split()
            if len(parts) < 3:
                continue
            record = {
                "worker": parts[0],
                "status": int(parts[1]),
                "latency_ms": float(parts[2]),
                "size": int(parts[3]) if len(parts) > 3 else 0,
            }
            records.append(record)
    return records


def compute_percentile(sorted_values: list[float], pct: float) -> float:
    """Compute the given percentile from a sorted list."""
    if not sorted_values:
        return 0.0
    idx = int(len(sorted_values) * pct / 100.0)
    idx = min(idx, len(sorted_values) - 1)
    return sorted_values[idx]


def analyze_records(records: list[dict], duration_secs: int | None = None) -> dict:
    """Compute statistics from a list of request records."""
    if not records:
        return {"total": 0, "error": "No data"}

    total = len(records)
    errors = sum(1 for r in records if r["status"] != 200)
    latencies = sorted(r["latency_ms"] for r in records)
    sizes = [r["size"] for r in records]

    stats = {
        "total_requests": total,
        "errors": errors,
        "error_rate": errors / total if total > 0 else 0,
        "latency_ms": {
            "min": latencies[0],
            "max": latencies[-1],
            "avg": sum(latencies) / len(latencies),
            "p50": compute_percentile(latencies, 50),
            "p90": compute_percentile(latencies, 90),
            "p95": compute_percentile(latencies, 95),
            "p99": compute_percentile(latencies, 99),
            "p999": compute_percentile(latencies, 99.9),
        },
        "response_size": {
            "min": min(sizes) if sizes else 0,
            "max": max(sizes) if sizes else 0,
            "avg": sum(sizes) / len(sizes) if sizes else 0,
        },
    }

    if duration_secs and duration_secs > 0:
        stats["tps"] = total / duration_secs

    return stats


def check_targets(stats: dict) -> list[tuple[str, bool, str]]:
    """Check stats against performance targets. Returns list of (name, passed, detail)."""
    checks = []

    tps = stats.get("tps", 0)
    if tps:
        passed = tps >= TARGETS["tps"]
        checks.append(("Throughput", passed, f"{tps:.1f} TPS (target: >= {TARGETS['tps']})"))

    lat = stats.get("latency_ms", {})
    p99 = lat.get("p99", 0)
    if p99:
        passed = p99 < TARGETS["p99_ms"]
        checks.append(("p99 Latency", passed, f"{p99:.1f}ms (target: < {TARGETS['p99_ms']}ms)"))

    p50 = lat.get("p50", 0)
    if p50:
        passed = p50 < TARGETS["p50_ms"]
        checks.append(("p50 Latency", passed, f"{p50:.1f}ms (target: < {TARGETS['p50_ms']}ms)"))

    err_rate = stats.get("error_rate", 0)
    passed = err_rate < TARGETS["error_rate"]
    checks.append(("Error Rate", passed, f"{err_rate:.4f} (target: < {TARGETS['error_rate']})"))

    return checks


def format_report(scenario: str, stats: dict) -> str:
    """Format a human-readable report for a scenario."""
    lines = []
    lines.append(f"{'=' * 55}")
    lines.append(f"  Scenario: {scenario}")
    lines.append(f"{'=' * 55}")
    lines.append("")

    lines.append("Requests:")
    lines.append(f"  Total:      {stats['total_requests']}")
    lines.append(f"  Errors:     {stats['errors']}")
    lines.append(f"  Error rate: {stats['error_rate']:.4f}")
    if "tps" in stats:
        lines.append(f"  Throughput: {stats['tps']:.1f} TPS")
    lines.append("")

    lat = stats.get("latency_ms", {})
    lines.append("Latency (ms):")
    for key in ["min", "avg", "p50", "p90", "p95", "p99", "p999", "max"]:
        if key in lat:
            lines.append(f"  {key:>5}: {lat[key]:>10.2f}")
    lines.append("")

    sz = stats.get("response_size", {})
    if sz:
        lines.append("Response Size (bytes):")
        lines.append(f"  Min: {sz['min']}")
        lines.append(f"  Avg: {sz['avg']:.0f}")
        lines.append(f"  Max: {sz['max']}")
        lines.append("")

    checks = check_targets(stats)
    lines.append("Target Validation:")
    for name, passed, detail in checks:
        marker = "PASS" if passed else "FAIL"
        lines.append(f"  [{marker}] {name}: {detail}")
    lines.append("")

    all_pass = all(p for _, p, _ in checks)
    lines.append(f"Overall: {'PASS' if all_pass else 'FAIL'}")
    lines.append("")

    return "\n".join(lines)


def parse_k6_summary(filepath: str, duration: int | None = None) -> dict | None:
    """Parse a k6 --summary-export JSON file into our standard stats format."""
    try:
        with open(filepath) as f:
            data = json.load(f)
    except (json.JSONDecodeError, OSError):
        return None

    metrics = data.get("metrics", {})
    dur = metrics.get("http_req_duration", {})
    reqs = metrics.get("http_reqs", {})
    fails = metrics.get("http_req_failed", {})

    if not dur or not reqs:
        return None

    total = reqs.get("count", 0)
    rate = reqs.get("rate", 0)
    err_rate = fails.get("rate", 0) if fails else 0

    stats = {
        "total_requests": total,
        "errors": int(total * err_rate),
        "error_rate": err_rate,
        "latency_ms": {
            "min": dur.get("min", 0),
            "max": dur.get("max", 0),
            "avg": dur.get("avg", 0),
            "p50": dur.get("med", 0),
            "p90": dur.get("p(90)", 0),
            "p95": dur.get("p(95)", 0),
            "p99": dur.get("p(99)", 0),
        },
        "tps": rate,
        "source": "k6",
    }

    return stats


def analyze_directory(results_dir: str, duration: int | None = None) -> dict:
    """Analyze all .dat files in a results directory."""
    results_path = Path(results_dir)
    if not results_path.exists():
        print(f"Error: directory not found: {results_dir}", file=sys.stderr)
        sys.exit(1)

    scenarios = {}
    for dat_file in sorted(results_path.glob("*_raw.dat")):
        scenario_name = dat_file.stem.replace("_raw", "")
        records = parse_dat_file(str(dat_file))
        if records:
            stats = analyze_records(records, duration)
            scenarios[scenario_name] = stats

    # Also load any JSON reports (from curl-based tests)
    for json_file in sorted(results_path.glob("*_report.json")):
        scenario_name = json_file.stem.replace("_report", "")
        if scenario_name not in scenarios:
            with open(json_file) as f:
                data = json.load(f)
                scenarios[scenario_name] = data

    # Also load k6 summary JSON files
    for k6_file in sorted(results_path.glob("*_k6_summary.json")):
        scenario_name = k6_file.stem.replace("_k6_summary", "")
        if scenario_name not in scenarios:
            stats = parse_k6_summary(str(k6_file), duration)
            if stats:
                scenarios[scenario_name] = stats

    return scenarios


def compare_runs(dir_a: str, dir_b: str, duration: int | None = None):
    """Compare two benchmark runs side by side."""
    results_a = analyze_directory(dir_a, duration)
    results_b = analyze_directory(dir_b, duration)

    all_scenarios = sorted(set(list(results_a.keys()) + list(results_b.keys())))

    print(f"{'=' * 70}")
    print(f"  Comparison: {os.path.basename(dir_a)} vs {os.path.basename(dir_b)}")
    print(f"{'=' * 70}")
    print()

    for scenario in all_scenarios:
        a = results_a.get(scenario, {})
        b = results_b.get(scenario, {})

        print(f"--- {scenario} ---")

        def cmp_val(label, va, vb, fmt=".1f", lower_better=True):
            if va and vb:
                diff = vb - va
                pct = (diff / va * 100) if va != 0 else 0
                better = diff < 0 if lower_better else diff > 0
                arrow = "v" if better else "^" if not better and diff != 0 else "="
                print(f"  {label:>20}: {va:{fmt}} -> {vb:{fmt}} ({diff:+{fmt}}, {pct:+.1f}% {arrow})")
            elif va:
                print(f"  {label:>20}: {va:{fmt}} -> N/A")
            elif vb:
                print(f"  {label:>20}: N/A -> {vb:{fmt}}")

        a_lat = a.get("latency_ms", {})
        b_lat = b.get("latency_ms", {})

        cmp_val("TPS", a.get("tps"), b.get("tps"), lower_better=False)
        cmp_val("p50 (ms)", a_lat.get("p50"), b_lat.get("p50"))
        cmp_val("p99 (ms)", a_lat.get("p99"), b_lat.get("p99"))
        cmp_val("Error rate", a.get("error_rate"), b.get("error_rate"), fmt=".4f")
        print()


def try_plot(results_dir: str, duration: int | None = None):
    """Generate latency distribution plots if matplotlib is available."""
    try:
        import matplotlib
        matplotlib.use("Agg")
        import matplotlib.pyplot as plt
    except ImportError:
        print("matplotlib not installed; skipping plots.", file=sys.stderr)
        print("Install with: pip install matplotlib", file=sys.stderr)
        return

    results_path = Path(results_dir)
    fig, axes = plt.subplots(2, 2, figsize=(14, 10))
    fig.suptitle("mentatd Load Test Results", fontsize=14)

    dat_files = sorted(results_path.glob("*_raw.dat"))
    if not dat_files:
        print("No .dat files found for plotting.", file=sys.stderr)
        return

    for idx, dat_file in enumerate(dat_files[:4]):
        ax = axes[idx // 2][idx % 2]
        scenario_name = dat_file.stem.replace("_raw", "")

        records = parse_dat_file(str(dat_file))
        if not records:
            ax.set_title(f"{scenario_name}: No data")
            continue

        latencies = [r["latency_ms"] for r in records]

        ax.hist(latencies, bins=50, color="steelblue", alpha=0.7, edgecolor="black", linewidth=0.5)
        ax.axvline(TARGETS["p99_ms"], color="red", linestyle="--", label=f"p99 target ({TARGETS['p99_ms']}ms)")
        ax.axvline(TARGETS["p50_ms"], color="orange", linestyle="--", label=f"p50 target ({TARGETS['p50_ms']}ms)")
        ax.set_title(scenario_name)
        ax.set_xlabel("Latency (ms)")
        ax.set_ylabel("Count")
        ax.legend(fontsize=8)

    # Hide unused subplots
    for idx in range(len(dat_files), 4):
        axes[idx // 2][idx % 2].set_visible(False)

    plt.tight_layout()
    plot_path = results_path / "latency_distribution.png"
    plt.savefig(str(plot_path), dpi=150)
    print(f"Plot saved to: {plot_path}")


def main():
    parser = argparse.ArgumentParser(description="Analyze mentatd load test results")
    parser.add_argument("results_dir", help="Path to results directory")
    parser.add_argument("--duration", type=int, default=None,
                        help="Test duration in seconds (for TPS calculation)")
    parser.add_argument("--compare", metavar="OTHER_DIR",
                        help="Compare with another results directory")
    parser.add_argument("--plot", action="store_true",
                        help="Generate latency distribution plots")
    parser.add_argument("--json", action="store_true",
                        help="Output results as JSON")

    args = parser.parse_args()

    if args.compare:
        compare_runs(args.results_dir, args.compare, args.duration)
        return

    scenarios = analyze_directory(args.results_dir, args.duration)

    if not scenarios:
        print("No benchmark data found.", file=sys.stderr)
        sys.exit(1)

    if args.json:
        # Convert for JSON serialization
        print(json.dumps(scenarios, indent=2, default=str))
        return

    all_pass = True
    for scenario_name, stats in scenarios.items():
        if "error" in stats:
            print(f"Scenario {scenario_name}: {stats['error']}")
            continue
        report = format_report(scenario_name, stats)
        print(report)
        checks = check_targets(stats)
        if not all(p for _, p, _ in checks):
            all_pass = False

    print("=" * 55)
    if all_pass:
        print("  ALL SCENARIOS PASSED")
    else:
        print("  SOME SCENARIOS FAILED")
    print("=" * 55)

    if args.plot:
        try_plot(args.results_dir, args.duration)

    sys.exit(0 if all_pass else 1)


if __name__ == "__main__":
    main()
