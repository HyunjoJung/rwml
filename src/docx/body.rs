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

use super::fields::TocEntry;
use super::numbering::Numbering;
use super::styles::Styles;
use super::{attr_local, local, toggle_on};
use crate::annotation::FieldKind;
use crate::model::{
    Align, AuthoredContentControl, Block, Cell, CellMargins, CharProps, Color, DocGrid,
    DocGridType, FieldRole, Image, Indent, ListInfo, PageNumberFormat, ParaProps, Paragraph, Row,
    Run, SectionBreakKind, SectionSetup, Spacing, Table, TableBorderColors, TableBorderSide,
    TableBorderSizes, TableBorderStyle, TableBorderStyles, TextDirection, VCell, VertAlign,
};
use crate::text;
use crate::CoreProperties;

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
const PAGE_BREAK_MARKER: char = '\u{000C}';

/// Resolved supplementary tables, passed down the descent.
pub(crate) struct Ctx<'a> {
    pub styles: &'a Styles,
    pub numbering: &'a Numbering,
    pub rels: &'a HashMap<String, (String, bool)>,
    pub media: &'a HashMap<String, Image>,
    pub ref_targets: &'a HashMap<String, String>,
    pub ref_position_context: &'a super::fields::RefPositionContext,
    pub ref_number_context: &'a super::fields::RefNumberContext,
    pub page_ref_context: &'a super::fields::PageRefContext,
    pub note_ref_context: &'a super::fields::NoteRefContext,
    pub section_context: &'a super::fields::SectionContext,
    pub style_ref_context: &'a super::fields::StyleRefContext,
    pub legacy_form_context: &'a super::fields::LegacyFormContext,
    pub table_formula_context: &'a super::fields::TableFormulaContext,
    pub toc_entries: &'a [TocEntry],
    pub core_properties: &'a CoreProperties,
    pub custom_properties: &'a HashMap<String, String>,
    pub document_variables: &'a HashMap<String, String>,
    pub extended_properties: &'a HashMap<String, String>,
    pub file_size_bytes: Option<usize>,
    pub ref_field_cursor: std::cell::RefCell<usize>,
    pub page_field_cursor: std::cell::RefCell<usize>,
    pub page_ref_field_cursor: std::cell::RefCell<usize>,
    pub note_ref_field_cursor: std::cell::RefCell<usize>,
    pub section_field_cursor: std::cell::RefCell<usize>,
    pub style_ref_field_cursor: std::cell::RefCell<usize>,
    pub form_field_cursor: std::cell::RefCell<usize>,
    pub formula_field_cursor: std::cell::RefCell<usize>,
    pub sequence_counters: std::cell::RefCell<HashMap<String, i64>>,
    pub autonum_counter: std::cell::RefCell<i64>,
    pub listnum_counter: std::cell::RefCell<i64>,
    pub field_bookmarks: std::cell::RefCell<HashMap<String, String>>,
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

/// A header/footer reference declared by a body `sectPr`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HeaderFooterRef {
    /// Relationship id from `r:id`.
    pub rel_id: String,
    /// WordprocessingML reference type: `default`, `first`, or `even`.
    pub type_name: String,
}

/// Header/footer references declared by one body `sectPr`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HeaderFooterRefs {
    pub headers: Vec<HeaderFooterRef>,
    pub footers: Vec<HeaderFooterRef>,
}

/// Scan `word/document.xml` for every `<w:headerReference>` / `<w:footerReference>`
/// relationship id and reference type (in document order, across all `w:sectPr`).
/// Returns `(header refs, footer refs)`; the caller resolves and de-duplicates
/// them.
#[cfg(test)]
pub(crate) fn scan_hf_refs(xml: &str) -> (Vec<HeaderFooterRef>, Vec<HeaderFooterRef>) {
    let sections = scan_hf_ref_sections(xml);
    let mut headers = Vec::new();
    let mut footers = Vec::new();
    for section in sections {
        headers.extend(section.headers);
        footers.extend(section.footers);
    }
    (headers, footers)
}

/// Scan `word/document.xml` for header/footer references grouped by each
/// `sectPr` in document order. Paragraph-level groups correspond to emitted
/// `Block::SectionBreak` nodes; the trailing body-level group describes the
/// final document setup.
pub(crate) fn scan_hf_ref_sections(xml: &str) -> Vec<HeaderFooterRefs> {
    let mut r = Reader::from_str(xml);
    let mut sections = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                sections.push(read_hf_ref_section(&mut r));
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                sections.push(HeaderFooterRefs::default());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    sections
}

fn read_hf_ref_section(r: &mut Xml<'_>) -> HeaderFooterRefs {
    let mut refs = HeaderFooterRefs::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"headerReference" => {
                    if let Some(reference) = header_footer_ref(&e) {
                        refs.headers.push(reference);
                    }
                    skip_subtree(r);
                }
                b"footerReference" => {
                    if let Some(reference) = header_footer_ref(&e) {
                        refs.footers.push(reference);
                    }
                    skip_subtree(r);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"headerReference" => {
                    if let Some(reference) = header_footer_ref(&e) {
                        refs.headers.push(reference);
                    }
                }
                b"footerReference" => {
                    if let Some(reference) = header_footer_ref(&e) {
                        refs.footers.push(reference);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    refs
}

fn header_footer_ref(e: &BytesStart<'_>) -> Option<HeaderFooterRef> {
    attr_local(e, b"id").map(|rel_id| HeaderFooterRef {
        rel_id,
        type_name: attr_local(e, b"type").unwrap_or_else(|| "default".to_string()),
    })
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

/// Scan the final/body section properties for text column count.
pub(crate) fn scan_section_columns(xml: &str) -> Option<u16> {
    let mut r = Reader::from_str(xml);
    let mut columns = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                columns = read_section_columns(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                columns = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    columns
}

/// Scan the final/body section properties for text flow direction.
pub(crate) fn scan_section_text_direction(xml: &str) -> Option<TextDirection> {
    let mut r = Reader::from_str(xml);
    let mut text_direction = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                text_direction = read_section_text_direction(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                text_direction = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text_direction
}

/// Scan the final/body section properties for document grid settings.
pub(crate) fn scan_section_doc_grid(xml: &str) -> Option<DocGrid> {
    let mut r = Reader::from_str(xml);
    let mut doc_grid = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                doc_grid = read_section_doc_grid(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                doc_grid = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    doc_grid
}

/// Scan the final/body section properties for explicit first-page section behavior.
pub(crate) fn scan_section_title_page(xml: &str) -> bool {
    let mut r = Reader::from_str(xml);
    let mut title_page = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                title_page = read_section_title_page(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                title_page = false;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    title_page
}

/// Scan the final/body section properties for a displayed page-number restart.
pub(crate) fn scan_page_number_start(xml: &str) -> Option<u32> {
    let mut r = Reader::from_str(xml);
    let mut page_number_start = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                page_number_start = read_section_page_number_start(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                page_number_start = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_start
}

/// Scan the final/body section properties for a displayed page-number format.
pub(crate) fn scan_page_number_format(xml: &str) -> Option<PageNumberFormat> {
    let mut r = Reader::from_str(xml);
    let mut page_number_format = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPr" => {
                page_number_format = read_section_page_number_format(&mut r);
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"sectPr" => {
                page_number_format = None;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_format
}

fn read_section_columns(r: &mut Xml<'_>) -> Option<u16> {
    let mut columns = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"cols" => {
                columns = attr_local(&e, b"num")
                    .and_then(|v| v.trim().parse::<u16>().ok())
                    .map(|value| value.max(1));
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    columns
}

fn read_section_text_direction(r: &mut Xml<'_>) -> Option<TextDirection> {
    let mut text_direction = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"textDirection" =>
            {
                text_direction =
                    attr_local(&e, b"val").and_then(|value| TextDirection::from_wml_value(&value));
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text_direction
}

fn doc_grid_from_attrs(e: &BytesStart<'_>) -> Option<DocGrid> {
    let grid_type = attr_local(e, b"type")
        .and_then(|value| DocGridType::from_wml_value(&value))
        .unwrap_or(DocGridType::Default);
    let line_pitch = attr_local(e, b"linePitch").and_then(|v| v.trim().parse::<u32>().ok());
    let character_space = attr_local(e, b"charSpace").and_then(|v| v.trim().parse::<u32>().ok());
    Some(DocGrid {
        grid_type,
        line_pitch,
        character_space,
    })
}

fn read_section_doc_grid(r: &mut Xml<'_>) -> Option<DocGrid> {
    let mut doc_grid = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"docGrid" => {
                doc_grid = doc_grid_from_attrs(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    doc_grid
}

fn read_section_title_page(r: &mut Xml<'_>) -> bool {
    let mut title_page = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"titlePg" => {
                title_page = true;
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    title_page
}

fn read_section_page_number_start(r: &mut Xml<'_>) -> Option<u32> {
    let mut page_number_start = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_start = attr_local(&e, b"start")
                    .and_then(|v| v.trim().parse::<u32>().ok())
                    .map(|value| value.max(1));
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_start
}

fn read_section_page_number_format(r: &mut Xml<'_>) -> Option<PageNumberFormat> {
    let mut page_number_format = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_format = attr_local(&e, b"fmt")
                    .and_then(|value| PageNumberFormat::from_wml_value(&value));
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_format
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
#[cfg(test)]
pub(crate) fn parse_notes(xml: &str, ctx: &Ctx<'_>, tag: &[u8]) -> Vec<Block> {
    parse_note_entries(xml, ctx, tag)
        .into_iter()
        .flat_map(|(_, blocks)| blocks)
        .collect()
}

/// Parse `word/footnotes.xml` / `endnotes.xml` into individual real note
/// entries. Each entry keeps the OOXML note id plus the block content parsed
/// with the same grammar as the flattened note reader.
pub(crate) fn parse_note_entries(
    xml: &str,
    ctx: &Ctx<'_>,
    tag: &[u8],
) -> Vec<(String, Vec<Block>)> {
    let mut r = Reader::from_str(xml);
    let mut entries = Vec::new();
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
                    let id = attr_local(&e, b"id").unwrap_or_default();
                    entries.push((id, read_blocks(&mut r, ctx, 0)));
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    entries
}

/// Scan `word/document.xml` for note reference ids and the containing top-level
/// body block text. `tag` is `b"footnoteReference"` or `b"endnoteReference"`.
pub(crate) fn scan_note_ref_anchors(xml: &str, tag: &[u8]) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut anchors = HashMap::new();
    let mut in_body = false;
    let mut body_depth = 0usize;
    let mut body_block_candidate_depths = vec![0usize];
    let mut current_block_depth = None;
    let mut current_block_text = String::new();
    let mut current_block_refs = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"body" {
                    in_body = true;
                    body_depth = 0;
                    body_block_candidate_depths.clear();
                    body_block_candidate_depths.push(0);
                    current_block_depth = None;
                    current_block_text.clear();
                    current_block_refs.clear();
                    continue;
                }
                if in_body {
                    if current_block_depth.is_none()
                        && body_block_candidate_depths.contains(&body_depth)
                        && is_note_anchor_transparent_body_container(name)
                    {
                        body_block_candidate_depths.push(body_depth + 1);
                    }
                    if current_block_depth.is_none()
                        && body_block_candidate_depths.contains(&body_depth)
                        && is_note_anchor_body_block(name)
                    {
                        current_block_depth = Some(body_depth + 1);
                        current_block_text.clear();
                        current_block_refs.clear();
                    }
                    body_depth += 1;
                }
                if current_block_depth.is_some() {
                    if name == tag {
                        if let Some(id) = attr_local(&e, b"id") {
                            current_block_refs.push(id);
                        }
                        skip_subtree(&mut r);
                        body_depth = body_depth.saturating_sub(1);
                    } else if name == b"t" {
                        current_block_text.push_str(&read_text(&mut r));
                        body_depth = body_depth.saturating_sub(1);
                    } else if is_note_anchor_embedded_body(name) {
                        skip_subtree(&mut r);
                        body_depth = body_depth.saturating_sub(1);
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if current_block_depth.is_some() {
                    if name == tag {
                        if let Some(id) = attr_local(&e, b"id") {
                            current_block_refs.push(id);
                        }
                    } else {
                        append_note_anchor_empty_marker(&mut current_block_text, name);
                    }
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"body" {
                    in_body = false;
                    body_depth = 0;
                    body_block_candidate_depths.clear();
                    body_block_candidate_depths.push(0);
                    current_block_depth = None;
                    current_block_text.clear();
                    current_block_refs.clear();
                    continue;
                }
                if in_body {
                    let ending_current_block = current_block_depth == Some(body_depth);
                    if ending_current_block {
                        insert_note_anchor_block(
                            &mut anchors,
                            &current_block_refs,
                            &current_block_text,
                        );
                    }
                    if body_block_candidate_depths.last().copied() == Some(body_depth) {
                        body_block_candidate_depths.pop();
                    }
                    body_depth = body_depth.saturating_sub(1);
                    if ending_current_block {
                        current_block_depth = None;
                        current_block_text.clear();
                        current_block_refs.clear();
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    anchors
}

fn is_note_anchor_body_block(name: &[u8]) -> bool {
    matches!(name, b"p" | b"tbl")
}

fn is_note_anchor_transparent_body_container(name: &[u8]) -> bool {
    matches!(
        name,
        b"sdt" | b"sdtContent" | b"customXml" | b"smartTag" | b"ins" | b"moveTo"
    )
}

fn is_note_anchor_embedded_body(name: &[u8]) -> bool {
    matches!(name, b"drawing" | b"pict" | b"object" | b"AlternateContent")
}

fn append_note_anchor_empty_marker(out: &mut String, name: &[u8]) {
    match name {
        b"tab" => out.push('\t'),
        b"br" | b"cr" => out.push('\n'),
        b"noBreakHyphen" => out.push('-'),
        _ => {}
    }
}

fn insert_note_anchor_block(
    anchors: &mut HashMap<String, String>,
    refs: &[String],
    raw_text: &str,
) {
    if refs.is_empty() {
        return;
    }
    let text = text::finalize(raw_text);
    for id in refs {
        anchors.entry(id.clone()).or_insert_with(|| text.clone());
    }
}

/// Parse visible `w:txbxContent` text boxes from `word/document.xml`, using the
/// same block parser and `mc:AlternateContent` first-branch policy as the flat
/// body reader.
pub(crate) fn parse_text_boxes(xml: &str, ctx: &Ctx<'_>) -> Vec<String> {
    let mut r = Reader::from_str(xml);
    let mut text_boxes = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"drawing" | b"pict" | b"object" => {
                    walk_text_box_drawing(&mut r, ctx, &mut text_boxes, 0)
                }
                b"AlternateContent" => {
                    walk_text_box_alternate_content(&mut r, ctx, &mut text_boxes, 0)
                }
                b"txbxContent" => {
                    let blocks = read_blocks(&mut r, ctx, 1);
                    let text = blocks_text(&blocks);
                    if !text.trim().is_empty() {
                        text_boxes.push(text);
                    }
                }
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text_boxes
}

fn walk_text_box_drawing(r: &mut Xml<'_>, ctx: &Ctx<'_>, text_boxes: &mut Vec<String>, depth: u32) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"txbxContent" => {
                    if depth < MAX_DEPTH {
                        let blocks = read_blocks(r, ctx, depth + 1);
                        let text = blocks_text(&blocks);
                        if !text.trim().is_empty() {
                            text_boxes.push(text);
                        }
                    } else {
                        skip_subtree(r);
                    }
                }
                b"AlternateContent" => {
                    walk_text_box_alternate_content(r, ctx, text_boxes, depth + 1)
                }
                _ => {
                    if depth < MAX_DEPTH {
                        walk_text_box_drawing(r, ctx, text_boxes, depth + 1);
                    } else {
                        skip_subtree(r);
                    }
                }
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn walk_text_box_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    text_boxes: &mut Vec<String>,
    depth: u32,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    if depth < MAX_DEPTH {
                        walk_text_box_drawing(r, ctx, text_boxes, depth + 1);
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

/// Read block-level children (`w:p`, `w:tbl`) until the enclosing `End`. Block
/// content controls (`w:sdt`/`w:sdtContent`), `w:customXml`, `w:smartTag`, and
/// accepted-current revision wrappers (`w:ins`/`w:moveTo`) are transparent
/// containers — descended into so their paragraphs/tables aren't lost.
fn read_blocks(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Block> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut blocks = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"p" => blocks.extend(read_paragraph_blocks(r, ctx, depth + 1)),
                b"tbl" => {
                    let t = read_table(r, ctx, depth + 1);
                    if !t.rows.is_empty() {
                        blocks.push(Block::Table(t));
                    }
                }
                b"sdt" => blocks.extend(read_content_control_blocks(r, ctx, depth + 1)),
                b"sdtContent" | b"customXml" | b"smartTag" | b"ins" | b"moveTo" => {
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

fn read_content_control_blocks(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Block> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut control = None;
    let mut blocks = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"sdtPr" => control = read_content_control_pr(r),
                b"sdtContent" => blocks.extend(read_blocks(r, ctx, depth + 1)),
                b"p" => blocks.extend(read_paragraph_blocks(r, ctx, depth + 1)),
                b"tbl" => {
                    let table = read_table(r, ctx, depth + 1);
                    if !table.rows.is_empty() {
                        blocks.push(Block::Table(table));
                    }
                }
                b"sdt" => blocks.extend(read_content_control_blocks(r, ctx, depth + 1)),
                b"customXml" | b"smartTag" | b"ins" | b"moveTo" => {
                    blocks.extend(read_blocks(r, ctx, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apply_content_control_to_blocks(&mut blocks, control);
    blocks
}

fn apply_content_control_to_blocks(blocks: &mut [Block], control: Option<AuthoredContentControl>) {
    let Some(control) = control else {
        return;
    };
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                apply_content_control(&mut paragraph.runs, Some(control.clone()));
            }
            Block::Table(table) => {
                for row in &mut table.rows {
                    for cell in &mut row.cells {
                        apply_content_control_to_blocks(&mut cell.blocks, Some(control.clone()));
                    }
                }
            }
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
        }
    }
}

/// Read a `<w:p>`: its `w:pPr` properties and inline runs.
fn read_paragraph(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> (Paragraph, Option<SectionSetup>) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return (Paragraph::default(), None);
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut pp = PPr::default();
    let mut complex_field = ComplexFieldTracker::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"pPr" => pp = read_ppr(r),
                b"r" => {
                    let next = read_run(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        Some(&mut complex_field),
                        runs.len(),
                    );
                    runs.extend(next);
                    complex_field.apply_pending(&mut runs);
                }
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    None,
                    depth + 1,
                    &mut runs,
                    &mut complex_field,
                ),
                b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo" | b"dir" => {
                    append_runs_container_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        &mut runs,
                        &mut complex_field,
                    )
                }
                // `w:del` = tracked deletion (removed text) → drop.
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let section = pp.section.take();
    (finalize_paragraph(runs, pp, ctx), section)
}

fn read_paragraph_blocks(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Block> {
    let (paragraph, section) = read_paragraph(r, ctx, depth);
    let mut blocks = split_page_breaks(paragraph);
    if let Some(mut section) = section {
        if section.section_break.is_none() {
            section.section_break = Some(SectionBreakKind::NextPage);
        }
        blocks.push(Block::SectionBreak(section));
    }
    blocks
}

fn split_page_breaks(paragraph: Paragraph) -> Vec<Block> {
    if !paragraph
        .runs
        .iter()
        .any(|run| run.text.contains(PAGE_BREAK_MARKER))
    {
        return if paragraph.is_blank() {
            Vec::new()
        } else {
            vec![Block::Paragraph(paragraph)]
        };
    }

    let props = paragraph.props;
    let mut blocks = Vec::new();
    let mut current = Paragraph {
        props: props.clone(),
        runs: Vec::new(),
    };
    for run in paragraph.runs {
        if !run.text.contains(PAGE_BREAK_MARKER) {
            current.runs.push(run);
            continue;
        }
        let parts: Vec<_> = run
            .text
            .split(PAGE_BREAK_MARKER)
            .map(str::to_owned)
            .collect();
        for (index, part) in parts.into_iter().enumerate() {
            if index > 0 {
                if !current.is_blank() {
                    blocks.push(Block::Paragraph(std::mem::replace(
                        &mut current,
                        Paragraph {
                            props: props.clone(),
                            runs: Vec::new(),
                        },
                    )));
                } else {
                    current.runs.clear();
                }
                blocks.push(Block::PageBreak);
            }
            if !part.is_empty() {
                let mut split_run = run.clone();
                split_run.text = part;
                current.runs.push(split_run);
            }
        }
    }
    if !current.is_blank() {
        blocks.push(Block::Paragraph(current));
    }
    blocks
}

#[derive(Default)]
struct ComplexFieldTracker {
    instruction: String,
    phase: Option<ComplexFieldPhase>,
    result_runs: Vec<usize>,
    result_start: Option<usize>,
    pending: Option<PendingComplexField>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ComplexFieldPhase {
    Instruction,
    Result,
}

struct PendingComplexField {
    instruction: String,
    text: Option<String>,
    result_runs: Vec<usize>,
    insert_at: usize,
}

impl ComplexFieldTracker {
    fn begin(&mut self) {
        self.instruction.clear();
        self.result_runs.clear();
        self.result_start = None;
        self.phase = Some(ComplexFieldPhase::Instruction);
        self.pending = None;
    }

    fn separate(&mut self, index: usize) {
        if self.phase.is_some() {
            self.phase = Some(ComplexFieldPhase::Result);
            self.result_start = Some(index);
        }
    }

    fn end(&mut self, ctx: &Ctx<'_>, index: usize) {
        if self.phase.is_some() {
            let instruction = normalize_field_instruction(&self.instruction);
            if !instruction.is_empty() {
                let has_result_runs = !self.result_runs.is_empty();
                let current_result = if has_result_runs { "\u{0}" } else { "" };
                let text = computed_simple_field_result(&instruction, ctx, current_result);
                let insert_at = self.result_start.unwrap_or(index);
                let should_apply = has_result_runs || text.is_some();
                self.pending = Some(PendingComplexField {
                    text,
                    instruction,
                    result_runs: std::mem::take(&mut self.result_runs),
                    insert_at,
                })
                .filter(|_| should_apply);
            }
        }
        self.instruction.clear();
        self.result_runs.clear();
        self.result_start = None;
        self.phase = None;
    }

    fn push_instruction(&mut self, text: &str) {
        if self.phase == Some(ComplexFieldPhase::Instruction) {
            self.instruction.push_str(text);
        }
    }

    fn in_result(&self) -> bool {
        self.phase == Some(ComplexFieldPhase::Result)
    }

    fn push_result_run(&mut self, index: usize) {
        if self.in_result() {
            self.result_runs.push(index);
        }
    }

    fn apply_pending(&mut self, runs: &mut Vec<Run>) {
        let Some(computed) = self.pending.take() else {
            return;
        };
        if computed.result_runs.is_empty() {
            if let Some(text) = computed.text {
                runs.insert(computed.insert_at.min(runs.len()), computed_field_run(text));
            }
            return;
        }
        for (offset, index) in computed.result_runs.iter().copied().enumerate() {
            let Some(run) = runs.get_mut(index) else {
                continue;
            };
            if computed.text.is_some() {
                run.field = FieldRole::Other;
            } else if offset == 0 {
                run.field = FieldRole::Simple {
                    instruction: computed.instruction.clone(),
                };
            }
            if let Some(text) = computed.text.as_deref() {
                run.text = if offset == 0 {
                    text.to_string()
                } else {
                    String::new()
                };
            }
        }
    }
}

fn computed_field_run(text: String) -> Run {
    Run {
        text,
        props: CharProps::default(),
        field: FieldRole::Other,
        field_dirty: false,
        image: None,
        comment: None,
        revision: None,
        content_control: None,
        bookmark: None,
        note: None,
    }
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
    page_break_before: bool,
    section: Option<SectionSetup>,
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
                b"r" => runs.extend(read_run(r, ctx, link, depth + 1, None, 0)),
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"sdt" => runs.extend(read_content_control_runs(r, ctx, link, depth + 1)),
                b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo" | b"dir" => {
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

fn append_runs_container_with_complex(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
    runs: &mut Vec<Run>,
    complex_field: &mut ComplexFieldTracker,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"r" => {
                    let next = read_run(r, ctx, link, depth + 1, Some(complex_field), runs.len());
                    runs.extend(next);
                    complex_field.apply_pending(runs);
                }
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo" | b"dir" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_content_control_runs(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
) -> Vec<Run> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut control = None;
    let mut runs = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"sdtPr" => control = read_content_control_pr(r),
                b"sdtContent" => runs.extend(read_runs_container(r, ctx, link, depth + 1)),
                b"r" => runs.extend(read_run(r, ctx, link, depth + 1, None, 0)),
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"sdt" => runs.extend(read_content_control_runs(r, ctx, link, depth + 1)),
                b"ins" | b"moveTo" | b"smartTag" | b"bdo" | b"dir" => {
                    runs.extend(read_runs_container(r, ctx, link, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apply_content_control(&mut runs, control);
    runs
}

fn append_content_control_runs_with_complex(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
    runs: &mut Vec<Run>,
    complex_field: &mut ComplexFieldTracker,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    let start = runs.len();
    let mut control = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"sdtPr" => control = read_content_control_pr(r),
                b"sdtContent" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                b"r" => {
                    let next = read_run(r, ctx, link, depth + 1, Some(complex_field), runs.len());
                    runs.extend(next);
                    complex_field.apply_pending(runs);
                }
                b"hyperlink" => runs.extend(read_hyperlink(r, &e, ctx, depth)),
                b"fldSimple" => runs.extend(read_fldsimple(r, &e, ctx, depth)),
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                b"ins" | b"moveTo" | b"smartTag" | b"bdo" | b"dir" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apply_content_control(&mut runs[start..], control);
}

fn read_content_control_pr(r: &mut Xml<'_>) -> Option<AuthoredContentControl> {
    let mut control = AuthoredContentControl::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                read_content_control_pr_item(&mut control, &e);
                skip_subtree(r);
            }
            Ok(Event::Empty(e)) => read_content_control_pr_item(&mut control, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sdtPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    content_control_if_present(control)
}

fn read_content_control_pr_item(control: &mut AuthoredContentControl, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"alias" => control.alias = attr_local(e, b"val"),
        b"tag" => control.tag = attr_local(e, b"val"),
        b"dataBinding" => {
            control.data_binding_xpath = attr_local(e, b"xpath");
            control.data_binding_store_item_id = attr_local(e, b"storeItemID");
        }
        _ => {}
    }
}

fn content_control_if_present(control: AuthoredContentControl) -> Option<AuthoredContentControl> {
    (control.alias.is_some()
        || control.tag.is_some()
        || control.data_binding_xpath.is_some()
        || control.data_binding_store_item_id.is_some())
    .then_some(control)
}

fn apply_content_control(runs: &mut [Run], control: Option<AuthoredContentControl>) {
    let Some(control) = control else {
        return;
    };
    for run in runs {
        if run.content_control.is_none() {
            run.content_control = Some(control.clone());
        }
    }
}

/// Read `<w:pPr>` properties (flattening `w:numPr`'s `w:ilvl`/`w:numId`).
fn read_ppr(r: &mut Xml<'_>) -> PPr {
    let mut pp = PPr::default();
    let mut num_id: Option<String> = None;
    let mut ilvl: u8 = 0;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                if local(e.name().as_ref()) == b"sectPr" {
                    pp.section = Some(read_sect_pr(r));
                } else {
                    read_ppr_item(&mut pp, &e, &mut num_id, &mut ilvl);
                }
            }
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"sectPr" {
                    pp.section = Some(SectionSetup::default());
                } else {
                    read_ppr_item(&mut pp, &e, &mut num_id, &mut ilvl);
                }
            }
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

fn read_ppr_item(pp: &mut PPr, e: &BytesStart<'_>, num_id: &mut Option<String>, ilvl: &mut u8) {
    match local(e.name().as_ref()) {
        b"pStyle" => pp.style_id = attr_local(e, b"val"),
        b"ilvl" => {
            if let Some(v) = attr_local(e, b"val").and_then(|v| v.parse().ok()) {
                *ilvl = v;
            }
        }
        b"numId" => *num_id = attr_local(e, b"val"),
        b"jc" => pp.jc = attr_local(e, b"val"),
        b"outlineLvl" => pp.outline = attr_local(e, b"val").and_then(|v| v.parse().ok()),
        b"pageBreakBefore" => pp.page_break_before = true,
        b"spacing" => {
            pp.spacing.before_pt = attr_local(e, b"before").and_then(|v| twips_to_pt(&v));
            pp.spacing.after_pt = attr_local(e, b"after").and_then(|v| twips_to_pt(&v));
            // `w:line` is 240ths of a line when lineRule is auto/absent.
            let exact = matches!(
                attr_local(e, b"lineRule").as_deref(),
                Some("exact") | Some("atLeast")
            );
            if !exact {
                pp.spacing.line_pct = attr_local(e, b"line")
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .map(|l| l / 240.0);
            }
        }
        b"ind" => {
            pp.indent.left_pt = attr_local(e, b"left")
                .or_else(|| attr_local(e, b"start"))
                .and_then(|v| twips_to_pt(&v));
            pp.indent.right_pt = attr_local(e, b"right")
                .or_else(|| attr_local(e, b"end"))
                .and_then(|v| twips_to_pt(&v));
            pp.indent.first_line_pt = attr_local(e, b"firstLine").and_then(|v| twips_to_pt(&v));
            pp.indent.hanging_pt = attr_local(e, b"hanging").and_then(|v| twips_to_pt(&v));
        }
        b"shd" => pp.shading = attr_local(e, b"fill").and_then(|v| parse_hex_color(&v)),
        _ => {}
    }
}

fn read_sect_pr(r: &mut Xml<'_>) -> SectionSetup {
    let mut section = SectionSetup::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"pgSz" => {
                    if let (Some(w), Some(h)) = (
                        attr_local(&e, b"w").and_then(|v| twips_to_pt(&v)),
                        attr_local(&e, b"h").and_then(|v| twips_to_pt(&v)),
                    ) {
                        section.page.width_pt = w;
                        section.page.height_pt = h;
                        section.page.landscape =
                            attr_local(&e, b"orient").as_deref() == Some("landscape");
                    }
                }
                b"type" => {
                    section.section_break = attr_local(&e, b"val")
                        .and_then(|value| SectionBreakKind::from_wml_value(&value));
                }
                b"pgMar" => {
                    let l = attr_local(&e, b"left").and_then(|v| twips_to_pt(&v));
                    let rr = attr_local(&e, b"right").and_then(|v| twips_to_pt(&v));
                    let t = attr_local(&e, b"top").and_then(|v| twips_to_pt(&v));
                    let b = attr_local(&e, b"bottom").and_then(|v| twips_to_pt(&v));
                    if let Some(l) = l {
                        section.page.margin_pt = l;
                    }
                    section.page.margin_left_pt = l;
                    section.page.margin_right_pt = rr;
                    section.page.margin_top_pt = t;
                    section.page.margin_bottom_pt = b;
                }
                b"pgNumType" => {
                    section.page_number_start = attr_local(&e, b"start")
                        .and_then(|v| v.trim().parse::<u32>().ok())
                        .map(|value| value.max(1));
                    section.page_number_format = attr_local(&e, b"fmt")
                        .and_then(|value| PageNumberFormat::from_wml_value(&value));
                }
                b"cols" => {
                    section.columns = attr_local(&e, b"num")
                        .and_then(|v| v.trim().parse::<u16>().ok())
                        .map(|value| value.max(1));
                }
                b"textDirection" => {
                    section.text_direction = attr_local(&e, b"val")
                        .and_then(|value| TextDirection::from_wml_value(&value));
                }
                b"docGrid" => {
                    section.doc_grid = doc_grid_from_attrs(&e);
                }
                b"titlePg" => {
                    section.title_page = true;
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    section
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
            field_dirty: false,
            image: Some(img),
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
        });
    }
    if !txbx.trim().is_empty() {
        images.push(Run {
            text: txbx,
            props: CharProps::default(),
            field: FieldRole::None,
            field_dirty: false,
            image: None,
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
        });
    }
}

fn read_run(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
    mut complex_field: Option<&mut ComplexFieldTracker>,
    base_index: usize,
) -> Vec<Run> {
    // A run can recurse back into block content through a drawing's text box
    // (drawing → txbxContent → paragraph → run → drawing …); `depth` threads the
    // structural recursion budget across that boundary so MAX_DEPTH bounds it.
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut props = CharProps::default();
    let mut text = String::new();
    let mut text_is_field_result = false;
    let mut images: Vec<Run> = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"rPr" => props = read_rpr(r),
                b"fldChar" => {
                    apply_complex_field_char(&e, ctx, complex_field.as_deref_mut(), base_index);
                    skip_subtree(r);
                }
                b"instrText" => {
                    let instruction = read_text(r);
                    if let Some(tracker) = complex_field.as_deref_mut() {
                        tracker.push_instruction(&instruction);
                    }
                }
                b"t" => {
                    let in_result = complex_field
                        .as_deref()
                        .map(ComplexFieldTracker::in_result)
                        .unwrap_or(false);
                    if in_result {
                        text_is_field_result = true;
                    }
                    text.push_str(&read_text(r));
                }
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
                    let mut anchor = DrawingAnchorOffset::default();
                    walk_alternate_content(r, ctx, &mut img, &mut txbx, &mut anchor, depth + 1);
                    push_drawing_runs(&mut images, img, txbx);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"fldChar" => {
                    apply_complex_field_char(&e, ctx, complex_field.as_deref_mut(), base_index)
                }
                b"tab" => text.push('\t'),
                b"br" => {
                    if matches!(attr_local(&e, b"type").as_deref(), Some("page")) {
                        text.push(PAGE_BREAK_MARKER);
                    } else {
                        text.push('\n');
                    }
                }
                b"cr" => text.push('\n'),
                b"noBreakHyphen" => text.push('-'),
                _ => {}
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut runs = Vec::new();
    if !text.is_empty() {
        if text_is_field_result {
            if let Some(tracker) = complex_field {
                tracker.push_result_run(base_index + runs.len());
            }
        }
        runs.push(Run {
            text,
            props,
            field: link
                .map(|u| FieldRole::Hyperlink { url: u.to_string() })
                .unwrap_or(FieldRole::None),
            field_dirty: false,
            image: None,
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
        });
    }
    runs.extend(images);
    runs
}

fn apply_complex_field_char(
    e: &BytesStart<'_>,
    ctx: &Ctx<'_>,
    tracker: Option<&mut ComplexFieldTracker>,
    index: usize,
) {
    let Some(tracker) = tracker else {
        return;
    };
    match attr_local(e, b"fldCharType").as_deref() {
        Some("begin") => tracker.begin(),
        Some("separate") => tracker.separate(index),
        Some("end") => tracker.end(ctx, index),
        _ => {}
    }
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
    let mut anchor = DrawingAnchorOffset::default();
    // Start from the caller's structural depth (not 0) so the recursion budget is
    // continuous across the drawing/text-box boundary.
    walk_drawing(r, ctx, &mut img, &mut text, &mut anchor, depth);
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
    anchor: &mut DrawingAnchorOffset,
    depth: u32,
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"anchor" => {
                    let previous_anchor = *anchor;
                    let had_image = img.is_some();
                    *anchor = DrawingAnchorOffset {
                        active: true,
                        ..DrawingAnchorOffset::default()
                    };
                    if depth < MAX_DEPTH {
                        walk_drawing(r, ctx, img, text, anchor, depth + 1);
                    } else {
                        skip_subtree(r);
                    }
                    if !had_image {
                        apply_floating_anchor_offset(img, anchor);
                    }
                    *anchor = previous_anchor;
                }
                b"positionH" => {
                    anchor.horizontal_page_offset_emu = read_page_position_offset(r, &e);
                }
                b"positionV" => {
                    anchor.vertical_page_offset_emu = read_page_position_offset(r, &e);
                }
                b"txbxContent" => {
                    if depth < MAX_DEPTH {
                        let blocks = read_blocks(r, ctx, depth + 1);
                        append_blocks_text(text, &blocks);
                    } else {
                        skip_subtree(r);
                    }
                }
                b"AlternateContent" => walk_alternate_content(r, ctx, img, text, anchor, depth + 1),
                _ => {
                    if local(e.name().as_ref()) == b"xfrm" {
                        apply_image_rotation(img, &e);
                    }
                    if img.is_none() {
                        *img = blip_image(&e, ctx);
                        apply_floating_anchor_offset(img, anchor);
                    }
                    if depth < MAX_DEPTH {
                        walk_drawing(r, ctx, img, text, anchor, depth + 1);
                    } else {
                        skip_subtree(r);
                    }
                }
            },
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"xfrm" {
                    apply_image_rotation(img, &e);
                }
                if img.is_none() {
                    *img = blip_image(&e, ctx);
                    apply_floating_anchor_offset(img, anchor);
                }
            }
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct DrawingAnchorOffset {
    active: bool,
    horizontal_page_offset_emu: Option<i64>,
    vertical_page_offset_emu: Option<i64>,
}

fn read_page_position_offset(r: &mut Xml<'_>, start: &BytesStart<'_>) -> Option<i64> {
    let page_relative = attr_local(start, b"relativeFrom").as_deref() == Some("page");
    let mut offset = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"posOffset" => {
                if page_relative {
                    offset = read_text(r).trim().parse().ok();
                } else {
                    skip_subtree(r);
                }
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e))
                if matches!(local(e.name().as_ref()), b"positionH" | b"positionV") =>
            {
                break;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    offset
}

fn apply_floating_anchor_offset(img: &mut Option<Image>, anchor: &DrawingAnchorOffset) {
    if !anchor.active {
        return;
    }
    if let (Some(image), Some(x), Some(y)) = (
        img.as_mut(),
        anchor.horizontal_page_offset_emu,
        anchor.vertical_page_offset_emu,
    ) {
        image.floating_offset_emu = Some((x, y));
    }
}

fn apply_image_rotation(img: &mut Option<Image>, e: &BytesStart<'_>) {
    let Some(image) = img.as_mut() else {
        return;
    };
    let Some(rot) = attr_local(e, b"rot").and_then(|rot| rot.parse::<i64>().ok()) else {
        return;
    };
    let units = rot.rem_euclid(21_600_000);
    image.rotation_degrees = Some(((units + 30_000) / 60_000) as i32 % 360);
}

/// `mc:AlternateContent` wraps the SAME box as a `Choice` (DrawingML) and a
/// `Fallback` (VML); descend the first branch only so its text isn't doubled.
fn walk_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    img: &mut Option<Image>,
    text: &mut String,
    anchor: &mut DrawingAnchorOffset,
    depth: u32,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    if depth < MAX_DEPTH {
                        walk_drawing(r, ctx, img, text, anchor, depth + 1);
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
fn blocks_text(blocks: &[Block]) -> String {
    let mut text = String::new();
    append_blocks_text(&mut text, blocks);
    text
}

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
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {
                String::new()
            }
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

/// Read `<w:fldSimple>`: hyperlinks keep link semantics; other simple fields
/// keep their normalized instruction on the cached result runs.
fn read_fldsimple(r: &mut Xml<'_>, start: &BytesStart<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Run> {
    let instruction = attr_local(start, b"instr").unwrap_or_default();
    let url = hyperlink_instr_url(&instruction);
    let mut runs = read_runs_container(r, ctx, url.as_deref(), depth + 1);
    if url.is_none() {
        let instruction = normalize_field_instruction(&instruction);
        if !instruction.is_empty() {
            let current_result = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            let computed = computed_simple_field_result(&instruction, ctx, &current_result);
            if runs.is_empty() {
                if let Some(text) = computed {
                    runs.push(computed_field_run(text));
                }
                return runs;
            }
            for (index, run) in runs.iter_mut().enumerate() {
                if let Some(text) = computed.as_deref() {
                    run.field = FieldRole::Other;
                    run.text = if index == 0 {
                        text.to_string()
                    } else {
                        String::new()
                    };
                } else {
                    run.field = FieldRole::Simple {
                        instruction: instruction.clone(),
                    };
                }
            }
        }
    }
    runs
}

fn computed_simple_field_result(
    instruction: &str,
    ctx: &Ctx<'_>,
    current_result: &str,
) -> Option<String> {
    let (ref_position, note_ref_position) = ref_field_positions(instruction, ctx);
    let ref_result = {
        let field_bookmarks = ctx.field_bookmarks.borrow();
        let ref_ctx = super::fields::RefResultContext {
            bookmarks: ctx.ref_targets,
            ref_positions: ctx.ref_position_context,
            ref_numbers: ctx.ref_number_context,
            note_refs: ctx.note_ref_context,
            field_bookmarks: &field_bookmarks,
        };
        super::fields::computed_ref_result(instruction, &ref_ctx, ref_position, note_ref_position)
    };
    ref_result
        .or_else(|| {
            let position = if FieldKind::from_instruction(instruction) == FieldKind::Page {
                let index = {
                    let mut cursor = ctx.page_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                ctx.page_ref_context.page_field_position(index)
            } else {
                None
            };
            super::fields::computed_page_result(instruction, position)
        })
        .or_else(|| {
            let position = if FieldKind::from_instruction(instruction) == FieldKind::PageRef {
                let index = {
                    let mut cursor = ctx.page_ref_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                ctx.page_ref_context.field_position(index)
            } else {
                None
            };
            super::fields::computed_page_ref_result(instruction, ctx.page_ref_context, position)
        })
        .or_else(|| {
            let position = if FieldKind::from_instruction(instruction) == FieldKind::NoteRef {
                let index = {
                    let mut cursor = ctx.note_ref_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                ctx.note_ref_context.field_position(index)
            } else {
                None
            };
            super::fields::computed_note_ref_result(instruction, ctx.note_ref_context, position)
        })
        .or_else(|| {
            if FieldKind::from_instruction(instruction) == FieldKind::Sequence {
                let mut counters = ctx.sequence_counters.borrow_mut();
                super::fields::computed_sequence_result(instruction, &mut counters)
            } else {
                None
            }
        })
        .or_else(|| super::fields::computed_toc_entry_result(instruction))
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::Numbering(kind)
                    if kind == "AUTONUM" || kind == "AUTONUMLGL" || kind == "AUTONUMOUT"
            ) {
                let mut counter = ctx.autonum_counter.borrow_mut();
                super::fields::computed_numbering_result(instruction, &mut counter)
            } else {
                None
            }
        })
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::Numbering(kind) if kind == "LISTNUM"
            ) {
                let mut counter = ctx.listnum_counter.borrow_mut();
                super::fields::computed_listnum_result(instruction, &mut counter)
            } else {
                None
            }
        })
        .or_else(|| {
            let position = if super::fields::is_section_field_instruction(instruction) {
                let index = {
                    let mut cursor = ctx.section_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                ctx.section_context.field_position(index)
            } else {
                None
            };
            super::fields::computed_section_result(instruction, position)
        })
        .or_else(|| {
            super::fields::computed_revision_number_result(instruction, ctx.core_properties)
        })
        .or_else(|| {
            let position = if super::fields::is_style_ref_field_instruction(instruction) {
                let index = {
                    let mut cursor = ctx.style_ref_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                ctx.style_ref_context.field_position(index)
            } else {
                None
            };
            super::fields::computed_style_ref_result(instruction, ctx.style_ref_context, position)
        })
        .or_else(|| computed_dynamic_field_result(instruction, ctx))
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::Dynamic(kind) if kind == "ASK"
            ) {
                let mut field_bookmarks = ctx.field_bookmarks.borrow_mut();
                super::fields::computed_ask_result(instruction, &mut field_bookmarks)
            } else {
                None
            }
        })
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::Dynamic(kind) if kind == "SET"
            ) {
                let mut field_bookmarks = ctx.field_bookmarks.borrow_mut();
                super::fields::computed_set_result(instruction, &mut field_bookmarks)
            } else {
                None
            }
        })
        .or_else(|| {
            super::fields::computed_document_info_result(
                instruction,
                ctx.core_properties,
                ctx.custom_properties,
                ctx.document_variables,
                ctx.extended_properties,
                ctx.file_size_bytes,
            )
        })
        .or_else(|| super::fields::computed_reference_index_result(instruction))
        .or_else(|| super::fields::computed_display_result(instruction))
        .or_else(|| super::fields::computed_action_result(instruction))
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::FormField(_)
            ) {
                let index = {
                    let mut cursor = ctx.form_field_cursor.borrow_mut();
                    let index = *cursor;
                    *cursor += 1;
                    index
                };
                super::fields::computed_legacy_form_result(
                    instruction,
                    current_result,
                    ctx.legacy_form_context,
                    index,
                )
            } else {
                None
            }
        })
        .or_else(|| {
            let (position, note_ref_position) =
                if super::fields::is_direct_bookmark_ref_field_instruction(instruction) {
                    let index = {
                        let mut cursor = ctx.ref_field_cursor.borrow_mut();
                        let index = *cursor;
                        *cursor += 1;
                        index
                    };
                    (
                        ctx.ref_position_context.field_position(index),
                        ctx.note_ref_context.ref_field_position(index),
                    )
                } else {
                    (None, None)
                };
            let field_bookmarks = ctx.field_bookmarks.borrow();
            let ref_ctx = super::fields::RefResultContext {
                bookmarks: ctx.ref_targets,
                ref_positions: ctx.ref_position_context,
                ref_numbers: ctx.ref_number_context,
                note_refs: ctx.note_ref_context,
                field_bookmarks: &field_bookmarks,
            };
            super::fields::computed_direct_bookmark_ref_result(
                instruction,
                &ref_ctx,
                position,
                note_ref_position,
            )
        })
        .or_else(|| super::fields::computed_toc_result(instruction, ctx.toc_entries))
}

fn computed_dynamic_field_result(instruction: &str, ctx: &Ctx<'_>) -> Option<String> {
    if matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Dynamic(kind) if kind == "="
    ) {
        let index = {
            let mut cursor = ctx.formula_field_cursor.borrow_mut();
            let index = *cursor;
            *cursor += 1;
            index
        };
        if let Some(result) = ctx.table_formula_context.field_result(index) {
            return Some(result);
        }
    }
    super::fields::computed_dynamic_result(instruction)
}

fn ref_field_positions(
    instruction: &str,
    ctx: &Ctx<'_>,
) -> (
    Option<super::fields::RefFieldPosition>,
    Option<super::fields::NoteRefFieldPosition>,
) {
    if FieldKind::from_instruction(instruction) != FieldKind::Ref {
        return (None, None);
    }
    let index = {
        let mut cursor = ctx.ref_field_cursor.borrow_mut();
        let index = *cursor;
        *cursor += 1;
        index
    };
    (
        ctx.ref_position_context.field_position(index),
        ctx.note_ref_context.ref_field_position(index),
    )
}

/// Extract a URL from a `HYPERLINK "…"` field instruction (matches the `.doc`
/// field-code parser).
pub(crate) fn hyperlink_instr_url(instr: &str) -> Option<String> {
    let s = instr.trim();
    let after = s.find("HYPERLINK").map(|i| &s[i + "HYPERLINK".len()..])?;
    let q = after.find('"')?;
    let rest = &after[q + 1..];
    let end = rest.find('"')?;
    let url = rest[..end].trim();
    (!url.is_empty()).then(|| url.to_string())
}

fn normalize_field_instruction(instruction: &str) -> String {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in instruction.chars() {
        if ch == '"' {
            in_quotes = !in_quotes;
            current.push(ch);
        } else if ch.is_whitespace() && !in_quotes {
            if !current.is_empty() {
                parts.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts.join(" ")
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
        page_break_before,
        section: _,
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
            style_id,
            style_name,
            heading_level,
            align,
            outline_level: outline,
            list,
            spacing,
            indent,
            shading,
            page_break_before,
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
    margins: Option<CellMargins>,
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
    let mut fixed_layout = false;
    let mut indent_twips = None;
    let mut align = None;
    let mut width_pct = None;
    let mut border_color = None;
    let mut border_colors = TableBorderColors::default();
    let mut border_size_eighths = None;
    let mut border_sizes = TableBorderSizes::default();
    let mut border_style = None;
    let mut border_styles = TableBorderStyles::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tblPr" => {
                    let tblpr = read_tblpr(r);
                    fixed_layout = tblpr.0;
                    indent_twips = tblpr.1;
                    align = tblpr.2;
                    width_pct = tblpr.3;
                    border_color = tblpr.4;
                    border_colors = tblpr.5;
                    border_size_eighths = tblpr.6;
                    border_sizes = tblpr.7;
                    border_style = tblpr.8;
                    border_styles = tblpr.9;
                }
                b"tr" => rows.push(read_row(r, ctx, depth)),
                _ => skip_subtree(r), // tblGrid, …
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    build_table(
        rows,
        fixed_layout,
        indent_twips,
        align,
        width_pct,
        border_color,
        border_colors,
        border_size_eighths,
        border_sizes,
        border_style,
        border_styles,
    )
}

/// Read `<w:tblPr>` layout metadata.
fn read_tblpr(
    r: &mut Xml<'_>,
) -> (
    bool,
    Option<i32>,
    Option<Align>,
    Option<f32>,
    Option<Color>,
    TableBorderColors,
    Option<u16>,
    TableBorderSizes,
    Option<TableBorderStyle>,
    TableBorderStyles,
) {
    let mut fixed_layout = false;
    let mut indent_twips = None;
    let mut align = None;
    let mut width_pct = None;
    let mut border_color = None;
    let mut border_colors = TableBorderColors::default();
    let mut border_size_eighths = None;
    let mut border_sizes = TableBorderSizes::default();
    let mut border_style = None;
    let mut border_styles = TableBorderStyles::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblW"
                    && attr_local(&e, b"type").as_deref() == Some("pct") =>
            {
                width_pct = attr_local(&e, b"w")
                    .and_then(|v| v.trim().parse::<f32>().ok())
                    .map(|p| p / 5000.0);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblLayout" =>
            {
                fixed_layout = attr_local(&e, b"type").as_deref() == Some("fixed");
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"tblInd" => {
                if matches!(attr_local(&e, b"type").as_deref(), None | Some("dxa")) {
                    indent_twips = attr_local(&e, b"w").and_then(|v| v.trim().parse().ok());
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"jc" => {
                align = match attr_local(&e, b"val").as_deref() {
                    Some("center") => Some(Align::Center),
                    Some("right") => Some(Align::Right),
                    Some("both") => Some(Align::Justify),
                    Some("left") | Some("start") => Some(Align::Left),
                    _ => None,
                };
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblBorders" => {
                let borders = read_tbl_borders(r);
                border_color = borders.0;
                border_colors = borders.1;
                border_size_eighths = borders.2;
                border_sizes = borders.3;
                border_style = borders.4;
                border_styles = borders.5;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tblPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (
        fixed_layout,
        indent_twips,
        align,
        width_pct,
        border_color,
        border_colors,
        border_size_eighths,
        border_sizes,
        border_style,
        border_styles,
    )
}

fn read_tbl_borders(
    r: &mut Xml<'_>,
) -> (
    Option<Color>,
    TableBorderColors,
    Option<u16>,
    TableBorderSizes,
    Option<TableBorderStyle>,
    TableBorderStyles,
) {
    let mut color = None;
    let mut colors = TableBorderColors::default();
    let mut sizes = TableBorderSizes::default();
    let mut styles = TableBorderStyles::default();
    let mut color_seen = false;
    let mut color_consistent = true;
    let mut size = None;
    let mut size_seen = false;
    let mut size_consistent = true;
    let mut style = None;
    let mut style_seen = false;
    let mut style_consistent = true;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let Some(side) = table_border_side(&e) else {
                    continue;
                };
                if let Some(next) = attr_local(&e, b"color").and_then(|v| parse_hex_color(&v)) {
                    colors.set(side, next);
                    color_seen = true;
                    match color {
                        Some(current) if current != next => color_consistent = false,
                        None => color = Some(next),
                        _ => {}
                    }
                }
                if let Some(next) = attr_local(&e, b"sz")
                    .and_then(|v| v.trim().parse::<u16>().ok())
                    .filter(|v| *v > 0)
                {
                    sizes.set(side, next);
                    size_seen = true;
                    match size {
                        Some(current) if current != next => size_consistent = false,
                        None => size = Some(next),
                        _ => {}
                    }
                }
                if let Some(next) =
                    attr_local(&e, b"val").and_then(|v| TableBorderStyle::from_wml_value(&v))
                {
                    styles.set(side, next);
                    style_seen = true;
                    match style {
                        Some(current) if current != next => style_consistent = false,
                        None => style = Some(next),
                        _ => {}
                    }
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tblBorders" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let uniform_color = if color_seen && color_consistent {
        color
    } else {
        None
    };
    let uniform_size = if size_seen && size_consistent {
        size
    } else {
        None
    };
    let uniform_style = if style_seen && style_consistent {
        style
    } else {
        None
    };
    (
        uniform_color,
        colors,
        uniform_size,
        sizes,
        uniform_style,
        styles,
    )
}

fn table_border_side(e: &BytesStart<'_>) -> Option<TableBorderSide> {
    match local(e.name().as_ref()) {
        b"top" => Some(TableBorderSide::Top),
        b"left" => Some(TableBorderSide::Left),
        b"bottom" => Some(TableBorderSide::Bottom),
        b"right" => Some(TableBorderSide::Right),
        b"insideH" => Some(TableBorderSide::InsideHorizontal),
        b"insideV" => Some(TableBorderSide::InsideVertical),
        _ => None,
    }
}

/// Read a `<w:tr>` and whether it is a repeated header row.
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
            margins: None,
        };
    }
    let mut blocks = Vec::new();
    let mut tc: Option<TcPr> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tcPr" => tc = Some(read_tcpr(r)),
                b"p" => blocks.extend(read_paragraph_blocks(r, ctx, depth + 1)),
                b"tbl" => {
                    let t = read_table(r, ctx, depth + 1);
                    if !t.rows.is_empty() {
                        blocks.push(Block::Table(t));
                    }
                }
                b"sdt" | b"sdtContent" | b"customXml" | b"smartTag" | b"ins" | b"moveTo" => {
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
        margins: None,
    });
    CellRaw {
        blocks,
        col_span: tc.gs,
        vmerge: tc.vm,
        shading: tc.shading,
        valign: tc.valign,
        width_pct: tc.width_pct,
        margins: tc.margins,
    }
}

/// Collected `<w:tcPr>` properties.
struct TcPr {
    gs: u16,
    vm: VMerge,
    shading: Option<Color>,
    valign: VCell,
    width_pct: Option<f32>,
    margins: Option<CellMargins>,
}

/// Read `<w:tcPr>` → gridSpan / vMerge / shading / vAlign / width.
fn read_tcpr(r: &mut Xml<'_>) -> TcPr {
    let mut t = TcPr {
        gs: 1,
        vm: VMerge::None,
        shading: None,
        valign: VCell::Top,
        width_pct: None,
        margins: None,
    };
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tcMar" => {
                t.margins = read_tc_mar(r);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_tcpr_child(&mut t, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tcPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    t
}

fn apply_tcpr_child(t: &mut TcPr, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"gridSpan" => {
            if let Some(v) = attr_local(e, b"val").and_then(|v| v.parse::<u16>().ok()) {
                t.gs = v.max(1);
            }
        }
        b"vMerge" => {
            t.vm = match attr_local(e, b"val").as_deref() {
                Some("restart") => VMerge::Restart,
                _ => VMerge::Continue, // present with "continue"/no val
            };
        }
        b"shd" => t.shading = attr_local(e, b"fill").and_then(|v| parse_hex_color(&v)),
        b"vAlign" => {
            t.valign = match attr_local(e, b"val").as_deref() {
                Some("center") => VCell::Center,
                Some("bottom") => VCell::Bottom,
                _ => VCell::Top,
            };
        }
        // `type="pct"` w:w is in fiftieths of a percent (5000 = 100%);
        // `dxa` (twips) is absolute and left as auto here.
        b"tcW" if attr_local(e, b"type").as_deref() == Some("pct") => {
            t.width_pct = attr_local(e, b"w")
                .and_then(|v| v.trim().parse::<f32>().ok())
                .map(|p| p / 5000.0);
        }
        _ => {}
    }
}

fn read_tc_mar(r: &mut Xml<'_>) -> Option<CellMargins> {
    let mut margins = CellMargins::default();
    let mut seen = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                apply_tc_mar_side(&mut margins, &mut seen, &e);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tcMar" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    seen.then_some(margins)
}

fn apply_tc_mar_side(margins: &mut CellMargins, seen: &mut bool, e: &BytesStart<'_>) {
    let name = e.name();
    let side = local(name.as_ref());
    if !matches!(side, b"top" | b"right" | b"bottom" | b"left") {
        return;
    }
    if !matches!(attr_local(e, b"type").as_deref(), None | Some("dxa")) {
        return;
    }
    let Some(value) = attr_local(e, b"w").and_then(|v| v.trim().parse::<u32>().ok()) else {
        return;
    };
    match side {
        b"top" => margins.top = value,
        b"right" => margins.right = value,
        b"bottom" => margins.bottom = value,
        b"left" => margins.left = value,
        _ => {}
    }
    *seen = true;
}

/// Place cells over a running column index and resolve vertical merges
/// (`vMerge="restart"` opens a span, a later `vMerge` continuation at the same
/// starting column grows the owner's `row_span` and is dropped) — the OOXML
/// analogue of `table.rs` Phase B.
fn build_table(
    raw_rows: Vec<(Vec<CellRaw>, bool)>,
    fixed_layout: bool,
    indent_twips: Option<i32>,
    align: Option<Align>,
    width_pct: Option<f32>,
    border_color: Option<Color>,
    border_colors: TableBorderColors,
    border_size_eighths: Option<u16>,
    border_sizes: TableBorderSizes,
    border_style: Option<TableBorderStyle>,
    border_styles: TableBorderStyles,
) -> Table {
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
        margins: Option<CellMargins>,
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
                margins: c.margins,
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
                margins: p.margins,
            })
            .collect();
        rows.push(Row { cells });
    }
    Table {
        rows,
        header_rows,
        fixed_layout,
        indent_twips,
        align,
        width_pct,
        border_color,
        border_colors,
        border_size_eighths,
        border_sizes,
        border_style,
        border_styles,
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
        let ref_targets = HashMap::new();
        let ref_position_context = super::super::fields::RefPositionContext::default();
        let ref_number_context = super::super::fields::RefNumberContext::empty();
        let page_ref_context = super::super::fields::PageRefContext::empty();
        let note_ref_context = super::super::fields::NoteRefContext::empty();
        let section_context = super::super::fields::SectionContext::empty();
        let style_ref_context = super::super::fields::StyleRefContext::empty();
        let legacy_form_context = super::super::fields::LegacyFormContext::empty();
        let table_formula_context = super::super::fields::TableFormulaContext::empty();
        let toc_entries = Vec::new();
        let core_properties = crate::CoreProperties::default();
        let custom_properties = HashMap::new();
        let document_variables = HashMap::new();
        let extended_properties = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            ref_targets: &ref_targets,
            ref_position_context: &ref_position_context,
            ref_number_context: &ref_number_context,
            page_ref_context: &page_ref_context,
            note_ref_context: &note_ref_context,
            section_context: &section_context,
            style_ref_context: &style_ref_context,
            legacy_form_context: &legacy_form_context,
            table_formula_context: &table_formula_context,
            toc_entries: &toc_entries,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
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
            margins: None,
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
        assert_eq!(
            headers,
            vec![
                HeaderFooterRef {
                    rel_id: "rIdH".to_string(),
                    type_name: "default".to_string()
                },
                HeaderFooterRef {
                    rel_id: "rIdH1".to_string(),
                    type_name: "first".to_string()
                }
            ]
        );
        assert_eq!(
            footers,
            vec![HeaderFooterRef {
                rel_id: "rIdF".to_string(),
                type_name: "default".to_string()
            }]
        );
    }

    #[test]
    fn scans_final_section_page_number_start_only() {
        let no_final_restart = r#"<w:document><w:body>
            <w:p><w:pPr><w:sectPr><w:pgNumType w:start="3" w:fmt="upperRoman"/></w:sectPr></w:pPr></w:p>
            <w:sectPr/>
        </w:body></w:document>"#;
        assert_eq!(scan_page_number_start(no_final_restart), None);
        assert_eq!(scan_page_number_format(no_final_restart), None);

        let final_restart = r#"<w:document><w:body>
            <w:p><w:pPr><w:sectPr><w:pgNumType w:start="3"/></w:sectPr></w:pPr></w:p>
            <w:sectPr><w:pgNumType w:start="7" w:fmt="decimalZero"/></w:sectPr>
        </w:body></w:document>"#;
        assert_eq!(scan_page_number_start(final_restart), Some(7));
        assert_eq!(
            scan_page_number_format(final_restart),
            Some(PageNumberFormat::DecimalZero)
        );
    }

    #[test]
    fn parses_real_notes_skipping_separators() {
        let styles = Styles::default();
        let numbering = Numbering::default();
        let rels = HashMap::new();
        let media = HashMap::new();
        let ref_targets = HashMap::new();
        let ref_position_context = super::super::fields::RefPositionContext::default();
        let ref_number_context = super::super::fields::RefNumberContext::empty();
        let page_ref_context = super::super::fields::PageRefContext::empty();
        let note_ref_context = super::super::fields::NoteRefContext::empty();
        let section_context = super::super::fields::SectionContext::empty();
        let style_ref_context = super::super::fields::StyleRefContext::empty();
        let legacy_form_context = super::super::fields::LegacyFormContext::empty();
        let table_formula_context = super::super::fields::TableFormulaContext::empty();
        let toc_entries = Vec::new();
        let core_properties = crate::CoreProperties::default();
        let custom_properties = HashMap::new();
        let document_variables = HashMap::new();
        let extended_properties = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            ref_targets: &ref_targets,
            ref_position_context: &ref_position_context,
            ref_number_context: &ref_number_context,
            page_ref_context: &page_ref_context,
            note_ref_context: &note_ref_context,
            section_context: &section_context,
            style_ref_context: &style_ref_context,
            legacy_form_context: &legacy_form_context,
            table_formula_context: &table_formula_context,
            toc_entries: &toc_entries,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
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
        let ref_targets = HashMap::new();
        let ref_position_context = super::super::fields::RefPositionContext::default();
        let ref_number_context = super::super::fields::RefNumberContext::empty();
        let page_ref_context = super::super::fields::PageRefContext::empty();
        let note_ref_context = super::super::fields::NoteRefContext::empty();
        let section_context = super::super::fields::SectionContext::empty();
        let style_ref_context = super::super::fields::StyleRefContext::empty();
        let legacy_form_context = super::super::fields::LegacyFormContext::empty();
        let table_formula_context = super::super::fields::TableFormulaContext::empty();
        let toc_entries = Vec::new();
        let core_properties = crate::CoreProperties::default();
        let custom_properties = HashMap::new();
        let document_variables = HashMap::new();
        let extended_properties = HashMap::new();
        let ctx = Ctx {
            styles: &styles,
            numbering: &numbering,
            rels: &rels,
            media: &media,
            ref_targets: &ref_targets,
            ref_position_context: &ref_position_context,
            ref_number_context: &ref_number_context,
            page_ref_context: &page_ref_context,
            note_ref_context: &note_ref_context,
            section_context: &section_context,
            style_ref_context: &style_ref_context,
            legacy_form_context: &legacy_form_context,
            table_formula_context: &table_formula_context,
            toc_entries: &toc_entries,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
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

        let table = build_table(
            rows,
            false,
            None,
            None,
            None,
            None,
            TableBorderColors::default(),
            None,
            TableBorderSizes::default(),
            None,
            TableBorderStyles::default(),
        );

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
                Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {
                    String::new()
                }
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
