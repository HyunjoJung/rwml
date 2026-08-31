#![cfg(all(feature = "docx", feature = "render"))]

use std::io::Write;

use rwml::Document;

fn fixture(body: &str, notes: &str, settings: &str) -> Vec<u8> {
    fixture_with_endnotes(body, notes, "", settings)
}

fn fixture_with_endnotes(body: &str, notes: &str, endnotes: &str, settings: &str) -> Vec<u8> {
    let document = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body>{body}<w:sectPr><w:cols w:num="2"/></w:sectPr></w:body></w:document>"#
    );
    let footnotes = format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{notes}</w:footnotes>"#
    );
    let settings = format!(
        r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{settings}</w:settings>"#
    );
    let endnotes = format!(
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{endnotes}</w:endnotes>"#
    );
    let mut bytes = Vec::new();
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut bytes));
    for (name, text) in [
        ("word/document.xml", document.as_str()),
        ("word/footnotes.xml", footnotes.as_str()),
        ("word/endnotes.xml", endnotes.as_str()),
        ("word/settings.xml", settings.as_str()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(text.as_bytes()).unwrap();
    }
    zip.finish().unwrap();
    bytes
}

fn assert_matches_literal_markers(source: &[u8], expected: &[u8]) {
    let document = Document::open(source).unwrap();
    let expected = Document::open(expected).unwrap();
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let original_text = document.text();
    let rendered = document
        .try_to_pdf_with_fixed_fonts_and_report(&fonts)
        .unwrap();
    assert!(
        rendered.pdf
            == expected
                .try_to_pdf_with_fixed_fonts_and_report(&fonts)
                .unwrap()
                .pdf,
        "note markers must render like their resolved literal text"
    );
    assert!(
        rendered.pdf
            == document
                .try_to_pdf_with_fixed_fonts_and_report(&fonts)
                .unwrap()
                .pdf
    );
    assert!(
        document.try_to_pdf_with_fonts(&fonts).unwrap()
            == expected.try_to_pdf_with_fonts(&fonts).unwrap()
    );
    assert_eq!(original_text, document.text());
    let saved = document.save().unwrap();
    let mut original = zip::ZipArchive::new(std::io::Cursor::new(source)).unwrap();
    let mut saved = zip::ZipArchive::new(std::io::Cursor::new(saved)).unwrap();
    use std::io::Read;
    for index in 0..original.len() {
        let mut before = original.by_index(index).unwrap();
        let mut after = saved.by_name(before.name()).unwrap();
        let mut a = Vec::new();
        let mut b = Vec::new();
        before.read_to_end(&mut a).unwrap();
        after.read_to_end(&mut b).unwrap();
        assert_eq!(a, b, "rendering must not change package parts");
    }
}

#[test]
fn note_markers_use_reference_order_and_settings_not_ids_or_part_order() {
    let body = r#"<w:p><w:r><w:t>First</w:t><w:footnoteReference w:id="90"/><w:br w:type="column"/><w:t>after</w:t></w:r></w:p>
        <w:tbl><w:tr><w:tc><w:p><w:r><w:t>Second</w:t><w:footnoteReference w:id="4"/></w:r></w:p></w:tc></w:tr></w:tbl>"#;
    let notes = r#"<w:footnote w:id="4"><w:p><w:r><w:footnoteRef/><w:t> Second note</w:t></w:r></w:p></w:footnote>
        <w:footnote w:id="90"><w:p><w:r><w:footnoteRef></w:footnoteRef><w:t> First note</w:t></w:r></w:p></w:footnote>"#;
    let settings =
        r#"<w:footnotePr><w:numStart w:val="3"/><w:numFmt w:val="lowerLetter"/></w:footnotePr>"#;
    let literal_body = body
        .replace(r#"<w:footnoteReference w:id="90"/>"#, "<w:t>c</w:t>")
        .replace(r#"<w:footnoteReference w:id="4"/>"#, "<w:t>d</w:t>");
    let literal_notes = notes
        .replace("<w:footnoteRef/>", "<w:t>d</w:t>")
        .replace("<w:footnoteRef></w:footnoteRef>", "<w:t>c</w:t>");
    assert_matches_literal_markers(
        &fixture(body, notes, settings),
        &fixture(&literal_body, &literal_notes, settings),
    );
}

#[test]
fn note_markers_preserve_run_style_and_skip_custom_and_deleted_ordinals() {
    let body = r#"<w:p><w:del><w:r><w:footnoteReference w:id="7"/></w:r></w:del>
        <w:r><w:t>Custom</w:t><w:footnoteReference w:id="8" w:customMarkFollows="1"/><w:t>*</w:t></w:r>
        <w:r><w:rPr><w:b/><w:color w:val="A02020"/><w:vertAlign w:val="superscript"/></w:rPr><w:footnoteReference w:id="42"></w:footnoteReference></w:r>
        <w:r><w:t> tail</w:t></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="8"><w:p><w:r><w:footnoteRef></w:footnoteRef><w:t> Custom note</w:t></w:r></w:p></w:footnote>
        <w:footnote w:id="42"><w:p><w:r><w:rPr><w:i/></w:rPr><w:footnoteRef/><w:t> Automatic note</w:t></w:r></w:p></w:footnote>"#;
    assert_matches_literal_markers(
        &fixture(body, notes, ""),
        &fixture(
            &body.replace(
                r#"<w:footnoteReference w:id="42"></w:footnoteReference>"#,
                "<w:t>1</w:t>",
            ),
            &notes
                .replace("<w:footnoteRef></w:footnoteRef>", "<w:t>*</w:t>")
                .replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}

#[test]
fn note_markers_keep_footnote_and_endnote_sequences_independent() {
    let body = r#"<w:p><w:r><w:t>Foot</w:t><w:footnoteReference w:id="9"/><w:t> End</w:t><w:endnoteReference w:id="9"/></w:r></w:p>"#;
    let footnotes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Footnote</w:t></w:r></w:p></w:footnote>"#;
    let endnotes = r#"<w:endnote w:id="9"><w:p><w:r><w:endnoteRef/><w:t> Endnote</w:t></w:r></w:p></w:endnote>"#;
    let settings = r#"<w:footnotePr><w:numStart w:val="3"/><w:numFmt w:val="lowerLetter"/></w:footnotePr><w:endnotePr><w:numStart w:val="2"/><w:numFmt w:val="lowerRoman"/></w:endnotePr>"#;
    assert_matches_literal_markers(
        &fixture_with_endnotes(body, footnotes, endnotes, settings),
        &fixture_with_endnotes(
            &body
                .replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>c</w:t>")
                .replace(r#"<w:endnoteReference w:id="9"/>"#, "<w:t>ii</w:t>"),
            &footnotes.replace("<w:footnoteRef/>", "<w:t>c</w:t>"),
            &endnotes.replace("<w:endnoteRef/>", "<w:t>ii</w:t>"),
            settings,
        ),
    );
}

#[test]
fn note_markers_do_not_invent_numbers_for_unsupported_numbering() {
    let body = r#"<w:p><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    for properties in [
        r#"<w:numRestart w:val="eachPage"/>"#,
        r#"<w:numRestart w:val="eachSect"/>"#,
        r#"<w:numFmt w:val="chicago"/>"#,
    ] {
        let settings = format!("<w:footnotePr>{properties}</w:footnotePr>");
        assert_matches_literal_markers(
            &fixture(body, notes, &settings),
            &fixture(
                &body.replace(r#"<w:footnoteReference w:id="9"/>"#, ""),
                &notes.replace("<w:footnoteRef/>", ""),
                &settings,
            ),
        );
    }
    let section_override = r#"<w:p><w:pPr><w:sectPr><w:footnotePr><w:numStart w:val="4"/></w:footnotePr></w:sectPr></w:pPr><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    assert_matches_literal_markers(
        &fixture(section_override, notes, ""),
        &fixture(
            &section_override.replace(r#"<w:footnoteReference w:id="9"/>"#, ""),
            &notes.replace("<w:footnoteRef/>", ""),
            "",
        ),
    );
}

#[test]
fn note_marker_numbering_ignores_section_placement_only_properties() {
    let body = r#"<w:p><w:pPr><w:sectPr><w:footnotePr><w:pos w:val="beneathText"/></w:footnotePr></w:sectPr></w:pPr><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    assert_matches_literal_markers(
        &fixture(body, notes, ""),
        &fixture(
            &body.replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &notes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}
