#![cfg(feature = "docx")]

use std::io::{Read, Write};

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

fn unzip_parts(bytes: &[u8]) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut parts = std::collections::BTreeMap::new();
    for index in 0..zip.len() {
        let mut file = zip.by_index(index).unwrap();
        let mut body = Vec::new();
        file.read_to_end(&mut body).unwrap();
        parts.insert(file.name().to_string(), body);
    }
    parts
}

fn exact_note_paragraphs_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="7"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="9"/></w:r><w:r><w:t> BODY C</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="7"><w:sdt><w:sdtContent><w:p><w:pPr><w:keepNext/><w:keepLines/><w:widowControl w:val="off"/><w:shd w:val="clear" w:fill="DDEEFF"/><w:tabs><w:tab w:val="center" w:pos="720" w:leader="hyphen"/></w:tabs><w:spacing w:line="260" w:lineRule="exact"/><w:ind w:left="240" w:firstLine="120"/></w:pPr><w:r><w:rPr><w:b/><w:highlight w:val="yellow"/></w:rPr><w:t>FOOT A</w:t></w:r><w:r><w:br w:type="column"/><w:t>FOOT B</w:t><w:tab/><w:t>FOOT TAB</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:pPr><w:keepLines/><w:spacing w:line="320" w:lineRule="atLeast"/><w:jc w:val="center"/></w:pPr><w:r><w:rPr><w:i/><w:vertAlign w:val="superscript"/></w:rPr><w:t>FOOT C</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:endnote><w:endnote w:id="9"><w:customXml><w:p><w:pPr><w:widowControl w:val="off"/><w:shd w:val="clear" w:fill="FFEEDD"/><w:tabs><w:tab w:val="right" w:pos="1200" w:leader="dot"/></w:tabs><w:spacing w:line="280" w:lineRule="atLeast"/></w:pPr><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>END A</w:t></w:r><w:r><w:br w:type="column"/><w:t>END B</w:t></w:r></w:p></w:customXml><w:p><w:pPr><w:keepNext/><w:ind w:left="300" w:hanging="100"/><w:jc w:val="right"/></w:pPr><w:r><w:rPr><w:smallCaps/></w:rPr><w:t>END C</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn mixed_supported_and_table_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="7"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="9"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="7"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:t>TABLE NOTE</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="9"><w:p><w:pPr><w:keepNext/><w:spacing w:line="240" w:lineRule="exact"/></w:pPr><w:r><w:t>END ONE</w:t></w:r><w:r><w:br w:type="column"/><w:t>END BREAK</w:t></w:r></w:p><w:p><w:r><w:t>END TWO</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn note_with_marker<'a>(xml: &'a str, item: &str, marker: &str) -> &'a str {
    let marker_offset = xml
        .find(marker)
        .unwrap_or_else(|| panic!("missing note marker {marker:?}: {xml}"));
    let start_marker = format!("<w:{item} w:id=");
    let start = xml[..marker_offset]
        .rfind(&start_marker)
        .unwrap_or_else(|| panic!("missing {item} start for {marker:?}: {xml}"));
    let end_marker = format!("</w:{item}>");
    let end = marker_offset
        + xml[marker_offset..]
            .find(&end_marker)
            .unwrap_or_else(|| panic!("missing {item} end for {marker:?}: {xml}"))
        + end_marker.len();
    &xml[start..end]
}

#[test]
fn opened_docx_exact_note_paragraph_payload_roundtrips_through_fresh_conversion() {
    let document = Document::open(&exact_note_paragraphs_docx()).expect("note fixture opens");
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let footnote = note_with_marker(footnotes, "footnote", "FOOT A");
    let endnote = note_with_marker(endnotes, "endnote", "END A");

    assert_eq!(footnote.matches("<w:p>").count(), 2, "{footnote}");
    assert_eq!(endnote.matches("<w:p>").count(), 2, "{endnote}");
    assert_eq!(
        footnote.matches(r#"<w:br w:type="column"/>"#).count(),
        1,
        "{footnote}"
    );
    assert_eq!(
        endnote.matches(r#"<w:br w:type="column"/>"#).count(),
        1,
        "{endnote}"
    );
    assert!(footnote.contains("<w:keepNext/>"), "{footnote}");
    assert_eq!(footnote.matches("<w:keepLines/>").count(), 2, "{footnote}");
    assert!(footnote.contains(r#"<w:widowControl w:val="0"/>"#));
    assert!(footnote.contains(r#"w:line="260" w:lineRule="exact""#));
    assert!(footnote.contains(r#"w:line="320" w:lineRule="atLeast""#));
    assert!(footnote.contains(r#"w:val="center" w:pos="720" w:leader="hyphen""#));
    assert!(footnote.contains(r#"w:fill="DDEEFF""#));
    assert!(footnote.contains("<w:b/>"));
    assert!(footnote.contains(r#"<w:highlight w:val="yellow"/>"#));
    assert!(footnote.contains("<w:i/>"));
    assert!(footnote.contains(r#"<w:vertAlign w:val="superscript"/>"#));

    assert!(endnote.contains(r#"<w:widowControl w:val="0"/>"#));
    assert!(endnote.contains(r#"w:line="280" w:lineRule="atLeast""#));
    assert!(endnote.contains(r#"w:val="right" w:pos="1200" w:leader="dot""#));
    assert!(endnote.contains(r#"w:fill="FFEEDD""#));
    assert!(endnote.contains("<w:keepNext/>"));
    assert!(endnote.contains(r#"<w:u w:val="single"/>"#));
    assert!(endnote.contains("<w:smallCaps/>"));

    let reopened = Document::open(&converted).expect("converted notes reopen");
    assert_eq!(reopened.model(), normalized_model);
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
}

#[test]
fn opened_docx_table_note_fallback_does_not_disable_supported_sibling_payload() {
    let document =
        Document::open(&mixed_supported_and_table_note_docx()).expect("mixed note fixture opens");
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx());
    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let footnote = note_with_marker(footnotes, "footnote", "TABLE NOTE");
    let endnote = note_with_marker(endnotes, "endnote", "END ONE");

    assert!(!footnote.contains("<w:tbl>"), "{footnote}");
    assert_eq!(footnote.matches("<w:p>").count(), 1, "{footnote}");
    assert_eq!(endnote.matches("<w:p>").count(), 2, "{endnote}");
    assert!(endnote.contains("<w:keepNext/>"), "{endnote}");
    assert!(endnote.contains(r#"w:line="240" w:lineRule="exact""#));
    assert_eq!(
        endnote.matches(r#"<w:br w:type="column"/>"#).count(),
        1,
        "{endnote}"
    );
}
