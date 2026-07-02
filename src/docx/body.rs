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

use std::collections::{HashMap, HashSet};

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::fields::{computed_run_symbol_char, TocEntry};
use super::numbering::Numbering;
use super::parse_rgb_hex_color;
use super::styles::Styles;
use super::xml_text::{read_i64_text, read_text, skip_subtree};
use super::{
    attr_f32, attr_i32, attr_i64, attr_local, attr_local_trimmed, attr_u16, attr_u32, attr_u8,
    field_char_type, is_page_break_type, local, toggle_on,
};
use crate::annotation::{
    barcode_field_syntax, direct_ref_field_syntax, instruction_parts, legacy_form_field_syntax,
    merge_field_syntax, normalized_field_instruction, note_ref_field_syntax, opaque_field_syntax,
    page_ref_field_syntax, ref_field_syntax, toc_field_syntax, FieldKind,
};
use crate::model::{
    Align, AuthoredContentControl, Block, Cell, CellMargins, CharProps, Color, DocGrid,
    DocGridType, FieldRole, FieldUnsupportedReason, Image, Indent, ListInfo, PageNumberFormat,
    PageSetup, ParaProps, Paragraph, Row, Run, SectionBreakKind, SectionSetup, Spacing, Table,
    TableBorderColors, TableBorderSide, TableBorderSizes, TableBorderStyle, TableBorderStyles,
    TextDirection, VCell, VertAlign,
};
use crate::text;
use crate::CoreProperties;

/// Twips (1/20 pt) string → points.
fn twips_to_pt(s: &str) -> Option<f32> {
    s.trim().parse::<f32>().ok().map(|t| t / 20.0)
}

fn type_defaults_to_dxa(e: &BytesStart<'_>) -> bool {
    attr_local_trimmed(e, b"type")
        .as_deref()
        .map_or(true, |value| value == "dxa")
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
    pub bookmark_names: &'a HashSet<String>,
    pub core_properties: &'a CoreProperties,
    pub custom_properties: &'a HashMap<String, String>,
    pub document_variables: &'a HashMap<String, String>,
    pub extended_properties: &'a HashMap<String, String>,
    pub file_size_bytes: Option<usize>,
    pub ref_field_cursor: std::cell::RefCell<usize>,
    pub page_field_cursor: std::cell::RefCell<usize>,
    pub last_page_field_unsupported_display_format: std::cell::RefCell<Option<bool>>,
    pub page_ref_field_cursor: std::cell::RefCell<usize>,
    pub note_ref_field_cursor: std::cell::RefCell<usize>,
    pub section_field_cursor: std::cell::RefCell<usize>,
    pub style_ref_field_cursor: std::cell::RefCell<usize>,
    pub form_field_cursor: std::cell::RefCell<usize>,
    pub formula_field_cursor: std::cell::RefCell<usize>,
    pub sequence_counters: std::cell::RefCell<HashMap<String, i64>>,
    pub sequence_heading_counts: std::cell::RefCell<[u32; 9]>,
    pub sequence_heading_scopes: std::cell::RefCell<HashMap<(String, u8), u32>>,
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
            Ok(Event::Start(e))
                if matches!(
                    local(e.name().as_ref()),
                    b"del"
                        | b"moveFrom"
                        | b"pPrChange"
                        | b"rPrChange"
                        | b"tblPrChange"
                        | b"trPrChange"
                        | b"tcPrChange"
                        | b"sectPrChange"
                ) =>
            {
                skip_subtree(&mut r);
            }
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_hf_ref_alternate_content(r, &mut refs);
            }
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"headerReference" | b"footerReference" => {
                    record_header_footer_ref(&mut refs, &e);
                    skip_subtree(r);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"headerReference" | b"footerReference" => record_header_footer_ref(&mut refs, &e),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    refs
}

fn read_hf_ref_alternate_content(r: &mut Xml<'_>, refs: &mut HeaderFooterRefs) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_hf_ref_alternate_content_branch(r, refs, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_hf_ref_alternate_content_branch(
    r: &mut Xml<'_>,
    refs: &mut HeaderFooterRefs,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_hf_ref_alternate_content(r, refs);
            }
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"headerReference" | b"footerReference" => {
                    record_header_footer_ref(refs, &e);
                    skip_subtree(r);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"headerReference" | b"footerReference" => record_header_footer_ref(refs, &e),
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn record_header_footer_ref(refs: &mut HeaderFooterRefs, e: &BytesStart<'_>) {
    let Some(reference) = header_footer_ref(e) else {
        return;
    };
    match local(e.name().as_ref()) {
        b"headerReference" => refs.headers.push(reference),
        b"footerReference" => refs.footers.push(reference),
        _ => {}
    }
}

fn header_footer_ref(e: &BytesStart<'_>) -> Option<HeaderFooterRef> {
    attr_local_trimmed(e, b"id").map(|rel_id| HeaderFooterRef {
        rel_id,
        type_name: attr_local_trimmed(e, b"type").unwrap_or_else(|| "default".to_string()),
    })
}

/// Scan the body's section properties for page geometry (`<w:pgSz>` size +
/// orientation, `<w:pgMar>` left margin) → [`crate::model::PageSetup`]. Uses the
/// last `sectPr` (the final/primary section). Falls back to the A4 default when
/// absent. Twips (1/20 pt) → points.
pub(crate) fn scan_page_setup(xml: &str) -> PageSetup {
    let mut r = Reader::from_str(xml);
    let mut page = PageSetup::default();
    let mut found = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                scan_page_setup_alternate_content(&mut r, &mut page, &mut found);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                if apply_page_setup_child(&mut page, &e) {
                    found = true;
                }
            }
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

fn scan_page_setup_alternate_content(r: &mut Xml<'_>, page: &mut PageSetup, found: &mut bool) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        scan_page_setup_alternate_content_branch(r, page, found, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn scan_page_setup_alternate_content_branch(
    r: &mut Xml<'_>,
    page: &mut PageSetup,
    found: &mut bool,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                scan_page_setup_alternate_content(r, page, found);
            }
            Ok(Event::Start(e)) => {
                if apply_page_setup_child(page, &e) {
                    *found = true;
                } else {
                    skip_subtree(r);
                }
            }
            Ok(Event::Empty(e)) => {
                if apply_page_setup_child(page, &e) {
                    *found = true;
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_page_setup_child(page: &mut PageSetup, e: &BytesStart<'_>) -> bool {
    match local(e.name().as_ref()) {
        b"pgSz" => {
            if let Some(size) = section_page_size(e) {
                apply_section_page_size(page, size);
                true
            } else {
                false
            }
        }
        b"pgMar" => {
            let margins = section_page_margins(e);
            if section_page_margins_present(margins) {
                apply_section_page_margins(page, margins);
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn section_page_size(e: &BytesStart<'_>) -> Option<(f32, f32, bool)> {
    let width = attr_local(e, b"w").and_then(|value| twips_to_pt(&value))?;
    let height = attr_local(e, b"h").and_then(|value| twips_to_pt(&value))?;
    let landscape = attr_local(e, b"orient").is_some_and(|value| value.trim() == "landscape");
    Some((width, height, landscape))
}

fn apply_section_page_size(page: &mut PageSetup, (width, height, landscape): (f32, f32, bool)) {
    page.width_pt = width;
    page.height_pt = height;
    page.landscape = landscape;
}

fn section_page_margins(
    e: &BytesStart<'_>,
) -> (Option<f32>, Option<f32>, Option<f32>, Option<f32>) {
    (
        attr_local(e, b"left").and_then(|value| twips_to_pt(&value)),
        attr_local(e, b"right").and_then(|value| twips_to_pt(&value)),
        attr_local(e, b"top").and_then(|value| twips_to_pt(&value)),
        attr_local(e, b"bottom").and_then(|value| twips_to_pt(&value)),
    )
}

fn section_page_margins_present(
    (left, right, top, bottom): (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
) -> bool {
    left.or(right).or(top).or(bottom).is_some()
}

fn apply_section_page_margins(
    page: &mut PageSetup,
    (left, right, top, bottom): (Option<f32>, Option<f32>, Option<f32>, Option<f32>),
) {
    if let Some(left) = left {
        page.margin_pt = left;
    }
    page.margin_left_pt = left;
    page.margin_right_pt = right;
    page.margin_top_pt = top;
    page.margin_bottom_pt = bottom;
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_section_setup_alternate_content(r).columns {
                    columns = value;
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"cols" => {
                columns = section_columns(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    columns
}

fn section_columns(e: &BytesStart<'_>) -> Option<u16> {
    attr_u16(e, b"num").map(|value| value.max(1))
}

fn read_section_text_direction(r: &mut Xml<'_>) -> Option<TextDirection> {
    let mut text_direction = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_section_setup_alternate_content(r).text_direction {
                    text_direction = value;
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"textDirection" =>
            {
                text_direction = section_text_direction(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text_direction
}

fn section_text_direction(e: &BytesStart<'_>) -> Option<TextDirection> {
    attr_local(e, b"val").and_then(|value| TextDirection::from_wml_value(&value))
}

fn doc_grid_from_attrs(e: &BytesStart<'_>) -> Option<DocGrid> {
    let grid_type = attr_local(e, b"type")
        .and_then(|value| DocGridType::from_wml_value(&value))
        .unwrap_or(DocGridType::Default);
    let line_pitch = attr_u32(e, b"linePitch");
    let character_space = attr_u32(e, b"charSpace");
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_section_setup_alternate_content(r).doc_grid {
                    doc_grid = value;
                }
            }
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if read_section_setup_alternate_content(r).title_page {
                    title_page = true;
                }
            }
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

#[derive(Default)]
struct SectionSetupScan {
    columns: Option<Option<u16>>,
    text_direction: Option<Option<TextDirection>>,
    doc_grid: Option<Option<DocGrid>>,
    title_page: bool,
}

fn read_section_setup_alternate_content(r: &mut Xml<'_>) -> SectionSetupScan {
    let mut setup = SectionSetupScan::default();
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        setup = read_section_setup_alternate_content_branch(r, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    setup
}

fn read_section_setup_alternate_content_branch(r: &mut Xml<'_>, branch: &[u8]) -> SectionSetupScan {
    let mut setup = SectionSetupScan::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                merge_section_setup_scan(&mut setup, read_section_setup_alternate_content(r));
            }
            Ok(Event::Start(e)) => {
                if !record_section_setup_child(&mut setup, &e) {
                    skip_subtree(r);
                }
            }
            Ok(Event::Empty(e)) => {
                record_section_setup_child(&mut setup, &e);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    setup
}

fn merge_section_setup_scan(target: &mut SectionSetupScan, source: SectionSetupScan) {
    if source.columns.is_some() {
        target.columns = source.columns;
    }
    if source.text_direction.is_some() {
        target.text_direction = source.text_direction;
    }
    if source.doc_grid.is_some() {
        target.doc_grid = source.doc_grid;
    }
    target.title_page |= source.title_page;
}

fn record_section_setup_child(setup: &mut SectionSetupScan, e: &BytesStart<'_>) -> bool {
    match local(e.name().as_ref()) {
        b"cols" => {
            setup.columns = Some(section_columns(e));
            true
        }
        b"textDirection" => {
            setup.text_direction = Some(section_text_direction(e));
            true
        }
        b"docGrid" => {
            setup.doc_grid = Some(doc_grid_from_attrs(e));
            true
        }
        b"titlePg" => {
            setup.title_page = true;
            true
        }
        _ => false,
    }
}

fn read_section_page_number_start(r: &mut Xml<'_>) -> Option<u32> {
    let mut page_number_start = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                page_number_start = read_section_page_number_start_alternate_content(r);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_start = section_page_number_start(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_start
}

fn read_section_page_number_start_alternate_content(r: &mut Xml<'_>) -> Option<u32> {
    let mut page_number_start = None;
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        page_number_start =
                            read_section_page_number_start_alternate_content_branch(r, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_start
}

fn read_section_page_number_start_alternate_content_branch(
    r: &mut Xml<'_>,
    branch: &[u8],
) -> Option<u32> {
    let mut page_number_start = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                page_number_start = read_section_page_number_start_alternate_content(r);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_start = section_page_number_start(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                page_number_format = read_section_page_number_format_alternate_content(r);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_format = section_page_number_format(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_format
}

fn read_section_page_number_format_alternate_content(r: &mut Xml<'_>) -> Option<PageNumberFormat> {
    let mut page_number_format = None;
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        page_number_format =
                            read_section_page_number_format_alternate_content_branch(r, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_format
}

fn read_section_page_number_format_alternate_content_branch(
    r: &mut Xml<'_>,
    branch: &[u8],
) -> Option<PageNumberFormat> {
    let mut page_number_format = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                page_number_format = read_section_page_number_format_alternate_content(r);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"pgNumType" =>
            {
                page_number_format = section_page_number_format(&e);
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    page_number_format
}

fn section_page_number_start(e: &BytesStart<'_>) -> Option<u32> {
    attr_u32(e, b"start").map(|value| value.max(1))
}

fn section_page_number_format(e: &BytesStart<'_>) -> Option<PageNumberFormat> {
    attr_local(e, b"fmt").and_then(|value| PageNumberFormat::from_wml_value(&value))
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_note_entries_alternate_content(&mut r, ctx, tag, &mut entries);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == tag => {
                if let Some(entry) = read_note_entry(&mut r, ctx, &e) {
                    entries.push(entry);
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    entries
}

fn read_note_entries_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    tag: &[u8],
    entries: &mut Vec<(String, Vec<Block>)>,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_note_entries_alternate_content_branch(r, ctx, tag, entries, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_note_entries_alternate_content_branch(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    tag: &[u8],
    entries: &mut Vec<(String, Vec<Block>)>,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_note_entries_alternate_content(r, ctx, tag, entries);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == tag => {
                if let Some(entry) = read_note_entry(r, ctx, &e) {
                    entries.push(entry);
                }
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_note_entry(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    e: &BytesStart<'_>,
) -> Option<(String, Vec<Block>)> {
    let boilerplate = matches!(
        attr_local_trimmed(e, b"type").as_deref(),
        Some("separator") | Some("continuationSeparator") | Some("continuationNotice")
    );
    if boilerplate {
        skip_subtree(r);
        return None;
    }
    let Some(id) = attr_local_trimmed(e, b"id") else {
        skip_subtree(r);
        return None;
    };
    Some((id, read_blocks(r, ctx, 0)))
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
                        if let Some(id) = attr_local_trimmed(&e, b"id") {
                            current_block_refs.push(id);
                        }
                        skip_subtree(&mut r);
                        body_depth = body_depth.saturating_sub(1);
                    } else if name == b"t" {
                        current_block_text.push_str(&read_text(&mut r));
                        body_depth = body_depth.saturating_sub(1);
                    } else if name == b"sym" {
                        append_run_symbol(&mut current_block_text, &e);
                        skip_subtree(&mut r);
                        body_depth = body_depth.saturating_sub(1);
                    } else if name == b"AlternateContent" {
                        append_note_anchor_alternate_content(
                            &mut r,
                            tag,
                            &mut current_block_text,
                            &mut current_block_refs,
                            0,
                        );
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
                        if let Some(id) = attr_local_trimmed(&e, b"id") {
                            current_block_refs.push(id);
                        }
                    } else {
                        append_note_anchor_empty(&mut current_block_text, &e, name);
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
    matches!(name, b"drawing" | b"pict" | b"object")
}

fn append_note_anchor_empty_marker(out: &mut String, name: &[u8]) {
    match name {
        b"tab" => out.push('\t'),
        b"br" | b"cr" => out.push('\n'),
        b"noBreakHyphen" => out.push('-'),
        b"softHyphen" => out.push('\u{00ad}'),
        _ => {}
    }
}

fn append_note_anchor_empty(out: &mut String, e: &BytesStart<'_>, name: &[u8]) {
    if name == b"sym" {
        append_run_symbol(out, e);
    } else {
        append_note_anchor_empty_marker(out, name);
    }
}

fn append_note_anchor_alternate_content(
    r: &mut Xml<'_>,
    tag: &[u8],
    text: &mut String,
    refs: &mut Vec<String>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    append_note_anchor_content(r, tag, text, refs, depth + 1);
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn append_note_anchor_content(
    r: &mut Xml<'_>,
    tag: &[u8],
    text: &mut String,
    refs: &mut Vec<String>,
    depth: u32,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == tag {
                    if let Some(id) = attr_local_trimmed(&e, b"id") {
                        refs.push(id);
                    }
                    skip_subtree(r);
                } else if name == b"t" {
                    text.push_str(&read_text(r));
                } else if name == b"sym" {
                    append_run_symbol(text, &e);
                    skip_subtree(r);
                } else if name == b"AlternateContent" {
                    append_note_anchor_alternate_content(r, tag, text, refs, depth + 1);
                } else if is_note_anchor_embedded_body(name) {
                    skip_subtree(r);
                } else {
                    append_note_anchor_content(r, tag, text, refs, depth + 1);
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == tag {
                    if let Some(id) = attr_local_trimmed(&e, b"id") {
                        refs.push(id);
                    }
                } else {
                    append_note_anchor_empty(text, &e, name);
                }
            }
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
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
                b"AlternateContent" => {
                    blocks.extend(read_alternate_content_blocks(r, ctx, depth + 1))
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    blocks
}

fn read_alternate_content_blocks(r: &mut Xml<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Block> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut blocks = Vec::new();
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    blocks.extend(read_blocks(r, ctx, depth + 1));
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
                b"AlternateContent" => {
                    read_content_control_blocks_alternate_content(
                        r,
                        ctx,
                        depth + 1,
                        &mut control,
                        &mut blocks,
                    );
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

fn read_content_control_blocks_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    control: &mut Option<AuthoredContentControl>,
    blocks: &mut Vec<Block>,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_content_control_blocks_alternate_content_branch(
                            r,
                            ctx,
                            depth + 1,
                            control,
                            blocks,
                            name,
                        );
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_content_control_blocks_alternate_content_branch(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    control: &mut Option<AuthoredContentControl>,
    blocks: &mut Vec<Block>,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"sdtPr" => *control = read_content_control_pr(r),
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
                b"AlternateContent" => read_content_control_blocks_alternate_content(
                    r,
                    ctx,
                    depth + 1,
                    control,
                    blocks,
                ),
                _ => skip_subtree(r),
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
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
    let mut sequence_heading_applied = false;
    let mut complex_field = ComplexFieldTracker::default();
    let mut bookmarks = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"pPr" => {
                    pp = read_ppr(r);
                    apply_sequence_heading_scope(&pp, ctx, &mut sequence_heading_applied);
                }
                b"r" => {
                    let start = runs.len();
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
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"hyperlink" => {
                    let start = runs.len();
                    runs.extend(read_hyperlink(r, &e, ctx, depth));
                    mark_complex_field_result_runs(&mut complex_field, &runs, start);
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"fldSimple" => {
                    let start = runs.len();
                    runs.extend(read_fldsimple(r, &e, ctx, depth));
                    mark_complex_field_result_runs(&mut complex_field, &runs, start);
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"sdt" => {
                    let start = runs.len();
                    append_content_control_runs_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        &mut runs,
                        &mut complex_field,
                    );
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"customXml" | b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo"
                | b"dir" => {
                    let start = runs.len();
                    append_runs_container_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        &mut runs,
                        &mut complex_field,
                    );
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"AlternateContent" => {
                    let start = runs.len();
                    append_alternate_content_runs_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        &mut runs,
                        &mut complex_field,
                    );
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                b"bookmarkStart" => {
                    push_active_bookmark(&mut bookmarks, &e);
                    skip_subtree(r);
                }
                b"bookmarkEnd" => {
                    remove_active_bookmark(&mut bookmarks, &e);
                    skip_subtree(r);
                }
                // `w:del` = tracked deletion (removed text) → drop.
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"bookmarkStart" => push_active_bookmark(&mut bookmarks, &e),
                b"bookmarkEnd" => remove_active_bookmark(&mut bookmarks, &e),
                b"fldSimple" => {
                    let start = runs.len();
                    push_empty_fldsimple_run(&mut runs, &e, ctx);
                    mark_complex_field_result_runs(&mut complex_field, &runs, start);
                    apply_active_bookmark(&mut runs, start, &bookmarks);
                }
                _ => {}
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apply_sequence_heading_scope(&pp, ctx, &mut sequence_heading_applied);
    let section = pp.section.take();
    (finalize_paragraph(runs, pp, ctx), section)
}

fn push_active_bookmark(bookmarks: &mut Vec<(String, String)>, e: &BytesStart<'_>) {
    let Some(id) = attr_local_trimmed(e, b"id") else {
        return;
    };
    let Some(name) = attr_local_trimmed(e, b"name") else {
        return;
    };
    bookmarks.push((id, name));
}

fn remove_active_bookmark(bookmarks: &mut Vec<(String, String)>, e: &BytesStart<'_>) {
    let Some(id) = attr_local_trimmed(e, b"id") else {
        return;
    };
    if let Some(index) = bookmarks
        .iter()
        .rposition(|(active_id, _)| active_id == &id)
    {
        bookmarks.remove(index);
    }
}

fn apply_active_bookmark(runs: &mut [Run], start: usize, bookmarks: &[(String, String)]) {
    let Some((_, name)) = bookmarks.last() else {
        return;
    };
    for run in runs.iter_mut().skip(start) {
        if run.bookmark.is_none() {
            run.bookmark = Some(name.clone());
        }
    }
}

fn mark_complex_field_result_runs(
    complex_field: &mut ComplexFieldTracker,
    runs: &[Run],
    start: usize,
) {
    if !complex_field.in_result() {
        return;
    }
    for (index, run) in runs.iter().enumerate().skip(start) {
        if !run.text.is_empty() {
            complex_field.push_result_run(index, &run.text, false);
        }
    }
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
        return if paragraph.is_blank() && !paragraph_has_field_runs(&paragraph) {
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
                if !current.is_blank() || paragraph_has_field_runs(&current) {
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
    if !current.is_blank() || paragraph_has_field_runs(&current) {
        blocks.push(Block::Paragraph(current));
    }
    blocks
}

fn paragraph_has_field_runs(paragraph: &Paragraph) -> bool {
    paragraph
        .runs
        .iter()
        .any(|run| !matches!(run.field, FieldRole::Other))
}

#[derive(Default)]
struct ComplexFieldTracker {
    instruction: String,
    phase: Option<ComplexFieldPhase>,
    result_runs: Vec<ComplexFieldResultRun>,
    result_text: String,
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
    unsupported_reason: Option<FieldUnsupportedReason>,
    result_runs: Vec<ComplexFieldResultRun>,
    insert_at: usize,
}

#[derive(Clone, Copy)]
struct ComplexFieldResultRun {
    index: usize,
    preserve_hyperlink: bool,
}

impl ComplexFieldTracker {
    fn begin(&mut self) {
        self.instruction.clear();
        self.result_runs.clear();
        self.result_text.clear();
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
            let instruction = normalized_field_instruction(&self.instruction);
            if !instruction.is_empty() {
                let current_result = self.result_text.as_str();
                let text = computed_simple_field_result(&instruction, ctx, current_result);
                let unsupported_reason = text
                    .is_none()
                    .then(|| unsupported_simple_field_reason_hint(&instruction, ctx))
                    .flatten();
                let insert_at = self.result_start.unwrap_or(index);
                self.pending = Some(PendingComplexField {
                    text,
                    instruction,
                    unsupported_reason,
                    result_runs: std::mem::take(&mut self.result_runs),
                    insert_at,
                });
            }
        }
        self.instruction.clear();
        self.result_runs.clear();
        self.result_text.clear();
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

    fn push_result_run(&mut self, index: usize, text: &str, preserve_hyperlink: bool) {
        if self.in_result() {
            self.result_runs.push(ComplexFieldResultRun {
                index,
                preserve_hyperlink,
            });
            self.result_text.push_str(text);
        }
    }

    fn apply_pending(&mut self, runs: &mut Vec<Run>) {
        let Some(computed) = self.pending.take() else {
            return;
        };
        if computed.result_runs.is_empty() {
            if let Some(text) = computed.text {
                runs.insert(
                    computed.insert_at.min(runs.len()),
                    computed_simple_field_run(computed.instruction, text),
                );
            } else {
                runs.insert(
                    computed.insert_at.min(runs.len()),
                    empty_simple_field_run(computed.instruction, computed.unsupported_reason),
                );
            }
            return;
        }
        for (offset, result_run) in computed.result_runs.iter().copied().enumerate() {
            let Some(run) = runs.get_mut(result_run.index) else {
                continue;
            };
            if let Some(text) = computed.text.as_deref() {
                run.field = if result_run.preserve_hyperlink
                    && matches!(run.field, FieldRole::Hyperlink { .. })
                {
                    run.field.clone()
                } else if text.is_empty()
                    && offset == 0
                    && preserves_computed_empty_field_instruction(&computed.instruction)
                {
                    FieldRole::Simple {
                        instruction: computed.instruction.clone(),
                    }
                } else {
                    FieldRole::Other
                };
                run.field_unsupported_reason = None;
            } else {
                run.field = FieldRole::Simple {
                    instruction: computed.instruction.clone(),
                };
                run.field_unsupported_reason = computed.unsupported_reason;
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
        field_unsupported_reason: None,
        image: None,
        comment: None,
        revision: None,
        content_control: None,
        bookmark: None,
        note: None,
    }
}

fn computed_simple_field_run(instruction: String, text: String) -> Run {
    if text.is_empty() && preserves_computed_empty_field_instruction(&instruction) {
        empty_simple_field_run(instruction, None)
    } else {
        computed_field_run(text)
    }
}

fn preserves_computed_empty_field_instruction(instruction: &str) -> bool {
    matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::ReferenceIndex(ref kind) if is_reference_index_marker_kind(kind)
    )
}

fn empty_simple_field_run(
    instruction: String,
    unsupported_reason: Option<FieldUnsupportedReason>,
) -> Run {
    Run {
        text: String::new(),
        field: FieldRole::Simple { instruction },
        field_unsupported_reason: unsupported_reason,
        ..Default::default()
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

fn read_runs_container_with_complex(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut complex_field = ComplexFieldTracker::default();
    append_runs_container_with_complex(r, ctx, link, depth, &mut runs, &mut complex_field);
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
                b"hyperlink" => {
                    let start = runs.len();
                    runs.extend(read_hyperlink(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"fldSimple" => {
                    let start = runs.len();
                    runs.extend(read_fldsimple(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                b"customXml" | b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo"
                | b"dir" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                b"AlternateContent" => append_alternate_content_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"fldSimple" {
                    let start = runs.len();
                    push_empty_fldsimple_run(runs, &e, ctx);
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
            }
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
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
                b"hyperlink" => {
                    let start = runs.len();
                    runs.extend(read_hyperlink(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"fldSimple" => {
                    let start = runs.len();
                    runs.extend(read_fldsimple(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                b"customXml" | b"ins" | b"moveTo" | b"smartTag" | b"bdo" | b"dir" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                b"AlternateContent" => append_content_control_runs_alternate_content_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    &mut control,
                    runs,
                    complex_field,
                ),
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"fldSimple" {
                    let start = runs.len();
                    push_empty_fldsimple_run(runs, &e, ctx);
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
            }
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    apply_content_control(&mut runs[start..], control);
}

fn append_content_control_runs_alternate_content_with_complex(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
    control: &mut Option<AuthoredContentControl>,
    runs: &mut Vec<Run>,
    complex_field: &mut ComplexFieldTracker,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        append_content_control_runs_alternate_content_branch_with_complex(
                            r,
                            ctx,
                            link,
                            depth + 1,
                            control,
                            runs,
                            complex_field,
                            name,
                        );
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn append_content_control_runs_alternate_content_branch_with_complex(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    link: Option<&str>,
    depth: u32,
    control: &mut Option<AuthoredContentControl>,
    runs: &mut Vec<Run>,
    complex_field: &mut ComplexFieldTracker,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"sdtPr" => *control = read_content_control_pr(r),
                b"sdtContent" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                b"r" => {
                    let next = read_run(r, ctx, link, depth + 1, Some(complex_field), runs.len());
                    runs.extend(next);
                    complex_field.apply_pending(runs);
                }
                b"hyperlink" => {
                    let start = runs.len();
                    runs.extend(read_hyperlink(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"fldSimple" => {
                    let start = runs.len();
                    runs.extend(read_fldsimple(r, &e, ctx, depth));
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
                b"sdt" => append_content_control_runs_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    runs,
                    complex_field,
                ),
                b"customXml" | b"ins" | b"moveTo" | b"smartTag" | b"bdo" | b"dir" => {
                    append_runs_container_with_complex(r, ctx, link, depth + 1, runs, complex_field)
                }
                b"AlternateContent" => append_content_control_runs_alternate_content_with_complex(
                    r,
                    ctx,
                    link,
                    depth + 1,
                    control,
                    runs,
                    complex_field,
                ),
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"fldSimple" {
                    let start = runs.len();
                    push_empty_fldsimple_run(runs, &e, ctx);
                    mark_complex_field_result_runs(complex_field, runs, start);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn append_alternate_content_runs_with_complex(
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
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    append_runs_container_with_complex(
                        r,
                        ctx,
                        link,
                        depth + 1,
                        runs,
                        complex_field,
                    );
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_content_control_pr(r: &mut Xml<'_>) -> Option<AuthoredContentControl> {
    let mut control = AuthoredContentControl::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_content_control_pr_alternate_content(r, &mut control);
            }
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

fn read_content_control_pr_alternate_content(
    r: &mut Xml<'_>,
    control: &mut AuthoredContentControl,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_content_control_pr_alternate_content_branch(r, control, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_content_control_pr_alternate_content_branch(
    r: &mut Xml<'_>,
    control: &mut AuthoredContentControl,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_content_control_pr_alternate_content(r, control);
            }
            Ok(Event::Start(e)) => {
                read_content_control_pr_item(control, &e);
                skip_subtree(r);
            }
            Ok(Event::Empty(e)) => read_content_control_pr_item(control, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_content_control_pr_item(control: &mut AuthoredContentControl, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"alias" => control.alias = attr_local_trimmed(e, b"val"),
        b"tag" => control.tag = attr_local_trimmed(e, b"val"),
        b"dataBinding" => {
            control.data_binding_xpath = attr_local_trimmed(e, b"xpath");
            control.data_binding_store_item_id = attr_local_trimmed(e, b"storeItemID");
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"pPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_ppr_alternate_content(r, &mut pp, &mut num_id, &mut ilvl);
            }
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

fn read_ppr_alternate_content(
    r: &mut Xml<'_>,
    pp: &mut PPr,
    num_id: &mut Option<String>,
    ilvl: &mut u8,
) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_ppr_alternate_content_branch(r, pp, num_id, ilvl, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_ppr_alternate_content_branch(
    r: &mut Xml<'_>,
    pp: &mut PPr,
    num_id: &mut Option<String>,
    ilvl: &mut u8,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"pPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_ppr_alternate_content(r, pp, num_id, ilvl);
            }
            Ok(Event::Start(e)) => {
                if local(e.name().as_ref()) == b"sectPr" {
                    pp.section = Some(read_sect_pr(r));
                } else {
                    read_ppr_item(pp, &e, num_id, ilvl);
                }
            }
            Ok(Event::Empty(e)) => {
                if local(e.name().as_ref()) == b"sectPr" {
                    pp.section = Some(SectionSetup::default());
                } else {
                    read_ppr_item(pp, &e, num_id, ilvl);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_ppr_item(pp: &mut PPr, e: &BytesStart<'_>, num_id: &mut Option<String>, ilvl: &mut u8) {
    match local(e.name().as_ref()) {
        b"pStyle" => pp.style_id = attr_local_trimmed(e, b"val"),
        b"ilvl" => {
            if let Some(v) = attr_u8(e, b"val") {
                *ilvl = v;
            }
        }
        b"numId" => *num_id = attr_local_trimmed(e, b"val"),
        b"jc" => pp.jc = attr_local_trimmed(e, b"val"),
        b"outlineLvl" => pp.outline = attr_u8(e, b"val"),
        b"pageBreakBefore" => pp.page_break_before = toggle_on(attr_local(e, b"val")),
        b"spacing" => {
            pp.spacing.before_pt = attr_local(e, b"before").and_then(|v| twips_to_pt(&v));
            pp.spacing.after_pt = attr_local(e, b"after").and_then(|v| twips_to_pt(&v));
            // `w:line` is 240ths of a line when lineRule is auto/absent.
            let exact = matches!(
                attr_local_trimmed(e, b"lineRule").as_deref(),
                Some("exact") | Some("atLeast")
            );
            if !exact {
                pp.spacing.line_pct = attr_f32(e, b"line").map(|l| l / 240.0);
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
        b"shd" => pp.shading = attr_local(e, b"fill").and_then(|v| parse_rgb_hex_color(&v)),
        _ => {}
    }
}

fn read_sect_pr(r: &mut Xml<'_>) -> SectionSetup {
    let mut section = SectionSetup::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_sect_pr_alternate_content(r, &mut section);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_sect_pr_child(&mut section, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"sectPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    section
}

fn read_sect_pr_alternate_content(r: &mut Xml<'_>, section: &mut SectionSetup) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_sect_pr_alternate_content_branch(r, section, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_sect_pr_alternate_content_branch(
    r: &mut Xml<'_>,
    section: &mut SectionSetup,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"sectPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_sect_pr_alternate_content(r, section);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_sect_pr_child(section, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_sect_pr_child(section: &mut SectionSetup, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"pgSz" => {
            if let Some(size) = section_page_size(e) {
                apply_section_page_size(&mut section.page, size);
            }
        }
        b"type" => {
            section.section_break =
                attr_local(e, b"val").and_then(|value| SectionBreakKind::from_wml_value(&value));
        }
        b"pgMar" => {
            apply_section_page_margins(&mut section.page, section_page_margins(e));
        }
        b"pgNumType" => {
            section.page_number_start = section_page_number_start(e);
            section.page_number_format = section_page_number_format(e);
        }
        b"cols" => {
            section.columns = section_columns(e);
        }
        b"textDirection" => {
            section.text_direction = section_text_direction(e);
        }
        b"docGrid" => {
            section.doc_grid = doc_grid_from_attrs(e);
        }
        b"titlePg" => {
            section.title_page = true;
        }
        _ => {}
    }
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
            field_unsupported_reason: None,
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
            field_unsupported_reason: None,
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
    let mut image_result_runs = Vec::new();
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
                b"sym" => {
                    if append_run_symbol(&mut text, &e) {
                        let in_result = complex_field
                            .as_deref()
                            .map(ComplexFieldTracker::in_result)
                            .unwrap_or(false);
                        if in_result {
                            text_is_field_result = true;
                        }
                    }
                    skip_subtree(r);
                }
                b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" => {
                    append_run_inline_marker(
                        &mut text,
                        &e,
                        complex_field.as_deref(),
                        &mut text_is_field_result,
                    );
                    skip_subtree(r);
                }
                b"drawing" | b"pict" | b"object" => {
                    let start = images.len();
                    let (img, txbx) = read_drawing(r, ctx, depth);
                    push_drawing_runs(&mut images, img, txbx);
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                // AlternateContent can wrap either ordinary run children or the
                // DrawingML/VML forms of the same shape; materialize one branch.
                b"AlternateContent" => {
                    append_run_alternate_content(
                        r,
                        ctx,
                        depth + 1,
                        complex_field.as_deref_mut(),
                        base_index,
                        &mut text,
                        &mut text_is_field_result,
                        &mut images,
                        &mut image_result_runs,
                    );
                }
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"fldChar" => {
                    apply_complex_field_char(&e, ctx, complex_field.as_deref_mut(), base_index)
                }
                b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" => {
                    append_run_inline_marker(
                        &mut text,
                        &e,
                        complex_field.as_deref(),
                        &mut text_is_field_result,
                    );
                }
                b"sym" => {
                    if append_run_symbol(&mut text, &e) {
                        let in_result = complex_field
                            .as_deref()
                            .map(ComplexFieldTracker::in_result)
                            .unwrap_or(false);
                        if in_result {
                            text_is_field_result = true;
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    let mut runs = Vec::new();
    if !text.is_empty() {
        if text_is_field_result {
            if let Some(tracker) = complex_field.as_deref_mut() {
                tracker.push_result_run(base_index + runs.len(), &text, link.is_some());
            }
        }
        runs.push(Run {
            text,
            props,
            field: link
                .map(|u| FieldRole::Hyperlink { url: u.to_string() })
                .unwrap_or(FieldRole::None),
            field_dirty: false,
            field_unsupported_reason: None,
            image: None,
            comment: None,
            revision: None,
            content_control: None,
            bookmark: None,
            note: None,
        });
    }
    if let Some(tracker) = complex_field.as_deref_mut() {
        let image_start = runs.len();
        for image_index in image_result_runs {
            let Some(run) = images.get(image_index) else {
                continue;
            };
            if !run.text.is_empty() {
                tracker.push_result_run(base_index + image_start + image_index, &run.text, false);
            }
        }
    }
    runs.extend(images);
    runs
}

fn append_run_inline_marker(
    text: &mut String,
    e: &BytesStart<'_>,
    complex_field: Option<&ComplexFieldTracker>,
    text_is_field_result: &mut bool,
) -> bool {
    let marker = match local(e.name().as_ref()) {
        b"tab" => Some('\t'),
        b"br" => Some(if is_page_break_type(e) {
            PAGE_BREAK_MARKER
        } else {
            '\n'
        }),
        b"cr" => Some('\n'),
        b"noBreakHyphen" => Some('-'),
        b"softHyphen" => Some('\u{00ad}'),
        _ => None,
    };
    if let Some(marker) = marker {
        mark_complex_field_result_text(complex_field, text_is_field_result);
        text.push(marker);
        true
    } else {
        false
    }
}

fn mark_complex_field_result_text(
    complex_field: Option<&ComplexFieldTracker>,
    text_is_field_result: &mut bool,
) {
    if complex_field.is_some_and(ComplexFieldTracker::in_result) {
        *text_is_field_result = true;
    }
}

fn append_run_symbol(text: &mut String, e: &BytesStart<'_>) -> bool {
    let Some(value) = attr_local_trimmed(e, b"char") else {
        return false;
    };
    let font = attr_local_trimmed(e, b"font");
    let Some(ch) = computed_run_symbol_char(font.as_deref(), &value) else {
        return false;
    };
    text.push(ch);
    true
}

fn append_run_alternate_content(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    mut complex_field: Option<&mut ComplexFieldTracker>,
    base_index: usize,
    text: &mut String,
    text_is_field_result: &mut bool,
    images: &mut Vec<Run>,
    image_result_runs: &mut Vec<usize>,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"Choice" | b"Fallback" if !took => {
                    took = true;
                    append_run_alternate_content_branch(
                        r,
                        ctx,
                        depth + 1,
                        complex_field.as_deref_mut(),
                        base_index,
                        text,
                        text_is_field_result,
                        images,
                        image_result_runs,
                    );
                }
                _ => skip_subtree(r),
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn append_run_alternate_content_branch(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    mut complex_field: Option<&mut ComplexFieldTracker>,
    base_index: usize,
    text: &mut String,
    text_is_field_result: &mut bool,
    images: &mut Vec<Run>,
    image_result_runs: &mut Vec<usize>,
) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return;
    }
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"r" => {
                    let start = images.len();
                    let mut nested_complex_field = ComplexFieldTracker::default();
                    let next = read_run(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        Some(&mut nested_complex_field),
                        images.len(),
                    );
                    images.extend(next);
                    nested_complex_field.apply_pending(images);
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
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
                        *text_is_field_result = true;
                    }
                    text.push_str(&read_text(r));
                }
                b"sym" => {
                    if append_run_symbol(text, &e) {
                        let in_result = complex_field
                            .as_deref()
                            .map(ComplexFieldTracker::in_result)
                            .unwrap_or(false);
                        if in_result {
                            *text_is_field_result = true;
                        }
                    }
                    skip_subtree(r);
                }
                b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" => {
                    append_run_inline_marker(
                        text,
                        &e,
                        complex_field.as_deref(),
                        text_is_field_result,
                    );
                    skip_subtree(r);
                }
                b"drawing" | b"pict" | b"object" => {
                    let start = images.len();
                    let (img, txbx) = read_drawing(r, ctx, depth);
                    push_drawing_runs(images, img, txbx);
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                b"fldSimple" => {
                    let start = images.len();
                    images.extend(read_fldsimple(r, &e, ctx, depth));
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                b"hyperlink" => {
                    let start = images.len();
                    images.extend(read_hyperlink(r, &e, ctx, depth));
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                b"sdt" => {
                    let start = images.len();
                    let mut nested_complex_field = ComplexFieldTracker::default();
                    append_content_control_runs_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        images,
                        &mut nested_complex_field,
                    );
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                b"customXml" | b"ins" | b"moveTo" | b"smartTag" | b"sdtContent" | b"bdo"
                | b"dir" => {
                    let start = images.len();
                    let mut nested_complex_field = ComplexFieldTracker::default();
                    append_runs_container_with_complex(
                        r,
                        ctx,
                        None,
                        depth + 1,
                        images,
                        &mut nested_complex_field,
                    );
                    if complex_field
                        .as_deref()
                        .is_some_and(ComplexFieldTracker::in_result)
                    {
                        image_result_runs.extend(start..images.len());
                    }
                }
                b"AlternateContent" => append_run_alternate_content(
                    r,
                    ctx,
                    depth + 1,
                    complex_field.as_deref_mut(),
                    base_index,
                    text,
                    text_is_field_result,
                    images,
                    image_result_runs,
                ),
                _ => skip_subtree(r),
            },
            Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"fldChar" => {
                    apply_complex_field_char(&e, ctx, complex_field.as_deref_mut(), base_index)
                }
                b"tab" | b"br" | b"cr" | b"noBreakHyphen" | b"softHyphen" => {
                    append_run_inline_marker(
                        text,
                        &e,
                        complex_field.as_deref(),
                        text_is_field_result,
                    );
                }
                b"sym" => {
                    if append_run_symbol(text, &e) {
                        let in_result = complex_field
                            .as_deref()
                            .map(ComplexFieldTracker::in_result)
                            .unwrap_or(false);
                        if in_result {
                            *text_is_field_result = true;
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
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
    match field_char_type(e).as_deref() {
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"rPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_rpr_alternate_content(r, &mut p);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_rpr_child(&mut p, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"rPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    p
}

fn read_rpr_alternate_content(r: &mut Xml<'_>, props: &mut CharProps) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_rpr_alternate_content_branch(r, props, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_rpr_alternate_content_branch(r: &mut Xml<'_>, props: &mut CharProps, branch: &[u8]) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"rPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_rpr_alternate_content(r, props);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_rpr_child(props, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_rpr_child(props: &mut CharProps, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"b" => props.bold = toggle_on(attr_local(e, b"val")),
        b"i" => props.italic = toggle_on(attr_local(e, b"val")),
        b"strike" => props.strike = toggle_on(attr_local(e, b"val")),
        b"dstrike" => props.strike |= toggle_on(attr_local(e, b"val")),
        b"vanish" => props.hidden = toggle_on(attr_local(e, b"val")),
        // `w:u` carries a line style; anything but "none" underlines.
        b"u" => {
            props.underline = attr_local(e, b"val")
                .map(|v| v.trim() != "none")
                .unwrap_or(true)
        }
        b"smallCaps" => props.small_caps = toggle_on(attr_local(e, b"val")),
        b"caps" => props.caps = toggle_on(attr_local(e, b"val")),
        // Font family: prefer the East-Asian face (Korean) over the Latin one.
        b"rFonts" => {
            props.font =
                attr_local_trimmed(e, b"eastAsia").or_else(|| attr_local_trimmed(e, b"ascii"));
        }
        b"sz" => props.size_half_pt = attr_u16(e, b"val"),
        b"color" => props.color = attr_local(e, b"val").and_then(|v| parse_rgb_hex_color(&v)),
        b"highlight" => props.highlight = attr_local_trimmed(e, b"val"),
        b"vertAlign" => {
            props.vert_align = match attr_local_trimmed(e, b"val").as_deref() {
                Some("superscript") => VertAlign::Super,
                Some("subscript") => VertAlign::Sub,
                _ => VertAlign::Baseline,
            };
        }
        _ => {}
    }
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
    let page_relative =
        attr_local_trimmed(start, b"relativeFrom").is_some_and(|value| value == "page");
    let mut offset = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"posOffset" => {
                if page_relative {
                    offset = read_i64_text(r);
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
    let Some(rot) = attr_i64(e, b"rot") else {
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
    ctx.media.get(id.trim()).cloned()
}

/// Read `<w:hyperlink>`: resolve its target (external `r:id` rel, or `#anchor`)
/// and tag its runs with the link.
fn read_hyperlink(r: &mut Xml<'_>, start: &BytesStart<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Run> {
    let url = hyperlink_url(start, ctx);
    read_runs_container_with_complex(r, ctx, url.as_deref(), depth + 1)
}

fn hyperlink_url(start: &BytesStart<'_>, ctx: &Ctx<'_>) -> Option<String> {
    if let Some(id) = attr_local_trimmed(start, b"id") {
        if let Some((target, _external)) = ctx.rels.get(&id) {
            return Some(target.clone());
        }
    }
    attr_local_trimmed(start, b"anchor").map(|a| format!("#{a}"))
}

/// Read `<w:fldSimple>`: hyperlinks keep link semantics; other simple fields
/// keep their normalized instruction on the cached result runs.
fn read_fldsimple(r: &mut Xml<'_>, start: &BytesStart<'_>, ctx: &Ctx<'_>, depth: u32) -> Vec<Run> {
    let instruction = attr_local(start, b"instr").unwrap_or_default();
    let url = hyperlink_instr_url(&instruction);
    let mut runs = read_runs_container_with_complex(r, ctx, url.as_deref(), depth + 1);
    if url.is_none() {
        let instruction = normalized_field_instruction(&instruction);
        if !instruction.is_empty() {
            let current_result = runs.iter().map(|run| run.text.as_str()).collect::<String>();
            let computed = computed_simple_field_result(&instruction, ctx, &current_result);
            if runs.is_empty() {
                if let Some(text) = computed {
                    runs.push(computed_simple_field_run(instruction.clone(), text));
                } else {
                    runs.push(empty_simple_field_run(
                        instruction.clone(),
                        unsupported_simple_field_reason_hint(&instruction, ctx),
                    ));
                }
                return runs;
            }
            for (index, run) in runs.iter_mut().enumerate() {
                if let Some(text) = computed.as_deref() {
                    run.field = if text.is_empty()
                        && index == 0
                        && preserves_computed_empty_field_instruction(&instruction)
                    {
                        FieldRole::Simple {
                            instruction: instruction.clone(),
                        }
                    } else {
                        FieldRole::Other
                    };
                    run.field_unsupported_reason = None;
                    run.text = if index == 0 {
                        text.to_string()
                    } else {
                        String::new()
                    };
                } else {
                    run.field = FieldRole::Simple {
                        instruction: instruction.clone(),
                    };
                    run.field_unsupported_reason =
                        unsupported_simple_field_reason_hint(&instruction, ctx);
                }
            }
        }
    }
    runs
}

fn read_empty_fldsimple(start: &BytesStart<'_>, ctx: &Ctx<'_>) -> Option<Run> {
    let instruction =
        normalized_field_instruction(&attr_local(start, b"instr").unwrap_or_default());
    if instruction.is_empty() {
        return None;
    }
    computed_simple_field_result(&instruction, ctx, "")
        .map(|text| computed_simple_field_run(instruction.clone(), text))
        .or_else(|| {
            Some(empty_simple_field_run(
                instruction.clone(),
                unsupported_simple_field_reason_hint(&instruction, ctx),
            ))
        })
}

fn push_empty_fldsimple_run(runs: &mut Vec<Run>, start: &BytesStart<'_>, ctx: &Ctx<'_>) {
    if let Some(run) = read_empty_fldsimple(start, ctx) {
        runs.push(run);
    }
}

fn unsupported_simple_field_reason_hint(
    instruction: &str,
    ctx: &Ctx<'_>,
) -> Option<FieldUnsupportedReason> {
    if let Some(reason) = unsupported_ref_reason_hint(instruction, ctx) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_page_ref_reason_hint(instruction, ctx) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_note_ref_reason_hint(instruction, ctx) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_toc_reason_hint(instruction, ctx) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_toc_entry_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_page_reason_hint(instruction, ctx) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_reference_index_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_document_structure_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_dynamic_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_compatibility_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_inserted_content_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_mail_merge_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_barcode_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_form_field_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_display_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_action_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_sequence_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_numbering_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_document_info_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_filename_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_hyperlink_reason_hint(instruction) {
        return Some(reason);
    }
    if let Some(reason) = unsupported_merge_field_reason_hint(instruction) {
        return Some(reason);
    }
    None
}

fn unsupported_ref_reason_hint(instruction: &str, ctx: &Ctx<'_>) -> Option<FieldUnsupportedReason> {
    let field_bookmarks = ctx.field_bookmarks.borrow();
    let syntax = if FieldKind::from_instruction(instruction) == FieldKind::Ref {
        let Some(syntax) = ref_field_syntax(instruction) else {
            return Some(FieldUnsupportedReason::UnsupportedSwitch);
        };
        syntax
    } else {
        let syntax = direct_ref_field_syntax(instruction)?;
        if !ctx.bookmark_names.contains(&syntax.target)
            && !field_bookmarks.contains_key(&syntax.target)
        {
            return None;
        }
        syntax
    };
    if ctx.bookmark_names.contains(&syntax.target) || field_bookmarks.contains_key(&syntax.target) {
        Some(FieldUnsupportedReason::NoComputedResult)
    } else {
        Some(FieldUnsupportedReason::UnresolvedBookmark)
    }
}

fn unsupported_page_ref_reason_hint(
    instruction: &str,
    ctx: &Ctx<'_>,
) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::PageRef {
        return None;
    }
    let Some(syntax) = page_ref_field_syntax(instruction) else {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    };
    if ctx
        .page_ref_context
        .target_uses_unsupported_display_format(&syntax.target)
    {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    }
    if ctx.bookmark_names.contains(&syntax.target) {
        Some(FieldUnsupportedReason::NoComputedResult)
    } else {
        Some(FieldUnsupportedReason::UnresolvedBookmark)
    }
}

fn unsupported_note_ref_reason_hint(
    instruction: &str,
    ctx: &Ctx<'_>,
) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::NoteRef {
        return None;
    }
    let Some(syntax) = note_ref_field_syntax(instruction) else {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    };
    if ctx.note_ref_context.target_is_note_marker(&syntax.target)
        || ctx.bookmark_names.contains(&syntax.target)
    {
        Some(FieldUnsupportedReason::NoComputedResult)
    } else {
        Some(FieldUnsupportedReason::UnresolvedBookmark)
    }
}

fn unsupported_toc_reason_hint(instruction: &str, ctx: &Ctx<'_>) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::Toc {
        return None;
    }
    let Some(syntax) = toc_field_syntax(instruction) else {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    };
    match syntax.bookmark {
        Some(target) if ctx.bookmark_names.contains(&target) => {
            Some(FieldUnsupportedReason::NoComputedResult)
        }
        Some(_) => Some(FieldUnsupportedReason::UnresolvedBookmark),
        None => Some(FieldUnsupportedReason::NoComputedResult),
    }
}

fn unsupported_toc_entry_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::TocEntry {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        super::fields::supports_toc_entry_field_syntax(instruction),
    ))
}

fn unsupported_page_reason_hint(
    instruction: &str,
    ctx: &Ctx<'_>,
) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::Page {
        return None;
    }
    if !super::fields::supports_page_field_syntax(instruction) {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    }
    if ctx
        .last_page_field_unsupported_display_format
        .borrow()
        .unwrap_or(false)
    {
        Some(FieldUnsupportedReason::UnsupportedSwitch)
    } else {
        Some(FieldUnsupportedReason::NoComputedResult)
    }
}

fn unsupported_reference_index_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    let FieldKind::ReferenceIndex(kind) = FieldKind::from_instruction(instruction) else {
        return None;
    };
    if is_generated_reference_index_kind(&kind) {
        return Some(unsupported_opaque_field_reason_hint(
            instruction,
            is_generated_reference_index_kind,
        ));
    }
    if is_reference_index_marker_kind(&kind) {
        return Some(FieldUnsupportedReason::UnsupportedSwitch);
    }
    None
}

fn is_generated_reference_index_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "BIBLIOGRAPHY" | "CITATION" | "INDEX" | "TOA"
    )
}

fn is_reference_index_marker_kind(kind: &str) -> bool {
    matches!(kind.to_ascii_uppercase().as_str(), "RD" | "TA" | "XE")
}

fn unsupported_document_structure_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    let FieldKind::DocumentStructure(kind) = FieldKind::from_instruction(instruction) else {
        return None;
    };
    if kind.eq_ignore_ascii_case("REVNUM") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_revision_number_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("SECTION") || kind.eq_ignore_ascii_case("SECTIONPAGES") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::is_section_field_instruction(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("STYLEREF") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_style_ref_field_syntax(instruction),
        ));
    }
    Some(FieldUnsupportedReason::NoComputedResult)
}

fn unsupported_dynamic_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    let FieldKind::Dynamic(kind) = FieldKind::from_instruction(instruction) else {
        return None;
    };
    if kind == "=" {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_formula_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("COMPARE") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_compare_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("IF") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_if_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("QUOTE") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_quote_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("FILLIN") || kind.eq_ignore_ascii_case("ASK") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_prompt_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("SET") {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_set_field_syntax(instruction),
        ));
    }
    if kind.eq_ignore_ascii_case("NEXT")
        || kind.eq_ignore_ascii_case("NEXTIF")
        || kind.eq_ignore_ascii_case("SKIPIF")
    {
        return Some(unsupported_syntax_field_reason_hint(
            super::fields::supports_merge_control_field_syntax(instruction),
        ));
    }
    Some(FieldUnsupportedReason::NoComputedResult)
}

fn unsupported_compatibility_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Compatibility(_)
    ) {
        return None;
    }
    Some(unsupported_opaque_field_reason_hint(
        instruction,
        is_compatibility_kind,
    ))
}

fn is_compatibility_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "ADDIN" | "DATA" | "GLOSSARY" | "HTMLACTIVEX" | "PRIVATE"
    )
}

fn unsupported_inserted_content_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::InsertedContent(_)
    ) {
        return None;
    }
    Some(unsupported_opaque_field_reason_hint(
        instruction,
        is_inserted_content_kind,
    ))
}

fn is_inserted_content_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "AUTOTEXT"
            | "AUTOTEXTLIST"
            | "DATABASE"
            | "DDE"
            | "DDEAUTO"
            | "EMBED"
            | "IMPORT"
            | "INCLUDE"
            | "INCLUDEPICTURE"
            | "INCLUDETEXT"
            | "LINK"
    )
}

fn unsupported_mail_merge_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::MailMerge(_)
    ) {
        return None;
    }
    Some(unsupported_opaque_field_reason_hint(
        instruction,
        is_mail_merge_kind,
    ))
}

fn unsupported_opaque_field_reason_hint(
    instruction: &str,
    is_kind: fn(&str) -> bool,
) -> FieldUnsupportedReason {
    if opaque_field_syntax(instruction, is_kind) {
        FieldUnsupportedReason::NoComputedResult
    } else {
        FieldUnsupportedReason::UnsupportedSwitch
    }
}

fn is_mail_merge_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "ADDRESSBLOCK" | "GREETINGLINE" | "MERGEREC" | "MERGESEQ"
    )
}

fn unsupported_barcode_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Barcode(_)
    ) {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(barcode_field_syntax(
        instruction,
    )))
}

fn unsupported_form_field_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::FormField(_)
    ) {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        legacy_form_field_syntax(instruction).is_some(),
    ))
}

fn unsupported_syntax_field_reason_hint(valid_syntax: bool) -> FieldUnsupportedReason {
    if valid_syntax {
        FieldUnsupportedReason::NoComputedResult
    } else {
        FieldUnsupportedReason::UnsupportedSwitch
    }
}

fn unsupported_display_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Display(_)
    ) {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        super::fields::supports_display_field_syntax(instruction),
    ))
}

fn unsupported_action_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Action(_)
    ) {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        super::fields::supports_action_field_syntax(instruction),
    ))
}

fn unsupported_sequence_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::Sequence {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        super::fields::supports_sequence_field_syntax(instruction),
    ))
}

fn unsupported_numbering_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Numbering(_)
    ) {
        return None;
    }
    Some(unsupported_syntax_field_reason_hint(
        super::fields::supports_numbering_field_syntax(instruction),
    ))
}

fn unsupported_document_info_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if !matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::DocumentInfo(_)
    ) {
        return None;
    }
    (!super::fields::supports_document_info_field_syntax(instruction))
        .then_some(FieldUnsupportedReason::UnsupportedSwitch)
}

fn unsupported_filename_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::Filename {
        return None;
    }
    (!super::fields::supports_filename_field_syntax(instruction))
        .then_some(FieldUnsupportedReason::UnsupportedSwitch)
}

fn unsupported_hyperlink_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::Hyperlink {
        return None;
    }
    hyperlink_instr_url(instruction)
        .is_none()
        .then_some(FieldUnsupportedReason::UnsupportedSwitch)
}

fn unsupported_merge_field_reason_hint(instruction: &str) -> Option<FieldUnsupportedReason> {
    if FieldKind::from_instruction(instruction) != FieldKind::MergeField {
        return None;
    }
    (!merge_field_syntax(instruction)).then_some(FieldUnsupportedReason::UnsupportedSwitch)
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
                ctx.last_page_field_unsupported_display_format.replace(Some(
                    ctx.page_ref_context
                        .page_field_uses_unsupported_display_format(index),
                ));
                ctx.page_ref_context.page_field_position(index)
            } else {
                None
            };
            super::fields::computed_page_result(instruction, position)
        })
        .or_else(|| {
            let (position, order) =
                if FieldKind::from_instruction(instruction) == FieldKind::PageRef {
                    let index = {
                        let mut cursor = ctx.page_ref_field_cursor.borrow_mut();
                        let index = *cursor;
                        *cursor += 1;
                        index
                    };
                    (
                        ctx.page_ref_context.field_position(index),
                        ctx.page_ref_context.field_order(index),
                    )
                } else {
                    (None, None)
                };
            super::fields::computed_page_ref_result(
                instruction,
                ctx.page_ref_context,
                position,
                order,
            )
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
                let heading_scope = *ctx.sequence_heading_counts.borrow();
                let mut counters = ctx.sequence_counters.borrow_mut();
                let mut heading_scopes = ctx.sequence_heading_scopes.borrow_mut();
                super::fields::computed_sequence_result_with_heading_scope(
                    instruction,
                    &mut counters,
                    Some(heading_scope),
                    &mut heading_scopes,
                )
            } else {
                None
            }
        })
        .or_else(|| super::fields::computed_toc_entry_result(instruction))
        .or_else(|| {
            if matches!(
                FieldKind::from_instruction(instruction),
                FieldKind::Numbering(kind)
                    if kind == "AUTONUM"
                        || kind == "AUTONUMLGL"
                        || kind == "AUTONUMOUT"
                        || kind == "BIDIOUTLINE"
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
        .or_else(|| {
            super::fields::computed_toc_result(instruction, ctx.toc_entries, ctx.bookmark_names)
        })
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
        let field_bookmarks = ctx.field_bookmarks.borrow();
        return super::fields::computed_formula_result_with_bookmark_context(
            instruction,
            ctx.ref_targets,
            &field_bookmarks,
        );
    }
    if matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Dynamic(kind) if kind == "IF" || kind == "COMPARE"
    ) {
        let field_bookmarks = ctx.field_bookmarks.borrow();
        return super::fields::computed_if_compare_result_with_bookmark_context(
            instruction,
            ctx.ref_targets,
            &field_bookmarks,
        );
    }
    if matches!(
        FieldKind::from_instruction(instruction),
        FieldKind::Dynamic(kind) if kind == "NEXTIF" || kind == "SKIPIF"
    ) {
        let field_bookmarks = ctx.field_bookmarks.borrow();
        return super::fields::computed_merge_control_result_with_bookmark_context(
            instruction,
            ctx.ref_targets,
            &field_bookmarks,
        );
    }
    let field_bookmarks = ctx.field_bookmarks.borrow();
    super::fields::computed_dynamic_result_with_bookmarks(instruction, &field_bookmarks)
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

fn apply_sequence_heading_scope(pp: &PPr, ctx: &Ctx<'_>, applied: &mut bool) {
    if *applied {
        return;
    }
    let Some(level) = sequence_heading_level(pp, ctx.styles) else {
        return;
    };
    let mut counts = ctx.sequence_heading_counts.borrow_mut();
    counts[usize::from(level - 1)] = counts[usize::from(level - 1)].saturating_add(1);
    *applied = true;
}

fn sequence_heading_level(pp: &PPr, styles: &Styles) -> Option<u8> {
    match pp.outline {
        Some(level) if level <= 8 => Some(level + 1),
        Some(_) => None,
        None => pp
            .style_id
            .as_deref()
            .and_then(|style_id| styles.heading_level(style_id)),
    }
}

/// Extract a URL from a `HYPERLINK "…"` field instruction (matches the `.doc`
/// field-code parser).
pub(crate) fn hyperlink_instr_url(instr: &str) -> Option<String> {
    let target = crate::annotation::hyperlink_field_target(instr)?;
    Some(if hyperlink_instr_uses_anchor_target(instr) {
        format!("#{target}")
    } else {
        target
    })
}

fn hyperlink_instr_uses_anchor_target(instr: &str) -> bool {
    let tokens = instruction_parts(instr);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("HYPERLINK") {
        return false;
    }
    let Some(first_arg) = parts.next() else {
        return false;
    };
    let lower = first_arg.to_ascii_lowercase();
    lower == "\\l" || lower.starts_with("\\l")
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
    let mut props = TableProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tblPr" => props = read_tblpr(r),
                b"tr" => rows.push(read_row(r, ctx, depth)),
                b"AlternateContent" => {
                    rows.extend(read_table_alternate_content_rows(r, ctx, depth + 1))
                }
                name if is_current_table_structural_wrapper(name) => {}
                _ => skip_subtree(r), // tblGrid, …
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tbl" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    build_table(rows, props)
}

fn is_current_table_structural_wrapper(name: &[u8]) -> bool {
    matches!(
        name,
        b"sdt" | b"sdtContent" | b"customXml" | b"smartTag" | b"ins" | b"moveTo"
    )
}

fn read_table_alternate_content_rows(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
) -> Vec<(Vec<CellRaw>, bool)> {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return Vec::new();
    }
    let mut rows = Vec::new();
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        rows.extend(read_table_alternate_content_branch_rows(
                            r,
                            ctx,
                            depth + 1,
                            name,
                        ));
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rows
}

fn read_table_alternate_content_branch_rows(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    branch: &[u8],
) -> Vec<(Vec<CellRaw>, bool)> {
    let mut rows = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"tr" => rows.push(read_row(r, ctx, depth)),
                b"AlternateContent" => {
                    rows.extend(read_table_alternate_content_rows(r, ctx, depth + 1))
                }
                name if is_current_table_structural_wrapper(name) => {}
                _ => skip_subtree(r),
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    rows
}

#[derive(Default)]
struct TableProps {
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
}

/// Read `<w:tblPr>` layout metadata.
fn read_tblpr(r: &mut Xml<'_>) -> TableProps {
    let mut props = TableProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tblpr_alternate_content(r, &mut props);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblW"
                    && attr_local_trimmed(&e, b"type").is_some_and(|value| value == "pct") =>
            {
                props.width_pct = attr_f32(&e, b"w").map(|p| p / 5000.0);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblLayout" =>
            {
                props.fixed_layout =
                    attr_local_trimmed(&e, b"type").is_some_and(|value| value == "fixed");
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"tblInd" => {
                if type_defaults_to_dxa(&e) {
                    props.indent_twips = attr_i32(&e, b"w");
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"jc" => {
                props.align = match attr_local_trimmed(&e, b"val").as_deref() {
                    Some("center") => Some(Align::Center),
                    Some("right") => Some(Align::Right),
                    Some("both") => Some(Align::Justify),
                    Some("left") | Some("start") => Some(Align::Left),
                    _ => None,
                };
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblBorders" => {
                let borders = read_tbl_borders(r);
                props.border_color = borders.0;
                props.border_colors = borders.1;
                props.border_size_eighths = borders.2;
                props.border_sizes = borders.3;
                props.border_style = borders.4;
                props.border_styles = borders.5;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tblPr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_tblpr_alternate_content(r: &mut Xml<'_>, props: &mut TableProps) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_tblpr_alternate_content_branch(r, props, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_tblpr_alternate_content_branch(r: &mut Xml<'_>, props: &mut TableProps, branch: &[u8]) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tblpr_alternate_content(r, props);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblW"
                    && attr_local_trimmed(&e, b"type").is_some_and(|value| value == "pct") =>
            {
                props.width_pct = attr_f32(&e, b"w").map(|p| p / 5000.0);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblLayout" =>
            {
                props.fixed_layout =
                    attr_local_trimmed(&e, b"type").is_some_and(|value| value == "fixed");
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"tblInd" => {
                if type_defaults_to_dxa(&e) {
                    props.indent_twips = attr_i32(&e, b"w");
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"jc" => {
                props.align = match attr_local_trimmed(&e, b"val").as_deref() {
                    Some("center") => Some(Align::Center),
                    Some("right") => Some(Align::Right),
                    Some("both") => Some(Align::Justify),
                    Some("left") | Some("start") => Some(Align::Left),
                    _ => None,
                };
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblBorders" => {
                let borders = read_tbl_borders(r);
                props.border_color = borders.0;
                props.border_colors = borders.1;
                props.border_size_eighths = borders.2;
                props.border_sizes = borders.3;
                props.border_style = borders.4;
                props.border_styles = borders.5;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
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
    let mut borders = TableBorderProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tbl_borders_alternate_content(r, &mut borders);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                borders.record(&e);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tblBorders" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    borders.finish()
}

struct TableBorderProps {
    color: Option<Color>,
    colors: TableBorderColors,
    color_seen: bool,
    color_consistent: bool,
    size: Option<u16>,
    sizes: TableBorderSizes,
    size_seen: bool,
    size_consistent: bool,
    style: Option<TableBorderStyle>,
    styles: TableBorderStyles,
    style_seen: bool,
    style_consistent: bool,
}

impl Default for TableBorderProps {
    fn default() -> Self {
        Self {
            color: None,
            colors: TableBorderColors::default(),
            color_seen: false,
            color_consistent: true,
            size: None,
            sizes: TableBorderSizes::default(),
            size_seen: false,
            size_consistent: true,
            style: None,
            styles: TableBorderStyles::default(),
            style_seen: false,
            style_consistent: true,
        }
    }
}

impl TableBorderProps {
    fn record(&mut self, e: &BytesStart<'_>) {
        let Some(side) = table_border_side(e) else {
            return;
        };
        if let Some(next) = attr_local(e, b"color").and_then(|v| parse_rgb_hex_color(&v)) {
            self.colors.set(side, next);
            self.color_seen = true;
            match self.color {
                Some(current) if current != next => self.color_consistent = false,
                None => self.color = Some(next),
                _ => {}
            }
        }
        if let Some(next) = attr_u16(e, b"sz").filter(|v| *v > 0) {
            self.sizes.set(side, next);
            self.size_seen = true;
            match self.size {
                Some(current) if current != next => self.size_consistent = false,
                None => self.size = Some(next),
                _ => {}
            }
        }
        if let Some(next) = attr_local(e, b"val").and_then(|v| TableBorderStyle::from_wml_value(&v))
        {
            self.styles.set(side, next);
            self.style_seen = true;
            match self.style {
                Some(current) if current != next => self.style_consistent = false,
                None => self.style = Some(next),
                _ => {}
            }
        }
    }

    fn finish(
        self,
    ) -> (
        Option<Color>,
        TableBorderColors,
        Option<u16>,
        TableBorderSizes,
        Option<TableBorderStyle>,
        TableBorderStyles,
    ) {
        let uniform_color = if self.color_seen && self.color_consistent {
            self.color
        } else {
            None
        };
        let uniform_size = if self.size_seen && self.size_consistent {
            self.size
        } else {
            None
        };
        let uniform_style = if self.style_seen && self.style_consistent {
            self.style
        } else {
            None
        };
        (
            uniform_color,
            self.colors,
            uniform_size,
            self.sizes,
            uniform_style,
            self.styles,
        )
    }
}

fn read_tbl_borders_alternate_content(r: &mut Xml<'_>, borders: &mut TableBorderProps) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_tbl_borders_alternate_content_branch(r, borders, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_tbl_borders_alternate_content_branch(
    r: &mut Xml<'_>,
    borders: &mut TableBorderProps,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tbl_borders_alternate_content(r, borders);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                borders.record(&e);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
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
                b"AlternateContent" => {
                    let (branch_cells, branch_header) =
                        read_row_alternate_content_cells(r, ctx, depth + 1);
                    cells.extend(branch_cells);
                    if let Some(value) = branch_header {
                        header = value;
                    }
                }
                name if is_current_table_structural_wrapper(name) => {}
                _ => skip_subtree(r),
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tr" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (cells, header)
}

fn read_row_alternate_content_cells(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
) -> (Vec<CellRaw>, Option<bool>) {
    if depth > MAX_DEPTH {
        skip_subtree(r);
        return (Vec::new(), None);
    }
    let mut cells = Vec::new();
    let mut header = None;
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        let (branch_cells, branch_header) =
                            read_row_alternate_content_branch_cells(r, ctx, depth + 1, name);
                        cells.extend(branch_cells);
                        if branch_header.is_some() {
                            header = branch_header;
                        }
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    (cells, header)
}

fn read_row_alternate_content_branch_cells(
    r: &mut Xml<'_>,
    ctx: &Ctx<'_>,
    depth: u32,
    branch: &[u8],
) -> (Vec<CellRaw>, Option<bool>) {
    let mut cells = Vec::new();
    let mut header = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => match local(e.name().as_ref()) {
                b"trPr" => header = Some(read_trpr(r)),
                b"tc" => cells.push(read_cell(r, ctx, depth + 1)),
                b"AlternateContent" => {
                    let (branch_cells, branch_header) =
                        read_row_alternate_content_cells(r, ctx, depth + 1);
                    cells.extend(branch_cells);
                    if branch_header.is_some() {
                        header = branch_header;
                    }
                }
                name if is_current_table_structural_wrapper(name) => {}
                _ => skip_subtree(r),
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"trPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_trpr_alternate_content(r) {
                    header = value;
                }
            }
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

fn read_trpr_alternate_content(r: &mut Xml<'_>) -> Option<bool> {
    let mut took = false;
    let mut header = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        header = read_trpr_alternate_content_branch(r, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    header
}

fn read_trpr_alternate_content_branch(r: &mut Xml<'_>, branch: &[u8]) -> Option<bool> {
    let mut header = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"trPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_trpr_alternate_content(r) {
                    header = Some(value);
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"tblHeader" =>
            {
                header = Some(toggle_on(attr_local(&e, b"val")));
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
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
                b"AlternateContent" => {
                    blocks.extend(read_alternate_content_blocks(r, ctx, depth + 1))
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
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tcPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tcpr_alternate_content(r, &mut t);
            }
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

fn read_tcpr_alternate_content(r: &mut Xml<'_>, t: &mut TcPr) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_tcpr_alternate_content_branch(r, t, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_tcpr_alternate_content_branch(r: &mut Xml<'_>, t: &mut TcPr, branch: &[u8]) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tcPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tcMar" => {
                t.margins = read_tc_mar(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tcpr_alternate_content(r, t);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_tcpr_child(t, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_tcpr_child(t: &mut TcPr, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"gridSpan" => {
            if let Some(v) = attr_u16(e, b"val") {
                t.gs = v.max(1);
            }
        }
        b"vMerge" => {
            t.vm = match attr_local_trimmed(e, b"val").as_deref() {
                Some("restart") => VMerge::Restart,
                _ => VMerge::Continue, // present with "continue"/no val
            };
        }
        b"shd" => t.shading = attr_local(e, b"fill").and_then(|v| parse_rgb_hex_color(&v)),
        b"vAlign" => {
            t.valign = match attr_local_trimmed(e, b"val").as_deref() {
                Some("center") => VCell::Center,
                Some("bottom") => VCell::Bottom,
                _ => VCell::Top,
            };
        }
        // `type="pct"` w:w is in fiftieths of a percent (5000 = 100%);
        // `dxa` (twips) is absolute and left as auto here.
        b"tcW" if attr_local_trimmed(e, b"type").is_some_and(|value| value == "pct") => {
            t.width_pct = attr_f32(e, b"w").map(|p| p / 5000.0);
        }
        _ => {}
    }
}

fn read_tc_mar(r: &mut Xml<'_>) -> Option<CellMargins> {
    let mut margins = CellMargins::default();
    let mut seen = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tc_mar_alternate_content(r, &mut margins, &mut seen);
            }
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

fn read_tc_mar_alternate_content(r: &mut Xml<'_>, margins: &mut CellMargins, seen: &mut bool) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_tc_mar_alternate_content_branch(r, margins, seen, name);
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_tc_mar_alternate_content_branch(
    r: &mut Xml<'_>,
    margins: &mut CellMargins,
    seen: &mut bool,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_tc_mar_alternate_content(r, margins, seen);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                apply_tc_mar_side(margins, seen, &e);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn apply_tc_mar_side(margins: &mut CellMargins, seen: &mut bool, e: &BytesStart<'_>) {
    let name = e.name();
    let side = local(name.as_ref());
    if !matches!(side, b"top" | b"right" | b"bottom" | b"left") {
        return;
    }
    if !type_defaults_to_dxa(e) {
        return;
    }
    let Some(value) = attr_u32(e, b"w") else {
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
fn build_table(raw_rows: Vec<(Vec<CellRaw>, bool)>, props: TableProps) -> Table {
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
        fixed_layout: props.fixed_layout,
        indent_twips: props.indent_twips,
        align: props.align,
        width_pct: props.width_pct,
        border_color: props.border_color,
        border_colors: props.border_colors,
        border_size_eighths: props.border_size_eighths,
        border_sizes: props.border_sizes,
        border_style: props.border_style,
        border_styles: props.border_styles,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Block;

    fn parse(xml: &str) -> Vec<Block> {
        parse_with_media(xml, HashMap::new())
    }

    fn parse_with_media(xml: &str, media: HashMap<String, Image>) -> Vec<Block> {
        let styles = Styles::default();
        let numbering = Numbering::default();
        let rels = HashMap::new();
        let ref_targets = HashMap::new();
        let ref_position_context = super::super::fields::RefPositionContext::default();
        let ref_number_context = super::super::fields::RefNumberContext::empty();
        let page_ref_context = super::super::fields::PageRefContext::empty();
        let note_ref_context = super::super::fields::NoteRefContext::empty();
        let section_context = super::super::fields::SectionContext::empty();
        let style_ref_context = super::super::fields::StyleRefContext::default();
        let legacy_form_context = super::super::fields::LegacyFormContext::default();
        let table_formula_context = super::super::fields::TableFormulaContext::default();
        let toc_entries = Vec::new();
        let bookmark_names = HashSet::new();
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
            bookmark_names: &bookmark_names,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            last_page_field_unsupported_display_format: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            sequence_heading_counts: Default::default(),
            sequence_heading_scopes: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
            counters: Default::default(),
        };
        parse_document(xml, &ctx)
    }

    #[test]
    fn hyperlink_instruction_rejects_trailing_non_switch_tokens() {
        assert_eq!(
            hyperlink_instr_url(r#"HYPERLINK "https://example.com" \o "tip""#).as_deref(),
            Some("https://example.com")
        );
        assert_eq!(
            hyperlink_instr_url(r#"HYPERLINK "https://example.com" "extra "#),
            None
        );
        assert_eq!(
            hyperlink_instr_url(r#"HYPERLINK "https://example.com" extra"#),
            None
        );
    }

    #[test]
    fn hyperlink_anchor_trims_ooxml_value() {
        let xml = r#"<w:document><w:body><w:p>
            <w:hyperlink w:anchor=" TargetBookmark "><w:r><w:t>Jump</w:t></w:r></w:hyperlink>
        </w:p></w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        assert!(matches!(
            &p.runs[0].field,
            FieldRole::Hyperlink { url } if url == "#TargetBookmark"
        ));
    }

    #[test]
    fn empty_unsupported_simple_fields_are_counted_in_model_inventory() {
        let xml = r#"<w:document><w:body><w:p>
            <w:fldSimple w:instr=" DOESNOTEXIST "/>
            <w:fldSimple w:instr=" CUSTOMEMPTY "></w:fldSimple>
        </w:p></w:body></w:document>"#;
        let blocks = parse(xml);

        let inventory = crate::report::feature_inventory_for_model(&blocks);

        assert_eq!(inventory.fields, 2);
        assert_eq!(
            inventory.unsupported_field_kinds,
            vec![
                crate::FieldKindCount {
                    kind: FieldKind::Unknown("DOESNOTEXIST".to_string()),
                    count: 1,
                },
                crate::FieldKindCount {
                    kind: FieldKind::Unknown("CUSTOMEMPTY".to_string()),
                    count: 1,
                },
            ]
        );
        assert_eq!(
            inventory.unsupported_field_reasons,
            vec![crate::FieldEvaluationReasonCount {
                reason: crate::FieldEvaluationReason::UnknownField,
                count: 2,
            }]
        );
        #[cfg(feature = "render")]
        {
            let render_inventory = crate::report::render_inventory_for_model(&blocks);
            assert_eq!(render_inventory.fields, inventory.fields);
            assert_eq!(
                render_inventory.unsupported_field_kinds,
                inventory.unsupported_field_kinds
            );
            assert_eq!(
                render_inventory.unsupported_field_reasons,
                inventory.unsupported_field_reasons
            );
        }
    }

    #[test]
    fn empty_unsupported_complex_fields_are_counted_in_model_inventory() {
        let xml = r#"<w:document><w:body><w:p>
            <w:r><w:fldChar w:fldCharType="begin"/></w:r>
            <w:r><w:instrText> CUSTOMCOMPLEX </w:instrText></w:r>
            <w:r><w:fldChar w:fldCharType="end"/></w:r>
        </w:p></w:body></w:document>"#;
        let blocks = parse(xml);

        let inventory = crate::report::feature_inventory_for_model(&blocks);

        assert_eq!(inventory.fields, 1);
        assert_eq!(
            inventory.unsupported_field_kinds,
            vec![crate::FieldKindCount {
                kind: FieldKind::Unknown("CUSTOMCOMPLEX".to_string()),
                count: 1,
            }]
        );
        assert_eq!(
            inventory.unsupported_field_reasons,
            vec![crate::FieldEvaluationReasonCount {
                reason: crate::FieldEvaluationReason::UnknownField,
                count: 1,
            }]
        );
        #[cfg(feature = "render")]
        {
            let render_inventory = crate::report::render_inventory_for_model(&blocks);
            assert_eq!(render_inventory.fields, inventory.fields);
            assert_eq!(
                render_inventory.unsupported_field_kinds,
                inventory.unsupported_field_kinds
            );
            assert_eq!(
                render_inventory.unsupported_field_reasons,
                inventory.unsupported_field_reasons
            );
        }
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
    fn paragraph_and_run_string_attrs_trim_ooxml_values() {
        let xml = r#"<w:document><w:body>
            <w:p><w:pPr><w:jc w:val=" center "/></w:pPr>
                <w:r><w:rPr><w:rFonts w:ascii=" Arial " w:eastAsia=" 맑은 고딕 "/><w:highlight w:val=" yellow "/></w:rPr><w:t>Styled</w:t></w:r>
            </w:p>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        assert_eq!(p.props.align, Align::Center);
        assert_eq!(p.runs[0].props.font.as_deref(), Some("맑은 고딕"));
        assert_eq!(p.runs[0].props.highlight.as_deref(), Some("yellow"));
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
    fn image_rotation_trims_ooxml_units() {
        let mut media = HashMap::new();
        media.insert("rIdImg".to_string(), Image::default());
        let xml = r#"<w:document><w:body><w:p><w:r><w:drawing><wp:inline>
            <a:blip r:embed="rIdImg"/>
            <a:xfrm rot=" 5400000 "/>
        </wp:inline></w:drawing></w:r></w:p></w:body></w:document>"#;
        let blocks = parse_with_media(xml, media);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        let image = p
            .runs
            .iter()
            .find_map(|run| run.image.as_ref())
            .expect("image run");
        assert_eq!(image.rotation_degrees, Some(90));
    }

    #[test]
    fn floating_image_offsets_trim_relative_from() {
        let mut media = HashMap::new();
        media.insert("rIdImg".to_string(), Image::default());
        let xml = r#"<w:document><w:body><w:p><w:r><w:drawing><wp:anchor>
            <wp:positionH relativeFrom=" page "><wp:posOffset>91440</wp:posOffset></wp:positionH>
            <wp:positionV relativeFrom=" page "><wp:posOffset>182880</wp:posOffset></wp:positionV>
            <a:blip r:embed="rIdImg"/>
        </wp:anchor></w:drawing></w:r></w:p></w:body></w:document>"#;
        let blocks = parse_with_media(xml, media);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        let image = p
            .runs
            .iter()
            .find_map(|run| run.image.as_ref())
            .expect("image run");
        assert_eq!(image.floating_offset_emu, Some((91440, 182880)));
    }

    #[test]
    fn scans_header_footer_references() {
        let xml = r#"<w:document><w:body>
            <w:p><w:r><w:t>x</w:t></w:r></w:p>
            <w:sectPr>
                <w:headerReference w:type="default" r:id="rIdH"/>
                <w:headerReference w:type=" " r:id=" rIdDefault "/>
                <w:headerReference w:type="first" r:id=" "/>
                <w:footerReference w:type="default" r:id="rIdF"/>
                <w:footerReference w:type="even" r:id=" "/>
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
                    rel_id: "rIdDefault".to_string(),
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
    fn scans_header_footer_references_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sectPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:headerReference w:type="default" r:id="rIdChoiceHeader"/>
                        <w:footerReference w:type="first" r:id="rIdChoiceFooter"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:headerReference w:type="default" r:id="rIdFallbackHeader"/>
                        <w:footerReference w:type="first" r:id="rIdFallbackFooter"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sectPr>
        </w:body></w:document>"#;
        let (headers, footers) = scan_hf_refs(xml);
        assert_eq!(
            headers,
            vec![HeaderFooterRef {
                rel_id: "rIdChoiceHeader".to_string(),
                type_name: "default".to_string()
            }]
        );
        assert_eq!(
            footers,
            vec![HeaderFooterRef {
                rel_id: "rIdChoiceFooter".to_string(),
                type_name: "first".to_string()
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
    fn scans_final_section_page_number_uses_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sectPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:pgNumType w:start="7" w:fmt="decimalZero"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:pgNumType w:start="12" w:fmt="upperRoman"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sectPr>
        </w:body></w:document>"#;
        assert_eq!(scan_page_number_start(xml), Some(7));
        assert_eq!(
            scan_page_number_format(xml),
            Some(PageNumberFormat::DecimalZero)
        );
    }

    #[test]
    fn page_orientation_trims_ooxml_value() {
        let xml = r#"<w:document><w:body>
            <w:p><w:pPr><w:sectPr><w:pgSz w:w="15840" w:h="12240" w:orient=" landscape "/></w:sectPr></w:pPr><w:r><w:t>x</w:t></w:r></w:p>
        </w:body></w:document>"#;
        assert!(scan_page_setup(xml).landscape);
        let blocks = parse(xml);
        let section = blocks
            .iter()
            .find_map(|block| match block {
                Block::SectionBreak(section) => Some(section),
                _ => None,
            })
            .expect("section break");
        assert!(section.page.landscape);
    }

    #[test]
    fn page_setup_uses_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sectPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>
                        <w:pgMar w:left="720" w:right="1080" w:top="1440" w:bottom="1800"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:pgSz w:w="12240" w:h="15840"/>
                        <w:pgMar w:left="1440" w:right="1440" w:top="1440" w:bottom="1440"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sectPr>
        </w:body></w:document>"#;
        let page = scan_page_setup(xml);

        assert!(page.landscape);
        assert_eq!(page.width_pt, 792.0);
        assert_eq!(page.height_pt, 612.0);
        assert_eq!(page.margin_pt, 36.0);
        assert_eq!(page.margin_left_pt, Some(36.0));
        assert_eq!(page.margin_right_pt, Some(54.0));
        assert_eq!(page.margin_top_pt, Some(72.0));
        assert_eq!(page.margin_bottom_pt, Some(90.0));
    }

    #[test]
    fn final_section_setup_scanners_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sectPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:cols w:num="2"/>
                        <w:textDirection w:val="tbRl"/>
                        <w:docGrid w:type="lines" w:linePitch="360" w:charSpace="120"/>
                        <w:titlePg/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:cols w:num="5"/>
                        <w:textDirection w:val="lrTb"/>
                        <w:docGrid w:type="snapToChars" w:linePitch="720" w:charSpace="240"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sectPr>
        </w:body></w:document>"#;

        assert_eq!(scan_section_columns(xml), Some(2));
        assert_eq!(
            scan_section_text_direction(xml),
            Some(TextDirection::TopToBottomRightToLeft)
        );
        assert_eq!(
            scan_section_doc_grid(xml),
            Some(DocGrid {
                grid_type: DocGridType::Lines,
                line_pitch: Some(360),
                character_space: Some(120),
            })
        );
        assert!(scan_section_title_page(xml));
    }

    #[test]
    fn section_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:p><w:pPr><w:sectPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:type w:val="nextPage"/>
                        <w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>
                        <w:pgNumType w:start="3" w:fmt="upperRoman"/>
                        <w:cols w:num="2"/>
                        <w:textDirection w:val="tbRl"/>
                        <w:docGrid w:type="lines" w:linePitch="360" w:charSpace="120"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:type w:val="continuous"/>
                        <w:pgSz w:w="12240" w:h="15840"/>
                        <w:pgNumType w:start="9" w:fmt="decimalZero"/>
                        <w:cols w:num="5"/>
                        <w:textDirection w:val="lrTb"/>
                        <w:docGrid w:type="snapToChars" w:linePitch="720" w:charSpace="240"/>
                        <w:titlePg/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sectPr></w:pPr></w:p>
        </w:body></w:document>"#;
        let section = parse(xml)
            .into_iter()
            .find_map(|block| match block {
                Block::SectionBreak(section) => Some(section),
                _ => None,
            })
            .expect("section break");

        assert_eq!(section.section_break, Some(SectionBreakKind::NextPage));
        assert!(section.page.landscape);
        assert_eq!(section.page_number_start, Some(3));
        assert_eq!(
            section.page_number_format,
            Some(PageNumberFormat::UpperRoman)
        );
        assert_eq!(section.columns, Some(2));
        assert_eq!(
            section.text_direction,
            Some(TextDirection::TopToBottomRightToLeft)
        );
        assert_eq!(
            section.doc_grid,
            Some(DocGrid {
                grid_type: DocGridType::Lines,
                line_pitch: Some(360),
                character_space: Some(120),
            })
        );
        assert!(!section.title_page);
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
        let style_ref_context = super::super::fields::StyleRefContext::default();
        let legacy_form_context = super::super::fields::LegacyFormContext::default();
        let table_formula_context = super::super::fields::TableFormulaContext::default();
        let toc_entries = Vec::new();
        let bookmark_names = HashSet::new();
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
            bookmark_names: &bookmark_names,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            last_page_field_unsupported_display_format: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            sequence_heading_counts: Default::default(),
            sequence_heading_scopes: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
            counters: Default::default(),
        };
        let xml = r#"<w:footnotes>
            <w:footnote w:type=" separator " w:id="-1"><w:p><w:r><w:t>SEP</w:t></w:r></w:p></w:footnote>
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
    fn note_entries_use_single_alternate_content_branch() {
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
        let style_ref_context = super::super::fields::StyleRefContext::default();
        let legacy_form_context = super::super::fields::LegacyFormContext::default();
        let table_formula_context = super::super::fields::TableFormulaContext::default();
        let toc_entries = Vec::new();
        let bookmark_names = HashSet::new();
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
            bookmark_names: &bookmark_names,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            last_page_field_unsupported_display_format: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            sequence_heading_counts: Default::default(),
            sequence_heading_scopes: Default::default(),
            autonum_counter: Default::default(),
            listnum_counter: Default::default(),
            field_bookmarks: Default::default(),
            counters: Default::default(),
        };
        let xml = r#"<w:footnotes xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <mc:AlternateContent>
                <mc:Choice Requires="w14">
                    <w:footnote w:id="1"><w:p><w:r><w:t>Choice note</w:t></w:r></w:p></w:footnote>
                </mc:Choice>
                <mc:Fallback>
                    <w:footnote w:id="9"><w:p><w:r><w:t>Fallback note</w:t></w:r></w:p></w:footnote>
                </mc:Fallback>
            </mc:AlternateContent>
        </w:footnotes>"#;

        let entries = parse_note_entries(xml, &ctx, b"footnote");
        let notes: Vec<_> = entries
            .iter()
            .map(|(id, blocks)| {
                let text = blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Paragraph(paragraph) => Some(paragraph.text()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("|");
                (id.as_str(), text)
            })
            .collect();

        assert_eq!(notes, vec![("1", "Choice note".to_string())]);
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
        let style_ref_context = super::super::fields::StyleRefContext::default();
        let legacy_form_context = super::super::fields::LegacyFormContext::default();
        let table_formula_context = super::super::fields::TableFormulaContext::default();
        let toc_entries = Vec::new();
        let bookmark_names = HashSet::new();
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
            bookmark_names: &bookmark_names,
            core_properties: &core_properties,
            custom_properties: &custom_properties,
            document_variables: &document_variables,
            extended_properties: &extended_properties,
            file_size_bytes: None,
            ref_field_cursor: Default::default(),
            page_field_cursor: Default::default(),
            last_page_field_unsupported_display_format: Default::default(),
            page_ref_field_cursor: Default::default(),
            note_ref_field_cursor: Default::default(),
            section_field_cursor: Default::default(),
            style_ref_field_cursor: Default::default(),
            form_field_cursor: Default::default(),
            formula_field_cursor: Default::default(),
            sequence_counters: Default::default(),
            sequence_heading_counts: Default::default(),
            sequence_heading_scopes: Default::default(),
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
        use crate::model::{CellMargins, Color, VCell, VertAlign};
        let xml = r#"<w:document><w:body>
            <w:p>
                <w:pPr><w:spacing w:before="240" w:after="120" w:line="360"/><w:ind w:left="720" w:firstLine="240"/><w:shd w:fill=" EEEEEE "/></w:pPr>
                <w:r><w:rPr><w:rFonts w:ascii="Arial" w:eastAsia="맑은 고딕"/><w:sz w:val="24"/><w:color w:val=" FF0000 "/><w:vertAlign w:val=" superscript "/><w:caps/></w:rPr><w:t>빨강</w:t></w:r>
            </w:p>
            <w:tbl><w:tblPr><w:tblW w:w="4000" w:type=" pct "/><w:tblLayout w:type=" fixed "/><w:tblInd w:w="720" w:type=" dxa "/><w:jc w:val=" center "/></w:tblPr><w:tr><w:tc>
                <w:tcPr><w:shd w:fill=" DDDDDD "/><w:vAlign w:val=" center "/><w:tcW w:w="2500" w:type=" pct "/><w:tcMar><w:top w:w="120" w:type=" dxa "/><w:right w:w="240" w:type=" dxa "/></w:tcMar></w:tcPr>
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
        assert_eq!(t.width_pct, Some(0.8));
        assert!(t.fixed_layout);
        assert_eq!(t.indent_twips, Some(720));
        assert_eq!(t.align, Some(Align::Center));
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
        assert_eq!(
            c.margins,
            Some(CellMargins {
                top: 120,
                right: 240,
                ..CellMargins::default()
            })
        );
    }

    #[test]
    fn underline_none_trims_ooxml_value() {
        let xml = r#"<w:document><w:body><w:p>
            <w:r><w:rPr><w:u w:val=" none "/></w:rPr><w:t>off</w:t></w:r>
            <w:r><w:rPr><w:u/></w:rPr><w:t>on</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(p) = &blocks[0] else {
            panic!("para");
        };
        assert!(!p.runs[0].props.underline);
        assert!(p.runs[1].props.underline);
    }

    #[test]
    fn run_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p>
            <w:r><w:rPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:b/>
                        <w:i/>
                        <w:u w:val="single"/>
                        <w:strike/>
                        <w:smallCaps/>
                        <w:caps/>
                        <w:rFonts w:ascii="Choice Latin" w:eastAsia="Choice Korean"/>
                        <w:sz w:val="28"/>
                        <w:color w:val="112233"/>
                        <w:highlight w:val="yellow"/>
                        <w:vertAlign w:val="superscript"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:b w:val="false"/>
                        <w:i w:val="false"/>
                        <w:u w:val="none"/>
                        <w:strike w:val="false"/>
                        <w:smallCaps w:val="false"/>
                        <w:caps w:val="false"/>
                        <w:rFonts w:ascii="Fallback Latin" w:eastAsia="Fallback Korean"/>
                        <w:sz w:val="20"/>
                        <w:color w:val="445566"/>
                        <w:highlight w:val="green"/>
                        <w:vertAlign w:val="subscript"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:rPr><w:t>Run properties</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("paragraph");
        };
        let props = &p.runs[0].props;

        assert!(props.bold);
        assert!(props.italic);
        assert!(props.underline);
        assert!(props.strike);
        assert!(props.small_caps);
        assert!(props.caps);
        assert_eq!(props.font.as_deref(), Some("Choice Korean"));
        assert_eq!(props.size_half_pt, Some(28));
        assert_eq!(
            props.color,
            Some(Color {
                r: 0x11,
                g: 0x22,
                b: 0x33
            })
        );
        assert_eq!(props.highlight.as_deref(), Some("yellow"));
        assert_eq!(props.vert_align, VertAlign::Super);
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
    fn page_break_type_trims_ooxml_value() {
        let xml = r#"<w:document><w:body><w:p>
            <w:r><w:t>before</w:t><w:br w:type=" page "/><w:t>after</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let blocks = parse(xml);
        assert!(matches!(blocks.get(1), Some(Block::PageBreak)));
        let Block::Paragraph(after) = &blocks[2] else {
            panic!("paragraph after break");
        };
        assert_eq!(after.text(), "after");
    }

    #[test]
    fn paragraph_spacing_line_rule_trims_ooxml_value() {
        let xml = r#"<w:document><w:body><w:p>
            <w:pPr><w:spacing w:line="360" w:lineRule=" exact "/></w:pPr>
            <w:r><w:t>exact line spacing</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("paragraph");
        };

        assert_eq!(p.props.spacing.line_pct, None);
    }

    #[test]
    fn paragraph_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p>
            <w:pPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:jc w:val="center"/>
                        <w:spacing w:before="240" w:after="120" w:line="360"/>
                        <w:ind w:left="720" w:firstLine="240"/>
                        <w:shd w:fill="EEEEEE"/>
                        <w:pageBreakBefore w:val="true"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:jc w:val="right"/>
                        <w:spacing w:before="480" w:after="360" w:line="480"/>
                        <w:ind w:left="1440" w:firstLine="480"/>
                        <w:shd w:fill="111111"/>
                        <w:pageBreakBefore w:val="false"/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:pPr>
            <w:r><w:t>Paragraph properties</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("paragraph");
        };

        assert_eq!(p.props.align, Align::Center);
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
        assert!(p.props.page_break_before);
    }

    #[test]
    fn table_gridspan_and_vmerge() {
        // 2x2 grid: row 0 col 0 spans 2 columns (gridSpan) and starts a vertical
        // merge; row 1 col 0 continues it (dropped, owner row_span=2).
        let xml = r#"<w:document><w:body><w:tbl>
            <w:tr>
              <w:tc><w:tcPr><w:gridSpan w:val=" 2 "/><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
              <w:tc><w:tcPr><w:gridSpan w:val=" 2 "/><w:vMerge/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc>
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
    fn table_vmerge_restart_trims_ooxml_value() {
        let xml = r#"<w:document><w:body><w:tbl>
            <w:tr>
              <w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>A</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
              <w:tc><w:tcPr><w:vMerge w:val=" restart "/></w:tcPr><w:p><w:r><w:t>B</w:t></w:r></w:p></w:tc>
            </w:tr>
            <w:tr>
              <w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>
            </w:tr>
        </w:tbl></w:body></w:document>"#;
        let Block::Table(t) = &parse(xml)[0] else {
            panic!("table")
        };

        assert_eq!(t.rows[0].cells[0].row_span, 1);
        assert_eq!(t.rows[1].cells[0].text(), "B");
        assert_eq!(t.rows[1].cells[0].row_span, 2);
        assert_eq!(t.rows[2].cells.len(), 0);
    }

    #[test]
    fn vertical_merge_row_span_saturates_instead_of_overflowing() {
        let mut rows = Vec::with_capacity(u16::MAX as usize + 1);
        rows.push((vec![raw_merge_cell(VMerge::Restart)], false));
        rows.extend(
            (0..u16::MAX as usize).map(|_| (vec![raw_merge_cell(VMerge::Continue)], false)),
        );

        let table = build_table(rows, TableProps::default());

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
    fn content_control_binding_metadata_trims_ooxml_values() {
        let xml = r#"<w:document><w:body>
            <w:sdt><w:sdtPr>
                <w:alias w:val=" Bound alias "/>
                <w:tag w:val=" bound-tag "/>
                <w:dataBinding w:xpath=" /root/client " w:storeItemID=" {11111111-2222-3333-4444-555555555555} "/>
            </w:sdtPr><w:sdtContent>
                <w:p><w:r><w:t>Bound value</w:t></w:r></w:p>
            </w:sdtContent></w:sdt>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("paragraph")
        };
        let control = paragraph.runs[0]
            .content_control
            .as_ref()
            .expect("content control metadata");
        assert_eq!(control.alias.as_deref(), Some("Bound alias"));
        assert_eq!(control.tag.as_deref(), Some("bound-tag"));
        assert_eq!(control.data_binding_xpath.as_deref(), Some("/root/client"));
        assert_eq!(
            control.data_binding_store_item_id.as_deref(),
            Some("{11111111-2222-3333-4444-555555555555}")
        );
    }

    #[test]
    fn content_control_metadata_uses_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sdt><w:sdtPr>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:alias w:val=" Choice alias "/>
                        <w:tag w:val=" choice-tag "/>
                        <w:dataBinding w:xpath=" /root/choice " w:storeItemID=" {11111111-2222-3333-4444-555555555555} "/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:alias w:val=" Fallback alias "/>
                        <w:tag w:val=" fallback-tag "/>
                        <w:dataBinding w:xpath=" /root/fallback " w:storeItemID=" {66666666-7777-8888-9999-AAAAAAAAAAAA} "/>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sdtPr><w:sdtContent>
                <w:p><w:r><w:t>Controlled value</w:t></w:r></w:p>
            </w:sdtContent></w:sdt>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("paragraph")
        };
        let control = paragraph.runs[0]
            .content_control
            .as_ref()
            .expect("content control metadata");

        assert_eq!(control.alias.as_deref(), Some("Choice alias"));
        assert_eq!(control.tag.as_deref(), Some("choice-tag"));
        assert_eq!(control.data_binding_xpath.as_deref(), Some("/root/choice"));
        assert_eq!(
            control.data_binding_store_item_id.as_deref(),
            Some("{11111111-2222-3333-4444-555555555555}")
        );
    }

    #[test]
    fn block_content_control_uses_single_alternate_content_child_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:sdt>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:sdtPr>
                            <w:alias w:val=" Choice block "/>
                            <w:tag w:val="choice-block"/>
                        </w:sdtPr>
                        <w:sdtContent>
                            <w:p><w:r><w:t>Choice block</w:t></w:r></w:p>
                        </w:sdtContent>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:sdtPr>
                            <w:alias w:val="Fallback block"/>
                            <w:tag w:val="fallback-block"/>
                        </w:sdtPr>
                        <w:sdtContent>
                            <w:p><w:r><w:t>Fallback block</w:t></w:r></w:p>
                        </w:sdtContent>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sdt>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("paragraph")
        };
        assert_eq!(paragraph.text(), "Choice block");
        let control = paragraph.runs[0]
            .content_control
            .as_ref()
            .expect("content control metadata");

        assert_eq!(control.alias.as_deref(), Some("Choice block"));
        assert_eq!(control.tag.as_deref(), Some("choice-block"));
    }

    #[test]
    fn run_content_control_uses_single_alternate_content_child_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:p><w:sdt>
                <mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:sdtPr>
                            <w:alias w:val=" Choice run "/>
                            <w:tag w:val="choice-run"/>
                        </w:sdtPr>
                        <w:sdtContent>
                            <w:r><w:t>Choice run</w:t></w:r>
                        </w:sdtContent>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:sdtPr>
                            <w:alias w:val="Fallback run"/>
                            <w:tag w:val="fallback-run"/>
                        </w:sdtPr>
                        <w:sdtContent>
                            <w:r><w:t>Fallback run</w:t></w:r>
                        </w:sdtContent>
                    </mc:Fallback>
                </mc:AlternateContent>
            </w:sdt></w:p>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("paragraph")
        };
        assert_eq!(paragraph.text(), "Choice run");
        let control = paragraph.runs[0]
            .content_control
            .as_ref()
            .expect("content control metadata");

        assert_eq!(control.alias.as_deref(), Some("Choice run"));
        assert_eq!(control.tag.as_deref(), Some("choice-run"));
    }

    #[test]
    fn content_control_blank_metadata_is_ignored() {
        let xml = r#"<w:document><w:body>
            <w:sdt><w:sdtPr>
                <w:alias w:val=" "/>
                <w:tag w:val=" "/>
                <w:dataBinding w:xpath=" " w:storeItemID=" "/>
            </w:sdtPr><w:sdtContent>
                <w:p><w:r><w:t>Plain value</w:t></w:r></w:p>
            </w:sdtContent></w:sdt>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Paragraph(paragraph) = &blocks[0] else {
            panic!("paragraph")
        };

        assert!(paragraph.runs[0].content_control.is_none());
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

    #[test]
    fn paragraph_property_change_keeps_visible_current_text() {
        let xml = r#"<w:document><w:body><w:p>
            <w:pPr>
                <w:pPrChange><w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange>
            </w:pPr>
            <w:r><w:t>Property change</w:t></w:r>
        </w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("para")
        };
        assert_eq!(p.text(), "Property change");
    }

    #[test]
    fn run_property_change_keeps_visible_current_text() {
        let xml = r#"<w:document><w:body><w:p><w:r>
            <w:rPr>
                <w:rPrChange><w:rPr><w:b/></w:rPr></w:rPrChange>
            </w:rPr>
            <w:t>Run property change</w:t>
        </w:r></w:p></w:body></w:document>"#;
        let Block::Paragraph(p) = &parse(xml)[0] else {
            panic!("para")
        };
        assert_eq!(p.text(), "Run property change");
    }

    #[test]
    fn table_property_changes_keep_visible_current_cells() {
        let xml = r#"<w:document><w:body><w:tbl>
            <w:tblPr>
                <w:tblPrChange><w:tblPr><w:tblW w:w="0"/></w:tblPr></w:tblPrChange>
            </w:tblPr>
            <w:tr>
                <w:trPr>
                    <w:trPrChange><w:trPr><w:tblHeader/></w:trPr></w:trPrChange>
                </w:trPr>
                <w:tc>
                    <w:tcPr>
                        <w:tcPrChange><w:tcPr><w:vAlign w:val="center"/></w:tcPr></w:tcPrChange>
                    </w:tcPr>
                    <w:p><w:r><w:t>Cell property change</w:t></w:r></w:p>
                </w:tc>
            </w:tr>
        </w:tbl></w:body></w:document>"#;
        let Block::Table(table) = &parse(xml)[0] else {
            panic!("table")
        };
        let Block::Paragraph(p) = &table.rows[0].cells[0].blocks[0] else {
            panic!("cell paragraph")
        };
        assert_eq!(p.text(), "Cell property change");
    }

    #[test]
    fn table_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:tbl>
                <w:tblPr>
                    <mc:AlternateContent>
                        <mc:Choice Requires="w14">
                            <w:tblW w:type="pct" w:w="4000"/>
                            <w:tblLayout w:type="fixed"/>
                            <w:jc w:val="center"/>
                        </mc:Choice>
                        <mc:Fallback>
                            <w:tblW w:type="pct" w:w="7000"/>
                            <w:tblLayout w:type="autofit"/>
                            <w:jc w:val="right"/>
                        </mc:Fallback>
                    </mc:AlternateContent>
                </w:tblPr>
                <w:tr><w:tc><w:p><w:r><w:t>Table properties</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl>
        </w:body></w:document>"#;
        let Block::Table(table) = &parse(xml)[0] else {
            panic!("table")
        };

        assert_eq!(table.width_pct, Some(0.8));
        assert!(table.fixed_layout);
        assert_eq!(table.align, Some(Align::Center));
    }

    #[test]
    fn table_border_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:tbl>
                <w:tblPr>
                    <w:tblBorders>
                        <mc:AlternateContent>
                            <mc:Choice Requires="w14">
                                <w:top w:val="double" w:sz="8" w:color="112233"/>
                                <w:left w:val="double" w:sz="8" w:color="112233"/>
                                <w:bottom w:val="double" w:sz="8" w:color="112233"/>
                                <w:right w:val="double" w:sz="8" w:color="112233"/>
                                <w:insideH w:val="double" w:sz="8" w:color="112233"/>
                                <w:insideV w:val="double" w:sz="8" w:color="112233"/>
                            </mc:Choice>
                            <mc:Fallback>
                                <w:top w:val="dotted" w:sz="12" w:color="445566"/>
                                <w:left w:val="dotted" w:sz="12" w:color="445566"/>
                                <w:bottom w:val="dotted" w:sz="12" w:color="445566"/>
                                <w:right w:val="dotted" w:sz="12" w:color="445566"/>
                                <w:insideH w:val="dotted" w:sz="12" w:color="445566"/>
                                <w:insideV w:val="dotted" w:sz="12" w:color="445566"/>
                            </mc:Fallback>
                        </mc:AlternateContent>
                    </w:tblBorders>
                </w:tblPr>
                <w:tr><w:tc><w:p><w:r><w:t>Table borders</w:t></w:r></w:p></w:tc></w:tr>
            </w:tbl>
        </w:body></w:document>"#;
        let Block::Table(table) = &parse(xml)[0] else {
            panic!("table")
        };

        let choice_color = Color {
            r: 0x11,
            g: 0x22,
            b: 0x33,
        };
        assert_eq!(table.border_color, Some(choice_color));
        assert_eq!(table.border_colors.top, Some(choice_color));
        assert_eq!(table.border_size_eighths, Some(8));
        assert_eq!(table.border_sizes.top, Some(8));
        assert_eq!(table.border_style, Some(TableBorderStyle::Double));
        assert_eq!(table.border_styles.top, Some(TableBorderStyle::Double));
    }

    #[test]
    fn table_cell_margins_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:tbl>
                <w:tr>
                    <w:tc>
                        <w:tcPr>
                            <w:tcMar>
                                <mc:AlternateContent>
                                    <mc:Choice Requires="w14">
                                        <w:top w:w="120" w:type="dxa"/>
                                        <w:right w:w="240" w:type="dxa"/>
                                        <w:bottom w:w="360" w:type="dxa"/>
                                        <w:left w:w="480" w:type="dxa"/>
                                    </mc:Choice>
                                    <mc:Fallback>
                                        <w:top w:w="720" w:type="dxa"/>
                                        <w:right w:w="840" w:type="dxa"/>
                                        <w:bottom w:w="960" w:type="dxa"/>
                                        <w:left w:w="1080" w:type="dxa"/>
                                    </mc:Fallback>
                                </mc:AlternateContent>
                            </w:tcMar>
                        </w:tcPr>
                        <w:p><w:r><w:t>Margin cell</w:t></w:r></w:p>
                    </w:tc>
                </w:tr>
            </w:tbl>
        </w:body></w:document>"#;
        let Block::Table(table) = &parse(xml)[0] else {
            panic!("table")
        };

        assert_eq!(
            table.rows[0].cells[0].margins,
            Some(CellMargins {
                top: 120,
                right: 240,
                bottom: 360,
                left: 480,
            })
        );
    }

    #[test]
    fn table_row_props_use_single_alternate_content_branch() {
        let xml = r#"<w:document xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>
            <w:tbl>
                <w:tr>
                    <w:trPr>
                        <mc:AlternateContent>
                            <mc:Choice Requires="w14"><w:cantSplit/></mc:Choice>
                            <mc:Fallback><w:tblHeader/></mc:Fallback>
                        </mc:AlternateContent>
                    </w:trPr>
                    <w:tc><w:p><w:r><w:t>Body row</w:t></w:r></w:p></w:tc>
                </w:tr>
            </w:tbl>
            <w:tbl>
                <w:tr>
                    <w:trPr>
                        <mc:AlternateContent>
                            <mc:Choice Requires="w14"><w:tblHeader/></mc:Choice>
                            <mc:Fallback><w:cantSplit/></mc:Fallback>
                        </mc:AlternateContent>
                    </w:trPr>
                    <w:tc><w:p><w:r><w:t>Header row</w:t></w:r></w:p></w:tc>
                </w:tr>
            </w:tbl>
        </w:body></w:document>"#;
        let blocks = parse(xml);
        let Block::Table(body_table) = &blocks[0] else {
            panic!("first table")
        };
        let Block::Table(header_table) = &blocks[1] else {
            panic!("second table")
        };

        assert_eq!(body_table.header_rows, 0);
        assert!(!body_table.rows[0].cells[0].is_header);
        assert_eq!(header_table.header_rows, 1);
        assert!(header_table.rows[0].cells[0].is_header);
    }
}
