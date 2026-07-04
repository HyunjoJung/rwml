//! Style sheet (STSH) parsing → paragraph style names and heading levels.
//!
//! The STSH (FIB `fcStshf`, pair index 1, in the table stream) is an `STSHI`
//! header followed by one `LPStd` per style. Each `STD` carries a built-in style
//! id (`sti`), a style kind (`sgc`), the base style it inherits from
//! (`istdBase`), and a display name. A paragraph's `istd` (from its PAPX) indexes
//! this array; the heading level is derived from `sti` (1–9 = Heading 1–9), the
//! base-style chain, or the localized name (`Heading N` / `제목 N`).
//!
//! Reference: [MS-DOC] 2.9.271 (STSH), 2.9.272 (STSHI), 2.9.135 (LPStd),
//! 2.9.270 (STD), 2.9.270.1 (StdfBase), 2.9.276 (sti).

use crate::util::u16le;

/// One style's identity (enough to resolve a heading level + name).
#[derive(Debug, Clone, Default)]
struct StyleDescription {
    sti: u16,
    sgc: u8,
    istd_base: u16,
    name: String,
}

/// The parsed stylesheet: per-`istd` resolved heading level and style name.
#[derive(Debug, Default)]
pub(crate) struct StyleSheet {
    heading: Vec<Option<u8>>,
    names: Vec<String>,
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

    /// Parse the STSH from the table stream. Returns an empty stylesheet (not an
    /// error) on absence/malformation — headings then simply aren't detected.
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
        // StdfBase is 10 bytes (Word97) or 18 (Word2000+); anything else is
        // corrupt — fall back to 10 rather than reading the name from garbage.
        let base_len = if cb_std_base == 18 { 18 } else { 10 } as usize;
        let names_written = u16le(stsh, 6).unwrap_or(0) & 1 != 0;

        let mut p = 2usize.saturating_add(cb_stshi as usize);
        let mut descs: Vec<Option<StyleDescription>> = Vec::with_capacity(cstd as usize);
        for _ in 0..cstd {
            let Some(cb_std) = u16le(stsh, p) else { break };
            p += 2;
            if cb_std == 0 {
                descs.push(None); // empty slot still consumes an istd index
                continue;
            }
            let cb_std = cb_std as usize;
            let Some(std) = stsh.get(p..p.saturating_add(cb_std)) else {
                break;
            };
            p += cb_std;
            descs.push(parse_std(std, base_len, names_written));
        }

        let n = descs.len();
        let mut heading = vec![None; n];
        let mut names = vec![String::new(); n];
        // Per-style cycle guard by epoch: `visited[i]` is the pass (`gen`) that last touched
        // style `i`. "Clearing" between styles is just a fresh `gen` (O(1)) — refilling the
        // whole buffer each pass was O(n) per style = O(n^2) writes (≈4.3e9 for cstd=65535),
        // a CPU DoS on a crafted `.doc`. `gen` starts at 1 so 0 reads as never-visited; `n`
        // is bounded by `cstd` (u16), so `gen = istd + 1` never overflows `u32`.
        let mut visited = vec![0u32; n];
        for istd in 0..n {
            let gen = istd as u32 + 1;
            heading[istd] = resolve_level(&descs, istd, &mut visited, gen, 0);
            if let Some(d) = &descs[istd] {
                names[istd] = d.name.clone();
            }
        }
        StyleSheet { heading, names }
    }
}

fn parse_std(std: &[u8], base_len: usize, names_written: bool) -> Option<StyleDescription> {
    let sti = u16le(std, 0)? & 0x0FFF;
    let grf = u16le(std, 2)?;
    let sgc = (grf & 0x000F) as u8;
    let istd_base = (grf >> 4) & 0x0FFF;
    let mut name = String::new();
    if names_written {
        if let Some(cch) = u16le(std, base_len) {
            let start = base_len + 2;
            if let Some(bytes) = std.get(start..start + cch as usize * 2) {
                name = utf16le(bytes);
            }
        }
    }
    Some(StyleDescription {
        sti,
        sgc,
        istd_base,
        name,
    })
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
