#![cfg(all(feature = "docx", feature = "render"))]

use std::io::Write;

use rwml::Document;

fn unequal_column_docx() -> Vec<u8> {
    let document_xml = r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
        <w:p><w:r><w:t>seed</w:t><w:br w:type="column"/></w:r></w:p>
        <w:p><w:r><w:t>alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu</w:t></w:r></w:p>
        <w:sectPr>
            <w:pgSz w:w="4400" w:h="2400"/>
            <w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/>
            <w:cols w:num="2" w:equalWidth="0">
                <w:col w:w="1200" w:space="400"/>
                <w:col w:w="2000"/>
            </w:cols>
        </w:sectPr>
    </w:body></w:document>"#;
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#,
            ),
            ("word/document.xml", document_xml),
        ] {
            zip.start_file(name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

#[test]
fn opened_docx_rewraps_a_paragraph_for_its_wider_target_column() {
    let document = Document::open(&unequal_column_docx()).expect("fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let layout = document
        .layout_pages_with_fonts(&fonts)
        .expect("fixture lays out");
    let pdf = document
        .try_to_pdf_with_fonts(&fonts)
        .expect("fixture renders");

    assert_eq!(layout.pages, 1);
    assert_eq!(layout.block_pages, [Some(1), Some(1)]);
    assert!(pdf.starts_with(b"%PDF-"));
    assert_eq!(
        pdf,
        document
            .try_to_pdf_with_fonts(&fonts)
            .expect("rerender is deterministic")
    );
}
