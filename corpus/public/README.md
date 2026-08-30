# rwml public validation corpus

A small, **license-clean** corpus for validating rwml's readers and the `.docx`
package-preserving editor in the open (CI and anyone who clones the repo). It
complements the maintainer's larger private real-world corpus, which is not
redistributable.

The corpus includes generated `.docx` fixtures plus a small generated Word
97-2003 `.doc` extraction benchmark. No third-party `.doc` file is redistributed:
the legacy fixtures were exported from repository-owned synthetic sources, and
their Apache POI and LibreOffice text outputs are checked alongside exact rwml
report expectations. The public hygiene audit scans bounded decoded byte views
of every legacy binary and blocks oversized files.

Every file here is safe to redistribute:

- `MANIFEST.tsv` — expected `Document::report()` feature counts and warning
  classes for every committed public `.docx` fixture, including the generated
  synthetic set and permissively licensed vendored files. It is checked by
  `tests/public_corpus.rs`.
- `RENDER_MANIFEST.tsv` — expected native-render page counts and render warning
  classes for every manifest-listed fixture. It is checked when tests run with
  `--features render` and keeps strict release validation aligned with
  `MANIFEST.tsv`.
- `RENDER_ORACLE.json` — the versioned external-render campaign lock. It binds
  every input to exact bytes and SHA-256, public provenance, canonical feature
  labels, expected page/warning metadata, and explicit resource ceilings.
  `scripts/render_oracle_contract.py` validates the lock before LibreOffice or
  rwml receives any input.
- `RENDER_SMOKE_ORACLE.json` — a generated 12-document local harness profile of
  the same locked inputs. It covers 35 of the parent's 37 feature labels, all
  five expected warning kinds, and 15 expected pages in 69,027 input bytes.
  `alternate-content` and `top-bottom-wrap` are explicitly outside this profile;
  it is diagnostic evidence, not a complete-coverage or release manifest.
- `oracle/` — checked-in identity locks for larger generated diagnostic
  campaigns that are intentionally outside the ordinary release corpus. The
  first lock covers 48 unequal-column table-continuation cases; its exact DOCX
  inputs and strict manifest are generated under ignored `target/` output by
  `scripts/generate_unequal_table_oracle.py`.
- `benchmark/` — three generated `.doc` fixtures, exact report expectations, and
  Apache POI 5.2.3 / LibreOffice 26.2.3.2 extraction goldens. It is also the
  self-contained input for the strict public extraction benchmark.
- `synthetic/` — generated from scratch by [`scripts/gen_public_corpus.py`](../../scripts/gen_public_corpus.py);
  see [`PROVENANCE.md`](PROVENANCE.md) for the per-file purpose and origin.
  You own these outright (no third-party content). They deliberately carry the **unmodeled
  content a package-preserving editor must round-trip intact**: tracked changes (`w:ins`/`w:del`),
  content controls (`w:sdt`), text boxes (`mc:AlternateContent` + `w:txbxContent`), footnotes,
  comments, headers/footers, fields, hyperlinks, nested tables, unsupported object markers, tables, floating shape placement metadata, and an inline PNG image. Dedicated render fixtures activate run paint and hidden text, explicit top-level body tabs, table margins and RTL order, small-page keep pagination, equal-width columns, mixed Arabic/Hebrew direction, bounded `wrapTopAndBottom` flow, and `table-cell-lists.docx` body/direct-cell/nested-cell numbering, bullet fallback, and RTL table placement. Regenerate with
  `python scripts/gen_public_corpus.py` (deterministic — a no-op in git if unchanged).
- `vendored/` — a few real-producer files copied from permissively-licensed upstreams
  (CC0 / MIT only). See [`ATTRIBUTION.md`](ATTRIBUTION.md) for the source and license of each.

## What the validation checks

For every `.docx` here, rwml must:
1. **open** it (`Document::open`);
2. **match expected diagnostics** for manifest-listed synthetic fixtures (`Document::report`);
3. **no-op `open → save` is part-payload byte-stable** — every unmodeled part (footnotes,
   comments, the text box, tracked changes, headers, media, …) round-trips byte-for-byte;
4. **element-tree edit works** — `add_image_png` produces a package that re-opens (python-docx)
   with the new inline image, and the unmodeled content still survives.
5. with `--features render`, **native render reports match the public render
   manifest** and each synthetic fixture emits a non-empty PDF.

For LibreOffice A/B evidence, `scripts/render_validate.py` uses the bundled Noto
subsets by default and reports the retained page-1 aHash plus bounded all-page
aHash, foreground ink IoU, and explicit unmatched/capped page counts. Strict
JSON runs additionally bind the report to the corpus root, source revision,
Cargo lock, harness, platform, tool versions, and recorded LibreOffice identity.
Local exports use fresh per-document LibreOffice profiles. When
`--verify-oracle` is selected, missing or unequal repeated page rasters fail the
evidence gate. Reference PDFs remain temporary and are not committed.

Regenerate, verify, and run the bounded local smoke profile:

```sh
python3 scripts/generate_render_smoke_manifest.py --refresh
python3 scripts/generate_render_smoke_manifest.py --check
python3 scripts/render_validate.py --json --verify-oracle \
  --manifest corpus/public/RENDER_SMOKE_ORACLE.json
```

The generator cross-checks every selected record against `RENDER_ORACLE.json`
and fails if the selected input identities, feature coverage, warnings, page
count, or byte budget drift. Smoke results do not define or relax a fidelity
threshold.

The unequal-column table campaign can be reproduced without expanding the
ordinary release set:

```sh
python3 scripts/generate_unequal_table_oracle.py
python3 scripts/generate_unequal_table_oracle.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/unequal-table-v1/RENDER_ORACLE.json
```

Its checked-in lock covers all 48 combinations of physical column layout,
table-width policy, row-fragment class, and column/page handoff. The lock proves
input identity only; external-render results remain diagnostic until separately
reviewed and accepted. `scripts/table_oracle_topology.py` can reduce complete
PDF output sets to path-neutral synthetic-token, page, border, and continuation-
segment evidence without retaining arbitrary document text.

Run it with the in-tree example + the python-docx checker:

```sh
cargo run --example validate_edit --features docx -- corpus/public <outdir>
python scripts/validate_edit_check.py corpus/public <outdir>
```
