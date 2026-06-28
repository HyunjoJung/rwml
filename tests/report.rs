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
fn field_kind_count(kind: FieldKind, count: usize) -> FieldKindCount {
    FieldKindCount { kind, count }
}

#[cfg(feature = "docx")]
fn field_reason_count(reason: FieldEvaluationReason, count: usize) -> FieldEvaluationReasonCount {
    FieldEvaluationReasonCount { reason, count }
}

#[cfg(feature = "docx")]
fn assert_report_field_diagnostics(
    fixture: Vec<u8>,
    field_count: usize,
    field_kinds: Vec<FieldKindCount>,
    unsupported_field_kinds: Vec<FieldKindCount>,
    unsupported_field_reasons: Vec<FieldEvaluationReasonCount>,
) {
    let doc = Document::open(&fixture).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, field_count);
    assert_eq!(report.features.field_kinds, field_kinds);
    assert_eq!(
        report.features.unsupported_field_kinds,
        unsupported_field_kinds
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        unsupported_field_reasons
    );
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
            r#"<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/"><dc:title>Quarter &lt;One&gt; &amp; Co</dc:title><dc:subject>Pipeline</dc:subject><dc:creator>Analyst</dc:creator><dc:description>Diagnostics summary</dc:description><cp:keywords>rdoc,metadata</cp:keywords><cp:category>Operations</cp:category><cp:contentStatus>Draft</cp:contentStatus><cp:lastModifiedBy>Reviewer</cp:lastModifiedBy><dcterms:created>2026-06-01T02:03:04Z</dcterms:created><dcterms:modified>2026-06-02T03:04:05Z</dcterms:modified><cp:lastPrinted>2026-06-03T04:05:06Z</cp:lastPrinted><cp:revision>12</cp:revision><cp:version>1.2</cp:version></cp:coreProperties>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn custom_properties_docx() -> Vec<u8> {
    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/docProps/custom.xml" ContentType="application/vnd.openxmlformats-officedocument.custom-properties+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rIdCustom" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/custom-properties" Target="docProps/custom.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY</w:t></w:r></w:p></w:body></w:document>"#,
        ),
        (
            "docProps/custom.xml",
            r#"<Properties xmlns="http://schemas.openxmlformats.org/officeDocument/2006/custom-properties" xmlns:vt="http://schemas.openxmlformats.org/officeDocument/2006/docPropsVTypes"><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="2" name="Client Name"><vt:lpwstr>ACME &lt;Launch&gt;</vt:lpwstr></property><property fmtid="{D5CDD505-2E9C-101B-9397-08002B2CF9AE}" pid="3" name="Phase"><vt:lpwstr>Review &amp; Ship</vt:lpwstr></property></Properties>"#,
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
fn malformed_hyperlink_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com "><w:r><w:t>Cached link</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HYPERLINK \o &quot;tip&quot; "><w:r><w:t>Cached tooltip-only link</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com&quot; extra "><w:r><w:t>Cached trailing link</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn mixed_case_hyperlink_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" hYpErLiNk &quot;https://example.com/mixed&quot; "><w:r><w:t>Mixed link</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_unsupported_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PAGE \* Unknown "><w:r><w:t>cached bad page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn section_pages_unsupported_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SECTIONPAGES \x "><w:r><w:t>cached bad section pages</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn revnum_unsupported_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" REVNUM \x "><w:r><w:t>cached bad revision</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn malformed_filename_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FILENAME \x "><w:r><w:t>cached filename</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn malformed_document_info_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" AUTHOR \* MERGEFORMAT "><w:r><w:t>Cached author</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DOCPROPERTY &quot;Client Name "><w:r><w:t>Cached broken property</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn display_layout_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" ADVANCE \r 2 "><w:r><w:t>offset</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\up8(A)\ai4(B) "><w:r><w:t>cached broader script</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \s\up8(A "><w:r><w:t>cached malformed script</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \d \fo10(A) "><w:r><w:t>cached broader displace</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EQ \d \fo10(A "><w:r><w:t>cached malformed displace</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 65 \f Wingdings "><w:r><w:t>cached unmapped wingdings</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SYMBOL 65 \f &quot;Wingdings "><w:r><w:t>cached malformed symbol</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn action_field_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn compatibility_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PRIVATE legacy-data "><w:r><w:t>cached private payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DATA legacy-data "><w:r><w:t>cached data payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GLOSSARY AutoTextName "><w:r><w:t>cached glossary payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" HTMLACTIVEX LegacyControl "><w:r><w:t>cached activex payload</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ADDIN &quot;bad "><w:r><w:t>cached malformed addin</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn protected_form_field_diagnostics_docx() -> Vec<u8> {
    docx_fixture(&[
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" FORMCHECKBOX "><w:ffData><w:checkBox><w:checked w:val="true"/></w:checkBox></w:ffData><w:r><w:t>cached protected checked</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMDROPDOWN "><w:ffData><w:ddList><w:result w:val="1"/><w:listEntry w:val="First"/><w:listEntry w:val="Second"/></w:ddList></w:ffData><w:r><w:t>cached protected dropdown</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMTEXT "><w:r><w:t>cached protected text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FORMTEXT &quot;bad "><w:r><w:t>cached malformed form text</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn numbering_field_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn dynamic_control_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" IF CustomerTier = &quot;Gold&quot; &quot;ship&quot; &quot;hold&quot; "><w:r><w:t>cached data if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IF 1 = "><w:r><w:t>cached broken if</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE CustomerTier = &quot;Gold&quot; "><w:r><w:t>cached data compare</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" COMPARE \o = &quot;Gold&quot; "><w:r><w:t>cached switch compare</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = CustomerTotal \# &quot;0.00&quot; "><w:r><w:t>cached data formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" = 1 \# &quot;0.00 "><w:r><w:t>cached broken formula</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILLIN &quot;Client?&quot; "><w:r><w:t>cached fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" ASK ClientCode &quot;Client code?&quot; "><w:r><w:t>cached ask</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FILLIN &quot;broken prompt "><w:r><w:t>cached broken fillin</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientName Client 42 "><w:r><w:t>cached ambiguous set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SET ClientName &quot;Acme "><w:r><w:t>cached broken set</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE &quot;literal&quot; "><w:r><w:t>cached quote</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" QUOTE &quot;bad "><w:r><w:t>cached broken quote</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXT "><w:r><w:t>cached next</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" SKIPIF 1 = 0 "><w:r><w:t>cached skipif</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXTIF City = &quot;Tokyo&quot; "><w:r><w:t>cached data nextif</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NEXTIF 1 = "><w:r><w:t>cached broken nextif</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn document_structure_field_diagnostics_docx() -> Vec<u8> {
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
fn malformed_merge_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" MERGEFIELD &quot;client-name "><w:r><w:t>Cached client</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEFIELD \* MERGEFORMAT ClientName "><w:r><w:t>Cached missing name before format</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn split_complex_field_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText> MERGE</w:instrText></w:r><w:r><w:instrText>FIELD &quot;client-name&quot; \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>Acme</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:body></w:document>"#,
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
fn page_ref_text_format_switch_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF Figure1 \* Upper "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF MissingBookmark \* Lower "><w:r><w:t>missing page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_non_current_section_format_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><w:del><w:p><w:pPr><w:sectPr><w:pgNumType w:fmt="bullet"/></w:sectPr></w:pPr></w:p></w:del><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:pPr><w:sectPr><w:pgNumType w:fmt="bullet"/></w:sectPr></w:pPr></w:p></mc:Fallback></mc:AlternateContent><w:p><w:r><w:t>Intro text.</w:t></w:r></w:p><w:p><w:bookmarkStart w:id="7" w:name="CurrentTarget"/><w:r><w:t>Current target</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF CurrentTarget \h "><w:r><w:t>cached current page</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_content_paragraph_section_format_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:pgNumType w:fmt="bullet"/></w:sectPr></w:pPr><w:bookmarkStart w:id="7" w:name="BeforeFormatBreak"/><w:r><w:t>Before format break</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="8" w:name="AfterFormatBreak"/><w:r><w:t>After format break</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF BeforeFormatBreak \p "><w:r><w:t>stale before</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF AfterFormatBreak \p "><w:r><w:t>stale after</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
fn page_ref_number_in_dash_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:sectPr><w:type w:val="nextPage"/><w:pgNumType w:start="5" w:fmt="numberInDash"/></w:sectPr></w:pPr></w:p><w:p><w:bookmarkStart w:id="9" w:name="DashedSection"/><w:r><w:t>Dashed target</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" PAGEREF DashedSection \h "><w:r><w:t>stale dashed</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF DashedSection \* Arabic "><w:r><w:t>stale dashed arabic</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn page_ref_decimal_full_width_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_decimal_enclosed_circle_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_decimal_enclosed_punctuation_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_decimal_width_variant_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_korean_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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

#[cfg(feature = "docx")]
fn page_ref_korean_numeric_section_page_number_format_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="Figure1"/><w:r><w:t>Figure 1</w:t></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="9" w:name="_Ref123456789"/><w:r><w:t>Table 2</w:t></w:r><w:bookmarkEnd w:id="9"/></w:p><w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>stale cached text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF _Ref123456789 "><w:r><w:t>stale hidden ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \p "><w:r><w:t>above</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \f "><w:r><w:t>note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF Figure1 \d- "><w:r><w:t>sequence separator</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingNote \f "><w:r><w:t>missing note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingRomanNote \f \* roman "><w:r><w:t>missing roman note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF MissingBookmark "><w:r><w:t>Missing</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "docx")]
fn ref_non_current_bookmark_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" mc:Ignorable="wps"><w:body><w:del><w:bookmarkStart w:id="10" w:name="DeletedOnly"/><w:r><w:t>old target</w:t></w:r><w:bookmarkEnd w:id="10"/></w:del><mc:AlternateContent><mc:Choice Requires="wps"><w:p/></mc:Choice><mc:Fallback><w:p><w:bookmarkStart w:id="11" w:name="FallbackOnly"/><w:r><w:t>fallback target</w:t></w:r><w:bookmarkEnd w:id="11"/></w:p></mc:Fallback></mc:AlternateContent><w:p><w:fldSimple w:instr=" REF DeletedOnly \f "><w:r><w:t>deleted note mark</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF FallbackOnly \d- "><w:r><w:t>fallback sequence separator</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" REF &quot;MissingBookmark "><w:r><w:t>cached malformed ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" REF &quot;Figure List&quot; "><w:r><w:t>cached whitespace ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" PAGEREF &quot;Figure List&quot; "><w:r><w:t>cached whitespace page ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF &quot;Figure List&quot; "><w:r><w:t>cached whitespace note ref</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \b &quot;Figure List&quot; "><w:r><w:t>cached whitespace toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="7" w:name="FootOne"/><w:r><w:footnoteReference w:id="1"/></w:r><w:bookmarkEnd w:id="7"/></w:p><w:p><w:bookmarkStart w:id="8" w:name="PlainText"/><w:r><w:t>Not a note mark</w:t></w:r><w:bookmarkEnd w:id="8"/></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne "><w:r><w:t>stale note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" FTNREF FootOne "><w:r><w:t>stale legacy note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \p "><w:r><w:t>stale relative note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF PlainText "><w:r><w:t>plain bookmark note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF MissingNote "><w:r><w:t>missing note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF MissingFormattedNote \* Upper "><w:r><w:t>missing formatted note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF MissingRomanNote \* roman "><w:r><w:t>missing roman note</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" NOTEREF FootOne \x "><w:r><w:t>unsupported note switch</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
fn malformed_sequence_diagnostics_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" SEQ Figure \x "><w:r><w:t>bad sequence</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="BodyText"/></w:pPr><w:r><w:t>Body paragraph</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \t &quot;Custom Heading,2&quot; "><w:r><w:t>stale quoted custom toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \s chapter \d &quot;-&quot; "><w:r><w:t>stale sequence prefix toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \c &quot;Figure List&quot; "><w:r><w:t>stale malformed caption toc</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" TOC \t &quot;Custom Heading,2 "><w:r><w:t>stale malformed style toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Executive Summary</w:t></w:r></w:p><w:p><w:pPr><w:outlineLvl w:val="1"/></w:pPr><w:r><w:t>Risks</w:t></w:r></w:p><w:p><w:fldSimple w:instr=" TOC \o &quot;1-2&quot; \s chapter \d &quot;-&quot; "><w:r><w:t>stale sequence separator toc</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
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
fn report_counts_feature_markers_in_docx_story_parts() {
    let doc = Document::open(&docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/><Override PartName="/word/header2.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rIdHeader" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body><w:sdt><w:sdtPr><w:tag w:val="body"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Body</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:r><w:footnoteReference w:id="1"/></w:r><w:r><w:endnoteReference w:id="2"/></w:r></w:p><w:sectPr><w:headerReference w:type="default" r:id="rIdHeader"/></w:sectPr></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="1"><w:sdt><w:sdtPr><w:tag w:val="foot"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Foot</w:t></w:r></w:p></w:sdtContent></w:sdt></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="2"><w:sdt><w:sdtPr><w:tag w:val="end"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>End</w:t></w:r></w:p></w:sdtContent></w:sdt><w:tbl><w:tr><w:tc><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Nested</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote></w:endnotes>"#,
        ),
        (
            "word/header1.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"><w:sdt><w:sdtPr><w:tag w:val="header"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:sdtContent></w:sdt><w:p><w:pPr><w:pPrChange w:id="9"><w:pPr><w:jc w:val="right"/></w:pPr></w:pPrChange></w:pPr><w:r><w:object/><w:drawing><c:chart/></w:drawing></w:r><w:r><mc:AlternateContent><mc:Choice Requires="wps"><w:drawing/></mc:Choice></mc:AlternateContent></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:commentReference w:id="5"/></w:r></w:p></w:hdr>"#,
        ),
        (
            "word/header2.xml",
            r#"<w:hdr xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:sdt><w:sdtPr><w:tag w:val="orphan"/></w:sdtPr><w:sdtContent><w:p><w:r><w:t>Orphan</w:t></w:r></w:p></w:sdtContent></w:sdt></w:hdr>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.content_controls, 4);
    assert_eq!(report.features.nested_tables, 1);
    assert_eq!(report.features.ole_objects, 1);
    assert_eq!(report.features.charts, 1);
    assert_eq!(report.features.floating_shapes, 1);
    assert_eq!(report.features.fields, 1);
    assert_eq!(report.features.tracked_property_changes, 1);
    assert_eq!(report.features.comments, 1);
    assert!(
        report.to_json().contains(r#""content_controls":4"#),
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
fn report_chart_and_ole_markers_follow_accepted_revision_view() {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><w:body><w:p><w:r><w:drawing><c:chart/></w:drawing><w:object/></w:r></w:p><w:del w:id="71" w:author="Editor"><w:r><w:drawing><c:chart/></w:drawing><w:object/></w:r></w:del><w:moveFrom w:id="72" w:author="Editor"><w:r><w:drawing><c:chart/></w:drawing><w:object/></w:r></w:moveFrom></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.charts, 1);
    assert_eq!(report.features.ole_objects, 1);
    assert_eq!(report.features.tracked_deletions, 1);
    assert_eq!(report.features.tracked_moves, 1);
}

#[cfg(feature = "docx")]
#[test]
fn report_counts_chart_payload_parts_without_style_companions() {
    let doc = Document::open(&docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/charts/chartEx1.xml" ContentType="application/vnd.ms-office.chartex+xml"/><Override PartName="/word/charts/style1.xml" ContentType="application/vnd.ms-office.chartstyle+xml"/><Override PartName="/word/charts/colors1.xml" ContentType="application/vnd.ms-office.chartcolorstyle+xml"/><Override PartName="/word/charts/chartStyle1.xml" ContentType="application/vnd.ms-office.chartstyle+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><w:body><w:p><w:r><w:drawing><c:chart/></w:drawing></w:r></w:p></w:body></w:document>"#,
        ),
        ("word/charts/chart1.xml", r#"<c:chartSpace/>"#),
        ("word/charts/chartEx1.xml", r#"<cx:chartSpace/>"#),
        ("word/charts/style1.xml", r#"<cs:chartStyle/>"#),
        ("word/charts/colors1.xml", r#"<cs:colorStyle/>"#),
        ("word/charts/chartStyle1.xml", r#"<cs:chartStyle/>"#),
    ]))
    .expect("fixture opens");

    assert_eq!(doc.report().features.charts, 2);
}

#[cfg(feature = "docx")]
#[test]
fn report_field_markers_follow_accepted_revision_view() {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p><w:del w:id="73" w:author="Editor"><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>deleted page</w:t></w:r></w:fldSimple></w:p></w:del><w:moveFrom w:id="74" w:author="Editor"><w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>moved-from page</w:t></w:r></w:fldSimple></w:p></w:moveFrom></w:body></w:document>"#,
        ),
    ]))
    .expect("fixture opens");
    let report = doc.report();

    assert_eq!(doc.fields().len(), 1);
    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Page,
            count: 1
        }]
    );
    assert_eq!(report.features.tracked_deletions, 1);
    assert_eq!(report.features.tracked_moves, 1);
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
fn report_malformed_hyperlink_reports_unsupported_switch() {
    let doc = Document::open(&malformed_hyperlink_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Hyperlink,
            count: 3,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 3,
        }]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_mixed_case_hyperlink_field_is_supported() {
    let doc = Document::open(&mixed_case_hyperlink_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Hyperlink);

    let report = doc.report();
    assert!(report.features.unsupported_field_kinds.is_empty());
    assert!(report.features.unsupported_field_reasons.is_empty());
    assert!(report
        .warnings
        .iter()
        .all(|warning| !matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })));
}

#[cfg(feature = "docx")]
#[test]
fn report_page_field_warning_distinguishes_unsupported_switches() {
    let doc = Document::open(&page_unsupported_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
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
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_sectionpages_unsupported_switch_reports_unsupported_switch() {
    let doc = Document::open(&section_pages_unsupported_switch_diagnostics_docx())
        .expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::DocumentStructure("SECTIONPAGES".to_string()),
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
}

#[cfg(feature = "docx")]
#[test]
fn report_revnum_unsupported_switch_reports_unsupported_switch() {
    let doc = Document::open(&revnum_unsupported_switch_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::DocumentStructure("REVNUM".to_string()),
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
}

#[cfg(feature = "docx")]
#[test]
fn report_malformed_filename_reports_unsupported_switch() {
    let doc = Document::open(&malformed_filename_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Filename);
    assert_eq!(fields[0].instruction, "FILENAME \\x");
    assert_eq!(fields[0].result, "cached filename");

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Filename,
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
}

#[cfg(feature = "docx")]
#[test]
fn report_malformed_document_info_reports_unsupported_switch() {
    let doc = Document::open(&malformed_document_info_diagnostics_docx()).expect("fixture opens");
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
}

#[cfg(feature = "docx")]
#[test]
fn report_display_layout_fields_split_cached_and_malformed_diagnostics() {
    let doc = Document::open(&display_layout_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 7);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Display("ADVANCE".to_string()),
                count: 1,
            },
            FieldKindCount {
                kind: FieldKind::Display("EQ".to_string()),
                count: 4,
            },
            FieldKindCount {
                kind: FieldKind::Display("SYMBOL".to_string()),
                count: 2,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Display("EQ".to_string()),
                count: 3,
            },
            FieldKindCount {
                kind: FieldKind::Display("SYMBOL".to_string()),
                count: 2,
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
                count: 3,
            },
        ]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_action_fields_split_cached_and_malformed_diagnostics() {
    let doc = Document::open(&action_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 11);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Action("GOTOBUTTON".to_string()),
                count: 2,
            },
            FieldKindCount {
                kind: FieldKind::Action("MACROBUTTON".to_string()),
                count: 3,
            },
            FieldKindCount {
                kind: FieldKind::Action("PRINT".to_string()),
                count: 6,
            },
        ]
    );
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
}

#[cfg(feature = "docx")]
#[test]
fn report_compatibility_fields_split_cached_and_malformed_diagnostics() {
    let doc = Document::open(&compatibility_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(report.features.fields, 5);
    assert_eq!(
        report.features.field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Compatibility("PRIVATE".to_string()),
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
            FieldKindCount {
                kind: FieldKind::Compatibility("ADDIN".to_string()),
                count: 1,
            },
        ]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![
            FieldKindCount {
                kind: FieldKind::Compatibility("PRIVATE".to_string()),
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
                count: 4,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 1,
            },
        ]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_field_category_matrix_splits_cached_and_malformed_diagnostics() {
    assert_report_field_diagnostics(
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
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" INCLUDETEXT &quot;appendix.docx&quot; "><w:r><w:t>Appendix text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" LINK Excel.Sheet.12 &quot;book.xlsx&quot; &quot;Sheet1!R1C1&quot; "><w:r><w:t>42</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" EMBED Excel.Sheet.12 "><w:r><w:t>Embedded object</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DATABASE \d &quot;source.accdb&quot; "><w:r><w:t>Rows</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DDE Excel &quot;book.xlsx&quot; &quot;R1C1&quot; "><w:r><w:t>DDE value</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DDEAUTO Excel &quot;book.xlsx&quot; &quot;R2C1&quot; "><w:r><w:t>Auto DDE value</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" IMPORT &quot;legacy.wmf&quot; "><w:r><w:t>Imported object</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDE &quot;legacy.doc&quot; "><w:r><w:t>Included text</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTOTEXT Signature "><w:r><w:t>AutoText signature</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" AUTOTEXTLIST &quot;Choose clause&quot; \s Legal "><w:r><w:t>AutoText list</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" INCLUDEPICTURE &quot;chart.png "><w:r><w:t>cached malformed include picture</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
            ),
        ]),
        11,
        vec![
            field_kind_count(FieldKind::InsertedContent("INCLUDETEXT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("LINK".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("EMBED".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DATABASE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DDE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DDEAUTO".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("IMPORT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("INCLUDE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("AUTOTEXT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("AUTOTEXTLIST".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("INCLUDEPICTURE".to_string()), 1),
        ],
        vec![
            field_kind_count(FieldKind::InsertedContent("INCLUDETEXT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("LINK".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("EMBED".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DATABASE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DDE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("DDEAUTO".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("IMPORT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("INCLUDE".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("AUTOTEXT".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("AUTOTEXTLIST".to_string()), 1),
            field_kind_count(FieldKind::InsertedContent("INCLUDEPICTURE".to_string()), 1),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 10),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 1),
        ],
    );

    assert_report_field_diagnostics(
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
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" ADDRESSBLOCK "><w:r><w:t>Acme Corp</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GREETINGLINE "><w:r><w:t>Dear Hyunjo,</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEREC "><w:r><w:t>7</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGESEQ "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" GREETINGLINE &quot;Dear "><w:r><w:t>cached malformed greeting</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
            ),
        ]),
        5,
        vec![
            field_kind_count(FieldKind::MailMerge("ADDRESSBLOCK".to_string()), 1),
            field_kind_count(FieldKind::MailMerge("GREETINGLINE".to_string()), 2),
            field_kind_count(FieldKind::MailMerge("MERGEREC".to_string()), 1),
            field_kind_count(FieldKind::MailMerge("MERGESEQ".to_string()), 1),
        ],
        vec![
            field_kind_count(FieldKind::MailMerge("ADDRESSBLOCK".to_string()), 1),
            field_kind_count(FieldKind::MailMerge("GREETINGLINE".to_string()), 2),
            field_kind_count(FieldKind::MailMerge("MERGEREC".to_string()), 1),
            field_kind_count(FieldKind::MailMerge("MERGESEQ".to_string()), 1),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 4),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 1),
        ],
    );

    assert_report_field_diagnostics(
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
        ]),
        12,
        vec![
            field_kind_count(FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("CITATION".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("INDEX".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("TOA".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("TA".to_string()), 4),
            field_kind_count(FieldKind::ReferenceIndex("XE".to_string()), 3),
            field_kind_count(FieldKind::ReferenceIndex("RD".to_string()), 1),
        ],
        vec![
            field_kind_count(FieldKind::ReferenceIndex("BIBLIOGRAPHY".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("CITATION".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("INDEX".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("TOA".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("XE".to_string()), 1),
            field_kind_count(FieldKind::ReferenceIndex("TA".to_string()), 1),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 4),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 2),
        ],
    );

    assert_report_field_diagnostics(
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
                r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \q H "><w:r><w:t>QR preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" MERGEBARCODE CustomerId CODE128 \t "><w:r><w:t>Merge barcode preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" BARCODE &quot;9781234567890&quot; "><w:r><w:t>Legacy barcode preview</w:t></w:r></w:fldSimple></w:p><w:p><w:fldSimple w:instr=" DISPLAYBARCODE &quot;https://example.com&quot; QR \q "><w:r><w:t>cached missing quality operand</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#,
            ),
        ]),
        4,
        vec![
            field_kind_count(FieldKind::Barcode("DISPLAYBARCODE".to_string()), 2),
            field_kind_count(FieldKind::Barcode("MERGEBARCODE".to_string()), 1),
            field_kind_count(FieldKind::Barcode("BARCODE".to_string()), 1),
        ],
        vec![
            field_kind_count(FieldKind::Barcode("DISPLAYBARCODE".to_string()), 2),
            field_kind_count(FieldKind::Barcode("MERGEBARCODE".to_string()), 1),
            field_kind_count(FieldKind::Barcode("BARCODE".to_string()), 1),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 3),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 1),
        ],
    );

    assert_report_field_diagnostics(
        protected_form_field_diagnostics_docx(),
        4,
        vec![
            field_kind_count(FieldKind::FormField("FORMCHECKBOX".to_string()), 1),
            field_kind_count(FieldKind::FormField("FORMDROPDOWN".to_string()), 1),
            field_kind_count(FieldKind::FormField("FORMTEXT".to_string()), 2),
        ],
        vec![
            field_kind_count(FieldKind::FormField("FORMCHECKBOX".to_string()), 1),
            field_kind_count(FieldKind::FormField("FORMDROPDOWN".to_string()), 1),
            field_kind_count(FieldKind::FormField("FORMTEXT".to_string()), 2),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 3),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 1),
        ],
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_numbering_fields_split_cached_and_malformed_diagnostics() {
    assert_report_field_diagnostics(
        numbering_field_diagnostics_docx(),
        14,
        vec![
            field_kind_count(FieldKind::Numbering("AUTONUM".to_string()), 7),
            field_kind_count(FieldKind::Numbering("AUTONUMLGL".to_string()), 2),
            field_kind_count(FieldKind::Numbering("AUTONUMOUT".to_string()), 2),
            field_kind_count(FieldKind::Numbering("LISTNUM".to_string()), 1),
            field_kind_count(FieldKind::Numbering("BIDIOUTLINE".to_string()), 2),
        ],
        vec![
            field_kind_count(FieldKind::Numbering("AUTONUM".to_string()), 1),
            field_kind_count(FieldKind::Numbering("LISTNUM".to_string()), 1),
            field_kind_count(FieldKind::Numbering("BIDIOUTLINE".to_string()), 2),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 2),
            field_reason_count(FieldEvaluationReason::NoComputedResult, 2),
        ],
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_dynamic_control_fields_split_cached_and_malformed_diagnostics() {
    assert_report_field_diagnostics(
        dynamic_control_field_diagnostics_docx(),
        17,
        vec![
            field_kind_count(FieldKind::Dynamic("IF".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("COMPARE".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("=".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("FILLIN".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("ASK".to_string()), 1),
            field_kind_count(FieldKind::Dynamic("SET".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("QUOTE".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("NEXT".to_string()), 1),
            field_kind_count(FieldKind::Dynamic("SKIPIF".to_string()), 1),
            field_kind_count(FieldKind::Dynamic("NEXTIF".to_string()), 2),
        ],
        vec![
            field_kind_count(FieldKind::Dynamic("IF".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("COMPARE".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("=".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("FILLIN".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("ASK".to_string()), 1),
            field_kind_count(FieldKind::Dynamic("SET".to_string()), 2),
            field_kind_count(FieldKind::Dynamic("QUOTE".to_string()), 1),
            field_kind_count(FieldKind::Dynamic("NEXTIF".to_string()), 2),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 7),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 7),
        ],
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_document_structure_fields_split_computed_cached_and_malformed_diagnostics() {
    assert_report_field_diagnostics(
        document_structure_field_diagnostics_docx(),
        6,
        vec![
            field_kind_count(FieldKind::DocumentStructure("REVNUM".to_string()), 1),
            field_kind_count(FieldKind::DocumentStructure("SECTION".to_string()), 1),
            field_kind_count(FieldKind::DocumentStructure("SECTIONPAGES".to_string()), 1),
            field_kind_count(FieldKind::DocumentStructure("STYLEREF".to_string()), 3),
        ],
        vec![
            field_kind_count(FieldKind::DocumentStructure("SECTIONPAGES".to_string()), 1),
            field_kind_count(FieldKind::DocumentStructure("STYLEREF".to_string()), 3),
        ],
        vec![
            field_reason_count(FieldEvaluationReason::NoComputedResult, 2),
            field_reason_count(FieldEvaluationReason::UnsupportedSwitch, 2),
        ],
    );
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
fn report_malformed_merge_field_reports_unsupported_switch() {
    let doc = Document::open(&malformed_merge_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::MergeField,
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 2,
        }]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_counts_split_complex_field_instruction_once() {
    let doc = Document::open(&split_complex_field_diagnostics_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(doc.fields().len(), 1);
    assert_eq!(report.features.fields, 1);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::MergeField,
            count: 1,
        }]
    );
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
fn report_page_ref_text_format_switches_keep_page_ref_reasons() {
    let doc =
        Document::open(&page_ref_text_format_switch_diagnostics_docx()).expect("fixture opens");
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
}

#[cfg(feature = "docx")]
#[test]
fn report_page_ref_section_format_scan_uses_current_single_branch_view() {
    let doc = Document::open(&page_ref_non_current_section_format_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].computed_result, None);

    let report = doc.report();
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
fn report_page_ref_defers_content_paragraph_section_format_until_paragraph_end() {
    let doc = Document::open(&page_ref_content_paragraph_section_format_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].kind, FieldKind::PageRef);
    assert_eq!(fields[0].instruction, "PAGEREF BeforeFormatBreak \\p");
    assert_eq!(fields[0].computed_result, None);
    assert_eq!(fields[1].kind, FieldKind::PageRef);
    assert_eq!(fields[1].instruction, "PAGEREF AfterFormatBreak \\p");
    assert_eq!(fields[1].computed_result, None);

    let report = doc.report();
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
fn report_page_ref_number_in_dash_section_page_number_format_is_supported() {
    let doc =
        Document::open(&page_ref_number_in_dash_section_page_number_format_diagnostics_docx())
            .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("- 5 -"));
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
fn report_page_ref_decimal_full_width_section_page_number_format_is_supported() {
    let doc =
        Document::open(&page_ref_decimal_full_width_section_page_number_format_diagnostics_docx())
            .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("１２"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));

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
fn report_page_ref_decimal_enclosed_circle_section_page_number_format_is_supported() {
    let doc = Document::open(
        &page_ref_decimal_enclosed_circle_section_page_number_format_diagnostics_docx(),
    )
    .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{246b}"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));

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
fn report_page_ref_decimal_enclosed_punctuation_section_page_number_formats_are_supported() {
    let doc = Document::open(
        &page_ref_decimal_enclosed_punctuation_section_page_number_format_diagnostics_docx(),
    )
    .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{2493}"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("12"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{2480}"));
    assert_eq!(fields[3].computed_result.as_deref(), Some("13"));

    let report = doc.report();
    assert_eq!(report.features.fields, 4);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 4,
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
fn report_page_ref_decimal_width_variant_section_page_number_formats_are_supported() {
    let doc = Document::open(
        &page_ref_decimal_width_variant_section_page_number_format_diagnostics_docx(),
    )
    .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("12"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("- 12 -"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("１３"));
    assert_eq!(fields[3].computed_result.as_deref(), Some("13"));

    let report = doc.report();
    assert_eq!(report.features.fields, 4);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 4,
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
fn report_page_ref_korean_section_page_number_formats_are_supported() {
    let doc = Document::open(&page_ref_korean_section_page_number_format_diagnostics_docx())
        .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{ac00}"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{3134}"));
    assert_eq!(fields[3].computed_result.as_deref(), Some("2"));

    let report = doc.report();
    assert_eq!(report.features.fields, 4);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 4,
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
fn report_page_ref_korean_numeric_section_page_number_formats_are_supported() {
    let doc =
        Document::open(&page_ref_korean_numeric_section_page_number_format_diagnostics_docx())
            .expect("fixture opens");
    let fields = doc.fields();
    assert_eq!(fields[0].computed_result.as_deref(), Some("\u{c77c}"));
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("\u{b458}"));
    assert_eq!(fields[3].computed_result.as_deref(), Some("2"));
    assert_eq!(fields[4].computed_result.as_deref(), Some("\u{c2ed}"));
    assert_eq!(fields[5].computed_result.as_deref(), Some("10"));
    assert_eq!(fields[6].computed_result.as_deref(), Some("\u{c774}"));
    assert_eq!(fields[7].computed_result.as_deref(), Some("2"));

    let report = doc.report();
    assert_eq!(report.features.fields, 8);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::PageRef,
            count: 8,
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

    assert_eq!(report.features.fields, 8);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 8,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 5,
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
                count: 3,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 5,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::Ref,
                count: 5,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"REF","count":8}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"REF","count":5}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"UnsupportedSwitch","count":1},{"reason":"NoComputedResult","count":1},{"reason":"UnresolvedBookmark","count":3}]"#),
        "{json}"
    );
    assert!(json.contains(r#""kind":"UnsupportedFieldEvaluation","count":5,"field_kinds":[{"kind":"REF","count":5}]"#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_ref_bookmark_names_follow_accepted_single_branch_view() {
    let doc = Document::open(&ref_non_current_bookmark_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 2);
    assert!(fields.iter().all(|field| field.kind == FieldKind::Ref));
    assert!(fields.iter().all(|field| field.computed_result.is_none()));

    let report = doc.report();
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Ref,
            count: 2,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::UnresolvedBookmark,
            count: 2,
        }]
    );
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
            count: 5,
        }]
    );
}

#[cfg(feature = "docx")]
#[test]
fn report_note_ref_field_warning_tracks_unresolved_and_unsupported_cases() {
    let doc = Document::open(&note_ref_field_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 8);
    assert!(fields.iter().all(|field| field.kind == FieldKind::NoteRef));
    assert_eq!(fields[0].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[1].instruction, "FTNREF FootOne");
    assert_eq!(fields[1].computed_result.as_deref(), Some("1"));
    assert_eq!(fields[2].computed_result.as_deref(), Some("above"));
    assert_eq!(fields[3].computed_result, None);
    assert_eq!(fields[4].computed_result, None);
    assert_eq!(fields[5].computed_result, None);
    assert_eq!(fields[6].computed_result, None);
    assert_eq!(fields[7].computed_result, None);

    let report = doc.report();

    assert_eq!(report.features.fields, 8);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::NoteRef,
            count: 8,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::NoteRef,
            count: 5,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_reasons,
        vec![
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnsupportedSwitch,
                count: 2,
            },
            FieldEvaluationReasonCount {
                reason: FieldEvaluationReason::UnresolvedBookmark,
                count: 3,
            },
        ]
    );
    assert_eq!(
        report
            .warnings
            .iter()
            .find(|warning| matches!(warning, DocumentWarning::UnsupportedFieldEvaluation { .. })),
        Some(&DocumentWarning::UnsupportedFieldEvaluation {
            count: 5,
            field_kinds: vec![FieldKindCount {
                kind: FieldKind::NoteRef,
                count: 5,
            }],
        })
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""field_kinds":[{"kind":"NOTEREF","count":8}]"#),
        "{json}"
    );
    assert!(
        json.contains(r#""unsupported_field_kinds":[{"kind":"NOTEREF","count":5}]"#),
        "{json}"
    );
    assert!(json.contains(r#""unsupported_field_reasons":[{"reason":"UnsupportedSwitch","count":2},{"reason":"UnresolvedBookmark","count":3}]"#), "{json}");
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

    assert_eq!(report.features.fields, 4);
    assert_eq!(
        report.features.field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 4,
        }]
    );
    assert_eq!(
        report.features.unsupported_field_kinds,
        vec![FieldKindCount {
            kind: FieldKind::Toc,
            count: 4,
        }]
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

    let json = report.to_json();
    assert!(
        json.contains(r#""unsupported_field_reasons":[{"reason":"NoComputedResult","count":2},{"reason":"UnsupportedSwitch","count":2}]"#),
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
fn report_malformed_sequence_reports_unsupported_switch() {
    let doc = Document::open(&malformed_sequence_diagnostics_docx()).expect("fixture opens");
    let fields = doc.fields();

    assert_eq!(fields.len(), 1);
    assert_eq!(fields[0].kind, FieldKind::Sequence);
    assert_eq!(fields[0].instruction, "SEQ Figure \\x");
    assert_eq!(fields[0].result, "bad sequence");

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
            reason: FieldEvaluationReason::UnsupportedSwitch,
            count: 1,
        }]
    );
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
            subject: Some("Pipeline".to_string()),
            creator: Some("Analyst".to_string()),
            description: Some("Diagnostics summary".to_string()),
            keywords: Some("rdoc,metadata".to_string()),
            category: Some("Operations".to_string()),
            content_status: Some("Draft".to_string()),
            last_modified_by: Some("Reviewer".to_string()),
            created: Some("2026-06-01T02:03:04Z".to_string()),
            modified: Some("2026-06-02T03:04:05Z".to_string()),
            last_printed: Some("2026-06-03T04:05:06Z".to_string()),
            revision: Some("12".to_string()),
            version: Some("1.2".to_string()),
        }
    );

    let json = report.to_json();
    assert!(
        json.contains(r#""core_properties":{"title":"Quarter <One> & Co""#),
        "{json}"
    );
    assert!(json.contains(r#""subject":"Pipeline""#), "{json}");
    assert!(json.contains(r#""creator":"Analyst""#), "{json}");
    assert!(
        json.contains(r#""description":"Diagnostics summary""#),
        "{json}"
    );
    assert!(json.contains(r#""keywords":"rdoc,metadata""#), "{json}");
    assert!(json.contains(r#""category":"Operations""#), "{json}");
    assert!(json.contains(r#""content_status":"Draft""#), "{json}");
    assert!(json.contains(r#""last_modified_by":"Reviewer""#), "{json}");
    assert!(
        json.contains(r#""created":"2026-06-01T02:03:04Z""#),
        "{json}"
    );
    assert!(
        json.contains(r#""modified":"2026-06-02T03:04:05Z""#),
        "{json}"
    );
    assert!(
        json.contains(r#""last_printed":"2026-06-03T04:05:06Z""#),
        "{json}"
    );
    assert!(json.contains(r#""revision":"12""#), "{json}");
    assert!(json.contains(r#""version":"1.2""#), "{json}");
}

#[cfg(feature = "docx")]
#[test]
fn report_includes_custom_properties_in_json() {
    let doc = Document::open(&custom_properties_docx()).expect("fixture opens");
    let report = doc.report();

    assert_eq!(
        report
            .custom_properties
            .get("Client Name")
            .map(String::as_str),
        Some("ACME <Launch>")
    );
    assert_eq!(
        report.custom_properties.get("Phase").map(String::as_str),
        Some("Review & Ship")
    );

    let json = report.to_json();
    assert!(
        json.contains(
            r#""custom_properties":{"Client Name":"ACME <Launch>","Phase":"Review & Ship"}"#
        ),
        "{json}"
    );
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
