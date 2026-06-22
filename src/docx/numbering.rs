//! `.docx` numbering (`word/numbering.xml`) → per `(numId, ilvl)` ordered/bullet,
//! enough for the exporters to choose `<ol>`/`<ul>` and `1.`/`-`.
//!
//! `<w:num w:numId>` points at a `<w:abstractNum w:abstractNumId>` whose
//! `<w:lvl w:ilvl><w:numFmt w:val>` gives each level's format; `bullet`/`none`
//! ⇒ unordered, everything else ⇒ ordered. (Exact autonumber *labels* aren't
//! reconstructed — the exporters use native list markers, as for `.doc`.)

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;

use super::{attr_local, local};

/// Parsed numbering: `numId → abstractNumId` and per-abstract level formats.
#[derive(Debug, Default)]
pub(crate) struct Numbering {
    num_to_abstract: HashMap<String, String>,
    /// `abstractNumId → (ilvl → ordered?)`.
    abstract_levels: HashMap<String, HashMap<u8, bool>>,
}

impl Numbering {
    /// `Some(true)` = numbered, `Some(false)` = bullet, for a paragraph's
    /// `(numId, ilvl)`. `None` when `numId` isn't a known list. Defaults to
    /// ordered when the specific level's format is unknown but the list exists.
    pub(crate) fn ordered(&self, num_id: &str, ilvl: u8) -> Option<bool> {
        let abs = self.num_to_abstract.get(num_id)?;
        Some(
            self.abstract_levels
                .get(abs)
                .and_then(|m| m.get(&ilvl).copied())
                .unwrap_or(true),
        )
    }
}

/// Parse `word/numbering.xml`. Returns empty on absence/malformation.
pub(crate) fn parse(xml: &str) -> Numbering {
    let mut r = Reader::from_str(xml);
    let mut nb = Numbering::default();
    let mut cur_abstract: Option<String> = None;
    let mut cur_ilvl: Option<u8> = None;
    let mut cur_num: Option<String> = None;
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => match local(e.name().as_ref()) {
                b"abstractNum" => {
                    cur_abstract = attr_local(&e, b"abstractNumId");
                    cur_ilvl = None;
                }
                b"lvl" => {
                    cur_ilvl = attr_local(&e, b"ilvl").and_then(|v| v.parse().ok());
                }
                b"numFmt" => {
                    if let (Some(abs), Some(ilvl), Some(fmt)) =
                        (cur_abstract.as_ref(), cur_ilvl, attr_local(&e, b"val"))
                    {
                        let ordered = fmt != "bullet" && fmt != "none";
                        nb.abstract_levels
                            .entry(abs.clone())
                            .or_default()
                            .insert(ilvl, ordered);
                    }
                }
                b"num" => cur_num = attr_local(&e, b"numId"),
                // `<w:abstractNumId w:val>` as a *child element* of `<w:num>`.
                b"abstractNumId" => {
                    if let (Some(num), Some(val)) = (cur_num.as_ref(), attr_local(&e, b"val")) {
                        nb.num_to_abstract.insert(num.clone(), val);
                    }
                }
                _ => {}
            },
            Ok(Event::End(e)) => match local(e.name().as_ref()) {
                b"abstractNum" => cur_abstract = None,
                b"num" => cur_num = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    nb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_numid_to_ordered_or_bullet() {
        let xml = r#"<w:numbering>
            <w:abstractNum w:abstractNumId="0">
                <w:lvl w:ilvl="0"><w:numFmt w:val="decimal"/></w:lvl>
                <w:lvl w:ilvl="1"><w:numFmt w:val="lowerLetter"/></w:lvl>
            </w:abstractNum>
            <w:abstractNum w:abstractNumId="1">
                <w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/></w:lvl>
            </w:abstractNum>
            <w:num w:numId="5"><w:abstractNumId w:val="0"/></w:num>
            <w:num w:numId="6"><w:abstractNumId w:val="1"/></w:num>
        </w:numbering>"#;
        let nb = parse(xml);
        assert_eq!(nb.ordered("5", 0), Some(true)); // decimal
        assert_eq!(nb.ordered("5", 1), Some(true)); // lowerLetter
        assert_eq!(nb.ordered("6", 0), Some(false)); // bullet
        assert_eq!(nb.ordered("99", 0), None); // unknown numId
    }
}
