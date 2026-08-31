#!/usr/bin/env python3
"""Independently verify one retained rwml render-campaign capture pair."""

from __future__ import annotations

import argparse
from pathlib import Path
import sys
import tempfile

import render_campaign_capture as capture
import render_oracle_contract as contract
import render_validate as render


SCHEMA = "rwml.render-campaign-repeat.v1"
SCOPE = "diagnostic-repeat-verification-not-release-evidence"
MAX_RECEIPT_BYTES = 16 * 1024 * 1024


def capture_binding(root: Path, bundle: dict, corpus) -> dict:
    receipt = capture.runtime.read_regular_file(
        root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
    )
    return {
        "schema": capture.SCHEMA,
        "sha256": capture.digest(receipt),
        "environment_sha256": capture.digest(capture.canonical(bundle["environment"])),
        "source_revision": bundle["source_revision"],
        "campaign": corpus.identity(),
        "renderer_sha256": bundle["renderer"]["sha256"],
        "font_scope": "declared-font-resources",
        "cases": [
            {
                "case_id": row["case_id"],
                "input_sha256": row["input"]["sha256"],
                "native_pdf_sha256": row["native"]["pdf"]["sha256"],
                "reference_pdf_sha256": row["reference"]["pdf"]["sha256"],
                "native_fonts_sha256": row["native"]["font_checks"]["sha256"],
                "reference_fonts_sha256": row["reference"]["font_checks"]["sha256"],
            }
            for row in bundle["rows"]
        ],
    }


def _actual_identity(path: Path, maximum: int) -> tuple[dict, bytes]:
    payload = capture.runtime.read_regular_file(path, maximum)
    return capture.identity(payload), payload


def _evidence_core(evidence: dict) -> dict:
    return {key: value for key, value in evidence.items() if key != "capture"}


def validate_output_path(output: Path, capture_roots: tuple[Path, Path]) -> Path:
    absolute = output.absolute()
    resolved = absolute.resolve()
    if any(
        resolved.is_relative_to(root.absolute().resolve()) for root in capture_roots
    ):
        raise ValueError("repeat verification output overlaps a capture root")
    if absolute.exists() or absolute.is_symlink():
        raise ValueError("repeat verification output must be fresh")
    return absolute


def replay_native_outputs(root: Path, corpus) -> dict[str, dict[str, bytes]]:
    scratch = capture.ROOT / "target/render-oracle/repeat-verification"
    scratch.mkdir(parents=True, exist_ok=True)
    lock = capture.shared.load_lock()
    fonts = [root / "fonts" / entry["name"] for entry in lock.fonts]
    result = {}
    with tempfile.TemporaryDirectory(dir=scratch) as temporary:
        output = Path(temporary)
        for document in corpus.documents:
            pdf = output / f"{document.case_id}.pdf"
            report = output / f"{document.case_id}.json"
            capture.runtime.run_bounded(
                capture.native_command(
                    root / "renderer",
                    root / "cases" / document.case_id / "input.docx",
                    pdf,
                    report,
                    fonts,
                ),
                timeout=120,
                cwd=capture.ROOT,
            )
            result[document.case_id] = {
                "pdf": capture.runtime.read_regular_file(
                    pdf, capture.resources.worker.MAX_PDF_BYTES
                ),
                "report": capture.runtime.read_regular_file(report, 1024 * 1024),
            }
    return result


def verify_repeated_campaign(
    manifest: Path,
    first_capture: Path,
    second_capture: Path,
    first_evidence: Path,
    second_evidence: Path,
    pack: Path,
    fonttools: Path,
    pypdf: Path,
) -> dict:
    roots = (first_capture.absolute(), second_capture.absolute())
    if roots[0].resolve() == roots[1].resolve():
        raise ValueError("repeat verification requires distinct capture roots")
    evidence_paths = (first_evidence.absolute(), second_evidence.absolute())
    if evidence_paths[0].resolve() == evidence_paths[1].resolve():
        raise ValueError("repeat verification requires distinct evidence reports")

    corpus = contract.load_corpus_manifest(manifest.absolute())
    bundles = [
        capture.run(
            corpus.path,
            root,
            pack.absolute(),
            fonttools.absolute(),
            pypdf.absolute(),
            verify=True,
        )
        for root in roots
    ]
    for key in (
        "schema",
        "scope",
        "source_revision",
        "campaign",
        "environment",
        "renderer",
        "limits",
    ):
        capture.require_equal(
            bundles[0].get(key), bundles[1].get(key), f"capture pair {key}"
        )

    evidence = [contract.load_evidence_report(path, corpus) for path in evidence_paths]
    for index in range(2):
        if evidence[index].get("schema") != contract.CAPTURE_EVIDENCE_SCHEMA:
            raise ValueError("repeat verification requires capture-bound evidence")
        capture.require_equal(
            capture_binding(roots[index], bundles[index], corpus),
            evidence[index].get("capture"),
            "evidence capture binding",
        )
    capture.require_equal(
        _evidence_core(evidence[0]),
        _evidence_core(evidence[1]),
        "metric reports outside capture bindings",
    )
    native_replays = [replay_native_outputs(root, corpus) for root in roots]

    visual = evidence[0]["visual_comparison"]
    dpi = visual["dpi"]
    page_cap = visual["page_cap"]
    cases = []
    reference_pages = 0
    expected_ids = [document.case_id for document in corpus.documents]
    for bundle in bundles:
        capture.require_equal(
            expected_ids,
            [row.get("case_id") for row in bundle["rows"]],
            "repeat case coverage",
        )

    for document, first_row, second_row in zip(
        corpus.documents, bundles[0]["rows"], bundles[1]["rows"]
    ):
        capture.require_equal(
            first_row["input"], second_row["input"], "repeated input identity"
        )
        capture.require_equal(
            document.sha256,
            first_row["input"]["sha256"],
            "repeated manifest input identity",
        )
        paths = [root / "cases" / document.case_id for root in roots]

        native_pdf = []
        native_report = []
        native_fonts = []
        for index, row in enumerate((first_row, second_row)):
            pdf_identity, pdf_payload = _actual_identity(
                paths[index] / "native.pdf", capture.resources.worker.MAX_PDF_BYTES
            )
            capture.require_equal(
                row["native"]["pdf"], pdf_identity, "native PDF identity"
            )
            native_pdf.append(pdf_payload)
            report_identity, report_payload = _actual_identity(
                paths[index] / "native-report.json", 1024 * 1024
            )
            capture.require_equal(
                row["native_report"], report_identity, "native report identity"
            )
            native_report.append(report_payload)
            fonts_identity, fonts_payload = _actual_identity(
                paths[index] / "native-fonts.json",
                capture.resources.worker.MAX_RESULT_BYTES,
            )
            capture.require_equal(
                row["native"]["font_checks"],
                fonts_identity,
                "native font receipt identity",
            )
            native_fonts.append(fonts_payload)
            capture.require_equal(
                pdf_payload,
                native_replays[index][document.case_id]["pdf"],
                "native PDF replay",
            )
            capture.require_equal(
                report_payload,
                native_replays[index][document.case_id]["report"],
                "native report replay",
            )

        capture.require_equal(native_pdf[0], native_pdf[1], "native PDF repeat")
        capture.require_equal(
            native_report[0], native_report[1], "native report repeat"
        )
        capture.require_equal(
            native_fonts[0], native_fonts[1], "native font receipt repeat"
        )

        raster = [
            render.reference_page_digests(
                path / "reference/output.pdf", dpi=dpi, page_cap=page_cap
            )
            for path in paths
        ]
        if not raster[0] or not raster[1]:
            raise ValueError(
                f"reference raster comparison is incomplete: {document.case_id}"
            )
        capture.require_equal(
            raster[0], raster[1], f"reference raster repeat: {document.case_id}"
        )
        reference_pages += len(raster[0])
        cases.append(
            {
                "case_id": document.case_id,
                "input_sha256": document.sha256,
                "native_pdf_sha256": capture.digest(native_pdf[0]),
                "native_report_sha256": capture.digest(native_report[0]),
                "native_fonts_sha256": capture.digest(native_fonts[0]),
                "reference_page_digests": raster[0],
            }
        )

    capture_receipts = [
        capture.runtime.read_regular_file(
            root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
        )
        for root in roots
    ]
    evidence_receipts = [
        capture.runtime.read_regular_file(path, contract.MAX_EVIDENCE_BYTES)
        for path in evidence_paths
    ]
    metric_core = capture.canonical(_evidence_core(evidence[0]))
    result = {
        "schema": SCHEMA,
        "scope": SCOPE,
        "source_revision": bundles[0]["source_revision"],
        "campaign": bundles[0]["campaign"],
        "environment_sha256": capture.digest(
            capture.canonical(bundles[0]["environment"])
        ),
        "renderer": bundles[0]["renderer"],
        "reference_raster": {
            "dpi": dpi,
            "page_cap": page_cap,
            "digest": "sha256-rgb-dimensions-v1",
        },
        "captures": [
            {
                "capture_sha256": capture.digest(capture_receipts[index]),
                "evidence_sha256": capture.digest(evidence_receipts[index]),
            }
            for index in range(2)
        ],
        "metric_report_core_sha256": capture.digest(metric_core),
        "cases": cases,
        "summary": {
            "documents": len(cases),
            "reference_pages": reference_pages,
            "native_pdf_exact": len(cases),
            "native_report_exact": len(cases),
            "native_font_receipts_exact": len(cases),
            "reference_raster_exact": len(cases),
            "metric_reports_exact": True,
        },
    }
    contract._assert_path_neutral(result)
    if len(capture.canonical(result)) > MAX_RECEIPT_BYTES:
        raise ValueError("repeat verification receipt exceeds its byte bound")
    return result


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--first-capture", type=Path, required=True)
    parser.add_argument("--second-capture", type=Path, required=True)
    parser.add_argument("--first-evidence", type=Path, required=True)
    parser.add_argument("--second-evidence", type=Path, required=True)
    parser.add_argument("--font-pack", type=Path, required=True)
    parser.add_argument("--fonttools-wheel", type=Path, required=True)
    parser.add_argument("--pypdf-wheel", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        output = validate_output_path(
            args.output, (args.first_capture, args.second_capture)
        )
        result = verify_repeated_campaign(
            args.manifest,
            args.first_capture,
            args.second_capture,
            args.first_evidence,
            args.second_evidence,
            args.font_pack,
            args.fonttools_wheel,
            args.pypdf_wheel,
        )
        serialized = capture.canonical(result) + b"\n"
        capture.write_new(output, serialized)
        print(
            capture.canonical(
                {
                    "documents": result["summary"]["documents"],
                    "reference_pages": result["summary"]["reference_pages"],
                    "scope": result["scope"],
                }
            ).decode()
        )
        return 0
    except (OSError, ValueError) as error:
        print(f"render_campaign_repeat: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
