//! Character-property (CHPX) reading — per-run bold/italic/underline/strike/
//! hidden, for the rich document model.
//!
//! The `PlcfBteChpx` bin table (FIB `fcPlcfBteChpx`, in the table stream) points
//! to 512-byte **CHPX FKP** pages in the `WordDocument` stream. Each FKP maps FC
//! ranges to a `Chpx` (`cb` byte + `grpprl`); we scan the grpprl for the
//! character `sprm`s that affect extracted/rendered text.
//!
//! The FKP shape is the PAPX FKP's sibling, with one difference: the per-run
//! offset array `rgb` is a single byte per run (a word offset; `0` = default
//! properties), where PAPX uses a 13-byte `BxPap`.
//!
//! Reference: [MS-DOC] 2.8.26 (PlcBteChpx), 2.9.32 (ChpxFkp), 2.9.31 (Chpx),
//! 2.6.1 (character sprms).

use crate::model::Color;
use crate::util::{u16le, u32le};

const FKP_SIZE: usize = 512;
/// Upper bound on CHPX bin-table page iterations *and* accumulated entries — far above any
/// real `.doc` (character property runs), but it bounds a crafted bin-table that would
/// otherwise amplify a small table into billions of entries (memory/CPU DoS).
const MAX_FKP_ENTRIES: usize = 1 << 20;

// Test-lowerable copy of the cap (see papx.rs); production always uses `MAX_FKP_ENTRIES`.
#[cfg(test)]
thread_local! {
    static TEST_MAX_FKP: std::cell::Cell<usize> = const { std::cell::Cell::new(MAX_FKP_ENTRIES) };
}
#[cfg(test)]
fn set_test_max_fkp(n: usize) {
    TEST_MAX_FKP.with(|c| c.set(n));
}
fn max_fkp_entries() -> usize {
    #[cfg(test)]
    {
        TEST_MAX_FKP.with(|c| c.get())
    }
    #[cfg(not(test))]
    {
        MAX_FKP_ENTRIES
    }
}

// Character sprms (sgc = 2). Toggle operands are 1 byte: 0 off, 1 on, 0x80
// inherit-from-style, 0x81 invert-style.
const SPRM_C_F_BOLD: u16 = 0x0835;
const SPRM_C_F_ITALIC: u16 = 0x0836;
const SPRM_C_F_STRIKE: u16 = 0x0837;
const SPRM_C_F_VANISH: u16 = 0x083C; // hidden text (NOT 0x0838 — that is Outline)
const SPRM_C_F_SPEC: u16 = 0x0855; // run's special char is a real object (1-byte)
const SPRM_C_KUL: u16 = 0x2A3E; // underline kind (0 = none)
const SPRM_C_PIC_LOCATION: u16 = 0x6A03; // fcPic into the Data stream (4-byte)
const SPRM_C_HPS: u16 = 0x4A43; // font size, half-points (2-byte)
const SPRM_C_RG_FTC0: u16 = 0x4A4F; // font index into SttbfFfn (2-byte)
const SPRM_C_CV: u16 = 0x6870; // 24-bit color COLORREF (4-byte)
const SPRM_C_ICO: u16 = 0x2A42; // legacy 0–16 palette color index (1-byte)

/// Map a legacy `sprmCIco` palette index (0–16) to RGB ([MS-DOC] Ico).
fn ico_color(i: u8) -> Option<Color> {
    let (r, g, b) = match i {
        0 | 1 => (0, 0, 0),       // auto / black
        2 => (0, 0, 0xFF),        // blue
        3 => (0, 0xFF, 0xFF),     // cyan
        4 => (0, 0xFF, 0),        // green
        5 => (0xFF, 0, 0xFF),     // magenta
        6 => (0xFF, 0, 0),        // red
        7 => (0xFF, 0xFF, 0),     // yellow
        8 => (0xFF, 0xFF, 0xFF),  // white
        9 => (0, 0, 0x80),        // dark blue
        10 => (0, 0x80, 0x80),    // dark cyan
        11 => (0, 0x80, 0),       // dark green
        12 => (0x80, 0, 0x80),    // dark magenta
        13 => (0x80, 0, 0),       // dark red
        14 => (0x80, 0x80, 0),    // dark yellow
        15 => (0x80, 0x80, 0x80), // dark grey
        16 => (0xC0, 0xC0, 0xC0), // light grey
        _ => return None,
    };
    Some(Color { r, g, b })
}

/// Resolved character properties scanned out of one CHPX grpprl.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Chp {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub hidden: bool,
    /// Font size in half-points (`sprmCHps`), if set.
    pub size_half_pt: Option<u16>,
    /// Font index into `SttbfFfn` (`sprmCRgFtc0`), resolved to a name by the
    /// assembler; `None` = inherit.
    pub ftc: Option<u16>,
    /// Text color (`sprmCCv` 24-bit, or legacy `sprmCIco` palette), if set.
    pub color: Option<Color>,
    /// For a special-char run (`fSpec`) that is an inline picture, the `fcPic`
    /// offset into the `Data` stream.
    pub pic: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
struct ChpEntry {
    fc_start: u32,
    fc_lim: u32,
    chp: Chp,
}

/// All represented character-property ranges, sorted and non-overlapping by
/// FC, for point lookup by a character's FC.
#[derive(Debug, Default)]
pub(crate) struct ChpxTable {
    entries: Vec<ChpEntry>,
}

impl ChpxTable {
    /// The character properties at `WordDocument` byte offset `fc`. Default
    /// (all-off) when no CHPX range covers `fc`.
    pub(crate) fn chp_at(&self, fc: u32) -> Chp {
        let i = self.entries.partition_point(|e| e.fc_lim <= fc);
        self.entries
            .get(i)
            .filter(|e| e.fc_start <= fc)
            .map(|e| e.chp)
            .unwrap_or_default()
    }

    /// The `fcPic` (offset into the `Data` stream) for an inline-picture run at
    /// `fc`, if this run is a picture.
    pub(crate) fn pic_at(&self, fc: u32) -> Option<u32> {
        self.chp_at(fc).pic
    }
}

/// Parse the CHPX bin table + FKP pages. Returns an empty table (not an error)
/// when the structures are absent or malformed — runs then degrade to default
/// (unstyled) properties.
pub(crate) fn parse(word: &[u8], table: &[u8], fc_plcf: usize, lcb_plcf: usize) -> ChpxTable {
    let mut entries = Vec::new();
    if lcb_plcf < 4 {
        return ChpxTable { entries };
    }
    let Some(plc_end) = fc_plcf.checked_add(lcb_plcf) else {
        return ChpxTable { entries };
    };
    let Some(plc) = table.get(fc_plcf..plc_end) else {
        return ChpxTable { entries };
    };
    // PlcBteChpx: (n+1) FCs then n PnFkpChpx (4 bytes each). n = (lcb-4)/8.
    // A PLC size that does not yield a whole number of entries is malformed.
    let payload_len = plc.len() - 4;
    if payload_len % 8 != 0 {
        return ChpxTable { entries };
    }
    let declared_n = payload_len / 8;
    if declared_n == 0 {
        return ChpxTable { entries };
    }
    // The Pn array follows every declared FC, even when processing is capped.
    let Some(pn_base) = declared_n
        .checked_add(1)
        .and_then(|count| count.checked_mul(4))
    else {
        return ChpxTable { entries };
    };

    // Bound page iterations and accumulated entries: a crafted .doc can make
    // `n` huge and point every page number at one valid FKP page, amplifying a
    // small table into billions of entries (memory/CPU DoS).
    let cap = max_fkp_entries();
    let n = declared_n.min(cap);

    // Validate every outer range that this bounded parse will process before
    // accepting any page, so malformed order cannot yield a partial table.
    for i in 0..n {
        let Some(fc_start) = u32le(plc, i * 4) else {
            return ChpxTable {
                entries: Vec::new(),
            };
        };
        let Some(fc_lim) = u32le(plc, (i + 1) * 4) else {
            return ChpxTable {
                entries: Vec::new(),
            };
        };
        if fc_start >= fc_lim {
            return ChpxTable {
                entries: Vec::new(),
            };
        }
    }

    for i in 0..n {
        if entries.len() >= cap {
            break;
        }
        let Some(pn_raw) = u32le(plc, pn_base + i * 4) else {
            break;
        };
        let page = (pn_raw & 0x003F_FFFF) as usize; // low 22 bits = page number
        let Some(off) = page.checked_mul(FKP_SIZE) else {
            continue;
        };
        let Some(fc_start) = u32le(plc, i * 4) else {
            break;
        };
        let Some(fc_lim) = u32le(plc, (i + 1) * 4) else {
            break;
        };
        parse_fkp(
            word,
            off,
            fc_start,
            fc_lim,
            cap - entries.len(),
            &mut entries,
        );
    }
    ChpxTable { entries }
}

/// Parse one 512-byte CHPX FKP at `page_off`, appending its runs.
fn parse_fkp(
    word: &[u8],
    page_off: usize,
    outer_start: u32,
    outer_lim: u32,
    budget: usize,
    out: &mut Vec<ChpEntry>,
) {
    if budget == 0 {
        return;
    }
    let Some(page_end) = page_off.checked_add(FKP_SIZE) else {
        return;
    };
    let Some(page) = word.get(page_off..page_end) else {
        return;
    };
    let crun = page[FKP_SIZE - 1] as usize;
    // rgfc is (crun+1) u32; rgb is crun single bytes.
    if !(1..=0x65).contains(&crun) {
        return;
    }
    let rgfc_end = 4 * (crun + 1);
    let rgb_start = rgfc_end;
    let rgb_end = rgb_start + crun;
    if rgb_end > FKP_SIZE - 1 {
        return;
    }

    // Every run is [rgfc[i], rgfc[i+1]); malformed pages are ignored as a
    // unit so sorting or duplicates cannot create ambiguous lookup ranges.
    for i in 0..crun {
        let Some(fc_start) = u32le(page, i * 4) else {
            return;
        };
        let Some(fc_lim) = u32le(page, (i + 1) * 4) else {
            return;
        };
        if fc_start >= fc_lim {
            return;
        }
    }

    let mut remaining = budget;
    for i in 0..crun {
        if remaining == 0 {
            break;
        }
        let Some(run_start) = u32le(page, i * 4) else {
            break;
        };
        let Some(run_lim) = u32le(page, (i + 1) * 4) else {
            break;
        };
        let fc_start = run_start.max(outer_start);
        let fc_lim = run_lim.min(outer_lim);
        if fc_start >= fc_lim {
            continue;
        }
        let Some(&b) = page.get(rgb_start + i) else {
            break;
        };
        let chp = if b == 0 {
            Chp::default()
        } else {
            parse_chpx(page, b as usize * 2, rgb_end).unwrap_or_default()
        };
        out.push(ChpEntry {
            fc_start,
            fc_lim,
            chp,
        });
        remaining -= 1;
    }
}

/// Read a `Chpx` (cb byte + grpprl) at `off` within an FKP page.
fn parse_chpx(page: &[u8], off: usize, metadata_end: usize) -> Option<Chp> {
    // Nonzero rgb offsets point after rgfc+rgb, and neither the size byte nor
    // grpprl may consume the final crun byte.
    if off < metadata_end || off >= FKP_SIZE - 1 {
        return None;
    }
    let cb = *page.get(off)? as usize;
    let data_start = off.checked_add(1)?;
    let data_end = data_start.checked_add(cb)?;
    if data_end > FKP_SIZE - 1 {
        return None;
    }
    Some(scan_grpprl(page.get(data_start..data_end)?))
}

/// Walk a CHPX grpprl, extracting the styling toggles. Stops on an unsizeable
/// or truncated sprm.
fn scan_grpprl(gp: &[u8]) -> Chp {
    let mut chp = Chp::default();
    let mut fspec = false;
    let mut picloc = None;
    let mut pos = 0;
    while pos + 2 <= gp.len() {
        let Some(sprm) = u16le(gp, pos) else { break };
        let op = pos + 2;
        let Some(len) = operand_len(sprm, gp, op) else {
            break;
        };
        let Some(operand_end) = op.checked_add(len) else {
            break;
        };
        let Some(operand) = gp.get(op..operand_end) else {
            break;
        };
        let toggle = || matches!(operand.first().copied().unwrap_or(0), 0x01 | 0x81);
        match sprm {
            SPRM_C_F_BOLD => chp.bold = toggle(),
            SPRM_C_F_ITALIC => chp.italic = toggle(),
            SPRM_C_F_STRIKE => chp.strike = toggle(),
            SPRM_C_F_VANISH => chp.hidden = toggle(),
            SPRM_C_F_SPEC => fspec = operand.first().copied().unwrap_or(0) != 0,
            SPRM_C_KUL => chp.underline = operand.first().copied().unwrap_or(0) != 0,
            SPRM_C_PIC_LOCATION => picloc = u32le(operand, 0),
            SPRM_C_HPS => chp.size_half_pt = u16le(operand, 0),
            SPRM_C_RG_FTC0 => chp.ftc = u16le(operand, 0),
            SPRM_C_CV => {
                // COLORREF: bytes [R, G, B, reserved].
                chp.color = Some(Color {
                    r: operand[0],
                    g: operand[1],
                    b: operand[2],
                });
            }
            // Legacy palette color, only when no 24-bit `sprmCCv` was seen.
            SPRM_C_ICO if chp.color.is_none() => {
                chp.color = ico_color(operand.first().copied().unwrap_or(0));
            }
            _ => {}
        }
        pos = operand_end;
    }
    // A picture run sets both fSpec and a picture location.
    chp.pic = if fspec { picloc } else { None };
    chp
}

/// Operand length for a sprm, from its `spra` field ([MS-DOC] 2.2.5). Character
/// sprms never use the `sprmTDefTable` special case, so the generic `spra == 6`
/// path (a leading length byte) suffices.
fn operand_len(sprm: u16, data: &[u8], op: usize) -> Option<usize> {
    match (sprm >> 13) & 0x7 {
        0 | 1 => Some(1),
        2 | 4 | 5 => Some(2),
        3 => Some(4),
        7 => Some(3),
        6 => Some(1 + *data.get(op)? as usize),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOLD_ON: &[u8] = &[0x35, 0x08, 0x01];
    const ITALIC_ON: &[u8] = &[0x36, 0x08, 0x01];

    fn plc(boundaries: &[u32], pages: &[u32]) -> Vec<u8> {
        assert_eq!(boundaries.len(), pages.len() + 1);
        let pn_base = boundaries.len() * 4;
        let mut bytes = vec![0; pn_base + pages.len() * 4];
        for (i, value) in boundaries.iter().chain(pages).enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    fn fkp(boundaries: &[u32], grpprls: &[Option<&[u8]>]) -> [u8; FKP_SIZE] {
        assert_eq!(boundaries.len(), grpprls.len() + 1);
        assert!(grpprls.len() <= 0x65);
        let crun = grpprls.len();
        let rgb_start = boundaries.len() * 4;
        let mut page = [0u8; FKP_SIZE];
        for (i, value) in boundaries.iter().enumerate() {
            page[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut payload = (rgb_start + crun + 1) & !1;
        for (i, grpprl) in grpprls.iter().enumerate() {
            let Some(grpprl) = grpprl else { continue };
            assert!(grpprl.len() <= u8::MAX as usize);
            assert!(payload + 1 + grpprl.len() < FKP_SIZE - 1);
            page[rgb_start + i] = (payload / 2) as u8;
            page[payload] = grpprl.len() as u8;
            page[payload + 1..payload + 1 + grpprl.len()].copy_from_slice(grpprl);
            payload = (payload + 1 + grpprl.len() + 1) & !1;
        }
        page[FKP_SIZE - 1] = crun as u8;
        page
    }

    fn word_with_pages(pages: &[(usize, [u8; FKP_SIZE])]) -> Vec<u8> {
        let page_count = pages
            .iter()
            .map(|(number, _)| number + 1)
            .max()
            .unwrap_or(0);
        let mut word = vec![0; page_count * FKP_SIZE];
        for (number, page) in pages {
            let start = number * FKP_SIZE;
            word[start..start + FKP_SIZE].copy_from_slice(page);
        }
        word
    }

    #[test]
    fn scans_bold_italic_hidden() {
        // sprmCFBold=1, sprmCFItalic=1, sprmCFVanish=1.
        let chp = scan_grpprl(&[0x35, 0x08, 0x01, 0x36, 0x08, 0x01, 0x3C, 0x08, 0x01]);
        assert!(chp.bold && chp.italic && chp.hidden);
        assert!(!chp.strike && !chp.underline);
    }

    #[test]
    fn bold_off_and_inherit() {
        assert!(!scan_grpprl(&[0x35, 0x08, 0x00]).bold); // explicit off
        assert!(!scan_grpprl(&[0x35, 0x08, 0x80]).bold); // inherit-from-style
        assert!(scan_grpprl(&[0x35, 0x08, 0x81]).bold); // invert (of default-off)
    }

    #[test]
    fn underline_and_strike() {
        let chp = scan_grpprl(&[0x3E, 0x2A, 0x01, 0x37, 0x08, 0x01]);
        assert!(chp.underline && chp.strike);
    }

    #[test]
    fn skips_unknown_sprm_by_spra() {
        // A 2-byte-operand sprm (spra=2, e.g. sprmCHps 0x4A43) then bold.
        let chp = scan_grpprl(&[0x43, 0x4A, 0xAA, 0xBB, 0x35, 0x08, 0x01]);
        assert!(chp.bold);
    }

    #[test]
    fn truncated_operands_preserve_prior_complete_properties() {
        let chp = scan_grpprl(&[
            0x35, 0x08, 0x01, // bold on
            0x35, 0x08, // truncated bold toggle
        ]);
        assert!(chp.bold);

        let chp = scan_grpprl(&[
            0x43, 0x4A, 0x18, 0x00, // 12 pt
            0x43, 0x4A, 0x20, // truncated font size
        ]);
        assert_eq!(chp.size_half_pt, Some(24));

        let chp = scan_grpprl(&[
            0x36, 0x08, 0x01, // italic on
            0x00, 0xC0, 0x04, 0xAA, // variable operand declares four bytes
        ]);
        assert!(chp.italic);
    }

    #[test]
    fn declared_plc_count_locates_page_numbers_when_processing_is_capped() {
        let word = word_with_pages(&[(1, fkp(&[100, 200], &[Some(BOLD_ON)]))]);
        let plc = plc(&[100, 200, 300], &[1, 0]);

        set_test_max_fkp(1);
        let table = parse(&word, &plc, 0, plc.len());
        set_test_max_fkp(MAX_FKP_ENTRIES);

        assert!(table.chp_at(150).bold);
        assert_eq!(table.entries.len(), 1);
    }

    #[test]
    fn cumulative_entry_budget_is_exact() {
        let boundaries = (0..=0x65).collect::<Vec<_>>();
        let grpprls = vec![None; 0x65];
        let word = word_with_pages(&[(0, fkp(&boundaries, &grpprls))]);
        let plc = plc(&[0, 0x65], &[0]);

        set_test_max_fkp(64);
        let table = parse(&word, &plc, 0, plc.len());
        set_test_max_fkp(MAX_FKP_ENTRIES);

        assert_eq!(table.entries.len(), 64);
    }

    #[test]
    fn malformed_or_unordered_plc_is_ignored() {
        let word = word_with_pages(&[(0, fkp(&[100, 200], &[Some(BOLD_ON)]))]);

        let mut noncanonical = plc(&[100, 200], &[0]);
        noncanonical.push(0);
        assert!(parse(&word, &noncanonical, 0, noncanonical.len())
            .entries
            .is_empty());

        let unordered = plc(&[200, 100], &[0]);
        assert!(parse(&word, &unordered, 0, unordered.len())
            .entries
            .is_empty());
    }

    #[test]
    fn malformed_fkp_ranges_are_ignored() {
        for boundaries in [[100, 200, 150], [100, 100, 200]] {
            let word = word_with_pages(&[(0, fkp(&boundaries, &[None, None]))]);
            let plc = plc(&[100, 200], &[0]);
            assert!(parse(&word, &plc, 0, plc.len()).entries.is_empty());
        }
    }

    #[test]
    fn lookup_respects_run_starts_and_uncovered_gaps() {
        let word = word_with_pages(&[
            (0, fkp(&[120, 180], &[Some(BOLD_ON)])),
            (1, fkp(&[220, 280], &[Some(ITALIC_ON)])),
        ]);
        let plc = plc(&[100, 200, 300], &[0, 1]);
        let table = parse(&word, &plc, 0, plc.len());

        assert_eq!(table.chp_at(119), Chp::default());
        assert!(table.chp_at(120).bold);
        assert!(table.chp_at(179).bold);
        assert_eq!(table.chp_at(180), Chp::default());
        assert_eq!(table.chp_at(219), Chp::default());
        assert!(table.chp_at(220).italic);
        assert!(table.chp_at(279).italic);
        assert_eq!(table.chp_at(280), Chp::default());
    }

    #[test]
    fn chpx_offsets_cannot_target_metadata_or_crun() {
        let mut metadata = [0u8; FKP_SIZE];
        metadata[0..4].copy_from_slice(&100u32.to_le_bytes());
        metadata[4..8].copy_from_slice(&0x0108_3503u32.to_le_bytes());
        metadata[8] = 2; // offset 4, inside rgfc
        metadata[FKP_SIZE - 1] = 1;
        let word = word_with_pages(&[(0, metadata)]);
        let metadata_plc = plc(&[100, 0x0108_3503], &[0]);
        assert_eq!(
            parse(&word, &metadata_plc, 0, metadata_plc.len()).chp_at(100),
            Chp::default()
        );

        let mut overlaps_crun = [0u8; FKP_SIZE];
        overlaps_crun[0..4].copy_from_slice(&100u32.to_le_bytes());
        overlaps_crun[4..8].copy_from_slice(&200u32.to_le_bytes());
        overlaps_crun[8] = 254; // offset 508
        overlaps_crun[508] = 3;
        overlaps_crun[509] = 0x35;
        overlaps_crun[510] = 0x08;
        overlaps_crun[FKP_SIZE - 1] = 1; // also the apparent bold operand
        let word = word_with_pages(&[(0, overlaps_crun)]);
        let crun_plc = plc(&[100, 200], &[0]);
        assert_eq!(
            parse(&word, &crun_plc, 0, crun_plc.len()).chp_at(150),
            Chp::default()
        );
    }

    #[test]
    fn lookup_by_fc() {
        let t = ChpxTable {
            entries: vec![
                ChpEntry {
                    fc_start: 0,
                    fc_lim: 100,
                    chp: Chp {
                        bold: true,
                        ..Chp::default()
                    },
                },
                ChpEntry {
                    fc_start: 100,
                    fc_lim: 200,
                    chp: Chp {
                        italic: true,
                        ..Chp::default()
                    },
                },
            ],
        };
        assert!(t.chp_at(50).bold);
        assert!(t.chp_at(150).italic);
        assert!(!t.chp_at(250).bold && !t.chp_at(250).italic); // past end = default
    }

    #[test]
    fn bin_table_page_iterations_are_capped() {
        let word = word_with_pages(&[(0, fkp(&[0, 10], &[None]))]);
        let boundaries = (0..=10).collect::<Vec<_>>();
        let pages = vec![0; 10];
        let plc = plc(&boundaries, &pages);

        set_test_max_fkp(3);
        let table = parse(&word, &plc, 0, plc.len());
        set_test_max_fkp(MAX_FKP_ENTRIES);

        assert_eq!(table.entries.len(), 3);
        assert_eq!(table.entries[0].fc_start, 0);
        assert_eq!(table.entries[2].fc_lim, 3);
    }
}
