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
    fc_lim: u32,
    chp: Chp,
}

/// All character runs' properties, sorted by FC, for point lookup by a
/// character's FC.
#[derive(Debug, Default)]
pub(crate) struct ChpxTable {
    entries: Vec<ChpEntry>,
}

impl ChpxTable {
    /// The character properties at `WordDocument` byte offset `fc` (the first run
    /// whose `fc_lim > fc`). Default (all-off) when no CHPX covers `fc`.
    pub(crate) fn chp_at(&self, fc: u32) -> Chp {
        let i = self.entries.partition_point(|e| e.fc_lim <= fc);
        self.entries.get(i).map(|e| e.chp).unwrap_or_default()
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
    let Some(plc) = table.get(fc_plcf..fc_plcf.saturating_add(lcb_plcf)) else {
        return ChpxTable { entries };
    };
    // PlcBteChpx: (n+1) FCs then n PnFkpChpx (4 bytes each). n = (lcb-4)/8.
    let n = plc.len().saturating_sub(4) / 8;
    let pn_base = 4 * (n + 1);
    for i in 0..n {
        let Some(pn_raw) = u32le(plc, pn_base + i * 4) else {
            break;
        };
        let page = (pn_raw & 0x003F_FFFF) as usize; // low 22 bits = page number
        let off = page.saturating_mul(FKP_SIZE);
        parse_fkp(word, off, &mut entries);
    }
    entries.sort_by_key(|e| e.fc_lim);
    ChpxTable { entries }
}

/// Parse one 512-byte CHPX FKP at `page_off`, appending its runs.
fn parse_fkp(word: &[u8], page_off: usize, out: &mut Vec<ChpEntry>) {
    let Some(page) = word.get(page_off..page_off + FKP_SIZE) else {
        return;
    };
    let crun = page[FKP_SIZE - 1] as usize;
    // rgfc is (crun+1) u32; rgb is crun single bytes.
    if crun == 0 || 4 * (crun + 1) + crun >= FKP_SIZE {
        return;
    }
    for i in 0..crun {
        let Some(fc_lim) = u32le(page, 4 * (i + 1)) else {
            break;
        };
        // rgb[i] is a single byte: word offset of this run's CHPX (0 = default).
        let b = page.get(4 * (crun + 1) + i).copied().unwrap_or(0) as usize;
        let chp = if b == 0 {
            Chp::default()
        } else {
            parse_chpx(page, b * 2)
        };
        out.push(ChpEntry { fc_lim, chp });
    }
}

/// Read a `Chpx` (cb byte + grpprl) at `off` within an FKP page.
fn parse_chpx(page: &[u8], off: usize) -> Chp {
    let Some(&cb) = page.get(off) else {
        return Chp::default();
    };
    match page.get(off + 1..off + 1 + cb as usize) {
        Some(gp) => scan_grpprl(gp),
        None => Chp::default(),
    }
}

/// Walk a CHPX grpprl, extracting the styling toggles. Stops on an unsizeable sprm.
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
        let toggle = || matches!(gp.get(op).copied().unwrap_or(0), 0x01 | 0x81);
        match sprm {
            SPRM_C_F_BOLD => chp.bold = toggle(),
            SPRM_C_F_ITALIC => chp.italic = toggle(),
            SPRM_C_F_STRIKE => chp.strike = toggle(),
            SPRM_C_F_VANISH => chp.hidden = toggle(),
            SPRM_C_F_SPEC => fspec = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_C_KUL => chp.underline = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_C_PIC_LOCATION => picloc = u32le(gp, op),
            SPRM_C_HPS => chp.size_half_pt = u16le(gp, op),
            SPRM_C_RG_FTC0 => chp.ftc = u16le(gp, op),
            SPRM_C_CV => {
                // COLORREF: bytes [R, G, B, reserved].
                if let (Some(&r), Some(&g), Some(&b)) = (gp.get(op), gp.get(op + 1), gp.get(op + 2))
                {
                    chp.color = Some(Color { r, g, b });
                }
            }
            // Legacy palette color, only when no 24-bit `sprmCCv` was seen.
            SPRM_C_ICO if chp.color.is_none() => {
                chp.color = ico_color(gp.get(op).copied().unwrap_or(0));
            }
            _ => {}
        }
        pos = op + len;
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
    fn lookup_by_fc() {
        let t = ChpxTable {
            entries: vec![
                ChpEntry {
                    fc_lim: 100,
                    chp: Chp {
                        bold: true,
                        ..Chp::default()
                    },
                },
                ChpEntry {
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
}
