//! `.docx` style sheet (`word/styles.xml`) → paragraph-style heading levels and
//! display names, the OOXML analogue of the `.doc` STSH resolver (`stsh.rs`).
//!
//! A heading level is derived from the `w:styleId` (`Heading1`…), the localized
//! `w:name` (`heading 1` / `제목 1`), or the style's own `w:outlineLvl` — reusing
//! [`crate::stsh::heading_from_name`] so both backends recognize the same names.

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::{attr_local, local};
use crate::stsh::heading_from_name;

/// Resolved per-`styleId` heading level and display name.
#[derive(Debug, Default)]
pub(crate) struct Styles {
    heading: HashMap<String, u8>,
    name: HashMap<String, String>,
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
}

/// Parse `word/styles.xml`. Returns an empty sheet on absence/malformation —
/// headings then simply aren't detected (lists/body text are unaffected).
pub(crate) fn parse(xml: &str) -> Styles {
    let mut r = Reader::from_str(xml);
    let mut styles = Styles::default();
    // State for the style currently being parsed.
    let mut cur_id: Option<String> = None;
    let mut cur_name = String::new();
    let mut cur_outline: Option<u8> = None;
    loop {
        match r.read_event() {
            // A new <w:style> opens; capture its id and reset per-style state.
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) if local(e.name().as_ref()) == b"style" => {
                cur_id = attr_local(&e, b"styleId");
                cur_name = String::new();
                cur_outline = None;
            }
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"name" => {
                    if let Some(v) = attr_local(&e, b"val") {
                        cur_name = v;
                    }
                }
                // The style's own paragraph outline level (in its <w:pPr>).
                b"outlineLvl" => {
                    cur_outline = attr_local(&e, b"val").and_then(|v| v.parse::<u8>().ok());
                }
                _ => {}
            },
            Ok(Event::End(e)) if local(e.name().as_ref()) == b"style" => {
                if let Some(id) = cur_id.take() {
                    let level = heading_from_name(&id)
                        .or_else(|| heading_from_name(&cur_name))
                        .or_else(|| cur_outline.filter(|&o| o <= 8).map(|o| o + 1));
                    if let Some(l) = level {
                        styles.heading.insert(id.clone(), l);
                    }
                    if !cur_name.is_empty() {
                        styles.name.insert(id, std::mem::take(&mut cur_name));
                    }
                }
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    styles
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
                <w:pPr><w:outlineLvl w:val="2"/></w:pPr></w:style>
            <w:style w:type="paragraph" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
        </w:styles>"#;
        let s = parse(xml);
        assert_eq!(s.heading_level("Heading1"), Some(1));
        assert_eq!(s.heading_level("KrTitle"), Some(2)); // 제목 2
        assert_eq!(s.heading_level("CustomH"), Some(3)); // outlineLvl 2 → h3
        assert_eq!(s.heading_level("Normal"), None);
        assert_eq!(s.name("KrTitle"), Some("제목 2"));
    }
}
