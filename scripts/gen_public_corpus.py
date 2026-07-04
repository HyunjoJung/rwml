#!/usr/bin/env python3
"""Generate a small, license-clean PUBLIC .docx corpus for rwml validation.

Every file here is authored from scratch (raw OOXML) by this script, so the output is
trivially license-clean (you own it) and can be committed to the public repo. The files
deliberately carry the *unmodeled* content a package-preserving editor must round-trip
intact: tracked changes (w:ins/w:del), content controls (w:sdt), text boxes
(mc:AlternateContent + w:txbxContent), footnotes, comments, headers/footers, fields,
hyperlinks, unsupported object markers, tables, floating shape placement metadata, and
an inline PNG image.

Usage:
    python scripts/gen_public_corpus.py            # writes corpus/public/synthetic/*.docx
    python scripts/gen_public_corpus.py --check     # also re-open each with python-docx

Output is deterministic (fixed zip member order, no timestamps) so regeneration is a no-op
in git.
"""
from __future__ import annotations
import os
import sys
import zipfile

OUT_DIR = os.path.join(os.path.dirname(__file__), "..", "corpus", "public", "synthetic")

W = "http://schemas.openxmlformats.org/wordprocessingml/2006/main"
R = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
C = "http://schemas.openxmlformats.org/drawingml/2006/chart"
MC = "http://schemas.openxmlformats.org/markup-compatibility/2006"
O = "urn:schemas-microsoft-com:office:office"

XML_DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

# A genuinely valid 2x3 RGB PNG (correct chunk CRCs + a real zlib IDAT). Same bytes the
# in-tree validate_edit example uses, so rwml's CRC-checked is_png accepts it.
TINY_PNG = bytes([
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
    0x52, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x03, 0x08, 0x02, 0x00, 0x00, 0x00, 0x36,
    0x88, 0x49, 0xD6, 0x00, 0x00, 0x00, 0x0B, 0x49, 0x44, 0x41, 0x54, 0x78, 0xDA, 0x63, 0x60,
    0xC0, 0x02, 0x00, 0x00, 0x15, 0x00, 0x01, 0x39, 0xC1, 0xE0, 0x23, 0x00, 0x00, 0x00, 0x00,
    0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
])

RELS_CT = "application/vnd.openxmlformats-package.relationships+xml"
MAIN_CT = "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"


def _zip(parts: list[tuple[str, bytes]]) -> bytes:
    """Build a deterministic .docx zip from an ordered (name, bytes) list."""
    import io
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        for name, data in parts:
            zi = zipfile.ZipInfo(name)  # fixed default date (1980-01-01), no per-run drift
            zi.compress_type = zipfile.ZIP_DEFLATED
            z.writestr(zi, data)
    return buf.getvalue()


def _b(s: str) -> bytes:
    return s.encode("utf-8")


def _content_types(overrides: list[tuple[str, str]], defaults: list[tuple[str, str]]) -> bytes:
    d = "".join(f'<Default Extension="{e}" ContentType="{c}"/>' for e, c in defaults)
    o = "".join(f'<Override PartName="{p}" ContentType="{c}"/>' for p, c in overrides)
    return _b(
        XML_DECL
        + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
        + d + o + "</Types>"
    )


def _rels(entries: list[tuple[str, str, str] | tuple[str, str, str, str]]) -> bytes:
    rels = []
    for entry in entries:
        if len(entry) == 4:
            i, t, tg, mode = entry
            rels.append(f'<Relationship Id="{i}" Type="{t}" Target="{tg}" TargetMode="{mode}"/>')
        else:
            i, t, tg = entry
            rels.append(f'<Relationship Id="{i}" Type="{t}" Target="{tg}"/>')
    body = "".join(rels)
    return _b(
        XML_DECL
        + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + body + "</Relationships>"
    )


def _minimal_docx(
    document: bytes,
    *,
    doc_rels: list[tuple[str, str, str] | tuple[str, str, str, str]] | None = None,
    overrides: list[tuple[str, str]] | None = None,
    defaults: list[tuple[str, str]] | None = None,
    extra_parts: list[tuple[str, bytes]] | None = None,
) -> bytes:
    doc_rels = doc_rels or []
    overrides = overrides or []
    defaults = defaults or []
    extra_parts = extra_parts or []
    ct = _content_types(
        overrides=[("/word/document.xml", MAIN_CT)] + overrides,
        defaults=[
            ("rels", RELS_CT),
            ("xml", "application/xml"),
        ] + defaults,
    )
    parts = [
        ("[Content_Types].xml", ct),
        ("_rels/.rels", _rels([("rId1", f"{R}/officeDocument", "word/document.xml")])),
        ("word/document.xml", document),
    ]
    if doc_rels:
        parts.append(("word/_rels/document.xml.rels", _rels(doc_rels)))
    parts.extend(extra_parts)
    return _zip(parts)


def kitchen_sink() -> bytes:
    """Body with tracked changes, content control, text box, footnote ref, comment range,
    a table, an inline image, and a header/footer. Exercises the editor's unmodeled-content
    preservation broadly."""
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}" '
        'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" '
        'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" '
        'xmlns:v="urn:schemas-microsoft-com:vml" '
        'mc:Ignorable="wps wp a pic">'
        "<w:body>"
        # tracked changes: an insertion and a deletion
        '<w:p><w:ins w:id="1" w:author="rwml" w:date="2020-01-01T00:00:00Z">'
        "<w:r><w:t>INSERTED </w:t></w:r></w:ins>"
        '<w:del w:id="2" w:author="rwml" w:date="2020-01-01T00:00:00Z">'
        "<w:r><w:delText>DELETED</w:delText></w:r></w:del>"
        "<w:r><w:t>PLAIN OLD</w:t></w:r></w:p>"
        # content control (structured document tag)
        "<w:sdt><w:sdtPr><w:alias w:val=\"cc\"/></w:sdtPr>"
        "<w:sdtContent><w:p><w:r><w:t>INSIDE SDT</w:t></w:r></w:p></w:sdtContent></w:sdt>"
        # comment range around a run
        '<w:p><w:commentRangeStart w:id="0"/><w:r><w:t>COMMENTED</w:t></w:r>'
        '<w:commentRangeEnd w:id="0"/>'
        '<w:r><w:commentReference w:id="0"/></w:r></w:p>'
        # footnote reference
        '<w:p><w:r><w:t>Footnoted</w:t></w:r>'
        '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>'
        '<w:footnoteReference w:id="2"/></w:r></w:p>'
        # text box via mc:AlternateContent (the unmodeled shape content)
        "<w:p><w:r><mc:AlternateContent><mc:Choice Requires=\"wps\">"
        "<w:drawing><wp:inline distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\">"
        '<wp:extent cx="1000000" cy="500000"/>'
        '<wp:docPr id="100" name="TextBox"/>'
        "<a:graphic><a:graphicData uri=\"http://schemas.microsoft.com/office/word/2010/wordprocessingShape\">"
        "<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>IN TEXTBOX</w:t></w:r></w:p>"
        "</w:txbxContent></wps:txbx>"
        '<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:inline></w:drawing>'
        "</mc:Choice><mc:Fallback><w:r><w:t>[textbox]</w:t></w:r></mc:Fallback>"
        "</mc:AlternateContent></w:r></w:p>"
        # a table
        "<w:tbl><w:tblPr><w:tblW w:w=\"0\" w:type=\"auto\"/></w:tblPr>"
        "<w:tr><w:tc><w:p><w:r><w:t>A1</w:t></w:r></w:p></w:tc>"
        "<w:tc><w:p><w:r><w:t>B1</w:t></w:r></w:p></w:tc></w:tr></w:tbl>"
        # inline image
        '<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">'
        '<wp:extent cx="190500" cy="285750"/><wp:docPr id="1" name="img"/>'
        "<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\">"
        "<pic:pic><pic:nvPicPr><pic:cNvPr id=\"0\" name=\"img\"/><pic:cNvPicPr/></pic:nvPicPr>"
        '<pic:blipFill><a:blip r:embed="rId10"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
        "<pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"190500\" cy=\"285750\"/></a:xfrm>"
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>'
        "</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        # section with header/footer references
        '<w:sectPr><w:headerReference w:type="default" r:id="rId20"/>'
        '<w:footerReference w:type="default" r:id="rId21"/>'
        '<w:pgSz w:w="12240" w:h="15840"/></w:sectPr>'
        "</w:body></w:document>"
    )
    footnotes = _b(
        XML_DECL + f'<w:footnotes xmlns:w="{W}">'
        '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
        '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
        '<w:footnote w:id="2"><w:p><w:r><w:t>A footnote.</w:t></w:r></w:p></w:footnote>'
        "</w:footnotes>"
    )
    comments = _b(
        XML_DECL + f'<w:comments xmlns:w="{W}">'
        '<w:comment w:id="0" w:author="rwml" w:date="2020-01-01T00:00:00Z" w:initials="r">'
        "<w:p><w:r><w:t>A comment.</w:t></w:r></w:p></w:comment></w:comments>"
    )
    header = _b(XML_DECL + f'<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>HEADER</w:t></w:r></w:p></w:hdr>')
    footer = _b(XML_DECL + f'<w:ftr xmlns:w="{W}"><w:p><w:r><w:t>FOOTER</w:t></w:r></w:p></w:ftr>')

    doc_rels = _rels([
        ("rId10", f"{R}/image", "media/image1.png"),
        ("rId20", f"{R}/header", "header1.xml"),
        ("rId21", f"{R}/footer", "footer1.xml"),
        ("rId30", f"{R}/footnotes", "footnotes.xml"),
        ("rId31", f"{R}/comments", "comments.xml"),
    ])
    root_rels = _rels([
        ("rId1", f"{R}/officeDocument", "word/document.xml"),
    ])
    ct = _content_types(
        overrides=[
            ("/word/document.xml", MAIN_CT),
            ("/word/footnotes.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"),
            ("/word/comments.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"),
            ("/word/header1.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"),
            ("/word/footer1.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"),
        ],
        defaults=[
            ("rels", RELS_CT),
            ("xml", "application/xml"),
            ("png", "image/png"),
        ],
    )
    return _zip([
        ("[Content_Types].xml", ct),
        ("_rels/.rels", root_rels),
        ("word/document.xml", document),
        ("word/_rels/document.xml.rels", doc_rels),
        ("word/footnotes.xml", footnotes),
        ("word/comments.xml", comments),
        ("word/header1.xml", header),
        ("word/footer1.xml", footer),
        ("word/media/image1.png", TINY_PNG),
    ])


def comments() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}"><w:body>'
        '<w:p><w:commentRangeStart w:id="7"/><w:r><w:t>Alpha</w:t></w:r>'
        '<w:commentRangeEnd w:id="7"/><w:r><w:commentReference w:id="7"/></w:r></w:p>'
        '<w:p><w:commentRangeStart w:id="8"/><w:r><w:t>Beta</w:t></w:r>'
        '<w:commentRangeEnd w:id="8"/><w:r><w:commentReference w:id="8"/></w:r></w:p>'
        "</w:body></w:document>"
    )
    comments_part = _b(
        XML_DECL
        + f'<w:comments xmlns:w="{W}">'
        '<w:comment w:id="7" w:author="Reviewer" w:initials="RV" w:date="2026-06-24T00:00:00Z">'
        '<w:p><w:r><w:t>First note.</w:t></w:r></w:p></w:comment>'
        '<w:comment w:id="8" w:author="Reviewer" w:initials="RV" w:date="2026-06-24T00:00:00Z">'
        '<w:p><w:r><w:t>Second note.</w:t></w:r></w:p></w:comment>'
        "</w:comments>"
    )
    return _minimal_docx(
        document,
        doc_rels=[("rId1", f"{R}/comments", "comments.xml")],
        overrides=[
            ("/word/comments.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml"),
        ],
        extra_parts=[("word/comments.xml", comments_part)],
    )


def revisions() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}"><w:body><w:p>'
        '<w:r><w:t>Stable </w:t></w:r>'
        '<w:ins w:id="1" w:author="Alice" w:date="2026-06-24T01:00:00Z"><w:r><w:t>Added</w:t></w:r></w:ins>'
        '<w:del w:id="2" w:author="Bob" w:date="2026-06-24T02:00:00Z"><w:r><w:delText>Removed</w:delText></w:r></w:del>'
        '<w:moveFrom w:id="3" w:author="Carol"><w:r><w:delText>Moved from</w:delText></w:r></w:moveFrom>'
        '<w:moveTo w:id="4" w:author="Carol"><w:r><w:t>Moved to</w:t></w:r></w:moveTo>'
        '</w:p><w:p><w:pPr><w:pPrChange w:id="5" w:author="Dana" w:date="2026-06-24T03:00:00Z">'
        '<w:pPr><w:jc w:val="center"/></w:pPr></w:pPrChange></w:pPr>'
        '<w:r><w:t>Property change</w:t></w:r></w:p></w:body></w:document>'
    )
    return _minimal_docx(document)


def fields() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}"><w:body>'
        '<w:p><w:fldSimple w:instr=" PAGE "><w:r><w:t>3</w:t></w:r></w:fldSimple></w:p>'
        '<w:p><w:fldSimple w:instr=" TOC \\o &quot;1-3&quot; "><w:r><w:t>Contents</w:t></w:r></w:fldSimple></w:p>'
        '<w:p><w:fldSimple w:instr=" REF Figure1 "><w:r><w:t>Figure 1</w:t></w:r></w:fldSimple></w:p>'
        '<w:p><w:fldSimple w:instr=" HYPERLINK &quot;https://example.com&quot; "><w:r><w:t>Example</w:t></w:r></w:fldSimple></w:p>'
        '<w:p><w:fldSimple w:instr=" CUSTOM value "><w:r><w:t>custom</w:t></w:r></w:fldSimple></w:p>'
        '<w:p><w:r><w:fldChar w:fldCharType="begin"/></w:r>'
        '<w:r><w:instrText> FILENAME \\p </w:instrText></w:r>'
        '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
        '<w:r><w:t>report.docx</w:t></w:r>'
        '<w:r><w:fldChar w:fldCharType="end"/></w:r></w:p>'
        "</w:body></w:document>"
    )
    return _minimal_docx(document)


def hyperlinks() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>'
        '<w:p><w:hyperlink r:id="rIdLink"><w:r><w:t>Relationship link</w:t></w:r></w:hyperlink></w:p>'
        "</w:body></w:document>"
    )
    return _minimal_docx(
        document,
        doc_rels=[("rIdLink", f"{R}/hyperlink", "https://example.com/relationship", "External")],
    )


def nested_tables() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}"><w:body>'
        '<w:tbl><w:tr><w:tc>'
        '<w:p><w:r><w:t>Outer cell text</w:t></w:r></w:p>'
        '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Inner cell text</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
        '</w:tc></w:tr></w:tbl>'
        '</w:body></w:document>'
    )
    return _minimal_docx(document)


def unsupported_objects() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:c="{C}" xmlns:mc="{MC}" xmlns:o="{O}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:v="urn:schemas-microsoft-com:vml" mc:Ignorable="v o wp a c"><w:body>'
        '<w:p><w:r><w:drawing><wp:anchor distT="0" distB="0" distL="0" distR="0">'
        '<wp:extent cx="1000000" cy="500000"/><wp:docPr id="1" name="Chart"/>'
        '<a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/chart">'
        '<c:chart r:id="rId1"/></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r></w:p>'
        '<w:p><w:r><mc:AlternateContent><mc:Choice Requires="v"><w:pict><v:shape id="_x0000_s1"/></w:pict></mc:Choice>'
        '<mc:Fallback><w:r><w:t>[shape]</w:t></w:r></mc:Fallback></mc:AlternateContent></w:r></w:p>'
        '<w:p><w:r><w:object><o:OLEObject r:id="rId2"/></w:object></w:r></w:p>'
        '<w:p><w:r><w:t>Unsupported media is related but intentionally not rendered.</w:t></w:r></w:p>'
        "</w:body></w:document>"
    )
    chart = _b(XML_DECL + f'<c:chartSpace xmlns:c="{C}"><c:chart/></c:chartSpace>')
    return _minimal_docx(
        document,
        doc_rels=[
            ("rId1", f"{R}/chart", "charts/chart1.xml"),
            ("rId2", f"{R}/oleObject", "embeddings/oleObject1.bin"),
            ("rId3", f"{R}/image", "media/vector1.emf"),
            ("rId4", f"{R}/image", "media/vector2.wmf"),
        ],
        overrides=[
            ("/word/charts/chart1.xml", "application/vnd.openxmlformats-officedocument.drawingml.chart+xml"),
            ("/word/embeddings/oleObject1.bin", "application/vnd.openxmlformats-officedocument.oleObject"),
        ],
        defaults=[
            ("emf", "image/x-emf"),
            ("wmf", "image/x-wmf"),
        ],
        extra_parts=[
            ("word/charts/chart1.xml", chart),
            ("word/embeddings/oleObject1.bin", b"rwml synthetic ole placeholder"),
            ("word/media/vector1.emf", b"rwml synthetic emf placeholder"),
            ("word/media/vector2.wmf", b"rwml synthetic wmf placeholder"),
        ],
    )


def floating_altcontent_anchor() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:mc="{MC}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
        '<w:body><w:p><w:r><w:t xml:space="preserve">Public alternate before. </w:t></w:r>'
        '<w:r><mc:AlternateContent><mc:Choice Requires="wps">'
        '<w:drawing><wp:anchor relativeHeight="410"><wp:extent cx="914400" cy="457200"/>'
        '<wp:docPr id="410" name="Public choice float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Public choice body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing>'
        '</mc:Choice><mc:Fallback>'
        '<w:drawing><wp:anchor relativeHeight="411"><wp:extent cx="914400" cy="457200"/>'
        '<wp:docPr id="411" name="Public fallback float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Public fallback body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing>'
        '</mc:Fallback></mc:AlternateContent></w:r>'
        '<w:r><w:t xml:space="preserve"> Public alternate after.</w:t></w:r></w:p></w:body></w:document>'
    )
    return _minimal_docx(document)


def floating_z_order_pair() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
        '<w:body><w:p><w:r><w:t xml:space="preserve">Z-order before. </w:t></w:r>'
        '<w:r><w:drawing><wp:anchor relativeHeight="100" behindDoc="1">'
        '<wp:extent cx="914400" cy="457200"/><wp:docPr id="420" name="Behind float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Behind body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r>'
        '<w:r><w:t xml:space="preserve"> middle. </w:t></w:r>'
        '<w:r><w:drawing><wp:anchor relativeHeight="200" behindDoc="0">'
        '<wp:extent cx="914400" cy="457200"/><wp:docPr id="421" name="Front float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Front body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r>'
        '<w:r><w:t xml:space="preserve"> Z-order after.</w:t></w:r></w:p></w:body></w:document>'
    )
    return _minimal_docx(document)


def floating_wrap_policy() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
        '<w:body><w:p><w:r><w:t xml:space="preserve">Wrap before. </w:t></w:r>'
        '<w:r><w:drawing><wp:anchor relativeHeight="430" behindDoc="0" '
        'distT="111" distB="222" distL="333" distR="444">'
        '<wp:extent cx="914400" cy="457200"/>'
        '<wp:wrapTight wrapText="bothSides" distT="555" distB="666" distL="777" distR="888">'
        '<wp:wrapPolygon edited="0"><wp:start x="0" y="0"/>'
        '<wp:lineTo x="914400" y="0"/><wp:lineTo x="914400" y="457200"/>'
        '<wp:lineTo x="457200" y="600000"/><wp:lineTo x="0" y="457200"/>'
        '</wp:wrapPolygon></wp:wrapTight>'
        '<wp:docPr id="430" name="Public wrap float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Wrap body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r>'
        '<w:r><w:t xml:space="preserve"> Wrap after.</w:t></w:r></w:p></w:body></w:document>'
    )
    return _minimal_docx(document)


def floating_text_bearing() -> bytes:
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape">'
        '<w:body><w:p><w:r><w:t xml:space="preserve">Containing block before. </w:t></w:r>'
        '<w:r><w:drawing><wp:anchor relativeHeight="440" behindDoc="0">'
        '<wp:extent cx="914400" cy="457200"/><wp:docPr id="440" name="Public text float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r><w:t>Public text body</w:t></w:r></w:p>'
        '</w:txbxContent></wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r>'
        '<w:r><w:t xml:space="preserve"> Containing block after.</w:t></w:r></w:p></w:body></w:document>'
    )
    return _minimal_docx(document)


CORPUS = {
    "kitchen_sink.docx": kitchen_sink,
    "comments.docx": comments,
    "revisions.docx": revisions,
    "fields.docx": fields,
    "hyperlinks.docx": hyperlinks,
    "nested_tables.docx": nested_tables,
    "unsupported_objects.docx": unsupported_objects,
    "floating_altcontent_anchor.docx": floating_altcontent_anchor,
    "floating_z_order_pair.docx": floating_z_order_pair,
    "floating_wrap_policy.docx": floating_wrap_policy,
    "floating_text_bearing.docx": floating_text_bearing,
}


def main() -> int:
    check = "--check" in sys.argv
    os.makedirs(OUT_DIR, exist_ok=True)
    for name, fn in CORPUS.items():
        path = os.path.join(OUT_DIR, name)
        data = fn()
        with open(path, "wb") as f:
            f.write(data)
        print(f"wrote {os.path.relpath(path)} ({len(data)} bytes)")
        if check:
            from docx import Document  # python-docx
            Document(path)  # raises if structurally invalid
            print(f"  python-docx OK: {name}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
