//! Integration coverage of the public authoring + rendering entry points: a model
//! built from data must serialize to an Office-openable `.docx` (and re-open
//! through the reader) and render to a valid PDF.

use rdoc::{
    Align, Block, Cell, CharProps, Color, DocModel, DocSetup, Document, ParaProps, Paragraph, Row,
    Table, VCell,
};

fn run(text: &str, props: CharProps) -> rdoc::Run {
    rdoc::Run {
        text: text.to_string(),
        props,
        ..rdoc::Run::default()
    }
}

fn report() -> DocModel {
    let navy = Color {
        r: 0x1F,
        g: 0x38,
        b: 0x64,
    };
    let white = Color {
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    };
    let title = Block::Paragraph(Paragraph {
        props: ParaProps {
            heading_level: Some(1),
            align: Align::Center,
            ..ParaProps::default()
        },
        runs: vec![run(
            "분기 운영 리포트",
            CharProps {
                color: Some(navy),
                ..CharProps::default()
            },
        )],
    });
    let hdr = |t: &str| Cell {
        blocks: vec![Block::Paragraph(Paragraph {
            props: ParaProps::default(),
            runs: vec![run(
                t,
                CharProps {
                    bold: true,
                    color: Some(white),
                    ..CharProps::default()
                },
            )],
        })],
        shading: Some(navy),
        valign: VCell::Center,
        ..Cell::default()
    };
    let cell = |t: &str| Cell {
        blocks: vec![Block::Paragraph(Paragraph {
            props: ParaProps::default(),
            runs: vec![run(t, CharProps::default())],
        })],
        ..Cell::default()
    };
    let table = Table {
        rows: vec![
            Row {
                cells: vec![hdr("작업"), hdr("담당 부서")],
            },
            Row {
                cells: vec![cell("문서 변환 점검"), cell("플랫폼팀")],
            },
        ],
        header_rows: 1,
        col_widths_pct: vec![0.7, 0.3],
    };
    DocModel {
        blocks: vec![title, Block::Table(table)],
        setup: DocSetup {
            title: Some("리포트".to_string()),
            ..DocSetup::default()
        },
        ..DocModel::default()
    }
}

#[test]
fn write_docx_round_trips_through_reader() {
    let bytes = rdoc::write_docx(&report());
    let blob = String::from_utf8_lossy(&bytes);
    assert!(blob.contains("word/document.xml"), "not an OPC package");
    assert!(blob.contains("word/styles.xml"), "named styles missing");
    let doc = Document::open(&bytes).expect("authored .docx must re-open");
    let text = doc.text();
    assert!(text.contains("분기 운영 리포트"), "title lost: {text:?}");
    assert!(
        text.contains("작업") && text.contains("플랫폼팀"),
        "table text lost"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_entry_points_produce_pdf() {
    let model = report();
    let a = rdoc::render_pdf(&model);
    assert!(a.starts_with(b"%PDF") && a.len() > 800);
    // The with-fonts entry point with no extra fonts behaves identically.
    let b = rdoc::render_pdf_with_fonts(&model, &[]);
    assert!(b.starts_with(b"%PDF"));
}
