# Noto Sans KR rdoc Subset Provenance

This crate bundles layout-derived subsets of Noto Sans KR Regular 400 for rdoc
PDF rendering:

- `fonts/NotoSansKR-rdoc-subset.ttf`: slim KS X 1001 Hangul subset.
- `fonts/NotoSansKR-rdoc-subset-full.ttf`: hanja-inclusive subset.

Neither file is the full upstream font.

## Source

- URL: <https://github.com/google/fonts/raw/main/ofl/notosanskr/NotoSansKR%5Bwght%5D.ttf>
- Upstream file: variable font, 10,414,588 bytes
- Upstream sha256:
  `194018e6b2b293a7964f037b25c0249ce1418bc9ab3c971060a03aa57861e252`
- Slim subset sha256:
  `e928aaee9e585e209b82ca7a59e3a843440f134104ee8eb2e084cf44c72a7087`
- Hanja-inclusive subset sha256:
  `9a39382a3f7bab6fa8295830609b9b3a4d5162e575461f8fdd1e55c94b42bcf9`
- Staged: 2026-07-03
- fonttools version: latest from pip on 2026-07-03 (Python 3.14 venv)

## Subset Rule

The subset includes the KS X 1001 wansung set: exactly 2,350 Hangul syllables
from `U+AC00..U+D7A3` whose `euc_kr` encoding has length 2. It also includes
Basic Latin, Latin-1, common punctuation, Hangul compatibility jamo, and related
punctuation ranges used by rdoc layout tests.

The hanja-inclusive subset uses the same source and static instance, then adds
the KS X 1001 hanja set: exactly 4,888 characters from `U+4E00..U+9FFF` and
`U+F900..U+FAFF` whose `euc_kr` encoding has length 2. The hanja text is
concatenated with the same 2,350-syllable Hangul text for the full subset input.

Coverage finding: 4,885 of the 4,888 KS X 1001 hanja characters map in the
hanja-inclusive subset cmap. The 3 missing characters are compatibility
ideographs absent from upstream Noto Sans KR itself. The font family remains
`Noto Sans KR`; the hanja-inclusive subset has 7,416 glyphs.

## Exact Commands

```sh
fonttools varLib.instancer 'NotoSansKR[wght].ttf' wght=400 --update-name-table -o NotoSansKR-static.ttf
```

KS X 1001 text file:

```python
[chr(c) for c in range(0xAC00, 0xD7A4) if len(chr(c).encode('euc_kr', errors='ignore')) == 2]
```

KS X 1001 hanja text file:

```python
[chr(c) for r in [(0x4E00,0xA000),(0xF900,0xFB00)] for c in range(*r) if len(chr(c).encode('euc_kr', errors='ignore')) == 2]
```

```sh
pyftsubset NotoSansKR-static.ttf --text-file=ksx1001.txt --unicodes="U+0020-007E,U+00A0-00FF,U+2010-2027,U+20A9,U+3000-303F,U+3130-318F" --name-IDs='*' --notdef-outline --output-file=NotoSansKR-rdoc-subset.ttf
```

```sh
pyftsubset NotoSansKR-static.ttf --text-file=ksx1001-full.txt --unicodes="U+0020-007E,U+00A0-00FF,U+2010-2027,U+20A9,U+3000-303F,U+3130-318F" --name-IDs='*' --notdef-outline --output-file=NotoSansKR-rdoc-subset-full.ttf
```

## License Finding

The upstream `OFL.txt` was retained verbatim. The only Reserved Font Name listed
there is `Source` (from Source Han Sans ancestry). `Noto Sans KR` is not a
Reserved Font Name, so this subset may keep the `Noto Sans KR` family name under
OFL 1.1. The font's name table retains the license text (`nameID 13` present).
