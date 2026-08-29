#![cfg(feature = "render")]

use rwml::{try_render_pdf_with_fonts, Align, Block, Cell, DocModel, Paragraph, Row, Run, Table};

fn paragraph(text: &str) -> Block {
    Block::Paragraph(Paragraph {
        runs: vec![Run {
            text: text.to_string(),
            ..Run::default()
        }],
        ..Paragraph::default()
    })
}

fn nested_table_model(col_widths_pct: Vec<f32>) -> DocModel {
    let nested = Table {
        rows: vec![Row {
            cells: vec![
                Cell {
                    blocks: vec![paragraph("Key")],
                    ..Cell::default()
                },
                Cell {
                    blocks: vec![paragraph("A value that wraps in the narrow column")],
                    ..Cell::default()
                },
            ],
        }],
        col_widths_pct,
        width_pct: Some(0.8),
        align: Some(Align::Center),
        ..Table::default()
    };
    DocModel {
        blocks: vec![Block::Table(Table {
            rows: vec![Row {
                cells: vec![Cell {
                    blocks: vec![Block::Table(nested)],
                    ..Cell::default()
                }],
            }],
            ..Table::default()
        })],
        ..DocModel::default()
    }
}

#[test]
fn nested_table_column_geometry_changes_public_pdf_output_deterministically() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let narrow_key = nested_table_model(vec![0.25, 0.75]);
    let wide_key = nested_table_model(vec![0.75, 0.25]);

    let narrow_pdf =
        try_render_pdf_with_fonts(&narrow_key, &fonts).expect("narrow-key model renders");
    let wide_pdf = try_render_pdf_with_fonts(&wide_key, &fonts).expect("wide-key model renders");

    assert!(narrow_pdf.starts_with(b"%PDF-"));
    assert!(wide_pdf.starts_with(b"%PDF-"));
    assert_ne!(narrow_pdf, wide_pdf);
    assert_eq!(
        narrow_pdf,
        try_render_pdf_with_fonts(&narrow_key, &fonts).expect("rerender is deterministic")
    );
}
