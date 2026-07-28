//! Paragraph-property (PAPX) reading — the minimum needed to reconstruct table
//! structure and source paragraph layout: table membership/termination, list
//! and style references, direct pagination and BiDi/justification controls,
//! and row properties.
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
/// Upper bound on PAPX bin-table page iterations *and* accumulated entries — far above any
/// real `.doc` (a 1000-page document has well under this many paragraph property runs), but
/// it bounds a crafted bin-table that would otherwise amplify into billions of entries.
const MAX_FKP_ENTRIES: usize = 1 << 20;

// A test-lowerable copy of the cap so the over-cap break can be exercised on a tiny fixture
// instead of a multi-million-entry one. Production always uses `MAX_FKP_ENTRIES`.
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
const SPRM_P_ISTD: u16 = 0x4600; // direct istd override (2-byte)
const SPRM_P_ISTD_PERMUTE: u16 = 0xC601; // conditional paragraph-style remap
const SPRM_P_JC_80: u16 = 0x2403; // physical paragraph justification (1-byte)
const SPRM_P_FKEEP: u16 = 0x2405;
const SPRM_P_FKEEP_FOLLOW: u16 = 0x2406;
const SPRM_P_FPAGE_BREAK_BEFORE: u16 = 0x2407;
const SPRM_P_FIN_TABLE: u16 = 0x2416;
const SPRM_P_FTTP: u16 = 0x2417;
const SPRM_P_FWIDOW_CONTROL: u16 = 0x2431;
const SPRM_P_F_BIDI: u16 = 0x2441;
const SPRM_P_JC: u16 = 0x2461; // logical paragraph justification (1-byte)
const SPRM_P_DXA_RIGHT: u16 = 0x845D; // logical right indent (signed twips)
const SPRM_P_DXA_LEFT: u16 = 0x845E; // logical left indent (signed twips)
const SPRM_P_NEST: u16 = 0x465F; // additive logical left indent (signed twips)
const SPRM_P_DXA_LEFT_1: u16 = 0x8460; // logical first-line offset (signed twips)
const SPRM_P_OUT_LVL: u16 = 0x2640; // outline level 0..8, 9 = body (1-byte)
const SPRM_P_ILVL: u16 = 0x260A;
const SPRM_T_FCANT_SPLIT_90: u16 = 0x3403;
const SPRM_T_TABLE_HEADER: u16 = 0x3404; // row repeats as a header (1-byte)
const SPRM_T_FCANT_SPLIT: u16 = 0x3466;
const SPRM_P_ILFO: u16 = 0x460B;
const SPRM_T_F_BIDI: u16 = 0x560B;
const SPRM_T_F_BIDI_90: u16 = 0x5664;
const SPRM_T_DEF_TABLE: u16 = 0xD608;

/// Direct paragraph pagination resolved against the MS-DOC format defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ParagraphPagination {
    pub(crate) keep_next: bool,
    pub(crate) keep_lines: bool,
    pub(crate) page_break_before: bool,
    pub(crate) widow_control: bool,
}

impl Default for ParagraphPagination {
    fn default() -> Self {
        Self {
            keep_next: false,
            keep_lines: false,
            page_break_before: false,
            widow_control: true,
        }
    }
}

/// Sparse paragraph pagination modifiers. `None` means the source did not
/// specify that property; this distinction is required when direct PAPX
/// formatting overrides an inherited paragraph style.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParagraphPaginationOverrides {
    pub(crate) keep_next: Option<bool>,
    pub(crate) keep_lines: Option<bool>,
    pub(crate) page_break_before: Option<bool>,
    pub(crate) widow_control: Option<bool>,
}

impl ParagraphPagination {
    pub(crate) fn apply(self, overrides: ParagraphPaginationOverrides) -> Self {
        Self {
            keep_next: overrides.keep_next.unwrap_or(self.keep_next),
            keep_lines: overrides.keep_lines.unwrap_or(self.keep_lines),
            page_break_before: overrides
                .page_break_before
                .unwrap_or(self.page_break_before),
            widow_control: overrides.widow_control.unwrap_or(self.widow_control),
        }
    }
}

impl From<ParagraphPagination> for ParagraphPaginationOverrides {
    fn from(value: ParagraphPagination) -> Self {
        Self {
            keep_next: Some(value.keep_next),
            keep_lines: Some(value.keep_lines),
            page_break_before: Some(value.page_break_before),
            widow_control: Some(value.widow_control),
        }
    }
}

/// Direction-independent representation of the bounded legacy justification
/// values that the shared physical-alignment model can preserve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParagraphJustification {
    PhysicalLeft,
    Center,
    PhysicalRight,
    Justify,
    LogicalStart,
    LogicalEnd,
    UnsupportedIndented,
}

/// Sparse direct paragraph layout modifiers. Logical justification remains
/// unresolved until assembly knows the final paragraph direction.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParagraphLayoutOverrides {
    pub(crate) bidi: Option<bool>,
    pub(crate) justification: Option<ParagraphJustification>,
}

impl ParagraphLayoutOverrides {
    pub(crate) fn apply(self, overrides: Self) -> Self {
        Self {
            bidi: overrides.bidi.or(self.bidi),
            justification: overrides.justification.or(self.justification),
        }
    }
}

/// Sparse modern logical twip indents from direct paragraph formatting.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParagraphIndentOverrides {
    pub(crate) logical_left_twips: Option<i16>,
    pub(crate) logical_right_twips: Option<i16>,
    pub(crate) nest_twips: Option<i16>,
    pub(crate) first_line_twips: Option<i16>,
}

impl ParagraphIndentOverrides {
    pub(crate) fn apply(self, overrides: Self) -> Self {
        Self {
            logical_left_twips: overrides.logical_left_twips.or(self.logical_left_twips),
            logical_right_twips: overrides.logical_right_twips.or(self.logical_right_twips),
            nest_twips: overrides.nest_twips.or(self.nest_twips),
            first_line_twips: overrides.first_line_twips.or(self.first_line_twips),
        }
    }
}

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
    /// Sparse direct paragraph direction and justification modifiers.
    layout: ParagraphLayoutOverrides,
    /// Sparse direct modern logical paragraph indents.
    indent: ParagraphIndentOverrides,
    /// Sparse direct paragraph pagination modifiers.
    pagination: ParagraphPaginationOverrides,
    /// Row repeats as a table header (`sprmTTableHeader`).
    table_header: bool,
    /// Resolved `sprmTFCantSplit` / `sprmTFCantSplit90` row property.
    table_cant_split: bool,
    /// Resolved direct table direction from `sprmTFBiDi` / `sprmTFBiDi90`.
    table_bidi_visual: bool,
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
    layout: ParagraphLayoutOverrides,
    indent: ParagraphIndentOverrides,
    pagination: ParagraphPaginationOverrides,
    table_header: bool,
    table_cant_split_90: Option<bool>,
    table_cant_split: Option<bool>,
    table_bidi: Option<bool>,
    table_bidi_90: Option<bool>,
}

impl Pap {
    fn resolved_cant_split(self) -> bool {
        self.table_cant_split
            .or(self.table_cant_split_90)
            .unwrap_or(false)
    }

    fn resolved_table_bidi_visual(self) -> bool {
        self.table_bidi.unwrap_or(false) || self.table_bidi_90.unwrap_or(false)
    }
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

    /// Style identity and outline level of the paragraph at `fc`.
    pub(crate) fn style_at(&self, fc: u32) -> (u16, Option<u8>) {
        self.entry_at(fc)
            .map(|e| (e.istd, e.outlvl))
            .unwrap_or((0, None))
    }

    /// Sparse direct paragraph direction and justification modifiers at `fc`.
    pub(crate) fn paragraph_layout_overrides_at(&self, fc: u32) -> ParagraphLayoutOverrides {
        self.entry_at(fc).map(|e| e.layout).unwrap_or_default()
    }

    /// Sparse direct modern logical paragraph indents at `fc`.
    pub(crate) fn paragraph_indent_overrides_at(&self, fc: u32) -> ParagraphIndentOverrides {
        self.entry_at(fc).map(|e| e.indent).unwrap_or_default()
    }

    /// Direct paragraph pagination controls at `fc`, with MS-DOC defaults.
    #[cfg(test)]
    pub(crate) fn paragraph_pagination_at(&self, fc: u32) -> ParagraphPagination {
        ParagraphPagination::default().apply(self.paragraph_pagination_overrides_at(fc))
    }

    /// Sparse direct paragraph pagination controls at `fc`.
    pub(crate) fn paragraph_pagination_overrides_at(
        &self,
        fc: u32,
    ) -> ParagraphPaginationOverrides {
        self.entry_at(fc).map(|e| e.pagination).unwrap_or_default()
    }

    /// The `sprmTDefTable` row definition for the row ending at `fc` (TTP), if any.
    pub(crate) fn table_def_at(&self, fc: u32) -> Option<&TableDef> {
        self.entry_at(fc).and_then(|e| e.table_def.as_ref())
    }

    /// Whether the row ending at `fc` repeats as a table header.
    pub(crate) fn table_header_at(&self, fc: u32) -> bool {
        self.entry_at(fc).map(|e| e.table_header).unwrap_or(false)
    }

    /// Whether the row ending at `fc` must not split across a page boundary.
    pub(crate) fn table_cant_split_at(&self, fc: u32) -> bool {
        self.entry_at(fc)
            .map(|e| e.table_cant_split)
            .unwrap_or(false)
    }

    /// Whether the row ending at `fc` uses visual right-to-left table ordering.
    pub(crate) fn table_bidi_visual_at(&self, fc: u32) -> bool {
        self.entry_at(fc)
            .map(|e| e.table_bidi_visual)
            .unwrap_or(false)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn from_test_entries(entries: &[(u32, bool, bool, bool)]) -> Self {
        Self {
            entries: entries
                .iter()
                .map(|&(fc_lim, in_table, ttp, table_cant_split)| PapEntry {
                    fc_lim,
                    in_table,
                    ttp,
                    table_cant_split,
                    ..PapEntry::default()
                })
                .collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_entries_with_pagination(
        entries: &[(u32, bool, bool, bool, ParagraphPagination)],
    ) -> Self {
        Self {
            entries: entries
                .iter()
                .map(
                    |&(fc_lim, in_table, ttp, table_cant_split, pagination)| PapEntry {
                        fc_lim,
                        in_table,
                        ttp,
                        table_cant_split,
                        pagination: pagination.into(),
                        ..PapEntry::default()
                    },
                )
                .collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_entries_with_style_pagination(
        entries: &[(u32, bool, bool, bool, u16, ParagraphPaginationOverrides)],
    ) -> Self {
        Self {
            entries: entries
                .iter()
                .map(
                    |&(fc_lim, in_table, ttp, table_cant_split, istd, pagination)| PapEntry {
                        fc_lim,
                        in_table,
                        ttp,
                        table_cant_split,
                        istd,
                        pagination,
                        ..PapEntry::default()
                    },
                )
                .collect(),
        }
    }

    #[cfg(test)]
    pub(crate) fn from_test_entries_with_table_bidi(
        entries: &[(u32, bool, bool, bool, bool)],
    ) -> Self {
        Self {
            entries: entries
                .iter()
                .map(
                    |&(fc_lim, in_table, ttp, table_cant_split, table_bidi_visual)| PapEntry {
                        fc_lim,
                        in_table,
                        ttp,
                        table_cant_split,
                        table_bidi_visual,
                        ..PapEntry::default()
                    },
                )
                .collect(),
        }
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
    // Both the page count and the accumulated entries are bounded: a crafted .doc can make
    // `n` huge and point every page number at the same valid FKP page, amplifying a small
    // table into billions of entries (a memory/CPU DoS). Cap iterations and break once the
    // cumulative entry budget is hit (mirrors the OPC/xmltree/ffn caps elsewhere).
    let cap = max_fkp_entries();
    let n = ((plc.len().saturating_sub(4)) / 8).min(cap);
    let pn_base = 4 * (n + 1);
    for i in 0..n {
        if entries.len() >= cap {
            break;
        }
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
            layout: pap.layout,
            indent: pap.indent,
            pagination: pap.pagination,
            table_header: pap.table_header,
            table_cant_split: pap.resolved_cant_split(),
            table_bidi_visual: pap.resolved_table_bidi_visual(),
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
/// outline level, justification, and direct pagination. Stops on an unsizeable
/// or truncated sprm.
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
        let Some(operand_end) = op.checked_add(len) else {
            break;
        };
        if gp.get(op..operand_end).is_none() {
            break;
        }
        if apply_pagination_sprm(&mut pap.pagination, sprm, &gp[op..operand_end]) {
            pos = operand_end;
            continue;
        }
        if apply_layout_sprm(&mut pap.layout, sprm, &gp[op..operand_end]) {
            pos = operand_end;
            continue;
        }
        if apply_indent_sprm(&mut pap.indent, sprm, &gp[op..operand_end]) {
            pos = operand_end;
            continue;
        }
        match sprm {
            SPRM_P_ISTD => {
                if let Some(new_istd) = u16le(gp, op) {
                    apply_paragraph_style_to_modeled_properties(&mut pap, new_istd);
                }
            }
            SPRM_P_ISTD_PERMUTE => {
                if let Some(new_istd) = permuted_istd(&gp[op..operand_end], pap.istd) {
                    apply_paragraph_style_to_modeled_properties(&mut pap, new_istd);
                }
            }
            SPRM_P_FIN_TABLE => pap.in_table = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_P_FTTP => pap.ttp = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_P_OUT_LVL => pap.outlvl = Some(gp.get(op).copied().unwrap_or(9)),
            SPRM_T_FCANT_SPLIT_90 => {
                pap.table_cant_split_90 = Some(gp.get(op).copied().unwrap_or(0) != 0);
            }
            SPRM_T_TABLE_HEADER => pap.table_header = gp.get(op).copied().unwrap_or(0) != 0,
            SPRM_T_FCANT_SPLIT => {
                pap.table_cant_split = Some(gp.get(op).copied().unwrap_or(0) != 0);
            }
            SPRM_T_F_BIDI => {
                if let Some(value) = strict_bool16(&gp[op..operand_end]) {
                    pap.table_bidi = Some(value);
                }
            }
            SPRM_T_F_BIDI_90 => {
                if let Some(value) = strict_bool16(&gp[op..operand_end]) {
                    pap.table_bidi_90 = Some(value);
                }
            }
            SPRM_P_ILVL => pap.ilvl = gp.get(op).copied().unwrap_or(0),
            SPRM_P_ILFO => pap.ilfo = u16le(gp, op).unwrap_or(0),
            SPRM_T_DEF_TABLE => {
                if let Some(operand) = gp.get(op..op + len) {
                    table_def = TableDef::parse(operand);
                }
            }
            _ => {}
        }
        pos = operand_end;
    }
    (pap, table_def)
}

fn apply_paragraph_style_to_modeled_properties(pap: &mut Pap, istd: u16) {
    pap.istd = istd;
    pap.ilfo = 0;
    pap.ilvl = 0;
    pap.outlvl = (1..=9).contains(&istd).then_some((istd - 1) as u8);
    pap.layout = ParagraphLayoutOverrides::default();
    pap.indent = ParagraphIndentOverrides::default();
    pap.pagination = ParagraphPaginationOverrides::default();
    // Table membership and row properties are intentionally preserved by a
    // paragraph-style change. Style-derived layout and pagination are resolved
    // during assembly; style-derived list values remain outside this scanner.
}

fn permuted_istd(operand: &[u8], current_istd: u16) -> Option<u16> {
    let declared_len = usize::from(*operand.first()?);
    if operand.len() != declared_len.checked_add(1)? {
        return None;
    }

    // SPPOperand = cb, fLong, istdFirst, istdLast, rgIstdPermute.
    let first = u16le(operand, 2)?;
    let last = u16le(operand, 4)?;
    let count = usize::from(last.checked_sub(first)?).checked_add(1)?;
    let expected_len = 6usize.checked_add(count.checked_mul(2)?)?;
    if operand.len() != expected_len || !(first..=last).contains(&current_istd) {
        return None;
    }

    let index = usize::from(current_istd - first);
    u16le(operand, 6 + index * 2)
}

fn apply_pagination_sprm(
    pagination: &mut ParagraphPaginationOverrides,
    sprm: u16,
    operand: &[u8],
) -> bool {
    let Some(value) = operand.first().map(|value| *value != 0) else {
        return false;
    };
    match sprm {
        SPRM_P_FKEEP => pagination.keep_lines = Some(value),
        SPRM_P_FKEEP_FOLLOW => pagination.keep_next = Some(value),
        SPRM_P_FPAGE_BREAK_BEFORE => pagination.page_break_before = Some(value),
        SPRM_P_FWIDOW_CONTROL => pagination.widow_control = Some(value),
        _ => return false,
    }
    true
}

fn apply_layout_sprm(layout: &mut ParagraphLayoutOverrides, sprm: u16, operand: &[u8]) -> bool {
    let Some(&value) = operand.first() else {
        return false;
    };
    match sprm {
        SPRM_P_F_BIDI => {
            if value <= 1 {
                layout.bidi = Some(value == 1);
            }
        }
        SPRM_P_JC_80 => {
            if let Some(justification) = physical_justification(value) {
                layout.justification = Some(justification);
            }
        }
        SPRM_P_JC => {
            if let Some(justification) = logical_justification(value) {
                layout.justification = Some(justification);
            }
        }
        _ => return false,
    }
    true
}

fn apply_indent_sprm(indent: &mut ParagraphIndentOverrides, sprm: u16, operand: &[u8]) -> bool {
    let target = match sprm {
        SPRM_P_DXA_RIGHT => &mut indent.logical_right_twips,
        SPRM_P_DXA_LEFT => &mut indent.logical_left_twips,
        SPRM_P_NEST => &mut indent.nest_twips,
        SPRM_P_DXA_LEFT_1 => &mut indent.first_line_twips,
        _ => return false,
    };
    if let Some(value) = strict_xas(operand) {
        *target = Some(value);
    }
    true
}

fn apply_style_indent_sprm(
    indent: &mut ParagraphIndentOverrides,
    sprm: u16,
    operand: &[u8],
) -> bool {
    if sprm == SPRM_P_NEST {
        return false;
    }
    apply_indent_sprm(indent, sprm, operand)
}

fn physical_justification(value: u8) -> Option<ParagraphJustification> {
    match value {
        0 => Some(ParagraphJustification::PhysicalLeft),
        1 => Some(ParagraphJustification::Center),
        2 => Some(ParagraphJustification::PhysicalRight),
        3..=5 => Some(ParagraphJustification::Justify),
        _ => None,
    }
}

fn logical_justification(value: u8) -> Option<ParagraphJustification> {
    match value {
        0 => Some(ParagraphJustification::LogicalStart),
        1 => Some(ParagraphJustification::Center),
        2 => Some(ParagraphJustification::LogicalEnd),
        3..=5 | 7..=9 => Some(ParagraphJustification::Justify),
        6 => Some(ParagraphJustification::UnsupportedIndented),
        _ => None,
    }
}

fn strict_bool16(operand: &[u8]) -> Option<bool> {
    match u16le(operand, 0)? {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

fn strict_xas(operand: &[u8]) -> Option<i16> {
    let value = u16le(operand, 0)? as i16;
    (-31_680..=31_680).contains(&value).then_some(value)
}

/// Strictly scan a style `grpprlPapx` for the layout, indent, and pagination
/// subsets modeled by the legacy reader. A malformed modifier invalidates the
/// local style payload instead of applying a partial prefix.
pub(crate) fn scan_paragraph_style_overrides(
    gp: &[u8],
) -> Option<(
    ParagraphLayoutOverrides,
    ParagraphIndentOverrides,
    ParagraphPaginationOverrides,
)> {
    let mut layout = ParagraphLayoutOverrides::default();
    let mut indent = ParagraphIndentOverrides::default();
    let mut pagination = ParagraphPaginationOverrides::default();
    let mut pos = 0;
    while pos < gp.len() {
        let sprm = u16le(gp, pos)?;
        let op = pos.checked_add(2)?;
        let len = operand_len(sprm, gp, op)?;
        let operand_end = op.checked_add(len)?;
        let operand = gp.get(op..operand_end)?;
        apply_layout_sprm(&mut layout, sprm, operand);
        apply_style_indent_sprm(&mut indent, sprm, operand);
        apply_pagination_sprm(&mut pagination, sprm, operand);
        pos = operand_end;
    }
    Some((layout, indent, pagination))
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
    fn resolves_modern_and_compatibility_table_row_no_split_properties() {
        let (default, _) = scan_grpprl(&[], 0);
        let (compatibility, _) = scan_grpprl(&[0x03, 0x34, 0x01], 0);
        let (modern, _) = scan_grpprl(&[0x66, 0x34, 0x01], 0);

        assert!(!default.resolved_cant_split());
        assert!(compatibility.resolved_cant_split());
        assert!(modern.resolved_cant_split());

        // `sprmTFCantSplit` supersedes `sprmTFCantSplit90` regardless of the
        // physical order in the grpprl.
        for grpprl in [
            [0x03, 0x34, 0x01, 0x66, 0x34, 0x00],
            [0x66, 0x34, 0x00, 0x03, 0x34, 0x01],
        ] {
            let (properties, _) = scan_grpprl(&grpprl, 0);
            assert!(!properties.resolved_cant_split());
        }
        for grpprl in [
            [0x03, 0x34, 0x00, 0x66, 0x34, 0x01],
            [0x66, 0x34, 0x01, 0x03, 0x34, 0x00],
        ] {
            let (properties, _) = scan_grpprl(&grpprl, 0);
            assert!(properties.resolved_cant_split());
        }
    }

    #[test]
    fn resolves_direct_table_bidi_properties_strictly() {
        let (default, _) = scan_grpprl(&[], 0);
        let (modern, _) = scan_grpprl(&[0x0B, 0x56, 0x01, 0x00], 0);
        let (compatibility, _) = scan_grpprl(&[0x64, 0x56, 0x01, 0x00], 0);
        assert!(!default.resolved_table_bidi_visual());
        assert!(modern.resolved_table_bidi_visual());
        assert!(compatibility.resolved_table_bidi_visual());

        for grpprl in [
            [0x0B, 0x56, 0x01, 0x00, 0x64, 0x56, 0x00, 0x00],
            [0x64, 0x56, 0x00, 0x00, 0x0B, 0x56, 0x01, 0x00],
            [0x0B, 0x56, 0x00, 0x00, 0x64, 0x56, 0x01, 0x00],
            [0x64, 0x56, 0x01, 0x00, 0x0B, 0x56, 0x00, 0x00],
        ] {
            let (properties, _) = scan_grpprl(&grpprl, 0);
            assert!(properties.resolved_table_bidi_visual());
        }

        let (modern_last_off, _) =
            scan_grpprl(&[0x0B, 0x56, 0x01, 0x00, 0x0B, 0x56, 0x00, 0x00], 0);
        let (compatibility_last_on, _) =
            scan_grpprl(&[0x64, 0x56, 0x00, 0x00, 0x64, 0x56, 0x01, 0x00], 0);
        assert!(!modern_last_off.resolved_table_bidi_visual());
        assert!(compatibility_last_on.resolved_table_bidi_visual());

        let (invalid_after_valid, _) =
            scan_grpprl(&[0x0B, 0x56, 0x01, 0x00, 0x0B, 0x56, 0x02, 0x00], 0);
        let (only_invalid, _) = scan_grpprl(&[0x64, 0x56, 0xFF, 0xFF], 0);
        let (truncated_after_valid, _) =
            scan_grpprl(&[0x0B, 0x56, 0x01, 0x00, 0x64, 0x56, 0x01], 0);
        assert!(invalid_after_valid.resolved_table_bidi_visual());
        assert!(!only_invalid.resolved_table_bidi_visual());
        assert!(truncated_after_valid.resolved_table_bidi_visual());
    }

    #[test]
    fn resolves_direct_paragraph_pagination_properties_and_defaults() {
        let (default, _) = scan_grpprl(&[], 0);
        assert_eq!(default.pagination, ParagraphPaginationOverrides::default());

        let (enabled, _) = scan_grpprl(
            &[
                0x05, 0x24, 0x01, // sprmPFKeep
                0x06, 0x24, 0x01, // sprmPFKeepFollow
                0x07, 0x24, 0x01, // sprmPFPageBreakBefore
                0x31, 0x24, 0x00, // sprmPFWidowControl
            ],
            0,
        );
        assert_eq!(
            enabled.pagination,
            ParagraphPaginationOverrides {
                keep_next: Some(true),
                keep_lines: Some(true),
                page_break_before: Some(true),
                widow_control: Some(false),
            }
        );

        let (last_value_wins, _) = scan_grpprl(
            &[
                0x05, 0x24, 0x01, 0x05, 0x24, 0x00, 0x06, 0x24, 0x00, 0x06, 0x24, 0x01, 0x07, 0x24,
                0x01, 0x07, 0x24, 0x00, 0x31, 0x24, 0x00, 0x31, 0x24, 0x01,
            ],
            0,
        );
        assert_eq!(
            last_value_wins.pagination,
            ParagraphPaginationOverrides {
                keep_next: Some(true),
                keep_lines: Some(false),
                page_break_before: Some(false),
                widow_control: Some(true),
            }
        );

        let (truncated, _) = scan_grpprl(
            &[
                0x05, 0x24, 0x01, // valid keep-lines survives
                0x31, 0x24, // truncated widow-control operand is ignored
            ],
            0,
        );
        assert_eq!(
            truncated.pagination,
            ParagraphPaginationOverrides {
                keep_lines: Some(true),
                ..ParagraphPaginationOverrides::default()
            }
        );

        assert!(scan_paragraph_style_overrides(&[0x05, 0x24, 0x01, 0x31, 0x24]).is_none());
        assert_eq!(
            scan_paragraph_style_overrides(&[
                0x5E, 0x84, 0x34, 0x12, // unrelated two-byte indent operand
                0x0D, 0xC6, 0x02, 0x00, 0x00, // unrelated variable tab operand
                0x07, 0x24, 0x01,
            ]),
            Some((
                ParagraphLayoutOverrides::default(),
                ParagraphIndentOverrides {
                    logical_left_twips: Some(0x1234),
                    ..ParagraphIndentOverrides::default()
                },
                ParagraphPaginationOverrides {
                    page_break_before: Some(true),
                    ..ParagraphPaginationOverrides::default()
                },
            ))
        );
    }

    #[test]
    fn paragraph_style_indents_are_strict_source_ordered_and_exclude_nest() {
        assert_eq!(
            scan_paragraph_style_overrides(&[
                0x5E, 0x84, 0x90, 0x01, // logical left = 400
                0x5E, 0x84, 0x00, 0x7D, // invalid XAS = 32000
                0x5E, 0x84, 0xF4, 0x01, // logical left = 500
                0x5D, 0x84, 0x40, 0x84, // logical right = -31680
                0x5D, 0x84, 0x3F, 0x84, // invalid XAS = -31681
                0x60, 0x84, 0xC0, 0x7B, // first line = 31680
                0x5F, 0x46, 0x78, 0x00, // prohibited style nest is ignored
            ]),
            Some((
                ParagraphLayoutOverrides::default(),
                ParagraphIndentOverrides {
                    logical_left_twips: Some(500),
                    logical_right_twips: Some(-31_680),
                    nest_twips: None,
                    first_line_twips: Some(31_680),
                },
                ParagraphPaginationOverrides::default(),
            ))
        );
    }

    #[test]
    fn paragraph_pagination_lookup_uses_fc_ranges_and_safe_defaults() {
        let table = PapxTable {
            entries: vec![
                PapEntry {
                    fc_lim: 100,
                    pagination: ParagraphPaginationOverrides {
                        keep_lines: Some(true),
                        ..ParagraphPaginationOverrides::default()
                    },
                    ..PapEntry::default()
                },
                PapEntry {
                    fc_lim: 200,
                    pagination: ParagraphPaginationOverrides {
                        keep_next: Some(true),
                        page_break_before: Some(true),
                        widow_control: Some(false),
                        ..ParagraphPaginationOverrides::default()
                    },
                    ..PapEntry::default()
                },
            ],
        };

        assert!(table.paragraph_pagination_at(50).keep_lines);
        let second = table.paragraph_pagination_at(150);
        assert!(second.keep_next);
        assert!(second.page_break_before);
        assert!(!second.widow_control);
        assert_eq!(
            table.paragraph_pagination_at(999),
            ParagraphPagination::default()
        );
    }

    #[test]
    fn scans_list_props() {
        // sprmPIlvl (0x260A, 1-byte) = 2, then sprmPIlfo (0x460B, 2-byte) = 5.
        let (p, _) = scan_grpprl(&[0x0A, 0x26, 0x02, 0x0B, 0x46, 0x05, 0x00], 0);
        assert_eq!((p.ilfo, p.ilvl), (5, 2));
    }

    #[test]
    fn scans_style_outline_align() {
        // leading istd = 7; sprmPJc80(0x2403)=1 center; sprmPOutLvl(0x2640)=2.
        let (p, _) = scan_grpprl(&[0x03, 0x24, 0x01, 0x40, 0x26, 0x02], 7);
        assert_eq!(p.istd, 7);
        assert_eq!(p.layout.justification, Some(ParagraphJustification::Center));
        assert_eq!(p.outlvl, Some(2));
        // sprmPIstd (0x4600, 2-byte) overrides the leading istd.
        let (p2, _) = scan_grpprl(&[0x00, 0x46, 0x05, 0x00], 7);
        assert_eq!(p2.istd, 5);
    }

    #[test]
    fn direct_paragraph_bidi_is_strict_bool8_and_last_valid_wins() {
        let (default, _) = scan_grpprl(&[], 0);
        assert_eq!(default.layout.bidi, None);

        let (last_valid, _) = scan_grpprl(
            &[
                0x41, 0x24, 0x01, // on
                0x41, 0x24, 0x00, // off
                0x41, 0x24, 0x01, // on again
            ],
            0,
        );
        assert_eq!(last_valid.layout.bidi, Some(true));

        let (invalid, _) = scan_grpprl(
            &[
                0x41, 0x24, 0x01, // valid prefix
                0x41, 0x24, 0x02, // invalid Bool8 preserves it
                0x41, 0x24, 0xFF, // invalid Bool8 preserves it
            ],
            0,
        );
        assert_eq!(invalid.layout.bidi, Some(true));

        let (only_invalid, _) = scan_grpprl(&[0x41, 0x24, 0x02], 0);
        assert_eq!(only_invalid.layout.bidi, None);

        let (truncated, _) = scan_grpprl(&[0x41, 0x24, 0x01, 0x41, 0x24], 0);
        assert_eq!(truncated.layout.bidi, Some(true));
    }

    #[test]
    fn direct_paragraph_justification_preserves_bounded_classes() {
        for (sprm, value, expected) in [
            (SPRM_P_JC_80, 0, ParagraphJustification::PhysicalLeft),
            (SPRM_P_JC_80, 1, ParagraphJustification::Center),
            (SPRM_P_JC_80, 2, ParagraphJustification::PhysicalRight),
            (SPRM_P_JC_80, 3, ParagraphJustification::Justify),
            (SPRM_P_JC_80, 4, ParagraphJustification::Justify),
            (SPRM_P_JC_80, 5, ParagraphJustification::Justify),
            (SPRM_P_JC, 0, ParagraphJustification::LogicalStart),
            (SPRM_P_JC, 1, ParagraphJustification::Center),
            (SPRM_P_JC, 2, ParagraphJustification::LogicalEnd),
            (SPRM_P_JC, 3, ParagraphJustification::Justify),
            (SPRM_P_JC, 4, ParagraphJustification::Justify),
            (SPRM_P_JC, 5, ParagraphJustification::Justify),
            (SPRM_P_JC, 6, ParagraphJustification::UnsupportedIndented),
            (SPRM_P_JC, 7, ParagraphJustification::Justify),
            (SPRM_P_JC, 8, ParagraphJustification::Justify),
            (SPRM_P_JC, 9, ParagraphJustification::Justify),
        ] {
            let mut grpprl = Vec::from(sprm.to_le_bytes());
            grpprl.push(value);
            let (pap, _) = scan_grpprl(&grpprl, 0);
            assert_eq!(pap.layout.justification, Some(expected));
        }
    }

    #[test]
    fn direct_paragraph_justification_uses_source_order_and_safe_fallbacks() {
        let (logical_last, _) = scan_grpprl(
            &[
                0x03, 0x24, 0x00, // physical left
                0x61, 0x24, 0x02, // logical end
            ],
            0,
        );
        assert_eq!(
            logical_last.layout.justification,
            Some(ParagraphJustification::LogicalEnd)
        );

        let (physical_last, _) = scan_grpprl(
            &[
                0x61, 0x24, 0x02, // logical end
                0x03, 0x24, 0x00, // physical left
            ],
            0,
        );
        assert_eq!(
            physical_last.layout.justification,
            Some(ParagraphJustification::PhysicalLeft)
        );

        let (invalid, _) = scan_grpprl(
            &[
                0x61, 0x24, 0x02, // valid logical end
                0x03, 0x24, 0x06, // invalid physical value
                0x61, 0x24, 0x0A, // invalid logical value
            ],
            0,
        );
        assert_eq!(
            invalid.layout.justification,
            Some(ParagraphJustification::LogicalEnd)
        );

        let (truncated, _) = scan_grpprl(&[0x03, 0x24, 0x02, 0x61, 0x24], 0);
        assert_eq!(
            truncated.layout.justification,
            Some(ParagraphJustification::PhysicalRight)
        );
    }

    #[test]
    fn direct_paragraph_indents_are_strict_source_ordered_and_prefix_safe() {
        let (default, _) = scan_grpprl(&[], 0);
        assert_eq!(default.indent, ParagraphIndentOverrides::default());

        let (values, _) = scan_grpprl(
            &[
                0x5D, 0x84, 0x60, 0x09, // logical right = 2400
                0x5E, 0x84, 0xD0, 0x07, // logical left = 2000
                0x5F, 0x46, 0x88, 0xFF, // nest = -120
                0x60, 0x84, 0x98, 0xFE, // first line = -360
            ],
            0,
        );
        assert_eq!(
            values.indent,
            ParagraphIndentOverrides {
                logical_left_twips: Some(2000),
                logical_right_twips: Some(2400),
                nest_twips: Some(-120),
                first_line_twips: Some(-360),
            }
        );

        let (last_valid, _) = scan_grpprl(
            &[
                0x5E, 0x84, 0x90, 0x01, // logical left = 400
                0x5E, 0x84, 0x00, 0x7D, // invalid XAS = 32000
                0x5E, 0x84, 0xF4, 0x01, // logical left = 500
                0x5D, 0x84, 0x40, 0x84, // valid lower XAS bound = -31680
                0x5D, 0x84, 0x3F, 0x84, // invalid XAS = -31681
                0x60, 0x84, 0xC0, 0x7B, // valid upper XAS bound = 31680
            ],
            0,
        );
        assert_eq!(last_valid.indent.logical_left_twips, Some(500));
        assert_eq!(last_valid.indent.logical_right_twips, Some(-31_680));
        assert_eq!(last_valid.indent.first_line_twips, Some(31_680));

        let (truncated, _) = scan_grpprl(
            &[
                0x5E, 0x84, 0xD0, 0x07, // valid logical-left prefix
                0x5D, 0x84, 0x20, // truncated logical-right operand
            ],
            0,
        );
        assert_eq!(truncated.indent.logical_left_twips, Some(2000));
        assert_eq!(truncated.indent.logical_right_twips, None);
    }

    #[test]
    fn paragraph_style_changes_discard_earlier_non_preserved_properties() {
        let (style_last, _) = scan_grpprl(
            &[
                0x41, 0x24, 0x01, // direct RTL on
                0x61, 0x24, 0x02, // direct logical end
                0x05, 0x24, 0x01, // direct keep lines
                0x06, 0x24, 0x01, // direct keep next
                0x07, 0x24, 0x01, // direct page break before
                0x31, 0x24, 0x00, // direct widow control off
                0x40, 0x26, 0x02, // direct outline level
                0x0A, 0x26, 0x03, // direct list level
                0x0B, 0x46, 0x05, 0x00, // direct list format override
                0x5E, 0x84, 0xD0, 0x07, // direct logical-left indent
                0x60, 0x84, 0x98, 0xFE, // direct hanging indent
                0x16, 0x24, 0x01, // paragraph is in a table
                0x17, 0x24, 0x01, // paragraph terminates a table row
                0x04, 0x34, 0x01, // table header row
                0x66, 0x34, 0x01, // table row cannot split
                0x00, 0x46, 0x05, 0x00, // sprmPIstd = 5
            ],
            2,
        );
        assert_eq!(style_last.istd, 5);
        assert_eq!(style_last.layout, ParagraphLayoutOverrides::default());
        assert_eq!(
            style_last.pagination,
            ParagraphPaginationOverrides::default()
        );
        assert_eq!(style_last.outlvl, Some(4));
        assert_eq!((style_last.ilfo, style_last.ilvl), (0, 0));
        assert_eq!(style_last.indent, ParagraphIndentOverrides::default());
        assert!(style_last.in_table);
        assert!(style_last.ttp);
        assert!(style_last.table_header);
        assert!(style_last.resolved_cant_split());

        let (direct_last, _) = scan_grpprl(
            &[
                0x00, 0x46, 0x05, 0x00, // sprmPIstd = 5
                0x41, 0x24, 0x01, // later direct RTL on
                0x61, 0x24, 0x02, // later direct logical end
                0x05, 0x24, 0x01, // later direct keep lines
                0x40, 0x26, 0x02, // later direct outline level
                0x0A, 0x26, 0x03, // later direct list level
                0x0B, 0x46, 0x05, 0x00, // later direct list format override
                0x5E, 0x84, 0xD0, 0x07, // later direct logical-left indent
                0x60, 0x84, 0x98, 0xFE, // later direct hanging indent
            ],
            2,
        );
        assert_eq!(direct_last.istd, 5);
        assert_eq!(
            direct_last.layout,
            ParagraphLayoutOverrides {
                bidi: Some(true),
                justification: Some(ParagraphJustification::LogicalEnd),
            }
        );
        assert_eq!(direct_last.pagination.keep_lines, Some(true));
        assert_eq!(direct_last.outlvl, Some(2));
        assert_eq!((direct_last.ilfo, direct_last.ilvl), (5, 3));
        assert_eq!(
            direct_last.indent,
            ParagraphIndentOverrides {
                logical_left_twips: Some(2000),
                first_line_twips: Some(-360),
                ..ParagraphIndentOverrides::default()
            }
        );
    }

    #[test]
    fn paragraph_style_permutation_resets_non_preserved_properties_when_affected() {
        // SPPOperand maps styles 4..=6 to 7, 8, and 9 respectively.
        let permutation = [
            0x01, 0xC6, // sprmPIstdPermute
            0x0B, // cb
            0x00, // fLong
            0x04, 0x00, // istdFirst
            0x06, 0x00, // istdLast
            0x07, 0x00, 0x08, 0x00, 0x09, 0x00, // rgIstdPermute
        ];

        let mut affected_grpprl = vec![
            0x41, 0x24, 0x01, // direct RTL on
            0x03, 0x24, 0x02, // direct physical right
            0x05, 0x24, 0x01, // direct keep lines
            0x40, 0x26, 0x02, // direct outline level
            0x0A, 0x26, 0x03, // direct list level
            0x0B, 0x46, 0x05, 0x00, // direct list format override
            0x16, 0x24, 0x01, // paragraph is in a table
            0x04, 0x34, 0x01, // table header row
        ];
        affected_grpprl.extend_from_slice(&permutation);
        let (affected, _) = scan_grpprl(&affected_grpprl, 5);
        assert_eq!(affected.istd, 8);
        assert_eq!(affected.layout, ParagraphLayoutOverrides::default());
        assert_eq!(affected.pagination, ParagraphPaginationOverrides::default());
        assert_eq!(affected.outlvl, Some(7));
        assert_eq!((affected.ilfo, affected.ilvl), (0, 0));
        assert!(affected.in_table);
        assert!(affected.table_header);

        let mut unaffected_grpprl = vec![
            0x41, 0x24, 0x01, // direct RTL on
            0x03, 0x24, 0x02, // direct physical right
            0x05, 0x24, 0x01, // direct keep lines
            0x40, 0x26, 0x02, // direct outline level
            0x0A, 0x26, 0x03, // direct list level
            0x0B, 0x46, 0x05, 0x00, // direct list format override
        ];
        unaffected_grpprl.extend_from_slice(&permutation);
        let (unaffected, _) = scan_grpprl(&unaffected_grpprl, 3);
        assert_eq!(unaffected.istd, 3);
        assert_eq!(
            unaffected.layout,
            ParagraphLayoutOverrides {
                bidi: Some(true),
                justification: Some(ParagraphJustification::PhysicalRight),
            }
        );
        assert_eq!(unaffected.pagination.keep_lines, Some(true));
        assert_eq!(unaffected.outlvl, Some(2));
        assert_eq!((unaffected.ilfo, unaffected.ilvl), (5, 3));
    }

    #[test]
    fn malformed_or_truncated_style_permutation_preserves_direct_layout() {
        let direct = [
            0x41, 0x24, 0x01, // direct RTL on
            0x03, 0x24, 0x02, // direct physical right
        ];
        let expected = ParagraphLayoutOverrides {
            bidi: Some(true),
            justification: Some(ParagraphJustification::PhysicalRight),
        };

        let mut reversed_bounds = direct.to_vec();
        reversed_bounds
            .extend_from_slice(&[0x01, 0xC6, 0x07, 0x00, 0x05, 0x00, 0x04, 0x00, 0x09, 0x00]);
        let (reversed, _) = scan_grpprl(&reversed_bounds, 5);
        assert_eq!(reversed.istd, 5);
        assert_eq!(reversed.layout, expected);

        let mut count_mismatch = direct.to_vec();
        count_mismatch
            .extend_from_slice(&[0x01, 0xC6, 0x07, 0x00, 0x05, 0x00, 0x06, 0x00, 0x09, 0x00]);
        let (mismatched, _) = scan_grpprl(&count_mismatch, 5);
        assert_eq!(mismatched.istd, 5);
        assert_eq!(mismatched.layout, expected);

        let mut truncated = direct.to_vec();
        truncated.extend_from_slice(&[0x01, 0xC6, 0x0B, 0x00, 0x04, 0x00, 0x06, 0x00]);
        let (truncated, _) = scan_grpprl(&truncated, 5);
        assert_eq!(truncated.istd, 5);
        assert_eq!(truncated.layout, expected);
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
            layout: ParagraphLayoutOverrides::default(),
            indent: ParagraphIndentOverrides::default(),
            pagination: ParagraphPaginationOverrides::default(),
            table_header: false,
            table_cant_split: false,
            table_bidi_visual: false,
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

    #[test]
    fn row_no_split_lookup_and_truncated_papx_default_safely() {
        let table =
            PapxTable::from_test_entries(&[(100, false, false, false), (200, true, true, true)]);
        assert!(!table.table_cant_split_at(50));
        assert!(table.table_cant_split_at(150));
        assert!(!table.table_cant_split_at(250));

        let mut word = vec![0u8; FKP_SIZE];
        word[4..8].copy_from_slice(&100u32.to_le_bytes());
        word[8] = u8::MAX; // PAPX offset 510: the declared payload is truncated.
        word[FKP_SIZE - 1] = 1;
        let mut plc = Vec::new();
        plc.extend_from_slice(&0u32.to_le_bytes());
        plc.extend_from_slice(&100u32.to_le_bytes());
        plc.extend_from_slice(&0u32.to_le_bytes());

        let parsed = parse(&word, &plc, 0, plc.len());
        assert!(!parsed.is_empty());
        assert_eq!(parsed.at(50), (false, false));
        assert!(!parsed.table_cant_split_at(50));
    }

    #[test]
    fn bin_table_entry_count_is_capped() {
        // One valid FKP page (page 0): crun = 29 default entries.
        let mut word = vec![0u8; FKP_SIZE];
        word[FKP_SIZE - 1] = 29;
        // A PlcBtePapx of 10 pages whose page numbers are all 0 (every entry points at the
        // same valid page) — the repeated-page amplification the fix bounds.
        let n_pages = 10usize;
        let plc = vec![0u8; 4 * (n_pages + 1) + 4 * n_pages];
        set_test_max_fkp(64);
        let t = parse(&word, &plc, 0, plc.len());
        set_test_max_fkp(MAX_FKP_ENTRIES);
        // Uncapped this would be 10 * 29 = 290; the cap stops it well before that, without
        // panicking or hanging.
        assert!(
            t.entries.len() >= 64 && t.entries.len() < n_pages * 29,
            "entry cap did not bound the repeated-page bin table: {}",
            t.entries.len()
        );
    }
}
