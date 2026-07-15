# Bundled Font Provenance

This crate bundles layout-derived regular-weight subsets of Noto Sans KR, Noto
Sans Arabic, and Noto Sans Hebrew for rwml PDF rendering. No full upstream font
is packaged.

## Noto Sans KR

Packaged files:

- `fonts/NotoSansKR-rwml-subset.ttf`: slim KS X 1001 Hangul subset, 478,940
  bytes, SHA-256
  `e928aaee9e585e209b82ca7a59e3a843440f134104ee8eb2e084cf44c72a7087`.
- `fonts/NotoSansKR-rwml-subset-full.ttf`: hanja-inclusive subset, 1,983,952
  bytes, SHA-256
  `9a39382a3f7bab6fa8295830609b9b3a4d5162e575461f8fdd1e55c94b42bcf9`.

Source:

- URL: <https://github.com/google/fonts/raw/main/ofl/notosanskr/NotoSansKR%5Bwght%5D.ttf>
- Variable font size: 10,414,588 bytes.
- Variable font SHA-256:
  `194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252`.
- Staged: 2026-07-03 with the then-current FontTools from pip.

The slim subset includes the 2,350 KS X 1001 wansung Hangul syllables, Basic
Latin, Latin-1, common punctuation, Hangul compatibility jamo, and related
punctuation. The full subset adds 4,885 of the 4,888 KS X 1001 hanja; three
compatibility ideographs are absent from upstream Noto Sans KR. See
`scripts/make_font_subset.py` for the exact legacy regeneration process.

The retained `OFL.txt` is the upstream license. Its only Reserved Font Name is
`Source`, inherited from Source Han Sans; `Noto Sans KR` is not reserved.

## RTL Subsets

The RTL assets are generated from Google Fonts revision
`26c5c976d82d50c24a8f0a7ac455e0a7c639c226` with FontTools `4.63.0`.
`scripts/make_rtl_font_subsets.py` enforces the tool version and every source,
intermediate, license, and output SHA-256 below. Instancing pins `wght=400` and
`wdth=100`; `--no-recalc-timestamp` makes the output independent of generation
time. Subsetting retains all layout scripts and features, including `GDEF`,
`GSUB`, and `GPOS`.

### Noto Sans Arabic

Source:

- URL: <https://raw.githubusercontent.com/google/fonts/26c5c976d82d50c24a8f0a7ac455e0a7c639c226/ofl/notosansarabic/NotoSansArabic%5Bwdth%2Cwght%5D.ttf>
- Variable font: 844,676 bytes, SHA-256
  `63111b5b2e074dd48cc67692e0a2726d86ee94c1c37fe8598257b7b4e87e869e`.
- Static regular intermediate SHA-256:
  `65efad4885c590b640d9601b8cc6d9b66026e9ff74154ac600207600842c0f70`.
- Upstream license SHA-256:
  `07fc70bfeb985cc1a87a8587d0a0c80bab11c86c9dc3fd95b6f0cb332f983e96`.

Packaged output:

- `fonts/NotoSansArabic-rwml-subset.ttf`: 85,068 bytes, 743 glyphs, 512 cmap
  codepoints, SHA-256
  `9d66a71d753f9853b292c748a9e66272b3cb5b8d898f4e69ebae3ec9e5367880`.
- Exact cmap intervals: `U+0020-007E`, `U+0600-06FF`, `U+0750-077F`,
  `U+08A0-08FF`, `U+200C-2011`, `U+2013-2014`, `U+2018-201A`,
  `U+201C-201E`, `U+2022`, `U+2026`, and `U+25CC`.

This covers the standard Arabic letters and marks, Arabic Supplement and
Arabic Extended-A, Arabic-Indic digits `U+0660-0669`, extended Arabic-Indic
digits `U+06F0-06F9`, Arabic punctuation, bidi controls, common typographic
punctuation, and dotted circle. Presentation-form blocks are intentionally not
mapped; the retained OpenType tables perform contextual shaping.

### Noto Sans Hebrew

Source:

- URL: <https://raw.githubusercontent.com/google/fonts/26c5c976d82d50c24a8f0a7ac455e0a7c639c226/ofl/notosanshebrew/NotoSansHebrew%5Bwdth%2Cwght%5D.ttf>
- Variable font: 112,640 bytes, SHA-256
  `7ef36a2c3593758cdb622e1bdef4f84523e92fbc3ccc667438dd80ff54c2de88`.
- Static regular intermediate SHA-256:
  `5fb2e1fc38c242d64f9fc5f77e026473f288b9791762d80df98a3ec762d5bbdf`.
- Upstream license SHA-256:
  `9b9fe028b5ba74d231659a1bbaf0ed09b11e759d1ca6a070999e16d151616b47`.

Packaged output:

- `fonts/NotoSansHebrew-rwml-subset.ttf`: 26,036 bytes, 208 glyphs, 199 cmap
  codepoints, SHA-256
  `7e39e0a065b66de7b920be4f40223e79dcace3a50c1d8aba33f51ada0c93678f`.
- Exact cmap intervals: `U+0020-007E`, `U+0591-05C7`, `U+05D0-05EA`,
  `U+05EF-05F4`, `U+200C-2010`, `U+2013-2014`, `U+2018-201A`,
  `U+201C-201E`, `U+2022`, `U+2026`, and `U+25CC`.

This is the upstream-supported portion of the requested Hebrew block: all
letters, final forms, points, cantillation marks, Yiddish ligatures, and Hebrew
punctuation in Noto Sans Hebrew, plus Basic Latin, bidi controls, common
typographic punctuation, and dotted circle.

The Arabic and Hebrew license files are retained verbatim as
`OFL-NotoSansArabic.txt` and `OFL-NotoSansHebrew.txt`. Neither upstream license
lists a Reserved Font Name, so the subsets retain their Noto family names.

## RTL Regeneration

Create an isolated environment with the pinned tool, then run the packaged
script:

```sh
python3 -m venv /tmp/rwml-fonttools
/tmp/rwml-fonttools/bin/pip install 'fonttools==4.63.0'
/tmp/rwml-fonttools/bin/python scripts/make_rtl_font_subsets.py
```

The script downloads only into `target/fontprep/rtl`, verifies source and
license hashes before processing, and writes only the two final subset fonts
and two license texts into the packaged tree.
