#!/usr/bin/env python3
"""Strict, path-neutral contracts for public render-oracle evidence."""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
import re
import stat
import sys
from dataclasses import dataclass
from pathlib import Path, PurePosixPath
from typing import Any

try:
    from render_evidence_metrics import (
        aggregate_metrics as aggregate_integer_metrics,
        validate_metric_contract,
        validate_metrics as validate_integer_metrics,
    )
    from render_pdf_diagnostics import (
        aggregate_geometry_reports as aggregate_pdf_geometry_reports,
        aggregate_semantic_reports as aggregate_pdf_semantic_reports,
        aggregate_text_geometry_reports as aggregate_pdf_text_geometry_reports,
        validate_diagnostic_contract,
        validate_geometry_report as validate_pdf_geometry_report,
        validate_geometry_summary as validate_pdf_geometry_summary,
        validate_semantic_report as validate_pdf_semantic_report,
        validate_text_geometry_report as validate_pdf_text_geometry_report,
        validate_text_geometry_summary as validate_pdf_text_geometry_summary,
    )
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.render_evidence_metrics import (
        aggregate_metrics as aggregate_integer_metrics,
        validate_metric_contract,
        validate_metrics as validate_integer_metrics,
    )
    from scripts.render_pdf_diagnostics import (
        aggregate_geometry_reports as aggregate_pdf_geometry_reports,
        aggregate_semantic_reports as aggregate_pdf_semantic_reports,
        aggregate_text_geometry_reports as aggregate_pdf_text_geometry_reports,
        validate_diagnostic_contract,
        validate_geometry_report as validate_pdf_geometry_report,
        validate_geometry_summary as validate_pdf_geometry_summary,
        validate_semantic_report as validate_pdf_semantic_report,
        validate_text_geometry_report as validate_pdf_text_geometry_report,
        validate_text_geometry_summary as validate_pdf_text_geometry_summary,
    )


CORPUS_SCHEMA = "rwml.render-oracle-corpus.v1"
EVIDENCE_SCHEMA = "rwml.render-oracle-evidence.v4"
LEGACY_CAPTURE_EVIDENCE_SCHEMA = "rwml.render-oracle-evidence.v5"
ANALYSIS_CAPTURE_EVIDENCE_SCHEMA = "rwml.render-oracle-evidence.v6"
CAPTURE_EVIDENCE_SCHEMA = "rwml.render-oracle-evidence.v7"
LEGACY_CAMPAIGN_CAPTURE_SCHEMA = "rwml.render-campaign-capture.v1"
ANALYSIS_CAMPAIGN_CAPTURE_SCHEMA = "rwml.render-campaign-capture.v2"
CAMPAIGN_CAPTURE_SCHEMA = "rwml.render-campaign-capture.v3"
CAPTURE_SCHEMAS = {
    LEGACY_CAPTURE_EVIDENCE_SCHEMA: LEGACY_CAMPAIGN_CAPTURE_SCHEMA,
    ANALYSIS_CAPTURE_EVIDENCE_SCHEMA: ANALYSIS_CAMPAIGN_CAPTURE_SCHEMA,
    CAPTURE_EVIDENCE_SCHEMA: CAMPAIGN_CAPTURE_SCHEMA,
}
MAX_MANIFEST_BYTES = 4 * 1024 * 1024
MAX_EVIDENCE_BYTES = 64 * 1024 * 1024
MAX_JSON_DEPTH = 64
MAX_JSON_NODES = 2_000_000
MAX_JSON_INTEGER_DIGITS = 128
MAX_DOCUMENTS = 10_000
MAX_INPUT_BYTES = 256 * 1024 * 1024
MAX_TOTAL_INPUT_BYTES = 4 * 1024 * 1024 * 1024
MAX_PAGES_PER_DOCUMENT = 4_096

CORPUS_KEYS = {"schema", "campaign", "limits", "provenance", "documents"}
LIMIT_KEYS = {
    "max_documents",
    "max_input_bytes",
    "max_total_input_bytes",
    "max_pages_per_document",
}
PROVENANCE_KEYS = {"id", "kind", "license", "reference"}
DOCUMENT_KEYS = {
    "id",
    "path",
    "format",
    "bytes",
    "sha256",
    "provenance",
    "features",
    "expected",
}
EXPECTED_KEYS = {"pages", "warnings"}
EVIDENCE_KEYS = {
    "schema",
    "campaign",
    "environment",
    "visual_comparison",
    "integer_visual_metrics",
    "pdf_diagnostic_contract",
    "pdf_point_geometry",
    "semantic_text_metrics",
    "text_geometry_metrics",
    "summary",
    "gate",
    "rows",
}
CAMPAIGN_IDENTITY_KEYS = {
    "name",
    "manifest_sha256",
    "corpus_root_sha256",
    "documents",
    "expected_pages",
}
ENVIRONMENT_KEYS = {
    "source_revision",
    "source_dirty",
    "harness_sha256",
    "cargo_lock_sha256",
    "renderer",
    "oracle",
    "platform",
    "tools",
}
RENDERER_KEYS = {"name", "font_mode"}
ORACLE_KEYS = {"name", "mode", "version", "identity_sha256"}
PLATFORM_KEYS = {"system", "release", "machine"}
TOOL_KEYS = {"name", "version"}
ROW_KEYS = {
    "document",
    "case_id",
    "input_bytes",
    "input_sha256",
    "status",
    "recall",
    "rwml_pages",
    "reference_pages",
    "page_ratio",
    "ahash_similarity",
    "mean_page_ahash_similarity",
    "foreground_ink_iou",
    "compared_pages",
    "unmatched_candidate_pages",
    "unmatched_reference_pages",
    "capped_matched_pages",
    "integer_visual_metrics",
    "pdf_point_geometry",
    "semantic_text_metrics",
    "text_geometry_metrics",
    "render_warnings",
    "render_warning_kinds",
    "reason",
}
MEASURED_ROW_KEYS = ROW_KEYS - {"reason"}
SKIPPED_ROW_KEYS = {
    "document",
    "case_id",
    "input_bytes",
    "input_sha256",
    "status",
    "reason",
}
VISUAL_COMPARISON_KEYS = {
    "dpi",
    "page_cap",
    "foreground_threshold",
    "ahash_size",
    "font_mode",
    "integer_metrics",
}
SUMMARY_KEYS = {
    "documents",
    "measured",
    "skipped",
    "below_recall_min",
    "recall_min",
    "reference_stable",
    "unstable_references",
    "mean_recall",
    "mean_page_ratio",
    "mean_ahash_similarity",
    "mean_page_ahash_similarity",
    "mean_foreground_ink_iou",
    "compared_pages",
    "unmatched_candidate_pages",
    "unmatched_reference_pages",
    "capped_matched_pages",
    "mean_render_warnings",
}
GATE_KEYS = {"passed", "checks"}
GATE_CHECK_KEYS = {"metric", "actual", "op", "threshold", "passed"}
KNOWN_WARNING_KINDS = {
    "UnsupportedFieldEvaluation",
    "FloatingShapePlaceholderOnly",
    "ChartsPreservedButNotModeled",
    "OleObjectsPreservedButNotModeled",
    "UnsupportedMetafileImages",
    "MissingImageBytes",
    "UndecodableRasterImages",
}
PROVENANCE_KINDS = {"generated", "vendored", "converted"}
FORMATS = {"doc", "docx"}
ORACLE_NAMES = {"libreoffice", "microsoft-word"}
ORACLE_MODES = {"local", "container", "com", "applescript", "locked-container"}
FONT_MODES = {"fixed-noto-subsets", "system", "locked-shared-fonts"}

CANONICAL_ID_RE = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
FEATURE_RE = CANONICAL_ID_RE
SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
REVISION_RE = re.compile(r"[0-9a-f]{40}\Z")
TOOL_NAME_RE = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
LOCAL_PATH_PATTERNS = (
    re.compile(r"(?<![A-Za-z]:)/Users/[A-Za-z0-9._-]+/"),
    re.compile(r"/home/[A-Za-z0-9._-]+/"),
    re.compile(r"[A-Za-z]:[/\\]Users[/\\][^/\\\s]+[/\\]"),
    re.compile(r"(?<!\\)\\\\[A-Za-z0-9._-]{2,}\\[^\\\s]{2,}\\"),
)


@dataclass(frozen=True)
class CorpusDocument:
    case_id: str
    relative_path: str
    path: Path
    format: str
    input_bytes: int
    sha256: str
    provenance: str
    features: tuple[str, ...]
    expected_pages: int
    expected_warnings: tuple[str, ...]


@dataclass(frozen=True)
class CorpusManifest:
    schema: str
    campaign: str
    path: Path
    manifest_sha256: str
    corpus_root_sha256: str
    limits: dict[str, int]
    provenance: tuple[dict[str, str], ...]
    documents: tuple[CorpusDocument, ...]

    @property
    def expected_pages(self) -> int:
        return sum(document.expected_pages for document in self.documents)

    def identity(self) -> dict[str, object]:
        return {
            "name": self.campaign,
            "manifest_sha256": self.manifest_sha256,
            "corpus_root_sha256": self.corpus_root_sha256,
            "documents": len(self.documents),
            "expected_pages": self.expected_pages,
        }


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_json_constant(value: str) -> object:
    raise ValueError(f"non-finite JSON number: {value}")


def _parse_json_integer(value: str) -> int:
    digits = value[1:] if value.startswith("-") else value
    if len(digits) > MAX_JSON_INTEGER_DIGITS:
        raise ValueError("JSON integer digit limit exceeded")
    return int(value)


def _validate_json_complexity(value: object) -> None:
    nodes = 0
    stack: list[tuple[object, int]] = [(value, 1)]
    while stack:
        current, depth = stack.pop()
        nodes += 1
        if nodes > MAX_JSON_NODES:
            raise ValueError("JSON node limit exceeded")
        if depth > MAX_JSON_DEPTH:
            raise ValueError("JSON depth limit exceeded")
        if isinstance(current, dict):
            stack.extend((item, depth + 1) for item in current.values())
        elif isinstance(current, list):
            stack.extend((item, depth + 1) for item in current)


def _read_bounded_regular_file(path: Path, maximum: int) -> bytes:
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
        remaining = before.st_size
        chunks: list[bytes] = []
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
        raise ValueError(f"{path.name} changed while reading")
    return payload


def _load_json(path: Path, maximum: int) -> tuple[dict[str, Any], bytes]:
    payload = _read_bounded_regular_file(path, maximum)
    try:
        text = payload.decode("utf-8")
    except UnicodeDecodeError as error:
        raise ValueError(f"{path.name} is not UTF-8") from error
    try:
        value = json.loads(
            text,
            object_pairs_hook=_reject_duplicate_pairs,
            parse_constant=_reject_json_constant,
            parse_int=_parse_json_integer,
        )
    except json.JSONDecodeError as error:
        raise ValueError(f"{path.name} is malformed JSON") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path.name} must contain a JSON object")
    _validate_json_complexity(value)
    return value, payload


def _require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        missing = sorted(expected - actual)
        extra = sorted(actual - expected)
        detail = []
        if missing:
            detail.append(f"missing {missing}")
        if extra:
            detail.append(f"unknown {extra}")
        raise ValueError(f"{label} keys are invalid: {', '.join(detail)}")


def _require_positive_int(value: object, label: str, maximum: int) -> int:
    if (
        not isinstance(value, int)
        or isinstance(value, bool)
        or value <= 0
        or value > maximum
    ):
        raise ValueError(f"{label} is outside the contract")
    return value


def _require_canonical_id(value: object, label: str) -> str:
    if not isinstance(value, str) or CANONICAL_ID_RE.fullmatch(value) is None:
        raise ValueError(f"{label} is not canonical")
    return value


def _safe_relative_path(value: object, label: str, *, suffix: str | None = None) -> str:
    if not isinstance(value, str) or not value or value != value.strip():
        raise ValueError(f"unsafe {label}")
    if not value.isascii() or any(character.isspace() for character in value):
        raise ValueError(f"unsafe {label}: {value}")
    if value.startswith(("/", "\\")) or "\\" in value or ":" in value:
        raise ValueError(f"unsafe {label}: {value}")
    path = PurePosixPath(value)
    if any(part in {"", ".", ".."} for part in path.parts):
        raise ValueError(f"unsafe {label}: {value}")
    if suffix is not None and path.suffix.lower() != suffix:
        raise ValueError(f"{label} has the wrong suffix: {value}")
    return value


def _require_safe_text(value: object, label: str, *, maximum: int = 256) -> str:
    if (
        not isinstance(value, str)
        or not value
        or value != value.strip()
        or len(value) > maximum
        or "\x00" in value
        or "\n" in value
        or "\r" in value
    ):
        raise ValueError(f"{label} is invalid")
    return value


def _require_sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise ValueError(f"{label} is invalid")
    return value


def _resolve_beneath(root: Path, relative: str, label: str) -> Path:
    candidate = root / PurePosixPath(relative)
    if candidate.is_symlink():
        raise ValueError(f"{label} must not be a symlink: {relative}")
    try:
        resolved_root = root.resolve(strict=True)
        resolved = candidate.resolve(strict=True)
        resolved.relative_to(resolved_root)
    except (FileNotFoundError, ValueError) as error:
        raise ValueError(
            f"{label} does not exist beneath corpus root: {relative}"
        ) from error
    if not resolved.is_file():
        raise ValueError(f"{label} is not a regular file: {relative}")
    return resolved


def _corpus_root_sha256(documents: list[CorpusDocument]) -> str:
    digest = hashlib.sha256()
    for document in documents:
        for value in (
            document.case_id,
            document.relative_path,
            document.format,
            str(document.input_bytes),
            document.sha256,
        ):
            encoded = value.encode("utf-8")
            digest.update(len(encoded).to_bytes(8, "little"))
            digest.update(encoded)
    return digest.hexdigest()


def load_corpus_manifest(path: Path) -> CorpusManifest:
    data, payload = _load_json(path, MAX_MANIFEST_BYTES)
    _require_exact_keys(data, CORPUS_KEYS, "corpus")
    if data["schema"] != CORPUS_SCHEMA:
        raise ValueError(f"corpus schema must be {CORPUS_SCHEMA}")
    campaign = _require_canonical_id(data["campaign"], "campaign")

    limits_value = data["limits"]
    if not isinstance(limits_value, dict):
        raise ValueError("limits must be an object")
    _require_exact_keys(limits_value, LIMIT_KEYS, "limits")
    limits = {
        "max_documents": _require_positive_int(
            limits_value["max_documents"], "max_documents", MAX_DOCUMENTS
        ),
        "max_input_bytes": _require_positive_int(
            limits_value["max_input_bytes"], "max_input_bytes", MAX_INPUT_BYTES
        ),
        "max_total_input_bytes": _require_positive_int(
            limits_value["max_total_input_bytes"],
            "max_total_input_bytes",
            MAX_TOTAL_INPUT_BYTES,
        ),
        "max_pages_per_document": _require_positive_int(
            limits_value["max_pages_per_document"],
            "max_pages_per_document",
            MAX_PAGES_PER_DOCUMENT,
        ),
    }

    provenance_value = data["provenance"]
    if not isinstance(provenance_value, list) or not provenance_value:
        raise ValueError("provenance must be a non-empty list")
    provenance: list[dict[str, str]] = []
    provenance_ids: set[str] = set()
    for item in provenance_value:
        if not isinstance(item, dict):
            raise ValueError("provenance entry must be an object")
        _require_exact_keys(item, PROVENANCE_KEYS, "provenance entry")
        provenance_id = _require_canonical_id(item["id"], "provenance id")
        if provenance_id in provenance_ids:
            raise ValueError(f"duplicate provenance id: {provenance_id}")
        provenance_ids.add(provenance_id)
        kind = item["kind"]
        if kind not in PROVENANCE_KINDS:
            raise ValueError(f"provenance kind is invalid: {kind}")
        license_name = _require_safe_text(
            item["license"], "provenance license", maximum=64
        )
        reference = _safe_relative_path(
            item["reference"], "provenance reference", suffix=".md"
        )
        _resolve_beneath(path.parent, reference, "provenance reference")
        provenance.append(
            {
                "id": provenance_id,
                "kind": kind,
                "license": license_name,
                "reference": reference,
            }
        )
    if [item["id"] for item in provenance] != sorted(provenance_ids):
        raise ValueError("provenance entries must be sorted by id")

    documents_value = data["documents"]
    if not isinstance(documents_value, list) or not documents_value:
        raise ValueError("documents must be a non-empty list")
    if len(documents_value) > limits["max_documents"]:
        raise ValueError("documents exceed max_documents")
    documents: list[CorpusDocument] = []
    case_ids: set[str] = set()
    relative_paths: set[str] = set()
    total_bytes = 0
    for item in documents_value:
        if not isinstance(item, dict):
            raise ValueError("document entry must be an object")
        _require_exact_keys(item, DOCUMENT_KEYS, "document")
        case_id = _require_canonical_id(item["id"], "document id")
        if case_id in case_ids:
            raise ValueError(f"duplicate document id: {case_id}")
        case_ids.add(case_id)
        format_name = item["format"]
        if format_name not in FORMATS:
            raise ValueError(f"document format is invalid: {format_name}")
        relative_path = _safe_relative_path(
            item["path"], "document path", suffix=f".{format_name}"
        )
        if relative_path in relative_paths:
            raise ValueError(f"duplicate document path: {relative_path}")
        relative_paths.add(relative_path)
        input_bytes = _require_positive_int(
            item["bytes"], "document bytes", MAX_INPUT_BYTES
        )
        if input_bytes > limits["max_input_bytes"]:
            raise ValueError(f"document exceeds max_input_bytes: {case_id}")
        expected_sha256 = _require_sha256(item["sha256"], "document sha256")
        provenance_id = _require_canonical_id(item["provenance"], "document provenance")
        if provenance_id not in provenance_ids:
            raise ValueError(f"document has unknown provenance: {provenance_id}")
        features_value = item["features"]
        if not isinstance(features_value, list) or not features_value:
            raise ValueError(f"document feature labels are invalid: {case_id}")
        features: list[str] = []
        for feature in features_value:
            if not isinstance(feature, str) or FEATURE_RE.fullmatch(feature) is None:
                raise ValueError(f"document feature label is invalid: {feature}")
            if feature in features:
                raise ValueError(f"duplicate document feature label: {feature}")
            features.append(feature)
        if features != sorted(features):
            raise ValueError(f"document feature labels must be sorted: {case_id}")

        expected = item["expected"]
        if not isinstance(expected, dict):
            raise ValueError("document expected value must be an object")
        _require_exact_keys(expected, EXPECTED_KEYS, "document expected")
        expected_pages = _require_positive_int(
            expected["pages"], "document expected pages", MAX_PAGES_PER_DOCUMENT
        )
        if expected_pages > limits["max_pages_per_document"]:
            raise ValueError(f"document exceeds max_pages_per_document: {case_id}")
        warnings_value = expected["warnings"]
        if not isinstance(warnings_value, list):
            raise ValueError("document expected warnings must be a list")
        warnings: list[str] = []
        for warning in warnings_value:
            if warning not in KNOWN_WARNING_KINDS:
                raise ValueError(f"unknown document expected warning: {warning}")
            if warning in warnings:
                raise ValueError(f"duplicate document expected warning: {warning}")
            warnings.append(warning)
        if warnings != sorted(warnings):
            raise ValueError(f"document expected warnings must be sorted: {case_id}")

        document_path = _resolve_beneath(path.parent, relative_path, "document")
        payload_bytes = _read_bounded_regular_file(
            document_path, limits["max_input_bytes"]
        )
        actual_sha256 = hashlib.sha256(payload_bytes).hexdigest()
        if len(payload_bytes) != input_bytes or actual_sha256 != expected_sha256:
            raise ValueError(f"document input identity mismatch: {case_id}")
        total_bytes += input_bytes
        if total_bytes > limits["max_total_input_bytes"]:
            raise ValueError("documents exceed max_total_input_bytes")
        documents.append(
            CorpusDocument(
                case_id=case_id,
                relative_path=relative_path,
                path=document_path,
                format=format_name,
                input_bytes=input_bytes,
                sha256=expected_sha256,
                provenance=provenance_id,
                features=tuple(features),
                expected_pages=expected_pages,
                expected_warnings=tuple(warnings),
            )
        )
    if [document.case_id for document in documents] != sorted(case_ids):
        raise ValueError("documents must be sorted by id")

    return CorpusManifest(
        schema=CORPUS_SCHEMA,
        campaign=campaign,
        path=path,
        manifest_sha256=hashlib.sha256(payload).hexdigest(),
        corpus_root_sha256=_corpus_root_sha256(documents),
        limits=limits,
        provenance=tuple(provenance),
        documents=tuple(documents),
    )


def _assert_path_neutral(value: object, label: str = "evidence") -> None:
    stack = [value]
    while stack:
        current = stack.pop()
        if isinstance(current, dict):
            stack.extend(current.values())
        elif isinstance(current, list):
            stack.extend(current)
        elif isinstance(current, str):
            if any(pattern.search(current) for pattern in LOCAL_PATH_PATTERNS):
                raise ValueError(f"{label} is not path-neutral")


def _validate_environment(environment: object) -> None:
    if not isinstance(environment, dict):
        raise ValueError("environment must be an object")
    _require_exact_keys(environment, ENVIRONMENT_KEYS, "environment")
    revision = environment["source_revision"]
    if not isinstance(revision, str) or REVISION_RE.fullmatch(revision) is None:
        raise ValueError("environment source revision is invalid")
    if not isinstance(environment["source_dirty"], bool):
        raise ValueError("environment source dirty flag is invalid")
    _require_sha256(environment["harness_sha256"], "environment harness sha256")
    _require_sha256(environment["cargo_lock_sha256"], "environment Cargo.lock sha256")

    renderer = environment["renderer"]
    if not isinstance(renderer, dict):
        raise ValueError("environment renderer must be an object")
    _require_exact_keys(renderer, RENDERER_KEYS, "environment renderer")
    if renderer["name"] != "rwml":
        raise ValueError("environment renderer name is invalid")
    if renderer["font_mode"] not in FONT_MODES:
        raise ValueError("environment renderer font mode is invalid")

    oracle = environment["oracle"]
    if not isinstance(oracle, dict):
        raise ValueError("environment oracle must be an object")
    _require_exact_keys(oracle, ORACLE_KEYS, "environment oracle")
    if oracle["name"] not in ORACLE_NAMES:
        raise ValueError("environment oracle name is invalid")
    if oracle["mode"] not in ORACLE_MODES:
        raise ValueError("environment oracle mode is invalid")
    _require_safe_text(oracle["version"], "environment oracle version")
    _require_sha256(oracle["identity_sha256"], "environment oracle identity")

    platform_value = environment["platform"]
    if not isinstance(platform_value, dict):
        raise ValueError("environment platform must be an object")
    _require_exact_keys(platform_value, PLATFORM_KEYS, "environment platform")
    for key in sorted(PLATFORM_KEYS):
        _require_safe_text(platform_value[key], f"environment platform {key}")

    tools = environment["tools"]
    if not isinstance(tools, list) or not tools:
        raise ValueError("environment tools must be a non-empty list")
    names: list[str] = []
    for tool in tools:
        if not isinstance(tool, dict):
            raise ValueError("environment tool must be an object")
        _require_exact_keys(tool, TOOL_KEYS, "environment tool")
        name = tool["name"]
        if not isinstance(name, str) or TOOL_NAME_RE.fullmatch(name) is None:
            raise ValueError("environment tool name is invalid")
        if name in names:
            raise ValueError(f"duplicate environment tool: {name}")
        names.append(name)
        _require_safe_text(tool["version"], "environment tool version")
    if names != sorted(names):
        raise ValueError("environment tools must be sorted by name")
    _assert_path_neutral(environment, "environment")


def bind_evidence_report(
    core_report: dict[str, Any],
    corpus: CorpusManifest,
    environment: dict[str, Any],
    *,
    capture: dict[str, Any] | None = None,
) -> dict[str, Any]:
    if not isinstance(core_report, dict):
        raise ValueError("core report must be an object")
    expected_core_keys = EVIDENCE_KEYS - {"schema", "campaign", "environment"}
    _require_exact_keys(core_report, expected_core_keys, "core report")
    evidence = {
        "schema": EVIDENCE_SCHEMA if capture is None else CAPTURE_EVIDENCE_SCHEMA,
        "campaign": corpus.identity(),
        "environment": copy.deepcopy(environment),
        **copy.deepcopy(core_report),
    }
    if capture is not None:
        evidence["capture"] = copy.deepcopy(capture)
    validate_evidence_report(evidence, corpus)
    return evidence


def _validate_evidence_row(row: object, document: CorpusDocument) -> None:
    if not isinstance(row, dict):
        raise ValueError("evidence row must be an object")
    status = row.get("status")
    expected_keys = SKIPPED_ROW_KEYS if status == "skip" else MEASURED_ROW_KEYS
    if set(row) != expected_keys:
        raise ValueError("evidence row keys are invalid")
    if row["case_id"] != document.case_id:
        raise ValueError(f"evidence row coverage mismatch: {document.case_id}")
    if row["document"] != document.path.name:
        raise ValueError(f"evidence row document mismatch: {document.case_id}")
    if (
        row["input_bytes"] != document.input_bytes
        or row["input_sha256"] != document.sha256
    ):
        raise ValueError(f"evidence row input identity mismatch: {document.case_id}")
    if status not in {"pass", "fail", "skip"}:
        raise ValueError(f"evidence row status is invalid: {document.case_id}")
    if "reason" in row:
        _require_canonical_id(row["reason"], "evidence row reason")
    if status != "skip":
        validate_integer_metrics(row["integer_visual_metrics"])
        if row["integer_visual_metrics"]["pages"] != row["compared_pages"]:
            raise ValueError("evidence row integer visual page count mismatch")
        validate_pdf_geometry_report(row["pdf_point_geometry"])
        if row["pdf_point_geometry"]["summary"]["pages"] != row["compared_pages"]:
            raise ValueError("evidence row PDF point geometry page count mismatch")
        validate_pdf_semantic_report(row["semantic_text_metrics"])
        if row["semantic_text_metrics"]["pages"] != row["compared_pages"]:
            raise ValueError("evidence row semantic text page count mismatch")
        validate_pdf_text_geometry_report(row["text_geometry_metrics"])
        if row["text_geometry_metrics"]["summary"]["pages"] != row["compared_pages"]:
            raise ValueError("evidence row text geometry page count mismatch")
    for key, value in row.items():
        if key in {
            "recall",
            "ahash_similarity",
            "mean_page_ahash_similarity",
            "foreground_ink_iou",
        }:
            if (
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or not math.isfinite(value)
                or not 0 <= value <= 1
            ):
                raise ValueError(f"evidence row metric is invalid: {key}")
        elif key == "page_ratio":
            if (
                not isinstance(value, (int, float))
                or isinstance(value, bool)
                or not math.isfinite(value)
                or value < 0
            ):
                raise ValueError("evidence row metric is invalid: page_ratio")
        elif key in {
            "input_bytes",
            "rwml_pages",
            "reference_pages",
            "compared_pages",
            "unmatched_candidate_pages",
            "unmatched_reference_pages",
            "capped_matched_pages",
            "render_warnings",
        }:
            if not isinstance(value, int) or isinstance(value, bool) or value < 0:
                raise ValueError(f"evidence row count is invalid: {key}")
    warnings = row.get("render_warning_kinds")
    if warnings is not None:
        if not isinstance(warnings, list) or warnings != sorted(set(warnings)):
            raise ValueError("evidence row warning kinds are invalid")
        if any(warning not in KNOWN_WARNING_KINDS for warning in warnings):
            raise ValueError("evidence row warning kind is unknown")
        if row.get("render_warnings") != len(warnings):
            raise ValueError("evidence row warning count mismatch")
    if status == "skip" and "reason" not in row:
        raise ValueError("skipped evidence row requires a reason")


def _mean(values: list[float | int]) -> float | None:
    if not values:
        return None
    return round(sum(values) / len(values), 4)


def _validate_visual_comparison(value: object) -> None:
    if not isinstance(value, dict):
        raise ValueError("evidence visual comparison must be an object")
    _require_exact_keys(value, VISUAL_COMPARISON_KEYS, "visual comparison")
    integer_limits = {
        "dpi": (1, 600),
        "page_cap": (1, MAX_PAGES_PER_DOCUMENT),
        "foreground_threshold": (0, 255),
        "ahash_size": (1, 64),
    }
    for key, (minimum, maximum) in integer_limits.items():
        item = value[key]
        if (
            not isinstance(item, int)
            or isinstance(item, bool)
            or not minimum <= item <= maximum
        ):
            raise ValueError(f"visual comparison value is invalid: {key}")
    if value["font_mode"] not in FONT_MODES:
        raise ValueError("visual comparison font mode is invalid")
    validate_metric_contract(value["integer_metrics"])


def _validate_metric_environment(
    visual_comparison: dict[str, Any], environment: dict[str, Any]
) -> None:
    implementation = visual_comparison["integer_metrics"]["implementation"]
    tool_names = {tool["name"] for tool in environment["tools"]}
    has_numpy = "numpy" in tool_names
    uses_numpy = implementation == "numpy-integer-exact-v1"
    if uses_numpy != has_numpy:
        raise ValueError("NumPy metric implementation identity is inconsistent")


def _validate_integer_visual_aggregate(
    value: object, rows: list[dict[str, Any]]
) -> None:
    measured = [row for row in rows if row["status"] != "skip"]
    if not measured:
        if value is not None:
            raise ValueError("integer visual aggregate requires measured rows")
        return
    validate_integer_metrics(value)
    expected = aggregate_integer_metrics(
        [row["integer_visual_metrics"] for row in measured]
    )
    if value != expected:
        raise ValueError("integer visual aggregate is inconsistent")


def _validate_pdf_diagnostic_aggregates(
    contract: object,
    geometry: object,
    semantics: object,
    text_geometry: object,
    rows: list[dict[str, Any]],
) -> None:
    validate_diagnostic_contract(contract)
    measured = [row for row in rows if row["status"] != "skip"]
    if not measured:
        if geometry is not None or semantics is not None or text_geometry is not None:
            raise ValueError("PDF diagnostics require measured rows")
        return
    validate_pdf_geometry_summary(geometry)
    expected_geometry = aggregate_pdf_geometry_reports(
        [row["pdf_point_geometry"] for row in measured]
    )
    if geometry != expected_geometry:
        raise ValueError("PDF point geometry aggregate is inconsistent")
    validate_pdf_semantic_report(semantics)
    expected_semantics = aggregate_pdf_semantic_reports(
        [row["semantic_text_metrics"] for row in measured]
    )
    if semantics != expected_semantics:
        raise ValueError("semantic text aggregate is inconsistent")
    validate_pdf_text_geometry_summary(text_geometry)
    expected_text_geometry = aggregate_pdf_text_geometry_reports(
        [row["text_geometry_metrics"] for row in measured]
    )
    if text_geometry != expected_text_geometry:
        raise ValueError("text geometry aggregate is inconsistent")


def _validate_summary(
    value: object, rows: list[dict[str, Any]], corpus: CorpusManifest
) -> None:
    if not isinstance(value, dict):
        raise ValueError("evidence summary must be an object")
    _require_exact_keys(value, SUMMARY_KEYS, "evidence summary")
    recall_min = value["recall_min"]
    if (
        not isinstance(recall_min, (int, float))
        or isinstance(recall_min, bool)
        or not math.isfinite(recall_min)
        or not 0 <= recall_min <= 1
    ):
        raise ValueError("evidence summary recall_min is invalid")
    measured = [row for row in rows if row["status"] != "skip"]
    skipped = [row for row in rows if row["status"] == "skip"]
    for row in measured:
        expected_status = "pass" if row["recall"] >= recall_min else "fail"
        if row["status"] != expected_status:
            raise ValueError(
                f"evidence row status contradicts recall: {row['case_id']}"
            )
    expected = {
        "documents": len(rows),
        "measured": len(measured),
        "skipped": len(skipped),
        "below_recall_min": sum(row["recall"] < recall_min for row in measured),
        "mean_recall": _mean([row["recall"] for row in measured]),
        "mean_page_ratio": _mean([row["page_ratio"] for row in measured]),
        "mean_ahash_similarity": _mean([row["ahash_similarity"] for row in measured]),
        "mean_page_ahash_similarity": _mean(
            [row["mean_page_ahash_similarity"] for row in measured]
        ),
        "mean_foreground_ink_iou": _mean(
            [row["foreground_ink_iou"] for row in measured]
        ),
        "compared_pages": sum(row["compared_pages"] for row in measured),
        "unmatched_candidate_pages": sum(
            row["unmatched_candidate_pages"] for row in measured
        ),
        "unmatched_reference_pages": sum(
            row["unmatched_reference_pages"] for row in measured
        ),
        "capped_matched_pages": sum(row["capped_matched_pages"] for row in measured),
        "mean_render_warnings": _mean([row["render_warnings"] for row in measured]),
    }
    for key, expected_value in expected.items():
        if value[key] != expected_value:
            raise ValueError(f"evidence summary {key} is inconsistent")
    if value["documents"] != len(corpus.documents):
        raise ValueError("evidence summary document count mismatch")
    reference_stable = value["reference_stable"]
    if reference_stable is not None and not isinstance(reference_stable, bool):
        raise ValueError("evidence summary reference_stable is invalid")
    unstable = value["unstable_references"]
    if (
        not isinstance(unstable, list)
        or unstable != sorted(set(unstable))
        or any(not isinstance(item, str) for item in unstable)
    ):
        raise ValueError("evidence summary unstable_references is invalid")
    known_documents = {document.path.name for document in corpus.documents}
    if any(item not in known_documents for item in unstable):
        raise ValueError("evidence summary names an unknown unstable reference")
    if reference_stable is True and unstable:
        raise ValueError("stable evidence summary lists unstable references")
    if reference_stable is False and not unstable:
        raise ValueError("unstable evidence summary omits unstable references")
    if reference_stable is None and unstable:
        raise ValueError("unchecked evidence summary lists unstable references")


def _validate_gate(value: object) -> None:
    if not isinstance(value, dict):
        raise ValueError("evidence gate must be an object")
    _require_exact_keys(value, GATE_KEYS, "evidence gate")
    if not isinstance(value["passed"], bool) or not isinstance(value["checks"], list):
        raise ValueError("evidence gate is invalid")
    seen: set[tuple[str, str]] = set()
    for check in value["checks"]:
        if not isinstance(check, dict):
            raise ValueError("evidence gate check must be an object")
        _require_exact_keys(check, GATE_CHECK_KEYS, "evidence gate check")
        metric = check["metric"]
        if (
            not isinstance(metric, str)
            or re.fullmatch(r"[a-z][a-z0-9_]*", metric) is None
        ):
            raise ValueError("evidence gate metric is invalid")
        op = check["op"]
        if op not in {">=", "<="}:
            raise ValueError("evidence gate operator is invalid")
        key = (metric, op)
        if key in seen:
            raise ValueError("duplicate evidence gate check")
        seen.add(key)
        threshold = check["threshold"]
        actual = check["actual"]
        if (
            not isinstance(threshold, (int, float))
            or isinstance(threshold, bool)
            or not math.isfinite(threshold)
        ):
            raise ValueError("evidence gate threshold is invalid")
        if actual is not None and (
            not isinstance(actual, (int, float))
            or isinstance(actual, bool)
            or not math.isfinite(actual)
        ):
            raise ValueError("evidence gate actual is invalid")
        expected_passed = actual is not None and (
            (op == ">=" and actual >= threshold) or (op == "<=" and actual <= threshold)
        )
        if check["passed"] is not expected_passed:
            raise ValueError("evidence gate check result is inconsistent")
    if value["passed"] is not all(check["passed"] for check in value["checks"]):
        raise ValueError("evidence gate passed result is inconsistent")


def _validate_capture_binding(evidence: dict, corpus: CorpusManifest) -> None:
    value = evidence["capture"]
    if not isinstance(value, dict):
        raise ValueError("capture binding must be an object")
    _require_exact_keys(
        value,
        {
            "schema",
            "sha256",
            "environment_sha256",
            "source_revision",
            "campaign",
            "renderer_sha256",
            "font_scope",
            "cases",
        },
        "capture binding",
    )
    environment = evidence["environment"]
    if (
        value["schema"] != CAPTURE_SCHEMAS[evidence["schema"]]
        or value["source_revision"] != environment["source_revision"]
        or environment["source_dirty"] is not False
        or value["campaign"] != corpus.identity()
        or environment["oracle"]["name"] != "libreoffice"
        or environment["oracle"]["mode"] != "locked-container"
        or environment["renderer"]["font_mode"] != "locked-shared-fonts"
        or evidence["visual_comparison"]["font_mode"] != "locked-shared-fonts"
        or value["environment_sha256"] != environment["oracle"]["identity_sha256"]
        or value["font_scope"] != "declared-font-resources"
        or evidence["summary"].get("reference_stable") is not None
        or evidence["summary"].get("unstable_references") != []
    ):
        raise ValueError("capture binding identity differs")
    for name in ("sha256", "environment_sha256", "renderer_sha256"):
        _require_sha256(value[name], f"capture {name}")
    if not isinstance(value["cases"], list) or len(value["cases"]) != len(
        corpus.documents
    ):
        raise ValueError("capture binding coverage differs")
    keys = {
        "case_id",
        "input_sha256",
        "native_pdf_sha256",
        "reference_pdf_sha256",
        "native_fonts_sha256",
        "reference_fonts_sha256",
    }
    for row, document in zip(value["cases"], corpus.documents, strict=True):
        if not isinstance(row, dict):
            raise ValueError("capture case must be an object")
        _require_exact_keys(row, keys, "capture case")
        if row["case_id"] != document.case_id or row["input_sha256"] != document.sha256:
            raise ValueError("capture case identity differs")
        for key in keys - {"case_id"}:
            _require_sha256(row[key], f"capture case {key}")


def validate_evidence_report(evidence: dict[str, Any], corpus: CorpusManifest) -> None:
    if not isinstance(evidence, dict):
        raise ValueError("evidence must be an object")
    captured = evidence.get("schema") in CAPTURE_SCHEMAS
    _require_exact_keys(
        evidence, EVIDENCE_KEYS | ({"capture"} if captured else set()), "evidence"
    )
    if evidence["schema"] not in {EVIDENCE_SCHEMA, *CAPTURE_SCHEMAS}:
        raise ValueError("evidence schema is unsupported")
    campaign = evidence["campaign"]
    if not isinstance(campaign, dict):
        raise ValueError("evidence campaign must be an object")
    _require_exact_keys(campaign, CAMPAIGN_IDENTITY_KEYS, "evidence campaign")
    if campaign != corpus.identity():
        raise ValueError("evidence campaign identity mismatch")
    _validate_environment(evidence["environment"])

    rows = evidence["rows"]
    if not isinstance(rows, list) or len(rows) != len(corpus.documents):
        raise ValueError("evidence row coverage mismatch")
    for row, document in zip(rows, corpus.documents, strict=True):
        _validate_evidence_row(row, document)
    _validate_integer_visual_aggregate(evidence["integer_visual_metrics"], rows)
    _validate_pdf_diagnostic_aggregates(
        evidence["pdf_diagnostic_contract"],
        evidence["pdf_point_geometry"],
        evidence["semantic_text_metrics"],
        evidence["text_geometry_metrics"],
        rows,
    )
    _validate_visual_comparison(evidence["visual_comparison"])
    _validate_metric_environment(evidence["visual_comparison"], evidence["environment"])
    _validate_summary(evidence["summary"], rows, corpus)
    _validate_gate(evidence["gate"])
    if captured:
        _validate_capture_binding(evidence, corpus)
    elif (
        evidence["environment"]["renderer"]["font_mode"] == "locked-shared-fonts"
        or evidence["environment"]["oracle"]["mode"] == "locked-container"
        or evidence["visual_comparison"]["font_mode"] == "locked-shared-fonts"
    ):
        raise ValueError("shared capture profile requires capture-bound evidence")
    _assert_path_neutral(evidence)


def load_evidence_report(path: Path, corpus: CorpusManifest) -> dict[str, Any]:
    evidence, _ = _load_json(path, MAX_EVIDENCE_BYTES)
    validate_evidence_report(evidence, corpus)
    return evidence


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Validate one strict public rwml render-oracle corpus manifest."
    )
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    try:
        corpus = load_corpus_manifest(args.manifest)
    except (OSError, ValueError) as error:
        print(f"render_oracle_contract: {error}", file=sys.stderr)
        return 1
    print(
        json.dumps(
            {
                "schema": corpus.schema,
                "campaign": corpus.campaign,
                "manifest_sha256": corpus.manifest_sha256,
                "corpus_root_sha256": corpus.corpus_root_sha256,
                "documents": len(corpus.documents),
                "expected_pages": corpus.expected_pages,
            },
            ensure_ascii=True,
            sort_keys=True,
            separators=(",", ":"),
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
