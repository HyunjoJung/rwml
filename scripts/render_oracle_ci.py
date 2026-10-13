#!/usr/bin/env python3
"""Run a commit-bound, twelve-document diagnostic capture pair for CI.

Success proves complete, repeated evidence, not a passing fidelity threshold.
Metric disagreements remain unchanged in both retained v8 evidence reports.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import sys

try:
    import generate_render_smoke_manifest as smoke
    import prepare_render_oracle_ci as prepare
    import render_campaign_capture as capture
    import render_campaign_metrics as metrics
    import render_oracle_contract as contract
    import render_validate as render
except ModuleNotFoundError:
    from scripts import generate_render_smoke_manifest as smoke
    from scripts import prepare_render_oracle_ci as prepare
    from scripts import render_campaign_capture as capture
    from scripts import render_campaign_metrics as metrics
    from scripts import render_oracle_contract as contract
    from scripts import render_validate as render


def run(output: Path, prepared: Path, source_revision: str) -> dict:
    if output.exists() or output.is_symlink():
        raise ValueError("CI evidence output must be fresh")
    output, prepared = output.resolve(), prepared.resolve()
    if output.is_relative_to(prepared) or prepared.is_relative_to(output):
        raise ValueError("CI evidence overlaps prepared inputs")
    capture.table_capture.source_revision(source_revision)
    if not smoke.check():
        raise ValueError("CI smoke corpus is stale")
    corpus = contract.load_corpus_manifest(smoke.OUTPUT_MANIFEST)
    pack = prepared / "shared-font-pack"
    fonttools = prepared / capture.attestation.tool_lock()["wheel"]["name"]
    pypdf = prepared / capture.resources.tool_lock()["wheel"]["name"]
    notices = prepare.notice_payloads(prepared)
    output.mkdir(parents=True)
    (output / "licenses").mkdir()
    for name, payload in notices.items():
        capture.write_new(output / "licenses" / name, payload)
    captures = [output / f"capture-{label}" for label in ("a", "b")]
    evidence = [output / f"evidence-{label}.json" for label in ("a", "b")]
    settings = render.validate_visual_settings({"font_mode": "locked-shared-fonts"})
    for root, report_path in zip(captures, evidence):
        capture.run(corpus.path, root, pack, fonttools, pypdf)
        report = metrics.run(
            corpus.path,
            root,
            pack,
            fonttools,
            pypdf,
            settings=settings,
            recall_min=0.97,
        )
        contract.validate_evidence_report(report, corpus)
        capture.require_equal(
            contract.CAPTURE_EVIDENCE_SCHEMA,
            report["schema"],
            "CI capture evidence schema",
        )
        capture.require_equal(
            [smoke.EXPECTED_DOCUMENTS, smoke.EXPECTED_DOCUMENTS, 0],
            [report["summary"][key] for key in ("documents", "measured", "skipped")],
            "CI complete measured coverage",
        )
        capture.write_new(report_path, render.json_report_payload(report).encode())
        print(
            f"{report_path.name}: fidelity gate passed={report['gate']['passed']}",
            flush=True,
        )
    result = metrics.verify_repeat(
        corpus.path, tuple(captures), tuple(evidence), pack, fonttools, pypdf
    )
    capture.table_capture.source_revision(source_revision)
    capture.write_new(output / "repeatability.json", capture.canonical(result) + b"\n")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--prepared", type=Path, required=True)
    parser.add_argument("--source-revision", required=True)
    args = parser.parse_args()
    try:
        result = run(args.output, args.prepared, args.source_revision)
    except (
        OSError,
        ValueError,
        render.RenderDependencyError,
        render.VisualMetricError,
    ) as error:
        print(f"render_oracle_ci: {error}", file=sys.stderr)
        return 1
    print(
        capture.canonical(
            {"scope": result["scope"], "summary": result["summary"]}
        ).decode()
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
