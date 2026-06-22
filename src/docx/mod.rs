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
    pub model: DocModel,
    pub text: String,
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
    };
    let blocks = body::parse_document(&doc_xml, &ctx);
    let stats = assemble::compute_stats(&blocks);
    let model = DocModel {
        blocks,
        // `.docx` text is Unicode (no ANSI codepage); these fields are not
        // meaningful here, unlike the `.doc` path's `lid`/codepage.
        meta: DocMeta {
            codepage: 0,
            lid: 0,
            stats,
        },
        setup: crate::model::DocSetup::default(),
    };
    let text = model_text(&model);
    Ok(DocxState { model, text })
}

/// Largest accepted *decompressed* size for an XML part — orders of magnitude
/// above any real document (a 64 MiB `document.xml` is a ~50,000-page doc), but
/// bounds a zip bomb. We reject a part whose declared uncompressed size already
/// exceeds this (rather than silently truncating it), and `take` still caps the
/// actual read in case the ZIP's declared size lies.
const MAX_XML_PART: u64 = 64 << 20;
/// Largest accepted embedded media (image) entry.
const MAX_MEDIA_PART: u64 = 64 << 20;

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

/// `word/_rels/document.xml.rels`: `<Relationship Id Target TargetMode?/>`.
fn parse_rels(xml: &str) -> Rels {
    let mut r = Reader::from_str(xml);
    let mut map = HashMap::new();
    loop {
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
    for (id, target) in image_rels {
        let Some(mime) = mime_for(&target) else {
            continue;
        };
        let path = normalize_media(&target);
        if let Some(bytes) = part_bytes(zip, &path) {
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

/// Media targets are relative to the `word/` part, e.g. `media/image1.png`,
/// `/word/media/image1.png`, or (rarely) `../media/...`.
fn normalize_media(target: &str) -> String {
    let t = target.trim_start_matches('/');
    if t.starts_with("word/") {
        t.to_string()
    } else {
        format!("word/{t}")
    }
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

/// Derive flat text from the model: paragraphs become lines, table rows become
/// tab-joined cells, then the shared [`text::finalize`] normalizes it (the same
/// output shape as the `.doc` `text()` path, so word-recall is comparable).
fn model_text(model: &DocModel) -> String {
    let mut raw = String::new();
    flatten(&model.blocks, &mut raw);
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
