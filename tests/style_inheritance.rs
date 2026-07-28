#![cfg(feature = "docx")]

use std::io::Write;

use rwml::{Block, Color, Document};

fn docx_fixture(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let opt = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            zip.start_file(*name, opt).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

fn content_types(include_styles: bool) -> String {
    let styles = if include_styles {
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>{styles}</Types>"#
    )
}

fn document_rels(include_styles: bool) -> &'static str {
    if include_styles {
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
    } else {
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#
    }
}

fn style_inheritance_docx() -> Vec<u8> {
    let content_types = content_types(true);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:docDefaults>
                    <w:rPrDefault><w:rPr><w:b w:val="0"/><w:sz w:val="20"/></w:rPr></w:rPrDefault>
                </w:docDefaults>
                <w:style w:type="paragraph" w:styleId="Normal">
                    <w:name w:val="Normal"/>
                    <w:rPr><w:color w:val="336699"/></w:rPr>
                </w:style>
                <w:style w:type="paragraph" w:styleId="Heading1">
                    <w:name w:val="heading 1"/>
                    <w:basedOn w:val="Normal"/>
                    <w:rPr><w:b/></w:rPr>
                </w:style>
                <w:style w:type="character" w:styleId="Em">
                    <w:name w:val="Em"/>
                    <w:rPr><w:i/></w:rPr>
                </w:style>
            </w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p>
                    <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                    <w:r><w:t>plain</w:t></w:r>
                    <w:r><w:rPr><w:rStyle w:val="Em"/></w:rPr><w:t>em</w:t></w:r>
                    <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>off</w:t></w:r>
                </w:p>
            </w:body></w:document>"#,
        ),
    ])
}

fn no_styles_docx() -> Vec<u8> {
    let content_types = content_types(false);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(false)),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>plain</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>direct</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "render")]
fn table_style_pagination_docx(direct_row_props: &str) -> Vec<u8> {
    let content_types = content_types(true);
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
            <w:p><w:pPr><w:spacing w:line="800" w:lineRule="exact"/></w:pPr><w:r><w:t>seed</w:t></w:r></w:p>
            <w:tbl><w:tblPr><w:tblStyle w:val="KeepDerived"/></w:tblPr><w:tr>{direct_row_props}<w:tc>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>three</w:t></w:r></w:p>
                <w:p><w:r><w:t>four</w:t></w:r></w:p>
                <w:p><w:r><w:t>five</w:t></w:r></w:p>
            </w:tc></w:tr></w:tbl>
            <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>after</w:t></w:r></w:p>
            <w:sectPr><w:pgSz w:w="4400" w:h="2400"/><w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/></w:sectPr>
        </w:body></w:document>"#
    );
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:style w:type="table" w:styleId="KeepBase"><w:trPr><w:cantSplit/></w:trPr></w:style>
                <w:style w:type="table" w:styleId="KeepDerived"><w:basedOn w:val="KeepBase"/></w:style>
            </w:styles>"#,
        ),
        ("word/document.xml", &document_xml),
    ])
}

#[test]
fn docx_run_props_resolve_docdefaults_paragraph_and_character_styles() {
    let doc = Document::open(&style_inheritance_docx()).expect("fixture opens");
    let model = doc.model();
    let [Block::Paragraph(paragraph)] = model.blocks.as_slice() else {
        panic!("expected one paragraph");
    };
    assert_eq!(paragraph.text(), "plainemoff");
    let [plain, em, off] = paragraph.runs.as_slice() else {
        panic!("expected three runs");
    };

    assert!(plain.props.bold);
    assert!(!plain.props.italic);
    assert_eq!(plain.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(plain.props.size_half_pt, Some(20));

    assert!(em.props.bold);
    assert!(em.props.italic);
    assert_eq!(em.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(em.props.size_half_pt, Some(20));

    assert!(!off.props.bold);
    assert!(!off.props.italic);
    assert_eq!(off.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(off.props.size_half_pt, Some(20));
}

#[test]
fn docx_without_styles_part_keeps_run_defaults_unchanged() {
    let doc = Document::open(&no_styles_docx()).expect("fixture opens");
    let model = doc.model();
    let [Block::Paragraph(paragraph)] = model.blocks.as_slice() else {
        panic!("expected one paragraph");
    };
    let [plain, direct] = paragraph.runs.as_slice() else {
        panic!("expected two runs");
    };

    assert_eq!(plain.text, "plain");
    assert_eq!(plain.props, Default::default());
    assert_eq!(direct.text, "direct");
    assert!(direct.props.italic);
    assert!(!direct.props.bold);
    assert_eq!(direct.props.size_half_pt, None);
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_honors_inherited_table_style_cant_split_and_direct_off() {
    let inherited = Document::open(&table_style_pagination_docx("")).expect("fixture opens");
    let direct_off = Document::open(&table_style_pagination_docx(
        r#"<w:trPr><w:cantSplit w:val="off"/></w:trPr>"#,
    ))
    .expect("fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let inherited_pages = inherited
        .layout_pages_with_fonts(&fonts)
        .expect("inherited table style lays out")
        .pages;
    let direct_off_pages = direct_off
        .layout_pages_with_fonts(&fonts)
        .expect("direct override lays out")
        .pages;

    assert_eq!(
        (inherited_pages, direct_off_pages),
        (3, 2),
        "base table style keeps the row together while direct off restores splitting"
    );
}
