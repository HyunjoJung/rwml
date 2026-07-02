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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id=" 1 " w:author=" Alice " w:date=" 2026-06-24T01:00:00Z "><w:r><w:t>added</w:t></w:r></w:ins><w:del w:id="2" w:author="Bob"><w:r><w:delText>removed</w:delText></w:r></w:del><w:moveFrom w:id="3"><w:r><w:delText>from</w:delText></w:r></w:moveFrom><w:moveTo w:id="4"><w:r><w:t>to</w:t></w:r></w:moveTo></w:p></w:body></w:document>"#,
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

fn multi_paragraph_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Before</w:t></w:r></w:p><w:ins w:id="20" w:author="Alice"><w:p><w:r><w:t>First</w:t></w:r></w:p><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:ins><w:p><w:r><w:t>After</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn simple_field_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="30" w:author="Alice"><w:fldSimple w:instr=" QUOTE &quot;Fresh added&quot; "><w:r><w:t>stale added</w:t></w:r></w:fldSimple></w:ins><w:del w:id="31" w:author="Bob"><w:fldSimple w:instr=" QUOTE &quot;Fresh removed&quot; "><w:r><w:delText>stale removed</w:delText></w:r></w:fldSimple></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn complex_field_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="32" w:author="Alice"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> QUOTE &quot;Fresh added&quot; </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale added</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:ins><w:del w:id="33" w:author="Bob"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> QUOTE &quot;Fresh removed&quot; </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>stale removed</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn dynamic_field_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="34" w:author="Alice"><w:fldSimple w:instr=" IF 1 = 1 &quot;Fresh added&quot; &quot;stale branch&quot; "><w:r><w:t>stale added</w:t></w:r></w:fldSimple></w:ins><w:del w:id="35" w:author="Bob"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> IF 2 &gt; 1 &quot;Fresh removed&quot; &quot;stale branch&quot; </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>stale removed</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn local_field_bookmark_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="36" w:author="Alice"><w:fldSimple w:instr=" SET Client &quot;Acme&quot; "/><w:fldSimple w:instr=" IF Client = &quot;Acme&quot; &quot;Fresh added&quot; &quot;stale branch&quot; "><w:r><w:t>stale added</w:t></w:r></w:fldSimple></w:ins></w:p></w:body></w:document>"#,
        ),
    ])
}

fn display_action_field_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="37" w:author="Alice"><w:fldSimple w:instr=" SYMBOL 183 \f Symbol "><w:r><w:t>stale insert symbol</w:t></w:r></w:fldSimple></w:ins><w:del w:id="38" w:author="Bob"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MACROBUTTON RunReport &quot;Fresh removed&quot; </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>stale action text</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn marker_field_revision_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="39" w:author="Alice"><w:fldSimple w:instr=" TC &quot;Inserted entry&quot; "><w:r><w:t>stale insert marker</w:t></w:r></w:fldSimple><w:r><w:t>added</w:t></w:r></w:ins><w:del w:id="40" w:author="Bob"><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> XE &quot;Deleted index&quot; </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:delText>stale delete marker</w:delText></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:delText>removed</w:delText></w:r></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn note_revised_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFootnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEndnotes" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Main</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:ins w:id="7" w:author="Editor" w:date="2026-06-24T00:00:00Z"><w:r><w:t>Foot added</w:t></w:r></w:ins><w:del w:id="8"><w:r><w:delText>Foot removed</w:delText></w:r></w:del></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:p><w:moveFrom w:id="9"><w:r><w:delText>End moved from</w:delText></w:r></w:moveFrom><w:moveTo w:id="10"><w:r><w:t>End moved to</w:t></w:r></w:moveTo></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn alternate_content_revised_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><mc:AlternateContent><mc:Choice Requires="w14"><w:ins w:id="1" w:author="Alice"><w:r><w:t>Choice insert</w:t></w:r></w:ins></mc:Choice><mc:Fallback><w:ins w:id="2" w:author="Bob"><w:r><w:t>Fallback insert</w:t></w:r></w:ins></mc:Fallback></mc:AlternateContent><w:ins w:id="3"><w:r><mc:AlternateContent><mc:Choice Requires="w14"><w:t>Choice inner</w:t></mc:Choice><mc:Fallback><w:t>Fallback inner</w:t></mc:Fallback></mc:AlternateContent></w:r></w:ins></w:p></w:body></w:document>"#,
        ),
    ])
}

fn inline_marker_revised_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:ins w:id="5"><w:r><w:t>Col1</w:t><w:tab/><w:t>Col2</w:t><w:br/><w:t>No</w:t><w:noBreakHyphen/><w:t>Break</w:t><w:softHyphen/><w:t>Soft</w:t></w:r></w:ins><w:del w:id="6"><w:r><w:delText>Old</w:delText><w:tab/><w:delText>Text</w:delText><w:cr/><w:delText>End</w:delText><w:softHyphen/><w:delText>Soft</w:delText></w:r></w:del></w:p></w:body></w:document>"#,
        ),
    ])
}

fn unchanged_inline_marker_revision_view_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Lead</w:t><w:tab/><w:t>Tab</w:t><w:br/><w:t>Line</w:t><w:br w:type="page"/><w:t>Page</w:t><w:noBreakHyphen/><w:t>Hard</w:t><w:softHyphen/><w:t>Soft</w:t></w:r><w:ins w:id="7"><w:r><w:t>Added</w:t></w:r></w:ins><w:del w:id="8"><w:r><w:delText>Removed</w:delText></w:r></w:del></w:p></w:body></w:document>"#,
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

#[test]
fn docx_note_revisions_are_exposed() {
    let doc = Document::open(&note_revised_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(revisions.len(), 4);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].id.as_deref(), Some("7"));
    assert_eq!(revisions[0].author.as_deref(), Some("Editor"));
    assert_eq!(revisions[0].date.as_deref(), Some("2026-06-24T00:00:00Z"));
    assert_eq!(revisions[0].text, "Foot added");

    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].id.as_deref(), Some("8"));
    assert_eq!(revisions[1].text, "Foot removed");

    assert_eq!(revisions[2].kind, RevisionKind::MoveFrom);
    assert_eq!(revisions[2].id.as_deref(), Some("9"));
    assert_eq!(revisions[2].text, "End moved from");
    assert_eq!(revisions[3].kind, RevisionKind::MoveTo);
    assert_eq!(revisions[3].id.as_deref(), Some("10"));
    assert_eq!(revisions[3].text, "End moved to");

    assert_eq!(doc.footnote_text(), "Foot added");
    assert_eq!(doc.endnote_text(), "End moved to");
}

#[test]
fn docx_revision_text_preserves_paragraph_boundaries() {
    let doc = Document::open(&multi_paragraph_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "Before\nFirst\nSecond\nAfter");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "First\nSecond");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Before First\nSecond After"
    );
}

#[test]
fn docx_revision_text_uses_computed_simple_field_text() {
    let doc = Document::open(&simple_field_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "Fresh added");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "Fresh added");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "Fresh removed");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Fresh added"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Fresh removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+Fresh added] [-Fresh removed]"
    );
}

#[test]
fn docx_revision_text_uses_computed_complex_field_text() {
    let doc = Document::open(&complex_field_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "Fresh added");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "Fresh added");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "Fresh removed");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Fresh added"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Fresh removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+Fresh added] [-Fresh removed]"
    );
}

#[test]
fn docx_revision_text_uses_computed_dynamic_field_text() {
    let doc = Document::open(&dynamic_field_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "Fresh added");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "Fresh added");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "Fresh removed");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Fresh added"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Fresh removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+Fresh added] [-Fresh removed]"
    );
}

#[test]
fn docx_revision_text_uses_local_field_bookmarks_in_dynamic_field_text() {
    let doc = Document::open(&local_field_bookmark_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "Fresh added");
    assert_eq!(revisions.len(), 1);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "Fresh added");
}

#[test]
fn docx_revision_text_uses_supported_display_and_action_field_text() {
    let doc = Document::open(&display_action_field_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "•");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "•");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "Fresh removed");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "•"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Fresh removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+•] [-Fresh removed]"
    );
}

#[test]
fn docx_revision_text_hides_supported_toc_and_index_marker_field_text() {
    let doc = Document::open(&marker_field_revision_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(doc.main_text(), "added");
    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "added");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "removed");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "added"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+added] [-removed]"
    );
}

#[test]
fn docx_revisions_use_first_alternate_content_branch() {
    let doc = Document::open(&alternate_content_revised_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].id.as_deref(), Some("1"));
    assert_eq!(revisions[0].author.as_deref(), Some("Alice"));
    assert_eq!(revisions[0].text, "Choice insert");

    assert_eq!(revisions[1].kind, RevisionKind::Insertion);
    assert_eq!(revisions[1].id.as_deref(), Some("3"));
    assert_eq!(revisions[1].text, "Choice inner");

    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Choice insert Choice inner"
    );
    assert_eq!(doc.main_text_with_revision_view(RevisionView::Original), "");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "[+Choice insert] [+Choice inner]"
    );
}

#[test]
fn docx_revision_text_preserves_inline_markers() {
    let doc = Document::open(&inline_marker_revised_docx()).expect("fixture opens");
    let revisions = doc.revisions();

    assert_eq!(revisions.len(), 2);
    assert_eq!(revisions[0].kind, RevisionKind::Insertion);
    assert_eq!(revisions[0].text, "Col1\tCol2\nNo-Break\u{00ad}Soft");
    assert_eq!(revisions[1].kind, RevisionKind::Deletion);
    assert_eq!(revisions[1].text, "Old\tText\nEnd\u{00ad}Soft");
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Col1\tCol2\nNo-Break\u{00ad}Soft"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Old\tText\nEnd\u{00ad}Soft"
    );
}

#[test]
fn docx_revision_views_preserve_unchanged_inline_markers() {
    let doc = Document::open(&unchanged_inline_marker_revision_view_docx()).expect("fixture opens");

    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Accepted),
        "Lead\tTab\nLine\nPage-Hard\u{00ad}Soft Added"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Original),
        "Lead\tTab\nLine\nPage-Hard\u{00ad}Soft Removed"
    );
    assert_eq!(
        doc.main_text_with_revision_view(RevisionView::Annotated),
        "Lead\tTab\nLine\nPage-Hard\u{00ad}Soft [+Added] [-Removed]"
    );
}
