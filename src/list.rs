//! List autonumber reconstruction. Parses the list-definition table (`PlfLst`)
//! and the format-override table (`PlfLfo`), then computes each list
//! paragraph's label (`1.`, `1.1`, `가.`, `(1)` …) the way Word renders it.
//!
//! Reference: [MS-DOC] 2.9.150 PlfLst, 2.9.131 LSTF, 2.9.132 LVL, 2.9.133 LVLF,
//! 2.9.149 PlfLfo, 2.9.129 LFO, 2.9.337 Xst; [MS-OSHARED] 2.2.1.3 MSONFC.

use std::collections::HashMap;

use crate::numfmt;
use crate::util::{u16le, u32le};

/// Max UTF-16 units kept from a list level's number template (`LVL.xst`). Real templates are
/// a handful of placeholders + separators; this bounds a crafted huge one so per-paragraph
/// label generation can't amplify into O(template × paragraphs) output.
const MAX_XST_LEN: usize = 256;

/// One list level: how to format its number and the surrounding template.
#[derive(Debug, Clone, Default)]
struct Level {
    nfc: u8,
    start: i32,
    /// `fNoRestart` — this level does not restart when a shallower level advances.
    no_restart: bool,
    /// 1-based positions in `xst` of each level's number placeholder (0-term).
    rgbxch_nums: [u8; 9],
    /// Character after the number: 0 = tab, 1 = space, 2 = nothing.
    ixch_follow: u8,
    /// The number template: literal UTF-16 chars + placeholders (value = level).
    xst: Vec<u16>,
}

/// A list definition: a stable id and its 1 (simple) or 9 levels.
#[derive(Debug, Clone)]
struct ListDef {
    lsid: i32,
    simple: bool,
    levels: Vec<Level>,
}

/// Parsed list tables, ready to drive a [`Numberer`].
#[derive(Debug, Default)]
pub(crate) struct Lists {
    defs: Vec<ListDef>,
    /// `ilfo` (1-based) → `lsid`, from `PlfLfo.rgLfo`.
    lfo_lsid: Vec<i32>,
}

impl Lists {
    pub(crate) fn is_empty(&self) -> bool {
        self.defs.is_empty() || self.lfo_lsid.is_empty()
    }
}

/// Parse the list tables from the table stream. Returns empty (not an error) on
/// absence or malformation — list rendering then simply does nothing.
pub(crate) fn parse(
    table: &[u8],
    fc_lst: usize,
    lcb_lst: usize,
    fc_lfo: usize,
    lcb_lfo: usize,
) -> Lists {
    Lists {
        defs: parse_plf_lst(table, fc_lst, lcb_lst),
        lfo_lsid: parse_plf_lfo(table, fc_lfo, lcb_lfo),
    }
}

fn parse_plf_lst(table: &[u8], fc: usize, lcb: usize) -> Vec<ListDef> {
    if lcb < 2 {
        return Vec::new();
    }
    let Some(blob) = table.get(fc..fc.saturating_add(lcb)) else {
        return Vec::new();
    };
    let clst = i16::from_le_bytes([blob[0], blob[1]]);
    if clst <= 0 {
        return Vec::new();
    }
    let clst = clst as usize;
    // LSTF headers (28 bytes each) follow cLst.
    let mut headers = Vec::with_capacity(clst);
    for i in 0..clst {
        let off = 2 + i * 28;
        let Some(lsid) = u32le(blob, off).map(|v| v as i32) else {
            return Vec::new();
        };
        let simple = blob.get(off + 26).copied().unwrap_or(0) & 0x01 != 0;
        headers.push((lsid, simple));
    }
    // The LVL array is appended immediately after the PlfLst blob.
    let mut cur = fc + lcb;
    let mut defs = Vec::with_capacity(clst);
    for (lsid, simple) in headers {
        let nlvl = if simple { 1 } else { 9 };
        let mut levels = Vec::with_capacity(nlvl);
        for _ in 0..nlvl {
            match parse_lvl(table, &mut cur) {
                Some(lvl) => levels.push(lvl),
                None => return defs, // truncated — keep what parsed cleanly
            }
        }
        defs.push(ListDef {
            lsid,
            simple,
            levels,
        });
    }
    defs
}

/// Parse one variable-size `LVL` starting at `*cur`, advancing the cursor.
fn parse_lvl(table: &[u8], cur: &mut usize) -> Option<Level> {
    let lvlf = table.get(*cur..*cur + 28)?;
    let start = i32::from_le_bytes(lvlf[0..4].try_into().ok()?);
    let nfc = lvlf[4];
    // Flags byte @5: bit3 = fNoRestart.
    let no_restart = (lvlf[5] >> 3) & 1 != 0;
    let mut rgbxch_nums = [0u8; 9];
    rgbxch_nums.copy_from_slice(&lvlf[6..15]);
    let ixch_follow = lvlf[15];
    let cb_grpprl_chpx = lvlf[24] as usize;
    let cb_grpprl_papx = lvlf[25] as usize;
    *cur += 28;
    // grpprlPapx (sized by cbGrpprlPapx) then grpprlChpx (sized by cbGrpprlChpx).
    *cur = cur.checked_add(cb_grpprl_papx)?;
    *cur = cur.checked_add(cb_grpprl_chpx)?;
    // Xst: cch (u16) then cch UTF-16 chars.
    let cch = u16le(table, *cur)? as usize;
    *cur += 2;
    // A real number template is tiny (≤ a few dozen units). Cap how many we read+store so a
    // crafted huge `cch` can't make every list paragraph render/insert a giant label —
    // O(template × paragraphs) output amplification. Still advance the cursor by the full
    // declared `cch` so subsequent levels stay aligned.
    let take = cch.min(MAX_XST_LEN);
    let mut xst = Vec::with_capacity(take);
    for i in 0..take {
        xst.push(u16le(table, *cur + i * 2)?);
    }
    *cur += 2 * cch;
    Some(Level {
        nfc,
        start,
        no_restart,
        rgbxch_nums,
        ixch_follow,
        xst,
    })
}

/// Parse `PlfLfo` → the `ilfo` → `lsid` map (`rgLfo[i].lsid`). Per-instance
/// `LFOData` overrides are not applied (rare; iStartAt overrides only).
fn parse_plf_lfo(table: &[u8], fc: usize, lcb: usize) -> Vec<i32> {
    if lcb < 4 {
        return Vec::new();
    }
    let Some(blob) = table.get(fc..fc.saturating_add(lcb)) else {
        return Vec::new();
    };
    let lfo_mac = u32le(blob, 0).unwrap_or(0) as usize;
    let mut out = Vec::with_capacity(lfo_mac.min(1 << 16));
    for i in 0..lfo_mac {
        match u32le(blob, 4 + i * 16) {
            Some(lsid) => out.push(lsid as i32),
            None => break,
        }
    }
    out
}

/// Stateful list numberer: advances per-`ilfo` counters in paragraph order and
/// renders each label.
#[derive(Debug)]
pub(crate) struct Numberer<'a> {
    lists: &'a Lists,
    /// `lsid` → index into `lists.defs`, built once. Replaces a per-paragraph
    /// linear scan of every list definition (`def_for`), which a crafted `.doc`
    /// with many list defs + many list paragraphs turned into O(paragraphs × defs)
    /// work at `Document::open` (CPU DoS). First definition wins, matching the
    /// previous `iter().find`.
    lsid_index: HashMap<i32, usize>,
    counters: HashMap<u16, [i32; 9]>,
    seen: HashMap<u16, [bool; 9]>,
}

impl<'a> Numberer<'a> {
    pub(crate) fn new(lists: &'a Lists) -> Self {
        let mut lsid_index = HashMap::with_capacity(lists.defs.len());
        for (i, d) in lists.defs.iter().enumerate() {
            lsid_index.entry(d.lsid).or_insert(i);
        }
        Numberer {
            lists,
            lsid_index,
            counters: HashMap::new(),
            seen: HashMap::new(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.lists.is_empty()
    }

    fn def_for(&self, ilfo: u16) -> Option<&'a ListDef> {
        let lsid = *self.lists.lfo_lsid.get((ilfo as usize).checked_sub(1)?)?;
        self.lists.defs.get(*self.lsid_index.get(&lsid)?)
    }

    /// Advance the counters for paragraph `(ilfo, ilvl)` and return its label
    /// (including the trailing follow character), or `None` for non-list /
    /// bullet / no-number paragraphs.
    pub(crate) fn label(&mut self, ilfo: u16, ilvl: u8) -> Option<String> {
        if ilfo == 0 {
            return None;
        }
        let def = self.def_for(ilfo)?;
        let ilvl = (ilvl as usize).min(8);
        let level = def.levels.get(if def.simple { 0 } else { ilvl })?;

        let lvl_idx = |k: usize| if def.simple { 0 } else { k };

        // Update counters. The active level: start-at on first sight, else +1.
        let cnt = self.counters.entry(ilfo).or_insert([0; 9]);
        let seen = self.seen.entry(ilfo).or_insert([false; 9]);
        if seen[ilvl] {
            cnt[ilvl] = cnt[ilvl].saturating_add(1);
        } else {
            cnt[ilvl] = level.start;
            seen[ilvl] = true;
        }
        // Deeper levels restart on the next occurrence — unless they opt out
        // (`fNoRestart`). (Indexed: `k` indexes both `seen` and `def.levels`.)
        #[allow(clippy::needless_range_loop)]
        for k in (ilvl + 1)..9 {
            let nr = def.levels.get(lvl_idx(k)).is_some_and(|l| l.no_restart);
            if !nr {
                seen[k] = false;
            }
        }
        // Ancestor levels referenced by this level's template but not yet seen
        // are seeded to their start-at, so a deep-first paragraph renders "1.1",
        // not "0.1".
        for k in 0..ilvl {
            if !seen[k] {
                cnt[k] = def.levels.get(lvl_idx(k)).map(|l| l.start).unwrap_or(1);
                seen[k] = true;
            }
        }
        let counters = *cnt;

        // Bullet / none → no number prefix (kept out of the indexed text).
        if level.nfc == 0x17 || level.nfc == 0xFF {
            return None;
        }

        let placeholders: &[u8] = {
            let n = level.rgbxch_nums.iter().take_while(|&&x| x != 0).count();
            &level.rgbxch_nums[..n]
        };
        let mut out = String::new();
        for (pos, &ch) in level.xst.iter().enumerate() {
            if placeholders.contains(&((pos + 1) as u8)) {
                let k = (ch as usize).min(8);
                let knfc = def
                    .levels
                    .get(if def.simple { 0 } else { k })
                    .map(|l| l.nfc)
                    .unwrap_or(level.nfc);
                out.push_str(&numfmt::format(counters[k].max(0) as u32, knfc));
            } else if let Some(c) = char::from_u32(ch as u32) {
                out.push(c);
            }
        }
        match level.ixch_follow {
            0 => out.push('\t'),
            1 => out.push(' '),
            _ => {}
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lvl(start: i32, rgbxch_nums: [u8; 9], ixch_follow: u8, xst: Vec<u16>) -> Level {
        Level {
            nfc: 0,
            start,
            no_restart: false,
            rgbxch_nums,
            ixch_follow,
            xst,
        }
    }

    /// Build a Lists with one simple decimal list ("1." template) for ilfo 1.
    fn decimal_list() -> Lists {
        Lists {
            defs: vec![ListDef {
                lsid: 42,
                simple: true,
                levels: vec![lvl(
                    1,
                    [1, 0, 0, 0, 0, 0, 0, 0, 0],
                    0,
                    vec![0x0000, '.' as u16],
                )],
            }],
            lfo_lsid: vec![42],
        }
    }

    #[test]
    fn parse_lvl_caps_xst_but_advances_cursor_fully() {
        // A crafted LVL declares a huge `cch` template. We keep at most MAX_XST_LEN units
        // (so per-paragraph label generation can't amplify), but still advance the cursor by
        // the FULL declared length so subsequent levels stay aligned.
        let mut buf = vec![0u8; 28]; // LVLF: start/nfc/flags/grpprl-sizes all 0
        let cch = 1000u16;
        buf.extend_from_slice(&cch.to_le_bytes());
        buf.extend(std::iter::repeat(0x41u8).take(cch as usize * 2)); // cch UTF-16 units
        let mut cur = 0usize;
        let lvl = parse_lvl(&buf, &mut cur).expect("LVL parses");
        assert_eq!(lvl.xst.len(), MAX_XST_LEN); // capped, not 1000
        assert_eq!(cur, 28 + 2 + 2 * cch as usize); // cursor advanced by full declared cch
    }

    #[test]
    fn def_for_resolves_correct_definition_by_lsid_among_many() {
        // ilfo 1 maps (via lfo_lsid) to the SECOND definition's lsid. The lsid index
        // must select that def, not the first — and do so without scanning every def
        // (the prior linear `def_for` was an O(paragraphs × defs) DoS).
        let lists = Lists {
            defs: vec![
                ListDef {
                    lsid: 100,
                    simple: true,
                    levels: vec![lvl(
                        5,
                        [5, 0, 0, 0, 0, 0, 0, 0, 0],
                        0,
                        vec![0x0000, '.' as u16],
                    )],
                },
                ListDef {
                    lsid: 200,
                    simple: true,
                    levels: vec![lvl(
                        1,
                        [1, 0, 0, 0, 0, 0, 0, 0, 0],
                        0,
                        vec![0x0000, ')' as u16],
                    )],
                },
            ],
            lfo_lsid: vec![200],
        };
        let mut n = Numberer::new(&lists);
        // Second def (start=1, ")" follow), not the first (would be "5.").
        assert_eq!(n.label(1, 0).as_deref(), Some("1)\t"));
    }

    #[test]
    fn numbers_a_simple_list() {
        let lists = decimal_list();
        let mut n = Numberer::new(&lists);
        assert_eq!(n.label(1, 0).as_deref(), Some("1.\t"));
        assert_eq!(n.label(1, 0).as_deref(), Some("2.\t"));
        assert_eq!(n.label(1, 0).as_deref(), Some("3.\t"));
        // Unknown ilfo / non-list → None.
        assert_eq!(n.label(0, 0), None);
        assert_eq!(n.label(99, 0), None);
    }

    /// ilfo 1: 9-level list, level 0 = "%0.", level 1 = "%0.%1".
    fn multilevel_list() -> Lists {
        let mut levels = vec![
            lvl(1, [1, 0, 0, 0, 0, 0, 0, 0, 0], 2, vec![0x0000, '.' as u16]),
            lvl(
                1,
                [1, 3, 0, 0, 0, 0, 0, 0, 0],
                2,
                vec![0x0000, '.' as u16, 0x0001],
            ),
        ];
        levels.resize(9, Level::default());
        Lists {
            defs: vec![ListDef {
                lsid: 7,
                simple: false,
                levels,
            }],
            lfo_lsid: vec![7],
        }
    }

    #[test]
    fn multilevel_resets_deeper_counter() {
        let lists = multilevel_list();
        let mut n = Numberer::new(&lists);
        assert_eq!(n.label(1, 0).as_deref(), Some("1.")); // level 0 → 1
        assert_eq!(n.label(1, 1).as_deref(), Some("1.1")); // level 1 → 1.1
        assert_eq!(n.label(1, 1).as_deref(), Some("1.2")); // → 1.2
        assert_eq!(n.label(1, 0).as_deref(), Some("2.")); // level 0 → 2 (resets L1)
        assert_eq!(n.label(1, 1).as_deref(), Some("2.1")); // L1 restarted
    }

    #[test]
    fn deep_first_paragraph_seeds_ancestors() {
        // First list paragraph appears at level 1 with no prior level-0 paragraph:
        // the ancestor must be seeded to its start (1), not rendered as 0.
        let lists = multilevel_list();
        let mut n = Numberer::new(&lists);
        assert_eq!(n.label(1, 1).as_deref(), Some("1.1")); // not "0.1"
    }
}
