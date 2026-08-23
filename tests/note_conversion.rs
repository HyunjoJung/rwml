#![cfg(feature = "docx")]

use std::io::{Read, Write};

use rwml::{
    Block, Chart, ChartKind, ChartSeries, DocModel, Document, FieldEvaluationReason,
    FieldEvaluationReasonCount, FieldKind,
};

fn docx_fixture(parts: &[(&str, &str)]) -> Vec<u8> {
    docx_fixture_with_media(parts, &[])
}

fn docx_fixture_with_media(parts: &[(&str, &str)], media: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let cursor = std::io::Cursor::new(&mut out);
        let mut zip = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in parts {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        for (name, body) in media {
            zip.start_file(*name, options).unwrap();
            zip.write_all(body).unwrap();
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

fn tiny_png() -> Vec<u8> {
    vec![
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
        0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
        0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00,
        0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ]
}

fn source_inline_drawing(rel_id: &str, alt: &str, rotation: i64) -> String {
    format!(
        r#"<w:r><w:drawing><wp:inline><wp:extent cx="19050" cy="28575"/><wp:docPr id="1" name="Source image" descr="{alt}"/><a:graphic><a:graphicData><pic:pic><pic:blipFill><a:blip r:embed="{rel_id}"/></pic:blipFill><pic:spPr><a:xfrm rot="{rotation}"/></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

fn source_chart_drawing(rel_id: &str, tag: &str, uri: &str, alt: &str) -> String {
    format!(
        r#"<w:r><w:drawing><wp:inline><wp:extent cx="3810000" cy="2286000"/><wp:docPr id="1" name="Source chart" descr="{alt}"/><a:graphic><a:graphicData uri="{uri}"><{tag} r:id="{rel_id}"/></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>"#
    )
}

fn native_chart_xml() -> (String, String) {
    let model = DocModel {
        blocks: vec![
            Block::Chart(Chart {
                kind: ChartKind::Bar,
                title: Some("Core source".to_string()),
                categories: vec!["North".to_string(), "South".to_string()],
                series: vec![ChartSeries {
                    name: "Core values".to_string(),
                    values: vec![12.0, 18.0],
                    bubble_sizes: Vec::new(),
                }],
                width_px: Some(400),
                height_px: Some(240),
                ..Chart::default()
            }),
            Block::Chart(Chart {
                kind: ChartKind::Waterfall,
                title: Some("Extended source".to_string()),
                categories: vec!["Start".to_string(), "Delta".to_string()],
                series: vec![ChartSeries {
                    name: "Extended values".to_string(),
                    values: vec![30.0, -7.0],
                    bubble_sizes: Vec::new(),
                }],
                width_px: Some(400),
                height_px: Some(240),
                ..Chart::default()
            }),
        ],
        ..DocModel::default()
    };
    let parts = unzip_parts(&rwml::write_docx(&model));
    let mut core = String::from_utf8(parts["word/charts/chart1.xml"].clone()).unwrap();
    let external_start = core.find("<c:externalData").unwrap();
    let external_end = external_start
        + core[external_start..].find("</c:externalData>").unwrap()
        + "</c:externalData>".len();
    core.replace_range(external_start..external_end, "");
    let extended = String::from_utf8(parts["word/charts/chartEx2.xml"].clone()).unwrap();
    (core, extended)
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

fn note_table_docx(document_xml: &str, footnotes_xml: &str, endnotes_xml: &str) -> Vec<u8> {
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
        ("word/document.xml", document_xml),
        ("word/footnotes.xml", footnotes_xml),
        ("word/endnotes.xml", endnotes_xml),
    ])
}

fn rich_note_tables_docx() -> Vec<u8> {
    note_table_docx(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="11"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="12"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:footnote w:id="11">
                <w:p><w:pPr><w:keepNext/></w:pPr><w:r><w:t>FOOT PREFIX A</w:t><w:br w:type="page"/><w:t>FOOT PREFIX B</w:t></w:r></w:p>
                <w:tbl>
                    <w:tblPr><w:tblW w:w="4000" w:type="pct"/><w:tblInd w:w="720" w:type="dxa"/><w:jc w:val="center"/><w:tblBorders><w:top w:val="double" w:sz="12" w:color="112233"/></w:tblBorders><w:tblLayout w:type="fixed"/></w:tblPr>
                    <w:tblGrid><w:gridCol w:w="1200"/><w:gridCol w:w="2400"/></w:tblGrid>
                    <w:tr><w:trPr><w:cantSplit/><w:tblHeader/></w:trPr>
                        <w:tc><w:tcPr><w:vMerge w:val="restart"/><w:shd w:fill="DDEEFF"/><w:vAlign w:val="center"/><w:tcW w:w="2500" w:type="pct"/><w:tcMar><w:top w:w="120" w:type="dxa"/><w:right w:w="240" w:type="dxa"/></w:tcMar></w:tcPr><w:p><w:pPr><w:keepNext/><w:keepLines/><w:widowControl w:val="off"/><w:spacing w:line="260" w:lineRule="exact"/><w:tabs><w:tab w:val="center" w:pos="720" w:leader="hyphen"/></w:tabs></w:pPr><w:r><w:rPr><w:b/><w:highlight w:val="yellow"/></w:rPr><w:t>FOOT TABLE A</w:t><w:tab/><w:t>FOOT TAB</w:t><w:br w:type="column"/><w:t>FOOT TABLE B</w:t><w:br w:type="page"/><w:t>FOOT TABLE C</w:t></w:r></w:p></w:tc>
                        <w:tc><w:tcPr/><w:p><w:pPr><w:spacing w:line="320" w:lineRule="atLeast"/></w:pPr><w:r><w:rPr><w:i/><w:vertAlign w:val="superscript"/></w:rPr><w:t>FOOT SIDE</w:t></w:r></w:p></w:tc>
                    </w:tr>
                    <w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p><w:r><w:t>DROPPED MERGE TEXT</w:t></w:r></w:p></w:tc><w:tc><w:tcPr/><w:p><w:pPr><w:keepLines/></w:pPr><w:r><w:t>FOOT LAST</w:t></w:r></w:p></w:tc></w:tr>
                </w:tbl>
                <w:p><w:r><w:t>FOOT SUFFIX</w:t></w:r></w:p>
            </w:footnote>
        </w:footnotes>"#,
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:endnote w:id="12">
                <w:p><w:r><w:t>END PREFIX</w:t></w:r></w:p>
                <w:tbl>
                    <w:tblPr><w:tblW w:w="3500" w:type="pct"/><w:bidiVisual/><w:jc w:val="right"/></w:tblPr>
                    <w:tblGrid><w:gridCol w:w="1800"/><w:gridCol w:w="1800"/></w:tblGrid>
                    <w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr><w:gridSpan w:val="2"/><w:shd w:fill="FFEEDD"/><w:vAlign w:val="bottom"/></w:tcPr><w:p><w:pPr><w:keepNext/><w:widowControl w:val="off"/><w:spacing w:line="280" w:lineRule="atLeast"/><w:tabs><w:tab w:val="right" w:pos="1200" w:leader="dot"/></w:tabs></w:pPr><w:r><w:rPr><w:u w:val="single"/><w:smallCaps/></w:rPr><w:t>END TABLE A</w:t><w:br w:type="column"/><w:t>END TABLE B</w:t><w:tab/><w:t>END TAB</w:t><w:br w:type="page"/><w:t>END PAGE</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblW w:w="3000" w:type="pct"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr/><w:p><w:pPr><w:keepLines/><w:spacing w:line="360" w:lineRule="exact"/><w:tabs><w:tab w:val="left" w:pos="600" w:leader="hyphen"/></w:tabs></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>END NESTED A</w:t><w:br w:type="column"/><w:t>END NESTED B</w:t><w:tab/><w:t>END NESTED TAB</w:t><w:br w:type="page"/><w:t>END NESTED PAGE</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr>
                </w:tbl>
                <w:p><w:pPr><w:keepLines/></w:pPr><w:r><w:t>END SUFFIX</w:t></w:r></w:p>
            </w:endnote>
        </w:endnotes>"#,
    )
}

fn nested_and_relationship_table_note_docx() -> Vec<u8> {
    note_table_docx(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="21"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="22"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="21"><w:tbl><w:tblPr><w:tblW w:w="4000" w:type="pct"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr/><w:p><w:pPr><w:keepNext/><w:spacing w:line="220" w:lineRule="exact"/></w:pPr><w:r><w:t>OUTER TABLE A</w:t><w:br w:type="page"/><w:t>OUTER TABLE B</w:t></w:r></w:p><w:tbl><w:tblPr><w:tblW w:w="3000" w:type="pct"/><w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:trPr><w:cantSplit/></w:trPr><w:tc><w:tcPr/><w:p><w:pPr><w:keepLines/><w:widowControl w:val="off"/><w:spacing w:line="300" w:lineRule="atLeast"/><w:tabs><w:tab w:val="center" w:pos="900" w:leader="dot"/></w:tabs></w:pPr><w:r><w:rPr><w:b/></w:rPr><w:t>NESTED TABLE A</w:t><w:br w:type="column"/><w:t>NESTED TABLE B</w:t><w:tab/><w:t>NESTED TAB</w:t><w:br w:type="page"/><w:t>NESTED PAGE</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:footnote></w:footnotes>"#,
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="22"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:fldSimple w:instr=" CUSTOM unsupported-table "><w:r><w:t>UNSUPPORTED FIELD TABLE</w:t></w:r></w:fldSimple></w:p></w:tc></w:tr></w:tbl></w:endnote></w:endnotes>"#,
    )
}

fn relationship_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="31"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="41"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="32"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="42"/></w:r><w:r><w:t> BODY E</w:t></w:r><w:r><w:endnoteReference w:id="43"/></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:footnote w:id="31"><w:p><w:hyperlink r:id="rFootOne"><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>FOOT LINK ONE</w:t></w:r></w:hyperlink></w:p></w:footnote><w:footnote w:id="32"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:hyperlink r:id="rFootTwo"><w:r><w:rPr><w:b/></w:rPr><w:t>FOOT NESTED LINK</w:t></w:r></w:hyperlink></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:footnote></w:footnotes>"#,
        ),
        (
            "word/_rels/footnotes.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rFootOne" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/foot-one?x=1&amp;y=2" TargetMode="External"/><Relationship Id="rFootTwo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/foot-two" TargetMode="External"/></Relationships>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:endnote w:id="41"><w:p><w:hyperlink r:id="rEndOne"><w:r><w:rPr><w:i/></w:rPr><w:t>END LINK ONE</w:t></w:r></w:hyperlink></w:p></w:endnote><w:endnote w:id="42"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:hyperlink r:id="rEndTwo"><w:r><w:rPr><w:smallCaps/></w:rPr><w:t>END NESTED LINK</w:t></w:r></w:hyperlink></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="43"><w:p><w:hyperlink r:id="rRejected"><w:r><w:t>REJECTED LINK</w:t></w:r></w:hyperlink><w:fldSimple w:instr=" CUSTOM rejected-link "><w:r><w:t> UNSUPPORTED FIELD</w:t></w:r></w:fldSimple></w:p></w:endnote></w:endnotes>"#,
        ),
        (
            "word/_rels/endnotes.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rEndOne" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/end-one" TargetMode="External"/><Relationship Id="rEndTwo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/end-two" TargetMode="External"/><Relationship Id="rRejected" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/rejected" TargetMode="External"/></Relationships>"#,
        ),
    ])
}

fn internal_anchor_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:bookmarkStart w:id="5" w:name="BodyTarget"/><w:r><w:t>BODY TARGET</w:t></w:r><w:bookmarkEnd w:id="5"/><w:r><w:t> BODY A</w:t></w:r><w:r><w:footnoteReference w:id="51"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="61"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="52"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="62"/></w:r><w:r><w:t> BODY E</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="51"><w:p><w:hyperlink w:anchor=" BodyTarget "><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>FOOT BODY LINK</w:t></w:r></w:hyperlink><w:r><w:t> | </w:t></w:r><w:bookmarkStart w:id="21" w:name="FootTarget"/><w:r><w:rPr><w:b/></w:rPr><w:t>FOOT TARGET</w:t></w:r><w:bookmarkEnd w:id="21"/><w:r><w:t> | </w:t></w:r><w:hyperlink w:anchor="FootTarget"><w:r><w:t>FOOT LOCAL LINK</w:t></w:r></w:hyperlink></w:p></w:footnote><w:footnote w:id="52"><w:p><w:hyperlink w:anchor=" Bad Target "><w:r><w:t>REJECTED ANCHOR</w:t></w:r></w:hyperlink><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="61"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:bookmarkStart w:id="31" w:name="EndTarget"/><w:r><w:rPr><w:i/></w:rPr><w:t>END TARGET</w:t></w:r><w:bookmarkEnd w:id="31"/><w:r><w:t> | </w:t></w:r><w:hyperlink w:anchor="BodyTarget"><w:r><w:t>END BODY LINK</w:t></w:r></w:hyperlink><w:r><w:t> | </w:t></w:r><w:hyperlink w:anchor="EndTarget"><w:r><w:t>END LOCAL LINK</w:t></w:r></w:hyperlink></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="62"><w:p><w:bookmarkStart w:id="41" w:name="Bad Target"/><w:r><w:t>REJECTED BOOKMARK</w:t></w:r><w:bookmarkEnd w:id="41"/><w:hyperlink w:anchor="BodyTarget"><w:r><w:t> VALID LINK</w:t></w:r></w:hyperlink></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn content_control_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="71"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="81"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="72"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="82"/></w:r><w:r><w:t> BODY E</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="71"><w:p><w:sdt><w:sdtPr><w:alias w:val=" Foot alias "/><w:tag w:val=" foot-tag "/></w:sdtPr><w:sdtContent><w:r><w:rPr><w:b/></w:rPr><w:t>FOOT CONTROL</w:t></w:r></w:sdtContent></w:sdt><w:r><w:t> TAIL</w:t></w:r></w:p></w:footnote><w:footnote w:id="72"><w:p><w:sdt><w:sdtPr><w:dataBinding w:xpath=" /root/half "/></w:sdtPr><w:sdtContent><w:r><w:t>REJECTED HALF BINDING</w:t></w:r></w:sdtContent></w:sdt><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="81"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:sdt><w:sdtPr><w:alias w:val=" End binding "/><w:dataBinding w:xpath=" /root/client " w:storeItemID=" {11111111-2222-3333-4444-555555555555} "/></w:sdtPr><w:sdtContent><w:r><w:rPr><w:i/></w:rPr><w:t>END BOUND CONTROL</w:t></w:r></w:sdtContent></w:sdt></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="82"><w:p><w:sdt><w:sdtPr><w:tag w:val=" rejected-tag "/><w:dataBinding w:storeItemID=" {AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE} "/></w:sdtPr><w:sdtContent><w:r><w:t>REJECTED TAGGED HALF</w:t></w:r></w:sdtContent></w:sdt><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn cached_field_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="91"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="101"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="92"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="102"/></w:r><w:r><w:t> BODY E</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="91"><w:p><w:fldSimple w:instr=" PRIVATE legacy-data "><w:r><w:rPr><w:b/></w:rPr><w:t>PRIVATE CACHE</w:t></w:r></w:fldSimple><w:r><w:t> | </w:t></w:r><w:fldSimple w:instr=" INCLUDETEXT &quot;appendix.docx&quot; "><w:r><w:t>INCLUDE CACHE</w:t></w:r></w:fldSimple></w:p></w:footnote><w:footnote w:id="92"><w:p><w:fldSimple w:instr=" ADDIN &quot;bad "><w:r><w:t>REJECTED MALFORMED FIELD</w:t></w:r></w:fldSimple><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="101"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:fldSimple w:instr=" ADDRESSBLOCK "><w:r><w:t>ADDRESS CACHE</w:t></w:r></w:fldSimple><w:r><w:t> | </w:t></w:r><w:fldSimple w:instr=" BARCODE &quot;9781234567890&quot; "><w:r><w:rPr><w:i/></w:rPr><w:t>BARCODE CACHE</w:t></w:r></w:fldSimple><w:r><w:t> | </w:t></w:r><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> PRIVATE complex-data </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:t>COMPLEX CACHE</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="102"><w:p><w:fldSimple w:instr=" REF MissingTarget "><w:r><w:t>REJECTED CONTEXT FIELD</w:t></w:r></w:fldSimple><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn merge_field_note_docx() -> Vec<u8> {
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
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="111"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="121"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="112"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="122"/></w:r><w:r><w:t> BODY E</w:t></w:r></w:p><w:sectPr/></w:body></w:document>"#,
        ),
        (
            "word/footnotes.xml",
            r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:footnote w:id="111"><w:p><w:fldSimple w:instr=" MERGEFIELD &quot;Client Name&quot; \* Upper "><w:r><w:rPr><w:b/></w:rPr><w:t>CLIENT CACHE</w:t></w:r></w:fldSimple><w:r><w:t> FOOT TAIL</w:t></w:r></w:p></w:footnote><w:footnote w:id="112"><w:p><w:fldSimple w:instr=" MERGEFIELD \* MERGEFORMAT "><w:r><w:t>REJECTED MALFORMED MERGE</w:t></w:r></w:fldSimple><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:footnote></w:footnotes>"#,
        ),
        (
            "word/endnotes.xml",
            r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:endnote w:id="121"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> MERGEFIELD ProjectName \* MERGEFORMAT </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>PROJECT CACHE</w:t></w:r><w:r><w:fldChar w:fldCharType="end"/></w:r><w:r><w:t> END TAIL</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="122"><w:p><w:fldSimple w:instr=" CUSTOM payload "><w:r><w:t>REJECTED UNKNOWN FIELD</w:t></w:r></w:fldSimple><w:r><w:t> FALLBACK</w:t></w:r></w:p></w:endnote></w:endnotes>"#,
        ),
    ])
}

fn raster_note_docx() -> Vec<u8> {
    let png = tiny_png();
    let body_image = source_inline_drawing("rBodyImage", "Body image", 0);
    let foot_one_image = source_inline_drawing("rFootImageOne", "Foot &lt;one&gt;", 0);
    let foot_two_image = source_inline_drawing("rFootImageTwo", "Foot nested", 0);
    let end_one_image = source_inline_drawing("rEndImageOne", "End rotate", 5_400_000);
    let end_two_image = source_inline_drawing("rEndImageTwo", "End nested", 0);
    let rejected_image = source_inline_drawing("rRejectedImage", "Rejected image", 0);
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:body><w:p>{body_image}</w:p><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="51"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="61"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="52"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="62"/></w:r><w:r><w:t> BODY E</w:t></w:r><w:r><w:endnoteReference w:id="63"/></w:r></w:p><w:sectPr/></w:body></w:document>"#
    );
    let footnotes_xml = format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:footnote w:id="51"><w:p><w:r><w:t>FOOT IMAGE ONE</w:t></w:r><w:hyperlink r:id="rFootLink">{foot_one_image}</w:hyperlink></w:p></w:footnote><w:footnote w:id="52"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:t>FOOT NESTED IMAGE</w:t></w:r>{foot_two_image}</w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:footnote></w:footnotes>"#
    );
    let endnotes_xml = format!(
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"><w:endnote w:id="61"><w:p><w:r><w:t>END IMAGE ONE</w:t></w:r>{end_one_image}</w:p></w:endnote><w:endnote w:id="62"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:t>END NESTED IMAGE</w:t></w:r><w:hyperlink r:id="rEndLink">{end_two_image}</w:hyperlink></w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="63"><w:p><w:r><w:t>REJECTED IMAGE</w:t></w:r>{rejected_image}<w:fldSimple w:instr=" CUSTOM rejected-image "><w:r><w:t> UNSUPPORTED FIELD</w:t></w:r></w:fldSimple></w:p></w:endnote></w:endnotes>"#
    );

    docx_fixture_with_media(
        &[
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
            ),
            (
                "word/_rels/document.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rBodyImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/body.png"/></Relationships>"#,
            ),
            ("word/document.xml", document_xml.as_str()),
            ("word/footnotes.xml", footnotes_xml.as_str()),
            (
                "word/_rels/footnotes.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rFootLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/foot-image" TargetMode="External"/><Relationship Id="rFootImageOne" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/foot-one.png"/><Relationship Id="rFootImageTwo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/foot-two.png"/></Relationships>"#,
            ),
            ("word/endnotes.xml", endnotes_xml.as_str()),
            (
                "word/_rels/endnotes.xml.rels",
                r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rEndImageOne" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/end-one.png"/><Relationship Id="rEndLink" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://example.com/end-image" TargetMode="External"/><Relationship Id="rEndImageTwo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/end-two.png"/><Relationship Id="rRejectedImage" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/image" Target="media/rejected.png"/></Relationships>"#,
            ),
        ],
        &[
            ("word/media/body.png", png.as_slice()),
            ("word/media/foot-one.png", png.as_slice()),
            ("word/media/foot-two.png", png.as_slice()),
            ("word/media/end-one.png", png.as_slice()),
            ("word/media/end-two.png", png.as_slice()),
            ("word/media/rejected.png", png.as_slice()),
        ],
    )
}

fn chart_note_docx() -> Vec<u8> {
    let (core_chart, extended_chart) = native_chart_xml();
    let core_uri = "http://schemas.openxmlformats.org/drawingml/2006/chart";
    let extended_uri = "http://schemas.microsoft.com/office/drawing/2014/chartex";
    let body_chart = source_chart_drawing("rBodyChart", "c:chart", core_uri, "Body chart");
    let foot_one_chart =
        source_chart_drawing("rFootChartOne", "c:chart", core_uri, "Foot &lt;chart&gt;");
    let foot_two_chart =
        source_chart_drawing("rFootChartTwo", "c:chart", core_uri, "Foot nested chart");
    let end_one_chart = source_chart_drawing(
        "rEndChartOne",
        "cx:chart",
        extended_uri,
        "End extended chart",
    );
    let end_two_chart =
        source_chart_drawing("rEndChartTwo", "cx:chart", extended_uri, "End nested chart");
    let rejected_chart =
        source_chart_drawing("rRejectedChart", "c:chart", core_uri, "Rejected chart");
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><w:body><w:p>{body_chart}</w:p><w:p><w:r><w:t>BODY A</w:t></w:r><w:r><w:footnoteReference w:id="71"/></w:r><w:r><w:t> BODY B</w:t></w:r><w:r><w:endnoteReference w:id="81"/></w:r><w:r><w:t> BODY C</w:t></w:r><w:r><w:footnoteReference w:id="72"/></w:r><w:r><w:t> BODY D</w:t></w:r><w:r><w:endnoteReference w:id="82"/></w:r><w:r><w:t> BODY E</w:t></w:r><w:r><w:endnoteReference w:id="83"/></w:r></w:p><w:sectPr/></w:body></w:document>"#
    );
    let footnotes_xml = format!(
        r#"<w:footnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart"><w:footnote w:id="71"><w:p><w:r><w:t>FOOT CHART ONE</w:t></w:r></w:p><w:p>{foot_one_chart}</w:p></w:footnote><w:footnote w:id="72"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:t>FOOT NESTED CHART</w:t></w:r></w:p><w:p>{foot_two_chart}</w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:footnote></w:footnotes>"#
    );
    let endnotes_xml = format!(
        r#"<w:endnotes xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:c="http://schemas.openxmlformats.org/drawingml/2006/chart" xmlns:cx="http://schemas.microsoft.com/office/drawing/2014/chartex"><w:endnote w:id="81"><w:p><w:r><w:t>END CHART ONE</w:t></w:r></w:p><w:p>{end_one_chart}</w:p></w:endnote><w:endnote w:id="82"><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="3000"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:tbl><w:tblPr/><w:tblGrid><w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr/><w:p><w:r><w:t>END NESTED CHART</w:t></w:r></w:p><w:p>{end_two_chart}</w:p></w:tc></w:tr></w:tbl></w:tc></w:tr></w:tbl></w:endnote><w:endnote w:id="83"><w:p><w:r><w:t>REJECTED CHART</w:t></w:r><w:fldSimple w:instr=" CUSTOM rejected-chart "><w:r><w:t> UNSUPPORTED FIELD</w:t></w:r></w:fldSimple></w:p><w:p>{rejected_chart}</w:p></w:endnote></w:endnotes>"#
    );

    docx_fixture(&[
        (
            "[Content_Types].xml",
            r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/><Override PartName="/word/endnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"/><Override PartName="/word/charts/chart1.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/charts/chart2.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/charts/chart3.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/><Override PartName="/word/charts/chartEx4.xml" ContentType="application/vnd.ms-office.chartex+xml"/><Override PartName="/word/charts/chartEx5.xml" ContentType="application/vnd.ms-office.chartex+xml"/><Override PartName="/word/charts/chart6.xml" ContentType="application/vnd.openxmlformats-officedocument.drawingml.chart+xml"/></Types>"#,
        ),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdDoc" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        (
            "word/_rels/document.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdFoot" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/footnotes" Target="footnotes.xml"/><Relationship Id="rIdEnd" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/endnotes" Target="endnotes.xml"/><Relationship Id="rBodyChart" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart1.xml"/></Relationships>"#,
        ),
        ("word/document.xml", document_xml.as_str()),
        ("word/footnotes.xml", footnotes_xml.as_str()),
        (
            "word/_rels/footnotes.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rFootChartOne" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart2.xml"/><Relationship Id="rFootChartTwo" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart3.xml"/></Relationships>"#,
        ),
        ("word/endnotes.xml", endnotes_xml.as_str()),
        (
            "word/_rels/endnotes.xml.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rEndChartOne" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="charts/chartEx4.xml"/><Relationship Id="rEndChartTwo" Type="http://schemas.microsoft.com/office/2014/relationships/chartEx" Target="charts/chartEx5.xml"/><Relationship Id="rRejectedChart" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/chart" Target="charts/chart6.xml"/></Relationships>"#,
        ),
        ("word/charts/chart1.xml", core_chart.as_str()),
        ("word/charts/chart2.xml", core_chart.as_str()),
        ("word/charts/chart3.xml", core_chart.as_str()),
        ("word/charts/chartEx4.xml", extended_chart.as_str()),
        ("word/charts/chartEx5.xml", extended_chart.as_str()),
        ("word/charts/chart6.xml", core_chart.as_str()),
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
fn opened_docx_simple_table_note_roundtrips_without_disabling_supported_sibling_payload() {
    let document =
        Document::open(&mixed_supported_and_table_note_docx()).expect("mixed note fixture opens");
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx());
    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let footnote = note_with_marker(footnotes, "footnote", "TABLE NOTE");
    let endnote = note_with_marker(endnotes, "endnote", "END ONE");

    assert_eq!(footnote.matches("<w:tbl>").count(), 1, "{footnote}");
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

#[test]
fn opened_docx_rich_mixed_note_tables_roundtrip_through_fresh_conversion() {
    let document = Document::open(&rich_note_tables_docx()).expect("rich note table fixture opens");
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
    let footnote = note_with_marker(footnotes, "footnote", "FOOT TABLE A");
    let endnote = note_with_marker(endnotes, "endnote", "END TABLE A");

    assert_eq!(
        footnote.matches(r#"<w:br w:type="page"/>"#).count(),
        2,
        "{footnote}"
    );
    let foot_table = footnote.find("<w:tbl>").unwrap();
    assert!(footnote.find("FOOT PREFIX").unwrap() < foot_table);
    assert!(foot_table < footnote.find("FOOT SUFFIX").unwrap());
    assert_eq!(footnote.matches("<w:tbl>").count(), 1, "{footnote}");
    assert!(footnote.contains(r#"<w:tblW w:w="4000" w:type="pct"/>"#));
    assert!(footnote.contains(r#"<w:tblInd w:w="720" w:type="dxa"/>"#));
    assert!(footnote.contains(r#"<w:jc w:val="center"/>"#));
    assert!(footnote.contains(r#"<w:top w:val="double" w:sz="12" w:space="0" w:color="112233"/>"#));
    assert!(footnote.contains(r#"<w:tblLayout w:type="fixed"/>"#));
    assert!(footnote.contains(r#"<w:vMerge w:val="restart"/>"#));
    assert!(footnote.contains("<w:vMerge/>"));
    assert!(!footnote.contains("DROPPED MERGE TEXT"));
    assert!(footnote.contains("<w:cantSplit/>"));
    assert!(footnote.contains("<w:tblHeader/>"));
    assert!(footnote.contains("<w:keepNext/>"));
    assert_eq!(footnote.matches("<w:keepLines/>").count(), 3, "{footnote}");
    assert!(footnote.contains(r#"<w:widowControl w:val="0"/>"#));
    assert!(footnote.contains(r#"w:line="260" w:lineRule="exact""#));
    assert!(footnote.contains(r#"w:line="320" w:lineRule="atLeast""#));
    assert!(footnote.contains(r#"w:val="center" w:pos="720" w:leader="hyphen""#));
    assert_eq!(footnote.matches(r#"<w:br w:type="column"/>"#).count(), 1);
    assert!(footnote.contains(r#"w:fill="DDEEFF""#));
    assert!(footnote.contains("<w:b/>"));
    assert!(footnote.contains(r#"<w:highlight w:val="yellow"/>"#));
    assert!(footnote.contains("<w:i/>"));
    assert!(footnote.contains(r#"<w:vertAlign w:val="superscript"/>"#));

    assert_eq!(
        endnote.matches(r#"<w:br w:type="page"/>"#).count(),
        2,
        "{endnote}"
    );
    let end_table = endnote.find("<w:tbl>").unwrap();
    assert!(endnote.find("END PREFIX").unwrap() < end_table);
    assert!(end_table < endnote.find("END SUFFIX").unwrap());
    assert_eq!(endnote.matches("<w:tbl>").count(), 2, "{endnote}");
    assert!(endnote.contains("<w:bidiVisual/>"));
    assert!(endnote.contains(r#"<w:jc w:val="right"/>"#));
    assert!(endnote.contains(r#"<w:gridSpan w:val="2"/>"#));
    assert_eq!(endnote.matches("<w:cantSplit/>").count(), 2, "{endnote}");
    assert!(endnote.contains(r#"w:line="280" w:lineRule="atLeast""#));
    assert!(endnote.contains(r#"w:val="right" w:pos="1200" w:leader="dot""#));
    assert_eq!(endnote.matches(r#"<w:br w:type="column"/>"#).count(), 2);
    assert!(endnote.contains(r#"w:fill="FFEEDD""#));
    assert!(endnote.contains(r#"<w:vAlign w:val="bottom"/>"#));
    assert!(endnote.contains(r#"<w:u w:val="single"/>"#));
    assert!(endnote.contains("<w:smallCaps/>"));
    assert!(endnote.contains("END NESTED A"));
    assert!(endnote.contains(r#"w:line="360" w:lineRule="exact""#));
    assert!(endnote.contains(r#"w:val="left" w:pos="600" w:leader="hyphen""#));

    let reopened = Document::open(&converted).expect("converted note tables reopen");
    assert_eq!(reopened.model(), normalized_model);
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
}

#[test]
fn nested_note_table_roundtrips_without_disabling_supported_sibling() {
    let document = Document::open(&nested_and_relationship_table_note_docx())
        .expect("mixed supported and unsupported note tables open");
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx());
    assert_eq!(document.model(), source_model);
    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let footnote = note_with_marker(footnotes, "footnote", "NESTED TABLE A");
    let endnote = note_with_marker(endnotes, "endnote", "UNSUPPORTED FIELD TABLE");

    assert_eq!(footnote.matches("<w:tbl>").count(), 2, "{footnote}");
    assert_eq!(
        footnote.matches(r#"<w:br w:type="page"/>"#).count(),
        2,
        "{footnote}"
    );
    assert_eq!(footnote.matches("<w:cantSplit/>").count(), 2, "{footnote}");
    assert!(footnote.contains("<w:keepNext/>"), "{footnote}");
    assert!(footnote.contains("<w:keepLines/>"), "{footnote}");
    assert!(footnote.contains(r#"<w:widowControl w:val="0"/>"#));
    assert!(footnote.contains(r#"w:line="220" w:lineRule="exact""#));
    assert!(footnote.contains(r#"w:line="300" w:lineRule="atLeast""#));
    assert!(footnote.contains(r#"w:val="center" w:pos="900" w:leader="dot""#));
    assert_eq!(footnote.matches(r#"<w:br w:type="column"/>"#).count(), 1);
    assert!(footnote.contains("<w:b/>"));

    assert!(!endnote.contains("<w:tbl>"), "{endnote}");
    assert_eq!(endnote.matches("<w:p>").count(), 1, "{endnote}");
    assert!(!endnote.contains("<w:fldSimple"), "{endnote}");

    let reopened = Document::open(&converted).expect("converted nested note table reopens");
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
}

#[test]
fn opened_docx_note_external_hyperlinks_keep_part_local_relationships() {
    let document = Document::open(&relationship_note_docx()).expect("relationship notes open");
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
    let document_rels = std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).unwrap();
    assert!(parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(parts.contains_key("word/_rels/endnotes.xml.rels"));
    let footnote_rels = std::str::from_utf8(&parts["word/_rels/footnotes.xml.rels"]).unwrap();
    let endnote_rels = std::str::from_utf8(&parts["word/_rels/endnotes.xml.rels"]).unwrap();

    let foot_one = note_with_marker(footnotes, "footnote", "FOOT LINK ONE");
    let foot_two = note_with_marker(footnotes, "footnote", "FOOT NESTED LINK");
    let end_one = note_with_marker(endnotes, "endnote", "END LINK ONE");
    let end_two = note_with_marker(endnotes, "endnote", "END NESTED LINK");
    let rejected = note_with_marker(endnotes, "endnote", "REJECTED LINK");
    assert!(footnotes.contains("xmlns:r="), "{footnotes}");
    assert!(endnotes.contains("xmlns:r="), "{endnotes}");
    assert!(foot_one.contains(r#"<w:hyperlink r:id="rId1">"#));
    assert!(foot_two.contains(r#"<w:hyperlink r:id="rId2">"#));
    assert_eq!(foot_two.matches("<w:tbl>").count(), 2, "{foot_two}");
    assert!(end_one.contains(r#"<w:hyperlink r:id="rId1">"#));
    assert!(end_two.contains(r#"<w:hyperlink r:id="rId2">"#));
    assert_eq!(end_two.matches("<w:tbl>").count(), 2, "{end_two}");
    assert!(!rejected.contains("<w:hyperlink"), "{rejected}");
    assert!(!rejected.contains("<w:fldSimple"), "{rejected}");
    assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");

    assert_eq!(footnote_rels.matches("TargetMode=\"External\"").count(), 2);
    assert!(footnote_rels.contains(r#"Id="rId1""#));
    assert!(footnote_rels.contains(r#"Id="rId2""#));
    assert!(footnote_rels.contains("https://example.com/foot-one?x=1&amp;y=2"));
    assert!(footnote_rels.contains("https://example.com/foot-two"));
    assert_eq!(endnote_rels.matches("TargetMode=\"External\"").count(), 2);
    assert!(endnote_rels.contains(r#"Id="rId1""#));
    assert!(endnote_rels.contains(r#"Id="rId2""#));
    assert!(endnote_rels.contains("https://example.com/end-one"));
    assert!(endnote_rels.contains("https://example.com/end-two"));
    assert!(!endnote_rels.contains("https://example.com/rejected"));
    for target in ["foot-one", "foot-two", "end-one", "end-two", "rejected"] {
        assert!(!document_rels.contains(target), "{document_rels}");
    }

    let reopened = Document::open(&converted).expect("converted relationship notes reopen");
    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    assert_eq!(
        &reopened_model.blocks[..reopened_model.blocks.len() - 1],
        &normalized_model.blocks[..normalized_model.blocks.len() - 1]
    );
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_internal_anchors_keep_bookmarks_without_relationships() {
    let document = Document::open(&internal_anchor_note_docx()).expect("anchor notes open");
    assert_eq!(document.notes().len(), 4, "source note records missing");
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone anchor normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let document_xml = std::str::from_utf8(&parts["word/document.xml"]).unwrap();
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let document_rels = std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).unwrap();
    assert!(!parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!parts.contains_key("word/_rels/endnotes.xml.rels"));
    assert!(!document_rels.contains("relationships/hyperlink"));
    assert!(!footnotes.contains("xmlns:r="), "{footnotes}");
    assert!(!endnotes.contains("xmlns:r="), "{endnotes}");

    let footnote = note_with_marker(footnotes, "footnote", "FOOT BODY LINK");
    let endnote = note_with_marker(endnotes, "endnote", "END TARGET");
    let rejected_anchor = note_with_marker(footnotes, "footnote", "REJECTED ANCHOR");
    let rejected_bookmark = note_with_marker(endnotes, "endnote", "REJECTED BOOKMARK");
    assert!(footnote.contains(r#"<w:hyperlink w:anchor="BodyTarget">"#));
    assert!(footnote.contains(r#"<w:hyperlink w:anchor="FootTarget">"#));
    assert!(footnote.contains(r#"<w:bookmarkStart w:id="1" w:name="FootTarget"/>"#));
    assert!(footnote.contains(r#"<w:bookmarkEnd w:id="1"/>"#));
    assert_eq!(endnote.matches("<w:tbl>").count(), 2, "{endnote}");
    assert!(endnote.contains(r#"<w:hyperlink w:anchor="BodyTarget">"#));
    assert!(endnote.contains(r#"<w:hyperlink w:anchor="EndTarget">"#));
    assert!(endnote.contains(r#"<w:bookmarkStart w:id="2" w:name="EndTarget"/>"#));
    assert!(endnote.contains(r#"<w:bookmarkEnd w:id="2"/>"#));

    let all_stories = format!("{document_xml}{footnotes}{endnotes}");
    for (id, name) in [("0", "BodyTarget"), ("1", "FootTarget"), ("2", "EndTarget")] {
        assert_eq!(
            all_stories
                .matches(&format!(
                    r#"<w:bookmarkStart w:id="{id}" w:name="{name}"/>"#
                ))
                .count(),
            1,
            "{all_stories}"
        );
        assert_eq!(
            all_stories
                .matches(&format!(r#"<w:bookmarkEnd w:id="{id}"/>"#))
                .count(),
            1,
            "{all_stories}"
        );
    }
    assert!(
        !rejected_anchor.contains("<w:hyperlink"),
        "{rejected_anchor}"
    );
    assert!(
        !rejected_anchor.contains("<w:bookmark"),
        "{rejected_anchor}"
    );
    assert_eq!(rejected_anchor.matches("<w:p>").count(), 1);
    assert!(
        !rejected_bookmark.contains("<w:hyperlink"),
        "{rejected_bookmark}"
    );
    assert!(
        !rejected_bookmark.contains("<w:bookmark"),
        "{rejected_bookmark}"
    );
    assert_eq!(rejected_bookmark.matches("<w:p>").count(), 1);

    let reopened = Document::open(&converted).expect("converted anchor notes reopen");
    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    for index in [0, 1, 3] {
        assert_eq!(reopened_model.blocks[index], normalized_model.blocks[index]);
    }
    let Block::Paragraph(rejected_anchor) = &reopened_model.blocks[2] else {
        panic!("rejected anchor fallback paragraph")
    };
    assert_eq!(rejected_anchor.text(), "REJECTED ANCHOR FALLBACK");
    let Block::Paragraph(rejected_bookmark) = &reopened_model.blocks[4] else {
        panic!("rejected bookmark fallback paragraph")
    };
    assert_eq!(rejected_bookmark.text(), "REJECTED BOOKMARK VALID LINK");
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_content_controls_keep_complete_modeled_metadata() {
    let document =
        Document::open(&content_control_note_docx()).expect("content-control notes open");
    assert_eq!(document.notes().len(), 4, "source note records missing");
    assert_eq!(document.report().features.content_controls, 4);
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone content-control normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    assert!(!parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!parts.contains_key("word/_rels/endnotes.xml.rels"));
    assert!(!footnotes.contains("xmlns:r="), "{footnotes}");
    assert!(!endnotes.contains("xmlns:r="), "{endnotes}");

    let footnote = note_with_marker(footnotes, "footnote", "FOOT CONTROL");
    let endnote = note_with_marker(endnotes, "endnote", "END BOUND CONTROL");
    let rejected_binding = note_with_marker(footnotes, "footnote", "REJECTED HALF BINDING");
    let rejected_tagged = note_with_marker(endnotes, "endnote", "REJECTED TAGGED HALF");

    let foot_sdt = footnote.find("<w:sdt>").unwrap_or(usize::MAX);
    let foot_alias = footnote
        .find(r#"<w:alias w:val="Foot alias"/>"#)
        .unwrap_or(usize::MAX);
    let foot_tag = footnote
        .find(r#"<w:tag w:val="foot-tag"/>"#)
        .unwrap_or(usize::MAX);
    let foot_content = footnote.find("<w:sdtContent>").unwrap_or(usize::MAX);
    let foot_bold = footnote.find("<w:b/>").unwrap_or(usize::MAX);
    let foot_text = footnote.find("FOOT CONTROL").unwrap_or(usize::MAX);
    let foot_end = footnote.find("</w:sdt>").unwrap_or(usize::MAX);
    assert!(
        foot_sdt < foot_alias
            && foot_alias < foot_tag
            && foot_tag < foot_content
            && foot_content < foot_bold
            && foot_bold < foot_text
            && foot_text < foot_end,
        "footnote content-control XML missing or out of order: {footnote}"
    );

    assert_eq!(endnote.matches("<w:tbl>").count(), 2, "{endnote}");
    let end_alias = endnote
        .find(r#"<w:alias w:val="End binding"/>"#)
        .unwrap_or(usize::MAX);
    let end_binding = endnote
        .find(r#"<w:dataBinding w:xpath="/root/client" w:storeItemID="{11111111-2222-3333-4444-555555555555}"/>"#)
        .unwrap_or(usize::MAX);
    let end_content = endnote.find("<w:sdtContent>").unwrap_or(usize::MAX);
    let end_italic = endnote.find("<w:i/>").unwrap_or(usize::MAX);
    let end_text = endnote.find("END BOUND CONTROL").unwrap_or(usize::MAX);
    assert!(
        end_alias < end_binding
            && end_binding < end_content
            && end_content < end_italic
            && end_italic < end_text,
        "endnote data-bound control XML missing or out of order: {endnote}"
    );

    for rejected in [rejected_binding, rejected_tagged] {
        assert!(!rejected.contains("<w:sdt"), "{rejected}");
        assert!(!rejected.contains("<w:dataBinding"), "{rejected}");
        assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");
    }
    assert!(!footnotes.contains("/root/half"), "{footnotes}");
    assert!(!endnotes.contains("rejected-tag"), "{endnotes}");
    assert!(!endnotes.contains("AAAAAAAA-BBBB"), "{endnotes}");

    let reopened = Document::open(&converted).expect("converted content-control notes reopen");
    assert_eq!(reopened.report().features.content_controls, 2);
    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    for index in [0, 1, 3] {
        assert_eq!(reopened_model.blocks[index], normalized_model.blocks[index]);
    }
    let Block::Paragraph(rejected_binding) = &reopened_model.blocks[2] else {
        panic!("rejected half-binding fallback paragraph")
    };
    assert_eq!(rejected_binding.text(), "REJECTED HALF BINDING FALLBACK");
    assert!(rejected_binding
        .runs
        .iter()
        .all(|run| run.content_control.is_none()));
    let Block::Paragraph(rejected_tagged) = &reopened_model.blocks[4] else {
        panic!("rejected tagged half-binding fallback paragraph")
    };
    assert_eq!(rejected_tagged.text(), "REJECTED TAGGED HALF FALLBACK");
    assert!(rejected_tagged
        .runs
        .iter()
        .all(|run| run.content_control.is_none()));
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_cache_only_fields_keep_normalized_results() {
    let document = Document::open(&cached_field_note_docx()).expect("cached-field notes open");
    assert_eq!(document.notes().len(), 4, "source note records missing");
    assert_eq!(document.report().features.fields, 7);
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone cached-field normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    assert!(!parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!parts.contains_key("word/_rels/endnotes.xml.rels"));
    assert!(!footnotes.contains("xmlns:r="), "{footnotes}");
    assert!(!endnotes.contains("xmlns:r="), "{endnotes}");

    let footnote = note_with_marker(footnotes, "footnote", "PRIVATE CACHE");
    let endnote = note_with_marker(endnotes, "endnote", "ADDRESS CACHE");
    let rejected_malformed = note_with_marker(footnotes, "footnote", "REJECTED MALFORMED FIELD");
    let rejected_context = note_with_marker(endnotes, "endnote", "REJECTED CONTEXT FIELD");

    assert_eq!(footnote.matches("<w:fldSimple").count(), 2, "{footnote}");
    assert!(
        footnote.contains(r#"<w:fldSimple w:instr=" PRIVATE legacy-data ">"#)
            && footnote
                .contains(r#"<w:fldSimple w:instr=" INCLUDETEXT &quot;appendix.docx&quot; ">"#)
            && footnote.contains("<w:b/>")
            && footnote.contains("PRIVATE CACHE")
            && footnote.contains("INCLUDE CACHE"),
        "top-level cached fields missing: {footnote}"
    );
    assert!(!footnote.contains("w:dirty="), "{footnote}");

    assert_eq!(endnote.matches("<w:tbl>").count(), 2, "{endnote}");
    assert_eq!(endnote.matches("<w:fldSimple").count(), 3, "{endnote}");
    assert!(
        endnote.contains(r#"<w:fldSimple w:instr=" ADDRESSBLOCK ">"#)
            && endnote.contains(r#"<w:fldSimple w:instr=" BARCODE &quot;9781234567890&quot; ">"#)
            && endnote.contains(r#"<w:fldSimple w:instr=" PRIVATE complex-data ">"#)
            && endnote.contains("<w:i/>")
            && endnote.contains("BARCODE CACHE")
            && endnote.contains("COMPLEX CACHE"),
        "nested cached fields missing: {endnote}"
    );
    assert!(!endnote.contains("<w:fldChar"), "{endnote}");
    assert!(!endnote.contains("w:dirty="), "{endnote}");

    for rejected in [rejected_malformed, rejected_context] {
        assert!(!rejected.contains("<w:fldSimple"), "{rejected}");
        assert!(!rejected.contains("<w:fldChar"), "{rejected}");
        assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");
    }
    assert!(!footnotes.contains("ADDIN"), "{footnotes}");
    assert!(!endnotes.contains("REF MissingTarget"), "{endnotes}");

    let reopened = Document::open(&converted).expect("converted cached-field notes reopen");
    assert_eq!(reopened.report().features.fields, 5);
    assert_eq!(
        reopened.report().features.unsupported_field_reasons,
        vec![FieldEvaluationReasonCount {
            reason: FieldEvaluationReason::NoComputedResult,
            count: 5,
        }]
    );
    let fields = reopened.fields();
    assert_eq!(fields.len(), 5);
    assert_eq!(
        fields[0].kind,
        FieldKind::Compatibility("PRIVATE".to_string())
    );
    assert_eq!(fields[0].instruction, "PRIVATE legacy-data");
    assert_eq!(fields[0].result, "PRIVATE CACHE");
    assert_eq!(
        fields[1].kind,
        FieldKind::InsertedContent("INCLUDETEXT".to_string())
    );
    assert_eq!(fields[1].instruction, r#"INCLUDETEXT "appendix.docx""#);
    assert_eq!(fields[1].result, "INCLUDE CACHE");
    assert_eq!(
        fields[2].kind,
        FieldKind::MailMerge("ADDRESSBLOCK".to_string())
    );
    assert_eq!(fields[2].result, "ADDRESS CACHE");
    assert_eq!(fields[3].kind, FieldKind::Barcode("BARCODE".to_string()));
    assert_eq!(fields[3].instruction, r#"BARCODE "9781234567890""#);
    assert_eq!(fields[3].result, "BARCODE CACHE");
    assert_eq!(
        fields[4].kind,
        FieldKind::Compatibility("PRIVATE".to_string())
    );
    assert_eq!(fields[4].instruction, "PRIVATE complex-data");
    assert_eq!(fields[4].result, "COMPLEX CACHE");
    assert!(fields.iter().all(|field| field.computed_result.is_none()));

    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    for index in [0, 1, 3] {
        assert_eq!(reopened_model.blocks[index], normalized_model.blocks[index]);
    }
    let Block::Paragraph(rejected_malformed) = &reopened_model.blocks[2] else {
        panic!("rejected malformed-field fallback paragraph")
    };
    assert_eq!(
        rejected_malformed.text(),
        "REJECTED MALFORMED FIELD FALLBACK"
    );
    let Block::Paragraph(rejected_context) = &reopened_model.blocks[4] else {
        panic!("rejected context-field fallback paragraph")
    };
    assert_eq!(rejected_context.text(), "REJECTED CONTEXT FIELD FALLBACK");
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_merge_fields_keep_validated_cached_results() {
    let document = Document::open(&merge_field_note_docx()).expect("merge-field notes open");
    assert_eq!(document.notes().len(), 4, "source note records missing");
    assert_eq!(document.report().features.fields, 4);
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone merge-field normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    assert!(!parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!parts.contains_key("word/_rels/endnotes.xml.rels"));
    assert!(!footnotes.contains("xmlns:r="), "{footnotes}");
    assert!(!endnotes.contains("xmlns:r="), "{endnotes}");

    let footnote = note_with_marker(footnotes, "footnote", "CLIENT CACHE");
    let endnote = note_with_marker(endnotes, "endnote", "PROJECT CACHE");
    let rejected_malformed = note_with_marker(footnotes, "footnote", "REJECTED MALFORMED MERGE");
    let rejected_unknown = note_with_marker(endnotes, "endnote", "REJECTED UNKNOWN FIELD");

    assert_eq!(footnote.matches("<w:fldSimple").count(), 1, "{footnote}");
    assert!(
        footnote
            .contains(r#"<w:fldSimple w:instr=" MERGEFIELD &quot;Client Name&quot; \* Upper ">"#)
            && footnote.contains("<w:b/>")
            && footnote.contains("CLIENT CACHE")
            && footnote.contains("FOOT TAIL"),
        "top-level merge field missing: {footnote}"
    );
    assert!(!footnote.contains("w:dirty="), "{footnote}");

    assert_eq!(endnote.matches("<w:tbl>").count(), 2, "{endnote}");
    assert_eq!(endnote.matches("<w:fldSimple").count(), 1, "{endnote}");
    assert!(
        endnote.contains(r#"<w:fldSimple w:instr=" MERGEFIELD ProjectName \* MERGEFORMAT ">"#)
            && endnote.contains("<w:i/>")
            && endnote.contains("PROJECT CACHE")
            && endnote.contains("END TAIL"),
        "nested merge field missing: {endnote}"
    );
    assert!(!endnote.contains("<w:fldChar"), "{endnote}");
    assert!(!endnote.contains("w:dirty="), "{endnote}");

    for rejected in [rejected_malformed, rejected_unknown] {
        assert!(!rejected.contains("<w:fldSimple"), "{rejected}");
        assert!(!rejected.contains("<w:fldChar"), "{rejected}");
        assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");
    }
    assert!(
        !footnotes.contains("MERGEFIELD \\* MERGEFORMAT"),
        "{footnotes}"
    );
    assert!(!endnotes.contains("CUSTOM payload"), "{endnotes}");

    let reopened = Document::open(&converted).expect("converted merge-field notes reopen");
    assert_eq!(reopened.report().features.fields, 2);
    assert!(reopened
        .report()
        .features
        .unsupported_field_reasons
        .is_empty());
    let fields = reopened.fields();
    assert_eq!(fields.len(), 2);
    assert!(fields
        .iter()
        .all(|field| field.kind == FieldKind::MergeField));
    assert_eq!(
        fields[0].instruction,
        r#"MERGEFIELD "Client Name" \* Upper"#
    );
    assert_eq!(fields[0].result, "CLIENT CACHE");
    assert_eq!(
        fields[1].instruction,
        r#"MERGEFIELD ProjectName \* MERGEFORMAT"#
    );
    assert_eq!(fields[1].result, "PROJECT CACHE");
    assert!(fields.iter().all(|field| field.computed_result.is_none()));

    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    for index in [0, 1, 3] {
        assert_eq!(reopened_model.blocks[index], normalized_model.blocks[index]);
    }
    let Block::Paragraph(rejected_malformed) = &reopened_model.blocks[2] else {
        panic!("rejected malformed-merge fallback paragraph")
    };
    assert_eq!(
        rejected_malformed.text(),
        "REJECTED MALFORMED MERGE FALLBACK"
    );
    let Block::Paragraph(rejected_unknown) = &reopened_model.blocks[4] else {
        panic!("rejected unknown-field fallback paragraph")
    };
    assert_eq!(rejected_unknown.text(), "REJECTED UNKNOWN FIELD FALLBACK");
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_inline_rasters_keep_media_and_relationship_ownership() {
    let png = tiny_png();
    let document = Document::open(&raster_note_docx()).expect("raster notes open");
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone raster normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let document_rels = std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).unwrap();
    assert!(parts.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(parts.contains_key("word/_rels/endnotes.xml.rels"));
    let footnote_rels = std::str::from_utf8(&parts["word/_rels/footnotes.xml.rels"]).unwrap();
    let endnote_rels = std::str::from_utf8(&parts["word/_rels/endnotes.xml.rels"]).unwrap();
    let content_types = std::str::from_utf8(&parts["[Content_Types].xml"]).unwrap();

    let foot_one = note_with_marker(footnotes, "footnote", "FOOT IMAGE ONE");
    let foot_two = note_with_marker(footnotes, "footnote", "FOOT NESTED IMAGE");
    let end_one = note_with_marker(endnotes, "endnote", "END IMAGE ONE");
    let end_two = note_with_marker(endnotes, "endnote", "END NESTED IMAGE");
    let rejected = note_with_marker(endnotes, "endnote", "REJECTED IMAGE");
    for namespace in ["xmlns:r=", "xmlns:wp=", "xmlns:a=", "xmlns:pic="] {
        assert!(footnotes.contains(namespace), "{footnotes}");
        assert!(endnotes.contains(namespace), "{endnotes}");
    }
    assert!(foot_one.contains(r#"<w:hyperlink r:id="rId1">"#));
    assert!(foot_one.contains(r#"r:embed="rId2""#));
    assert!(foot_one.contains(r#"descr="Foot &lt;one&gt;""#));
    assert!(foot_one.contains(r#"<wp:extent cx="19050" cy="28575"/>"#));
    assert!(foot_two.contains(r#"r:embed="rId3""#));
    assert_eq!(foot_two.matches("<w:tbl>").count(), 2, "{foot_two}");
    assert!(end_one.contains(r#"r:embed="rId1""#));
    assert!(end_one.contains(r#"<a:xfrm rot="5400000">"#));
    assert!(end_two.contains(r#"<w:hyperlink r:id="rId2">"#));
    assert!(end_two.contains(r#"r:embed="rId3""#));
    assert_eq!(end_two.matches("<w:tbl>").count(), 2, "{end_two}");
    assert!(!rejected.contains("<w:drawing>"), "{rejected}");
    assert!(!rejected.contains("<w:fldSimple"), "{rejected}");
    assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");

    assert!(document_rels.contains(r#"Target="media/image1.png""#));
    for target in ["image2.png", "image3.png", "image4.png", "image5.png"] {
        assert!(!document_rels.contains(target), "{document_rels}");
    }
    assert!(footnote_rels.contains(r#"Id="rId1""#));
    assert!(footnote_rels.contains(r#"Target="https://example.com/foot-image""#));
    assert!(footnote_rels.contains(r#"Id="rId2""#));
    assert!(footnote_rels.contains(r#"Target="media/image2.png""#));
    assert!(footnote_rels.contains(r#"Id="rId3""#));
    assert!(footnote_rels.contains(r#"Target="media/image4.png""#));
    assert!(endnote_rels.contains(r#"Id="rId1""#));
    assert!(endnote_rels.contains(r#"Target="media/image3.png""#));
    assert!(endnote_rels.contains(r#"Id="rId2""#));
    assert!(endnote_rels.contains(r#"Target="https://example.com/end-image""#));
    assert!(endnote_rels.contains(r#"Id="rId3""#));
    assert!(endnote_rels.contains(r#"Target="media/image5.png""#));
    assert!(!endnote_rels.contains("rejected"));
    assert!(content_types.contains(r#"Extension="png" ContentType="image/png""#));

    let media = parts
        .iter()
        .filter(|(name, _)| name.starts_with("word/media/"))
        .collect::<Vec<_>>();
    assert_eq!(media.len(), 5, "{:?}", parts.keys().collect::<Vec<_>>());
    for index in 1..=5 {
        assert_eq!(parts[&format!("word/media/image{index}.png")], png);
    }
    assert!(!parts.contains_key("word/media/image6.png"));

    let reopened = Document::open(&converted).expect("converted raster notes reopen");
    let reopened_model = reopened.model();
    assert_eq!(reopened_model.blocks.len(), normalized_model.blocks.len());
    assert_eq!(
        &reopened_model.blocks[..reopened_model.blocks.len() - 1],
        &normalized_model.blocks[..normalized_model.blocks.len() - 1]
    );
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}

#[test]
fn opened_docx_note_modeled_charts_keep_package_and_relationship_ownership() {
    let document = Document::open(&chart_note_docx()).expect("modeled chart notes open");
    let source_model = document.model();
    let standalone_bytes = rwml::write_docx(&source_model);
    let normalized_model = Document::open(&standalone_bytes)
        .expect("standalone chart normalization reopens")
        .model();
    let converted = document.to_docx();
    assert_eq!(converted, document.to_docx(), "conversion is deterministic");
    assert_eq!(document.model(), source_model);

    let parts = unzip_parts(&converted);
    let footnotes = std::str::from_utf8(&parts["word/footnotes.xml"]).unwrap();
    let endnotes = std::str::from_utf8(&parts["word/endnotes.xml"]).unwrap();
    let document_rels = std::str::from_utf8(&parts["word/_rels/document.xml.rels"]).unwrap();
    let footnote_rels = std::str::from_utf8(&parts["word/_rels/footnotes.xml.rels"]).unwrap();
    let endnote_rels = std::str::from_utf8(&parts["word/_rels/endnotes.xml.rels"]).unwrap();
    let content_types = std::str::from_utf8(&parts["[Content_Types].xml"]).unwrap();

    let foot_one = note_with_marker(footnotes, "footnote", "FOOT CHART ONE");
    let foot_two = note_with_marker(footnotes, "footnote", "FOOT NESTED CHART");
    let end_one = note_with_marker(endnotes, "endnote", "END CHART ONE");
    let end_two = note_with_marker(endnotes, "endnote", "END NESTED CHART");
    let rejected = note_with_marker(endnotes, "endnote", "REJECTED CHART");
    for namespace in ["xmlns:r=", "xmlns:wp=", "xmlns:a=", "xmlns:c="] {
        assert!(footnotes.contains(namespace), "{footnotes}");
    }
    assert!(!footnotes.contains("xmlns:cx="), "{footnotes}");
    for namespace in ["xmlns:r=", "xmlns:wp=", "xmlns:a=", "xmlns:cx="] {
        assert!(endnotes.contains(namespace), "{endnotes}");
    }
    assert!(!endnotes.contains("xmlns:c="), "{endnotes}");
    assert!(foot_one.contains(r#"<c:chart r:id="rId1"/>"#));
    assert!(foot_one.contains(r#"descr="Foot &lt;chart&gt;""#));
    assert!(foot_one.contains(r#"<wp:extent cx="3810000" cy="2286000"/>"#));
    assert!(foot_two.contains(r#"<c:chart r:id="rId2"/>"#));
    assert_eq!(foot_two.matches("<w:tbl>").count(), 2, "{foot_two}");
    assert!(end_one.contains(r#"<cx:chart r:id="rId1"/>"#));
    assert!(end_two.contains(r#"<cx:chart r:id="rId2"/>"#));
    assert_eq!(end_two.matches("<w:tbl>").count(), 2, "{end_two}");
    assert!(!rejected.contains("<w:drawing>"), "{rejected}");
    assert!(!rejected.contains("<w:fldSimple"), "{rejected}");
    assert_eq!(rejected.matches("<w:p>").count(), 1, "{rejected}");

    assert!(document_rels.contains(r#"Target="charts/chart1.xml""#));
    for target in ["chart2.xml", "chartEx3.xml", "chart4.xml", "chartEx5.xml"] {
        assert!(!document_rels.contains(target), "{document_rels}");
    }
    assert!(footnote_rels.contains(r#"Id="rId1""#));
    assert!(footnote_rels.contains(r#"Target="charts/chart2.xml""#));
    assert!(footnote_rels.contains(r#"Id="rId2""#));
    assert!(footnote_rels.contains(r#"Target="charts/chart4.xml""#));
    assert!(endnote_rels.contains(r#"Id="rId1""#));
    assert!(endnote_rels.contains(r#"Target="charts/chartEx3.xml""#));
    assert!(endnote_rels.contains(r#"Id="rId2""#));
    assert!(endnote_rels.contains(r#"Target="charts/chartEx5.xml""#));
    assert!(!endnote_rels.contains("chart6"));

    let chart_paths = parts
        .keys()
        .filter(|name| name.starts_with("word/charts/chart") && name.ends_with(".xml"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        chart_paths,
        [
            "word/charts/chart1.xml",
            "word/charts/chart2.xml",
            "word/charts/chart4.xml",
            "word/charts/chartEx3.xml",
            "word/charts/chartEx5.xml",
        ]
    );
    assert!(!parts.contains_key("word/charts/chart6.xml"));
    for index in [1, 2, 4] {
        let chart = std::str::from_utf8(&parts[&format!("word/charts/chart{index}.xml")]).unwrap();
        assert!(chart.contains("<a:t>Core source</a:t>"), "{chart}");
        assert!(chart.contains("<c:v>Core values</c:v>"), "{chart}");
        assert!(chart.contains("<c:v>18</c:v>"), "{chart}");
        assert!(parts.contains_key(&format!("word/charts/_rels/chart{index}.xml.rels")));
        assert!(parts.contains_key(&format!(
            "word/embeddings/Microsoft_Excel_Worksheet{index}.xlsx"
        )));
    }
    for index in [3, 5] {
        let chart =
            std::str::from_utf8(&parts[&format!("word/charts/chartEx{index}.xml")]).unwrap();
        assert!(chart.contains(r#"layoutId="waterfall""#), "{chart}");
        assert!(chart.contains("<a:t>Extended source</a:t>"), "{chart}");
        assert!(chart.contains("<cx:v>Extended values</cx:v>"), "{chart}");
        assert!(chart.contains("<cx:v>-7</cx:v>"), "{chart}");
        assert!(!parts.contains_key(&format!("word/charts/_rels/chartEx{index}.xml.rels")));
        assert!(!parts.contains_key(&format!(
            "word/embeddings/Microsoft_Excel_Worksheet{index}.xlsx"
        )));
    }
    assert_eq!(
        content_types
            .matches("application/vnd.openxmlformats-officedocument.drawingml.chart+xml")
            .count(),
        3
    );
    assert_eq!(
        content_types
            .matches("application/vnd.ms-office.chartex+xml")
            .count(),
        2
    );

    let reopened = Document::open(&converted).expect("converted chart notes reopen");
    let reopened_model = reopened.model();
    assert_eq!(
        reopened_model.blocks.len() + 1,
        normalized_model.blocks.len()
    );
    let supported_end = reopened_model.blocks.len() - 1;
    assert_eq!(
        &reopened_model.blocks[..supported_end],
        &normalized_model.blocks[..supported_end]
    );
    let Block::Paragraph(rejected_fallback) = &reopened_model.blocks[supported_end] else {
        panic!("rejected chart note fallback paragraph")
    };
    assert_eq!(rejected_fallback.text(), "REJECTED CHART UNSUPPORTED FIELD");
    assert_eq!(reopened.to_docx(), converted);

    let standalone = unzip_parts(&standalone_bytes);
    assert!(!standalone.contains_key("word/footnotes.xml"));
    assert!(!standalone.contains_key("word/endnotes.xml"));
    assert!(!standalone.contains_key("word/_rels/footnotes.xml.rels"));
    assert!(!standalone.contains_key("word/_rels/endnotes.xml.rels"));
}
