#![cfg(feature = "docx")]

//! NOTEREF auto-numbering slices driven end-to-end through the public
//! `Document::open(..).fields()` API. Covers custom-mark notes (which are not
//! auto-numbered) and document-level footnote/endnote `numStart`/`numFmt`.

use std::io::Write;

use rwml::{Document, FieldKind};

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

const CONTENT_TYPES_WITH_FOOTNOTES: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/></Types>"#;

const ROOT_RELS: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;

const DOCUMENT_RELS_FOOTNOTES: &str = r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#;

/// footnote #1 normal, footnote #2 with a custom mark (`w:customMarkFollows`),
/// footnote #3 normal. A `NOTEREF` to footnote #3 must skip the custom-mark note
/// in the auto-count and resolve to "2", not "3".
fn custom_mark_note_docx() -> Vec<u8> {
    docx_fixture(&[
        ("[Content_Types].xml", CONTENT_TYPES_WITH_FOOTNOTES),
        ("_rels/.rels", ROOT_RELS),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS_FOOTNOTES),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p><w:p><w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>*</w:t></w:r><w:r><w:footnoteReference w:customMarkFollows="1" w:id="2"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="ThirdNote"/><w:r><w:footnoteReference w:id="3"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF ThirdNote "><w:r><w:t>stale third note</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Custom-mark footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="3"><w:p><w:r><w:t>Third footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
    ])
}

/// Plain two-footnote document: a `NOTEREF` to the second note stays "2".
/// Guards against a regression where the custom-mark handling would mis-count
/// ordinary notes.
fn plain_two_note_docx() -> Vec<u8> {
    docx_fixture(&[
        ("[Content_Types].xml", CONTENT_TYPES_WITH_FOOTNOTES),
        ("_rels/.rels", ROOT_RELS),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS_FOOTNOTES),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:footnoteReference w:id="1"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="SecondNote"/><w:r><w:footnoteReference w:id="2"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF SecondNote "><w:r><w:t>stale second note</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Second footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
    ])
}

#[test]
fn docx_custom_mark_note_does_not_consume_auto_number() {
    let doc = Document::open(&custom_mark_note_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::NoteRef);
    assert_eq!(fields[0].instruction, "NOTEREF ThirdNote");
    // Footnote #2 uses a custom mark, so the third footnote reference is the
    // second auto-numbered note.
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
}

#[test]
fn docx_plain_two_note_document_is_unchanged() {
    let doc = Document::open(&plain_two_note_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::NoteRef);
    assert_eq!(fields[0].instruction, "NOTEREF SecondNote");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
}

const CONTENT_TYPES_WITH_FOOTNOTES_AND_SETTINGS: &str = r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#;

/// A one-footnote document whose `word/settings.xml` sets a footnote `numStart`
/// (and optional `numFmt`). The single `NOTEREF FirstNote` resolves to the
/// document-level start offset formatted with the document-level footnote format.
fn footnote_settings_docx(footnote_pr: &str) -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            CONTENT_TYPES_WITH_FOOTNOTES_AND_SETTINGS,
        ),
        ("_rels/.rels", ROOT_RELS),
        ("word/_rels/document.xml.rels", DOCUMENT_RELS_FOOTNOTES),
        (
            "word/settings.xml",
            &format!(
                r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">{footnote_pr}</w:settings>"#
            ),
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="FirstNote"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF FirstNote "><w:r><w:t>stale first note</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
    ])
}

#[test]
fn docx_footnote_num_start_offsets_first_note_number() {
    let doc = Document::open(&footnote_settings_docx(
        r#"<w:footnotePr><w:numStart w:val="5"/></w:footnotePr>"#,
    ))
    .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::NoteRef);
    assert_eq!(fields[0].instruction, "NOTEREF FirstNote");
    // numStart=5 makes the first footnote number 5.
    assert_eq!(fields[0].computed_result.as_deref(), Some("5"));
}

#[test]
fn docx_footnote_num_fmt_formats_note_number() {
    let doc = Document::open(&footnote_settings_docx(
        r#"<w:footnotePr><w:numStart w:val="5"/><w:numFmt w:val="lowerRoman"/></w:footnotePr>"#,
    ))
    .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::NoteRef);
    assert_eq!(fields[0].instruction, "NOTEREF FirstNote");
    // numStart=5 formatted as lowerRoman is "v".
    assert_eq!(fields[0].computed_result.as_deref(), Some("v"));
}

#[test]
fn docx_footnote_without_settings_starts_at_one() {
    // No numStart/numFmt: the default footnote numbering is unchanged.
    let doc = Document::open(&footnote_settings_docx("")).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::NoteRef);
    assert_eq!(fields[0].instruction, "NOTEREF FirstNote");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
}
