#!/usr/bin/env python3
"""Extract and compare content-safe unequal-table PDF topology evidence.

Only the campaign's synthetic ``TnnLnn`` / ``TnnRnn`` tokens and normalized
axis-aligned border geometry are retained. Arbitrary document text and local paths do
not enter the report. This tool establishes diagnostics, not fidelity thresholds.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
import math
import os
import platform
import re
import stat
import sys
import tempfile
from pathlib import Path
from typing import Any, Iterable

try:
    import pymupdf
except ImportError:
    try:
        import fitz as pymupdf  # type: ignore[no-redef]
    except ImportError:
        pymupdf = None

try:
    from generate_unequal_table_oracle import CAMPAIGN, CASES
    from render_oracle_contract import CorpusManifest, load_corpus_manifest
except ModuleNotFoundError:  # Imported as ``scripts.*`` by unit tests.
    from scripts.generate_unequal_table_oracle import CAMPAIGN, CASES
    from scripts.render_oracle_contract import CorpusManifest, load_corpus_manifest


ROOT = Path(__file__).resolve().parents[1]
SCRIPT_PATH = Path(__file__).resolve()
CAPTURE_SCHEMA = "rwml.table-topology-capture.v1"
COMPARISON_SCHEMA = "rwml.table-topology-comparison.v1"
EXTRACTOR_NAME = "rwml-table-topology"
EXTRACTOR_VERSION = "1"

MAX_REPORT_BYTES = 64 * 1024 * 1024
MAX_PDF_BYTES = 64 * 1024 * 1024
MAX_PAGES = 8
MAX_PAGE_POINTS = 2_000
MAX_DRAWINGS_PER_PAGE = 20_000
MAX_WORDS_PER_PAGE = 50_000
MAX_BORDERS_PER_PAGE = 20_000
MAX_JSON_DEPTH = 64
MAX_JSON_NODES = 2_000_000
MAX_JSON_INTEGER_DIGITS = 128
MAX_TOKEN_PAIR_Y_DELTA_MILLIPOINTS = 2_000
MAX_BORDER_THICKNESS_POINTS = 4.0
MIN_BORDER_LENGTH_POINTS = 4.0
AXIS_TOLERANCE_POINTS = 0.25
TOKEN_BORDER_TOLERANCE_MILLIPOINTS = 2_000

TOKEN_RE = re.compile(r"T(?P<case>\d{2})(?P<side>[LR])(?P<unit>\d{2})\Z")
SHA256_RE = re.compile(r"[0-9a-f]{64}\Z")
REVISION_RE = re.compile(r"[0-9a-f]{40}\Z")
CANONICAL_ID_RE = re.compile(r"[a-z][a-z0-9]*(?:-[a-z0-9]+)*\Z")
LOCAL_PATH_PATTERNS = (
    re.compile(r"(?<![A-Za-z]:)/Users/[A-Za-z0-9._-]+/"),
    re.compile(r"/home/[A-Za-z0-9._-]+/"),
    re.compile(r"[A-Za-z]:[/\\]Users[/\\][^/\\\s]+[/\\]"),
    re.compile(r"(?<!\\)\\\\[A-Za-z0-9._-]{2,}\\[^\\\s]{2,}\\"),
)

TOP_LEVEL_KEYS = {
    "schema",
    "campaign",
    "environment",
    "producer",
    "extractor",
    "limits",
    "documents",
}
COMPARISON_KEYS = {
    "schema",
    "campaign",
    "harness_sha256",
    "candidate_capture_sha256",
    "reference_capture_sha256",
    "candidate",
    "reference",
    "summary",
    "documents",
}
ENVIRONMENT_KEYS = {
    "source_revision",
    "source_dirty",
    "harness_sha256",
    "platform",
    "tools",
}
PRODUCER_KEYS = {"name", "mode", "version", "identity_sha256", "platform"}
PLATFORM_KEYS = {"system", "release", "machine"}
TOOL_KEYS = {"name", "version"}
EXTRACTOR_KEYS = {"name", "version", "identity_sha256"}
LIMIT_KEYS = {
    "max_pdf_bytes",
    "max_pages",
    "max_page_points",
    "max_drawings_per_page",
    "max_words_per_page",
    "max_borders_per_page",
}
DOCUMENT_KEYS = {
    "case_id",
    "input_bytes",
    "input_sha256",
    "pdf",
    "pages",
    "topology",
}
PDF_KEYS = {"bytes", "sha256", "pages"}
PAGE_KEYS = {
    "number",
    "width_millipoints",
    "height_millipoints",
    "tokens",
    "horizontal_borders",
    "vertical_borders",
}
TOKEN_KEYS = {"id", "paint_order", "bbox_millipoints"}
BORDER_KEYS = {
    "axis_millipoints",
    "start_millipoints",
    "end_millipoints",
    "thickness_millipoints",
}
BORDER_ORDER = (
    "axis_millipoints",
    "start_millipoints",
    "end_millipoints",
    "thickness_millipoints",
)
TOPOLOGY_KEYS = {
    "expected_tokens",
    "observed_tokens",
    "paired_units",
    "pair_page_matches",
    "pair_y_aligned",
    "paint_sequence_exact",
    "segments",
}
SEGMENT_KEYS = {
    "first_unit",
    "last_unit",
    "page",
    "left_millipoints",
    "divider_millipoints",
    "right_millipoints",
    "top_millipoints",
    "bottom_millipoints",
}


def case_by_id(case_id: str):
    for case in CASES:
        if case.case_id == case_id:
            return case
    raise ValueError(f"unknown unequal-table case: {case_id}")


def expected_token_ids(case) -> tuple[str, ...]:
    units = 20 if case.fragment == "row-boundary" else 26
    if case.fragment == "row-boundary":
        return tuple(
            f"T{case.index:02d}{side}{unit:02d}"
            for unit in range(1, units + 1)
            for side in ("L", "R")
        )
    return tuple(
        f"T{case.index:02d}{side}{unit:02d}"
        for side in ("L", "R")
        for unit in range(1, units + 1)
    )


def _require_exact_keys(value: dict[str, Any], expected: set[str], label: str) -> None:
    actual = set(value)
    if actual != expected:
        raise ValueError(
            f"{label} keys are invalid: missing={sorted(expected - actual)}, "
            f"unknown={sorted(actual - expected)}"
        )


def _require_int(
    value: object,
    label: str,
    *,
    minimum: int = 0,
    maximum: int | None = None,
) -> int:
    if not isinstance(value, int) or isinstance(value, bool) or value < minimum:
        raise ValueError(f"{label} is invalid")
    if maximum is not None and value > maximum:
        raise ValueError(f"{label} exceeds its limit")
    return value


def _safe_text(value: object, label: str, maximum: int = 256) -> str:
    if (
        not isinstance(value, str)
        or not value
        or value != value.strip()
        or len(value) > maximum
        or "\x00" in value
        or "\n" in value
        or "\r" in value
    ):
        raise ValueError(f"{label} is invalid")
    return value


def _sha256(value: object, label: str) -> str:
    if not isinstance(value, str) or SHA256_RE.fullmatch(value) is None:
        raise ValueError(f"{label} is invalid")
    return value


def _canonical_json(value: object) -> bytes:
    return (
        json.dumps(value, ensure_ascii=True, indent=2, sort_keys=True) + "\n"
    ).encode("utf-8")


def _reject_duplicate_pairs(pairs: list[tuple[str, object]]) -> dict[str, object]:
    result: dict[str, object] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError(f"duplicate JSON key: {key}")
        result[key] = value
    return result


def _reject_constant(value: str) -> object:
    raise ValueError(f"non-finite JSON number: {value}")


def _parse_int(value: str) -> int:
    digits = value[1:] if value.startswith("-") else value
    if len(digits) > MAX_JSON_INTEGER_DIGITS:
        raise ValueError("JSON integer digit limit exceeded")
    return int(value)


def _validate_json_complexity(value: object) -> None:
    nodes = 0
    stack: list[tuple[object, int]] = [(value, 1)]
    while stack:
        current, depth = stack.pop()
        nodes += 1
        if nodes > MAX_JSON_NODES:
            raise ValueError("JSON node limit exceeded")
        if depth > MAX_JSON_DEPTH:
            raise ValueError("JSON depth limit exceeded")
        if isinstance(current, dict):
            stack.extend((item, depth + 1) for item in current.values())
        elif isinstance(current, list):
            stack.extend((item, depth + 1) for item in current)


def _read_bounded_regular_file(path: Path, maximum: int) -> bytes:
    if path.is_symlink():
        raise ValueError(f"{path.name} must not be a symlink")
    descriptor: int | None = None
    try:
        descriptor = os.open(
            path,
            os.O_RDONLY
            | getattr(os, "O_CLOEXEC", 0)
            | getattr(os, "O_NOFOLLOW", 0)
            | getattr(os, "O_NONBLOCK", 0),
        )
        before = os.fstat(descriptor)
        if not stat.S_ISREG(before.st_mode):
            raise ValueError(f"{path.name} is not a regular file")
        if before.st_size <= 0 or before.st_size > maximum:
            raise ValueError(f"{path.name} size is outside the contract")
        chunks = []
        remaining = before.st_size
        while remaining:
            chunk = os.read(descriptor, min(1024 * 1024, remaining))
            if not chunk:
                break
            chunks.append(chunk)
            remaining -= len(chunk)
        after = os.fstat(descriptor)
    except ValueError:
        raise
    except OSError as error:
        raise ValueError(f"{path.name} is unreadable") from error
    finally:
        if descriptor is not None:
            os.close(descriptor)
    payload = b"".join(chunks)
    if (
        len(payload) != before.st_size
        or before.st_dev != after.st_dev
        or before.st_ino != after.st_ino
        or before.st_size != after.st_size
        or before.st_mtime_ns != after.st_mtime_ns
    ):
        raise ValueError(f"{path.name} changed while reading")
    return payload


def _load_json(path: Path, maximum: int) -> tuple[dict[str, Any], bytes]:
    payload = _read_bounded_regular_file(path, maximum)
    try:
        value = json.loads(
            payload.decode("utf-8"),
            object_pairs_hook=_reject_duplicate_pairs,
            parse_constant=_reject_constant,
            parse_int=_parse_int,
        )
    except (UnicodeDecodeError, json.JSONDecodeError) as error:
        raise ValueError(f"{path.name} is malformed JSON") from error
    if not isinstance(value, dict):
        raise ValueError(f"{path.name} must contain an object")
    _validate_json_complexity(value)
    return value, payload


def _assert_path_neutral(value: object) -> None:
    stack = [value]
    while stack:
        current = stack.pop()
        if isinstance(current, dict):
            stack.extend(current.values())
        elif isinstance(current, list):
            stack.extend(current)
        elif isinstance(current, str) and any(
            pattern.search(current) for pattern in LOCAL_PATH_PATTERNS
        ):
            raise ValueError("topology evidence is not path-neutral")


def _millipoints(value: float) -> int:
    if not isinstance(value, (int, float)) or not math.isfinite(value):
        raise ValueError("non-finite PDF coordinate")
    if value < 0 or value > MAX_PAGE_POINTS + MAX_BORDER_THICKNESS_POINTS:
        raise ValueError("PDF coordinate exceeds its limit")
    return int(math.floor(float(value) * 1000.0 + 0.5))


def _dark_color(value: object) -> bool:
    if (
        not isinstance(value, (tuple, list))
        or len(value) not in {1, 3, 4}
        or not all(
            isinstance(item, (int, float)) and math.isfinite(item) for item in value
        )
    ):
        return False
    channels = [min(1.0, max(0.0, float(item))) for item in value]
    if len(channels) == 1:
        return channels[0] <= 0.25
    if len(channels) == 3:
        return max(channels) <= 0.25
    cyan, magenta, yellow, black = channels
    rgb = (
        (1.0 - cyan) * (1.0 - black),
        (1.0 - magenta) * (1.0 - black),
        (1.0 - yellow) * (1.0 - black),
    )
    return max(rgb) <= 0.25


def _border(axis: float, start: float, end: float, thickness: float) -> dict[str, int]:
    start_value, end_value = sorted((_millipoints(start), _millipoints(end)))
    if end_value - start_value < int(MIN_BORDER_LENGTH_POINTS * 1000):
        raise ValueError("table border is too short")
    thickness_value = max(1, _millipoints(max(0.001, thickness)))
    return {
        "axis_millipoints": _millipoints(axis),
        "start_millipoints": start_value,
        "end_millipoints": end_value,
        "thickness_millipoints": thickness_value,
    }


def _extract_axis_borders(
    drawings: list[dict[str, Any]],
) -> tuple[list[dict[str, int]], list[dict[str, int]]]:
    horizontal: set[tuple[int, int, int, int]] = set()
    vertical: set[tuple[int, int, int, int]] = set()

    def retain(target: set[tuple[int, int, int, int]], value: dict[str, int]) -> None:
        target.add(
            (
                value["axis_millipoints"],
                value["start_millipoints"],
                value["end_millipoints"],
                value["thickness_millipoints"],
            )
        )

    for drawing in drawings:
        stroke_width = drawing.get("width")
        stroke_ok = _dark_color(drawing.get("color")) and isinstance(
            stroke_width, (int, float)
        )
        fill_ok = _dark_color(drawing.get("fill"))
        for item in drawing.get("items", ()):
            if not isinstance(item, tuple) or not item:
                continue
            if item[0] == "l" and len(item) >= 3 and stroke_ok:
                first, second = item[1], item[2]
                width = float(stroke_width)
                if not 0 <= width <= MAX_BORDER_THICKNESS_POINTS:
                    continue
                dx = abs(float(second.x) - float(first.x))
                dy = abs(float(second.y) - float(first.y))
                if dy <= AXIS_TOLERANCE_POINTS and dx >= MIN_BORDER_LENGTH_POINTS:
                    retain(
                        horizontal,
                        _border(
                            (float(first.y) + float(second.y)) / 2.0,
                            float(first.x),
                            float(second.x),
                            max(width, 0.001),
                        ),
                    )
                elif dx <= AXIS_TOLERANCE_POINTS and dy >= MIN_BORDER_LENGTH_POINTS:
                    retain(
                        vertical,
                        _border(
                            (float(first.x) + float(second.x)) / 2.0,
                            float(first.y),
                            float(second.y),
                            max(width, 0.001),
                        ),
                    )
            elif item[0] in {"re", "qu"} and len(item) >= 2:
                rect = item[1]
                if item[0] == "qu":
                    rect = getattr(rect, "rect", None)
                if rect is None:
                    continue
                width = abs(float(rect.x1) - float(rect.x0))
                height = abs(float(rect.y1) - float(rect.y0))
                if (
                    fill_ok
                    and width <= MAX_BORDER_THICKNESS_POINTS
                    and height >= MIN_BORDER_LENGTH_POINTS
                ):
                    retain(
                        vertical,
                        _border(
                            (float(rect.x0) + float(rect.x1)) / 2.0,
                            float(rect.y0),
                            float(rect.y1),
                            max(width, 0.001),
                        ),
                    )
                if (
                    fill_ok
                    and height <= MAX_BORDER_THICKNESS_POINTS
                    and width >= MIN_BORDER_LENGTH_POINTS
                ):
                    retain(
                        horizontal,
                        _border(
                            (float(rect.y0) + float(rect.y1)) / 2.0,
                            float(rect.x0),
                            float(rect.x1),
                            max(height, 0.001),
                        ),
                    )
                if (
                    stroke_ok
                    and 0 <= float(stroke_width) <= MAX_BORDER_THICKNESS_POINTS
                    and width >= MIN_BORDER_LENGTH_POINTS
                    and height >= MIN_BORDER_LENGTH_POINTS
                ):
                    line_width = max(float(stroke_width), 0.001)
                    retain(
                        horizontal,
                        _border(float(rect.y0), float(rect.x0), float(rect.x1), line_width),
                    )
                    retain(
                        horizontal,
                        _border(float(rect.y1), float(rect.x0), float(rect.x1), line_width),
                    )
                    retain(
                        vertical,
                        _border(float(rect.x0), float(rect.y0), float(rect.y1), line_width),
                    )
                    retain(
                        vertical,
                        _border(float(rect.x1), float(rect.y0), float(rect.y1), line_width),
                    )

    def records(values: set[tuple[int, int, int, int]]) -> list[dict[str, int]]:
        return [
            {
                "axis_millipoints": axis,
                "start_millipoints": start,
                "end_millipoints": end,
                "thickness_millipoints": thickness,
            }
            for axis, start, end, thickness in sorted(values)
        ]

    return records(horizontal), records(vertical)


def _token_center(token: dict[str, Any]) -> tuple[int, int]:
    x0, y0, x1, y1 = token["bbox_millipoints"]
    return ((x0 + x1) // 2, (y0 + y1) // 2)


def _unit_edges(
    page: dict[str, Any], left_token: dict[str, Any], right_token: dict[str, Any]
) -> tuple[int, int, int, int, int]:
    _, left_y = _token_center(left_token)
    _, right_y = _token_center(right_token)
    left_start = left_token["bbox_millipoints"][0]
    right_start = right_token["bbox_millipoints"][0]
    center_y = (left_y + right_y) // 2
    candidates = [
        border
        for border in page["vertical_borders"]
        if border["start_millipoints"] - TOKEN_BORDER_TOLERANCE_MILLIPOINTS
        <= center_y
        <= border["end_millipoints"] + TOKEN_BORDER_TOLERANCE_MILLIPOINTS
    ]
    left_candidates = [
        border for border in candidates if border["axis_millipoints"] < left_start
    ]
    divider_candidates = [
        border
        for border in candidates
        if left_start < border["axis_millipoints"] < right_start
    ]
    if not left_candidates or not divider_candidates:
        raise ValueError("table border topology is incomplete")
    left = max(left_candidates, key=lambda border: border["axis_millipoints"])
    divider = max(divider_candidates, key=lambda border: border["axis_millipoints"])
    right_candidates = [
        border
        for border in candidates
        if border["axis_millipoints"] > divider["axis_millipoints"]
    ]
    if not right_candidates:
        raise ValueError("table border topology is incomplete")
    right = min(
        right_candidates,
        key=lambda border: border["axis_millipoints"],
    )
    if not (
        left["axis_millipoints"]
        < divider["axis_millipoints"]
        < right["axis_millipoints"]
    ):
        raise ValueError("table border order is invalid")
    return (
        left["axis_millipoints"],
        divider["axis_millipoints"],
        right["axis_millipoints"],
        min(
            left["start_millipoints"],
            divider["start_millipoints"],
            right["start_millipoints"],
        ),
        max(
            left["end_millipoints"],
            divider["end_millipoints"],
            right["end_millipoints"],
        ),
    )


def derive_topology(case, pages: list[dict[str, Any]]) -> dict[str, Any]:
    expected = expected_token_ids(case)
    expected_set = set(expected)
    token_rows: dict[str, tuple[int, dict[str, Any]]] = {}
    paint_rows: list[tuple[int, str]] = []
    for page in pages:
        page_number = page["number"]
        for token_row in page["tokens"]:
            token_id = token_row["id"]
            if token_id in token_rows:
                raise ValueError(f"duplicate token: {token_id}")
            token_rows[token_id] = (page_number, token_row)
            paint_rows.append((token_row["paint_order"], token_id))
    if set(token_rows) != expected_set:
        missing = sorted(expected_set - set(token_rows))
        extra = sorted(set(token_rows) - expected_set)
        raise ValueError(f"token coverage mismatch: missing={missing}, extra={extra}")
    orders = sorted(order for order, _ in paint_rows)
    if orders != list(range(len(expected))):
        raise ValueError("token paint order is incomplete")

    page_by_number = {page["number"]: page for page in pages}
    unit_count = 20 if case.fragment == "row-boundary" else 26
    placements = []
    page_matches = 0
    y_aligned = 0
    for unit in range(1, unit_count + 1):
        left_id = f"T{case.index:02d}L{unit:02d}"
        right_id = f"T{case.index:02d}R{unit:02d}"
        left_page, left = token_rows[left_id]
        right_page, right = token_rows[right_id]
        if left_page != right_page:
            raise ValueError("paired table tokens span different pages")
        page_matches += 1
        left_y = _token_center(left)[1]
        right_y = _token_center(right)[1]
        if abs(left_y - right_y) <= MAX_TOKEN_PAIR_Y_DELTA_MILLIPOINTS:
            y_aligned += 1
        edges = _unit_edges(page_by_number[left_page], left, right)
        placements.append((unit, left_page, *edges))

    segments = []
    for unit, page_number, left, divider, right, top, bottom in placements:
        key = (page_number, left, divider, right)
        if segments and segments[-1]["_key"] == key:
            segments[-1]["last_unit"] = unit
            segments[-1]["top_millipoints"] = min(
                segments[-1]["top_millipoints"], top
            )
            segments[-1]["bottom_millipoints"] = max(
                segments[-1]["bottom_millipoints"], bottom
            )
            continue
        segments.append(
            {
                "_key": key,
                "first_unit": unit,
                "last_unit": unit,
                "page": page_number,
                "left_millipoints": left,
                "divider_millipoints": divider,
                "right_millipoints": right,
                "top_millipoints": top,
                "bottom_millipoints": bottom,
            }
        )
    for segment in segments:
        del segment["_key"]

    return {
        "expected_tokens": len(expected),
        "observed_tokens": len(token_rows),
        "paired_units": unit_count,
        "pair_page_matches": page_matches,
        "pair_y_aligned": y_aligned,
        "paint_sequence_exact": tuple(
            token_id for _, token_id in sorted(paint_rows)
        )
        == expected,
        "segments": segments,
    }


def extract_pdf(path: Path, case) -> dict[str, Any]:
    if pymupdf is None:
        raise ValueError("PyMuPDF is required for topology extraction")
    payload = _read_bounded_regular_file(path, MAX_PDF_BYTES)
    try:
        document = pymupdf.open(stream=payload, filetype="pdf")
    except Exception as error:
        raise ValueError("PDF could not be opened") from error
    try:
        page_count = len(document)
        if not 1 <= page_count <= MAX_PAGES:
            raise ValueError("PDF page count exceeds the topology contract")
        pages = []
        paint_order = 0
        expected_set = set(expected_token_ids(case))
        for page_index, source_page in enumerate(document):
            width = float(source_page.rect.width)
            height = float(source_page.rect.height)
            if (
                not math.isfinite(width)
                or not math.isfinite(height)
                or not 0 < width <= MAX_PAGE_POINTS
                or not 0 < height <= MAX_PAGE_POINTS
            ):
                raise ValueError("PDF page geometry exceeds the topology contract")
            words = source_page.get_text("words", sort=False)
            if len(words) > MAX_WORDS_PER_PAGE:
                raise ValueError("PDF word count exceeds the topology contract")
            tokens = []
            for word in words:
                if len(word) < 5 or not isinstance(word[4], str):
                    continue
                token_id = word[4]
                if TOKEN_RE.fullmatch(token_id) is None:
                    continue
                if token_id not in expected_set:
                    raise ValueError(f"unexpected synthetic token: {token_id}")
                x0, y0, x1, y1 = (float(value) for value in word[:4])
                if not (0 <= x0 < x1 <= width and 0 <= y0 < y1 <= height):
                    raise ValueError("synthetic token box exceeds page geometry")
                tokens.append(
                    {
                        "id": token_id,
                        "paint_order": paint_order,
                        "bbox_millipoints": [
                            _millipoints(x0),
                            _millipoints(y0),
                            _millipoints(x1),
                            _millipoints(y1),
                        ],
                    }
                )
                paint_order += 1
            drawings = source_page.get_drawings()
            if len(drawings) > MAX_DRAWINGS_PER_PAGE:
                raise ValueError("PDF drawing count exceeds the topology contract")
            horizontal, vertical = _extract_axis_borders(drawings)
            if max(len(horizontal), len(vertical)) > MAX_BORDERS_PER_PAGE:
                raise ValueError("PDF border count exceeds the topology contract")
            pages.append(
                {
                    "number": page_index + 1,
                    "width_millipoints": _millipoints(width),
                    "height_millipoints": _millipoints(height),
                    "tokens": sorted(tokens, key=lambda item: item["id"]),
                    "horizontal_borders": horizontal,
                    "vertical_borders": vertical,
                }
            )
    finally:
        document.close()
    topology = derive_topology(case, pages)
    return {
        "case_id": case.case_id,
        "pdf": {
            "bytes": len(payload),
            "sha256": hashlib.sha256(payload).hexdigest(),
            "pages": len(pages),
        },
        "pages": pages,
        "topology": topology,
    }


def _flatten_tokens(document: dict[str, Any]) -> dict[str, tuple[int, dict[str, Any]]]:
    return {
        token["id"]: (page["number"], token)
        for page in document["pages"]
        for token in page["tokens"]
    }


def compare_document_capture(
    case, candidate: dict[str, Any], reference: dict[str, Any]
) -> dict[str, Any]:
    candidate_tokens = _flatten_tokens(candidate)
    reference_tokens = _flatten_tokens(reference)
    expected = set(expected_token_ids(case))
    if set(candidate_tokens) != expected or set(reference_tokens) != expected:
        raise ValueError("comparison token coverage mismatch")
    page_matches = 0
    max_bbox_delta = 0
    for token_id in sorted(expected):
        candidate_page, candidate_token = candidate_tokens[token_id]
        reference_page, reference_token = reference_tokens[token_id]
        if candidate_page == reference_page:
            page_matches += 1
        max_bbox_delta = max(
            max_bbox_delta,
            *(
                abs(left - right)
                for left, right in zip(
                    candidate_token["bbox_millipoints"],
                    reference_token["bbox_millipoints"],
                    strict=True,
                )
            ),
        )
    candidate_segments = candidate["topology"]["segments"]
    reference_segments = reference["topology"]["segments"]
    partition_keys = ("first_unit", "last_unit", "page")
    geometry_keys = (
        "left_millipoints",
        "divider_millipoints",
        "right_millipoints",
        "top_millipoints",
        "bottom_millipoints",
    )
    partition_exact = len(candidate_segments) == len(reference_segments) and all(
        all(candidate_row[key] == reference_row[key] for key in partition_keys)
        for candidate_row, reference_row in zip(
            candidate_segments, reference_segments, strict=True
        )
    )
    max_edge_delta: int | None = None
    geometry_exact = False
    if partition_exact:
        deltas = [
            abs(candidate_row[key] - reference_row[key])
            for candidate_row, reference_row in zip(
                candidate_segments, reference_segments, strict=True
            )
            for key in geometry_keys
        ]
        max_edge_delta = max(deltas, default=0)
        geometry_exact = max_edge_delta == 0
    normalized_exact = (
        candidate["pages"] == reference["pages"]
        and candidate["topology"] == reference["topology"]
    )
    return {
        "case_id": case.case_id,
        "candidate_pages": len(candidate["pages"]),
        "reference_pages": len(reference["pages"]),
        "matched_tokens": len(expected),
        "token_page_matches": page_matches,
        "max_token_bbox_delta_millipoints": max_bbox_delta,
        "segment_partition_exact": partition_exact,
        "segment_geometry_exact": geometry_exact,
        "max_segment_edge_delta_millipoints": max_edge_delta,
        "normalized_exact": normalized_exact,
    }


def _validate_platform(value: object, label: str) -> None:
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    _require_exact_keys(value, PLATFORM_KEYS, label)
    for key in sorted(PLATFORM_KEYS):
        _safe_text(value[key], f"{label} {key}")


def _validate_producer(value: object) -> None:
    if not isinstance(value, dict):
        raise ValueError("producer must be an object")
    _require_exact_keys(value, PRODUCER_KEYS, "producer")
    name = value["name"]
    if name not in {"rwml", "libreoffice", "microsoft-word"}:
        raise ValueError("producer name is invalid")
    if (
        not isinstance(value["mode"], str)
        or CANONICAL_ID_RE.fullmatch(value["mode"]) is None
    ):
        raise ValueError("producer mode is invalid")
    _safe_text(value["version"], "producer version")
    _sha256(value["identity_sha256"], "producer identity")
    _validate_platform(value["platform"], "producer platform")


def load_producer_metadata(path: Path) -> dict[str, Any]:
    value, _ = _load_json(path, 64 * 1024)
    _validate_producer(value)
    _assert_path_neutral(value)
    return value


def _source_identity(source_revision: str | None) -> tuple[str, bool]:
    import subprocess

    if source_revision is None:
        completed = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            check=False,
            capture_output=True,
            text=True,
        )
        if completed.returncode != 0:
            raise ValueError("source revision is unavailable")
        source_revision = completed.stdout.strip()
    if REVISION_RE.fullmatch(source_revision) is None:
        raise ValueError("source revision must be a full lowercase Git SHA")
    completed = subprocess.run(
        ["git", "status", "--porcelain=v1", "--untracked-files=all"],
        cwd=ROOT,
        check=False,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise ValueError("source dirty state is unavailable")
    return source_revision, bool(completed.stdout)


def _harness_sha256() -> str:
    digest = hashlib.sha256()
    for path in (
        SCRIPT_PATH,
        SCRIPT_PATH.with_name("generate_unequal_table_oracle.py"),
        SCRIPT_PATH.with_name("render_oracle_contract.py"),
    ):
        payload = path.read_bytes()
        name = path.name.encode("ascii")
        digest.update(len(name).to_bytes(8, "little"))
        digest.update(name)
        digest.update(len(payload).to_bytes(8, "little"))
        digest.update(payload)
    return digest.hexdigest()


def _current_platform() -> dict[str, str]:
    return {
        "system": platform.system() or "unknown",
        "release": platform.release() or "unknown",
        "machine": platform.machine() or "unknown",
    }


def _environment(source_revision: str | None) -> dict[str, Any]:
    revision, dirty = _source_identity(source_revision)
    version = getattr(pymupdf, "__version__", "unavailable")
    return {
        "source_revision": revision,
        "source_dirty": dirty,
        "harness_sha256": _harness_sha256(),
        "platform": _current_platform(),
        "tools": [{"name": "pymupdf", "version": str(version)}],
    }


def _limits() -> dict[str, int]:
    return {
        "max_pdf_bytes": MAX_PDF_BYTES,
        "max_pages": MAX_PAGES,
        "max_page_points": MAX_PAGE_POINTS,
        "max_drawings_per_page": MAX_DRAWINGS_PER_PAGE,
        "max_words_per_page": MAX_WORDS_PER_PAGE,
        "max_borders_per_page": MAX_BORDERS_PER_PAGE,
    }


def build_capture_report(
    corpus: CorpusManifest,
    pdf_directory: Path,
    producer: dict[str, Any],
    *,
    source_revision: str | None = None,
) -> dict[str, Any]:
    if corpus.campaign != CAMPAIGN or len(corpus.documents) != len(CASES):
        raise ValueError("manifest is not the unequal-table diagnostic campaign")
    if pdf_directory.is_symlink() or not pdf_directory.is_dir():
        raise ValueError("PDF directory is unavailable or symlinked")
    _validate_producer(producer)
    rows = []
    for document, case in zip(corpus.documents, CASES, strict=True):
        if document.case_id != case.case_id:
            raise ValueError("manifest case order does not match the generator")
        try:
            capture = extract_pdf(pdf_directory / f"{case.case_id}.pdf", case)
        except ValueError as error:
            raise ValueError(f"{case.case_id}: {error}") from error
        capture["input_bytes"] = document.input_bytes
        capture["input_sha256"] = document.sha256
        rows.append(capture)
    report = {
        "schema": CAPTURE_SCHEMA,
        "campaign": corpus.identity(),
        "environment": _environment(source_revision),
        "producer": copy.deepcopy(producer),
        "extractor": {
            "name": EXTRACTOR_NAME,
            "version": EXTRACTOR_VERSION,
            "identity_sha256": hashlib.sha256(SCRIPT_PATH.read_bytes()).hexdigest(),
        },
        "limits": _limits(),
        "documents": rows,
    }
    validate_capture_report(report, corpus)
    return report


def _validate_border(value: object, page_axis_limit: int, page_span_limit: int) -> None:
    if not isinstance(value, dict):
        raise ValueError("border must be an object")
    _require_exact_keys(value, BORDER_KEYS, "border")
    axis = _require_int(
        value["axis_millipoints"], "border axis", maximum=page_axis_limit
    )
    start = _require_int(value["start_millipoints"], "border start")
    end = _require_int(
        value["end_millipoints"], "border end", maximum=page_span_limit
    )
    thickness = _require_int(
        value["thickness_millipoints"],
        "border thickness",
        minimum=1,
        maximum=int(MAX_BORDER_THICKNESS_POINTS * 1000),
    )
    if end - start < int(MIN_BORDER_LENGTH_POINTS * 1000) or axis < 0 or thickness < 1:
        raise ValueError("border geometry is invalid")


def _validate_document_capture(
    value: object, document, case, limits: dict[str, int]
) -> None:
    if not isinstance(value, dict):
        raise ValueError("capture document must be an object")
    _require_exact_keys(value, DOCUMENT_KEYS, "capture document")
    if value["case_id"] != case.case_id:
        raise ValueError("capture case identity mismatch")
    if (
        value["input_bytes"] != document.input_bytes
        or value["input_sha256"] != document.sha256
    ):
        raise ValueError("capture input identity mismatch")
    pdf = value["pdf"]
    if not isinstance(pdf, dict):
        raise ValueError("capture PDF identity must be an object")
    _require_exact_keys(pdf, PDF_KEYS, "capture PDF identity")
    _require_int(pdf["bytes"], "capture PDF bytes", minimum=1, maximum=limits["max_pdf_bytes"])
    _sha256(pdf["sha256"], "capture PDF sha256")
    page_count = _require_int(
        pdf["pages"], "capture PDF pages", minimum=1, maximum=limits["max_pages"]
    )
    pages = value["pages"]
    if not isinstance(pages, list) or len(pages) != page_count:
        raise ValueError("capture page coverage mismatch")
    paint_orders: list[int] = []
    for page_index, page in enumerate(pages, start=1):
        if not isinstance(page, dict):
            raise ValueError("capture page must be an object")
        _require_exact_keys(page, PAGE_KEYS, "capture page")
        if page["number"] != page_index:
            raise ValueError("capture page order is invalid")
        width = _require_int(
            page["width_millipoints"],
            "capture page width",
            minimum=1,
            maximum=limits["max_page_points"] * 1000,
        )
        height = _require_int(
            page["height_millipoints"],
            "capture page height",
            minimum=1,
            maximum=limits["max_page_points"] * 1000,
        )
        tokens = page["tokens"]
        if not isinstance(tokens, list) or len(tokens) > limits["max_words_per_page"]:
            raise ValueError("capture token list is invalid")
        token_ids = []
        for token_row in tokens:
            if not isinstance(token_row, dict):
                raise ValueError("capture token must be an object")
            _require_exact_keys(token_row, TOKEN_KEYS, "capture token")
            token_id = token_row["id"]
            if not isinstance(token_id, str) or TOKEN_RE.fullmatch(token_id) is None:
                raise ValueError("capture token id is invalid")
            token_ids.append(token_id)
            paint_orders.append(
                _require_int(token_row["paint_order"], "capture token paint order")
            )
            bbox = token_row["bbox_millipoints"]
            if (
                not isinstance(bbox, list)
                or len(bbox) != 4
                or any(not isinstance(item, int) or isinstance(item, bool) for item in bbox)
                or not (0 <= bbox[0] < bbox[2] <= width)
                or not (0 <= bbox[1] < bbox[3] <= height)
            ):
                raise ValueError("capture token box is invalid")
        if token_ids != sorted(token_ids) or len(token_ids) != len(set(token_ids)):
            raise ValueError("capture tokens must be sorted and unique")
        for key, axis_limit, span_limit in (
            ("horizontal_borders", height, width),
            ("vertical_borders", width, height),
        ):
            borders = page[key]
            if not isinstance(borders, list) or len(borders) > limits["max_borders_per_page"]:
                raise ValueError("capture border list is invalid")
            for border in borders:
                _validate_border(border, axis_limit, span_limit)
            border_tuples = [tuple(border[item] for item in BORDER_ORDER) for border in borders]
            if border_tuples != sorted(border_tuples) or len(border_tuples) != len(
                set(border_tuples)
            ):
                raise ValueError("capture borders must be sorted and unique")
    if sorted(paint_orders) != list(range(len(paint_orders))):
        raise ValueError("capture token paint order is invalid")
    topology = value["topology"]
    if not isinstance(topology, dict):
        raise ValueError("capture topology must be an object")
    _require_exact_keys(topology, TOPOLOGY_KEYS, "capture topology")
    for segment in topology["segments"]:
        if not isinstance(segment, dict):
            raise ValueError("capture topology segment must be an object")
        _require_exact_keys(segment, SEGMENT_KEYS, "capture topology segment")
    if topology != derive_topology(case, pages):
        raise ValueError("capture topology is inconsistent")


def validate_capture_report(report: dict[str, Any], corpus: CorpusManifest) -> None:
    _require_exact_keys(report, TOP_LEVEL_KEYS, "capture report")
    if report["schema"] != CAPTURE_SCHEMA:
        raise ValueError(f"capture schema must be {CAPTURE_SCHEMA}")
    if report["campaign"] != corpus.identity():
        raise ValueError("capture campaign identity mismatch")
    environment = report["environment"]
    if not isinstance(environment, dict):
        raise ValueError("capture environment must be an object")
    _require_exact_keys(environment, ENVIRONMENT_KEYS, "capture environment")
    if not isinstance(environment["source_revision"], str) or REVISION_RE.fullmatch(
        environment["source_revision"]
    ) is None:
        raise ValueError("capture source revision is invalid")
    if not isinstance(environment["source_dirty"], bool):
        raise ValueError("capture source dirty flag is invalid")
    _sha256(environment["harness_sha256"], "capture harness sha256")
    _validate_platform(environment["platform"], "capture platform")
    tools = environment["tools"]
    if not isinstance(tools, list) or not tools:
        raise ValueError("capture tools are invalid")
    names = []
    for tool in tools:
        if not isinstance(tool, dict):
            raise ValueError("capture tool must be an object")
        _require_exact_keys(tool, TOOL_KEYS, "capture tool")
        name = tool["name"]
        if not isinstance(name, str) or CANONICAL_ID_RE.fullmatch(name) is None:
            raise ValueError("capture tool name is invalid")
        names.append(name)
        _safe_text(tool["version"], "capture tool version")
    if names != sorted(names) or len(names) != len(set(names)):
        raise ValueError("capture tools must be sorted and unique")
    _validate_producer(report["producer"])
    extractor = report["extractor"]
    if not isinstance(extractor, dict):
        raise ValueError("capture extractor must be an object")
    _require_exact_keys(extractor, EXTRACTOR_KEYS, "capture extractor")
    if extractor["name"] != EXTRACTOR_NAME or extractor["version"] != EXTRACTOR_VERSION:
        raise ValueError("capture extractor identity is invalid")
    _sha256(extractor["identity_sha256"], "capture extractor sha256")
    limits = report["limits"]
    if not isinstance(limits, dict):
        raise ValueError("capture limits must be an object")
    _require_exact_keys(limits, LIMIT_KEYS, "capture limits")
    if limits != _limits():
        raise ValueError("capture limits do not match the schema")
    documents = report["documents"]
    if not isinstance(documents, list) or len(documents) != len(corpus.documents):
        raise ValueError("capture document coverage mismatch")
    for row, document, case in zip(documents, corpus.documents, CASES, strict=True):
        _validate_document_capture(row, document, case, limits)
    _assert_path_neutral(report)


def load_capture_report(path: Path, corpus: CorpusManifest) -> dict[str, Any]:
    report, _ = _load_json(path, MAX_REPORT_BYTES)
    validate_capture_report(report, corpus)
    return report


def compare_capture_reports(
    candidate: dict[str, Any], reference: dict[str, Any], corpus: CorpusManifest
) -> dict[str, Any]:
    validate_capture_report(candidate, corpus)
    validate_capture_report(reference, corpus)
    rows = [
        compare_document_capture(case, candidate_row, reference_row)
        for case, candidate_row, reference_row in zip(
            CASES, candidate["documents"], reference["documents"], strict=True
        )
    ]
    summary = {
        "documents": len(rows),
        "candidate_pages": sum(row["candidate_pages"] for row in rows),
        "reference_pages": sum(row["reference_pages"] for row in rows),
        "matched_tokens": sum(row["matched_tokens"] for row in rows),
        "token_page_matches": sum(row["token_page_matches"] for row in rows),
        "segment_partition_exact_documents": sum(
            1 for row in rows if row["segment_partition_exact"]
        ),
        "segment_geometry_exact_documents": sum(
            1 for row in rows if row["segment_geometry_exact"]
        ),
        "normalized_exact_documents": sum(1 for row in rows if row["normalized_exact"]),
        "max_token_bbox_delta_millipoints": max(
            row["max_token_bbox_delta_millipoints"] for row in rows
        ),
        "max_segment_edge_delta_millipoints": max(
            (
                row["max_segment_edge_delta_millipoints"]
                for row in rows
                if row["max_segment_edge_delta_millipoints"] is not None
            ),
            default=None,
        ),
    }
    report = {
        "schema": COMPARISON_SCHEMA,
        "campaign": corpus.identity(),
        "harness_sha256": _harness_sha256(),
        "candidate_capture_sha256": hashlib.sha256(
            _canonical_json(candidate)
        ).hexdigest(),
        "reference_capture_sha256": hashlib.sha256(
            _canonical_json(reference)
        ).hexdigest(),
        "candidate": copy.deepcopy(candidate["producer"]),
        "reference": copy.deepcopy(reference["producer"]),
        "summary": summary,
        "documents": rows,
    }
    _assert_path_neutral(report)
    return report


def validate_comparison_report(
    report: dict[str, Any],
    candidate: dict[str, Any],
    reference: dict[str, Any],
    corpus: CorpusManifest,
) -> None:
    _require_exact_keys(report, COMPARISON_KEYS, "topology comparison")
    if report["schema"] != COMPARISON_SCHEMA:
        raise ValueError(f"comparison schema must be {COMPARISON_SCHEMA}")
    expected = compare_capture_reports(candidate, reference, corpus)
    if report != expected:
        raise ValueError("topology comparison is inconsistent with its captures")


def load_comparison_report(
    path: Path,
    candidate: dict[str, Any],
    reference: dict[str, Any],
    corpus: CorpusManifest,
) -> dict[str, Any]:
    report, _ = _load_json(path, MAX_REPORT_BYTES)
    validate_comparison_report(report, candidate, reference, corpus)
    return report


def _write_json(path: Path, value: object) -> None:
    payload = _canonical_json(value)
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="Extract or compare unequal-table topology evidence."
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    extract_parser = subparsers.add_parser("extract")
    extract_parser.add_argument("--manifest", type=Path, required=True)
    extract_parser.add_argument("--pdf-dir", type=Path, required=True)
    extract_parser.add_argument("--producer-metadata", type=Path, required=True)
    extract_parser.add_argument("--source-revision")
    extract_parser.add_argument("--output", type=Path, required=True)

    compare_parser = subparsers.add_parser("compare")
    compare_parser.add_argument("--manifest", type=Path, required=True)
    compare_parser.add_argument("--candidate", type=Path, required=True)
    compare_parser.add_argument("--reference", type=Path, required=True)
    compare_parser.add_argument("--output", type=Path, required=True)
    compare_parser.add_argument("--require-normalized-exact", action="store_true")

    validate_parser = subparsers.add_parser("validate")
    validate_parser.add_argument("--manifest", type=Path, required=True)
    validate_parser.add_argument("--capture", type=Path, required=True)

    validate_comparison_parser = subparsers.add_parser("validate-comparison")
    validate_comparison_parser.add_argument("--manifest", type=Path, required=True)
    validate_comparison_parser.add_argument("--candidate", type=Path, required=True)
    validate_comparison_parser.add_argument("--reference", type=Path, required=True)
    validate_comparison_parser.add_argument("--comparison", type=Path, required=True)

    args = parser.parse_args(argv)
    try:
        corpus = load_corpus_manifest(args.manifest)
        if args.command == "extract":
            producer = load_producer_metadata(args.producer_metadata)
            report = build_capture_report(
                corpus,
                args.pdf_dir,
                producer,
                source_revision=args.source_revision,
            )
            _write_json(args.output, report)
            return 0
        if args.command == "validate":
            report = load_capture_report(args.capture, corpus)
            print(
                json.dumps(
                    {
                        "schema": report["schema"],
                        "campaign": report["campaign"],
                        "producer": report["producer"],
                        "documents": len(report["documents"]),
                    },
                    ensure_ascii=True,
                    sort_keys=True,
                    separators=(",", ":"),
                )
            )
            return 0
        if args.command == "validate-comparison":
            candidate = load_capture_report(args.candidate, corpus)
            reference = load_capture_report(args.reference, corpus)
            report = load_comparison_report(
                args.comparison, candidate, reference, corpus
            )
            print(
                json.dumps(
                    {
                        "schema": report["schema"],
                        "campaign": report["campaign"],
                        "summary": report["summary"],
                    },
                    ensure_ascii=True,
                    sort_keys=True,
                    separators=(",", ":"),
                )
            )
            return 0
        candidate = load_capture_report(args.candidate, corpus)
        reference = load_capture_report(args.reference, corpus)
        comparison = compare_capture_reports(candidate, reference, corpus)
        _write_json(args.output, comparison)
        if (
            args.require_normalized_exact
            and comparison["summary"]["normalized_exact_documents"]
            != comparison["summary"]["documents"]
        ):
            return 2
        return 0
    except (OSError, ValueError) as error:
        print(f"table_oracle_topology: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
