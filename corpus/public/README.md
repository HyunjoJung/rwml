# rdoc public validation corpus

A small, **license-clean** corpus of `.docx` files for validating rdoc's reader + the
package-preserving editor in the open (CI and anyone who clones the repo). It complements the
maintainer's larger private real-world corpus, which is not redistributable.

**Scope: `.docx` only.** The legacy binary `.doc` reader is validated against a private local
corpus (Apache-POI / GovDocs1 samples), not here: `.doc` cannot be synthesized cleanly (binary
CFB/OLE2), and redistributing third-party `.doc` files in a public repo raises per-file license
questions we keep out of scope. Public `.doc` coverage is intentionally omitted for that reason.

Every file here is safe to redistribute:

- `synthetic/` — generated from scratch by [`scripts/gen_public_corpus.py`](../../scripts/gen_public_corpus.py).
  You own these outright (no third-party content). They deliberately carry the **unmodeled
  content a package-preserving editor must round-trip intact**: tracked changes (`w:ins`/`w:del`),
  content controls (`w:sdt`), text boxes (`mc:AlternateContent` + `w:txbxContent`), footnotes,
  comments, headers/footers, tables, and an inline PNG image. Regenerate with
  `python scripts/gen_public_corpus.py` (deterministic — a no-op in git if unchanged).
- `vendored/` — a few real-producer files copied from permissively-licensed upstreams
  (CC0 / MIT only). See [`ATTRIBUTION.md`](ATTRIBUTION.md) for the source and license of each.

## What the validation checks

For every `.docx` here, rdoc must:
1. **open** it (`Document::open`);
2. **no-op `open → save` is part-payload byte-stable** — every unmodeled part (footnotes,
   comments, the text box, tracked changes, headers, media, …) round-trips byte-for-byte;
3. **element-tree edit works** — `add_image_png` produces a package that re-opens (python-docx)
   with the new inline image, and the unmodeled content still survives.

Run it with the in-tree example + the python-docx checker:

```sh
cargo run --example validate_edit --features docx -- corpus/public <outdir>
python scripts/validate_edit_check.py corpus/public <outdir>
```
