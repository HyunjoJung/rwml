# rwml-fonts

Bundled OFL-1.1 font subsets for the PDF renderer in
[`rwml`](https://github.com/HyunjoJung/rwml). The bundled faces are regular
weight Noto Sans KR, Noto Sans Arabic, and Noto Sans Hebrew. These are focused
subsets rather than full upstream fonts; the Arabic and Hebrew assets retain the
OpenType layout tables required for shaping and mark positioning.

## API

- `noto_sans_kr_subset()` — slim subset: KS X 1001 wansung Hangul (2,350
  syllables), Hangul compatibility jamo, Basic Latin, Latin-1, and common
  punctuation.
- `noto_sans_kr_subset_with_hanja()` — the above plus KS X 1001 hanja coverage
  (4,885 of 4,888 characters; 3 compatibility ideographs are absent from
  upstream Noto Sans KR itself).
- `noto_sans_arabic_subset()` — Basic Latin, the Arabic, Arabic Supplement,
  and Arabic Extended-A blocks, bidi controls, common punctuation, and dotted
  circle. It includes Arabic and extended Arabic-Indic digits.
- `noto_sans_hebrew_subset()` — Basic Latin, the upstream-supported Hebrew
  letters, points, cantillation marks, punctuation, bidi controls, common
  punctuation, and dotted circle.

Each function returns `&'static [u8]` TTF bytes. The Arabic and Hebrew subsets
retain `GDEF`, `GSUB`, and `GPOS` for script shaping and mark positioning.

## Usage

This crate backs `rwml`'s optional `bundled-fonts` feature, which wires all three
families into the renderer so covered Korean, Arabic, and Hebrew text can render
without system fonts:

```toml
rwml = { version = "0.1", features = ["bundled-fonts"] }
```

The TTF bytes can also be used standalone.

## Provenance & license

The subsets derive from Google Fonts copies of the Noto families under the SIL
Open Font License 1.1. See [`PROVENANCE.md`](PROVENANCE.md) for exact sources,
checksums, coverage, and regeneration commands. The applicable license texts
are [`OFL.txt`](OFL.txt),
[`OFL-NotoSansArabic.txt`](OFL-NotoSansArabic.txt), and
[`OFL-NotoSansHebrew.txt`](OFL-NotoSansHebrew.txt).
