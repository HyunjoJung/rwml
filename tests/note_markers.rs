#![cfg(all(feature = "docx", feature = "render"))]

use std::io::Write;

use rwml::Document;

fn fixture(body: &str, notes: &str, settings: &str) -> Vec<u8> {
    fixture_with_endnotes(body, notes, "", settings)
}

fn fixture_with_endnotes(body: &str, notes: &str, endnotes: &str, settings: &str) -> Vec<u8> {
    fixture_with_extra_parts(body, notes, endnotes, settings, &[])
}

fn fixture_with_extra_parts(
    body: &str,
    notes: &str,
    endnotes: &str,
    settings: &str,
    extra_parts: &[(&str, &str)],
) -> Vec<u8> {
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
        ("[Content_Types].xml", r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#),
        ("_rels/.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="document" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#),
        ("word/_rels/document.xml.rels", r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="footnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="endnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="settings" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/settings" Target="settings.xml"/></Relationships>"#),
        ("word/document.xml", document.as_str()),
        ("word/footnotes.xml", footnotes.as_str()),
        ("word/endnotes.xml", endnotes.as_str()),
        ("word/settings.xml", settings.as_str()),
    ]
    .into_iter()
    .filter(|(name, _)| !extra_parts.iter().any(|(extra, _)| name == extra))
    .chain(extra_parts.iter().copied())
    {
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
    let original_model = document.model();
    let original_notes = document.notes();
    let original_shapes = document.floating_shapes();
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
    assert_eq!(original_model, document.model());
    assert_eq!(original_notes, document.notes());
    assert_eq!(original_shapes, document.floating_shapes());
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
        r#"<w:footnotePr><w:numFmt w:val="lowerLetter"/><w:numStart w:val="3"/></w:footnotePr>"#;
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
    let settings = r#"<w:footnotePr><w:numFmt w:val="lowerLetter"/><w:numStart w:val="3"/></w:footnotePr><w:endnotePr><w:numFmt w:val="lowerRoman"/><w:numStart w:val="2"/></w:endnotePr>"#;
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

#[test]
fn note_markers_keep_page_break_and_section_column_hints_aligned() {
    let body = format!(
        r#"<w:p><w:r><w:footnoteReference w:id="9"/><w:br w:type="page"/><w:t>After break</w:t></w:r></w:p>
        <w:p><w:pPr><w:sectPr><w:type w:val="continuous"/><w:cols w:num="2" w:equalWidth="0"><w:col w:w="1000" w:space="360"/><w:col w:w="7640"/></w:cols></w:sectPr></w:pPr><w:r><w:t>{}</w:t></w:r></w:p>
        <w:p><w:r><w:t>Final section.</w:t></w:r></w:p>"#,
        "A source marker must not shift section geometry. ".repeat(12)
    );
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    assert_matches_literal_markers(
        &fixture(&body, notes, ""),
        &fixture(
            &body.replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &notes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}

#[test]
fn note_markers_do_not_label_ambiguous_note_part_entries() {
    let body = r#"<w:p><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> First</w:t></w:r></w:p></w:footnote>
        <w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Duplicate</w:t></w:r></w:p></w:footnote>"#;
    assert_matches_literal_markers(
        &fixture(body, notes, ""),
        &fixture(
            &body.replace(r#"<w:footnoteReference w:id="9"/>"#, ""),
            &notes.replace("<w:footnoteRef/>", ""),
            "",
        ),
    );
}

#[test]
fn note_markers_ignore_dangling_and_duplicate_body_references() {
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    for references in [
        r#"<w:footnoteReference w:id="404"/>"#,
        r#"<w:footnoteReference w:id="9"/><w:footnoteReference w:id="9"/>"#,
    ] {
        let body = format!("<w:p><w:r><w:t>Body</w:t>{references}</w:r></w:p>");
        assert_matches_literal_markers(
            &fixture(&body, notes, ""),
            &fixture(
                "<w:p><w:r><w:t>Body</w:t></w:r></w:p>",
                &notes.replace("<w:footnoteRef/>", ""),
                "",
            ),
        );
    }
}

#[test]
fn note_markers_preserve_accepted_alternate_content_and_field_counters() {
    let body = r#"<w:p><w:moveFrom><w:r><w:footnoteReference w:id="7"/></w:r></w:moveFrom>
        <w:r><w:t>First</w:t><mc:AlternateContent><mc:Choice Requires="w"><w:footnoteReference w:id="9"/></mc:Choice><mc:Fallback><w:footnoteReference w:id="7"/></mc:Fallback></mc:AlternateContent></w:r>
        <w:fldSimple w:instr="SEQ item"><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p>
        <w:p><w:fldSimple w:instr="SEQ item"><w:r><w:t>99</w:t></w:r></w:fldSimple><w:ins><w:r><w:footnoteReference w:id="4"/></w:r></w:ins></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> First note</w:t></w:r></w:p></w:footnote>
        <w:footnote w:id="4"><w:p><w:r><w:footnoteRef/><w:t> Second note</w:t></w:r></w:p></w:footnote>"#;
    let literal_notes = notes
        .replacen("<w:footnoteRef/>", "<w:t>1</w:t>", 1)
        .replace("<w:footnoteRef/>", "<w:t>2</w:t>");
    assert_matches_literal_markers(
        &fixture(body, notes, ""),
        &fixture(
            &body
                .replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>")
                .replace(r#"<w:footnoteReference w:id="4"/>"#, "<w:t>2</w:t>"),
            &literal_notes,
            "",
        ),
    );
}

#[test]
fn note_marker_page_breaks_keep_endnote_entry_boundaries_aligned() {
    let body = r#"<w:p><w:r><w:t>Foot</w:t><w:footnoteReference w:id="9"/><w:t> End</w:t><w:endnoteReference w:id="9"/></w:r></w:p>"#;
    let footnotes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:br w:type="page"/><w:t> Footnote</w:t></w:r></w:p></w:footnote>"#;
    let endnotes = r#"<w:endnote w:id="9"><w:p><w:r><w:endnoteRef/><w:t> Endnote</w:t></w:r></w:p></w:endnote>"#;
    assert_matches_literal_markers(
        &fixture_with_endnotes(body, footnotes, endnotes, ""),
        &fixture_with_endnotes(
            &body
                .replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>")
                .replace(r#"<w:endnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &footnotes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            &endnotes.replace("<w:endnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}

#[test]
fn note_markers_do_not_materialize_replaced_complex_field_results() {
    let body = r#"<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText>SEQ item</w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:footnoteReference w:id="9"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>
        <w:p><w:r><w:t>Actual reference</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    assert_matches_literal_markers(
        &fixture(body, notes, ""),
        &fixture(
            &body
                .replacen(r#"<w:footnoteReference w:id="9"/>"#, "", 1)
                .replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &notes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}

#[test]
fn note_markers_ignore_historical_section_numbering() {
    let body = r#"<w:p><w:pPr><w:sectPr><w:sectPrChange w:id="4"><w:sectPr><w:footnotePr><w:numFmt w:val="lowerRoman"/><w:numStart w:val="7"/></w:footnotePr></w:sectPr></w:sectPrChange></w:sectPr></w:pPr><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
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

#[test]
fn note_markers_keep_floating_shape_anchors_aligned() {
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    for wrapping in ["wrapNone", "wrapTopAndBottom"] {
        for marker in [
            r#"<w:footnoteReference w:id="9"/>"#,
            r#"<w:footnoteReference w:id="9"></w:footnoteReference>"#,
        ] {
            let body = format!(
                r#"<w:p><w:r>{marker}<w:br w:type="page"/><w:t>After break</w:t></w:r></w:p>
                <w:p><w:r><w:br w:type="page"/><w:t>Shape anchor</w:t><w:drawing xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:anchor behindDoc="0"><wp:positionV relativeFrom="page"><wp:posOffset>914400</wp:posOffset></wp:positionV><wp:extent cx="914400" cy="457200"/><wp:{wrapping}/><wp:docPr id="73" name="Note-shifted shape"/></wp:anchor></w:drawing><w:t>{}</w:t></w:r></w:p>"#,
                " Following flow text.".repeat(100)
            );
            assert_matches_literal_markers(
                &fixture(&body, notes, ""),
                &fixture(
                    &body.replace(marker, "<w:t>1</w:t>"),
                    &notes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
                    "",
                ),
            );
        }
    }
}

#[test]
fn note_marker_render_view_is_refreshed_after_note_edits() {
    let body = r#"<w:p><w:r><w:t>Body</w:t><w:footnoteReference w:id="9"/></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t>Original note</w:t></w:r></w:p></w:footnote>"#;
    let mut document = Document::open(&fixture(body, notes, "")).unwrap();
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let before = document
        .try_to_pdf_with_fixed_fonts_and_report(&fonts)
        .unwrap();
    assert_eq!(
        document
            .replace_note_text("Original note", "Edited note")
            .unwrap(),
        1
    );
    assert!(
        before.pdf
            == document
                .try_to_pdf_with_fixed_fonts_and_report(&fonts)
                .unwrap()
                .pdf
    );
    document.refresh_read_view().unwrap();
    let edited = document
        .try_to_pdf_with_fixed_fonts_and_report(&fonts)
        .unwrap();
    assert!(before.pdf != edited.pdf);
    let saved = document.save().unwrap();
    let reopened = Document::open(&saved).unwrap();
    assert!(
        edited.pdf
            == reopened
                .try_to_pdf_with_fixed_fonts_and_report(&fonts)
                .unwrap()
                .pdf
    );
    assert_matches_literal_markers(
        &saved,
        &fixture(
            &body.replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &notes
                .replace("Original note", "Edited note")
                .replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
        ),
    );
}

#[test]
fn note_marker_labels_shift_in_paragraph_shape_anchor_offsets() {
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    let settings = r#"<w:footnotePr><w:numStart w:val="123456789"/></w:footnotePr>"#;
    for words in 1..25 {
        let body = format!(
            r#"<w:p><w:pPr><w:widowControl w:val="0"/><w:sectPr><w:pgSz w:w="2400" w:h="1600"/><w:pgMar w:top="200" w:bottom="200" w:left="200" w:right="200"/><w:cols w:num="1"/></w:sectPr></w:pPr><w:r><w:t>{}</w:t><w:footnoteReference w:id="9"/><w:drawing xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"><wp:anchor behindDoc="0"><wp:positionV relativeFrom="page"><wp:posOffset>127000</wp:posOffset></wp:positionV><wp:extent cx="127000" cy="127000"/><wp:wrapNone/><wp:docPr id="74" name="Offset shape"/></wp:anchor></w:drawing><w:t> Tail text for the next line.</w:t></w:r></w:p>"#,
            "Word ".repeat(words)
        );
        assert_matches_literal_markers(
            &fixture(&body, notes, settings),
            &fixture(
                &body.replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>123456789</w:t>"),
                &notes.replace("<w:footnoteRef/>", "<w:t>123456789</w:t>"),
                settings,
            ),
        );
    }
}

#[test]
fn note_marker_render_view_preserves_section_headers() {
    let body = r#"<w:p><w:r><w:footnoteReference w:id="9"/><w:br w:type="page"/><w:t>First section</w:t></w:r></w:p>
        <w:p><w:pPr><w:sectPr><w:headerReference xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" w:type="default" r:id="headerFirst"/><w:type w:val="nextPage"/></w:sectPr></w:pPr><w:r><w:t>First tail</w:t></w:r></w:p>
        <w:p><w:pPr><w:sectPr><w:headerReference xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" w:type="default" r:id="headerSecond"/><w:type w:val="nextPage"/></w:sectPr></w:pPr><w:r><w:t>Second section</w:t></w:r></w:p>
        <w:p><w:r><w:t>Final section</w:t></w:r></w:p>"#;
    let notes = r#"<w:footnote w:id="9"><w:p><w:r><w:footnoteRef/><w:t> Note</w:t></w:r></w:p></w:footnote>"#;
    let extra = [
        (
            "word/_rels/document.xml.rels",
            r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="footnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="headerFirst" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/><Relationship Id="headerSecond" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header2.xml"/></Relationships>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:pPr><w:spacing w:line="600" w:lineRule="exact"/></w:pPr><w:r><w:t>FIRST HEADER</w:t></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:p><w:r><w:t>SECOND HEADER</w:t></w:r></w:p></w:hdr>"#,
        ),
    ];
    let source = fixture_with_extra_parts(body, notes, "", "", &extra);
    assert!(Document::open(&source)
        .unwrap()
        .header_text()
        .contains("FIRST HEADER"));
    assert!(Document::open(&source)
        .unwrap()
        .header_text()
        .contains("SECOND HEADER"));
    assert_matches_literal_markers(
        &source,
        &fixture_with_extra_parts(
            &body.replace(r#"<w:footnoteReference w:id="9"/>"#, "<w:t>1</w:t>"),
            &notes.replace("<w:footnoteRef/>", "<w:t>1</w:t>"),
            "",
            "",
            &extra,
        ),
    );
}
