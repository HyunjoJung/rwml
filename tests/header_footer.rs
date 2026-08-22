#![cfg(feature = "docx")]

use std::io::Write;

use rwml::{Block, Document, HeaderFooterKind};

fn docx_fixture(parts: &[(&str, &str)]) -> Vec<u8> {
    let parts = parts
        .iter()
        .map(|(name, body)| (*name, body.as_bytes()))
        .collect::<Vec<_>>();
    docx_fixture_bytes(&parts)
}

fn docx_fixture_bytes(parts: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let opt = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            zip.start_file(*name, opt).unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

#[cfg(feature = "render")]
fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
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

fn header_footer_alternate_content_refs_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdChoiceHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdChoiceFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/><Relationship Id="rIdFallbackHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/><Relationship Id="rIdFallbackFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer2.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><w:t>BODY</w:t></w:r></w:p><w:sectPr><mc:AlternateContent><mc:Choice Requires="w14"><w:headerReference w:type="default" r:id="rIdChoiceHeader"/><w:footerReference w:type="first" r:id="rIdChoiceFooter"/></mc:Choice><mc:Fallback><w:headerReference w:type="default" r:id="rIdFallbackHeader"/><w:footerReference w:type="first" r:id="rIdFallbackFooter"/></mc:Fallback></mc:AlternateContent></w:sectPr></w:body></w:document>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>CHOICE HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer1.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>CHOICE FOOT</w:t></w:r></w:p></w:ftr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FALLBACK HEAD</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/footer2.xml",
            r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>FALLBACK FOOT</w:t></w:r></w:p></w:ftr>"#,
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

#[cfg(feature = "render")]
fn running_surface_image_docx(header_image: bool, footer_image: bool) -> Vec<u8> {
    let empty_header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:hdr>"#;
    let image_header = br#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:p><w:r><w:drawing><wp:inline><wp:extent cx="19050" cy="28575"/><wp:docPr id="1" name="Header logo" descr="Header logo"/><a:blip r:embed="rIdImage"/></wp:inline></w:drawing></w:r></w:p></w:hdr>"#;
    let empty_footer = br#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p/></w:ftr>"#;
    let image_footer = br#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:p><w:r><w:drawing><wp:inline><wp:extent cx="19050" cy="28575"/><wp:docPr id="1" name="Footer mark" descr="Footer mark"/><a:blip r:embed="rIdImage"/></wp:inline></w:drawing></w:r></w:p></w:ftr>"#;
    let header = if header_image {
        image_header.as_slice()
    } else {
        empty_header.as_slice()
    };
    let footer = if footer_image {
        image_footer.as_slice()
    } else {
        empty_footer.as_slice()
    };
    let png = tiny_png();

    docx_fixture_bytes(&[
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:r><w:t>BODY</w:t></w:r></w:p><w:sectPr><w:pgSz w:w="4400" w:h="6000"/><w:pgMar w:top="1200" w:right="400" w:bottom="1200" w:left="400"/><w:headerReference w:type="default" r:id="rIdHeader"/><w:footerReference w:type="default" r:id="rIdFooter"/></w:sectPr></w:body></w:document>"#,
        ),
        ("word/header1.xml", header),
        ("word/footer1.xml", footer),
        (
            "word/_rels/header1.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/logo.png"/></Relationships>"#,
        ),
        (
            "word/_rels/footer1.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/logo.png"/></Relationships>"#,
        ),
        ("word/media/logo.png", png.as_slice()),
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
fn docx_header_footer_refs_use_single_alternate_content_branch() {
    let doc = Document::open(&header_footer_alternate_content_refs_docx()).expect("fixture opens");

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
                "CHOICE HEAD"
            ),
            (
                "word/footer1.xml#first",
                HeaderFooterKind::FirstPageFooter,
                "CHOICE FOOT"
            ),
        ]
    );

    assert_eq!(doc.header_text(), "CHOICE HEAD\nCHOICE FOOT");
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

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_paints_decoded_running_header_and_footer_images() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let open = |header, footer| {
        Document::open(&running_surface_image_docx(header, footer))
            .expect("running-surface image fixture opens")
    };
    let baseline = open(false, false);
    let header = open(true, false);
    let footer = open(false, true);
    let both = open(true, true);

    let header_model = header.model();
    let [Block::Paragraph(header_paragraph)] = header_model.setup.header.as_slice() else {
        panic!("decoded header image must remain in its source paragraph");
    };
    assert_eq!(
        header_paragraph
            .runs
            .iter()
            .filter(|run| run.image.is_some())
            .count(),
        1
    );
    let footer_model = footer.model();
    let [Block::Paragraph(footer_paragraph)] = footer_model.setup.footer.as_slice() else {
        panic!("decoded footer image must remain in its source paragraph");
    };
    assert_eq!(
        footer_paragraph
            .runs
            .iter()
            .filter(|run| run.image.is_some())
            .count(),
        1
    );
    for document in [&baseline, &header, &footer, &both] {
        assert_eq!(
            document
                .layout_pages_with_fonts(&fonts)
                .expect("running-surface image layout succeeds")
                .pages,
            1
        );
    }

    let baseline_pdf = baseline.to_pdf_with_fonts(&fonts);
    let header_pdf = header.to_pdf_with_fonts(&fonts);
    let footer_pdf = footer.to_pdf_with_fonts(&fonts);
    let both_pdf = both.to_pdf_with_fonts(&fonts);
    for rendered in [&header_pdf, &footer_pdf, &both_pdf] {
        assert!(rendered.starts_with(b"%PDF-"));
        assert!(
            rendered != &baseline_pdf,
            "decoded running image was dropped"
        );
    }
    assert_ne!(
        header_pdf, footer_pdf,
        "header and footer positions must differ"
    );
    assert_ne!(both_pdf, header_pdf);
    assert_ne!(both_pdf, footer_pdf);
    assert_eq!(header_pdf, header.to_pdf_with_fonts(&fonts));
    assert_eq!(footer_pdf, footer.to_pdf_with_fonts(&fonts));
    assert_eq!(both_pdf, both.to_pdf_with_fonts(&fonts));
}

#[cfg(feature = "render")]
fn running_surface_line_spacing_docx(
    ending_header_properties: &str,
    final_header_properties: &str,
    even_footer_properties: &str,
) -> Vec<u8> {
    let ending_header = format!(
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr>{ending_header_properties}</w:pPr><w:r><w:t>SHARED RUNNING HEADER</w:t></w:r></w:p></w:hdr>"#
    );
    let final_header = format!(
        r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr>{final_header_properties}</w:pPr><w:r><w:t>SHARED RUNNING HEADER</w:t></w:r></w:p></w:hdr>"#
    );
    let even_footer = format!(
        r#"<w:ftr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr>{even_footer_properties}</w:pPr><w:r><w:t>EVEN RUNNING FOOTER</w:t></w:r></w:p></w:ftr>"#
    );
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdEndingHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="rIdFinalHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/><Relationship Id="rIdEvenFooter" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footer" Target="footer1.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>
                <w:p><w:r><w:t>ending section body</w:t></w:r></w:p>
                <w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgSz w:w="4400" w:h="6000"/><w:pgMar w:top="1200" w:right="400" w:bottom="1200" w:left="400"/><w:headerReference w:type="default" r:id="rIdEndingHeader"/></w:sectPr></w:pPr></w:p>
                <w:p><w:r><w:t>final section body</w:t></w:r></w:p>
                <w:sectPr><w:pgSz w:w="4400" w:h="6000"/><w:pgMar w:top="1200" w:right="400" w:bottom="1200" w:left="400"/><w:headerReference w:type="default" r:id="rIdFinalHeader"/><w:footerReference w:type="even" r:id="rIdEvenFooter"/></w:sectPr>
            </w:body></w:document>"#,
        ),
        ("word/header1.xml", &ending_header),
        ("word/header2.xml", &final_header),
        ("word/footer1.xml", &even_footer),
    ])
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_consumes_section_and_variant_running_surface_absolute_spacing() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline_model = Document::open(&running_surface_line_spacing_docx("", "", ""))
        .expect("baseline running-surface fixture opens")
        .model();
    let render = |ending_header_properties: &str,
                  final_header_properties: &str,
                  even_footer_properties: &str| {
        let document = Document::open(&running_surface_line_spacing_docx(
            ending_header_properties,
            final_header_properties,
            even_footer_properties,
        ))
        .expect("running-surface fixture opens");
        assert_eq!(
            document.model(),
            baseline_model,
            "absolute running-surface spacing must remain outside the public model"
        );
        let layout = document
            .layout_pages_with_fonts(&fonts)
            .expect("running-surface layout succeeds");
        assert_eq!(layout.pages, 2, "fixture must keep one page per section");
        document.to_pdf_with_fonts(&fonts)
    };

    let baseline = render("", "", "");
    let ending_exact = render(r#"<w:spacing w:line="100" w:lineRule="exact"/>"#, "", "");
    let final_minimum = render("", r#"<w:spacing w:line="800" w:lineRule="atLeast"/>"#, "");
    let even_footer_exact = render("", "", r#"<w:spacing w:line="100" w:lineRule="exact"/>"#);

    for (name, rendered) in [
        ("ending-section exact header", &ending_exact),
        ("final-section minimum header", &final_minimum),
        ("even-page exact footer", &even_footer_exact),
    ] {
        assert!(rendered.starts_with(b"%PDF-"), "{name}");
        assert_ne!(rendered, &baseline, "{name} must affect PDF output");
    }
    assert_ne!(ending_exact, final_minimum);
    assert_ne!(ending_exact, even_footer_exact);
    assert_ne!(final_minimum, even_footer_exact);
    assert_eq!(
        ending_exact,
        render(r#"<w:spacing w:line="100" w:lineRule="exact"/>"#, "", "")
    );
    assert_eq!(
        final_minimum,
        render("", r#"<w:spacing w:line="800" w:lineRule="atLeast"/>"#, "")
    );
    assert_eq!(
        even_footer_exact,
        render("", "", r#"<w:spacing w:line="100" w:lineRule="exact"/>"#)
    );
}
