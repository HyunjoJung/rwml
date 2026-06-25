#![cfg(feature = "docx")]

use std::io::Write;

use rdoc::{Document, RevisionKind, RevisionView};

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

fn revised_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="1" w:author="Alice" w:date="2026-06-24T01:00:00Z"><w:r><w:t>added</w:t></w:r></w:ins><w:del w:id="2" w:author="Bob"><w:r><w:delText>removed</w:delText></w:r></w:del><w:moveFrom w:id="3"><w:r><w:delText>from</w:delText></w:r></w:moveFrom><w:moveTo w:id="4"><w:r><w:t>to</w:t></w:r></w:moveTo></w:p></w:body></w:document>"#,
        ),
    ])
}

fn block_level_revised_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t></w:r></w:p><w:ins w:id="10" w:author="Alice"><w:p><w:r><w:t>Inserted block</w:t></w:r></w:p></w:ins><w:moveTo w:id="11" w:author="Casey"><w:p><w:r><w:t>Moved-to block</w:t></w:r></w:p></w:moveTo><w:del w:id="12" w:author="Bob"><w:p><w:r><w:delText>Deleted block</w:delText></w:r></w:p></w:del><w:moveFrom w:id="13" w:author="Dana"><w:p><w:r><w:delText>Moved-from block</w:delText></w:r></w:p></w:moveFrom><w:p><w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

#[test]
fn docx_revisions_are_extracted() {
    let doc = Document::open(&revised_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(revisions.len(), 4);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].id.as_deref(), Some("1"));
    assert_eq!(revisions[0].author.as_deref(), Some("Alice"));
    assert_eq!(revisions[0].date.as_deref(), Some("2026-06-24T01:00:00Z"));
    assert_eq!(revisions[0].text, "added");

    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].id.as_deref(), Some("2"));
    assert_eq!(revisions[1].author.as_deref(), Some("Bob"));
    assert_eq!(revisions[1].text, "removed");

    assert_eq!(revisions[2].kind, RevisionKind::MoveFrom);
    assert_eq!(revisions[2].text, "from");
    assert_eq!(revisions[3].kind, RevisionKind::MoveTo);
    assert_eq!(revisions[3].text, "to");
}

#[test]
fn docx_revision_views_extract_accepted_original_and_annotated_text() {
    let doc = Document::open(&revised_docx()).expect("fixture opens");

    assert_eq!(doc.main_text(), "addedto");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "added to"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "removed from"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+added] [-removed] [~from->] [~->to]"
    );
}

#[test]
fn docx_block_level_current_revision_paragraphs_are_accepted_in_body_model() {
    let doc = Document::open(&block_level_revised_docx()).expect("fixture opens");

    assert_eq!(
        doc.main_text(),
        "Before\nInserted block\nMoved-to block\nAfter"
    );
    assert!(!doc.main_text().contains("Deleted block"));
    assert!(!doc.main_text().contains("Moved-from block"));

    let blocks = &doc.model().blocks;
    assert_eq!(blocks.len(), 4);
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Before Inserted block Moved-to block After"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Before Deleted block Moved-from block After"
    );
}
