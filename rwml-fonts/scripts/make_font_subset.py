#!/usr/bin/env python3
"""Regenerate the rwml Noto Sans KR subset.

This is a developer utility. The crate build never invokes it.
"""

from __future__ import annotations

import hashlib
import subprocess
import sys
import urllib.request
from pathlib import Path

import fontTools


FONTTOOLS_VERSION = "4.63.0"
GOOGLE_FONTS_REVISION = "4efc2774c63917927efe769ca845def6bd6debae"
SOURCE_URL = (
    "https://raw.githubusercontent.com/google/fonts/"
    f"{GOOGLE_FONTS_REVISION}/ofl/notosanskr/NotoSansKR%5Bwght%5D.ttf"
)
UPSTREAM_SHA256 = "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252"
STATIC_SHA256 = "4609a7b62a6da24cae3a8b73ecde7003581b8f60662d60cc8f55a3793de07763"
SUBSET_SHA256 = "5e90c39a6222113aa261b3d40efdbff7e3a3e09868854232295bac7a636d556b"
FULL_SUBSET_SHA256 = "2291c987b64cdc579a4a450149487e742aa497e23d6fc811b7904845f254bf07"
UNICODE_RANGES = (
    "U+0020-007E,U+00A0-00FF,U+2010-2027,U+20A9,"
    "U+25AA,U+25CB,U+25E6,U+3000-303F,U+3130-318F"
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def run(command: list[str], cwd: Path) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def ksx1001_wansung_text() -> str:
    chars = [
        chr(codepoint)
        for codepoint in range(0xAC00, 0xD7A4)
        if len(chr(codepoint).encode("euc_kr", errors="ignore")) == 2
    ]
    if len(chars) != 2350:
        raise RuntimeError(f"expected 2350 KS X 1001 syllables, got {len(chars)}")
    return "".join(chars)


def ksx1001_hanja_text() -> str:
    chars = [
        chr(codepoint)
        for start, end in ((0x4E00, 0xA000), (0xF900, 0xFB00))
        for codepoint in range(start, end)
        if len(chr(codepoint).encode("euc_kr", errors="ignore")) == 2
    ]
    if len(chars) != 4888:
        raise RuntimeError(f"expected 4888 KS X 1001 hanja, got {len(chars)}")
    return "".join(chars)


def main() -> int:
    if fontTools.__version__ != FONTTOOLS_VERSION:
        raise RuntimeError(
            f"FontTools {FONTTOOLS_VERSION} required, found {fontTools.__version__}"
        )

    crate_root = Path(__file__).resolve().parents[1]
    work_dir = crate_root / "target" / "fontprep"
    fonts_dir = crate_root / "fonts"
    work_dir.mkdir(parents=True, exist_ok=True)
    fonts_dir.mkdir(parents=True, exist_ok=True)

    variable_font = work_dir / "NotoSansKR[wght].ttf"
    static_font = work_dir / "NotoSansKR-static.ttf"
    ksx1001 = work_dir / "ksx1001.txt"
    ksx1001_hanja = work_dir / "ksx1001-hanja.txt"
    ksx1001_full = work_dir / "ksx1001-full.txt"
    subset = fonts_dir / "NotoSansKR-rwml-subset.ttf"
    full_subset = fonts_dir / "NotoSansKR-rwml-subset-full.ttf"

    urllib.request.urlretrieve(SOURCE_URL, variable_font)
    upstream_hash = sha256(variable_font)
    if upstream_hash != UPSTREAM_SHA256:
        raise RuntimeError(f"upstream sha256 mismatch: {upstream_hash}")

    run(
        [
            sys.executable,
            "-m",
            "fontTools.varLib.instancer",
            "NotoSansKR[wght].ttf",
            "wght=400",
            "--update-name-table",
            "--no-recalc-timestamp",
            "-o",
            "NotoSansKR-static.ttf",
        ],
        work_dir,
    )
    static_hash = sha256(static_font)
    if static_hash != STATIC_SHA256:
        raise RuntimeError(f"static font sha256 mismatch: {static_hash}")
    hangul_text = ksx1001_wansung_text()
    hanja_text = ksx1001_hanja_text()
    ksx1001.write_text(hangul_text, encoding="utf-8")
    ksx1001_hanja.write_text(hanja_text, encoding="utf-8")
    ksx1001_full.write_text(hangul_text + hanja_text, encoding="utf-8")
    run(
        [
            sys.executable,
            "-m",
            "fontTools.subset",
            "NotoSansKR-static.ttf",
            "--text-file=ksx1001.txt",
            f"--unicodes={UNICODE_RANGES}",
            "--name-IDs=*",
            "--notdef-outline",
            "--no-recalc-timestamp",
            f"--output-file={subset}",
        ],
        work_dir,
    )
    run(
        [
            sys.executable,
            "-m",
            "fontTools.subset",
            "NotoSansKR-static.ttf",
            "--text-file=ksx1001-full.txt",
            f"--unicodes={UNICODE_RANGES}",
            "--name-IDs=*",
            "--notdef-outline",
            "--no-recalc-timestamp",
            f"--output-file={full_subset}",
        ],
        work_dir,
    )

    subset_hash = sha256(subset)
    if subset_hash != SUBSET_SHA256:
        raise RuntimeError(f"subset sha256 mismatch: {subset_hash}")
    full_subset_hash = sha256(full_subset)
    if full_subset_hash != FULL_SUBSET_SHA256:
        raise RuntimeError(f"full subset sha256 mismatch: {full_subset_hash}")
    print(f"wrote {subset} ({subset.stat().st_size} bytes)")
    print(f"wrote {full_subset} ({full_subset.stat().st_size} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
