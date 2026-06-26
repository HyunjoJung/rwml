use rdoc::{
    CoreProperties, Document, DocumentFormat, DocumentWarning, EditReadOnlyReason,
    FieldEvaluationReason, FieldEvaluationReasonCount, FieldKind, FieldKindCount, MetafileFormat,
};
use std::io::Write;

#[test]
fn blank_docx_report_exposes_format_and_stats() {
    let doc = Document::new();
    let report = doc.report();

    assert_eq!(report.format, DocumentFormat::Docx);
    assert_eq!(report.stats, doc.model().meta.stats);
    assert!(report.edit.package_preserving);
    assert!(report.edit.read_only_reasons.is_empty());
    assert_eq!(doc.edit_capability(), report.edit);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

#[cfg(feature = "docx")]
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

#[cfg(feature = "docx")]
fn docx_fixture_bytes(parts: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let opt = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            zip.start_file(name, opt).unwrap();
            zip.write_all(&body).unwrap();
        }
        zip.finish().unwrap();
    }
    out
}

#[cfg(feature = "docx")]
fn put_u16le(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "docx")]
fn put_i16le(out: &mut [u8], offset: usize, value: i16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "docx")]
fn put_u32le(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "docx")]
fn put_i32le(out: &mut [u8], offset: usize, value: i32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

#[cfg(feature = "docx")]
fn sample_emf(width: i32, height: i32) -> Vec<u8> {
    let mut bytes = vec![0u8; 88];
    put_u32le(&mut bytes, 0, 1); // EMR_HEADER
    put_u32le(&mut bytes, 4, 88);
    put_i32le(&mut bytes, 8, 0);
    put_i32le(&mut bytes, 12, 0);
    put_i32le(&mut bytes, 16, width);
    put_i32le(&mut bytes, 20, height);
    bytes[40..44].copy_from_slice(b" EMF");
    bytes
}

#[cfg(feature = "docx")]
fn sample_placeable_wmf(width_units: i16, height_units: i16, units_per_inch: u16) -> Vec<u8> {
    let mut bytes = vec![0u8; 40];
    put_u32le(&mut bytes, 0, 0x9AC6CDD7);
    put_i16le(&mut bytes, 6, 0);
    put_i16le(&mut bytes, 8, 0);
    put_i16le(&mut bytes, 10, width_units);
    put_i16le(&mut bytes, 12, height_units);
    put_u16le(&mut bytes, 14, units_per_inch);
    put_u16le(&mut bytes, 22, 1); // standard WMF header follows placeable header
    bytes
}

#[cfg(feature = "docx")]
fn sample_compressed_emf() -> Vec<u8> {
    vec![
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0x63, 0x64, 0x60, 0x60, 0x88,
        0x60, 0x40, 0x00, 0x07, 0x46, 0x06, 0x86, 0x0f, 0x0c, 0x98, 0x40, 0xc1, 0xd5, 0xd7, 0x8d,
        0x81, 0x04, 0x00, 0x00, 0xa4, 0x29, 0x32, 0xcb, 0x58, 0x00, 0x00, 0x00,
    ]
}

#[cfg(feature = "docx")]
fn sample_compressed_placeable_wmf() -> Vec<u8> {
    vec![
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02, 0xff, 0xbb, 0x7e, 0xf6, 0xd8, 0x2c,
        0x06, 0x30, 0x70, 0xe0, 0x5e, 0xc0, 0xba, 0x80, 0x15, 0xc2, 0x66, 0x64, 0x40, 0x07, 0x00,
        0x39, 0x10, 0x03, 0xa4, 0x28, 0x00, 0x00, 0x00,
    ]
}

#[cfg(feature = "docx")]
fn complex_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="emf" ContentType="image/x-emf"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/comments.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/comments" Target="comments.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart1.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pic1.emf"/><Relationship Id="rIdLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/" TargetMode="External"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:p><w:commentRangeStart w:id="0"/><w:r><w:t>Hello</w:t></w:r><w:commentRangeEnd w:id="0"/><w:r><w:commentReference w:id="0"/></w:r></w:p><w:p><w:hyperlink r:id="rIdLink"><w:r><w:t>Link</w:t></w:r></w:hyperlink></w:p><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><w:ins><w:r><w:t>new</w:t></w:r></w:ins><w:del><w:r><w:delText>old</w:delText></w:r></w:del><w:moveFrom><w:r><w:delText>moved</w:delText></w:r></w:moveFrom><w:p><w:pPr><w:pPrChange w:id="9" w:author="Reviewer"><w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange></w:pPr><w:r><w:t>changed props</w:t></w:r></w:p><w:sdt><w:sdtContent><w:p><w:r><w:t>control</w:t></w:r></w:p></w:sdtContent></w:sdt><w:tbl><w:tr><w:tc><w:p><w:r><w:t>outer</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>inner</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl><w:r><w:drawing><wp:anchor/></w:drawing></w:r><w:r><w:object/></w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice></mc:AlternateContent></w:body></w:document>"#,
        ),
        (
            "word/comments.xml",
            r#"<w:comments xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:comment w:id="0"><w:p><w:r><w:t>note</w:t></w:r></w:p></w:comment></w:comments>"#,
        ),
        (
            "word/charts/chart1.xml",
            r#"<c:chartSpace xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><c:chart/></c:chartSpace>"#,
        ),
        ("word/media/pic1.emf", "fake"),
    ])
}

#[cfg(feature = "docx")]
fn metafile_docx() -> Vec<u8> {
    docx_fixture_bytes(vec![
        (
            "[Content_Types].xml",
            br#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="emf" ContentType="image/x-emf"/><Default Extension="wmf" ContentType="image/x-wmf"/><Default Extension="emz" ContentType="image/x-emz"/><Default Extension="wmz" ContentType="image/x-wmz"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#.to_vec(),
        ),
        (
            "_rels/.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#.to_vec(),
        ),
        (
            "word/_rels/document.xml.rels",
            br#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdEmf" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pic1.emf"/><Relationship Id="rIdWmf" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pic2.wmf"/><Relationship Id="rIdEmz" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pic3.emz"/><Relationship Id="rIdWmz" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/pic4.wmz"/></Relationships>"#.to_vec(),
        ),
        (
            "word/document.xml",
            br#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>metafiles</w:t></w:r></w:p></w:body></w:document>"#.to_vec(),
        ),
        ("word/media/pic1.emf", sample_emf(640, 480)),
        (
            "word/media/pic2.wmf",
            sample_placeable_wmf(1440, 720, 1440),
        ),
        ("word/media/pic3.emz", sample_compressed_emf()),
        ("word/media/pic4.wmz", sample_compressed_placeable_wmf()),
    ])
}

#[cfg(feature = "docx")]
fn lossy_metadata_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn editable_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>OLD</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn core_properties_docx() -> Vec<u8> {
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
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY</w:t></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "docProps/core.xml",
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Quarter &lt;One&gt; &amp; Co</dc:title><dc:creator>Analyst</dc:creator><cp:category>Operations</cp:category><cp:contentStatus>Draft</cp:contentStatus><cp:version>1.2</cp:version></cp:coreProperties>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILENAME \p "><w:r><w:t>report.docx</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com&quot; "><w:r><w:t>Example</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-3&quot; "><w:r><w:t>Contents</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>Figure 1</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>custom</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn merge_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" MERGEFIELD client-name \* MERGEFORMAT "><w:r><w:t>Acme</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MERGEFIELD &quot;project-name&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Roadmap</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF MissingBookmark \h "><w:r><w:t>missing page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_manual_break_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> PAGEREF &quot;Figure1&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>old page</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* CardText "><w:r><w:t>stale cardtext</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* OrdText "><w:r><w:t>stale ordtext</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_rendered_break_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_rendered_then_manual_break_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Page one text.</w:t></w:r></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t><w:br w:type="page"/></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="PageThree"/><w:r><w:t>Page three target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF PageThree \h "><w:r><w:t>99</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF PageThree \p "><w:r><w:t>old relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_section_break_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="NextSection"/><w:r><w:t>Next section target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page three lead.</w:t></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val="evenPage"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="8" w:name="EvenSection"/><w:r><w:t>Even section target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:pPr><w:sectPr><w:type w:val="oddPage"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="9" w:name="OddSection"/><w:r><w:t>Odd section target</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF NextSection \h "><w:r><w:t>stale next</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF EvenSection \* roman "><w:r><w:t>stale even</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF OddSection \p "><w:r><w:t>stale odd relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_default_section_break_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_page_break_before_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:bookmarkStart w:id="7" w:name="BreakBefore"/><w:r><w:t>Break-before target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Page two lead.</w:t></w:r></w:p><w:p><w:pPr><w:pageBreakBefore/></w:pPr><w:bookmarkStart w:id="8" w:name="RenderedBreakBefore"/><w:r><w:t>Rendered break-before target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF BreakBefore \h "><w:r><w:t>stale break-before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RenderedBreakBefore \* Ordinal "><w:r><w:t>stale rendered break-before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RenderedBreakBefore \p "><w:r><w:t>stale relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_section_page_number_restart_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:br w:type="page"/></w:r></w:p><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="7"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="7" w:name="Restarted"/><w:r><w:t>Restarted target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Restarted next page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="8" w:name="RestartedNext"/><w:r><w:t>Restarted next target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Restarted \h "><w:r><w:t>stale restart</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedNext \* ROMAN "><w:r><w:t>stale restart roman</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF RestartedNext \p "><w:r><w:t>stale restart relative</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_decimal_zero_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="4" w:fmt="decimalZero"/></w:sectPr></w:pPr></w:p><w:p><w:r><w:lastRenderedPageBreak/><w:t>Decimal zero page lead.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="8" w:name="DecimalZeroSection"/><w:r><w:t>Decimal zero target</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF DecimalZeroSection \h "><w:r><w:t>stale decimal zero</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DecimalZeroSection \* Arabic "><w:r><w:t>stale decimal arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_visible_manual_break_before_rendered_hint_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_rendered_break_page_one_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_deleted_rendered_break_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn ref_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="9" w:name="_Ref123456789"/><w:r><w:t>Table 2</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>stale cached text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF _Ref123456789 "><w:r><w:t>stale hidden ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \f "><w:r><w:t>note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \d- "><w:r><w:t>sequence separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingNote \f "><w:r><w:t>missing note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingBookmark "><w:r><w:t>Missing</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn malformed_ref_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" REF &quot;MissingBookmark "><w:r><w:t>cached malformed ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn note_ref_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="8" w:name="PlainText"/><w:r><w:t>Not a note mark</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne "><w:r><w:t>stale note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \p "><w:r><w:t>stale relative note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF PlainText "><w:r><w:t>plain bookmark note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF MissingNote "><w:r><w:t>missing note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \x "><w:r><w:t>unsupported note switch</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn direct_bookmark_ref_field_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn direct_bookmark_ref_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>figure one</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" Figure1 \* Upper "><w:r><w:t>stale direct upper</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \*FirstCap "><w:r><w:t>stale direct first-cap</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \h "><w:r><w:t>stale direct hyperlink</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \d- "><w:r><w:t>direct sequence separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Figure1 \f "><w:r><w:t>direct note mark</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn direct_relative_ref_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn numbered_ref_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:numPr><w:ilvl w:val="0"/><w:numId w:val="42"/></w:numPr></w:pPr><w:bookmarkStart w:id="7" w:name="Clause"/><w:r><w:t>Numbered clause</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Clause \n "><w:r><w:t>stale number</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Clause \n \p "><w:r><w:t>stale number relative</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" Clause \n "><w:r><w:t>stale direct number</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
        (
            "word/numbering.xml",
            r#"<w:numbering xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:abstractNum w:abstractNumId="9"><w:lvl w:ilvl="0"><w:start w:val="3"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/></w:lvl></w:abstractNum><w:num w:numId="42"><w:abstractNumId w:val="9"/></w:num></w:numbering>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn numbered_ref_suppress_text_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn full_context_ref_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn full_context_ref_suppress_text_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn relative_context_ref_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn ref_text_format_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>figure one</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \* Upper "><w:r><w:t>stale upper ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \* Caps "><w:r><w:t>stale caps ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \*FirstCap "><w:r><w:t>stale first-cap ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingBookmark \*Lower "><w:r><w:t>missing lower ref</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn multi_paragraph_ref_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="11" w:name="ClauseText"/><w:r><w:t>First paragraph.</w:t></w:r></w:p><w:p><w:r><w:t>Second paragraph.</w:t></w:r><w:bookmarkEnd w:id="11"/></w:p><w:p><w:fldSimple w:instr=" REF ClauseText "><w:r><w:t>stale multi ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingBookmark "><w:r><w:t>Missing</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; "><w:r><w:t>stale toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>custom</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_neutral_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \h \z \w \x "><w:r><w:t>stale neutral toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_tc_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" TC &quot;Manual Entry&quot; \f m \l 2 "/></w:p><w:p><w:fldSimple w:instr=" TOC \f m "><w:r><w:t>stale tc toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_sequence_caption_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Figure </w:t></w:r><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>1</w:t></w:r></w:fldSimple><w:r><w:t>: Mercury</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \c &quot;Figure&quot; "><w:r><w:t>stale figures toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_sequence_caption_text_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>Figure </w:t></w:r><w:fldSimple w:instr=" SEQ Figure "><w:r><w:t>8</w:t></w:r></w:fldSimple><w:r><w:t>: Mercury</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \a Figure "><w:r><w:t>stale caption-text toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_general_format_switch_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn toc_quoted_custom_style_no_result_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:style w:type="paragraph" w:styleId="BodyText"><w:name w:val="Body Text"/></w:style></w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="BodyText"/></w:pPr><w:r><w:t>Body paragraph</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \t &quot;Custom Heading,2&quot; "><w:r><w:t>stale quoted custom toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_no_page_numbers_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \n &quot;1-2&quot; "><w:r><w:t>stale no-page toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_entry_page_separator_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \p &quot;-&quot; "><w:r><w:t>stale separator toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_sequence_page_separator_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \d &quot;-&quot; "><w:r><w:t>stale sequence separator toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_outline_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Style Heading</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Outline Heading</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \u "><w:r><w:t>stale outline toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \t &quot;CustomHeading,1&quot; "><w:r><w:t>stale advanced toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_bookmark_scope_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="ScopedToc"/><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Scoped Heading</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="8" w:name="EmptyScope"/><w:r><w:t>No headings here</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; \b ScopedToc "><w:r><w:t>stale scoped toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \b EmptyScope "><w:r><w:t>stale empty scope toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-1&quot; \b MissingScope "><w:r><w:t>stale missing scope toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_bookmark_only_scope_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="ScopedToc"/><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Scoped Heading</w:t></w:r></w:p><w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Scoped Detail</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" TOC \b ScopedToc "><w:r><w:t>stale bookmark-only toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn toc_outline_without_range_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn bare_toc_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="2"/></w:pPr><w:r><w:t>Mitigation</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC "><w:r><w:t>stale bare toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>custom</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
#[test]
fn report_counts_docx_feature_markers() {
    let doc = Document::open(&complex_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.comments, 1);
    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Page,
            count: 1
        }]
    );
    assert_eq!(report.features.hyperlinks, 1);
    assert_eq!(report.features.tracked_insertions, 1);
    assert_eq!(report.features.tracked_deletions, 1);
    assert_eq!(report.features.tracked_moves, 1);
    assert_eq!(report.features.tracked_property_changes, 1);
    assert_eq!(report.features.content_controls, 1);
    assert_eq!(report.features.nested_tables, 1);
    assert_eq!(report.features.floating_shapes, 2);
    assert_eq!(report.features.charts, 1);
    assert_eq!(report.features.ole_objects, 1);
    assert_eq!(report.features.unsupported_metafiles, 1);
    assert!(report.warnings.iter().any(|warning| matches!(
        warning,
        DocumentWarning::IncompleteRevisionView {
            property_changes: 1
        }
    )));
    assert_eq!(report.warnings.len(), 6);
    assert!(
        report.to_json().contains(r#""nested_tables":1"#),
        "{}",
        report.to_json()
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_counts_alternate_content_floating_shape_once() {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wp:anchor relativeHeight="51"><wp:extent cx="914400" cy="457200"/><wp:docPr id="51" name="Choice report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Choice body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></mc:Choice><mc:Fallback><w:drawing><wp:anchor relativeHeight="52"><wp:extent cx="914400" cy="457200"/><wp:docPr id="52" name="Fallback report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Fallback body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></mc:Fallback></mc:AlternateContent></w:r></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");

    assert_eq!(doc.floating_shapes().len(), 1);
    assert_eq!(doc.report().features.floating_shapes, 1);
}

#[cfg(feature = "docx")]
#[test]
fn report_floating_shapes_follow_accepted_revision_view() {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:body><w:p><w:r><w:drawing><wp:anchor relativeHeight="61"><wp:extent cx="914400" cy="457200"/><wp:docPr id="61" name="Direct report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Direct body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p><w:ins w:id="62" w:author="Editor"><w:p><w:r><w:drawing><wp:anchor relativeHeight="62"><wp:extent cx="914400" cy="457200"/><wp:docPr id="62" name="Inserted report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Inserted body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p></w:ins><w:moveTo w:id="63" w:author="Editor"><w:p><w:r><w:drawing><wp:anchor relativeHeight="63"><wp:extent cx="914400" cy="457200"/><wp:docPr id="63" name="Moved-to report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Moved-to body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p></w:moveTo><w:del w:id="64" w:author="Editor"><w:p><w:r><w:drawing><wp:anchor relativeHeight="64"><wp:extent cx="914400" cy="457200"/><wp:docPr id="64" name="Deleted report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Deleted body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p></w:del><w:moveFrom w:id="65" w:author="Editor"><w:p><w:r><w:drawing><wp:anchor relativeHeight="65"><wp:extent cx="914400" cy="457200"/><wp:docPr id="65" name="Moved-from report float"/><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Moved-from body</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p></w:moveFrom></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(doc.floating_shapes().len(), 3);
    assert_eq!(report.features.floating_shapes, 3);
    assert_eq!(report.features.tracked_insertions, 1);
    assert_eq!(report.features.tracked_deletions, 1);
    assert_eq!(report.features.tracked_moves, 2);
}

#[cfg(feature = "docx")]
#[test]
fn report_omits_old_only_alternate_content_floating_shape_marker() {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"><w:body><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice></mc:AlternateContent></w:r></w:p><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:del w:id="70" w:author="Editor"><w:r><w:drawing/></w:r></w:del></mc:Choice></mc:AlternateContent></w:r></w:p></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.floating_shapes, 1);
    assert_eq!(report.features.tracked_deletions, 1);
}

#[cfg(feature = "docx")]
#[test]
fn report_exposes_metafile_metadata() {
    let doc = Document::open(&metafile_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.unsupported_metafiles, 4);
    assert_eq!(report.features.metafiles.len(), 4);

    let emf = report
        .features
        .metafiles
        .iter()
        .find(|info| info.path == "word/media/pic1.emf")
        .expect("EMF metadata");
    assert_eq!(emf.format, MetafileFormat::Emf);
    assert!(!emf.compressed);
    assert_eq!(emf.bytes, 88);
    assert_eq!(emf.width_px, Some(640));
    assert_eq!(emf.height_px, Some(480));

    let wmf = report
        .features
        .metafiles
        .iter()
        .find(|info| info.path == "word/media/pic2.wmf")
        .expect("WMF metadata");
    assert_eq!(wmf.format, MetafileFormat::Wmf);
    assert!(!wmf.compressed);
    assert_eq!(wmf.width_px, Some(96));
    assert_eq!(wmf.height_px, Some(48));

    let emz = report
        .features
        .metafiles
        .iter()
        .find(|info| info.path == "word/media/pic3.emz")
        .expect("compressed EMF metadata");
    assert_eq!(emz.format, MetafileFormat::Emf);
    assert!(emz.compressed);
    assert_eq!(emz.bytes, 42);
    assert_eq!(emz.width_px, Some(320));
    assert_eq!(emz.height_px, Some(240));

    let wmz = report
        .features
        .metafiles
        .iter()
        .find(|info| info.path == "word/media/pic4.wmz")
        .expect("compressed WMF metadata");
    assert_eq!(wmz.format, MetafileFormat::Wmf);
    assert!(wmz.compressed);
    assert_eq!(wmz.bytes, 38);
    assert_eq!(wmz.width_px, Some(192));
    assert_eq!(wmz.height_px, Some(96));

    let json = report.to_json();
    assert!(
        json.contains(r#""metafiles":[{"path":"word/media/pic1.emf","format":"EMF","bytes":88,"compressed":false,"width_px":640,"height_px":480}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"path":"word/media/pic2.wmf","format":"WMF","bytes":40,"compressed":false,"width_px":96,"height_px":48}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"path":"word/media/pic3.emz","format":"EMF","bytes":42,"compressed":true,"width_px":320,"height_px":240}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"path":"word/media/pic4.wmz","format":"WMF","bytes":38,"compressed":true,"width_px":192,"height_px":96}"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_field_evaluation_warning_only_lists_unsupported_kinds() {
    let doc = Document::open(&field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 6);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Page,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Filename,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Hyperlink,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Ref,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Unknown("CUSTOM".to_string()),
                count: 1,
            },
        ]
    );

    let warning = report
        .warnings
        .iter()
        .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. }))
        .expect("unsupported field warning");
    assert_eq!(
        warning,
        &DocumentWarning::UnsupportedFieldEvaluation {
            count: 3,
            field_kinds: vec![
                FieldKindCount {
                    kind: FieldKind::Toc,
                    count: 1,
                },
                FieldKindCount {
                    kind: FieldKind::Ref,
                    count: 1,
                },
                FieldKindCount {
                    kind: FieldKind::Unknown("CUSTOM".to_string()),
                    count: 1,
                },
            ],
        }
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnknownField,
                count: 1,
            },
        ]
    );

    let json = report.to_json();
    assert!(json.contains(r#""field_kinds":[{"kind":"PAGE","count":1},{"kind":"FILENAME","count":1},{"kind":"HYPERLINK","count":1},{"kind":"TOC","count":1},{"kind":"REF","count":1},{"kind":"CUSTOM","count":1}]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":1},{"reason":"UnknownField","count":1}]"#), "{json}");
    assert!(json.contains(r#""kind":"UnsupportedFieldEvaluation","count":3,"field_kinds":[{"kind":"TOC","count":1},{"kind":"REF","count":1},{"kind":"CUSTOM","count":1}]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_treats_merge_fields_as_supported_cached_display_fields() {
    let doc = Document::open(&merge_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::MergeField,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "cached MERGEFIELD results should not produce unsupported evaluation warnings: {:?}",
        report.warnings
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"MERGEFIELD","count":2}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_field_warning_is_named_unsupported_evaluation() {
    let doc = Document::open(&page_ref_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
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
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 1,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 2,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::PageRef,
                count: 2,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGEREF","count":2}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"PAGEREF","count":2}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":1}]"#),
        "{json}"
    );
    assert!(!json.contains(r#""reason":"UnknownField""#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_manual_break_targets_are_supported_when_unambiguous() {
    let doc = Document::open(&page_ref_manual_break_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 5);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 5,
        }]
    );
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
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 1,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::PageRef,
                count: 1,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGEREF","count":5}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"PAGEREF","count":1}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_rendered_break_targets_are_supported_when_source_hints_exist() {
    let doc = Document::open(&page_ref_rendered_break_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGEREF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_rendered_context_manual_break_targets_are_supported() {
    let doc = Document::open(&page_ref_rendered_then_manual_break_diagnostics_docx())
        .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "computed rendered-context hard-break PAGEREF should not warn: {:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_section_break_targets_are_supported_when_structural() {
    let doc = Document::open(&page_ref_section_break_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("iv"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let report = doc.report();

    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGEREF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_default_section_break_target_is_supported() {
    let doc =
        Document::open(&page_ref_default_section_break_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));

    let report = doc.report();
    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_page_break_before_targets_are_supported_when_structural() {
    let doc =
        Document::open(&page_ref_page_break_before_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("4th"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let report = doc.report();
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_section_page_number_restart_targets_are_supported() {
    let doc = Document::open(&page_ref_section_page_number_restart_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("7"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("VIII"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));

    let report = doc.report();
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_decimal_zero_section_page_number_format_is_supported() {
    let doc = Document::open(&page_ref_decimal_zero_section_page_number_format_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("05"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("5"));

    let report = doc.report();
    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        report
            .warnings
            .iter()
            .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "{:?}",
        report.warnings
    );

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_ambiguous_pre_marker_manual_break_remains_unsupported() {
    let doc =
        Document::open(&page_ref_visible_manual_break_before_rendered_hint_diagnostics_docx())
            .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
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
    assert!(
        report
            .warnings
            .iter()
            .any(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "ambiguous pre-marker hard-break PAGEREF should remain unsupported: {:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_rendered_break_page_one_targets_are_supported() {
    let doc = Document::open(&page_ref_rendered_break_page_one_diagnostics_docx())
        .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        "computed rendered page-one PAGEREF should not warn: {:?}",
        report.warnings
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_deleted_rendered_break_marker_remains_unsupported() {
    let doc =
        Document::open(&page_ref_deleted_rendered_break_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
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

#[cfg(feature = "docx")]
#[test]
fn report_direct_bookmark_field_counts_as_supported_ref_when_bookmark_exists() {
    let doc = Document::open(&direct_bookmark_ref_field_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Ref);
    assert_eq!(fields[0].computed_result.as_deref(), Some("Figure 1"));
    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":1}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_direct_bookmark_field_switches_count_as_supported_refs() {
    let doc =
        Document::open(&direct_bookmark_ref_switch_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 5);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![
            Some("FIGURE ONE"),
            Some("Figure one"),
            Some("figure one"),
            None,
            None
        ]
    );
    assert_eq!(report.features.fields, 5);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 5,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
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
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 2,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Ref,
                count: 2,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":5}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"REF","count":2}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1},{"reason":"UnsupportedSwitch","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_direct_bookmark_p_fields_count_as_supported_refs() {
    let doc =
        Document::open(&direct_relative_ref_switch_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("below"), Some("above")]
    );
    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":2}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_n_fields_count_as_supported_refs() {
    let doc = Document::open(&numbered_ref_switch_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("3"), Some("3 above"), Some("3")]
    );
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_n_t_fields_count_as_supported_refs() {
    let doc = Document::open(&numbered_ref_suppress_text_switch_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("1.01"), Some("1.01 above"), Some("1.01")]
    );
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_w_fields_count_as_supported_refs() {
    let doc = Document::open(&full_context_ref_switch_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("1.a.i"), Some("1.a.i above"), Some("1.a.i")]
    );
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_w_t_fields_count_as_supported_refs() {
    let doc = Document::open(&full_context_ref_suppress_text_switch_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("1.a.i"), Some("1.a.i above"), Some("1.a.i")]
    );
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_r_fields_count_as_supported_refs() {
    let doc =
        Document::open(&relative_context_ref_switch_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    let report = doc.report();

    assert_eq!(fields.len(), 3);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert_eq!(
        fields
            .iter()
            .map(|field| field.computed_result.as_deref())
            .collect::<Vec<_>>(),
        vec![Some("5.2"), Some("5.2 below"), Some("5.2")]
    );
    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        None
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_field_warning_ignores_computed_bookmark_refs() {
    let doc = Document::open(&ref_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 7);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 7,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 4,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::NoComputedResult,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 2,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 4,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Ref,
                count: 4,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":7}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"REF","count":4}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"UnsupportedSwitch","count":1},{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":2}]"#),
        "{json}"
    );
    assert!(json.contains(r#""kind":"UnsupportedFieldEvaluation","count":4,"field_kinds":[{"kind":"REF","count":4}]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_malformed_ref_targets_are_unsupported_switches() {
    let doc = Document::open(&malformed_ref_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
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

#[cfg(feature = "docx")]
#[test]
fn report_note_ref_field_warning_tracks_unresolved_and_unsupported_cases() {
    let doc = Document::open(&note_ref_field_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 5);
    assert!(fields.iter().all(|field| field.kind == FieldKind::NoteRef));
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[2].computed_result, None);
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].computed_result, None);

    let report = doc.report();

    assert_eq!(report.features.fields, 5);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::NoteRef,
            count: 5,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::NoteRef,
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
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 1,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 3,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::NoteRef,
                count: 3,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"NOTEREF","count":5}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"NOTEREF","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":1},{"reason":"UnsupportedSwitch","count":1}]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_field_warning_ignores_supported_text_format_switch_refs() {
    let doc = Document::open(&ref_text_format_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 5);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 5,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnresolvedBookmark,
            count: 1,
        }]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 1,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Ref,
                count: 1,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"UnresolvedBookmark","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_field_warning_ignores_multi_paragraph_bookmark_refs() {
    let doc = Document::open(&multi_paragraph_ref_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnresolvedBookmark,
            count: 1,
        }]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 1,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Ref,
                count: 1,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"REF","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_computed_heading_tocs() {
    let doc = Document::open(&toc_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 2,
            },
            FieldKindCount {
                kind: FieldKind::Unknown("CUSTOM".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Unknown("CUSTOM".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnknownField,
            count: 1,
        }]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 1,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Unknown("CUSTOM".to_string()),
                count: 1,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"CUSTOM","count":1}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"UnknownField","count":1}]"#),
        "{json}"
    );
    assert!(json.contains(r#""kind":"UnsupportedFieldEvaluation","count":1,"field_kinds":[{"kind":"CUSTOM","count":1}]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_bare_toc_field_warning_ignores_computed_default_tocs() {
    let doc = Document::open(&bare_toc_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Unknown("CUSTOM".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Unknown("CUSTOM".to_string()),
            count: 1,
        }]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 1,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Unknown("CUSTOM".to_string()),
                count: 1,
            }],
        })
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_neutral_switch_tocs() {
    let doc = Document::open(&toc_neutral_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_general_format_switch_tocs() {
    let doc = Document::open(&toc_general_format_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 3,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_with_quoted_custom_style_switch_reports_no_computed_result() {
    let doc = Document::open(&toc_quoted_custom_style_no_result_diagnostics_docx())
        .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 1,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
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

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_no_page_number_switch_tocs() {
    let doc =
        Document::open(&toc_no_page_numbers_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_entry_page_separator_switch_tocs() {
    let doc =
        Document::open(&toc_entry_page_separator_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_sequence_page_separator_switch_tocs() {
    let doc = Document::open(&toc_sequence_page_separator_switch_diagnostics_docx())
        .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_outline_switch_tocs() {
    let doc = Document::open(&toc_outline_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 2,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_supported_tc_entry_tocs() {
    let doc = Document::open(&toc_tc_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::TocEntry,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            },
        ]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_supported_sequence_caption_tocs() {
    let doc = Document::open(&toc_sequence_caption_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Sequence,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            },
        ]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_supported_sequence_caption_text_tocs() {
    let doc = Document::open(&toc_sequence_caption_text_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 2);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Sequence,
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Toc,
                count: 1,
            },
        ]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_resolved_bookmark_scope_tocs() {
    let doc = Document::open(&toc_bookmark_scope_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 3);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 3,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
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
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 1,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 2,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Toc,
                count: 2,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"TOC","count":2}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":1}]"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_bookmark_only_scope_tocs() {
    let doc = Document::open(&toc_bookmark_only_scope_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_toc_field_warning_ignores_range_less_outline_tocs() {
    let doc = Document::open(&toc_outline_without_range_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 1,
        }]
    );
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));

    let json = report.to_json();
    assert!(json.contains(r#""unsupported_field_kinds":[]"#), "{json}");
    assert!(json.contains(r#""unsupported_field_reasons":[]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_includes_core_properties_for_diagnostics_json() {
    let doc = Document::open(&core_properties_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(
        report.core_properties,
        CoreProperties {
            title: Some("Quarter <One> & Co".to_string()),
            creator: Some("Analyst".to_string()),
            category: Some("Operations".to_string()),
            content_status: Some("Draft".to_string()),
            version: Some("1.2".to_string()),
            ..CoreProperties::default()
        }
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""core_properties":{"title":"Quarter <One> & Co""#),
        "{json}"
    );
    assert!(json.contains(r#""subject":null"#), "{json}");
    assert!(json.contains(r#""creator":"Analyst""#), "{json}");
    assert!(json.contains(r#""category":"Operations""#), "{json}");
    assert!(json.contains(r#""content_status":"Draft""#), "{json}");
    assert!(json.contains(r#""version":"1.2""#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_counts_docx_note_and_text_box_records() {
    let doc = Document::open(&docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" xmlns:v="urn:schemas-microsoft-com:vml"><w:body><w:p><w:r><w:t>BODY</w:t></w:r><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r></w:p><w:p><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing><wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>BOX</w:t></w:r></w:p></w:txbxContent></wps:txbx></wps:wsp></w:drawing></mc:Choice><mc:Fallback><w:pict><v:shape><v:textbox><w:txbxContent><w:p><w:r><w:t>BOX</w:t></w:r></w:p></w:txbxContent></v:textbox></v:shape></w:pict></mc:Fallback></mc:AlternateContent></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote><w:footnote w:id="1"><w:p><w:r><w:t>FOOT</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:p><w:r><w:t>END</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.footnotes, 1);
    assert_eq!(report.features.endnotes, 1);
    assert_eq!(report.features.text_boxes, 1);
    let json = report.to_json();
    assert!(json.contains(r#""footnotes":1"#), "{json}");
    assert!(json.contains(r#""endnotes":1"#), "{json}");
    assert!(json.contains(r#""text_boxes":1"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_exposes_package_read_only_reason_for_lossy_opc_metadata() {
    let mut doc = Document::open(&lossy_metadata_docx()).expect("fixture opens");
    let report = doc.report();

    assert!(!report.edit.package_preserving);
    assert_eq!(
        report.edit.read_only_reasons,
        vec![EditReadOnlyReason::LossyOpcMetadata]
    );
    assert_eq!(doc.edit_capability(), report.edit);
    assert!(report.warnings.iter().any(|warning| matches!(
        warning,
        DocumentWarning::PackageReadOnly { reasons }
            if reasons == &vec![EditReadOnlyReason::LossyOpcMetadata]
    )));
    assert!(
        doc.replace_body_text("OLD", "NEW").is_err(),
        "lossy OPC metadata should make preservation editing read-only"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_can_be_serialized_as_compact_json() {
    let doc = Document::open(&complex_docx()).expect("fixture opens");
    let json = doc.report().to_json();

    assert!(json.starts_with(r#"{"format":"docx","#), "{json}");
    assert!(
        json.contains(r#""edit":{"package_preserving":true,"read_only_reasons":[]}"#),
        "{json}"
    );
    assert!(json.contains(r#""comments":1"#), "{json}");
    assert!(json.contains(r#""tracked_insertions":1"#), "{json}");
    assert!(json.contains(r#""tracked_property_changes":1"#), "{json}");
    assert!(json.contains(r#""fields":1"#), "{json}");
    assert!(json.contains(r#""hyperlinks":1"#), "{json}");
    assert!(
        json.contains(r#""field_kinds":[{"kind":"PAGE","count":1}]"#),
        "{json}"
    );
    assert!(
        !json.contains(r#""kind":"UnsupportedFieldEvaluation""#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"kind":"TrackedChangesPresent","insertions":1,"deletions":1,"moves":1}"#),
        "{json}"
    );
    assert!(
        json.contains(r#"{"kind":"IncompleteRevisionView","property_changes":1}"#),
        "{json}"
    );
    assert!(json.ends_with("]}"), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn read_only_edit_capability_is_serialized_as_compact_json() {
    let doc = Document::open(&lossy_metadata_docx()).expect("fixture opens");
    let json = doc.report().to_json();

    assert!(
        json.contains(
            r#""edit":{"package_preserving":false,"read_only_reasons":["lossy_opc_metadata"]}"#
        ),
        "{json}"
    );
    assert!(
        json.contains(r#"{"kind":"PackageReadOnly","reasons":["lossy_opc_metadata"]}"#),
        "{json}"
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_includes_edited_parts_after_preservation_edit() {
    let mut doc = Document::open(&editable_docx()).expect("fixture opens");

    assert_eq!(doc.replace_body_text("OLD", "NEW").unwrap(), 1);

    let report = doc.report();
    assert_eq!(report.edited_parts, vec!["word/document.xml"]);
    assert_eq!(report.edited_parts, doc.edited_parts());
    assert!(
        report
            .to_json()
            .contains(r#""edited_parts":["word/document.xml"]"#),
        "{}",
        report.to_json()
    );
}
