#!/usr/bin/env python3
"""Capture strict, repeatable Microsoft Word evidence for render diagnostics.

The PowerShell transport consumes absolute local paths, but all retained metadata is
path-neutral. A capture is diagnostic evidence only: this tool does not establish a
release threshold or silently substitute another producer for Microsoft Word.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import os
import platform
import re
import shutil
import stat
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath
from typing import Any

try:
    from generate_unequal_table_oracle import CAMPAIGN, CASES, materialize
    from render_oracle_contract import CorpusManifest, load_corpus_manifest
    from table_oracle_topology import (
        build_capture_report,
        compare_capture_reports,
        validate_capture_report,
    )
    import table_oracle_topology as topology
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.generate_unequal_table_oracle import CAMPAIGN, CASES, materialize
    from scripts.render_oracle_contract import CorpusManifest, load_corpus_manifest
    from scripts.table_oracle_topology import (
        build_capture_report,
        compare_capture_reports,
        validate_capture_report,
    )
    from scripts import table_oracle_topology as topology


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
BACKEND_PATH = SCRIPT_PATH.with_name("word_oracle_export.ps1")
DEFAULT_FONT_LOCK = ROOT / "corpus" / "public" / "oracle" / "word-font-lock.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "word-unequal-table-v1"

FONT_LOCK_SCHEMA = "rwml.word-oracle-font-lock.v1"
EXPORT_JOB_SCHEMA = "rwml.word-export-job.v1"
EXPORT_METADATA_SCHEMA = "rwml.word-export-metadata.v1"
CAPTURE_BUNDLE_SCHEMA = "rwml.word-oracle-capture-bundle.v1"
MAX_JSON_BYTES = 8 * 1024 * 1024
MAX_JSON_DEPTH = 64
MAX_JSON_NODES = 500_000
MAX_PDF_BYTES = 64 * 1024 * 1024
MAX_FONT_BYTES = 16 * 1024 * 1024
EXPECTED_FONT_BYTES = 825_628
EXPECTED_FONT_SHA256 = (
    "f5f552c8c5edb61fe6efb824baf4d4de47b1a8689ab4925ff43f7bd6a4ebece5"
)

SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
RUN_ID_RE = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
FONT_NAME_RE = re.compile(r"[A-Za-z0-9_.-]+\Z")
SUBSET_FONT_RE = re.compile(r"[A-Z]{6}\+(?P<name>[A-Za-z0-9_.-]+)\Z")
REVISION_RE = re.compile(r"[0-9a-f]{40}\Z")
LOCAL_PATH_PATTERNS = (
    re.compile(r"(?<![A-Za-z]:)/Users/[A-Za-z0-9._-]+/"),
    re.compile(r"/home/[A-Za-z0-9._-]+/"),
    re.compile(r"[A-Za-z]:[/\\]Users[/\\][^/\\\s]+[/\\]"),
    re.compile(r"(?<!\\)\\\\[A-Za-z0-9._-]{2,}\\[^\\\s]{2,}\\"),
)

FONT_LOCK_KEYS = {
    "schema",
    "family",
    "postscript_name",
    "style",
    "license",
    "file",
    "source",
}
FONT_FILE_KEYS = {"name", "bytes", "sha256"}
FONT_SOURCE_KEYS = {"repository", "release_tag", "target_commit", "asset"}
FONT_SOURCE_ASSET_KEYS = {"name", "bytes", "sha256", "member"}
JOB_KEYS = {
    "schema",
    "run_id",
    "output_directory",
    "metadata_path",
    "font",
    "export",
    "documents",
}
JOB_FONT_KEYS = {
    "path",
    "family",
    "postscript_name",
    "bytes",
    "sha256",
}
JOB_DOCUMENT_KEYS = {
    "case_id",
    "input",
    "output",
    "input_bytes",
    "input_sha256",
}
METADATA_KEYS = {
    "schema",
    "run_id",
    "producer",
    "runtime",
    "font",
    "export",
    "documents",
}
PRODUCER_KEYS = {"name", "mode", "version", "identity_sha256", "platform"}
PLATFORM_KEYS = {"system", "release", "machine"}
RUNTIME_KEYS = {
    "application",
    "version",
    "build",
    "executable_sha256",
    "os_version",
    "os_build",
    "machine",
    "powershell_version",
}
METADATA_FONT_KEYS = {
    "family",
    "postscript_name",
    "bytes",
    "sha256",
    "installed_font_directory",
}
METADATA_DOCUMENT_KEYS = {"case_id", "pdf_bytes", "pdf_sha256"}
RUNTIME_IDENTITY_ORDER = (
    "application",
    "version",
    "build",
    "executable_sha256",
    "os_version",
    "os_build",
    "machine",
    "powershell_version",
)

WORD_EXPORT_OPTIONS: dict[str, object] = {
    "bitmap_missing_fonts": False,
    "bookmarks": "none",
    "document_structure_tags": True,
    "format": "pdf",
    "include_document_properties": True,
    "item": "document-content",
    "keep_irm": True,
    "optimize_for": "print",
    "pdfa": False,
    "range": "all-document",
}


def _canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        raise ValueError(f"{label} keys differ: missing={missing}, extra={extra}")


def _safe_text(value: object, label: str, *, maximum: int = 256) -> str:
    if not isinstance(value, str) or not value or len(value) > maximum:
        raise ValueError(f"{label} must be a non-empty bounded string")
    if any(ord(character) < 0x20 for character in value):
        raise ValueError(f"{label} contains control characters")
    return value


def _sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise ValueError(f"{label} must be a lowercase SHA-256")
    return value


def _positive_int(value: object, label: str, *, maximum: int) -> int:
    if isinstance(value, bool) or not isinstance(value, int):
        raise ValueError(f"{label} must be an integer")
    if value <= 0 or value > maximum:
        raise ValueError(f"{label} is outside the contract")
    return value


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value: str) -> object:
    raise ValueError(f"non-finite JSON number: {value}")


def _validate_json_complexity(value: object) -> None:
    nodes = 0
    pending: list[tuple[object, int]] = [(value, 1)]
    while pending:
        current, depth = pending.pop()
        nodes += 1
        if nodes > MAX_JSON_NODES:
            raise ValueError("JSON node limit exceeded")
        if depth > MAX_JSON_DEPTH:
            raise ValueError("JSON depth limit exceeded")
        if isinstance(current, dict):
            pending.extend((item, depth + 1) for item in current.values())
        elif isinstance(current, list):
            pending.extend((item, depth + 1) for item in current)


def _read_regular_file(path: Path, maximum: int) -> bytes:
    if path.is_symlink():
        raise ValueError(f"{path.name} must not be a symlink")
    descriptor: int | None = None
    try:
        descriptor = os.open(
            path,
            os.O_RDONLY
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_NONBLOCK", 0),
        )
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"{path.name} is not a regular file")
        if before.st_size <= 0 or before.st_size > maximum:
            raise ValueError(f"{path.name} size is outside the contract")
        chunks: list[bytes] = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        after = os.fstat(descriptor)
    except ValueError:
        raise
    except OSError as error:
        raise ValueError(f"{path.name} is unreadable") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
    payload = b"".join(chunks)
    if (
        len(payload) != before.st_size
        or before.st_dev != after.st_dev
        or before.st_ino != after.st_ino
        or before.st_size != after.st_size
        or before.st_mtime_ns != after.st_mtime_ns
    ):
        raise ValueError(f"{path.name} changed while being read")
    return payload


def _load_json(path: Path, maximum: int = MAX_JSON_BYTES) -> dict[str, Any]:
    payload = _read_regular_file(path, maximum)
    try:
        value = json.loads(
            payload,
            object_pairs_hook=_reject_duplicate_pairs,
            parse_constant=_reject_constant,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{path.name} is not strict UTF-8 JSON") from error
    _validate_json_complexity(value)
    if not isinstance(value, dict):
        raise ValueError(f"{path.name} must contain a JSON object")
    return value


def _write_json(path: Path, value: object) -> None:
    payload = _canonical_json(value)
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", dir=path.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def _assert_path_neutral(value: object) -> None:
    rendered = json.dumps(value, ensure_ascii=True, sort_keys=True)
    for pattern in LOCAL_PATH_PATTERNS:
        if pattern.search(rendered):
            raise ValueError("retained metadata contains a local path")


def _validate_word_font_lock_value(value: dict[str, Any]) -> None:
    _require_exact_keys(value, FONT_LOCK_KEYS, "font lock")
    if value["schema"] != FONT_LOCK_SCHEMA:
        raise ValueError(f"font lock schema must be {FONT_LOCK_SCHEMA}")
    for key in ("family", "postscript_name", "style", "license"):
        _safe_text(value[key], f"font lock {key}")
    if value["postscript_name"] != "NotoSans-Regular":
        raise ValueError("font lock must select NotoSans-Regular")
    if value["license"] != "SIL-OFL-1.1":
        raise ValueError("font lock license must be SIL-OFL-1.1")

    file_value = value["file"]
    if not isinstance(file_value, dict):
        raise ValueError("font lock file must be an object")
    _require_exact_keys(file_value, FONT_FILE_KEYS, "font lock file")
    if file_value["name"] != "NotoSans-Regular.ttf":
        raise ValueError("font lock filename is invalid")
    _positive_int(file_value["bytes"], "font bytes", maximum=MAX_FONT_BYTES)
    _sha256(file_value["sha256"], "font SHA-256")
    if (
        file_value["bytes"] != EXPECTED_FONT_BYTES
        or file_value["sha256"] != EXPECTED_FONT_SHA256
    ):
        raise ValueError("font lock file identity is invalid")

    source = value["source"]
    if not isinstance(source, dict):
        raise ValueError("font lock source must be an object")
    _require_exact_keys(source, FONT_SOURCE_KEYS, "font lock source")
    if source["repository"] != "notofonts/latin-greek-cyrillic":
        raise ValueError("font source repository is invalid")
    if source["release_tag"] != "NotoSans-v2.015":
        raise ValueError("font source release tag is invalid")
    if source["target_commit"] != "c4a321e123e4d4ff315f57f4e0adf294fe3a95be":
        raise ValueError("font source target commit is invalid")
    asset = source["asset"]
    if not isinstance(asset, dict):
        raise ValueError("font source asset must be an object")
    _require_exact_keys(asset, FONT_SOURCE_ASSET_KEYS, "font source asset")
    if (
        asset["name"] != "NotoSans-v2.015.zip"
        or asset["bytes"] != 117491253
        or asset["sha256"]
        != "0c34df072a3fa7efbb7cbf34950e1f971a4447cffe365d3a359e2d4089b958f5"
        or asset["member"] != "NotoSans/full/ttf/NotoSans-Regular.ttf"
    ):
        raise ValueError("font source asset identity is invalid")
    if not isinstance(asset["name"], str):
        raise ValueError("font source asset name is invalid")
    _positive_int(asset["bytes"], "font source asset bytes", maximum=256 * 1024 * 1024)
    _sha256(asset["sha256"], "font source asset SHA-256")
    member = _safe_text(asset["member"], "font source asset member", maximum=512)
    member_path = PurePosixPath(member)
    if (
        member_path.is_absolute()
        or ".." in member_path.parts
        or member_path.name != file_value["name"]
    ):
        raise ValueError("font source asset member must be a safe relative path")
    _assert_path_neutral(value)


def load_word_font_lock(path: Path = DEFAULT_FONT_LOCK) -> dict[str, Any]:
    value = _load_json(path, 64 * 1024)
    _validate_word_font_lock_value(value)
    return value


def _validate_campaign(corpus: CorpusManifest) -> None:
    if corpus.campaign != CAMPAIGN or len(corpus.documents) != len(CASES):
        raise ValueError("manifest is not the unequal-table diagnostic campaign")
    for document, case in zip(corpus.documents, CASES, strict=True):
        if document.case_id != case.case_id:
            raise ValueError("manifest case order does not match the generator")


def _absolute_path(path: Path, label: str) -> str:
    resolved = path.resolve(strict=False)
    if not resolved.is_absolute():
        raise ValueError(f"{label} must resolve to an absolute path")
    return str(resolved)


def build_export_job(
    corpus: CorpusManifest,
    run_directory: Path,
    font_path: Path,
    font_lock: dict[str, Any],
    *,
    run_id: str,
) -> dict[str, Any]:
    _validate_campaign(corpus)
    if RUN_ID_RE.fullmatch(run_id) is None:
        raise ValueError("run ID is not canonical")
    if run_directory.is_symlink() or not run_directory.is_dir():
        raise ValueError("run directory is unavailable or symlinked")
    if not isinstance(font_lock, dict):
        raise ValueError("font lock must be an object")
    _validate_word_font_lock_value(font_lock)
    pdf_directory = run_directory / "pdf"
    metadata_path = run_directory / "metadata.json"
    documents = [
        {
            "case_id": document.case_id,
            "input": _absolute_path(document.path, "document input"),
            "output": _absolute_path(
                pdf_directory / f"{document.case_id}.pdf", "PDF output"
            ),
            "input_bytes": document.input_bytes,
            "input_sha256": document.sha256,
        }
        for document in corpus.documents
    ]
    job = {
        "schema": EXPORT_JOB_SCHEMA,
        "run_id": run_id,
        "output_directory": _absolute_path(pdf_directory, "PDF directory"),
        "metadata_path": _absolute_path(metadata_path, "metadata path"),
        "font": {
            "path": _absolute_path(font_path, "font path"),
            "family": font_lock["family"],
            "postscript_name": font_lock["postscript_name"],
            "bytes": font_lock["file"]["bytes"],
            "sha256": font_lock["file"]["sha256"],
        },
        "export": copy.deepcopy(WORD_EXPORT_OPTIONS),
        "documents": documents,
    }
    validate_export_job(
        job,
        corpus,
        run_directory,
        font_path,
        font_lock,
        expected_run_id=run_id,
    )
    return job


def validate_export_job(
    job: dict[str, Any],
    corpus: CorpusManifest,
    run_directory: Path,
    font_path: Path,
    font_lock: dict[str, Any],
    *,
    expected_run_id: str,
) -> None:
    _validate_campaign(corpus)
    _validate_word_font_lock_value(font_lock)
    if run_directory.is_symlink() or not run_directory.is_dir():
        raise ValueError("run directory is unavailable or symlinked")
    _require_exact_keys(job, JOB_KEYS, "Word export job")
    if job["schema"] != EXPORT_JOB_SCHEMA:
        raise ValueError(f"Word export job schema must be {EXPORT_JOB_SCHEMA}")
    if job["run_id"] != expected_run_id or RUN_ID_RE.fullmatch(expected_run_id) is None:
        raise ValueError("Word export job run ID is invalid")
    expected_pdf_directory = _absolute_path(run_directory / "pdf", "PDF directory")
    expected_metadata_path = _absolute_path(
        run_directory / "metadata.json", "metadata path"
    )
    if job["output_directory"] != expected_pdf_directory:
        raise ValueError("Word export job output directory is invalid")
    if job["metadata_path"] != expected_metadata_path:
        raise ValueError("Word export job metadata path is invalid")

    font = job["font"]
    if not isinstance(font, dict):
        raise ValueError("Word export job font must be an object")
    _require_exact_keys(font, JOB_FONT_KEYS, "Word export job font")
    expected_font = {
        "path": _absolute_path(font_path, "font path"),
        "family": font_lock["family"],
        "postscript_name": font_lock["postscript_name"],
        "bytes": font_lock["file"]["bytes"],
        "sha256": font_lock["file"]["sha256"],
    }
    if font != expected_font:
        raise ValueError("Word export job font identity is invalid")
    if job["export"] != WORD_EXPORT_OPTIONS:
        raise ValueError("Word export job options differ from the fixed contract")

    documents = job["documents"]
    if not isinstance(documents, list) or len(documents) != len(corpus.documents):
        raise ValueError("Word export job document count is invalid")
    for row, document in zip(documents, corpus.documents, strict=True):
        if not isinstance(row, dict):
            raise ValueError("Word export job document must be an object")
        _require_exact_keys(row, JOB_DOCUMENT_KEYS, "Word export job document")
        expected = {
            "case_id": document.case_id,
            "input": _absolute_path(document.path, "document input"),
            "output": _absolute_path(
                run_directory / "pdf" / f"{document.case_id}.pdf",
                "PDF output",
            ),
            "input_bytes": document.input_bytes,
            "input_sha256": document.sha256,
        }
        if row != expected:
            raise ValueError(f"Word export job identity differs for {document.case_id}")


def word_producer_identity(runtime: dict[str, Any]) -> str:
    if not isinstance(runtime, dict):
        raise ValueError("Word runtime must be an object")
    _require_exact_keys(runtime, RUNTIME_KEYS, "Word runtime")
    rows: list[str] = []
    for key in RUNTIME_IDENTITY_ORDER:
        value = _safe_text(runtime[key], f"Word runtime {key}")
        if key == "executable_sha256":
            _sha256(value, "Word executable SHA-256")
        rows.append(f"{key}={value}")
    return hashlib.sha256("\n".join(rows).encode("utf-8")).hexdigest()


def _validate_producer(producer: object, runtime: dict[str, Any]) -> dict[str, Any]:
    if not isinstance(producer, dict):
        raise ValueError("producer must be an object")
    _require_exact_keys(producer, PRODUCER_KEYS, "producer")
    if producer["name"] != "microsoft-word" or producer["mode"] != "windows-com":
        raise ValueError("producer must be Microsoft Word through Windows COM")
    if runtime["application"] != "Microsoft Word":
        raise ValueError("Word runtime application is invalid")
    expected_identity = word_producer_identity(runtime)
    if producer["identity_sha256"] != expected_identity:
        raise ValueError("producer identity does not match the Word runtime")
    expected_version = (
        f"Microsoft Word {runtime['version']} build {runtime['build']}"
    )
    if producer["version"] != expected_version:
        raise ValueError("producer version does not match the Word runtime")
    platform_value = producer["platform"]
    if not isinstance(platform_value, dict):
        raise ValueError("producer platform must be an object")
    _require_exact_keys(platform_value, PLATFORM_KEYS, "producer platform")
    expected_platform = {
        "system": "Windows",
        "release": f"{runtime['os_version']} build {runtime['os_build']}",
        "machine": runtime["machine"],
    }
    if platform_value != expected_platform:
        raise ValueError("producer platform does not match the Word runtime")
    return copy.deepcopy(producer)


def _validate_metadata_font(value: object, lock: dict[str, Any]) -> None:
    if not isinstance(value, dict):
        raise ValueError("metadata font must be an object")
    _require_exact_keys(value, METADATA_FONT_KEYS, "metadata font")
    expected = {
        "family": lock["family"],
        "postscript_name": lock["postscript_name"],
        "bytes": lock["file"]["bytes"],
        "sha256": lock["file"]["sha256"],
        "installed_font_directory": True,
    }
    if value != expected:
        raise ValueError("metadata font does not match the checked-in lock")


def _directory_entries(directory: Path) -> set[str]:
    try:
        return {entry.name for entry in directory.iterdir()}
    except OSError as error:
        raise ValueError("PDF directory is unreadable") from error


def validate_export_metadata(
    metadata: dict[str, Any],
    corpus: CorpusManifest,
    pdf_directory: Path,
    font_lock: dict[str, Any],
    *,
    expected_run_id: str,
) -> dict[str, Any]:
    _validate_campaign(corpus)
    if not isinstance(metadata, dict):
        raise ValueError("export metadata must be an object")
    _require_exact_keys(metadata, METADATA_KEYS, "export metadata")
    if metadata["schema"] != EXPORT_METADATA_SCHEMA:
        raise ValueError(f"export metadata schema must be {EXPORT_METADATA_SCHEMA}")
    if metadata["run_id"] != expected_run_id:
        raise ValueError("export metadata run ID does not match")
    if RUN_ID_RE.fullmatch(expected_run_id) is None:
        raise ValueError("expected run ID is not canonical")

    runtime = metadata["runtime"]
    if not isinstance(runtime, dict):
        raise ValueError("Word runtime must be an object")
    producer = _validate_producer(metadata["producer"], runtime)
    _validate_metadata_font(metadata["font"], font_lock)
    if metadata["export"] != WORD_EXPORT_OPTIONS:
        raise ValueError("Word export options do not match the fixed contract")

    if pdf_directory.is_symlink() or not pdf_directory.is_dir():
        raise ValueError("PDF directory is unavailable or symlinked")
    expected_names = {
        f"{document.case_id}.pdf" for document in corpus.documents
    }
    if _directory_entries(pdf_directory) != expected_names:
        raise ValueError("PDF directory does not contain the exact campaign set")

    documents = metadata["documents"]
    if not isinstance(documents, list) or len(documents) != len(corpus.documents):
        raise ValueError("export metadata document count is invalid")
    observed_ids: set[str] = set()
    for row, expected in zip(documents, corpus.documents, strict=True):
        if not isinstance(row, dict):
            raise ValueError("export metadata document row must be an object")
        _require_exact_keys(row, METADATA_DOCUMENT_KEYS, "export metadata document")
        if row["case_id"] != expected.case_id or row["case_id"] in observed_ids:
            raise ValueError("export metadata document order or identity is invalid")
        observed_ids.add(row["case_id"])
        payload = _read_regular_file(
            pdf_directory / f"{expected.case_id}.pdf", MAX_PDF_BYTES
        )
        expected_bytes = _positive_int(
            row["pdf_bytes"], "PDF byte length", maximum=MAX_PDF_BYTES
        )
        expected_sha = _sha256(row["pdf_sha256"], "PDF SHA-256")
        if len(payload) != expected_bytes or hashlib.sha256(payload).hexdigest() != expected_sha:
            raise ValueError(f"PDF identity mismatch for {expected.case_id}")
    _assert_path_neutral(metadata)
    return producer


def load_export_metadata(
    path: Path,
    corpus: CorpusManifest,
    pdf_directory: Path,
    font_lock: dict[str, Any],
    *,
    expected_run_id: str,
) -> tuple[dict[str, Any], dict[str, Any]]:
    metadata = _load_json(path)
    producer = validate_export_metadata(
        metadata,
        corpus,
        pdf_directory,
        font_lock,
        expected_run_id=expected_run_id,
    )
    return metadata, producer


def normalize_pdf_font_name(name: str) -> str:
    if not isinstance(name, str):
        raise ValueError("PDF font name is not canonical")
    subset = SUBSET_FONT_RE.fullmatch(name)
    if subset:
        return subset.group("name")
    if FONT_NAME_RE.fullmatch(name) is None:
        raise ValueError("PDF font name is not canonical")
    return name


def validate_pdf_fonts(
    pdf_directory: Path,
    corpus: CorpusManifest,
    expected_postscript_name: str,
) -> dict[str, Any]:
    if topology.pymupdf is None:
        raise ValueError("PyMuPDF is required to validate embedded Word fonts")
    if FONT_NAME_RE.fullmatch(expected_postscript_name) is None:
        raise ValueError("expected PDF font name is invalid")
    _validate_campaign(corpus)
    page_count = 0
    font_uses = 0
    for document in corpus.documents:
        path = pdf_directory / f"{document.case_id}.pdf"
        _read_regular_file(path, MAX_PDF_BYTES)
        try:
            pdf = topology.pymupdf.open(path)
        except Exception as error:
            raise ValueError(f"{document.case_id}: PDF cannot be opened") from error
        try:
            if pdf.page_count <= 0 or pdf.page_count > topology.MAX_PAGES:
                raise ValueError(f"{document.case_id}: PDF page count is invalid")
            for page in pdf:
                page_count += 1
                page_fonts = page.get_fonts(full=False)
                if not page_fonts:
                    raise ValueError(f"{document.case_id}: PDF page has no fonts")
                for font in page_fonts:
                    if len(font) < 4 or not isinstance(font[3], str):
                        raise ValueError(f"{document.case_id}: malformed PDF font record")
                    xref = font[0]
                    if isinstance(xref, bool) or not isinstance(xref, int) or xref <= 0:
                        raise ValueError(f"{document.case_id}: PDF font is not embedded")
                    normalized = normalize_pdf_font_name(font[3])
                    if normalized != expected_postscript_name:
                        raise ValueError(
                            f"{document.case_id}: unexpected embedded PDF font {normalized}"
                        )
                    extracted = pdf.extract_font(xref)
                    if len(extracted) < 4 or not extracted[3]:
                        raise ValueError(f"{document.case_id}: PDF font is not embedded")
                    font_uses += 1
        finally:
            pdf.close()
    return {
        "documents": len(corpus.documents),
        "pages": page_count,
        "font_uses": font_uses,
        "postscript_name": expected_postscript_name,
    }


def _verify_font_file(path: Path, lock: dict[str, Any]) -> None:
    payload = _read_regular_file(path, MAX_FONT_BYTES)
    if (
        len(payload) != lock["file"]["bytes"]
        or hashlib.sha256(payload).hexdigest() != lock["file"]["sha256"]
    ):
        raise ValueError("installed font does not match the checked-in font lock")


def _git_revision(explicit: str | None) -> str:
    completed = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=ROOT,
        check=False,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise ValueError("source revision is unavailable")
    head = completed.stdout.strip()
    if REVISION_RE.fullmatch(head) is None:
        raise ValueError("checked-out revision is not a full lowercase Git SHA")
    if explicit is not None and explicit != head:
        raise ValueError("source revision does not match the checked-out commit")
    if explicit is not None and REVISION_RE.fullmatch(explicit) is None:
        raise ValueError("source revision must be a full lowercase Git SHA")
    return head


def _git_dirty() -> bool:
    completed = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise ValueError("source dirty state is unavailable")
    return bool(completed.stdout)


def _validate_windows_font_location(font_path: Path) -> None:
    candidates = []
    windir = os.environ.get("WINDIR")
    local_app_data = os.environ.get("LOCALAPPDATA")
    if windir:
        candidates.append((Path(windir) / "Fonts").resolve())
    if local_app_data:
        candidates.append((Path(local_app_data) / "Microsoft" / "Windows" / "Fonts").resolve())
    resolved = font_path.resolve()
    if not candidates or not any(resolved.parent == candidate for candidate in candidates):
        raise ValueError("font must be the locked file in a Windows font directory")


def _run_backend(
    *,
    powershell: str,
    job_path: Path,
    timeout_seconds: int,
) -> None:
    command = [
        powershell,
        "-NoLogo",
        "-NoProfile",
        "-NonInteractive",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        str(BACKEND_PATH),
        "-JobPath",
        str(job_path),
    ]
    try:
        completed = subprocess.run(
            command,
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout_seconds,
        )
    except subprocess.TimeoutExpired as error:
        raise ValueError("Microsoft Word export timed out") from error
    if completed.returncode != 0:
        detail = (completed.stderr or completed.stdout).strip()
        if len(detail) > 2_000:
            detail = detail[:2_000] + "..."
        raise ValueError(f"Microsoft Word export failed: {detail or 'unknown error'}")


def _run_capture(
    *,
    run_id: str,
    run_directory: Path,
    corpus: CorpusManifest,
    font_path: Path,
    font_lock: dict[str, Any],
    powershell: str,
    source_revision: str,
    timeout_seconds: int,
) -> dict[str, Any]:
    run_directory.mkdir()
    job = build_export_job(
        corpus, run_directory, font_path, font_lock, run_id=run_id
    )
    job_path = run_directory / "job.json"
    _write_json(job_path, job)
    _run_backend(
        powershell=powershell,
        job_path=job_path,
        timeout_seconds=timeout_seconds,
    )
    pdf_directory = run_directory / "pdf"
    metadata_path = run_directory / "metadata.json"
    metadata, producer = load_export_metadata(
        metadata_path,
        corpus,
        pdf_directory,
        font_lock,
        expected_run_id=run_id,
    )
    font_evidence = validate_pdf_fonts(
        pdf_directory, corpus, font_lock["postscript_name"]
    )
    capture = build_capture_report(
        corpus,
        pdf_directory,
        producer,
        source_revision=source_revision,
    )
    validate_capture_report(capture, corpus)
    capture_path = run_directory / "topology.json"
    _write_json(capture_path, capture)
    job_path.unlink()
    return {
        "run_id": run_id,
        "producer": producer,
        "metadata_file_sha256": hashlib.sha256(
            _read_regular_file(metadata_path, MAX_JSON_BYTES)
        ).hexdigest(),
        "metadata_canonical_sha256": hashlib.sha256(
            _canonical_json(metadata)
        ).hexdigest(),
        "topology_sha256": hashlib.sha256(_canonical_json(capture)).hexdigest(),
        "font_validation": font_evidence,
        "capture": capture,
    }


def _harness_identity() -> dict[str, str]:
    paths = (
        SCRIPT_PATH,
        BACKEND_PATH,
        SCRIPT_PATH.with_name("generate_unequal_table_oracle.py"),
        SCRIPT_PATH.with_name("render_oracle_contract.py"),
        SCRIPT_PATH.with_name("table_oracle_topology.py"),
    )
    return {path.name: hashlib.sha256(path.read_bytes()).hexdigest() for path in paths}


def capture_repeatable_word_oracle(
    *,
    output: Path,
    font_path: Path,
    powershell: str,
    source_revision: str | None,
    timeout_seconds: int,
) -> dict[str, Any]:
    if os.name != "nt" or platform.system() != "Windows":
        raise ValueError("authoritative Word capture requires a Windows host")
    if output.exists() or output.is_symlink():
        raise ValueError("capture output must be a fresh path")
    if timeout_seconds < 60 or timeout_seconds > 7_200:
        raise ValueError("timeout must be between 60 and 7200 seconds")
    resolved_powershell = shutil.which(powershell)
    if resolved_powershell is None:
        raise ValueError("PowerShell executable is unavailable")
    revision = _git_revision(source_revision)
    if _git_dirty():
        raise ValueError("authoritative Word capture requires a clean source tree")

    font_lock = load_word_font_lock()
    _verify_font_file(font_path, font_lock)
    _validate_windows_font_location(font_path)
    output.mkdir(parents=True)
    campaign_directory = output / "corpus"
    materialize(campaign_directory)
    corpus = load_corpus_manifest(campaign_directory / "RENDER_ORACLE.json")
    _validate_campaign(corpus)

    runs = [
        _run_capture(
            run_id=run_id,
            run_directory=output / run_id,
            corpus=corpus,
            font_path=font_path,
            font_lock=font_lock,
            powershell=resolved_powershell,
            source_revision=revision,
            timeout_seconds=timeout_seconds,
        )
        for run_id in ("run-a", "run-b")
    ]
    if runs[0]["producer"] != runs[1]["producer"]:
        raise ValueError("Word producer identity changed between repeatability runs")
    comparison = compare_capture_reports(
        runs[1]["capture"], runs[0]["capture"], corpus
    )
    exact = comparison["summary"]["normalized_exact_documents"]
    if exact != len(corpus.documents):
        raise ValueError(
            f"Word repeatability failed: {exact}/{len(corpus.documents)} documents exact"
        )
    _write_json(output / "repeatability.json", comparison)

    retained_runs = []
    for run in runs:
        retained = {key: value for key, value in run.items() if key != "capture"}
        retained_runs.append(retained)
    bundle = {
        "schema": CAPTURE_BUNDLE_SCHEMA,
        "campaign": corpus.identity(),
        "source_revision": revision,
        "font": {
            "family": font_lock["family"],
            "postscript_name": font_lock["postscript_name"],
            "bytes": font_lock["file"]["bytes"],
            "sha256": font_lock["file"]["sha256"],
        },
        "export": copy.deepcopy(WORD_EXPORT_OPTIONS),
        "harness": _harness_identity(),
        "runs": retained_runs,
        "repeatability": {
            "comparison_sha256": hashlib.sha256(
                _canonical_json(comparison)
            ).hexdigest(),
            "documents": len(corpus.documents),
            "normalized_exact_documents": exact,
            "passed": True,
        },
    }
    _assert_path_neutral(bundle)
    _write_json(output / "CAPTURE.json", bundle)
    return bundle


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Capture strict Microsoft Word evidence for the unequal-table campaign."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    capture_parser = subparsers.add_parser(
        "capture", help="run two fresh Word COM exports and require exact topology"
    )
    capture_parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    capture_parser.add_argument("--font", type=Path, required=True)
    capture_parser.add_argument("--powershell", default="powershell.exe")
    capture_parser.add_argument("--source-revision")
    capture_parser.add_argument("--timeout-seconds", type=int, default=1800)

    validate_parser = subparsers.add_parser(
        "validate-metadata", help="validate one completed Word export"
    )
    validate_parser.add_argument("--manifest", type=Path, required=True)
    validate_parser.add_argument("--metadata", type=Path, required=True)
    validate_parser.add_argument("--pdf-dir", type=Path, required=True)
    validate_parser.add_argument("--run-id", required=True)
    validate_parser.add_argument("--font-lock", type=Path, default=DEFAULT_FONT_LOCK)

    args = parser.parse_args(argv)
    try:
        if args.command == "capture":
            bundle = capture_repeatable_word_oracle(
                output=args.output,
                font_path=args.font,
                powershell=args.powershell,
                source_revision=args.source_revision,
                timeout_seconds=args.timeout_seconds,
            )
            print(
                "captured repeatable Microsoft Word evidence: "
                f"{bundle['repeatability']['documents']} documents"
            )
            return 0
        corpus = load_corpus_manifest(args.manifest)
        lock = load_word_font_lock(args.font_lock)
        _, producer = load_export_metadata(
            args.metadata,
            corpus,
            args.pdf_dir,
            lock,
            expected_run_id=args.run_id,
        )
        validate_pdf_fonts(args.pdf_dir, corpus, lock["postscript_name"])
        print(
            "validated Microsoft Word export metadata for "
            f"{producer['version']}"
        )
        return 0
    except (OSError, ValueError) as error:
        print(f"word_oracle_capture: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
