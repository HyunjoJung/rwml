//! `DocModel` → `.docx` (OOXML WordprocessingML) — the inverse of the `docx`
//! reader. Each mapping is the exact dual of [`crate::docx`] so the round-trip
//! `read → write → read` preserves the model:
//!
//! * heading level `h` → `<w:outlineLvl w:val="h-1"/>` (the reader recovers
//!   `outline+1`); a heading suppresses list rendering, as in the reader.
//! * list item → `<w:numPr>` referencing a synthetic `numbering.xml` (numId 1 =
//!   ordered/decimal, numId 2 = bullet, all nine levels declared).
//! * alignment → `<w:jc>`; char toggles → `<w:b/> <w:i/> <w:strike/> <w:vanish/>
//!   <w:u w:val="single"/>`.
//! * table merges → `<w:gridSpan>` + `<w:vMerge>` with reconstructed continuation
//!   cells (the reader dropped them on read; we re-insert them).
//! * image → a `media/` part + `<a:blip r:embed>`; hyperlink → an external
//!   relationship + `<w:hyperlink r:id>`.

use super::opc::{Package, Rel};
use super::{esc_attr, esc_text};
use crate::model::{
    Align, AuthoredComment, AuthoredContentControl, AuthoredNote, AuthoredRevision, Block,
    CellMargins, CharProps, Chart, ChartKind, ChartSeries, ChartShape, Color, FieldRole, Image,
    Indent, ParaProps, Paragraph, ParagraphStyle, SectionSetup, Spacing, Table, VertAlign,
};
use crate::{NoteKind, RevisionKind};

/// `Color` → 6-hex `RRGGBB` for OOXML `w:val`.
fn hex(c: Color) -> String {
    format!("{:02X}{:02X}{:02X}", c.r, c.g, c.b)
}

/// Points → twips (1/20 pt), the OOXML measurement unit.
fn pt_twips(pt: f32) -> i64 {
    (pt * 20.0).round() as i64
}

/// Image extent in EMU from intrinsic pixels (96 dpi → 9525 EMU/px), clamped to
/// the ~6in content width with aspect preserved. Falls back to 2in² when the
/// dimensions are unknown.
fn image_extent_emu(w: Option<u32>, h: Option<u32>) -> (u32, u32) {
    const EMU_PER_PX: u32 = 9525;
    const MAX_W_EMU: u32 = 5_486_400; // 6 inches
    const FALLBACK: u32 = 1_828_800; // 2 inches
    let (Some(w), Some(h)) = (w, h) else {
        return (FALLBACK, FALLBACK);
    };
    if w == 0 || h == 0 {
        return (FALLBACK, FALLBACK);
    }
    let mut cx = w.saturating_mul(EMU_PER_PX);
    let mut cy = h.saturating_mul(EMU_PER_PX);
    if cx > MAX_W_EMU {
        cy = ((cy as u64 * MAX_W_EMU as u64) / cx as u64).max(1) as u32;
        cx = MAX_W_EMU;
    }
    (cx.max(1), cy.max(1))
}

const CT_STYLES: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml";
const REL_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const CT_SETTINGS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml";
const REL_SETTINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings";
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";
const CT_COMMENTS: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml";
const CT_COMMENTS_EXT: &str = "application/vnd.ms-word.commentsExt+xml";
const REL_COMMENTS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments";
const REL_COMMENTS_EXT: &str =
    "http://schemas.microsoft.com/office/2011/relationships/commentsExtended";
const CT_FOOTNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml";
const CT_ENDNOTES: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml";
const REL_FOOTNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes";
const REL_ENDNOTES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes";
const CT_CUSTOM_PROPERTIES: &str =
    "application/vnd.openxmlformats-officedocument.custom-properties+xml";
const REL_CUSTOM_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties";
const CT_XML: &str = "application/xml";
const CT_CUSTOM_XML_PROPERTIES: &str =
    "application/vnd.openxmlformats-officedocument.customXmlProperties+xml";
const REL_CUSTOM_XML_PROPERTIES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps";

/// Build a `word/header1.xml` / `footer1.xml` part body from running paragraphs
/// (text + run formatting + alignment; images/links/tables inside a header are
/// out of scope — rendered as their text). Appends a centered `PAGE` field when
/// `page_numbers`. A header/footer must contain at least one paragraph.
fn render_hf_body(blocks: &[crate::model::Block], page_numbers: bool) -> String {
    use crate::model::Block;
    let mut out = String::new();
    for b in blocks {
        if let Block::Paragraph(p) = b {
            out.push_str("<w:p>");
            let jc = match p.props.align {
                Align::Center => Some("center"),
                Align::Right => Some("right"),
                Align::Justify => Some("both"),
                Align::Left => None,
            };
            if let Some(j) = jc {
                out.push_str(&format!(r#"<w:pPr><w:jc w:val="{j}"/></w:pPr>"#));
            }
            for r in &p.runs {
                out.push_str("<w:r>");
                write_rpr(&mut out, &r.props);
                write_run_text(&mut out, &r.text);
                out.push_str("</w:r>");
            }
            out.push_str("</w:p>");
        }
    }
    if page_numbers {
        out.push_str(
            r#"<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p>"#,
        );
    }
    if out.is_empty() {
        out.push_str("<w:p/>");
    }
    out
}

/// Wrap a header/footer body in its root element + namespaces.
fn hf_part(tag: &str, body: &str) -> Vec<u8> {
    format!(r#"{XML_DECL}<w:{tag} xmlns:w="{W_NS}" xmlns:r="{R_NS}">{body}</w:{tag}>"#).into_bytes()
}

fn settings_xml(even_and_odd_headers: bool) -> String {
    let even_odd = if even_and_odd_headers {
        "<w:evenAndOddHeaders/>"
    } else {
        ""
    };
    format!(r#"{XML_DECL}<w:settings xmlns:w="{W_NS}">{even_odd}</w:settings>"#)
}

/// A `word/styles.xml` defining `Normal`, optional `Heading1..6`, and caller
/// supplied paragraph styles.
fn styles_xml(styles: &[ParagraphStyle], include_headings: bool) -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(r#"<w:styles xmlns:w="{W_NS}">"#));
    s.push_str(
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
    );
    if include_headings {
        for (lvl, sz) in [(1u8, 32), (2, 28), (3, 26), (4, 24), (5, 22), (6, 22)] {
            s.push_str(&format!(
                concat!(
                    r#"<w:style w:type="paragraph" w:styleId="Heading{lvl}">"#,
                    r#"<w:name w:val="heading {lvl}"/><w:basedOn w:val="Normal"/>"#,
                    r#"<w:next w:val="Normal"/><w:qFormat/>"#,
                    r#"<w:pPr><w:outlineLvl w:val="{ol}"/></w:pPr>"#,
                    r#"<w:rPr><w:b/><w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/></w:rPr></w:style>"#,
                ),
                lvl = lvl,
                ol = lvl - 1,
                sz = sz
            ));
        }
    }
    for style in styles {
        write_paragraph_style(&mut s, style);
    }
    s.push_str("</w:styles>");
    s
}

fn write_paragraph_style(out: &mut String, style: &ParagraphStyle) {
    if style.id.is_empty() || style.name.is_empty() {
        return;
    }
    let id = esc_attr(&style.id);
    let name = esc_attr(&style.name);
    out.push_str(&format!(
        r#"<w:style w:type="paragraph" w:styleId="{id}"><w:name w:val="{name}"/>"#
    ));
    if let Some(based_on) = style.based_on.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!(r#"<w:basedOn w:val="{}"/>"#, esc_attr(based_on)));
    }
    if let Some(next) = style.next.as_deref().filter(|s| !s.is_empty()) {
        out.push_str(&format!(r#"<w:next w:val="{}"/>"#, esc_attr(next)));
    }
    if style.q_format {
        out.push_str("<w:qFormat/>");
    }
    write_style_ppr(out, style);
    write_rpr(out, &style.run);
    out.push_str("</w:style>");
}

fn write_style_ppr(out: &mut String, style: &ParagraphStyle) {
    let jc = match style.align {
        Align::Left => None,
        Align::Center => Some("center"),
        Align::Right => Some("right"),
        Align::Justify => Some("both"),
    };
    let sp = style.spacing;
    let ind = style.indent;
    let has_spacing = sp.before_pt.is_some() || sp.after_pt.is_some() || sp.line_pct.is_some();
    let has_indent = ind.left_pt.is_some()
        || ind.right_pt.is_some()
        || ind.first_line_pt.is_some()
        || ind.hanging_pt.is_some();
    let outline = style.heading_level.map(|level| level.clamp(1, 9) - 1);
    if jc.is_none() && !has_spacing && !has_indent && style.shading.is_none() && outline.is_none() {
        return;
    }
    out.push_str("<w:pPr>");
    if let Some(c) = style.shading {
        out.push_str(&format!(
            r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
            hex(c)
        ));
    }
    write_spacing(out, sp);
    write_indent(out, ind);
    if let Some(j) = jc {
        out.push_str(&format!(r#"<w:jc w:val="{j}"/>"#));
    }
    if let Some(o) = outline {
        out.push_str(&format!(r#"<w:outlineLvl w:val="{o}"/>"#));
    }
    out.push_str("</w:pPr>");
}

fn write_spacing(out: &mut String, sp: Spacing) {
    if sp.before_pt.is_none() && sp.after_pt.is_none() && sp.line_pct.is_none() {
        return;
    }
    let mut a = String::new();
    if let Some(b) = sp.before_pt {
        a += &format!(r#" w:before="{}""#, pt_twips(b));
    }
    if let Some(af) = sp.after_pt {
        a += &format!(r#" w:after="{}""#, pt_twips(af));
    }
    if let Some(l) = sp.line_pct {
        a += &format!(
            r#" w:line="{}" w:lineRule="auto""#,
            (l * 240.0).round() as i64
        );
    }
    out.push_str(&format!("<w:spacing{a}/>"));
}

fn write_indent(out: &mut String, ind: Indent) {
    if ind.left_pt.is_none()
        && ind.right_pt.is_none()
        && ind.first_line_pt.is_none()
        && ind.hanging_pt.is_none()
    {
        return;
    }
    let mut a = String::new();
    if let Some(l) = ind.left_pt {
        a += &format!(r#" w:left="{}""#, pt_twips(l));
    }
    if let Some(r) = ind.right_pt {
        a += &format!(r#" w:right="{}""#, pt_twips(r));
    }
    if let Some(f) = ind.first_line_pt {
        a += &format!(r#" w:firstLine="{}""#, pt_twips(f));
    }
    if let Some(h) = ind.hanging_pt {
        a += &format!(r#" w:hanging="{}""#, pt_twips(h));
    }
    out.push_str(&format!("<w:ind{a}/>"));
}

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const W14_NS: &str = "http://schemas.microsoft.com/office/word/2010/wordml";
const W15_NS: &str = "http://schemas.microsoft.com/office/word/2012/wordml";
const MC_NS: &str = "http://schemas.openxmlformats.org/markup-compatibility/2006";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const C_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/chart";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const PIC_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const CT_DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const CT_CHART: &str = "application/vnd.openxmlformats-officedocument.drawingml.chart+xml";
const CT_EMBEDDED_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
const CT_XLSX_WORKBOOK: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";
const CT_XLSX_WORKSHEET: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml";
const CT_XLSX_STYLES: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml";
const CT_XLSX_SHARED_STRINGS: &str =
    "application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
const REL_CHART: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart";
const REL_PACKAGE: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/package";
const REL_XLSX_WORKSHEET: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet";
const REL_XLSX_STYLES: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles";
const REL_XLSX_SHARED_STRINGS: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings";
const S_NS: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
/// Hard cap on table columns / a cell's column or row span, so a hostile model
/// (`col_span = u16::MAX`) cannot amplify into millions of `<w:gridCol>`/cells.
const MAX_TABLE_COLS: usize = 1024;

fn page_number_type_xml(setup: &SectionSetup) -> String {
    if setup.page_number_start.is_none() && setup.page_number_format.is_none() {
        return String::new();
    }
    let mut out = String::from("<w:pgNumType");
    if let Some(start) = setup.page_number_start {
        out.push_str(&format!(r#" w:start="{}""#, start.max(1)));
    }
    if let Some(format) = setup.page_number_format {
        out.push_str(&format!(r#" w:fmt="{}""#, format.wml_value()));
    }
    out.push_str("/>");
    out
}

fn doc_grid_xml(setup: &SectionSetup) -> String {
    let Some(grid) = setup.doc_grid else {
        return String::new();
    };
    let mut out = format!(r#"<w:docGrid w:type="{}""#, grid.grid_type.wml_value());
    if let Some(line_pitch) = grid.line_pitch {
        out.push_str(&format!(r#" w:linePitch="{line_pitch}""#));
    }
    if let Some(character_space) = grid.character_space {
        out.push_str(&format!(r#" w:charSpace="{character_space}""#));
    }
    out.push_str("/>");
    out
}

/// Side-state accumulated while folding the model into `document.xml`: the body
/// XML is built in `out` strings passed to each method, while these tables grow.
struct Ctx {
    /// `word/_rels/document.xml.rels` entries (hyperlinks + images).
    doc_rels: Vec<Rel>,
    /// Media parts to emit: `(part path, bytes, extension, content-type)`.
    media: Vec<(String, Vec<u8>, &'static str, &'static str)>,
    /// Chart parts to emit: `(part path, bytes)`.
    chart_parts: Vec<(String, Vec<u8>)>,
    /// Chart relationship files to emit: `(rels path, relationships)`.
    chart_rels: Vec<(String, Vec<Rel>)>,
    /// Embedded XLSX workbooks backing authored chart data.
    embedded_workbooks: Vec<(String, Vec<u8>)>,
    /// Header/footer parts to emit: `(part path, content-type, bytes)`.
    hf_parts: Vec<(String, &'static str, Vec<u8>)>,
    /// Next relationship id ordinal.
    next_rid: u32,
    /// Whether any list item was emitted (⇒ write `numbering.xml`).
    has_list: bool,
    /// Whether a generated heading style is needed in `styles.xml`.
    has_heading: bool,
    /// Whether any paragraph style reference was emitted (⇒ write `styles.xml`).
    has_styles: bool,
    /// Whether authored even-page header/footer variants require settings.xml.
    has_even_header_footer: bool,
    /// Image counter for unique `media/imageN` names + drawing ids.
    img_id: u32,
    /// Chart counter for unique `charts/chartN.xml` names.
    chart_id: u32,
    /// Drawing counter for unique `wp:docPr` ids.
    drawing_id: u32,
    /// Next authored comment id.
    comment_id: u32,
    /// Next authored revision id.
    revision_id: u32,
    /// Authored comments emitted while writing body runs.
    comments: Vec<WrittenComment>,
    /// Next authored bookmark id.
    bookmark_id: u32,
    /// Authored footnotes emitted while writing body runs.
    footnotes: Vec<WrittenNote>,
    /// Authored endnotes emitted while writing body runs.
    endnotes: Vec<WrittenNote>,
    /// Next generated header part number.
    header_id: u32,
    /// Next generated footer part number.
    footer_id: u32,
}

#[derive(Debug, Clone)]
struct WrittenComment {
    id: String,
    comment: AuthoredComment,
}

#[derive(Debug, Clone)]
struct WrittenNote {
    id: String,
    text: String,
}

impl Ctx {
    fn new() -> Self {
        Ctx {
            doc_rels: Vec::new(),
            media: Vec::new(),
            chart_parts: Vec::new(),
            chart_rels: Vec::new(),
            embedded_workbooks: Vec::new(),
            hf_parts: Vec::new(),
            next_rid: 1,
            has_list: false,
            has_heading: false,
            has_styles: false,
            has_even_header_footer: false,
            img_id: 0,
            chart_id: 0,
            drawing_id: 0,
            comment_id: 0,
            revision_id: 0,
            comments: Vec::new(),
            bookmark_id: 0,
            footnotes: Vec::new(),
            endnotes: Vec::new(),
            header_id: 0,
            footer_id: 0,
        }
    }

    fn add_rel(&mut self, rel_type: &str, target: &str, external: bool) -> String {
        let id = format!("rId{}", self.next_rid);
        self.next_rid += 1;
        self.doc_rels.push(Rel {
            id: id.clone(),
            rel_type: rel_type.to_string(),
            target: target.to_string(),
            external,
        });
        id
    }

    fn write_block(&mut self, out: &mut String, b: &Block) {
        match b {
            Block::Paragraph(p) => self.write_paragraph(out, p),
            Block::Table(t) => self.write_table(out, t),
            Block::Image(img) => {
                out.push_str("<w:p>");
                if img.bytes.is_some() {
                    self.write_image(out, img);
                }
                out.push_str("</w:p>");
            }
            Block::Chart(chart) => self.write_chart(out, chart),
            Block::PageBreak => out.push_str(r#"<w:p><w:r><w:br w:type="page"/></w:r></w:p>"#),
            Block::SectionBreak(setup) => self.write_section_break(out, setup),
        }
    }

    fn write_section_break(&mut self, out: &mut String, setup: &SectionSetup) {
        out.push_str("<w:p><w:pPr>");
        self.write_sect_pr(out, setup, true);
        out.push_str("</w:pPr></w:p>");
    }

    fn write_header_ref(&mut self, refs: &mut String, type_name: &str, blocks: &[Block]) {
        if blocks.is_empty() {
            return;
        }
        self.header_id += 1;
        let path = format!("word/header{}.xml", self.header_id);
        let target = format!("header{}.xml", self.header_id);
        let rid = self.add_rel(REL_HEADER, &target, false);
        self.hf_parts.push((
            path,
            CT_HEADER,
            hf_part("hdr", &render_hf_body(blocks, false)),
        ));
        refs.push_str(&format!(
            r#"<w:headerReference w:type="{type_name}" r:id="{rid}"/>"#
        ));
    }

    fn write_footer_ref(
        &mut self,
        refs: &mut String,
        type_name: &str,
        blocks: &[Block],
        page_numbers: bool,
    ) {
        if blocks.is_empty() && !page_numbers {
            return;
        }
        self.footer_id += 1;
        let path = format!("word/footer{}.xml", self.footer_id);
        let target = format!("footer{}.xml", self.footer_id);
        let rid = self.add_rel(REL_FOOTER, &target, false);
        self.hf_parts.push((
            path,
            CT_FOOTER,
            hf_part("ftr", &render_hf_body(blocks, page_numbers)),
        ));
        refs.push_str(&format!(
            r#"<w:footerReference w:type="{type_name}" r:id="{rid}"/>"#
        ));
    }

    fn write_sect_pr(&mut self, out: &mut String, setup: &SectionSetup, next_page: bool) {
        let mut refs = String::new();
        self.write_header_ref(&mut refs, "default", &setup.header);
        self.write_header_ref(&mut refs, "first", &setup.first_header);
        self.write_header_ref(&mut refs, "even", &setup.even_header);
        self.write_footer_ref(&mut refs, "default", &setup.footer, setup.page_numbers);
        self.write_footer_ref(&mut refs, "first", &setup.first_footer, false);
        self.write_footer_ref(&mut refs, "even", &setup.even_footer, false);

        let has_first_variant = !setup.first_header.is_empty() || !setup.first_footer.is_empty();
        let has_even_variant = !setup.even_header.is_empty() || !setup.even_footer.is_empty();
        if has_even_variant {
            self.has_even_header_footer = true;
        }

        let page = &setup.page;
        let (w, h) = (pt_twips(page.width_pt), pt_twips(page.height_pt));
        let orient = if page.landscape {
            " w:orient=\"landscape\""
        } else {
            ""
        };
        let (mt, mr, mb, ml) = (
            pt_twips(page.top()),
            pt_twips(page.right()),
            pt_twips(page.bottom()),
            pt_twips(page.left()),
        );
        let columns = setup
            .columns
            .map(|columns| format!(r#"<w:cols w:num="{}"/>"#, columns.max(1)))
            .unwrap_or_default();
        let text_direction = setup
            .text_direction
            .map(|direction| format!(r#"<w:textDirection w:val="{}"/>"#, direction.wml_value()))
            .unwrap_or_default();
        let doc_grid = doc_grid_xml(setup);
        let page_number_type = page_number_type_xml(setup);
        let start = if next_page {
            r#"<w:type w:val="nextPage"/>"#
        } else {
            ""
        };
        let title_pg = if setup.title_page || has_first_variant {
            "<w:titlePg/>"
        } else {
            ""
        };
        out.push_str(&format!(
            r#"<w:sectPr>{start}{refs}{title_pg}<w:pgSz w:w="{w}" w:h="{h}"{orient}/><w:pgMar w:top="{mt}" w:right="{mr}" w:bottom="{mb}" w:left="{ml}" w:header="708" w:footer="708" w:gutter="0"/>{text_direction}{page_number_type}{columns}{doc_grid}</w:sectPr>"#
        ));
    }

    fn write_paragraph(&mut self, out: &mut String, p: &Paragraph) {
        out.push_str("<w:p>");
        self.write_ppr(out, &p.props);
        for r in &p.runs {
            self.write_run(out, r);
        }
        out.push_str("</w:p>");
    }

    fn write_ppr(&mut self, out: &mut String, pr: &ParaProps) {
        let heading = pr.heading_level;
        // A heading suppresses list rendering — mirror the reader's precedence.
        let list = pr.list.as_ref().filter(|_| heading.is_none());
        let jc = match pr.align {
            Align::Left => None,
            Align::Center => Some("center"),
            Align::Right => Some("right"),
            Align::Justify => Some("both"),
        };
        let outline = heading.map(|h| h.saturating_sub(1));
        let generated_heading_style = pr.style_id.is_none() && heading.is_some();
        let style_id = pr
            .style_id
            .as_deref()
            .map(str::to_string)
            .or_else(|| heading.map(|h| format!("Heading{}", h.clamp(1, 6))));
        let sp = pr.spacing;
        let ind = pr.indent;
        let has_spacing = sp.before_pt.is_some() || sp.after_pt.is_some() || sp.line_pct.is_some();
        let has_indent = ind.left_pt.is_some()
            || ind.right_pt.is_some()
            || ind.first_line_pt.is_some()
            || ind.hanging_pt.is_some();
        if style_id.is_none()
            && list.is_none()
            && jc.is_none()
            && outline.is_none()
            && !has_spacing
            && !has_indent
            && pr.shading.is_none()
            && !pr.page_break_before
        {
            return;
        }
        out.push_str("<w:pPr>");
        // Schema order: pStyle, numPr, pageBreakBefore, shd, spacing, ind, jc, outlineLvl.
        if let Some(s) = &style_id {
            self.has_styles = true;
            if generated_heading_style {
                self.has_heading = true;
            }
            out.push_str(&format!(r#"<w:pStyle w:val="{}"/>"#, esc_attr(s)));
        }
        if let Some(li) = list {
            self.has_list = true;
            let num_id = if li.ordered { 1 } else { 2 };
            out.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{num_id}"/></w:numPr>"#,
                li.level
            ));
        }
        if pr.page_break_before {
            out.push_str("<w:pageBreakBefore/>");
        }
        if let Some(c) = pr.shading {
            out.push_str(&format!(
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                hex(c)
            ));
        }
        write_spacing(out, sp);
        write_indent(out, ind);
        if let Some(j) = jc {
            out.push_str(&format!(r#"<w:jc w:val="{j}"/>"#));
        }
        if let Some(o) = outline {
            out.push_str(&format!(r#"<w:outlineLvl w:val="{o}"/>"#));
        }
        out.push_str("</w:pPr>");
    }

    fn write_run(&mut self, out: &mut String, r: &crate::model::Run) {
        let comment_id = self.begin_comment(out, r.comment.as_ref());
        let deleted = matches!(
            r.revision.as_ref().map(|revision| revision.kind),
            Some(RevisionKind::Deletion)
        );
        let mut run_xml = String::new();
        match &r.field {
            FieldRole::Hyperlink { url } => {
                let rid = self.add_rel(REL_HYPERLINK, url, true);
                run_xml.push_str(&format!(r#"<w:hyperlink r:id="{rid}">"#));
                self.write_run_inner(&mut run_xml, r, deleted);
                run_xml.push_str("</w:hyperlink>");
            }
            FieldRole::Simple { instruction } => {
                let instruction = normalize_field_instruction(instruction);
                let dirty = if r.field_dirty {
                    r#" w:dirty="true""#
                } else {
                    ""
                };
                run_xml.push_str(&format!(
                    r#"<w:fldSimple w:instr=" {} "{dirty}>"#,
                    esc_attr(&instruction)
                ));
                self.write_run_inner(&mut run_xml, r, deleted);
                run_xml.push_str("</w:fldSimple>");
            }
            _ => self.write_run_inner(&mut run_xml, r, deleted),
        }
        let run_xml = self.content_control_wrapper(r.content_control.as_ref(), &run_xml);
        let run_xml = self.bookmark_wrapper(r.bookmark.as_deref(), &run_xml);
        self.write_revision_wrapper(out, r.revision.as_ref(), &run_xml);
        self.end_comment(out, comment_id);
        self.write_note_reference(out, r.note.as_ref());
    }

    fn begin_comment(
        &mut self,
        out: &mut String,
        comment: Option<&AuthoredComment>,
    ) -> Option<String> {
        let comment = comment.filter(|comment| !comment.text.is_empty())?;
        let id = self.comment_id.to_string();
        self.comment_id += 1;
        self.comments.push(WrittenComment {
            id: id.clone(),
            comment: comment.clone(),
        });
        out.push_str(&format!(r#"<w:commentRangeStart w:id="{id}"/>"#));
        Some(id)
    }

    fn end_comment(&mut self, out: &mut String, id: Option<String>) {
        if let Some(id) = id {
            out.push_str(&format!(
                r#"<w:commentRangeEnd w:id="{id}"/><w:r><w:commentReference w:id="{id}"/></w:r>"#
            ));
        }
    }

    fn write_note_reference(&mut self, out: &mut String, note: Option<&AuthoredNote>) {
        let Some(note) = note else {
            return;
        };
        let (tag, notes) = match note.kind {
            NoteKind::Footnote => ("footnoteReference", &mut self.footnotes),
            NoteKind::Endnote => ("endnoteReference", &mut self.endnotes),
        };
        let id = (notes.len() + 1).to_string();
        notes.push(WrittenNote {
            id: id.clone(),
            text: note.text.clone(),
        });
        out.push_str(&format!(r#"<w:r><w:{tag} w:id="{id}"/></w:r>"#));
    }

    fn bookmark_wrapper(&mut self, name: Option<&str>, run_xml: &str) -> String {
        let Some(name) = name.filter(|name| !name.is_empty()) else {
            return run_xml.to_string();
        };
        let id = self.bookmark_id;
        self.bookmark_id += 1;
        format!(
            r#"<w:bookmarkStart w:id="{id}" w:name="{}"/>{run_xml}<w:bookmarkEnd w:id="{id}"/>"#,
            esc_attr(name)
        )
    }

    fn write_revision_wrapper(
        &mut self,
        out: &mut String,
        revision: Option<&AuthoredRevision>,
        run_xml: &str,
    ) {
        let Some(revision) = revision else {
            out.push_str(run_xml);
            return;
        };
        let tag = match revision.kind {
            RevisionKind::Insertion => "ins",
            RevisionKind::Deletion => "del",
            _ => {
                out.push_str(run_xml);
                return;
            }
        };
        let id = self.revision_id;
        self.revision_id += 1;
        let mut attrs = format!(r#" w:id="{id}""#);
        if let Some(author) = revision.author.as_deref() {
            attrs.push_str(&format!(r#" w:author="{}""#, esc_attr(author)));
        }
        if let Some(date) = revision.date.as_deref() {
            attrs.push_str(&format!(r#" w:date="{}""#, esc_attr(date)));
        }
        out.push_str(&format!("<w:{tag}{attrs}>"));
        out.push_str(run_xml);
        out.push_str(&format!("</w:{tag}>"));
    }

    fn content_control_wrapper(
        &mut self,
        control: Option<&AuthoredContentControl>,
        run_xml: &str,
    ) -> String {
        let Some(control) = control else {
            return run_xml.to_string();
        };
        let mut xml = String::new();
        xml.push_str("<w:sdt><w:sdtPr>");
        if let Some(alias) = control.alias.as_deref().filter(|value| !value.is_empty()) {
            xml.push_str(&format!(r#"<w:alias w:val="{}"/>"#, esc_attr(alias)));
        }
        if let Some(tag) = control.tag.as_deref().filter(|value| !value.is_empty()) {
            xml.push_str(&format!(r#"<w:tag w:val="{}"/>"#, esc_attr(tag)));
        }
        if let (Some(xpath), Some(store_item_id)) = (
            control
                .data_binding_xpath
                .as_deref()
                .filter(|value| !value.is_empty()),
            control
                .data_binding_store_item_id
                .as_deref()
                .filter(|value| !value.is_empty()),
        ) {
            xml.push_str(&format!(
                r#"<w:dataBinding w:xpath="{}" w:storeItemID="{}"/>"#,
                esc_attr(xpath),
                esc_attr(store_item_id)
            ));
        }
        xml.push_str("</w:sdtPr><w:sdtContent>");
        xml.push_str(run_xml);
        xml.push_str("</w:sdtContent></w:sdt>");
        xml
    }

    fn write_run_inner(&mut self, out: &mut String, r: &crate::model::Run, deleted: bool) {
        if let Some(img) = &r.image {
            if img.bytes.is_some() {
                self.write_image(out, img);
                return;
            }
        }
        out.push_str("<w:r>");
        write_rpr(out, &r.props);
        if deleted {
            write_run_deleted_text(out, &r.text);
        } else {
            write_run_text(out, &r.text);
        }
        out.push_str("</w:r>");
    }

    fn write_image(&mut self, out: &mut String, img: &Image) {
        let Some(bytes) = img.bytes.clone() else {
            return;
        };
        let (ext, ct) = img_ext_ct(img.mime.as_deref());
        self.img_id += 1;
        let n = self.img_id;
        self.drawing_id += 1;
        let drawing_id = self.drawing_id;
        let target = format!("media/image{n}.{ext}");
        let rid = self.add_rel(REL_IMAGE, &target, false);
        self.media.push((format!("word/{target}"), bytes, ext, ct));
        // Extent (EMU) from the image's intrinsic pixels at 96 dpi (1px = 9525
        // EMU), clamped to the ~6in content width; falls back to 2in² if the
        // header had no dimensions.
        let (cx, cy) = image_extent_emu(img.width_px, img.height_px);
        let descr = img
            .alt
            .as_deref()
            .filter(|alt| !alt.is_empty())
            .map(|alt| format!(r#" descr="{}""#, esc_attr(alt)))
            .unwrap_or_default();
        let rotation = img
            .rotation_degrees
            .map(|degrees| format!(r#" rot="{}""#, i64::from(degrees.rem_euclid(360)) * 60_000))
            .unwrap_or_default();
        let graphic = format!(
            concat!(
                r#"<a:graphic><a:graphicData uri="{uri}"><pic:pic><pic:nvPicPr>"#,
                r#"<pic:cNvPr id="{n}" name="Image{n}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm{rotation}><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic>"#,
            ),
            cx = cx,
            cy = cy,
            n = n,
            rotation = rotation,
            uri = PIC_URI,
            rid = rid
        );
        if let Some((x_emu, y_emu)) = img.floating_offset_emu {
            out.push_str(&format!(
                concat!(
                    r#"<w:r><w:drawing><wp:anchor simplePos="0" relativeHeight="251659264" behindDoc="0" locked="0" layoutInCell="1" allowOverlap="1" distT="0" distB="0" distL="0" distR="0">"#,
                    r#"<wp:simplePos x="0" y="0"/>"#,
                    r#"<wp:positionH relativeFrom="page"><wp:posOffset>{x_emu}</wp:posOffset></wp:positionH>"#,
                    r#"<wp:positionV relativeFrom="page"><wp:posOffset>{y_emu}</wp:posOffset></wp:positionV>"#,
                    r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:effectExtent l="0" t="0" r="0" b="0"/>"#,
                    r#"<wp:wrapSquare wrapText="bothSides"/><wp:docPr id="{drawing_id}" name="Image{n}"{descr}/>"#,
                    r#"<wp:cNvGraphicFramePr><a:graphicFrameLocks noChangeAspect="1"/></wp:cNvGraphicFramePr>"#,
                    r#"{graphic}</wp:anchor></w:drawing></w:r>"#,
                ),
                cx = cx,
                cy = cy,
                x_emu = x_emu,
                y_emu = y_emu,
                n = n,
                drawing_id = drawing_id,
                descr = descr,
                graphic = graphic
            ));
        } else {
            out.push_str(&format!(
                concat!(
                    r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
                    r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{drawing_id}" name="Image{n}"{descr}/>"#,
                    r#"{graphic}</wp:inline></w:drawing></w:r>"#,
                ),
                cx = cx,
                cy = cy,
                n = n,
                drawing_id = drawing_id,
                descr = descr,
                graphic = graphic
            ));
        }
    }

    fn write_chart(&mut self, out: &mut String, chart: &Chart) {
        self.chart_id += 1;
        let chart_id = self.chart_id;
        self.drawing_id += 1;
        let drawing_id = self.drawing_id;
        let target = format!("charts/chart{chart_id}.xml");
        let rid = self.add_rel(REL_CHART, &target, false);
        let workbook_name = format!("Microsoft_Excel_Worksheet{chart_id}.xlsx");
        let workbook_rid = "rId1".to_string();
        self.chart_rels.push((
            format!("word/charts/_rels/chart{chart_id}.xml.rels"),
            vec![Rel {
                id: workbook_rid.clone(),
                rel_type: REL_PACKAGE.to_string(),
                target: format!("../embeddings/{workbook_name}"),
                external: false,
            }],
        ));
        self.embedded_workbooks.push((
            format!("word/embeddings/{workbook_name}"),
            chart_workbook_xlsx(chart),
        ));
        self.chart_parts.push((
            format!("word/{target}"),
            chart_xml(chart, chart_id, Some(&workbook_rid)).into_bytes(),
        ));

        let (cx, cy) = image_extent_emu(chart.width_px, chart.height_px);
        let descr = chart
            .alt
            .as_deref()
            .filter(|alt| !alt.is_empty())
            .map(|alt| format!(r#" descr="{}""#, esc_attr(alt)))
            .unwrap_or_default();
        out.push_str(&format!(
            concat!(
                r#"<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
                r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{drawing_id}" name="Chart{chart_id}"{descr}/>"#,
                r#"<a:graphic><a:graphicData uri="{uri}"><c:chart r:id="{rid}"/></a:graphicData></a:graphic>"#,
                r#"</wp:inline></w:drawing></w:r></w:p>"#,
            ),
            cx = cx,
            cy = cy,
            drawing_id = drawing_id,
            chart_id = chart_id,
            descr = descr,
            uri = C_NS,
            rid = rid
        ));
    }

    /// Cell content: at least one paragraph; a cell ending in a table needs a
    /// trailing empty paragraph (OOXML requires `w:tc` to end with `w:p`).
    fn write_cell_blocks(&mut self, out: &mut String, blocks: &[Block]) {
        if blocks.is_empty() {
            out.push_str("<w:p/>");
            return;
        }
        for b in blocks {
            self.write_block(out, b);
        }
        if matches!(blocks.last(), Some(Block::Table(_))) {
            out.push_str("<w:p/>");
        }
    }

    fn write_cell_margins(out: &mut String, margins: CellMargins) {
        out.push_str("<w:tcMar>");
        out.push_str(&format!(r#"<w:top w:w="{}" w:type="dxa"/>"#, margins.top));
        out.push_str(&format!(
            r#"<w:right w:w="{}" w:type="dxa"/>"#,
            margins.right
        ));
        out.push_str(&format!(
            r#"<w:bottom w:w="{}" w:type="dxa"/>"#,
            margins.bottom
        ));
        out.push_str(&format!(r#"<w:left w:w="{}" w:type="dxa"/>"#, margins.left));
        out.push_str("</w:tcMar>");
    }

    /// Write a table, reconstructing the full grid (re-inserting the `vMerge`
    /// continuation cells the reader dropped) so merges round-trip.
    fn write_table(&mut self, out: &mut String, t: &Table) {
        struct Active {
            col: usize,
            span: usize,
            rows_left: usize,
        }
        let mut active: Vec<Active> = Vec::new();
        let mut rows_xml = String::new();
        let mut ncols = 0usize;

        for (ri, row) in t.rows.iter().enumerate() {
            let is_header = ri < t.header_rows;
            let mut row_xml = String::new();
            let mut col = 0usize;
            let mut ci = 0usize;
            let mut carried: Vec<Active> = Vec::new();

            loop {
                if col >= MAX_TABLE_COLS {
                    break;
                }
                if let Some(pos) = active.iter().position(|a| a.col == col) {
                    let a = active.remove(pos);
                    row_xml.push_str("<w:tc><w:tcPr>");
                    if a.span > 1 {
                        row_xml.push_str(&format!(r#"<w:gridSpan w:val="{}"/>"#, a.span));
                    }
                    row_xml.push_str("<w:vMerge/></w:tcPr><w:p/></w:tc>");
                    col += a.span;
                    if a.rows_left > 1 {
                        carried.push(Active {
                            col: a.col,
                            span: a.span,
                            rows_left: a.rows_left - 1,
                        });
                    }
                    continue;
                }
                if ci < row.cells.len() {
                    let c = &row.cells[ci];
                    ci += 1;
                    let span = (c.col_span.max(1) as usize).min(MAX_TABLE_COLS);
                    let rs = (c.row_span.max(1) as usize).min(MAX_TABLE_COLS);
                    row_xml.push_str("<w:tc><w:tcPr>");
                    if let Some(p) = c.width_pct {
                        let w = (p.clamp(0.0, 1.0) * 5000.0).round() as i64;
                        row_xml.push_str(&format!(r#"<w:tcW w:w="{w}" w:type="pct"/>"#));
                    }
                    if span > 1 {
                        row_xml.push_str(&format!(r#"<w:gridSpan w:val="{span}"/>"#));
                    }
                    if rs > 1 {
                        row_xml.push_str(r#"<w:vMerge w:val="restart"/>"#);
                    }
                    match c.valign {
                        crate::model::VCell::Center => {
                            row_xml.push_str(r#"<w:vAlign w:val="center"/>"#)
                        }
                        crate::model::VCell::Bottom => {
                            row_xml.push_str(r#"<w:vAlign w:val="bottom"/>"#)
                        }
                        crate::model::VCell::Top => {}
                    }
                    if let Some(col) = c.shading {
                        row_xml.push_str(&format!(
                            r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                            hex(col)
                        ));
                    }
                    if let Some(margins) = c.margins {
                        Self::write_cell_margins(&mut row_xml, margins);
                    }
                    row_xml.push_str("</w:tcPr>");
                    self.write_cell_blocks(&mut row_xml, &c.blocks);
                    row_xml.push_str("</w:tc>");
                    if rs > 1 {
                        carried.push(Active {
                            col,
                            span,
                            rows_left: rs - 1,
                        });
                    }
                    col += span;
                    continue;
                }
                break;
            }
            ncols = ncols.max(col);
            active.extend(carried);
            active.sort_by_key(|a| a.col);

            rows_xml.push_str("<w:tr>");
            if is_header {
                rows_xml.push_str("<w:trPr><w:tblHeader/></w:trPr>");
            }
            rows_xml.push_str(&row_xml);
            rows_xml.push_str("</w:tr>");
        }

        let ncols = ncols.max(1);
        out.push_str("<w:tbl><w:tblPr>");
        if let Some(width_pct) = t.width_pct {
            let w = (width_pct.clamp(0.0, 1.0) * 5000.0).round() as i64;
            out.push_str(&format!(r#"<w:tblW w:w="{w}" w:type="pct"/>"#));
        } else {
            out.push_str(r#"<w:tblW w:w="0" w:type="auto"/>"#);
        }
        if let Some(indent) = t.indent_twips {
            out.push_str(&format!(r#"<w:tblInd w:w="{indent}" w:type="dxa"/>"#));
        }
        if let Some(align) = t.align {
            let val = match align {
                Align::Left => "left",
                Align::Center => "center",
                Align::Right => "right",
                Align::Justify => "both",
            };
            out.push_str(&format!(r#"<w:jc w:val="{val}"/>"#));
        }
        let border_color = t
            .border_color
            .map(hex)
            .unwrap_or_else(|| "auto".to_string());
        out.push_str(&format!(
            concat!(
                r#"<w:tblBorders>"#,
                r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"<w:bottom w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"<w:right w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"<w:insideV w:val="single" w:sz="4" w:space="0" w:color="{border_color}"/>"#,
                r#"</w:tblBorders>"#,
            ),
            border_color = border_color
        ));
        if t.fixed_layout {
            out.push_str(r#"<w:tblLayout w:type="fixed"/>"#);
        }
        out.push_str("</w:tblPr><w:tblGrid>");
        let colw = (9000 / ncols).max(1);
        for _ in 0..ncols {
            out.push_str(&format!(r#"<w:gridCol w:w="{colw}"/>"#));
        }
        out.push_str("</w:tblGrid>");
        out.push_str(&rows_xml);
        out.push_str("</w:tbl>");
    }
}

/// Write `<w:rPr>` toggles in schema order (b, i, strike, vanish, u). Free
/// function (no `Ctx` state needed).
fn write_rpr(out: &mut String, p: &CharProps) {
    let has = p.bold
        || p.italic
        || p.underline
        || p.strike
        || p.hidden
        || p.small_caps
        || p.caps
        || p.font.is_some()
        || p.size_half_pt.is_some()
        || p.color.is_some()
        || p.highlight.is_some()
        || p.vert_align != VertAlign::Baseline;
    if !has {
        return;
    }
    out.push_str("<w:rPr>");
    // Schema order: rFonts, b, i, smallCaps, strike, vanish, color, sz/szCs,
    // highlight, u, vertAlign.
    if let Some(f) = &p.font {
        let f = esc_attr(f);
        out.push_str(&format!(
            r#"<w:rFonts w:ascii="{f}" w:hAnsi="{f}" w:eastAsia="{f}" w:cs="{f}"/>"#
        ));
    }
    if p.bold {
        out.push_str("<w:b/>");
    }
    if p.italic {
        out.push_str("<w:i/>");
    }
    if p.small_caps {
        out.push_str("<w:smallCaps/>");
    }
    if p.caps {
        out.push_str("<w:caps/>");
    }
    if p.strike {
        out.push_str("<w:strike/>");
    }
    if p.hidden {
        out.push_str("<w:vanish/>");
    }
    if let Some(c) = p.color {
        out.push_str(&format!(r#"<w:color w:val="{}"/>"#, hex(c)));
    }
    if let Some(sz) = p.size_half_pt {
        out.push_str(&format!(r#"<w:sz w:val="{sz}"/><w:szCs w:val="{sz}"/>"#));
    }
    if let Some(h) = &p.highlight {
        out.push_str(&format!(r#"<w:highlight w:val="{}"/>"#, esc_attr(h)));
    }
    if p.underline {
        out.push_str(r#"<w:u w:val="single"/>"#);
    }
    match p.vert_align {
        VertAlign::Super => out.push_str(r#"<w:vertAlign w:val="superscript"/>"#),
        VertAlign::Sub => out.push_str(r#"<w:vertAlign w:val="subscript"/>"#),
        VertAlign::Baseline => {}
    }
    out.push_str("</w:rPr>");
}

/// Write a run's text, mapping `\t` → `<w:tab/>` and `\n` → `<w:br/>` (the dual
/// of the reader) and dropping XML-invalid control characters.
fn write_run_text(out: &mut String, text: &str) {
    write_run_text_element(out, text, "w:t");
}

fn write_run_deleted_text(out: &mut String, text: &str) {
    write_run_text_element(out, text, "w:delText");
}

fn write_run_text_element(out: &mut String, text: &str, tag: &str) {
    let mut buf = String::new();
    let flush = |out: &mut String, buf: &mut String| {
        if !buf.is_empty() {
            out.push_str(&format!(r#"<{tag} xml:space="preserve">"#));
            out.push_str(&esc_text(buf));
            out.push_str(&format!("</{tag}>"));
            buf.clear();
        }
    };
    for ch in text.chars() {
        match ch {
            '\t' => {
                flush(out, &mut buf);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(out, &mut buf);
                out.push_str("<w:br/>");
            }
            '\r' => {}
            c if (c as u32) < 0x20 => {}
            c => buf.push(c),
        }
    }
    flush(out, &mut buf);
}

fn normalize_field_instruction(instruction: &str) -> String {
    instruction.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extension + content type for an image MIME (reverse of the reader's
/// `mime_for`); unknown ⇒ PNG.
fn img_ext_ct(mime: Option<&str>) -> (&'static str, &'static str) {
    match mime {
        Some("image/jpeg") => ("jpg", "image/jpeg"),
        Some("image/gif") => ("gif", "image/gif"),
        Some("image/bmp") => ("bmp", "image/bmp"),
        Some("image/tiff") => ("tif", "image/tiff"),
        _ => ("png", "image/png"),
    }
}

/// The synthetic `word/numbering.xml`: numId 1 = ordered (decimal), numId 2 =
/// bullet, every level 0–8 declared so the reader resolves `ordered` exactly.
fn numbering_xml() -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(r#"<w:numbering xmlns:w="{W_NS}">"#));
    for (aid, fmt, txt) in [(0u8, "decimal", "%1."), (1u8, "bullet", "\u{2022}")] {
        s.push_str(&format!(r#"<w:abstractNum w:abstractNumId="{aid}">"#));
        for lvl in 0u8..9 {
            s.push_str(&format!(
                r#"<w:lvl w:ilvl="{lvl}"><w:numFmt w:val="{fmt}"/><w:lvlText w:val="{txt}"/><w:lvlJc w:val="left"/></w:lvl>"#
            ));
        }
        s.push_str("</w:abstractNum>");
    }
    s.push_str(r#"<w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>"#);
    s.push_str(r#"<w:num w:numId="2"><w:abstractNumId w:val="1"/></w:num>"#);
    s.push_str("</w:numbering>");
    s
}

fn comments_xml(comments: &[WrittenComment]) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(XML_DECL);
    let threaded = has_comment_replies(comments);
    if threaded {
        s.push_str(&format!(
            r#"<w:comments xmlns:w="{W_NS}" xmlns:w14="{W14_NS}" xmlns:mc="{MC_NS}" mc:Ignorable="w14">"#
        ));
    } else {
        s.push_str(&format!(r#"<w:comments xmlns:w="{W_NS}">"#));
    }
    for (index, comment) in comments.iter().enumerate() {
        let mut attrs = format!(r#" w:id="{}""#, esc_attr(&comment.id));
        if let Some(author) = comment.comment.author.as_deref() {
            attrs.push_str(&format!(r#" w:author="{}""#, esc_attr(author)));
        }
        if let Some(initials) = comment.comment.initials.as_deref() {
            attrs.push_str(&format!(r#" w:initials="{}""#, esc_attr(initials)));
        }
        if let Some(date) = comment.comment.date.as_deref() {
            attrs.push_str(&format!(r#" w:date="{}""#, esc_attr(date)));
        }
        if let Some(parent_id) = comment.comment.parent_comment_id.as_deref() {
            attrs.push_str(&format!(r#" w:parentId="{}""#, esc_attr(parent_id)));
        }
        let para_id = if threaded {
            format!(r#" w14:paraId="{}""#, comment_para_id(index))
        } else {
            String::new()
        };
        s.push_str(&format!(r#"<w:comment{attrs}><w:p{para_id}><w:r>"#));
        write_comment_text(&mut s, &comment.comment.text);
        s.push_str("</w:r></w:p></w:comment>");
    }
    s.push_str("</w:comments>");
    s.into_bytes()
}

fn comments_extended_xml(comments: &[WrittenComment]) -> Option<Vec<u8>> {
    if !has_comment_replies(comments) {
        return None;
    }
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(r#"<w15:commentsEx xmlns:w15="{W15_NS}">"#));
    for (index, comment) in comments.iter().enumerate() {
        let para_id = comment_para_id(index);
        let mut attrs = format!(r#" w15:paraId="{para_id}""#);
        if let Some(parent_id) = comment.comment.parent_comment_id.as_deref() {
            if let Some(parent_para_id) = comment_para_id_for_id(comments, parent_id) {
                attrs.push_str(&format!(r#" w15:paraIdParent="{parent_para_id}""#));
            }
        }
        s.push_str(&format!(r#"<w15:commentEx{attrs} w15:done="0"/>"#));
    }
    s.push_str("</w15:commentsEx>");
    Some(s.into_bytes())
}

fn has_comment_replies(comments: &[WrittenComment]) -> bool {
    comments
        .iter()
        .any(|comment| comment.comment.parent_comment_id.is_some())
}

fn comment_para_id(index: usize) -> String {
    format!("{:08X}", (index + 1).min(0x7FFF_FFFF))
}

fn comment_para_id_for_id(comments: &[WrittenComment], id: &str) -> Option<String> {
    comments
        .iter()
        .position(|comment| comment.id == id)
        .map(comment_para_id)
}

fn notes_xml(root: &str, item: &str, notes: &[WrittenNote]) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(r#"<w:{root} xmlns:w="{W_NS}">"#));
    s.push_str(&format!(
        concat!(
            r#"<w:{item} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{item}>"#,
            r#"<w:{item} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:{item}>"#
        ),
        item = item
    ));
    for note in notes {
        s.push_str(&format!(
            r#"<w:{item} w:id="{}"><w:p><w:r>"#,
            esc_attr(&note.id)
        ));
        write_run_text(&mut s, &note.text);
        s.push_str(&format!("</w:r></w:p></w:{item}>"));
    }
    s.push_str(&format!("</w:{root}>"));
    s.into_bytes()
}

fn write_comment_text(out: &mut String, text: &str) {
    let mut buf = String::new();
    let flush = |out: &mut String, buf: &mut String| {
        if !buf.is_empty() {
            let space = if needs_xml_space(buf) {
                r#" xml:space="preserve""#
            } else {
                ""
            };
            out.push_str(&format!(r#"<w:t{space}>{}</w:t>"#, esc_text(buf)));
            buf.clear();
        }
    };
    for ch in text.chars() {
        match ch {
            '\t' => {
                flush(out, &mut buf);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(out, &mut buf);
                out.push_str("<w:br/>");
            }
            '\r' => {}
            c if (c as u32) < 0x20 => {}
            c => buf.push(c),
        }
    }
    flush(out, &mut buf);
}

fn custom_properties_xml(properties: &std::collections::BTreeMap<String, String>) -> Vec<u8> {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(
        r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes">"#,
    );
    for (index, (name, value)) in properties.iter().enumerate() {
        s.push_str(&format!(
            r#"<property fmtid="{{D5CDD505-2E9C-101B-9397-08002B2CF9AE}}" pid="{}" name="{}"><vt:lpwstr>{}</vt:lpwstr></property>"#,
            index + 2,
            esc_attr(name),
            esc_text(value)
        ));
    }
    s.push_str("</Properties>");
    s.into_bytes()
}

fn custom_xml_item_props_xml(store_item_id: &str) -> Vec<u8> {
    format!(
        r#"{XML_DECL}<ds:datastoreItem ds:itemID="{}" xmlns:ds="http://schemas.openxmlformats.org/officeDocument/2006/customXml"><ds:schemaRefs/></ds:datastoreItem>"#,
        esc_attr(store_item_id)
    )
    .into_bytes()
}

fn chart_workbook_xlsx(chart: &Chart) -> Vec<u8> {
    let (sheet_xml, shared_strings_xml) = chart_workbook_sheet_xml(chart);
    let mut pkg = Package::new();
    pkg.add_part(
        "xl/workbook.xml",
        Some(CT_XLSX_WORKBOOK),
        xlsx_workbook_xml().into_bytes(),
    );
    pkg.add_part(
        "xl/worksheets/sheet1.xml",
        Some(CT_XLSX_WORKSHEET),
        sheet_xml.into_bytes(),
    );
    pkg.add_part(
        "xl/sharedStrings.xml",
        Some(CT_XLSX_SHARED_STRINGS),
        shared_strings_xml.into_bytes(),
    );
    pkg.add_part(
        "xl/styles.xml",
        Some(CT_XLSX_STYLES),
        xlsx_styles_xml().into_bytes(),
    );
    pkg.add_rels(
        "_rels/.rels",
        vec![Rel {
            id: "rId1".to_string(),
            rel_type: REL_OFFICE_DOCUMENT.to_string(),
            target: "xl/workbook.xml".to_string(),
            external: false,
        }],
    );
    pkg.add_rels(
        "xl/_rels/workbook.xml.rels",
        vec![
            Rel {
                id: "rId1".to_string(),
                rel_type: REL_XLSX_WORKSHEET.to_string(),
                target: "worksheets/sheet1.xml".to_string(),
                external: false,
            },
            Rel {
                id: "rId2".to_string(),
                rel_type: REL_XLSX_STYLES.to_string(),
                target: "styles.xml".to_string(),
                external: false,
            },
            Rel {
                id: "rId3".to_string(),
                rel_type: REL_XLSX_SHARED_STRINGS.to_string(),
                target: "sharedStrings.xml".to_string(),
                external: false,
            },
        ],
    );
    pkg.try_into_zip().unwrap_or_default()
}

fn xlsx_workbook_xml() -> String {
    format!(
        r#"{XML_DECL}<workbook xmlns="{S_NS}" xmlns:r="{R_NS}"><workbookPr/><sheets><sheet name="Chart Data" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    )
}

fn chart_workbook_sheet_xml(chart: &Chart) -> (String, String) {
    let mut shared_strings = Vec::new();
    let mut sheet = String::new();
    sheet.push_str(XML_DECL);
    sheet.push_str(&format!(
        r#"<worksheet xmlns="{S_NS}" xmlns:r="{R_NS}"><sheetData>"#
    ));

    sheet.push_str(r#"<row r="1">"#);
    let category_header = shared_string_index(&mut shared_strings, "Category");
    write_xlsx_shared_cell(&mut sheet, 0, 1, category_header);
    let mut next_col = 1usize;
    for series in &chart.series {
        let name = shared_string_index(&mut shared_strings, &series.name);
        write_xlsx_shared_cell(&mut sheet, next_col, 1, name);
        next_col += 1;
        if chart.kind == ChartKind::Bubble {
            let size_name =
                shared_string_index(&mut shared_strings, &format!("{} size", series.name));
            write_xlsx_shared_cell(&mut sheet, next_col, 1, size_name);
            next_col += 1;
        }
    }
    sheet.push_str("</row>");

    let row_count = chart
        .series
        .iter()
        .map(|series| series.values.len())
        .max()
        .unwrap_or(0)
        .max(chart.categories.len());
    for row_index in 0..row_count {
        let row_number = row_index + 2;
        sheet.push_str(&format!(r#"<row r="{row_number}">"#));
        let category = chart
            .categories
            .get(row_index)
            .map(String::as_str)
            .unwrap_or("");
        let category_index = shared_string_index(&mut shared_strings, category);
        write_xlsx_shared_cell(&mut sheet, 0, row_number, category_index);
        let mut next_col = 1usize;
        for series in &chart.series {
            let value = series.values.get(row_index).copied().unwrap_or(0.0);
            write_xlsx_number_cell(&mut sheet, next_col, row_number, value);
            next_col += 1;
            if chart.kind == ChartKind::Bubble {
                let bubble_size = series.bubble_sizes.get(row_index).copied().unwrap_or(1.0);
                write_xlsx_number_cell(&mut sheet, next_col, row_number, bubble_size);
                next_col += 1;
            }
        }
        sheet.push_str("</row>");
    }

    sheet.push_str("</sheetData></worksheet>");
    (sheet, shared_strings_xml(&shared_strings))
}

fn shared_string_index(shared_strings: &mut Vec<String>, value: &str) -> usize {
    if let Some(index) = shared_strings.iter().position(|existing| existing == value) {
        index
    } else {
        shared_strings.push(value.to_string());
        shared_strings.len() - 1
    }
}

fn write_xlsx_shared_cell(out: &mut String, col_index: usize, row_number: usize, value: usize) {
    let cell_ref = xlsx_cell_ref(col_index, row_number);
    out.push_str(&format!(r#"<c r="{cell_ref}" t="s"><v>{value}</v></c>"#));
}

fn write_xlsx_number_cell(out: &mut String, col_index: usize, row_number: usize, value: f64) {
    let cell_ref = xlsx_cell_ref(col_index, row_number);
    out.push_str(&format!(
        r#"<c r="{cell_ref}"><v>{}</v></c>"#,
        format_chart_number(value)
    ));
}

fn xlsx_cell_ref(col_index: usize, row_number: usize) -> String {
    format!("{}{}", xlsx_col_name(col_index), row_number)
}

fn xlsx_col_name(mut col_index: usize) -> String {
    col_index += 1;
    let mut name = Vec::new();
    while col_index > 0 {
        let rem = (col_index - 1) % 26;
        name.push((b'A' + rem as u8) as char);
        col_index = (col_index - 1) / 26;
    }
    name.iter().rev().collect()
}

fn shared_strings_xml(shared_strings: &[String]) -> String {
    let mut out = String::new();
    out.push_str(XML_DECL);
    out.push_str(&format!(
        r#"<sst xmlns="{S_NS}" count="{count}" uniqueCount="{count}">"#,
        count = shared_strings.len()
    ));
    for value in shared_strings {
        let space = if needs_xml_space(value) {
            r#" xml:space="preserve""#
        } else {
            ""
        };
        out.push_str(&format!(r#"<si><t{space}>{}</t></si>"#, esc_text(value)));
    }
    out.push_str("</sst>");
    out
}

fn xlsx_styles_xml() -> String {
    format!(
        concat!(
            r#"{xml_decl}<styleSheet xmlns="{s_ns}">"#,
            r#"<fonts count="1"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font></fonts>"#,
            r#"<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>"#,
            r#"<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>"#,
            r#"<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>"#,
            r#"<cellXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/></cellXfs>"#,
            r#"<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>"#,
            r#"<dxfs count="0"/><tableStyles count="0" defaultTableStyle="TableStyleMedium2" defaultPivotStyle="PivotStyleLight16"/>"#,
            r#"</styleSheet>"#
        ),
        xml_decl = XML_DECL,
        s_ns = S_NS
    )
}

fn chart_xml(chart: &Chart, chart_id: u32, workbook_rid: Option<&str>) -> String {
    let cat_axis_id = 10_000u32.saturating_add(chart_id.saturating_mul(2));
    let val_axis_id = cat_axis_id.saturating_add(1);
    let ser_axis_id = val_axis_id.saturating_add(1);
    let mut out = String::new();
    out.push_str(XML_DECL);
    out.push_str(&format!(
        r#"<c:chartSpace xmlns:c="{C_NS}" xmlns:a="{A_NS}" xmlns:r="{R_NS}">"#
    ));
    out.push_str(
        r#"<c:date1904 val="0"/><c:lang val="en-US"/><c:roundedCorners val="0"/><c:chart>"#,
    );
    if let Some(title) = chart.title.as_deref().filter(|title| !title.is_empty()) {
        write_chart_title(&mut out, title);
    }
    out.push_str("<c:plotArea><c:layout/>");
    match chart.kind {
        ChartKind::Bar => {
            write_bar_or_column_chart(&mut out, chart, cat_axis_id, val_axis_id, "bar")
        }
        ChartKind::Bar3D => {
            write_bar_or_column_3d_chart(&mut out, chart, cat_axis_id, val_axis_id, "bar")
        }
        ChartKind::Column => {
            write_bar_or_column_chart(&mut out, chart, cat_axis_id, val_axis_id, "col")
        }
        ChartKind::Column3D => {
            write_bar_or_column_3d_chart(&mut out, chart, cat_axis_id, val_axis_id, "col")
        }
        ChartKind::Line => write_line_chart(&mut out, chart, cat_axis_id, val_axis_id),
        ChartKind::Line3D => {
            write_line_3d_chart(&mut out, chart, cat_axis_id, val_axis_id, ser_axis_id)
        }
        ChartKind::Area => write_area_chart(&mut out, chart, cat_axis_id, val_axis_id),
        ChartKind::Area3D => {
            write_area_3d_chart(&mut out, chart, cat_axis_id, val_axis_id, ser_axis_id)
        }
        ChartKind::Radar => write_radar_chart(&mut out, chart, cat_axis_id, val_axis_id),
        ChartKind::Scatter => write_scatter_chart(&mut out, chart, cat_axis_id, val_axis_id),
        ChartKind::Bubble => write_bubble_chart(&mut out, chart, cat_axis_id, val_axis_id),
        ChartKind::Pie => write_pie_chart(&mut out, chart),
        ChartKind::Pie3D => write_pie_3d_chart(&mut out, chart),
        ChartKind::PieOfPie => write_of_pie_chart(&mut out, chart, "pie"),
        ChartKind::BarOfPie => write_of_pie_chart(&mut out, chart, "bar"),
        ChartKind::Doughnut => write_doughnut_chart(&mut out, chart),
        ChartKind::Surface => {
            write_surface_chart(&mut out, chart, cat_axis_id, val_axis_id, ser_axis_id)
        }
        ChartKind::Surface3D => {
            write_surface_3d_chart(&mut out, chart, cat_axis_id, val_axis_id, ser_axis_id)
        }
        ChartKind::Stock => write_stock_chart(&mut out, chart, cat_axis_id, val_axis_id),
    }
    match chart.kind {
        ChartKind::Pie
        | ChartKind::Pie3D
        | ChartKind::PieOfPie
        | ChartKind::BarOfPie
        | ChartKind::Doughnut => {}
        ChartKind::Scatter | ChartKind::Bubble => {
            write_scatter_axes(&mut out, cat_axis_id, val_axis_id)
        }
        ChartKind::Line3D | ChartKind::Area3D | ChartKind::Surface | ChartKind::Surface3D => {
            write_surface_axes(&mut out, cat_axis_id, val_axis_id, ser_axis_id)
        }
        _ => write_chart_axes(&mut out, chart.kind, cat_axis_id, val_axis_id),
    }
    out.push_str("</c:plotArea>");
    if chart.series.len() > 1 {
        out.push_str(
            r#"<c:legend><c:legendPos val="r"/><c:layout/><c:overlay val="0"/></c:legend>"#,
        );
    }
    out.push_str(r#"<c:plotVisOnly val="1"/><c:dispBlanksAs val="gap"/></c:chart>"#);
    if let Some(rid) = workbook_rid {
        out.push_str(&format!(
            r#"<c:externalData r:id="{}"><c:autoUpdate val="0"/></c:externalData>"#,
            esc_attr(rid)
        ));
    }
    out.push_str(r#"<c:printSettings><c:headerFooter/><c:pageMargins b="0.75" l="0.7" r="0.7" t="0.75" header="0.3" footer="0.3"/><c:pageSetup/></c:printSettings>"#);
    out.push_str("</c:chartSpace>");
    out
}

fn write_chart_title(out: &mut String, title: &str) {
    out.push_str("<c:title><c:tx><c:rich><a:bodyPr/><a:lstStyle/><a:p><a:r><a:t>");
    out.push_str(&esc_text(title));
    out.push_str("</a:t></a:r></a:p></c:rich></c:tx><c:layout/><c:overlay val=\"0\"/></c:title>");
}

fn write_bar_or_column_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    bar_dir: &str,
) {
    out.push_str(&format!(
        r#"<c:barChart><c:barDir val="{bar_dir}"/><c:grouping val="clustered"/><c:varyColors val="0"/>"#
    ));
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:barChart>"#
    ));
}

fn write_bar_or_column_3d_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    bar_dir: &str,
) {
    out.push_str(&format!(
        r#"<c:bar3DChart><c:barDir val="{bar_dir}"/><c:grouping val="clustered"/><c:varyColors val="0"/>"#
    ));
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    let shape = chart_shape_value(chart.shape);
    out.push_str(&format!(
        r#"<c:gapWidth val="150"/><c:gapDepth val="150"/><c:shape val="{shape}"/><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:bar3DChart>"#
    ));
}

fn chart_shape_value(shape: ChartShape) -> &'static str {
    match shape {
        ChartShape::Box => "box",
        ChartShape::Cylinder => "cylinder",
        ChartShape::Cone => "cone",
        ChartShape::ConeToMax => "coneToMax",
        ChartShape::Pyramid => "pyramid",
        ChartShape::PyramidToMax => "pyramidToMax",
    }
}

fn write_line_chart(out: &mut String, chart: &Chart, cat_axis_id: u32, val_axis_id: u32) {
    out.push_str(r#"<c:lineChart><c:grouping val="standard"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx><c:marker><c:symbol val="circle"/></c:marker>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:lineChart>"#
    ));
}

fn write_line_3d_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    ser_axis_id: u32,
) {
    out.push_str(r#"<c:line3DChart><c:grouping val="standard"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx><c:marker><c:symbol val="circle"/></c:marker>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:gapDepth val="150"/><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/><c:axId val="{ser_axis_id}"/></c:line3DChart>"#
    ));
}

fn write_area_chart(out: &mut String, chart: &Chart, cat_axis_id: u32, val_axis_id: u32) {
    out.push_str(r#"<c:areaChart><c:grouping val="standard"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:areaChart>"#
    ));
}

fn write_area_3d_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    ser_axis_id: u32,
) {
    out.push_str(r#"<c:area3DChart><c:grouping val="standard"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:gapDepth val="150"/><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/><c:axId val="{ser_axis_id}"/></c:area3DChart>"#
    ));
}

fn write_radar_chart(out: &mut String, chart: &Chart, cat_axis_id: u32, val_axis_id: u32) {
    out.push_str(r#"<c:radarChart><c:radarStyle val="standard"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx><c:marker><c:symbol val="circle"/></c:marker>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:radarChart>"#
    ));
}

fn write_scatter_chart(out: &mut String, chart: &Chart, x_axis_id: u32, y_axis_id: u32) {
    out.push_str(r#"<c:scatterChart><c:scatterStyle val="lineMarker"/><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx><c:marker><c:symbol val="circle"/></c:marker>"#,
            esc_text(&series.name)
        ));
        write_chart_x_values(out, series.values.len());
        write_chart_y_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:axId val="{x_axis_id}"/><c:axId val="{y_axis_id}"/></c:scatterChart>"#
    ));
}

fn write_bubble_chart(out: &mut String, chart: &Chart, x_axis_id: u32, y_axis_id: u32) {
    out.push_str(r#"<c:bubbleChart><c:varyColors val="0"/>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_x_values(out, series.values.len());
        write_chart_y_values(out, &series.values);
        write_chart_bubble_sizes(out, series, series.values.len());
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:bubbleScale val="100"/><c:showNegBubbles val="0"/><c:axId val="{x_axis_id}"/><c:axId val="{y_axis_id}"/></c:bubbleChart>"#
    ));
}

fn write_pie_chart(out: &mut String, chart: &Chart) {
    out.push_str(r#"<c:pieChart><c:varyColors val="1"/>"#);
    for (index, series) in chart.series.iter().take(1).enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(r#"<c:firstSliceAng val="0"/></c:pieChart>"#);
}

fn write_pie_3d_chart(out: &mut String, chart: &Chart) {
    out.push_str(r#"<c:pie3DChart><c:varyColors val="1"/>"#);
    for (index, series) in chart.series.iter().take(1).enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(r#"<c:firstSliceAng val="0"/></c:pie3DChart>"#);
}

fn write_of_pie_chart(out: &mut String, chart: &Chart, of_pie_type: &str) {
    out.push_str(&format!(
        r#"<c:ofPieChart><c:ofPieType val="{of_pie_type}"/><c:varyColors val="1"/>"#
    ));
    for (index, series) in chart.series.iter().take(1).enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(
        r#"<c:gapWidth val="150"/><c:splitType val="auto"/><c:secondPieSize val="75"/><c:serLines/></c:ofPieChart>"#,
    );
}

fn write_doughnut_chart(out: &mut String, chart: &Chart) {
    out.push_str(r#"<c:doughnutChart><c:varyColors val="1"/>"#);
    for (index, series) in chart.series.iter().take(1).enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(r#"<c:firstSliceAng val="0"/><c:holeSize val="50"/></c:doughnutChart>"#);
}

fn write_surface_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    ser_axis_id: u32,
) {
    let wireframe = u8::from(chart.wireframe);
    out.push_str(&format!(
        r#"<c:surfaceChart><c:wireframe val="{wireframe}"/>"#
    ));
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:bandFmts/><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/><c:axId val="{ser_axis_id}"/></c:surfaceChart>"#
    ));
}

fn write_surface_3d_chart(
    out: &mut String,
    chart: &Chart,
    cat_axis_id: u32,
    val_axis_id: u32,
    ser_axis_id: u32,
) {
    let wireframe = u8::from(chart.wireframe);
    out.push_str(&format!(
        r#"<c:surface3DChart><c:wireframe val="{wireframe}"/>"#
    ));
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:bandFmts/><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/><c:axId val="{ser_axis_id}"/></c:surface3DChart>"#
    ));
}

fn write_stock_chart(out: &mut String, chart: &Chart, cat_axis_id: u32, val_axis_id: u32) {
    out.push_str(r#"<c:stockChart>"#);
    for (index, series) in chart.series.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:ser><c:idx val="{index}"/><c:order val="{index}"/><c:tx><c:v>{}</c:v></c:tx>"#,
            esc_text(&series.name)
        ));
        write_chart_categories(out, &chart.categories);
        write_chart_values(out, &series.values);
        out.push_str("</c:ser>");
    }
    out.push_str(&format!(
        r#"<c:hiLowLines/><c:upDownBars><c:gapWidth val="150"/></c:upDownBars><c:axId val="{cat_axis_id}"/><c:axId val="{val_axis_id}"/></c:stockChart>"#
    ));
}

fn write_chart_categories(out: &mut String, categories: &[String]) {
    out.push_str(&format!(
        r#"<c:cat><c:strLit><c:ptCount val="{}"/>"#,
        categories.len()
    ));
    for (index, category) in categories.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{}</c:v></c:pt>"#,
            esc_text(category)
        ));
    }
    out.push_str("</c:strLit></c:cat>");
}

fn write_chart_values(out: &mut String, values: &[f64]) {
    out.push_str(&format!(
        r#"<c:val><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#,
        values.len()
    ));
    for (index, value) in values.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{}</c:v></c:pt>"#,
            format_chart_number(*value)
        ));
    }
    out.push_str("</c:numLit></c:val>");
}

fn write_chart_x_values(out: &mut String, count: usize) {
    out.push_str(&format!(
        r#"<c:xVal><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="{count}"/>"#
    ));
    for index in 0..count {
        out.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{}</c:v></c:pt>"#,
            format_chart_number((index + 1) as f64)
        ));
    }
    out.push_str("</c:numLit></c:xVal>");
}

fn write_chart_y_values(out: &mut String, values: &[f64]) {
    out.push_str(&format!(
        r#"<c:yVal><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="{}"/>"#,
        values.len()
    ));
    for (index, value) in values.iter().enumerate() {
        out.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{}</c:v></c:pt>"#,
            format_chart_number(*value)
        ));
    }
    out.push_str("</c:numLit></c:yVal>");
}

fn write_chart_bubble_sizes(out: &mut String, series: &ChartSeries, count: usize) {
    out.push_str(&format!(
        r#"<c:bubbleSize><c:numLit><c:formatCode>General</c:formatCode><c:ptCount val="{count}"/>"#
    ));
    for index in 0..count {
        let size = series.bubble_sizes.get(index).copied().unwrap_or(1.0);
        out.push_str(&format!(
            r#"<c:pt idx="{index}"><c:v>{}</c:v></c:pt>"#,
            format_chart_number(size)
        ));
    }
    out.push_str("</c:numLit></c:bubbleSize>");
}

fn format_chart_number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "0".to_string()
    }
}

fn write_chart_axes(out: &mut String, kind: ChartKind, cat_axis_id: u32, val_axis_id: u32) {
    let (cat_pos, val_pos) = match kind {
        ChartKind::Bar | ChartKind::Bar3D => ("l", "b"),
        ChartKind::Column
        | ChartKind::Column3D
        | ChartKind::Line
        | ChartKind::Line3D
        | ChartKind::Area
        | ChartKind::Area3D
        | ChartKind::Radar => ("b", "l"),
        ChartKind::Scatter
        | ChartKind::Bubble
        | ChartKind::Pie
        | ChartKind::Pie3D
        | ChartKind::PieOfPie
        | ChartKind::BarOfPie
        | ChartKind::Doughnut
        | ChartKind::Surface
        | ChartKind::Surface3D
        | ChartKind::Stock => ("b", "l"),
    };
    out.push_str(&format!(
        concat!(
            r#"<c:catAx><c:axId val="{cat_axis_id}"/>"#,
            r#"<c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>"#,
            r#"<c:axPos val="{cat_pos}"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/>"#,
            r#"<c:tickLblPos val="nextTo"/><c:crossAx val="{val_axis_id}"/>"#,
            r#"<c:crosses val="autoZero"/><c:auto val="1"/><c:lblAlgn val="ctr"/>"#,
            r#"<c:lblOffset val="100"/></c:catAx>"#
        ),
        cat_axis_id = cat_axis_id,
        val_axis_id = val_axis_id,
        cat_pos = cat_pos
    ));
    out.push_str(&format!(
        concat!(
            r#"<c:valAx><c:axId val="{val_axis_id}"/>"#,
            r#"<c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>"#,
            r#"<c:axPos val="{val_pos}"/><c:majorGridlines/><c:numFmt formatCode="General" sourceLinked="1"/>"#,
            r#"<c:majorTickMark val="out"/><c:minorTickMark val="none"/>"#,
            r#"<c:tickLblPos val="nextTo"/><c:crossAx val="{cat_axis_id}"/>"#,
            r#"<c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#
        ),
        cat_axis_id = cat_axis_id,
        val_axis_id = val_axis_id,
        val_pos = val_pos
    ));
}

fn write_surface_axes(out: &mut String, cat_axis_id: u32, val_axis_id: u32, ser_axis_id: u32) {
    write_chart_axes(out, ChartKind::Surface, cat_axis_id, val_axis_id);
    out.push_str(&format!(
        concat!(
            r#"<c:serAx><c:axId val="{ser_axis_id}"/>"#,
            r#"<c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>"#,
            r#"<c:axPos val="r"/><c:majorTickMark val="none"/><c:minorTickMark val="none"/>"#,
            r#"<c:tickLblPos val="nextTo"/><c:crossAx val="{val_axis_id}"/>"#,
            r#"<c:crosses val="autoZero"/></c:serAx>"#
        ),
        ser_axis_id = ser_axis_id,
        val_axis_id = val_axis_id
    ));
}

fn write_scatter_axes(out: &mut String, x_axis_id: u32, y_axis_id: u32) {
    out.push_str(&format!(
        concat!(
            r#"<c:valAx><c:axId val="{x_axis_id}"/>"#,
            r#"<c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>"#,
            r#"<c:axPos val="b"/><c:numFmt formatCode="General" sourceLinked="1"/>"#,
            r#"<c:majorTickMark val="out"/><c:minorTickMark val="none"/>"#,
            r#"<c:tickLblPos val="nextTo"/><c:crossAx val="{y_axis_id}"/>"#,
            r#"<c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#
        ),
        x_axis_id = x_axis_id,
        y_axis_id = y_axis_id
    ));
    out.push_str(&format!(
        concat!(
            r#"<c:valAx><c:axId val="{y_axis_id}"/>"#,
            r#"<c:scaling><c:orientation val="minMax"/></c:scaling><c:delete val="0"/>"#,
            r#"<c:axPos val="l"/><c:majorGridlines/><c:numFmt formatCode="General" sourceLinked="1"/>"#,
            r#"<c:majorTickMark val="out"/><c:minorTickMark val="none"/>"#,
            r#"<c:tickLblPos val="nextTo"/><c:crossAx val="{x_axis_id}"/>"#,
            r#"<c:crosses val="autoZero"/><c:crossBetween val="between"/></c:valAx>"#
        ),
        x_axis_id = x_axis_id,
        y_axis_id = y_axis_id
    ));
}

fn needs_xml_space(text: &str) -> bool {
    text != text.trim_matches([' ', '\t', '\n', '\r'])
}

/// Serialize a [`crate::DocModel`] to `.docx` bytes.
/// Infallible generator (the original contract): yields an empty buffer on the
/// unreachable in-memory ZIP error rather than panicking.
pub(crate) fn to_docx(model: &crate::DocModel) -> Vec<u8> {
    try_to_docx(model).unwrap_or_default()
}

/// Fallible generator — used by the public `try_write_docx` so a serialization
/// failure surfaces instead of becoming silent empty bytes.
pub(crate) fn try_to_docx(model: &crate::DocModel) -> crate::Result<Vec<u8>> {
    let br = render_body(model);

    let mut pkg = Package::new();
    pkg.add_part("word/document.xml", Some(CT_DOCUMENT), br.document_xml);
    for (path, ct, bytes) in br.hf_parts {
        pkg.add_part(&path, Some(ct), bytes);
    }
    if let Some(comments) = br.comments_xml {
        pkg.add_part("word/comments.xml", Some(CT_COMMENTS), comments);
    }
    if let Some(comments_ext) = br.comments_ext_xml {
        pkg.add_part(
            "word/commentsExtended.xml",
            Some(CT_COMMENTS_EXT),
            comments_ext,
        );
    }
    if let Some(footnotes) = br.footnotes_xml {
        pkg.add_part("word/footnotes.xml", Some(CT_FOOTNOTES), footnotes);
    }
    if let Some(endnotes) = br.endnotes_xml {
        pkg.add_part("word/endnotes.xml", Some(CT_ENDNOTES), endnotes);
    }
    if !model.custom_properties.is_empty() {
        pkg.add_part(
            "docProps/custom.xml",
            Some(CT_CUSTOM_PROPERTIES),
            custom_properties_xml(&model.custom_properties),
        );
    }
    for (index, item) in model.custom_xml_items.iter().enumerate() {
        let n = index + 1;
        pkg.add_part(
            &format!("customXml/item{n}.xml"),
            Some(CT_XML),
            item.xml.as_bytes().to_vec(),
        );
        pkg.add_part(
            &format!("customXml/itemProps{n}.xml"),
            Some(CT_CUSTOM_XML_PROPERTIES),
            custom_xml_item_props_xml(&item.store_item_id),
        );
        pkg.add_rels(
            &format!("customXml/_rels/item{n}.xml.rels"),
            vec![Rel {
                id: "rId1".to_string(),
                rel_type: REL_CUSTOM_XML_PROPERTIES.to_string(),
                target: format!("itemProps{n}.xml"),
                external: false,
            }],
        );
    }
    if br.has_list {
        pkg.add_part(
            "word/numbering.xml",
            Some(CT_NUMBERING),
            numbering_xml().into_bytes(),
        );
    }
    if br.has_styles {
        pkg.add_part(
            "word/styles.xml",
            Some(CT_STYLES),
            styles_xml(&model.setup.styles, br.has_heading).into_bytes(),
        );
    }
    if br.has_even_header_footer {
        pkg.add_part(
            "word/settings.xml",
            Some(CT_SETTINGS),
            settings_xml(true).into_bytes(),
        );
    }
    for (path, bytes, ext, ct) in br.media {
        pkg.add_default(ext, ct);
        pkg.add_part(&path, None, bytes);
    }
    for (path, bytes) in br.chart_parts {
        pkg.add_part(&path, Some(CT_CHART), bytes);
    }
    for (path, bytes) in br.embedded_workbooks {
        pkg.add_part(&path, Some(CT_EMBEDDED_XLSX), bytes);
    }
    for (path, rels) in br.chart_rels {
        pkg.add_rels(&path, rels);
    }

    if !br.doc_rels.is_empty() {
        pkg.add_rels("word/_rels/document.xml.rels", br.doc_rels);
    }
    let mut root_rels = vec![Rel {
        id: "rId1".to_string(),
        rel_type: REL_OFFICE_DOCUMENT.to_string(),
        target: "word/document.xml".to_string(),
        external: false,
    }];
    if !model.custom_properties.is_empty() {
        root_rels.push(Rel {
            id: "rId2".to_string(),
            rel_type: REL_CUSTOM_PROPERTIES.to_string(),
            target: "docProps/custom.xml".to_string(),
            external: false,
        });
    }
    pkg.add_rels("_rels/.rels", root_rels);

    pkg.try_into_zip()
        .map_err(|e| crate::Error::Docx(format!("docx serialize: {e}")))
}

/// The body half of a `.docx`: `word/document.xml` plus everything it references —
/// produced from the model by the from-scratch generator ([`try_to_docx`]).
pub(crate) struct BodyRender {
    /// Serialized `word/document.xml`.
    pub document_xml: Vec<u8>,
    /// Relationships the body references (hyperlinks, images, header/footer) plus
    /// the `styles.xml`/`numbering.xml` type-links, with `rId`s minted from 1.
    pub doc_rels: Vec<Rel>,
    /// `(part path, content-type, bytes)` for any header/footer parts.
    pub hf_parts: Vec<(String, &'static str, Vec<u8>)>,
    /// Serialized comments part, if authored comments were emitted.
    pub comments_xml: Option<Vec<u8>>,
    /// Serialized commentsExtended part, if authored comment replies were emitted.
    pub comments_ext_xml: Option<Vec<u8>>,
    /// Serialized footnotes part, if authored footnotes were emitted.
    pub footnotes_xml: Option<Vec<u8>>,
    /// Serialized endnotes part, if authored endnotes were emitted.
    pub endnotes_xml: Option<Vec<u8>>,
    /// `(part path, bytes, extension, content-type)` for inline/block images.
    pub media: Vec<(String, Vec<u8>, &'static str, &'static str)>,
    /// `(part path, bytes)` for authored chart parts.
    pub chart_parts: Vec<(String, Vec<u8>)>,
    /// `(rels path, relationships)` for authored chart package relationships.
    pub chart_rels: Vec<(String, Vec<Rel>)>,
    /// `(part path, bytes)` for embedded XLSX chart data workbooks.
    pub embedded_workbooks: Vec<(String, Vec<u8>)>,
    pub has_list: bool,
    pub has_styles: bool,
    pub has_heading: bool,
    pub has_even_header_footer: bool,
}

/// Render the body parts from the model. List items reference the synthetic
/// `numbering.xml` (numId 1 = ordered, 2 = bullet). Self-contained: the returned
/// `doc_rels` already include the `numbering`/`styles` type-links.
fn render_body(model: &crate::DocModel) -> BodyRender {
    let mut ctx = Ctx::new();
    let mut body = String::new();
    for b in &model.blocks {
        ctx.write_block(&mut body, b);
    }

    // word/document.xml
    let mut doc = String::new();
    doc.push_str(XML_DECL);
    doc.push_str(&format!(
        r#"<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}" xmlns:wp="{WP_NS}" xmlns:a="{A_NS}" xmlns:c="{C_NS}" xmlns:pic="{PIC_NS}"><w:body>"#
    ));
    doc.push_str(&body);

    // Final section properties describe the last section in the body. Earlier
    // section breaks were emitted while folding blocks.
    ctx.write_sect_pr(&mut doc, &SectionSetup::from(&model.setup), false);
    let comments_xml = if ctx.comments.is_empty() {
        None
    } else {
        let rid = format!("rId{}", ctx.next_rid);
        ctx.next_rid += 1;
        ctx.doc_rels.push(Rel {
            id: rid,
            rel_type: REL_COMMENTS.to_string(),
            target: "comments.xml".to_string(),
            external: false,
        });
        Some(comments_xml(&ctx.comments))
    };
    let comments_ext_xml = comments_extended_xml(&ctx.comments);
    if comments_ext_xml.is_some() {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_COMMENTS_EXT.to_string(),
            target: "commentsExtended.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }
    let footnotes_xml = if ctx.footnotes.is_empty() {
        None
    } else {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_FOOTNOTES.to_string(),
            target: "footnotes.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
        Some(notes_xml("footnotes", "footnote", &ctx.footnotes))
    };
    let endnotes_xml = if ctx.endnotes.is_empty() {
        None
    } else {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_ENDNOTES.to_string(),
            target: "endnotes.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
        Some(notes_xml("endnotes", "endnote", &ctx.endnotes))
    };
    doc.push_str("</w:body></w:document>");

    // Type-link rels for the styles/numbering parts (read-by-path doesn't need
    // them, but strict consumers like Word expect the relationship to exist). Minted
    // after the hf rels so ids match the previous single-pass writer exactly.
    if ctx.has_list {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_NUMBERING.to_string(),
            target: "numbering.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }
    let has_styles = ctx.has_styles || !model.setup.styles.is_empty();
    if has_styles {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_STYLES.to_string(),
            target: "styles.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }
    if ctx.has_even_header_footer {
        ctx.doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_SETTINGS.to_string(),
            target: "settings.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }

    BodyRender {
        document_xml: doc.into_bytes(),
        doc_rels: ctx.doc_rels,
        hf_parts: ctx.hf_parts,
        comments_xml,
        comments_ext_xml,
        footnotes_xml,
        endnotes_xml,
        media: ctx.media,
        chart_parts: ctx.chart_parts,
        chart_rels: ctx.chart_rels,
        embedded_workbooks: ctx.embedded_workbooks,
        has_list: ctx.has_list,
        has_styles,
        has_heading: ctx.has_heading,
        has_even_header_footer: ctx.has_even_header_footer,
    }
}

#[cfg(test)]
mod tests {
    use crate::model::{
        Align, Block, Cell, CharProps, DocModel, FieldRole, Image, ListInfo, ParaProps, Paragraph,
        Row, Run, Table,
    };
    use crate::Document;

    fn para(text: &str) -> Paragraph {
        Paragraph {
            props: ParaProps::default(),
            runs: vec![Run {
                text: text.to_string(),
                ..Run::default()
            }],
        }
    }

    fn cell(text: &str) -> Cell {
        Cell {
            blocks: vec![Block::Paragraph(para(text))],
            ..Cell::default()
        }
    }

    /// Build a representative model, write it to `.docx`, read it back, and assert
    /// the structure survives the round-trip.
    #[test]
    fn round_trips_structure_through_docx() {
        let heading = Paragraph {
            props: ParaProps {
                heading_level: Some(2),
                outline_level: Some(1),
                align: Align::Center,
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "제목 둘".to_string(),
                ..Run::default()
            }],
        };
        let emphasized = Paragraph {
            props: ParaProps::default(),
            runs: vec![
                Run {
                    text: "굵게".to_string(),
                    props: CharProps {
                        bold: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
                Run {
                    text: " 보통 ".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "기울임".to_string(),
                    props: CharProps {
                        italic: true,
                        ..CharProps::default()
                    },
                    ..Run::default()
                },
            ],
        };
        let ordered = Paragraph {
            props: ParaProps {
                list: Some(ListInfo {
                    level: 0,
                    ordered: true,
                    label: String::new(),
                }),
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "첫째 항목".to_string(),
                ..Run::default()
            }],
        };
        let bullet = Paragraph {
            props: ParaProps {
                list: Some(ListInfo {
                    level: 0,
                    ordered: false,
                    label: String::new(),
                }),
                ..ParaProps::default()
            },
            runs: vec![Run {
                text: "글머리 항목".to_string(),
                ..Run::default()
            }],
        };
        let link = Paragraph {
            props: ParaProps::default(),
            runs: vec![Run {
                text: "프로젝트 홈".to_string(),
                field: FieldRole::Hyperlink {
                    url: "https://example.com/".to_string(),
                },
                ..Run::default()
            }],
        };
        // 2x2 table: header row, a colspan-2 owner that vertically merges down.
        let table = Table {
            rows: vec![
                Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(para("머리글"))],
                        col_span: 2,
                        row_span: 2,
                        is_header: true,
                        ..Default::default()
                    }],
                },
                Row {
                    cells: vec![cell("a"), cell("b")],
                },
            ],
            header_rows: 1,
            ..Default::default()
        };
        // A genuinely valid 2×3 PNG (sig + IHDR + IDAT + IEND, correct CRCs) so the
        // round-trip proves a real, Office-openable image part — not just self-readable.
        let png = vec![
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00,
            0x00, 0x36, 0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78,
            0xDA, 0x63, 0x60, 0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23,
            0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
        ];
        let image = Block::Image(Image {
            alt: None,
            bytes: Some(png.clone()),
            mime: Some("image/png".to_string()),
            ..Default::default()
        });

        let model = DocModel {
            blocks: vec![
                Block::Paragraph(heading),
                Block::Paragraph(emphasized),
                Block::Paragraph(ordered),
                Block::Paragraph(bullet),
                Block::Paragraph(link),
                Block::Table(table),
                image,
            ],
            ..DocModel::default()
        };

        let bytes = super::to_docx(&model);
        let doc = Document::open(&bytes).expect("written .docx must reopen");
        let m2 = doc.model();

        // Heading.
        let Block::Paragraph(h) = &m2.blocks[0] else {
            panic!("expected heading paragraph, got {:?}", m2.blocks[0]);
        };
        assert_eq!(h.props.heading_level, Some(2));
        assert_eq!(h.props.align, Align::Center);
        assert_eq!(h.text(), "제목 둘");

        // Emphasis runs.
        let Block::Paragraph(e) = &m2.blocks[1] else {
            panic!("para");
        };
        assert_eq!(e.text(), "굵게 보통 기울임");
        assert!(e.runs.iter().any(|r| r.props.bold && r.text == "굵게"));
        assert!(e.runs.iter().any(|r| r.props.italic && r.text == "기울임"));

        // Lists.
        let Block::Paragraph(o) = &m2.blocks[2] else {
            panic!("para");
        };
        assert_eq!(o.props.list.as_ref().map(|l| l.ordered), Some(true));
        let Block::Paragraph(b) = &m2.blocks[3] else {
            panic!("para");
        };
        assert_eq!(b.props.list.as_ref().map(|l| l.ordered), Some(false));

        // Hyperlink.
        let Block::Paragraph(l) = &m2.blocks[4] else {
            panic!("para");
        };
        assert!(matches!(
            l.runs.iter().find(|r| r.text == "프로젝트 홈").map(|r| &r.field),
            Some(FieldRole::Hyperlink { url }) if url == "https://example.com/"
        ));

        // Table with merges.
        let Block::Table(t) = &m2.blocks[5] else {
            panic!("expected table, got {:?}", m2.blocks[5]);
        };
        assert_eq!(t.header_rows, 1);
        assert_eq!(t.rows[0].cells[0].col_span, 2);
        assert_eq!(t.rows[0].cells[0].row_span, 2);
        assert_eq!(t.rows[0].cells[0].text(), "머리글");
        assert!(t.rows[0].cells[0].is_header);
        // Row 1's continuation cell was dropped, leaving the two body cells.
        assert_eq!(t.rows[1].cells.len(), 2);
        assert_eq!(t.rows[1].cells[1].text(), "b");

        // Image survives.
        let imgs = doc.images();
        assert_eq!(imgs.len(), 1);
        assert_eq!(imgs[0].bytes.as_deref(), Some(&png[..]));
    }

    #[test]
    fn round_trips_rich_char_and_para_formatting() {
        use crate::model::{Color, Indent, Spacing};
        let run = Run {
            text: "빨강굵게".to_string(),
            props: CharProps {
                bold: true,
                color: Some(Color {
                    r: 0xFF,
                    g: 0,
                    b: 0,
                }),
                size_half_pt: Some(28),
                font: Some("맑은 고딕".to_string()),
                ..CharProps::default()
            },
            ..Run::default()
        };
        let para = Paragraph {
            props: ParaProps {
                spacing: Spacing {
                    before_pt: Some(12.0),
                    after_pt: Some(6.0),
                    line_pct: Some(1.5),
                },
                indent: Indent {
                    left_pt: Some(24.0),
                    ..Indent::default()
                },
                shading: Some(Color {
                    r: 0xEE,
                    g: 0xEE,
                    b: 0xEE,
                }),
                ..ParaProps::default()
            },
            runs: vec![run],
        };
        let model = DocModel {
            blocks: vec![Block::Paragraph(para)],
            ..DocModel::default()
        };
        let m2 = Document::open(&super::to_docx(&model)).unwrap().model();
        let Block::Paragraph(p) = &m2.blocks[0] else {
            panic!("para")
        };
        let rp = &p.runs[0].props;
        assert!(rp.bold);
        assert_eq!(
            rp.color,
            Some(Color {
                r: 0xFF,
                g: 0,
                b: 0
            })
        );
        assert_eq!(rp.size_half_pt, Some(28));
        assert_eq!(rp.font.as_deref(), Some("맑은 고딕"));
        assert_eq!(p.props.spacing.before_pt, Some(12.0));
        assert_eq!(p.props.spacing.after_pt, Some(6.0));
        assert_eq!(p.props.spacing.line_pct, Some(1.5));
        assert_eq!(p.props.indent.left_pt, Some(24.0));
        assert_eq!(
            p.props.shading,
            Some(Color {
                r: 0xEE,
                g: 0xEE,
                b: 0xEE
            })
        );
    }

    #[test]
    fn empty_model_writes_openable_docx() {
        let bytes = super::to_docx(&DocModel::default());
        let doc = Document::open(&bytes).expect("empty .docx must still open");
        assert!(doc.model().blocks.is_empty());
    }

    #[test]
    fn giant_span_table_stays_bounded() {
        // A hostile col_span/row_span must be clamped, not amplified into millions
        // of <w:gridCol>/cells.
        let model = DocModel {
            blocks: vec![Block::Table(Table {
                rows: vec![Row {
                    cells: vec![Cell {
                        blocks: vec![Block::Paragraph(para("x"))],
                        col_span: u16::MAX,
                        row_span: u16::MAX,
                        is_header: false,
                        ..Default::default()
                    }],
                }],
                header_rows: 0,
                ..Default::default()
            })],
            ..DocModel::default()
        };
        let bytes = super::to_docx(&model);
        assert!(
            bytes.len() < 1_000_000,
            "giant span amplified output to {} bytes",
            bytes.len()
        );
        assert!(Document::open(&bytes).is_ok());
    }

    /// A header/footer + page numbers emit the `header1.xml`/`footer1.xml` parts
    /// (with a `PAGE` field) and section references, and crucially do **not**
    /// corrupt the body: it still re-opens and reads back. (LibreOffice's headless
    /// converter cannot load *any* docx with a header — even canonical ones — so
    /// the body round-trip + the part bytes are the verifiable oracle.)
    #[test]
    fn emits_header_footer_and_page_numbers() {
        use crate::model::DocSetup;
        let model = DocModel {
            blocks: vec![Block::Paragraph(para("본문"))],
            setup: DocSetup {
                header: vec![Block::Paragraph(para("러닝 헤더"))],
                footer: vec![Block::Paragraph(para("푸터"))],
                page_numbers: true,
                ..DocSetup::default()
            },
            ..DocModel::default()
        };
        let bytes = super::to_docx(&model);
        let blob = String::from_utf8_lossy(&bytes);
        // The OPC zip stores part names uncompressed in the local headers.
        assert!(blob.contains("word/header1.xml"), "missing header part");
        assert!(blob.contains("word/footer1.xml"), "missing footer part");
        // Full round-trip: the reader now extracts the header/footer back, so the
        // body stays in main_text() and the running header/footer reach text().
        let doc = Document::open(&bytes).expect("doc with header/footer must open");
        assert_eq!(doc.main_text().trim(), "본문");
        let full = doc.text();
        assert!(full.contains("본문"), "body lost: {full:?}");
        assert!(full.contains("러닝 헤더"), "header not read back: {full:?}");
        assert!(full.contains("푸터"), "footer not read back: {full:?}");
    }
}
