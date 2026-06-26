#!/usr/bin/env python3
"""Create a deterministic release artifact manifest with SHA-256 checksums.

This is a packaging helper for release automation. It does not build artifacts;
it records the artifacts a release job already produced and, optionally, embeds
the summary and compact gate section from a validation report such as
`scripts/render_validate.py --json`. Extraction benchmark reports from
`scripts/bench_vs_mature.py --json` can be attached the same way; only their
summaries and gate metadata are embedded. Public hygiene audit reports from
`scripts/public_hygiene_audit.py --json` and public corpus TSV manifests can also
be summarized without copying row data. When a release policy is named, the
manifest also records whether strict local policy evidence was enforced.

Example:

  python scripts/release_manifest.py \
    --version 0.1.0 \
    --git-rev "$(git rev-parse HEAD)" \
    --release-policy public-release \
    --enforce-policy-inputs \
    --corpus-manifest corpus/public/MANIFEST.tsv \
    --corpus-manifest corpus/public/RENDER_MANIFEST.tsv \
    --hygiene-report public-hygiene.json \
    --validation-report render-report.json \
    --benchmark-report extract-benchmark.json \
    --output dist/rdoc-release-manifest.json \
    dist/rdoc-aarch64-apple-darwin.tar.gz dist/rdoc.wasm
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import sys
from pathlib import Path
from typing import Any


SCHEMA = "rdoc.release-manifest.v1"
PUBLIC_RELEASE_CORPUS_MANIFESTS = ("MANIFEST.tsv", "RENDER_MANIFEST.tsv")
RELEASE_POLICIES: dict[str, dict[str, Any]] = {
    "public-release": {
        "name": "public-release",
        "required_gates": {
            "default": [
                "python3 scripts/public_hygiene_audit.py",
                "cargo fmt --all -- --check",
                "cargo clippy --all-targets -- -D warnings",
                "cargo test --all-targets",
                "cargo test --doc",
            ],
            "render": [
                "cargo test --all-targets --features render",
            ],
        },
        "optional_local_gates": {
            "extraction_benchmark": {
                "min_poi_recall_mean": 0.95,
                "min_poi_f1_mean": 0.95,
                "max_errors": 0,
            },
            "render_validation": {
                "recall_min": 0.97,
                "min_mean_recall": 0.90,
                "max_skipped": 0,
            },
            "public_corpus": {
                "manifest_match": "exact",
            },
        },
    }
}


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as file:
        for chunk in iter(lambda: file.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def artifact_record(path: Path) -> dict[str, Any]:
    stat = path.stat()
    return {
        "name": path.name,
        "path": path.as_posix(),
        "bytes": stat.st_size,
        "sha256": sha256_file(path),
    }


def report_summary(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    summary = data.get("summary")
    if not isinstance(summary, dict):
        raise ValueError(f"{path} does not contain a JSON object field named 'summary'")
    report = {"path": path.as_posix(), "summary": summary}
    gate = data.get("gate")
    if isinstance(gate, dict):
        report["gate"] = gate
    return report


def validation_summary(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    return report_summary(path)


def hygiene_summary(path: Path | None) -> dict[str, Any] | None:
    if path is None:
        return None
    data = json.loads(path.read_text(encoding="utf-8"))
    passed = data.get("passed")
    findings = data.get("findings")
    if not isinstance(passed, bool):
        raise ValueError(f"{path} does not contain a boolean field named 'passed'")
    if not isinstance(findings, list):
        raise ValueError(f"{path} does not contain a list field named 'findings'")
    return {
        "path": path.as_posix(),
        "gate": {
            "passed": passed,
            "findings": len(findings),
        },
    }


def benchmark_summaries(paths: list[Path] | None) -> list[dict[str, Any]]:
    return [report_summary(path) for path in sorted(paths or [], key=lambda p: p.name)]


def release_policy_summary(name: str | None) -> dict[str, Any] | None:
    if name is None:
        return None
    try:
        return copy.deepcopy(RELEASE_POLICIES[name])
    except KeyError as error:
        raise ValueError(f"unknown release policy: {name}") from error


def check_required_policy_inputs(
    name: str | None,
    *,
    hygiene_report: Path | None,
    validation_report: Path | None,
    benchmark_reports: list[Path] | None,
    corpus_manifests: list[Path] | None,
) -> None:
    if name is None:
        return
    if name not in RELEASE_POLICIES:
        raise ValueError(f"unknown release policy: {name}")
    if name != "public-release":
        return

    missing = []
    if hygiene_report is None:
        missing.append("hygiene report")
    if validation_report is None:
        missing.append("validation report")
    if not benchmark_reports:
        missing.append("benchmark report")
    if not corpus_manifests:
        missing.append("corpus manifest")
    if missing:
        raise ValueError(f"{name} requires {', '.join(missing)}")
    require_public_release_corpus_manifest_pair(name, corpus_manifests or [])


def public_release_policy_input_gaps(
    *,
    hygiene_report: Path | None,
    validation_report: Path | None,
    benchmark_reports: list[Path] | None,
    corpus_manifests: list[Path] | None,
) -> list[str]:
    missing = []
    if hygiene_report is None:
        missing.append("hygiene report")
    if validation_report is None:
        missing.append("validation report")
    if not benchmark_reports:
        missing.append("benchmark report")
    if not corpus_manifests:
        missing.append("corpus manifest")
    elif not public_release_corpus_manifest_pair_matches(corpus_manifests):
        missing.append("exact public corpus manifest pair")
    return missing


def public_release_corpus_manifest_pair_matches(corpus_manifests: list[Path]) -> bool:
    provided = [path.name for path in corpus_manifests]
    return len(provided) == len(PUBLIC_RELEASE_CORPUS_MANIFESTS) and set(provided) == set(
        PUBLIC_RELEASE_CORPUS_MANIFESTS
    )


def require_public_release_corpus_manifest_pair(name: str, corpus_manifests: list[Path]) -> None:
    required = " and ".join(PUBLIC_RELEASE_CORPUS_MANIFESTS)
    if not public_release_corpus_manifest_pair_matches(corpus_manifests):
        raise ValueError(f"{name} requires corpus manifests exactly {required}")


def require_report_gate_passed(policy: str, report: dict[str, Any], label: str) -> None:
    gate = report.get("gate")
    if not isinstance(gate, dict):
        raise ValueError(f"{policy} {label} report does not contain a gate result")
    if gate.get("passed") is not True:
        raise ValueError(f"{policy} {label} report gate did not pass")


def require_public_release_report_thresholds(
    policy: str,
    report: dict[str, Any],
    label: str,
) -> None:
    if policy != "public-release":
        return
    if label == "validation":
        thresholds = RELEASE_POLICIES[policy]["optional_local_gates"]["render_validation"]
        require_summary_threshold_at_least(
            policy,
            report,
            label,
            "recall_min",
            thresholds["recall_min"],
        )
        require_gate_check_threshold(
            policy,
            report,
            label,
            "below_recall_min",
            "<=",
            0,
        )
        require_gate_check_threshold(
            policy,
            report,
            label,
            "mean_recall",
            ">=",
            thresholds["min_mean_recall"],
        )
        require_gate_check_threshold(
            policy,
            report,
            label,
            "skipped",
            "<=",
            thresholds["max_skipped"],
        )
    elif label == "benchmark":
        thresholds = RELEASE_POLICIES[policy]["optional_local_gates"]["extraction_benchmark"]
        require_gate_check_threshold(
            policy,
            report,
            label,
            "poi_recall_mean",
            ">=",
            thresholds["min_poi_recall_mean"],
        )
        require_gate_check_threshold(
            policy,
            report,
            label,
            "poi_f1_mean",
            ">=",
            thresholds["min_poi_f1_mean"],
        )
        require_gate_check_threshold(
            policy,
            report,
            label,
            "errors",
            "<=",
            thresholds["max_errors"],
        )


def require_summary_threshold_at_least(
    policy: str,
    report: dict[str, Any],
    label: str,
    metric: str,
    minimum: float | int,
) -> None:
    summary = report.get("summary")
    if not isinstance(summary, dict):
        raise ValueError(f"{policy} {label} report does not contain a summary")
    actual = summary.get(metric)
    if not isinstance(actual, (int, float)) or actual < minimum:
        raise ValueError(
            f"{policy} {label} report summary {metric} must be at least {minimum}"
        )


def require_gate_check_threshold(
    policy: str,
    report: dict[str, Any],
    label: str,
    metric: str,
    op: str,
    policy_threshold: float | int,
) -> None:
    gate = report.get("gate")
    checks = gate.get("checks") if isinstance(gate, dict) else None
    if not isinstance(checks, list):
        raise ValueError(f"{policy} {label} report does not contain gate checks")
    for check in checks:
        if not isinstance(check, dict):
            continue
        if check.get("metric") != metric or check.get("op") != op:
            continue
        threshold = check.get("threshold")
        if not isinstance(threshold, (int, float)):
            continue
        if (op == ">=" and threshold >= policy_threshold) or (
            op == "<=" and threshold <= policy_threshold
        ):
            if check.get("passed") is not True:
                raise ValueError(
                    f"{policy} {label} report gate check did not pass: "
                    f"{metric} {op} {policy_threshold}"
                )
            return
    raise ValueError(
        f"{policy} {label} report gate must include {metric} {op} {policy_threshold}"
    )


def parse_manifest_header(line: str) -> list[str]:
    trimmed = line.strip()
    if trimmed.startswith("#"):
        trimmed = trimmed[1:].strip()
    return trimmed.split("\t")


def corpus_manifest_summary(path: Path) -> dict[str, Any]:
    header: list[str] | None = None
    rows: list[list[str]] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        trimmed = line.strip()
        if not trimmed:
            continue
        if header is None:
            header = parse_manifest_header(trimmed)
            if not header or header[0] != "path":
                raise ValueError(f"{path} does not start with a TSV path header")
            continue
        if trimmed.startswith("#"):
            continue
        if trimmed.startswith("path\t"):
            continue
        cols = trimmed.split("\t")
        if len(cols) != len(header):
            raise ValueError(
                f"{path} row has {len(cols)} columns, expected {len(header)}: {line}"
            )
        rows.append(cols)

    if header is None:
        raise ValueError(f"{path} is empty")

    numeric_totals: dict[str, int] = {}
    warning_counts: dict[str, int] = {}
    for index, name in enumerate(header):
        if name in {"path", "warnings"}:
            continue
        total = 0
        numeric = True
        for row in rows:
            try:
                total += int(row[index])
            except ValueError:
                numeric = False
                break
        if numeric:
            numeric_totals[name] = total

    if "warnings" in header:
        warning_index = header.index("warnings")
        for row in rows:
            warnings = row[warning_index]
            if warnings == "-":
                continue
            for warning in warnings.split("|"):
                warning = warning.strip()
                if warning:
                    warning_counts[warning] = warning_counts.get(warning, 0) + 1

    return {
        "documents": len(rows),
        "numeric_totals": numeric_totals,
        "warning_counts": dict(sorted(warning_counts.items())),
    }


def corpus_manifest_summaries(paths: list[Path] | None) -> list[dict[str, Any]]:
    return [
        {"path": path.as_posix(), "summary": corpus_manifest_summary(path)}
        for path in sorted(paths or [], key=lambda p: p.name)
    ]


def release_evidence_summary(
    name: str | None,
    *,
    enforce_policy_inputs: bool,
    hygiene_report: Path | None,
    validation_report: Path | None,
    benchmark_reports: list[Path] | None,
    corpus_manifests: list[Path] | None,
) -> dict[str, Any] | None:
    if name is None:
        return None
    if name not in RELEASE_POLICIES:
        raise ValueError(f"unknown release policy: {name}")

    strict_missing: list[str] = []
    if name == "public-release":
        strict_missing = public_release_policy_input_gaps(
            hygiene_report=hygiene_report,
            validation_report=validation_report,
            benchmark_reports=benchmark_reports,
            corpus_manifests=corpus_manifests,
        )

    return {
        "policy": name,
        "strict_policy_enforced": enforce_policy_inputs,
        "strict_policy_inputs_complete": not strict_missing,
        "strict_missing": strict_missing,
        "provided": {
            "hygiene_report": hygiene_report.as_posix() if hygiene_report else None,
            "validation_report": validation_report.as_posix() if validation_report else None,
            "benchmark_reports": [
                path.as_posix() for path in sorted(benchmark_reports or [], key=lambda p: p.name)
            ],
            "corpus_manifests": [
                path.as_posix() for path in sorted(corpus_manifests or [], key=lambda p: p.name)
            ],
        },
    }


def release_manifest(
    artifacts: list[Path],
    *,
    hygiene_report: Path | None = None,
    validation_report: Path | None = None,
    benchmark_reports: list[Path] | None = None,
    corpus_manifests: list[Path] | None = None,
    release_policy: str | None = None,
    enforce_policy_inputs: bool = False,
    version: str | None = None,
    git_rev: str | None = None,
) -> dict[str, Any]:
    if not artifacts:
        raise ValueError("at least one artifact path is required")
    if enforce_policy_inputs:
        check_required_policy_inputs(
            release_policy,
            hygiene_report=hygiene_report,
            validation_report=validation_report,
            benchmark_reports=benchmark_reports,
            corpus_manifests=corpus_manifests,
        )
    resolved = [path if isinstance(path, Path) else Path(path) for path in artifacts]
    missing = [path.as_posix() for path in resolved if not path.is_file()]
    if missing:
        raise FileNotFoundError("missing artifact(s): " + ", ".join(missing))

    manifest: dict[str, Any] = {
        "schema": SCHEMA,
        "artifacts": [artifact_record(path) for path in sorted(resolved, key=lambda p: p.name)],
    }
    if version is not None:
        manifest["version"] = version
    if git_rev is not None:
        manifest["git_rev"] = git_rev
    policy = release_policy_summary(release_policy)
    if policy is not None:
        manifest["release_policy"] = policy
        manifest["release_evidence"] = release_evidence_summary(
            release_policy,
            enforce_policy_inputs=enforce_policy_inputs,
            hygiene_report=hygiene_report,
            validation_report=validation_report,
            benchmark_reports=benchmark_reports,
            corpus_manifests=corpus_manifests,
        )
    hygiene = hygiene_summary(hygiene_report)
    if hygiene is not None:
        if enforce_policy_inputs and release_policy is not None:
            require_report_gate_passed(release_policy, hygiene, "hygiene")
        manifest["hygiene"] = hygiene
    validation = validation_summary(validation_report)
    if validation is not None:
        if enforce_policy_inputs and release_policy is not None:
            require_report_gate_passed(release_policy, validation, "validation")
            require_public_release_report_thresholds(
                release_policy, validation, "validation"
            )
        manifest["validation"] = validation
    benchmarks = benchmark_summaries(benchmark_reports)
    if benchmarks:
        if enforce_policy_inputs and release_policy is not None:
            for benchmark in benchmarks:
                require_report_gate_passed(release_policy, benchmark, "benchmark")
                require_public_release_report_thresholds(
                    release_policy, benchmark, "benchmark"
                )
        manifest["benchmarks"] = benchmarks
    corpus = corpus_manifest_summaries(corpus_manifests)
    if corpus:
        manifest["corpus_manifests"] = corpus
    return manifest


def parse_args(argv: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("artifacts", nargs="+", type=Path, help="release artifact files")
    parser.add_argument("--version", help="release version string")
    parser.add_argument("--git-rev", help="git revision included in the release")
    parser.add_argument(
        "--release-policy",
        choices=sorted(RELEASE_POLICIES),
        help="embed the named release validation policy in the manifest",
    )
    parser.add_argument(
        "--hygiene-report",
        type=Path,
        help=(
            "optional JSON report from scripts/public_hygiene_audit.py --json; "
            "only path and compact gate metadata are embedded"
        ),
    )
    parser.add_argument(
        "--validation-report",
        type=Path,
        help="optional JSON validation report; only its summary is embedded",
    )
    parser.add_argument(
        "--benchmark-report",
        action="append",
        type=Path,
        help="optional JSON benchmark report; may be repeated; only summaries are embedded",
    )
    parser.add_argument(
        "--corpus-manifest",
        action="append",
        type=Path,
        help="optional public corpus TSV manifest; may be repeated; only summaries are embedded",
    )
    parser.add_argument(
        "--enforce-policy-inputs",
        action="store_true",
        help=(
            "when --release-policy is set, require that policy's local evidence "
            "reports/manifests and reject hygiene, validation, or benchmark reports whose gates fail; "
            "public-release requires exactly MANIFEST.tsv and RENDER_MANIFEST.tsv corpus manifests"
        ),
    )
    parser.add_argument(
        "--output",
        type=Path,
        help="write manifest JSON to this path instead of stdout",
    )
    return parser.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(sys.argv[1:] if argv is None else argv)
    try:
        manifest = release_manifest(
            args.artifacts,
            hygiene_report=args.hygiene_report,
            validation_report=args.validation_report,
            benchmark_reports=args.benchmark_report,
            corpus_manifests=args.corpus_manifest,
            release_policy=args.release_policy,
            enforce_policy_inputs=args.enforce_policy_inputs,
            version=args.version,
            git_rev=args.git_rev,
        )
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"release_manifest: {error}", file=sys.stderr)
        return 2

    payload = json.dumps(manifest, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if args.output is None:
        sys.stdout.write(payload)
    else:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(payload, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
