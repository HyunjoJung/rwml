#!/usr/bin/env python3
"""Build a balanced, bounded cross-subsystem render corpus batch."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from html import escape
from pathlib import Path
import sys
import tempfile

try:
    import gen_public_corpus as ooxml
    import generate_render_metafile_corpus as metafiles
    import render_corpus_batch as batch
except ModuleNotFoundError:
    from scripts import gen_public_corpus as ooxml
    from scripts import generate_render_metafile_corpus as metafiles
    from scripts import render_corpus_batch as batch


ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LOCK = ROOT / "corpus/public/oracle/render-full-interaction-v1.json"
DEFAULT_OUTPUT = ROOT / "target/render-oracle/render-full-interaction-v1"
CAMPAIGN = "public-render-full-interaction-v1"
CASE_COUNT = 197
MAX_CASE_BYTES = 128 * 1024
PROVENANCE_ID = "rwml-render-full-interaction"
PROVENANCE_PATH = f"provenance/{PROVENANCE_ID}.md"
W, R = ooxml.W, ooxml.R
WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"
PIC = "http://schemas.openxmlformats.org/drawingml/2006/picture"
WPS = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
CONTENT_TYPE_PREFIX = "application/vnd.openxmlformats-officedocument.wordprocessingml."
FACTOR_NAMES = (
    "highlight",
    "superscript",
    "paragraph-shading",
    "hanging-tab",
    "exact-line-spacing",
    "ordered-list",
    "paragraph-bidi",
    "table-bidi",
    "nested-table",
    "unequal-columns",
    "first-page-header",
    "endnote",
    "field-in-insertion",
    "metafile",
    "floating-top-bottom",
    "keep-controls",
)
FACTOR_MASKS = (1, 2, 4, 8, 16, 32, 64, 128, 3, 5, 9, 17, 33, 65, 129, 7)
FACTOR_LEVELS = (
    ("no-highlight", "highlight"),
    ("baseline-run", "superscript"),
    ("no-paragraph-shading", "paragraph-shading"),
    ("no-tabs", "hanging-tab"),
    ("at-least-line-spacing", "exact-line-spacing"),
    ("bullet-list", "ordered-list"),
    ("ltr-paragraph", "paragraph-bidi"),
    ("ltr-table", "table-bidi"),
    ("flat-table", "nested-table"),
    ("single-column", "unequal-columns"),
    ("default-header", "first-page-header"),
    ("footnote", "endnote"),
    ("direct-field", "field-in-insertion"),
    ("png-image", "metafile"),
    ("no-floating-shape", "floating-top-bottom"),
    ("no-keep-controls", "keep-controls"),
)
PROVENANCE_TEXT = """# Public full-render interaction batch provenance

The 197 `full-interaction-*` DOCX inputs are generated from repository-owned
OOXML by `scripts/generate_render_interaction_corpus.py`. The design selects
rows `(178 + 23 * index) mod 256` from an eight-bit binary lattice. Sixteen
distinct nonzero parity masks select character paint, paragraph geometry,
list direction, table direction and nesting, section columns, running headers,
note placement, accepted-current fields, image format, floating shape flow,
and keep controls. Each factor is enabled in 98 or 99 cases; all four states
of every factor pair occur between 47 and 52 times.

The properties share a document, including note and field content inside table
cells, with nested-table content and narrow section columns where selected.
This is pairwise factor coverage, not exhaustive combinations or a claim that
each selected property forces a visible layout difference. Short documents
bound pagination; keep controls do not claim page-break stress coverage.

PNG bytes and valid single-raster EMF bytes reuse repository-owned generators.
All text, styles, ZIP metadata, part order, and geometry are deterministic.
Inputs and this record are MIT licensed. The lock binds every input and the
generator closure. This diagnostic batch does not establish Word-exact layout,
complete RTL or vector rendering, external-render fidelity, or a release gate.
"""

canonical_json = batch.canonical_json
load_corpus_manifest = batch.load_corpus_manifest


@dataclass(frozen=True)
class CaseSpec:
    index: int
    case_id: str
    design_row: int
    factor_state: tuple[bool, ...]

    @property
    def relative_path(self) -> str:
        return f"documents/{self.case_id}.docx"

    @property
    def features(self) -> tuple[str, ...]:
        return tuple(
            sorted(
                (
                    "cross-subsystem-interactions",
                    *(
                        levels[int(enabled)]
                        for levels, enabled in zip(
                            FACTOR_LEVELS, self.factor_state, strict=True
                        )
                    ),
                )
            )
        )

    @property
    def highlight(self) -> bool:
        return self.factor_state[0]

    @property
    def superscript(self) -> bool:
        return self.factor_state[1]

    @property
    def paragraph_shading(self) -> bool:
        return self.factor_state[2]

    @property
    def hanging_tab(self) -> bool:
        return self.factor_state[3]

    @property
    def exact_line_spacing(self) -> bool:
        return self.factor_state[4]

    @property
    def ordered_list(self) -> bool:
        return self.factor_state[5]

    @property
    def paragraph_bidi(self) -> bool:
        return self.factor_state[6]

    @property
    def table_bidi(self) -> bool:
        return self.factor_state[7]

    @property
    def nested_table(self) -> bool:
        return self.factor_state[8]

    @property
    def unequal_columns(self) -> bool:
        return self.factor_state[9]

    @property
    def first_page_header(self) -> bool:
        return self.factor_state[10]

    @property
    def endnote(self) -> bool:
        return self.factor_state[11]

    @property
    def field_in_insertion(self) -> bool:
        return self.factor_state[12]

    @property
    def metafile(self) -> bool:
        return self.factor_state[13]

    @property
    def floating_top_bottom(self) -> bool:
        return self.factor_state[14]

    @property
    def keep_controls(self) -> bool:
        return self.factor_state[15]


def _spec(index: int) -> CaseSpec:
    if type(index) is not int or not 0 <= index < CASE_COUNT:
        raise ValueError(f"invalid interaction index: {index}")
    row = (178 + 23 * index) % 256
    return CaseSpec(
        index,
        f"full-interaction-{index:03d}",
        row,
        tuple((row & mask).bit_count() % 2 == 0 for mask in FACTOR_MASKS),
    )


def _pairwise_state_counts(specs: tuple[CaseSpec, ...]) -> list[dict[str, object]]:
    pairs = []
    for left in range(len(FACTOR_NAMES)):
        for right in range(left + 1, len(FACTOR_NAMES)):
            states = dict.fromkeys(("00", "01", "10", "11"), 0)
            for spec in specs:
                state = f"{int(spec.factor_state[left])}{int(spec.factor_state[right])}"
                states[state] += 1
            pairs.append(
                {"factors": [FACTOR_NAMES[left], FACTOR_NAMES[right]], "states": states}
            )
    return pairs


def _validate_specs(specs: tuple[CaseSpec, ...]) -> None:
    if len(specs) != CASE_COUNT:
        raise ValueError("interaction case count is incomplete")
    if len({spec.design_row for spec in specs}) != CASE_COUNT:
        raise ValueError("interaction design rows are not unique")
    if any(spec != _spec(index) for index, spec in enumerate(specs)):
        raise ValueError("invalid deterministic interaction case identity")
    for position in range(len(FACTOR_NAMES)):
        if sum(spec.factor_state[position] for spec in specs) not in (98, 99):
            raise ValueError("interaction factor is unbalanced")
    for pair in _pairwise_state_counts(specs):
        if not all(47 <= count <= 52 for count in pair["states"].values()):
            raise ValueError("interaction pairwise coverage is unbalanced")


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(_spec(index) for index in range(CASE_COUNT))
    _validate_specs(specs)
    return specs


def _run(text: str, properties: str = "") -> str:
    rpr = f"<w:rPr>{properties}</w:rPr>" if properties else ""
    return f'<w:r>{rpr}<w:t xml:space="preserve">{escape(text)}</w:t></w:r>'


def _paragraph(text: str) -> str:
    return f"<w:p>{_run(text)}</w:p>"


def _control(spec: CaseSpec) -> str:
    line_rule = "exact" if spec.exact_line_spacing else "atLeast"
    ppr = f'<w:spacing w:line="320" w:lineRule="{line_rule}" w:after="80"/>'
    if spec.paragraph_shading:
        ppr += '<w:shd w:val="clear" w:fill="DDEBF7"/>'
    if spec.hanging_tab:
        ppr += '<w:tabs><w:tab w:val="left" w:pos="600"/></w:tabs>'
        ppr += '<w:ind w:left="600" w:hanging="240"/>'
    rpr = '<w:highlight w:val="yellow"/>' if spec.highlight else ""
    if spec.superscript:
        rpr += '<w:vertAlign w:val="superscript"/>'
    tab = "<w:r><w:tab/></w:r>" if spec.hanging_tab else ""
    return f"<w:p><w:pPr>{ppr}</w:pPr>{_run('Control', rpr)}{tab}{_run(' ' + spec.case_id)}</w:p>"


def _numbering() -> bytes:
    definitions = []
    for num_id, fmt, text in ((1, "bullet", "&#8226;"), (2, "decimal", "%1.")):
        definitions.append(
            f'<w:abstractNum w:abstractNumId="{num_id}"><w:multiLevelType w:val="singleLevel"/>'
            f'<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="{fmt}"/>'
            f'<w:lvlText w:val="{text}"/><w:lvlJc w:val="left"/>'
            '<w:pPr><w:tabs><w:tab w:val="num" w:pos="360"/></w:tabs>'
            '<w:ind w:left="360" w:hanging="180"/></w:pPr></w:lvl></w:abstractNum>'
        )
    references = "".join(
        f'<w:num w:numId="{num_id}"><w:abstractNumId w:val="{num_id}"/></w:num>'
        for num_id in (1, 2)
    )
    return ooxml._b(
        ooxml.XML_DECL
        + f'<w:numbering xmlns:w="{W}">'
        + "".join(definitions)
        + references
        + "</w:numbering>"
    )


def _list(spec: CaseSpec) -> str:
    bidi = "<w:bidi/>" if spec.paragraph_bidi else ""
    text = (
        "\u05e9\u05dc\u05d5\u05dd 123 alpha"
        if spec.paragraph_bidi
        else "List alpha 123"
    )
    return (
        '<w:p><w:pPr><w:numPr><w:ilvl w:val="0"/>'
        f'<w:numId w:val="{2 if spec.ordered_list else 1}"/></w:numPr>{bidi}'
        "</w:pPr>" + _run(text, "<w:rtl/>" if spec.paragraph_bidi else "") + "</w:p>"
    )


def _field(spec: CaseSpec) -> str:
    field = (
        '<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
        '<w:r><w:instrText xml:space="preserve"> = 6 * 7 </w:instrText></w:r>'
        '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
        + _run("42")
        + '<w:r><w:fldChar w:fldCharType="end"/></w:r>'
    )
    if spec.field_in_insertion:
        field = (
            '<w:ins w:id="1" w:author="rwml" w:date="2026-01-01T00:00:00Z">'
            + field
            + "</w:ins>"
        )
    return "<w:p>" + _run("Formula ") + field + "</w:p>"


def _table(spec: CaseSpec) -> str:
    note_kind = "endnote" if spec.endnote else "footnote"
    first_cell = (
        "<w:p>" + _run("Note ") + f'<w:r><w:{note_kind}Reference w:id="1"/></w:r></w:p>'
    )
    if spec.nested_table:
        first_cell += (
            '<w:tbl><w:tblPr><w:tblW w:w="1440" w:type="dxa"/>'
            '<w:tblLayout w:type="fixed"/></w:tblPr><w:tblGrid><w:gridCol w:w="1440"/>'
            '</w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="1440" w:type="dxa"/>'
            "</w:tcPr>" + _paragraph("Nested cell") + "</w:tc></w:tr></w:tbl><w:p/>"
        )
    else:
        first_cell += _paragraph("Flat cell")
    bidi = "<w:bidiVisual/>" if spec.table_bidi else ""
    borders = "".join(
        f'<w:{edge} w:val="single" w:sz="4" w:color="405060"/>'
        for edge in ("top", "left", "bottom", "right", "insideH", "insideV")
    )
    cells = "".join(
        '<w:tc><w:tcPr><w:tcW w:w="1800" w:type="dxa"/></w:tcPr>' + content + "</w:tc>"
        for content in (first_cell, _field(spec))
    )
    return (
        '<w:tbl><w:tblPr><w:tblW w:w="3600" w:type="dxa"/><w:tblLayout w:type="fixed"/>'
        + bidi
        + f"<w:tblBorders>{borders}</w:tblBorders></w:tblPr>"
        '<w:tblGrid><w:gridCol w:w="1800"/><w:gridCol w:w="1800"/></w:tblGrid>'
        f"<w:tr>{cells}</w:tr></w:tbl>"
    )


def _drawing() -> str:
    return (
        '<w:p><w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0">'
        '<wp:extent cx="914400" cy="457200"/><wp:docPr id="1" name="Raster control"/>'
        f'<a:graphic><a:graphicData uri="{PIC}"><pic:pic><pic:nvPicPr>'
        '<pic:cNvPr id="0" name="Raster"/><pic:cNvPicPr/></pic:nvPicPr>'
        '<pic:blipFill><a:blip r:embed="rIdImage"/><a:stretch><a:fillRect/></a:stretch>'
        '</pic:blipFill><pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="914400" cy="457200"/>'
        '</a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr></pic:pic>'
        "</a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
    )


def _floating() -> str:
    return (
        '<w:r><w:drawing><wp:anchor simplePos="0" relativeHeight="2" behindDoc="0" '
        'locked="0" layoutInCell="1" allowOverlap="1" distT="0" distB="0" distL="0" distR="0">'
        '<wp:simplePos x="0" y="0"/><wp:positionH relativeFrom="column"><wp:posOffset>0</wp:posOffset>'
        '</wp:positionH><wp:positionV relativeFrom="paragraph"><wp:posOffset>0</wp:posOffset>'
        '</wp:positionV><wp:extent cx="914400" cy="228600"/><wp:wrapTopAndBottom/>'
        '<wp:docPr id="2" name="Floating control"/>'
        f'<a:graphic><a:graphicData uri="{WPS}"><wps:wsp><wps:cNvSpPr txBox="1"/>'
        '<wps:spPr><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr>'
        "<wps:txbx><w:txbxContent>"
        + _paragraph("Floating text")
        + "</w:txbxContent></wps:txbx>"
        "<wps:bodyPr/></wps:wsp></a:graphicData></a:graphic></wp:anchor></w:drawing></w:r>"
    )


def _section(spec: CaseSpec) -> str:
    references = '<w:headerReference w:type="default" r:id="rIdHeader"/>'
    if spec.first_page_header:
        references += '<w:headerReference w:type="first" r:id="rIdFirstHeader"/>'
    columns = (
        '<w:cols w:num="2" w:equalWidth="0"><w:col w:w="4200" w:space="720"/>'
        '<w:col w:w="5880"/></w:cols>'
        if spec.unequal_columns
        else ""
    )
    return (
        "<w:sectPr>" + references + '<w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/>'
        + columns
        + ("<w:titlePg/>" if spec.first_page_header else "")
        + "</w:sectPr>"
    )


def _note(spec: CaseSpec) -> bytes:
    kind = "endnote" if spec.endnote else "footnote"
    body = (
        f'<w:{kind} w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:{kind}>'
        f'<w:{kind} w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/>'
        f'</w:r></w:p></w:{kind}><w:{kind} w:id="1"><w:p><w:r><w:{kind}Ref/>'
        f'<w:t xml:space="preserve"> {kind} body.</w:t></w:r></w:p></w:{kind}>'
    )
    return ooxml._b(
        ooxml.XML_DECL + f'<w:{kind}s xmlns:w="{W}">' + body + f"</w:{kind}s>"
    )


def _header(text: str) -> bytes:
    return ooxml._b(
        ooxml.XML_DECL + f'<w:hdr xmlns:w="{W}">' + _paragraph(text) + "</w:hdr>"
    )


def build_case(spec: CaseSpec) -> bytes:
    if spec != _spec(spec.index):
        raise ValueError(f"invalid deterministic case identity: {spec.case_id}")
    keep = "<w:pPr><w:keepNext/><w:keepLines/></w:pPr>" if spec.keep_controls else ""
    body = _control(spec) + _list(spec) + _table(spec) + _drawing()
    body += (
        "<w:p>"
        + keep
        + _run("Flow control ")
        + (_floating() if spec.floating_top_bottom else "")
        + "</w:p>"
    )
    body += _paragraph("Following text alpha beta gamma delta.") + _section(spec)
    document = ooxml._b(
        ooxml.XML_DECL + f'<w:document xmlns:w="{W}" xmlns:r="{R}" xmlns:wp="{WP}" '
        f'xmlns:a="{A}" xmlns:pic="{PIC}" xmlns:wps="{WPS}"><w:body>'
        + body
        + "</w:body></w:document>"
    )
    note_name = "endnotes" if spec.endnote else "footnotes"
    extension = "emf" if spec.metafile else "png"
    media = (
        metafiles.build_metafile(
            metafiles.CaseSpec(0, "full-metafile-000", (False,) * 6)
        )
        if spec.metafile
        else ooxml.TINY_PNG
    )
    parts = {
        "word/document.xml": document,
        "word/styles.xml": metafiles._styles(),
        "word/numbering.xml": _numbering(),
        "word/header1.xml": _header("Default header"),
        f"word/{note_name}.xml": _note(spec),
        f"word/media/image1.{extension}": media,
    }
    relationships = [
        ("rIdStyles", f"{R}/styles", "styles.xml"),
        ("rIdNumbering", f"{R}/numbering", "numbering.xml"),
        ("rIdHeader", f"{R}/header", "header1.xml"),
        ("rIdNotes", f"{R}/{note_name}", f"{note_name}.xml"),
        ("rIdImage", f"{R}/image", f"media/image1.{extension}"),
    ]
    overrides = [("/word/document.xml", ooxml.MAIN_CT)] + [
        (f"/word/{name}.xml", CONTENT_TYPE_PREFIX + kind + "+xml")
        for name, kind in [
            ("styles", "styles"),
            ("numbering", "numbering"),
            ("header1", "header"),
            (note_name, note_name),
        ]
    ]
    if spec.first_page_header:
        parts["word/header2.xml"] = _header("First page header")
        overrides.append(("/word/header2.xml", CONTENT_TYPE_PREFIX + "header+xml"))
        relationships.append(("rIdFirstHeader", f"{R}/header", "header2.xml"))
    parts["[Content_Types].xml"] = ooxml._content_types(
        overrides,
        [
            ("rels", ooxml.RELS_CT),
            ("xml", "application/xml"),
            (extension, "image/x-emf" if spec.metafile else "image/png"),
        ],
    )
    parts["_rels/.rels"] = ooxml._rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    parts["word/_rels/document.xml.rels"] = ooxml._rels(relationships)
    payload = ooxml._zip(sorted(parts.items()))
    if not 0 < len(payload) <= MAX_CASE_BYTES:
        raise ValueError(f"case byte limit exceeded: {spec.case_id}")
    return payload


def _build_batch() -> tuple[dict[str, object], dict[str, bytes]]:
    specs = case_specs()
    payloads = {spec.relative_path: build_case(spec) for spec in specs}
    documents = []
    for spec in specs:
        payload = payloads[spec.relative_path]
        documents.append(
            {
                "id": spec.case_id,
                "path": spec.relative_path,
                "format": "docx",
                "bytes": len(payload),
                "sha256": hashlib.sha256(payload).hexdigest(),
                "features": list(spec.features),
                "provenance": PROVENANCE_ID,
                "expected": {
                    "pages": 1,
                    "warnings": ["FloatingShapePlaceholderOnly"]
                    if spec.floating_top_bottom
                    else [],
                },
                "source": "generated",
                "source_path": f"scripts/{Path(__file__).name}#{spec.case_id}",
            }
        )
    if len({item["sha256"] for item in documents}) != CASE_COUNT:
        raise ValueError("duplicate generated interaction payload")
    provenance = PROVENANCE_TEXT.encode("utf-8")
    payloads[PROVENANCE_PATH] = provenance
    closure = tuple(
        Path(module.__file__).resolve() for module in (ooxml, metafiles, batch)
    )
    lock = {
        "schema": "rwml.render-corpus-batch-lock.v1",
        "campaign": CAMPAIGN,
        "generator_closure_sha256": batch.generator_closure_sha256(
            ROOT, (*closure, Path(__file__).resolve())
        ),
        "documents": documents,
        "provenance": [
            {
                "id": PROVENANCE_ID,
                "reference": PROVENANCE_PATH,
                "kind": "generated",
                "license": "MIT",
                "bytes": len(provenance),
                "sha256": hashlib.sha256(provenance).hexdigest(),
            }
        ],
        "limits": {
            "max_documents": CASE_COUNT,
            "max_input_bytes": MAX_CASE_BYTES,
            "max_pages_per_document": 4,
            "max_total_input_bytes": CASE_COUNT * MAX_CASE_BYTES,
        },
        "coverage": {
            "cohort": "cross-subsystem-interactions",
            "case_count": CASE_COUNT,
            "design": "deterministic-197-row-binary-lattice-slice",
            "factor_names": list(FACTOR_NAMES),
            "factor_masks": dict(zip(FACTOR_NAMES, FACTOR_MASKS, strict=True)),
            "design_rows": [spec.design_row for spec in specs],
            "interaction_scope": "document",
            "factor_levels": dict(zip(FACTOR_NAMES, FACTOR_LEVELS, strict=True)),
            "factor_case_counts": {
                name: sum(spec.factor_state[i] for spec in specs)
                for i, name in enumerate(FACTOR_NAMES)
            },
            "pairwise_state_counts": _pairwise_state_counts(specs),
        },
    }
    return lock, payloads


def build_lock() -> dict[str, object]:
    return _build_batch()[0]


def load_lock(path: Path = DEFAULT_LOCK) -> dict[str, object]:
    return batch.load_lock(path, build_lock())


def materialize(output: Path, lock: dict[str, object]) -> Path:
    current, payloads = _build_batch()
    return batch.materialize(output, lock, current, payloads)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 197-case interaction batch."
    )
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT)
    parser.add_argument("--lock", type=Path, default=DEFAULT_LOCK)
    action = parser.add_mutually_exclusive_group()
    action.add_argument("--check", action="store_true")
    action.add_argument("--refresh-lock", action="store_true")
    args = parser.parse_args(argv)
    try:
        current, payloads = _build_batch()
        if args.refresh_lock:
            batch.atomic_write(args.lock, canonical_json(current))
        else:
            lock = batch.load_lock(args.lock, current)
            if args.check:
                with tempfile.TemporaryDirectory() as tmp:
                    batch.materialize(
                        Path(tmp) / "interaction", lock, current, payloads
                    )
            else:
                print(batch.materialize(args.output, lock, current, payloads))
        return 0
    except (OSError, ValueError) as error:
        print(f"generate_render_interaction_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
