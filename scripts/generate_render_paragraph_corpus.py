#!/usr/bin/env python3
"""Build the paragraph-geometry batch of the deterministic full render corpus."""

from __future__ import annotations

import argparse
from dataclasses import dataclass
import hashlib
from html import escape
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
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-paragraph-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-paragraph-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-paragraph-v1"
PROVENANCE_ID = "rwml-render-full-paragraph"
PROVENANCE_PATH = "provenance/rwml-render-full-paragraph.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)

PROVENANCE_TEXT = """# Public full-render paragraph-geometry batch provenance

The 64 `full-paragraph-*` DOCX inputs are generated from repository-owned raw
OOXML by `scripts/generate_render_paragraph_corpus.py`. They form a complete
64-row binary orthogonal lattice over fifteen modeled paragraph properties.
Every property appears in 32 cases and every pair has all four on/off states
16 times. Mutually exclusive values use separate labeled probe paragraphs;
the declared pairwise interaction scope is the complete document.

Package order, ZIP metadata, text, styles, page geometry, case identities, and
property values are deterministic. The generated documents and this provenance
record are licensed under the repository's MIT license. The checked-in batch
lock binds the generator closure and every generated input by byte length and
SHA-256. This batch is diagnostic corpus material; it does not claim completion
of the planned full corpus or change release validation.
"""

# The six basis masks make every document feature vector unique. Any two
# distinct nonzero masks have four equal state buckets over all 64 rows.
FEATURE_MASKS = (
    ("align-center", 0b000001),
    ("align-justify", 0b000010),
    ("align-right", 0b000100),
    ("explicit-tabs", 0b001000),
    ("first-line-indent", 0b010000),
    ("hanging-indent", 0b100000),
    ("left-indent", 0b000011),
    ("line-spacing-at-least", 0b000101),
    ("line-spacing-auto", 0b001001),
    ("line-spacing-exact", 0b010001),
    ("paragraph-borders", 0b100001),
    ("paragraph-shading", 0b000110),
    ("right-indent", 0b001010),
    ("space-after", 0b010010),
    ("space-before", 0b100010),
)
LATTICE_FEATURES = tuple(feature for feature, _ in FEATURE_MASKS)
BASE_FEATURES = ("paragraph-geometry",)

PARAGRAPH_PROPERTIES = {
    "align-center": '<w:jc w:val="center"/>',
    "align-justify": '<w:jc w:val="both"/>',
    "align-right": '<w:jc w:val="right"/>',
    "explicit-tabs": (
        '<w:tabs><w:tab w:val="left" w:pos="1440"/>'
        '<w:tab w:val="center" w:pos="2880"/>'
        '<w:tab w:val="right" w:pos="4320"/>'
        '<w:tab w:val="decimal" w:pos="5040"/></w:tabs>'
    ),
    "first-line-indent": '<w:ind w:firstLine="360"/>',
    "hanging-indent": '<w:ind w:left="720" w:hanging="360"/>',
    "left-indent": '<w:ind w:left="720"/>',
    "line-spacing-at-least": '<w:spacing w:line="330" w:lineRule="atLeast"/>',
    "line-spacing-auto": '<w:spacing w:line="360" w:lineRule="auto"/>',
    "line-spacing-exact": '<w:spacing w:line="360" w:lineRule="exact"/>',
    "paragraph-borders": (
        '<w:pBdr><w:top w:val="single" w:sz="8" w:space="2" w:color="1F4E78"/>'
        '<w:left w:val="single" w:sz="8" w:space="2" w:color="1F4E78"/>'
        '<w:bottom w:val="single" w:sz="8" w:space="2" w:color="1F4E78"/>'
        '<w:right w:val="single" w:sz="8" w:space="2" w:color="1F4E78"/>'
        "</w:pBdr>"
    ),
    "paragraph-shading": '<w:shd w:val="clear" w:color="auto" w:fill="DDEBF7"/>',
    "right-indent": '<w:ind w:right="720"/>',
    "space-after": '<w:spacing w:after="240"/>',
    "space-before": '<w:spacing w:before="240"/>',
}


@dataclass(frozen=True)
class CaseSpec:
    index: int
    case_id: str
    features: tuple[str, ...]

    @property
    def relative_path(self) -> str:
        return f"documents/{self.case_id}.docx"


def canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _enabled(index: int, mask: int) -> bool:
    return ((index & mask).bit_count() % 2) == 0


def _features(index: int) -> tuple[str, ...]:
    if index < 0 or index >= CASE_COUNT:
        raise ValueError(f"invalid paragraph lattice index: {index}")
    enabled = [feature for feature, mask in FEATURE_MASKS if _enabled(index, mask)]
    return tuple(sorted((*BASE_FEATURES, *enabled)))


def _validate_specs(specs: tuple[CaseSpec, ...]) -> None:
    if len(specs) != CASE_COUNT:
        raise ValueError("paragraph lattice case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("paragraph lattice case identities are not unique")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("paragraph lattice feature vectors are not unique")
    for feature in LATTICE_FEATURES:
        count = sum(feature in spec.features for spec in specs)
        if count != CASE_COUNT // 2:
            raise ValueError(f"paragraph lattice feature is unbalanced: {feature}")
    expected_states = {
        (False, False): CASE_COUNT // 4,
        (False, True): CASE_COUNT // 4,
        (True, False): CASE_COUNT // 4,
        (True, True): CASE_COUNT // 4,
    }
    for left_index, left in enumerate(LATTICE_FEATURES):
        for right in LATTICE_FEATURES[left_index + 1 :]:
            states = {
                state: sum(
                    (left in spec.features, right in spec.features) == state
                    for spec in specs
                )
                for state in expected_states
            }
            if states != expected_states:
                raise ValueError(
                    f"paragraph pairwise lattice is incomplete: {left}/{right}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-paragraph-{index:03d}",
            features=_features(index),
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


def _run(text: str) -> str:
    return f'<w:r><w:t xml:space="preserve">{escape(text)}</w:t></w:r>'


def _line_break_run(first: str, second: str) -> str:
    return (
        f'<w:r><w:t xml:space="preserve">{escape(first)}</w:t><w:br/>'
        f'<w:t xml:space="preserve">{escape(second)}</w:t></w:r>'
    )


def _probe_runs(feature: str) -> str:
    if feature == "explicit-tabs":
        return (
            "<w:r><w:t>Tabs</w:t><w:tab/><w:t>center</w:t><w:tab/>"
            "<w:t>right</w:t><w:tab/><w:t>12.34</w:t></w:r>"
        )
    if feature.startswith("line-spacing-"):
        return _line_break_run(
            f"{feature} first line.",
            "Second line exposes the deterministic vertical distance.",
        )
    if feature in {
        "align-center",
        "align-justify",
        "align-right",
        "first-line-indent",
        "hanging-indent",
        "left-indent",
        "right-indent",
    }:
        return _run(
            f"{feature} probe: alpha beta gamma delta epsilon zeta eta theta "
            "iota kappa lambda mu nu xi omicron pi rho sigma tau."
        )
    return _run(f"{feature} probe with stable paragraph geometry.")


def _probe_paragraph(spec: CaseSpec, feature: str) -> str:
    properties = PARAGRAPH_PROPERTIES[feature] if feature in spec.features else ""
    ppr = f"<w:pPr>{properties}</w:pPr>" if properties else ""
    return f"<w:p>{ppr}{_probe_runs(feature)}</w:p>"


def _document_xml(spec: CaseSpec) -> bytes:
    title = "<w:p>" + _run(f"{spec.case_id} deterministic paragraph control") + "</w:p>"
    probes = "".join(_probe_paragraph(spec, feature) for feature in LATTICE_FEATURES)
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
    expected_id = f"full-paragraph-{spec.index:03d}"
    if (
        spec.index < 0
        or spec.index >= CASE_COUNT
        or spec.case_id != expected_id
        or spec.features != _features(spec.index)
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
    for left_index, left in enumerate(LATTICE_FEATURES):
        for right in LATTICE_FEATURES[left_index + 1 :]:
            states = {"00": 0, "01": 0, "10": 0, "11": 0}
            for spec in specs:
                state = f"{int(left in spec.features)}{int(right in spec.features)}"
                states[state] += 1
            rows.append({"features": [left, right], "states": states})
    return rows


def _coverage(specs: tuple[CaseSpec, ...]) -> dict[str, object]:
    return {
        "case_count": len(specs),
        "cohort": "paragraph-geometry",
        "feature_case_counts": {
            feature: sum(feature in spec.features for spec in specs)
            for feature in LATTICE_FEATURES
        },
        "feature_masks": {feature: mask for feature, mask in FEATURE_MASKS},
        "interaction_scope": "document",
        "lattice_features": list(LATTICE_FEATURES),
        "lattice_rows": CASE_COUNT,
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
                    f"scripts/generate_render_paragraph_corpus.py#{spec.case_id}"
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
            materialize(Path(temporary) / "paragraph", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_paragraph_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus paragraph batch."
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
        print(f"generate_render_paragraph_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
