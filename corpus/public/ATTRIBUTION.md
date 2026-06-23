# Corpus attribution & licenses

## synthetic/

Generated from scratch by `scripts/gen_public_corpus.py` in this repository. No third-party
content; these files are part of rdoc and covered by the repository's own license.

| File | Features exercised |
|------|--------------------|
| `kitchen_sink.docx` | tracked changes (`w:ins`/`w:del`), content control (`w:sdt`), text box (`mc:AlternateContent`/`w:txbxContent`), footnote, comment, table, inline PNG, header/footer |

## vendored/

Real-producer files copied verbatim from permissively-licensed upstreams. Only **MIT** /
**CC0** sources are vendored; each file's origin and license is listed below.

### `vendored/python-docx/` — MIT

From [python-openxml/python-docx](https://github.com/python-openxml/python-docx) `tests/test_files/`
(MIT License, © python-docx contributors). Real Word-produced `.docx` exercising common structures.

| File | Notes |
|------|-------|
| `test.docx` | general document (paragraphs, styles, sections) |
| `having-images.docx` | inline images |
| `blk-inner-content.docx` | block-level content (tables, etc.) |
| `sct-inner-content.docx` | section content |

> openpreserve/format-corpus (CC0) was evaluated but its office corpus contains no `.docx`
> (legacy `.doc`/`.odt` only), so nothing was taken from it.
>
> Excluded by policy: GovDocs1 (redistribution license unconfirmed; almost no `.docx`), the
> maintainer's private real-world corpus (not redistributable), and Apache-POI test files copied
> verbatim (per-file provenance not individually cleared). The legacy binary `.doc` reader is
> validated only against that private corpus — see the scope note in [README.md](README.md).
