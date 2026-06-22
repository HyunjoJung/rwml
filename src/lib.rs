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
//! The parser is panic-free / bounds-checked: malformed input yields [`Error`],
//! never a crash.

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
mod papx;
#[cfg(feature = "render")]
mod render;
mod stsh;
mod table;
mod text;
mod util;
#[cfg(feature = "docx")]
mod write;

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

    /// Build the rich document model — paragraphs, character runs (bold/italic/
    /// …), structured tables, lists, and fields. For `.doc` this is built lazily
    /// (the flat [`Document::text`] path never pays for it); for `.docx` the model
    /// is built eagerly at open and cloned here.
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
            Backend::Docx(d) => d.model.clone(),
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

    /// Render this document to a single-column A4 **PDF** with native typesetting
    /// — `parley` lays out and shapes the text (Korean/CJK line-breaking and font
    /// fallback included) and `krilla` emits the PDF with subsetted embedded fonts
    /// and selectable text. Available with the `render` feature (which raises the
    /// MSRV to 1.88). Tables render as text rows and inline images are not yet
    /// placed — a gridded layout is the next milestone.
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
    /// first `ccpText` characters (excluding footnotes/headers); `.docx` parses
    /// only the main body part, so this equals [`Document::text`].
    pub fn main_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => text::finalize(&d.region(0, d.fib.ccp_text as usize)),
            #[cfg(feature = "docx")]
            Backend::Docx(d) => d.text.clone(),
        }
    }

    /// Normalized footnote + endnote text (`.doc` only; empty for `.docx`, whose
    /// notes live in separate parts not yet parsed).
    pub fn footnote_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => {
                let start = d.fib.ccp_text as usize;
                let len = (d.fib.ccp_ftn + d.fib.ccp_edn) as usize;
                text::finalize(&d.region(start, len))
            }
            #[cfg(feature = "docx")]
            Backend::Docx(_) => String::new(),
        }
    }

    /// Normalized header/footer text (`.doc` only; empty for `.docx`).
    pub fn header_text(&self) -> String {
        match &self.backend {
            Backend::Doc(d) => {
                let start = (d.fib.ccp_text + d.fib.ccp_ftn) as usize;
                text::finalize(&d.region(start, d.fib.ccp_hdd as usize))
            }
            #[cfg(feature = "docx")]
            Backend::Docx(_) => String::new(),
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
}
