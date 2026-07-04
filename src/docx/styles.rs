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
use crate::model::{CharProps, Color, VertAlign};
use crate::stsh::heading_from_name;

const STYLE_CHAIN_LIMIT: usize = 32;

/// Resolved per-`styleId` heading level, display name, and run defaults.
#[derive(Debug, Default)]
pub(crate) struct Styles {
    heading: HashMap<String, u8>,
    name: HashMap<String, String>,
    doc_defaults_run: RunProps,
    paragraph_run: HashMap<String, RunProps>,
    character_run: HashMap<String, RunProps>,
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

    pub(crate) fn run_props(
        &self,
        paragraph_style_id: Option<&str>,
        character_style_id: Option<&str>,
    ) -> CharProps {
        let mut props = CharProps::default();
        self.doc_defaults_run.apply_to(&mut props);
        if let Some(style_id) = paragraph_style_id {
            if let Some(style_props) = self.paragraph_run.get(style_id) {
                style_props.apply_to(&mut props);
            }
        }
        if let Some(style_id) = character_style_id {
            if let Some(style_props) = self.character_run.get(style_id) {
                style_props.apply_to(&mut props);
            }
        }
        props
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
    }

    fn overlay(&mut self, other: &RunProps) {
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
    for id in paragraph_ids {
        let props = resolve_style_run_props(
            &id,
            StyleKind::Paragraph,
            &raw_styles,
            &mut paragraph_cache,
            &mut Vec::new(),
            0,
        );
        styles.paragraph_run.insert(id, props);
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StyleKind {
    Paragraph,
    Character,
}

impl StyleKind {
    fn from_attr(value: Option<&str>) -> Option<Self> {
        match value {
            Some("paragraph") => Some(Self::Paragraph),
            Some("character") => Some(Self::Character),
            _ => None,
        }
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
}
