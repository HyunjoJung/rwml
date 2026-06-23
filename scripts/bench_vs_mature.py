#!/usr/bin/env python3
"""Head-to-head: rdoc vs mature extractors on a local .doc corpus.

Compares rdoc's plain-text extraction against two mature references on the same
`.doc` sample:

  * Apache POI  — `WordExtractor.getText()` golden (sample-poi/*.poi.txt)
  * LibreOffice — `--convert-to txt` golden       (sample-lo/*.txt)

For each file it reports whitespace-token set recall (fraction of the reference's
tokens rdoc also produced), precision, and F1 — apples-to-apples because all three
are tokenized identically. The script reports measurements only; it does not
decide which extractor is "better".

  python scripts/bench_vs_mature.py --corpus DIR [--limit N] [--json]

The corpus directory must contain:
  sample-poi/*.poi.txt        Apache POI golden output
  sample-lo/*.txt             LibreOffice golden output
  sample/*.doc or govdocs/files/*.doc

You can also set RDOC_BENCH_CORPUS instead of passing --corpus. Needs the release
`extract` example built:
  cargo build --release --example extract
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path


def clean_golden(s: str) -> str:
    """Strip extractor artifacts that aren't document text: a UTF-8/UTF-16 BOM
    (LibreOffice prepends one, merging with the first token) and Apache POI's
    Log4j2 'could not find a logging implementation' startup lines."""
    s = s.replace("﻿", "")
    keep = [
        ln
        for ln in s.splitlines()
        if "Log4j" not in ln and "StatusLogger" not in ln
    ]
    return "\n".join(keep)


def toks(s: str) -> set[str]:
    return set(clean_golden(s).split())


def prf(ref: set[str], got: set[str]) -> tuple[float, float, float]:
    if not ref:
        return (1.0, 1.0, 1.0)
    inter = len(ref & got)
    recall = inter / len(ref)
    precision = inter / len(got) if got else 0.0
    f1 = (2 * recall * precision / (recall + precision)) if (recall + precision) else 0.0
    return (recall, precision, f1)


def rdoc_text(extract_bin: Path, doc: Path) -> str | None:
    r = subprocess.run([str(extract_bin), str(doc)], capture_output=True)
    if r.returncode != 0:
        return None
    return r.stdout.decode("utf-8", "replace")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--corpus", type=Path, default=None)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--json", action="store_true")
    args = ap.parse_args()
    if args.corpus is None:
        env_corpus = os.environ.get("RDOC_BENCH_CORPUS")
        if not env_corpus:
            sys.exit("pass --corpus DIR or set RDOC_BENCH_CORPUS")
        args.corpus = Path(env_corpus)

    repo = Path(__file__).resolve().parent.parent
    extract_bin = repo / "target" / "release" / "examples" / "extract.exe"
    if not extract_bin.exists():
        extract_bin = repo / "target" / "release" / "examples" / "extract"
    if not extract_bin.exists():
        sys.exit("build first: cargo build --release --example extract")

    poi_dir = args.corpus / "sample-poi"
    lo_dir = args.corpus / "sample-lo"
    src_dirs = [args.corpus / "sample", args.corpus / "govdocs" / "files"]

    golden = sorted(poi_dir.glob("*.poi.txt"))
    if args.limit:
        golden = golden[: args.limit]

    rows = []
    for g in golden:
        base = g.name[: -len(".poi.txt")]
        doc = next((d / f"{base}.doc" for d in src_dirs if (d / f"{base}.doc").exists()), None)
        if doc is None:
            continue
        got = rdoc_text(extract_bin, doc)
        if got is None:
            rows.append({"file": base, "rdoc": "ERROR"})
            continue
        gt = toks(got)
        poi = g.read_text(encoding="utf-8", errors="replace")
        rp, pp, fp = prf(toks(poi), gt)
        row = {"file": base, "poi_recall": rp, "poi_prec": pp, "poi_f1": fp}
        lo_file = lo_dir / f"{base}.txt"
        if lo_file.exists():
            lo = lo_file.read_text(encoding="utf-8", errors="replace")
            rl, pl, fl = prf(toks(lo), gt)
            row.update({"lo_recall": rl, "lo_prec": pl, "lo_f1": fl})
        rows.append(row)

    ok = [r for r in rows if "poi_recall" in r]
    summary = {
        "files": len(rows),
        "scored": len(ok),
        "errors": sum(1 for r in rows if r.get("rdoc") == "ERROR"),
        "poi_recall_mean": round(sum(r["poi_recall"] for r in ok) / len(ok), 4) if ok else 0,
        "poi_f1_mean": round(sum(r["poi_f1"] for r in ok) / len(ok), 4) if ok else 0,
        "poi_recall_median": round(sorted(r["poi_recall"] for r in ok)[len(ok) // 2], 4) if ok else 0,
        "lo_recall_mean": round(
            sum(r["lo_recall"] for r in ok if "lo_recall" in r) / max(1, sum(1 for r in ok if "lo_recall" in r)), 4
        ),
    }

    if args.json:
        print(json.dumps({"summary": summary, "rows": rows}, ensure_ascii=False, indent=2))
        return 0

    print(f"{'file':16} {'POI rec':>8} {'POI F1':>8} {'LO rec':>8}")
    print("-" * 46)
    for r in rows:
        if "poi_recall" not in r:
            print(f"{r['file']:16} {'ERROR':>8}")
            continue
        lo = f"{r.get('lo_recall', float('nan')):8.3f}" if "lo_recall" in r else f"{'—':>8}"
        print(f"{r['file']:16} {r['poi_recall']:8.3f} {r['poi_f1']:8.3f} {lo}")
    print("-" * 46)
    print(
        f"rdoc vs Apache POI: recall {summary['poi_recall_mean']:.3f} mean / "
        f"{summary['poi_recall_median']:.3f} median, F1 {summary['poi_f1_mean']:.3f} "
        f"over {summary['scored']} files ({summary['errors']} errors)"
    )
    print(f"rdoc vs LibreOffice: recall {summary['lo_recall_mean']:.3f} mean")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
