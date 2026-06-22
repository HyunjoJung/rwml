//! Table reconstruction: parse `sprmTDefTable` row definitions and fold streamed
//! rows/cells into a merge-aware [`model::Table`] (colspan from `fMerged`,
//! rowspan from `fVertRestart`/`fVertMerge`, matched by column).
//!
//! Reference: [MS-DOC] 2.4.3 (cell boundaries), 2.9.349 (TDefTableOperand),
//! 2.9.330 (TC80).

use crate::model::{Block, Cell, Row, Table};

const F_MERGED: u16 = 0x0002; // cell folds into the one to its left
const F_VERT_MERGE: u16 = 0x0020; // cell continues a vertical merge from above
const F_VERT_RESTART: u16 = 0x0040; // cell starts a vertical-merge group

/// A parsed row definition (the `sprmTDefTable` operand carried on the TTP).
#[derive(Debug, Clone, Default)]
pub(crate) struct TableDef {
    /// Cell-boundary x-positions in twips (`itcMac + 1` entries).
    pub rgdxa: Vec<i16>,
    /// Per-cell `TC80.tcgrf` (merge flags); `itcMac` entries.
    pub tcgrf: Vec<u16>,
}

impl TableDef {
    /// Parse a `TDefTableOperand`: `cb:u16, itcMac:u8, rgdxaCenter[itcMac+1]:i16,
    /// rgTc[itcMac]:TC80(20B)`. The `tcgrf` is the leading `u16` of each TC80.
    pub(crate) fn parse(operand: &[u8]) -> Option<TableDef> {
        let itc_mac = *operand.get(2)? as usize;
        if itc_mac == 0 || itc_mac > 63 {
            return None;
        }
        let mut rgdxa = Vec::with_capacity(itc_mac + 1);
        for k in 0..=itc_mac {
            let o = 3 + 2 * k;
            let b = operand.get(o..o + 2)?;
            rgdxa.push(i16::from_le_bytes([b[0], b[1]]));
        }
        let tc_base = 3 + 2 * (itc_mac + 1);
        let mut tcgrf = Vec::with_capacity(itc_mac);
        for k in 0..itc_mac {
            let o = tc_base + k * 20;
            let g = operand
                .get(o..o + 2)
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .unwrap_or(0);
            tcgrf.push(g);
        }
        Some(TableDef { rgdxa, tcgrf })
    }
}

/// One streamed row: its cells' block content + the row definition + header flag.
pub(crate) struct RowBuild {
    pub cells: Vec<Vec<Block>>,
    pub def: Option<TableDef>,
    pub header: bool,
}

/// An output cell during merge resolution.
struct Out {
    blocks: Vec<Block>,
    /// Starting column over the table's global boundary set.
    col: usize,
    colspan: u16,
    rowspan: u16,
    tcgrf: u16,
    dropped: bool,
}

/// Fold streamed rows into a merge-aware table.
///
/// Column geometry uses the **global set of cell-boundary x-positions**
/// (`rgdxaCenter`) across all rows, so a row with fewer cells than the table has
/// columns (e.g. a single wide header cell) gets the right colspan. Within a row,
/// `fMerged` cells fold left; `rgdxa` then yields the final span.
pub(crate) fn build(rows: Vec<RowBuild>) -> Table {
    let header_rows = rows.iter().take_while(|r| r.header).count();

    // Global sorted set of distinct boundary positions across the whole table.
    let mut bounds: Vec<i16> = rows
        .iter()
        .filter_map(|r| r.def.as_ref())
        .flat_map(|d| d.rgdxa.iter().copied())
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    let col_of = |x: i16| bounds.binary_search(&x).unwrap_or_else(|e| e);

    // Phase A: per-row cells, folding `fMerged` left and computing colspan/col
    // from the global boundary set (or sequential columns when no row definition).
    let mut grid: Vec<Vec<Out>> = Vec::with_capacity(rows.len());
    for rb in rows {
        let mut out: Vec<Out> = Vec::new();
        match rb.def.filter(|d| d.rgdxa.len() >= 2) {
            Some(def) => {
                let ncell = def.rgdxa.len() - 1;
                let mut cells = rb.cells.into_iter();
                for k in 0..ncell {
                    let blocks = cells.next().unwrap_or_default();
                    let g = def.tcgrf.get(k).copied().unwrap_or(0);
                    let (left, right) = (def.rgdxa[k], def.rgdxa[k + 1]);
                    if g & F_MERGED != 0 && !out.is_empty() {
                        let last = out.last_mut().expect("non-empty");
                        last.colspan = (col_of(right).saturating_sub(last.col)).max(1) as u16;
                        last.blocks.extend(blocks);
                    } else {
                        let col = col_of(left);
                        let colspan = (col_of(right).saturating_sub(col)).max(1) as u16;
                        out.push(Out {
                            blocks,
                            col,
                            colspan,
                            rowspan: 1,
                            tcgrf: g,
                            dropped: false,
                        });
                    }
                }
                // Extra streamed cells beyond the definition fold into the last.
                for blocks in cells {
                    if let Some(last) = out.last_mut() {
                        last.blocks.extend(blocks);
                    }
                }
            }
            None => {
                for (k, blocks) in rb.cells.into_iter().enumerate() {
                    out.push(Out {
                        blocks,
                        col: k,
                        colspan: 1,
                        rowspan: 1,
                        tcgrf: 0,
                        dropped: false,
                    });
                }
            }
        }
        grid.push(out);
    }

    // Phase B: vertical merge (fVertRestart/fVertMerge), matched by column index.
    // open[col] = (row, idx) of the cell currently owning the vertical span.
    let mut open: std::collections::HashMap<usize, (usize, usize)> =
        std::collections::HashMap::new();
    for r in 0..grid.len() {
        for o in 0..grid[r].len() {
            let g = grid[r][o].tcgrf;
            let col = grid[r][o].col;
            let vert_merge = g & F_VERT_MERGE != 0;
            let vert_restart = g & F_VERT_RESTART != 0;
            if vert_restart {
                open.insert(col, (r, o));
            } else if vert_merge {
                if let Some(&(rr, oo)) = open.get(&col) {
                    grid[rr][oo].rowspan += 1;
                    grid[r][o].dropped = true;
                }
            } else {
                open.remove(&col);
            }
        }
    }

    // Emit, skipping merged-away cells.
    let mut model_rows = Vec::with_capacity(grid.len());
    for (r, row) in grid.into_iter().enumerate() {
        let is_header = r < header_rows;
        let cells: Vec<Cell> = row
            .into_iter()
            .filter(|o| !o.dropped)
            .map(|o| Cell {
                blocks: o.blocks,
                col_span: o.colspan,
                row_span: o.rowspan,
                is_header,
                ..Default::default()
            })
            .collect();
        model_rows.push(Row { cells });
    }
    Table {
        rows: model_rows,
        header_rows,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, ParaProps, Paragraph, Run};

    fn cell(text: &str) -> Vec<Block> {
        vec![Block::Paragraph(Paragraph {
            props: ParaProps::default(),
            runs: vec![Run {
                text: text.to_string(),
                ..Default::default()
            }],
        })]
    }

    #[test]
    fn parse_tdef_two_cells() {
        // cb, itcMac=2, rgdxa[3] = {0, 100, 200}, rgTc[2] TC80 (only tcgrf set).
        let mut op = vec![0u8, 0u8, 2u8];
        for v in [0i16, 100, 200] {
            op.extend_from_slice(&v.to_le_bytes());
        }
        // TC80 #0: tcgrf=0, then 18 padding; TC80 #1: tcgrf=fMerged.
        op.extend_from_slice(&0u16.to_le_bytes());
        op.extend_from_slice(&[0u8; 18]);
        op.extend_from_slice(&F_MERGED.to_le_bytes());
        op.extend_from_slice(&[0u8; 18]);
        let def = TableDef::parse(&op).unwrap();
        assert_eq!(def.rgdxa, vec![0, 100, 200]);
        assert_eq!(def.tcgrf, vec![0, F_MERGED]);
    }

    #[test]
    fn horizontal_merge_colspan() {
        // Row: cell A, cell B(fMerged → folds into A) → one cell, colspan 2.
        let def = TableDef {
            rgdxa: vec![0, 100, 200],
            tcgrf: vec![0, F_MERGED],
        };
        let t = build(vec![RowBuild {
            cells: vec![cell("A"), cell("B")],
            def: Some(def),
            header: false,
        }]);
        assert_eq!(t.rows[0].cells.len(), 1);
        assert_eq!(t.rows[0].cells[0].col_span, 2);
    }

    #[test]
    fn vertical_merge_rowspan() {
        // Two rows, column 0: top fVertRestart, bottom fVertMerge → rowspan 2,
        // the continuation cell dropped.
        let top = RowBuild {
            cells: vec![cell("X"), cell("a")],
            def: Some(TableDef {
                rgdxa: vec![0, 100, 200],
                tcgrf: vec![F_VERT_RESTART, 0],
            }),
            header: false,
        };
        let bot = RowBuild {
            cells: vec![cell(""), cell("b")],
            def: Some(TableDef {
                rgdxa: vec![0, 100, 200],
                tcgrf: vec![F_VERT_MERGE, 0],
            }),
            header: false,
        };
        let t = build(vec![top, bot]);
        assert_eq!(t.rows[0].cells[0].row_span, 2);
        assert_eq!(t.rows[1].cells.len(), 1); // continuation dropped
    }
}
