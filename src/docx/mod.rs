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

use std::collections::{BTreeMap, HashMap};
use std::io::Read;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use crate::annotation::{
    Comment, Field, FloatingShape, HeaderFooter, HeaderFooterKind, Note, NoteKind, Revision,
    ShapeDistance, ShapeEffectExtent, ShapeExtent, ShapePoint, ShapePosition, ShapeWrapping,
    TextAnchor, TextBox,
};
use crate::assemble;
use crate::error::{Error, Result};
use crate::model::{Block, Color, CustomXmlItem, DocMeta, DocModel, Image};
use crate::text;
use crate::CoreProperties;

mod body;
mod comments;
mod fields;
mod numbering;
mod revisions;
mod styles;

pub(crate) fn parse_fields(xml: &str) -> Vec<Field> {
    let core_properties = CoreProperties::default();
    let custom_properties = HashMap::new();
    let document_variables = HashMap::new();
    let extended_properties = HashMap::new();
    fields::parse(
        xml,
        &styles::Styles::default(),
        &[],
        &numbering::Numbering::default(),
        fields::FieldDocumentProperties {
            core: &core_properties,
            custom: &custom_properties,
            variables: &document_variables,
            extended: &extended_properties,
            file_size_bytes: None,
        },
    )
}

pub(crate) fn supports_display_field_syntax(instruction: &str) -> bool {
    fields::computed_display_result(instruction).is_some()
}

pub(crate) fn supports_action_field_syntax(instruction: &str) -> bool {
    fields::computed_action_result(instruction).is_some()
}

pub(crate) fn supports_reference_index_marker_syntax(instruction: &str) -> bool {
    fields::computed_reference_index_result(instruction).is_some()
}

pub(crate) fn supports_numbering_field_syntax(instruction: &str) -> bool {
    fields::supports_numbering_field_syntax(instruction)
}

pub(crate) fn supports_compare_field_syntax(instruction: &str) -> bool {
    fields::supports_compare_field_syntax(instruction)
}

pub(crate) fn supports_quote_field_syntax(instruction: &str) -> bool {
    fields::supports_quote_field_syntax(instruction)
}

pub(crate) fn supports_prompt_field_syntax(instruction: &str) -> bool {
    fields::supports_prompt_field_syntax(instruction)
}

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
    /// Footnote/endnote side-table records parsed from `word/footnotes.xml` and
    /// `word/endnotes.xml`.
    pub note_records: Vec<Note>,
    /// Text-box side-table records parsed from body `w:txbxContent` shapes.
    pub text_boxes: Vec<TextBox>,
    /// Floating shape geometry parsed from body `wp:anchor` drawing markup.
    pub floating_shapes: Vec<FloatingShape>,
    /// Exact running header/footer records parsed from referenced `.docx` parts.
    pub header_footers: Vec<HeaderFooter>,
    /// Core metadata parsed from `docProps/core.xml`.
    pub core_properties: CoreProperties,
    /// Full flat text: body, then footnotes/endnotes, then headers and footers.
    pub text: String,
    /// Just the main body (excludes notes and headers/footers).
    pub main_text: String,
    /// The retained OPC package (every part verbatim) — the source of truth for
    /// package-preserving `save()`. Element-tree edits mutate its `document.xml` in
    /// place; the lossy `model` above is the read/render view.
    pub package: crate::opc::Package,
    /// Comments parsed from `word/comments.xml` and optional commentsExtended links.
    pub comments: Vec<Comment>,
    /// Fields parsed from `word/document.xml`.
    pub fields: Vec<Field>,
    /// Tracked revisions parsed from `word/document.xml`.
    pub revisions: Vec<Revision>,
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
    let core_properties = part(&mut zip, "docProps/core.xml")
        .map(|s| parse_core_properties(&s))
        .unwrap_or_default();
    let custom_properties = part(&mut zip, "docProps/custom.xml")
        .map(|s| parse_custom_properties(&s))
        .unwrap_or_default();
    let custom_property_fields = custom_properties
        .iter()
        .map(|(key, value)| (fields::document_property_key(key), value.clone()))
        .collect::<HashMap<_, _>>();
    let custom_xml_items = read_custom_xml_items(&mut zip);
    let extended_properties = part(&mut zip, "docProps/app.xml")
        .map(|s| parse_extended_properties(&s))
        .unwrap_or_default();
    let settings_xml = part(&mut zip, "word/settings.xml");
    let document_variables = settings_xml
        .as_deref()
        .map(parse_document_variables)
        .unwrap_or_default();
    let document_id = settings_xml.as_deref().and_then(parse_document_id);
    let ref_targets = fields::ref_targets(&doc_xml);
    let ref_position_context = fields::ref_position_context(&doc_xml, &numbering);
    let ref_number_context = fields::ref_number_context(&doc_xml, &numbering);
    let page_ref_context = fields::page_ref_context(&doc_xml);
    let note_ref_context = fields::note_ref_context(&doc_xml);
    let section_context = fields::section_context(&doc_xml);
    let style_ref_context = fields::style_ref_context(&doc_xml, &styles, &numbering);
    let legacy_form_context = fields::legacy_form_context(&doc_xml);
    let table_formula_context = fields::table_formula_context(&doc_xml);
    let toc_entries = fields::toc_entries(&doc_xml, &styles);
    let document_properties = DocumentPropertyRefs {
        core: &core_properties,
        custom: &custom_property_fields,
        variables: &document_variables,
        extended: &extended_properties,
        file_size_bytes: Some(bytes.len()),
    };

    let ctx = body::Ctx {
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
        custom_properties: &custom_property_fields,
        document_variables: &document_variables,
        extended_properties: &extended_properties,
        file_size_bytes: Some(bytes.len()),
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
    let mut blocks = body::parse_document(&doc_xml, &ctx); // body only
                                                           // Footnotes/endnotes live in their own parts. Keep them SEPARATE from the body
                                                           // (not appended into `model.blocks`); their parts are preserved verbatim on save.
                                                           // They are re-joined for the read/text views below and in `Document::model()`.
    let (mut notes, mut note_records) = read_notes(
        &mut zip,
        "word/footnotes.xml",
        b"footnote",
        NoteKind::Footnote,
        &styles,
        &numbering,
        document_properties,
    );
    let (endnote_blocks, mut endnote_records) = read_notes(
        &mut zip,
        "word/endnotes.xml",
        b"endnote",
        NoteKind::Endnote,
        &styles,
        &numbering,
        document_properties,
    );
    notes.extend(endnote_blocks);
    note_records.append(&mut endnote_records);
    attach_note_reference_anchors(&mut note_records, &doc_xml);
    let floating_shapes = read_floating_shapes(&doc_xml);
    let text_boxes = read_text_boxes(&doc_xml, &ctx, &floating_shapes);
    // Running headers/footers referenced by the body's sectPr(s). `ctx` only holds
    // shared (&) borrows of rels/styles/numbering, so the &mut zip pass is fine.
    let (section_header_footers, final_header_footer, header_footers) = read_headers_footers(
        &mut zip,
        &doc_xml,
        &rels,
        &styles,
        &numbering,
        document_properties,
    );
    apply_section_header_footers(&mut blocks, &section_header_footers);
    let comments_xml = part(&mut zip, "word/comments.xml");
    let comments_ext_xml = part(&mut zip, "word/commentsExtended.xml");
    let mut comments = comments_xml
        .as_deref()
        .map(comments::parse)
        .unwrap_or_default();
    if let (Some(comments_xml), Some(comments_ext_xml)) =
        (comments_xml.as_deref(), comments_ext_xml.as_deref())
    {
        comments::apply_extended_parent_ids(&mut comments, comments_xml, comments_ext_xml);
    }
    let comment_anchors = comments::parse_anchors(&doc_xml);
    for comment in &mut comments {
        comment.anchor = comment_anchors.get(&comment.id).cloned();
    }
    let fields = fields::parse(
        &doc_xml,
        &styles,
        &toc_entries,
        &numbering,
        fields::FieldDocumentProperties {
            core: &core_properties,
            custom: &custom_property_fields,
            variables: &document_variables,
            extended: &extended_properties,
            file_size_bytes: Some(bytes.len()),
        },
    );
    let revisions = revisions::parse(&doc_xml);
    // Stats reflect the full visible content (body + notes).
    let stats = {
        let mut all = blocks.clone();
        all.extend(notes.iter().cloned());
        assemble::compute_stats(&all)
    };
    let model = DocModel {
        blocks, // body only
        regions: Vec::new(),
        // `.docx` text is Unicode (no ANSI codepage); these fields are not
        // meaningful here, unlike the `.doc` path's `lid`/codepage.
        meta: DocMeta {
            codepage: 0,
            lid: 0,
            stats,
        },
        custom_properties,
        custom_xml_items,
        setup: crate::model::DocSetup {
            page: body::scan_page_setup(&doc_xml),
            header: final_header_footer.header,
            first_header: final_header_footer.first_header,
            even_header: final_header_footer.even_header,
            footer: final_header_footer.footer,
            first_footer: final_header_footer.first_footer,
            even_footer: final_header_footer.even_footer,
            page_number_start: body::scan_page_number_start(&doc_xml),
            page_number_format: body::scan_page_number_format(&doc_xml),
            columns: body::scan_section_columns(&doc_xml),
            text_direction: body::scan_section_text_direction(&doc_xml),
            doc_grid: body::scan_section_doc_grid(&doc_xml),
            document_id,
            title_page: body::scan_section_title_page(&doc_xml),
            title: core_properties.title.clone(),
            creator: core_properties.creator.clone(),
            ..crate::model::DocSetup::default()
        },
    };
    let main_text = body_text(&model); // body only
                                       // Full text: body, then notes, then section/final headers/footers.
    let text = {
        let mut raw = String::new();
        flatten(&model.blocks, &mut raw);
        flatten(&notes, &mut raw);
        flatten_header_footer_surfaces(&model, &mut raw);
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
        comments,
        note_records,
        text_boxes,
        floating_shapes,
        header_footers,
        core_properties,
        fields,
        revisions,
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
#[derive(Clone, Default)]
struct SectionHeaderFooter {
    header: Vec<Block>,
    first_header: Vec<Block>,
    even_header: Vec<Block>,
    footer: Vec<Block>,
    first_footer: Vec<Block>,
    even_footer: Vec<Block>,
}

#[derive(Default)]
struct HeaderFooterBlocks {
    default: Vec<Block>,
    first: Vec<Block>,
    even: Vec<Block>,
}

#[derive(Clone, Copy)]
struct DocumentPropertyRefs<'a> {
    core: &'a CoreProperties,
    custom: &'a HashMap<String, String>,
    variables: &'a HashMap<String, String>,
    extended: &'a HashMap<String, String>,
    file_size_bytes: Option<usize>,
}

fn read_headers_footers(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    doc_xml: &str,
    rels: &Rels,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
    properties: DocumentPropertyRefs<'_>,
) -> (
    Vec<SectionHeaderFooter>,
    SectionHeaderFooter,
    Vec<HeaderFooter>,
) {
    let section_refs = body::scan_hf_ref_sections(doc_xml);
    let mut sections = Vec::with_capacity(section_refs.len());
    let mut records = Vec::new();
    let mut seen_records = std::collections::HashSet::new();
    let mut inherited_header = Vec::new();
    let mut inherited_footer = Vec::new();

    for refs in section_refs {
        let header_has_default = has_default_header_footer_ref(&refs.headers);
        let footer_has_default = has_default_header_footer_ref(&refs.footers);
        let (header_blocks, header_records) = read_hf_parts(
            zip,
            &refs.headers,
            HeaderFooterPartKind::Header,
            rels,
            styles,
            numbering,
            properties,
        );
        extend_unique_header_footer_records(&mut records, &mut seen_records, header_records);
        let mut header = header_blocks.default;
        // Omitted odd/default refs inherit the previous section; an explicit
        // default ref, even when blank/unresolved, resets the inherited surface.
        if !header_has_default && !inherited_header.is_empty() {
            header = inherited_header.clone();
        }
        if header_has_default || !header.is_empty() {
            inherited_header = header.clone();
        }

        let (footer_blocks, footer_records) = read_hf_parts(
            zip,
            &refs.footers,
            HeaderFooterPartKind::Footer,
            rels,
            styles,
            numbering,
            properties,
        );
        extend_unique_header_footer_records(&mut records, &mut seen_records, footer_records);
        let mut footer = footer_blocks.default;
        // Same inheritance rule as headers.
        if !footer_has_default && !inherited_footer.is_empty() {
            footer = inherited_footer.clone();
        }
        if footer_has_default || !footer.is_empty() {
            inherited_footer = footer.clone();
        }
        sections.push(SectionHeaderFooter {
            header,
            first_header: header_blocks.first,
            even_header: header_blocks.even,
            footer,
            first_footer: footer_blocks.first,
            even_footer: footer_blocks.even,
        });
    }

    let final_section = sections.last().cloned().unwrap_or_default();
    (sections, final_section, records)
}

fn extend_unique_header_footer_records(
    records: &mut Vec<HeaderFooter>,
    seen: &mut std::collections::HashSet<String>,
    next: Vec<HeaderFooter>,
) {
    for record in next {
        if seen.insert(record.id.clone()) {
            records.push(record);
        }
    }
}

fn apply_section_header_footers(blocks: &mut [Block], sections: &[SectionHeaderFooter]) {
    if sections.is_empty() {
        return;
    }
    let section_break_count = blocks
        .iter()
        .filter(|block| matches!(block, Block::SectionBreak(_)))
        .count();
    let section_count = if sections.len() > section_break_count {
        section_break_count
    } else {
        sections.len()
    };
    let mut section_iter = sections[..section_count].iter();
    for block in blocks {
        if let Block::SectionBreak(setup) = block {
            let Some(section) = section_iter.next() else {
                break;
            };
            setup.header = section.header.clone();
            setup.first_header = section.first_header.clone();
            setup.even_header = section.even_header.clone();
            setup.footer = section.footer.clone();
            setup.first_footer = section.first_footer.clone();
            setup.even_footer = section.even_footer.clone();
        }
    }
}

fn has_default_header_footer_ref(refs: &[body::HeaderFooterRef]) -> bool {
    refs.iter()
        .any(|reference| normalized_header_footer_type(&reference.type_name) == "default")
}

/// Read each unique referenced header/footer part once (dedup by part name), with
/// its own `_rels`/media so links and images inside the part resolve correctly.
#[derive(Clone, Copy)]
enum HeaderFooterPartKind {
    Header,
    Footer,
}

fn read_hf_parts(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    refs: &[body::HeaderFooterRef],
    part_kind: HeaderFooterPartKind,
    rels: &Rels,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
    properties: DocumentPropertyRefs<'_>,
) -> (HeaderFooterBlocks, Vec<HeaderFooter>) {
    let mut seen_blocks = std::collections::HashSet::new();
    let mut seen_records = std::collections::HashSet::new();
    let mut blocks = HeaderFooterBlocks::default();
    let mut records = Vec::new();
    for reference in refs {
        let Some((target, external)) = rels.get(&reference.rel_id) else {
            continue;
        };
        if *external {
            continue;
        }
        let path = normalize_part(target);
        let part_rels = part(zip, &part_rels_path(&path))
            .map(|s| parse_rels(&s))
            .unwrap_or_default();
        let part_media = read_media(zip, &part_rels);
        let ref_targets = HashMap::new();
        let ref_position_context = fields::RefPositionContext::default();
        let ref_number_context = fields::RefNumberContext::empty();
        let page_ref_context = fields::PageRefContext::empty();
        let note_ref_context = fields::NoteRefContext::empty();
        let section_context = fields::SectionContext::empty();
        let style_ref_context = fields::StyleRefContext::empty();
        let legacy_form_context = fields::LegacyFormContext::empty();
        let table_formula_context = fields::TableFormulaContext::empty();
        let toc_entries = Vec::new();
        let hf_ctx = body::Ctx {
            styles,
            numbering,
            rels: &part_rels,
            media: &part_media,
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
            core_properties: properties.core,
            custom_properties: properties.custom,
            document_variables: properties.variables,
            extended_properties: properties.extended,
            file_size_bytes: properties.file_size_bytes,
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
        if let Some(xml) = part(zip, &path) {
            let part_blocks = body::parse_hdrftr(&xml, &hf_ctx);
            let type_name = normalized_header_footer_type(&reference.type_name);
            if seen_blocks.insert((path.clone(), type_name.to_string())) {
                match type_name {
                    "first" => blocks.first.extend(part_blocks.clone()),
                    "even" => blocks.even.extend(part_blocks.clone()),
                    _ => blocks.default.extend(part_blocks.clone()),
                }
            }
            if seen_records.insert((path.clone(), type_name.to_string())) {
                let text = blocks_text(&part_blocks);
                if !text.is_empty() {
                    records.push(HeaderFooter {
                        id: format!("{path}#{type_name}"),
                        kind: header_footer_kind(part_kind, type_name),
                        text,
                    });
                }
            }
        }
    }
    (blocks, records)
}

fn normalized_header_footer_type(value: &str) -> &'static str {
    match value {
        "first" => "first",
        "even" => "even",
        _ => "default",
    }
}

fn header_footer_kind(part_kind: HeaderFooterPartKind, type_name: &str) -> HeaderFooterKind {
    match (part_kind, type_name) {
        (HeaderFooterPartKind::Header, "first") => HeaderFooterKind::FirstPageHeader,
        (HeaderFooterPartKind::Header, "even") => HeaderFooterKind::EvenPageHeader,
        (HeaderFooterPartKind::Header, _) => HeaderFooterKind::Header,
        (HeaderFooterPartKind::Footer, "first") => HeaderFooterKind::FirstPageFooter,
        (HeaderFooterPartKind::Footer, "even") => HeaderFooterKind::EvenPageFooter,
        (HeaderFooterPartKind::Footer, _) => HeaderFooterKind::Footer,
    }
}

/// Read a footnotes/endnotes part (if present) into its real notes' blocks, with
/// the part's own rels/media so links and images inside notes resolve.
fn read_notes(
    zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>,
    name: &str,
    tag: &[u8],
    kind: NoteKind,
    styles: &styles::Styles,
    numbering: &numbering::Numbering,
    properties: DocumentPropertyRefs<'_>,
) -> (Vec<Block>, Vec<Note>) {
    let Some(xml) = part(zip, name) else {
        return (Vec::new(), Vec::new());
    };
    let part_rels = part(zip, &part_rels_path(name))
        .map(|s| parse_rels(&s))
        .unwrap_or_default();
    let part_media = read_media(zip, &part_rels);
    let ref_targets = HashMap::new();
    let ref_position_context = fields::RefPositionContext::default();
    let ref_number_context = fields::RefNumberContext::empty();
    let page_ref_context = fields::PageRefContext::empty();
    let note_ref_context = fields::NoteRefContext::empty();
    let section_context = fields::SectionContext::empty();
    let style_ref_context = fields::StyleRefContext::empty();
    let legacy_form_context = fields::LegacyFormContext::empty();
    let table_formula_context = fields::TableFormulaContext::empty();
    let toc_entries = Vec::new();
    let ctx = body::Ctx {
        styles,
        numbering,
        rels: &part_rels,
        media: &part_media,
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
        core_properties: properties.core,
        custom_properties: properties.custom,
        document_variables: properties.variables,
        extended_properties: properties.extended,
        file_size_bytes: properties.file_size_bytes,
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
    let mut blocks = Vec::new();
    let mut records = Vec::new();
    for (id, note_blocks) in body::parse_note_entries(&xml, &ctx, tag) {
        let text = blocks_text(&note_blocks);
        records.push(Note {
            id,
            kind,
            text,
            anchor: None,
        });
        blocks.extend(note_blocks);
    }
    (blocks, records)
}

fn read_text_boxes(
    doc_xml: &str,
    ctx: &body::Ctx<'_>,
    floating_shapes: &[FloatingShape],
) -> Vec<TextBox> {
    let text_boxes: Vec<_> = body::parse_text_boxes(doc_xml, ctx)
        .into_iter()
        .enumerate()
        .filter(|(_, text)| !text.is_empty())
        .collect();
    let ordered_anchors = ordered_text_box_anchors(&text_boxes, floating_shapes);
    text_boxes
        .into_iter()
        .enumerate()
        .map(|(text_box_index, (index, text))| TextBox {
            id: format!("docx-text-box-{index}"),
            anchor: ordered_anchors
                .get(text_box_index)
                .and_then(|anchor| anchor.clone())
                .or_else(|| text_box_anchor(&text, floating_shapes)),
            text,
        })
        .collect()
}

fn ordered_text_box_anchors(
    text_boxes: &[(usize, String)],
    floating_shapes: &[FloatingShape],
) -> Vec<Option<TextAnchor>> {
    let text_box_shapes: Vec<_> = floating_shapes
        .iter()
        .filter(|shape| shape.text.is_some() && shape.anchor_text.is_some())
        .collect();
    if text_boxes.len() != text_box_shapes.len()
        || !text_boxes
            .iter()
            .map(|(_, text)| text.as_str())
            .zip(&text_box_shapes)
            .all(|(text, shape)| shape.text.as_deref() == Some(text))
    {
        return vec![None; text_boxes.len()];
    }
    text_box_shapes
        .into_iter()
        .map(text_anchor_from_shape)
        .collect()
}

fn text_box_anchor(text: &str, floating_shapes: &[FloatingShape]) -> Option<TextAnchor> {
    let mut matches = floating_shapes.iter().filter(|shape| {
        shape.text.as_deref() == Some(text) && shape.anchor_text.as_deref().is_some()
    });
    let shape = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    text_anchor_from_shape(shape)
}

fn text_anchor_from_shape(shape: &FloatingShape) -> Option<TextAnchor> {
    Some(TextAnchor {
        id: shape.id.clone(),
        text: shape.anchor_text.clone()?,
    })
}

fn read_floating_shapes(doc_xml: &str) -> Vec<FloatingShape> {
    let mut r = Reader::from_str(doc_xml);
    let mut shapes = Vec::new();
    let mut in_body = false;
    let mut body_depth = 0usize;
    let mut body_block_candidate_depths = vec![0usize];
    let mut next_body_block_index = 0usize;
    let mut current_body_block_index = None;
    let mut current_body_block_depth = None;
    let mut current_body_block_text = String::new();
    let mut current_body_block_shapes = Vec::new();
    let mut alternate_content_stack = Vec::new();
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
                    current_body_block_index = None;
                    current_body_block_depth = None;
                    current_body_block_text.clear();
                    current_body_block_shapes.clear();
                    alternate_content_stack.clear();
                    continue;
                }
                if in_body {
                    if should_skip_redundant_alternate_branch(
                        &mut alternate_content_stack,
                        body_depth,
                        name,
                    ) {
                        skip_shape_scan_subtree(&mut r);
                        continue;
                    }
                    if is_old_revision_content(name) {
                        skip_shape_scan_subtree(&mut r);
                        continue;
                    }
                    if name == b"AlternateContent" {
                        alternate_content_stack.push(AlternateContentState {
                            branch_depth: body_depth + 1,
                            took_branch: false,
                        });
                    }
                    if current_body_block_index.is_none()
                        && body_block_candidate_depths.contains(&body_depth)
                        && is_transparent_body_block_container(name)
                    {
                        body_block_candidate_depths.push(body_depth + 1);
                    }
                    if current_body_block_index.is_none()
                        && body_block_candidate_depths.contains(&body_depth)
                        && is_body_block(name)
                    {
                        current_body_block_index = Some(next_body_block_index);
                        current_body_block_depth = Some(body_depth + 1);
                        current_body_block_text.clear();
                        current_body_block_shapes.clear();
                        next_body_block_index += 1;
                    }
                    body_depth += 1;
                }
                if name == b"anchor" {
                    let index = shapes.len();
                    shapes.push(read_floating_shape(
                        &mut r,
                        &e,
                        index,
                        current_body_block_index,
                    ));
                    if current_body_block_index.is_some() {
                        current_body_block_shapes.push(FloatingShapeAnchorCandidate {
                            shape_index: index,
                            raw_prefix: current_body_block_text.clone(),
                        });
                    }
                    if in_body {
                        body_depth = body_depth.saturating_sub(1);
                    }
                    continue;
                }
                if in_body && current_body_block_index.is_some() && name == b"t" {
                    append_floating_anchor_text(
                        &mut current_body_block_text,
                        &read_leaf_text(&mut r),
                    );
                    body_depth = body_depth.saturating_sub(1);
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if in_body
                    && current_body_block_index.is_none()
                    && body_block_candidate_depths.contains(&body_depth)
                    && is_body_block(name)
                {
                    next_body_block_index += 1;
                }
                if name == b"anchor" {
                    let index = shapes.len();
                    shapes.push(floating_shape_shell(index, &e, current_body_block_index));
                    if current_body_block_index.is_some() {
                        current_body_block_shapes.push(FloatingShapeAnchorCandidate {
                            shape_index: index,
                            raw_prefix: current_body_block_text.clone(),
                        });
                    }
                } else if in_body && current_body_block_index.is_some() {
                    append_floating_anchor_empty_marker(&mut current_body_block_text, name);
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
                    current_body_block_index = None;
                    current_body_block_depth = None;
                    current_body_block_text.clear();
                    current_body_block_shapes.clear();
                    alternate_content_stack.clear();
                    continue;
                }
                if in_body {
                    let ending_current_body_block = current_body_block_depth == Some(body_depth);
                    if ending_current_body_block {
                        apply_floating_anchor_text_with_offsets(
                            &mut shapes,
                            &current_body_block_shapes,
                            &current_body_block_text,
                        );
                    }
                    if body_block_candidate_depths.last().copied() == Some(body_depth) {
                        body_block_candidate_depths.pop();
                    }
                    if name == b"AlternateContent"
                        && alternate_content_stack
                            .last()
                            .is_some_and(|state| state.branch_depth == body_depth)
                    {
                        alternate_content_stack.pop();
                    }
                    body_depth = body_depth.saturating_sub(1);
                    if ending_current_body_block || body_depth == 0 {
                        current_body_block_index = None;
                        current_body_block_depth = None;
                        current_body_block_text.clear();
                        current_body_block_shapes.clear();
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    shapes
}

fn append_floating_anchor_text(out: &mut String, text: &str) {
    out.push_str(text);
}

fn append_floating_anchor_empty_marker(out: &mut String, name: &[u8]) {
    match name {
        b"tab" => out.push('\t'),
        b"br" | b"cr" => out.push('\n'),
        b"noBreakHyphen" => out.push('-'),
        _ => {}
    }
}

#[derive(Debug, Clone)]
struct FloatingShapeAnchorCandidate {
    shape_index: usize,
    raw_prefix: String,
}

#[derive(Debug, Clone, Copy)]
struct AlternateContentState {
    branch_depth: usize,
    took_branch: bool,
}

fn should_skip_redundant_alternate_branch(
    stack: &mut [AlternateContentState],
    body_depth: usize,
    name: &[u8],
) -> bool {
    if !matches!(name, b"Choice" | b"Fallback") {
        return false;
    }
    let Some(state) = stack.last_mut() else {
        return false;
    };
    if state.branch_depth != body_depth {
        return false;
    }
    if state.took_branch {
        true
    } else {
        state.took_branch = true;
        false
    }
}

fn apply_floating_anchor_text_with_offsets(
    shapes: &mut [FloatingShape],
    shape_indices: &[FloatingShapeAnchorCandidate],
    raw: &str,
) {
    if shape_indices.is_empty() {
        return;
    }
    let text = text::finalize(raw);
    if text.is_empty() {
        return;
    }
    for index in shape_indices {
        if let Some(shape) = shapes.get_mut(index.shape_index) {
            shape.anchor_text = Some(text.clone());
            shape.anchor_char_offset = normalized_anchor_char_offset(raw, &index.raw_prefix);
        }
    }
}

fn normalized_anchor_char_offset(raw: &str, raw_prefix: &str) -> Option<usize> {
    let suffix = raw.get(raw_prefix.len()..)?;
    const MARKER: char = '\u{E000}';
    if raw.contains(MARKER) {
        return None;
    }
    let mut marked = String::with_capacity(raw.len() + MARKER.len_utf8());
    marked.push_str(raw_prefix);
    marked.push(MARKER);
    marked.push_str(suffix);
    let normalized = text::finalize(&marked);
    let marker_byte = normalized.find(MARKER)?;
    Some(normalized[..marker_byte].chars().count())
}

fn read_floating_shape(
    r: &mut Reader<&[u8]>,
    start: &BytesStart<'_>,
    index: usize,
    anchor_block_index: Option<usize>,
) -> FloatingShape {
    let mut shape = floating_shape_shell(index, start, anchor_block_index);
    let mut text_box_depth = 0usize;
    let mut shape_text = String::new();
    let mut outline_depth = 0usize;
    let mut solid_fill = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if text_box_depth > 0 {
                    match name {
                        b"t" => append_shape_text(&mut shape_text, &read_leaf_text(r)),
                        _ => text_box_depth += 1,
                    }
                    continue;
                }
                enter_shape_color_context(name, &mut outline_depth, &mut solid_fill);
                match name {
                    b"positionH" => shape.horizontal_position = Some(read_shape_position(r, &e)),
                    b"positionV" => shape.vertical_position = Some(read_shape_position(r, &e)),
                    b"simplePos" => shape.simple_position = shape_point(&e),
                    b"extent" => shape.extent = shape_extent(&e),
                    b"effectExtent" => shape.effect_extent = shape_effect_extent(&e),
                    b"docPr" => apply_shape_doc_pr(&mut shape, &e),
                    b"prstGeom" => apply_shape_preset_geometry(&mut shape, &e),
                    b"srgbClr" => apply_shape_srgb_color(&mut shape, &e, solid_fill),
                    b"txbxContent" => text_box_depth = 1,
                    b"wrapNone" | b"wrapSquare" | b"wrapTight" | b"wrapThrough"
                    | b"wrapTopAndBottom" => {
                        shape.wrapping = Some(shape_wrapping(&e));
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if text_box_depth > 0 {
                    match name {
                        b"tab" => shape_text.push('\t'),
                        b"br" | b"cr" => shape_text.push('\n'),
                        _ => {}
                    }
                    continue;
                }
                if name == b"srgbClr" {
                    apply_shape_srgb_color(&mut shape, &e, solid_fill);
                }
                match name {
                    b"positionH" => shape.horizontal_position = Some(empty_shape_position(&e)),
                    b"positionV" => shape.vertical_position = Some(empty_shape_position(&e)),
                    b"simplePos" => shape.simple_position = shape_point(&e),
                    b"extent" => shape.extent = shape_extent(&e),
                    b"effectExtent" => shape.effect_extent = shape_effect_extent(&e),
                    b"docPr" => apply_shape_doc_pr(&mut shape, &e),
                    b"prstGeom" => apply_shape_preset_geometry(&mut shape, &e),
                    b"wrapNone" | b"wrapSquare" | b"wrapTight" | b"wrapThrough"
                    | b"wrapTopAndBottom" => {
                        shape.wrapping = Some(shape_wrapping(&e));
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"anchor" => break,
            Ok(Event::End(e)) if text_box_depth > 0 => {
                if local(e.name().as_ref()) == b"p" {
                    append_shape_paragraph_break(&mut shape_text);
                }
                text_box_depth = text_box_depth.saturating_sub(1);
            }
            Ok(Event::End(_)) => {
                leave_shape_color_context(&mut outline_depth, &mut solid_fill);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    shape.text = finalized_shape_text(shape_text);
    shape
}

fn floating_shape_shell(
    index: usize,
    start: &BytesStart<'_>,
    anchor_block_index: Option<usize>,
) -> FloatingShape {
    FloatingShape {
        id: format!("docx-floating-shape-{index}"),
        name: None,
        description: None,
        text: None,
        preset_geometry: None,
        fill_color: None,
        outline_color: None,
        simple_position_enabled: attr_bool(start, b"simplePos"),
        simple_position: None,
        effect_extent: None,
        anchor_block_index,
        anchor_text: None,
        anchor_char_offset: None,
        extent: None,
        horizontal_position: None,
        vertical_position: None,
        relative_height: attr_i64(start, b"relativeHeight"),
        behind_doc: attr_bool(start, b"behindDoc"),
        layout_in_cell: attr_bool(start, b"layoutInCell"),
        locked: attr_bool(start, b"locked"),
        allow_overlap: attr_bool(start, b"allowOverlap"),
        distance: ShapeDistance {
            top_emu: attr_i64(start, b"distT"),
            bottom_emu: attr_i64(start, b"distB"),
            left_emu: attr_i64(start, b"distL"),
            right_emu: attr_i64(start, b"distR"),
        },
        wrapping: None,
    }
}

fn is_body_block(name: &[u8]) -> bool {
    matches!(name, b"p" | b"tbl")
}

fn is_transparent_body_block_container(name: &[u8]) -> bool {
    matches!(
        name,
        b"sdt" | b"sdtContent" | b"customXml" | b"smartTag" | b"ins" | b"moveTo"
    )
}

fn is_old_revision_content(name: &[u8]) -> bool {
    matches!(name, b"del" | b"moveFrom")
}

fn skip_shape_scan_subtree(r: &mut Reader<&[u8]>) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(_)) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn append_shape_text(out: &mut String, text: &str) {
    let previous_is_space = matches!(out.chars().last(), Some(' ' | '\n' | '\t'));
    let next_is_space = matches!(text.chars().next(), Some(' ' | '\n' | '\t'));
    if !out.is_empty() && !previous_is_space && !next_is_space {
        out.push(' ');
    }
    out.push_str(text);
}

fn append_shape_paragraph_break(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn finalized_shape_text(text: String) -> Option<String> {
    let text = text.trim_matches('\n').to_string();
    (!text.trim().is_empty()).then_some(text)
}

fn empty_shape_position(start: &BytesStart<'_>) -> ShapePosition {
    ShapePosition {
        relative_from: attr_local(start, b"relativeFrom"),
        offset_emu: None,
        align: None,
    }
}

fn read_shape_position(r: &mut Reader<&[u8]>, start: &BytesStart<'_>) -> ShapePosition {
    let mut position = empty_shape_position(start);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"posOffset" => {
                position.offset_emu = read_leaf_text(r).trim().parse().ok();
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"align" => {
                position.align = Some(read_leaf_text(r));
            }
            Ok(Event::End(e))
                if matches!(local(e.name().as_ref()), b"positionH" | b"positionV") =>
            {
                break;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    position
}

fn shape_extent(e: &BytesStart<'_>) -> Option<ShapeExtent> {
    Some(ShapeExtent {
        cx_emu: attr_i64(e, b"cx")?,
        cy_emu: attr_i64(e, b"cy")?,
    })
}

fn shape_point(e: &BytesStart<'_>) -> Option<ShapePoint> {
    Some(ShapePoint {
        x_emu: attr_i64(e, b"x")?,
        y_emu: attr_i64(e, b"y")?,
    })
}

fn shape_effect_extent(e: &BytesStart<'_>) -> Option<ShapeEffectExtent> {
    Some(ShapeEffectExtent {
        left_emu: attr_i64(e, b"l")?,
        top_emu: attr_i64(e, b"t")?,
        right_emu: attr_i64(e, b"r")?,
        bottom_emu: attr_i64(e, b"b")?,
    })
}

fn shape_wrapping(e: &BytesStart<'_>) -> ShapeWrapping {
    let kind = match local(e.name().as_ref()) {
        b"wrapNone" => "none",
        b"wrapSquare" => "square",
        b"wrapTight" => "tight",
        b"wrapThrough" => "through",
        b"wrapTopAndBottom" => "topAndBottom",
        _ => "unknown",
    };
    ShapeWrapping {
        kind: kind.to_string(),
        text: attr_local(e, b"wrapText"),
        distance: ShapeDistance {
            top_emu: attr_i64(e, b"distT"),
            bottom_emu: attr_i64(e, b"distB"),
            left_emu: attr_i64(e, b"distL"),
            right_emu: attr_i64(e, b"distR"),
        },
    }
}

fn apply_shape_doc_pr(shape: &mut FloatingShape, e: &BytesStart<'_>) {
    if let Some(id) = attr_local(e, b"id") {
        shape.id = id;
    }
    shape.name = attr_local(e, b"name");
    shape.description = attr_local(e, b"descr");
}

fn apply_shape_preset_geometry(shape: &mut FloatingShape, e: &BytesStart<'_>) {
    if shape.preset_geometry.is_none() {
        shape.preset_geometry = attr_local(e, b"prst");
    }
}

#[derive(Debug, Clone, Copy)]
enum ShapeColorTarget {
    Fill,
    Outline,
}

fn enter_shape_color_context(
    name: &[u8],
    outline_depth: &mut usize,
    solid_fill: &mut Option<(usize, ShapeColorTarget)>,
) {
    if *outline_depth > 0 || name == b"ln" {
        *outline_depth += 1;
    }
    if name == b"solidFill" {
        let target = if *outline_depth > 0 {
            ShapeColorTarget::Outline
        } else {
            ShapeColorTarget::Fill
        };
        *solid_fill = Some((1, target));
    } else if let Some((depth, _)) = solid_fill.as_mut() {
        *depth += 1;
    }
}

fn leave_shape_color_context(
    outline_depth: &mut usize,
    solid_fill: &mut Option<(usize, ShapeColorTarget)>,
) {
    if let Some((depth, _)) = solid_fill.as_mut() {
        *depth = depth.saturating_sub(1);
        if *depth == 0 {
            *solid_fill = None;
        }
    }
    *outline_depth = outline_depth.saturating_sub(1);
}

fn apply_shape_srgb_color(
    shape: &mut FloatingShape,
    e: &BytesStart<'_>,
    solid_fill: Option<(usize, ShapeColorTarget)>,
) {
    let Some((_, target)) = solid_fill else {
        return;
    };
    let Some(color) = attr_local(e, b"val").and_then(|value| parse_rgb_hex_color(&value)) else {
        return;
    };
    match target {
        ShapeColorTarget::Fill if shape.fill_color.is_none() => shape.fill_color = Some(color),
        ShapeColorTarget::Outline if shape.outline_color.is_none() => {
            shape.outline_color = Some(color);
        }
        _ => {}
    }
}

fn parse_rgb_hex_color(value: &str) -> Option<Color> {
    if value.len() != 6 {
        return None;
    }
    let rgb = u32::from_str_radix(value, 16).ok()?;
    Some(Color {
        r: (rgb >> 16) as u8,
        g: (rgb >> 8) as u8,
        b: rgb as u8,
    })
}

fn attr_i64(e: &BytesStart<'_>, key: &[u8]) -> Option<i64> {
    attr_local(e, key)?.parse().ok()
}

fn attr_bool(e: &BytesStart<'_>, key: &[u8]) -> Option<bool> {
    attr_local(e, key).map(|value| {
        !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off"
        )
    })
}

fn read_leaf_text(r: &mut Reader<&[u8]>) -> String {
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Text(t)) => match t.unescape().ok().map(|c| c.into_owned()) {
                Some(c) => text.push_str(&c),
                None => text.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            },
            Ok(Event::CData(t)) => text.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text
}

fn parse_core_properties(xml: &str) -> CoreProperties {
    let mut r = Reader::from_str(xml);
    let mut props = CoreProperties::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let key = local(e.name().as_ref()).to_vec();
                if is_core_property_key(&key) {
                    set_core_property_value(&mut props, &key, read_core_property_text(&mut r));
                }
            }
            Ok(Event::Empty(e)) => {
                let key = local(e.name().as_ref()).to_vec();
                if is_core_property_key(&key) {
                    set_core_property_value(&mut props, &key, String::new());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn is_core_property_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"title"
            | b"subject"
            | b"creator"
            | b"description"
            | b"keywords"
            | b"category"
            | b"contentStatus"
            | b"lastModifiedBy"
            | b"created"
            | b"modified"
            | b"lastPrinted"
            | b"revision"
            | b"version"
    )
}

fn set_core_property_value(props: &mut CoreProperties, key: &[u8], value: String) {
    match key {
        b"title" => props.title = Some(value),
        b"subject" => props.subject = Some(value),
        b"creator" => props.creator = Some(value),
        b"description" => props.description = Some(value),
        b"keywords" => props.keywords = Some(value),
        b"category" => props.category = Some(value),
        b"contentStatus" => props.content_status = Some(value),
        b"lastModifiedBy" => props.last_modified_by = Some(value),
        b"created" => props.created = Some(value),
        b"modified" => props.modified = Some(value),
        b"lastPrinted" => props.last_printed = Some(value),
        b"revision" => props.revision = Some(value),
        b"version" => props.version = Some(value),
        _ => {}
    }
}

fn parse_custom_properties(xml: &str) -> BTreeMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut props = BTreeMap::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"property" => {
                if let Some(name) = attr_local(&e, b"name").filter(|name| !name.trim().is_empty()) {
                    if let Some(value) = read_custom_property_value(&mut r) {
                        props.insert(name, value);
                    }
                } else {
                    skip_custom_property(&mut r);
                }
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"property" => {
                if let Some(name) = attr_local(&e, b"name").filter(|name| !name.trim().is_empty()) {
                    props.insert(name, String::new());
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_custom_xml_items(zip: &mut zip::ZipArchive<std::io::Cursor<&[u8]>>) -> Vec<CustomXmlItem> {
    let mut names = Vec::new();
    for index in 0..zip.len() {
        if let Ok(file) = zip.by_index(index) {
            let name = file.name().to_string();
            if let Some(number) = custom_xml_item_number(&name) {
                names.push((number, name));
            }
        }
    }
    names.sort_by_key(|(number, _)| *number);
    names
        .into_iter()
        .filter_map(|(number, name)| {
            let xml = part(zip, &name)?;
            let store_item_id = part(zip, &format!("customXml/itemProps{number}.xml"))
                .and_then(|props| custom_xml_item_id(&props))
                .unwrap_or_default();
            Some(CustomXmlItem { store_item_id, xml })
        })
        .collect()
}

fn custom_xml_item_number(name: &str) -> Option<usize> {
    name.strip_prefix("customXml/item")?
        .strip_suffix(".xml")?
        .parse()
        .ok()
}

fn custom_xml_item_id(xml: &str) -> Option<String> {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"datastoreItem" =>
            {
                return attr_local(&e, b"itemID");
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

fn parse_extended_properties(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut props = HashMap::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let key = local(e.name().as_ref()).to_vec();
                if is_extended_property_key(&key) {
                    if let Ok(name) = std::str::from_utf8(&key) {
                        props.insert(
                            fields::document_property_key(name),
                            read_core_property_text(&mut r),
                        );
                    }
                }
            }
            Ok(Event::Empty(e)) => {
                let key = local(e.name().as_ref()).to_vec();
                if is_extended_property_key(&key) {
                    if let Ok(name) = std::str::from_utf8(&key) {
                        props.insert(fields::document_property_key(name), String::new());
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn is_extended_property_key(key: &[u8]) -> bool {
    matches!(
        key,
        b"Application"
            | b"AppVersion"
            | b"Characters"
            | b"CharactersWithSpaces"
            | b"Company"
            | b"DocSecurity"
            | b"HiddenSlides"
            | b"HyperlinkBase"
            | b"HyperlinksChanged"
            | b"Lines"
            | b"LinksUpToDate"
            | b"Manager"
            | b"MMClips"
            | b"Notes"
            | b"Pages"
            | b"Paragraphs"
            | b"PresentationFormat"
            | b"ScaleCrop"
            | b"SharedDoc"
            | b"Slides"
            | b"Template"
            | b"TotalTime"
            | b"Words"
    )
}

fn parse_document_variables(xml: &str) -> HashMap<String, String> {
    let mut r = Reader::from_str(xml);
    let mut vars = HashMap::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"docVar" => {
                if let Some(name) = attr_local(&e, b"name").filter(|name| !name.trim().is_empty()) {
                    vars.insert(
                        fields::document_property_key(&name),
                        attr_local(&e, b"val").unwrap_or_default(),
                    );
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    vars
}

fn parse_document_id(xml: &str) -> Option<String> {
    let mut r = Reader::from_str(xml);
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if e.name().as_ref() == b"w14:docId" => {
                return attr_local(&e, b"val").filter(|id| !id.trim().is_empty());
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    None
}

fn read_custom_property_value(r: &mut Reader<&[u8]>) -> Option<String> {
    let mut value = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) if value.is_none() => {
                value = Some(read_core_property_text(r));
            }
            Ok(Event::Empty(_)) if value.is_none() => {
                value = Some(String::new());
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"property" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    value
}

fn skip_custom_property(r: &mut Reader<&[u8]>) {
    let mut depth = 1usize;
    loop {
        match r.read_event() {
            Ok(Event::Start(_)) => depth += 1,
            Ok(Event::End(e)) => {
                if depth == 1 && local(e.name().as_ref()) == b"property" {
                    break;
                }
                depth = depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

fn read_core_property_text(r: &mut Reader<&[u8]>) -> String {
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Text(t)) => match t.unescape().ok().map(|c| c.into_owned()) {
                Some(c) => text.push_str(&c),
                None => text.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            },
            Ok(Event::CData(t)) => text.push_str(&String::from_utf8_lossy(t.into_inner().as_ref())),
            Ok(Event::End(_)) | Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    text
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
                    rotation_degrees: None,
                    floating_offset_emu: None,
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

/// Flat text of just the main body (excludes headers/footers).
fn body_text(model: &DocModel) -> String {
    blocks_text(&model.blocks)
}

fn blocks_text(blocks: &[Block]) -> String {
    let mut raw = String::new();
    flatten(blocks, &mut raw);
    text::finalize(&raw)
}

fn attach_note_reference_anchors(notes: &mut [Note], doc_xml: &str) {
    let footnote_refs = body::scan_note_ref_anchors(doc_xml, b"footnoteReference");
    let endnote_refs = body::scan_note_ref_anchors(doc_xml, b"endnoteReference");
    for note in notes {
        let anchor_text = match note.kind {
            NoteKind::Footnote => footnote_refs.get(&note.id),
            NoteKind::Endnote => endnote_refs.get(&note.id),
        };
        if let Some(text) = anchor_text {
            note.anchor = Some(TextAnchor {
                id: note.id.clone(),
                text: text.clone(),
            });
        }
    }
}

/// Flat text of the running headers and footers only.
pub(crate) fn header_footer_text(model: &DocModel) -> String {
    let mut raw = String::new();
    flatten_header_footer_surfaces(model, &mut raw);
    text::finalize(&raw)
}

fn flatten_header_footer_surfaces(model: &DocModel, out: &mut String) {
    for block in &model.blocks {
        if let Block::SectionBreak(section) = block {
            flatten(&section.header, out);
            flatten(&section.first_header, out);
            flatten(&section.even_header, out);
            flatten(&section.footer, out);
            flatten(&section.first_footer, out);
            flatten(&section.even_footer, out);
        }
    }
    flatten(&model.setup.header, out);
    flatten(&model.setup.first_header, out);
    flatten(&model.setup.even_header, out);
    flatten(&model.setup.footer, out);
    flatten(&model.setup.first_footer, out);
    flatten(&model.setup.even_footer, out);
}

pub(crate) fn main_text_with_revision_view(state: &DocxState, view: crate::RevisionView) -> String {
    state
        .package
        .part("word/document.xml")
        .map(|xml| revisions::main_text_with_view(&String::from_utf8_lossy(&xml), view))
        .unwrap_or_else(|| state.main_text.clone())
}

fn flatten(blocks: &[Block], out: &mut String) {
    for b in blocks {
        match b {
            Block::Paragraph(p) => {
                out.push_str(&p.text());
                out.push('\n');
            }
            Block::PageBreak | Block::SectionBreak(_) => out.push('\n'),
            Block::Image(_) | Block::Chart(_) => {}
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
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
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
