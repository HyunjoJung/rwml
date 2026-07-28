//! Style sheet (STSH) parsing for paragraph style identity and the bounded
//! pagination subset consumed by the legacy renderer.
//!
//! The STSH (FIB `fcStshf`, pair index 1, in the table stream) is an `STSHI`
//! header followed by one `LPStd` per style. Each `STD` carries a built-in style
//! id (`sti`), a style kind (`sgc`), the base style it inherits from
//! (`istdBase`), a display name, and length-prefixed UPX property differences.
//! A paragraph's `istd` (from its PAPX) indexes this array; the heading level is
//! derived from `sti` (1–9 = Heading 1–9), the base-style chain, or the localized
//! name (`Heading N` / `제목 N`). Paragraph styles also resolve the four
//! pagination SPRMs modeled by the renderer through the bounded base chain.
//!
//! Reference: [MS-DOC] 2.9.271 (STSH), 2.9.272 (STSHI), 2.9.135 (LPStd),
//! 2.9.270 (STD), 2.9.270.1 (StdfBase), 2.9.276 (sti).

use crate::papx::{
    scan_paragraph_pagination_overrides, ParagraphPagination, ParagraphPaginationOverrides,
};
use crate::util::u16le;

/// One style's identity (enough to resolve a heading level + name).
#[derive(Debug, Clone, Default)]
struct StyleDescription {
    sti: u16,
    sgc: u8,
    istd_base: u16,
    name: String,
}

struct ParsedStyle {
    description: StyleDescription,
    pagination: Option<ParagraphPaginationOverrides>,
}

/// The parsed stylesheet: per-`istd` heading, name, and resolved pagination.
#[derive(Debug, Default)]
pub(crate) struct StyleSheet {
    heading: Vec<Option<u8>>,
    names: Vec<String>,
    pagination: Vec<ParagraphPagination>,
}

impl StyleSheet {
    /// The heading level (1–9) for a paragraph style index, or `None` for body.
    pub(crate) fn heading_level(&self, istd: u16) -> Option<u8> {
        self.heading.get(istd as usize).copied().flatten()
    }

    /// The display name for a style index (e.g. `Heading 1`, `제목 1`), if known.
    pub(crate) fn name(&self, istd: u16) -> Option<&str> {
        self.names
            .get(istd as usize)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Pagination properties resolved through the paragraph style's base chain.
    pub(crate) fn paragraph_pagination(&self, istd: u16) -> ParagraphPagination {
        self.pagination
            .get(istd as usize)
            .copied()
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn from_test_pagination(pagination: Vec<ParagraphPagination>) -> Self {
        let len = pagination.len();
        Self {
            heading: vec![None; len],
            names: vec![String::new(); len],
            pagination,
        }
    }

    /// Parse the STSH from the table stream. Returns an empty stylesheet (not an
    /// error) on absence or header malformation; malformed style slots degrade
    /// independently.
    pub(crate) fn parse(table: &[u8], fc_stshf: usize, lcb_stshf: usize) -> StyleSheet {
        let empty = StyleSheet::default();
        if lcb_stshf < 8 {
            return empty;
        }
        let Some(stsh) = table.get(fc_stshf..fc_stshf.saturating_add(lcb_stshf)) else {
            return empty;
        };
        let (Some(cb_stshi), Some(cstd), Some(cb_std_base)) =
            (u16le(stsh, 0), u16le(stsh, 2), u16le(stsh, 4))
        else {
            return empty;
        };
        let base_len = match cb_std_base {
            10 | 18 => cb_std_base as usize,
            _ => return empty,
        };

        let mut p = 2usize.saturating_add(cb_stshi as usize);
        let mut descs: Vec<Option<StyleDescription>> = Vec::with_capacity(cstd as usize);
        let mut local_pagination = Vec::with_capacity(cstd as usize);
        for _ in 0..cstd {
            let Some(cb_std) = u16le(stsh, p) else { break };
            p += 2;
            if cb_std == 0 {
                descs.push(None); // empty slot still consumes an istd index
                local_pagination.push(None);
                continue;
            }
            let cb_std = cb_std as usize;
            let Some(std) = stsh.get(p..p.saturating_add(cb_std)) else {
                break;
            };
            p += cb_std;
            let istd = descs.len() as u16;
            let parsed = parse_std(std, base_len, istd);
            local_pagination.push(parsed.as_ref().and_then(|style| style.pagination));
            descs.push(parsed.map(|style| style.description));
        }

        let n = descs.len();
        let mut heading = vec![None; n];
        let mut names = vec![String::new(); n];
        let mut pagination = vec![ParagraphPagination::default(); n];
        // Per-style cycle guard by epoch: `visited[i]` is the pass (`gen`) that last touched
        // style `i`. "Clearing" between styles is just a fresh `gen` (O(1)) — refilling the
        // whole buffer each pass was O(n) per style = O(n^2) writes (≈4.3e9 for cstd=65535),
        // a CPU DoS on a crafted `.doc`. `gen` starts at 1 so 0 reads as never-visited; `n`
        // is bounded by `cstd` (u16), so `gen = istd + 1` never overflows `u32`.
        let mut visited = vec![0u32; n];
        let mut pagination_visited = vec![0u32; n];
        for istd in 0..n {
            let gen = istd as u32 + 1;
            heading[istd] = resolve_level(&descs, istd, &mut visited, gen, 0);
            pagination[istd] = resolve_pagination(
                &descs,
                &local_pagination,
                istd,
                &mut pagination_visited,
                gen,
                0,
            )
            .unwrap_or_default();
            if let Some(d) = &descs[istd] {
                names[istd] = d.name.clone();
            }
        }
        StyleSheet {
            heading,
            names,
            pagination,
        }
    }
}

fn parse_std(std: &[u8], base_len: usize, istd: u16) -> Option<ParsedStyle> {
    let sti = u16le(std, 0)? & 0x0FFF;
    let grf = u16le(std, 2)?;
    let sgc = (grf & 0x000F) as u8;
    let istd_base = (grf >> 4) & 0x0FFF;
    let cupx = (u16le(std, 4)? & 0x000F) as u8;
    let has_original_style = base_len == 18 && u16le(std, 10)? & 0x1000 != 0;
    let (name, grlp_offset) = parse_xstz(std, base_len)?;
    let pagination = if sgc == 1 {
        parse_paragraph_style_pagination(std.get(grlp_offset..)?, cupx, has_original_style, istd)
    } else {
        None
    };
    Some(ParsedStyle {
        description: StyleDescription {
            sti,
            sgc,
            istd_base,
            name,
        },
        pagination,
    })
}

fn parse_xstz(std: &[u8], offset: usize) -> Option<(String, usize)> {
    let cch = u16le(std, offset)? as usize;
    let chars_start = offset.checked_add(2)?;
    let chars_len = cch.checked_mul(2)?;
    let chars_end = chars_start.checked_add(chars_len)?;
    let terminator_end = chars_end.checked_add(2)?;
    let chars = std.get(chars_start..chars_end)?;
    if u16le(std, chars_end)? != 0 {
        return None;
    }
    Some((utf16le(chars), terminator_end))
}

fn parse_paragraph_style_pagination(
    grlp: &[u8],
    cupx: u8,
    has_original_style: bool,
    istd: u16,
) -> Option<ParagraphPaginationOverrides> {
    if !matches!((cupx, has_original_style), (2, false) | (3, true)) {
        return None;
    }
    let (papx, mut offset) = read_lp_upx(grlp, 0)?;
    if papx.len() < 2 || u16le(papx, 0)? != istd {
        return None;
    }
    let pagination = scan_paragraph_pagination_overrides(&papx[2..])?;

    let (_, next) = read_lp_upx(grlp, offset)?;
    offset = next;
    if cupx == 3 {
        let cb = u16le(grlp, offset)? as usize;
        let start = offset.checked_add(2)?;
        let end = start.checked_add(cb)?;
        validate_revision_paragraph_upx(grlp.get(start..end)?)?;
        offset = end;
    }
    (offset == grlp.len()).then_some(pagination)
}

fn validate_revision_paragraph_upx(data: &[u8]) -> Option<()> {
    let (_, after_papx) = read_lp_upx(data, 8)?;
    let (_, end) = read_lp_upx(data, after_papx)?;
    (end == data.len()).then_some(())
}

fn read_lp_upx(data: &[u8], offset: usize) -> Option<(&[u8], usize)> {
    let cb = u16le(data, offset)? as usize;
    let start = offset.checked_add(2)?;
    let end = start.checked_add(cb)?;
    let payload = data.get(start..end)?;
    let next = end.checked_add(cb & 1)?;
    if cb & 1 != 0 && data.get(end).copied()? != 0 {
        return None;
    }
    data.get(end..next)?;
    Some((payload, next))
}

/// Maximum base-style chain depth followed before giving up. Far deeper than any real style
/// hierarchy, but it bounds the recursion so a crafted `.doc` with a long `basedOn` chain
/// can't overflow the stack (the `visited` guard already breaks cycles; this caps a long
/// acyclic chain, whose length is otherwise only bounded by the attacker-controlled count).
const MAX_STYLE_BASE_DEPTH: usize = 64;

/// Resolve a style's heading level: by built-in `sti`, then up the base chain,
/// then by name. `visited` guards against base-style cycles; `depth` bounds an acyclic chain.
fn resolve_level(
    descs: &[Option<StyleDescription>],
    istd: usize,
    visited: &mut [u32],
    gen: u32,
    depth: usize,
) -> Option<u8> {
    if depth > MAX_STYLE_BASE_DEPTH
        || istd >= descs.len()
        || visited.get(istd).copied().unwrap_or(gen) == gen
    {
        return None;
    }
    visited[istd] = gen;
    let Some(d) = &descs[istd] else { return None };
    if (1..=9).contains(&d.sti) {
        return Some(d.sti as u8);
    }
    if d.sti == 0 {
        return None; // Normal
    }
    // Custom style based on a heading inherits its level.
    if d.sgc == 1 && d.istd_base != 0x0FFF && d.istd_base as usize != istd {
        if let Some(n) = resolve_level(descs, d.istd_base as usize, visited, gen, depth + 1) {
            return Some(n);
        }
    }
    heading_from_name(&d.name)
}

fn resolve_pagination(
    descs: &[Option<StyleDescription>],
    local: &[Option<ParagraphPaginationOverrides>],
    istd: usize,
    visited: &mut [u32],
    gen: u32,
    depth: usize,
) -> Option<ParagraphPagination> {
    if depth > MAX_STYLE_BASE_DEPTH
        || istd >= descs.len()
        || visited.get(istd).copied().unwrap_or(gen) == gen
    {
        return None;
    }
    visited[istd] = gen;
    let description = descs.get(istd)?.as_ref()?;
    if description.sgc != 1 {
        return None;
    }
    let overrides = local.get(istd).copied().flatten()?;
    let inherited = if description.istd_base == 0x0FFF {
        ParagraphPagination::default()
    } else {
        resolve_pagination(
            descs,
            local,
            description.istd_base as usize,
            visited,
            gen,
            depth + 1,
        )?
    };
    Some(inherited.apply(overrides))
}

/// `Heading N` (any case) or Korean `제목 N` → the digit `N` (1–9). Shared with
/// the `.docx` style resolver (`w:styleId` / `w:name` use the same conventions).
pub(crate) fn heading_from_name(name: &str) -> Option<u8> {
    let t = name.trim();
    if let Some(rest) = t.strip_prefix("제목") {
        if let Ok(n) = rest.trim().parse::<u8>() {
            if (1..=9).contains(&n) {
                return Some(n);
            }
        }
    }
    let lower = t.to_lowercase();
    if let Some(rest) = lower.strip_prefix("heading") {
        if let Ok(n) = rest.trim().parse::<u8>() {
            if (1..=9).contains(&n) {
                return Some(n);
            }
        }
    }
    None
}

fn utf16le(b: &[u8]) -> String {
    let units: Vec<u16> = b
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paragraph_style_std(
        base_len: usize,
        istd: u16,
        base: u16,
        name: &str,
        properties: &[(u16, u8)],
        revision_marked: bool,
    ) -> Vec<u8> {
        let mut std = vec![0u8; base_len];
        let sti: u16 = if istd == 0 { 0 } else { 0x0FFE };
        std[0..2].copy_from_slice(&sti.to_le_bytes());
        std[2..4].copy_from_slice(&(1 | ((base & 0x0FFF) << 4)).to_le_bytes());
        let cupx: u16 = if revision_marked { 3 } else { 2 };
        std[4..6].copy_from_slice(&cupx.to_le_bytes());
        if revision_marked && base_len == 18 {
            std[10..12].copy_from_slice(&0x1000u16.to_le_bytes());
        }

        let name_units: Vec<u16> = name.encode_utf16().collect();
        std.extend_from_slice(&(name_units.len() as u16).to_le_bytes());
        for unit in name_units {
            std.extend_from_slice(&unit.to_le_bytes());
        }
        std.extend_from_slice(&0u16.to_le_bytes());

        let mut papx = Vec::with_capacity(2 + properties.len() * 3);
        papx.extend_from_slice(&istd.to_le_bytes());
        for &(sprm, value) in properties {
            papx.extend_from_slice(&sprm.to_le_bytes());
            papx.push(value);
        }
        std.extend_from_slice(&(papx.len() as u16).to_le_bytes());
        std.extend_from_slice(&papx);
        if papx.len() % 2 == 1 {
            std.push(0);
        }
        std.extend_from_slice(&0u16.to_le_bytes());

        if revision_marked {
            let mut revision = vec![0u8; 8];
            revision.extend_from_slice(&0u16.to_le_bytes());
            revision.extend_from_slice(&0u16.to_le_bytes());
            std.extend_from_slice(&(revision.len() as u16).to_le_bytes());
            std.extend_from_slice(&revision);
        }
        let cb_std = std.len() as u16;
        std[6..8].copy_from_slice(&cb_std.to_le_bytes());
        std
    }

    fn stylesheet(base_len: usize, styles: &[Option<Vec<u8>>]) -> Vec<u8> {
        let mut stsh = vec![0u8; 20];
        stsh[0..2].copy_from_slice(&18u16.to_le_bytes());
        stsh[2..4].copy_from_slice(&(styles.len() as u16).to_le_bytes());
        stsh[4..6].copy_from_slice(&(base_len as u16).to_le_bytes());
        stsh[6..8].copy_from_slice(&1u16.to_le_bytes());
        for style in styles {
            match style {
                Some(std) => {
                    stsh.extend_from_slice(&(std.len() as u16).to_le_bytes());
                    stsh.extend_from_slice(std);
                }
                None => stsh.extend_from_slice(&0u16.to_le_bytes()),
            }
        }
        stsh
    }

    fn parsed_pagination(
        base_len: usize,
        styles: &[Option<Vec<u8>>],
        istd: u16,
    ) -> ParagraphPagination {
        let stsh = stylesheet(base_len, styles);
        StyleSheet::parse(&stsh, 0, stsh.len()).paragraph_pagination(istd)
    }

    #[test]
    fn resolves_paragraph_pagination_inheritance_for_both_std_sizes() {
        for base_len in [10, 18] {
            let styles = vec![
                Some(paragraph_style_std(
                    base_len,
                    0,
                    0x0FFF,
                    "Normal",
                    &[(0x2405, 1), (0x2431, 0)],
                    false,
                )),
                Some(paragraph_style_std(
                    base_len,
                    1,
                    0,
                    "Parent",
                    &[(0x2406, 1), (0x2407, 1)],
                    false,
                )),
                Some(paragraph_style_std(
                    base_len,
                    2,
                    1,
                    "Child",
                    &[(0x2405, 0), (0x2407, 0)],
                    false,
                )),
            ];
            assert_eq!(
                parsed_pagination(base_len, &styles, 2),
                ParagraphPagination {
                    keep_next: true,
                    keep_lines: false,
                    page_break_before: false,
                    widow_control: false,
                }
            );
        }
    }

    #[test]
    fn paragraph_style_pagination_is_last_sprm_wins() {
        let styles = vec![Some(paragraph_style_std(
            10,
            0,
            0x0FFF,
            "Normal",
            &[
                (0x2405, 1),
                (0x2405, 0),
                (0x2407, 0),
                (0x2407, 1),
                (0x2431, 0),
                (0x2431, 1),
            ],
            false,
        ))];
        assert_eq!(
            parsed_pagination(10, &styles, 0),
            ParagraphPagination {
                keep_next: false,
                keep_lines: false,
                page_break_before: true,
                widow_control: true,
            }
        );
    }

    #[test]
    fn revision_marked_style_uses_current_paragraph_upx() {
        let styles = vec![Some(paragraph_style_std(
            18,
            0,
            0x0FFF,
            "Normal",
            &[(0x2406, 1), (0x2431, 0)],
            true,
        ))];
        assert_eq!(
            parsed_pagination(18, &styles, 0),
            ParagraphPagination {
                keep_next: true,
                keep_lines: false,
                page_break_before: false,
                widow_control: false,
            }
        );
    }

    #[test]
    fn malformed_paragraph_style_upx_falls_back_without_partial_values() {
        let valid = paragraph_style_std(10, 0, 0x0FFF, "Normal", &[(0x2405, 1)], false);
        let grlp = 10 + 2 + "Normal".encode_utf16().count() * 2 + 2;

        let mut cases = Vec::new();

        let mut wrong_kind = valid.clone();
        wrong_kind[2..4].copy_from_slice(&(2u16 | (0x0FFFu16 << 4)).to_le_bytes());
        cases.push(wrong_kind);

        let mut wrong_cupx = valid.clone();
        wrong_cupx[4..6].copy_from_slice(&1u16.to_le_bytes());
        cases.push(wrong_cupx);

        let mut wrong_istd = valid.clone();
        wrong_istd[grlp + 2..grlp + 4].copy_from_slice(&1u16.to_le_bytes());
        cases.push(wrong_istd);

        let mut nonzero_padding = valid.clone();
        let cb_upx = u16le(&nonzero_padding, grlp).unwrap() as usize;
        nonzero_padding[grlp + 2 + cb_upx] = 1;
        cases.push(nonzero_padding);

        let mut oversized_upx = valid.clone();
        oversized_upx[grlp..grlp + 2].copy_from_slice(&u16::MAX.to_le_bytes());
        cases.push(oversized_upx);

        let mut truncated_sprm = valid.clone();
        truncated_sprm.remove(grlp + 2 + 4);
        cases.push(truncated_sprm);

        let mut missing_name_terminator = valid;
        missing_name_terminator[grlp - 2..grlp].copy_from_slice(&1u16.to_le_bytes());
        cases.push(missing_name_terminator);

        for std in cases {
            assert_eq!(
                parsed_pagination(10, &[Some(std)], 0),
                ParagraphPagination::default()
            );
        }
    }

    #[test]
    fn invalid_base_chains_do_not_apply_partial_style_values() {
        let page_break = &[(0x2407, 1)];
        let out_of_range = vec![Some(paragraph_style_std(
            10, 0, 7, "BadBase", page_break, false,
        ))];
        assert_eq!(
            parsed_pagination(10, &out_of_range, 0),
            ParagraphPagination::default()
        );

        let empty_base = vec![
            None,
            Some(paragraph_style_std(
                10,
                1,
                0,
                "EmptyBase",
                page_break,
                false,
            )),
        ];
        assert_eq!(
            parsed_pagination(10, &empty_base, 1),
            ParagraphPagination::default()
        );

        let cycle = vec![
            Some(paragraph_style_std(10, 0, 1, "CycleA", page_break, false)),
            Some(paragraph_style_std(10, 1, 0, "CycleB", &[], false)),
        ];
        assert_eq!(
            parsed_pagination(10, &cycle, 0),
            ParagraphPagination::default()
        );
    }

    #[test]
    fn paragraph_pagination_base_chain_has_a_depth_bound() {
        let mut styles = Vec::new();
        styles.push(Some(paragraph_style_std(
            10,
            0,
            0x0FFF,
            "Root",
            &[(0x2407, 1)],
            false,
        )));
        for istd in 1..=MAX_STYLE_BASE_DEPTH + 2 {
            styles.push(Some(paragraph_style_std(
                10,
                istd as u16,
                (istd - 1) as u16,
                "Derived",
                &[],
                false,
            )));
        }
        assert_eq!(
            parsed_pagination(10, &styles, (styles.len() - 1) as u16),
            ParagraphPagination::default()
        );
    }

    #[test]
    fn invalid_std_base_size_and_revision_shape_fall_back() {
        let styles = vec![Some(paragraph_style_std(
            10,
            0,
            0x0FFF,
            "Normal",
            &[(0x2407, 1)],
            false,
        ))];
        let malformed_header = stylesheet(12, &styles);
        let parsed = StyleSheet::parse(&malformed_header, 0, malformed_header.len());
        assert_eq!(
            parsed.paragraph_pagination(0),
            ParagraphPagination::default()
        );

        let invalid_revision = paragraph_style_std(10, 0, 0x0FFF, "Normal", &[(0x2407, 1)], true);
        assert_eq!(
            parsed_pagination(10, &[Some(invalid_revision)], 0),
            ParagraphPagination::default()
        );
    }

    #[test]
    fn heading_name_matching() {
        assert_eq!(heading_from_name("Heading 1"), Some(1));
        assert_eq!(heading_from_name("heading3"), Some(3));
        assert_eq!(heading_from_name("제목 2"), Some(2));
        assert_eq!(heading_from_name("제목3"), Some(3));
        assert_eq!(heading_from_name("Normal"), None);
        assert_eq!(heading_from_name("본문"), None);
    }

    #[test]
    fn resolve_by_sti_and_base_chain() {
        let descs = vec![
            Some(StyleDescription {
                sti: 0,
                sgc: 1,
                istd_base: 0x0FFF,
                name: "Normal".into(),
            }),
            Some(StyleDescription {
                sti: 1,
                sgc: 1,
                istd_base: 0,
                name: "Heading 1".into(),
            }),
            // custom style (high sti) based on Heading 1 (istd 1) → level 1.
            Some(StyleDescription {
                sti: 0x0FFE,
                sgc: 1,
                istd_base: 1,
                name: "MyHead".into(),
            }),
            // custom style by name only.
            Some(StyleDescription {
                sti: 0x0FFE,
                sgc: 1,
                istd_base: 0x0FFF,
                name: "제목 4".into(),
            }),
        ];
        let n = descs.len();
        let lvl = |i| resolve_level(&descs, i, &mut vec![0u32; n], 1, 0);
        assert_eq!(lvl(0), None);
        assert_eq!(lvl(1), Some(1));
        assert_eq!(lvl(2), Some(1));
        assert_eq!(lvl(3), Some(4));
    }

    #[test]
    fn cycle_guard() {
        // A↔B base cycle must terminate, not stack-overflow.
        let descs = vec![
            Some(StyleDescription {
                sti: 0x0FFE,
                sgc: 1,
                istd_base: 1,
                name: String::new(),
            }),
            Some(StyleDescription {
                sti: 0x0FFE,
                sgc: 1,
                istd_base: 0,
                name: String::new(),
            }),
        ];
        assert_eq!(resolve_level(&descs, 0, &mut [0u32; 2], 1, 0), None);
    }

    #[test]
    fn long_acyclic_base_chain_does_not_stack_overflow() {
        // A crafted .doc can declare a very long acyclic basedOn chain (style i based on
        // i+1). The `visited` guard breaks cycles but not depth, so without the depth cap the
        // recursion would blow the stack. With it, this terminates and yields the bottom
        // style's heading name without panicking.
        let n = 200_000usize;
        let mut descs: Vec<Option<StyleDescription>> = (0..n)
            .map(|i| {
                Some(StyleDescription {
                    sti: 0x0FFE,
                    sgc: 1,
                    istd_base: (i + 1) as u16, // chain upward; last points past the end
                    name: String::new(),
                })
            })
            .collect();
        // Make the deepest style a named heading so a (hypothetical) full walk would resolve.
        descs[n - 1] = Some(StyleDescription {
            sti: 0x0FFE,
            sgc: 1,
            istd_base: 0x0FFF,
            name: "Heading 3".into(),
        });
        // Must not panic / overflow the stack; the depth cap stops the walk early → None.
        let mut visited = vec![0u32; n];
        assert_eq!(resolve_level(&descs, 0, &mut visited, 1, 0), None);
    }

    #[test]
    fn huge_style_count_resolves_in_linear_time() {
        // Crafted STSH: cbStshi=6, cstd=65535, cbStdBase=10, namesWritten=0, then 65535 empty
        // (cbStd=0) style slots — ~128 KiB. Resolving every style's heading must be O(n); the
        // old per-style `visited.fill(false)` was O(n²) (≈4.3e9 writes) = a CPU DoS at open.
        let cstd = 65535u16;
        let mut stsh = Vec::new();
        stsh.extend_from_slice(&6u16.to_le_bytes()); // cbStshi → records start at byte 8
        stsh.extend_from_slice(&cstd.to_le_bytes()); // cstd
        stsh.extend_from_slice(&10u16.to_le_bytes()); // cbStdBase
        stsh.extend_from_slice(&0u16.to_le_bytes()); // flags (namesWritten = 0)
        stsh.resize(stsh.len() + cstd as usize * 2, 0u8); // 65535 × cbStd=0
        let ss = StyleSheet::parse(&stsh, 0, stsh.len());
        // All slots empty ⇒ no headings; the point is it terminates fast (no O(n²) work).
        assert_eq!(ss.heading.len(), cstd as usize);
        assert!(ss.heading.iter().all(|h| h.is_none()));
    }
}
