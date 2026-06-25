# rdoc public validation corpus

A small, **license-clean** corpus of `.docx` files for validating rdoc's reader + the
package-preserving editor in the open (CI and anyone who clones the repo). It complements the
maintainer's larger private real-world corpus, which is not redistributable.

**Scope: `.docx` only.** The legacy binary `.doc` reader is validated against a private local
corpus (Apache-POI / GovDocs1 samples), not here: `.doc` cannot be synthesized cleanly (binary
CFB/OLE2), and redistributing third-party `.doc` files in a public repo raises per-file license
questions we keep out of scope. Public `.doc` coverage is intentionally omitted for that reason.
If a legacy `.doc` file is accidentally added to the public tree, the public hygiene audit still
scans bounded decoded byte text views and blocks oversized legacy binaries.

Every file here is safe to redistribute:

- `MANIFEST.tsv` — expected `Document::report()` feature counts and warning
  classes for the synthetic fixtures. It is checked by `tests/public_corpus.rs`.
- `RENDER_MANIFEST.tsv` — expected native-render page counts and render warning
  classes for the synthetic fixtures. It is checked when tests run with
  `--features render`.
- `synthetic/` — generated from scratch by [`scripts/gen_public_corpus.py`](../../scripts/gen_public_corpus.py).
  You own these outright (no third-party content). They deliberately carry the **unmodeled
  content a package-preserving editor must round-trip intact**: tracked changes (`w:ins`/`w:del`),
  content controls (`w:sdt`), text boxes (`mc:AlternateContent` + `w:txbxContent`), footnotes,
  comments, headers/footers, fields, hyperlinks, nested tables, unsupported object markers, tables, and an inline PNG image. Regenerate with
  `python scripts/gen_public_corpus.py` (deterministic — a no-op in git if unchanged).
- `vendored/` — a few real-producer files copied from permissively-licensed upstreams
  (CC0 / MIT only). See [`ATTRIBUTION.md`](ATTRIBUTION.md) for the source and license of each.

## What the validation checks

For every `.docx` here, rdoc must:
1. **open** it (`Document::open`);
2. **match expected diagnostics** for manifest-listed synthetic fixtures (`Document::report`);
3. **no-op `open → save` is part-payload byte-stable** — every unmodeled part (footnotes,
   comments, the text box, tracked changes, headers, media, …) round-trips byte-for-byte;
4. **element-tree edit works** — `add_image_png` produces a package that re-opens (python-docx)
   with the new inline image, and the unmodeled content still survives.
5. with `--features render`, **native render reports match the public render
   manifest** and each synthetic fixture emits a non-empty PDF.

Run it with the in-tree example + the python-docx checker:

```sh
cargo run --example validate_edit --features docx -- corpus/public <outdir>
python scripts/validate_edit_check.py corpus/public <outdir>
```
