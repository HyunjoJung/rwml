#!/usr/bin/env python3
"""Build the table topology/paint batch of the deterministic render corpus."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from html import escape
import itertools
from pathlib import Path
import sys
import tempfile

try:
    import render_corpus_batch as batch
    from gen_public_corpus import (
        MAIN_CT,
        R,
        RELS_CT,
        W,
        XML_DECL,
        _b,
        _content_types,
        _rels,
        _zip,
    )
    from render_oracle_contract import load_corpus_manifest
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts import render_corpus_batch as batch
    from scripts.gen_public_corpus import (
        MAIN_CT,
        R,
        RELS_CT,
        W,
        XML_DECL,
        _b,
        _content_types,
        _rels,
        _zip,
    )
    from scripts.render_oracle_contract import load_corpus_manifest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
PUBLIC_GENERATOR = ROOT / "scripts" / "gen_public_corpus.py"
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-table-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-table-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-table-v1"
PROVENANCE_ID = "rwml-render-full-table"
PROVENANCE_PATH = "provenance/rwml-render-full-table.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)

PROVENANCE_TEXT = """# Public full-render table topology and paint batch provenance

The 64 `full-table-*` DOCX inputs are generated from repository-owned raw OOXML
by `scripts/generate_render_table_corpus.py`. They form the complete two-level
factorial over six factors in one primary table: logical or visual-RTL order,
separate cells or a horizontal grid span, separate rows or a vertical merge,
uniform or asymmetric borders, unshaded or shaded owner cell, and inherited or
direct owner-cell margins. Every factor level appears in 32 cases and every
factor pair has all four states 16 times. A fixed bottom-aligned cell with a
taller peer makes vertical cell placement observable in every document.

Table width, fixed-layout policy, grid widths, page geometry, and row height
drivers are held constant. Column layouts, width-policy variation, row
fragmentation, and column/page handoff belong to the separate unequal-column
oracle. Package order, ZIP metadata, text, styles, case identities, and property
values are deterministic. The generated documents and this provenance record
are licensed under the repository's MIT license. The checked-in batch lock binds
the generator closure and every generated input by byte length and SHA-256. This
batch is diagnostic corpus material; it does not claim Word parity, completion
of the planned full corpus, or a change to release validation.
"""

FACTOR_NAMES = (
    "table-bidi-visual",
    "horizontal-grid-span",
    "vertical-row-span",
    "asymmetric-borders",
    "cell-shading",
    "direct-cell-margins",
)
FACTOR_FEATURES = (
    ("table-ltr", "table-bidi-visual"),
    ("separate-columns", "horizontal-grid-span"),
    ("separate-rows", "vertical-row-span"),
    ("uniform-borders", "asymmetric-borders"),
    ("unshaded-cell", "cell-shading"),
    ("inherited-cell-margins", "direct-cell-margins"),
)
BASE_FEATURES = (
    "cell-bottom-alignment",
    "fixed-table-layout",
    "table-topology-paint",
    "three-column-grid",
)


@dataclass(frozen=True)
class CaseSpec:
    index: int
    case_id: str
    factor_state: tuple[bool, bool, bool, bool, bool, bool]

    @property
    def relative_path(self) -> str:
        return f"documents/{self.case_id}.docx"

    @property
    def bidi_visual(self) -> bool:
        return self.factor_state[0]

    @property
    def horizontal_span(self) -> bool:
        return self.factor_state[1]

    @property
    def vertical_span(self) -> bool:
        return self.factor_state[2]

    @property
    def asymmetric_borders(self) -> bool:
        return self.factor_state[3]

    @property
    def cell_shading(self) -> bool:
        return self.factor_state[4]

    @property
    def direct_cell_margins(self) -> bool:
        return self.factor_state[5]

    @property
    def features(self) -> tuple[str, ...]:
        selected = [
            levels[int(enabled)]
            for levels, enabled in zip(FACTOR_FEATURES, self.factor_state, strict=True)
        ]
        return tuple(sorted((*BASE_FEATURES, *selected)))


def canonical_json(value: object) -> bytes:
    return batch.canonical_json(value)


def _factor_state(index: int) -> tuple[bool, bool, bool, bool, bool, bool]:
    if index < 0 or index >= CASE_COUNT:
        raise ValueError(f"invalid table factorial index: {index}")
    return (
        bool(index & 0b000001),
        bool(index & 0b000010),
        bool(index & 0b000100),
        bool(index & 0b001000),
        bool(index & 0b010000),
        bool(index & 0b100000),
    )


def _validate_specs(specs: tuple[CaseSpec, ...]) -> None:
    if len(specs) != CASE_COUNT:
        raise ValueError("table factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("table factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("table factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("table factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"table factorial factor is unbalanced: {factor}")
    expected_pair = {
        (False, False): CASE_COUNT // 4,
        (False, True): CASE_COUNT // 4,
        (True, False): CASE_COUNT // 4,
        (True, True): CASE_COUNT // 4,
    }
    for left in range(len(FACTOR_NAMES)):
        for right in range(left + 1, len(FACTOR_NAMES)):
            counts = {
                state: sum(
                    (spec.factor_state[left], spec.factor_state[right]) == state
                    for spec in specs
                )
                for state in expected_pair
            }
            if counts != expected_pair:
                raise ValueError(
                    "table factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-table-{index:03d}",
            factor_state=_factor_state(index),
        )
        for index in range(CASE_COUNT)
    )
    _validate_specs(specs)
    return specs


def _styles() -> bytes:
    return _b(
        XML_DECL + f'<w:styles xmlns:w="{W}">'
        '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Noto Sans" '
        'w:hAnsi="Noto Sans" w:eastAsia="Noto Sans" w:cs="Noto Sans"/>'
        '<w:sz w:val="20"/><w:szCs w:val="20"/></w:rPr></w:rPrDefault>'
        '<w:pPrDefault><w:pPr><w:spacing w:before="0" w:after="0"/>'
        "</w:pPr></w:pPrDefault></w:docDefaults>"
        '<w:style w:type="paragraph" w:default="1" w:styleId="Normal">'
        '<w:name w:val="Normal"/></w:style></w:styles>'
    )


def _section() -> str:
    return (
        '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/></w:sectPr>'
    )


def _paragraph(text: str) -> str:
    return f'<w:p><w:r><w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>'


def _border(edge: str, style: str, color: str, size: int) -> str:
    return f'<w:{edge} w:val="{style}" w:sz="{size}" w:space="0" w:color="{color}"/>'


def _table_borders(asymmetric: bool) -> str:
    if asymmetric:
        values = (
            ("top", "double", "C00000", 16),
            ("left", "dotted", "008000", 8),
            ("bottom", "dashed", "0000C0", 12),
            ("right", "single", "7030A0", 10),
            ("insideH", "dotted", "C08000", 6),
            ("insideV", "dashed", "008080", 6),
        )
    else:
        values = tuple(
            (edge, "single", "000000", 8)
            for edge in ("top", "left", "bottom", "right", "insideH", "insideV")
        )
    return (
        "<w:tblBorders>"
        + "".join(_border(*value) for value in values)
        + "</w:tblBorders>"
    )


def _table_properties(spec: CaseSpec) -> str:
    bidi = "<w:bidiVisual/>" if spec.bidi_visual else ""
    return (
        "<w:tblPr>"
        '<w:tblW w:w="7200" w:type="dxa"/><w:tblLayout w:type="fixed"/>'
        + bidi
        + _table_borders(spec.asymmetric_borders)
        + '<w:tblCellMar><w:top w:w="80" w:type="dxa"/>'
        '<w:right w:w="80" w:type="dxa"/>'
        '<w:bottom w:w="80" w:type="dxa"/>'
        '<w:left w:w="80" w:type="dxa"/></w:tblCellMar></w:tblPr>'
    )


def _direct_margins() -> str:
    return (
        '<w:tcMar><w:top w:w="240" w:type="dxa"/>'
        '<w:right w:w="240" w:type="dxa"/>'
        '<w:bottom w:w="240" w:type="dxa"/>'
        '<w:left w:w="240" w:type="dxa"/></w:tcMar>'
    )


def _cell(properties: str, *paragraphs: str) -> str:
    tc_pr = f"<w:tcPr>{properties}</w:tcPr>" if properties else "<w:tcPr/>"
    return "<w:tc>" + tc_pr + "".join(paragraphs) + "</w:tc>"


def _primary_table(spec: CaseSpec) -> str:
    owner_properties = (
        '<w:tcW w:w="4800" w:type="dxa"/>'
        if spec.horizontal_span
        else '<w:tcW w:w="2400" w:type="dxa"/>'
    )
    if spec.horizontal_span:
        owner_properties += '<w:gridSpan w:val="2"/>'
    if spec.cell_shading:
        owner_properties += '<w:shd w:val="clear" w:color="auto" w:fill="DDEBF7"/>'
    if spec.direct_cell_margins:
        owner_properties += _direct_margins()
    top_cells = [
        _cell(
            owner_properties,
            _paragraph(f"PRIMARY {spec.case_id} OWNER"),
            _paragraph("paint and margin probe"),
        )
    ]
    if not spec.horizontal_span:
        top_cells.append(_cell("", _paragraph("TOP B")))
    top_cells.append(_cell("", _paragraph("TOP C")))

    vertical_properties = '<w:tcW w:w="2400" w:type="dxa"/>'
    if spec.vertical_span:
        vertical_properties += '<w:vMerge w:val="restart"/>'
    vertical_properties += '<w:vAlign w:val="bottom"/>'
    middle_cells = (
        _cell(
            '<w:tcW w:w="2400" w:type="dxa"/>',
            _paragraph("TALL A1"),
            _paragraph("TALL A2"),
            _paragraph("TALL A3"),
        )
        + _cell('<w:tcW w:w="2400" w:type="dxa"/>', _paragraph("MIDDLE B"))
        + _cell(vertical_properties, _paragraph("BOTTOM-ALIGNED C"))
    )
    bottom_last = (
        _cell('<w:tcW w:w="2400" w:type="dxa"/><w:vMerge/>', _paragraph(""))
        if spec.vertical_span
        else _cell('<w:tcW w:w="2400" w:type="dxa"/>', _paragraph("BOTTOM C"))
    )
    bottom_cells = (
        _cell('<w:tcW w:w="2400" w:type="dxa"/>', _paragraph("BOTTOM A"))
        + _cell('<w:tcW w:w="2400" w:type="dxa"/>', _paragraph("BOTTOM B"))
        + bottom_last
    )
    grid = (
        '<w:tblGrid><w:gridCol w:w="2400"/><w:gridCol w:w="2400"/>'
        '<w:gridCol w:w="2400"/></w:tblGrid>'
    )
    return (
        "<w:tbl>"
        + _table_properties(spec)
        + grid
        + "<w:tr>"
        + "".join(top_cells)
        + "</w:tr><w:tr>"
        + middle_cells
        + "</w:tr><w:tr>"
        + bottom_cells
        + "</w:tr></w:tbl>"
    )


def _document_xml(spec: CaseSpec) -> bytes:
    title = _paragraph(f"{spec.case_id} deterministic table control")
    return _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>'
        + title
        + _primary_table(spec)
        + _section()
        + "</w:body></w:document>"
    )


def _docx(spec: CaseSpec) -> bytes:
    content_types = _content_types(
        overrides=[
            ("/word/document.xml", MAIN_CT),
            ("/word/styles.xml", STYLES_CONTENT_TYPE),
        ],
        defaults=[("rels", RELS_CT), ("xml", "application/xml")],
    )
    package_relationships = _rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    document_relationships = _rels([("rIdStyles", f"{R}/styles", "styles.xml")])
    parts = sorted(
        [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", package_relationships),
            ("word/_rels/document.xml.rels", document_relationships),
            ("word/document.xml", _document_xml(spec)),
            ("word/styles.xml", _styles()),
        ],
        key=lambda item: item[0],
    )
    return _zip(parts)


def build_case(spec: CaseSpec) -> bytes:
    expected_id = f"full-table-{spec.index:03d}"
    if (
        spec.index < 0
        or spec.index >= CASE_COUNT
        or spec.case_id != expected_id
        or spec.factor_state != _factor_state(spec.index)
    ):
        raise ValueError(f"invalid deterministic case identity: {spec.case_id}")
    payload = _docx(spec)
    if len(payload) <= 0 or len(payload) > MAX_CASE_BYTES:
        raise ValueError(f"case byte limit exceeded: {spec.case_id}")
    return payload


def _generator_closure_sha256() -> str:
    return batch.generator_closure_sha256(
        ROOT,
        (
            SCRIPT_PATH,
            PUBLIC_GENERATOR,
            Path(batch.__file__).resolve(),
            ROOT / "scripts" / "render_oracle_contract.py",
        ),
    )


def _pairwise_state_counts(specs: tuple[CaseSpec, ...]) -> list[dict[str, object]]:
    rows = []
    for left in range(len(FACTOR_NAMES)):
        for right in range(left + 1, len(FACTOR_NAMES)):
            states = {"00": 0, "01": 0, "10": 0, "11": 0}
            for spec in specs:
                state = f"{int(spec.factor_state[left])}{int(spec.factor_state[right])}"
                states[state] += 1
            rows.append(
                {"factors": [FACTOR_NAMES[left], FACTOR_NAMES[right]], "states": states}
            )
    return rows


def _coverage(specs: tuple[CaseSpec, ...]) -> dict[str, object]:
    return {
        "case_count": len(specs),
        "cohort": "table-topology-paint",
        "design": "complete-2-level-factorial",
        "factor_case_counts": {
            factor: sum(spec.factor_state[position] for spec in specs)
            for position, factor in enumerate(FACTOR_NAMES)
        },
        "factor_levels": {
            factor: list(levels)
            for factor, levels in zip(FACTOR_NAMES, FACTOR_FEATURES, strict=True)
        },
        "factor_names": list(FACTOR_NAMES),
        "factorial_rows": CASE_COUNT,
        "held_constant": [
            "fixed-table-layout",
            "one-page-geometry",
            "three-equal-grid-columns",
            "table-width-dxa-7200",
        ],
        "interaction_scope": "primary-table",
        "pairwise_state_counts": _pairwise_state_counts(specs),
    }


def _provenance_record() -> dict[str, object]:
    payload = PROVENANCE_TEXT.encode("utf-8")
    return {
        "id": PROVENANCE_ID,
        "kind": "generated",
        "license": "MIT",
        "reference": PROVENANCE_PATH,
        "bytes": len(payload),
        "sha256": hashlib.sha256(payload).hexdigest(),
    }


def build_lock() -> dict[str, object]:
    specs = case_specs()
    documents = []
    payload_hashes = set()
    total_bytes = 0
    for spec in specs:
        payload = build_case(spec)
        total_bytes += len(payload)
        payload_sha256 = hashlib.sha256(payload).hexdigest()
        if payload_sha256 in payload_hashes:
            raise ValueError(f"duplicate generated payload: {spec.case_id}")
        payload_hashes.add(payload_sha256)
        documents.append(
            {
                "bytes": len(payload),
                "expected": {"pages": 1, "warnings": []},
                "features": list(spec.features),
                "format": "docx",
                "id": spec.case_id,
                "path": spec.relative_path,
                "provenance": PROVENANCE_ID,
                "sha256": payload_sha256,
                "source": "generated",
                "source_path": f"scripts/generate_render_table_corpus.py#{spec.case_id}",
            }
        )
    if total_bytes > MAX_TOTAL_BYTES:
        raise ValueError("batch total byte limit exceeded")
    return {
        "campaign": CAMPAIGN,
        "coverage": _coverage(specs),
        "documents": documents,
        "generator_closure_sha256": _generator_closure_sha256(),
        "limits": {
            "max_documents": CASE_COUNT,
            "max_input_bytes": MAX_CASE_BYTES,
            "max_pages_per_document": 4,
            "max_total_input_bytes": MAX_TOTAL_BYTES,
        },
        "provenance": [_provenance_record()],
        "schema": LOCK_SCHEMA,
    }


def _payloads() -> dict[str, bytes]:
    payloads = {spec.relative_path: build_case(spec) for spec in case_specs()}
    payloads[PROVENANCE_PATH] = PROVENANCE_TEXT.encode("utf-8")
    return payloads


def materialize(output: Path, lock: dict[str, object]) -> Path:
    return batch.materialize(output, lock, build_lock(), _payloads())


def refresh_lock(path: Path = DEFAULT_LOCK) -> None:
    batch.atomic_write(path, canonical_json(build_lock()))


def load_lock(path: Path = DEFAULT_LOCK) -> dict[str, object]:
    return batch.load_lock(path, build_lock())


def check_lock(path: Path = DEFAULT_LOCK) -> bool:
    try:
        lock = load_lock(path)
        with tempfile.TemporaryDirectory() as temporary:
            materialize(Path(temporary) / "table", lock)
        return True
    except (OSError, ValueError) as error:
        print(f"generate_render_table_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus table batch."
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
    except (OSError, ValueError) as error:
        print(f"generate_render_table_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
