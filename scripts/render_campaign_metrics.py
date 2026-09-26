#!/usr/bin/env python3
"""Measure verified shared-font captures and independently verify their repeat."""

from __future__ import annotations

import argparse
import hashlib
import math
from pathlib import Path
import platform
import sys
import tempfile
import time

try:
    import render_campaign_capture as capture
    import render_oracle_contract as contract
    import render_validate as render
except ModuleNotFoundError:
    from scripts import render_campaign_capture as capture
    from scripts import render_oracle_contract as contract
    from scripts import render_validate as render

ROOT = Path(__file__).resolve().parents[1]
REQUEST_SCHEMA = "rwml.capture-metric-request.v1"
WORKER_SCHEMA = "rwml.capture-metric-result.v1"
REPEAT_SCHEMA = "rwml.render-campaign-repeat.v2"
MAX_REQUEST_BYTES = 64 * 1024
MAX_RESULT_BYTES = contract.CAPTURE_MEASUREMENT_LIMITS["stdout_bytes"]
MAX_BATCH_SECONDS = contract.CAPTURE_MEASUREMENT_LIMITS["batch_seconds"]
MAX_CASE_SECONDS = contract.CAPTURE_MEASUREMENT_LIMITS["wall_seconds"]
MAX_REPEAT_SECONDS = 2 * (capture.MAX_CAMPAIGN_SECONDS + MAX_BATCH_SECONDS)


def measurement_contract() -> dict:
    return {
        **contract.CAPTURE_MEASUREMENT_LIMITS,
        "address_space_bytes": 4 * 1024 * 1024 * 1024
        if sys.platform.startswith("linux")
        else None,
    }


def visual_settings(value: dict | None = None) -> dict:
    settings = render.validate_visual_settings(
        {"font_mode": "locked-shared-fonts", **(value or {})}
    )
    if settings["font_mode"] != "locked-shared-fonts":
        raise ValueError("capture metrics require locked shared fonts")
    return settings


def validate_recall(value: float) -> float:
    if (
        type(value) not in (int, float)
        or not 0 <= value <= 1
        or not math.isfinite(value)
    ):
        raise ValueError("capture recall minimum is outside its bound")
    return value


def harness_identity() -> str:
    files = capture.harness_identity()
    files[Path(__file__).name] = capture.digest(
        capture.runtime.read_regular_file(Path(__file__), 1024 * 1024)
    )
    return capture.digest(capture.canonical(files))


def reference_rasters(path: Path, *, dpi: int, page_cap: int) -> list[str]:
    digests = []
    with render.fitz.open(path) as document:
        if not 1 <= document.page_count <= page_cap:
            raise ValueError("reference page coverage exceeds its bound")
        for index in range(document.page_count):
            image = render.rasterize_pdf_page(
                document, index, dpi=dpi, pdf_name=path.name
            )
            digest = hashlib.sha256()
            digest.update(image.width.to_bytes(8, "big"))
            digest.update(image.height.to_bytes(8, "big"))
            digest.update(image.mode.encode("ascii"))
            digest.update(image.tobytes())
            digests.append(digest.hexdigest())
    return digests


def measure_case(directory: Path, request: dict) -> dict:
    contract._require_exact_keys(
        request,
        {"schema", "case", "visual", "recall_min", "max_pages"},
        "metric request",
    )
    if request["schema"] != REQUEST_SCHEMA:
        raise ValueError("metric worker request schema differs")
    settings = visual_settings(request["visual"])
    recall_min = validate_recall(request["recall_min"])
    max_pages = contract._require_positive_int(
        request["max_pages"], "max pages", contract.MAX_PAGES_PER_DOCUMENT
    )
    case = request["case"]
    contract._require_exact_keys(
        case,
        {"document", "case_id", "input_bytes", "input_sha256"},
        "metric case",
    )
    render.require_pdf_deps()
    reference, candidate = directory / "reference.pdf", directory / "native.pdf"
    for path in (reference, candidate):
        capture.runtime.read_regular_file(path, capture.resources.worker.MAX_PDF_BYTES)
    report, _ = contract._load_json(directory / "native-report.json", 1024 * 1024)
    kinds = render.warning_kinds(report)
    if kinds is None:
        raise ValueError("captured render report is invalid")
    pages = []
    for path in (candidate, reference):
        with render.fitz.open(path) as document:
            pages.append(document.page_count)
    if any(not 1 <= count <= min(settings["page_cap"], max_pages) for count in pages):
        raise ValueError("metric page coverage exceeds its bound")
    recall = render.text_recall(reference, candidate, kinds, report)
    visual = render.compare_pdf_visuals(
        reference,
        candidate,
        **{
            key: settings[key]
            for key in ("dpi", "page_cap", "foreground_threshold", "ahash_size")
        },
        integer_metrics=True,
        pdf_diagnostics=True,
    )
    row = render.ValidationRow(
        **case,
        status="pass" if recall >= recall_min else "fail",
        recall=recall,
        rwml_pages=pages[0],
        reference_pages=pages[1],
        page_ratio=round(pages[0] / pages[1], 4),
        ahash_similarity=round(render.hash_similarity(reference, candidate), 4),
        render_warnings=len(kinds),
        render_warning_kinds=sorted(kinds),
        **vars(visual),
    )
    return {
        "schema": WORKER_SCHEMA,
        "row": render.row_dict(row),
        "reference_page_digests": reference_rasters(
            reference,
            dpi=settings["dpi"],
            page_cap=settings["page_cap"],
        ),
    }


def run_worker(directory: Path, request: dict, *, timeout: float) -> dict:
    if (
        type(timeout) not in (int, float)
        or not 0 < timeout <= MAX_CASE_SECONDS
        or not math.isfinite(timeout)
    ):
        raise ValueError("metric worker timeout is outside its bound")
    request_path = directory / "request.json"
    payload = capture.canonical(request) + b"\n"
    if len(payload) > MAX_REQUEST_BYTES:
        raise ValueError("metric request exceeds its byte bound")
    capture.write_new(request_path, payload)
    limits = measurement_contract()
    command = [
        sys.executable,
        "-B",
        str(ROOT / "scripts/posix_resource_exec.py"),
        "--cpu-seconds",
        str(limits["cpu_seconds"]),
        "--file-bytes",
        str(limits["file_bytes"]),
        "--open-files",
        str(limits["open_files"]),
        "--processes",
        str(limits["processes"]),
        "--core-bytes",
        "0",
    ]
    if limits["address_space_bytes"] is not None:
        command.extend(["--address-space-bytes", str(limits["address_space_bytes"])])
    command.extend(
        [
            "--",
            str(Path(sys.executable).absolute()),
            "-B",
            str(Path(__file__).resolve()),
            "_worker",
            "--request",
            str(request_path.absolute()),
        ]
    )
    try:
        output = capture.runtime.run_bounded(
            command,
            cwd=ROOT,
            timeout=timeout,
            stdout_limit=MAX_RESULT_BYTES,
        )
    except capture.runtime.ProcessFailed as error:
        raise ValueError(
            "metric worker failed: "
            + error.stderr.decode("utf-8", errors="replace").strip()
        ) from error
    result = capture.resources.strict_json(output)
    contract._require_exact_keys(
        result, {"schema", "row", "reference_page_digests"}, "metric worker result"
    )
    if result["schema"] != WORKER_SCHEMA:
        raise ValueError("metric worker result schema differs")
    capture.require_equal(
        payload,
        capture.runtime.read_regular_file(request_path, MAX_REQUEST_BYTES),
        "metric worker request",
    )
    return result


def _case_payloads(root: Path, row: dict) -> dict[str, bytes]:
    directory = root / "cases" / row["case_id"]
    result = {}
    for name, relative, expected, maximum in (
        (
            "native.pdf",
            "native.pdf",
            row["native"]["pdf"],
            capture.resources.worker.MAX_PDF_BYTES,
        ),
        (
            "reference.pdf",
            "reference/output.pdf",
            row["reference"]["pdf"],
            capture.resources.worker.MAX_PDF_BYTES,
        ),
        ("native-report.json", "native-report.json", row["native_report"], 1024 * 1024),
        (
            "native-fonts.json",
            "native-fonts.json",
            row["native"]["font_checks"],
            capture.resources.worker.MAX_RESULT_BYTES,
        ),
        (
            "reference-fonts.json",
            "reference-fonts.json",
            row["reference"]["font_checks"],
            capture.resources.worker.MAX_RESULT_BYTES,
        ),
    ):
        payload = capture.runtime.read_regular_file(directory / relative, maximum)
        capture.require_equal(
            expected, capture.identity(payload), f"metric input {name}"
        )
        result[name] = payload
    return result


def check_artifacts(
    root: Path, bundle: dict, corpus, sources: dict, lock, *, deadline: float
) -> None:
    if root.is_symlink() or {p.name for p in root.iterdir()} != {
        "CAPTURE.json",
        "renderer",
        "fonts",
        "cases",
    }:
        raise ValueError("metric capture root coverage differs")
    capture.verify_files(root / "fonts", capture.font_files(lock.fonts, sources))
    if (root / "cases").is_symlink() or {
        p.name for p in (root / "cases").iterdir()
    } != {document.case_id for document in corpus.documents}:
        raise ValueError("metric capture case coverage differs")
    capture.require_equal(
        {key: bundle["renderer"][key] for key in ("bytes", "sha256")},
        capture.identity(
            capture.runtime.read_regular_file(
                root / "renderer", capture.MAX_RENDERER_BYTES
            )
        ),
        "metric renderer",
    )
    for document, row in zip(corpus.documents, bundle["rows"], strict=True):
        capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
        directory = root / "cases" / document.case_id
        if directory.is_symlink() or {p.name for p in directory.iterdir()} != {
            capture.STAGED_INPUT_NAME,
            "SHA256SUMS",
            "reference",
            "native.pdf",
            "native-report.json",
            "native-fonts.json",
            "reference-fonts.json",
        }:
            raise ValueError("metric case artifacts differ")
        source = capture.source_payload(document)
        capture.require_equal(
            source,
            capture.runtime.read_regular_file(
                directory / capture.STAGED_INPUT_NAME, document.input_bytes
            ),
            "metric source",
        )
        capture.require_equal(
            (capture.digest(source) + f"  {capture.STAGED_INPUT_NAME}\n").encode(),
            capture.runtime.read_regular_file(directory / "SHA256SUMS", 1024),
            "metric source checksum",
        )
        _case_payloads(root, row)
        auxiliary = capture.read_artifacts(
            directory / "reference",
            capture.runtime.CAPTURE_MEMBERS,
            capture.runtime.MAX_PDF_BYTES,
        )
        capture.require_equal(
            row["reference_auxiliary"],
            {
                name: capture.identity(payload)
                for name, payload in auxiliary.items()
                if name != "output.pdf"
            },
            "metric reference metadata",
        )


def environment(bundle: dict) -> dict:
    material = bundle["environment"]
    versions = capture.table_capture.analysis.tool_versions(material["analysis_tools"])
    return {
        "source_revision": bundle["source_revision"],
        "source_dirty": False,
        "harness_sha256": harness_identity(),
        "cargo_lock_sha256": bundle["renderer"]["cargo_lock_sha256"],
        "renderer": {"name": "rwml", "font_mode": "locked-shared-fonts"},
        "oracle": {
            "name": "libreoffice",
            "mode": "locked-container",
            "version": capture.runtime.VERSION_LINE,
            "identity_sha256": capture.digest(capture.canonical(material)),
        },
        "platform": {
            "system": platform.system(),
            "release": platform.release(),
            "machine": platform.machine(),
        },
        "tools": [
            {"name": name, "version": version}
            for name, version in sorted(versions.items())
        ],
    }


def check_retained_state(
    manifest: Path,
    root: Path,
    corpus,
    bundle: dict,
    receipt: bytes,
    env: dict,
    pack: Path,
    fonttools: Path,
    pypdf: Path,
    *,
    deadline: float,
) -> None:
    capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
    material, sources, lock, _ = capture.prepare_environment(pack, fonttools, pypdf)
    capture.require_equal(bundle["environment"], material, "metric environment")
    check_artifacts(root, bundle, corpus, sources, lock, deadline=deadline)
    capture.require_equal(
        receipt,
        capture.runtime.read_regular_file(
            root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
        ),
        "metric capture receipt",
    )
    capture.require_equal(env, environment(bundle), "metric environment and harness")
    capture.table_capture.source_revision(bundle["source_revision"])
    if contract.load_corpus_manifest(manifest) != corpus:
        raise ValueError("metric corpus changed")
    capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)


def run(
    manifest: Path,
    root: Path,
    pack: Path,
    fonttools: Path,
    pypdf: Path,
    *,
    settings: dict | None = None,
    recall_min: float = 0.97,
    evidence: Path | None = None,
) -> dict:
    settings = visual_settings(settings)
    validate_recall(recall_min)
    corpus = contract.load_corpus_manifest(manifest)
    expected, expected_bytes = (
        contract._load_json(evidence, contract.MAX_EVIDENCE_BYTES)
        if evidence
        else (None, None)
    )
    if expected is not None:
        contract.validate_evidence_report(expected, corpus)
        if expected["schema"] != contract.CAPTURE_EVIDENCE_SCHEMA:
            raise ValueError("verification requires capture-bound evidence")
        capture.require_equal(
            {
                key: value
                for key, value in expected["visual_comparison"].items()
                if key != "integer_metrics"
            },
            settings,
            "requested metric settings",
        )
        if expected["summary"]["recall_min"] != recall_min:
            raise ValueError("requested metric recall minimum differs")
    receipt = capture.runtime.read_regular_file(
        root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
    )
    bundle = capture.run(manifest, root, pack, fonttools, pypdf, verify=True)
    deadline = time.monotonic() + MAX_BATCH_SECONDS
    env = environment(bundle)
    cases, rows = [], []
    scratch = ROOT / "target/render-oracle/capture-metrics"
    scratch.mkdir(parents=True, exist_ok=True)
    for document, recorded in zip(corpus.documents, bundle["rows"], strict=True):
        capture.remaining_seconds(deadline, MAX_BATCH_SECONDS)
        payloads = _case_payloads(root, recorded)
        request = {
            "schema": REQUEST_SCHEMA,
            "case": {
                "document": document.path.name,
                "case_id": document.case_id,
                "input_bytes": document.input_bytes,
                "input_sha256": document.sha256,
            },
            "visual": settings,
            "recall_min": recall_min,
            "max_pages": corpus.limits["max_pages_per_document"],
        }
        with tempfile.TemporaryDirectory(dir=scratch) as temporary:
            directory = Path(temporary)
            for name in ("native.pdf", "reference.pdf", "native-report.json"):
                capture.write_new(directory / name, payloads[name])
            measured = run_worker(
                directory,
                request,
                timeout=capture.remaining_seconds(deadline, MAX_CASE_SECONDS),
            )
            for name in ("native.pdf", "reference.pdf", "native-report.json"):
                capture.require_equal(
                    payloads[name],
                    capture.runtime.read_regular_file(
                        directory / name, len(payloads[name])
                    ),
                    "metric worker input",
                )
        current = _case_payloads(root, recorded)
        for name, payload in payloads.items():
            capture.require_equal(payload, current[name], "retained metric input")
        rows.append(render.ValidationRow(**measured["row"]))
        cases.append(
            {
                "case_id": document.case_id,
                "input_sha256": document.sha256,
                "native_pdf_sha256": recorded["native"]["pdf"]["sha256"],
                "reference_pdf_sha256": recorded["reference"]["pdf"]["sha256"],
                "native_report_sha256": recorded["native_report"]["sha256"],
                "native_fonts_sha256": recorded["native"]["font_checks"]["sha256"],
                "reference_fonts_sha256": recorded["reference"]["font_checks"][
                    "sha256"
                ],
                "reference_page_digests": measured["reference_page_digests"],
            }
        )
        print(
            f"{len(rows)}/{len(corpus.documents)} {document.case_id}",
            file=sys.stderr,
            flush=True,
        )
    core = render.validation_report(
        rows,
        recall_min,
        thresholds={"max_skipped": 0},
        visual_settings=settings,
        integer_metrics=True,
        pdf_diagnostics=True,
    )
    binding = {
        "schema": capture.SCHEMA,
        "sha256": capture.digest(receipt),
        "environment_sha256": env["oracle"]["identity_sha256"],
        "source_revision": bundle["source_revision"],
        "campaign": corpus.identity(),
        "renderer_sha256": bundle["renderer"]["sha256"],
        "font_scope": "declared-font-resources",
        "measurement": measurement_contract(),
        "cases": cases,
    }
    result = contract.bind_evidence_report(core, corpus, env, capture=binding)
    if len(capture.canonical(result)) + 1 > contract.MAX_EVIDENCE_BYTES:
        raise ValueError("metric evidence exceeds its byte bound")
    check_retained_state(
        manifest,
        root,
        corpus,
        bundle,
        receipt,
        env,
        pack,
        fonttools,
        pypdf,
        deadline=deadline,
    )
    if expected is not None:
        capture.require_equal(expected, result, "independently recomputed metrics")
        capture.require_equal(
            expected_bytes,
            capture.runtime.read_regular_file(evidence, contract.MAX_EVIDENCE_BYTES),
            "metric evidence",
        )
    capture.remaining_seconds(deadline, MAX_BATCH_SECONDS)
    return result


def verify_repeat(
    manifest: Path,
    roots: tuple[Path, Path],
    evidence: tuple[Path, Path],
    pack: Path,
    fonttools: Path,
    pypdf: Path,
) -> dict:
    deadline = time.monotonic() + MAX_REPEAT_SECONDS
    if (
        roots[0].resolve() == roots[1].resolve()
        or evidence[0].resolve() == evidence[1].resolve()
    ):
        raise ValueError("repeat requires distinct captures and evidence")
    corpus = contract.load_corpus_manifest(manifest)
    retained = [
        contract._load_json(path, contract.MAX_EVIDENCE_BYTES) for path in evidence
    ]
    captures = [
        contract._load_json(root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES)
        for root in roots
    ]
    verified = []
    for index, (report, _) in enumerate(retained):
        capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
        contract.validate_evidence_report(report, corpus)
        if report["schema"] != contract.CAPTURE_EVIDENCE_SCHEMA:
            raise ValueError("repeat requires capture-bound evidence")
        capture.require_equal(
            report["capture"]["sha256"],
            capture.digest(captures[index][1]),
            "repeat capture identity",
        )
        settings = {
            key: value
            for key, value in report["visual_comparison"].items()
            if key != "integer_metrics"
        }
        verified.append(
            run(
                manifest,
                roots[index],
                pack,
                fonttools,
                pypdf,
                settings=settings,
                recall_min=report["summary"]["recall_min"],
                evidence=evidence[index],
            )
        )
        capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
    capture.require_equal(
        {key: value for key, value in verified[0].items() if key != "capture"},
        {key: value for key, value in verified[1].items() if key != "capture"},
        "repeated metric content",
    )
    for key in (
        "environment_sha256",
        "source_revision",
        "campaign",
        "renderer_sha256",
        "font_scope",
        "measurement",
    ):
        capture.require_equal(
            verified[0]["capture"][key],
            verified[1]["capture"][key],
            f"repeated capture {key}",
        )
    cases = []
    for first, second in zip(
        verified[0]["capture"]["cases"], verified[1]["capture"]["cases"], strict=True
    ):
        capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
        for key in (
            "case_id",
            "input_sha256",
            "native_pdf_sha256",
            "native_report_sha256",
            "native_fonts_sha256",
            "reference_page_digests",
        ):
            capture.require_equal(first[key], second[key], f"repeated {key}")
        for filename, maximum in (
            ("native.pdf", capture.resources.worker.MAX_PDF_BYTES),
            ("native-report.json", 1024 * 1024),
            ("native-fonts.json", capture.resources.worker.MAX_RESULT_BYTES),
        ):
            payloads = [
                capture.runtime.read_regular_file(
                    root / "cases" / first["case_id"] / filename, maximum
                )
                for root in roots
            ]
            capture.require_equal(payloads[0], payloads[1], f"native {filename} repeat")
        cases.append(
            {
                key: first[key]
                for key in (
                    "case_id",
                    "input_sha256",
                    "native_pdf_sha256",
                    "native_report_sha256",
                    "native_fonts_sha256",
                    "reference_page_digests",
                )
            }
        )
    for index in range(2):
        bundle, receipt = captures[index]
        check_retained_state(
            manifest,
            roots[index],
            corpus,
            bundle,
            receipt,
            verified[index]["environment"],
            pack,
            fonttools,
            pypdf,
            deadline=deadline,
        )
        capture.require_equal(
            retained[index][1],
            capture.runtime.read_regular_file(
                evidence[index], contract.MAX_EVIDENCE_BYTES
            ),
            "repeat metric receipt",
        )
    result = {
        "schema": REPEAT_SCHEMA,
        "scope": "diagnostic-repeat-not-release-acceptance",
        "source_revision": verified[0]["environment"]["source_revision"],
        "campaign": corpus.identity(),
        "cases": cases,
        "captures": [
            {
                "capture_sha256": report["capture"]["sha256"],
                "evidence_sha256": capture.digest(retained[index][1]),
            }
            for index, report in enumerate(verified)
        ],
        "metric_core_sha256": capture.digest(
            capture.canonical(
                {key: value for key, value in verified[0].items() if key != "capture"}
            )
        ),
        "summary": {
            "documents": len(cases),
            "reference_pages": sum(len(row["reference_page_digests"]) for row in cases),
            "exact_native_pairs": len(cases),
            "exact_reference_rasters": len(cases),
            "metrics_recomputed": True,
            "fidelity_gate_passed": verified[0]["gate"]["passed"],
        },
    }
    contract._assert_path_neutral(result)
    if len(capture.canonical(result)) + 1 > contract.MAX_EVIDENCE_BYTES:
        raise ValueError("repeat receipt exceeds its bound")
    capture.remaining_seconds(deadline, MAX_REPEAT_SECONDS)
    return result


def fresh_output(path: Path, protected: list[Path]) -> None:
    if path.exists() or path.is_symlink():
        raise ValueError("output must be fresh")
    if any(path.resolve().is_relative_to(root.resolve()) for root in protected):
        raise ValueError("output overlaps retained inputs")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="mode", required=True)
    worker = sub.add_parser("_worker", help=argparse.SUPPRESS)
    worker.add_argument("--request", type=Path, required=True)
    for name in ("measure", "verify", "repeat"):
        command = sub.add_parser(name)
        for option in ("manifest", "font-pack", "fonttools-wheel", "pypdf-wheel"):
            command.add_argument("--" + option, type=Path, required=True)
        if name == "repeat":
            for option in (
                "first-capture",
                "second-capture",
                "first-evidence",
                "second-evidence",
                "output",
            ):
                command.add_argument("--" + option, type=Path, required=True)
        else:
            command.add_argument("--capture", type=Path, required=True)
            command.add_argument(
                "--output" if name == "measure" else "--evidence",
                type=Path,
                required=True,
            )
            command.add_argument("--recall-min", type=float, default=0.97)
            command.add_argument("--dpi", type=int, default=render.DEFAULT_RASTER_DPI)
            command.add_argument(
                "--page-cap", type=int, default=render.DEFAULT_PAGE_CAP
            )
    args = parser.parse_args()
    try:
        if args.mode == "_worker":
            request, _ = contract._load_json(args.request, MAX_REQUEST_BYTES)
            result = measure_case(args.request.parent, request)
            payload = capture.canonical(result)
            if len(payload) + 1 > MAX_RESULT_BYTES:
                raise ValueError("metric worker output exceeds its bound")
            print(payload.decode("ascii"))
            return 0
        if args.mode == "repeat":
            roots = (args.first_capture, args.second_capture)
            evidence = (args.first_evidence, args.second_evidence)
            fresh_output(args.output, [*roots, args.font_pack, args.manifest.parent])
            result = verify_repeat(
                args.manifest,
                roots,
                evidence,
                args.font_pack,
                args.fonttools_wheel,
                args.pypdf_wheel,
            )
        else:
            if args.mode == "measure":
                fresh_output(
                    args.output, [args.capture, args.font_pack, args.manifest.parent]
                )
            result = run(
                args.manifest,
                args.capture,
                args.font_pack,
                args.fonttools_wheel,
                args.pypdf_wheel,
                settings={"dpi": args.dpi, "page_cap": args.page_cap},
                recall_min=args.recall_min,
                evidence=args.evidence if args.mode == "verify" else None,
            )
        if args.mode != "verify":
            capture.write_new(args.output, capture.canonical(result) + b"\n")
        print(capture.canonical(result["summary"]).decode("ascii"))
        return 0
    except (
        OSError,
        ValueError,
        render.RenderDependencyError,
        render.VisualMetricError,
    ) as error:
        print(f"render_campaign_metrics: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
