#!/usr/bin/env python3
"""Regenerate the rwml Noto Sans Arabic and Hebrew subsets.

This is a developer utility. The crate build never invokes it.
"""

from __future__ import annotations

import hashlib
import subprocess
import sys
import urllib.request
from dataclasses import dataclass
from pathlib import Path

import fontTools


FONTTOOLS_VERSION = "4.63.0"
GOOGLE_FONTS_REVISION = "26c5c976d82d50c24a8f0a7ac455e0a7c639c226"
COMMON_RANGES = "U+0020-007E,U+200C-200F,U+2010-2027,U+25CC"


@dataclass(frozen=True)
class FontSpec:
    family: str
    source_dir: str
    unicode_ranges: str
    source_sha256: str
    static_sha256: str
    subset_sha256: str
    license_sha256: str

    @property
    def source_name(self) -> str:
        return f"{self.family}[wdth,wght].ttf"

    @property
    def source_url(self) -> str:
        name = (
            self.source_name.replace("[", "%5B")
            .replace("]", "%5D")
            .replace(",", "%2C")
        )
        return (
            "https://raw.githubusercontent.com/google/fonts/"
            f"{GOOGLE_FONTS_REVISION}/ofl/{self.source_dir}/{name}"
        )

    @property
    def license_url(self) -> str:
        return (
            "https://raw.githubusercontent.com/google/fonts/"
            f"{GOOGLE_FONTS_REVISION}/ofl/{self.source_dir}/OFL.txt"
        )


FONTS = (
    FontSpec(
        family="NotoSansArabic",
        source_dir="notosansarabic",
        unicode_ranges=f"{COMMON_RANGES},U+0600-06FF,U+0750-077F,U+08A0-08FF",
        source_sha256="63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e",
        static_sha256="65efad4885c590b640d9601b8cc6d9b66026e9ff74154ac600207600842c0f70",
        subset_sha256="9d66a71d753f9853b292c748a9e66272b3cb5b8d898f4e69ebae3ec9e5367880",
        license_sha256="07fc70bfeb985cc1a87a8587d0a0c80bab11c86c9dc3fd95b6f0cb332f983e96",
    ),
    FontSpec(
        family="NotoSansHebrew",
        source_dir="notosanshebrew",
        unicode_ranges=f"{COMMON_RANGES},U+0590-05FF",
        source_sha256="7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88",
        static_sha256="5fb2e1fc38c242d64f9fc5f77e026473f288b9791762d80df98a3ec762d5bbdf",
        subset_sha256="7e39e0a065b66de7b920be4f40223e79dcace3a50c1d8aba33f51ada0c93678f",
        license_sha256="9b9fe028b5ba74d231659a1bbaf0ed09b11e759d1ca6a070999e16d151616b47",
    ),
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify(path: Path, expected: str) -> None:
    actual = sha256(path)
    if actual != expected:
        raise RuntimeError(f"sha256 mismatch for {path.name}: {actual}")


def download(url: str, path: Path, expected: str) -> None:
    with urllib.request.urlopen(url) as response, path.open("wb") as output:
        output.write(response.read())
    verify(path, expected)


def run(command: list[str], cwd: Path) -> None:
    subprocess.run(command, cwd=cwd, check=True)


def regenerate(spec: FontSpec, crate_root: Path, work_dir: Path) -> None:
    source = work_dir / spec.source_name
    static = work_dir / f"{spec.family}-static.ttf"
    subset = crate_root / "fonts" / f"{spec.family}-rwml-subset.ttf"
    staged_license = work_dir / f"OFL-{spec.family}.txt"
    packaged_license = crate_root / f"OFL-{spec.family}.txt"

    download(spec.source_url, source, spec.source_sha256)
    download(spec.license_url, staged_license, spec.license_sha256)

    run(
        [
            sys.executable,
            "-m",
            "fontTools.varLib.instancer",
            source.name,
            "wght=400",
            "wdth=100",
            "--update-name-table",
            "--no-recalc-timestamp",
            "-o",
            static.name,
        ],
        work_dir,
    )
    verify(static, spec.static_sha256)

    run(
        [
            sys.executable,
            "-m",
            "fontTools.subset",
            static.name,
            f"--unicodes={spec.unicode_ranges}",
            "--layout-features=*",
            "--layout-scripts=*",
            "--name-IDs=*",
            "--notdef-outline",
            "--no-recalc-timestamp",
            f"--output-file={subset}",
        ],
        work_dir,
    )
    verify(subset, spec.subset_sha256)
    packaged_license.write_bytes(staged_license.read_bytes())
    verify(packaged_license, spec.license_sha256)
    print(f"wrote {subset} ({subset.stat().st_size} bytes)")


def main() -> int:
    if fontTools.__version__ != FONTTOOLS_VERSION:
        raise RuntimeError(
            f"FontTools {FONTTOOLS_VERSION} required, found {fontTools.__version__}"
        )

    crate_root = Path(__file__).resolve().parents[1]
    work_dir = crate_root / "target" / "fontprep" / "rtl"
    work_dir.mkdir(parents=True, exist_ok=True)
    (crate_root / "fonts").mkdir(parents=True, exist_ok=True)

    for spec in FONTS:
        regenerate(spec, crate_root, work_dir)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
