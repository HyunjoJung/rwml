#![cfg(feature = "docx")]

use std::io::{Read, Write};

use rdoc::{
    Block, Document, FieldEvaluationReason, FieldEvaluationReasonCount, FieldKind, FieldKindCount,
};

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
        let mut buf = Vec::new();
        file.read_to_end(&mut buf).unwrap();
        parts.insert(file.name().to_string(), buf);
    }
    parts
}

fn field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-3&quot; "><w:r><w:t>Contents</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>Figure 1</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com&quot; "><w:r><w:t>Example</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>custom</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType=" begin "/></w:r><w:r><w:instrText> FILENAME \p </w:instrText></w:r><w:r><w:fldChar w:fldCharType=" separate "/></w:r><w:r><w:t>report.docx</w:t></w:r><w:r><w:fldChar w:fldCharType=" end "/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn revision_wrapped_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" MERGEFIELD DirectName "><w:r><w:t>direct name</w:t></w:r></w:fldSimple></w:p><w:ins w:id="20" w:author="Editor"><w:p><w:fldSimple w:instr=" MERGEFIELD InsertedName "><w:r><w:t>inserted name</w:t></w:r></w:fldSimple></w:p></w:ins><w:moveTo w:id="21" w:author="Editor"><w:p><w:fldSimple w:instr=" MERGEFIELD MovedToName "><w:r><w:t>moved-to name</w:t></w:r></w:fldSimple></w:p></w:moveTo><w:del w:id="22" w:author="Editor"><w:p><w:fldSimple w:instr=" CUSTOM DeletedField "><w:r><w:delText>deleted field</w:delText></w:r></w:fldSimple></w:p></w:del><w:moveFrom w:id="23" w:author="Editor"><w:p><w:fldSimple w:instr=" CUSTOM MovedFromField "><w:r><w:delText>moved-from field</w:delText></w:r></w:fldSimple></w:p></w:moveFrom></w:body></w:document>"#,
        ),
    ])
}

fn alternate_content_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:fldSimple w:instr=" MERGEFIELD AltClient "><w:r><w:t>Choice Client</w:t></w:r></w:fldSimple></mc:Choice><mc:Fallback><w:fldSimple w:instr=" MERGEFIELD AltClient "><w:r><w:t>Fallback Client</w:t></w:r></w:fldSimple></mc:Fallback></mc:AlternateContent></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="4" w:fmt="decimalZero"/></w:sectPr></w:pPr></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>stale restart decimal zero</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale restart arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGE \* CardText \* Upper "><w:r><w:t>stale restart card upper</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:t>Visible before ambiguous break.</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>cached ambiguous page</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Rendered page lead.</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE \* roman </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale rendered roman page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_field_visible_intro_section_page_number_restart_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Cover text can auto-paginate before the section break.</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="7"/></w:sectPr></w:pPr></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>stale restarted current page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_field_page_break_before_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Intro text before the break-before paragraph.</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale break-before page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn empty_section_type_page_accounting_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="DefaultTyped"/><w:r><w:t>Default typed target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>stale page</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DefaultTyped \h "><w:r><w:t>stale ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn disabled_page_break_before_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pageBreakBefore w:val="0"/></w:pPr><w:r><w:t>No forced break.</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn wrapped_page_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:ins><w:p><w:r><w:lastRenderedPageBreak/><w:t>Inserted page two.</w:t></w:r></w:p></w:ins><w:ins><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE \* Arabic </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale inserted page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:ins><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:lastRenderedPageBreak/></mc:Choice><mc:Fallback><w:lastRenderedPageBreak/></mc:Fallback></mc:AlternateContent></w:r><w:r><w:t>Alternate page three.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGE \* Ordinal "><w:r><w:t>stale alternate page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn merge_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" MERGEFIELD  client-name  \* MERGEFORMAT "><w:r><w:t>Acme</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MERGEFIELD &quot;project-name&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Roadmap</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn sequence_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure one</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure two</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \r 7 "><w:r><w:t>stale figure reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \c "><w:r><w:t>stale figure current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \h "><w:r><w:t>stale hidden figure</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure after hidden</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \r -1 "><w:r><w:t>cached invalid reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure after invalid reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Appendix \* roman "><w:r><w:t>stale appendix roman</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn sequence_heading_reset_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure one</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \s 1 "><w:r><w:t>cached heading reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>stale figure two</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn document_info_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" DATE \@ &quot;yyyy-MM-dd&quot; "><w:r><w:t>2026-06-24</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TIME \@ &quot;HH:mm&quot; "><w:r><w:t>14:35</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTHOR \* MERGEFORMAT "><w:r><w:t>Hyunjo Jung</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Company&quot; "><w:r><w:t>Example Co.</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>12</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EDITTIME "><w:r><w:t>42</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Broken Name "><w:r><w:t>cached broken property</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn file_size_switch_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FILESIZE "><w:r><w:t>stale bytes</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILESIZE \k "><w:r><w:t>stale kilobytes</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILESIZE \m "><w:r><w:t>stale megabytes</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn user_info_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" USERNAME "><w:r><w:t>cached user name</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" USERINITIALS "><w:r><w:t>cached initials</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" USERADDRESS "><w:r><w:t>cached address</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" USERNAME &quot;Casey Reviewer&quot; \* Upper "><w:r><w:t>stale override name</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" USERINITIALS &quot;cr&quot; \* Upper "><w:r><w:t>stale override initials</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" USERADDRESS &quot;Review desk, Seoul&quot; \* Upper "><w:r><w:t>stale override address</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn document_info_package_properties_field_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/><Override PartName="/docProps/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.custom-properties+xml"/><Override PartName="/docProps/app.xml" ContentType="application/vnd.openxmlformats-officedocument.extended-properties+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rIdCore" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/><Relationship Id="rIdCustom" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" Target="docProps/custom.xml"/><Relationship Id="rIdApp" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/extended-properties" Target="docProps/app.xml"/></Relationships>"#,
        ),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>Quarter Plan</dc:title><dc:subject>Launch</dc:subject><dc:creator>Hyunjo Jung</dc:creator><dc:description>Field coverage</dc:description><cp:keywords>rdoc,fields</cp:keywords><cp:category>Operations</cp:category><cp:contentStatus>Draft</cp:contentStatus><cp:lastModifiedBy>Reviewer</cp:lastModifiedBy><dcterms:created>2026-06-01T02:03:04Z</dcterms:created><dcterms:modified>2026-06-02T03:04:05Z</dcterms:modified><cp:lastPrinted>2026-06-03T04:05:06Z</cp:lastPrinted><cp:version>1.2</cp:version></cp:coreProperties>"#,
        ),
        (
            "docProps/custom.xml",
            r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name=" Client Name "><vt:lpwstr>acme launch</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="RiskScore"><vt:i4>7</vt:i4></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="4" name="Review Date"><vt:filetime>2026-06-15T09:10:11Z</vt:filetime></property></Properties>"#,
        ),
        (
            "docProps/app.xml",
            r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/extended-properties"><Application>rdoc field engine</Application><AppVersion>16.0000</AppVersion><Template>Normal.dotm</Template><TotalTime>42</TotalTime><Pages>12</Pages><Words>321</Words><Characters>2048</Characters><CharactersWithSpaces>2500</CharactersWithSpaces><Company>Example Co</Company><Manager>Document Lead</Manager><HyperlinkBase>https://docs.example/base/</HyperlinkBase><DocSecurity>4</DocSecurity><LinksUpToDate>true</LinksUpToDate></Properties>"#,
        ),
        (
            "word/settings.xml",
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:docVars><w:docVar w:name=" ClientCode " w:val="alpha-42"/></w:docVars></w:settings>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" TITLE "><w:r><w:t>stale title</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTHOR "><w:r><w:t>stale author</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;Title&quot; \* Upper "><w:r><w:t>stale info title</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Subject&quot; \* Upper "><w:r><w:t>stale subject</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Comments&quot; \* FirstCap "><w:r><w:t>stale comments</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY Keywords "><w:r><w:t>stale keywords</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY Category "><w:r><w:t>stale category</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;ContentStatus&quot; "><w:r><w:t>stale content status</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY Version "><w:r><w:t>stale version</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Client Name&quot; \* Caps "><w:r><w:t>stale client</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY RiskScore "><w:r><w:t>stale score</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCVARIABLE ClientCode \* Upper "><w:r><w:t>stale variable</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CREATEDATE \@ &quot;dddd, MMMM d, yyyy&quot; "><w:r><w:t>stale created date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SAVEDATE \@ &quot;MMM dd, yyyy HH:mm:ss&quot; "><w:r><w:t>stale saved date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINTDATE \@ &quot;yy-M-d h:mm AM/PM&quot; "><w:r><w:t>stale print date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NUMPAGES "><w:r><w:t>stale pages</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NUMWORDS "><w:r><w:t>stale words</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NUMCHARS "><w:r><w:t>stale chars</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EDITTIME "><w:r><w:t>stale edit time</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TEMPLATE \* Upper "><w:r><w:t>stale template</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY Pages "><w:r><w:t>stale docproperty pages</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;Words&quot; "><w:r><w:t>stale info words</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY CharactersWithSpaces "><w:r><w:t>stale chars with spaces</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;TotalTime&quot; "><w:r><w:t>stale info total time</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY Company \* Upper "><w:r><w:t>stale company</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;Manager&quot; "><w:r><w:t>stale manager</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LASTSAVEDBY "><w:r><w:t>stale editor</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CATEGORY \* Upper "><w:r><w:t>stale direct category</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CONTENTSTATUS "><w:r><w:t>stale direct content status</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" VERSION "><w:r><w:t>stale direct version</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY HyperlinkBase "><w:r><w:t>stale hyperlink base</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INFO &quot;DocSecurity&quot; "><w:r><w:t>stale doc security</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY LinksUpToDate "><w:r><w:t>stale links up to date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CREATOR \* Upper "><w:r><w:t>stale creator alias</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DESCRIPTION \* Upper "><w:r><w:t>stale description alias</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" KEYWORD \* Upper "><w:r><w:t>stale keyword alias</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LASTMODIFIEDBY \* Upper "><w:r><w:t>stale modified alias</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" APPLICATION \* Upper "><w:r><w:t>stale application</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" APPVERSION "><w:r><w:t>stale app version</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MANAGER \* Upper "><w:r><w:t>stale direct manager</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPANY \* Upper "><w:r><w:t>stale direct company</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HYPERLINKBASE "><w:r><w:t>stale direct hyperlink base</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCSECURITY "><w:r><w:t>stale direct doc security</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LINKSUPTODATE "><w:r><w:t>stale direct links up to date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Review Date&quot; \@ &quot;MMM d, yyyy&quot; "><w:r><w:t>stale review date</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILESIZE "><w:r><w:t>stale file size</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn dynamic_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = (2 + 3) * 4 "><w:r><w:t>stale formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 10 / 4 \# &quot;0.00&quot; "><w:r><w:t>stale formatted formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF 1 = 1 &quot;yes&quot; &quot;no&quot; "><w:r><w:t>stale true</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF &quot;Tokyo&quot; &lt;&gt; &quot;Tokyo&quot; &quot;yes&quot; &quot;no&quot; "><w:r><w:t>stale false</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF 100&gt;=99 &quot;big&quot; &quot;small&quot; "><w:r><w:t>stale compact</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE 5 &gt; 3 "><w:r><w:t>stale compare true</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE &quot;98512&quot; = &quot;985*&quot; "><w:r><w:t>stale compare wildcard</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE &quot;AB&quot; &lt;&gt; &quot;A?&quot; "><w:r><w:t>stale compare false</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE &quot;literal&quot; "><w:r><w:t>literal</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILLIN &quot;Client?&quot; "><w:r><w:t>Acme</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ASK ClientCode &quot;Client code?&quot; "><w:r><w:t>cached ask</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientName &quot;Acme&quot; "><w:r><w:t>cached set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF ClientName \* Upper "><w:r><w:t>stale set ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientTier &quot;Gold&quot; \* MERGEFORMAT "><w:r><w:t>cached formatted set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF ClientTier "><w:r><w:t>stale formatted set ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientCode Client-42 "><w:r><w:t>cached unsupported set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXT "><w:r><w:t>cached next</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXTIF 1 = 1 "><w:r><w:t>cached nextif</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SKIPIF 1 = 0 "><w:r><w:t>cached skipif</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF 1e2 = 100 &quot;scientific&quot; &quot;bad&quot; "><w:r><w:t>stale scientific if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE 1e309 &gt; 0 "><w:r><w:t>cached nonfinite compare</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXTIF City = &quot;Tokyo&quot; "><w:r><w:t>cached unsupported nextif</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILLIN &quot;broken prompt "><w:r><w:t>cached broken fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXTIF 1 = "><w:r><w:t>cached broken nextif</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn if_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" IF CustomerTier = &quot;Gold&quot; &quot;ship&quot; &quot;hold&quot; "><w:r><w:t>cached data if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF 1 = "><w:r><w:t>cached broken if</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn compare_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" COMPARE CustomerTier = &quot;Gold&quot; "><w:r><w:t>cached data compare</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE 1e309 &gt; 0 "><w:r><w:t>cached nonfinite compare</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE \o = &quot;Gold&quot; "><w:r><w:t>cached switch compare</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = CustomerTotal \# &quot;0.00&quot; "><w:r><w:t>cached data formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 1 \# &quot;0.00 "><w:r><w:t>cached broken formula</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn set_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SET ClientName Client 42 "><w:r><w:t>cached ambiguous set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientName &quot;Acme "><w:r><w:t>cached broken set</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn prompt_default_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FILLIN &quot;Client?&quot; \d &quot;Acme&quot; "><w:r><w:t>stale fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILLIN &quot;Department?&quot; \d &quot;ops&quot; \* Upper "><w:r><w:t>stale formatted fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ASK ClientCode &quot;Client code?&quot; \d &quot;ac-42&quot; \o "><w:r><w:t>cached ask default</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF ClientCode \* Upper "><w:r><w:t>stale ask ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn compact_prompt_default_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FILLIN &quot;Client?&quot; \d&quot;Acme&quot; "><w:r><w:t>stale compact fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ASK ClientCode &quot;Client code?&quot; \d&quot;ac-42&quot; "><w:r><w:t>cached compact ask</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF ClientCode \* Upper "><w:r><w:t>stale compact ask ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn unquoted_set_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SET ClientCode Client-42 "><w:r><w:t>cached set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF ClientCode \* Upper "><w:r><w:t>stale ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientName Client 42 "><w:r><w:t>cached multi-token set</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_numeric_picture_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = 1234.5 \# &quot;$#,##0.00&quot; "><w:r><w:t>stale currency formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 33 \# &quot;##%&quot; "><w:r><w:t>stale percent formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 111053 + 111439 \# x## "><w:r><w:t>stale dropped formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 1 / 8 \# 0.00x "><w:r><w:t>stale precision formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 3 / 4 \# .x "><w:r><w:t>stale rounded formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 5 \# &quot;0 units&quot; "><w:r><w:t>stale spaced suffix formula</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_compact_numeric_picture_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = 10 / 4 \#&quot;0.0&quot; "><w:r><w:t>stale compact quoted picture</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SUM(100, 20) \#$0 "><w:r><w:t>stale compact unquoted picture</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 10 \# "><w:r><w:t>cached missing compact picture</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_sectioned_numeric_picture_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = 1245.65 \# &quot;$#,##0.00;-$#,##0.00&quot; "><w:r><w:t>stale positive section</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 0 - 345.56 \# &quot;$#,##0.00;-$#,##0.00&quot; "><w:r><w:t>stale negative section</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 0 \# &quot;$#,##0.00;($#,##0.00);$0&quot; "><w:r><w:t>stale zero section</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_sign_control_numeric_picture_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = 100 - 90 \# +## "><w:r><w:t>stale plus positive</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 90 - 100 \# +## "><w:r><w:t>stale plus negative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 10 - 90 \# -## "><w:r><w:t>stale minus negative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 10 \# -## "><w:r><w:t>stale minus positive</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_function_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = ABS(-22) "><w:r><w:t>stale abs</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SUM(1, 2, 3) "><w:r><w:t>stale sum</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = PRODUCT(2, 3, 4) "><w:r><w:t>stale product</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = MIN(5, -2, 9) "><w:r><w:t>stale min</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = MAX(5, -2, 9) "><w:r><w:t>stale max</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = ROUND(123.456, 2) "><w:r><w:t>stale round</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = INT(5.67) "><w:r><w:t>stale int</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SIGN(-11) "><w:r><w:t>stale sign</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SUM(100, 20) \# &quot;$0&quot; "><w:r><w:t>stale formatted function</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 2 ^ 3 "><w:r><w:t>stale exponent</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = (2 + 1) ^ 3 "><w:r><w:t>stale parenthesized exponent</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = ROUND(4 ^ 0.5, 1) "><w:r><w:t>stale fractional exponent</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 1E3 + 2.5e2 "><w:r><w:t>stale scientific sum</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = ROUND(1.25e-2, 4) "><w:r><w:t>stale scientific fraction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 2E+3 / 4 "><w:r><w:t>stale signed scientific exponent</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_additional_function_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = AVERAGE(2, 4, 6) "><w:r><w:t>stale average</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = COUNT(2, 4, 6) "><w:r><w:t>stale count</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = MOD(10, 4) "><w:r><w:t>stale mod</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = TRUE "><w:r><w:t>stale true constant</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = FALSE "><w:r><w:t>stale false constant</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = AND(1, 2, 3) "><w:r><w:t>stale and true</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = AND(1, 0, 3) "><w:r><w:t>stale and false</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = OR(0, 0, 7) "><w:r><w:t>stale or true</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = NOT(0) "><w:r><w:t>stale not</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = IF(0, 10, 20) "><w:r><w:t>stale if false</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = IF(OR(0, TRUE), SUM(1, 2), 9) "><w:r><w:t>stale nested if</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_semicolon_function_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = SUM(1; 2; 3) "><w:r><w:t>stale semicolon sum</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = IF(OR(0; TRUE); SUM(1; 2); 9) "><w:r><w:t>stale semicolon nested if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SUM(1, 2; 3) "><w:r><w:t>cached mixed separators</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_neutral_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = 2 + 3 \* MERGEFORMAT "><w:r><w:t>stale neutral formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = SUM(1; 2; 3) \*CHARFORMAT "><w:r><w:t>stale compact neutral formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 2 + 3 \* Upper "><w:r><w:t>cached unsupported formula format</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_defined_function_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = DEFINED(SUM(1; 2; 3)) "><w:r><w:t>stale defined expression</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = DEFINED(UnknownBookmark) "><w:r><w:t>stale undefined name</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = DEFINED(1 / 0) "><w:r><w:t>stale error expression</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = IF(DEFINED(2 + 3), 7, 9) "><w:r><w:t>stale nested defined</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = DEFINED() "><w:r><w:t>cached empty defined</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_reference_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>stale left sum</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>10</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = AVERAGE(LEFT) \# &quot;0.0&quot; "><w:r><w:t>stale left average</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(BELOW) "><w:r><w:t>stale below sum</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>9</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = SUM(RIGHT) "><w:r><w:t>stale right sum</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>0</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>0</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(ABOVE) "><w:r><w:t>stale above sum</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>n/a</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>cached nonnumeric left</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_deleted_preceding_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:del><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>deleted formula</w:t></w:r></w:fldSimple></w:p></w:del><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>stale visible sum</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_alternate_content_preceding_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>fallback formula</w:t></w:r></w:fldSimple></w:p></mc:Fallback></mc:AlternateContent><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT) "><w:r><w:t>stale visible sum</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_combined_reference_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT,ABOVE) "><w:r><w:t>stale left above sum</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = SUM(RIGHT;ABOVE) "><w:r><w:t>stale right above sum</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = COUNT(LEFT,RIGHT) "><w:r><w:t>stale side count</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(LEFT,RIGHT;ABOVE) "><w:r><w:t>cached mixed positional separators</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_cell_reference_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = SUM(A1:B2) "><w:r><w:t>stale a1 range</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = PRODUCT(R1C2:R2C3) "><w:r><w:t>stale rncn range</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(A1,C2) "><w:r><w:t>stale a1 list</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(R) "><w:r><w:t>stale current row</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>9</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(C) "><w:r><w:t>stale current column</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>9</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>5</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>7</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(R2C1:R2C3) "><w:r><w:t>stale explicit row range</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(R1C1:R3C1) "><w:r><w:t>stale explicit column range</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = SUM(A1,B1;C1) "><w:r><w:t>cached mixed cell separators</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_nested_expression_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = IF(SUM(LEFT)&gt;=10,10,0) "><w:r><w:t>stale nested if</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = AND(SUM(LEFT)&lt;10,SUM(ABOVE)&gt;=2) "><w:r><w:t>stale nested and</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>8</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = ROUND(AVERAGE(A1:B2),1) "><w:r><w:t>stale nested round</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p/></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = IF(SUM(LEFT,RIGHT;ABOVE)&gt;0,1,0) "><w:r><w:t>cached mixed nested table expression</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_table_ragged_reference_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:tbl><w:tr><w:tc><w:p><w:r><w:t>2</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>4</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>6</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>3</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:fldSimple w:instr=" = SUM(A1:C2) "><w:r><w:t>stale ragged range</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(C) "><w:r><w:t>stale ragged column</w:t></w:r></w:fldSimple></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(ABOVE) "><w:r><w:t>stale ragged above</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl><w:tbl><w:tr><w:tc><w:p><w:r><w:t>1</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:p><w:r><w:t>0</w:t></w:r></w:p></w:tc><w:tc><w:p><w:fldSimple w:instr=" = SUM(ABOVE) "><w:r><w:t>cached absent above</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        ),
    ])
}

fn formula_comparison_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" = IF(2 &gt; 1, 10, 20) "><w:r><w:t>stale greater if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = IF(2 &lt; 1, 10, 20) "><w:r><w:t>stale less if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 2 = 2 "><w:r><w:t>stale equal</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 3 &lt;&gt; 3 "><w:r><w:t>stale not equal false</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = AND(2 &gt;= 2, 3 &lt;= 4, 5 &lt;&gt; 6) "><w:r><w:t>stale logical comparisons</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = OR(1 &gt; 2, 3 &lt; 4) "><w:r><w:t>stale or comparison</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = NOT(7 = 8) "><w:r><w:t>stale not comparison</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn quote_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" QUOTE &quot;literal text&quot; "><w:r><w:t>stale literal</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE &quot;mixed words&quot; \* Caps "><w:r><w:t>stale caps</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> QUOTE &quot;word&quot; \* Upper </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale upper</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" QUOTE PlainToken "><w:r><w:t>stale unquoted token</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE plain words \* Upper "><w:r><w:t>stale unquoted phrase</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE &quot;broken literal "><w:r><w:t>cached broken quote</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn inserted_content_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" INCLUDETEXT &quot;appendix.docx&quot; "><w:r><w:t>Appendix text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDEPICTURE &quot;chart.png&quot; "><w:r><w:t>Chart preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LINK Excel.Sheet.12 &quot;book.xlsx&quot; &quot;Sheet1!R1C1&quot; "><w:r><w:t>42</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EMBED Excel.Sheet.12 "><w:r><w:t>Embedded object</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DATABASE \d &quot;source.accdb&quot; "><w:r><w:t>Rows</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DDE Excel &quot;book.xlsx&quot; &quot;R1C1&quot; "><w:r><w:t>DDE value</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DDEAUTO Excel &quot;book.xlsx&quot; &quot;R2C1&quot; "><w:r><w:t>Auto DDE value</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IMPORT &quot;legacy.wmf&quot; "><w:r><w:t>Imported object</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDE &quot;legacy.doc&quot; "><w:r><w:t>Included text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTOTEXT Signature "><w:r><w:t>AutoText signature</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTOTEXTLIST &quot;Choose clause&quot; \s Legal "><w:r><w:t>AutoText list</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn mail_merge_helper_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" ADDRESSBLOCK "><w:r><w:t>Acme Corp</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GREETINGLINE "><w:r><w:t>Dear Hyunjo,</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEREC "><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGESEQ "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn reference_index_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" BIBLIOGRAPHY \l 1033 "><w:r><w:t>Works cited</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CITATION Smith2026 \l 1033 "><w:r><w:t>(Smith, 2026)</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INDEX \e &quot; - &quot; "><w:r><w:t>Index preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOA \c &quot;1&quot; "><w:r><w:t>Authorities</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TA \l &quot;Case v. Example&quot; \c 1 "><w:r><w:t>Case v. Example</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" XE &quot;Term&quot; "><w:r><w:t>Term</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" RD &quot;appendix.docx&quot; "><w:r><w:t>Referenced doc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TA \l&quot;Compact Case&quot; \c2 "><w:r><w:t>Compact Case</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TA \sShortEntry \c3 "><w:r><w:t>Short Entry</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" XE &quot;See Term&quot; \t&quot;See Also&quot; "><w:r><w:t>See Term</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" XE &quot;Duplicate Format&quot; \* Upper \* Lower "><w:r><w:t>Duplicate Format</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TA \l &quot;Broken Case&quot; \c 99 "><w:r><w:t>Broken Case</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn numbering_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" AUTONUM "><w:r><w:t>stale autonum one</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \* MERGEFORMAT "><w:r><w:t>stale autonum two</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \* roman "><w:r><w:t>stale autonum roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \* Unknown "><w:r><w:t>cached unsupported autonum</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM "><w:r><w:t>stale autonum after unsupported</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \s. "><w:r><w:t>stale autonum separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \s &quot;)&quot; "><w:r><w:t>stale quoted autonum separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUMLGL "><w:r><w:t>cached legal number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUMLGL \* roman "><w:r><w:t>cached legal roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUMOUT "><w:r><w:t>cached outline number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUMOUT \* roman "><w:r><w:t>cached outline roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM LegalDefault \l 2 "><w:r><w:t>cached list number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" BIDIOUTLINE "><w:r><w:t>cached bidi outline</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" BIDIOUTLINE \x "><w:r><w:t>cached malformed bidi outline</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn listnum_number_default_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault "><w:r><w:t>stale listnum one</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \* MERGEFORMAT "><w:r><w:t>stale listnum mergeformat</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \*CHARFORMAT "><w:r><w:t>stale listnum charformat</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \s 4 "><w:r><w:t>stale listnum reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault "><w:r><w:t>stale listnum after reset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \* roman "><w:r><w:t>stale listnum roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \l 2 "><w:r><w:t>cached nested listnum</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM LegalDefault "><w:r><w:t>cached legal listnum</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn sequence_numbering_text_format_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SEQ Figure \* CardText \* Upper "><w:r><w:t>stale sequence card</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SEQ Figure \* roman \* Upper "><w:r><w:t>stale sequence roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \* CardText \* Upper "><w:r><w:t>stale autonum card</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTONUM \* roman \* Upper "><w:r><w:t>stale autonum roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \* CardText \* Upper "><w:r><w:t>stale listnum card</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LISTNUM NumberDefault \* roman \* Upper "><w:r><w:t>stale listnum roman</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn document_structure_field_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rIdCore" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#,
        ),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties"><cp:revision>12</cp:revision></cp:coreProperties>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" REVNUM "><w:r><w:t>4</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SECTION "><w:r><w:t>2</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SECTIONPAGES "><w:r><w:t>5</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF &quot;Heading 1&quot; \n "><w:r><w:t>Executive Summary</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF &quot;Heading 1 "><w:r><w:t>cached broken style ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF \p &quot;Heading 1&quot; "><w:r><w:t>cached switch-first style ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn section_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SECTION "><w:r><w:t>stale first section</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr></w:p><w:p><w:fldSimple w:instr=" SECTION "><w:r><w:t>stale second section</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> SECTION \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale third section</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
    ])
}

fn section_field_invalid_switch_alignment_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SECTION \* Unknown "><w:r><w:t>cached invalid section</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SECTION "><w:r><w:t>stale valid section</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn section_alternate_content_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p><w:pPr><w:sectPr/></w:pPr></w:p></mc:Choice><mc:Fallback><w:p><w:pPr><w:sectPr/></w:pPr></w:p></mc:Fallback></mc:AlternateContent><w:p><w:fldSimple w:instr=" SECTION "><w:r><w:t>stale alternate section</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn section_pages_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SECTIONPAGES "></w:fldSimple></w:p><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:fldSimple w:instr=" SECTIONPAGES \* ROMAN "></w:fldSimple></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:fldSimple w:instr=" SECTIONPAGES \* CardText \* Upper "></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> SECTIONPAGES \* Ordinal </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
    ])
}

fn style_ref_field_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId=" Heading1 "><w:name w:val=" heading 1 "/></w:style><w:style w:type="paragraph" w:styleId=" CustomCallout "><w:name w:val=" Custom Heading "/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val=" Heading1 "/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" STYLEREF &quot;heading 1&quot; "><w:r><w:t>stale heading style</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF &quot;heading 1&quot; \p "><w:r><w:t>stale heading relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF CustomCallout "><w:r><w:t>stale forward style</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF CustomCallout \p \* Upper "><w:r><w:t>stale forward relative</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:pStyle w:val=" CustomCallout "/></w:pPr><w:r><w:t>Forward Finding</w:t></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> STYLEREF &quot;Custom Heading&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale custom style</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn style_ref_deleted_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Visible Heading</w:t></w:r></w:p><w:del><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Deleted Heading</w:t></w:r></w:p></w:del><w:moveFrom><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Moved Heading</w:t></w:r></w:p></w:moveFrom><w:p><w:fldSimple w:instr=" STYLEREF &quot;heading 1&quot; "><w:r><w:t>stale style ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn style_ref_alternate_content_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Choice </w:t><mc:AlternateContent><mc:Choice Requires="wps"><w:t>Heading</w:t></mc:Choice><mc:Fallback><w:t>Fallback Inline</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Fallback Heading</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent><w:p><w:fldSimple w:instr=" STYLEREF &quot;heading 1&quot; "><w:r><w:t>stale style ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn numbered_style_ref_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="NumberedTarget"><w:name w:val="Numbered Target"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="91"/></w:numPr></w:pPr><w:r><w:t>Top 4</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="91"/></w:numPr></w:pPr><w:r><w:t>Child 4.3</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="91"/></w:numPr></w:pPr><w:fldSimple w:instr=" STYLEREF NumberedTarget \r "><w:r><w:t>stale relative style number</w:t></w:r></w:fldSimple><w:r><w:t> </w:t></w:r><w:fldSimple w:instr=" STYLEREF NumberedTarget \r \t "><w:r><w:t>stale relative numeric style number</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="91"/></w:numPr></w:pPr><w:r><w:t>Child 4.4</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="91"/></w:numPr></w:pPr><w:r><w:t>Child 4.5</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="NumberedTarget"/><w:numPr><w:ilvl w:val="2"/><w:numId w:val="91"/></w:numPr></w:pPr><w:r><w:t>Target number</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" STYLEREF NumberedTarget \n "><w:r><w:t>stale style number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" STYLEREF NumberedTarget \n \t "><w:r><w:t>stale numeric style number</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> STYLEREF &quot;Numbered Target&quot; \w \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale full style number</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="15"><w:lvl w:ilvl="0"><w:start w:val="4"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="3"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="Part %3."/></w:lvl></w:abstractNum><w:num w:numId="91"><w:abstractNumId w:val="15"/></w:num></w:numbering>"#,
        ),
    ])
}

fn character_style_ref_field_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="character" w:styleId="LastName"><w:name w:val="Last Name"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" STYLEREF &quot;Last Name&quot; "><w:r><w:t>stale forward last</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:rPr><w:rStyle w:val="LastName"/></w:rPr><w:t>Ackerman</w:t></w:r><w:r><w:t> / </w:t></w:r><w:fldSimple w:instr=" STYLEREF LastName "><w:r><w:t>stale same paragraph first</w:t></w:r></w:fldSimple><w:r><w:t> / </w:t></w:r><w:r><w:rPr><w:rStyle w:val="LastName"/></w:rPr><w:t>Berg</w:t></w:r><w:r><w:t> / </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> STYLEREF &quot;Last Name&quot; \* Upper </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale same paragraph second</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn display_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" ADVANCE \r 2 \d4 \* MERGEFORMAT "><w:r><w:t>offset text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ADVANCE \z 2 "><w:r><w:t>cached unsupported advance</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \f(1,2) "><w:r><w:t>stale equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \f( &quot;Alpha, One&quot; , &quot;Beta Two&quot; ) \* Upper "><w:r><w:t>stale quoted equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \f(3;4) "><w:r><w:t>stale semicolon equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \f(A\,B,C\\D) "><w:r><w:t>stale escaped equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \r(9) "><w:r><w:t>stale square root</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \r(3,27) "><w:r><w:t>stale cube root</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \f(1,\f(2,3)) "><w:r><w:t>stale nested equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \r(\f(1,4)) "><w:r><w:t>stale nested radical</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \b(&quot;Chapter One&quot;) "><w:r><w:t>stale bracket equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \x \to \bo(\f(5,8)) \* Upper "><w:r><w:t>stale boxed equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \b \bc\{ (\r(3,x)) "><w:r><w:t>stale brace bracket equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \b \lc\[ \rc\] (&quot;Range&quot;) "><w:r><w:t>stale explicit bracket equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \l(A,&quot;B, C&quot;,\r(4,16)) "><w:r><w:t>stale list equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \a \al \co2 \vs3 \hs3(Axy,Bxy,A,B) "><w:r><w:t>stale array equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \d \fo10 \li() "><w:r><w:t>stale displace equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\up8(UB)\s\do8(2) "><w:r><w:t>stale script equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i(0,1,x) "><w:r><w:t>stale integral equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i \su(1,5,\f(3,4)) "><w:r><w:t>stale summation equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i \pr(1,3,y) "><w:r><w:t>stale product equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i \in(a,b,\r(9)) "><w:r><w:t>stale integral option equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i \fc\∮(C,D,z) "><w:r><w:t>stale custom integral equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \i \vc\∯(S,T,w) "><w:r><w:t>stale vertical custom integral equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \o \ac(&quot;A&quot;,/) "><w:r><w:t>stale overstrike equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 183 \f Symbol "><w:r><w:t>symbol</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\ai4()\di3() "><w:r><w:t>stale empty script equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\ai4()\up8(UB)\s\di3()\do8(2) "><w:r><w:t>stale layout script equation</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn symbol_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SYMBOL 163 "><w:r><w:t>stale pound</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 0x03BB \u \h "><w:r><w:t>stale lambda</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> SYMBOL 211 \f &quot;Symbol&quot; \s 12 </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale copyright</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" SYMBOL 183 \fSymbol \s12 "><w:r><w:t>stale compact symbol</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 0x0041 \u \s &quot;10&quot; "><w:r><w:t>stale quoted size</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 0x0042 \u \s&quot;11&quot; "><w:r><w:t>stale compact quoted size</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn action_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" GOTOBUTTON TargetBookmark &quot;Jump&quot; "><w:r><w:t>stale jump</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GOTOBUTTON TargetBookmark Jump Now \* Upper "><w:r><w:t>stale jump upper</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MACROBUTTON RunReport &quot;Run report&quot; "><w:r><w:t>stale run</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MACROBUTTON RunReport Run \* Upper Again "><w:r><w:t>cached malformed action</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MACROBUTTON RunReport \* MERGEFORMAT "><w:r><w:t>cached target-only action</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT &quot;page \p&quot; "><w:r><w:t>Print instruction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT status "><w:r><w:t>Unquoted print instruction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT status ready \* MERGEFORMAT "><w:r><w:t>Multi-token print instruction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT \p ReportBox &quot;0 0 moveto&quot; \* MERGEFORMAT "><w:r><w:t>PostScript instruction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT \pReportBox &quot;compact moveto&quot; "><w:r><w:t>Compact PostScript instruction</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PRINT \z &quot;bad&quot; "><w:r><w:t>cached unsupported print</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn compatibility_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PRIVATE "><w:r><w:rPr><w:vanish/></w:rPr><w:t>converted payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ADDIN hidden-data "><w:r><w:rPr><w:vanish/></w:rPr><w:t>addin payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DATA legacy-data "><w:r><w:rPr><w:vanish/></w:rPr><w:t>data payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GLOSSARY AutoTextName "><w:r><w:t>glossary payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HTMLACTIVEX LegacyControl "><w:r><w:t>activex payload</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn barcode_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \q H "><w:r><w:t>QR preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEBARCODE CustomerId CODE128 \t "><w:r><w:t>Merge barcode preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" BARCODE &quot;9781234567890&quot; "><w:r><w:t>Legacy barcode preview</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn legacy_form_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMTEXT "><w:r><w:t>Alice</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMTEXT "><w:ffData><w:textInput><w:default w:val="No content."/></w:textInput></w:ffData><w:r><w:t></w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale checked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="false"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale unchecked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:default w:val="true"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale default checked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><w:ddList><w:default w:val="0"/><w:result w:val="2"/><w:listEntry w:val="Option A"/><w:listEntry w:val="Option B"/><w:listEntry w:val="Option C"/></w:ddList></w:ffData><w:r><w:t>stale option</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><w:ddList><w:default w:val="1"/><w:listEntry w:val="Default A"/><w:listEntry w:val="Default B"/></w:ddList></w:ffData><w:r><w:t>stale default option</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><w:ddList><w:default w:val="1"/><w:result w:val="9"/><w:listEntry w:val="Fallback A"/><w:listEntry w:val="Fallback B"/></w:ddList></w:ffData><w:r><w:t>stale invalid option</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn legacy_form_deleted_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:del><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>deleted checked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:del><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="false"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale visible unchecked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn legacy_form_alternate_content_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>fallback checked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></mc:Fallback></mc:AlternateContent><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><mc:AlternateContent><mc:Choice Requires="wps"><w:ddList><w:result w:val="0"/><w:listEntry w:val="Choice option"/></w:ddList></mc:Choice><mc:Fallback><w:ddList><w:result w:val="1"/><w:listEntry w:val="Fallback option"/></w:ddList></mc:Fallback></mc:AlternateContent></w:ffData><w:r><w:t>stale option</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn simple_cached_result_inline_marker_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>Alpha</w:t><w:tab/><w:t>Beta</w:t><w:br/><w:t>Gamma</w:t><w:noBreakHyphen/><w:t>Hard</w:t><w:softHyphen/><w:t>Soft</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn complex_cached_result_inline_marker_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> CUSTOM value </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>One</w:t><w:tab/><w:t>Two</w:t><w:br/><w:t>Three</w:t><w:noBreakHyphen/><w:t>Hard</w:t><w:softHyphen/><w:t>Soft</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> CUSTOM markersOnly </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:tab/><w:br/><w:noBreakHyphen/><w:softHyphen/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn nested_complex_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> CUSTOM outer </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Before </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MERGEFIELD InnerName </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Inner Value</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t> After</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn nested_complex_with_simple_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> CUSTOM outer </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Before </w:t></w:r><w:fldSimple w:instr=" MERGEFIELD InnerName "><w:r><w:t>Inner Value</w:t></w:r></w:fldSimple><w:r><w:t> After</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn ref_bookmark_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>stale cached text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingBookmark "><w:r><w:t>Missing</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn ref_deleted_bookmark_text_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="ClauseText"/><w:r><w:t>Visible clause</w:t></w:r><w:del><w:r><w:t> deleted clause</w:t></w:r></w:del><w:moveFrom><w:r><w:t> moved clause</w:t></w:r></w:moveFrom><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF ClauseText "><w:r><w:t>stale deleted ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn ref_alternate_content_bookmark_text_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:bookmarkStart w:id="7" w:name="AltText"/><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:t>Choice clause</w:t></mc:Choice><mc:Fallback><w:t>Fallback clause</w:t></mc:Fallback></mc:AlternateContent></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF AltText "><w:r><w:t>stale alternate ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn complex_ref_bookmark_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF Figure1 </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale complex ref</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn multi_paragraph_ref_bookmark_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="11" w:name="ClauseText"/><w:r><w:t>First paragraph.</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph.</w:t></w:r><w:bookmarkEnd w:id="11"/></w:p><w:p><w:fldSimple w:instr=" REF ClauseText "><w:r><w:t>stale multi ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn inline_break_ref_bookmark_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="12" w:name="InlineText"/><w:r><w:t>Alpha</w:t><w:tab/><w:t>Beta</w:t><w:br/><w:t>Gamma</w:t><w:noBreakHyphen/><w:t>Delta</w:t></w:r><w:bookmarkEnd w:id="12"/></w:p><w:p><w:fldSimple w:instr=" REF InlineText "><w:r><w:t>stale inline ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn hidden_ref_bookmark_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="9" w:name="_Ref123456789"/><w:r><w:t>Table 2</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" REF _Ref123456789 "><w:r><w:t>stale hidden ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn direct_bookmark_ref_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" Figure1 "><w:r><w:t>stale direct ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn direct_bookmark_ref_switch_field_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>figure one</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" Figure1 \* Upper "><w:r><w:t>stale direct upper</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \*FirstCap "><w:r><w:t>stale direct first-cap</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \h "><w:r><w:t>stale direct hyperlink</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \d &quot;-&quot; "><w:r><w:t>direct sequence separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \f "><w:r><w:t>direct note mark</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn direct_relative_ref_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" LaterBookmark \p "><w:r><w:t>stale direct below</w:t></w:r></w:fldSimple></w:p><w:p><w:bookmarkStart w:id="8" w:name="LaterBookmark"/><w:r><w:t>Later target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" Figure1 \p "><w:r><w:t>stale direct above</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn ref_text_format_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>figure one</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \* Upper "><w:r><w:t>stale upper ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \*Lower "><w:r><w:t>stale lower ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \* Caps "><w:r><w:t>stale caps ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \*FirstCap "><w:r><w:t>stale first-cap ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn broader_ref_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \f "><w:r><w:t>note mark</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn note_ref_field_docx() -> Vec<u8> {
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
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" NOTEREF LaterNote \p "><w:r><w:t>stale below note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF LaterNote \p \* Upper "><w:r><w:t>stale uppercase below note</w:t></w:r></w:fldSimple></w:p><w:del><w:r><w:footnoteReference w:id="99"/></w:r></w:del><w:p><w:r><w:t>First reference</w:t></w:r><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:t>Second reference</w:t></w:r><w:bookmarkStart w:id="8" w:name="LaterNote"/><w:r><w:footnoteReference w:id="2"/></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \h "><w:r><w:t>stale note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FTNREF FootOne "><w:r><w:t>stale legacy note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \f \* MERGEFORMAT "><w:r><w:t>stale formatted note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \p "><w:r><w:t>stale above note</w:t></w:r></w:fldSimple></w:p><w:p><w:bookmarkStart w:id="9" w:name="EndOne"/><w:r><w:endnoteReference w:id="3"/></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF EndOne "><w:r><w:t>stale endnote mark</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> NOTEREF LaterNote </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale complex note mark</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" NOTEREF MissingNote "><w:r><w:t>stale missing note</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Second footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="3"><w:p><w:r><w:t>First endnote.</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn note_body_field_docx() -> Vec<u8> {
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
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Body</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:fldSimple w:instr=" FILENAME \p "><w:r><w:t>note.docx</w:t></w:r></w:fldSimple></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>4</w:t></w:r></w:fldSimple></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn note_ref_alternate_content_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:r><w:bookmarkStart w:id="50" w:name="FallbackNote"/><w:footnoteReference w:id="50"/><w:bookmarkEnd w:id="50"/></w:r></w:p></mc:Fallback></mc:AlternateContent><w:p><w:r><w:t>Target note</w:t></w:r><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne "><w:r><w:t>stale alternate note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \p "><w:r><w:t>stale alternate relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="50"><w:p><w:r><w:t>Fallback footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
    ])
}

fn note_ref_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="8" w:name="LaterNote"/><w:r><w:footnoteReference w:id="2"/></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF LaterNote \* roman "><w:r><w:t>stale roman note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \* OrdText \* Upper "><w:r><w:t>stale ordinal note</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn ref_note_switch_docx() -> Vec<u8> {
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
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:del><w:r><w:footnoteReference w:id="99"/></w:r></w:del><w:p><w:r><w:t>Target footnote</w:t></w:r><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:t>Second footnote</w:t></w:r><w:r><w:footnoteReference w:id="2"/></w:r></w:p><w:p><w:fldSimple w:instr=" REF FootOne \f "><w:r><w:t>stale foot ref mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FootOne \f "><w:r><w:t>stale direct foot ref mark</w:t></w:r></w:fldSimple></w:p><w:p><w:bookmarkStart w:id="9" w:name="EndOne"/><w:r><w:endnoteReference w:id="3"/></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" REF EndOne \h \f \* MERGEFORMAT "><w:r><w:t>stale end ref mark</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> REF FootOne \f </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale complex foot ref mark</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" REF FootOne \f \* roman "><w:r><w:t>stale roman foot ref mark</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>First footnote.</w:t></w:r></w:p></w:footnote><w:footnote w:id="2"><w:p><w:r><w:t>Second footnote.</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="3"><w:p><w:r><w:t>First endnote.</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn numbered_ref_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val=" 42 "/></w:numPr></w:pPr><w:bookmarkStart w:id="7" w:name="Clause"/><w:r><w:t>Numbered clause</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Clause \n "><w:r><w:t>stale number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Clause \n \p "><w:r><w:t>stale number relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Clause \n "><w:r><w:t>stale direct number</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="9"><w:lvl w:ilvl="0"><w:start w:val=" 3 "/><w:numFmt w:val="decimal"/><w:lvlText w:val=" "/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val=" 9 "/></w:num></w:numbering>"#,
        ),
    ])
}

fn alternate_content_numbered_ref_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="45"/></w:numPr></w:pPr><w:r><w:t>Choice preface</w:t></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="45"/></w:numPr></w:pPr><w:r><w:t>Fallback preface</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="45"/></w:numPr></w:pPr><w:bookmarkStart w:id="7" w:name="AltClause"/><w:r><w:t>Target clause</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF AltClause \n "><w:r><w:t>stale alt number</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="10"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="45"><w:abstractNumId w:val="10"/></w:num></w:numbering>"#,
        ),
    ])
}

fn numbered_ref_suppress_text_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="77"/></w:numPr></w:pPr><w:bookmarkStart w:id="9" w:name="SectionClause"/><w:r><w:t>Section clause</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" REF SectionClause \n \t "><w:r><w:t>stale numeric text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF SectionClause \n \t \p "><w:r><w:t>stale numeric relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SectionClause \n \t "><w:r><w:t>stale direct numeric text</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="11"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="Section %1.01,"/></w:lvl></w:abstractNum><w:num w:numId="77"><w:abstractNumId w:val="11"/></w:num></w:numbering>"#,
        ),
    ])
}

fn full_context_ref_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="88"/></w:numPr></w:pPr><w:r><w:t>Top clause</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="88"/></w:numPr></w:pPr><w:r><w:t>Child clause</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="88"/></w:numPr></w:pPr><w:bookmarkStart w:id="12" w:name="DeepClause"/><w:r><w:t>Deep clause</w:t></w:r><w:bookmarkEnd w:id="12"/></w:p><w:p><w:fldSimple w:instr=" REF DeepClause \w "><w:r><w:t>stale full context</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF DeepClause \w \p "><w:r><w:t>stale full relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DeepClause \w "><w:r><w:t>stale direct full</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="12"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="%2."/></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="%3."/></w:lvl></w:abstractNum><w:num w:numId="88"><w:abstractNumId w:val="12"/></w:num></w:numbering>"#,
        ),
    ])
}

fn full_context_ref_suppress_text_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="89"/></w:numPr></w:pPr><w:r><w:t>Top clause</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="89"/></w:numPr></w:pPr><w:r><w:t>Child clause</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="89"/></w:numPr></w:pPr><w:bookmarkStart w:id="13" w:name="DeepClause"/><w:r><w:t>Deep clause</w:t></w:r><w:bookmarkEnd w:id="13"/></w:p><w:p><w:fldSimple w:instr=" REF DeepClause \w \t "><w:r><w:t>stale full numeric text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF DeepClause \w \t \p "><w:r><w:t>stale full numeric relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DeepClause \w \t "><w:r><w:t>stale direct full numeric</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="13"><w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="Section %1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/><w:lvlText w:val="Article %2."/></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="lowerRoman"/><w:lvlText w:val="Part %3."/></w:lvl></w:abstractNum><w:num w:numId="89"><w:abstractNumId w:val="13"/></w:num></w:numbering>"#,
        ),
    ])
}

fn relative_context_ref_switch_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="90"/></w:numPr></w:pPr><w:r><w:t>Top 4</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="90"/></w:numPr></w:pPr><w:r><w:t>Child 4.3</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="90"/></w:numPr></w:pPr><w:fldSimple w:instr=" REF LaterClause \r "><w:r><w:t>stale relative context</w:t></w:r></w:fldSimple><w:r><w:t> </w:t></w:r><w:fldSimple w:instr=" REF LaterClause \r \p "><w:r><w:t>stale relative context position</w:t></w:r></w:fldSimple><w:r><w:t> </w:t></w:r><w:fldSimple w:instr=" LaterClause \r \t "><w:r><w:t>stale direct relative context</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="90"/></w:numPr></w:pPr><w:r><w:t>Child 4.4</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="1"/><w:numId w:val="90"/></w:numPr></w:pPr><w:r><w:t>Child 4.5</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="90"/></w:numPr></w:pPr><w:r><w:t>Target sibling 4.5.1</w:t></w:r></w:p><w:p><w:pPr><w:numPr><w:ilvl w:val="2"/><w:numId w:val="90"/></w:numPr></w:pPr><w:bookmarkStart w:id="14" w:name="LaterClause"/><w:r><w:t>Target 4.5.2</w:t></w:r><w:bookmarkEnd w:id="14"/></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="14"><w:lvl w:ilvl="0"><w:start w:val="4"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl><w:lvl w:ilvl="1"><w:start w:val="3"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%2."/></w:lvl><w:lvl w:ilvl="2"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%3."/></w:lvl></w:abstractNum><w:num w:numId="90"><w:abstractNumId w:val="14"/></w:num></w:numbering>"#,
        ),
    ])
}

fn relative_ref_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" REF LaterBookmark \p "><w:r><w:t>stale below</w:t></w:r></w:fldSimple></w:p><w:p><w:bookmarkStart w:id="8" w:name="LaterBookmark"/><w:r><w:t>Later target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \p "><w:r><w:t>stale above</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn relative_ref_alternate_content_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:fldSimple w:instr=" REF LaterBookmark \p "><w:r><w:t>fallback relative</w:t></w:r></w:fldSimple></w:p></mc:Fallback></mc:AlternateContent><w:p><w:bookmarkStart w:id="8" w:name="LaterBookmark"/><w:r><w:t>Later target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" REF LaterBookmark \p "><w:r><w:t>stale visible above</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="9" w:name="TableOne"/><w:r><w:t>Table 1</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;TableOne&quot; \p </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>above</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_manual_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;Figure1&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>old page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_leading_break_relative_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \p "><w:r><w:t>stale relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_format_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* ROMAN "><w:r><w:t>stale upper roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \*roman "><w:r><w:t>stale lower roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* alphabetic "><w:r><w:t>stale alphabetic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \*ALPHABETIC "><w:r><w:t>stale upper alphabetic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* Ordinal "><w:r><w:t>stale ordinal</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* CardText "><w:r><w:t>stale cardtext</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* CardText \* Upper "><w:r><w:t>stale uppercase cardtext</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* OrdText "><w:r><w:t>stale ordtext</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* Arabic "><w:r><w:t>stale arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* ArabicDash "><w:r><w:t>stale arabic dash</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_mixed_case_format_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* ArAbIc "><w:r><w:t>stale mixed arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_after_visible_manual_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Cover text can auto-paginate before the hard break.</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text can be long enough to paginate.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="FigureTwo"/><w:r><w:t>Figure 2</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FigureTwo \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;FigureTwo&quot; \* CHARFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>old rendered page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF FigureTwo \p "><w:r><w:t>below</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_break_page_one_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Cover"/><w:r><w:t>Cover title</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Cover \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_break_relative_below_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF FigureLater \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p><w:p><w:bookmarkStart w:id="7" w:name="FigureLater"/><w:r><w:t>Figure later</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_then_manual_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF PageThree \p "><w:r><w:t>stale distant relative</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="PageThree"/><w:r><w:t>Page three target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF PageThree \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF PageThree \p "><w:r><w:t>old relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale current page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_section_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="NextSection"/><w:r><w:t>Next section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale next current</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page three lead.</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val=" evenPage "/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="8" w:name="EvenSection"/><w:r><w:t>Even section target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale even current</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="oddPage"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="9" w:name="OddSection"/><w:r><w:t>Odd section target</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale odd current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF NextSection \h "><w:r><w:t>stale next</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF EvenSection \* roman "><w:r><w:t>stale even</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF OddSection \p "><w:r><w:t>stale odd relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_content_paragraph_section_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr><w:bookmarkStart w:id="7" w:name="BeforeSectionBreak"/><w:r><w:t>Before break</w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>stale same-paragraph page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:t>After break</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF BeforeSectionBreak \h "><w:r><w:t>stale before break</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_default_section_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr/></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="DefaultSection"/><w:r><w:t>Default section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF DefaultSection \h "><w:r><w:t>stale default</w:t></w:r></w:fldSimple></w:p><w:sectPr/></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_page_break_before_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:bookmarkStart w:id="7" w:name="BreakBefore"/><w:r><w:t>Break-before target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:t>Visible intro before another break-before paragraph.</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:bookmarkStart w:id="8" w:name="BreakAfterIntro"/><w:r><w:t>Break-before target after intro</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page after rendered marker.</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:bookmarkStart w:id="9" w:name="RenderedBreakBefore"/><w:r><w:t>Rendered break-before target</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF BreakBefore \h "><w:r><w:t>stale break-before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF BreakAfterIntro \h "><w:r><w:t>stale after-intro break-before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RenderedBreakBefore \* Ordinal "><w:r><w:t>stale rendered break-before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RenderedBreakBefore \p "><w:r><w:t>stale relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_disabled_page_break_before_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore w:val=" Off "/></w:pPr><w:bookmarkStart w:id="7" w:name="NoForcedBreak"/><w:r><w:t>No forced break target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF NoForcedBreak \h "><w:r><w:t>stale disabled break</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_section_page_number_restart_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start=" 7 "/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="Restarted"/><w:r><w:t>Restarted target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Restarted next page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="8" w:name="RestartedNext"/><w:r><w:t>Restarted next target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Restarted \h "><w:r><w:t>stale restart</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedNext \* ROMAN "><w:r><w:t>stale restart roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedNext \p "><w:r><w:t>stale restart relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_visible_intro_section_page_number_restart_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Intro text can auto-paginate before the restart.</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="7" w:fmt=" "/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="RestartedAfterIntro"/><w:r><w:t>Restarted target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedAfterIntro \h "><w:r><w:t>stale restarted page</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedAfterIntro \p "><w:r><w:t>stale restarted relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="3" w:fmt=" lowerRoman "/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="RomanSection"/><w:r><w:t>Roman section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale roman current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RomanSection \h "><w:r><w:t>stale roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RomanSection \* Arabic "><w:r><w:t>stale arabic override</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RomanSection \p "><w:r><w:t>stale roman relative</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="4" w:fmt="decimalZero"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Decimal zero page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="8" w:name="DecimalZeroSection"/><w:r><w:t>Decimal zero target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>stale decimal current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DecimalZeroSection \h "><w:r><w:t>stale decimal zero</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DecimalZeroSection \* Arabic "><w:r><w:t>stale decimal arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="5" w:fmt="numberInDash"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="9" w:name="DashedSection"/><w:r><w:t>Dashed target</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>stale dashed current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DashedSection \h "><w:r><w:t>stale dashed</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DashedSection \* Arabic "><w:r><w:t>stale dashed arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_relative_unsupported_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="2" w:fmt="chicago"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name=" UnsupportedFmt "/><w:r><w:t>Unsupported format target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Later page lead.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF UnsupportedFmt \p "><w:r><w:t>stale relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_decimal_full_width_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="12" w:fmt="decimalFullWidth"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="10" w:name="FullWidthSection"/><w:r><w:t>Full-width target</w:t></w:r><w:bookmarkEnd w:id="10"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullWidthSection \h "><w:r><w:t>stale fullwidth</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullWidthSection \* Arabic "><w:r><w:t>stale fullwidth arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_decimal_enclosed_circle_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="12" w:fmt="decimalEnclosedCircle"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="11" w:name="CircleSection"/><w:r><w:t>Circle target</w:t></w:r><w:bookmarkEnd w:id="11"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF CircleSection \h "><w:r><w:t>stale circle</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF CircleSection \* Arabic "><w:r><w:t>stale circle arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_decimal_enclosed_punctuation_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="12" w:fmt="decimalEnclosedFullstop"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="12" w:name="FullstopSection"/><w:r><w:t>Fullstop target</w:t></w:r><w:bookmarkEnd w:id="12"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullstopSection \h "><w:r><w:t>stale fullstop</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullstopSection \* Arabic "><w:r><w:t>stale fullstop arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="11" w:fmt="decimalEnclosedParen"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Paren page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="13" w:name="ParenSection"/><w:r><w:t>Paren target</w:t></w:r><w:bookmarkEnd w:id="13"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF ParenSection \h "><w:r><w:t>stale paren</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF ParenSection \* Arabic "><w:r><w:t>stale paren arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_decimal_width_variant_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="12" w:fmt="decimalHalfWidth"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="14" w:name="HalfWidthSection"/><w:r><w:t>Half-width target</w:t></w:r><w:bookmarkEnd w:id="14"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF HalfWidthSection \h "><w:r><w:t>stale halfwidth</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF HalfWidthSection \* ArabicDash "><w:r><w:t>stale halfwidth dash</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="12" w:fmt="decimalFullWidth2"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Full-width alternate page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="15" w:name="FullWidthAltSection"/><w:r><w:t>Full-width alternate target</w:t></w:r><w:bookmarkEnd w:id="15"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullWidthAltSection \h "><w:r><w:t>stale fullwidth alt</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FullWidthAltSection \* Arabic "><w:r><w:t>stale fullwidth alt arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_korean_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="1" w:fmt="ganada"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="16" w:name="GanadaSection"/><w:r><w:t>Ganada target</w:t></w:r><w:bookmarkEnd w:id="16"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF GanadaSection \h "><w:r><w:t>stale ganada</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF GanadaSection \* Arabic "><w:r><w:t>stale ganada arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="1" w:fmt="chosung"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Chosung page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="17" w:name="ChosungSection"/><w:r><w:t>Chosung target</w:t></w:r><w:bookmarkEnd w:id="17"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF ChosungSection \h "><w:r><w:t>stale chosung</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF ChosungSection \* Arabic "><w:r><w:t>stale chosung arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_korean_numeric_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="1" w:fmt="koreanDigital"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="18" w:name="KoreanDigitalSection"/><w:r><w:t>Korean digital target</w:t></w:r><w:bookmarkEnd w:id="18"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanDigitalSection \h "><w:r><w:t>stale korean digital</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanDigitalSection \* Arabic "><w:r><w:t>stale korean digital arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="1" w:fmt="koreanCounting"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Korean counting page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="19" w:name="KoreanCountingSection"/><w:r><w:t>Korean counting target</w:t></w:r><w:bookmarkEnd w:id="19"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanCountingSection \h "><w:r><w:t>stale korean counting</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanCountingSection \* Arabic "><w:r><w:t>stale korean counting arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="9" w:fmt="koreanLegal"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Korean legal page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="20" w:name="KoreanLegalSection"/><w:r><w:t>Korean legal target</w:t></w:r><w:bookmarkEnd w:id="20"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanLegalSection \h "><w:r><w:t>stale korean legal</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanLegalSection \* Arabic "><w:r><w:t>stale korean legal arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="1" w:fmt="koreanDigital2"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Korean digital2 page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="21" w:name="KoreanDigital2Section"/><w:r><w:t>Korean digital2 target</w:t></w:r><w:bookmarkEnd w:id="21"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanDigital2Section \h "><w:r><w:t>stale korean digital2</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF KoreanDigital2Section \* Arabic "><w:r><w:t>stale korean digital2 arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_final_section_page_number_format_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="FinalSection"/><w:r><w:t>Final-section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGE \* Arabic "><w:r><w:t>stale final current</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \h "><w:r><w:t>stale final roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \* Arabic "><w:r><w:t>stale final arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \p "><w:r><w:t>stale final relative</w:t></w:r></w:fldSimple></w:p><w:sectPr><w:pgNumType w:start="5" w:fmt="lowerRoman"/></w:sectPr></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_final_section_ignores_deleted_paragraph_section_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:pPr><w:sectPr><w:pgNumType w:start="88"/></w:sectPr></w:pPr></w:p></mc:Fallback></mc:AlternateContent><w:del><w:p><w:pPr><w:sectPr><w:pgNumType w:start="99"/></w:sectPr></w:pPr></w:p></w:del><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="FinalSection"/><w:r><w:t>Final-section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \h "><w:r><w:t>stale final roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \* Arabic "><w:r><w:t>stale final arabic</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF FinalSection \p "><w:r><w:t>stale final relative</w:t></w:r></w:fldSimple></w:p><w:sectPr><w:pgNumType w:start="5" w:fmt="upperRoman"/></w:sectPr></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_visible_manual_break_before_rendered_hint_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Cover text can auto-paginate before the hard break.</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="AmbiguousTarget"/><w:r><w:t>Ambiguous target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Later rendered hint.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF AmbiguousTarget \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_deleted_rendered_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:del><w:r><w:lastRenderedPageBreak/></w:r></w:del></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_leading_break_precedes_rendered_hint_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Later rendered hint.</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_break_no_cached_result_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="FigureTwo"/><w:r><w:t>Figure 2</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF FigureTwo \h "></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;FigureTwo&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_rendered_break_wrapped_complex_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="FigureTwo"/><w:r><w:t>Figure 2</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:ins><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;FigureTwo&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>old wrapped page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:ins></w:p></w:body></w:document>"#,
        ),
    ])
}

fn page_ref_alternate_content_rendered_break_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:lastRenderedPageBreak/></mc:Choice><mc:Fallback><w:lastRenderedPageBreak/></mc:Fallback></mc:AlternateContent><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="AltPage"/><w:r><w:t>Alternate target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF AltPage \h "><w:r><w:t>stale alternate page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val=" 1 "/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="3"/></w:pPr><w:r><w:t>Excluded Detail</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; "><w:r><w:t>stale toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_heading_inline_break_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive</w:t><w:tab/><w:t>Summary</w:t><w:br/><w:t>Detail</w:t><w:noBreakHyphen/><w:t>Follow-up</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; "><w:r><w:t>stale inline toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn bare_toc_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>Mitigation</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="3"/></w:pPr><w:r><w:t>Excluded Detail</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC "><w:r><w:t>stale bare toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn default_neutral_toc_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>Mitigation</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="3"/></w:pPr><w:r><w:t>Excluded Detail</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \h \z "><w:r><w:t>stale neutral default toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \n &quot;1-3&quot; "><w:r><w:t>stale no-page default toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \p &quot;-&quot; "><w:r><w:t>stale separator default toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \s chapter \d &quot;-&quot; "><w:r><w:t>stale sequence default toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \* Upper "><w:r><w:t>stale upper default toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \* MERGEFORMAT "><w:r><w:t>stale mergeformat default toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn advanced_toc_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_custom_style_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="CustomCallout"><w:name w:val="CustomHeading"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="CustomCallout"/></w:pPr><w:r><w:t>Custom Finding</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; \t &quot;CustomHeading,2&quot; "><w:r><w:t>stale custom toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_quoted_custom_style_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="CustomCallout"><w:name w:val="Custom Heading"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="CustomCallout"/></w:pPr><w:r><w:t>Custom Finding</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; \t &quot;Custom Heading,2&quot; "><w:r><w:t>stale quoted custom toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_tc_field_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" TC &quot;Manual Entry&quot; \f m \l 2 "><w:r><w:t>stale manual tc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TC &quot;Other Entry&quot; \f x \l 1 "><w:r><w:t>stale other tc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \f m "><w:r><w:t>stale tc toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_deleted_tc_field_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:del><w:p><w:fldSimple w:instr=" TC &quot;Deleted Entry&quot; \f m \l 1 "/></w:p></w:del><w:moveFrom><w:p><w:fldSimple w:instr=" TC &quot;Moved Entry&quot; \f m \l 1 "/></w:p></w:moveFrom><w:p><w:fldSimple w:instr=" TC &quot;Visible Entry&quot; \f m \l 1 "><w:r><w:t>stale visible marker</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \f m "><w:r><w:t>stale deleted tc toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_alternate_content_heading_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><mc:AlternateContent><mc:Choice Requires="wps"><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Choice </w:t><mc:AlternateContent><mc:Choice Requires="wps"><w:t>Inline</w:t></mc:Choice><mc:Fallback><w:t>Fallback Inline</w:t></mc:Fallback></mc:AlternateContent></w:r></w:p></mc:Choice><mc:Fallback><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Fallback Heading</w:t></w:r></w:p></mc:Fallback></mc:AlternateContent><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; "><w:r><w:t>stale alternate toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn invalid_toc_entry_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" TC \l 2 "><w:r><w:t>cached invalid tc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_sequence_caption_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Figure </w:t></w:r><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:t>: Mercury</w:t></w:r></w:p><w:p><w:r><w:t>Table </w:t></w:r><w:fldSimple w:instr=" SEQ Table "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:t>: Invoices</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \c &quot;Figure&quot; "><w:r><w:t>stale figures toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_sequence_caption_text_switch_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Figure </w:t></w:r><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>8</w:t></w:r></w:fldSimple><w:r><w:t>: Mercury</w:t></w:r></w:p><w:p><w:r><w:t>Table </w:t></w:r><w:fldSimple w:instr=" SEQ Table "><w:r><w:t>2</w:t></w:r></w:fldSimple><w:r><w:t>: Invoices</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \a Figure "><w:r><w:t>stale caption-text toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_neutral_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \h \z \w \x "><w:r><w:t>stale neutral toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_general_format_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>executive summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>risk review</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \* Upper "><w:r><w:t>stale upper toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \* Caps "><w:r><w:t>stale caps toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \* MERGEFORMAT "><w:r><w:t>stale mergeformat toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_no_page_numbers_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \n &quot;1-2&quot; "><w:r><w:t>stale no-page toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_entry_page_separator_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \p &quot;-&quot; "><w:r><w:t>stale separator toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_sequence_page_separator_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \s chapter \d &quot;-&quot; "><w:r><w:t>stale sequence separator toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_outline_level_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Style Heading</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Outline Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \u "><w:r><w:t>stale outline toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_heading_and_outline_switch_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Style Heading</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Outline Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \u "><w:r><w:t>stale combined toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_bookmark_scope_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Outside Heading</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="ScopedToc"/><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Scoped Heading</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Scoped Detail</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Trailing Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \b ScopedToc "><w:r><w:t>stale scoped toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_bookmark_only_scope_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/></w:style><w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Outside Heading</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="ScopedToc"/><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Scoped Heading</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Scoped Detail</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:pPr><w:pStyle w:val="Heading4"/></w:pPr><w:r><w:t>Scoped Deep Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \b ScopedToc "><w:r><w:t>stale bookmark-only toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn toc_outline_without_range_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style><w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading4"/></w:pPr><w:r><w:t>Appendix Detail</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o "><w:r><w:t>stale open-outline toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

fn missing_toc_bookmark_scope_docx() -> Vec<u8> {
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
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Existing Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; \b MissingScope "><w:r><w:t>stale missing scope toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[test]
fn docx_fields_are_extracted() {
    let doc = Document::open(&field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "1");
    assert_eq!(fields[1].kind, FieldKind::Toc);
    assert_eq!(fields[1].instruction, "TOC \\o \"1-3\"");
    assert_eq!(fields[2].kind, FieldKind::Ref);
    assert_eq!(fields[3].kind, FieldKind::Hyperlink);
    assert_eq!(fields[4].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[5].kind, FieldKind::Filename);
    assert_eq!(fields[5].instruction, "FILENAME \\p");
    assert_eq!(fields[5].result, "report.docx");
}

#[test]
fn docx_complex_field_char_type_trims_ooxml_value() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType=" begin "/></w:r><w:r><w:instrText> PAGE </w:instrText></w:r><w:r><w:fldChar w:fldCharType=" separate "/></w:r><w:r><w:t>cached page</w:t></w:r><w:r><w:fldChar w:fldCharType=" end "/></w:r></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "cached page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(doc.main_text(), "1");
    assert_eq!(doc.report().features.fields, 1);
}

#[test]
fn docx_fields_follow_accepted_revision_view() {
    let doc = Document::open(&revision_wrapped_field_docx()).expect("fixture opens");

    assert_eq!(doc.main_text(), "direct name\ninserted name\nmoved-to name");
    let fields = doc.fields();
    let instructions: Vec<_> = fields
        .iter()
        .map(|field| field.instruction.as_str())
        .collect();
    assert_eq!(
        instructions,
        vec![
            "MERGEFIELD DirectName",
            "MERGEFIELD InsertedName",
            "MERGEFIELD MovedToName"
        ]
    );
    assert!(fields
        .iter()
        .all(|field| field.kind == FieldKind::MergeField));
}

#[test]
fn docx_fields_use_single_alternate_content_branch() {
    let doc = Document::open(&alternate_content_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::MergeField);
    assert_eq!(fields[0].instruction, "MERGEFIELD AltClient");
    assert_eq!(fields[0].result, "Choice Client");
    assert_eq!(doc.report().features.fields, 1);
}

#[test]
fn docx_page_fields_compute_trusted_current_page_numbers() {
    let doc = Document::open(&page_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "stale restart decimal zero");
    assert_eq!(fields[0].computed_result.as_deref(), Some("04"));
    assert_eq!(fields[1].kind, FieldKind::Page);
    assert_eq!(fields[1].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[1].result, "stale restart arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[2].kind, FieldKind::Page);
    assert_eq!(fields[2].instruction, "PAGE \\* CardText \\* Upper");
    assert_eq!(fields[2].result, "stale restart card upper");
    assert_eq!(fields[2].computed_result.as_deref(), Some("FOUR"));
    assert_eq!(fields[3].kind, FieldKind::Page);
    assert_eq!(fields[3].instruction, "PAGE");
    assert_eq!(fields[3].result, "cached ambiguous page");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].kind, FieldKind::Page);
    assert_eq!(fields[4].instruction, "PAGE \\* roman");
    assert_eq!(fields[4].result, "stale rendered roman page");
    assert_eq!(fields[4].computed_result.as_deref(), Some("v"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("04")
            && main_text.contains("4")
            && main_text.contains("FOUR")
            && main_text.contains("cached ambiguous page")
            && main_text.contains("v"),
        "computed trusted PAGE fields and cached ambiguous PAGE field should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale restart decimal zero")
            && !main_text.contains("stale restart arabic")
            && !main_text.contains("stale restart card upper")
            && !main_text.contains("stale rendered roman page"),
        "computed PAGE fields should replace stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Page,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );
}

#[test]
fn docx_page_field_computes_restarted_section_display_page_after_visible_intro() {
    let doc = Document::open(&page_field_visible_intro_section_page_number_restart_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "stale restarted current page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("7"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("7") && !main_text.contains("stale restarted current page"),
        "restarted section display page should compute for PAGE fields: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_field_computes_page_break_before_current_page_after_visible_intro() {
    let doc = Document::open(&page_field_page_break_before_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[0].result, "stale break-before page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("stale break-before page"),
        "pageBreakBefore PAGE fields should use the explicit paragraph break: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_empty_section_type_defaults_to_next_page_for_page_accounting() {
    let doc = Document::open(&empty_section_type_page_accounting_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "stale page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF DefaultTyped \\h");
    assert_eq!(fields[1].result, "stale ref");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2")
            && !main_text.contains("stale page")
            && !main_text.contains("stale ref"),
        "empty section type should use the default next-page break: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_disabled_page_break_before_does_not_set_paragraph_break_flag() {
    let doc = Document::open(&disabled_page_break_before_docx()).expect("fixture opens");
    let [Block::Paragraph(paragraph)] = &doc.model().blocks[..] else {
        panic!("expected one paragraph");
    };

    assert!(!paragraph.props.page_break_before);
}

#[test]
fn docx_page_fields_follow_accepted_wrappers_and_single_alternate_branch() {
    let doc = Document::open(&wrapped_page_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].instruction, "PAGE \\* Ordinal");
    assert_eq!(fields[1].computed_result.as_deref(), Some("3rd"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && main_text.contains("3rd"),
        "computed PAGE fields should be materialized in accepted/current text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale inserted page") && !main_text.contains("stale alternate page"),
        "computed PAGE fields should replace stale cached text: {main_text:?}"
    );
    assert!(doc.report().features.unsupported_field_kinds.is_empty());
}

#[test]
fn docx_merge_fields_are_named_field_kind() {
    let doc = Document::open(&merge_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::MergeField);
    assert_eq!(
        fields[0].instruction,
        "MERGEFIELD client-name \\* MERGEFORMAT"
    );
    assert_eq!(fields[0].result, "Acme");
    assert_eq!(fields[1].kind, FieldKind::MergeField);
    assert_eq!(
        fields[1].instruction,
        "MERGEFIELD \"project-name\" \\* MERGEFORMAT"
    );
    assert_eq!(fields[1].result, "Roadmap");
}

#[test]
fn docx_merge_field_diagnostics_reject_missing_name_before_format_tail() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" MERGEFIELD ClientName \* MERGEFORMAT "><w:r><w:t>Acme</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEFIELD \* MERGEFORMAT ClientName "><w:r><w:t>cached missing merge name</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::MergeField);
    assert_eq!(
        fields[0].instruction,
        "MERGEFIELD ClientName \\* MERGEFORMAT"
    );
    assert_eq!(fields[1].kind, FieldKind::MergeField);
    assert_eq!(
        fields[1].instruction,
        "MERGEFIELD \\* MERGEFORMAT ClientName"
    );

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::MergeField,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
    assert!(doc.main_text().contains("cached missing merge name"));
}

#[test]
fn docx_sequence_fields_compute_source_order_numbers() {
    let doc = Document::open(&sequence_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 9);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure");
    assert_eq!(fields[0].result, "stale figure one");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Sequence);
    assert_eq!(fields[1].instruction, "SEQ Figure");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].kind, FieldKind::Sequence);
    assert_eq!(fields[2].instruction, "SEQ Figure \\r 7");
    assert_eq!(fields[2].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[3].kind, FieldKind::Sequence);
    assert_eq!(fields[3].instruction, "SEQ Figure \\c");
    assert_eq!(fields[3].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[4].kind, FieldKind::Sequence);
    assert_eq!(fields[4].instruction, "SEQ Figure \\h");
    assert_eq!(fields[4].computed_result.as_deref(), Some(""));
    assert_eq!(fields[5].kind, FieldKind::Sequence);
    assert_eq!(fields[5].instruction, "SEQ Figure");
    assert_eq!(fields[5].computed_result.as_deref(), Some("9"));
    assert_eq!(fields[6].kind, FieldKind::Sequence);
    assert_eq!(fields[6].instruction, "SEQ Figure \\r -1");
    assert_eq!(fields[6].result, "cached invalid reset");
    assert_eq!(fields[6].computed_result, None);
    assert_eq!(fields[7].kind, FieldKind::Sequence);
    assert_eq!(fields[7].instruction, "SEQ Figure");
    assert_eq!(fields[7].computed_result.as_deref(), Some("10"));
    assert_eq!(fields[8].kind, FieldKind::Sequence);
    assert_eq!(fields[8].instruction, "SEQ Appendix \\* roman");
    assert_eq!(fields[8].computed_result.as_deref(), Some("i"));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Sequence,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert_eq!(
        main_text
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>(),
        vec!["1", "2", "7", "7", "9", "cached invalid reset", "10", "i"]
    );
    assert!(
        !main_text.contains("stale figure")
            && !main_text.contains("stale hidden figure")
            && !main_text.contains("stale appendix roman"),
        "computed SEQ field text should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_sequence_heading_reset_is_known_uncomputed_syntax() {
    let doc = Document::open(&sequence_heading_reset_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Sequence);
    assert_eq!(fields[1].instruction, "SEQ Figure \\s 1");
    assert_eq!(fields[1].result, "cached heading reset");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Sequence);
    assert_eq!(fields[2].instruction, "SEQ Figure");
    assert_eq!(fields[2].computed_result.as_deref(), Some("2"));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Sequence,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );

    assert_eq!(
        doc.main_text()
            .lines()
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>(),
        vec!["1", "cached heading reset", "2"]
    );
}

#[test]
fn docx_document_info_fields_are_named_cached_display_fields() {
    let doc = Document::open(&document_info_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 7);
    assert_eq!(fields[0].kind, FieldKind::DocumentInfo("DATE".to_string()));
    assert_eq!(fields[0].instruction, "DATE \\@ \"yyyy-MM-dd\"");
    assert_eq!(fields[0].result, "2026-06-24");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::DocumentInfo("TIME".to_string()));
    assert_eq!(fields[1].instruction, "TIME \\@ \"HH:mm\"");
    assert_eq!(fields[1].result, "14:35");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::DocumentInfo("AUTHOR".to_string())
    );
    assert_eq!(fields[2].result, "Hyunjo Jung");
    assert_eq!(
        fields[3].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[3].result, "Example Co.");
    assert_eq!(
        fields[4].kind,
        FieldKind::DocumentInfo("NUMPAGES".to_string())
    );
    assert_eq!(fields[4].result, "12");
    assert_eq!(
        fields[5].kind,
        FieldKind::DocumentInfo("EDITTIME".to_string())
    );
    assert_eq!(fields[5].result, "42");
    assert_eq!(
        fields[6].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[6].instruction, "DOCPROPERTY \"Broken Name ");
    assert_eq!(fields[6].result, "cached broken property");
    assert_eq!(fields[6].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::DocumentInfo("DOCPROPERTY".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2026-06-24")
            && main_text.contains("14:35")
            && main_text.contains("Hyunjo Jung")
            && main_text.contains("Example Co.")
            && main_text.contains("12")
            && main_text.contains("42")
            && main_text.contains("cached broken property"),
        "document-info fields should preserve cached display text: {main_text:?}"
    );
}

#[test]
fn docx_file_size_fields_compute_unit_switches_from_package_size() {
    let fixture = file_size_switch_field_docx();
    let expected_bytes = fixture.len().to_string();
    let expected_kilobytes = ((fixture.len() + 500) / 1_000).to_string();
    let expected_megabytes = ((fixture.len() + 500_000) / 1_000_000).to_string();
    let doc = Document::open(&fixture).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    for field in &fields {
        assert_eq!(field.kind, FieldKind::DocumentInfo("FILESIZE".to_string()));
    }
    assert_eq!(fields[0].instruction, "FILESIZE");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some(expected_bytes.as_str())
    );
    assert_eq!(fields[1].instruction, "FILESIZE \\k");
    assert_eq!(
        fields[1].computed_result.as_deref(),
        Some(expected_kilobytes.as_str())
    );
    assert_eq!(fields[2].instruction, "FILESIZE \\m");
    assert_eq!(
        fields[2].computed_result.as_deref(),
        Some(expected_megabytes.as_str())
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale bytes")
            && !main_text.contains("stale kilobytes")
            && !main_text.contains("stale megabytes"),
        "computed FILESIZE fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_user_info_fields_compute_explicit_literal_overrides() {
    let doc = Document::open(&user_info_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentInfo("USERNAME".to_string())
    );
    assert_eq!(fields[0].instruction, "USERNAME");
    assert_eq!(fields[0].result, "cached user name");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::DocumentInfo("USERINITIALS".to_string())
    );
    assert_eq!(fields[1].instruction, "USERINITIALS");
    assert_eq!(fields[1].result, "cached initials");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::DocumentInfo("USERADDRESS".to_string())
    );
    assert_eq!(fields[2].instruction, "USERADDRESS");
    assert_eq!(fields[2].result, "cached address");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].instruction,
        "USERNAME \"Casey Reviewer\" \\* Upper"
    );
    assert_eq!(fields[3].result, "stale override name");
    assert_eq!(fields[3].computed_result.as_deref(), Some("CASEY REVIEWER"));
    assert_eq!(fields[4].instruction, "USERINITIALS \"cr\" \\* Upper");
    assert_eq!(fields[4].result, "stale override initials");
    assert_eq!(fields[4].computed_result.as_deref(), Some("CR"));
    assert_eq!(
        fields[5].instruction,
        "USERADDRESS \"Review desk, Seoul\" \\* Upper"
    );
    assert_eq!(fields[5].result, "stale override address");
    assert_eq!(
        fields[5].computed_result.as_deref(),
        Some("REVIEW DESK, SEOUL")
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached user name")
            && main_text.contains("cached initials")
            && main_text.contains("cached address")
            && main_text.contains("CASEY REVIEWER")
            && main_text.contains("CR")
            && main_text.contains("REVIEW DESK, SEOUL"),
        "cached user-info fields and computed literal overrides should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale override name")
            && !main_text.contains("stale override initials")
            && !main_text.contains("stale override address"),
        "computed user-info overrides should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_document_info_fields_compute_package_properties_when_available() {
    let fixture = document_info_package_properties_field_docx();
    let expected_file_size = fixture.len().to_string();
    let doc = Document::open(&fixture).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 46);
    assert_eq!(
        doc.model()
            .custom_properties
            .get("Client Name")
            .map(String::as_str),
        Some("acme launch")
    );
    assert_eq!(fields[0].kind, FieldKind::DocumentInfo("TITLE".to_string()));
    assert_eq!(fields[0].result, "stale title");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Quarter Plan"));
    assert_eq!(
        fields[1].kind,
        FieldKind::DocumentInfo("AUTHOR".to_string())
    );
    assert_eq!(fields[1].computed_result.as_deref(), Some("Hyunjo Jung"));
    assert_eq!(fields[2].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[2].computed_result.as_deref(), Some("QUARTER PLAN"));
    assert_eq!(
        fields[3].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[3].computed_result.as_deref(), Some("LAUNCH"));
    assert_eq!(
        fields[4].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[4].computed_result.as_deref(), Some("Field coverage"));
    assert_eq!(
        fields[5].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[5].computed_result.as_deref(), Some("rdoc,fields"));
    assert_eq!(
        fields[6].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[6].computed_result.as_deref(), Some("Operations"));
    assert_eq!(fields[7].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[7].computed_result.as_deref(), Some("Draft"));
    assert_eq!(
        fields[8].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[8].computed_result.as_deref(), Some("1.2"));
    assert_eq!(
        fields[9].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[9].computed_result.as_deref(), Some("Acme Launch"));
    assert_eq!(
        fields[10].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[10].computed_result.as_deref(), Some("7"));
    assert_eq!(
        fields[11].kind,
        FieldKind::DocumentInfo("DOCVARIABLE".to_string())
    );
    assert_eq!(fields[11].computed_result.as_deref(), Some("ALPHA-42"));
    assert_eq!(
        fields[12].kind,
        FieldKind::DocumentInfo("CREATEDATE".to_string())
    );
    assert_eq!(
        fields[12].computed_result.as_deref(),
        Some("Monday, June 1, 2026")
    );
    assert_eq!(
        fields[13].kind,
        FieldKind::DocumentInfo("SAVEDATE".to_string())
    );
    assert_eq!(
        fields[13].computed_result.as_deref(),
        Some("Jun 02, 2026 03:04:05")
    );
    assert_eq!(
        fields[14].kind,
        FieldKind::DocumentInfo("PRINTDATE".to_string())
    );
    assert_eq!(
        fields[14].computed_result.as_deref(),
        Some("26-6-3 4:05 AM")
    );
    assert_eq!(
        fields[15].kind,
        FieldKind::DocumentInfo("NUMPAGES".to_string())
    );
    assert_eq!(fields[15].computed_result.as_deref(), Some("12"));
    assert_eq!(
        fields[16].kind,
        FieldKind::DocumentInfo("NUMWORDS".to_string())
    );
    assert_eq!(fields[16].computed_result.as_deref(), Some("321"));
    assert_eq!(
        fields[17].kind,
        FieldKind::DocumentInfo("NUMCHARS".to_string())
    );
    assert_eq!(fields[17].computed_result.as_deref(), Some("2048"));
    assert_eq!(
        fields[18].kind,
        FieldKind::DocumentInfo("EDITTIME".to_string())
    );
    assert_eq!(fields[18].computed_result.as_deref(), Some("42"));
    assert_eq!(
        fields[19].kind,
        FieldKind::DocumentInfo("TEMPLATE".to_string())
    );
    assert_eq!(fields[19].computed_result.as_deref(), Some("NORMAL.DOTM"));
    assert_eq!(
        fields[20].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[20].computed_result.as_deref(), Some("12"));
    assert_eq!(fields[21].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[21].computed_result.as_deref(), Some("321"));
    assert_eq!(
        fields[22].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[22].computed_result.as_deref(), Some("2500"));
    assert_eq!(fields[23].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[23].computed_result.as_deref(), Some("42"));
    assert_eq!(
        fields[24].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[24].computed_result.as_deref(), Some("EXAMPLE CO"));
    assert_eq!(fields[25].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[25].computed_result.as_deref(), Some("Document Lead"));
    assert_eq!(
        fields[26].kind,
        FieldKind::DocumentInfo("LASTSAVEDBY".to_string())
    );
    assert_eq!(fields[26].computed_result.as_deref(), Some("Reviewer"));
    assert_eq!(
        fields[27].kind,
        FieldKind::DocumentInfo("CATEGORY".to_string())
    );
    assert_eq!(fields[27].computed_result.as_deref(), Some("OPERATIONS"));
    assert_eq!(
        fields[28].kind,
        FieldKind::DocumentInfo("CONTENTSTATUS".to_string())
    );
    assert_eq!(fields[28].computed_result.as_deref(), Some("Draft"));
    assert_eq!(
        fields[29].kind,
        FieldKind::DocumentInfo("VERSION".to_string())
    );
    assert_eq!(fields[29].computed_result.as_deref(), Some("1.2"));
    assert_eq!(
        fields[30].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(
        fields[30].computed_result.as_deref(),
        Some("https://docs.example/base/")
    );
    assert_eq!(fields[31].kind, FieldKind::DocumentInfo("INFO".to_string()));
    assert_eq!(fields[31].computed_result.as_deref(), Some("4"));
    assert_eq!(
        fields[32].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(fields[32].computed_result.as_deref(), Some("true"));
    assert_eq!(
        fields[33].kind,
        FieldKind::DocumentInfo("CREATOR".to_string())
    );
    assert_eq!(fields[33].computed_result.as_deref(), Some("HYUNJO JUNG"));
    assert_eq!(
        fields[34].kind,
        FieldKind::DocumentInfo("DESCRIPTION".to_string())
    );
    assert_eq!(
        fields[34].computed_result.as_deref(),
        Some("FIELD COVERAGE")
    );
    assert_eq!(
        fields[35].kind,
        FieldKind::DocumentInfo("KEYWORD".to_string())
    );
    assert_eq!(fields[35].computed_result.as_deref(), Some("RDOC,FIELDS"));
    assert_eq!(
        fields[36].kind,
        FieldKind::DocumentInfo("LASTMODIFIEDBY".to_string())
    );
    assert_eq!(fields[36].computed_result.as_deref(), Some("REVIEWER"));
    assert_eq!(
        fields[37].kind,
        FieldKind::DocumentInfo("APPLICATION".to_string())
    );
    assert_eq!(
        fields[37].computed_result.as_deref(),
        Some("RDOC FIELD ENGINE")
    );
    assert_eq!(
        fields[38].kind,
        FieldKind::DocumentInfo("APPVERSION".to_string())
    );
    assert_eq!(fields[38].computed_result.as_deref(), Some("16.0000"));
    assert_eq!(
        fields[39].kind,
        FieldKind::DocumentInfo("MANAGER".to_string())
    );
    assert_eq!(fields[39].computed_result.as_deref(), Some("DOCUMENT LEAD"));
    assert_eq!(
        fields[40].kind,
        FieldKind::DocumentInfo("COMPANY".to_string())
    );
    assert_eq!(fields[40].computed_result.as_deref(), Some("EXAMPLE CO"));
    assert_eq!(
        fields[41].kind,
        FieldKind::DocumentInfo("HYPERLINKBASE".to_string())
    );
    assert_eq!(
        fields[41].computed_result.as_deref(),
        Some("https://docs.example/base/")
    );
    assert_eq!(
        fields[42].kind,
        FieldKind::DocumentInfo("DOCSECURITY".to_string())
    );
    assert_eq!(fields[42].computed_result.as_deref(), Some("4"));
    assert_eq!(
        fields[43].kind,
        FieldKind::DocumentInfo("LINKSUPTODATE".to_string())
    );
    assert_eq!(fields[43].computed_result.as_deref(), Some("true"));
    assert_eq!(
        fields[44].kind,
        FieldKind::DocumentInfo("DOCPROPERTY".to_string())
    );
    assert_eq!(
        fields[44].instruction,
        r#"DOCPROPERTY "Review Date" \@ "MMM d, yyyy""#
    );
    assert_eq!(fields[44].result, "stale review date");
    assert_eq!(fields[44].computed_result.as_deref(), Some("Jun 15, 2026"));
    assert_eq!(
        fields[45].kind,
        FieldKind::DocumentInfo("FILESIZE".to_string())
    );
    assert_eq!(fields[45].instruction, "FILESIZE");
    assert_eq!(fields[45].result, "stale file size");
    assert_eq!(
        fields[45].computed_result.as_deref(),
        Some(expected_file_size.as_str())
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Quarter Plan")
            && main_text.contains("Hyunjo Jung")
            && main_text.contains("QUARTER PLAN")
            && main_text.contains("LAUNCH")
            && main_text.contains("Field coverage")
            && main_text.contains("rdoc,fields")
            && main_text.contains("Operations")
            && main_text.contains("Draft")
            && main_text.contains("1.2")
            && main_text.contains("Acme Launch")
            && main_text.contains("7")
            && main_text.contains("ALPHA-42")
            && main_text.contains("Monday, June 1, 2026")
            && main_text.contains("Jun 02, 2026 03:04:05")
            && main_text.contains("26-6-3 4:05 AM")
            && main_text.contains("12")
            && main_text.contains("321")
            && main_text.contains("2048")
            && main_text.contains("42")
            && main_text.contains("NORMAL.DOTM")
            && main_text.contains("2500")
            && main_text.contains("EXAMPLE CO")
            && main_text.contains("Document Lead")
            && main_text.contains("Reviewer")
            && main_text.contains("OPERATIONS")
            && main_text.contains("https://docs.example/base/")
            && main_text.contains("true")
            && main_text.contains("HYUNJO JUNG")
            && main_text.contains("FIELD COVERAGE")
            && main_text.contains("RDOC,FIELDS")
            && main_text.contains("REVIEWER")
            && main_text.contains("RDOC FIELD ENGINE")
            && main_text.contains("16.0000")
            && main_text.contains("DOCUMENT LEAD")
            && main_text.matches("EXAMPLE CO").count() >= 2
            && main_text.matches("https://docs.example/base/").count() >= 2
            && main_text.matches("true").count() >= 2
            && main_text.contains("Jun 15, 2026")
            && main_text.contains(&expected_file_size),
        "computed package property field text should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale title")
            && !main_text.contains("stale author")
            && !main_text.contains("stale info title")
            && !main_text.contains("stale subject")
            && !main_text.contains("stale comments")
            && !main_text.contains("stale keywords")
            && !main_text.contains("stale category")
            && !main_text.contains("stale content status")
            && !main_text.contains("stale version")
            && !main_text.contains("stale client")
            && !main_text.contains("stale score")
            && !main_text.contains("stale variable")
            && !main_text.contains("stale created date")
            && !main_text.contains("stale saved date")
            && !main_text.contains("stale print date")
            && !main_text.contains("stale pages")
            && !main_text.contains("stale words")
            && !main_text.contains("stale chars")
            && !main_text.contains("stale edit time")
            && !main_text.contains("stale template")
            && !main_text.contains("stale docproperty pages")
            && !main_text.contains("stale info words")
            && !main_text.contains("stale chars with spaces")
            && !main_text.contains("stale info total time")
            && !main_text.contains("stale company")
            && !main_text.contains("stale manager")
            && !main_text.contains("stale editor")
            && !main_text.contains("stale direct category")
            && !main_text.contains("stale direct content status")
            && !main_text.contains("stale direct version")
            && !main_text.contains("stale hyperlink base")
            && !main_text.contains("stale doc security")
            && !main_text.contains("stale links up to date")
            && !main_text.contains("stale creator alias")
            && !main_text.contains("stale description alias")
            && !main_text.contains("stale keyword alias")
            && !main_text.contains("stale modified alias")
            && !main_text.contains("stale application")
            && !main_text.contains("stale app version")
            && !main_text.contains("stale direct manager")
            && !main_text.contains("stale direct company")
            && !main_text.contains("stale direct hyperlink base")
            && !main_text.contains("stale direct doc security")
            && !main_text.contains("stale direct links up to date")
            && !main_text.contains("stale review date")
            && !main_text.contains("stale file size"),
        "computed package property field text should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_dynamic_fields_compute_formula_quote_if_compare_and_literal_set_ref() {
    let doc = Document::open(&dynamic_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 24);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, "= (2 + 3) * 4");
    assert_eq!(fields[0].result, "stale formula");
    assert_eq!(fields[0].computed_result.as_deref(), Some("20"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= 10 / 4 \# "0.00""#);
    assert_eq!(fields[1].result, "stale formatted formula");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2.50"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(fields[2].result, "stale true");
    assert_eq!(fields[2].computed_result.as_deref(), Some("yes"));
    assert_eq!(fields[3].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(fields[3].result, "stale false");
    assert_eq!(fields[3].computed_result.as_deref(), Some("no"));
    assert_eq!(fields[4].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(fields[4].result, "stale compact");
    assert_eq!(fields[4].computed_result.as_deref(), Some("big"));
    assert_eq!(fields[5].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[5].result, "stale compare true");
    assert_eq!(fields[5].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[6].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[6].result, "stale compare wildcard");
    assert_eq!(fields[6].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[7].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[7].result, "stale compare false");
    assert_eq!(fields[7].computed_result.as_deref(), Some("0"));
    assert_eq!(fields[8].kind, FieldKind::Dynamic("QUOTE".to_string()));
    assert_eq!(fields[8].result, "literal");
    assert_eq!(fields[8].computed_result.as_deref(), Some("literal"));
    assert_eq!(fields[9].kind, FieldKind::Dynamic("FILLIN".to_string()));
    assert_eq!(fields[9].result, "Acme");
    assert_eq!(fields[9].computed_result, None);
    assert_eq!(fields[10].kind, FieldKind::Dynamic("ASK".to_string()));
    assert_eq!(fields[10].instruction, "ASK ClientCode \"Client code?\"");
    assert_eq!(fields[10].result, "cached ask");
    assert_eq!(fields[10].computed_result, None);
    assert_eq!(fields[11].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[11].instruction, "SET ClientName \"Acme\"");
    assert_eq!(fields[11].result, "cached set");
    assert_eq!(fields[11].computed_result.as_deref(), Some(""));
    assert_eq!(fields[12].kind, FieldKind::Ref);
    assert_eq!(fields[12].instruction, r#"REF ClientName \* Upper"#);
    assert_eq!(fields[12].result, "stale set ref");
    assert_eq!(fields[12].computed_result.as_deref(), Some("ACME"));
    assert_eq!(fields[13].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(
        fields[13].instruction,
        "SET ClientTier \"Gold\" \\* MERGEFORMAT"
    );
    assert_eq!(fields[13].result, "cached formatted set");
    assert_eq!(fields[13].computed_result.as_deref(), Some(""));
    assert_eq!(fields[14].kind, FieldKind::Ref);
    assert_eq!(fields[14].instruction, "REF ClientTier");
    assert_eq!(fields[14].result, "stale formatted set ref");
    assert_eq!(fields[14].computed_result.as_deref(), Some("Gold"));
    assert_eq!(fields[15].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[15].instruction, "SET ClientCode Client-42");
    assert_eq!(fields[15].result, "cached unsupported set");
    assert_eq!(fields[15].computed_result.as_deref(), Some(""));
    assert_eq!(fields[16].kind, FieldKind::Dynamic("NEXT".to_string()));
    assert_eq!(fields[16].instruction, "NEXT");
    assert_eq!(fields[16].computed_result.as_deref(), Some(""));
    assert_eq!(fields[17].kind, FieldKind::Dynamic("NEXTIF".to_string()));
    assert_eq!(fields[17].instruction, "NEXTIF 1 = 1");
    assert_eq!(fields[17].computed_result.as_deref(), Some(""));
    assert_eq!(fields[18].kind, FieldKind::Dynamic("SKIPIF".to_string()));
    assert_eq!(fields[18].instruction, "SKIPIF 1 = 0");
    assert_eq!(fields[18].computed_result.as_deref(), Some(""));
    assert_eq!(fields[19].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(
        fields[19].instruction,
        "IF 1e2 = 100 \"scientific\" \"bad\""
    );
    assert_eq!(fields[19].result, "stale scientific if");
    assert_eq!(fields[19].computed_result.as_deref(), Some("scientific"));
    assert_eq!(fields[20].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[20].instruction, "COMPARE 1e309 > 0");
    assert_eq!(fields[20].result, "cached nonfinite compare");
    assert_eq!(fields[20].computed_result, None);
    assert_eq!(fields[21].kind, FieldKind::Dynamic("NEXTIF".to_string()));
    assert_eq!(fields[21].instruction, "NEXTIF City = \"Tokyo\"");
    assert_eq!(fields[21].result, "cached unsupported nextif");
    assert_eq!(fields[21].computed_result, None);
    assert_eq!(fields[22].kind, FieldKind::Dynamic("FILLIN".to_string()));
    assert_eq!(fields[22].instruction, "FILLIN \"broken prompt ");
    assert_eq!(fields[22].result, "cached broken fillin");
    assert_eq!(fields[22].computed_result, None);
    assert_eq!(fields[23].kind, FieldKind::Dynamic("NEXTIF".to_string()));
    assert_eq!(fields[23].instruction, "NEXTIF 1 =");
    assert_eq!(fields[23].result, "cached broken nextif");
    assert_eq!(fields[23].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Dynamic("FILLIN".to_string()),
                count: 2,
            },
            FieldKindCount {
                kind: FieldKind::Dynamic("ASK".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Dynamic("COMPARE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Dynamic("NEXTIF".to_string()),
                count: 2,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 3,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 3,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("20")
            && main_text.contains("2.50")
            && main_text.contains("yes")
            && main_text.contains("no")
            && main_text.contains("big")
            && main_text.contains("1\n1\n0")
            && main_text.contains("literal")
            && main_text.contains("Acme")
            && main_text.contains("cached ask")
            && main_text.contains("ACME")
            && main_text.contains("Gold")
            && main_text.contains("scientific")
            && main_text.contains("cached nonfinite compare")
            && main_text.contains("cached unsupported nextif")
            && main_text.contains("cached broken fillin")
            && main_text.contains("cached broken nextif"),
        "computed/cached dynamic field results should be materialized in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale true")
            && !main_text.contains("stale false")
            && !main_text.contains("stale compact")
            && !main_text.contains("stale compare")
            && !main_text.contains("stale formula")
            && !main_text.contains("stale formatted formula")
            && !main_text.contains("stale scientific if")
            && !main_text.contains("cached set")
            && !main_text.contains("cached formatted set")
            && !main_text.contains("stale formatted set ref")
            && !main_text.contains("cached unsupported set")
            && !main_text.contains("cached next")
            && !main_text.contains("cached nextif")
            && !main_text.contains("cached skipif")
            && !main_text.contains("stale set ref"),
        "computed formula/IF/COMPARE results should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_malformed_if_reports_unsupported_switch_without_flagging_data_dependent_if() {
    let doc = Document::open(&if_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(
        fields[0].instruction,
        r#"IF CustomerTier = "Gold" "ship" "hold""#
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Dynamic("IF".to_string()));
    assert_eq!(fields[1].instruction, "IF 1 =");
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("IF".to_string()),
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached data if") && main_text.contains("cached broken if"),
        "uncomputed IF fields should preserve cached text: {main_text:?}"
    );
}

#[test]
fn docx_malformed_compare_reports_unsupported_switch_without_flagging_data_compare() {
    let doc = Document::open(&compare_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[0].instruction, r#"COMPARE CustomerTier = "Gold""#);
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[1].instruction, "COMPARE 1e309 > 0");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Dynamic("COMPARE".to_string()));
    assert_eq!(fields[2].instruction, r#"COMPARE \o = "Gold""#);
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("COMPARE".to_string()),
            count: 3,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached data compare")
            && main_text.contains("cached nonfinite compare")
            && main_text.contains("cached switch compare"),
        "uncomputed COMPARE fields should preserve cached text: {main_text:?}"
    );
}

#[test]
fn docx_malformed_formula_picture_reports_unsupported_switch_without_flagging_data_formula() {
    let doc = Document::open(&formula_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= CustomerTotal \# "0.00""#);
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= 1 \# "0.00 "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached data formula") && main_text.contains("cached broken formula"),
        "uncomputed formula fields should preserve cached text: {main_text:?}"
    );
}

#[test]
fn docx_malformed_set_reports_unsupported_switch_without_flagging_ambiguous_set() {
    let doc = Document::open(&set_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[0].instruction, "SET ClientName Client 42");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[1].instruction, r#"SET ClientName "Acme "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("SET".to_string()),
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached ambiguous set") && main_text.contains("cached broken set"),
        "uncomputed SET fields should preserve cached text: {main_text:?}"
    );
}

#[test]
fn docx_prompt_fields_compute_explicit_defaults() {
    let doc = Document::open(&prompt_default_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("FILLIN".to_string()));
    assert_eq!(fields[0].instruction, r#"FILLIN "Client?" \d "Acme""#);
    assert_eq!(fields[0].result, "stale fillin");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Acme"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("FILLIN".to_string()));
    assert_eq!(
        fields[1].instruction,
        r#"FILLIN "Department?" \d "ops" \* Upper"#
    );
    assert_eq!(fields[1].result, "stale formatted fillin");
    assert_eq!(fields[1].computed_result.as_deref(), Some("OPS"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("ASK".to_string()));
    assert_eq!(
        fields[2].instruction,
        r#"ASK ClientCode "Client code?" \d "ac-42" \o"#
    );
    assert_eq!(fields[2].result, "cached ask default");
    assert_eq!(fields[2].computed_result.as_deref(), Some(""));
    assert_eq!(fields[3].kind, FieldKind::Ref);
    assert_eq!(fields[3].instruction, r#"REF ClientCode \* Upper"#);
    assert_eq!(fields[3].result, "stale ask ref");
    assert_eq!(fields[3].computed_result.as_deref(), Some("AC-42"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Acme") && main_text.contains("OPS") && main_text.contains("AC-42"),
        "computed prompt defaults should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale fillin")
            && !main_text.contains("stale formatted fillin")
            && !main_text.contains("cached ask default")
            && !main_text.contains("stale ask ref"),
        "computed prompt defaults should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_prompt_fields_compute_compact_explicit_defaults() {
    let doc = Document::open(&compact_prompt_default_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("FILLIN".to_string()));
    assert_eq!(fields[0].instruction, r#"FILLIN "Client?" \d"Acme""#);
    assert_eq!(fields[0].result, "stale compact fillin");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Acme"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("ASK".to_string()));
    assert_eq!(
        fields[1].instruction,
        r#"ASK ClientCode "Client code?" \d"ac-42""#
    );
    assert_eq!(fields[1].result, "cached compact ask");
    assert_eq!(fields[1].computed_result.as_deref(), Some(""));
    assert_eq!(fields[2].kind, FieldKind::Ref);
    assert_eq!(fields[2].instruction, r#"REF ClientCode \* Upper"#);
    assert_eq!(fields[2].result, "stale compact ask ref");
    assert_eq!(fields[2].computed_result.as_deref(), Some("AC-42"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Acme") && main_text.contains("AC-42"),
        "computed compact prompt defaults should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale compact fillin")
            && !main_text.contains("cached compact ask")
            && !main_text.contains("stale compact ask ref"),
        "computed compact prompt defaults should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_unquoted_single_token_set_fields_feed_later_refs() {
    let doc = Document::open(&unquoted_set_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[0].instruction, "SET ClientCode Client-42");
    assert_eq!(fields[0].result, "cached set");
    assert_eq!(fields[0].computed_result.as_deref(), Some(""));
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, r#"REF ClientCode \* Upper"#);
    assert_eq!(fields[1].result, "stale ref");
    assert_eq!(fields[1].computed_result.as_deref(), Some("CLIENT-42"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("SET".to_string()));
    assert_eq!(fields[2].instruction, "SET ClientName Client 42");
    assert_eq!(fields[2].result, "cached multi-token set");
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("SET".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("CLIENT-42") && main_text.contains("cached multi-token set"),
        "unquoted single-token SET should feed REF while ambiguous SET stays cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("cached set") && !main_text.contains("stale ref"),
        "computed SET/REF output should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_sectioned_numeric_pictures() {
    let doc = Document::open(&formula_sectioned_numeric_picture_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(
        fields[0].instruction,
        r#"= 1245.65 \# "$#,##0.00;-$#,##0.00""#
    );
    assert_eq!(fields[0].result, "stale positive section");
    assert_eq!(fields[0].computed_result.as_deref(), Some("$1,245.65"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(
        fields[1].instruction,
        r#"= 0 - 345.56 \# "$#,##0.00;-$#,##0.00""#
    );
    assert_eq!(fields[1].result, "stale negative section");
    assert_eq!(fields[1].computed_result.as_deref(), Some("-$345.56"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(
        fields[2].instruction,
        r#"= 0 \# "$#,##0.00;($#,##0.00);$0""#
    );
    assert_eq!(fields[2].result, "stale zero section");
    assert_eq!(fields[2].computed_result.as_deref(), Some("$0"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("$1,245.65")
            && main_text.contains("-$345.56")
            && main_text.contains("$0"),
        "computed sectioned numeric pictures should materialize selected sections: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale positive section")
            && !main_text.contains("stale negative section")
            && !main_text.contains("stale zero section"),
        "computed sectioned numeric pictures should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_numeric_picture_sign_controls() {
    let doc = Document::open(&formula_sign_control_numeric_picture_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= 100 - 90 \# +##"#);
    assert_eq!(fields[0].result, "stale plus positive");
    assert_eq!(fields[0].computed_result.as_deref(), Some("+10"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= 90 - 100 \# +##"#);
    assert_eq!(fields[1].result, "stale plus negative");
    assert_eq!(fields[1].computed_result.as_deref(), Some("-10"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= 10 - 90 \# -##"#);
    assert_eq!(fields[2].result, "stale minus negative");
    assert_eq!(fields[2].computed_result.as_deref(), Some("-80"));
    assert_eq!(fields[3].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[3].instruction, r#"= 10 \# -##"#);
    assert_eq!(fields[3].result, "stale minus positive");
    assert_eq!(fields[3].computed_result.as_deref(), Some(" 10"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("+10")
            && main_text.contains("-10")
            && main_text.contains("-80")
            && main_text.contains("10"),
        "computed sign-control numeric pictures should materialize sign-controlled results: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale plus positive")
            && !main_text.contains("stale plus negative")
            && !main_text.contains("stale minus negative")
            && !main_text.contains("stale minus positive"),
        "computed sign-control numeric pictures should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_literal_numeric_functions() {
    let doc = Document::open(&formula_function_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 15);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= ABS(-22)"#);
    assert_eq!(fields[0].result, "stale abs");
    assert_eq!(fields[0].computed_result.as_deref(), Some("22"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= SUM(1, 2, 3)"#);
    assert_eq!(fields[1].result, "stale sum");
    assert_eq!(fields[1].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= PRODUCT(2, 3, 4)"#);
    assert_eq!(fields[2].result, "stale product");
    assert_eq!(fields[2].computed_result.as_deref(), Some("24"));
    assert_eq!(fields[3].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[3].instruction, r#"= MIN(5, -2, 9)"#);
    assert_eq!(fields[3].result, "stale min");
    assert_eq!(fields[3].computed_result.as_deref(), Some("-2"));
    assert_eq!(fields[4].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[4].instruction, r#"= MAX(5, -2, 9)"#);
    assert_eq!(fields[4].result, "stale max");
    assert_eq!(fields[4].computed_result.as_deref(), Some("9"));
    assert_eq!(fields[5].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[5].instruction, r#"= ROUND(123.456, 2)"#);
    assert_eq!(fields[5].result, "stale round");
    assert_eq!(fields[5].computed_result.as_deref(), Some("123.46"));
    assert_eq!(fields[6].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[6].instruction, r#"= INT(5.67)"#);
    assert_eq!(fields[6].result, "stale int");
    assert_eq!(fields[6].computed_result.as_deref(), Some("5"));
    assert_eq!(fields[7].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[7].instruction, r#"= SIGN(-11)"#);
    assert_eq!(fields[7].result, "stale sign");
    assert_eq!(fields[7].computed_result.as_deref(), Some("-1"));
    assert_eq!(fields[8].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[8].instruction, r#"= SUM(100, 20) \# "$0""#);
    assert_eq!(fields[8].result, "stale formatted function");
    assert_eq!(fields[8].computed_result.as_deref(), Some("$120"));
    assert_eq!(fields[9].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[9].instruction, r#"= 2 ^ 3"#);
    assert_eq!(fields[9].result, "stale exponent");
    assert_eq!(fields[9].computed_result.as_deref(), Some("8"));
    assert_eq!(fields[10].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[10].instruction, r#"= (2 + 1) ^ 3"#);
    assert_eq!(fields[10].result, "stale parenthesized exponent");
    assert_eq!(fields[10].computed_result.as_deref(), Some("27"));
    assert_eq!(fields[11].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[11].instruction, r#"= ROUND(4 ^ 0.5, 1)"#);
    assert_eq!(fields[11].result, "stale fractional exponent");
    assert_eq!(fields[11].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[12].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[12].instruction, r#"= 1E3 + 2.5e2"#);
    assert_eq!(fields[12].result, "stale scientific sum");
    assert_eq!(fields[12].computed_result.as_deref(), Some("1250"));
    assert_eq!(fields[13].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[13].instruction, r#"= ROUND(1.25e-2, 4)"#);
    assert_eq!(fields[13].result, "stale scientific fraction");
    assert_eq!(fields[13].computed_result.as_deref(), Some("0.0125"));
    assert_eq!(fields[14].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[14].instruction, r#"= 2E+3 / 4"#);
    assert_eq!(fields[14].result, "stale signed scientific exponent");
    assert_eq!(fields[14].computed_result.as_deref(), Some("500"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        [
            "22", "6", "24", "-2", "9", "123.46", "5", "-1", "$120", "8", "27", "2", "1250",
            "0.0125", "500",
        ]
        .into_iter()
        .all(|result| main_text.contains(result)),
        "computed literal function results should be materialized in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale"),
        "computed literal function results should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_additional_literal_functions() {
    let doc = Document::open(&formula_additional_function_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= AVERAGE(2, 4, 6)"#, "stale average", "4"),
        (r#"= COUNT(2, 4, 6)"#, "stale count", "3"),
        (r#"= MOD(10, 4)"#, "stale mod", "2"),
        (r#"= TRUE"#, "stale true constant", "1"),
        (r#"= FALSE"#, "stale false constant", "0"),
        (r#"= AND(1, 2, 3)"#, "stale and true", "1"),
        (r#"= AND(1, 0, 3)"#, "stale and false", "0"),
        (r#"= OR(0, 0, 7)"#, "stale or true", "1"),
        (r#"= NOT(0)"#, "stale not", "1"),
        (r#"= IF(0, 10, 20)"#, "stale if false", "20"),
        (r#"= IF(OR(0, TRUE), SUM(1, 2), 9)"#, "stale nested if", "3"),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), Some(result));
    }

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    for (_, stale, result) in expected {
        assert!(
            main_text.contains(result),
            "computed literal function result should be materialized in main text: {main_text:?}"
        );
        assert!(
            !main_text.contains(stale),
            "computed literal function result should replace stale cached text: {main_text:?}"
        );
    }
}

#[test]
fn docx_formula_fields_compute_semicolon_literal_function_arguments() {
    let doc = Document::open(&formula_semicolon_function_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= SUM(1; 2; 3)"#);
    assert_eq!(fields[0].result, "stale semicolon sum");
    assert_eq!(fields[0].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= IF(OR(0; TRUE); SUM(1; 2); 9)"#);
    assert_eq!(fields[1].result, "stale semicolon nested if");
    assert_eq!(fields[1].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= SUM(1, 2; 3)"#);
    assert_eq!(fields[2].result, "cached mixed separators");
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("6")
            && main_text.contains("3")
            && main_text.contains("cached mixed separators"),
        "semicolon formula results should materialize while mixed separators stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale semicolon sum")
            && !main_text.contains("stale semicolon nested if"),
        "computed semicolon formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_accept_neutral_format_switches() {
    let doc = Document::open(&formula_neutral_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= 2 + 3 \* MERGEFORMAT"#);
    assert_eq!(fields[0].result, "stale neutral formula");
    assert_eq!(fields[0].computed_result.as_deref(), Some("5"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= SUM(1; 2; 3) \*CHARFORMAT"#);
    assert_eq!(fields[1].result, "stale compact neutral formula");
    assert_eq!(fields[1].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= 2 + 3 \* Upper"#);
    assert_eq!(fields[2].result, "cached unsupported formula format");
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("5")
            && main_text.contains("6")
            && main_text.contains("cached unsupported formula format"),
        "neutral formula switches should compute while non-neutral switches stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale neutral formula")
            && !main_text.contains("stale compact neutral formula"),
        "computed neutral formula switches should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_defined_literal_expressions() {
    let doc = Document::open(&formula_defined_function_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= DEFINED(SUM(1; 2; 3))"#);
    assert_eq!(fields[0].result, "stale defined expression");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= DEFINED(UnknownBookmark)"#);
    assert_eq!(fields[1].result, "stale undefined name");
    assert_eq!(fields[1].computed_result.as_deref(), Some("0"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= DEFINED(1 / 0)"#);
    assert_eq!(fields[2].result, "stale error expression");
    assert_eq!(fields[2].computed_result.as_deref(), Some("0"));
    assert_eq!(fields[3].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[3].instruction, r#"= IF(DEFINED(2 + 3), 7, 9)"#);
    assert_eq!(fields[3].result, "stale nested defined");
    assert_eq!(fields[3].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[4].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[4].instruction, r#"= DEFINED()"#);
    assert_eq!(fields[4].result, "cached empty defined");
    assert_eq!(fields[4].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("1")
            && main_text.matches('0').count() >= 2
            && main_text.contains("7")
            && main_text.contains("cached empty defined"),
        "DEFINED formula results should materialize while malformed empty calls stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale defined expression")
            && !main_text.contains("stale undefined name")
            && !main_text.contains("stale error expression")
            && !main_text.contains("stale nested defined"),
        "computed DEFINED formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_simple_table_references() {
    let doc = Document::open(&formula_table_reference_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= SUM(LEFT)"#, "stale left sum", Some("5")),
        (
            r#"= AVERAGE(LEFT) \# "0.0""#,
            "stale left average",
            Some("7.5"),
        ),
        (r#"= SUM(BELOW)"#, "stale below sum", Some("11")),
        (r#"= SUM(RIGHT)"#, "stale right sum", Some("15")),
        (r#"= SUM(ABOVE)"#, "stale above sum", Some("29")),
        (r#"= SUM(LEFT)"#, "cached nonnumeric left", None),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), result);
    }

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("5")
            && main_text.contains("7.5")
            && main_text.contains("11")
            && main_text.contains("15")
            && main_text.contains("29")
            && main_text.contains("cached nonnumeric left"),
        "table-reference formula results should materialize while unsafe ranges stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale left sum")
            && !main_text.contains("stale left average")
            && !main_text.contains("stale below sum")
            && !main_text.contains("stale right sum")
            && !main_text.contains("stale above sum"),
        "computed table-reference formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_table_formula_context_ignores_deleted_fields() {
    let doc = Document::open(&formula_table_deleted_preceding_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= SUM(LEFT)"#);
    assert_eq!(fields[0].result, "stale visible sum");
    assert_eq!(fields[0].computed_result.as_deref(), Some("5"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("5") && !main_text.contains("deleted formula"),
        "deleted formulas must not shift visible table formula results: {main_text:?}"
    );
}

#[test]
fn docx_table_formula_context_uses_single_alternate_content_branch() {
    let doc = Document::open(&formula_table_alternate_content_preceding_field_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= SUM(LEFT)"#);
    assert_eq!(fields[0].result, "stale visible sum");
    assert_eq!(fields[0].computed_result.as_deref(), Some("5"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("5") && !main_text.contains("fallback formula"),
        "AlternateContent fallback formulas must not shift visible table formula results: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_combined_table_references() {
    let doc = Document::open(&formula_table_combined_reference_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= SUM(LEFT,ABOVE)"#, "stale left above sum", Some("6")),
        (r#"= SUM(RIGHT;ABOVE)"#, "stale right above sum", Some("16")),
        (r#"= COUNT(LEFT,RIGHT)"#, "stale side count", Some("2")),
        (
            r#"= SUM(LEFT,RIGHT;ABOVE)"#,
            "cached mixed positional separators",
            None,
        ),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), result);
    }

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("6")
            && main_text.contains("16")
            && main_text.contains("2")
            && main_text.contains("cached mixed positional separators"),
        "combined table-reference formulas should materialize while mixed separators stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale left above sum")
            && !main_text.contains("stale right above sum")
            && !main_text.contains("stale side count"),
        "computed combined table-reference formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_table_cell_references() {
    let doc = Document::open(&formula_table_cell_reference_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= SUM(A1:B2)"#, "stale a1 range", Some("16")),
        (r#"= PRODUCT(R1C2:R2C3)"#, "stale rncn range", Some("504")),
        (r#"= SUM(A1,C2)"#, "stale a1 list", Some("9")),
        (r#"= SUM(R)"#, "stale current row", Some("10")),
        (r#"= SUM(C)"#, "stale current column", Some("10")),
        (
            r#"= SUM(R2C1:R2C3)"#,
            "stale explicit row range",
            Some("15"),
        ),
        (
            r#"= SUM(R1C1:R3C1)"#,
            "stale explicit column range",
            Some("12"),
        ),
        (r#"= SUM(A1,B1;C1)"#, "cached mixed cell separators", None),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), result);
    }

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("16")
            && main_text.contains("504")
            && main_text.contains("9")
            && main_text.matches("10").count() >= 2
            && main_text.contains("15")
            && main_text.contains("12")
            && main_text.contains("cached mixed cell separators"),
        "table cell-reference formulas should materialize while mixed separators stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale a1 range")
            && !main_text.contains("stale rncn range")
            && !main_text.contains("stale a1 list")
            && !main_text.contains("stale current row")
            && !main_text.contains("stale current column")
            && !main_text.contains("stale explicit row range")
            && !main_text.contains("stale explicit column range"),
        "computed table cell-reference formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_nested_table_reference_expressions() {
    let doc = Document::open(&formula_table_nested_expression_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= IF(SUM(LEFT)>=10,10,0)"#, "stale nested if", Some("10")),
        (
            r#"= AND(SUM(LEFT)<10,SUM(ABOVE)>=2)"#,
            "stale nested and",
            Some("1"),
        ),
        (
            r#"= ROUND(AVERAGE(A1:B2),1)"#,
            "stale nested round",
            Some("5"),
        ),
        (
            r#"= IF(SUM(LEFT,RIGHT;ABOVE)>0,1,0)"#,
            "cached mixed nested table expression",
            None,
        ),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), result);
    }

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("10")
            && main_text.contains("1")
            && main_text.contains("5")
            && main_text.contains("cached mixed nested table expression"),
        "nested table-reference formula results should materialize while mixed separators stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale nested if")
            && !main_text.contains("stale nested and")
            && !main_text.contains("stale nested round"),
        "computed nested table-reference formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_ragged_table_existing_cell_references() {
    let doc = Document::open(&formula_table_ragged_reference_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= SUM(A1:C2)"#, "stale ragged range", Some("15")),
        (r#"= SUM(C)"#, "stale ragged column", Some("4")),
        (r#"= SUM(ABOVE)"#, "stale ragged above", Some("6")),
        (r#"= SUM(ABOVE)"#, "cached absent above", None),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), result);
    }

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("15")
            && main_text.contains("4")
            && main_text.contains("6")
            && main_text.contains("cached absent above"),
        "ragged table-reference formula results should materialize while empty references stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale ragged range")
            && !main_text.contains("stale ragged column")
            && !main_text.contains("stale ragged above"),
        "computed ragged table-reference formulas should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_literal_comparison_expressions() {
    let doc = Document::open(&formula_comparison_docx()).expect("fixture opens");
    let fields = doc.fields();

    let expected = [
        (r#"= IF(2 > 1, 10, 20)"#, "stale greater if", "10"),
        (r#"= IF(2 < 1, 10, 20)"#, "stale less if", "20"),
        (r#"= 2 = 2"#, "stale equal", "1"),
        (r#"= 3 <> 3"#, "stale not equal false", "0"),
        (
            r#"= AND(2 >= 2, 3 <= 4, 5 <> 6)"#,
            "stale logical comparisons",
            "1",
        ),
        (r#"= OR(1 > 2, 3 < 4)"#, "stale or comparison", "1"),
        (r#"= NOT(7 = 8)"#, "stale not comparison", "1"),
    ];

    assert_eq!(fields.len(), expected.len());
    for (field, (instruction, stale, result)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::Dynamic("=".to_string()));
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.result, stale);
        assert_eq!(field.computed_result.as_deref(), Some(result));
    }

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    for (_, stale, result) in expected {
        assert!(
            main_text.contains(result),
            "computed literal comparison result should be materialized in main text: {main_text:?}"
        );
        assert!(
            !main_text.contains(stale),
            "computed literal comparison result should replace stale cached text: {main_text:?}"
        );
    }
}

#[test]
fn docx_formula_fields_compute_numeric_picture_affixes_and_x_placeholders() {
    let doc = Document::open(&formula_numeric_picture_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= 1234.5 \# "$#,##0.00""#);
    assert_eq!(fields[0].result, "stale currency formula");
    assert_eq!(fields[0].computed_result.as_deref(), Some("$1,234.50"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r###"= 33 \# "##%""###);
    assert_eq!(fields[1].result, "stale percent formula");
    assert_eq!(fields[1].computed_result.as_deref(), Some("33%"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= 111053 + 111439 \# x##"#);
    assert_eq!(fields[2].result, "stale dropped formula");
    assert_eq!(fields[2].computed_result.as_deref(), Some("492"));
    assert_eq!(fields[3].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[3].instruction, r#"= 1 / 8 \# 0.00x"#);
    assert_eq!(fields[3].result, "stale precision formula");
    assert_eq!(fields[3].computed_result.as_deref(), Some("0.125"));
    assert_eq!(fields[4].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[4].instruction, r#"= 3 / 4 \# .x"#);
    assert_eq!(fields[4].result, "stale rounded formula");
    assert_eq!(fields[4].computed_result.as_deref(), Some(".8"));
    assert_eq!(fields[5].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[5].instruction, r#"= 5 \# "0 units""#);
    assert_eq!(fields[5].result, "stale spaced suffix formula");
    assert_eq!(fields[5].computed_result.as_deref(), Some("5 units"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("$1,234.50")
            && main_text.contains("33%")
            && main_text.contains("492")
            && main_text.contains("0.125")
            && main_text.contains(".8")
            && main_text.contains("5 units"),
        "computed formula numeric pictures should materialize literal affixes: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale currency formula")
            && !main_text.contains("stale percent formula")
            && !main_text.contains("stale dropped formula")
            && !main_text.contains("stale precision formula")
            && !main_text.contains("stale rounded formula")
            && !main_text.contains("stale spaced suffix formula"),
        "computed formula numeric pictures should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_formula_fields_compute_compact_numeric_pictures() {
    let doc = Document::open(&formula_compact_numeric_picture_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[0].instruction, r#"= 10 / 4 \#"0.0""#);
    assert_eq!(fields[0].result, "stale compact quoted picture");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2.5"));
    assert_eq!(fields[1].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[1].instruction, r#"= SUM(100, 20) \#$0"#);
    assert_eq!(fields[1].result, "stale compact unquoted picture");
    assert_eq!(fields[1].computed_result.as_deref(), Some("$120"));
    assert_eq!(fields[2].kind, FieldKind::Dynamic("=".to_string()));
    assert_eq!(fields[2].instruction, r#"= 10 \#"#);
    assert_eq!(fields[2].result, "cached missing compact picture");
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("=".to_string()),
            count: 1
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2.5")
            && main_text.contains("$120")
            && main_text.contains("cached missing compact picture"),
        "compact numeric pictures should compute while malformed switches stay cached: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale compact quoted picture")
            && !main_text.contains("stale compact unquoted picture"),
        "computed compact numeric pictures should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_quote_field_computes_literal_text_and_general_text_formats() {
    let doc = Document::open(&quote_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert!(fields
        .iter()
        .all(|field| field.kind == FieldKind::Dynamic("QUOTE".to_string())));
    assert_eq!(fields[0].instruction, "QUOTE \"literal text\"");
    assert_eq!(fields[0].result, "stale literal");
    assert_eq!(fields[0].computed_result.as_deref(), Some("literal text"));
    assert_eq!(fields[1].instruction, "QUOTE \"mixed words\" \\* Caps");
    assert_eq!(fields[1].result, "stale caps");
    assert_eq!(fields[1].computed_result.as_deref(), Some("Mixed Words"));
    assert_eq!(fields[2].instruction, "QUOTE \"word\" \\* Upper");
    assert_eq!(fields[2].result, "stale upper");
    assert_eq!(fields[2].computed_result.as_deref(), Some("WORD"));
    assert_eq!(fields[3].instruction, "QUOTE PlainToken");
    assert_eq!(fields[3].result, "stale unquoted token");
    assert_eq!(fields[3].computed_result.as_deref(), Some("PlainToken"));
    assert_eq!(fields[4].instruction, "QUOTE plain words \\* Upper");
    assert_eq!(fields[4].result, "stale unquoted phrase");
    assert_eq!(fields[4].computed_result.as_deref(), Some("PLAIN WORDS"));
    assert_eq!(fields[5].instruction, "QUOTE \"broken literal ");
    assert_eq!(fields[5].result, "cached broken quote");
    assert_eq!(fields[5].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Dynamic("QUOTE".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("literal text\nMixed Words\nWORD\nPlainToken\nPLAIN WORDS"),
        "computed QUOTE fields should be materialized in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale literal")
            && !main_text.contains("stale caps")
            && !main_text.contains("stale upper")
            && !main_text.contains("stale unquoted token")
            && !main_text.contains("stale unquoted phrase"),
        "computed QUOTE fields should not display stale cached text: {main_text:?}"
    );
    assert!(main_text.contains("cached broken quote"), "{main_text:?}");
}

#[test]
fn docx_inserted_content_fields_are_named_noncomputed_fields() {
    let doc = Document::open(&inserted_content_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 11);
    assert_eq!(
        fields[0].kind,
        FieldKind::InsertedContent("INCLUDETEXT".to_string())
    );
    assert_eq!(fields[0].instruction, "INCLUDETEXT \"appendix.docx\"");
    assert_eq!(fields[0].result, "Appendix text");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::InsertedContent("INCLUDEPICTURE".to_string())
    );
    assert_eq!(fields[1].result, "Chart preview");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::InsertedContent("LINK".to_string())
    );
    assert_eq!(fields[2].result, "42");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].kind,
        FieldKind::InsertedContent("EMBED".to_string())
    );
    assert_eq!(fields[3].result, "Embedded object");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(
        fields[4].kind,
        FieldKind::InsertedContent("DATABASE".to_string())
    );
    assert_eq!(fields[4].result, "Rows");
    assert_eq!(fields[4].computed_result, None);
    assert_eq!(
        fields[5].kind,
        FieldKind::InsertedContent("DDE".to_string())
    );
    assert_eq!(fields[5].result, "DDE value");
    assert_eq!(fields[5].computed_result, None);
    assert_eq!(
        fields[6].kind,
        FieldKind::InsertedContent("DDEAUTO".to_string())
    );
    assert_eq!(fields[6].result, "Auto DDE value");
    assert_eq!(fields[6].computed_result, None);
    assert_eq!(
        fields[7].kind,
        FieldKind::InsertedContent("IMPORT".to_string())
    );
    assert_eq!(fields[7].result, "Imported object");
    assert_eq!(fields[7].computed_result, None);
    assert_eq!(
        fields[8].kind,
        FieldKind::InsertedContent("INCLUDE".to_string())
    );
    assert_eq!(fields[8].result, "Included text");
    assert_eq!(fields[8].computed_result, None);
    assert_eq!(
        fields[9].kind,
        FieldKind::InsertedContent("AUTOTEXT".to_string())
    );
    assert_eq!(fields[9].instruction, "AUTOTEXT Signature");
    assert_eq!(fields[9].result, "AutoText signature");
    assert_eq!(fields[9].computed_result, None);
    assert_eq!(
        fields[10].kind,
        FieldKind::InsertedContent("AUTOTEXTLIST".to_string())
    );
    assert_eq!(
        fields[10].instruction,
        "AUTOTEXTLIST \"Choose clause\" \\s Legal"
    );
    assert_eq!(fields[10].result, "AutoText list");
    assert_eq!(fields[10].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::InsertedContent("INCLUDETEXT".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("INCLUDEPICTURE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("LINK".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("EMBED".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("DATABASE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("DDE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("DDEAUTO".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("IMPORT".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("INCLUDE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("AUTOTEXT".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("AUTOTEXTLIST".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 11,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Appendix text")
            && main_text.contains("Chart preview")
            && main_text.contains("42")
            && main_text.contains("Embedded object")
            && main_text.contains("Rows")
            && main_text.contains("DDE value")
            && main_text.contains("Auto DDE value")
            && main_text.contains("Imported object")
            && main_text.contains("Included text")
            && main_text.contains("AutoText signature")
            && main_text.contains("AutoText list"),
        "cached inserted-content field results should remain in main text: {main_text:?}"
    );
}

#[test]
fn docx_inserted_content_diagnostics_split_valid_broader_fields_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" INCLUDETEXT &quot;appendix.docx&quot; "><w:r><w:t>Appendix text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDEPICTURE &quot;chart.png "><w:r><w:t>cached malformed include picture</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LINK \* "><w:r><w:t>cached dangling format switch</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDETEXT &quot;chapter.docx&quot; \* BadFormat "><w:r><w:t>cached bad include format</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 4);
    assert_eq!(
        fields[0].kind,
        FieldKind::InsertedContent("INCLUDETEXT".to_string())
    );
    assert_eq!(fields[0].instruction, r#"INCLUDETEXT "appendix.docx""#);
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::InsertedContent("INCLUDEPICTURE".to_string())
    );
    assert_eq!(fields[1].instruction, r#"INCLUDEPICTURE "chart.png "#);
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::InsertedContent("LINK".to_string())
    );
    assert_eq!(fields[2].instruction, r#"LINK \*"#);
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].kind,
        FieldKind::InsertedContent("INCLUDETEXT".to_string())
    );
    assert_eq!(
        fields[3].instruction,
        r#"INCLUDETEXT "chapter.docx" \* BadFormat"#
    );
    assert_eq!(fields[3].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::InsertedContent("INCLUDETEXT".to_string()),
                count: 2,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("INCLUDEPICTURE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::InsertedContent("LINK".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 3,
            },
        ]
    );
    assert!(doc.main_text().contains("cached malformed include picture"));
    assert!(doc.main_text().contains("cached dangling format switch"));
    assert!(doc.main_text().contains("cached bad include format"));
}

#[test]
fn docx_mail_merge_helper_fields_are_named_noncomputed_fields() {
    let doc = Document::open(&mail_merge_helper_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(
        fields[0].kind,
        FieldKind::MailMerge("ADDRESSBLOCK".to_string())
    );
    assert_eq!(fields[0].instruction, "ADDRESSBLOCK");
    assert_eq!(fields[0].result, "Acme Corp");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::MailMerge("GREETINGLINE".to_string())
    );
    assert_eq!(fields[1].result, "Dear Hyunjo,");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::MailMerge("MERGEREC".to_string()));
    assert_eq!(fields[2].result, "7");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(fields[3].kind, FieldKind::MailMerge("MERGESEQ".to_string()));
    assert_eq!(fields[3].result, "3");
    assert_eq!(fields[3].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::MailMerge("ADDRESSBLOCK".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::MailMerge("GREETINGLINE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::MailMerge("MERGEREC".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::MailMerge("MERGESEQ".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 4,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Acme Corp")
            && main_text.contains("Dear Hyunjo,")
            && main_text.contains("7")
            && main_text.contains("3"),
        "cached mail-merge helper field results should remain in main text: {main_text:?}"
    );
}

#[test]
fn docx_mail_merge_helper_diagnostics_split_valid_broader_fields_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" ADDRESSBLOCK "><w:r><w:t>Acme Corp</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GREETINGLINE &quot;Dear "><w:r><w:t>cached malformed greeting</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::MailMerge("ADDRESSBLOCK".to_string())
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::MailMerge("GREETINGLINE".to_string())
    );
    assert_eq!(fields[1].instruction, r#"GREETINGLINE "Dear "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::MailMerge("ADDRESSBLOCK".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::MailMerge("GREETINGLINE".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
    assert!(doc.main_text().contains("cached malformed greeting"));
}

#[test]
fn docx_reference_index_fields_are_named_noncomputed_fields() {
    let doc = Document::open(&reference_index_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 12);
    assert_eq!(
        fields[0].kind,
        FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string())
    );
    assert_eq!(fields[0].instruction, "BIBLIOGRAPHY \\l 1033");
    assert_eq!(fields[0].result, "Works cited");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::ReferenceIndex("CITATION".to_string())
    );
    assert_eq!(fields[1].result, "(Smith, 2026)");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::ReferenceIndex("INDEX".to_string())
    );
    assert_eq!(fields[2].result, "Index preview");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(fields[3].kind, FieldKind::ReferenceIndex("TOA".to_string()));
    assert_eq!(fields[3].result, "Authorities");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].kind, FieldKind::ReferenceIndex("TA".to_string()));
    assert_eq!(fields[4].result, "Case v. Example");
    assert_eq!(fields[4].computed_result.as_deref(), Some(""));
    assert_eq!(fields[5].kind, FieldKind::ReferenceIndex("XE".to_string()));
    assert_eq!(fields[5].result, "Term");
    assert_eq!(fields[5].computed_result.as_deref(), Some(""));
    assert_eq!(fields[6].kind, FieldKind::ReferenceIndex("RD".to_string()));
    assert_eq!(fields[6].result, "Referenced doc");
    assert_eq!(fields[6].computed_result.as_deref(), Some(""));
    assert_eq!(fields[7].kind, FieldKind::ReferenceIndex("TA".to_string()));
    assert_eq!(fields[7].instruction, "TA \\l\"Compact Case\" \\c2");
    assert_eq!(fields[7].result, "Compact Case");
    assert_eq!(fields[7].computed_result.as_deref(), Some(""));
    assert_eq!(fields[8].kind, FieldKind::ReferenceIndex("TA".to_string()));
    assert_eq!(fields[8].instruction, "TA \\sShortEntry \\c3");
    assert_eq!(fields[8].result, "Short Entry");
    assert_eq!(fields[8].computed_result.as_deref(), Some(""));
    assert_eq!(fields[9].kind, FieldKind::ReferenceIndex("XE".to_string()));
    assert_eq!(fields[9].instruction, "XE \"See Term\" \\t\"See Also\"");
    assert_eq!(fields[9].result, "See Term");
    assert_eq!(fields[9].computed_result.as_deref(), Some(""));
    assert_eq!(fields[10].kind, FieldKind::ReferenceIndex("XE".to_string()));
    assert_eq!(
        fields[10].instruction,
        "XE \"Duplicate Format\" \\* Upper \\* Lower"
    );
    assert_eq!(fields[10].result, "Duplicate Format");
    assert_eq!(fields[10].computed_result, None);
    assert_eq!(fields[11].kind, FieldKind::ReferenceIndex("TA".to_string()));
    assert_eq!(fields[11].instruction, "TA \\l \"Broken Case\" \\c 99");
    assert_eq!(fields[11].result, "Broken Case");
    assert_eq!(fields[11].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("CITATION".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("INDEX".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("TOA".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("XE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("TA".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 4,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Works cited")
            && main_text.contains("(Smith, 2026)")
            && main_text.contains("Index preview")
            && main_text.contains("Authorities")
            && main_text.contains("Duplicate Format"),
        "cached unresolved reference/index field results should remain in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("Case v. Example")
            && !main_text.contains("Term")
            && !main_text.contains("Referenced doc")
            && !main_text.contains("Compact Case")
            && !main_text.contains("Short Entry")
            && !main_text.contains("See Term"),
        "computed RD/TA/XE marker fields should be hidden in main text: {main_text:?}"
    );
}

#[test]
fn docx_reference_index_diagnostics_split_valid_generated_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" BIBLIOGRAPHY \l 1033 "><w:r><w:t>Works cited</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INDEX &quot;bad "><w:r><w:t>cached malformed index</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string())
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::ReferenceIndex("INDEX".to_string())
    );
    assert_eq!(fields[1].instruction, r#"INDEX "bad "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::ReferenceIndex("INDEX".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
    assert!(doc.main_text().contains("cached malformed index"));
}

#[test]
fn docx_numbering_fields_compute_formatted_autonum_subset() {
    let doc = Document::open(&numbering_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 14);
    assert_eq!(fields[0].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[0].instruction, "AUTONUM");
    assert_eq!(fields[0].result, "stale autonum one");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[1].instruction, "AUTONUM \\* MERGEFORMAT");
    assert_eq!(fields[1].result, "stale autonum two");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[2].instruction, "AUTONUM \\* roman");
    assert_eq!(fields[2].result, "stale autonum roman");
    assert_eq!(fields[2].computed_result.as_deref(), Some("iii"));
    assert_eq!(fields[3].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[3].instruction, "AUTONUM \\* Unknown");
    assert_eq!(fields[3].result, "cached unsupported autonum");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[4].instruction, "AUTONUM");
    assert_eq!(fields[4].result, "stale autonum after unsupported");
    assert_eq!(fields[4].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[5].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[5].instruction, "AUTONUM \\s.");
    assert_eq!(fields[5].result, "stale autonum separator");
    assert_eq!(fields[5].computed_result.as_deref(), Some("5."));
    assert_eq!(fields[6].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[6].instruction, "AUTONUM \\s \")\"");
    assert_eq!(fields[6].result, "stale quoted autonum separator");
    assert_eq!(fields[6].computed_result.as_deref(), Some("6)"));
    assert_eq!(
        fields[7].kind,
        FieldKind::Numbering("AUTONUMLGL".to_string())
    );
    assert_eq!(fields[7].result, "cached legal number");
    assert_eq!(fields[7].computed_result.as_deref(), Some("7"));
    assert_eq!(
        fields[8].kind,
        FieldKind::Numbering("AUTONUMLGL".to_string())
    );
    assert_eq!(fields[8].instruction, "AUTONUMLGL \\* roman");
    assert_eq!(fields[8].result, "cached legal roman");
    assert_eq!(fields[8].computed_result.as_deref(), Some("viii"));
    assert_eq!(
        fields[9].kind,
        FieldKind::Numbering("AUTONUMOUT".to_string())
    );
    assert_eq!(fields[9].result, "cached outline number");
    assert_eq!(fields[9].computed_result.as_deref(), Some("9"));
    assert_eq!(
        fields[10].kind,
        FieldKind::Numbering("AUTONUMOUT".to_string())
    );
    assert_eq!(fields[10].instruction, "AUTONUMOUT \\* roman");
    assert_eq!(fields[10].result, "cached outline roman");
    assert_eq!(fields[10].computed_result.as_deref(), Some("x"));
    assert_eq!(fields[11].kind, FieldKind::Numbering("LISTNUM".to_string()));
    assert_eq!(fields[11].instruction, "LISTNUM LegalDefault \\l 2");
    assert_eq!(fields[11].result, "cached list number");
    assert_eq!(fields[11].computed_result, None);
    assert_eq!(
        fields[12].kind,
        FieldKind::Numbering("BIDIOUTLINE".to_string())
    );
    assert_eq!(fields[12].instruction, "BIDIOUTLINE");
    assert_eq!(fields[12].result, "cached bidi outline");
    assert_eq!(fields[12].computed_result, None);
    assert_eq!(
        fields[13].kind,
        FieldKind::Numbering("BIDIOUTLINE".to_string())
    );
    assert_eq!(fields[13].instruction, "BIDIOUTLINE \\x");
    assert_eq!(fields[13].result, "cached malformed bidi outline");
    assert_eq!(fields[13].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Numbering("AUTONUM".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Numbering("LISTNUM".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Numbering("BIDIOUTLINE".to_string()),
                count: 2,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 2,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("1")
            && main_text.contains("2")
            && main_text.contains("iii")
            && main_text.contains("cached unsupported autonum")
            && main_text.contains("4")
            && main_text.contains("5.")
            && main_text.contains("6)")
            && main_text.contains("7")
            && main_text.contains("viii")
            && main_text.contains("9")
            && main_text.contains("x")
            && main_text.contains("cached list number")
            && main_text.contains("cached bidi outline")
            && main_text.contains("cached malformed bidi outline"),
        "computed AUTONUM and cached remaining numbering field results should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale autonum one")
            && !main_text.contains("stale autonum two")
            && !main_text.contains("stale autonum roman")
            && !main_text.contains("stale autonum after unsupported")
            && !main_text.contains("stale autonum separator")
            && !main_text.contains("stale quoted autonum separator")
            && !main_text.contains("cached legal number")
            && !main_text.contains("cached legal roman")
            && !main_text.contains("cached outline number")
            && !main_text.contains("cached outline roman"),
        "computed automatic numbering fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_listnum_number_default_computes_level_one_subset() {
    let doc = Document::open(&listnum_number_default_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 8);
    assert!(fields
        .iter()
        .all(|field| field.kind == FieldKind::Numbering("LISTNUM".to_string())));
    assert_eq!(fields[0].instruction, "LISTNUM NumberDefault");
    assert_eq!(fields[0].result, "stale listnum one");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(
        fields[1].instruction,
        "LISTNUM NumberDefault \\* MERGEFORMAT"
    );
    assert_eq!(fields[1].result, "stale listnum mergeformat");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].instruction, "LISTNUM NumberDefault \\*CHARFORMAT");
    assert_eq!(fields[2].result, "stale listnum charformat");
    assert_eq!(fields[2].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[3].instruction, "LISTNUM NumberDefault \\s 4");
    assert_eq!(fields[3].result, "stale listnum reset");
    assert_eq!(fields[3].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[4].instruction, "LISTNUM NumberDefault");
    assert_eq!(fields[4].result, "stale listnum after reset");
    assert_eq!(fields[4].computed_result.as_deref(), Some("5"));
    assert_eq!(fields[5].instruction, "LISTNUM NumberDefault \\* roman");
    assert_eq!(fields[5].result, "stale listnum roman");
    assert_eq!(fields[5].computed_result.as_deref(), Some("vi"));
    assert_eq!(fields[6].instruction, "LISTNUM NumberDefault \\l 2");
    assert_eq!(fields[6].result, "cached nested listnum");
    assert_eq!(fields[6].computed_result, None);
    assert_eq!(fields[7].instruction, "LISTNUM LegalDefault");
    assert_eq!(fields[7].result, "cached legal listnum");
    assert_eq!(fields[7].computed_result.as_deref(), Some("7"));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Numbering("LISTNUM".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("1")
            && main_text.contains("2")
            && main_text.contains("3")
            && main_text.contains("4")
            && main_text.contains("5")
            && main_text.contains("vi")
            && main_text.contains("cached nested listnum")
            && main_text.contains("7"),
        "computed listnum values and unsupported cached results should be visible: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale listnum one")
            && !main_text.contains("stale listnum mergeformat")
            && !main_text.contains("stale listnum charformat")
            && !main_text.contains("stale listnum reset")
            && !main_text.contains("stale listnum after reset")
            && !main_text.contains("stale listnum roman")
            && !main_text.contains("cached legal listnum"),
        "computed listnum results should replace stale cached field text: {main_text:?}"
    );
}

#[test]
fn docx_sequence_and_numbering_fields_apply_text_format_switches() {
    let doc = Document::open(&sequence_numbering_text_format_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure \\* CardText \\* Upper");
    assert_eq!(fields[0].computed_result.as_deref(), Some("ONE"));
    assert_eq!(fields[1].kind, FieldKind::Sequence);
    assert_eq!(fields[1].instruction, "SEQ Figure \\* roman \\* Upper");
    assert_eq!(fields[1].computed_result.as_deref(), Some("II"));
    assert_eq!(fields[2].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[2].instruction, "AUTONUM \\* CardText \\* Upper");
    assert_eq!(fields[2].computed_result.as_deref(), Some("ONE"));
    assert_eq!(fields[3].kind, FieldKind::Numbering("AUTONUM".to_string()));
    assert_eq!(fields[3].instruction, "AUTONUM \\* roman \\* Upper");
    assert_eq!(fields[3].computed_result.as_deref(), Some("II"));
    assert_eq!(fields[4].kind, FieldKind::Numbering("LISTNUM".to_string()));
    assert_eq!(
        fields[4].instruction,
        "LISTNUM NumberDefault \\* CardText \\* Upper"
    );
    assert_eq!(fields[4].computed_result.as_deref(), Some("ONE"));
    assert_eq!(fields[5].kind, FieldKind::Numbering("LISTNUM".to_string()));
    assert_eq!(
        fields[5].instruction,
        "LISTNUM NumberDefault \\* roman \\* Upper"
    );
    assert_eq!(fields[5].computed_result.as_deref(), Some("II"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale sequence card")
            && !main_text.contains("stale sequence roman")
            && !main_text.contains("stale autonum card")
            && !main_text.contains("stale autonum roman")
            && !main_text.contains("stale listnum card")
            && !main_text.contains("stale listnum roman"),
        "formatted sequence and numbering fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_document_structure_fields_are_named_and_section_is_computed() {
    let doc = Document::open(&document_structure_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentStructure("REVNUM".to_string())
    );
    assert_eq!(fields[0].instruction, "REVNUM");
    assert_eq!(fields[0].result, "4");
    assert_eq!(fields[0].computed_result.as_deref(), Some("12"));
    assert_eq!(
        fields[1].kind,
        FieldKind::DocumentStructure("SECTION".to_string())
    );
    assert_eq!(fields[1].result, "2");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(
        fields[2].kind,
        FieldKind::DocumentStructure("SECTIONPAGES".to_string())
    );
    assert_eq!(fields[2].result, "5");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].kind,
        FieldKind::DocumentStructure("STYLEREF".to_string())
    );
    assert_eq!(fields[3].instruction, "STYLEREF \"Heading 1\" \\n");
    assert_eq!(fields[3].result, "Executive Summary");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(
        fields[4].kind,
        FieldKind::DocumentStructure("STYLEREF".to_string())
    );
    assert_eq!(fields[4].instruction, "STYLEREF \"Heading 1 ");
    assert_eq!(fields[4].result, "cached broken style ref");
    assert_eq!(fields[4].computed_result, None);
    assert_eq!(
        fields[5].kind,
        FieldKind::DocumentStructure("STYLEREF".to_string())
    );
    assert_eq!(fields[5].instruction, "STYLEREF \\p \"Heading 1\"");
    assert_eq!(fields[5].result, "cached switch-first style ref");
    assert_eq!(fields[5].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::DocumentStructure("SECTIONPAGES".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::DocumentStructure("STYLEREF".to_string()),
                count: 3,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 2,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("12")
            && main_text.contains("1")
            && main_text.contains("5")
            && main_text.contains("Executive Summary")
            && main_text.contains("cached broken style ref")
            && main_text.contains("cached switch-first style ref"),
        "computed REVNUM/SECTION and cached remaining document-structure fields should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("4"),
        "computed REVNUM should replace stale cached revision text: {main_text:?}"
    );
}

#[test]
fn docx_section_field_computes_current_structural_section_number() {
    let doc = Document::open(&section_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    for field in &fields {
        assert_eq!(
            field.kind,
            FieldKind::DocumentStructure("SECTION".to_string())
        );
    }
    assert_eq!(fields[0].instruction, "SECTION");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].instruction, "SECTION");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].instruction, "SECTION \\* MERGEFORMAT");
    assert_eq!(fields[2].computed_result.as_deref(), Some("3"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale first section")
            && !main_text.contains("stale second section")
            && !main_text.contains("stale third section"),
        "computed SECTION fields should replace stale cached text: {main_text:?}"
    );
    assert!(main_text.contains("1"), "{main_text:?}");
    assert!(main_text.contains("2"), "{main_text:?}");
    assert!(main_text.contains("3"), "{main_text:?}");
}

#[test]
fn docx_section_field_invalid_switch_does_not_shift_later_fields() {
    let doc =
        Document::open(&section_field_invalid_switch_alignment_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentStructure("SECTION".to_string())
    );
    assert_eq!(fields[0].instruction, "SECTION \\* Unknown");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::DocumentStructure("SECTION".to_string())
    );
    assert_eq!(fields[1].instruction, "SECTION");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::DocumentStructure("SECTION".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached invalid section"),
        "{main_text:?}"
    );
    assert!(!main_text.contains("stale valid section"), "{main_text:?}");
    assert!(main_text.contains("1"), "{main_text:?}");
}

#[test]
fn docx_section_context_uses_single_alternate_content_branch() {
    let doc = Document::open(&section_alternate_content_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentStructure("SECTION".to_string())
    );
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
}

#[test]
fn docx_section_pages_field_computes_structural_section_page_count() {
    let doc = Document::open(&section_pages_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    for field in &fields {
        assert_eq!(
            field.kind,
            FieldKind::DocumentStructure("SECTIONPAGES".to_string())
        );
    }
    assert_eq!(fields[0].instruction, "SECTIONPAGES");
    assert_eq!(fields[0].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[1].instruction, "SECTIONPAGES \\* ROMAN");
    assert_eq!(fields[1].computed_result.as_deref(), Some("III"));
    assert_eq!(fields[2].instruction, "SECTIONPAGES \\* CardText \\* Upper");
    assert_eq!(fields[2].computed_result.as_deref(), Some("THREE"));
    assert_eq!(fields[3].instruction, "SECTIONPAGES \\* Ordinal");
    assert_eq!(fields[3].computed_result.as_deref(), Some("1st"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(main_text.contains("3"), "{main_text:?}");
    assert!(main_text.contains("III"), "{main_text:?}");
    assert!(main_text.contains("THREE"), "{main_text:?}");
    assert!(main_text.contains("1st"), "{main_text:?}");
}

#[test]
fn docx_style_ref_field_computes_nearest_paragraph_style_text() {
    let doc = Document::open(&style_ref_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    for field in &fields {
        assert_eq!(
            field.kind,
            FieldKind::DocumentStructure("STYLEREF".to_string())
        );
    }
    assert_eq!(fields[0].instruction, "STYLEREF \"heading 1\"");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary")
    );
    assert_eq!(fields[1].instruction, "STYLEREF \"heading 1\" \\p");
    assert_eq!(fields[1].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[2].instruction, "STYLEREF CustomCallout");
    assert_eq!(
        fields[2].computed_result.as_deref(),
        Some("Forward Finding")
    );
    assert_eq!(
        fields[3].instruction,
        "STYLEREF CustomCallout \\p \\* Upper"
    );
    assert_eq!(fields[3].computed_result.as_deref(), Some("BELOW"));
    assert_eq!(
        fields[4].instruction,
        "STYLEREF \"Custom Heading\" \\* MERGEFORMAT"
    );
    assert_eq!(
        fields[4].computed_result.as_deref(),
        Some("Forward Finding")
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale heading style")
            && !main_text.contains("stale heading relative")
            && !main_text.contains("stale forward style")
            && !main_text.contains("stale forward relative")
            && !main_text.contains("stale custom style"),
        "computed STYLEREF fields should replace stale cached text: {main_text:?}"
    );

    let model = doc.model();
    let paragraph_style_ids = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => paragraph.props.style_id.as_deref(),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraph_style_ids, vec!["Heading1", "CustomCallout"]);

    let paragraph_text = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph.text()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        paragraph_text,
        vec![
            "Executive Summary".to_string(),
            "Executive Summary".to_string(),
            "above".to_string(),
            "Forward Finding".to_string(),
            "BELOW".to_string(),
            "Forward Finding".to_string(),
            "Forward Finding".to_string(),
        ]
    );
}

#[test]
fn docx_style_ref_context_ignores_deleted_styled_paragraphs() {
    let doc = Document::open(&style_ref_deleted_heading_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentStructure("STYLEREF".to_string())
    );
    assert_eq!(fields[0].instruction, "STYLEREF \"heading 1\"");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Visible Heading")
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Visible Heading")
            && !main_text.contains("Deleted Heading")
            && !main_text.contains("Moved Heading"),
        "STYLEREF should follow accepted-current style context: {main_text:?}"
    );
}

#[test]
fn docx_style_ref_context_uses_single_alternate_content_branch() {
    let doc = Document::open(&style_ref_alternate_content_heading_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::DocumentStructure("STYLEREF".to_string())
    );
    assert_eq!(fields[0].instruction, "STYLEREF \"heading 1\"");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Choice Heading"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Choice Heading")
            && !main_text.contains("Fallback Heading")
            && !main_text.contains("Fallback Inline"),
        "STYLEREF should use one AlternateContent branch: {main_text:?}"
    );
}

#[test]
fn docx_style_ref_number_switches_compute_numbered_paragraph_labels() {
    let doc = Document::open(&numbered_style_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    for field in &fields {
        assert_eq!(
            field.kind,
            FieldKind::DocumentStructure("STYLEREF".to_string())
        );
    }
    assert_eq!(fields[0].instruction, "STYLEREF NumberedTarget \\r");
    assert_eq!(fields[0].computed_result.as_deref(), Some("5.1"));
    assert_eq!(fields[1].instruction, "STYLEREF NumberedTarget \\r \\t");
    assert_eq!(fields[1].computed_result.as_deref(), Some("5.1"));
    assert_eq!(fields[2].instruction, "STYLEREF NumberedTarget \\n");
    assert_eq!(fields[2].computed_result.as_deref(), Some("Part 1"));
    assert_eq!(fields[3].instruction, "STYLEREF NumberedTarget \\n \\t");
    assert_eq!(fields[3].computed_result.as_deref(), Some("1"));
    assert_eq!(
        fields[4].instruction,
        "STYLEREF \"Numbered Target\" \\w \\* MERGEFORMAT"
    );
    assert_eq!(fields[4].computed_result.as_deref(), Some("4.5.1"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale relative style number")
            && !main_text.contains("stale relative numeric style number")
            && !main_text.contains("stale style number")
            && !main_text.contains("stale numeric style number")
            && !main_text.contains("stale full style number"),
        "computed STYLEREF numbering fields should replace stale cached text: {main_text:?}"
    );
    assert!(main_text.contains("5.1"), "{main_text:?}");
    assert!(main_text.contains("Part 1"), "{main_text:?}");
    assert!(main_text.contains("4.5.1"), "{main_text:?}");
}

#[test]
fn docx_style_ref_field_computes_character_style_text_by_source_order() {
    let doc = Document::open(&character_style_ref_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    for field in &fields {
        assert_eq!(
            field.kind,
            FieldKind::DocumentStructure("STYLEREF".to_string())
        );
    }
    assert_eq!(fields[0].instruction, "STYLEREF \"Last Name\"");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Ackerman"));
    assert_eq!(fields[1].instruction, "STYLEREF LastName");
    assert_eq!(fields[1].computed_result.as_deref(), Some("Ackerman"));
    assert_eq!(fields[2].instruction, "STYLEREF \"Last Name\" \\* Upper");
    assert_eq!(fields[2].computed_result.as_deref(), Some("BERG"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale forward last")
            && !main_text.contains("stale same paragraph first")
            && !main_text.contains("stale same paragraph second"),
        "computed character-style STYLEREF fields should replace stale cached text: {main_text:?}"
    );
    assert!(main_text.contains("Ackerman"), "{main_text:?}");
    assert!(main_text.contains("BERG"), "{main_text:?}");
}

#[test]
fn docx_display_fields_compute_deterministic_subset() {
    let doc = Document::open(&display_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 28);
    assert_eq!(fields[0].kind, FieldKind::Display("ADVANCE".to_string()));
    assert_eq!(fields[0].instruction, "ADVANCE \\r 2 \\d4 \\* MERGEFORMAT");
    assert_eq!(fields[0].result, "offset text");
    assert_eq!(fields[0].computed_result.as_deref(), Some(""));
    assert_eq!(fields[1].kind, FieldKind::Display("ADVANCE".to_string()));
    assert_eq!(fields[1].instruction, "ADVANCE \\z 2");
    assert_eq!(fields[1].result, "cached unsupported advance");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[2].result, "stale equation");
    assert_eq!(fields[2].computed_result.as_deref(), Some("1/2"));
    assert_eq!(fields[3].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(
        fields[3].instruction,
        r#"EQ \f( "Alpha, One" , "Beta Two" ) \* Upper"#
    );
    assert_eq!(fields[3].result, "stale quoted equation");
    assert_eq!(
        fields[3].computed_result.as_deref(),
        Some("ALPHA, ONE/BETA TWO")
    );
    assert_eq!(fields[4].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[4].result, "stale semicolon equation");
    assert_eq!(fields[4].computed_result.as_deref(), Some("3/4"));
    assert_eq!(fields[5].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[5].result, "stale escaped equation");
    assert_eq!(fields[5].computed_result.as_deref(), Some("A,B/C\\D"));
    assert_eq!(fields[6].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[6].result, "stale square root");
    assert_eq!(fields[6].computed_result.as_deref(), Some("√9"));
    assert_eq!(fields[7].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[7].result, "stale cube root");
    assert_eq!(fields[7].computed_result.as_deref(), Some("3√27"));
    assert_eq!(fields[8].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[8].result, "stale nested equation");
    assert_eq!(fields[8].computed_result.as_deref(), Some("1/(2/3)"));
    assert_eq!(fields[9].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[9].result, "stale nested radical");
    assert_eq!(fields[9].computed_result.as_deref(), Some("√(1/4)"));
    assert_eq!(fields[10].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[10].instruction, r#"EQ \b("Chapter One")"#);
    assert_eq!(fields[10].result, "stale bracket equation");
    assert_eq!(fields[10].computed_result.as_deref(), Some("(Chapter One)"));
    assert_eq!(fields[11].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[11].instruction, r#"EQ \x \to \bo(\f(5,8)) \* Upper"#);
    assert_eq!(fields[11].result, "stale boxed equation");
    assert_eq!(fields[11].computed_result.as_deref(), Some("(5/8)"));
    assert_eq!(fields[12].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[12].instruction, r#"EQ \b \bc\{ (\r(3,x))"#);
    assert_eq!(fields[12].result, "stale brace bracket equation");
    assert_eq!(fields[12].computed_result.as_deref(), Some("{(3√x)}"));
    assert_eq!(fields[13].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[13].instruction, r#"EQ \b \lc\[ \rc\] ("Range")"#);
    assert_eq!(fields[13].result, "stale explicit bracket equation");
    assert_eq!(fields[13].computed_result.as_deref(), Some("[Range]"));
    assert_eq!(fields[14].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[14].instruction, r#"EQ \l(A,"B, C",\r(4,16))"#);
    assert_eq!(fields[14].result, "stale list equation");
    assert_eq!(fields[14].computed_result.as_deref(), Some("A,B, C,(4√16)"));
    assert_eq!(fields[15].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(
        fields[15].instruction,
        r#"EQ \a \al \co2 \vs3 \hs3(Axy,Bxy,A,B)"#
    );
    assert_eq!(fields[15].result, "stale array equation");
    assert_eq!(
        fields[15].computed_result.as_deref(),
        Some("Axy\tBxy\nA\tB")
    );
    assert_eq!(fields[16].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[16].instruction, r#"EQ \d \fo10 \li()"#);
    assert_eq!(fields[16].result, "stale displace equation");
    assert_eq!(fields[16].computed_result.as_deref(), Some(""));
    assert_eq!(fields[17].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[17].instruction, r#"EQ \s\up8(UB)\s\do8(2)"#);
    assert_eq!(fields[17].result, "stale script equation");
    assert_eq!(fields[17].computed_result.as_deref(), Some("^{UB}_2"));
    assert_eq!(fields[18].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[18].instruction, r#"EQ \i(0,1,x)"#);
    assert_eq!(fields[18].result, "stale integral equation");
    assert_eq!(fields[18].computed_result.as_deref(), Some("∫_0^1 x"));
    assert_eq!(fields[19].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[19].instruction, r#"EQ \i \su(1,5,\f(3,4))"#);
    assert_eq!(fields[19].result, "stale summation equation");
    assert_eq!(fields[19].computed_result.as_deref(), Some("Σ_1^5 (3/4)"));
    assert_eq!(fields[20].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[20].instruction, r#"EQ \i \pr(1,3,y)"#);
    assert_eq!(fields[20].result, "stale product equation");
    assert_eq!(fields[20].computed_result.as_deref(), Some("Π_1^3 y"));
    assert_eq!(fields[21].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[21].instruction, r#"EQ \i \in(a,b,\r(9))"#);
    assert_eq!(fields[21].result, "stale integral option equation");
    assert_eq!(fields[21].computed_result.as_deref(), Some("∫_a^b (√9)"));
    assert_eq!(fields[22].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[22].instruction, r#"EQ \i \fc\∮(C,D,z)"#);
    assert_eq!(fields[22].result, "stale custom integral equation");
    assert_eq!(fields[22].computed_result.as_deref(), Some("∮_C^D z"));
    assert_eq!(fields[23].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[23].instruction, r#"EQ \i \vc\∯(S,T,w)"#);
    assert_eq!(fields[23].result, "stale vertical custom integral equation");
    assert_eq!(fields[23].computed_result.as_deref(), Some("∯_S^T w"));
    assert_eq!(fields[24].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[24].instruction, r#"EQ \o \ac("A",/)"#);
    assert_eq!(fields[24].result, "stale overstrike equation");
    assert_eq!(fields[24].computed_result.as_deref(), Some("A/"));
    assert_eq!(fields[25].kind, FieldKind::Display("SYMBOL".to_string()));
    assert_eq!(fields[25].result, "symbol");
    assert_eq!(fields[25].computed_result.as_deref(), Some("•"));
    assert_eq!(fields[26].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[26].instruction, r#"EQ \s\ai4()\di3()"#);
    assert_eq!(fields[26].result, "stale empty script equation");
    assert_eq!(fields[26].computed_result.as_deref(), Some(""));
    assert_eq!(fields[27].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(
        fields[27].instruction,
        r#"EQ \s\ai4()\up8(UB)\s\di3()\do8(2)"#
    );
    assert_eq!(fields[27].result, "stale layout script equation");
    assert_eq!(fields[27].computed_result.as_deref(), Some("^{UB}_2"));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Display("ADVANCE".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached unsupported advance")
            && main_text.contains("1/2")
            && main_text.contains("ALPHA, ONE/BETA TWO")
            && main_text.contains("3/4")
            && main_text.contains("A,B/C\\D")
            && main_text.contains("√9")
            && main_text.contains("3√27")
            && main_text.contains("1/(2/3)")
            && main_text.contains("√(1/4)")
            && main_text.contains("(Chapter One)")
            && main_text.contains("(5/8)")
            && main_text.contains("{(3√x)}")
            && main_text.contains("[Range]")
            && main_text.contains("A,B, C,(4√16)")
            && main_text.contains("Axy\tBxy\nA\tB")
            && main_text.contains("^{UB}_2")
            && main_text.contains("∫_0^1 x")
            && main_text.contains("Σ_1^5 (3/4)")
            && main_text.contains("Π_1^3 y")
            && main_text.contains("∫_a^b (√9)")
            && main_text.contains("∮_C^D z")
            && main_text.contains("∯_S^T w")
            && main_text.contains("A/")
            && main_text.contains("•"),
        "computed and cached display field results should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("offset text")
            && !main_text.contains("stale equation")
            && !main_text.contains("stale quoted equation")
            && !main_text.contains("stale semicolon equation")
            && !main_text.contains("stale escaped equation")
            && !main_text.contains("stale square root")
            && !main_text.contains("stale cube root")
            && !main_text.contains("stale nested equation")
            && !main_text.contains("stale nested radical")
            && !main_text.contains("stale bracket equation")
            && !main_text.contains("stale boxed equation")
            && !main_text.contains("stale brace bracket equation")
            && !main_text.contains("stale explicit bracket equation")
            && !main_text.contains("stale list equation")
            && !main_text.contains("stale array equation")
            && !main_text.contains("stale displace equation")
            && !main_text.contains("stale script equation")
            && !main_text.contains("stale integral equation")
            && !main_text.contains("stale summation equation")
            && !main_text.contains("stale product equation")
            && !main_text.contains("stale integral option equation")
            && !main_text.contains("stale custom integral equation")
            && !main_text.contains("stale vertical custom integral equation")
            && !main_text.contains("stale overstrike equation")
            && !main_text.contains("symbol")
            && !main_text.contains("stale empty script equation")
            && !main_text.contains("stale layout script equation"),
        "computed display fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_display_field_diagnostics_split_valid_broader_eq_from_malformed_eq() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" EQ \s\up8(A)\ai4(B) "><w:r><w:t>cached broader script equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\up8(A "><w:r><w:t>cached malformed script equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \d \fo10(A) "><w:r><w:t>cached broader displace equation</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \d \fo10(A "><w:r><w:t>cached malformed displace equation</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[0].instruction, r#"EQ \s\up8(A)\ai4(B)"#);
    assert_eq!(fields[0].result, "cached broader script equation");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[1].instruction, r#"EQ \s\up8(A"#);
    assert_eq!(fields[1].result, "cached malformed script equation");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[2].instruction, r#"EQ \d \fo10(A)"#);
    assert_eq!(fields[2].result, "cached broader displace equation");
    assert_eq!(fields[2].computed_result.as_deref(), Some("A"));
    assert_eq!(fields[3].kind, FieldKind::Display("EQ".to_string()));
    assert_eq!(fields[3].instruction, r#"EQ \d \fo10(A"#);
    assert_eq!(fields[3].result, "cached malformed displace equation");
    assert_eq!(fields[3].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Display("EQ".to_string()),
            count: 3,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(main_text.contains("cached broader script equation"));
    assert!(main_text.contains("cached malformed script equation"));
    assert!(!main_text.contains("cached broader displace equation"));
    assert!(main_text.contains("A"));
    assert!(main_text.contains("cached malformed displace equation"));
}

#[test]
fn docx_symbol_field_computes_deterministic_symbols() {
    let doc = Document::open(&symbol_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    for field in &fields {
        assert_eq!(field.kind, FieldKind::Display("SYMBOL".to_string()));
    }
    assert_eq!(fields[0].instruction, "SYMBOL 163");
    assert_eq!(fields[0].computed_result.as_deref(), Some("£"));
    assert_eq!(fields[1].instruction, "SYMBOL 0x03BB \\u \\h");
    assert_eq!(fields[1].computed_result.as_deref(), Some("λ"));
    assert_eq!(fields[2].instruction, "SYMBOL 211 \\f \"Symbol\" \\s 12");
    assert_eq!(fields[2].computed_result.as_deref(), Some("©"));
    assert_eq!(fields[3].instruction, "SYMBOL 183 \\fSymbol \\s12");
    assert_eq!(fields[3].computed_result.as_deref(), Some("•"));
    assert_eq!(fields[4].instruction, "SYMBOL 0x0041 \\u \\s \"10\"");
    assert_eq!(fields[4].computed_result.as_deref(), Some("A"));
    assert_eq!(fields[5].instruction, "SYMBOL 0x0042 \\u \\s\"11\"");
    assert_eq!(fields[5].computed_result.as_deref(), Some("B"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale pound")
            && !main_text.contains("stale lambda")
            && !main_text.contains("stale copyright")
            && !main_text.contains("stale compact symbol")
            && !main_text.contains("stale quoted size")
            && !main_text.contains("stale compact quoted size"),
        "computed SYMBOL fields should replace stale cached text: {main_text:?}"
    );
    assert!(main_text.contains("£"), "{main_text:?}");
    assert!(main_text.contains("λ"), "{main_text:?}");
    assert!(main_text.contains("©"), "{main_text:?}");
    assert!(main_text.contains("•"), "{main_text:?}");
    assert!(main_text.contains("A"), "{main_text:?}");
    assert!(main_text.contains("B"), "{main_text:?}");
}

#[test]
fn docx_symbol_field_diagnostics_split_valid_unmapped_font_from_malformed_symbol() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SYMBOL 65 \f Wingdings "><w:r><w:t>cached unmapped wingdings</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 65 \f &quot;Wingdings "><w:r><w:t>cached malformed symbol</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Display("SYMBOL".to_string()));
    assert_eq!(fields[0].instruction, r#"SYMBOL 65 \f Wingdings"#);
    assert_eq!(fields[0].result, "cached unmapped wingdings");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Display("SYMBOL".to_string()));
    assert_eq!(fields[1].instruction, r#"SYMBOL 65 \f "Wingdings "#);
    assert_eq!(fields[1].result, "cached malformed symbol");
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Display("SYMBOL".to_string()),
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(main_text.contains("cached unmapped wingdings"));
    assert!(main_text.contains("cached malformed symbol"));
}

#[test]
fn docx_action_fields_compute_display_text_without_running_actions() {
    let doc = Document::open(&action_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 11);
    assert_eq!(fields[0].kind, FieldKind::Action("GOTOBUTTON".to_string()));
    assert_eq!(fields[0].instruction, "GOTOBUTTON TargetBookmark \"Jump\"");
    assert_eq!(fields[0].result, "stale jump");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Jump"));
    assert_eq!(fields[1].kind, FieldKind::Action("GOTOBUTTON".to_string()));
    assert_eq!(
        fields[1].instruction,
        "GOTOBUTTON TargetBookmark Jump Now \\* Upper"
    );
    assert_eq!(fields[1].result, "stale jump upper");
    assert_eq!(fields[1].computed_result.as_deref(), Some("JUMP NOW"));
    assert_eq!(fields[2].kind, FieldKind::Action("MACROBUTTON".to_string()));
    assert_eq!(fields[2].result, "stale run");
    assert_eq!(fields[2].computed_result.as_deref(), Some("Run report"));
    assert_eq!(fields[3].kind, FieldKind::Action("MACROBUTTON".to_string()));
    assert_eq!(
        fields[3].instruction,
        "MACROBUTTON RunReport Run \\* Upper Again"
    );
    assert_eq!(fields[3].result, "cached malformed action");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].kind, FieldKind::Action("MACROBUTTON".to_string()));
    assert_eq!(
        fields[4].instruction,
        "MACROBUTTON RunReport \\* MERGEFORMAT"
    );
    assert_eq!(fields[4].result, "cached target-only action");
    assert_eq!(fields[4].computed_result, None);
    assert_eq!(fields[5].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(fields[5].result, "Print instruction");
    assert_eq!(fields[5].computed_result.as_deref(), Some(""));
    assert_eq!(fields[6].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(fields[6].instruction, "PRINT status");
    assert_eq!(fields[6].result, "Unquoted print instruction");
    assert_eq!(fields[6].computed_result.as_deref(), Some(""));
    assert_eq!(fields[7].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(fields[7].instruction, "PRINT status ready \\* MERGEFORMAT");
    assert_eq!(fields[7].result, "Multi-token print instruction");
    assert_eq!(fields[7].computed_result.as_deref(), Some(""));
    assert_eq!(fields[8].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(
        fields[8].instruction,
        "PRINT \\p ReportBox \"0 0 moveto\" \\* MERGEFORMAT"
    );
    assert_eq!(fields[8].result, "PostScript instruction");
    assert_eq!(fields[8].computed_result.as_deref(), Some(""));
    assert_eq!(fields[9].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(
        fields[9].instruction,
        "PRINT \\pReportBox \"compact moveto\""
    );
    assert_eq!(fields[9].result, "Compact PostScript instruction");
    assert_eq!(fields[9].computed_result.as_deref(), Some(""));
    assert_eq!(fields[10].kind, FieldKind::Action("PRINT".to_string()));
    assert_eq!(fields[10].instruction, "PRINT \\z \"bad\"");
    assert_eq!(fields[10].result, "cached unsupported print");
    assert_eq!(fields[10].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Action("MACROBUTTON".to_string()),
                count: 2,
            },
            FieldKindCount {
                kind: FieldKind::Action("PRINT".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
        ]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Jump")
            && main_text.contains("JUMP NOW")
            && main_text.contains("Run report")
            && main_text.contains("cached malformed action")
            && main_text.contains("cached target-only action")
            && main_text.contains("cached unsupported print"),
        "computed action display text and cached unsupported PRINT text should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale jump")
            && !main_text.contains("stale jump upper")
            && !main_text.contains("stale run")
            && !main_text.contains("Print instruction")
            && !main_text.contains("Unquoted print instruction")
            && !main_text.contains("Multi-token print instruction")
            && !main_text.contains("PostScript instruction")
            && !main_text.contains("Compact PostScript instruction"),
        "computed action display text and validated hidden PRINT output should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_compatibility_fields_are_named_noncomputed_fields() {
    let doc = Document::open(&compatibility_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert_eq!(
        fields[0].kind,
        FieldKind::Compatibility("PRIVATE".to_string())
    );
    assert_eq!(fields[0].instruction, "PRIVATE");
    assert_eq!(fields[0].result, "converted payload");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::Compatibility("ADDIN".to_string())
    );
    assert_eq!(fields[1].instruction, "ADDIN hidden-data");
    assert_eq!(fields[1].result, "addin payload");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Compatibility("DATA".to_string()));
    assert_eq!(fields[2].instruction, "DATA legacy-data");
    assert_eq!(fields[2].result, "data payload");
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].kind,
        FieldKind::Compatibility("GLOSSARY".to_string())
    );
    assert_eq!(fields[3].instruction, "GLOSSARY AutoTextName");
    assert_eq!(fields[3].result, "glossary payload");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(
        fields[4].kind,
        FieldKind::Compatibility("HTMLACTIVEX".to_string())
    );
    assert_eq!(fields[4].instruction, "HTMLACTIVEX LegacyControl");
    assert_eq!(fields[4].result, "activex payload");
    assert_eq!(fields[4].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Compatibility("PRIVATE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Compatibility("ADDIN".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Compatibility("DATA".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Compatibility("GLOSSARY".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Compatibility("HTMLACTIVEX".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 5,
        }]
    );
    assert!(
        doc.main_text().contains("converted payload")
            && doc.main_text().contains("addin payload")
            && doc.main_text().contains("data payload")
            && doc.main_text().contains("glossary payload")
            && doc.main_text().contains("activex payload"),
        "{:?}",
        doc.main_text()
    );
}

#[test]
fn docx_compatibility_diagnostics_split_valid_broader_fields_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PRIVATE legacy-data "><w:r><w:t>cached private payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ADDIN &quot;bad "><w:r><w:t>cached malformed addin</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::Compatibility("PRIVATE".to_string())
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::Compatibility("ADDIN".to_string())
    );
    assert_eq!(fields[1].instruction, r#"ADDIN "bad "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Compatibility("PRIVATE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Compatibility("ADDIN".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
    assert!(doc.main_text().contains("cached malformed addin"));
}

#[test]
fn docx_barcode_fields_are_named_noncomputed_fields() {
    let doc = Document::open(&barcode_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(
        fields[0].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[0].instruction,
        "DISPLAYBARCODE \"https://example.com\" QR \\q H"
    );
    assert_eq!(fields[0].result, "QR preview");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::Barcode("MERGEBARCODE".to_string())
    );
    assert_eq!(fields[1].instruction, "MERGEBARCODE CustomerId CODE128 \\t");
    assert_eq!(fields[1].result, "Merge barcode preview");
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(fields[2].kind, FieldKind::Barcode("BARCODE".to_string()));
    assert_eq!(fields[2].instruction, "BARCODE \"9781234567890\"");
    assert_eq!(fields[2].result, "Legacy barcode preview");
    assert_eq!(fields[2].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Barcode("DISPLAYBARCODE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Barcode("MERGEBARCODE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Barcode("BARCODE".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 3,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("QR preview")
            && main_text.contains("Merge barcode preview")
            && main_text.contains("Legacy barcode preview"),
        "cached barcode field results should remain in main text: {main_text:?}"
    );
}

#[test]
fn docx_barcode_diagnostics_split_valid_broader_fields_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \q H "><w:r><w:t>QR preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; \q H "><w:r><w:t>cached missing barcode type</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \q "><w:r><w:t>cached missing quality operand</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com QR "><w:r><w:t>cached malformed barcode</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \* "><w:r><w:t>cached dangling barcode format</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 5);
    assert_eq!(
        fields[0].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[0].instruction,
        r#"DISPLAYBARCODE "https://example.com" QR \q H"#
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(
        fields[1].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[1].instruction,
        r#"DISPLAYBARCODE "https://example.com" \q H"#
    );
    assert_eq!(fields[1].computed_result, None);
    assert_eq!(
        fields[2].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[2].instruction,
        r#"DISPLAYBARCODE "https://example.com" QR \q"#
    );
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(
        fields[3].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[3].instruction,
        r#"DISPLAYBARCODE "https://example.com QR "#
    );
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(
        fields[4].kind,
        FieldKind::Barcode("DISPLAYBARCODE".to_string())
    );
    assert_eq!(
        fields[4].instruction,
        r#"DISPLAYBARCODE "https://example.com" QR \*"#
    );
    assert_eq!(fields[4].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Barcode("DISPLAYBARCODE".to_string()),
            count: 5,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 4,
            },
        ]
    );
    assert!(doc.main_text().contains("cached missing barcode type"));
    assert!(doc.main_text().contains("cached missing quality operand"));
    assert!(doc.main_text().contains("cached malformed barcode"));
    assert!(doc.main_text().contains("cached dangling barcode format"));
}

#[test]
fn docx_legacy_form_fields_materialize_deterministic_values() {
    let doc = Document::open(&legacy_form_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 8);
    assert_eq!(fields[0].kind, FieldKind::FormField("FORMTEXT".to_string()));
    assert_eq!(fields[0].instruction, "FORMTEXT");
    assert_eq!(fields[0].result, "Alice");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Alice"));
    assert_eq!(fields[1].kind, FieldKind::FormField("FORMTEXT".to_string()));
    assert_eq!(fields[1].instruction, "FORMTEXT");
    assert_eq!(fields[1].result, "");
    assert_eq!(fields[1].computed_result.as_deref(), Some("No content."));
    assert_eq!(
        fields[2].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[2].result, "stale checked");
    assert_eq!(fields[2].computed_result.as_deref(), Some("☒"));
    assert_eq!(
        fields[3].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[3].result, "stale unchecked");
    assert_eq!(fields[3].computed_result.as_deref(), Some("☐"));
    assert_eq!(
        fields[4].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[4].result, "stale default checked");
    assert_eq!(fields[4].computed_result.as_deref(), Some("☒"));
    assert_eq!(
        fields[5].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[5].result, "stale option");
    assert_eq!(fields[5].computed_result.as_deref(), Some("Option C"));
    assert_eq!(
        fields[6].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[6].result, "stale default option");
    assert_eq!(fields[6].computed_result.as_deref(), Some("Default B"));
    assert_eq!(
        fields[7].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[7].result, "stale invalid option");
    assert_eq!(fields[7].computed_result.as_deref(), Some("Fallback B"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Alice")
            && main_text.contains("No content.")
            && main_text.contains("☒")
            && main_text.contains("☐")
            && main_text.contains("Option C")
            && main_text.contains("Default B")
            && main_text.contains("Fallback B"),
        "computed legacy form field results should appear in main text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale checked")
            && !main_text.contains("stale unchecked")
            && !main_text.contains("stale default checked")
            && !main_text.contains("stale option")
            && !main_text.contains("stale default option")
            && !main_text.contains("stale invalid option"),
        "computed legacy form field results should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_legacy_form_field_format_switches_apply_to_dropdowns() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMDROPDOWN \* Upper "><w:ffData><w:ddList><w:result w:val="1"/><w:listEntry w:val="first"/><w:listEntry w:val="chosen option"/></w:ddList></w:ffData><w:r><w:t>stale dropdown</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMCHECKBOX \* MERGEFORMAT "><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData><w:r><w:t>stale checkbox</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[0].computed_result.as_deref(), Some("CHOSEN OPTION"));
    assert_eq!(
        fields[1].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[1].computed_result.as_deref(), Some("\u{2612}"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("CHOSEN OPTION")
            && main_text.contains('\u{2612}')
            && !main_text.contains("stale dropdown")
            && !main_text.contains("stale checkbox"),
        "legacy form field format switches should materialize deterministic results: {main_text:?}"
    );
}

#[test]
fn docx_legacy_form_dropdown_indices_accept_whitespace() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><w:ddList><w:default w:val=" 0 "/><w:result w:val=" 1 "/><w:listEntry w:val="Default A"/><w:listEntry w:val="Chosen B"/></w:ddList></w:ffData><w:r><w:t>stale spaced dropdown</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[0].result, "stale spaced dropdown");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Chosen B"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Chosen B") && !main_text.contains("stale spaced dropdown"),
        "whitespace-padded dropdown indexes should materialize: {main_text:?}"
    );
}

#[test]
fn docx_legacy_form_context_ignores_deleted_fields() {
    let doc = Document::open(&legacy_form_deleted_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[0].result, "stale visible unchecked");
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{2610}"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains('\u{2610}') && !main_text.contains("deleted checked"),
        "deleted form metadata must not shift visible form results: {main_text:?}"
    );
}

#[test]
fn docx_legacy_form_context_uses_single_alternate_content_branch() {
    let doc = Document::open(&legacy_form_alternate_content_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMDROPDOWN".to_string())
    );
    assert_eq!(fields[0].result, "stale option");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Choice option"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Choice option") && !main_text.contains("Fallback option"),
        "computed legacy form field must use one AlternateContent branch: {main_text:?}"
    );
}

#[test]
fn docx_malformed_legacy_form_fields_preserve_cached_text() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMCHECKBOX &quot;bad "><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData><w:r><w:t>cached malformed checkbox</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[0].instruction, r#"FORMCHECKBOX "bad "#);
    assert_eq!(fields[0].result, "cached malformed checkbox");
    assert_eq!(fields[0].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::FormField("FORMCHECKBOX".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
    assert!(doc.main_text().contains("cached malformed checkbox"));
}

#[test]
fn docx_legacy_form_fields_report_malformed_format_switches() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMTEXT \* Upper "><w:r><w:t>alice</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMTEXT \* BadFormat "><w:r><w:t>cached bad format</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].computed_result.as_deref(), Some("ALICE"));
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
    assert!(doc.main_text().contains("ALICE"));
    assert!(doc.main_text().contains("cached bad format"));
}

#[test]
fn docx_protected_legacy_form_fields_preserve_cached_text() {
    let doc = Document::open(&docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/settings.xml",
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:documentProtection w:edit=" forms " w:enforcement="1"/></w:settings>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData></w:fldChar></w:r><w:r><w:instrText> FORMCHECKBOX </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>cached protected checked</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[0].result, "cached protected checked");
    assert_eq!(fields[0].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::FormField("FORMCHECKBOX".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(main_text.contains("cached protected checked"));
    assert!(!main_text.contains('\u{2612}'));
}

#[test]
fn docx_unenforced_legacy_form_protection_materializes_values() {
    let doc = Document::open(&docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/settings.xml",
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:documentProtection w:edit="forms"/></w:settings>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMCHECKBOX "><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData><w:r><w:t>cached unenforced checked</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{2612}"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(main_text.contains('\u{2612}'));
    assert!(!main_text.contains("cached unenforced checked"));
}

#[test]
fn docx_protected_legacy_form_diagnostics_split_valid_cached_from_malformed_syntax() {
    let doc = Document::open(&docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/settings.xml",
            r#"<w:settings xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:documentProtection w:edit="forms" w:enforcement="1"/></w:settings>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMCHECKBOX "><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData><w:r><w:t>cached protected checked</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMTEXT &quot;bad "><w:r><w:t>cached malformed form text</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    let fields = doc.fields();
    assert_eq!(fields.len(), 2);
    assert_eq!(
        fields[0].kind,
        FieldKind::FormField("FORMCHECKBOX".to_string())
    );
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::FormField("FORMTEXT".to_string()));
    assert_eq!(fields[1].instruction, r#"FORMTEXT "bad "#);
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::FormField("FORMCHECKBOX".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::FormField("FORMTEXT".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
    assert!(doc.main_text().contains("cached malformed form text"));
}

#[test]
fn docx_simple_field_result_preserves_cached_inline_markers() {
    let doc = Document::open(&simple_cached_result_inline_marker_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[0].instruction, "CUSTOM value");
    assert_eq!(fields[0].result, "Alpha\tBeta\nGamma-Hard\u{00ad}Soft");
    assert_eq!(fields[0].computed_result, None);
    assert!(
        doc.main_text()
            .contains("Alpha\tBeta\nGamma-Hard\u{00ad}Soft"),
        "{:?}",
        doc.main_text()
    );
}

#[test]
fn docx_complex_field_result_preserves_cached_inline_markers() {
    let doc = Document::open(&complex_cached_result_inline_marker_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[0].instruction, "CUSTOM value");
    assert_eq!(fields[0].result, "One\tTwo\nThree-Hard\u{00ad}Soft");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[1].instruction, "CUSTOM markersOnly");
    assert_eq!(fields[1].result, "\t\n-\u{00ad}");
    assert_eq!(fields[1].computed_result, None);
    assert!(
        doc.main_text().contains("One\tTwo\nThree-Hard\u{00ad}Soft")
            && doc.main_text().contains("\t\n-\u{00ad}"),
        "{:?}",
        doc.main_text()
    );
}

#[test]
fn docx_nested_complex_fields_preserve_outer_cached_result() {
    let doc = Document::open(&nested_complex_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::MergeField);
    assert_eq!(fields[0].instruction, "MERGEFIELD InnerName");
    assert_eq!(fields[0].result, "Inner Value");
    assert_eq!(fields[1].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[1].instruction, "CUSTOM outer");
    assert_eq!(fields[1].result, "Before Inner Value After");
}

#[test]
fn docx_nested_complex_with_nested_simple_field_preserves_outer_cached_result() {
    let doc = Document::open(&nested_complex_with_simple_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::MergeField);
    assert_eq!(fields[0].instruction, "MERGEFIELD InnerName");
    assert_eq!(fields[0].result, "Inner Value");
    assert_eq!(fields[1].kind, FieldKind::Unknown("CUSTOM".to_string()));
    assert_eq!(fields[1].instruction, "CUSTOM outer");
    assert_eq!(fields[1].result, "Before Inner Value After");
}

#[test]
fn docx_toc_field_computes_unambiguous_heading_outline_range() {
    let doc = Document::open(&toc_heading_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\"");
    assert_eq!(fields[0].result, "stale toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale toc"),
        "resolved TOC fields should display computed heading text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
    assert_eq!(main_text.matches("Excluded Detail").count(), 1);
}

#[test]
fn docx_toc_field_normalizes_inline_tabs_and_breaks_in_heading_text() {
    let doc = Document::open(&toc_heading_inline_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-1\"");
    assert_eq!(fields[0].result, "stale inline toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary Detail-Follow-up")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale inline toc"),
        "resolved TOC fields should display computed heading text with normalized inline markers: {main_text:?}"
    );
    assert!(
        main_text.contains("Executive Summary Detail-Follow-up"),
        "{main_text:?}"
    );
}

#[test]
fn docx_bare_toc_field_defaults_to_heading_levels_one_through_three() {
    let doc = Document::open(&bare_toc_heading_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC");
    assert_eq!(fields[0].result, "stale bare toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale bare toc"),
        "bare TOC fields should display computed heading text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
    assert_eq!(main_text.matches("Mitigation").count(), 2);
    assert_eq!(main_text.matches("Excluded Detail").count(), 1);
}

#[test]
fn docx_default_toc_with_neutral_switches_uses_default_heading_levels() {
    let doc = Document::open(&default_neutral_toc_heading_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\h \\z");
    assert_eq!(fields[0].result, "stale neutral default toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );
    assert_eq!(fields[1].kind, FieldKind::Toc);
    assert_eq!(fields[1].instruction, "TOC \\n \"1-3\"");
    assert_eq!(fields[1].result, "stale no-page default toc");
    assert_eq!(
        fields[1].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );
    assert_eq!(fields[2].kind, FieldKind::Toc);
    assert_eq!(fields[2].instruction, "TOC \\p \"-\"");
    assert_eq!(fields[2].result, "stale separator default toc");
    assert_eq!(
        fields[2].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );
    assert_eq!(fields[3].kind, FieldKind::Toc);
    assert_eq!(fields[3].instruction, "TOC \\s chapter \\d \"-\"");
    assert_eq!(fields[3].result, "stale sequence default toc");
    assert_eq!(
        fields[3].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );
    assert_eq!(fields[4].kind, FieldKind::Toc);
    assert_eq!(fields[4].instruction, "TOC \\* Upper");
    assert_eq!(fields[4].result, "stale upper default toc");
    assert_eq!(
        fields[4].computed_result.as_deref(),
        Some("EXECUTIVE SUMMARY\n  RISKS\n    MITIGATION")
    );
    assert_eq!(fields[5].kind, FieldKind::Toc);
    assert_eq!(fields[5].instruction, "TOC \\* MERGEFORMAT");
    assert_eq!(fields[5].result, "stale mergeformat default toc");
    assert_eq!(
        fields[5].computed_result.as_deref(),
        Some("Executive Summary\n  Risks\n    Mitigation")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale neutral default toc"),
        "neutral-only TOC fields should display computed default heading text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale no-page default toc"),
        "no-page TOC fields should display computed default heading text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale separator default toc"),
        "separator TOC fields should display computed default heading text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale sequence default toc"),
        "sequence TOC fields should display computed default heading text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale upper default toc"),
        "formatted TOC fields should display computed default heading text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale mergeformat default toc"),
        "mergeformat TOC fields should display computed default heading text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_toc_field_with_neutral_switches_computes_heading_outline_range() {
    let doc = Document::open(&toc_neutral_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\h \\z \\w \\x");
    assert_eq!(fields[0].result, "stale neutral toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale neutral toc"),
        "neutral TOC switches should still display computed heading text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
}

#[test]
fn docx_toc_field_applies_general_format_switches() {
    let doc = Document::open(&toc_general_format_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\* Upper");
    assert_eq!(fields[0].result, "stale upper toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("EXECUTIVE SUMMARY\n  RISK REVIEW")
    );
    assert_eq!(fields[1].kind, FieldKind::Toc);
    assert_eq!(fields[1].instruction, "TOC \\o \"1-2\" \\* Caps");
    assert_eq!(fields[1].result, "stale caps toc");
    assert_eq!(
        fields[1].computed_result.as_deref(),
        Some("Executive Summary\n  Risk Review")
    );
    assert_eq!(fields[2].kind, FieldKind::Toc);
    assert_eq!(fields[2].instruction, "TOC \\o \"1-2\" \\* MERGEFORMAT");
    assert_eq!(fields[2].result, "stale mergeformat toc");
    assert_eq!(
        fields[2].computed_result.as_deref(),
        Some("executive summary\n  risk review")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale upper toc")
            && !main_text.contains("stale caps toc")
            && !main_text.contains("stale mergeformat toc"),
        "TOC general format switches should display computed field text: {main_text:?}"
    );
    assert!(main_text.contains("EXECUTIVE SUMMARY"), "{main_text:?}");
    assert!(main_text.contains("Executive Summary"), "{main_text:?}");
    assert!(main_text.contains("executive summary"), "{main_text:?}");
}

#[test]
fn docx_toc_field_with_no_page_number_switch_computes_heading_outline_range() {
    let doc = Document::open(&toc_no_page_numbers_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\n \"1-2\"");
    assert_eq!(fields[0].result, "stale no-page toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale no-page toc"),
        "TOC \\n should not block computed heading text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
}

#[test]
fn docx_toc_field_with_entry_page_separator_switch_computes_heading_text() {
    let doc = Document::open(&toc_entry_page_separator_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\p \"-\"");
    assert_eq!(fields[0].result, "stale separator toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale separator toc"),
        "TOC \\p changes only the omitted page-number separator and should keep computed heading text: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
}

#[test]
fn docx_toc_field_with_sequence_page_separator_switch_computes_heading_text() {
    let doc = Document::open(&toc_sequence_page_separator_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(
        fields[0].instruction,
        "TOC \\o \"1-2\" \\s chapter \\d \"-\""
    );
    assert_eq!(fields[0].result, "stale sequence separator toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Risks")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale sequence separator toc"),
        "TOC \\s and \\d change only omitted page-number prefixes/separators and should keep computed heading text: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Risks").count(), 2);
}

#[test]
fn docx_toc_u_field_computes_explicit_outline_levels_only() {
    let doc = Document::open(&toc_outline_level_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\u");
    assert_eq!(fields[0].result, "stale outline toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Outline Heading")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale outline toc"),
        "TOC \\u should display computed outline-level text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Style Heading").count(), 1);
    assert!(main_text.contains("Outline Heading"), "{main_text:?}");
}

#[test]
fn docx_toc_o_u_field_combines_heading_styles_and_outline_levels() {
    let doc = Document::open(&toc_heading_and_outline_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\u");
    assert_eq!(fields[0].result, "stale combined toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Style Heading\nOutline Heading")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale combined toc"),
        "TOC \\o plus \\u should display computed heading and outline text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Style Heading").count(), 2);
    assert_eq!(main_text.matches("Outline Heading").count(), 2);
}

#[test]
fn docx_toc_b_field_limits_computation_to_bookmark_scope() {
    let doc = Document::open(&toc_bookmark_scope_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-2\" \\b ScopedToc");
    assert_eq!(fields[0].result, "stale scoped toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Scoped Heading\n  Scoped Detail")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale scoped toc"),
        "resolved TOC \\b fields should display scoped computed heading text: {main_text:?}"
    );
    assert_eq!(main_text.matches("Outside Heading").count(), 1);
    assert_eq!(main_text.matches("Scoped Heading").count(), 2);
    assert_eq!(main_text.matches("Scoped Detail").count(), 2);
    assert_eq!(main_text.matches("Trailing Heading").count(), 1);
}

#[test]
fn docx_toc_b_field_without_inclusion_switch_uses_default_heading_levels_in_scope() {
    let doc = Document::open(&toc_bookmark_only_scope_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\b ScopedToc");
    assert_eq!(fields[0].result, "stale bookmark-only toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Scoped Heading\n  Scoped Detail")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale bookmark-only toc"),
        "standalone TOC \\b should display scoped default heading text: {main_text:?}"
    );
    assert_eq!(main_text.matches("Outside Heading").count(), 1);
    assert_eq!(main_text.matches("Scoped Heading").count(), 2);
    assert_eq!(main_text.matches("Scoped Detail").count(), 2);
    assert_eq!(main_text.matches("Scoped Deep Heading").count(), 1);
}

#[test]
fn docx_toc_o_field_without_range_includes_all_heading_levels() {
    let doc = Document::open(&toc_outline_without_range_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o");
    assert_eq!(fields[0].result, "stale open-outline toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n      Appendix Detail")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale open-outline toc"),
        "TOC \\o without an explicit range should display every heading level: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Appendix Detail").count(), 2);
}

#[test]
fn docx_toc_b_field_with_missing_bookmark_keeps_cached_text() {
    let doc = Document::open(&missing_toc_bookmark_scope_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(fields[0].instruction, "TOC \\o \"1-1\" \\b MissingScope");
    assert_eq!(fields[0].result, "stale missing scope toc");
    assert_eq!(fields[0].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("stale missing scope toc"),
        "TOC \\b with a missing scope should keep cached result text: {main_text:?}"
    );
}

#[test]
fn docx_toc_field_with_custom_style_switch_computes_matching_entries() {
    let doc = Document::open(&toc_custom_style_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(
        fields[0].instruction,
        "TOC \\o \"1-1\" \\t \"CustomHeading,2\""
    );
    assert_eq!(fields[0].result, "stale custom toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Custom Finding")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale custom toc"),
        "TOC \\t should display computed heading and custom-style entries: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Custom Finding").count(), 2);
}

#[test]
fn docx_toc_field_with_quoted_custom_style_switch_keeps_style_name_spaces() {
    let doc = Document::open(&toc_quoted_custom_style_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(
        fields[0].instruction,
        "TOC \\o \"1-1\" \\t \"Custom Heading,2\""
    );
    assert_eq!(fields[0].result, "stale quoted custom toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary\n  Custom Finding")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale quoted custom toc"),
        "TOC \\t should preserve quoted custom style names with spaces: {main_text:?}"
    );
    assert_eq!(main_text.matches("Executive Summary").count(), 2);
    assert_eq!(main_text.matches("Custom Finding").count(), 2);
}

#[test]
fn docx_toc_field_with_unmatched_custom_style_switch_keeps_heading_computation() {
    let doc = Document::open(&advanced_toc_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Toc);
    assert_eq!(
        fields[0].instruction,
        "TOC \\o \"1-2\" \\t \"CustomHeading,1\""
    );
    assert_eq!(fields[0].result, "stale advanced toc");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Executive Summary")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale advanced toc"),
        "unmatched TOC \\t styles should not block computable heading text: {main_text:?}"
    );
    assert!(main_text.contains("Executive Summary"), "{main_text:?}");
}

#[test]
fn docx_toc_field_with_tc_switch_computes_matching_tc_entries() {
    let doc = Document::open(&toc_tc_field_switch_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::TocEntry);
    assert_eq!(fields[0].instruction, "TC \"Manual Entry\" \\f m \\l 2");
    assert_eq!(fields[0].result, "stale manual tc");
    assert_eq!(fields[0].computed_result.as_deref(), Some(""));
    assert_eq!(fields[1].kind, FieldKind::TocEntry);
    assert_eq!(fields[1].instruction, "TC \"Other Entry\" \\f x \\l 1");
    assert_eq!(fields[1].result, "stale other tc");
    assert_eq!(fields[1].computed_result.as_deref(), Some(""));
    let toc = fields
        .iter()
        .find(|field| field.kind == FieldKind::Toc)
        .expect("TOC field is parsed");

    assert_eq!(toc.instruction, "TOC \\f m");
    assert_eq!(toc.result, "stale tc toc");
    assert_eq!(toc.computed_result.as_deref(), Some("  Manual Entry"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale tc toc"),
        "TOC \\f should display computed TC entry text in the read model: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale manual tc") && !main_text.contains("stale other tc"),
        "TC marker fields should not leak cached marker text into main text: {main_text:?}"
    );
    assert!(main_text.contains("Manual Entry"), "{main_text:?}");
    assert!(
        !main_text.contains("Other Entry"),
        "TOC \\f identifier should filter non-matching TC entries: {main_text:?}"
    );
}

#[test]
fn docx_toc_entries_ignore_deleted_tc_fields() {
    let doc = Document::open(&toc_deleted_tc_field_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::TocEntry);
    assert_eq!(fields[0].instruction, "TC \"Visible Entry\" \\f m \\l 1");
    assert_eq!(fields[0].computed_result.as_deref(), Some(""));
    let toc = fields
        .iter()
        .find(|field| field.kind == FieldKind::Toc)
        .expect("TOC field is parsed");

    assert_eq!(toc.computed_result.as_deref(), Some("Visible Entry"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Visible Entry")
            && !main_text.contains("Deleted Entry")
            && !main_text.contains("Moved Entry"),
        "TOC entries must follow accepted-current revision wrappers: {main_text:?}"
    );
}

#[test]
fn docx_toc_entries_use_single_alternate_content_branch() {
    let doc = Document::open(&toc_alternate_content_heading_docx()).expect("fixture opens");
    let fields = doc.fields();
    let toc = fields
        .iter()
        .find(|field| field.kind == FieldKind::Toc)
        .expect("TOC field is parsed");

    assert_eq!(toc.computed_result.as_deref(), Some("Choice Inline"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Choice Inline")
            && !main_text.contains("Fallback Heading")
            && !main_text.contains("Fallback Inline"),
        "TOC entries must use one AlternateContent branch: {main_text:?}"
    );
}

#[test]
fn docx_invalid_toc_entry_reports_unsupported_switch() {
    let doc = Document::open(&invalid_toc_entry_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::TocEntry);
    assert_eq!(fields[0].instruction, "TC \\l 2");
    assert_eq!(fields[0].result, "cached invalid tc");
    assert_eq!(fields[0].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::TocEntry,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );

    let main_text = doc.main_text();
    assert!(
        main_text.contains("cached invalid tc"),
        "unsupported TC marker should preserve cached text: {main_text:?}"
    );
}

#[test]
fn docx_toc_c_field_computes_matching_seq_caption_entries() {
    let doc = Document::open(&toc_sequence_caption_switch_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure");
    assert_eq!(fields[0].result, "1");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Sequence);
    assert_eq!(fields[1].instruction, "SEQ Table");
    assert_eq!(fields[1].result, "1");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    let toc = fields
        .iter()
        .find(|field| field.kind == FieldKind::Toc)
        .expect("TOC field is parsed");

    assert_eq!(toc.instruction, "TOC \\c \"Figure\"");
    assert_eq!(toc.result, "stale figures toc");
    assert_eq!(toc.computed_result.as_deref(), Some("Figure 1: Mercury"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale figures toc"),
        "TOC \\c should display computed SEQ caption entries in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Figure 1: Mercury").count(), 2);
    assert_eq!(main_text.matches("Table 1: Invoices").count(), 1);
}

#[test]
fn docx_toc_a_field_computes_matching_seq_caption_text_without_label() {
    let doc = Document::open(&toc_sequence_caption_text_switch_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure");
    assert_eq!(fields[0].result, "8");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::Sequence);
    assert_eq!(fields[1].instruction, "SEQ Table");
    assert_eq!(fields[1].result, "2");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    let toc = fields
        .iter()
        .find(|field| field.kind == FieldKind::Toc)
        .expect("TOC field is parsed");

    assert_eq!(toc.instruction, "TOC \\a Figure");
    assert_eq!(toc.result, "stale caption-text toc");
    assert_eq!(toc.computed_result.as_deref(), Some("Mercury"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale caption-text toc"),
        "TOC \\a should display computed SEQ caption text in the read model: {main_text:?}"
    );
    assert_eq!(main_text.matches("Figure 1: Mercury").count(), 1);
    assert_eq!(main_text.matches("Mercury").count(), 2);
    assert_eq!(main_text.matches("Table 1: Invoices").count(), 1);
}

#[test]
fn docx_ref_field_computes_unambiguous_bookmark_text() {
    let doc = Document::open(&ref_bookmark_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF Figure1");
    assert_eq!(fields[0].result, "stale cached text");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Figure 1"));
    assert!(
        !doc.main_text().contains("stale cached text"),
        "resolved REF fields should display computed bookmark text in the read model"
    );
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, "REF MissingBookmark");
    assert_eq!(fields[1].computed_result, None);
}

#[test]
fn docx_ref_targets_ignore_deleted_bookmark_text() {
    let doc = Document::open(&ref_deleted_bookmark_text_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF ClauseText");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Visible clause"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Visible clause")
            && !main_text.contains("deleted clause")
            && !main_text.contains("moved clause")
            && !main_text.contains("stale deleted ref"),
        "computed REF bookmark text must follow accepted-current wrappers: {main_text:?}"
    );
}

#[test]
fn docx_ref_targets_use_single_alternate_content_branch() {
    let doc = Document::open(&ref_alternate_content_bookmark_text_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF AltText");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Choice clause"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Choice clause")
            && !main_text.contains("Fallback clause")
            && !main_text.contains("stale alternate ref"),
        "computed REF bookmark text must use one AlternateContent branch: {main_text:?}"
    );
}

#[test]
fn docx_complex_ref_field_displays_computed_bookmark_text() {
    let doc = Document::open(&complex_ref_bookmark_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF Figure1");
    assert_eq!(fields[0].result, "stale complex ref");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Figure 1"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale complex ref"),
        "resolved complex REF fields should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(main_text.contains("Figure 1"), "{main_text:?}");
}

#[test]
fn docx_ref_field_preserves_paragraph_breaks_in_bookmark_text() {
    let doc = Document::open(&multi_paragraph_ref_bookmark_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF ClauseText");
    assert_eq!(fields[0].result, "stale multi ref");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("First paragraph.\nSecond paragraph.")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale multi ref"),
        "resolved multi-paragraph REF fields should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(
        main_text.contains("First paragraph.\nSecond paragraph."),
        "{main_text:?}"
    );
}

#[test]
fn docx_ref_field_preserves_inline_tabs_and_breaks_in_bookmark_text() {
    let doc = Document::open(&inline_break_ref_bookmark_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF InlineText");
    assert_eq!(fields[0].result, "stale inline ref");
    assert_eq!(
        fields[0].computed_result.as_deref(),
        Some("Alpha\tBeta\nGamma-Delta")
    );

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale inline ref"),
        "resolved inline-break REF fields should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(
        main_text.contains("Alpha\tBeta\nGamma-Delta"),
        "{main_text:?}"
    );
}

#[test]
fn docx_ref_field_computes_hidden_word_bookmark_text() {
    let doc = Document::open(&hidden_ref_bookmark_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF _Ref123456789");
    assert_eq!(fields[0].result, "stale hidden ref");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Table 2"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale hidden ref"),
        "resolved hidden-bookmark REF fields should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(main_text.contains("Table 2"), "{main_text:?}");
}

#[test]
fn docx_direct_bookmark_field_is_treated_as_ref_when_bookmark_exists() {
    let doc = Document::open(&direct_bookmark_ref_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "Figure1");
    assert_eq!(fields[0].result, "stale direct ref");
    assert_eq!(fields[0].computed_result.as_deref(), Some("Figure 1"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale direct ref"),
        "direct bookmark fields should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(main_text.contains("Figure 1"), "{main_text:?}");
}

#[test]
fn docx_direct_bookmark_field_applies_supported_ref_switches() {
    let doc = Document::open(&direct_bookmark_ref_switch_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "Figure1 \\* Upper");
    assert_eq!(fields[0].result, "stale direct upper");
    assert_eq!(fields[0].computed_result.as_deref(), Some("FIGURE ONE"));
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, "Figure1 \\*FirstCap");
    assert_eq!(fields[1].result, "stale direct first-cap");
    assert_eq!(fields[1].computed_result.as_deref(), Some("Figure one"));
    assert_eq!(fields[2].kind, FieldKind::Ref);
    assert_eq!(fields[2].instruction, "Figure1 \\h");
    assert_eq!(fields[2].result, "stale direct hyperlink");
    assert_eq!(fields[2].computed_result.as_deref(), Some("figure one"));
    assert_eq!(fields[3].kind, FieldKind::Ref);
    assert_eq!(fields[3].instruction, "Figure1 \\d \"-\"");
    assert_eq!(fields[3].result, "direct sequence separator");
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].kind, FieldKind::Ref);
    assert_eq!(fields[4].instruction, "Figure1 \\f");
    assert_eq!(fields[4].result, "direct note mark");
    assert_eq!(fields[4].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale direct upper")
            && !main_text.contains("stale direct first-cap")
            && !main_text.contains("stale direct hyperlink")
            && main_text.contains("direct sequence separator")
            && main_text.contains("direct note mark"),
        "direct bookmark fields with supported REF switches should display computed bookmark text: {main_text:?}"
    );
    assert!(main_text.contains("FIGURE ONE"), "{main_text:?}");
    assert!(main_text.contains("Figure one"), "{main_text:?}");
    assert!(main_text.contains("figure one"), "{main_text:?}");
}

#[test]
fn docx_direct_bookmark_p_field_computes_relative_source_position() {
    let doc = Document::open(&direct_relative_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "LaterBookmark \\p");
    assert_eq!(fields[0].result, "stale direct below");
    assert_eq!(fields[0].computed_result.as_deref(), Some("below"));
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, "Figure1 \\p");
    assert_eq!(fields[1].result, "stale direct above");
    assert_eq!(fields[1].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale direct below") && !main_text.contains("stale direct above"),
        "direct bookmark \\p fields should display computed source-order relative text: {main_text:?}"
    );
    assert!(main_text.contains("below"), "{main_text:?}");
    assert!(main_text.contains("above"), "{main_text:?}");
}

#[test]
fn docx_ref_field_applies_upper_lower_format_switches() {
    let doc = Document::open(&ref_text_format_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF Figure1 \\* Upper");
    assert_eq!(fields[0].result, "stale upper ref");
    assert_eq!(fields[0].computed_result.as_deref(), Some("FIGURE ONE"));
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, "REF Figure1 \\*Lower");
    assert_eq!(fields[1].result, "stale lower ref");
    assert_eq!(fields[1].computed_result.as_deref(), Some("figure one"));
    assert_eq!(fields[2].kind, FieldKind::Ref);
    assert_eq!(fields[2].instruction, "REF Figure1 \\* Caps");
    assert_eq!(fields[2].result, "stale caps ref");
    assert_eq!(fields[2].computed_result.as_deref(), Some("Figure One"));
    assert_eq!(fields[3].kind, FieldKind::Ref);
    assert_eq!(fields[3].instruction, "REF Figure1 \\*FirstCap");
    assert_eq!(fields[3].result, "stale first-cap ref");
    assert_eq!(fields[3].computed_result.as_deref(), Some("Figure one"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale upper ref")
            && !main_text.contains("stale lower ref")
            && !main_text.contains("stale caps ref")
            && !main_text.contains("stale first-cap ref"),
        "resolved REF format switches should display computed bookmark text in the read model: {main_text:?}"
    );
    assert!(main_text.contains("FIGURE ONE"), "{main_text:?}");
    assert!(main_text.contains("figure one"), "{main_text:?}");
    assert!(main_text.contains("Figure One"), "{main_text:?}");
    assert!(main_text.contains("Figure one"), "{main_text:?}");
}

#[test]
fn docx_ref_field_with_broader_switch_keeps_cached_text() {
    let doc = Document::open(&broader_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF Figure1 \\f");
    assert_eq!(fields[0].result, "note mark");
    assert_eq!(fields[0].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("note mark"),
        "broader REF switch fields should keep cached result text in the read model: {main_text:?}"
    );
}

#[test]
fn docx_note_ref_field_computes_bookmarked_note_marks_and_relative_position() {
    let doc = Document::open(&note_ref_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 9);
    assert!(fields.iter().all(|field| field.kind == FieldKind::NoteRef));
    assert_eq!(fields[0].instruction, "NOTEREF LaterNote \\p");
    assert_eq!(fields[0].result, "stale below note");
    assert_eq!(fields[0].computed_result.as_deref(), Some("below"));
    assert_eq!(fields[1].instruction, "NOTEREF LaterNote \\p \\* Upper");
    assert_eq!(fields[1].result, "stale uppercase below note");
    assert_eq!(fields[1].computed_result.as_deref(), Some("BELOW"));
    assert_eq!(fields[2].instruction, "NOTEREF FootOne \\h");
    assert_eq!(fields[2].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[3].instruction, "FTNREF FootOne");
    assert_eq!(fields[3].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[4].instruction, "NOTEREF FootOne \\f \\* MERGEFORMAT");
    assert_eq!(fields[4].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[5].instruction, "NOTEREF FootOne \\p");
    assert_eq!(fields[5].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[6].instruction, "NOTEREF EndOne");
    assert_eq!(fields[6].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[7].instruction, "NOTEREF LaterNote");
    assert_eq!(fields[7].result, "stale complex note mark");
    assert_eq!(fields[7].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[8].instruction, "NOTEREF MissingNote");
    assert_eq!(fields[8].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale below note")
            && !main_text.contains("stale uppercase below note")
            && !main_text.contains("stale note mark")
            && !main_text.contains("stale legacy note mark")
            && !main_text.contains("stale formatted note mark")
            && !main_text.contains("stale above note")
            && !main_text.contains("stale endnote mark")
            && !main_text.contains("stale complex note mark"),
        "resolved NOTEREF fields should display computed note text: {main_text:?}"
    );
    assert!(main_text.contains("below"), "{main_text:?}");
    assert!(main_text.contains("BELOW"), "{main_text:?}");
    assert!(main_text.contains("above"), "{main_text:?}");
    assert!(main_text.contains("stale missing note"), "{main_text:?}");
}

#[test]
fn docx_note_ref_context_uses_single_alternate_content_branch() {
    let doc = Document::open(&note_ref_alternate_content_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.kind == FieldKind::NoteRef));
    assert_eq!(fields[0].instruction, "NOTEREF FootOne");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].instruction, "NOTEREF FootOne \\p");
    assert_eq!(fields[1].computed_result.as_deref(), Some("above"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(main_text.contains("1"), "{main_text:?}");
    assert!(main_text.contains("above"), "{main_text:?}");
    assert!(!main_text.contains("stale alternate"), "{main_text:?}");
}

#[test]
fn docx_note_ref_applies_number_format_switches() {
    let doc = Document::open(&note_ref_number_format_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.kind == FieldKind::NoteRef));
    assert_eq!(fields[0].instruction, "NOTEREF LaterNote \\* roman");
    assert_eq!(fields[0].computed_result.as_deref(), Some("ii"));
    assert_eq!(
        fields[1].instruction,
        "NOTEREF FootOne \\* OrdText \\* Upper"
    );
    assert_eq!(fields[1].computed_result.as_deref(), Some("FIRST"));

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());

    let main_text = doc.main_text();
    assert!(main_text.contains("ii"), "{main_text:?}");
    assert!(main_text.contains("FIRST"), "{main_text:?}");
    assert!(!main_text.contains("stale roman note"), "{main_text:?}");
    assert!(!main_text.contains("stale ordinal note"), "{main_text:?}");
}

#[test]
fn docx_note_body_fields_are_exposed() {
    let doc = Document::open(&note_body_field_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Filename);
    assert_eq!(fields[0].instruction, "FILENAME \\p");
    assert_eq!(fields[0].result, "note.docx");
    assert_eq!(fields[1].kind, FieldKind::Page);
    assert_eq!(fields[1].instruction, "PAGE");
    assert_eq!(fields[1].result, "4");
    assert_eq!(doc.footnote_text(), "note.docx");
    assert_eq!(doc.endnote_text(), "4");
}

#[test]
fn docx_ref_f_field_computes_bookmarked_note_reference_marks() {
    let doc = Document::open(&ref_note_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF FootOne \\f");
    assert_eq!(fields[0].result, "stale foot ref mark");
    assert_eq!(fields[0].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[1].instruction, "FootOne \\f");
    assert_eq!(fields[1].result, "stale direct foot ref mark");
    assert_eq!(fields[1].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[2].instruction, "REF EndOne \\h \\f \\* MERGEFORMAT");
    assert_eq!(fields[2].result, "stale end ref mark");
    assert_eq!(fields[2].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[3].instruction, "REF FootOne \\f");
    assert_eq!(fields[3].result, "stale complex foot ref mark");
    assert_eq!(fields[3].computed_result.as_deref(), Some("5"));
    assert_eq!(fields[4].instruction, "REF FootOne \\f \\* roman");
    assert_eq!(fields[4].result, "stale roman foot ref mark");
    assert_eq!(fields[4].computed_result.as_deref(), Some("vi"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale foot ref mark")
            && !main_text.contains("stale direct foot ref mark")
            && !main_text.contains("stale end ref mark")
            && !main_text.contains("stale complex foot ref mark")
            && !main_text.contains("stale roman foot ref mark"),
        "resolved REF \\f fields should display computed note reference marks: {main_text:?}"
    );
}

#[test]
fn docx_ref_n_field_computes_numbered_bookmark_paragraph() {
    let doc = Document::open(&numbered_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF Clause \\n");
    assert_eq!(fields[0].result, "stale number");
    assert_eq!(fields[0].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[1].instruction, "REF Clause \\n \\p");
    assert_eq!(fields[1].result, "stale number relative");
    assert_eq!(fields[1].computed_result.as_deref(), Some("3 above"));
    assert_eq!(fields[2].instruction, "Clause \\n");
    assert_eq!(fields[2].result, "stale direct number");
    assert_eq!(fields[2].computed_result.as_deref(), Some("3"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale number")
            && !main_text.contains("stale number relative")
            && !main_text.contains("stale direct number"),
        "REF \\n fields should display computed numbered-paragraph text: {main_text:?}"
    );
    assert!(main_text.contains("Numbered clause"), "{main_text:?}");
    assert!(main_text.contains("3"), "{main_text:?}");
    assert!(main_text.contains("3 above"), "{main_text:?}");
}

#[test]
fn docx_ref_numbering_uses_single_alternate_content_branch() {
    let doc = Document::open(&alternate_content_numbered_ref_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF AltClause \\n");
    assert_eq!(fields[0].result, "stale alt number");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("stale alt number"),
        "REF \\n should not double-count Choice/Fallback numbered paragraphs: {main_text:?}"
    );
}

#[test]
fn docx_ref_n_t_field_suppresses_non_numeric_label_text() {
    let doc = Document::open(&numbered_ref_suppress_text_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF SectionClause \\n \\t");
    assert_eq!(fields[0].result, "stale numeric text");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1.01"));
    assert_eq!(fields[1].instruction, "REF SectionClause \\n \\t \\p");
    assert_eq!(fields[1].result, "stale numeric relative");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1.01 above"));
    assert_eq!(fields[2].instruction, "SectionClause \\n \\t");
    assert_eq!(fields[2].result, "stale direct numeric text");
    assert_eq!(fields[2].computed_result.as_deref(), Some("1.01"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale numeric text")
            && !main_text.contains("stale numeric relative")
            && !main_text.contains("stale direct numeric text")
            && !main_text.contains("Section 1.01"),
        "REF \\n \\t fields should display only numeric label text: {main_text:?}"
    );
    assert!(main_text.contains("1.01"), "{main_text:?}");
    assert!(main_text.contains("1.01 above"), "{main_text:?}");
}

#[test]
fn docx_ref_w_field_computes_full_context_numbered_bookmark_paragraph() {
    let doc = Document::open(&full_context_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF DeepClause \\w");
    assert_eq!(fields[0].result, "stale full context");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1.a.i"));
    assert_eq!(fields[1].instruction, "REF DeepClause \\w \\p");
    assert_eq!(fields[1].result, "stale full relative");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1.a.i above"));
    assert_eq!(fields[2].instruction, "DeepClause \\w");
    assert_eq!(fields[2].result, "stale direct full");
    assert_eq!(fields[2].computed_result.as_deref(), Some("1.a.i"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale full context")
            && !main_text.contains("stale full relative")
            && !main_text.contains("stale direct full"),
        "REF \\w fields should display computed full-context numbering: {main_text:?}"
    );
    assert!(main_text.contains("1.a.i"), "{main_text:?}");
    assert!(main_text.contains("1.a.i above"), "{main_text:?}");
}

#[test]
fn docx_ref_w_t_field_accepts_full_context_numeric_text_suppression() {
    let doc = Document::open(&full_context_ref_suppress_text_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF DeepClause \\w \\t");
    assert_eq!(fields[0].result, "stale full numeric text");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1.a.i"));
    assert_eq!(fields[1].instruction, "REF DeepClause \\w \\t \\p");
    assert_eq!(fields[1].result, "stale full numeric relative");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1.a.i above"));
    assert_eq!(fields[2].instruction, "DeepClause \\w \\t");
    assert_eq!(fields[2].result, "stale direct full numeric");
    assert_eq!(fields[2].computed_result.as_deref(), Some("1.a.i"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale full numeric text")
            && !main_text.contains("stale full numeric relative")
            && !main_text.contains("stale direct full numeric")
            && !main_text.contains("Section")
            && !main_text.contains("Article")
            && !main_text.contains("Part"),
        "REF \\w \\t fields should display computed full-context numbering without level text: {main_text:?}"
    );
    assert!(main_text.contains("1.a.i"), "{main_text:?}");
    assert!(main_text.contains("1.a.i above"), "{main_text:?}");
}

#[test]
fn docx_ref_r_field_computes_relative_context_numbered_bookmark_paragraph() {
    let doc = Document::open(&relative_context_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(fields[0].instruction, "REF LaterClause \\r");
    assert_eq!(fields[0].result, "stale relative context");
    assert_eq!(fields[0].computed_result.as_deref(), Some("5.2"));
    assert_eq!(fields[1].instruction, "REF LaterClause \\r \\p");
    assert_eq!(fields[1].result, "stale relative context position");
    assert_eq!(fields[1].computed_result.as_deref(), Some("5.2 below"));
    assert_eq!(fields[2].instruction, "LaterClause \\r \\t");
    assert_eq!(fields[2].result, "stale direct relative context");
    assert_eq!(fields[2].computed_result.as_deref(), Some("5.2"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale relative context")
            && !main_text.contains("stale relative context position")
            && !main_text.contains("stale direct relative context"),
        "REF \\r fields should display computed relative-context numbering: {main_text:?}"
    );
    assert!(main_text.contains("5.2"), "{main_text:?}");
    assert!(main_text.contains("5.2 below"), "{main_text:?}");
}

#[test]
fn docx_ref_p_field_computes_relative_source_position() {
    let doc = Document::open(&relative_ref_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF LaterBookmark \\p");
    assert_eq!(fields[0].result, "stale below");
    assert_eq!(fields[0].computed_result.as_deref(), Some("below"));
    assert_eq!(fields[1].kind, FieldKind::Ref);
    assert_eq!(fields[1].instruction, "REF Figure1 \\p");
    assert_eq!(fields[1].result, "stale above");
    assert_eq!(fields[1].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        !main_text.contains("stale below") && !main_text.contains("stale above"),
        "REF \\p should display computed source-order relative text: {main_text:?}"
    );
    assert!(main_text.contains("below"), "{main_text:?}");
    assert!(main_text.contains("above"), "{main_text:?}");
}

#[test]
fn docx_ref_p_field_uses_single_alternate_content_branch() {
    let doc = Document::open(&relative_ref_alternate_content_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].instruction, "REF LaterBookmark \\p");
    assert_eq!(fields[0].result, "stale visible above");
    assert_eq!(fields[0].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("above") && !main_text.contains("fallback relative"),
        "REF \\p position context must ignore untaken AlternateContent branches: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_fields_are_named_field_kind_without_computed_page_numbers() {
    let doc = Document::open(&page_ref_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "3");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF \"TableOne\" \\p");
    assert_eq!(fields[1].result, "above");
    assert_eq!(fields[1].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("3") && main_text.contains("above"),
        "PAGEREF fields should keep cached page-reference text until layout page resolution exists: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_explicit_manual_break_page_targets() {
    let doc = Document::open(&page_ref_manual_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF \"Figure1\" \\* MERGEFORMAT");
    assert_eq!(fields[1].result, "old page");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF Figure1 \\p");
    assert_eq!(fields[2].result, "above");
    assert_eq!(fields[2].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2\n2") && main_text.contains("above"),
        "manual-break PAGEREF fields should replace only unambiguous page numbers: {main_text:?}"
    );
    assert!(
        !main_text.contains("99") && !main_text.contains("old page"),
        "computed manual-break PAGEREF fields should not display stale cached page text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_leading_break_relative_position() {
    let doc = Document::open(&page_ref_leading_break_relative_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\p");
    assert_eq!(fields[0].result, "stale relative");
    assert_eq!(fields[0].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("above") && !main_text.contains("stale relative"),
        "leading-break PAGEREF relative positions should use computed text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_applies_deterministic_number_format_switches() {
    let doc = Document::open(&page_ref_format_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 10);
    let expected = [
        ("PAGEREF Figure1 \\* ROMAN", "II"),
        ("PAGEREF Figure1 \\*roman", "ii"),
        ("PAGEREF Figure1 \\* alphabetic", "b"),
        ("PAGEREF Figure1 \\*ALPHABETIC", "B"),
        ("PAGEREF Figure1 \\* Ordinal", "2nd"),
        ("PAGEREF Figure1 \\* CardText", "two"),
        ("PAGEREF Figure1 \\* CardText \\* Upper", "TWO"),
        ("PAGEREF Figure1 \\* OrdText", "second"),
        ("PAGEREF Figure1 \\* Arabic", "2"),
        ("PAGEREF Figure1 \\* ArabicDash", "- 2 -"),
    ];
    for (field, (instruction, computed)) in fields.iter().zip(expected) {
        assert_eq!(field.kind, FieldKind::PageRef);
        assert_eq!(field.instruction, instruction);
        assert_eq!(field.computed_result.as_deref(), Some(computed));
    }

    let main_text = doc.main_text();
    assert!(
        main_text.contains("II\nii\nb\nB\n2nd\ntwo\nTWO\nsecond\n2\n- 2 -"),
        "formatted PAGEREF values should be materialized in the read model: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale"),
        "computed PAGEREF format switches should not keep stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_accepts_mixed_case_number_format_switch() {
    let doc = Document::open(&page_ref_mixed_case_format_switch_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\* ArAbIc");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("stale mixed arabic"),
        "mixed-case PAGEREF number format should compute like Arabic: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_after_visible_content_keeps_cached_page_text() {
    let doc = Document::open(&page_ref_after_visible_manual_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("99"),
        "PAGEREF after visible content should keep cached text because auto-pagination can intervene: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_source_rendered_page_break_targets() {
    let doc = Document::open(&page_ref_rendered_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FigureTwo \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(
        fields[1].instruction,
        "PAGEREF \"FigureTwo\" \\* CHARFORMAT"
    );
    assert_eq!(fields[1].result, "old rendered page");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF FigureTwo \\p");
    assert_eq!(fields[2].result, "below");
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2\n2\nabove"),
        "rendered-break PAGEREF fields should replace supported page-number and relative-position fields: {main_text:?}"
    );
    assert!(
        !main_text.contains("99")
            && !main_text.contains("old rendered page")
            && !main_text.contains("below"),
        "computed rendered-break PAGEREF fields should not display stale cached page text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_source_rendered_relative_below() {
    let doc =
        Document::open(&page_ref_rendered_break_relative_below_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FigureLater \\p");
    assert_eq!(fields[0].result, "above");
    assert_eq!(fields[0].computed_result.as_deref(), Some("below"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("below") && !main_text.contains("above"),
        "same-page PAGEREF before its bookmark should compute relative position as below: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_advances_rendered_context_across_manual_page_breaks() {
    let doc = Document::open(&page_ref_rendered_then_manual_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF PageThree \\p");
    assert_eq!(fields[0].result, "stale distant relative");
    assert_eq!(fields[0].computed_result.as_deref(), Some("on page 3"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF PageThree \\h");
    assert_eq!(fields[1].result, "99");
    assert_eq!(fields[1].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF PageThree \\p");
    assert_eq!(fields[2].result, "old relative");
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[3].kind, FieldKind::Page);
    assert_eq!(fields[3].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[3].result, "stale current page");
    assert_eq!(fields[3].computed_result.as_deref(), Some("3"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("on page 3\nPage two lead.")
            && main_text.contains("3\nabove\n3"),
        "rendered-context hard breaks should advance computed PAGEREF relative, PAGEREF page, and PAGE output: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale distant relative")
            && !main_text.contains("99")
            && !main_text.contains("old relative")
            && !main_text.contains("stale current page"),
        "computed rendered-context fields should not keep stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_and_page_ref_compute_structural_section_break_targets() {
    let doc = Document::open(&page_ref_section_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 6);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::Page);
    assert_eq!(fields[1].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[2].kind, FieldKind::Page);
    assert_eq!(fields[2].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[2].computed_result.as_deref(), Some("5"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF NextSection \\h");
    assert_eq!(fields[3].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[4].kind, FieldKind::PageRef);
    assert_eq!(fields[4].instruction, "PAGEREF EvenSection \\* roman");
    assert_eq!(fields[4].computed_result.as_deref(), Some("iv"));
    assert_eq!(fields[5].kind, FieldKind::PageRef);
    assert_eq!(fields[5].instruction, "PAGEREF OddSection \\p");
    assert_eq!(fields[5].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2\nPage three lead.\nEven section target\n4\nOdd section target\n5\n2\niv\nabove"),
        "structural section-break PAGE and PAGEREF fields should render computed text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale"),
        "computed section-break PAGE and PAGEREF fields should not keep stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_defers_content_paragraph_section_break_until_paragraph_end() {
    let doc =
        Document::open(&page_ref_content_paragraph_section_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF BeforeSectionBreak \\h");
    assert_eq!(fields[1].result, "stale before break");
    assert_eq!(fields[1].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("Before break1\nAfter break\nstale before break"),
        "section-break paragraph content should stay on the pre-break page: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale same-paragraph page"),
        "computed fields should replace stale section-break paragraph text: {main_text:?}"
    );

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );
}

#[test]
fn docx_page_ref_computes_default_next_page_section_break_target() {
    let doc = Document::open(&page_ref_default_section_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF DefaultSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2"),
        "default next-page section-break PAGEREF should render computed text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale default"),
        "computed default section-break PAGEREF should not keep stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_page_break_before_targets() {
    let doc = Document::open(&page_ref_page_break_before_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF BreakBefore \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF BreakAfterIntro \\h");
    assert_eq!(fields[1].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(
        fields[2].instruction,
        "PAGEREF RenderedBreakBefore \\* Ordinal"
    );
    assert_eq!(fields[2].computed_result.as_deref(), Some("5th"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF RenderedBreakBefore \\p");
    assert_eq!(fields[3].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2\n3\n5th\nabove"),
        "pageBreakBefore PAGEREF fields should render computed text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale"),
        "computed pageBreakBefore PAGEREF fields should not keep stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_ignores_disabled_page_break_before() {
    let doc = Document::open(&page_ref_disabled_page_break_before_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF NoForcedBreak \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("stale disabled break"),
        "disabled pageBreakBefore should not advance PAGEREF page context: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_applies_structural_section_page_number_restart() {
    let doc = Document::open(&page_ref_section_page_number_restart_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Restarted \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF RestartedNext \\* ROMAN");
    assert_eq!(fields[1].computed_result.as_deref(), Some("VIII"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF RestartedNext \\p");
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("7\nVIII\nabove"),
        "section page-number restart should format displayed page labels: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale"),
        "computed restarted-section PAGEREF fields should not keep stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_computes_restarted_section_display_page_after_visible_intro() {
    let doc = Document::open(&page_ref_visible_intro_section_page_number_restart_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF RestartedAfterIntro \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF RestartedAfterIntro \\p");
    assert_eq!(fields[1].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("7")
            && !main_text.contains("stale restarted page")
            && main_text.contains("stale restarted relative"),
        "restarted section display page should compute while relative position stays cached: {main_text:?}"
    );

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 1,
        }]
    );
}

#[test]
fn docx_page_and_page_ref_apply_trusted_section_page_number_formats() {
    let doc = Document::open(&page_ref_section_page_number_format_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 10);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[0].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF RomanSection \\h");
    assert_eq!(fields[1].computed_result.as_deref(), Some("iii"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF RomanSection \\* Arabic");
    assert_eq!(fields[2].computed_result.as_deref(), Some("3"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF RomanSection \\p");
    assert_eq!(fields[3].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[4].kind, FieldKind::Page);
    assert_eq!(fields[4].instruction, "PAGE");
    assert_eq!(fields[4].computed_result.as_deref(), Some("04"));
    assert_eq!(fields[5].kind, FieldKind::PageRef);
    assert_eq!(fields[5].instruction, "PAGEREF DecimalZeroSection \\h");
    assert_eq!(fields[5].computed_result.as_deref(), Some("04"));
    assert_eq!(fields[6].kind, FieldKind::PageRef);
    assert_eq!(
        fields[6].instruction,
        "PAGEREF DecimalZeroSection \\* Arabic"
    );
    assert_eq!(fields[6].computed_result.as_deref(), Some("4"));
    assert_eq!(fields[7].kind, FieldKind::Page);
    assert_eq!(fields[7].instruction, "PAGE");
    assert_eq!(fields[7].computed_result.as_deref(), Some("- 5 -"));
    assert_eq!(fields[8].kind, FieldKind::PageRef);
    assert_eq!(fields[8].instruction, "PAGEREF DashedSection \\h");
    assert_eq!(fields[8].computed_result.as_deref(), Some("- 5 -"));
    assert_eq!(fields[9].kind, FieldKind::PageRef);
    assert_eq!(fields[9].instruction, "PAGEREF DashedSection \\* Arabic");
    assert_eq!(fields[9].computed_result.as_deref(), Some("5"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("3\niii\n3\nabove")
            && main_text.contains("04\n04\n4")
            && main_text.contains("- 5 -\n- 5 -\n5"),
        "trusted section page-number formats should drive supported PAGE and PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale roman current")
            && !main_text.contains("stale roman")
            && !main_text.contains("stale arabic override")
            && !main_text.contains("stale roman relative")
            && !main_text.contains("stale decimal current")
            && !main_text.contains("stale decimal zero")
            && !main_text.contains("stale decimal arabic")
            && !main_text.contains("stale dashed current")
            && !main_text.contains("stale dashed")
            && !main_text.contains("stale dashed arabic"),
        "computed section-format PAGE and PAGEREF fields should replace stale cached text: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[test]
fn docx_page_ref_relative_unsupported_section_format_reports_unsupported_switch() {
    let doc = Document::open(&page_ref_relative_unsupported_section_page_number_format_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF UnsupportedFmt \\p");
    assert_eq!(fields[0].result, "stale relative");
    assert_eq!(fields[0].computed_result, None);

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
}

#[test]
fn docx_page_ref_applies_decimal_full_width_section_page_number_format() {
    let doc = Document::open(&page_ref_decimal_full_width_section_page_number_format_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FullWidthSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("１２"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF FullWidthSection \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("１２\n12"),
        "decimalFullWidth section page-number format should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale fullwidth"),
        "computed decimalFullWidth PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_applies_decimal_enclosed_circle_section_page_number_format() {
    let doc = Document::open(&page_ref_decimal_enclosed_circle_section_page_number_format_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF CircleSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{246b}"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF CircleSection \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));

    let main_text = doc.main_text();
    let expected = format!("{}\n12", "\u{246b}");
    assert!(
        main_text.contains(&expected),
        "decimalEnclosedCircle section page-number format should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale circle"),
        "computed decimalEnclosedCircle PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_applies_decimal_enclosed_punctuation_section_page_number_formats() {
    let doc =
        Document::open(&page_ref_decimal_enclosed_punctuation_section_page_number_format_docx())
            .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FullstopSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{2493}"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF FullstopSection \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF ParenSection \\h");
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{2480}"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF ParenSection \\* Arabic");
    assert_eq!(fields[3].computed_result.as_deref(), Some("13"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains(&format!("{}\n12", "\u{2493}"))
            && main_text.contains(&format!("{}\n13", "\u{2480}")),
        "decimal enclosed punctuation section page-number formats should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale fullstop") && !main_text.contains("stale paren"),
        "computed decimal enclosed punctuation PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_applies_decimal_width_variant_section_page_number_formats() {
    let doc = Document::open(&page_ref_decimal_width_variant_section_page_number_format_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF HalfWidthSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("12"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(
        fields[1].instruction,
        "PAGEREF HalfWidthSection \\* ArabicDash"
    );
    assert_eq!(fields[1].computed_result.as_deref(), Some("- 12 -"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF FullWidthAltSection \\h");
    assert_eq!(fields[2].computed_result.as_deref(), Some("１３"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(
        fields[3].instruction,
        "PAGEREF FullWidthAltSection \\* Arabic"
    );
    assert_eq!(fields[3].computed_result.as_deref(), Some("13"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("12\n- 12 -") && main_text.contains("１３\n13"),
        "decimal width variant section page-number formats should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale halfwidth") && !main_text.contains("stale fullwidth alt"),
        "computed decimal width variant PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_applies_korean_section_page_number_formats() {
    let doc =
        Document::open(&page_ref_korean_section_page_number_format_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF GanadaSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{ac00}"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF GanadaSection \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF ChosungSection \\h");
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{3134}"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF ChosungSection \\* Arabic");
    assert_eq!(fields[3].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains(&format!("{}\n1", "\u{ac00}"))
            && main_text.contains(&format!("{}\n2", "\u{3134}")),
        "Korean section page-number formats should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale ganada") && !main_text.contains("stale chosung"),
        "computed Korean PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_applies_korean_numeric_section_page_number_formats() {
    let doc = Document::open(&page_ref_korean_numeric_section_page_number_format_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 8);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF KoreanDigitalSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{c77c}"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(
        fields[1].instruction,
        "PAGEREF KoreanDigitalSection \\* Arabic"
    );
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF KoreanCountingSection \\h");
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{b458}"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(
        fields[3].instruction,
        "PAGEREF KoreanCountingSection \\* Arabic"
    );
    assert_eq!(fields[3].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[4].kind, FieldKind::PageRef);
    assert_eq!(fields[4].instruction, "PAGEREF KoreanLegalSection \\h");
    assert_eq!(fields[4].computed_result.as_deref(), Some("\u{c2ed}"));
    assert_eq!(fields[5].kind, FieldKind::PageRef);
    assert_eq!(
        fields[5].instruction,
        "PAGEREF KoreanLegalSection \\* Arabic"
    );
    assert_eq!(fields[5].computed_result.as_deref(), Some("10"));
    assert_eq!(fields[6].kind, FieldKind::PageRef);
    assert_eq!(fields[6].instruction, "PAGEREF KoreanDigital2Section \\h");
    assert_eq!(fields[6].computed_result.as_deref(), Some("\u{c774}"));
    assert_eq!(fields[7].kind, FieldKind::PageRef);
    assert_eq!(
        fields[7].instruction,
        "PAGEREF KoreanDigital2Section \\* Arabic"
    );
    assert_eq!(fields[7].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains(&format!("{}\n1", "\u{c77c}"))
            && main_text.contains(&format!("{}\n2", "\u{b458}"))
            && main_text.contains(&format!("{}\n10", "\u{c2ed}"))
            && main_text.contains(&format!("{}\n2", "\u{c774}")),
        "Korean numeric section page-number formats should drive supported PAGEREF text: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale korean digital")
            && !main_text.contains("stale korean counting")
            && !main_text.contains("stale korean legal")
            && !main_text.contains("stale korean digital2"),
        "computed Korean numeric PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_and_page_ref_apply_final_section_page_number_format() {
    let doc =
        Document::open(&page_ref_final_section_page_number_format_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 4);
    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE \\* Arabic");
    assert_eq!(fields[0].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF FinalSection \\h");
    assert_eq!(fields[1].computed_result.as_deref(), Some("vi"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF FinalSection \\* Arabic");
    assert_eq!(fields[2].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[3].kind, FieldKind::PageRef);
    assert_eq!(fields[3].instruction, "PAGEREF FinalSection \\p");
    assert_eq!(fields[3].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("6\nvi\n6\nabove"),
        "final section page-number format should apply to computed PAGE and PAGEREF output: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale final"),
        "computed final-section PAGE and PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_final_section_numbering_ignores_deleted_paragraph_section() {
    let doc = Document::open(&page_ref_final_section_ignores_deleted_paragraph_section_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 3);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FinalSection \\h");
    assert_eq!(fields[0].computed_result.as_deref(), Some("VI"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF FinalSection \\* Arabic");
    assert_eq!(fields[1].computed_result.as_deref(), Some("6"));
    assert_eq!(fields[2].kind, FieldKind::PageRef);
    assert_eq!(fields[2].instruction, "PAGEREF FinalSection \\p");
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("VI\n6\nabove"),
        "accepted-current final section numbering should ignore deleted paragraph sectPr: {main_text:?}"
    );
    assert!(
        !main_text.contains("stale final"),
        "computed final-section PAGEREF fields should replace stale cached text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_keeps_ambiguous_pre_marker_manual_break_cached() {
    let doc = Document::open(&page_ref_visible_manual_break_before_rendered_hint_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF AmbiguousTarget \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("99"),
        "pre-marker hard breaks after visible content should remain cached: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computes_source_rendered_page_one_targets() {
    let doc = Document::open(&page_ref_rendered_break_page_one_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Cover \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("1") && !main_text.contains("99"),
        "rendered-break PAGEREF should compute page-one targets once source page markers exist: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_ignores_deleted_source_rendered_page_breaks() {
    let doc = Document::open(&page_ref_deleted_rendered_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result, None);

    let main_text = doc.main_text();
    assert!(
        main_text.contains("99"),
        "deleted rendered page-break markers should not make visible PAGEREF values computable: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_keeps_leading_hard_break_target_over_later_rendered_hint() {
    let doc = Document::open(&page_ref_leading_break_precedes_rendered_hint_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF Figure1 \\h");
    assert_eq!(fields[0].result, "99");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("99"),
        "leading hard-break target should not be overwritten by a later rendered-page hint: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_computed_fields_without_cached_results_are_materialized() {
    let doc =
        Document::open(&page_ref_rendered_break_no_cached_result_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF FigureTwo \\h");
    assert_eq!(fields[0].result, "");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(
        fields[1].instruction,
        "PAGEREF \"FigureTwo\" \\* MERGEFORMAT"
    );
    assert_eq!(fields[1].result, "");
    assert_eq!(fields[1].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2\n2"),
        "computed PAGEREF fields without cached result runs should still read as computed text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_wrapped_complex_field_uses_computed_result_in_body() {
    let doc =
        Document::open(&page_ref_rendered_break_wrapped_complex_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(
        fields[0].instruction,
        "PAGEREF \"FigureTwo\" \\* MERGEFORMAT"
    );
    assert_eq!(fields[0].result, "old wrapped page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("old wrapped page"),
        "complex PAGEREF inside accepted visible wrappers should read as computed text: {main_text:?}"
    );
}

#[test]
fn docx_page_ref_uses_single_alternate_content_page_break_branch() {
    let doc =
        Document::open(&page_ref_alternate_content_rendered_break_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF AltPage \\h");
    assert_eq!(fields[0].result, "stale alternate page");
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let main_text = doc.main_text();
    assert!(
        main_text.contains("2") && !main_text.contains("stale alternate page"),
        "PAGEREF page context should not double-count Choice/Fallback page markers: {main_text:?}"
    );

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
}

#[cfg(feature = "render")]
#[test]
fn docx_page_ref_complex_field_is_counted_in_model_render_report() {
    let doc = Document::open(&page_ref_docx()).expect("fixture opens");
    let model = doc.model();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert_eq!(rendered.report.unsupported.fields, 2);
    assert_eq!(
        rendered.report.unsupported.field_kinds,
        vec![rdoc::FieldKindCount {
            kind: FieldKind::PageRef,
            count: 2,
        }]
    );
}

#[cfg(feature = "render")]
#[test]
fn docx_page_ref_computed_manual_break_fields_are_not_model_render_warnings() {
    let doc = Document::open(&page_ref_manual_break_docx()).expect("fixture opens");
    let model = doc.model();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert_eq!(rendered.report.unsupported.fields, 1);
    assert_eq!(
        rendered.report.unsupported.field_kinds,
        vec![rdoc::FieldKindCount {
            kind: FieldKind::PageRef,
            count: 1,
        }]
    );
}

#[cfg(feature = "render")]
#[test]
fn docx_page_ref_computed_rendered_break_fields_are_not_model_render_warnings() {
    let doc = Document::open(&page_ref_rendered_break_docx()).expect("fixture opens");
    let model = doc.model();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert_eq!(rendered.report.unsupported.fields, 0);
    assert!(rendered.report.unsupported.field_kinds.is_empty());
}

#[cfg(feature = "render")]
#[test]
fn docx_page_ref_no_cached_result_fields_are_not_model_render_warnings() {
    let doc =
        Document::open(&page_ref_rendered_break_no_cached_result_docx()).expect("fixture opens");
    let model = doc.model();

    let rendered = rdoc::render_pdf_with_report(&model);

    assert_eq!(rendered.report.unsupported.fields, 0);
    assert!(rendered.report.unsupported.field_kinds.is_empty());
}

#[test]
fn field_kind_reports_canonical_names() {
    assert_eq!(FieldKind::Hyperlink.as_str(), "HYPERLINK");
    assert_eq!(FieldKind::Page.as_str(), "PAGE");
    assert_eq!(FieldKind::Toc.as_str(), "TOC");
    assert_eq!(FieldKind::Filename.as_str(), "FILENAME");
    assert_eq!(FieldKind::MergeField.as_str(), "MERGEFIELD");
    assert_eq!(FieldKind::Ref.as_str(), "REF");
    assert_eq!(FieldKind::PageRef.as_str(), "PAGEREF");
    assert_eq!(FieldKind::NoteRef.as_str(), "NOTEREF");
    assert_eq!(FieldKind::TocEntry.as_str(), "TC");
    assert_eq!(FieldKind::Sequence.as_str(), "SEQ");
    assert_eq!(FieldKind::DocumentInfo("DATE".to_string()).as_str(), "DATE");
    assert_eq!(
        FieldKind::from_instruction("CATEGORY"),
        FieldKind::DocumentInfo("CATEGORY".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("CONTENTSTATUS"),
        FieldKind::DocumentInfo("CONTENTSTATUS".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("VERSION"),
        FieldKind::DocumentInfo("VERSION".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("CREATOR"),
        FieldKind::DocumentInfo("CREATOR".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("DESCRIPTION"),
        FieldKind::DocumentInfo("DESCRIPTION".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("KEYWORD"),
        FieldKind::DocumentInfo("KEYWORD".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("LASTMODIFIEDBY"),
        FieldKind::DocumentInfo("LASTMODIFIEDBY".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("APPLICATION"),
        FieldKind::DocumentInfo("APPLICATION".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("APPVERSION"),
        FieldKind::DocumentInfo("APPVERSION".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("MANAGER"),
        FieldKind::DocumentInfo("MANAGER".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("COMPANY"),
        FieldKind::DocumentInfo("COMPANY".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("HYPERLINKBASE"),
        FieldKind::DocumentInfo("HYPERLINKBASE".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("DOCSECURITY"),
        FieldKind::DocumentInfo("DOCSECURITY".to_string())
    );
    assert_eq!(
        FieldKind::from_instruction("LINKSUPTODATE"),
        FieldKind::DocumentInfo("LINKSUPTODATE".to_string())
    );
    assert_eq!(FieldKind::Dynamic("IF".to_string()).as_str(), "IF");
    assert_eq!(
        FieldKind::InsertedContent("INCLUDETEXT".to_string()).as_str(),
        "INCLUDETEXT"
    );
    assert_eq!(
        FieldKind::MailMerge("ADDRESSBLOCK".to_string()).as_str(),
        "ADDRESSBLOCK"
    );
    assert_eq!(
        FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string()).as_str(),
        "BIBLIOGRAPHY"
    );
    assert_eq!(
        FieldKind::Numbering("AUTONUM".to_string()).as_str(),
        "AUTONUM"
    );
    assert_eq!(
        FieldKind::DocumentStructure("SECTION".to_string()).as_str(),
        "SECTION"
    );
    assert_eq!(FieldKind::Display("EQ".to_string()).as_str(), "EQ");
    assert_eq!(
        FieldKind::Action("MACROBUTTON".to_string()).as_str(),
        "MACROBUTTON"
    );
    assert_eq!(
        FieldKind::Compatibility("PRIVATE".to_string()).as_str(),
        "PRIVATE"
    );
    assert_eq!(
        FieldKind::Barcode("DISPLAYBARCODE".to_string()).as_str(),
        "DISPLAYBARCODE"
    );
    assert_eq!(
        FieldKind::FormField("FORMTEXT".to_string()).as_str(),
        "FORMTEXT"
    );
    assert_eq!(FieldKind::Unknown("custom".to_string()).as_str(), "custom");
}

#[test]
fn set_field_result_updates_simple_and_complex_cached_results() {
    let mut doc = Document::open(&field_docx()).expect("fixture opens");

    doc.set_field_result(0, "7").expect("update PAGE result");
    doc.set_field_result(5, "renamed.docx")
        .expect("update FILENAME result");

    let saved = doc.save().expect("save edited docx");
    let mut reopened = Document::open(&saved).expect("reopen edited docx");
    let fields = reopened.fields();

    assert_eq!(fields[0].kind, FieldKind::Page);
    assert_eq!(fields[0].instruction, "PAGE");
    assert_eq!(fields[0].result, "7");
    assert_eq!(fields[5].kind, FieldKind::Filename);
    assert_eq!(fields[5].instruction, "FILENAME \\p");
    assert_eq!(fields[5].result, "renamed.docx");
    assert!(reopened.set_field_result(6, "missing").is_err());
}

#[test]
fn set_field_result_skips_deleted_body_fields() {
    let fixture = docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:del w:id="1"><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:delText>deleted page</w:delText></w:r></w:fldSimple></w:p></w:del><w:moveFrom w:id="2"><w:p><w:fldSimple w:instr=" CUSTOM moved "><w:r><w:delText>moved page</w:delText></w:r></w:fldSimple></w:p></w:moveFrom><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ]);
    let mut doc = Document::open(&fixture).expect("fixture opens");

    assert_eq!(doc.fields().len(), 1);
    doc.set_field_result(0, "7")
        .expect("visible field result updated");

    let saved = doc.save().expect("save edited docx");
    let body = String::from_utf8(unzip_parts(&saved)["word/document.xml"].clone()).unwrap();
    assert!(
        body.contains("<w:delText>deleted page</w:delText>"),
        "{body}"
    );
    assert!(body.contains("<w:delText>moved page</w:delText>"), "{body}");

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let fields = reopened.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].result, "7");
}

#[test]
fn set_field_result_uses_fields_order_for_nested_complex_fields() {
    let mut doc = Document::open(&nested_complex_field_docx()).expect("fixture opens");

    doc.set_field_result(1, "Outer Updated")
        .expect("update outer nested field result");

    let saved = doc.save().expect("save edited docx");
    let reopened = Document::open(&saved).expect("reopen edited docx");
    let fields = reopened.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[1].instruction, "CUSTOM outer");
    assert_eq!(fields[1].result, "Outer Updated");
}

#[test]
fn set_field_result_rejects_note_field_indexes_without_mutation() {
    let fixture = note_body_field_docx();
    let before = unzip_parts(&fixture);
    let mut doc = Document::open(&fixture).expect("fixture opens");

    assert_eq!(doc.fields().len(), 2);
    let err = doc
        .set_field_result(0, "changed.docx")
        .expect_err("note field index rejected");

    assert!(
        err.to_string().contains("editable body field range"),
        "{err}"
    );
    assert!(doc.edited_parts().is_empty());

    let after = unzip_parts(&doc.save().expect("save rejected edit"));
    assert_eq!(
        before.get("word/footnotes.xml"),
        after.get("word/footnotes.xml"),
        "rejected note field edit should not mutate footnotes.xml"
    );
    assert_eq!(
        before.get("word/endnotes.xml"),
        after.get("word/endnotes.xml"),
        "rejected note field edit should not mutate endnotes.xml"
    );
}

#[test]
fn set_field_result_writes_tabs_and_breaks_as_markers() {
    let mut doc = Document::open(&field_docx()).expect("fixture opens");

    doc.set_field_result(0, "Line 1\nLine\t2")
        .expect("update simple field result");
    doc.set_field_result(5, "Path\nName\t2")
        .expect("update complex field result");

    let saved = doc.save().expect("save edited docx");
    let parts = unzip_parts(&saved);
    let body = String::from_utf8(parts["word/document.xml"].clone()).unwrap();

    assert!(
        body.contains("<w:t>Line 1</w:t><w:br/><w:t>Line</w:t><w:tab/><w:t>2</w:t>"),
        "simple field result did not use WML markers: {body}"
    );
    assert!(
        body.contains("<w:t>Path</w:t><w:br/><w:t>Name</w:t><w:tab/><w:t>2</w:t>"),
        "complex field result did not use WML markers: {body}"
    );

    let reopened = Document::open(&saved).expect("reopen edited docx");
    let fields = reopened.fields();
    assert_eq!(fields[0].result, "Line 1\nLine\t2");
    assert_eq!(fields[5].result, "Path\nName\t2");
}
