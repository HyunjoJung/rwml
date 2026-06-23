# PRD — rdoc package-preserving `.docx` editing (A → B)

> **Status update (shipped):** The model-overlay surface (**A**, `body_mut()`) was
> **removed**. It regenerated `document.xml` from the lossy model, which structurally
> cannot preserve body-coordinated constructs (headers, comments, footnotes, fields,
> list identity…). The shipped editor is **B only**: the element-tree edit surface
> (`replace_body_text` / `add_image_png`, package-preserving), `write_docx`
> (author/convert from a `DocModel`, intentionally lossy), and `save()` (passthrough).
> The A→B narrative below is retained as design history.

**Status:** Draft · **Owner:** rdoc · **Scope:** the `.docx` write/edit surface only
(reading, rendering, and the `.doc` path are unchanged). · **Target end-state: B**
(full package-preserving, element-tree editing, python-docx/POI-grade fidelity),
reached in two shippable phases with **A** (passthrough-save) first.

## 1. Why

rdoc's original **write** path had one structural limitation: it was a
from-scratch generator, not an editor. `write_docx` folds the lossy `DocModel`
into a freshly synthesized package, and `docx::open` *discards the original ZIP*
the moment it returns
([`src/docx/mod.rs`](../src/docx/mod.rs): `DocxState { model, text, main_text }`).
So "open a real Word document, change one thing, save" silently drops everything
the model doesn't represent: `theme1.xml`, `settings.xml`, `fontTable.xml`,
`docProps/*`, `customXml/*`, comments, glossary, embedded objects, charts/SmartArt
(`mc:AlternateContent`), real `styles.xml`/`numbering.xml`, fields, bookmarks, and
tracked-change originals.

The mature libraries don't have this problem because they made a different
architectural choice. This PRD adopts it.

## 2. What the mature libraries actually do (prior art)

| | python-docx 1.2 | Apache POI XWPF | docx-rs (Rust) | **rdoc today** |
|---|---|---|---|---|
| Model | live lxml tree + thin proxies | OPC parts + XMLBeans schema tree + XWPF wrappers | owned typed structs (builder) | **lossy `DocModel` (Pandoc-style blocks)** |
| Create "new" | opens a **bundled `default.docx`** template | `new XWPFDocument()` loads a blank package | `Docx::new()` synthesizes | **synthesizes from scratch** |
| Edit surface | mutate the real element tree | mutate the real schema tree | rebuild the owned model | regenerate from model |
| **Unknown parts** | **raw `Part.blob` passthrough — verbatim** | **generic `PackagePart` passthrough** | **discarded on read** | **discarded on read** |
| Round-trip fidelity | high (re-serialized, not bit-exact) | high (re-serialized) | **lossy (model-only)** | **lossy (model-only)** |
| `.doc` | ✗ | ✗ | ✗ | ✓ (read) |
| Render | ✗ | ✗ | ✗ | ✓ |
| Runtime | Python | **JVM + ~6–16 MB schema jar** | pure Rust | pure Rust |
| Error API | optimistic, few exceptions | checked exceptions | `Result` + split error enums | infallible `Vec<u8>` |

**The one lesson all three editors teach:** never re-emit a document by serializing
your own model when a source package exists. *Make the OPC package the source of
truth and edit it in place.* python-docx: the base `Part` returns its load bytes
unchanged, so parts it has no class for "are left alone and saved without
understanding what they are." POI: fidelity is a property of the **data model**
(full schema + preserved package), not of the wrapper API. docx-rs proves the
negative — `read_docx(&[u8]) -> Docx` throws the envelope away, so it cannot safely
edit a foreign file.

**What rdoc should borrow specifically**

- python-docx / POI: package-as-source-of-truth; **raw-bytes passthrough** for
  unmodeled parts; **template-based creation** (ship a blank `.docx`); auto-mint
  relationships when adding a part.
- docx-rs: **builder ergonomics** for authoring (`Docx::new().add_paragraph(…)`)
  and **`Result`-everywhere** with read/write/render distinguished.
- Avoid: docx-rust's `DocxFile`/`Docx` lifetime coupling (use `&[u8]` entry
  points); POI's JVM weight + schema-jar; docx-rs's lossy single-model storage.

## 3. Goals / Non-goals

**Goals**
- **G1 (A):** `open(bytes) → save() -> Vec<u8>` round-trips a real `.docx` with
  **every part rdoc didn't deliberately rewrite preserved byte-for-byte** (theme,
  settings, fontTable, styles, numbering, comments, customXml, embeddings, charts,
  unknown/future parts). A no-op open→save changes nothing.
- **G2 (A):** targeted body edits (replace/insert/delete paragraphs, runs, table
  cells, set run/paragraph formatting) that rewrite *only* `word/document.xml`
  while every other part passes through; **relationships stay consistent** (no
  orphaned/colliding `rId`).
- **G3 (A):** template-based creation — `Document::new()` opens a bundled blank
  package so generated docs are Word-valid without hand-authoring boilerplate.
- **G4 (B):** edits apply to a **preserved element tree** for `document.xml` (and
  any edited part), so **unmodeled body content survives** — fields, bookmarks,
  content controls (`w:sdt`), `mc:AlternateContent` shapes, `w:del`, equations.
- **G5 (B):** structural edits + new satellite parts (add image / comment /
  footnote / header) with transactional reference integrity (part + content-type +
  rels allocated together).
- **G6:** fallible public API (`try_write_docx`/`save -> Result`) and builder
  authoring ergonomics; keep `#![forbid(unsafe_code)]`, panic-free, bounded.

**Non-goals**
- Editing the legacy binary **`.doc`** in place (no OLE write-back). `.doc` opens
  → convert to `.docx` (lossy through the model) or read-only.
- A schema-complete generated model (the POI/XMLBeans route) — overkill for Rust.
- Re-flowing/rendering edits, recalculating fields/TOC/page numbers (that is the
  renderer's and Word's job).
- Bit-identical re-serialization of *parsed/edited* parts (only *untouched* parts
  are byte-stable; see Risks).

## 4. Design principles

1. **The package is the source of truth.** On open, retain every part. On save,
   re-zip the retained parts, overwriting only the ones an edit touched. Default =
   passthrough, never "drop what I don't model."
2. **The lossy `DocModel` is a read/render *view*, not edit storage.** Edits never
   round-trip through `DocModel` (that is exactly what loses data). `DocModel`
   stays the convenient query/Markdown/HTML/PDF projection.
3. **Lazy-parse beats eager-parse.** Keep each part as raw bytes; promote a part to
   a parsed, editable tree **only when it is actually edited**. This makes
   untouched parts byte-stable — *better than python-docx/POI*, which eagerly
   re-serialize parts they understand and thus churn namespaces/whitespace even on
   a no-op.
4. **Preserve the long tail as opaque nodes.** The edit tree keeps unknown
   elements/attributes/namespaces/`mc:*` verbatim; typed accessors cover the common
   path, the preserved tree carries everything else (the escape hatch docx-rs
   lacks — no second "raw lxml" API needed by users).
5. **Centralize relationship reconciliation.** One `rId` allocator seeded from the
   max existing id across *all* rels; adding a part mints its `rId` + content-type
   + rels entry transactionally; references rdoc rewrites are remapped consistently.
6. **Separate the package layer from the body producer.** This is what keeps A→B
   from tangling (see §6).

## 5. API design (python-docx direction, Rust ergonomics)

```rust
// OPEN existing — the fidelity path (A)
let mut doc = rdoc::Document::open(&bytes)?;        // retains the full package

// CREATE new — template-based (A)
let mut doc = rdoc::Document::new();                // opens the bundled blank .docx

// EDIT — phase A: model-overlay edits that rewrite only document.xml
doc.body_mut().push_paragraph(Paragraph::new().run(Run::new("Hello").bold()));

// EDIT — phase B: structural / preserving edits over the live tree
let p = doc.paragraphs().find(|p| p.text().contains("DRAFT"))?;
p.runs_mut()[0].set_text("FINAL");                  // unmodeled siblings untouched
doc.add_comment(p.range(), "looks good", "Joe");    // new part + rels, transactional

// SAVE — re-zips retained parts, overwrites only what changed
let out: Vec<u8> = doc.save()?;                     // fallible: Result<Vec<u8>, Error>

// AUTHOR from data (borrow docx-rs builder shape; generation, not editing)
let bytes = rdoc::Docx::new()
    .add_paragraph(Paragraph::new().add_run(Run::new("Report").bold().size(24)))
    .save()?;
```

- `Document::open`/`new` both hold a retained package; `save()` is fallible
  (surfaces packaging/serialization errors instead of empty bytes). Keep the
  infallible `write_docx(&DocModel) -> Vec<u8>` as the simple
  generator entry, plus `try_write_docx -> Result`.
- `&[u8]`/`Vec<u8>` entry points only — no source-lifetime coupling.

## 6. Architecture — and why A → B does not tangle

The user's concern: does shipping A first paint us into a corner B has to undo?
**No — if the package layer is separated from the body producer.** A and B are two
body strategies over one shared package core.

```
            ┌──────────────────────────────────────────────┐
            │  opc::Package  (round-trip layer, A + B share) │
            │  • parts: map<PartName, PartBytes|ParsedTree>  │  ← lazy-parse
            │  • [Content_Types]: defaults + overrides       │
            │  • rels graph (per part) + rId allocator       │
            │  • from_zip(bytes) / to_zip() (re-zip retained)│
            └───────────────┬──────────────────────────────┘
                            │ body producer (pluggable)
        ┌───────────────────┴───────────────────────┐
        │ A: passthrough / regen-document.xml        │  B: tree-edit document.xml
        │   • untouched parts → raw bytes verbatim   │    • parse part → preserved
        │   • edited body → rewrite document.xml from │      element tree (quick-xml
        │     model + **rels merge** (keep unmodeled  │      Reader→nodes→Writer)
        │     rels, remap rewritten refs)            │    • mutate nodes in place
        └────────────────────────────────────────────┘    • rels passthrough (no
                                                             renumber → simpler)
```

**Shared core, built in A, reused unchanged by B:**
- retain-original-package on open (today [`docx::open`](../src/docx/mod.rs) drops
  the zip — add an `OpenPackage`);
- `opc::Package::from_zip` reader + `to_zip` that re-emits retained entries,
  overriding only changed parts (today [`src/write/opc.rs`](../src/write/opc.rs)
  `Package` is write-only — make it round-trippable);
- **content-type carry-through** — start from the original's `[Content_Types]`
  tables, add/replace only for rewritten parts (don't reset to the hardcoded seed);
- **`rId` allocator** seeded from max-existing across all rels (replaces the naive
  monotonic counter in `write/docx.rs` `add_rel`, which orphans/collides).

**Where A and B differ — only the body producer:**
- A regenerates `document.xml` from the model, so it must **merge rels** (preserve
  rels to unmodeled parts like comments/customXml + remap its own) and accepts that
  unmodeled *body* elements (sdt, fields, shapes) are not preserved (documented).
- B edits the *live* `document.xml` tree, so it **does not renumber `rId`s** → rels
  just pass through (B is *simpler* on rels), and unmodeled body elements survive.

**The three rules that keep it clean** (from §4 + the research):
1. package/passthrough is its own module, independent of the body model;
2. `rId` reconciliation is centralized in the allocator (A needs merge, B doesn't —
   A's extra work never blocks B);
3. `save()`'s contract is explicit per mode — A = "rewrites body from model
   (unmodeled body content not preserved); other parts preserved"; B = full
   preservation. Two edit surfaces (model-overlay vs tree) coexist as documented
   modes (docx-rs ships a builder + reader; POI ships wrappers + schema), which is
   acceptable, not a structural tangle.

**Accepted impurity:** two edit surfaces. Mitigated by clear naming/contracts and
by `DocModel` staying read-only-ish (the query/render view), never the B edit store.

## 7. Phases & acceptance

### Phase A — passthrough-save + targeted edits + template creation
1. `opc::Package` round-trip: `from_zip`/`to_zip`, retained part bytes, parsed
   `[Content_Types]`, full rels graph, `rId` allocator. *(Reusable by `rxls`.)*
2. `Document::open` retains the package; `Document::new()` opens a bundled blank
   `.docx` template (commit a minimal valid package as a crate asset).
3. `save() -> Result<Vec<u8>>`: re-zip retained parts; for the body, regenerate
   `document.xml` from the (edited) model **with rels merge**.
4. `try_write_docx`/fallible APIs; keep `write_docx` infallible for simple gen.

**A acceptance:**
- No-op `open→save` on the POI/GovDocs `.docx` corpus: **every part except
  (optionally) `document.xml` is byte-identical**; the file re-opens in Word and
  LibreOffice; a new round-trip test asserts part-set + bytes equality for
  untouched parts.
- After a body text edit, `theme1.xml`/`settings.xml`/`fontTable.xml`/`styles.xml`/
  `numbering.xml`/`customXml`/comments still present and referenced (no orphan/
  collision); validated by re-open + a rels-integrity check.
- Panic-free + clippy `-D --all-features`; fuzz the package round-trip.

### Phase B — preserved element-tree editing
5. A faithful `document.xml` (and any edited part) element tree via `quick-xml`
   `Reader`→owned nodes→`Writer`, preserving unknown elements, attribute order,
   namespace decls, and `mc:*`. Lazy: parse a part only when edited.
6. Thin typed wrappers over real nodes (`Paragraph`→`w:p`, `Run`→`w:r`,
   `Table`→`w:tbl`) — getters/setters read/write the element; unmodeled siblings
   ride along.
7. Structural edits + new satellite parts (image/comment/footnote/header) with
   transactional `rId`+content-type+rels allocation against the retained package.

**B acceptance:**
- Edit one run in a doc containing comments + a chart + a content control + a
  field → save → **all of comments/chart/sdt/field survive** and the edit applies;
  golden test diffs the tree (only the edited node changed).
- Insert an image into a foreign doc → new media part + content-type + rels with a
  non-colliding `rId`; re-opens clean.
- Re-run the write/edit validation against python-docx/docx-rs for the
  open→edit→save use case.

## 8. Success metrics
- **Fidelity:** ≥99% of a real `.docx` corpus round-trips (open→save) with
  untouched parts byte-identical and re-opens in Word + LibreOffice.
- **Preservation under edit (B):** comments/themes/styles/charts/sdt/fields present
  after a body edit on 100% of corpus docs that contain them.
- **Competitiveness:** beats docx-rs/python-docx on the "edit a foreign doc without
  losing content" task (they drop unmodeled content; rdoc preserves it) — and is
  the only pure-Rust option that also reads `.doc` and renders.
- **Hygiene:** panic-free, fuzzed, `Result` APIs, accurate docs.

## 9. Risks & mitigations
- **`rId` reconciliation (top hazard).** One allocator, seeded from max-existing;
  A merges rels, B passes through. Tested with rels-integrity assertions + fuzz.
- **Re-serialization churn.** Only *edited* parts re-serialize; untouched parts are
  raw passthrough (lazy-parse), so they stay byte-stable — we sidestep the
  python-docx/POL churn for the common case.
- **Two edit surfaces.** Documented modes; `DocModel` stays the read/render view;
  B is the preservation path. Don't grow `DocModel` into the edit model (POI's
  history shows the model must be the tree).
- **Scope creep toward a schema model.** Explicitly out of scope — preserved
  generic tree + typed accessors for the common path only.
- **`.doc` confusion.** `.doc` is read/convert-only; editing is `.docx`-only and
  the API makes that explicit.

## 10. Out of scope / future
- `.doc` (binary) in-place editing; field/TOC recalculation; rendering of edits;
  full DrawingML/chart authoring (preserved, not authored); a schema-complete model.

## References
- python-docx (python-openxml/python-docx) — `Part.blob` passthrough; `default.docx`
  template; tree-as-model.
- Apache POI XWPF / OpenXML4J — `OPCPackage` + XMLBeans schema model; fidelity is a
  property of the data model.
- docx-rs (bokuweb/docx-rs) — builder + `read_docx(&[u8])`; lossy single-model
  (the anti-pattern); `Result` ergonomics to borrow.
- ECMA-376 OPC: `[Content_Types].xml`, `_rels` graph, relationship `rId` resolution,
  markup compatibility (`mc:AlternateContent`).
- rdoc current state: [`src/docx/mod.rs`](../src/docx/mod.rs),
  [`src/write/docx.rs`](../src/write/docx.rs), [`src/write/opc.rs`](../src/write/opc.rs),
  [`src/model.rs`](../src/model.rs).
