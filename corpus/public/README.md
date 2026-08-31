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
  unequal-table lock covers 48 table-continuation cases. The render-pilot lock
  combines all 21 ordinary public inputs with 19 focused generated inputs, 51
  expected pages, and 71 feature labels. The first five full-corpus batch
  locks separately cover 64 generated run-paint cases, 64 generated paragraph-
  geometry cases, 64 generated list/RTL interaction cases, and 64 generated
  table topology/paint cases, plus 64 generated section/column/running-surface
  cases. Each batch has balanced factors and complete declared pairwise
  coverage. Exact DOCX inputs and strict manifests are generated under ignored
  `target/` output.
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
JSON evidence v4 also retains raw integer RGB error counts, integer PPM scores,
one-pixel-matched foreground/edge/conservative text-ink masks, matched foreground
color error, three-pixel blurred-luma similarity, and fixed work-unit accounting.
Document and campaign values are recomputed from raw counts rather than averaging
rounded page scores. The metric implementation and constants are explicit, and the
Python reference and pinned NumPy paths are required to produce identical results.
The same bounded PDF reader records page, MediaBox, and CropBox coordinates in
millipoints and emits content-free token, codepoint, and token-bigram counts with
integer PPM precision/recall/F1. It normalizes NFC, removes only listed layout
direction controls, preserves page boundaries, and retains no document text.
Word and line boxes are compared only when their exact normalized token tuple is
unique on both sides; repeated labels are reported as ambiguous, never greedily
paired. Signed millipoint summaries and bounded histograms retain no token text.
These diagnostics do not define or relax a fidelity threshold. Strict JSON runs
additionally bind the report to the corpus root, source revision, Cargo lock,
harness, platform, tool versions, and recorded LibreOffice identity.
Local exports seed and initialize fresh per-document LibreOffice profiles before
conversion. The seed maps observed Office and platform fallback families to the exact
LibreOffice-bundled Noto files pinned by `oracle/libreoffice-font-lock.json`; the
harness verifies those files and each reference PDF's embedded PostScript names and
SFNT revisions. Strict corpus runs always require zero skipped documents. When
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

Build and run the broader 40-document diagnostic pilot:

```sh
python3 scripts/generate_render_pilot.py
python3 scripts/generate_render_pilot.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-pilot-v1/RENDER_ORACLE.json
python3 scripts/render_validate.py --json --verify-oracle \
  --manifest target/render-oracle/render-pilot-v1/RENDER_ORACLE.json
```

The checked-in lock binds the parent manifest, both generator sources, copied
provenance records, and all 40 input payloads. The 19 additions exercise run
paint, paragraph geometry, mixed sections, RTL text/lists/tables, fields and
notes, table merging/spacing/continuation, floating and inline objects, revision
structure, and Unicode line breaking. Pilot results remain diagnostic and do not
change release policy or fidelity thresholds.

Input reproducibility is not a fidelity result. The pilot retains same-line
Arabic/Hebrew/Latin direction changes, mixed RTL numbers, CJK and emoji,
discretionary Unicode breaks, document-count fields, and distinct tracked-deletion
text. These cases must not be simplified or given duplicate visible text to
improve recall. An unavailable locked font is an explicit skip, not a pass;
use `--max-skipped 0` to make incomplete measurements fail the campaign gate.
Oracle view differences and text-extractor disagreements require investigation,
not a reduced input surface or a parity claim.

Materialize and verify the reviewed full-corpus batches:

```sh
python3 scripts/generate_render_full_corpus.py
python3 scripts/generate_render_full_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-run-paint-v1/RENDER_ORACLE.json

python3 scripts/generate_render_paragraph_corpus.py
python3 scripts/generate_render_paragraph_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-paragraph-v1/RENDER_ORACLE.json

python3 scripts/generate_render_list_rtl_corpus.py
python3 scripts/generate_render_list_rtl_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-list-rtl-v1/RENDER_ORACLE.json

python3 scripts/generate_render_table_corpus.py
python3 scripts/generate_render_table_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-table-v1/RENDER_ORACLE.json

python3 scripts/generate_render_section_corpus.py
python3 scripts/generate_render_section_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-section-v1/RENDER_ORACLE.json

python3 scripts/generate_render_note_field_corpus.py
python3 scripts/generate_render_note_field_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-note-field-v1/RENDER_ORACLE.json

python3 scripts/generate_render_metafile_corpus.py
python3 scripts/generate_render_metafile_corpus.py --check
python3 scripts/render_oracle_contract.py \
  target/render-oracle/render-full-metafile-v1/RENDER_ORACLE.json
```

The 64 one-page inputs form a complete orthogonal lattice over bold, italic,
underline, strike, font size/color, highlight, caps, small caps, super/subscript,
and hidden text. Each property appears in 32 inputs; every pair has all four
on/off states exactly 16 times. The batch is generated, MIT-licensed, bounded,
and byte-locked. It is one reviewed component of the planned 800-case corpus,
not a completed full campaign, fidelity threshold, or release requirement.

The second 64-input lattice covers center/right/justified alignment, left/right/
first-line/hanging indentation, before/after spacing, automatic/exact/minimum
line spacing, paragraph shading and borders, and explicit tabs. Mutually
exclusive values use separate labeled paragraphs, so pairwise coverage remains
document-scoped. It has the same generated provenance, limits, and non-release
status as the run-paint batch.

The third 64-input batch is the complete two-level factorial over six factors
that meet in one primary list paragraph: Arabic/Hebrew script, paragraph bidi,
run RTL, ordered/bullet numbering, level zero/one, and plain/tabbed content.
Every factor level occurs in 32 cases and every factor pair has all four states
16 times. Fixed supplemental probes cover numbering starts, replacement levels,
three-level labels, and bullet fallback. This remains bounded diagnostic input
evidence, not complete RTL support, external-oracle fidelity, or a release gate.

The fourth 64-input batch holds width, fixed layout, equal grid columns, and
one-page geometry constant while varying visual RTL order, horizontal and
vertical spans, uniform/asymmetric borders, cell shading, and inherited/direct
cell margins in one primary table. A fixed bottom-aligned cell with a taller
peer makes vertical placement observable. Width-policy, row-fragment, and
column/page-handoff coverage remains in the unequal-column oracle. This batch is
diagnostic input evidence, not external fidelity or a release requirement.

The fifth 64-input batch varies next-page/odd-page section starts, portrait/
landscape geometry, equal/unequal two-column layouts, LTR/RTL column progression,
column separators, and quarter-inch/half-inch running-surface distances in one
three-page final section. Distinct first, even, and default headers and footers
make page and section selection observable; odd-page cases retain the parity
filler as a fifth page. The explicit page and column breaks bound the input but
do not establish Word-exact automatic pagination, external fidelity, or a
release requirement.

The sixth 64-input batch combines footnotes/endnotes, numbering starts one/five,
decimal/lower-Roman formats, simple/complex `NOTEREF`, plain/accepted-insertion
contexts, and body/table placement in one primary note-reference interaction.
Fixed controls cover a preceding custom mark, a deleted reference decoy,
accepted/rejected note-body revisions, a deterministic formula, and note IDs
and part order unrelated to visible sequence. This batch establishes bounded
accepted-current input coverage, not page-bottom note placement, Word-exact
pagination, external fidelity, or a release requirement.

The seventh 64-input batch combines EMF/WMF containers, raw/gzip payloads,
source-blit/SETDIB records, indexed-palette/RGB565 bitfield DIBs, direct-body/
table-cell placement, and zero/ninety-degree rotation in one primary image. All
representation combinations encode the same generated 160-by-80 four-quadrant
raster, making decode equivalence independently observable. This batch covers
the strict single-DIB subset, not general metafile vector replay, floating-object
layout, external fidelity, or a release requirement.

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
