#!/usr/bin/env python3
"""Multi-metric validation of rdoc's PDF renderer against LibreOffice.

For each input `.doc`/`.docx`, render it two ways and compare:

  * rdoc        — `cargo run --features render --example to_pdf -- IN OUT`
  * LibreOffice — `soffice --headless --convert-to pdf` (the reference oracle)

and report three metrics per document:

  * text recall   — fraction of the reference's whitespace-normalized tokens that
                    also appear in rdoc's text layer (selectable-text fidelity).
  * page ratio    — rdoc page count / reference page count (≈ 1.0 is good).
  * visual aHash  — mean per-page average-hash Hamming similarity of page 1
                    (0..1; a coarse "does it look alike" signal, not exactness).
  * warnings      — rdoc `RenderReport` warning count/kinds for trend tracking.

This is a developer tool, not part of the crate. It needs PyMuPDF (`pip install
pymupdf`), Pillow, and either a local `soffice` or the `lo-cli` Docker image.

  python scripts/render_validate.py corpus/*.docx
  python scripts/render_validate.py --soffice docker corpus/*.doc
  python scripts/render_validate.py --json corpus/*.docx > render-report.json
  python scripts/render_validate.py --json --min-mean-recall 0.90 --max-skipped 0 corpus/*.docx
"""

from __future__ import annotations

import argparse
import json
import math
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


@dataclass
class ValidationRow:
    document: str
    status: str
    recall: float | None = None
    rdoc_pages: int | None = None
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
        if isinstance(warning, dict) and isinstance(warning.get("kind"), str):
            kinds.append(warning["kind"])
    return kinds


def render_rdoc(src: Path, out: Path, report_out: Path | None = None) -> dict | None:
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
    r = subprocess.run(cmd, capture_output=True)
    out = outdir / (src.stem + ".pdf")
    return out if (r.returncode == 0 and out.exists()) else None


def tokens(pdf: Path) -> list[str]:
    require_pdf_deps()
    doc = fitz.open(pdf)
    text = " ".join(p.get_text() for p in doc)
    return text.split()


def text_recall(ref: Path, got: Path) -> float:
    ref_tokens = tokens(ref)
    if not ref_tokens:
        return 1.0
    got_set = set(tokens(got))
    hit = sum(1 for t in ref_tokens if t in got_set)
    return hit / len(ref_tokens)


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
    ap.add_argument("inputs", nargs="+", type=Path)
    ap.add_argument("--soffice", choices=["local", "docker"], default="docker")
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

    if not args.json:
        print(
            f"{'document':40} {'recall':>8} {'pages':>10} "
            f"{'aHash':>8} {'warn':>5}  result"
        )
        print("-" * 88)
    rows = []
    # Temp dir under cwd so Docker Desktop (which can't mount the system temp on
    # Windows) can bind-mount it for the LibreOffice reference render.
    with tempfile.TemporaryDirectory(dir=Path.cwd()) as td:
        tmp = Path(td)
        for src in args.inputs:
            ref = render_libreoffice(src, tmp, args.soffice)
            got = tmp / (src.stem + ".rdoc.pdf")
            render_report = render_rdoc(src, got, tmp / (src.stem + ".rdoc.report.json"))
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
            rec = text_recall(ref, got)
            got_pages = page_count(got)
            ref_pages = page_count(ref)
            pr = got_pages / max(1, ref_pages)
            sim = hash_similarity(ref, got)
            passed = rec >= args.recall_min
            status = "pass" if passed else "fail"
            kinds = warning_kinds(render_report)
            rows.append(
                ValidationRow(
                    document=src.name,
                    status=status,
                    recall=round(rec, 4),
                    rdoc_pages=got_pages,
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
