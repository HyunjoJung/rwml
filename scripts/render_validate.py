#!/usr/bin/env python3
"""Multi-metric validation of rwml's PDF renderer against LibreOffice.

For each input `.doc`/`.docx`, render it two ways and compare:

  * rwml        — `cargo run --features render --example to_pdf -- IN OUT`
  * LibreOffice — `soffice --headless --convert-to pdf` (the reference oracle)

and report three metrics per document:

  * text recall   — fraction of the reference's whitespace-normalized tokens that
                    also appear in rwml's text layer, after dropping volatile
                    LibreOffice-only field text such as local file paths and
                    missing-reference placeholders, plus known fallback shape
                    placeholders and joined tracked-change/footnote markers
                    when rwml's report proves that context.
  * page ratio    — rwml page count / reference page count (≈ 1.0 is good).
  * visual aHash  — mean per-page average-hash Hamming similarity of page 1
                    (0..1; a coarse "does it look alike" signal, not exactness).
  * warnings      — rwml `RenderReport` warning count/kinds for trend tracking.

This is a developer tool, not part of the crate. It needs PyMuPDF (`pip install
pymupdf`), Pillow, and either a local `soffice` or the `lo-cli` Docker image.
By default, `--soffice auto` prefers local `soffice` when present and falls back
to Docker.

  python scripts/render_validate.py corpus/public/**/*.docx
  python scripts/render_validate.py --manifest corpus/public/RENDER_MANIFEST.tsv
  python scripts/render_validate.py --soffice docker corpus/*.doc
  python scripts/render_validate.py --json corpus/public/**/*.docx > render-report.json
  python scripts/render_validate.py --json --manifest corpus/public/RENDER_MANIFEST.tsv > render-report.json
  python scripts/render_validate.py --json --min-mean-recall 0.90 --max-skipped 0 corpus/public/**/*.docx > render-report.json
"""

from __future__ import annotations

import argparse
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
from dataclasses import asdict, dataclass
from pathlib import Path

try:
    import fitz  # PyMuPDF
except ImportError:
    fitz = None
try:
    from PIL import Image
except ImportError:
    Image = None


COUNT_THRESHOLD_METRICS = {"below_recall_min", "skipped"}
SCORE_THRESHOLD_METRICS = {
    "mean_recall",
    "mean_page_ratio",
    "mean_ahash_similarity",
}
BOUNDED_SCORE_THRESHOLD_METRICS = {"mean_recall", "mean_ahash_similarity"}
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


@dataclass
class ValidationRow:
    document: str
    status: str
    recall: float | None = None
    rwml_pages: int | None = None
    reference_pages: int | None = None
    page_ratio: float | None = None
    ahash_similarity: float | None = None
    render_warnings: int | None = None
    render_warning_kinds: list[str] | None = None
    reason: str | None = None


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
    if Image is None:
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
    checks = []
    add_threshold_check(
        checks,
        "below_recall_min",
        summary.get("below_recall_min"),
        "<=",
        0,
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
    return {"passed": all(check["passed"] for check in checks), "checks": checks}


def validation_report(
    rows: list[ValidationRow],
    recall_min: float,
    thresholds: dict | None = None,
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
        for metric in (
            "recall",
            "page_ratio",
            "ahash_similarity",
            "render_warnings",
        ):
            value = getattr(row, metric)
            if value is not None and not is_finite_number(value):
                raise ValueError(f"metric is invalid: {metric}")
            if value is not None and metric in {"recall", "ahash_similarity"}:
                if not 0 <= value <= 1:
                    raise ValueError(f"metric is out of range: {metric}")
        for metric in ("rwml_pages", "reference_pages", "render_warnings"):
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
        if row.status == "skip" and any(
            getattr(row, metric) is not None
            for metric in (
                "recall",
                "rwml_pages",
                "reference_pages",
                "page_ratio",
                "ahash_similarity",
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
    measured = [r for r in rows if r.recall is not None]
    summary = {
        "documents": len(rows),
        "measured": len(measured),
        "skipped": sum(1 for r in rows if r.status == "skip"),
        "below_recall_min": sum(
            1 for r in measured if r.recall is not None and r.recall < recall_min
        ),
        "recall_min": recall_min,
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
        "mean_render_warnings": mean(
            [
                r.render_warnings
                for r in measured
                if r.render_warnings is not None
            ]
        ),
    }
    return {
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


def render_rwml(src: Path, out: Path, report_out: Path | None = None) -> dict | None:
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
    if report_out is not None:
        cmd.extend(["--report-json", str(report_out)])
    r = subprocess.run(
        cmd,
        capture_output=True,
    )
    if not (r.returncode == 0 and out.exists() and out.stat().st_size > 0):
        return None
    if report_out is not None and report_out.exists():
        try:
            return json.loads(report_out.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return None
    return {}


def render_libreoffice(src: Path, outdir: Path, mode: str) -> Path | None:
    """Render via LibreOffice (`local` soffice or `docker` lo-cli)."""
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
        cmd = [
            "soffice", "--headless", "--convert-to", "pdf",
            "--outdir", str(outdir), str(src),
        ]
    try:
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
    if manifest is None:
        return inputs
    if inputs:
        raise ValueError("--manifest cannot be combined with positional inputs")
    return manifest_document_inputs(manifest)


def manifest_document_inputs(manifest: Path) -> list[Path]:
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


def tokens(pdf: Path) -> list[str]:
    require_pdf_deps()
    doc = fitz.open(pdf)
    text = " ".join(p.get_text() for p in doc)
    return text.split()


def reference_recall_tokens(
    raw_tokens: list[str],
    render_warning_kinds: list[str] | None = None,
) -> list[str]:
    tokens = []
    index = 0
    missing_reference = ["Error:", "Reference", "source", "not", "found"]
    while index < len(raw_tokens):
        if raw_tokens[index : index + len(missing_reference)] == missing_reference:
            index += len(missing_reference)
            continue
        token = raw_tokens[index]
        if not is_volatile_reference_path_token(
            token
        ) and not is_volatile_reference_shape_placeholder_token(
            token, render_warning_kinds
        ):
            tokens.append(token)
        index += 1
    return tokens


def is_volatile_reference_path_token(token: str) -> bool:
    value = token.strip(" \t\r\n\"'`.,;:()[]{}<>")
    if not value:
        return False
    lower = value.lower()
    office_extensions = (".doc", ".docx", ".docm", ".dot", ".dotx", ".rtf")
    if value.startswith(("/", "~/", "\\\\")):
        return True
    if len(value) >= 3 and value[1] == ":" and value[2] in {"/", "\\"}:
        return True
    if "/" in value and lower.endswith(office_extensions):
        return True
    if "\\" in value and lower.endswith(office_extensions):
        return True
    return False


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


def text_recall(
    ref: Path,
    got: Path,
    render_warning_kinds: list[str] | None = None,
    render_report: dict | None = None,
) -> float:
    ref_tokens = reference_recall_tokens(tokens(ref), render_warning_kinds)
    return token_recall(ref_tokens, tokens(got), render_report)


def page_count(pdf: Path) -> int:
    require_pdf_deps()
    return fitz.open(pdf).page_count


def ahash(pdf: Path, page: int = 0, size: int = 16) -> int:
    require_pdf_deps()
    doc = fitz.open(pdf)
    if page >= doc.page_count:
        return 0
    pix = doc[page].get_pixmap(dpi=72)
    img = Image.frombytes("RGB", (pix.width, pix.height), pix.samples)
    img = img.convert("L").resize((size, size))
    px = list(img.tobytes())  # L mode ⇒ one byte per pixel
    mean = sum(px) / len(px)
    bits = 0
    for i, v in enumerate(px):
        if v >= mean:
            bits |= 1 << i
    return bits


def hash_similarity(ref: Path, got: Path, size: int = 16) -> float:
    a, b = ahash(ref, size=size), ahash(got, size=size)
    ham = bin(a ^ b).count("1")
    return 1.0 - ham / (size * size)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("inputs", nargs="*", type=Path)
    ap.add_argument(
        "--manifest",
        type=Path,
        help="Read input document paths from a public corpus TSV manifest.",
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
    ap.add_argument("--max-mean-render-warnings", type=float)
    ap.add_argument("--max-skipped", type=int)
    ap.add_argument(
        "--json",
        action="store_true",
        help="Emit a machine-readable validation report instead of the table.",
    )
    args = ap.parse_args()
    try:
        inputs = resolve_input_paths(args.inputs, args.manifest)
    except ValueError as exc:
        ap.error(str(exc))
    if not inputs:
        ap.error("the following arguments are required: inputs or --manifest")

    if not args.json:
        print(
            f"{'document':40} {'recall':>8} {'pages':>10} "
            f"{'aHash':>8} {'warn':>5}  result"
        )
        print("-" * 88)
    rows = []
    try:
        soffice_mode = resolve_soffice_mode(args.soffice)
    except RenderDependencyError as exc:
        sys.exit(str(exc))
    # Temp dir under cwd so Docker Desktop (which can't mount the system temp on
    # Windows) can bind-mount it for the LibreOffice reference render.
    with tempfile.TemporaryDirectory(dir=Path.cwd()) as td:
        tmp = Path(td)
        for src in inputs:
            try:
                ref = render_libreoffice(src, tmp, soffice_mode)
            except RenderDependencyError as exc:
                sys.exit(str(exc))
            got = tmp / (src.stem + ".rwml.pdf")
            render_report = render_rwml(src, got, tmp / (src.stem + ".rwml.report.json"))
            if ref is None or render_report is None:
                rows.append(
                    ValidationRow(
                        document=src.name,
                        status="skip",
                        reason="render failed",
                    )
                )
                if not args.json:
                    print(
                        f"{src.name[:40]:40} {'—':>8} {'—':>10} "
                        f"{'—':>8} {'—':>5}  SKIP (render failed)"
                    )
                continue
            kinds = warning_kinds(render_report)
            if kinds is None:
                rows.append(
                    ValidationRow(
                        document=src.name,
                        status="skip",
                        reason="render report invalid warnings",
                    )
                )
                if not args.json:
                    print(
                        f"{src.name[:40]:40} {'--':>8} {'--':>10} "
                        f"{'--':>8} {'--':>5}  SKIP (render report invalid warnings)"
                    )
                continue
            rec = text_recall(ref, got, kinds, render_report)
            got_pages = page_count(got)
            ref_pages = page_count(ref)
            pr = got_pages / max(1, ref_pages)
            sim = hash_similarity(ref, got)
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
                    render_warnings=len(kinds) if kinds is not None else None,
                    render_warning_kinds=kinds,
                )
            )
            if not args.json:
                mark = "PASS" if passed else "FAIL"
                warns = len(kinds) if kinds is not None else 0
                print(
                    f"{src.name[:40]:40} {rec:8.3f} "
                    f"{got_pages}/{ref_pages:<7} {sim:8.3f} {warns:5}  {mark}"
                )
    thresholds = {
        "min_mean_recall": args.min_mean_recall,
        "min_mean_page_ratio": args.min_mean_page_ratio,
        "max_mean_page_ratio": args.max_mean_page_ratio,
        "min_mean_ahash_similarity": args.min_mean_ahash_similarity,
        "max_mean_render_warnings": args.max_mean_render_warnings,
        "max_skipped": args.max_skipped,
    }
    report = validation_report(rows, args.recall_min, thresholds=thresholds)
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
            f"mean aHash {report['summary']['mean_ahash_similarity']:.3f}; "
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
