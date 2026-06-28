//! Annotation types extracted from Word documents.

use crate::model::Color;

/// A text range in the main document body associated with an annotation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextAnchor {
    /// Anchor id as stored by WordprocessingML (`w:id`).
    pub id: String,
    /// Visible text covered by the anchor range.
    pub text: String,
}

/// A Word comment extracted from a `.docx` comments part or recovered from a
/// legacy `.doc` annotation subdocument.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Comment {
    /// Comment id as stored by WordprocessingML (`w:id`).
    pub id: String,
    /// Comment author (`w:author`), if present.
    pub author: Option<String>,
    /// Author initials (`w:initials`), if present.
    pub initials: Option<String>,
    /// Comment timestamp (`w:date`), if present.
    pub date: Option<String>,
    /// Parent comment id for replies, from `w:parentId` or commentsExtended metadata.
    pub parent_comment_id: Option<String>,
    /// Visible text contained in the comment body.
    pub text: String,
    /// Main-document range this comment is anchored to, if rdoc found one.
    pub anchor: Option<TextAnchor>,
}

/// Kind of note recovered from a Word document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    /// Footnote content.
    Footnote,
    /// Endnote content.
    Endnote,
}

/// A footnote or endnote recovered from a Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Note {
    /// Stable note id. For legacy `.doc` recovery this is a synthetic id because
    /// the current binary reader only exposes subdocument regions, not individual
    /// note reference ids.
    pub id: String,
    /// Footnote or endnote.
    pub kind: NoteKind,
    /// Visible note body text.
    pub text: String,
    /// Body range or reference marker this note is anchored to, if known.
    pub anchor: Option<TextAnchor>,
}

/// A text box recovered from a Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextBox {
    /// Stable text-box id. For legacy `.doc` recovery this is a synthetic id
    /// because the current binary reader exposes text-box subdocument regions,
    /// not individual shape ids.
    pub id: String,
    /// Visible text-box content.
    pub text: String,
    /// Body range or shape anchor this text box is attached to, if known.
    pub anchor: Option<TextAnchor>,
}

/// Drawing extent for a recovered floating shape, in English Metric Units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeExtent {
    /// Width in EMUs.
    pub cx_emu: i64,
    /// Height in EMUs.
    pub cy_emu: i64,
}

/// Absolute point for a recovered floating shape, in English Metric Units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapePoint {
    /// X coordinate in EMUs.
    pub x_emu: i64,
    /// Y coordinate in EMUs.
    pub y_emu: i64,
}

/// Visual-effect extents for a recovered floating shape, in English Metric Units.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShapeEffectExtent {
    /// Extra left extent from `wp:effectExtent/@l`.
    pub left_emu: i64,
    /// Extra top extent from `wp:effectExtent/@t`.
    pub top_emu: i64,
    /// Extra right extent from `wp:effectExtent/@r`.
    pub right_emu: i64,
    /// Extra bottom extent from `wp:effectExtent/@b`.
    pub bottom_emu: i64,
}

/// Positioning metadata for a recovered floating shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapePosition {
    /// WordprocessingML `relativeFrom` value, such as `page`, `margin`,
    /// `column`, or `paragraph`, when present.
    pub relative_from: Option<String>,
    /// Absolute position offset in EMUs, when the shape uses `wp:posOffset`.
    pub offset_emu: Option<i64>,
    /// Alignment keyword from `wp:align`, such as `left`, `center`, or `top`,
    /// when the shape is aligned rather than offset.
    pub align: Option<String>,
}

/// Distance from a floating shape to surrounding text, in English Metric Units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShapeDistance {
    /// Top distance from a `distT` attribute, when present.
    pub top_emu: Option<i64>,
    /// Bottom distance from a `distB` attribute, when present.
    pub bottom_emu: Option<i64>,
    /// Left distance from a `distL` attribute, when present.
    pub left_emu: Option<i64>,
    /// Right distance from a `distR` attribute, when present.
    pub right_emu: Option<i64>,
}

/// Text-wrapping policy declared by a floating shape anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeWrapping {
    /// WordprocessingML wrap kind, such as `none`, `square`, `tight`,
    /// `through`, or `topAndBottom`.
    pub kind: String,
    /// `wrapText` value, such as `bothSides`, `left`, or `right`, when the
    /// wrap kind exposes one.
    pub text: Option<String>,
    /// Text-distance margins declared on the `wp:wrap*` element.
    pub distance: ShapeDistance,
}

/// Floating shape geometry recovered from a Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FloatingShape {
    /// Stable shape id, normally the `wp:docPr/@id` value for `.docx`.
    pub id: String,
    /// Display name from `wp:docPr/@name`, if present.
    pub name: Option<String>,
    /// Alternative text/description from `wp:docPr/@descr`, if present.
    pub description: Option<String>,
    /// Visible text recovered from a text-bearing shape body, when present.
    pub text: Option<String>,
    /// DrawingML preset geometry name from `a:prstGeom/@prst`, when present.
    pub preset_geometry: Option<String>,
    /// Simple DrawingML solid fill color from `a:solidFill/a:srgbClr`, when present.
    pub fill_color: Option<Color>,
    /// Simple DrawingML outline color from `a:ln/a:solidFill/a:srgbClr`, when present.
    pub outline_color: Option<Color>,
    /// Whether `wp:anchor/@simplePos` asks consumers to use `wp:simplePos`.
    pub simple_position_enabled: Option<bool>,
    /// Absolute `wp:simplePos` point, when present.
    pub simple_position: Option<ShapePoint>,
    /// Visual-effect extents from `wp:effectExtent`, when present.
    pub effect_extent: Option<ShapeEffectExtent>,
    /// Best-effort zero-based top-level body block index containing the anchor,
    /// when recoverable from `.docx` body order.
    pub anchor_block_index: Option<usize>,
    /// Best-effort visible text of the containing top-level body block, excluding
    /// the floating shape body itself when recovered from `.docx`.
    pub anchor_text: Option<String>,
    /// Zero-width anchor offset inside `anchor_text`, counted in Unicode scalar
    /// values after the same text normalization used for `anchor_text`.
    pub anchor_char_offset: Option<usize>,
    /// Shape drawing extent in EMUs, if present.
    pub extent: Option<ShapeExtent>,
    /// Horizontal positioning metadata from `wp:positionH`, if present.
    pub horizontal_position: Option<ShapePosition>,
    /// Vertical positioning metadata from `wp:positionV`, if present.
    pub vertical_position: Option<ShapePosition>,
    /// WordprocessingML `relativeHeight` z-order value, when present.
    pub relative_height: Option<i64>,
    /// Whether the anchor asks Word to place the shape behind document text.
    pub behind_doc: Option<bool>,
    /// Whether the anchor is laid out inside table cells.
    pub layout_in_cell: Option<bool>,
    /// Whether the anchor is locked.
    pub locked: Option<bool>,
    /// Whether the anchor may overlap other floating objects.
    pub allow_overlap: Option<bool>,
    /// Text-distance margins declared by `wp:anchor/@dist*`.
    pub distance: ShapeDistance,
    /// Text-wrapping policy declared by the anchor, if present.
    pub wrapping: Option<ShapeWrapping>,
}

/// Kind of running header/footer region recovered from a Word document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderFooterKind {
    /// Running header content.
    Header,
    /// Running footer content.
    Footer,
    /// Even-page header content.
    EvenPageHeader,
    /// Odd-page header content.
    OddPageHeader,
    /// Even-page footer content.
    EvenPageFooter,
    /// Odd-page footer content.
    OddPageFooter,
    /// First-page header content.
    FirstPageHeader,
    /// First-page footer content.
    FirstPageFooter,
    /// Header/footer text recovered from a format surface that does not yet
    /// distinguish the exact variant.
    Unknown,
}

/// A running header/footer record recovered from a Word document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderFooter {
    /// Stable header/footer id. `.docx` records use `part#type` ids such as
    /// `word/header1.xml#default`; legacy `.doc` recovery uses synthetic ids.
    pub id: String,
    /// Header/footer variant when known.
    pub kind: HeaderFooterKind,
    /// Visible header/footer text.
    pub text: String,
}

/// Known Word field instruction classes rdoc distinguishes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// `HYPERLINK`.
    Hyperlink,
    /// Current-page field. Trusted structural/rendered `.docx` body contexts
    /// compute a current page value; broader layout-derived cases preserve
    /// cached display text.
    Page,
    /// `TOC`.
    Toc,
    /// `FILENAME`.
    Filename,
    /// `MERGEFIELD`.
    MergeField,
    /// `REF`.
    Ref,
    /// `PAGEREF`.
    PageRef,
    /// `NOTEREF` (and legacy `FTNREF`).
    NoteRef,
    /// `TC` table-of-contents entry markers. Supported literal marker fields
    /// compute as hidden output and feed matching `TOC \f` fields; unsupported
    /// marker syntax preserves cached text with no computed result.
    TocEntry,
    /// `SEQ`. Deterministic source-order forms compute; unsupported sequence
    /// syntax preserves cached text without mutating later counters.
    Sequence,
    /// Cached display fields for document information, date/time, and document
    /// statistics such as `DATE`, `AUTHOR`, `DOCPROPERTY`, or `NUMPAGES`.
    DocumentInfo(String),
    /// Named dynamic/control fields, including deterministic literal
    /// expression fields (`=`, `IF`, `QUOTE`, `COMPARE`), explicit-default
    /// prompt fields (`FILLIN`, `ASK`), literal bookmark assignment (`SET`),
    /// and merge-control fields (`NEXT`, `NEXTIF`, `SKIPIF`) where rdoc can
    /// compute a side-effect-free result.
    Dynamic(String),
    /// Fields that insert or link external/package/application content that rdoc
    /// does not evaluate yet, such as `INCLUDETEXT`, `INCLUDEPICTURE`, `LINK`,
    /// `EMBED`, `DATABASE`, `DDE`, `DDEAUTO`, `IMPORT`, `INCLUDE`, `AUTOTEXT`,
    /// or `AUTOTEXTLIST`.
    InsertedContent(String),
    /// Mail-merge helper fields beyond `MERGEFIELD`, such as `ADDRESSBLOCK`,
    /// `GREETINGLINE`, `MERGEREC`, or `MERGESEQ`.
    MailMerge(String),
    /// Bibliography, citation, index, reference-document, and table-of-authorities
    /// fields. Simple literal `RD`, `TA`, and `XE` marker fields compute as
    /// hidden output; generated bibliography, index, and table-of-authorities
    /// fields preserve cached display text until native generation is broader.
    ReferenceIndex(String),
    /// Automatic numbering and list-number fields. Plain `AUTONUM` computes
    /// source-order values with common number-format switches and the
    /// documented separator switch, including unquoted or quoted one-character
    /// separators; standalone plain/neutral `AUTONUMLGL` and `AUTONUMOUT`
    /// compute on the same source-order counter; level-1 `LISTNUM
    /// NumberDefault`/`LegalDefault` computes source-order values with common
    /// number-format switches, neutral field-format switches, and starts/resets;
    /// richer outline/list fields preserve cached display text until native
    /// automatic-numbering evaluation is broader.
    Numbering(String),
    /// Document-structure fields such as `REVNUM`, `SECTION`, `SECTIONPAGES`,
    /// or `STYLEREF`. Deterministic structural/style-reference subsets compute;
    /// broader layout-derived cases preserve cached display text.
    DocumentStructure(String),
    /// Display and layout fields such as `ADVANCE`, `EQ`, or `SYMBOL`.
    /// Deterministic hidden-output or plain-text subsets compute; remaining
    /// visual/layout cases preserve cached display text.
    Display(String),
    /// Action and automation fields such as `GOTOBUTTON`, `MACROBUTTON`, or
    /// `PRINT`. Display-text or hidden-output subsets compute without executing
    /// navigation, macros, printer, or PostScript side effects.
    Action(String),
    /// Opaque compatibility/private fields whose cached text is preserved but
    /// whose payload is not interpreted, such as `PRIVATE`, `ADDIN`, `DATA`,
    /// `GLOSSARY`, or `HTMLACTIVEX`.
    Compatibility(String),
    /// Barcode-generation fields whose cached visible text is preserved but
    /// whose barcode image generation is not evaluated, such as
    /// `BARCODE`, `DISPLAYBARCODE`, or `MERGEBARCODE`.
    Barcode(String),
    /// Legacy Word form fields such as `FORMTEXT`, `FORMCHECKBOX`, or
    /// `FORMDROPDOWN`. Deterministic `w:ffData` current/default values compute;
    /// broader protected-form behavior preserves cached display text.
    FormField(String),
    /// Any field instruction whose first token is not one of rdoc's named
    /// classes yet.
    Unknown(String),
}

impl FieldKind {
    /// Classify a normalized or raw Word field instruction by its first token.
    pub fn from_instruction(instruction: &str) -> Self {
        let token = instruction
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_matches('"')
            .to_ascii_uppercase();
        match token.as_str() {
            "HYPERLINK" => FieldKind::Hyperlink,
            "PAGE" => FieldKind::Page,
            "TOC" => FieldKind::Toc,
            "FILENAME" => FieldKind::Filename,
            "MERGEFIELD" => FieldKind::MergeField,
            "REF" => FieldKind::Ref,
            "PAGEREF" => FieldKind::PageRef,
            "NOTEREF" | "FTNREF" => FieldKind::NoteRef,
            "TC" => FieldKind::TocEntry,
            "SEQ" => FieldKind::Sequence,
            _ if is_document_info_field(&token) => FieldKind::DocumentInfo(token),
            _ if is_dynamic_field(&token) => FieldKind::Dynamic(token),
            _ if is_inserted_content_field(&token) => FieldKind::InsertedContent(token),
            _ if is_mail_merge_field(&token) => FieldKind::MailMerge(token),
            _ if is_reference_index_field(&token) => FieldKind::ReferenceIndex(token),
            _ if is_numbering_field(&token) => FieldKind::Numbering(token),
            _ if is_document_structure_field(&token) => FieldKind::DocumentStructure(token),
            _ if is_display_field(&token) => FieldKind::Display(token),
            _ if is_action_field(&token) => FieldKind::Action(token),
            _ if is_compatibility_field(&token) => FieldKind::Compatibility(token),
            _ if is_barcode_field(&token) => FieldKind::Barcode(token),
            _ if is_legacy_form_field(&token) => FieldKind::FormField(token),
            _ => FieldKind::Unknown(token),
        }
    }

    /// Canonical field instruction name.
    pub fn as_str(&self) -> &str {
        match self {
            FieldKind::Hyperlink => "HYPERLINK",
            FieldKind::Page => "PAGE",
            FieldKind::Toc => "TOC",
            FieldKind::Filename => "FILENAME",
            FieldKind::MergeField => "MERGEFIELD",
            FieldKind::Ref => "REF",
            FieldKind::PageRef => "PAGEREF",
            FieldKind::NoteRef => "NOTEREF",
            FieldKind::TocEntry => "TC",
            FieldKind::Sequence => "SEQ",
            FieldKind::DocumentInfo(kind) => kind,
            FieldKind::Dynamic(kind) => kind,
            FieldKind::InsertedContent(kind) => kind,
            FieldKind::MailMerge(kind) => kind,
            FieldKind::ReferenceIndex(kind) => kind,
            FieldKind::Numbering(kind) => kind,
            FieldKind::DocumentStructure(kind) => kind,
            FieldKind::Display(kind) => kind,
            FieldKind::Action(kind) => kind,
            FieldKind::Compatibility(kind) => kind,
            FieldKind::Barcode(kind) => kind,
            FieldKind::FormField(kind) => kind,
            FieldKind::Unknown(kind) => kind,
        }
    }
}

pub(crate) fn is_neutral_field_format_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("MERGEFORMAT") || part.eq_ignore_ascii_case("CHARFORMAT")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldTextFormat {
    Upper,
    Lower,
    Caps,
    FirstCap,
}

pub(crate) fn field_text_format_switch(part: &str) -> Option<FieldTextFormat> {
    if part.eq_ignore_ascii_case("Upper") {
        Some(FieldTextFormat::Upper)
    } else if part.eq_ignore_ascii_case("Lower") {
        Some(FieldTextFormat::Lower)
    } else if part.eq_ignore_ascii_case("Caps") {
        Some(FieldTextFormat::Caps)
    } else if part.eq_ignore_ascii_case("FirstCap") {
        Some(FieldTextFormat::FirstCap)
    } else {
        None
    }
}

pub(crate) fn accept_field_text_format_switch(
    part: &str,
    text_format: &mut Option<FieldTextFormat>,
) -> bool {
    if is_neutral_field_format_switch(part) {
        return true;
    }
    field_text_format_switch(part).is_some_and(|format| text_format.replace(format).is_none())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FieldNumberFormat {
    Arabic,
    ArabicDash,
    AlphabeticLower,
    AlphabeticUpper,
    RomanLower,
    RomanUpper,
    Ordinal,
    CardText,
    OrdText,
}

pub(crate) fn field_number_format_switch(part: &str) -> Option<FieldNumberFormat> {
    match part {
        _ if part.eq_ignore_ascii_case("Arabic") => Some(FieldNumberFormat::Arabic),
        "alphabetic" => Some(FieldNumberFormat::AlphabeticLower),
        "ALPHABETIC" => Some(FieldNumberFormat::AlphabeticUpper),
        "roman" => Some(FieldNumberFormat::RomanLower),
        "ROMAN" => Some(FieldNumberFormat::RomanUpper),
        _ if part.eq_ignore_ascii_case("Ordinal") => Some(FieldNumberFormat::Ordinal),
        _ if part.eq_ignore_ascii_case("CardText") => Some(FieldNumberFormat::CardText),
        _ if part.eq_ignore_ascii_case("OrdText") => Some(FieldNumberFormat::OrdText),
        _ if part.eq_ignore_ascii_case("ArabicDash") => Some(FieldNumberFormat::ArabicDash),
        _ => None,
    }
}

pub(crate) fn accept_field_number_format_switch(
    part: &str,
    number_format: &mut Option<FieldNumberFormat>,
) -> bool {
    if is_neutral_field_format_switch(part) {
        return true;
    }
    field_number_format_switch(part).is_some_and(|format| number_format.replace(format).is_none())
}

pub(crate) fn accept_general_format_switch<'a, I>(
    part: &'a str,
    parts: &mut I,
    accept: impl FnOnce(&str) -> bool,
) -> Option<bool>
where
    I: Iterator<Item = &'a str>,
{
    if part == "\\*" {
        return accept(parts.next()?).then_some(true);
    }
    if let Some(format) = part.strip_prefix("\\*") {
        return accept(format).then_some(true);
    }
    Some(false)
}

pub(crate) fn is_toc_value_neutral_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("\\h")
        || part.eq_ignore_ascii_case("\\z")
        || part.eq_ignore_ascii_case("\\w")
        || part.eq_ignore_ascii_case("\\x")
}

pub(crate) fn is_ref_value_neutral_switch(part: &str) -> bool {
    part.eq_ignore_ascii_case("\\h")
}

pub(crate) fn is_note_ref_kind(kind: &str) -> bool {
    kind.eq_ignore_ascii_case("NOTEREF") || kind.eq_ignore_ascii_case("FTNREF")
}

pub(crate) fn strip_ascii_switch_prefix<'a>(part: &'a str, switch: &str) -> Option<&'a str> {
    let prefix = part.get(..switch.len())?;
    prefix
        .eq_ignore_ascii_case(switch)
        .then_some(&part[switch.len()..])
}

pub(crate) fn field_name_token(value: &str) -> Option<&str> {
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

pub(crate) fn field_literal_token(value: &str) -> Option<&str> {
    let value = value.trim();
    let value = match (value.starts_with('"'), value.ends_with('"')) {
        (true, true) if value.len() >= 2 => &value[1..value.len() - 1],
        (true, _) | (_, true) => return None,
        (false, false) => value,
    };
    (!value.contains('"')).then_some(value)
}

pub(crate) fn instruction_parts(s: &str) -> Vec<String> {
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

fn is_document_info_field(token: &str) -> bool {
    matches!(
        token,
        "APPLICATION"
            | "APPVERSION"
            | "AUTHOR"
            | "CATEGORY"
            | "CHARACTERS"
            | "CHARACTERSWITHSPACES"
            | "COMMENT"
            | "COMMENTS"
            | "COMPANY"
            | "CONTENTSTATUS"
            | "CREATOR"
            | "CREATEDATE"
            | "DATE"
            | "DESCRIPTION"
            | "DOCPROPERTY"
            | "DOCVARIABLE"
            | "DOCSECURITY"
            | "EDITTIME"
            | "FILESIZE"
            | "HIDDENSLIDES"
            | "HYPERLINKBASE"
            | "HYPERLINKSCHANGED"
            | "INFO"
            | "KEYWORD"
            | "KEYWORDS"
            | "LASTMODIFIEDBY"
            | "LASTSAVEDBY"
            | "LINES"
            | "LINKSUPTODATE"
            | "MANAGER"
            | "MMCLIPS"
            | "NOTES"
            | "NUMCHARS"
            | "NUMPAGES"
            | "NUMWORDS"
            | "PAGES"
            | "PARAGRAPHS"
            | "PRINTDATE"
            | "PRESENTATIONFORMAT"
            | "SCALECROP"
            | "SAVEDATE"
            | "SHAREDDOC"
            | "SLIDES"
            | "SUBJECT"
            | "TEMPLATE"
            | "TIME"
            | "TITLE"
            | "TOTALTIME"
            | "USERADDRESS"
            | "USERINITIALS"
            | "USERNAME"
            | "VERSION"
            | "WORDS"
    )
}

fn is_dynamic_field(token: &str) -> bool {
    matches!(
        token,
        "=" | "ASK" | "COMPARE" | "FILLIN" | "IF" | "NEXT" | "NEXTIF" | "QUOTE" | "SET" | "SKIPIF"
    )
}

fn is_inserted_content_field(token: &str) -> bool {
    matches!(
        token,
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

fn is_mail_merge_field(token: &str) -> bool {
    matches!(
        token,
        "ADDRESSBLOCK" | "GREETINGLINE" | "MERGEREC" | "MERGESEQ"
    )
}

fn is_reference_index_field(token: &str) -> bool {
    matches!(
        token,
        "BIBLIOGRAPHY" | "CITATION" | "INDEX" | "RD" | "TA" | "TOA" | "XE"
    )
}

fn is_numbering_field(token: &str) -> bool {
    matches!(
        token,
        "AUTONUM" | "AUTONUMLGL" | "AUTONUMOUT" | "BIDIOUTLINE" | "LISTNUM"
    )
}

fn is_document_structure_field(token: &str) -> bool {
    matches!(token, "REVNUM" | "SECTION" | "SECTIONPAGES" | "STYLEREF")
}

fn is_display_field(token: &str) -> bool {
    matches!(token, "ADVANCE" | "EQ" | "SYMBOL")
}

fn is_action_field(token: &str) -> bool {
    matches!(token, "GOTOBUTTON" | "MACROBUTTON" | "PRINT")
}

fn is_compatibility_field(token: &str) -> bool {
    matches!(
        token,
        "ADDIN" | "DATA" | "GLOSSARY" | "HTMLACTIVEX" | "PRIVATE"
    )
}

fn is_barcode_field(token: &str) -> bool {
    matches!(token, "BARCODE" | "DISPLAYBARCODE" | "MERGEBARCODE")
}

fn is_legacy_form_field(token: &str) -> bool {
    matches!(token, "FORMTEXT" | "FORMCHECKBOX" | "FORMDROPDOWN")
}

/// A Word field observed in a `.docx` body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Field {
    /// Field kind derived from the first instruction token.
    pub kind: FieldKind,
    /// Normalized field instruction text.
    pub instruction: String,
    /// Cached visible result text stored in the document.
    pub result: String,
    /// Value rdoc computed from document semantics, when evaluation is supported
    /// and unambiguous. Cached result text remains available in [`Field::result`].
    pub computed_result: Option<String>,
}

impl Default for FieldKind {
    fn default() -> Self {
        FieldKind::Unknown(String::new())
    }
}

/// Kind of tracked revision marker observed in a `.docx` body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionKind {
    /// `w:ins`.
    Insertion,
    /// `w:del`.
    Deletion,
    /// `w:moveFrom`.
    MoveFrom,
    /// `w:moveTo`.
    MoveTo,
}

/// Flat text view policy for tracked revisions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevisionView {
    /// Current accepted view: include insertions and move destinations; exclude
    /// deletions and move sources.
    Accepted,
    /// Original view: include deletions and move sources; exclude insertions and
    /// move destinations.
    Original,
    /// Annotated view: keep both sides with compact textual markers.
    Annotated,
}

/// A tracked revision extracted from a `.docx` body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Revision kind.
    pub kind: RevisionKind,
    /// Revision id (`w:id`), if present.
    pub id: Option<String>,
    /// Revision author (`w:author`), if present.
    pub author: Option<String>,
    /// Revision timestamp (`w:date`), if present.
    pub date: Option<String>,
    /// Visible text contained in the revision subtree.
    pub text: String,
}
