#!/usr/bin/env python3
"""Capture two fixed Word pagination probes and a digest-locked 11pt control.

This separate diagnostic profile records repeatable page, text, font and raster
observations. It does not change the 48-case table topology contract or define
expected Word page counts. Generated inputs are repository-owned synthetic OOXML.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import io
import os
import platform
import re
import shutil
import sys
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

try:
    import gen_public_corpus as generator
    import generate_render_pilot as pilot
    import word_oracle_capture as word
except ModuleNotFoundError:
    from scripts import gen_public_corpus as generator
    from scripts import generate_render_pilot as pilot
    from scripts import word_oracle_capture as word


SCRIPT_PATH = Path(__file__).resolve()
PROFILE = "pagination-pilot-v1"
INPUTS = {
    "pilot-unequal-table-continuation": (
        18300,
        "748340d0344926fe0c937233d2b9c12a97c5d905761eac2f8d1b98a17c0ea0cb",
    ),
    "synthetic-pagination-keep": (
        4507,
        "ac3002711280f78ce473e5d1edc1660735485a70e1b6ecec943e720d5b3593fe",
    ),
    "synthetic-pagination-keep-noto11": (
        4701,
        "f04c46ea3da6e4c1ea023fee71389602cf7c0091f351e8a950b61982bd035979",
    ),
}
ORIGINAL_CASE = "synthetic-pagination-keep"
PYMUPDF_VERSION = "1.28.2"
RASTER_DPI = 110
PROVENANCE = (
    "# Word pagination diagnostic inputs\n\n"
    "These MIT-licensed inputs are generated from repository-owned synthetic "
    "OOXML by gen_public_corpus.py and generate_render_pilot.py. The noto11 "
    "control changes only word/styles.xml document run defaults to explicitly "
    "request Noto Sans in all four script slots at 11pt; package member order "
    "and all other part payloads are preserved. INPUTS.json binds exact bytes.\n"
)


def digest(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def validate_inputs(payloads: dict[str, bytes]) -> None:
    """Reject missing, extra or changed inputs independently of their generator."""
    if set(payloads) != set(INPUTS):
        raise ValueError("inputs must contain the exact pilot profile")
    for case_id, payload in payloads.items():
        if (len(payload), digest(payload)) != INPUTS[case_id]:
            raise ValueError(f"pilot input identity mismatch: {case_id}")


def input_payloads() -> dict[str, bytes]:
    """Rebuild the two original inputs and their exact deterministic control."""
    pilot.load_lock()
    payloads = pilot._payloads()
    selected = {
        case_id: payloads[f"documents/{case_id}.docx"]
        for case_id in INPUTS
        if case_id != "synthetic-pagination-keep-noto11"
    }
    with zipfile.ZipFile(io.BytesIO(selected[ORIGINAL_CASE])) as archive:
        parts = [(name, archive.read(name)) for name in archive.namelist()]
    styles = ET.fromstring(dict(parts)["word/styles.xml"])
    namespace = generator.W
    ET.register_namespace("w", namespace)
    defaults = ET.Element(f"{{{namespace}}}docDefaults")
    styles.insert(0, defaults)
    props = ET.SubElement(
        ET.SubElement(defaults, f"{{{namespace}}}rPrDefault"), f"{{{namespace}}}rPr"
    )
    ET.SubElement(
        props,
        f"{{{namespace}}}rFonts",
        {
            f"{{{namespace}}}{key}": "Noto Sans"
            for key in ("ascii", "hAnsi", "eastAsia", "cs")
        },
    )
    for tag in ("sz", "szCs"):
        ET.SubElement(props, f"{{{namespace}}}{tag}", {f"{{{namespace}}}val": "22"})
    control_xml = ET.tostring(styles, encoding="utf-8", xml_declaration=True)
    selected["synthetic-pagination-keep-noto11"] = generator._zip(
        [
            (name, control_xml if name == "word/styles.xml" else data)
            for name, data in parts
        ]
    )
    validate_inputs(selected)
    return selected


def input_manifest() -> dict:
    return {
        "schema": "rwml.word-pilot-inputs.v1",
        "profile": PROFILE,
        "provenance": {
            "reference": "PROVENANCE.md",
            "license": "MIT",
            "sha256": digest(PROVENANCE.encode()),
        },
        "documents": [
            {
                "case_id": case_id,
                "bytes": size,
                "sha256": sha,
                "font_policy": "observe-installed-defaults"
                if case_id == ORIGINAL_CASE
                else "locked-noto-sans-regular",
            }
            for case_id, (size, sha) in INPUTS.items()
        ],
    }


def materialize(output: Path) -> None:
    """Create fresh, public-safe inputs without changing any existing lock."""
    if output.exists() or output.is_symlink():
        raise ValueError("pilot input output must be fresh")
    payloads = input_payloads()
    output.mkdir(parents=True)
    for case_id, payload in payloads.items():
        pilot.batch.atomic_write(output / f"{case_id}.docx", payload)
    pilot.batch.atomic_write(output / "PROVENANCE.md", PROVENANCE.encode())
    word._write_json(output / "INPUTS.json", input_manifest())


def validate_input_directory(directory: Path) -> None:
    expected = {f"{case_id}.docx" for case_id in INPUTS} | {
        "INPUTS.json",
        "PROVENANCE.md",
    }
    if directory.is_symlink() or not directory.is_dir():
        raise ValueError("pilot input directory is unavailable")
    if word._directory_entries(directory) != expected:
        raise ValueError("input directory must contain the exact pilot profile")
    validate_inputs(
        {
            case_id: word._read_regular_file(directory / f"{case_id}.docx", 256 * 1024)
            for case_id in INPUTS
        }
    )
    if word._load_json(directory / "INPUTS.json") != input_manifest():
        raise ValueError("pilot input manifest mismatch")
    if (
        word._read_regular_file(directory / "PROVENANCE.md", 8192)
        != PROVENANCE.encode()
    ):
        raise ValueError("pilot provenance mismatch")


def build_job(corpus: Path, run: Path, font: Path, run_id: str) -> dict:
    """Build a distinct, fixed-profile transport job for the shared COM backend."""
    validate_input_directory(corpus)
    if run_id not in {"run-a", "run-b"}:
        raise ValueError("pilot run ID is invalid")
    lock = word.load_word_font_lock()
    return {
        "schema": "rwml.word-pilot-export-job.v1",
        "run_id": run_id,
        "output_directory": str((run / "pdf").resolve()),
        "metadata_path": str((run / "metadata.json").resolve()),
        "font": {
            "path": str(font.resolve()),
            "family": lock["family"],
            "postscript_name": lock["postscript_name"],
            "bytes": lock["file"]["bytes"],
            "sha256": lock["file"]["sha256"],
        },
        "export": copy.deepcopy(word.WORD_EXPORT_OPTIONS),
        "documents": [
            {
                "case_id": case_id,
                "input": str((corpus / f"{case_id}.docx").resolve()),
                "output": str((run / "pdf" / f"{case_id}.pdf").resolve()),
                "input_bytes": size,
                "input_sha256": sha,
            }
            for case_id, (size, sha) in INPUTS.items()
        ],
    }


def load_metadata(run: Path, run_id: str) -> dict:
    """Validate producer/export/font metadata and the exact three-PDF inventory."""
    metadata = word._load_json(run / "metadata.json")
    word._require_exact_keys(metadata, word.METADATA_KEYS, "pilot metadata")
    if (
        metadata["schema"] != "rwml.word-pilot-export-metadata.v1"
        or metadata["run_id"] != run_id
    ):
        raise ValueError("pilot metadata schema or run identity mismatch")
    word._validate_producer(metadata["producer"], metadata["runtime"])
    word._validate_metadata_font(metadata["font"], word.load_word_font_lock())
    if metadata["export"] != word.WORD_EXPORT_OPTIONS:
        raise ValueError("pilot export options differ")
    pdf_directory = run / "pdf"
    if pdf_directory.is_symlink() or not pdf_directory.is_dir():
        raise ValueError("pilot PDF directory is unavailable")
    if word._directory_entries(pdf_directory) != {
        f"{case_id}.pdf" for case_id in INPUTS
    }:
        raise ValueError("pilot PDF inventory mismatch")
    rows = []
    for case_id in INPUTS:
        payload = word._read_regular_file(
            pdf_directory / f"{case_id}.pdf", word.MAX_PDF_BYTES
        )
        rows.append(
            {
                "case_id": case_id,
                "pdf_bytes": len(payload),
                "pdf_sha256": digest(payload),
            }
        )
    if metadata["documents"] != rows:
        raise ValueError("pilot PDF identity mismatch")
    word._assert_path_neutral(metadata)
    return metadata


def observe_pdf(path: Path, case_id: str) -> dict:
    """Extract bounded font, span-baseline, page and raster observations anew."""
    if case_id not in INPUTS:
        raise ValueError("unknown pilot case")
    return _observe_pdf(path, observe_installed_defaults=case_id == ORIGINAL_CASE)


def _observe_pdf(path: Path, *, observe_installed_defaults: bool = False) -> dict:
    """Share bounded PDF extraction with the separate fixed note-numbering probe."""
    pdf_module = _analysis_runtime()
    payload = word._read_regular_file(path, word.MAX_PDF_BYTES)
    pages, programs = [], {}
    with pdf_module.open(stream=payload, filetype="pdf") as pdf:
        if not 1 <= pdf.page_count <= 16:
            raise ValueError("pilot PDF page count is outside the contract")
        for page in pdf:
            if not (0 < page.rect.width <= 2000 and 0 < page.rect.height <= 2000):
                raise ValueError("pilot PDF page size is outside the contract")
            fonts = []
            for font in page.get_fonts(full=False):
                raw_name = word._safe_text(font[3], "PDF font name")
                name = (
                    re.sub(r"^[A-Z]{6}\+", "", raw_name)
                    if observe_installed_defaults
                    else word.normalize_pdf_font_name(raw_name)
                )
                if font[0] <= 0:
                    raise ValueError("pilot PDF font is not embedded")
                extracted = pdf.extract_font(font[0])
                if not extracted[3]:
                    raise ValueError("pilot PDF font is not embedded")
                if not observe_installed_defaults and name != "NotoSans-Regular":
                    raise ValueError("explicit-font pilot used an unexpected font")
                fonts.append(name)
                programs[(name, digest(extracted[3]))] = len(extracted[3])
            if not fonts:
                raise ValueError("pilot PDF page has no embedded fonts")
            spans = []
            for block in page.get_text("dict", flags=0)["blocks"]:
                for line in block.get("lines", []):
                    for span in line["spans"]:
                        if len(spans) >= 50000 or len(span["text"]) > 10000:
                            raise ValueError("pilot PDF text exceeds the contract")
                        spans.append(
                            {
                                "text": span["text"],
                                "font": span["font"],
                                "size_millipoints": word.topology._millipoints(
                                    span["size"]
                                ),
                                "origin_millipoints": [
                                    word.topology._millipoints(v)
                                    for v in span["origin"]
                                ],
                                "bbox_millipoints": [
                                    word.topology._millipoints(v) for v in span["bbox"]
                                ],
                            }
                        )
            if not spans:
                raise ValueError("pilot PDF page has no text")
            raster = page.get_pixmap(
                dpi=RASTER_DPI, colorspace=pdf_module.csRGB, alpha=False
            )
            pages.append(
                {
                    "width_millipoints": word.topology._millipoints(page.rect.width),
                    "height_millipoints": word.topology._millipoints(page.rect.height),
                    "fonts": sorted(set(fonts)),
                    "spans": spans,
                    "raster": {
                        "dpi": RASTER_DPI,
                        "width": raster.width,
                        "height": raster.height,
                        "sha256": digest(raster.samples),
                    },
                }
            )
    result = {
        "pdf": {"bytes": len(payload), "sha256": digest(payload), "pages": len(pages)},
        "font_programs": [
            {"pdf_font_name": name, "bytes": size, "sha256": sha}
            for (name, sha), size in sorted(programs.items())
        ],
        "pages": pages,
    }
    word._assert_path_neutral(result)
    return result


def compare_observations(candidate: dict, reference: dict) -> dict:
    """Require exact normalized observations while retaining variable PDF bytes."""
    if set(candidate) != set(INPUTS) or set(reference) != set(INPUTS):
        raise ValueError("repeatability requires the exact pilot profile")
    for case_id in INPUTS:
        if candidate[case_id]["pages"] != reference[case_id]["pages"]:
            raise ValueError(f"pilot repeatability failed: {case_id}")
    return {"documents": len(INPUTS), "exact_documents": len(INPUTS), "passed": True}


def harness_identity() -> dict:
    paths = [
        SCRIPT_PATH,
        word.SCRIPT_PATH,
        word.BACKEND_PATH,
        word.topology.SCRIPT_PATH,
        Path(generator.__file__),
        pilot.SCRIPT_PATH,
        Path(pilot.batch.__file__),
        SCRIPT_PATH.with_name("render_oracle_contract.py"),
    ]
    return word._harness_identity() | {
        path.name: digest(path.read_bytes()) for path in paths
    }


def _analysis_runtime():
    module = word.topology.pymupdf
    if module is None or module.__version__ != PYMUPDF_VERSION:
        raise ValueError(f"PyMuPDF {PYMUPDF_VERSION} is required")
    return module


def _run_backend(job: Path, powershell: str, timeout_seconds: int) -> None:
    word._run_backend(
        powershell=powershell,
        job_path=job,
        timeout_seconds=timeout_seconds,
        profile=PROFILE,
    )


def _reconstruct_run(output: Path, run_id: str) -> dict:
    run = output / run_id
    metadata = load_metadata(run, run_id)
    observations = {
        case_id: observe_pdf(run / "pdf" / f"{case_id}.pdf", case_id)
        for case_id in INPUTS
    }
    return {
        "run_id": run_id,
        "metadata_sha256": digest((run / "metadata.json").read_bytes()),
        "producer": metadata["producer"],
        "observations": observations,
    }


def _bundle(output: Path, revision: str, runs: list[dict]) -> dict:
    if runs[0]["producer"] != runs[1]["producer"]:
        raise ValueError("Word pilot producer changed between runs")
    return {
        "schema": "rwml.word-pilot-capture.v1",
        "profile": PROFILE,
        "source_revision": revision,
        "harness": harness_identity(),
        "analysis": {
            "python": platform.python_version(),
            "pymupdf": PYMUPDF_VERSION,
            "platform": word.topology._current_platform(),
        },
        "inputs": input_manifest(),
        "font": word.load_word_font_lock(),
        "export": copy.deepcopy(word.WORD_EXPORT_OPTIONS),
        "runs": runs,
        "repeatability": compare_observations(
            runs[1]["observations"], runs[0]["observations"]
        ),
    }


def verify(output: Path) -> dict:
    """Independently reconstruct all retained PDFs and bind every capture field."""
    _analysis_runtime()
    bundle = word._load_json(output / "CAPTURE.json")
    revision = word._git_revision(bundle["source_revision"])
    if word._git_dirty():
        raise ValueError("Word pilot verification requires a clean source tree")
    validate_input_directory(output / "corpus")
    runs = [_reconstruct_run(output, run_id) for run_id in ("run-a", "run-b")]
    for run in runs:
        if word._load_json(output / run["run_id"] / "observations.json") != run:
            raise ValueError(
                "retained pilot observations differ from PDF reconstruction"
            )
    expected = _bundle(output, revision, runs)
    if bundle != expected:
        raise ValueError("pilot capture receipt differs from reconstructed evidence")
    return bundle


def capture(
    output: Path, font: Path, powershell: str, revision: str | None, timeout: int
) -> dict:
    """Run two fresh native Word exports and write a receipt only after verification."""
    if os.name != "nt" or platform.system() != "Windows":
        raise ValueError("Word pilot capture requires native Windows")
    if output.exists() or output.is_symlink():
        raise ValueError("Word pilot output must be fresh")
    if not 60 <= timeout <= 7200:
        raise ValueError("timeout must be between 60 and 7200 seconds")
    _analysis_runtime()
    executable = shutil.which(powershell)
    if executable is None:
        raise ValueError("PowerShell executable is unavailable")
    revision = word._git_revision(revision)
    if word._git_dirty():
        raise ValueError("Word pilot capture requires a clean source tree")
    lock = word.load_word_font_lock()
    word._verify_font_file(font, lock)
    word._validate_windows_font_location(font)
    materialize(output / "corpus")
    runs = []
    for run_id in ("run-a", "run-b"):
        run = output / run_id
        run.mkdir()
        job = run / "job.json"
        word._write_json(job, build_job(output / "corpus", run, font, run_id))
        _run_backend(job, executable, timeout)
        evidence = _reconstruct_run(output, run_id)
        word._write_json(run / "observations.json", evidence)
        job.unlink()
        runs.append(evidence)
    bundle = _bundle(output, revision, runs)
    word._assert_path_neutral(bundle)
    # Re-extract before publishing the successful receipt.
    if runs != [_reconstruct_run(output, run_id) for run_id in ("run-a", "run-b")]:
        raise ValueError("pilot observations changed during independent reconstruction")
    validate_input_directory(output / "corpus")
    word._verify_font_file(font, lock)
    word._git_revision(revision)
    if word._git_dirty():
        raise ValueError("source tree changed during Word pilot capture")
    word._write_json(output / "CAPTURE.json", bundle)
    return bundle


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    generate_parser = commands.add_parser("generate")
    generate_parser.add_argument("--output", type=Path, required=True)
    verify_parser = commands.add_parser("verify")
    verify_parser.add_argument("--output", type=Path, required=True)
    capture_parser = commands.add_parser("capture")
    capture_parser.add_argument("--output", type=Path, required=True)
    capture_parser.add_argument("--font", type=Path, required=True)
    capture_parser.add_argument("--powershell", default="powershell.exe")
    capture_parser.add_argument("--source-revision")
    capture_parser.add_argument("--timeout-seconds", type=int, default=600)
    args = parser.parse_args(argv)
    try:
        if args.command == "generate":
            materialize(args.output)
            print("generated three locked Word pagination inputs")
        else:
            bundle = (
                verify(args.output)
                if args.command == "verify"
                else capture(
                    args.output,
                    args.font,
                    args.powershell,
                    args.source_revision,
                    args.timeout_seconds,
                )
            )
            print(
                f"verified Word pagination evidence: {bundle['repeatability']['exact_documents']}/3 exact"
            )
        return 0
    except (OSError, ValueError) as error:
        print(f"word_pilot_capture: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
