//! `.docx` style sheet (`word/styles.xml`) → paragraph-style heading levels,
//! display names, and inherited run defaults, the OOXML analogue of the `.doc`
//! STSH resolver (`stsh.rs`).
//!
//! A heading level is derived from the `w:styleId` (`Heading1`…), the localized
//! `w:name` (`heading 1` / `제목 1`), or the style's own `w:outlineLvl` — reusing
//! [`crate::stsh::heading_from_name`] so both backends recognize the same names.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::xml_text::{skip_alternate_content_branch, skip_subtree, AlternateContentBranchState};
use super::{
    attr_local, attr_local_trimmed, attr_u16, attr_u8, local, parse_rgb_hex_color, toggle_on,
};
use crate::model::{CharProps, Color, TabAlignment, TabStop, VertAlign, MAX_TAB_STOPS};
use crate::stsh::heading_from_name;

const STYLE_CHAIN_LIMIT: usize = 32;

/// Resolved per-`styleId` heading level, display name, and run defaults.
#[derive(Debug, Default)]
pub(crate) struct Styles {
    heading: HashMap<String, u8>,
    name: HashMap<String, String>,
    doc_defaults_run: RunProps,
    doc_defaults_paragraph: ParagraphProps,
    paragraph_run: HashMap<String, RunProps>,
    paragraph: HashMap<String, ParagraphProps>,
    character_run: HashMap<String, RunProps>,
    table_row: HashMap<String, TableRowStyleProps>,
}

impl Styles {
    /// Heading level (1–9) for a paragraph `styleId`, or `None` for body styles.
    pub(crate) fn heading_level(&self, style_id: &str) -> Option<u8> {
        self.heading.get(style_id).copied()
    }

    /// Display name for a `styleId` (e.g. `heading 1`, `제목 1`), if known.
    pub(crate) fn name(&self, style_id: &str) -> Option<&str> {
        self.name
            .get(style_id)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    pub(crate) fn resolved_run_props(
        &self,
        paragraph_style_id: Option<&str>,
        character_style_id: Option<&str>,
    ) -> RunProps {
        let mut props = self.doc_defaults_run.clone();
        if let Some(style_id) = paragraph_style_id {
            if let Some(style_props) = self.paragraph_run.get(style_id) {
                props.overlay(style_props);
            }
        }
        if let Some(style_id) = character_style_id {
            if let Some(style_props) = self.character_run.get(style_id) {
                props.overlay(style_props);
            }
        }
        props
    }

    pub(crate) fn paragraph_props(&self, style_id: Option<&str>) -> ParagraphProps {
        let mut props = self.doc_defaults_paragraph.clone();
        if let Some(style_id) = style_id {
            if let Some(style_props) = self.paragraph.get(style_id) {
                props.overlay(style_props);
            }
        }
        props
    }

    #[cfg(test)]
    fn table_row_cant_split(&self, style_id: Option<&str>) -> Option<bool> {
        style_id
            .and_then(|style_id| self.table_row.get(style_id))
            .and_then(|props| props.direct.cant_split)
    }

    pub(crate) fn table_row_cant_split_for_regions(
        &self,
        style_id: Option<&str>,
        regions: TableRowStyleRegions,
    ) -> Option<bool> {
        let props = style_id.and_then(|style_id| self.table_row.get(style_id))?;
        let mut value = props.direct.cant_split;
        // ISO/IEC 29500-1 17.7.6.6: later matching conditional regions
        // override earlier ones; direct row formatting is applied by body.rs.
        overlay_cant_split(&mut value, props.whole_table);
        if regions.band1_horizontal {
            overlay_cant_split(&mut value, props.band1_horizontal);
        }
        if regions.band2_horizontal {
            overlay_cant_split(&mut value, props.band2_horizontal);
        }
        if regions.first_row {
            overlay_cant_split(&mut value, props.first_row);
        }
        if regions.last_row {
            overlay_cant_split(&mut value, props.last_row);
        }
        value
    }

    pub(crate) fn table_row_band_size(&self, style_id: Option<&str>) -> Option<u8> {
        style_id
            .and_then(|style_id| self.table_row.get(style_id))
            .and_then(|props| props.row_band_size)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TableRowStyleRegions {
    pub(crate) first_row: bool,
    pub(crate) last_row: bool,
    pub(crate) band1_horizontal: bool,
    pub(crate) band2_horizontal: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct ParagraphProps {
    pub(crate) bidi: Option<bool>,
    pub(crate) keep_next: Option<bool>,
    pub(crate) keep_lines: Option<bool>,
    pub(crate) widow_control: Option<bool>,
    pub(crate) jc: Option<String>,
    pub(crate) indent_left_pt: Option<f32>,
    pub(crate) indent_right_pt: Option<f32>,
    pub(crate) indent_start_pt: Option<f32>,
    pub(crate) indent_end_pt: Option<f32>,
    pub(crate) tab_stops: Vec<TabStop>,
}

impl ParagraphProps {
    fn overlay(&mut self, other: &ParagraphProps) {
        if other.bidi.is_some() {
            self.bidi = other.bidi;
        }
        if other.keep_next.is_some() {
            self.keep_next = other.keep_next;
        }
        if other.keep_lines.is_some() {
            self.keep_lines = other.keep_lines;
        }
        if other.widow_control.is_some() {
            self.widow_control = other.widow_control;
        }
        if other.jc.is_some() {
            self.jc = other.jc.clone();
        }
        if other.indent_left_pt.is_some() {
            self.indent_left_pt = other.indent_left_pt;
        }
        if other.indent_right_pt.is_some() {
            self.indent_right_pt = other.indent_right_pt;
        }
        if other.indent_start_pt.is_some() {
            self.indent_start_pt = other.indent_start_pt;
        }
        if other.indent_end_pt.is_some() {
            self.indent_end_pt = other.indent_end_pt;
        }
        let remaining = MAX_TAB_STOPS.saturating_sub(self.tab_stops.len());
        self.tab_stops
            .extend(other.tab_stops.iter().take(remaining).copied());
    }
}

fn twips_attr(e: &BytesStart<'_>, name: &[u8]) -> Option<f32> {
    attr_local(e, name)
        .and_then(|value| value.trim().parse::<f32>().ok())
        .filter(|value| value.is_finite())
        .map(|value| value / 20.0)
}

pub(super) fn tab_stop(e: &BytesStart<'_>) -> Option<TabStop> {
    let position_pt = twips_attr(e, b"pos").filter(|position| *position >= 0.0)?;
    let alignment = match attr_local_trimmed(e, b"val").as_deref() {
        None | Some("left") | Some("start") => TabAlignment::Left,
        Some("center") => TabAlignment::Center,
        Some("right") | Some("end") => TabAlignment::Right,
        Some("decimal") | Some("num") => TabAlignment::Decimal,
        Some("clear") => TabAlignment::Clear,
        _ => return None,
    };
    Some(TabStop {
        position_pt,
        alignment,
    })
}

fn apply_paragraph_props_child(props: &mut ParagraphProps, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"bidi" => props.bidi = Some(toggle_on(attr_local(e, b"val"))),
        b"keepNext" => props.keep_next = Some(toggle_on(attr_local(e, b"val"))),
        b"keepLines" => props.keep_lines = Some(toggle_on(attr_local(e, b"val"))),
        b"widowControl" => props.widow_control = Some(toggle_on(attr_local(e, b"val"))),
        b"jc" => props.jc = attr_local_trimmed(e, b"val"),
        b"ind" => {
            props.indent_left_pt = twips_attr(e, b"left");
            props.indent_right_pt = twips_attr(e, b"right");
            props.indent_start_pt = twips_attr(e, b"start");
            props.indent_end_pt = twips_attr(e, b"end");
        }
        b"tab" if props.tab_stops.len() < MAX_TAB_STOPS => {
            if let Some(tab) = tab_stop(e) {
                props.tab_stops.push(tab);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RunProps {
    bold: Option<bool>,
    italic: Option<bool>,
    underline: Option<bool>,
    strike: Option<bool>,
    hidden: Option<bool>,
    font: Option<String>,
    font_cs: Option<String>,
    size_half_pt: Option<u16>,
    color: Option<Color>,
    highlight: Option<String>,
    vert_align: Option<VertAlign>,
    small_caps: Option<bool>,
    caps: Option<bool>,
    rtl: Option<bool>,
}

impl RunProps {
    pub(crate) fn apply_to(&self, props: &mut CharProps) {
        if let Some(value) = self.bold {
            props.bold = value;
        }
        if let Some(value) = self.italic {
            props.italic = value;
        }
        if let Some(value) = self.underline {
            props.underline = value;
        }
        if let Some(value) = self.strike {
            props.strike = value;
        }
        if let Some(value) = self.hidden {
            props.hidden = value;
        }
        if let Some(value) = &self.font {
            props.font = Some(value.clone());
        }
        if let Some(value) = self.size_half_pt {
            props.size_half_pt = Some(value);
        }
        if let Some(value) = self.color {
            props.color = Some(value);
        }
        if let Some(value) = &self.highlight {
            props.highlight = Some(value.clone());
        }
        if let Some(value) = self.vert_align {
            props.vert_align = value;
        }
        if let Some(value) = self.small_caps {
            props.small_caps = value;
        }
        if let Some(value) = self.caps {
            props.caps = value;
        }
        if let Some(value) = self.rtl {
            props.rtl = value;
        }
        if props.rtl {
            if let Some(value) = &self.font_cs {
                props.font = Some(value.clone());
            }
        }
    }

    pub(crate) fn overlay(&mut self, other: &RunProps) {
        if other.bold.is_some() {
            self.bold = other.bold;
        }
        if other.italic.is_some() {
            self.italic = other.italic;
        }
        if other.underline.is_some() {
            self.underline = other.underline;
        }
        if other.strike.is_some() {
            self.strike = other.strike;
        }
        if other.hidden.is_some() {
            self.hidden = other.hidden;
        }
        if other.font.is_some() {
            self.font = other.font.clone();
        }
        if other.font_cs.is_some() {
            self.font_cs = other.font_cs.clone();
        }
        if other.size_half_pt.is_some() {
            self.size_half_pt = other.size_half_pt;
        }
        if other.color.is_some() {
            self.color = other.color;
        }
        if other.highlight.is_some() {
            self.highlight = other.highlight.clone();
        }
        if other.vert_align.is_some() {
            self.vert_align = other.vert_align;
        }
        if other.small_caps.is_some() {
            self.small_caps = other.small_caps;
        }
        if other.caps.is_some() {
            self.caps = other.caps;
        }
        if other.rtl.is_some() {
            self.rtl = other.rtl;
        }
    }
}

pub(crate) fn apply_run_props_child(props: &mut RunProps, e: &BytesStart<'_>) {
    match local(e.name().as_ref()) {
        b"b" => props.bold = Some(toggle_on(attr_local(e, b"val"))),
        b"i" => props.italic = Some(toggle_on(attr_local(e, b"val"))),
        b"strike" | b"dstrike" => props.strike = Some(toggle_on(attr_local(e, b"val"))),
        b"vanish" => props.hidden = Some(toggle_on(attr_local(e, b"val"))),
        b"u" => {
            props.underline = Some(
                attr_local(e, b"val")
                    .map(|v| v.trim() != "none")
                    .unwrap_or(true),
            )
        }
        b"smallCaps" => props.small_caps = Some(toggle_on(attr_local(e, b"val"))),
        b"caps" => props.caps = Some(toggle_on(attr_local(e, b"val"))),
        b"rtl" => props.rtl = Some(toggle_on(attr_local(e, b"val"))),
        b"rFonts" => {
            props.font =
                attr_local_trimmed(e, b"eastAsia").or_else(|| attr_local_trimmed(e, b"ascii"));
            props.font_cs = attr_local_trimmed(e, b"cs");
        }
        b"sz" => props.size_half_pt = attr_u16(e, b"val"),
        b"color" => props.color = attr_local(e, b"val").and_then(|v| parse_rgb_hex_color(&v)),
        b"highlight" => props.highlight = attr_local_trimmed(e, b"val"),
        b"vertAlign" => {
            props.vert_align = Some(match attr_local_trimmed(e, b"val").as_deref() {
                Some("superscript") => VertAlign::Super,
                Some("subscript") => VertAlign::Sub,
                _ => VertAlign::Baseline,
            });
        }
        _ => {}
    }
}

/// Parse `word/styles.xml`. Returns an empty sheet on absence/malformation —
/// headings then simply aren't detected (lists/body text are unaffected).
pub(crate) fn parse(xml: &str) -> Styles {
    let mut r = Reader::from_str(xml);
    let mut styles = Styles::default();
    let mut raw_styles: HashMap<String, RawStyle> = HashMap::new();
    // State for the style currently being parsed.
    let mut cur_style: Option<RawStyle> = None;
    let mut in_doc_defaults = false;
    let mut in_rpr_default = false;
    let mut in_ppr_default = false;
    let mut alternate_content_stack = Vec::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) =>
            {
                skip_subtree(&mut r);
            }
            Ok(Event::Empty(e))
                if skip_alternate_content_branch(
                    &mut alternate_content_stack,
                    local(e.name().as_ref()),
                ) => {}
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.push(AlternateContentBranchState::default());
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"docDefaults" => {
                in_doc_defaults = true;
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"rPrDefault" => {
                in_rpr_default = true;
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"pPrDefault" => {
                in_ppr_default = true;
            }
            // A new <w:style> opens; capture its id and reset per-style state.
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"style" => {
                cur_style = attr_local_trimmed(&e, b"styleId").map(|id| RawStyle {
                    id,
                    kind: StyleKind::from_attr(attr_local_trimmed(&e, b"type").as_deref()),
                    ..RawStyle::default()
                });
            }
            Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"style" => {
                if let Some(id) = attr_local_trimmed(&e, b"styleId") {
                    raw_styles.insert(
                        id.clone(),
                        RawStyle {
                            id,
                            kind: StyleKind::from_attr(attr_local_trimmed(&e, b"type").as_deref()),
                            ..RawStyle::default()
                        },
                    );
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"tblStylePr" => {
                let region =
                    TableRowStyleRegion::from_attr(attr_local_trimmed(&e, b"type").as_deref());
                if let (Some(style), Some(region)) = (&mut cur_style, region) {
                    if style.kind == Some(StyleKind::Table) {
                        let props = read_conditional_table_row_props(&mut r, b"tblStylePr");
                        style.table_row_props.region_mut(region).overlay(props);
                    } else {
                        skip_subtree(&mut r);
                    }
                } else {
                    skip_subtree(&mut r);
                }
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"trPr" => {
                if let Some(style) = &mut cur_style {
                    if style.kind == Some(StyleKind::Table) {
                        style
                            .table_row_props
                            .direct
                            .overlay(read_table_row_props(&mut r, b"trPr"));
                    } else {
                        skip_subtree(&mut r);
                    }
                } else {
                    skip_subtree(&mut r);
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                // An empty `<w:rPr/>` carries no run properties, so both rPr targets
                // (doc defaults, current style) only act on a non-empty element. Merging
                // the two rPr arms behind one `!e.is_empty()` guard keeps the original
                // arm priority (doc-defaults wins over an open style) while dropping the
                // nested single-branch `if` clippy flagged.
                b"rPr" if !e.is_empty() => {
                    if in_doc_defaults && in_rpr_default {
                        styles.doc_defaults_run = read_run_props(&mut r, b"rPr");
                    } else if let Some(style) = &mut cur_style {
                        style.run_props = read_run_props(&mut r, b"rPr");
                    }
                }
                b"bidi" | b"keepNext" | b"keepLines" | b"widowControl" | b"jc" | b"ind"
                | b"tab" => {
                    if in_doc_defaults && in_ppr_default {
                        apply_paragraph_props_child(&mut styles.doc_defaults_paragraph, &e);
                    } else if let Some(style) = &mut cur_style {
                        apply_paragraph_props_child(&mut style.paragraph_props, &e);
                    }
                }
                b"name" => {
                    if let Some(v) = attr_local_trimmed(&e, b"val") {
                        if let Some(style) = &mut cur_style {
                            style.name = v;
                        }
                    }
                }
                b"basedOn" => {
                    if let Some(v) = attr_local_trimmed(&e, b"val") {
                        if let Some(style) = &mut cur_style {
                            style.based_on = Some(v);
                        }
                    }
                }
                // The style's own paragraph outline level (in its <w:pPr>).
                b"outlineLvl" => {
                    if let Some(style) = &mut cur_style {
                        style.outline = attr_u8(&e, b"val");
                    }
                }
                b"tblStyleRowBandSize" => {
                    if let Some(style) = &mut cur_style {
                        if style.kind == Some(StyleKind::Table) {
                            if let Some(size) = attr_u8(&e, b"val").filter(|size| *size <= 3) {
                                style.table_row_props.row_band_size = Some(size);
                            }
                        }
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"style" => {
                if let Some(style) = cur_style.take() {
                    raw_styles.insert(style.id.clone(), style);
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"rPrDefault" => {
                in_rpr_default = false;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"pPrDefault" => {
                in_ppr_default = false;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"docDefaults" => {
                in_doc_defaults = false;
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                alternate_content_stack.pop();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    for style in raw_styles.values() {
        let level = heading_from_name(&style.id)
            .or_else(|| heading_from_name(&style.name))
            .or_else(|| style.outline.filter(|&o| o <= 8).map(|o| o + 1));
        if let Some(level) = level {
            styles.heading.insert(style.id.clone(), level);
        }
        if !style.name.is_empty() {
            styles.name.insert(style.id.clone(), style.name.clone());
        }
    }
    let paragraph_ids = raw_styles
        .iter()
        .filter(|(_, style)| style.kind == Some(StyleKind::Paragraph))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let character_ids = raw_styles
        .iter()
        .filter(|(_, style)| style.kind == Some(StyleKind::Character))
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut paragraph_cache = HashMap::new();
    let mut paragraph_props_cache = HashMap::new();
    for id in paragraph_ids {
        let props = resolve_style_run_props(
            &id,
            StyleKind::Paragraph,
            &raw_styles,
            &mut paragraph_cache,
            &mut Vec::new(),
            0,
        );
        styles.paragraph_run.insert(id.clone(), props);
        let paragraph_props = resolve_style_paragraph_props(
            &id,
            &raw_styles,
            &mut paragraph_props_cache,
            &mut Vec::new(),
            0,
        );
        styles.paragraph.insert(id, paragraph_props);
    }
    let mut character_cache = HashMap::new();
    for id in character_ids {
        let props = resolve_style_run_props(
            &id,
            StyleKind::Character,
            &raw_styles,
            &mut character_cache,
            &mut Vec::new(),
            0,
        );
        styles.character_run.insert(id, props);
    }
    for (id, _) in raw_styles
        .iter()
        .filter(|(_, style)| style.kind == Some(StyleKind::Table))
    {
        let props = resolve_style_table_row_props(id, &raw_styles, &mut Vec::new(), 0);
        if props.has_any() {
            styles.table_row.insert(id.clone(), props);
        }
    }
    styles
}

#[derive(Debug, Default)]
struct RawStyle {
    id: String,
    kind: Option<StyleKind>,
    name: String,
    based_on: Option<String>,
    outline: Option<u8>,
    run_props: RunProps,
    paragraph_props: ParagraphProps,
    table_row_props: TableRowStyleProps,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Paragraph,
    Character,
    Table,
}

impl StyleKind {
    fn from_attr(value: Option<&str>) -> Option<Self> {
        match value {
            Some("paragraph") => Some(Self::Paragraph),
            Some("character") => Some(Self::Character),
            Some("table") => Some(Self::Table),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct TableRowProps {
    cant_split: Option<bool>,
}

impl TableRowProps {
    fn overlay(&mut self, other: Self) {
        if other.cant_split.is_some() {
            self.cant_split = other.cant_split;
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct TableRowStyleProps {
    direct: TableRowProps,
    whole_table: TableRowProps,
    band1_horizontal: TableRowProps,
    band2_horizontal: TableRowProps,
    first_row: TableRowProps,
    last_row: TableRowProps,
    row_band_size: Option<u8>,
}

impl TableRowStyleProps {
    fn has_any(self) -> bool {
        self.direct.cant_split.is_some()
            || self.whole_table.cant_split.is_some()
            || self.band1_horizontal.cant_split.is_some()
            || self.band2_horizontal.cant_split.is_some()
            || self.first_row.cant_split.is_some()
            || self.last_row.cant_split.is_some()
    }

    fn overlay(&mut self, other: Self) {
        self.direct.overlay(other.direct);
        self.whole_table.overlay(other.whole_table);
        self.band1_horizontal.overlay(other.band1_horizontal);
        self.band2_horizontal.overlay(other.band2_horizontal);
        self.first_row.overlay(other.first_row);
        self.last_row.overlay(other.last_row);
        if other.row_band_size.is_some() {
            self.row_band_size = other.row_band_size;
        }
    }

    fn region_mut(&mut self, region: TableRowStyleRegion) -> &mut TableRowProps {
        match region {
            TableRowStyleRegion::WholeTable => &mut self.whole_table,
            TableRowStyleRegion::Band1Horizontal => &mut self.band1_horizontal,
            TableRowStyleRegion::Band2Horizontal => &mut self.band2_horizontal,
            TableRowStyleRegion::FirstRow => &mut self.first_row,
            TableRowStyleRegion::LastRow => &mut self.last_row,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum TableRowStyleRegion {
    WholeTable,
    Band1Horizontal,
    Band2Horizontal,
    FirstRow,
    LastRow,
}

impl TableRowStyleRegion {
    fn from_attr(value: Option<&str>) -> Option<Self> {
        match value {
            Some("wholeTable") => Some(Self::WholeTable),
            Some("band1Horz") => Some(Self::Band1Horizontal),
            Some("band2Horz") => Some(Self::Band2Horizontal),
            Some("firstRow") => Some(Self::FirstRow),
            Some("lastRow") => Some(Self::LastRow),
            _ => None,
        }
    }
}

fn overlay_cant_split(value: &mut Option<bool>, props: TableRowProps) {
    if props.cant_split.is_some() {
        *value = props.cant_split;
    }
}

fn resolve_style_run_props(
    id: &str,
    kind: StyleKind,
    raw_styles: &HashMap<String, RawStyle>,
    cache: &mut HashMap<String, RunProps>,
    stack: &mut Vec<String>,
    depth: usize,
) -> RunProps {
    if let Some(props) = cache.get(id) {
        return props.clone();
    }
    if depth >= STYLE_CHAIN_LIMIT || stack.iter().any(|seen| seen == id) {
        return RunProps::default();
    }
    let Some(style) = raw_styles.get(id).filter(|style| style.kind == Some(kind)) else {
        return RunProps::default();
    };

    stack.push(id.to_string());
    let mut props = style
        .based_on
        .as_deref()
        .map(|base| resolve_style_run_props(base, kind, raw_styles, cache, stack, depth + 1))
        .unwrap_or_default();
    props.overlay(&style.run_props);
    stack.pop();

    cache.insert(id.to_string(), props.clone());
    props
}

fn resolve_style_paragraph_props(
    id: &str,
    raw_styles: &HashMap<String, RawStyle>,
    cache: &mut HashMap<String, ParagraphProps>,
    stack: &mut Vec<String>,
    depth: usize,
) -> ParagraphProps {
    if let Some(props) = cache.get(id) {
        return props.clone();
    }
    if depth >= STYLE_CHAIN_LIMIT || stack.iter().any(|seen| seen == id) {
        return ParagraphProps::default();
    }
    let Some(style) = raw_styles
        .get(id)
        .filter(|style| style.kind == Some(StyleKind::Paragraph))
    else {
        return ParagraphProps::default();
    };

    stack.push(id.to_string());
    let mut props = style
        .based_on
        .as_deref()
        .map(|base| resolve_style_paragraph_props(base, raw_styles, cache, stack, depth + 1))
        .unwrap_or_default();
    props.overlay(&style.paragraph_props);
    stack.pop();

    cache.insert(id.to_string(), props.clone());
    props
}

fn resolve_style_table_row_props(
    id: &str,
    raw_styles: &HashMap<String, RawStyle>,
    stack: &mut Vec<String>,
    depth: usize,
) -> TableRowStyleProps {
    if depth >= STYLE_CHAIN_LIMIT || stack.iter().any(|seen| seen == id) {
        return TableRowStyleProps::default();
    }
    let Some(style) = raw_styles
        .get(id)
        .filter(|style| style.kind == Some(StyleKind::Table))
    else {
        return TableRowStyleProps::default();
    };

    stack.push(id.to_string());
    let mut props = style
        .based_on
        .as_deref()
        .map(|base| resolve_style_table_row_props(base, raw_styles, stack, depth + 1))
        .unwrap_or_default();
    props.overlay(style.table_row_props);
    stack.pop();
    props
}

fn read_conditional_table_row_props(r: &mut Reader<&[u8]>, end: &[u8]) -> TableRowProps {
    let mut props = TableRowProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"trPr" => {
                props.overlay(read_table_row_props(r, b"trPr"));
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_conditional_table_row_props_alternate_content(r) {
                    props.overlay(value);
                }
            }
            Ok(Event::Start(_)) => skip_subtree(r),
            Ok(Event::End(e)) if local(e.name().as_ref()) == end => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_conditional_table_row_props_alternate_content(
    r: &mut Reader<&[u8]>,
) -> Option<TableRowProps> {
    let mut took = false;
    let mut props = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        props = Some(read_conditional_table_row_props(r, name));
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_table_row_props(r: &mut Reader<&[u8]>, end: &[u8]) -> TableRowProps {
    let mut props = TableRowProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"trPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                if let Some(value) = read_table_row_props_alternate_content(r) {
                    props.overlay(value);
                }
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e))
                if local(e.name().as_ref()) == b"cantSplit" =>
            {
                props.cant_split = Some(toggle_on(attr_local(&e, b"val")));
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == end => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_table_row_props_alternate_content(r: &mut Reader<&[u8]>) -> Option<TableRowProps> {
    let mut took = false;
    let mut props = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        props = Some(read_table_row_props(r, name));
                    }
                    _ => skip_subtree(r),
                }
            }
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"AlternateContent" => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_run_props(r: &mut Reader<&[u8]>, end: &[u8]) -> RunProps {
    let mut props = RunProps::default();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"rPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_run_props_alternate_content(r, &mut props);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_run_props_child(&mut props, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == end => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    props
}

fn read_run_props_alternate_content(r: &mut Reader<&[u8]>, props: &mut RunProps) {
    let mut took = false;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let qname = e.name();
                let name = local(qname.as_ref());
                match name {
                    b"Choice" | b"Fallback" if !took => {
                        took = true;
                        read_run_props_alternate_content_branch(r, props, name);
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

fn read_run_props_alternate_content_branch(
    r: &mut Reader<&[u8]>,
    props: &mut RunProps,
    branch: &[u8],
) {
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"rPrChange" => {
                skip_subtree(r);
            }
            Ok(Event::Start(e)) if local(e.name().as_ref()) == b"AlternateContent" => {
                read_run_props_alternate_content(r, props);
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => apply_run_props_child(props, &e),
            Ok(Event::End(e)) if local(e.name().as_ref()) == branch => break,
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_from_style_id_name_and_outline() {
        let xml = r#"<w:styles>
            <w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style>
            <w:style w:type="paragraph" w:styleId="KrTitle"><w:name w:val="제목 2"/></w:style>
            <w:style w:type="paragraph" w:styleId="CustomH"><w:name w:val="MyStyle"/>
                <w:pPr><w:outlineLvl w:val=" 2 "/></w:pPr></w:style>
            <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
        </w:styles>"#;
        let s = parse(xml);
        assert_eq!(s.heading_level("Heading1"), Some(1));
        assert_eq!(s.heading_level("KrTitle"), Some(2)); // 제목 2
        assert_eq!(s.heading_level("CustomH"), Some(3)); // outlineLvl 2 → h3
        assert_eq!(s.heading_level("Normal"), None);
        assert_eq!(s.name("KrTitle"), Some("제목 2"));
    }

    #[test]
    fn uses_single_alternate_content_branch() {
        let xml = r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <mc:AlternateContent>
                <mc:Choice Requires="w14">
                    <w:style w:type="paragraph" w:styleId="ChoiceHeading"><w:name w:val="heading 1"/></w:style>
                </mc:Choice>
                <mc:Fallback>
                    <w:style w:type="paragraph" w:styleId="FallbackHeading"><w:name w:val="heading 1"/></w:style>
                </mc:Fallback>
            </mc:AlternateContent>
        </w:styles>"#;
        let s = parse(xml);

        assert_eq!(s.heading_level("ChoiceHeading"), Some(1));
        assert_eq!(s.name("ChoiceHeading"), Some("heading 1"));
        assert_eq!(s.heading_level("FallbackHeading"), None);
        assert_eq!(s.name("FallbackHeading"), None);
    }

    #[test]
    fn resolves_inherited_paragraph_direction_alignment_and_logical_indents() {
        let xml = r#"<w:styles>
            <w:docDefaults><w:pPrDefault><w:pPr><w:jc w:val="left"/></w:pPr></w:pPrDefault></w:docDefaults>
            <w:style w:type="paragraph" w:styleId="RtlBase">
                <w:pPr><w:bidi/><w:jc w:val="start"/><w:ind w:start="720" w:end="1440"/></w:pPr>
            </w:style>
            <w:style w:type="paragraph" w:styleId="RtlDerived"><w:basedOn w:val="RtlBase"/></w:style>
            <w:style w:type="paragraph" w:styleId="LtrOverride"><w:basedOn w:val="RtlBase"/>
                <w:pPr><w:bidi w:val="0"/><w:jc w:val="end"/></w:pPr>
            </w:style>
        </w:styles>"#;
        let styles = parse(xml);
        let defaults = styles.paragraph_props(None);
        let inherited = styles.paragraph_props(Some("RtlDerived"));
        let overridden = styles.paragraph_props(Some("LtrOverride"));

        assert_eq!(defaults.jc.as_deref(), Some("left"));
        assert_eq!(inherited.bidi, Some(true));
        assert_eq!(inherited.jc.as_deref(), Some("start"));
        assert_eq!(inherited.indent_start_pt, Some(36.0));
        assert_eq!(inherited.indent_end_pt, Some(72.0));
        assert_eq!(overridden.bidi, Some(false));
        assert_eq!(overridden.jc.as_deref(), Some("end"));
    }

    #[test]
    fn resolves_inherited_pagination_controls_and_explicit_off_values() {
        let xml = r#"<w:styles>
            <w:docDefaults><w:pPrDefault><w:pPr><w:widowControl/></w:pPr></w:pPrDefault></w:docDefaults>
            <w:style w:type="paragraph" w:styleId="KeepBase">
                <w:pPr><w:keepNext/><w:keepLines/><w:widowControl w:val="0"/></w:pPr>
            </w:style>
            <w:style w:type="paragraph" w:styleId="KeepDerived"><w:basedOn w:val="KeepBase"/>
                <w:pPr><w:keepLines w:val="off"/><w:widowControl/></w:pPr>
            </w:style>
        </w:styles>"#;
        let styles = parse(xml);
        let defaults = styles.paragraph_props(None);
        let base = styles.paragraph_props(Some("KeepBase"));
        let derived = styles.paragraph_props(Some("KeepDerived"));

        assert_eq!(defaults.widow_control, Some(true));
        assert_eq!(base.keep_next, Some(true));
        assert_eq!(base.keep_lines, Some(true));
        assert_eq!(base.widow_control, Some(false));
        assert_eq!(derived.keep_next, Some(true));
        assert_eq!(derived.keep_lines, Some(false));
        assert_eq!(derived.widow_control, Some(true));
    }

    #[test]
    fn resolves_nonconditional_table_row_pagination_through_table_style_chains() {
        let xml = r#"<w:styles xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <w:style w:type="table" w:styleId="KeepBase">
                <w:trPr><w:cantSplit/></w:trPr>
            </w:style>
            <w:style w:type="table" w:styleId="KeepDerived">
                <w:basedOn w:val="KeepBase"/>
            </w:style>
            <w:style w:type="table" w:styleId="AllowDerived">
                <w:basedOn w:val="KeepBase"/>
                <w:trPr><w:cantSplit w:val="off"/></w:trPr>
            </w:style>
            <w:style w:type="table" w:styleId="ChoiceOff">
                <w:trPr><mc:AlternateContent>
                    <mc:Choice Requires="w14"><w:cantSplit w:val="0"/></mc:Choice>
                    <mc:Fallback><w:cantSplit/></mc:Fallback>
                </mc:AlternateContent></w:trPr>
            </w:style>
            <w:style w:type="table" w:styleId="ConditionalOnly">
                <w:tblStylePr w:type="firstRow"><w:trPr><w:cantSplit/></w:trPr></w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="HistoricalOnly">
                <w:trPr><w:trPrChange><w:trPr><w:cantSplit/></w:trPr></w:trPrChange></w:trPr>
            </w:style>
            <w:style w:type="table" w:styleId="CycleA"><w:basedOn w:val="CycleB"/></w:style>
            <w:style w:type="table" w:styleId="CycleB"><w:basedOn w:val="CycleA"/></w:style>
            <w:style w:type="paragraph" w:styleId="WrongKind">
                <w:trPr><w:cantSplit/></w:trPr>
            </w:style>
            <w:style w:type="table" w:styleId="CrossKind">
                <w:basedOn w:val="WrongKind"/>
            </w:style>
        </w:styles>"#;
        let styles = parse(xml);

        assert_eq!(styles.table_row_cant_split(Some("KeepBase")), Some(true));
        assert_eq!(styles.table_row_cant_split(Some("KeepDerived")), Some(true));
        assert_eq!(
            styles.table_row_cant_split(Some("AllowDerived")),
            Some(false)
        );
        assert_eq!(styles.table_row_cant_split(Some("ChoiceOff")), Some(false));
        assert_eq!(styles.table_row_cant_split(Some("ConditionalOnly")), None);
        assert_eq!(styles.table_row_cant_split(Some("HistoricalOnly")), None);
        assert_eq!(styles.table_row_cant_split(Some("CycleA")), None);
        assert_eq!(styles.table_row_cant_split(Some("CrossKind")), None);
        assert_eq!(styles.table_row_cant_split(Some("missing")), None);
        assert_eq!(styles.table_row_cant_split(None), None);
    }

    #[test]
    fn table_row_style_resolution_stops_at_the_chain_limit() {
        let mut xml = String::from("<w:styles>");
        for index in 0..=STYLE_CHAIN_LIMIT {
            xml.push_str(&format!(
                r#"<w:style w:type="table" w:styleId="S{index}"><w:basedOn w:val="S{}"/></w:style>"#,
                index + 1
            ));
        }
        xml.push_str(&format!(
            r#"<w:style w:type="table" w:styleId="S{}"><w:trPr><w:cantSplit/></w:trPr><w:tblStylePr w:type="firstRow"><w:trPr><w:cantSplit/></w:trPr></w:tblStylePr></w:style></w:styles>"#,
            STYLE_CHAIN_LIMIT + 1
        ));

        let styles = parse(&xml);

        assert_eq!(styles.table_row_cant_split(Some("S0")), None);
        assert_eq!(
            styles.table_row_cant_split(Some(&format!("S{}", STYLE_CHAIN_LIMIT))),
            Some(true)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(
                Some("S0"),
                TableRowStyleRegions {
                    first_row: true,
                    ..Default::default()
                },
            ),
            None
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(
                Some(&format!("S{}", STYLE_CHAIN_LIMIT)),
                TableRowStyleRegions {
                    first_row: true,
                    ..Default::default()
                },
            ),
            Some(true)
        );
    }

    #[test]
    fn resolves_bounded_conditional_table_row_pagination_with_region_precedence() {
        let xml = r#"<w:styles xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <w:style w:type="table" w:styleId="ConditionalBase">
                <w:trPr><w:cantSplit/></w:trPr>
                <w:tblStylePr w:type="wholeTable">
                    <w:trPr><w:cantSplit w:val="off"/></w:trPr>
                </w:tblStylePr>
                <w:tblStylePr w:type="firstRow">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
                <w:tblStylePr w:type="lastRow">
                    <w:trPr><w:cantSplit w:val="off"/></w:trPr>
                </w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="ConditionalDerived">
                <w:basedOn w:val="ConditionalBase"/>
                <w:tblStylePr w:type="lastRow">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="ChoiceOff">
                <w:tblStylePr w:type="firstRow"><mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:trPr><w:cantSplit w:val="0"/></w:trPr>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:trPr><w:cantSplit/></w:trPr>
                    </mc:Fallback>
                </mc:AlternateContent></w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="DeferredBand">
                <w:tblStylePr w:type="band1Horz">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="HistoricalConditional">
                <w:tblStylePr w:type="firstRow"><w:trPr>
                    <w:trPrChange><w:trPr><w:cantSplit/></w:trPr></w:trPrChange>
                </w:trPr></w:tblStylePr>
            </w:style>
        </w:styles>"#;
        let styles = parse(xml);
        let neither = TableRowStyleRegions::default();
        let first = TableRowStyleRegions {
            first_row: true,
            ..Default::default()
        };
        let last = TableRowStyleRegions {
            last_row: true,
            ..Default::default()
        };
        let both = TableRowStyleRegions {
            first_row: true,
            last_row: true,
            ..Default::default()
        };
        let band1 = TableRowStyleRegions {
            band1_horizontal: true,
            ..Default::default()
        };

        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ConditionalBase"), neither),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ConditionalBase"), first),
            Some(true)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ConditionalBase"), last),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ConditionalBase"), both),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ConditionalDerived"), last),
            Some(true)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("ChoiceOff"), first),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("DeferredBand"), band1),
            Some(true)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("HistoricalConditional"), first),
            None
        );
    }

    #[test]
    fn resolves_horizontal_table_bands_and_inherited_band_sizes() {
        let xml = r#"<w:styles xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006">
            <w:style w:type="table" w:styleId="BandBase">
                <w:tblPr><w:tblStyleRowBandSize w:val="2"/></w:tblPr>
                <w:tblStylePr w:type="band1Horz">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
                <w:tblStylePr w:type="band2Horz">
                    <w:trPr><w:cantSplit w:val="off"/></w:trPr>
                </w:tblStylePr>
                <w:tblStylePr w:type="firstRow">
                    <w:trPr><w:cantSplit w:val="off"/></w:trPr>
                </w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="BandDerived">
                <w:basedOn w:val="BandBase"/>
                <w:tblPr><w:tblStyleRowBandSize w:val="3"/></w:tblPr>
                <w:tblStylePr w:type="band2Horz">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
            </w:style>
            <w:style w:type="table" w:styleId="InvalidKeepsBase">
                <w:basedOn w:val="BandBase"/>
                <w:tblPr><w:tblStyleRowBandSize w:val="4"/></w:tblPr>
            </w:style>
            <w:style w:type="table" w:styleId="ZeroDisables">
                <w:basedOn w:val="BandBase"/>
                <w:tblPr><w:tblStyleRowBandSize w:val="0"/></w:tblPr>
            </w:style>
            <w:style w:type="table" w:styleId="LastValidWins">
                <w:basedOn w:val="BandBase"/>
                <w:tblPr>
                    <w:tblStyleRowBandSize w:val="1"/>
                    <w:tblStyleRowBandSize w:val="invalid"/>
                    <w:tblStyleRowBandSize w:val="3"/>
                    <w:tblStyleRowBandSize w:val="9"/>
                </w:tblPr>
            </w:style>
            <w:style w:type="table" w:styleId="SelectedChoice">
                <w:basedOn w:val="BandBase"/>
                <w:tblPr><mc:AlternateContent>
                    <mc:Choice Requires="w14">
                        <w:tblStyleRowBandSize w:val="1"/>
                    </mc:Choice>
                    <mc:Fallback>
                        <w:tblStyleRowBandSize w:val="3"/>
                    </mc:Fallback>
                </mc:AlternateContent></w:tblPr>
            </w:style>
            <w:style w:type="paragraph" w:styleId="WrongKind">
                <w:tblStyleRowBandSize w:val="3"/>
                <w:tblStylePr w:type="band1Horz">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
            </w:style>
        </w:styles>"#;
        let styles = parse(xml);
        let band1 = TableRowStyleRegions {
            band1_horizontal: true,
            ..Default::default()
        };
        let band2 = TableRowStyleRegions {
            band2_horizontal: true,
            ..Default::default()
        };
        let band1_first = TableRowStyleRegions {
            first_row: true,
            band1_horizontal: true,
            ..Default::default()
        };
        let both_bands = TableRowStyleRegions {
            band1_horizontal: true,
            band2_horizontal: true,
            ..Default::default()
        };

        assert_eq!(styles.table_row_band_size(Some("BandBase")), Some(2));
        assert_eq!(styles.table_row_band_size(Some("BandDerived")), Some(3));
        assert_eq!(
            styles.table_row_band_size(Some("InvalidKeepsBase")),
            Some(2)
        );
        assert_eq!(styles.table_row_band_size(Some("ZeroDisables")), Some(0));
        assert_eq!(styles.table_row_band_size(Some("LastValidWins")), Some(3));
        assert_eq!(styles.table_row_band_size(Some("SelectedChoice")), Some(1));
        assert_eq!(styles.table_row_band_size(Some("WrongKind")), None);
        assert_eq!(styles.table_row_band_size(Some("missing")), None);
        assert_eq!(styles.table_row_band_size(None), None);

        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("BandBase"), band1),
            Some(true)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("BandBase"), band2),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("BandBase"), band1_first),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("BandBase"), both_bands),
            Some(false)
        );
        assert_eq!(
            styles.table_row_cant_split_for_regions(Some("BandDerived"), band2),
            Some(true)
        );
    }
}
