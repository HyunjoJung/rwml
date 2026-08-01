#![cfg(feature = "docx")]

use std::io::Write;

use rwml::{Block, Color, Document};

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

fn content_types(include_styles: bool) -> String {
    let styles = if include_styles {
        r#"<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>"#
    } else {
        ""
    };
    format!(
        r#"<?xml version="1.0"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>{styles}</Types>"#
    )
}

fn document_rels(include_styles: bool) -> &'static str {
    if include_styles {
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/></Relationships>"#
    } else {
        r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"/>"#
    }
}

fn style_inheritance_docx() -> Vec<u8> {
    let content_types = content_types(true);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:docDefaults>
                    <w:rPrDefault><w:rPr><w:b w:val="0"/><w:sz w:val="20"/></w:rPr></w:rPrDefault>
                </w:docDefaults>
                <w:style w:type="paragraph" w:styleId="Normal">
                    <w:name w:val="Normal"/>
                    <w:rPr><w:color w:val="336699"/></w:rPr>
                </w:style>
                <w:style w:type="paragraph" w:styleId="Heading1">
                    <w:name w:val="heading 1"/>
                    <w:basedOn w:val="Normal"/>
                    <w:rPr><w:b/></w:rPr>
                </w:style>
                <w:style w:type="character" w:styleId="Em">
                    <w:name w:val="Em"/>
                    <w:rPr><w:i/></w:rPr>
                </w:style>
            </w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p>
                    <w:pPr><w:pStyle w:val="Heading1"/></w:pPr>
                    <w:r><w:t>plain</w:t></w:r>
                    <w:r><w:rPr><w:rStyle w:val="Em"/></w:rPr><w:t>em</w:t></w:r>
                    <w:r><w:rPr><w:b w:val="0"/></w:rPr><w:t>off</w:t></w:r>
                </w:p>
            </w:body></w:document>"#,
        ),
    ])
}

fn no_styles_docx() -> Vec<u8> {
    let content_types = content_types(false);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(false)),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>plain</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>direct</w:t></w:r></w:p></w:body></w:document>"#,
        ),
    ])
}

fn default_paragraph_style_docx() -> Vec<u8> {
    let content_types = content_types(true);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:docDefaults>
                    <w:rPrDefault><w:rPr><w:i/></w:rPr></w:rPrDefault>
                    <w:pPrDefault><w:pPr><w:spacing w:before="60"/></w:pPr></w:pPrDefault>
                </w:docDefaults>
                <w:style w:default="true" w:styleId="Normal">
                    <w:pPr>
                        <w:spacing w:before="240" w:after="120" w:line="360"/>
                        <w:ind w:firstLine="200"/>
                        <w:shd w:val="clear" w:fill="112233"/>
                        <w:pageBreakBefore/>
                    </w:pPr>
                    <w:rPr><w:b/></w:rPr>
                </w:style>
                <w:style w:type="paragraph" w:styleId="Explicit">
                    <w:pPr><w:spacing w:before="360"/></w:pPr>
                </w:style>
            </w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:r><w:t>implicit</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Explicit"/></w:pPr><w:r><w:t>explicit</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Missing"/></w:pPr><w:r><w:t>missing</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="  "/></w:pPr><w:r><w:t>empty</w:t></w:r></w:p>
            </w:body></w:document>"#,
        ),
    ])
}

fn paragraph_layout_inheritance_docx() -> Vec<u8> {
    let content_types = content_types(true);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:docDefaults><w:pPrDefault><w:pPr>
                    <w:spacing w:before="120" w:after="240" w:line="300"/>
                    <w:ind w:firstLine="200"/>
                    <w:shd w:val="clear" w:fill="112233"/>
                    <w:pageBreakBefore/>
                </w:pPr></w:pPrDefault></w:docDefaults>
                <w:style w:type="paragraph" w:styleId="Base">
                    <w:pPr>
                        <w:spacing w:before="0" w:after="360" w:line="360" w:lineRule="auto"/>
                        <w:ind w:hanging="240"/>
                        <w:shd w:val="clear" w:fill="445566"/>
                    </w:pPr>
                </w:style>
                <w:style w:type="paragraph" w:styleId="Derived">
                    <w:basedOn w:val="Base"/>
                    <w:pPr>
                        <w:spacing w:before="480"/>
                        <w:ind w:firstLine="320"/>
                        <w:shd w:val="clear" w:fill="778899"/>
                    </w:pPr>
                </w:style>
                <w:style w:type="paragraph" w:styleId="Blocked">
                    <w:basedOn w:val="Base"/>
                    <w:pPr>
                        <w:spacing w:beforeLines="100" w:afterLines="100"
                                   w:line="480" w:lineRule="exact"/>
                        <w:ind w:hangingChars="100"/>
                        <w:shd w:val="pct20" w:fill="ABCDEF"/>
                        <w:pageBreakBefore w:val="0"/>
                    </w:pPr>
                </w:style>
            </w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:r><w:t>defaults</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Base"/></w:pPr><w:r><w:t>base</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Derived"/></w:pPr><w:r><w:t>derived</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Derived"/>
                    <w:spacing w:before="0" w:after="0" w:line="240"/>
                    <w:ind w:hanging="0"/>
                    <w:shd w:val="clear" w:fill="000000"/>
                    <w:pageBreakBefore w:val="0"/>
                </w:pPr><w:r><w:t>direct</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Blocked"/></w:pPr><w:r><w:t>style blocked</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Derived"/>
                    <w:spacing w:beforeLines="100" w:afterLines="100"
                               w:line="480" w:lineRule="atLeast"/>
                    <w:ind w:firstLineChars="100"/>
                    <w:shd w:themeFill="accent1" w:fill="DDEEFF"/>
                    <w:pageBreakBefore w:val="off"/>
                </w:pPr><w:r><w:t>direct blocked</w:t></w:r></w:p>
            </w:body></w:document>"#,
        ),
    ])
}

fn paragraph_layout_cascade_docx() -> Vec<u8> {
    let content_types = content_types(true);
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:style w:type="paragraph" w:styleId="Automatic"><w:pPr>
                    <w:spacing w:before="120" w:beforeAutospacing="1"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="Exact"><w:pPr>
                    <w:spacing w:line="360" w:lineRule="exact"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="CharacterIndent"><w:pPr>
                    <w:ind w:hangingChars="100"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="LineUnits"><w:pPr>
                    <w:spacing w:before="120" w:beforeLines="100"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="FirstLineChars"><w:pPr>
                    <w:ind w:firstLineChars="100"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="HangingChars"><w:pPr>
                    <w:ind w:hangingChars="100"/>
                </w:pPr></w:style>
                <w:style w:type="paragraph" w:styleId="Break"><w:pPr>
                    <w:pageBreakBefore/>
                </w:pPr></w:style>
            </w:styles>"#,
        ),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:pStyle w:val="Automatic"/>
                    <w:spacing w:before="480"/>
                </w:pPr><w:r><w:t>automatic retained</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Automatic"/>
                    <w:spacing w:beforeAutospacing="0"/>
                </w:pPr><w:r><w:t>automatic disabled</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Exact"/>
                    <w:spacing w:line="480"/>
                </w:pPr><w:r><w:t>implicit auto</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Exact"/>
                    <w:spacing w:lineRule="auto"/>
                </w:pPr><w:r><w:t>auto override</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="CharacterIndent"/>
                    <w:ind w:firstLine="240"/>
                </w:pPr><w:r><w:t>character unit retained</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="Break"/>
                    <w:pageBreakBefore w:val="TRUE"/>
                </w:pPr><w:r><w:t>invalid toggle</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="LineUnits"/>
                    <w:spacing w:beforeLines="-0"/>
                </w:pPr><w:r><w:t>line units cleared</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="FirstLineChars"/>
                    <w:ind w:firstLineChars="-0" w:firstLine="240"/>
                </w:pPr><w:r><w:t>character units cleared</w:t></w:r></w:p>
                <w:p><w:pPr><w:spacing w:afterLines="0"/></w:pPr>
                    <w:r><w:t>standalone line unit zero</w:t></w:r></w:p>
                <w:p><w:pPr><w:pStyle w:val="HangingChars"/>
                    <w:ind w:hangingChars="+0" w:hanging="240"/>
                </w:pPr><w:r><w:t>hanging character units cleared</w:t></w:r></w:p>
            </w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "render")]
fn paragraph_layout_render_docx(direct_page_break: &str) -> Vec<u8> {
    let content_types = content_types(true);
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
            <w:p><w:r><w:t>first page</w:t></w:r></w:p>
            <w:p><w:pPr><w:pStyle w:val="Rendered"/>{direct_page_break}</w:pPr>
                <w:r><w:t>styled second paragraph wraps across its inherited line box</w:t></w:r>
            </w:p>
            <w:sectPr><w:pgSz w:w="6000" w:h="6000"/>
                <w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/>
            </w:sectPr>
        </w:body></w:document>"#
    );
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        (
            "word/styles.xml",
            r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
                <w:style w:type="paragraph" w:styleId="Rendered"><w:pPr>
                    <w:spacing w:before="240" w:after="120" w:line="360"/>
                    <w:ind w:firstLine="240"/>
                    <w:shd w:val="clear" w:fill="DDEEFF"/>
                    <w:pageBreakBefore/>
                </w:pPr></w:style>
            </w:styles>"#,
        ),
        ("word/document.xml", &document_xml),
    ])
}

#[cfg(feature = "render")]
fn paragraph_layout_render_variant_docx(style_properties: &str) -> Vec<u8> {
    let content_types = content_types(true);
    let styles_xml = format!(
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:style w:type="paragraph" w:styleId="Rendered"><w:pPr>
                {style_properties}
            </w:pPr></w:style>
        </w:styles>"#
    );
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        ("word/styles.xml", &styles_xml),
        (
            "word/document.xml",
            r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
                <w:p><w:pPr><w:pStyle w:val="Rendered"/></w:pPr>
                    <w:r><w:t>styled paragraph wraps over several words for stable layout evidence</w:t></w:r>
                </w:p>
                <w:p><w:r><w:t>following paragraph</w:t></w:r></w:p>
                <w:sectPr><w:pgSz w:w="4400" w:h="6000"/>
                    <w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/>
                </w:sectPr>
            </w:body></w:document>"#,
        ),
    ])
}

#[cfg(feature = "render")]
fn table_pagination_docx(
    table_properties: &str,
    direct_row_props: &str,
    styles_xml: &str,
) -> Vec<u8> {
    let content_types = content_types(true);
    let document_xml = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>
            <w:p><w:pPr><w:spacing w:line="800" w:lineRule="exact"/></w:pPr><w:r><w:t>seed</w:t></w:r></w:p>
            <w:tbl><w:tblPr>{table_properties}</w:tblPr><w:tr>{direct_row_props}<w:tc>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>one</w:t></w:r></w:p>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>two</w:t></w:r></w:p>
                <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>three</w:t></w:r></w:p>
                <w:p><w:r><w:t>four</w:t></w:r></w:p>
                <w:p><w:r><w:t>five</w:t></w:r></w:p>
            </w:tc></w:tr></w:tbl>
            <w:p><w:pPr><w:spacing w:line="400" w:lineRule="exact"/></w:pPr><w:r><w:t>after</w:t></w:r></w:p>
            <w:sectPr><w:pgSz w:w="4400" w:h="2400"/><w:pgMar w:top="400" w:right="400" w:bottom="400" w:left="400"/></w:sectPr>
        </w:body></w:document>"#
    );
    docx_fixture(&[
        ("[Content_Types].xml", &content_types),
        (
            "_rels/.rels",
            r#"<?xml version="1.0"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#,
        ),
        ("word/_rels/document.xml.rels", document_rels(true)),
        ("word/styles.xml", styles_xml),
        ("word/document.xml", &document_xml),
    ])
}

#[cfg(feature = "render")]
fn table_style_pagination_docx(direct_row_props: &str) -> Vec<u8> {
    table_pagination_docx(
        r#"<w:tblStyle w:val="KeepDerived"/>"#,
        direct_row_props,
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:style w:type="table" w:styleId="KeepBase"><w:trPr><w:cantSplit/></w:trPr></w:style>
            <w:style w:type="table" w:styleId="KeepDerived"><w:basedOn w:val="KeepBase"/></w:style>
        </w:styles>"#,
    )
}

#[cfg(feature = "render")]
fn conditional_table_style_pagination_docx(direct_row_props: &str) -> Vec<u8> {
    table_pagination_docx(
        r#"<w:tblStyle w:val="ConditionalKeep"/><w:tblLook w:firstRow="1"/>"#,
        direct_row_props,
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:style w:type="table" w:styleId="ConditionalKeep">
                <w:tblStylePr w:type="firstRow"><w:trPr><w:cantSplit/></w:trPr></w:tblStylePr>
            </w:style>
        </w:styles>"#,
    )
}

#[cfg(feature = "render")]
fn horizontal_band_table_style_pagination_docx(direct_row_props: &str) -> Vec<u8> {
    table_pagination_docx(
        r#"<w:tblStyle w:val="BandedKeep"/>"#,
        direct_row_props,
        r#"<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
            <w:style w:type="table" w:styleId="BandedKeep">
                <w:tblPr><w:tblStyleRowBandSize w:val="1"/></w:tblPr>
                <w:tblStylePr w:type="band1Horz">
                    <w:trPr><w:cantSplit/></w:trPr>
                </w:tblStylePr>
            </w:style>
        </w:styles>"#,
    )
}

#[test]
fn docx_run_props_resolve_docdefaults_paragraph_and_character_styles() {
    let doc = Document::open(&style_inheritance_docx()).expect("fixture opens");
    let model = doc.model();
    let [Block::Paragraph(paragraph)] = model.blocks.as_slice() else {
        panic!("expected one paragraph");
    };
    assert_eq!(paragraph.text(), "plainemoff");
    let [plain, em, off] = paragraph.runs.as_slice() else {
        panic!("expected three runs");
    };

    assert!(plain.props.bold);
    assert!(!plain.props.italic);
    assert_eq!(plain.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(plain.props.size_half_pt, Some(20));

    assert!(em.props.bold);
    assert!(em.props.italic);
    assert_eq!(em.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(em.props.size_half_pt, Some(20));

    assert!(!off.props.bold);
    assert!(!off.props.italic);
    assert_eq!(off.props.color, Some(Color::rgb(0x33, 0x66, 0x99)));
    assert_eq!(off.props.size_half_pt, Some(20));
}

#[test]
fn docx_without_styles_part_keeps_run_defaults_unchanged() {
    let doc = Document::open(&no_styles_docx()).expect("fixture opens");
    let model = doc.model();
    let [Block::Paragraph(paragraph)] = model.blocks.as_slice() else {
        panic!("expected one paragraph");
    };
    let [plain, direct] = paragraph.runs.as_slice() else {
        panic!("expected two runs");
    };

    assert_eq!(plain.text, "plain");
    assert_eq!(plain.props, Default::default());
    assert_eq!(direct.text, "direct");
    assert!(direct.props.italic);
    assert!(!direct.props.bold);
    assert_eq!(direct.props.size_half_pt, None);
}

#[test]
fn docx_default_paragraph_style_applies_only_to_unstyled_paragraphs() {
    let doc = Document::open(&default_paragraph_style_docx()).expect("fixture opens");
    let model = doc.model();
    let paragraphs = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 4);

    let implicit = paragraphs[0];
    assert_eq!(implicit.props.spacing.before_pt, Some(12.0));
    assert_eq!(implicit.props.spacing.after_pt, Some(6.0));
    assert_eq!(implicit.props.spacing.line_pct, Some(1.5));
    assert_eq!(implicit.props.indent.first_line_pt, Some(10.0));
    assert_eq!(implicit.props.shading, Some(Color::rgb(0x11, 0x22, 0x33)));
    assert!(implicit.props.page_break_before);
    assert!(implicit.runs[0].props.bold);
    assert!(implicit.runs[0].props.italic);

    let explicit = paragraphs[1];
    assert_eq!(explicit.props.spacing.before_pt, Some(18.0));
    assert_eq!(explicit.props.spacing.after_pt, None);
    assert_eq!(explicit.props.spacing.line_pct, None);
    assert!(!explicit.props.page_break_before);
    assert!(!explicit.runs[0].props.bold);
    assert!(explicit.runs[0].props.italic);

    let missing = paragraphs[2];
    assert_eq!(missing.props.spacing.before_pt, Some(3.0));
    assert_eq!(missing.props.spacing.after_pt, None);
    assert!(!missing.props.page_break_before);
    assert!(!missing.runs[0].props.bold);
    assert!(missing.runs[0].props.italic);

    let empty = paragraphs[3];
    assert_eq!(empty.props.spacing.before_pt, Some(3.0));
    assert_eq!(empty.props.style_id, None);
    assert!(!empty.props.page_break_before);
    assert!(!empty.runs[0].props.bold);
    assert!(empty.runs[0].props.italic);
}

#[test]
fn docx_paragraph_layout_resolves_defaults_styles_and_direct_overrides() {
    let doc = Document::open(&paragraph_layout_inheritance_docx()).expect("fixture opens");
    let model = doc.model();
    let paragraphs = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 6);

    let defaults = paragraphs[0];
    assert_eq!(defaults.props.spacing.before_pt, Some(6.0));
    assert_eq!(defaults.props.spacing.after_pt, Some(12.0));
    assert_eq!(defaults.props.spacing.line_pct, Some(1.25));
    assert_eq!(defaults.props.indent.first_line_pt, Some(10.0));
    assert_eq!(defaults.props.indent.hanging_pt, None);
    assert_eq!(defaults.props.shading, Some(Color::rgb(0x11, 0x22, 0x33)));
    assert!(defaults.props.page_break_before);

    let base = paragraphs[1];
    assert_eq!(base.props.spacing.before_pt, Some(0.0));
    assert_eq!(base.props.spacing.after_pt, Some(18.0));
    assert_eq!(base.props.spacing.line_pct, Some(1.5));
    assert_eq!(base.props.indent.first_line_pt, None);
    assert_eq!(base.props.indent.hanging_pt, Some(12.0));
    assert_eq!(base.props.shading, Some(Color::rgb(0x44, 0x55, 0x66)));
    assert!(base.props.page_break_before);

    let derived = paragraphs[2];
    assert_eq!(derived.props.spacing.before_pt, Some(24.0));
    assert_eq!(derived.props.spacing.after_pt, Some(18.0));
    assert_eq!(derived.props.spacing.line_pct, Some(1.5));
    assert_eq!(derived.props.indent.first_line_pt, Some(16.0));
    assert_eq!(derived.props.indent.hanging_pt, None);
    assert_eq!(derived.props.shading, Some(Color::rgb(0x77, 0x88, 0x99)));
    assert!(derived.props.page_break_before);

    let direct = paragraphs[3];
    assert_eq!(direct.props.spacing.before_pt, Some(0.0));
    assert_eq!(direct.props.spacing.after_pt, Some(0.0));
    assert_eq!(direct.props.spacing.line_pct, Some(1.0));
    assert_eq!(direct.props.indent.first_line_pt, None);
    assert_eq!(direct.props.indent.hanging_pt, Some(0.0));
    assert_eq!(direct.props.shading, Some(Color::rgb(0, 0, 0)));
    assert!(!direct.props.page_break_before);

    for blocked in [&paragraphs[4], &paragraphs[5]] {
        assert_eq!(blocked.props.spacing.before_pt, None);
        assert_eq!(blocked.props.spacing.after_pt, None);
        assert_eq!(blocked.props.spacing.line_pct, None);
        assert_eq!(blocked.props.indent.first_line_pt, None);
        assert_eq!(blocked.props.indent.hanging_pt, None);
        assert_eq!(blocked.props.shading, None);
        assert!(!blocked.props.page_break_before);
    }
}

#[test]
fn docx_paragraph_layout_cascades_dependent_direct_attributes() {
    let doc = Document::open(&paragraph_layout_cascade_docx()).expect("fixture opens");
    let model = doc.model();
    let paragraphs = model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(paragraphs.len(), 10);

    assert_eq!(paragraphs[0].props.spacing.before_pt, None);
    assert_eq!(paragraphs[1].props.spacing.before_pt, Some(6.0));
    assert_eq!(paragraphs[2].props.spacing.line_pct, Some(2.0));
    assert_eq!(paragraphs[3].props.spacing.line_pct, Some(1.5));
    assert_eq!(paragraphs[4].props.indent.first_line_pt, None);
    assert_eq!(paragraphs[4].props.indent.hanging_pt, None);
    assert!(!paragraphs[5].props.page_break_before);
    assert_eq!(paragraphs[6].props.spacing.before_pt, Some(6.0));
    assert_eq!(paragraphs[7].props.indent.first_line_pt, Some(12.0));
    assert_eq!(paragraphs[7].props.indent.hanging_pt, None);
    assert_eq!(paragraphs[8].props.spacing.after_pt, Some(0.0));
    assert_eq!(paragraphs[9].props.indent.first_line_pt, None);
    assert_eq!(paragraphs[9].props.indent.hanging_pt, Some(12.0));
}

#[test]
fn style_derived_paragraph_layout_survives_fresh_docx_conversion() {
    let source = Document::open(&paragraph_layout_inheritance_docx()).expect("fixture opens");
    let source_model = source.model();
    let converted = rwml::write_docx(&source_model);
    let reopened = Document::open(&converted).expect("converted document reopens");
    let reopened_model = reopened.model();
    let source_paragraphs = source_model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect::<Vec<_>>();
    let reopened_paragraphs = reopened_model
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Paragraph(paragraph) => Some(paragraph),
            _ => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(source_model.blocks.len(), 6);
    assert_eq!(reopened_model.blocks.len(), 6);
    assert_eq!(source_paragraphs.len(), source_model.blocks.len());
    assert_eq!(reopened_paragraphs.len(), reopened_model.blocks.len());
    for (source, reopened) in source_paragraphs.iter().zip(&reopened_paragraphs) {
        assert_eq!(reopened.props.spacing, source.props.spacing);
        assert_eq!(reopened.props.indent, source.props.indent);
        assert_eq!(reopened.props.shading, source.props.shading);
        assert_eq!(
            reopened.props.page_break_before,
            source.props.page_break_before
        );
    }
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_honors_style_layout_and_direct_page_break_off() {
    let inherited =
        Document::open(&paragraph_layout_render_docx("")).expect("styled fixture opens");
    let direct_off = Document::open(&paragraph_layout_render_docx(
        r#"<w:pageBreakBefore w:val="off"/>"#,
    ))
    .expect("direct-off fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let inherited_layout = inherited
        .layout_pages_with_fonts(&fonts)
        .expect("inherited layout succeeds");
    let direct_off_layout = direct_off
        .layout_pages_with_fonts(&fonts)
        .expect("direct-off layout succeeds");
    assert_eq!((inherited_layout.pages, direct_off_layout.pages), (2, 1));

    let first_pdf = inherited.to_pdf_with_fonts(&fonts);
    let second_pdf = inherited.to_pdf_with_fonts(&fonts);
    assert!(first_pdf.starts_with(b"%PDF-"));
    assert_eq!(first_pdf, second_pdf);
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_consumes_each_style_layout_value() {
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];
    let render = |properties: &str| {
        Document::open(&paragraph_layout_render_variant_docx(properties))
            .expect("variant opens")
            .to_pdf_with_fonts(&fonts)
    };
    let baseline = render("");
    assert_eq!(baseline, render(""));

    for (name, properties) in [
        ("before spacing", r#"<w:spacing w:before="240"/>"#),
        ("after spacing", r#"<w:spacing w:after="240"/>"#),
        (
            "proportional line spacing",
            r#"<w:spacing w:line="480" w:lineRule="auto"/>"#,
        ),
        ("first-line indent", r#"<w:ind w:firstLine="360"/>"#),
        ("flat shading", r#"<w:shd w:val="clear" w:fill="DDEEFF"/>"#),
    ] {
        let rendered = render(properties);
        assert!(rendered.starts_with(b"%PDF-"), "{name}");
        assert_ne!(rendered, baseline, "{name} must affect PDF output");
        assert_eq!(rendered, render(properties), "{name} must be deterministic");
    }
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_honors_inherited_table_style_cant_split_and_direct_off() {
    let inherited = Document::open(&table_style_pagination_docx("")).expect("fixture opens");
    let direct_off = Document::open(&table_style_pagination_docx(
        r#"<w:trPr><w:cantSplit w:val="off"/></w:trPr>"#,
    ))
    .expect("fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let inherited_pages = inherited
        .layout_pages_with_fonts(&fonts)
        .expect("inherited table style lays out")
        .pages;
    let direct_off_pages = direct_off
        .layout_pages_with_fonts(&fonts)
        .expect("direct override lays out")
        .pages;

    assert_eq!(
        (inherited_pages, direct_off_pages),
        (3, 2),
        "base table style keeps the row together while direct off restores splitting"
    );
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_honors_first_row_conditional_table_style_cant_split() {
    let conditional =
        Document::open(&conditional_table_style_pagination_docx("")).expect("fixture opens");
    let direct_off = Document::open(&conditional_table_style_pagination_docx(
        r#"<w:trPr><w:cantSplit w:val="off"/></w:trPr>"#,
    ))
    .expect("fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let conditional_pages = conditional
        .layout_pages_with_fonts(&fonts)
        .expect("conditional table style lays out")
        .pages;
    let direct_off_pages = direct_off
        .layout_pages_with_fonts(&fonts)
        .expect("direct override lays out")
        .pages;

    assert_eq!(
        (conditional_pages, direct_off_pages),
        (3, 2),
        "the selected first-row style keeps the row together while direct off restores splitting"
    );
}

#[cfg(feature = "render")]
#[test]
fn opened_docx_render_honors_horizontal_table_style_band_cant_split() {
    let banded =
        Document::open(&horizontal_band_table_style_pagination_docx("")).expect("fixture opens");
    let direct_off = Document::open(&horizontal_band_table_style_pagination_docx(
        r#"<w:trPr><w:cantSplit w:val="off"/></w:trPr>"#,
    ))
    .expect("fixture opens");
    let fonts = vec![rwml_fonts::noto_sans_kr_subset().to_vec()];

    let banded_pages = banded
        .layout_pages_with_fonts(&fonts)
        .expect("banded table style lays out")
        .pages;
    let direct_off_pages = direct_off
        .layout_pages_with_fonts(&fonts)
        .expect("direct override lays out")
        .pages;

    assert_eq!(
        (banded_pages, direct_off_pages),
        (3, 2),
        "the selected horizontal band keeps the row together while direct off restores splitting"
    );
}
