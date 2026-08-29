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

fn protected_note_table_xml(
    nested: bool,
    keep_lines: bool,
    cant_split: bool,
    line_spacing: &str,
) -> String {
    let pagination = if keep_lines {
        "<w:keepLines/>"
    } else {
        r#"<w:keepLines w:val="off"/>"#
    };
    let row_pagination = if cant_split {
        "<w:trPr><w:cantSplit/></w:trPr>"
    } else {
        r#"<w:trPr><w:cantSplit w:val="off"/></w:trPr>"#
    };
    let protected_table = format!(
        r#"<w:tbl><w:tr>{row_pagination}<w:tc>
            <w:p><w:pPr>{pagination}<w:widowControl w:val="off"/>{line_spacing}</w:pPr>
                <w:r><w:t>one</w:t><w:br/><w:t>two</w:t><w:br/><w:t>three</w:t></w:r>
            </w:p>
        </w:tc></w:tr></w:tbl>"#
    );
    if nested {
        format!(r#"<w:tbl><w:tr><w:tc>{protected_table}<w:p/></w:tc></w:tr></w:tbl>"#)
    } else {
        protected_table
    }
}

fn note_table_pagination_docx(
    nested: bool,
    keep_lines: bool,
    cant_split: bool,
    line_spacing: &str,
) -> Vec<u8> {
    let table = protected_note_table_xml(nested, keep_lines, cant_split, line_spacing);
    let footnotes = format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>
            <w:footnote w:id="1">{table}
                <w:p><w:pPr><w:widowControl w:val="off"/></w:pPr><w:r><w:t>after</w:t></w:r></w:p>
            </w:footnote>
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:spacing w:line="480"/></w:pPr><w:r><w:t>seed</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r></w:p>
                <w:sectPr><w:pgSz w:w="4400" w:h="2000"/><w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/></w:sectPr>
            </w:body></w:document>"#,
        ),
        ("word/footnotes.xml", &footnotes),
    ])
}

fn mixed_endnote_table_pagination_docx(keep_lines: bool, cant_split: bool) -> Vec<u8> {
    let table = protected_note_table_xml(false, keep_lines, cant_split, "");
    let footnotes = r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
        <w:footnote w:id="1"><w:p><w:r><w:t>footnote prefix</w:t></w:r></w:p></w:footnote>
    </w:footnotes>"#;
    let endnotes = format!(
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:endnote w:id="2">{table}
                <w:p><w:pPr><w:widowControl w:val="off"/></w:pPr><w:r><w:t>after</w:t></w:r></w:p>
            </w:endnote>
        </w:endnotes>"#
    );
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:spacing w:line="240"/></w:pPr><w:r><w:t>seed</w:t></w:r><w:r><w:footnoteReference w:id="1"/><w:endnoteReference w:id="2"/></w:r></w:p>
                <w:sectPr><w:pgSz w:w="4400" w:h="2000"/><w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/></w:sectPr>
            </w:body></w:document>"#,
        ),
        ("word/footnotes.xml", footnotes),
        ("word/endnotes.xml", &endnotes),
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

#[test]
fn opened_docx_render_consumes_note_table_cell_keep_lines() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline = Document::open(&note_table_pagination_docx(false, false, false, ""))
        .expect("baseline note-table fixture opens");
    let kept = Document::open(&note_table_pagination_docx(false, true, false, ""))
        .expect("keep-lines note-table fixture opens");

    assert_eq!(baseline.model(), kept.model());
    let baseline_pages = baseline
        .layout_pages_with_fonts(&fonts)
        .expect("baseline note table lays out")
        .pages;
    let kept_pages = kept
        .layout_pages_with_fonts(&fonts)
        .expect("kept note table lays out")
        .pages;
    assert_eq!((baseline_pages, kept_pages), (2, 3));
}

#[test]
fn opened_docx_render_consumes_note_table_row_cant_split() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline = Document::open(&note_table_pagination_docx(false, false, false, ""))
        .expect("baseline note-table fixture opens");
    let kept = Document::open(&note_table_pagination_docx(false, false, true, ""))
        .expect("cant-split note-table fixture opens");

    assert_eq!(baseline.model(), kept.model());
    let baseline_pages = baseline
        .layout_pages_with_fonts(&fonts)
        .expect("baseline note table lays out")
        .pages;
    let kept_pages = kept
        .layout_pages_with_fonts(&fonts)
        .expect("cant-split note table lays out")
        .pages;
    assert_eq!((baseline_pages, kept_pages), (2, 3));
}

#[test]
fn opened_docx_render_consumes_nested_note_table_pagination() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline = Document::open(&note_table_pagination_docx(true, false, false, ""))
        .expect("baseline nested note-table fixture opens");
    let kept = Document::open(&note_table_pagination_docx(true, true, false, ""))
        .expect("keep-lines nested note-table fixture opens");
    let unsplit = Document::open(&note_table_pagination_docx(true, false, true, ""))
        .expect("cant-split nested note-table fixture opens");

    assert_eq!(baseline.model(), kept.model());
    assert_eq!(baseline.model(), unsplit.model());
    let pages = |document: &Document| {
        document
            .layout_pages_with_fonts(&fonts)
            .expect("nested note table lays out")
            .pages
    };
    assert_eq!((pages(&baseline), pages(&kept), pages(&unsplit)), (2, 3, 3));
}

#[test]
fn opened_docx_render_keeps_endnote_table_sidecars_aligned_after_footnotes() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline = Document::open(&mixed_endnote_table_pagination_docx(false, false))
        .expect("mixed baseline fixture opens");
    let kept = Document::open(&mixed_endnote_table_pagination_docx(true, false))
        .expect("mixed keep-lines fixture opens");
    let unsplit = Document::open(&mixed_endnote_table_pagination_docx(false, true))
        .expect("mixed cant-split fixture opens");

    assert_eq!(baseline.model(), kept.model());
    assert_eq!(baseline.model(), unsplit.model());
    let pages = |document: &Document| {
        document
            .layout_pages_with_fonts(&fonts)
            .expect("mixed note fixture lays out")
            .pages
    };
    assert_eq!((pages(&baseline), pages(&kept), pages(&unsplit)), (2, 3, 3));
    let render = |document: &Document| {
        document
            .try_to_pdf_with_fonts(&fonts)
            .expect("mixed note fixture renders")
    };
    let baseline_pdf = render(&baseline);
    assert_ne!(baseline_pdf, render(&kept));
    assert_ne!(baseline_pdf, render(&unsplit));
}

#[test]
fn opened_docx_render_consumes_note_table_cell_absolute_line_spacing() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let baseline = Document::open(&note_table_pagination_docx(false, false, false, ""))
        .expect("baseline note-table fixture opens");
    let exact = Document::open(&note_table_pagination_docx(
        false,
        false,
        false,
        r#"<w:spacing w:line="100" w:lineRule="exact"/>"#,
    ))
    .expect("exact-spacing note-table fixture opens");

    assert_eq!(baseline.model(), exact.model());
    let baseline_pdf = baseline
        .try_to_pdf_with_fonts(&fonts)
        .expect("baseline note table renders");
    let exact_pdf = exact
        .try_to_pdf_with_fonts(&fonts)
        .expect("exact-spacing note table renders");
    assert_ne!(baseline_pdf, exact_pdf);
    assert_eq!(
        exact_pdf,
        exact
            .try_to_pdf_with_fonts(&fonts)
            .expect("exact-spacing note table rerenders")
    );
}
