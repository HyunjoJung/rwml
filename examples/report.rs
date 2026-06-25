//! Authoring template: build a styled Korean operations report from data and
//! write a `.docx` with the public builder API.
//!
//! ```text
//! cargo run --example report -- report.docx
//! ```

use rdoc::{
    Align, CellBuilder, Color, CommentBuilder, DocBuilder, ImageBuilder, PageSetup,
    ParagraphBuilder, ParagraphStyleBuilder, RunBuilder, TableBuilder, VCell,
};

const KR_FONT: &str = "맑은 고딕";

fn run(text: &str) -> RunBuilder {
    RunBuilder::new(text).font(KR_FONT)
}

fn heading(level: u8, text: &str, color: Color) -> ParagraphBuilder {
    ParagraphBuilder::new()
        .heading_level(level)
        .spacing_before_pt(if level == 1 { 0.0 } else { 14.0 })
        .spacing_after_pt(8.0)
        .push_run(run(text).bold().color(color).build())
}

fn cell(text: &str, fill: Option<Color>, color: Option<Color>, bold: bool) -> CellBuilder {
    let mut builder = CellBuilder::new()
        .paragraph_runs([run(text)
            .color(color.unwrap_or_else(|| Color::rgb(0x22, 0x22, 0x22)))
            .build()])
        .valign(VCell::Center);
    if bold {
        builder = CellBuilder::new()
            .paragraph_runs([run(text)
                .bold()
                .color(color.unwrap_or_else(|| Color::rgb(0x22, 0x22, 0x22)))
                .build()])
            .valign(VCell::Center);
    }
    if let Some(fill) = fill {
        builder.shading(fill)
    } else {
        builder
    }
}

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "report.docx".to_string());

    let navy = Color::rgb(0x1F, 0x38, 0x64);
    let red = Color::rgb(0xC0, 0x00, 0x00);
    let white = Color::rgb(0xFF, 0xFF, 0xFF);
    let zebra = Color::rgb(0xF2, 0xF2, 0xF2);
    let risk_fill = Color::rgb(0xFE, 0xF2, 0xF2);

    let header = |t: &str| cell(t, Some(navy), Some(white), true).header();
    let status_cell = |status: &str, urgent: bool, fill: Option<Color>| {
        let text = run(status)
            .color(if urgent {
                red
            } else {
                Color::rgb(0x22, 0x22, 0x22)
            })
            .bold()
            .build();
        let mut builder = CellBuilder::new()
            .paragraph_runs([text])
            .valign(VCell::Center);
        if let Some(fill) = fill {
            builder = builder.shading(fill);
        }
        builder
    };
    let due_cell = |due: &str, urgent: bool, fill: Option<Color>| {
        let mut text = run(due);
        if urgent {
            text = text.bold().color(red);
        }
        let mut builder = CellBuilder::new()
            .paragraph_runs([text.build()])
            .valign(VCell::Center);
        if let Some(fill) = fill {
            builder = builder.shading(fill);
        }
        builder
    };
    let task_row = |name: &str, team: &str, status: &str, due: &str, urgent: bool, fill| {
        [
            cell(name, fill, None, false),
            cell(team, fill, None, false),
            status_cell(status, urgent, fill),
            due_cell(due, urgent, fill),
        ]
    };

    let task_table = TableBuilder::new()
        .header_rows(1)
        .col_widths_pct([0.40, 0.22, 0.23, 0.15])
        .row([
            header("작업"),
            header("담당 부서"),
            header("상태"),
            header("완료 예정일"),
        ])
        .row(task_row(
            "문서 변환 점검",
            "플랫폼팀",
            "진행 중",
            "2026-06-25",
            true,
            None,
        ))
        .row(task_row(
            "템플릿 정리",
            "제품팀",
            "검토 중",
            "2026-07-10",
            false,
            Some(zebra),
        ))
        .row(task_row(
            "릴리스 노트 작성",
            "문서팀",
            "주의 필요",
            "2026-06-24",
            true,
            None,
        ));

    let mut builder = DocBuilder::new()
        .title("분기 운영 리포트")
        .creator("rdoc")
        .margins_each_pt(54.0, 54.0, 54.0, 54.0)
        .header_runs([run("분기 운영 리포트").bold().color(navy).build()])
        .footer_runs([run("Page ").italic().build()])
        .page_numbers()
        .paragraph_style(
            ParagraphStyleBuilder::new("ReportTitle", "Report Title")
                .based_on("Title")
                .align(Align::Center)
                .spacing_after_pt(18.0)
                .run_font(KR_FONT)
                .run_size_half_pt(32)
                .run_color(navy)
                .run_bold(),
        )
        .paragraph_style(
            ParagraphStyleBuilder::new("RiskCallout", "Risk Callout")
                .spacing_before_pt(6.0)
                .spacing_after_pt(8.0)
                .shading(risk_fill)
                .run_font(KR_FONT)
                .run_color(red)
                .run_bold(),
        )
        .rich_paragraph(
            ParagraphBuilder::new()
                .style("ReportTitle")
                .align(Align::Center)
                .push_run(run("분기 운영 리포트").bold().color(navy).build()),
        )
        .rich_paragraph(ParagraphBuilder::new().spacing_after_pt(10.0).runs([
            run("작성일 2026-06-22 - 아래 표에서 ").build(),
            run("주의 필요").bold().color(red).build(),
            run(" 항목을 빨간색으로 표시했습니다.").build(),
        ]))
        .rich_paragraph(
            ParagraphBuilder::new().style("RiskCallout").push_run(
                run("릴리스 노트 작성 일정 확인")
                    .comment(
                        CommentBuilder::new("담당자와 마감 시간을 재확인하세요.")
                            .author("Reviewer")
                            .initials("RV"),
                    )
                    .build(),
            ),
        )
        .rich_paragraph(heading(2, "작업 목록", navy))
        .rich_table(task_table)
        .section_break()
        .clear_header()
        .page_setup(PageSetup {
            width_pt: 792.0,
            height_pt: 612.0,
            margin_pt: 54.0,
            landscape: true,
            ..PageSetup::default()
        })
        .header_runs([run("후속 조치").bold().color(navy).build()])
        .rich_paragraph(heading(2, "후속 조치", navy))
        .numbered_list(["문서 변환 점검 결과 확인", "릴리스 노트 초안 검토"])
        .bullet_list_level(1, ["담당자 확인"])
        .field("FILENAME \\p", "report.docx")
        .hyperlink("프로젝트 링크", "https://example.com/");

    if let Ok(path) = std::env::var("RDOC_IMAGE") {
        if let Ok(bytes) = std::fs::read(&path) {
            let mime = if path.ends_with(".jpg") || path.ends_with(".jpeg") {
                "image/jpeg"
            } else {
                "image/png"
            };
            builder = builder
                .rich_paragraph(heading(2, "처리 추이", navy))
                .rich_image(
                    ImageBuilder::new(bytes, mime)
                        .alt("처리 추이 차트")
                        .width_px(720),
                );
        }
    }

    let model = builder.build();
    match std::fs::write(&out, rdoc::write_docx(&model)) {
        Ok(()) => eprintln!("wrote {out}"),
        Err(e) => eprintln!("write {out}: {e}"),
    }
}
