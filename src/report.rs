//! Machine-readable document feature reports.
//!
//! The report surface is intentionally conservative: feature counts mean rdoc
//! observed format markers for a construct, not that every behavior of that
//! construct is fully modeled, editable, or renderable.

use crate::annotation::{
    accept_field_number_format_switch, accept_field_text_format_switch,
    accept_general_format_switch, Field, FieldKind, FieldNumberFormat, FieldTextFormat,
};
use crate::model::{Block, FieldRole, Stats, Table};
use crate::CoreProperties;
#[cfg(feature = "docx")]
use crate::RevisionKind;
use std::collections::HashSet;
#[cfg(feature = "docx")]
use std::io::Read;

/// Source document format detected by [`crate::Document::open`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentFormat {
    /// Legacy Word 97-2003 binary `.doc`.
    Doc,
    /// Modern OOXML WordprocessingML `.docx`.
    Docx,
}

impl DocumentFormat {
    fn as_json_str(self) -> &'static str {
        match self {
            DocumentFormat::Doc => "doc",
            DocumentFormat::Docx => "docx",
        }
    }
}

/// Reason package-preserving edits are unavailable for an opened document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditReadOnlyReason {
    /// The source was a legacy binary `.doc`; rdoc can convert it to `.docx`,
    /// but cannot preserve-edit the binary package in place.
    LegacyDoc,
    /// The OPC ZIP contained one or more unreadable entries, so saving the
    /// retained package would silently drop data.
    IncompletePackage,
    /// `[Content_Types].xml`, a `.rels` part, or case-colliding package metadata
    /// parsed lossily; edits that regenerate OPC metadata are therefore refused.
    LossyOpcMetadata,
}

impl EditReadOnlyReason {
    fn as_json_str(&self) -> &'static str {
        match self {
            EditReadOnlyReason::LegacyDoc => "legacy_doc",
            EditReadOnlyReason::IncompletePackage => "incomplete_package",
            EditReadOnlyReason::LossyOpcMetadata => "lossy_opc_metadata",
        }
    }
}

/// Whether package-preserving edit APIs are available for an opened document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditCapability {
    /// `true` when preservation edit APIs such as
    /// [`crate::Document::replace_body_text`] can mutate the retained source
    /// package and later [`crate::Document::save`] it.
    pub package_preserving: bool,
    /// Machine-readable reasons edits are unavailable. Empty when
    /// [`EditCapability::package_preserving`] is `true`.
    pub read_only_reasons: Vec<EditReadOnlyReason>,
}

impl EditCapability {
    #[cfg(feature = "docx")]
    pub(crate) fn editable() -> Self {
        Self {
            package_preserving: true,
            read_only_reasons: Vec::new(),
        }
    }

    pub(crate) fn read_only(read_only_reasons: Vec<EditReadOnlyReason>) -> Self {
        Self {
            package_preserving: false,
            read_only_reasons,
        }
    }
}

/// Count of observed fields for one field kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldKindCount {
    /// Field kind.
    pub kind: FieldKind,
    /// Number of fields of this kind.
    pub count: usize,
}

/// Reason a field's computed value is not available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldEvaluationReason {
    /// rdoc does not know this field instruction class yet.
    UnknownField,
    /// The field points at a bookmark/scope rdoc could not resolve.
    UnresolvedBookmark,
    /// The instruction contains a switch whose value can change the result.
    UnsupportedSwitch,
    /// The instruction is supported, but the document contains no computable value.
    NoComputedResult,
}

impl FieldEvaluationReason {
    fn as_json_str(self) -> &'static str {
        match self {
            FieldEvaluationReason::UnknownField => "UnknownField",
            FieldEvaluationReason::UnresolvedBookmark => "UnresolvedBookmark",
            FieldEvaluationReason::UnsupportedSwitch => "UnsupportedSwitch",
            FieldEvaluationReason::NoComputedResult => "NoComputedResult",
        }
    }
}

/// Count of unsupported field evaluations for one reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldEvaluationReasonCount {
    /// Unsupported evaluation reason.
    pub reason: FieldEvaluationReason,
    /// Number of fields with this reason.
    pub count: usize,
}

/// Metafile image container observed in a package.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetafileFormat {
    /// Enhanced Metafile (`.emf` or compressed `.emz`).
    Emf,
    /// Windows Metafile (`.wmf` or compressed `.wmz`).
    Wmf,
}

impl MetafileFormat {
    fn as_json_str(self) -> &'static str {
        match self {
            MetafileFormat::Emf => "EMF",
            MetafileFormat::Wmf => "WMF",
        }
    }
}

/// Best-effort metadata for a preserved WMF/EMF package part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetafileInfo {
    /// OPC part path, for example `word/media/image1.emf`.
    pub path: String,
    /// Metafile family inferred from the part extension.
    pub format: MetafileFormat,
    /// Stored part payload size in bytes.
    pub bytes: usize,
    /// `true` for compressed `.emz`/`.wmz` wrappers or gzip-marked payloads.
    pub compressed: bool,
    /// Header-derived width in pixels/preview units when recoverable.
    pub width_px: Option<u32>,
    /// Header-derived height in pixels/preview units when recoverable.
    pub height_px: Option<u32>,
}

/// Counts of Word features observed while opening a document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureInventory {
    /// Comment anchors or comment records.
    pub comments: usize,
    /// Footnote records.
    pub footnotes: usize,
    /// Endnote records.
    pub endnotes: usize,
    /// Text-box records.
    pub text_boxes: usize,
    /// Tracked insertion markers.
    pub tracked_insertions: usize,
    /// Tracked deletion markers.
    pub tracked_deletions: usize,
    /// Tracked move markers.
    pub tracked_moves: usize,
    /// Tracked property-change markers such as `w:pPrChange` and `w:rPrChange`.
    pub tracked_property_changes: usize,
    /// Field markers or field instructions.
    pub fields: usize,
    /// Field counts grouped by normalized field kind.
    pub field_kinds: Vec<FieldKindCount>,
    /// Field counts whose computed values rdoc still cannot evaluate.
    pub unsupported_field_kinds: Vec<FieldKindCount>,
    /// Unsupported field evaluation counts grouped by reason.
    pub unsupported_field_reasons: Vec<FieldEvaluationReasonCount>,
    /// Relationship-backed or field-backed hyperlinks.
    pub hyperlinks: usize,
    /// Content controls.
    pub content_controls: usize,
    /// Tables nested inside another table cell.
    pub nested_tables: usize,
    /// Floating or alternate-content shape markers.
    pub floating_shapes: usize,
    /// Chart parts or chart references.
    pub charts: usize,
    /// OLE embedded object markers.
    pub ole_objects: usize,
    /// WMF/EMF image parts, which rdoc preserves but does not render.
    pub unsupported_metafiles: usize,
    /// Best-effort metadata for preserved WMF/EMF image parts.
    pub metafiles: Vec<MetafileInfo>,
}

impl FeatureInventory {
    /// `true` when this inventory contains features the current renderer cannot
    /// faithfully draw beyond placeholders or preserved package payloads.
    pub fn has_unsupported_render_features(&self) -> bool {
        self.floating_shapes > 0
            || self.charts > 0
            || self.ole_objects > 0
            || self.unsupported_metafiles > 0
    }
}

/// Human- and machine-readable warnings derived from [`FeatureInventory`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DocumentWarning {
    /// Field instructions whose computed value rdoc does not evaluate yet were
    /// found; cached visible text is preserved instead.
    UnsupportedFieldEvaluation {
        /// Number of observed field markers/instructions.
        count: usize,
        /// Observed field kinds.
        field_kinds: Vec<FieldKindCount>,
    },
    /// Tracked-change markup was found.
    TrackedChangesPresent {
        /// Number of insertion markers.
        insertions: usize,
        /// Number of deletion markers.
        deletions: usize,
        /// Number of move markers.
        moves: usize,
    },
    /// Tracked property-change markup was found. Revision text views preserve
    /// current visible text, but do not reconstruct original formatting.
    IncompleteRevisionView {
        /// Number of tracked property-change markers.
        property_changes: usize,
    },
    /// Floating shape markers were found.
    FloatingShapePlaceholderOnly {
        /// Number of observed floating/alternate-content shape markers.
        count: usize,
    },
    /// Chart payloads or references were found.
    ChartsPreservedButNotModeled {
        /// Number of chart parts/references observed.
        count: usize,
    },
    /// OLE object markers were found.
    OleObjectsPreservedButNotModeled {
        /// Number of OLE object markers observed.
        count: usize,
    },
    /// WMF/EMF/EMZ/WMZ images were found.
    UnsupportedMetafileImages {
        /// Number of WMF/EMF parts observed.
        count: usize,
    },
    /// A legacy `.doc` contains non-body subdocuments that the current rich
    /// model still flattens into the body flow.
    LegacyDocFlattenedSubdocuments {
        /// Footnote characters reported by the FIB.
        footnotes: usize,
        /// Header/footer characters reported by the FIB.
        headers_footers: usize,
        /// Annotation/comment characters reported by the FIB.
        annotations: usize,
        /// Endnote characters reported by the FIB.
        endnotes: usize,
        /// Text-box characters reported by the FIB.
        text_boxes: usize,
    },
    /// Package-preserving edits are unavailable for this opened document.
    PackageReadOnly {
        /// Machine-readable read-only reasons.
        reasons: Vec<EditReadOnlyReason>,
    },
}

/// Summary of the opened document's format, visible stats, observed feature
/// markers, and warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentReport {
    /// Detected source format.
    pub format: DocumentFormat,
    /// Visible model statistics.
    pub stats: Stats,
    /// Core document metadata.
    pub core_properties: CoreProperties,
    /// Package-preserving edit availability and read-only reasons.
    pub edit: EditCapability,
    /// Package part names touched by preservation edits in the current session.
    pub edited_parts: Vec<String>,
    /// Observed feature inventory.
    pub features: FeatureInventory,
    /// Warnings derived from the feature inventory.
    pub warnings: Vec<DocumentWarning>,
}

/// Human- and machine-readable warnings derived from features the current
/// renderer cannot faithfully compute or draw yet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RenderWarning {
    /// Field instructions whose computed value the renderer does not evaluate
    /// yet are rendered from cached visible text.
    UnsupportedFieldEvaluation {
        /// Number of observed field markers/instructions.
        count: usize,
        /// Observed field kinds.
        field_kinds: Vec<FieldKindCount>,
    },
    /// Floating shape markers were found; renderer support is placeholder-only.
    FloatingShapePlaceholderOnly {
        /// Number of observed floating/alternate-content shape markers.
        count: usize,
    },
    /// Chart payloads or references were found, but charts are not drawn.
    ChartsPreservedButNotModeled {
        /// Number of chart parts/references observed.
        count: usize,
    },
    /// OLE object markers were found, but embedded OLE payloads are not drawn.
    OleObjectsPreservedButNotModeled {
        /// Number of OLE object markers observed.
        count: usize,
    },
    /// WMF/EMF/EMZ/WMZ images were found, but the renderer does not draw them.
    UnsupportedMetafileImages {
        /// Number of WMF/EMF parts observed.
        count: usize,
    },
}

/// Renderer metrics and warnings for a generated PDF.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderReport {
    /// Number of PDF pages emitted by the renderer.
    pub pages: usize,
    /// Renderer-specific warnings derived from the feature inventory.
    pub warnings: Vec<RenderWarning>,
    /// Observed features relevant to unsupported or partial rendering behavior.
    pub unsupported: FeatureInventory,
}

/// PDF bytes plus the render report produced by the same pagination pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedPdf {
    /// Generated PDF bytes.
    pub pdf: Vec<u8>,
    /// Render metrics and warnings for `pdf`.
    pub report: RenderReport,
}

impl DocumentReport {
    /// Serialize this report as compact JSON without requiring a serde
    /// dependency.
    ///
    /// The schema is intentionally small and stable enough for examples, shell
    /// scripts, and future CLI output:
    /// `format`, `stats`, `core_properties`, `edit`, `edited_parts`, `features`,
    /// and `warnings`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        json_field_str(&mut out, "format", self.format.as_json_str());
        out.push(',');
        out.push_str("\"stats\":{");
        json_field_num(&mut out, "paragraphs", self.stats.paragraphs);
        out.push(',');
        json_field_num(&mut out, "tables", self.stats.tables);
        out.push(',');
        json_field_num(&mut out, "figures", self.stats.figures);
        out.push(',');
        json_field_num(&mut out, "text_chars", self.stats.text_chars);
        out.push('}');
        out.push(',');
        out.push_str("\"core_properties\":");
        core_properties_json(&mut out, &self.core_properties);
        out.push(',');
        out.push_str("\"edit\":");
        edit_capability_json(&mut out, &self.edit);
        out.push(',');
        out.push_str("\"edited_parts\":");
        json_string_array(&mut out, &self.edited_parts);
        out.push(',');
        out.push_str("\"features\":");
        feature_inventory_json(&mut out, &self.features);
        out.push(',');
        out.push_str("\"warnings\":[");
        for (i, warning) in self.warnings.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            warning_json(&mut out, warning);
        }
        out.push_str("]}");
        out
    }
}

impl RenderReport {
    /// Serialize this render report as compact JSON without requiring a serde
    /// dependency.
    ///
    /// The schema is intended for CLI and validation-script output:
    /// `pages`, `unsupported`, and `warnings`.
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        json_field_num(&mut out, "pages", self.pages);
        out.push(',');
        out.push_str("\"unsupported\":");
        feature_inventory_json(&mut out, &self.unsupported);
        out.push(',');
        out.push_str("\"warnings\":[");
        for (i, warning) in self.warnings.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            render_warning_json(&mut out, warning);
        }
        out.push_str("]}");
        out
    }
}

pub(crate) fn warnings_for(
    features: &FeatureInventory,
    edit: &EditCapability,
) -> Vec<DocumentWarning> {
    let mut warnings = Vec::new();
    let (unsupported_fields, unsupported_field_kinds) = unsupported_field_evaluation(features);
    if unsupported_fields > 0 {
        warnings.push(DocumentWarning::UnsupportedFieldEvaluation {
            count: unsupported_fields,
            field_kinds: unsupported_field_kinds,
        });
    }
    if features.tracked_insertions > 0
        || features.tracked_deletions > 0
        || features.tracked_moves > 0
    {
        warnings.push(DocumentWarning::TrackedChangesPresent {
            insertions: features.tracked_insertions,
            deletions: features.tracked_deletions,
            moves: features.tracked_moves,
        });
    }
    if features.tracked_property_changes > 0 {
        warnings.push(DocumentWarning::IncompleteRevisionView {
            property_changes: features.tracked_property_changes,
        });
    }
    if features.floating_shapes > 0 {
        warnings.push(DocumentWarning::FloatingShapePlaceholderOnly {
            count: features.floating_shapes,
        });
    }
    if features.charts > 0 {
        warnings.push(DocumentWarning::ChartsPreservedButNotModeled {
            count: features.charts,
        });
    }
    if features.ole_objects > 0 {
        warnings.push(DocumentWarning::OleObjectsPreservedButNotModeled {
            count: features.ole_objects,
        });
    }
    if features.unsupported_metafiles > 0 {
        warnings.push(DocumentWarning::UnsupportedMetafileImages {
            count: features.unsupported_metafiles,
        });
    }
    if !edit.package_preserving {
        warnings.push(DocumentWarning::PackageReadOnly {
            reasons: edit.read_only_reasons.clone(),
        });
    }
    warnings
}

pub(crate) fn legacy_doc_flattened_subdocuments_warning(
    footnotes: usize,
    headers_footers: usize,
    annotations: usize,
    endnotes: usize,
    text_boxes: usize,
) -> Option<DocumentWarning> {
    let total = footnotes
        .saturating_add(headers_footers)
        .saturating_add(annotations)
        .saturating_add(endnotes)
        .saturating_add(text_boxes);
    (total > 0).then_some(DocumentWarning::LegacyDocFlattenedSubdocuments {
        footnotes,
        headers_footers,
        annotations,
        endnotes,
        text_boxes,
    })
}

#[cfg(feature = "render")]
pub(crate) fn render_warnings_for(features: &FeatureInventory) -> Vec<RenderWarning> {
    let mut warnings = Vec::new();
    let (unsupported_fields, unsupported_field_kinds) = unsupported_field_evaluation(features);
    if unsupported_fields > 0 {
        warnings.push(RenderWarning::UnsupportedFieldEvaluation {
            count: unsupported_fields,
            field_kinds: unsupported_field_kinds,
        });
    }
    if features.floating_shapes > 0 {
        warnings.push(RenderWarning::FloatingShapePlaceholderOnly {
            count: features.floating_shapes,
        });
    }
    if features.charts > 0 {
        warnings.push(RenderWarning::ChartsPreservedButNotModeled {
            count: features.charts,
        });
    }
    if features.ole_objects > 0 {
        warnings.push(RenderWarning::OleObjectsPreservedButNotModeled {
            count: features.ole_objects,
        });
    }
    if features.unsupported_metafiles > 0 {
        warnings.push(RenderWarning::UnsupportedMetafileImages {
            count: features.unsupported_metafiles,
        });
    }
    warnings
}

#[cfg(feature = "render")]
pub(crate) fn render_unsupported_features(features: &FeatureInventory) -> FeatureInventory {
    let mut unsupported = features.clone();
    unsupported.field_kinds = unsupported.unsupported_field_kinds.clone();
    unsupported.fields = unsupported
        .unsupported_field_kinds
        .iter()
        .map(|item| item.count)
        .sum();
    unsupported
}

fn unsupported_field_evaluation(features: &FeatureInventory) -> (usize, Vec<FieldKindCount>) {
    let count = features
        .unsupported_field_kinds
        .iter()
        .map(|item| item.count)
        .sum();
    (count, features.unsupported_field_kinds.clone())
}

fn supports_field_kind_evaluation(kind: &FieldKind) -> bool {
    matches!(kind, FieldKind::DocumentInfo(_))
}

fn supports_field_evaluation(field: &Field) -> bool {
    if field.computed_result.is_some() {
        return true;
    }
    if field.kind == FieldKind::Hyperlink {
        return supports_hyperlink_field_evaluation(field);
    }
    if matches!(&field.kind, FieldKind::DocumentInfo(_)) {
        return supports_document_info_field_evaluation(field);
    }
    if field.kind == FieldKind::MergeField {
        return supports_merge_field_evaluation(field);
    }
    if field.kind == FieldKind::Filename {
        return supported_filename_syntax(&field.instruction);
    }
    supports_field_kind_evaluation(&field.kind)
}

fn supports_hyperlink_field_evaluation(field: &Field) -> bool {
    #[cfg(feature = "docx")]
    {
        crate::docx::supports_hyperlink_field_syntax(&field.instruction)
    }
    #[cfg(not(feature = "docx"))]
    {
        let _ = field;
        true
    }
}

fn supports_merge_field_evaluation(field: &Field) -> bool {
    #[cfg(feature = "docx")]
    {
        crate::merge_field_name(&field.instruction).is_some()
    }
    #[cfg(not(feature = "docx"))]
    {
        let _ = field;
        true
    }
}

fn supports_document_info_field_evaluation(field: &Field) -> bool {
    #[cfg(feature = "docx")]
    {
        crate::docx::supports_document_info_field_syntax(&field.instruction)
    }
    #[cfg(not(feature = "docx"))]
    {
        let _ = field;
        true
    }
}

#[cfg(feature = "render")]
fn supports_render_model_field_evaluation(field: &Field) -> bool {
    matches!(field.kind, FieldKind::Page) || supports_field_evaluation(field)
}

pub(crate) fn fields_for_model(blocks: &[Block]) -> Vec<Field> {
    let mut fields = Vec::new();
    collect_model_fields(blocks, &mut fields);
    fields
}

pub(crate) fn feature_inventory_for_model(blocks: &[Block]) -> FeatureInventory {
    let mut inventory = FeatureInventory::default();
    let fields = fields_for_model(blocks);
    inventory.fields = fields.len();
    inventory.field_kinds = count_field_kinds(&fields);
    inventory.unsupported_field_kinds = count_unsupported_field_kinds(&fields);
    inventory.unsupported_field_reasons = count_unsupported_field_reasons(&fields);
    inventory.hyperlinks = fields
        .iter()
        .filter(|field| field.kind == FieldKind::Hyperlink)
        .count();
    count_nested_model_tables(blocks, 0, &mut inventory, true);
    inventory
}

#[cfg(feature = "render")]
pub(crate) fn render_inventory_for_model(blocks: &[Block]) -> FeatureInventory {
    let mut inventory = FeatureInventory::default();
    let fields = fields_for_model(blocks);
    inventory.fields = fields.len();
    inventory.field_kinds = count_field_kinds(&fields);
    inventory.hyperlinks = fields
        .iter()
        .filter(|field| field.kind == FieldKind::Hyperlink)
        .count();
    for field in &fields {
        if supports_render_model_field_evaluation(field) {
            continue;
        }
        if let Some(existing) = inventory
            .unsupported_field_kinds
            .iter_mut()
            .find(|item| item.kind == field.kind)
        {
            existing.count += 1;
        } else {
            inventory.unsupported_field_kinds.push(FieldKindCount {
                kind: field.kind.clone(),
                count: 1,
            });
        }
        if let Some(reason) = unsupported_field_reason(field) {
            increment_field_evaluation_reason_count(
                &mut inventory.unsupported_field_reasons,
                reason,
            );
        }
    }
    count_nested_model_tables(blocks, 0, &mut inventory, false);
    inventory
}

fn count_nested_model_tables(
    blocks: &[Block],
    depth: usize,
    inventory: &mut FeatureInventory,
    count_charts: bool,
) {
    for block in blocks {
        match block {
            Block::Table(table) => {
                if depth > 0 {
                    inventory.nested_tables += 1;
                }
                for row in &table.rows {
                    for cell in &row.cells {
                        count_nested_model_tables(&cell.blocks, depth + 1, inventory, count_charts);
                    }
                }
            }
            Block::Chart(_) if count_charts => inventory.charts += 1,
            Block::Chart(_) => {}
            Block::Paragraph(_) | Block::Image(_) | Block::PageBreak | Block::SectionBreak(_) => {}
        }
    }
}

fn collect_model_fields(blocks: &[Block], out: &mut Vec<Field>) {
    for block in blocks {
        match block {
            Block::Paragraph(paragraph) => {
                let mut current: Option<Field> = None;
                for run in &paragraph.runs {
                    let field = field_from_role(&run.field, &run.text);
                    match field {
                        Some(field) => {
                            if let Some(active) = &mut current {
                                if active.kind == field.kind
                                    && active.instruction == field.instruction
                                {
                                    active.result.push_str(&field.result);
                                    continue;
                                }
                                out.push(current.take().expect("checked above"));
                            }
                            current = Some(field);
                        }
                        None => {
                            if let Some(done) = current.take() {
                                out.push(done);
                            }
                        }
                    }
                }
                if let Some(done) = current {
                    out.push(done);
                }
            }
            Block::Table(table) => collect_model_table_fields(table, out),
            Block::Image(_) | Block::Chart(_) | Block::PageBreak | Block::SectionBreak(_) => {}
        }
    }
}

fn collect_model_table_fields(table: &Table, out: &mut Vec<Field>) {
    for row in &table.rows {
        for cell in &row.cells {
            collect_model_fields(&cell.blocks, out);
        }
    }
}

fn field_from_role(role: &FieldRole, result: &str) -> Option<Field> {
    match role {
        FieldRole::Simple { instruction } => {
            let instruction = normalize_model_field_instruction(instruction);
            if instruction.is_empty() {
                None
            } else {
                Some(Field {
                    kind: FieldKind::from_instruction(&instruction),
                    instruction,
                    result: result.to_string(),
                    computed_result: None,
                })
            }
        }
        FieldRole::Hyperlink { url } => Some(Field {
            kind: FieldKind::Hyperlink,
            instruction: format!("HYPERLINK \"{url}\""),
            result: result.to_string(),
            computed_result: None,
        }),
        FieldRole::None | FieldRole::Other => None,
    }
}

fn normalize_model_field_instruction(instruction: &str) -> String {
    instruction.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn doc_edit_capability() -> EditCapability {
    EditCapability::read_only(vec![EditReadOnlyReason::LegacyDoc])
}

fn json_field_str(out: &mut String, name: &str, value: &str) {
    push_json_string(out, name);
    out.push(':');
    push_json_string(out, value);
}

fn json_field_opt_str(out: &mut String, name: &str, value: Option<&str>) {
    push_json_string(out, name);
    out.push(':');
    match value {
        Some(value) => push_json_string(out, value),
        None => out.push_str("null"),
    }
}

fn json_field_num<T: std::fmt::Display>(out: &mut String, name: &str, value: T) {
    push_json_string(out, name);
    out.push(':');
    out.push_str(&value.to_string());
}

fn json_field_bool(out: &mut String, name: &str, value: bool) {
    push_json_string(out, name);
    out.push(':');
    out.push_str(if value { "true" } else { "false" });
}

fn json_field_opt_num<T: std::fmt::Display>(out: &mut String, name: &str, value: Option<T>) {
    push_json_string(out, name);
    out.push(':');
    match value {
        Some(value) => out.push_str(&value.to_string()),
        None => out.push_str("null"),
    }
}

fn edit_capability_json(out: &mut String, edit: &EditCapability) {
    out.push('{');
    json_field_bool(out, "package_preserving", edit.package_preserving);
    out.push(',');
    out.push_str("\"read_only_reasons\":");
    let reasons: Vec<&str> = edit
        .read_only_reasons
        .iter()
        .map(EditReadOnlyReason::as_json_str)
        .collect();
    json_str_array(out, &reasons);
    out.push('}');
}

fn json_string_array(out: &mut String, values: &[String]) {
    let refs: Vec<&str> = values.iter().map(String::as_str).collect();
    json_str_array(out, &refs);
}

fn core_properties_json(out: &mut String, props: &CoreProperties) {
    out.push('{');
    json_field_opt_str(out, "title", props.title.as_deref());
    out.push(',');
    json_field_opt_str(out, "subject", props.subject.as_deref());
    out.push(',');
    json_field_opt_str(out, "creator", props.creator.as_deref());
    out.push(',');
    json_field_opt_str(out, "description", props.description.as_deref());
    out.push(',');
    json_field_opt_str(out, "keywords", props.keywords.as_deref());
    out.push(',');
    json_field_opt_str(out, "category", props.category.as_deref());
    out.push(',');
    json_field_opt_str(out, "content_status", props.content_status.as_deref());
    out.push(',');
    json_field_opt_str(out, "last_modified_by", props.last_modified_by.as_deref());
    out.push(',');
    json_field_opt_str(out, "created", props.created.as_deref());
    out.push(',');
    json_field_opt_str(out, "modified", props.modified.as_deref());
    out.push(',');
    json_field_opt_str(out, "last_printed", props.last_printed.as_deref());
    out.push(',');
    json_field_opt_str(out, "revision", props.revision.as_deref());
    out.push(',');
    json_field_opt_str(out, "version", props.version.as_deref());
    out.push('}');
}

fn json_str_array(out: &mut String, values: &[&str]) {
    out.push('[');
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_json_string(out, value);
    }
    out.push(']');
}

fn feature_inventory_json(out: &mut String, features: &FeatureInventory) {
    out.push('{');
    json_field_num(out, "comments", features.comments);
    out.push(',');
    json_field_num(out, "footnotes", features.footnotes);
    out.push(',');
    json_field_num(out, "endnotes", features.endnotes);
    out.push(',');
    json_field_num(out, "text_boxes", features.text_boxes);
    out.push(',');
    json_field_num(out, "tracked_insertions", features.tracked_insertions);
    out.push(',');
    json_field_num(out, "tracked_deletions", features.tracked_deletions);
    out.push(',');
    json_field_num(out, "tracked_moves", features.tracked_moves);
    out.push(',');
    json_field_num(
        out,
        "tracked_property_changes",
        features.tracked_property_changes,
    );
    out.push(',');
    json_field_num(out, "fields", features.fields);
    out.push(',');
    out.push_str("\"field_kinds\":[");
    for (i, item) in features.field_kinds.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field_kind_count_json(out, item);
    }
    out.push(']');
    out.push(',');
    out.push_str("\"unsupported_field_kinds\":[");
    for (i, item) in features.unsupported_field_kinds.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field_kind_count_json(out, item);
    }
    out.push(']');
    out.push(',');
    out.push_str("\"unsupported_field_reasons\":[");
    for (i, item) in features.unsupported_field_reasons.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field_evaluation_reason_count_json(out, item);
    }
    out.push(']');
    out.push(',');
    json_field_num(out, "hyperlinks", features.hyperlinks);
    out.push(',');
    json_field_num(out, "content_controls", features.content_controls);
    out.push(',');
    json_field_num(out, "nested_tables", features.nested_tables);
    out.push(',');
    json_field_num(out, "floating_shapes", features.floating_shapes);
    out.push(',');
    json_field_num(out, "charts", features.charts);
    out.push(',');
    json_field_num(out, "ole_objects", features.ole_objects);
    out.push(',');
    json_field_num(out, "unsupported_metafiles", features.unsupported_metafiles);
    out.push(',');
    out.push_str("\"metafiles\":[");
    for (i, item) in features.metafiles.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        metafile_info_json(out, item);
    }
    out.push(']');
    out.push('}');
}

fn metafile_info_json(out: &mut String, item: &MetafileInfo) {
    out.push('{');
    json_field_str(out, "path", &item.path);
    out.push(',');
    json_field_str(out, "format", item.format.as_json_str());
    out.push(',');
    json_field_num(out, "bytes", item.bytes);
    out.push(',');
    json_field_bool(out, "compressed", item.compressed);
    out.push(',');
    json_field_opt_num(out, "width_px", item.width_px);
    out.push(',');
    json_field_opt_num(out, "height_px", item.height_px);
    out.push('}');
}

fn warning_json(out: &mut String, warning: &DocumentWarning) {
    out.push('{');
    match warning {
        DocumentWarning::UnsupportedFieldEvaluation { count, field_kinds } => {
            json_field_str(out, "kind", "UnsupportedFieldEvaluation");
            out.push(',');
            json_field_num(out, "count", count);
            out.push(',');
            out.push_str("\"field_kinds\":[");
            for (i, item) in field_kinds.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                field_kind_count_json(out, item);
            }
            out.push(']');
        }
        DocumentWarning::TrackedChangesPresent {
            insertions,
            deletions,
            moves,
        } => {
            json_field_str(out, "kind", "TrackedChangesPresent");
            out.push(',');
            json_field_num(out, "insertions", insertions);
            out.push(',');
            json_field_num(out, "deletions", deletions);
            out.push(',');
            json_field_num(out, "moves", moves);
        }
        DocumentWarning::IncompleteRevisionView { property_changes } => {
            json_field_str(out, "kind", "IncompleteRevisionView");
            out.push(',');
            json_field_num(out, "property_changes", property_changes);
        }
        DocumentWarning::FloatingShapePlaceholderOnly { count } => {
            json_field_str(out, "kind", "FloatingShapePlaceholderOnly");
            out.push(',');
            json_field_num(out, "count", count);
        }
        DocumentWarning::ChartsPreservedButNotModeled { count } => {
            json_field_str(out, "kind", "ChartsPreservedButNotModeled");
            out.push(',');
            json_field_num(out, "count", count);
        }
        DocumentWarning::OleObjectsPreservedButNotModeled { count } => {
            json_field_str(out, "kind", "OleObjectsPreservedButNotModeled");
            out.push(',');
            json_field_num(out, "count", count);
        }
        DocumentWarning::UnsupportedMetafileImages { count } => {
            json_field_str(out, "kind", "UnsupportedMetafileImages");
            out.push(',');
            json_field_num(out, "count", count);
        }
        DocumentWarning::LegacyDocFlattenedSubdocuments {
            footnotes,
            headers_footers,
            annotations,
            endnotes,
            text_boxes,
        } => {
            json_field_str(out, "kind", "LegacyDocFlattenedSubdocuments");
            out.push(',');
            json_field_num(out, "footnotes", footnotes);
            out.push(',');
            json_field_num(out, "headers_footers", headers_footers);
            out.push(',');
            json_field_num(out, "annotations", annotations);
            out.push(',');
            json_field_num(out, "endnotes", endnotes);
            out.push(',');
            json_field_num(out, "text_boxes", text_boxes);
        }
        DocumentWarning::PackageReadOnly { reasons } => {
            json_field_str(out, "kind", "PackageReadOnly");
            out.push(',');
            out.push_str("\"reasons\":[");
            for (i, reason) in reasons.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                push_json_string(out, reason.as_json_str());
            }
            out.push(']');
        }
    }
    out.push('}');
}

fn render_warning_json(out: &mut String, warning: &RenderWarning) {
    out.push('{');
    match warning {
        RenderWarning::UnsupportedFieldEvaluation { count, field_kinds } => {
            json_field_str(out, "kind", "UnsupportedFieldEvaluation");
            out.push(',');
            json_field_num(out, "count", count);
            out.push(',');
            out.push_str("\"field_kinds\":[");
            for (i, item) in field_kinds.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                field_kind_count_json(out, item);
            }
            out.push(']');
        }
        RenderWarning::FloatingShapePlaceholderOnly { count } => {
            json_field_str(out, "kind", "FloatingShapePlaceholderOnly");
            out.push(',');
            json_field_num(out, "count", count);
        }
        RenderWarning::ChartsPreservedButNotModeled { count } => {
            json_field_str(out, "kind", "ChartsPreservedButNotModeled");
            out.push(',');
            json_field_num(out, "count", count);
        }
        RenderWarning::OleObjectsPreservedButNotModeled { count } => {
            json_field_str(out, "kind", "OleObjectsPreservedButNotModeled");
            out.push(',');
            json_field_num(out, "count", count);
        }
        RenderWarning::UnsupportedMetafileImages { count } => {
            json_field_str(out, "kind", "UnsupportedMetafileImages");
            out.push(',');
            json_field_num(out, "count", count);
        }
    }
    out.push('}');
}

fn field_kind_count_json(out: &mut String, item: &FieldKindCount) {
    out.push('{');
    json_field_str(out, "kind", item.kind.as_str());
    out.push(',');
    json_field_num(out, "count", item.count);
    out.push('}');
}

fn field_evaluation_reason_count_json(out: &mut String, item: &FieldEvaluationReasonCount) {
    out.push('{');
    json_field_str(out, "reason", item.reason.as_json_str());
    out.push(',');
    json_field_num(out, "count", item.count);
    out.push('}');
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1F}' => {
                use std::fmt::Write;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(feature = "docx")]
pub(crate) fn docx_features(docx: &crate::docx::DocxState) -> FeatureInventory {
    let mut features = FeatureInventory::default();
    let document_xml = docx.package.part("word/document.xml");
    if let Some(xml) = document_xml.as_ref() {
        scan_docx_xml(&String::from_utf8_lossy(xml), &mut features);
    }
    if let Some(xml) = docx.package.part("word/comments.xml") {
        features.comments = features
            .comments
            .max(count_elements(&String::from_utf8_lossy(&xml), b"comment"));
    }
    features.comments = features.comments.max(docx.comments.len());
    features.footnotes = docx
        .note_records
        .iter()
        .filter(|note| note.kind == crate::NoteKind::Footnote)
        .count();
    features.endnotes = docx
        .note_records
        .iter()
        .filter(|note| note.kind == crate::NoteKind::Endnote)
        .count();
    features.text_boxes = docx.text_boxes.len();
    features.fields = features.fields.max(docx.fields.len());
    features.field_kinds = count_field_kinds(&docx.fields);
    features.unsupported_field_kinds = count_unsupported_field_kinds(&docx.fields);
    features.unsupported_field_reasons =
        count_docx_unsupported_field_reasons(&docx.fields, document_xml.as_deref());
    features.hyperlinks += docx
        .fields
        .iter()
        .filter(|field| field.kind == FieldKind::Hyperlink)
        .count();
    features.floating_shapes = features.floating_shapes.max(docx.floating_shapes.len());
    let mut parsed_insertions = 0;
    let mut parsed_deletions = 0;
    let mut parsed_moves = 0;
    for rev in &docx.revisions {
        match rev.kind {
            RevisionKind::Insertion => parsed_insertions += 1,
            RevisionKind::Deletion => parsed_deletions += 1,
            RevisionKind::MoveFrom | RevisionKind::MoveTo => parsed_moves += 1,
        }
    }
    features.tracked_insertions = features.tracked_insertions.max(parsed_insertions);
    features.tracked_deletions = features.tracked_deletions.max(parsed_deletions);
    features.tracked_moves = features.tracked_moves.max(parsed_moves);
    let chart_parts = docx
        .package
        .part_names()
        .filter(|name| name.starts_with("word/charts/") && name.ends_with(".xml"))
        .count();
    features.charts = features.charts.max(chart_parts);
    features.metafiles = metafile_infos(docx);
    features.unsupported_metafiles += features.metafiles.len();
    features
}

#[cfg(feature = "docx")]
fn metafile_infos(docx: &crate::docx::DocxState) -> Vec<MetafileInfo> {
    let mut infos: Vec<MetafileInfo> = docx
        .package
        .part_names()
        .filter_map(|name| {
            let (format, compressed_by_extension) = metafile_format_for_part(name)?;
            let bytes = docx.package.part(name).unwrap_or_default();
            let compressed = compressed_by_extension || is_gzip_payload(&bytes);
            let (width_px, height_px) = metafile_dimensions(format, &bytes, compressed);
            Some(MetafileInfo {
                path: name.to_string(),
                format,
                bytes: bytes.len(),
                compressed,
                width_px,
                height_px,
            })
        })
        .collect();
    infos.sort_by(|a, b| a.path.cmp(&b.path));
    infos
}

#[cfg(feature = "docx")]
fn metafile_dimensions(
    format: MetafileFormat,
    bytes: &[u8],
    compressed: bool,
) -> (Option<u32>, Option<u32>) {
    let inflated = if compressed && is_gzip_payload(bytes) {
        inflate_gzip_metafile_header(bytes)
    } else {
        None
    };
    let payload = match (compressed, inflated.as_deref()) {
        (true, Some(payload)) => payload,
        (true, None) => return (None, None),
        (false, _) => bytes,
    };
    match format {
        MetafileFormat::Emf => emf_dimensions(payload).unzip(),
        MetafileFormat::Wmf => wmf_dimensions(payload).unzip(),
    }
}

#[cfg(feature = "docx")]
fn inflate_gzip_metafile_header(bytes: &[u8]) -> Option<Vec<u8>> {
    const MAX_METAFILE_HEADER_INFLATE: u64 = 1 << 20;
    let decoder = flate2::read::GzDecoder::new(bytes);
    let mut limited = decoder.take(MAX_METAFILE_HEADER_INFLATE);
    let mut out = Vec::new();
    limited.read_to_end(&mut out).ok()?;
    (!out.is_empty()).then_some(out)
}

#[cfg(feature = "docx")]
fn metafile_format_for_part(name: &str) -> Option<(MetafileFormat, bool)> {
    let ext = name.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "emf" => Some((MetafileFormat::Emf, false)),
        "wmf" => Some((MetafileFormat::Wmf, false)),
        "emz" => Some((MetafileFormat::Emf, true)),
        "wmz" => Some((MetafileFormat::Wmf, true)),
        _ => None,
    }
}

#[cfg(feature = "docx")]
fn is_gzip_payload(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b
}

#[cfg(feature = "docx")]
fn emf_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 44 || read_u32le(bytes, 0)? != 1 {
        return None;
    }
    let header_size = read_u32le(bytes, 4)? as usize;
    if header_size < 44 || bytes.get(40..44)? != b" EMF" {
        return None;
    }
    rect_dimensions(
        read_i32le(bytes, 8)?,
        read_i32le(bytes, 12)?,
        read_i32le(bytes, 16)?,
        read_i32le(bytes, 20)?,
    )
}

#[cfg(feature = "docx")]
fn wmf_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 22 || read_u32le(bytes, 0)? != 0x9AC6CDD7 {
        return None;
    }
    let units_per_inch = read_u16le(bytes, 14)? as u32;
    if units_per_inch == 0 {
        return None;
    }
    let width_units = (read_i16le(bytes, 10)? as i32) - (read_i16le(bytes, 6)? as i32);
    let height_units = (read_i16le(bytes, 12)? as i32) - (read_i16le(bytes, 8)? as i32);
    let width_px = scale_wmf_units(width_units, units_per_inch)?;
    let height_px = scale_wmf_units(height_units, units_per_inch)?;
    Some((width_px, height_px))
}

#[cfg(feature = "docx")]
fn rect_dimensions(left: i32, top: i32, right: i32, bottom: i32) -> Option<(u32, u32)> {
    let width = right.checked_sub(left)?;
    let height = bottom.checked_sub(top)?;
    (width > 0 && height > 0).then_some((width as u32, height as u32))
}

#[cfg(feature = "docx")]
fn scale_wmf_units(value: i32, units_per_inch: u32) -> Option<u32> {
    if value <= 0 {
        return None;
    }
    let value = value as u64;
    let units = units_per_inch as u64;
    let px = (value * 96 + units / 2) / units;
    (px > 0 && px <= u32::MAX as u64).then_some(px as u32)
}

#[cfg(feature = "docx")]
fn read_u16le(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(feature = "docx")]
fn read_i16le(bytes: &[u8], offset: usize) -> Option<i16> {
    Some(i16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

#[cfg(feature = "docx")]
fn read_u32le(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(feature = "docx")]
fn read_i32le(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

#[cfg(feature = "docx")]
pub(crate) fn docx_edit_capability(docx: &crate::docx::DocxState) -> EditCapability {
    let mut reasons = Vec::new();
    if !docx.package.is_complete() {
        reasons.push(EditReadOnlyReason::IncompletePackage);
    }
    if docx.package.is_meta_lossy() {
        reasons.push(EditReadOnlyReason::LossyOpcMetadata);
    }
    if reasons.is_empty() {
        EditCapability::editable()
    } else {
        EditCapability::read_only(reasons)
    }
}

fn count_field_kinds(fields: &[Field]) -> Vec<FieldKindCount> {
    let mut counts: Vec<FieldKindCount> = Vec::new();
    for field in fields {
        if let Some(existing) = counts.iter_mut().find(|item| item.kind == field.kind) {
            existing.count += 1;
        } else {
            counts.push(FieldKindCount {
                kind: field.kind.clone(),
                count: 1,
            });
        }
    }
    counts
}

fn count_unsupported_field_kinds(fields: &[Field]) -> Vec<FieldKindCount> {
    let mut counts: Vec<FieldKindCount> = Vec::new();
    for field in fields {
        if supports_field_evaluation(field) {
            continue;
        }
        if let Some(existing) = counts.iter_mut().find(|item| item.kind == field.kind) {
            existing.count += 1;
        } else {
            counts.push(FieldKindCount {
                kind: field.kind.clone(),
                count: 1,
            });
        }
    }
    counts
}

fn count_unsupported_field_reasons(fields: &[Field]) -> Vec<FieldEvaluationReasonCount> {
    let mut counts: Vec<FieldEvaluationReasonCount> = Vec::new();
    for field in fields {
        if let Some(reason) = unsupported_field_reason(field) {
            increment_field_evaluation_reason_count(&mut counts, reason);
        }
    }
    counts
}

#[cfg(feature = "docx")]
fn count_docx_unsupported_field_reasons(
    fields: &[Field],
    document_xml: Option<&[u8]>,
) -> Vec<FieldEvaluationReasonCount> {
    let document_xml = document_xml.map(String::from_utf8_lossy);
    let bookmark_names = document_xml.as_deref().map(docx_bookmark_names);
    let note_ref_target_names = document_xml
        .as_deref()
        .map(crate::docx::note_ref_target_names);
    let unsupported_page_ref_section_format_targets = document_xml
        .as_deref()
        .map(docx_page_ref_unsupported_section_format_targets);
    let mut counts: Vec<FieldEvaluationReasonCount> = Vec::new();
    for field in fields {
        if let Some(reason) = unsupported_docx_field_reason(
            field,
            bookmark_names.as_ref(),
            note_ref_target_names.as_ref(),
            unsupported_page_ref_section_format_targets.as_ref(),
        ) {
            increment_field_evaluation_reason_count(&mut counts, reason);
        }
    }
    counts
}

fn increment_field_evaluation_reason_count(
    counts: &mut Vec<FieldEvaluationReasonCount>,
    reason: FieldEvaluationReason,
) {
    if let Some(existing) = counts.iter_mut().find(|item| item.reason == reason) {
        existing.count += 1;
    } else {
        counts.push(FieldEvaluationReasonCount { reason, count: 1 });
    }
}

#[cfg(feature = "docx")]
fn unsupported_docx_field_reason(
    field: &Field,
    bookmark_names: Option<&HashSet<String>>,
    note_ref_target_names: Option<&HashSet<String>>,
    unsupported_page_ref_section_format_targets: Option<&HashSet<String>>,
) -> Option<FieldEvaluationReason> {
    if supports_field_evaluation(field) {
        return None;
    }
    if field.kind == FieldKind::PageRef {
        return Some(docx_page_ref_uncomputed_reason(
            &field.instruction,
            bookmark_names,
            unsupported_page_ref_section_format_targets,
        ));
    }
    if field.kind == FieldKind::Ref {
        return Some(ref_uncomputed_reason(&field.instruction, bookmark_names));
    }
    if field.kind == FieldKind::NoteRef {
        return Some(note_ref_uncomputed_reason(
            &field.instruction,
            bookmark_names,
            note_ref_target_names,
        ));
    }
    if field.kind == FieldKind::Toc {
        return Some(toc_uncomputed_reason(&field.instruction, bookmark_names));
    }
    unsupported_field_reason(field)
}

fn unsupported_field_reason(field: &Field) -> Option<FieldEvaluationReason> {
    if supports_field_evaluation(field) {
        return None;
    }
    match field.kind {
        FieldKind::Unknown(_) => Some(FieldEvaluationReason::UnknownField),
        FieldKind::Page => Some(page_uncomputed_reason(&field.instruction)),
        FieldKind::Ref => Some(ref_uncomputed_reason(&field.instruction, None)),
        FieldKind::Toc => Some(toc_uncomputed_reason(&field.instruction, None)),
        FieldKind::PageRef => Some(page_ref_uncomputed_reason(&field.instruction, None)),
        FieldKind::NoteRef => {
            if supported_note_ref_target(&field.instruction).is_some() {
                Some(FieldEvaluationReason::UnresolvedBookmark)
            } else {
                Some(FieldEvaluationReason::UnsupportedSwitch)
            }
        }
        FieldKind::Dynamic(ref kind) if kind == "=" => {
            Some(formula_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind) if kind.eq_ignore_ascii_case("COMPARE") => {
            Some(compare_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind) if kind.eq_ignore_ascii_case("IF") => {
            Some(if_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind) if kind.eq_ignore_ascii_case("QUOTE") => {
            Some(quote_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind)
            if kind.eq_ignore_ascii_case("FILLIN") || kind.eq_ignore_ascii_case("ASK") =>
        {
            Some(prompt_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind) if kind.eq_ignore_ascii_case("SET") => {
            Some(set_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(ref kind)
            if kind.eq_ignore_ascii_case("NEXT")
                || kind.eq_ignore_ascii_case("NEXTIF")
                || kind.eq_ignore_ascii_case("SKIPIF") =>
        {
            Some(merge_control_uncomputed_reason(&field.instruction))
        }
        FieldKind::Dynamic(_) => Some(FieldEvaluationReason::NoComputedResult),
        FieldKind::InsertedContent(_) => {
            Some(inserted_content_uncomputed_reason(&field.instruction))
        }
        FieldKind::MailMerge(_) => Some(mail_merge_uncomputed_reason(&field.instruction)),
        FieldKind::ReferenceIndex(ref kind) if is_reference_index_marker_kind(kind.as_str()) => {
            Some(reference_index_marker_uncomputed_reason(&field.instruction))
        }
        FieldKind::ReferenceIndex(_) => Some(reference_index_uncomputed_reason(&field.instruction)),
        FieldKind::Numbering(_) => Some(numbering_uncomputed_reason(&field.instruction)),
        FieldKind::DocumentStructure(ref kind)
            if is_section_document_structure_kind(kind.as_str()) =>
        {
            Some(section_document_structure_uncomputed_reason(
                &field.instruction,
            ))
        }
        FieldKind::DocumentStructure(ref kind) if kind.eq_ignore_ascii_case("REVNUM") => {
            Some(revision_number_uncomputed_reason(&field.instruction))
        }
        FieldKind::DocumentStructure(ref kind) if kind.eq_ignore_ascii_case("STYLEREF") => {
            Some(style_ref_uncomputed_reason(&field.instruction))
        }
        FieldKind::DocumentStructure(_) => Some(FieldEvaluationReason::NoComputedResult),
        FieldKind::Display(_) => Some(display_uncomputed_reason(&field.instruction)),
        FieldKind::Action(_) => Some(action_uncomputed_reason(&field.instruction)),
        FieldKind::Compatibility(_) => Some(compatibility_uncomputed_reason(&field.instruction)),
        FieldKind::Barcode(_) => Some(barcode_uncomputed_reason(&field.instruction)),
        FieldKind::FormField(_) => Some(form_field_uncomputed_reason(&field.instruction)),
        FieldKind::Filename => Some(filename_uncomputed_reason(&field.instruction)),
        FieldKind::Hyperlink => Some(FieldEvaluationReason::UnsupportedSwitch),
        FieldKind::MergeField => Some(FieldEvaluationReason::UnsupportedSwitch),
        FieldKind::DocumentInfo(_) => Some(FieldEvaluationReason::UnsupportedSwitch),
        FieldKind::Sequence => Some(sequence_uncomputed_reason(&field.instruction)),
        FieldKind::TocEntry => Some(toc_entry_uncomputed_reason(&field.instruction)),
    }
}

fn filename_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_filename_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

#[cfg(feature = "docx")]
fn supported_filename_syntax(instruction: &str) -> bool {
    crate::docx::supports_filename_field_syntax(instruction)
}

#[cfg(not(feature = "docx"))]
fn supported_filename_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("FILENAME") {
        return false;
    }
    let mut path = false;
    let mut text_format = false;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\p") {
            if path {
                return false;
            }
            path = true;
            continue;
        }
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        return false;
    }
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RefDiagnosticSyntax {
    target: String,
    note_reference: bool,
    sequence_separator: bool,
}

fn ref_uncomputed_reason(
    instruction: &str,
    bookmark_names: Option<&HashSet<String>>,
) -> FieldEvaluationReason {
    let Some(syntax) =
        supported_ref_syntax(instruction).or_else(|| supported_direct_ref_syntax(instruction))
    else {
        return FieldEvaluationReason::UnsupportedSwitch;
    };
    if bookmark_names.is_some_and(|names| !names.contains(&syntax.target)) {
        return FieldEvaluationReason::UnresolvedBookmark;
    }
    if syntax.sequence_separator {
        FieldEvaluationReason::NoComputedResult
    } else if syntax.note_reference {
        FieldEvaluationReason::UnsupportedSwitch
    } else {
        FieldEvaluationReason::UnresolvedBookmark
    }
}

fn supported_ref_syntax(instruction: &str) -> Option<RefDiagnosticSyntax> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("REF") {
        return None;
    }
    supported_ref_syntax_parts(parts)
}

fn supported_direct_ref_syntax(instruction: &str) -> Option<RefDiagnosticSyntax> {
    let tokens = instruction_parts(instruction);
    let first = tokens.first()?;
    if first.eq_ignore_ascii_case("REF") {
        return None;
    }
    supported_ref_syntax_parts(tokens.iter().map(String::as_str))
}

fn page_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_page_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

#[cfg(feature = "docx")]
fn supported_page_syntax(instruction: &str) -> bool {
    crate::docx::supports_page_field_syntax(instruction)
}

#[cfg(not(feature = "docx"))]
fn supported_page_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("PAGE") {
        return false;
    }
    let mut number_format = false;
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_page_field_format_switch(format, &mut number_format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        return false;
    }
    true
}

fn is_section_document_structure_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("SECTION") || kind.eq_ignore_ascii_case("SECTIONPAGES")
}

fn section_document_structure_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_section_document_structure_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

#[cfg(feature = "docx")]
fn supported_section_document_structure_syntax(instruction: &str) -> bool {
    crate::docx::supports_section_field_syntax(instruction)
}

#[cfg(not(feature = "docx"))]
fn supported_section_document_structure_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !is_section_document_structure_kind(kind) {
        return false;
    }
    let mut number_format = false;
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_page_field_format_switch(format, &mut number_format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        return false;
    }
    true
}

fn revision_number_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_revision_number_field_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_revision_number_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_revision_number_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("REVNUM") {
        return false;
    }
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if !accepted {
            return false;
        }
    }
    true
}

fn style_ref_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_style_ref_field_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_style_ref_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_style_ref_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("STYLEREF") {
        return false;
    }
    let Some(style_identifier) = parts.next() else {
        return false;
    };
    if diagnostic_name_token(style_identifier).is_none() {
        return false;
    }
    let mut text_format = false;
    let mut result = 0u8;
    let mut suppress_non_numeric = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if part.eq_ignore_ascii_case("\\t") {
            if suppress_non_numeric {
                return false;
            }
            suppress_non_numeric = true;
            continue;
        }
        let next_result = if part.eq_ignore_ascii_case("\\n") {
            1
        } else if part.eq_ignore_ascii_case("\\r") {
            2
        } else if part.eq_ignore_ascii_case("\\w") {
            3
        } else if part.eq_ignore_ascii_case("\\p") {
            4
        } else {
            return false;
        };
        if result != 0 {
            return false;
        }
        result = next_result;
    }
    !suppress_non_numeric || matches!(result, 1..=3)
}

fn display_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_display_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_advance_syntax(instruction)
            || supported_symbol_syntax(instruction)
            || supported_eq_displacement_syntax(instruction)
            || supported_eq_script_syntax(instruction)
            || supported_eq_fraction_syntax(instruction)
            || supported_eq_radical_syntax(instruction)
        {
            return FieldEvaluationReason::NoComputedResult;
        }
    }
    FieldEvaluationReason::UnsupportedSwitch
}

#[cfg(not(feature = "docx"))]
fn supported_advance_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("ADVANCE") {
        return false;
    }
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if accept_advance_switch_for_report(part, &mut parts).is_none() {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn accept_advance_switch_for_report<'a>(
    part: &str,
    parts: &mut impl Iterator<Item = &'a str>,
) -> Option<()> {
    for switch in ["\\d", "\\u", "\\l", "\\r", "\\x", "\\y"] {
        if part.eq_ignore_ascii_case(switch) {
            parse_advance_points_for_report(parts.next()?)?;
            return Some(());
        }
        if let Some(value) = strip_ascii_switch_prefix(part, switch) {
            if value.is_empty() {
                return None;
            }
            parse_advance_points_for_report(value)?;
            return Some(());
        }
    }
    None
}

#[cfg(not(feature = "docx"))]
fn parse_advance_points_for_report(value: &str) -> Option<f32> {
    diagnostic_name_token(value)?
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite())
}

#[cfg(not(feature = "docx"))]
fn supported_symbol_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("SYMBOL") {
        return false;
    }
    if parse_symbol_code_for_report(parts.next()).is_none() {
        return false;
    }
    let mut text_format = false;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\a") || part.eq_ignore_ascii_case("\\h") {
            continue;
        }
        if part.eq_ignore_ascii_case("\\u") {
            continue;
        }
        if part.eq_ignore_ascii_case("\\j") {
            return false;
        }
        if part.eq_ignore_ascii_case("\\f") {
            let Some(font) = parts.next() else {
                return false;
            };
            if diagnostic_name_token(font).is_none() {
                return false;
            }
            continue;
        }
        if let Some(font) = strip_ascii_switch_prefix(part, "\\f") {
            if font.is_empty() || diagnostic_name_token(font).is_none() {
                return false;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            let Some(size) = parts.next() else {
                return false;
            };
            if parse_symbol_size_for_report(size).is_none() {
                return false;
            }
            continue;
        }
        if let Some(size) = strip_ascii_switch_prefix(part, "\\s") {
            if size.is_empty() || parse_symbol_size_for_report(size).is_none() {
                return false;
            }
            continue;
        }
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if !accepted {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn parse_symbol_code_for_report(token: Option<&str>) -> Option<u32> {
    let token = diagnostic_name_token(token?)?;
    if let Some(hex) = token
        .strip_prefix("0x")
        .or_else(|| token.strip_prefix("0X"))
    {
        return u32::from_str_radix(hex, 16).ok();
    }
    if let Ok(code) = token.parse::<u32>() {
        return Some(code);
    }
    let mut chars = token.chars();
    let ch = chars.next()?;
    chars.next().is_none().then_some(ch as u32)
}

#[cfg(not(feature = "docx"))]
fn parse_symbol_size_for_report(token: &str) -> Option<f32> {
    diagnostic_name_token(token)?
        .parse::<f32>()
        .ok()
        .filter(|value| value.is_finite() && *value > 0.0)
}

#[cfg(not(feature = "docx"))]
fn supported_eq_displacement_syntax(instruction: &str) -> bool {
    let Some(expression) = eq_expression_for_report(instruction) else {
        return false;
    };
    let Some(mut body) = strip_ascii_switch_prefix(expression.trim_start(), "\\d") else {
        return false;
    };
    body = body.trim_start();
    let mut has_option = false;
    loop {
        if let Some(rest) = consume_eq_numeric_prefix_option_for_report(body, "\\fo")
            .or_else(|| consume_eq_numeric_prefix_option_for_report(body, "\\ba"))
        {
            has_option = true;
            body = rest.trim_start();
            continue;
        }
        if let Some(rest) = consume_eq_prefix_switch_for_report(body, "\\li") {
            has_option = true;
            body = rest.trim_start();
            continue;
        }
        break;
    }
    let Some(inner) = body
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
    else {
        return false;
    };
    has_option
        && (inner.trim().is_empty()
            || diagnostic_literal_token(inner).is_some_and(|text| !text.trim().is_empty()))
}

#[cfg(not(feature = "docx"))]
fn consume_eq_numeric_prefix_option_for_report<'a>(
    value: &'a str,
    option: &str,
) -> Option<&'a str> {
    let rest = strip_ascii_switch_prefix(value, option)?;
    if matches!(
        rest.chars().next(),
        Some(ch) if ch.is_ascii_alphabetic()
    ) {
        return None;
    }
    let rest = rest.trim_start();
    let mut end = 0usize;
    for (index, ch) in rest.char_indices() {
        if index == 0 && (ch == '-' || ch == '+') {
            end = ch.len_utf8();
            continue;
        }
        if !ch.is_ascii_digit() && ch != '.' && ch != 'e' && ch != 'E' && ch != '-' && ch != '+' {
            break;
        }
        end = index + ch.len_utf8();
    }
    if end == 0 || matches!(rest.get(..end), Some("+") | Some("-")) {
        return None;
    }
    parse_advance_points_for_report(&rest[..end])?;
    Some(&rest[end..])
}

#[cfg(not(feature = "docx"))]
fn consume_eq_prefix_switch_for_report<'a>(value: &'a str, switch: &str) -> Option<&'a str> {
    let rest = strip_ascii_switch_prefix(value, switch)?;
    if matches!(
        rest.chars().next(),
        Some(ch) if ch.is_ascii_alphabetic()
    ) {
        return None;
    }
    Some(rest)
}

#[cfg(not(feature = "docx"))]
fn supported_eq_script_syntax(instruction: &str) -> bool {
    let Some(expression) = eq_expression_for_report(instruction) else {
        return false;
    };
    let mut body = expression.trim_start();
    loop {
        let Some(rest) = strip_ascii_switch_prefix(body, "\\s") else {
            return false;
        };
        let Some(remaining) = eq_script_syntax_segment_for_report(rest.trim_start()) else {
            return false;
        };
        body = remaining.trim_start();
        if body.is_empty() {
            return true;
        }
    }
}

#[cfg(not(feature = "docx"))]
fn eq_expression_for_report(instruction: &str) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("EQ") {
        return None;
    }
    let mut expression_parts = Vec::new();
    let mut text_format = false;
    while let Some(part) = parts.next() {
        if accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        })? {
            continue;
        }
        expression_parts.push(part);
    }
    (!expression_parts.is_empty()).then(|| expression_parts.join(" "))
}

#[cfg(not(feature = "docx"))]
fn eq_script_syntax_segment_for_report(mut body: &str) -> Option<&str> {
    let mut saw_option = false;
    loop {
        if body.is_empty() || consume_eq_prefix_switch_for_report(body, "\\s").is_some() {
            return saw_option.then_some(body);
        }
        if let Some(rest) = consume_eq_script_option_for_report(body, "\\up", false)
            .or_else(|| consume_eq_script_option_for_report(body, "\\do", false))
            .or_else(|| consume_eq_script_option_for_report(body, "\\ai", true))
            .or_else(|| consume_eq_script_option_for_report(body, "\\di", true))
        {
            body = rest.trim_start();
            saw_option = true;
            continue;
        }
        return None;
    }
}

#[cfg(not(feature = "docx"))]
fn consume_eq_script_option_for_report<'a>(
    value: &'a str,
    option: &str,
    allow_empty: bool,
) -> Option<&'a str> {
    let rest = consume_eq_numeric_prefix_option_for_report(value, option)?;
    let (operand, rest) = take_eq_parenthesized_operand_for_report(rest)?;
    if operand.trim().is_empty() {
        return allow_empty.then_some(rest);
    }
    diagnostic_literal_token(operand)
        .is_some_and(|text| !text.trim().is_empty())
        .then_some(rest)
}

#[cfg(not(feature = "docx"))]
fn take_eq_parenthesized_operand_for_report(value: &str) -> Option<(&str, &str)> {
    let value = value.trim_start();
    let rest = value.strip_prefix('(')?;
    let end = rest.find(')')?;
    Some((&rest[..end], &rest[end + 1..]))
}

#[cfg(not(feature = "docx"))]
fn supported_eq_fraction_syntax(instruction: &str) -> bool {
    let Some(expression) = eq_expression_for_report(instruction) else {
        return false;
    };
    let Some(body) = strip_ascii_switch_prefix(expression.trim_start(), "\\f") else {
        return false;
    };
    let Some(inner) = body
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
    else {
        return false;
    };
    let Some((numerator, denominator)) = split_eq_fraction_operands_for_report(inner) else {
        return false;
    };
    eq_operand_for_report(numerator) && eq_operand_for_report(denominator)
}

#[cfg(not(feature = "docx"))]
fn split_eq_fraction_operands_for_report(inner: &str) -> Option<(&str, &str)> {
    let mut depth = 0usize;
    let mut separator = None;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            ',' | ';' if !in_quotes && depth == 0 && separator.replace(index).is_some() => {
                return None;
            }
            _ => {}
        }
    }
    if in_quotes || escaped || depth != 0 {
        return None;
    }
    let index = separator?;
    Some((&inner[..index], &inner[index + 1..]))
}

#[cfg(not(feature = "docx"))]
fn eq_operand_for_report(operand: &str) -> bool {
    let operand = operand.trim();
    if operand.is_empty() {
        return false;
    }
    let Some(text) = diagnostic_literal_token(operand) else {
        return false;
    };
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            continue;
        }
        if !matches!(chars.next(), Some(',' | ';' | '(' | ')' | '\\')) {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn supported_eq_radical_syntax(instruction: &str) -> bool {
    let Some(expression) = eq_expression_for_report(instruction) else {
        return false;
    };
    let Some(body) = strip_ascii_switch_prefix(expression.trim_start(), "\\r") else {
        return false;
    };
    let Some(inner) = body
        .strip_prefix('(')
        .and_then(|body| body.strip_suffix(')'))
    else {
        return false;
    };
    let Some((degree, radicand)) = split_eq_radical_operands_for_report(inner) else {
        return false;
    };
    eq_operand_for_report(degree)
        && match radicand {
            Some(radicand) => eq_operand_for_report(radicand),
            None => true,
        }
}

#[cfg(not(feature = "docx"))]
fn split_eq_radical_operands_for_report(inner: &str) -> Option<(&str, Option<&str>)> {
    let mut depth = 0usize;
    let mut separator = None;
    let mut in_quotes = false;
    let mut escaped = false;
    for (index, ch) in inner.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => in_quotes = !in_quotes,
            '(' if !in_quotes => depth += 1,
            ')' if !in_quotes => depth = depth.checked_sub(1)?,
            ',' | ';' if !in_quotes && depth == 0 && separator.replace(index).is_some() => {
                return None;
            }
            _ => {}
        }
    }
    if in_quotes || escaped || depth != 0 {
        return None;
    }
    match separator {
        Some(index) => Some((&inner[..index], Some(&inner[index + 1..]))),
        None => Some((inner, None)),
    }
}

fn action_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_action_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_action_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
    }
    FieldEvaluationReason::UnsupportedSwitch
}

#[cfg(not(feature = "docx"))]
fn supported_action_syntax(instruction: &str) -> bool {
    supported_print_syntax(instruction) || supported_action_button_syntax(instruction)
}

#[cfg(not(feature = "docx"))]
fn supported_print_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("PRINT") {
        return false;
    }
    let Some(first) = parts.next() else {
        return false;
    };
    if first.eq_ignore_ascii_case("\\p") {
        let Some(group) = parts.next() else {
            return false;
        };
        let mut text_format = false;
        return diagnostic_identifier_token(group).is_some()
            && quoted_action_text_for_report(parts.next()).is_some()
            && supported_field_format_tail_for_report(&mut parts, &mut text_format);
    }
    if let Some(group) = strip_ascii_switch_prefix(first, "\\p") {
        let mut text_format = false;
        return diagnostic_identifier_token(group).is_some()
            && quoted_action_text_for_report(parts.next()).is_some()
            && supported_field_format_tail_for_report(&mut parts, &mut text_format);
    }
    if action_text_for_report(first).is_none() {
        return false;
    }
    let mut text_format = false;
    let mut saw_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            saw_format = true;
            continue;
        }
        if saw_format || action_text_for_report(part).is_none() {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn supported_action_button_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("GOTOBUTTON") && !kind.eq_ignore_ascii_case("MACROBUTTON") {
        return false;
    }
    let Some(target) = parts.next() else {
        return false;
    };
    if diagnostic_identifier_token(target).is_none() {
        return false;
    }
    let mut display_parts = Vec::new();
    let mut text_format = false;
    let mut saw_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            saw_format = true;
            continue;
        }
        if saw_format || part.starts_with('\\') {
            return false;
        }
        display_parts.push(part);
    }
    display_parts.is_empty() || action_text_for_report(&display_parts.join(" ")).is_some()
}

#[cfg(not(feature = "docx"))]
fn action_text_for_report(token: &str) -> Option<&str> {
    let text = diagnostic_literal_token(token)?;
    (!text.is_empty() && !text.starts_with('\\')).then_some(text)
}

#[cfg(not(feature = "docx"))]
fn quoted_action_text_for_report(token: Option<&str>) -> Option<&str> {
    let token = token?;
    token
        .starts_with('"')
        .then(|| action_text_for_report(token))?
}

fn inserted_content_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_inserted_content_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_inserted_content_syntax(instruction: &str) -> bool {
    supported_opaque_field_syntax(instruction, is_inserted_content_kind)
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

fn mail_merge_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_mail_merge_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_mail_merge_syntax(instruction: &str) -> bool {
    supported_opaque_field_syntax(instruction, is_mail_merge_kind)
}

fn is_mail_merge_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "ADDRESSBLOCK" | "GREETINGLINE" | "MERGEREC" | "MERGESEQ"
    )
}

fn reference_index_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_reference_index_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_reference_index_syntax(instruction: &str) -> bool {
    supported_opaque_field_syntax(instruction, is_generated_reference_index_kind)
}

fn is_generated_reference_index_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "BIBLIOGRAPHY" | "CITATION" | "INDEX" | "TOA"
    )
}

fn compatibility_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_compatibility_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_compatibility_syntax(instruction: &str) -> bool {
    supported_opaque_field_syntax(instruction, is_compatibility_kind)
}

fn is_compatibility_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "ADDIN" | "DATA" | "GLOSSARY" | "HTMLACTIVEX" | "PRIVATE"
    )
}

fn barcode_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_barcode_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_barcode_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    let required_value_tokens = if kind.eq_ignore_ascii_case("BARCODE") {
        1
    } else if kind.eq_ignore_ascii_case("DISPLAYBARCODE")
        || kind.eq_ignore_ascii_case("MERGEBARCODE")
    {
        2
    } else {
        return false;
    };
    let mut value_tokens = 0usize;
    let mut positional_values = true;
    for part in parts {
        if !diagnostic_field_token_well_formed(part) {
            return false;
        }
        if part.starts_with('\\') {
            positional_values = false;
            continue;
        }
        if positional_values {
            value_tokens += 1;
        }
    }
    value_tokens >= required_value_tokens
}

fn form_field_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    if supported_form_field_syntax(instruction) {
        FieldEvaluationReason::NoComputedResult
    } else {
        FieldEvaluationReason::UnsupportedSwitch
    }
}

fn supported_form_field_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !is_form_field_kind(kind) {
        return false;
    }
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if !accepted {
            return false;
        }
    }
    true
}

fn is_form_field_kind(kind: &str) -> bool {
    matches!(
        kind.to_ascii_uppercase().as_str(),
        "FORMCHECKBOX" | "FORMDROPDOWN" | "FORMTEXT"
    )
}

fn supported_opaque_field_syntax(instruction: &str, is_kind: fn(&str) -> bool) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !is_kind(kind) {
        return false;
    }
    parts.all(diagnostic_field_token_well_formed)
}

fn diagnostic_field_token_well_formed(part: &str) -> bool {
    if part.starts_with('\\') {
        !part.contains('"')
    } else {
        diagnostic_literal_token(part).is_some()
    }
}

fn is_reference_index_marker_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("RD")
        || kind.eq_ignore_ascii_case("TA")
        || kind.eq_ignore_ascii_case("XE")
}

fn reference_index_marker_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_reference_index_marker_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
    }
    #[cfg(not(feature = "docx"))]
    let _ = instruction;
    FieldEvaluationReason::UnsupportedSwitch
}

fn toc_entry_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_toc_entry_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_toc_entry_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_toc_entry_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str).peekable();
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("TC")
        || diagnostic_name_token(parts.next().unwrap_or("")).is_none()
    {
        return false;
    }
    let mut entry_type = false;
    let mut level = false;
    while let Some(part) = parts.next() {
        if part.eq_ignore_ascii_case("\\f") {
            let Some(value) = parts.next_if(|next| !next.starts_with('\\')) else {
                return false;
            };
            if entry_type || diagnostic_identifier_token(value).is_none() {
                return false;
            }
            entry_type = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f") {
            if value.is_empty() || entry_type || diagnostic_identifier_token(value).is_none() {
                return false;
            }
            entry_type = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let Some(value) = parts.next_if(|next| !next.starts_with('\\')) else {
                return false;
            };
            if level || parse_toc_level_for_report(value).is_none() {
                return false;
            }
            level = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\l") {
            if value.is_empty() || level || parse_toc_level_for_report(value).is_none() {
                return false;
            }
            level = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") {
            continue;
        }
        return false;
    }
    true
}

fn numbering_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_numbering_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_numbering_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_numbering_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if kind.eq_ignore_ascii_case("AUTONUM")
        || kind.eq_ignore_ascii_case("AUTONUMLGL")
        || kind.eq_ignore_ascii_case("AUTONUMOUT")
    {
        return supported_autonum_syntax(kind, parts);
    }
    if kind.eq_ignore_ascii_case("LISTNUM") {
        return supported_listnum_syntax(parts);
    }
    kind.eq_ignore_ascii_case("BIDIOUTLINE") && parts.next().is_none()
}

#[cfg(not(feature = "docx"))]
fn supported_autonum_syntax<'a>(kind: &str, mut parts: impl Iterator<Item = &'a str>) -> bool {
    let accepts_separator = kind.eq_ignore_ascii_case("AUTONUM");
    let mut number_format = false;
    let mut separator = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_page_number_format_switch(format, &mut number_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if accepts_separator && part.eq_ignore_ascii_case("\\s") {
            let Some(value) = parts.next() else {
                return false;
            };
            if !accept_autonum_separator_for_report(value, &mut separator) {
                return false;
            }
            continue;
        }
        if accepts_separator {
            if let Some(value) = strip_ascii_switch_prefix(part, "\\s") {
                if !accept_autonum_separator_for_report(value, &mut separator) {
                    return false;
                }
                continue;
            }
        }
        return false;
    }
    true
}

#[cfg(not(feature = "docx"))]
fn supported_listnum_syntax<'a>(mut parts: impl Iterator<Item = &'a str>) -> bool {
    let mut list_name_seen = false;
    let mut level_seen = false;
    let mut reset_seen = false;
    let mut number_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_page_number_format_switch(format, &mut number_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let Some(level) = parts.next() else {
                return false;
            };
            if !accept_listnum_level_for_report(level, &mut level_seen) {
                return false;
            }
            continue;
        }
        if let Some(level) = strip_ascii_switch_prefix(part, "\\l") {
            if level.is_empty() || !accept_listnum_level_for_report(level, &mut level_seen) {
                return false;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            let Some(start) = parts.next() else {
                return false;
            };
            if !accept_listnum_start_for_report(start, &mut reset_seen) {
                return false;
            }
            continue;
        }
        if let Some(start) = strip_ascii_switch_prefix(part, "\\s") {
            if start.is_empty() || !accept_listnum_start_for_report(start, &mut reset_seen) {
                return false;
            }
            continue;
        }
        if part.starts_with('\\') || list_name_seen || diagnostic_name_token(part).is_none() {
            return false;
        }
        list_name_seen = true;
    }
    true
}

#[cfg(not(feature = "docx"))]
fn accept_listnum_level_for_report(part: &str, level_seen: &mut bool) -> bool {
    if *level_seen {
        return false;
    }
    let Some(level) = diagnostic_name_token(part).and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    if level == 0 {
        return false;
    }
    *level_seen = true;
    true
}

#[cfg(not(feature = "docx"))]
fn accept_listnum_start_for_report(part: &str, reset_seen: &mut bool) -> bool {
    if *reset_seen {
        return false;
    }
    let Some(start) = diagnostic_name_token(part).and_then(|part| part.parse::<i64>().ok()) else {
        return false;
    };
    if start < 0 {
        return false;
    }
    *reset_seen = true;
    true
}

#[cfg(not(feature = "docx"))]
fn accept_autonum_separator_for_report(part: &str, separator: &mut bool) -> bool {
    if *separator {
        return false;
    }
    let Some(value) = diagnostic_literal_token(part) else {
        return false;
    };
    let mut chars = value.chars();
    let Some(_) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }
    *separator = true;
    true
}

fn compare_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_compare_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_compare_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_compare_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("COMPARE") {
        return false;
    }
    let Some(first) = parts.next() else {
        return false;
    };
    if !supported_comparison_operands(first, &mut parts) {
        return false;
    }
    let mut text_format = false;
    supported_field_format_tail_for_report(&mut parts, &mut text_format)
}

#[cfg(not(feature = "docx"))]
fn supported_field_format_tail_for_report<'a>(
    parts: &mut impl Iterator<Item = &'a str>,
    text_format: &mut bool,
) -> bool {
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, parts, |format| {
            accept_field_format_switch(format, text_format)
        }) else {
            return false;
        };
        if !accepted {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn supported_comparison_operands<'a>(
    first: &str,
    parts: &mut impl Iterator<Item = &'a str>,
) -> bool {
    if let Some((left, operator, right)) = compact_comparison_operands(first) {
        return comparison_operand_for_report(left)
            && comparison_operator_for_report(operator)
            && comparison_operand_for_report(right);
    }
    let Some(operator) = parts.next() else {
        return false;
    };
    let Some(right) = parts.next() else {
        return false;
    };
    comparison_operand_for_report(first)
        && comparison_operator_for_report(operator)
        && comparison_operand_for_report(right)
}

#[cfg(not(feature = "docx"))]
fn compact_comparison_operands(token: &str) -> Option<(&str, &str, &str)> {
    for operator in [">=", "<=", "<>", "=", ">", "<"] {
        let Some(index) = find_unquoted_operator_for_report(token, operator) else {
            continue;
        };
        let (left, right_with_operator) = token.split_at(index);
        let right = &right_with_operator[operator.len()..];
        if left.is_empty() || right.is_empty() {
            return None;
        }
        return Some((left, operator, right));
    }
    None
}

#[cfg(not(feature = "docx"))]
fn find_unquoted_operator_for_report(token: &str, operator: &str) -> Option<usize> {
    let mut in_quotes = false;
    for (index, ch) in token.char_indices() {
        if ch == '"' {
            in_quotes = !in_quotes;
        } else if !in_quotes && token[index..].starts_with(operator) {
            return Some(index);
        }
    }
    None
}

#[cfg(not(feature = "docx"))]
fn comparison_operator_for_report(token: &str) -> bool {
    matches!(token, "=" | "<>" | ">" | "<" | ">=" | "<=")
}

#[cfg(not(feature = "docx"))]
fn comparison_operand_for_report(token: &str) -> bool {
    if token.parse::<f64>().is_ok_and(f64::is_finite) {
        return true;
    }
    diagnostic_literal_token(token)
        .is_some_and(|value| !value.is_empty() && !value.starts_with('\\'))
}

fn formula_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_formula_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_formula_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_formula_syntax(instruction: &str) -> bool {
    let Some(body) = instruction.trim().strip_prefix('=') else {
        return false;
    };
    let body = body.trim();
    if body.is_empty() {
        return false;
    }
    let tokens = instruction_parts(body);
    let Some(format_index) = tokens.iter().position(|part| {
        part == "\\#"
            || strip_ascii_switch_prefix(part, "\\#").is_some_and(|picture| !picture.is_empty())
    }) else {
        if let Some(tail_index) = tokens
            .iter()
            .position(|part| part == "\\*" || part.starts_with("\\*"))
        {
            if tail_index == 0 {
                return false;
            }
            let mut tail = tokens[tail_index..].iter().map(String::as_str);
            let mut text_format = false;
            return supported_field_format_tail_for_report(&mut tail, &mut text_format);
        }
        return true;
    };
    if format_index == 0 {
        return false;
    }
    let (picture, tail_start) = if tokens[format_index] == "\\#" {
        let Some(picture) = tokens.get(format_index + 1) else {
            return false;
        };
        (picture.as_str(), format_index + 2)
    } else if let Some(picture) = strip_ascii_switch_prefix(&tokens[format_index], "\\#") {
        (picture, format_index + 1)
    } else {
        return false;
    };
    let valid_picture = if picture.starts_with('"') {
        diagnostic_literal_token(picture).is_some()
    } else {
        diagnostic_literal_token(picture).is_some_and(|picture| !picture.starts_with('\\'))
    };
    if !valid_picture {
        return false;
    }
    let mut tail = tokens[tail_start..].iter().map(String::as_str);
    let mut text_format = false;
    supported_field_format_tail_for_report(&mut tail, &mut text_format)
}

fn sequence_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_sequence_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_sequence_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_sequence_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("SEQ")
        || diagnostic_identifier_token(parts.next().unwrap_or("")).is_none()
    {
        return false;
    }
    let mut action_seen = false;
    let mut hidden = false;
    let mut number_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_page_number_format_switch(format, &mut number_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") || part.eq_ignore_ascii_case("\\c") {
            if action_seen {
                return false;
            }
            action_seen = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\h") {
            if hidden {
                return false;
            }
            hidden = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\r") {
            let Some(reset) = parts.next() else {
                return false;
            };
            if !accept_sequence_reset_for_report(reset, &mut action_seen) {
                return false;
            }
            continue;
        }
        if let Some(reset) = strip_ascii_switch_prefix(part, "\\r") {
            if reset.is_empty() || !accept_sequence_reset_for_report(reset, &mut action_seen) {
                return false;
            }
            continue;
        }
        return false;
    }
    true
}

#[cfg(not(feature = "docx"))]
fn accept_sequence_reset_for_report(part: &str, action_seen: &mut bool) -> bool {
    if *action_seen {
        return false;
    }
    if diagnostic_name_token(part)
        .and_then(|part| part.parse::<i64>().ok())
        .is_none()
    {
        return false;
    }
    *action_seen = true;
    true
}

fn if_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_if_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_if_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_if_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("IF") {
        return false;
    }
    let Some(first) = parts.next() else {
        return false;
    };
    if !supported_comparison_operands(first, &mut parts) {
        return false;
    }
    let Some(true_text) = parts.next() else {
        return false;
    };
    if if_result_text_for_report(true_text).is_none() {
        return false;
    }
    let mut text_format = false;
    if let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if !accepted && if_result_text_for_report(part).is_none() {
            return false;
        }
    }
    supported_field_format_tail_for_report(&mut parts, &mut text_format)
}

#[cfg(not(feature = "docx"))]
fn if_result_text_for_report(token: &str) -> Option<&str> {
    (!token.starts_with('\\'))
        .then(|| diagnostic_literal_token(token))
        .flatten()
}

fn quote_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_quote_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_quote_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_quote_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("QUOTE") {
        return false;
    }
    let mut text_parts = Vec::new();
    let mut text_format = false;
    let mut saw_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            saw_format = true;
            continue;
        }
        if saw_format || part.starts_with('\\') {
            return false;
        }
        text_parts.push(part);
    }
    diagnostic_literal_token(&text_parts.join(" ")).is_some_and(|text| !text.is_empty())
}

fn prompt_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_prompt_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_prompt_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_prompt_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if kind.eq_ignore_ascii_case("FILLIN") {
        return supported_fillin_syntax(parts);
    }
    if kind.eq_ignore_ascii_case("ASK") {
        return supported_ask_syntax(parts);
    }
    false
}

#[cfg(not(feature = "docx"))]
fn supported_fillin_syntax<'a>(mut parts: impl Iterator<Item = &'a str>) -> bool {
    let mut default = false;
    let mut text_format = false;
    let mut ask_once = false;
    let mut prompt_seen = false;
    while let Some(part) = parts.next() {
        if accept_prompt_default_switch(part, &mut parts, &mut default) {
            continue;
        }
        if part.eq_ignore_ascii_case("\\o") {
            if ask_once {
                return false;
            }
            ask_once = true;
            continue;
        }
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if prompt_seen || prompt_text_token_for_report(part).is_none() {
            return false;
        }
        prompt_seen = true;
    }
    true
}

#[cfg(not(feature = "docx"))]
fn supported_ask_syntax<'a>(mut parts: impl Iterator<Item = &'a str>) -> bool {
    if diagnostic_identifier_token(parts.next().unwrap_or("")).is_none() {
        return false;
    }
    let Some(prompt) = parts.next() else {
        return false;
    };
    if prompt_text_token_for_report(prompt).is_none() {
        return false;
    }
    let mut default = false;
    let mut text_format = false;
    let mut ask_once = false;
    while let Some(part) = parts.next() {
        if accept_prompt_default_switch(part, &mut parts, &mut default) {
            continue;
        }
        if part.eq_ignore_ascii_case("\\o") {
            if ask_once {
                return false;
            }
            ask_once = true;
            continue;
        }
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if !accepted {
            return false;
        }
    }
    true
}

#[cfg(not(feature = "docx"))]
fn accept_prompt_default_switch<'a>(
    part: &str,
    parts: &mut impl Iterator<Item = &'a str>,
    default: &mut bool,
) -> bool {
    let value = if part.eq_ignore_ascii_case("\\d") {
        parts.next().and_then(prompt_default_token_for_report)
    } else {
        strip_ascii_switch_prefix(part, "\\d").and_then(prompt_default_token_for_report)
    };
    value.is_some() && !std::mem::replace(default, true)
}

#[cfg(not(feature = "docx"))]
fn prompt_default_token_for_report(token: &str) -> Option<&str> {
    let value = diagnostic_literal_token(token)?;
    (!value.starts_with('\\')).then_some(value)
}

#[cfg(not(feature = "docx"))]
fn prompt_text_token_for_report(token: &str) -> Option<&str> {
    let value = prompt_default_token_for_report(token)?;
    (!value.is_empty()).then_some(value)
}

fn set_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_set_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_set_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_set_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    if !kind.eq_ignore_ascii_case("SET") {
        return false;
    }
    if diagnostic_identifier_token(parts.next().unwrap_or("")).is_none() {
        return false;
    }
    let Some(value) = parts.next() else {
        return false;
    };
    let quoted_value = value.trim().starts_with('"');
    if quoted_value {
        if diagnostic_literal_token(value).is_none() {
            return false;
        }
    } else if value.is_empty() || value.starts_with('\\') || value.contains('"') {
        return false;
    }
    let mut text_format = false;
    while let Some(part) = parts.next() {
        let Some(accepted) = accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        }) else {
            return false;
        };
        if accepted {
            continue;
        }
        if quoted_value || part.starts_with('\\') || part.contains('"') {
            return false;
        }
    }
    true
}

fn merge_control_uncomputed_reason(instruction: &str) -> FieldEvaluationReason {
    #[cfg(feature = "docx")]
    {
        if crate::docx::supports_merge_control_field_syntax(instruction) {
            return FieldEvaluationReason::NoComputedResult;
        }
        FieldEvaluationReason::UnsupportedSwitch
    }
    #[cfg(not(feature = "docx"))]
    {
        if supported_merge_control_syntax(instruction) {
            FieldEvaluationReason::NoComputedResult
        } else {
            FieldEvaluationReason::UnsupportedSwitch
        }
    }
}

#[cfg(not(feature = "docx"))]
fn supported_merge_control_syntax(instruction: &str) -> bool {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let Some(kind) = parts.next() else {
        return false;
    };
    let mut text_format = false;
    if kind.eq_ignore_ascii_case("NEXT") {
        return supported_field_format_tail_for_report(&mut parts, &mut text_format);
    }
    if kind.eq_ignore_ascii_case("NEXTIF") || kind.eq_ignore_ascii_case("SKIPIF") {
        let Some(first) = parts.next() else {
            return false;
        };
        return supported_comparison_operands(first, &mut parts)
            && supported_field_format_tail_for_report(&mut parts, &mut text_format);
    }
    false
}

fn supported_ref_syntax_parts<'a>(
    mut parts: impl Iterator<Item = &'a str>,
) -> Option<RefDiagnosticSyntax> {
    let target = diagnostic_identifier_token(parts.next()?)?.to_string();
    let mut text_format = false;
    let mut note_reference = false;
    let mut sequence_separator = false;
    let mut relative = false;
    let mut paragraph_number = false;
    let mut full_context_number = false;
    let mut relative_context_number = false;
    let mut suppress_non_numeric = false;
    while let Some(part) = parts.next() {
        if accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        })? {
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\t") {
                if suppress_non_numeric {
                    return None;
                }
                suppress_non_numeric = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\f") {
                if note_reference {
                    return None;
                }
                note_reference = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\d") {
                if sequence_separator {
                    return None;
                }
                let separator = diagnostic_literal_token(parts.next()?)?;
                if separator.is_empty() || separator.starts_with('\\') {
                    return None;
                }
                sequence_separator = true;
                continue;
            }
            if let Some(separator) = strip_ascii_switch_prefix(part, "\\d") {
                if sequence_separator {
                    return None;
                }
                let separator = diagnostic_literal_token(separator)?;
                if separator.is_empty() || separator.starts_with('\\') {
                    return None;
                }
                sequence_separator = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\n") {
                if paragraph_number || full_context_number || relative_context_number {
                    return None;
                }
                paragraph_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\w") {
                if full_context_number || paragraph_number || relative_context_number {
                    return None;
                }
                full_context_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\r") {
                if relative_context_number || paragraph_number || full_context_number {
                    return None;
                }
                relative_context_number = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\h") {
                continue;
            }
            return None;
        }
        return None;
    }
    if suppress_non_numeric && !(paragraph_number || full_context_number || relative_context_number)
    {
        return None;
    }
    if note_reference
        && (relative
            || paragraph_number
            || full_context_number
            || relative_context_number
            || suppress_non_numeric
            || sequence_separator)
    {
        return None;
    }
    Some(RefDiagnosticSyntax {
        target,
        note_reference,
        sequence_separator,
    })
}

struct PageRefDiagnosticSyntax {
    target: String,
    #[cfg(feature = "docx")]
    uses_target_section_number_format: bool,
}

fn supported_page_ref_syntax(instruction: &str) -> Option<PageRefDiagnosticSyntax> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("PAGEREF") {
        return None;
    }
    let target = diagnostic_identifier_token(parts.next()?)?.to_string();
    let mut number_format = false;
    let mut text_format = false;
    let mut relative = false;
    while let Some(part) = parts.next() {
        if accept_general_format_switch(part, &mut parts, |format| {
            accept_page_field_format_switch(format, &mut number_format, &mut text_format)
        })? {
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\h") {
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            return None;
        }
        return None;
    }
    Some(PageRefDiagnosticSyntax {
        target,
        #[cfg(feature = "docx")]
        uses_target_section_number_format: !number_format,
    })
}

fn accept_page_field_format_switch(
    part: &str,
    number_format: &mut bool,
    text_format: &mut bool,
) -> bool {
    accept_page_number_format_switch(part, number_format)
        || accept_field_format_switch(part, text_format)
}

fn page_ref_uncomputed_reason(
    instruction: &str,
    bookmark_names: Option<&HashSet<String>>,
) -> FieldEvaluationReason {
    let Some(syntax) = supported_page_ref_syntax(instruction) else {
        return FieldEvaluationReason::UnsupportedSwitch;
    };
    if bookmark_names.is_some_and(|names| !names.contains(&syntax.target)) {
        return FieldEvaluationReason::UnresolvedBookmark;
    }
    FieldEvaluationReason::NoComputedResult
}

#[cfg(feature = "docx")]
fn docx_page_ref_uncomputed_reason(
    instruction: &str,
    bookmark_names: Option<&HashSet<String>>,
    unsupported_section_format_targets: Option<&HashSet<String>>,
) -> FieldEvaluationReason {
    let Some(syntax) = supported_page_ref_syntax(instruction) else {
        return FieldEvaluationReason::UnsupportedSwitch;
    };
    if bookmark_names.is_some_and(|names| !names.contains(&syntax.target)) {
        return FieldEvaluationReason::UnresolvedBookmark;
    }
    if syntax.uses_target_section_number_format
        && unsupported_section_format_targets
            .is_some_and(|targets| targets.contains(&syntax.target))
    {
        return FieldEvaluationReason::UnsupportedSwitch;
    }
    FieldEvaluationReason::NoComputedResult
}

#[cfg(feature = "docx")]
fn note_ref_uncomputed_reason(
    instruction: &str,
    bookmark_names: Option<&HashSet<String>>,
    note_ref_target_names: Option<&HashSet<String>>,
) -> FieldEvaluationReason {
    let Some(target) = supported_note_ref_target(instruction) else {
        return FieldEvaluationReason::UnsupportedSwitch;
    };
    if note_ref_target_names.is_some_and(|names| names.contains(&target)) {
        return FieldEvaluationReason::NoComputedResult;
    }
    match bookmark_names {
        Some(names) if names.contains(&target) => FieldEvaluationReason::UnsupportedSwitch,
        Some(_) => FieldEvaluationReason::UnresolvedBookmark,
        None => FieldEvaluationReason::UnresolvedBookmark,
    }
}

fn supported_note_ref_target(instruction: &str) -> Option<String> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str);
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("NOTEREF") && !kind.eq_ignore_ascii_case("FTNREF") {
        return None;
    }
    let target = diagnostic_identifier_token(parts.next()?)?.to_string();
    let mut relative = false;
    let mut formatted = false;
    let mut text_format = false;
    while let Some(part) = parts.next() {
        if accept_general_format_switch(part, &mut parts, |format| {
            accept_note_ref_format_switch(format, &mut text_format)
        })? {
            continue;
        }
        if part.starts_with('\\') {
            if part.eq_ignore_ascii_case("\\h") {
                continue;
            }
            if part.eq_ignore_ascii_case("\\f") {
                if formatted {
                    return None;
                }
                formatted = true;
                continue;
            }
            if part.eq_ignore_ascii_case("\\p") {
                if relative {
                    return None;
                }
                relative = true;
                continue;
            }
            return None;
        }
        return None;
    }
    Some(target)
}

fn diagnostic_name_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    }
    .trim();
    if value.is_empty() || value.starts_with('\\') || value.contains('"') {
        return None;
    }
    Some(value)
}

fn diagnostic_literal_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    };
    (!value.contains('"')).then_some(value)
}

fn accept_note_ref_format_switch(part: &str, text_format: &mut bool) -> bool {
    accept_field_format_switch(part, text_format)
}

fn accept_page_number_format_switch(part: &str, number_format: &mut bool) -> bool {
    let mut format = number_format.then_some(FieldNumberFormat::Arabic);
    let accepted = accept_field_number_format_switch(part, &mut format);
    if accepted {
        *number_format = format.is_some();
    }
    accepted
}

fn accept_field_format_switch(part: &str, text_format: &mut bool) -> bool {
    let mut format = text_format.then_some(FieldTextFormat::Upper);
    let accepted = accept_field_text_format_switch(part, &mut format);
    if accepted {
        *text_format = format.is_some();
    }
    accepted
}

fn supported_toc_bookmark_scope(instruction: &str) -> Option<Option<String>> {
    let tokens = instruction_parts(instruction);
    let mut parts = tokens.iter().map(String::as_str).peekable();
    let kind = parts.next()?;
    if !kind.eq_ignore_ascii_case("TOC") {
        return None;
    }
    let mut saw_switch = false;
    let mut outline_range = None;
    let mut saw_outline_switch = false;
    let mut bookmark = None;
    let mut saw_custom_style_switch = false;
    let mut saw_tc_switch = false;
    let mut saw_tc_level_switch = false;
    let mut saw_sequence_switch = false;
    let mut saw_page_number_sequence_prefix = false;
    let mut saw_default_toc_neutral_switch = false;
    let mut text_format = false;
    while let Some(part) = parts.next() {
        saw_switch = true;
        if accept_general_format_switch(part, &mut parts, |format| {
            accept_field_format_switch(format, &mut text_format)
        })? {
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if is_toc_value_neutral_switch_for_report(part) {
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\f") {
            if saw_tc_switch {
                return None;
            }
            if let Some(value) = parts.next_if(|next| !next.starts_with('\\')) {
                diagnostic_identifier_token(value)?;
            }
            saw_tc_switch = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\f") {
            if saw_tc_switch {
                return None;
            }
            if !value.is_empty() {
                diagnostic_identifier_token(value)?;
            }
            saw_tc_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\a") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            if saw_sequence_switch {
                return None;
            }
            diagnostic_identifier_token(value)?;
            saw_sequence_switch = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\a") {
            if saw_sequence_switch {
                return None;
            }
            diagnostic_identifier_token(value)?;
            saw_sequence_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\c") {
            let value = parts.next_if(|next| !next.starts_with('\\'))?;
            if saw_sequence_switch {
                return None;
            }
            diagnostic_identifier_token(value)?;
            saw_sequence_switch = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\c") {
            if saw_sequence_switch {
                return None;
            }
            diagnostic_identifier_token(value)?;
            saw_sequence_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\l") {
            let range = parts.next_if(|next| !next.starts_with('\\'))?;
            parse_toc_outline_range_for_report(range)?;
            if saw_tc_level_switch {
                return None;
            }
            saw_tc_level_switch = true;
            continue;
        }
        if let Some(range) = strip_ascii_switch_prefix(part, "\\l") {
            if range.is_empty() || saw_tc_level_switch {
                return None;
            }
            parse_toc_outline_range_for_report(range)?;
            saw_tc_level_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\u") {
            saw_outline_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\n") {
            if let Some(range) = parts.next_if(|next| !next.starts_with('\\')) {
                parse_toc_outline_range_for_report(range)?;
            }
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(range) = strip_ascii_switch_prefix(part, "\\n") {
            if range.is_empty() {
                return None;
            }
            parse_toc_outline_range_for_report(range)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\p") {
            diagnostic_literal_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(separator) = strip_ascii_switch_prefix(part, "\\p") {
            diagnostic_literal_token(separator)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\d") {
            diagnostic_literal_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(separator) = strip_ascii_switch_prefix(part, "\\d") {
            diagnostic_literal_token(separator)?;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\s") {
            diagnostic_identifier_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            if saw_page_number_sequence_prefix {
                return None;
            }
            saw_page_number_sequence_prefix = true;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if let Some(identifier) = strip_ascii_switch_prefix(part, "\\s") {
            diagnostic_identifier_token(identifier)?;
            if saw_page_number_sequence_prefix {
                return None;
            }
            saw_page_number_sequence_prefix = true;
            saw_default_toc_neutral_switch = true;
            continue;
        }
        if part.eq_ignore_ascii_case("\\b") {
            let target =
                diagnostic_identifier_token(parts.next_if(|next| !next.starts_with('\\'))?)?;
            if bookmark.replace(target.to_string()).is_some() {
                return None;
            }
            continue;
        }
        if let Some(target) = strip_ascii_switch_prefix(part, "\\b") {
            let target = diagnostic_identifier_token(target)?;
            if bookmark.replace(target.to_string()).is_some() {
                return None;
            }
            continue;
        }
        if part.eq_ignore_ascii_case("\\t") {
            parse_toc_style_specs_for_report(parts.next_if(|next| !next.starts_with('\\'))?)?;
            saw_custom_style_switch = true;
            continue;
        }
        if let Some(value) = strip_ascii_switch_prefix(part, "\\t") {
            if value.is_empty() {
                return None;
            }
            parse_toc_style_specs_for_report(value)?;
            saw_custom_style_switch = true;
            continue;
        }
        let range = if part.eq_ignore_ascii_case("\\o") {
            match parts.next_if(|next| !next.starts_with('\\')) {
                Some(range) => range,
                None => {
                    if outline_range.replace((1, 9)).is_some() {
                        return None;
                    }
                    continue;
                }
            }
        } else {
            strip_ascii_switch_prefix(part, "\\o")?
        };
        if outline_range
            .replace(parse_toc_outline_range_for_report(range)?)
            .is_some()
        {
            return None;
        }
    }
    if saw_switch
        && outline_range.is_none()
        && !saw_outline_switch
        && !saw_custom_style_switch
        && !saw_tc_switch
        && !saw_tc_level_switch
        && !saw_sequence_switch
        && !saw_default_toc_neutral_switch
        && bookmark.is_none()
    {
        return None;
    }
    Some(bookmark)
}

fn toc_uncomputed_reason(
    instruction: &str,
    bookmark_names: Option<&HashSet<String>>,
) -> FieldEvaluationReason {
    match supported_toc_bookmark_scope(instruction) {
        Some(Some(target)) => match bookmark_names {
            Some(names) if names.contains(&target) => FieldEvaluationReason::NoComputedResult,
            Some(_) => FieldEvaluationReason::UnresolvedBookmark,
            None => FieldEvaluationReason::UnresolvedBookmark,
        },
        Some(None) => FieldEvaluationReason::NoComputedResult,
        None => FieldEvaluationReason::UnsupportedSwitch,
    }
}

fn parse_toc_style_specs_for_report(value: &str) -> Option<()> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    };
    let parts: Vec<_> = value.split(',').map(str::trim).collect();
    if parts.is_empty() || parts.len() % 2 != 0 {
        return None;
    }
    for pair in parts.chunks_exact(2) {
        let name = pair[0];
        let level = pair[1];
        if name.is_empty() || name.starts_with('\\') || name.contains('"') || level.contains('"') {
            return None;
        }
        let level = level.parse::<u8>().ok()?;
        if !(1..=9).contains(&level) {
            return None;
        }
    }
    Some(())
}

fn diagnostic_identifier_token(value: &str) -> Option<&str> {
    let value = diagnostic_name_token(value)?;
    (!value.chars().any(char::is_whitespace)).then_some(value)
}

fn is_toc_value_neutral_switch_for_report(part: &str) -> bool {
    part.eq_ignore_ascii_case("\\h")
        || part.eq_ignore_ascii_case("\\z")
        || part.eq_ignore_ascii_case("\\w")
        || part.eq_ignore_ascii_case("\\x")
}

fn strip_ascii_switch_prefix<'a>(part: &'a str, switch: &str) -> Option<&'a str> {
    let prefix = part.get(..switch.len())?;
    prefix
        .eq_ignore_ascii_case(switch)
        .then_some(&part[switch.len()..])
}

fn parse_toc_outline_range_for_report(range: &str) -> Option<(u8, u8)> {
    let range = diagnostic_name_token(range)?;
    let (start, end) = range.split_once('-')?;
    let start = start.parse::<u8>().ok()?;
    let end = end.parse::<u8>().ok()?;
    ((1..=9).contains(&start) && start <= end && end <= 9).then_some((start, end))
}

#[cfg(not(feature = "docx"))]
fn parse_toc_level_for_report(value: &str) -> Option<u8> {
    let level = diagnostic_name_token(value)?.parse::<u8>().ok()?;
    (1..=9).contains(&level).then_some(level)
}

fn instruction_parts(s: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for ch in s.chars() {
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
    parts
}

#[cfg(feature = "docx")]
fn scan_docx_xml(xml: &str, features: &mut FeatureInventory) {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut object_depth = 0usize;
    let mut object_has_ole = false;
    let mut table_depth = 0usize;
    let mut xml_depth = 0usize;
    let mut old_revision_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    skip_report_subtree(&mut reader);
                    continue;
                }
                match name {
                    b"AlternateContent" => {
                        alternate_content_stack.push(ReportAlternateContentState {
                            branch_depth: xml_depth + 1,
                            anchor_seen: false,
                            shape_marker_seen: false,
                            took_branch: false,
                        });
                    }
                    b"tbl" => {
                        if table_depth > 0 && old_revision_depth == 0 {
                            features.nested_tables += 1;
                        }
                        table_depth = table_depth.saturating_add(1);
                    }
                    b"commentReference" if old_revision_depth == 0 => features.comments += 1,
                    b"commentReference" => {}
                    b"ins" => features.tracked_insertions += 1,
                    b"del" => {
                        features.tracked_deletions += 1;
                        old_revision_depth = old_revision_depth.saturating_add(1);
                    }
                    b"moveFrom" => {
                        features.tracked_moves += 1;
                        old_revision_depth = old_revision_depth.saturating_add(1);
                    }
                    b"moveTo" => features.tracked_moves += 1,
                    name if is_revision_property_change(name) => {
                        features.tracked_property_changes += 1;
                    }
                    b"fldSimple" if old_revision_depth == 0 => features.fields += 1,
                    b"fldSimple" => {}
                    b"fldChar" if old_revision_depth == 0 && is_complex_field_begin(&e) => {
                        features.fields += 1;
                    }
                    b"fldChar" => {}
                    b"hyperlink" if old_revision_depth == 0 => features.hyperlinks += 1,
                    b"hyperlink" => {}
                    b"sdt" if old_revision_depth == 0 => features.content_controls += 1,
                    b"sdt" => {}
                    b"anchor" if old_revision_depth == 0 => {
                        features.floating_shapes += 1;
                        mark_report_alternate_anchor_seen(&mut alternate_content_stack);
                    }
                    b"anchor" => {}
                    name if is_alternate_content_shape_marker(name) && old_revision_depth == 0 => {
                        mark_report_alternate_shape_marker_seen(&mut alternate_content_stack);
                    }
                    b"chart" if old_revision_depth == 0 => features.charts += 1,
                    b"chart" => {}
                    b"object" if old_revision_depth == 0 => {
                        object_depth = object_depth.saturating_add(1);
                        object_has_ole = false;
                    }
                    b"object" => {}
                    b"oleObject" if old_revision_depth == 0 => {
                        if object_depth > 0 {
                            if !object_has_ole {
                                features.ole_objects += 1;
                                object_has_ole = true;
                            }
                        } else {
                            features.ole_objects += 1;
                        }
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_add(1);
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    continue;
                }
                match name {
                    b"AlternateContent" if old_revision_depth == 0 => features.floating_shapes += 1,
                    b"AlternateContent" => {}
                    b"tbl" if table_depth > 0 && old_revision_depth == 0 => {
                        features.nested_tables += 1;
                    }
                    b"tbl" => {}
                    b"commentReference" if old_revision_depth == 0 => features.comments += 1,
                    b"commentReference" => {}
                    b"ins" => features.tracked_insertions += 1,
                    b"del" => features.tracked_deletions += 1,
                    b"moveFrom" | b"moveTo" => features.tracked_moves += 1,
                    name if is_revision_property_change(name) => {
                        features.tracked_property_changes += 1;
                    }
                    b"fldSimple" if old_revision_depth == 0 => features.fields += 1,
                    b"fldSimple" => {}
                    b"fldChar" if old_revision_depth == 0 && is_complex_field_begin(&e) => {
                        features.fields += 1;
                    }
                    b"fldChar" => {}
                    b"hyperlink" if old_revision_depth == 0 => features.hyperlinks += 1,
                    b"hyperlink" => {}
                    b"sdt" if old_revision_depth == 0 => features.content_controls += 1,
                    b"sdt" => {}
                    b"anchor" if old_revision_depth == 0 => {
                        features.floating_shapes += 1;
                        mark_report_alternate_anchor_seen(&mut alternate_content_stack);
                    }
                    b"anchor" => {}
                    name if is_alternate_content_shape_marker(name) && old_revision_depth == 0 => {
                        mark_report_alternate_shape_marker_seen(&mut alternate_content_stack);
                    }
                    b"chart" if old_revision_depth == 0 => features.charts += 1,
                    b"chart" => {}
                    b"object" if old_revision_depth == 0 => features.ole_objects += 1,
                    b"object" => {}
                    b"oleObject" if old_revision_depth == 0 => {
                        if object_depth > 0 {
                            if !object_has_ole {
                                features.ole_objects += 1;
                                object_has_ole = true;
                            }
                        } else {
                            features.ole_objects += 1;
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"object" && object_depth > 0 => {
                if !object_has_ole {
                    features.ole_objects += 1;
                }
                object_depth -= 1;
                object_has_ole = false;
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"tbl" && table_depth > 0 => {
                table_depth -= 1;
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if name == b"AlternateContent" {
                    if let Some(state) = alternate_content_stack.pop() {
                        if !state.anchor_seen && state.shape_marker_seen && old_revision_depth == 0
                        {
                            features.floating_shapes += 1;
                        }
                    }
                }
                if is_old_revision_content(name) {
                    old_revision_depth = old_revision_depth.saturating_sub(1);
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(feature = "docx")]
fn is_complex_field_begin(e: &quick_xml::events::BytesStart<'_>) -> bool {
    crate::docx::attr_local(e, b"fldCharType").as_deref() == Some("begin")
}

#[cfg(feature = "docx")]
#[derive(Debug, Clone, Copy)]
struct ReportAlternateContentState {
    branch_depth: usize,
    anchor_seen: bool,
    shape_marker_seen: bool,
    took_branch: bool,
}

#[cfg(feature = "docx")]
fn should_skip_report_alternate_branch(
    stack: &mut [ReportAlternateContentState],
    xml_depth: usize,
    name: &[u8],
) -> bool {
    if !matches!(name, b"Choice" | b"Fallback") {
        return false;
    }
    let Some(state) = stack.last_mut() else {
        return false;
    };
    if state.branch_depth != xml_depth {
        return false;
    }
    if state.took_branch {
        true
    } else {
        state.took_branch = true;
        false
    }
}

#[cfg(feature = "docx")]
fn mark_report_alternate_anchor_seen(stack: &mut [ReportAlternateContentState]) {
    if let Some(state) = stack.last_mut() {
        state.anchor_seen = true;
    }
}

#[cfg(feature = "docx")]
fn mark_report_alternate_shape_marker_seen(stack: &mut [ReportAlternateContentState]) {
    if let Some(state) = stack.last_mut() {
        state.shape_marker_seen = true;
    }
}

#[cfg(feature = "docx")]
fn skip_report_subtree(reader: &mut quick_xml::Reader<&[u8]>) {
    use quick_xml::events::Event;

    let mut depth = 1usize;
    loop {
        match reader.read_event() {
            Ok(Event::Start(_)) => depth = depth.saturating_add(1),
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

#[cfg(feature = "docx")]
fn is_revision_property_change(name: &[u8]) -> bool {
    matches!(
        name,
        b"rPrChange"
            | b"pPrChange"
            | b"tblPrChange"
            | b"tblGridChange"
            | b"trPrChange"
            | b"tcPrChange"
            | b"sectPrChange"
            | b"numPrChange"
    )
}

#[cfg(feature = "docx")]
fn is_old_revision_content(name: &[u8]) -> bool {
    matches!(name, b"del" | b"moveFrom")
}

#[cfg(feature = "docx")]
fn is_alternate_content_shape_marker(name: &[u8]) -> bool {
    matches!(name, b"drawing" | b"pict" | b"shape")
}

#[cfg(feature = "docx")]
fn count_elements(xml: &str, needle: &[u8]) -> usize {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut count = 0;
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == needle => {
                count += 1;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    count
}

#[cfg(feature = "docx")]
fn docx_bookmark_names(xml: &str) -> HashSet<String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    let mut names = HashSet::new();
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    skip_report_subtree(&mut reader);
                    continue;
                }
                match name {
                    b"del" | b"moveFrom" => {
                        skip_report_subtree(&mut reader);
                        continue;
                    }
                    b"AlternateContent" => {
                        alternate_content_stack.push(ReportAlternateContentState {
                            branch_depth: xml_depth + 1,
                            anchor_seen: false,
                            shape_marker_seen: false,
                            took_branch: false,
                        });
                    }
                    b"bookmarkStart" => {
                        if let Some(name) = crate::docx::attr_local(&e, b"name") {
                            names.insert(name);
                        }
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_add(1);
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    continue;
                }
                if name == b"bookmarkStart" {
                    if let Some(name) = crate::docx::attr_local(&e, b"name") {
                        names.insert(name);
                    }
                }
            }
            Ok(Event::End(e)) => {
                if local(e.name().as_ref()) == b"AlternateContent" {
                    alternate_content_stack.pop();
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    names
}

#[cfg(feature = "docx")]
fn docx_page_ref_unsupported_section_format_targets(xml: &str) -> HashSet<String> {
    use quick_xml::events::{BytesStart, Event};
    use quick_xml::Reader;

    fn section_page_number_format_unsupported(e: &BytesStart<'_>) -> Option<bool> {
        let format = crate::docx::attr_local(e, b"fmt")?;
        Some(!matches!(
            format.as_str(),
            "decimal"
                | "decimalZero"
                | "numberInDash"
                | "decimalFullWidth"
                | "decimalHalfWidth"
                | "decimalFullWidth2"
                | "decimalEnclosedCircle"
                | "decimalEnclosedFullstop"
                | "decimalEnclosedParen"
                | "ganada"
                | "chosung"
                | "koreanDigital"
                | "koreanCounting"
                | "koreanLegal"
                | "koreanDigital2"
                | "lowerLetter"
                | "upperLetter"
                | "lowerRoman"
                | "upperRoman"
                | "ordinal"
                | "cardinalText"
                | "ordinalText"
        ))
    }

    fn record_bookmark(
        e: &BytesStart<'_>,
        current_section_unsupported: bool,
        current_section_bookmarks: &mut Vec<String>,
        unsupported_targets: &mut HashSet<String>,
    ) {
        if let Some(name) = crate::docx::attr_local(e, b"name") {
            if current_section_unsupported {
                unsupported_targets.insert(name.clone());
            }
            current_section_bookmarks.push(name);
        }
    }

    fn apply_section_format(
        is_paragraph_break: bool,
        page_format_unsupported: Option<bool>,
        current_section_unsupported: &mut bool,
        current_section_bookmarks: &mut Vec<String>,
        unsupported_targets: &mut HashSet<String>,
    ) {
        if is_paragraph_break {
            if let Some(is_unsupported) = page_format_unsupported {
                *current_section_unsupported = is_unsupported;
            }
            current_section_bookmarks.clear();
        } else if let Some(is_unsupported) = page_format_unsupported {
            *current_section_unsupported = is_unsupported;
            for name in current_section_bookmarks.iter() {
                if is_unsupported {
                    unsupported_targets.insert(name.clone());
                } else {
                    unsupported_targets.remove(name);
                }
            }
        }
    }

    let mut reader = Reader::from_str(xml);
    let mut unsupported_targets = HashSet::new();
    let mut current_section_bookmarks = Vec::new();
    let mut current_section_unsupported = false;
    let mut paragraph_properties_depth = 0usize;
    let mut section_properties_depth = 0usize;
    let mut section_is_paragraph_break = false;
    let mut section_page_format_unsupported = None;
    let mut xml_depth = 0usize;
    let mut alternate_content_stack = Vec::new();
    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    skip_report_subtree(&mut reader);
                    continue;
                }
                match name {
                    b"del" | b"moveFrom" => {
                        skip_report_subtree(&mut reader);
                        continue;
                    }
                    b"AlternateContent" => {
                        alternate_content_stack.push(ReportAlternateContentState {
                            branch_depth: xml_depth + 1,
                            anchor_seen: false,
                            shape_marker_seen: false,
                            took_branch: false,
                        });
                    }
                    b"pPr" => paragraph_properties_depth += 1,
                    b"sectPr" => {
                        section_properties_depth += 1;
                        if section_properties_depth == 1 {
                            section_is_paragraph_break = paragraph_properties_depth > 0;
                            section_page_format_unsupported = None;
                        }
                    }
                    b"pgNumType"
                        if section_properties_depth > 0
                            && section_page_format_unsupported.is_none() =>
                    {
                        section_page_format_unsupported =
                            section_page_number_format_unsupported(&e);
                    }
                    b"bookmarkStart" => record_bookmark(
                        &e,
                        current_section_unsupported,
                        &mut current_section_bookmarks,
                        &mut unsupported_targets,
                    ),
                    _ => {}
                }
                xml_depth = xml_depth.saturating_add(1);
            }
            Ok(Event::Empty(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                if should_skip_report_alternate_branch(
                    &mut alternate_content_stack,
                    xml_depth,
                    name,
                ) {
                    continue;
                }
                match name {
                    b"sectPr" => {
                        apply_section_format(
                            paragraph_properties_depth > 0,
                            None,
                            &mut current_section_unsupported,
                            &mut current_section_bookmarks,
                            &mut unsupported_targets,
                        );
                    }
                    b"pgNumType"
                        if section_properties_depth > 0
                            && section_page_format_unsupported.is_none() =>
                    {
                        section_page_format_unsupported =
                            section_page_number_format_unsupported(&e);
                    }
                    b"bookmarkStart" => record_bookmark(
                        &e,
                        current_section_unsupported,
                        &mut current_section_bookmarks,
                        &mut unsupported_targets,
                    ),
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let qname = e.name();
                match local(qname.as_ref()) {
                    b"sectPr" => {
                        if section_properties_depth == 1 {
                            apply_section_format(
                                section_is_paragraph_break,
                                section_page_format_unsupported,
                                &mut current_section_unsupported,
                                &mut current_section_bookmarks,
                                &mut unsupported_targets,
                            );
                            section_is_paragraph_break = false;
                            section_page_format_unsupported = None;
                        }
                        section_properties_depth = section_properties_depth.saturating_sub(1);
                    }
                    b"pPr" => {
                        paragraph_properties_depth = paragraph_properties_depth.saturating_sub(1);
                    }
                    b"AlternateContent" => {
                        alternate_content_stack.pop();
                    }
                    _ => {}
                }
                xml_depth = xml_depth.saturating_sub(1);
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    unsupported_targets
}

#[cfg(feature = "docx")]
fn local(name: &[u8]) -> &[u8] {
    match name.iter().rposition(|&b| b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

#[cfg(test)]
mod tests {
    use super::fields_for_model;
    use crate::annotation::FieldKind;
    use crate::model::{Block, FieldRole, Paragraph, Run};

    #[test]
    fn supported_page_ref_syntax_accepts_mixed_case_arabic() {
        assert!(super::supported_page_ref_syntax(r"PAGEREF Figure1 \* ArAbIc").is_some());
    }

    #[test]
    fn reference_diagnostics_reject_switch_first_names() {
        assert!(super::supported_ref_syntax(r"REF \h Figure1").is_none());
        assert!(super::supported_direct_ref_syntax(r"\h Figure1").is_none());
        assert!(super::supported_page_ref_syntax(r"PAGEREF \p Figure1").is_none());
        assert!(super::supported_note_ref_target(r"NOTEREF \p FootOne").is_none());
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_revision_number_diagnostics_reject_malformed_tails() {
        assert_eq!(
            super::revision_number_uncomputed_reason(r"REVNUM \* MERGEFORMAT"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::revision_number_uncomputed_reason(r"REVNUM \x"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_style_ref_diagnostics_reject_malformed_tails() {
        assert_eq!(
            super::style_ref_uncomputed_reason(r"STYLEREF Heading1 \n \t"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::style_ref_uncomputed_reason(r"STYLEREF Heading1 \t"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_toc_entry_diagnostics_reject_malformed_tails() {
        assert_eq!(
            super::toc_entry_uncomputed_reason(r#"TC "Entry" \f A \l 2"#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::toc_entry_uncomputed_reason(r#"TC "Entry" \l "2"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_numbering_diagnostics_reject_malformed_bidi_outline() {
        assert_eq!(
            super::numbering_uncomputed_reason("BIDIOUTLINE"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::numbering_uncomputed_reason(r"BIDIOUTLINE \x"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_action_diagnostics_reject_malformed_format_tails() {
        assert_eq!(
            super::action_uncomputed_reason(r#"PRINT \p ReportBox "0 0 moveto""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::action_uncomputed_reason(r#"MACROBUTTON RunReport \* MERGEFORMAT"#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::action_uncomputed_reason(r#"MACROBUTTON RunReport Run \* Upper Again"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_accept_valid_advance() {
        assert_eq!(
            super::display_uncomputed_reason(r"ADVANCE \r 2 \d4 \* MERGEFORMAT"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r"ADVANCE \z 2"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_reject_malformed_symbol() {
        assert_eq!(
            super::display_uncomputed_reason(r"SYMBOL 65 \f Wingdings"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r#"SYMBOL 65 \f "Wingdings"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_accept_valid_eq_displacement() {
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \d \fo10 \li(Title)"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \d \fo10 \li(Title"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_accept_valid_eq_script() {
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \s\ai4(Above)\di3(Below)"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \s\ai4(Above"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_accept_valid_eq_fraction() {
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \f(1,2)"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \f(1)"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_display_diagnostics_accept_valid_eq_radical() {
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \r(3,27)"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::display_uncomputed_reason(r"EQ \r()"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_compare_diagnostics_reject_malformed_operands() {
        assert_eq!(
            super::compare_uncomputed_reason(r#"COMPARE "98512" = "985*""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::compare_uncomputed_reason(r#"COMPARE \o = "Gold""#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_formula_diagnostics_reject_malformed_switches() {
        assert_eq!(
            super::formula_uncomputed_reason(r#"= CustomerTotal \# "0.00""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::formula_uncomputed_reason(r#"= 1 \# "0.00"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_if_diagnostics_reject_malformed_operands() {
        assert_eq!(
            super::if_uncomputed_reason(r#"IF 1 = 1 "yes" "no""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::if_uncomputed_reason(r#"IF \o = "Gold" "ship" "hold""#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_merge_control_diagnostics_reject_malformed_comparisons() {
        assert_eq!(
            super::merge_control_uncomputed_reason(r"NEXT \* MERGEFORMAT"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::merge_control_uncomputed_reason(r#"NEXTIF City = "Tokyo""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::merge_control_uncomputed_reason(r#"NEXTIF \o = "Tokyo""#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_quote_diagnostics_reject_malformed_tails() {
        assert_eq!(
            super::quote_uncomputed_reason(r#"QUOTE "Ready" \* Upper"#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::quote_uncomputed_reason(r#"QUOTE "Ready" \z"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_set_diagnostics_reject_malformed_names() {
        assert_eq!(
            super::set_uncomputed_reason(r#"SET ClientName "Acme" \* Upper"#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::set_uncomputed_reason(r#"SET \r "Acme""#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_sequence_diagnostics_reject_malformed_tails() {
        assert_eq!(
            super::sequence_uncomputed_reason(r"SEQ Figure \r -1"),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::sequence_uncomputed_reason(r"SEQ Figure \x"),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[cfg(not(feature = "docx"))]
    #[test]
    fn no_default_prompt_diagnostics_reject_malformed_syntax() {
        assert_eq!(
            super::prompt_uncomputed_reason(r#"FILLIN "Client?" \d Acme"#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::prompt_uncomputed_reason(r#"ASK ClientCode "Client code?" \d "ac-42""#),
            super::FieldEvaluationReason::NoComputedResult
        );
        assert_eq!(
            super::prompt_uncomputed_reason(r#"FILLIN \z \d Acme"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
        assert_eq!(
            super::prompt_uncomputed_reason(r#"ASK ClientCode \o \d ac-42"#),
            super::FieldEvaluationReason::UnsupportedSwitch
        );
    }

    #[test]
    fn supported_toc_bookmark_scope_rejects_duplicate_tc_filter() {
        assert!(super::supported_toc_bookmark_scope(r"TOC \f m \f x").is_none());
    }

    #[test]
    fn supported_toc_bookmark_scope_rejects_unbalanced_outline_range_quotes() {
        assert!(super::supported_toc_bookmark_scope(r#"TOC \o "1-2"#).is_none());
        assert!(super::supported_toc_bookmark_scope(r#"TOC \o "1-2" \l "2-3"#).is_none());
    }

    #[test]
    fn model_fields_coalesce_contiguous_result_runs() {
        let blocks = vec![Block::Paragraph(Paragraph {
            runs: vec![
                Run {
                    text: "7".to_string(),
                    field: FieldRole::Simple {
                        instruction: " PAGE ".to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: " of ".to_string(),
                    field: FieldRole::Simple {
                        instruction: "PAGE".to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: "tail".to_string(),
                    ..Run::default()
                },
                Run {
                    text: "site".to_string(),
                    field: FieldRole::Hyperlink {
                        url: "https://example.com".to_string(),
                    },
                    ..Run::default()
                },
            ],
            ..Paragraph::default()
        })];

        let fields = fields_for_model(&blocks);

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].kind, FieldKind::Page);
        assert_eq!(fields[0].instruction, "PAGE");
        assert_eq!(fields[0].result, "7 of ");
        assert_eq!(fields[1].kind, FieldKind::Hyperlink);
        assert_eq!(fields[1].instruction, r#"HYPERLINK "https://example.com""#);
        assert_eq!(fields[1].result, "site");

        let inventory = super::feature_inventory_for_model(&blocks);
        assert_eq!(inventory.fields, 2);
        assert_eq!(inventory.hyperlinks, 1);
        assert_eq!(
            inventory.field_kinds,
            vec![
                super::FieldKindCount {
                    kind: FieldKind::Page,
                    count: 1,
                },
                super::FieldKindCount {
                    kind: FieldKind::Hyperlink,
                    count: 1,
                },
            ]
        );
    }

    #[cfg(feature = "render")]
    #[test]
    fn render_model_fields_coalesce_contiguous_result_runs() {
        let blocks = vec![Block::Paragraph(Paragraph {
            runs: vec![
                Run {
                    text: "A".to_string(),
                    field: FieldRole::Simple {
                        instruction: r#" TOC \o "1-3" "#.to_string(),
                    },
                    ..Run::default()
                },
                Run {
                    text: "B".to_string(),
                    field: FieldRole::Simple {
                        instruction: r#"TOC \o "1-3""#.to_string(),
                    },
                    ..Run::default()
                },
            ],
            ..Paragraph::default()
        })];

        let inventory = super::render_inventory_for_model(&blocks);

        assert_eq!(inventory.fields, 1);
        assert_eq!(
            inventory.field_kinds,
            vec![super::FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            }]
        );
        assert_eq!(
            inventory.unsupported_field_kinds,
            vec![super::FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            }]
        );
    }

    #[test]
    fn flattened_legacy_doc_warning_requires_non_body_counts() {
        assert_eq!(
            super::legacy_doc_flattened_subdocuments_warning(0, 0, 0, 0, 0),
            None
        );

        assert_eq!(
            super::legacy_doc_flattened_subdocuments_warning(2, 3, 4, 5, 6),
            Some(super::DocumentWarning::LegacyDocFlattenedSubdocuments {
                footnotes: 2,
                headers_footers: 3,
                annotations: 4,
                endnotes: 5,
                text_boxes: 6,
            })
        );
    }
}
