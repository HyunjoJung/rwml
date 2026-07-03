#!/usr/bin/env python3
"""Regenerate the rdoc Noto Sans KR subset.

This is a developer utility. The crate build never invokes it.
"""

from __future__ import annotations

import hashlib
import subprocess
import urllib.request
from pathlib import Path


SOURCE_URL = (
    "https://github.com/google/fonts/raw/main/ofl/notosanskr/"
    "NotoSansKR%5Bwght%5D.ttf"
)
UPSTREAM_SHA256 = "194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252"
SUBSET_SHA256 = "e928aaee9e585e209b82ca7a59e3a843440f134104ee8eb2e084cf44c72a7087"
FULL_SUBSET_SHA256 = "9a39382a3f7bab6fa8295830609b9b3a4d5162e575461f8fdd1e55c94b42bcf9"
UNICODE_RANGES = "U+0020-007E,U+00A0-00FF,U+2010-2027,U+20A9,U+3000-303F,U+3130-318F"


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
    subset = fonts_dir / "NotoSansKR-rdoc-subset.ttf"
    full_subset = fonts_dir / "NotoSansKR-rdoc-subset-full.ttf"

    urllib.request.urlretrieve(SOURCE_URL, variable_font)
    upstream_hash = sha256(variable_font)
    if upstream_hash != UPSTREAM_SHA256:
        raise RuntimeError(f"upstream sha256 mismatch: {upstream_hash}")

    run(
        [
            "fonttools",
            "varLib.instancer",
            "NotoSansKR[wght].ttf",
            "wght=400",
            "--update-name-table",
            "-o",
            "NotoSansKR-static.ttf",
        ],
        work_dir,
    )
    hangul_text = ksx1001_wansung_text()
    hanja_text = ksx1001_hanja_text()
    ksx1001.write_text(hangul_text, encoding="utf-8")
    ksx1001_hanja.write_text(hanja_text, encoding="utf-8")
    ksx1001_full.write_text(hangul_text + hanja_text, encoding="utf-8")
    run(
        [
            "pyftsubset",
            "NotoSansKR-static.ttf",
            "--text-file=ksx1001.txt",
            f"--unicodes={UNICODE_RANGES}",
            "--name-IDs=*",
            "--notdef-outline",
            "--output-file",
            str(subset),
        ],
        work_dir,
    )
    run(
        [
            "pyftsubset",
            "NotoSansKR-static.ttf",
            "--text-file=ksx1001-full.txt",
            f"--unicodes={UNICODE_RANGES}",
            "--name-IDs=*",
            "--notdef-outline",
            "--output-file",
            str(full_subset),
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
