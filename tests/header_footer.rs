#![cfg(feature = "docx")]

use std::io::Write;

use rdoc::{Block, Document, HeaderFooterKind};

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

fn header_footer_variants_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header3.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footer2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footer3.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDefaultHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFirstHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/><Relationship Id="rIdEvenHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header3.xml"/><Relationship Id="rIdDefaultFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rIdFirstFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer2.xml"/><Relationship Id="rIdEvenFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer3.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>BODY</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdDefaultHeader"/><w:headerReference w:type=" first " r:id="rIdFirstHeader"/><w:headerReference w:type=" even " r:id="rIdEvenHeader"/><w:footerReference w:type="default" r:id="rIdDefaultFooter"/><w:footerReference w:type=" first " r:id="rIdFirstFooter"/><w:footerReference w:type=" even " r:id="rIdEvenFooter"/></w:sectPr></w:body></w:document>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>DEFAULT HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header3.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>EVEN HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer1.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>DEFAULT FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/footer2.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/footer3.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>EVEN FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
    ])
}

fn multi_section_header_footer_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footer2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFirstHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdSecondHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/><Relationship Id="rIdFirstFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rIdSecondFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer2.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>FIRST BODY</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdFirstHeader"/><w:footerReference w:type="default" r:id="rIdFirstFooter"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:t>SECOND BODY</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdSecondHeader"/><w:footerReference w:type="default" r:id="rIdSecondFooter"/></w:sectPr></w:body></w:document>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>SECOND HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer1.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/footer2.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>SECOND FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
    ])
}

fn multi_section_inherited_header_footer_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFirstHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFirstFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>FIRST BODY</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:headerReference w:type="default" r:id="rIdFirstHeader"/><w:footerReference w:type="default" r:id="rIdFirstFooter"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:t>SECOND BODY</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>INHERITED HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer1.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>INHERITED FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
    ])
}

fn multi_section_variant_header_footer_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header3.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footer2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/footer3.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/header4.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer4.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFirstDefaultHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFirstFirstHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/><Relationship Id="rIdFirstEvenHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header3.xml"/><Relationship Id="rIdFirstDefaultFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rIdFirstFirstFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer2.xml"/><Relationship Id="rIdFirstEvenFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer3.xml"/><Relationship Id="rIdSecondHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header4.xml"/><Relationship Id="rIdSecondFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer4.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>FIRST BODY</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:titlePg/><w:headerReference w:type="default" r:id="rIdFirstDefaultHeader"/><w:headerReference w:type="first" r:id="rIdFirstFirstHeader"/><w:headerReference w:type="even" r:id="rIdFirstEvenHeader"/><w:footerReference w:type="default" r:id="rIdFirstDefaultFooter"/><w:footerReference w:type="first" r:id="rIdFirstFirstFooter"/><w:footerReference w:type="even" r:id="rIdFirstEvenFooter"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:t>SECOND BODY</w:t></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdSecondHeader"/><w:footerReference w:type="default" r:id="rIdSecondFooter"/></w:sectPr></w:body></w:document>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST DEFAULT HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST FIRST HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header3.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST EVEN HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer1.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST DEFAULT FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/footer2.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST FIRST FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/footer3.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FIRST EVEN FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/header4.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>SECOND HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer4.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>SECOND FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
    ])
}

fn single_paragraph_text(blocks: &[Block]) -> String {
    let [Block::Paragraph(paragraph)] = blocks else {
        panic!("expected exactly one paragraph block, got {blocks:?}");
    };
    paragraph.text()
}

#[test]
fn docx_header_footer_side_table_preserves_reference_variants() {
    let doc = Document::open(&header_footer_variants_docx()).expect("fixture opens");

    let records = doc.header_footers();
    let actual = records
        .iter()
        .map(|record| (record.id.as_str(), record.kind, record.text.as_str()))
        .collect::<Vec<_>>();
    assert_eq!(
        actual,
        vec![
            (
                "word/header1.xml#default",
                HeaderFooterKind::Header,
                "DEFAULT HEAD"
            ),
            (
                "word/header2.xml#first",
                HeaderFooterKind::FirstPageHeader,
                "FIRST HEAD"
            ),
            (
                "word/header3.xml#even",
                HeaderFooterKind::EvenPageHeader,
                "EVEN HEAD"
            ),
            (
                "word/footer1.xml#default",
                HeaderFooterKind::Footer,
                "DEFAULT FOOT"
            ),
            (
                "word/footer2.xml#first",
                HeaderFooterKind::FirstPageFooter,
                "FIRST FOOT"
            ),
            (
                "word/footer3.xml#even",
                HeaderFooterKind::EvenPageFooter,
                "EVEN FOOT"
            ),
        ]
    );

    assert_eq!(
        doc.header_text(),
        "DEFAULT HEAD\nFIRST HEAD\nEVEN HEAD\nDEFAULT FOOT\nFIRST FOOT\nEVEN FOOT",
        "header_text should expose all modeled default/first/even running variants"
    );

    let model = doc.model();
    assert_eq!(model.setup.header.len(), 1);
    assert_eq!(model.setup.first_header.len(), 1);
    assert_eq!(model.setup.even_header.len(), 1);
    assert_eq!(model.setup.footer.len(), 1);
    assert_eq!(model.setup.first_footer.len(), 1);
    assert_eq!(model.setup.even_footer.len(), 1);
    let Block::Paragraph(header) = &model.setup.header[0] else {
        panic!("default header block should be a paragraph");
    };
    let Block::Paragraph(first_header) = &model.setup.first_header[0] else {
        panic!("first-page header block should be a paragraph");
    };
    let Block::Paragraph(even_header) = &model.setup.even_header[0] else {
        panic!("even-page header block should be a paragraph");
    };
    let Block::Paragraph(footer) = &model.setup.footer[0] else {
        panic!("default footer block should be a paragraph");
    };
    let Block::Paragraph(first_footer) = &model.setup.first_footer[0] else {
        panic!("first-page footer block should be a paragraph");
    };
    let Block::Paragraph(even_footer) = &model.setup.even_footer[0] else {
        panic!("even-page footer block should be a paragraph");
    };
    assert_eq!(header.text(), "DEFAULT HEAD");
    assert_eq!(first_header.text(), "FIRST HEAD");
    assert_eq!(even_header.text(), "EVEN HEAD");
    assert_eq!(footer.text(), "DEFAULT FOOT");
    assert_eq!(first_footer.text(), "FIRST FOOT");
    assert_eq!(even_footer.text(), "EVEN FOOT");
}

#[test]
fn docx_multi_section_default_headers_attach_to_section_boundaries() {
    let doc = Document::open(&multi_section_header_footer_docx()).expect("fixture opens");
    let model = doc.model();

    let section = model
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::SectionBreak(setup) => Some(setup),
            _ => None,
        })
        .expect("expected a section break for the paragraph sectPr");

    assert_eq!(single_paragraph_text(&section.header), "FIRST HEAD");
    assert_eq!(single_paragraph_text(&section.footer), "FIRST FOOT");
    assert_eq!(single_paragraph_text(&model.setup.header), "SECOND HEAD");
    assert_eq!(single_paragraph_text(&model.setup.footer), "SECOND FOOT");

    assert_eq!(
        doc.header_text(),
        "FIRST HEAD\nFIRST FOOT\nSECOND HEAD\nSECOND FOOT",
        "header_text should expose default running surfaces for each section without folding them all into the final setup"
    );
    let text = doc.text();
    assert!(
        text.contains("FIRST HEAD")
            && text.contains("FIRST FOOT")
            && text.contains("SECOND HEAD")
            && text.contains("SECOND FOOT"),
        "full text should retain all section default header/footer surfaces: {text:?}"
    );
}

#[test]
fn docx_section_defaults_inherit_from_previous_section_when_omitted() {
    let doc = Document::open(&multi_section_inherited_header_footer_docx()).expect("fixture opens");
    let model = doc.model();

    let section = model
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::SectionBreak(setup) => Some(setup),
            _ => None,
        })
        .expect("expected a section break for the paragraph sectPr");

    assert_eq!(single_paragraph_text(&section.header), "INHERITED HEAD");
    assert_eq!(single_paragraph_text(&section.footer), "INHERITED FOOT");
    assert_eq!(single_paragraph_text(&model.setup.header), "INHERITED HEAD");
    assert_eq!(single_paragraph_text(&model.setup.footer), "INHERITED FOOT");
    assert_eq!(
        doc.header_footers().len(),
        2,
        "inherited section surfaces should not duplicate side-table part records"
    );
}

#[test]
fn docx_multi_section_first_even_headers_attach_to_section_boundaries() {
    let doc = Document::open(&multi_section_variant_header_footer_docx()).expect("fixture opens");
    let model = doc.model();

    let section = model
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::SectionBreak(setup) => Some(setup),
            _ => None,
        })
        .expect("expected a section break for the paragraph sectPr");

    assert_eq!(single_paragraph_text(&section.header), "FIRST DEFAULT HEAD");
    assert_eq!(
        single_paragraph_text(&section.first_header),
        "FIRST FIRST HEAD"
    );
    assert_eq!(
        single_paragraph_text(&section.even_header),
        "FIRST EVEN HEAD"
    );
    assert_eq!(single_paragraph_text(&section.footer), "FIRST DEFAULT FOOT");
    assert_eq!(
        single_paragraph_text(&section.first_footer),
        "FIRST FIRST FOOT"
    );
    assert_eq!(
        single_paragraph_text(&section.even_footer),
        "FIRST EVEN FOOT"
    );
    assert_eq!(single_paragraph_text(&model.setup.header), "SECOND HEAD");
    assert!(model.setup.first_header.is_empty());
    assert!(model.setup.even_header.is_empty());
}
