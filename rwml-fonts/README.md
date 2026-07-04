# rwml-fonts

Bundled [OFL-1.1](OFL.txt) font subset for the PDF renderer in
[`rwml`](https://github.com/HyunjoJung/rwml). The bundled faces are **Noto Sans
KR Regular (400)** — layout-derived subsets, not the full upstream font.

## API

- `noto_sans_kr_subset()` — slim subset: KS X 1001 wansung Hangul (2,350
  syllables), Hangul compatibility jamo, Basic Latin, Latin-1, and common
  punctuation.
- `noto_sans_kr_subset_with_hanja()` — the above plus KS X 1001 hanja coverage
  (4,885 of 4,888 characters; 3 compatibility ideographs are absent from
  upstream Noto Sans KR itself).

Both return `&'static [u8]` TTF bytes with the family name `Noto Sans KR`.

## Usage

This crate backs `rwml`'s optional `bundled-fonts` feature, which wires the
subsets into the renderer so Korean PDFs render without a system font installed:

```toml
rwml = { version = "0.1", features = ["bundled-fonts"] }
```

The TTF bytes can also be used standalone.

## Provenance & license

The subsets derive from Google's Noto Sans KR (SIL Open Font License 1.1). See
[`PROVENANCE.md`](PROVENANCE.md) for the exact upstream source, the subsetting
process (`scripts/make_font_subset.py`), and coverage, and [`OFL.txt`](OFL.txt)
for the full license. Redistribution is OFL-compliant; "Noto Sans KR" is not an
OFL reserved font name.
