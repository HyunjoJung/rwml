//! Authoring template — build a styled Korean procurement report from data and
//! write a `.docx`. Demonstrates the write/authoring API (`rdoc::write_docx`).
//!
//! ```text
//! cargo run --example report -- report.docx
//! ```

use rdoc::{
    Align, Block, Cell, CharProps, Color, DocModel, DocSetup, ParaProps, Paragraph, Row, Run,
    Table, VCell,
};

fn run(text: &str, props: CharProps) -> Run {
    Run {
        text: text.to_string(),
        props,
        ..Run::default()
    }
}

fn plain(text: &str) -> Run {
    run(text, CharProps::default())
}

fn heading(level: u8, color: Color, text: &str) -> Block {
    Block::Paragraph(Paragraph {
        props: ParaProps {
            heading_level: Some(level),
            ..ParaProps::default()
        },
        runs: vec![run(
            text,
            CharProps {
                color: Some(color),
                ..CharProps::default()
            },
        )],
    })
}

fn para(runs: Vec<Run>) -> Block {
    Block::Paragraph(Paragraph {
        props: ParaProps::default(),
        runs,
    })
}

fn cell(runs: Vec<Run>, shading: Option<Color>) -> Cell {
    Cell {
        blocks: vec![para(runs)],
        shading,
        valign: VCell::Center,
        ..Cell::default()
    }
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "report.docx".to_string());

    let navy = Color {
        r: 0x1F,
        g: 0x38,
        b: 0x64,
    };
    let red = Color {
        r: 0xC0,
        g: 0,
        b: 0,
    };
    let white = Color {
        r: 0xFF,
        g: 0xFF,
        b: 0xFF,
    };
    let zebra = Color {
        r: 0xF2,
        g: 0xF2,
        b: 0xF2,
    };
    let kr = |bold: bool, color: Option<Color>| CharProps {
        bold,
        color,
        font: Some("맑은 고딕".to_string()),
        ..CharProps::default()
    };

    // Centered title (Heading 1, navy).
    let title = Block::Paragraph(Paragraph {
        props: ParaProps {
            heading_level: Some(1),
            align: Align::Center,
            ..ParaProps::default()
        },
        runs: vec![run(
            "나라장터 입찰 비교 리포트",
            CharProps {
                color: Some(navy),
                ..CharProps::default()
            },
        )],
    });

    let intro = para(vec![
        plain("작성일 2026-06-22 — 아래 표에서 "),
        run("마감 임박", kr(true, Some(red))),
        plain(" 건을 빨간색으로 표시했습니다."),
    ]);

    let hdr = |t: &str| cell(vec![run(t, kr(true, Some(white)))], Some(navy));
    let row =
        |name: &str, org: &str, price: &str, due: &str, urgent: bool, fill: Option<Color>| Row {
            cells: vec![
                cell(vec![plain(name)], fill),
                cell(vec![plain(org)], fill),
                cell(vec![run(price, kr(false, None))], fill),
                cell(vec![run(due, kr(urgent, urgent.then_some(red)))], fill),
            ],
        };
    let table = Table {
        rows: vec![
            Row {
                cells: vec![
                    hdr("공고명"),
                    hdr("수요기관"),
                    hdr("추정가격"),
                    hdr("마감일"),
                ],
            },
            row(
                "AI 학습데이터 구축",
                "조달청",
                "₩ 1,200,000,000",
                "2026-06-25",
                true,
                None,
            ),
            row(
                "클라우드 인프라 도입",
                "행정안전부",
                "₩ 800,000,000",
                "2026-07-10",
                false,
                Some(zebra),
            ),
            row(
                "노후 PC 교체",
                "교육부",
                "₩ 350,000,000",
                "2026-06-24",
                true,
                None,
            ),
        ],
        header_rows: 1,
        col_widths_pct: vec![0.40, 0.22, 0.23, 0.15],
    };

    let model = DocModel {
        blocks: vec![
            title,
            intro,
            heading(2, navy, "입찰 목록"),
            Block::Table(table),
        ],
        setup: DocSetup {
            title: Some("입찰 비교 리포트".to_string()),
            creator: Some("kr-bid".to_string()),
            header: vec![Block::Paragraph(Paragraph {
                props: ParaProps {
                    align: Align::Right,
                    ..ParaProps::default()
                },
                runs: vec![run(
                    "나라장터 입찰 비교 리포트 — 대외비",
                    CharProps {
                        color: Some(navy),
                        ..CharProps::default()
                    },
                )],
            })],
            page_numbers: true,
            ..DocSetup::default()
        },
        ..DocModel::default()
    };

    match std::fs::write(&out, rdoc::write_docx(&model)) {
        Ok(()) => eprintln!("wrote {out}"),
        Err(e) => eprintln!("write {out}: {e}"),
    }
}
