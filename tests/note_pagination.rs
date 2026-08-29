#![cfg(all(feature = "docx", feature = "render"))]

use std::io::Write;

use rwml::Document;

fn docx_fixture(parts: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

fn note_pagination_docx(seed_line_twips: u32, note_properties: &str, note_text: &str) -> Vec<u8> {
    let footnotes = format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
            <w:footnote w:id="1"><w:p><w:pPr>{note_properties}<w:spacing w:line="200" w:lineRule="exact"/></w:pPr>
                <w:r>{note_text}</w:r>
            </w:p></w:footnote>
        </w:footnotes>"#
    );
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            &format!(
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:spacing w:line="{seed_line_twips}" w:lineRule="exact"/></w:pPr><w:r><w:t>seed</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>
                <w:sectPr><w:pgSz w:w="4400" w:h="2000"/><w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/></w:sectPr>
            </w:body></w:document>"#
            ),
        ),
        ("word/footnotes.xml", &footnotes),
    ])
}

#[test]
fn opened_docx_render_consumes_real_note_keep_lines() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let note_text = "<w:t>one</w:t><w:br/><w:t>two</w:t><w:br/><w:t>three</w:t>";
    let baseline = Document::open(&note_pagination_docx(
        800,
        r#"<w:widowControl w:val="off"/>"#,
        note_text,
    ))
    .expect("baseline note fixture opens");
    let kept = Document::open(&note_pagination_docx(
        800,
        r#"<w:keepLines/><w:widowControl w:val="off"/>"#,
        note_text,
    ))
    .expect("keep-lines note fixture opens");

    assert_eq!(
        baseline.model(),
        kept.model(),
        "note pagination controls must remain outside the public model"
    );
    let baseline_layout = baseline
        .layout_pages_with_fonts(&fonts)
        .expect("baseline note layout succeeds");
    let kept_layout = kept
        .layout_pages_with_fonts(&fonts)
        .expect("keep-lines note layout succeeds");

    assert_eq!(baseline_layout.block_pages, vec![Some(1), Some(1)]);
    assert_eq!(kept_layout.block_pages, vec![Some(1), Some(2)]);
    assert_eq!(kept_layout.pages, 2);
    let kept_pdf = kept
        .try_to_pdf_with_fonts(&fonts)
        .expect("keep-lines note PDF renders");
    assert_ne!(
        baseline
            .try_to_pdf_with_fonts(&fonts)
            .expect("baseline note PDF renders"),
        kept_pdf
    );
    assert_eq!(
        kept_pdf,
        kept.try_to_pdf_with_fonts(&fonts)
            .expect("keep-lines note PDF rerenders")
    );
}

#[test]
fn opened_docx_render_consumes_real_note_widow_control() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let note_text =
        "<w:t>one</w:t><w:br/><w:t>two</w:t><w:br/><w:t>three</w:t><w:br/><w:t>four</w:t>";
    let disabled = Document::open(&note_pagination_docx(
        360,
        r#"<w:widowControl w:val="off"/>"#,
        note_text,
    ))
    .expect("widow-off note fixture opens");
    let enabled = Document::open(&note_pagination_docx(360, "<w:widowControl/>", note_text))
        .expect("widow-on note fixture opens");

    assert_eq!(
        disabled.model(),
        enabled.model(),
        "note widow control must remain outside the public model"
    );
    let disabled_layout = disabled
        .layout_pages_with_fonts(&fonts)
        .expect("widow-off note layout succeeds");
    let enabled_layout = enabled
        .layout_pages_with_fonts(&fonts)
        .expect("widow-on note layout succeeds");
    assert_eq!(disabled_layout.block_pages, vec![Some(1), Some(1)]);
    assert_eq!(enabled_layout.block_pages, vec![Some(1), Some(1)]);
    assert_eq!(disabled_layout.pages, 2);
    assert_eq!(enabled_layout.pages, 2);

    let disabled_pdf = disabled
        .try_to_pdf_with_fonts(&fonts)
        .expect("widow-off note PDF renders");
    let enabled_pdf = enabled
        .try_to_pdf_with_fonts(&fonts)
        .expect("widow-on note PDF renders");
    assert_ne!(disabled_pdf, enabled_pdf);
    assert_eq!(
        enabled_pdf,
        enabled
            .try_to_pdf_with_fonts(&fonts)
            .expect("widow-on note PDF rerenders")
    );
}
