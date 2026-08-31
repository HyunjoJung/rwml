#!/usr/bin/env python3
"""Build the note, field, and revision interaction render corpus batch."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from html import escape
import itertools
import json
import os
from pathlib import Path
import sys
import tempfile

try:
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
    from render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
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
    from scripts.render_oracle_contract import CORPUS_SCHEMA, load_corpus_manifest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
PUBLIC_GENERATOR = ROOT / "scripts" / "gen_public_corpus.py"
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-note-field-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-note-field-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-note-field-v1"
PROVENANCE_ID = "rwml-render-full-note-field"
PROVENANCE_PATH = "provenance/rwml-render-full-note-field.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)
SETTINGS_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"
)
FOOTNOTES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"
)
ENDNOTES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml"
)

PROVENANCE_TEXT = """# Public full-render note, field, and revision batch provenance

The 64 `full-note-field-*` DOCX inputs are generated from repository-owned raw
OOXML by `scripts/generate_render_note_field_corpus.py`. They form the complete
two-level factorial over six factors in one primary note-reference/NOTEREF pair:
footnote or endnote, numbering start one or five, decimal or lower-Roman number
format, simple or complex NOTEREF encoding, plain or accepted-insertion context,
and direct-body or table-cell placement. Every factor level appears in 32 cases
and every factor pair has all four states 16 times.

Every document also carries a deleted note-reference decoy, a custom-mark note
that must not consume the automatic sequence, and accepted/rejected revisions
plus a deterministic formula inside the primary note body. Note IDs and note-part
order are deliberately unrelated to the visible sequence. Page geometry, text,
styles, explicit note settings, package order, and ZIP metadata are deterministic.
The generated documents and this provenance record are licensed under the
repository's MIT license. The checked-in batch lock binds the generator closure
and every generated input by byte length and SHA-256. This batch is diagnostic
corpus material; it does not establish page-bottom note placement, Word-exact
pagination, external-render fidelity, completion of the planned full corpus, or
a release-gate change.
"""

FACTOR_NAMES = (
    "endnote-kind",
    "note-start-five",
    "lower-roman-note-numbering",
    "complex-noteref-field",
    "accepted-insertion-primary",
    "table-cell-primary",
)
FACTOR_FEATURES = (
    ("footnote-kind", "endnote-kind"),
    ("note-start-one", "note-start-five"),
    ("decimal-note-numbering", "lower-roman-note-numbering"),
    ("simple-noteref-field", "complex-noteref-field"),
    ("plain-primary", "accepted-insertion-primary"),
    ("direct-body-primary", "table-cell-primary"),
)
BASE_FEATURES = (
    "accepted-current-revisions",
    "custom-note-mark",
    "deleted-note-reference-decoy",
    "note-body-formula-field",
    "note-body-revisions",
    "source-order-note-numbering",
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
    def endnote(self) -> bool:
        return self.factor_state[0]

    @property
    def start_at_five(self) -> bool:
        return self.factor_state[1]

    @property
    def lower_roman(self) -> bool:
        return self.factor_state[2]

    @property
    def complex_noteref(self) -> bool:
        return self.factor_state[3]

    @property
    def accepted_insertion(self) -> bool:
        return self.factor_state[4]

    @property
    def table_cell(self) -> bool:
        return self.factor_state[5]

    @property
    def expected_primary_marker(self) -> str:
        value = 5 if self.start_at_five else 1
        if not self.lower_roman:
            return str(value)
        return "v" if value == 5 else "i"

    @property
    def features(self) -> tuple[str, ...]:
        selected = [
            levels[int(enabled)]
            for levels, enabled in zip(FACTOR_FEATURES, self.factor_state, strict=True)
        ]
        return tuple(sorted((*BASE_FEATURES, *selected)))


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _factor_state(index: int) -> tuple[bool, bool, bool, bool, bool, bool]:
    if index < 0 or index >= CASE_COUNT:
        raise ValueError(f"invalid note/field factorial index: {index}")
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
        raise ValueError("note/field factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("note/field factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("note/field factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("note/field factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"note/field factorial factor is unbalanced: {factor}")
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
                    "note/field factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-note-field-{index:03d}",
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


def _note_names(spec: CaseSpec) -> tuple[str, str, str, str, str]:
    if spec.endnote:
        return (
            "endnote",
            "endnotes",
            "endnoteRef",
            ENDNOTES_CONTENT_TYPE,
            "Endnote",
        )
    return (
        "footnote",
        "footnotes",
        "footnoteRef",
        FOOTNOTES_CONTENT_TYPE,
        "Footnote",
    )


def _settings(spec: CaseSpec) -> bytes:
    note_kind, _, _, _, _ = _note_names(spec)
    start = 5 if spec.start_at_five else 1
    number_format = "lowerRoman" if spec.lower_roman else "decimal"
    return _b(
        XML_DECL + f'<w:settings xmlns:w="{W}"><w:{note_kind}Pr>'
        f'<w:numFmt w:val="{number_format}"/><w:numStart w:val="{start}"/>'
        f"</w:{note_kind}Pr></w:settings>"
    )


def _paragraph(text: str) -> str:
    return f'<w:p><w:r><w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>'


def _note_reference(spec: CaseSpec, note_id: int, *, custom: bool = False) -> str:
    note_kind, _, _, _, _ = _note_names(spec)
    custom_attribute = ' w:customMarkFollows="1"' if custom else ""
    return f'<w:{note_kind}Reference w:id="{note_id}"{custom_attribute}/>'


def _styled_note_reference(spec: CaseSpec, note_id: int) -> str:
    return (
        '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>'
        + _note_reference(spec, note_id)
        + "</w:r>"
    )


def _revision_wrap(markup: str, revision_id: int) -> str:
    return (
        f'<w:ins w:id="{revision_id}" w:author="rwml" '
        'w:date="2026-01-01T00:00:00Z">' + markup + "</w:ins>"
    )


def _deleted_decoy(spec: CaseSpec) -> str:
    return (
        '<w:p><w:del w:id="700" w:author="rwml" '
        'w:date="2026-01-01T00:00:00Z"><w:r><w:delText>'
        "DELETED NOTE DECOY </w:delText>"
        + _note_reference(spec, 777)
        + "</w:r></w:del></w:p>"
    )


def _custom_mark_control(spec: CaseSpec) -> str:
    return (
        '<w:p><w:r><w:t xml:space="preserve">Custom mark control </w:t>'
        + _note_reference(spec, 90, custom=True)
        + "<w:t>*</w:t></w:r></w:p>"
    )


def _primary_marker_paragraph(spec: CaseSpec) -> str:
    marker = _styled_note_reference(spec, 4)
    if spec.accepted_insertion:
        marker = _revision_wrap(marker, 701)
    return (
        '<w:p><w:r><w:t xml:space="preserve">Primary marker </w:t></w:r>'
        '<w:bookmarkStart w:id="7" w:name="PrimaryNote"/>'
        + marker
        + '<w:bookmarkEnd w:id="7"/><w:r><w:t xml:space="preserve"> expected '
        + escape(spec.expected_primary_marker)
        + "</w:t></w:r></w:p>"
    )


def _noteref_result_run() -> str:
    return (
        '<w:r><w:rPr><w:vertAlign w:val="superscript"/></w:rPr>'
        "<w:t>STALE-NOTEREF</w:t></w:r>"
    )


def _noteref_field(spec: CaseSpec) -> str:
    if not spec.complex_noteref:
        return (
            '<w:fldSimple w:instr=" NOTEREF PrimaryNote ">'
            + _noteref_result_run()
            + "</w:fldSimple>"
        )
    return (
        '<w:r><w:fldChar w:fldCharType="begin"/></w:r>'
        '<w:r><w:instrText xml:space="preserve"> NOTEREF PrimaryNote </w:instrText></w:r>'
        '<w:r><w:fldChar w:fldCharType="separate"/></w:r>'
        + _noteref_result_run()
        + '<w:r><w:fldChar w:fldCharType="end"/></w:r>'
    )


def _noteref_paragraph(spec: CaseSpec) -> str:
    field = _noteref_field(spec)
    if spec.accepted_insertion:
        field = _revision_wrap(field, 702)
    return (
        '<w:p><w:r><w:t xml:space="preserve">Resolved NOTEREF </w:t></w:r>'
        + field
        + "</w:p>"
    )


def _primary_container(spec: CaseSpec) -> str:
    content = _primary_marker_paragraph(spec) + _noteref_paragraph(spec)
    if not spec.table_cell:
        return content
    return (
        '<w:tbl><w:tblPr><w:tblW w:w="7200" w:type="dxa"/>'
        '<w:tblLayout w:type="fixed"/><w:tblBorders>'
        '<w:top w:val="single" w:sz="8" w:color="000000"/>'
        '<w:left w:val="single" w:sz="8" w:color="000000"/>'
        '<w:bottom w:val="single" w:sz="8" w:color="000000"/>'
        '<w:right w:val="single" w:sz="8" w:color="000000"/>'
        '</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="7200"/>'
        '</w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="7200" w:type="dxa"/>'
        "</w:tcPr>" + content + "</w:tc></w:tr></w:tbl>"
    )


def _section() -> str:
    return (
        '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/></w:sectPr>'
    )


def _document_xml(spec: CaseSpec) -> bytes:
    body = _paragraph(f"{spec.case_id} deterministic note/field control")
    body += _deleted_decoy(spec)
    body += _custom_mark_control(spec)
    body += _primary_container(spec)
    return _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>'
        + body
        + _section()
        + "</w:body></w:document>"
    )


def _note_part(spec: CaseSpec) -> bytes:
    note_kind, note_plural, marker_name, _, display_name = _note_names(spec)
    root = note_plural
    return _b(
        XML_DECL + f'<w:{root} xmlns:w="{W}">'
        f'<w:{note_kind} w:type="separator" w:id="-1"><w:p><w:r>'
        f"<w:separator/></w:r></w:p></w:{note_kind}>"
        f'<w:{note_kind} w:type="continuationSeparator" w:id="0"><w:p><w:r>'
        f"<w:continuationSeparator/></w:r></w:p></w:{note_kind}>"
        f'<w:{note_kind} w:id="4"><w:p><w:r><w:{marker_name}/>'
        f'<w:t xml:space="preserve"> Primary {display_name.lower()} body. </w:t></w:r>'
        '<w:ins w:id="801" w:author="rwml" w:date="2026-01-01T00:00:00Z">'
        "<w:r><w:t>accepted note text</w:t></w:r></w:ins>"
        '<w:del w:id="802" w:author="rwml" w:date="2026-01-01T00:00:00Z">'
        "<w:r><w:delText>rejected note text</w:delText></w:r></w:del>"
        '<w:fldSimple w:instr=" = 6 * 7 \\# &quot;0&quot; ">'
        "<w:r><w:t>STALE-FORMULA</w:t></w:r></w:fldSimple>"
        f"</w:p></w:{note_kind}>"
        f'<w:{note_kind} w:id="90"><w:p><w:r><w:{marker_name}/>'
        f'<w:t xml:space="preserve"> Custom mark {display_name.lower()} body.</w:t>'
        f"</w:r></w:p></w:{note_kind}></w:{root}>"
    )


def _docx(spec: CaseSpec) -> bytes:
    note_kind, note_plural, _, note_content_type, _ = _note_names(spec)
    note_path = f"word/{note_plural}.xml"
    content_types = _content_types(
        overrides=[
            ("/word/document.xml", MAIN_CT),
            ("/word/settings.xml", SETTINGS_CONTENT_TYPE),
            ("/word/styles.xml", STYLES_CONTENT_TYPE),
            (f"/{note_path}", note_content_type),
        ],
        defaults=[("rels", RELS_CT), ("xml", "application/xml")],
    )
    package_relationships = _rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    document_relationships = _rels(
        [
            ("rIdNotes", f"{R}/{note_plural}", f"{note_plural}.xml"),
            ("rIdSettings", f"{R}/settings", "settings.xml"),
            ("rIdStyles", f"{R}/styles", "styles.xml"),
        ]
    )
    parts = sorted(
        [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", package_relationships),
            ("word/_rels/document.xml.rels", document_relationships),
            ("word/document.xml", _document_xml(spec)),
            (note_path, _note_part(spec)),
            ("word/settings.xml", _settings(spec)),
            ("word/styles.xml", _styles()),
        ],
        key=lambda item: item[0],
    )
    return _zip(parts)


def build_case(spec: CaseSpec) -> bytes:
    expected_id = f"full-note-field-{spec.index:03d}"
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
    digest = hashlib.sha256()
    for path in sorted((SCRIPT_PATH, PUBLIC_GENERATOR)):
        relative = path.relative_to(ROOT).as_posix().encode("utf-8")
        payload = path.read_bytes()
        digest.update(len(relative).to_bytes(8, "little"))
        digest.update(relative)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


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
        "cohort": "note-field-revision-interactions",
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
            "accepted-current-view",
            "custom-mark-before-primary",
            "deleted-decoy-before-primary",
            "one-page-geometry",
            "primary-note-id-four",
        ],
        "interaction_scope": "primary-note-and-noteref",
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
                "source_path": (
                    f"scripts/generate_render_note_field_corpus.py#{spec.case_id}"
                ),
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
        raise ValueError(f"output must not be a symlink: {path.name}")
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


def _payloads() -> dict[str, bytes]:
    payloads = {spec.relative_path: build_case(spec) for spec in case_specs()}
    payloads[PROVENANCE_PATH] = PROVENANCE_TEXT.encode("utf-8")
    return payloads


def materialize(output: Path, lock: dict[str, object]) -> Path:
    if canonical_json(lock) != canonical_json(build_lock()):
        raise ValueError(
            "render corpus lock does not match the current generator closure"
        )
    if output.is_symlink() or (output.exists() and not output.is_dir()):
        raise ValueError(f"invalid render corpus output directory: {output}")
    if output.exists() and any(output.iterdir()):
        raise ValueError(f"render corpus output directory must be fresh: {output}")
    output.mkdir(parents=True, exist_ok=True)
    for relative_path, payload in sorted(_payloads().items()):
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
        raise ValueError("render corpus lock is missing, noncanonical, or stale")
    value = json.loads(actual)
    if not isinstance(value, dict):
        raise ValueError("render corpus lock must be an object")
    return value


def check_lock(path: Path = DEFAULT_LOCK) -> bool:
    try:
        lock = load_lock(path)
        with tempfile.TemporaryDirectory() as temporary:
            materialize(Path(temporary) / "note-field", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_note_field_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus note/field batch."
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
        print(f"generate_render_note_field_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
