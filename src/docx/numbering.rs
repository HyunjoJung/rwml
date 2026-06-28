//! `.docx` numbering (`word/numbering.xml`) → per `(numId, ilvl)` ordered/bullet
//! and the autonumber **label** (`1.`, `a)`, `1.1`, `i.` …), computed from each
//! level's `<w:numFmt>`, `<w:lvlText>` pattern, and `<w:start>` with live
//! per-level counters maintained in document order.
//!
//! `<w:num w:numId>` points at a `<w:abstractNum w:abstractNumId>` whose
//! `<w:lvl w:ilvl>` carries `numFmt` (decimal/lowerLetter/lowerRoman/…; bullet/
//! none ⇒ unordered), `lvlText` (e.g. `%1.`), and `start`.

use std::collections::HashMap;

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

use super::{attr_local, attr_u32, attr_u8, local};

/// One numbering level's resolved formatting.
#[derive(Debug, Clone)]
struct Level {
    ordered: bool,
    num_fmt: String,
    lvl_text: String,
    start: u32,
}

impl Default for Level {
    fn default() -> Self {
        Level {
            ordered: true,
            num_fmt: "decimal".into(),
            lvl_text: String::new(),
            start: 1,
        }
    }
}

/// Parsed numbering: `numId → abstractNumId` and per-abstract level formats.
#[derive(Debug, Default)]
pub(crate) struct Numbering {
    num_to_abstract: HashMap<String, String>,
    /// `abstractNumId → (ilvl → Level)`.
    abstract_levels: HashMap<String, HashMap<u8, Level>>,
}

impl Numbering {
    fn levels(&self, num_id: &str) -> Option<&HashMap<u8, Level>> {
        self.abstract_levels.get(self.num_to_abstract.get(num_id)?)
    }

    /// `Some(true)` = numbered, `Some(false)` = bullet, for `(numId, ilvl)`.
    /// `None` when `numId` isn't a known list; defaults to ordered when the list
    /// exists but the specific level's format is unknown.
    pub(crate) fn ordered(&self, num_id: &str, ilvl: u8) -> Option<bool> {
        let levels = self.levels(num_id)?;
        Some(levels.get(&ilvl).map(|l| l.ordered).unwrap_or(true))
    }

    /// Advance `counters` for this list item and format its autonumber label
    /// (e.g. `1.`, `a)`, `1.1`). Returns `None` for a bullet level (the caller
    /// supplies the bullet glyph) or an unknown list.
    pub(crate) fn label(&self, num_id: &str, ilvl: u8, counters: &mut [u32; 9]) -> Option<String> {
        let levels = self.levels(num_id)?;
        let i = ilvl.min(8) as usize;
        let lvl = levels.get(&ilvl).cloned().unwrap_or_default();
        // Advance this level (seed at `start` on first use), reset deeper levels.
        if counters[i] == 0 {
            counters[i] = lvl.start.max(1);
        } else {
            counters[i] += 1;
        }
        for c in counters.iter_mut().skip(i + 1) {
            *c = 0;
        }
        if !lvl.ordered {
            return None;
        }
        let pattern = if lvl.lvl_text.is_empty() {
            format!("%{}.", i + 1)
        } else {
            lvl.lvl_text.clone()
        };
        Some(expand(&pattern, levels, counters))
    }

    /// Format the current list item using all available ancestor counters,
    /// matching REF `\w` full-context numbering such as `1.a.i`.
    pub(crate) fn full_context_label(
        &self,
        num_id: &str,
        ilvl: u8,
        counters: &[u32; 9],
    ) -> Option<String> {
        let levels = self.levels(num_id)?;
        let max = ilvl.min(8);
        let mut parts = Vec::new();
        for level in 0..=max {
            let count = counters[level as usize];
            if count == 0 {
                return None;
            }
            let lvl = levels.get(&level).cloned().unwrap_or_default();
            if !lvl.ordered {
                return None;
            }
            parts.push(format_num(count, &lvl.num_fmt));
        }
        (!parts.is_empty()).then(|| parts.join("."))
    }
}

/// Expand an `lvlText` pattern, replacing `%1`..`%9` with the corresponding
/// level's counter formatted in that level's `numFmt`.
fn expand(pattern: &str, levels: &HashMap<u8, Level>, counters: &[u32; 9]) -> String {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
            if let Some(d) = chars.peek().and_then(|d| d.to_digit(10)) {
                chars.next();
                if (1..=9).contains(&d) {
                    let k = (d - 1) as usize;
                    let fmt = levels
                        .get(&(k as u8))
                        .map(|l| l.num_fmt.as_str())
                        .unwrap_or("decimal");
                    out.push_str(&format_num(counters[k].max(1), fmt));
                    continue;
                }
                out.push('%');
                out.push(char::from_digit(d, 10).unwrap_or('?'));
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// Format a 1-based counter in a `w:numFmt` style. Unknown/CJK formats fall back
/// to decimal (the legacy `.doc` path handles Korean autonumber styles).
fn format_num(n: u32, fmt: &str) -> String {
    match fmt {
        "decimalZero" => format!("{n:02}"),
        "lowerLetter" => alpha(n, 'a'),
        "upperLetter" => alpha(n, 'A'),
        "lowerRoman" => roman(n).to_lowercase(),
        "upperRoman" => roman(n),
        _ => n.to_string(),
    }
}

/// Spreadsheet-style letters: 1→a, 26→z, 27→aa (base = `'a'` or `'A'`).
fn alpha(mut n: u32, base: char) -> String {
    if n == 0 {
        return base.to_string();
    }
    let mut s = Vec::new();
    while n > 0 {
        let rem = (n - 1) % 26;
        s.push((base as u8 + rem as u8) as char);
        n = (n - 1) / 26;
    }
    s.iter().rev().collect()
}

/// Roman numerals (uppercase), clamped to a sane range.
fn roman(mut n: u32) -> String {
    if n == 0 || n > 3999 {
        return n.to_string();
    }
    const VALS: &[(u32, &str)] = &[
        (1000, "M"),
        (900, "CM"),
        (500, "D"),
        (400, "CD"),
        (100, "C"),
        (90, "XC"),
        (50, "L"),
        (40, "XL"),
        (10, "X"),
        (9, "IX"),
        (5, "V"),
        (4, "IV"),
        (1, "I"),
    ];
    let mut s = String::new();
    for &(v, sym) in VALS {
        while n >= v {
            s.push_str(sym);
            n -= v;
        }
    }
    s
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
                    cur_ilvl = attr_u8(&e, b"ilvl");
                    set_level(&mut nb, &cur_abstract, cur_ilvl, &e, |_, _| {});
                }
                b"numFmt" => set_level(&mut nb, &cur_abstract, cur_ilvl, &e, |l, e| {
                    if let Some(v) = attr_local(e, b"val") {
                        let value = v.trim();
                        l.ordered = value != "bullet" && value != "none";
                        l.num_fmt = value.to_string();
                    }
                }),
                b"lvlText" => set_level(&mut nb, &cur_abstract, cur_ilvl, &e, |l, e| {
                    if let Some(v) = attr_local(e, b"val") {
                        l.lvl_text = v;
                    }
                }),
                b"start" => set_level(&mut nb, &cur_abstract, cur_ilvl, &e, |l, e| {
                    if let Some(v) = attr_u32(e, b"val") {
                        l.start = v;
                    }
                }),
                b"num" => cur_num = attr_local(&e, b"numId"),
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
                b"lvl" => cur_ilvl = None,
                _ => {}
            },
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    nb
}

/// Apply a mutation to the current `(abstract, ilvl)` level, creating it if new.
fn set_level(
    nb: &mut Numbering,
    abs: &Option<String>,
    ilvl: Option<u8>,
    e: &BytesStart<'_>,
    f: impl FnOnce(&mut Level, &BytesStart<'_>),
) {
    if let (Some(abs), Some(ilvl)) = (abs.as_ref(), ilvl) {
        let lvl = nb
            .abstract_levels
            .entry(abs.clone())
            .or_default()
            .entry(ilvl)
            .or_default();
        f(lvl, e);
    }
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
                <w:lvl w:ilvl="0"><w:numFmt w:val=" bullet "/></w:lvl>
            </w:abstractNum>
            <w:abstractNum w:abstractNumId="2">
                <w:lvl w:ilvl="0"><w:numFmt w:val=" none "/></w:lvl>
            </w:abstractNum>
            <w:num w:numId="5"><w:abstractNumId w:val="0"/></w:num>
            <w:num w:numId="6"><w:abstractNumId w:val="1"/></w:num>
            <w:num w:numId="7"><w:abstractNumId w:val="2"/></w:num>
        </w:numbering>"#;
        let nb = parse(xml);
        assert_eq!(nb.ordered("5", 0), Some(true));
        assert_eq!(nb.ordered("5", 1), Some(true));
        assert_eq!(nb.ordered("6", 0), Some(false));
        assert_eq!(nb.ordered("7", 0), Some(false));
        assert_eq!(nb.ordered("99", 0), None);
    }

    #[test]
    fn formats_multi_level_labels() {
        let xml = r#"<w:numbering>
            <w:abstractNum w:abstractNumId="0">
                <w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl>
                <w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2)"/></w:lvl>
                <w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val=" lowerRoman "/><w:lvlText w:val="%1.%2.%3"/></w:lvl>
            </w:abstractNum>
            <w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num>
            <w:abstractNum w:abstractNumId="9"><w:lvl w:ilvl="0"><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/></w:lvl></w:abstractNum>
            <w:num w:numId="2"><w:abstractNumId w:val="9"/></w:num>
        </w:numbering>"#;
        let nb = parse(xml);
        let mut c = [0u32; 9];
        assert_eq!(nb.label("1", 0, &mut c).as_deref(), Some("1."));
        assert_eq!(nb.label("1", 0, &mut c).as_deref(), Some("2."));
        assert_eq!(nb.label("1", 1, &mut c).as_deref(), Some("a)"));
        assert_eq!(nb.label("1", 1, &mut c).as_deref(), Some("b)"));
        assert_eq!(nb.label("1", 2, &mut c).as_deref(), Some("2.b.i"));
        assert_eq!(nb.label("1", 0, &mut c).as_deref(), Some("3."));
        assert_eq!(nb.label("2", 0, &mut c), None);
    }
}
