//! `.docx` body (`word/document.xml`) → [`Block`]s, by recursive descent over a
//! streaming [`quick_xml`] reader.
//!
//! Each `read_*` helper is entered just after its element's `Start` event and
//! consumes through the matching `End`. The invariant that keeps the loops simple
//! is: **every child `Start` is consumed by a sub-parser or [`skip_subtree`], and
//! `w:t` text is read by [`read_text`]** — so the only `End` that reaches a
//! parser's own loop is its own, and it can break on the first `End` it sees.
//! (`w:pPr`/`w:rPr`/`w:tcPr`/`w:trPr` flatten their simple children instead and
//! break on their *named* end.)

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::numbering::Numbering;
use super::styles::Styles;
use super::{attr_local, local, toggle_on};
use crate::model::{
    Align, Block, Cell, CharProps, Color, FieldRole, Image, Indent, ListInfo, ParaProps, Paragraph,
    Row, Run, Spacing, Table, VCell, VertAlign,
};

/// Parse an OOXML hex color (`"RRGGBB"`); `"auto"`/invalid → `None`.
fn parse_hex_color(s: &str) -> Option<Color> {
    if s.eq_ignore_ascii_case("auto") || s.len() != 6 {
        return None;
    }
    let n = u32::from_str_radix(s, 16).ok()?;
    Some(Color {
        r: (n >> 16) as u8,
        g: (n >> 8) as u8,
        b: n as u8,
    })
}

/// Half-points (`w:val` twentieths-of-a-point are NOT used here; `w:sz` is in
/// half-points) → `Option<u16>`.
fn parse_u16(s: &str) -> Option<u16> {
    s.trim().parse().ok()
}

/// Twips (1/20 pt) string → points.
fn twips_to_pt(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().map(|t| t / 20.0)
}

/// The borrowing reader produced by `Reader::from_str`.
type Xml<'a> = Reader<&'a [u8]>;

/// Hard cap on structural nesting depth (nested tables / run wrappers). Real
/// documents nest a handful of levels; pathological/fuzzed files (e.g. POI's
/// `deep-table-cell.docx`) nest thousands deep and would overflow the recursive
/// descent's stack — a process abort that breaks the panic-free contract. Past
/// this depth the subtree is skipped rather than recursed into.
const MAX_DEPTH: u32 = 128;

/// Resolved supplementary tables, passed down the descent.
pub(crate) struct Ctx<'a> {
    pub styles: &'a Styles,
    pub numbering: &'a Numbering,
    pub rels: &'a HashMap<String, (String, bool)>,
    pub media: &'a HashMap<String, Image>,
    /// Live per-`numId` level counters for autonumber labels, advanced in document
    /// order as list paragraphs are finalized (interior-mutable: parsing is
    /// single-threaded and `finalize_paragraph` runs in reading order).
    pub counters: std::cell::RefCell<HashMap<String, [u32; 9]>>,
}

/// Parse `word/document.xml` into block-level nodes.
pub(crate) fn parse_document(xml: &str, ctx: &Ctx<'_>) -> Vec<Block> {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"body" => {
                return read_blocks(&mut r, ctx, 0);
            }
            Ok(Event::Eof) | Err(_) => return Vec::new(),
            _ => {}
        }
    }
}

/// Scan `word/document.xml` for every `<w:headerReference>` / `<w:footerReference>`
/// relationship id (in document order, across all `w:sectPr`). Returns
/// `(header r:ids, footer r:ids)`; the caller resolves and de-duplicates them.
pub(crate) fn scan_hf_refs(xml: &str) -> (Vec<String>, Vec<String>) {
    let mut r = Reader::from_str(xml);
    let (mut headers, mut footers) = (Vec::new(), Vec::new());
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"headerReference" => {
                    if let Some(id) = attr_local(&e, b"id") {
                        headers.push(id);
                    }
                }
                b"footerReference" => {
                    if let Some(id) = attr_local(&e, b"id") {
                        footers.push(id);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (headers, footers)
}

/// Scan the body's section properties for page geometry (`<w:pgSz>` size +
/// orientation, `<w:pgMar>` left margin) → [`crate::model::PageSetup`]. Uses the
/// last `sectPr` (the final/primary section). Falls back to the A4 default when
/// absent. Twips (1/20 pt) → points.
pub(crate) fn scan_page_setup(xml: &str) -> crate::model::PageSetup {
    use crate::model::PageSetup;
    let mut r = Reader::from_str(xml);
    let mut page = PageSetup::default();
    let mut found = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pgSz" => {
                    if let (Some(w), Some(h)) = (
                        attr_local(&e, b"w").and_then(|v| twips_to_pt(&v)),
                        attr_local(&e, b"h").and_then(|v| twips_to_pt(&v)),
                    ) {
                        page.width_pt = w;
                        page.height_pt = h;
                        page.landscape = attr_local(&e, b"orient").as_deref() == Some("landscape");
                        found = true;
                    }
                }
                b"pgMar" => {
                    let l = attr_local(&e, b"left").and_then(|v| twips_to_pt(&v));
                    let r = attr_local(&e, b"right").and_then(|v| twips_to_pt(&v));
                    let t = attr_local(&e, b"top").and_then(|v| twips_to_pt(&v));
                    let b = attr_local(&e, b"bottom").and_then(|v| twips_to_pt(&v));
                    if l.or(r).or(t).or(b).is_some() {
                        found = true;
                        if let Some(l) = l {
                            page.margin_pt = l; // uniform fallback = left
                        }
                        page.margin_left_pt = l;
                        page.margin_right_pt = r;
                        page.margin_top_pt = t;
                        page.margin_bottom_pt = b;
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if found {
        page
    } else {
        PageSetup::default()
    }
}

/// Parse a `word/headerN.xml` / `footerN.xml` part (root `<w:hdr>` / `<w:ftr>`)
/// into block-level nodes, reusing the same grammar as the body.
pub(crate) fn parse_hdrftr(xml: &str, ctx: &Ctx<'_>) -> Vec<Block> {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if matches!(local(e.name().as_ref()), b"hdr" | b"ftr") => {
                return read_blocks(&mut r, ctx, 0);
            }
            Ok(Event::Eof) | Err(_) => return Vec::new(),
            _ => {}
        }
    }
}

/// Parse `word/footnotes.xml` / `endnotes.xml`: the real notes' block content,
/// skipping the `separator`/`continuationSeparator`/`continuationNotice`
/// boilerplate notes. `tag` is `b"footnote"` or `b"endnote"`.
pub(crate) fn parse_notes(xml: &str, ctx: &Ctx<'_>, tag: &[u8]) -> Vec<Block> {
    let mut r = Reader::from_str(xml);
    let mut blocks = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == tag => {
                let boilerplate = matches!(
                    attr_local(&e, b"type").as_deref(),
                    Some("separator") | Some("continuationSeparator") | Some("continuationNotice")
                );
                if boilerplate {
                    skip_subtree(&mut r);
                } else {
                    blocks.extend(read_blocks(&mut r, ctx, 0));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    blocks
}

/// Read block-level children (`w:p`, `w:tbl`) until the enclosing `End`. Block
/// content controls (`w:sdt`/`w:sdtContent`) and `w:customXml` are transparent
/// wrappers — descended into so their paragraphs/tables aren't lost.
fn read_blocks(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Block> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut blocks = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"p" => {
                    let p = read_paragraph(r, ctx, depth + 1);
                    if !p.is_blank() {
                        blocks.push(Block::Paragraph(p));
                    }
                }
                b"tbl" => {
                    let t = read_table(r, ctx, depth + 1);
                    if !t.rows.is_empty() {
                        blocks.push(Block::Table(t));
                    }
                }
                b"sdt" | b"sdtContent" | b"customXml" => {
                    blocks.extend(read_blocks(r, ctx, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    blocks
}

/// Read a `<w:p>`: its `w:pPr` properties and inline runs.
fn read_paragraph(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Paragraph {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Paragraph::default();
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut pp = PPr::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"pPr" => pp = read_ppr(r),
                b"r" => runs.extend(read_run(r, ctx, None, depth + 1)),
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"ins" | b"smartTag" | b"sdtContent" | b"sdt" | b"bdo" | b"dir" => {
                    runs.extend(read_runs_container(r, ctx, None, depth + 1))
                }
                // `w:del` = tracked deletion (removed text) → drop.
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    finalize_paragraph(runs, pp, ctx)
}

/// Collected `<w:pPr>` properties.
#[derive(Default)]
struct PPr {
    style_id: Option<String>,
    num: Option<(String, u8)>,
    jc: Option<String>,
    outline: Option<u8>,
    spacing: Spacing,
    indent: Indent,
    shading: Option<Color>,
}

/// Collect runs from a run-bearing wrapper (`w:hyperlink`, `w:ins`, `w:sdt`, …)
/// until its `End`, carrying an optional hyperlink `url` onto the text runs.
fn read_runs_container(r: &mut Xml<'_>, ctx: &Ctx<'_>, link: Option<&str>, depth: u32) -> Vec<Run> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut runs = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"r" => runs.extend(read_run(r, ctx, link, depth + 1)),
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"ins" | b"smartTag" | b"sdtContent" | b"sdt" | b"bdo" | b"dir" => {
                    runs.extend(read_runs_container(r, ctx, link, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    runs
}

/// Read `<w:pPr>` properties (flattening `w:numPr`'s `w:ilvl`/`w:numId`).
fn read_ppr(r: &mut Xml<'_>) -> PPr {
    let mut pp = PPr::default();
    let mut num_id: Option<String> = None;
    let mut ilvl: u8 = 0;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pStyle" => pp.style_id = attr_local(&e, b"val"),
                b"ilvl" => {
                    if let Some(v) = attr_local(&e, b"val").and_then(|v| v.parse().ok()) {
                        ilvl = v;
                    }
                }
                b"numId" => num_id = attr_local(&e, b"val"),
                b"jc" => pp.jc = attr_local(&e, b"val"),
                b"outlineLvl" => pp.outline = attr_local(&e, b"val").and_then(|v| v.parse().ok()),
                b"spacing" => {
                    pp.spacing.before_pt = attr_local(&e, b"before").and_then(|v| twips_to_pt(&v));
                    pp.spacing.after_pt = attr_local(&e, b"after").and_then(|v| twips_to_pt(&v));
                    // `w:line` is 240ths of a line when lineRule is auto/absent.
                    let exact = matches!(
                        attr_local(&e, b"lineRule").as_deref(),
                        Some("exact") | Some("atLeast")
                    );
                    if !exact {
                        pp.spacing.line_pct = attr_local(&e, b"line")
                            .and_then(|v| v.trim().parse::<f32>().ok())
                            .map(|l| l / 240.0);
                    }
                }
                b"ind" => {
                    pp.indent.left_pt = attr_local(&e, b"left")
                        .or_else(|| attr_local(&e, b"start"))
                        .and_then(|v| twips_to_pt(&v));
                    pp.indent.right_pt = attr_local(&e, b"right")
                        .or_else(|| attr_local(&e, b"end"))
                        .and_then(|v| twips_to_pt(&v));
                    pp.indent.first_line_pt =
                        attr_local(&e, b"firstLine").and_then(|v| twips_to_pt(&v));
                    pp.indent.hanging_pt = attr_local(&e, b"hanging").and_then(|v| twips_to_pt(&v));
                }
                b"shd" => pp.shading = attr_local(&e, b"fill").and_then(|v| parse_hex_color(&v)),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"pPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    if let Some(id) = num_id {
        pp.num = Some((id, ilvl));
    }
    pp
}

/// Read a `<w:r>`: its `w:rPr` formatting plus text / breaks / drawings. Returns
/// a (possibly empty) text run followed by any inline image runs.
/// Push an extracted drawing's image and/or text-box text as plain runs so they
/// surface in the body / exporters / renderer.
fn push_drawing_runs(images: &mut Vec<Run>, img: Option<Image>, txbx: String) {
    if let Some(img) = img {
        images.push(Run {
            text: String::new(),
            props: CharProps::default(),
            field: FieldRole::None,
            image: Some(img),
        });
    }
    if !txbx.trim().is_empty() {
        images.push(Run {
            text: txbx,
            props: CharProps::default(),
            field: FieldRole::None,
            image: None,
        });
    }
}

fn read_run(r: &mut Xml<'_>, ctx: &Ctx<'_>, link: Option<&str>, depth: u32) -> Vec<Run> {
    // A run can recurse back into block content through a drawing's text box
    // (drawing → txbxContent → paragraph → run → drawing …); `depth` threads the
    // structural recursion budget across that boundary so MAX_DEPTH bounds it.
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut props = CharProps::default();
    let mut text = String::new();
    let mut images: Vec<Run> = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"rPr" => props = read_rpr(r),
                b"t" => text.push_str(&read_text(r)),
                b"drawing" | b"pict" | b"object" => {
                    let (img, txbx) = read_drawing(r, ctx, depth);
                    push_drawing_runs(&mut images, img, txbx);
                }
                // A floating shape often sits as <w:r><mc:AlternateContent> wrapping
                // the DrawingML <w:drawing> (Choice) and VML <w:pict> (Fallback) of
                // the SAME shape — descend one branch so its text box is recovered
                // (and not doubled), instead of skipping the whole shape.
                b"AlternateContent" => {
                    let mut img = None;
                    let mut txbx = String::new();
                    walk_alternate_content(r, ctx, &mut img, &mut txbx, depth + 1);
                    push_drawing_runs(&mut images, img, txbx);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"tab" => text.push('\t'),
                b"br" | b"cr" => text.push('\n'),
                b"noBreakHyphen" => text.push('-'),
                _ => {}
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut runs = Vec::new();
    if !text.is_empty() {
        runs.push(Run {
            text,
            props,
            field: link
                .map(|u| FieldRole::Hyperlink { url: u.to_string() })
                .unwrap_or(FieldRole::None),
            image: None,
        });
    }
    runs.extend(images);
    runs
}

/// Read `<w:rPr>` formatting toggles (bold/italic/underline/strike/hidden).
fn read_rpr(r: &mut Xml<'_>) -> CharProps {
    let mut p = CharProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"b" => p.bold = toggle_on(attr_local(&e, b"val")),
                b"i" => p.italic = toggle_on(attr_local(&e, b"val")),
                b"strike" => p.strike = toggle_on(attr_local(&e, b"val")),
                b"dstrike" => p.strike |= toggle_on(attr_local(&e, b"val")),
                b"vanish" => p.hidden = toggle_on(attr_local(&e, b"val")),
                // `w:u` carries a line style; anything but "none" underlines.
                b"u" => p.underline = attr_local(&e, b"val").map(|v| v != "none").unwrap_or(true),
                b"smallCaps" => p.small_caps = toggle_on(attr_local(&e, b"val")),
                b"caps" => p.caps = toggle_on(attr_local(&e, b"val")),
                // Font family: prefer the East-Asian face (Korean) over the Latin one.
                b"rFonts" => {
                    p.font = attr_local(&e, b"eastAsia").or_else(|| attr_local(&e, b"ascii"));
                }
                b"sz" => p.size_half_pt = attr_local(&e, b"val").and_then(|v| parse_u16(&v)),
                b"color" => p.color = attr_local(&e, b"val").and_then(|v| parse_hex_color(&v)),
                b"highlight" => p.highlight = attr_local(&e, b"val"),
                b"vertAlign" => {
                    p.vert_align = match attr_local(&e, b"val").as_deref() {
                        Some("superscript") => VertAlign::Super,
                        Some("subscript") => VertAlign::Sub,
                        _ => VertAlign::Baseline,
                    };
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"rPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    p
}

/// Read the text content of a `<w:t>` (preserving whitespace), through `</w:t>`.
///
/// `unescape` resolves the standard XML entities but errors on an unknown/custom
/// entity (e.g. an XXE `SYSTEM` entity) — in that case we keep the raw text
/// verbatim rather than dropping the whole node, which both preserves the
/// readable words and never resolves (fetches) the external entity.
fn read_text(r: &mut Xml<'_>) -> String {
    let mut s = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Text(t)) => {
                let unescaped = t.unescape().ok().map(|c| c.into_owned());
                match unescaped {
                    Some(c) => s.push_str(&c),
                    None => s.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
                }
            }
            Ok(Event::CData(t)) => s.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    s
}

/// Scan a `<w:drawing>`/`<w:pict>` subtree for (a) the first image blip, resolved
/// to extracted bytes via the relationship/media tables, and (b) any text-box
/// (`w:txbxContent`) text. Honors `mc:AlternateContent` (descends a single branch)
/// so a box serialized as both DrawingML and VML isn't counted twice.
fn read_drawing(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> (Option<Image>, String) {
    let mut img = None;
    let mut text = String::new();
    // Start from the caller's structural depth (not 0) so the recursion budget is
    // continuous across the drawing/text-box boundary.
    walk_drawing(r, ctx, &mut img, &mut text, depth);
    (img, text)
}

/// Recursively consume a drawing subtree through its `End`, collecting the first
/// blip image and all text-box text. `txbxContent` children hold body-level
/// content, parsed with [`read_blocks`] and flattened to text.
fn walk_drawing(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    img: &mut Option<Image>,
    text: &mut String,
    depth: u32,
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"txbxContent" => {
                    if depth < MAX_DEPTH {
                        let blocks = read_blocks(r, ctx, depth + 1);
                        append_blocks_text(text, &blocks);
                    } else {
                        skip_subtree(r);
                    }
                }
                b"AlternateContent" => walk_alternate_content(r, ctx, img, text, depth + 1),
                _ => {
                    if img.is_none() {
                        *img = blip_image(&e, ctx);
                    }
                    if depth < MAX_DEPTH {
                        walk_drawing(r, ctx, img, text, depth + 1);
                    } else {
                        skip_subtree(r);
                    }
                }
            },
            Ok(Event::Empty(e)) if img.is_none() => {
                *img = blip_image(&e, ctx);
            }
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

/// `mc:AlternateContent` wraps the SAME box as a `Choice` (DrawingML) and a
/// `Fallback` (VML); descend the first branch only so its text isn't doubled.
fn walk_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    img: &mut Option<Image>,
    text: &mut String,
    depth: u32,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    if depth < MAX_DEPTH {
                        walk_drawing(r, ctx, img, text, depth + 1);
                    } else {
                        skip_subtree(r);
                    }
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

/// Append the flattened text of block-level nodes (a text box's paragraphs/tables)
/// to `out`, newline-separated.
fn append_blocks_text(out: &mut String, blocks: &[Block]) {
    for b in blocks {
        let chunk = match b {
            Block::Paragraph(p) => p.text(),
            Block::Table(t) => t
                .rows
                .iter()
                .flat_map(|row| row.cells.iter().map(|c| c.text()))
                .filter(|c| !c.is_empty())
                .collect::<Vec<_>>()
                .join("\n"),
            Block::Image(_) => String::new(),
        };
        if !chunk.is_empty() {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&chunk);
        }
    }
}

/// `<a:blip r:embed>` (DrawingML) or `<v:imagedata r:id>` (VML) → the extracted
/// image for that relationship id, if it is one we extracted.
fn blip_image(e: &BytesStart<'_>, ctx: &Ctx<'_>) -> Option<Image> {
    let id = match local(e.name().as_ref()) {
        b"blip" => attr_local(e, b"embed")?,
        b"imagedata" => attr_local(e, b"id")?,
        _ => return None,
    };
    ctx.media.get(&id).cloned()
}

/// Read `<w:hyperlink>`: resolve its target (external `r:id` rel, or `#anchor`)
/// and tag its runs with the link.
fn read_hyperlink(r: &mut Xml<'_>, start: &BytesStart<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Run> {
    let url = hyperlink_url(start, ctx);
    read_runs_container(r, ctx, url.as_deref(), depth + 1)
}

fn hyperlink_url(start: &BytesStart<'_>, ctx: &Ctx<'_>) -> Option<String> {
    if let Some(id) = attr_local(start, b"id") {
        if let Some((target, _external)) = ctx.rels.get(&id) {
            return Some(target.clone());
        }
    }
    attr_local(start, b"anchor").map(|a| format!("#{a}"))
}

/// Read `<w:fldSimple>`: if its `w:instr` is a HYPERLINK field, link its runs.
fn read_fldsimple(r: &mut Xml<'_>, start: &BytesStart<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Run> {
    let url = attr_local(start, b"instr").and_then(|i| hyperlink_instr_url(&i));
    read_runs_container(r, ctx, url.as_deref(), depth + 1)
}

/// Extract a URL from a `HYPERLINK "…"` field instruction (matches the `.doc`
/// field-code parser).
fn hyperlink_instr_url(instr: &str) -> Option<String> {
    let s = instr.trim();
    let after = s.find("HYPERLINK").map(|i| &s[i + "HYPERLINK".len()..])?;
    let q = after.find('"')?;
    let rest = &after[q + 1..];
    let end = rest.find('"')?;
    let url = rest[..end].trim();
    (!url.is_empty()).then(|| url.to_string())
}

/// Resolve paragraph-level properties (heading level, alignment, list) from the
/// collected `w:pPr` fields — mirroring `assemble.rs::take_paragraph` precedence
/// (explicit outline level wins; a heading suppresses list rendering).
fn finalize_paragraph(runs: Vec<Run>, pp: PPr, ctx: &Ctx<'_>) -> Paragraph {
    let PPr {
        style_id,
        num,
        jc,
        outline,
        spacing,
        indent,
        shading,
    } = pp;
    let heading_level = match outline {
        Some(o) if o <= 8 => Some(o + 1),
        Some(_) => None, // outlineLvl 9 = body text
        None => style_id
            .as_deref()
            .and_then(|s| ctx.styles.heading_level(s)),
    };
    let style_name = style_id
        .as_deref()
        .and_then(|s| ctx.styles.name(s))
        .map(str::to_string);
    let align = match jc.as_deref() {
        Some("center") => Align::Center,
        Some("right") | Some("end") => Align::Right,
        Some("both") | Some("distribute") => Align::Justify,
        _ => Align::Left,
    };
    // A heading takes precedence over list-item rendering. `numId == "0"` is the
    // OOXML "no list" sentinel.
    let list = if heading_level.is_some() {
        None
    } else {
        match num {
            Some((num_id, ilvl)) if num_id != "0" => {
                let ordered = ctx.numbering.ordered(&num_id, ilvl).unwrap_or(true);
                // Advance the live counters (document order) and format the label.
                let label = {
                    let mut map = ctx.counters.borrow_mut();
                    let c = map.entry(num_id.clone()).or_insert([0; 9]);
                    ctx.numbering.label(&num_id, ilvl, c).unwrap_or_default()
                };
                Some(ListInfo {
                    level: ilvl,
                    ordered,
                    label,
                })
            }
            _ => None,
        }
    };
    Paragraph {
        props: ParaProps {
            style_name,
            heading_level,
            align,
            outline_level: outline,
            list,
            spacing,
            indent,
            shading,
        },
        runs,
    }
}

/// A streamed cell before vertical-merge resolution.
struct CellRaw {
    blocks: Vec<Block>,
    col_span: u16,
    vmerge: VMerge,
    shading: Option<Color>,
    valign: VCell,
    width_pct: Option<f32>,
}

#[derive(Clone, Copy, PartialEq)]
enum VMerge {
    None,
    Restart,
    Continue,
}

/// Read a `<w:tbl>` and resolve merges into a [`Table`].
fn read_table(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Table {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Table::default();
    }
    let mut rows: Vec<(Vec<CellRaw>, bool)> = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tr" => rows.push(read_row(r, ctx, depth)),
                _ => skip_subtree(r), // tblPr, tblGrid, …
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    build_table(rows)
}

/// Read a `<w:tr>` → its cells and whether it is a repeated header row.
fn read_row(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> (Vec<CellRaw>, bool) {
    let mut cells = Vec::new();
    let mut header = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"trPr" => header = read_trpr(r),
                b"tc" => cells.push(read_cell(r, ctx, depth + 1)),
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (cells, header)
}

/// Read `<w:trPr>` → `w:tblHeader` flag.
fn read_trpr(r: &mut Xml<'_>) -> bool {
    let mut header = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblHeader" =>
            {
                header = toggle_on(attr_local(&e, b"val"));
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"trPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    header
}

/// Read a `<w:tc>` → its block content + `gridSpan`/`vMerge`.
fn read_cell(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> CellRaw {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return CellRaw {
            blocks: Vec::new(),
            col_span: 1,
            vmerge: VMerge::None,
            shading: None,
            valign: VCell::Top,
            width_pct: None,
        };
    }
    let mut blocks = Vec::new();
    let mut tc: Option<TcPr> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tcPr" => tc = Some(read_tcpr(r)),
                b"p" => blocks.push(Block::Paragraph(read_paragraph(r, ctx, depth + 1))),
                b"tbl" => {
                    let t = read_table(r, ctx, depth + 1);
                    if !t.rows.is_empty() {
                        blocks.push(Block::Table(t));
                    }
                }
                b"sdt" | b"sdtContent" | b"customXml" => {
                    blocks.extend(read_blocks(r, ctx, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let tc = tc.unwrap_or(TcPr {
        gs: 1,
        vm: VMerge::None,
        shading: None,
        valign: VCell::Top,
        width_pct: None,
    });
    CellRaw {
        blocks,
        col_span: tc.gs,
        vmerge: tc.vm,
        shading: tc.shading,
        valign: tc.valign,
        width_pct: tc.width_pct,
    }
}

/// Collected `<w:tcPr>` properties.
struct TcPr {
    gs: u16,
    vm: VMerge,
    shading: Option<Color>,
    valign: VCell,
    width_pct: Option<f32>,
}

/// Read `<w:tcPr>` → gridSpan / vMerge / shading / vAlign / width.
fn read_tcpr(r: &mut Xml<'_>) -> TcPr {
    let mut t = TcPr {
        gs: 1,
        vm: VMerge::None,
        shading: None,
        valign: VCell::Top,
        width_pct: None,
    };
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"gridSpan" => {
                    if let Some(v) = attr_local(&e, b"val").and_then(|v| v.parse::<u16>().ok()) {
                        t.gs = v.max(1);
                    }
                }
                b"vMerge" => {
                    t.vm = match attr_local(&e, b"val").as_deref() {
                        Some("restart") => VMerge::Restart,
                        _ => VMerge::Continue, // present with "continue"/no val
                    };
                }
                b"shd" => t.shading = attr_local(&e, b"fill").and_then(|v| parse_hex_color(&v)),
                b"vAlign" => {
                    t.valign = match attr_local(&e, b"val").as_deref() {
                        Some("center") => VCell::Center,
                        Some("bottom") => VCell::Bottom,
                        _ => VCell::Top,
                    };
                }
                // `type="pct"` w:w is in fiftieths of a percent (5000 = 100%);
                // `dxa` (twips) is absolute and left as auto here.
                b"tcW" if attr_local(&e, b"type").as_deref() == Some("pct") => {
                    t.width_pct = attr_local(&e, b"w")
                        .and_then(|v| v.trim().parse::<f32>().ok())
                        .map(|p| p / 5000.0);
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tcPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    t
}

/// Place cells over a running column index and resolve vertical merges
/// (`vMerge="restart"` opens a span, a later `vMerge` continuation at the same
/// starting column grows the owner's `row_span` and is dropped) — the OOXML
/// analogue of `table.rs` Phase B.
fn build_table(raw_rows: Vec<(Vec<CellRaw>, bool)>) -> Table {
    let header_rows = raw_rows.iter().take_while(|(_, h)| *h).count();

    struct Placed {
        blocks: Vec<Block>,
        col: usize,
        col_span: u16,
        row_span: u16,
        is_header: bool,
        vmerge: VMerge,
        dropped: bool,
        shading: Option<Color>,
        valign: VCell,
        width_pct: Option<f32>,
    }

    let mut grid: Vec<Vec<Placed>> = Vec::with_capacity(raw_rows.len());
    for (cells, header) in raw_rows {
        let mut col = 0usize;
        let mut row = Vec::with_capacity(cells.len());
        for c in cells {
            let cs = c.col_span.max(1);
            row.push(Placed {
                blocks: c.blocks,
                col,
                col_span: cs,
                row_span: 1,
                is_header: header,
                vmerge: c.vmerge,
                dropped: false,
                shading: c.shading,
                valign: c.valign,
                width_pct: c.width_pct,
            });
            col += cs as usize;
        }
        grid.push(row);
    }

    let mut open: HashMap<usize, (usize, usize)> = HashMap::new();
    for r in 0..grid.len() {
        for o in 0..grid[r].len() {
            let col = grid[r][o].col;
            match grid[r][o].vmerge {
                VMerge::Restart => {
                    open.insert(col, (r, o));
                }
                VMerge::Continue => {
                    if let Some(&(rr, oo)) = open.get(&col) {
                        grid[rr][oo].row_span = grid[rr][oo].row_span.saturating_add(1);
                        grid[r][o].dropped = true;
                    } else {
                        // Continuation with no open restart → recover as its own
                        // cell that a following continuation may merge into.
                        open.insert(col, (r, o));
                    }
                }
                VMerge::None => {
                    open.remove(&col);
                }
            }
        }
    }

    let mut rows = Vec::with_capacity(grid.len());
    for row in grid {
        let cells: Vec<Cell> = row
            .into_iter()
            .filter(|p| !p.dropped)
            .map(|p| Cell {
                blocks: p.blocks,
                col_span: p.col_span,
                row_span: p.row_span,
                is_header: p.is_header,
                shading: p.shading,
                valign: p.valign,
                width_pct: p.width_pct,
            })
            .collect();
        rows.push(Row { cells });
    }
    Table {
        rows,
        header_rows,
        ..Default::default()
    }
}

/// Consume the current element's subtree (we just read its `Start`), through the
/// matching `End`, depth-tracked so nested same-named elements are handled.
fn skip_subtree(r: &mut Xml<'_>) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
                depth -= 1;
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Block;

    fn parse(xml: &str) -> Vec<Block> {
        let styles = Styles::default();
        let numbering = Numbering::default();
        let rels = HashMap::new();
        let media = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            counters: Default::default(),
        };
        parse_document(xml, &ctx)
    }

    fn raw_merge_cell(vmerge: VMerge) -> CellRaw {
        CellRaw {
            blocks: Vec::new(),
            col_span: 1,
            vmerge,
            shading: None,
            valign: VCell::Top,
            width_pct: None,
        }
    }

    #[test]
    fn paragraph_runs_with_emphasis() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>plain </w:t></w:r>
                 <w:r><w:rPr><w:b/></w:rPr><w:t>bold</w:t></w:r>
                 <w:r><w:rPr><w:i/></w:rPr><w:t> ital</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        assert_eq!(p.text(), "plain bold ital");
        assert!(p.runs[1].props.bold);
        assert!(p.runs[2].props.italic);
    }

    #[test]
    fn extracts_textbox_text_once_across_alternate_content() {
        // The same box is serialized as a DrawingML Choice and a VML Fallback; its
        // text must be recovered exactly once.
        let xml = r#"<w:document><w:body>
            <w:p>
                <w:r><w:t>본문 </w:t></w:r>
                <w:r><w:drawing><mc:AlternateContent>
                    <mc:Choice Requires="wps"><wps:wsp><wps:txbx><w:txbxContent>
                        <w:p><w:r><w:t>박스 텍스트</w:t></w:r></w:p>
                    </w:txbxContent></wps:txbx></wps:wsp></mc:Choice>
                    <mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent>
                        <w:p><w:r><w:t>박스 텍스트</w:t></w:r></w:p>
                    </w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback>
                </mc:AlternateContent></w:drawing></w:r>
            </w:p>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para")
        };
        let text = p.text();
        assert!(text.contains("본문"), "body run lost: {text:?}");
        assert!(
            text.contains("박스 텍스트"),
            "textbox text missing: {text:?}"
        );
        assert_eq!(
            text.matches("박스 텍스트").count(),
            1,
            "textbox text double-counted across AlternateContent: {text:?}"
        );
    }

    #[test]
    fn extracts_floating_shape_text_at_run_level() {
        // A floating shape sits as <w:r><mc:AlternateContent> (Choice=DrawingML,
        // Fallback=VML) directly under the run — its text box must be recovered
        // once, not skipped (the previous behavior dropped the whole shape).
        let xml = r#"<w:document><w:body><w:p>
            <w:r><w:t>본문 </w:t></w:r>
            <w:r><mc:AlternateContent>
                <mc:Choice Requires="wps"><w:drawing><wps:wsp><wps:txbx><w:txbxContent>
                    <w:p><w:r><w:t>도형 속 글자</w:t></w:r></w:p>
                </w:txbxContent></wps:txbx></wps:wsp></w:drawing></mc:Choice>
                <mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent>
                    <w:p><w:r><w:t>도형 속 글자</w:t></w:r></w:p>
                </w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback>
            </mc:AlternateContent></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("para")
        };
        let t = p.text();
        assert!(t.contains("본문"), "body lost: {t:?}");
        assert!(t.contains("도형 속 글자"), "shape text missing: {t:?}");
        assert_eq!(
            t.matches("도형 속 글자").count(),
            1,
            "shape text doubled: {t:?}"
        );
    }

    #[test]
    fn scans_header_footer_references() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>x</w:t></w:r></w:p>
            <w:sectPr>
                <w:headerReference w:type="default" r:id="rIdH"/>
                <w:footerReference w:type="default" r:id="rIdF"/>
                <w:headerReference w:type="first" r:id="rIdH1"/>
            </w:sectPr>
        </w:body></w:document>"#;
        let (headers, footers) = scan_hf_refs(xml);
        assert_eq!(headers, vec!["rIdH".to_string(), "rIdH1".to_string()]);
        assert_eq!(footers, vec!["rIdF".to_string()]);
    }

    #[test]
    fn parses_real_notes_skipping_separators() {
        let styles = Styles::default();
        let numbering = Numbering::default();
        let rels = HashMap::new();
        let media = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            counters: Default::default(),
        };
        let xml = r#"<w:footnotes>
            <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:t>SEP</w:t></w:r></w:p></w:footnote>
            <w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:t>CONT</w:t></w:r></w:p></w:footnote>
            <w:footnote w:id="1"><w:p><w:r><w:t>실제 각주 내용</w:t></w:r></w:p></w:footnote>
        </w:footnotes>"#;
        let blocks = parse_notes(xml, &ctx, b"footnote");
        let text: String = blocks
            .iter()
            .filter_map(|b| match b {
                Block::Paragraph(p) => Some(p.text()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("|");
        assert_eq!(text, "실제 각주 내용", "got: {text:?}");
    }

    #[test]
    fn parses_header_part_blocks() {
        let styles = Styles::default();
        let numbering = Numbering::default();
        let rels = HashMap::new();
        let media = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            counters: Default::default(),
        };
        let xml = r#"<w:hdr><w:p><w:r><w:t>헤더 텍스트</w:t></w:r></w:p></w:hdr>"#;
        let blocks = parse_hdrftr(xml, &ctx);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para")
        };
        assert_eq!(p.text(), "헤더 텍스트");
    }

    #[test]
    fn reads_rich_char_para_and_cell_formatting() {
        use crate::model::{Color, VCell, VertAlign};
        let xml = r#"<w:document><w:body>
            <w:p>
                <w:pPr><w:spacing w:before="240" w:after="120" w:line="360"/><w:ind w:left="720" w:firstLine="240"/><w:shd w:fill="EEEEEE"/></w:pPr>
                <w:r><w:rPr><w:rFonts w:ascii="Arial" w:eastAsia="맑은 고딕"/><w:sz w:val="24"/><w:color w:val="FF0000"/><w:vertAlign w:val="superscript"/><w:caps/></w:rPr><w:t>빨강</w:t></w:r>
            </w:p>
            <w:tbl><w:tr><w:tc>
                <w:tcPr><w:shd w:fill="DDDDDD"/><w:vAlign w:val="center"/><w:tcW w:w="2500" w:type="pct"/></w:tcPr>
                <w:p><w:r><w:t>셀</w:t></w:r></w:p>
            </w:tc></w:tr></w:tbl>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para")
        };
        let rp = &p.runs[0].props;
        assert_eq!(rp.font.as_deref(), Some("맑은 고딕")); // eastAsia preferred
        assert_eq!(rp.size_half_pt, Some(24));
        assert_eq!(rp.color, Some(Color { r: 255, g: 0, b: 0 }));
        assert_eq!(rp.vert_align, VertAlign::Super);
        assert!(rp.caps, "w:caps not captured");
        assert_eq!(p.props.spacing.before_pt, Some(12.0));
        assert_eq!(p.props.spacing.after_pt, Some(6.0));
        assert_eq!(p.props.spacing.line_pct, Some(1.5));
        assert_eq!(p.props.indent.left_pt, Some(36.0));
        assert_eq!(p.props.indent.first_line_pt, Some(12.0));
        assert_eq!(
            p.props.shading,
            Some(Color {
                r: 0xEE,
                g: 0xEE,
                b: 0xEE
            })
        );
        let Block::Table(t) = &blocks[1] else {
            panic!("table")
        };
        let c = &t.rows[0].cells[0];
        assert_eq!(
            c.shading,
            Some(Color {
                r: 0xDD,
                g: 0xDD,
                b: 0xDD
            })
        );
        assert_eq!(c.valign, VCell::Center);
        assert_eq!(c.width_pct, Some(0.5));
    }

    #[test]
    fn preserves_significant_whitespace_in_t() {
        let xml = r#"<w:document><w:body><w:p>
            <w:r><w:t xml:space="preserve">a </w:t></w:r><w:r><w:t>b</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("para")
        };
        assert_eq!(p.text(), "a b");
    }

    #[test]
    fn table_gridspan_and_vmerge() {
        // 2x2 grid: row 0 col 0 spans 2 columns (gridSpan) and starts a vertical
        // merge; row 1 col 0 continues it (dropped, owner row_span=2).
        let xml = r#"<w:document><w:body><w:tbl>
            <w:tr>
              <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
              <w:tc><w:tcPr><w:gridSpan w:val="2"/><w:vMerge/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc>
            </w:tr>
        </w:tbl></w:body></w:document>"#;
        let Block::Table(t) = &parse(xml)[0] else {
            panic!("table")
        };
        assert_eq!(t.rows.len(), 2);
        assert_eq!(t.rows[0].cells.len(), 1);
        assert_eq!(t.rows[0].cells[0].col_span, 2);
        assert_eq!(t.rows[0].cells[0].row_span, 2);
        assert_eq!(t.rows[1].cells.len(), 0); // continuation dropped
    }

    #[test]
    fn vertical_merge_row_span_saturates_instead_of_overflowing() {
        let mut rows = Vec::with_capacity(u16::MAX as usize + 1);
        rows.push((vec![raw_merge_cell(VMerge::Restart)], false));
        rows.extend(
            (0..u16::MAX as usize).map(|_| (vec![raw_merge_cell(VMerge::Continue)], false)),
        );

        let table = build_table(rows);

        assert_eq!(table.rows[0].cells[0].row_span, u16::MAX);
    }

    #[test]
    fn block_level_sdt_content_is_not_lost() {
        // A content control (w:sdt) wrapping body paragraphs is a transparent
        // block container — its paragraphs must survive, not be skipped.
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>before</w:t></w:r></w:p>
            <w:sdt><w:sdtPr></w:sdtPr><w:sdtContent>
                <w:p><w:r><w:t>inside_sdt</w:t></w:r></w:p>
                <w:tbl><w:tr><w:tc><w:p><w:r><w:t>cell</w:t></w:r></w:p></w:tc></w:tr></w:tbl>
            </w:sdtContent></w:sdt>
            <w:p><w:r><w:t>after</w:t></w:r></w:p>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let joined = blocks
            .iter()
            .map(|b| match b {
                Block::Paragraph(p) => p.text(),
                Block::Table(t) => t.rows[0].cells[0]
                    .blocks
                    .iter()
                    .filter_map(|b| match b {
                        Block::Paragraph(p) => Some(p.text()),
                        _ => None,
                    })
                    .collect(),
                Block::Image(_) => String::new(),
            })
            .collect::<Vec<_>>()
            .join("|");
        assert_eq!(joined, "before|inside_sdt|cell|after");
    }

    #[test]
    fn deeply_nested_tables_do_not_overflow_the_stack() {
        // Thousands of nested table cells (cf. POI deep-table-cell.docx) must be
        // bounded by MAX_DEPTH and skipped iteratively, not recursed to a crash.
        let depth = 4000;
        let mut xml = String::from("<w:document><w:body>");
        for _ in 0..depth {
            xml.push_str("<w:tbl><w:tr><w:tc>");
        }
        xml.push_str("<w:p><w:r><w:t>deep</w:t></w:r></w:p>");
        for _ in 0..depth {
            xml.push_str("</w:tc></w:tr></w:tbl>");
        }
        xml.push_str("</w:body></w:document>");
        let blocks = parse(&xml); // returns instead of overflowing
        assert!(!blocks.is_empty());
    }

    #[test]
    fn deeply_nested_textboxes_do_not_overflow_the_stack() {
        // The drawing → txbxContent → paragraph → run → drawing … cycle must be
        // bounded by the same MAX_DEPTH budget (threaded across the drawing
        // boundary), not recursed to a stack overflow on hostile input.
        let depth = 4000;
        let mut xml = String::from("<w:document><w:body>");
        for _ in 0..depth {
            xml.push_str("<w:p><w:r><w:drawing><w:txbxContent>");
        }
        xml.push_str("<w:p><w:r><w:t>deep</w:t></w:r></w:p>");
        for _ in 0..depth {
            xml.push_str("</w:txbxContent></w:drawing></w:r></w:p>");
        }
        xml.push_str("</w:body></w:document>");
        let _ = parse(&xml); // must return, not abort
    }

    #[test]
    fn skips_field_and_deletion_but_keeps_body() {
        // w:del (tracked deletion) content must not appear; w:ins must.
        let xml = r#"<w:document><w:body><w:p>
            <w:del><w:r><w:delText>gone</w:delText></w:r></w:del>
            <w:ins><w:r><w:t>kept</w:t></w:r></w:ins>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("para")
        };
        assert_eq!(p.text(), "kept");
    }
}
