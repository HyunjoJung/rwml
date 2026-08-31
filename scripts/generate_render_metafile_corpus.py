#!/usr/bin/env python3
"""Build the valid single-raster metafile render corpus batch."""

from __future__ import annotations

import argparse
import binascii
from dataclasses import dataclass
import hashlib
from html import escape
import itertools
import json
import os
from pathlib import Path
import struct
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
DEFAULT_LOCK = ROOT / "corpus" / "public" / "oracle" / "render-full-metafile-v1.json"
DEFAULT_OUTPUT = ROOT / "target" / "render-oracle" / "render-full-metafile-v1"

LOCK_SCHEMA = "rwml.render-corpus-batch-lock.v1"
CAMPAIGN = "public-render-full-metafile-v1"
PROVENANCE_ID = "rwml-render-full-metafile"
PROVENANCE_PATH = "provenance/rwml-render-full-metafile.md"
CASE_COUNT = 64
MAX_CASE_BYTES = 64 * 1024
MAX_TOTAL_BYTES = 4 * 1024 * 1024
WIDTH = 160
HEIGHT = 80
SRCCOPY = 0x00CC_0020

STYLES_CONTENT_TYPE = (
    "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"
)

PROVENANCE_TEXT = """# Public full-render metafile batch provenance

The 64 `full-metafile-*` DOCX inputs are generated from repository-owned raw
OOXML and synthetic raster-bearing WMF/EMF bytes by
`scripts/generate_render_metafile_corpus.py`. They form the complete two-level
factorial over six factors in one primary image: EMF or WMF container, raw or
gzip-wrapped payload, source-blit or SETDIB record, indexed-palette or RGB565
bitfield DIB, direct-body or table-cell placement, and zero- or ninety-degree
rotation. Every factor level appears in 32 cases and every factor pair has all
four states 16 times.

All representation combinations decode to the same 160 by 80 four-quadrant
raster. Page geometry, visible text, styles, image extent, package order, and ZIP
metadata are deterministic. The generated documents and this provenance record
are licensed under the repository's MIT license. The checked-in batch lock binds
the generator closure and every generated input by byte length and SHA-256. This
batch is diagnostic corpus material; it does not establish general WMF/EMF
vector replay, floating-object layout, external-render fidelity, completion of
the planned full corpus, or a release-gate change.
"""

FACTOR_NAMES = (
    "wmf-container",
    "gzip-compressed",
    "setdib-record",
    "bitfields-dib",
    "table-cell-placement",
    "quarter-turn",
)
FACTOR_FEATURES = (
    ("emf-container", "wmf-container"),
    ("raw-metafile", "gzip-compressed"),
    ("source-blit-record", "setdib-record"),
    ("indexed-palette-dib", "bitfields-dib"),
    ("direct-body-placement", "table-cell-placement"),
    ("unrotated-image", "quarter-turn"),
)
BASE_FEATURES = (
    "deterministic-raster",
    "metafiles",
    "single-dib-raster",
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
    def wmf(self) -> bool:
        return self.factor_state[0]

    @property
    def compressed(self) -> bool:
        return self.factor_state[1]

    @property
    def setdib(self) -> bool:
        return self.factor_state[2]

    @property
    def bitfields(self) -> bool:
        return self.factor_state[3]

    @property
    def table_cell(self) -> bool:
        return self.factor_state[4]

    @property
    def quarter_turn(self) -> bool:
        return self.factor_state[5]

    @property
    def extension(self) -> str:
        if self.wmf:
            return "wmz" if self.compressed else "wmf"
        return "emz" if self.compressed else "emf"

    @property
    def media_name(self) -> str:
        return f"image.{self.extension}"

    @property
    def content_type(self) -> str:
        return f"image/x-{self.extension}"

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
        raise ValueError(f"invalid metafile factorial index: {index}")
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
        raise ValueError("metafile factorial case count is incomplete")
    if len({spec.case_id for spec in specs}) != CASE_COUNT:
        raise ValueError("metafile factorial case identities are not unique")
    states = {spec.factor_state for spec in specs}
    expected_states = set(itertools.product((False, True), repeat=6))
    if states != expected_states:
        raise ValueError("metafile factorial factor vectors are incomplete")
    if len({spec.features for spec in specs}) != CASE_COUNT:
        raise ValueError("metafile factorial feature vectors are not unique")
    for position, factor in enumerate(FACTOR_NAMES):
        if sum(spec.factor_state[position] for spec in specs) != CASE_COUNT // 2:
            raise ValueError(f"metafile factorial factor is unbalanced: {factor}")
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
                    "metafile factorial pair is incomplete: "
                    f"{FACTOR_NAMES[left]}/{FACTOR_NAMES[right]}"
                )


def case_specs() -> tuple[CaseSpec, ...]:
    specs = tuple(
        CaseSpec(
            index=index,
            case_id=f"full-metafile-{index:03d}",
            factor_state=_factor_state(index),
        )
        for index in range(CASE_COUNT)
    )
    _validate_specs(specs)
    return specs


def _put_u16(out: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<H", out, offset, value)


def _put_i16(out: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<h", out, offset, value)


def _put_u32(out: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<I", out, offset, value)


def _put_i32(out: bytearray, offset: int, value: int) -> None:
    struct.pack_into("<i", out, offset, value)


def _quadrant_index(x: int, y: int) -> int:
    return (2 if y >= HEIGHT // 2 else 0) + (1 if x >= WIDTH // 2 else 0)


def _dib_payload(bitfields: bool) -> bytes:
    header = bytearray(40)
    _put_u32(header, 0, 40)
    _put_i32(header, 4, WIDTH)
    _put_i32(header, 8, -HEIGHT)
    _put_u16(header, 12, 1)
    if not bitfields:
        _put_u16(header, 14, 8)
        bits = bytes(_quadrant_index(x, y) for y in range(HEIGHT) for x in range(WIDTH))
        _put_u32(header, 20, len(bits))
        _put_u32(header, 32, 4)
        palette = bytes(
            (
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0xFF,
                0x00,
                0x00,
                0x00,
                0x00,
                0xFF,
                0xFF,
                0x00,
            )
        )
        return bytes(header) + palette + bits

    _put_u16(header, 14, 16)
    _put_u32(header, 16, 3)
    pixels = (0xF800, 0x07E0, 0x001F, 0xFFE0)
    bits = b"".join(
        struct.pack("<H", pixels[_quadrant_index(x, y)])
        for y in range(HEIGHT)
        for x in range(WIDTH)
    )
    _put_u32(header, 20, len(bits))
    masks = struct.pack("<III", 0xF800, 0x07E0, 0x001F)
    return bytes(header) + masks + bits


def _dib_header_len(dib: bytes) -> int:
    bit_count = struct.unpack_from("<H", dib, 14)[0]
    return 56 if bit_count == 8 else 52


def _append_emf_eof(payload: bytearray, record_count: int) -> None:
    start = len(payload)
    payload.extend(bytes(20))
    _put_u32(payload, start, 14)
    _put_u32(payload, start + 4, 20)
    _put_u32(payload, start + 16, 20)
    _put_u32(payload, 48, len(payload))
    _put_u32(payload, 52, record_count)


def _emf(spec: CaseSpec, dib: bytes) -> bytes:
    payload = bytearray(88)
    _put_u32(payload, 0, 1)
    _put_u32(payload, 4, 88)
    _put_i32(payload, 16, WIDTH - 1)
    _put_i32(payload, 20, HEIGHT - 1)
    payload[40:44] = b" EMF"
    _put_u32(payload, 44, 0x0001_0000)

    fixed_size = 76 if spec.setdib else 100
    bmi_len = _dib_header_len(dib)
    bits_len = len(dib) - bmi_len
    start = len(payload)
    record_size = fixed_size + len(dib)
    payload.extend(bytes(record_size))
    _put_u32(payload, start, 80 if spec.setdib else 76)
    _put_u32(payload, start + 4, record_size)
    _put_i32(payload, start + 16, WIDTH - 1)
    _put_i32(payload, start + 20, HEIGHT - 1)
    if spec.setdib:
        _put_i32(payload, start + 40, WIDTH)
        _put_i32(payload, start + 44, HEIGHT)
        _put_u32(payload, start + 48, fixed_size)
        _put_u32(payload, start + 52, bmi_len)
        _put_u32(payload, start + 56, fixed_size + bmi_len)
        _put_u32(payload, start + 60, bits_len)
        _put_u32(payload, start + 72, HEIGHT)
    else:
        _put_i32(payload, start + 32, WIDTH)
        _put_i32(payload, start + 36, HEIGHT)
        _put_u32(payload, start + 40, SRCCOPY)
        _put_u32(payload, start + 52, struct.unpack("<I", struct.pack("<f", 1.0))[0])
        _put_u32(payload, start + 64, struct.unpack("<I", struct.pack("<f", 1.0))[0])
        _put_u32(payload, start + 84, fixed_size)
        _put_u32(payload, start + 88, bmi_len)
        _put_u32(payload, start + 92, fixed_size + bmi_len)
        _put_u32(payload, start + 96, bits_len)
    payload[start + fixed_size : start + record_size] = dib
    _append_emf_eof(payload, 3)
    return bytes(payload)


def _finalize_wmf(payload: bytearray, max_record_words: int) -> None:
    _put_u16(payload, 22, 1)
    _put_u16(payload, 24, 9)
    _put_u16(payload, 26, 0x0300)
    _put_u32(payload, 28, (len(payload) - 22) // 2)
    _put_u32(payload, 34, max_record_words)
    checksum = 0
    for offset in range(0, 20, 2):
        checksum ^= struct.unpack_from("<H", payload, offset)[0]
    _put_u16(payload, 20, checksum)


def _wmf(spec: CaseSpec, dib: bytes) -> bytes:
    payload = bytearray(40)
    _put_u32(payload, 0, 0x9AC6_CDD7)
    _put_i16(payload, 10, WIDTH)
    _put_i16(payload, 12, HEIGHT)
    _put_u16(payload, 14, 96)

    fixed_size = 24 if spec.setdib else 22
    start = len(payload)
    record_size = fixed_size + len(dib)
    payload.extend(bytes(record_size))
    _put_u32(payload, start, record_size // 2)
    _put_u16(payload, start + 4, 0x0D33 if spec.setdib else 0x0940)
    if spec.setdib:
        _put_u16(payload, start + 8, HEIGHT)
        _put_u16(payload, start + 16, HEIGHT)
        _put_u16(payload, start + 18, WIDTH)
    else:
        _put_u32(payload, start + 6, SRCCOPY)
        _put_i16(payload, start + 14, HEIGHT)
        _put_i16(payload, start + 16, WIDTH)
    payload[start + fixed_size : start + record_size] = dib
    payload.extend(struct.pack("<IH", 3, 0))
    _finalize_wmf(payload, record_size // 2)
    return bytes(payload)


def _deterministic_gzip(payload: bytes) -> bytes:
    if len(payload) > 0xFFFF:
        raise ValueError("metafile is too large for the deterministic gzip block")
    deflate = (
        b"\x01" + struct.pack("<HH", len(payload), len(payload) ^ 0xFFFF) + payload
    )
    trailer = struct.pack("<II", binascii.crc32(payload) & 0xFFFF_FFFF, len(payload))
    return b"\x1f\x8b\x08\x00\x00\x00\x00\x00\x00\xff" + deflate + trailer


def build_metafile(spec: CaseSpec) -> bytes:
    dib = _dib_payload(spec.bitfields)
    raw = _wmf(spec, dib) if spec.wmf else _emf(spec, dib)
    return _deterministic_gzip(raw) if spec.compressed else raw


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


def _drawing(spec: CaseSpec) -> str:
    rotation = ' rot="5400000"' if spec.quarter_turn else ""
    return (
        '<w:p><w:pPr><w:jc w:val="center"/></w:pPr><w:r><w:drawing>'
        '<wp:inline distT="0" distB="0" distL="0" distR="0">'
        '<wp:extent cx="1524000" cy="762000"/>'
        f'<wp:docPr id="1" name="Metafile control" descr="{escape(spec.case_id)}"/>'
        "<a:graphic><a:graphicData "
        'uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        '<pic:pic><pic:nvPicPr><pic:cNvPr id="0" name="Metafile raster"/>'
        '<pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip r:embed="rIdMeta"/>'
        "<a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr>"
        f'<a:xfrm{rotation}><a:off x="0" y="0"/><a:ext cx="1524000" cy="762000"/>'
        '</a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom>'
        "</pic:spPr></pic:pic></a:graphicData></a:graphic>"
        "</wp:inline></w:drawing></w:r></w:p>"
    )


def _document_xml(spec: CaseSpec) -> bytes:
    drawing = _drawing(spec)
    if spec.table_cell:
        placement = (
            '<w:tbl><w:tblPr><w:tblW w:w="7200" w:type="dxa"/>'
            '<w:tblLayout w:type="fixed"/><w:tblBorders>'
            '<w:top w:val="single" w:sz="8" w:color="000000"/>'
            '<w:left w:val="single" w:sz="8" w:color="000000"/>'
            '<w:bottom w:val="single" w:sz="8" w:color="000000"/>'
            '<w:right w:val="single" w:sz="8" w:color="000000"/>'
            '</w:tblBorders></w:tblPr><w:tblGrid><w:gridCol w:w="7200"/>'
            '</w:tblGrid><w:tr><w:tc><w:tcPr><w:tcW w:w="7200" w:type="dxa"/>'
            "</w:tcPr>" + drawing + "</w:tc></w:tr></w:tbl>"
        )
    else:
        placement = drawing
    return _b(
        XML_DECL + f'<w:document xmlns:w="{W}" xmlns:r="{R}" '
        'xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" '
        'xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
        'xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture">'
        "<w:body><w:p><w:r><w:t>Deterministic metafile raster control</w:t></w:r></w:p>"
        + placement
        + "<w:p><w:r><w:t>After metafile control</w:t></w:r></w:p>"
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
        defaults=[
            (spec.extension, spec.content_type),
            ("rels", RELS_CT),
            ("xml", "application/xml"),
        ],
    )
    package_relationships = _rels(
        [("rId1", f"{R}/officeDocument", "word/document.xml")]
    )
    document_relationships = _rels(
        [
            ("rIdMeta", f"{R}/image", f"media/{spec.media_name}"),
            ("rIdStyles", f"{R}/styles", "styles.xml"),
        ]
    )
    parts = sorted(
        [
            ("[Content_Types].xml", content_types),
            ("_rels/.rels", package_relationships),
            ("word/_rels/document.xml.rels", document_relationships),
            ("word/document.xml", _document_xml(spec)),
            (f"word/media/{spec.media_name}", build_metafile(spec)),
            ("word/styles.xml", _styles()),
        ],
        key=lambda item: item[0],
    )
    return _zip(parts)


def build_case(spec: CaseSpec) -> bytes:
    expected_id = f"full-metafile-{spec.index:03d}"
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
        "cohort": "metafile-raster-interactions",
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
            "four-quadrant-raster",
            "one-page-geometry",
            "single-image-relationship",
            "visible-text",
        ],
        "interaction_scope": "single-raster-image",
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
                    f"scripts/generate_render_metafile_corpus.py#{spec.case_id}"
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
            materialize(Path(temporary) / "metafile", lock)
        return True
    except (OSError, ValueError, json.JSONDecodeError) as error:
        print(f"generate_render_metafile_corpus: {error}", file=sys.stderr)
        return False


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Generate or verify the 64-case full-corpus metafile batch."
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
        print(f"generate_render_metafile_corpus: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
