#!/usr/bin/env python3
"""Run a commit-bound, twelve-document diagnostic capture pair for CI.

Success proves complete, repeated evidence, not a passing fidelity threshold.
Metric disagreements remain unchanged in both retained v7 evidence reports.
"""

from __future__ import annotations

import argparse
from pathlib import Path
import shutil
import sys
from types import SimpleNamespace

import generate_render_smoke_manifest as smoke
import render_campaign_capture as capture
import render_campaign_repeat as repeat
import render_oracle_contract as contract
import render_validate as render


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
    output.mkdir(parents=True)
    shutil.copytree(prepared / "licenses", output / "licenses")
    captures = [output / f"capture-{label}" for label in ("a", "b")]
    evidence = [output / f"evidence-{label}.json" for label in ("a", "b")]
    settings = render.validate_visual_settings({"font_mode": "locked-shared-fonts"})
    for root, report_path in zip(captures, evidence):
        capture.run(corpus.path, root, pack, fonttools, pypdf)
        args = SimpleNamespace(
            capture_dir=root,
            shared_font_pack=pack,
            fonttools_wheel=fonttools,
            pypdf_wheel=pypdf,
            source_revision=source_revision,
            recall_min=0.97,
        )
        report = render.captured_validation_report(
            args, corpus, {"max_skipped": 0}, settings
        )
        contract.validate_evidence_report(report, corpus)
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
    result = repeat.verify_repeated_campaign(
        corpus.path, *captures, *evidence, pack, fonttools, pypdf
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
