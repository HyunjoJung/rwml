#!/usr/bin/env python3
"""Capture and independently verify repeated unequal-table LibreOffice diagnostics."""

from __future__ import annotations

import argparse
import json
import platform
from pathlib import Path
import re
import shutil
import sys

import libreoffice_container as runtime
from generate_unequal_table_oracle import CASES, check_materialized, materialize
from libreoffice_oracle_fonts import sfnt_revision
from render_oracle_contract import (
    _assert_path_neutral,
    _load_json,
    _require_exact_keys,
    _require_safe_text,
    _require_sha256,
    load_corpus_manifest,
)
from render_validate import (
    Image,
    fitz,
    reference_page_digests,
    reference_pdf_font_identities,
)
from table_oracle_topology import (
    _harness_sha256 as topology_harness_sha256,
    _write_json as write_json,
    build_capture_report,
    compare_capture_reports,
    extract_pdf,
    load_capture_report,
    load_comparison_report,
)
from word_oracle_capture import load_word_font_lock

ROOT = runtime.ROOT
SCHEMA = "rwml.libreoffice-table-capture.v1"
RUN_SCHEMA = "rwml.libreoffice-table-export.v1"
DEFAULT_OUTPUT = ROOT / "target/render-oracle/libreoffice-tables"
FONT_NAME = "NotoSans-Regular.ttf"
FONT_PATHS = b"/oracle/fonts/NotoSans-Regular.ttf\n"
MAX_JSON_BYTES = 16 * 1024 * 1024
BUNDLE_KEYS = {
    "schema",
    "campaign",
    "source_revision",
    "runtime_lock_sha256",
    "executor",
    "producer",
    "font_lock",
    "harness",
    "analysis_tools",
    "runs",
    "repeatability",
}


def require_equal(expected: object, actual: object, label: str) -> None:
    if runtime.canonical_json(expected) != runtime.canonical_json(actual):
        raise ValueError(f"{label} differs")


def source_revision(expected: str | None = None, *, clean: bool = True) -> str:
    revision = (
        runtime.run_bounded(["git", "-C", str(ROOT), "rev-parse", "HEAD"])
        .decode()
        .strip()
    )
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None or (
        expected is not None and expected != revision
    ):
        raise ValueError("capture source revision differs")
    if clean and runtime.run_bounded(
        ["git", "-C", str(ROOT), "status", "--porcelain=v1", "--untracked-files=all"]
    ):
        raise ValueError("capture requires a clean source tree")
    return revision


def font_payload(path: Path) -> bytes:
    lock = load_word_font_lock()
    payload = runtime.read_regular_file(path, lock["file"]["bytes"])
    if (
        len(payload) != lock["file"]["bytes"]
        or runtime.sha256(payload) != lock["file"]["sha256"]
    ):
        raise ValueError("font does not match the Word diagnostic font lock")
    return payload


def font_files(payload: bytes) -> dict[str, bytes]:
    return {
        FONT_NAME: payload,
        "SHA256SUMS": f"{runtime.sha256(payload)}  {FONT_NAME}\n".encode(),
        "expected-paths.txt": FONT_PATHS,
    }


def verify_directory(directory: Path, expected: dict[str, bytes]) -> None:
    if (
        directory.is_symlink()
        or not directory.is_dir()
        or {p.name for p in directory.iterdir()} != set(expected)
    ):
        raise ValueError("staged file set differs")
    for name, payload in expected.items():
        if runtime.read_regular_file(directory / name, max(1, len(payload))) != payload:
            raise ValueError("staged file identity differs")


def stage_font(path: Path, directory: Path) -> bytes:
    payload = font_payload(path)
    directory.mkdir(exist_ok=False)
    for name, content in font_files(payload).items():
        (directory / name).write_bytes(content)
    verify_directory(directory, font_files(payload))
    return payload


def validate_capture_content(entries: dict[str, bytes]) -> None:
    if set(entries) != runtime.CAPTURE_MEMBERS:
        raise ValueError("capture file set differs")
    if entries["version.txt"].strip() != runtime.VERSION_LINE.encode():
        raise ValueError("captured LibreOffice version differs")
    if entries["fonts.txt"] != FONT_PATHS:
        raise ValueError("captured font closure differs")
    if not entries["output.pdf"].startswith(b"%PDF-"):
        raise ValueError("captured output is not a PDF")
    expected = runtime.sha256(entries["output.pdf"]) + "  output.pdf\n"
    if entries["sha256.txt"] != expected.encode():
        raise ValueError("captured PDF digest differs")


def harness_identity() -> dict[str, str]:
    names = [
        "libreoffice_container.py",
        "libreoffice_table_capture.py",
        "libreoffice_oracle_fonts.py",
        "word_oracle_capture.py",
        "render_oracle_contract.py",
        "render_validate.py",
        "render_evidence_metrics.py",
        "render_pdf_diagnostics.py",
        "table_oracle_topology.py",
        "generate_unequal_table_oracle.py",
    ]
    return {
        name: runtime.sha256(
            runtime.read_regular_file(ROOT / "scripts" / name, 1024 * 1024)
        )
        for name in names
    }


def analysis_tools() -> dict[str, str]:
    if fitz is None or Image is None:
        raise ValueError("PyMuPDF and Pillow are required for capture validation")
    return {
        "python": platform.python_version(),
        "pymupdf": str(fitz.__version__),
        "pillow": str(Image.__version__),
    }


def execution_identity() -> dict:
    values = json.loads(
        runtime.run_bounded(["docker", "version", "--format", "{{json .}}"])
    )
    if not isinstance(values, dict):
        raise ValueError("Docker runtime identity is malformed")
    result = {}
    for section in ("Client", "Server"):
        source = values.get(section)
        if not isinstance(source, dict):
            raise ValueError("Docker runtime identity is unavailable")
        result[section.lower()] = {
            key: source.get(key)
            for key in ("Version", "ApiVersion", "GitCommit", "Os", "Arch")
        }
    kernel = (
        runtime.run_bounded(["docker", "info", "--format", "{{.KernelVersion}}"])
        .decode()
        .strip()
    )
    result["kernel"] = kernel
    executable = shutil.which("docker")
    if executable is None:
        raise ValueError("Docker client is unavailable")
    result["client_sha256"] = runtime.sha256(
        runtime.read_regular_file(Path(executable).resolve(), 128 * 1024 * 1024)
    )
    validate_executor(result)
    return result


def validate_executor(value: dict) -> None:
    if not isinstance(value, dict):
        raise ValueError("capture executor identity is invalid")
    _require_exact_keys(
        value, {"client", "server", "kernel", "client_sha256"}, "executor"
    )
    _require_safe_text(value["kernel"], "executor kernel")
    _require_sha256(value["client_sha256"], "executor client digest")
    for section in ("client", "server"):
        details = value[section]
        if not isinstance(details, dict):
            raise ValueError("executor section is invalid")
        _require_exact_keys(
            details,
            {"Version", "ApiVersion", "GitCommit", "Os", "Arch"},
            "executor section",
        )
        for item in details.values():
            _require_safe_text(item, "executor value")
    if value["server"]["Os"] != "linux":
        raise ValueError("capture requires a Linux Docker daemon")


def producer_identity(lock: dict, font: bytes, executor: dict) -> dict:
    identity = {
        "runtime": lock,
        "font_sha256": runtime.sha256(font),
        "executor": executor,
    }
    return {
        "name": "libreoffice",
        "mode": "locked-container",
        "version": runtime.VERSION_LINE,
        "identity_sha256": runtime.sha256(runtime.canonical_json(identity)),
        "platform": {
            "system": "Linux",
            "release": executor["kernel"],
            "machine": "x86_64",
        },
    }


def export_row(document, directory: Path, font: bytes) -> dict:
    pdf = directory / "pdf" / f"{document.case_id}.pdf"
    pdf_bytes = runtime.read_regular_file(pdf, runtime.MAX_PDF_BYTES)
    auxiliary = directory / "metadata" / document.case_id
    if auxiliary.is_symlink() or not auxiliary.is_dir():
        raise ValueError("capture metadata directory is invalid")
    names = runtime.CAPTURE_MEMBERS - {"output.pdf"}
    if {entry.name for entry in auxiliary.iterdir()} != names:
        raise ValueError("capture metadata set differs")
    entries = {
        name: runtime.read_regular_file(auxiliary / name, 65536, allow_empty=True)
        for name in names
    }
    validate_capture_content({**entries, "output.pdf": pdf_bytes})
    fonts = reference_pdf_font_identities(pdf)
    expected_fonts = [
        {"postscript_name": "NotoSans-Regular", "sfnt_revision": sfnt_revision(font)}
    ]
    require_equal(expected_fonts, fonts, "PDF font identity")
    rasters = reference_page_digests(pdf, dpi=110, page_cap=32)
    if not rasters:
        raise ValueError("complete PDF raster evidence is unavailable")
    return {
        "case_id": document.case_id,
        "input_bytes": document.input_bytes,
        "input_sha256": document.sha256,
        "pdf_bytes": len(pdf_bytes),
        "pdf_sha256": runtime.sha256(pdf_bytes),
        "fonts": fonts,
        "raster_sha256": rasters,
        "auxiliary_sha256": {
            name: runtime.sha256(data) for name, data in entries.items()
        },
    }


def capture_run(
    corpus,
    directory: Path,
    image: str,
    fonts: Path,
    font: bytes,
    producer: dict,
    revision: str,
) -> dict:
    directory.mkdir()
    (directory / "pdf").mkdir()
    (directory / "metadata").mkdir()
    staging = directory / "staging"
    staging.mkdir()
    rows = []
    for document in corpus.documents:
        source = staging / document.case_id
        source.mkdir()
        payload = runtime.read_regular_file(document.path, document.input_bytes)
        if (
            len(payload) != document.input_bytes
            or runtime.sha256(payload) != document.sha256
        ):
            raise ValueError("corpus input changed before capture")
        expected_source = {
            "input.docx": payload,
            "SHA256SUMS": f"{document.sha256}  input.docx\n".encode(),
        }
        for name, data in expected_source.items():
            (source / name).write_bytes(data)
        captured = runtime.capture_document(image, source, fonts)
        verify_directory(source, expected_source)
        verify_directory(fonts, font_files(font))
        validate_capture_content(captured)
        (directory / "pdf" / f"{document.case_id}.pdf").write_bytes(
            captured.pop("output.pdf")
        )
        auxiliary = directory / "metadata" / document.case_id
        auxiliary.mkdir()
        for name, data in captured.items():
            (auxiliary / name).write_bytes(data)
        rows.append(export_row(document, directory, font))
        shutil.rmtree(source)
        print(f"{directory.name}: {document.case_id}", flush=True)
    staging.rmdir()
    report = {"schema": RUN_SCHEMA, "documents": rows}
    write_json(directory / "exports.json", report)
    topology = build_capture_report(
        corpus, directory / "pdf", producer, source_revision=revision
    )
    write_json(directory / "topology.json", topology)
    return topology


def validate_topology_binding(topology: dict, producer: dict, revision: str) -> None:
    require_equal(producer, topology["producer"], "topology producer")
    environment = topology["environment"]
    require_equal(revision, environment["source_revision"], "topology revision")
    require_equal(False, environment["source_dirty"], "topology source state")
    require_equal(
        topology_harness_sha256(), environment["harness_sha256"], "topology harness"
    )
    require_equal(
        [{"name": "pymupdf", "version": analysis_tools()["pymupdf"]}],
        environment["tools"],
        "topology tools",
    )
    require_equal(
        harness_identity()["table_oracle_topology.py"],
        topology["extractor"]["identity_sha256"],
        "topology extractor",
    )


def validate_bundle(output: Path, *, bundle: dict | None = None) -> dict:
    if output.is_symlink() or not output.is_dir():
        raise ValueError("capture directory is unavailable or symlinked")
    if bundle is None:
        bundle, _ = _load_json(output / "CAPTURE.json", MAX_JSON_BYTES)
    if not isinstance(bundle, dict):
        raise ValueError("capture bundle is invalid")
    _require_exact_keys(bundle, BUNDLE_KEYS, "capture bundle")
    require_equal(SCHEMA, bundle["schema"], "capture schema")
    if (
        not isinstance(bundle["source_revision"], str)
        or re.fullmatch(r"[0-9a-f]{40}", bundle["source_revision"]) is None
    ):
        raise ValueError("capture source revision is malformed")
    lock = runtime.load_runtime_lock()
    require_equal(
        runtime.sha256(runtime.canonical_json(lock)),
        bundle["runtime_lock_sha256"],
        "runtime lock",
    )
    require_equal(harness_identity(), bundle["harness"], "capture harness")
    require_equal(analysis_tools(), bundle["analysis_tools"], "capture analysis tools")
    require_equal(load_word_font_lock(), bundle["font_lock"], "capture font lock")
    font = font_payload(output / "fonts" / FONT_NAME)
    verify_directory(output / "fonts", font_files(font))
    corpus_path = output / "corpus/RENDER_ORACLE.json"
    if not check_materialized(corpus_path.parent):
        raise ValueError("captured corpus differs from the checked-in lock")
    corpus = load_corpus_manifest(corpus_path)
    require_equal(corpus.identity(), bundle["campaign"], "capture corpus")
    revision = source_revision(bundle["source_revision"], clean=False)
    executor = bundle["executor"]
    validate_executor(executor)
    producer = producer_identity(lock, font, executor)
    require_equal(producer, bundle["producer"], "capture producer")
    reports = []
    digests = []
    rows_by_run = []
    for run in ("run-a", "run-b"):
        directory = output / run
        if (
            directory.is_symlink()
            or (directory / "pdf").is_symlink()
            or (directory / "metadata").is_symlink()
        ):
            raise ValueError("capture directory is symlinked")
        expected_pdfs = {f"{case.case_id}.pdf" for case in CASES}
        if {path.name for path in (directory / "pdf").iterdir()} != expected_pdfs:
            raise ValueError("capture PDF set differs")
        if {path.name for path in (directory / "metadata").iterdir()} != {
            case.case_id for case in CASES
        }:
            raise ValueError("capture metadata document set differs")
        rows = [export_row(document, directory, font) for document in corpus.documents]
        stored_exports, _ = _load_json(directory / "exports.json", MAX_JSON_BYTES)
        require_equal(
            {"schema": RUN_SCHEMA, "documents": rows}, stored_exports, "export report"
        )
        topology = load_capture_report(directory / "topology.json", corpus)
        validate_topology_binding(topology, producer, revision)
        for document, case, stored in zip(
            corpus.documents, CASES, topology["documents"], strict=True
        ):
            actual = extract_pdf(directory / "pdf" / f"{case.case_id}.pdf", case)
            actual.update(
                input_bytes=document.input_bytes, input_sha256=document.sha256
            )
            require_equal(actual, stored, "PDF topology")
        reports.append(topology)
        rows_by_run.append(rows)
        digests.append(
            {
                "run_id": run,
                "exports_sha256": runtime.sha256(
                    runtime.canonical_json(stored_exports)
                ),
                "topology_sha256": runtime.sha256(runtime.canonical_json(topology)),
            }
        )
    comparison = load_comparison_report(
        output / "repeatability.json", reports[1], reports[0], corpus
    )
    exact = comparison["summary"]["normalized_exact_documents"]
    raster_exact = sum(
        a["raster_sha256"] == b["raster_sha256"]
        for a, b in zip(*rows_by_run, strict=True)
    )
    if exact != len(CASES) or raster_exact != len(CASES):
        raise ValueError("LibreOffice capture is not completely repeatable")
    expected = {
        "schema": SCHEMA,
        "campaign": corpus.identity(),
        "source_revision": revision,
        "runtime_lock_sha256": runtime.sha256(runtime.canonical_json(lock)),
        "executor": executor,
        "producer": producer,
        "font_lock": load_word_font_lock(),
        "harness": harness_identity(),
        "analysis_tools": analysis_tools(),
        "runs": digests,
        "repeatability": {
            "documents": len(CASES),
            "normalized_exact_documents": exact,
            "raster_exact_documents": raster_exact,
            "comparison_sha256": runtime.sha256(runtime.canonical_json(comparison)),
        },
    }
    require_equal(expected, bundle, "capture bundle")
    _assert_path_neutral(bundle)
    return bundle


def capture(output: Path, font_path: Path) -> dict:
    output = output.absolute()
    if output.exists() or output.is_symlink():
        raise ValueError("capture requires a fresh output directory")
    revision = source_revision()
    harness = harness_identity()
    tools = analysis_tools()
    lock = runtime.load_runtime_lock()
    image = runtime.inspect_image(lock)
    executor = execution_identity()
    output.mkdir(parents=True)
    font = stage_font(font_path, output / "fonts")
    producer = producer_identity(lock, font, executor)
    materialize(output / "corpus")
    corpus = load_corpus_manifest(output / "corpus/RENDER_ORACLE.json")
    reports = [
        capture_run(
            corpus, output / name, image, output / "fonts", font, producer, revision
        )
        for name in ("run-a", "run-b")
    ]
    comparison = compare_capture_reports(reports[1], reports[0], corpus)
    write_json(output / "repeatability.json", comparison)
    runs = []
    raster_rows = []
    for name, report in zip(("run-a", "run-b"), reports, strict=True):
        exports, _ = _load_json(output / name / "exports.json", MAX_JSON_BYTES)
        runs.append(
            {
                "run_id": name,
                "exports_sha256": runtime.sha256(runtime.canonical_json(exports)),
                "topology_sha256": runtime.sha256(runtime.canonical_json(report)),
            }
        )
        raster_rows.append(exports["documents"])
    bundle = {
        "schema": SCHEMA,
        "campaign": corpus.identity(),
        "source_revision": revision,
        "runtime_lock_sha256": runtime.sha256(runtime.canonical_json(lock)),
        "executor": executor,
        "producer": producer,
        "font_lock": load_word_font_lock(),
        "harness": harness,
        "analysis_tools": tools,
        "runs": runs,
        "repeatability": {
            "documents": len(CASES),
            "normalized_exact_documents": comparison["summary"][
                "normalized_exact_documents"
            ],
            "raster_exact_documents": sum(
                a["raster_sha256"] == b["raster_sha256"]
                for a, b in zip(*raster_rows, strict=True)
            ),
            "comparison_sha256": runtime.sha256(runtime.canonical_json(comparison)),
        },
    }
    require_equal(revision, source_revision(), "source revision after capture")
    require_equal(harness, harness_identity(), "harness after capture")
    require_equal(lock, runtime.load_runtime_lock(), "runtime lock after capture")
    require_equal(image, runtime.inspect_image(lock), "runtime image after capture")
    require_equal(executor, execution_identity(), "executor after capture")
    require_equal(
        runtime.sha256(font),
        runtime.sha256(font_payload(font_path)),
        "source font after capture",
    )
    validate_bundle(output, bundle=bundle)
    write_json(output / "CAPTURE.json", bundle)
    return bundle


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    create = commands.add_parser("capture")
    create.add_argument("--font", type=Path, required=True)
    create.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    verify = commands.add_parser("validate")
    verify.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = (
            capture(args.output, args.font)
            if args.command == "capture"
            else validate_bundle(args.output)
        )
        print(json.dumps(result["repeatability"], sort_keys=True))
        return 0
    except (OSError, ValueError) as error:
        print(f"libreoffice_table_capture: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
