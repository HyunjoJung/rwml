#!/usr/bin/env python3
"""Build the digest-locked 40-document public render pilot campaign.

The campaign is materialized under ``target/``. It copies all 21 inputs from the
strict public corpus into a self-contained directory and adds 19 deterministic raw
OOXML fixtures. The checked-in lock binds the parent manifest, generator closure,
provenance files, and every document byte.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import tempfile
from dataclasses import dataclass
from html import escape
from pathlib import Path
from typing import Callable

try:
    from gen_public_corpus import R, TINY_PNG, W, XML_DECL, _b, _minimal_docx
    from render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.gen_public_corpus import (
        R,
        TINY_PNG,
        W,
        XML_DECL,
        _b,
        _minimal_docx,
    )
    from scripts.render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
PUBLIC_GENERATOR = ROOT / "scripts" / "gen_public_corpus.py"
PARENT_MANIFEST = ROOT / "corpus" / "public" / "RENDER_ORACLE.json"
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-pilot-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-pilot-v1"

LOCK_SCHEMA = "rwml.render-pilot-lock.v1"
CAMPAIGN = "public-render-pilot-v1"
PILOT_PROVENANCE = "rwml-render-pilot"
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)
NUMBERING_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"
)
FOOTNOTES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"
)
ENDNOTES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"
)
SETTINGS_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"
)
HEADER_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"
)
FOOTER_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"
)

PILOT_PROVENANCE_TEXT = """# Public render pilot provenance

The 19 `pilot-*` DOCX inputs are generated from repository-owned raw OOXML by
`scripts/generate_render_pilot.py`. Package order, ZIP metadata, page geometry,
text, relationships, and media bytes are deterministic. The fixtures contain only
synthetic text and the repository's existing generated 2-by-3 pixel PNG.

The generated documents and this provenance record are licensed under the
repository's MIT license. The checked-in render-pilot lock binds the generator
closure and every generated input by byte length and SHA-256.
"""


@dataclass(frozen=True)
class PilotCase:
    case_id: str
    features: tuple[str, ...]
    expected_pages: int
    expected_warnings: tuple[str, ...]
    builder: Callable[[], bytes]


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _styles() -> bytes:
    return _b(
        XML_DECL
        + f'<w:styles xmlns:w="{W}">'
        '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Noto Sans" '
        'w:hAnsi="Noto Sans" w:eastAsia="Noto Sans" w:cs="Noto Sans"/>'
        '<w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:rPrDefault>'
        '<w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0"/>'
        '</w:pPr></w:pPrDefault></w:docDefaults>'
        '<w:style w:type="paragraph" w:default="1" w:styleId="Normal">'
        '<w:name w:val="Normal"/></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading1">'
        '<w:name w:val="heading 1"/><w:basedOn w:val="Normal"/>'
        '<w:pPr><w:outlineLvl w:val="0"/></w:pPr>'
        '<w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>'
        '<w:style w:type="paragraph" w:styleId="Heading2">'
        '<w:name w:val="heading 2"/><w:basedOn w:val="Normal"/>'
        '<w:pPr><w:outlineLvl w:val="1"/></w:pPr>'
        '<w:rPr><w:b/><w:sz w:val="24"/></w:rPr></w:style>'
        '</w:styles>'
    )


def _section(
    *,
    width: int = 12240,
    height: int = 15840,
    orient: str | None = None,
    columns: str = "",
    references: str = "",
) -> str:
    orientation = f' w:orient="{orient}"' if orient else ""
    return (
        "<w:sectPr>"
        + references
        + f'<w:pgSz w:w="{width}" w:h="{height}"{orientation}/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/>'
        + columns
        + "</w:sectPr>"
    )


def _paragraph(text: str, *, paragraph_properties: str = "", run_properties: str = "") -> str:
    ppr = f"<w:pPr>{paragraph_properties}</w:pPr>" if paragraph_properties else ""
    rpr = f"<w:rPr>{run_properties}</w:rPr>" if run_properties else ""
    return (
        "<w:p>"
        + ppr
        + "<w:r>"
        + rpr
        + f'<w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>'
    )


def _simple_field(instruction: str, cached: str) -> str:
    return (
        f'<w:fldSimple w:instr="{escape(instruction, quote=True)}">'
        f'<w:r><w:t>{escape(cached)}</w:t></w:r></w:fldSimple>'
    )


def _document(
    body: str,
    *,
    section: str | None = None,
    namespaces: str = "",
    relationships: list[tuple[str, str, str]] | None = None,
    overrides: list[tuple[str, str]] | None = None,
    defaults: list[tuple[str, str]] | None = None,
    extra_parts: list[tuple[str, bytes]] | None = None,
) -> bytes:
    relationships = relationships or []
    overrides = overrides or []
    defaults = defaults or []
    extra_parts = extra_parts or []
    document = _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"{namespaces}><w:body>'
        + body
        + (section if section is not None else _section())
        + "</w:body></w:document>"
    )
    return _minimal_docx(
        document,
        doc_rels=[("rIdStyles", f"{R}/styles", "styles.xml"), *relationships],
        overrides=[("/word/styles.xml", STYLES_CONTENT_TYPE), *overrides],
        defaults=defaults,
        extra_parts=[("word/styles.xml", _styles()), *extra_parts],
    )


def _character_paint() -> bytes:
    runs = (
        '<w:p><w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr>'
        '<w:t>Highlighted text</w:t></w:r><w:r><w:t xml:space="preserve"> | </w:t></w:r>'
        '<w:r><w:rPr><w:caps/></w:rPr><w:t>caps text</w:t></w:r>'
        '<w:r><w:t xml:space="preserve"> | </w:t></w:r>'
        '<w:r><w:rPr><w:smallCaps/></w:rPr><w:t>Small caps text</w:t></w:r></w:p>'
        '<w:p><w:r><w:t>Baseline</w:t></w:r>'
        '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr><w:t>SUP</w:t></w:r>'
        '<w:r><w:rPr><w:vertAlign w:val="subscript"/></w:rPr><w:t>SUB</w:t></w:r>'
        '<w:r><w:rPr><w:color w:val="C00000"/><w:shd w:fill="D9EAF7"/></w:rPr>'
        '<w:t> colored and shaded run</w:t></w:r></w:p>'
    )
    return _document(runs)


def _fields_document_formula() -> bytes:
    body = _paragraph("Deterministic formula and document fields")
    body += "<w:p>" + _simple_field('= 21 * 2 \\# "0"', "42") + "</w:p>"
    body += "<w:p>" + _simple_field("NUMWORDS", "7") + "</w:p>"
    body += "<w:p>" + _simple_field("NUMCHARS", "41") + "</w:p>"
    body += "<w:p>" + _simple_field("INFO TITLE", "Render pilot") + "</w:p>"
    return _document(body)


def _footnotes_part() -> bytes:
    return _b(
        XML_DECL
        + f'<w:footnotes xmlns:w="{W}">'
        '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/>'
        '</w:r></w:p></w:footnote>'
        '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r>'
        '<w:continuationSeparator/></w:r></w:p></w:footnote>'
        '<w:footnote w:id="2"><w:p><w:r><w:footnoteRef/>'
        '<w:t xml:space="preserve"> First pilot footnote.</w:t></w:r></w:p></w:footnote>'
        '<w:footnote w:id="3"><w:p><w:r><w:footnoteRef/>'
        '<w:t xml:space="preserve"> Second pilot footnote.</w:t></w:r></w:p></w:footnote>'
        '</w:footnotes>'
    )


def _fields_reference() -> bytes:
    body = (
        '<w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr>'
        '<w:bookmarkStart w:id="10" w:name="pilotHeading"/>'
        '<w:r><w:t>Pilot heading</w:t></w:r><w:bookmarkEnd w:id="10"/></w:p>'
        '<w:p><w:r><w:t>Reference: </w:t></w:r>'
        + _simple_field("REF pilotHeading", "Pilot heading")
        + '</w:p><w:p><w:bookmarkStart w:id="11" w:name="pilotNote"/>'
        '<w:r><w:footnoteReference w:id="2"/></w:r>'
        '<w:bookmarkEnd w:id="11"/><w:r><w:t> Note reference: </w:t></w:r>'
        + _simple_field("NOTEREF pilotNote", "1")
        + '</w:p><w:p>'
        + _simple_field('TOC \\o "1-2"', "Pilot heading")
        + "</w:p>"
    )
    return _document(
        body,
        relationships=[("rIdFootnotes", f"{R}/footnotes", "footnotes.xml")],
        overrides=[("/word/footnotes.xml", FOOTNOTES_CONTENT_TYPE)],
        extra_parts=[("word/footnotes.xml", _footnotes_part())],
    )


def _floating_relative_placement() -> bytes:
    namespaces = (
        ' xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"'
        ' xmlns:wps="http://schemas.microsoft.com/office/word/2010/wordprocessingShape"'
    )
    body = _paragraph("Text before relative floating placement.")
    body += (
        '<w:p><w:r><w:drawing><wp:anchor relativeHeight="42" behindDoc="0" '
        'distT="36000" distB="36000" distL="36000" distR="36000">'
        '<wp:positionH relativeFrom="margin"><wp:align>right</wp:align></wp:positionH>'
        '<wp:positionV relativeFrom="page"><wp:posOffset>1371600</wp:posOffset>'
        '</wp:positionV><wp:extent cx="1828800" cy="731520"/>'
        '<wp:wrapTopAndBottom/><wp:docPr id="920" name="Pilot relative float"/>'
        '<wps:wsp><wps:txbx><w:txbxContent><w:p><w:r>'
        '<w:t>Relative floating text</w:t></w:r></w:p></w:txbxContent>'
        '</wps:txbx></wps:wsp></wp:anchor></w:drawing></w:r></w:p>'
    )
    body += _paragraph("Text after relative floating placement.")
    return _document(body, namespaces=namespaces)


def _header_footer_variants() -> bytes:
    first_refs = (
        '<w:headerReference w:type="default" r:id="rIdHeaderDefault"/>'
        '<w:headerReference w:type="first" r:id="rIdHeaderFirst"/>'
        '<w:footerReference w:type="default" r:id="rIdFooterDefault"/>'
        '<w:titlePg/>'
    )
    first_section = _section(references=first_refs)
    body = _paragraph("First page with first-page header.")
    body += '<w:p><w:pPr>' + first_section + '</w:pPr><w:r><w:t>Section boundary</w:t></w:r></w:p>'
    body += _paragraph("Second page with default header and footer.")
    settings = _b(
        XML_DECL
        + f'<w:settings xmlns:w="{W}"><w:evenAndOddHeaders/></w:settings>'
    )
    header_first = _b(
        XML_DECL
        + f'<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>PILOT FIRST HEADER</w:t>'
        '</w:r></w:p></w:hdr>'
    )
    header_default = _b(
        XML_DECL
        + f'<w:hdr xmlns:w="{W}"><w:p><w:r><w:t>PILOT DEFAULT HEADER</w:t>'
        '</w:r></w:p></w:hdr>'
    )
    footer_default = _b(
        XML_DECL
        + f'<w:ftr xmlns:w="{W}"><w:p><w:r><w:t>PILOT FOOTER</w:t>'
        '</w:r></w:p></w:ftr>'
    )
    return _document(
        body,
        section=_section(references=first_refs),
        relationships=[
            ("rIdHeaderDefault", f"{R}/header", "header1.xml"),
            ("rIdHeaderFirst", f"{R}/header", "header2.xml"),
            ("rIdFooterDefault", f"{R}/footer", "footer1.xml"),
            ("rIdSettings", f"{R}/settings", "settings.xml"),
        ],
        overrides=[
            ("/word/header1.xml", HEADER_CONTENT_TYPE),
            ("/word/header2.xml", HEADER_CONTENT_TYPE),
            ("/word/footer1.xml", FOOTER_CONTENT_TYPE),
            ("/word/settings.xml", SETTINGS_CONTENT_TYPE),
        ],
        extra_parts=[
            ("word/header1.xml", header_default),
            ("word/header2.xml", header_first),
            ("word/footer1.xml", footer_default),
            ("word/settings.xml", settings),
        ],
    )


def _inline_image_layout() -> bytes:
    namespaces = (
        ' xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"'
        ' xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main"'
        ' xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture"'
    )
    drawing = (
        '<w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">'
        '<wp:extent cx="762000" cy="1143000"/><wp:docPr id="930" name="Pilot PNG"/>'
        '<a:graphic><a:graphicData '
        'uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        '<pic:pic><pic:nvPicPr><pic:cNvPr id="930" name="pilot.png"/>'
        '<pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdImage"/>'
        '<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr>'
        '<a:xfrm><a:off x="0" y="0"/><a:ext cx="762000" cy="1143000"/>'
        '</a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>'
        '</pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing>'
    )
    body = '<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r>' + drawing + '</w:r></w:p>'
    body += _paragraph("Centered inline image with following text.")
    return _document(
        body,
        namespaces=namespaces,
        relationships=[("rIdImage", f"{R}/image", "media/pilot.png")],
        defaults=[("png", "image/png")],
        extra_parts=[("word/media/pilot.png", TINY_PNG)],
    )


def _mixed_custom_widths() -> bytes:
    first = _section(width=10080, height=10080)
    body = _paragraph("Square custom page width: seven inches.")
    body += '<w:p><w:pPr>' + first + '</w:pPr><w:r><w:t>Width boundary</w:t></w:r></w:p>'
    body += _paragraph("Narrow custom page width: five inches.")
    return _document(body, section=_section(width=7200, height=12960))


def _mixed_sections() -> bytes:
    portrait = _section(width=12240, height=15840)
    body = _paragraph("Portrait section before a next-page section break.")
    body += '<w:p><w:pPr>' + portrait + '</w:pPr><w:r><w:t>Portrait boundary</w:t></w:r></w:p>'
    body += _paragraph("Landscape two-column section.")
    columns = '<w:cols w:num="2" w:equalWidth="1" w:space="720"/>'
    return _document(
        body,
        section=_section(width=15840, height=12240, orient="landscape", columns=columns),
    )


def _notes_numbering() -> bytes:
    body = (
        '<w:p><w:r><w:t>Footnote marker</w:t></w:r><w:r>'
        '<w:footnoteReference w:id="2"/></w:r></w:p>'
        '<w:p><w:r><w:t>Second footnote marker</w:t></w:r><w:r>'
        '<w:footnoteReference w:id="3"/></w:r></w:p>'
        '<w:p><w:r><w:t>Endnote marker</w:t></w:r><w:r>'
        '<w:endnoteReference w:id="2"/></w:r></w:p>'
    )
    endnotes = _b(
        XML_DECL
        + f'<w:endnotes xmlns:w="{W}">'
        '<w:endnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/>'
        '</w:r></w:p></w:endnote>'
        '<w:endnote w:type="continuationSeparator" w:id="0"><w:p><w:r>'
        '<w:continuationSeparator/></w:r></w:p></w:endnote>'
        '<w:endnote w:id="2"><w:p><w:r><w:endnoteRef/>'
        '<w:t xml:space="preserve"> Pilot endnote.</w:t></w:r></w:p></w:endnote>'
        '</w:endnotes>'
    )
    settings = _b(
        XML_DECL
        + f'<w:settings xmlns:w="{W}"><w:footnotePr><w:numFmt w:val="lowerLetter"/>'
        '<w:numStart w:val="3"/></w:footnotePr><w:endnotePr>'
        '<w:numFmt w:val="lowerRoman"/><w:numStart w:val="2"/>'
        '</w:endnotePr></w:settings>'
    )
    return _document(
        body,
        relationships=[
            ("rIdFootnotes", f"{R}/footnotes", "footnotes.xml"),
            ("rIdEndnotes", f"{R}/endnotes", "endnotes.xml"),
            ("rIdSettings", f"{R}/settings", "settings.xml"),
        ],
        overrides=[
            ("/word/footnotes.xml", FOOTNOTES_CONTENT_TYPE),
            ("/word/endnotes.xml", ENDNOTES_CONTENT_TYPE),
            ("/word/settings.xml", SETTINGS_CONTENT_TYPE),
        ],
        extra_parts=[
            ("word/footnotes.xml", _footnotes_part()),
            ("word/endnotes.xml", endnotes),
            ("word/settings.xml", settings),
        ],
    )


def _pagination_controls() -> bytes:
    body = _paragraph(
        "Keep this heading with the next paragraph.",
        paragraph_properties="<w:keepNext/><w:keepLines/><w:widowControl/>",
    )
    body += _paragraph("Kept body line one. Kept body line two.")
    body += '<w:p><w:r><w:br w:type="page"/></w:r></w:p>'
    body += _paragraph("Explicit second page before a column break.")
    body += '<w:p><w:r><w:br w:type="column"/></w:r></w:p>'
    body += _paragraph("Second column after an explicit column break.")
    return _document(
        body,
        section=_section(columns='<w:cols w:num="2" w:equalWidth="1" w:space="720"/>'),
    )


def _paragraph_borders_shading() -> bytes:
    borders = (
        '<w:pBdr><w:top w:val="single" w:sz="12" w:space="4" w:color="1F4E78"/>'
        '<w:left w:val="double" w:sz="12" w:space="4" w:color="C00000"/>'
        '<w:bottom w:val="single" w:sz="12" w:space="4" w:color="1F4E78"/>'
        '<w:right w:val="double" w:sz="12" w:space="4" w:color="C00000"/>'
        '</w:pBdr><w:shd w:val="clear" w:color="auto" w:fill="DDEBF7"/>'
    )
    body = _paragraph("Bordered and shaded paragraph.", paragraph_properties=borders)
    body += _paragraph(
        "A second shaded paragraph with paragraph spacing.",
        paragraph_properties='<w:shd w:fill="E2F0D9"/><w:spacing w:before="160" w:after="160"/>',
    )
    return _document(body)


def _paragraph_spacing_tabs() -> bytes:
    body = _paragraph(
        "First-line indent with 150 percent line spacing wraps across a deliberately "
        "long sentence for deterministic geometry.",
        paragraph_properties=(
            '<w:ind w:left="720" w:firstLine="360"/><w:spacing w:line="360" '
            'w:lineRule="auto"/><w:tabs><w:tab w:val="left" w:pos="2160"/>'
            '<w:tab w:val="decimal" w:pos="4320"/></w:tabs>'
        ),
    )
    body += (
        '<w:p><w:pPr><w:ind w:left="1080" w:hanging="360"/>'
        '<w:tabs><w:tab w:val="left" w:pos="2160"/>'
        '<w:tab w:val="right" w:pos="5760"/></w:tabs></w:pPr>'
        '<w:r><w:t>Hanging</w:t><w:tab/><w:t>middle</w:t><w:tab/>'
        '<w:t>right edge</w:t></w:r></w:p>'
    )
    return _document(body)


def _numbering() -> bytes:
    return _b(
        XML_DECL
        + f'<w:numbering xmlns:w="{W}">'
        '<w:abstractNum w:abstractNumId="7"><w:multiLevelType w:val="multilevel"/>'
        '<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/>'
        '<w:lvlText w:val="%1."/><w:lvlJc w:val="right"/>'
        '<w:pPr><w:tabs><w:tab w:val="num" w:pos="720"/></w:tabs>'
        '<w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>'
        '<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="lowerLetter"/>'
        '<w:lvlText w:val="%1.%2"/><w:lvlJc w:val="right"/>'
        '<w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>'
        '</w:abstractNum><w:num w:numId="27"><w:abstractNumId w:val="7"/>'
        '</w:num></w:numbering>'
    )


def _rtl_list() -> bytes:
    body = (
        '<w:p><w:pPr><w:bidi/><w:jc w:val="right"/><w:numPr>'
        '<w:ilvl w:val="0"/><w:numId w:val="27"/></w:numPr></w:pPr>'
        '<w:r><w:rPr><w:rtl/></w:rPr><w:t>عنصر عربي أول 123</w:t></w:r></w:p>'
        '<w:p><w:pPr><w:bidi/><w:jc w:val="right"/><w:numPr>'
        '<w:ilvl w:val="1"/><w:numId w:val="27"/></w:numPr></w:pPr>'
        '<w:r><w:rPr><w:rtl/></w:rPr><w:t>פריט עברי מקונן 45</w:t></w:r></w:p>'
    )
    return _document(
        body,
        relationships=[("rIdNumbering", f"{R}/numbering", "numbering.xml")],
        overrides=[("/word/numbering.xml", NUMBERING_CONTENT_TYPE)],
        extra_parts=[("word/numbering.xml", _numbering())],
    )


def _rtl_merged_table() -> bytes:
    body = (
        '<w:tbl><w:tblPr><w:bidiVisual/><w:tblW w:w="7200" w:type="dxa"/>'
        '</w:tblPr><w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/>'
        '<w:gridCol w:w="2400"/></w:tblGrid><w:tr><w:tc><w:tcPr>'
        '<w:gridSpan w:val="2"/></w:tcPr><w:p><w:pPr><w:bidi/>'
        '<w:jc w:val="right"/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr>'
        '<w:t>כותרת ממוזגת</w:t></w:r></w:p></w:tc><w:tc><w:p>'
        '<w:pPr><w:bidi/></w:pPr><w:r><w:rPr><w:rtl/></w:rPr>'
        '<w:t>ثالث</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc>'
        '<w:tcPr><w:vMerge w:val="restart"/></w:tcPr><w:p><w:r><w:t>A</w:t>'
        '</w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>B</w:t></w:r></w:p>'
        '</w:tc><w:tc><w:p><w:r><w:t>C</w:t></w:r></w:p></w:tc></w:tr>'
        '<w:tr><w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc>'
        '<w:tc><w:p><w:r><w:t>D</w:t></w:r></w:p></w:tc>'
        '<w:tc><w:p><w:r><w:t>E</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
    )
    return _document(body)


def _rtl_mixed_text() -> bytes:
    body = (
        '<w:p><w:pPr><w:bidi/><w:jc w:val="right"/></w:pPr>'
        '<w:r><w:rPr><w:rtl/></w:rPr><w:t>مرحبا بالعالم </w:t></w:r>'
        '<w:r><w:t>rwml 2026 (A-17)</w:t></w:r>'
        '<w:r><w:rPr><w:rtl/></w:rPr><w:t> שלום</w:t></w:r></w:p>'
        '<w:p><w:r><w:t>LTR prefix: </w:t></w:r><w:r><w:rPr><w:rtl/>'
        '</w:rPr><w:t>اختبار 123, עברית 456</w:t></w:r>'
        '<w:r><w:t> :LTR suffix.</w:t></w:r></w:p>'
    )
    return _document(body)


def _structured_revisions() -> bytes:
    body = (
        '<w:sdt><w:sdtPr><w:alias w:val="Pilot structured region"/>'
        '<w:tag w:val="pilot-structured"/></w:sdtPr><w:sdtContent>'
        '<w:p><w:ins w:id="71" w:author="rwml" w:date="2026-01-01T00:00:00Z">'
        '<w:r><w:t>Accepted inserted text</w:t></w:r></w:ins>'
        '<w:del w:id="72" w:author="rwml" w:date="2026-01-01T00:00:00Z">'
        '<w:r><w:delText>Rejected deleted text</w:delText></w:r></w:del></w:p>'
        '<w:tbl><w:tr><w:tc><w:p><w:r><w:t>Structured cell A</w:t>'
        '</w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Structured cell B</w:t>'
        '</w:r></w:p></w:tc></w:tr></w:tbl></w:sdtContent></w:sdt>'
    )
    return _document(body)


def _table_cell_spacing() -> bytes:
    body = (
        '<w:tbl><w:tblPr><w:tblW w:w="7200" w:type="dxa"/>'
        '<w:tblCellSpacing w:w="120" w:type="dxa"/><w:tblCellMar>'
        '<w:top w:w="160" w:type="dxa"/><w:left w:w="220" w:type="dxa"/>'
        '<w:bottom w:w="160" w:type="dxa"/><w:right w:w="220" w:type="dxa"/>'
        '</w:tblCellMar></w:tblPr><w:tblGrid><w:gridCol w:w="2400"/>'
        '<w:gridCol w:w="2400"/><w:gridCol w:w="2400"/></w:tblGrid>'
        '<w:tr><w:tc><w:tcPr><w:gridSpan w:val="2"/></w:tcPr>'
        '<w:p><w:r><w:t>Spanning two columns</w:t></w:r></w:p></w:tc>'
        '<w:tc><w:tcPr><w:vMerge w:val="restart"/></w:tcPr>'
        '<w:p><w:r><w:t>Vertical merge</w:t></w:r></w:p></w:tc></w:tr>'
        '<w:tr><w:tc><w:p><w:r><w:t>Lower A</w:t></w:r></w:p></w:tc>'
        '<w:tc><w:p><w:r><w:t>Lower B</w:t></w:r></w:p></w:tc>'
        '<w:tc><w:tcPr><w:vMerge/></w:tcPr><w:p/></w:tc></w:tr></w:tbl>'
    )
    return _document(body)


def _unicode_line_breaking() -> bytes:
    body = _paragraph(
        "한글 줄바꿈 검증 문장입니다. 日本語の禁則処理を確認します。"
        "中文标点，不能错误换行。"
    )
    body += _paragraph(
        "Emoji clusters: 👩‍💻 family 👨‍👩‍👧‍👦 flags 🇰🇷 🇺🇸 combining "
        "cafe\u0301 and résumé."
    )
    body += _paragraph(
        "Long mixed token sequence: alpha-beta/gamma_delta 123,456.78 한국어English日本語."
    )
    return _document(body, section=_section(width=7200, height=10080))


def _unequal_table_continuation() -> bytes:
    rows = []
    for index in range(1, 73):
        rows.append(
            '<w:tr><w:tc><w:tcPr><w:tcW w:w="720" w:type="dxa"/></w:tcPr>'
            f'<w:p><w:r><w:t>L{index:02d}</w:t></w:r></w:p></w:tc>'
            '<w:tc><w:tcPr><w:tcW w:w="720" w:type="dxa"/></w:tcPr>'
            f'<w:p><w:r><w:t>R{index:02d}</w:t></w:r></w:p></w:tc></w:tr>'
        )
    table = (
        '<w:tbl><w:tblPr><w:tblW w:w="1440" w:type="dxa"/>'
        '<w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid>'
        '<w:gridCol w:w="720"/><w:gridCol w:w="720"/></w:tblGrid>'
        + "".join(rows)
        + "</w:tbl>"
    )
    columns = (
        '<w:cols w:num="2" w:equalWidth="0"><w:col w:w="1800" w:space="360"/>'
        '<w:col w:w="3600"/></w:cols>'
    )
    return _document(
        _paragraph("Unequal-column table continuation pilot.") + table,
        section=_section(width=7200, height=7200, columns=columns),
    )


PILOT_CASES = tuple(
    sorted(
        (
            PilotCase(
                "pilot-character-paint",
                ("caps", "character-paint", "highlight", "small-caps", "vertical-align"),
                1,
                (),
                _character_paint,
            ),
            PilotCase(
                "pilot-fields-document-formula",
                ("document-fields", "fields", "formula-fields"),
                1,
                (),
                _fields_document_formula,
            ),
            PilotCase(
                "pilot-fields-reference",
                ("fields", "note-reference-fields", "reference-fields", "toc-fields"),
                1,
                (),
                _fields_reference,
            ),
            PilotCase(
                "pilot-floating-relative-placement",
                ("floating-shapes", "relative-placement", "text-boxes", "top-bottom-wrap"),
                1,
                ("FloatingShapePlaceholderOnly",),
                _floating_relative_placement,
            ),
            PilotCase(
                "pilot-header-footer-variants",
                ("headers-footers", "mixed-sections", "sections"),
                2,
                (),
                _header_footer_variants,
            ),
            PilotCase(
                "pilot-inline-image-layout",
                ("inline-images", "paragraph-geometry"),
                1,
                (),
                _inline_image_layout,
            ),
            PilotCase(
                "pilot-mixed-custom-widths",
                ("mixed-sections", "page-layout", "sections"),
                2,
                (),
                _mixed_custom_widths,
            ),
            PilotCase(
                "pilot-mixed-sections",
                ("columns", "mixed-sections", "orientation", "sections"),
                2,
                (),
                _mixed_sections,
            ),
            PilotCase(
                "pilot-notes-numbering",
                ("endnotes", "footnotes", "note-numbering"),
                1,
                (),
                _notes_numbering,
            ),
            PilotCase(
                "pilot-pagination-controls",
                ("column-breaks", "keep-lines", "keep-next", "page-breaks", "widow-control"),
                2,
                (),
                _pagination_controls,
            ),
            PilotCase(
                "pilot-paragraph-borders-shading",
                ("paragraph-borders", "paragraph-shading"),
                1,
                (),
                _paragraph_borders_shading,
            ),
            PilotCase(
                "pilot-paragraph-spacing-tabs",
                (
                    "first-line-indent",
                    "hanging-indent",
                    "line-spacing",
                    "paragraph-geometry",
                    "tabs",
                ),
                1,
                (),
                _paragraph_spacing_tabs,
            ),
            PilotCase(
                "pilot-rtl-list",
                ("bidi", "lists", "rtl", "rtl-list"),
                1,
                (),
                _rtl_list,
            ),
            PilotCase(
                "pilot-rtl-merged-table",
                ("bidi", "rtl", "table-merges", "tables"),
                1,
                (),
                _rtl_merged_table,
            ),
            PilotCase(
                "pilot-rtl-mixed-text",
                ("bidi", "mixed-direction-text", "rtl"),
                1,
                (),
                _rtl_mixed_text,
            ),
            PilotCase(
                "pilot-structured-revisions",
                ("content-controls", "revisions", "tables", "tracked-changes"),
                1,
                (),
                _structured_revisions,
            ),
            PilotCase(
                "pilot-table-cell-spacing",
                ("table-cell-spacing", "table-merges", "tables"),
                1,
                (),
                _table_cell_spacing,
            ),
            PilotCase(
                "pilot-unicode-line-breaking",
                ("cjk", "emoji", "unicode-line-breaking"),
                1,
                (),
                _unicode_line_breaking,
            ),
            PilotCase(
                "pilot-unequal-table-continuation",
                ("columns", "table-continuation", "tables", "unequal-table-continuation"),
                3,
                (),
                _unequal_table_continuation,
            ),
        ),
        key=lambda case: case.case_id,
    )
)


def _generator_closure_sha256() -> str:
    digest = hashlib.sha256()
    for path in sorted((SCRIPT_PATH, PUBLIC_GENERATOR)):
        relative = path.relative_to(ROOT).as_posix().encode("utf-8")
        payload = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "little"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def _provenance_records() -> list[dict[str, object]]:
    parent = load_corpus_manifest(PARENT_MANIFEST)
    source_by_id = {item["id"]: item for item in parent.provenance}
    records = []
    for provenance_id, output_reference in (
        ("python-docx-mit", "provenance/python-docx-mit.md"),
        ("rwml-generated", "provenance/rwml-generated.md"),
    ):
        source = source_by_id[provenance_id]
        payload = (PARENT_MANIFEST.parent / source["reference"]).read_bytes()
        records.append(
            {
                "id": provenance_id,
                "kind": source["kind"],
                "license": source["license"],
                "reference": output_reference,
                "bytes": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
            }
        )
    payload = PILOT_PROVENANCE_TEXT.encode("utf-8")
    records.append(
        {
            "id": PILOT_PROVENANCE,
            "kind": "generated",
            "license": "MIT",
            "reference": "provenance/rwml-render-pilot.md",
            "bytes": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
        }
    )
    return sorted(records, key=lambda record: str(record["id"]))


def _document_record(
    *,
    case_id: str,
    payload: bytes,
    provenance: str,
    features: tuple[str, ...],
    expected_pages: int,
    expected_warnings: tuple[str, ...],
    format_name: str,
    source: str,
    source_path: str,
) -> dict[str, object]:
    return {
        "id": case_id,
        "path": f"documents/{case_id}.{format_name}",
        "format": format_name,
        "bytes": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
        "provenance": provenance,
        "features": sorted(features),
        "expected": {
            "pages": expected_pages,
            "warnings": sorted(expected_warnings),
        },
        "source": source,
        "source_path": source_path,
    }


def build_lock() -> dict[str, object]:
    parent = load_corpus_manifest(PARENT_MANIFEST)
    documents = []
    for document in parent.documents:
        documents.append(
            _document_record(
                case_id=document.case_id,
                payload=document.path.read_bytes(),
                provenance=document.provenance,
                features=document.features,
                expected_pages=document.expected_pages,
                expected_warnings=document.expected_warnings,
                format_name=document.format,
                source="parent-public",
                source_path=document.relative_path,
            )
        )
    for case in PILOT_CASES:
        documents.append(
            _document_record(
                case_id=case.case_id,
                payload=case.builder(),
                provenance=PILOT_PROVENANCE,
                features=case.features,
                expected_pages=case.expected_pages,
                expected_warnings=case.expected_warnings,
                format_name="docx",
                source="pilot-generated",
                source_path=f"scripts/generate_render_pilot.py#{case.case_id}",
            )
        )
    documents.sort(key=lambda document: str(document["id"]))
    return {
        "schema": LOCK_SCHEMA,
        "campaign": CAMPAIGN,
        "generator_closure_sha256": _generator_closure_sha256(),
        "parent_manifest_sha256": hashlib.sha256(
            PARENT_MANIFEST.read_bytes()
        ).hexdigest(),
        "limits": {
            "max_documents": 40,
            "max_input_bytes": 256 * 1024,
            "max_total_input_bytes": 4 * 1024 * 1024,
            "max_pages_per_document": 16,
        },
        "provenance": _provenance_records(),
        "documents": documents,
    }


def _payloads() -> dict[str, bytes]:
    parent = load_corpus_manifest(PARENT_MANIFEST)
    payloads = {
        f"documents/{document.case_id}.{document.format}": document.path.read_bytes()
        for document in parent.documents
    }
    payloads.update(
        {f"documents/{case.case_id}.docx": case.builder() for case in PILOT_CASES}
    )
    provenance_by_id = {item["id"]: item for item in parent.provenance}
    payloads["provenance/python-docx-mit.md"] = (
        PARENT_MANIFEST.parent / provenance_by_id["python-docx-mit"]["reference"]
    ).read_bytes()
    payloads["provenance/rwml-generated.md"] = (
        PARENT_MANIFEST.parent / provenance_by_id["rwml-generated"]["reference"]
    ).read_bytes()
    payloads["provenance/rwml-render-pilot.md"] = PILOT_PROVENANCE_TEXT.encode("utf-8")
    return payloads


def _manifest(lock: dict[str, object]) -> dict[str, object]:
    provenance = lock["provenance"]
    documents = lock["documents"]
    assert isinstance(provenance, list)
    assert isinstance(documents, list)
    return {
        "schema": CORPUS_SCHEMA,
        "campaign": CAMPAIGN,
        "limits": lock["limits"],
        "provenance": [
            {
                "id": item["id"],
                "kind": item["kind"],
                "license": item["license"],
                "reference": item["reference"],
            }
            for item in provenance
        ],
        "documents": [
            {
                "id": item["id"],
                "path": item["path"],
                "format": item["format"],
                "bytes": item["bytes"],
                "sha256": item["sha256"],
                "provenance": item["provenance"],
                "features": item["features"],
                "expected": item["expected"],
            }
            for item in documents
        ],
    }


def _atomic_write(path: Path, payload: bytes) -> None:
    if path.is_symlink():
        raise ValueError(f"output must not be a symlink: {path}")
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(
        prefix=f".{path.name}.", dir=path.parent
    )
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(payload)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, path)
    finally:
        try:
            temporary.unlink()
        except FileNotFoundError:
            pass


def materialize(output: Path, lock: dict[str, object]) -> Path:
    expected_lock = build_lock()
    if canonical_json(lock) != canonical_json(expected_lock):
        raise ValueError("render pilot lock does not match the current generator closure")
    if output.is_symlink() or (output.exists() and not output.is_dir()):
        raise ValueError(f"invalid render pilot output directory: {output}")
    output.mkdir(parents=True, exist_ok=True)
    payloads = _payloads()
    for relative_path, payload in sorted(payloads.items()):
        _atomic_write(output / relative_path, payload)
    manifest_path = output / "RENDER_ORACLE.json"
    _atomic_write(manifest_path, canonical_json(_manifest(lock)))
    load_corpus_manifest(manifest_path)
    return manifest_path


def refresh_lock(path: Path = DEFAULT_LOCK) -> None:
    _atomic_write(path, canonical_json(build_lock()))


def load_lock(path: Path = DEFAULT_LOCK) -> dict[str, object]:
    actual = path.read_bytes()
    expected = canonical_json(build_lock())
    if actual != expected:
        raise ValueError("render pilot lock is missing, noncanonical, or stale")
    value = json.loads(actual)
    if not isinstance(value, dict):
        raise ValueError("render pilot lock must be an object")
    return value


def check_lock(path: Path = DEFAULT_LOCK) -> bool:
    try:
        load_lock(path)
        with tempfile.TemporaryDirectory() as temporary:
            materialize(Path(temporary) / "pilot", build_lock())
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_pilot: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the digest-locked 40-case render pilot."
    )
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true")
    action.add_argument("--refresh-lock", action="store_true")
    args = parser.parse_args(argv)
    try:
        if args.refresh_lock:
            refresh_lock(args.lock)
            print(f"wrote {args.lock}")
            return 0
        if args.check:
            return 0 if check_lock(args.lock) else 1
        manifest = materialize(args.output, load_lock(args.lock))
        print(f"wrote {manifest}")
        return 0
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_pilot: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
