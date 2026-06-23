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
    Align, Block, CharProps, Color, FieldRole, Image, ParaProps, Paragraph, Table, VertAlign,
};

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
const CT_HEADER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml";
const CT_FOOTER: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml";
const REL_HEADER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/header";
const REL_FOOTER: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer";

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

/// A `word/styles.xml` defining `Normal` + `Heading1..6` (so `w:pStyle` renders
/// as a real heading in Word; the `outlineLvl` we also emit keeps round-trip
/// robust). Heading sizes in half-points: 32/28/26/24/22/22.
fn styles_xml() -> String {
    let mut s = String::new();
    s.push_str(XML_DECL);
    s.push_str(&format!(r#"<w:styles xmlns:w="{W_NS}">"#));
    s.push_str(
        r#"<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>"#,
    );
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
    s.push_str("</w:styles>");
    s
}

const W_NS: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const R_NS: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
const WP_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing";
const A_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/main";
const PIC_NS: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";
const PIC_URI: &str = "http://schemas.openxmlformats.org/drawingml/2006/picture";

const XML_DECL: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;
const CT_DOCUMENT: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";
const CT_NUMBERING: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml";
const REL_OFFICE_DOCUMENT: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument";
const REL_NUMBERING: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering";
const REL_HYPERLINK: &str =
    "http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink";
const REL_IMAGE: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/image";
/// Hard cap on table columns / a cell's column or row span, so a hostile model
/// (`col_span = u16::MAX`) cannot amplify into millions of `<w:gridCol>`/cells.
const MAX_TABLE_COLS: usize = 1024;

/// Side-state accumulated while folding the model into `document.xml`: the body
/// XML is built in `out` strings passed to each method, while these tables grow.
struct Ctx {
    /// `word/_rels/document.xml.rels` entries (hyperlinks + images).
    doc_rels: Vec<Rel>,
    /// Media parts to emit: `(part path, bytes, extension, content-type)`.
    media: Vec<(String, Vec<u8>, &'static str, &'static str)>,
    /// Next relationship id ordinal.
    next_rid: u32,
    /// Whether any list item was emitted (⇒ write `numbering.xml`).
    has_list: bool,
    /// Whether any heading was emitted (⇒ write `styles.xml`).
    has_heading: bool,
    /// Image counter for unique `media/imageN` names + drawing ids.
    img_id: u32,
}

impl Ctx {
    fn new() -> Self {
        Ctx {
            doc_rels: Vec::new(),
            media: Vec::new(),
            next_rid: 1,
            has_list: false,
            has_heading: false,
            img_id: 0,
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
        }
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
        let style_id = heading.map(|h| format!("Heading{}", h.clamp(1, 6)));
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
        {
            return;
        }
        out.push_str("<w:pPr>");
        // Schema order: pStyle, numPr, shd, spacing, ind, jc, outlineLvl.
        if let Some(s) = &style_id {
            self.has_heading = true;
            out.push_str(&format!(r#"<w:pStyle w:val="{s}"/>"#));
        }
        if let Some(li) = list {
            self.has_list = true;
            let num_id = if li.ordered { 1 } else { 2 };
            out.push_str(&format!(
                r#"<w:numPr><w:ilvl w:val="{}"/><w:numId w:val="{num_id}"/></w:numPr>"#,
                li.level
            ));
        }
        if let Some(c) = pr.shading {
            out.push_str(&format!(
                r#"<w:shd w:val="clear" w:color="auto" w:fill="{}"/>"#,
                hex(c)
            ));
        }
        if has_spacing {
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
        if has_indent {
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
        if let Some(j) = jc {
            out.push_str(&format!(r#"<w:jc w:val="{j}"/>"#));
        }
        if let Some(o) = outline {
            out.push_str(&format!(r#"<w:outlineLvl w:val="{o}"/>"#));
        }
        out.push_str("</w:pPr>");
    }

    fn write_run(&mut self, out: &mut String, r: &crate::model::Run) {
        match &r.field {
            FieldRole::Hyperlink { url } => {
                let rid = self.add_rel(REL_HYPERLINK, url, true);
                out.push_str(&format!(r#"<w:hyperlink r:id="{rid}">"#));
                self.write_run_inner(out, r);
                out.push_str("</w:hyperlink>");
            }
            _ => self.write_run_inner(out, r),
        }
    }

    fn write_run_inner(&mut self, out: &mut String, r: &crate::model::Run) {
        if let Some(img) = &r.image {
            if img.bytes.is_some() {
                self.write_image(out, img);
                return;
            }
        }
        out.push_str("<w:r>");
        write_rpr(out, &r.props);
        write_run_text(out, &r.text);
        out.push_str("</w:r>");
    }

    fn write_image(&mut self, out: &mut String, img: &Image) {
        let Some(bytes) = img.bytes.clone() else {
            return;
        };
        let (ext, ct) = img_ext_ct(img.mime.as_deref());
        self.img_id += 1;
        let n = self.img_id;
        let target = format!("media/image{n}.{ext}");
        let rid = self.add_rel(REL_IMAGE, &target, false);
        self.media.push((format!("word/{target}"), bytes, ext, ct));
        // Extent (EMU) from the image's intrinsic pixels at 96 dpi (1px = 9525
        // EMU), clamped to the ~6in content width; falls back to 2in² if the
        // header had no dimensions.
        let (cx, cy) = image_extent_emu(img.width_px, img.height_px);
        out.push_str(&format!(
            concat!(
                r#"<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">"#,
                r#"<wp:extent cx="{cx}" cy="{cy}"/><wp:docPr id="{n}" name="Image{n}"/>"#,
                r#"<a:graphic><a:graphicData uri="{uri}"><pic:pic><pic:nvPicPr>"#,
                r#"<pic:cNvPr id="{n}" name="Image{n}"/><pic:cNvPicPr/></pic:nvPicPr>"#,
                r#"<pic:blipFill><a:blip r:embed="{rid}"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>"#,
                r#"<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm>"#,
                r#"<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>"#,
                r#"</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#,
            ),
            cx = cx,
            cy = cy,
            n = n,
            uri = PIC_URI,
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
        out.push_str(r#"<w:tbl><w:tblPr><w:tblW w:w="0" w:type="auto"/>"#);
        out.push_str(concat!(
            r#"<w:tblBorders>"#,
            r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"<w:bottom w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"<w:right w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"<w:insideV w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#,
            r#"</w:tblBorders>"#,
        ));
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
    let mut buf = String::new();
    let flush = |out: &mut String, buf: &mut String| {
        if !buf.is_empty() {
            out.push_str(r#"<w:t xml:space="preserve">"#);
            out.push_str(&esc_text(buf));
            out.push_str("</w:t>");
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
        pkg.add_part(path, Some(ct), bytes);
    }
    if br.has_list {
        pkg.add_part(
            "word/numbering.xml",
            Some(CT_NUMBERING),
            numbering_xml().into_bytes(),
        );
    }
    if br.has_heading {
        pkg.add_part(
            "word/styles.xml",
            Some(CT_STYLES),
            styles_xml().into_bytes(),
        );
    }
    for (path, bytes, ext, ct) in br.media {
        pkg.add_default(ext, ct);
        pkg.add_part(&path, None, bytes);
    }

    if !br.doc_rels.is_empty() {
        pkg.add_rels("word/_rels/document.xml.rels", br.doc_rels);
    }
    pkg.add_rels(
        "_rels/.rels",
        vec![Rel {
            id: "rId1".to_string(),
            rel_type: REL_OFFICE_DOCUMENT.to_string(),
            target: "word/document.xml".to_string(),
            external: false,
        }],
    );

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
    pub hf_parts: Vec<(&'static str, &'static str, Vec<u8>)>,
    /// `(part path, bytes, extension, content-type)` for inline/block images.
    pub media: Vec<(String, Vec<u8>, &'static str, &'static str)>,
    pub has_list: bool,
    pub has_heading: bool,
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
        r#"<w:document xmlns:w="{W_NS}" xmlns:r="{R_NS}" xmlns:wp="{WP_NS}" xmlns:a="{A_NS}" xmlns:pic="{PIC_NS}"><w:body>"#
    ));
    doc.push_str(&body);

    // Header/footer parts + their section references (computed before the sectPr,
    // which references them by relationship id).
    let mut doc_rels = std::mem::take(&mut ctx.doc_rels);
    let mut hf_parts: Vec<(&'static str, &'static str, Vec<u8>)> = Vec::new();
    let mut sect_refs = String::new();
    if !model.setup.header.is_empty() {
        let rid = format!("rId{}", ctx.next_rid);
        ctx.next_rid += 1;
        hf_parts.push((
            "word/header1.xml",
            CT_HEADER,
            hf_part("hdr", &render_hf_body(&model.setup.header, false)),
        ));
        doc_rels.push(Rel {
            id: rid.clone(),
            rel_type: REL_HEADER.to_string(),
            target: "header1.xml".to_string(),
            external: false,
        });
        sect_refs.push_str(&format!(
            r#"<w:headerReference w:type="default" r:id="{rid}"/>"#
        ));
    }
    if !model.setup.footer.is_empty() || model.setup.page_numbers {
        let rid = format!("rId{}", ctx.next_rid);
        ctx.next_rid += 1;
        hf_parts.push((
            "word/footer1.xml",
            CT_FOOTER,
            hf_part(
                "ftr",
                &render_hf_body(&model.setup.footer, model.setup.page_numbers),
            ),
        ));
        doc_rels.push(Rel {
            id: rid.clone(),
            rel_type: REL_FOOTER.to_string(),
            target: "footer1.xml".to_string(),
            external: false,
        });
        sect_refs.push_str(&format!(
            r#"<w:footerReference w:type="default" r:id="{rid}"/>"#
        ));
    }

    // Section: header/footer refs (schema order: before pgSz) then page geometry
    // from DocSetup. width_pt/height_pt are the already-oriented dimensions; the
    // landscape flag is emitted as `w:orient` (no swap).
    let page = &model.setup.page;
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
    doc.push_str(&format!(
        r#"<w:sectPr>{sect_refs}<w:pgSz w:w="{w}" w:h="{h}"{orient}/><w:pgMar w:top="{mt}" w:right="{mr}" w:bottom="{mb}" w:left="{ml}" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr>"#
    ));
    doc.push_str("</w:body></w:document>");

    // Type-link rels for the styles/numbering parts (read-by-path doesn't need
    // them, but strict consumers like Word expect the relationship to exist). Minted
    // after the hf rels so ids match the previous single-pass writer exactly.
    if ctx.has_list {
        doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_NUMBERING.to_string(),
            target: "numbering.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }
    if ctx.has_heading {
        doc_rels.push(Rel {
            id: format!("rId{}", ctx.next_rid),
            rel_type: REL_STYLES.to_string(),
            target: "styles.xml".to_string(),
            external: false,
        });
        ctx.next_rid += 1;
    }

    BodyRender {
        document_xml: doc.into_bytes(),
        doc_rels,
        hf_parts,
        media: ctx.media,
        has_list: ctx.has_list,
        has_heading: ctx.has_heading,
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
