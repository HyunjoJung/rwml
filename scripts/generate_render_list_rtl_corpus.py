#!/usr/bin/env python3
"""Build the list/RTL batch of the deterministic full render corpus."""

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
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-list-rtl-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-list-rtl-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-list-rtl-v1"
PROVENANCE_ID = "rwml-render-full-list-rtl"
PROVENANCE_PATH = "provenance/rwml-render-full-list-rtl.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)
NUMBERING_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"
)

PROVENANCE_TEXT = """# Public full-render list and RTL batch provenance

The 64 `full-list-rtl-*` DOCX inputs are generated from repository-owned raw
OOXML by `scripts/generate_render_list_rtl_corpus.py`. They form the complete
two-level factorial over six factors in one primary list paragraph: Arabic or
Hebrew script, paragraph bidi off or on, inherited or explicit RTL run direction,
bullet or ordered numbering, list level zero or one, and plain or explicitly
tabbed content. Every factor level appears in 32 cases and every factor pair has
all four states 16 times.

Each document also carries fixed probes for a numbering start override, a full
level replacement, a three-level decimal/letter/Roman label, and an undeclared
bullet glyph that exercises renderer fallback. Package order, ZIP metadata,
text, styles, page geometry, case identities, and property values are
deterministic. The generated documents and this provenance record are licensed
under the repository's MIT license. The checked-in batch lock binds the
generator closure and every generated input by byte length and SHA-256. This
batch is diagnostic corpus material; it does not claim complete RTL support,
completion of the planned full corpus, or a change to release validation.
"""

FACTOR_NAMES = (
    "arabic-script",
    "paragraph-bidi",
    "run-rtl",
    "ordered-list",
    "list-level-one",
    "explicit-tabs",
)
FACTOR_FEATURES = (
    ("script-hebrew", "script-arabic"),
    ("paragraph-ltr", "paragraph-bidi"),
    ("run-inherited", "run-rtl"),
    ("bullet-list", "ordered-list"),
    ("list-level-0", "list-level-1"),
    ("plain-separator", "explicit-tabs"),
)
BASE_FEATURES = (
    "bullet-fallback",
    "list-rtl",
    "multilevel-numbering",
    "numbering-level-override",
    "numbering-start-override",
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
    def arabic(self) -> bool:
        return self.factor_state[0]

    @property
    def paragraph_bidi(self) -> bool:
        return self.factor_state[1]

    @property
    def run_rtl(self) -> bool:
        return self.factor_state[2]

    @property
    def ordered(self) -> bool:
        return self.factor_state[3]

    @property
    def level_one(self) -> bool:
        return self.factor_state[4]

    @property
    def explicit_tabs(self) -> bool:
        return self.factor_state[5]

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
        raise ValueError(f"invalid list/RTL factorial index: {index}")
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
        raise ValueError("list/RTL factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("list/RTL factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("list/RTL factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("list/RTL factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"list/RTL factorial factor is unbalanced: {factor}")
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
                    "list/RTL factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-list-rtl-{index:03d}",
            factor_state=_factor_state(index),
        )
        for index in range(CASE_COUNT)
    )
    _validate_specs(specs)
    return specs


def primary_expected_label(spec: CaseSpec) -> str:
    if spec.ordered:
        return "1.a)" if spec.level_one else "1."
    return "\u25e6" if spec.level_one else "\u2022"


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


def _level(
    ilvl: int,
    num_fmt: str,
    text: str | None,
    *,
    start: int = 1,
) -> str:
    level_text = "" if text is None else f'<w:lvlText w:val="{escape(text)}"/>'
    position = 720 * (ilvl + 1)
    return (
        f'<w:lvl w:ilvl="{ilvl}"><w:start w:val="{start}"/>'
        f'<w:numFmt w:val="{num_fmt}"/>{level_text}'
        '<w:lvlJc w:val="start"/><w:suff w:val="tab"/>'
        f'<w:pPr><w:tabs><w:tab w:val="num" w:pos="{position}"/></w:tabs>'
        f'<w:ind w:start="{position}" w:hanging="360"/></w:pPr></w:lvl>'
    )


def _numbering() -> bytes:
    ordered = _level(0, "decimal", "%1.") + _level(1, "lowerLetter", "%1.%2)")
    multilevel = ordered + _level(2, "lowerRoman", "%1.%2.%3")
    bullets = _level(0, "bullet", "\u2022") + _level(1, "bullet", "\u25e6")
    fallback_bullets = _level(0, "bullet", None) + _level(1, "bullet", None)
    return _b(
        XML_DECL
        + f'<w:numbering xmlns:w="{W}">'
        + f'<w:abstractNum w:abstractNumId="1">{ordered}</w:abstractNum>'
        + f'<w:abstractNum w:abstractNumId="4">{multilevel}</w:abstractNum>'
        + f'<w:abstractNum w:abstractNumId="5">{bullets}</w:abstractNum>'
        + '<w:abstractNum w:abstractNumId="6">'
        + fallback_bullets
        + "</w:abstractNum>"
        + '<w:num w:numId="11"><w:abstractNumId w:val="1"/></w:num>'
        + '<w:num w:numId="14"><w:abstractNumId w:val="4"/></w:num>'
        + '<w:num w:numId="15"><w:abstractNumId w:val="5"/></w:num>'
        + '<w:num w:numId="16"><w:abstractNumId w:val="6"/></w:num>'
        + '<w:num w:numId="17"><w:abstractNumId w:val="1"/>'
        + '<w:lvlOverride w:ilvl="0"><w:startOverride w:val="5"/>'
        + "</w:lvlOverride></w:num>"
        + '<w:num w:numId="18"><w:abstractNumId w:val="1"/>'
        + '<w:lvlOverride w:ilvl="0">'
        + _level(0, "lowerLetter", "(%1)")
        + "</w:lvlOverride></w:num></w:numbering>"
    )


def _section() -> str:
    return (
        '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/></w:sectPr>'
    )


def _run(text: str, *, rtl: bool = False) -> str:
    rpr = "<w:rPr><w:rtl/></w:rPr>" if rtl else ""
    return f'<w:r>{rpr}<w:t xml:space="preserve">{escape(text)}</w:t></w:r>'


def _list_ppr(
    num_id: int,
    ilvl: int,
    *,
    bidi: bool = False,
    tabs: bool = False,
) -> str:
    tab_stops = (
        '<w:tabs><w:tab w:val="start" w:pos="2880" w:leader="dot"/></w:tabs>'
        if tabs
        else ""
    )
    direction = "<w:bidi/>" if bidi else ""
    return (
        "<w:pPr>"
        f'<w:numPr><w:ilvl w:val="{ilvl}"/><w:numId w:val="{num_id}"/>'
        "</w:numPr>"
        f'{tab_stops}{direction}<w:jc w:val="start"/></w:pPr>'
    )


def _primary_paragraph(spec: CaseSpec) -> str:
    script = (
        "\u0627\u0644\u0639\u0631\u0628\u064a\u0629"
        if spec.arabic
        else "\u05e2\u05d1\u05e8\u05d9\u05ea"
    )
    prefix = f"PRIMARY {spec.case_id} {script}"
    rpr = "<w:rPr><w:rtl/></w:rPr>" if spec.run_rtl else ""
    if spec.explicit_tabs:
        runs = (
            f'<w:r>{rpr}<w:t xml:space="preserve">{escape(prefix)}</w:t>'
            '<w:tab/><w:t xml:space="preserve">Alpha 123 (scope)</w:t></w:r>'
        )
    else:
        runs = _run(f"{prefix} Alpha 123 (scope)", rtl=spec.run_rtl)
    return (
        "<w:p>"
        + _list_ppr(
            11 if spec.ordered else 15,
            1 if spec.level_one else 0,
            bidi=spec.paragraph_bidi,
            tabs=spec.explicit_tabs,
        )
        + runs
        + "</w:p>"
    )


def _list_paragraph(label: str, num_id: int, ilvl: int) -> str:
    return f"<w:p>{_list_ppr(num_id, ilvl)}{_run(label)}</w:p>"


def _document_xml(spec: CaseSpec) -> bytes:
    title = "<w:p>" + _run(f"{spec.case_id} deterministic list RTL control") + "</w:p>"
    probes = "".join(
        (
            _primary_paragraph(spec),
            _list_paragraph("START OVERRIDE expected 5.", 17, 0),
            _list_paragraph("LEVEL OVERRIDE expected (a)", 18, 0),
            _list_paragraph("MULTILEVEL root expected 1.", 14, 0),
            _list_paragraph("MULTILEVEL child expected 1.a)", 14, 1),
            _list_paragraph("MULTILEVEL leaf expected 1.a.i", 14, 2),
            _list_paragraph("BULLET FALLBACK expected level-one circle", 16, 1),
        )
    )
    return _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>'
        + title
        + probes
        + _section()
        + "</w:body></w:document>"
    )


def _docx(spec: CaseSpec) -> bytes:
    content_types = _content_types(
        overrides=[
            ("/word/document.xml", MAIN_CT),
            ("/word/numbering.xml", NUMBERING_CONTENT_TYPE),
            ("/word/styles.xml", STYLES_CONTENT_TYPE),
        ],
        defaults=[("rels", RELS_CT), ("xml", "application/xml")],
    )
    package_relationships = _rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    document_relationships = _rels(
        [
            ("rIdNumbering", f"{R}/numbering", "numbering.xml"),
            ("rIdStyles", f"{R}/styles", "styles.xml"),
        ]
    )
    parts = sorted(
        [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", package_relationships),
            ("word/_rels/document.xml.rels", document_relationships),
            ("word/document.xml", _document_xml(spec)),
            ("word/numbering.xml", _numbering()),
            ("word/styles.xml", _styles()),
        ],
        key=lambda item: item[0],
    )
    return _zip(parts)


def build_case(spec: CaseSpec) -> bytes:
    expected_id = f"full-list-rtl-{spec.index:03d}"
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
    label_counts: dict[str, int] = {}
    for spec in specs:
        label = primary_expected_label(spec)
        label_counts[label] = label_counts.get(label, 0) + 1
    return {
        "case_count": len(specs),
        "cohort": "list-rtl",
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
        "interaction_scope": "primary-paragraph",
        "pairwise_state_counts": _pairwise_state_counts(specs),
        "primary_label_case_counts": dict(sorted(label_counts.items())),
        "supplemental_probes": [
            "bullet-fallback",
            "multilevel-numbering",
            "numbering-level-override",
            "numbering-start-override",
        ],
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
                    f"scripts/generate_render_list_rtl_corpus.py#{spec.case_id}"
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
            materialize(Path(temporary) / "list-rtl", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_list_rtl_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus list/RTL batch."
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
        print(f"generate_render_list_rtl_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
