#!/usr/bin/env python3
"""Multi-metric validation of rwml's PDF renderer against LibreOffice.

For each input `.doc`/`.docx`, render it two ways and compare:

  * rwml        — `cargo run --features render --example to_pdf -- IN OUT`
  * LibreOffice — `soffice --headless --convert-to pdf` (the reference oracle)

and report complementary metrics per document:

  * text recall   — fraction of the reference's whitespace-normalized tokens that
                    also appear in rwml's text layer, after dropping volatile
                    LibreOffice-only field text such as local file paths and
                    missing-reference placeholders, plus known fallback shape
                    placeholders and joined tracked-change/footnote markers
                    when rwml's report proves that context.
  * page ratio    — rwml page count / reference page count (≈ 1.0 is good).
  * legacy aHash  — average-hash Hamming similarity of page 1 at 72 DPI
                    (0..1; retained unchanged for historical trend continuity).
  * page aHash    — mean average-hash similarity across every matched page up to
                    a configurable hard cap at a reported fixed DPI.
  * ink IoU       — foreground-pixel intersection-over-union across those pages;
                    canvases are white-padded, never stretched, before comparison.
  * integer image — raw error sums and integer PPM similarity for RGB, foreground,
                    edge, conservative text-ink, matched color, and blurred luma;
                    one-pixel mask matching and fixed work accounting are explicit.
  * PDF geometry  — page, MediaBox, and CropBox coordinates in integer
                    millipoints, with exact candidate-minus-reference deltas.
  * semantics     — privacy-preserving token, codepoint, and token-bigram counts
                    and integer PPM precision/recall/F1; no document text is kept.
  * text geometry — word/line boxes whose normalized token tuple is unique on
                    both sides, with bounded signed millipoint histograms.
  * warnings      — rwml `RenderReport` warning count/kinds for trend tracking.

Local LibreOffice runs seed and initialize a fresh per-document user profile before
export, verify the installed Noto font bundle, and attest each embedded reference PDF
font. With ``--verify-oracle``, a missing or unequal second reference render is a gate
failure; it is never reported as a successful fidelity comparison.

This is a developer tool, not part of the crate. It needs PyMuPDF (`pip install
pymupdf`), Pillow, and either a local `soffice` or the `lo-cli` Docker image.
By default, `--soffice auto` prefers local `soffice` when present and falls back
to Docker.

  python scripts/render_validate.py corpus/public/**/*.docx
  python scripts/render_validate.py --manifest corpus/public/RENDER_ORACLE.json
  python scripts/render_validate.py --soffice docker corpus/*.doc
  python scripts/render_validate.py --json corpus/public/**/*.docx > render-report.json
  python scripts/render_validate.py --json --manifest corpus/public/RENDER_ORACLE.json > render-report.json
  python scripts/render_validate.py --json --min-mean-recall 0.90 --max-skipped 0 corpus/public/**/*.docx > render-report.json
"""

from __future__ import annotations

import argparse
import contextlib
import hashlib
import json
import math
import os
import platform
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path

try:
    from render_oracle_contract import (
        CorpusDocument,
        CorpusManifest,
        bind_evidence_report,
        load_corpus_manifest,
    )
    from libreoffice_oracle_fonts import (
        DEFAULT_FONT_LOCK as LOCAL_ORACLE_FONT_LOCK,
        installation_font_identity,
        load_font_lock,
        normalized_postscript_name,
        sfnt_revision,
        validate_pdf_font_identities,
    )
    from render_evidence_metrics import (
        METRIC_WORK_UNITS_PER_PIXEL,
        aggregate_metrics as aggregate_integer_metrics,
        image_metrics as integer_image_metrics,
        metric_contract as integer_metric_contract,
        numpy_module as integer_metric_numpy,
        validate_metrics as validate_integer_metrics,
    )
    from render_pdf_diagnostics import (
        MAX_SEMANTIC_CODEPOINTS,
        MAX_SEMANTIC_TOKENS,
        MAX_TEXT_GEOMETRY_ITEMS,
        SemanticTextBox as PdfSemanticTextBox,
        aggregate_geometry_reports as aggregate_pdf_geometry_reports,
        aggregate_semantic_reports as aggregate_pdf_semantic_reports,
        aggregate_text_geometry_reports as aggregate_pdf_text_geometry_reports,
        canonical_page_geometry as canonical_pdf_page_geometry,
        canonical_text_box as canonical_pdf_text_box,
        diagnostic_contract as pdf_diagnostic_contract,
        geometry_report as pdf_geometry_report,
        normalize_semantic_tokens as normalize_pdf_semantic_tokens,
        page_geometry_metrics as pdf_page_geometry_metrics,
        semantic_metrics as pdf_semantic_metrics,
        semantic_report as pdf_semantic_report,
        text_geometry_page as pdf_text_geometry_page,
        text_geometry_report as pdf_text_geometry_report,
        validate_geometry_report as validate_pdf_geometry_report,
        validate_semantic_report as validate_pdf_semantic_report,
        validate_text_geometry_report as validate_pdf_text_geometry_report,
    )
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.render_oracle_contract import (
        CorpusDocument,
        CorpusManifest,
        bind_evidence_report,
        load_corpus_manifest,
    )
    from scripts.libreoffice_oracle_fonts import (
        DEFAULT_FONT_LOCK as LOCAL_ORACLE_FONT_LOCK,
        installation_font_identity,
        load_font_lock,
        normalized_postscript_name,
        sfnt_revision,
        validate_pdf_font_identities,
    )
    from scripts.render_evidence_metrics import (
        METRIC_WORK_UNITS_PER_PIXEL,
        aggregate_metrics as aggregate_integer_metrics,
        image_metrics as integer_image_metrics,
        metric_contract as integer_metric_contract,
        numpy_module as integer_metric_numpy,
        validate_metrics as validate_integer_metrics,
    )
    from scripts.render_pdf_diagnostics import (
        MAX_SEMANTIC_CODEPOINTS,
        MAX_SEMANTIC_TOKENS,
        MAX_TEXT_GEOMETRY_ITEMS,
        SemanticTextBox as PdfSemanticTextBox,
        aggregate_geometry_reports as aggregate_pdf_geometry_reports,
        aggregate_semantic_reports as aggregate_pdf_semantic_reports,
        aggregate_text_geometry_reports as aggregate_pdf_text_geometry_reports,
        canonical_page_geometry as canonical_pdf_page_geometry,
        canonical_text_box as canonical_pdf_text_box,
        diagnostic_contract as pdf_diagnostic_contract,
        geometry_report as pdf_geometry_report,
        normalize_semantic_tokens as normalize_pdf_semantic_tokens,
        page_geometry_metrics as pdf_page_geometry_metrics,
        semantic_metrics as pdf_semantic_metrics,
        semantic_report as pdf_semantic_report,
        text_geometry_page as pdf_text_geometry_page,
        text_geometry_report as pdf_text_geometry_report,
        validate_geometry_report as validate_pdf_geometry_report,
        validate_semantic_report as validate_pdf_semantic_report,
        validate_text_geometry_report as validate_pdf_text_geometry_report,
    )

with contextlib.redirect_stdout(sys.stderr):
    try:
        import pymupdf as fitz
    except ImportError:
        try:
            import fitz  # type: ignore[no-redef]  # Legacy PyMuPDF.
        except ImportError:
            fitz = None
try:
    from PIL import Image, ImageChops
except ImportError:
    Image = None
    ImageChops = None


DEFAULT_RASTER_DPI = 110
DEFAULT_PAGE_CAP = 32
DEFAULT_FOREGROUND_THRESHOLD = 245
DEFAULT_AHASH_SIZE = 16
DEFAULT_FONT_MODE = "fixed-noto-subsets"
REPO = Path(__file__).resolve().parents[1]
LOCAL_ORACLE_PROFILE = Path(__file__).with_name(
    "render-oracle-local-profile.xcu"
).resolve()
MAX_RASTER_DPI = 600
MAX_PAGE_CAP = 256
MAX_AHASH_SIZE = 64
MAX_RASTER_PAGE_PIXELS = 40_000_000
MAX_NORMALIZED_CANVAS_PIXELS = 50_000_000
MAX_BUFFERED_RASTER_PIXELS = 100_000_000
MAX_INTEGER_METRIC_WORK_UNITS = (
    MAX_NORMALIZED_CANVAS_PIXELS * METRIC_WORK_UNITS_PER_PIXEL
)
MAX_VOLATILE_REFERENCE_PATH_TOKENS = 8
OFFICE_DOCUMENT_EXTENSIONS = (".doc", ".docx", ".docm", ".dot", ".dotx", ".rtf")

COUNT_THRESHOLD_METRICS = {
    "below_recall_min",
    "skipped",
    "unmatched_candidate_pages",
    "unmatched_reference_pages",
}
SCORE_THRESHOLD_METRICS = {
    "mean_recall",
    "mean_page_ratio",
    "mean_ahash_similarity",
    "mean_page_ahash_similarity",
    "mean_foreground_ink_iou",
}
BOUNDED_SCORE_THRESHOLD_METRICS = {
    "mean_recall",
    "mean_ahash_similarity",
    "mean_page_ahash_similarity",
    "mean_foreground_ink_iou",
}
VALID_RENDER_WARNING_KINDS = {
    "UnsupportedFieldEvaluation",
    "FloatingShapePlaceholderOnly",
    "ChartsPreservedButNotModeled",
    "OleObjectsPreservedButNotModeled",
    "UnsupportedMetafileImages",
    "MissingImageBytes",
    "UndecodableRasterImages",
}
UNSUPPORTED_OBJECT_WARNING_KINDS = {
    "FloatingShapePlaceholderOnly",
    "ChartsPreservedButNotModeled",
    "OleObjectsPreservedButNotModeled",
    "UnsupportedMetafileImages",
}


class RenderDependencyError(RuntimeError):
    """A selected render backend executable is unavailable."""


class VisualMetricError(RuntimeError):
    """A PDF page could not be rasterized or compared deterministically."""


@dataclass
class ValidationRow:
    document: str
    status: str
    case_id: str | None = None
    input_bytes: int | None = None
    input_sha256: str | None = None
    recall: float | None = None
    rwml_pages: int | None = None
    reference_pages: int | None = None
    page_ratio: float | None = None
    ahash_similarity: float | None = None
    mean_page_ahash_similarity: float | None = None
    foreground_ink_iou: float | None = None
    compared_pages: int | None = None
    unmatched_candidate_pages: int | None = None
    unmatched_reference_pages: int | None = None
    capped_matched_pages: int | None = None
    integer_visual_metrics: dict[str, int] | None = None
    pdf_point_geometry: dict[str, object] | None = None
    semantic_text_metrics: dict[str, int] | None = None
    text_geometry_metrics: dict[str, object] | None = None
    render_warnings: int | None = None
    render_warning_kinds: list[str] | None = None
    reason: str | None = None


@dataclass(frozen=True)
class VisualMetrics:
    mean_page_ahash_similarity: float | None
    foreground_ink_iou: float | None
    compared_pages: int
    unmatched_candidate_pages: int
    unmatched_reference_pages: int
    capped_matched_pages: int
    integer_visual_metrics: dict[str, int] | None
    pdf_point_geometry: dict[str, object] | None
    semantic_text_metrics: dict[str, int] | None
    text_geometry_metrics: dict[str, object] | None


def is_finite_number(value: object) -> bool:
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
    )


def require_pdf_deps() -> None:
    missing = []
    if fitz is None:
        missing.append("PyMuPDF (pip install pymupdf)")
    if Image is None or ImageChops is None:
        missing.append("Pillow (pip install pillow)")
    if missing:
        sys.exit("PDF validation dependencies required: " + ", ".join(missing))


def resolve_soffice_mode(mode: str) -> str:
    if mode != "auto":
        return mode
    if shutil.which("soffice") is not None:
        return "local"
    if shutil.which("docker") is not None:
        return "docker"
    raise RenderDependencyError(
        "LibreOffice validation dependency required: neither soffice nor docker "
        "executable found; install LibreOffice or Docker"
    )


def mean(values: list[float]) -> float | None:
    if not values:
        return None
    return round(sum(values) / len(values), 4)


def row_dict(row: ValidationRow) -> dict:
    return {k: v for k, v in asdict(row).items() if v is not None}


def validate_visual_settings(settings: dict | None = None) -> dict[str, int | str]:
    defaults = {
        "dpi": DEFAULT_RASTER_DPI,
        "page_cap": DEFAULT_PAGE_CAP,
        "foreground_threshold": DEFAULT_FOREGROUND_THRESHOLD,
        "ahash_size": DEFAULT_AHASH_SIZE,
        "font_mode": DEFAULT_FONT_MODE,
    }
    if settings is None:
        return defaults
    if not isinstance(settings, dict):
        raise ValueError("visual settings must be an object")
    unknown = set(settings) - set(defaults)
    if unknown:
        raise ValueError(f"unknown visual setting: {sorted(unknown)[0]}")
    values = defaults | settings
    for name in ("dpi", "page_cap", "foreground_threshold", "ahash_size"):
        value = values[name]
        if not isinstance(value, int) or isinstance(value, bool):
            raise ValueError(f"visual setting is invalid: {name}")
    if values["font_mode"] not in {"fixed-noto-subsets", "system", "locked-shared-fonts"}:
        raise ValueError(
            f"visual setting is out of range: font_mode={values['font_mode']}"
        )
    if not 1 <= values["dpi"] <= MAX_RASTER_DPI:
        raise ValueError(f"visual setting is out of range: dpi={values['dpi']}")
    if not 1 <= values["page_cap"] <= MAX_PAGE_CAP:
        raise ValueError(
            f"visual setting is out of range: page_cap={values['page_cap']}"
        )
    if not 0 <= values["foreground_threshold"] <= 255:
        raise ValueError(
            "visual setting is out of range: "
            f"foreground_threshold={values['foreground_threshold']}"
        )
    if not 1 <= values["ahash_size"] <= MAX_AHASH_SIZE:
        raise ValueError(
            f"visual setting is out of range: ahash_size={values['ahash_size']}"
        )
    return values


def resolve_validation_thresholds(
    args: argparse.Namespace, *, strict_corpus: bool
) -> dict[str, object]:
    if strict_corpus:
        if args.max_skipped not in (None, 0):
            raise ValueError("strict corpus threshold must be zero: max_skipped")
    return {
        "require_reference_stable": args.verify_oracle,
        "min_mean_recall": args.min_mean_recall,
        "min_mean_page_ratio": args.min_mean_page_ratio,
        "max_mean_page_ratio": args.max_mean_page_ratio,
        "min_mean_ahash_similarity": args.min_mean_ahash_similarity,
        "min_mean_page_ahash_similarity": args.min_mean_page_ahash_similarity,
        "min_mean_foreground_ink_iou": args.min_mean_foreground_ink_iou,
        "max_mean_render_warnings": args.max_mean_render_warnings,
        "max_skipped": 0 if strict_corpus else args.max_skipped,
        "max_unmatched_candidate_pages": args.max_unmatched_candidate_pages,
        "max_unmatched_reference_pages": args.max_unmatched_reference_pages,
    }


def add_threshold_check(
    checks: list[dict],
    metric: str,
    actual: float | int | None,
    op: str,
    threshold: float | int | None,
) -> None:
    if threshold is None:
        return
    if not is_finite_number(threshold):
        raise ValueError(f"non-finite threshold for {metric}: {threshold}")
    if metric in COUNT_THRESHOLD_METRICS and threshold < 0:
        raise ValueError(f"negative count threshold for {metric}: {threshold}")
    if op == ">=" and metric in SCORE_THRESHOLD_METRICS and threshold < 0:
        raise ValueError(f"negative score threshold for {metric}: {threshold}")
    if metric in BOUNDED_SCORE_THRESHOLD_METRICS and threshold > 1:
        raise ValueError(f"score threshold above one for {metric}: {threshold}")
    if actual is None:
        passed = False
    elif op == ">=":
        passed = actual >= threshold
    elif op == "<=":
        passed = actual <= threshold
    else:
        raise ValueError(f"unsupported threshold operator: {op}")
    checks.append(
        {
            "metric": metric,
            "op": op,
            "threshold": threshold,
            "actual": actual,
            "passed": passed,
        }
    )


def validation_gate(summary: dict, thresholds: dict | None = None) -> dict:
    thresholds = thresholds or {}
    require_reference_stable = thresholds.get("require_reference_stable", False)
    if not isinstance(require_reference_stable, bool):
        raise ValueError("require_reference_stable must be a boolean")
    checks = []
    add_threshold_check(
        checks,
        "below_recall_min",
        summary.get("below_recall_min"),
        "<=",
        0,
    )
    if require_reference_stable:
        add_threshold_check(
            checks,
            "reference_stable",
            1 if summary.get("reference_stable") is True else 0,
            ">=",
            1,
        )
    add_threshold_check(
        checks,
        "mean_recall",
        summary.get("mean_recall"),
        ">=",
        thresholds.get("min_mean_recall"),
    )
    add_threshold_check(
        checks,
        "mean_page_ratio",
        summary.get("mean_page_ratio"),
        ">=",
        thresholds.get("min_mean_page_ratio"),
    )
    add_threshold_check(
        checks,
        "mean_page_ratio",
        summary.get("mean_page_ratio"),
        "<=",
        thresholds.get("max_mean_page_ratio"),
    )
    add_threshold_check(
        checks,
        "mean_ahash_similarity",
        summary.get("mean_ahash_similarity"),
        ">=",
        thresholds.get("min_mean_ahash_similarity"),
    )
    add_threshold_check(
        checks,
        "mean_page_ahash_similarity",
        summary.get("mean_page_ahash_similarity"),
        ">=",
        thresholds.get("min_mean_page_ahash_similarity"),
    )
    add_threshold_check(
        checks,
        "mean_foreground_ink_iou",
        summary.get("mean_foreground_ink_iou"),
        ">=",
        thresholds.get("min_mean_foreground_ink_iou"),
    )
    add_threshold_check(
        checks,
        "mean_render_warnings",
        summary.get("mean_render_warnings"),
        "<=",
        thresholds.get("max_mean_render_warnings"),
    )
    add_threshold_check(
        checks,
        "skipped",
        summary.get("skipped"),
        "<=",
        thresholds.get("max_skipped"),
    )
    add_threshold_check(
        checks,
        "unmatched_candidate_pages",
        summary.get("unmatched_candidate_pages"),
        "<=",
        thresholds.get("max_unmatched_candidate_pages"),
    )
    add_threshold_check(
        checks,
        "unmatched_reference_pages",
        summary.get("unmatched_reference_pages"),
        "<=",
        thresholds.get("max_unmatched_reference_pages"),
    )
    return {"passed": all(check["passed"] for check in checks), "checks": checks}


def reference_page_digests(pdf: Path, *, dpi: int, page_cap: int) -> list[str] | None:
    """Complete, dimension-bound raster digests for a reference stability probe."""
    try:
        images, total_pages = rasterize_pdf_pages(pdf, dpi=dpi, page_cap=page_cap)
    except Exception:
        return None
    if not images or len(images) != total_pages:
        return None
    digests = []
    for image in images:
        try:
            digest = hashlib.sha256()
            digest.update(image.width.to_bytes(8, "big"))
            digest.update(image.height.to_bytes(8, "big"))
            digest.update(image.mode.encode("ascii"))
            digest.update(image.tobytes())
            digests.append(digest.hexdigest())
        except Exception:
            return None
    return digests


def reference_pdf_font_identities(pdf: Path) -> list[dict[str, object]]:
    """Read path-neutral font identities from embedded PDF subset programs."""
    if fitz is None:
        raise ValueError("PyMuPDF is required for reference font attestation")
    document = fitz.open(pdf)
    identities: dict[str, int] = {}
    extracted_xrefs: set[int] = set()
    try:
        for page in document:
            for font in page.get_fonts(full=True):
                if len(font) < 4:
                    raise ValueError("reference PDF font resource is malformed")
                xref = font[0]
                base_name = font[3]
                if (
                    isinstance(xref, bool)
                    or not isinstance(xref, int)
                    or xref <= 0
                    or not isinstance(base_name, str)
                    or not base_name
                ):
                    raise ValueError("reference PDF font resource is malformed")
                if xref in extracted_xrefs:
                    continue
                extracted_xrefs.add(xref)
                extracted = document.extract_font(xref)
                if (
                    not isinstance(extracted, tuple)
                    or len(extracted) < 4
                    or not isinstance(extracted[3], bytes)
                ):
                    raise ValueError("reference PDF font program is unavailable")
                name = normalized_postscript_name(base_name)
                revision = sfnt_revision(extracted[3])
                previous = identities.get(name)
                if previous is not None and previous != revision:
                    raise ValueError("reference PDF font identity is ambiguous")
                identities[name] = revision
    finally:
        document.close()
    return [
        {"postscript_name": name, "sfnt_revision": identities[name]}
        for name in sorted(identities)
    ]


def oracle_stability_verdict(
    first: list[str] | None, second: list[str] | None
) -> bool | None:
    """Whether two renders of the same reference document match.

    `None` means a complete comparison is unavailable. A caller requiring
    repeatability must not treat that unknown result as successful verification.
    """
    if not first or not second:
        return None
    return list(first) == list(second)


def validation_report(
    rows: list[ValidationRow],
    recall_min: float,
    thresholds: dict | None = None,
    visual_settings: dict | None = None,
    reference_stable: bool | None = None,
    unstable_references: list[str] | None = None,
) -> dict:
    for row in rows:
        if not isinstance(row.document, str):
            raise ValueError("document must be a string")
        if not row.document.strip():
            raise ValueError("document must not be empty")
        if row.document != row.document.strip():
            raise ValueError(
                f"document must not have surrounding whitespace: {row.document}"
            )
        if "/" in row.document or "\\" in row.document:
            raise ValueError(f"document path is invalid: {row.document}")
        if row.status not in {"pass", "fail", "skip"}:
            raise ValueError(f"status is invalid: {row.status}")
        identity_values = (row.case_id, row.input_bytes, row.input_sha256)
        if any(value is not None for value in identity_values) and not all(
            value is not None for value in identity_values
        ):
            raise ValueError("row input identity is incomplete")
        if row.case_id is not None and (
            not isinstance(row.case_id, str)
            or not row.case_id
            or row.case_id != row.case_id.strip()
            or not row.case_id.isascii()
        ):
            raise ValueError("row case id is invalid")
        if row.input_sha256 is not None and (
            not isinstance(row.input_sha256, str)
            or re.fullmatch(r"[0-9a-f]{64}", row.input_sha256) is None
        ):
            raise ValueError("row input sha256 is invalid")
        for metric in (
            "recall",
            "page_ratio",
            "ahash_similarity",
            "mean_page_ahash_similarity",
            "foreground_ink_iou",
            "render_warnings",
        ):
            value = getattr(row, metric)
            if value is not None and not is_finite_number(value):
                raise ValueError(f"metric is invalid: {metric}")
            if value is not None and metric in {
                "recall",
                "ahash_similarity",
                "mean_page_ahash_similarity",
                "foreground_ink_iou",
            }:
                if not 0 <= value <= 1:
                    raise ValueError(f"metric is out of range: {metric}")
        for metric in (
            "rwml_pages",
            "reference_pages",
            "input_bytes",
            "render_warnings",
            "compared_pages",
            "unmatched_candidate_pages",
            "unmatched_reference_pages",
            "capped_matched_pages",
        ):
            value = getattr(row, metric)
            if value is not None and (
                not isinstance(value, int) or isinstance(value, bool) or value < 0
            ):
                raise ValueError(f"count is invalid: {metric}")
        if row.render_warning_kinds is not None:
            if not isinstance(row.render_warning_kinds, list):
                raise ValueError("render warning kinds must be a list")
            if (
                row.render_warnings is not None
                and row.render_warnings != len(row.render_warning_kinds)
            ):
                raise ValueError("render warning count mismatch")
            row_warnings = set()
            for warning in row.render_warning_kinds:
                if (
                    not isinstance(warning, str)
                    or not warning
                    or warning != warning.strip()
                    or not warning.isascii()
                    or not warning.isidentifier()
                ):
                    raise ValueError(f"render warning kind is invalid: {warning}")
                if warning not in VALID_RENDER_WARNING_KINDS:
                    raise ValueError(f"unknown render warning kind: {warning}")
                if warning in row_warnings:
                    raise ValueError(f"duplicate render warning kind: {warning}")
                row_warnings.add(warning)
        if row.integer_visual_metrics is not None:
            validate_integer_metrics(row.integer_visual_metrics)
            if row.compared_pages != row.integer_visual_metrics["pages"]:
                raise ValueError("integer visual page count mismatch")
        if row.pdf_point_geometry is not None:
            validate_pdf_geometry_report(row.pdf_point_geometry)
            if row.compared_pages != row.pdf_point_geometry["summary"]["pages"]:
                raise ValueError("PDF point geometry page count mismatch")
        if row.semantic_text_metrics is not None:
            validate_pdf_semantic_report(row.semantic_text_metrics)
            if row.compared_pages != row.semantic_text_metrics["pages"]:
                raise ValueError("semantic text metric page count mismatch")
        if row.text_geometry_metrics is not None:
            validate_pdf_text_geometry_report(row.text_geometry_metrics)
            if row.compared_pages != row.text_geometry_metrics["summary"]["pages"]:
                raise ValueError("text geometry metric page count mismatch")
        if row.status == "skip" and any(
            getattr(row, metric) is not None
            for metric in (
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
            )
        ):
            raise ValueError("skipped row has metrics")
        if row.status != "skip" and row.recall is None:
            raise ValueError("non-skip row is missing recall")
    if not is_finite_number(recall_min):
        raise ValueError(f"non-finite recall threshold: {recall_min}")
    if recall_min < 0:
        raise ValueError(f"negative recall threshold: {recall_min}")
    if recall_min > 1:
        raise ValueError(f"recall threshold above one: {recall_min}")
    visual_settings = validate_visual_settings(visual_settings)
    measured = [r for r in rows if r.recall is not None]
    integer_metric_rows = [
        r.integer_visual_metrics
        for r in measured
        if r.integer_visual_metrics is not None
    ]
    if integer_metric_rows and len(integer_metric_rows) != len(measured):
        raise ValueError("integer visual evidence is partial")
    integer_metric_summary = (
        aggregate_integer_metrics(integer_metric_rows)
        if integer_metric_rows
        else None
    )
    geometry_rows = [
        r.pdf_point_geometry for r in measured if r.pdf_point_geometry is not None
    ]
    if geometry_rows and len(geometry_rows) != len(measured):
        raise ValueError("PDF point geometry is partial")
    geometry_summary = (
        aggregate_pdf_geometry_reports(geometry_rows) if geometry_rows else None
    )
    semantic_rows = [
        r.semantic_text_metrics
        for r in measured
        if r.semantic_text_metrics is not None
    ]
    if semantic_rows and len(semantic_rows) != len(measured):
        raise ValueError("semantic text evidence is partial")
    semantic_summary = (
        aggregate_pdf_semantic_reports(semantic_rows) if semantic_rows else None
    )
    text_geometry_rows = [
        r.text_geometry_metrics
        for r in measured
        if r.text_geometry_metrics is not None
    ]
    if text_geometry_rows and len(text_geometry_rows) != len(measured):
        raise ValueError("text geometry evidence is partial")
    text_geometry_summary = (
        aggregate_pdf_text_geometry_reports(text_geometry_rows)
        if text_geometry_rows
        else None
    )
    summary = {
        "documents": len(rows),
        "measured": len(measured),
        "skipped": sum(1 for r in rows if r.status == "skip"),
        "below_recall_min": sum(
            1 for r in measured if r.recall is not None and r.recall < recall_min
        ),
        "recall_min": recall_min,
        # Whether the reference renderer reproduced itself. When false, the
        # visual metrics below are not comparable across runs; text recall and
        # the page-count ratio still are.
        "reference_stable": reference_stable,
        "unstable_references": sorted(unstable_references or []),
        "mean_recall": mean([r.recall for r in measured if r.recall is not None]),
        "mean_page_ratio": mean(
            [r.page_ratio for r in measured if r.page_ratio is not None]
        ),
        "mean_ahash_similarity": mean(
            [
                r.ahash_similarity
                for r in measured
                if r.ahash_similarity is not None
            ]
        ),
        "mean_page_ahash_similarity": mean(
            [
                r.mean_page_ahash_similarity
                for r in measured
                if r.mean_page_ahash_similarity is not None
            ]
        ),
        "mean_foreground_ink_iou": mean(
            [
                r.foreground_ink_iou
                for r in measured
                if r.foreground_ink_iou is not None
            ]
        ),
        "compared_pages": sum(
            r.compared_pages for r in measured if r.compared_pages is not None
        ),
        "unmatched_candidate_pages": sum(
            r.unmatched_candidate_pages
            for r in measured
            if r.unmatched_candidate_pages is not None
        ),
        "unmatched_reference_pages": sum(
            r.unmatched_reference_pages
            for r in measured
            if r.unmatched_reference_pages is not None
        ),
        "capped_matched_pages": sum(
            r.capped_matched_pages
            for r in measured
            if r.capped_matched_pages is not None
        ),
        "mean_render_warnings": mean(
            [
                r.render_warnings
                for r in measured
                if r.render_warnings is not None
            ]
        ),
    }
    return {
        "visual_comparison": {
            **visual_settings,
            "integer_metrics": integer_metric_contract(),
        },
        "integer_visual_metrics": integer_metric_summary,
        "pdf_diagnostic_contract": pdf_diagnostic_contract(),
        "pdf_point_geometry": geometry_summary,
        "semantic_text_metrics": semantic_summary,
        "text_geometry_metrics": text_geometry_summary,
        "summary": summary,
        "gate": validation_gate(summary, thresholds),
        "rows": [row_dict(r) for r in rows],
    }


def json_report_payload(report: dict) -> str:
    return json.dumps(report, ensure_ascii=False, indent=2, allow_nan=False)


def warning_kinds(report: dict | None) -> list[str] | None:
    if report is None:
        return None
    warnings = report.get("warnings", [])
    if not isinstance(warnings, list):
        return None
    kinds = []
    for warning in warnings:
        if not isinstance(warning, dict):
            return None
        kind = warning.get("kind")
        if (
            not isinstance(kind, str)
            or not kind
            or kind != kind.strip()
            or not kind.isascii()
            or not kind.isidentifier()
            or kind not in VALID_RENDER_WARNING_KINDS
        ):
            return None
        if kind in kinds:
            return None
        kinds.append(kind)
    return kinds


def render_rwml(
    src: Path,
    out: Path,
    report_out: Path | None = None,
    *,
    fixed_fonts: bool = True,
) -> dict | None:
    """Render via the crate's to_pdf example and return its JSON report."""
    cmd = [
        "cargo",
        "run",
        "--quiet",
        "--features",
        "render",
        "--example",
        "to_pdf",
        "--",
        str(src),
        str(out),
    ]
    if fixed_fonts:
        cmd.append("--fixed-fonts")
    if report_out is not None:
        cmd.extend(["--report-json", str(report_out)])
    r = subprocess.run(cmd, capture_output=True, env=rust_tool_environment())
    if not (r.returncode == 0 and out.exists() and out.stat().st_size > 0):
        return None
    if report_out is not None and report_out.exists():
        try:
            return json.loads(report_out.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return None
    return {}


def rust_tool_environment() -> dict[str, str]:
    env = os.environ.copy()
    rustup_bin = Path.home() / ".cargo" / "bin"
    cargo_name = "cargo.exe" if os.name == "nt" else "cargo"
    if (rustup_bin / cargo_name).is_file():
        current_path = env.get("PATH", "")
        env["PATH"] = (
            str(rustup_bin)
            if not current_path
            else str(rustup_bin) + os.pathsep + current_path
        )
    return env


def render_libreoffice(src: Path, outdir: Path, mode: str) -> Path | None:
    """Render via LibreOffice (`local` soffice or `docker` lo-cli)."""
    initialize_cmd: list[str] | None = None
    if mode == "docker":
        # Docker Desktop wants forward-slash host paths.
        d = src.parent.resolve().as_posix()
        o = Path(outdir).resolve().as_posix()
        cmd = [
            "docker", "run", "--rm", "-v", f"{d}:/data:ro", "-v", f"{o}:/out",
            "lo-cli", "soffice", "--headless", "--convert-to", "pdf",
            "--outdir", "/out", f"/data/{src.name}",
        ]
    else:
        profile_token = hashlib.sha256(
            str(src.resolve()).encode("utf-8")
        ).hexdigest()[:16]
        profile = outdir / f".rwml-lo-profile-{profile_token}"
        if profile.exists() or profile.is_symlink():
            raise RenderDependencyError(
                "LibreOffice validation requires a fresh per-document profile"
            )
        profile_user = profile / "user"
        profile_user.mkdir(parents=True)
        shutil.copyfile(
            LOCAL_ORACLE_PROFILE,
            profile_user / "registrymodifications.xcu",
        )
        profile_argument = f"-env:UserInstallation={profile.resolve().as_uri()}"
        initialize_cmd = [
            "soffice",
            profile_argument,
            "--headless",
            "--terminate_after_init",
        ]
        cmd = [
            "soffice", profile_argument,
            "--headless", "--convert-to", "pdf",
            "--outdir", str(outdir), str(src),
        ]
    try:
        if initialize_cmd is not None:
            initialized = subprocess.run(initialize_cmd, capture_output=True)
            if initialized.returncode != 0:
                return None
        r = subprocess.run(cmd, capture_output=True)
    except FileNotFoundError as exc:
        if mode == "docker":
            raise RenderDependencyError(
                "LibreOffice validation dependency required: docker executable "
                "not found; install Docker or pass --soffice local"
            ) from exc
        raise RenderDependencyError(
            "LibreOffice validation dependency required: soffice executable "
            "not found; install LibreOffice or pass --soffice docker"
        ) from exc
    out = outdir / (src.stem + ".pdf")
    return out if (r.returncode == 0 and out.exists()) else None


def resolve_input_paths(inputs: list[Path], manifest: Path | None) -> list[Path]:
    return resolve_input_campaign(inputs, manifest)[0]


def resolve_input_campaign(
    inputs: list[Path], manifest: Path | None
) -> tuple[list[Path], CorpusManifest | None]:
    if manifest is None:
        return inputs, None
    if inputs:
        raise ValueError("--manifest cannot be combined with positional inputs")
    if manifest.suffix.lower() == ".json":
        corpus = load_corpus_manifest(manifest)
        return [document.path for document in corpus.documents], corpus
    return manifest_document_inputs(manifest), None


def manifest_document_inputs(manifest: Path) -> list[Path]:
    if manifest.suffix.lower() == ".json":
        return [document.path for document in load_corpus_manifest(manifest).documents]
    header = None
    documents = []
    seen = set()
    for line in manifest.read_text(encoding="utf-8").splitlines():
        trimmed = line.strip()
        if not trimmed:
            continue
        if header is None:
            if not line.startswith("#"):
                raise ValueError(f"{manifest} does not start with a TSV path header")
            header = line[1:].lstrip(" ").split("\t")
            if not header or header[0] != "path":
                raise ValueError(f"{manifest} does not start with a TSV path header")
            continue
        if trimmed.startswith("#"):
            continue
        cols = line.split("\t")
        if len(cols) != len(header):
            raise ValueError(f"{manifest} row has {len(cols)} columns: {line}")
        document_path = cols[0]
        if unsafe_manifest_document_path(document_path):
            raise ValueError(f"{manifest} has unsafe document path: {document_path}")
        if document_path in seen:
            raise ValueError(f"{manifest} has duplicate document path: {document_path}")
        seen.add(document_path)
        document = manifest.parent / document_path
        if not document.is_file():
            raise ValueError(f"{manifest} document does not exist: {document_path}")
        documents.append(document)
    if header is None:
        raise ValueError(f"{manifest} is empty")
    if not documents:
        raise ValueError(f"{manifest} does not contain document rows")
    return documents


def unsafe_manifest_document_path(document_path: str) -> bool:
    return (
        not document_path
        or document_path != document_path.strip()
        or document_path.startswith(("/", "\\"))
        or "\\" in document_path
        or ":" in document_path
        or any(part in {"", ".", ".."} for part in document_path.split("/"))
        or any(char.isspace() for char in document_path)
    )


def corpus_document_map(
    corpus: CorpusManifest | None,
) -> dict[Path, CorpusDocument]:
    if corpus is None:
        return {}
    return {document.path: document for document in corpus.documents}


def row_identity(
    source: Path, documents: dict[Path, CorpusDocument]
) -> dict[str, object]:
    document = documents.get(source)
    if document is None:
        return {}
    return {
        "case_id": document.case_id,
        "input_bytes": document.input_bytes,
        "input_sha256": document.sha256,
    }


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        while chunk := source.read(1024 * 1024):
            digest.update(chunk)
    return digest.hexdigest()


def _command_text(command: list[str], *, cwd: Path | None = None) -> str:
    completed = subprocess.run(
        command,
        cwd=cwd,
        check=False,
        capture_output=True,
        text=True,
    )
    value = completed.stdout.strip()
    if completed.returncode != 0 or not value:
        raise RenderDependencyError(
            f"identity command failed: {command[0]}"
        )
    return value.splitlines()[0].strip()


def _source_identity(explicit_revision: str | None) -> tuple[str, bool]:
    if (
        explicit_revision is not None
        and re.fullmatch(r"[0-9a-f]{40}", explicit_revision) is None
    ):
        raise ValueError("source revision must be a full lowercase Git SHA")
    revision = _command_text(["git", "rev-parse", "HEAD"], cwd=REPO)
    if re.fullmatch(r"[0-9a-f]{40}", revision) is None:
        raise ValueError("source revision must be a full lowercase Git SHA")
    if explicit_revision is not None and explicit_revision != revision:
        raise ValueError("source revision does not match the current HEAD")
    completed = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=REPO,
        check=False,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise ValueError("source dirty state could not be determined")
    return revision, bool(completed.stdout)


def _harness_sha256() -> str:
    digest = hashlib.sha256()
    for path in (
        Path(__file__).resolve(),
        Path(__file__).with_name("render_oracle_contract.py").resolve(),
        Path(__file__).with_name("render_evidence_metrics.py").resolve(),
        Path(__file__).with_name("render_pdf_diagnostics.py").resolve(),
        Path(__file__).with_name("libreoffice_oracle_fonts.py").resolve(),
        LOCAL_ORACLE_PROFILE,
        LOCAL_ORACLE_FONT_LOCK,
    ):
        payload = path.read_bytes()
        name = path.name.encode("ascii")
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def _libreoffice_identity(mode: str) -> dict[str, str]:
    if mode == "local":
        executable = shutil.which("soffice")
        if executable is None:
            raise RenderDependencyError("soffice executable identity is unavailable")
        resolved = Path(executable).resolve()
        version = _command_text([str(resolved), "--version"])
        executable_sha256 = _sha256_file(resolved)
        font_lock = load_font_lock(LOCAL_ORACLE_FONT_LOCK)
        identity_mode = "local"
        material = {
            "mode": identity_mode,
            "version": version,
            "executable_sha256": executable_sha256,
            "profile_policy": "seeded-fresh-warmed-per-document-v1",
            "profile_sha256": _sha256_file(LOCAL_ORACLE_PROFILE),
            "font_lock_sha256": _sha256_file(LOCAL_ORACLE_FONT_LOCK),
            "font_bundle": installation_font_identity(resolved, font_lock),
        }
    else:
        image_id = _command_text(
            ["docker", "image", "inspect", "lo-cli", "--format", "{{.Id}}"]
        )
        version = _command_text(
            ["docker", "run", "--rm", "lo-cli", "soffice", "--version"]
        )
        identity_mode = "container"
        material = {"mode": identity_mode, "version": version, "image_id": image_id}
    identity_sha256 = hashlib.sha256(
        json.dumps(
            material,
            ensure_ascii=True,
            sort_keys=True,
            separators=(",", ":"),
        ).encode("ascii")
    ).hexdigest()
    return {
        "name": "libreoffice",
        "mode": identity_mode,
        "version": version,
        "identity_sha256": identity_sha256,
    }


def environment_identity(
    *,
    soffice_mode: str,
    font_mode: str,
    source_revision: str | None = None,
) -> dict[str, object]:
    revision, dirty = _source_identity(source_revision)
    pymupdf_version = getattr(fitz, "__version__", "unavailable")
    pillow_version = getattr(Image, "__version__", "unavailable")
    tools = [
        {"name": "pillow", "version": str(pillow_version)},
        {"name": "pymupdf", "version": str(pymupdf_version)},
        {"name": "python", "version": platform.python_version()},
    ]
    numpy = integer_metric_numpy()
    if numpy is not None:
        tools.append({"name": "numpy", "version": str(numpy.__version__)})
        tools.sort(key=lambda tool: tool["name"])
    return {
        "source_revision": revision,
        "source_dirty": dirty,
        "harness_sha256": _harness_sha256(),
        "cargo_lock_sha256": _sha256_file(REPO / "Cargo.lock"),
        "renderer": {"name": "rwml", "font_mode": font_mode},
        "oracle": _libreoffice_identity(soffice_mode),
        "platform": {
            "system": platform.system() or "unknown",
            "release": platform.release() or "unknown",
            "machine": platform.machine() or "unknown",
        },
        "tools": tools,
    }


def verify_campaign_inputs(
    corpus: CorpusManifest, environment: dict[str, object]
) -> None:
    """Reject observed source, tooling, or corpus drift before binding evidence."""
    _, dirty = _source_identity(environment["source_revision"])
    if dirty != environment["source_dirty"]:
        raise ValueError("source tree changed during the render campaign")
    if _harness_sha256() != environment["harness_sha256"]:
        raise ValueError("render harness changed during the campaign")
    if _sha256_file(REPO / "Cargo.lock") != environment["cargo_lock_sha256"]:
        raise ValueError("Cargo.lock changed during the render campaign")
    if load_corpus_manifest(corpus.path) != corpus:
        raise ValueError("render corpus changed during the campaign")


_HEX_STRING = re.compile(rb"<([0-9A-Fa-f\s]*)>")
_CODESPACE = re.compile(
    rb"begincodespacerange(.*?)endcodespacerange", re.S
)
_BFCHAR = re.compile(rb"beginbfchar(.*?)endbfchar", re.S)
_BFRANGE = re.compile(rb"beginbfrange(.*?)endbfrange", re.S)
_ACTUAL_TEXT = re.compile(rb"/ActualText\s*<([0-9A-Fa-f\s]*)>")


def _hex_to_text(raw: bytes) -> str:
    """Decode a CMap destination (UTF-16BE, optionally BOM-prefixed)."""
    digits = b"".join(raw.split())
    if len(digits) % 2:
        return ""
    try:
        data = bytes.fromhex(digits.decode("ascii"))
    except ValueError:
        return ""
    if data[:2] == b"\xfe\xff":
        data = data[2:]
    if len(data) % 2:
        return ""
    try:
        return data.decode("utf-16-be")
    except UnicodeDecodeError:
        return ""


def _hex_to_int(raw: bytes) -> int | None:
    digits = b"".join(raw.split())
    try:
        return int(digits, 16)
    except ValueError:
        return None


def parse_tounicode_cmap(data: bytes) -> dict[str, object]:
    """Read a `ToUnicode` CMap into a code width and a code-to-text map.

    Returns an empty map rather than raising on anything unrecognized: this
    feeds a measurement, so failing to decode must never invent text.
    """
    width = 2
    codespace = _CODESPACE.search(data)
    if codespace:
        first = _HEX_STRING.search(codespace.group(1))
        if first:
            digits = b"".join(first.group(1).split())
            if digits:
                width = max(1, len(digits) // 2)
    mapping: dict[int, str] = {}
    for block in _BFCHAR.findall(data):
        entries = _HEX_STRING.findall(block)
        for index in range(0, len(entries) - 1, 2):
            code = _hex_to_int(entries[index])
            text = _hex_to_text(entries[index + 1])
            if code is not None and text:
                mapping[code] = text
    for block in _BFRANGE.findall(data):
        for low_raw, high_raw, dest in _bfrange_entries(block):
            low = _hex_to_int(low_raw)
            high = _hex_to_int(high_raw)
            if low is None or high is None or high < low or high - low > 0xFFFF:
                continue
            if isinstance(dest, list):
                for offset, item in enumerate(dest):
                    text = _hex_to_text(item)
                    if text:
                        mapping[low + offset] = text
                continue
            base = _hex_to_text(dest)
            if not base:
                continue
            for offset in range(high - low + 1):
                mapping[low + offset] = base[:-1] + chr(ord(base[-1]) + offset)
    return {"width": width, "map": mapping}


def _bfrange_entries(block: bytes):
    """Yield `(low, high, destination)` triples from a `bfrange` block."""
    position = 0
    while True:
        low = _HEX_STRING.search(block, position)
        if not low:
            return
        high = _HEX_STRING.search(block, low.end())
        if not high:
            return
        rest = block[high.end() :]
        array = re.match(rb"\s*\[(.*?)\]", rest, re.S)
        if array:
            yield low.group(1), high.group(1), _HEX_STRING.findall(array.group(1))
            position = high.end() + array.end()
            continue
        dest = _HEX_STRING.search(block, high.end())
        if not dest:
            return
        yield low.group(1), high.group(1), dest.group(1)
        position = dest.end()


_STRING_OPERAND = re.compile(rb"\((?:\\.|[^\\()])*\)|<[0-9A-Fa-f\s]*>", re.S)
_TOKEN = re.compile(
    rb"/[^\s/<>\[\]()]+|<<|>>|\[|\]|\((?:\\.|[^\\()])*\)|<[0-9A-Fa-f\s]*>|[^\s/<>\[\]()]+",
    re.S,
)


def _decode_pdf_string(raw: bytes) -> bytes:
    if raw.startswith(b"<"):
        digits = b"".join(raw[1:-1].split())
        if len(digits) % 2:
            digits += b"0"
        try:
            return bytes.fromhex(digits.decode("ascii"))
        except ValueError:
            return b""
    body = raw[1:-1]
    out = bytearray()
    index = 0
    escapes = {b"n": 10, b"r": 13, b"t": 9, b"b": 8, b"f": 12}
    while index < len(body):
        byte = body[index : index + 1]
        if byte != b"\\":
            out += byte
            index += 1
            continue
        index += 1
        if index >= len(body):
            break
        nxt = body[index : index + 1]
        if nxt in escapes:
            out.append(escapes[nxt])
            index += 1
        elif nxt.isdigit():
            digits = b""
            while index < len(body) and len(digits) < 3 and body[index : index + 1].isdigit():
                digits += body[index : index + 1]
                index += 1
            out.append(int(digits, 8) & 0xFF)
        else:
            out += nxt
            index += 1
    return bytes(out)


def content_stream_text(content: bytes, cmaps: dict) -> str:
    """Reconstruct a page's text the way a conforming reader would.

    Glyph codes are mapped through each font's `ToUnicode`, and any
    `/Span <</ActualText ...>>` marked-content section contributes its declared
    text instead of its glyphs. No separator is invented: word spacing comes
    only from space glyphs the page actually draws.
    """
    pieces: list[str] = []
    font: str | None = None
    span_depth = 0
    pending_actual: str | None = None
    operands: list[bytes] = []
    for match in _TOKEN.finditer(content):
        token = match.group(0)
        if token in (b"Tf",):
            names = [item for item in operands if item.startswith(b"/")]
            font = names[-1][1:].decode("latin-1") if names else font
            operands = []
            continue
        if token == b"BDC":
            joined = b" ".join(operands)
            actual = _ACTUAL_TEXT.search(joined)
            if actual is not None:
                span_depth += 1
                pending_actual = _hex_to_text(actual.group(1))
            operands = []
            continue
        if token in (b"BT", b"ET"):
            # Each text object is a separately positioned run, so it cannot
            # continue a word; shaped clusters always stay inside one object.
            pieces.append(" ")
            operands = []
            continue
        if token == b"EMC":
            if span_depth:
                span_depth -= 1
                if pending_actual:
                    pieces.append(pending_actual)
                pending_actual = None
            operands = []
            continue
        if token in (b"Tj", b"TJ", b"'", b'"'):
            if span_depth == 0:
                entry = cmaps.get(font or "")
                if entry:
                    width = int(entry.get("width", 2)) or 2
                    mapping = entry.get("map", {})
                    for operand in operands:
                        if not _STRING_OPERAND.fullmatch(operand):
                            continue
                        raw = _decode_pdf_string(operand)
                        for index in range(0, len(raw) - width + 1, width):
                            code = int.from_bytes(raw[index : index + width], "big")
                            pieces.append(mapping.get(code, ""))
            operands = []
            continue
        if re.fullmatch(rb"[A-Za-z*'\"]+[01]?", token) and not token.startswith(b"/"):
            operands = []
            continue
        operands.append(token)
    return " ".join("".join(pieces).split())


def _is_rtl_char(ch: str) -> bool:
    code = ord(ch)
    # Hebrew, Arabic, Syriac, Thaana, N'Ko and the Arabic supplements.
    return 0x0590 <= code <= 0x08FF or 0xFB1D <= code <= 0xFDFF or 0xFE70 <= code <= 0xFEFF


def reverse_rtl_runs(text: str) -> str:
    """Put right-to-left runs back into logical order.

    Content streams draw glyphs in visual order. Reversing each maximal
    right-to-left run recovers logical order for runs that carry no embedded
    left-to-right text or digits, which is the case this measurement needs;
    it is a deliberate simplification of the Unicode bidirectional algorithm.
    """
    out: list[str] = []
    run: list[str] = []
    for ch in text:
        if _is_rtl_char(ch):
            run.append(ch)
            continue
        if run:
            out.extend(reversed(run))
            run = []
        out.append(ch)
    out.extend(reversed(run))
    return "".join(out)


def page_tounicode_cmaps(doc, page) -> dict:
    """Map each of a page's font resource names to its decoded `ToUnicode`."""
    cmaps: dict[str, dict] = {}
    try:
        fonts = page.get_fonts(full=True)
    except Exception:
        return cmaps
    for font in fonts:
        try:
            xref = int(font[0])
            resource = str(font[4])
            key = doc.xref_get_key(xref, "ToUnicode")
        except Exception:
            continue
        if not key or key[0] != "xref":
            continue
        try:
            stream = doc.xref_stream(int(str(key[1]).split()[0]))
        except Exception:
            continue
        if stream:
            cmaps[resource] = parse_tounicode_cmap(stream)
    return cmaps


def conforming_tokens(doc) -> list[str]:
    """Tokens a reader that honors `ActualText` recovers from the page text.

    PyMuPDF, like pdfminer.six and pypdf, ignores `ActualText` marked content
    and splits complex-script words apart at every span boundary. Rebuilding the
    text from the content stream recovers what Acrobat or Chrome would copy.
    """
    out: list[str] = []
    for page in doc:
        try:
            content = page.read_contents()
        except Exception:
            continue
        if not content:
            continue
        text = content_stream_text(content, page_tounicode_cmaps(doc, page))
        out.extend(reverse_rtl_runs(text).split())
    return out


def extracted_text_tokens(text: str) -> list[str]:
    """Split extracted PDF text, treating NUL layout separators as whitespace."""
    return text.replace("\x00", " ").split()


def tokens(pdf: Path) -> list[str]:
    require_pdf_deps()
    doc = fitz.open(pdf)
    text = " ".join(p.get_text() for p in doc)
    return extracted_text_tokens(text)


def candidate_tokens(pdf: Path) -> list[str]:
    """What rwml's PDF yields to any legitimate reader.

    The reference stays whatever a plain reader reports, because it is the
    oracle. For the candidate a token also counts when only an `ActualText`
    aware reader recovers it — that text is genuinely in the PDF, and both
    paths read the file rather than inventing anything.
    """
    require_pdf_deps()
    doc = fitz.open(pdf)
    text = " ".join(p.get_text() for p in doc)
    return extracted_text_tokens(text) + conforming_tokens(doc)


def reference_recall_tokens(
    raw_tokens: list[str],
    render_warning_kinds: list[str] | None = None,
    render_report: dict | None = None,
) -> list[str]:
    tokens = []
    index = 0
    ole_labels = (
        report_unsupported_count(render_report, "ole_objects")
        if render_warning_kinds
        and "OleObjectsPreservedButNotModeled" in render_warning_kinds
        else 0
    )
    missing_reference = ["Error:", "Reference", "source", "not", "found"]
    while index < len(raw_tokens):
        if raw_tokens[index : index + len(missing_reference)] == missing_reference:
            index += len(missing_reference)
            continue
        path_span = volatile_reference_path_span(raw_tokens, index)
        if path_span:
            index += path_span
            continue
        if (
            ole_labels > 0
            and raw_tokens[index] == "Object"
            and index + 1 < len(raw_tokens)
            and raw_tokens[index + 1].isascii()
            and raw_tokens[index + 1].isdigit()
        ):
            ole_labels -= 1
            index += 2
            continue
        token = raw_tokens[index]
        if not is_volatile_reference_shape_placeholder_token(
            token, render_warning_kinds
        ):
            tokens.append(token)
        index += 1
    return tokens


def is_volatile_reference_path_token(token: str) -> bool:
    value = normalized_reference_path_token(token)
    if not value:
        return False
    lower = value.lower()
    if is_absolute_reference_path_token(value):
        return True
    if "/" in value and lower.endswith(OFFICE_DOCUMENT_EXTENSIONS):
        return True
    if "\\" in value and lower.endswith(OFFICE_DOCUMENT_EXTENSIONS):
        return True
    return False


def volatile_reference_path_span(raw_tokens: list[str], index: int) -> int:
    value = normalized_reference_path_token(raw_tokens[index])
    if not is_volatile_reference_path_token(value):
        return 0
    if not is_absolute_reference_path_token(value) or is_office_document_token(value):
        return 1

    limit = min(len(raw_tokens), index + MAX_VOLATILE_REFERENCE_PATH_TOKENS)
    for end in range(index + 1, limit):
        continuation = normalized_reference_path_token(raw_tokens[end])
        if not continuation:
            break
        if is_office_document_token(continuation):
            return end - index + 1
        if "/" not in continuation and "\\" not in continuation:
            break
    return 1


def normalized_reference_path_token(token: str) -> str:
    return token.strip(" \t\r\n\"'`.,;:()[]{}<>")


def is_absolute_reference_path_token(value: str) -> bool:
    return value.startswith(("/", "~/", "\\\\")) or (
        len(value) >= 3 and value[1] == ":" and value[2] in {"/", "\\"}
    )


def is_office_document_token(value: str) -> bool:
    return value.lower().endswith(OFFICE_DOCUMENT_EXTENSIONS)


def is_volatile_reference_shape_placeholder_token(
    token: str,
    render_warning_kinds: list[str] | None,
) -> bool:
    if token != "[shape]" or not render_warning_kinds:
        return False
    return any(kind in UNSUPPORTED_OBJECT_WARNING_KINDS for kind in render_warning_kinds)


def token_recall(
    ref_tokens: list[str],
    got_tokens: list[str],
    render_report: dict | None = None,
) -> float:
    if not ref_tokens:
        return 1.0
    got_set = set(got_tokens)
    hit = sum(
        1
        for token in ref_tokens
        if reference_token_recalled(token, got_tokens, got_set, render_report)
    )
    return hit / len(ref_tokens)


def reference_token_recalled(
    token: str,
    got_tokens: list[str],
    got_set: set[str],
    render_report: dict | None,
) -> bool:
    if token in got_set:
        return True
    if report_has_tracked_changes(render_report) and tracked_reference_token_recalled(
        token, got_tokens
    ):
        return True
    if report_unsupported_count(render_report, "footnotes") > 0 and joined_note_marker_recalled(
        token, got_set
    ):
        return True
    if split_rtl_list_marker_recalled(token, got_set):
        return True
    if adjacent_rtl_transposition_recalled(token, got_set):
        return True
    return False


def report_unsupported_count(report: dict | None, key: str) -> int:
    if not isinstance(report, dict):
        return 0
    unsupported = report.get("unsupported")
    if not isinstance(unsupported, dict):
        return 0
    value = unsupported.get(key, 0)
    return value if isinstance(value, int) and not isinstance(value, bool) else 0


def report_has_tracked_changes(report: dict | None) -> bool:
    return any(
        report_unsupported_count(report, key) > 0
        for key in (
            "tracked_insertions",
            "tracked_deletions",
            "tracked_moves",
            "tracked_property_changes",
        )
    )


def tracked_reference_token_recalled(token: str, got_tokens: list[str]) -> bool:
    value = token.strip(" \t\r\n\"'`.,;:()[]{}<>")
    if len(value) < 5:
        return False
    needle = value.lower()
    fragments = []
    for got in got_tokens:
        fragments.extend(visible_token_fragments(got))
    matches = {
        fragment.lower()
        for fragment in fragments
        if len(fragment) >= 4 and fragment.lower() in needle
    }
    if len(matches) >= 2:
        return True
    return any(
        len(fragment) >= 5
        and len(value) > len(fragment)
        and needle.endswith(fragment.lower())
        for fragment in matches
    )


def visible_token_fragments(token: str) -> list[str]:
    value = token.strip(" \t\r\n\"'`.,;:()[]{}<>")
    if not value:
        return []
    return re.findall(r"[A-Z]+(?=[A-Z][a-z]|$)|[A-Z]?[a-z]+|\d+", value)


def joined_note_marker_recalled(token: str, got_set: set[str]) -> bool:
    value = token.strip(" \t\r\n\"'`.,;:()[]{}<>")
    if len(value) < 2:
        return False
    if value[-1].isdigit() and value[:-1] in got_set:
        return True
    return value[0].isdigit() and value[1:] in got_set


def split_rtl_list_marker_recalled(token: str, got_set: set[str]) -> bool:
    """Accept a list period split from an adjacent RTL label word.

    LibreOffice can expose a right-to-left list label as ``.word`` while the
    candidate's ActualText-aware content stream exposes ``word`` and ``.`` as
    separate tokens. Both are the same visible marker/text pair; only accept
    this normalization for RTL words and an explicitly present period.
    """
    value = token.strip(" \t\r\n\"'`(),;:[]{}<>")
    if not value.startswith(".") or len(value) == 1:
        return False
    word = value[1:]
    return "." in got_set and word in got_set and any(_is_rtl_char(ch) for ch in word)


def adjacent_rtl_transposition_recalled(token: str, got_set: set[str]) -> bool:
    if not 3 <= len(token) <= 128 or not all(_is_rtl_char(ch) for ch in token):
        return False
    for candidate in got_set:
        if len(candidate) != len(token) or not all(
            _is_rtl_char(ch) for ch in candidate
        ):
            continue
        differences = [
            index
            for index, (reference, rendered) in enumerate(zip(token, candidate))
            if reference != rendered
        ]
        if (
            len(differences) == 2
            and differences[1] == differences[0] + 1
            and token[differences[0]] == candidate[differences[1]]
            and token[differences[1]] == candidate[differences[0]]
        ):
            return True
    return False


def text_recall(
    ref: Path,
    got: Path,
    render_warning_kinds: list[str] | None = None,
    render_report: dict | None = None,
) -> float:
    ref_tokens = reference_recall_tokens(
        tokens(ref), render_warning_kinds, render_report
    )
    return token_recall(ref_tokens, candidate_tokens(got), render_report)


def page_count(pdf: Path) -> int:
    require_pdf_deps()
    return fitz.open(pdf).page_count


def pymupdf_page_geometry(page) -> dict[str, int]:
    rect = page.rect
    media = page.mediabox
    crop = page.cropbox
    return canonical_pdf_page_geometry(
        page_size=(rect.width, rect.height),
        media_box=(media.x0, media.y0, media.x1, media.y1),
        crop_box=(crop.x0, crop.y0, crop.x1, crop.y1),
        rotation_degrees=page.rotation,
    )


def pymupdf_page_semantic_tokens(
    page, *, max_codepoints: int, max_tokens: int
) -> tuple[str, ...]:
    return normalize_pdf_semantic_tokens(
        page.get_text(),
        max_codepoints=max_codepoints,
        max_tokens=max_tokens,
    )


def pymupdf_page_text_boxes(
    page,
    *,
    max_items: int,
    max_codepoints: int,
    max_tokens: int,
) -> tuple[tuple[PdfSemanticTextBox, ...], tuple[PdfSemanticTextBox, ...], int, int]:
    if (
        not isinstance(max_items, int)
        or isinstance(max_items, bool)
        or not 1 <= max_items <= MAX_TEXT_GEOMETRY_ITEMS
    ):
        raise ValueError("text geometry item limit is invalid")
    records = page.get_text("words", sort=False)
    if not isinstance(records, (list, tuple)) or len(records) > max_items:
        raise ValueError("text geometry word item limit exceeded")
    words = []
    line_groups: dict[tuple[int, int], list[tuple[int, int, PdfSemanticTextBox]]] = {}
    used_codepoints = 0
    used_tokens = 0
    for order, record in enumerate(records):
        if not isinstance(record, (list, tuple)) or len(record) < 8:
            raise ValueError("PyMuPDF word record is invalid")
        block_number, line_number, word_number = record[5:8]
        if any(
            not isinstance(value, int) or isinstance(value, bool) or value < 0
            for value in (block_number, line_number, word_number)
        ):
            raise ValueError("PyMuPDF word record index is invalid")
        tokens = normalize_pdf_semantic_tokens(
            record[4],
            max_codepoints=max_codepoints - used_codepoints,
            max_tokens=max_tokens - used_tokens,
        )
        if not tokens:
            continue
        box = canonical_pdf_text_box(tokens, record[:4])
        words.append(box)
        used_codepoints += sum(map(len, tokens))
        used_tokens += len(tokens)
        line_groups.setdefault((block_number, line_number), []).append(
            (word_number, order, box)
        )
    if len(words) > max_items or len(line_groups) > max_items:
        raise ValueError("text geometry item limit exceeded")
    lines = []
    for entries in line_groups.values():
        entries.sort(key=lambda entry: (entry[0], entry[1]))
        boxes = [entry[2] for entry in entries]
        line_tokens = tuple(token for box in boxes for token in box.tokens)
        line_bbox = (
            min(box.bbox_millipoints[0] for box in boxes),
            min(box.bbox_millipoints[1] for box in boxes),
            max(box.bbox_millipoints[2] for box in boxes),
            max(box.bbox_millipoints[3] for box in boxes),
        )
        lines.append(PdfSemanticTextBox(line_tokens, line_bbox))
    return tuple(words), tuple(lines), used_codepoints, used_tokens


def opaque_rgb(image):
    if image.mode in {"RGBA", "LA"} or "transparency" in image.info:
        rgba = image.convert("RGBA")
        background = Image.new("RGBA", rgba.size, (255, 255, 255, 255))
        background.alpha_composite(rgba)
        return background.convert("RGB")
    return image.convert("RGB")


def ensure_pixel_budget(width: int, height: int, limit: int, context: str) -> None:
    if width < 1 or height < 1 or width > limit or height > limit or width * height > limit:
        raise VisualMetricError(
            f"{context} exceeds the {limit}-pixel safety limit: {width}x{height}"
        )


def normalize_page_pair(reference, candidate):
    """White-pad both images to one canvas without scaling either page."""
    reference = opaque_rgb(reference)
    candidate = opaque_rgb(candidate)
    width = max(reference.width, candidate.width)
    height = max(reference.height, candidate.height)
    ensure_pixel_budget(width, height, MAX_NORMALIZED_CANVAS_PIXELS, "normalized canvas")
    normalized_reference = Image.new("RGB", (width, height), color="white")
    normalized_candidate = Image.new("RGB", (width, height), color="white")
    normalized_reference.paste(reference, (0, 0))
    normalized_candidate.paste(candidate, (0, 0))
    return normalized_reference, normalized_candidate


def image_ahash(image, size: int = DEFAULT_AHASH_SIZE) -> int:
    if not isinstance(size, int) or isinstance(size, bool) or not 1 <= size <= MAX_AHASH_SIZE:
        raise ValueError(f"aHash size is out of range: {size}")
    grayscale = image.convert("L").resize((size, size))
    pixels = list(grayscale.tobytes())
    average = sum(pixels) / len(pixels)
    bits = 0
    for index, value in enumerate(pixels):
        if value >= average:
            bits |= 1 << index
    return bits


def image_hash_similarity(reference, candidate, size: int = DEFAULT_AHASH_SIZE) -> float:
    reference, candidate = normalize_page_pair(reference, candidate)
    return normalized_image_hash_similarity(reference, candidate, size=size)


def normalized_image_hash_similarity(
    reference, candidate, size: int = DEFAULT_AHASH_SIZE
) -> float:
    difference = image_ahash(reference, size=size) ^ image_ahash(candidate, size=size)
    return 1.0 - bin(difference).count("1") / (size * size)


def foreground_ink_iou_images(reference, candidate, threshold: int) -> float:
    if (
        not isinstance(threshold, int)
        or isinstance(threshold, bool)
        or not 0 <= threshold <= 255
    ):
        raise ValueError(f"foreground threshold is out of range: {threshold}")
    reference, candidate = normalize_page_pair(reference, candidate)
    return normalized_foreground_ink_iou(reference, candidate, threshold)


def normalized_foreground_ink_iou(reference, candidate, threshold: int) -> float:
    ink_lut = [255 if value < threshold else 0 for value in range(256)]
    reference_mask = reference.convert("L").point(ink_lut)
    candidate_mask = candidate.convert("L").point(ink_lut)
    intersection = ImageChops.darker(reference_mask, candidate_mask).histogram()[255]
    union = ImageChops.lighter(reference_mask, candidate_mask).histogram()[255]
    if union == 0:
        return 1.0
    return intersection / union


def compare_page_images(
    reference_pages: list,
    candidate_pages: list,
    *,
    page_cap: int,
    foreground_threshold: int,
    ahash_size: int,
    reference_page_count: int | None = None,
    candidate_page_count: int | None = None,
) -> VisualMetrics:
    settings = validate_visual_settings(
        {
            "page_cap": page_cap,
            "foreground_threshold": foreground_threshold,
            "ahash_size": ahash_size,
        }
    )
    reference_page_count = (
        len(reference_pages) if reference_page_count is None else reference_page_count
    )
    candidate_page_count = (
        len(candidate_pages) if candidate_page_count is None else candidate_page_count
    )
    for name, count, available in (
        ("reference", reference_page_count, len(reference_pages)),
        ("candidate", candidate_page_count, len(candidate_pages)),
    ):
        if (
            not isinstance(count, int)
            or isinstance(count, bool)
            or count < available
        ):
            raise ValueError(f"{name} page count is invalid: {count}")
    compared_pages = min(
        reference_page_count,
        candidate_page_count,
        settings["page_cap"],
        len(reference_pages),
        len(candidate_pages),
    )
    page_hashes = []
    page_ink_ious = []
    integer_pages = []
    for index in range(compared_pages):
        reference_page, candidate_page = normalize_page_pair(
            reference_pages[index], candidate_pages[index]
        )
        page_hashes.append(
            normalized_image_hash_similarity(
                reference_page,
                candidate_page,
                size=settings["ahash_size"],
            )
        )
        page_ink_ious.append(
            normalized_foreground_ink_iou(
                reference_page,
                candidate_page,
                threshold=settings["foreground_threshold"],
            )
        )
        integer_pages.append(
            integer_image_metrics(
                reference_page.tobytes(),
                candidate_page.tobytes(),
                reference_page.width,
                reference_page.height,
                max_metric_work_units=MAX_INTEGER_METRIC_WORK_UNITS,
            )
        )
    return visual_metrics_from_scores(
        page_hashes,
        page_ink_ious,
        integer_pages,
        reference_page_count=reference_page_count,
        candidate_page_count=candidate_page_count,
        page_cap=settings["page_cap"],
    )


def visual_metrics_from_scores(
    page_hashes: list[float],
    page_ink_ious: list[float],
    integer_pages: list[dict[str, int]] | None,
    *,
    reference_page_count: int,
    candidate_page_count: int,
    page_cap: int,
    pdf_point_geometry: dict[str, object] | None = None,
    semantic_text_metrics: dict[str, int] | None = None,
    text_geometry_metrics: dict[str, object] | None = None,
) -> VisualMetrics:
    if len(page_hashes) != len(page_ink_ious):
        raise ValueError("visual page metric count mismatch")
    if integer_pages is not None and len(page_hashes) != len(integer_pages):
        raise ValueError("integer visual page metric count mismatch")
    return VisualMetrics(
        mean_page_ahash_similarity=mean(page_hashes),
        foreground_ink_iou=mean(page_ink_ious),
        compared_pages=len(page_hashes),
        unmatched_candidate_pages=max(0, candidate_page_count - reference_page_count),
        unmatched_reference_pages=max(0, reference_page_count - candidate_page_count),
        capped_matched_pages=max(
            0,
            min(reference_page_count, candidate_page_count) - page_cap,
        ),
        integer_visual_metrics=(
            aggregate_integer_metrics(integer_pages) if integer_pages else None
        ),
        pdf_point_geometry=pdf_point_geometry,
        semantic_text_metrics=semantic_text_metrics,
        text_geometry_metrics=text_geometry_metrics,
    )


def rasterize_pdf_page(document, index: int, *, dpi: int, pdf_name: str):
    page = document[index]
    scale = dpi / 72.0
    predicted_width = max(1, math.ceil(abs(page.rect.width) * scale))
    predicted_height = max(1, math.ceil(abs(page.rect.height) * scale))
    ensure_pixel_budget(
        predicted_width,
        predicted_height,
        MAX_RASTER_PAGE_PIXELS,
        f"raster page {index + 1} of {pdf_name}",
    )
    pixmap = page.get_pixmap(dpi=dpi, alpha=False)
    ensure_pixel_budget(
        pixmap.width,
        pixmap.height,
        MAX_RASTER_PAGE_PIXELS,
        f"raster page {index + 1} of {pdf_name}",
    )
    return Image.frombytes("RGB", (pixmap.width, pixmap.height), pixmap.samples)


def rasterize_pdf_pages(pdf: Path, *, dpi: int, page_cap: int) -> tuple[list, int]:
    if fitz is None or Image is None:
        raise VisualMetricError("PyMuPDF and Pillow are required for page rasterization")
    settings = validate_visual_settings({"dpi": dpi, "page_cap": page_cap})
    try:
        with fitz.open(pdf) as document:
            page_count_value = document.page_count
            pages = []
            buffered_pixels = 0
            for index in range(min(page_count_value, settings["page_cap"])):
                page = rasterize_pdf_page(
                    document,
                    index,
                    dpi=settings["dpi"],
                    pdf_name=pdf.name,
                )
                buffered_pixels += page.width * page.height
                ensure_pixel_budget(
                    buffered_pixels,
                    1,
                    MAX_BUFFERED_RASTER_PIXELS,
                    f"buffered raster pages of {pdf.name}",
                )
                pages.append(page)
            return pages, page_count_value
    except VisualMetricError:
        raise
    except Exception as exc:
        raise VisualMetricError(f"rasterization failed for {pdf.name}: {exc}") from exc


def compare_pdf_visuals(
    reference: Path,
    candidate: Path,
    *,
    dpi: int,
    page_cap: int,
    foreground_threshold: int,
    ahash_size: int,
) -> VisualMetrics:
    if fitz is None or Image is None or ImageChops is None:
        raise VisualMetricError("PyMuPDF and Pillow are required for page rasterization")
    settings = validate_visual_settings(
        {
            "dpi": dpi,
            "page_cap": page_cap,
            "foreground_threshold": foreground_threshold,
            "ahash_size": ahash_size,
        }
    )
    try:
        with fitz.open(reference) as reference_document, fitz.open(
            candidate
        ) as candidate_document:
            reference_page_count = reference_document.page_count
            candidate_page_count = candidate_document.page_count
            compared_pages = min(
                reference_page_count,
                candidate_page_count,
                settings["page_cap"],
            )
            page_hashes = []
            page_ink_ious = []
            integer_pages = []
            geometry_pages = []
            semantic_pages = []
            text_geometry_pages = []
            reference_codepoints = 0
            reference_tokens = 0
            candidate_codepoints = 0
            candidate_tokens = 0
            reference_box_codepoints = 0
            reference_box_tokens = 0
            candidate_box_codepoints = 0
            candidate_box_tokens = 0
            for index in range(compared_pages):
                reference_source_page = reference_document[index]
                candidate_source_page = candidate_document[index]
                geometry_pages.append(
                    pdf_page_geometry_metrics(
                        pymupdf_page_geometry(reference_source_page),
                        pymupdf_page_geometry(candidate_source_page),
                    )
                )
                reference_page_tokens = pymupdf_page_semantic_tokens(
                    reference_source_page,
                    max_codepoints=MAX_SEMANTIC_CODEPOINTS - reference_codepoints,
                    max_tokens=MAX_SEMANTIC_TOKENS - reference_tokens,
                )
                candidate_page_tokens = pymupdf_page_semantic_tokens(
                    candidate_source_page,
                    max_codepoints=MAX_SEMANTIC_CODEPOINTS - candidate_codepoints,
                    max_tokens=MAX_SEMANTIC_TOKENS - candidate_tokens,
                )
                reference_codepoints += sum(map(len, reference_page_tokens))
                reference_tokens += len(reference_page_tokens)
                candidate_codepoints += sum(map(len, candidate_page_tokens))
                candidate_tokens += len(candidate_page_tokens)
                semantic_pages.append(
                    pdf_semantic_metrics(
                        reference_page_tokens, candidate_page_tokens
                    )
                )
                (
                    reference_word_boxes,
                    reference_line_boxes,
                    used_reference_box_codepoints,
                    used_reference_box_tokens,
                ) = pymupdf_page_text_boxes(
                    reference_source_page,
                    max_items=MAX_TEXT_GEOMETRY_ITEMS,
                    max_codepoints=(
                        MAX_SEMANTIC_CODEPOINTS - reference_box_codepoints
                    ),
                    max_tokens=MAX_SEMANTIC_TOKENS - reference_box_tokens,
                )
                (
                    candidate_word_boxes,
                    candidate_line_boxes,
                    used_candidate_box_codepoints,
                    used_candidate_box_tokens,
                ) = pymupdf_page_text_boxes(
                    candidate_source_page,
                    max_items=MAX_TEXT_GEOMETRY_ITEMS,
                    max_codepoints=(
                        MAX_SEMANTIC_CODEPOINTS - candidate_box_codepoints
                    ),
                    max_tokens=MAX_SEMANTIC_TOKENS - candidate_box_tokens,
                )
                reference_box_codepoints += used_reference_box_codepoints
                reference_box_tokens += used_reference_box_tokens
                candidate_box_codepoints += used_candidate_box_codepoints
                candidate_box_tokens += used_candidate_box_tokens
                text_geometry_pages.append(
                    pdf_text_geometry_page(
                        reference_word_boxes,
                        candidate_word_boxes,
                        reference_line_boxes,
                        candidate_line_boxes,
                    )
                )
                reference_page = rasterize_pdf_page(
                    reference_document,
                    index,
                    dpi=settings["dpi"],
                    pdf_name=reference.name,
                )
                candidate_page = rasterize_pdf_page(
                    candidate_document,
                    index,
                    dpi=settings["dpi"],
                    pdf_name=candidate.name,
                )
                reference_page, candidate_page = normalize_page_pair(
                    reference_page, candidate_page
                )
                page_hashes.append(
                    normalized_image_hash_similarity(
                        reference_page,
                        candidate_page,
                        size=settings["ahash_size"],
                    )
                )
                page_ink_ious.append(
                    normalized_foreground_ink_iou(
                        reference_page,
                        candidate_page,
                        threshold=settings["foreground_threshold"],
                    )
                )
                integer_pages.append(
                    integer_image_metrics(
                        reference_page.tobytes(),
                        candidate_page.tobytes(),
                        reference_page.width,
                        reference_page.height,
                        max_metric_work_units=MAX_INTEGER_METRIC_WORK_UNITS,
                    )
                )
            return visual_metrics_from_scores(
                page_hashes,
                page_ink_ious,
                integer_pages,
                reference_page_count=reference_page_count,
                candidate_page_count=candidate_page_count,
                page_cap=settings["page_cap"],
                pdf_point_geometry=(
                    pdf_geometry_report(geometry_pages) if geometry_pages else None
                ),
                semantic_text_metrics=(
                    pdf_semantic_report(semantic_pages) if semantic_pages else None
                ),
                text_geometry_metrics=(
                    pdf_text_geometry_report(text_geometry_pages)
                    if text_geometry_pages
                    else None
                ),
            )
    except VisualMetricError:
        raise
    except Exception as exc:
        raise VisualMetricError(
            f"visual diagnostics failed for {reference.name} / {candidate.name}: {exc}"
        ) from exc


def ahash(pdf: Path, page: int = 0, size: int = 16) -> int:
    require_pdf_deps()
    doc = fitz.open(pdf)
    if page >= doc.page_count:
        return 0
    pix = doc[page].get_pixmap(dpi=72)
    img = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    return image_ahash(img, size=size)


def hash_similarity(ref: Path, got: Path, size: int = 16) -> float:
    a, b = ahash(ref, size=size), ahash(got, size=size)
    ham = bin(a ^ b).count("1")
    return 1.0 - ham / (size * size)


def captured_validation_report(args, corpus, thresholds, visual_settings) -> dict:
    import render_campaign_capture as capture

    root = args.capture_dir.absolute()
    retained = capture.runtime.read_regular_file(
        root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
    )
    bundle = capture.run(
        corpus.path,
        root,
        args.shared_font_pack.absolute(),
        args.fonttools_wheel.absolute(),
        args.pypdf_wheel.absolute(),
        verify=True,
    )
    if (
        args.source_revision is not None
        and bundle["source_revision"] != args.source_revision
    ):
        raise ValueError("capture source revision differs from requested revision")
    material = bundle["environment"]
    tools = [
        {"name": name, "version": version}
        for name, version in sorted(material["analysis_tools"].items())
    ]
    environment = {
        "source_revision": bundle["source_revision"],
        "source_dirty": False,
        "harness_sha256": _harness_sha256(),
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
        "tools": tools,
    }
    rows = []
    for document in corpus.documents:
        directory = root / "cases" / document.case_id
        reference, candidate = (
            directory / "reference/output.pdf",
            directory / "native.pdf",
        )
        report, _ = capture._load_json(directory / "native-report.json", 1024 * 1024)
        kinds = warning_kinds(report)
        if kinds is None:
            raise ValueError("captured native warning report is invalid")
        recall = text_recall(reference, candidate, kinds, report)
        candidate_pages, reference_pages = page_count(candidate), page_count(reference)
        visual = compare_pdf_visuals(
            reference,
            candidate,
            dpi=visual_settings["dpi"],
            page_cap=visual_settings["page_cap"],
            foreground_threshold=visual_settings["foreground_threshold"],
            ahash_size=visual_settings["ahash_size"],
        )
        rows.append(
            ValidationRow(
                document=document.path.name,
                case_id=document.case_id,
                input_bytes=document.input_bytes,
                input_sha256=document.sha256,
                status="pass" if recall >= args.recall_min else "fail",
                recall=round(recall, 4),
                rwml_pages=candidate_pages,
                reference_pages=reference_pages,
                page_ratio=round(candidate_pages / max(1, reference_pages), 4),
                ahash_similarity=round(hash_similarity(reference, candidate), 4),
                mean_page_ahash_similarity=visual.mean_page_ahash_similarity,
                foreground_ink_iou=visual.foreground_ink_iou,
                compared_pages=visual.compared_pages,
                unmatched_candidate_pages=visual.unmatched_candidate_pages,
                unmatched_reference_pages=visual.unmatched_reference_pages,
                capped_matched_pages=visual.capped_matched_pages,
                integer_visual_metrics=visual.integer_visual_metrics,
                pdf_point_geometry=visual.pdf_point_geometry,
                semantic_text_metrics=visual.semantic_text_metrics,
                text_geometry_metrics=visual.text_geometry_metrics,
                render_warnings=len(kinds),
                render_warning_kinds=sorted(kinds),
            )
        )
    binding = {
        "schema": capture.SCHEMA,
        "sha256": capture.digest(retained),
        "environment_sha256": environment["oracle"]["identity_sha256"],
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
    verify_campaign_inputs(corpus, environment)
    capture.require_equal(
        retained,
        capture.runtime.read_regular_file(
            root / "CAPTURE.json", capture.MAX_BUNDLE_BYTES
        ),
        "capture during metrics",
    )
    # Metric analysis must not turn modified retained artifacts into fresh evidence.
    for row in bundle["rows"]:
        directory = root / "cases" / row["case_id"]
        for name, path in (
            ("native", directory / "native.pdf"),
            ("reference", directory / "reference/output.pdf"),
        ):
            capture.require_equal(
                row[name]["pdf"],
                capture.identity(
                    capture.runtime.read_regular_file(
                        path, capture.resources.worker.MAX_PDF_BYTES
                    )
                ),
                "PDF during metrics",
            )
            capture.require_equal(
                row[name]["font_checks"],
                capture.identity(
                    capture.runtime.read_regular_file(
                        directory / f"{name}-fonts.json",
                        capture.resources.worker.MAX_RESULT_BYTES,
                    )
                ),
                "font receipt during metrics",
            )
        capture.require_equal(
            row["native_report"],
            capture.identity(
                capture.runtime.read_regular_file(
                    directory / "native-report.json", 1024 * 1024
                )
            ),
            "native report during metrics",
        )
    final_material, _, _, _ = capture.prepare_environment(
        args.shared_font_pack.absolute(),
        args.fonttools_wheel.absolute(),
        args.pypdf_wheel.absolute(),
    )
    capture.require_equal(
        material, final_material, "capture environment during metrics"
    )
    report = validation_report(
        rows, args.recall_min, thresholds=thresholds, visual_settings=visual_settings
    )
    return bind_evidence_report(report, corpus, environment, capture=binding)


def main() -> int:
    # Keep --help ASCII-only so it remains printable on Windows consoles using
    # legacy code pages such as cp949. The module docstring remains the detailed
    # design/reference documentation.
    ap = argparse.ArgumentParser(
        description="Validate rwml PDF output against LibreOffice reference renders."
    )
    ap.add_argument("inputs", nargs="*", type=Path)
    ap.add_argument("--capture-dir", type=Path, help="Independently verify and measure a retained shared-font capture.")
    ap.add_argument("--shared-font-pack", type=Path)
    ap.add_argument("--fonttools-wheel", type=Path)
    ap.add_argument("--pypdf-wheel", type=Path)
    ap.add_argument(
        "--manifest",
        type=Path,
        help=(
            "Read inputs from a strict render-oracle JSON corpus lock or a "
            "legacy public TSV manifest."
        ),
    )
    ap.add_argument(
        "--source-revision",
        help=(
            "Bind strict JSON evidence to this full lowercase Git SHA; "
            "it must match the current repository HEAD, which is the default."
        ),
    )
    ap.add_argument(
        "--soffice",
        choices=["auto", "local", "docker"],
        default="auto",
        help="LibreOffice backend; auto prefers local soffice, then Docker lo-cli.",
    )
    ap.add_argument("--recall-min", type=float, default=0.97)
    ap.add_argument("--min-mean-recall", type=float)
    ap.add_argument("--min-mean-page-ratio", type=float)
    ap.add_argument("--max-mean-page-ratio", type=float)
    ap.add_argument("--min-mean-ahash-similarity", type=float)
    ap.add_argument("--min-mean-page-ahash-similarity", type=float)
    ap.add_argument("--min-mean-foreground-ink-iou", type=float)
    ap.add_argument("--max-mean-render-warnings", type=float)
    ap.add_argument("--max-skipped", type=int)
    ap.add_argument("--max-unmatched-candidate-pages", type=int)
    ap.add_argument("--max-unmatched-reference-pages", type=int)
    ap.add_argument(
        "--raster-dpi",
        type=int,
        default=DEFAULT_RASTER_DPI,
        help=f"DPI for multi-page visual metrics (default: {DEFAULT_RASTER_DPI}).",
    )
    ap.add_argument(
        "--page-cap",
        type=int,
        default=DEFAULT_PAGE_CAP,
        help=f"Maximum matched pages rasterized per document (default: {DEFAULT_PAGE_CAP}).",
    )
    ap.add_argument(
        "--foreground-threshold",
        type=int,
        default=DEFAULT_FOREGROUND_THRESHOLD,
        help=(
            "Grayscale values below this are foreground ink "
            f"(default: {DEFAULT_FOREGROUND_THRESHOLD})."
        ),
    )
    ap.add_argument(
        "--ahash-size",
        type=int,
        default=DEFAULT_AHASH_SIZE,
        help=f"Side length for all-page aHash (default: {DEFAULT_AHASH_SIZE}).",
    )
    ap.add_argument(
        "--verify-oracle",
        action="store_true",
        help=(
            "Render every reference document twice and report whether the "
            "reference renderer reproduced itself. Doubles the reference render "
            "cost, so it is off by default."
        ),
    )
    ap.add_argument(
        "--system-fonts",
        action="store_true",
        help="Use host system fonts instead of the deterministic Noto subset set.",
    )
    ap.add_argument(
        "--json",
        action="store_true",
        help="Emit a machine-readable validation report instead of the table.",
    )
    args = ap.parse_args()
    try:
        visual_settings = validate_visual_settings(
            {
                "dpi": args.raster_dpi,
                "page_cap": args.page_cap,
                "foreground_threshold": args.foreground_threshold,
                "ahash_size": args.ahash_size,
                "font_mode": ("locked-shared-fonts" if args.capture_dir else
                              "system" if args.system_fonts else DEFAULT_FONT_MODE),
            }
        )
    except ValueError as exc:
        ap.error(str(exc))
    try:
        inputs, corpus = resolve_input_campaign(args.inputs, args.manifest)
    except ValueError as exc:
        ap.error(str(exc))
    if not inputs:
        ap.error("the following arguments are required: inputs or --manifest")
    try:
        thresholds = resolve_validation_thresholds(
            args, strict_corpus=corpus is not None
        )
    except ValueError as exc:
        ap.error(str(exc))

    capture_options = (args.shared_font_pack, args.fonttools_wheel, args.pypdf_wheel)
    if args.capture_dir:
        if (corpus is None or not args.json or not all(capture_options) or args.inputs
                or args.system_fonts or args.verify_oracle or args.soffice != "auto"):
            ap.error("--capture-dir requires a strict manifest, --json, shared font pack and both wheels; renderer overrides and --verify-oracle are not allowed")
        try:
            report = captured_validation_report(args, corpus, thresholds, visual_settings)
        except (OSError, ValueError, RenderDependencyError, VisualMetricError) as exc:
            ap.error(str(exc))
        print(json_report_payload(report))
        return 0 if report["gate"]["passed"] else 1
    elif any(capture_options):
        ap.error("shared capture options require --capture-dir")

    if not args.json:
        print(
            f"{'document':40} {'recall':>8} {'pages':>10} "
            f"{'aHash':>8} {'pageHash':>8} {'inkIoU':>8} {'warn':>5}  result"
        )
        print("-" * 108)
    rows = []
    corpus_documents = corpus_document_map(corpus)
    reference_stable: bool | None = None
    reference_incomplete = False
    unstable_references: list[str] = []
    try:
        soffice_mode = resolve_soffice_mode(args.soffice)
    except RenderDependencyError as exc:
        sys.exit(str(exc))
    try:
        local_font_lock = (
            load_font_lock(LOCAL_ORACLE_FONT_LOCK)
            if soffice_mode == "local"
            else None
        )
        bound_environment = (
            environment_identity(
                soffice_mode=soffice_mode,
                font_mode=visual_settings["font_mode"],
                source_revision=args.source_revision,
            )
            if corpus is not None
            else None
        )
    except (OSError, RenderDependencyError, ValueError) as exc:
        ap.error(str(exc))
    # Temp dir under cwd so Docker Desktop (which can't mount the system temp on
    # Windows) can bind-mount it for the LibreOffice reference render.
    with tempfile.TemporaryDirectory(dir=Path.cwd()) as td:
        tmp = Path(td)
        for src in inputs:
            try:
                ref = render_libreoffice(src, tmp, soffice_mode)
            except RenderDependencyError as exc:
                sys.exit(str(exc))
            reference_fonts_valid = True
            probe_unverified = False
            if ref is not None and local_font_lock is not None:
                try:
                    validate_pdf_font_identities(
                        reference_pdf_font_identities(ref),
                        local_font_lock,
                        allow_empty=True,
                    )
                except ValueError:
                    reference_fonts_valid = False
            if args.verify_oracle and ref is not None and reference_fonts_valid:
                # Render the same document a second time. A reference renderer
                # that does not reproduce itself makes the visual metrics
                # incomparable across runs, so every document is checked rather
                # than a sample, which would only give false confidence.
                probe_dir = tmp / "oracle-probe" / src.stem
                probe_dir.mkdir(parents=True, exist_ok=True)
                try:
                    again = render_libreoffice(src, probe_dir, soffice_mode)
                except RenderDependencyError:
                    again = None
                if again is not None and local_font_lock is not None:
                    try:
                        validate_pdf_font_identities(
                            reference_pdf_font_identities(again),
                            local_font_lock,
                            allow_empty=True,
                        )
                    except ValueError:
                        reference_fonts_valid = False
                verdict = (
                    oracle_stability_verdict(
                        reference_page_digests(
                            ref, dpi=args.raster_dpi, page_cap=args.page_cap
                        ),
                        reference_page_digests(
                            again, dpi=args.raster_dpi, page_cap=args.page_cap
                        ),
                    )
                    if again is not None and reference_fonts_valid
                    else None
                )
                if verdict is False:
                    unstable_references.append(src.name)
                if verdict is None:
                    probe_unverified = True
                if verdict is not None:
                    reference_stable = (reference_stable is not False) and verdict
            if args.verify_oracle and (
                ref is None or not reference_fonts_valid or probe_unverified
            ):
                reference_incomplete = True
            got = tmp / (src.stem + ".rwml.pdf")
            render_report = render_rwml(
                src,
                got,
                tmp / (src.stem + ".rwml.report.json"),
                fixed_fonts=not args.system_fonts,
            )
            if (
                ref is None
                or render_report is None
                or not reference_fonts_valid
                or probe_unverified
            ):
                if not reference_fonts_valid:
                    reason = "reference-font-lock-failed" if corpus else "render failed"
                elif probe_unverified:
                    reason = "reference-repeat-unverified"
                else:
                    reason = "render-failed" if corpus else "render failed"
                rows.append(
                    ValidationRow(
                        document=src.name,
                        status="skip",
                        reason=reason,
                        **row_identity(src, corpus_documents),
                    )
                )
                if not args.json:
                    print(
                        f"{src.name[:40]:40} {'—':>8} {'—':>10} "
                        f"{'—':>8} {'—':>8} {'—':>8} {'—':>5}  SKIP ({reason})"
                    )
                continue
            kinds = warning_kinds(render_report)
            if kinds is None:
                rows.append(
                    ValidationRow(
                        document=src.name,
                        status="skip",
                        reason=(
                            "invalid-render-warnings"
                            if corpus is not None
                            else "render report invalid warnings"
                        ),
                        **row_identity(src, corpus_documents),
                    )
                )
                if not args.json:
                    print(
                        f"{src.name[:40]:40} {'--':>8} {'--':>10} "
                        f"{'--':>8} {'--':>8} {'--':>8} {'--':>5}  "
                        "SKIP (render report invalid warnings)"
                    )
                continue
            rec = text_recall(ref, got, kinds, render_report)
            got_pages = page_count(got)
            ref_pages = page_count(ref)
            pr = got_pages / max(1, ref_pages)
            sim = hash_similarity(ref, got)
            try:
                visual = compare_pdf_visuals(
                    ref,
                    got,
                    dpi=visual_settings["dpi"],
                    page_cap=visual_settings["page_cap"],
                    foreground_threshold=visual_settings["foreground_threshold"],
                    ahash_size=visual_settings["ahash_size"],
                )
            except VisualMetricError as exc:
                rows.append(
                    ValidationRow(
                        document=src.name,
                        status="skip",
                        reason=(
                            "visual-metric-failed" if corpus is not None else str(exc)
                        ),
                        **row_identity(src, corpus_documents),
                    )
                )
                if not args.json:
                    print(
                        f"{src.name[:40]:40} {'--':>8} {'--':>10} "
                        f"{'--':>8} {'--':>8} {'--':>8} {'--':>5}  "
                        f"SKIP ({exc})"
                    )
                continue
            kinds = sorted(kinds)
            passed = rec >= args.recall_min
            status = "pass" if passed else "fail"
            rows.append(
                ValidationRow(
                    document=src.name,
                    status=status,
                    recall=round(rec, 4),
                    rwml_pages=got_pages,
                    reference_pages=ref_pages,
                    page_ratio=round(pr, 4),
                    ahash_similarity=round(sim, 4),
                    mean_page_ahash_similarity=visual.mean_page_ahash_similarity,
                    foreground_ink_iou=visual.foreground_ink_iou,
                    compared_pages=visual.compared_pages,
                    unmatched_candidate_pages=visual.unmatched_candidate_pages,
                    unmatched_reference_pages=visual.unmatched_reference_pages,
                    capped_matched_pages=visual.capped_matched_pages,
                    integer_visual_metrics=visual.integer_visual_metrics,
                    pdf_point_geometry=visual.pdf_point_geometry,
                    semantic_text_metrics=visual.semantic_text_metrics,
                    text_geometry_metrics=visual.text_geometry_metrics,
                    render_warnings=len(kinds) if kinds is not None else None,
                    render_warning_kinds=kinds,
                    **row_identity(src, corpus_documents),
                )
            )
            if not args.json:
                mark = "PASS" if passed else "FAIL"
                warns = len(kinds) if kinds is not None else 0
                print(
                    f"{src.name[:40]:40} {rec:8.3f} "
                    f"{got_pages}/{ref_pages:<7} {sim:8.3f} "
                    f"{(visual.mean_page_ahash_similarity or 0.0):8.3f} "
                    f"{(visual.foreground_ink_iou or 0.0):8.3f} {warns:5}  {mark}"
                )
    if reference_incomplete and reference_stable is not False:
        reference_stable = None
    report = validation_report(
        rows,
        args.recall_min,
        thresholds=thresholds,
        visual_settings=visual_settings,
        reference_stable=reference_stable,
        unstable_references=unstable_references,
    )
    if corpus is not None:
        try:
            assert bound_environment is not None
            verify_campaign_inputs(corpus, bound_environment)
            report = bind_evidence_report(
                report,
                corpus,
                bound_environment,
            )
        except (OSError, RenderDependencyError, ValueError) as exc:
            ap.error(str(exc))
    if args.json:
        print(json_report_payload(report))
    elif report["summary"]["measured"]:
        mean_warnings = report["summary"]["mean_render_warnings"]
        print("-" * 80)
        print(
            "mean recall "
            f"{report['summary']['mean_recall']:.3f} over "
            f"{report['summary']['measured']} docs, "
            f"{report['summary']['below_recall_min']} below {args.recall_min}; "
            f"mean page ratio {report['summary']['mean_page_ratio']:.3f}; "
            f"legacy aHash {report['summary']['mean_ahash_similarity']:.3f}; "
            "mean page aHash "
            f"{(report['summary']['mean_page_ahash_similarity'] or 0.0):.3f}; "
            "mean ink IoU "
            f"{(report['summary']['mean_foreground_ink_iou'] or 0.0):.3f}; "
            f"mean warnings {(mean_warnings or 0.0):.3f}"
        )
        failures = [check for check in report["gate"]["checks"] if not check["passed"]]
        for check in failures:
            print(
                "threshold failed: "
                f"{check['metric']} {check['op']} {check['threshold']} "
                f"(actual {check['actual']})"
            )
    return 0 if report["gate"]["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
