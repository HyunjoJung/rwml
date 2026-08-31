#!/usr/bin/env python3
"""Build the floating-shape geometry and wrap render corpus batch."""

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
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-floating-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-floating-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-floating-v1"
PROVENANCE_ID = "rwml-render-full-floating"
PROVENANCE_PATH = "provenance/rwml-render-full-floating.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)

WP = "http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing"
WPS = "http://schemas.microsoft.com/office/word/2010/wordprocessingShape"
A = "http://schemas.openxmlformats.org/drawingml/2006/main"

PROVENANCE_TEXT = """# Public full-render floating-shape batch provenance

The 64 `full-floating-*` DOCX inputs are generated from repository-owned raw
OOXML by `scripts/generate_render_floating_corpus.py`. They form the complete
two-level factorial over six factors in one primary text-bearing floating
shape: page- or margin-relative horizontal placement, near or far horizontal
offset, page- or margin-relative vertical placement, high or low vertical
offset, no wrap or top-and-bottom wrap, and behind- or in-front-of-text layer.
Every factor level appears in 32 cases and every factor pair has all four states
16 times.

The page, margins, font defaults, shape extent, anchor distance, visible text,
package order, and ZIP metadata are deterministic. The generated documents and
this provenance record are licensed under the repository's MIT license. The
checked-in batch lock binds the generator closure and every generated input by
byte length and SHA-256. This batch records bounded anchor geometry, placeholder
placement, layer ordering, and top-and-bottom flow behavior. It does not
establish arbitrary floating-object reflow, non-rectangular exclusion zones,
Word-exact pagination, external-render fidelity, completion of the planned full
corpus, or a release-gate change.
"""

FACTOR_NAMES = (
    "horizontal-margin-reference",
    "far-horizontal-offset",
    "vertical-margin-reference",
    "low-vertical-offset",
    "top-and-bottom-wrap",
    "front-layer",
)
FACTOR_FEATURES = (
    ("horizontal-page-reference", "horizontal-margin-reference"),
    ("near-horizontal-offset", "far-horizontal-offset"),
    ("vertical-page-reference", "vertical-margin-reference"),
    ("high-vertical-offset", "low-vertical-offset"),
    ("no-wrap", "top-and-bottom-wrap"),
    ("behind-text-layer", "front-text-layer"),
)
BASE_FEATURES = (
    "deterministic-anchor-geometry",
    "floating-shape",
    "text-bearing-shape",
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
    def horizontal_margin(self) -> bool:
        return self.factor_state[0]

    @property
    def far_horizontal(self) -> bool:
        return self.factor_state[1]

    @property
    def vertical_margin(self) -> bool:
        return self.factor_state[2]

    @property
    def low_vertical(self) -> bool:
        return self.factor_state[3]

    @property
    def top_bottom(self) -> bool:
        return self.factor_state[4]

    @property
    def front(self) -> bool:
        return self.factor_state[5]

    @property
    def horizontal_reference(self) -> str:
        return "margin" if self.horizontal_margin else "page"

    @property
    def horizontal_offset_emu(self) -> int:
        return 3_200_400 if self.far_horizontal else 457_200

    @property
    def vertical_reference(self) -> str:
        return "margin" if self.vertical_margin else "page"

    @property
    def vertical_offset_emu(self) -> int:
        return 2_514_600 if self.low_vertical else 457_200

    @property
    def behind_doc(self) -> bool:
        return not self.front

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
        raise ValueError(f"invalid floating factorial index: {index}")
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
        raise ValueError("floating factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("floating factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("floating factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("floating factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"floating factorial factor is unbalanced: {factor}")
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
                    "floating factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-floating-{index:03d}",
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


def _anchor(spec: CaseSpec) -> str:
    wrap = "<wp:wrapTopAndBottom/>" if spec.top_bottom else "<wp:wrapNone/>"
    behind_doc = "1" if spec.behind_doc else "0"
    return (
        '<w:r><w:drawing><wp:anchor simplePos="0" relativeHeight="500" '
        f'behindDoc="{behind_doc}" locked="0" layoutInCell="1" allowOverlap="1" '
        'distT="114300" distB="114300" distL="0" distR="0">'
        '<wp:simplePos x="0" y="0"/>'
        f'<wp:positionH relativeFrom="{spec.horizontal_reference}">'
        f"<wp:posOffset>{spec.horizontal_offset_emu}</wp:posOffset>"
        "</wp:positionH>"
        f'<wp:positionV relativeFrom="{spec.vertical_reference}">'
        f"<wp:posOffset>{spec.vertical_offset_emu}</wp:posOffset>"
        "</wp:positionV>"
        '<wp:extent cx="2286000" cy="1371600"/>'
        '<wp:effectExtent l="0" t="0" r="0" b="0"/>'
        + wrap
        + f'<wp:docPr id="500" name="Primary floating control" '
        f'descr="{escape(spec.case_id)}"/>'
        '<wps:wsp><wps:cNvSpPr txBox="1"/><wps:spPr>'
        '<a:prstGeom prst="rect"><a:avLst/></a:prstGeom></wps:spPr>'
        "<wps:txbx><w:txbxContent><w:p><w:r>"
        "<w:t>Primary floating text</w:t></w:r></w:p></w:txbxContent>"
        "</wps:txbx><wps:bodyPr/></wps:wsp>"
        "</wp:anchor></w:drawing></w:r>"
    )


def _document_xml(spec: CaseSpec) -> bytes:
    flow = " ".join(f"flow token {index}" for index in range(1, 181))
    return _b(
        XML_DECL + f'<w:document xmlns:w="{W}" xmlns:wp="{WP}" '
        f'xmlns:wps="{WPS}" xmlns:a="{A}">'
        "<w:body><w:p><w:r><w:t>Floating geometry control</w:t></w:r></w:p>"
        '<w:p><w:r><w:t xml:space="preserve">Anchor lead </w:t></w:r>'
        + _anchor(spec)
        + '</w:p><w:p><w:r><w:t xml:space="preserve">'
        + flow
        + "</w:t></w:r></w:p>"
        "<w:p><w:r><w:t>After floating geometry control</w:t></w:r></w:p>"
        '<w:sectPr><w:pgSz w:w="12240" w:h="15840"/>'
        '<w:pgMar w:top="720" w:right="720" w:bottom="720" w:left="720" '
        'w:header="360" w:footer="360" w:gutter="0"/></w:sectPr>'
        "</w:body></w:document>"
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
    expected_id = f"full-floating-{spec.index:03d}"
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
        "cohort": "floating-geometry-interactions",
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
            "anchor-distance",
            "one-page-geometry",
            "shape-extent",
            "visible-text",
        ],
        "interaction_scope": "primary-floating-shape",
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
                "expected": {
                    "pages": 1,
                    "warnings": ["FloatingShapePlaceholderOnly"],
                },
                "features": list(spec.features),
                "format": "docx",
                "id": spec.case_id,
                "path": spec.relative_path,
                "provenance": PROVENANCE_ID,
                "sha256": payload_sha256,
                "source": "generated",
                "source_path": (
                    f"scripts/generate_render_floating_corpus.py#{spec.case_id}"
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
            materialize(Path(temporary) / "floating", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_floating_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus floating batch."
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
        print(f"generate_render_floating_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
