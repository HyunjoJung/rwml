#![cfg(feature = "docx")]

use std::io::Write;

use rdoc::Document;

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

fn unzip_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut parts = std::collections::BTreeMap::new();
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).unwrap();
        let mut bytes = Vec::new();
        use std::io::Read;
        file.read_to_end(&mut bytes).unwrap();
        parts.insert(file.name().to_string(), bytes);
    }
    parts
}

fn plain_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:r><w:t>Other</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn split_run_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Hel</w:t></w:r><w:r><w:t>lo</w:t></w:r><w:r><w:t>!</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn commented_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id=" 7 "/><w:r><w:t>Hello</w:t></w:r><w:commentRangeEnd w:id=" 7 "/><w:r><w:commentReference w:id=" 7 "/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id=" 7 " w:author=" Reviewer " w:initials=" RV " w:date=" 2026-06-24T00:00:00Z "><w:p><w:r><w:t>First </w:t></w:r><w:r><w:t>note</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ])
}

fn comments_with_blank_ids_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id="1"/><w:r><w:t>Valid</w:t></w:r><w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r></w:p><w:p><w:commentRangeStart w:id=" "/><w:r><w:t>Blank</w:t></w:r><w:commentRangeEnd w:id=" "/><w:r><w:commentReference w:id=" "/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="1" w:author="Reviewer"><w:p><w:r><w:t>Valid comment</w:t></w:r></w:p></w:comment><w:comment w:id=" " w:author="Reviewer"><w:p><w:r><w:t>Blank comment</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ])
}

fn threaded_comments_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/><Override PartName="/word/commentsExtended.xml" ContentType="application/vnd.ms-word.commentsExt+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="rId2" Type="http://schemas.microsoft.com/office/2011/relationships/commentsExtended" Target="commentsExtended.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id="1"/><w:r><w:t>Reviewed clause</w:t></w:r><w:commentRangeEnd w:id="1"/><w:r><w:commentReference w:id="1"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"><w:comment w:id="1" w:author="Reviewer"><w:p w14:paraId="11111111"><w:r><w:t>Original note</w:t></w:r></w:p></w:comment><w:comment w:id="2" w:author="Approver"><w:p w14:paraId="22222222"><w:r><w:t>Reply note</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
        (
            "word/commentsExtended.xml",
            r#"<w15:commentsEx xmlns:w15="http://schemas.microsoft.com/office/word/2012/wordml"><w15:commentEx w15:paraId="11111111" w15:done="0"/><w15:commentEx w15:paraId="22222222" w15:paraIdParent="11111111" w15:done="0"/></w15:commentsEx>"#,
        ),
    ])
}

fn revision_wrapped_commented_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id="7"/><w:r><w:t>Direct anchor</w:t></w:r><w:commentRangeEnd w:id="7"/><w:r><w:commentReference w:id="7"/></w:r></w:p><w:ins w:id="20" w:author="Editor"><w:p><w:commentRangeStart w:id="8"/><w:r><w:t>Inserted anchor</w:t></w:r><w:commentRangeEnd w:id="8"/><w:r><w:commentReference w:id="8"/></w:r></w:p></w:ins><w:moveTo w:id="21" w:author="Editor"><w:p><w:commentRangeStart w:id="9"/><w:r><w:t>Moved-to anchor</w:t></w:r><w:commentRangeEnd w:id="9"/><w:r><w:commentReference w:id="9"/></w:r></w:p></w:moveTo><w:del w:id="22" w:author="Editor"><w:p><w:commentRangeStart w:id="10"/><w:r><w:delText>Deleted anchor</w:delText></w:r><w:commentRangeEnd w:id="10"/><w:r><w:commentReference w:id="10"/></w:r></w:p></w:del><w:moveFrom w:id="23" w:author="Editor"><w:p><w:commentRangeStart w:id="11"/><w:r><w:delText>Moved-from anchor</w:delText></w:r><w:commentRangeEnd w:id="11"/><w:r><w:commentReference w:id="11"/></w:r></w:p></w:moveFrom></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="7" w:author="Reviewer"><w:p><w:r><w:t>Direct note</w:t></w:r></w:p></w:comment><w:comment w:id="8" w:author="Reviewer"><w:p><w:r><w:t>Inserted note</w:t></w:r></w:p></w:comment><w:comment w:id="9" w:author="Reviewer"><w:p><w:r><w:t>Moved-to note</w:t></w:r></w:p></w:comment><w:comment w:id="10" w:author="Reviewer"><w:p><w:r><w:t>Deleted note</w:t></w:r></w:p></w:comment><w:comment w:id="11" w:author="Reviewer"><w:p><w:r><w:t>Moved-from note</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ])
}

fn note_commented_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Body</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:p><w:commentRangeStart w:id="7"/><w:r><w:t>Foot anchor</w:t></w:r><w:commentRangeEnd w:id="7"/><w:r><w:commentReference w:id="7"/></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:p><w:commentRangeStart w:id="8"/><w:r><w:t>End anchor</w:t></w:r><w:commentRangeEnd w:id="8"/><w:r><w:commentReference w:id="8"/></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="7" w:author="Reviewer"><w:p><w:r><w:t>Foot note</w:t></w:r></w:p></w:comment><w:comment w:id="8" w:author="Reviewer"><w:p><w:r><w:t>End note</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ])
}

#[test]
fn docx_comment_replies_use_comments_extended_parent_ids() {
    let doc = Document::open(&threaded_comments_docx()).expect("fixture opens");
    let comments = doc.comments();

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].id, "1");
    assert_eq!(comments[0].parent_comment_id, None);
    assert_eq!(comments[1].id, "2");
    assert_eq!(comments[1].parent_comment_id.as_deref(), Some("1"));
    assert_eq!(comments[1].text, "Reply note");
}

#[test]
fn docx_comments_are_extracted() {
    let doc = Document::open(&commented_docx()).expect("fixture opens");
    let comments = doc.comments();

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "7");
    assert_eq!(comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(comments[0].initials.as_deref(), Some("RV"));
    assert_eq!(comments[0].date.as_deref(), Some("2026-06-24T00:00:00Z"));
    assert_eq!(comments[0].text, "First note");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello")
    );
}

#[test]
fn docx_comments_ignore_blank_ids() {
    let doc = Document::open(&comments_with_blank_ids_docx()).expect("fixture opens");
    let comments = doc.comments();

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "1");
    assert_eq!(comments[0].text, "Valid comment");
    assert_eq!(
        comments[0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.text.as_str()),
        Some("Valid")
    );
}

#[test]
fn docx_comment_anchors_follow_accepted_revision_view() {
    let doc = Document::open(&revision_wrapped_commented_docx()).expect("fixture opens");

    assert_eq!(
        doc.main_text(),
        "Direct anchor\nInserted anchor\nMoved-to anchor"
    );
    let comments = doc.comments();
    assert_eq!(comments.len(), 5);
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Direct anchor")
    );
    assert_eq!(
        comments[1].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Inserted anchor")
    );
    assert_eq!(
        comments[2].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Moved-to anchor")
    );
    assert!(comments[3].anchor.is_none());
    assert!(comments[4].anchor.is_none());
}

#[test]
fn docx_note_comment_anchors_are_exposed() {
    let doc = Document::open(&note_commented_docx()).expect("fixture opens");
    let comments = doc.comments();

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].text, "Foot note");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Foot anchor")
    );
    assert_eq!(comments[1].text, "End note");
    assert_eq!(
        comments[1].anchor.as_ref().map(|a| a.text.as_str()),
        Some("End anchor")
    );
}

#[test]
fn docx_comments_preserve_tabs_and_breaks_in_text_and_anchor() {
    let docx = docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id="4"/><w:r><w:t>Hello</w:t><w:tab/><w:t>anchor</w:t><w:br/><w:t>end</w:t></w:r><w:commentRangeEnd w:id="4"/><w:r><w:commentReference w:id="4"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="4" w:author="Reviewer"><w:p><w:r><w:t>Line 1</w:t><w:br/><w:t>Line 2</w:t><w:tab/><w:t>Cell</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ]);
    let doc = Document::open(&docx).expect("fixture opens");
    let comments = doc.comments();

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Line 1\nLine 2\tCell");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello\tanchor\nend")
    );
}

#[test]
fn set_comment_text_updates_existing_comment_body() {
    let mut doc = Document::open(&commented_docx()).expect("fixture opens");

    doc.set_comment_text("7", "Updated note")
        .expect("comment text updates");

    assert_eq!(doc.edited_parts(), ["word/comments.xml"]);
    let saved = doc.save().expect("save edited docx");
    let mut reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();

    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "7");
    assert_eq!(comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(comments[0].initials.as_deref(), Some("RV"));
    assert_eq!(comments[0].date.as_deref(), Some("2026-06-24T00:00:00Z"));
    assert_eq!(comments[0].text, "Updated note");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello")
    );
    assert!(
        reopened.set_comment_text("missing", "x").is_err(),
        "missing comment id should be an error"
    );
}

#[test]
fn set_comment_text_skips_deleted_comment_body_text() {
    let fixture = docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:commentRangeStart w:id="7"/><w:r><w:t>Hello</w:t></w:r><w:commentRangeEnd w:id="7"/><w:r><w:commentReference w:id="7"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="7" w:author="Reviewer"><w:p><w:del w:id="1"><w:r><w:delText>OLD</w:delText></w:r></w:del><w:moveFrom w:id="2"><w:r><w:t>OLD</w:t></w:r></w:moveFrom><w:r><w:t>OLD</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
    ]);
    let mut doc = Document::open(&fixture).expect("fixture opens");
    assert_eq!(doc.comments()[0].text, "OLD");

    doc.set_comment_text("7", "Updated")
        .expect("comment text updates");

    let saved = doc.save().expect("save edited docx");
    let comments_xml = String::from_utf8(unzip_parts(&saved)["word/comments.xml"].clone()).unwrap();
    assert!(
        comments_xml.contains(r#"<w:del w:id="1"><w:r><w:delText>OLD</w:delText></w:r></w:del>"#),
        "deleted comment text changed: {comments_xml}"
    );
    assert!(
        comments_xml.contains(r#"<w:moveFrom w:id="2"><w:r><w:t>OLD</w:t></w:r></w:moveFrom>"#),
        "moved-from comment text changed: {comments_xml}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    assert_eq!(reopened.comments()[0].text, "Updated");
}

#[test]
fn set_comment_text_writes_tabs_and_breaks_as_markers() {
    let mut doc = Document::open(&commented_docx()).expect("fixture opens");

    doc.set_comment_text("7", "Line 1\nLine\t2")
        .expect("comment text updates");

    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let comments = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    assert!(
        comments.contains(r#"<w:t>Line 1</w:t><w:br/><w:t>Line</w:t><w:tab/><w:t>2</w:t>"#),
        "updated comment text should encode tabs and breaks as WML markers: {comments}"
    );
    assert!(
        !comments.contains("Line 1\nLine\t2"),
        "updated comment text should not keep raw tab/break characters in one w:t: {comments}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Line 1\nLine\t2");
}

#[test]
fn add_comment_on_text_creates_comments_part_relationship_and_anchor() {
    let mut doc = Document::open(&plain_docx()).expect("fixture opens");

    let id = doc
        .add_comment_on_text("Hello", "Note <one> & two", "Reviewer")
        .expect("comment added");

    assert_eq!(id, "0");
    assert_eq!(
        doc.edited_parts(),
        [
            "[Content_Types].xml",
            "word/_rels/document.xml.rels",
            "word/comments.xml",
            "word/document.xml"
        ]
    );
    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let body = String::from_utf8(parts["word/document.xml"].clone()).unwrap();
    let comments = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    let rels = String::from_utf8(parts["word/_rels/document.xml.rels"].clone()).unwrap();
    let ct = String::from_utf8(parts["[Content_Types].xml"].clone()).unwrap();

    let start = body.find(r#"<w:commentRangeStart"#).unwrap_or(usize::MAX);
    let anchor = body.find(r#"<w:t>Hello</w:t>"#).unwrap_or(usize::MAX);
    let end = body.find(r#"<w:commentRangeEnd"#).unwrap_or(usize::MAX);
    let reference = body.find(r#"<w:commentReference"#).unwrap_or(usize::MAX);
    assert!(
        start < anchor && anchor < end && end < reference && body.contains(r#"w:id="0""#),
        "body anchor missing or out of order: {body}"
    );
    assert!(
        comments.contains(r#"<w:comment "#)
            && comments.contains(r#"w:id="0""#)
            && comments.contains(r#"w:author="Reviewer""#),
        "comment metadata missing: {comments}"
    );
    assert!(
        comments.contains(r#"<w:t>Note &lt;one&gt; &amp; two</w:t>"#),
        "comment text not escaped: {comments}"
    );
    assert!(
        rels.contains(
            r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml""#
        ),
        "comments relationship missing: {rels}"
    );
    assert!(
        ct.contains(r#"PartName="/word/comments.xml""#)
            && ct.contains("wordprocessingml.comments+xml"),
        "comments content type missing: {ct}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].id, "0");
    assert_eq!(comments[0].author.as_deref(), Some("Reviewer"));
    assert_eq!(comments[0].text, "Note <one> & two");
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello")
    );
}

#[test]
fn add_comment_on_text_preserves_edge_whitespace() {
    let mut doc = Document::open(&plain_docx()).expect("fixture opens");

    doc.add_comment_on_text("Hello", " Padded note ", "Reviewer")
        .expect("comment added");

    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let comments = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    assert!(
        comments.contains(r#"<w:t xml:space="preserve"> Padded note </w:t>"#),
        "comment text should preserve edge whitespace: {comments}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, " Padded note ");
}

#[test]
fn add_comment_on_text_writes_tabs_and_breaks_as_markers() {
    let mut doc = Document::open(&plain_docx()).expect("fixture opens");

    doc.add_comment_on_text("Hello", "Line 1\nLine\t2", "Reviewer")
        .expect("comment added");

    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let comments = String::from_utf8(parts["word/comments.xml"].clone()).unwrap();
    assert!(
        comments.contains(r#"<w:t>Line 1</w:t><w:br/><w:t>Line</w:t><w:tab/><w:t>2</w:t>"#),
        "comment text should encode tabs and breaks as WML markers: {comments}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(comments[0].text, "Line 1\nLine\t2");
}

#[test]
fn add_comment_on_text_can_anchor_across_adjacent_runs() {
    let mut doc = Document::open(&split_run_docx()).expect("fixture opens");

    let id = doc
        .add_comment_on_text("Hello", "Split note", "Reviewer")
        .expect("comment added across split runs");

    assert_eq!(id, "0");
    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let body = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    let start = body.find(r#"<w:commentRangeStart"#).unwrap_or(usize::MAX);
    let first = body.find(r#"<w:t>Hel</w:t>"#).unwrap_or(usize::MAX);
    let second = body.find(r#"<w:t>lo</w:t>"#).unwrap_or(usize::MAX);
    let end = body.find(r#"<w:commentRangeEnd"#).unwrap_or(usize::MAX);
    let reference = body.find(r#"<w:commentReference"#).unwrap_or(usize::MAX);
    let tail = body.find(r#"<w:t>!</w:t>"#).unwrap_or(usize::MAX);
    assert!(
        start < first && first < second && second < end && end < reference && reference < tail,
        "split-run comment anchor missing or misplaced: {body}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();
    assert_eq!(comments.len(), 1);
    assert_eq!(
        comments[0].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello")
    );
}

#[test]
fn add_comment_on_text_uses_next_comment_id_and_preserves_existing_comments() {
    let mut doc = Document::open(&commented_docx()).expect("fixture opens");

    let id = doc
        .add_comment_on_text("Hello", "Second note", "Reviewer 2")
        .expect("comment added");

    assert_eq!(id, "8");
    assert_eq!(
        doc.edited_parts(),
        ["word/comments.xml", "word/document.xml"]
    );
    let saved = doc.save().expect("save edited docx");
    let reopened = Document::open(&saved).expect("reopen edited docx");
    let comments = reopened.comments();

    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].id, "7");
    assert_eq!(comments[0].text, "First note");
    assert_eq!(comments[1].id, "8");
    assert_eq!(comments[1].author.as_deref(), Some("Reviewer 2"));
    assert_eq!(comments[1].text, "Second note");
    assert_eq!(
        comments[1].anchor.as_ref().map(|a| a.text.as_str()),
        Some("Hello")
    );

    let mut missing = reopened;
    assert!(
        missing
            .add_comment_on_text("Missing anchor", "Nope", "Reviewer")
            .is_err(),
        "missing anchor text should be an error"
    );
}
