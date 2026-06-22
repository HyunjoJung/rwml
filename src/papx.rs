//! Paragraph-property (PAPX) reading — the minimum needed to reconstruct table
//! structure: for each paragraph, whether it is inside a table (`fInTable`) and
//! whether it is a table-terminating paragraph (`fTtp`, the row end).
//!
//! The `PlcfBtePapx` bin table (FIB `fcPlcfBtePapx`, in the table stream) points
//! to 512-byte **PAPX FKP** pages in the `WordDocument` stream. Each FKP maps FC
//! ranges to a `grpprl` (a list of `sprm` property modifiers); we scan those for
//! `sprmPFInTable` (0x2416) and `sprmPFTtp` (0x2417).
//!
//! Reference: [MS-DOC] 2.8.25 (PlcBtePapx), 2.9.137 (PapxInFkp), 2.6.2 (sprm).

use crate::table::TableDef;
use crate::util::{u16le, u32le};

const FKP_SIZE: usize = 512;
const SPRM_P_ISTD: u16 = 0x4600; // direct istd override (2-byte)
const SPRM_P_JC: u16 = 0x2403; // paragraph justification (1-byte)
const SPRM_P_FIN_TABLE: u16 = 0x2416;
const SPRM_P_FTTP: u16 = 0x2417;
const SPRM_P_OUT_LVL: u16 = 0x2640; // outline level 0..8, 9 = body (1-byte)
const SPRM_P_ILVL: u16 = 0x260A;
const SPRM_T_TABLE_HEADER: u16 = 0x3404; // row repeats as a header (1-byte)
const SPRM_P_ILFO: u16 = 0x460B;
const SPRM_T_DEF_TABLE: u16 = 0xD608;

/// Per-paragraph properties over an FC range `[fc_start, fc_lim)`.
#[derive(Debug, Clone, Default)]
struct PapEntry {
    fc_lim: u32,
    in_table: bool,
    ttp: bool,
    /// `ilfo` — list-format-override index (1-based). 0 = not a list paragraph.
    ilfo: u16,
    /// `ilvl` — list level (0-based).
    ilvl: u8,
    /// `istd` — paragraph style index (into the STSH), for heading resolution.
    istd: u16,
    /// `sprmPOutLvl` operand (0..8 = outline levels 1..9, 9 = body), if present.
    outlvl: Option<u8>,
    /// `sprmPJc` — justification (0 left, 1 center, 2 right, 3/4 justify).
    jc: u8,
    /// Row repeats as a table header (`sprmTTableHeader`).
    table_header: bool,
    /// Parsed `sprmTDefTable` row definition — present only on TTP paragraphs.
    table_def: Option<TableDef>,
}

/// Per-paragraph properties scanned out of one grpprl.
#[derive(Debug, Clone, Copy, Default)]
struct Pap {
    in_table: bool,
    ttp: bool,
    ilfo: u16,
    ilvl: u8,
    istd: u16,
    outlvl: Option<u8>,
    jc: u8,
    table_header: bool,
}

/// All paragraphs' properties, sorted by FC, for point lookup by a mark's FC.
#[derive(Debug, Default)]
pub(crate) struct PapxTable {
    entries: Vec<PapEntry>,
}

impl PapxTable {
    /// The paragraph whose mark sits at byte offset `fc` (the first entry whose
    /// `fc_lim > fc`, since entries are sorted by `fc_lim`).
    fn entry_at(&self, fc: u32) -> Option<&PapEntry> {
        let i = self.entries.partition_point(|e| e.fc_lim <= fc);
        self.entries.get(i)
    }

    /// Table state of the paragraph at `fc`: `(in_table, is_row_end)`.
    pub(crate) fn at(&self, fc: u32) -> (bool, bool) {
        self.entry_at(fc)
            .map(|e| (e.in_table, e.ttp))
            .unwrap_or((false, false))
    }

    /// List membership of the paragraph at `fc`: `(ilfo, ilvl)` — `ilfo` 0 means
    /// not a list paragraph.
    pub(crate) fn list_at(&self, fc: u32) -> (u16, u8) {
        self.entry_at(fc)
            .map(|e| (e.ilfo, e.ilvl))
            .unwrap_or((0, 0))
    }

    /// Style info of the paragraph at `fc`: `(istd, outline_level, justification)`.
    pub(crate) fn style_at(&self, fc: u32) -> (u16, Option<u8>, u8) {
        self.entry_at(fc)
            .map(|e| (e.istd, e.outlvl, e.jc))
            .unwrap_or((0, None, 0))
    }

    /// The `sprmTDefTable` row definition for the row ending at `fc` (TTP), if any.
    pub(crate) fn table_def_at(&self, fc: u32) -> Option<&TableDef> {
        self.entry_at(fc).and_then(|e| e.table_def.as_ref())
    }

    /// Whether the row ending at `fc` repeats as a table header.
    pub(crate) fn table_header_at(&self, fc: u32) -> bool {
        self.entry_at(fc).map(|e| e.table_header).unwrap_or(false)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Parse the PAPX bin table + FKP pages into a [`PapxTable`]. Returns an empty
/// table (not an error) if the structures are absent or malformed — table
/// reconstruction then simply degrades to the plain-paragraph rendering.
pub(crate) fn parse(word: &[u8], table: &[u8], fc_plcf: usize, lcb_plcf: usize) -> PapxTable {
    let mut entries = Vec::new();
    if lcb_plcf < 4 {
        return PapxTable { entries };
    }
    let Some(plc) = table.get(fc_plcf..fc_plcf.saturating_add(lcb_plcf)) else {
        return PapxTable { entries };
    };
    // PlcBtePapx: (n+1) FCs then n PnFkpPapx (4 bytes each). n = (lcb-4)/8.
    let n = (plc.len().saturating_sub(4)) / 8;
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
    PapxTable { entries }
}

/// Parse one 512-byte PAPX FKP at `page_off`, appending its paragraphs.
fn parse_fkp(word: &[u8], page_off: usize, out: &mut Vec<PapEntry>) {
    let Some(page) = word.get(page_off..page_off + FKP_SIZE) else {
        return;
    };
    let crun = page[FKP_SIZE - 1] as usize;
    if crun == 0 || 4 * (crun + 1) + 13 * crun >= FKP_SIZE {
        return;
    }
    for i in 0..crun {
        let fc_lim = match u32le(page, 4 * (i + 1)) {
            Some(v) => v,
            None => break,
        };
        // BxPap[i]: bOffset(1) + PHE(12); papx at bOffset*2 within the page.
        let bx_off = 4 * (crun + 1) + i * 13;
        let b_offset = page.get(bx_off).copied().unwrap_or(0) as usize;
        let (pap, table_def) = if b_offset == 0 {
            (Pap::default(), None)
        } else {
            parse_papx(page, b_offset * 2)
        };
        out.push(PapEntry {
            fc_lim,
            in_table: pap.in_table,
            ttp: pap.ttp,
            ilfo: pap.ilfo,
            ilvl: pap.ilvl,
            istd: pap.istd,
            outlvl: pap.outlvl,
            jc: pap.jc,
            table_header: pap.table_header,
            table_def,
        });
    }
}

/// Read a `PapxInFkp` at `off` within an FKP and scan its grpprl, returning the
/// scalar properties plus any `sprmTDefTable` row definition.
fn parse_papx(page: &[u8], off: usize) -> (Pap, Option<TableDef>) {
    let Some(&cb) = page.get(off) else {
        return (Pap::default(), None);
    };
    // GrpprlAndIstd = istd(2) + grpprl. Size depends on whether cb is 0.
    let (data_off, data_len) = if cb != 0 {
        (off + 1, (cb as usize) * 2 - 1)
    } else {
        let cb2 = page.get(off + 1).copied().unwrap_or(0) as usize;
        (off + 2, cb2 * 2)
    };
    if data_len < 2 {
        return (Pap::default(), None);
    }
    // The leading u16 of GrpprlAndIstd is the paragraph style index (istd); the
    // grpprl follows. A grpprl-level sprmPIstd overrides it.
    let istd = u16le(page, data_off).unwrap_or(0);
    match page.get(data_off + 2..data_off + data_len) {
        Some(gp) => scan_grpprl(gp, istd),
        None => (
            Pap {
                istd,
                ..Pap::default()
            },
            None,
        ),
    }
}

/// Walk a grpprl, extracting table flags, list (`ilfo`/`ilvl`), style index,
/// outline level, and justification. Stops on an unsizeable sprm.
fn scan_grpprl(gp: &[u8], istd: u16) -> (Pap, Option<TableDef>) {
    let mut pap = Pap {
        istd,
        ..Pap::default()
    };
    let mut table_def = None;
    let mut pos = 0;
    while pos + 2 <= gp.len() {
        let Some(sprm) = u16le(gp, pos) else { break };
        let op = pos + 2;
        let Some(len) = operand_len(sprm, gp, op) else {
            break;
        };
        match sprm {
            SPRM_P_ISTD => pap.istd = u16le(gp, op).unwrap_or(istd),
            SPRM_P_JC => pap.jc = gp.get(op).copied().unwrap_or(0),
            SPRM_P_FIN_TABLE => pap.in_table = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_P_FTTP => pap.ttp = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_P_OUT_LVL => pap.outlvl = Some(gp.get(op).copied().unwrap_or(9)),
            SPRM_T_TABLE_HEADER => pap.table_header = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_P_ILVL => pap.ilvl = gp.get(op).copied().unwrap_or(0),
            SPRM_P_ILFO => pap.ilfo = u16le(gp, op).unwrap_or(0),
            SPRM_T_DEF_TABLE => {
                if let Some(operand) = gp.get(op..op + len) {
                    table_def = TableDef::parse(operand);
                }
            }
            _ => {}
        }
        pos = op + len;
    }
    (pap, table_def)
}

/// Operand length for a sprm, from its `spra` field ([MS-DOC] 2.2.5).
fn operand_len(sprm: u16, data: &[u8], op: usize) -> Option<usize> {
    match (sprm >> 13) & 0x7 {
        0 | 1 => Some(1),
        2 | 4 | 5 => Some(2),
        3 => Some(4),
        7 => Some(3),
        6 => {
            if sprm == SPRM_T_DEF_TABLE {
                // [MS-DOC] 2.9.349: the leading u16 `cb` is the remainder length
                // PLUS ONE, so total operand = cb-field(2) + (cb-1) = cb + 1.
                let cb = u16le(data, op)? as usize;
                (cb != 0).then_some(1 + cb)
            } else {
                Some(1 + *data.get(op)? as usize)
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_table_flags() {
        // grpprl with sprmPFInTable=1 then sprmPFTtp=1.
        let (p, _) = scan_grpprl(&[0x16, 0x24, 0x01, 0x17, 0x24, 0x01], 0);
        assert!(p.in_table && p.ttp);
        // a 2-byte-operand sprm (spra=2, e.g. 0x4400) then fInTable.
        let (p2, _) = scan_grpprl(&[0x00, 0x44, 0xAA, 0xBB, 0x16, 0x24, 0x01], 0);
        assert!(p2.in_table && !p2.ttp);
    }

    #[test]
    fn scans_list_props() {
        // sprmPIlvl (0x260A, 1-byte) = 2, then sprmPIlfo (0x460B, 2-byte) = 5.
        let (p, _) = scan_grpprl(&[0x0A, 0x26, 0x02, 0x0B, 0x46, 0x05, 0x00], 0);
        assert_eq!((p.ilfo, p.ilvl), (5, 2));
    }

    #[test]
    fn scans_style_outline_align() {
        // leading istd = 7; sprmPJc(0x2403)=1 center; sprmPOutLvl(0x2640)=2.
        let (p, _) = scan_grpprl(&[0x03, 0x24, 0x01, 0x40, 0x26, 0x02], 7);
        assert_eq!(p.istd, 7);
        assert_eq!(p.jc, 1);
        assert_eq!(p.outlvl, Some(2));
        // sprmPIstd (0x4600, 2-byte) overrides the leading istd.
        let (p2, _) = scan_grpprl(&[0x00, 0x46, 0x05, 0x00], 7);
        assert_eq!(p2.istd, 5);
    }

    #[test]
    fn parses_tdeftable_then_reads_flags() {
        // sprmTDefTable (0xD608) with cb=26 (remainder 25 bytes), then the table
        // flags. The walker must skip exactly cb+1 = 27 operand bytes and parse
        // the row definition.
        let mut gp = vec![0x08, 0xD6, 0x1A, 0x00]; // sprm + cb=26
        gp.push(1); // itcMac = 1
        gp.extend_from_slice(&0i16.to_le_bytes()); // rgdxa[0]
        gp.extend_from_slice(&100i16.to_le_bytes()); // rgdxa[1]
        gp.extend_from_slice(&[0u8; 20]); // one TC80 (tcgrf=0 + padding)
        gp.extend_from_slice(&[0x16, 0x24, 0x01]); // sprmPFInTable = 1
        gp.extend_from_slice(&[0x17, 0x24, 0x01]); // sprmPFTtp = 1
        let (p, def) = scan_grpprl(&gp, 0);
        assert!(p.in_table && p.ttp);
        assert_eq!(def.unwrap().rgdxa, vec![0, 100]);
    }

    #[test]
    fn lookup_by_fc() {
        let mk = |fc_lim, in_table, ttp| PapEntry {
            fc_lim,
            in_table,
            ttp,
            ilfo: 0,
            ilvl: 0,
            istd: 0,
            outlvl: None,
            jc: 0,
            table_header: false,
            table_def: None,
        };
        let t = PapxTable {
            entries: vec![
                mk(100, false, false),
                mk(200, true, false),
                mk(300, true, true),
            ],
        };
        assert_eq!(t.at(50), (false, false)); // first paragraph
        assert_eq!(t.at(150), (true, false)); // cell paragraph
        assert_eq!(t.at(250), (true, true)); // row-terminating
        assert_eq!(t.at(999), (false, false)); // past the end
    }
}
