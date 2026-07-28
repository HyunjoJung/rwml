//! Table reconstruction: parse `sprmTDefTable` row definitions and fold streamed
//! rows/cells into a merge-aware [`model::Table`] (colspan from `fMerged`,
//! rowspan from `fVertRestart`/`fVertMerge`, matched by column).
//!
//! Reference: [MS-DOC] 2.4.3 (cell boundaries), 2.9.349 (TDefTableOperand),
//! 2.9.330 (TC80).

use crate::model::{Block, Cell, PaginationHint, Row, Table, TableCellPaginationHints};

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

/// One streamed cell, keeping block content and its source pagination metadata
/// together through merge resolution.
#[derive(Default)]
pub(crate) struct CellBuild {
    pub blocks: Vec<Block>,
    pub pagination: Vec<Option<PaginationHint>>,
}

/// One streamed row: its cells + the row definition + header flag.
pub(crate) struct RowBuild {
    pub cells: Vec<CellBuild>,
    pub def: Option<TableDef>,
    pub header: bool,
}

pub(crate) struct TableBuildOutput {
    pub table: Table,
    pub cell_pagination: TableCellPaginationHints,
}

/// An output cell during merge resolution.
struct Out {
    blocks: Vec<Block>,
    pagination: Vec<Option<PaginationHint>>,
    /// Starting column over the table's global boundary set.
    col: usize,
    colspan: u16,
    rowspan: u16,
    tcgrf: u16,
    dropped: bool,
}

fn normalized_column_widths(rows: &[RowBuild], bounds: &[i16]) -> Vec<f32> {
    let Some(first) = rows.first().and_then(|row| row.def.as_ref()) else {
        return Vec::new();
    };
    let Some((&left, &right)) = first.rgdxa.first().zip(first.rgdxa.last()) else {
        return Vec::new();
    };
    if first.rgdxa.len() < 2 {
        return Vec::new();
    }

    for row in rows {
        let Some(def) = row.def.as_ref() else {
            return Vec::new();
        };
        if def.rgdxa.len() < 2
            || def.rgdxa.first() != Some(&left)
            || def.rgdxa.last() != Some(&right)
            || def.rgdxa.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Vec::new();
        }
    }

    let total = i32::from(right) - i32::from(left);
    if total <= 0 || bounds.len() < 2 {
        return Vec::new();
    }
    bounds
        .windows(2)
        .map(|pair| (i32::from(pair[1]) - i32::from(pair[0])) as f32 / total as f32)
        .collect()
}

/// Fold streamed rows into a merge-aware table.
///
/// Column geometry uses the **global set of cell-boundary x-positions**
/// (`rgdxaCenter`) across all rows, so a row with fewer cells than the table has
/// columns (e.g. a single wide header cell) gets the right colspan. Within a row,
/// `fMerged` cells fold left; `rgdxa` then yields the final span.
pub(crate) fn build(rows: Vec<RowBuild>) -> TableBuildOutput {
    let header_rows = rows.iter().take_while(|r| r.header).count();

    // Global sorted set of distinct boundary positions across the whole table.
    let mut bounds: Vec<i16> = rows
        .iter()
        .filter_map(|r| r.def.as_ref())
        .flat_map(|d| d.rgdxa.iter().copied())
        .collect();
    bounds.sort_unstable();
    bounds.dedup();
    let col_widths_pct = normalized_column_widths(&rows, &bounds);
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
                    let cell = cells.next().unwrap_or_default();
                    let g = def.tcgrf.get(k).copied().unwrap_or(0);
                    let (left, right) = (def.rgdxa[k], def.rgdxa[k + 1]);
                    if g & F_MERGED != 0 && !out.is_empty() {
                        let last = out.last_mut().expect("non-empty");
                        last.colspan = (col_of(right).saturating_sub(last.col)).max(1) as u16;
                        last.blocks.extend(cell.blocks);
                        last.pagination.extend(cell.pagination);
                    } else {
                        let col = col_of(left);
                        let colspan = (col_of(right).saturating_sub(col)).max(1) as u16;
                        out.push(Out {
                            blocks: cell.blocks,
                            pagination: cell.pagination,
                            col,
                            colspan,
                            rowspan: 1,
                            tcgrf: g,
                            dropped: false,
                        });
                    }
                }
                // Extra streamed cells beyond the definition fold into the last.
                for cell in cells {
                    if let Some(last) = out.last_mut() {
                        last.blocks.extend(cell.blocks);
                        last.pagination.extend(cell.pagination);
                    }
                }
            }
            None => {
                for (k, cell) in rb.cells.into_iter().enumerate() {
                    out.push(Out {
                        blocks: cell.blocks,
                        pagination: cell.pagination,
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
                    grid[rr][oo].rowspan = grid[rr][oo].rowspan.saturating_add(1);
                    grid[r][o].dropped = true;
                }
            } else {
                open.remove(&col);
            }
        }
    }

    // Emit, skipping merged-away cells.
    let mut model_rows = Vec::with_capacity(grid.len());
    let mut cell_pagination = Vec::with_capacity(grid.len());
    for (r, row) in grid.into_iter().enumerate() {
        let is_header = r < header_rows;
        let mut cells = Vec::with_capacity(row.len());
        let mut row_pagination = Vec::with_capacity(row.len());
        for output in row.into_iter().filter(|output| !output.dropped) {
            row_pagination.push(output.pagination);
            cells.push(Cell {
                blocks: output.blocks,
                col_span: output.colspan,
                row_span: output.rowspan,
                is_header,
                ..Default::default()
            });
        }
        model_rows.push(Row { cells });
        cell_pagination.push(row_pagination);
    }
    TableBuildOutput {
        table: Table {
            rows: model_rows,
            header_rows,
            col_widths_pct,
            ..Default::default()
        },
        cell_pagination,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Block, PaginationHint, ParaProps, Paragraph, Run};

    fn cell(text: &str) -> Vec<Block> {
        vec![Block::Paragraph(Paragraph {
            props: ParaProps::default(),
            runs: vec![Run {
                text: text.to_string(),
                ..Default::default()
            }],
        })]
    }

    fn row_with_bounds(bounds: &[i16]) -> RowBuild {
        let cell_count = bounds.len().saturating_sub(1);
        RowBuild {
            cells: (0..cell_count).map(|_| CellBuild::default()).collect(),
            def: Some(TableDef {
                rgdxa: bounds.to_vec(),
                tcgrf: vec![0; cell_count],
            }),
            header: false,
        }
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
            cells: vec![
                CellBuild {
                    blocks: cell("A"),
                    ..CellBuild::default()
                },
                CellBuild {
                    blocks: cell("B"),
                    ..CellBuild::default()
                },
            ],
            def: Some(def),
            header: false,
        }])
        .table;
        assert_eq!(t.rows[0].cells.len(), 1);
        assert_eq!(t.rows[0].cells[0].col_span, 2);
    }

    #[test]
    fn mixed_row_grids_preserve_global_column_proportions() {
        let table = build(vec![
            row_with_bounds(&[-500, 500, 3500]),
            row_with_bounds(&[-500, 1500, 3500]),
        ])
        .table;

        assert_eq!(table.col_widths_pct, vec![0.25, 0.25, 0.5]);
        assert_eq!(
            table.rows[0]
                .cells
                .iter()
                .map(|cell| cell.col_span)
                .collect::<Vec<_>>(),
            vec![1, 2]
        );
        assert_eq!(
            table.rows[1]
                .cells
                .iter()
                .map(|cell| cell.col_span)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
    }

    #[test]
    fn unusable_row_geometry_keeps_content_sized_width_fallback() {
        let cases = [
            vec![RowBuild {
                cells: vec![CellBuild::default()],
                def: None,
                header: false,
            }],
            vec![row_with_bounds(&[0, 0, 100])],
            vec![row_with_bounds(&[0, 200, 100])],
            vec![
                row_with_bounds(&[0, 100, 300]),
                row_with_bounds(&[10, 100, 300]),
            ],
        ];

        for rows in cases {
            assert!(build(rows).table.col_widths_pct.is_empty());
        }
    }

    #[test]
    fn vertical_merge_rowspan() {
        // Two rows, column 0: top fVertRestart, bottom fVertMerge → rowspan 2,
        // the continuation cell dropped.
        let top = RowBuild {
            cells: vec![
                CellBuild {
                    blocks: cell("X"),
                    ..CellBuild::default()
                },
                CellBuild {
                    blocks: cell("a"),
                    ..CellBuild::default()
                },
            ],
            def: Some(TableDef {
                rgdxa: vec![0, 100, 200],
                tcgrf: vec![F_VERT_RESTART, 0],
            }),
            header: false,
        };
        let bot = RowBuild {
            cells: vec![
                CellBuild {
                    blocks: cell(""),
                    ..CellBuild::default()
                },
                CellBuild {
                    blocks: cell("b"),
                    ..CellBuild::default()
                },
            ],
            def: Some(TableDef {
                rgdxa: vec![0, 100, 200],
                tcgrf: vec![F_VERT_MERGE, 0],
            }),
            header: false,
        };
        let t = build(vec![top, bot]).table;
        assert_eq!(t.rows[0].cells[0].row_span, 2);
        assert_eq!(t.rows[1].cells.len(), 1); // continuation dropped
    }

    #[test]
    fn merge_resolution_keeps_cell_pagination_aligned() {
        let a = PaginationHint {
            keep_next: true,
            ..PaginationHint::default()
        };
        let b = PaginationHint {
            keep_lines: true,
            ..PaginationHint::default()
        };
        let c = PaginationHint {
            widow_control: true,
            ..PaginationHint::default()
        };
        let extra = PaginationHint {
            keep_next: true,
            keep_lines: true,
            ..PaginationHint::default()
        };
        let d = PaginationHint {
            keep_next: true,
            widow_control: true,
            ..PaginationHint::default()
        };
        let e = PaginationHint {
            keep_lines: true,
            widow_control: true,
            ..PaginationHint::default()
        };
        let dropped = PaginationHint {
            keep_next: true,
            keep_lines: true,
            widow_control: true,
        };
        let built_cell = |text: &str, pagination| CellBuild {
            blocks: cell(text),
            pagination: vec![Some(pagination)],
        };

        let built = build(vec![
            RowBuild {
                cells: vec![
                    built_cell("A", a),
                    built_cell("B", b),
                    built_cell("C", c),
                    built_cell("extra", extra),
                ],
                def: Some(TableDef {
                    rgdxa: vec![0, 100, 200, 300],
                    tcgrf: vec![0, F_MERGED, F_VERT_RESTART],
                }),
                header: false,
            },
            RowBuild {
                cells: vec![
                    built_cell("D", d),
                    built_cell("E", e),
                    built_cell("dropped", dropped),
                ],
                def: Some(TableDef {
                    rgdxa: vec![0, 100, 200, 300],
                    tcgrf: vec![0, 0, F_VERT_MERGE],
                }),
                header: false,
            },
        ]);

        assert_eq!(built.table.rows[0].cells.len(), 2);
        assert_eq!(built.table.rows[0].cells[0].col_span, 2);
        assert_eq!(built.table.rows[0].cells[1].row_span, 2);
        assert_eq!(built.table.rows[1].cells.len(), 2);
        assert_eq!(
            built.cell_pagination,
            vec![
                vec![vec![Some(a), Some(b)], vec![Some(c), Some(extra)]],
                vec![vec![Some(d)], vec![Some(e)]],
            ]
        );
    }

    #[test]
    fn doc_vertical_merge_row_span_saturates_instead_of_overflowing() {
        let mut rows = Vec::with_capacity(u16::MAX as usize + 1);
        rows.push(RowBuild {
            cells: vec![CellBuild::default()],
            def: Some(TableDef {
                rgdxa: vec![0, 100],
                tcgrf: vec![F_VERT_RESTART],
            }),
            header: false,
        });
        rows.extend((0..u16::MAX as usize).map(|_| RowBuild {
            cells: vec![CellBuild::default()],
            def: Some(TableDef {
                rgdxa: vec![0, 100],
                tcgrf: vec![F_VERT_MERGE],
            }),
            header: false,
        }));

        let table = build(rows).table;

        assert_eq!(table.rows[0].cells[0].row_span, u16::MAX);
    }
}
