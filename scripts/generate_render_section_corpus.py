#!/usr/bin/env python3
"""Build the section, column, and running-surface render corpus batch."""

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
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-section-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-section-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-section-v1"
PROVENANCE_ID = "rwml-render-full-section"
PROVENANCE_PATH = "provenance/rwml-render-full-section.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 96 * 1024
MAX_TOTAL_BYTES = 8 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
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

PROVENANCE_TEXT = """# Public full-render section and running-surface batch provenance

The 64 `full-section-*` DOCX inputs are generated from repository-owned raw
OOXML by `scripts/generate_render_section_corpus.py`. They form the complete
two-level factorial over six factors in one final section: next-page or odd-page
section start, portrait or landscape geometry, equal or unequal two-column
layout, left-to-right or right-to-left column progression, absent or present
column separator, and quarter-inch or half-inch running-surface distance. Every
factor level appears in 32 cases and every factor pair has all four states 16
times.

Every document has one ending section and a three-page final section. The final
section names distinct first, even, and default header and footer stories, uses
title-page and even/odd selection, and places an explicit column break on each
of its three pages. Body margins, fonts, story content, column count, package
order, ZIP metadata, and explicit page count drivers are held constant. The
generated documents and this provenance record are licensed under the
repository's MIT license. The checked-in batch lock binds the generator closure
and every generated input by byte length and SHA-256. This batch is diagnostic
corpus material; it does not establish Word-exact pagination, external-render
fidelity, completion of the planned full corpus, or a release-gate change.
"""

FACTOR_NAMES = (
    "odd-page-section-start",
    "landscape-final-section",
    "unequal-section-columns",
    "rtl-section-columns",
    "section-column-separator",
    "running-surface-distance",
)
FACTOR_FEATURES = (
    ("next-page-section-start", "odd-page-section-start"),
    ("portrait-final-section", "landscape-final-section"),
    ("equal-section-columns", "unequal-section-columns"),
    ("ltr-section-columns", "rtl-section-columns"),
    ("columns-without-separator", "section-column-separator"),
    ("quarter-inch-running-distance", "half-inch-running-distance"),
)
BASE_FEATURES = (
    "explicit-column-breaks",
    "first-even-default-footers",
    "first-even-default-headers",
    "multi-section",
    "three-page-final-section",
    "title-page-running-surfaces",
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
    def odd_page_start(self) -> bool:
        return self.factor_state[0]

    @property
    def landscape(self) -> bool:
        return self.factor_state[1]

    @property
    def unequal_columns(self) -> bool:
        return self.factor_state[2]

    @property
    def rtl_columns(self) -> bool:
        return self.factor_state[3]

    @property
    def column_separator(self) -> bool:
        return self.factor_state[4]

    @property
    def inset_running_surfaces(self) -> bool:
        return self.factor_state[5]

    @property
    def expected_pages(self) -> int:
        return 5 if self.odd_page_start else 4

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
        raise ValueError(f"invalid section factorial index: {index}")
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
        raise ValueError("section factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("section factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("section factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("section factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"section factorial factor is unbalanced: {factor}")
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
                    "section factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-section-{index:03d}",
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


def _settings() -> bytes:
    return _b(
        XML_DECL + f'<w:settings xmlns:w="{W}"><w:evenAndOddHeaders/></w:settings>'
    )


def _paragraph(text: str) -> str:
    return f'<w:p><w:r><w:t xml:space="preserve">{escape(text)}</w:t></w:r></w:p>'


def _break_paragraph(kind: str) -> str:
    return f'<w:p><w:r><w:br w:type="{kind}"/></w:r></w:p>'


def _running_part(root: str, label: str) -> bytes:
    return _b(
        XML_DECL + f'<w:{root} xmlns:w="{W}"><w:p><w:pPr><w:jc w:val="center"/>'
        '<w:spacing w:before="0" w:after="0"/></w:pPr><w:r><w:rPr><w:b/>'
        '<w:sz w:val="18"/><w:szCs w:val="18"/></w:rPr>'
        f"<w:t>{escape(label)}</w:t></w:r></w:p></w:{root}>"
    )


def _ending_references() -> str:
    return (
        '<w:headerReference w:type="default" r:id="rIdEndingHeader"/>'
        '<w:footerReference w:type="default" r:id="rIdEndingFooter"/>'
    )


def _final_references() -> str:
    return (
        '<w:headerReference w:type="default" r:id="rIdFinalDefaultHeader"/>'
        '<w:headerReference w:type="first" r:id="rIdFinalFirstHeader"/>'
        '<w:headerReference w:type="even" r:id="rIdFinalEvenHeader"/>'
        '<w:footerReference w:type="default" r:id="rIdFinalDefaultFooter"/>'
        '<w:footerReference w:type="first" r:id="rIdFinalFirstFooter"/>'
        '<w:footerReference w:type="even" r:id="rIdFinalEvenFooter"/>'
    )


def _page_size(spec: CaseSpec) -> str:
    if spec.landscape:
        return '<w:pgSz w:w="15840" w:h="12240" w:orient="landscape"/>'
    return '<w:pgSz w:w="12240" w:h="15840"/>'


def _page_margins(distance_twips: int) -> str:
    return (
        '<w:pgMar w:top="1440" w:right="1080" w:bottom="1440" w:left="1080" '
        f'w:header="{distance_twips}" w:footer="{distance_twips}" w:gutter="0"/>'
    )


def _columns(spec: CaseSpec) -> str:
    separator = ' w:sep="1"' if spec.column_separator else ""
    if not spec.unequal_columns:
        return f'<w:cols w:num="2" w:equalWidth="1" w:space="720"{separator}/>'
    widths = (4200, 8760) if spec.landscape else (3000, 6360)
    return (
        f'<w:cols w:num="2" w:equalWidth="0"{separator}>'
        f'<w:col w:w="{widths[0]}" w:space="720"/>'
        f'<w:col w:w="{widths[1]}"/></w:cols>'
    )


def _ending_section(spec: CaseSpec) -> str:
    section_type = "oddPage" if spec.odd_page_start else "nextPage"
    return (
        "<w:sectPr>" + _ending_references() + f'<w:type w:val="{section_type}"/>'
        '<w:pgSz w:w="12240" w:h="15840"/>' + _page_margins(360) + "</w:sectPr>"
    )


def _final_section(spec: CaseSpec) -> str:
    distance = 720 if spec.inset_running_surfaces else 360
    bidi = "<w:bidi/>" if spec.rtl_columns else ""
    return (
        "<w:sectPr>"
        + _final_references()
        + _page_size(spec)
        + _page_margins(distance)
        + _columns(spec)
        + "<w:titlePg/>"
        + bidi
        + "</w:sectPr>"
    )


def _ending_boundary(spec: CaseSpec) -> str:
    return (
        "<w:p><w:pPr>"
        + _ending_section(spec)
        + "</w:pPr><w:r><w:t>SECTION ONE BOUNDARY</w:t></w:r></w:p>"
    )


def _final_page_probe(spec: CaseSpec, page_index: int) -> str:
    label = ("FIRST", "SECOND", "THIRD")[page_index]
    first = (
        f"{spec.case_id} {label} logical column one alpha beta gamma delta "
        "epsilon zeta eta theta"
    )
    second = (
        f"{spec.case_id} {label} logical column two iota kappa lambda mu nu xi "
        "omicron pi"
    )
    body = _paragraph(first) + _break_paragraph("column") + _paragraph(second)
    if page_index < 2:
        body += _break_paragraph("page")
    return body


def _document_xml(spec: CaseSpec) -> bytes:
    body = _paragraph(f"{spec.case_id} ending-section control")
    body += _ending_boundary(spec)
    body += "".join(_final_page_probe(spec, index) for index in range(3))
    return _b(
        XML_DECL
        + f'<w:document xmlns:w="{W}" xmlns:r="{R}"><w:body>'
        + body
        + _final_section(spec)
        + "</w:body></w:document>"
    )


def _document_relationships() -> bytes:
    relationships = [
        ("rIdEndingHeader", f"{R}/header", "header1.xml"),
        ("rIdEndingFooter", f"{R}/footer", "footer1.xml"),
        ("rIdFinalDefaultHeader", f"{R}/header", "header2.xml"),
        ("rIdFinalFirstHeader", f"{R}/header", "header3.xml"),
        ("rIdFinalEvenHeader", f"{R}/header", "header4.xml"),
        ("rIdFinalDefaultFooter", f"{R}/footer", "footer2.xml"),
        ("rIdFinalFirstFooter", f"{R}/footer", "footer3.xml"),
        ("rIdFinalEvenFooter", f"{R}/footer", "footer4.xml"),
        ("rIdSettings", f"{R}/settings", "settings.xml"),
        ("rIdStyles", f"{R}/styles", "styles.xml"),
    ]
    return _rels(relationships)


def _docx(spec: CaseSpec) -> bytes:
    content_types = _content_types(
        overrides=[
            ("/word/document.xml", MAIN_CT),
            ("/word/styles.xml", STYLES_CONTENT_TYPE),
            ("/word/settings.xml", SETTINGS_CONTENT_TYPE),
            *[
                (f"/word/header{index}.xml", HEADER_CONTENT_TYPE)
                for index in range(1, 5)
            ],
            *[
                (f"/word/footer{index}.xml", FOOTER_CONTENT_TYPE)
                for index in range(1, 5)
            ],
        ],
        defaults=[("rels", RELS_CT), ("xml", "application/xml")],
    )
    package_relationships = _rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    parts = sorted(
        [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", package_relationships),
            ("word/_rels/document.xml.rels", _document_relationships()),
            ("word/document.xml", _document_xml(spec)),
            ("word/footer1.xml", _running_part("ftr", "ENDING DEFAULT FOOTER")),
            ("word/footer2.xml", _running_part("ftr", "FINAL DEFAULT FOOTER")),
            ("word/footer3.xml", _running_part("ftr", "FINAL FIRST FOOTER")),
            ("word/footer4.xml", _running_part("ftr", "FINAL EVEN FOOTER")),
            ("word/header1.xml", _running_part("hdr", "ENDING DEFAULT HEADER")),
            ("word/header2.xml", _running_part("hdr", "FINAL DEFAULT HEADER")),
            ("word/header3.xml", _running_part("hdr", "FINAL FIRST HEADER")),
            ("word/header4.xml", _running_part("hdr", "FINAL EVEN HEADER")),
            ("word/settings.xml", _settings()),
            ("word/styles.xml", _styles()),
        ],
        key=lambda item: item[0],
    )
    return _zip(parts)


def build_case(spec: CaseSpec) -> bytes:
    expected_id = f"full-section-{spec.index:03d}"
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
        "cohort": "section-columns-running-surfaces",
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
            "explicit-page-count-drivers",
            "one-inch-top-and-bottom-body-margins",
            "six-final-running-surface-stories",
            "three-final-section-pages",
            "two-final-section-columns",
        ],
        "interaction_scope": "final-section-pages",
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
                "expected": {"pages": spec.expected_pages, "warnings": []},
                "features": list(spec.features),
                "format": "docx",
                "id": spec.case_id,
                "path": spec.relative_path,
                "provenance": PROVENANCE_ID,
                "sha256": payload_sha256,
                "source": "generated",
                "source_path": (
                    f"scripts/generate_render_section_corpus.py#{spec.case_id}"
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
            "max_pages_per_document": 5,
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
            materialize(Path(temporary) / "section", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_section_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus section batch."
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
        print(f"generate_render_section_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
