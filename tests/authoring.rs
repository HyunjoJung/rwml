//! Integration coverage of the public authoring + rendering entry points: a model
//! built from data must serialize to an Office-openable `.docx` (and re-open
//! through the reader) and render to a valid PDF.

use std::io::Read;

use rdoc::{
    Align, Block, Cell, CellBuilder, CellMargins, CharProps, ChartBuilder, ChartKind, ChartShape,
    Color, CommentBuilder, ContentControlBuilder, DocBuilder, DocGridType, DocModel, DocSetup,
    Document, DocumentWarning, FieldKind, FieldRole, ImageBuilder, NoteKind, PageNumberFormat,
    PageSetup, ParaProps, Paragraph, ParagraphBuilder, ParagraphStyleBuilder, RevisionBuilder,
    RevisionKind, RevisionView, Row, RunBuilder, Table, TableBorderSide, TableBorderStyle,
    TableBuilder, TextDirection, VCell,
};

fn run(text: &str, props: CharProps) -> rdoc::Run {
    rdoc::Run {
        text: text.to_string(),
        props,
        ..rdoc::Run::default()
    }
}

fn unzip_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut parts = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();
        parts.insert(file.name().to_string(), bytes);
    }
    parts
}

fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn single_paragraph_text(blocks: &[Block]) -> String {
    let [Block::Paragraph(paragraph)] = blocks else {
        panic!("expected exactly one paragraph block, got {blocks:?}");
    };
    paragraph.text()
}

fn plain_paragraph(text: &str) -> Paragraph {
    Paragraph {
        runs: vec![rdoc::Run {
            text: text.to_string(),
            ..rdoc::Run::default()
        }],
        ..Paragraph::default()
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
        width_pct: None,
        fixed_layout: false,
        indent_twips: None,
        align: None,
        border_color: None,
        border_colors: Default::default(),
        border_size_eighths: None,
        border_sizes: Default::default(),
        border_style: None,
        border_styles: Default::default(),
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
fn doc_builder_adds_lists_links_and_images() {
    let png = tiny_png();
    let model = DocBuilder::new()
        .numbered_list(["First", "Second"])
        .bullet_list(["Check"])
        .hyperlink("rdoc site", "https://example.com/")
        .image(png.clone(), "image/png")
        .build();

    assert_eq!(model.blocks.len(), 5);
    let Block::Paragraph(first) = &model.blocks[0] else {
        panic!("numbered item should be a paragraph");
    };
    let list = first.props.list.as_ref().expect("numbered item has list");
    assert!(list.ordered);
    assert_eq!(list.level, 0);
    assert_eq!(list.label, "");

    let Block::Paragraph(bullet) = &model.blocks[2] else {
        panic!("bullet item should be a paragraph");
    };
    assert_eq!(bullet.props.list.as_ref().map(|l| l.ordered), Some(false));

    let Block::Paragraph(link) = &model.blocks[3] else {
        panic!("hyperlink should be a paragraph");
    };
    assert_eq!(link.runs[0].text, "rdoc site");
    assert!(matches!(
        &link.runs[0].field,
        FieldRole::Hyperlink { url } if url == "https://example.com/"
    ));

    let Block::Image(image) = &model.blocks[4] else {
        panic!("image should be a block image");
    };
    assert_eq!(image.mime.as_deref(), Some("image/png"));
    assert_eq!(image.bytes.as_deref(), Some(png.as_slice()));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();

    assert!(
        document_xml.contains("<w:numPr>") && document_xml.contains("<w:hyperlink r:id="),
        "expected list and hyperlink XML: {document_xml}"
    );
    assert!(
        rels.contains("relationships/hyperlink")
            && rels.contains(r#"Target="https://example.com/""#)
            && rels.contains(r#"TargetMode="External""#)
            && rels.contains("relationships/image"),
        "expected hyperlink and image rels: {rels}"
    );
    assert!(
        content_types.contains(r#"ContentType="image/png""#),
        "expected PNG content type: {content_types}"
    );
    assert_eq!(parts["word/media/image1.png"], png);

    let reopened = Document::open(&bytes).expect("builder-authored .docx reopens");
    let text = reopened.text();
    assert!(
        text.contains("First")
            && text.contains("Second")
            && text.contains("Check")
            && text.contains("rdoc site"),
        "authored text lost: {text:?}"
    );
    assert_eq!(reopened.images().len(), 1);
}

#[test]
fn doc_builder_adds_leveled_lists() {
    let model = DocBuilder::new()
        .numbered_list(["Top"])
        .numbered_list_level(2, ["Deep numbered"])
        .bullet_list_level(1, ["Nested bullet"])
        .list_level(["Clamped"], true, 42)
        .build();

    assert_eq!(model.blocks.len(), 4);
    for (index, expected) in [(0, 0), (1, 2), (2, 1), (3, 8)] {
        let Block::Paragraph(paragraph) = &model.blocks[index] else {
            panic!("list item should be a paragraph");
        };
        assert_eq!(
            paragraph.props.list.as_ref().map(|list| list.level),
            Some(expected),
            "unexpected list level at block {index}"
        );
    }
    let Block::Paragraph(bullet) = &model.blocks[2] else {
        panic!("third item should be a paragraph");
    };
    assert_eq!(
        bullet.props.list.as_ref().map(|list| list.ordered),
        Some(false)
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:ilvl w:val="2"/><w:numId w:val="1"/>"#)
            && document_xml.contains(r#"<w:ilvl w:val="1"/><w:numId w:val="2"/>"#)
            && document_xml.contains(r#"<w:ilvl w:val="8"/><w:numId w:val="1"/>"#),
        "leveled list XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("leveled-list .docx reopens");
    let levels: Vec<_> = reopened
        .model()
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.props.list.as_ref().map(|list| {
                (
                    paragraph.text(),
                    list.level,
                    list.ordered,
                    list.label.clone(),
                )
            }),
            _ => None,
        })
        .collect();
    assert_eq!(levels.len(), 4);
    assert_eq!(levels[1].0, "Deep numbered");
    assert_eq!(levels[1].1, 2);
    assert!(levels[1].2);
    assert_eq!(levels[2].0, "Nested bullet");
    assert_eq!(levels[2].1, 1);
    assert!(!levels[2].2);
    assert_eq!(levels[3].1, 8);
}

#[test]
fn doc_builder_adds_page_breaks() {
    let model = DocBuilder::new()
        .paragraph("Cover")
        .page_break()
        .heading(1, "Detail")
        .build();

    assert!(matches!(model.blocks[1], Block::PageBreak));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:br w:type="page"/>"#),
        "page break XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("page-break .docx reopens");
    assert!(matches!(reopened.model().blocks[1], Block::PageBreak));
    assert_eq!(reopened.to_markdown(), "Cover\n\n\\pagebreak\n\n# Detail");
    assert_eq!(
        reopened.to_html(),
        "<p>Cover</p><hr class=\"page-break\"><h1>Detail</h1>"
    );
}

#[test]
fn doc_builder_adds_section_breaks_with_section_setup() {
    let cover_page = PageSetup {
        margin_pt: 72.0,
        ..PageSetup::default()
    };
    let detail_page = PageSetup {
        width_pt: 792.0,
        height_pt: 612.0,
        margin_pt: 36.0,
        landscape: true,
        ..PageSetup::default()
    };

    let model = DocBuilder::new()
        .page_setup(cover_page)
        .columns(2)
        .page_number_start(3)
        .page_number_format(PageNumberFormat::UpperRoman)
        .text_direction(TextDirection::TopToBottomRightToLeft)
        .header("Cover header")
        .paragraph("Cover")
        .section_break()
        .clear_header()
        .page_setup(detail_page)
        .columns(3)
        .page_number_start(7)
        .page_number_format(PageNumberFormat::DecimalZero)
        .text_direction(TextDirection::LeftToRightTopToBottomVertical)
        .header("Detail header")
        .paragraph("Detail")
        .build();

    assert_eq!(model.blocks.len(), 3);
    let Block::SectionBreak(section) = &model.blocks[1] else {
        panic!("second block should be a section break");
    };
    assert_eq!(section.page.margin_pt, 72.0);
    assert_eq!(section.columns, Some(2));
    assert_eq!(section.page_number_start, Some(3));
    assert_eq!(
        section.page_number_format,
        Some(PageNumberFormat::UpperRoman)
    );
    assert_eq!(
        section.text_direction,
        Some(TextDirection::TopToBottomRightToLeft)
    );
    assert_eq!(section.header.len(), 1);
    assert_eq!(model.setup.page.width_pt, 792.0);
    assert_eq!(model.setup.columns, Some(3));
    assert_eq!(model.setup.page_number_start, Some(7));
    assert_eq!(
        model.setup.page_number_format,
        Some(PageNumberFormat::DecimalZero)
    );
    assert_eq!(
        model.setup.text_direction,
        Some(TextDirection::LeftToRightTopToBottomVertical)
    );
    assert_eq!(model.setup.header.len(), 1);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let header1_xml = String::from_utf8(parts["word/header1.xml"].clone()).unwrap();
    let header2_xml = String::from_utf8(parts["word/header2.xml"].clone()).unwrap();

    assert_eq!(
        document_xml.matches("<w:sectPr>").count(),
        2,
        "expected one section break plus final section properties: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:type w:val="nextPage"/>"#)
            && document_xml.contains(r#"<w:pgSz w:w="11906" w:h="16838"/>"#)
            && document_xml.contains(r#"<w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>"#)
            && document_xml.contains(r#"<w:pgNumType w:start="3" w:fmt="upperRoman"/>"#)
            && document_xml.contains(r#"<w:pgNumType w:start="7" w:fmt="decimalZero"/>"#)
            && document_xml.contains(r#"<w:textDirection w:val="tbRl"/>"#)
            && document_xml.contains(r#"<w:textDirection w:val="lrTbV"/>"#)
            && document_xml.contains(r#"<w:cols w:num="2"/>"#)
            && document_xml.contains(r#"<w:cols w:num="3"/>"#),
        "section page setup missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="header1.xml""#) && rels.contains(r#"Target="header2.xml""#),
        "section header relationships missing: {rels}"
    );
    assert!(
        header1_xml.contains("Cover header") && header2_xml.contains("Detail header"),
        "distinct section headers missing: header1={header1_xml}, header2={header2_xml}"
    );

    let reopened = Document::open(&bytes).expect("multi-section .docx reopens");
    let reopened_model = reopened.model();
    assert!(
        reopened_model
            .blocks
            .iter()
            .any(|block| matches!(block, Block::SectionBreak(_))),
        "section break lost on reopen"
    );
    assert_eq!(reopened_model.setup.page.width_pt, 792.0);
    assert_eq!(reopened_model.setup.columns, Some(3));
    assert_eq!(reopened_model.setup.page_number_start, Some(7));
    assert_eq!(
        reopened_model.setup.page_number_format,
        Some(PageNumberFormat::DecimalZero)
    );
    assert_eq!(
        reopened_model.setup.text_direction,
        Some(TextDirection::LeftToRightTopToBottomVertical)
    );
    let Some(Block::SectionBreak(reopened_section)) = reopened_model
        .blocks
        .iter()
        .find(|block| matches!(block, Block::SectionBreak(_)))
    else {
        panic!("section break lost on reopen");
    };
    assert_eq!(reopened_section.columns, Some(2));
    assert_eq!(reopened_section.page_number_start, Some(3));
    assert_eq!(
        reopened_section.page_number_format,
        Some(PageNumberFormat::UpperRoman)
    );
    assert_eq!(
        reopened_section.text_direction,
        Some(TextDirection::TopToBottomRightToLeft)
    );
    let text = reopened.text();
    assert!(
        text.contains("Cover header") && text.contains("Detail header"),
        "section headers lost from text view: {text:?}"
    );
}

#[test]
fn doc_builder_adds_number_in_dash_page_number_format() {
    let model = DocBuilder::new()
        .page_number_start(5)
        .page_number_format(PageNumberFormat::NumberInDash)
        .paragraph("Dashed numbering")
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:pgNumType w:start="5" w:fmt="numberInDash"/>"#),
        "numberInDash page-number format missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("numberInDash .docx reopens");
    assert_eq!(
        reopened.model().setup.page_number_format,
        Some(PageNumberFormat::NumberInDash)
    );
}

#[test]
fn doc_builder_adds_decimal_full_width_page_number_format() {
    let model = DocBuilder::new()
        .page_number_start(12)
        .page_number_format(PageNumberFormat::DecimalFullWidth)
        .paragraph("Full-width numbering")
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:pgNumType w:start="12" w:fmt="decimalFullWidth"/>"#),
        "decimalFullWidth page-number format missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("decimalFullWidth .docx reopens");
    assert_eq!(
        reopened.model().setup.page_number_format,
        Some(PageNumberFormat::DecimalFullWidth)
    );
}

#[test]
fn doc_builder_adds_decimal_enclosed_circle_page_number_format() {
    let model = DocBuilder::new()
        .page_number_start(12)
        .page_number_format(PageNumberFormat::DecimalEnclosedCircle)
        .paragraph("Circled numbering")
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:pgNumType w:start="12" w:fmt="decimalEnclosedCircle"/>"#),
        "decimalEnclosedCircle page-number format missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("decimalEnclosedCircle .docx reopens");
    assert_eq!(
        reopened.model().setup.page_number_format,
        Some(PageNumberFormat::DecimalEnclosedCircle)
    );
}

#[test]
fn doc_builder_adds_decimal_enclosed_punctuation_page_number_formats() {
    for (format, wml, text) in [
        (
            PageNumberFormat::DecimalEnclosedFullstop,
            "decimalEnclosedFullstop",
            "Fullstop numbering",
        ),
        (
            PageNumberFormat::DecimalEnclosedParen,
            "decimalEnclosedParen",
            "Parenthesized numbering",
        ),
    ] {
        let model = DocBuilder::new()
            .page_number_start(12)
            .page_number_format(format)
            .paragraph(text)
            .build();

        let bytes = rdoc::write_docx(&model);
        let parts = unzip_parts(&bytes);
        let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
        assert!(
            document_xml.contains(&format!(r#"<w:pgNumType w:start="12" w:fmt="{wml}"/>"#)),
            "{wml} page-number format missing: {document_xml}"
        );

        let reopened = Document::open(&bytes).expect("punctuation page format .docx reopens");
        assert_eq!(reopened.model().setup.page_number_format, Some(format));
    }
}

#[test]
fn doc_builder_adds_decimal_width_variant_page_number_formats() {
    for (format, wml, text) in [
        (
            PageNumberFormat::DecimalHalfWidth,
            "decimalHalfWidth",
            "Half-width numbering",
        ),
        (
            PageNumberFormat::DecimalFullWidth2,
            "decimalFullWidth2",
            "Full-width alternate numbering",
        ),
    ] {
        let model = DocBuilder::new()
            .page_number_start(12)
            .page_number_format(format)
            .paragraph(text)
            .build();

        let bytes = rdoc::write_docx(&model);
        let parts = unzip_parts(&bytes);
        let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
        assert!(
            document_xml.contains(&format!(r#"<w:pgNumType w:start="12" w:fmt="{wml}"/>"#)),
            "{wml} page-number format missing: {document_xml}"
        );

        let reopened = Document::open(&bytes).expect("decimal width variant .docx reopens");
        assert_eq!(reopened.model().setup.page_number_format, Some(format));
    }
}

#[test]
fn doc_builder_adds_korean_page_number_formats() {
    for (format, wml, text) in [
        (PageNumberFormat::Ganada, "ganada", "Ganada numbering"),
        (PageNumberFormat::Chosung, "chosung", "Chosung numbering"),
        (
            PageNumberFormat::KoreanDigital,
            "koreanDigital",
            "Korean digital numbering",
        ),
        (
            PageNumberFormat::KoreanCounting,
            "koreanCounting",
            "Korean counting numbering",
        ),
        (
            PageNumberFormat::KoreanLegal,
            "koreanLegal",
            "Korean legal numbering",
        ),
        (
            PageNumberFormat::KoreanDigital2,
            "koreanDigital2",
            "Korean digital alternate numbering",
        ),
    ] {
        let model = DocBuilder::new()
            .page_number_start(1)
            .page_number_format(format)
            .paragraph(text)
            .build();

        let bytes = rdoc::write_docx(&model);
        let parts = unzip_parts(&bytes);
        let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
        assert!(
            document_xml.contains(&format!(r#"<w:pgNumType w:start="1" w:fmt="{wml}"/>"#)),
            "{wml} page-number format missing: {document_xml}"
        );

        let reopened = Document::open(&bytes).expect("Korean page format .docx reopens");
        assert_eq!(reopened.model().setup.page_number_format, Some(format));
    }
}

#[test]
fn write_docx_emits_first_even_header_footer_variants() {
    let model = DocModel {
        blocks: vec![Block::Paragraph(plain_paragraph("Body"))],
        setup: DocSetup {
            header: vec![Block::Paragraph(plain_paragraph("Default header"))],
            first_header: vec![Block::Paragraph(plain_paragraph("First header"))],
            even_header: vec![Block::Paragraph(plain_paragraph("Even header"))],
            footer: vec![Block::Paragraph(plain_paragraph("Default footer"))],
            first_footer: vec![Block::Paragraph(plain_paragraph("First footer"))],
            even_footer: vec![Block::Paragraph(plain_paragraph("Even footer"))],
            ..DocSetup::default()
        },
        ..DocModel::default()
    };

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let settings_xml = String::from_utf8(parts["word/settings.xml"].clone()).unwrap();

    assert!(
        document_xml.contains("<w:titlePg/>")
            && document_xml.contains(r#"<w:headerReference w:type="first""#)
            && document_xml.contains(r#"<w:headerReference w:type="even""#)
            && document_xml.contains(r#"<w:footerReference w:type="first""#)
            && document_xml.contains(r#"<w:footerReference w:type="even""#),
        "first/even section references missing: {document_xml}"
    );
    assert!(
        settings_xml.contains("<w:evenAndOddHeaders/>"),
        "even/odd settings missing: {settings_xml}"
    );

    let reopened = Document::open(&bytes).expect("variant header/footer .docx reopens");
    let setup = &reopened.model().setup;
    assert_eq!(single_paragraph_text(&setup.header), "Default header");
    assert_eq!(single_paragraph_text(&setup.first_header), "First header");
    assert_eq!(single_paragraph_text(&setup.even_header), "Even header");
    assert_eq!(single_paragraph_text(&setup.footer), "Default footer");
    assert_eq!(single_paragraph_text(&setup.first_footer), "First footer");
    assert_eq!(single_paragraph_text(&setup.even_footer), "Even footer");
}

#[test]
fn doc_builder_adds_first_even_header_footer_variants() {
    let model = DocBuilder::new()
        .header("Default header")
        .first_header("First header")
        .even_header("Even header")
        .footer("Default footer")
        .first_footer("First footer")
        .even_footer("Even footer")
        .paragraph("Body")
        .build();

    assert_eq!(single_paragraph_text(&model.setup.header), "Default header");
    assert_eq!(
        single_paragraph_text(&model.setup.first_header),
        "First header"
    );
    assert_eq!(
        single_paragraph_text(&model.setup.even_header),
        "Even header"
    );
    assert_eq!(single_paragraph_text(&model.setup.footer), "Default footer");
    assert_eq!(
        single_paragraph_text(&model.setup.first_footer),
        "First footer"
    );
    assert_eq!(
        single_paragraph_text(&model.setup.even_footer),
        "Even footer"
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let settings_xml = String::from_utf8(parts["word/settings.xml"].clone()).unwrap();

    assert!(
        document_xml.contains("<w:titlePg/>")
            && document_xml.contains(r#"<w:headerReference w:type="first""#)
            && document_xml.contains(r#"<w:headerReference w:type="even""#)
            && document_xml.contains(r#"<w:footerReference w:type="first""#)
            && document_xml.contains(r#"<w:footerReference w:type="even""#),
        "builder first/even references missing: {document_xml}"
    );
    assert!(
        settings_xml.contains("<w:evenAndOddHeaders/>"),
        "builder even/odd settings missing: {settings_xml}"
    );

    let reopened = Document::open(&bytes).expect("builder variant header/footer .docx reopens");
    let setup = &reopened.model().setup;
    assert_eq!(single_paragraph_text(&setup.first_header), "First header");
    assert_eq!(single_paragraph_text(&setup.even_header), "Even header");
    assert_eq!(single_paragraph_text(&setup.first_footer), "First footer");
    assert_eq!(single_paragraph_text(&setup.even_footer), "Even footer");
}

#[test]
fn doc_builder_adds_section_columns() {
    let model = DocBuilder::new()
        .columns(2)
        .paragraph("Column body")
        .build();

    assert_eq!(model.setup.columns, Some(2));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<w:cols w:num="2"/>"#),
        "section columns missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("column section .docx reopens");
    assert_eq!(reopened.model().setup.columns, Some(2));
}

#[test]
fn doc_builder_adds_line_document_grid() {
    let model = DocBuilder::new()
        .doc_grid_lines(360)
        .paragraph("Grid body")
        .build();

    let grid = model.setup.doc_grid.expect("doc grid");
    assert_eq!(grid.grid_type, DocGridType::Lines);
    assert_eq!(grid.line_pitch, Some(360));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<w:docGrid w:type="lines" w:linePitch="360"/>"#),
        "document grid missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("document-grid section .docx reopens");
    assert_eq!(reopened.model().setup.doc_grid, Some(grid));
}

#[test]
fn doc_builder_adds_line_and_character_document_grid() {
    let model = DocBuilder::new()
        .doc_grid_lines_and_chars(360, 40960)
        .paragraph("Grid body")
        .build();

    let grid = model.setup.doc_grid.expect("doc grid");
    assert_eq!(grid.grid_type, DocGridType::LinesAndChars);
    assert_eq!(grid.line_pitch, Some(360));
    assert_eq!(grid.character_space, Some(40960));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(
            r#"<w:docGrid w:type="linesAndChars" w:linePitch="360" w:charSpace="40960"/>"#
        ),
        "line-and-character document grid missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("line-and-character grid .docx reopens");
    assert_eq!(reopened.model().setup.doc_grid, Some(grid));
}

#[test]
fn doc_builder_adds_explicit_title_page_section_option() {
    let model = DocBuilder::new()
        .title_page()
        .header("Default header")
        .paragraph("Body")
        .build();

    assert!(model.setup.title_page);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    assert!(
        document_xml.contains("<w:titlePg/>"),
        "title page section option missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("title-page section .docx reopens");
    assert!(reopened.model().setup.title_page);
}

#[test]
fn run_builder_adds_simple_field_runs() {
    let model = DocBuilder::new()
        .paragraph_runs([
            RunBuilder::new("File: ").build(),
            RunBuilder::new("report.docx").field("FILENAME \\p").build(),
        ])
        .field("PAGE", "1")
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph.runs[1].field,
        FieldRole::Simple {
            instruction: "FILENAME \\p".to_string()
        }
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:fldSimple w:instr=" FILENAME \p ">"#)
            && document_xml.contains(r#"<w:fldSimple w:instr=" PAGE ">"#)
            && document_xml.contains("report.docx"),
        "simple field XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("field-authored .docx reopens");
    let fields = reopened.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Filename);
    assert_eq!(fields[0].instruction, "FILENAME \\p");
    assert_eq!(fields[0].result, "report.docx");
    assert_eq!(fields[1].kind, FieldKind::Page);
    assert_eq!(fields[1].instruction, "PAGE");
    assert_eq!(fields[1].result, "1");

    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened paragraph");
    };
    assert!(matches!(
        &reopened_paragraph.runs[1].field,
        FieldRole::Simple { instruction } if instruction == "FILENAME \\p"
    ));
}

#[test]
fn doc_builder_adds_dirty_toc_heading_range() {
    let model = DocBuilder::new()
        .heading(1, "Executive Summary")
        .heading(3, "Detail")
        .heading(4, "Appendix Detail")
        .toc_heading_range(1, 3)
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[3] else {
        panic!("expected TOC paragraph");
    };
    assert_eq!(
        paragraph.runs[0].field,
        FieldRole::Simple {
            instruction: "TOC \\o \"1-3\"".to_string()
        }
    );
    assert!(paragraph.runs[0].field_dirty);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:fldSimple w:instr=" TOC \o &quot;1-3&quot; " w:dirty="true">"#),
        "dirty TOC field XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("TOC-authored .docx reopens");
    let fields = reopened.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-3\"");
    assert_eq!(fields[0].result, "Contents");
    let computed = fields[0]
        .computed_result
        .as_deref()
        .expect("TOC should compute from authored headings");
    assert!(computed.contains("Executive Summary"), "{computed:?}");
    assert!(computed.contains("Detail"), "{computed:?}");
    assert!(!computed.contains("Appendix Detail"), "{computed:?}");
    assert!(reopened
        .report()
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));
}

#[test]
fn doc_builder_adds_string_custom_properties() {
    let model = DocBuilder::new()
        .custom_property("Client Name", "ACME <Launch>")
        .field(r#"DOCPROPERTY "Client Name""#, "cached")
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    let root_rels = String::from_utf8(parts["_rels/.rels"].clone()).unwrap();
    let custom_xml = String::from_utf8(parts["docProps/custom.xml"].clone()).unwrap();

    assert!(
        content_types.contains(
            r#"<Override PartName="/docProps/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.custom-properties+xml"/>"#
        ),
        "custom properties content type missing: {content_types}"
    );
    assert!(
        root_rels.contains(
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" Target="docProps/custom.xml""#
        ),
        "custom properties relationship missing: {root_rels}"
    );
    assert!(
        custom_xml
            .contains(r#"pid="2" name="Client Name"><vt:lpwstr>ACME &lt;Launch&gt;</vt:lpwstr>"#),
        "custom property XML missing: {custom_xml}"
    );

    let reopened = Document::open(&bytes).expect("custom-property .docx reopens");
    assert_eq!(
        reopened
            .model()
            .custom_properties
            .get("Client Name")
            .map(String::as_str),
        Some("ACME <Launch>")
    );
    let fields = reopened.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("ACME <Launch>"));
}

#[test]
fn run_builder_adds_bookmark_for_ref_fields() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Figure 1").bookmark("Figure1").build()])
        .field("REF Figure1", "stale")
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected bookmark paragraph");
    };
    assert_eq!(paragraph.runs[0].bookmark.as_deref(), Some("Figure1"));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let start = document_xml
        .find(r#"<w:bookmarkStart w:id="0" w:name="Figure1"/>"#)
        .unwrap_or(usize::MAX);
    let text = document_xml
        .find(r#"<w:t xml:space="preserve">Figure 1</w:t>"#)
        .unwrap_or(usize::MAX);
    let end = document_xml
        .find(r#"<w:bookmarkEnd w:id="0"/>"#)
        .unwrap_or(usize::MAX);
    let field = document_xml
        .find(r#"<w:fldSimple w:instr=" REF Figure1 ">"#)
        .unwrap_or(usize::MAX);
    assert!(
        start < text && text < end && end < field,
        "bookmark XML missing or out of order: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("bookmark-authored .docx reopens");
    let fields = reopened.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF Figure1");
    assert_eq!(fields[0].result, "stale");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Figure 1"));
    assert!(
        !reopened.main_text().contains("stale"),
        "resolved REF should display computed bookmark text"
    );
    assert!(reopened
        .report()
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));
}

#[test]
fn run_builder_adds_page_ref_field() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Figure 1").bookmark("Figure1").build()])
        .paragraph_runs([RunBuilder::new("3").page_ref("Figure1").build()])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[1] else {
        panic!("expected page-ref paragraph");
    };
    assert!(matches!(
        &paragraph.runs[0].field,
        FieldRole::Simple { instruction } if instruction == "PAGEREF Figure1 \\h"
    ));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:fldSimple w:instr=" PAGEREF Figure1 \h ">"#)
            && document_xml.contains(r#"<w:t xml:space="preserve">3</w:t>"#),
        "PAGEREF field missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("page-ref-authored .docx reopens");
    let fields = reopened.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "3");
}

#[test]
fn run_builder_adds_inline_hyperlink_runs() {
    let model = DocBuilder::new()
        .rich_paragraph(
            ParagraphBuilder::new().runs([
                RunBuilder::new("See ").build(),
                RunBuilder::new("rdoc")
                    .hyperlink("https://example.com/rdoc?x=1&y=2")
                    .bold()
                    .build(),
                RunBuilder::new(" now").build(),
            ]),
        )
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(paragraph.text(), "See rdoc now");
    assert!(matches!(
        &paragraph.runs[1].field,
        FieldRole::Hyperlink { url } if url == "https://example.com/rdoc?x=1&y=2"
    ));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let see_pos = document_xml
        .find(r#"<w:t xml:space="preserve">See </w:t>"#)
        .expect("leading text run");
    let link_pos = document_xml
        .find(r#"<w:hyperlink r:id="rId"#)
        .expect("hyperlink wrapper");
    let text_pos = document_xml.find(">rdoc</w:t>").expect("hyperlink text");
    let close_pos = document_xml
        .find("</w:hyperlink>")
        .expect("hyperlink close");
    let now_pos = document_xml
        .find(r#"<w:t xml:space="preserve"> now</w:t>"#)
        .expect("trailing text run");
    assert!(
        see_pos < link_pos
            && link_pos < text_pos
            && text_pos < close_pos
            && close_pos < now_pos
            && document_xml.contains("<w:b/>"),
        "inline hyperlink XML missing or not in run order: {document_xml}"
    );
    assert!(
        rels.contains("relationships/hyperlink")
            && rels.contains(r#"Target="https://example.com/rdoc?x=1&amp;y=2""#)
            && rels.contains(r#"TargetMode="External""#),
        "hyperlink relationship missing: {rels}"
    );

    let reopened = Document::open(&bytes).expect("inline-hyperlink .docx reopens");
    assert_eq!(reopened.text(), "See rdoc now");
    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened paragraph");
    };
    assert!(matches!(
        &reopened_paragraph.runs[1].field,
        FieldRole::Hyperlink { url } if url == "https://example.com/rdoc?x=1&y=2"
    ));
}

#[test]
fn run_builder_adds_authored_comment() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Reviewed clause")
            .comment(
                CommentBuilder::new("Check <risk> & owner")
                    .author("Reviewer")
                    .initials("RV")
                    .date("2026-06-24T00:00:00Z"),
            )
            .build()])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    let comment = paragraph.runs[0]
        .comment
        .as_ref()
        .expect("run carries authored comment");
    assert_eq!(comment.text, "Check <risk> & owner");
    assert_eq!(comment.author.as_deref(), Some("Reviewer"));
    assert_eq!(comment.initials.as_deref(), Some("RV"));
    assert_eq!(comment.date.as_deref(), Some("2026-06-24T00:00:00Z"));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let comments_xml = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();

    let start = document_xml
        .find(r#"<w:commentRangeStart w:id="0"/>"#)
        .unwrap_or(usize::MAX);
    let anchor = document_xml
        .find(r#"<w:t xml:space="preserve">Reviewed clause</w:t>"#)
        .unwrap_or(usize::MAX);
    let end = document_xml
        .find(r#"<w:commentRangeEnd w:id="0"/>"#)
        .unwrap_or(usize::MAX);
    let reference = document_xml
        .find(r#"<w:commentReference w:id="0"/>"#)
        .unwrap_or(usize::MAX);
    assert!(
        start < anchor && anchor < end && end < reference,
        "comment markers missing or out of order: {document_xml}"
    );
    assert!(
        comments_xml.contains(r#"<w:comment w:id="0" w:author="Reviewer" w:initials="RV" w:date="2026-06-24T00:00:00Z">"#)
            && comments_xml.contains(r#"<w:t>Check &lt;risk&gt; &amp; owner</w:t>"#),
        "comments.xml missing authored metadata/text: {comments_xml}"
    );
    assert!(
        rels.contains("relationships/comments") && rels.contains(r#"Target="comments.xml""#),
        "comments relationship missing: {rels}"
    );
    assert!(
        content_types.contains(r#"PartName="/word/comments.xml""#)
            && content_types.contains("wordprocessingml.comments+xml"),
        "comments content type missing: {content_types}"
    );

    let reopened = Document::open(&bytes).expect("comment-authored .docx reopens");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "0");
    assert_eq!(comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(comments[0].initials.as_deref(), Some("RV"));
    assert_eq!(comments[0].date.as_deref(), Some("2026-06-24T00:00:00Z"));
    assert_eq!(comments[0].text, "Check <risk> & owner");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Reviewed clause")
    );
}

#[test]
fn run_builder_adds_comment_reply_parent_id() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Original clause")
            .comment(CommentBuilder::new("Original note").author("Reviewer"))
            .build()])
        .paragraph_runs([RunBuilder::new("Reply clause")
            .comment(
                CommentBuilder::new("Reply note")
                    .author("Approver")
                    .parent_comment_id("0"),
            )
            .build()])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[1] else {
        panic!("expected reply paragraph");
    };
    assert_eq!(
        paragraph.runs[0]
            .comment
            .as_ref()
            .and_then(|comment| comment.parent_comment_id.as_deref()),
        Some("0")
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let comments_xml = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    let comments_ex_xml = String::from_utf8(parts["word/commentsExtended.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    assert!(
        comments_xml.contains(r#"<w:comment w:id="1" w:author="Approver" w:parentId="0">"#)
            && comments_xml.contains(r#"<w:t>Reply note</w:t>"#),
        "reply parent id missing: {comments_xml}"
    );
    assert!(
        comments_xml.contains(r#"<w:p w14:paraId="00000001">"#)
            && comments_xml.contains(r#"<w:p w14:paraId="00000002">"#),
        "comment paragraphs should carry ids for reply threading: {comments_xml}"
    );
    assert!(
        comments_ex_xml.contains(r#"<w15:commentEx w15:paraId="00000001" w15:done="0"/>"#)
            && comments_ex_xml.contains(
                r#"<w15:commentEx w15:paraId="00000002" w15:paraIdParent="00000001" w15:done="0"/>"#
            ),
        "commentsExtended.xml missing reply threading metadata: {comments_ex_xml}"
    );
    assert!(
        rels.contains("relationships/commentsExtended")
            && rels.contains(r#"Target="commentsExtended.xml""#),
        "commentsExtended relationship missing: {rels}"
    );
    assert!(
        content_types.contains(r#"PartName="/word/commentsExtended.xml""#)
            && content_types.contains("application/vnd.ms-word.commentsExt+xml"),
        "commentsExtended content type missing: {content_types}"
    );

    let reopened = Document::open(&bytes).expect("comment reply .docx reopens");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[1].id, "1");
    assert_eq!(comments[1].parent_comment_id.as_deref(), Some("0"));
}

#[test]
fn run_builder_authored_comment_writes_tabs_and_breaks() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Reviewed clause")
            .comment(CommentBuilder::new("Line 1\nLine\t2").author("Reviewer"))
            .build()])
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let comments_xml = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();

    assert!(
        comments_xml.contains(r#"<w:t>Line 1</w:t><w:br/><w:t>Line</w:t><w:tab/><w:t>2</w:t>"#),
        "authored comment text should encode tabs and breaks as WML markers: {comments_xml}"
    );

    let reopened = Document::open(&bytes).expect("comment-authored .docx reopens");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Line 1\nLine\t2");
}

#[test]
fn run_builder_adds_authored_notes() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Clause")
            .footnote("Foot <one> & two")
            .build()])
        .paragraph_runs([RunBuilder::new("Appendix").endnote("End\nLine\t2").build()])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected first paragraph");
    };
    let Block::Paragraph(second_paragraph) = &model.blocks[1] else {
        panic!("expected second paragraph");
    };
    assert_eq!(
        paragraph.runs[0].note.as_ref().map(|note| note.kind),
        Some(NoteKind::Footnote)
    );
    assert_eq!(
        second_paragraph.runs[0].note.as_ref().map(|note| note.kind),
        Some(NoteKind::Endnote)
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let footnotes_xml = String::from_utf8(parts["word/footnotes.xml"].clone()).unwrap();
    let endnotes_xml = String::from_utf8(parts["word/endnotes.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();

    let clause_pos = document_xml
        .find(r#"<w:t xml:space="preserve">Clause</w:t>"#)
        .unwrap_or(usize::MAX);
    let footnote_ref_pos = document_xml
        .find(r#"<w:footnoteReference w:id="1"/>"#)
        .unwrap_or(usize::MAX);
    let appendix_pos = document_xml
        .find(r#"<w:t xml:space="preserve">Appendix</w:t>"#)
        .unwrap_or(usize::MAX);
    let endnote_ref_pos = document_xml
        .find(r#"<w:endnoteReference w:id="1"/>"#)
        .unwrap_or(usize::MAX);
    assert!(
        clause_pos < footnote_ref_pos
            && footnote_ref_pos < appendix_pos
            && appendix_pos < endnote_ref_pos,
        "note references missing or out of order: {document_xml}"
    );
    assert!(
        footnotes_xml.contains(r#"<w:footnote w:type="separator" w:id="-1">"#)
            && footnotes_xml.contains(r#"<w:footnote w:id="1">"#)
            && footnotes_xml
                .contains(r#"<w:t xml:space="preserve">Foot &lt;one&gt; &amp; two</w:t>"#),
        "footnotes part missing authored note: {footnotes_xml}"
    );
    assert!(
        endnotes_xml.contains(r#"<w:endnote w:type="separator" w:id="-1">"#)
            && endnotes_xml.contains(r#"<w:endnote w:id="1">"#)
            && endnotes_xml.contains(r#"<w:t xml:space="preserve">End</w:t><w:br/><w:t xml:space="preserve">Line</w:t><w:tab/><w:t xml:space="preserve">2</w:t>"#),
        "endnotes part missing authored note: {endnotes_xml}"
    );
    assert!(
        rels.contains("relationships/footnotes")
            && rels.contains(r#"Target="footnotes.xml""#)
            && rels.contains("relationships/endnotes")
            && rels.contains(r#"Target="endnotes.xml""#),
        "note relationships missing: {rels}"
    );
    assert!(
        content_types.contains(r#"PartName="/word/footnotes.xml""#)
            && content_types.contains("wordprocessingml.footnotes+xml")
            && content_types.contains(r#"PartName="/word/endnotes.xml""#)
            && content_types.contains("wordprocessingml.endnotes+xml"),
        "note content types missing: {content_types}"
    );

    let reopened = Document::open(&bytes).expect("note-authored .docx reopens");
    let notes = reopened.notes();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0].kind, NoteKind::Footnote);
    assert_eq!(notes[0].id, "1");
    assert_eq!(notes[0].text, "Foot <one> & two");
    assert_eq!(
        notes[0].anchor.as_ref().map(|anchor| anchor.text.as_str()),
        Some("Clause")
    );
    assert_eq!(notes[1].kind, NoteKind::Endnote);
    assert_eq!(notes[1].id, "1");
    assert_eq!(notes[1].text, "End\nLine\t2");
    assert_eq!(
        notes[1].anchor.as_ref().map(|anchor| anchor.text.as_str()),
        Some("Appendix")
    );
}

#[test]
fn run_builder_adds_authored_revision_runs() {
    let model = DocBuilder::new()
        .paragraph_runs([
            RunBuilder::new("kept").build(),
            RunBuilder::new("added")
                .revision(
                    RevisionBuilder::insertion()
                        .author("Alice")
                        .date("2026-06-24T01:00:00Z"),
                )
                .bold()
                .build(),
            RunBuilder::new("removed")
                .revision(RevisionBuilder::deletion().author("Bob"))
                .italic()
                .build(),
        ])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    assert_eq!(
        paragraph.runs[1].revision.as_ref().map(|r| r.kind),
        Some(RevisionKind::Insertion)
    );
    assert_eq!(
        paragraph.runs[2].revision.as_ref().map(|r| r.kind),
        Some(RevisionKind::Deletion)
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    let ins_pos = document_xml
        .find(r#"<w:ins w:id="0""#)
        .unwrap_or(usize::MAX);
    let add_pos = document_xml
        .find(r#"<w:t xml:space="preserve">added</w:t>"#)
        .unwrap_or(usize::MAX);
    let del_pos = document_xml
        .find(r#"<w:del w:id="1""#)
        .unwrap_or(usize::MAX);
    let del_text_pos = document_xml
        .find(r#"<w:delText xml:space="preserve">removed</w:delText>"#)
        .unwrap_or(usize::MAX);
    assert!(
        ins_pos < add_pos && add_pos < del_pos && del_pos < del_text_pos,
        "revision XML missing or out of order: {document_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:ins w:id="0" w:author="Alice" w:date="2026-06-24T01:00:00Z">"#)
            && document_xml.contains("<w:b/>")
            && document_xml.contains(r#"<w:del w:id="1" w:author="Bob">"#)
            && document_xml.contains("<w:i/>"),
        "revision metadata or run properties missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("revision-authored .docx reopens");
    let revisions = reopened.revisions();
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].author.as_deref(), Some("Alice"));
    assert_eq!(revisions[0].date.as_deref(), Some("2026-06-24T01:00:00Z"));
    assert_eq!(revisions[0].text, "added");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].author.as_deref(), Some("Bob"));
    assert_eq!(revisions[1].text, "removed");
    assert_eq!(
        reopened.main_text_with_revision_view(RevisionView::Accepted),
        "kept added"
    );
    assert_eq!(
        reopened.main_text_with_revision_view(RevisionView::Original),
        "kept removed"
    );
}

#[test]
fn run_builder_adds_plain_text_content_control() {
    let model = DocBuilder::new()
        .paragraph_runs([
            RunBuilder::new("Locked clause")
                .content_control(
                    ContentControlBuilder::new()
                        .alias("Clause title")
                        .tag("clause-001"),
                )
                .bold()
                .build(),
            RunBuilder::new(" tail").build(),
        ])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    let control = paragraph.runs[0]
        .content_control
        .as_ref()
        .expect("run carries content-control metadata");
    assert_eq!(control.alias.as_deref(), Some("Clause title"));
    assert_eq!(control.tag.as_deref(), Some("clause-001"));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let sdt_pos = document_xml.find("<w:sdt>").unwrap_or(usize::MAX);
    let alias_pos = document_xml
        .find(r#"<w:alias w:val="Clause title"/>"#)
        .unwrap_or(usize::MAX);
    let tag_pos = document_xml
        .find(r#"<w:tag w:val="clause-001"/>"#)
        .unwrap_or(usize::MAX);
    let content_pos = document_xml.find("<w:sdtContent>").unwrap_or(usize::MAX);
    let text_pos = document_xml
        .find(r#"<w:t xml:space="preserve">Locked clause</w:t>"#)
        .unwrap_or(usize::MAX);
    assert!(
        sdt_pos < alias_pos
            && alias_pos < tag_pos
            && tag_pos < content_pos
            && content_pos < text_pos,
        "content-control XML missing or out of order: {document_xml}"
    );
    assert!(
        document_xml.contains("<w:b/>"),
        "content-control run formatting missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("content-control-authored .docx reopens");
    assert_eq!(reopened.text(), "Locked clause tail");
    assert_eq!(reopened.report().features.content_controls, 1);
    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened paragraph");
    };
    assert_eq!(reopened_paragraph.text(), "Locked clause tail");
    assert!(reopened_paragraph.runs[0].props.bold);
    let reopened_control = reopened_paragraph.runs[0]
        .content_control
        .as_ref()
        .expect("reopened run carries content-control metadata");
    assert_eq!(reopened_control.alias.as_deref(), Some("Clause title"));
    assert_eq!(reopened_control.tag.as_deref(), Some("clause-001"));
}

#[test]
fn content_control_builder_adds_data_binding_metadata() {
    let model = DocBuilder::new()
        .paragraph_runs([RunBuilder::new("Bound value")
            .content_control(
                ContentControlBuilder::new()
                    .alias("Client")
                    .tag("client-name")
                    .data_binding(
                        r#"/root/client[@code="A&B"]"#,
                        "{11111111-2222-3333-4444-555555555555}",
                    ),
            )
            .build()])
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph");
    };
    let control = paragraph.runs[0]
        .content_control
        .as_ref()
        .expect("run carries content-control metadata");
    assert_eq!(
        control.data_binding_xpath.as_deref(),
        Some(r#"/root/client[@code="A&B"]"#)
    );
    assert_eq!(
        control.data_binding_store_item_id.as_deref(),
        Some("{11111111-2222-3333-4444-555555555555}")
    );

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let tag_pos = document_xml
        .find(r#"<w:tag w:val="client-name"/>"#)
        .unwrap_or(usize::MAX);
    let binding_pos = document_xml
        .find(r#"<w:dataBinding w:xpath="/root/client[@code=&quot;A&amp;B&quot;]" w:storeItemID="{11111111-2222-3333-4444-555555555555}"/>"#)
        .unwrap_or(usize::MAX);
    let content_pos = document_xml.find("<w:sdtContent>").unwrap_or(usize::MAX);
    assert!(
        tag_pos < binding_pos && binding_pos < content_pos,
        "data binding XML missing or out of order: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("data-bound content-control .docx reopens");
    assert_eq!(reopened.text(), "Bound value");
    assert_eq!(reopened.report().features.content_controls, 1);
    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened paragraph");
    };
    let reopened_control = reopened_paragraph.runs[0]
        .content_control
        .as_ref()
        .expect("reopened run carries data-binding metadata");
    assert_eq!(reopened_control.alias.as_deref(), Some("Client"));
    assert_eq!(reopened_control.tag.as_deref(), Some("client-name"));
    assert_eq!(
        reopened_control.data_binding_xpath.as_deref(),
        Some(r#"/root/client[@code="A&B"]"#)
    );
    assert_eq!(
        reopened_control.data_binding_store_item_id.as_deref(),
        Some("{11111111-2222-3333-4444-555555555555}")
    );
}

#[test]
fn doc_builder_adds_custom_xml_item() {
    let store_item_id = "{11111111-2222-3333-4444-555555555555}";
    let xml = r#"<root><client code="A&amp;B">ACME</client></root>"#;
    let model = DocBuilder::new()
        .custom_xml_item(store_item_id, xml)
        .paragraph_runs([RunBuilder::new("Bound value")
            .content_control(
                ContentControlBuilder::new()
                    .tag("client-name")
                    .data_binding("/root/client", store_item_id),
            )
            .build()])
        .build();

    assert_eq!(model.custom_xml_items[0].store_item_id, store_item_id);
    assert_eq!(model.custom_xml_items[0].xml, xml);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    assert_eq!(
        String::from_utf8(parts["customXml/item1.xml"].clone()).unwrap(),
        xml
    );
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    assert!(
        content_types.contains(
            r#"<Override PartName="/customXml/itemProps1.xml" ContentType="application/vnd.openxmlformats-officedocument.customXmlProperties+xml"/>"#
        ),
        "custom XML properties content type missing: {content_types}"
    );
    let item_rels = String::from_utf8(parts["customXml/_rels/item1.xml.rels"].clone()).unwrap();
    assert!(
        item_rels.contains(
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/customXmlProps" Target="itemProps1.xml""#
        ),
        "custom XML item relationship missing: {item_rels}"
    );
    let item_props = String::from_utf8(parts["customXml/itemProps1.xml"].clone()).unwrap();
    assert!(
        item_props
            .contains(r#"<ds:datastoreItem ds:itemID="{11111111-2222-3333-4444-555555555555}""#)
            && item_props.contains("<ds:schemaRefs/>"),
        "custom XML item properties missing: {item_props}"
    );

    let reopened = Document::open(&bytes).expect("custom XML .docx reopens");
    assert_eq!(reopened.text(), "Bound value");
    assert_eq!(reopened.report().features.content_controls, 1);
    assert_eq!(reopened.model().custom_xml_items, model.custom_xml_items);
}

#[test]
fn run_builder_adds_styled_paragraph_and_heading_runs() {
    let model = DocBuilder::new()
        .paragraph_runs([
            RunBuilder::new("Status: ").bold().build(),
            RunBuilder::new("green")
                .italic()
                .underline()
                .font("Arial")
                .size_half_pt(28)
                .color(Color::rgb(0, 128, 0))
                .highlight("yellow")
                .build(),
        ])
        .heading_runs(2, [RunBuilder::new("Section").small_caps().caps().build()])
        .build();

    assert_eq!(model.blocks.len(), 2);
    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("first block should be a paragraph");
    };
    assert_eq!(paragraph.runs.len(), 2);
    assert!(paragraph.runs[0].props.bold);
    assert!(paragraph.runs[1].props.italic);
    assert!(paragraph.runs[1].props.underline);
    assert_eq!(paragraph.runs[1].props.font.as_deref(), Some("Arial"));
    assert_eq!(paragraph.runs[1].props.size_half_pt, Some(28));
    assert_eq!(paragraph.runs[1].props.color, Some(Color::rgb(0, 128, 0)));
    assert_eq!(paragraph.runs[1].props.highlight.as_deref(), Some("yellow"));

    let Block::Paragraph(heading) = &model.blocks[1] else {
        panic!("second block should be a heading");
    };
    assert_eq!(heading.props.heading_level, Some(2));
    assert!(heading.runs[0].props.small_caps);
    assert!(heading.runs[0].props.caps);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains("<w:b/>")
            && document_xml.contains("<w:i/>")
            && document_xml.contains(r#"<w:u w:val="single"/>"#)
            && document_xml.contains(r#"<w:rFonts w:ascii="Arial""#)
            && document_xml.contains(r#"<w:sz w:val="28"/>"#)
            && document_xml.contains(r#"<w:color w:val="008000"/>"#)
            && document_xml.contains(r#"<w:highlight w:val="yellow"/>"#)
            && document_xml.contains("<w:smallCaps/>")
            && document_xml.contains("<w:caps/>"),
        "styled run properties missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("styled builder .docx reopens");
    let text = reopened.text();
    assert!(text.contains("Status: green"), "styled text lost: {text:?}");
}

#[test]
fn paragraph_builder_adds_layout_properties() {
    let shade = Color::rgb(0xEE, 0xF2, 0xF7);
    let model = DocBuilder::new()
        .rich_paragraph(
            ParagraphBuilder::new()
                .align(Align::Justify)
                .spacing_before_pt(12.0)
                .spacing_after_pt(6.0)
                .line_pct(1.5)
                .indent_left_pt(24.0)
                .indent_right_pt(12.0)
                .first_line_pt(18.0)
                .page_break_before()
                .shading(shade)
                .runs([
                    RunBuilder::new("Layout ").bold().build(),
                    RunBuilder::new("paragraph").italic().build(),
                ]),
        )
        .build();

    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected paragraph block");
    };
    assert_eq!(paragraph.props.align, Align::Justify);
    assert_eq!(paragraph.props.spacing.before_pt, Some(12.0));
    assert_eq!(paragraph.props.spacing.after_pt, Some(6.0));
    assert_eq!(paragraph.props.spacing.line_pct, Some(1.5));
    assert_eq!(paragraph.props.indent.left_pt, Some(24.0));
    assert_eq!(paragraph.props.indent.right_pt, Some(12.0));
    assert_eq!(paragraph.props.indent.first_line_pt, Some(18.0));
    assert!(paragraph.props.page_break_before);
    assert_eq!(paragraph.props.shading, Some(shade));
    assert!(paragraph.runs[0].props.bold);
    assert!(paragraph.runs[1].props.italic);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:shd w:val="clear" w:color="auto" w:fill="EEF2F7"/>"#)
            && document_xml.contains(
                r#"<w:spacing w:before="240" w:after="120" w:line="360" w:lineRule="auto"/>"#
            )
            && document_xml.contains(r#"<w:ind w:left="480" w:right="240" w:firstLine="360"/>"#)
            && document_xml.contains(r#"<w:jc w:val="both"/>"#)
            && document_xml.contains("<w:pageBreakBefore/>")
            && document_xml.contains("<w:b/>")
            && document_xml.contains("<w:i/>"),
        "paragraph layout XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("paragraph builder .docx reopens");
    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened paragraph");
    };
    assert_eq!(reopened_paragraph.props.align, Align::Justify);
    assert!(reopened_paragraph.props.page_break_before);
    assert_eq!(reopened_paragraph.props.shading, Some(shade));
    assert_eq!(reopened_paragraph.text(), "Layout paragraph");
}

#[test]
fn doc_builder_adds_custom_paragraph_style() {
    let accent = Color::rgb(0x7A, 0x1F, 0x1F);
    let model = DocBuilder::new()
        .paragraph_style(
            ParagraphStyleBuilder::new("RiskCallout", "Risk callout")
                .based_on("Normal")
                .next("Normal")
                .q_format()
                .align(Align::Justify)
                .spacing_before_pt(6.0)
                .spacing_after_pt(12.0)
                .indent_left_pt(18.0)
                .shading(Color::rgb(0xFE, 0xF2, 0xF2))
                .run_bold()
                .run_color(accent)
                .run_size_half_pt(24),
        )
        .rich_paragraph(
            ParagraphBuilder::text("Risk status")
                .style("RiskCallout")
                .push_run(RunBuilder::new(": review required").italic().build()),
        )
        .build();

    assert_eq!(model.setup.styles.len(), 1);
    assert_eq!(model.setup.styles[0].id, "RiskCallout");
    let Block::Paragraph(paragraph) = &model.blocks[0] else {
        panic!("expected styled paragraph");
    };
    assert_eq!(paragraph.props.style_id.as_deref(), Some("RiskCallout"));
    assert_eq!(paragraph.text(), "Risk status: review required");

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let styles_xml = String::from_utf8(parts["word/styles.xml"].clone()).unwrap();
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();

    assert!(
        styles_xml.contains(r#"<w:style w:type="paragraph" w:styleId="RiskCallout">"#)
            && styles_xml.contains(r#"<w:name w:val="Risk callout"/>"#)
            && styles_xml.contains(r#"<w:basedOn w:val="Normal"/>"#)
            && styles_xml.contains(r#"<w:next w:val="Normal"/>"#)
            && styles_xml.contains("<w:qFormat/>")
            && styles_xml.contains(r#"<w:shd w:val="clear" w:color="auto" w:fill="FEF2F2"/>"#)
            && styles_xml.contains(r#"<w:spacing w:before="120" w:after="240"/>"#)
            && styles_xml.contains(r#"<w:ind w:left="360"/>"#)
            && styles_xml.contains(r#"<w:jc w:val="both"/>"#)
            && styles_xml.contains("<w:b/>")
            && styles_xml.contains(r#"<w:color w:val="7A1F1F"/>"#)
            && styles_xml.contains(r#"<w:sz w:val="24"/>"#),
        "custom style XML missing: {styles_xml}"
    );
    assert!(
        document_xml.contains(r#"<w:pStyle w:val="RiskCallout"/>"#)
            && document_xml.contains("<w:i/>"),
        "styled paragraph XML missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/styles") && rels.contains(r#"Target="styles.xml""#),
        "styles relationship missing: {rels}"
    );

    let reopened = Document::open(&bytes).expect("custom style .docx reopens");
    let Block::Paragraph(reopened_paragraph) = &reopened.model().blocks[0] else {
        panic!("expected reopened styled paragraph");
    };
    assert_eq!(
        reopened_paragraph.props.style_id.as_deref(),
        Some("RiskCallout")
    );
    assert_eq!(
        reopened_paragraph.props.style_name.as_deref(),
        Some("Risk callout")
    );
    assert_eq!(reopened_paragraph.text(), "Risk status: review required");
}

#[test]
fn doc_builder_creates_basic_report_model() {
    let model = DocBuilder::new()
        .title("Builder Report")
        .creator("rdoc")
        .page_setup(PageSetup {
            margin_pt: 54.0,
            ..PageSetup::default()
        })
        .heading(1, "Builder Report")
        .paragraph("Summary")
        .table_with_header([["Metric", "Value"], ["Openable", "Yes"]])
        .header("Builder Report")
        .footer("Confidential")
        .page_numbers()
        .build();

    assert_eq!(model.setup.title.as_deref(), Some("Builder Report"));
    assert_eq!(model.setup.creator.as_deref(), Some("rdoc"));
    assert_eq!(model.setup.page.margin_pt, 54.0);
    assert_eq!(model.setup.header.len(), 1);
    assert_eq!(model.setup.footer.len(), 1);
    assert!(model.setup.page_numbers);
    assert_eq!(model.blocks.len(), 3);

    let Block::Paragraph(title) = &model.blocks[0] else {
        panic!("first block should be a heading");
    };
    assert_eq!(title.props.heading_level, Some(1));
    assert_eq!(title.text(), "Builder Report");

    let Block::Table(table) = &model.blocks[2] else {
        panic!("third block should be a table");
    };
    assert_eq!(table.header_rows, 1);
    assert!(table.rows[0].cells.iter().all(|cell| cell.is_header));
    assert_eq!(table.rows[1].cells[1].text(), "Yes");

    let bytes = rdoc::write_docx(&model);
    let reopened = Document::open(&bytes).expect("builder-authored .docx reopens");
    let text = reopened.text();
    assert!(text.contains("Builder Report"), "title lost: {text:?}");
    assert!(
        text.contains("Openable") && text.contains("Yes"),
        "table lost"
    );
}

#[test]
fn doc_builder_sets_page_geometry_with_helpers() {
    let model = DocBuilder::new()
        .page_size_pt(792.0, 612.0)
        .landscape()
        .margins_pt(36.0)
        .margins_each_pt(72.0, 54.0, 36.0, 18.0)
        .paragraph("geometry")
        .build();

    assert_eq!(model.setup.page.width_pt, 792.0);
    assert_eq!(model.setup.page.height_pt, 612.0);
    assert!(model.setup.page.landscape);
    assert_eq!(model.setup.page.margin_pt, 36.0);
    assert_eq!(model.setup.page.margin_top_pt, Some(72.0));
    assert_eq!(model.setup.page.margin_right_pt, Some(54.0));
    assert_eq!(model.setup.page.margin_bottom_pt, Some(36.0));
    assert_eq!(model.setup.page.margin_left_pt, Some(18.0));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>"#),
        "page size missing: {document_xml}"
    );
    assert!(
        document_xml
            .contains(r#"<w:pgMar w:top="1440" w:right="1080" w:bottom="720" w:left="360""#),
        "page margins missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("geometry .docx reopens");
    let page = reopened.model().setup.page;
    assert_eq!(page.width_pt, 792.0);
    assert_eq!(page.height_pt, 612.0);
    assert!(page.landscape);
    assert_eq!(page.margin_top_pt, Some(72.0));
    assert_eq!(page.margin_right_pt, Some(54.0));
    assert_eq!(page.margin_bottom_pt, Some(36.0));
    assert_eq!(page.margin_left_pt, Some(18.0));
}

#[test]
fn doc_builder_adds_styled_header_and_footer_blocks() {
    let footer_block = Block::Paragraph(Paragraph {
        props: ParaProps {
            align: Align::Center,
            ..ParaProps::default()
        },
        runs: vec![RunBuilder::new("Manual footer").underline().build()],
    });

    let model = DocBuilder::new()
        .header_runs([RunBuilder::new("Confidential").bold().build()])
        .push_footer_block(footer_block)
        .footer_runs([RunBuilder::new("Page ").italic().build()])
        .page_numbers()
        .paragraph("body")
        .build();

    assert_eq!(model.setup.header.len(), 1);
    assert_eq!(model.setup.footer.len(), 2);
    let Block::Paragraph(header) = &model.setup.header[0] else {
        panic!("header should be paragraph");
    };
    assert!(header.runs[0].props.bold);
    let Block::Paragraph(footer) = &model.setup.footer[0] else {
        panic!("footer should be paragraph");
    };
    assert_eq!(footer.props.align, Align::Center);
    assert!(footer.runs[0].props.underline);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let header_xml = String::from_utf8(parts["word/header1.xml"].clone()).unwrap();
    let footer_xml = String::from_utf8(parts["word/footer1.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();

    assert!(
        header_xml.contains("Confidential") && header_xml.contains("<w:b/>"),
        "styled header missing: {header_xml}"
    );
    assert!(
        footer_xml.contains("Manual footer")
            && footer_xml.contains(r#"<w:u w:val="single"/>"#)
            && footer_xml.contains("Page ")
            && footer_xml.contains("<w:i/>")
            && footer_xml.contains("PAGE"),
        "styled footer/page number missing: {footer_xml}"
    );
    assert!(
        rels.contains("relationships/header") && rels.contains("relationships/footer"),
        "header/footer rels missing: {rels}"
    );
}

#[test]
fn table_builder_adds_rich_table_cells() {
    let navy = Color::rgb(0x1F, 0x38, 0x64);
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .header_rows(1)
                .col_widths_pct([0.25, 0.50, 0.25])
                .row([
                    CellBuilder::text("Metric")
                        .header()
                        .shading(navy)
                        .valign(VCell::Center)
                        .width_pct(0.25),
                    CellBuilder::text("Quarter")
                        .header()
                        .shading(navy)
                        .valign(VCell::Center)
                        .width_pct(0.50)
                        .col_span(2),
                ])
                .row([
                    CellBuilder::text("Revenue")
                        .row_span(2)
                        .valign(VCell::Bottom),
                    CellBuilder::new()
                        .paragraph_runs([RunBuilder::new("Q1").bold().build()])
                        .width_pct(0.50),
                    CellBuilder::text("42"),
                ])
                .row([CellBuilder::text("Q2"), CellBuilder::text("51")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.header_rows, 1);
    assert_eq!(table.col_widths_pct, vec![0.25, 0.50, 0.25]);
    assert!(table.rows[0].cells[0].is_header);
    assert_eq!(table.rows[0].cells[0].shading, Some(navy));
    assert_eq!(table.rows[0].cells[1].col_span, 2);
    assert_eq!(table.rows[1].cells[0].row_span, 2);
    assert_eq!(table.rows[1].cells[0].valign, VCell::Bottom);
    assert!(matches!(
        &table.rows[1].cells[1].blocks[0],
        Block::Paragraph(p) if p.runs[0].props.bold && p.text() == "Q1"
    ));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains("<w:tblHeader/>")
            && document_xml.contains(r#"<w:gridSpan w:val="2"/>"#)
            && document_xml.contains(r#"<w:vMerge w:val="restart"/>"#)
            && document_xml.contains(r#"<w:vAlign w:val="center"/>"#)
            && document_xml.contains(r#"<w:vAlign w:val="bottom"/>"#)
            && document_xml.contains(r#"<w:shd w:val="clear" w:color="auto" w:fill="1F3864"/>"#)
            && document_xml.contains(r#"<w:tcW w:w="2500" w:type="pct"/>"#)
            && document_xml.contains("<w:b/>"),
        "rich table XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("rich builder table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened rich table");
    };
    assert_eq!(reopened_table.header_rows, 1);
    assert_eq!(reopened_table.rows[0].cells[0].shading, Some(navy));
    assert_eq!(reopened_table.rows[0].cells[1].col_span, 2);
    assert_eq!(reopened_table.rows[1].cells[0].row_span, 2);
    assert_eq!(reopened_table.rows[1].cells[0].valign, VCell::Bottom);
    assert_eq!(reopened_table.rows[1].cells[1].text(), "Q1");
}

#[test]
fn table_builder_adds_fixed_layout() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .fixed_layout()
                .col_widths_pct([0.4, 0.6])
                .row([CellBuilder::text("A"), CellBuilder::text("B")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert!(table.fixed_layout);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:tblLayout w:type="fixed"/>"#),
        "fixed table layout XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("fixed-layout table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert!(reopened_table.fixed_layout);
}

#[test]
fn table_builder_adds_table_indent() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .indent_twips(720)
                .row([CellBuilder::text("Indented")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.indent_twips, Some(720));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:tblInd w:w="720" w:type="dxa"/>"#),
        "table indent XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("indented table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.indent_twips, Some(720));
}

#[test]
fn table_builder_adds_table_alignment() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .align(Align::Center)
                .row([CellBuilder::text("Centered")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.align, Some(Align::Center));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:jc w:val="center"/>"#),
        "table alignment XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("aligned table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.align, Some(Align::Center));
}

#[test]
fn table_builder_adds_table_width() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .width_pct(0.8)
                .row([CellBuilder::text("Wide")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.width_pct, Some(0.8));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:tblW w:w="4000" w:type="pct"/>"#),
        "table width XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("width table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.width_pct, Some(0.8));
}

#[test]
fn table_builder_adds_table_border_color() {
    let border = Color::rgb(0x22, 0x66, 0xAA);
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_color(border)
                .row([CellBuilder::text("Bordered")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_color, Some(border));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="2266AA"/>"#),
        "table border color XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("border-color table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.border_color, Some(border));
}

#[test]
fn table_builder_adds_table_border_size() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_size_eighths(12)
                .row([CellBuilder::text("Thick")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_size_eighths, Some(12));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="single" w:sz="12" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:insideV w:val="single" w:sz="12" w:space="0" w:color="auto"/>"#),
        "table border size XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("border-size table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.border_size_eighths, Some(12));
}

#[test]
fn table_builder_adds_table_border_style() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_style(TableBorderStyle::Dotted)
                .row([CellBuilder::text("Dotted")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_style, Some(TableBorderStyle::Dotted));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="dotted" w:sz="4" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:insideV w:val="dotted" w:sz="4" w:space="0" w:color="auto"/>"#),
        "table border style XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("border-style table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.border_style, Some(TableBorderStyle::Dotted));
}

#[test]
fn table_builder_adds_side_specific_border_colors() {
    let border = Color::rgb(0x22, 0x66, 0xAA);
    let top = Color::rgb(0xAA, 0x33, 0x11);
    let inside = Color::rgb(0x11, 0x88, 0x44);
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_color(border)
                .border_side_color(TableBorderSide::Top, top)
                .border_side_color(TableBorderSide::InsideHorizontal, inside)
                .row([CellBuilder::text("Bordered")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_color, Some(border));
    assert_eq!(table.border_colors.top, Some(top));
    assert_eq!(table.border_colors.inside_h, Some(inside));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="single" w:sz="4" w:space="0" w:color="AA3311"/>"#)
            && document_xml
                .contains(r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="2266AA"/>"#)
            && document_xml
                .contains(r#"<w:insideH w:val="single" w:sz="4" w:space="0" w:color="118844"/>"#),
        "side-specific table border XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("side-border table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.border_colors.top, Some(top));
    assert_eq!(reopened_table.border_colors.left, Some(border));
    assert_eq!(reopened_table.border_colors.inside_h, Some(inside));
}

#[test]
fn table_builder_adds_side_specific_border_styles() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_style(TableBorderStyle::Single)
                .border_side_style(TableBorderSide::Top, TableBorderStyle::Double)
                .border_side_style(TableBorderSide::InsideVertical, TableBorderStyle::Dotted)
                .row([CellBuilder::text("Styled")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_style, Some(TableBorderStyle::Single));
    assert_eq!(table.border_styles.top, Some(TableBorderStyle::Double));
    assert_eq!(table.border_styles.inside_v, Some(TableBorderStyle::Dotted));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="double" w:sz="4" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:insideV w:val="dotted" w:sz="4" w:space="0" w:color="auto"/>"#),
        "side-specific table border style XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("side-border-style table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(
        reopened_table.border_styles.top,
        Some(TableBorderStyle::Double)
    );
    assert_eq!(
        reopened_table.border_styles.left,
        Some(TableBorderStyle::Single)
    );
    assert_eq!(
        reopened_table.border_styles.inside_v,
        Some(TableBorderStyle::Dotted)
    );
}

#[test]
fn table_builder_adds_side_specific_border_sizes() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new()
                .border_size_eighths(4)
                .border_side_size_eighths(TableBorderSide::Top, 12)
                .border_side_size_eighths(TableBorderSide::InsideHorizontal, 8)
                .row([CellBuilder::text("Sized")]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.border_size_eighths, Some(4));
    assert_eq!(table.border_sizes.top, Some(12));
    assert_eq!(table.border_sizes.inside_h, Some(8));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<w:top w:val="single" w:sz="12" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:left w:val="single" w:sz="4" w:space="0" w:color="auto"/>"#)
            && document_xml
                .contains(r#"<w:insideH w:val="single" w:sz="8" w:space="0" w:color="auto"/>"#),
        "side-specific table border size XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("side-border-size table .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.border_sizes.top, Some(12));
    assert_eq!(reopened_table.border_sizes.left, Some(4));
    assert_eq!(reopened_table.border_sizes.inside_h, Some(8));
}

#[test]
fn cell_builder_adds_cell_margins() {
    let margins = CellMargins {
        top: 120,
        right: 240,
        bottom: 360,
        left: 480,
    };
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new().row([CellBuilder::text("Padded").margins_twips(
                margins.top,
                margins.right,
                margins.bottom,
                margins.left,
            )]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected builder to add a table block");
    };
    assert_eq!(table.rows[0].cells[0].margins, Some(margins));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(
            r#"<w:tcMar><w:top w:w="120" w:type="dxa"/><w:right w:w="240" w:type="dxa"/><w:bottom w:w="360" w:type="dxa"/><w:left w:w="480" w:type="dxa"/></w:tcMar>"#
        ),
        "cell margin XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("cell margins .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened table");
    };
    assert_eq!(reopened_table.rows[0].cells[0].margins, Some(margins));
}

#[test]
fn cell_builder_adds_typed_nested_blocks() {
    let model = DocBuilder::new()
        .rich_table(
            TableBuilder::new().row([CellBuilder::new()
                .rich_paragraph(
                    ParagraphBuilder::new()
                        .runs([RunBuilder::new("Outer note").bold().build()])
                        .align(Align::Center),
                )
                .rich_table(TableBuilder::new().row([CellBuilder::text("Inner value")]))]),
        )
        .build();

    let Block::Table(table) = &model.blocks[0] else {
        panic!("expected outer table");
    };
    let outer_cell = &table.rows[0].cells[0];
    assert_eq!(outer_cell.blocks.len(), 2);
    assert!(matches!(
        &outer_cell.blocks[0],
        Block::Paragraph(paragraph)
            if paragraph.props.align == Align::Center
                && paragraph.runs[0].props.bold
                && paragraph.text() == "Outer note"
    ));
    assert!(matches!(
        &outer_cell.blocks[1],
        Block::Table(nested)
            if nested.rows.len() == 1
                && nested.rows[0].cells.len() == 1
                && nested.rows[0].cells[0].text() == "Inner value"
    ));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.matches("<w:tbl>").count() >= 2
            && document_xml.contains("Outer note")
            && document_xml.contains("Inner value"),
        "nested table XML missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("nested cell blocks .docx reopens");
    let Block::Table(reopened_table) = &reopened.model().blocks[0] else {
        panic!("expected reopened outer table");
    };
    let reopened_cell = &reopened_table.rows[0].cells[0];
    assert!(matches!(
        &reopened_cell.blocks[0],
        Block::Paragraph(paragraph)
            if paragraph.props.align == Align::Center && paragraph.text() == "Outer note"
    ));
    assert!(matches!(
        &reopened_cell.blocks[1],
        Block::Table(nested)
            if nested.rows[0].cells[0].text() == "Inner value"
    ));
}

#[test]
fn image_builder_adds_alt_text_and_size() {
    let png = tiny_png();
    let model = DocBuilder::new()
        .rich_image(
            ImageBuilder::new(png.clone(), "image/png")
                .alt("Chart <trend>")
                .size_px(200, 100),
        )
        .build();

    let Block::Image(image) = &model.blocks[0] else {
        panic!("expected image block");
    };
    assert_eq!(image.bytes.as_deref(), Some(png.as_slice()));
    assert_eq!(image.mime.as_deref(), Some("image/png"));
    assert_eq!(image.alt.as_deref(), Some("Chart <trend>"));
    assert_eq!(image.width_px, Some(200));
    assert_eq!(image.height_px, Some(100));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<wp:extent cx="1905000" cy="952500"/>"#)
            && document_xml
                .contains(r#"<wp:docPr id="1" name="Image1" descr="Chart &lt;trend&gt;"/>"#),
        "image metadata missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("image builder .docx reopens");
    let images = reopened.images();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].bytes.as_deref(), Some(png.as_slice()));
}

#[test]
fn image_builder_adds_rotation() {
    let png = tiny_png();
    let model = DocBuilder::new()
        .rich_image(
            ImageBuilder::new(png.clone(), "image/png")
                .size_px(200, 100)
                .rotate_degrees(90),
        )
        .build();

    let Block::Image(image) = &model.blocks[0] else {
        panic!("expected image block");
    };
    assert_eq!(image.rotation_degrees, Some(90));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<a:xfrm rot="5400000"><a:off x="0" y="0"/>"#),
        "image rotation missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("rotated image builder .docx reopens");
    let images = reopened.images();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].bytes.as_deref(), Some(png.as_slice()));
    assert_eq!(images[0].rotation_degrees, Some(90));
}

#[test]
fn image_builder_adds_floating_anchor_offset() {
    let png = tiny_png();
    let model = DocBuilder::new()
        .rich_image(
            ImageBuilder::new(png.clone(), "image/png")
                .size_px(200, 100)
                .alt("Float <image>")
                .floating_offset_emu(91440, 182880),
        )
        .build();

    let Block::Image(image) = &model.blocks[0] else {
        panic!("expected image block");
    };
    assert_eq!(image.floating_offset_emu, Some((91440, 182880)));

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    assert!(
        document_xml.contains(r#"<wp:anchor "#)
            && !document_xml.contains("<wp:inline")
            && document_xml.contains(
                r#"<wp:positionH relativeFrom="page"><wp:posOffset>91440</wp:posOffset></wp:positionH>"#
            )
            && document_xml.contains(
                r#"<wp:positionV relativeFrom="page"><wp:posOffset>182880</wp:posOffset></wp:positionV>"#
            )
            && document_xml.contains(r#"<wp:wrapSquare wrapText="bothSides"/>"#)
            && document_xml
                .contains(r#"<wp:docPr id="1" name="Image1" descr="Float &lt;image&gt;"/>"#),
        "floating image anchor missing: {document_xml}"
    );

    let reopened = Document::open(&bytes).expect("floating image builder .docx reopens");
    let images = reopened.images();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].bytes.as_deref(), Some(png.as_slice()));
    let shapes = reopened.floating_shapes();
    assert_eq!(shapes.len(), 1);
    assert_eq!(
        shapes[0]
            .horizontal_position
            .as_ref()
            .and_then(|pos| pos.offset_emu),
        Some(91440)
    );
    assert_eq!(
        shapes[0]
            .vertical_position
            .as_ref()
            .and_then(|pos| pos.offset_emu),
        Some(182880)
    );
}

#[test]
fn doc_builder_adds_bar_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar()
                .title("Quarterly revenue")
                .categories(["Q1", "Q2"])
                .series("Revenue", [42.0, 51.5])
                .size_px(480, 320)
                .alt("Revenue chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.title.as_deref(), Some("Quarterly revenue"));
    assert_eq!(chart.categories, ["Q1".to_string(), "Q2".to_string()]);
    assert_eq!(chart.series.len(), 1);
    assert_eq!(chart.series[0].name, "Revenue");
    assert_eq!(chart.series[0].values, [42.0, 51.5]);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:extent cx="4572000" cy="3048000"/>"#)
            && document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Revenue chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "chart rel missing: {rels}"
    );
    assert!(
        content_types.contains(
            r#"ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml""#
        ),
        "chart content type missing: {content_types}"
    );
    assert!(
        chart_xml.contains("<c:barChart>")
            && chart_xml.contains("<a:t>Quarterly revenue</a:t>")
            && chart_xml.contains("<c:v>Revenue</c:v>")
            && chart_xml.contains("<c:v>Q1</c:v>")
            && chart_xml.contains("<c:v>42</c:v>")
            && chart_xml.contains("<c:v>51.5</c:v>"),
        "chart payload missing: {chart_xml}"
    );

    let reopened = Document::open(&bytes).expect("chart builder .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_embeds_workbook_backing_chart_data() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar()
                .title("Quarterly revenue")
                .categories(["Q1", "Q2"])
                .series("Revenue", [42.0, 51.5]),
        )
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let content_types = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(
        parts
            .get("word/charts/_rels/chart1.xml.rels")
            .expect("chart relationship part")
            .clone(),
    )
    .unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let workbook_content_types =
        String::from_utf8(workbook_parts["[Content_Types].xml"].clone()).unwrap();
    let workbook_xml = String::from_utf8(workbook_parts["xl/workbook.xml"].clone()).unwrap();
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
    let shared_strings = String::from_utf8(workbook_parts["xl/sharedStrings.xml"].clone()).unwrap();

    assert!(
        content_types.contains(r#"PartName="/word/embeddings/Microsoft_Excel_Worksheet1.xlsx""#)
            && content_types.contains(
                r#"ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet""#
            ),
        "embedded workbook content type missing: {content_types}"
    );
    assert!(
        chart_xml
            .contains(r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#),
        "chart external workbook reference missing: {chart_xml}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        workbook_content_types.contains(
            r#"ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml""#
        ) && workbook_content_types.contains(
            r#"ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml""#
        ) && workbook_content_types.contains(
            r#"ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml""#
        ),
        "workbook content types missing: {workbook_content_types}"
    );
    assert!(
        workbook_xml.contains(r#"<sheet name="Chart Data" sheetId="1" r:id="rId1"/>"#),
        "workbook sheet missing: {workbook_xml}"
    );
    assert!(
        shared_strings.contains("<t>Category</t>")
            && shared_strings.contains("<t>Revenue</t>")
            && shared_strings.contains("<t>Q1</t>")
            && shared_strings.contains("<t>Q2</t>"),
        "chart labels missing from shared strings: {shared_strings}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>42</v></c>"#)
            && sheet_xml.contains(r#"<c r="B3"><v>51.5</v></c>"#),
        "chart values missing from worksheet: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("workbook-backed chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_column_and_line_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::column()
                .title("Quarterly pipeline")
                .categories(["Q1", "Q2"])
                .series("Pipeline", [10.0, 14.5])
                .size_px(420, 280)
                .alt("Pipeline column chart"),
        )
        .chart(
            ChartBuilder::line()
                .title("Retention trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Retention", [0.91, 0.93, 0.95])
                .size_px(420, 280)
                .alt("Retention line chart"),
        )
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let column_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let line_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Pipeline column chart"/>"#)
            && document_xml
                .contains(r#"<wp:docPr id="2" name="Chart2" descr="Retention line chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId2"/>"#),
        "chart drawings missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="charts/chart1.xml""#)
            && rels.contains(r#"Target="charts/chart2.xml""#),
        "chart relationships missing: {rels}"
    );
    assert!(
        column_xml.contains("<c:barChart>")
            && column_xml.contains(r#"<c:barDir val="col"/>"#)
            && column_xml.contains("<a:t>Quarterly pipeline</a:t>")
            && column_xml.contains("<c:v>Pipeline</c:v>")
            && column_xml.contains("<c:v>14.5</c:v>"),
        "column chart payload missing: {column_xml}"
    );
    assert!(
        line_xml.contains("<c:lineChart>")
            && line_xml.contains(r#"<c:marker><c:symbol val="circle"/></c:marker>"#)
            && line_xml.contains("<a:t>Retention trend</a:t>")
            && line_xml.contains("<c:v>Retention</c:v>")
            && line_xml.contains("<c:v>0.95</c:v>"),
        "line chart payload missing: {line_xml}"
    );

    let reopened = Document::open(&bytes).expect("multi-chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[test]
fn doc_builder_adds_3d_bar_and_column_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar_3d()
                .title("Regional backlog")
                .categories(["North", "South"])
                .series("Open", [18.0, 23.5])
                .series("Closed", [9.0, 12.0])
                .size_px(460, 300)
                .alt("Regional backlog 3-D bar chart"),
        )
        .chart(
            ChartBuilder::column_3d()
                .title("Quarterly pipeline")
                .categories(["Q1", "Q2", "Q3"])
                .series("Pipeline", [10.0, 14.5, 18.0])
                .size_px(460, 300)
                .alt("Quarterly pipeline 3-D column chart"),
        )
        .build();

    let Block::Chart(first) = &model.blocks[0] else {
        panic!("expected first chart block");
    };
    let Block::Chart(second) = &model.blocks[1] else {
        panic!("expected second chart block");
    };
    assert_eq!(first.kind, ChartKind::Bar3D);
    assert_eq!(second.kind, ChartKind::Column3D);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let bar_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let column_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();
    let bar_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let column_rels =
        String::from_utf8(parts["word/charts/_rels/chart2.xml.rels"].clone()).unwrap();
    let workbook1_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded 3-D bar chart workbook");
    let workbook2_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet2.xlsx")
        .expect("embedded 3-D column chart workbook");
    let workbook1_parts = unzip_parts(workbook1_bytes);
    let workbook2_parts = unzip_parts(workbook2_bytes);
    let sheet1_xml =
        String::from_utf8(workbook1_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
    let sheet2_xml =
        String::from_utf8(workbook2_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Regional backlog 3-D bar chart"/>"#)
            && document_xml.contains(
                r#"<wp:docPr id="2" name="Chart2" descr="Quarterly pipeline 3-D column chart"/>"#
            )
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId2"/>"#),
        "3-D bar/column chart drawings missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="charts/chart1.xml""#)
            && rels.contains(r#"Target="charts/chart2.xml""#),
        "3-D bar/column chart relationships missing: {rels}"
    );
    assert!(
        bar_rels.contains("relationships/package")
            && bar_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "3-D bar chart workbook relationship missing: {bar_rels}"
    );
    assert!(
        column_rels.contains("relationships/package")
            && column_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet2.xlsx""#),
        "3-D column chart workbook relationship missing: {column_rels}"
    );
    assert!(
        bar_xml.contains("<c:bar3DChart>")
            && bar_xml.contains(r#"<c:barDir val="bar"/>"#)
            && bar_xml.contains(r#"<c:grouping val="clustered"/>"#)
            && bar_xml.contains(r#"<c:shape val="box"/>"#)
            && bar_xml.contains(r#"<c:gapDepth val="150"/>"#)
            && bar_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && bar_xml.contains("<a:t>Regional backlog</a:t>")
            && bar_xml.contains("<c:v>Open</c:v>")
            && bar_xml.contains("<c:v>Closed</c:v>")
            && bar_xml.contains("<c:v>South</c:v>")
            && bar_xml.contains("<c:v>23.5</c:v>"),
        "3-D bar chart payload missing: {bar_xml}"
    );
    assert!(
        column_xml.contains("<c:bar3DChart>")
            && column_xml.contains(r#"<c:barDir val="col"/>"#)
            && column_xml.contains(r#"<c:grouping val="clustered"/>"#)
            && column_xml.contains(r#"<c:shape val="box"/>"#)
            && column_xml.contains(r#"<c:gapDepth val="150"/>"#)
            && column_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && column_xml.contains("<a:t>Quarterly pipeline</a:t>")
            && column_xml.contains("<c:v>Pipeline</c:v>")
            && column_xml.contains("<c:v>Q3</c:v>")
            && column_xml.contains("<c:v>18</c:v>"),
        "3-D column chart payload missing: {column_xml}"
    );
    assert!(
        sheet1_xml.contains(r#"<c r="B2"><v>18</v></c>"#)
            && sheet1_xml.contains(r#"<c r="C3"><v>12</v></c>"#),
        "3-D bar chart workbook values missing: {sheet1_xml}"
    );
    assert!(
        sheet2_xml.contains(r#"<c r="B2"><v>10</v></c>"#)
            && sheet2_xml.contains(r#"<c r="B4"><v>18</v></c>"#),
        "3-D column chart workbook values missing: {sheet2_xml}"
    );

    let reopened = Document::open(&bytes).expect("3-D bar/column chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[test]
fn doc_builder_adds_3d_bar_and_column_shape_styling() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar_3d()
                .shape(ChartShape::Cylinder)
                .title("Regional backlog cylinders")
                .categories(["North", "South"])
                .series("Open", [18.0, 23.5])
                .series("Closed", [9.0, 12.0])
                .size_px(460, 300)
                .alt("Regional backlog cylinder 3-D bar chart"),
        )
        .chart(
            ChartBuilder::column_3d()
                .shape(ChartShape::Pyramid)
                .title("Quarterly pipeline pyramids")
                .categories(["Q1", "Q2", "Q3"])
                .series("Pipeline", [10.0, 14.5, 18.0])
                .size_px(460, 300)
                .alt("Quarterly pipeline pyramid 3-D column chart"),
        )
        .build();

    let Block::Chart(first) = &model.blocks[0] else {
        panic!("expected first chart block");
    };
    let Block::Chart(second) = &model.blocks[1] else {
        panic!("expected second chart block");
    };
    assert_eq!(first.kind, ChartKind::Bar3D);
    assert_eq!(first.shape, ChartShape::Cylinder);
    assert_eq!(second.kind, ChartKind::Column3D);
    assert_eq!(second.shape, ChartShape::Pyramid);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let bar_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let column_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();
    let bar_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let column_rels =
        String::from_utf8(parts["word/charts/_rels/chart2.xml.rels"].clone()).unwrap();

    assert!(
        document_xml.contains(
            r#"<wp:docPr id="1" name="Chart1" descr="Regional backlog cylinder 3-D bar chart"/>"#
        ) && document_xml.contains(
            r#"<wp:docPr id="2" name="Chart2" descr="Quarterly pipeline pyramid 3-D column chart"/>"#
        ),
        "3-D shaped bar/column chart drawings missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="charts/chart1.xml""#)
            && rels.contains(r#"Target="charts/chart2.xml""#),
        "3-D shaped bar/column chart relationships missing: {rels}"
    );
    assert!(
        bar_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#)
            && column_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet2.xlsx""#),
        "3-D shaped bar/column workbook relationships missing: {bar_rels} {column_rels}"
    );
    assert!(
        bar_xml.contains("<c:bar3DChart>")
            && bar_xml.contains(r#"<c:barDir val="bar"/>"#)
            && bar_xml.contains(r#"<c:shape val="cylinder"/>"#)
            && bar_xml.contains("<a:t>Regional backlog cylinders</a:t>")
            && bar_xml.contains("<c:v>Closed</c:v>")
            && bar_xml.contains("<c:v>23.5</c:v>"),
        "3-D cylinder bar chart payload missing: {bar_xml}"
    );
    assert!(
        column_xml.contains("<c:bar3DChart>")
            && column_xml.contains(r#"<c:barDir val="col"/>"#)
            && column_xml.contains(r#"<c:shape val="pyramid"/>"#)
            && column_xml.contains("<a:t>Quarterly pipeline pyramids</a:t>")
            && column_xml.contains("<c:v>Q3</c:v>")
            && column_xml.contains("<c:v>18</c:v>"),
        "3-D pyramid column chart payload missing: {column_xml}"
    );

    let reopened = Document::open(&bytes).expect("3-D shaped bar/column chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[test]
fn doc_builder_adds_area_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::area()
                .title("Adoption trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Free", [20.0, 28.0, 33.0])
                .series("Paid", [8.0, 13.5, 21.0])
                .size_px(420, 280)
                .alt("Adoption area chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Area);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded area chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Adoption area chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "area chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "area chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "area chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:areaChart>")
            && chart_xml.contains(r#"<c:grouping val="standard"/>"#)
            && chart_xml.contains(r#"<c:varyColors val="0"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Adoption trend</a:t>")
            && chart_xml.contains("<c:v>Free</c:v>")
            && chart_xml.contains("<c:v>Paid</c:v>")
            && chart_xml.contains("<c:v>Mar</c:v>")
            && chart_xml.contains("<c:v>21</c:v>"),
        "area chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>20</v></c>"#)
            && sheet_xml.contains(r#"<c r="C3"><v>13.5</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>21</v></c>"#),
        "area chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("area chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_3d_line_and_area_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::line_3d()
                .title("Retention trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Current", [0.91, 0.93, 0.95])
                .series("Target", [0.94, 0.95, 0.97])
                .size_px(460, 300)
                .alt("Retention trend 3-D line chart"),
        )
        .chart(
            ChartBuilder::area_3d()
                .title("Adoption trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Free", [20.0, 28.0, 33.0])
                .series("Paid", [8.0, 13.5, 21.0])
                .size_px(460, 300)
                .alt("Adoption trend 3-D area chart"),
        )
        .build();

    let Block::Chart(first) = &model.blocks[0] else {
        panic!("expected first chart block");
    };
    let Block::Chart(second) = &model.blocks[1] else {
        panic!("expected second chart block");
    };
    assert_eq!(first.kind, ChartKind::Line3D);
    assert_eq!(second.kind, ChartKind::Area3D);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let line_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let area_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();
    let line_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let area_rels = String::from_utf8(parts["word/charts/_rels/chart2.xml.rels"].clone()).unwrap();
    let workbook1_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded 3-D line chart workbook");
    let workbook2_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet2.xlsx")
        .expect("embedded 3-D area chart workbook");
    let workbook1_parts = unzip_parts(workbook1_bytes);
    let workbook2_parts = unzip_parts(workbook2_bytes);
    let sheet1_xml =
        String::from_utf8(workbook1_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
    let sheet2_xml =
        String::from_utf8(workbook2_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Retention trend 3-D line chart"/>"#)
            && document_xml.contains(
                r#"<wp:docPr id="2" name="Chart2" descr="Adoption trend 3-D area chart"/>"#
            )
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId2"/>"#),
        "3-D line/area chart drawings missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="charts/chart1.xml""#)
            && rels.contains(r#"Target="charts/chart2.xml""#),
        "3-D line/area chart relationships missing: {rels}"
    );
    assert!(
        line_rels.contains("relationships/package")
            && line_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "3-D line chart workbook relationship missing: {line_rels}"
    );
    assert!(
        area_rels.contains("relationships/package")
            && area_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet2.xlsx""#),
        "3-D area chart workbook relationship missing: {area_rels}"
    );
    assert!(
        line_xml.contains("<c:line3DChart>")
            && line_xml.contains(r#"<c:grouping val="standard"/>"#)
            && line_xml.contains(r#"<c:varyColors val="0"/>"#)
            && line_xml.contains(r#"<c:marker><c:symbol val="circle"/></c:marker>"#)
            && line_xml.contains(r#"<c:gapDepth val="150"/>"#)
            && line_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && line_xml.contains("<a:t>Retention trend</a:t>")
            && line_xml.contains("<c:v>Current</c:v>")
            && line_xml.contains("<c:v>Target</c:v>")
            && line_xml.contains("<c:v>Mar</c:v>")
            && line_xml.contains("<c:v>0.97</c:v>"),
        "3-D line chart payload missing: {line_xml}"
    );
    assert!(
        area_xml.contains("<c:area3DChart>")
            && area_xml.contains(r#"<c:grouping val="standard"/>"#)
            && area_xml.contains(r#"<c:varyColors val="0"/>"#)
            && area_xml.contains(r#"<c:gapDepth val="150"/>"#)
            && area_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && area_xml.contains("<a:t>Adoption trend</a:t>")
            && area_xml.contains("<c:v>Free</c:v>")
            && area_xml.contains("<c:v>Paid</c:v>")
            && area_xml.contains("<c:v>Mar</c:v>")
            && area_xml.contains("<c:v>21</c:v>"),
        "3-D area chart payload missing: {area_xml}"
    );
    assert!(
        sheet1_xml.contains(r#"<c r="B2"><v>0.91</v></c>"#)
            && sheet1_xml.contains(r#"<c r="C4"><v>0.97</v></c>"#),
        "3-D line chart workbook values missing: {sheet1_xml}"
    );
    assert!(
        sheet2_xml.contains(r#"<c r="B2"><v>20</v></c>"#)
            && sheet2_xml.contains(r#"<c r="C4"><v>21</v></c>"#),
        "3-D area chart workbook values missing: {sheet2_xml}"
    );

    let reopened = Document::open(&bytes).expect("3-D line/area chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[test]
fn doc_builder_adds_radar_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::radar()
                .title("Capability profile")
                .categories(["Speed", "Quality", "Cost", "Reach"])
                .series("Current", [4.0, 3.0, 2.0, 5.0])
                .series("Target", [5.0, 4.0, 4.0, 5.0])
                .size_px(420, 320)
                .alt("Capability radar chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Radar);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded radar chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Capability radar chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "radar chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "radar chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "radar chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:radarChart>")
            && chart_xml.contains(r#"<c:radarStyle val="standard"/>"#)
            && chart_xml.contains(r#"<c:varyColors val="0"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Capability profile</a:t>")
            && chart_xml.contains("<c:v>Current</c:v>")
            && chart_xml.contains("<c:v>Target</c:v>")
            && chart_xml.contains("<c:v>Speed</c:v>")
            && chart_xml.contains("<c:v>5</c:v>"),
        "radar chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>4</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>4</v></c>"#)
            && sheet_xml.contains(r#"<c r="C5"><v>5</v></c>"#),
        "radar chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("radar chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_scatter_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::scatter()
                .title("Latency distribution")
                .categories(["P50", "P90", "P99"])
                .series("Before", [120.0, 240.0, 510.0])
                .series("After", [90.0, 180.0, 360.0])
                .size_px(420, 280)
                .alt("Latency scatter chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Scatter);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded scatter chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Latency scatter chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "scatter chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "scatter chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "scatter chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:scatterChart>")
            && chart_xml.contains(r#"<c:scatterStyle val="lineMarker"/>"#)
            && chart_xml.contains(r#"<c:varyColors val="0"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Latency distribution</a:t>")
            && chart_xml.contains("<c:v>Before</c:v>")
            && chart_xml.contains("<c:v>After</c:v>")
            && chart_xml.contains("<c:xVal>")
            && chart_xml.contains("<c:yVal>")
            && chart_xml.contains(r#"<c:pt idx="0"><c:v>1</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="2"><c:v>3</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="0"><c:v>120</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="2"><c:v>360</c:v></c:pt>"#),
        "scatter chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>120</v></c>"#)
            && sheet_xml.contains(r#"<c r="C3"><v>180</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>360</v></c>"#),
        "scatter chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("scatter chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_bubble_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bubble()
                .title("Opportunity size")
                .categories(["Segment A", "Segment B", "Segment C"])
                .bubble_series("Pipeline", [12.0, 24.0, 36.0], [5.0, 12.5, 21.0])
                .size_px(420, 280)
                .alt("Opportunity bubble chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Bubble);
    assert_eq!(chart.series[0].values, [12.0, 24.0, 36.0]);
    assert_eq!(chart.series[0].bubble_sizes, [5.0, 12.5, 21.0]);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded bubble chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
    let shared_strings = String::from_utf8(workbook_parts["xl/sharedStrings.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Opportunity bubble chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "bubble chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "bubble chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "bubble chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:bubbleChart>")
            && chart_xml.contains(r#"<c:varyColors val="0"/>"#)
            && chart_xml.contains(r#"<c:bubbleScale val="100"/>"#)
            && chart_xml.contains(r#"<c:showNegBubbles val="0"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Opportunity size</a:t>")
            && chart_xml.contains("<c:v>Pipeline</c:v>")
            && chart_xml.contains("<c:xVal>")
            && chart_xml.contains("<c:yVal>")
            && chart_xml.contains("<c:bubbleSize>")
            && chart_xml.contains(r#"<c:pt idx="0"><c:v>1</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="2"><c:v>3</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="0"><c:v>12</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="2"><c:v>36</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="0"><c:v>5</c:v></c:pt>"#)
            && chart_xml.contains(r#"<c:pt idx="2"><c:v>21</c:v></c:pt>"#),
        "bubble chart payload missing: {chart_xml}"
    );
    assert!(
        shared_strings.contains("<t>Pipeline size</t>")
            && sheet_xml.contains(r#"<c r="B2"><v>12</v></c>"#)
            && sheet_xml.contains(r#"<c r="C2"><v>5</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>21</v></c>"#),
        "bubble chart workbook values missing: {sheet_xml} / {shared_strings}"
    );

    let reopened = Document::open(&bytes).expect("bubble chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_pie_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0])
                .size_px(420, 280)
                .alt("Revenue mix pie chart"),
        )
        .build();

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Revenue mix pie chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "pie chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "pie chart relationship missing: {rels}"
    );
    assert!(
        chart_xml.contains("<c:pieChart>")
            && chart_xml.contains(r#"<c:varyColors val="1"/>"#)
            && chart_xml.contains(r#"<c:firstSliceAng val="0"/>"#)
            && chart_xml.contains("<a:t>Revenue mix</a:t>")
            && chart_xml.contains("<c:v>Share</c:v>")
            && chart_xml.contains("<c:v>Cloud</c:v>")
            && chart_xml.contains("<c:v>65</c:v>"),
        "pie chart payload missing: {chart_xml}"
    );

    let reopened = Document::open(&bytes).expect("pie chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_3d_pie_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie_3d()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0])
                .size_px(420, 280)
                .alt("Revenue mix 3-D pie chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Pie3D);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded 3-D pie chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Revenue mix 3-D pie chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "3-D pie chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "3-D pie chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "3-D pie chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:pie3DChart>")
            && chart_xml.contains(r#"<c:varyColors val="1"/>"#)
            && chart_xml.contains(r#"<c:firstSliceAng val="0"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Revenue mix</a:t>")
            && chart_xml.contains("<c:v>Share</c:v>")
            && chart_xml.contains("<c:v>Cloud</c:v>")
            && chart_xml.contains("<c:v>65</c:v>"),
        "3-D pie chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>65</v></c>"#)
            && sheet_xml.contains(r#"<c r="B3"><v>25</v></c>"#)
            && sheet_xml.contains(r#"<c r="B4"><v>10</v></c>"#),
        "3-D pie chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("3-D pie chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_doughnut_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::doughnut()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0])
                .size_px(420, 280)
                .alt("Revenue mix doughnut chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Doughnut);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded doughnut chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Revenue mix doughnut chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "doughnut chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "doughnut chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "doughnut chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:doughnutChart>")
            && chart_xml.contains(r#"<c:varyColors val="1"/>"#)
            && chart_xml.contains(r#"<c:firstSliceAng val="0"/>"#)
            && chart_xml.contains(r#"<c:holeSize val="50"/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Revenue mix</a:t>")
            && chart_xml.contains("<c:v>Share</c:v>")
            && chart_xml.contains("<c:v>Cloud</c:v>")
            && chart_xml.contains("<c:v>65</c:v>"),
        "doughnut chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>65</v></c>"#)
            && sheet_xml.contains(r#"<c r="B3"><v>25</v></c>"#)
            && sheet_xml.contains(r#"<c r="B4"><v>10</v></c>"#),
        "doughnut chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("doughnut chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_surface_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface()
                .title("Risk surface")
                .categories(["Low", "Medium", "High"])
                .series("Exposure", [1.0, 3.5, 6.0])
                .series("Control", [0.5, 2.0, 4.5])
                .size_px(460, 300)
                .alt("Risk surface chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Surface);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded surface chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml.contains(r#"<wp:docPr id="1" name="Chart1" descr="Risk surface chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "surface chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "surface chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "surface chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:surfaceChart>")
            && chart_xml.contains(r#"<c:wireframe val="0"/>"#)
            && chart_xml.contains(r#"<c:bandFmts/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Risk surface</a:t>")
            && chart_xml.contains("<c:v>Exposure</c:v>")
            && chart_xml.contains("<c:v>High</c:v>")
            && chart_xml.contains("<c:v>6</c:v>"),
        "surface chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>1</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>4.5</v></c>"#),
        "surface chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("surface chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_3d_surface_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface_3d()
                .title("Terrain surface")
                .categories(["North", "Center", "South"])
                .series("Elevation", [8.0, 4.5, 6.0])
                .series("Slope", [2.0, 3.5, 5.0])
                .size_px(460, 300)
                .alt("Terrain 3-D surface chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Surface3D);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded 3-D surface chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Terrain 3-D surface chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "3-D surface chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "3-D surface chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "3-D surface chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:surface3DChart>")
            && chart_xml.contains(r#"<c:wireframe val="0"/>"#)
            && chart_xml.contains(r#"<c:bandFmts/>"#)
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Terrain surface</a:t>")
            && chart_xml.contains("<c:v>Elevation</c:v>")
            && chart_xml.contains("<c:v>South</c:v>")
            && chart_xml.contains("<c:v>6</c:v>"),
        "3-D surface chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>8</v></c>"#)
            && sheet_xml.contains(r#"<c r="C4"><v>5</v></c>"#),
        "3-D surface chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("3-D surface chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_wireframe_surface_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface()
                .wireframe()
                .title("Risk wireframe")
                .categories(["Low", "Medium", "High"])
                .series("Exposure", [1.0, 3.5, 6.0])
                .series("Control", [0.5, 2.0, 4.5])
                .size_px(460, 300)
                .alt("Risk wireframe surface chart"),
        )
        .chart(
            ChartBuilder::surface_3d()
                .wireframe()
                .title("Terrain wireframe")
                .categories(["North", "Center", "South"])
                .series("Elevation", [8.0, 4.5, 6.0])
                .series("Slope", [2.0, 3.5, 5.0])
                .size_px(460, 300)
                .alt("Terrain wireframe 3-D surface chart"),
        )
        .build();

    let Block::Chart(first) = &model.blocks[0] else {
        panic!("expected first chart block");
    };
    let Block::Chart(second) = &model.blocks[1] else {
        panic!("expected second chart block");
    };
    assert_eq!(first.kind, ChartKind::Surface);
    assert!(first.wireframe);
    assert_eq!(second.kind, ChartKind::Surface3D);
    assert!(second.wireframe);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let surface_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let surface_3d_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();
    let surface_rels =
        String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let surface_3d_rels =
        String::from_utf8(parts["word/charts/_rels/chart2.xml.rels"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Risk wireframe surface chart"/>"#)
            && document_xml.contains(
                r#"<wp:docPr id="2" name="Chart2" descr="Terrain wireframe 3-D surface chart"/>"#
            ),
        "wireframe surface chart drawings missing: {document_xml}"
    );
    assert!(
        rels.contains(r#"Target="charts/chart1.xml""#)
            && rels.contains(r#"Target="charts/chart2.xml""#),
        "wireframe surface chart relationships missing: {rels}"
    );
    assert!(
        surface_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#)
            && surface_3d_rels
                .contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet2.xlsx""#),
        "wireframe surface chart workbook relationships missing: {surface_rels} {surface_3d_rels}"
    );
    assert!(
        surface_xml.contains("<c:surfaceChart>")
            && surface_xml.contains(r#"<c:wireframe val="1"/>"#)
            && surface_xml.contains("<a:t>Risk wireframe</a:t>")
            && surface_xml.contains("<c:v>Control</c:v>")
            && surface_xml.contains("<c:v>4.5</c:v>"),
        "wireframe surface chart payload missing: {surface_xml}"
    );
    assert!(
        surface_3d_xml.contains("<c:surface3DChart>")
            && surface_3d_xml.contains(r#"<c:wireframe val="1"/>"#)
            && surface_3d_xml.contains("<a:t>Terrain wireframe</a:t>")
            && surface_3d_xml.contains("<c:v>Slope</c:v>")
            && surface_3d_xml.contains("<c:v>5</c:v>"),
        "wireframe 3-D surface chart payload missing: {surface_3d_xml}"
    );

    let reopened = Document::open(&bytes).expect("wireframe surface charts .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[test]
fn doc_builder_adds_stock_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::stock()
                .title("Share price")
                .categories(["Mon", "Tue", "Wed"])
                .series("Open", [10.0, 11.0, 12.5])
                .series("High", [12.0, 13.5, 14.0])
                .series("Low", [9.5, 10.5, 11.0])
                .series("Close", [11.0, 12.8, 13.2])
                .size_px(460, 300)
                .alt("Share price stock chart"),
        )
        .build();

    let Block::Chart(chart) = &model.blocks[0] else {
        panic!("expected chart block");
    };
    assert_eq!(chart.kind, ChartKind::Stock);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let chart_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart_rels = String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let workbook_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded stock chart workbook");
    let workbook_parts = unzip_parts(workbook_bytes);
    let sheet_xml = String::from_utf8(workbook_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Share price stock chart"/>"#)
            && document_xml.contains(r#"<c:chart r:id="rId1"/>"#),
        "stock chart drawing missing: {document_xml}"
    );
    assert!(
        rels.contains("relationships/chart") && rels.contains(r#"Target="charts/chart1.xml""#),
        "stock chart relationship missing: {rels}"
    );
    assert!(
        chart_rels.contains("relationships/package")
            && chart_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#),
        "stock chart workbook relationship missing: {chart_rels}"
    );
    assert!(
        chart_xml.contains("<c:stockChart>")
            && chart_xml.contains("<c:hiLowLines/>")
            && chart_xml.contains("<c:upDownBars>")
            && chart_xml.contains(
                r#"<c:externalData r:id="rId1"><c:autoUpdate val="0"/></c:externalData>"#
            )
            && chart_xml.contains("<a:t>Share price</a:t>")
            && chart_xml.contains("<c:v>Open</c:v>")
            && chart_xml.contains("<c:v>Close</c:v>")
            && chart_xml.contains("<c:v>13.2</c:v>"),
        "stock chart payload missing: {chart_xml}"
    );
    assert!(
        sheet_xml.contains(r#"<c r="B2"><v>10</v></c>"#)
            && sheet_xml.contains(r#"<c r="E4"><v>13.2</v></c>"#),
        "stock chart workbook values missing: {sheet_xml}"
    );

    let reopened = Document::open(&bytes).expect("stock chart .docx reopens");
    assert_eq!(reopened.report().features.charts, 1);
}

#[test]
fn doc_builder_adds_pie_of_pie_and_bar_of_pie_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie_of_pie()
                .title("Pipeline mix")
                .categories(["Core", "Expansion", "Services", "Other"])
                .series("Share", [55.0, 24.0, 14.0, 7.0])
                .size_px(420, 280)
                .alt("Pipeline pie-of-pie chart"),
        )
        .chart(
            ChartBuilder::bar_of_pie()
                .title("Support split")
                .categories(["Priority", "Standard", "Deferred", "Closed"])
                .series("Tickets", [12.0, 32.0, 7.0, 18.0])
                .size_px(420, 280)
                .alt("Support bar-of-pie chart"),
        )
        .build();

    let Block::Chart(first) = &model.blocks[0] else {
        panic!("expected first chart block");
    };
    let Block::Chart(second) = &model.blocks[1] else {
        panic!("expected second chart block");
    };
    assert_eq!(first.kind, ChartKind::PieOfPie);
    assert_eq!(second.kind, ChartKind::BarOfPie);

    let bytes = rdoc::write_docx(&model);
    let parts = unzip_parts(&bytes);
    let document_xml = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let chart1_xml = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let chart2_xml = String::from_utf8(parts["word/charts/chart2.xml"].clone()).unwrap();
    let chart1_rels =
        String::from_utf8(parts["word/charts/_rels/chart1.xml.rels"].clone()).unwrap();
    let chart2_rels =
        String::from_utf8(parts["word/charts/_rels/chart2.xml.rels"].clone()).unwrap();
    let workbook1_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet1.xlsx")
        .expect("embedded pie-of-pie chart workbook");
    let workbook2_bytes = parts
        .get("word/embeddings/Microsoft_Excel_Worksheet2.xlsx")
        .expect("embedded bar-of-pie chart workbook");
    let workbook1_parts = unzip_parts(workbook1_bytes);
    let workbook2_parts = unzip_parts(workbook2_bytes);
    let sheet1_xml =
        String::from_utf8(workbook1_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();
    let sheet2_xml =
        String::from_utf8(workbook2_parts["xl/worksheets/sheet1.xml"].clone()).unwrap();

    assert!(
        document_xml
            .contains(r#"<wp:docPr id="1" name="Chart1" descr="Pipeline pie-of-pie chart"/>"#)
            && document_xml
                .contains(r#"<wp:docPr id="2" name="Chart2" descr="Support bar-of-pie chart"/>"#),
        "of-pie chart drawings missing: {document_xml}"
    );
    assert!(
        chart1_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet1.xlsx""#)
            && chart2_rels.contains(r#"Target="../embeddings/Microsoft_Excel_Worksheet2.xlsx""#),
        "of-pie chart workbook relationships missing: {chart1_rels} / {chart2_rels}"
    );
    assert!(
        chart1_xml.contains("<c:ofPieChart>")
            && chart1_xml.contains(r#"<c:ofPieType val="pie"/>"#)
            && chart1_xml.contains(r#"<c:secondPieSize val="75"/>"#)
            && chart1_xml.contains("<c:serLines/>")
            && chart1_xml.contains("<a:t>Pipeline mix</a:t>")
            && chart1_xml.contains("<c:v>Share</c:v>")
            && chart1_xml.contains("<c:v>Other</c:v>")
            && chart1_xml.contains("<c:v>7</c:v>"),
        "pie-of-pie chart payload missing: {chart1_xml}"
    );
    assert!(
        chart2_xml.contains("<c:ofPieChart>")
            && chart2_xml.contains(r#"<c:ofPieType val="bar"/>"#)
            && chart2_xml.contains(r#"<c:secondPieSize val="75"/>"#)
            && chart2_xml.contains("<c:serLines/>")
            && chart2_xml.contains("<a:t>Support split</a:t>")
            && chart2_xml.contains("<c:v>Tickets</c:v>")
            && chart2_xml.contains("<c:v>Closed</c:v>")
            && chart2_xml.contains("<c:v>18</c:v>"),
        "bar-of-pie chart payload missing: {chart2_xml}"
    );
    assert!(
        sheet1_xml.contains(r#"<c r="B5"><v>7</v></c>"#)
            && sheet2_xml.contains(r#"<c r="B5"><v>18</v></c>"#),
        "of-pie workbook values missing: {sheet1_xml} / {sheet2_xml}"
    );

    let reopened = Document::open(&bytes).expect("of-pie charts .docx reopens");
    assert_eq!(reopened.report().features.charts, 2);
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_bar_chart_without_unsupported_chart_warning() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar()
                .title("Quarterly revenue")
                .categories(["Q1", "Q2", "Q3"])
                .series("Revenue", [42.0, 51.5, 63.0])
                .size_px(480, 320)
                .alt("Revenue chart"),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored chart should be rendered by the model renderer, not reported as preserved-only"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_column_and_line_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::column()
                .title("Quarterly pipeline")
                .categories(["Q1", "Q2"])
                .series("Pipeline", [10.0, 14.5]),
        )
        .chart(
            ChartBuilder::line()
                .title("Retention trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Retention", [0.91, 0.93, 0.95]),
        )
        .build();

    let bar = rdoc::render_pdf_with_report(
        &DocBuilder::new()
            .chart(
                ChartBuilder::bar()
                    .categories(["Q1", "Q2"])
                    .series("Pipeline", [10.0, 14.5]),
            )
            .build(),
    );
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored column/line charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > bar.pdf.len() + 100,
        "two rendered chart families should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_area_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::area()
                .title("Adoption trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Free", [20.0, 28.0, 33.0])
                .series("Paid", [8.0, 13.5, 21.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored area chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "area chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_radar_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::radar()
                .title("Capability profile")
                .categories(["Speed", "Quality", "Cost", "Reach"])
                .series("Current", [4.0, 3.0, 2.0, 5.0])
                .series("Target", [5.0, 4.0, 4.0, 5.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored radar chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "radar chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_scatter_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::scatter()
                .title("Latency distribution")
                .categories(["P50", "P90", "P99"])
                .series("Before", [120.0, 240.0, 510.0])
                .series("After", [90.0, 180.0, 360.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored scatter chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "scatter chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_bubble_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bubble()
                .title("Opportunity size")
                .categories(["Segment A", "Segment B", "Segment C"])
                .bubble_series("Pipeline", [12.0, 24.0, 36.0], [5.0, 12.5, 21.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored bubble chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "bubble chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_pie_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored pie chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "pie chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_3d_pie_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie_3d()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored 3-D pie chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "3-D pie chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_3d_bar_and_column_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar_3d()
                .title("Regional backlog")
                .categories(["North", "South"])
                .series("Open", [18.0, 23.5])
                .series("Closed", [9.0, 12.0]),
        )
        .chart(
            ChartBuilder::column_3d()
                .title("Quarterly pipeline")
                .categories(["Q1", "Q2", "Q3"])
                .series("Pipeline", [10.0, 14.5, 18.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored 3-D bar/column charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "3-D bar/column chart drawings should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_3d_bar_and_column_shape_styling() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::bar_3d()
                .shape(ChartShape::Cylinder)
                .title("Regional backlog cylinders")
                .categories(["North", "South"])
                .series("Open", [18.0, 23.5])
                .series("Closed", [9.0, 12.0]),
        )
        .chart(
            ChartBuilder::column_3d()
                .shape(ChartShape::Pyramid)
                .title("Quarterly pipeline pyramids")
                .categories(["Q1", "Q2", "Q3"])
                .series("Pipeline", [10.0, 14.5, 18.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored shaped 3-D bar/column charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "shaped 3-D bar/column chart drawings should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_3d_line_and_area_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::line_3d()
                .title("Retention trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Current", [0.91, 0.93, 0.95])
                .series("Target", [0.94, 0.95, 0.97]),
        )
        .chart(
            ChartBuilder::area_3d()
                .title("Adoption trend")
                .categories(["Jan", "Feb", "Mar"])
                .series("Free", [20.0, 28.0, 33.0])
                .series("Paid", [8.0, 13.5, 21.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored 3-D line/area charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "3-D line/area chart drawings should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_doughnut_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::doughnut()
                .title("Revenue mix")
                .categories(["Cloud", "Services", "Support"])
                .series("Share", [65.0, 25.0, 10.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored doughnut chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "doughnut chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_surface_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface()
                .title("Risk surface")
                .categories(["Low", "Medium", "High"])
                .series("Exposure", [1.0, 3.5, 6.0])
                .series("Control", [0.5, 2.0, 4.5]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored surface chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "surface chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_3d_surface_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface_3d()
                .title("Terrain surface")
                .categories(["North", "Center", "South"])
                .series("Elevation", [8.0, 4.5, 6.0])
                .series("Slope", [2.0, 3.5, 5.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored 3-D surface chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "3-D surface chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_wireframe_surface_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::surface()
                .wireframe()
                .title("Risk wireframe")
                .categories(["Low", "Medium", "High"])
                .series("Exposure", [1.0, 3.5, 6.0])
                .series("Control", [0.5, 2.0, 4.5]),
        )
        .chart(
            ChartBuilder::surface_3d()
                .wireframe()
                .title("Terrain wireframe")
                .categories(["North", "Center", "South"])
                .series("Elevation", [8.0, 4.5, 6.0])
                .series("Slope", [2.0, 3.5, 5.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored wireframe surface charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "wireframe surface chart drawings should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_stock_chart() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::stock()
                .title("Share price")
                .categories(["Mon", "Tue", "Wed"])
                .series("Open", [10.0, 11.0, 12.5])
                .series("High", [12.0, 13.5, 14.0])
                .series("Low", [9.5, 10.5, 11.0])
                .series("Close", [11.0, 12.8, 13.2]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored stock chart should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "stock chart drawing should add visible PDF content"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_draws_authored_of_pie_charts() {
    let model = DocBuilder::new()
        .chart(
            ChartBuilder::pie_of_pie()
                .title("Pipeline mix")
                .categories(["Core", "Expansion", "Services", "Other"])
                .series("Share", [55.0, 24.0, 14.0, 7.0]),
        )
        .chart(
            ChartBuilder::bar_of_pie()
                .title("Support split")
                .categories(["Priority", "Standard", "Deferred", "Closed"])
                .series("Tickets", [12.0, 32.0, 7.0, 18.0]),
        )
        .build();

    let empty = rdoc::render_pdf_with_report(&DocModel::default());
    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 1);
    assert_eq!(rendered.report.unsupported.charts, 0);
    assert!(
        !rendered.report.warnings.iter().any(|warning| matches!(
            warning,
            rdoc::RenderWarning::ChartsPreservedButNotModeled { .. }
        )),
        "authored of-pie charts should render without preserved-only warnings"
    );
    assert!(
        rendered.pdf.len() > empty.pdf.len() + 100,
        "of-pie chart drawing should add visible PDF content"
    );
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
    let try_a = rdoc::try_render_pdf(&model).expect("fallible render succeeds");
    assert!(try_a.starts_with(b"%PDF") && try_a.len() > 800);
    let try_b =
        rdoc::try_render_pdf_with_fonts(&model, &[]).expect("fallible font render succeeds");
    assert!(try_b.starts_with(b"%PDF"));

    let doc = Document::open(&rdoc::write_docx(&model)).expect("authored docx reopens");
    assert!(doc
        .try_to_pdf()
        .expect("document render succeeds")
        .starts_with(b"%PDF"));
    assert!(doc.to_pdf_with_fonts(&[]).starts_with(b"%PDF"));
    assert!(doc
        .try_to_pdf_with_fonts(&[])
        .expect("document font render succeeds")
        .starts_with(b"%PDF"));
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_honors_paragraph_page_break_before() {
    let model = DocBuilder::new()
        .paragraph("First page")
        .rich_paragraph(ParagraphBuilder::text("Second page").page_break_before())
        .build();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 2);
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_report_treats_page_and_filename_fields_as_supported() {
    let model = DocBuilder::new()
        .field("PAGE", "1")
        .field("FILENAME \\p", "report.docx")
        .field("TOC \\o \"1-3\"", "Contents")
        .build();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.unsupported.fields, 1);
    assert_eq!(rendered.report.unsupported.field_kinds.len(), 1);
    assert_eq!(
        rendered.report.unsupported.field_kinds[0].kind,
        FieldKind::Toc
    );
    assert!(rendered.report.warnings.iter().any(|warning| matches!(
        warning,
        rdoc::RenderWarning::UnsupportedFieldEvaluation { count: 1, field_kinds }
            if field_kinds.len() == 1 && field_kinds[0].kind == FieldKind::Toc
    )));
    let json = rendered.report.to_json();
    assert!(json.contains(r#""pages":1"#), "{json}");
    assert!(json.contains(r#""unsupported":{"#), "{json}");
    assert!(!json.contains(r#""kind":"FILENAME""#), "{json}");
    assert!(
        json.contains(r#""kind":"UnsupportedFieldEvaluation""#),
        "{json}"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_report_treats_page_ref_as_named_unsupported_field() {
    let model = DocBuilder::new().field("PAGEREF Figure1 \\h", "3").build();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.unsupported.fields, 1);
    assert_eq!(
        rendered.report.unsupported.field_kinds,
        vec![rdoc::FieldKindCount {
            kind: FieldKind::PageRef,
            count: 1,
        }]
    );
    assert!(rendered.report.warnings.iter().any(|warning| matches!(
        warning,
        rdoc::RenderWarning::UnsupportedFieldEvaluation { count: 1, field_kinds }
            if field_kinds.len() == 1 && field_kinds[0].kind == FieldKind::PageRef
    )));
    let json = rendered.report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGEREF","count":1}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"PAGEREF","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pdf_report_exposes_pages_and_warnings() {
    let model = DocBuilder::new()
        .paragraph("First page")
        .page_break()
        .paragraph("Second page")
        .field("TOC \\o \"1-3\"", "Contents")
        .build();

    let rendered = rdoc::render_pdf_with_report(&model);
    assert!(rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered.report.pages, 2);
    assert_eq!(rendered.report.unsupported.fields, 1);
    assert!(rendered.report.warnings.iter().any(|warning| matches!(
        warning,
        rdoc::RenderWarning::UnsupportedFieldEvaluation { count: 1, .. }
    )));
    let try_rendered = rdoc::try_render_pdf_with_report(&model).expect("report render succeeds");
    assert!(try_rendered.pdf.starts_with(b"%PDF"));
    assert_eq!(try_rendered.report.pages, 2);
    let try_rendered_fonts =
        rdoc::try_render_pdf_with_fonts_and_report(&model, &[]).expect("font report succeeds");
    assert!(try_rendered_fonts.pdf.starts_with(b"%PDF"));
    assert_eq!(try_rendered_fonts.report.pages, 2);

    let doc = Document::open(&rdoc::write_docx(&model)).expect("authored field docx reopens");
    let rendered_doc = doc.to_pdf_with_report();
    assert!(rendered_doc.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered_doc.report.pages, 2);
    assert_eq!(rendered_doc.report.unsupported.fields, 1);
    let try_rendered_doc = doc
        .try_to_pdf_with_report()
        .expect("document report render succeeds");
    assert!(try_rendered_doc.pdf.starts_with(b"%PDF"));
    assert_eq!(try_rendered_doc.report.pages, 2);
    assert_eq!(try_rendered_doc.report.unsupported.fields, 1);

    let rendered_doc_fonts = doc.to_pdf_with_fonts_and_report(&[]);
    assert!(rendered_doc_fonts.pdf.starts_with(b"%PDF"));
    assert_eq!(rendered_doc_fonts.report.pages, 2);
    assert_eq!(rendered_doc_fonts.report.unsupported.fields, 1);
    let try_rendered_doc_fonts = doc
        .try_to_pdf_with_fonts_and_report(&[])
        .expect("document font report render succeeds");
    assert!(try_rendered_doc_fonts.pdf.starts_with(b"%PDF"));
    assert_eq!(try_rendered_doc_fonts.report.pages, 2);
    assert_eq!(try_rendered_doc_fonts.report.unsupported.fields, 1);
}
