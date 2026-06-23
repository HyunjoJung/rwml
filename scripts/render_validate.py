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

This is a developer tool, not part of the crate. It needs PyMuPDF (`pip install
pymupdf`), Pillow, and either a local `soffice` or the `lo-cli` Docker image.

  python scripts/render_validate.py corpus/*.docx
  python scripts/render_validate.py --soffice docker corpus/*.doc
"""

from __future__ import annotations

import argparse
import os
import subprocess
import sys
import tempfile
from pathlib import Path

try:
    import fitz  # PyMuPDF
except ImportError:
    sys.exit("PyMuPDF required: pip install pymupdf")
from PIL import Image  # noqa: E402  (after the fitz import guard)


def render_rdoc(src: Path, out: Path) -> bool:
    """Render via the crate's to_pdf example. Returns True on success."""
    r = subprocess.run(
        [
            "cargo", "run", "--quiet", "--features", "render",
            "--example", "to_pdf", "--", str(src), str(out),
        ],
        capture_output=True,
    )
    return r.returncode == 0 and out.exists() and out.stat().st_size > 0


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
    return fitz.open(pdf).page_count


def ahash(pdf: Path, page: int = 0, size: int = 16) -> int:
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
    args = ap.parse_args()

    print(f"{'document':40} {'recall':>8} {'pages':>10} {'aHash':>8}  result")
    print("-" * 80)
    recalls, fails = [], 0
    # Temp dir under cwd so Docker Desktop (which can't mount the system temp on
    # Windows) can bind-mount it for the LibreOffice reference render.
    with tempfile.TemporaryDirectory(dir=Path.cwd()) as td:
        tmp = Path(td)
        for src in args.inputs:
            ref = render_libreoffice(src, tmp, args.soffice)
            got = tmp / (src.stem + ".rdoc.pdf")
            ok = render_rdoc(src, got)
            if ref is None or not ok:
                print(f"{src.name[:40]:40} {'—':>8} {'—':>10} {'—':>8}  SKIP (render failed)")
                continue
            rec = text_recall(ref, got)
            pr = page_count(got) / max(1, page_count(ref))
            sim = hash_similarity(ref, got)
            recalls.append(rec)
            passed = rec >= args.recall_min
            fails += 0 if passed else 1
            mark = "PASS" if passed else "FAIL"
            print(
                f"{src.name[:40]:40} {rec:8.3f} "
                f"{page_count(got)}/{page_count(ref):<7} {sim:8.3f}  {mark}"
            )
    if recalls:
        print("-" * 80)
        print(f"mean recall {sum(recalls) / len(recalls):.3f} over {len(recalls)} docs, {fails} below {args.recall_min}")
    return 1 if fails else 0


if __name__ == "__main__":
    raise SystemExit(main())
