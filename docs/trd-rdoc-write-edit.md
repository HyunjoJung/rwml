# TRD — rdoc package-preserving `.docx` editing (A → B)

> **Status update (shipped):** Milestone **A** (model-overlay `body_mut()` +
> `apply_body_overlay`) was **removed** — regenerating `document.xml` from the lossy
> model cannot preserve body-coordinated constructs. The shipped editor is **B**
> (element-tree
> `replace_body_text` / `add_image_png`) plus `write_docx` (lossy author/convert) and
> passthrough `save()`. References to A / `body_mut` / `apply_body_overlay` below are
> design history.
>
> **Status: COMPLETE (B shipped, 2026-07-03).** Also superseded as design
> history: the §5.2 `ParaRef`/`RunRef`/`set_run_text`/`insert_paragraph_after`
> handle API sketch — the shipped surface is the targeted `Document` edit
> method set (`replace_body_text`, `set_field_result`,
> `fill_content_control(s)_by_tag`, `fill_template_fields`,
> `set_hyperlink_target`, `set_comment_text`, `add_comment_on_text`,
> `set_table_cell_text`, `replace_note_text`, `replace_header_footer_text`,
> `replace_text_in_part`, `accept/reject_all_revisions`, `set_core_property`,
> plus the image/footnote/endnote insert family). §2's module layout shipped
> as single modules (`src/opc.rs`, `src/xmltree.rs`) with the edit surface on
> `impl Document` in `src/lib.rs`; `src/write/opc.rs` remains the independent
> from-scratch generator. XmlTree round-trip is unit-tested (idempotent
> serialize, depth caps); the package open→save path is fuzzed via the `parse`
> target, with a scripted edit-surface fuzz target planned.

Technical design for [`prd-rdoc-write-edit.md`](prd-rdoc-write-edit.md). Implements
**A** (passthrough-save + retained package + targeted body edits + template
creation) then **B** (preserved element-tree editing). Constraints carried from the
crate: `#![forbid(unsafe_code)]`, panic-free on hostile input, bounded
(zip-bomb/recursion caps), no new heavy deps (reuse `zip` + `quick-xml`).

## 1. Current code inventory (what we build on / change)

| Area | File:item | State | Action |
|---|---|---|---|
| OPC writer | [`write/opc.rs`](../src/write/opc.rs) `Package` `{parts, defaults, overrides, rels}` + `into_zip` | **write-only**; `new()` seeds `rels`+`xml` defaults | promote to a round-trippable `opc::Package` with `from_zip` |
| Rel | `write/opc.rs` `Rel{id,rel_type,target,external}` | fine | reuse; add parse |
| docx open | [`docx/mod.rs`](../src/docx/mod.rs) `open` → `DocxState{model,text,main_text}` | **drops the `ZipArchive`** | retain the package |
| rId minting | [`write/docx.rs`](../src/write/docx.rs) `Ctx::add_rel` (monotonic counter) | collides/orphans against a foreign package | replace with `RIdAllocator` |
| styles/numbering | `write/docx.rs` `styles_xml`/`numbering_xml` | hardcoded | passthrough originals when editing |
| body writer | `write/docx.rs` `to_docx` (string concat from `DocModel`) | from-scratch | becomes the **A body producer** |
| IR | [`model.rs`](../src/model.rs) `DocModel` | lossy | stays the **read/render view**, never the B edit store |

## 2. Module layout (target)

```
src/opc/                     ← NEW: format-agnostic OPC round-trip layer (reusable by rxls)
  mod.rs        Package (from_zip / to_zip / part access / set_part)
  rels.rs       Rel, RelsGraph, RIdAllocator
  ctypes.rs     ContentTypes (defaults + overrides; lookup, carry-through)
src/xmltree/                 ← NEW (Phase B): faithful, edit-preserving XML tree
  mod.rs        XmlTree (arena), Node, NodeId, parse(), serialize()
  edit.rs       structural ops (insert/remove/replace child, set attr)
src/docx/
  mod.rs        open(): retain Package; existing reader unchanged
  edit.rs       NEW: Document edit surface (A overlay ops; B tree wrappers)
src/write/
  opc.rs        thin re-export/shim → src/opc (keep call sites working)
  docx.rs       A body producer: regenerate document.xml + rels-merge
assets/
  blank.docx    NEW: bundled minimal valid package for Document::new()
```

`src/write/opc.rs`'s current `Package` (write-only builder) is **subsumed** by
`src/opc::Package`; the from-scratch generator keeps working by building an empty
`Package` and `set_part`-ing into it, so `write_docx(&DocModel)` is unchanged.

## 3. Data structures

### 3.1 `opc::Package` (Phase A foundation)

```rust
pub(crate) struct Package {
    /// Parts in original ZIP order (order preserved for stable re-zip).
    order: Vec<String>,                 // part names, incl. [Content_Types] handled separately
    parts: HashMap<String, Part>,       // part name (no leading '/') -> content
    ctypes: ContentTypes,
    rels: RelsGraph,                    // rels-part-path -> Vec<Rel>
}

enum Part {
    Raw(Vec<u8>),       // verbatim bytes — default; byte-stable passthrough
    Xml(XmlTree),       // promoted lazily only when edited (Phase B)
}

struct ContentTypes { defaults: Vec<(String,String)>, overrides: Vec<(String,String)> }
struct RelsGraph { by_part: HashMap<String, Vec<Rel>> }  // key = "_rels/.rels", "word/_rels/document.xml.rels", …

struct RIdAllocator { next: u32 }       // seeded from max existing rId across ALL rels
```

Key methods:

```rust
impl Package {
    fn from_zip(bytes: &[u8]) -> Result<Package>;     // parse: CT, all *_rels, rest = Part::Raw
    fn to_zip(&self) -> Result<Vec<u8>>;              // re-emit in `order`; Raw verbatim, Xml serialized
    fn part(&self, name: &str) -> Option<&[u8]>;      // raw view (serializes Xml on demand for read)
    fn set_part(&mut self, name: &str, bytes: Vec<u8>, ct: Option<&str>); // add/replace + content-type
    fn remove_part(&mut self, name: &str);            // also drops its rels + override
    fn rels_for(&self, part: &str) -> &[Rel];         // resolve a part's _rels
    fn alloc_rid(&mut self) -> String;                // "rId{n}" above all existing
    fn add_related_part(&mut self, src: &str, rel_type: &str, name: &str,
                        ct: Option<&str>, bytes: Vec<u8>) -> String; // transactional → rId
    fn part_tree_mut(&mut self, name: &str) -> Result<&mut XmlTree>; // lazy Raw→Xml (Phase B)
}
```

`from_zip` reuses the bounded read helpers (`MAX_XML_PART`/`MAX_MEDIA_PART`/
`MAX_TOTAL_MEDIA`) already in `docx/mod.rs`; total-archive budget applies to the
retained bytes too.

### 3.2 `RIdAllocator` — the central correctness primitive

Seed: scan every `Rel.id` in `RelsGraph`, parse the trailing integer of `rId{n}`,
`next = max + 1`. `alloc_rid()` returns `format!("rId{next}")` then increments.
This guarantees any relationship rdoc *adds* never collides with a preserved one.
Replaces `write/docx.rs::Ctx::add_rel`'s naive counter (which starts at 1 and
collides with a foreign package's `rId1`).

### 3.3 `XmlTree` (Phase B) — faithful, edit-preserving

Arena tree (indextree/ego-tree style) — chosen over `Rc<RefCell<Node>>` to stay
idiomatic, allocation-cheap, and cycle-free; handles are `Copy`.

```rust
pub(crate) struct XmlTree { nodes: Vec<NodeData>, root: NodeId }
#[derive(Clone, Copy, PartialEq)] pub(crate) struct NodeId(u32);

enum Node {
    Element { name: Box<[u8]>,            // raw qualified name incl. prefix, e.g. b"w:p"
              attrs: Vec<(Box<[u8]>, Box<[u8]>)>,  // ordered, raw key/value (ns decls included)
              empty: bool },              // was it <x/> vs <x></x>
    Text(Box<[u8]>),                      // raw, unescaped-on-write preserved
    CData(Box<[u8]>), Comment(Box<[u8]>), PI(Box<[u8]>), Decl(Box<[u8]>),
}
struct NodeData { node: Node, parent: Option<NodeId>, children: Vec<NodeId> }
```

- Built by a `quick_xml::Reader` event loop (already the reader's tool); attribute
  **order and raw bytes are preserved** (no normalization), so an unedited
  subtree re-serializes faithfully.
- `mc:AlternateContent`, `w:sdt`, fields, `w:del`, DrawingML, OMML — all just
  ordinary preserved `Element`/`Text` nodes. No special handling → nothing is lost.
- Serialized by a `quick_xml::Writer` walking the arena, emitting raw names/attrs.
- Depth-bounded build (reuse `MAX_DEPTH`), so a hostile deeply-nested part can't
  overflow the stack.

### 3.4 `Document` integration

`DocxState` (today `{model, text, main_text}`) gains `package: Package`. `Document`
exposes the edit surface; the `.doc` backend has `package: None` (edit/save is
`.docx`-only — `save()` on a `.doc`-backed `Document` returns
`Err(Error::Unsupported)` or the lossy `to_docx` generator, explicitly).

## 4. Phase A — passthrough-save, targeted edits, template creation

### 4.1 Round-trip plumbing
- `Document::open(bytes)`: build the read model as today **and** keep
  `Package::from_zip(bytes)`.
- `Document::new()`: `Package::from_zip(include_bytes!("../assets/blank.docx"))`.
  `assets/blank.docx` = a minimal Word-valid package (document/styles/settings/
  fontTable/theme/[Content_Types]/rels), committed as a crate asset.
- `save() -> Result<Vec<u8>>`: apply pending edits to the package, then `to_zip()`.
  A no-op `open→save` re-emits retained `Raw` parts verbatim → byte-stable.

### 4.2 A body edit = regenerate `document.xml` + rels-merge
When the caller edited the body model (A overlay), the body producer
(`write/docx.rs`) renders a fresh `word/document.xml` from `DocModel`, and
`save()` reconciles relationships:

```
rewrites = { "word/document.xml" } ∪ { parts rdoc re-authored: maybe styles/numbering/headers }
preserved = all other retained parts (theme, settings, fontTable, comments,
            footnotes, customXml, docProps, media, charts, embeddings, …)

document.xml.rels (merged) =
    original rels whose Target ∉ rewrites          // keep styles/numbering/theme/comments/… by original rId
  ∪ freshly-minted rels for body refs rdoc created // hyperlinks/images, rId from alloc_rid() (> original max)
content-types (merged) =
    original defaults+overrides, with overrides for rewrites replaced/added
```

Rationale: body `pStyle`/`numId` resolve by **style-id / num-id**, not by `rId`;
the styles/numbering *parts* are linked by relationship **type**, so preserving
their original rels keeps the regenerated body wired correctly. Only body
`r:id`/`r:embed` (hyperlinks, images, header/footer refs) need rId consistency, and
those rdoc mints together from the seeded allocator → no collision with preserved
rels.

**Honest A limitation (documented, fixed by B):** parts rdoc *regenerates* lose
their unmodeled content (a regenerated `document.xml` drops `w:sdt`/fields/shapes
the model doesn't carry; if rdoc re-authors `styles.xml`, custom styles are lost).
Mitigation in A: minimize the `rewrites` set — for MVP, regenerate **only
`document.xml`** and passthrough `styles.xml`/`numbering.xml`/headers/footers; the
body's `pStyle` must reference style ids that exist in the preserved `styles.xml`
(true for Word-authored docs using `Heading1`…; a custom-id doc degrades — that's
the B case).

### 4.3 API
```rust
impl Document {
    pub fn open(bytes: &[u8]) -> Result<Document>;     // retains package (.docx)
    pub fn new() -> Document;                          // bundled template
    pub fn save(&self) -> Result<Vec<u8>>;             // fallible package serialization
    pub fn body_mut(&mut self) -> BodyEditor<'_>;      // A overlay: append/replace/remove blocks
}
pub fn write_docx(model: &DocModel) -> Vec<u8>;        // unchanged (infallible generator)
pub fn try_write_docx(model: &DocModel) -> Result<Vec<u8>>;  // new fallible variant
```

### 4.4 A acceptance (tests)
- `roundtrip_preserves_unmodeled_parts`: for N corpus `.docx`, `open→save`, assert
  the part name set is identical and every part except `document.xml` is
  byte-identical; re-open with our reader + assert it still parses.
- `body_edit_keeps_satellites`: edit a paragraph, save, assert theme/settings/
  fontTable/styles/numbering/comments parts still present and their rels intact
  (no orphan, no duplicate `rId`).
- `new_from_template_opens_in_word`: `Document::new()` + a paragraph → LibreOffice
  converts it (lo-cli) and python-docx reads it.
- Fuzz `Package::from_zip`→`to_zip` (no panic/OOM on arbitrary zip bytes).

## 5. Phase B — preserved element-tree editing

### 5.1 Lazy promotion
`part_tree_mut("word/document.xml")` parses `Part::Raw` → `Part::Xml(XmlTree)` on
first edit. `to_zip` serializes `Xml` parts, leaves `Raw` parts verbatim. So only
*edited* parts re-serialize (the byte-stability win over python-docx/POI, which
eagerly parse known parts).

### 5.2 Typed wrappers over the tree (the Rust borrow design)
python-docx mutates via Python references; Rust forbids that aliasing. We use
**handles, not references**: wrappers carry a `NodeId` into the `Document`'s arena,
and edits go through `&mut self`.

```rust
impl Document {
    pub fn paragraphs(&self) -> impl Iterator<Item = ParaRef> + '_;  // ParaRef = Copy { id: NodeId }
    pub fn run_text(&self, r: RunRef) -> String;
    pub fn set_run_text(&mut self, r: RunRef, text: &str);           // mutate w:t children of w:r
    pub fn set_run_bold(&mut self, r: RunRef, on: bool);            // get-or-add w:rPr/w:b
    pub fn insert_paragraph_after(&mut self, p: ParaRef, text: &str) -> ParaRef;
    pub fn remove(&mut self, p: ParaRef);
}
```

- Wrappers are `Copy` ids → no borrow conflicts; methods take `&self`/`&mut self`.
- Edits are element ops in `xmltree/edit.rs`: `set_attr`, `ensure_child`,
  `insert_child_at`, `remove`, `replace_children`. Unedited siblings are untouched →
  preserved.
- The lossy `DocModel` view (`Document::model()`) is rebuilt lazily from the tree
  for read/render/export; it is never written back.

### 5.3 Reference integrity on structural edits
Adding an image/hyperlink/comment is transactional via
`Package::add_related_part(src, rel_type, name, ct, bytes) -> rId`, then the tree
edit inserts the `r:id`/`r:embed` referencing that `rId`. Adding a comment also
splices `commentRangeStart/End` + `commentReference` into the body tree and creates
/extends `word/comments.xml` (a tree part) — all under one allocator.

### 5.4 B acceptance (tests)
- `edit_preserves_unmodeled_body`: a doc with a chart (`mc:AlternateContent`), a
  content control (`w:sdt`), a field, and a comment → change one run → save →
  assert all four survive and only the edited `w:t` changed (golden tree diff).
- `insert_image_reconciles_rels`: add a PNG → new `media/*` part + content-type +
  rels with a non-colliding `rId`; re-opens in Word/LibreOffice.
- `lazy_parse_byte_stable`: editing `document.xml` leaves every other part
  byte-identical.
- Fuzz `XmlTree::parse`→`serialize` (round-trip no-panic; structural cap holds).

## 6. Risks & mitigations
- **rId reconciliation** — single seeded allocator; A merges, B passes through;
  rels-integrity assertions + fuzz.
- **Re-serialization churn** — only edited parts re-serialize (lazy `Raw`→`Xml`);
  untouched parts byte-stable.
- **`[Content_Types]` correctness** — carry original defaults+overrides; on
  `set_part`, add/replace the override (or rely on a Default extension); on
  `remove_part`, drop its override; assert every part is typed before `to_zip`.
- **Per-part rels + external targets** — `RelsGraph` keyed by rels-part path;
  preserve `TargetMode="External"`; never resolve a foreign `r:id` against the
  wrong part's rels.
- **Borrow ergonomics** — arena + `Copy` handles (no `Rc<RefCell>`, no lifetimes in
  the public wrappers).
- **`.doc` path** — untouched; `Document` from `.doc` has no package; `save()` is
  `.docx`-only and says so.
- **Backward compat** — `write_docx(&DocModel)`/`render_pdf`/reader APIs unchanged;
  the generator builds an empty `Package` and `set_part`s, so existing tests pass.

## 7. Rollout (PR sequence)
1. `src/opc` module: `Package::from_zip`/`to_zip`, `ContentTypes`, `RelsGraph`,
   `RIdAllocator` + fuzz + round-trip tests. (`write/opc.rs` → shim.)
2. `Document::open` retains the package; `Document::new()` + `assets/blank.docx`;
   `save()`/`try_write_docx` (passthrough, no body edit yet) — ships **A’s
   no-op fidelity** gate.
3. A body overlay (`body_mut`) + rels-merge regen of `document.xml`.
4. `src/xmltree` parse/serialize + fuzz (no edits yet) — proves faithful round-trip.
5. B wrappers + lazy promotion + structural edits + reference integrity.
6. Docs + write/edit validation.

Each PR: gate green (`fmt`, `clippy -D --all-features`, `test`, `doc`) and a
corpus round-trip check; accurate CHANGELOG/README updates.
