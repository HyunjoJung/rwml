//! `rdoc` — one native Rust reader for **both** Microsoft Word formats: legacy
//! `.doc` (Word 97–2003 binary, [MS-DOC]) and modern `.docx` (OOXML
//! WordprocessingML). No JVM, no Apache POI, no external `.docx` crate, no
//! shelling out — [`Document::open`] format-detects from the magic bytes and
//! both feed the **same** [`DocModel`] and Markdown/HTML exporters.
//!
//! * **`.doc`** is an OLE2/CFB compound file. The text lives in the
//!   `WordDocument` stream; the **piece table** (CLX) in the `0Table`/`1Table`
//!   stream maps character positions to byte offsets, and each piece is either
//!   UTF-16LE (Korean body text) or 8-bit text in the document's ANSI codepage
//!   (`fCompressed` — cp1252 for Western, cp949 for Korean, from the FIB language
//!   id).
//! * **`.docx`** is a ZIP of XML parts (`word/document.xml` + styles, numbering,
//!   relationships, media), parsed with `zip` + `quick-xml` behind the default
//!   `docx` feature. Disable it (`default-features = false`) for a
//!   dependency-light `.doc`-only build.
//!
//! ```no_run
//! // Works for either format — detection is automatic.
//! let bytes = std::fs::read("report.doc").unwrap();
//! let text = rdoc::extract_text(&bytes).unwrap();
//! println!("{text}");
//! ```
//!
//! Two surfaces:
//!
//! * **Flat text** — [`extract_text`] / [`Document::text`], the same output as
//!   POI `WordExtractor.getText()` (fast, allocation-light).
//! * **A full document model** — [`Document::model`] (paragraphs, character runs
//!   with bold/italic/…, structured tables with colspan/rowspan, headings,
//!   lists, hyperlinks, and extracted images), plus [`Document::to_markdown`]
//!   and [`Document::to_html`]. Built lazily, so the flat path never pays for it.
//!
//! Parsing **untrusted input** is panic-free / bounds-checked: a malformed or hostile
//! `.doc`/`.docx` yields [`Error`], never a crash. (The only `expect` is on the crate's
//! own compiled-in blank template behind the infallible [`Document::new`]/[`Default`];
//! use [`Document::try_new`] for a `Result` instead of that build-invariant panic.)

#![forbid(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations, rust_2018_idioms)]

mod assemble;
mod chpx;
mod clx;
#[cfg(feature = "docx")]
mod docx;
mod error;
mod export;
mod ffn;
mod fib;
mod image;
mod list;
mod model;
mod numfmt;
mod ole;
#[cfg(feature = "docx")]
mod opc;
mod papx;
#[cfg(feature = "render")]
mod render;
mod stsh;
mod table;
mod text;
mod util;
#[cfg(feature = "docx")]
mod write;
#[cfg(feature = "docx")]
mod xmltree;

pub use error::{Error, Result};
pub use model::{
    Align, Block, Cell, CharProps, Color, DocMeta, DocModel, DocSetup, FieldRole, Image, Indent,
    ListInfo, PageSetup, ParaProps, Paragraph, Row, Run, Spacing, Stats, Table, VCell, VertAlign,
};

use fib::Fib;

/// Convenience: decode `.doc` bytes into normalized plain text (all
/// sub-documents — main body, then footnotes/endnotes/headers). Errors with
/// [`Error::NoText`] if nothing indexable was found.
pub fn extract_text(bytes: &[u8]) -> Result<String> {
    let doc = Document::open(bytes)?;
    let t = doc.text();
    if text::has_indexable(&t) {
        Ok(t)
    } else {
        Err(Error::NoText)
    }
}

/// Serialize a [`DocModel`] — one you built from data, or read from a `.doc`/
/// `.docx` — to a clean, Office-openable **`.docx`** byte buffer. This is the
/// authoring entry point: construct a `DocModel` (paragraphs/runs with fonts,
/// sizes, colors; headings; styled/sized/shaded tables; images; page setup) and
/// write a styled Word document. Available with the default `docx` feature.
///
/// **Image bytes are trusted as-is:** an embedded [`Image`]'s `bytes` are written
/// verbatim under a part typed from its `mime` — the writer does not transcode or
/// validate the raster, so the caller must ensure `bytes` really are that format (a
/// mismatch produces a part Word can't render). The element-tree editor's
/// [`Document::add_image_png`] does validate, since it accepts arbitrary caller input.
#[cfg(feature = "docx")]
pub fn write_docx(model: &DocModel) -> Vec<u8> {
    write::to_docx(model)
}

/// Fallible variant of [`write_docx`]: returns the serialization error instead of
/// an empty buffer if packaging ever fails. Prefer this when you need to surface
/// write failures rather than silently emit nothing.
#[cfg(feature = "docx")]
pub fn try_write_docx(model: &DocModel) -> Result<Vec<u8>> {
    write::try_to_docx(model)
}

/// Render a [`DocModel`] — one you built from data, or read from a `.doc`/`.docx`
/// — to a single-column A4 **PDF** with native typesetting (`parley` + `krilla`).
/// The rendering entry point for previews and generated reports: rich text
/// (color/size/font), lists, indentation, bordered tables with shaded cells, and
/// images. Available with the `render` feature.
#[cfg(feature = "render")]
pub fn render_pdf(model: &DocModel) -> Vec<u8> {
    render::to_pdf(model)
}

/// Render a [`DocModel`] to PDF after registering caller-supplied fonts (e.g. a
/// bundled Korean face via `include_bytes!`). Use this in headless/server
/// environments that lack system CJK fonts: each blob is added to the layout font
/// collection, made available by its family name and used for script fallback.
/// Available with the `render` feature.
#[cfg(feature = "render")]
pub fn render_pdf_with_fonts(model: &DocModel, fonts: &[Vec<u8>]) -> Vec<u8> {
    render::to_pdf_with_fonts(model, fonts)
}

/// A parsed Word document — either legacy `.doc` (OLE2/[MS-DOC]) or modern
/// `.docx` (OOXML). [`Document::open`] format-detects from the magic bytes and
/// both backends feed the **same** [`DocModel`] and exporters, so `text()`,
/// `to_markdown()`, `to_html()`, and `images()` behave identically regardless of
/// which Word format the bytes are in.
pub struct Document {
    backend: Backend,
}

/// The format-specific state behind a [`Document`]. Boxed so the enum isn't
/// dominated by the larger `.doc` variant.
enum Backend {
    Doc(Box<DocState>),
    #[cfg(feature = "docx")]
    Docx(Box<docx::DocxState>),
}

/// Legacy `.doc` state: the decoded character stream plus the FIB and the
/// retained structures for the lazy rich-model build.
struct DocState {
    /// CP-aligned char stream (all sub-docs, control marks embedded) — sliced by
    /// the sub-document accessors, which work in Word's CP space.
    raw: String,
    /// Full render with reconstructed list autonumbers (used by `text()`).
    labeled: String,
    fib: Fib,
    // Retained for the lazy rich-model build ([`Document::model`]); none of this
    // is touched by the fast `text()` path.
    word: Vec<u8>,
    pieces: Vec<clx::Piece>,
    papx: papx::PapxTable,
    chpx: chpx::ChpxTable,
    stylesheet: stsh::StyleSheet,
    lists: list::Lists,
    /// Font-name table (`SttbfFfn`), for resolving CHPX font indices to names.
    fonts: Vec<String>,
    /// The `Data` stream bytes (inline pictures), empty if absent.
    data: Vec<u8>,
    enc: &'static encoding_rs::Encoding,
}

impl Document {
    /// Open and decode a Word document from its raw bytes, detecting the format:
    /// the OLE2/CFB magic (`D0CF11E0`) routes to the legacy `.doc` parser, the
    /// ZIP magic (`PK\x03\x04`) to the `.docx` parser (when the `docx` feature is
    /// enabled). Neither ⇒ [`Error::NotOle2`].
    pub fn open(bytes: &[u8]) -> Result<Self> {
        if ole::is_ole2(bytes) {
            return Ok(Document {
                backend: Backend::Doc(Box::new(DocState::open(bytes)?)),
            });
        }
        #[cfg(feature = "docx")]
        if docx::is_zip(bytes) {
            return Ok(Document {
                backend: Backend::Docx(Box::new(docx::open(bytes)?)),
            });
        }
        #[cfg(not(feature = "docx"))]
        if bytes.starts_with(b"PK\x03\x04") {
            return Err(Error::Docx(
                "`.docx` support not compiled in (enable the `docx` cargo feature)".into(),
            ));
        }
        Err(Error::NotOle2)
    }

    /// Create a new, empty `.docx`-backed document from the bundled blank template
    /// (one empty paragraph, default page setup) — mirroring how python-docx's
    /// `Document()` opens its `default.docx`. The returned document carries a full,
    /// valid OPC package, so [`Document::save`] produces an Office-openable file.
    ///
    /// Panics only if the crate's own bundled template is corrupt (a build-time
    /// invariant covered by tests); use [`Document::try_new`] for a non-panicking
    /// variant. Available with the default `docx` feature.
    #[cfg(feature = "docx")]
    pub fn new() -> Self {
        Document {
            backend: Backend::Docx(Box::new(docx::blank())),
        }
    }

    /// Fallible [`Document::new`]: returns an error instead of panicking if the
    /// bundled blank template can't be opened. Available with the default `docx`
    /// feature.
    #[cfg(feature = "docx")]
    pub fn try_new() -> Result<Self> {
        Ok(Document {
            backend: Backend::Docx(Box::new(docx::try_blank()?)),
        })
    }

    /// Build the rich document model — paragraphs, character runs (bold/italic/
    /// …), structured tables, lists, and fields. For `.doc` this is built lazily
    /// (the flat [`Document::text`] path never pays for it); for `.docx` the model
    /// is built eagerly at open and cloned here.
    ///
    /// **Stale after an in-place edit.** This (and everything derived from it —
    /// [`Document::to_markdown`], [`Document::to_html`], [`Document::images`],
    /// [`Document::to_docx`], [`Document::to_pdf`]) reflects the document **as opened**.
    /// Element-tree edits ([`Document::replace_body_text`] / [`Document::add_image_png`])
    /// mutate the package's `document.xml` directly, not this model, so they are not
    /// visible here until you [`Document::save`] and re-[`Document::open`] the result.
    pub fn model(&self) -> DocModel {
        match &self.backend {
            Backend::Doc(d) => {
                let mut numberer = list::Numberer::new(&d.lists);
                assemble::build_model(
                    &d.word,
                    &d.pieces,
                    d.enc,
                    &d.papx,
                    &d.chpx,
                    &d.stylesheet,
                    &d.data,
                    &d.fonts,
                    &mut numberer,
                    &d.fib,
                )
            }
            #[cfg(feature = "docx")]
            Backend::Docx(d) => {
                // The stored model is body-only; re-append footnote/endnote blocks for
                // the read/render view (they are kept separate because their parts are
                // preserved verbatim on save, never inlined into document.xml).
                let mut m = d.model.clone();
                m.blocks.extend(d.notes.iter().cloned());
                m
            }
        }
    }

    /// Render the document as GitHub-Flavored **Markdown** (headings, bold/italic,
    /// lists, hyperlinks, and tables).
    pub fn to_markdown(&self) -> String {
        export::markdown::render(&self.model())
    }

    /// Render the document as semantic **HTML** (`<h1>`–`<h6>`, `<strong>`,
    /// `<table>` with `colspan`/`rowspan`, nested `<ol>`/`<ul>`, `<a href>`).
    pub fn to_html(&self) -> String {
        export::html::render(&self.model())
    }

    /// Extract every embedded raster image (PNG/JPEG/GIF) with its bytes, in
    /// reading order — the equivalent of POI's `PicturesTable.getAllPictures()`.
    pub fn images(&self) -> Vec<Image> {
        fn walk(blocks: &[Block], out: &mut Vec<Image>) {
            for b in blocks {
                match b {
                    Block::Paragraph(p) => {
                        for r in &p.runs {
                            if let Some(img) = &r.image {
                                if img.bytes.is_some() {
                                    out.push(img.clone());
                                }
                            }
                        }
                    }
                    Block::Image(img) if img.bytes.is_some() => out.push(img.clone()),
                    Block::Table(t) => {
                        for row in &t.rows {
                            for c in &row.cells {
                                walk(&c.blocks, out);
                            }
                        }
                    }
                    Block::Image(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.model().blocks, &mut out);
        out
    }

    /// Serialize this document to a modern **`.docx`** (OOXML WordprocessingML)
    /// byte buffer — the inverse of the reader. `read → DocModel → write → read`
    /// preserves the structure (text, character runs, headings, alignment, lists,
    /// tables with colspan/rowspan, images, hyperlinks), so a legacy `.doc` can be
    /// converted to a clean, Office-openable `.docx` through the shared model.
    /// Available with the default `docx` feature.
    #[cfg(feature = "docx")]
    pub fn to_docx(&self) -> Vec<u8> {
        write::to_docx(&self.model())
    }

    /// **Package-preserving save** — re-emit this document's `.docx` with every part
    /// it doesn't model preserved verbatim (themes, settings, fonts, comments,
    /// custom XML, charts, embeddings, unknown parts). A no-op `open → save` is
    /// byte-stable per part. Element-tree edits ([`Document::replace_body_text`] /
    /// [`Document::add_image_png`]) mutate `word/document.xml` in place, so untouched
    /// **non-metadata** parts stay byte-for-byte; `[Content_Types].xml` is rewritten
    /// only when the edit must *repair* the document's content typing (e.g. the source
    /// lacked or mistyped the `word/document.xml` override) so the output stays
    /// Word-openable. This is distinct
    /// from [`Document::to_docx`], which regenerates a fresh package from the lossy
    /// model (use that to *convert* a `.doc`). `save()` requires a `.docx`-backed
    /// document (one from [`Document::open`] on a `.docx`, or [`Document::new`]); a
    /// `.doc`-backed document has no package to preserve and returns an error pointing
    /// to [`Document::to_docx`]. Available with the default `docx` feature.
    #[cfg(feature = "docx")]
    pub fn save(&self) -> Result<Vec<u8>> {
        match &self.backend {
            // Refuse to save a package that wasn't fully retained on open: if `from_zip`
            // had to skip an unreadable/corrupt entry, `save()` would silently drop that
            // part, breaking the preservation guarantee. (Reading still works — use
            // `to_docx()` to emit a fresh package from the model instead.)
            Backend::Docx(d) if !d.package.is_complete() => Err(Error::Docx(
                "save() cannot preserve this package: it was opened with one or more \
                 unreadable/corrupt parts that were not retained — re-acquire the source \
                 file, or use to_docx() to emit a fresh package from the model"
                    .into(),
            )),
            // The retained package already holds every part (and any element-tree edit
            // is already applied to its `document.xml`); serializing it preserves them.
            Backend::Docx(d) => d.package.to_zip(),
            Backend::Doc(_) => Err(Error::Docx(
                "save() preserves an opened .docx package; this document was opened \
                 from a legacy .doc — use to_docx() to convert it"
                    .into(),
            )),
        }
    }

    /// **Element-tree editing: replace body text in place.** Finds
    /// every text run (`w:t`) whose text equals `old` and rewrites it to `new`,
    /// editing the live `word/document.xml` element tree — so **everything else is
    /// preserved**, including content the model can't represent (fields, content
    /// controls, shapes, comments, tracked changes). Returns how many runs changed.
    ///
    /// This promotes `document.xml` to an editable tree; [`Document::save`] then
    /// re-serializes only that part (every other part stays byte-for-byte). Requires
    /// a `.docx`-backed document. Note: this edits the package directly, not the
    /// `model()`/`text()` views, which become stale until reopened. On any error the
    /// document is left untouched (the edit is transactional). Available with the
    /// default `docx` feature.
    #[cfg(feature = "docx")]
    pub fn replace_body_text(&mut self, old: &str, new: &str) -> Result<usize> {
        // Backend/editability check FIRST (so a `.doc` or un-editable package gets the
        // documented error, not a misleading `Ok`), then short-circuit a same-value
        // no-op so we don't promote/canonicalize `document.xml` for no actual change.
        let d = self.docx_tree_editable()?;
        if old == new {
            return Ok(0);
        }
        // Preflight on a throwaway parse (no promotion): confirm a body exists and
        // count matches. WordprocessingML `w:t` only (namespace-resolved), so
        // DrawingML `a:t`/default-ns `<t>` inside shapes is left alone while genuine
        // text-box `w:t` is still editable.
        let raw = d
            .package
            .part("word/document.xml")
            .ok_or_else(|| Error::Docx("missing word/document.xml".into()))?;
        let probe = xmltree::XmlTree::parse(&raw)?;
        // Strict: a multi-root / non-`w:document` `document.xml` is malformed → passthrough-only.
        let probe_body = probe.wml_body_strict()?;
        // Anchored to the body, so a stray `w:t` sibling of `w:body` is never edited.
        let matched: Vec<_> = probe
            .wml_text_runs_under(probe_body)
            .into_iter()
            .filter(|&id| probe.text_of(id) == old)
            .collect();
        if matched.is_empty() {
            // Nothing to do: don't promote/canonicalize the part or change edit mode.
            return Ok(0);
        }
        // Each match without a reusable text carrier (e.g. an empty `<w:t/>`) allocates a
        // new node; preflight that against the node budget so the commit can't exceed it.
        // Count against the LIVE arena (which includes any detached nodes a prior edit
        // left) — the throwaway `probe` re-parses the serialized form and would undercount.
        let new_nodes = matched
            .iter()
            .filter(|&&id| !probe.has_text_carrier(id))
            .count();
        let live_count = d
            .package
            .part_tree_ref("word/document.xml")
            .map_or(probe.node_count(), |t| t.node_count());
        if live_count.saturating_add(new_nodes) > xmltree::node_budget() {
            return Err(Error::Docx(
                "replace_body_text: edit would exceed the node budget".into(),
            ));
        }
        // Attribute-budget preflight: if `new` carries significant edge whitespace,
        // `set_element_text` will add `xml:space="preserve"`. Reject up front (so the
        // commit stays transactional — never edits some runs then fails) any matched run
        // that is already at the attribute cap and lacks that attribute.
        let needs_space = new != new.trim_matches([' ', '\t', '\n', '\r']);
        if needs_space
            && matched
                .iter()
                .any(|&id| !probe.can_set_attr(id, b"xml:space"))
        {
            return Err(Error::Docx(
                "replace_body_text: edit would exceed an element's attribute budget".into(),
            ));
        }
        // Commit on a CLONE, swapped in only after the whole edit succeeds. The budgets are
        // preflighted, but `set_element_text` is fallible (a no-carrier run allocates via
        // `try_reserve`), so a mid-loop out-of-memory could otherwise leave SOME runs
        // rewritten and others not. Building on a clone keeps the edit all-or-nothing.
        let mut pkg = d.package.clone();
        let tree = pkg.part_tree_mut("word/document.xml")?;
        let body = tree.wml_body_strict()?;
        let mut changed = 0;
        for id in tree.wml_text_runs_under(body) {
            if tree.text_of(id) == old {
                // Preflighted above (node budget + attribute budget), so this only ever
                // surfaces a genuine out-of-memory condition rather than a logic failure.
                tree.set_element_text(id, new)?;
                changed += 1;
            }
        }
        // We've edited (touched) document.xml — guarantee the saved package types it as
        // the WML main document, so `save()` can't fail on a missing/generic override.
        pkg.ensure_content_type("word/document.xml", CT_DOCUMENT_MAIN);
        d.package = pkg;
        Ok(changed)
    }

    /// **Element-tree editing: append an inline PNG image** to the
    /// body, reconciling relationships transactionally — the media part, its
    /// `image/png` content-type, and a fresh non-colliding `rId` are added together,
    /// then a drawing paragraph referencing that `rId` (with a unique drawing id) is
    /// spliced into `w:body` **before** the final `w:sectPr`. Every existing
    /// part/relationship is preserved.
    ///
    /// `name` must be a plain `*.png` file name (no path separators or `..`) that does
    /// not already exist under `word/media/`. `png` is checked to be a structurally
    /// well-formed **PNG container** (signature/framing/CRCs/IHDR/ordering/zlib header,
    /// *not* a full image decode) so the declared `image/png` content type is honest.
    /// **Transactional:** all preconditions (name validity, PNG
    /// container validity, part not present, `w:body` exists, node budget) are checked
    /// before any mutation, so on error the document is unchanged. Like
    /// [`Document::replace_body_text`], this edits the package directly, so the
    /// `model()`/`images()`/`text()` read views are stale until the saved bytes are
    /// reopened. Available with the default `docx` feature.
    #[cfg(feature = "docx")]
    pub fn add_image_png(&mut self, png: &[u8], name: &str) -> Result<()> {
        const REL_IMAGE: &str =
            "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
        // Restrict to a conservative, URI-safe segment so the name can be written
        // verbatim into a relationship target without OPC pack-URI escaping issues:
        // `[A-Za-z0-9._-]+` ending in `.png`, no `..`.
        let stem_ok = !name.is_empty()
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            && !name.contains("..")
            && name.to_ascii_lowercase().ends_with(".png");
        if !stem_ok {
            return Err(Error::Docx(format!(
                "invalid image name {name:?}: expected a plain [A-Za-z0-9._-]+.png file name"
            )));
        }
        // Reject an oversized image FIRST — a cheap length check before the linear
        // PNG/CRC scan (so a hostile caller can't force a CRC walk over a huge slice),
        // matching the per-part budget `save()` enforces.
        if png.len() as u64 > opc::max_part() {
            return Err(Error::Docx(
                "add_image_png: image exceeds the per-part size budget".into(),
            ));
        }
        // Then validate the bytes are a structurally well-formed PNG container, so the
        // declared `image/png` content type isn't a lie.
        if !is_png(png) {
            return Err(Error::Docx(
                "add_image_png: bytes are not a structurally-valid PNG container".into(),
            ));
        }
        let d = self.docx_tree_editable()?;
        let part = format!("word/media/{name}");
        // Enforce the same part-name length limit the open path enforces, so the edit
        // can't produce a package `Document::open` would later reject.
        if part.len() > opc::MAX_NAME_LEN {
            return Err(Error::Docx(
                "add_image_png: image part name too long".into(),
            ));
        }
        if d.package.has_part(&part) {
            return Err(Error::Docx(format!("media part {part} already exists")));
        }
        let (cx, cy) = png_extent_emu(png);
        // Preflight WITHOUT promoting: read the live tree if `document.xml` is already
        // promoted (a prior edit) so the node count includes any detached nodes; else
        // parse a throwaway copy (a still-`Raw` part was never edited, so a fresh parse
        // has the same count). This keeps the budget accurate AND leaves a still-`Raw`
        // part untouched on failure (no canonicalizing promotion) — fully transactional.
        // `wml_body_strict` rejects a multi-root / non-`w:document` part before any mutation
        // (transactional preflight), so a malformed `document.xml` stays passthrough-only.
        let (draw_id, live_count) = match d.package.part_tree_ref("word/document.xml") {
            Some(t) => {
                t.wml_body_strict()?;
                (t.fresh_drawing_id(), t.node_count())
            }
            None => {
                let raw = d
                    .package
                    .part("word/document.xml")
                    .ok_or_else(|| Error::Docx("missing word/document.xml".into()))?;
                let t = xmltree::XmlTree::parse(&raw)?;
                t.wml_body_strict()?;
                (t.fresh_drawing_id(), t.node_count())
            }
        };
        // The fragment's node count is independent of the (not-yet-allocated) rId, so a
        // placeholder is fine for the budget check.
        let frag_probe = image_paragraph_xml("rIdPENDING", cx, cy, draw_id);
        let frag_nodes = xmltree::XmlTree::parse(frag_probe.as_bytes())?.node_count();
        if live_count.saturating_add(frag_nodes) > xmltree::node_budget() {
            return Err(Error::Docx(
                "add_image_png: edit would exceed the node budget".into(),
            ));
        }

        // Commit on a CLONE, swapped in only after every step succeeds. The budget is
        // preflighted, but the underlying tree edits are now fallible (`XmlTree::push` uses
        // `try_reserve`), so a genuine out-of-memory after `add_related_part` could
        // otherwise leave an orphaned media part + relationship. Building on a clone keeps
        // the documented guarantee literally true: on ANY error the document is unchanged.
        let mut pkg = d.package.clone();
        let rid = pkg.add_related_part(
            "word/document.xml",
            REL_IMAGE,
            &part,
            Some("image/png"),
            png.to_vec(),
        );
        let frag = image_paragraph_xml(&rid, cx, cy, draw_id);
        let tree = pkg.part_tree_mut("word/document.xml")?;
        let body = tree.wml_body_strict()?;
        tree.insert_fragment_before_ns_local(body, frag.as_bytes(), xmltree::WML_NS, b"sectPr")?;
        // Guarantee the edited document.xml is typed as the WML main document on save.
        pkg.ensure_content_type("word/document.xml", CT_DOCUMENT_MAIN);
        d.package = pkg;
        Ok(())
    }

    /// Mutable `.docx` state for element-tree editing, refusing a `.doc` backend (no
    /// package to edit) or a package whose OPC metadata parsed lossily (editing would
    /// regenerate `[Content_Types].xml`/`.rels` from an incomplete view — the document
    /// still opens and round-trips raw, just can't be safely edited).
    #[cfg(feature = "docx")]
    fn docx_tree_editable(&mut self) -> Result<&mut docx::DocxState> {
        match &mut self.backend {
            // An incomplete package (an unreadable entry was dropped on open) can't be
            // package-preserving-saved, so refuse edits up front rather than letting an
            // edit "succeed" and then `save()` fail — editable ⇔ saveable.
            Backend::Docx(d) if !d.package.is_complete() => Err(Error::Docx(
                "cannot edit: this document was opened with unreadable/dropped parts, so a \
                 package-preserving save is impossible — re-acquire the source file"
                    .into(),
            )),
            Backend::Docx(d) if d.package.is_meta_lossy() => Err(Error::Docx(
                "cannot edit: this document's OPC metadata ([Content_Types].xml or a \
                 .rels part) is malformed, so an edit would regenerate it lossily — \
                 re-acquire the source file"
                    .into(),
            )),
            Backend::Docx(d) => Ok(d),
            Backend::Doc(_) => Err(Error::Docx(
                "element-tree editing requires a .docx-backed document".into(),
            )),
        }
    }

    /// Test-only: the live `word/document.xml` arena node count (promotes the part),
    /// including any detached nodes a prior edit left — used to set a precise node
    /// budget for transactional-rollback tests.
    #[cfg(all(test, feature = "docx"))]
    fn docx_node_count(&mut self) -> usize {
        match &mut self.backend {
            Backend::Docx(d) => d
                .package
                .part_tree_mut("word/document.xml")
                .map(|t| t.node_count())
                .unwrap_or(0),
            Backend::Doc(_) => 0,
        }
    }

    /// Render this document to a single-column A4 **PDF** with native typesetting
    /// — `parley` lays out and shapes the text (Korean/CJK line-breaking and font
    /// fallback included) and `krilla` emits the PDF with subsetted embedded fonts
    /// and selectable text. Tables render as a real bordered grid with rich,
    /// shaded, vertically-aligned cells; paragraphs honor color/size/font, lists,
    /// and indentation; images are drawn. Available with the `render` feature
    /// (which raises the MSRV to 1.88).
    #[cfg(feature = "render")]
    pub fn to_pdf(&self) -> Vec<u8> {
        render::to_pdf(&self.model())
    }

    /// Normalized plain text of the entire document (all sub-documents), with
    /// reconstructed list autonumbers (`.doc`) or model-derived text (`.docx`).
    pub fn text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => text::finalize(&d.labeled),
            #[cfg(feature = "docx")]
            Backend::Docx(d) => d.text.clone(),
        }
    }

    /// Normalized text of just the main document body. For `.doc` this is the
    /// first `ccpText` characters (excluding footnotes/headers); for `.docx` it is
    /// the body part, excluding the running headers/footers that [`Document::text`]
    /// also includes.
    pub fn main_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => text::finalize(&d.region(0, d.fib.ccp_text as usize)),
            #[cfg(feature = "docx")]
            Backend::Docx(d) => d.main_text.clone(),
        }
    }

    /// Normalized footnote + endnote text (`.doc` only; empty for `.docx`, whose
    /// notes live in separate parts not yet parsed).
    pub fn footnote_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => {
                // ccp_* are unbounded attacker-controlled FIB u32s — add in usize with
                // saturating math so a crafted .doc can't overflow (panic) / wrap here.
                let start = d.fib.ccp_text as usize;
                let len = (d.fib.ccp_ftn as usize).saturating_add(d.fib.ccp_edn as usize);
                text::finalize(&d.region(start, len))
            }
            #[cfg(feature = "docx")]
            Backend::Docx(_) => String::new(),
        }
    }

    /// Normalized header/footer text. `.doc` slices the `ccpHdd` region; `.docx`
    /// flattens the running header/footer parts resolved from the section refs.
    pub fn header_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => {
                // Saturating usize sum: ccp_text/ccp_ftn are unbounded FIB u32s (a plain
                // u32 + u32 panics on overflow-checks / wraps for a crafted .doc).
                let start = (d.fib.ccp_text as usize).saturating_add(d.fib.ccp_ftn as usize);
                text::finalize(&d.region(start, d.fib.ccp_hdd as usize))
            }
            #[cfg(feature = "docx")]
            Backend::Docx(d) => crate::docx::header_footer_text(&d.model),
        }
    }

    /// Total character count: the FIB CP space across all sub-documents (`.doc`)
    /// or the model's visible character count (`.docx`).
    pub fn char_count(&self) -> usize {
        match &self.backend {
            Backend::Doc(d) => d.fib.total_cp(),
            #[cfg(feature = "docx")]
            Backend::Docx(d) => d.model.meta.stats.text_chars,
        }
    }

    /// `true` if a `.doc` is "complex" (fast-saved). Always `false` for `.docx`.
    pub fn is_complex(&self) -> bool {
        match &self.backend {
            Backend::Doc(d) => d.fib.complex,
            #[cfg(feature = "docx")]
            Backend::Docx(_) => false,
        }
    }
}

#[cfg(feature = "docx")]
impl Default for Document {
    /// Equivalent to [`Document::new`] — a blank `.docx`-backed document.
    fn default() -> Self {
        Self::new()
    }
}

/// The WordprocessingML main-document content type — what `word/document.xml` must be
/// typed as for Word to open the package. An element-tree edit ensures this override.
#[cfg(feature = "docx")]
const CT_DOCUMENT_MAIN: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

/// CRC-32 (ISO-HDLC / the variant PNG uses) of `data`, computed bitwise so no lookup
/// table or dependency is needed. Used to verify each PNG chunk's integrity.
#[cfg(feature = "docx")]
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xEDB8_8320 & (!(crc & 1)).wrapping_add(1));
        }
    }
    !crc
}

/// PNG validation by a bounded chunk walk. It enforces: the 8-byte signature; a single
/// leading `IHDR` (length 13, non-zero width/height ≤ 2²⁴, a legal `color_type`/
/// `bit_depth` pair, compression/filter = 0, interlace ≤ 1); well-formed chunk framing
/// (each `length(4) + type(4) + data + crc(4)`, no overrun/truncation) with a **correct
/// CRC-32** on every chunk; correct chunk ordering (`PLTE` required for indexed colour /
/// forbidden for greyscale, before any `IDAT`; `IDAT` chunks consecutive); **non-empty**
/// `IDAT` data carrying a well-formed **zlib header** (deflate method, valid `FCHECK`);
/// and a terminating `IEND` (length 0) with no trailing bytes. A forged or corrupt
/// payload — bad framing, wrong CRC, impossible header fields, misordered/empty/
/// non-zlib image data — is rejected, so the declared `image/png` is a structurally
/// well-formed **PNG container**.
///
/// This is a structural/container check, **not a full image decode**: the IDAT zlib
/// stream is header-validated but not inflated, so a container whose compressed body is
/// itself corrupt can still pass here and fail in a strict PNG decoder. (Full decode is
/// intentionally out of scope to avoid a decompressor dependency on this path.)
/// Panic-free and linear: every iteration advances a full chunk via checked math.
#[cfg(feature = "docx")]
fn is_png(bytes: &[u8]) -> bool {
    const SIG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.len() < 8 || bytes[..8] != SIG {
        return false;
    }
    // PNG declares dimensions up to 2^31-1, but cap at a sane bound so a hostile header
    // can't claim an absurd size (and downstream EMU math stays comfortable).
    const MAX_DIM: u32 = 1 << 24;
    let mut i = 8usize;
    let mut color_type = 0u8;
    let mut idat_bytes = 0usize;
    let mut zlib_hdr = [0u8; 2]; // first two bytes of the concatenated IDAT stream
    let mut zlib_have = 0usize;
    let (mut seen_ihdr, mut seen_plte, mut seen_idat, mut idat_done) = (false, false, false, false);
    while i + 8 <= bytes.len() {
        let len = u32::from_be_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
        let typ = &bytes[i + 4..i + 8];
        // A PNG chunk type is four ASCII letters; anything else is not a real chunk.
        if !typ.iter().all(u8::is_ascii_alphabetic) {
            return false;
        }
        // Full chunk end = 4 (len) + 4 (type) + len (data) + 4 (crc), checked.
        let end = match i.checked_add(12).and_then(|x| x.checked_add(len)) {
            Some(e) if e <= bytes.len() => e,
            _ => return false,
        };
        // Verify the chunk CRC over type + data (stored in the final 4 bytes).
        let stored = u32::from_be_bytes([
            bytes[end - 4],
            bytes[end - 3],
            bytes[end - 2],
            bytes[end - 1],
        ]);
        if crc32(&bytes[i + 4..end - 4]) != stored {
            return false;
        }
        let data = &bytes[i + 8..end - 4];
        if !seen_ihdr {
            // The first chunk must be a 13-byte IHDR with non-zero, bounded dimensions
            // and valid header fields (an impossible color-type/bit-depth combo means the
            // bytes are not a real image even if every CRC checks out).
            if typ != b"IHDR" || len != 13 {
                return false;
            }
            let w = u32::from_be_bytes([data[0], data[1], data[2], data[3]]);
            let h = u32::from_be_bytes([data[4], data[5], data[6], data[7]]);
            let (bit_depth, ct) = (data[8], data[9]);
            let (compression, filter, interlace) = (data[10], data[11], data[12]);
            color_type = ct;
            // PNG spec: compression/filter methods are 0, interlace is 0 or 1, and only
            // these (color_type, bit_depth) pairs are legal.
            let depth_ok = match ct {
                0 => matches!(bit_depth, 1 | 2 | 4 | 8 | 16), // grayscale
                3 => matches!(bit_depth, 1 | 2 | 4 | 8),      // indexed
                2 | 4 | 6 => matches!(bit_depth, 8 | 16),     // truecolor / +alpha
                _ => false,
            };
            if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM || !depth_ok {
                return false;
            }
            if compression != 0 || filter != 0 || interlace > 1 {
                return false;
            }
            seen_ihdr = true;
        } else if typ == b"IHDR" {
            return false; // duplicate IHDR
        } else if typ == b"PLTE" {
            // A palette is required for indexed images and forbidden for grayscale; it
            // must appear after IHDR and before any IDAT, and its length must be a whole
            // number of 1..=256 RGB triples.
            let entries = len / 3;
            if seen_plte
                || seen_idat
                || matches!(color_type, 0 | 4)
                || len % 3 != 0
                || !(1..=256).contains(&entries)
            {
                return false;
            }
            seen_plte = true;
        } else if typ == b"IDAT" {
            if idat_done {
                return false; // IDAT chunks must be consecutive
            }
            // Capture the leading zlib header bytes across the (possibly split) stream.
            for &b in data {
                if zlib_have < 2 {
                    zlib_hdr[zlib_have] = b;
                    zlib_have += 1;
                }
            }
            seen_idat = true;
            idat_bytes += len;
        } else if typ == b"IEND" {
            // IEND ends the file: empty, last (no trailing bytes), with a palette present
            // iff indexed, non-empty IDAT, and a well-formed zlib header on that stream.
            let zlib_ok = zlib_have == 2
                && (zlib_hdr[0] & 0x0F) == 8                       // deflate method
                && (zlib_hdr[0] >> 4) <= 7                          // window size
                && (zlib_hdr[1] & 0x20) == 0                        // no preset dictionary (FDICT)
                && (u16::from(zlib_hdr[0]) * 256 + u16::from(zlib_hdr[1])) % 31 == 0;
            let palette_ok = (color_type != 3) || seen_plte;
            return len == 0 && end == bytes.len() && idat_bytes > 0 && zlib_ok && palette_ok;
        } else if seen_idat {
            idat_done = true; // a non-IDAT chunk after IDAT closes the IDAT run
        }
        i = end;
    }
    false // ran out of bytes without a terminating IEND
}

/// Inline-image extent in EMU from a PNG's `IHDR` dimensions (96 dpi → 9525
/// EMU/px), width clamped to ~6 in with aspect preserved; 2 in² fallback if the
/// PNG header can't be read.
#[cfg(feature = "docx")]
fn png_extent_emu(png: &[u8]) -> (u32, u32) {
    const EMU_PER_PX: u64 = 9525;
    const MAX_W: u64 = 5_486_400; // 6 in
    const FALLBACK: u32 = 1_828_800; // 2 in
    if png.len() >= 24
        && png[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        && &png[12..16] == b"IHDR"
    {
        let w = u32::from_be_bytes([png[16], png[17], png[18], png[19]]) as u64;
        let h = u32::from_be_bytes([png[20], png[21], png[22], png[23]]) as u64;
        if w > 0 && h > 0 {
            // u128 intermediates: a huge IHDR (w,h up to u32::MAX) makes
            // `h * EMU_PER_PX * MAX_W` ≈ 2.2e20, which overflows u64.
            let (mut cx, mut cy) = (w * EMU_PER_PX, h * EMU_PER_PX);
            if cx > MAX_W {
                cy = ((cy as u128 * MAX_W as u128) / cx as u128).max(1) as u64;
                cx = MAX_W;
            }
            return (
                cx.min(u32::MAX as u64) as u32,
                cy.min(u32::MAX as u64) as u32,
            );
        }
    }
    (FALLBACK, FALLBACK)
}

/// A self-contained inline-image paragraph fragment referencing relationship `rid`,
/// with drawing/picture id `did`. It declares **all** prefixes it uses — including
/// `w` — on the root `w:p`, so it grafts correctly into any host `document.xml`
/// regardless of which prefix (or default namespace) the host bound for
/// WordprocessingML.
#[cfg(feature = "docx")]
fn image_paragraph_xml(rid: &str, cx: u32, cy: u32, did: u32) -> String {
    const W: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
    const WP: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
    const A: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
    const PIC: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
    const R: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    format!(
        r#"<w:p xmlns:w="{W}"><w:r><w:drawing xmlns:wp="{WP}" xmlns:a="{A}" xmlns:pic="{PIC}" xmlns:r="{R}"><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{did}" name="image{did}"/><a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:nvPicPr><pic:cNvPr id="{did}" name="image{did}"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"#
    )
}

impl DocState {
    /// Open and decode a legacy `.doc` from its raw OLE2 bytes.
    fn open(bytes: &[u8]) -> Result<Self> {
        let mut container = ole::Container::open(bytes)?;
        let word = container.required("WordDocument")?;
        let fib = Fib::parse(&word)?;

        // Refuse encrypted/obfuscated docs (catdoc/POI behaviour) rather than
        // indexing scrambled bytes.
        if fib.encrypted {
            return Err(Error::Encrypted {
                obfuscated: fib.obfuscated,
            });
        }
        // Pre-Word-97 (Word 6/95) has an all-8-bit text model and a different
        // FIB/piece-table layout; route those to a fallback extractor.
        if fib.nfib < 0x00C1 {
            return Err(Error::UnsupportedVersion(fib.nfib));
        }

        // Prefer the table stream the FIB selects; fall back to the other since
        // some writers emit only one.
        let table = container
            .stream(fib.table_stream())?
            .or(container.stream(if fib.which_table_stream_one {
                "0Table"
            } else {
                "1Table"
            })?)
            .ok_or(Error::MissingStream("0Table/1Table"))?;

        let end = fib.fc_clx.saturating_add(fib.lcb_clx).min(table.len());
        let clx = table
            .get(fib.fc_clx..end)
            .ok_or_else(|| Error::PieceTable("CLX out of table bounds".into()))?;

        let pieces = clx::parse(clx)?;
        if pieces.is_empty() {
            return Err(Error::PieceTable("empty piece table".into()));
        }

        // Paragraph properties (best-effort) for table reconstruction; an empty
        // table degrades gracefully to plain-paragraph rendering.
        let papx = papx::parse(&word, &table, fib.fc_plcf_bte_papx, fib.lcb_plcf_bte_papx);
        // Character properties (bold/italic/…) for the rich model; unused by text().
        let chpx = chpx::parse(&word, &table, fib.fc_plcf_bte_chpx, fib.lcb_plcf_bte_chpx);
        // Style sheet (heading levels, style names) for the rich model.
        let stylesheet = stsh::StyleSheet::parse(&table, fib.fc_stshf, fib.lcb_stshf);
        // Font-name table, for resolving CHPX font indices to family names.
        let fonts = ffn::parse(&table, fib.fc_sttbf_ffn, fib.lcb_sttbf_ffn);
        // The Data stream holds inline picture bytes (absent in most text docs).
        let data = container.stream("Data")?.unwrap_or_default();
        // List tables for autonumber reconstruction.
        let lists = list::parse(
            &table,
            fib.fc_plf_lst,
            fib.lcb_plf_lst,
            fib.fc_plf_lfo,
            fib.lcb_plf_lfo,
        );

        let enc = text::encoding_for_codepage(fib.ansi_codepage());
        let decoded = {
            let mut numberer = list::Numberer::new(&lists);
            text::decode_pieces(&word, &pieces, enc, &papx, &mut numberer)
        };
        Ok(DocState {
            raw: decoded.raw,
            labeled: decoded.labeled,
            fib,
            word,
            pieces,
            papx,
            chpx,
            stylesheet,
            lists,
            fonts,
            data,
            enc,
        })
    }

    /// Slice the raw stream by character position (clamped). Word CP counts are
    /// in **UTF-16 code units**, so a supplementary-plane character counts as
    /// two — slice on units, not Rust `char`s, to keep sub-document boundaries
    /// aligned with the FIB `ccp*` counts.
    fn region(&self, start_cp: usize, len: usize) -> String {
        let units: Vec<u16> = self.raw.encode_utf16().collect();
        let start = start_cp.min(units.len());
        let end = start_cp.saturating_add(len).min(units.len());
        String::from_utf16_lossy(&units[start..end])
    }
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document")
            .field("char_count", &self.char_count())
            .field("is_complex", &self.is_complex())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Write};

    /// Build a minimal valid `.doc` in memory: one uncompressed (UTF-16LE)
    /// piece and one compressed (cp1252) piece, with a single-piece CLX in the
    /// `1Table` stream.
    fn synth_doc(text_utf16: &str, ansi_tail: &str) -> Vec<u8> {
        synth_doc_ex(text_utf16, ansi_tail, 0x00C1, 0, 0)
    }

    /// As [`synth_doc`] but with explicit `nFib`, `lid`, and extra FIB flag bits.
    fn synth_doc_ex(
        text_utf16: &str,
        ansi_tail: &str,
        nfib: u16,
        lid: u16,
        extra_flags: u16,
    ) -> Vec<u8> {
        // --- WordDocument stream ---
        let mut word = vec![0u8; 0x200];
        word[0] = 0xEC; // wIdent 0xA5EC
        word[1] = 0xA5;
        word[2..4].copy_from_slice(&nfib.to_le_bytes());
        // flags @ 0x0A: fWhichTblStm (bit 9) set -> use 1Table, plus extras.
        word[0x0A..0x0C].copy_from_slice(&(0x0200u16 | extra_flags).to_le_bytes());
        word[0x14..0x16].copy_from_slice(&lid.to_le_bytes());
        // csw @ 32 = 14, cslw @ 34+28 = 22 (standard Word 97 layout).
        word[32] = 14;
        word[34 + 28] = 22;
        let rglw = 34 + 28 + 2;
        let fclcb = rglw + 22 * 4 + 2;
        // ccpText (field 3) = number of main-doc chars.
        let ccp_text = (text_utf16.chars().count() + ansi_tail.chars().count()) as u32;
        word[rglw + 12..rglw + 16].copy_from_slice(&ccp_text.to_le_bytes());

        // Piece 1 text (UTF-16LE) at offset 0x200; piece 2 (cp1252) right after.
        let utf16: Vec<u8> = text_utf16
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let fc1 = 0x200usize;
        word.extend_from_slice(&utf16);
        let fc2 = word.len();
        word.extend_from_slice(ansi_tail.as_bytes());

        // --- 1Table stream: CLX = Pcdt(0x02) + lcb + PlcPcd(2 pieces) ---
        let cch1 = text_utf16.chars().count() as u32;
        let cch2 = ansi_tail.chars().count() as u32;
        let mut plc = Vec::new();
        // CPs: [0, cch1, cch1+cch2]
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&cch1.to_le_bytes());
        plc.extend_from_slice(&(cch1 + cch2).to_le_bytes());
        // PCD 1: uncompressed, fc = fc1
        plc.extend_from_slice(&0u16.to_le_bytes());
        plc.extend_from_slice(&(fc1 as u32).to_le_bytes());
        plc.extend_from_slice(&0u16.to_le_bytes());
        // PCD 2: compressed, FcCompressed = bit30 | (fc2*2)
        plc.extend_from_slice(&0u16.to_le_bytes());
        plc.extend_from_slice(&(0x4000_0000u32 | (fc2 as u32 * 2)).to_le_bytes());
        plc.extend_from_slice(&0u16.to_le_bytes());

        let mut clx = vec![0x02u8];
        clx.extend_from_slice(&(plc.len() as u32).to_le_bytes());
        clx.extend_from_slice(&plc);

        // fcClx = 0, lcbClx = clx.len() (CLX at start of 1Table).
        word[fclcb + 33 * 8..fclcb + 33 * 8 + 4].copy_from_slice(&0u32.to_le_bytes());
        word[fclcb + 33 * 8 + 4..fclcb + 33 * 8 + 8]
            .copy_from_slice(&(clx.len() as u32).to_le_bytes());

        // --- assemble compound file ---
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        comp.create_stream("/WordDocument")
            .unwrap()
            .write_all(&word)
            .unwrap();
        comp.create_stream("/1Table")
            .unwrap()
            .write_all(&clx)
            .unwrap();
        comp.flush().unwrap();
        comp.into_inner().into_inner()
    }

    #[test]
    fn extracts_utf16_and_cp1252_pieces() {
        let bytes = synth_doc("안녕 rdoc\r세계", " ABC");
        let text = extract_text(&bytes).unwrap();
        assert!(text.contains("안녕 rdoc"), "{text:?}");
        assert!(text.contains("세계"), "{text:?}");
        assert!(text.contains("ABC"), "{text:?}");
        // 0x0D became a line break.
        assert_eq!(text, "안녕 rdoc\n세계 ABC");
    }

    #[test]
    fn main_text_excludes_nothing_when_all_main() {
        let bytes = synth_doc("본문", "X");
        let doc = Document::open(&bytes).unwrap();
        assert_eq!(doc.main_text(), "본문X");
        assert_eq!(doc.char_count(), 3);
        assert!(!doc.is_complex());
    }

    #[test]
    fn refuses_encrypted_document() {
        // fEncrypted = bit 8 (0x0100); fObfuscated = bit 15 (0x8000).
        let bytes = synth_doc_ex("x", "y", 0x00C1, 0, 0x0100 | 0x8000);
        assert!(matches!(
            Document::open(&bytes),
            Err(Error::Encrypted { obfuscated: true })
        ));
    }

    #[test]
    fn refuses_pre_word97_version() {
        // nFib 0x0065 = Word 6.0 (< 0x00C1).
        let bytes = synth_doc_ex("x", "y", 0x0065, 0, 0);
        assert!(matches!(
            Document::open(&bytes),
            Err(Error::UnsupportedVersion(0x0065))
        ));
    }

    #[test]
    fn lid_selects_korean_codepage() {
        // Korean lid 0x0412 -> cp949 -> EUC_KR; default lid -> cp1252.
        let kr = synth_doc_ex("본문", "", 0x00C1, 0x0412, 0);
        let doc = Document::open(&kr).unwrap();
        // The codepage surfaces on the model metadata (fib is now backend-private).
        assert_eq!(doc.model().meta.codepage, 949);
        assert!(std::ptr::eq(
            text::encoding_for_codepage(949),
            encoding_rs::EUC_KR
        ));
        assert!(std::ptr::eq(
            text::encoding_for_codepage(0),
            encoding_rs::WINDOWS_1252
        ));
    }

    #[test]
    fn rejects_non_ole2() {
        assert!(matches!(extract_text(b"not a doc"), Err(Error::NotOle2)));
    }

    #[test]
    fn missing_word_document_stream_errors() {
        let mut comp = cfb::CompoundFile::create(Cursor::new(Vec::new())).unwrap();
        comp.create_stream("/1Table")
            .unwrap()
            .write_all(b"x")
            .unwrap();
        comp.flush().unwrap();
        let bytes = comp.into_inner().into_inner();
        assert!(matches!(
            Document::open(&bytes),
            Err(Error::MissingStream("WordDocument"))
        ));
    }

    /// Build a minimal `.docx` (ZIP of OOXML parts) in memory and read it
    /// end-to-end through the *same* public API as `.doc`, proving format
    /// detection and that both backends feed the shared model/exporters.
    #[cfg(feature = "docx")]
    #[test]
    fn reads_a_minimal_docx_through_the_shared_model() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let png = [0x89u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 1, 2, 3, 4];
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        let text_parts = [
            (
                "word/_rels/document.xml.rels",
                r#"<Relationships><Relationship Id="rId1" Type="http://x/image" Target="media/image1.png"/></Relationships>"#,
            ),
            (
                "word/styles.xml",
                r#"<w:styles><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
            ),
            (
                "word/numbering.xml",
                r#"<w:numbering><w:abstractNum w:abstractNumId="0"><w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl></w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document><w:body>
                    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>제목</w:t></w:r></w:p>
                    <w:p><w:r><w:rPr><w:b/></w:rPr><w:t>굵게</w:t></w:r><w:r><w:t> 보통</w:t></w:r></w:p>
                    <w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr></w:pPr><w:r><w:t>항목</w:t></w:r></w:p>
                    <w:p><w:r><w:drawing><a:blip r:embed="rId1"/></w:drawing></w:r></w:p>
                    <w:tbl>
                        <w:tr><w:tc><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc></w:tr>
                        <w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr>
                    </w:tbl>
                </w:body></w:document>"#,
            ),
        ];
        for (name, body) in text_parts {
            zw.start_file(name, opt).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
        }
        zw.start_file("word/media/image1.png", opt).unwrap();
        zw.write_all(&png).unwrap();
        let bytes = zw.finish().unwrap().into_inner();

        let doc = Document::open(&bytes).unwrap();

        // Flat text: heading, emphasis run merge, list item, tab-joined table row.
        let text = doc.text();
        assert!(text.contains("제목"), "{text:?}");
        assert!(text.contains("굵게 보통"), "{text:?}");
        assert!(text.contains("항목"), "{text:?}");
        assert!(text.contains("A\tB"), "{text:?}");

        // Markdown via the shared exporter.
        let md = doc.to_markdown();
        assert!(md.contains("# 제목"), "{md}");
        assert!(md.contains("**굵게**"), "{md}");
        assert!(md.contains("1. 항목"), "{md}"); // numbering → ordered list
        assert!(md.contains("| A | B |"), "{md}");

        // HTML via the shared exporter.
        let html = doc.to_html();
        assert!(html.contains("<h1>제목</h1>"), "{html}");
        assert!(html.contains("<strong>굵게</strong>"), "{html}");

        // Image extraction through the shared accessor.
        let imgs = doc.images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].mime.as_deref(), Some("image/png"));
        assert_eq!(imgs[0].bytes.as_deref(), Some(&png[..]));

        assert!(!doc.is_complex());
        assert!(doc.model().meta.stats.tables >= 1);
    }

    #[cfg(feature = "docx")]
    #[test]
    fn docx_magic_routes_to_docx_backend() {
        // A truncated/garbage ZIP is a clean Docx error, never an OLE2 error.
        assert!(matches!(
            Document::open(b"PK\x03\x04garbage"),
            Err(Error::Docx(_))
        ));
    }

    /// Unzip a `.docx` into a name→bytes map for byte-level part comparison.
    #[cfg(feature = "docx")]
    fn unzip_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
        use std::io::Read;
        let mut z = zip::ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
        let mut m = std::collections::BTreeMap::new();
        for i in 0..z.len() {
            let mut f = z.by_index(i).unwrap();
            let n = f.name().to_string();
            let mut b = Vec::new();
            f.read_to_end(&mut b).unwrap();
            m.insert(n, b);
        }
        m
    }

    /// PR2: a `.docx` (with **unique** part names) opened then saved is byte-stable for
    /// every part (the package-preserving no-op round-trip — nothing the model doesn't
    /// carry is touched or dropped). Duplicate-part-name normalization is a separate,
    /// documented behavior (collapsed to the single entry the ZIP reader exposes) covered
    /// by `opc::tests::duplicate_part_names_collapse_deterministically`.
    #[cfg(feature = "docx")]
    #[test]
    fn roundtrip_preserves_unmodeled_parts() {
        // A heading + body ⇒ several parts: document.xml, styles.xml, rels, CT.
        let model = DocModel {
            blocks: vec![
                Block::Paragraph(Paragraph {
                    props: ParaProps {
                        heading_level: Some(1),
                        ..Default::default()
                    },
                    runs: vec![Run {
                        text: "제목".into(),
                        ..Default::default()
                    }],
                }),
                Block::Paragraph(Paragraph {
                    runs: vec![Run {
                        text: "본문".into(),
                        ..Default::default()
                    }],
                    ..Default::default()
                }),
            ],
            ..Default::default()
        };
        // Inject parts rdoc does NOT model — a custom XML item and an entirely
        // unknown binary part — to prove the round-trip preserves arbitrary content,
        // not just the parts the writer happens to emit.
        let orig = {
            use std::io::{Read, Write};
            use zip::write::SimpleFileOptions;
            let gen = write_docx(&model);
            let mut zin = zip::ZipArchive::new(Cursor::new(gen)).unwrap();
            let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
            let opt = SimpleFileOptions::default();
            for i in 0..zin.len() {
                let mut f = zin.by_index(i).unwrap();
                let name = f.name().to_string();
                let mut b = Vec::new();
                f.read_to_end(&mut b).unwrap();
                // Type the unmodeled binary part so it's a valid Word-openable OPC part
                // (not just an untyped extra entry): add a `bin` Default to [Content_Types].
                if name == "[Content_Types].xml" {
                    let s = String::from_utf8(b).unwrap().replace(
                        "</Types>",
                        r#"<Default Extension="bin" ContentType="application/octet-stream"/></Types>"#,
                    );
                    b = s.into_bytes();
                }
                zw.start_file(name, opt).unwrap();
                zw.write_all(&b).unwrap();
            }
            zw.start_file("customXml/item1.xml", opt).unwrap();
            zw.write_all(br#"<?xml version="1.0"?><root note="keep me"/>"#)
                .unwrap();
            zw.start_file("word/unknownPart.bin", opt).unwrap();
            zw.write_all(&[0u8, 1, 2, 3, 255, 254]).unwrap();
            zw.finish().unwrap().into_inner()
        };
        let saved = Document::open(&orig).unwrap().save().unwrap();

        let a = unzip_parts(&orig);
        let b = unzip_parts(&saved);
        assert_eq!(
            a.keys().collect::<Vec<_>>(),
            b.keys().collect::<Vec<_>>(),
            "part set changed on no-op save"
        );
        for (name, bytes) in &a {
            assert_eq!(bytes, &b[name], "part {name} not byte-stable on no-op save");
        }
        assert!(a.contains_key("word/styles.xml"), "fixture lacked styles");
        // The unmodeled parts survived byte-for-byte.
        assert_eq!(
            b.get("customXml/item1.xml").map(|v| v.as_slice()),
            Some(&br#"<?xml version="1.0"?><root note="keep me"/>"#[..]),
            "custom XML part not preserved"
        );
        assert_eq!(
            b.get("word/unknownPart.bin").map(|v| v.as_slice()),
            Some(&[0u8, 1, 2, 3, 255, 254][..]),
            "unknown binary part not preserved"
        );
        // And the saved package still types both unmodeled parts (content-type
        // correctness, not just byte passthrough), with [Content_Types].xml byte-stable.
        let pkg = crate::opc::Package::from_zip(&saved).unwrap();
        assert!(pkg.part_has_content_type("word/unknownPart.bin"));
        assert!(pkg.part_has_content_type("customXml/item1.xml"));
        assert_eq!(
            a.get("[Content_Types].xml"),
            b.get("[Content_Types].xml"),
            "[Content_Types].xml not byte-stable on no-op save"
        );
    }

    /// PR2: `Document::new()` is a valid blank package that saves and re-opens.
    #[cfg(feature = "docx")]
    #[test]
    fn new_from_template_saves_and_reopens() {
        let doc = Document::new();
        assert!(
            doc.text().trim().is_empty(),
            "blank template should have no body text, got {:?}",
            doc.text()
        );
        let bytes = doc.save().unwrap();
        let reopened = Document::open(&bytes).unwrap();
        assert!(reopened.text().trim().is_empty());
        assert!(unzip_parts(&bytes).contains_key("word/document.xml"));
    }

    /// Build a `.docx` whose body carries exactly what the lossy model drops — a
    /// content control, a field, an mc:AlternateContent shape, and a comment
    /// reference — plus a comments.xml satellite, to prove B preserves them.
    #[cfg(feature = "docx")]
    fn docx_rich_body() -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        let parts: [(&str, &str); 5] = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p><w:sdt><w:sdtContent><w:p><w:r><w:t>SDT-CONTENT</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice></mc:AlternateContent><w:p><w:commentRangeStart w:id="0"/><w:r><w:t>commented</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r></w:p></w:body></w:document>"#,
            ),
            (
                "word/comments.xml",
                r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="0" w:author="A"><w:p><w:r><w:t>note</w:t></w:r></w:p></w:comment></w:comments>"#,
            ),
        ];
        for (name, body) in parts {
            zw.start_file(name, opt).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    /// A genuinely valid 2×3 RGB PNG (correct chunk CRCs + a real zlib `IDAT`) for
    /// image-insertion tests — passes [`is_png`]'s full CRC-checked validation.
    #[cfg(feature = "docx")]
    fn tiny_png() -> Vec<u8> {
        vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xDA, 0x63, 0x60, 0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ]
    }

    /// Wrap a `document.xml` body in a minimal valid package (CT + root rels). The
    /// caller supplies the full `<w:document>…</w:document>` string.
    #[cfg(feature = "docx")]
    fn minimal_docx(document_xml: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            ("[Content_Types].xml", r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_string()),
            ("_rels/.rels", r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_string()),
            ("word/document.xml", document_xml.to_string()),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    #[cfg(feature = "docx")]
    #[test]
    fn edit_reuses_case_variant_document_override() {
        use zip::write::SimpleFileOptions;

        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        let ct = format!(
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/Document.xml" ContentType="{CT_DOCUMENT_MAIN}"/></Types>"#
        );
        for (n, b) in [
            ("[Content_Types].xml", ct.as_str()),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();

        assert_eq!(doc.replace_body_text("OLD", "NEW").unwrap(), 1);

        let saved = doc.save().unwrap();
        let parts = unzip_parts(&saved);
        let ct = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
        assert_eq!(
            ct.to_ascii_lowercase()
                .matches(r#"partname="/word/document.xml""#)
                .count(),
            1,
            "edit duplicated a case-variant document Override: {ct}"
        );
        assert!(Document::open(&saved).is_ok(), "saved output must reopen");
    }

    /// Body & `sectPr` anchoring is **namespace-aware**, so a
    /// foreign `<x:body>` / `<x:sectPr>` cannot misdirect an image insert.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_anchors_are_namespace_aware() {
        // (a) A document with only a FOREIGN <x:body> (no WML body) must be rejected,
        // not treated as a body.
        let foreign_body = minimal_docx(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:x"><x:body><x:p/></x:body></w:document>"#,
        );
        let mut d = Document::open(&foreign_body).unwrap();
        assert!(
            d.add_image_png(&tiny_png(), "image1.png").is_err(),
            "foreign <x:body> wrongly accepted as a body"
        );

        // (b) A WML body whose LAST child is a foreign <x:sectPr> after the real
        // <w:sectPr>: the image must land before the real w:sectPr, not the x:sectPr.
        let mixed = minimal_docx(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:x="urn:x"><w:body><w:p><w:r><w:t>t</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr><x:sectPr/></w:body></w:document>"#,
        );
        let mut d2 = Document::open(&mixed).unwrap();
        d2.add_image_png(&tiny_png(), "image1.png").unwrap();
        let body = String::from_utf8(unzip_parts(&d2.save().unwrap())["word/document.xml"].clone())
            .unwrap();
        let draw = body.find("<w:drawing").expect("drawing inserted");
        let real_sect = body.find("<w:sectPr").expect("w:sectPr present");
        assert!(
            draw < real_sect,
            "image must precede the real w:sectPr (not the foreign x:sectPr):\n{body}"
        );

        // (c) A nested WML `<w:body>` (not a child of w:document) must NOT be mistaken
        // for the real body — the image goes into the document's direct-child body.
        let nested = minimal_docx(
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:foo><w:body><w:p><w:r><w:t>FAKE</w:t></w:r></w:p></w:body></w:foo><w:body><w:p><w:r><w:t>REAL</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        );
        let mut d3 = Document::open(&nested).unwrap();
        d3.add_image_png(&tiny_png(), "image1.png").unwrap();
        let body3 =
            String::from_utf8(unzip_parts(&d3.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        let real = body3.find("REAL").expect("real body present");
        let drew = body3.find("<w:drawing").expect("drawing inserted");
        assert!(
            drew > real,
            "image went into the nested fake body, not the document's real body:\n{body3}"
        );
        assert!(body3.contains("FAKE"), "nested body content lost");
    }

    /// `replace_body_text` is anchored to the document body — a
    /// `w:t` that is a SIBLING of `w:body` (malformed/extension input) is not edited.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_is_scoped_to_body() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:t>OUTSIDE</w:t><w:body><w:p><w:r><w:t>INSIDE</w:t></w:r></w:p></w:body></w:document>"#;
        let mut doc = Document::open(&minimal_docx(doc_xml)).unwrap();
        // The out-of-body run is not matched (count 0, no-op).
        assert_eq!(doc.replace_body_text("OUTSIDE", "X").unwrap(), 0);
        // The in-body run is.
        assert_eq!(doc.replace_body_text("INSIDE", "EDITED").unwrap(), 1);
        let body =
            String::from_utf8(unzip_parts(&doc.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        assert!(
            body.contains("OUTSIDE") && body.contains("EDITED"),
            "out-of-body text must be untouched, in-body text edited: {body}"
        );
    }

    /// A misplaced XML declaration makes `document.xml` malformed for editing, even if the
    /// lenient read view can extract the body text. The element-tree editor must keep it
    /// passthrough-only rather than serializing a still-invalid edited part.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_rejects_late_xml_declaration() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document><?xml version="1.0"?>"#;
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        let mut doc = Document::open(&bytes).unwrap();

        assert!(
            doc.replace_body_text("OLD", "NEW").is_err(),
            "malformed document.xml must be read-only for element-tree edits"
        );

        let after = unzip_parts(&doc.save().unwrap());
        assert_eq!(
            after["word/document.xml"], before["word/document.xml"],
            "failed edit must leave malformed document.xml byte-identical"
        );
    }

    /// `replace_body_text` matches `w:t` text held as CDATA.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_matches_cdata() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t><![CDATA[OLD]]></w:t></w:r></w:p></w:body></w:document>"#;
        let mut doc = Document::open(&minimal_docx(doc_xml)).unwrap();
        let n = doc.replace_body_text("OLD", "NEW").unwrap();
        assert_eq!(n, 1, "CDATA w:t text not matched");
        let body =
            String::from_utf8(unzip_parts(&doc.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        assert!(
            body.contains("NEW") && !body.contains("OLD"),
            "CDATA text not replaced: {body}"
        );
    }

    /// Edited text must serialize as XML-valid character data even when caller input
    /// contains Rust-valid but XML-forbidden scalar values.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_drops_xml_forbidden_scalars() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#;
        let mut doc = Document::open(&minimal_docx(doc_xml)).unwrap();
        let n = doc.replace_body_text("OLD", "A\u{FFFF}B\u{FFFE}C").unwrap();
        assert_eq!(n, 1);
        let saved = doc.save().unwrap();
        let body = String::from_utf8(unzip_parts(&saved)["word/document.xml"].clone()).unwrap();
        assert!(
            body.contains("<w:t>ABC</w:t>"),
            "forbidden XML scalar leaked into document.xml: {body:?}"
        );
        assert_eq!(Document::open(&saved).unwrap().text(), "ABC");
    }

    /// See [`replace_body_text_is_scoped_to_body`]; `add_image_png` is
    /// transactional even when the body insertion
    /// would fail. With the node budget lowered so the drawing fragment can't fit, the
    /// call errors and leaves the package untouched — no media part, content-type, or
    /// relationship is added.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_png_rolls_back_on_budget_failure() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>x</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        // Budget just above the body's node count: parsing document.xml still succeeds,
        // but grafting the (multi-node) drawing fragment would exceed it.
        let doc_nodes = xmltree::XmlTree::parse(doc_xml.as_bytes())
            .unwrap()
            .node_count();
        xmltree::set_test_node_budget(doc_nodes + 1);
        let mut doc = Document::open(&bytes).unwrap();
        let r = doc.add_image_png(&tiny_png(), "image1.png");
        xmltree::reset_test_node_budget(); // back to production MAX_NODES before asserting
        assert!(r.is_err(), "over-budget image insert should error");
        let after = unzip_parts(&doc.save().unwrap());
        assert!(
            !after.contains_key("word/media/image1.png"),
            "media part leaked after a failed insert"
        );
        let rels = String::from_utf8_lossy(
            after
                .get("word/_rels/document.xml.rels")
                .map(|v| v.as_slice())
                .unwrap_or(b""),
        );
        assert!(!rels.contains("image1.png"), "image rel leaked: {rels}");
        // document.xml is unchanged (no orphaned drawing).
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "document.xml changed despite a failed insert"
        );
    }

    /// `add_image_png` stays transactional even after a prior
    /// `replace_body_text` left detached arena nodes — the budget is preflighted against
    /// the LIVE tree, so an over-budget insert errors BEFORE the media part/rel are added
    /// (no orphaned package change).
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_png_transactional_after_prior_edit() {
        // The `OLD` run has an extra element child; replacing its text detaches that
        // child (it stays in the arena, uncounted by a fresh re-parse).
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD<w:noBreakHyphen/></w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        let mut doc = Document::open(&bytes).unwrap();
        assert_eq!(doc.replace_body_text("OLD", "NEW").unwrap(), 1);
        // Budget = live arena count + 1: promotion is fine, but the multi-node drawing
        // fragment can't fit, so the insert must be rejected before any package mutation.
        let live = doc.docx_node_count();
        xmltree::set_test_node_budget(live + 1);
        let r = doc.add_image_png(&tiny_png(), "image1.png");
        xmltree::reset_test_node_budget();
        assert!(r.is_err(), "over-budget insert after an edit should error");
        let after = unzip_parts(&doc.save().unwrap());
        assert!(
            !after.contains_key("word/media/image1.png"),
            "media part leaked after a failed insert"
        );
        let rels = String::from_utf8_lossy(
            after
                .get("word/_rels/document.xml.rels")
                .map(|v| v.as_slice())
                .unwrap_or(b""),
        );
        assert!(!rels.contains("image1.png"), "image rel leaked: {rels}");
        let body = String::from_utf8_lossy(&after["word/document.xml"]);
        assert!(body.contains("NEW"), "prior edit lost");
        assert!(!body.contains("<w:drawing"), "drawing leaked: {body}");
    }

    /// An edit can't produce a part/package over the size budget that
    /// the crate would later refuse to open — add_image_png rejects an oversize image up
    /// front, and save() rejects an over-budget edited part. (Budget lowered for the test.)
    #[cfg(feature = "docx")]
    #[test]
    fn edits_respect_part_size_budget() {
        // add_image_png: oversize image rejected before mutation (budget lowered AFTER
        // open, so opening the doc itself is unaffected).
        let mut doc = Document::open(&docx_rich_body()).unwrap();
        crate::opc::set_test_max_part(8); // tiny_png is 68 bytes > 8
        let r = doc.add_image_png(&tiny_png(), "image1.png");
        crate::opc::reset_test_max_part();
        assert!(r.is_err(), "oversize image should be rejected");
        let parts = unzip_parts(&doc.save().unwrap());
        assert!(
            !parts.contains_key("word/media/image1.png"),
            "rejected image leaked"
        );

        // save(): an edited document.xml over the budget is rejected on save.
        let mut doc2 = Document::open(&docx_rich_body()).unwrap();
        doc2.replace_body_text("OLD", "NEW").unwrap();
        crate::opc::set_test_max_part(8); // document.xml is far larger than 8 bytes
        let saved = doc2.save();
        crate::opc::reset_test_max_part();
        assert!(saved.is_err(), "over-budget edited part should fail save");
    }

    /// add_image_png rejects a part name longer than the OPC limit,
    /// so an edit can't produce a package `Document::open` would reject.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_png_rejects_overlong_name() {
        let long = format!("{}.png", "a".repeat(5000)); // valid charset, far over MAX_NAME_LEN
        let mut doc = Document::open(&docx_rich_body()).unwrap();
        assert!(doc.add_image_png(&tiny_png(), &long).is_err());
        // A normal name still works (sanity).
        let mut ok = Document::open(&docx_rich_body()).unwrap();
        assert!(ok.add_image_png(&tiny_png(), "image1.png").is_ok());
    }

    /// An edit REPAIRS a mistyped `word/document.xml` content type
    /// (a generic `application/xml` override) to the WML main+xml type, so the saved file
    /// stays Word-openable — the documented intentional `[Content_Types].xml` rewrite.
    #[cfg(feature = "docx")]
    #[test]
    fn edit_repairs_mistyped_document_content_type() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            // document.xml is mistyped as the generic application/xml (resolves, but wrong).
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();
        assert_eq!(doc.replace_body_text("OLD", "NEW").unwrap(), 1);
        let after = unzip_parts(&doc.save().unwrap());
        let ct = String::from_utf8_lossy(&after["[Content_Types].xml"]);
        assert!(
            ct.contains("wordprocessingml.document.main+xml"),
            "document.xml content type not repaired: {ct}"
        );
    }

    /// A package with NO [Content_Types].xml opens read-only — the
    /// body reads, but edits are refused (regenerating content types from nothing would
    /// leave referenced parts untyped, producing a file Word rejects).
    #[cfg(feature = "docx")]
    #[test]
    fn missing_content_types_is_read_only() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        // No [Content_Types].xml. document.xml references styles.xml via rels.
        for (n, b) in [
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#,
            ),
            (
                "word/styles.xml",
                r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"/>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap(); // opens for read
        assert!(doc.text().contains("OLD"));
        assert!(
            doc.add_image_png(&tiny_png(), "i.png").is_err(),
            "editing a CT-less package must be refused"
        );
        assert!(doc.replace_body_text("OLD", "NEW").is_err());
    }

    /// A malformed UNRELATED `.rels` doesn't block the read path —
    /// `Document::open` succeeds, the body reads, a no-op save preserves the raw malformed
    /// part; only EDITS (which would regenerate metadata lossily) are refused.
    #[cfg(feature = "docx")]
    #[test]
    fn malformed_unrelated_rels_opens_read_only() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let bad_rels = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#; // unclosed root
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
            ),
            ("word/_rels/header1.xml.rels", bad_rels),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap(); // opens despite the malformed .rels
        assert!(doc.text().contains("OLD"), "body should still read");
        assert!(
            doc.replace_body_text("OLD", "NEW").is_err(),
            "edit must refuse"
        );
        assert!(
            doc.add_image_png(&tiny_png(), "i.png").is_err(),
            "edit must refuse"
        );
        let after = unzip_parts(&doc.save().unwrap()); // no-op save still works
        assert_eq!(
            after
                .get("word/_rels/header1.xml.rels")
                .map(|v| v.as_slice()),
            Some(bad_rels.as_bytes()),
            "malformed unrelated .rels not preserved verbatim"
        );
    }

    /// A same-value `replace_body_text("X","X")` is a no-op — it
    /// returns 0 and leaves `document.xml` byte-identical (no canonicalizing promotion).
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_same_value_is_noop() {
        let orig = docx_rich_body();
        let before = unzip_parts(&orig);
        let mut doc = Document::open(&orig).unwrap();
        assert_eq!(doc.replace_body_text("OLD", "OLD").unwrap(), 0);
        let after = unzip_parts(&doc.save().unwrap());
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "same-value replace canonicalized document.xml"
        );
    }

    /// A failed `add_image_png` preflight does NOT promote/
    /// canonicalize `document.xml` — non-canonical input (single-quoted attrs) is left
    /// byte-identical (the preflight reads without dirtying a still-`Raw` part).
    #[cfg(feature = "docx")]
    #[test]
    fn failed_add_image_leaves_noncanonical_xml_byte_identical() {
        // Single-quoted xmlns + no w:body ⇒ add_image_png fails the body check.
        let doc_xml = "<w:document xmlns:w='http://schemas.openxmlformats.org/wordprocessingml/2006/main'></w:document>";
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        let mut doc = Document::open(&bytes).unwrap();
        assert!(doc.add_image_png(&tiny_png(), "i.png").is_err());
        let after = unzip_parts(&doc.save().unwrap());
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "failed insert canonicalized document.xml"
        );
        assert!(
            String::from_utf8_lossy(&after["word/document.xml"]).contains("xmlns:w='"),
            "single-quoted attrs were rewritten despite the insert failing"
        );
    }

    /// `replace_body_text` preflights the node budget for matches
    /// that lack a reusable text carrier (empty `<w:t/>`), so it can't grow the arena
    /// past the budget — it errors cleanly and leaves the document untouched.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_respects_node_budget() {
        // Two empty w:t runs: replacing "" with text would allocate a node for each.
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t/></w:r><w:r><w:t/></w:r></w:p></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        let doc_nodes = xmltree::XmlTree::parse(doc_xml.as_bytes())
            .unwrap()
            .node_count();
        // Budget allows parsing but not the 2 new text nodes the replacement needs.
        xmltree::set_test_node_budget(doc_nodes + 1);
        let mut doc = Document::open(&bytes).unwrap();
        let r = doc.replace_body_text("", "X");
        xmltree::reset_test_node_budget();
        assert!(r.is_err(), "over-budget text replace should error");
        // Untouched: a no-op save preserves document.xml verbatim.
        let after = unzip_parts(&doc.save().unwrap());
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "document.xml changed despite a failed (over-budget) replace"
        );
    }

    /// `replace_body_text` preflights the *attribute* budget the
    /// same way it preflights the node budget. A `w:t` already at the attribute cap whose
    /// replacement needs `xml:space="preserve"` (edge whitespace) would otherwise grow to
    /// cap+1 attributes — an element `XmlTree::parse` would reject. It errors cleanly up
    /// front and leaves the document untouched (transactional, parse/edit symmetry).
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_respects_attribute_budget() {
        // A `w:t` with two attributes; with the cap lowered to 2 it parses but has no room
        // for a new `xml:space`. (w:document carries one xmlns attr; everything else ≤ 2.)
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t w:id="1" w:rsid="2">OLD</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        xmltree::set_test_max_attrs(2);
        let mut doc = Document::open(&bytes).unwrap();
        // Replacement WITH edge whitespace needs `xml:space` → over the attribute cap.
        let over = doc.replace_body_text("OLD", " NEW ");
        // A no-op-whitespace replacement needs no new attribute and still succeeds; it
        // finds "OLD" only because the failed attempt above left no partial edit behind.
        let within = doc.replace_body_text("OLD", "NEW");
        xmltree::set_test_max_attrs(65_536);
        assert!(over.is_err(), "over-attribute-budget replace should error");
        assert_eq!(
            within.unwrap(),
            1,
            "non-whitespace replace within budget should apply (and prove no partial edit)"
        );
    }

    /// The clone-and-swap path makes `add_image_png` all-or-nothing
    /// even when a commit-time tree edit fails (the now-fallible `try_reserve` path). Using
    /// the commit-fail seam, the fragment insert fails AFTER `add_related_part` has committed
    /// the media part + relationship on the clone — and the document must be byte-identical
    /// (no orphaned media part). This test FAILS if the edit mutates the package in place.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_png_rolls_back_on_commit_failure() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p/></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        // Fail the first commit-time edit (the fragment insert) — it runs only AFTER
        // add_related_part has committed the media part + rel on the clone.
        xmltree::set_test_fail_commit_after(0);
        let mut doc = Document::open(&bytes).unwrap();
        let r = doc.add_image_png(&tiny_png(), "image1.png");
        xmltree::reset_test_fail_commit();
        assert!(r.is_err(), "a commit-time failure must surface as Err");
        let after = unzip_parts(&doc.save().unwrap());
        assert!(
            !after.contains_key("word/media/image1.png"),
            "rollback failed: media part orphaned after a failed image insert"
        );
        assert_eq!(
            before, after,
            "a failed add_image_png must leave the package byte-identical"
        );
    }

    /// the clone-and-swap also makes `replace_body_text` all-or-nothing.
    /// With two matching runs and the second run's commit edit forced to fail, NEITHER run
    /// may be rewritten — a partial "NEW" would mean the in-place mutation leaked. FAILS if
    /// the loop edits the live package directly.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_rolls_back_on_commit_failure() {
        let doc_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#;
        let bytes = minimal_docx(doc_xml);
        let before = unzip_parts(&bytes);
        // First run's edit succeeds, second fails — a mid-loop commit failure.
        xmltree::set_test_fail_commit_after(1);
        let mut doc = Document::open(&bytes).unwrap();
        let r = doc.replace_body_text("OLD", "NEW");
        xmltree::reset_test_fail_commit();
        assert!(r.is_err(), "a mid-loop commit failure must surface as Err");
        let after = unzip_parts(&doc.save().unwrap());
        let doc_after = String::from_utf8_lossy(after.get("word/document.xml").unwrap());
        assert!(
            !doc_after.contains("NEW"),
            "rollback failed: a partial edit ('NEW') leaked from a failed replace"
        );
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "document.xml changed despite a failed replace"
        );
    }

    /// A malformed (truncated) `document.xml` makes element-tree
    /// edits fail cleanly, and a no-op save still preserves the raw part byte-for-byte
    /// (the editor never invents close tags to "repair" damaged input).
    #[cfg(feature = "docx")]
    #[test]
    fn malformed_document_xml_edit_errs_but_passthrough_preserves() {
        // Unclosed <w:t>/<w:r>/<w:p>/<w:body>/<w:document>.
        let truncated = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD"#;
        let bytes = minimal_docx(truncated);
        let before = unzip_parts(&bytes);

        let mut doc = Document::open(&bytes).unwrap();
        assert!(
            doc.replace_body_text("OLD", "NEW").is_err(),
            "edit on malformed XML must error"
        );
        assert!(
            doc.add_image_png(&tiny_png(), "image1.png").is_err(),
            "image insert on malformed XML must error"
        );
        // No edit took hold ⇒ save is a passthrough that preserves the raw part.
        let after = unzip_parts(&doc.save().unwrap());
        assert_eq!(
            before.get("word/document.xml"),
            after.get("word/document.xml"),
            "no-op save must preserve the malformed part verbatim"
        );
    }

    /// A `document.xml` that is tokenizable but not a single well-formed
    /// document — multiple top-level elements, or non-whitespace text outside the root — is
    /// passthrough-only. Edits must NOT promote-and-rewrite it (which would leave malformed
    /// multi-root XML); they error and the raw part is preserved byte-for-byte. This FAILS
    /// with a fragment-tolerant body lookup (which would edit the first `w:document`).
    #[cfg(feature = "docx")]
    #[test]
    fn multi_root_or_junk_document_xml_is_passthrough_only() {
        const NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
        let cases = [
            // Two top-level <w:document> elements.
            format!(
                r#"<w:document xmlns:w="{NS}"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document><w:document xmlns:w="{NS}"/>"#
            ),
            // Non-whitespace character data after the root element.
            format!(
                r#"<w:document xmlns:w="{NS}"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>junk"#
            ),
        ];
        for body in cases {
            let bytes = minimal_docx(&body);
            let before = unzip_parts(&bytes);
            let mut doc = Document::open(&bytes).unwrap();
            assert!(
                doc.replace_body_text("OLD", "NEW").is_err(),
                "a non-single-document document.xml must not be editable"
            );
            assert!(
                doc.add_image_png(&tiny_png(), "image1.png").is_err(),
                "a non-single-document document.xml must not accept an image"
            );
            let after = unzip_parts(&doc.save().unwrap());
            assert_eq!(
                before.get("word/document.xml"),
                after.get("word/document.xml"),
                "passthrough must preserve the raw (malformed) document.xml"
            );
        }
    }

    /// PR5: element-tree edit (B) preserves unmodeled body content. Replacing a run
    /// keeps the content control, field, mc:AlternateContent shape, comment
    /// reference, AND the comments.xml satellite.
    #[cfg(feature = "docx")]
    #[test]
    fn edit_preserves_unmodeled_body() {
        let orig = docx_rich_body();
        let before = unzip_parts(&orig);
        let mut doc = Document::open(&orig).unwrap();

        let changed = doc.replace_body_text("OLD", "NEW").unwrap();
        assert_eq!(changed, 1, "expected exactly one run replaced");
        let saved = doc.save().unwrap();
        let after = unzip_parts(&saved);
        let body = String::from_utf8_lossy(&after["word/document.xml"]);

        assert!(
            body.contains("NEW") && !body.contains("OLD"),
            "edit not applied: {body}"
        );
        for needle in [
            "w:sdt",
            "SDT-CONTENT",
            "w:fldSimple",
            "w:instr=\" PAGE \"",
            "mc:AlternateContent",
            "mc:Choice",
            "w:commentReference",
        ] {
            assert!(body.contains(needle), "B edit dropped {needle}: {body}");
        }
        // The comments.xml satellite is untouched, byte-for-byte.
        assert_eq!(
            after.get("word/comments.xml"),
            before.get("word/comments.xml"),
            "comments.xml not preserved"
        );
        // Re-opens cleanly.
        assert!(Document::open(&saved).is_ok());
    }

    /// PR5: lazy promotion — a body edit re-serializes ONLY document.xml; every
    /// other part stays byte-identical.
    #[cfg(feature = "docx")]
    #[test]
    fn lazy_parse_byte_stable() {
        let orig = docx_rich_body();
        let before = unzip_parts(&orig);
        let mut doc = Document::open(&orig).unwrap();
        doc.replace_body_text("OLD", "NEW").unwrap();
        let after = unzip_parts(&doc.save().unwrap());

        for (name, bytes) in &before {
            if name == "word/document.xml" {
                assert_ne!(bytes, &after[name], "document.xml should have changed");
            } else {
                assert_eq!(Some(bytes), after.get(name), "{name} should be byte-stable");
            }
        }
    }

    /// PR5: inserting an image reconciles relationships transactionally — new media
    /// part + content-type + a non-colliding rId the body's blip references.
    #[cfg(feature = "docx")]
    #[test]
    fn insert_image_reconciles_rels() {
        let png = tiny_png();
        let mut doc = Document::open(&docx_rich_body()).unwrap();
        doc.add_image_png(&png, "image1.png").unwrap();
        let saved = doc.save().unwrap();
        let parts = unzip_parts(&saved);

        assert_eq!(
            parts.get("word/media/image1.png"),
            Some(&png),
            "media not added"
        );
        let ct = String::from_utf8_lossy(&parts["[Content_Types].xml"]);
        assert!(ct.contains("image/png"), "png content-type missing: {ct}");
        let rels = String::from_utf8_lossy(&parts["word/_rels/document.xml.rels"]);
        assert!(
            rels.contains("media/image1.png"),
            "image rel missing: {rels}"
        );
        let body = String::from_utf8_lossy(&parts["word/document.xml"]);
        assert!(
            body.contains("a:blip") && body.contains("r:embed"),
            "drawing missing: {body}"
        );
        let rid = {
            let i = body.find("r:embed=\"").unwrap() + 9;
            let s = &body[i..];
            s[..s.find('"').unwrap()].to_string()
        };

        // Structural assertions via the crate's own OPC parser (not substring checks):
        let pkg = crate::opc::Package::from_zip(&saved).unwrap();
        // The media part resolves to a content type (Override or png Default).
        assert!(
            pkg.part_has_content_type("word/media/image1.png"),
            "media part has no resolvable content type"
        );
        // Exactly one image relationship, its Id is the blip's rId, and every rId on
        // document.xml is unique (no dangling/colliding reference).
        let doc_rels = pkg.rels_for("word/document.xml");
        let imgs: Vec<_> = doc_rels
            .iter()
            .filter(|r| r.rel_type.ends_with("/image") && !r.external)
            .collect();
        assert_eq!(imgs.len(), 1, "expected exactly one image rel");
        assert_eq!(imgs[0].id, rid, "blip rId does not match the image rel Id");
        assert!(
            imgs[0].target.ends_with("media/image1.png"),
            "image rel target wrong"
        );
        let mut ids: Vec<&String> = doc_rels.iter().map(|r| &r.id).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate rIds on document.xml");

        // Re-opens and the image is extractable through the reader.
        let re = Document::open(&saved).unwrap();
        assert!(
            !re.images().is_empty(),
            "inserted image not extractable on reopen"
        );
    }

    /// An inserted image goes BEFORE the body's final `w:sectPr`,
    /// which OOXML requires to stay last.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_inserts_before_sectpr() {
        let mut doc = Document::new(); // blank template ends its body with sectPr
        doc.add_image_png(&tiny_png(), "image1.png").unwrap();
        let saved = doc.save().unwrap();
        let body = String::from_utf8(unzip_parts(&saved)["word/document.xml"].clone()).unwrap();
        let sect = body.rfind("<w:sectPr").expect("sectPr present");
        let draw = body.find("<w:drawing").expect("drawing inserted");
        assert!(draw < sect, "image must precede the final sectPr:\n{body}");
        // sectPr is still the last body-level element (nothing after its close).
        let tail = &body[body.rfind("</w:sectPr>").unwrap()..];
        assert!(
            !tail.contains("<w:p"),
            "a paragraph follows sectPr (invalid order): {tail}"
        );
        assert_eq!(Document::open(&saved).unwrap().images().len(), 1);
    }

    /// `replace_body_text` edits WordprocessingML `w:t` only, not
    /// DrawingML `a:t` inside shapes/charts.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_skips_drawingml_text() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p><w:p><w:r><w:drawing><a:t>OLD</a:t></w:drawing></w:r></w:p></w:body></w:document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();
        let n = doc.replace_body_text("OLD", "NEW").unwrap();
        assert_eq!(n, 1, "should edit only the w:t run");
        let body =
            String::from_utf8(unzip_parts(&doc.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        assert!(body.contains("<w:t>NEW</w:t>"), "w:t not edited: {body}");
        assert!(
            body.contains("<a:t>OLD</a:t>"),
            "a:t wrongly edited: {body}"
        );
    }

    /// `try_new` is a non-panicking constructor.
    #[cfg(feature = "docx")]
    #[test]
    fn try_new_yields_valid_blank() {
        let doc = Document::try_new().unwrap();
        assert!(doc.text().trim().is_empty());
        assert!(Document::open(&doc.save().unwrap()).is_ok());
    }

    /// Helper: a one-part `.docx` wrapping the given `<w:body>` inner XML.
    #[cfg(feature = "docx")]
    fn docx_with_body_xml(document_xml: &str) -> Vec<u8> {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            ("word/document.xml", document_xml),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        zw.finish().unwrap().into_inner()
    }

    /// `replace_body_text` resolves namespaces — it
    /// skips a bare `<t>` under a `w:drawing` that binds DrawingML as the DEFAULT
    /// namespace, while still editing the real `w:t`.
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_skips_default_ns_drawingml() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p><w:p><w:r><w:drawing xmlns="http://schemas.openxmlformats.org/drawingml/2006/main"><t>OLD</t></w:drawing></w:r></w:p></w:body></w:document>"#;
        let mut doc = Document::open(&docx_with_body_xml(xml)).unwrap();
        let n = doc.replace_body_text("OLD", "NEW").unwrap();
        assert_eq!(n, 1, "should edit only the WordprocessingML w:t");
        let body =
            String::from_utf8(unzip_parts(&doc.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        assert!(body.contains("<w:t>NEW</w:t>"), "w:t not edited: {body}");
        assert!(
            body.contains("<t>OLD</t>"),
            "default-ns DrawingML text wrongly edited: {body}"
        );
    }

    /// Namespace resolution edits genuine `w:t` even inside a text
    /// box nested under `w:drawing` (which the earlier blanket-skip approach missed).
    #[cfg(feature = "docx")]
    #[test]
    fn replace_body_text_edits_textbox_wml() {
        let xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:drawing><wps:txbx xmlns:wps="urn:wps"><w:txbxContent><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:txbxContent></wps:txbx></w:drawing></w:r></w:p></w:body></w:document>"#;
        let mut doc = Document::open(&docx_with_body_xml(xml)).unwrap();
        let n = doc.replace_body_text("OLD", "NEW").unwrap();
        assert_eq!(n, 1, "text-box w:t should be editable");
        let body =
            String::from_utf8(unzip_parts(&doc.save().unwrap())["word/document.xml"].clone())
                .unwrap();
        assert!(
            body.contains("<w:t>NEW</w:t>"),
            "text-box w:t not edited: {body}"
        );
    }

    /// `add_image_png` rejects non-PNG, forged-framing, AND
    /// CRC-correct-but-semantically-invalid PNG bytes (the validator checks chunk CRCs,
    /// IHDR fields, and non-empty IDAT — a correct signature/CRC is not enough).
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_png_rejects_non_png() {
        const SIG: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        // A correctly-framed, correctly-CRC'd chunk builder (uses the crate's own crc32).
        fn chunk(typ: &[u8; 4], data: &[u8]) -> Vec<u8> {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(typ);
            c.extend_from_slice(data);
            let crc = super::crc32(&[&typ[..], data].concat());
            c.extend_from_slice(&crc.to_be_bytes());
            c
        }
        // The 11-byte zlib IDAT payload from a real 2×3 PNG.
        let real_idat = &[
            0x78u8, 0xDA, 0x63, 0x60, 0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01,
        ];

        let mut bad: Vec<Vec<u8>> = vec![
            b"this is not a png".to_vec(),
            SIG.to_vec(), // signature only
            // first chunk spells IHDR but wrong length (5 ≠ 13)
            [SIG, &[0, 0, 0, 5], b"IHDR", &[0; 9]].concat(),
        ];
        // Bad CRC on an otherwise well-framed IHDR.
        {
            let mut v = SIG.to_vec();
            v.extend_from_slice(&[0, 0, 0, 13]);
            v.extend_from_slice(b"IHDR");
            v.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]);
            v.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]); // wrong CRC
            bad.push(v);
        }
        // CRC-CORRECT but IMPOSSIBLE color type (99): every CRC valid, still not a PNG.
        {
            let mut v = SIG.to_vec();
            v.extend(chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 3, 8, 99, 0, 0, 0]));
            v.extend(chunk(b"IDAT", real_idat));
            v.extend(chunk(b"IEND", &[]));
            bad.push(v);
        }
        // CRC-CORRECT valid IHDR but EMPTY IDAT (no image data).
        {
            let mut v = SIG.to_vec();
            v.extend(chunk(b"IHDR", &[0, 0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0]));
            v.extend(chunk(b"IDAT", &[]));
            v.extend(chunk(b"IEND", &[]));
            bad.push(v);
        }
        // A real PNG with trailing junk after IEND.
        let mut trailing = tiny_png();
        trailing.extend_from_slice(b"junk");
        bad.push(trailing);

        for (i, b) in bad.iter().enumerate() {
            let mut doc = Document::open(&docx_rich_body()).unwrap();
            assert!(
                doc.add_image_png(b, "x.png").is_err(),
                "invalid PNG #{i} was accepted"
            );
        }
        // A genuinely valid PNG is accepted.
        let mut ok = Document::open(&docx_rich_body()).unwrap();
        assert!(ok.add_image_png(&tiny_png(), "x.png").is_ok());
    }

    /// `add_image_png` rejects unsafe names and existing parts.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_rejects_bad_names() {
        let png = tiny_png();
        for bad in ["../evil.png", "a/b.png", "dir/", "no-ext", "img.jpg", ""] {
            let mut doc = Document::open(&docx_rich_body()).unwrap();
            assert!(
                doc.add_image_png(&png, bad).is_err(),
                "accepted bad name {bad:?}"
            );
        }
        // Existing media name is rejected (no overwrite).
        let mut doc = Document::open(&docx_rich_body()).unwrap();
        doc.add_image_png(&png, "image1.png").unwrap();
        assert!(
            doc.add_image_png(&png, "image1.png").is_err(),
            "overwrote existing media"
        );
    }

    #[cfg(feature = "docx")]
    #[test]
    fn add_image_rejects_case_variant_existing_media_part() {
        use zip::write::SimpleFileOptions;

        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            (
                "[Content_Types].xml",
                format!(
                    r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="{CT_DOCUMENT_MAIN}"/></Types>"#
                )
                .into_bytes(),
            ),
            (
                "_rels/.rels",
                br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
            ),
            (
                "word/document.xml",
                br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#.to_vec(),
            ),
            ("word/media/Image1.png", tiny_png()),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(&b).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();

        assert!(
            doc.add_image_png(&tiny_png(), "image1.png").is_err(),
            "case-variant media part must be treated as an existing part"
        );

        let parts = unzip_parts(&doc.save().unwrap());
        assert!(parts.contains_key("word/media/Image1.png"));
        assert!(
            !parts.contains_key("word/media/image1.png"),
            "failed insert left a case-variant duplicate media part"
        );
    }

    /// A failed `add_image_png` (no `w:body`) leaves the package
    /// unchanged — no orphaned media part or relationship.
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_rolls_back_without_body() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        for (n, b) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"></w:document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();
        assert!(doc.add_image_png(&tiny_png(), "image1.png").is_err());
        // No orphaned media part nor image relationship was persisted.
        let parts = unzip_parts(&doc.save().unwrap());
        assert!(
            !parts.contains_key("word/media/image1.png"),
            "orphaned media"
        );
        let rels = parts
            .get("word/_rels/document.xml.rels")
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        assert!(!rels.contains("media/image1.png"), "orphaned rel: {rels}");
    }

    /// Two inserted images get distinct drawing ids, and insertion
    /// works when the host binds WordprocessingML as the default namespace (no `w:`).
    #[cfg(feature = "docx")]
    #[test]
    fn add_image_unique_ids_and_default_ns_host() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;
        let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opt = SimpleFileOptions::default();
        // Host binds the main namespace as DEFAULT (elements have no `w:` prefix).
        for (n, b) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/document.xml",
                r#"<document xmlns="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><body><p><r><t>hi</t></r></p></body></document>"#,
            ),
        ] {
            zw.start_file(n, opt).unwrap();
            zw.write_all(b.as_bytes()).unwrap();
        }
        let bytes = zw.finish().unwrap().into_inner();
        let mut doc = Document::open(&bytes).unwrap();
        doc.add_image_png(&tiny_png(), "image1.png").unwrap();
        doc.add_image_png(&tiny_png(), "image2.png").unwrap();
        let saved = doc.save().unwrap();
        let parts = unzip_parts(&saved);
        let body = String::from_utf8_lossy(&parts["word/document.xml"]);
        // Two distinct docPr ids (1 and 2), not duplicated "1".
        assert!(body.contains(r#"docPr id="1""#), "first drawing id: {body}");
        assert!(
            body.contains(r#"docPr id="2""#),
            "second drawing id not unique: {body}"
        );
        // python-docx-grade validity: re-opens and both images extract.
        assert_eq!(Document::open(&saved).unwrap().images().len(), 2);
    }

    /// `set_part` corrects a stale/mismatched content-type override
    /// rather than leaving the wrong one.
    #[cfg(feature = "docx")]
    #[test]
    fn set_part_updates_mismatched_content_type() {
        let mut pkg = crate::opc::Package::from_zip(&docx_rich_body()).unwrap();
        // Re-type document.xml with a (deliberately wrong then) corrected override.
        pkg.set_part(
            "word/document.xml",
            b"<w:document/>".to_vec(),
            Some("application/xml"),
        );
        pkg.set_part(
            "word/document.xml",
            b"<w:document/>".to_vec(),
            Some(
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml",
            ),
        );
        let ct = String::from_utf8(pkg.part("[Content_Types].xml").unwrap()).unwrap();
        assert_eq!(
            ct.matches("/word/document.xml").count(),
            1,
            "duplicate override for the same part: {ct}"
        );
        assert!(
            ct.contains("document.main+xml"),
            "override not corrected: {ct}"
        );
        assert!(!ct.contains(r#"PartName="/word/document.xml" ContentType="application/xml""#));
    }
}
