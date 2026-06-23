#!/usr/bin/env python3
"""Generate a small, license-clean PUBLIC .docx corpus for rdoc validation.

Every file here is authored from scratch (raw OOXML) by this script, so the output is
trivially license-clean (you own it) and can be committed to the public repo. The files
deliberately carry the *unmodeled* content a package-preserving editor must round-trip
intact: tracked changes (w:ins/w:del), content controls (w:sdt), text boxes
(mc:AlternateContent + w:txbxContent), footnotes, comments, headers/footers, tables, and
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

XML_DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

# A genuinely valid 2x3 RGB PNG (correct chunk CRCs + a real zlib IDAT). Same bytes the
# in-tree validate_edit example uses, so rdoc's CRC-checked is_png accepts it.
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


def _rels(entries: list[tuple[str, str, str]]) -> bytes:
    body = "".join(
        f'<Relationship Id="{i}" Type="{t}" Target="{tg}"/>' for i, t, tg in entries
    )
    return _b(
        XML_DECL
        + '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
        + body + "</Relationships>"
    )


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
        '<w:p><w:ins w:id="1" w:author="rdoc" w:date="2020-01-01T00:00:00Z">'
        "<w:r><w:t>INSERTED </w:t></w:r></w:ins>"
        '<w:del w:id="2" w:author="rdoc" w:date="2020-01-01T00:00:00Z">'
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
        '<w:comment w:id="0" w:author="rdoc" w:date="2020-01-01T00:00:00Z" w:initials="r">'
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


CORPUS = {
    "kitchen_sink.docx": kitchen_sink,
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
