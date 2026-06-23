//! Modern `.docx` (OOXML WordprocessingML) reading — the second Word backend.
//!
//! A `.docx` is a ZIP of XML parts: `word/document.xml` (the body — paragraphs,
//! runs, tables), `word/styles.xml` (style → heading level / name),
//! `word/numbering.xml` (list levels → ordered/bullet),
//! `word/_rels/document.xml.rels` (relationship id → hyperlink target / media
//! path), and `word/media/*` (image bytes).
//!
//! Everything is parsed into the **same** [`crate::model::DocModel`] the legacy
//! `.doc` path produces, so [`crate::Document::to_markdown`] /
//! [`crate::Document::to_html`] / [`crate::Document::images`] are shared and
//! `.doc` and `.docx` render identically. This is a *unification* play (one Word
//! crate, no JVM, no external `.docx` dependency) — see the README on how it
//! relates to the mature `docx-rs` crate.

use std::collections::HashMap;
use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::assemble;
use crate::error::{Error, Result};
use crate::model::{Block, DocMeta, DocModel, Image};
use crate::text;

mod body;
mod numbering;
mod styles;

/// Relationship table: `Id` → `(Target, is_external)`.
type Rels = HashMap<String, (String, bool)>;

/// Detect the ZIP / OOXML magic (`PK\x03\x04`).
pub(crate) fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}

/// A parsed `.docx`: the rich model (built eagerly — XML parsing is cheap, so
/// there is no lazy split like the `.doc` path) plus the derived flat text.
pub(crate) struct DocxState {
    /// The **body-only** model (no footnote/endnote blocks). `Document::model()`
    /// re-appends `notes` for the read view; the lossy model is read/render only.
    pub model: DocModel,
    /// Footnote/endnote blocks, kept separate from `model.blocks` (their `.docx`
    /// parts are preserved on save, never inlined into the body).
    pub notes: Vec<Block>,
    /// Full flat text: body, then footnotes/endnotes, then headers and footers.
    pub text: String,
    /// Just the main body (excludes notes and headers/footers).
    pub main_text: String,
    /// The retained OPC package (every part verbatim) — the source of truth for
    /// package-preserving `save()`. Element-tree edits mutate its `document.xml` in
    /// place; the lossy `model` above is the read/render view.
    pub package: crate::opc::Package,
}

impl std::fmt::Debug for DocxState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DocxState")
            .field("blocks", &self.model.blocks.len())
            .finish_non_exhaustive()
    }
}

/// Open and decode a `.docx` from its raw bytes.
pub(crate) fn open(bytes: &[u8]) -> Result<DocxState> {
    // Bound the entry count BEFORE `ZipArchive::new` (which eagerly collects the whole
    // central directory) — same authoritative limit the package layer enforces, so a
    // hostile archive can't amplify on the read path either.
    crate::opc::check_zip_entry_budget(bytes)?;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| Error::Docx(format!("not a valid .docx (zip) container: {e}")))?;

    // All supplementary parts are best-effort: a missing styles/numbering/rels
    // part just means fewer headings/lists/links, never a failure.
    let rels = part(&mut zip, "word/_rels/document.xml.rels")
        .map(|s| parse_rels(&s))
        .unwrap_or_default();
    let styles = part(&mut zip, "word/styles.xml")
        .map(|s| styles::parse(&s))
        .unwrap_or_default();
    let numbering = part(&mut zip, "word/numbering.xml")
        .map(|s| numbering::parse(&s))
        .unwrap_or_default();
    let media = read_media(&mut zip, &rels);

    // The body is the one required part.
    let doc_xml = part(&mut zip, "word/document.xml")
        .ok_or_else(|| Error::Docx("missing word/document.xml".into()))?;

    let ctx = body::Ctx {
        styles: &styles,
        numbering: &numbering,
        rels: &rels,
        media: &media,
        counters: Default::default(),
    };
    let blocks = body::parse_document(&doc_xml, &ctx); // body only
                                                       // Footnotes/endnotes live in their own parts. Keep them SEPARATE from the body
                                                       // (not appended into `model.blocks`); their parts are preserved verbatim on save.
                                                       // They are re-joined for the read/text views below and in `Document::model()`.
    let mut notes = read_notes(
        &mut zip,
        "word/footnotes.xml",
        b"footnote",
        &styles,
        &numbering,
    );
    notes.extend(read_notes(
        &mut zip,
        "word/endnotes.xml",
        b"endnote",
        &styles,
        &numbering,
    ));
    // Running headers/footers referenced by the body's sectPr(s). `ctx` only holds
    // shared (&) borrows of rels/styles/numbering, so the &mut zip pass is fine.
    let (header, footer) = read_headers_footers(&mut zip, &doc_xml, &rels, &styles, &numbering);
    // Stats reflect the full visible content (body + notes).
    let stats = {
        let mut all = blocks.clone();
        all.extend(notes.iter().cloned());
        assemble::compute_stats(&all)
    };
    let model = DocModel {
        blocks, // body only
        // `.docx` text is Unicode (no ANSI codepage); these fields are not
        // meaningful here, unlike the `.doc` path's `lid`/codepage.
        meta: DocMeta {
            codepage: 0,
            lid: 0,
            stats,
        },
        setup: crate::model::DocSetup {
            page: body::scan_page_setup(&doc_xml),
            header,
            footer,
            ..crate::model::DocSetup::default()
        },
    };
    let main_text = body_text(&model); // body only
                                       // Full text: body, then notes, then headers/footers.
    let text = {
        let mut raw = String::new();
        flatten(&model.blocks, &mut raw);
        flatten(&notes, &mut raw);
        flatten(&model.setup.header, &mut raw);
        flatten(&model.setup.footer, &mut raw);
        text::finalize(&raw)
    };
    // Retain the whole package verbatim for package-preserving editing/save. The
    // reader above is unchanged; this is an independent second pass over `bytes`.
    let package = crate::opc::Package::from_zip(bytes)?;
    Ok(DocxState {
        model,
        notes,
        text,
        main_text,
        package,
    })
}

/// The bundled blank template bytes — a valid package this crate ships and tests.
const BLANK_DOCX: &[u8] = include_bytes!("../../assets/blank.docx");

/// A blank `.docx` state from the bundled template — backs [`crate::Document::new`].
/// Cannot fail in practice (a corrupt asset is caught by `new_from_template`); see
/// [`try_blank`] for the non-panicking variant.
pub(crate) fn blank() -> DocxState {
    open(BLANK_DOCX).expect("bundled assets/blank.docx is a valid package")
}

/// Fallible blank-template open — backs [`crate::Document::try_new`].
pub(crate) fn try_blank() -> Result<DocxState> {
    open(BLANK_DOCX)
}

/// Resolve and parse the header/footer parts referenced by the body's sectPr(s).
fn read_headers_footers(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    doc_xml: &str,
    rels: &Rels,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
) -> (Vec<Block>, Vec<Block>) {
    let (hdr_ids, ftr_ids) = body::scan_hf_refs(doc_xml);
    let header = read_hf_parts(zip, &hdr_ids, rels, styles, numbering);
    let footer = read_hf_parts(zip, &ftr_ids, rels, styles, numbering);
    (header, footer)
}

/// Read each unique referenced header/footer part once (dedup by part name), with
/// its own `_rels`/media so links and images inside the part resolve correctly.
fn read_hf_parts(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    ids: &[String],
    rels: &Rels,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
) -> Vec<Block> {
    let mut seen = std::collections::HashSet::new();
    let mut blocks = Vec::new();
    for rid in ids {
        let Some((target, external)) = rels.get(rid) else {
            continue;
        };
        if *external {
            continue;
        }
        let path = normalize_part(target);
        if !seen.insert(path.clone()) {
            continue; // a part can be referenced by several types/sections
        }
        let part_rels = part(zip, &part_rels_path(&path))
            .map(|s| parse_rels(&s))
            .unwrap_or_default();
        let part_media = read_media(zip, &part_rels);
        let hf_ctx = body::Ctx {
            styles,
            numbering,
            rels: &part_rels,
            media: &part_media,
            counters: Default::default(),
        };
        if let Some(xml) = part(zip, &path) {
            blocks.extend(body::parse_hdrftr(&xml, &hf_ctx));
        }
    }
    blocks
}

/// Read a footnotes/endnotes part (if present) into its real notes' blocks, with
/// the part's own rels/media so links and images inside notes resolve.
fn read_notes(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
    tag: &[u8],
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
) -> Vec<Block> {
    let Some(xml) = part(zip, name) else {
        return Vec::new();
    };
    let part_rels = part(zip, &part_rels_path(name))
        .map(|s| parse_rels(&s))
        .unwrap_or_default();
    let part_media = read_media(zip, &part_rels);
    let ctx = body::Ctx {
        styles,
        numbering,
        rels: &part_rels,
        media: &part_media,
        counters: Default::default(),
    };
    body::parse_notes(&xml, &ctx, tag)
}

/// `word/header1.xml` → `word/_rels/header1.xml.rels`.
fn part_rels_path(part_path: &str) -> String {
    match part_path.rsplit_once('/') {
        Some((dir, file)) => format!("{dir}/_rels/{file}.rels"),
        None => format!("_rels/{part_path}.rels"),
    }
}

/// Largest accepted *decompressed* size for an XML part — orders of magnitude
/// above any real document (a 64 MiB `document.xml` is a ~50,000-page doc), but
/// bounds a zip bomb. We reject a part whose declared uncompressed size already
/// exceeds this (rather than silently truncating it), and `take` still caps the
/// actual read in case the ZIP's declared size lies.
const MAX_XML_PART: u64 = 64 << 20;
/// Largest accepted embedded media (image) entry.
const MAX_MEDIA_PART: u64 = 64 << 20;
/// Whole-archive budget for decompressed media. Per-entry caps alone don't bound a
/// hostile package with thousands of large image relationships; this caps the
/// cumulative media inflation across all entries.
const MAX_TOTAL_MEDIA: u64 = 256 << 20;

/// Read a ZIP entry to a UTF-8 string, if present — bounded to guard against a
/// zip bomb (a tiny entry that decompresses to gigabytes).
fn part(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<String> {
    let f = zip.by_name(name).ok()?;
    if f.size() > MAX_XML_PART {
        return None;
    }
    let mut s = String::new();
    f.take(MAX_XML_PART).read_to_string(&mut s).ok()?;
    Some(s)
}

/// Read a ZIP entry to raw bytes (for media), bounded like [`part`].
fn part_bytes(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let f = zip.by_name(name).ok()?;
    if f.size() > MAX_MEDIA_PART {
        return None;
    }
    let mut v = Vec::new();
    f.take(MAX_MEDIA_PART).read_to_end(&mut v).ok()?;
    Some(v)
}

/// Cap on relationships the lenient reader path collects from one `.rels` part — bounds
/// memory on a size-capped but record-stuffed part (the package layer caps separately).
const MAX_REL_RECORDS: usize = 1 << 16;

/// `word/_rels/document.xml.rels`: `<Relationship Id Target TargetMode?/>`.
fn parse_rels(xml: &str) -> Rels {
    let mut r = Reader::from_str(xml);
    let mut map = HashMap::new();
    loop {
        if map.len() >= MAX_REL_RECORDS {
            break; // bounded: stop collecting (lenient read path)
        }
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"Relationship" =>
            {
                if let (Some(id), Some(target)) = (attr_local(&e, b"Id"), attr_local(&e, b"Target"))
                {
                    let external = attr_local(&e, b"TargetMode").as_deref() == Some("External");
                    map.insert(id, (target, external));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    map
}

/// Pre-read every embedded raster (PNG/JPEG/GIF/BMP/TIFF) referenced by an
/// internal relationship into `rel-id → Image`. Metafiles (EMF/WMF) and external
/// links are skipped, mirroring the `.doc` path which leaves them as placeholders.
fn read_media(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    rels: &Rels,
) -> HashMap<String, Image> {
    let mut media = HashMap::new();
    // Collect first to avoid borrowing `rels` while mutably borrowing `zip`.
    let image_rels: Vec<(String, String)> = rels
        .iter()
        .filter(|(_, (target, external))| !external && mime_for(target).is_some())
        .map(|(id, (target, _))| (id.clone(), target.clone()))
        .collect();
    let mut total: u64 = 0;
    for (id, target) in image_rels {
        let Some(mime) = mime_for(&target) else {
            continue;
        };
        let path = normalize_part(&target);
        if let Some(bytes) = part_bytes(zip, &path) {
            // Stop BEFORE inserting a part that would push the in-memory media set past
            // the whole-archive budget, so the advertised cap is a hard ceiling (not
            // cap + one part). `part_bytes` already bounds each part to MAX_MEDIA_PART.
            if total.saturating_add(bytes.len() as u64) > MAX_TOTAL_MEDIA {
                break;
            }
            total = total.saturating_add(bytes.len() as u64);
            let (width_px, height_px) = crate::image::dims(&bytes, mime).unzip();
            media.insert(
                id,
                Image {
                    alt: None,
                    bytes: Some(bytes),
                    mime: Some(mime.to_string()),
                    width_px,
                    height_px,
                },
            );
        }
    }
    media
}

/// A `word/document.xml.rels` relationship target → a ZIP entry name, resolving it
/// relative to the `word/` directory and normalizing `.`/`..`/leading-`/` per OPC URI
/// rules: `media/image1.png` → `word/media/image1.png`, `/word/header1.xml` →
/// `word/header1.xml`, `../customXml/item1.xml` → `customXml/item1.xml`, `./media/x.png`
/// → `word/media/x.png`. A target escaping the package root yields the joined remainder.
fn normalize_part(target: &str) -> String {
    // `/`-absolute targets are package-root relative; others are relative to `word/`.
    let base: &[&str] = if target.starts_with('/') {
        &[]
    } else {
        &["word"]
    };
    let mut segs: Vec<&str> = base.to_vec();
    for seg in target.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                segs.pop();
            }
            s => segs.push(s),
        }
    }
    segs.join("/")
}

/// MIME type for a media target by extension, restricted to the rasters the
/// `.doc` path also extracts. `None` ⇒ not extracted (metafile / unknown).
fn mime_for(target: &str) -> Option<&'static str> {
    let ext = target.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "bmp" => Some("image/bmp"),
        "tif" | "tiff" => Some("image/tiff"),
        _ => None,
    }
}

/// Full flat text: body, then headers and footers (mirroring the `.doc` `text()`
/// convention of body followed by the other sub-documents), normalized by the
/// shared [`text::finalize`] so word-recall is comparable.
/// Flat text of just the main body (excludes headers/footers).
fn body_text(model: &DocModel) -> String {
    let mut raw = String::new();
    flatten(&model.blocks, &mut raw);
    text::finalize(&raw)
}

/// Flat text of the running headers and footers only.
pub(crate) fn header_footer_text(model: &DocModel) -> String {
    let mut raw = String::new();
    flatten(&model.setup.header, &mut raw);
    flatten(&model.setup.footer, &mut raw);
    text::finalize(&raw)
}

fn flatten(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                out.push_str(&p.text());
                out.push('\n');
            }
            Block::Image(_) => {}
            Block::Table(t) => {
                for row in &t.rows {
                    for (i, cell) in row.cells.iter().enumerate() {
                        if i > 0 {
                            out.push('\t');
                        }
                        flatten_inline(&cell.blocks, out);
                    }
                    out.push('\n');
                }
            }
        }
    }
}

/// Flatten a cell's content to a single line (paragraphs and nested-table cells
/// space-joined) so a table row stays one tab-separated line.
fn flatten_inline(blocks: &[Block], out: &mut String) {
    let mut first = true;
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                let t = p.text();
                if !t.is_empty() {
                    if !first {
                        out.push(' ');
                    }
                    out.push_str(&t);
                    first = false;
                }
            }
            Block::Table(t) => {
                for row in &t.rows {
                    for cell in &row.cells {
                        if !first {
                            out.push(' ');
                        }
                        flatten_inline(&cell.blocks, out);
                        first = false;
                    }
                }
            }
            Block::Image(_) => {}
        }
    }
}

// --- shared XML helpers (namespace-prefix-agnostic, like the rxls .xlsx path) ---

/// Strip a namespace prefix: `w:p` → `p`, `r:embed` → `embed`.
pub(crate) fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// First attribute value whose local name equals `key` (unescaped, owned).
pub(crate) fn attr_local(e: &BytesStart<'_>, key: &[u8]) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        if local(a.key.as_ref()) == key {
            a.unescape_value().ok().map(|v| v.into_owned())
        } else {
            None
        }
    })
}

/// Resolve an OOXML on/off toggle: a present element with no `w:val` means *on*;
/// `false`/`0`/`off` mean *off*; anything else is *on*.
pub(crate) fn toggle_on(val: Option<String>) -> bool {
    match val.as_deref() {
        None => true,
        Some(v) => !matches!(v, "false" | "0" | "off"),
    }
}

#[cfg(test)]
mod tests {
    use super::{normalize_part, parse_rels, MAX_REL_RECORDS};

    /// The lenient reader path bounds how many relationships it collects
    /// from one part, so a size-capped but record-stuffed `.rels` can't amplify memory.
    #[test]
    fn reader_rels_parse_is_bounded() {
        let mut s = String::from(
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        );
        for i in 0..(MAX_REL_RECORDS + 1000) {
            s.push_str(&format!(r#"<Relationship Id="r{i}" Target="t{i}"/>"#));
        }
        s.push_str("</Relationships>");
        assert!(
            parse_rels(&s).len() <= MAX_REL_RECORDS,
            "reader rels not bounded"
        );
    }

    /// Relationship targets resolve relative to `word/` with `.`/`..`/
    /// leading-`/` normalized per OPC URI rules (the reader was missing dot-segment ones).
    #[test]
    fn normalize_part_resolves_dot_segments() {
        assert_eq!(normalize_part("media/image1.png"), "word/media/image1.png");
        assert_eq!(
            normalize_part("/word/media/image1.png"),
            "word/media/image1.png"
        );
        assert_eq!(
            normalize_part("./media/image1.png"),
            "word/media/image1.png"
        );
        assert_eq!(
            normalize_part("../customXml/item1.xml"),
            "customXml/item1.xml"
        );
        assert_eq!(normalize_part("header1.xml"), "word/header1.xml");
    }
}
